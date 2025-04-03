use dashmap::DashMap;
use ed25519_dalek::{Signature, VerifyingKey as PublicKey};
use lazy_static::lazy_static;
use pod_common::{
    Crypto, Message, PeerId, Pod, PodError, Transaction, TransactionData, TransactionId,
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

/// Client state that is shared (with appropriate locks) across tasks.
struct ClientState {
    mrt: DashMap<PublicKey, u64>,     // Most recent timestamps per replica
    next_sn: DashMap<PublicKey, u64>, // Next expected sequence numbers
    votes: DashMap<Transaction, HashMap<PublicKey, Vote>>, // Votes per transaction
    backlog: DashMap<PublicKey, BTreeMap<u64, Vote>>, // Out-of-order votes
    tx_status: DashMap<TransactionId, TransactionStatus>, // Transaction status
    pod: Pod,                         // Cached pod state
}

/// The client that uses a network to connect to replicas and process messages.
pub struct Client {
    crypto: Crypto,
    network: Arc<crate::network::ClientNetwork>,
    state: Arc<Mutex<ClientState>>,
    alpha: usize,
    beta: usize,
}

impl Client {
    pub fn new(
        network: Arc<crate::network::ClientNetwork>,
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

    /// Connects to replicas (provided as (PublicKey, URL) pairs) and spawns tasks to handle incoming messages.
    pub async fn run(&self, replicas: HashMap<PublicKey, String>) -> Result<(), PodError> {
        // For each replica, connect and initialize state.
        for (replica_pk, replica_url) in replicas.into_iter() {
            {
                let state = self.state.lock().await;
                state.mrt.insert(replica_pk, 0);
                state.next_sn.insert(replica_pk, 0);
            }
            self.network
                .connect_to_replica(replica_pk, replica_url)
                .await?;
            self.network
                .send_to_replica(replica_pk, Message::Connect)
                .await?;
        }

        // Listen for broadcast messages and handle votes.
        let mut rx = self.network.subscribe();
        while let Ok(message) = rx.recv().await {
            if let Message::Vote(vote) = message {
                self.handle_vote(vote).await?;
            }
        }
        Ok(())
    }

    pub async fn write(&self, content: Vec<u8>) -> Result<TransactionId, PodError> {
        let tx = self.crypto.generate_tx(content);
        let state = self.state.lock().await;
        if state.tx_status.contains_key(&tx.id) {
            return Ok(tx.id);
        }
        self.network.broadcast(Message::Write(tx.clone())).await?;
        state.tx_status.insert(tx.id, TransactionStatus::Pending);
        TX_WRITTEN.inc();
        info!("Wrote transaction {:?}", tx.id);
        Ok(tx.id)
    }

    /// Get the transaction status.
    pub async fn get_tx_status(&self, tx_id: TransactionId) -> Result<TransactionStatus, PodError> {
        let state = self.state.lock().await;
        state
            .tx_status
            .get(&tx_id)
            .map(|status| status.value().clone())
            .ok_or(PodError::UnknownTransaction(hex::encode(tx_id)))
    }

    /// Read the current pod state.
    pub async fn read(&self) -> Result<Pod, PodError> {
        let state = self.state.lock().await;
        Ok(state.pod.clone())
    }

    /// Handle an incoming vote from a replica.
    async fn handle_vote(&self, vote: Vote) -> Result<(), PodError> {
        let mut state = self.state.lock().await;

        // Verify vote signature.
        let message = bincode::encode_to_vec(
            (&vote.tx, vote.timestamp, vote.sequence_number),
            bincode::config::standard(),
        )
        .map_err(|e| PodError::SerializationError(e.to_string()))?;
        let signature = Signature::from_slice(&vote.signature)
            .map_err(|e| PodError::SerializationError(e.to_string()))?;
        let replica_id = PeerId::raw_replica_pubkey_from_bytes(&vote.replica_id)?;
        Crypto::verify(&message, &signature, &replica_id)?;

        // Check sequence number and handle backlog.
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

        // Apply the vote.
        Self::apply_vote(&mut state, vote.clone(), replica_id, self.alpha, self.beta)?;

        // Process backlog.
        loop {
            let mut backlog = state.backlog.entry(replica_id).or_default();
            let expected_sn = state.next_sn.get(&replica_id).map_or(0, |v| *v);
            if let Some((&next_sn, _)) = backlog.iter().next() {
                if next_sn == expected_sn {
                    let vote = backlog.remove(&next_sn).unwrap();
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

    /// Apply a vote and update pod incrementally.
    fn apply_vote(
        state: &mut MutexGuard<ClientState>,
        vote: Vote,
        replica_id: PublicKey,
        alpha: usize,
        beta: usize,
    ) -> Result<(), PodError> {
        state.next_sn.insert(replica_id, vote.sequence_number + 1);
        let mrt = state.mrt.get(&replica_id).map_or(0, |v| *v);
        if vote.timestamp < mrt {
            return Err(PodError::ProtocolViolation(
                "Old timestamp".to_string(),
                format!("ReplicaID: {}", hex::encode(replica_id.to_bytes())),
            ));
        }
        state.mrt.insert(replica_id, vote.timestamp);

        if let Some(tx) = vote.tx.clone() {
            let mut tx_votes = state.votes.entry(tx.clone()).or_default();
            if tx_votes.contains_key(&replica_id)
                && tx_votes[&replica_id].timestamp != vote.timestamp
            {
                return Err(PodError::ProtocolViolation(
                    "Duplicate timestamp".to_string(),
                    format!("ReplicaID: {}", hex::encode(replica_id.to_bytes())),
                ));
            }
            tx_votes.insert(replica_id, vote.clone());
            if tx_votes.len() >= alpha {
                let timestamps: Vec<u64> = tx_votes.values().map(|v| v.timestamp).collect();
                let r_min = Self::min_possible_ts(&timestamps, alpha, beta)?;
                let r_max = Self::max_possible_ts(&timestamps, alpha, beta)?;
                let r_conf = Self::median(&timestamps);
                let votes: Vec<Vote> = tx_votes.values().cloned().collect();
                let tx_id = tx.id;
                drop(tx_votes);
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
                info!("Confirmed transaction {:?} at round {}", tx_id, r_conf);
            } else {
                drop(tx_votes);
                state.pod.auxiliary_data.push(vote);
            }
        }
        state.pod.past_perfect_round = Self::compute_r_perf(&state.mrt, alpha, beta)?;
        Ok(())
    }

    fn min_possible_ts(timestamps: &[u64], alpha: usize, beta: usize) -> Result<u64, PodError> {
        let mut ts = timestamps.to_vec();
        while ts.len() < alpha {
            ts.push(0);
        }
        ts.sort_unstable();
        Ok(ts[alpha / 2 - beta])
    }

    fn max_possible_ts(timestamps: &[u64], alpha: usize, beta: usize) -> Result<u64, PodError> {
        let mut ts = timestamps.to_vec();
        while ts.len() < alpha {
            ts.push(u64::MAX);
        }
        ts.sort_unstable();
        Ok(ts[ts.len() - alpha + alpha / 2 + beta])
    }

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

    fn median(timestamps: &[u64]) -> u64 {
        let mut sorted = timestamps.to_vec();
        sorted.sort_unstable();
        sorted[sorted.len() / 2]
    }
}
