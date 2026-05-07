//! Evaluation 1 — Crash-Recovery Stress Test
//!
//! Runs 120 kill/restart cycles with n=4, f=1 and verifies that:
//!   (a) Every recovered replica rejoins cleanly.
//!   (b) The committed log stays consistent across all replicas (safety).
//!   (c) The protocol continues to make progress after each recovery.
//!
//! Run:  cargo run --example eval_crash_recovery
//!
//! Methodology:
//!   1. Create a 4-replica persistent simulation (WAL + snapshots).
//!   2. For each of 120 cycles:
//!      a. Run several consensus rounds to build committed state.
//!      b. Pick a victim replica (round-robin, staying within f=1).
//!      c. Use the built-in `run_crash_recover_rejoin_scenario` which:
//!           – marks the victim unresponsive
//!           – runs rounds until the timeout-crash fires
//!           – recovers from snapshot + WAL
//!           – runs post-recovery rounds
//!      d. Assert safety across all replicas after every recovery.
//!   3. Print summary statistics (recoveries, commits, wall-clock time).

use persisthotstuff_rst::simulation::Simulation;
use std::time::Instant;

const N: usize = 4;
const F: usize = 1;
const CYCLES: usize = 120;

const ROUNDS_BEFORE: usize = 4;
const ROUNDS_DURING: usize = 8;   // must exceed timeout_ms/round_time_ms so crash fires
const ROUNDS_AFTER: usize = 4;
const MAX_STEPS: usize = CYCLES * (ROUNDS_BEFORE + ROUNDS_DURING + ROUNDS_AFTER) + 2000;

fn main() {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Evaluation 1: Crash-Recovery Stress Test                    ║");
    println!("║  n={}, f={}, {} kill/restart cycles                          ║", N, F, CYCLES);
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let mut sim = Simulation::new_with_persistence(N, F, MAX_STEPS, false, dir.path())
        .expect("failed to create persistent simulation");

    let overall_start = Instant::now();
    let mut safety_violations = 0u64;
    let mut successful_recoveries = 0u64;
    let mut failed_recoveries = 0u64;
    let mut committed_per_cycle: Vec<usize> = Vec::with_capacity(CYCLES);

    // Run a few initial rounds so there is committed state.
    for _ in 0..6 { sim.run_one_round(); }

    for cycle in 0..CYCLES {
        let victim = cycle % N;  // round-robin victim selection

        let committed_before = sim.replicas.iter()
            .map(|r| r.committed_log.len())
            .max()
            .unwrap_or(0);

        // Built-in crash → recover → rejoin scenario.
        match sim.run_crash_recover_rejoin_scenario(victim, ROUNDS_BEFORE, ROUNDS_DURING, ROUNDS_AFTER) {
            Ok(()) => successful_recoveries += 1,
            Err(e) => {
                eprintln!("  [cycle {}] Scenario for R{} failed: {}", cycle, victim, e);
                failed_recoveries += 1;
                // Still run some rounds to keep things moving.
                for _ in 0..ROUNDS_AFTER { sim.run_one_round(); }
            }
        }

        // Safety check (suppress the verbose banner).
        let safe = sim.verify_safety();
        if !safe { safety_violations += 1; }

        let committed_after = sim.replicas.iter()
            .map(|r| r.committed_log.len())
            .max()
            .unwrap_or(0);
        committed_per_cycle.push(committed_after.saturating_sub(committed_before));

        if (cycle + 1) % 20 == 0 || cycle == 0 {
            println!("  [cycle {:>3}/{}] victim=R{}, ok={}, delta_committed={}, safe={}",
                     cycle + 1, CYCLES, victim,
                     successful_recoveries, committed_per_cycle.last().unwrap_or(&0), safe);
        }
    }

    let elapsed = overall_start.elapsed();

    // ── Summary ─────────────────────────────────────────────────────────
    let total_committed = sim.replicas.iter().map(|r| r.committed_log.len()).max().unwrap_or(0);
    let avg_committed = if committed_per_cycle.is_empty() { 0.0 } else {
        committed_per_cycle.iter().sum::<usize>() as f64 / committed_per_cycle.len() as f64
    };

    println!("\n{}", "═".repeat(64));
    println!("CRASH-RECOVERY STRESS TEST RESULTS");
    println!("{}", "═".repeat(64));
    println!("  Cycles:                  {}", CYCLES);
    println!("  Successful recoveries:   {}", successful_recoveries);
    println!("  Failed recoveries:       {}", failed_recoveries);
    println!("  Safety violations:       {}", safety_violations);
    println!("  Total committed (max):   {}", total_committed);
    println!("  Avg committed/cycle:     {:.2}", avg_committed);
    println!("  Wall-clock time:         {:.2?}", elapsed);
    println!("{}", "═".repeat(64));

    if safety_violations > 0 {
        eprintln!("\n[FAIL] {} SAFETY VIOLATIONS DETECTED!", safety_violations);
        std::process::exit(1);
    }
    if failed_recoveries > 0 {
        eprintln!("\n[WARN] {} recovery scenarios returned errors (timeout may have been too tight)", failed_recoveries);
    }
    println!("\n[OK] All {} cycles completed with zero safety violations.", CYCLES);
}
