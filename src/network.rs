//! Network module for simulating message passing between replicas in the consensus protocol.
//!
//! This module provides the infrastructure for multi-replica communication including
//! message types, network simulation, and broadcast primitives.

use crate::types::*;
use crate::config::ReplicaId;
use std::collections::VecDeque;

/// Message types for communication between replicas in the consensus protocol
#[derive(Clone, Debug)]
pub enum Message {
    /// Proposal message sent from leader to all replicas
    Proposal {
        from: ReplicaId,
        to: ReplicaId,
        block: Block,
    },
    
    /// Vote message sent from replica to leader
    Vote {
        from: ReplicaId,
        to: ReplicaId,
        vote: Vote,
    },
    
    /// Broadcast QC to all replicas after formation
    QuorumCertBroadcast {
        from: ReplicaId,
        to: ReplicaId,
        qc: QuorumCert,
        block_hash: Hash,
    },
    
    /// New view notification when view changes
    NewView {
        from: ReplicaId,
        to: ReplicaId,
        view: u64,
        high_qc: Option<QuorumCert>,
    },
}

impl Message {
    pub fn sender(&self) -> ReplicaId {
        match self {
            Message::Proposal { from, .. } => *from,
            Message::Vote { from, .. } => *from,
            Message::QuorumCertBroadcast { from, .. } => *from,
            Message::NewView { from, .. } => *from,
        }
    }
    
    pub fn receiver(&self) -> ReplicaId {
        match self {
            Message::Proposal { to, .. } => *to,
            Message::Vote { to, .. } => *to,
            Message::QuorumCertBroadcast { to, .. } => *to,
            Message::NewView { to, .. } => *to,
        }
    }
    
    pub fn msg_type(&self) -> &str {
        match self {
            Message::Proposal { .. } => "Proposal",
            Message::Vote { .. } => "Vote",
            Message::QuorumCertBroadcast { .. } => "QC-Broadcast",
            Message::NewView { .. } => "NewView",
        }
    }
}

/// Network simulator that manages message delivery between replicas
pub struct Network {
    /// Queue of pending messages to be delivered
    message_queue: VecDeque<Message>,
    
    /// Total number of replicas in the network
    num_replicas: usize,
    
    /// Flag to enable/disable message delays (for testing)
    simulate_delays: bool,
    
    /// Statistics
    pub total_messages_sent: usize,
    pub messages_by_type: [usize; 4], // Proposal, Vote, QC, NewView
    
    /// Global hash counter for unique block hashes across all replicas
    next_global_hash: Hash,
}

impl Network {
    /// Create a new network with the specified number of replicas
    pub fn new(num_replicas: usize) -> Self {
        Network {
            message_queue: VecDeque::new(),
            num_replicas,
            simulate_delays: false,
            total_messages_sent: 0,
            messages_by_type: [0, 0, 0, 0],
            next_global_hash: 1, // Start from 1 (0 is reserved for genesis block)
        }
    }
    
    /// Send a message to a specific replica
    pub fn send(&mut self, msg: Message) {
        self.total_messages_sent += 1;
        
        // Update statistics
        let idx = match msg {
            Message::Proposal { .. } => 0,
            Message::Vote { .. } => 1,
            Message::QuorumCertBroadcast { .. } => 2,
            Message::NewView { .. } => 3,
        };
        self.messages_by_type[idx] += 1;
        
        self.message_queue.push_back(msg);
    }
    
    /// Broadcast a proposal to all replicas
    pub fn broadcast_proposal(&mut self, from: ReplicaId, block: Block) {
        for to in 0..self.num_replicas {
            if to as ReplicaId != from {
                self.send(Message::Proposal {
                    from,
                    to: to as ReplicaId,
                    block: block.clone(),
                });
            }
        }
    }
    
    /// Broadcast a QC to all replicas
    pub fn broadcast_qc(&mut self, from: ReplicaId, qc: QuorumCert, block_hash: Hash) {
        for to in 0..self.num_replicas {
            if to as ReplicaId != from {
                self.send(Message::QuorumCertBroadcast {
                    from,
                    to: to as ReplicaId,
                    qc: qc.clone(),
                    block_hash,
                });
            }
        }
    }
    
    /// Send a vote from a replica to the leader
    pub fn send_vote(&mut self, from: ReplicaId, to: ReplicaId, vote: Vote) {
        self.send(Message::Vote { from, to, vote });
    }
    
    /// Broadcast new view notification
    pub fn broadcast_new_view(&mut self, from: ReplicaId, view: u64, high_qc: Option<QuorumCert>) {
        for to in 0..self.num_replicas {
            if to as ReplicaId != from {
                self.send(Message::NewView {
                    from,
                    to: to as ReplicaId,
                    view,
                    high_qc: high_qc.clone(),
                });
            }
        }
    }
    
    /// Receive the next message from the queue
    pub fn receive(&mut self) -> Option<Message> {
        self.message_queue.pop_front()
    }
    
    /// Check if there are pending messages
    pub fn has_messages(&self) -> bool {
        !self.message_queue.is_empty()
    }
    
    /// Get the number of pending messages
    pub fn pending_count(&self) -> usize {
        self.message_queue.len()
    }
    
    /// Clear all pending messages (useful for testing)
    pub fn clear(&mut self) {
        self.message_queue.clear();
    }
    
    /// Generate a globally unique hash for a new block
    /// 
    /// This ensures that blocks from different replicas have unique hashes
    /// and prevents hash collisions that would break the 3-chain commit rule.
    pub fn generate_unique_hash(&mut self) -> Hash {
        let hash = self.next_global_hash;
        self.next_global_hash += 1;
        hash
    }
    
    /// Print network statistics
    pub fn print_stats(&self) {
        println!("\n=== Network Statistics ===");
        println!("Total messages sent: {}", self.total_messages_sent);
        println!("  Proposals: {}", self.messages_by_type[0]);
        println!("  Votes: {}", self.messages_by_type[1]);
        println!("  QC Broadcasts: {}", self.messages_by_type[2]);
        println!("  NewView: {}", self.messages_by_type[3]);
        println!("Pending messages: {}", self.pending_count());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::sign;
    
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
}
