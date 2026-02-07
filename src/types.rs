use crate::config::ReplicaId;

pub type Hash = u64;

// Block structure for the consensus protocol,
// Each block contains a hash, a reference to its parent block,
// the view number, the ID of the proposer, 
// and an optional quorum certificate (QC).
#[derive(Clone)]
pub struct Block {
    pub hash: Hash,
    pub parent: Option<Hash>,
    pub view: u64,
    pub proposer: ReplicaId,
    pub qc: Option<QuorumCert>,
}

use crate::crypto::Signature;

// Quorum certificate (QC) structure for the consensus protocol,
// Each QC contains the hash of the block it certifies,
// the view number, and a 
// vector of signatures from the replicas that signed it.

#[derive(Clone)]
pub struct QuorumCert {
    pub block_hash: u64,
    pub view: u64,
    pub signatures: Vec<Signature>,
}

// Vote structure for the consensus protocol,
// Each vote contains the hash of the block being voted on,
// the view number, and a 
// signature from the voter.

pub struct Vote {
    pub block_hash: u64,
    pub view: u64,
    pub signature: Signature,
}

pub fn dummy_qc(hash: u64, view: u64) -> QuorumCert {
    QuorumCert {
        block_hash: hash,
        view,
        signatures: vec![],
    }
}