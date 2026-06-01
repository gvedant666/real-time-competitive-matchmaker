use serde::Deserialize;
use std::fs;

#[derive(Clone, Debug, Deserialize)]
pub struct EngineConfig {
    pub min_mmr: u16,
    pub max_mmr: u16,
    pub bucket_size: u16,
    
    pub max_wait_seconds: usize,
    pub max_expansion_radius: usize,
    pub decay_acceleration: f64,
    
    pub arena_size: usize,
}

impl EngineConfig {
    // Reads the toml file
    pub fn load() -> Self {
        let config_str = fs::read_to_string("matchmaker.toml")
            .expect("Failed to read matchmaker.toml file");
        
        toml::from_str(&config_str)
            .expect("Failed to parse matchmaker.toml")
    }

    pub fn num_buckets(&self) -> usize {
        ((self.max_mmr - self.min_mmr) / self.bucket_size) as usize + 1
    }
}