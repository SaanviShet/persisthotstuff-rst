//! Evaluation 2 — Dynamic Membership Cycling Test
//!
//! Exercises the membership reconfiguration subsystem through multiple
//! scenarios, each in its own simulation, verifying safety throughout.
//!
//! Run:  cargo run --example eval_membership_cycling
//!
//! Background
//! ----------
//! The invariant `can_apply_new_validator_count(n)` requires n ≥ 4 and
//! n % 3 == 1, so the only valid validator-set sizes are {4, 7, 10, …}.
//! Single-step additions/removals are valid only when the *resulting*
//! size is in that set.  Valid single-step transitions:
//!
//!   ADD  3→4 ✓   6→7 ✓   9→10 ✓   ...
//!   REM  5→4 ✓   8→7 ✓   11→10 ✓  ...
//!
//! This evaluation tests:
//!   Scenario A – Valid join  (3 → 4), repeated 10 times with fresh sims.
//!   Scenario B – Valid remove (5 → 4), repeated 10 times.
//!   Scenario C – Invariant enforcement: invalid join (4 → 5) is rejected.
//!   Scenario D – Invariant enforcement: invalid remove (4 → 3) is rejected.
//!   Scenario E – End-to-end cycle: 3→4, then attempt 4→5 (rejected), safety.
//!
//! Each scenario asserts safety (all committed logs are prefix-consistent).

use persisthotstuff_rst::simulation::Simulation;
use persisthotstuff_rst::types::ConsensusCommand;

const ROUNDS_FOR_COMMIT: usize = 15;

