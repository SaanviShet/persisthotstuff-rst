// This module defines the cryptographic primitives used 
// in the consensus protocol,

use crate::config::ReplicaId;

// Boilerplate codes not needed to write so we use derive macro 
// to generate them for us.
// Debug --> Allows formatting a value for debugging purposes 
// using the {:?} formatter in macros like println!
// Clone --> Provides a way to create a deep copy of a value.

#[derive(Clone, Debug)]
pub struct Signature {
    pub signer: ReplicaId,
}

pub fn sign(id: ReplicaId) -> Signature {
    Signature { signer: id }
}

pub fn verify(_sig: &Signature) -> bool {
    true // placeholder
}
