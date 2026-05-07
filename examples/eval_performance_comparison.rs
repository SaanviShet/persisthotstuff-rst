//! Evaluation 4 — Performance Comparison Benchmark
//!
//! Compares PersistHotStuff against analytical models of PBFT and Raft
//! across multiple cluster sizes (n = 4, 7, 10, 13).
//!
//! Run:  cargo run --example eval_performance_comparison
//!
//! Metrics collected per cluster size:
//!   1. Message complexity per consensus round (measured for PersistHotStuff,
//!      analytical for PBFT O(n²) and Raft O(n)).
//!   2. Commit latency: rounds required to commit a client command.
//!   3. Throughput: committed commands per round (steady-state).
//!   4. Wall-clock time per round.
//!
//! PBFT analytical model:
//!   - Pre-prepare: leader → all (n-1 messages)
//!   - Prepare: each replica → all (n × (n-1) messages)
//!   - Commit: each replica → all (n × (n-1) messages)
//!   - Total ≈ 2n² - n per consensus round
//!
//! Raft analytical model:
//!   - AppendEntries: leader → all (n-1 messages)
//!   - Replies: all → leader (n-1 messages)
//!   - Total ≈ 2(n-1) per consensus round
//!   - Note: Raft tolerates only crash faults (f < n/2), not Byzantine faults.
//!
//! HotStuff/PersistHotStuff (measured):
//!   - Proposal: leader → all (n-1)
//!   - Votes: all → leader (n-1)
//!   - QC broadcast: leader → all (n-1)
//!   - Total ≈ 3(n-1) per round, but 3 rounds needed for commit.
//!   - Measured directly via network.total_messages_sent.

use persisthotstuff_rst::simulation::Simulation;
use persisthotstuff_rst::types::ConsensusCommand;
use std::time::Instant;

/// Cluster sizes to benchmark.  These correspond to valid validator-set
/// sizes: 4 (min BFT), 7, 10, 13 — all satisfying n ≥ 4 and n % 3 == 1.
const CLUSTER_SIZES: &[usize] = &[4, 7, 10, 13];

/// Number of trials per cluster size for latency measurements.
const TRIALS: usize = 20;

/// Maximum rounds per trial before declaring timeout.
const MAX_ROUNDS: usize = 200;

/// Number of steady-state rounds for throughput measurement.
const THROUGHPUT_ROUNDS: usize = 50;

// ═══════════════════════════════════════════════════════════════════════
//  Analytical models
// ═══════════════════════════════════════════════════════════════════════

/// PBFT message complexity per consensus instance.
/// pre-prepare(n-1) + prepare(n(n-1)) + commit(n(n-1)) = 2n² - n
fn pbft_messages_per_round(n: usize) -> usize {
    2 * n * n - n
}

/// Raft message complexity per log entry replication.
/// AppendEntries(n-1) + Replies(n-1) = 2(n-1)
fn raft_messages_per_round(n: usize) -> usize {
    2 * (n - 1)
}

/// PBFT commit latency: 3 communication steps (pre-prepare, prepare, commit)
/// mapped to rounds — each PBFT "round" commits in 1 consensus instance
/// but with O(n²) messages.  We model it as 1 round (optimistic).
fn pbft_commit_latency() -> usize {
    1
}

/// Raft commit latency: 1 round-trip (AppendEntries + majority ack).
fn raft_commit_latency() -> usize {
    1
}

/// Maximum Byzantine faults tolerated.
fn max_byzantine_faults(n: usize) -> usize {
    // BFT: f < n/3
    (n - 1) / 3
}

/// Maximum crash faults tolerated (Raft model).
fn max_crash_faults(n: usize) -> usize {
    // CFT: f < n/2
    (n - 1) / 2
}

// ═══════════════════════════════════════════════════════════════════════
//  PersistHotStuff measurement
// ═══════════════════════════════════════════════════════════════════════

struct MeasuredMetrics {
    n: usize,
    messages_per_round: f64,
    commit_latency_mean: f64,
    commit_latency_min: usize,
    commit_latency_max: usize,
    commit_latency_p50: usize,
    throughput_per_round: f64,
    wall_ms_per_round: f64,
    commit_success_rate: f64,
}

