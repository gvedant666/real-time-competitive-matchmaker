# The 5v5 Real-Time Competitive Matchmaker

A high-performance, multithreaded 5v5 matchmaking engine built in Rust. Designed for massive concurrency, this engine solves the classic Latency vs. Match Quality conflict by utilizing lock-sharding, zero-allocation memory pools, and pre-computed mathematical time-decay heuristics.

## Key Features

* **Massive Concurrency:** Lock-sharded state across 100 isolated Mutex queues eliminates thread contention.
* **Zero-Allocation Memory:** Pre-allocated 100,000-slot Arena with a LIFO Free List ensures maximum L1 cache temporal locality.
* **O(1) Constraint Relaxation:** Mathematical time-decay Look-Up Table (LUT) dynamically expands search radii to prevent edge-case player starvation without computing floating-point math in the hot path.

## Quick Start

### Prerequisites

* Rust toolchain (1.70+)

### Installation

Clone the repository and build the project:

```bash
git clone https://github.com/gvedant666/real-time-competitive-matchmaker.git
cd real-time-competitive-matchmaker
cargo build --release
```

## Running the Simulation

This repository includes a standalone, heavy-load simulation suite to benchmark performance and validate edge-case handling. To run the suite:

```bash
cargo run --release --bin simulate
```

The simulation will execute four sequential tests:

* **Bell Curve Test:** Injects 50,000 players via a Normal Distribution.
* **Uniform Chaos Test:** Injects 50,000 players via a purely random distribution.
* **Raw CPU Speed Benchmark:** Synchronous memory injection to test raw lock/extraction speeds.
* **8-Bucket Gap Test:** Validates the asynchronous Tick Thread and LUT decay logic.

## Project Structure

```plaintext
src/
├── api/          # WebSocket / HTTP ingestion layer (Future)
├── models/       # Shared DTOs and API models
├── engine/       # Core Matchmaking Logic
│   ├── state.rs      # EngineState, Sharded Buckets, LIFO Arena
│   ├── worker.rs     # Hot-path locking, extraction, and LUT initialization
│   ├── balancer.rs   # Snake Draft team balancing logic
│   └── primitives.rs # Player structs and atomics
├── main.rs       # Production Server Entry Point
└── bin/
    └── simulate.rs   # High-throughput benchmarking suite
```

## 6.1 Peak Throughput & Memory Stability (The Arena Stress Test)
Objective: Measure the raw ingestion speed and concurrency limits of the lock-sharded Mutex architecture and the pre-allocated LIFO Arena, bypassing network I/O. Methodology: 8 parallel OS threads simultaneously injecting players into a 500,000-capacity engine.
Total Players Injected: 400,000
Execution Time: 218.90ms
Peak Throughput: 1,827,348 insertions/sec
Conclusion: Moving memory allocation off the OS heap and relying on the LIFO Free List successfully eliminated page faults and cache thrashing. The lock-sharded bucket grid allowed all 8 threads to write simultaneously without triggering global bottlenecks or thread starvation.
6.2 Combinatorial Accuracy & Time-Decay (The Distribution Test)
Objective: Verify that the 10-player bitmasking algorithm maintains strict competitive integrity and that the pre-computed Time-Decay LUT successfully routes extreme edge-case players out of infinite queues. Methodology: Ingesting 10,000 players mapped to a standard Normal Distribution (Bell Curve) centered at 2500 MMR, with a strict 10-second timeout threshold.
Total Matches Formed: 998 (9,980 players)
Edge-Case Timeouts: 20 (0.2% of total population)
Average MMR Spread: 0.6 points between Team A and Team B
MMR Spread Histogram (Absolute Difference)
00-09 Points: 99.8% (996 matches)
10-19 Points: 0.2% (2 matches)
20+ Points: 0.0% (0 matches)
Conclusion: The combinatorial bitmasking algorithm guarantees near-perfect matchmaking accuracy, with 99.8% of matches having an MMR differential of less than 10 points. The Time-Decay LUT accurately identified the extreme 0.2% tails of the distribution curve, safely timing them out rather than forcing an unplayable match or freezing the queue.
6.3 Network Concurrency & Stampede Resiliency (The Latency Test)

Note : Before testing increase fd limit \Bash command  $ ulimit -n 100000
Objective: Ensure the Tokio asynchronous networking layer does not bottleneck the synchronous engine loops under massive sudden load (the "Thundering Herd" problem). Methodology: A headless client script established 2,000 concurrent WebSocket TCP connections and used an asynchronous barrier to release 2,000 JSON connection payloads at the exact same millisecond.
Total Concurrent Clients: 2,000
Connection Failures/Dropped Packets: 0
Minimum Latency: 1.49ms
Median Latency (P50): 107.55ms
90th Percentile (P90): 151.35ms
99th Percentile (P99): 173.42ms
Maximum Latency: 180.45ms
Conclusion: The hybrid concurrency model successfully separates I/O from computation. The Tokio runtime effortlessly managed the 2,000-connection socket stampede without dropping a single packet. A P99 round-trip time of 173ms from packet generation to match formation confirms the engine is highly responsive and perfectly suited for real-time competitive environments.


## Architecture Deep-Dive

For a comprehensive breakdown of the systems engineering decisions—including deadlock prevention strategies, the LIFO memory arena, and why the asynchronous Tick Thread is used over standard `const fn` arrays—please read the full Architecture & Design Document.
