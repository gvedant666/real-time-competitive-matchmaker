use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;
use tracing::info;

use super::state::EngineState;
use super::config::EngineConfig;

// The LUT is now a dynamically sized Vector stored in static memory
static DECAY_LUT: OnceLock<Vec<usize>> = OnceLock::new();

// generate the look up table at boot time using the TOML configuration
pub fn initialize_decay_lut(config: &EngineConfig) {
    DECAY_LUT.get_or_init(|| {
        let mut lut = Vec::with_capacity(config.max_wait_seconds);
        let max_radius = config.max_expansion_radius as f64;
        let decay_rate = config.decay_acceleration; 
        
        for t in 0..config.max_wait_seconds {
            // R(t) = R_max * (1 - e^(-k * t))
            let radius = max_radius * (1.0 - std::f64::consts::E.powf(-decay_rate * (t as f64)));
            lut.push(radius as usize);
        }
        lut
    });
}

#[inline(always)]
pub fn get_search_radius(seconds_waited: usize) -> usize {
    let lut = DECAY_LUT.get().expect("Decay LUT was not initialized at boot!");
    let t = seconds_waited.min(lut.len() - 1);
    *lut.get(t).unwrap()
}

// periodically increments relaxation levels for waiting players.
pub async fn spawn_tick_thread(state: Arc<EngineState>) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;

        let num_buckets = state.buckets.len();
        for i in 0..num_buckets {
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
            let num_buckets = state.buckets.len();

            for i in 0..num_buckets {
                // atomic pre check fast lookup
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

                    if bucket.is_empty() {
                        continue;
                    }

                    let front_index = *bucket.peek_front().unwrap();
                    let relaxation = {
                        let arena = state.arena.lock().unwrap();
                        let player = arena.get(front_index).unwrap();
                        player.relaxation_level.load(Relaxed)
                    };

                    let radius = get_search_radius(relaxation as usize);

                    let min_bucket = i.saturating_sub(radius);
                    let max_bucket = (i + radius).min(num_buckets - 1);

                    // classic deadlock condition prevention
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