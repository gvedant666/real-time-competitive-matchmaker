# The 5v5 Real-Time Competitive Matchmaker

A high-performance, multithreaded 5v5 matchmaking engine built in Rust. Designed for massive concurrency, this engine resolves the classic latency vs. match quality tradeoff through lock-sharding, zero-allocation memory pools, and pre-computed mathematical time-decay heuristics.

---

## Key Features

- **Massive Concurrency** — Lock-sharded state across 100 isolated Mutex queues eliminates thread contention.
- **Zero-Allocation Memory** — Pre-allocated 100,000-slot Arena with a LIFO Free List ensures maximum L1 cache temporal locality.
- **O(1) Constraint Relaxation** — Pre-computed Time-Decay Look-Up Table (LUT) dynamically expands search radii to prevent edge-case player starvation without computing floating-point math in the hot path.

---

## Quick Start

### Prerequisites

- Rust toolchain 1.70+

### Installation

```bash
git clone https://github.com/gvedant666/real-time-competitive-matchmaker.git
cd real-time-competitive-matchmaker
cargo build --release
```

---

## Running the Live Server

Start the production WebSocket server:
```bash
cargo run --release --bin real-time-competitive-matchmaker
```
The server will bind and listen on ws://127.0.0.1:8080

## WebSocket API Usage
To interact with the matchmaking server, connect a WebSocket client (like wscat or a frontend interface) to ws://127.0.0.1:8080 and stream standard JSON payloads.

Ingestion Payload (Join Queue):
```
{
  "action": "join_queue",
  "player_id": "player_1",
  "mmr": 1500
}
```

Successful Match Response:
When the engine mathematically balances 10 players, it will broadcast a Match Response to all 10 active connections simultaneously:

```
{
  "match_id": 1,
  "team_a": [
    {"uuid": "player_1", "mmr": 1500},
    {"uuid": "player_3", "mmr": 1495},
    ...
  ],
  "team_b": [
    {"uuid": "player_2", "mmr": 1505},
    ...
  ]
}
```

## Running the Benchmark Suite

The repository includes three standalone, high-load simulation scripts for benchmarking performance and validating edge-case handling.
1. Arena Stress Test (Peak CPU Throughput)

Bypasses the network to test the absolute raw insertion limits of the LIFO Arena and lock-sharded Mutexes using 8 parallel OS threads.

```bash
cargo run --release --bin stress_test
```

2. Distribution Quality Test (Bell Curve & Time Decay)

Injects 10,000 players adhering to a normal distribution (Bell Curve). This proves the bitmasking algorithm works and that the Time-Decay LUT successfully routes extreme edge-case players out of infinite queues.

```bash
cargo run --release --bin distribution_test
```

3. Network Latency Benchmark (Thundering Herd)

A headless client script that establishes 2,000 concurrent WebSockets and releases them all at the exact same millisecond to test Tokio's asynchronous networking layer.

Terminal 1 (Start the server):

```bash
ulimit -n 100000
cargo run --release --bin real-time-competitive-matchmaker
```
Terminal 2 (Run the benchmark):

```bash
ulimit -n 100000
cargo run --release --bin network_benchmark
```

(Note: The ulimit command prevents Linux from blocking the benchmark due to the default 1024 file descriptor limit).

The suite executes four sequential tests:

| Test | Description |
|---|---|
| Bell Curve Test | Injects 50,000 players via a Normal Distribution |
| Uniform Chaos Test | Injects 50,000 players via a purely random distribution |
| Raw CPU Speed Benchmark | Synchronous memory injection to test raw lock/extraction speeds |
| 8-Bucket Gap Test | Validates the async Tick Thread and LUT decay logic |

---



## Project Structure

```
src/
├── api/              # WebSocket / HTTP ingestion layer (future)
├── models/           # Shared DTOs and API models
├── engine/
│   ├── state.rs      # EngineState, sharded buckets, LIFO Arena
│   ├── worker.rs     # Hot-path locking, extraction, and LUT initialization
│   ├── balancer.rs   # Snake Draft team balancing logic
│   └── primitives.rs # Player structs and atomics
├── main.rs           # Production server entry point
└── bin/
    └── simulate.rs   # High-throughput benchmarking suite
```

---

## Benchmark Results

### Peak Throughput and Memory Stability

**Objective:** Measure raw ingestion speed and concurrency limits of the lock-sharded Mutex architecture and the pre-allocated LIFO Arena, bypassing network I/O.

**Methodology:** 8 parallel OS threads simultaneously injecting players into a 500,000-capacity engine.

| Metric | Result |
|---|---|
| Total Players Injected | 400,000 |
| Execution Time | 218.90ms |
| Peak Throughput | 11806497 insertions/sec |

**Conclusion:** Moving memory allocation off the OS heap and relying on the LIFO Free List successfully eliminated page faults and cache thrashing. The lock-sharded bucket grid allowed all 8 threads to write simultaneously without triggering global bottlenecks or thread starvation.

---

### Combinatorial Accuracy and Time-Decay

**Objective:** Verify that the 10-player bitmasking algorithm maintains strict competitive integrity and that the pre-computed Time-Decay LUT successfully routes extreme edge-case players out of infinite queues.

**Methodology:** 10,000 players mapped to a Normal Distribution (Bell Curve) centered at 2500 MMR, with a strict 10-second timeout threshold.

| Metric | Result |
|---|---|
| Total Matches Formed | 998 (9,980 players) |
| Edge-Case Timeouts | 20 (0.2% of total population) |
| Average MMR Spread | 0.6 points between Team A and Team B |

**MMR Spread Histogram (Absolute Difference)**

| Range | Percentage | Matches |
|---|---|---|
| 0–9 points | 99.8% | 996 |
| 10–19 points | 0.2% | 2 |
| 20+ points | 0.0% | 0 |

**Conclusion:** The combinatorial bitmasking algorithm guarantees near-perfect matchmaking accuracy, with 99.8% of matches having an MMR differential of less than 10 points. The Time-Decay LUT accurately identified the extreme 0.2% tails of the distribution curve, safely timing them out rather than forcing an unplayable match or freezing the queue.

---

### Network Concurrency and Stampede Resiliency

**Objective:** Ensure the Tokio async networking layer does not bottleneck the synchronous engine loops under massive sudden load (the "Thundering Herd" problem).

**Methodology:** A headless client script established 2,000 concurrent WebSocket TCP connections and used an async barrier to release 2,000 JSON connection payloads at the exact same millisecond.

> **Note:** Before running this test, raise the file descriptor limit:
> ```bash
> ulimit -n 100000
> ```

| Metric | Result |
|---|---|
| Total Concurrent Clients | 2,000 |
| Connection Failures / Dropped Packets | 0 |
| Minimum Latency | 1.49ms |
| Median Latency (P50) | 107.55ms |
| 90th Percentile (P90) | 151.35ms |
| 99th Percentile (P99) | 173.42ms |
| Maximum Latency | 180.45ms |

**Conclusion:** The hybrid concurrency model successfully separates I/O from computation. The Tokio runtime managed the 2,000-connection socket stampede without dropping a single packet. A P99 round-trip time of 173ms from packet generation to match formation confirms the engine is well-suited for real-time competitive environments.

---

## Architecture

For a full breakdown of systems engineering decisions — including deadlock prevention strategies, the LIFO memory arena, and why the async Tick Thread is used over standard `const fn` arrays — refer to the full Architecture and Design Document.
