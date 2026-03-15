//! Configuration module for the consensus protocol.
//!
//! Defines system parameters including replica count, fault tolerance,
//! and leader selection logic.

pub type ReplicaId = u64;

/// Configuration for the consensus protocol.
///
/// Contains the number of replicas (n), the number of faulty replicas (f),
/// the ID of the current replica, and timeout settings.
#[derive(Clone)]
pub struct Config {
    pub n: usize,
    pub f: usize,
    pub id: ReplicaId,
    pub timeout_ms: u64,
}

impl Config {
    /// Calculate the quorum size (2f + 1).
    ///
    /// The quorum size is the minimum number of replicas that must agree
    /// on a value for it to be considered committed.
    pub fn quorum_size(&self) -> usize {
        2 * self.f + 1
    }

    /// Determine the leader for a given view using round-robin selection.
    ///
    /// # Arguments
    /// * `view` - The view number
    ///
    /// # Returns
    /// The replica ID of the leader for this view
    pub fn leader_for_view(&self, view: u64) -> ReplicaId {
        let idx = (view as usize) % self.n;
        idx as ReplicaId
    }
}