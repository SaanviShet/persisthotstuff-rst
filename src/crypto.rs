//! Cryptographic primitives module for the consensus protocol.
//!
//! This module provides Ed25519-based cryptographic operations including
//! key generation, signature creation, verification, and QC validation.
//!
//! Each replica has an Ed25519 key pair. Signatures are produced over the
//! message content (block_hash ++ view) using the signer's private key,
//! and verified against the signer's public key stored in the KeyStore.

use crate::config::ReplicaId;
use crate::types::QuorumCert;
use std::collections::{HashMap, HashSet};
use ed25519_dalek::{SigningKey, VerifyingKey, Signer, Verifier, ed25519::signature::SignerMut};
use sha2::{Sha256, Digest};

/// Signature structure representing a replica's cryptographic signature.
///
/// Contains the signer's replica ID and the raw Ed25519 signature bytes (64 bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub signer: ReplicaId,
    pub bytes: [u8; 64],
}

/// Key store managing Ed25519 keys for a single replica.
///
/// Each replica holds:
/// - Its own private signing key (for creating signatures)
/// - Public keys of all replicas in the network (for verification)
///
/// This models a realistic distributed system where each node only
/// has access to its own private key, but knows the public keys of
/// all other participants.
#[derive(Clone)]
pub struct KeyStore {
    /// This replica's ID
    my_id: ReplicaId,
    /// This replica's private signing key (only this replica knows this)
    my_signing_key: SigningKey,
    /// Public keys of all replicas (including this one)
    public_keys: HashMap<ReplicaId, VerifyingKey>,
}

impl KeyStore {
    /// Create a KeyStore for a specific replica.
    ///
    /// # Arguments
    /// * `my_id` - This replica's ID
    /// * `my_signing_key` - This replica's private key
    /// * `public_keys` - Map of all replicas' public keys (including this replica)
    ///
    /// # Returns
    /// A new KeyStore configured for this specific replica
    pub fn new_for_replica(
        my_id: ReplicaId, 
        my_signing_key: SigningKey, 
        public_keys: HashMap<ReplicaId, VerifyingKey>
    ) -> Self {
        KeyStore {
            my_id,
            my_signing_key,
            public_keys,
        }
    }

    /// Generate key pairs for all replicas (simulation helper).
    ///
    /// This is used during simulation setup to generate all keys.
    /// In production, each replica would generate its own key pair
    /// and distribute its public key through a PKI or configuration.
    ///
    /// # Arguments
    /// * `n` - The number of replicas to generate keys for
    ///
    /// # Returns
    /// A map of (ReplicaId -> (SigningKey, VerifyingKey)) for all replicas
    pub fn generate_keys(n: usize) -> HashMap<ReplicaId, (SigningKey, VerifyingKey)> {
        let mut keys = HashMap::new();
        let mut rng = rand::thread_rng();

        for id in 0..n {
            let sk = SigningKey::generate(&mut rng);
            let vk = sk.verifying_key();
            keys.insert(id as ReplicaId, (sk, vk));
        }

        keys
    }

    /// Create KeyStores for all replicas from a generated key map.
    ///
    /// Each replica gets its own KeyStore with:
    /// - Only its own private key
    /// - All replicas' public keys
    ///
    /// # Arguments
    /// * `all_keys` - Map of all generated key pairs
    ///
    /// # Returns
    /// A vector of KeyStores, one per replica
    pub fn distribute_keys(all_keys: &HashMap<ReplicaId, (SigningKey, VerifyingKey)>) -> Vec<KeyStore> {
        let mut keystores = Vec::new();
        
        // Build the public key map (same for all replicas)
        let public_keys: HashMap<ReplicaId, VerifyingKey> = all_keys
            .iter()
            .map(|(&id, (_sk, vk))| (id, vk.clone()))
            .collect();

        // Create a KeyStore for each replica
        for (&replica_id, (signing_key, _)) in all_keys.iter() {
            let keystore = KeyStore::new_for_replica(
                replica_id,
                signing_key.clone(),
                public_keys.clone(),
            );
            keystores.push(keystore);
        }

        keystores
    }

