use axum::{routing::get, routing::post, Router};
use clap::Parser;
use hex::FromHex;
use pod_client::client::*;
use pod_client::network::*;
use pod_client::rest_api::*;
use pod_common::Crypto;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info};
use tracing_subscriber;

// Command line arguments.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the YAML configuration file
    #[arg(short, long)]
    config: String,

    /// Threshold for confirmation
    #[arg(short, long)]
    alpha: usize,

    /// Byzantine fault tolerance threshold
    #[arg(short, long)]
    beta: usize,
}

// Replica configuration as represented in the YAML file.
#[derive(Debug, serde::Deserialize)]
struct ReplicaEntry {
    public_key: String, // hex encoded
    url: String,
}

// The top-level config structure.
#[derive(Debug, serde::Deserialize)]
struct Config {
    replicas: Vec<ReplicaEntry>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    // Parse command-line arguments.
    let args = Args::parse();
    let config_content = std::fs::read_to_string(&args.config).expect("Failed to read config file");
    let config: Config =
        serde_yaml::from_str(&config_content).expect("Failed to parse config file");

    // Build a hashmap of replicas.
    let mut replicas = HashMap::new();
    for entry in config.replicas.into_iter() {
        // Parse the hex string into bytes.
        let pk_bytes = Vec::from_hex(&entry.public_key).expect("Invalid hex in public_key");
        let pub_key = Crypto::replica_pubkey_from_bytes(&pk_bytes)
            .expect(format!("Could not derive public key for {:?}", &entry.public_key).as_str());
        replicas.insert(pub_key, entry.url);
    }

    let network = Arc::new(ClientNetwork::new());
    let seed = [0u8; 32];
    let client =
        Arc::new(Client::new(network.clone(), args.alpha, args.beta, &seed).expect("Failed to create client"));

    // Spawn the client runtime that connects to replicas and processes incoming messages.
    let client_runner = client.clone();
    tokio::spawn(async move {
        if let Err(e) = client_runner.run(replicas).await {
            error!("Client run failed: {}", e);
        }
    });

    // Setup REST API endpoints.
    let app = Router::new()
        .route("/write", post(write_tx))
        .route("/read", get(read_pod))
        .route("/status", post(get_tx_status))
        .route("/metrics", get(metrics))
        .with_state(client);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3001));
    info!("Starting REST API on {}", addr);
    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
