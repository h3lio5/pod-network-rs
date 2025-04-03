use bincode::{Decode, Encode};
use ed25519_dalek::VerifyingKey as PublicKey;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::PodError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Encode, Decode)]
pub struct Transaction {
    pub id: [u8; 32],
    pub content: Vec<u8>,
    pub sender: Vec<u8>,
}

pub type TransactionId = [u8; 32];

#[derive(Clone, Debug)]
pub enum PeerId {
    ReplicaId(PublicKey),
    ClientId(PublicKey),
}

impl PeerId {
    pub fn from_replica_id_bytes(bytes: &[u8]) -> Result<PeerId, PodError> {
        let replica_id = Self::raw_replica_pubkey_from_bytes(bytes)?;
        Ok(PeerId::ReplicaId(replica_id))
    }

    pub fn raw_replica_pubkey_from_bytes(bytes: &[u8]) -> Result<PublicKey, PodError> {
        let replica_id = PublicKey::from_bytes(&bytes.try_into().map_err(|_| {
            PodError::SerializationError("Failed to deserialize public key".to_string())
        })?)
        .map_err(|_| {
            PodError::SerializationError("Failed to deserialize public key".to_string())
        })?;
        Ok(replica_id)
    }

    pub fn from_client_id_bytes(bytes: &[u8]) -> Result<PeerId, PodError> {
        let client_id = Self::raw_client_pubkey_from_bytes(bytes)?;
        Ok(PeerId::ReplicaId(client_id))
    }

    pub fn raw_client_pubkey_from_bytes(bytes: &[u8]) -> Result<PublicKey, PodError> {
        let client_id = PublicKey::from_bytes(&bytes.try_into().map_err(|_| {
            PodError::SerializationError("Failed to deserialize public key".to_string())
        })?)
        .map_err(|_| {
            PodError::SerializationError("Failed to deserialize public key".to_string())
        })?;
        Ok(client_id)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TransactionStatus {
    Pending,
    Confirmed { r_conf: u64 },
}
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct Vote {
    pub tx: Option<Transaction>,
    pub timestamp: u64,
    pub sequence_number: u64,
    pub signature: Vec<u8>,
    pub replica_id: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pod {
    pub transactions: HashMap<Transaction, TransactionData>,
    pub past_perfect_round: u64,
    pub auxiliary_data: Vec<Vote>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct TransactionData {
    pub r_min: u64,
    pub r_max: u64,
    pub r_conf: Option<u64>,
    pub votes: Vec<Vote>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum Message {
    Connect,
    Write(Transaction),
    Vote(Vote),
    Heartbeat,
}
