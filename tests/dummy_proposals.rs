//! Tests for the Enhanced Pacemaker — dummy NoOp proposals.
//!
//! These tests verify the behaviour described in Section 4.4 of the
//! paper: when no client commands are available and the 3-chain is
//! incomplete, the pacemaker injects dummy (NoOp) blocks to keep the
//! protocol making progress.

use persisthotstuff_rst::config::Config;
use persisthotstuff_rst::replica::Replica;
use persisthotstuff_rst::simulation::Simulation;
use persisthotstuff_rst::types::*;
use persisthotstuff_rst::crypto::KeyStore;
use std::collections::BTreeMap;

// ── Helpers ──────────────────────────────────────────────────────────────

fn make_replica(id: u64) -> Replica {
    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);
    let config = Config {
        n: 4,
        f: 1,
        id,
        timeout_ms: 5000,
    };
    let mut replica = Replica {
        config: config.clone(),
        current_view: id,    // so this replica IS the leader for its view
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
        keystore: keystores[id as usize].clone(),
        wal: None,
        snapshot_counter: 0,
        app: None,
        pending_app_state: None,
        client_queue: Vec::new(),
        dummy_proposal_enabled: false,
        last_proposed_time: 0,
        dummy_timeout_ms: 0,
    };
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
    replica
}

// ── Unit-level tests on Replica ──────────────────────────────────────────

#[test]
fn should_propose_dummy_returns_false_when_disabled() {
    let replica = make_replica(0);
    assert!(!replica.should_propose_dummy());
}

#[test]
fn should_propose_dummy_returns_false_when_not_leader() {
    let mut replica = make_replica(0);
    replica.enable_dummy_proposals(0);       // zero-ms timeout
    replica.current_view = 1;                // leader is replica 1, not 0
    assert!(!replica.should_propose_dummy());
}

#[test]
fn should_propose_dummy_returns_false_when_queue_has_commands() {
    let mut replica = make_replica(0);
    replica.enable_dummy_proposals(0);
    replica.enqueue_command(ConsensusCommand::ClientTx("SET x 1".into()));
    assert!(!replica.should_propose_dummy());
}

#[test]
fn should_propose_dummy_returns_true_when_idle() {
    let mut replica = make_replica(0);
    replica.enable_dummy_proposals(0);       // zero-ms = immediately
    // There's 1 block (genesis) and 0 committed → 1 pending < 3
    assert!(replica.should_propose_dummy());
}

#[test]
fn should_propose_dummy_returns_false_when_3chain_complete() {
    let mut replica = make_replica(0);
    replica.enable_dummy_proposals(0);
    // Build a complete 3-chain so a block is ready to commit.
    // B1 extends genesis with a QC.
    replica.block_tree.insert(
        1,
        Block {
            hash: 1,
            parent: Some(0),
            view: 1,
            epoch: 0,
            proposer: 0,
            qc: Some(dummy_qc(0, 0)),
            command: ConsensusCommand::NoOp,
        },
    );
    // B2 extends B1 with a QC.
    replica.block_tree.insert(
        2,
        Block {
            hash: 2,
            parent: Some(1),
            view: 2,
            epoch: 0,
            proposer: 0,
            qc: Some(dummy_qc(1, 1)),
            command: ConsensusCommand::NoOp,
        },
    );
    // Now genesis has a 3-chain: genesis ← B1[QC] ← B2[QC].
    // find_committed_block() should return Some(genesis).
    assert!(replica.find_committed_block().is_some());
    assert!(!replica.should_propose_dummy());
}

#[test]
fn propose_next_dequeues_client_command() {
    let mut replica = make_replica(0);
    replica.enable_dummy_proposals(0);
    replica.enqueue_command(ConsensusCommand::ClientTx("SET a 1".into()));

    let block = replica.propose_next(0).expect("should propose client cmd");
    assert_eq!(block.command, ConsensusCommand::ClientTx("SET a 1".into()));
    assert!(replica.client_queue.is_empty());
}

#[test]
fn propose_next_falls_back_to_dummy() {
    let mut replica = make_replica(0);
    replica.enable_dummy_proposals(0);

    let block = replica.propose_next(0).expect("should propose dummy");
    assert_eq!(block.command, ConsensusCommand::NoOp);
}

#[test]
fn propose_next_returns_none_when_no_proposal_warranted() {
    let mut replica = make_replica(0);
    // Dummy proposals disabled, no client commands.
    assert!(replica.propose_next(0).is_none());
}

#[test]
fn client_command_takes_priority_over_dummy() {
    let mut replica = make_replica(0);
    replica.enable_dummy_proposals(0);
    replica.enqueue_command(ConsensusCommand::ClientTx("CMD1".into()));
    replica.enqueue_command(ConsensusCommand::ClientTx("CMD2".into()));

    // Vec::pop takes from the back, so CMD2 is dequeued first.
    let b1 = replica.propose_next(0).unwrap();
    assert_eq!(b1.command, ConsensusCommand::ClientTx("CMD2".into()));

    // CMD1 next.
    let b2 = replica.propose_next(0).unwrap();
    assert_eq!(b2.command, ConsensusCommand::ClientTx("CMD1".into()));

    assert!(replica.client_queue.is_empty());
    // Queue is now empty.  The 3-chain still cannot commit (genesis +
    // B1 + B2 exist, but the blocks don't carry embedded QCs since
    // high_qc was None at proposal time).  So the pacemaker should
    // fall back to a dummy.
    let b3 = replica.propose_next(0).unwrap();
    assert_eq!(b3.command, ConsensusCommand::NoOp);
}

