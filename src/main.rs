use std::collections::HashMap;
use persisthotstuff_rst::config::Config;
use persisthotstuff_rst::replica::Replica;
use persisthotstuff_rst::types::*;

fn main() {
    let config = Config { n: 4, f: 1, id: 0 };

    let mut replica = Replica {
        config,
        current_view: 4,
        block_tree: HashMap::new(),
        high_qc: None,
    };

    // Genesis
    replica.block_tree.insert(0, Block {
        hash: 0,
        parent: None,
        view: 0,
        proposer: 0,
        qc: None,
    });

    // B1
    replica.block_tree.insert(1, Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        proposer: 1,
        qc: Some(dummy_qc(0, 0)),
    });

    // B2
    replica.block_tree.insert(2, Block {
        hash: 2,
        parent: Some(1),
        view: 2,
        proposer: 2,
        qc: Some(dummy_qc(1, 1)),
    });

    // B3
    replica.block_tree.insert(3, Block {
        hash: 3,
        parent: Some(2),
        view: 3,
        proposer: 3,
        qc: Some(dummy_qc(2, 2)),
    });

    // B4
    replica.block_tree.insert(4, Block {
        hash: 4,
        parent: Some(3),
        view: 4,
        proposer: 0,
        qc: Some(dummy_qc(3, 3)),
    });

    replica.high_qc = replica.block_tree.get(&3).unwrap().qc.clone();

    replica.visualize();

    println!("PersistHotStuff core initialized");
}