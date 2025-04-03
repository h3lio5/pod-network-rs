use crate::client::*;
use axum::{extract::State, Json};
use pod_common::{Pod, TransactionId, TransactionStatus};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct WriteRequest {
    content: Vec<u8>,
}

#[derive(Deserialize)]
pub struct TxStatusRequest {
    tx_id: TransactionId,
}

pub async fn write_tx(
    State(client): State<Arc<Client>>,
    Json(req): Json<WriteRequest>,
) -> Result<Json<TransactionId>, String> {
    client
        .write(req.content)
        .await
        .map(Json)
        .map_err(|e| e.to_string())
}

pub async fn read_pod(State(client): State<Arc<Client>>) -> Result<Json<Pod>, String> {
    client.read().await.map(Json).map_err(|e| e.to_string())
}

pub async fn get_tx_status(
    State(client): State<Arc<Client>>,
    Json(req): Json<TxStatusRequest>,
) -> Result<Json<TransactionStatus>, String> {
    client
        .get_tx_status(req.tx_id)
        .await
        .map(Json)
        .map_err(|e| e.to_string())
}

pub async fn metrics() -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let mut buffer = Vec::new();
    encoder.encode(&prometheus::gather(), &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}
