# Lesson 6: Error Handling & the `?` Operator

## Where this appears in the project
- `src/wal.rs` — `WALError`, `From` impls, `?` operator everywhere
- `src/snapshot.rs` — `SnapshotError`, `From` impls
- `src/recovery.rs` — `RecoveryError`, `From` impls
- `src/replica.rs` — `map_err()` to convert error types

---

## 6.1 — The `?` Operator: Propagate Errors Concisely

The `?` operator is syntax sugar. When you write `expression?`, Rust does this:

```rust
// This:
let value = some_operation()?;

// Is equivalent to:
let value = match some_operation() {
    Ok(v)  => v,        // unwrap the success value
    Err(e) => return Err(e.into()),  // convert and return the error
};
```

If the operation succeeds, you get the value. If it fails, the error is returned
from the **current function** immediately.

### From `src/wal.rs` — WAL::create():
```rust
pub fn create(replica_id: ReplicaId, dir: &Path) -> Result<Self, WALError> {
    std::fs::create_dir_all(dir)?;         // ? on io::Error → WALError::Io
    let path = dir.join(format!("replica_{}_wal.log", replica_id));

    let mut file = OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .open(&path)?;                      // ? on io::Error → WALError::Io

    file.write_all(&WAL_MAGIC)?;           // ? on io::Error → WALError::Io
    file.write_all(&WAL_VERSION.to_le_bytes())?;
    file.write_all(&replica_id.to_le_bytes())?;
    let created_ts = Self::now_ms();
    file.write_all(&created_ts.to_le_bytes())?;

    file.sync_all()?;                      // ? on io::Error → WALError::Io

    Ok(WAL { file, path, replica_id, entry_count: 0 })
}
```

Every `?` here can fail. If `create_dir_all` fails, the function immediately returns
`Err(WALError::Io(...))`. If everything succeeds, we reach `Ok(...)` at the bottom.

**Without `?`**, this would be deeply nested match statements:
```rust
// DON'T write this — it's the point-free version of the above
match std::fs::create_dir_all(dir) {
    Ok(_) => {
        match OpenOptions::new().write(true).read(true).create_new(true).open(&path) {
            Ok(mut file) => {
                match file.write_all(&WAL_MAGIC) {
                    Ok(_) => {
                        // ... 5 more nested matches ...
                    },
                    Err(e) => Err(WALError::Io(e)),
                }
            },
            Err(e) => Err(WALError::Io(e)),
        }
    },
    Err(e) => Err(WALError::Io(e)),
}
```

---

## 6.2 — The `From` Trait: Automatic Error Conversion

The `?` operator calls `.into()` on the error, which uses the `From` trait to
convert one error type to another. You must implement `From` for this to work.

### From `src/wal.rs`:
```rust
impl From<io::Error> for WALError {
    fn from(e: io::Error) -> Self {
        WALError::Io(e)
    }
}

impl From<Box<bincode::ErrorKind>> for WALError {
    fn from(e: Box<bincode::ErrorKind>) -> Self {
        WALError::Serialization(e)
    }
}
```

Now when `?` encounters an `io::Error`, it automatically wraps it in
`WALError::Io(...)`. When it encounters a bincode error, it wraps it in
`WALError::Serialization(...)`.

### Same pattern in `src/snapshot.rs`:
```rust
impl From<io::Error> for SnapshotError {
    fn from(e: io::Error) -> Self {
        SnapshotError::Io(e)
    }
}

impl From<Box<bincode::ErrorKind>> for SnapshotError {
    fn from(e: Box<bincode::ErrorKind>) -> Self {
        SnapshotError::Serialization(e)
    }
}
```

### The pattern: define error enum → implement From for each cause → use `?`

```
io::Error ──── From ────→ WALError::Io
bincode error ── From ──→ WALError::Serialization
                          WALError::CorruptedEntry   (constructed manually)
                          WALError::InvalidMagic     (constructed manually)
                          WALError::UnsupportedVersion (constructed manually)
```

---

## 6.3 — Implementing `Display` for Error Types

The `Display` trait lets errors produce human-readable messages with `{}`.

