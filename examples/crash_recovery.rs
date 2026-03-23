//! Crash Recovery Demo
//!
//! Demonstrates the full persistence lifecycle:
//!   1. Create replicas with WAL logging enabled
//!   2. Run consensus rounds (blocks are logged to disk)
//!   3. Take a snapshot
//!   4. Simulate a crash (drop everything in memory)
//!   5. Recover from snapshot + WAL
//!   6. Verify the recovered state matches the pre-crash state
//!
//! Run with:  cargo run --example crash_recovery

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use persisthotstuff_rst::config::Config;
use persisthotstuff_rst::replica::Replica;
use persisthotstuff_rst::types::*;
use persisthotstuff_rst::crypto::KeyStore;
use persisthotstuff_rst::wal::{WAL, LogEntry, ViewChangeReason};
use persisthotstuff_rst::snapshot::Snapshot;
use persisthotstuff_rst::recovery::recover;

/// The directory where WAL and snapshot files are stored.
const DATA_DIR: &str = "data";

fn main() {
    println!("\n╔═══════════════════════════════════════════════════════════╗");
    println!("║     PersistHotStuff — Crash Recovery Demo               ║");
    println!("╚═══════════════════════════════════════════════════════════╝\n");

    let data_dir = Path::new(DATA_DIR);

    // Clean up any leftover data from previous runs.
    if data_dir.exists() {
        std::fs::remove_dir_all(data_dir).unwrap();
        println!("Cleaned up old data/ directory");
    }

    // ─────────────────────────────────────────────────────────────
    // PHASE 1: Normal operation — create a replica, run consensus,
    //          and let the WAL record everything.
    // ─────────────────────────────────────────────────────────────
    println!("\n━━━ PHASE 1: Normal Operation ━━━\n");

    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    let config = Config { n: 4, f: 1, id: 0, timeout_ms: 5000 };

    // Create the replica.
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

    // Attach a WAL so every mutation is logged to disk.
    let wal = WAL::create(config.id, data_dir)
        .expect("Failed to create WAL");
    replica.attach_wal(wal);
    println!("WAL created at data/replica_0_wal.log");

    // Insert genesis block.
    let genesis = Block {
        hash: 0,
        parent: None,
        view: 0,
        epoch: 0,
        proposer: 0,
        qc: None,
        command: ConsensusCommand::NoOp,
    };
    replica.block_tree.insert(0, genesis);
    // Log genesis manually (it bypasses validate_and_insert_proposal).
    if let Some(ref mut wal) = replica.wal {
        wal.append(&LogEntry::BlockInserted {
            hash: 0,
            parent: None,
            view: 0,
            epoch: 0,
            proposer: 0,
            qc_block_hash: None,
            qc_view: None,
            command: ConsensusCommand::NoOp,
            timestamp: WAL::now_ms(),
        }).unwrap();
    }
    println!("Genesis block inserted (hash=0)");

    // Build a chain: B1 → B2 → B3 → B4 (each with a QC on its parent).
    let blocks_data = vec![
        (1, Some(0), 1, 1, Some((0, 0))),  // B1: parent=0, QC on B0
        (2, Some(1), 2, 2, Some((1, 1))),  // B2: parent=1, QC on B1
        (3, Some(2), 3, 3, Some((2, 2))),  // B3: parent=2, QC on B2
        (4, Some(3), 4, 0, Some((3, 3))),  // B4: parent=3, QC on B3
    ];

    for (hash, parent, view, proposer, qc_info) in &blocks_data {
        let qc = qc_info.map(|(bh, v)| dummy_qc(bh, v));
        let block = Block {
            hash: *hash,
            parent: *parent,
            view: *view,
            epoch: 0,
            proposer: *proposer,
            qc,
            command: ConsensusCommand::NoOp,
        };

        // Use validate_and_insert_proposal — this auto-logs to WAL.
        // Note: we skip the leader check by inserting directly for demo.
        replica.block_tree.insert(*hash, block.clone());
        if let Some(ref mut wal) = replica.wal {
            wal.append(&LogEntry::from_block(&block)).unwrap();
        }
        println!("Block {} inserted (view={}, proposer=R{})", hash, view, proposer);
    }

    // Update high_qc to the QC on B3 (carried by B4).
    replica.high_qc = Some(dummy_qc(3, 3));
    if let Some(ref mut wal) = replica.wal {
        wal.append(&LogEntry::HighQCUpdated {
            block_hash: 3,
            view: 3,
            epoch: 0,
            timestamp: WAL::now_ms(),
        }).unwrap();
    }
    println!("high_qc updated to QC(block=3, view=3)");

    // Commit using the 3-chain rule.
    // 3-chain: B0 ← B1[QC] ← B2[QC] commits B0
    // 3-chain: B1 ← B2[QC] ← B3[QC] commits B1
    // 3-chain: B2 ← B3[QC] ← B4[QC] commits B2
    replica.commit_all();
    println!("Committed {} block(s) via 3-chain rule", replica.committed_log.len());

    // Advance to view 5 via a timeout.
    replica.start_view(5);
    if let Some(ref mut wal) = replica.wal {
        wal.append(&LogEntry::ViewChanged {
            old_view: 0,
            new_view: 5,
            reason: ViewChangeReason::Timeout,
            timestamp: WAL::now_ms(),
        }).unwrap();
    }
    println!("View advanced to {}", replica.current_view);

    // Record the state before the "crash".
    let pre_crash_view = replica.current_view;
    let pre_crash_tree_size = replica.block_tree.len();
    let pre_crash_committed = replica.committed_log.len();
    let pre_crash_high_qc = replica.high_qc.as_ref().map(|qc| (qc.block_hash, qc.view));
    let pre_crash_committed_hashes: Vec<Hash> =
        replica.committed_log.iter().map(|b| b.hash).collect();

    println!("\nPre-crash state:");
    println!("   View:            {}", pre_crash_view);
    println!("   Blocks in tree:  {}", pre_crash_tree_size);
    println!("   Committed:       {}", pre_crash_committed);
    println!("   High QC:         {:?}", pre_crash_high_qc);
    println!("   Committed hashes: {:?}", pre_crash_committed_hashes);

    // ─────────────────────────────────────────────────────────────
    // PHASE 2: Take a snapshot (optional but speeds up recovery).
    // ─────────────────────────────────────────────────────────────
    println!("\n━━━ PHASE 2: Snapshot ━━━\n");

    let snap = Snapshot::capture(
        0,                             // snapshot_id
        config.id,
        replica.current_view,
        replica.config_epoch,
        &replica.active_validators.iter().copied().collect::<Vec<_>>(),
        &replica.block_tree,
        &replica.committed_log,
        replica.committed_up_to,
        replica.high_qc.as_ref(),
        replica.next_hash,
        None,                          // app_state
    );
    let snap_path = snap.save(data_dir).expect("Failed to save snapshot");
    println!("Snapshot saved to {:?}", snap_path);

    // Truncate the WAL (all entries are now in the snapshot).
    if let Some(ref mut wal) = replica.wal {
        wal.truncate_after_snapshot().expect("Failed to truncate WAL");
        println!("WAL truncated (entries now in snapshot)");
    }

    // Simulate one MORE operation AFTER the snapshot, so recovery
    // must replay this from the WAL on top of the snapshot.
    let post_snap_block = Block {
        hash: 5,
        parent: Some(4),
        view: 5,
        epoch: 0,
        proposer: 1,
        qc: Some(dummy_qc(4, 4)),
        command: ConsensusCommand::NoOp,
    };
    replica.block_tree.insert(5, post_snap_block.clone());
    if let Some(ref mut wal) = replica.wal {
        wal.append(&LogEntry::from_block(&post_snap_block)).unwrap();
    }
    println!("Block 5 inserted AFTER snapshot (this must survive recovery via WAL)");

    let pre_crash_tree_size = replica.block_tree.len();  // now 6

    // ─────────────────────────────────────────────────────────────
    // PHASE 3: CRASH -- drop everything in memory.
    // ─────────────────────────────────────────────────────────────
    println!("\n━━━ PHASE 3: CRASH ━━━\n");
    drop(replica);   // All in-memory state is gone!
    println!("Replica dropped -- all memory lost!");
    println!("   Only files on disk remain:");
    for entry in std::fs::read_dir(data_dir).unwrap() {
        let entry = entry.unwrap();
        let size = entry.metadata().unwrap().len();
        println!("   {} ({} bytes)", entry.file_name().to_string_lossy(), size);
    }

    // ─────────────────────────────────────────────────────────────
    // PHASE 4: Recovery — rebuild from snapshot + WAL.
    // ─────────────────────────────────────────────────────────────
    println!("\n━━━ PHASE 4: Recovery ━━━\n");

    let (mut recovered, wal) = recover(config.clone(), keystores[0].clone(), data_dir)
        .expect("Recovery failed!");
    recovered.attach_wal(wal);

    println!("\nPost-recovery state:");
    println!("   View:            {}", recovered.current_view);
    println!("   Blocks in tree:  {}", recovered.block_tree.len());
    println!("   Committed:       {}", recovered.committed_log.len());
    let recovered_hqc = recovered.high_qc.as_ref().map(|qc| (qc.block_hash, qc.view));
    println!("   High QC:         {:?}", recovered_hqc);
    let recovered_hashes: Vec<Hash> =
        recovered.committed_log.iter().map(|b| b.hash).collect();
    println!("   Committed hashes: {:?}", recovered_hashes);

    // ─────────────────────────────────────────────────────────────
    // PHASE 5: Verify correctness.
    // ─────────────────────────────────────────────────────────────
    println!("\n━━━ PHASE 5: Verification ━━━\n");

    let mut all_ok = true;

    // Check view.
    if recovered.current_view == pre_crash_view {
        println!("   [OK] View matches: {}", pre_crash_view);
    } else {
        println!("   [FAIL] View mismatch: expected {}, got {}",
                 pre_crash_view, recovered.current_view);
        all_ok = false;
    }

    // Check block tree size (should be 6: genesis + B1-B4 + B5).
    if recovered.block_tree.len() == pre_crash_tree_size {
        println!("   [OK] Block tree size matches: {}", pre_crash_tree_size);
    } else {
        println!("   [FAIL] Block tree size mismatch: expected {}, got {}",
                 pre_crash_tree_size, recovered.block_tree.len());
        all_ok = false;
    }

    // Check committed log.
    if recovered_hashes == pre_crash_committed_hashes {
        println!("   [OK] Committed log matches: {:?}", pre_crash_committed_hashes);
    } else {
        println!("   [FAIL] Committed log mismatch: expected {:?}, got {:?}",
                 pre_crash_committed_hashes, recovered_hashes);
        all_ok = false;
    }

    // Check high_qc.
    if recovered_hqc == pre_crash_high_qc {
        println!("   [OK] High QC matches: {:?}", pre_crash_high_qc);
    } else {
        println!("   [FAIL] High QC mismatch: expected {:?}, got {:?}",
                 pre_crash_high_qc, recovered_hqc);
        all_ok = false;
    }

    // Check that block 5 (inserted AFTER snapshot) survived.
    if recovered.block_tree.contains_key(&5) {
        println!("   [OK] Post-snapshot block 5 recovered from WAL");
    } else {
        println!("   [FAIL] Post-snapshot block 5 is MISSING!");
        all_ok = false;
    }

    // Check the replica can continue operating after recovery.
    println!("\n━━━ Continuing after recovery… ━━━\n");
    let block6 = Block {
        hash: 6,
        parent: Some(5),
        view: 6,
        epoch: 0,
        proposer: 2,
        qc: Some(dummy_qc(5, 5)),
        command: ConsensusCommand::NoOp,
    };
    recovered.block_tree.insert(6, block6.clone());
    if let Some(ref mut wal) = recovered.wal {
        wal.append(&LogEntry::from_block(&block6)).unwrap();
    }
    println!("   Block 6 appended after recovery -- WAL still works!");

    // Final verdict.
    println!();
    if all_ok {
        println!("╔═══════════════════════════════════════════════════════════╗");
        println!("║   ALL CHECKS PASSED -- Recovery is correct!          ║");
        println!("╚═══════════════════════════════════════════════════════════╝");
    } else {
        println!("╔═══════════════════════════════════════════════════════════╗");
        println!("║   SOME CHECKS FAILED -- see above for details        ║");
        println!("╚═══════════════════════════════════════════════════════════╝");
    }

    // Clean up demo data.
    std::fs::remove_dir_all(data_dir).unwrap();
    println!("\nCleaned up data/ directory\n");
}
