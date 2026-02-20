# Lesson 3: Enums & Pattern Matching

## Where this appears in the project
- `src/network.rs` — `Message` enum (4 variants with named fields)
- `src/wal.rs` — `LogEntry` enum (7 variants), `WALError` enum, `ViewChangeReason` enum
- `src/snapshot.rs` — `SnapshotError` enum

---

## 3.1 — Basic Enums

An enum in Rust can hold one of several **variants**. Unlike C enums (which are just
integers), Rust enums can carry data.

### Simplest form — no data:
```rust
// From src/wal.rs:
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ViewChangeReason {
    Timeout,
    Commit,
}
```
A `ViewChangeReason` is EITHER `Timeout` OR `Commit` — nothing else. This is
exhaustive: the compiler knows every possible value.

---

## 3.2 — Enums with Named Fields (Struct Variants)

Each variant can hold different data, structured like an anonymous struct.

### From `src/network.rs`:
```rust
#[derive(Clone, Debug)]
pub enum Message {
    Proposal {
        from: ReplicaId,
        to: ReplicaId,
        block: Block,
    },
    Vote {
        from: ReplicaId,
        to: ReplicaId,
        vote: Vote,
    },
    QuorumCertBroadcast {
        from: ReplicaId,
        to: ReplicaId,
        qc: QuorumCert,
        block_hash: Hash,
    },
    NewView {
        from: ReplicaId,
        to: ReplicaId,
        view: u64,
        high_qc: Option<QuorumCert>,
    },
}
```

Each variant has different fields. A `Message::Proposal` carries a `Block`, while a
`Message::Vote` carries a `Vote`. They all share `from` and `to`, but each variant
can have unique fields too.

### How to create an enum value:
```rust
// From src/network.rs — constructing a Proposal message:
self.send(Message::Proposal {
    from,
    to: to as ReplicaId,
    block: block.clone(),
});
```

---

## 3.3 — Enums as Error Types

A very common Rust pattern: define an error enum with one variant per error kind.

### From `src/wal.rs`:
```rust
#[derive(Debug)]
pub enum WALError {
    Io(io::Error),                           // wraps a standard I/O error
    Serialization(Box<bincode::ErrorKind>),   // wraps a bincode error
    CorruptedEntry {                           // struct-like variant
        offset: u64,
        expected: u32,
        actual: u32,
    },
    InvalidMagic,                             // no data, just a signal
    UnsupportedVersion(u16),                  // tuple variant with one field
}
```

Notice the three different variant styles:
1. **Tuple variant**: `Io(io::Error)` — wraps one value
2. **Struct variant**: `CorruptedEntry { offset, expected, actual }` — named fields
3. **Unit variant**: `InvalidMagic` — no data at all

### Same pattern in `src/snapshot.rs`:
```rust
pub enum SnapshotError {
    Io(io::Error),
    Serialization(Box<bincode::ErrorKind>),
    ChecksumMismatch { expected: String, actual: String },
    NotFound,
}
```

---

## 3.4 — Pattern Matching with `match`

`match` is Rust's way of branching on an enum variant. The compiler **forces you to
handle every variant** — you can't forget one.

### From `src/network.rs` — extracting the sender:
```rust
impl Message {
    pub fn sender(&self) -> ReplicaId {
        match self {
            Message::Proposal { from, .. }            => *from,
            Message::Vote { from, .. }                => *from,
            Message::QuorumCertBroadcast { from, .. } => *from,
            Message::NewView { from, .. }             => *from,
        }
    }
}
```

Breaking this down:
- `match self` — look at which variant `self` is
- `Message::Proposal { from, .. }` — destructure the Proposal variant, bind `from`,
  ignore everything else (`..`)
- `=> *from` — dereference the reference and return it
- Every variant MUST be covered or you get a compile error

### From `src/network.rs` — returning a string label:
```rust
pub fn msg_type(&self) -> &str {
    match self {
        Message::Proposal { .. }            => "Proposal",
        Message::Vote { .. }                => "Vote",
        Message::QuorumCertBroadcast { .. } => "QC-Broadcast",
        Message::NewView { .. }             => "NewView",
    }
}
```

### From `src/wal.rs` — Display for WALError:
```rust
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
```

Here each variant is destructured differently:
- `Io(e)` — extracts the inner `io::Error` into `e`
- `CorruptedEntry { offset, expected, actual }` — pulls out all three named fields
- `UnsupportedVersion(v)` — extracts the single `u16` value
- `InvalidMagic` — no data to extract

---

## 3.5 — Match with `if let` (Single-Variant Check)

When you only care about ONE variant and want to ignore the rest, use `if let`:

### From `src/replica.rs`:
```rust
let parent = if let Some(qc) = &self.high_qc {
    Some(qc.block_hash)
} else {
    self.latest_block_hash()
};
```

This says: "if `high_qc` is `Some`, extract the QC; otherwise do something else."
It's equivalent to:

```rust
let parent = match &self.high_qc {
    Some(qc) => Some(qc.block_hash),
    None => self.latest_block_hash(),
};
```

### Another example from `src/replica.rs`:
```rust
if let Some(ref qc) = block.qc {
    if !self.keystore.verify_qc(qc, self.config.quorum_size()) {
        return false;
    }
}
```

`ref` borrows the inner value instead of moving it. We'll cover this more in
Lesson 7 (Ownership & Borrowing).

---

## 3.6 — `match` on Integers and Wildcards

Match works on more than just enums:

### From `src/network.rs` — updating stats by message type:
```rust
let idx = match msg {
    Message::Proposal { .. }            => 0,
    Message::Vote { .. }                => 1,
    Message::QuorumCertBroadcast { .. } => 2,
    Message::NewView { .. }             => 3,
};
self.messages_by_type[idx] += 1;
```

The wildcard `_` matches anything:
```rust
match some_number {
    0 => println!("zero"),
    1 => println!("one"),
    _ => println!("something else"),   // catch-all
}
```

---

## 3.7 — The `Option` and `Result` Enums (Built-In)

Rust's two most important enums are built into the standard library:

```rust
// Option — either something or nothing (replaces null)
enum Option<T> {
    Some(T),
    None,
}

// Result — either success or failure (replaces exceptions)
enum Result<T, E> {
    Ok(T),
    Err(E),
}
```

These are so common they get their own lesson (Lesson 5), but know that they are
just enums under the hood — everything you learned here applies to them.

---

## 3.8 — Enums with Complex Data (WAL LogEntry)

### From `src/wal.rs`:
```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum LogEntry {
    BlockInserted {
        hash: Hash,
        parent: Option<Hash>,
        view: u64,
        proposer: ReplicaId,
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
    QCFormed {
        block_hash: Hash,
        view: u64,
        signer_count: usize,
        timestamp: u128,
    },
    // ... 4 more variants
}
```

This single enum captures every possible state change in the protocol. The WAL
serialises whichever variant is appended — bincode handles the discrimination
automatically.

---

## Exercises

1. Add a fifth `Message` variant called `Heartbeat { from: ReplicaId, view: u64 }`.
   What happens when you compile? (Hint: every `match` on `Message` must be updated.)

2. In `src/wal.rs`, count how many `LogEntry` variants there are. For each one, is
   it a struct variant, tuple variant, or unit variant?

3. Write a function `fn describe_error(e: &WALError) -> String` that returns a
   one-word description: "io", "serialization", "corruption", "magic", or "version".
   Use `match`.

4. What's the difference between `if let Some(x) = value` and `match value { Some(x) => ..., None => ... }`?
   When would you prefer one over the other?

---

Next lesson: [04 — Impl Blocks & Methods](04_impl_blocks_and_methods.md)
