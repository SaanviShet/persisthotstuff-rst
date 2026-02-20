# Lesson 9: Traits & Trait Implementations

## Where this appears in the project
- `#[derive(Clone, Debug, PartialEq, Eq)]` — derived traits on all data types
- `#[derive(Serialize, Deserialize)]` — serde traits on WAL/snapshot types
- `impl From<io::Error> for WALError` — manual trait implementation
- `impl std::fmt::Display for WALError` — manual trait implementation

---

## 9.1 — What is a Trait?

A trait defines shared behavior — like an interface in Java or a protocol in Swift.
It declares a set of methods that a type must implement.

```rust
// Built-in trait (simplified):
trait Clone {
    fn clone(&self) -> Self;
}

trait Debug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result;
}

trait Display {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result;
}
```

---

## 9.2 — Derive Macros: Auto-Implementing Traits

When you write `#[derive(Clone)]`, the compiler generates the `Clone` implementation
automatically. This only works if ALL fields also implement `Clone`.

### From `src/types.rs`:
```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub hash: Hash,               // u64: implements Clone + Debug + PartialEq + Eq
    pub parent: Option<Hash>,     // Option<u64>: same
    pub view: u64,                // same
    pub proposer: ReplicaId,      // u64: same
    pub qc: Option<QuorumCert>,   // QuorumCert must also derive these traits
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuorumCert {
    pub block_hash: u64,
    pub view: u64,
    pub signatures: Vec<Signature>,   // Signature must also derive these traits
}
```

The chain: `Block` derives `Clone` → needs `QuorumCert: Clone` → needs
`Vec<Signature>: Clone` → needs `Signature: Clone`. If ANY type in the chain
doesn't implement `Clone`, the derive fails.

### Common derivable traits:

| Trait       | What it gives you            | Generated code does what?           |
|-------------|------------------------------|--------------------------------------|
| `Clone`     | `.clone()` method            | Deep copy every field                |
| `Debug`     | `{:?}` formatting            | Print struct name + all fields       |
| `PartialEq` | `==` and `!=`               | Compare every field                  |
| `Eq`        | Marker for total equality    | (no new code, just a promise)        |
| `Hash`      | Can be used as HashMap key   | Hash every field                     |
| `Default`   | `Type::default()`           | Use default value for each field     |
| `Copy`      | Implicit copy on assignment  | Only for small stack types           |

---

## 9.3 — Serde's Derive Macros: Serialize & Deserialize

The `serde` crate provides traits for converting types to/from bytes, JSON, etc.
These are **not** built into Rust — they come from an external crate.

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
        hash: Hash,
        parent: Option<Hash>,
        view: u64,
        proposer: ReplicaId,
        qc_block_hash: Option<Hash>,
        qc_view: Option<u64>,
        timestamp: u128,
    },
    // ... more variants
}
```

With `Serialize` derived, you can do:
```rust
let payload: Vec<u8> = bincode::serialize(entry)?;    // struct → bytes
```

With `Deserialize` derived:
```rust
let entry: LogEntry = bincode::deserialize(&payload)?;  // bytes → struct
```

The derive macro generates all the serialization code at compile time.

---

## 9.4 — Manual Trait Implementation: `From`

When you need custom logic, you implement the trait by hand.

### From `src/wal.rs` — converting io::Error into WALError:
```rust
impl From<io::Error> for WALError {
    fn from(e: io::Error) -> Self {
        WALError::Io(e)    // wrap the io::Error inside our enum variant
    }
}
```

The syntax:
```
impl TraitName for TypeName {
    fn method_name(params) -> ReturnType {
        // implementation
    }
}
```

This `From` implementation is what makes `?` work (see Lesson 6):
```rust
file.write_all(&data)?;
// If write_all returns Err(io::Error), the ? operator calls
// WALError::from(io_error), which triggers our From impl.
```

---

## 9.5 — Manual Trait Implementation: `Display`

The `Display` trait controls how a type is printed with `{}` (the "user-facing" format).

### From `src/wal.rs`:
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

Now you can do:
```rust
println!("Error: {}", wal_error);     // uses Display
println!("Debug: {:?}", wal_error);   // uses Debug (derived)
```

Key difference:
- `Debug` (`{:?}`) — for developers, shows internal structure
- `Display` (`{}`) — for users, shows a clean message

---

## 9.6 — Trait Bounds: "This Type Must Implement..."

When you write a generic function, you can require that the type implements certain
traits. This is called a **trait bound**.

```rust
// Not from this project, but illustrates the concept:
fn print_if_equal<T: PartialEq + Debug>(a: &T, b: &T) {
    if a == b {                    // needs PartialEq
        println!("{:?}", a);       // needs Debug
    }
}
```

### Where you see this implicitly in the project:

When you use `BTreeMap<Hash, Block>`, the key type `Hash` (= `u64`) must implement
`Ord` (ordered comparison). If you tried `BTreeMap<Vec<u8>, Block>`, it would work
because `Vec<u8>` implements `Ord`. But if you tried `BTreeMap<f64, Block>`, it would
fail because `f64` does NOT implement `Ord` (due to `NaN`).

When you use `HashMap<ReplicaId, VerifyingKey>`, the key `ReplicaId` (= `u64`) must
implement `Hash + Eq`.

---

## 9.7 — Traits in Iterators

Many iterator methods take closures (covered in Lesson 14) but also rely on traits:

### From `src/replica.rs`:
```rust
// max_by_key needs Ord on the key type (u64 view numbers are Ord)
self.block_tree.values().max_by_key(|b| b.view).map(|b| b.hash)

// find needs the closure to return bool (FnMut trait)
self.block_tree.values().find(|b| b.parent == Some(b0.hash))

// collect needs FromIterator trait on the target type
let committed_hashes: Vec<Hash> = self.committed_log.iter().map(|b| b.hash).collect();
```

---

## 9.8 — Trait Objects (Brief Introduction)

Sometimes you want to store "any type that implements Trait X." This uses `dyn Trait`:

```rust
// Not directly used in this project, but good to know:
fn print_error(e: &dyn std::fmt::Display) {
    println!("Error: {}", e);
}

// Can pass a WALError, SnapshotError, String, etc.
print_error(&wal_error);
print_error(&snapshot_error);
print_error(&"simple string error");
```

`Box<dyn Error>` is a common pattern for "any error type" — the project doesn't
use it (it uses specific error enums instead), but many Rust libraries do.

---

## Exercises

1. The `Config` struct derives only `Clone`. Try adding `#[derive(Debug)]` to it
   and using `println!("{:?}", config)`. What output do you get?

2. Why can't `WAL` derive `Clone`? Look at its fields — which one is the problem?
   (Hint: `File` does not implement `Clone` because you can't duplicate a file
   descriptor just by copying bytes.)

3. Implement `Display` for the `Message` enum in `src/network.rs`. Each variant
   should print something like "Proposal from R0 to R1" or "Vote from R2 to R0".

4. The project uses `From<io::Error> for WALError` and a separate
   `From<io::Error> for SnapshotError`. Could you combine them into a single
   generic implementation? Why or why not? (Hint: Rust's orphan rule.)

---

Next lesson: [10 — Modules & Visibility](10_modules_and_visibility.md)
