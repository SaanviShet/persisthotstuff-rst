//! Snapshot module for the consensus protocol.
//!
//! A **snapshot** is a point-in-time capture of the entire replica state
//! serialised to a single file.  After taking a snapshot the WAL can be
//! truncated, because every entry prior to the snapshot is already baked
//! into the snapshot file.
//!
//! Recovery = load latest snapshot + replay WAL entries written *after*
//! the snapshot.  This keeps recovery time bounded even if the system
//! has been running for days.
//!
//! # File naming
//! ```text
//! data/replica_<id>_snapshot_<seq>.bin
//! ```
//!
//! # Integrity
//! The entire snapshot is protected by a SHA-256 checksum stored in a
//! companion `.sha256` file.  On load we recompute and compare.

// ── Imports ──────────────────────────────────────────────────────────────

use std::collections::BTreeMap;            // ordered map used for block_tree
use std::fs::{self, File};                 // filesystem primitives
use std::io::{self, Read, Write};          // I/O traits
use std::path::{Path, PathBuf};            // cross-platform paths
use serde::{Serialize, Deserialize};       // (de)serialization derive macros
use sha2::{Sha256, Digest};               // SHA-256 for snapshot integrity

use crate::types::{ConsensusCommand, Hash, Block, QuorumCert};
use crate::config::ReplicaId;

// ── Error type ───────────────────────────────────────────────────────────

/// Errors specific to snapshot create / load operations.
#[derive(Debug)]
pub enum SnapshotError {
    /// A raw I/O error (file not found, permission denied, …).
    Io(io::Error),

    /// bincode could not (de)serialise the snapshot struct.
    Serialization(Box<bincode::ErrorKind>),

    /// The SHA-256 checksum on disk does not match the one we
    /// recomputed from the snapshot bytes — the file is corrupted
    /// or was tampered with.
    ChecksumMismatch {
        expected: String,   // hex string from .sha256 file
        actual: String,     // hex string we computed
    },

    /// No snapshot file was found for this replica — this is normal
    /// on first boot; the caller falls back to a fresh start.
    NotFound,
}

// ── Conversions ──────────────────────────────────────────────────────────

/// Auto-convert `io::Error` → `SnapshotError::Io` for `?`.
impl From<io::Error> for SnapshotError {
    fn from(e: io::Error) -> Self {
        SnapshotError::Io(e)
    }
}

/// Auto-convert bincode errors → `SnapshotError::Serialization` for `?`.
impl From<Box<bincode::ErrorKind>> for SnapshotError {
    fn from(e: Box<bincode::ErrorKind>) -> Self {
        SnapshotError::Serialization(e)
    }
}

/// Display implementation for user-friendly error messages.
impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::Io(e) =>
                write!(f, "Snapshot I/O error: {}", e),
            SnapshotError::Serialization(e) =>
                write!(f, "Snapshot serialization error: {}", e),
            SnapshotError::ChecksumMismatch { expected, actual } =>
                write!(f, "Snapshot checksum mismatch: expected {}, got {}", expected, actual),
            SnapshotError::NotFound =>
                write!(f, "No snapshot file found"),
        }
    }
}

// ── Serialisable block representation ────────────────────────────────────

/// A snapshot-friendly copy of a `Block`.
///
/// The original `Block` type does not derive `Serialize` / `Deserialize`
/// (it uses `ed25519_dalek::Signature` inside `QuorumCert` which is not
/// serde-compatible by default).  So we strip the QC down to its hash
/// and view — enough to reconstruct `high_qc` and the block tree
/// structure, without needing the raw signature bytes.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SerializableBlock {
    pub hash: Hash,
    pub parent: Option<Hash>,
    pub view: u64,
    pub epoch: u64,
    pub proposer: ReplicaId,
    /// If the block carried a QC, we store just the certified block_hash.
    pub qc_block_hash: Option<Hash>,
    /// … and the QC's view.
    pub qc_view: Option<u64>,
    pub command: ConsensusCommand,
}

/// A snapshot-friendly copy of a `QuorumCert`.
///
/// We store just the block_hash and view — the actual Ed25519 signatures
/// are not persisted in the snapshot because:
/// 1. They would triple the snapshot size.
/// 2. On recovery we trust our own disk, not a remote peer.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SerializableQC {
    pub block_hash: Hash,
    pub view: u64,
    pub epoch: u64,
}

// ── Snapshot struct ──────────────────────────────────────────────────────

