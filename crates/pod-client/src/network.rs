use async_trait::async_trait;
use ed25519_dalek::VerifyingKey as PublicKey;
use futures_util::StreamExt;
use pod_common::{
    handle_connection, send_message_to_peer, Message, NetworkTrait, PodError, WsSink,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tokio_tungstenite::connect_async;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct ClientNetwork {
    // Map of replica public keys to their websocket sinks.
    peers: Arc<RwLock<HashMap<PublicKey, WsSink>>>,
    // A broadcast channel used to propagate incoming messages to all subscribers.
    tx: broadcast::Sender<Message>,
}

impl Default for ClientNetwork {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientNetwork {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            tx,
        }
    }

    /// Connect to a replica at the given URL and store the write half in the peers map.
    pub async fn connect_to_replica(
        &self,
        public_key: PublicKey,
        url: String,
    ) -> Result<(), PodError> {
        let (ws_stream, _) = connect_async(url)
            .await
            .map_err(|e| PodError::NetworkError(e.to_string()))?;
        let (sink, stream) = ws_stream.split();
        {
            let mut peers = self.peers.write().await;
            peers.insert(public_key, sink);
        }
        // Spawn a task to process incoming messages from this replica.
        let tx = self.tx.clone();
        tokio::spawn(async move {
            handle_connection(stream, tx).await;
        });
        Ok(())
    }

    /// Sends a message to a replica (using its stored websocket sink).
    pub async fn send_to_replica(
        &self,
        public_key: PublicKey,
        message: Message,
    ) -> Result<(), PodError> {
        let mut peers = self.peers.write().await;
        if let Some(sink) = peers.get_mut(&public_key) {
            send_message_to_peer(sink, message).await
        } else {
            Err(PodError::NetworkError(
                "Connection dropped (WsSink)".to_string(),
            ))
        }
    }

    /// Broadcasts a message to all connected replicas.
    pub async fn broadcast(&self, message: Message) -> Result<(), PodError> {
        let mut peers = self.peers.write().await;
        let mut disconnected = Vec::new();

        for (pub_key, peer_sink) in peers.iter_mut() {
            if let Err(e) = send_message_to_peer(peer_sink, message.clone()).await {
                warn!(
                    "Failed to send to replica {:?}: {}",
                    hex::encode(pub_key.to_bytes()),
                    e
                );
                disconnected.push(*pub_key);
            }
        }
        // Remove disconnected replicas
        for pub_key in disconnected {
            peers.remove(&pub_key);
            info!(
                "Session with ID {:?} terminated",
                hex::encode(pub_key.to_bytes())
            );
        }
        Ok(())
    }

    /// Returns a broadcast receiver for incoming messages.
    pub fn subscribe(&self) -> broadcast::Receiver<Message> {
        self.tx.subscribe()
    }
}

#[async_trait]
impl NetworkTrait for ClientNetwork {
    async fn broadcast(&self, message: Message) -> Result<(), PodError> {
        self.broadcast(message).await
    }

    fn subscribe(&self) -> broadcast::Receiver<Message> {
        self.subscribe()
    }

    // The client does not accept incoming connections.
    async fn listen(&self, _address: &str) -> Result<(), PodError> {
        Ok(())
    }
}
