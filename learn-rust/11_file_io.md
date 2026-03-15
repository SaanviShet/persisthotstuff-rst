# Lesson 11: File I/O & Paths

## Where this appears in the project
- `src/wal.rs` — file creation, header writing, entry appending, fsync
- `src/snapshot.rs` — binary file save/load, SHA-256 sidecar files
- `std::path::{Path, PathBuf}` — cross-platform path handling
- `std::fs` — directory creation, file listing, deletion

---

## 11.1 — `Path` and `PathBuf`

These are Rust's cross-platform path types (like `&str` and `String` for file paths).

```rust
use std::path::{Path, PathBuf};

let p: &Path = Path::new("/data/wal");        // borrowed path (like &str)
let mut pb: PathBuf = PathBuf::from("/data");  // owned path (like String)
pb.push("wal");                                // /data/wal
pb.push("replica_0_wal.log");                  // /data/wal/replica_0_wal.log
```

### From `src/wal.rs` — building a file path:
```rust
pub fn create(replica_id: ReplicaId, dir: &Path) -> Result<Self, WALError> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("replica_{}_wal.log", replica_id));
    // dir = /tmp/data → path = /tmp/data/replica_0_wal.log
```

Key methods:
- `dir.join("file.txt")` — append a component, returns `PathBuf`
- `path.exists()` — check if a file/directory exists
- `path.is_file()` / `path.is_dir()` — type check
- `path.extension()` — get file extension
- `path.file_name()` — get just the filename
- `path.parent()` — get the parent directory

### Why `&Path` in function signatures:
```rust
pub fn create(replica_id: ReplicaId, dir: &Path) -> Result<Self, WALError>
//                                        ↑ borrowed — doesn't take ownership
```

