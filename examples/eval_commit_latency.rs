//! Evaluation 3 — Commit Latency Benchmark
//!
//! Measures the number of consensus rounds required to commit a single
//! client command with and without the dummy-proposal pacemaker.
//!
//! Run:  cargo run --example eval_commit_latency
//!
//! Methodology:
//!   For each trial:
//!     A. WITHOUT pacemaker (baseline):
//!        1. Create a fresh 4-replica simulation.
//!        2. Enqueue one client command (Transfer { from, to, amount }).
//!        3. Use run_one_round() (standard: always proposes NoOp after the
//!           command block).  The client block is proposed once when it's
//!           popped from the queue, but subsequent rounds produce NoOps
//!           only if the standard round always proposes.  Under HotStuff,
//!           a 3-chain requires 3 blocks on top.
//!        4. Count rounds until the command appears in committed_log.
//!
//!     B. WITH pacemaker (enhanced):
//!        1. Same setup but enable_dummy_proposals(0) so idle NoOp blocks
//!           are injected immediately.
//!        2. Use run_one_round_with_pacemaker().
//!        3. Count rounds until the command commits.
//!
//!   Summary: mean, min, max, p50, p99 latencies for both modes.

use persisthotstuff_rst::simulation::Simulation;
use persisthotstuff_rst::types::ConsensusCommand;
use std::time::Instant;

const N: usize = 4;
const F: usize = 1;
const TRIALS: usize = 50;
const MAX_ROUNDS_PER_TRIAL: usize = 200;

