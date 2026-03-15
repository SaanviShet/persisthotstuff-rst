use std::collections::{BTreeMap, BTreeSet};

use persisthotstuff_rst::config::Config;
use persisthotstuff_rst::crypto::KeyStore;
use persisthotstuff_rst::replica::Replica;
use persisthotstuff_rst::types::{Block, ConsensusCommand, QuorumCert, Vote};
use persisthotstuff_rst::wal::{LogEntry, ViewChangeReason};

fn mk_replica(config: Config, keystore: KeyStore, validators: BTreeSet<u64>, epoch: u64) -> Replica {
    let mut block_tree = BTreeMap::new();
    block_tree.insert(
        0,
        Block {
            hash: 0,
            parent: None,
            view: 0,
            epoch,
            proposer: 0,
            qc: None,
            command: ConsensusCommand::NoOp,
        },
    );

    Replica {
        config: config.clone(),
        current_view: 1,
        block_tree,
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 1,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: config.timeout_ms,
        view_start_time: Replica::current_time_ms(),
        active_validators: validators,
        config_epoch: epoch,
        keystore,
        wal: None,
        snapshot_counter: 0,
    }
}

#[test]
fn join_applied_only_after_commit() {
    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    // Start from 3 validators so single join to 4 is allowed by current invariant check.
    let config = Config {
        n: 3,
        f: 0,
        id: 0,
        timeout_ms: 1000,
    };
    let mut replica = mk_replica(config, keystores[0].clone(), [0, 1, 2].into_iter().collect(), 0);

    let join_cmd = ConsensusCommand::JoinValidator {
        replica_id: 3,
        public_key: keystores[3].my_public_key_bytes().to_vec(),
    };

    // Before commit: membership must not change.
    assert!(!replica.active_validators.contains(&3));

    let join_block = Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        epoch: 0,
        proposer: 1,
        qc: Some(QuorumCert {
            block_hash: 0,
            view: 0,
            epoch: 0,
            signatures: vec![],
        }),
        command: join_cmd,
    };

    // Insertion/proposal path should not apply membership.
    replica.block_tree.insert(1, join_block.clone());
    assert!(!replica.active_validators.contains(&3));

    // Commit path applies membership atomically.
    replica.execute_and_commit(join_block);
    assert!(replica.active_validators.contains(&3));
    assert_eq!(replica.config_epoch, 1);
}

#[test]
fn remove_applied_only_after_commit() {
    let all_keys = KeyStore::generate_keys(5);
    let keystores = KeyStore::distribute_keys(&all_keys);

    // Start from n=5 so single remove to n=4 is allowed by current invariant check.
    let config = Config {
        n: 5,
        f: 1,
        id: 0,
        timeout_ms: 1000,
    };
    let mut replica = mk_replica(
        config,
        keystores[0].clone(),
        [0, 1, 2, 3, 4].into_iter().collect(),
        0,
    );

    let remove_block = Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        epoch: 0,
        proposer: 1,
        qc: Some(QuorumCert {
            block_hash: 0,
            view: 0,
            epoch: 0,
            signatures: vec![],
        }),
        command: ConsensusCommand::RemoveValidator { replica_id: 4 },
    };

    // Before commit, validator still active.
    assert!(replica.active_validators.contains(&4));

    replica.block_tree.insert(1, remove_block.clone());
    assert!(replica.active_validators.contains(&4));

    replica.execute_and_commit(remove_block);
    assert!(!replica.active_validators.contains(&4));
    assert_eq!(replica.config_epoch, 1);
}

#[test]
fn reject_invalid_remove_breaking_invariant() {
    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let config = Config {
        n: 4,
        f: 1,
        id: 0,
        timeout_ms: 1000,
    };
    let mut replica = mk_replica(config, keystores[0].clone(), [0, 1, 2, 3].into_iter().collect(), 0);

    let remove_block = Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        epoch: 0,
        proposer: 1,
        qc: Some(QuorumCert {
            block_hash: 0,
            view: 0,
            epoch: 0,
            signatures: vec![],
        }),
        command: ConsensusCommand::RemoveValidator { replica_id: 3 },
    };

    replica.execute_and_commit(remove_block);

    // Current implementation rejects n=3 transition.
    assert!(replica.active_validators.contains(&3));
    assert_eq!(replica.dynamic_n(), 4);
    assert_eq!(replica.config_epoch, 0);
}

#[test]
fn quorum_recomputed_after_join_commit() {
    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let config = Config {
        n: 3,
        f: 0,
        id: 0,
        timeout_ms: 1000,
    };
    let mut replica = mk_replica(config, keystores[0].clone(), [0, 1, 2].into_iter().collect(), 0);

    assert_eq!(replica.dynamic_quorum_size(), 1);

    let join_block = Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        epoch: 0,
        proposer: 1,
        qc: Some(QuorumCert {
            block_hash: 0,
            view: 0,
            epoch: 0,
            signatures: vec![],
        }),
        command: ConsensusCommand::JoinValidator {
            replica_id: 3,
            public_key: keystores[3].my_public_key_bytes().to_vec(),
        },
    };

    replica.execute_and_commit(join_block);

    assert_eq!(replica.dynamic_n(), 4);
    assert_eq!(replica.dynamic_quorum_size(), 3);
}

