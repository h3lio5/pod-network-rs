use ed25519_dalek::{Signature, VerifyingKey as PublicKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Transaction {
    pub id: Vec<u8>,
    pub content: Vec<u8>,
    pub sender: PublicKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub tx: Transaction,
    pub timestamp: u64,
    pub sequence_number: u64,
    pub signature: Signature,
    pub replica_id: PublicKey,
    pub session_id: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pod {
    pub transactions: HashMap<Transaction, TransactionData>,
    pub past_perfect_round: u64,
    pub auxiliary_data: Vec<Vote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionData {
    pub r_min: u64,
    pub r_max: u64,
    pub r_conf: Option<u64>,
    pub votes: Vec<Vote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Connect,
    Write(Transaction),
    Vote(Vote),
}
