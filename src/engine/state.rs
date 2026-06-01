use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use super::arena::Arena;
use super::primitives::{Bucket, Player};
use super::config::EngineConfig;
use super::balancer::{MatchResponse, QueueEvent};

pub struct Connection {
    pub uuid: String,
    pub sender: tokio::sync::oneshot::Sender<QueueEvent>,
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("uuid", &self.uuid)
            .field("sender", &"<oneshot_channel>")
            .finish()
    }
}

#[derive(Debug)]
pub struct EngineState {
    pub(crate) arena: Mutex<Arena>,
    pub(crate) buckets: Vec<Mutex<Bucket>>,
    pub active_counts: Vec<AtomicUsize>, 
    pub config: EngineConfig,
    pub connection_registry: Vec<Mutex<Option<Connection>>>,
}

impl EngineState {
    pub fn new(config: EngineConfig) -> Self {
        let num_buckets = config.num_buckets();
        let arena_size = config.arena_size;
        
        let mut buckets = Vec::with_capacity(num_buckets);
        let mut active_counts = Vec::with_capacity(num_buckets);
        let mut connection_registry = Vec::with_capacity(arena_size);
        
        for _ in 0..num_buckets {
            buckets.push(Mutex::new(Bucket::new()));
            active_counts.push(AtomicUsize::new(0));
        }

        for _ in 0..arena_size {
            connection_registry.push(Mutex::new(None));
        }

        Self {
            arena: Mutex::new(Arena::new()),
            buckets,
            active_counts,
            config,
            connection_registry,
        }
    }

    // adding a player into the arena and routing to the correct bucket based on MMR
    pub fn add_player(&self, uuid: String, mmr: u16, tx: tokio::sync::oneshot::Sender<QueueEvent>) -> Result<usize, &'static str> {
        let player = Player::new(mmr);

        let arena_index = {
            let mut arena_guard = self.arena.lock().map_err(|_| "Arena lock poisoned")?;
            arena_guard.insert(player).ok_or("Arena is full")?
        };

        {
            let mut registry_guard = self.connection_registry[arena_index].lock().map_err(|_| "Registry lock poisoned")?;
            *registry_guard = Some(Connection { uuid, sender: tx });
        }

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