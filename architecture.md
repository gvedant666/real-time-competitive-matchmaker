# The 5v5 Real-Time Competitive Matchmaker — Architecture & Design

**Author:** Vedant Gaikwad  
**Contact:** gvedant666@gmail.com

---

## Overview

The goal wasn't just to make matchmaking work — it was to make it work fast, at scale, without falling apart under load. This doc walks through the decisions I made, why I made them, and honestly some of the things I got wrong before I got them right.

---

## 1. The Core Algorithm — How Do You Even Start?

The first question was: how do you find compatible players quickly without scanning everyone in the queue?

The answer I landed on is spatial partitioning. The full MMR range (0–5000) gets divided into fixed-size buckets — 50 MMR wide by default, so 101 buckets total. When a player joins, a single integer division (`mmr / bucket_size`) tells you exactly which bucket they go into. No searching, no sorting, just a direct index.

Worker threads then sweep these buckets looking for 10 players to form a match. If bucket 47 has 10+ players, pull them out and you're done.

This works great for the bulk of players clustered around average MMR. The problem shows up at the extremes — an elite player sitting alone in bucket 97 never gets a match because there's nobody else in bucket 97. That's the core tension in matchmaking: strict quality vs. actually getting a game.

### The tradeoff

I thought about a few approaches here:

- **Pure strict matching** — only match within the same bucket. Perfect quality, but the top and bottom 5% of players wait forever.
- **Global pool with sorting** — throw everyone in one queue, sort by MMR, try to form balanced groups. Simple but doesn't scale — you're touching the whole queue on every sweep.
- **Time-based relaxation** — start strict, loosen the radius as wait time increases. This is what I went with.

The third option means most players get high quality matches quickly, and edge-case players gradually see their search widen until something is found. Worst case they time out after 5 minutes, which is better than waiting forever.

---

## 2. The Memory Problem — This Is Where It Got Interesting

Once the bucket system was working I ran a basic load test. Around 20,000 inserts/sec the throughput just... stopped improving. CPU was at 100% but the matchmaking wasn't the bottleneck — the allocator was.

The original code had each bucket holding a `Vec<Player>` with actual player structs. Every join was a heap allocation. Every match formed was a bunch of frees. At high volume this causes memory fragmentation and the OS allocator becomes a serialization point. I was basically asking the kernel for memory on every single request.

### The arena

The fix is to allocate all player memory upfront at startup and never touch the heap again during runtime.

At boot the engine creates one big `Vec<Option<Player>>` — the arena — sized to however many concurrent players you expect (`arena_size` in the config). Alongside it lives a free list: a stack of indices pointing to empty slots.

When a player joins: pop an index off the free list, write the player there, hand the index to their bucket. When a match forms: take the players out by index, push those indices back onto the free list. Zero allocations, zero frees, just writes into a pre-existing block of memory.

Buckets store `usize` indices now, not player structs. Holding a bucket lock and popping 10 integers is nearly instant.

### Why a stack and not a queue for the free list

This is a small detail but I think it matters. A FIFO free list hands out the oldest freed slots first — those haven't been touched in a while and are cold in cache. A LIFO stack hands out the most recently freed slot first — that memory was just written to, it's still sitting in L1/L2. At high throughput this adds up.

---

## 3. Concurrency — The Deadlock Problem I Had to Think Through Carefully

One Mutex around the whole bucket array is safe but kills performance. Every thread blocks every other thread regardless of which MMR range they're working on. Adding a second worker thread actually made things slower in my first test because of the contention.

The solution is lock sharding — every bucket gets its own Mutex. Thread A sweeping the 1000 MMR range and Thread B sweeping the 3000 MMR range don't touch the same locks at all.

### The deadlock I had to solve

This immediately introduced a new problem I had to think through. The time-decay system means a worker might need to lock several adjacent buckets at once (e.g., buckets 18 through 22 when the search radius is 4). If two threads are both trying to acquire overlapping ranges in different orders you get a classic deadlock:

```
Thread A: locks bucket 18, tries to lock bucket 19...
Thread B: locks bucket 19, tries to lock bucket 18...
both wait forever
```

Two rules prevent this completely:

1. **Always acquire locks left to right** — lowest bucket index first, always. If every thread follows this, circular waits can't happen.
2. **`try_lock` with instant bailout** — if a lock is already held, drop everything and skip this sweep. Don't wait. The sweep loop runs continuously so it'll come back around in a millisecond.

