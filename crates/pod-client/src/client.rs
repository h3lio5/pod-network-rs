use dashmap::DashMap;
use ed25519_dalek::{Signature, VerifyingKey as PublicKey};
use lazy_static::lazy_static;
use pod_common::{
    Crypto, Message, NetworkTrait, Pod, PodError, Transaction, TransactionData, TransactionId,
    TransactionStatus, Vote,
};
use prometheus::{Counter, Registry};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};
use tracing::{info, warn};

lazy_static! {
    static ref REGISTRY: Registry = Registry::new();
    static ref TX_WRITTEN: Counter =
        Counter::new("tx_written_total", "Total transactions written").unwrap();
    static ref TX_CONFIRMED: Counter =
        Counter::new("tx_confirmed_total", "Total transactions confirmed").unwrap();
}

pub fn init_metrics() {
    REGISTRY.register(Box::new(TX_WRITTEN.clone())).unwrap();
    REGISTRY.register(Box::new(TX_CONFIRMED.clone())).unwrap();
}

pub struct Client {
    crypto: Crypto,
    network: Arc<Box<dyn NetworkTrait>>,
    state: Arc<Mutex<ClientState>>,
    alpha: usize,
    beta: usize,
}

struct ClientState {
    mrt: DashMap<PublicKey, u64>,     // Most recent timestamps per replica
    next_sn: DashMap<PublicKey, u64>, // Next expected sequence numbers
    votes: DashMap<Transaction, HashMap<PublicKey, Vote>>, // Votes per transaction
    backlog: DashMap<PublicKey, BTreeMap<u64, Vote>>, // Out-of-order votes
    tx_status: DashMap<TransactionId, TransactionStatus>, // Transaction status
    pod: Pod,                         // Cached pod state
}

impl Client {
    pub fn new(
        network: Arc<Box<dyn NetworkTrait>>,
        alpha: usize,
        beta: usize,
        seed: &[u8; 32],
    ) -> Result<Self, PodError> {
        if alpha < 4 * beta + 1 {
            return Err(PodError::InvalidConfig);
        }
        let crypto = Crypto::from_secret_key(seed)?;
        init_metrics();

        Ok(Client {
            crypto,
            network,
            state: Arc::new(Mutex::new(ClientState {
                mrt: DashMap::new(),
                next_sn: DashMap::new(),
                votes: DashMap::new(),
                backlog: DashMap::new(),
                tx_status: DashMap::new(),
                pod: Pod {
                    transactions: HashMap::new(),
                    past_perfect_round: 0,
                    auxiliary_data: Vec::new(),
                },
            })),
            alpha,
            beta,
        })
    }

    pub async fn run(&mut self, address: &str) -> Result<(), PodError> {
        self.network.listen(address).await?;
        let mut rx = self.network.subscribe();
        // broadcast a Connect message to all the peers
        for replica in self.network.replicas() {
            self.state.lock().await.mrt.insert(replica, 0);
            self.state.lock().await.next_sn.insert(replica, 0);
            self.network
                .send_to_replica(replica.clone(), Message::Connect)
                .await?;
        }
        while let Ok(message) = rx.recv().await {
            if let Message::Vote(vote) = message {
                self.handle_vote(vote).await?;
            }
        }
        Ok(())
    }

    pub async fn write(&self, content: Vec<u8>) -> Result<TransactionId, PodError> {
        let tx = self.crypto.generate_tx(content);
        if self.state.lock().await.tx_status.contains_key(&tx.id) {
            return Ok(tx.id);
        }
        self.network.broadcast(Message::Write(tx.clone())).await?;
        self.state
            .lock()
            .await
            .tx_status
            .insert(tx.id.clone(), TransactionStatus::Pending);
        TX_WRITTEN.inc();
        info!("Wrote transaction {:?}", tx.id);
        Ok(tx.id)
    }

    /// Get the transaction status
    pub async fn get_tx_status(&self, tx_id: TransactionId) -> Result<TransactionStatus, PodError> {
        self.state
            .lock()
            .await
            .tx_status
            .get(&tx_id)
            .map(|status| status.value().clone())
            .ok_or(PodError::UnknownTransaction(hex::encode(tx_id)))
    }
    /// Read the current pod state efficiently
    pub async fn read(&self) -> Result<Pod, PodError> {
        let state = self.state.lock().await;
        Ok(state.pod.clone()) // O(1) lock + O(n) clone
    }