#[test]
fn leader_rotation_uses_new_validator_set() {
    let all_keys = KeyStore::generate_keys(5);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let config = Config {
        n: 5,
        f: 1,
        id: 0,
        timeout_ms: 1000,
    };
    let replica = mk_replica(config, keystores[0].clone(), [0, 2, 5, 7].into_iter().collect(), 0);

    // Sorted active set: [0,2,5,7]
    assert_eq!(replica.dynamic_leader_for_view(0), 0);
    assert_eq!(replica.dynamic_leader_for_view(1), 2);
    assert_eq!(replica.dynamic_leader_for_view(2), 5);
    assert_eq!(replica.dynamic_leader_for_view(3), 7);
    assert_eq!(replica.dynamic_leader_for_view(4), 0);
}

#[test]
fn reject_vote_wrong_epoch() {
    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let config = Config {
        n: 4,
        f: 1,
        id: 0,
        timeout_ms: 1000,
    };
    let mut replica = mk_replica(config, keystores[0].clone(), [0, 1, 2, 3].into_iter().collect(), 0);

    let block_hash = 11;
    replica.block_tree.insert(
        block_hash,
        Block {
            hash: block_hash,
            parent: Some(0),
            view: 1,
            epoch: 0,
            proposer: 1,
            qc: None,
            command: ConsensusCommand::NoOp,
        },
    );

    // Vote signed for epoch=1 while replica expects epoch=0.
    let sig = keystores[1].sign(block_hash, 1, 1);
    let vote = Vote {
        block_hash,
        view: 1,
        epoch: 1,
        signature: sig,
    };

    let out = replica.handle_vote(vote);
    assert!(out.is_none());
    assert!(replica.vote_pool.get(&block_hash).is_none());
}

#[test]
fn reject_qc_wrong_epoch_on_proposal_validation() {
    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let config = Config {
        n: 4,
        f: 1,
        id: 2,
        timeout_ms: 1000,
    };
    let mut replica = mk_replica(config, keystores[2].clone(), [0, 1, 2, 3].into_iter().collect(), 0);

    // Parent block exists.
    replica.block_tree.insert(
        1,
        Block {
            hash: 1,
            parent: Some(0),
            view: 1,
            epoch: 0,
            proposer: 1,
            qc: None,
            command: ConsensusCommand::NoOp,
        },
    );

    // proposal for view=1 must be from leader=1 (under active set [0,1,2,3]).
    let proposal = Block {
        hash: 2,
        parent: Some(1),
        view: 1,
        epoch: 0,
        proposer: 1,
        qc: Some(QuorumCert {
            block_hash: 1,
            view: 1,
            epoch: 9, // wrong epoch
            signatures: vec![],
        }),
        command: ConsensusCommand::NoOp,
    };

    assert!(!replica.validate_and_insert_proposal(proposal));
}

#[test]
fn accept_vote_from_joined_validator_after_commit() {
    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    // Start at n=3 so single join to 4 is permitted.
    let config = Config {
        n: 3,
        f: 0,
        id: 0,
        timeout_ms: 1000,
    };
    let mut replica = mk_replica(config, keystores[0].clone(), [0, 1, 2].into_iter().collect(), 0);

    let join_block = Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        epoch: 0,
        proposer: 1,
        qc: Some(QuorumCert {
            block_hash: 0,
            view: 0,
            epoch: 0,
            signatures: vec![],
        }),
        command: ConsensusCommand::JoinValidator {
            replica_id: 3,
            public_key: keystores[3].my_public_key_bytes().to_vec(),
        },
    };
    replica.execute_and_commit(join_block);

    assert_eq!(replica.config_epoch, 1);
    assert!(replica.active_validators.contains(&3));

    let target_hash = 2;
    replica.block_tree.insert(
        target_hash,
        Block {
            hash: target_hash,
            parent: Some(1),
            view: 2,
            epoch: 1,
            proposer: 2,
            qc: None,
            command: ConsensusCommand::NoOp,
        },
    );

    let joined_sig = keystores[3].sign(target_hash, 2, 1);
    let vote = Vote {
        block_hash: target_hash,
        view: 2,
        epoch: 1,
        signature: joined_sig,
    };

    let _ = replica.handle_vote(vote);
    assert_eq!(replica.vote_pool.get(&target_hash).map(|v| v.len()), Some(1));
}

#[test]
fn catchup_import_for_joined_validator_restores_state() {
    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let config = Config {
        n: 4,
        f: 1,
        id: 3,
        timeout_ms: 1000,
    };
    let mut joined = mk_replica(config, keystores[3].clone(), [0, 1, 2, 3].into_iter().collect(), 0);

    let entries = vec![
        LogEntry::BlockInserted {
            hash: 9,
            parent: Some(0),
            view: 2,
            epoch: 0,
            proposer: 1,
            qc_block_hash: Some(0),
            qc_view: Some(0),
            command: ConsensusCommand::NoOp,
            timestamp: 1,
        },
        LogEntry::ViewChanged {
            old_view: 0,
            new_view: 2,
            reason: ViewChangeReason::Commit,
            timestamp: 2,
        },
    ];

    joined.import_catchup_state(None, &entries).unwrap();

    assert!(joined.block_tree.contains_key(&9));
    assert_eq!(joined.current_view, 2);
}
