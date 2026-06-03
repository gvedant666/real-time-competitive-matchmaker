use std::collections::VecDeque;
use super::balancer::QueueEvent;

#[derive(Debug)]
pub struct Player {
    pub uuid: String,
    pub mmr: u16,
    pub relaxation_level: u8,
    pub sender: Option<tokio::sync::oneshot::Sender<QueueEvent>>,
}

impl Player {
    pub fn new(uuid: String, mmr: u16, sender: tokio::sync::oneshot::Sender<QueueEvent>) -> Self {
        Self {
            uuid,
            mmr,
            relaxation_level: 0,
            sender: Some(sender),
        }
    }
}

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