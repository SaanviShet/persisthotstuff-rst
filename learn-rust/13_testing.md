# Lesson 13: Testing in Rust

## Where this appears in the project
- `src/network.rs` — inline tests (`#[cfg(test)] mod tests`)
- `src/wal.rs` — inline tests for WAL operations
- `src/snapshot.rs` — inline tests for snapshot save/load
- `tests/commit_rule.rs` — integration tests (3-chain rule)
- `tests/qc_test.rs` — integration tests (QC formation)
- `tests/pacemaker.rs` — integration tests (view management)

---

## 13.1 — Two Kinds of Tests

### Unit tests (inside `src/`):
```
src/network.rs   → tests the Network struct from inside the module
src/wal.rs       → tests WAL operations with access to private internals
```

### Integration tests (inside `tests/`):
```
tests/commit_rule.rs   → tests the commit rule through the public API
tests/qc_test.rs       → tests QC formation through the public API
```

Key difference: unit tests can access private fields and methods (via `super::*`),
integration tests can only use the public API.

---

## 13.2 — Writing a Unit Test

### From `src/network.rs`:
```rust
#[cfg(test)]                        // only compile when testing
mod tests {                         // test sub-module
    use super::*;                   // import everything from parent module
    use crate::crypto::sign;        // import helpers from other modules

    #[test]                         // marks this function as a test
    fn test_network_creation() {
        let network = Network::new(4);
        assert_eq!(network.num_replicas, 4);   // check equality
        assert!(!network.has_messages());       // check boolean
    }

    #[test]
    fn test_send_receive() {
        let mut network = Network::new(4);
        let vote = Vote {
            block_hash: 1,
            view: 1,
            signature: sign(0),                 // dummy signature for testing
        };

        network.send_vote(0, 1, vote.clone());
        assert!(network.has_messages());
        assert_eq!(network.pending_count(), 1);

        let msg = network.receive();
        assert!(msg.is_some());                 // got a message
        assert!(!network.has_messages());        // queue is now empty
    }
}
```

Anatomy:
1. `#[cfg(test)]` — conditional compilation, only included in test builds
2. `mod tests` — a child module (can access private items via `super`)
3. `#[test]` — tells `cargo test` this function is a test
4. `assert!`, `assert_eq!`, `assert_ne!` — assertion macros

---

## 13.3 — Writing an Integration Test

Integration tests live in the `tests/` directory. Each `.rs` file is compiled as
a separate crate that depends on your library.

### From `tests/qc_test.rs`:
```rust
use persisthotstuff_rst::crypto::*;      // import from the crate's public API
use persisthotstuff_rst::types::*;
use persisthotstuff_rst::config::*;

#[test]
fn quorum_cert_forms_correctly() {
    let f = 1;
    let config = Config { n: 4, f, id: 0, timeout_ms: 5000 };

    let mut sigs = vec![];
    for id in 0..config.quorum_size() {
        sigs.push(sign(id as u64));
    }

    let qc = QuorumCert {
        block_hash: 42,
        view: 1,
        signatures: sigs,
    };

    assert!(
        qc.signatures.len() >= config.quorum_size(),
        "QC doesn't have enough signatures"    // custom failure message
    );
}
```

Notice: `use persisthotstuff_rst::crypto::*` — integration tests use the full
crate name, not `use crate::crypto::*`.

---

## 13.4 — Assertion Macros

```rust
// Check that something is true
assert!(condition);
assert!(condition, "Custom failure message");
assert!(condition, "Failed with value: {}", some_var);

// Check equality
assert_eq!(left, right);
assert_eq!(left, right, "Expected {} but got {}", left, right);

// Check inequality
assert_ne!(x, y);
assert_ne!(x, y, "Values should not be equal");
```

### From `tests/commit_rule.rs`:
```rust
let committed = replica.find_committed_block();
assert!(committed.is_some(), "Should find committed block in 3-chain");
assert_eq!(committed.unwrap().hash, 0, "B0 should be committed");
```

---

## 13.5 — Test Setup: Building State

Tests often need complex setup. In this project, creating a replica with keys and
blocks is the most common setup pattern:

### From `tests/commit_rule.rs`:
```rust
#[test]
fn three_chain_commits_block() {
    // 1. Create cryptographic keys
    let all_keys = KeyStore::generate_keys(4);
    let keystores = KeyStore::distribute_keys(&all_keys);

    // 2. Create a replica
    let config = Config { n: 4, f: 1, id: 0, timeout_ms: 5000 };
    let mut replica = Replica {
        config: config.clone(),
        current_view: 3,
        block_tree: BTreeMap::new(),
        high_qc: None,
        vote_pool: BTreeMap::new(),
        next_hash: 0,
        committed_log: Vec::new(),
        committed_up_to: None,
        timeout_ms: 5000,
        view_start_time: 0,
        keystore: keystores[0].clone(),
        wal: None,                         // no WAL needed for this test
        snapshot_counter: 0,
    };

    // 3. Build the block tree manually
    let b0 = Block { hash: 0, parent: None, view: 0, proposer: 0, qc: None };
    replica.block_tree.insert(b0.hash, b0.clone());

    let b1 = Block {
        hash: 1, parent: Some(0), view: 1, proposer: 1,
        qc: Some(QuorumCert {
            block_hash: 0, view: 0,
            signatures: vec![sign(0), sign(1), sign(2)],
        }),
    };
    replica.block_tree.insert(b1.hash, b1.clone());

    // 4. Assert the behavior
    let committed = replica.find_committed_block();
    assert!(committed.is_some());
}
```

