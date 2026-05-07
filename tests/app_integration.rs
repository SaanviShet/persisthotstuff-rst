//! Integration tests for the App trait and Replica integration.
//!
//! These tests verify that the pluggable state machine interface works
//! correctly when wired into the consensus commit pipeline.

use persisthotstuff_rst::app::{App, AppError, CommittedCommand, KeyValueApp, NoOpApp};
use persisthotstuff_rst::config::Config;
use persisthotstuff_rst::crypto::KeyStore;
use persisthotstuff_rst::replica::Replica;
use persisthotstuff_rst::types::*;
use std::collections::BTreeMap;

// ── Helper ───────────────────────────────────────────────────────────────

fn make_replica_with_app(app: Box<dyn App>) -> Replica {
    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);
    let config = Config {
        n: 4,
        f: 1,
        id: 0,
        timeout_ms: 5000,
    };

    let mut replica = Replica {
        config: config.clone(),
        current_view: 0,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 1,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: Replica::current_time_ms(),
        active_validators: (0..config.n as u64).collect(),
        config_epoch: 0,
        keystore: keystores[0].clone(),
        wal: None,
        snapshot_counter: 0,
        app: None,
        pending_app_state: None,
        client_queue: Vec::new(),
        dummy_proposal_enabled: false,
        last_proposed_time: 0,
        dummy_timeout_ms: 0,
    };

    // Insert genesis.
    replica.block_tree.insert(
        0,
        Block {
            hash: 0,
            parent: None,
            view: 0,
            epoch: 0,
            proposer: 0,
            qc: None,
            command: ConsensusCommand::NoOp,
        },
    );

    // Attach the app.
    replica.attach_app(app);
    replica
}

/// Build a 3-chain and commit a block carrying `command`.
///
/// Creates blocks B1(command) → B2(NoOp, QC(B1)) → B3(NoOp, QC(B2))
/// and calls commit_all(), which should commit B1 through the 3-chain rule.
fn build_and_commit(replica: &mut Replica, command: ConsensusCommand) {
    // B1 carries the target command.
    let b1 = Block {
        hash: replica.next_hash,
        parent: Some(0),
        view: 1,
        epoch: 0,
        proposer: 0,
        qc: Some(dummy_qc(0, 0)),
        command,
    };
    replica.next_hash += 1;
    replica.block_tree.insert(b1.hash, b1.clone());

    // B2 extends B1 with a QC on B1.
    let b2 = Block {
        hash: replica.next_hash,
        parent: Some(b1.hash),
        view: 2,
        epoch: 0,
        proposer: 0,
        qc: Some(dummy_qc(b1.hash, 1)),
        command: ConsensusCommand::NoOp,
    };
    replica.next_hash += 1;
    replica.block_tree.insert(b2.hash, b2.clone());

    // B3 extends B2 with a QC on B2 — now B1 satisfies the 3-chain.
    let b3 = Block {
        hash: replica.next_hash,
        parent: Some(b2.hash),
        view: 3,
        epoch: 0,
        proposer: 0,
        qc: Some(dummy_qc(b2.hash, 2)),
        command: ConsensusCommand::NoOp,
    };
    replica.next_hash += 1;
    replica.block_tree.insert(b3.hash, b3);

    replica.commit_all();
}

// ══════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn noop_app_does_not_affect_commit() {
    let mut replica = make_replica_with_app(Box::new(NoOpApp::new()));
    build_and_commit(&mut replica, ConsensusCommand::NoOp);
    // Genesis + B1 are both committed by the 3-chain rule.
    assert_eq!(replica.committed_log.len(), 2);
}

#[test]
fn kv_app_receives_committed_client_tx() {
    let kv = KeyValueApp::new();
    let mut replica = make_replica_with_app(Box::new(kv));

    build_and_commit(
        &mut replica,
        ConsensusCommand::ClientTx("SET greeting hello".into()),
    );

    // Genesis + B1 are both committed by the 3-chain rule.
    assert_eq!(replica.committed_log.len(), 2);

    // Verify the KV store was updated through the App trait.
    let app_ref = replica.app.as_ref().unwrap();
    // We can't downcast easily, but we can snapshot and check.
    let state = app_ref.snapshot().unwrap();
    let mut checker = KeyValueApp::new();
    checker.restore(&state).unwrap();
    assert_eq!(checker.get("greeting").unwrap(), "hello");
}

