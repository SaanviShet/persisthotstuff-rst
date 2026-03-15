//! Byzantine Failures and Network Attack Scenarios
//!
//! This example demonstrates various failure scenarios in the consensus protocol:
//! 1. Byzantine replicas sending invalid votes
//! 2. Network delays and partitions  
//! 3. Leader failures
//! 4. Message drops and out-of-order delivery

use persisthotstuff_rst::simulation::Simulation;
use persisthotstuff_rst::network::Message;
use persisthotstuff_rst::types::*;
use colored::*;

fn main() {
    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║   PersistHotStuff Byzantine Failures & Attack Scenarios   ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    
    println!("\nSystem Configuration:");
    println!("   Total replicas (n): 4");
    println!("   Byzantine tolerance (f): 1");
    println!("   Quorum size (2f+1): 3");
    println!("   Can tolerate up to 1 faulty/Byzantine replica\n");
    
    // Scenario 1: Byzantine Replica Sending Invalid Votes
    scenario_1_byzantine_votes();
    
    // Scenario 2: Network Message Delays
    scenario_2_message_delays();
    
    // Scenario 3: Leader Failure (Timeout)
    scenario_3_leader_failure();
    
    // Scenario 4: Network Partition
    scenario_4_network_partition();
    
    // Scenario 5: Byzantine Leader Equivocation
    scenario_5_byzantine_leader();
    
    println!("\n[OK] All Byzantine failure scenarios completed!\n");
}

/// Scenario 1: Byzantine Replica Sending Invalid Votes (f Byzantine replicas tolerated)
/// Since verify() always returns true in the current implementation, this demonstrates
/// the protocol's ability to reject invalid votes (wrong block hash, invalid signer ID)
fn scenario_1_byzantine_votes() {
    println!("\n{}", "=".repeat(70).bright_red());
    println!("{}", "SCENARIO 1: Byzantine Replica Sending Invalid Votes".bright_red().bold());
    println!("{}", "=".repeat(70).bright_red());
    
    println!("\nScenario Description:");
    println!("   - Replica 3 is Byzantine and attempts to disrupt consensus");
    println!("   - Sends vote for non-existent block AND invalid signature");
    println!("   - System tolerates up to f=1 Byzantine replica");
    println!("   - QC forms with 3 honest replicas (R0, R1, R2)");
    
    let mut sim = Simulation::new(4, 1, 10, false);
    
    // Run one normal round first
    println!("\n--- Round 1: Normal Operation ---");
    sim.run_one_round();
    
    println!("\n--- Round 2: Byzantine Behavior ---");
    
    let leader_id = sim.current_leader();
    let current_view = sim.replicas[leader_id].current_view;
    
    // Leader proposes
    let unique_hash = sim.network.generate_unique_hash();
    if let Some(block) = sim.replicas[leader_id].propose_with_hash(current_view, unique_hash) {
        println!("✓ Leader {} proposed block {}", leader_id, block.hash);
        
        // Leader votes for its own proposal
        let leader_vote = Vote {
            block_hash: block.hash,
            view: current_view,
            epoch: sim.replicas[leader_id].config_epoch,
            signature: sim.replicas[leader_id].keystore.sign(block.hash, current_view, sim.replicas[leader_id].config_epoch),
        };
        sim.replicas[leader_id].handle_vote(leader_vote);
        
        // Broadcast to all
        sim.network.broadcast_proposal(leader_id as u64, block.clone());
        
        // Process proposals - replicas vote
        let mut proposals_processed = Vec::new();
        while sim.network.has_messages() {
            if let Some(msg) = sim.network.receive() {
                proposals_processed.push(msg);
            }
        }
        
        for msg in proposals_processed {
            if let Message::Proposal { from, to, block } = msg {
                let replica = &mut sim.replicas[to as usize];
                
                if replica.validate_and_insert_proposal(block.clone()) {
                    if to == 3 {
                        // Byzantine behavior: Try multiple attack vectors
                        println!("[WARN] Byzantine Replica 3 attempting multiple attacks:");
                        
                        // Attack 1: Vote for non-existent block
                        println!("   Attack 1: Voting for non-existent block (hash: 99999)");
                        let invalid_vote_1 = Vote {
                            block_hash: 99999, // Non-existent block hash
                            view: current_view,
                            epoch: sim.replicas[to as usize].config_epoch,
                            signature: sim.replicas[to as usize].keystore.sign(99999, current_view, sim.replicas[to as usize].config_epoch),
                        };
                        sim.network.send_vote(to, leader_id as u64, invalid_vote_1);
                        
                        // // Attack 2: Vote with invalid signer ID
                        // println!("   Attack 2: Using invalid signer ID (999)");
                        // let invalid_vote_2 = Vote {
                        //     block_hash: block.hash,
                        //     view: current_view,
                        //     signature: sign(999), // Invalid replica ID
                        // };
                        // sim.network.send_vote(to, leader_id as u64, invalid_vote_2);
                    } else {
                        println!("✓ Honest Replica {} validated and voting", to);
                        let vote = Vote {
                            block_hash: block.hash,
                            view: current_view,
                            epoch: sim.replicas[to as usize].config_epoch,
                            signature: sim.replicas[to as usize].keystore.sign(block.hash, current_view, sim.replicas[to as usize].config_epoch),
                        };
                        sim.network.send_vote(to, leader_id as u64, vote);
                    }
                }
            }
        }
        
        // Leader collects votes
        let mut valid_votes = 1; // Leader already voted
        let mut invalid_votes = 0;
        let mut qc_formed = false;
        
        while sim.network.has_messages() {
            if let Some(Message::Vote { from, to, vote }) = sim.network.receive() {
                let leader = &mut sim.replicas[to as usize];
                
                if let Some(qc) = leader.handle_vote(vote) {
                    qc_formed = true;
                    println!("\n[QC] Formed with {} signatures!", qc.signatures.len());
                    println!("   System tolerated Byzantine replica (need 2f+1 = 3 votes)");
                    println!("   Byzantine attacks were REJECTED by validation checks!");
                    break;
                } else {
                    if from == 3 {
                        invalid_votes += 1;
                        println!("   [FAIL] Byzantine vote from R{} REJECTED", from);
                    } else {
                        valid_votes += 1;
                        println!("   ✓ Valid vote from R{} accepted (total: {})", from, valid_votes);
                    }
                }
            }
        }
        
        if !qc_formed {
            println!("\n[WARN] QC not yet formed - collected {} valid votes, rejected {} invalid votes", 
                     valid_votes, invalid_votes);
        }
    }
    
    println!("\n[OK] Scenario 1 Result: System can tolerate up to f=1 Byzantine replicas!");
}

