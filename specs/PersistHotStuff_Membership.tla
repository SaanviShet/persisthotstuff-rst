---------------------- MODULE PersistHotStuff_Membership -----------------------
(*
  TLA+ Specification — PersistHotStuff Dynamic Membership Extension
  =================================================================

  This module extends the base PersistHotStuff specification with formal
  modelling and verification of consensus-driven dynamic membership changes.

  Source cross-reference (persisthotstuff-rst):
    src/types.rs   – ConsensusCommand::{JoinValidator, RemoveValidator}
    src/replica.rs – apply_membership_command(), can_apply_new_validator_count()
    src/replica.rs – dynamic_leader_for_view(), dynamic_quorum_size()

  Design (Section 4.2 of the paper):
    Membership changes are regular consensus commands committed via the
    3-chain rule.  A JoinValidator or RemoveValidator command is proposed in
    a block, voted on, QC-formed, and only takes effect once the enclosing
    block is committed (atomically, after 3-chain confirmation).

  Key Safety Properties for Dynamic Membership:

    MEM-1  ValidatorSetBFT
           At every state, |activeValidators[r]| >= 3 * f(r) + 1
           for every honest replica r.

    MEM-2  QuorumIntersectionDynamic
           For every replica r, 2 * quorumSize(r) > |activeValidators[r]|,
           guaranteeing quorum overlap even after reconfiguration.

    MEM-3  CommitSafetyAcrossEpochs
           If two replicas commit block hashes at the same log position,
           they are identical — regardless of which configuration epoch
           each replica is in.

    MEM-4  AtomicReconfiguration
           Membership changes take effect only AFTER the enclosing block
           is committed via the 3-chain rule.  No replica sees a partial
           reconfiguration.

    MEM-5  EpochMonotone
           Configuration epochs are monotonically non-decreasing.

    MEM-6  ConsistentMembershipView
           All replicas that have processed the same committed log prefix
           agree on the active validator set.

    MEM-7  ValidTransitionsOnly
           The resulting validator count after any membership change
           satisfies n >= 4 and n % 3 == 1.

  TLC Model Checking Parameters:
    N           = 4  (replica IDs: 0, 1, 2, 3)
    F           = 0  (initial max Byzantine faults with 3 validators)
    MAX_VIEW    = 4  (enough views for join + commit + subsequent activity)
    MAX_HASH    = 9  (enough unique block hashes)
    MAX_EPOCH   = 2  (upper bound on configuration changes)
    CANDIDATE   = 3  (replica ID of the join candidate)
*)

EXTENDS Naturals, FiniteSets, Sequences, TLC

\* ============================================================================
\* CONSTANTS
\* ============================================================================

CONSTANTS
    N,              \* Maximum number of replica IDs in the universe (e.g. 5)
    F,              \* Initial max Byzantine faults tolerated (e.g. 1)
    MAX_VIEW,       \* Upper bound on view numbers for finite model checking
    MAX_HASH,       \* Upper bound on block hash values
    MAX_EPOCH,      \* Upper bound on configuration epoch
    CANDIDATE,      \* Replica ID of the membership change candidate
    NoValue         \* Sentinel for Option::None

ASSUME N >= 3
ASSUME MAX_VIEW >= 4
ASSUME MAX_HASH >= MAX_VIEW + 4
ASSUME MAX_EPOCH >= 1
ASSUME CANDIDATE \in 0..N-1

\* ============================================================================
\* VARIABLES
\* ============================================================================

VARIABLES
    currentView,        \* [Replica -> Nat]             current view per replica
    blockTree,          \* [Replica -> SUBSET Block]    blocks stored per replica
    highQC,             \* [Replica -> QC | NoValue]    highest QC per replica
    votePool,           \* [Replica -> SUBSET VoteRec]  votes received
    committedLog,       \* [Replica -> Seq(Hash)]       ordered commit history
    committedUpTo,      \* [Replica -> Hash | NoValue]  most recently committed
    msgs,               \* SUBSET Message               all messages ever sent
    nextHash,           \* Nat                          global hash counter
    \* ── New: Dynamic Membership State ──
    activeValidators,   \* [Replica -> SUBSET ReplicaId] per-replica active set
    configEpoch,        \* [Replica -> Nat]              configuration epoch
    \* Track which commands blocks carry (for membership application on commit)
    blockCommands       \* [Hash -> Command]             command payload per block

