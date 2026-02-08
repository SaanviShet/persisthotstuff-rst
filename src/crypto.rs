// This module defines the cryptographic primitives used 
// in the consensus protocol,

use crate::config::ReplicaId;
use crate::types::QuorumCert;
use std::collections::HashSet;

// Boilerplate codes not needed to write so we use derive macro 
// to generate them for us.
// Debug --> Allows formatting a value for debugging purposes 
// using the {:?} formatter in macros like println!
// Clone --> Provides a way to create a deep copy of a value.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub signer: ReplicaId,
}

pub fn sign(id: ReplicaId) -> Signature {
    Signature { signer: id }
}

pub fn verify(_sig: &Signature) -> bool {
    true // placeholder
}

pub fn verify_qc(qc: &QuorumCert, quorum: usize) -> bool {
    let unique: HashSet<ReplicaId> = qc.signatures.iter().map(|s| s.signer).collect();
    unique.len() >= quorum
}
