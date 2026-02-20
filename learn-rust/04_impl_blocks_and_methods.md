# Lesson 4: Impl Blocks & Methods

## Where this appears in the project
- `src/config.rs` — `impl Config` with `quorum_size()`, `leader_for_view()`
- `src/replica.rs` — large `impl Replica` block with all protocol logic
- `src/wal.rs` — `impl WAL` with `create()`, `open()`, `append()`, etc.
- `src/network.rs` — `impl Message`, `impl Network`

---

## 4.1 — What is an `impl` Block?

An `impl` block attaches functions (methods) to a struct or enum. It's like writing
methods inside a class in Java/Python, but the struct definition and its methods
are separate.

### From `src/config.rs`:
```rust
pub struct Config {
    pub n: usize,
    pub f: usize,
    pub id: ReplicaId,
    pub timeout_ms: u64,
}

impl Config {
    pub fn quorum_size(&self) -> usize {
        2 * self.f + 1
    }

    pub fn leader_for_view(&self, view: u64) -> ReplicaId {
        let idx = (view as usize) % self.n;
        idx as ReplicaId
    }
}
```

The struct `Config` holds data. The `impl Config` block gives it behavior.
You can have **multiple** `impl` blocks for the same type (useful for organising code).

---

## 4.2 — `&self`, `&mut self`, and `self`

The first parameter of a method determines how it accesses the struct:

| Signature         | What it means                                    | Can modify fields? |
|-------------------|--------------------------------------------------|--------------------|
| `&self`           | Immutable borrow — read-only access              | No                 |
| `&mut self`       | Mutable borrow — can modify fields               | Yes                |
| `self`            | Takes ownership — consumes the struct             | Yes (but struct is gone after) |
| (no self)         | Associated function (like a static method)        | N/A                |

### `&self` — read-only access:
```rust
// From src/config.rs:
pub fn quorum_size(&self) -> usize {
    2 * self.f + 1       // reads self.f but doesn't change anything
}
```

### `&mut self` — can modify fields:
```rust
// From src/replica.rs:
pub fn on_view_timeout(&mut self) {
    let old = self.current_view;
    self.current_view += 1;        // modifies self.current_view
    self.view_start_time = Self::current_time_ms();
    self.vote_pool.clear();        // modifies self.vote_pool
}
```

### No self — associated function (static method):
```rust
// From src/replica.rs:
pub fn current_time_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
```

Called as `Replica::current_time_ms()` — no instance needed. This is like a static
method in Java.

### `Self` (capital S):
Inside an `impl` block, `Self` refers to the type being implemented. So inside
`impl Config`, `Self` means `Config`.

```rust
// From src/wal.rs:
impl WAL {
    pub fn create(replica_id: ReplicaId, dir: &Path) -> Result<Self, WALError> {
        // ...
        Ok(WAL { file, path, replica_id, entry_count: 0 })
        // Self == WAL here, so this returns a WAL
    }
}
```

---

## 4.3 — Calling Methods

```rust
// Calling &self methods:
let quorum = config.quorum_size();          // dot notation
let leader = config.leader_for_view(3);

// Calling &mut self methods:
replica.on_view_timeout();                   // needs &mut replica

// Calling associated functions (no self):
let time = Replica::current_time_ms();       // Type::function()
let wal = WAL::create(0, &path)?;            // Type::function()
```

---

## 4.4 — Method Chaining

Methods that return a value can be chained:

### From `src/crypto.rs`:
```rust
pub fn vote_message(block_hash: u64, view: u64) -> Vec<u8> {
    let mut hasher = Sha256::new();       // create hasher
    hasher.update(block_hash.to_le_bytes());  // feed data
    hasher.update(view.to_le_bytes());        // feed more data
    hasher.finalize().to_vec()                // finalize → GenericArray → Vec<u8>
}
```

`hasher.finalize()` returns a hash result, and `.to_vec()` is called immediately on
that result. This avoids storing the intermediate value.

