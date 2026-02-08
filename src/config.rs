// This module defines the configuration for the consensus protocol,

pub type ReplicaId = u64;

// Configuation for the consensus protocol, 
// including the number of replicas (n), 
// the number of faulty replicas (f), and 
// the ID of the current replica.

#[derive(Clone)]
pub struct Config {
    pub n: usize,
    pub f: usize,
    pub id: ReplicaId,
}

// The quorum size is the minimum number of replicas 
// that must agree on a value for it to be considered committed.

impl Config {
    pub fn quorum_size(&self) -> usize {
        2 * self.f + 1
    }

    pub fn leader_for_view(&self, view: u64) -> ReplicaId {
        let idx = (view as usize) % self.n;
        idx as ReplicaId
    }
}