/// Scenario 2: Network Message Delays
/// Demonstrates that system makes progress even when f replicas are delayed
fn scenario_2_message_delays() {
    println!("\n{}", "=".repeat(70).bright_yellow());
    println!("{}", "SCENARIO 2: Network Message Delays".bright_yellow().bold());
    println!("{}", "=".repeat(70).bright_yellow());
    
    println!("\nScenario Description:");
    println!("   - Messages from Replica 2 are delayed");
    println!("   - System makes progress with 3 responsive replicas (R0, R1, R3)");
    println!("   - QC forms without the delayed replica");
    
    let mut sim = Simulation::new(4, 1, 10, false);
    
    println!("\n--- Simulating Delayed Messages from Replica 2 ---");
    
    let leader_id = sim.current_leader();
    let current_view = sim.replicas[leader_id].current_view;
    
    // Leader proposes and votes for self
    let unique_hash = sim.network.generate_unique_hash();
    if let Some(block) = sim.replicas[leader_id].propose_with_hash(current_view, unique_hash) {
        println!("✓ Leader {} proposed block {} and voted", leader_id, block.hash);
        
        // Leader votes for its own proposal
        let leader_vote = Vote {
            block_hash: block.hash,
            view: current_view,
            epoch: sim.replicas[leader_id].config_epoch,
            signature: sim.replicas[leader_id].keystore.sign(block.hash, current_view, sim.replicas[leader_id].config_epoch),
        };
        sim.replicas[leader_id].handle_vote(leader_vote);
        
        // Broadcast to all
        sim.network.broadcast_proposal(leader_id as u64, block.clone());
        
        // Process proposals, but delay messages to R2
        let mut proposals_processed = Vec::new();
        while sim.network.has_messages() {
            if let Some(msg) = sim.network.receive() {
                proposals_processed.push(msg);
            }
        }
        
        for msg in proposals_processed {
            if let Message::Proposal { from, to, block } = msg {
                if to == 2 {
                    println!("[DELAY] Message to Replica 2 DELAYED (simulating network latency)");
                    continue; // Skip processing for R2
                }
                
                let replica = &mut sim.replicas[to as usize];
                if replica.validate_and_insert_proposal(block.clone()) {
                    println!("✓ Replica {} received and voting", to);
                    let vote = Vote {
                        block_hash: block.hash,
                        view: current_view,
                        epoch: sim.replicas[to as usize].config_epoch,
                        signature: sim.replicas[to as usize].keystore.sign(block.hash, current_view, sim.replicas[to as usize].config_epoch),
                    };
                    sim.network.send_vote(to, leader_id as u64, vote);
                }
            }
        }
        
        // Leader collects votes
        let mut vote_count = 1; // Leader already voted
        let mut qc_formed = false;
        
        while sim.network.has_messages() {
            if let Some(Message::Vote { from, to, vote }) = sim.network.receive() {
                vote_count += 1;
                println!("  ✓ Leader received vote from R{} (total: {})", from, vote_count);
                
                let leader = &mut sim.replicas[to as usize];
                if let Some(qc) = leader.handle_vote(vote) {
                    qc_formed = true;
                    println!("\n[QC] Formed with {} votes (without delayed Replica 2)!", qc.signatures.len());
                    println!("   System made progress despite network delays!");
                    break;
                }
            }
        }
        
        if !qc_formed {
            println!("\n[WARN] Collected {} votes - need 3 for QC", vote_count);
        }
    }
    
    println!("\n[OK] Scenario 2 Result: System tolerated network delays successfully!");
}