fn main() {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Evaluation 3: Commit Latency Benchmark                      ║");
    println!("║  {} trials × 2 modes (with/without pacemaker)                ║", TRIALS);
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    let overall_start = Instant::now();
    let mut latencies_without: Vec<usize> = Vec::with_capacity(TRIALS);
    let mut latencies_with: Vec<usize> = Vec::with_capacity(TRIALS);
    let mut wall_without: Vec<f64> = Vec::with_capacity(TRIALS);
    let mut wall_with: Vec<f64> = Vec::with_capacity(TRIALS);

    for trial in 0..TRIALS {
        let cmd = ConsensusCommand::ClientTx(format!("transfer:alice_{}_to_bob_{}:amount_{}", trial, trial, 100 + trial));

        // ── A: WITHOUT pacemaker ────────────────────────────────────────
        {
            let mut sim = Simulation::new(N, F, MAX_ROUNDS_PER_TRIAL + 100, false);

            // Warm-up: run a few rounds so there is a base of committed blocks.
            for _ in 0..3 { sim.run_one_round(); }

            // Enqueue the command on the leader.
            sim.enqueue_client_command(cmd.clone());

            let t0 = Instant::now();
            let committed_before = sim.replicas[0].committed_log.len();
            let mut rounds = 0;
            let mut committed = false;

            for _ in 0..MAX_ROUNDS_PER_TRIAL {
                // Use the pacemaker round so client commands actually get proposed
                // (run_one_round always proposes NoOp so it would never pick up queued cmds).
                // But we disable dummies to isolate the "no pacemaker" effect.
                sim.disable_dummy_proposals();
                sim.run_one_round_with_pacemaker();
                rounds += 1;

                // Check if any replica committed the target command.
                for r in &sim.replicas {
                    for entry in r.committed_log.iter().skip(committed_before) {
                        if entry.command == cmd {
                            committed = true;
                            break;
                        }
                    }
                    if committed { break; }
                }
                if committed { break; }
            }

            let wall = t0.elapsed().as_secs_f64() * 1000.0; // ms
            if committed {
                latencies_without.push(rounds);
                wall_without.push(wall);
            }
            // If not committed within MAX_ROUNDS, record as sentinel.
            if !committed {
                latencies_without.push(MAX_ROUNDS_PER_TRIAL);
                wall_without.push(wall);
            }
        }

        // ── B: WITH pacemaker ───────────────────────────────────────────
        {
            let mut sim = Simulation::new(N, F, MAX_ROUNDS_PER_TRIAL + 100, false);
            for _ in 0..3 { sim.run_one_round(); }

            sim.enable_dummy_proposals(0); // zero delay → immediate dummies
            sim.enqueue_client_command(cmd.clone());

            let t0 = Instant::now();
            let committed_before = sim.replicas[0].committed_log.len();
            let mut rounds = 0;
            let mut committed = false;

            for _ in 0..MAX_ROUNDS_PER_TRIAL {
                sim.run_one_round_with_pacemaker();
                rounds += 1;

                for r in &sim.replicas {
                    for entry in r.committed_log.iter().skip(committed_before) {
                        if entry.command == cmd {
                            committed = true;
                            break;
                        }
                    }
                    if committed { break; }
                }
                if committed { break; }
            }

            let wall = t0.elapsed().as_secs_f64() * 1000.0;
            if committed {
                latencies_with.push(rounds);
                wall_with.push(wall);
            } else {
                latencies_with.push(MAX_ROUNDS_PER_TRIAL);
                wall_with.push(wall);
            }
        }

        if (trial + 1) % 10 == 0 {
            println!("  [trial {:>3}/{}] without={} rounds, with={} rounds",
                     trial + 1, TRIALS,
                     latencies_without.last().unwrap_or(&0),
                     latencies_with.last().unwrap_or(&0));
        }
    }

    let elapsed = overall_start.elapsed();

    // ── Statistics ───────────────────────────────────────────────────────
    fn stats(v: &[usize]) -> (f64, usize, usize, usize, usize) {
        if v.is_empty() { return (0.0, 0, 0, 0, 0); }
        let mut sorted = v.to_vec();
        sorted.sort();
        let mean = sorted.iter().sum::<usize>() as f64 / sorted.len() as f64;
        let min = sorted[0];
        let max = *sorted.last().unwrap();
        let p50 = sorted[sorted.len() / 2];
        let p99 = sorted[((sorted.len() as f64 * 0.99) as usize).min(sorted.len() - 1)];
        (mean, min, max, p50, p99)
    }

    fn wall_stats(v: &[f64]) -> (f64, f64, f64) {
        if v.is_empty() { return (0.0, 0.0, 0.0); }
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        let min = v.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        (mean, min, max)
    }

    let (mean_w, min_w, max_w, p50_w, p99_w) = stats(&latencies_without);
    let (mean_p, min_p, max_p, p50_p, p99_p) = stats(&latencies_with);
    let (wmean_w, wmin_w, wmax_w) = wall_stats(&wall_without);
    let (wmean_p, wmin_p, wmax_p) = wall_stats(&wall_with);

    println!("\n{}", "═".repeat(72));
    println!("COMMIT LATENCY BENCHMARK RESULTS  ({} trials, n={}, f={})", TRIALS, N, F);
    println!("{}", "═".repeat(72));
    println!("{:<32} {:>18} {:>18}", "", "Without Pacemaker", "With Pacemaker");
    println!("{}", "─".repeat(72));
    println!("{:<32} {:>18.2} {:>18.2}", "Mean rounds-to-commit:", mean_w, mean_p);
    println!("{:<32} {:>18} {:>18}", "Min:", min_w, min_p);
    println!("{:<32} {:>18} {:>18}", "Max:", max_w, max_p);
    println!("{:<32} {:>18} {:>18}", "p50:", p50_w, p50_p);
    println!("{:<32} {:>18} {:>18}", "p99:", p99_w, p99_p);
    println!("{}", "─".repeat(72));
    println!("{:<32} {:>15.2} ms {:>15.2} ms", "Mean wall-clock:", wmean_w, wmean_p);
    println!("{:<32} {:>15.2} ms {:>15.2} ms", "Min wall-clock:", wmin_w, wmin_p);
    println!("{:<32} {:>15.2} ms {:>15.2} ms", "Max wall-clock:", wmax_w, wmax_p);
    println!("{}", "═".repeat(72));
    println!("  Speedup (mean): {:.1}×", if mean_p > 0.0 { mean_w / mean_p } else { 0.0 });
    println!("  Total wall-clock: {:.2?}", elapsed);
    println!("{}", "═".repeat(72));

    // Determine expected behaviour.
    let without_committed = latencies_without.iter().filter(|&&r| r < MAX_ROUNDS_PER_TRIAL).count();
    let with_committed = latencies_with.iter().filter(|&&r| r < MAX_ROUNDS_PER_TRIAL).count();
    println!("\n  Commands committed: without={}/{}, with={}/{}", without_committed, TRIALS, with_committed, TRIALS);

    if with_committed == TRIALS {
        println!("\n[OK] Pacemaker committed all {} commands.", TRIALS);
    } else {
        eprintln!("\n[WARN] Pacemaker failed to commit {} commands.", TRIALS - with_committed);
    }
}
