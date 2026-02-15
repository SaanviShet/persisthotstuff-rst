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

/// Key store managing Ed25519 key pairs for all replicas.
///
/// Maps each replica ID to its signing (private) key and verifying (public) key.
/// In a real system, each replica would only hold its own private key and
/// the public keys of all other replicas. Here, for simulation convenience,
/// the KeyStore holds all key pairs centrally.
#[derive(Clone)]
pub struct KeyStore {
    signing_keys: HashMap<ReplicaId, SigningKey>,
    verifying_keys: HashMap<ReplicaId, VerifyingKey>,
}

impl KeyStore {
    /// Create a new KeyStore with Ed25519 key pairs for `n` replicas (IDs 0..n-1).
    ///
    /// # Arguments
    /// * `n` - The number of replicas to generate keys for
    ///
    /// # Returns
    /// A new KeyStore with freshly generated key pairs
    pub fn new(n: usize) -> Self {
        let mut signing_keys = HashMap::new();
        let mut verifying_keys = HashMap::new();
        let mut rng = rand::thread_rng();

        for id in 0..n {
            let sk = SigningKey::generate(&mut rng);
            let vk = sk.verifying_key();
            signing_keys.insert(id as ReplicaId, sk);
            verifying_keys.insert(id as ReplicaId, vk);
        }

        KeyStore {
            signing_keys,
            verifying_keys,
        }
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

    /// Sign a vote (block_hash, view) with the private key of the given replica.
    ///
    /// # Arguments
    /// * `signer_id` - The replica ID that is signing
    /// * `block_hash` - The block hash being voted on
    /// * `view` - The view number
    ///
    /// # Returns
    /// A `Signature` containing the signer ID and Ed25519 signature bytes
    ///
    /// # Panics
    /// Panics if the signer_id is not in the KeyStore (no key pair exists)
    pub fn sign(&self, signer_id: ReplicaId, block_hash: u64, view: u64) -> Signature {
        let msg = Self::vote_message(block_hash, view);
        let sk = self.signing_keys
            .get(&signer_id)
            .unwrap_or_else(|| panic!("No signing key for replica {}", signer_id));
        let sig = sk.sign(&msg);
        Signature {
            signer: signer_id,
            bytes: sig.to_bytes(),
        }
    }

    /// Verify a signature against the public key of the claimed signer.
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
        let vk = match self.verifying_keys.get(&sig.signer) {
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
