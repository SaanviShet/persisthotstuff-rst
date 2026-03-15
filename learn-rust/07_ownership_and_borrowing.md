# Lesson 7: Ownership, Borrowing & References

## Where this appears in the project
This is everywhere — it's the core idea that makes Rust different from every other
language. Every function signature in the project embodies these rules.

---

## 7.1 — The Three Rules of Ownership

1. Every value in Rust has exactly ONE owner (a variable).
2. When the owner goes out of scope, the value is dropped (freed).
3. You can transfer ownership (move) to another variable.

```rust
let s1 = String::from("hello");
let s2 = s1;              // ownership MOVES from s1 to s2
// println!("{}", s1);    // COMPILE ERROR: s1 no longer owns the string
println!("{}", s2);       // fine — s2 is the owner now
```

### Why this matters for the project:
```rust
// From src/replica.rs — execute_and_commit:
pub fn execute_and_commit(&mut self, block: Block) {
    // block is MOVED into this function
    self.committed_log.push(block.clone());
    // We clone because push takes ownership, and we need block again below
    self.committed_up_to = Some(block.hash);
}
```

---

## 7.2 — Borrowing with `&` (Immutable References)

Instead of giving ownership away, you can **lend** a value. The borrower can look
but not touch.

```rust
fn print_quorum(config: &Config) {        // borrows Config
    println!("Quorum: {}", config.quorum_size());
}   // config reference expires, but the original Config is untouched

let config = Config { n: 4, f: 1, id: 0, timeout_ms: 5000 };
print_quorum(&config);       // lend it
print_quorum(&config);       // can lend it again — still owned by us
```

### From `src/crypto.rs` — verify borrows the signature:
```rust
pub fn verify(&self, sig: &Signature, block_hash: u64, view: u64) -> bool {
    // sig is borrowed — we only read it, never modify or consume it
    let vk = match self.public_keys.get(&sig.signer) {
        Some(vk) => vk,
        None => return false,
    };
    // ...
}
```

`&self` — borrows the KeyStore
`sig: &Signature` — borrows the Signature
Neither is consumed — the caller still owns them after the function returns.

---

## 7.3 — Mutable Borrowing with `&mut`

A mutable reference lets you modify the borrowed value. But there's a strict rule:
**at any point, you can have EITHER one `&mut` OR any number of `&` — never both.**

### From `src/replica.rs`:
```rust
pub fn handle_vote(&mut self, vote: Vote) -> Option<QuorumCert> {
    // &mut self — we will modify self.vote_pool
    // vote: Vote — this is MOVED in (owned), not borrowed
    
    let entry = self.vote_pool.entry(vote.block_hash).or_insert_with(Vec::new);
    entry.push(sig);    // modifying the vote pool
    
    self.try_form_qc(vote.block_hash, vote.view)
}
```

### From `src/wal.rs`:
```rust
pub fn append(&mut self, entry: &LogEntry) -> Result<(), WALError> {
    // &mut self — we modify self.file (write to it) and self.entry_count
    // &LogEntry — we only READ the entry to serialize it
    
    let payload: Vec<u8> = bincode::serialize(entry)?;
    // ...
    self.entry_count += 1;
    Ok(())
}
```

Notice the pattern: `&mut self` (we modify the WAL) but `&LogEntry` (we only read
the entry). This is intentional — the caller keeps ownership of the LogEntry.

---

## 7.4 — Clone: Making Explicit Copies

When you need to give ownership to multiple places, use `.clone()`:

### From `src/network.rs`:
```rust
pub fn broadcast_proposal(&mut self, from: ReplicaId, block: Block) {
    for to in 0..self.num_replicas {
        if to as ReplicaId != from {
            self.send(Message::Proposal {
                from,
                to: to as ReplicaId,
                block: block.clone(),      // each message gets its OWN copy
            });
        }
    }
}
```

We send the block to (n-1) replicas. Each message needs its own `Block` because
`Message` owns its fields. So we clone the block for each message.

### From `src/crypto.rs` — distributing keys:
```rust
pub fn distribute_keys(all_keys: &HashMap<ReplicaId, (SigningKey, VerifyingKey)>) -> Vec<KeyStore> {
    let public_keys: HashMap<ReplicaId, VerifyingKey> = all_keys
        .iter()
        .map(|(&id, (_sk, vk))| (id, vk.clone()))     // clone each VerifyingKey
        .collect();

    for (&replica_id, (signing_key, _)) in all_keys.iter() {
        let keystore = KeyStore::new_for_replica(
            replica_id,
            signing_key.clone(),          // each replica gets its own copy
            public_keys.clone(),          // each replica gets ALL public keys
        );
        keystores.push(keystore);
    }
    keystores
}
```