    /// Build the message bytes that are signed/verified for a vote.
    ///
    /// The message is: SHA-256(block_hash || view), producing a fixed 32-byte digest.
    ///
    /// # Arguments
    /// * `block_hash` - The hash of the block being voted on
    /// * `view` - The view number
    ///
    /// # Returns
    /// A 32-byte digest
    pub fn vote_message(block_hash: u64, view: u64) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(block_hash.to_le_bytes());
        hasher.update(view.to_le_bytes());
        hasher.finalize().to_vec()
    }

    /// Sign a vote (block_hash, view) with this replica's private key.
    ///
    /// # Arguments
    /// * `block_hash` - The block hash being voted on
    /// * `view` - The view number
    ///
    /// # Returns
    /// A `Signature` containing this replica's ID and Ed25519 signature bytes
    pub fn sign(&self, block_hash: u64, view: u64) -> Signature {
        let msg = Self::vote_message(block_hash, view);
        let sig = self.my_signing_key.sign(&msg);
        Signature {
            signer: self.my_id,
            bytes: sig.to_bytes(),
        }
    }

    /// Verify a signature against the public key of the claimed signer.
    ///
    /// Looks up the signer's public key and verifies the signature.
    /// This replica doesn't need to know the signer's private key.
    ///
    /// # Arguments
    /// * `sig` - The signature to verify
    /// * `block_hash` - The block hash that was supposedly voted on
    /// * `view` - The view number
    ///
    /// # Returns
    /// `true` if the signature is valid for the given (block_hash, view) and the
    /// signer's public key; `false` if the signer is unknown or the signature
    /// does not match.
    pub fn verify(&self, sig: &Signature, block_hash: u64, view: u64) -> bool {
        let vk = match self.public_keys.get(&sig.signer) {
            Some(vk) => vk,
            None => return false, // Unknown signer → reject
        };
        let msg = Self::vote_message(block_hash, view);
        let ed_sig = match ed25519_dalek::Signature::from_bytes(&sig.bytes) {
            sig => sig,
        };
        vk.verify(&msg, &ed_sig).is_ok()
    }

    /// Verify that a QC has enough valid, unique signatures.
    ///
    /// Each signature in the QC is verified against the block_hash and view
    /// embedded in the QC itself.
    ///
    /// # Arguments
    /// * `qc` - The QuorumCert to verify
    /// * `quorum` - The required number of unique valid signatures
    ///
    /// # Returns
    /// `true` if the QC has at least `quorum` valid unique signatures
    pub fn verify_qc(&self, qc: &QuorumCert, quorum: usize) -> bool {
        let mut valid_signers: HashSet<ReplicaId> = HashSet::new();
        for sig in &qc.signatures {
            if self.verify(sig, qc.block_hash, qc.view) {
                valid_signers.insert(sig.signer);
            }
        }
        valid_signers.len() >= quorum
    }
}

// ──────────────────────────────────────────────────────────────
// Legacy compatibility helpers (used in tests that construct
// Signatures/QCs by hand without cryptographic content).
// These produce signatures with zeroed bytes that will NOT pass
// real verification — they are only for structural tests.
// ──────────────────────────────────────────────────────────────

/// Create a **dummy** signature for a given replica ID.
///
/// The resulting signature has zeroed bytes and will **not** pass
/// cryptographic verification. Use only for structural tests
/// (e.g. commit-rule tests that do not verify signatures).
pub fn sign(id: ReplicaId) -> Signature {
    Signature {
        signer: id,
        bytes: [0u8; 64],
    }
}

/// Placeholder verify — always returns `true`.
///
/// Retained for legacy tests that call `verify()` directly.
/// The real verification path is `KeyStore::verify()`.
pub fn verify(_sig: &Signature) -> bool {
    true
}

/// Placeholder QC verification — checks only that enough unique
/// signers are present, without verifying actual signatures.
///
/// Retained for legacy tests. The real verification path is
/// `KeyStore::verify_qc()`.
pub fn verify_qc(qc: &QuorumCert, quorum: usize) -> bool {
    let unique: HashSet<ReplicaId> = qc.signatures.iter().map(|s| s.signer).collect();
    unique.len() >= quorum
}
