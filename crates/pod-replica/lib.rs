// use ed25519_dalek::VerifyingKey as PublicKey;
// use lazy_static::lazy_static;
// use pod_common::{Crypto, Message, NetworkTrait, PodError, Transaction, Vote};
// use prometheus::{Gauge, Registry};
// use std::collections::{HashMap, HashSet};
// use std::sync::Arc;
// use tokio::sync::Mutex;
// use tracing::info;

// lazy_static! {
//     static ref REGISTRY: Registry = Registry::new();
//     static ref REPLICA_LOG_SIZE: Gauge =
//         Gauge::new("replica_log_size", "Size of replica log").unwrap();
// }

// pub fn init_metrics() {
//     REGISTRY
//         .register(Box::new(REPLICA_LOG_SIZE.clone()))
//         .unwrap();
// }

// pub struct Replica {
//     id: PublicKey,
//     crypto: Crypto,
//     state: Arc<Mutex<ReplicaState>>,
//     network: Arc<Box<dyn NetworkTrait>>,
// }

// struct ReplicaState {
//     sequence_number: u64,
//     log: HashMap<Vec<u8>, Vote>,
//     clients: HashSet<PublicKey>,
// }

// impl Replica {
//     pub fn new(network: Arc<Box<dyn NetworkTrait>>, seed: &[u8; 32]) -> Result<Self, PodError> {
//         let crypto = Crypto::from_secret_key(seed)?;
//         init_metrics();
//         Ok(Replica {
//             id: crypto.public_key(),
//             crypto,
//             state: Arc::new(Mutex::new(ReplicaState {
//                 sequence_number: 0,
//                 log: HashMap::new(),
//                 clients: HashSet::new(),
//             })),
//             network,
//         })
//     }

//     pub async fn run(&self, address: &str) -> Result<(), PodError> {
//         self.network.listen(address).await?;
//         let mut rx = self.network.subscribe();
//         while let Ok(message) = rx.recv().await {
//             match message {
//                 Message::Connect => {
//                     info!(
//                         "Replica {} received CONNECT",
//                         hex::encode(self.id.to_bytes())
//                     );
//                     self.state.lock().await.clients.insert(self.id.clone());
//                     self.broadcast_log().await?;
//                 }
//                 Message::Write(tx) => {
//                     self.handle_write(tx).await?;
//                 }
//                 _ => {}
//             }
//         }
//         Ok(())
//     }

//     async fn handle_write(&self, tx: Transaction) -> Result<(), PodError> {
//         let mut state = self.state.lock().await;
//         if state.log.contains_key(&tx.id) {
//             return Ok(()); // Duplicate transaction
//         }

//         let timestamp = self.current_round();
//         let sn = state.sequence_number;
//         state.sequence_number += 1;

//         let message = bincode::encode_to_vec(&(&tx, timestamp, sn), bincode::config::standard())
//             .map_err(|e| PodError::NetworkError(e.to_string()))?;
//         let signature = self.crypto.sign(&message).to_vec();
//         let vote = Vote {
//             tx,
//             timestamp,
//             sequence_number: sn,
//             signature,
//             replica_id: self.id.to_bytes().to_vec(),
//         };
//         state.log.insert(vote.tx.id.clone(), vote.clone());
//         REPLICA_LOG_SIZE.set(state.log.len() as f64);
//         self.network.broadcast(Message::Vote(vote)).await?;
//         Ok(())
//     }

//     async fn broadcast_log(&self) -> Result<(), PodError> {
//         let state = self.state.lock().await;
//         for vote in state.log.values() {
//             self.network.broadcast(Message::Vote(vote.clone())).await?;
//         }
//         Ok(())
//     }

//     fn current_round(&self) -> u64 {
//         std::time::SystemTime::now()
//             .duration_since(std::time::UNIX_EPOCH)
//             .unwrap()
//             .as_secs()
//     }
// }
