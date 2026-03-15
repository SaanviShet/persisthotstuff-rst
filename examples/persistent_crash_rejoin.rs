use std::path::Path;
use persisthotstuff_rst::simulation::Simulation;

fn main() {
    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║   Persisted Multi-Replica Crash/Recover/Rejoin Demo       ║");
    println!("╚════════════════════════════════════════════════════════════╝");

    let data_dir = Path::new("data/persistent_crash_rejoin");
    if data_dir.exists() {
        std::fs::remove_dir_all(data_dir).expect("failed to clean previous demo data dir");
    }

    let mut sim = Simulation::new_with_persistence(4, 1, 200, true, data_dir)
        .expect("failed to initialize persisted simulation");

    sim.run_crash_recover_rejoin_scenario(2, 5, 8, 5)
        .expect("crash/recover/rejoin scenario failed");

    sim.network.print_stats();
    sim.print_final_state();

    assert!(sim.verify_safety(), "safety verification failed after recovery");

    println!("\n[OK] Crash/recover/rejoin scenario completed with persistence enabled\n");
}
