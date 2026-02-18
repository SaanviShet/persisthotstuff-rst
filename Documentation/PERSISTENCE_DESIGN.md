# PersistHotStuff: Persistence Layer Design
## Write-Ahead Logging, Recovery, and Snapshot Support

**Date:** February 15, 2026  
**Status:** Design & Implementation Plan

---

## Table of Contents
1. [Overview](#overview)
2. [Current System Analysis](#current-system-analysis)
3. [Design Goals](#design-goals)
4. [Component Architecture](#component-architecture)
5. [Write-Ahead Logging (WAL)](#write-ahead-logging-wal)
6. [Recovery Mechanism](#recovery-mechanism)
7. [Snapshot Support](#snapshot-support)
8. [Implementation Plan](#implementation-plan)
9. [Testing Strategy](#testing-strategy)

---

## Overview

The current PersistHotStuff implementation operates entirely **in-memory**, meaning:
- ❌ All state is lost on crash/restart
- ❌ No way to recover from failures
- ❌ Cannot persist committed blocks
- ❌ View changes and QCs are not durable

This design document outlines adding **persistence** to make the system fault-tolerant and production-ready.

---

## Current System Analysis

### **Volatile State (Lost on Crash):**

```rust
pub struct Replica {
    pub config: Config,                    // Configuration - can be reloaded
    pub current_view: u64,                 // ⚠️ CRITICAL: Current view
    pub block_tree: BTreeMap<Hash, Block>, // ⚠️ CRITICAL: All blocks
    pub high_qc: Option<QuorumCert>,       // ⚠️ CRITICAL: Highest QC
    pub vote_pool: BTreeMap<...>,          // Transient - can be cleared
    pub next_hash: Hash,                   // Can be recalculated
    pub committed_log: Vec<Block>,         // ⚠️ CRITICAL: Committed blocks
    pub committed_up_to: Option<Hash>,     // ⚠️ CRITICAL: Commit pointer
    pub timeout_ms: u64,                   // Configuration
    pub view_start_time: u128,             // Transient - reset on restart
    pub keystore: KeyStore,                // Can be reloaded
}
```

### **Critical State to Persist:**
1. **Committed Blocks** - The finalized transaction log (safety-critical)
2. **Block Tree** - Proposed but not yet committed blocks
3. **High QC** - Drives chain selection (liveness-critical)
4. **Current View** - Ensures view progression doesn't regress
5. **Vote Pool** - Can be transient (cleared on view change anyway)

---

## Design Goals

### **Correctness (Safety):**
✅ Never lose committed blocks (durability guarantee)  
✅ Maintain commit order consistency  
✅ Prevent double-voting after recovery  
✅ Ensure view monotonicity (never go backwards)

### **Performance:**
✅ Minimize I/O overhead on critical path  
✅ Asynchronous writes where possible  
✅ Batch log entries efficiently  
✅ Fast recovery (snapshot + incremental WAL)

### **Simplicity:**
✅ Clear separation: WAL module vs Replica logic  
✅ Easy to test and verify  
✅ Graceful degradation if persistence fails

---

## Component Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      Replica                            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │   Block Tree │  │  Committed   │  │   High QC    │  │
│  │              │  │     Log      │  │              │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  │
│         │                 │                 │           │
│         └─────────────────┼─────────────────┘           │
│                           │                             │
│                    ┌──────▼──────┐                      │
│                    │   WAL API   │                      │
│                    │  log_*()    │                      │
│                    └──────┬──────┘                      │
└───────────────────────────┼──────────────────────────────┘
                            │
                    ┌───────▼────────┐
                    │  Persistence   │
                    │    Manager     │
                    └───────┬────────┘
                            │
          ┌─────────────────┼─────────────────┐
          │                 │                 │
    ┌─────▼──────┐   ┌──────▼──────┐   ┌─────▼─────┐
    │  WAL File  │   │  Snapshot   │   │  Metrics  │
    │ (append)   │   │   File      │   │   File    │
    └────────────┘   └─────────────┘   └───────────┘
```

---

## Write-Ahead Logging (WAL)

### **Purpose:**
Log **every state change** before applying it to in-memory state. Ensures we can replay changes after a crash.

### **Log Entry Types:**

```rust
/// WAL log entry types
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum LogEntry {
    /// New block added to block tree
    BlockInserted {
        block: Block,
        timestamp: u128,
    },
    
    /// Vote received and added to vote pool
    VoteReceived {
        vote: Vote,
        timestamp: u128,
    },
    
    /// QC formed for a block
    QCFormed {
        qc: QuorumCert,
        timestamp: u128,
    },
    
    /// High QC updated
    HighQCUpdated {
        qc: QuorumCert,
        timestamp: u128,
    },
    
    /// Block committed (3-chain rule satisfied)
    BlockCommitted {
        block: Block,
        commit_index: usize,
        timestamp: u128,
    },
    
    /// View changed (timeout or progress)
    ViewChanged {
        old_view: u64,
        new_view: u64,
        reason: ViewChangeReason,
        timestamp: u128,
    },
    
    /// Snapshot taken (allows truncating old logs)
    SnapshotTaken {
        snapshot_id: u64,
        last_committed_block: Hash,
        timestamp: u128,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ViewChangeReason {
    Timeout,
    Commit,
}
```

### **WAL File Format:**

```
File: replica_<id>_wal.log

[Header: Magic bytes, version, replica_id]
[Entry 1: Length | Checksum | Serialized LogEntry]
[Entry 2: Length | Checksum | Serialized LogEntry]
[Entry 3: Length | Checksum | Serialized LogEntry]
...
[Entry N: Length | Checksum | Serialized LogEntry]
```

**Entry Structure:**
```
┌────────────┬────────────┬──────────────────────┐
│ Length (4B)│ CRC32 (4B) │ JSON/Bincode Payload │
└────────────┴────────────┴──────────────────────┘
```

---

## Deep Dive: Append-Only Log with Checksums

### **What is an Append-Only Log?**

An **append-only log** is a data structure where:
- New entries are always written **at the end** of the file
- Existing entries are **never modified or deleted**
- Entries are written **sequentially** in chronological order

Think of it like a **diary** where you can only add new pages, never erase old ones.

### **Why Append-Only? (vs Random Writes)**

| **Append-Only**                          | **Random Writes (Overwrite)**           |
|------------------------------------------|-----------------------------------------|
| ✅ **Sequential I/O** (~100-200 MB/s)    | ❌ **Random I/O** (~1-10 MB/s)          |
| ✅ **Simple crash recovery**             | ❌ Complex rollback needed              |
| ✅ **No data loss on partial write**     | ❌ Partial write corrupts data          |
| ✅ **Full audit trail** (all history)    | ❌ History lost on overwrite            |
| ✅ **No file fragmentation**             | ❌ Fragmentation over time              |
| ✅ **Concurrent readers safe**           | ❌ Readers need locks                   |

**Key Insight:** Hard drives and SSDs are **optimized for sequential writes**. Appending is 10-100x faster than random writes!

### **Append-Only Guarantees:**

```
Guarantee 1: Atomicity
  Either the entire entry is written, or nothing is written.
  No partial/corrupted entries (detected by checksum).

Guarantee 2: Durability  
  After fsync() returns, data survives power loss.
  The OS has committed data to physical storage.

Guarantee 3: Ordering
  Entries appear in the exact order they were written.
  Entry N is always before Entry N+1 in the file.
```

### **The Checksum Mechanism**

A **checksum** is a small fixed-size value computed from data that detects corruption.

#### **How Checksums Work:**

```
Step 1: Before Writing
┌─────────────────────────────────────┐
│  Original Data: "Block 5 committed" │
└─────────────────────────────────────┘
          │
          ▼
    [Hash Function]
    (CRC32, SHA256)
          │
          ▼
┌──────────────────┐
│ Checksum: 0x3A4F │  ← Small fingerprint of the data
└──────────────────┘

Step 2: Write to Disk
┌────────────────────────────────────────┐
│ Length: 20 | CRC32: 0x3A4F | "Block..." │
└────────────────────────────────────────┘

Step 3: After Reading (Maybe Corrupted)
┌─────────────────────────────────────┐
│  Read Data: "Block 5 c0rrupted"     │  ← Bit flip!
└─────────────────────────────────────┘
          │
          ▼
    [Hash Function]
          │
          ▼
┌──────────────────┐
│ Checksum: 0x7B2E │  ← Different from 0x3A4F!
└──────────────────┘

Step 4: Verification
if (stored_checksum == computed_checksum):
    ✅ Data is intact
else:
    ❌ CORRUPTION DETECTED!
```

#### **Types of Checksums:**

| **Algorithm** | **Size** | **Speed**      | **Collision Resistance** | **Use Case**              |
|---------------|----------|----------------|--------------------------|---------------------------|
| **CRC32**     | 4 bytes  | Very Fast      | Low (accidental errors)  | WAL entries (our choice)  |
| **CRC64**     | 8 bytes  | Fast           | Medium                   | Large files               |
| **SHA256**    | 32 bytes | Slower         | Cryptographic            | Snapshots, critical data  |
| **xxHash**    | 4/8 bytes| Extremely Fast | Medium                   | High-performance systems  |

**For WAL, we use CRC32 because:**
- ✅ Fast to compute (~500 MB/s)
- ✅ Good enough for detecting disk corruption
- ✅ Small overhead (4 bytes per entry)
- ✅ Standard library support

#### **What Corruption Can Checksums Detect?**

```
✅ Bit flips (cosmic rays, bad RAM)
   Original: 0b10101010
   Corrupted: 0b10101011  ← Single bit flipped
   
✅ Disk sector errors
   Entire sector returns garbage data
   
✅ Partial writes (power loss during write)
   Only first half of entry written to disk
   
✅ Silent data corruption
   Drive returns wrong data without error
   
❌ Intentional malicious tampering (use SHA256 for this)
```

### **Detailed File Structure**

#### **1. File Header:**

```
Offset | Size | Field          | Description
-------|------|----------------|----------------------------------
0      | 4    | Magic Number   | 0x48534C57 ("HSLW" = HotStuff Log WAL)
4      | 2    | Version        | Format version (e.g., 0x0001)
6      | 8    | Replica ID     | Which replica owns this log
14     | 8    | Created Time   | Unix timestamp (ms)
22     | 32   | Header Checksum| SHA256 of header (integrity)
```

**Why a header?**
- **Magic number:** Quickly verify this is the right file type
- **Version:** Handle format changes over time
- **Replica ID:** Prevent mixing logs from different replicas
- **Created time:** Debugging and auditing

#### **2. Log Entries:**

Each entry follows this **framed** format:

```
┌─────────────────────────────────────────────────────────────┐
│                        Entry Frame                          │
├─────────────┬────────────┬──────────────────────────────────┤
│ Length      │ Checksum   │ Payload (Serialized LogEntry)   │
│ (4 bytes)   │ (4 bytes)  │ (Length bytes)                   │
└─────────────┴────────────┴──────────────────────────────────┘
```

**Frame Design:**
1. **Length field:** Reader knows how many bytes to read next
2. **Checksum:** Validates payload integrity
3. **Payload:** Actual log entry data

**Why this framing?**
- ✅ Self-describing: Each entry contains its own size
- ✅ Variable length: Entries can be different sizes
- ✅ Corruption detection: Checksum per entry
- ✅ Fast seeking: Can skip entries by length

#### **3. Entry Payload (Serialized with Bincode):**

```rust
// Example: BlockCommitted entry
LogEntry::BlockCommitted {
    block: Block {
        hash: 42,
        parent: Some(41),
        view: 5,
        proposer: 2,
        qc: Some(QuorumCert { ... }),
    },
    commit_index: 10,
    timestamp: 1708012345678,
}

// After bincode serialization (binary format):
[Type Tag: 0x04]              ← BlockCommitted variant
[Block Hash: 42]              ← 8 bytes
[Parent: Some(41)]            ← 1 byte (Some tag) + 8 bytes
[View: 5]                     ← 8 bytes
[Proposer: 2]                 ← 8 bytes
[QC: ...]                     ← Variable bytes
[Commit Index: 10]            ← 8 bytes
[Timestamp: 1708012345678]    ← 16 bytes
```

**Total Entry Size:**
```
Frame overhead: 8 bytes (length + checksum)
Payload: ~100-500 bytes (depends on entry type)
Total: ~108-508 bytes per entry
```

### **Write Path: Ensuring Durability**

```rust
pub fn append(&mut self, entry: LogEntry) -> Result<(), WALError> {
    // STEP 1: Serialize the entry to bytes
    let payload = bincode::serialize(&entry)?;
    // Example: [0x04, 0x2A, 0x00, ...] (binary data)
    
    // STEP 2: Calculate checksum of the payload
    let checksum = crc32fast::hash(&payload);
    // Example: 0x3A4F2B1C (32-bit hash)
    
    // STEP 3: Write frame header (length + checksum)
    self.file.write_u32::<LittleEndian>(payload.len() as u32)?;
    self.file.write_u32::<LittleEndian>(checksum)?;
    
    // STEP 4: Write payload
    self.file.write_all(&payload)?;
    
    // STEP 5: CRITICAL - Force to disk (fsync)
    self.file.sync_all()?;
    // ☝️ This is what makes it durable!
    // Without fsync, data might stay in OS buffer cache
    
    Ok(())
}
```

**What happens without `fsync()`?**

```
Without fsync:
1. Write to file → Goes to OS page cache (RAM)
2. OS writes to disk... eventually (5-30 seconds)
3. Power loss before flush → DATA LOST! ❌

With fsync:
1. Write to file → Goes to OS page cache
2. fsync() → Forces immediate disk write
3. Returns only after disk confirms write
4. Power loss after fsync → Data is safe ✅
```

**Performance Impact:**
- `write()` without fsync: ~1 microsecond (in-memory)
- `fsync()`: ~1-10 milliseconds (disk latency)
- **Trade-off:** 1000x slower, but data survives crashes!

### **Read Path: Detecting Corruption**

```rust
pub fn read_all(&mut self) -> Result<Vec<LogEntry>, WALError> {
    let mut entries = Vec::new();
    
    // Skip the header
    self.file.seek(SeekFrom::Start(HEADER_SIZE))?;
    
    loop {
        // STEP 1: Read length field (4 bytes)
        let length = match self.file.read_u32::<LittleEndian>() {
            Ok(len) => len,
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
                // End of file - normal termination
                break;
            }
            Err(e) => return Err(e.into()),
        };
        
        // STEP 2: Read checksum (4 bytes)
        let stored_checksum = self.file.read_u32::<LittleEndian>()?;
        
        // STEP 3: Read payload (length bytes)
        let mut payload = vec![0u8; length as usize];
        self.file.read_exact(&mut payload)?;
        
        // STEP 4: Verify checksum
        let computed_checksum = crc32fast::hash(&payload);
        
        if computed_checksum != stored_checksum {
            // CORRUPTION DETECTED!
            return Err(WALError::CorruptedEntry {
                offset: self.file.stream_position()? - length as u64 - 8,
                expected: stored_checksum,
                actual: computed_checksum,
            });
        }
        
        // STEP 5: Deserialize
        let entry: LogEntry = bincode::deserialize(&payload)?;
        entries.push(entry);
    }
    
    Ok(entries)
}
```

**Corruption Handling Strategies:**

```rust
// Strategy 1: Fail-fast (our current approach)
if checksum_mismatch {
    return Err(WALError::CorruptedEntry);
    // Stop recovery, alert operator
}

// Strategy 2: Skip corrupted entry
if checksum_mismatch {
    log::warn!("Skipping corrupted entry at offset {}", offset);
    continue; // Try to recover rest of log
}

// Strategy 3: Truncate at corruption
if checksum_mismatch {
    log::warn!("Truncating log at corrupted entry");
    self.file.set_len(offset)?;
    break; // Discard everything after corruption
}
```

### **Real-World Example: Writing and Reading**

```rust
// === WRITING ===

let mut wal = WAL::create("replica_0_wal.log")?;

// Write entry 1
wal.append(LogEntry::BlockInserted {
    block: Block { hash: 1, ... },
    timestamp: 100,
})?;

// Disk now contains:
// [Header: 54 bytes]
// [Length: 0x00000078] [CRC: 0xABCD1234] [Payload: 120 bytes]

// Write entry 2
wal.append(LogEntry::BlockCommitted {
    block: Block { hash: 1, ... },
    commit_index: 0,
    timestamp: 200,
})?;

// Disk now contains:
// [Header: 54 bytes]
// [Entry 1: 128 bytes]
// [Length: 0x00000090] [CRC: 0x5678DCBA] [Payload: 144 bytes]

// === READING ===

let mut wal = WAL::open("replica_0_wal.log")?;
let entries = wal.read_all()?;

assert_eq!(entries.len(), 2);
assert!(matches!(entries[0], LogEntry::BlockInserted { .. }));
assert!(matches!(entries[1], LogEntry::BlockCommitted { .. }));
```

### **Crash Scenarios and Recovery**

#### **Scenario 1: Crash After Complete Write**

```
Timeline:
1. append(Entry A) → write length, checksum, payload
2. fsync() → disk commits write
3. fsync() returns successfully ✅
4. 💥 CRASH
5. Recovery: Read finds complete Entry A with valid checksum ✅

Result: Entry A is recovered successfully
```

#### **Scenario 2: Crash During Write (Partial Entry)**

```
Timeline:
1. append(Entry B) → write length, checksum
2. Writing payload... (only 50% written)
3. 💥 CRASH (before fsync)
4. Recovery: 
   - Read length: OK
   - Read checksum: OK
   - Read payload: Only 60 bytes instead of 120
   - read_exact() fails with UnexpectedEof ❌

Result: Partial entry detected, log stops at last valid entry
```

#### **Scenario 3: Crash Before fsync**

```
Timeline:
1. append(Entry C) → write length, checksum, payload
2. All data in OS buffer cache (not on disk yet)
3. 💥 CRASH (before fsync)
4. Recovery: Log doesn't contain Entry C at all

Result: Entry C is lost, but consistency maintained
```

**Key Insight:** This is why we call it "write-AHEAD" logging. We log **before** applying the change to in-memory state. If the log write fails or is lost, we never applied the change, so state is still consistent!

#### **Scenario 4: Silent Corruption (Bit Flip)**

```
Timeline:
1. Entry D written and fsynced successfully ✅
2. Years pass...
3. Cosmic ray flips a bit in the payload on disk 🌌
4. Recovery:
   - Read length: OK
   - Read checksum: 0xABCD1234 (stored)
   - Read payload: "Block 5 c0mmitted" (corrupted)
   - Compute checksum: 0xABCD9999 (different!)
   - checksum_mismatch! ❌

Result: Corruption detected, recovery fails safely
```

### **Performance Optimization: Batching**

Single writes are slow due to fsync overhead:

```rust
// SLOW: 10 entries × 5ms fsync = 50ms total
for entry in entries {
    wal.append(entry)?; // Each does an fsync
}

// FAST: 10 entries × write + 1 fsync = 5ms total
wal.append_batch(entries)?; // Single fsync at end
```

**Batch Implementation:**

```rust
pub fn append_batch(&mut self, entries: Vec<LogEntry>) -> Result<(), WALError> {
    for entry in entries {
        let payload = bincode::serialize(&entry)?;
        let checksum = crc32fast::hash(&payload);
        
        // Write to buffer, no fsync yet
        self.file.write_u32::<LittleEndian>(payload.len() as u32)?;
        self.file.write_u32::<LittleEndian>(checksum)?;
        self.file.write_all(&payload)?;
    }
    
    // Single fsync for entire batch
    self.file.sync_all()?;
    
    Ok(())
}
```

**Trade-off:**
- ✅ 10x faster for bulk writes
- ⚠️ If crash before batch fsync, lose entire batch
- 👉 Use for non-critical operations or async logging

### **Comparison to Other Systems**

| **System**        | **Log Type**     | **Checksum** | **Durability**          |
|-------------------|------------------|--------------|-------------------------|
| **PostgreSQL**    | WAL (pg_wal)     | CRC32        | fsync every commit      |
| **MySQL/InnoDB**  | Redo log         | CRC32        | fsync every commit      |
| **Kafka**         | Commit log       | CRC32        | fsync configurable      |
| **etcd/Raft**     | WAL              | CRC32        | fsync every entry       |
| **LevelDB**       | Write-ahead log  | CRC32        | fsync configurable      |
| **PersistHotStuff** | Consensus WAL  | CRC32        | fsync every entry       |

**Everyone uses append-only logs with checksums!** It's the industry-standard approach for durable, crash-safe storage.

---

### **Write Operations:**

```rust
impl WAL {
    /// Append a log entry (synchronous - ensures durability)
    pub fn append(&mut self, entry: LogEntry) -> Result<(), WALError> {
        // 1. Serialize entry
        let payload = bincode::serialize(&entry)?;
        
        // 2. Calculate checksum
        let checksum = crc32(&payload);
        
        // 3. Write: length + checksum + payload
        self.file.write_u32(payload.len() as u32)?;
        self.file.write_u32(checksum)?;
        self.file.write_all(&payload)?;
        
        // 4. CRITICAL: fsync to ensure durability
        self.file.sync_all()?;
        
        Ok(())
    }
    
    /// Append multiple entries in a batch (optimized)
    pub fn append_batch(&mut self, entries: Vec<LogEntry>) -> Result<(), WALError> {
        for entry in entries {
            // Serialize all
        }
        // Single fsync at the end
        self.file.sync_all()?;
        Ok(())
    }
}
```

### **Read Operations:**

```rust
impl WAL {
    /// Read all log entries from file
    pub fn read_all(&mut self) -> Result<Vec<LogEntry>, WALError> {
        let mut entries = Vec::new();
        
        self.file.seek(SeekFrom::Start(HEADER_SIZE))?;
        
        loop {
            // Read length
            let len = match self.file.read_u32() {
                Ok(l) => l,
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            };
            
            // Read checksum
            let expected_checksum = self.file.read_u32()?;
            
            // Read payload
            let mut payload = vec![0u8; len as usize];
            self.file.read_exact(&mut payload)?;
            
            // Verify checksum
            let actual_checksum = crc32(&payload);
            if actual_checksum != expected_checksum {
                return Err(WALError::CorruptedEntry);
            }
            
            // Deserialize
            let entry: LogEntry = bincode::deserialize(&payload)?;
            entries.push(entry);
        }
        
        Ok(entries)
    }
}
```

### **Integration with Replica:**

```rust
impl Replica {
    /// Insert block with WAL logging
    pub fn validate_and_insert_proposal(&mut self, block: Block) -> bool {
        // Existing validation logic...
        
        if valid {
            // LOG BEFORE APPLYING!
            if let Some(ref mut wal) = self.wal {
                wal.append(LogEntry::BlockInserted {
                    block: block.clone(),
                    timestamp: Self::current_time_ms(),
                }).expect("WAL write failed");
            }
            
            // Now safe to apply to in-memory state
            self.block_tree.insert(block.hash, block);
            true
        } else {
            false
        }
    }
    
    /// Commit block with WAL logging
    pub fn execute_and_commit(&mut self, block: Block) {
        // LOG BEFORE COMMITTING!
        if let Some(ref mut wal) = self.wal {
            wal.append(LogEntry::BlockCommitted {
                block: block.clone(),
                commit_index: self.committed_log.len(),
                timestamp: Self::current_time_ms(),
            }).expect("WAL write failed");
        }
        
        // Now safe to commit
        self.committed_log.push(block.clone());
        self.committed_up_to = Some(block.hash);
        self.on_commit();
    }
}
```

---

## Recovery Mechanism

### **Recovery Process:**

```
Startup:
1. Load configuration (replica ID, keys, etc.)
2. Check for existing snapshot
   - If exists: Load snapshot as base state
3. Open WAL file
4. Replay all log entries since snapshot
   - Reconstruct block_tree
   - Rebuild committed_log
   - Restore high_qc
   - Set current_view
5. Clear transient state (vote_pool)
6. Resume normal operation
```

### **Implementation:**

```rust
impl Replica {
    /// Create replica from persistent state
    pub fn recover(config: Config) -> Result<Self, RecoveryError> {
        let replica_id = config.id;
        
        // Step 1: Try to load snapshot
        let mut replica = if let Ok(snapshot) = Snapshot::load(replica_id) {
            println!("📦 Loaded snapshot at block {}", 
                     snapshot.last_committed_block);
            Self::from_snapshot(config, snapshot)
        } else {
            println!("🆕 No snapshot found, starting fresh");
            Self::new(config)
        };
        
        // Step 2: Open WAL and replay entries
        let mut wal = WAL::open(replica_id)?;
        let entries = wal.read_all()?;
        
        println!("📝 Replaying {} WAL entries...", entries.len());
        
        for (idx, entry) in entries.iter().enumerate() {
            replica.replay_entry(entry)?;
            
            if idx % 1000 == 0 {
                println!("   Replayed {}/{} entries", idx, entries.len());
            }
        }
        
        // Step 3: Clear transient state
        replica.vote_pool.clear();
        replica.view_start_time = Self::current_time_ms();
        
        // Step 4: Attach WAL for future writes
        replica.wal = Some(wal);
        
        println!("✅ Recovery complete!");
        println!("   Current view: {}", replica.current_view);
        println!("   Blocks in tree: {}", replica.block_tree.len());
        println!("   Committed blocks: {}", replica.committed_log.len());
        
        Ok(replica)
    }
    
    /// Replay a single WAL entry
    fn replay_entry(&mut self, entry: &LogEntry) -> Result<(), RecoveryError> {
        match entry {
            LogEntry::BlockInserted { block, .. } => {
                self.block_tree.insert(block.hash, block.clone());
            }
            
            LogEntry::QCFormed { qc, .. } => {
                // QC was formed, but we don't need to rebuild vote pool
                // Just update high_qc if this is higher
                if self.should_update_high_qc(qc) {
                    self.high_qc = Some(qc.clone());
                }
            }
            
            LogEntry::HighQCUpdated { qc, .. } => {
                self.high_qc = Some(qc.clone());
            }
            
            LogEntry::BlockCommitted { block, commit_index, .. } => {
                // Ensure blocks are committed in order
                if *commit_index == self.committed_log.len() {
                    self.committed_log.push(block.clone());
                    self.committed_up_to = Some(block.hash);
                } else {
                    return Err(RecoveryError::OutOfOrderCommit);
                }
            }
            
            LogEntry::ViewChanged { new_view, .. } => {
                self.current_view = *new_view;
                // Vote pool already cleared
            }
            
            LogEntry::SnapshotTaken { .. } => {
                // No action needed during replay
                // Snapshot was already loaded
            }
            
            _ => {
                // Ignore votes and other transient entries
            }
        }
        
        Ok(())
    }
}
```

### **Crash Recovery Scenarios:**

**Scenario 1: Crash after block insert, before commit**
```
Before crash:
- WAL: [BlockInserted(B5)]
- Memory: block_tree has B5, not committed

After recovery:
- Replay: Insert B5 into block_tree
- B5 is present but not committed (correct!)
- If 3-chain forms later, B5 can be committed
```

**Scenario 2: Crash during commit**
```
Before crash:
- WAL: [BlockInserted(B5), BlockCommitted(B5)]
- Memory: Maybe committed, maybe not

After recovery:
- Replay: Insert B5, then commit B5
- B5 is correctly in committed_log
- Idempotent operation (safe)
```

**Scenario 3: Crash during view change**
```
Before crash:
- WAL: [ViewChanged(old=3, new=4)]
- Memory: current_view might be 3 or 4

After recovery:
- Replay: Set current_view = 4
- Vote pool cleared (fresh start)
- Correct view resumed
```

---

## Snapshot Support

### **Purpose:**
- Avoid unbounded WAL growth
- Speed up recovery (don't replay millions of entries)
- Compress committed state efficiently

### **Snapshot Contents:**

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct Snapshot {
    /// Snapshot metadata
    pub snapshot_id: u64,
    pub timestamp: u128,
    pub replica_id: ReplicaId,
    
    /// Consensus state
    pub current_view: u64,
    pub committed_log: Vec<Block>,
    pub committed_up_to: Option<Hash>,
    pub high_qc: Option<QuorumCert>,
    
    /// Block tree (only uncommitted blocks)
    pub block_tree: BTreeMap<Hash, Block>,
    
    /// Snapshot hash for integrity
    pub checksum: [u8; 32],
}
```

### **Snapshot Triggers:**

```rust
impl Replica {
    /// Check if snapshot should be taken
    pub fn should_snapshot(&self) -> bool {
        // Option 1: After every N commits
        self.committed_log.len() % 100 == 0
        
        // Option 2: After N WAL entries
        // self.wal.as_ref().unwrap().entry_count() > 10000
        
        // Option 3: Time-based (every 5 minutes)
        // current_time - last_snapshot_time > 300_000
    }
    
    /// Take a snapshot
    pub fn take_snapshot(&mut self) -> Result<(), SnapshotError> {
        let snapshot = Snapshot {
            snapshot_id: self.snapshot_counter,
            timestamp: Self::current_time_ms(),
            replica_id: self.config.id,
            current_view: self.current_view,
            committed_log: self.committed_log.clone(),
            committed_up_to: self.committed_up_to,
            high_qc: self.high_qc.clone(),
            block_tree: self.block_tree.clone(),
            checksum: [0; 32], // Calculated below
        };
        
        // Calculate checksum
        let serialized = bincode::serialize(&snapshot)?;
        let checksum = sha256(&serialized);
        
        // Write to file
        let filename = format!("replica_{}_snapshot_{}.bin", 
                              self.config.id, snapshot.snapshot_id);
        let mut file = File::create(filename)?;
        file.write_all(&serialized)?;
        file.sync_all()?;
        
        // Log snapshot in WAL
        if let Some(ref mut wal) = self.wal {
            wal.append(LogEntry::SnapshotTaken {
                snapshot_id: snapshot.snapshot_id,
                last_committed_block: self.committed_up_to.unwrap_or(0),
                timestamp: snapshot.timestamp,
            })?;
            
            // Truncate old WAL entries (keep only recent)
            wal.truncate_before_snapshot()?;
        }
        
        self.snapshot_counter += 1;
        
        Ok(())
    }
}
```

### **WAL Truncation:**

After snapshot, old entries can be discarded:

```
Before snapshot:
WAL: [Entry1, Entry2, ..., Entry10000, SnapshotTaken]

After truncation:
WAL: [SnapshotTaken]
(Entries 1-10000 are in the snapshot file)
```

---

## Implementation Plan

### **Phase 1: Basic WAL** (Week 1)
- [ ] Create `src/wal.rs` module
- [ ] Define `LogEntry` enum
- [ ] Implement file I/O with checksums
- [ ] Add WAL to Replica struct
- [ ] Log critical operations (commit, view change)
- [ ] Write unit tests for WAL

### **Phase 2: Recovery** (Week 2)
- [ ] Implement `Replica::recover()`
- [ ] Add `replay_entry()` logic
- [ ] Test crash/recovery scenarios
- [ ] Handle corrupted logs gracefully
- [ ] Verify safety properties after recovery

### **Phase 3: Snapshots** (Week 3)
- [ ] Create `src/snapshot.rs` module
- [ ] Define `Snapshot` struct
- [ ] Implement snapshot creation
- [ ] Implement snapshot loading
- [ ] Add WAL truncation
- [ ] Test snapshot-based recovery

### **Phase 4: Optimizations** (Week 4)
- [ ] Async WAL writes (where safe)
- [ ] Batch logging
- [ ] Compression for snapshots
- [ ] Metrics (log size, recovery time)
- [ ] Cleanup old snapshots

### **Phase 5: Integration** (Week 5)
- [ ] Update simulation to use persistence
- [ ] Add example: crash and recover
- [ ] Add example: checkpoint and restore
- [ ] Performance benchmarks
- [ ] Documentation

---

## Testing Strategy

### **Unit Tests:**

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_wal_write_read() {
        // Write entries to WAL
        // Read them back
        // Verify correctness
    }
    
    #[test]
    fn test_wal_checksum_detection() {
        // Write entry
        // Corrupt the file
        // Verify error on read
    }
    
    #[test]
    fn test_recovery_after_commit() {
        // Commit blocks
        // Close replica
        // Recover
        // Verify committed_log is same
    }
    
    #[test]
    fn test_snapshot_restore() {
        // Run consensus for 100 blocks
        // Take snapshot
        // Create new replica from snapshot
        // Verify state matches
    }
}
```

### **Integration Tests:**

```rust
#[test]
fn test_byzantine_crash_recovery() {
    // Scenario: Replica crashes during Byzantine attack
    // 1. Start 4 replicas
    // 2. Run normal rounds
    // 3. Crash replica 2
    // 4. Recover replica 2 from WAL
    // 5. Verify it catches up and consensus continues
}

#[test]
fn test_partition_recovery() {
    // Scenario: Network partition, then heal
    // 1. Partition network
    // 2. Take snapshots on both sides
    // 3. Heal partition
    // 4. Replicas recover and sync
    // 5. Verify safety (same committed log)
}
```

---

## File Structure

```
src/
  wal.rs          # NEW: Write-ahead logging
  snapshot.rs     # NEW: Snapshot management
  recovery.rs     # NEW: Recovery logic
  replica.rs      # MODIFIED: Add persistence calls
  
examples/
  crash_recovery.rs      # NEW: Demonstrate recovery
  snapshot_restore.rs    # NEW: Demonstrate snapshots
  
tests/
  wal_tests.rs          # NEW: WAL unit tests
  recovery_tests.rs     # NEW: Recovery tests
  persistence_integration.rs  # NEW: End-to-end tests
  
data/                   # NEW: Persistent storage
  replica_0_wal.log
  replica_0_snapshot_1.bin
  replica_0_snapshot_2.bin
  replica_1_wal.log
  ...
```

---

## Dependencies Needed

```toml
[dependencies]
# Serialization
serde = { version = "1.0", features = ["derive"] }
bincode = "1.3"       # Binary serialization (compact)

# Checksums
crc32fast = "1.3"     # Fast CRC32 for WAL entries

# Hashing
sha2 = "0.10"         # Already have this

# File I/O
tempfile = "3.0"      # For test temp files

# Compression (optional)
flate2 = "1.0"        # Gzip compression for snapshots
```

---

## Performance Considerations

### **WAL Write Latency:**
- Each log entry requires `fsync()` → ~1-10ms disk latency
- **Optimization:** Batch multiple entries before sync
- **Trade-off:** Batching risks losing last few entries on crash

### **Recovery Time:**
```
Without snapshots:
- 10,000 log entries × 0.1ms replay = 1 second recovery

With snapshots (every 1000 commits):
- Load snapshot: ~50ms
- Replay recent 100 entries × 0.1ms = 10ms
- Total: ~60ms recovery (16x faster!)
```

### **Storage Growth:**
```
WAL size:
- ~200 bytes per entry
- 10,000 entries = 2MB
- With snapshots: Truncate to ~100 entries = 20KB

Snapshot size:
- Depends on committed_log size
- 1000 blocks × 100 bytes = 100KB per snapshot
- Keep last 3 snapshots = 300KB
```

---

## Summary

This persistence layer adds **production-grade fault tolerance** to PersistHotStuff:

✅ **Safety:** Committed blocks never lost  
✅ **Liveness:** Replicas recover and continue  
✅ **Performance:** Snapshots keep recovery fast  
✅ **Simplicity:** Clean WAL abstraction  

**Next Steps:** Start with Phase 1 (Basic WAL) implementation!