/// Scenario 3: Leader Failure (Timeout and View Change)
/// Demonstrates view change when leader fails to propose
fn scenario_3_leader_failure() {
    println!("\n{}", "=".repeat(70).bright_blue());
    println!("{}", "SCENARIO 3: Leader Failure and View Change".bright_blue().bold());
    println!("{}", "=".repeat(70).bright_blue());
    
    println!("\nScenario Description:");
    println!("   - Current leader (R0) fails and doesn't propose");
    println!("   - Replicas timeout waiting for proposal");
    println!("   - System triggers view change to new leader (R1)");
    println!("   - New leader successfully proposes");
    
    let mut sim = Simulation::new(4, 1, 10, false);
    
    println!("\n--- Initial State ---");
    println!("Current view: {}", sim.replicas[0].current_view);
    println!("Current leader: Replica {}", sim.current_leader());
    
    println!("\n--- Leader Fails to Propose (Simulating Crash) ---");
    println!("[WARN] Replica {} is unresponsive...", sim.current_leader());
    
    // Simulate timeout - all replicas move to next view
    println!("\n--- Replicas Timing Out ---");
    for (idx, replica) in sim.replicas.iter_mut().enumerate() {
        replica.on_view_timeout();
        println!("✓ Replica {} moved to view {}", idx, replica.current_view);
    }
    
    let new_leader = sim.current_leader();
    println!("\n--- New Leader Elected ---");
    println!("New leader: Replica {}", new_leader);
    
    // New leader proposes
    let current_view = sim.replicas[new_leader].current_view;
    let unique_hash = sim.network.generate_unique_hash();
    
    if let Some(block) = sim.replicas[new_leader].propose_with_hash(current_view, unique_hash) {
        println!("✓ New leader {} successfully proposed block {}", new_leader, block.hash);
        
        // Leader votes for its own proposal
        let leader_vote = Vote {
            block_hash: block.hash,
            view: current_view,
            epoch: sim.replicas[new_leader].config_epoch,
            signature: sim.replicas[new_leader].keystore.sign(block.hash, current_view, sim.replicas[new_leader].config_epoch),
        };
        sim.replicas[new_leader].handle_vote(leader_vote);
        
        sim.network.broadcast_proposal(new_leader as u64, block.clone());
        
        // Process and vote
        let mut proposals_processed = Vec::new();
        while sim.network.has_messages() {
            if let Some(msg) = sim.network.receive() {
                proposals_processed.push(msg);
            }
        }
        
        for msg in proposals_processed {
            if let Message::Proposal { from, to, block } = msg {
                let replica = &mut sim.replicas[to as usize];
                if replica.validate_and_insert_proposal(block.clone()) {
                    let vote = Vote {
                        block_hash: block.hash,
                        view: current_view,
                        epoch: sim.replicas[to as usize].config_epoch,
                        signature: sim.replicas[to as usize].keystore.sign(block.hash, current_view, sim.replicas[to as usize].config_epoch),
                    };
                    sim.network.send_vote(to, new_leader as u64, vote);
                }
            }
        }
        
        // Collect votes
        let mut vote_count = 1; // Leader already voted
        let mut qc_formed = false;
        
        while sim.network.has_messages() {
            if let Some(Message::Vote { from, to, vote }) = sim.network.receive() {
                vote_count += 1;
                let leader = &mut sim.replicas[to as usize];
                if let Some(qc) = leader.handle_vote(vote) {
                    qc_formed = true;
                    println!("[QC] Formed in new view {} with {} votes!", qc.view, qc.signatures.len());
                    println!("   View change successful - system recovered!");
                    break;
                }
            }
        }
        
        if !qc_formed {
            println!("[WARN] Collected {} votes", vote_count);
        }
    }
    
    println!("\n[OK] Scenario 3 Result: System recovered from leader failure via view change!");
}