Every replica needs its own `SigningKey` and a full copy of `public_keys`. Since
ownership can't be shared, we clone.

---

## 7.5 — `ref` and `ref mut` in Pattern Matching

When destructuring in a `match` or `if let`, `ref` borrows instead of moving:

### From `src/replica.rs`:
```rust
if let Some(ref qc) = block.qc {
    if !self.keystore.verify_qc(qc, self.config.quorum_size()) {
        return false;
    }
}
```

`ref qc` borrows the QuorumCert inside the Option. Without `ref`, the QC would be
**moved out** of `block.qc`, partially invalidating `block`.

### Mutable borrow in pattern:
```rust
if let Some(ref mut wal) = self.wal {
    let _ = wal.append(&LogEntry::QCFormed { ... });
}
```

`ref mut wal` borrows the WAL mutably so we can call `append()` (which needs
`&mut self`).

---

## 7.6 — The Borrow Checker in Action

The compiler enforces these rules at compile time. Here are common scenarios from
the project:

### Can't use after move:
```rust
let block = Block { hash: 1, parent: None, view: 1, proposer: 0, qc: None };
self.committed_log.push(block);      // block is MOVED into the Vec
// println!("{}", block.hash);       // COMPILE ERROR: block was moved
```

Solution: clone before moving:
```rust
self.committed_log.push(block.clone());
self.committed_up_to = Some(block.hash);  // fine — block is still here
```

### Can't mutably borrow while immutably borrowed:
```rust
let qc = &self.high_qc;           // immutable borrow of self
self.vote_pool.clear();            // ERROR: needs &mut self, but self is borrowed
```

Solution: restructure to not overlap borrows:
```rust
let has_qc = self.high_qc.is_some();   // borrow ends here
self.vote_pool.clear();                 // now we can mutate
```

---

## 7.7 — Copy vs. Clone

Some types are `Copy` — they are automatically duplicated without `.clone()`.
These are small stack-allocated types:

| Copy types          | Non-Copy (must clone)    |
|---------------------|--------------------------|
| `u8, u16, u32, u64` | `String`                 |
| `i32, f64, bool`    | `Vec<T>`                 |
| `char`              | `HashMap<K,V>`           |
| `(u32, u64)` tuples | `Block`, `QuorumCert`    |
| `&T` (references)   | `File`                   |

### From `src/network.rs`:
```rust
pub fn sender(&self) -> ReplicaId {
    match self {
        Message::Proposal { from, .. } => *from,   // *from dereferences &u64 → u64
        // u64 is Copy, so this "copy" is free
    }
}
```

`*from` — since `from` is `&ReplicaId` (borrowed from the enum), we dereference it.
Because `ReplicaId` (= `u64`) is `Copy`, this just copies the 8 bytes.

---

## 7.8 — String and &str

A quick note on Rust's two string types (used in error messages throughout):

```rust
let s: String = String::from("hello");   // owned, heap-allocated, growable
let r: &str = "hello";                    // borrowed string slice, usually a literal

// &str → String:
let owned: String = "hello".to_string();
let owned: String = String::from("hello");

// String → &str:
let borrowed: &str = &owned;              // automatic deref coercion
```

### From `src/network.rs`:
```rust
pub fn msg_type(&self) -> &str {    // returns a borrowed string slice
    match self {
        Message::Proposal { .. } => "Proposal",   // string literals are &str
        Message::Vote { .. }     => "Vote",
    }
}
```

The function returns `&str` — it doesn't allocate a new String, it just points to
a string literal embedded in the binary.

---

## Exercises

1. In `src/replica.rs`, find `execute_and_commit()`. Why does it call `block.clone()`
   before pushing to `committed_log`? What would happen without the clone?

2. Look at `broadcast_proposal()` in `src/network.rs`. The last iteration doesn't
   need to clone (it could move the original). How would you optimise this?
   (Hint: use `into_iter` / `enumerate` / treat the last one specially.)

3. In `src/crypto.rs`, `verify()` takes `sig: &Signature`. Could it take
   `sig: Signature` (by value) instead? What would the trade-off be?

4. Find three methods in `src/replica.rs` that take `&self` and three that take
   `&mut self`. For each one, explain why that specific borrow type is needed.

---

Next lesson: [08 — Collections](08_collections.md)
