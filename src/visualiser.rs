use std::collections::HashMap;
use std::collections::BTreeMap;
use crate::types::*;

pub fn print_block_tree(blocks: &BTreeMap<u64, Block>) {
    fn print_subtree(
        blocks: &BTreeMap<u64, Block>,
        current: u64,
        prefix: String,
        is_last: bool,
    ) {
        let connector = if is_last { "└─ " } else { "├─ " };
        let block = &blocks[&current];

        let qc_marker = if block.qc.is_some() { " [QC]" } else { "" };
        println!("{}{}B{}{}", prefix, connector, block.hash, qc_marker);

        let children: Vec<u64> = blocks
            .values()
            .filter(|b| b.parent == Some(current))
            .map(|b| b.hash)
            .collect();

        let new_prefix = prefix + if is_last { "   " } else { "│  " };

        for (i, child) in children.iter().enumerate() {
            print_subtree(
                blocks,
                *child,
                new_prefix.clone(),
                i == children.len() - 1,
            );
        }
    }

    println!("Block Tree:");
    println!("B0 (genesis)");
    print_subtree(blocks, 0, String::from(" "), true);
}