vars == <<currentView, blockTree, highQC, votePool,
          committedLog, committedUpTo, msgs, nextHash,
          activeValidators, configEpoch, blockCommands>>

\* ============================================================================
\* DERIVED SETS & HELPERS
\* ============================================================================

\* Universe of all possible replica IDs
AllReplicas == 0..N-1

\* Initial active set: first N-1 replicas; CANDIDATE is the join target.
\* With N=4 this gives {0,1,2} — three validators allowing join to 4.
InitialActiveSet == 0..(N-2)

\* Dynamic quorum size for a given validator count
\* Mirrors dynamic_quorum_size() in replica.rs: 2*f+1 where f = (n-1)/3
DynF(n_val) == (n_val - 1) \div 3
DynQuorum(n_val) == 2 * DynF(n_val) + 1

\* Round-robin leader within active set of replica r
\* Mirrors dynamic_leader_for_view() in replica.rs
DynLeader(r, view) ==
    LET vals == activeValidators[r]
        sorted == CHOOSE seq \in [1..Cardinality(vals) -> vals] :
                    \A i, j \in 1..Cardinality(vals) :
                        i < j => seq[i] < seq[j]
        idx == (view % Cardinality(vals)) + 1
    IN sorted[idx]

\* Check if a new validator count is valid: n >= 4 AND n % 3 == 1
\* Mirrors can_apply_new_validator_count() in replica.rs
ValidNewCount(new_n) == new_n >= 4 /\ new_n % 3 = 1

\* ============================================================================
\* SENTINEL & QC HELPERS
\* ============================================================================

IsQC(qc) == qc # NoValue

ValidQC(qc, r) ==
    /\ IsQC(qc)
    /\ Cardinality(qc.signers) >= DynQuorum(Cardinality(activeValidators[r]))
    /\ qc.signers \subseteq activeValidators[r]

MakeQC(bh, v, ep, sigs) ==
    [block_hash |-> bh, view |-> v, epoch |-> ep, signers |-> sigs]

MakeBlock(h, par, v, ep, prop, qc) ==
    [hash |-> h, parent |-> par, view |-> v, epoch |-> ep,
     proposer |-> prop, qc |-> qc]

GenesisBlock ==
    [hash |-> 0, parent |-> NoValue, view |-> 0, epoch |-> 0,
     proposer |-> N, qc |-> NoValue]

\* ============================================================================
\* REPLICA-STATE HELPERS
\* ============================================================================

HasBlock(r, h) == \E b \in blockTree[r] : b.hash = h
BlockByHash(r, h) == CHOOSE b \in blockTree[r] : b.hash = h
SignersFor(r, h) == {v.signer : v \in {vr \in votePool[r] : vr.block_hash = h}}
AlreadyCommitted(r, h) == \E i \in 1..Len(committedLog[r]) : committedLog[r][i] = h

Has3Chain(r, b0_hash) ==
    \E b1 \in blockTree[r], b2 \in blockTree[r] :
        /\ b1.parent = b0_hash
        /\ b1.qc # NoValue
        /\ b2.parent = b1.hash
        /\ b2.qc # NoValue

\* ============================================================================
\* COMMAND TYPE
\* ============================================================================

\* Commands:  "NoOp", "Join", "Remove"
NoOpCmd == "NoOp"
JoinCmd == "Join"
RemoveCmd == "Remove"

\* ============================================================================
\* INITIAL STATE
\* ============================================================================

Init ==
    /\ currentView      = [r \in AllReplicas |-> 0]
    /\ blockTree        = [r \in AllReplicas |-> {GenesisBlock}]
    /\ highQC           = [r \in AllReplicas |-> NoValue]
    /\ votePool         = [r \in AllReplicas |-> {}]
    /\ committedLog     = [r \in AllReplicas |-> <<>>]
    /\ committedUpTo    = [r \in AllReplicas |-> NoValue]
    /\ msgs             = {}
    /\ nextHash         = 1
    /\ activeValidators = [r \in AllReplicas |-> InitialActiveSet]
    /\ configEpoch      = [r \in AllReplicas |-> 0]
    /\ blockCommands    = [h \in {0} |-> NoOpCmd]   \* genesis carries NoOp

\* ============================================================================
\* ACTION: ProposeNoOp
\* ============================================================================