#[test]
fn kv_app_ignores_noop_blocks() {
    let kv = KeyValueApp::new();
    let mut replica = make_replica_with_app(Box::new(kv));

    build_and_commit(&mut replica, ConsensusCommand::NoOp);

    let state = replica.app.as_ref().unwrap().snapshot().unwrap();
    let mut checker = KeyValueApp::new();
    checker.restore(&state).unwrap();
    assert!(checker.is_empty());
}

#[test]
fn kv_app_multiple_commits_accumulate_state() {
    let kv = KeyValueApp::new();
    let mut replica = make_replica_with_app(Box::new(kv));

    // Commit first KV command.
    build_and_commit(
        &mut replica,
        ConsensusCommand::ClientTx("SET x 1".into()),
    );

    // Build another 3-chain for a second commit.
    let base = replica.next_hash - 1; // last inserted hash
    let b4 = Block {
        hash: replica.next_hash,
        parent: Some(base),
        view: 4,
        epoch: 0,
        proposer: 0,
        qc: Some(dummy_qc(base, 3)),
        command: ConsensusCommand::ClientTx("SET y 2".into()),
    };
    replica.next_hash += 1;
    replica.block_tree.insert(b4.hash, b4.clone());

    let b5 = Block {
        hash: replica.next_hash,
        parent: Some(b4.hash),
        view: 5,
        epoch: 0,
        proposer: 0,
        qc: Some(dummy_qc(b4.hash, 4)),
        command: ConsensusCommand::NoOp,
    };
    replica.next_hash += 1;
    replica.block_tree.insert(b5.hash, b5.clone());

    let b6 = Block {
        hash: replica.next_hash,
        parent: Some(b5.hash),
        view: 6,
        epoch: 0,
        proposer: 0,
        qc: Some(dummy_qc(b5.hash, 5)),
        command: ConsensusCommand::NoOp,
    };
    replica.next_hash += 1;
    replica.block_tree.insert(b6.hash, b6);
    replica.commit_all();

    // Both SET x and SET y should be in the KV store.
    let state = replica.app.as_ref().unwrap().snapshot().unwrap();
    let mut checker = KeyValueApp::new();
    checker.restore(&state).unwrap();
    assert_eq!(checker.get("x").unwrap(), "1");
    assert_eq!(checker.get("y").unwrap(), "2");
}

#[test]
fn attach_app_restores_pending_state() {
    // Simulate: replica recovered with pending_app_state, then app attached.
    let mut kv1 = KeyValueApp::new();
    let cmd = CommittedCommand {
        block_hash: 1,
        view: 1,
        epoch: 0,
        proposer: 0,
        command: ConsensusCommand::ClientTx("SET restored yes".into()),
        commit_index: 0,
    };
    kv1.apply(&cmd).unwrap();
    let state = kv1.snapshot().unwrap();

    // Build a replica with pending_app_state (as if from recovery).
    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);
    let config = Config {
        n: 4,
        f: 1,
        id: 0,
        timeout_ms: 5000,
    };

    let mut replica = Replica {
        config: config.clone(),
        current_view: 0,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 1,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
        active_validators: (0..config.n as u64).collect(),
        config_epoch: 0,
        keystore: keystores[0].clone(),
        wal: None,
        snapshot_counter: 0,
        app: None,
        pending_app_state: Some(state),
        client_queue: Vec::new(),
        dummy_proposal_enabled: false,
        last_proposed_time: 0,
        dummy_timeout_ms: 0,
    };

    // Now attach a fresh KV app — it should be restored from pending_app_state.
    let kv2 = KeyValueApp::new();
    replica.attach_app(Box::new(kv2));

    let restored_state = replica.app.as_ref().unwrap().snapshot().unwrap();
    let mut checker = KeyValueApp::new();
    checker.restore(&restored_state).unwrap();
    assert_eq!(checker.get("restored").unwrap(), "yes");

    // pending_app_state should be consumed.
    assert!(replica.pending_app_state.is_none());
}

#[test]
fn snapshot_captures_app_state() {
    let kv = KeyValueApp::new();
    let mut replica = make_replica_with_app(Box::new(kv));

    build_and_commit(
        &mut replica,
        ConsensusCommand::ClientTx("SET snapped value".into()),
    );

    // Take snapshot via the Snapshot::capture path (simulate what take_snapshot does).
    let app_state = replica.app.as_ref().and_then(|a| a.snapshot().ok());
    assert!(app_state.is_some());

    // Verify the captured state contains our KV entry.
    let mut checker = KeyValueApp::new();
    checker.restore(&app_state.unwrap()).unwrap();
    assert_eq!(checker.get("snapped").unwrap(), "value");
}