/// The full point-in-time state of a replica, ready for serialisation.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Snapshot {
    // ── metadata ──
    /// Monotonically increasing sequence number.  Snapshot N+1 is
    /// always newer than snapshot N.
    pub snapshot_id: u64,

    /// Milliseconds since UNIX epoch when the snapshot was taken.
    pub timestamp: u128,

    /// Which replica produced this snapshot.
    pub replica_id: ReplicaId,

    // ── consensus state ──
    /// The view the replica was in at snapshot time.
    pub current_view: u64,

    /// Membership epoch.
    pub config_epoch: u64,

    /// Active validator set at snapshot time.
    pub active_validators: Vec<ReplicaId>,

    /// All committed blocks in commit order.
    pub committed_log: Vec<SerializableBlock>,

    /// Hash of the most recently committed block (or `None`).
    pub committed_up_to: Option<Hash>,

    /// The highest QC observed so far.
    pub high_qc: Option<SerializableQC>,

    /// Every block in the block tree — includes both committed and
    /// pending (proposed-but-not-yet-committed) blocks.
    pub block_tree: Vec<SerializableBlock>,

    /// The next hash counter so we don't reuse hash values after
    /// recovery.
    pub next_hash: Hash,
}

// ── Conversion helpers ───────────────────────────────────────────────────

impl SerializableBlock {
    /// Convert a domain `Block` into a snapshot-friendly representation
    /// by stripping the full QC down to just (block_hash, view).
    pub fn from_block(b: &Block) -> Self {
        SerializableBlock {
            hash: b.hash,
            parent: b.parent,
            view: b.view,
            epoch: b.epoch,
            proposer: b.proposer,
            qc_block_hash: b.qc.as_ref().map(|qc| qc.block_hash),
            qc_view: b.qc.as_ref().map(|qc| qc.view),
            command: b.command.clone(),
        }
    }

    /// Convert back to a domain `Block`.
    ///
    /// The QC is reconstructed with an **empty** signature list because
    /// we didn't persist the raw Ed25519 bytes.  This is fine: after
    /// recovery we don't re-validate our own committed data, we only
    /// validate new messages from the network.
    pub fn to_block(&self) -> Block {
        let qc = match (self.qc_block_hash, self.qc_view) {
            (Some(bh), Some(v)) => Some(QuorumCert {
                block_hash: bh,
                view: v,
                epoch: self.epoch,
                signatures: vec![],   // signatures not persisted
            }),
            _ => None,
        };
        Block {
            hash: self.hash,
            parent: self.parent,
            view: self.view,
            epoch: self.epoch,
            proposer: self.proposer,
            qc,
            command: self.command.clone(),
        }
    }
}

impl SerializableQC {
    /// Build from a real `QuorumCert`, dropping signatures.
    pub fn from_qc(qc: &QuorumCert) -> Self {
        SerializableQC {
            block_hash: qc.block_hash,
            view: qc.view,
            epoch: qc.epoch,
        }
    }

    /// Reconstruct a `QuorumCert` with an empty signature list.
    pub fn to_qc(&self) -> QuorumCert {
        QuorumCert {
            block_hash: self.block_hash,
            view: self.view,
            epoch: self.epoch,
            signatures: vec![],
        }
    }
}

// ── Snapshot I/O ─────────────────────────────────────────────────────────

impl Snapshot {
    /// Build a `Snapshot` from the live replica state.
    ///
    /// This captures **everything** needed to resume consensus without
    /// any WAL replay.
    pub fn capture(
        snapshot_id: u64,
        replica_id: ReplicaId,
        current_view: u64,
        config_epoch: u64,
        active_validators: &[ReplicaId],
        block_tree: &BTreeMap<Hash, Block>,
        committed_log: &[Block],
        committed_up_to: Option<Hash>,
        high_qc: Option<&QuorumCert>,
        next_hash: Hash,
    ) -> Self {
        Snapshot {
            snapshot_id,
            timestamp: crate::wal::WAL::now_ms(),
            replica_id,
            current_view,
            config_epoch,
            active_validators: active_validators.to_vec(),
            // Convert every block in the tree to its serialisable form.
            block_tree: block_tree.values()
                .map(SerializableBlock::from_block)
                .collect(),
            // Same for committed blocks (order matters here).
            committed_log: committed_log.iter()
                .map(SerializableBlock::from_block)
                .collect(),
            committed_up_to,
            high_qc: high_qc.map(SerializableQC::from_qc),
            next_hash,
        }
    }

