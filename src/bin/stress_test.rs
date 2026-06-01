use std::sync::Arc;
use std::time::Instant;
use std::thread;
use rand::Rng;

use real_time_competitive_matchmaker::engine::config::EngineConfig;
use real_time_competitive_matchmaker::engine::state::EngineState;
use real_time_competitive_matchmaker::engine::worker::{
    initialize_decay_lut, spawn_worker_thread
};

fn main() {
    println!("=== arena concurrency stress test ===");

    let mut config = EngineConfig::load();
    config.arena_size = 500_000;
    
    initialize_decay_lut(&config);
    let state = Arc::new(EngineState::new(config));

    // spawn background workers to process the queue
    for _ in 0..4 {
        spawn_worker_thread(Arc::clone(&state));
    }

    let thread_count = 8;
    let players_per_thread = 50_000;
    let mut handles = vec![];

    let start_time = Instant::now();

    // hammer the arena from 8 parallel threads
    for thread_id in 0..thread_count {
        let state_clone = Arc::clone(&state);
        
        let handle = thread::spawn(move || {
            let mut rng = rand::thread_rng();
            
            for i in 0..players_per_thread {
                let mmr = rng.gen_range(0..5000) as u16;
                let uuid = format!("t{}_p{}", thread_id, i);
                let (tx, _rx) = tokio::sync::oneshot::channel();
                
                // panic immediately if a lock poisons or arena overflows
                state_clone.add_player(uuid, mmr, tx).expect("insertion failed");
            }
        });
        
        handles.push(handle);
    }

    // wait for all injection threads to finish
    for handle in handles {
        handle.join().unwrap();
    }

    let inject_duration = start_time.elapsed();
    let total_players = thread_count * players_per_thread;
    let tps = (total_players as f64 / inject_duration.as_secs_f64()) as u64;

    println!("injected {} players successfully", total_players);
    println!("time taken: {:.2?}", inject_duration);
    println!("peak throughput: {} insertions/sec", tps);
}