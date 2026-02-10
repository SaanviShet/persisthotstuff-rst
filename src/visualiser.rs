//! Visualization module for displaying block trees, consensus state, and replica comparisons.
//!
//! This module provides enhanced visualization capabilities with colored output,
//! including block tree display, multi-replica comparison, view timelines, and statistics.

use std::collections::BTreeMap;
use colored::*;
use crate::types::*;
use crate::config::ReplicaId;

/// Enhanced block tree visualization with metadata
pub fn print_block_tree(blocks: &BTreeMap<u64, Block>) {
    print_block_tree_enhanced(blocks, None, None);
}

/// Print block tree with commit status and high QC indication
pub fn print_block_tree_enhanced(
    blocks: &BTreeMap<u64, Block>,
    committed_hashes: Option<&Vec<Hash>>,
    high_qc_hash: Option<Hash>,
) {
    fn print_subtree(
        blocks: &BTreeMap<u64, Block>,
        current: u64,
        prefix: String,
        is_last: bool,
        committed_hashes: Option<&Vec<Hash>>,
        high_qc_hash: Option<Hash>,
    ) {
        let connector = if is_last { "└─ " } else { "├─ " };
        let block = &blocks[&current];

        // Build block info string
        let mut info = format!("B{}", block.hash);
        
        // Add view and proposer metadata
        info.push_str(&format!(" (v:{}, p:R{})", block.view, block.proposer));
        
        // Add QC marker
        if block.qc.is_some() {
            info.push_str(&format!(" {}", "[QC]".bright_cyan()));
        }
        
        // Add HIGH_QC marker
        if high_qc_hash.is_some() && high_qc_hash.unwrap() == block.hash {
            info.push_str(&format!(" {}", "[HIGH_QC]".bright_yellow().bold()));
        }
        
        // Add commit status
        if let Some(committed) = committed_hashes {
            if committed.contains(&block.hash) {
                info.push_str(&format!(" {}", "✓committed".green().bold()));
            }
        }

        println!("{}{}{}", prefix, connector, info);

        let children: Vec<u64> = blocks
            .values()
            .filter(|b| b.parent == Some(current))
            .map(|b| b.hash)
            .collect();

        let new_prefix = prefix + if is_last { "   " } else { "│  " };

        for (i, child) in children.iter().enumerate() {
            print_subtree(
                blocks,
                *child,
                new_prefix.clone(),
                i == children.len() - 1,
                committed_hashes,
                high_qc_hash,
            );
        }
    }

    println!("{}", "Block Tree:".bright_white().bold());
    println!("{}", "B0 (v:0, p:R0) [genesis]".bright_magenta());
    print_subtree(blocks, 0, String::from(" "), true, committed_hashes, high_qc_hash);
}

/// Print multiple replicas' block trees side-by-side
pub fn print_replicas_comparison(replicas_data: Vec<ReplicaVisualizationData>) {
    println!("\n{}", "═══ MULTI-REPLICA COMPARISON ═══".bright_white().bold().underline());
    
    for (idx, data) in replicas_data.iter().enumerate() {
        if idx > 0 {
            println!(); // Spacing between replicas
        }
        
        println!("\n{}", format!("┌─── Replica {} (View: {}) ───┐", data.replica_id, data.current_view)
            .bright_blue().bold());
        
        if let Some(qc) = &data.high_qc {
            println!("│ High QC: Block {} (view {})", qc.block_hash, qc.view);
        } else {
            println!("│ High QC: None");
        }
        
        println!("│ Committed: {} blocks", data.committed_log.len());
        println!("{}", "└─────────────────────────────┘".bright_blue());
        
        let committed_hashes: Vec<Hash> = data.committed_log.iter().map(|b| b.hash).collect();
        let high_qc_hash = data.high_qc.as_ref().map(|qc| qc.block_hash);
        
        print_block_tree_enhanced(&data.block_tree, Some(&committed_hashes), high_qc_hash);
    }
    
    println!("\n{}", "═════════════════════════════════".bright_white().bold());
}

/// Data structure for replica visualization
#[derive(Clone)]
pub struct ReplicaVisualizationData {
    pub replica_id: ReplicaId,
    pub current_view: u64,
    pub block_tree: BTreeMap<Hash, Block>,
    pub high_qc: Option<QuorumCert>,
    pub committed_log: Vec<Block>,
}

