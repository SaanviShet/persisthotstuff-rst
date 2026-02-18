//! Write-Ahead Logging (WAL) module for the consensus protocol.
//!
//! This module implements an **append-only** log with CRC32 checksums.
//! Every state change is logged to disk **before** it is applied to
//! in-memory state, so that after a crash we can replay the log and
//! reconstruct the exact pre-crash state.
//!
//! # File Format
//! ```text
//! [Header: magic(4) | version(2) | replica_id(8) | created_ts(8)]
//! [Entry : length(4) | crc32(4) | payload(length bytes)]
//! [Entry : length(4) | crc32(4) | payload(length bytes)]
//! ...
//! ```

// ── Imports ──────────────────────────────────────────────────────────────

use std::fs::{File, OpenOptions};          // File handle and builder for open/create
use std::io::{self, Read, Write, Seek, SeekFrom, BufWriter, BufReader};
                                            // Standard I/O traits + buffered wrappers
use std::path::{Path, PathBuf};            // Cross-platform path handling
use serde::{Serialize, Deserialize};       // Derive-able (de)serialization traits
use crate::types::{Hash, Block, QuorumCert, Vote};
                                            // Re-use existing domain types
use crate::config::ReplicaId;              // Type alias for u64 replica IDs

// ── Constants ────────────────────────────────────────────────────────────

/// Magic bytes written at byte-0 of every WAL file.
/// Spelling out "HSLW" (HotStuff Log WAL) in hex so we can detect
/// whether a file is actually a WAL or some unrelated data.
const WAL_MAGIC: [u8; 4] = [0x48, 0x53, 0x4C, 0x57]; // ASCII "HSLW"

/// Format version — lets us change the on-disk layout in the future
/// without breaking old files.  Old code seeing version > 1 can refuse
/// to open the file gracefully instead of silently misreading it.
const WAL_VERSION: u16 = 1;

/// Total size of the file header in bytes.
/// 4 (magic) + 2 (version) + 8 (replica_id) + 16 (created_ts as u128) = 30 bytes.
const HEADER_SIZE: u64 = 30;

// ── Error Type ───────────────────────────────────────────────────────────

/// All errors that can originate from WAL operations.
///
/// We wrap I/O, serialisation, and integrity errors into a single enum
/// so callers can match on the cause without dealing with raw `io::Error`
/// everywhere.
#[derive(Debug)]
pub enum WALError {
    /// An I/O operation (open / read / write / fsync) failed.
    Io(io::Error),

    /// bincode could not serialise or deserialise a `LogEntry`.
    Serialization(Box<bincode::ErrorKind>),

    /// The CRC32 stored on disk did not match the one we recomputed
    /// from the payload bytes — the data is corrupted.
    CorruptedEntry {
        offset: u64,           // byte position where the bad entry starts
        expected: u32,         // checksum that was stored on disk
        actual: u32,           // checksum we computed from the read bytes
    },

    /// The first 4 bytes of the file are not `HSLW`, so this is not
    /// a WAL file (or it belongs to a different application).
    InvalidMagic,

    /// The file was written by a newer version of the code and we
    /// do not know how to parse it.
    UnsupportedVersion(u16),
}

// ── Conversions into WALError ────────────────────────────────────────────

/// Let the `?` operator convert a raw `io::Error` into `WALError::Io`
/// automatically, so we don't need explicit `.map_err(...)` on every I/O
/// call.
impl From<io::Error> for WALError {
    fn from(e: io::Error) -> Self {
        WALError::Io(e)
    }
}

/// Same convenience for bincode errors.
impl From<Box<bincode::ErrorKind>> for WALError {
    fn from(e: Box<bincode::ErrorKind>) -> Self {
        WALError::Serialization(e)
    }
}