fn measure_persisthotstuff(n: usize) -> MeasuredMetrics {
    let f = max_byzantine_faults(n);

    // ── 1. Message complexity ────────────────────────────────────────
    let mut total_msgs: usize = 0;
    let msg_rounds = 20;
    {
        let mut sim = Simulation::new(n, f, msg_rounds + 20, false);
        // Warm up
        for _ in 0..5 { sim.run_one_round(); }
        let msgs_before = sim.network.total_messages_sent;
        for _ in 0..msg_rounds { sim.run_one_round(); }
        total_msgs = sim.network.total_messages_sent - msgs_before;
    }
    let messages_per_round = total_msgs as f64 / msg_rounds as f64;

    // ── 2. Commit latency ────────────────────────────────────────────
    let mut latencies: Vec<usize> = Vec::with_capacity(TRIALS);
    let mut wall_times: Vec<f64> = Vec::with_capacity(TRIALS);
    let mut committed_count = 0usize;

    for trial in 0..TRIALS {
        let mut sim = Simulation::new(n, f, MAX_ROUNDS + 50, false);
        // Warm-up
        for _ in 0..5 { sim.run_one_round(); }
        sim.enable_dummy_proposals(0);

        let cmd = ConsensusCommand::ClientTx(
            format!("perf_test:tx_{}_n_{}", trial, n),
        );
        sim.enqueue_client_command(cmd.clone());

        let committed_before = sim.replicas[0].committed_log.len();
        let t0 = Instant::now();
        let mut rounds = 0;
        let mut found = false;

        for _ in 0..MAX_ROUNDS {
            sim.run_one_round_with_pacemaker();
            rounds += 1;

            for r in &sim.replicas {
                for entry in r.committed_log.iter().skip(committed_before) {
                    if entry.command == cmd {
                        found = true;
                        break;
                    }
                }
                if found { break; }
            }
            if found { break; }
        }

        let wall = t0.elapsed().as_secs_f64() * 1000.0;
        if found {
            latencies.push(rounds);
            committed_count += 1;
        } else {
            latencies.push(MAX_ROUNDS);
        }
        wall_times.push(wall);
    }

    let commit_latency_mean = if committed_count > 0 {
        latencies.iter().filter(|&&r| r < MAX_ROUNDS).sum::<usize>() as f64
            / committed_count as f64
    } else {
        MAX_ROUNDS as f64
    };

    let mut sorted_lat = latencies.clone();
    sorted_lat.sort();
    let commit_latency_min = sorted_lat[0];
    let commit_latency_max = *sorted_lat.last().unwrap();
    let commit_latency_p50 = sorted_lat[sorted_lat.len() / 2];

    let wall_ms_per_round = if !wall_times.is_empty() {
        wall_times.iter().sum::<f64>() / wall_times.len() as f64
            / (latencies.iter().sum::<usize>() as f64 / latencies.len() as f64).max(1.0)
    } else {
        0.0
    };

    // ── 3. Throughput (steady-state commits per round) ───────────────
    let throughput = {
        let mut sim = Simulation::new(n, f, THROUGHPUT_ROUNDS + 50, false);
        sim.enable_dummy_proposals(0);
        // Run warm-up
        for _ in 0..5 { sim.run_one_round_with_pacemaker(); }
        let commits_before: usize = sim.replicas.iter()
            .map(|r| r.committed_log.len())
            .max()
            .unwrap_or(0);

        for _ in 0..THROUGHPUT_ROUNDS {
            sim.run_one_round_with_pacemaker();
        }

        let commits_after: usize = sim.replicas.iter()
            .map(|r| r.committed_log.len())
            .max()
            .unwrap_or(0);
        let new_commits = commits_after.saturating_sub(commits_before);
        new_commits as f64 / THROUGHPUT_ROUNDS as f64
    };

    MeasuredMetrics {
        n,
        messages_per_round,
        commit_latency_mean,
        commit_latency_min,
        commit_latency_max,
        commit_latency_p50,
        throughput_per_round: throughput,
        wall_ms_per_round,
        commit_success_rate: committed_count as f64 / TRIALS as f64 * 100.0,
    }
}

