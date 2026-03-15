# Lesson 1: Variables, Types & Type Aliases

## Where this appears in the project
- `src/types.rs` — type alias for `Hash`
- `src/config.rs` — type alias for `ReplicaId`, struct fields

---

## 1.1 — Let Bindings (Immutable by Default)

In Rust, variables are **immutable** by default. You must explicitly opt in to mutability.

```rust
let x = 5;          // immutable — cannot be reassigned
let mut y = 10;     // mutable — can be reassigned
y = 20;             // fine
// x = 6;           // COMPILE ERROR: cannot assign twice to immutable variable
```

### From the project (`src/wal.rs`, line ~333):
```rust
let payload: Vec<u8> = bincode::serialize(entry)?;
let checksum: u32 = crc32fast::hash(&payload);
```
Here `payload` and `checksum` are immutable — they are computed once and never change.
We add an explicit type annotation (`: Vec<u8>`, `: u32`) for clarity, but Rust can
usually infer the type on its own.

---

## 1.2 — Scalar Types

Rust has several built-in numeric types:

| Type    | Description            | Example          |
|---------|------------------------|------------------|
| `u8`    | Unsigned 8-bit integer | `let a: u8 = 255;` |
| `u16`   | Unsigned 16-bit        | `let b: u16 = 1;`  |
| `u32`   | Unsigned 32-bit        | `let c: u32 = 42;` |
| `u64`   | Unsigned 64-bit        | `let d: u64 = 100;`|
| `u128`  | Unsigned 128-bit       | `let e: u128 = 0;` |
| `usize` | Pointer-sized unsigned | `let f: usize = 3;`|
| `i32`   | Signed 32-bit          | `let g: i32 = -1;` |
| `f64`   | 64-bit float           | `let h: f64 = 3.14;`|
| `bool`  | Boolean                | `let ok = true;`   |

### From the project (`src/wal.rs`):
```rust
const WAL_VERSION: u16 = 1;        // 16-bit unsigned
const HEADER_SIZE: u64 = 30;       // 64-bit unsigned
```
`const` declares a compile-time constant. It must have an explicit type and its value
can never change.

---

## 1.3 — Type Aliases (`type`)

A type alias creates a **new name** for an existing type. It does NOT create a new type —
it is just a shorthand that makes code more readable.

### From `src/types.rs`:
```rust
pub type Hash = u64;
```
Now instead of writing `u64` everywhere for block hashes, we write `Hash`.
If we later decide that hashes should be `u128` or `[u8; 32]`, we change ONE line.

### From `src/config.rs`:
```rust
pub type ReplicaId = u64;
```
Same idea — replica IDs are just `u64` under the hood, but the alias tells readers
"this number represents a replica, not a block hash or a view number."

### Used later in structs:
```rust
pub struct Block {
    pub hash: Hash,              // actually u64
    pub proposer: ReplicaId,     // also u64, but semantically different
    pub view: u64,               // no alias here — it IS just a view number
}
```

---

## 1.4 — Shadowing

You can declare a new variable with the same name in the same scope. This is called
**shadowing** — the old value is hidden (not mutated).

```rust
let x = 5;
let x = x + 1;    // new binding, old x is gone
let x = x * 2;    // another new binding
println!("{}", x); // prints 12
```

### From `src/wal.rs` (open method):
```rust
let mut ver_bytes = [0u8; 2];
file.read_exact(&mut ver_bytes)?;
let version = u16::from_le_bytes(ver_bytes);
```
`ver_bytes` is a mutable byte array. We read into it, then create a new immutable
`version` from the parsed bytes. The raw bytes and the parsed integer coexist as
separate variables.

---

## 1.5 — Fixed-Size Arrays

A fixed-size array `[T; N]` stores exactly N elements of type T on the stack.

### From `src/crypto.rs`:
```rust
pub struct Signature {
    pub signer: ReplicaId,
    pub bytes: [u8; 64],     // exactly 64 bytes — Ed25519 signature size
}
```
This is different from `Vec<u8>` which lives on the heap and can grow.
`[u8; 64]` is stack-allocated and its size is known at compile time.

### From `src/wal.rs`:
```rust
const WAL_MAGIC: [u8; 4] = [0x48, 0x53, 0x4C, 0x57];  // ASCII "HSLW"
```
A 4-byte array used as the magic number at the start of every WAL file.

---

## 1.6 — Type Casting (`as`)

The `as` keyword performs explicit type conversion between numeric types.

### From `src/config.rs`:
```rust
pub fn leader_for_view(&self, view: u64) -> ReplicaId {
    let idx = (view as usize) % self.n;   // u64 → usize for modulo with n
    idx as ReplicaId                        // usize → u64 (ReplicaId)
}
```
- `view as usize` — converts the 64-bit view number to a pointer-sized integer
  (because `self.n` is `usize`).
- `idx as ReplicaId` — converts the result back to `u64` (alias `ReplicaId`).

Be careful: `as` can silently truncate. For example, `256u16 as u8` gives `0`.

---

## Exercises

1. Open `src/types.rs`. What would break if you changed `type Hash = u64` to `type Hash = u32`?
   (Hint: think about the WAL header which writes `hash.to_le_bytes()` — how many bytes is that?)

2. In `src/wal.rs`, why is `HEADER_SIZE` a `u64` and not a `usize`?
   (Hint: look at how it's used with `file.seek(SeekFrom::Start(HEADER_SIZE))`)

3. Create a type alias `type ViewNumber = u64;` and try using it in place of raw `u64`
   for view numbers in one of the structs.

---

Next lesson: [02 — Structs & Derive Macros](02_structs_and_derives.md)