/// Implement Display so WALError can be printed with `{}` in error messages.
impl std::fmt::Display for WALError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WALError::Io(e) => write!(f, "WAL I/O error: {}", e),
            WALError::Serialization(e) => write!(f, "WAL serialization error: {}", e),
            WALError::CorruptedEntry { offset, expected, actual } =>
                write!(f, "Corrupted WAL entry at offset {}: expected CRC 0x{:08X}, got 0x{:08X}",
                       offset, expected, actual),
            WALError::InvalidMagic => write!(f, "Not a valid WAL file (bad magic bytes)"),
            WALError::UnsupportedVersion(v) => write!(f, "WAL version {} is not supported", v),
        }
    }
}

// ── Log Entry ────────────────────────────────────────────────────────────

/// Reason a view change happened — distinguishes leader timeout from
/// normal commit-driven view advancement.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ViewChangeReason {
    Timeout,
    Commit,
}

/// One atomic event that must be durably recorded.
///
/// Every variant carries a `timestamp` (milliseconds since UNIX epoch)
/// so that during replay or debugging we know *when* it happened.
///
/// `Serialize` / `Deserialize` are derived so bincode can convert these
/// to/from raw bytes automatically.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum LogEntry {
    /// A new block was added to the in-memory block tree.
    BlockInserted {
        hash: Hash,                    // block's unique ID
        parent: Option<Hash>,         // hash of parent block (None for genesis)
        view: u64,                     // view in which it was proposed
        proposer: ReplicaId,           // who proposed it
        qc_block_hash: Option<Hash>,   // QC piggybacked on the block (if any)
        qc_view: Option<u64>,         // view of that QC
        timestamp: u128,
    },

    /// A vote was received and added to the vote pool.
    VoteReceived {
        block_hash: Hash,
        view: u64,
        signer: ReplicaId,
        timestamp: u128,
    },

    /// Enough votes were collected to form a quorum certificate.
    QCFormed {
        block_hash: Hash,
        view: u64,
        signer_count: usize,          // how many unique signers
        timestamp: u128,
    },

    /// The high_qc pointer was updated to a newer / higher QC.
    HighQCUpdated {
        block_hash: Hash,
        view: u64,
        timestamp: u128,
    },

    /// A block was committed (the 3-chain rule was satisfied).
    BlockCommitted {
        hash: Hash,
        parent: Option<Hash>,
        view: u64,
        proposer: ReplicaId,
        commit_index: usize,          // position in committed_log
        timestamp: u128,
    },

    /// The replica moved to a new view.
    ViewChanged {
        old_view: u64,
        new_view: u64,
        reason: ViewChangeReason,
        timestamp: u128,
    },

    /// A snapshot was taken — WAL entries before this point can be
    /// discarded because they are captured in the snapshot file.
    SnapshotTaken {
        snapshot_id: u64,
        last_committed_hash: Option<Hash>,
        timestamp: u128,
    },
}

// ── WAL struct ───────────────────────────────────────────────────────────

/// The main WAL handle.
///
/// Holds an open file descriptor positioned at the end (for appending)
/// and tracks how many entries have been written so far (for snapshot
/// trigger logic).
pub struct WAL {
    /// The open file handle — kept for the lifetime of the WAL so we
    /// can append without re-opening.
    file: File,

    /// Full path to the WAL file on disk — kept so we can re-open or
    /// delete it during truncation.
    path: PathBuf,

    /// Which replica this WAL belongs to — used for sanity checks and
    /// error messages.
    replica_id: ReplicaId,

    /// Running count of entries written since the file was opened.
    /// Used by `should_truncate()` / snapshot logic.
    entry_count: u64,
}

impl WAL {
    // ── Create a brand-new WAL file ──────────────────────────────────

