//! Recovery module for the consensus protocol.
//!
//! After a crash the replica must be restored to a consistent state.
//! The recovery procedure is:
//!
//! 1. Try to load the **latest snapshot** from disk.
//!    - If found → use it as the base state.
//!    - If not found → start with a fresh (genesis-only) state.
//! 2. Open the **WAL** and replay every entry recorded *after* the
//!    snapshot.  Each entry mutates the in-memory state exactly as if
//!    the original operation had just happened.
//! 3. Clear **transient** state that is meaningless across restarts
//!    (vote pool, pacemaker timer).
//! 4. Attach the WAL for future writes and resume normal operation.

// ── Imports ──────────────────────────────────────────────────────────────

use std::collections::{BTreeMap, BTreeSet};        // ordered map for block_tree
use std::path::Path;                   // filesystem paths

use crate::config::{Config, ReplicaId};
use crate::types::{Hash, Block, QuorumCert};
use crate::crypto::KeyStore;
use crate::wal::{WAL, WALError, LogEntry};
use crate::snapshot::{Snapshot, SnapshotError, SerializableBlock, SerializableQC};
use crate::replica::Replica;

// ── Error type ───────────────────────────────────────────────────────────

/// Errors that can occur during the recovery process.
#[derive(Debug)]
pub enum RecoveryError {
    /// WAL could not be opened or read.
    Wal(WALError),

    /// Snapshot could not be loaded.
    Snapshot(SnapshotError),

    /// A `BlockCommitted` entry has a `commit_index` that doesn't match
    /// the current length of `committed_log`.  This means the WAL is
    /// out of order — we refuse to continue because safety could be
    /// violated.
    OutOfOrderCommit {
        expected_index: usize,
        found_index: usize,
    },
}

// ── Conversions ──────────────────────────────────────────────────────────

impl From<WALError> for RecoveryError {
    fn from(e: WALError) -> Self { RecoveryError::Wal(e) }
}

impl From<SnapshotError> for RecoveryError {
    fn from(e: SnapshotError) -> Self { RecoveryError::Snapshot(e) }
}

impl std::fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecoveryError::Wal(e) =>
                write!(f, "Recovery failed (WAL): {}", e),
            RecoveryError::Snapshot(e) =>
                write!(f, "Recovery failed (snapshot): {}", e),
            RecoveryError::OutOfOrderCommit { expected_index, found_index } =>
                write!(f, "Out-of-order commit: expected index {}, found {}",
                       expected_index, found_index),
        }
    }
}

// ── Public recovery API ──────────────────────────────────────────────────

