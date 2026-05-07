//! gRPC adapter for the pluggable state machine interface.
//!
//! This module provides two halves of the gRPC bridge:
//!
//! - [`GrpcAppClient`] — an [`App`](crate::app::App) implementation that
//!   forwards every call to a remote `AppService` gRPC server.  This
//!   lets the consensus replica talk to an out-of-process application
//!   written in any language.
//!
//! - [`start_grpc_app_server`] / [`start_grpc_app_server_background`] —
//!   spin up a gRPC server that wraps any in-process [`App`] so it can
//!   be called from a remote consensus replica (or for integration tests).
//!
//! The wire protocol is defined in `proto/app_service.proto`.

use std::sync::{Arc, Mutex};

use crate::app::{App, AppError, CommittedCommand};
use crate::types::ConsensusCommand;

// ── Generated protobuf / gRPC stubs ─────────────────────────────────────

pub mod proto {
    tonic::include_proto!("persisthotstuff.app");
}

use proto::app_service_client::AppServiceClient;
use proto::app_service_server::{AppService, AppServiceServer};
use proto::{
    CommandPayload, Empty, ExecuteRequest, ExecuteResponse,
    JoinValidator as ProtoJoinValidator, RemoveValidator as ProtoRemoveValidator,
    RestoreRequest, RestoreResponse, SnapshotRequest, SnapshotResponse,
};

// ── Conversion helpers ───────────────────────────────────────────────────

fn consensus_command_to_proto(cmd: &ConsensusCommand) -> Option<CommandPayload> {
    let kind = match cmd {
        ConsensusCommand::NoOp => {
            proto::command_payload::Kind::NoOp(Empty {})
        }
        ConsensusCommand::ClientTx(payload) => {
            proto::command_payload::Kind::ClientTx(payload.clone())
        }
        ConsensusCommand::JoinValidator {
            replica_id,
            public_key,
        } => proto::command_payload::Kind::JoinValidator(ProtoJoinValidator {
            replica_id: *replica_id,
            public_key: public_key.clone(),
        }),
        ConsensusCommand::RemoveValidator { replica_id } => {
            proto::command_payload::Kind::RemoveValidator(ProtoRemoveValidator {
                replica_id: *replica_id,
            })
        }
    };
    Some(CommandPayload { kind: Some(kind) })
}

fn proto_to_consensus_command(payload: Option<CommandPayload>) -> ConsensusCommand {
    let Some(payload) = payload else {
        return ConsensusCommand::NoOp;
    };
    let Some(kind) = payload.kind else {
        return ConsensusCommand::NoOp;
    };
    match kind {
        proto::command_payload::Kind::NoOp(_) => ConsensusCommand::NoOp,
        proto::command_payload::Kind::ClientTx(s) => ConsensusCommand::ClientTx(s),
        proto::command_payload::Kind::JoinValidator(j) => ConsensusCommand::JoinValidator {
            replica_id: j.replica_id,
            public_key: j.public_key,
        },
        proto::command_payload::Kind::RemoveValidator(r) => {
            ConsensusCommand::RemoveValidator {
                replica_id: r.replica_id,
            }
        }
    }
}

fn committed_command_to_request(cmd: &CommittedCommand) -> ExecuteRequest {
    ExecuteRequest {
        block_hash: cmd.block_hash,
        view: cmd.view,
        epoch: cmd.epoch,
        proposer: cmd.proposer,
        commit_index: cmd.commit_index as u64,
        command: consensus_command_to_proto(&cmd.command),
    }
}

// ══════════════════════════════════════════════════════════════════════════
// GrpcAppClient — implements App by forwarding to a remote gRPC server
// ══════════════════════════════════════════════════════════════════════════

/// Client-side adapter that implements [`App`] by forwarding every call
/// to a remote `AppService` gRPC server.
///
/// Internally maintains a dedicated [`tokio::runtime::Runtime`] to bridge
/// the synchronous [`App`] trait with the async tonic transport.
///
/// # Example
///
/// ```no_run
/// use persisthotstuff_rst::grpc::GrpcAppClient;
///
/// let client = GrpcAppClient::connect("http://127.0.0.1:50051")
///     .expect("failed to connect");
/// ```
pub struct GrpcAppClient {
    runtime: tokio::runtime::Runtime,
    client: AppServiceClient<tonic::transport::Channel>,
    endpoint: String,
}

impl GrpcAppClient {
    /// Connect to a remote `AppService` gRPC server.
    ///
    /// # Arguments
    /// * `endpoint` — URI of the server, e.g. `"http://127.0.0.1:50051"`.
    pub fn connect(endpoint: &str) -> Result<Self, AppError> {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| AppError::Internal(format!("tokio runtime: {}", e)))?;

        let ep = endpoint.to_string();
        let client = runtime
            .block_on(async { AppServiceClient::connect(ep).await })
            .map_err(|e| AppError::ConnectionFailed(format!("{}", e)))?;

        Ok(GrpcAppClient {
            runtime,
            client,
            endpoint: endpoint.to_string(),
        })
    }

    /// The endpoint this client is connected to.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

impl App for GrpcAppClient {
    fn name(&self) -> &str {
        "grpc-remote"
    }

    fn apply(&mut self, cmd: &CommittedCommand) -> Result<Vec<u8>, AppError> {
        let request = tonic::Request::new(committed_command_to_request(cmd));
        let response = self
            .runtime
            .block_on(async { self.client.execute(request).await })
            .map_err(|e| AppError::Internal(format!("gRPC execute: {}", e)))?;

        let resp = response.into_inner();
        if resp.success {
            Ok(resp.result)
        } else {
            Err(AppError::ExecutionFailed(resp.error))
        }
    }