    /// Create a **new** WAL file for the given replica.
    ///
    /// Writes the fixed-size header (magic + version + replica_id +
    /// timestamp) and fsyncs it so that even if we crash immediately
    /// the header is on disk.
    ///
    /// # Errors
    /// Returns `WALError::Io` if file creation or writing fails.
    pub fn create(replica_id: ReplicaId, dir: &Path) -> Result<Self, WALError> {
        // ── build file path ──
        // Each replica gets its own WAL file named `replica_<id>_wal.log`.
        std::fs::create_dir_all(dir)?;               // ensure data/ directory exists
        let path = dir.join(format!("replica_{}_wal.log", replica_id));

        // ── open the file ──
        // create_new(true) fails if the file already exists, preventing
        // accidental overwrite of an existing WAL.
        let mut file = OpenOptions::new()
            .write(true)        // we will write to it
            .read(true)         // we also need to read during recovery
            .create_new(true)   // fail if file exists (safety)
            .open(&path)?;

        // ── write the header ──
        file.write_all(&WAL_MAGIC)?;                            // 4 bytes: magic
        file.write_all(&WAL_VERSION.to_le_bytes())?;            // 2 bytes: version (little-endian)
        file.write_all(&replica_id.to_le_bytes())?;             // 8 bytes: replica ID
        let created_ts = Self::now_ms();
        file.write_all(&created_ts.to_le_bytes())?;             // 8 bytes: creation timestamp

        // ── force to disk ──
        // sync_all() = fsync: flushes OS page cache AND file metadata.
        // Without this, a crash right after `create()` could leave us
        // with a zero-length file.
        file.sync_all()?;

        Ok(WAL {
            file,
            path,
            replica_id,
            entry_count: 0,
        })
    }

    // ── Open an existing WAL file ────────────────────────────────────

    /// Open an **existing** WAL file and validate its header.
    ///
    /// After this call the file cursor is at the end of the file, ready
    /// for appending new entries.
    pub fn open(replica_id: ReplicaId, dir: &Path) -> Result<Self, WALError> {
        let path = dir.join(format!("replica_{}_wal.log", replica_id));

        let mut file = OpenOptions::new()
            .read(true)         // need to read header + entries
            .write(true)        // will append new entries
            .open(&path)?;

        // ── validate magic bytes ──
        let mut magic = [0u8; 4];
        file.read_exact(&mut magic)?;              // read exactly 4 bytes
        if magic != WAL_MAGIC {
            return Err(WALError::InvalidMagic);    // not our file format
        }

        // ── validate version ──
        let mut ver_bytes = [0u8; 2];
        file.read_exact(&mut ver_bytes)?;
        let version = u16::from_le_bytes(ver_bytes);
        if version != WAL_VERSION {
            return Err(WALError::UnsupportedVersion(version));
        }

        // ── read replica_id from header (skip; we trust the filename) ──
        let mut rid_bytes = [0u8; 8];
        file.read_exact(&mut rid_bytes)?;
        // We could assert: u64::from_le_bytes(rid_bytes) == replica_id

        // ── skip created_ts (u128 = 16 bytes) ──
        let mut ts_bytes = [0u8; 16];
        file.read_exact(&mut ts_bytes)?;

        // ── count existing entries (needed for entry_count) ──
        let entries = Self::read_entries_from_file(&mut file)?;
        let entry_count = entries.len() as u64;

        // ── seek to end so next append goes at EOF ──
        file.seek(SeekFrom::End(0))?;

        Ok(WAL {
            file,
            path,
            replica_id,
            entry_count,
        })
    }

    // ── Append a single entry ────────────────────────────────────────

