//! Core data types for the consensus protocol.
//!
//! This module defines the fundamental structures used in HotStuff consensus:
//! Block, QuorumCert (QC), and Vote.

use crate::config::ReplicaId;
use serde::{Deserialize, Serialize};

pub type Hash = u64;

/// Command payload carried by a block.
///
/// Membership changes are represented as normal consensus commands and
/// are applied atomically only when the enclosing block is committed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsensusCommand {
    NoOp,
    ClientTx(String),
    JoinValidator {
        replica_id: ReplicaId,
        /// Ed25519 public key bytes (32 bytes)
        public_key: Vec<u8>,
    },
    RemoveValidator {
        replica_id: ReplicaId,
    },
}

/// Block structure for the consensus protocol.
///
/// Each block contains:
/// - A unique hash identifier
/// - A reference to its parent block (forming a tree)
/// - The view number when it was proposed
/// - The ID of the proposer replica
/// - An optional quorum certificate (QC)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub hash: Hash,
    pub parent: Option<Hash>,
    pub view: u64,
    /// Membership epoch under which this block was proposed.
    pub epoch: u64,
    pub proposer: ReplicaId,
    pub qc: Option<QuorumCert>,
    pub command: ConsensusCommand,
}

use crate::crypto::Signature;

/// Quorum certificate (QC) structure for the consensus protocol.
///
/// A QC represents agreement from a quorum (2f+1) of replicas.
/// Contains:
/// - The hash of the block being certified
/// - The view number
/// - A vector of signatures from replicas that voted for the block
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuorumCert {
    pub block_hash: u64,
    pub view: u64,
    /// Membership epoch under which signatures were collected.
    pub epoch: u64,
    pub signatures: Vec<Signature>,
}

/// Vote structure for the consensus protocol.
///
/// Represents an individual replica's vote on a proposed block.
/// Contains:
/// - The hash of the block being voted on
/// - The view number
/// - A signature from the voting replica
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vote {
    pub block_hash: u64,
    pub view: u64,
    /// Membership epoch under which the vote was created.
    pub epoch: u64,
    pub signature: Signature,
}

/// Create a dummy QC for testing purposes.
///
/// # Arguments
/// * `hash` - The block hash
/// * `view` - The view number
///
/// # Returns
/// A QuorumCert with no signatures
pub fn dummy_qc(hash: u64, view: u64) -> QuorumCert {
    QuorumCert {
        block_hash: hash,
        view,
        epoch: 0,
        signatures: vec![],
    }
}