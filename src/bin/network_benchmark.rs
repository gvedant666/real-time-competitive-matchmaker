use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Barrier;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use serde::Serialize;

#[derive(Serialize)]
struct JoinRequest {
    action: String,
    player_id: String,
    mmr: u16,
}

#[tokio::main]
async fn main() {
    println!("=== WEBSOCKET LATENCY & CONCURRENCY BENCHMARK ===\n");

    let total_clients = 2000; // Must be a multiple of 10
    let url = "ws://127.0.0.1:8080";
    
    println!("Establishing {} concurrent WebSocket connections...", total_clients);
    
    // We use a barrier to ensure all 2000 clients connect BEFORE anyone sends a message.
    // This creates a massive, simultaneous stampede of network traffic.
    let barrier = Arc::new(Barrier::new(total_clients));
    let mut tasks = Vec::new();

    let start_setup = Instant::now();

    for i in 0..total_clients {
        let barrier_clone = Arc::clone(&barrier);
        let player_id = format!("bench_{}", i);
        
        let task = tokio::spawn(async move {
            let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
            let (mut sender, mut receiver) = ws_stream.split();
            
            let request = JoinRequest {
                action: "join_queue".to_string(),
                player_id,
                // Hardcode MMR to 1500 so they instantly match without waiting for decay
                mmr: 1500, 
            };
            let json = serde_json::to_string(&request).unwrap();

            // Wait at the barrier until all 2000 clients are fully connected
            barrier_clone.wait().await;
            
            let send_time = Instant::now();
            sender.send(Message::Text(json.into())).await.unwrap();

            // Wait for the match payload to come back
            while let Some(Ok(Message::Text(text))) = receiver.next().await {
                if text.contains("match_id") {
                    let rtt = send_time.elapsed();
                    return Ok::<Duration, String>(rtt);
                }
            }
            Err("Connection closed before match found".to_string())
        });
        
        tasks.push(task);
    }

    println!("All clients connected in {:.2?}. Releasing the barrier (Stampede!)...", start_setup.elapsed());
    
    let mut latencies = Vec::new();
    let mut failures = 0;

    for task in tasks {
        match task.await.unwrap() {
            Ok(rtt) => latencies.push(rtt),
            Err(_) => failures += 1,
        }
    }

    latencies.sort();

    if !latencies.is_empty() {
        let p50 = latencies[latencies.len() / 2];
        let p90 = latencies[(latencies.len() as f64 * 0.90) as usize];
        let p99 = latencies[(latencies.len() as f64 * 0.99) as usize];

        println!("\n=== NETWORK ROUND-TRIP LATENCY (RTT) RESULTS ===");
        println!("Total Clients: {}", total_clients);
        println!("Failed Connections: {}", failures);
        println!("Min Latency: {:.2?}", latencies[0]);
        println!("Median Latency (P50): {:.2?}", p50);
        println!("90th Percentile (P90): {:.2?}", p90);
        println!("99th Percentile (P99): {:.2?}", p99);
        println!("Max Latency: {:.2?}", latencies.last().unwrap());
    }
}