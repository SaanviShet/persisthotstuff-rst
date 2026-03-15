# Lesson 12: Serialization with Serde & Bincode

## Where this appears in the project
- `Cargo.toml` — `serde = "1.0"`, `bincode = "1.3"` dependencies
- `src/wal.rs` — `LogEntry` serialization for WAL entries
- `src/snapshot.rs` — `Snapshot` serialization for state snapshots
- `#[derive(Serialize, Deserialize)]` on WAL and snapshot types

---

## 12.1 — What is Serialization?

Serialization converts an in-memory Rust struct into bytes (or text) that can be:
- Written to a file (persistence)
- Sent over the network
- Stored in a database

Deserialization is the reverse: bytes → struct.

```
Rust struct  ──serialize──→  [bytes on disk]  ──deserialize──→  Rust struct
```

---

## 12.2 — Serde: The Framework

`serde` is Rust's universal serialization framework. It provides two traits:
- `Serialize` — "this type can be converted TO bytes/text"
- `Deserialize` — "this type can be created FROM bytes/text"

Serde itself doesn't define a format. Separate crates provide formats:

| Crate      | Format        | Use case                   |
|------------|---------------|----------------------------|
| `bincode`  | Binary        | Compact, fast, not human-readable |
| `serde_json` | JSON       | Human-readable, web APIs    |
| `toml`     | TOML          | Configuration files         |
| `serde_yaml` | YAML       | Configuration files         |

This project uses **bincode** for both the WAL and snapshots because:
1. It's very compact (no field names stored)
2. It's very fast (no parsing overhead)
3. We don't need human-readability for internal logs

---

## 12.3 — Deriving Serialize and Deserialize

### From `src/wal.rs`:
```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ViewChangeReason {
    Timeout,
    Commit,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum LogEntry {
    BlockInserted {
        hash: Hash,                    // u64
        parent: Option<Hash>,         // Option<u64>
        view: u64,
        proposer: ReplicaId,          // u64
        qc_block_hash: Option<Hash>,
        qc_view: Option<u64>,
        timestamp: u128,
    },
    VoteReceived {
        block_hash: Hash,
        view: u64,
        signer: ReplicaId,
        timestamp: u128,
    },
    // ... more variants ...
}
```

The `#[derive(Serialize, Deserialize)]` macro generates all the conversion code at
compile time. Every field type must ALSO implement Serialize/Deserialize:

- `u64`, `u128`, `usize` — serde handles these natively
- `Option<T>` — handled if T is serializable
- `Vec<T>` — handled if T is serializable
- `String` — handled natively
- Custom types — must also derive Serialize/Deserialize

---

## 12.4 — Using Bincode: Serialize

### From `src/wal.rs` — converting a LogEntry to bytes:
```rust
pub fn append(&mut self, entry: &LogEntry) -> Result<(), WALError> {
    let payload: Vec<u8> = bincode::serialize(entry)?;
    // payload is now a compact byte vector
    // e.g., a BlockInserted might be ~50 bytes
}
```

`bincode::serialize(entry)` returns `Result<Vec<u8>, Box<bincode::ErrorKind>>`.
The `?` propagates errors (using our `From` impl to convert to `WALError`).

### What the bytes look like (conceptual):
```
LogEntry::BlockInserted {
    hash: 42,
    parent: Some(0),
    view: 3,
    proposer: 1,
    ...
}
→ [0x00, 0x00, 0x00, 0x00,    // variant tag (0 = BlockInserted)
   0x2A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // hash = 42
   0x01,                                              // Option tag (1 = Some)
   0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // parent = 0
   0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // view = 3
   ...]
```

Bincode is **not self-describing** — it doesn't store field names, only values in
order. This makes it very compact but means both sides must agree on the exact
struct layout.

---

## 12.5 — Using Bincode: Deserialize

### From `src/wal.rs` — converting bytes back to a LogEntry:
```rust
fn read_entries_from_file(file: &mut File) -> Result<Vec<LogEntry>, WALError> {
    // ... read len_bytes, crc_bytes, payload from file ...

    let entry: LogEntry = bincode::deserialize(&payload)?;
    entries.push(entry);
}
```

`bincode::deserialize(&payload)` returns `Result<LogEntry, Box<bincode::ErrorKind>>`.
The type annotation `: LogEntry` tells bincode what type to reconstruct.

---

## 12.6 — Snapshot Serialization

### From `src/snapshot.rs` — the full round-trip:

The snapshot needs to serialize types that DON'T derive Serialize (like `Block` which
has a `QuorumCert` with `Signature`). The solution: create serializable wrapper types.

```rust
// Wrapper that mirrors Block but is serde-compatible
#[derive(Serialize, Deserialize)]
pub struct SerializableBlock {
    pub hash: Hash,
    pub parent: Option<Hash>,
    pub view: u64,
    pub proposer: ReplicaId,
    pub qc: Option<SerializableQC>,
}

// Wrapper for QuorumCert
#[derive(Serialize, Deserialize)]
pub struct SerializableQC {
    pub block_hash: u64,
    pub view: u64,
    pub signatures: Vec<SerializableSignature>,
}

// The main Snapshot struct
#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub sequence: u64,
    pub replica_id: ReplicaId,
    pub current_view: u64,
    pub block_tree: Vec<SerializableBlock>,
    pub committed_log: Vec<SerializableBlock>,
    pub committed_up_to: Option<Hash>,
    pub high_qc: Option<SerializableQC>,
    pub next_hash: Hash,
}
```

### Save to disk:
```rust
pub fn save(&self, data_dir: &Path) -> Result<(), SnapshotError> {
    let bytes = bincode::serialize(self)?;              // Snapshot → bytes

    // Write snapshot file
    let mut file = File::create(&snap_path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;

    // Write SHA-256 checksum sidecar
    let hash = Sha256::digest(&bytes);
    let hex = format!("{:x}", hash);
    std::fs::write(&sha_path, hex.as_bytes())?;

    Ok(())
}
```

### Load from disk:
```rust
pub fn load(path: &Path) -> Result<Self, SnapshotError> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;

    // Verify SHA-256 checksum
    // ... (read .sha256 sidecar and compare) ...

    let snapshot: Snapshot = bincode::deserialize(&bytes)?;  // bytes → Snapshot
    Ok(snapshot)
}
```

---

## 12.7 — CRC32 vs SHA-256: Two Integrity Approaches

The project uses BOTH:

### CRC32 (in the WAL):
```rust
let checksum: u32 = crc32fast::hash(&payload);
```
- **Purpose**: detect accidental corruption (bit flips, truncation)
- **Speed**: very fast (~GB/s)
- **Size**: 4 bytes
- **Security**: not secure (easy to forge collisions)

### SHA-256 (in snapshots):
```rust
let hash = sha2::Sha256::digest(&bytes);
```
- **Purpose**: detect corruption AND tampering
- **Speed**: slower (~500 MB/s)
- **Size**: 32 bytes
- **Security**: cryptographically secure

Why the difference? WAL entries are small and frequent (need speed). Snapshots
are large and infrequent (can afford SHA-256).

---

## 12.8 — The `format!` and `write!` Macros

Used throughout for string formatting:

```rust
// format! — returns a String
let filename = format!("replica_{}_wal.log", replica_id);
let hex = format!("{:x}", hash);       // lowercase hex
let hex = format!("{:08X}", checksum); // uppercase hex, zero-padded to 8 chars

// write! — writes to a formatter or writer
write!(f, "WAL I/O error: {}", e)?;

// println! — writes to stdout
println!("Entry count: {}", self.entry_count);
```

Format specifiers:
| Specifier | Output for 255          |
|-----------|-------------------------|
| `{}`      | `255`                   |
| `{:?}`    | `255` (Debug)           |
| `{:x}`    | `ff` (lowercase hex)    |
| `{:X}`    | `FF` (uppercase hex)    |
| `{:08X}`  | `000000FF` (padded hex) |
| `{:b}`    | `11111111` (binary)     |

---

## Exercises

1. Look at `Cargo.toml`. What version of `bincode` and `serde` does the project use?
   What does the `features = ["derive"]` in serde's entry enable?

2. Why does the project create `SerializableBlock` instead of adding
   `#[derive(Serialize, Deserialize)]` directly to `Block`? (Hint: look at what
   `Signature` contains — `ed25519_dalek` types may not implement serde.)

3. What would happen if you changed the order of fields in `LogEntry::BlockInserted`
   and tried to read a WAL file written with the old order? (Hint: bincode is
   position-based, not name-based.)

4. The WAL uses `bincode` for entries but writes the header (magic, version, etc.)
   manually with `write_all()`. Why not serialize the header with bincode too?

---

Next lesson: [13 — Testing in Rust](13_testing.md)