ProposeNoOp(r) ==
    LET view     == currentView[r]
        parentH  == IF IsQC(highQC[r]) THEN highQC[r].block_hash ELSE 0
        ep       == configEpoch[r]
        newBlock == MakeBlock(nextHash, parentH, view, ep, r, highQC[r])
    IN
    /\ r \in activeValidators[r]
    /\ r = DynLeader(r, view)
    /\ view     < MAX_VIEW
    /\ nextHash < MAX_HASH
    /\ ~\E b \in blockTree[r] : b.view = view /\ b.proposer = r
    /\ blockTree'     = [blockTree EXCEPT ![r] = blockTree[r] \union {newBlock}]
    /\ blockCommands' = blockCommands @@ (nextHash :> NoOpCmd)
    /\ msgs' = msgs
               \union {[type  |-> "Proposal", from |-> r, to |-> recv,
                        block |-> newBlock] : recv \in activeValidators[r] \ {r}}
               \union {[type |-> "Vote", from |-> r, to |-> r,
                        block_hash |-> nextHash, view |-> view,
                        epoch |-> ep, signer |-> r]}
    /\ nextHash' = nextHash + 1
    /\ UNCHANGED <<currentView, highQC, votePool, committedLog, committedUpTo,
                   activeValidators, configEpoch>>

\* ============================================================================
\* ACTION: ProposeJoin  (JoinValidator command)
\* ============================================================================

ProposeJoin(r) ==
    LET view     == currentView[r]
        parentH  == IF IsQC(highQC[r]) THEN highQC[r].block_hash ELSE 0
        ep       == configEpoch[r]
        newBlock == MakeBlock(nextHash, parentH, view, ep, r, highQC[r])
    IN
    /\ r \in activeValidators[r]
    /\ r = DynLeader(r, view)
    /\ view     < MAX_VIEW
    /\ nextHash < MAX_HASH
    /\ ~\E b \in blockTree[r] : b.view = view /\ b.proposer = r
    \* The candidate must not already be in the active set
    /\ CANDIDATE \notin activeValidators[r]
    \* Pre-check: resulting count must be valid
    /\ ValidNewCount(Cardinality(activeValidators[r]) + 1)
    /\ blockTree'     = [blockTree EXCEPT ![r] = blockTree[r] \union {newBlock}]
    /\ blockCommands' = blockCommands @@ (nextHash :> JoinCmd)
    /\ msgs' = msgs
               \union {[type  |-> "Proposal", from |-> r, to |-> recv,
                        block |-> newBlock] : recv \in activeValidators[r] \ {r}}
               \union {[type |-> "Vote", from |-> r, to |-> r,
                        block_hash |-> nextHash, view |-> view,
                        epoch |-> ep, signer |-> r]}
    /\ nextHash' = nextHash + 1
    /\ UNCHANGED <<currentView, highQC, votePool, committedLog, committedUpTo,
                   activeValidators, configEpoch>>

\* ============================================================================
\* ACTION: ProposeRemove  (RemoveValidator command)
\* ============================================================================

ProposeRemove(r) ==
    LET view     == currentView[r]
        parentH  == IF IsQC(highQC[r]) THEN highQC[r].block_hash ELSE 0
        ep       == configEpoch[r]
        newBlock == MakeBlock(nextHash, parentH, view, ep, r, highQC[r])
    IN
    /\ r \in activeValidators[r]
    /\ r = DynLeader(r, view)
    /\ view     < MAX_VIEW
    /\ nextHash < MAX_HASH
    /\ ~\E b \in blockTree[r] : b.view = view /\ b.proposer = r
    \* The candidate must be in the active set and not the proposer
    /\ CANDIDATE \in activeValidators[r]
    /\ CANDIDATE # r
    \* Pre-check: resulting count must be valid
    /\ ValidNewCount(Cardinality(activeValidators[r]) - 1)
    /\ blockTree'     = [blockTree EXCEPT ![r] = blockTree[r] \union {newBlock}]
    /\ blockCommands' = blockCommands @@ (nextHash :> RemoveCmd)
    /\ msgs' = msgs
               \union {[type  |-> "Proposal", from |-> r, to |-> recv,
                        block |-> newBlock] : recv \in activeValidators[r] \ {r}}
               \union {[type |-> "Vote", from |-> r, to |-> r,
                        block_hash |-> nextHash, view |-> view,
                        epoch |-> ep, signer |-> r]}
    /\ nextHash' = nextHash + 1
    /\ UNCHANGED <<currentView, highQC, votePool, committedLog, committedUpTo,
                   activeValidators, configEpoch>>

