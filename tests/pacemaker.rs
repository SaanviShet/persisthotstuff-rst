use persisthotstuff_rst::config::Config;
use persisthotstuff_rst::replica::Replica;
use persisthotstuff_rst::types::*;
use persisthotstuff_rst::crypto::KeyStore;
use std::collections::BTreeMap;

#[test]
fn timeout_increments_view() {
    let config = Config { n: 4, f: 1, id: 0, timeout_ms: 1000 };
    
    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let mut replica = Replica {
        config: config.clone(),
        current_view: 0,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 1000,
        view_start_time: Replica::current_time_ms(),
        keystore: keystores[0].clone(),
    };

    let initial_view = replica.current_view;
    replica.on_view_timeout();
    
    assert_eq!(replica.current_view, initial_view + 1, "View should increment on timeout");
}

#[test]
fn view_timeout_clears_vote_pool() {
    let config = Config { n: 4, f: 1, id: 0, timeout_ms: 1000 };

    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let mut replica = Replica {
        config: config.clone(),
        current_view: 0,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 1000,
        view_start_time: Replica::current_time_ms(),
        keystore: keystores[0].clone(),
    };

    // Add some votes
    let sig = persisthotstuff_rst::crypto::sign(0);
    replica.vote_pool.insert(42, vec![sig]);
    
    assert!(!replica.vote_pool.is_empty(), "Vote pool should have votes");
    
    replica.on_view_timeout();
    
    assert!(replica.vote_pool.is_empty(), "Vote pool should be cleared on view timeout");
}

#[test]
fn correct_leader_selected_per_view() {
    let config = Config { n: 4, f: 1, id: 0, timeout_ms: 1000 };

    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let mut replica = Replica {
        config: config.clone(),
        current_view: 0,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 1000,
        view_start_time: Replica::current_time_ms(),
        keystore: keystores[0].clone(),
    };

    // Test round-robin leader selection
    // With n=4, leaders cycle: 0 -> 1 -> 2 -> 3 -> 0 -> ...
    let expected_leaders = [0, 1, 2, 3, 0, 1, 2, 3];
    
    for (view, &expected_leader) in expected_leaders.iter().enumerate() {
        replica.start_view(view as u64);
        let leader = replica.current_leader();
        assert_eq!(leader, expected_leader as u64, "View {} should have leader {}", view, expected_leader);
    }
}

#[test]
fn am_i_leader_works_correctly() {
    let config = Config { n: 4, f: 1, id: 1, timeout_ms: 1000 };

    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let mut replica = Replica {
        config: config.clone(),
        current_view: 0,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 1000,
        view_start_time: Replica::current_time_ms(),
        keystore: keystores[1].clone(),  // Use the keystore for replica id=1
    };

    // Replica id=1, view=0: leader should be 0 % 4 = 0, so not leader
    replica.start_view(0);
    assert!(!replica.am_i_leader(), "Replica 1 should not be leader in view 0");
    
    // View=1: leader should be 1 % 4 = 1, so am leader
    replica.start_view(1);
    assert!(replica.am_i_leader(), "Replica 1 should be leader in view 1");
    
    // View=5: leader should be 5 % 4 = 1, so am leader
    replica.start_view(5);
    assert!(replica.am_i_leader(), "Replica 1 should be leader in view 5");
}

#[test]
fn reset_timer_on_proposal() {
    let config = Config { n: 4, f: 1, id: 0, timeout_ms: 5000 };

    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

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
        view_start_time: 0,  // Old time
        keystore: keystores[0].clone(),
    };

    let old_time = replica.view_start_time;
    replica.on_inserting_block_proposal();
    
    assert!(replica.view_start_time > old_time, "Timer should be reset when proposal received");
}

#[test]
fn reset_timer_on_commit() {
    let config = Config { n: 4, f: 1, id: 0, timeout_ms: 5000 };

    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

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
        view_start_time: 0,  // Old time
        keystore: keystores[0].clone(),
    };

    let old_time = replica.view_start_time;
    replica.on_commit();
    
    assert!(replica.view_start_time > old_time, "Timer should be reset when block committed");
}

#[test]
fn sequential_view_changes() {
    let config = Config { n: 4, f: 1, id: 2, timeout_ms: 1000 };

    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let mut replica = Replica {
        config: config.clone(),
        current_view: 0,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 1000,
        view_start_time: Replica::current_time_ms(),
        keystore: keystores[2].clone(),
    };

    // Simulate several view changes and check leader transitions
    for i in 0..8 {
        let expected_leader = (i % 4) as u64;
        assert_eq!(replica.current_leader(), expected_leader, "View {} should have leader {}", i, expected_leader);
        replica.on_view_timeout();
    }
    
    // After 8 timeouts, we should be at view 8
    assert_eq!(replica.current_view, 8, "Should be at view 8 after 8 timeouts");
}
