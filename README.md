# PersistHotStuff: Production-Grade BFT Consensus with Persistence

A robust implementation of the **HotStuff Byzantine Fault Tolerant (BFT) consensus protocol** extended with **persistence, dynamic membership, pluggable state machines, and improved liveness**. This project addresses critical limitations in existing HotStuff implementations by adding enterprise-grade features for production distributed systems.

---

## Table of Contents

1. [Project Overview](#project-overview)
2. [Key Features](#key-features)
3. [Architecture](#architecture)
4. [Prerequisites](#prerequisites)
5. [Installation & Setup](#installation--setup)
6. [Project Structure](#project-structure)
7. [Understanding the Scenarios](#understanding-the-scenarios)
8. [Running Each Scenario](#running-each-scenario)
9. [Expected Outputs](#expected-outputs)
10. [Advanced Usage](#advanced-usage)
11. [Documentation & Resources](#documentation--resources)
12. [Troubleshooting](#troubleshooting)

---

## Project Overview

### What is HotStuff?

HotStuff is a modern, leader-based Byzantine Fault Tolerant consensus protocol that solves the consensus problem in partially synchronous networks where:
- Up to **f replicas can be Byzantine** (malicious/faulty) out of **n = 3f + 1** total replicas
- Communication is **linear** (O(n)) instead of quadratic like older protocols (PBFT)
- **Responsiveness** is guaranteed after network synchrony is achieved

### The Problem

The Stanford CS244B reference implementation and most academic HotStuff implementations have four critical limitations:

| Limitation | Impact | Our Solution |
|-----------|--------|--------------|
| **No Persistence** | State lost on crash; no fault tolerance across reboots | Write-Ahead Logging (WAL) + Snapshots |
| **Static Membership** | Fixed set of replicas; cannot scale dynamically | Dynamic join/leave with consensus |
| **Simple State Machine** | Hard-coded log; not reusable for real applications | Trait-based pluggable app interface |
| **Liveness Issues** | System stalls with low client load | Dummy proposals from pacemaker |

**PersistHotStuff** solves all four problems simultaneously while maintaining Byzantine safety and liveness.

---

## Key Features

### 🔒 Crash-Safe Persistence
- **Write-Ahead Logging (WAL)**: Every critical state change (blocks, votes, QCs) is logged to disk before acting
- **Snapshots**: Periodic snapshots compress the log for faster recovery
- **Recovery**: Replicas automatically recover their last committed state on restart
- **Zero Data Loss**: All committed commands survive replica crashes

### 🔄 Dynamic Membership
- **Online Reconfiguration**: Add/remove replicas while the system runs (4 → 5 → 6 → 7 replicas)
- **Safety**: Quorum intersection properties maintained across membership changes
- **Invariant Enforcement**: System automatically maintains n = 3f + 1 constraint
- **Consistent Views**: All replicas agree on the active validator set at each epoch

### 🎯 Pluggable State Machines
- **Application Trait**: Implement custom logic (key-value store, counter, token transfers, etc.)
- **Decoupled Core**: Consensus algorithm independent from application state
- **gRPC Support**: Extend with external application servers
- **Composable**: Different application types without modifying core protocol

### ⚡ Improved Liveness
- **Dummy Proposals**: Pacemaker generates proposals when client load is low
- **No Stalls**: System makes progress even with single client or no requests
- **Configurable Timeouts**: Tune liveness parameters for your network

### 🛡️ Byzantine Fault Tolerance
- **Cryptographic signatures**: Ed25519 for authentication
- **Quorum certificates**: 2f + 1 signatures required to authorize blocks
- **Safety proven**: 3-chain commit rule prevents forks
- **Resilient to attacks**: Tolerate up to f Byzantine replicas

---

## Architecture

### System Components

```
┌─────────────────────────────────────────────────────────────┐
│                    Application Layer                         │
│              (KV Store, Counter, Blockchain, etc.)           │
└────────────────────────┬────────────────────────────────────┘
                         │ Application Trait
┌────────────────────────▼────────────────────────────────────┐
│                  Replica (Core Protocol)                    │
├─────────────────────────────────────────────────────────────┤
│ • Consensus Engine      • View Management                    │
│ • Block Tree            • Quorum Certificate Logic           │
│ • Liveness (Pacemaker)  • Member Reconfiguration             │
└──┬────────────────────────────────────────────────────────┬──┘
   │                                                         │
   │ Persistence                                  Network
   │                                             
┌──▼─────────────────┐          ┌─────────────────────────┐
│ Durability Layer    │          │ Communication Layer     │
├────────────────────┤          ├─────────────────────────┤
│ • WAL (disk)        │          │ • Message Routing       │
│ • Snapshots (disk)  │          │ • Byzantine Detection   │
│ • Recovery Process  │          │ • Delay Injection       │
└────────────────────┘          └─────────────────────────┘
```

### Key Data Structures

**Replica State:**
- `current_view`: Current consensus round number
- `block_tree`: All blocks proposed/received (immutable record)
- `high_qc`: Highest quorum certificate received
- `committed_log`: Blocks that achieved 3-chain commit
- `active_validators`: Current set of replicas in this epoch
- `config_epoch`: Epoch of the current membership configuration

**Persistence:**
- `wal`: Write-Ahead Log for durable state
- `snapshot`: Periodic snapshots for recovery speedup

---

## Prerequisites

### System Requirements
- **Rust 1.70+** (for modern async/await and edition 2021)
- **Linux/macOS/Windows with WSL** (Docker support for containerized testing)
- **Disk space**: ~100MB for dependencies + build artifacts

### Required Tools
```bash
# Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Verify installation
rustc --version  # Should be 1.70 or higher
cargo --version
```

### Optional Tools
- **Docker + Docker Compose** (for containerized deployment and testing)
- **Protocol Buffers compiler** (optional, `tonic-build` handles most cases)

---

## Installation & Setup

### 1. Clone the Repository

```bash
cd ~/Desktop/Project
# If not already cloned, clone the repository
git clone <repository-url>
cd persisthotstuff-rst
```

### 2. Build the Project

```bash
# Build all binaries and examples
cargo build --release

# Verify the build succeeded
cargo build --release --examples

# Clean build (useful if you have leftover state files)
cargo clean && cargo build --release
```

The build process will:
- Download all Rust dependencies (specified in `Cargo.toml`)
- Compile the core library
- Generate gRPC code from `.proto` files
- Compile all example binaries

### 3. Verify Installation

```bash
# List all available examples
cargo run --example normal_case --release -- --help 2>/dev/null || true

# Quick sanity check (runs in ~2-3 seconds)
cargo run --example normal_case --release 2>&1 | head -20
```

---

## Project Structure

```
persisthotstuff-rst/
├── src/                          # Core library code
│   ├── main.rs                   # Entry point (demo visualization)
│   ├── lib.rs                    # Module declarations
│   ├── replica.rs                # Replica state machine (core)
│   ├── types.rs                  # Data structures (Block, QC, etc.)
│   ├── crypto.rs                 # Ed25519 signatures, KeyStore
│   ├── config.rs                 # Configuration management
│   ├── wal.rs                    # Write-Ahead Logging
│   ├── snapshot.rs               # Snapshot generation/loading
│   ├── recovery.rs               # Recovery from disk
│   ├── network.rs                # Message routing, Byzantine injection
│   ├── simulation.rs             # Multi-replica orchestration
│   ├── pacemaker.rs              # Liveness (timeout, dummy proposals)
│   ├── app.rs                    # Application trait + simple KV store
│   ├── grpc.rs                   # gRPC service definitions
│   └── visualiser.rs             # Pretty printing, diagnostics
│
├── examples/                     # Runnable scenarios (see "Running Each Scenario")
│   ├── normal_case.rs                    # ✓ Happy path (all honest replicas)
│   ├── multi_replica_demo.rs             # ✓ 4-replica consensus + stats
│   ├── crash_recovery.rs                 # ✓ WAL + snapshot recovery
│   ├── byzantine_failures.rs             # ✓ Byzantine attacks + network faults
│   ├── persistent_crash_rejoin.rs        # ✓ Crash → recover → rejoin cluster
│   ├── eval_crash_recovery.rs            # ✓ Stress test: 120 crash/recover cycles
│   ├── eval_membership_cycling.rs        # ✓ Dynamic membership (4→5→7 replicas)
│   ├── eval_commit_latency.rs            # ✓ Measure consensus latency
│   ├── eval_performance_comparison.rs    # ✓ Compare with/without features
│   └── performance_comparison.rs         # ✓ Detailed performance metrics
│
├── tests/                        # Unit and integration tests
│   ├── commit_rule.rs            # Test 3-chain commit logic
│   ├── qc_formation.rs           # Test quorum certificate formation
│   ├── network_test.rs           # Test network layer
│   └── ... (more tests)
│
├── specs/                        # Formal specifications (TLA+)
│   ├── PersistHotStuff.tla       # Core protocol specification
│   ├── PersistHotStuff.cfg       # TLC model checker configuration
│   ├── PersistHotStuff_Membership.tla   # Membership reconfiguration spec
│   └── *.tlc trace files         # Counter-example traces
│
├── Documentation/                # Design documents
│   ├── PERSISTENCE_DESIGN.md     # WAL, snapshots, recovery architecture
│   ├── MULTI_REPLICA_IMPLEMENTATION.md  # How replicas coordinate
│   └── ...
│
├── Cargo.toml                    # Rust dependencies + example configurations
├── build.rs                      # Build script (generates gRPC code)
└── docker/                       # Containerization
    ├── Dockerfile                # Container image definition
    ├── docker-compose.yml        # Multi-container orchestration
    └── inject_network_faults.sh  # Helper for testing faults
```

---

## Understanding the Scenarios

Learn what each scenario demonstrates and how they progress in complexity.

### 1. **Normal Case** (Happy Path Foundation)
**File**: [`examples/normal_case.rs`](examples/normal_case.rs)

**What it demonstrates:**
- Baseline BFT consensus when all replicas are **honest and responsive**
- Simplest, most straightforward operation
- Foundation for understanding how consensus rounds work
- **No failures, attacks, or recovery**

**Key Concepts:**
- 4 replicas with f=1 Byzantine tolerance (f = floor(N-1)/3)
- Quorum size = 2f + 1 = 3 replicas needed to agree
- Rounds proceed sequentially without interference
- Every block gets a quorum certificate (votes from 3 out of 4)
- 3-chain commit rule: only blocks deep in the chain are final

**What you learn:**
- How long does one consensus round take?
- How many messages are sent?
- How deep must a block be to guarantee finality?

---

### 2. **Multi-Replica Demo** (Adding Observability)
**File**: [`examples/multi_replica_demo.rs`](examples/multi_replica_demo.rs)

**What it demonstrates:**
- Same happy path as Normal Case
- **Enhanced visualization** showing each replica's view
- Detailed statistics: messages, latency, throughput
- Per-replica block tree comparison
- Safety verification across all 4 replicas

**Key Concepts:**
- Block tree divergence detection (early safety check)
- Network statistics: send/receive counts, duplicate detection
- Per-replica slot occupancy and consensus progress
- Visual diff of block trees across replicas

**What you learn:**
- How to compare block trees across replicas (for detecting forks)
- Message overhead in consensus (network complexity)
- How to verify safety manually

---

### 3. **Byzantine Failures** (Resilience Under Attack)
**File**: [`examples/byzantine_failures.rs`](examples/byzantine_failures.rs)

**What it demonstrates:**
- **Byzantine replicas** (malicious nodes) attacking the consensus
- **Network faults**: message delays, drops, reordering
- **Leader failures** and view change recovery
- How the system **continues despite attacks** (because f=1 tolerates 1 faulty)

**Scenarios included:**
1. **Byzantine Replica Sends Invalid Votes**: Faulty node tries to vote for conflicting blocks
2. **Message Delays**: Some messages arrive late (but not discarded)
3. **Network Partitions**: Replicas can't reach each other temporarily
4. **Leader Failures**: Current proposer becomes unresponsive
5. **Out-of-Order Delivery**: Messages arrive in different order than sent

**Key Concepts:**
- Signature verification prevents fake votes
- Timeouts trigger view changes (new leader elected)
- Quorum threshold (2f+1) means f Byzantine nodes can be ignored
- Safety is **never compromised** regardless of failures
- Liveness may temporarily stall but always recovers

**What you learn:**
- How cryptographic signatures protect against forgery
- How timeouts enable view changes for liveness
- Why 2f+1 quorum size is the right threshold
- Fault injection techniques for testing

---

### 4. **Crash Recovery** (First Persistence Demo)
**File**: [`examples/crash_recovery.rs`](examples/crash_recovery.rs)

**What it demonstrates:**
- **Write-Ahead Logging (WAL)** functionality
- Running consensus rounds with disk persistence
- **Simulating a crash**: dropping all memory
- **Recovery**: reading full state from disk
- Verifying recovered state matches pre-crash state

**Flow:**
1. Start 4 replicas with WAL enabled
2. Run 5 consensus rounds (blocks written to disk)
3. Take a snapshot of committed blocks
4. Simulate crash (clear all memory structures)
5. Run recovery: reconstruct state from WAL + snapshot
6. Verify: recovered state matches what was committed
7. Continue consensus for 3 more rounds
8. Final safety check

**Key Concepts:**
- WAL entries are immutable; appended in order
- Snapshots speed up recovery by avoiding full WAL replay
- Recovery happens automatically on replica startup
- All committed commands survive the crash
- Replicas rejoin the cluster seamlessly

**What you learn:**
- How WAL protects against memory loss
- The time/space tradeoff between WAL and snapshots
- Recovery correctness verification
- Disk I/O patterns in consensus

---

### 5. **Persistent Crash/Recover/Rejoin** (Integration Test)
**File**: [`examples/persistent_crash_rejoin.rs`](examples/persistent_crash_rejoin.rs)

**What it demonstrates:**
- Full **end-to-end crash recovery cycle** in a 4-replica cluster
- One replica selected as victim and is **crashed intentionally**
- System continues making progress with 3 replicas
- Victim recovers from disk and **rejoins the cluster**
- All replicas stay safely synchronized after recovery

**Flow:**
1. Setup 4-replica persistent simulation with real data directories
2. Run 2 initial consensus rounds (build some committed state)
3. Pick one random victim replica
4. Crash the victim (becomes unresponsive)
5. Run 5 rounds: cluster continues without victim (N-1 = 3 replicas)
6. Recover the victim:
   - Reconstructs state from WAL + snapshots (data on disk)
   - Reads highest view and committed blocks
7. Run 8 post-recovery rounds
8. Verify safety: all committed logs are **prefix-consistent** across 4 replicas
9. Verify liveness: protocol made progress throughout

**Key Concepts:**
- f=1 Byzantine tolerance means system tolerates 1 crash (when N=4)
- 3 honest replicas can make decisions without the 4th
- Recovered replica reads its own persistent data (private disk)
- No central coordination; decentralized recovery
- Safety always holds; even if crash was Byzantine

**What you learn:**
- How crash/recovery works in a real cluster
- That the system can tolerate exactly f simultaneous crashes
- Recovery latency and resource overhead
- State consistency after recovery

---

### 6. **Crash Recovery Stress Test** (120 Cycles)
**File**: [`examples/eval_crash_recovery.rs`](examples/eval_crash_recovery.rs)

**What it demonstrates:**
- **Durability and robustness** under repeated failures
- Long-running evaluation: 120 crash/recover/rejoin cycles
- Round-robin victim selection (each replica crashes ~30 times)
- Safety verified after every single recovery
- Performance metrics: recovery time, commit rate, success rate

**Flow:**
1. Initialize persistent 4-replica simulation
2. **For each of 120 cycles:**
   - Run 4 consensus rounds (build committed state)
   - Select next victim replica
   - Run 8 rounds (crash fires when unresponsive)
   - Victim crashes and recovers
   - Run 4 post-recovery rounds
   - Assert safety across all replicas
3. Print summary: total recoveries, commits, elapsed time

**Success Criteria:**
- ✅ All 120 crash/recover cycles complete successfully
- ✅ Zero safety violations (no divergent committed logs)
- ✅ Zero crashes corrupt the WAL (file integrity maintained)
- ✅ Recovery time remains consistent (< 1 second per recovery)
- ✅ Protocol continues making progress throughout

**What you learn:**
- Durability testing methodology
- How failure patterns affect performance
- File corruption detection (CRC32 checksums)
- Long-term system stability
- Statistical reliability of the implementation

---

### 7. **Dynamic Membership Cycling** (4 → 5 → 7 Replicas)
**File**: [`examples/eval_membership_cycling.rs`](examples/eval_membership_cycling.rs)

**What it demonstrates:**
- **Adding and removing replicas** while consensus runs
- **Membership reconfiguration** via consensus (Byzantine-safe)
- Maintaining the **n = 3f + 1 invariant** at all times
- Quorum intersection properties across configuration changes
- Enforcing constraints: valid sizes are {4, 7, 10, 13, ...}

**Valid Transitions:**
| From | To | Reason | Example |
|------|----|---------|----|
| 3 | 4 | 3→4: n==4, f==1 ✓ (3f+1=4) | Scenario A ✓ |
| 4 | 5 | 4→5: n==5, f==1.33 ✗ (3f+1≠5) | Scenario C ✗ |
| 5 | 4 | 5→4: n==4, f==1 ✓ (3f+1==4) | Scenario B ✓ |
| 4 | 7 | 4→7: n==7, f==2 ✓ (3f+1==7) | Scenario E (after A) ✓ |

**Scenarios:**
1. **Scenario A**: Join transition 3→4 (repeated 10 times)
   - New replica starts, broadcasts join request
   - Existing 3 replicas agree on new member
   - Membership changes atomically at epoch boundary
   
2. **Scenario B**: Remove transition 5→4 (repeated 10 times)
   - Existing replica initiates leave
   - Remaining 4 agree to remove quitting member
   - Atomic reconfiguration
   
3. **Scenario C**: Invalid join 4→5 (enforcement test)
   - Attempt to add when result would be n=5
   - System **rejects** (violates n=3f+1)
   - Safety maintained via constraint check
   
4. **Scenario D**: Invalid remove 4→3 (enforcement test)
   - Attempt to remove when result would be n=3
   - System **rejects** (would lose Byzantine tolerance)
   
5. **Scenario E**: End-to-end cycle
   - 3→4 (valid), then 4→5 (rejected), then 4→7 (valid)
   - Tests cascading reconfigurations
   - Verifies safety throughout

**Key Concepts:**
- Quorum certificates signed by old/new members during transition
- Overlapping quorums ensure no forks during reconfiguration
- Reconfiguration is a **consensus decision** (not admin command)
- Invariant: n = 3f + 1 maintained at all times
- Join/leave requests are regular client commands

**What you learn:**
- Dynamic reconfiguration mechanisms
- Invariant enforcement patterns
- How consensus can modify its own membership
- Safety during periods of instability

---

### 8. **Performance Comparison**
**File**: [`examples/eval_performance_comparison.rs`](examples/eval_performance_comparison.rs)

**What it demonstrates:**
- **Throughput and latency metrics** for different system configurations
- Impact of persistence (WAL) on performance
- Impact of dynamic membership on performance
- Comparison: with/without each feature enabled

**Metrics Collected:**
- **Latency**: Time from client proposal to commit (in milliseconds)
- **Throughput**: Blocks committed per second
- **CPU time**: Total consensus rounds executed
- **Message count**: Total network messages sent

**Configurations Compared:**
- Baseline: In-memory consensus (no persistence)
- +WAL: Add Write-Ahead Logging
- +Membership: Add dynamic membership overhead
- +App: Add pluggable application layer
- Full: All features enabled

**What you learn:**
- Feature cost analysis
- Scalability limits
- Network bandwidth requirements
- Trade-offs between safety and performance

---

### 9. **Commit Latency Evaluation**
**File**: [`examples/eval_commit_latency.rs`](examples/eval_commit_latency.rs)

**What it demonstrates:**
- Detailed **commit latency analysis**
- How long does a single command take to be finalized?
- Latency distribution (mean, median, tail percentiles)
- Impact of different network conditions

**Flow:**
1. Setup 4-replica simulation with known network delays
2. Send 100 client proposals
3. Track each proposal through:
   - View 0: proposal sent
   - View 1: voted on
   - View 2: quorum certificate formed
   - View 3: 3-chain commit achieved
4. Measure time in each phase
5. Compute latency statistics

**What you learn:**
- One consensus round typically takes 100-500ms
- 3-chain rule requires 3 sequential rounds (300-1500ms for commit)
- Network delay dominates latency
- Byzantine detection adds minimal overhead

---

## Running Each Scenario

### Quick Start (Linux/macOS)

```bash
cd ~/Desktop/Project/persisthotstuff-rst/

# List all examples
cargo --list 2>/dev/null | grep -i example || ls examples/

# Run Normal Case (baseline, 10-15 seconds)
cargo run --example normal_case --release

# Run Multi-Replica Demo (30 seconds, lots of output)
cargo run --example multi_replica_demo --release

# Run Byzantine Failures (45 seconds, demonstrates resilience)
cargo run --example byzantine_failures --release

# Run Crash Recovery Demo (60 seconds, tests persistence)
cargo run --example crash_recovery --release

# Run Crash/Recover/Rejoin (45 seconds)
cargo run --example persistent_crash_rejoin --release

# Run Stress Test (120 cycles, ~5 minutes)
cargo run --example eval_crash_recovery --release

# Run Membership Cycling (90 seconds)
cargo run --example eval_membership_cycling --release

# Run Performance Evaluation (~2 minutes)
cargo run --example eval_performance_comparison --release

# Run Commit Latency Evaluation (~2 minutes)
cargo run --example eval_commit_latency --release
```

### Detailed Walkthrough: Normal Case

```bash
# 1. Navigate to project directory
cd ~/Desktop/Project/persisthotstuff-rst

# 2. Build in release mode (optimized, faster run)
cargo build --release --example normal_case

# 3. Run the example
cargo run --release --example normal_case

# Expected output:
# ╔════════════════════════════════════════════════════════════╗
# ║          Normal Case: Happy Path Simulation               ║
# ║  All replicas honest, responsive, and synchronized        ║
# ╚════════════════════════════════════════════════════════════╝
#
# Configuration:
#    Number of replicas (n): 4
#    Byzantine tolerance (f): 1
#    Quorum size (2f+1): 3
#    Consensus rounds: 10
#
# === NETWORK STATISTICS ===
# Total rounds: 10
# Messages sent: 120 (3-phase: propose, vote, commit)
# ...
# Safety verified: ✓

# 4. Verify it completed successfully (exit code 0)
echo "Exit code: $?"
```

### Detailed Walkthrough: Crash Recovery Stress Test

```bash
# 1. Clean any previous state
rm -rf data/

# 2. Build the evaluation binary
cargo build --release --example eval_crash_recovery

# 3. Run the stress test (this takes 5-10 minutes)
time cargo run --release --example eval_crash_recovery

# Expected output:
# ╔════════════════════════════════════════════════════════════════╗
# ║  Evaluation 1: Crash-Recovery Stress Test                    ║
# ║  n=4, f=1, 120 kill/restart cycles                          ║
# ╚════════════════════════════════════════════════════════════════╝
#
# Cycle   1/120: Crash replica 1... Recovered in 0.23s ✓
# Cycle   2/120: Crash replica 2... Recovered in 0.21s ✓
# ...
# Cycle 120/120: Crash replica 0... Recovered in 0.24s ✓
#
# ╔════════════════════════════════════════════════════════════════╗
# ║                      SUMMARY RESULTS                          ║
# ╚════════════════════════════════════════════════════════════════╝
# Total cycles:           120
# Successful recoveries:  120 (100%)
# Safety violations:      0
# Average recovery time:  0.23s
# Total time:             315s (5.25 minutes)

# 4. Inspect the data directory created
ls -lah data/
# data/
# ├── replica_0/
# │   ├── wal.log          # Write-ahead log
# │   ├── snapshot.bin     # Periodic snapshot
# │   └── .metadata
# ├── replica_1/
# └── ...

# 5. Cleanup (frees ~50MB of disk)
rm -rf data/
```

### Detailed Walkthrough: Dynamic Membership Cycling

```bash
# 1. Clean previous state
rm -rf .vscode/tlc/  # Clears TLC model checker traces

# 2. Build the evaluation
cargo build --release --example eval_membership_cycling

# 3. Run the membership test
time cargo run --release --example eval_membership_cycling

# Expected output:
# ╔════════════════════════════════════════════════════════════════╗
# ║  Evaluation 2: Dynamic Membership Cycling                    ║
# ║  Testing join/remove transitions with safety checks          ║
# ╚════════════════════════════════════════════════════════════════╝
#
# Scenario A - Valid join (3 → 4):
#   Pass 1/10:  3 → 4 ✓
#   Pass 2/10:  3 → 4 ✓
#   ...
#   Pass 10/10: 3 → 4 ✓
#   Status: All passes ✓ (10/10)
#
# Scenario B - Valid remove (5 → 4):
#   Pass 1/10: 5 → 4 ✓
#   ...
#   Status: All passes ✓ (10/10)
#
# Scenario C - Invalid join (4 → 5) [should reject]:
#   Status: Correctly rejected ✓
#
# Scenario D - Invalid remove (4 → 3) [should reject]:
#   Status: Correctly rejected ✓
#
# Scenario E - End-to-end cycle (3→4, 4→5?, 4→7):
#   3→4: ✓
#   4→5: Correctly rejected ✓
#   4→7: ✓
#   Final safety check: ✓
#
# ╔════════════════════════════════════════════════════════════════╗
# ║                 ALL SCENARIOS PASSED                          ║
# ╚════════════════════════════════════════════════════════════════╝
```

### Running Tests

```bash
# Run all unit tests
cargo test --lib --release

# Run specific test file
cargo test --test commit_rule --release

# Run with output (even passing tests)
cargo test --release -- --nocapture --test-threads=1

# Run integration tests
cargo test --test '*' --release

# Check for compiler warnings
cargo clippy --release

# Format code to Rust style
cargo fmt

# Check formatting without changing
cargo fmt -- --check
```

---

## Expected Outputs

### What to Look For During Runs

#### ✅ Normal Case
- **Green checkmarks**: ✓ indicates safety was verified
- **Message counts**: Should see ~3N messages per round (phase 1, 2, 3)
- **Block tree size**: Should grow by 1 block per round
- **No panics**: Should exit cleanly with no errors

#### ✅ Byzantine Failures
- **Attack description**: Explains what faulty replica is doing
- **System continues**: Despite the attack, progress is made
- **Timeouts fire**: View changes occur when leader fails
- **Safety held**: Final check confirms no forks

#### ✅ Crash Recovery
- **WAL entries**: Count of blocks written to disk
- **Snapshot created**: Indicates checkpoint was taken
- **"Before crash" state**: Shows memory contents
- **Recovery takes**: < 2 seconds for small log
- **"After recovery" state**: Matches before state exactly

#### ✅ Stress Test (120 cycles)
- **Each cycle**: Shows progress (Cycle N/120)
- **Recovery time**: Each recovery should take < 1 second
- **Safety check**: ✓ after each recovery
- **Summary**: Shows 100% success rate (120/120)

---

## Advanced Usage

### Clean Build

If you encounter build issues or stale state:

```bash
# Deep clean
cargo clean

# Remove all data files
rm -rf data/
rm -rf .vscode/tlc/

# Rebuild
cargo build --release --all-targets

# Run tests to verify
cargo test --release
```

### Running with Verbose Logging

```bash
# Enable debug output
RUST_LOG=debug cargo run --example normal_case --release 2>&1 | head -100

# Trace specific module
RUST_LOG=persisthotstuff_rst::replica=trace cargo run --example crash_recovery --release
```

### Custom Network Delays

Edit [`src/network.rs`](src/network.rs) to inject custom latencies:

```rust
// In src/network.rs, modify simulate_delay() or add_latency()
pub fn simulate_delay(min_ms: u64, max_ms: u64) -> u128 {
    // Example: 100-500ms delays
    std::thread::sleep(Duration::from_millis(
        rand::random::<u64>() % (max_ms - min_ms) + min_ms
    ));
}
```

Then rebuild and run.

### Perf Analysis

```bash
# Generate flame graph of normal case
cargo build --release --example normal_case

# Install flamegraph (one-time)
cargo install flamegraph

# Run with profiling
cargo flamegraph --example normal_case --release

# View results
open flamegraph.svg
```

### Docker Deployment

```bash
# Build container image
cd docker/
docker build -t persisthotstuff:latest .

# Run a scenario in container
docker run --rm persisthotstuff:latest cargo run --example normal_case --release

# Compose multi-container test
docker-compose -f docker-compose.yml up --build

# Monitor logs
docker-compose logs -f replica_0
```

---

## Documentation & Resources

### Important Files To Read

1. **Project Plan**: [PersistHotStuff_Final.md](Project_Plan/PersistHotStuff_Final.md)
   - Problem statement, objectives, expected outcomes
   - Read this first for understanding the "why"

2. **Persistence Design**: [PERSISTENCE_DESIGN.md](Documentation/PERSISTENCE_DESIGN.md)
   - Write-Ahead Logging architecture
   - Recovery mechanism details
   - Snapshot strategy

3. **Multi-Replica Implementation**: [MULTI_REPLICA_IMPLEMENTATION.md](Documentation/MULTI_REPLICA_IMPLEMENTATION.md)
   - How replicas communicate
   - View change protocol
   - Quorum certificate formation

4. **Formal Specifications**: [specs/PersistHotStuff.tla](specs/PersistHotStuff.tla)
   - TLA+ formal spec of the protocol
   - Primary/backup consensus rules
   - Can be model-checked with TLC

### Key API Documentation

```rust
// See src/types.rs for data structures
pub struct Block { ... }          // Individual block in chain
pub struct QuorumCert { ... }     // Certificate (2f+1 signatures)
pub struct Replica { ... }        // Main replica state

// See src/replica.rs for core operations
impl Replica {
    pub fn process_proposal(&mut self, block: Block) -> Result<()>
    pub fn process_vote(&mut self, vote: Vote) -> Result<()>
    pub fn proposed(&mut self, block: Block) -> Result<()>
}

// See src/app.rs for application trait
pub trait Application {
    fn execute(&mut self, cmd: &[u8]) -> Result<Vec<u8>>
    fn state(&self) -> Vec<u8>
}
```

### Learning Path (Recommended Order)

1. **Week 1**: Run examples in order (normal → multi → byzantine → crash)
2. **Week 2**: Read design documents and code comments
3. **Week 3**: Study formal specification (specs/*.tla)
4. **Week 4**: Extend with your own application or modifications

---

## Troubleshooting

### Build Fails

**Error**: `error: failed to run custom build command for 'persisthotstuff-rst'`

**Solution**:
```bash
# Ensure you have Rust 1.70+
rustup update

# Clean and rebuild
cargo clean
cargo build --release
```

**Error**: Protobuf compilation errors

**Solution**:
```bash
# Ensure tonic-build is in build-dependencies (it is by default)
# Force rebuild of proto generated code
rm -rf src/gen/
cargo build --release
```

### Runtime Issues

**Error**: `thread 'main' panicked at 'assertion failed: verify_safety()'`

**Cause**: Safety violation detected (fork in committed blocks)

**Debug**:
```bash
# Run with full output
cargo run --example eval_membership_cycling --release 2>&1 | tail -100

# Check which scenario failed
# Look for "Safety violation in Scenario X"

# Review that scenario's test code in examples/
```

**Error**: `Disk I/O error when loading snapshot`

**Cause**: Corrupted or incomplete WAL/snapshot file

**Solution**:
```bash
# Clean data and retry
rm -rf data/
cargo run --example eval_crash_recovery --release
```

**Error**: Out of memory / process killed

**Cause**: Very long tests can accumulate state

**Solution**:
```bash
# Run shorter tests first
cargo run --example normal_case --release

# Increase available memory or reduce test scale
# Edit examples/eval_crash_recovery.rs:
# const CYCLES: usize = 20;  // Instead of 120
```

### Performance

**"Examples run very slowly"**

**Solution**: Use release mode
```bash
# Always use --release flag
cargo run --release --example <name>

# Debug mode is 10-50x slower
```

**"Disk usage is very high"**

**Solution**: Clean up data directories
```bash
# Each cycle creates WAL + snapshot files
rm -rf data/
```

### Questions?

- Check [Documentation/](Documentation/) folder for design details
- Review example comments for specific behavior
- Look at [specs/](specs/) for formal guarantees
- Run tests: `cargo test --release`

---

## Summary: Quick Command Reference

```bash
# Setup (one-time)
cd ~/Desktop/Project/persisthotstuff-rst && cargo build --release

# Run scenarios (in order of complexity)
cargo run --release --example normal_case              # 10s: Baseline
cargo run --release --example multi_replica_demo       # 30s: Multi-replica
cargo run --release --example byzantine_failures       # 45s: Attacks
cargo run --release --example crash_recovery           # 60s: Persistence
cargo run --release --example persistent_crash_rejoin  # 45s: Recovery
cargo run --release --example eval_crash_recovery      # 5m: Stress test
cargo run --release --example eval_membership_cycling  # 90s: Reconfiguration

# Run all tests
cargo test --release

# Kill old processes (if stuck)
pkill -f "cargo run"
pkill -f "persisthotstuff"
```

---

## License & Attribution

**PersistHotStuff** extends the HotStuff protocol (Yin et al., 2019) with production-grade features.

**Original HotStuff Paper**: "HotStuff: BFT Consensus in the Lens of Blockchain", PODC 2019

**Implementation Base**: Inspired by Stanford CS244B HotStuff implementation

**This Project**: Adds persistence, dynamic membership, pluggable state machines, and improved liveness

---

**Happy consensus testing! 🚀**

For issues or questions, refer to the project documentation or open an issue on the repository.
