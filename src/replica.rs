//! Replica module implementing the core HotStuff consensus protocol logic.
//!
//! This module contains the Replica struct which maintains consensus state,
//! handles proposals and votes, forms QCs, and detects commits using the 3-chain rule.

use std::collections::{BTreeMap, BTreeSet};
use crate::types::*;
use crate::config::*;
use crate::wal::{WAL, LogEntry, ViewChangeReason};
use crate::snapshot::Snapshot;
use std::path::Path;

/// Replica structure for the consensus protocol.
///
/// Each replica maintains:
/// - Configuration (n, f, id)
/// - Current view number
/// - Block tree storing all seen blocks
/// - Highest QC observed (high_qc)
/// - Vote pool for collecting votes on blocks
/// - Committed log of finalized blocks
/// - Pacemaker state for view synchronization
/// - KeyStore for Ed25519 cryptographic operations
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
    /// Current active validator set used for leader election and quorum.
    pub active_validators: BTreeSet<ReplicaId>,
    /// Membership configuration epoch.
    pub config_epoch: u64,
    pub keystore: KeyStore,
    /// Optional WAL handle — `None` means persistence is disabled
    /// (e.g. during unit tests that don't need durability).
    pub wal: Option<WAL>,
    /// Monotonically increasing snapshot sequence number.
    pub snapshot_counter: u64,
}

use crate::visualiser::{print_block_tree_enhanced, print_commit_log, 
                        print_replica_stats, print_view_timeline, ReplicaVisualizationData};
