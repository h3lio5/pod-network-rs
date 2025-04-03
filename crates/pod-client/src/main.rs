use axum::{routing::get, routing::post, Router};
use pod_client::client::*;
use pod_client::rest_api::*;
use pod_common::Network;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info};
use tracing_subscriber;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        // .with_env_filter("info")
        .init();

    // Setup mock network and client
    let replicas = HashMap::new();
    let network = Arc::new(Box::new(Network::new(replicas)) as Box<dyn pod_common::NetworkTrait>);
    let seed = [0u8; 32];
    let client = Client::new(network.clone(), 5, 1, &seed).unwrap();

    // Spawn client runtime
    let network_clone = network.clone();
    tokio::spawn(async move {
        let mut client = Client::new(network_clone, 5, 1, &seed).unwrap();
        if let Err(e) = client.run("127.0.0.1:3000").await {
            error!("Client run failed: {}", e);
        }
    });

    // Setup REST API
    let app = Router::new()
        .route("/write", post(write_tx))
        .route("/read", get(read_pod))
        .route("/status", post(get_tx_status))
        .route("/metrics", get(metrics))
        .with_state(Arc::new(client));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3001));
    info!("Starting REST API on {}", addr);
    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
