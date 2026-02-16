use persisthotstuff_rst::config::Config;
use persisthotstuff_rst::replica::Replica;
use persisthotstuff_rst::types::*;
use persisthotstuff_rst::crypto::KeyStore;
use std::collections::BTreeMap;

#[test]
fn three_chain_commits_block() {
    let config = Config { n: 4, f: 1, id: 0, timeout_ms: 5000 };

    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let mut replica = Replica {
        config: config.clone(),
        current_view: 3,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
        keystore: keystores[0].clone(),
    };

    // Genesis block B0
    let b0 = Block {
        hash: 0,
        parent: None,
        view: 0,
        proposer: 0,
        qc: None,
    };
    replica.block_tree.insert(b0.hash, b0.clone());

    // Block B1 with QC on B0
    let b1 = Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        proposer: 1,
        qc: Some(QuorumCert {
            block_hash: 0,
            view: 0,
            signatures: vec![
                persisthotstuff_rst::crypto::sign(0),
                persisthotstuff_rst::crypto::sign(1),
                persisthotstuff_rst::crypto::sign(2),
            ],
        }),
    };
    replica.block_tree.insert(b1.hash, b1.clone());

    // Block B2 with QC on B1
    let b2 = Block {
        hash: 2,
        parent: Some(1),
        view: 2,
        proposer: 2,
        qc: Some(QuorumCert {
            block_hash: 1,
            view: 1,
            signatures: vec![
                persisthotstuff_rst::crypto::sign(0),
                persisthotstuff_rst::crypto::sign(1),
                persisthotstuff_rst::crypto::sign(2),
            ],
        }),
    };
    replica.block_tree.insert(b2.hash, b2.clone());

    // B0 forms a 3-chain: B0 <- B1 (with QC) <- B2 (with QC)
    let committed = replica.find_committed_block();
    assert!(committed.is_some(), "Should find committed block in 3-chain");
    assert_eq!(committed.unwrap().hash, 0, "B0 should be committed");
}

#[test]
fn insufficient_qc_blocks_dont_commit() {
    let config = Config { n: 4, f: 1, id: 0, timeout_ms: 5000 };

    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let mut replica = Replica {
        config: config.clone(),
        current_view: 2,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
        keystore: keystores[0].clone(),
    };

    // Genesis block B0 (no QC)
    let b0 = Block {
        hash: 0,
        parent: None,
        view: 0,
        proposer: 0,
        qc: None,
    };
    replica.block_tree.insert(b0.hash, b0.clone());

    // Block B1 without QC
    let b1 = Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        proposer: 1,
        qc: None,
    };
    replica.block_tree.insert(b1.hash, b1.clone());

    // Block B2 without QC
    let b2 = Block {
        hash: 2,
        parent: Some(1),
        view: 2,
        proposer: 2,
        qc: None,
    };
    replica.block_tree.insert(b2.hash, b2.clone());

    // Without QCs, no block should be committed
    let committed = replica.find_committed_block();
    assert!(
        committed.is_none(),
        "Should not commit blocks without QCs in the chain"
    );
}

#[test]
fn commit_log_grows_correctly() {
    let config = Config { n: 4, f: 1, id: 0, timeout_ms: 5000 };

    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let mut replica = Replica {
        config: config.clone(),
        current_view: 5,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
        keystore: keystores[0].clone(),
    };

    // Genesis block B0
    let b0 = Block {
        hash: 0,
        parent: None,
        view: 0,
        proposer: 0,
        qc: None,
    };
    replica.block_tree.insert(b0.hash, b0);

    // B1 with QC on B0
    let b1 = Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        proposer: 1,
        qc: Some(QuorumCert {
            block_hash: 0,
            view: 0,
            signatures: vec![
                persisthotstuff_rst::crypto::sign(0),
                persisthotstuff_rst::crypto::sign(1),
                persisthotstuff_rst::crypto::sign(2),
            ],
        }),
    };
    replica.block_tree.insert(b1.hash, b1);

    // B2 with QC on B1
    let b2 = Block {
        hash: 2,
        parent: Some(1),
        view: 2,
        proposer: 2,
        qc: Some(QuorumCert {
            block_hash: 1,
            view: 1,
            signatures: vec![
                persisthotstuff_rst::crypto::sign(0),
                persisthotstuff_rst::crypto::sign(1),
                persisthotstuff_rst::crypto::sign(2),
            ],
        }),
    };
    replica.block_tree.insert(b2.hash, b2);

    // B3 with QC on B2
    let b3 = Block {
        hash: 3,
        parent: Some(2),
        view: 3,
        proposer: 3,
        qc: Some(QuorumCert {
            block_hash: 2,
            view: 2,
            signatures: vec![
                persisthotstuff_rst::crypto::sign(0),
                persisthotstuff_rst::crypto::sign(1),
                persisthotstuff_rst::crypto::sign(2),
            ],
        }),
    };
    replica.block_tree.insert(b3.hash, b3);

    // B4 with QC on B3
    let b4 = Block {
        hash: 4,
        parent: Some(3),
        view: 4,
        proposer: 0,
        qc: Some(QuorumCert {
            block_hash: 3,
            view: 3,
            signatures: vec![
                persisthotstuff_rst::crypto::sign(0),
                persisthotstuff_rst::crypto::sign(1),
                persisthotstuff_rst::crypto::sign(2),
            ],
        }),
    };
    replica.block_tree.insert(b4.hash, b4);

    // Commit all possible blocks
    replica.commit_all();

    // Should have committed B0 (3-chain: B0 <- B1 <- B2)
    // and B1 (3-chain: B1 <- B2 <- B3) and B2 (3-chain: B2 <- B3 <- B4)
    assert!(
        replica.committed_log.len() >= 1,
        "Should have at least one committed block"
    );
    assert_eq!(
        replica.committed_log[0].hash, 0,
        "First committed block should be B0"
    );
}

#[test]
fn cannot_commit_same_block_twice() {
    let config = Config { n: 4, f: 1, id: 0, timeout_ms: 5000 };

    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let mut replica = Replica {
        config: config.clone(),
        current_view: 3,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
        keystore: keystores[0].clone(),
    };

    // Genesis block B0
    let b0 = Block {
        hash: 0,
        parent: None,
        view: 0,
        proposer: 0,
        qc: None,
    };
    replica.block_tree.insert(b0.hash, b0.clone());

    // Block B1 with QC on B0
    let b1 = Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        proposer: 1,
        qc: Some(QuorumCert {
            block_hash: 0,
            view: 0,
            signatures: vec![
                persisthotstuff_rst::crypto::sign(0),
                persisthotstuff_rst::crypto::sign(1),
                persisthotstuff_rst::crypto::sign(2),
            ],
        }),
    };
    replica.block_tree.insert(b1.hash, b1.clone());

    // Block B2 with QC on B1
    let b2 = Block {
        hash: 2,
        parent: Some(1),
        view: 2,
        proposer: 2,
        qc: Some(QuorumCert {
            block_hash: 1,
            view: 1,
            signatures: vec![
                persisthotstuff_rst::crypto::sign(0),
                persisthotstuff_rst::crypto::sign(1),
                persisthotstuff_rst::crypto::sign(2),
            ],
        }),
    };
    replica.block_tree.insert(b2.hash, b2.clone());

    // Commit once
    replica.try_commit_once();
    assert_eq!(replica.committed_log.len(), 1);

    // Try to commit again (should not commit the same block)
    let result = replica.try_commit_once();
    assert!(!result, "Should not commit if no new 3-chain is found");
    assert_eq!(
        replica.committed_log.len(),
        1,
        "Log size should remain unchanged"
    );
}
