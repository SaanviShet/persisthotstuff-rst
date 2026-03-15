# Lesson 8: Collections

## Where this appears in the project
- `Vec<T>` — everywhere (signatures, committed log, keystores)
- `BTreeMap<K, V>` — `src/replica.rs` (block_tree, vote_pool)
- `HashMap<K, V>` — `src/crypto.rs` (public_keys)
- `HashSet<T>` — `src/crypto.rs` (unique signer tracking)
- `VecDeque<T>` — `src/network.rs` (message queue)

---

## 8.1 — `Vec<T>` — Growable Array

The most common collection. Heap-allocated, contiguous, can grow.

### Creating:
```rust
let mut v: Vec<i32> = Vec::new();   // empty
let v = vec![1, 2, 3];              // macro with initial values
let v = vec![0; 10];                // 10 zeros
let v: Vec<Signature> = Vec::new(); // empty vec of Signatures
```

### From `src/types.rs`:
```rust
pub struct QuorumCert {
    pub block_hash: u64,
    pub view: u64,
    pub signatures: Vec<Signature>,    // variable number of signatures
}
```

### Common operations:

```rust
// Push (append)
// From src/replica.rs:
self.committed_log.push(block.clone());

// Check length
// From src/replica.rs:
self.committed_log.len()

// Check if empty
self.committed_log.is_empty()

// Iterate
// From src/replica.rs:
let committed_hashes: Vec<Hash> = self.committed_log.iter().map(|b| b.hash).collect();

// Check if any element matches
// From src/replica.rs:
if entry.iter().any(|s| s.signer == sig.signer) {
    return None;  // duplicate vote
}

// Find an element
// From src/replica.rs:
if self.committed_log.iter().any(|b| b.hash == b0.hash) {
    continue;  // already committed
}
```

---

## 8.2 — `BTreeMap<K, V>` — Ordered Map

A balanced tree map — keys are sorted. Lookup is O(log n).

### Why BTreeMap instead of HashMap?

The project uses `BTreeMap` for the block tree because:
1. Blocks have `Hash` (= `u64`) as keys, which is ordered
2. We often need to iterate in order (by hash or view)
3. `BTreeMap` provides deterministic iteration order

### From `src/replica.rs`:
```rust
pub struct Replica {
    pub block_tree: BTreeMap<Hash, Block>,     // hash → block (ordered by hash)
    pub vote_pool: BTreeMap<Hash, Vec<Signature>>,  // hash → collected votes
}
```

### Common operations:

```rust
// Insert
// From src/replica.rs:
self.block_tree.insert(hash, block.clone());

// Lookup
// From src/replica.rs:
if !self.block_tree.contains_key(&parent_hash) {
    return false;
}

// Get a reference to a value
if let Some(block) = self.block_tree.get(&hash) {
    println!("Found block at view {}", block.view);
}

// Iterate over all values
// From src/replica.rs:
for b0 in self.block_tree.values() {
    // b0 is &Block (a reference to each block)
}

// Find with a predicate on values
// From src/replica.rs:
let b1 = self.block_tree.values().find(|b| b.parent == Some(b0.hash));

// Get max by a key
// From src/replica.rs:
fn latest_block_hash(&self) -> Option<Hash> {
    self.block_tree.values().max_by_key(|b| b.view).map(|b| b.hash)
}

// The entry API — insert only if not present
// From src/replica.rs:
let entry = self.vote_pool.entry(vote.block_hash).or_insert_with(Vec::new);
entry.push(sig);
```

### The Entry API (important pattern):

```rust
self.vote_pool.entry(vote.block_hash).or_insert_with(Vec::new);
```

This is equivalent to:
```rust
if !self.vote_pool.contains_key(&vote.block_hash) {
    self.vote_pool.insert(vote.block_hash, Vec::new());
}
let entry = self.vote_pool.get_mut(&vote.block_hash).unwrap();
```

But the entry API does it in a single lookup, which is both faster and more idiomatic.

---

## 8.3 — `HashMap<K, V>` — Unordered Map

Like BTreeMap but uses hashing for O(1) average lookup. Keys must implement `Hash` + `Eq`.

### From `src/crypto.rs`:
```rust
pub struct KeyStore {
    my_id: ReplicaId,
    my_signing_key: SigningKey,
    pub public_keys: HashMap<ReplicaId, VerifyingKey>,   // replica ID → public key
}
```

### Building a HashMap:
```rust
// From src/crypto.rs:
let public_keys: HashMap<ReplicaId, VerifyingKey> = all_keys
    .iter()
    .map(|(&id, (_sk, vk))| (id, vk.clone()))   // transform to (key, value) tuples
    .collect();                                    // collect into HashMap
```

`.collect()` can build many collection types from an iterator. The target type is
inferred from the type annotation.

### Lookup:
```rust
// From src/crypto.rs:
let vk = match self.public_keys.get(&sig.signer) {
    Some(vk) => vk,
    None => return false,   // unknown signer
};
```

---

## 8.4 — `HashSet<T>` — Unordered Unique Set

Like HashMap but stores only keys (no values). Great for tracking unique elements.

