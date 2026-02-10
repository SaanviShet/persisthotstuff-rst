use persisthotstuff_rst::simulation::Simulation;
use persisthotstuff_rst::visualiser::*;

fn main() {
    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!("║     PersistHotStuff Multi-Replica Consensus Simulation    ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    
    // Create simulation with 4 replicas, f=1 (tolerates 1 Byzantine fault)
    let mut sim = Simulation::new(4, 1, 100, true);
    
    // Run 5 consensus rounds
    sim.run(5);
    
    // Print network statistics
    sim.network.print_stats();
    
    // Print final state
    sim.print_final_state();
    
    // Verify safety
    sim.verify_safety();
    
    // Compare block trees
    sim.compare_block_trees();
    
    // Visualize each replica's state
    println!("\n{}", "=".repeat(60));
    println!("INDIVIDUAL REPLICA VISUALIZATIONS");
    println!("{}", "=".repeat(60));
    
    for (idx, replica) in sim.replicas.iter().enumerate() {
        println!("\n--- Replica {} ---", idx);
        replica.visualize();
    }
    
    // Side-by-side comparison
    println!("\n{}", "=".repeat(60));
    println!("SIDE-BY-SIDE COMPARISON");
    println!("{}", "=".repeat(60));
    
    compare_replicas_side_by_side(&sim.replicas[0], &sim.replicas[1], "Replica 0", "Replica 1");
    
    // View timeline
    println!("\n{}", "=".repeat(60));
    println!("VIEW TIMELINE");
    println!("{}", "=".repeat(60));
    
    print_view_timeline(&sim.replicas[0].block_tree, sim.replicas[0].current_view);
    
    println!("\n✅ Multi-replica simulation completed successfully!\n");
}
