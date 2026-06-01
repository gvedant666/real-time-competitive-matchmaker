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
    // atomics are zeroed/set to false by default.
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