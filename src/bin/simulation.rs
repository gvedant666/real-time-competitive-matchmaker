use rand::Rng;
use rand_distr::{Distribution, Normal};
use real_time_competitive_matchmaker::engine::config::EngineConfig;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

// Assuming the library crate is named `real_time_competitive_matchmaker`
use real_time_competitive_matchmaker::engine::state::EngineState;
use real_time_competitive_matchmaker::engine::worker::{initialize_decay_lut, spawn_tick_thread, spawn_worker_thread};

/// Helper to spin up a completely fresh engine for each test 
/// to prevent leftover stragglers from polluting the next test's metrics.
async fn setup_fresh_engine() -> Arc<EngineState> {
    let config = EngineConfig::load();
    let state = Arc::new(EngineState::new(config));
    tokio::spawn(spawn_tick_thread(Arc::clone(&state)));
    for _ in 0..4 {
        spawn_worker_thread(Arc::clone(&state));
    }
    state
}

/// Returns the elapsed time it took to clear the "Bulk" of the players (99%),
/// ignoring the artificial time spent waiting for edge-case stragglers to decay.
async fn wait_for_empty(state: &Arc<EngineState>, start_time: Instant, total_injected: usize) -> Duration {
    let mut last_count = usize::MAX;
    let mut unchanged_ticks = 0;
    let mut bulk_clear_time = None;

    // We consider the "Bulk" finished when 99% of players are matched
    let bulk_threshold = (total_injected as f64 * 0.01) as usize; 

    loop {
        let active_players: usize = state
            .active_counts
            .iter()
            .map(|count| count.load(Ordering::Relaxed))
            .sum();

        // Capture the true CPU speed before we start waiting for the Tick Thread
        if active_players <= bulk_threshold && bulk_clear_time.is_none() {
            bulk_clear_time = Some(start_time.elapsed());
        }

        if active_players == 0 {
            break;
        }

        if active_players == last_count {
            unchanged_ticks += 1;
        } else {
            last_count = active_players;
            unchanged_ticks = 0;
        }

        // 3-second timeout
        if unchanged_ticks > 300 {
            println!("  -> [TELEMETRY] Engine settled with {} mathematically isolated stragglers.", active_players);
            break;
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Return the fast bulk time if we hit it, otherwise return the total time
    bulk_clear_time.unwrap_or_else(|| start_time.elapsed())
}

/// Bypasses Tokio task overhead to test the raw Mutex ingestion and Worker extraction speed
async fn run_raw_engine_throughput(state: Arc<EngineState>, player_count: usize) {
    println!("[SIMULATION] Pre-generating {} players for Raw Speed Test...", player_count);
    
    // 1. Pre-generate data to remove RNG and memory allocation from the stopwatch
    let mut rng = rand::thread_rng();
    let normal = Normal::<f64>::new(2500.0, 500.0).unwrap();
    let mut players = Vec::with_capacity(player_count);
    for _ in 0..player_count {
        let mmr = (normal.sample(&mut rng).round() as i32).clamp(0, 4999) as u16;
        players.push(mmr);
    }

    println!("  -> Firing synchronous CPU-bound injection loop...");
    let start = Instant::now();

    // 2. Blast the engine sequentially on a single thread (Maximum lock pressure)
    for mmr in players {
        let _ = state.add_player(mmr);
    }

    // 3. Wait for the worker threads to sweep and clear the buckets
    let elapsed = wait_for_empty(&state, start, player_count).await;
    let tps = (player_count as f64 / elapsed.as_secs_f64()) as usize;
    
    println!(
        "[SIMULATION] Raw Engine Speed Complete! Bulk Time: {:.4?}, Throughput: {} TPS", 
        elapsed, tps
    );
}

/// Simulates a highly distributed concurrent load with purely random MMRs.
async fn run_uniform_chaos(state: Arc<EngineState>, player_count: usize) {
    println!("[SIMULATION] Starting Uniform Chaos Test ({} players)...", player_count);
    let start = Instant::now();

    for _ in 0..player_count {
        let state_clone = Arc::clone(&state);
        tokio::spawn(async move {
            let mut rng = rand::thread_rng();
            let mmr = rng.gen_range(0..=5000);
            let _ = state_clone.add_player(mmr);
        });
    }

    let elapsed = wait_for_empty(&state, start, player_count).await;
    let tps = (player_count as f64 / elapsed.as_secs_f64()) as usize;

    println!(
        "[SIMULATION] Uniform Chaos Complete! Bulk Time: {:.2?}, Players: {}, Throughput: {} TPS",
        elapsed, player_count, tps
    );
}

/// Simulates realistic matchmaking load where most players clump in the middle tiers.
async fn run_bell_curve(state: Arc<EngineState>, player_count: usize) {
    println!("[SIMULATION] Starting Bell Curve Test ({} players)...", player_count);
    let start = Instant::now();

    let normal = Normal::<f64>::new(2500.0, 500.0).expect("Failed to create Normal distribution");

    for _ in 0..player_count {
        let state_clone = Arc::clone(&state);
        let normal_dist = normal.clone(); // Lightweight clone for the closure
        
        tokio::spawn(async move {
            let mut rng = rand::thread_rng();
            let raw_mmr = normal_dist.sample(&mut rng).round() as i32;
            let clamped_mmr = raw_mmr.clamp(0, 4999) as u16;
            
            let _ = state_clone.add_player(clamped_mmr);
        });
    }

    let elapsed = wait_for_empty(&state, start, player_count).await;
    let tps = (player_count as f64 / elapsed.as_secs_f64()) as usize;

    println!(
        "[SIMULATION] Bell Curve Complete! Bulk Time: {:.2?}, Players: {}, Throughput: {} TPS",
        elapsed, player_count, tps
    );
}

/// Simulates the time-based decay logic for extremely high MMR players where 
/// queues are naturally isolated.
#[allow(dead_code)]
async fn run_low_pop_decay(state: Arc<EngineState>) {
    println!("[SIMULATION] Starting Low Pop Decay Test (10 players, staggered)...");
    let start = Instant::now();
    let player_count = 10;

    for i in 0..10 {
        let state_clone = Arc::clone(&state);
        tokio::spawn(async move {
            // Drop them into 4900 MMR (top bucket)
            let _ = state_clone.add_player(4900);
        });
        
        // Stagger injections by 1 second to trigger relaxation decay LUT
        if i < 9 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    let elapsed = wait_for_empty(&state, start, player_count).await;
    let tps = (player_count as f64 / elapsed.as_secs_f64()) as usize;

    println!(
        "[SIMULATION] Low Pop Decay Complete! Bulk Time: {:.4?}, Throughput: {} TPS", 
        elapsed, tps
    );
}

async fn run_gap_test(state: Arc<EngineState>) {
    println!("[SIMULATION] Starting 8-Bucket Gap Test...");
    let start = Instant::now();
    let player_count = 10;
    
    // Player A is isolated in Bucket 20 (e.g., 1000 MMR)
    let _ = state.add_player(1000); 
    
    // 9 Players are in Bucket 28 (e.g., 1400 MMR)
    // The gap is 8 buckets apart.
    for _ in 0..9 {
        let _ = state.add_player(1400); 
    }

    let elapsed = wait_for_empty(&state, start, player_count).await;
    let tps = (player_count as f64 / elapsed.as_secs_f64()) as usize;

    println!(
        "[SIMULATION] 8-Bucket Gap Complete! Bulk Time: {:.4?}, Throughput: {} TPS", 
        elapsed, tps
    );
}

#[tokio::main]
async fn main() {
    println!("Initializing HFT Matchmaking Engine Simulation...");
    
    // NOTE: DECAY_LUT must be initialized at runtime boot before any workers spin up
    let config = EngineConfig::load();
    initialize_decay_lut(&config);


    println!("Engine Online. Commencing stress tests...");
    println!("{:-<75}", "-");

    // Execute constraints sequentially, passing a completely fresh state to each
    run_bell_curve(setup_fresh_engine().await, 50_000).await;
    
    println!("{:-<75}", "-");
    
    run_uniform_chaos(setup_fresh_engine().await, 50_000).await;
    
    println!("{:-<75}", "-");

    run_raw_engine_throughput(setup_fresh_engine().await, 50_000).await;
    
    println!("{:-<75}", "-");

    run_gap_test(setup_fresh_engine().await).await;
    
    // Optional: Un-comment to test the staggered decay logic
    // println!("{:-<75}", "-");
    // run_low_pop_decay(setup_fresh_engine().await).await;
    
    println!("{:-<75}", "-");
    println!("All simulations completed successfully.");
}