    /// Durably append one `LogEntry` to the WAL.
    ///
    /// Layout of each entry on disk:
    /// ```text
    /// [payload_length: u32 LE] [crc32: u32 LE] [payload: N bytes]
    /// ```
    ///
    /// After writing we call `sync_all()` (fsync) so the data survives
    /// a power failure.  This is the **critical durability guarantee**.
    pub fn append(&mut self, entry: &LogEntry) -> Result<(), WALError> {
        // Step 1: serialize the LogEntry into a byte vector using bincode.
        // bincode is a compact binary format — much smaller than JSON.
        let payload: Vec<u8> = bincode::serialize(entry)?;

        // Step 2: compute CRC32 checksum over the payload bytes.
        // If a cosmic ray flips a bit on disk, the checksum will not
        // match when we read the entry back during recovery.
        let checksum: u32 = crc32fast::hash(&payload);

        // Step 3: write the 4-byte length prefix (little-endian).
        // The reader will use this to know how many bytes to read for
        // the payload.
        let len = payload.len() as u32;
        self.file.write_all(&len.to_le_bytes())?;

        // Step 4: write the 4-byte CRC32 checksum.
        self.file.write_all(&checksum.to_le_bytes())?;

        // Step 5: write the actual payload bytes.
        self.file.write_all(&payload)?;

        // Step 6: fsync — force all buffered data AND metadata to the
        // physical storage device.  After this returns, the entry is
        // durable even if the power goes out one nanosecond later.
        self.file.sync_all()?;

        // Step 7: bump our running counter.
        self.entry_count += 1;

        Ok(())
    }

    // ── Append a batch of entries with a single fsync ────────────────

    /// Append multiple entries with **one** fsync at the end.
    ///
    /// This is 10× faster than calling `append()` in a loop because
    /// fsync (~1-10 ms) dominates the cost.  The trade-off is that if
    /// we crash before the final fsync, we lose the **entire batch**
    /// instead of just the last entry.
    pub fn append_batch(&mut self, entries: &[LogEntry]) -> Result<(), WALError> {
        for entry in entries {
            let payload = bincode::serialize(entry)?;
            let checksum = crc32fast::hash(&payload);
            let len = payload.len() as u32;

            self.file.write_all(&len.to_le_bytes())?;     // length prefix
            self.file.write_all(&checksum.to_le_bytes())?; // CRC32
            self.file.write_all(&payload)?;                // payload

            self.entry_count += 1;
        }

        // One fsync covers the entire batch.
        self.file.sync_all()?;

        Ok(())
    }

    // ── Read all entries ─────────────────────────────────────────────

    /// Read every entry from the WAL file, verifying each checksum.
    ///
    /// Used during **recovery**: we replay all entries to reconstruct
    /// the in-memory state that existed before the crash.
    ///
    /// The file cursor is **not** repositioned afterwards — the caller
    /// should seek if needed.
    pub fn read_all(&mut self) -> Result<Vec<LogEntry>, WALError> {
        // Position the cursor right after the header so we start
        // reading the first entry.
        self.file.seek(SeekFrom::Start(HEADER_SIZE))?;
        Self::read_entries_from_file(&mut self.file)
    }

    /// Internal helper that reads entries starting from the current
    /// cursor position.  Extracted so both `open()` and `read_all()`
    /// can reuse the same logic.
    fn read_entries_from_file(file: &mut File) -> Result<Vec<LogEntry>, WALError> {
        let mut entries = Vec::new();

        loop {
            // ── read 4-byte length prefix ──
            let mut len_bytes = [0u8; 4];
            match file.read_exact(&mut len_bytes) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    // We reached the end of the file — that is the
                    // normal termination condition, not an error.
                    break;
                }
                Err(e) => return Err(WALError::Io(e)),
            }
            let payload_len = u32::from_le_bytes(len_bytes) as usize;

            // ── read 4-byte CRC32 checksum ──
            let mut crc_bytes = [0u8; 4];
            file.read_exact(&mut crc_bytes)?;
            let stored_checksum = u32::from_le_bytes(crc_bytes);

            // ── read `payload_len` bytes of payload ──
            let mut payload = vec![0u8; payload_len];
            file.read_exact(&mut payload)?;

            // ── verify integrity ──
            let computed_checksum = crc32fast::hash(&payload);
            if computed_checksum != stored_checksum {
                // Record where the bad entry starts so the operator
                // can inspect the file with a hex editor.
                let current_pos = file.seek(SeekFrom::Current(0))
                    .unwrap_or(0);
                return Err(WALError::CorruptedEntry {
                    offset: current_pos - payload_len as u64 - 8,
                    expected: stored_checksum,
                    actual: computed_checksum,
                });
            }

