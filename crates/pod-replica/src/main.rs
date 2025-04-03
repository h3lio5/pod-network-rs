use clap::Parser;
use pod_replica::network::ReplicaNetwork;
use pod_replica::Replica;
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber;
use hex;

/// Command line arguments for the replica
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Address to listen on (e.g., "127.0.0.1:8080")
    #[arg(short, long, default_value = "127.0.0.1:8080")]
    address: String,

    /// Secret key seed (32 bytes in hex format)
    #[arg(short, long)]
    seed: String,
}

#[tokio::main]
async fn main() {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize tracing
    tracing_subscriber::fmt().with_env_filter("info").init();

    // Parse seed from hex string
    let seed = match hex::decode(&args.seed) {
        Ok(bytes) => {
            if bytes.len() != 32 {
                error!("Seed must be 32 bytes (64 hex characters)");
                return;
            }
            let mut seed = [0u8; 32];
            seed.copy_from_slice(&bytes);
            seed
        }
        Err(e) => {
            error!("Failed to parse seed: {}", e);
            return;
        }
    };

    // Create network
    let network = Arc::new(Box::new(ReplicaNetwork::new()) as Box<dyn pod_common::NetworkTrait>);

    // Create and run replica
    match Replica::new(network, &seed) {
        Ok(replica) => {
            info!("Starting replica on {}", args.address);
            if let Err(e) = replica.run(&args.address).await {
                error!("Replica failed: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to create replica: {}", e);
        }
    }
}