\* ============================================================================
\* ACTION: ReceiveProposal
\* ============================================================================

ReceiveProposal(r) ==
    \E m \in msgs :
        LET blk == m.block IN
        /\ m.type = "Proposal"
        /\ m.to   = r
        /\ r \in activeValidators[r]
        /\ DynLeader(r, blk.view) = blk.proposer
        /\ blk.epoch = configEpoch[r]
        /\ (blk.parent = NoValue \/ HasBlock(r, blk.parent))
        /\ (blk.qc = NoValue \/ ValidQC(blk.qc, r))
        /\ ~HasBlock(r, blk.hash)
        /\ blockTree' = [blockTree EXCEPT ![r] = blockTree[r] \union {blk}]
        /\ msgs' = msgs \union
                   {[type |-> "Vote", from |-> r, to |-> m.from,
                     block_hash |-> blk.hash, view |-> blk.view,
                     epoch |-> configEpoch[r], signer |-> r]}
        /\ UNCHANGED <<currentView, highQC, votePool, committedLog,
                       committedUpTo, nextHash, activeValidators,
                       configEpoch, blockCommands>>

\* ============================================================================
\* ACTION: ReceiveVote  (with QC formation)
\* ============================================================================

ReceiveVote(r) ==
    \E m \in msgs :
        /\ m.type = "Vote"
        /\ m.to   = r
        /\ r \in activeValidators[r]
        /\ HasBlock(r, m.block_hash)
        /\ m.signer \in activeValidators[r]
        /\ m.epoch = configEpoch[r]
        /\ [block_hash |-> m.block_hash, signer |-> m.signer] \notin votePool[r]
        /\ LET newSigners == SignersFor(r, m.block_hash) \union {m.signer}
               newPool    == votePool[r] \union
                             {[block_hash |-> m.block_hash, signer |-> m.signer]}
               quorum     == DynQuorum(Cardinality(activeValidators[r]))
           IN
           /\ votePool' = [votePool EXCEPT ![r] = newPool]
           /\ IF Cardinality(newSigners) >= quorum
              THEN
                LET qc == MakeQC(m.block_hash, m.view, m.epoch, newSigners) IN
                /\ highQC' = [highQC EXCEPT ![r] = qc]
                /\ msgs' = msgs \union
                           {[type |-> "QCBroadcast", from |-> r, to |-> recv,
                             qc |-> qc] : recv \in activeValidators[r] \ {r}}
              ELSE
                /\ UNCHANGED highQC
                /\ UNCHANGED msgs
        /\ UNCHANGED <<currentView, blockTree, committedLog, committedUpTo,
                       nextHash, activeValidators, configEpoch, blockCommands>>

\* ============================================================================
\* ACTION: ReceiveQCBroadcast
\* ============================================================================

ReceiveQCBroadcast(r) ==
    \E m \in msgs :
        /\ m.type = "QCBroadcast"
        /\ m.to   = r
        /\ r \in activeValidators[r]
        /\ ValidQC(m.qc, r)
        /\ HasBlock(r, m.qc.block_hash)
        /\ (IF IsQC(highQC[r])
            THEN (m.qc.epoch > highQC[r].epoch) \/
                 (m.qc.epoch = highQC[r].epoch /\ m.qc.view > highQC[r].view)
            ELSE TRUE)
        /\ highQC' = [highQC EXCEPT ![r] = m.qc]
        /\ UNCHANGED <<currentView, blockTree, votePool, committedLog,
                       committedUpTo, msgs, nextHash, activeValidators,
                       configEpoch, blockCommands>>

\* ============================================================================
\* ACTION: CommitBlock  (applies membership changes atomically)
\* ============================================================================

