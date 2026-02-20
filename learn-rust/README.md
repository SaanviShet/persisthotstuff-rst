# Learn Rust — From Your PersistHotStuff Codebase

A hands-on Rust course using real code from this project. Every concept is
illustrated with actual snippets from `src/`, `tests/`, and `examples/` — no
toy examples.

---

## Lessons

| #  | Topic                           | Key project files used                     |
|----|---------------------------------|--------------------------------------------|
| 01 | [Variables, Types & Aliases](01_variables_and_types.md)     | `types.rs`, `config.rs`, `wal.rs`          |
| 02 | [Structs & Derive Macros](02_structs_and_derives.md)        | `types.rs`, `config.rs`, `crypto.rs`       |
| 03 | [Enums & Pattern Matching](03_enums_and_matching.md)        | `network.rs`, `wal.rs`, `snapshot.rs`      |
| 04 | [Impl Blocks & Methods](04_impl_blocks_and_methods.md)      | `config.rs`, `replica.rs`, `wal.rs`        |
| 05 | [Option & Result Types](05_option_and_result.md)            | `replica.rs`, `wal.rs`, `types.rs`         |
| 06 | [Error Handling & `?`](06_error_handling.md)                | `wal.rs`, `snapshot.rs`, `recovery.rs`     |
| 07 | [Ownership, Borrowing & Refs](07_ownership_and_borrowing.md)| `replica.rs`, `network.rs`, `crypto.rs`    |
| 08 | [Collections](08_collections.md)                            | `replica.rs`, `crypto.rs`, `network.rs`    |
| 09 | [Traits & Trait Impls](09_traits.md)                        | `wal.rs`, `types.rs`, `snapshot.rs`        |
| 10 | [Modules & Visibility](10_modules_and_visibility.md)        | `lib.rs`, `replica.rs`, `network.rs`       |
| 11 | [File I/O & Paths](11_file_io.md)                           | `wal.rs`, `snapshot.rs`                    |
| 12 | [Serialization (Serde)](12_serialization.md)                | `wal.rs`, `snapshot.rs`, `Cargo.toml`      |
| 13 | [Testing in Rust](13_testing.md)                            | `tests/*`, `network.rs`, `wal.rs`          |
| 14 | [Closures & Iterators](14_closures_and_iterators.md)        | `replica.rs`, `crypto.rs`                  |

---

## How to Use These Lessons

1. **Read the lesson** — each one is self-contained (~5-10 min).
2. **Open the referenced source files** side-by-side and find the snippets
   in their full context.
3. **Do the exercises** at the end of each lesson — they point you to specific
   parts of the codebase to explore.
4. **Run the code** — `cargo test` runs the tests, `cargo run --example normal_case`
   runs examples.

---

## Suggested Learning Order

### Beginner (start here if new to Rust):
1. Variables & Types → 2. Structs → 3. Enums → 4. Impl Blocks → 5. Option & Result

### Intermediate:
6. Error Handling → 7. Ownership & Borrowing → 8. Collections → 9. Traits

### Project-Specific:
10. Modules → 11. File I/O → 12. Serialization → 13. Testing → 14. Iterators

---

## Quick Reference: Where Each Concept Lives

| Rust Concept           | Best example in codebase                            |
|------------------------|-----------------------------------------------------|
| Type aliases           | `src/types.rs` → `type Hash = u64`                  |
| Structs with Options   | `src/types.rs` → `Block { parent: Option<Hash> }`   |
| Complex enums          | `src/wal.rs` → `LogEntry` (7 variants)              |
| Pattern matching       | `src/network.rs` → `Message::sender()`              |
| `impl` blocks          | `src/config.rs` → `impl Config`                     |
| `&self` vs `&mut self` | `src/replica.rs` → read vs write methods             |
| Error enum + `From`    | `src/wal.rs` → `WALError` + `From<io::Error>`       |
| The `?` operator       | `src/wal.rs` → `WAL::create()`                      |
| `BTreeMap`             | `src/replica.rs` → `block_tree`                     |
| `HashMap`              | `src/crypto.rs` → `public_keys`                     |
| `VecDeque`             | `src/network.rs` → `message_queue`                   |
| `HashSet`              | `src/crypto.rs` → `verify_qc()`                     |
| Serde + bincode        | `src/wal.rs` → `LogEntry` serialization              |
| File I/O + fsync       | `src/wal.rs` → `WAL::append()`                      |
| SHA-256 integrity      | `src/snapshot.rs` → snapshot save/load               |
| Unit tests             | `src/network.rs` → `#[cfg(test)] mod tests`         |
| Integration tests      | `tests/commit_rule.rs`                               |
| `tempfile` in tests    | `src/wal.rs` tests                                   |
| Iterator chains        | `src/replica.rs` → `find_committed_block()`          |
| Closures               | `src/crypto.rs` → `.map(|(&id, ...)| ...)`           |

---

## Useful Commands

```bash
cargo build              # compile the project
cargo test               # run all tests
cargo test -- --nocapture # run tests with println! visible
cargo run --example normal_case    # run the happy-path example
cargo run --example crash_recovery # run the persistence demo
cargo doc --open         # generate and open API documentation
```

---

> This folder is in `.gitignore` — it won't be pushed to your repo.
