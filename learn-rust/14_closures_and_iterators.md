# Lesson 14: Closures & Iterators

## Where this appears in the project
- `.map()`, `.filter()`, `.find()`, `.any()`, `.collect()` — used heavily throughout
- `|b| b.hash` — closure syntax in iterator chains
- `src/replica.rs` — iterator chains on block_tree and committed_log
- `src/crypto.rs` — `.iter().map().collect()` pattern for building collections

---

## 14.1 — What is a Closure?

A closure is an anonymous function that can capture variables from its surrounding
scope. It's like a lambda in Python or an arrow function in JavaScript.

```rust
// Regular function
fn add_one(x: i32) -> i32 { x + 1 }

// Closure (anonymous function)
let add_one = |x: i32| -> i32 { x + 1 };

// Closure with type inference (most common form)
let add_one = |x| x + 1;

// Multi-line closure
let process = |x| {
    let doubled = x * 2;
    doubled + 1
};
```

---

## 14.2 — Closures with Iterators

Iterators + closures are how Rust replaces traditional for loops for data transformation.

### The `.map()` pattern — transform each element:

```rust
// From src/replica.rs — extract hashes from committed blocks:
let committed_hashes: Vec<Hash> = self.committed_log
    .iter()              // create an iterator over &Block
    .map(|b| b.hash)    // transform each &Block into its hash (u64)
    .collect();          // gather results into a Vec<Hash>
```

Step by step:
1. `self.committed_log.iter()` — yields `&Block`, `&Block`, `&Block`, ...
2. `.map(|b| b.hash)` — for each `&Block`, extract the `hash` field → yields `u64`, `u64`, ...
3. `.collect()` — collects all the `u64` values into a `Vec<Hash>`

### Without closures (the imperative version):
```rust
let mut committed_hashes: Vec<Hash> = Vec::new();
for b in &self.committed_log {
    committed_hashes.push(b.hash);
}
```

Both produce the same result, but the iterator version is more concise and
often more efficient (the compiler can optimise iterator chains).

---

## 14.3 — `.find()` — Find the First Match

### From `src/replica.rs` — find a block whose parent matches:
```rust
let b1 = self.block_tree.values().find(|b| b.parent == Some(b0.hash));
```

- `self.block_tree.values()` — iterates over all `&Block` values in the BTreeMap
- `.find(|b| ...)` — returns `Option<&&Block>`: `Some` if found, `None` if not
- The closure `|b| b.parent == Some(b0.hash)` returns `true` for the match

### Combined with `match`:
```rust
let b1 = match b1 {
    Some(b) => b,
    None => continue,    // no matching child found, try next candidate
};
```

---

## 14.4 — `.any()` — Check if Any Element Matches

### From `src/replica.rs` — check if a block is already committed:
```rust
if self.committed_log.iter().any(|b| b.hash == b0.hash) {
    continue;   // skip — already in the committed log
}
```

`.any()` returns `true` as soon as it finds a matching element (short-circuits).

### Checking for duplicate votes:
```rust
// From src/replica.rs:
if entry.iter().any(|s| s.signer == sig.signer) {
    return None;  // duplicate vote from the same signer
}
```

---

## 14.5 — `.max_by_key()` — Find Maximum by a Key

### From `src/replica.rs`:
```rust
fn latest_block_hash(&self) -> Option<Hash> {
    self.block_tree.values()
        .max_by_key(|b| b.view)    // find the block with the highest view
        .map(|b| b.hash)           // extract its hash (Option<&Block> → Option<Hash>)
}
```

- `.max_by_key(|b| b.view)` — compares blocks by their `view` field, returns
  `Option<&Block>` (None if the tree is empty)
- `.map(|b| b.hash)` — transforms `Option<&Block>` into `Option<Hash>`

---

## 14.6 — `.collect()` — Build a Collection from an Iterator

`.collect()` is the most versatile method — it can build many collection types.
The target type is inferred from the type annotation.

### Collecting into a Vec:
```rust
// From src/replica.rs:
let committed_hashes: Vec<Hash> = self.committed_log
    .iter()
    .map(|b| b.hash)
    .collect();
```

### Collecting into a HashMap:
```rust
// From src/crypto.rs:
let public_keys: HashMap<ReplicaId, VerifyingKey> = all_keys
    .iter()
    .map(|(&id, (_sk, vk))| (id, vk.clone()))   // produce (key, value) tuples
    .collect();                                    // collect into HashMap
```

The iterator yields `(ReplicaId, VerifyingKey)` tuples, and `.collect()` knows to
insert each tuple as a key-value pair because the target type is `HashMap`.