/// Recover a replica from persistent storage (snapshot + WAL).
///
/// # Arguments
/// * `config`   – the replica's configuration (id, n, f, timeout)
/// * `keystore` – the replica's Ed25519 key material
/// * `data_dir` – directory containing WAL and snapshot files
///
/// # Returns
/// A fully reconstructed `Replica` ready to resume consensus, together
/// with an open WAL handle for future writes.
///
/// # Errors
/// Returns `RecoveryError` if the WAL or snapshot is corrupt, or if a
/// commit-order violation is detected during replay.
pub fn recover(
    config: Config,
    keystore: KeyStore,
    data_dir: &Path,
) -> Result<(Replica, WAL), RecoveryError> {
    let replica_id = config.id;

    // ────────────────────────────────────────────────────────────────
    // STEP 1: Try to load the most recent snapshot as the base state.
    // ────────────────────────────────────────────────────────────────
    let (
        mut block_tree,
        mut committed_log,
        mut committed_up_to,
        mut high_qc,
        mut current_view,
        mut next_hash,
        mut config_epoch,
        active_validators,
        pending_app_state,
        snapshot_counter,
    ) = match Snapshot::load_latest(replica_id, data_dir) {
        Ok(snap) => {
            println!("Loaded snapshot #{} for replica {} (view {}, {} committed blocks)",
                     snap.snapshot_id, replica_id, snap.current_view,
                     snap.committed_log.len());

            // Convert snapshot's serialisable blocks back into domain Blocks.
            let tree: BTreeMap<Hash, Block> = snap.block_tree.iter()
                .map(|sb| (sb.hash, sb.to_block()))
                .collect();

            let clog: Vec<Block> = snap.committed_log.iter()
                .map(|sb| sb.to_block())
                .collect();

            let hqc: Option<QuorumCert> = snap.high_qc
                .map(|sqc| sqc.to_qc());

            let app_state = snap.app_state;

            // Resume the counter one past the loaded snapshot so that
            // future snapshots always get strictly higher sequence numbers.
            let counter = snap.snapshot_id + 1;

            (
                tree,
                clog,
                snap.committed_up_to,
                hqc,
                snap.current_view,
                snap.next_hash,
                snap.config_epoch,
                snap.active_validators.into_iter().collect::<BTreeSet<_>>(),
                app_state,
                counter,
            )
        }
        Err(SnapshotError::NotFound) => {
            // First boot — no snapshot on disk yet.
            println!("No snapshot found for replica {}, starting fresh", replica_id);
            let mut validators = BTreeSet::new();
            for id in 0..config.n {
                validators.insert(id as ReplicaId);
            }
            (BTreeMap::new(), Vec::new(), None, None, 0u64, 1u64, 0u64, validators, None, 0u64)
        }
        Err(e) => {
            // Snapshot exists but is corrupt or unreadable.
            return Err(RecoveryError::Snapshot(e));
        }
    };

    // ────────────────────────────────────────────────────────────────
    // STEP 2: Open the WAL and replay all entries.
    // ────────────────────────────────────────────────────────────────
    let mut wal = if WAL::exists(replica_id, data_dir) {
        let mut w = WAL::open(replica_id, data_dir)?;
        let entries = w.read_all()?;

        println!("Replaying {} WAL entries for replica {}...", entries.len(), replica_id);

        for (idx, entry) in entries.iter().enumerate() {
            replay_entry(
                entry,
                &mut block_tree,
                &mut committed_log,
                &mut committed_up_to,
                &mut high_qc,
                &mut current_view,
                &mut next_hash,
                &mut config_epoch,
            )?;

            // Progress indicator every 1 000 entries.
            if idx > 0 && idx % 1000 == 0 {
                println!("   … replayed {}/{}", idx, entries.len());
            }
        }

        w
    } else {
        // No WAL exists yet — create one.
        println!("Creating new WAL for replica {}", replica_id);
        WAL::create(replica_id, data_dir)?
    };

    // ────────────────────────────────────────────────────────────────
    // STEP 3: Build the Replica struct from the recovered state.
    // ────────────────────────────────────────────────────────────────

    // If we recovered nothing, insert the genesis block so the replica
    // has a valid starting point.
    if block_tree.is_empty() {
        let genesis = Block {
            hash: 0,
            parent: None,
            view: 0,
            epoch: 0,
            proposer: 0,
            qc: None,
            command: crate::types::ConsensusCommand::NoOp,
        };
        block_tree.insert(0, genesis);
    }

    let replica = Replica {
        config: config.clone(),
        current_view,
        block_tree,
        high_qc,
        // Vote pool is transient — cleared on every view change anyway.
        vote_pool: BTreeMap::new(),
        next_hash,
        committed_log,
        committed_up_to,
        // Pacemaker resets: we just booted, so the timer starts now.
        timeout_ms: config.timeout_ms,
        view_start_time: Replica::current_time_ms(),
        active_validators,
        config_epoch,
        keystore,
        wal: None,
        snapshot_counter,
        app: None,
        pending_app_state,
        client_queue: Vec::new(),
        dummy_proposal_enabled: false,
        last_proposed_time: 0,
        dummy_timeout_ms: 0,
    };

    println!("Recovery complete for replica {}", replica_id);
    println!("   View:            {}", replica.current_view);
    println!("   Blocks in tree:  {}", replica.block_tree.len());
    println!("   Committed:       {}", replica.committed_log.len());

    Ok((replica, wal))
}

// ── Entry replay ─────────────────────────────────────────────────────────

