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

## Architecture Deep-Dive

For a comprehensive breakdown of the systems engineering decisions—including deadlock prevention strategies, the LIFO memory arena, and why the asynchronous Tick Thread is used over standard `const fn` arrays—please read the full Architecture & Design Document.
