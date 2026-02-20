# Lesson 5: Option & Result Types

## Where this appears in the project
- `Option<Hash>` in `Block.parent`, `Block.qc`
- `Option<QuorumCert>` in `Replica.high_qc`
- `Option<WAL>` in `Replica.wal`
- `Result<Self, WALError>` in `WAL::create()`, `WAL::open()`
- `Result<(), WALError>` in `WAL::append()`
- `Result<(), String>` in `Replica::take_snapshot()`

---

## 5.1 — The Problem: Null Doesn't Exist in Rust

In languages like Java, C, or Python, any reference can be `null`/`None` and you
find out at runtime when things blow up. Rust eliminates this entire category of
bugs by making "absence" explicit in the type system.

```rust
// This is NOT valid Rust:
// let x: Hash = null;    // COMPILE ERROR — no null in Rust

// Instead:
let x: Option<Hash> = None;        // explicitly "no value"
let y: Option<Hash> = Some(42);    // explicitly "has value 42"
```

---

## 5.2 — `Option<T>` — Maybe a Value

`Option<T>` is just an enum with two variants:
```rust
enum Option<T> {
    Some(T),    // there is a value of type T
    None,       // there is no value
}
```

### From `src/types.rs`:
```rust
pub struct Block {
    pub hash: Hash,
    pub parent: Option<Hash>,         // genesis block has parent = None
    pub view: u64,
    pub proposer: ReplicaId,
    pub qc: Option<QuorumCert>,       // some blocks carry a QC, some don't
}
```

Creating values:
```rust
// Genesis block — no parent, no QC
let genesis = Block {
    hash: 0,
    parent: None,
    view: 0,
    proposer: 0,
    qc: None,
};

// Normal block — has a parent and carries a QC
let block = Block {
    hash: 1,
    parent: Some(0),           // parent is genesis
    view: 1,
    proposer: 0,
    qc: Some(some_qc),
};
```

---

## 5.3 — Extracting Values from `Option`

### Method 1: `match`
```rust
match &replica.high_qc {
    Some(qc) => println!("High QC for block {}", qc.block_hash),
    None     => println!("No high QC yet"),
}
```

### Method 2: `if let` (when you only care about `Some`)
```rust
// From src/replica.rs:
if let Some(qc) = &self.high_qc {
    println!("High QC: Block {} (view {})", qc.block_hash, qc.view);
} else {
    println!("High QC: None");
}
```

### Method 3: `.unwrap()` — panics if None
```rust
let hash = some_option.unwrap();  // CRASHES if None
```
Only use this when you are 100% sure the value is `Some`. The project uses it
sparingly:

```rust
// From src/replica.rs — we know there's always at least one block:
let max_view = self.block_tree.values().map(|b| b.view).max().unwrap_or(0);
```

### Method 4: `.unwrap_or(default)` — safe fallback
```rust
// If max() returns None (empty iterator), use 0 instead
let max_view = self.block_tree.values()
    .map(|b| b.view)
    .max()
    .unwrap_or(0);         // returns 0 if None
```

### Method 5: `.map()` — transform the inner value
```rust
// From src/replica.rs:
let high_qc_hash = self.high_qc.as_ref().map(|qc| qc.block_hash);
// If high_qc is Some(qc), returns Some(qc.block_hash)
// If high_qc is None, returns None
```

### Method 6: `.is_some()` / `.is_none()` — just check
```rust
// From src/replica.rs — checking if a QC exists:
if b1.qc.is_none() {
    continue;    // skip this block, it has no QC
}
```

---

## 5.4 — `as_ref()` — Borrowing Inside an Option

A subtle but critical method. `Option<T>` owns its inner value. If you want to
peek at it without taking ownership, use `.as_ref()`:

```rust
// self.high_qc is Option<QuorumCert>

// as_ref() converts Option<QuorumCert> → Option<&QuorumCert>
// Now we can look at the QC without moving it out of self
let high_qc_hash = self.high_qc.as_ref().map(|qc| qc.block_hash);
```

Without `as_ref()`, `map()` would try to move the QuorumCert out of `self.high_qc`,
which would leave `self` in a partially-moved state — the compiler rejects this.

---

## 5.5 — `Result<T, E>` — Success or Failure

`Result<T, E>` is an enum for operations that can fail:
```rust
enum Result<T, E> {
    Ok(T),     // success, carrying a value of type T
    Err(E),    // failure, carrying an error of type E
}
```

### From `src/wal.rs`:
```rust
pub fn create(replica_id: ReplicaId, dir: &Path) -> Result<Self, WALError> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("replica_{}_wal.log", replica_id));

    let mut file = OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .open(&path)?;

    // ... write header ...
    file.sync_all()?;

    Ok(WAL {                     // success: return the WAL handle
        file,
        path,
        replica_id,
        entry_count: 0,
    })
}
```

- `Ok(WAL { ... })` — success path, returns the newly created WAL
- The `?` operators propagate errors (covered in Lesson 6)

### `Result<(), E>` — Success with No Value

When the function succeeds but has nothing to return:

```rust
// From src/wal.rs:
pub fn append(&mut self, entry: &LogEntry) -> Result<(), WALError> {
    // ... write to file ...
    Ok(())      // () is the "unit type" — like void in C
}
```

`()` is Rust's "nothing" type. `Ok(())` means "succeeded, nothing to return."

---

## 5.6 — Handling Results

### Method 1: `match`
```rust
match WAL::create(0, &path) {
    Ok(wal) => println!("Created WAL successfully"),
    Err(e)  => println!("Failed: {}", e),
}
```

### Method 2: `.unwrap()` — panics on error
```rust
let wal = WAL::create(0, &path).unwrap();  // CRASHES if Err
```

### Method 3: `.map_err()` — convert error type
```rust
// From src/replica.rs:
snap.save(data_dir).map_err(|e| format!("{}", e))?;
```
This converts `SnapshotError` into a `String` so it matches the function's return
type `Result<(), String>`.

---

## 5.7 — `Option` in the Project: Tracking State

### From `src/replica.rs`:
```rust
pub struct Replica {
    pub high_qc: Option<QuorumCert>,      // no QC until first quorum
    pub committed_up_to: Option<Hash>,    // no commits until 3-chain
    pub wal: Option<WAL>,                  // persistence is optional
}
```

Each `Option` here represents a real-world concept:
- `high_qc: None` → protocol just started, no quorum formed yet
- `committed_up_to: None` → nothing committed yet
- `wal: None` → running without persistence (e.g., in unit tests)

### Conditional WAL logging:
```rust
// From src/replica.rs:
if let Some(ref mut wal) = self.wal {
    let _ = wal.append(&LogEntry::QCFormed { ... });
}
```

If `self.wal` is `None`, the WAL logging is silently skipped. If it's `Some(wal)`,
we append to it. This lets the same code work with and without persistence.

`let _ = ...` means "I know this returns a Result but I'm intentionally ignoring
whether it succeeded." The underscore suppresses the compiler warning.

---

## Exercises

1. In `src/replica.rs`, the `find_committed_block()` method returns `Option<Block>`.
   Trace through the code: under what conditions does it return `None` vs `Some`?

2. Change the WAL logging in `try_form_qc()` to use `match` instead of `if let`.
   Which style do you find more readable?

3. What's the difference between `Option<&QuorumCert>` and `&Option<QuorumCert>`?
   (Hint: who owns the QuorumCert, and who owns the Option?)

4. Why does `Replica.wal` use `Option<WAL>` instead of always requiring a WAL?
   Think about testing — what would tests look like if WAL was mandatory?

---

Next lesson: [06 — Error Handling & the ? Operator](06_error_handling.md)