#[test]
fn pending_uncommitted_count_tracks_correctly() {
    let mut replica = make_replica(0);
    // Genesis is in tree, committed_log is empty → 1 pending.
    assert_eq!(replica.pending_uncommitted_count(), 1);

    // Manually "commit" the genesis.
    let genesis = replica.block_tree.get(&0).unwrap().clone();
    replica.committed_log.push(genesis);
    assert_eq!(replica.pending_uncommitted_count(), 0);
}

#[test]
fn enable_and_disable_dummy_proposals() {
    let mut replica = make_replica(0);
    assert!(!replica.dummy_proposal_enabled);

    replica.enable_dummy_proposals(500);
    assert!(replica.dummy_proposal_enabled);
    assert_eq!(replica.dummy_timeout_ms, 500);

    replica.disable_dummy_proposals();
    assert!(!replica.dummy_proposal_enabled);
}

// ── Simulation-level tests ───────────────────────────────────────────────

#[test]
fn simulation_dummy_proposals_complete_3chain() {
    // Scenario from the paper: a single client command is enqueued.
    // Without dummy proposals, it would sit in limbo because the
    // 3-chain can never form.  With dummies enabled, the pacemaker
    // injects NoOp blocks and the command commits.
    let mut sim = Simulation::new(4, 1, 100, false);
    sim.enable_dummy_proposals(0); // immediate dummies

    // Enqueue one client command on the first leader.
    sim.enqueue_client_command(ConsensusCommand::ClientTx("SET key value".into()));

    // Run enough rounds for the 3-chain to form and commit.
    for _ in 0..10 {
        sim.run_one_round_with_pacemaker();
    }

    // The client command should be committed on at least one replica.
    let committed = sim.replicas.iter().any(|r| {
        r.committed_log.iter().any(|b| {
            b.command == ConsensusCommand::ClientTx("SET key value".into())
        })
    });
    assert!(committed, "Client command should be committed after dummy proposals fill the 3-chain");
}

#[test]
fn simulation_dummy_proposals_do_not_starve_client() {
    // Verify that when a client command is enqueued between dummy
    // rounds, it gets proposed instead of another dummy.
    let mut sim = Simulation::new(4, 1, 100, false);
    sim.enable_dummy_proposals(0);

    // Run one round with pacemaker (should be a dummy).
    sim.run_one_round_with_pacemaker();

    // Enqueue a client command.
    sim.enqueue_client_command(ConsensusCommand::ClientTx("PRIORITY".into()));

    // Next round should pick up the client command.
    sim.run_one_round_with_pacemaker();

    // Check that the client command was proposed somewhere in the block trees.
    let proposed = sim.replicas.iter().any(|r| {
        r.block_tree.values().any(|b| {
            b.command == ConsensusCommand::ClientTx("PRIORITY".into())
        })
    });
    assert!(proposed, "Client command should be proposed ahead of dummies");
}

#[test]
fn simulation_without_dummies_client_cmd_does_not_commit() {
    // Control test: without dummy proposals, a single client command
    // followed by no further commands will NOT commit because the
    // 3-chain never fills out (the leader's propose_next returns None
    // for subsequent rounds).
    let mut sim = Simulation::new(4, 1, 20, false);
    // dummy proposals are DISABLED.

    // Enqueue one client command.
    sim.enqueue_client_command(ConsensusCommand::ClientTx("LONELY".into()));

    // Run rounds using the pacemaker loop.
    for _ in 0..10 {
        sim.run_one_round_with_pacemaker();
    }

    // The command might have been proposed but should NOT be committed
    // because there aren't enough follow-up blocks to form a 3-chain.
    let committed = sim.replicas.iter().any(|r| {
        r.committed_log.iter().any(|b| {
            b.command == ConsensusCommand::ClientTx("LONELY".into())
        })
    });
    assert!(!committed, "Without dummies, a lone client command should not commit");
}

#[test]
fn simulation_safety_holds_with_dummies() {
    // Run a longer simulation with mixed client + dummy rounds and
    // verify safety (all replicas committed the same prefix).
    let mut sim = Simulation::new(4, 1, 200, false);
    sim.enable_dummy_proposals(0);

    // Sprinkle client commands at irregular intervals.
    for round in 0..50 {
        if round % 7 == 0 {
            sim.enqueue_client_command(
                ConsensusCommand::ClientTx(format!("CMD_{}", round)),
            );
        }
        sim.run_one_round_with_pacemaker();
    }

    assert!(sim.verify_safety(), "Safety must hold with dummy proposals enabled");
}
