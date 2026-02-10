use std::collections::BTreeMap;
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
    pub block_tree: BTreeMap<Hash, Block>,
    pub high_qc: Option<QuorumCert>,
    pub vote_pool: BTreeMap<Hash, Vec<Signature>>,
    pub next_hash: Hash,
    pub committed_log: Vec<Block>,
    pub committed_up_to: Option<Hash>,
    pub timeout_ms: u64,
    pub view_start_time: u128,
}

use crate::visualiser::{print_block_tree_enhanced, print_commit_log, 
                        print_replica_stats, print_view_timeline, ReplicaVisualizationData};
use crate::crypto::{Signature, sign, verify, verify_qc};

impl Replica {
    /// Enhanced visualization with metadata and colors
    pub fn visualize(&self) {
        let committed_hashes: Vec<Hash> = self.committed_log.iter().map(|b| b.hash).collect();
        let high_qc_hash = self.high_qc.as_ref().map(|qc| qc.block_hash);
        
        println!("\n{}", "═".repeat(60));
        println!("Replica R{} | View {}", self.config.id, self.current_view);
        println!("{}", "═".repeat(60));

        if let Some(qc) = &self.high_qc {
            println!("High QC: Block {} (view {})", qc.block_hash, qc.view);
        } else {
            println!("High QC: None");
        }

        print_block_tree_enhanced(&self.block_tree, Some(&committed_hashes), high_qc_hash);
        
        if !self.committed_log.is_empty() {
            print_commit_log(&self.committed_log);
        }
        
        println!("{}", "═".repeat(60));
    }

    /// Get visualization data for this replica
    pub fn get_visualization_data(&self) -> ReplicaVisualizationData {
        ReplicaVisualizationData {
            replica_id: self.config.id,
            current_view: self.current_view,
            block_tree: self.block_tree.clone(),
            high_qc: self.high_qc.clone(),
            committed_log: self.committed_log.clone(),
        }
    }

    /// Display detailed statistics
    pub fn show_stats(&self) {
        print_replica_stats(
            self.config.id,
            self.current_view,
            self.block_tree.len(),
            self.committed_log.len(),
            self.vote_pool.len(),
        );
    }

    /// Display view timeline for this replica
    pub fn show_timeline(&self) {
        let max_view = self.block_tree.values().map(|b| b.view).max().unwrap_or(0);
        print_view_timeline(&self.block_tree, max_view);
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
        self.on_inserting_block_proposal(); // Reset timer on valid proposal
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
            // Skip if already committed (check if in log)
            if self.committed_log.iter().any(|b| b.hash == b0.hash) {
                continue;
            }

            // Find B1 (child of B0)
            let b1 = self.block_tree.values().find(|b| b.parent == Some(b0.hash));
            let b1 = match b1 {
                Some(b) => b,
                None => continue, // No child found, try next candidate
            };
            
            // B1 must have a QC
            if b1.qc.is_none() {
                continue;
            }

            // Find B2 (child of B1)
            let b2 = self.block_tree.values().find(|b| b.parent == Some(b1.hash));
            let b2 = match b2 {
                Some(b) => b,
                None => continue, // No child found, try next candidate
            };
            
            // B2 must have a QC
            if b2.qc.is_none() {
                continue;
            }

            // Valid 3-chain found
            return Some(b0.clone());
        }
        None
    }
// ***************
    // Execute and commit a block to the log
    pub fn execute_and_commit(&mut self, block: Block) {
        self.committed_log.push(block.clone());
        self.committed_up_to = Some(block.hash);
        // view change on commit
        self.on_commit();
    }
    // ******************

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

    // ===== Pacemaker & View Change Methods =====

    // Get current time in milliseconds (mock: returns incrementing counter)
    pub fn current_time_ms() -> u128 {
        // In a real system, use std::time::SystemTime
        // For testing, we use a deterministic approach
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    }

    // Start a new view (reset timer)
    pub fn start_view(&mut self, view: u64) {
        self.current_view = view;
        self.view_start_time = Self::current_time_ms();
    }

    // Check if view has timed out
    pub fn is_view_timeout(&self) -> bool {
        let elapsed = Self::current_time_ms().saturating_sub(self.view_start_time);
        elapsed >= self.timeout_ms as u128
    }

    // Handle a view timeout (move to next view)
    pub fn on_view_timeout(&mut self) {
        self.current_view += 1;
        self.view_start_time = Self::current_time_ms();
        // Clear votes from previous view (votes are view-specific)
        self.vote_pool.clear();
    }

    // Reset timer when receiving a valid proposal (heartbeat)
    pub fn on_inserting_block_proposal(&mut self) {
        self.view_start_time = Self::current_time_ms();
    }

    // Reset timer when a block is committed (progress signal)
    pub fn on_commit(&mut self) {
        self.view_start_time = Self::current_time_ms();
    }

    // Get current leader for this view
    pub fn current_leader(&self) -> ReplicaId {
        self.config.leader_for_view(self.current_view)
    }

    // Check if this replica is the current leader
    pub fn am_i_leader(&self) -> bool {
        self.config.leader_for_view(self.current_view) == self.config.id
    }
}

