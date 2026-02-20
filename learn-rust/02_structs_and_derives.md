# Lesson 2: Structs & Derive Macros

## Where this appears in the project
- `src/types.rs` — `Block`, `QuorumCert`, `Vote`
- `src/config.rs` — `Config`
- `src/crypto.rs` — `Signature`, `KeyStore`
- `src/network.rs` — `Network`

---

## 2.1 — Defining a Struct

A struct groups related data together, like a class in other languages (but with no
inheritance).

### From `src/config.rs`:
```rust
pub struct Config {
    pub n: usize,            // total number of replicas
    pub f: usize,            // max faulty replicas tolerated
    pub id: ReplicaId,       // this replica's own ID
    pub timeout_ms: u64,     // view timeout in milliseconds
}
```

- `pub` before the struct: it's visible outside this module
- `pub` before each field: the fields are also visible outside (not always wanted!)
- Each field has a name and a type, separated by `:`

---

## 2.2 — Creating (Instantiating) a Struct

### From `src/simulation.rs`:
```rust
let config = Config {
    n,                       // shorthand: field name matches variable name
    f,
    id: id as u64,           // explicit value when name doesn't match
    timeout_ms: 5000,
};
```

When the variable name matches the field name, Rust lets you write `n` instead of
`n: n`. This is called **field init shorthand**.

---

## 2.3 — Accessing Fields

Use dot notation:

```rust
let quorum = 2 * config.f + 1;
println!("Replica {} has timeout {}ms", config.id, config.timeout_ms);
```

---

## 2.4 — Structs with Complex Fields

Structs can contain other structs, `Option`, `Vec`, or any other type.

### From `src/types.rs`:
```rust
pub struct Block {
    pub hash: Hash,               // type alias for u64
    pub parent: Option<Hash>,     // might not have a parent (genesis block)
    pub view: u64,
    pub proposer: ReplicaId,
    pub qc: Option<QuorumCert>,   // might carry a QC, might not
}
```

- `Option<Hash>` means "either there is a hash, or there isn't" — no null pointers
- `Option<QuorumCert>` — the block optionally carries a quorum certificate
- The genesis block has `parent: None` and `qc: None`

### From `src/types.rs`:
```rust
pub struct QuorumCert {
    pub block_hash: u64,
    pub view: u64,
    pub signatures: Vec<Signature>,   // a growable list of signatures
}
```

`Vec<Signature>` is a dynamically-sized vector (like `ArrayList` in Java or `list` in
Python). It lives on the heap and can grow as needed.

---

## 2.5 — The `#[derive(...)]` Attribute

Derive macros auto-generate trait implementations at compile time. Instead of writing
boilerplate code by hand, you annotate the struct.

### From `src/types.rs`:
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub hash: Hash,
    pub parent: Option<Hash>,
    pub view: u64,
    pub proposer: ReplicaId,
    pub qc: Option<QuorumCert>,
}
```

What each derive does:

| Derive      | What it gives you                         | Example usage                        |
|-------------|-------------------------------------------|--------------------------------------|
| `Clone`     | `.clone()` — deep copy                    | `block.clone()`                      |
| `Debug`     | `{:?}` — debug printing                  | `println!("{:?}", block)`            |
| `PartialEq` | `==` and `!=` comparison                 | `if block1 == block2 { ... }`       |
| `Eq`        | Marks equality as reflexive (a == a)      | Required for use as `HashMap` key    |

### Why `Clone` is used everywhere in this project:
In the consensus protocol, blocks, QCs, and votes are frequently **sent to multiple
replicas**. Each replica gets its own copy. Without `Clone`, you'd have to manually
reconstruct each copy.

```rust
// From src/network.rs — broadcasting a proposal to all replicas
pub fn broadcast_proposal(&mut self, from: ReplicaId, block: Block) {
    for to in 0..self.num_replicas {
        if to as ReplicaId != from {
            self.send(Message::Proposal {
                from,
                to: to as ReplicaId,
                block: block.clone(),     // each message gets its own copy
            });
        }
    }
}
```

### Additional derives used in the project:

#### `Serialize` and `Deserialize` (from the `serde` crate):

```rust
// From src/wal.rs:
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum LogEntry {
    BlockInserted { hash: Hash, parent: Option<Hash>, ... },
    VoteReceived  { block_hash: Hash, view: u64, ... },
    // ...
}
```

These are NOT built into Rust — they come from the `serde` crate and let you convert
structs to/from bytes (bincode), JSON, TOML, etc.

---

## 2.6 — Tuple Structs & Unit Structs

Rust also has two other struct forms (less commonly used in this project):

```rust
// Tuple struct — fields accessed by index, not name
struct Point(f64, f64);
let p = Point(1.0, 2.0);
println!("x = {}", p.0);

// Unit struct — no fields at all, used as a marker/sentinel
struct Marker;
```

---

## 2.7 — Struct Instantiation with Spread (`..`)

If you have an existing struct and want to create a new one that differs in only a
few fields:

```rust
let config1 = Config { n: 4, f: 1, id: 0, timeout_ms: 5000 };
let config2 = Config { id: 1, ..config1 };
// config2 has n=4, f=1, id=1, timeout_ms=5000
```

The `..config1` fills in all remaining fields from `config1`.

---

## 2.8 — Helper / Constructor Functions

Rust has no special `constructor` keyword. Instead, you write a function (usually
called `new`) inside an `impl` block (covered in Lesson 4). But you can also write
standalone helper functions:

### From `src/types.rs`:
```rust
pub fn dummy_qc(hash: u64, view: u64) -> QuorumCert {
    QuorumCert {
        block_hash: hash,
        view,                    // field init shorthand
        signatures: vec![],      // vec![] creates an empty Vec
    }
}
```

`vec![]` is a macro that creates an empty `Vec`. You can also do `vec![1, 2, 3]` to
create a vector with initial elements.

---

## Exercises

1. Look at `src/crypto.rs`. The `Signature` struct has a field `bytes: [u8; 64]`.
   Why is this a fixed-size array instead of a `Vec<u8>`?
   (Hint: Ed25519 signatures are always exactly 64 bytes.)

2. The `Block` struct derives `Clone`. Find a place in `src/replica.rs` where
   `.clone()` is called on a block and explain why it's needed there.

3. Try adding `#[derive(Hash)]` to the `Config` struct. Does it compile? Why or why not?
   (Hint: check if all fields implement `Hash`.)

4. What happens if you remove `PartialEq` from Block's derive list? Which parts
   of the codebase break? (Look in the tests.)

---

Next lesson: [03 — Enums & Pattern Matching](03_enums_and_matching.md)
