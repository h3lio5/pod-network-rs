use ed25519_dalek::{Signature, VerifyingKey as PublicKey};
use lazy_static::lazy_static;
use pod_common::{
    Crypto, Message, Network, NetworkTrait, Pod, PodError, Transaction, TransactionData, TransactionId, TransactionStatus, Vote
};
use prometheus::{Counter, Registry};
use std::collections::{HashMap, VecDeque};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
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
    mrt: DashMap<PublicKey, u64>,
    next_sn: DashMap<PublicKey, u64>,
    tsps: DashMap<Transaction, HashMap<PublicKey, u64>>,
    backlog: DashMap<PublicKey, VecDeque<Vote>>,
    votes: DashMap<Transaction, HashMap<PublicKey, Vote>>,
    tx_status: DashMap<TransactionId, TransactionStatus>,
    pod: Pod,
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
                tsps: DashMap::new(),
                backlog: DashMap::new(),
                votes: DashMap::new(),
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

    pub async fn run(
        &self,
        address: &str,
        replicas: Vec<(PublicKey, String)>,
    ) -> Result<(), PodError> {
        self.network.listen(address).await?;
        let mut rx = self.network.subscribe();
        for (replica, _) in &replicas {
            self.state.lock().await.mrt.insert(replica.clone(), 0);
            self.state.lock().await.next_sn.insert(replica.clone(), 0);
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

    pub async fn write(&self, content: Vec<u8>) -> Result<(), PodError> {
        let tx = self.crypto.generate_tx(content);
        self.network.broadcast(Message::Write(tx)).await?;
        TX_WRITTEN.inc();
        Ok(())
    }

    pub async fn read(&self) -> Result<Pod, PodError> {
        let state = self.state.lock().await;
        let mut pod = state.pod.clone();
        for (tx, votes) in &state.tsps {
            let mut timestamps: Vec<u64> = votes.values().copied().collect();
            timestamps.sort_unstable();

            let r_min = self.min_possible_ts(&timestamps)?;
            let r_max = self.max_possible_ts(&timestamps)?;
            let (r_conf, votes_collected) = if timestamps.len() >= self.alpha {
                let median_idx = self.alpha / 2;
                TX_CONFIRMED.inc();
                (
                    Some(timestamps[median_idx]),
                    self.collect_votes(&state.pod, tx),
                )
            } else {
                (None, Vec::new())
            };

            pod.transactions.insert(
                tx.clone(),
                TransactionData {
                    r_min,
                    r_max,
                    r_conf,
                    votes: votes_collected,
                },
            );
        }
        pod.past_perfect_round = self.min_possible_ts_for_new_tx(&state)?;
        Ok(pod)
    }

    async fn handle_vote(&self, vote: Vote) -> Result<(), PodError> {
        let mut state = self.state.lock().await;
        let replica_id = vote.replica_id.clone();
        let message = bincode::encode_to_vec(
            &(&vote.tx, vote.timestamp, vote.sequence_number),
            bincode::config::standard(),
        )
        .map_err(|e| PodError::NetworkError(e.to_string()))?;

        // Convert raw bytes Signature and Replica ID into concrete Signature and PublicKey types
        let signature = Signature::from_slice(&vote.signature)
            .map_err(|e| PodError::SerializationError(e.to_string()))?;
        let replica_id_fixed_bytes: [u8; 32] =
            vote.replica_id.clone().try_into().map_err(|_| {
                PodError::SerializationError(
                    "Failed to deserialize fixed bytes public key from raw bytes".to_string(),
                )
            })?;
        let replica_id = PublicKey::from_bytes(&replica_id_fixed_bytes)
            .map_err(|e| PodError::SerializationError(e.to_string()))?;

        self.crypto.verify(&message, &signature, &replica_id)?;

        let expected_sn = state.next_sn.get(&replica_id).unwrap_or(&0);
        if vote.sequence_number != *expected_sn {
            warn!("Out-of-order vote from replica {:?}", replica_id);
            return Ok(());
        }
        state
            .next_sn
            .insert(replica_id.clone(), vote.sequence_number + 1);

        let mrt = state.mrt.get(&replica_id).unwrap_or(&0);
        if vote.timestamp < *mrt {
            return Err(PodError::ProtocolViolation("Old timestamp".to_string()));
        }
        state.mrt.insert(replica_id.clone(), vote.timestamp);

        let tx_votes = state
            .tsps
            .entry(vote.tx.clone())
            .or_insert_with(HashMap::new);
        if tx_votes.contains_key(&replica_id) && tx_votes[&replica_id] != vote.timestamp {
            return Err(PodError::ProtocolViolation(
                "Duplicate timestamp".to_string(),
            ));
        }
        tx_votes.insert(replica_id, vote.timestamp);
        state.pod.auxiliary_data.push(vote);
        Ok(())
    }

    fn min_possible_ts(&self, timestamps: &[u64]) -> Result<u64, PodError> {
        let mut ts = timestamps.to_vec();
        while ts.len() < self.alpha {
            ts.push(0);
        }
        ts.sort_unstable();
        Ok(ts[self.alpha / 2 - self.beta])
    }

    fn max_possible_ts(&self, timestamps: &[u64]) -> Result<u64, PodError> {
        let mut ts = timestamps.to_vec();
        while ts.len() < self.alpha {
            ts.push(u64::MAX);
        }
        ts.sort_unstable();
        Ok(ts[ts.len() - self.alpha + self.alpha / 2 + self.beta])
    }

    fn min_possible_ts_for_new_tx(&self, state: &ClientState) -> Result<u64, PodError> {
        let mut mrt: Vec<u64> = state.mrt.values().copied().collect();
        mrt.sort_unstable();
        if mrt.len() < self.alpha {
            return Err(PodError::InsufficientVotes);
        }
        Ok(mrt[self.alpha / 2 - self.beta])
    }

    fn collect_votes(&self, pod: &Pod, tx: &Transaction) -> Vec<Vote> {
        pod.auxiliary_data
            .iter()
            .filter(|v| v.tx == *tx)
            .cloned()
            .collect()
    }
}
