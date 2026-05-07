//! Pluggable state machine interface for the consensus protocol.
//!
//! This module defines the [`App`] trait that any application can implement
//! to receive committed commands from the consensus layer.  Two reference
//! implementations are provided:
//!
//! - [`NoOpApp`] — silently ignores all commands (the default).
//! - [`KeyValueApp`] — a simple in-memory key-value store.
//!
//! For out-of-process applications written in any language, see the
//! [`grpc`](crate::grpc) module which exposes the same three operations
//! over gRPC (inspired by Tendermint's ABCI).

use crate::config::ReplicaId;
use crate::types::ConsensusCommand;
use std::collections::BTreeMap;
use std::fmt;

// ── Error type ───────────────────────────────────────────────────────────

/// Errors that can occur during application execution.
#[derive(Debug)]
pub enum AppError {
    /// The command payload is malformed or unrecognised.
    InvalidCommand(String),
    /// A deterministic execution error.
    ExecutionFailed(String),
    /// Snapshot serialisation or deserialisation failed.
    SnapshotError(String),
    /// Internal / unexpected error.
    Internal(String),
    /// gRPC connection failure.
    ConnectionFailed(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::InvalidCommand(msg) => write!(f, "invalid command: {}", msg),
            AppError::ExecutionFailed(msg) => write!(f, "execution failed: {}", msg),
            AppError::SnapshotError(msg) => write!(f, "snapshot error: {}", msg),
            AppError::Internal(msg) => write!(f, "internal error: {}", msg),
            AppError::ConnectionFailed(msg) => write!(f, "connection failed: {}", msg),
        }
    }
}

impl std::error::Error for AppError {}

// ── Committed command metadata ───────────────────────────────────────────

/// A committed command together with its block metadata.
///
/// Passed to [`App::apply`] so the application has full context about
/// when and where the command was ordered.
#[derive(Clone, Debug)]
pub struct CommittedCommand {
    pub block_hash: u64,
    pub view: u64,
    pub epoch: u64,
    pub proposer: ReplicaId,
    pub command: ConsensusCommand,
    pub commit_index: usize,
}

// ── App trait ────────────────────────────────────────────────────────────

/// The pluggable state machine trait.
///
/// Implementors define how committed commands are executed, how
/// application state is captured (snapshot), and how it is restored.
///
/// # Contract
///
/// - **`apply()` MUST be deterministic**: given the same sequence of
///   commands, every replica must produce identical state.  This is
///   critical for consensus correctness.
/// - **`snapshot()` and `restore()` must be inverses**: restoring a
///   snapshot and then re-applying subsequent commands must yield the
///   same state as executing all commands from genesis.
pub trait App: Send {
    /// Human-readable name for this application (for logging).
    fn name(&self) -> &str;

    /// Execute a committed command and return the result.
    ///
    /// Called exactly once for each block that passes the 3-chain commit
    /// rule, in strict commit order.  The returned bytes are opaque to
    /// the consensus layer and may be forwarded to the client.
    fn apply(&mut self, cmd: &CommittedCommand) -> Result<Vec<u8>, AppError>;

    /// Capture the current application state as an opaque byte blob.
    ///
    /// Called by the consensus layer when taking a periodic snapshot.
    fn snapshot(&self) -> Result<Vec<u8>, AppError>;

    /// Restore application state from a previously captured snapshot.
    ///
    /// Called during crash recovery before the system resumes.
    fn restore(&mut self, state: &[u8]) -> Result<(), AppError>;
}

// ══════════════════════════════════════════════════════════════════════════
// NoOpApp — the default application that ignores everything.
// ══════════════════════════════════════════════════════════════════════════

/// A no-op application that discards all commands.
///
/// This is the default when no application is configured.  It
/// demonstrates the minimum viable [`App`] implementation.
pub struct NoOpApp;

impl NoOpApp {
    pub fn new() -> Self {
        NoOpApp
    }
}

impl App for NoOpApp {
    fn name(&self) -> &str {
        "no-op"
    }

    fn apply(&mut self, _cmd: &CommittedCommand) -> Result<Vec<u8>, AppError> {
        Ok(vec![])
    }

    fn snapshot(&self) -> Result<Vec<u8>, AppError> {
        Ok(vec![])
    }

    fn restore(&mut self, _state: &[u8]) -> Result<(), AppError> {
        Ok(())
    }
}

// ══════════════════════════════════════════════════════════════════════════
// KeyValueApp — a simple in-memory key-value store.
// ══════════════════════════════════════════════════════════════════════════

/// A simple key-value store application.
///
/// Understands three command verbs encoded as strings in
/// [`ConsensusCommand::ClientTx`]:
///
/// | Verb | Syntax | Description |
/// |------|--------|-------------|
/// | SET  | `SET <key> <value>` | Insert or update a key. |
/// | GET  | `GET <key>` | Return the value (empty string if missing). |
/// | DEL  | `DEL <key>` | Remove a key. |
///
/// Membership commands (`JoinValidator`, `RemoveValidator`) and `NoOp`
/// are silently acknowledged without side-effects.
pub struct KeyValueApp {
    store: BTreeMap<String, String>,
}

impl KeyValueApp {
    pub fn new() -> Self {
        KeyValueApp {
            store: BTreeMap::new(),
        }
    }

    /// Read a value from the store (for testing / inspection).
    pub fn get(&self, key: &str) -> Option<&String> {
        self.store.get(key)
    }

    /// Number of entries in the store.
    pub fn len(&self) -> usize {
        self.store.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }
}

impl App for KeyValueApp {
    fn name(&self) -> &str {
        "key-value"
    }

