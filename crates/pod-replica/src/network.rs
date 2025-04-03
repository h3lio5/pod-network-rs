use pod_common::{
    handle_connection, send_message_to_peer, Message, NetworkTrait, PodError, WsSink,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::{
    sync::{broadcast, RwLock},
    time::Duration,
};
use tokio_tungstenite::accept_async;
use futures_util::stream::StreamExt;
use tracing::{info, warn};
use uuid::Uuid;

/// Network implementation for replicas that only need to track connected clients
pub struct ReplicaNetwork {
    tx: broadcast::Sender<Message>,
    peers: Arc<RwLock<HashMap<Uuid, WsSink>>>,
    heartbeat_interval: Duration,
}

impl ReplicaNetwork {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            tx,
            heartbeat_interval: Duration::from_secs(5),
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start_heartbeat(&self) {
        let tx = self.tx.clone();
        let interval = self.heartbeat_interval;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval);
            loop {
                interval.tick().await;
                if tx.send(Message::Heartbeat).is_err() {
                    tracing::error!("Failed to send message to pod-replica channel");
                    break;
                }
            }
        });
    }
}

#[async_trait::async_trait]
impl NetworkTrait for ReplicaNetwork {
    async fn broadcast(&self, message: Message) -> Result<(), PodError> {
        let mut peers = self.peers.write().await;
        let mut disconnected = Vec::new();

        for (session_id, peer_sink) in peers.iter_mut() {
            if let Err(e) = send_message_to_peer(peer_sink, message.clone()).await {
                warn!("Failed to send to peer {}: {}", session_id, e);
                disconnected.push(*session_id);
            }
        }

        // Remove disconnected peers
        for session_id in disconnected {
            peers.remove(&session_id);
            info!("Session with ID {:?} terminated", session_id);
        }
        Ok(())
    }

    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<pod_common::types::Message> {
        self.tx.subscribe()
    }

    async fn listen(&self, address: &str) -> Result<(), PodError> {
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|e| PodError::NetworkError(e.to_string()))?;

        let tx = self.tx.clone();
        let peers = self.peers.clone();

        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let tx = tx.clone();
                let peers = peers.clone();
                tokio::spawn(async move {
                    if let Ok(ws_stream) = accept_async(stream).await {
                        let session_id = Uuid::new_v4();
                        // Split the stream into write (sink) and read (stream) halves.
                        let (write, read) = ws_stream.split();

                        {
                            // Insert the write half into the peers hashmap.
                            let mut peers_lock = peers.write().await;
                            peers_lock.insert(session_id, write);
                        }

                        // Process incoming messages from the read half.
                        handle_connection(read, tx).await;
                    }
                });
            }
        });

        // Start heartbeat
        self.start_heartbeat().await;

        info!("WebSocket server listening on {}", address);
        Ok(())
    }
}