    fn snapshot(&self) -> Result<Vec<u8>, AppError> {
        // tonic clients require &mut, so clone the lightweight channel handle.
        let mut client = self.client.clone();
        let response = self
            .runtime
            .block_on(async move {
                client
                    .take_snapshot(tonic::Request::new(SnapshotRequest {}))
                    .await
            })
            .map_err(|e| AppError::Internal(format!("gRPC snapshot: {}", e)))?;

        Ok(response.into_inner().state)
    }

    fn restore(&mut self, state: &[u8]) -> Result<(), AppError> {
        let request = tonic::Request::new(RestoreRequest {
            state: state.to_vec(),
        });
        let response = self
            .runtime
            .block_on(async { self.client.restore_snapshot(request).await })
            .map_err(|e| AppError::Internal(format!("gRPC restore: {}", e)))?;

        let resp = response.into_inner();
        if resp.success {
            Ok(())
        } else {
            Err(AppError::SnapshotError(resp.error))
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// gRPC server — wraps any App as a tonic service
// ══════════════════════════════════════════════════════════════════════════

/// Internal tonic service implementation that delegates to an `App`.
struct AppServiceImpl {
    app: Arc<Mutex<Box<dyn App + 'static>>>,
}

#[tonic::async_trait]
impl AppService for AppServiceImpl {
    async fn execute(
        &self,
        request: tonic::Request<ExecuteRequest>,
    ) -> Result<tonic::Response<ExecuteResponse>, tonic::Status> {
        let req = request.into_inner();
        let cmd = CommittedCommand {
            block_hash: req.block_hash,
            view: req.view,
            epoch: req.epoch,
            proposer: req.proposer,
            command: proto_to_consensus_command(req.command),
            commit_index: req.commit_index as usize,
        };

        let mut app = self.app.lock().unwrap();
        match app.apply(&cmd) {
            Ok(result) => Ok(tonic::Response::new(ExecuteResponse {
                success: true,
                result,
                error: String::new(),
            })),
            Err(e) => Ok(tonic::Response::new(ExecuteResponse {
                success: false,
                result: vec![],
                error: e.to_string(),
            })),
        }
    }

    async fn take_snapshot(
        &self,
        _request: tonic::Request<SnapshotRequest>,
    ) -> Result<tonic::Response<SnapshotResponse>, tonic::Status> {
        let app = self.app.lock().unwrap();
        match app.snapshot() {
            Ok(state) => Ok(tonic::Response::new(SnapshotResponse { state })),
            Err(e) => Err(tonic::Status::internal(e.to_string())),
        }
    }

    async fn restore_snapshot(
        &self,
        request: tonic::Request<RestoreRequest>,
    ) -> Result<tonic::Response<RestoreResponse>, tonic::Status> {
        let req = request.into_inner();
        let mut app = self.app.lock().unwrap();
        match app.restore(&req.state) {
            Ok(()) => Ok(tonic::Response::new(RestoreResponse {
                success: true,
                error: String::new(),
            })),
            Err(e) => Ok(tonic::Response::new(RestoreResponse {
                success: false,
                error: e.to_string(),
            })),
        }
    }
}

/// Start a gRPC server that exposes an [`App`] as an `AppService`.
///
/// This is an async function — call it inside a `tokio` runtime.
///
/// # Arguments
/// * `app`  — the application to serve.
/// * `addr` — socket address to bind, e.g. `"127.0.0.1:50051"`.
pub async fn start_grpc_app_server(
    app: Box<dyn App + 'static>,
    addr: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let addr = addr.parse()?;
    let service = AppServiceImpl {
        app: Arc::new(Mutex::new(app)),
    };

    println!("AppService gRPC server listening on {}", addr);

    tonic::transport::Server::builder()
        .add_service(AppServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}

/// Start a gRPC server on a **background thread** and return a handle.
///
/// The server will keep running as long as the returned
/// [`GrpcServerHandle`] is alive.  This is the recommended entry
/// point for integration tests and examples.
///
/// # Arguments
/// * `app`  — the application to serve.
/// * `addr` — socket address to bind, e.g. `"127.0.0.1:50051"`.
pub fn start_grpc_app_server_background(
    app: Box<dyn App + 'static>,
    addr: String,
) -> Result<GrpcServerHandle, AppError> {
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<()>(1);

    let thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let addr_parsed: std::net::SocketAddr = addr.parse().expect("bad address");
            let service = AppServiceImpl {
                app: Arc::new(Mutex::new(app)),
            };

            // Signal that we are about to start serving.
            let _ = ready_tx.send(());

            tonic::transport::Server::builder()
                .add_service(AppServiceServer::new(service))
                .serve(addr_parsed)
                .await
                .expect("gRPC server failed");
        });
    });

    // Wait for the background thread to signal readiness.
    ready_rx
        .recv()
        .map_err(|e| AppError::Internal(format!("server start: {}", e)))?;

    // Brief pause to let the TCP listener actually bind.
    std::thread::sleep(std::time::Duration::from_millis(100));

    Ok(GrpcServerHandle {
        _thread: Some(thread),
    })
}

/// Handle for a background gRPC server.
///
/// The server thread will continue running as long as this handle exists.
/// Dropping the handle does **not** shut down the server (the thread is
/// detached).
pub struct GrpcServerHandle {
    _thread: Option<std::thread::JoinHandle<()>>,
}

// ── Utility ──────────────────────────────────────────────────────────────

/// Find a free TCP port on localhost.
///
/// Opens a listener on port 0, reads the assigned port, then drops the
/// listener.  There is a small TOCTOU race, but it is fine for tests.
pub fn find_free_port() -> u16 {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("bind to port 0");
    listener.local_addr().expect("local_addr").port()
}