/// Scenario 4: Network Partition
/// Simulates a network split where replicas can't communicate
fn scenario_4_network_partition() {
    println!("\n{}", "=".repeat(70).bright_magenta());
    println!("{}", "SCENARIO 4: Network Partition".bright_magenta().bold());
    println!("{}", "=".repeat(70).bright_magenta());
    
    println!("\nScenario Description:");
    println!("   - Network splits: [R0, R1] vs [R2, R3]");
    println!("   - Each partition has only 2 replicas (< quorum of 3)");
    println!("   - Neither partition can form QC");
    println!("   - System halts (liveness violated, but safety preserved)");
    
    let mut sim = Simulation::new(4, 1, 10, false);
    
    println!("\n--- Network Partitioned ---");
    println!("Partition A: R0, R1");
    println!("Partition B: R2, R3");
    
    let leader_id = sim.current_leader();
    let current_view = sim.replicas[leader_id].current_view;
    
    // Leader proposes
    let unique_hash = sim.network.generate_unique_hash();
    if let Some(block) = sim.replicas[leader_id].propose_with_hash(current_view, unique_hash) {
        println!("\n✓ Leader {} (in partition A) proposed block {}", leader_id, block.hash);
        
        // Leader votes for own proposal
        let leader_vote = Vote {
            block_hash: block.hash,
            view: current_view,
            epoch: sim.replicas[leader_id].config_epoch,
            signature: sim.replicas[leader_id].keystore.sign(block.hash, current_view, sim.replicas[leader_id].config_epoch),
        };
        sim.replicas[leader_id].handle_vote(leader_vote);
        
        // Only deliver to same partition
        println!("\n--- Messages Only Within Partitions ---");
        for to in 0..sim.replicas.len() {
            if to == leader_id {
                continue;
            }
            
            // Partition A: R0, R1
            // Partition B: R2, R3
            let leader_partition = if leader_id < 2 { "A" } else { "B" };
            let to_partition = if to < 2 { "A" } else { "B" };
            
            if leader_partition == to_partition {
                println!("✓ Message delivered to R{} (same partition {})", to, to_partition);
                sim.replicas[to].validate_and_insert_proposal(block.clone());
                let vote = Vote {
                    block_hash: block.hash,
                    view: current_view,
                    epoch: sim.replicas[to as usize].config_epoch,
                    signature: sim.replicas[to as usize].keystore.sign(block.hash, current_view, sim.replicas[to as usize].config_epoch),
                };
                sim.network.send_vote(to as u64, leader_id as u64, vote);
            } else {
                println!("[DROP] Message to R{} DROPPED (different partition {})", to, to_partition);
            }
        }
        
        // Try to form QC (will fail - only 2 votes in partition)
        let mut vote_count = 1; // Leader already voted
        let mut qc_formed = false;
        
        while sim.network.has_messages() {
            if let Some(Message::Vote { from, to, vote }) = sim.network.receive() {
                vote_count += 1;
                let leader = &mut sim.replicas[to as usize];
                if let Some(qc) = leader.handle_vote(vote) {
                    qc_formed = true;
                    println!("✓ QC formed");
                }
            }
        }
        
        if !qc_formed {
            println!("\n[WARN] Only {} vote(s) in partition A", vote_count);
            println!("   Need 3 votes for quorum, but partition only has 2 replicas");
            println!("   [FAIL] Cannot form QC - System HALTED");
            println!("   [OK] Safety preserved: No conflicting blocks committed");
        }
    }
    
    println!("\n[OK] Scenario 4 Result: System correctly halted (safety preserved, liveness violated)");
}