### Collecting into a HashSet:
```rust
// From src/crypto.rs:
let unique: HashSet<ReplicaId> = qc.signatures
    .iter()
    .map(|s| s.signer)
    .collect();           // automatically deduplicates
```

---

## 14.7 — Closure Capturing

Closures can "capture" variables from their enclosing scope:

### From `src/replica.rs`:
```rust
let b1 = self.block_tree.values().find(|b| b.parent == Some(b0.hash));
//                                                         ^^^^^^^^
//                                           b0 is captured from the outer scope
```

The closure `|b| b.parent == Some(b0.hash)` uses `b0` which is defined outside
the closure. Rust automatically captures it by reference.

Three capture modes:
1. **By reference** (`&T`) — default, just borrows the value
2. **By mutable reference** (`&mut T`) — if the closure modifies the captured variable
3. **By value** (`T`) — if the closure moves the value, use `move` keyword

```rust
// Capture by reference (default):
let threshold = 3;
let is_above = |x: usize| x > threshold;   // borrows threshold

// Capture by move (takes ownership):
let name = String::from("hello");
let closure = move || println!("{}", name);  // name is MOVED into the closure
// println!("{}", name);  // ERROR: name was moved
```

---

## 14.8 — Iterator Methods Cheat Sheet

| Method            | What it does                               | Returns         |
|-------------------|--------------------------------------------|-----------------|
| `.iter()`         | Iterate by reference                       | Iterator<&T>    |
| `.into_iter()`    | Iterate by value (consumes collection)     | Iterator<T>     |
| `.map(f)`         | Transform each element                     | Iterator        |
| `.filter(f)`      | Keep elements where f returns true         | Iterator        |
| `.find(f)`        | First element where f returns true         | Option<&T>      |
| `.any(f)`         | True if any element matches                | bool            |
| `.all(f)`         | True if all elements match                 | bool            |
| `.count()`        | Count elements                             | usize           |
| `.collect()`      | Build a collection                         | Vec, HashMap, etc. |
| `.max()`          | Maximum element                            | Option<&T>      |
| `.max_by_key(f)`  | Maximum by a computed key                  | Option<&T>      |
| `.min()`          | Minimum element                            | Option<&T>      |
| `.enumerate()`    | Pair each element with its index           | Iterator<(usize, T)> |
| `.zip(other)`     | Pair elements from two iterators           | Iterator<(A, B)> |
| `.flatten()`      | Flatten nested iterators                   | Iterator        |
| `.for_each(f)`    | Call f on each element (like a for loop)   | ()              |
| `.cloned()`       | Clone each &T to get T                     | Iterator<T>     |
| `.sum()`          | Sum all elements                           | T               |

---

## 14.9 — Chaining Multiple Operations

### From `src/crypto.rs` — building a map from another map:
```rust
let public_keys: HashMap<ReplicaId, VerifyingKey> = all_keys
    .iter()                                        // iterate over (&id, &(sk, vk))
    .map(|(&id, (_sk, vk))| (id, vk.clone()))    // destructure and transform
    .collect();                                    // build HashMap
```

Breaking down the closure: `|(&id, (_sk, vk))| (id, vk.clone())`
- `&id` — dereference the key (borrow → copy, since it's u64)
- `_sk` — underscore prefix: we have the signing key but don't need it
- `vk` — the verifying key (we clone it because we need ownership)
- Returns `(id, vk.clone())` — a tuple for HashMap insertion

---

## 14.10 — `or_insert_with()` — Closures in the Entry API

### From `src/replica.rs`:
```rust
let entry = self.vote_pool.entry(vote.block_hash).or_insert_with(Vec::new);
```

`or_insert_with(Vec::new)` takes a closure `Vec::new` (which is actually a function
pointer in this case). It's only called if the key doesn't exist — this is called
**lazy initialization**.

You could also write it as a closure:
```rust
let entry = self.vote_pool.entry(vote.block_hash).or_insert_with(|| Vec::new());
```

Or use `or_default()` since `Vec::new()` is the default:
```rust
let entry = self.vote_pool.entry(vote.block_hash).or_default();
```

---

## Exercises

1. Rewrite this for loop as an iterator chain:
   ```rust
   let mut total_view = 0;
   for block in self.block_tree.values() {
       total_view += block.view;
   }
   ```

2. In `src/replica.rs`, the `find_committed_block()` method uses nested
   `.find()` calls. Could you rewrite it using `.filter()` and `.any()`?

3. What's the difference between `.iter()` and `.into_iter()`? When would
   you use each? (Hint: ownership.)

4. In `src/crypto.rs`, the `distribute_keys()` function clones `public_keys`
   for each replica. Could you use `Arc` (atomic reference counting) instead
   of cloning? What would the trade-offs be?

---

That's the final lesson! See the [README](README.md) for a summary and what to explore next.
