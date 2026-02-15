use persisthotstuff_rst::types::*;
use persisthotstuff_rst::config::ReplicaId;
use persisthotstuff_rst::crypto::{sign, KeyStore};
use persisthotstuff_rst::network::{Network, Message};


#[test]
fn test_network_creation() {
    let network = Network::new(4);
    assert_eq!(network.num_replicas, 4);
    assert!(!network.has_messages());
}

#[test]
fn test_send_receive() {
    let mut network = Network::new(4);
    let vote = Vote {
        block_hash: 1,
        view: 1,
        signature: sign(0),
    };
    
    network.send_vote(0, 1, vote.clone());
    assert!(network.has_messages());
    assert_eq!(network.pending_count(), 1);
    
    let msg = network.receive();
    assert!(msg.is_some());
    assert!(!network.has_messages());
}

#[test]
fn test_broadcast_proposal() {
    let mut network = Network::new(4);
    let block = Block {
        hash: 1,
        parent: Some(0),
        view: 1,
        proposer: 0,
        qc: None,
    };
    
    network.broadcast_proposal(0, block);
    // Should send to 3 other replicas (not to self)
    assert_eq!(network.pending_count(), 3);
}

