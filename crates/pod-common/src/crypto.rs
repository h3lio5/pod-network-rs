use crate::{errors::PodError, types::Transaction};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey as PublicKey};
use sha2::{Digest, Sha256};

pub struct Crypto {
    public_key: PublicKey,
    signing_key: SigningKey,
}

impl Crypto {
    pub fn from_secret_key(secret_key: &[u8; 32]) -> Result<Self, PodError> {
        let signing_key = SigningKey::from_bytes(secret_key);
        let public_key = signing_key.verifying_key();
        Ok(Crypto {
            public_key,
            signing_key,
        })
    }

    pub fn public_key(&self) -> PublicKey {
        self.public_key
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        self.signing_key.sign(message)
    }

    pub fn replica_pubkey_from_bytes(bytes: &[u8]) -> Result<PublicKey, PodError> {
        let replica_id = PublicKey::from_bytes(&bytes.try_into().map_err(|_| {
            PodError::SerializationError("Failed to deserialize public key".to_string())
        })?)
        .map_err(|_| {
            PodError::SerializationError("Failed to deserialize public key".to_string())
        })?;
        Ok(replica_id)
    }

    pub fn verify(
        message: &[u8],
        signature: &Signature,
        public_key: &PublicKey,
    ) -> Result<(), PodError> {
        public_key
            .verify(message, signature)
            .map_err(|_| PodError::InvalidSignature)
    }

    pub fn generate_tx(&self, content: Vec<u8>) -> Transaction {
        let mut hasher = Sha256::new();
        hasher.update(&content);
        let id: [u8; 32] = hasher.finalize().into();
        Transaction {
            id,
            content,
            sender: self.public_key.to_bytes().to_vec(),
        }
    }
}
