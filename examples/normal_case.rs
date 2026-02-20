use persisthotstuff_rst::simulation::Simulation;

/// Demonstrates the happy path: all replicas are honest and responsive
fn main() {
    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║          Normal Case: Happy Path Simulation               ║");
    println!("║  All replicas honest, responsive, and synchronized        ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    
    // Create simulation with 4 replicas, f=1
    let mut sim = Simulation::new(4, 1, 100, false);
    
    println!("\nConfiguration:");
    println!("   Number of replicas (n): 4");
    println!("   Byzantine tolerance (f): 1");
    println!("   Quorum size (2f+1): 3");
    println!("   Consensus rounds: 10");
    
    // Run 10 consensus rounds
    sim.run(10);
    
    // Print results
    sim.network.print_stats();
    sim.print_final_state();
    
    // Verify safety
    assert!(sim.verify_safety(), "Safety violation detected!");
    
    // Compare block trees
    sim.compare_block_trees();
    
    // Check that all replicas committed the same blocks
    let committed_count = sim.replicas[0].committed_log.len();
    println!("\nResults:");
    println!("   Total blocks committed: {}", committed_count);
    println!("   Expected commits: >= 8 (3-chain rule has 2-block lag)");
    
    for (idx, replica) in sim.replicas.iter().enumerate() {
        println!("   Replica {}: {} commits", idx, replica.committed_log.len());
        assert_eq!(replica.committed_log.len(), committed_count, 
                   "Replica {} has different commit count!", idx);
    }
    
    println!("\n[OK] Normal case simulation successful!");
    println!("   ✓ All replicas synchronized");
    println!("   ✓ Consistent commit sequences");
    println!("   ✓ No safety violations");
    println!();
}
