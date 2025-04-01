use bincode::{Decode, Encode};
use ed25519_dalek::{Signature, VerifyingKey as PublicKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Encode, Decode)]
pub struct Transaction {
    pub id: Vec<u8>,
    pub content: Vec<u8>,
    pub sender: Vec<u8>,
}

pub type TransactionId = Vec<u8>;
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
}
