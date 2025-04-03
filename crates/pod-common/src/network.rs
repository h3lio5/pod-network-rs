use crate::errors::PodError;
use crate::types::Message;
use async_trait::async_trait;
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use tokio::{net::TcpStream, sync::broadcast};
use tokio_tungstenite::{
    tungstenite::{self, Message as TungMessage},
    MaybeTlsStream, WebSocketStream,
};
use tracing::warn;

pub type WsSink = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, TungMessage>;
pub type WsStream = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// Core network functionality for message broadcasting and receiving
#[async_trait]
pub trait NetworkTrait: Send + Sync {
    async fn broadcast(&self, message: Message) -> Result<(), PodError>;
    fn subscribe(&self) -> broadcast::Receiver<Message>;
    async fn listen(&self, address: &str) -> Result<(), PodError>;
}

pub async fn handle_connection(mut read: WsStream, tx: broadcast::Sender<Message>) {
    while let Some(Ok(msg)) = read.next().await {
        if let tungstenite::Message::Binary(data) = msg {
            match bincode::decode_from_slice(&data, bincode::config::standard()) {
                Ok((message, _)) => {
                    if tx.send(message).is_err() {
                        tracing::error!("Failed to send message to channel");
                        break;
                    }
                }
                Err(e) => warn!("Failed to deserialize message: {}", e),
            }
        }
    }
}

pub async fn send_message_to_peer(sink: &mut WsSink, message: Message) -> Result<(), PodError> {
    let data = bincode::encode_to_vec(message, bincode::config::standard())
        .map_err(|e| PodError::NetworkError(e.to_string()))?;
    sink.send(tungstenite::protocol::Message::Binary(data.into()))
        .await
        .map_err(|e| PodError::NetworkError(e.to_string()))?;
    Ok(())
}
