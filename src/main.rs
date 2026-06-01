use axum::{routing::get, Router};
use real_time_competitive_matchmaker::engine::config::EngineConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::info;

use real_time_competitive_matchmaker::api;
use real_time_competitive_matchmaker::engine::state::EngineState;
use real_time_competitive_matchmaker::engine::worker::{spawn_tick_thread, spawn_worker_thread, initialize_decay_lut};

#[tokio::main]
async fn main() {

    tracing_subscriber::fmt::init();
    info!("Starting Matchmaking Server...");

    let config = EngineConfig::load();

    initialize_decay_lut(&config);

    let engine_state = Arc::new(EngineState::new(config));

    tokio::spawn(spawn_tick_thread(engine_state.clone()));

    // spawnning 2 threads for now
    // will increase according to the system later
    for _ in 0..1 {
        spawn_worker_thread(Arc::clone(&engine_state));
    }

    // building the axum router
    let app = Router::new()
        .route("/ws", get(api::ws::ws_handler))
        .with_state(engine_state);

    // binding to localhost:3000
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("Listening on ws://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}