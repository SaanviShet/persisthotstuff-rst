use std::collections::BTreeMap;
use persisthotstuff_rst::config::Config;
use persisthotstuff_rst::replica::Replica;
use persisthotstuff_rst::types::*;
use persisthotstuff_rst::crypto::KeyStore;
use persisthotstuff_rst::visualiser::{print_replicas_comparison, print_qc_details};

fn main() {
    println!("\n{}", "╔═══════════════════════════════════════════════════════════╗");
    println!("{}", "║     PersistHotStuff - Enhanced Visualization Demo        ║");
    println!("{}", "╚═══════════════════════════════════════════════════════════╝");

    let keystore = KeyStore::new(4);

    // Create first replica
    let config = Config { n: 4, f: 1, id: 0, timeout_ms: 5000 };

    let mut replica = Replica {
        config: config.clone(),
        current_view: 0,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
        keystore: keystore.clone(),
    };

    // Genesis
    replica.block_tree.insert(0, Block {
        hash: 0,
        parent: None,
        view: 0,
        proposer: 0,
        qc: None,
    });

    // B1
    replica.block_tree.insert(1, Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        proposer: 1,
        qc: Some(dummy_qc(0, 0)),
    });

    // B2
    replica.block_tree.insert(2, Block {
        hash: 2,
        parent: Some(1),
        view: 2,
        proposer: 2,
        qc: Some(dummy_qc(1, 1)),
    });

    // B3
    replica.block_tree.insert(3, Block {
        hash: 3,
        parent: Some(2),
        view: 3,
        proposer: 3,
        qc: Some(dummy_qc(2, 2)),
    });

    // B4
    replica.block_tree.insert(4, Block {
        hash: 4,
        parent: Some(3),
        view: 4,
        proposer: 0,
        qc: Some(dummy_qc(3, 3)),
    });

    replica.high_qc = replica.block_tree.get(&3).unwrap().qc.clone();

    println!("\n{}", "=== INITIAL STATE ===");
    replica.visualize();

    // Show view timeline
    replica.show_timeline();

    // Demonstrate pacemaker: show view and leader
    println!("\n{}", "=== PACEMAKER DEMO ===");
    println!("Initial View: {}, Leader: R{}", replica.current_view, replica.current_leader());
    
    // Try to commit blocks using the 3-chain rule
    println!("\n{}", "=== ATTEMPTING COMMITS (3-CHAIN RULE) ===");
    replica.commit_all();

    // Show updated state after commits
    println!("\n{}", "=== STATE AFTER COMMITS ===");
    replica.visualize();

    // Show detailed statistics
    replica.show_stats();

    // Simulate timeout based view changes
    println!("\n{}", "=== SIMULATING VIEW CHANGES ===");
    for i in 0..3 {
        replica.on_view_timeout();
        println!("View changed to: {}, New Leader: R{}", replica.current_view, replica.current_leader());
    }

    // Demonstrate QC details
    if let Some(qc) = &replica.high_qc {
        print_qc_details(qc);
    }

    // Create a second replica to demonstrate comparison
    println!("\n\n{}", "=== MULTI-REPLICA COMPARISON DEMO ===");
    
    let config2 = Config { n: 4, f: 1, id: 1, timeout_ms: 5000 };
    let mut replica2 = Replica {
        config: config2.clone(),
        current_view: 2,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
        keystore: keystore.clone(),
    };

    // Replica 2 has slightly different state (simulating network delay/partition)
    replica2.block_tree.insert(0, Block {
        hash: 0,
        parent: None,
        view: 0,
        proposer: 0,
        qc: None,
    });

    replica2.block_tree.insert(1, Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        proposer: 1,
        qc: Some(dummy_qc(0, 0)),
    });

    replica2.block_tree.insert(2, Block {
        hash: 2,
        parent: Some(1),
        view: 2,
        proposer: 2,
        qc: Some(dummy_qc(1, 1)),
    });

    // Replica 2 only has up to B2
    replica2.high_qc = replica2.block_tree.get(&2).unwrap().qc.clone();
    replica2.commit_all();

    // Create a third replica with a fork
    let config3 = Config { n: 4, f: 1, id: 2, timeout_ms: 5000 };
    let mut replica3 = Replica {
        config: config3.clone(),
        current_view: 3,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
        keystore: keystore.clone(),
    };

    // Replica 3 has same base but different fork
    replica3.block_tree.insert(0, Block {
        hash: 0,
        parent: None,
        view: 0,
        proposer: 0,
        qc: None,
    });

    replica3.block_tree.insert(1, Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        proposer: 1,
        qc: Some(dummy_qc(0, 0)),
    });

    replica3.block_tree.insert(2, Block {
        hash: 2,
        parent: Some(1),
        view: 2,
        proposer: 2,
        qc: Some(dummy_qc(1, 1)),
    });

    // Fork: different block 5 instead of continuing with 3
    replica3.block_tree.insert(5, Block {
        hash: 5,
        parent: Some(2),
        view: 3,
        proposer: 3,
        qc: Some(dummy_qc(2, 2)),
    });

    replica3.high_qc = replica3.block_tree.get(&2).unwrap().qc.clone();

    // Compare all three replicas
    let viz_data = vec![
        replica.get_visualization_data(),
        replica2.get_visualization_data(),
        replica3.get_visualization_data(),
    ];
    
    print_replicas_comparison(viz_data);

    println!("\n{}", "╔═══════════════════════════════════════════════════════════╗");
    println!("{}", "║   Enhanced Visualization Implementation Complete! ✓      ║");
    println!("{}", "╚═══════════════════════════════════════════════════════════╝\n");
}