            // ── deserialize the payload into a LogEntry ──
            let entry: LogEntry = bincode::deserialize(&payload)?;
            entries.push(entry);
        }

        Ok(entries)
    }

    // ── Truncation ───────────────────────────────────────────────────

    /// Truncate the WAL after a snapshot has been taken.
    ///
    /// We create a fresh empty WAL file (header only) and replace the
    /// old one.  This reclaims disk space because all the state up to
    /// the snapshot is captured in the snapshot file.
    pub fn truncate_after_snapshot(&mut self) -> Result<(), WALError> {
        // Step 1: create a temporary new WAL file next to the old one.
        let tmp_path = self.path.with_extension("tmp");

        {
            let mut tmp = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)?;

            // Write a fresh header.
            tmp.write_all(&WAL_MAGIC)?;
            tmp.write_all(&WAL_VERSION.to_le_bytes())?;
            tmp.write_all(&self.replica_id.to_le_bytes())?;
            let ts = Self::now_ms();
            tmp.write_all(&ts.to_le_bytes())?;
            tmp.sync_all()?;
        }
        // The tmp file is closed (and fsynced) when we leave this block.

        // Step 2: atomically rename tmp → original path.
        // On POSIX (Linux/macOS) `rename` is atomic — the directory
        // entry is updated in one step, so we never end up with a
        // half-written file.
        std::fs::rename(&tmp_path, &self.path)?;

        // Step 3: re-open the new (empty) file so future appends go there.
        self.file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)?;
        self.file.seek(SeekFrom::End(0))?;
        self.entry_count = 0;

        Ok(())
    }

    // ── Utilities ────────────────────────────────────────────────────

    /// How many entries have been written since the file was (re)opened.
    pub fn entry_count(&self) -> u64 {
        self.entry_count
    }

    /// Full filesystem path of this WAL file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Current time in milliseconds since the UNIX epoch.
    /// Wrapped in a helper so tests could override it if needed.
    pub fn now_ms() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    }

    /// Check whether a WAL file exists for the given replica.
    pub fn exists(replica_id: ReplicaId, dir: &Path) -> bool {
        dir.join(format!("replica_{}_wal.log", replica_id)).exists()
    }
}

// ── Helper: convert a Block into a BlockInserted LogEntry ────────────────

impl LogEntry {
    /// Convenience constructor — turns a domain `Block` into the
    /// matching `LogEntry::BlockInserted` variant.
    pub fn from_block(block: &Block) -> Self {
        LogEntry::BlockInserted {
            hash: block.hash,
            parent: block.parent,
            view: block.view,
            proposer: block.proposer,
            qc_block_hash: block.qc.as_ref().map(|qc| qc.block_hash),
            qc_view: block.qc.as_ref().map(|qc| qc.view),
            timestamp: WAL::now_ms(),
        }
    }

