# Architecture & Design — 5v5 Matchmaking Engine

**Vedant Gaikwad** — gvedant666@gmail.com

---

I want to write this doc a bit differently than a typical design doc. Instead of just explaining what the system does, I want to walk through the actual decisions I made, including the ones that didn't work the first time.

---

## The core problem

Matchmaking sounds simple: find 10 players near the same skill level, split them into two fair teams. The problem is that "near the same skill level" is doing a lot of work in that sentence.

If you're strict about it — only match players within ±25 MMR — most players get great matches but elite players (and very low-skill players) wait forever because the tails of the distribution are sparse. If you're loose about it, everyone gets a match quickly but the quality falls apart.

The other dimension is concurrency. A naive implementation with a single global lock works fine at 100 players/sec. It falls over at 100,000.

I wanted to solve both problems properly.

---

## How players get routed: buckets

The MMR range (0–5000) is divided into fixed-size buckets — 50 MMR each by default, giving 101 buckets. When a player joins, you just do `mmr / bucket_size` to get their bucket index. O(1), no searching.

Each bucket has its own `Mutex<Bucket>` holding a queue of arena indices (more on the arena below). Worker threads sweep buckets looking for 10 players to match.

This is basically spatial hashing applied to a 1D skill range. It works really well for the dense middle of the distribution and is fast to implement correctly.

The downside: a player at MMR 2473 and a player at MMR 2501 are in different buckets (bucket 49 and bucket 50) and won't naturally match, even though they're 28 MMR apart and that's a completely reasonable game. This is where constraint relaxation comes in.

---

## Time-based constraint relaxation

The longer a player waits, the wider their search radius gets. After 0 seconds the radius is 0 (exact bucket only). After 30 seconds it might be 8 buckets in each direction (±400 MMR). After 5 minutes it's capped at 15 buckets.

I went with a bounded exponential curve rather than linear growth:

```
R(t) = R_max * (1 - e^(-k * t))
```

Linear growth would eventually let a Diamond player match a Bronze player given enough wait time, which is never acceptable. The exponential curve asymptotes at `R_max`, so there's a hard ceiling on how far the search can expand regardless of wait time. `R_max` and `k` are tunable in `matchmaker.toml`.

I considered a step function (0 buckets for first 30s, then 5, then 10, then 15) but the exponential feels more natural and doesn't have the jarring jump behavior.

The relaxation values are precomputed into a lookup table at boot time. The hot path just does an array index — no floating point math during matching.

---

## Memory: the arena

My first implementation had each bucket hold `Vec<Player>` directly. Under load this was terrible — constant heap allocations and frees, cache thrashing, the allocator becoming a bottleneck. Profiling at around 20k inserts/sec showed the CPU burning time in the allocator, not in my code.

The fix is a pre-allocated arena: one big `Vec<Option<Player>>` allocated at startup, with a free list tracking which slots are available. Players go in, get an index back. When they leave, the index goes back on the free list. Zero allocations during runtime.

The free list is a stack (LIFO) rather than a queue. The reason is cache locality — if a slot was just freed, its memory is still warm in L1/L2. Handing that slot out immediately on the next insert means we're touching hot memory instead of pulling in a cold cache line from a slot that hasn't been touched in minutes.

Buckets store `usize` indices, not Player structs. This keeps the bucket locks cheap to hold and means the arena is the single source of truth for player state.

One thing I changed during development: I originally had a separate `connection_registry` (a `Vec<Mutex<Option<Connection>>>`) for storing each player's WebSocket sender. This was redundant — the arena slot already represents "one waiting player", so why have two parallel data structures? I moved the `uuid` and `oneshot::Sender` directly into the `Player` struct. Fewer locks, simpler ownership, same correctness.

---

## Concurrency and deadlocks

Each bucket has its own `Mutex`. This is lock sharding — thread A sweeping bucket 20 doesn't block thread B sweeping bucket 60.

The problem that comes up immediately: the time-decay radius means a worker might need to lock buckets 18 through 22 simultaneously. If thread A locks 18 then tries to lock 19, and thread B locks 19 then tries to lock 18, you have a classic deadlock.