### From `src/crypto.rs` — counting unique valid signers:
```rust
pub fn verify_qc(&self, qc: &QuorumCert, quorum: usize) -> bool {
    let mut valid_signers: HashSet<ReplicaId> = HashSet::new();
    for sig in &qc.signatures {
        if self.verify(sig, qc.block_hash, qc.view) {
            valid_signers.insert(sig.signer);   // duplicates are ignored
        }
    }
    valid_signers.len() >= quorum   // count unique valid signers
}
```

### Legacy version (simpler, no crypto verification):
```rust
pub fn verify_qc(qc: &QuorumCert, quorum: usize) -> bool {
    let unique: HashSet<ReplicaId> = qc.signatures
        .iter()
        .map(|s| s.signer)
        .collect();              // collect into HashSet automatically deduplicates
    unique.len() >= quorum
}
```

---

## 8.5 — `VecDeque<T>` — Double-Ended Queue

A ring buffer that supports efficient push/pop from both ends. Used as a FIFO queue.

### From `src/network.rs`:
```rust
use std::collections::VecDeque;

pub struct Network {
    message_queue: VecDeque<Message>,   // FIFO message queue
    // ...
}

impl Network {
    pub fn new(num_replicas: usize) -> Self {
        Network {
            message_queue: VecDeque::new(),
            // ...
        }
    }

    // Add to the back (enqueue)
    pub fn send(&mut self, msg: Message) {
        self.message_queue.push_back(msg);
    }

    // Remove from the front (dequeue)
    pub fn receive(&mut self) -> Option<Message> {
        self.message_queue.pop_front()   // returns None if empty
    }

    // Check if empty
    pub fn has_messages(&self) -> bool {
        !self.message_queue.is_empty()
    }

    // Count pending
    pub fn pending_count(&self) -> usize {
        self.message_queue.len()
    }

    // Clear all
    pub fn clear(&mut self) {
        self.message_queue.clear();
    }
}
```

Why VecDeque instead of Vec?
- `Vec::remove(0)` is O(n) — it shifts all elements left
- `VecDeque::pop_front()` is O(1) — it's a ring buffer
- For a message queue (FIFO), VecDeque is the right choice

---

## 8.6 — Fixed-Size Arrays `[T; N]`

Not a heap collection — lives on the stack, size known at compile time.

### From `src/network.rs`:
```rust
pub struct Network {
    pub messages_by_type: [usize; 4],   // exactly 4 counters
}
```

Access by index:
```rust
self.messages_by_type[idx] += 1;
```

### From `src/wal.rs`:
```rust
const WAL_MAGIC: [u8; 4] = [0x48, 0x53, 0x4C, 0x57];

// Reading into a fixed-size buffer:
let mut magic = [0u8; 4];           // 4 zeroed bytes on the stack
file.read_exact(&mut magic)?;       // fill the buffer from disk
if magic != WAL_MAGIC {
    return Err(WALError::InvalidMagic);
}
```

---

## 8.7 — Collection Comparison Table

| Collection       | Ordered? | Lookup   | Insert   | Use case                     |
|------------------|----------|----------|----------|------------------------------|
| `Vec<T>`         | By index | O(1)     | O(1)*    | Lists, sequences             |
| `BTreeMap<K,V>`  | By key   | O(log n) | O(log n) | Ordered key-value pairs      |
| `HashMap<K,V>`   | No       | O(1) avg | O(1) avg | Fast key-value lookup        |
| `HashSet<T>`     | No       | O(1) avg | O(1) avg | Unique elements, membership  |
| `VecDeque<T>`    | By index | O(1)     | O(1)**   | FIFO queues                  |
| `[T; N]`         | By index | O(1)     | N/A      | Fixed-size, stack-allocated  |

\* amortised — occasional reallocation  
\** at front or back

---

## 8.8 — `vec![]` Macro

The `vec![]` macro is the most common way to create vectors:

```rust
let empty: Vec<i32> = vec![];           // empty
let numbers = vec![1, 2, 3];           // with elements
let zeros = vec![0u8; 64];             // 64 zeros

// From src/types.rs:
pub fn dummy_qc(hash: u64, view: u64) -> QuorumCert {
    QuorumCert {
        block_hash: hash,
        view,
        signatures: vec![],   // empty Vec<Signature>
    }
}
```

---

## Exercises

1. The block tree uses `BTreeMap<Hash, Block>`. What would change if you replaced it
   with `HashMap<Hash, Block>`? Would the protocol still work correctly? (Hint: think
   about iteration order in `find_committed_block()`.)

2. In `src/network.rs`, the message queue is `VecDeque<Message>`. If you replaced it
   with `Vec<Message>` and used `remove(0)` for receive, what performance problem
   would you get with 1000 pending messages?

3. Look at the entry API usage in `handle_vote()`:
   `self.vote_pool.entry(vote.block_hash).or_insert_with(Vec::new)`
   Rewrite this using `if/else` and `contains_key/insert/get_mut`. Which is better?

4. In `verify_qc()`, why use `HashSet` instead of just counting the signatures?
   (Hint: what if the same signer submitted two signatures?)

---

Next lesson: [09 — Traits & Trait Implementations](09_traits.md)