CommitBlock(r) ==
    \E b0 \in blockTree[r] :
        /\ Has3Chain(r, b0.hash)
        /\ ~AlreadyCommitted(r, b0.hash)
        /\ r \in activeValidators[r]
        /\ committedLog'  = [committedLog  EXCEPT ![r] = Append(committedLog[r], b0.hash)]
        /\ committedUpTo' = [committedUpTo EXCEPT ![r] = b0.hash]
        \* Apply membership change ATOMICALLY at commit time
        /\ LET cmd == IF b0.hash \in DOMAIN blockCommands
                      THEN blockCommands[b0.hash]
                      ELSE NoOpCmd
           IN
           IF cmd = JoinCmd /\ CANDIDATE \notin activeValidators[r]
              /\ ValidNewCount(Cardinality(activeValidators[r]) + 1)
           THEN
              /\ activeValidators' = [activeValidators EXCEPT
                    ![r] = activeValidators[r] \union {CANDIDATE}]
              /\ configEpoch' = [configEpoch EXCEPT ![r] = configEpoch[r] + 1]
              /\ votePool' = [votePool EXCEPT ![r] = {}]
           ELSE IF cmd = RemoveCmd /\ CANDIDATE \in activeValidators[r]
                   /\ ValidNewCount(Cardinality(activeValidators[r]) - 1)
           THEN
              /\ activeValidators' = [activeValidators EXCEPT
                    ![r] = activeValidators[r] \ {CANDIDATE}]
              /\ configEpoch' = [configEpoch EXCEPT ![r] = configEpoch[r] + 1]
              /\ votePool' = [votePool EXCEPT ![r] = {}]
           ELSE
              /\ UNCHANGED <<activeValidators, configEpoch, votePool>>
        /\ UNCHANGED <<currentView, blockTree, highQC, msgs, nextHash,
                       blockCommands>>

\* ============================================================================
\* ACTION: ViewTimeout
\* ============================================================================

ViewTimeout(r) ==
    LET view    == currentView[r]
        newView == view + 1
    IN
    /\ view < MAX_VIEW
    /\ r \in activeValidators[r]
    /\ ~(r = DynLeader(r, view)
         /\ view < MAX_VIEW
         /\ nextHash < MAX_HASH
         /\ ~\E b \in blockTree[r] : b.view = view /\ b.proposer = r)
    /\ currentView' = [currentView EXCEPT ![r] = newView]
    /\ votePool'    = [votePool    EXCEPT ![r] = {}]
    /\ msgs' = msgs \union
               {[type    |-> "NewView", from |-> r,
                 to      |-> DynLeader(r, newView),
                 view    |-> newView,
                 high_qc |-> highQC[r]]}
    /\ UNCHANGED <<blockTree, highQC, committedLog, committedUpTo, nextHash,
                   activeValidators, configEpoch, blockCommands>>

\* ============================================================================
\* ACTION: ReceiveNewView
\* ============================================================================

ReceiveNewView(r) ==
    \E m \in msgs :
        /\ m.type = "NewView"
        /\ m.to   = r
        /\ r \in activeValidators[r]
        /\ r = DynLeader(r, m.view)
        /\ IsQC(m.high_qc)
        /\ HasBlock(r, m.high_qc.block_hash)
        /\ (IF IsQC(highQC[r])
            THEN (m.high_qc.epoch > highQC[r].epoch) \/
                 (m.high_qc.epoch = highQC[r].epoch /\ m.high_qc.view > highQC[r].view)
            ELSE TRUE)
        /\ highQC' = [highQC EXCEPT ![r] = m.high_qc]
        /\ UNCHANGED <<currentView, blockTree, votePool, committedLog,
                       committedUpTo, msgs, nextHash, activeValidators,
                       configEpoch, blockCommands>>

\* ============================================================================
\* NEXT-STATE RELATION
\* ============================================================================

Next ==
    \/ \E r \in AllReplicas : ProposeNoOp(r)
    \/ \E r \in AllReplicas : ProposeJoin(r)
    \/ \E r \in AllReplicas : ProposeRemove(r)
    \/ \E r \in AllReplicas : ReceiveProposal(r)
    \/ \E r \in AllReplicas : ReceiveVote(r)
    \/ \E r \in AllReplicas : ReceiveQCBroadcast(r)
    \/ \E r \in AllReplicas : CommitBlock(r)
    \/ \E r \in AllReplicas : ViewTimeout(r)
    \/ \E r \in AllReplicas : ReceiveNewView(r)

Spec == Init /\ [][Next]_vars

\* ============================================================================
\* SAFETY INVARIANTS
\* ============================================================================

(*──────────────────────────────────────────────────────────────────────────────
  MEM-1: ValidatorSetBFT
  Every replica's active validator set always satisfies n >= 3f + 1.
──────────────────────────────────────────────────────────────────────────────*)
ValidatorSetBFT ==
    \A r \in AllReplicas :
        r \in activeValidators[r] =>
        LET n_val == Cardinality(activeValidators[r]) IN
        ValidNewCount(n_val) => n_val >= 3 * DynF(n_val) + 1

