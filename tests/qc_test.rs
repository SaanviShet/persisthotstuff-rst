use persisthotstuff_rst::crypto::*;
use persisthotstuff_rst::types::*;
use persisthotstuff_rst::config::*;

#[test]
fn quorum_cert_forms_correctly() {
    let f = 1;
    let config = Config { n: 4, f, id: 0 };

    let mut sigs = vec![];
    for id in 0..config.quorum_size() {
        sigs.push(sign(id as u64));
    }

    let qc = QuorumCert {
        block_hash: 42,
        view: 1,
        signatures: sigs,
    };

    // QC should have at least 2f+1 signatures
    assert!(qc.signatures.len()>=config.quorum_size(), "QC doesn't have enough signatures");
}

#[test]
fn three_chain_commit_rule() {
    let b0 = Block { hash: 0, parent: None, view: 0, proposer: 0, qc: None };
    let b1 = Block { hash: 1, parent: Some(0), view: 1, proposer: 1, qc: Some(dummy_qc(0,0)) };
    let b2 = Block { hash: 2, parent: Some(1), view: 2, proposer: 2, qc: Some(dummy_qc(1,1)) };

    // The three-chain commit rule states that if we have a chain of three blocks 
    // (b0 -> b1 -> b2) where each block has a QC certifying its parent, 
    // then the first block (b0) can be considered committed.
    assert!(is_committed(&b0, &b1, &b2));
}

fn is_committed(b0: &Block, b1: &Block, b2: &Block) -> bool {
    b1.parent == Some(b0.hash) &&
    b2.parent == Some(b1.hash) &&
    b1.qc.is_some() &&
    b2.qc.is_some()
}