    /// Write the snapshot to disk with a companion SHA-256 checksum.
    ///
    /// Two files are created:
    /// 1. `replica_<id>_snapshot_<seq>.bin`  — the bincode payload
    /// 2. `replica_<id>_snapshot_<seq>.sha256` — hex-encoded SHA-256
    ///
    /// Both are fsynced before we return, so they survive a power loss.
    pub fn save(&self, dir: &Path) -> Result<PathBuf, SnapshotError> {
        // Ensure the data directory exists.
        fs::create_dir_all(dir)?;

        // ── serialise to bytes ──
        let payload: Vec<u8> = bincode::serialize(self)?;

        // ── compute SHA-256 over the entire payload ──
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let digest = hasher.finalize();
        // Convert to a hex string like "a3f2b1c4…" for easy inspection.
        let hex_digest: String = digest.iter()
            .map(|byte| format!("{:02x}", byte))
            .collect();

        // ── write the binary snapshot file ──
        let snap_path = dir.join(format!(
            "replica_{}_snapshot_{}.bin",
            self.replica_id, self.snapshot_id
        ));
        let mut f = File::create(&snap_path)?;
        f.write_all(&payload)?;
        f.sync_all()?;   // durability: survive power loss

        // ── write the checksum sidecar file ──
        let hash_path = snap_path.with_extension("sha256");
        let mut hf = File::create(&hash_path)?;
        hf.write_all(hex_digest.as_bytes())?;
        hf.sync_all()?;

        Ok(snap_path)
    }

    /// Load the **latest** snapshot for a given replica.
    ///
    /// We scan the directory for files matching the naming pattern,
    /// pick the one with the highest `snapshot_id`, verify its SHA-256,
    /// and deserialise it.
    pub fn load_latest(replica_id: ReplicaId, dir: &Path) -> Result<Self, SnapshotError> {
        // ── find all snapshot files for this replica ──
        let prefix = format!("replica_{}_snapshot_", replica_id);
        let mut candidates: Vec<(u64, PathBuf)> = Vec::new();

        // Read directory entries; if the dir doesn't exist → NotFound.
        let entries = fs::read_dir(dir).map_err(|_| SnapshotError::NotFound)?;

        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Match pattern: replica_<id>_snapshot_<N>.bin
            if name_str.starts_with(&prefix) && name_str.ends_with(".bin") {
                // Extract <N> from the filename.
                let id_part = &name_str[prefix.len()..name_str.len() - 4];
                if let Ok(seq) = id_part.parse::<u64>() {
                    candidates.push((seq, entry.path()));
                }
            }
        }

        if candidates.is_empty() {
            return Err(SnapshotError::NotFound);
        }

        // ── pick the snapshot with the highest sequence number ──
        candidates.sort_by_key(|(seq, _)| *seq);
        let (_best_seq, best_path) = candidates.last().unwrap();

        // ── read the binary payload ──
        let payload = fs::read(best_path)?;

        // ── verify SHA-256 ──
        let hash_path = best_path.with_extension("sha256");
        if hash_path.exists() {
            let expected_hex = fs::read_to_string(&hash_path)?
                .trim()
                .to_lowercase();

            let mut hasher = Sha256::new();
            hasher.update(&payload);
            let actual_hex: String = hasher.finalize().iter()
                .map(|b| format!("{:02x}", b))
                .collect();

            if expected_hex != actual_hex {
                return Err(SnapshotError::ChecksumMismatch {
                    expected: expected_hex,
                    actual: actual_hex,
                });
            }
        }
        // If no .sha256 file exists we skip verification (backwards compat).

        // ── deserialise ──
        let snapshot: Snapshot = bincode::deserialize(&payload)?;

