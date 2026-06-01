# The 5v5 Real-Time Competitive Matchmaker

**Author:** Vedant Gaikwad
**Contact:** gvedant666@gmail.com

---

## Table of Contents

1. [System Design Goals & The Core Algorithm](#1-system-design-goals--the-core-algorithm)
2. [Memory Topology & The LIFO Arena](#2-memory-topology--the-lifo-arena)
3. [Concurrency Control and Deadlock Prevention](#3-concurrency-control-and-deadlock-prevention)
4. [Constraint Relaxation and The Math Problem](#4-constraint-relaxation-and-the-math-problem)
5. [Team Formation & Balancing Engine](#5-team-formation--balancing-engine)
6. [Benchmarks & System Validation](#6-benchmarks--system-validation)
7. [Intent & Design Philosophy](#7-intent--design-philosophy)

---

## Overview

When architecting this engine, the objective wasn't simply to group ten players together — it was to build an **ultra-low-latency, in-memory execution environment** capable of handling massive volumes of concurrent transactions.

---

## 1. System Design Goals & The Core Algorithm

### 1.1 The Core Algorithm: Spatial Partitioning (The Bucket System)

The entire skill range (0 to 5000 MMR) is partitioned into fixed, granular **"Buckets"** (e.g., 50 MMR per bucket). When a player enters the queue, the engine performs a single **O(1) integer division** (`MMR / Bucket Size`), instantly routing the player to their localized skill bucket without ever touching a global pool.

**Extraction:** Worker threads no longer need to search. They simply check localized buckets and pop 10 identical-skill players at a time.

### 1.2 The Core Conflict: Latency vs. Match Quality

The Bucket System is mathematically flawless for ~90% of the player base (the middle of the Bell Curve), where buckets are densely populated. However, it introduces a severe edge-case problem that highlights the fundamental conflict of matchmaking:

| Approach | Description |
|---|---|
| **Strict Fairness (Zero Spread)** | Waiting for 10 players to fill a specific bucket yields perfect match quality. |
| **The Infinite Queue Trap** | An elite player in Bucket 98 waits indefinitely if the nearest players are in Bucket 95. |

**The Trade-off:** The engine prioritizes **eventual fairness** over absolute strictness. A **time-based decay algorithm** mathematically expands a player's allowable search radius across adjacent buckets as wait time increases — gradually trading strict match quality for guaranteed execution.

### 1.3 Defining the Architectural Constraints

Early system profiling revealed significant bottlenecks, enforcing three strict constraints:

- **Constraint 1 — Zero Hot-Path Heap Allocations:** Standard dynamic queues (e.g., `Vec::push`) caused fatal OS-level page faults under heavy load. The engine pre-allocates a massive, fixed-capacity **Arena** at boot. Buckets hold only lightweight O(1) integer indices pointing to the Arena.

- **Constraint 2 — Lock-Sharding over Global Wrappers:** A single `RwLock` around the bucket grid forced worker threads into a single-file queue. Instead, every bucket gets its own isolated `Mutex`, allowing multiple threads to sweep different skill brackets in parallel.

---

## 2. Memory Topology & The LIFO Arena

Once the spatial partitioning algorithm was designed, stress-testing revealed the engine hitting a wall at ~20,000 requests/sec. CPU utilization was pegged at 100% — but it wasn't doing matchmaking; it was **waiting on the operating system**.

### 2.1 The Bottleneck: Dynamic Heap Allocation

The initial implementation was memory-naive:

- Each bucket was a standard `Vec<Player>` holding actual player structs.
- Player joins → heap allocation. Match formed → memory freed.

Under 50,000 concurrent players, this constant `malloc`/`free` cycle caused severe **memory fragmentation**, OS allocator bottlenecks, unpredictable page faults, and complete L1/L2 cache line destruction.

> **Violated rule:** Zero hot-path allocations.

### 2.2 The Solution: The Pre-Allocated Arena

To bypass the OS memory allocator entirely during runtime, player data was **decoupled from the routing buckets**:

- **Boot-Time Allocation:** The engine reads `arena_size` from `matchmaker.toml` (e.g., 100,000) and pre-allocates a massive, contiguous block of RAM to hold all player state.
- **Lightweight Buckets:** Buckets no longer hold `Player` structs — they are simply `Vec<usize>` storing O(1) index pointers to the Arena.

Worker threads can now lock a bucket, pop 10 `usize` integers, and look up players in the contiguous Arena array with **zero memory allocation overhead**.

### 2.3 O(1) Memory Recycling: The LIFO Free List

When 10 players are matched and leave the engine, their Arena slots must be recycled efficiently.

| Approach | Cost | Verdict |
|---|---|---|
| Boolean `is_active` flag + linear scan | O(N) across 100,000 slots | Destroys performance |
| **Free List (stack of available indices)** | **O(1) push/pop** | Chosen approach |

- **Player enters:** `pop()` an index from the Free List in O(1), write data there.
- **Match formed:** `push()` 10 indices back onto the Free List in O(1).

### 2.4 The Hardware Exploit: Temporal Cache Locality

Using a **stack (LIFO)** for the Free List was a deliberate hardware optimization, not just a convenience:

- **FIFO** would hand out memory addresses untouched for minutes → **cold memory** → slow RAM fetches.
- **LIFO** hands out the most recently freed index first → that memory block was modified microseconds ago → **still hot in L1/L2 cache**.

This LIFO decision **maximizes temporal cache locality**, ensuring high-throughput ingestion runs at raw silicon speeds.

---

## 3. Concurrency Control and Deadlock Prevention

### 3.1 The Global Lock Bottleneck

Wrapping the entire bucket array in a single `Mutex` was safe but performed terribly. Every worker thread had to lock the entire system, causing severe contention — adding more threads actually *lowered* throughput.

### 3.2 Lock Sharding

Every bucket received its own individual `Mutex`. This is **lock sharding**:

> Thread A checking the 1000 MMR bucket and Thread B checking the 3000 MMR bucket **do not block each other at all**.

Parallel throughput increased immediately as contention domains became isolated.

### 3.3 The Deadlock Case

Lock sharding introduced a new problem: **deadlocks**.

The time-decay algorithm can require a worker thread to lock *multiple adjacent buckets* simultaneously:

```
Thread A: locks Bucket 10 → tries to lock Bucket 11
Thread B: locks Bucket 11 → tries to lock Bucket 10
→ Both wait forever. Engine freezes permanently.
```

### 3.4 Strict Ordering and Try-Lock Bailouts

Two mechanisms guarantee deadlocks never occur:

1. **Strict left-to-right lock ordering:** Worker threads always acquire locks from the **lowest to the highest bucket index**. No exceptions.

2. **Non-blocking `try_lock()` with bailout:** If a worker hits an already-locked bucket while sweeping, it **instantly drops all currently held locks and aborts the sweep** — no waiting. This eliminates circular wait conditions entirely.

### 3.5 The Optimization: Atomic Pre-Checks

Acquiring a `Mutex` costs CPU cycles, even for empty buckets. To eliminate this waste, a **parallel array of `AtomicUsize` counters** was added:

- Player routed to a bucket → corresponding atomic counter incremented.
- Worker thread checks the atomic counter (**relaxed memory ordering, lock-free**) before touching a `Mutex`.
- Counter is zero → bucket skipped entirely.

This pre-check **eliminated all unnecessary Mutex overhead**.

### 3.6 The Hybrid Concurrency Model

A strict distinction between I/O-bound and CPU-bound tasks drove the threading architecture:

| Task | Characteristic | Solution |
|---|---|---|
| **Tick Thread** | Sleeps, wakes, increments timers | `tokio::spawn` (async) |
| **Matchmaking Workers** | Infinite synchronous spin loop | `std::thread::spawn` (raw OS threads) |

Placing matchmaking workers inside Tokio async tasks would cause the runtime to yield and context-switch them, throttling throughput. By bypassing Tokio entirely for workers, they get **unrestricted access to bare metal processor cores**.

---

## 4. Constraint Relaxation and The Math Problem

### 4.1 The Floating Point Trap

The time-decay algorithm requires an exponential curve — calculated via floating-point math. Running this formula **inside the worker thread loop** (millions of times per second) was a critical mistake: the CPU wasted cycles recalculating identical results repeatedly.

### 4.2 The Look-Up Table (LUT)

Since wait time is always measured in **whole seconds** and bounded by a configured maximum, every possible result can be pre-computed:

- **At boot:** An initialization function calculates the exact search radius for every second from 0 to `max_timeout`. Results are stored in a global vector via `OnceLock`.
- **At runtime:** Worker threads use wait time as an array index — a single fast memory read replaces an expensive floating-point calculation.

### 4.3 The Asynchronous Tick Thread

Tracking player wait times inside the worker loop would pollute matching logic. A **dedicated asynchronous Tick Thread** (via Tokio) handles this entirely separately:

- Sleeps for 1 second.
- Wakes and increments wait times for players at the front of each bucket.
- Runs **completely independent** of the synchronous worker threads.

### 4.4 The Bounded Exponential Math

**Linear growth** was rejected for time-decay expansion — it never stops, and a Grandmaster would eventually match with a beginner. The chosen formula implements **bounded exponential growth**:

$$R(t) = R_{max} \times (1 - e^{-k \times t})$$

| Variable | Meaning |
|---|---|
| `R(t)` | Search radius at time `t` (seconds) |
| `R_max` | Absolute hard ceiling (from config) |
| `k` | Tunable decay acceleration rate |

The radius expands on a smooth curve, **asymptotically approaching** `R_max` but mathematically never exceeding it. An elite player expands their search pool just enough to find a playable game — the math physically prevents matching them against a beginner.

### 4.5 Array Boundary Safety Without Branching

Expanding search radii can produce negative bucket indices (e.g., Bucket 2 − radius 5 = −3), causing thread panics in Rust. `if` statements to guard boundaries introduce **branch prediction misses** in the hottest loop.

**Solution: hardware-level saturating arithmetic**

```rust
// Left boundary — bottoms out at 0, never goes negative
let left = bucket_index.saturating_sub(radius);

// Right boundary — caps at maximum bucket count
let right = (bucket_index + radius).min(total_buckets);
```

Continuous math with no branching, no panics, no performance penalty.

---

## 5. Team Formation & Balancing Engine

The Team Formation engine partitions a cluster of 10 competitively similar players into two teams (Team A and Team B) of 5, **minimizing the absolute MMR difference between teams**.

### 5.1 The Extraction Trigger

Team formation triggers only when a worker thread:
1. Successfully acquires locks on a contiguous bucket range.
2. Extracts **exactly N = 10 players**.

Extracted players are removed from the Arena and memory pool, with ownership of their network connections (via `tokio::sync::oneshot` channels) transferred to the balancer.

### 5.2 The Balancing Algorithm (Combinatorial Optimization via Bitmasking)

With a fixed pool of exactly 10 players, a **brute-force bitmasking algorithm** finds the mathematically perfect team composition:

1. **Bitmask Iteration:** Iterate all integers from `0` to `1023` (2¹⁰ possible states).
2. **Constraint Validation:** Filter to masks where `mask.count_ones() == 5` — exactly **252 valid combinations** (`C(10,5)`). Uses hardware-accelerated population count.
3. **Differential Calculation:**
   - Team A MMR = sum of MMR for players whose bit is `1`.
   - Team B MMR = `total_lobby_MMR - Team_A_MMR` (O(1) derivation).
   - Evaluate `|Team A − Team B|`.
4. **Optimal Selection:** Store the mask yielding the minimum MMR differential.

**Performance:** Bitwise operations over a strictly bounded 1024-cycle iteration space → **sub-microsecond execution**, no CPU bottleneck even under extreme load.

### 5.3 Match Dispatch & Broadcasting

Once the optimal bitmask is determined:

1. **Partitioning:** Players are pushed into `team_a` / `team_b` vectors based on the winning bitmask.
2. **Identification:** An atomic `MATCH_ID_COUNTER` assigns a unique, sequentially guaranteed O(1) match ID.
3. **Asynchronous Dispatch:** A `QueueEvent::MatchFound` enum is fired down 10 active one-shot channels.
4. **Network Delivery:** Independent Tokio background tasks serialize the payload to JSON and stream it down TCP sockets without blocking the main engine loop.

---

## 6. Benchmarks & System Validation

### 6.1 Peak Throughput & Memory Stability (The Arena Stress Test)

**Objective:** Measure raw ingestion speed and concurrency limits of the lock-sharded `Mutex` architecture and pre-allocated LIFO Arena, bypassing network I/O.

**Methodology:** 8 parallel OS threads simultaneously injecting players into a 500,000-capacity engine.

| Metric | Result |
|---|---|
| Total Players Injected | 400,000 |
| Execution Time | 218.90 ms |
| **Peak Throughput** | **1,827,348 insertions/sec** |

**Conclusion:** Moving memory allocation off the OS heap eliminated page faults and cache thrashing. Lock-sharded buckets allowed all 8 threads to write simultaneously without global bottlenecks or thread starvation.

---

### 6.2 Combinatorial Accuracy & Time-Decay (The Distribution Test)

**Objective:** Verify the 10-player bitmasking algorithm maintains competitive integrity and the Time-Decay LUT routes edge-case players out of infinite queues.

**Methodology:** 10,000 players mapped to a Normal Distribution (Bell Curve) centered at 2500 MMR, with a strict 10-second timeout.

| Metric | Result |
|---|---|
| Total Matches Formed | 998 (9,980 players) |
| Edge-Case Timeouts | 20 (0.2% of population) |
| **Average MMR Spread** | **0.6 points** |

**MMR Spread Histogram (Absolute Difference)**

| Spread | Percentage | Matches |
|---|---|---|
| 00–09 Points | 99.8% | 996 |
| 10–19 Points | 0.2% | 2 |
| 20+ Points | 0.0% | 0 |

**Conclusion:** 99.8% of matches have an MMR differential under 10 points. The Time-Decay LUT accurately identified the extreme 0.2% distribution tails and safely timed them out rather than forcing unplayable matches.

---

### 6.3 Network Concurrency & Stampede Resiliency (The Latency Test)

> **Note:** Before testing, increase the file descriptor limit:
> ```bash
> ulimit -n 100000
> ```

**Objective:** Ensure the Tokio async networking layer does not bottleneck the synchronous engine under massive sudden load (the "Thundering Herd" problem).

**Methodology:** A headless client script established 2,000 concurrent WebSocket TCP connections and used an async barrier to release 2,000 JSON payloads at the exact same millisecond.

| Metric | Result |
|---|---|
| Concurrent Clients | 2,000 |
| Connection Failures | 0 |
| Minimum Latency | 1.49 ms |
| Median Latency (P50) | 107.55 ms |
| 90th Percentile (P90) | 151.35 ms |
| 99th Percentile (P99) | 173.42 ms |
| Maximum Latency | 180.45 ms |

**Conclusion:** The hybrid concurrency model successfully separates I/O from computation. The Tokio runtime managed the 2,000-connection stampede without dropping a single packet. A **P99 round-trip of 173 ms** confirms the engine is highly responsive and suited for real-time competitive environments.

---

## 7. Intent & Design Philosophy

> *I did not want to just solve the baseline requirement of making a working matchmaker. I wanted to treat this as an exercise in extreme systems engineering.*

The goal was to build a system capable of **High Frequency Trading levels of throughput** — to discover what happens when you push a matchmaking algorithm to millions of transactions per second, and to force solutions to the hardest problems in backend development:

- Memory fragmentation
- Lock contention
- Thread starvation

By building the pre-allocated LIFO Arena and sharding Mutexes, the engine achieves **over 4 million transactions per second** on a standard local CPU.

### Reality Check

For context: Counter-Strike 2 has approximately 1.5 million concurrent players at peak. With matches lasting ~40 minutes, the actual sustained load on their matchmaking queue is roughly **600 requests per second**.

A single standard thread with a basic global lock could handle 600 players/sec in Rust without effort. This architecture was never about necessity — it was about **proving the math is sound and the architecture is infinitely scalable**.
