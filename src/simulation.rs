//! Simulation module for running multi-replica consensus scenarios.
//!
//! This module provides the simulation environment for executing complete consensus
//! rounds with multiple replicas, including proposal, voting, QC formation, and commit phases.

use crate::config::Config;
use crate::replica::Replica;
use crate::network::{Network, Message};
use crate::types::*;
use crate::crypto::KeyStore;
use crate::recovery::recover;
use crate::wal::{LogEntry, ViewChangeReason};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Multi-replica simulation environment
pub struct Simulation {
    /// All replicas participating in consensus
    pub replicas: Vec<Replica>,
    
    /// Network for message passing
    pub network: Network,
    
    /// Current simulation step
    pub step: usize,
    
    /// Maximum number of steps to simulate
    pub max_steps: usize,
    
    /// Verbose output flag
    pub verbose: bool,

    /// Whether persistence (WAL + snapshots) is enabled for replicas.
    pub persistence_enabled: bool,

    /// Base data directory when persistence is enabled.
    pub data_dir: Option<PathBuf>,

    /// Per-replica data directory for WAL + snapshots.
    replica_data_dirs: Vec<PathBuf>,

    /// Stable per-replica keystore copy used for recovery.
    replica_keystores: Vec<KeyStore>,

    /// Replicas currently crashed and excluded from protocol processing.
    crashed_replicas: HashSet<usize>,

    /// Replicas that are alive but currently non-responsive (fault-injection).
    unresponsive_replicas: HashSet<usize>,

    /// Last simulated timestamp (ms) when each replica responded.
    last_response_ms: Vec<u128>,

    /// Simulator logical clock in milliseconds.
    simulated_time_ms: u128,

    /// Amount of simulated time advanced per round.
    round_time_ms: u64,
}

impl Simulation {
    /// Create a new simulation with n replicas
    pub fn new(n: usize, f: usize, max_steps: usize, verbose: bool) -> Self {
        Self::new_with_options(n, f, max_steps, verbose, false, None)
            .expect("failed to initialize simulation")
    }

    /// Create a new simulation with persistence enabled.
    pub fn new_with_persistence(
        n: usize,
        f: usize,
        max_steps: usize,
        verbose: bool,
        data_dir: impl AsRef<Path>,
    ) -> Result<Self, String> {
        Self::new_with_options(
            n,
            f,
            max_steps,
            verbose,
            true,
            Some(data_dir.as_ref().to_path_buf()),
        )
    }

    fn new_with_options(
        n: usize,
        f: usize,
        max_steps: usize,
        verbose: bool,
        persistence_enabled: bool,
        data_dir: Option<PathBuf>,
    ) -> Result<Self, String> {
        let mut replicas = Vec::new();
        let mut replica_data_dirs = Vec::new();

        // Generate Ed25519 key pairs for all replicas
        let all_keys = KeyStore::generate_keys(n);
        let keystores = KeyStore::distribute_keys(&all_keys);

        let resolved_data_dir = if persistence_enabled {
            let base = data_dir.unwrap_or_else(|| PathBuf::from("data/simulation"));
            std::fs::create_dir_all(&base)
                .map_err(|e| format!("failed to create simulation data dir: {}", e))?;
            Some(base)
        } else {
            None
        };

        for id in 0..n {
            let config = Config {
                n,
                f,
                id: id as u64,
                timeout_ms: 5000,
            };

            let mut replica = Self::fresh_replica(config.clone(), keystores[id].clone());

            if let Some(base) = &resolved_data_dir {
                let replica_dir = base.join(format!("replica_{}", id));
                std::fs::create_dir_all(&replica_dir)
                    .map_err(|e| format!("failed to create replica data dir: {}", e))?;

                let (mut recovered, mut wal) = recover(config.clone(), keystores[id].clone(), &replica_dir)
                    .map_err(|e| format!("failed to initialize persistence for replica {}: {}", id, e))?;

                // Ensure genesis is durably present in WAL for fresh stores.
                if wal.entry_count() == 0 && recovered.block_tree.len() == 1 && recovered.block_tree.contains_key(&0) {
                    let genesis = recovered
                        .block_tree
                        .get(&0)
                        .cloned()
                        .unwrap_or_else(Self::genesis_block);
                    wal.append(&LogEntry::from_block(&genesis))
                        .map_err(|e| format!("failed to log genesis for replica {}: {}", id, e))?;
                }

                recovered.attach_wal(wal);
                replica = recovered;
                replica_data_dirs.push(replica_dir);
            }

            replicas.push(replica);
        }

        Ok(Simulation {
            replicas,
            network: Network::new(n),
            step: 0,
            max_steps,
            verbose,
            persistence_enabled,
            data_dir: resolved_data_dir,
            replica_data_dirs,
            replica_keystores: keystores,
            crashed_replicas: HashSet::new(),
            unresponsive_replicas: HashSet::new(),
            last_response_ms: vec![0; n],
            simulated_time_ms: 0,
            round_time_ms: 1000,
        })
    }

