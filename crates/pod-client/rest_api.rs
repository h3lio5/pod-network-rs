
use axum::{Json, extract::State};
use pod_common::{Pod, TransactionId, TransactionStatus, PodError, NetworkTrait};
use serde::Deserialize;
use std::sync::Arc;
use crate::client::*;


// REST API request types
#[derive(Deserialize)]
struct WriteRequest {
    content: Vec<u8>,
}

#[derive(Deserialize)]
struct TxStatusRequest {
    tx_id: TransactionId,
}

// // Mock network implementation (replace with real network in production)
// struct MockNetwork {
//     tx: mpsc::Sender<Message>,
//     rx: broadcast::Receiver<Message>,
// }

// REST API handlers
pub(crate) async fn write_tx(
    State(client): State<Arc<Client>>,
    Json(req): Json<WriteRequest>,
) -> Result<Json<TransactionId>, String> {
    let client = client.clone(); 
    client.write(req.content).await.map(Json).map_err(|e| e.to_string())
}

pub(crate) async fn read_pod(
    State(client): State<Arc<Client>>,
) -> Result<Json<Pod>, String> {
    let client = client.clone(); 
    client.read().await.map(Json).map_err(|e| e.to_string())
}

pub(crate) async fn get_tx_status(
    State(client): State<Arc<Client>>,
    Json(req): Json<TxStatusRequest>,
) -> Result<Json<TransactionStatus>, String> {
    let client = client.clone();
    client.get_tx_status(req.tx_id).await.map(Json).map_err(|e| e.to_string())
}

pub(crate) async fn metrics() -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let mut buffer = Vec::new();
    encoder.encode(&prometheus::gather(), &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

