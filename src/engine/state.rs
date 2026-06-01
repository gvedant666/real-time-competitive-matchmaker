use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use super::arena::Arena;
use super::primitives::{Bucket, Player};
use super::config::EngineConfig;

#[derive(Debug)]
pub struct EngineState {
    pub(crate) arena: Mutex<Arena>,
    pub(crate) buckets: Vec<Mutex<Bucket>>,
    pub active_counts: Vec<AtomicUsize>, 
    pub config: EngineConfig,
}

impl EngineState {
    pub fn new(config: EngineConfig) -> Self {
        let num_buckets = config.num_buckets();
        
        let mut buckets = Vec::with_capacity(num_buckets);
        let mut active_counts = Vec::with_capacity(num_buckets);
        
        for _ in 0..num_buckets {
            buckets.push(Mutex::new(Bucket::new()));
            active_counts.push(AtomicUsize::new(0));
        }

        Self {
            arena: Mutex::new(Arena::new()),
            buckets,
            active_counts,
            config,
        }
    }

    // adding a player into the arena and routing to the correct bucket based on MMR
    pub fn add_player(&self, mmr: u16) -> Result<usize, &'static str> {
        let player = Player::new(mmr);

        let arena_index = {
            let mut arena_guard = self.arena.lock().map_err(|_| "Arena lock poisoned")?;
            arena_guard.insert(player).ok_or("Arena is full")?
        };

        let raw_bucket_index = (mmr / self.config.bucket_size) as usize;
        let target_bucket_index = raw_bucket_index.min(self.config.num_buckets() - 1);

        {
            let mut bucket_guard = self.buckets[target_bucket_index]
                .lock()
                .map_err(|_| "Bucket lock poisoned")?;
            bucket_guard.push(arena_index);
        }

        // increment the active count before returning the index
        self.active_counts[target_bucket_index].fetch_add(1, Ordering::Relaxed);

        Ok(arena_index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    // Helper to generate a default config for tests
    fn default_test_config() -> EngineConfig {
        EngineConfig {
            min_mmr: 0,
            max_mmr: 5000,
            bucket_size: 50,
            max_wait_seconds: 300,
            max_expansion_radius: 15,
            decay_acceleration: 0.03,
            arena_size: 100_000,
        }
    }

    #[test]
    fn test_bucket_routing() {
        let state = EngineState::new(default_test_config());

        // Test 1050 MMR routing (1050 / 50 = 21)
        let idx1 = state.add_player(1050).expect("Should add successfully");
        let bucket_21 = state.buckets[21].lock().unwrap();
        assert_eq!(bucket_21.len(), 1);
        assert_eq!(bucket_21.peek_front(), Some(&idx1));
        drop(bucket_21);

        // Test 400 MMR routing (400 / 50 = 8)
        let idx2 = state.add_player(400).expect("Should add successfully");
        let bucket_8 = state.buckets[8].lock().unwrap();
        assert_eq!(bucket_8.len(), 1);
        assert_eq!(bucket_8.peek_front(), Some(&idx2));
        drop(bucket_8);

        // Edge case: Test out-of-bounds MMR clamping (e.g., 6000 MMR -> should clamp to 99)
        let idx3 = state.add_player(6000).expect("Should add successfully");
        let bucket_99 = state.buckets[99].lock().unwrap();
        assert_eq!(bucket_99.len(), 1);
        assert_eq!(bucket_99.peek_front(), Some(&idx3));
    }

    #[test]
    fn test_concurrent_inserts() {
        let state = Arc::new(EngineState::new(default_test_config()));
        let mut handles = vec![];

        // Spawn 10 threads
        for _ in 0..10 {
            let state_clone = Arc::clone(&state);
            let handle = thread::spawn(move || {
                // Each thread inserts 100 players
                for _ in 0..100 {
                    // 1200 MMR -> Bucket 24
                    state_clone.add_player(1200).expect("Arena should not be full");
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().expect("Thread panicked during execution");
        }

        // Verify no deadlocks occurred and exactly 1,000 players exist in the target bucket
        let bucket_24 = state.buckets[24].lock().unwrap();
        assert_eq!(bucket_24.len(), 1000, "Bucket 24 should contain exactly 1,000 players from concurrent inserts");
    }
}