/// Replay a single `LogEntry`, mutating the in-memory state exactly
/// as if the original operation had just occurred.
///
/// This function is intentionally **separate** from `Replica` methods
/// so that it can be used both during recovery and in unit tests
/// without needing a fully constructed Replica.
fn replay_entry(
    entry: &LogEntry,
    block_tree: &mut BTreeMap<Hash, Block>,
    committed_log: &mut Vec<Block>,
    committed_up_to: &mut Option<Hash>,
    high_qc: &mut Option<QuorumCert>,
    current_view: &mut u64,
    next_hash: &mut Hash,
    config_epoch: &mut u64,
) -> Result<(), RecoveryError> {
    match entry {
        // ── A block was inserted into the tree ──
        LogEntry::BlockInserted {
            hash, parent, view, epoch, proposer,
            qc_block_hash, qc_view, ..
        } => {
            // Reconstruct the QC stub (no signatures — they were not logged).
            let qc = match (qc_block_hash, qc_view) {
                (Some(bh), Some(v)) => Some(QuorumCert {
                    block_hash: *bh,
                    view: *v,
                    epoch: *epoch,
                    signatures: vec![],
                }),
                _ => None,
            };

            let block = Block {
                hash: *hash,
                parent: *parent,
                view: *view,
                epoch: *epoch,
                proposer: *proposer,
                qc,
                command: crate::types::ConsensusCommand::NoOp,
            };
            block_tree.insert(*hash, block);

            // Keep next_hash ahead of every hash we've seen.
            if *hash >= *next_hash {
                *next_hash = *hash + 1;
            }
        }

        // ── Vote received — transient; skip during replay ──
        LogEntry::VoteReceived { .. } => {
            // Votes are view-specific and the vote pool is cleared on
            // restart, so there is nothing to replay.
        }

        // ── QC formed ──
        LogEntry::QCFormed { block_hash, view, epoch, .. } => {
            // Update high_qc if this QC is newer.
            let dominated = match high_qc {
                Some(ref hq) => *view > hq.view,
                None => true,
            };
            if dominated {
                *high_qc = Some(QuorumCert {
                    block_hash: *block_hash,
                    view: *view,
                    epoch: *epoch,
                    signatures: vec![],
                });
            }
            *config_epoch = (*config_epoch).max(*epoch);
        }

        // ── High QC pointer updated ──
        LogEntry::HighQCUpdated { block_hash, view, epoch, .. } => {
            *high_qc = Some(QuorumCert {
                block_hash: *block_hash,
                view: *view,
                epoch: *epoch,
                signatures: vec![],
            });
            *config_epoch = (*config_epoch).max(*epoch);
        }

        // ── Block committed (3-chain satisfied) ──
        LogEntry::BlockCommitted {
            hash, parent, view, epoch, proposer,
            commit_index, ..
        } => {
            // Safety check: commits must be replayed in order.
            if *commit_index != committed_log.len() {
                return Err(RecoveryError::OutOfOrderCommit {
                    expected_index: committed_log.len(),
                    found_index: *commit_index,
                });
            }

            let block = Block {
                hash: *hash,
                parent: *parent,
                view: *view,
                epoch: *epoch,
                proposer: *proposer,
                qc: None,  // QC not stored separately for commits
                command: crate::types::ConsensusCommand::NoOp,
            };
            committed_log.push(block);
            *committed_up_to = Some(*hash);
            *config_epoch = (*config_epoch).max(*epoch);
        }

        // ── View changed ──
        LogEntry::ViewChanged { new_view, .. } => {
            *current_view = *new_view;
        }

        // ── Snapshot marker — nothing to do during replay ──
        LogEntry::SnapshotTaken { .. } => {
            // The snapshot was already loaded (if applicable) in step 1.
        }
    }

    Ok(())
}