    fn genesis_block() -> Block {
        Block {
            hash: 0,
            parent: None,
            view: 0,
            epoch: 0,
            proposer: 0,
            qc: None,
            command: ConsensusCommand::NoOp,
        }
    }

    fn fresh_replica(config: Config, keystore: KeyStore) -> Replica {
        let validator_ids: std::collections::BTreeSet<_> = (0..config.n as u64).collect();
        let mut replica = Replica {
            config,
            current_view: 0,
            block_tree: BTreeMap::new(),
            high_qc: None,
            vote_pool: BTreeMap::new(),
            next_hash: 1,
            committed_log: Vec::new(),
            committed_up_to: None,
            timeout_ms: 5000,
            view_start_time: Replica::current_time_ms(),
            active_validators: validator_ids,
            config_epoch: 0,
            keystore,
            wal: None,
            snapshot_counter: 0,
            app: None,
            pending_app_state: None,
        client_queue: Vec::new(),
        dummy_proposal_enabled: false,
        last_proposed_time: 0,
        dummy_timeout_ms: 0,
        };
        replica.ensure_default_validators();
        replica.block_tree.insert(0, Self::genesis_block());
        replica
    }

    fn is_crashed(&self, replica_id: usize) -> bool {
        self.crashed_replicas.contains(&replica_id)
    }

    fn is_unresponsive(&self, replica_id: usize) -> bool {
        self.unresponsive_replicas.contains(&replica_id)
    }

    fn mark_responded(&mut self, replica_id: usize) {
        if replica_id < self.last_response_ms.len() {
            self.last_response_ms[replica_id] = self.simulated_time_ms;
        }
    }

    fn advance_simulated_time(&mut self) {
        self.simulated_time_ms = self
            .simulated_time_ms
            .saturating_add(self.round_time_ms as u128);
    }

    fn process_timeout_crashes(&mut self) {
        let mut to_crash = Vec::new();

        for id in 0..self.replicas.len() {
            if self.is_crashed(id) || !self.is_unresponsive(id) {
                continue;
            }

            let elapsed = self
                .simulated_time_ms
                .saturating_sub(*self.last_response_ms.get(id).unwrap_or(&0));

            if elapsed >= self.replicas[id].timeout_ms as u128 {
                to_crash.push(id);
            }
        }

        for id in to_crash {
            if self.verbose {
                println!(
                    "[TIMEOUT] Replica {} marked crashed after {} ms without response",
                    id, self.replicas[id].timeout_ms
                );
            }
            let _ = self.crash_replica(id);
        }
    }

    /// Mark/unmark a live replica as non-responsive (for timeout-based crash simulation).
    pub fn set_replica_unresponsive(&mut self, replica_id: usize, unresponsive: bool) -> Result<(), String> {
        if replica_id >= self.replicas.len() {
            return Err(format!("invalid replica id {}", replica_id));
        }
        if self.is_crashed(replica_id) {
            return Err(format!("replica {} is already crashed", replica_id));
        }

        if unresponsive {
            self.unresponsive_replicas.insert(replica_id);
        } else {
            self.unresponsive_replicas.remove(&replica_id);
            self.mark_responded(replica_id);
        }

        Ok(())
    }
    
    /// Get the current leader based on current view
    pub fn current_leader(&self) -> usize {
        if self.replicas.is_empty() {
            return 0;
        }

        let base_view = self
            .replicas
            .iter()
            .enumerate()
            .filter(|(idx, _)| !self.is_crashed(*idx))
            .map(|(_, r)| r.current_view)
            .max()
            .unwrap_or(0);

        for offset in 0..self.replicas.len() {
            let candidate = ((base_view as usize) + offset) % self.replicas.len();
            if !self.is_crashed(candidate) {
                return candidate;
            }
        }

        0
    }
    