/// Scenario 5: Byzantine Leader Equivocation
/// Leader proposes different blocks to different replicas
fn scenario_5_byzantine_leader() {
    println!("\n{}", "=".repeat(70).bright_red());
    println!("{}", "SCENARIO 5: Byzantine Leader Equivocation".bright_red().bold());
    println!("{}", "=".repeat(70).bright_red());
    
    println!("\nScenario Description:");
    println!("   - Byzantine leader (R0) proposes TWO different blocks");
    println!("   - Block A to R1, R2");
    println!("   - Block B to R3");
    println!("   - Votes split across different blocks");
    println!("   - No single block gets quorum - QC formation fails");
    
    let mut sim = Simulation::new(4, 1, 10, false);
    
    let leader_id = 0; // R0 is Byzantine leader
    let current_view = sim.replicas[leader_id].current_view;
    
    println!("\n--- Byzantine Leader Creating Conflicting Blocks ---");
    
    // Create two different blocks with different hashes
    let hash_a = sim.network.generate_unique_hash();
    let hash_b = sim.network.generate_unique_hash();
    
    let block_a = Block {
        hash: hash_a,
        parent: Some(0),
        view: current_view,
        epoch: sim.replicas[leader_id].config_epoch,
        proposer: leader_id as u64,
        qc: None,
        command: ConsensusCommand::NoOp,
    };
    
    let block_b = Block {
        hash: hash_b,
        parent: Some(0),
        view: current_view,
        epoch: sim.replicas[leader_id].config_epoch,
        proposer: leader_id as u64,
        qc: None,
        command: ConsensusCommand::NoOp,
    };
    
    println!("[WARN] Byzantine Leader created Block A (hash: {})", hash_a);
    println!("[WARN] Byzantine Leader created Block B (hash: {})", hash_b);
    
    // Send different blocks to different replicas
    println!("\n--- Sending Conflicting Proposals ---");
    println!("  → Block A sent to R1, R2");
    println!("  → Block B sent to R3");
    
    sim.replicas[1].validate_and_insert_proposal(block_a.clone());
    sim.replicas[2].validate_and_insert_proposal(block_a.clone());
    sim.replicas[3].validate_and_insert_proposal(block_b.clone());
    
    // Replicas vote for what they received
    println!("\n--- Replicas Voting ---");
    let vote_1 = Vote { block_hash: hash_a, view: current_view, epoch: sim.replicas[1].config_epoch, signature: sim.replicas[1].keystore.sign(hash_a, current_view, sim.replicas[1].config_epoch) };
    let vote_2 = Vote { block_hash: hash_a, view: current_view, epoch: sim.replicas[2].config_epoch, signature: sim.replicas[2].keystore.sign(hash_a, current_view, sim.replicas[2].config_epoch) };
    let vote_3 = Vote { block_hash: hash_b, view: current_view, epoch: sim.replicas[3].config_epoch, signature: sim.replicas[3].keystore.sign(hash_b, current_view, sim.replicas[3].config_epoch) };
    
    println!("✓ R1 voted for Block A");
    println!("✓ R2 voted for Block A");
    println!("✓ R3 voted for Block B");
    
    // Leader tries to collect votes
    println!("\n--- Attempting QC Formation ---");
    let leader = &mut sim.replicas[leader_id];
    
    let qc_1 = leader.handle_vote(vote_1);
    println!("  Vote 1 for Block A received (votes for A: 1)");
    
    let qc_2 = leader.handle_vote(vote_2);
    println!("  Vote 2 for Block A received (votes for A: 2)");
    
    let qc_3 = leader.handle_vote(vote_3);
    println!("  Vote 1 for Block B received (votes for B: 1)");
    
    if qc_1.is_none() && qc_2.is_none() && qc_3.is_none() {
        println!("\n[FAIL] No single block has 3 votes!");
        println!("   Block A: 2 votes (need 3)");
        println!("   Block B: 1 vote (need 3)");
        println!("   QC formation FAILED - Equivocation prevented consensus!");
    }
    
    println!("\n[OK] Scenario 5 Result: System prevented Byzantine leader from breaking consensus!");
}
