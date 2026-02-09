use persisthotstuff_rst::config::Config;
use persisthotstuff_rst::replica::Replica;
use persisthotstuff_rst::types::*;
use std::collections::BTreeMap;

#[test]
fn leader_proposes_block() {
    let config = Config { n: 4, f: 1, id: 1 , timeout_ms: 5000};

    let mut replica = Replica {
        config: config.clone(),
        current_view: 1,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
    };

    let block_opt = replica.propose(1);
    assert!(block_opt.is_some());
    let block = block_opt.unwrap();
    assert_eq!(block.proposer, replica.config.id);
    assert!(replica.block_tree.contains_key(&block.hash));
}

#[test]
fn non_leader_cannot_propose() {
    let config = Config { n: 4, f: 1, id: 0, timeout_ms: 5000 };

    let mut replica = Replica {
        config: config.clone(),
        current_view: 1,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
    };

    assert!(replica.propose(1).is_none());
}

#[test]
fn proposal_validation_accepts_valid() {
    let leader_cfg = Config { n: 4, f: 1, id: 1, timeout_ms: 5000 };
    let mut leader = Replica {
        config: leader_cfg.clone(),
        current_view: 1,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
    };

    let follower_cfg = Config { n: 4, f: 1, id: 2, timeout_ms: 5000 };
    let mut follower = Replica {
        config: follower_cfg.clone(),
        current_view: 1,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
    };

    let block = leader.propose(1).expect("leader should propose");
    let ok = follower.validate_and_insert_proposal(block.clone());
    assert!(ok);
    assert!(follower.block_tree.contains_key(&block.hash));
}

#[test]
fn proposal_validation_rejects_invalid_qc() {
    let leader_cfg = Config { n: 4, f: 1, id: 1, timeout_ms: 5000 };
    let mut leader = Replica {
        config: leader_cfg.clone(),
        current_view: 1,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
    };

    let follower_cfg = Config { n: 4, f: 1, id: 2, timeout_ms: 5000 };
    let mut follower = Replica {
        config: follower_cfg.clone(),
        current_view: 1,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
    };

    // Create a block with a QC that has insufficient signatures
    let mut block = leader.propose(1).expect("leader should propose");
    block.qc = Some(QuorumCert { block_hash: 0, view: 0, signatures: vec![persisthotstuff_rst::crypto::sign(0)] });

    let ok = follower.validate_and_insert_proposal(block);
    assert!(!ok, "proposal with invalid QC should be rejected");
}