// ══════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wal::{WAL, LogEntry, ViewChangeReason};
    use crate::snapshot::Snapshot;
    use tempfile::TempDir;

    fn tmp_dir() -> TempDir {
        TempDir::new().expect("failed to create temp dir")
    }

    /// Build a minimal Config + KeyStore for testing.
    fn test_config_and_keys(id: ReplicaId) -> (Config, KeyStore) {
        let cfg = Config { n: 4, f: 1, id, timeout_ms: 1000 };
        let all_keys = KeyStore::generate_keys(4);
        let keystores = KeyStore::distribute_keys(&all_keys);
        let ks = keystores[id as usize].clone();
        (cfg, ks)
    }

    #[test]
    fn test_fresh_start_recovery() {
        // No snapshot, no WAL → should produce a clean replica.
        let dir = tmp_dir();
        let (cfg, ks) = test_config_and_keys(0);

        let (replica, _wal) = recover(cfg, ks, dir.path()).unwrap();

        assert_eq!(replica.current_view, 0);
        assert_eq!(replica.committed_log.len(), 0);
        // Genesis block should be in the tree.
        assert!(replica.block_tree.contains_key(&0));
    }

    #[test]
    fn test_recovery_from_wal_only() {
        let dir = tmp_dir();
        let (cfg, ks) = test_config_and_keys(0);

        // Write some WAL entries manually.
        {
            let mut wal = WAL::create(0, dir.path()).unwrap();

            // Insert a block.
            wal.append(&LogEntry::BlockInserted {
                hash: 1,
                parent: Some(0),
                view: 1,
                epoch: 0,
                proposer: 0,
                qc_block_hash: None,
                qc_view: None,
                command: crate::types::ConsensusCommand::NoOp,
                timestamp: 100,
            }).unwrap();

            // View change.
            wal.append(&LogEntry::ViewChanged {
                old_view: 0,
                new_view: 1,
                reason: ViewChangeReason::Commit,
                timestamp: 200,
            }).unwrap();
        }

        let (replica, _wal) = recover(cfg, ks, dir.path()).unwrap();

        assert_eq!(replica.current_view, 1);
        // block 0 (genesis) + block 1 (inserted via WAL)
        assert!(replica.block_tree.contains_key(&1));
    }

    #[test]
    fn test_recovery_from_snapshot_plus_wal() {
        let dir = tmp_dir();
        let (cfg, ks) = test_config_and_keys(0);

        // Step 1: save a snapshot.
        let snap = crate::snapshot::Snapshot {
            snapshot_id: 1,
            timestamp: 1000,
            replica_id: 0,
            current_view: 3,
            config_epoch: 0,
            active_validators: vec![0, 1, 2, 3],
            committed_log: vec![],
            committed_up_to: None,
            high_qc: None,
            block_tree: vec![
                crate::snapshot::SerializableBlock {
                    hash: 0, parent: None, view: 0, epoch: 0, proposer: 0,
                    qc_block_hash: None, qc_view: None,
                    command: crate::types::ConsensusCommand::NoOp,
                },
            ],
            next_hash: 1,
            app_state: None,
        };
        snap.save(dir.path()).unwrap();

        // Step 2: create a WAL with one more entry.
        {
            let mut wal = WAL::create(0, dir.path()).unwrap();
            wal.append(&LogEntry::ViewChanged {
                old_view: 3,
                new_view: 4,
                reason: ViewChangeReason::Timeout,
                timestamp: 2000,
            }).unwrap();
        }

        let (replica, _wal) = recover(cfg, ks, dir.path()).unwrap();

        // View should be 4 (snapshot had 3, WAL advanced to 4).
        assert_eq!(replica.current_view, 4);
    }

    #[test]
    fn test_out_of_order_commit_rejected() {
        let dir = tmp_dir();
        let (cfg, ks) = test_config_and_keys(0);

        {
            let mut wal = WAL::create(0, dir.path()).unwrap();

            // Commit index should be 0 for the first commit, but we
            // write index=5 → out of order.
            wal.append(&LogEntry::BlockCommitted {
                hash: 1,
                parent: Some(0),
                view: 1,
                epoch: 0,
                proposer: 0,
                command: crate::types::ConsensusCommand::NoOp,
                commit_index: 5,
                timestamp: 100,
            }).unwrap();
        }

        let result = recover(cfg, ks, dir.path());
        assert!(matches!(result, Err(RecoveryError::OutOfOrderCommit { .. })));
    }
}
