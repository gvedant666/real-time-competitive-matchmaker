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

pub fn initialize_decay_lut(config: &EngineConfig) {
    DECAY_LUT.get_or_init(|| {
        let mut lut = Vec::with_capacity(config.max_wait_seconds);
        let max_radius = config.max_expansion_radius as f64;
        let decay_rate = config.decay_acceleration; 
        
        for t in 0..config.max_wait_seconds {
            let radius = max_radius * (1.0 - std::f64::consts::E.powf(-decay_rate * (t as f64)));
            lut.push(radius as usize);
        }
        lut
    });
}

#[inline(always)]
pub fn get_search_radius(seconds_waited: usize) -> usize {
    let lut = DECAY_LUT.get().expect("lut missing");
    let t = seconds_waited.min(lut.len() - 1);
    *lut.get(t).unwrap()
}

pub async fn spawn_tick_thread(state: Arc<EngineState>) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        
        let mut indices_to_update = Vec::new();
        for bucket in &state.buckets {
            if let Ok(guard) = bucket.try_lock() {
                if let Some(&first_idx) = guard.peek_front() {
                    indices_to_update.push(first_idx);
                }
            }
        } 

        if !indices_to_update.is_empty() {
            if let Ok(mut arena) = state.arena.lock() {
                for idx in indices_to_update {
                    if let Some(player) = arena.get_mut(idx) {
                        player.relaxation_level = player.relaxation_level.saturating_add(1);
                    }
                }
            }
        }
    }
}

pub fn spawn_worker_thread(state: Arc<EngineState>) {
    std::thread::spawn(move || {
        loop {
            let mut matches_formed_this_sweep = false;
            let num_buckets = state.buckets.len();

            for i in 0..num_buckets {
                if state.active_counts[i].load(Relaxed) == 0 {
                    continue;
                }

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
                        player.relaxation_level
                    };

                    let radius = get_search_radius(relaxation as usize);
                    let min_bucket = i.saturating_sub(radius);
                    let max_bucket = (i + radius).min(num_buckets - 1);

                    drop(bucket);
                    (radius, min_bucket, max_bucket)
                };

                if radius == 0 && state.active_counts[i].load(Relaxed) < 10 {
                    continue;
                }

                let mut locked_buckets = Vec::with_capacity(max_bucket - min_bucket + 1);
                let mut lock_failed = false;

                for b_idx in min_bucket..=max_bucket {
                    if state.active_counts[b_idx].load(Relaxed) == 0 {
                        continue;
                    }

                    match state.buckets[b_idx].try_lock() {
                        Ok(guard) => locked_buckets.push((b_idx, guard)),
                        Err(_) => {
                            locked_buckets.clear();
                            lock_failed = true;
                            break;
                        }
                    }
                }

                if lock_failed {
                    continue;
                }

                let total_players: usize = locked_buckets.iter().map(|(_, g)| g.len()).sum();
                
                if total_players >= 10 {
                    let mut extracted = Vec::with_capacity(10);
                    
                    for (actual_bucket_idx, mut guard) in locked_buckets {
                        while extracted.len() < 10 {
                            if let Some(idx) = guard.pop() {
                                extracted.push(idx);
                                state.active_counts[actual_bucket_idx].fetch_sub(1, Relaxed);
                            } else {
                                break;
                            }
                        }
                        if extracted.len() == 10 {
                            break;
                        }
                    }

                    let mut match_players = Vec::with_capacity(10);
                    let mut active_connections = Vec::with_capacity(10);

                    {
                        let mut arena = state.arena.lock().unwrap();
                        for &idx in &extracted {
                            if let Some(mut player) = arena.take(idx) {
                                if let Some(sender) = player.sender.take() {
                                    match_players.push(MatchPlayer {
                                        uuid: player.uuid,
                                        mmr: player.mmr,
                                    });
                                    active_connections.push(sender);
                                }
                            }
                        }
                    }

                    if match_players.len() == 10 {
                        let final_match = create_balanced_match(match_players);
                        let current_match_id = MATCH_ID_COUNTER.fetch_add(1, Relaxed);

                        let payload = MatchResponse {
                            match_id: current_match_id,
                            team_a: final_match.team_a.clone(),
                            team_b: final_match.team_b.clone(),
                        };

                        for conn in active_connections {
                            let _ = conn.send(QueueEvent::MatchFound(payload.clone()));
                        }

                        let team_a_avg = final_match.team_a.iter().map(|p| p.mmr as f64).sum::<f64>() / 5.0;
                        let team_b_avg = final_match.team_b.iter().map(|p| p.mmr as f64).sum::<f64>() / 5.0;
                        
                        info!(
                            "Match formed! Team A Avg: {:.1}, Team B Avg: {:.1}, Diff: {:.1}",
                            team_a_avg,
                            team_b_avg,
                            (team_a_avg - team_b_avg).abs()
                        );
                    }

                    matches_formed_this_sweep = true;
                }
            }

            if !matches_formed_this_sweep {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    });
}