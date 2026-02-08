use std::collections::HashMap;
use crate::types::*;
use crate::config::*;

// Replica structure for the consensus protocol,
// Each replica maintains its configuration(n, f, id), 
// the current view number,
// a block tree to store the blocks it has seen,
// and the highest QC it has observed.
// It also maintains a vote pool to track votes received for each block hash.
pub struct Replica {
    pub config: Config,
    pub current_view: u64,
    pub block_tree: HashMap<Hash, Block>,
    pub high_qc: Option<QuorumCert>,
    pub vote_pool: HashMap<Hash, Vec<Signature>>,
    pub next_hash: Hash,
    pub committed_log: Vec<Block>,
    pub committed_up_to: Option<Hash>,
}

use crate::visualiser::print_block_tree;
use crate::crypto::{Signature, sign, verify, verify_qc};
use crate::types::*;

impl Replica {
    pub fn visualize(&self) {
        println!("==============================");
        println!("Replica {} | View {}", self.config.id, self.current_view);

        if let Some(qc) = &self.high_qc {
            println!("High QC: Block {} (view {})", qc.block_hash, qc.view);
        } else {
            println!("High QC: None");
        }

        print_block_tree(&self.block_tree);
        println!("==============================");
    }

    // Handles an incoming vote, verifies it, and updates the vote pool.
    pub fn handle_vote(&mut self, vote: Vote) -> Option<QuorumCert> {
        if !verify(&vote.signature) {
            return None;
        }

        let sig = vote.signature;
        let entry = self.vote_pool.entry(vote.block_hash).or_insert_with(Vec::new);

        if entry.iter().any(|s| s.signer == sig.signer) {
            return None;
        }

        entry.push(sig);

        self.try_form_qc(vote.block_hash, vote.view)
    }

    // Checks if enough votes have been collected for a block hash to form a QC.
    fn try_form_qc(&mut self, block_hash: Hash, view: u64) -> Option<QuorumCert> {
        if let Some(sigs) = self.vote_pool.get(&block_hash) {
            if sigs.len() >= self.config.quorum_size() {
                let qc = QuorumCert { block_hash, view, signatures: sigs.clone() };
                self.high_qc = Some(qc.clone());
                return Some(qc);
            }
        }
        None
    }

    // Simulates receiving a vote from another replica, creates a Vote object, and processes it.
    pub fn receive_vote_from_replica(&mut self, replica_id: ReplicaId, block_hash: Hash, view: u64) -> Option<QuorumCert> {
        let signature = sign(replica_id);
        let vote = Vote { block_hash, view, signature };
        self.handle_vote(vote)
    }

    // Determines if this replica is the leader for a given view
    pub fn is_leader(&self, view: u64) -> bool {
        self.config.leader_for_view(view) == self.config.id
    }

    // Proposes a new block if this replica is the leader for the given view. 
    // The new block references the latest QC or 
    // the latest block as its parent.
    pub fn propose(&mut self, view: u64) -> Option<Block> {
        if !self.is_leader(view) {
            return None;
        }

        let hash = self.next_hash;
        self.next_hash = self.next_hash.wrapping_add(1);

        let parent = if let Some(qc) = &self.high_qc { Some(qc.block_hash) } else { self.latest_block_hash() };

        let block = Block { hash, parent, view, proposer: self.config.id, qc: self.high_qc.clone() };
        self.block_tree.insert(hash, block.clone());
        Some(block)
    }

    // Validates an incoming block proposal by checking the proposer, parent existence, and QC validity.
    pub fn validate_and_insert_proposal(&mut self, block: Block) -> bool {
        // Check proposer is the expected leader for the view
        let expected = self.config.leader_for_view(block.view);
        if block.proposer != expected {
            return false;
        }

        // If block has a parent, ensure the parent exists in our tree
        if let Some(parent_hash) = block.parent {
            if !self.block_tree.contains_key(&parent_hash) {
                // Parent missing; reject proposal
                return false;
            }
        }

        // If the block carries a QC, validate it
        if let Some(ref qc) = block.qc {
            if !verify_qc(qc, self.config.quorum_size()) {
                return false;
            }
            // ensure QC's block exists in our tree (sanity check)
            if !self.block_tree.contains_key(&qc.block_hash) {
                return false;
            }
        }

        self.block_tree.insert(block.hash, block);
        true
    }

    // Helper function to find the hash of the latest block in the block tree based on view number.
    fn latest_block_hash(&self) -> Option<Hash> {
        self.block_tree.values().max_by_key(|b| b.view).map(|b| b.hash)
    }

    // Find a block at a specific view number
    fn get_block_at_view(&self, view: u64) -> Option<Block> {
        self.block_tree.values().find(|b| b.view == view).cloned()
    }

    // Detect if a 3-chain exists and return the committed block (B0)
    // Pattern: B0 <- B1 <- B2, where B1 and B2 have valid QCs
    pub fn find_committed_block(&self) -> Option<Block> {
        // Iterate through all blocks to find a 3-chain
        for b0 in self.block_tree.values() {
            // Skip if already committed
            if let Some(committed_hash) = self.committed_up_to {
                if b0.hash == committed_hash {
                    continue;
                }
            }

            // Find B1 (child of B0)
            let b1 = self.block_tree.values().find(|b| b.parent == Some(b0.hash))?;
            
            // B1 must have a QC
            if b1.qc.is_none() {
                continue;
            }

            // Find B2 (child of B1)
            let b2 = self.block_tree.values().find(|b| b.parent == Some(b1.hash))?;
            
            // B2 must have a QC
            if b2.qc.is_none() {
                continue;
            }

            // Valid 3-chain found
            return Some(b0.clone());
        }
        None
    }

    // Execute and commit a block to the log
    pub fn execute_and_commit(&mut self, block: Block) {
        self.committed_log.push(block.clone());
        self.committed_up_to = Some(block.hash);
    }

    // Try to commit once by finding and executing the earliest uncommitted block in a 3-chain
    pub fn try_commit_once(&mut self) -> bool {
            if let Some(block) = self.find_committed_block() {
            self.execute_and_commit(block);
            return true;
        }
        false
    }

    // Commit all possible blocks (repeatedly call try_commit_once)
    pub fn commit_all(&mut self) {
        while self.try_commit_once() {}
    }
}