use crate::crypto::{Signature, KeyStore, sign, verify, verify_qc};

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

    /// Current validator count under active configuration.
    pub fn dynamic_n(&self) -> usize {
        self.active_validators.len()
    }

    /// Current Byzantine fault threshold under active configuration.
    pub fn dynamic_f(&self) -> usize {
        if self.dynamic_n() == 0 {
            return 0;
        }
        (self.dynamic_n().saturating_sub(1)) / 3
    }

    /// Dynamic quorum size 2f+1 for the active validator set.
    pub fn dynamic_quorum_size(&self) -> usize {
        2 * self.dynamic_f() + 1
    }

    /// Dynamic leader selection over sorted active validator IDs.
    pub fn dynamic_leader_for_view(&self, view: u64) -> ReplicaId {
        if self.active_validators.is_empty() {
            return self.config.id;
        }
        let idx = (view as usize) % self.active_validators.len();
        self.active_validators
            .iter()
            .nth(idx)
            .copied()
            .unwrap_or(self.config.id)
    }

    /// Set initial validator set from config (0..n-1) if empty.
    pub fn ensure_default_validators(&mut self) {
        if !self.active_validators.is_empty() {
            return;
        }
        for id in 0..self.config.n {
            self.active_validators.insert(id as ReplicaId);
        }
    }

    fn can_apply_new_validator_count(&self, new_n: usize) -> bool {
        if new_n < 4 {
            return false;
        }
        new_n % 3 == 1
    }

    /// Handle an incoming vote from another replica.
    ///
    /// Cryptographically verifies the Ed25519 signature on the vote using the
    /// KeyStore, validates the block hash exists, and adds it to the vote pool.
    /// Attempts to form a QC if enough votes have been collected.
    ///
    /// # Arguments
    /// * `vote` - The vote to process
    ///
    /// # Returns
    /// Some(QuorumCert) if a QC was formed, None otherwise
    pub fn handle_vote(&mut self, vote: Vote) -> Option<QuorumCert> {
        if vote.epoch != self.config_epoch {
            return None;
        }

        if !self.active_validators.contains(&vote.signature.signer) {
            return None;
        }

        // Cryptographically verify the Ed25519 signature against the signer's public key
        if !self.keystore.verify(&vote.signature, vote.block_hash, vote.view, vote.epoch) {
            return None;
        }

        let sig = vote.signature;
        
        // Check if signer ID is within valid range (0 to n-1)
        if sig.signer >= self.config.n as u64 {
            return None;
        }
        
        // Check if the block_hash being voted on actually exists in our block tree
        if !self.block_tree.contains_key(&vote.block_hash) {
            return None;
        }

        if let Some(b) = self.block_tree.get(&vote.block_hash) {
            if b.epoch != vote.epoch {
                return None;
            }
        }

        let entry = self.vote_pool.entry(vote.block_hash).or_insert_with(Vec::new);

        // Check for duplicate vote from same signer
        if entry.iter().any(|s| s.signer == sig.signer) {
            return None;
        }

        entry.push(sig);

        self.try_form_qc(vote.block_hash, vote.view, vote.epoch)
    }

    /// Try to form a QC if enough votes have been collected.
    ///
    /// Checks if the vote pool has at least quorum_size signatures for the block.
    ///
    /// # Arguments
    /// * `block_hash` - The block to check
    /// * `view` - The view number
    ///
    /// # Returns
    /// Some(QuorumCert) if quorum reached, None otherwise
    fn try_form_qc(&mut self, block_hash: Hash, view: u64, epoch: u64) -> Option<QuorumCert> {
        if let Some(sigs) = self.vote_pool.get(&block_hash) {
            if sigs.len() >= self.dynamic_quorum_size() {
                let qc = QuorumCert { block_hash, view, epoch, signatures: sigs.clone() };
                self.high_qc = Some(qc.clone());

                // ── WAL: log QC formation + high_qc update ──
                if let Some(ref mut wal) = self.wal {
                    let _ = wal.append(&LogEntry::QCFormed {
                        block_hash,
                        view,
                        epoch,
                        signer_count: sigs.len(),
                        timestamp: WAL::now_ms(),
                    });
                    let _ = wal.append(&LogEntry::HighQCUpdated {
                        block_hash,
                        view,
                        epoch,
                        timestamp: WAL::now_ms(),
                    });
                }

                return Some(qc);
            }
        }
        None
    }

    /// Simulate receiving a vote from another replica.
    ///
    /// Creates a cryptographically signed Vote and processes it through handle_vote.
    /// Note: This method can only be used by the replica whose ID matches replica_id,
    /// as each KeyStore only contains its own private key.
    ///
    /// # Arguments
    /// * `replica_id` - The ID of the voting replica (must match this replica's ID)
    /// * `block_hash` - The block being voted on
    /// * `view` - The view number
    ///
    /// # Returns
    /// Some(QuorumCert) if this vote completes a QC, None otherwise
    pub fn receive_vote_from_replica(&mut self, replica_id: ReplicaId, block_hash: Hash, view: u64) -> Option<QuorumCert> {
        // Verify this replica can only sign votes as itself
        if replica_id != self.config.id {
            return None;
        }
        
        let signature = self.keystore.sign(block_hash, view, self.config_epoch);
        let vote = Vote { block_hash, view, epoch: self.config_epoch, signature };
        self.handle_vote(vote)
    }

    /// Apply a high QC update learned from the network.
    ///
    /// Returns true if the local high_qc advanced, false otherwise.
    pub fn apply_high_qc_from_network(&mut self, qc: QuorumCert) -> bool {
        let should_update = match &self.high_qc {
            Some(current) => (qc.epoch, qc.view) > (current.epoch, current.view),
            None => true,
        };

        if !should_update {
            return false;
        }

        self.high_qc = Some(qc.clone());

        if let Some(ref mut wal) = self.wal {
            let _ = wal.append(&LogEntry::HighQCUpdated {
                block_hash: qc.block_hash,
                view: qc.view,
                epoch: qc.epoch,
                timestamp: WAL::now_ms(),
            });
        }

        true
    }

    /// Check if this replica is the leader for a given view.
    ///
    /// # Arguments
    /// * `view` - The view number to check
    ///
    /// # Returns
    /// True if this replica is the leader for the view
    pub fn is_leader(&self, view: u64) -> bool {
        self.dynamic_leader_for_view(view) == self.config.id
    }

    /// Propose a new block if this replica is the leader.
    ///
    /// Creates a new block extending the highest QC or latest block.
    /// Only succeeds if this replica is the leader for the view.
    ///
    /// # Arguments
    /// * `view` - The view number for the proposal
    ///
    /// # Returns
    /// Some(Block) if proposal succeeds, None if not the leader
    pub fn propose(&mut self, view: u64) -> Option<Block> {
        if !self.is_leader(view) {
            return None;
        }

        let hash = self.next_hash;
        self.next_hash = self.next_hash.wrapping_add(1);

        let parent = if let Some(qc) = &self.high_qc { Some(qc.block_hash) } else { self.latest_block_hash() };

        let block = Block {
            hash,
            parent,
            view,
            epoch: self.config_epoch,
            proposer: self.config.id,
            qc: self.high_qc.clone(),
            command: ConsensusCommand::NoOp,
        };
        self.block_tree.insert(hash, block.clone());
        Some(block)
    }

    /// Propose a command block (client tx or membership reconfiguration).
    pub fn propose_command(&mut self, view: u64, command: ConsensusCommand) -> Option<Block> {
        if !self.is_leader(view) {
            return None;
        }

        let hash = self.next_hash;
        self.next_hash = self.next_hash.wrapping_add(1);
        let parent = if let Some(qc) = &self.high_qc { Some(qc.block_hash) } else { self.latest_block_hash() };

        let block = Block {
            hash,
            parent,
            view,
            epoch: self.config_epoch,
            proposer: self.config.id,
            qc: self.high_qc.clone(),
            command,
        };
        self.block_tree.insert(hash, block.clone());
        Some(block)
    }

    /// Propose a new block with a specific hash (for multi-replica simulations).
    ///
    /// Similar to `propose()` but uses a globally unique hash provided by the network
    /// to prevent hash collisions between replicas.
    ///
    /// # Arguments
    /// * `view` - The view number for the proposal
    /// * `unique_hash` - A globally unique hash for this block
    ///
    /// # Returns
    /// Some(Block) if proposal succeeds, None if not the leader
    pub fn propose_with_hash(&mut self, view: u64, unique_hash: Hash) -> Option<Block> {
        if !self.is_leader(view) {
            return None;
        }

        let parent = if let Some(qc) = &self.high_qc { Some(qc.block_hash) } else { self.latest_block_hash() };

        let block = Block { 
            hash: unique_hash, 
            parent, 
            view, 
            epoch: self.config_epoch,
            proposer: self.config.id, 
            qc: self.high_qc.clone(),
            command: ConsensusCommand::NoOp,
        };
        self.block_tree.insert(unique_hash, block.clone());
        Some(block)
    }

    /// Validate and insert an incoming block proposal.
    ///
    /// Checks:
    /// - Proposer is the expected leader for the view
    /// - Parent block exists in the tree
    /// - Embedded QC is valid (if present) — verified cryptographically via KeyStore
    ///
    /// # Arguments
    /// * `block` - The proposed block to validate
    ///
    /// # Returns
    /// True if the block is valid and inserted, false otherwise
    pub fn validate_and_insert_proposal(&mut self, block: Block) -> bool {
        // Check proposer is the expected leader for the view
        let expected = self.dynamic_leader_for_view(block.view);
        if block.proposer != expected {
            return false;
        }

        if block.epoch != self.config_epoch {
            return false;
        }

        // If block has a parent, ensure the parent exists in our tree
        if let Some(parent_hash) = block.parent {
            if !self.block_tree.contains_key(&parent_hash) {
                // Parent missing; reject proposal
                return false;
            }
        }

        // If the block carries a QC, validate it cryptographically
        if let Some(ref qc) = block.qc {
            if qc.epoch != self.config_epoch {
                return false;
            }
            if !self.keystore.verify_qc(qc, self.dynamic_quorum_size()) {
                return false;
            }
            // ensure QC's block exists in our tree (sanity check)
            if !self.block_tree.contains_key(&qc.block_hash) {
                return false;
            }
        }

        // ── WAL: log block insertion BEFORE applying to memory ──
        if let Some(ref mut wal) = self.wal {
            let _ = wal.append(&LogEntry::from_block(&block));
        }

        self.block_tree.insert(block.hash, block);
        self.on_inserting_block_proposal(); // Reset timer on valid proposal
        true
    }

    /// Find the hash of the latest block in the tree (by view number).
    ///
    /// # Returns
    /// Some(Hash) of the latest block, or None if tree is empty
    fn latest_block_hash(&self) -> Option<Hash> {
        self.block_tree.values().max_by_key(|b| b.view).map(|b| b.hash)
    }

    /// Find a block at a specific view number.
    ///
    /// # Arguments
    /// * `view` - The view number to search for
    ///
    /// # Returns
    /// Some(Block) if a block exists at that view, None otherwise
    fn get_block_at_view(&self, view: u64) -> Option<Block> {
        self.block_tree.values().find(|b| b.view == view).cloned()
    }

    /// Detect if a 3-chain exists and return the committed block.
    ///
    /// Searches for pattern: B0 ← B1\[QC\] ← B2\[QC\]
    /// where B1 and B2 both have valid QCs.
    ///
    /// # Returns
    /// Some(Block) representing B0 if a 3-chain is found, None otherwise
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

    /// Execute and commit a block to the committed log.
    ///
    /// Adds the block to committed_log and updates committed_up_to.
    /// Also triggers on_commit() to reset the pacemaker timer.
    ///
    /// # Arguments
    /// * `block` - The block to commit
    pub fn execute_and_commit(&mut self, block: Block) {
        // ── WAL: log commit BEFORE applying ──
        if let Some(ref mut wal) = self.wal {
            let _ = wal.append(&LogEntry::from_committed_block(
                &block,
                self.committed_log.len(),  // this will be the commit_index
            ));
        }

        self.committed_log.push(block.clone());
        self.committed_up_to = Some(block.hash);

        // Apply membership reconfiguration atomically at commit time.
        self.apply_membership_command(&block.command);

        // view change on commit
        self.on_commit();
    }

    fn apply_membership_command(&mut self, command: &ConsensusCommand) {
        match command {
            ConsensusCommand::JoinValidator { replica_id, public_key } => {
                if self.active_validators.contains(replica_id) {
                    return;
                }

                let new_n = self.active_validators.len() + 1;
                if !self.can_apply_new_validator_count(new_n) {
                    return;
                }

                if !self.keystore.add_public_key(*replica_id, public_key) {
                    return;
                }

                self.active_validators.insert(*replica_id);
                self.config_epoch = self.config_epoch.saturating_add(1);
                self.config.n = self.active_validators.len();
                self.config.f = self.dynamic_f();
                self.vote_pool.clear();
            }
            ConsensusCommand::RemoveValidator { replica_id } => {
                if !self.active_validators.contains(replica_id) {
                    return;
                }

                let new_n = self.active_validators.len().saturating_sub(1);
                if !self.can_apply_new_validator_count(new_n) {
                    return;
                }

                self.active_validators.remove(replica_id);
                self.keystore.remove_public_key(*replica_id);
                self.config_epoch = self.config_epoch.saturating_add(1);
                self.config.n = self.active_validators.len();
                self.config.f = self.dynamic_f();
                self.vote_pool.clear();
            }
            _ => {}
        }
    }

    /// Leader helper: propose joining a validator.
    pub fn propose_join_validator(
        &mut self,
        view: u64,
        replica_id: ReplicaId,
        public_key: Vec<u8>,
    ) -> Option<Block> {
        self.propose_command(
            view,
            ConsensusCommand::JoinValidator {
                replica_id,
                public_key,
            },
        )
    }

    /// Leader helper: propose removing a validator.
    pub fn propose_remove_validator(&mut self, view: u64, replica_id: ReplicaId) -> Option<Block> {
        self.propose_command(view, ConsensusCommand::RemoveValidator { replica_id })
    }

    /// Attempt to commit one block using the 3-chain rule.
    ///
    /// Finds the earliest uncommitted block in a 3-chain and commits it.
    ///
    /// # Returns
    /// True if a block was committed, false otherwise
    pub fn try_commit_once(&mut self) -> bool {
            if let Some(block) = self.find_committed_block() {
            self.execute_and_commit(block);
            return true;
        }
        false
    }

    /// Commit all committable blocks in sequence.
    ///
    /// Repeatedly calls try_commit_once() until no more blocks can be committed.
    pub fn commit_all(&mut self) {
        while self.try_commit_once() {}
    }

    // ===== Pacemaker & View Change Methods =====

    /// Get current time in milliseconds.
    ///
    /// Uses system time for timeout calculations.
    ///
    /// # Returns
    /// Current time in milliseconds since UNIX epoch
    pub fn current_time_ms() -> u128 {
        // In a real system, use std::time::SystemTime
        // For testing, we use a deterministic approach
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    }

    /// Start a new view and reset the pacemaker timer.
    ///
    /// # Arguments
    /// * `view` - The new view number to start
    pub fn start_view(&mut self, view: u64) {
        self.current_view = view;
        self.view_start_time = Self::current_time_ms();
    }

    /// Check if the current view has timed out.
    ///
    /// # Returns
    /// True if elapsed time exceeds timeout_ms, false otherwise
    pub fn is_view_timeout(&self) -> bool {
        let elapsed = Self::current_time_ms().saturating_sub(self.view_start_time);
        elapsed >= self.timeout_ms as u128
    }

    /// Handle a view timeout by moving to the next view.
    ///
    /// Increments view, resets timer, and clears view-specific votes.
    pub fn on_view_timeout(&mut self) {
        self.advance_view_with_reason(ViewChangeReason::Timeout);
    }

    /// Advance to the next view and durably record the transition.
    pub fn advance_view_with_reason(&mut self, reason: ViewChangeReason) {
        let old = self.current_view;
        self.current_view = self.current_view.saturating_add(1);

        if let Some(ref mut wal) = self.wal {
            let _ = wal.append(&LogEntry::ViewChanged {
                old_view: old,
                new_view: self.current_view,
                reason,
                timestamp: WAL::now_ms(),
            });
        }

        self.view_start_time = Self::current_time_ms();
        self.vote_pool.clear();
    }

    /// Reset timer when a valid proposal is received (heartbeat).
    ///
    /// Signals that the leader is active and making progress.
    pub fn on_inserting_block_proposal(&mut self) {
        self.view_start_time = Self::current_time_ms();
    }

    /// Reset timer when a block is committed (progress signal).
    ///
    /// Indicates that the protocol is making progress.
    pub fn on_commit(&mut self) {
        self.view_start_time = Self::current_time_ms();
    }

    /// Get the leader for the current view.
    ///
    /// # Returns
    /// The replica ID of the current leader
    pub fn current_leader(&self) -> ReplicaId {
        self.dynamic_leader_for_view(self.current_view)
    }

    /// Check if this replica is the leader for the current view.
    ///
    /// # Returns
    /// True if this replica is the current leader, false otherwise
    pub fn am_i_leader(&self) -> bool {
        self.dynamic_leader_for_view(self.current_view) == self.config.id
    }

    // ===== Persistence & Snapshot Methods =====

    /// Attach a WAL handle to this replica so future mutations are logged.
    pub fn attach_wal(&mut self, wal: WAL) {
        self.wal = Some(wal);
    }

    /// Export state for join catch-up (latest snapshot + WAL tail).
    pub fn export_catchup_state(
        &self,
        data_dir: &Path,
    ) -> Result<(Option<Snapshot>, Vec<LogEntry>), String> {
        let latest_snapshot = match Snapshot::load_latest(self.config.id, data_dir) {
            Ok(s) => Some(s),
            Err(crate::snapshot::SnapshotError::NotFound) => None,
            Err(e) => return Err(format!("snapshot load error: {}", e)),
        };

        let wal_entries = if WAL::exists(self.config.id, data_dir) {
            let mut wal = WAL::open(self.config.id, data_dir).map_err(|e| format!("wal open error: {}", e))?;
            wal.read_all().map_err(|e| format!("wal read error: {}", e))?
        } else {
            Vec::new()
        };

        Ok((latest_snapshot, wal_entries))
    }

    /// Import catch-up state for a newly joined validator.
    pub fn import_catchup_state(
        &mut self,
        snapshot: Option<Snapshot>,
        wal_entries: &[LogEntry],
    ) -> Result<(), String> {
        if let Some(snap) = snapshot {
            self.current_view = snap.current_view;
            self.block_tree = snap
                .block_tree
                .iter()
                .map(|b| (b.hash, b.to_block()))
                .collect();
            self.committed_log = snap.committed_log.iter().map(|b| b.to_block()).collect();
            self.committed_up_to = snap.committed_up_to;
            self.high_qc = snap.high_qc.map(|q| q.to_qc());
            self.next_hash = snap.next_hash;
        }

        for entry in wal_entries {
            match entry {
                LogEntry::BlockInserted {
                    hash,
                    parent,
                    view,
                    proposer,
                    qc_block_hash,
                    qc_view,
                    ..
                } => {
                    let qc = match (qc_block_hash, qc_view) {
                        (Some(bh), Some(v)) => Some(QuorumCert {
                            block_hash: *bh,
                            view: *v,
                            epoch: self.config_epoch,
                            signatures: vec![],
                        }),
                        _ => None,
                    };
                    self.block_tree.insert(
                        *hash,
                        Block {
                            hash: *hash,
                            parent: *parent,
                            view: *view,
                            epoch: self.config_epoch,
                            proposer: *proposer,
                            qc,
                            command: ConsensusCommand::NoOp,
                        },
                    );
                }
                LogEntry::BlockCommitted {
                    hash,
                    parent,
                    view,
                    proposer,
                    ..
                } => {
                    let b = Block {
                        hash: *hash,
                        parent: *parent,
                        view: *view,
                        epoch: self.config_epoch,
                        proposer: *proposer,
                        qc: None,
                        command: ConsensusCommand::NoOp,
                    };
                    self.committed_log.push(b.clone());
                    self.committed_up_to = Some(b.hash);
                }
                LogEntry::HighQCUpdated { block_hash, view, .. } => {
                    self.high_qc = Some(QuorumCert {
                        block_hash: *block_hash,
                        view: *view,
                        epoch: self.config_epoch,
                        signatures: vec![],
                    });
                }
                LogEntry::ViewChanged { new_view, .. } => {
                    self.current_view = *new_view;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Check whether a snapshot should be taken now.
    ///
    /// Current policy: every 100 commits.  This keeps the WAL bounded
    /// and ensures recovery stays fast.
    pub fn should_snapshot(&self) -> bool {
        !self.committed_log.is_empty() && self.committed_log.len() % 100 == 0
    }

    /// Take a snapshot of the current state and truncate the WAL.
    ///
    /// # Arguments
    /// * `data_dir` – directory where snapshots are stored
    ///
    /// # Errors
    /// Propagates I/O errors from snapshot write or WAL truncation.
    pub fn take_snapshot(&mut self, data_dir: &Path) -> Result<(), String> {
        // Capture a point-in-time snapshot of the entire state.
        let snap = Snapshot::capture(
            self.snapshot_counter,
            self.config.id,
            self.current_view,
            self.config_epoch,
            &self.active_validators.iter().copied().collect::<Vec<_>>(),
            &self.block_tree,
            &self.committed_log,
            self.committed_up_to,
            self.high_qc.as_ref(),
            self.next_hash,
        );

        // Write to disk (bincode + SHA-256 sidecar).
        snap.save(data_dir).map_err(|e| format!("{}", e))?;

        // Log the snapshot event in the WAL so that future WAL readers
        // know everything before this point is captured.
        if let Some(ref mut wal) = self.wal {
            let _ = wal.append(&LogEntry::SnapshotTaken {
                snapshot_id: self.snapshot_counter,
                last_committed_hash: self.committed_up_to,
                timestamp: WAL::now_ms(),
            });

            // Now truncate the WAL — all prior entries are in the snapshot.
            wal.truncate_after_snapshot()
                .map_err(|e| format!("{}", e))?;
        }

        // Bump the counter so the next snapshot gets a higher sequence.
        self.snapshot_counter += 1;

        // Delete old snapshots, keeping only the 3 most recent.
        let _ = Snapshot::cleanup_old(self.config.id, data_dir, 3);

        println!("Snapshot #{} taken for replica {}",
                 self.snapshot_counter - 1, self.config.id);

        Ok(())
    }
}

