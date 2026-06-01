use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use rand_distr::{Distribution, Normal};
use serde::Serialize;
use std::time::{Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Serialize)]
struct JoinRequest {
    action: String,
    player_id: String,
    mmr: u16,
}

#[tokio::main]
async fn main() {
    println!("=== WEBSOCKET LOAD TESTER ===");
    
    let url = "ws://127.0.0.1:8080";
    println!("Connecting to {}...", url);
    
    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect to the matchmaking server! Is it running?");
    println!("Connected successfully!\n");
    
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Spawn a background task to read responses from the server
    tokio::spawn(async move {
        let mut match_count = 0;
        let mut timeout_count = 0;
        
        while let Some(msg) = ws_receiver.next().await {
            if let Ok(Message::Text(text)) = msg {
                if text.contains("match_id") {
                    match_count += 1;
                    println!("[RECEIVE] Match #{} formed! Data: {}\n", match_count, text);
                } else if text.contains("timeout") {
                    timeout_count += 1;
                    println!("[RECEIVE] Timeout #{} received.\n", timeout_count);
                } else {
                    // Ignore standard "queued" confirmation messages to keep terminal clean
                }
            }
        }
    });

    // Configuration for the injection rate
    let target_tps = 5;
    let sleep_duration = Duration::from_millis(1000 / target_tps as u64); // 50ms
    let total_players = 2000;
    
    let mut rng = rand::thread_rng();
    let normal = Normal::new(2500.0, 800.0).unwrap();

    println!("Starting injection: {} players/sec", target_tps);
    let start_time = Instant::now();

    for i in 1..=total_players {
        let mut mmr = normal.sample(&mut rng) as i32;
        mmr = mmr.clamp(0, 4999);
        
        let request = JoinRequest {
            action: "join_queue".to_string(),
            player_id: format!("bot_{}", i),
            mmr: mmr as u16,
        };

        let json_payload = serde_json::to_string(&request).unwrap();
        
        if let Err(e) = ws_sender.send(Message::Text(json_payload.into())).await {
            eprintln!("Failed to send data: {}", e);
            break;
        }

        tokio::time::sleep(sleep_duration).await;
        
        if i % 100 == 0 {
            println!("[SEND] Injected {} / {} players (Elapsed: {}s)", i, total_players, start_time.elapsed().as_secs());
        }
    }

    println!("\nInjection complete! Leaving connection open for 15 seconds to catch remaining matches...");
    tokio::time::sleep(Duration::from_secs(15)).await;
    println!("Load test finished.");
}