Two rules prevent this:

**Strict left-to-right ordering.** Workers always acquire bucket locks from lowest index to highest. No exceptions. This breaks the circular wait condition.

**`try_lock` with immediate bailout.** If a worker can't acquire a bucket lock (because another thread holds it), it drops all its currently held locks and skips this sweep iteration. It doesn't wait. The sweep loop runs continuously anyway, so it'll try again in microseconds.

The `active_counts` array is a parallel array of `AtomicUsize` counters, one per bucket. Before touching a `Mutex`, the worker checks the atomic counter. If it's 0, skip the bucket entirely — no lock acquisition overhead. This makes empty buckets essentially free to scan.

The tick thread (which increments relaxation levels) is async under Tokio. The worker threads are `std::thread::spawn` — raw OS threads outside the Tokio runtime. Matchmaking workers do CPU-bound spin loops; putting them inside Tokio tasks would let the runtime preempt them and kill throughput. The separation matters.

---

## Team balancing

Once 10 players are extracted, they need to be split into two teams of 5 with the smallest possible MMR difference.

This is a variant of the balanced partition problem. With exactly 10 players, there are C(10,5) = 252 valid 5-vs-5 splits. I just iterate all of them.

Concretely: iterate all integers from 0 to 1023, check `mask.count_ones() == 5`, compute Team A MMR as the sum of players whose bit is set, Team B MMR is `total - team_a` (free), track the minimum difference. 1024 iterations of simple bit operations runs in under a microsecond.

I looked at whether a greedy or dynamic programming approach would be better here. For exactly 10 players, brute force wins — the constant is small enough that the overhead of a fancier algorithm would dominate. If this ever needed to scale to e.g. 20-player lobbies the calculus changes (C(20,10) = 184,756 iterations, still probably fine).

---

## Benchmarks

Three test scripts:

**stress_test** bypasses the network entirely and hammers the arena from 8 threads. On my machine (Ryzen 7, 16GB RAM, release build) 400,000 insertions across 8 threads completed in ~34ms. I've also seen numbers around 218ms in debug mode — the release build difference is significant.

**distribution_test** injects 10,000 players from a normal distribution centered at 2500 MMR (std dev ~600). With a 10-second timeout to stress the decay logic:
- 998 matches formed (9,980 players)
- 20 timeouts (the extreme tails)
- Average MMR spread between teams: 0.6 points

The 0.6 point average spread is the part I'm most happy with. The bitmask balancer is doing its job.

**network_benchmark** spins up 2,000 WebSocket clients, waits for all of them to connect, then releases them all simultaneously. This is the thundering herd test — making sure Tokio can absorb a connection stampede without the engine falling over. P99 round-trip was 173ms, no dropped connections.

One note: you need `ulimit -n 100000` before running the network test or Linux will block you at the default 1024 file descriptor limit.

---

## What I'd change with more time

The arena's global `Mutex` is still a serialization point. All worker threads contend on it when extracting players. Sharding the arena itself — or making it lock-free with an atomic free list — would push throughput higher. I prototyped a lock-free version but the ABA problem made it tricky to get right without `crossbeam`, and I didn't want to add that dependency without being sure it was correct.

The tick thread currently only updates the front player in each bucket, not all waiting players. This means a player sitting behind others in a bucket doesn't get their relaxation level incremented until the players ahead match. For the purposes of this assignment it's fine — the timeout logic still works correctly — but a production system would want proper per-player wait tracking, probably by storing a join timestamp instead of an incrementing counter.

The WebSocket layer is minimal by design. It does the job but a production version would need connection keepalives, authentication, graceful shutdown, and probably a REST endpoint for queue status.

---

## Why I built it this way

Honestly the CS2 matchmaking queue at peak is maybe 600 players/second. A single thread with a basic mutex handles that easily. I didn't build this because it was necessary — I built it because I wanted to see how far the architecture could be pushed and what problems come up when you take it seriously. The arena, the LUT, the lock ordering — none of that is required at 600 req/s. All of it is interesting to build correctly.
