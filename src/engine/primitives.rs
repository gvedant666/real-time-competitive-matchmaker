use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};


// atomics for lock free state on hot paht
#[derive(Debug)]
pub struct Player {
    pub mmr: u16,
    pub relaxation_level: AtomicU8,
    pub is_locked: AtomicBool,
}

impl Player {
    /// Initializes a new Player with a given MMR. 
    /// Atomics are zeroed/set to false by default.
    pub fn new(mmr: u16) -> Self {
        Self {
            mmr,
            relaxation_level: AtomicU8::new(0),
            is_locked: AtomicBool::new(false),
        }
    }
}

// storing arrena indices only

#[derive(Debug, Default)]
pub struct Bucket {
    queue: VecDeque<usize>,
}

impl Bucket {

    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    pub fn push(&mut self, index: usize) {
        self.queue.push_back(index);
    }

    pub fn pop(&mut self) -> Option<usize> {
        self.queue.pop_front()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn peek_front(&self) -> Option<&usize> {
        self.queue.front()
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucket_fifo_ordering() {
        let mut bucket = Bucket::new();
        
        // Push three arena indices
        bucket.push(42);
        bucket.push(108);
        bucket.push(7);

        assert_eq!(bucket.len(), 3);
        assert_eq!(bucket.peek_front(), Some(&42));
        
        // Verify strict FIFO (First-In, First-Out) popping
        assert_eq!(bucket.pop(), Some(42));
        assert_eq!(bucket.pop(), Some(108));
        assert_eq!(bucket.pop(), Some(7));
        
        // Ensure bucket is empty
        assert_eq!(bucket.pop(), None);
        assert_eq!(bucket.len(), 0);
    }

    #[test]
    fn test_player_optimistic_lock() {
        let player = Player::new(1500);

        // Attempt 1: Simulating thread 1 successfully locking the player
        let lock_success = player.is_locked.compare_exchange(
            false,
            true,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
        assert!(lock_success.is_ok(), "First lock attempt should succeed");

        // Attempt 2: Simulating thread 2 colliding and failing to lock
        let lock_fail = player.is_locked.compare_exchange(
            false,
            true,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
        assert!(lock_fail.is_err(), "Second lock attempt should fail due to thread collision");
    }
}