    /// Handle an incoming vote from a replica
    async fn handle_vote(&mut self, vote: Vote) -> Result<(), PodError> {
        let mut state = self.state.lock().await;

        // Verify vote signature
        let message = bincode::encode_to_vec(
            &(&vote.tx, vote.timestamp, vote.sequence_number),
            bincode::config::standard(),
        )
        .map_err(|e| PodError::SerializationError(e.to_string()))?;

        let signature = Signature::from_slice(&vote.signature)
            .map_err(|e| PodError::SerializationError(e.to_string()))?;

        let replica_id =
            PublicKey::from_bytes(&vote.replica_id.clone().try_into().map_err(|_| {
                PodError::SerializationError("Failed to deserialize public key".to_string())
            })?)
            .map_err(|_| {
                PodError::SerializationError("Failed to deserialize public key".to_string())
            })?;
        self.crypto.verify(&message, &signature, &replica_id)?;

        // Check sequence number and handle backlog
        let expected_sn = state.next_sn.get(&replica_id).map_or(0, |v| *v);
        if vote.sequence_number != expected_sn {
            warn!(
                "Out-of-order vote from replica {:?}: sn={} expected={}",
                replica_id, vote.sequence_number, expected_sn
            );
            state
                .backlog
                .entry(replica_id)
                .or_default()
                .insert(vote.sequence_number, vote);
            return Ok(());
        }

        // Apply the vote
        Self::apply_vote(&mut state, vote, replica_id, self.alpha, self.beta)?;

        // Process backlog
        loop {
            let mut backlog = state.backlog.entry(replica_id).or_default();
            let expected_sn = state.next_sn.get(&replica_id).map_or(0, |v| *v);
            if let Some((next_sn, _)) = backlog.first_key_value() {
                if *next_sn == expected_sn {
                    let vote = backlog.pop_first().unwrap().1;
                    drop(backlog);
                    Self::apply_vote(&mut state, vote, replica_id, self.alpha, self.beta)?;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(())
    }

    /// Apply a vote and update pod incrementally
    fn apply_vote(
        state: &mut MutexGuard<ClientState>,
        vote: Vote,
        replica_id: PublicKey,
        alpha: usize,
        beta: usize,
    ) -> Result<(), PodError> {
        // Update sequence number and check timestamp monotonicity
        state.next_sn.insert(replica_id, vote.sequence_number + 1);

        let mrt = state.mrt.get(&replica_id).map_or(0, |v| *v);
        if vote.timestamp < mrt {
            return Err(PodError::ProtocolViolation(
                "Old timestamp".to_string(),
                format!("ReplicaID: {}", hex::encode(replica_id.to_bytes())),
            ));
        }
        state.mrt.insert(replica_id, vote.timestamp);

        // Handle transaction if present
        if let Some(tx) = vote.tx.clone() {
            // Process transaction votes
            let mut tx_votes = state.votes.entry(tx.clone()).or_default();

            // Check for duplicate timestamp
            if tx_votes.contains_key(&replica_id)
                && tx_votes[&replica_id].timestamp != vote.timestamp
            {
                return Err(PodError::ProtocolViolation(
                    "Duplicate timestamp".to_string(),
                    format!("ReplicaID: {}", hex::encode(replica_id.to_bytes())),
                ));
            }

            // Add vote to transaction votes
            tx_votes.insert(replica_id, vote.clone());

            // Process quorum if reached
            if tx_votes.len() >= alpha {
                let timestamps: Vec<u64> = tx_votes.values().map(|v| v.timestamp).collect();
                let r_min = Self::min_possible_ts(&timestamps, alpha, beta)?;
                let r_max = Self::max_possible_ts(&timestamps, alpha, beta)?;
                let r_conf = Self::median(&timestamps);
                let votes: Vec<Vote> = tx_votes.values().cloned().collect();
                let tx_id = tx.id.clone();
                let tx_for_log = tx.clone();

                // Drop tx_votes before state modifications
                drop(tx_votes);

                // Update state with confirmed transaction
                state.pod.auxiliary_data.push(vote);
                state
                    .tx_status
                    .insert(tx_id, TransactionStatus::Confirmed { r_conf });
                state.pod.transactions.insert(
                    tx,
                    TransactionData {
                        r_min,
                        r_max,
                        r_conf: Some(r_conf),
                        votes,
                    },
                );

                TX_CONFIRMED.inc();
                info!(
                    "Confirmed transaction {:?} at round {}",
                    tx_for_log.id, r_conf
                );
            } else {
                // Drop tx_votes before state modification to stop the borrow checker from yelling!
                drop(tx_votes);
                state.pod.auxiliary_data.push(vote);
            }
        }

        // Update past-perfect round
        state.pod.past_perfect_round = Self::compute_r_perf(&state.mrt, alpha, beta)?;

        Ok(())
    }

    /// Compute r_min
    fn min_possible_ts(timestamps: &[u64], alpha: usize, beta: usize) -> Result<u64, PodError> {
        let mut ts = timestamps.to_vec();
        while ts.len() < alpha {
            ts.push(0);
        }
        ts.sort_unstable();
        Ok(ts[alpha / 2 - beta])
    }

    /// Compute r_max
    fn max_possible_ts(timestamps: &[u64], alpha: usize, beta: usize) -> Result<u64, PodError> {
        let mut ts = timestamps.to_vec();
        while ts.len() < alpha {
            ts.push(u64::MAX);
        }
        ts.sort_unstable();
        Ok(ts[ts.len() - alpha + alpha / 2 + beta])
    }

    /// Compute r_perf
    fn compute_r_perf(
        mrt: &DashMap<PublicKey, u64>,
        alpha: usize,
        beta: usize,
    ) -> Result<u64, PodError> {
        let mut mrt_vals: Vec<u64> = mrt.iter().map(|v| *v.value()).collect();
        if mrt_vals.len() < alpha {
            return Err(PodError::InsufficientVotes);
        }
        mrt_vals.sort_unstable();
        Ok(mrt_vals[alpha / 2 - beta])
    }

    /// Compute median for r_conf
    fn median(timestamps: &[u64]) -> u64 {
        let mut sorted = timestamps.to_vec();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    }
}