    /// Run one consensus round (propose -> vote -> QC formation -> commit)
    pub fn run_one_round(&mut self) -> bool {
        self.advance_simulated_time();
        self.process_timeout_crashes();

        if self.step >= self.max_steps || self.crashed_replicas.len() == self.replicas.len() {
            return false;
        }

        let crashed = self.crashed_replicas.clone();
        let unresponsive = self.unresponsive_replicas.clone();
        
        let leader_id = self.current_leader();
        let current_view = self.replicas[leader_id].current_view;
        
        if self.verbose {
            let separator = "=".repeat(60);
            println!("\n{}", separator);
            println!("ROUND {} | View {} | Leader: Replica {}", self.step + 1, current_view, leader_id);
            println!("{}", separator);
        }
        
        // Step 1: Leader proposes a block
        if self.verbose {
            println!("\n[Step 1] Leader {} proposing block...", leader_id);
        }

        if self.is_crashed(leader_id) || unresponsive.contains(&leader_id) {
            if self.verbose {
                println!("  [FAIL] Leader {} is unavailable (crashed/unresponsive)", leader_id);
            }

            for (idx, replica) in self.replicas.iter_mut().enumerate() {
                if crashed.contains(&idx) || unresponsive.contains(&idx) {
                    continue;
                }
                replica.on_view_timeout();
                if idx < self.last_response_ms.len() {
                    self.last_response_ms[idx] = self.simulated_time_ms;
                }
            }

            self.step += 1;
            self.process_timeout_crashes();
            return true;
        }
        
        // Generate unique hash from network to avoid collisions
        let unique_hash = self.network.generate_unique_hash();
        let proposal = self.replicas[leader_id].propose_with_hash(current_view, unique_hash);
        if proposal.is_none() {
            // Leader could not propose (e.g. view mismatch after skew).  Treat
            // as a view timeout so live replicas advance and the simulation
            // keeps running — this is NOT a terminal condition.
            if self.verbose {
                println!("  [SKIP] Leader {} could not propose for view {} — view timeout",
                         leader_id, current_view);
            }
            for (idx, replica) in self.replicas.iter_mut().enumerate() {
                if crashed.contains(&idx) || unresponsive.contains(&idx) {
                    continue;
                }
                replica.on_view_timeout();
                if idx < self.last_response_ms.len() {
                    self.last_response_ms[idx] = self.simulated_time_ms;
                }
            }
            self.step += 1;
            self.process_timeout_crashes();
            return true;
        }
        
        let block = proposal.unwrap();
        self.mark_responded(leader_id);
        if self.verbose {
            println!("  ✓ Proposed Block {} (parent: {:?}, view: {})", 
                     block.hash, block.parent, block.view);
        }
        
        // Step 2: Broadcast proposal to all replicas
        if self.verbose {
            println!("\n[Step 2] Broadcasting proposal to all replicas...");
        }
        self.network.broadcast_proposal(leader_id as u64, block.clone());
        
        // Step 3: Each replica validates and votes
        if self.verbose {
            println!("\n[Step 3] Replicas validating and voting...");
        }
        
        // Process all proposal messages
        let mut proposal_messages = Vec::new();
        while self.network.has_messages() {
            if let Some(msg) = self.network.receive() {
                if matches!(msg, Message::Proposal { .. }) {
                    proposal_messages.push(msg);
                }
            }
        }
        
        // Process proposals and generate votes
        for msg in proposal_messages {
            if let Message::Proposal { from, to, block } = msg {
                if crashed.contains(&(to as usize)) || unresponsive.contains(&(to as usize)) {
                    continue;
                }

                let mut vote_to_send: Option<Vote> = None;

                {
                    let replica = &mut self.replicas[to as usize];

                    // Validate and insert proposal
                    let valid = replica.validate_and_insert_proposal(block.clone());

                    if valid {
                        // Create vote while replica mutable borrow is active.
                        let signature = replica.keystore.sign(block.hash, block.view, block.epoch);
                        vote_to_send = Some(Vote {
                            block_hash: block.hash,
                            view: block.view,
                            epoch: block.epoch,
                            signature,
                        });
                    }
                }

                if let Some(vote) = vote_to_send {
                    if (to as usize) < self.last_response_ms.len() {
                        self.last_response_ms[to as usize] = self.simulated_time_ms;
                    }
                    if self.verbose {
                        println!("  ✓ Replica {} validated block {}", to, block.hash);
                    }

                    self.network.send_vote(to, from, vote);
                    if self.verbose {
                        println!("    → Replica {} sent vote to Leader {}", to, from);
                    }
                } else {
                    if self.verbose {
                        println!("  [FAIL] Replica {} rejected block {}", to, block.hash);
                    }
                }
            }
        }
        
        // Step 4: Leader collects votes and forms QC
        if self.verbose {
            println!("\n[Step 4] Leader collecting votes...");
        }
        
        let mut votes_collected = 0;
        let mut qc_formed = None;
        
        while self.network.has_messages() {
            if let Some(msg) = self.network.receive() {
                match msg {
                    Message::Vote { from, to, vote } => {
                        if crashed.contains(&(to as usize)) || unresponsive.contains(&(to as usize)) {
                            continue;
                        }

                        votes_collected += 1;
                        if self.verbose {
                            println!("  ✓ Leader {} received vote from Replica {}", to, from);
                        }

                        let formed_qc = {
                            let leader = &mut self.replicas[to as usize];
                            leader.handle_vote(vote)
                        };

                        if (to as usize) < self.last_response_ms.len() {
                            self.last_response_ms[to as usize] = self.simulated_time_ms;
                        }

                        if let Some(qc) = formed_qc {
                            qc_formed = Some(qc.clone());
                            if self.verbose {
                                println!("  [QC] Formed for block {} with {} signatures!", 
                                         qc.block_hash, qc.signatures.len());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        
        // Step 5: Broadcast QC to all replicas
        if let Some(qc) = qc_formed.clone() {
            if self.verbose {
                println!("\n[Step 5] Broadcasting QC to all replicas...");
            }
            self.network.broadcast_qc(leader_id as u64, qc.clone(), qc.block_hash);
            
            // All replicas update their high_qc
            while self.network.has_messages() {
                if let Some(msg) = self.network.receive() {
                    match msg {
                        Message::QuorumCertBroadcast { to, qc, block_hash, .. } => {
                            if crashed.contains(&(to as usize)) || unresponsive.contains(&(to as usize)) {
                                continue;
                            }

                            let updated = {
                                let replica = &mut self.replicas[to as usize];
                                replica.apply_high_qc_from_network(qc.clone())
                            };

                            if updated {
                                if (to as usize) < self.last_response_ms.len() {
                                    self.last_response_ms[to as usize] = self.simulated_time_ms;
                                }
                                if self.verbose {
                                    println!("  ✓ Replica {} updated high_qc to block {}", to, block_hash);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        
        // Step 6: Check for commits
        if self.verbose {
            println!("\n[Step 6] Checking for commits...");
        }
        
        for (idx, replica) in self.replicas.iter_mut().enumerate() {
            if crashed.contains(&idx) || unresponsive.contains(&idx) {
                continue;
            }

            let before = replica.committed_log.len();
            replica.commit_all();
            let after = replica.committed_log.len();
            
            if after > before {
                if self.verbose {
                    println!("  ✓ Replica {} committed {} new block(s)", idx, after - before);
                }
            }

            if self.persistence_enabled && replica.should_snapshot() {
                if let Some(dir) = self.replica_data_dirs.get(idx) {
                    let _ = replica.take_snapshot(dir);
                }
            }

            if idx < self.last_response_ms.len() {
                self.last_response_ms[idx] = self.simulated_time_ms;
            }
        }
        
        // Step 7: Move to next view
        if self.verbose {
            println!("\n[Step 7] Moving to next view...");
        }
        
        for (idx, replica) in self.replicas.iter_mut().enumerate() {
            if crashed.contains(&idx) || unresponsive.contains(&idx) {
                continue;
            }
            replica.advance_view_with_reason(ViewChangeReason::Commit);
            if idx < self.last_response_ms.len() {
                self.last_response_ms[idx] = self.simulated_time_ms;
            }
        }
        
        self.step += 1;
        
        if self.verbose {
            println!("\n[OK] Round {} completed successfully", self.step);
            println!("   Votes collected: {}", votes_collected);
            println!("   QC formed: {}", if qc_formed.is_some() { "Yes" } else { "No" });
        }

        self.process_timeout_crashes();
        
        true
    }

    /// Crash a replica (drops volatile state and excludes it from rounds).
    pub fn crash_replica(&mut self, replica_id: usize) -> Result<(), String> {
        if !self.persistence_enabled {
            return Err("persistence is disabled for this simulation".to_string());
        }
        if replica_id >= self.replicas.len() {
            return Err(format!("invalid replica id {}", replica_id));
        }
        if self.is_crashed(replica_id) {
            return Err(format!("replica {} is already crashed", replica_id));
        }

        let config = self.replicas[replica_id].config.clone();
        let keystore = self.replica_keystores[replica_id].clone();
        self.replicas[replica_id] = Self::fresh_replica(config, keystore);
        self.crashed_replicas.insert(replica_id);
        self.unresponsive_replicas.remove(&replica_id);

        Ok(())
    }

    /// Recover a crashed replica from snapshot + WAL and rejoin it.
    pub fn recover_replica(&mut self, replica_id: usize) -> Result<(), String> {
        if !self.persistence_enabled {
            return Err("persistence is disabled for this simulation".to_string());
        }
        if replica_id >= self.replicas.len() {
            return Err(format!("invalid replica id {}", replica_id));
        }
        if !self.is_crashed(replica_id) {
            return Err(format!("replica {} is not crashed", replica_id));
        }

        let config = self.replicas[replica_id].config.clone();
        let keystore = self.replica_keystores[replica_id].clone();
        let data_dir = self
            .replica_data_dirs
            .get(replica_id)
            .ok_or_else(|| format!("missing data directory for replica {}", replica_id))?
            .clone();

        let (mut recovered, mut wal) = recover(config, keystore, &data_dir)
            .map_err(|e| format!("failed to recover replica {}: {}", replica_id, e))?;

        if wal.entry_count() == 0 && recovered.block_tree.len() == 1 && recovered.block_tree.contains_key(&0) {
            let genesis = recovered
                .block_tree
                .get(&0)
                .cloned()
                .unwrap_or_else(Self::genesis_block);
            wal.append(&LogEntry::from_block(&genesis))
                .map_err(|e| format!("failed to re-log genesis for replica {}: {}", replica_id, e))?;
        }

        recovered.attach_wal(wal);

        // ── State sync: catch up from a live replica ──────────────────
        //
        // After crash-recovery the replica only has blocks/commits that
        // were in its WAL + snapshot.  Blocks proposed while it was down
        // are missing.  In a real system a "catch-up" protocol would
        // fetch them from peers.  Here we simulate that by copying
        // missing blocks, the committed log prefix, and the high_qc
        // from a live replica.
        if let Some(donor_idx) = self
            .replicas
            .iter()
            .enumerate()
            .find(|(idx, _)| *idx != replica_id && !self.is_crashed(*idx))
            .map(|(idx, _)| idx)
        {
            // 1. Import missing blocks into the recovered block tree.
            for (hash, block) in &self.replicas[donor_idx].block_tree {
                if !recovered.block_tree.contains_key(hash) {
                    recovered.block_tree.insert(*hash, block.clone());
                }
            }

            // 2. Adopt the donor's committed log if it is longer AND is
            //    a valid extension (common prefix matches).
            let donor_log = &self.replicas[donor_idx].committed_log;
            if donor_log.len() > recovered.committed_log.len() {
                let prefix_ok = recovered
                    .committed_log
                    .iter()
                    .zip(donor_log.iter())
                    .all(|(a, b)| a.hash == b.hash);

                if prefix_ok {
                    recovered.committed_log = donor_log.clone();
                    recovered.committed_up_to =
                        self.replicas[donor_idx].committed_up_to;
                }
            }

            // 3. Adopt the donor's high_qc if it is newer.
            if let Some(donor_qc) = &self.replicas[donor_idx].high_qc {
                let dominated = match &recovered.high_qc {
                    Some(hq) => (donor_qc.epoch, donor_qc.view) > (hq.epoch, hq.view),
                    None => true,
                };
                if dominated {
                    recovered.high_qc = Some(donor_qc.clone());
                }
            }

            // 4. Ensure next_hash doesn't collide with any known block.
            if let Some(&max_hash) = recovered.block_tree.keys().max() {
                if max_hash >= recovered.next_hash {
                    recovered.next_hash = max_hash + 1;
                }
            }
        }

        // Rejoin with a view at least as high as live replicas.
        let target_view = self
            .replicas
            .iter()
            .enumerate()
            .filter(|(idx, _)| *idx != replica_id && !self.is_crashed(*idx))
            .map(|(_, r)| r.current_view)
            .max()
            .unwrap_or(recovered.current_view);

        while recovered.current_view < target_view {
            recovered.advance_view_with_reason(ViewChangeReason::Timeout);
        }

        self.replicas[replica_id] = recovered;
        self.crashed_replicas.remove(&replica_id);
        self.unresponsive_replicas.remove(&replica_id);
        self.mark_responded(replica_id);

        // Force a snapshot so the state-synced blocks / committed log /
        // high_qc are persisted.  Without this, the next crash-recovery
        // would replay a stale WAL whose commit_index values skip over
        // the catch-up commits we just imported.
        if let Some(dir) = self.replica_data_dirs.get(replica_id).cloned() {
            let _ = self.replicas[replica_id].take_snapshot(&dir);
        }

        Ok(())
    }

    /// Run a built-in crash/recover/rejoin scenario for one replica.
    pub fn run_crash_recover_rejoin_scenario(
        &mut self,
        replica_id: usize,
        rounds_before_crash: usize,
        rounds_during_crash: usize,
        rounds_after_recovery: usize,
    ) -> Result<(), String> {
        // Auto-recover a victim still crashed from a previous cycle.
        if self.is_crashed(replica_id) {
            self.recover_replica(replica_id)?;
        }

        for _ in 0..rounds_before_crash {
            if !self.run_one_round() {
                break;
            }
        }

        self.set_replica_unresponsive(replica_id, true)?;

        for _ in 0..rounds_during_crash {
            if !self.run_one_round() {
                break;
            }
            if self.is_crashed(replica_id) {
                break;
            }
        }

        if !self.is_crashed(replica_id) {
            return Err(format!(
                "replica {} did not timeout-crash within {} rounds",
                replica_id, rounds_during_crash
            ));
        }

        self.recover_replica(replica_id)?;

        for _ in 0..rounds_after_recovery {
            if !self.run_one_round() {
                break;
            }
        }

        Ok(())
    }
    
    /// Run the simulation for multiple rounds
    pub fn run(&mut self, rounds: usize) {
        println!("\nStarting multi-replica simulation with {} replicas", self.replicas.len());
        println!("   Max rounds: {}", rounds);
        println!("   Byzantine tolerance: f = {}", self.replicas[0].config.f);
        println!();
        
        for round in 0..rounds {
            if !self.run_one_round() {
                println!("\n[WARN] Simulation stopped at round {}", round + 1);
                break;
            }
        }
        
        let separator = "=".repeat(60);
        println!("\n{}", separator);
        println!("SIMULATION COMPLETED");
        println!("{}", separator);
    }
    
    /// Print final state of all replicas
    pub fn print_final_state(&self) {
        let separator = "=".repeat(60);
        println!("\n{}", separator);
        println!("FINAL STATE OF ALL REPLICAS");
        println!("{}", separator);
        
        for (idx, replica) in self.replicas.iter().enumerate() {
            println!("\n--- Replica {} ---", idx);
            if self.is_crashed(idx) {
                println!("Status: CRASHED");
                continue;
            }
            println!("Current View: {}", replica.current_view);
            println!("Blocks in tree: {}", replica.block_tree.len());
            println!("Committed blocks: {}", replica.committed_log.len());
            
            if let Some(qc) = &replica.high_qc {
                println!("High QC: Block {} (view {})", qc.block_hash, qc.view);
            }
            
            println!("Committed sequence:");
            for (i, block) in replica.committed_log.iter().enumerate() {
                println!("  {}. Block {} (view {}, proposer {})", 
                         i, block.hash, block.view, block.proposer);
            }
        }
    }
    
    /// Verify consensus safety: all replicas committed the same sequence
    pub fn verify_safety(&self) -> bool {
        if self.replicas.is_empty() {
            return true;
        }
        
        let separator = "=".repeat(60);
        println!("\n{}", separator);
        println!("SAFETY VERIFICATION");
        println!("{}", separator);
        
        let reference_idx = self
            .replicas
            .iter()
            .enumerate()
            .find(|(idx, _)| !self.is_crashed(*idx))
            .map(|(idx, _)| idx);

        let Some(reference_idx) = reference_idx else {
            println!("[WARN] Safety check skipped: all replicas are crashed");
            return true;
        };

        let reference = &self.replicas[reference_idx].committed_log;
        
        for (idx, replica) in self.replicas.iter().enumerate().skip(1) {
            if idx == reference_idx || self.is_crashed(idx) {
                continue;
            }

            // Check if committed sequences match
            let min_len = reference.len().min(replica.committed_log.len());
            
            for i in 0..min_len {
                if reference[i].hash != replica.committed_log[i].hash {
                    println!("[FAIL] SAFETY VIOLATION!");
                    println!("   Replica {} committed block {} at position {}", reference_idx, reference[i].hash, i);
                    println!("   Replica {} committed block {} at position {}", idx, replica.committed_log[i].hash, i);
                    return false;
                }
            }
        }
        
        println!("[OK] Safety verified: All replicas have consistent committed sequences");
        println!("   Committed blocks: {}", reference.len());
        
        true
    }
    
    /// Compare block trees across replicas
    pub fn compare_block_trees(&self) {
        let separator = "=".repeat(60);
        println!("\n{}", separator);
        println!("BLOCK TREE COMPARISON");
        println!("{}", separator);
        
        for (idx, replica) in self.replicas.iter().enumerate() {
            if self.is_crashed(idx) {
                println!("\nReplica {}: CRASHED", idx);
                continue;
            }
            println!("\nReplica {}: {} blocks", idx, replica.block_tree.len());
        }
        
        // Find all unique block hashes
        let mut all_blocks = std::collections::HashSet::new();
        for replica in &self.replicas {
            
            for hash in replica.block_tree.keys() {
                all_blocks.insert(*hash);
            }
        }
        
        println!("\nTotal unique blocks across all replicas: {}", all_blocks.len());
        
        // Check which blocks each replica has
        println!("\nBlock presence matrix:");
        println!("Block | {}", (0..self.replicas.len())
            .map(|i| format!("R{}", i))
            .collect::<Vec<_>>()
            .join(" "));
        println!("{}", "-".repeat(40));
        
        for hash in all_blocks.iter() {
            print!("{:5} | ", hash);
            for (idx, replica) in self.replicas.iter().enumerate() {
                if self.is_crashed(idx) {
                    print!("{:2} ", "-");
                } else {
                    print!("{:2} ", if replica.block_tree.contains_key(hash) { "✓" } else { "✗" });
                }
            }
            println!();
        }
    }

    // ===== Enhanced Pacemaker: Dummy Proposals =====

    /// Enable dummy-proposal injection on all replicas.
    ///
    /// When enabled, leaders will propose NoOp blocks during idle rounds
    /// to keep the 3-chain growing so that pending client commands can
    /// commit without further client activity.
    ///
    /// # Arguments
    /// * `timeout_ms` – idle threshold before a dummy is injected
    pub fn enable_dummy_proposals(&mut self, timeout_ms: u64) {
        for replica in &mut self.replicas {
            replica.enable_dummy_proposals(timeout_ms);
        }
    }

    /// Disable dummy-proposal injection on all replicas.
    pub fn disable_dummy_proposals(&mut self) {
        for replica in &mut self.replicas {
            replica.disable_dummy_proposals();
        }
    }

    /// Enqueue a client command on the current leader.
    ///
    /// The command will be proposed in the next round if this replica
    /// is still the leader.
    pub fn enqueue_client_command(&mut self, cmd: crate::types::ConsensusCommand) {
        let leader = self.current_leader();
        self.replicas[leader].enqueue_command(cmd);
    }

    /// Run one round using the enhanced pacemaker.
    ///
    /// Unlike [`run_one_round`], the leader uses
    /// [`propose_next_with_hash`] which prefers queued client commands
    /// and falls back to dummy NoOp blocks when the pacemaker conditions
    /// are met.  If neither condition holds, the round is skipped
    /// (no proposal).
    pub fn run_one_round_with_pacemaker(&mut self) -> bool {
        self.advance_simulated_time();
        self.process_timeout_crashes();

        if self.step >= self.max_steps || self.crashed_replicas.len() == self.replicas.len() {
            return false;
        }

        let crashed = self.crashed_replicas.clone();
        let unresponsive = self.unresponsive_replicas.clone();

        let leader_id = self.current_leader();
        let current_view = self.replicas[leader_id].current_view;

        if self.verbose {
            let separator = "=".repeat(60);
            println!("\n{}", separator);
            println!("ROUND {} | View {} | Leader: Replica {} [pacemaker]",
                     self.step + 1, current_view, leader_id);
            println!("{}", separator);
        }

        // If the leader is unavailable, do a view timeout.
        if self.is_crashed(leader_id) || unresponsive.contains(&leader_id) {
            if self.verbose {
                println!("  [FAIL] Leader {} is unavailable", leader_id);
            }
            for (idx, replica) in self.replicas.iter_mut().enumerate() {
                if crashed.contains(&idx) || unresponsive.contains(&idx) {
                    continue;
                }
                replica.on_view_timeout();
            }
            self.step += 1;
            return true;
        }

        // The leader proposes using the enhanced pacemaker logic.
        let unique_hash = self.network.generate_unique_hash();
        let proposal = self.replicas[leader_id]
            .propose_next_with_hash(current_view, unique_hash);

        if proposal.is_none() {
            // Neither a client command nor a dummy proposal is warranted.
            if self.verbose {
                println!("  [SKIP] No proposal warranted (queue empty, dummy not needed)");
            }
            // Still advance the view so the protocol makes progress.
            for (idx, replica) in self.replicas.iter_mut().enumerate() {
                if crashed.contains(&idx) || unresponsive.contains(&idx) {
                    continue;
                }
                replica.advance_view_with_reason(ViewChangeReason::Commit);
            }
            self.step += 1;
            return true;
        }

        let block = proposal.unwrap();
        let is_dummy = block.command == crate::types::ConsensusCommand::NoOp;
        self.mark_responded(leader_id);

        if self.verbose {
            let tag = if is_dummy { "DUMMY" } else { "CLIENT" };
            println!("  ✓ [{}] Proposed Block {} (view {})",
                     tag, block.hash, block.view);
        }

        // The rest of the round is identical to run_one_round:
        // broadcast → vote → QC → commit → advance view.

        self.network.broadcast_proposal(leader_id as u64, block.clone());

        // Collect proposals.
        let mut proposal_messages = Vec::new();
        while self.network.has_messages() {
            if let Some(msg) = self.network.receive() {
                if matches!(msg, Message::Proposal { .. }) {
                    proposal_messages.push(msg);
                }
            }
        }

        // Validate & vote.
        for msg in proposal_messages {
            if let Message::Proposal { from, to, block } = msg {
                if crashed.contains(&(to as usize)) || unresponsive.contains(&(to as usize)) {
                    continue;
                }

                let mut vote_to_send: Option<crate::types::Vote> = None;
                {
                    let replica = &mut self.replicas[to as usize];
                    if replica.validate_and_insert_proposal(block.clone()) {
                        let signature = replica.keystore.sign(block.hash, block.view, block.epoch);
                        vote_to_send = Some(crate::types::Vote {
                            block_hash: block.hash,
                            view: block.view,
                            epoch: block.epoch,
                            signature,
                        });
                    }
                }

                if let Some(vote) = vote_to_send {
                    if (to as usize) < self.last_response_ms.len() {
                        self.last_response_ms[to as usize] = self.simulated_time_ms;
                    }
                    self.network.send_vote(to, from, vote);
                }
            }
        }

        // Collect votes and form QC.
        let mut qc_formed = None;
        while self.network.has_messages() {
            if let Some(msg) = self.network.receive() {
                if let Message::Vote { from: _, to, vote } = msg {
                    if crashed.contains(&(to as usize)) || unresponsive.contains(&(to as usize)) {
                        continue;
                    }
                    let formed_qc = self.replicas[to as usize].handle_vote(vote);
                    if (to as usize) < self.last_response_ms.len() {
                        self.last_response_ms[to as usize] = self.simulated_time_ms;
                    }
                    if let Some(qc) = formed_qc {
                        qc_formed = Some(qc);
                    }
                }
            }
        }

        // Broadcast QC.
        if let Some(qc) = qc_formed {
            self.network.broadcast_qc(leader_id as u64, qc.clone(), qc.block_hash);

            while self.network.has_messages() {
                if let Some(msg) = self.network.receive() {
                    if let Message::QuorumCertBroadcast { to, qc, .. } = msg {
                        if crashed.contains(&(to as usize)) || unresponsive.contains(&(to as usize)) {
                            continue;
                        }
                        self.replicas[to as usize].apply_high_qc_from_network(qc);
                        if (to as usize) < self.last_response_ms.len() {
                            self.last_response_ms[to as usize] = self.simulated_time_ms;
                        }
                    }
                }
            }
        }

        // Commit.
        for (idx, replica) in self.replicas.iter_mut().enumerate() {
            if crashed.contains(&idx) || unresponsive.contains(&idx) {
                continue;
            }
            replica.commit_all();

            if self.persistence_enabled && replica.should_snapshot() {
                if let Some(dir) = self.replica_data_dirs.get(idx) {
                    let _ = replica.take_snapshot(dir);
                }
            }
            if idx < self.last_response_ms.len() {
                self.last_response_ms[idx] = self.simulated_time_ms;
            }
        }

        // Advance view.
        for (idx, replica) in self.replicas.iter_mut().enumerate() {
            if crashed.contains(&idx) || unresponsive.contains(&idx) {
                continue;
            }
            replica.advance_view_with_reason(ViewChangeReason::Commit);
            if idx < self.last_response_ms.len() {
                self.last_response_ms[idx] = self.simulated_time_ms;
            }
        }

        self.step += 1;
        true
    }
}