---

## 4.5 — Methods Returning `Option` and `Result`

Many methods in the project return `Option<T>` to signal "might not produce a value":

### From `src/replica.rs`:
```rust
pub fn propose(&mut self, view: u64) -> Option<Block> {
    if !self.is_leader(view) {
        return None;          // early return: not the leader
    }

    let hash = self.next_hash;
    self.next_hash = self.next_hash.wrapping_add(1);

    let parent = if let Some(qc) = &self.high_qc {
        Some(qc.block_hash)
    } else {
        self.latest_block_hash()
    };

    let block = Block {
        hash,
        parent,
        view,
        proposer: self.config.id,
        qc: self.high_qc.clone(),
    };
    self.block_tree.insert(hash, block.clone());
    Some(block)               // success: wrap in Some
}
```

And `Result<T, E>` for operations that can fail:

### From `src/wal.rs`:
```rust
pub fn append(&mut self, entry: &LogEntry) -> Result<(), WALError> {
    let payload: Vec<u8> = bincode::serialize(entry)?;   // can fail
    let checksum: u32 = crc32fast::hash(&payload);
    // ... writing to file ...
    self.file.sync_all()?;                                // can fail
    self.entry_count += 1;
    Ok(())                                                // success
}
```

---

## 4.6 — Impl Blocks for Enums

You can also attach methods to enums:

### From `src/network.rs`:
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

    pub fn receiver(&self) -> ReplicaId {
        match self {
            Message::Proposal { to, .. } => *to,
            Message::Vote { to, .. }     => *to,
            // ...
        }
    }
}
```

---

## 4.7 — Multiple `impl` Blocks

Rust allows splitting methods across multiple `impl` blocks. The `Replica` in this
project effectively has one large block, but the pattern is useful for organisation:

```rust
impl Replica {
    // ===== Core consensus methods =====
    pub fn handle_vote(&mut self, vote: Vote) -> Option<QuorumCert> { ... }
    pub fn propose(&mut self, view: u64) -> Option<Block> { ... }
}

impl Replica {
    // ===== Pacemaker methods =====
    pub fn on_view_timeout(&mut self) { ... }
    pub fn is_view_timeout(&self) -> bool { ... }
}

impl Replica {
    // ===== Persistence methods =====
    pub fn attach_wal(&mut self, wal: WAL) { ... }
    pub fn take_snapshot(&mut self, data_dir: &Path) -> Result<(), String> { ... }
}
```

This is purely organisational — the compiler sees them all as one.

---

## 4.8 — Private vs. Public Methods

### From `src/replica.rs`:
```rust
// Public — can be called from outside the module
pub fn handle_vote(&mut self, vote: Vote) -> Option<QuorumCert> { ... }

// Private — only callable within this module (no `pub`)
fn try_form_qc(&mut self, block_hash: Hash, view: u64) -> Option<QuorumCert> { ... }
fn latest_block_hash(&self) -> Option<Hash> { ... }
fn get_block_at_view(&self, view: u64) -> Option<Block> { ... }
```

`try_form_qc` is an internal helper that `handle_vote` calls. Outside code should
not call it directly, so it's private. This is encapsulation — the public API is
clean and the implementation details are hidden.

---

## Exercises

1. `Config` has a `quorum_size()` method. Write an additional method
   `fn is_valid(&self) -> bool` that returns `true` if `n >= 3 * f + 1`
   (the BFT condition).

2. In `src/replica.rs`, find the `is_leader()` method. It calls
   `self.config.leader_for_view(view)`. Follow the call chain: what exactly
   does that compute?

3. What's the difference between `Replica::current_time_ms()` (associated function)
   and `replica.is_leader(view)` (method)? When would you use one style vs the other?

4. Why does `propose()` take `&mut self` but `is_leader()` takes `&self`?
   Identify which fields each method accesses to justify the choice.

---

Next lesson: [05 — Option & Result Types](05_option_and_result.md)