/// Display view timeline showing block proposals per view
pub fn print_view_timeline(blocks: &BTreeMap<u64, Block>, max_view: u64) {
    println!("\n{}", "═══ VIEW TIMELINE ═══".bright_white().bold().underline());
    
    for view in 0..=max_view {
        let blocks_in_view: Vec<&Block> = blocks
            .values()
            .filter(|b| b.view == view)
            .collect();
        
        if !blocks_in_view.is_empty() {
            print!("{}: ", format!("View {}", view).bright_yellow());
            
            for (i, block) in blocks_in_view.iter().enumerate() {
                if i > 0 {
                    print!(", ");
                }
                let block_str = format!("B{}(R{})", block.hash, block.proposer);
                if block.qc.is_some() {
                    print!("{}", block_str.bright_green());
                } else {
                    print!("{}", block_str.white());
                }
            }
            println!();
        } else {
            println!("{}: {}", format!("View {}", view).bright_yellow(), "---".dimmed());
        }
    }
    
    println!("{}", "═════════════════════".bright_white().bold());
}

/// Display QC details for a given block
pub fn print_qc_details(qc: &QuorumCert) {
    println!("\n{}", "QC Details:".bright_cyan().bold());
    println!("  Block Hash: {}", qc.block_hash);
    println!("  View: {}", qc.view);
    println!("  Signatures: {} replicas", qc.signatures.len());
    
    if !qc.signatures.is_empty() {
        print!("  Signers: ");
        for (i, sig) in qc.signatures.iter().enumerate() {
            if i > 0 {
                print!(", ");
            }
            print!("R{}", sig.signer);
        }
        println!();
    }
}

/// Display commit log with details
pub fn print_commit_log(committed_log: &[Block]) {
    println!("\n{}", "═══ COMMITTED BLOCKS ═══".bright_green().bold().underline());
    
    if committed_log.is_empty() {
        println!("{}", "No blocks committed yet".dimmed());
    } else {
        for (idx, block) in committed_log.iter().enumerate() {
            println!(
                "{} {} {} {}",
                format!("#{}", idx).bright_black(),
                format!("B{}", block.hash).green().bold(),
                format!("(view:{}, proposer:R{})", block.view, block.proposer).white(),
                if block.qc.is_some() { "[QC]".cyan() } else { "".normal() }
            );
        }
    }
    
    println!("{}", "════════════════════════".bright_green().bold());
}

/// Display replica statistics
pub fn print_replica_stats(
    replica_id: ReplicaId,
    current_view: u64,
    total_blocks: usize,
    committed_blocks: usize,
    vote_pool_size: usize,
) {
    println!("\n{}", "═══ REPLICA STATISTICS ═══".bright_white().bold().underline());
    println!("  Replica ID: {}", format!("R{}", replica_id).bright_blue().bold());
    println!("  Current View: {}", format!("{}", current_view).yellow());
    println!("  Total Blocks in Tree: {}", total_blocks);
    println!("  Committed Blocks: {}", format!("{}", committed_blocks).green().bold());
    println!("  Active Votes Collected: {}", vote_pool_size);
    println!("{}", "═══════════════════════════".bright_white().bold());
}
/// Compare two replicas side-by-side (convenience function)
pub fn compare_replicas_side_by_side(
    replica1: &crate::replica::Replica, 
    replica2: &crate::replica::Replica,
    label1: &str,
    label2: &str
) {
    let data1 = replica1.get_visualization_data();
    let data2 = replica2.get_visualization_data();
    
    println!("\n{}", format!("╔═══ {} vs {} ═══╗", label1, label2).bright_white().bold().underline());
    println!("\n{:<35} │ {}", label1.bright_cyan().bold(), label2.bright_cyan().bold());
    println!("{}", "─".repeat(70));
    
    println!("{:<35} │ {}", 
        format!("View: {}", data1.current_view).yellow(), 
        format!("View: {}", data2.current_view).yellow()
    );
    
    println!("{:<35} │ {}", 
        format!("Blocks: {}", data1.block_tree.len()), 
        format!("Blocks: {}", data2.block_tree.len())
    );
    
    println!("{:<35} │ {}", 
        format!("Committed: {}", data1.committed_log.len()).green(), 
        format!("Committed: {}", data2.committed_log.len()).green()
    );
    
    let high_qc1 = if let Some(qc) = &data1.high_qc {
        format!("Block {} (v:{})", qc.block_hash, qc.view)
    } else {
        "None".to_string()
    };
    
    let high_qc2 = if let Some(qc) = &data2.high_qc {
        format!("Block {} (v:{})", qc.block_hash, qc.view)
    } else {
        "None".to_string()
    };
    
    println!("{:<35} │ {}", 
        format!("High QC: {}", high_qc1).bright_magenta(), 
        format!("High QC: {}", high_qc2).bright_magenta()
    );
    
    // Compare committed logs
    if data1.committed_log == data2.committed_log {
        println!("\n{}", "✅ Committed logs are IDENTICAL (Safety preserved!)".green().bold());
    } else {
        println!("\n{}", "⚠️  WARNING: Committed logs DIFFER!".red().bold());
    }
    
    println!("{}", "═".repeat(70));
}