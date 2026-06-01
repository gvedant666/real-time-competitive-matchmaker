use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;
use tracing::info;

use super::state::EngineState;
use super::config::EngineConfig;
use super::balancer::{create_balanced_match, MatchPlayer, MatchResponse, QueueEvent};


static DECAY_LUT: OnceLock<Vec<usize>> = OnceLock::new();
static MATCH_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

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
    let timeout_limit = state.config.max_wait_seconds as u8;

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let num_buckets = state.buckets.len();

        for i in 0..num_buckets {
            if state.active_counts[i].load(Relaxed) == 0 {
                continue;
            }

            if let Ok(mut bucket) = state.buckets[i].try_lock() {
                let mut timed_out_indices = Vec::new();
                let len = bucket.len();
                
                for _ in 0..len {
                    if let Some(idx) = bucket.pop() {
                        let mut is_timed_out = false;
                        
                        if let Ok(arena) = state.arena.lock() {
                            if let Some(player) = arena.get(idx) {
                                let current_wait = player.relaxation_level.fetch_add(1, Relaxed) + 1;
                                if current_wait >= timeout_limit {
                                    is_timed_out = true;
                                }
                            }
                        }

                        if is_timed_out {
                            timed_out_indices.push(idx);
                        } else {
                            // Re-insert active players
                            bucket.push(idx); 
                        }
                    }
                }

                // Process Atomic Evictions
                if !timed_out_indices.is_empty() {
                    let mut arena = state.arena.lock().unwrap();
                    for idx in timed_out_indices {
                        if let Ok(mut reg_guard) = state.connection_registry[idx].lock() {
                            if let Some(conn) = reg_guard.take() {
                                let _ = conn.sender.send(QueueEvent::Timeout);
                            }
                        }
                        arena.free(idx);
                        state.active_counts[i].fetch_sub(1, Relaxed);
                    }
                }
            }
        }
    }
}

// Matchmaking logic, lock-sharded synchronous sweep loop.
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
                let (_radius, min_bucket, max_bucket) = {
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

                    for guard in guards.iter_mut() {
                        while extracted.len() < 10 {
                            if let Some(idx) = guard.pop() {
                                extracted.push(idx);
                                state.active_counts[current_b_idx].fetch_sub(1, Relaxed);
                            } else {
                                break;
                            }
                        }
                        
                        if extracted.len() == 10 {
                            break;
                        }
                        current_b_idx += 1;
                    }

                    let mut match_players = Vec::with_capacity(10);
                    let mut active_connections = Vec::with_capacity(10);

                    {
                        let mut arena = state.arena.lock().unwrap();
                        for &idx in &extracted {
                            if let Some(player) = arena.get(idx) {
                                if let Ok(mut reg_guard) = state.connection_registry[idx].lock() {
                                    if let Some(conn) = reg_guard.take() {
                                        match_players.push(MatchPlayer {
                                            uuid: conn.uuid.clone(),
                                            mmr: player.mmr,
                                        });
                                        active_connections.push(conn);
                                    }
                                }
                            }
                            arena.free(idx);
                        }
                    }

                    drop(guards);

                    if match_players.len() == 10 {
                        let final_match = create_balanced_match(match_players);
                        let current_match_id = MATCH_ID_COUNTER.fetch_add(1, Relaxed);

                        let payload = MatchResponse {
                            match_id: current_match_id,
                            team_a: final_match.team_a.clone(),
                            team_b: final_match.team_b.clone(),
                        };

                        for conn in active_connections {
                            let _ = conn.sender.send(QueueEvent::MatchFound(payload.clone()));
                        }

                        let team_a_avg = final_match.team_a.iter().map(|p| p.mmr as f64).sum::<f64>() / 5.0;
                        let team_b_avg = final_match.team_b.iter().map(|p| p.mmr as f64).sum::<f64>() / 5.0;
                        
                        info!(
                            "Match formed! Team A Avg: {:.1}, Team B Avg: {:.1}, Difference: {:.1}",
                            team_a_avg,
                            team_b_avg,
                            (team_a_avg - team_b_avg).abs()
                        );
                    }

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