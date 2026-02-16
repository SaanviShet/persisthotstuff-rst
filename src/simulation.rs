//! Simulation module for running multi-replica consensus scenarios.
//!
//! This module provides the simulation environment for executing complete consensus
//! rounds with multiple replicas, including proposal, voting, QC formation, and commit phases.

use crate::config::Config;
use crate::replica::Replica;
use crate::network::{Network, Message};
use crate::types::*;
use crate::crypto::KeyStore;
use std::collections::BTreeMap;

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
}

impl Simulation {
    /// Create a new simulation with n replicas
    pub fn new(n: usize, f: usize, max_steps: usize, verbose: bool) -> Self {
        let mut replicas = Vec::new();
        
        // Generate Ed25519 key pairs for all replicas
        let all_keys = KeyStore::generate_keys(n);
        
        // Distribute keys: each replica gets its own private key + all public keys
        let keystores = KeyStore::distribute_keys(&all_keys);
        
        // Initialize all replicas with the same genesis block
        for id in 0..n {
            let config = Config {
                n,
                f,
                id: id as u64,
                timeout_ms: 5000,
            };
            
            let mut replica = Replica {
                config,
                current_view: 0,
                block_tree: BTreeMap::new(),
                high_qc: None,
                vote_pool: BTreeMap::new(),
                next_hash: 1, // Start from 1 (0 is genesis)
                committed_log: Vec::new(),
                committed_up_to: None,
                timeout_ms: 5000,
                view_start_time: Replica::current_time_ms(),
                keystore: keystores[id].clone(),
            };
            
            // Insert genesis block
            replica.block_tree.insert(0, Block {
                hash: 0,
                parent: None,
                view: 0,
                proposer: 0,
                qc: None,
            });
            
            replicas.push(replica);
        }
        
        let network = Network::new(n);
        
        Simulation {
            replicas,
            network,
            step: 0,
            max_steps,
            verbose,
        }
    }
    
    /// Get the current leader based on current view
    pub fn current_leader(&self) -> usize {
        let view = self.replicas[0].current_view;
        (view as usize) % self.replicas.len()
    }
    
    /// Run one consensus round (propose -> vote -> QC formation -> commit)
    pub fn run_one_round(&mut self) -> bool {
        if self.step >= self.max_steps {
            return false;
        }
        
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
        
        // Generate unique hash from network to avoid collisions
        let unique_hash = self.network.generate_unique_hash();
        let proposal = self.replicas[leader_id].propose_with_hash(current_view, unique_hash);
        if proposal.is_none() {
            if self.verbose {
                println!("  ❌ Leader failed to propose");
            }
            return false;
        }
        
        let block = proposal.unwrap();
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
                let replica = &mut self.replicas[to as usize];
                
                // Validate and insert proposal
                let valid = replica.validate_and_insert_proposal(block.clone());
                
                if valid {
                    if self.verbose {
                        println!("  ✓ Replica {} validated block {}", to, block.hash);
                    }
                    
                    // Create and send vote with Ed25519 signature using replica's own keystore
                    let signature = replica.keystore.sign(block.hash, block.view);
                    let vote = Vote {
                        block_hash: block.hash,
                        view: block.view,
                        signature,
                    };
                    
                    self.network.send_vote(to, from, vote);
                    if self.verbose {
                        println!("    → Replica {} sent vote to Leader {}", to, from);
                    }
                } else {
                    if self.verbose {
                        println!("  ❌ Replica {} rejected block {}", to, block.hash);
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
                        votes_collected += 1;
                        if self.verbose {
                            println!("  ✓ Leader {} received vote from Replica {}", to, from);
                        }
                        
                        let leader = &mut self.replicas[to as usize];
                        if let Some(qc) = leader.handle_vote(vote) {
                            qc_formed = Some(qc.clone());
                            if self.verbose {
                                println!("  🎉 QC formed for block {} with {} signatures!", 
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
                            let replica = &mut self.replicas[to as usize];
                            if replica.high_qc.is_none() || 
                               replica.high_qc.as_ref().unwrap().view < qc.view {
                                replica.high_qc = Some(qc.clone());
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
            let before = replica.committed_log.len();
            replica.commit_all();
            let after = replica.committed_log.len();
            
            if after > before {
                if self.verbose {
                    println!("  ✓ Replica {} committed {} new block(s)", idx, after - before);
                }
            }
        }
        
        // Step 7: Move to next view
        if self.verbose {
            println!("\n[Step 7] Moving to next view...");
        }
        
        for replica in self.replicas.iter_mut() {
            replica.current_view += 1;
        }
        
        self.step += 1;
        
        if self.verbose {
            println!("\n✅ Round {} completed successfully", self.step);
            println!("   Votes collected: {}", votes_collected);
            println!("   QC formed: {}", if qc_formed.is_some() { "Yes" } else { "No" });
        }
        
        true
    }
    
    /// Run the simulation for multiple rounds
    pub fn run(&mut self, rounds: usize) {
        println!("\n🚀 Starting multi-replica simulation with {} replicas", self.replicas.len());
        println!("   Max rounds: {}", rounds);
        println!("   Byzantine tolerance: f = {}", self.replicas[0].config.f);
        println!();
        
        for round in 0..rounds {
            if !self.run_one_round() {
                println!("\n⚠️  Simulation stopped at round {}", round + 1);
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
        
        let reference = &self.replicas[0].committed_log;
        
        for (idx, replica) in self.replicas.iter().enumerate().skip(1) {
            // Check if committed sequences match
            let min_len = reference.len().min(replica.committed_log.len());
            
            for i in 0..min_len {
                if reference[i].hash != replica.committed_log[i].hash {
                    println!("❌ SAFETY VIOLATION!");
                    println!("   Replica 0 committed block {} at position {}", reference[i].hash, i);
                    println!("   Replica {} committed block {} at position {}", idx, replica.committed_log[i].hash, i);
                    return false;
                }
            }
        }
        
        println!("✅ Safety verified: All replicas have consistent committed sequences");
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
            for replica in &self.replicas {
                print!("{:2} ", if replica.block_tree.contains_key(hash) { "✓" } else { "✗" });
            }
            println!();
        }
    }
}
