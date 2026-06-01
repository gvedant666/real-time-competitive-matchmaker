use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;
use tracing::info;

use super::state::{EngineState, NUM_BUCKETS};

// generate the look up table at compile time
const fn build_decay_lut() -> [usize; 300] {
    let mut lut = [0; 300];
    let mut i = 0;
    while i < 300 {
        lut[i] = if i <= 5 {
            0
        } else if i <= 15 {
            1
        } else if i <= 30 {
            2
        } else {
            3
        };
        i += 1;
    }
    lut
}

const DECAY_LUT: [usize; 300] = build_decay_lut();

// periodically increments relaxation levels for waiting players.
// relaxation level is the time player wait in matchmaking queue
// for now, its hardcoded, will think about it later
pub async fn spawn_tick_thread(state: Arc<EngineState>) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;

        for i in 0..NUM_BUCKETS {
            // fast atomic pre check before locking
            if state.active_counts[i].load(Relaxed) == 0 {
                continue;
            }

            if let Ok(bucket) = state.buckets[i].try_lock() {
                if let Some(&index) = bucket.peek_front() {
                    // lock the arena
                    if let Ok(arena) = state.arena.lock() {
                        if let Some(player) = arena.get(index) {
                            player.relaxation_level.fetch_add(1, Relaxed);
                        }
                    }
                }
            }
        }
    }
}

/// Matchmaking logic, lock-sharded synchronous sweep loop.
pub fn spawn_worker_thread(state: Arc<EngineState>) {
    std::thread::spawn(move || {
        loop {
            let mut matches_formed_this_sweep = false;

            for i in 0..NUM_BUCKETS {
                // atomic pre check fast lookup
                // skip if bucklet empty to avoid heavy lock
                if state.active_counts[i].load(Relaxed) == 0 {
                    continue;
                }

                // lock the center bucket to read the relaxation level of the front player 
                // and determine our look-up radius
                let (radius, min_bucket, max_bucket) = {
                    let bucket = match state.buckets[i].try_lock() {
                        Ok(b) => b,
                        Err(_) => continue,
                    };

                    // if another took our player, so need to check bucket empty

                    if bucket.is_empty() {
                        continue;
                    }

                    let front_index = *bucket.peek_front().unwrap();
                    let relaxation = {
                        let arena = state.arena.lock().unwrap();
                        let player = arena.get(front_index).unwrap();
                        player.relaxation_level.load(Relaxed)
                    };

                    // Cap look-up to prevent out-of-bounds panics
                    let lut_index = (relaxation as usize).min(299);
                    let radius = DECAY_LUT[lut_index];

                    let min_bucket = i.saturating_sub(radius);
                    let max_bucket = (i + radius).min(NUM_BUCKETS - 1);

                    // classic deadlock condition
                    // drop before locking multiple buckets to prevent deadlocks
                    drop(bucket);
                    (radius, min_bucket, max_bucket)
                };

                // left to right locking
                let mut guards = Vec::with_capacity(max_bucket - min_bucket + 1);
                let mut lock_failed = false;

                for b_idx in min_bucket..=max_bucket {
                    match state.buckets[b_idx].try_lock() {
                        Ok(guard) => guards.push(guard),
                        Err(_) => {
                            // Thread contention detected. Bail out instantly.
                            // The `guards` Vec drops here, automatically releasing all successfully held locks.
                            lock_failed = true;
                            break;
                        }
                    }
                }

                if lock_failed {
                    continue;
                }

                // extration
                let total_players: usize = guards.iter().map(|g| g.len()).sum();
                
                if total_players >= 10 {
                    let mut extracted = Vec::with_capacity(10);
                    let mut current_b_idx = min_bucket;

                    // Greedily pop players out of our held buckets until we have 10
                    for guard in guards.iter_mut() {
                        while extracted.len() < 10 {
                            if let Some(idx) = guard.pop() {
                                extracted.push(idx);
                                state.active_counts[current_b_idx].fetch_sub(1, Relaxed);
                            } else {
                                break; // Move to the next bucket
                            }
                        }
                        
                        if extracted.len() == 10 {
                            break;
                        }
                        current_b_idx += 1;
                    }

                    // free the slots and add to free list
                    let mut arena = state.arena.lock().unwrap();
                    for &idx in &extracted {
                        arena.free(idx);
                    }

                    info!("Match formed with Player Arena Indices: {:?}", extracted);
                    matches_formed_this_sweep = true;
                }
            }

            // If an entire sweep yields nothing, sleep 1ms to prevent core pegging
            if !matches_formed_this_sweep {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    });
}