    /// Convenience constructor — turns a domain `Block` + commit index
    /// into a `LogEntry::BlockCommitted`.
    pub fn from_committed_block(block: &Block, commit_index: usize) -> Self {
        LogEntry::BlockCommitted {
            hash: block.hash,
            parent: block.parent,
            view: block.view,
            proposer: block.proposer,
            commit_index,
            timestamp: WAL::now_ms(),
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: create a throwaway directory that is deleted when `_dir`
    /// goes out of scope.
    fn tmp_dir() -> TempDir {
        TempDir::new().expect("failed to create temp dir")
    }

    #[test]
    fn test_create_and_open() {
        let dir = tmp_dir();

        // Create a new WAL for replica 0.
        let wal = WAL::create(0, dir.path());
        assert!(wal.is_ok(), "WAL::create should succeed");

        // Re-open the same WAL.
        let wal2 = WAL::open(0, dir.path());
        assert!(wal2.is_ok(), "WAL::open should succeed on existing file");
    }

    #[test]
    fn test_append_and_read_back() {
        let dir = tmp_dir();
        let mut wal = WAL::create(0, dir.path()).unwrap();

        // Write two entries.
        let e1 = LogEntry::ViewChanged {
            old_view: 0,
            new_view: 1,
            reason: ViewChangeReason::Timeout,
            timestamp: 100,
        };
        let e2 = LogEntry::BlockCommitted {
            hash: 42,
            parent: Some(0),
            view: 1,
            proposer: 0,
            commit_index: 0,
            timestamp: 200,
        };
        wal.append(&e1).unwrap();
        wal.append(&e2).unwrap();

        // Read them back.
        let entries = wal.read_all().unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_corruption_detected() {
        let dir = tmp_dir();
        let mut wal = WAL::create(0, dir.path()).unwrap();

        wal.append(&LogEntry::ViewChanged {
            old_view: 0,
            new_view: 1,
            reason: ViewChangeReason::Timeout,
            timestamp: 100,
        }).unwrap();

        // Manually corrupt a byte in the payload region.
        // The header is 30 bytes (magic=4, version=2, replica_id=8, timestamp=16).
        // The first entry starts at offset 30.
        // Entry frame: length(4) + crc(4) = 8 bytes, so payload starts at 38.
        drop(wal);  // close the file

        let path = dir.path().join("replica_0_wal.log");
        let mut raw = std::fs::read(&path).unwrap();
        // Flip a bit somewhere in the payload (offset 38+).
        if raw.len() > 40 {
            raw[40] ^= 0xFF;
        }
        std::fs::write(&path, &raw).unwrap();

        // Try to open — should detect corruption either during open or read_all.
        match WAL::open(0, dir.path()) {
            Err(WALError::CorruptedEntry { .. }) => {
                // Success! Corruption detected during open.
            }
            Ok(mut wal2) => {
                // open succeeded, corruption should be caught by read_all.
                let result = wal2.read_all();
                assert!(matches!(result, Err(WALError::CorruptedEntry { .. })),
                        "Should detect corrupted entry");
            }
            Err(e) => {
                panic!("Unexpected error type: {:?}", e);
            }
        }
    }

    #[test]
    fn test_batch_append() {
        let dir = tmp_dir();
        let mut wal = WAL::create(0, dir.path()).unwrap();

        let entries: Vec<LogEntry> = (0..10)
            .map(|i| LogEntry::ViewChanged {
                old_view: i,
                new_view: i + 1,
                reason: ViewChangeReason::Timeout,
                timestamp: i as u128 * 100,
            })
            .collect();

        wal.append_batch(&entries).unwrap();

        let read_back = wal.read_all().unwrap();
        assert_eq!(read_back.len(), 10);
    }

    #[test]
    fn test_truncate_after_snapshot() {
        let dir = tmp_dir();
        let mut wal = WAL::create(0, dir.path()).unwrap();

        // Write 5 entries.
        for i in 0..5 {
            wal.append(&LogEntry::ViewChanged {
                old_view: i,
                new_view: i + 1,
                reason: ViewChangeReason::Timeout,
                timestamp: i as u128 * 100,
            }).unwrap();
        }
        assert_eq!(wal.entry_count(), 5);

        // Truncate (simulates post-snapshot cleanup).
        wal.truncate_after_snapshot().unwrap();
        assert_eq!(wal.entry_count(), 0);

        // Read back — should be empty.
        let entries = wal.read_all().unwrap();
        assert_eq!(entries.len(), 0);

        // Can still append after truncation.
        wal.append(&LogEntry::ViewChanged {
            old_view: 5,
            new_view: 6,
            reason: ViewChangeReason::Commit,
            timestamp: 600,
        }).unwrap();
        let entries = wal.read_all().unwrap();
        assert_eq!(entries.len(), 1);
    }
}