        Ok(snapshot)
    }

    /// Delete old snapshot files, keeping only the most recent `keep` snapshots.
    ///
    /// Prevents unbounded disk usage over long runs.
    pub fn cleanup_old(replica_id: ReplicaId, dir: &Path, keep: usize) -> Result<(), SnapshotError> {
        let prefix = format!("replica_{}_snapshot_", replica_id);
        let mut candidates: Vec<(u64, PathBuf)> = Vec::new();

        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Ok(()),  // directory doesn't exist → nothing to clean
        };

        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(&prefix) && name_str.ends_with(".bin") {
                let id_part = &name_str[prefix.len()..name_str.len() - 4];
                if let Ok(seq) = id_part.parse::<u64>() {
                    candidates.push((seq, entry.path()));
                }
            }
        }

        // Sort ascending by sequence number.
        candidates.sort_by_key(|(seq, _)| *seq);

        // Remove all but the last `keep` snapshots.
        if candidates.len() > keep {
            let to_remove = candidates.len() - keep;
            for (_, path) in candidates.iter().take(to_remove) {
                // Remove both the .bin and the .sha256 sidecar.
                let _ = fs::remove_file(path);
                let _ = fs::remove_file(path.with_extension("sha256"));
            }
        }

        Ok(())
    }

    /// Check whether any snapshot exists for a given replica.
    pub fn exists(replica_id: ReplicaId, dir: &Path) -> bool {
        let prefix = format!("replica_{}_snapshot_", replica_id);
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with(&prefix) && name_str.ends_with(".bin") {
                    return true;
                }
            }
        }
        false
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp_dir() -> TempDir {
        TempDir::new().expect("failed to create temp dir")
    }

    /// Build a small snapshot with a couple of blocks.
    fn sample_snapshot(replica_id: ReplicaId, seq: u64) -> Snapshot {
        Snapshot {
            snapshot_id: seq,
            timestamp: 1000 + seq as u128,
            replica_id,
            current_view: 5,
            config_epoch: 0,
            active_validators: vec![0, 1, 2, 3],
            committed_log: vec![
                SerializableBlock {
                    hash: 0,
                    parent: None,
                    view: 0,
                    epoch: 0,
                    proposer: 0,
                    qc_block_hash: None,
                    qc_view: None,
                    command: ConsensusCommand::NoOp,
                },
            ],
            committed_up_to: Some(0),
            high_qc: Some(SerializableQC { block_hash: 0, view: 0, epoch: 0 }),
            block_tree: vec![
                SerializableBlock {
                    hash: 0,
                    parent: None,
                    view: 0,
                    epoch: 0,
                    proposer: 0,
                    qc_block_hash: None,
                    qc_view: None,
                    command: ConsensusCommand::NoOp,
                },
                SerializableBlock {
                    hash: 1,
                    parent: Some(0),
                    view: 1,
                    epoch: 0,
                    proposer: 1,
                    qc_block_hash: Some(0),
                    qc_view: Some(0),
                    command: ConsensusCommand::NoOp,
                },
            ],
            next_hash: 2,
        }
    }

    #[test]
    fn test_save_and_load() {
        let dir = tmp_dir();
        let snap = sample_snapshot(0, 1);

        // Save.
        let path = snap.save(dir.path()).unwrap();
        assert!(path.exists());

        // Load.
        let loaded = Snapshot::load_latest(0, dir.path()).unwrap();
        assert_eq!(loaded.snapshot_id, 1);
        assert_eq!(loaded.current_view, 5);
        assert_eq!(loaded.committed_log.len(), 1);
        assert_eq!(loaded.block_tree.len(), 2);
    }

    #[test]
    fn test_load_latest_picks_highest_seq() {
        let dir = tmp_dir();

        // Save three snapshots with different sequence numbers.
        sample_snapshot(0, 1).save(dir.path()).unwrap();
        sample_snapshot(0, 3).save(dir.path()).unwrap();
        sample_snapshot(0, 2).save(dir.path()).unwrap();

        let loaded = Snapshot::load_latest(0, dir.path()).unwrap();
        // Should pick seq=3 (the highest).
        assert_eq!(loaded.snapshot_id, 3);
    }

    #[test]
    fn test_checksum_mismatch_detected() {
        let dir = tmp_dir();
        let snap = sample_snapshot(0, 1);
        let path = snap.save(dir.path()).unwrap();

        // Corrupt the .bin file.
        let mut raw = fs::read(&path).unwrap();
        if raw.len() > 10 {
            raw[10] ^= 0xFF;
        }
        fs::write(&path, &raw).unwrap();

        // Load should fail with ChecksumMismatch.
        let result = Snapshot::load_latest(0, dir.path());
        assert!(matches!(result, Err(SnapshotError::ChecksumMismatch { .. })));
    }

    #[test]
    fn test_cleanup_old() {
        let dir = tmp_dir();

        // Create 5 snapshots.
        for i in 1..=5 {
            sample_snapshot(0, i).save(dir.path()).unwrap();
        }

        // Keep only the 2 most recent.
        Snapshot::cleanup_old(0, dir.path(), 2).unwrap();

        // Snapshots 1, 2, 3 should be deleted; 4 and 5 remain.
        assert!(!dir.path().join("replica_0_snapshot_1.bin").exists());
        assert!(!dir.path().join("replica_0_snapshot_2.bin").exists());
        assert!(!dir.path().join("replica_0_snapshot_3.bin").exists());
        assert!(dir.path().join("replica_0_snapshot_4.bin").exists());
        assert!(dir.path().join("replica_0_snapshot_5.bin").exists());
    }

    #[test]
    fn test_not_found() {
        let dir = tmp_dir();
        let result = Snapshot::load_latest(99, dir.path());
        assert!(matches!(result, Err(SnapshotError::NotFound)));
    }

    #[test]
    fn test_block_round_trip() {
        let original = Block {
            hash: 42,
            parent: Some(41),
            view: 7,
            epoch: 0,
            proposer: 2,
            qc: Some(QuorumCert {
                block_hash: 41,
                view: 6,
                epoch: 0,
                signatures: vec![],
            }),
            command: ConsensusCommand::NoOp,
        };

        let serializable = SerializableBlock::from_block(&original);
        let restored = serializable.to_block();

        assert_eq!(restored.hash, original.hash);
        assert_eq!(restored.parent, original.parent);
        assert_eq!(restored.view, original.view);
        assert_eq!(restored.proposer, original.proposer);
        assert_eq!(restored.qc.as_ref().unwrap().block_hash, 41);
        assert_eq!(restored.qc.as_ref().unwrap().view, 6);
    }
}
