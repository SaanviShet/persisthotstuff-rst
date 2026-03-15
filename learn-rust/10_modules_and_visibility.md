# Lesson 10: Modules & Visibility

## Where this appears in the project
- `src/lib.rs` — the module registry
- `src/*.rs` — one file per module
- `use crate::types::*;` — imports throughout
- `pub` / private visibility on structs, fields, and functions

---

## 10.1 — How Rust Organises Code

Rust uses a **module system** instead of header files (C) or packages (Java/Python).
Every `.rs` file is a module. The module tree is declared in `lib.rs` (for libraries)
or `main.rs` (for binaries).

### From `src/lib.rs` (the entire file):
```rust
pub mod config;
pub mod crypto;
pub mod types;
pub mod wal;
pub mod snapshot;
pub mod recovery;
pub mod replica;
pub mod visualiser;
pub mod network;
pub mod simulation;
```

Each line says: "there is a module named X, and its code lives in `src/X.rs`."
The `pub` makes each module accessible from outside the crate.

---

## 10.2 — Using Items from Other Modules

### `use` statements bring items into scope:

```rust
// From src/replica.rs:
use std::collections::BTreeMap;           // from the standard library
use crate::types::*;                      // everything from our types module
use crate::config::*;                     // everything from our config module
use crate::wal::{WAL, LogEntry, ViewChangeReason};  // specific items
use crate::snapshot::Snapshot;            // one specific item
use std::path::Path;                      // standard library path type
```

### Import styles:

| Syntax                          | What it imports                                    |
|---------------------------------|----------------------------------------------------|
| `use crate::types::*;`         | Everything public from the `types` module           |
| `use crate::types::Block;`     | Just `Block`                                        |
| `use crate::types::{Block, Hash};` | `Block` and `Hash`                             |
| `use std::collections::BTreeMap;`  | `BTreeMap` from the standard library            |
| `use std::io::{self, Read, Write};` | `io` module itself + `Read` and `Write` traits |

### `crate::` vs `std::` vs external crates:

```rust
use crate::types::Block;       // crate:: = this project's own modules
use std::collections::BTreeMap; // std:: = Rust standard library
use serde::{Serialize, Deserialize}; // serde:: = external crate (from Cargo.toml)
use sha2::{Sha256, Digest};    // sha2:: = external crate
```

---

## 10.3 — Visibility Rules (`pub`)

By default, everything in Rust is PRIVATE. You opt in to visibility.

### Module level:
```rust
// In lib.rs:
pub mod config;      // public — other crates can access it
mod secret_module;   // private — only this crate can access it
```

### Struct level:
```rust
// All fields public — anyone can read/write them
pub struct Config {
    pub n: usize,
    pub f: usize,
    pub id: ReplicaId,
    pub timeout_ms: u64,
}

// Private fields — controlled access
pub struct WAL {
    file: File,           // PRIVATE — can't be accessed outside wal.rs
    path: PathBuf,        // PRIVATE
    replica_id: ReplicaId,// PRIVATE
    entry_count: u64,     // PRIVATE
}
```

`WAL` is a `pub` struct (visible outside), but its fields are private. This means
outside code can hold a `WAL` value but can't directly access `wal.file`. They must
use methods like `wal.append()`.

### Function level:
```rust
impl Replica {
    pub fn handle_vote(&mut self, vote: Vote) -> Option<QuorumCert> { ... }
    // ↑ public — part of the API

    fn try_form_qc(&mut self, ...) -> Option<QuorumCert> { ... }
    // ↑ private — internal helper, no pub keyword
}
```

### Why it matters:
```rust
// In tests/some_test.rs (outside the module):
let mut replica = Replica { ... };
replica.handle_vote(vote);      // fine — pub method
// replica.try_form_qc(hash, view);  // COMPILE ERROR — private method
```

---

## 10.4 — The `use super::*` Pattern (Tests)

Inside a `#[cfg(test)] mod tests` block, you need to access the parent module's
items. `super` means "the module that contains this one."

### From `src/network.rs`:
```rust
// Main module code above...

#[cfg(test)]
mod tests {
    use super::*;                    // import everything from network module
    use crate::crypto::sign;         // also need the sign helper

    #[test]
    fn test_network_creation() {
        let network = Network::new(4);
        assert_eq!(network.num_replicas, 4);
    }
}
```

`super::*` imports `Network`, `Message`, `Hash`, etc. — everything that was in scope
in the parent module, including private items. This is why tests inside the same file
can access private fields and methods.

---

## 10.5 — Cross-Module Imports in the Project

Let's trace how `Block` flows through the codebase:

```
1. Defined in src/types.rs:      pub struct Block { ... }
2. Used in src/replica.rs:       use crate::types::*;
3. Used in src/network.rs:       use crate::types::*;
4. Used in src/wal.rs:           use crate::types::{Hash, Block, QuorumCert, Vote};
5. Used in src/simulation.rs:    use crate::types::*;
```

The `*` (glob import) brings in ALL public items from the module. The explicit
`{Hash, Block, ...}` form only brings in what's listed — it's more precise but more
verbose.

---

## 10.6 — Re-exports and Module Organisation

A module can re-export items from sub-modules:

```rust
// Not used in this project directly, but a common pattern:
// In lib.rs:
pub mod types;
pub use types::{Block, QuorumCert, Vote, Hash};  // re-export at crate root
```

Now external users can write:
```rust
use persisthotstuff_rst::Block;
// instead of:
use persisthotstuff_rst::types::Block;
```

---

## 10.7 — Doc Comments (`///` and `//!`)

Rust has two kinds of documentation comments:

### `//!` — module-level documentation:
```rust
// From src/wal.rs:
//! Write-Ahead Logging (WAL) module for the consensus protocol.
//!
//! This module implements an **append-only** log with CRC32 checksums.
//! Every state change is logged to disk **before** it is applied to
//! in-memory state.
```

This describes the MODULE itself. It appears at the top of the file.

### `///` — item-level documentation:
```rust
// From src/config.rs:
/// Calculate the quorum size (2f + 1).
///
/// # Arguments
/// (none)
///
/// # Returns
/// The minimum number of replicas needed for agreement
pub fn quorum_size(&self) -> usize {
    2 * self.f + 1
}
```

This describes a specific function, struct, or enum. The `# Arguments` and
`# Returns` sections are Rust conventions for API docs.

Generate documentation with:
```bash
cargo doc --open
```

The project already has generated docs in `target/doc/`.

---

## 10.8 — Conditional Compilation: `#[cfg(test)]`

The `#[cfg(test)]` attribute tells the compiler "only include this code when running
`cargo test`". It's not compiled into the release binary.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // all test functions here are only compiled during testing
}
```

This is different from `#[test]` which marks individual test functions:
```rust
#[test]
fn test_something() {
    assert_eq!(2 + 2, 4);
}
```

---

## Exercises

1. Look at `src/lib.rs`. What would happen if you removed `pub mod wal;`? Which other
   modules would fail to compile? (Hint: check who does `use crate::wal::...`)

2. The `WAL` struct has private fields but `Config` has all public fields. What's the
   design rationale? When would you make fields private?

3. Why does `src/network.rs` test module use `use super::*` but also imports
   `use crate::crypto::sign`? Isn't `sign` available through `super::*`?

4. Try running `cargo doc --open` and explore the generated documentation. Find the
   docs for `WAL::append()`. How do the `///` comments translate to HTML?

---

Next lesson: [11 — File I/O & Paths](11_file_io.md)
