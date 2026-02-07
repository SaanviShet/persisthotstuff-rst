use std::collections::HashMap;
use crate::types::*;
use crate::config::*;

// Replica structure for the consensus protocol,
// Each replica maintains its configuration(n, f, id), 
// the current view number,
// a block tree to store the blocks it has seen,
// and the highest QC it has observed.
pub struct Replica {
    pub config: Config,
    pub current_view: u64,
    pub block_tree: HashMap<Hash, Block>,
    pub high_qc: Option<QuorumCert>,
}

use crate::visualiser::print_block_tree;

impl Replica {
    pub fn visualize(&self) {
        println!("==============================");
        println!("Replica {} | View {}", self.config.id, self.current_view);

        if let Some(qc) = &self.high_qc {
            println!("High QC: Block {} (view {})", qc.block_hash, qc.view);
        } else {
            println!("High QC: None");
        }

        print_block_tree(&self.block_tree);
        println!("==============================");
    }
}

