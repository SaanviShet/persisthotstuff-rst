//! Cryptographic primitives module for the consensus protocol.
//!
//! This module provides simplified cryptographic operations including
//! signature creation, verification, and QC validation.
//!
//! Note: This is a placeholder implementation for educational purposes.
//! Production systems should use proper cryptographic libraries.

use crate::config::ReplicaId;
use crate::types::QuorumCert;
use std::collections::HashSet;

/// Signature structure representing a replica's signature.
///
/// Contains the ID of the signing replica.
/// Derives Clone, Debug, PartialEq, and Eq for convenience.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub signer: ReplicaId,
}

/// Create a signature for a given replica ID.
///
/// # Arguments
/// * `id` - The replica ID creating the signature
///
/// # Returns
/// A new Signature instance
pub fn sign(id: ReplicaId) -> Signature {
    Signature { signer: id }
}

/// Verify a signature (placeholder implementation).
///
/// # Arguments
/// * `_sig` - The signature to verify
///
/// # Returns
/// Always returns true in this placeholder implementation
pub fn verify(_sig: &Signature) -> bool {
    true // placeholder
}

/// Verify that a QC has enough valid signatures.
///
/// # Arguments
/// * `qc` - The QuorumCert to verify
/// * `quorum` - The required quorum size
///
/// # Returns
/// True if the QC has at least `quorum` unique signatures
pub fn verify_qc(qc: &QuorumCert, quorum: usize) -> bool {
    let unique: HashSet<ReplicaId> = qc.signatures.iter().map(|s| s.signer).collect();
    unique.len() >= quorum
}
