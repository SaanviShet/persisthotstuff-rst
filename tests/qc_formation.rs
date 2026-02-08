use persisthotstuff_rst::config::Config;
use persisthotstuff_rst::replica::Replica;
use persisthotstuff_rst::types::*;

#[test]
fn qc_formation_from_votes() {
    let config = Config { n: 4, f: 1, id: 0 };

    let mut replica = Replica {
        config: config.clone(),
        current_view: 1,
        block_tree: std::collections::HashMap::new(),
        high_qc: None,
        vote_pool: std::collections::HashMap::new(),
        next_hash: 0,
    };

    let block_hash = 42u64;
    let view = 1u64;

    for id in 0..config.quorum_size() {
        let qc_opt = replica.receive_vote_from_replica(id as u64, block_hash, view);
        if let Some(qc) = qc_opt {
            // QC should have at least 2f+1 signatures, and the high QC should be updated to the new QC.
            assert!(qc.signatures.len() >= 2*config.f+1, "QC doesn't have enough signatures");
            assert_eq!(replica.high_qc.as_ref().unwrap().block_hash, block_hash);
            return;
        }
    }

    panic!("QC not formed");
}
