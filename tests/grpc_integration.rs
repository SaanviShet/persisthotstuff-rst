//! Integration tests for the gRPC pluggable state machine bridge.
//!
//! These tests start a gRPC AppService server in the background,
//! connect a `GrpcAppClient`, and verify the full round-trip for
//! Execute, TakeSnapshot, and RestoreSnapshot RPCs.

use persisthotstuff_rst::app::{App, CommittedCommand, KeyValueApp};
use persisthotstuff_rst::grpc::{find_free_port, start_grpc_app_server_background, GrpcAppClient};
use persisthotstuff_rst::types::ConsensusCommand;

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

#[test]
fn grpc_execute_roundtrip() {
    let port = find_free_port();
    let addr = format!("127.0.0.1:{}", port);

    let _server = start_grpc_app_server_background(
        Box::new(KeyValueApp::new()),
        addr.clone(),
    )
    .expect("failed to start gRPC server");

    let mut client =
        GrpcAppClient::connect(&format!("http://{}", addr)).expect("failed to connect");

    // Execute a SET command.
    let result = client
        .apply(&make_cmd(
            ConsensusCommand::ClientTx("SET hello world".into()),
            0,
        ))
        .unwrap();
    assert_eq!(result, b"OK");

    // Execute a GET command to verify.
    let result = client
        .apply(&make_cmd(
            ConsensusCommand::ClientTx("GET hello".into()),
            1,
        ))
        .unwrap();
    assert_eq!(String::from_utf8(result).unwrap(), "world");
}

#[test]
fn grpc_snapshot_and_restore() {
    let port = find_free_port();
    let addr = format!("127.0.0.1:{}", port);

    let _server = start_grpc_app_server_background(
        Box::new(KeyValueApp::new()),
        addr.clone(),
    )
    .expect("failed to start gRPC server");

    let mut client =
        GrpcAppClient::connect(&format!("http://{}", addr)).expect("failed to connect");

    // Insert some data.
    client
        .apply(&make_cmd(
            ConsensusCommand::ClientTx("SET a 1".into()),
            0,
        ))
        .unwrap();
    client
        .apply(&make_cmd(
            ConsensusCommand::ClientTx("SET b 2".into()),
            1,
        ))
        .unwrap();

    // Take a snapshot.
    let state = client.snapshot().unwrap();
    assert!(!state.is_empty());

    // Overwrite data.
    client
        .apply(&make_cmd(
            ConsensusCommand::ClientTx("SET a overwritten".into()),
            2,
        ))
        .unwrap();

    // Restore from snapshot — should revert to {a: 1, b: 2}.
    client.restore(&state).unwrap();

    let val = client
        .apply(&make_cmd(
            ConsensusCommand::ClientTx("GET a".into()),
            3,
        ))
        .unwrap();
    assert_eq!(String::from_utf8(val).unwrap(), "1");

    let val = client
        .apply(&make_cmd(
            ConsensusCommand::ClientTx("GET b".into()),
            4,
        ))
        .unwrap();
    assert_eq!(String::from_utf8(val).unwrap(), "2");
}

#[test]
fn grpc_noop_passthrough() {
    let port = find_free_port();
    let addr = format!("127.0.0.1:{}", port);

    let _server = start_grpc_app_server_background(
        Box::new(KeyValueApp::new()),
        addr.clone(),
    )
    .expect("failed to start gRPC server");

    let mut client =
        GrpcAppClient::connect(&format!("http://{}", addr)).expect("failed to connect");

    // NoOp should succeed with empty result.
    let result = client
        .apply(&make_cmd(ConsensusCommand::NoOp, 0))
        .unwrap();
    assert!(result.is_empty());
}

#[test]
fn grpc_membership_command_passthrough() {
    let port = find_free_port();
    let addr = format!("127.0.0.1:{}", port);

    let _server = start_grpc_app_server_background(
        Box::new(KeyValueApp::new()),
        addr.clone(),
    )
    .expect("failed to start gRPC server");

    let mut client =
        GrpcAppClient::connect(&format!("http://{}", addr)).expect("failed to connect");

    // Membership commands are passed to the app; KV ignores them.
    let result = client
        .apply(&make_cmd(
            ConsensusCommand::JoinValidator {
                replica_id: 42,
                public_key: vec![0u8; 32],
            },
            0,
        ))
        .unwrap();
    assert!(result.is_empty());

    let result = client
        .apply(&make_cmd(
            ConsensusCommand::RemoveValidator { replica_id: 42 },
            1,
        ))
        .unwrap();
    assert!(result.is_empty());
}

#[test]
fn grpc_client_reports_name() {
    let port = find_free_port();
    let addr = format!("127.0.0.1:{}", port);

    let _server = start_grpc_app_server_background(
        Box::new(KeyValueApp::new()),
        addr.clone(),
    )
    .expect("failed to start gRPC server");

    let client =
        GrpcAppClient::connect(&format!("http://{}", addr)).expect("failed to connect");

    assert_eq!(client.name(), "grpc-remote");
    assert!(client.endpoint().contains(&port.to_string()));
}