Notice `wal: None` — tests that don't need persistence skip the WAL entirely.
This is why `wal` is `Option<WAL>`.

---

## 13.6 — Testing with `tempfile` — Isolated File System Tests

For tests that need real files (WAL, snapshot), use the `tempfile` crate to create
temporary directories that are automatically cleaned up:

```rust
use tempfile::TempDir;

#[test]
fn test_wal_create_and_append() {
    let tmp = TempDir::new().unwrap();          // creates /tmp/rustXXXXXX/
    let dir = tmp.path();                        // get the path

    let mut wal = WAL::create(0, dir).unwrap();  // creates file inside tmp dir
    wal.append(&LogEntry::BlockInserted { ... }).unwrap();

    let entries = wal.read_all().unwrap();
    assert_eq!(entries.len(), 1);
}
// When `tmp` goes out of scope, the directory is deleted automatically
```

Key points:
- `TempDir::new()` creates a unique temporary directory
- `tmp.path()` returns a `&Path` pointing to it
- When `tmp` is dropped (goes out of scope), the directory and all its contents
  are deleted — no manual cleanup needed
- This prevents tests from leaving junk files on disk
- Each test gets its own directory, so tests never conflict

---

## 13.7 — Helper Functions for Tests

### From `tests/qc_test.rs`:
```rust
fn is_committed(b0: &Block, b1: &Block, b2: &Block) -> bool {
    b1.parent == Some(b0.hash) &&
    b2.parent == Some(b1.hash) &&
    b1.qc.is_some() &&
    b2.qc.is_some()
}

#[test]
fn three_chain_commit_rule() {
    let b0 = Block { hash: 0, parent: None, view: 0, proposer: 0, qc: None };
    let b1 = Block { hash: 1, parent: Some(0), view: 1, proposer: 1, qc: Some(dummy_qc(0, 0)) };
    let b2 = Block { hash: 2, parent: Some(1), view: 2, proposer: 2, qc: Some(dummy_qc(1, 1)) };

    assert!(is_committed(&b0, &b1, &b2));
}
```

The `is_committed` helper keeps the test focused on the assertion, not the
mechanics of checking the 3-chain.

### From `src/types.rs` — test helper available everywhere:
```rust
pub fn dummy_qc(hash: u64, view: u64) -> QuorumCert {
    QuorumCert {
        block_hash: hash,
        view,
        signatures: vec![],    // no real signatures — just structural testing
    }
}
```

---

## 13.8 — Running Tests

```bash
# Run all tests
cargo test

# Run a specific test by name
cargo test three_chain_commits_block

# Run tests in a specific file
cargo test --test commit_rule

# Run tests with output visible (println! is hidden by default)
cargo test -- --nocapture

# Run only unit tests (in src/)
cargo test --lib

# Run only integration tests (in tests/)
cargo test --tests
```

---

## 13.9 — Test Organisation in This Project

```
tests/
  commit_rule.rs     ← 3-chain commit rule (4 tests)
  qc_test.rs         ← QC formation and verification (2 tests)
  qc_formation.rs    ← Detailed QC formation via vote pool
  pacemaker.rs       ← View changes and timeouts
  proposal_phase.rs  ← Block proposal and validation
  network_test.rs    ← Network message passing

src/
  network.rs         ← #[cfg(test)] mod tests { ... }   (3 tests)
  wal.rs             ← #[cfg(test)] mod tests { ... }   (several tests)
  snapshot.rs        ← #[cfg(test)] mod tests { ... }   (several tests)
  recovery.rs        ← #[cfg(test)] mod tests { ... }   (several tests)
```

The general rule:
- Put tests close to the code they test (inline) when they need private access
- Put integration tests in `tests/` when they test public API behavior

---

## Exercises

1. Run `cargo test` and observe the output. How many tests pass? Are there any
   failures?

2. Write a new test in `tests/qc_test.rs` that verifies `dummy_qc(42, 1)` returns
   a QC with `block_hash == 42`, `view == 1`, and an empty signatures vector.

3. Add a test to `src/network.rs`'s test module that broadcasts a proposal from
   replica 0 in a 4-replica network and verifies that 3 messages are enqueued.

4. Why do WAL tests use `tempfile::TempDir` instead of hardcoded paths like
   `"/tmp/test_wal"`? What problem would hardcoded paths cause if two developers
   ran tests simultaneously?

---

Next lesson: [14 — Closures & Iterators](14_closures_and_iterators.md)
