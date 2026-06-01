use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use super::arena::Arena;
use super::primitives::{Bucket, Player};

pub const NUM_BUCKETS: usize = 100;
pub const BUCKET_MMR_RANGE: u16 = 50;


#[derive(Debug)]
pub struct EngineState {
    pub(crate) arena: Mutex<Arena>,
    pub(crate) buckets: [Mutex<Bucket>; NUM_BUCKETS],
    pub active_counts: [AtomicUsize; NUM_BUCKETS], 
}

impl EngineState {
    pub fn new() -> Self {
        Self {
            arena: Mutex::new(Arena::new()),
            buckets: std::array::from_fn(|_| Mutex::new(Bucket::new())),
            // ADDED: Initialize active counts to 0
            active_counts: std::array::from_fn(|_| AtomicUsize::new(0)),
        }
    }

    // adding a player into the arena and routing to the correct bucket based on MMR
    pub fn add_player(&self, mmr: u16) -> Result<usize, &'static str> {
        let player = Player::new(mmr);

        let arena_index = {
            let mut arena_guard = self.arena.lock().map_err(|_| "Arena lock poisoned")?;
            arena_guard.insert(player).ok_or("Arena is full")?
        };

        let raw_bucket_index = (mmr / BUCKET_MMR_RANGE) as usize;
        let target_bucket_index = raw_bucket_index.min(NUM_BUCKETS - 1);

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

impl Default for EngineState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_bucket_routing() {
        let state = EngineState::new();

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
        let state = Arc::new(EngineState::new());
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