I also added a parallel array of `AtomicUsize` counters — one per bucket — that track how many players are in each bucket without needing to acquire a lock. Workers check this first. If it's zero, skip the bucket entirely. This eliminated a lot of unnecessary lock attempts on empty buckets.

### Two kinds of threads

The worker threads (matchmaking logic) run as raw OS threads via `std::thread::spawn`, completely outside Tokio. The tick thread (which increments wait times) is a Tokio async task.

This matters because Tokio is cooperative — it can preempt async tasks to keep the runtime responsive. If I put the matchmaking workers inside Tokio they'd get interrupted mid-sweep. The workers need to run to completion without yielding, so they get real OS threads. The tick thread just sleeps for a second and does a quick pass, so async is fine there.

---

## 4. The Wait Time Math

Computing `R(t) = R_max * (1 - e^(-k*t))` inside the hot path was a mistake I caught early. Every worker sweep would be calling `exp()` in a loop. The result for any given wait time in seconds is always the same — there's no reason to recompute it.

So at boot the engine pre-computes the search radius for every integer second from 0 to `max_wait_seconds` and stores the results in a static Vec via `OnceLock`. At runtime, workers just index into it. One array access instead of a floating point calculation.

The formula itself is a bounded exponential. I looked at linear growth first (`radius = k * t`) but that would eventually let a top-0.1% player match with a bottom-0.1% player given enough wait time. The exponential curve asymptotes at `R_max` — you can tune how fast it expands with `k` but it physically cannot exceed the ceiling.

`saturating_sub` on the left boundary and `.min(num_buckets - 1)` on the right handles the edge cases at the extremes of the MMR range without any if-statements.

---

## 5. Team Balancing

Finding 10 compatible players is only half the problem. Splitting them into two fair teams of 5 is the other half.

With exactly 10 players there are C(10,5) = 252 valid 5v5 splits. I just check all of them. Iterate 0..1024, skip masks where `count_ones() != 5`, compute Team A MMR as the sum of players whose bit is set, Team B is `total - team_a`. Track the minimum difference. 

252 iterations of integer arithmetic runs in under a microsecond. I considered dynamic programming or a greedy approach but for exactly 10 players the brute force is faster because the constant is so small — any fancier algorithm has more overhead than the search itself.

Once the best mask is found, players get partitioned into team_a and team_b, a match ID is assigned via an atomic counter, and a `QueueEvent::MatchFound` fires down each player's oneshot channel to their waiting WebSocket task.

---

## 6. Benchmarks

Three test scripts, each testing something different:

**stress_test** — 8 threads hammering the arena with 400,000 total inserts, no network. This isolates the raw lock + arena throughput. On a release build I got around 11.8M insertions/sec. In debug mode it's much lower (218ms / ~1.8M/sec) because the compiler doesn't optimize the tight loops.

**distribution_test** — 10,000 players injected from a normal distribution centered at 2500 MMR, std dev ~600. Tight 10-second timeout to force the decay logic to actually fire. Results:

| Metric | Result |
|---|---|
| Matches formed | 998 (9,980 players matched) |
| Timeouts | 20 (the extreme tails) |
| Avg MMR spread | 0.6 points between teams |

The 0.6 point average spread is what I was most curious about. The bitmask balancer is doing its job — even after the bucket system groups players by skill range, the team balance within each match is nearly perfect.

**network_benchmark** — 2,000 WebSocket clients, all connected, all sending at the same millisecond via a Tokio barrier. This is the thundering herd test. P99 latency was 173ms, zero dropped connections.

> Note: run `ulimit -n 100000` before the network test or Linux caps you at 1024 file descriptors.

---

## 7. Why Build It This Way

CS2 at peak has maybe 1.5 million concurrent players. Matches last ~40 minutes. The actual sustained matchmaking load is roughly 600 requests/second. A single thread with a global Mutex handles that trivially in Rust.

I didn't build this because the requirements demanded it. I built it because I wanted to see what happens when you take the problem seriously — when you treat a matchmaker the same way you'd treat a trading engine or a real-time auction system. The arena, the LUT, the lock ordering — none of it is necessary at 600 req/s. It's all necessary at 1,000,000 req/s, and figuring out where exactly each bottleneck appears and why is the part I found genuinely interesting.
