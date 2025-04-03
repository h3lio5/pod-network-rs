use ed25519_dalek::VerifyingKey as PublicKey;
use lazy_static::lazy_static;
use pod_common::{Crypto, Message, NetworkTrait, PodError, Transaction, TransactionId, Vote};
use prometheus::{Gauge, Registry};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

pub mod network;

lazy_static! {
    static ref REGISTRY: Registry = Registry::new();
    static ref REPLICA_LOG_SIZE: Gauge =
        Gauge::new("replica_log_size", "Size of replica log").unwrap();
}

pub fn init_metrics() {
    REGISTRY
        .register(Box::new(REPLICA_LOG_SIZE.clone()))
        .unwrap();
}

/// A replica in the POD network that processes transactions and votes.
pub struct Replica {
    id: PublicKey,
    crypto: Crypto,
    state: Arc<Mutex<ReplicaState>>,
    network: Arc<Box<dyn NetworkTrait>>,
}

/// Internal state of a replica.
#[derive(Default)]
struct ReplicaState {
    sequence_number: u64,
    log: HashMap<TransactionId, Vote>,
    clients: HashSet<PublicKey>,
}

impl Replica {
    /// Creates a new replica with the given network and secret key.
    pub fn new(network: Arc<Box<dyn NetworkTrait>>, seed: &[u8; 32]) -> Result<Self, PodError> {
        let crypto = Crypto::from_secret_key(seed)?;
        init_metrics();
        Ok(Replica {
            id: crypto.public_key(),
            crypto,
            state: Arc::new(Mutex::new(ReplicaState::default())),
            network,
        })
    }

    /// Runs the replica, listening for messages and processing them.
    pub async fn run(&self, address: &str) -> Result<(), PodError> {
        self.network.listen(address).await?;
        let mut rx = self.network.subscribe();

        while let Ok(message) = rx.recv().await {
            match message {
                Message::Connect => self.handle_connect().await?,
                Message::Write(tx) => self.handle_write(tx).await?,
                Message::Vote(_) => {
                    warn!("Replica received unexpected vote message");
                }
                Message::Heartbeat => self.handle_heartbeat_request().await?,
            }
        }
        Ok(())
    }

    /// Handles a Connect message from a client.
    async fn handle_connect(&self) -> Result<(), PodError> {
        info!(
            "Replica {} received CONNECT",
            hex::encode(self.id.to_bytes())
        );
        self.state.lock().await.clients.insert(self.id);

        self.broadcast_log().await
    }

    /// Handles a Write message containing a transaction.
    async fn handle_write(&self, tx: Transaction) -> Result<(), PodError> {
        let mut state = self.state.lock().await;

        // Skip duplicate transactions
        if state.log.contains_key(&tx.id) {
            return Ok(());
        }

        // Create and sign vote
        let timestamp = self.current_round();
        let sn = state.sequence_number;
        state.sequence_number += 1;

        let vote = self.create_vote(Some(&tx), timestamp, sn)?;

        // Update state and broadcast
        state.log.insert(tx.id, vote.clone());
        REPLICA_LOG_SIZE.set(state.log.len() as f64);

        self.network.broadcast(Message::Vote(vote)).await?;
        Ok(())
    }

    async fn handle_heartbeat_request(&self) -> Result<(), PodError> {
        let mut state = self.state.lock().await;
        // create a heartbeat vote
        let timestamp = self.current_round();
        let sn = state.sequence_number;
        state.sequence_number += 1;

        let vote = self.create_vote(None, timestamp, sn)?;

        self.network.broadcast(Message::Vote(vote)).await?;
        Ok(())
    }

    /// Creates a signed vote for a transaction.
    fn create_vote(
        &self,
        tx: Option<&Transaction>,
        timestamp: u64,
        sn: u64,
    ) -> Result<Vote, PodError> {
        let message = bincode::encode_to_vec(&(tx, timestamp, sn), bincode::config::standard())
            .map_err(|e| PodError::NetworkError(e.to_string()))?;

        let signature = self.crypto.sign(&message).to_vec();
        Ok(Vote {
            tx: tx.map(|t| t.clone()),
            timestamp,
            sequence_number: sn,
            signature,
            replica_id: self.id.to_bytes().to_vec(),
        })
    }

    /// Broadcasts the entire log to all clients.
    async fn broadcast_log(&self) -> Result<(), PodError> {
        let state = self.state.lock().await;
        for vote in state.log.values() {
            self.network.broadcast(Message::Vote(vote.clone())).await?;
        }
        Ok(())
    }

    /// Returns the current round number based on system time.
    fn current_round(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}