    fn apply(&mut self, cmd: &CommittedCommand) -> Result<Vec<u8>, AppError> {
        match &cmd.command {
            ConsensusCommand::ClientTx(payload) => {
                let parts: Vec<&str> = payload.splitn(3, ' ').collect();
                match parts.first().map(|s| s.to_uppercase()).as_deref() {
                    Some("SET") => {
                        let key = parts
                            .get(1)
                            .ok_or_else(|| {
                                AppError::InvalidCommand("SET requires <key> <value>".into())
                            })?;
                        let value = parts
                            .get(2)
                            .ok_or_else(|| {
                                AppError::InvalidCommand("SET requires <key> <value>".into())
                            })?;
                        self.store.insert(key.to_string(), value.to_string());
                        Ok(b"OK".to_vec())
                    }
                    Some("GET") => {
                        let key = parts
                            .get(1)
                            .ok_or_else(|| {
                                AppError::InvalidCommand("GET requires <key>".into())
                            })?;
                        let value = self.store.get(*key).cloned().unwrap_or_default();
                        Ok(value.into_bytes())
                    }
                    Some("DEL") => {
                        let key = parts
                            .get(1)
                            .ok_or_else(|| {
                                AppError::InvalidCommand("DEL requires <key>".into())
                            })?;
                        self.store.remove(*key);
                        Ok(b"OK".to_vec())
                    }
                    _ => {
                        // Unknown verbs are silently accepted (not an error).
                        Ok(vec![])
                    }
                }
            }
            // Membership commands and NoOp are handled by the consensus
            // layer; the application just acknowledges them.
            _ => Ok(vec![]),
        }
    }

    fn snapshot(&self) -> Result<Vec<u8>, AppError> {
        bincode::serialize(&self.store)
            .map_err(|e| AppError::SnapshotError(e.to_string()))
    }

    fn restore(&mut self, state: &[u8]) -> Result<(), AppError> {
        self.store = bincode::deserialize(state)
            .map_err(|e| AppError::SnapshotError(e.to_string()))?;
        Ok(())
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ConsensusCommand;

    fn make_cmd(command: ConsensusCommand, idx: usize) -> CommittedCommand {
        CommittedCommand {
            block_hash: idx as u64,
            view: idx as u64,
            epoch: 0,
            proposer: 0,
            command,
            commit_index: idx,
        }
    }

    // ── NoOpApp ──────────────────────────────────────────────────────

    #[test]
    fn noop_app_ignores_everything() {
        let mut app = NoOpApp::new();
        let result = app
            .apply(&make_cmd(ConsensusCommand::ClientTx("hello".into()), 0))
            .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn noop_app_snapshot_roundtrip() {
        let mut app = NoOpApp::new();
        let state = app.snapshot().unwrap();
        app.restore(&state).unwrap();
    }

    // ── KeyValueApp ──────────────────────────────────────────────────

    #[test]
    fn kv_set_and_get() {
        let mut app = KeyValueApp::new();
        let r = app
            .apply(&make_cmd(
                ConsensusCommand::ClientTx("SET foo bar".into()),
                0,
            ))
            .unwrap();
        assert_eq!(r, b"OK");

        let r = app
            .apply(&make_cmd(
                ConsensusCommand::ClientTx("GET foo".into()),
                1,
            ))
            .unwrap();
        assert_eq!(String::from_utf8(r).unwrap(), "bar");
    }

    #[test]
    fn kv_del() {
        let mut app = KeyValueApp::new();
        app.apply(&make_cmd(
            ConsensusCommand::ClientTx("SET x 1".into()),
            0,
        ))
        .unwrap();
        assert_eq!(app.len(), 1);

        app.apply(&make_cmd(
            ConsensusCommand::ClientTx("DEL x".into()),
            1,
        ))
        .unwrap();
        assert_eq!(app.len(), 0);
    }

    #[test]
    fn kv_get_missing_key() {
        let mut app = KeyValueApp::new();
        let r = app
            .apply(&make_cmd(
                ConsensusCommand::ClientTx("GET missing".into()),
                0,
            ))
            .unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn kv_snapshot_and_restore() {
        let mut app = KeyValueApp::new();
        app.apply(&make_cmd(
            ConsensusCommand::ClientTx("SET a 1".into()),
            0,
        ))
        .unwrap();
        app.apply(&make_cmd(
            ConsensusCommand::ClientTx("SET b 2".into()),
            1,
        ))
        .unwrap();

        let state = app.snapshot().unwrap();

        // Restore into a fresh instance.
        let mut app2 = KeyValueApp::new();
        app2.restore(&state).unwrap();
        assert_eq!(app2.get("a").unwrap(), "1");
        assert_eq!(app2.get("b").unwrap(), "2");
    }

    #[test]
    fn kv_ignores_noop() {
        let mut app = KeyValueApp::new();
        let r = app
            .apply(&make_cmd(ConsensusCommand::NoOp, 0))
            .unwrap();
        assert!(r.is_empty());
        assert!(app.is_empty());
    }

    #[test]
    fn kv_ignores_membership_commands() {
        let mut app = KeyValueApp::new();
        let r = app
            .apply(&make_cmd(
                ConsensusCommand::JoinValidator {
                    replica_id: 99,
                    public_key: vec![0u8; 32],
                },
                0,
            ))
            .unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn kv_invalid_set_missing_value() {
        let mut app = KeyValueApp::new();
        let r = app.apply(&make_cmd(
            ConsensusCommand::ClientTx("SET onlykey".into()),
            0,
        ));
        assert!(r.is_err());
    }
}
