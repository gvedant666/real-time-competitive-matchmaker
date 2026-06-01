use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use rand::Rng;
use rand_distr::{Normal, Distribution};

use real_time_competitive_matchmaker::engine::config::EngineConfig;
use real_time_competitive_matchmaker::engine::state::EngineState;
use real_time_competitive_matchmaker::engine::worker::{
    initialize_decay_lut, spawn_tick_thread, spawn_worker_thread
};
use real_time_competitive_matchmaker::engine::balancer::QueueEvent;

#[tokio::main]
async fn main() {
    println!("=== DISTRIBUTION & MMR SPREAD TEST ===\n");

    let mut config = EngineConfig::load();
    // Force a tight bucket and fast timeout to stress the decay logic
    config.bucket_size = 20; 
    config.max_wait_seconds = 10;
    config.arena_size = 20_000;
    
    initialize_decay_lut(&config);
    let state = Arc::new(EngineState::new(config));

    tokio::spawn(spawn_tick_thread(Arc::clone(&state)));
    for _ in 0..4 {
        spawn_worker_thread(Arc::clone(&state));
    }

    let total_players = 10_000;
    let mut normal_rng = rand::thread_rng();
    let normal_dist = Normal::new(2500.0, 600.0).unwrap(); // Bell curve centered at 2500 MMR

    println!("Injecting {} players via Normal Distribution (Bell Curve)...", total_players);
    
    let mut receivers = Vec::with_capacity(total_players);

    for i in 0..total_players {
        let mut mmr = normal_dist.sample(&mut normal_rng) as i32;
        mmr = mmr.clamp(0, 4999);
        
        let (tx, rx) = tokio::sync::oneshot::channel();
        state.add_player(format!("p_{}", i), mmr as u16, tx).expect("Failed to inject");
        receivers.push(rx);
    }

    println!("Waiting for the matchmaking engine to drain the queue...");

    let mut match_ids = HashSet::new();
    let mut spread_counts = vec![0; 10]; // Buckets for the histogram
    let mut timeouts = 0;
    let mut total_spread = 0;
    
    // We set a hard timeout for the test script itself
    let timeout_duration = Duration::from_secs(12);

    for rx in receivers {
        if let Ok(Ok(event)) = tokio::time::timeout(timeout_duration, rx).await {
            match event {
                QueueEvent::MatchFound(response) => {
                    // Deduplicate because 10 players receive the same match_id
                    if match_ids.insert(response.match_id) {
                        
                        let team_a_mmr: u32 = response.team_a.iter().map(|p| p.mmr as u32).sum();
                        let team_b_mmr: u32 = response.team_b.iter().map(|p| p.mmr as u32).sum();
                        
                        let diff = team_a_mmr.abs_diff(team_b_mmr);
                        total_spread += diff;
                        
                        // Categorize for histogram (0-10, 11-20, etc.)
                        let index = (diff / 10).min(9) as usize;
                        spread_counts[index] += 1;
                    }
                }
                QueueEvent::Timeout => {
                    timeouts += 1;
                }
            }
        }
    }

    let formed_matches = match_ids.len();
    println!("\n=== TEST RESULTS ===");
    println!("Total Matches Formed: {}", formed_matches);
    println!("Total Player Timeouts (Extreme Edge Cases): {}", timeouts);
    
    if formed_matches > 0 {
        println!("Average MMR Spread (Difference between Team A and Team B): {:.1}", total_spread as f64 / formed_matches as f64);
        
        println!("\n=== MMR SPREAD HISTOGRAM ===");
        for (i, count) in spread_counts.iter().enumerate() {
            let label = if i == 9 { ">90".to_string() } else { format!("{:02}-{:02}", i * 10, i * 10 + 9) };
            let bar = "█".repeat((*count as f64 / formed_matches as f64 * 50.0) as usize);
            println!("{:<7} | {} ({})", label, bar, count);
        }
    }
}