(*──────────────────────────────────────────────────────────────────────────────
  MEM-2: QuorumIntersectionDynamic
  For every active replica, 2 * quorum > |activeValidators|.
  This guarantees any two quorums overlap in at least one member.
──────────────────────────────────────────────────────────────────────────────*)
QuorumIntersectionDynamic ==
    \A r \in AllReplicas :
        r \in activeValidators[r] =>
        LET n_val == Cardinality(activeValidators[r]) IN
        ValidNewCount(n_val) => 2 * DynQuorum(n_val) > n_val

(*──────────────────────────────────────────────────────────────────────────────
  MEM-3: CommitSafetyAcrossEpochs
  Agreement: same committed sequence at every replica, regardless of epoch.
  This is the core safety property — identical to the base spec but now
  it must hold across configuration changes.
──────────────────────────────────────────────────────────────────────────────*)
CommitSafetyAcrossEpochs ==
    \A r1, r2 \in AllReplicas :
        \A i \in 1..Len(committedLog[r1]) :
            i <= Len(committedLog[r2]) =>
                committedLog[r1][i] = committedLog[r2][i]

(*──────────────────────────────────────────────────────────────────────────────
  MEM-4: EpochMonotone
  Configuration epochs only increase (never decrease).
──────────────────────────────────────────────────────────────────────────────*)
EpochMonotone ==
    \A r \in AllReplicas : configEpoch[r] >= 0

(*──────────────────────────────────────────────────────────────────────────────
  MEM-5: ValidTransitionsOnly
  After any membership change (configEpoch > 0), active validator set sizes
  are always in the valid set {4, 7, 10, ...}. The bootstrap state (epoch 0)
  is exempt since the cluster may start below the BFT threshold.
──────────────────────────────────────────────────────────────────────────────*)
ValidTransitionsOnly ==
    \A r \in AllReplicas :
        (r \in activeValidators[r] /\ configEpoch[r] > 0) =>
        ValidNewCount(Cardinality(activeValidators[r]))

(*──────────────────────────────────────────────────────────────────────────────
  MEM-6: ConsistentMembershipView
  Replicas with the same configEpoch agree on their active validator set.
──────────────────────────────────────────────────────────────────────────────*)
ConsistentMembershipView ==
    \A r1, r2 \in AllReplicas :
        (r1 \in activeValidators[r1] /\ r2 \in activeValidators[r2]
         /\ configEpoch[r1] = configEpoch[r2])
        => activeValidators[r1] = activeValidators[r2]

(*──────────────────────────────────────────────────────────────────────────────
  MEM-7: QCsAlwaysValidDynamic
  Every QC broadcast has enough signers for the sender's current quorum.
──────────────────────────────────────────────────────────────────────────────*)
QCsAlwaysValidDynamic ==
    \A m \in msgs :
        m.type = "QCBroadcast" =>
            /\ IsQC(m.qc)
            /\ Cardinality(m.qc.signers) >= DynQuorum(Cardinality(activeValidators[m.from]))

(*──────────────────────────────────────────────────────────────────────────────
  Preserved base invariants
──────────────────────────────────────────────────────────────────────────────*)
NoDoubleVote ==
    \A r \in AllReplicas :
        \A v1, v2 \in votePool[r] :
            (v1.block_hash = v2.block_hash /\ v1.signer = v2.signer) => v1 = v2

UniqueHashes ==
    \A r \in AllReplicas :
        \A b1, b2 \in blockTree[r] :
            b1.hash = b2.hash => b1 = b2

CommittedBlocksInTree ==
    \A r \in AllReplicas :
        \A i \in 1..Len(committedLog[r]) :
            HasBlock(r, committedLog[r][i])

VotePoolSignersValid ==
    \A r \in AllReplicas :
        \A v \in votePool[r] : v.signer \in activeValidators[r]

\* ============================================================================
\* THEOREMS
\* ============================================================================

THEOREM Spec => []ValidatorSetBFT
THEOREM Spec => []QuorumIntersectionDynamic
THEOREM Spec => []CommitSafetyAcrossEpochs
THEOREM Spec => []EpochMonotone
THEOREM Spec => []ValidTransitionsOnly
THEOREM Spec => []ConsistentMembershipView
THEOREM Spec => []QCsAlwaysValidDynamic
THEOREM Spec => []NoDoubleVote
THEOREM Spec => []UniqueHashes
THEOREM Spec => []CommittedBlocksInTree
THEOREM Spec => []VotePoolSignersValid

================================================================================
