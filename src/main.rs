use axum::{routing::get, Router};
use std::net::SocketAddr;
use tracing::info;

mod api;
mod models;

#[tokio::main]
async fn main() {

    tracing_subscriber::fmt::init();
    info!("Starting Eterna Matchmaking Server...");

    // Build the Axum router
    let app = Router::new()
        .route("/ws", get(api::ws::ws_handler));

    // Bind to port 3000 and ipv4 localhost
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("Listening on ws://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}