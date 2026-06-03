use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use super::arena::Arena;
use super::primitives::{Bucket, Player};
use super::config::EngineConfig;
use super::balancer::QueueEvent;

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

    pub fn add_player(&self, uuid: String, mmr: u16, tx: tokio::sync::oneshot::Sender<QueueEvent>) -> Result<usize, &'static str> {
        let player = Player::new(uuid, mmr, tx);

        let arena_index = {
            let mut arena_guard = self.arena.lock().map_err(|_| "lock poisoned")?;
            arena_guard.insert(player).ok_or("arena full")?
        };

        let raw_bucket_index = (mmr / self.config.bucket_size) as usize;
        let target_bucket_index = raw_bucket_index.min(self.config.num_buckets() - 1);

        {
            let mut bucket_guard = self.buckets[target_bucket_index]
                .lock()
                .map_err(|_| "bucket poisoned")?;
            bucket_guard.push(arena_index);
        }

        self.active_counts[target_bucket_index].fetch_add(1, Ordering::Relaxed);

        Ok(arena_index)
    }
}