### From `src/wal.rs`:
```rust
impl std::fmt::Display for WALError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WALError::Io(e) =>
                write!(f, "WAL I/O error: {}", e),
            WALError::Serialization(e) =>
                write!(f, "WAL serialization error: {}", e),
            WALError::CorruptedEntry { offset, expected, actual } =>
                write!(f, "Corrupted WAL entry at offset {}: expected CRC 0x{:08X}, got 0x{:08X}",
                       offset, expected, actual),
            WALError::InvalidMagic =>
                write!(f, "Not a valid WAL file (bad magic bytes)"),
            WALError::UnsupportedVersion(v) =>
                write!(f, "WAL version {} is not supported", v),
        }
    }
}
```

Key formatting:
- `{}` — calls Display (human-readable)
- `{:?}` — calls Debug (programmer-readable, derive-able)
- `{:08X}` — uppercase hex, zero-padded to 8 digits

---

## 6.4 — `map_err()` — Converting Between Error Types

When function A returns `Result<T, ErrorA>` but you need `Result<T, ErrorB>`, use
`.map_err()`:

### From `src/replica.rs`:
```rust
pub fn take_snapshot(&mut self, data_dir: &Path) -> Result<(), String> {
    let snap = Snapshot::capture(...);

    // snap.save() returns Result<(), SnapshotError>
    // but this function returns Result<(), String>
    // map_err converts SnapshotError → String
    snap.save(data_dir).map_err(|e| format!("{}", e))?;

    if let Some(ref mut wal) = self.wal {
        wal.truncate_after_snapshot()
            .map_err(|e| format!("{}", e))?;   // WALError → String
    }

    Ok(())
}
```

`format!("{}", e)` calls the `Display` impl to get a human-readable string.

---

## 6.5 — Ignoring Errors with `let _ = ...`

Sometimes you deliberately want to try something and not care if it fails:

### From `src/replica.rs`:
```rust
if let Some(ref mut wal) = self.wal {
    let _ = wal.append(&LogEntry::QCFormed { ... });
}
```

`let _ = ...` tells the compiler "I know this returns a Result, and I'm intentionally
discarding it." Without the underscore, you'd get a warning:
```
warning: unused `Result` that must be used
```

Use this sparingly — most errors should be handled. Here it's acceptable because
a WAL write failure during normal operation shouldn't halt the consensus protocol.

---

## 6.6 — Error Propagation Chain

Let's trace a complete error path through the project:

```
1. file.write_all(&data)     → fails with io::Error
2. ? operator                → calls io::Error.into()
3. From<io::Error> for WALError → wraps as WALError::Io(io_err)
4. ? operator                → returns Err(WALError::Io(io_err)) from WAL::append()
5. Caller uses map_err       → converts to String: "WAL I/O error: ..."
6. ? operator                → returns Err(String) from take_snapshot()
7. Final caller does match   → displays the error message to user
```

---

## 6.7 — Return Type Shorthand

You'll see these return types throughout the project:

| Return type                   | Meaning                                        |
|-------------------------------|------------------------------------------------|
| `Result<Self, WALError>`      | Returns a WAL or a WAL-specific error          |
| `Result<(), WALError>`        | Succeeds with nothing, or fails with WALError  |
| `Result<Vec<LogEntry>, WALError>` | Returns entries or fails                   |
| `Result<(), String>`          | Succeeds or returns a human-readable error     |

---

## 6.8 — `unwrap()` vs `expect()` vs `?`

```rust
// unwrap(): panics with a generic message if Err
let wal = WAL::create(0, &path).unwrap();

// expect(): panics with YOUR message if Err
let wal = WAL::create(0, &path).expect("Failed to create WAL");

// ?: returns the error to the caller (no panic)
let wal = WAL::create(0, &path)?;
```

Rules of thumb:
- **`?`** — use in library/production code (propagate errors up)
- **`expect("reason")`** — use when you KNOW it should never fail and want a good message
- **`unwrap()`** — use in tests or throwaway code only

---

## Exercises

1. In `src/wal.rs`, find all the places where `?` is used in the `append()` method.
   For each one, what type of error could it produce?

2. Write a `From<WALError> for String` implementation. Now you could use `?` directly
   in `take_snapshot()` without `map_err()`.

3. What happens if you remove the `From<io::Error> for WALError` implementation and
   try to compile? What error message does the compiler give you?

4. In `src/recovery.rs`, find the `RecoveryError` enum. How does it differ from
   `WALError` and `SnapshotError`? Does it wrap those error types?

---

Next lesson: [07 — Ownership, Borrowing & References](07_ownership_and_borrowing.md)