The caller keeps their path, we just borrow it. Both `&PathBuf` and `&str` can
be passed as `&Path` automatically (Rust's deref coercion).

---

## 11.2 — Creating and Opening Files

### From `src/wal.rs` — creating a new file:
```rust
use std::fs::{File, OpenOptions};

let mut file = OpenOptions::new()
    .write(true)        // we want to write
    .read(true)         // we also want to read
    .create_new(true)   // fail if file already exists
    .open(&path)?;
```

`OpenOptions` is a builder pattern (chain methods to set options, then call `.open()`).

| Method           | What it does                                          |
|------------------|-------------------------------------------------------|
| `.write(true)`   | Allow writing                                         |
| `.read(true)`    | Allow reading                                         |
| `.create(true)`  | Create if doesn't exist, open if it does              |
| `.create_new(true)` | Create ONLY if doesn't exist (fail otherwise)     |
| `.append(true)`  | All writes go to end of file                          |
| `.truncate(true)` | Truncate file to zero length on open                 |

### From `src/wal.rs` — opening an existing file:
```rust
let mut file = OpenOptions::new()
    .read(true)
    .write(true)
    .open(&path)?;       // fails if file doesn't exist
```

---

## 11.3 — Reading from Files

### Read exact number of bytes:
```rust
// From src/wal.rs — reading the magic bytes:
let mut magic = [0u8; 4];         // stack buffer
file.read_exact(&mut magic)?;      // fills exactly 4 bytes, or fails

if magic != WAL_MAGIC {
    return Err(WALError::InvalidMagic);
}
```

`read_exact` reads EXACTLY the requested number of bytes. If the file is shorter,
it returns an error.

### Read integers from bytes:
```rust
// From src/wal.rs:
let mut ver_bytes = [0u8; 2];
file.read_exact(&mut ver_bytes)?;
let version = u16::from_le_bytes(ver_bytes);  // [u8; 2] → u16
```

`from_le_bytes` converts little-endian byte arrays into integers. The array size
must match: `u16` needs `[u8; 2]`, `u32` needs `[u8; 4]`, `u64` needs `[u8; 8]`.

### Read entire file into bytes:
```rust
// From src/snapshot.rs:
use std::io::Read;

let mut file = File::open(&snap_path)?;
let mut bytes = Vec::new();
file.read_to_end(&mut bytes)?;    // reads everything into the vector
```

---

## 11.4 — Writing to Files

### Write raw bytes:
```rust
// From src/wal.rs — writing the header:
file.write_all(&WAL_MAGIC)?;                          // 4 bytes
file.write_all(&WAL_VERSION.to_le_bytes())?;           // 2 bytes
file.write_all(&replica_id.to_le_bytes())?;            // 8 bytes
```

`write_all` writes the ENTIRE buffer. Unlike `write()` which might write partially,
`write_all` guarantees all bytes are written or returns an error.

### Writing integers as bytes:
```rust
let len = payload.len() as u32;
self.file.write_all(&len.to_le_bytes())?;       // u32 → [u8; 4] → disk

let checksum: u32 = crc32fast::hash(&payload);
self.file.write_all(&checksum.to_le_bytes())?;  // CRC → disk
```

`.to_le_bytes()` converts an integer to a little-endian byte array.

---

## 11.5 — File Seeking

Moving the file cursor (read/write position):

```rust
use std::io::{Seek, SeekFrom};

// From src/wal.rs:
file.seek(SeekFrom::Start(HEADER_SIZE))?;   // jump to position 30 (after header)
file.seek(SeekFrom::End(0))?;                // jump to end of file

// SeekFrom variants:
// SeekFrom::Start(n)   — n bytes from the beginning
// SeekFrom::End(n)     — n bytes from the end (usually 0 for end)
// SeekFrom::Current(n) — n bytes from current position
```

---

## 11.6 — `fsync` / `sync_all()` — Durability Guarantee

The most critical I/O concept in this project.

### From `src/wal.rs`:
```rust
// After writing an entry:
self.file.sync_all()?;
```

What `sync_all()` does:
1. Flushes the OS write buffer to the disk controller
2. Tells the disk to write its internal cache to the physical medium
3. Updates file metadata (size, modification time)

**Without `sync_all()`**: If the power fails after `write_all()`, the data might
still be in OS memory and lost forever. With `sync_all()`, the data is on the
physical disk.

This is why the WAL is crash-safe — every entry is fsync'd before the function
returns. The trade-off: fsync is slow (~1-10ms), which is why `append_batch()`
does only ONE fsync for multiple entries.

---

## 11.7 — Directory Operations

### Create directory tree:
```rust
// From src/wal.rs:
std::fs::create_dir_all(dir)?;   // like mkdir -p, creates all parents
```

### List directory contents:
```rust
// From src/snapshot.rs:
for entry in std::fs::read_dir(data_dir)? {
    let entry = entry?;           // each entry can fail independently
    let path = entry.path();
    let name = entry.file_name(); // OsString, not &str
}
```

### Delete a file:
```rust
std::fs::remove_file(&path)?;
```

---

## 11.8 — `BufWriter` and `BufReader` — Buffered I/O

For many small writes, buffering reduces system calls:

```rust
use std::io::BufWriter;

let file = File::create("output.bin")?;
let mut writer = BufWriter::new(file);
// Multiple writes are buffered in memory...
writer.write_all(&data1)?;
writer.write_all(&data2)?;
writer.write_all(&data3)?;
// ...and flushed to the OS in one large write
writer.flush()?;
```

The WAL in this project does NOT use BufWriter because it needs explicit fsync
control after each entry. Buffering would defeat the durability guarantee.

---

## 11.9 — `format!()` for Building Strings

Used to construct file names and paths:

### From `src/wal.rs`:
```rust
let path = dir.join(format!("replica_{}_wal.log", replica_id));
// replica_id = 3 → "replica_3_wal.log"
```

### From `src/snapshot.rs`:
```rust
let filename = format!("replica_{}_snapshot_{}.bin", replica_id, seq);
// replica_id = 0, seq = 5 → "replica_0_snapshot_5.bin"
```

---

## 11.10 — Complete File I/O Example from the Project

Here's the full WAL entry append flow:

```rust
pub fn append(&mut self, entry: &LogEntry) -> Result<(), WALError> {
    // 1. Serialize the Rust struct into raw bytes
    let payload: Vec<u8> = bincode::serialize(entry)?;

    // 2. Compute CRC32 checksum for integrity verification
    let checksum: u32 = crc32fast::hash(&payload);

    // 3. Write length prefix (so reader knows how many bytes to read)
    let len = payload.len() as u32;
    self.file.write_all(&len.to_le_bytes())?;      // 4 bytes

    // 4. Write checksum (so reader can verify data wasn't corrupted)
    self.file.write_all(&checksum.to_le_bytes())?;  // 4 bytes

    // 5. Write the actual serialized data
    self.file.write_all(&payload)?;                  // N bytes

    // 6. Force everything to physical disk
    self.file.sync_all()?;

    // 7. Update in-memory counter
    self.entry_count += 1;

    Ok(())
}
```

And reading it back:
```rust
// Read length
let mut len_bytes = [0u8; 4];
file.read_exact(&mut len_bytes)?;
let len = u32::from_le_bytes(len_bytes) as usize;

// Read checksum
let mut crc_bytes = [0u8; 4];
file.read_exact(&mut crc_bytes)?;
let stored_crc = u32::from_le_bytes(crc_bytes);

// Read payload
let mut payload = vec![0u8; len];
file.read_exact(&mut payload)?;

// Verify checksum
let computed_crc = crc32fast::hash(&payload);
if stored_crc != computed_crc {
    return Err(WALError::CorruptedEntry { ... });
}

// Deserialize
let entry: LogEntry = bincode::deserialize(&payload)?;
```

---

## Exercises

1. In `src/wal.rs`, why does `create()` use `create_new(true)` instead of
   `create(true)`? What could go wrong with `create(true)`?

2. The WAL header is 30 bytes. Manually verify: 4 (magic) + 2 (version) +
   8 (replica_id) + 16 (timestamp as u128) = 30. Why is the timestamp a `u128`?

3. What would happen if you removed `sync_all()` from `append()`? Write a
   scenario where data loss occurs.

4. In `src/snapshot.rs`, find the SHA-256 verification code. How does it
   differ from the CRC32 approach in the WAL? What are the trade-offs?

---

Next lesson: [12 — Serialization with Serde](12_serialization.md)
