use crate::errors::PodError;
use crate::types::Message;
use async_trait::async_trait;
use ed25519_dalek::VerifyingKey as PublicKey;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use tokio::{net::TcpStream, sync::mpsc};
use tokio_tungstenite::{tungstenite, WebSocketStream};
use tracing::warn;

#[async_trait]
pub trait NetworkTrait: Send + Sync {
    async fn send_to_replica(
        &self,
        replica_id: PublicKey,
        message: Message,
    ) -> Result<(), PodError>;
    async fn broadcast(&self, message: Message) -> Result<(), PodError>;
    // async fn receive(&self) -> mpsc::Receiver<Message>;
    async fn listen(&self, address: &str) -> Result<(), PodError>;
}

pub struct Network {
    tx: mpsc::Sender<Message>,
    rx: mpsc::Receiver<Message>,
    peers: HashMap<PublicKey, String>,
}

impl Network {
    pub fn new(peers: HashMap<PublicKey, String>) -> Self {
        let (tx, rx) = mpsc::channel(1000);
        Self { tx, rx, peers }
    }

    async fn send_message(&self, addr: &str, message: Message) -> Result<(), PodError> {
        let (mut ws_stream, _) = tokio_tungstenite::connect_async(addr)
            .await
            .map_err(|e| PodError::NetworkError(e.to_string()))?;
        let data = bincode::encode_to_vec(&message, bincode::config::standard())
            .map_err(|e| PodError::NetworkError(e.to_string()))?;
        ws_stream
            .send(tungstenite::protocol::Message::Binary(data.into()))
            .await
            .map_err(|e| PodError::NetworkError(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
impl NetworkTrait for Network {
    async fn send_to_replica(
        &self,
        replica_id: PublicKey,
        message: Message,
    ) -> Result<(), PodError> {
        let addr = self
            .peers
            .get(&replica_id)
            .ok_or_else(|| PodError::NetworkError("Unknown replica".to_string()))?;
        self.send_message(addr, message).await
    }

    async fn broadcast(&self, message: Message) -> Result<(), PodError> {
        for (_, addr) in self.peers.iter() {
            if let Err(e) = self.send_message(addr, message.clone()).await {
                warn!("Failed to send to {}: {}", addr, e);
            }
        }
        Ok(())
    }

    // async fn receive(&self) -> mpsc::Receiver<Message> {
    //     self.rx.clone()
    // }

    async fn listen(&self, address: &str) -> Result<(), PodError> {
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|e| PodError::NetworkError(e.to_string()))?;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let tx = tx.clone();
                tokio::spawn(async move {
                    if let Ok(ws_stream) = tokio_tungstenite::accept_async(stream).await {
                        handle_connection(ws_stream, tx).await;
                    }
                });
            }
        });
        tracing::info!("WebSocket server listening on {}", address);
        Ok(())
    }
}

async fn handle_connection(mut ws_stream: WebSocketStream<TcpStream>, tx: mpsc::Sender<Message>) {
    while let Some(Ok(msg)) = ws_stream.next().await {
        if let tungstenite::Message::Binary(data) = msg {
            match bincode::decode_from_slice(&data, bincode::config::standard()) {
                Ok((message, _)) => {
                    if tx.send(message).await.is_err() {
                        tracing::error!("Failed to send message to channel");
                        break;
                    }
                }
                Err(e) => warn!("Failed to deserialize message: {}", e),
            }
        }
    }
}