fn main() {
    println!("╔════════════════════════════════════════════════════════════════════╗");
    println!("║  Evaluation 4: Performance Comparison Benchmark                  ║");
    println!("║  PersistHotStuff vs PBFT (analytical) vs Raft (analytical)       ║");
    println!("║  Cluster sizes: {:?}{:>25}║",
             CLUSTER_SIZES,
             format!("{} trials each", TRIALS));
    println!("╚════════════════════════════════════════════════════════════════════╝\n");

    let overall = Instant::now();
    let mut results: Vec<MeasuredMetrics> = Vec::new();

    for &n in CLUSTER_SIZES {
        let f = max_byzantine_faults(n);
        println!("─── Benchmarking n={} (f={}) ─────────────────────────", n, f);
        let t0 = Instant::now();
        let m = measure_persisthotstuff(n);
        println!("    Messages/round: {:.1}  Commit latency: {:.1} rounds  Throughput: {:.3} commits/round",
                 m.messages_per_round, m.commit_latency_mean, m.throughput_per_round);
        println!("    Wall-clock: {:.3} ms/round  Success: {:.0}%  ({:.2?})\n",
                 m.wall_ms_per_round, m.commit_success_rate, t0.elapsed());
        results.push(m);
    }

    // ═══════════════════════════════════════════════════════════════════
    //  TABLE 1: Message Complexity Comparison
    // ═══════════════════════════════════════════════════════════════════
    println!("\n{}", "═".repeat(78));
    println!("TABLE 1: Message Complexity per Consensus Round");
    println!("{}", "═".repeat(78));
    println!("{:>4} {:>6} {:>14} {:>14} {:>14} {:>14}",
             "n", "f_BFT", "PBFT O(n²)", "Raft O(n)", "HotStuff O(n)", "PHS (measured)");
    println!("{}", "─".repeat(78));
    for m in &results {
        let n = m.n;
        let f = max_byzantine_faults(n);
        let pbft = pbft_messages_per_round(n);
        let raft = raft_messages_per_round(n);
        let hotstuff_analytical = 3 * (n - 1); // 3 linear phases
        println!("{:>4} {:>6} {:>14} {:>14} {:>14} {:>14.1}",
                 n, f, pbft, raft, hotstuff_analytical, m.messages_per_round);
    }
    println!("{}", "═".repeat(78));
    println!("  PBFT: 2n²-n (pre-prepare + prepare + commit, all-to-all)");
    println!("  Raft: 2(n-1) (AppendEntries + replies, crash-fault only)");
    println!("  HotStuff: 3(n-1) (proposal + votes + QC broadcast, BFT)");

    // ═══════════════════════════════════════════════════════════════════
    //  TABLE 2: Commit Latency Comparison
    // ═══════════════════════════════════════════════════════════════════
    println!("\n{}", "═".repeat(78));
    println!("TABLE 2: Commit Latency (rounds to commit a client command)");
    println!("{}", "═".repeat(78));
    println!("{:>4} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
             "n", "PHS mean", "PHS min", "PHS max", "PHS p50", "PBFT", "Raft");
    println!("{}", "─".repeat(78));
    for m in &results {
        println!("{:>4} {:>10.1} {:>10} {:>10} {:>10} {:>10} {:>10}",
                 m.n, m.commit_latency_mean, m.commit_latency_min,
                 m.commit_latency_max, m.commit_latency_p50,
                 pbft_commit_latency(), raft_commit_latency());
    }
    println!("{}", "═".repeat(78));
    println!("  PHS requires 3-chain (3 rounds minimum); PBFT/Raft commit in 1 round.");
    println!("  PHS trades latency for O(n) message complexity (vs O(n²) for PBFT).");

    // ═══════════════════════════════════════════════════════════════════
    //  TABLE 3: Throughput and Scalability
    // ═══════════════════════════════════════════════════════════════════
    println!("\n{}", "═".repeat(78));
    println!("TABLE 3: Throughput and Scalability");
    println!("{}", "═".repeat(78));
    println!("{:>4} {:>6} {:>6} {:>16} {:>16} {:>14}",
             "n", "f_BFT", "f_CFT", "PHS commits/rnd", "Wall ms/round", "Success %");
    println!("{}", "─".repeat(78));
    for m in &results {
        println!("{:>4} {:>6} {:>6} {:>16.3} {:>16.3} {:>14.0}",
                 m.n,
                 max_byzantine_faults(m.n),
                 max_crash_faults(m.n),
                 m.throughput_per_round,
                 m.wall_ms_per_round,
                 m.commit_success_rate);
    }
    println!("{}", "═".repeat(78));

    // ═══════════════════════════════════════════════════════════════════
    //  TABLE 4: Fault Tolerance Comparison
    // ═══════════════════════════════════════════════════════════════════
    println!("\n{}", "═".repeat(78));
    println!("TABLE 4: Fault Tolerance Comparison");
    println!("{}", "═".repeat(78));
    println!("{:>4} {:>12} {:>12} {:>12} {:>14} {:>14}",
             "n", "PBFT f_BFT", "HS f_BFT", "Raft f_CFT", "PHS persist?", "PHS dynamic?");
    println!("{}", "─".repeat(78));
    for m in &results {
        let n = m.n;
        println!("{:>4} {:>12} {:>12} {:>12} {:>14} {:>14}",
                 n,
                 max_byzantine_faults(n),
                 max_byzantine_faults(n),
                 max_crash_faults(n),
                 "Yes (WAL+Snap)",
                 "Yes (n%3==1)");
    }
    println!("{}", "═".repeat(78));
    println!("  PBFT/HotStuff: tolerates f < n/3 Byzantine faults");
    println!("  Raft: tolerates f < n/2 crash faults only (no Byzantine)");
    println!("  PersistHotStuff adds: WAL+snapshot persistence, dynamic membership");

    // ═══════════════════════════════════════════════════════════════════
    //  TABLE 5: Scalability Factor (messages at n vs n=4)
    // ═══════════════════════════════════════════════════════════════════
    if results.len() >= 2 {
        let base_pbft = pbft_messages_per_round(4) as f64;
        let base_raft = raft_messages_per_round(4) as f64;
        let base_phs = results[0].messages_per_round;

        println!("\n{}", "═".repeat(78));
        println!("TABLE 5: Message Growth Factor (relative to n=4)");
        println!("{}", "═".repeat(78));
        println!("{:>4} {:>18} {:>18} {:>18}",
                 "n", "PBFT growth", "Raft growth", "PHS growth");
        println!("{}", "─".repeat(78));
        for m in &results {
            let n = m.n;
            let pbft_growth = pbft_messages_per_round(n) as f64 / base_pbft;
            let raft_growth = raft_messages_per_round(n) as f64 / base_raft;
            let phs_growth = m.messages_per_round / base_phs;
            println!("{:>4} {:>18.2}× {:>18.2}× {:>18.2}×",
                     n, pbft_growth, raft_growth, phs_growth);
        }
        println!("{}", "═".repeat(78));
        println!("  Quadratic growth in PBFT is clearly visible at larger n.");
    }

    let total_time = overall.elapsed();
    println!("\n{}", "═".repeat(78));
    println!("BENCHMARK COMPLETE");
    println!("  Total wall-clock time: {:.2?}", total_time);
    println!("  Cluster sizes tested: {:?}", CLUSTER_SIZES);
    println!("  Trials per size: {}", TRIALS);
    println!("{}", "═".repeat(78));

    // CSV-style output for easy embedding in paper
    println!("\n--- CSV DATA (for paper tables) ---");
    println!("n,f_BFT,PBFT_msgs,Raft_msgs,PHS_msgs,PHS_latency_mean,PHS_latency_p50,PHS_throughput,PHS_wall_ms");
    for m in &results {
        println!("{},{},{},{},{:.1},{:.1},{},{:.3},{:.3}",
                 m.n,
                 max_byzantine_faults(m.n),
                 pbft_messages_per_round(m.n),
                 raft_messages_per_round(m.n),
                 m.messages_per_round,
                 m.commit_latency_mean,
                 m.commit_latency_p50,
                 m.throughput_per_round,
                 m.wall_ms_per_round);
    }
}