fn main() {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Evaluation 2: Dynamic Membership Cycling                    ║");
    println!("║  Testing join/remove transitions with safety checks          ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    let overall = std::time::Instant::now();
    let mut a_pass = 0u32;
    let mut b_pass = 0u32;
    let mut c_pass = 0u32;
    let mut d_pass = 0u32;
    let mut e_pass = 0u32;

    // ═══════════════════════════════════════════════════════════════════
    //  Scenario A: Valid join 3 → 4  (10 repetitions)
    // ═══════════════════════════════════════════════════════════════════
    println!("─── Scenario A: Valid JOIN (3→4) ──────────────────────");
    for rep in 0..10 {
        let dir = tempfile::tempdir().unwrap();
        // Create 4 replicas but start with only {0,1,2} active.
        let mut sim = Simulation::new_with_persistence(4, 1, 5000, false, dir.path()).unwrap();

        // Shrink active set to {0,1,2} on live replicas.
        for id in 0..3usize {
            sim.replicas[id].active_validators = (0..3u64).collect();
            sim.replicas[id].config.n = 3;
            sim.replicas[id].config.f = 0;
        }
        let _ = sim.crash_replica(3);

        // Warm-up rounds.
        for _ in 0..4 { sim.run_one_round(); }

        // Enqueue JoinValidator(3) through the pacemaker.
        let pub_key = sim.replicas[3].keystore.my_public_key_bytes().to_vec();
        sim.enqueue_client_command(ConsensusCommand::JoinValidator {
            replica_id: 3,
            public_key: pub_key,
        });
        sim.enable_dummy_proposals(0);
        for _ in 0..ROUNDS_FOR_COMMIT { sim.run_one_round_with_pacemaker(); }

        let joined = sim.replicas[0].active_validators.contains(&3);
        let safe = sim.verify_safety();

        if joined && safe {
            a_pass += 1;
        } else {
            eprintln!("  [A-{}] FAIL joined={} safe={}", rep, joined, safe);
        }
    }
    println!("  Scenario A: {}/10 passed\n", a_pass);

    // ═══════════════════════════════════════════════════════════════════
    //  Scenario B: Valid remove 5 → 4  (10 repetitions)
    // ═══════════════════════════════════════════════════════════════════
    println!("─── Scenario B: Valid REMOVE (5→4) ───────────────────");
    for rep in 0..10 {
        let dir = tempfile::tempdir().unwrap();
        let mut sim = Simulation::new_with_persistence(5, 1, 5000, false, dir.path()).unwrap();

        // Active set starts as {0,1,2,3,4} (n=5).
        for _ in 0..4 { sim.run_one_round(); }

        sim.enqueue_client_command(ConsensusCommand::RemoveValidator { replica_id: 4 });
        sim.enable_dummy_proposals(0);
        for _ in 0..ROUNDS_FOR_COMMIT { sim.run_one_round_with_pacemaker(); }

        let removed = !sim.replicas[0].active_validators.contains(&4);
        let new_n = sim.replicas[0].active_validators.len();
        let safe = sim.verify_safety();

        if removed && new_n == 4 && safe {
            b_pass += 1;
        } else {
            eprintln!("  [B-{}] FAIL removed={} n={} safe={}", rep, removed, new_n, safe);
        }
    }
    println!("  Scenario B: {}/10 passed\n", b_pass);

    // ═══════════════════════════════════════════════════════════════════
    //  Scenario C: Invalid join 4 → 5 (should be rejected)
    // ═══════════════════════════════════════════════════════════════════
    println!("─── Scenario C: Invariant enforcement (4→5 rejected) ─");
    for rep in 0..5 {
        let dir = tempfile::tempdir().unwrap();
        let mut sim = Simulation::new_with_persistence(5, 1, 5000, false, dir.path()).unwrap();
        // Crash replica 4 so active set is {0,1,2,3}.
        let _ = sim.crash_replica(4);
        for id in 0..4usize {
            sim.replicas[id].active_validators = (0..4u64).collect();
            sim.replicas[id].config.n = 4;
            sim.replicas[id].config.f = 1;
        }

        for _ in 0..4 { sim.run_one_round(); }

        let pub_key = sim.replicas[4].keystore.my_public_key_bytes().to_vec();
        sim.enqueue_client_command(ConsensusCommand::JoinValidator {
            replica_id: 4,
            public_key: pub_key,
        });
        sim.enable_dummy_proposals(0);
        for _ in 0..ROUNDS_FOR_COMMIT { sim.run_one_round_with_pacemaker(); }

        // The join SHOULD have been rejected by the invariant (5%3≠1).
        let still_four = sim.replicas[0].active_validators.len() == 4;
        let not_joined = !sim.replicas[0].active_validators.contains(&4);
        let safe = sim.verify_safety();

        if still_four && not_joined && safe {
            c_pass += 1;
        } else {
            eprintln!("  [C-{}] FAIL n={} contains_4={} safe={}", rep,
                      sim.replicas[0].active_validators.len(),
                      sim.replicas[0].active_validators.contains(&4), safe);
        }
    }
    println!("  Scenario C: {}/5 passed\n", c_pass);

    // ═══════════════════════════════════════════════════════════════════
    //  Scenario D: Invalid remove 4 → 3 (should be rejected)
    // ═══════════════════════════════════════════════════════════════════
    println!("─── Scenario D: Invariant enforcement (4→3 rejected) ─");
    for rep in 0..5 {
        let mut sim = Simulation::new(4, 1, 5000, false);

        for _ in 0..4 { sim.run_one_round(); }

        sim.enqueue_client_command(ConsensusCommand::RemoveValidator { replica_id: 3 });
        sim.enable_dummy_proposals(0);
        for _ in 0..ROUNDS_FOR_COMMIT { sim.run_one_round_with_pacemaker(); }

        let still_four = sim.replicas[0].active_validators.len() == 4;
        let still_has_3 = sim.replicas[0].active_validators.contains(&3);
        let safe = sim.verify_safety();

        if still_four && still_has_3 && safe {
            d_pass += 1;
        } else {
            eprintln!("  [D-{}] FAIL n={} has_3={} safe={}", rep,
                      sim.replicas[0].active_validators.len(),
                      sim.replicas[0].active_validators.contains(&3), safe);
        }
    }
    println!("  Scenario D: {}/5 passed\n", d_pass);

    // ═══════════════════════════════════════════════════════════════════
    //  Scenario E: Full cycle  3 → 4 → 5(rejected) → safety
    // ═══════════════════════════════════════════════════════════════════
    println!("─── Scenario E: End-to-end join→reject cycle ─────────");
    for rep in 0..5 {
        let dir = tempfile::tempdir().unwrap();

        // Phase 1: join 3 → 4
        let mut sim = Simulation::new_with_persistence(5, 1, 5000, false, dir.path()).unwrap();
        let _ = sim.crash_replica(3);
        let _ = sim.crash_replica(4);
        for id in 0..3usize {
            sim.replicas[id].active_validators = (0..3u64).collect();
            sim.replicas[id].config.n = 3;
            sim.replicas[id].config.f = 0;
        }
        for _ in 0..4 { sim.run_one_round(); }

        let pk3 = sim.replicas[3].keystore.my_public_key_bytes().to_vec();
        sim.enqueue_client_command(ConsensusCommand::JoinValidator { replica_id: 3, public_key: pk3 });
        sim.enable_dummy_proposals(0);
        // Need extra rounds because 2/5 replicas are crashed, so leader rotation
        // wastes rounds when a crashed replica is the leader.
        for _ in 0..30 { sim.run_one_round_with_pacemaker(); }

        let joined = sim.replicas[0].active_validators.contains(&3);
        let n_after_join = sim.replicas[0].active_validators.len();
        let safe1 = sim.verify_safety();

        // Phase 2: recover replica 3, sync membership, then try invalid join of 4.
        if joined {
            let _ = sim.recover_replica(3);
            let vals = sim.replicas[0].active_validators.clone();
            let epoch = sim.replicas[0].config_epoch;
            sim.replicas[3].active_validators = vals.clone();
            sim.replicas[3].config_epoch = epoch;
            sim.replicas[3].config.n = vals.len();
            sim.replicas[3].config.f = (vals.len() - 1) / 3;
        }

        // Try to join replica 4 → n=5 (should fail invariant).
        let pk4 = sim.replicas[4].keystore.my_public_key_bytes().to_vec();
        sim.enqueue_client_command(ConsensusCommand::JoinValidator { replica_id: 4, public_key: pk4 });
        for _ in 0..ROUNDS_FOR_COMMIT { sim.run_one_round_with_pacemaker(); }
        let rejected_5 = !sim.replicas[0].active_validators.contains(&4);

        let safe2 = sim.verify_safety();

        let pass = joined && n_after_join == 4 && safe1 && rejected_5 && safe2;
        if pass {
            e_pass += 1;
        } else {
            eprintln!("  [E-{}] joined={} n={} safe1={} rej5={} safe2={}",
                      rep, joined, n_after_join, safe1, rejected_5, safe2);
        }
    }
    println!("  Scenario E: {}/5 passed\n", e_pass);

    // ── Summary ─────────────────────────────────────────────────────
    let elapsed = overall.elapsed();
    let total_scenarios: u32 = 10 + 10 + 5 + 5 + 5;
    let total_passed = a_pass + b_pass + c_pass + d_pass + e_pass;

    println!("{}", "═".repeat(64));
    println!("DYNAMIC MEMBERSHIP CYCLING RESULTS");
    println!("{}", "═".repeat(64));
    println!("  Scenario A (join 3→4):             {:>2}/10", a_pass);
    println!("  Scenario B (remove 5→4):           {:>2}/10", b_pass);
    println!("  Scenario C (invariant: 4→5 rej):   {:>2}/5", c_pass);
    println!("  Scenario D (invariant: 4→3 rej):   {:>2}/5", d_pass);
    println!("  Scenario E (join+reject cycle):    {:>2}/5", e_pass);
    println!("  ─────────────────────────────────────────");
    println!("  Total:                             {:>2}/{}", total_passed, total_scenarios);
    println!("  Wall-clock time:                   {:.2?}", elapsed);
    println!("{}", "═".repeat(64));

    if total_passed == total_scenarios {
        println!("\n[OK] All {} membership scenarios passed with zero safety violations.", total_scenarios);
    } else {
        eprintln!("\n[FAIL] {} / {} scenarios failed.", total_scenarios - total_passed, total_scenarios);
        std::process::exit(1);
    }
}
