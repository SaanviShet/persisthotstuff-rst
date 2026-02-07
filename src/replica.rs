use std::collections::HashMap;
use crate::types::*;
use crate::config::*;

// Replica structure for the consensus protocol,
// Each replica maintains its configuration(n, f, id), 
// the current view number,
// a block tree to store the blocks it has seen,
// and the highest QC it has observed.
// It also maintains a vote pool to track votes received for each block hash.
pub struct Replica {
    pub config: Config,
    pub current_view: u64,
    pub block_tree: HashMap<Hash, Block>,
    pub high_qc: Option<QuorumCert>,
    pub vote_pool: HashMap<Hash, Vec<Signature>>,
}

use crate::visualiser::print_block_tree;
use crate::crypto::{Signature, sign, verify};
use crate::types::*;

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

    pub fn handle_vote(&mut self, vote: Vote) -> Option<QuorumCert> {
        if !verify(&vote.signature) {
            return None;
        }

        let sig = vote.signature;
        let entry = self.vote_pool.entry(vote.block_hash).or_insert_with(Vec::new);

        if entry.iter().any(|s| s.signer == sig.signer) {
            return None;
        }

        entry.push(sig);

        self.try_form_qc(vote.block_hash, vote.view)
    }

    fn try_form_qc(&mut self, block_hash: Hash, view: u64) -> Option<QuorumCert> {
        if let Some(sigs) = self.vote_pool.get(&block_hash) {
            if sigs.len() >= self.config.quorum_size() {
                let qc = QuorumCert { block_hash, view, signatures: sigs.clone() };
                self.high_qc = Some(qc.clone());
                return Some(qc);
            }
        }
        None
    }

    pub fn receive_vote_from_replica(&mut self, replica_id: ReplicaId, block_hash: Hash, view: u64) -> Option<QuorumCert> {
        let signature = sign(replica_id);
        let vote = Vote { block_hash, view, signature };
        self.handle_vote(vote)
    }
}

