//! ABCI++ Server Implementation for CometBFT Integration
//!
//! This module implements the ABCI++ protocol server that allows CometBFT
//! to communicate with the Helium blockchain application. It provides all
//! the necessary ABCI++ methods including the new PrepareProposal and
//! ProcessProposal for block proposal handling.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tonic::{transport::Server, Request, Response, Status};
// use tokio::io::{AsyncReadExt, AsyncWriteExt};
// use prost_types::Any;

use crate::consensus::ConsensusParamsManager;
use crate::snapshot::SnapshotManager;
use crate::validators::ValidatorManager;
use helium_baseapp::{BaseApp, Event};

use crate::config::AbciConfig;
use thiserror::Error;
use tracing::{debug, error, info};

/// ABCI++ server errors
#[derive(Error, Debug)]
pub enum AbciError {
    /// BaseApp error
    #[error("BaseApp error: {0}")]
    BaseApp(#[from] helium_baseapp::BaseAppError),

    /// Invalid request
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// Server error
    #[error("Server error: {0}")]
    ServerError(String),

    /// Codec error
    #[error("Codec error: {0}")]
    CodecError(String),

    /// Invalid transaction
    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),

    /// Insufficient fees
    #[error("Insufficient fees: {0}")]
    InsufficientFees(String),

    /// Invalid sequence
    #[error("Invalid sequence: {0}")]
    InvalidSequence(String),

    /// Unknown request
    #[error("Unknown request: {0}")]
    UnknownRequest(String),

    /// Internal error
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl AbciError {
    /// Get the error code for ABCI responses
    pub fn code(&self) -> u32 {
        match self {
            AbciError::InvalidTransaction(_) => 1,
            AbciError::InsufficientFees(_) => 2,
            AbciError::InvalidSequence(_) => 3,
            AbciError::UnknownRequest(_) => 4,
            AbciError::InternalError(_) => 99,
            _ => 100,
        }
    }
}

pub type Result<T> = std::result::Result<T, AbciError>;

/// ABCI++ Protocol Definitions
pub mod abci {
    tonic::include_proto!("cometbft.abci.v1");
}

use abci::{
    abci_service_server::{AbciService, AbciServiceServer},
    *,
};

/// ABCI++ Server implementation
#[derive(Clone)]
pub struct AbciServer {
    /// The base application
    app: Arc<RwLock<BaseApp>>,
    /// Chain ID
    chain_id: String,
    /// Initial height
    #[allow(dead_code)]
    initial_height: i64,
    /// Server configuration
    config: AbciConfig,
    /// Snapshot manager
    snapshot_manager: Option<Arc<SnapshotManager>>,
    /// Validator manager
    validator_manager: Arc<ValidatorManager>,
    /// Consensus parameter manager
    consensus_manager: Arc<ConsensusParamsManager>,
}

impl AbciServer {
    /// Create a new ABCI++ server
    pub fn new(app: BaseApp, chain_id: String) -> Self {
        Self::with_config(app, chain_id, AbciConfig::default())
    }

    /// Create a new ABCI++ server with configuration
    pub fn with_config(app: BaseApp, chain_id: String, config: AbciConfig) -> Self {
        // Initialize snapshot manager if snapshot directory is configured
        let snapshot_manager = if let Some(ref snapshot_dir) = config.snapshot_dir {
            match SnapshotManager::new(PathBuf::from(snapshot_dir)) {
                Ok(manager) => Some(Arc::new(manager)),
                Err(e) => {
                    error!("Failed to initialize snapshot manager: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Initialize validator manager
        let validator_manager = Arc::new(ValidatorManager::new(config.max_validators));

        // Initialize consensus parameter manager
        let consensus_manager = Arc::new(ConsensusParamsManager::new());

        Self {
            app: Arc::new(RwLock::new(app)),
            chain_id,
            initial_height: 1,
            config,
            snapshot_manager,
            validator_manager,
            consensus_manager,
        }
    }

    /// Start the ABCI++ server
    pub async fn start(self, addr: &str) -> Result<()> {
        let addr = addr
            .parse()
            .map_err(|e| AbciError::ServerError(format!("Invalid address: {e}")))?;

        info!("Starting ABCI++ server on {}", addr);

        Server::builder()
            .add_service(AbciServiceServer::new(self))
            .serve(addr)
            .await
            .map_err(|e| AbciError::ServerError(e.to_string()))?;

        Ok(())
    }

    /// Start the ABCI server with TCP connection handling
    pub async fn start_abci_server(
        app: Arc<RwLock<BaseApp>>,
        config: &AbciConfig,
        mut shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<()> {
        // Initialize snapshot manager if snapshot directory is configured
        let snapshot_manager = if let Some(ref snapshot_dir) = config.snapshot_dir {
            match SnapshotManager::new(PathBuf::from(snapshot_dir)) {
                Ok(manager) => Some(Arc::new(manager)),
                Err(e) => {
                    error!("Failed to initialize snapshot manager: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Initialize validator manager
        let validator_manager = Arc::new(ValidatorManager::new(config.max_validators));

        // Initialize consensus parameter manager
        let consensus_manager = Arc::new(ConsensusParamsManager::new());

        let server = AbciServer {
            app,
            chain_id: config.chain_id.clone(),
            initial_height: 1,
            config: config.clone(),
            snapshot_manager,
            validator_manager,
            consensus_manager,
        };

        // Parse listen address
        let addr: std::net::SocketAddr = config
            .listen_address
            .strip_prefix("tcp://")
            .unwrap_or(&config.listen_address)
            .parse()
            .map_err(|e| AbciError::ServerError(format!("Invalid address: {e}")))?;

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| AbciError::ServerError(format!("Failed to bind: {e}")))?;

        info!("ABCI server listening on {}", addr);

        // Accept connections with graceful shutdown
        loop {
            tokio::select! {
                // Handle incoming connections
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, peer_addr)) => {
                            let server_clone = server.clone();

                            tokio::spawn(async move {
                                if let Err(e) = handle_abci_connection(server_clone, stream, peer_addr).await {
                                    error!("ABCI connection error from {}: {}", peer_addr, e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept connection: {}", e);
                            continue;
                        }
                    }
                }
                // Handle shutdown signal
                _ = &mut shutdown_rx => {
                    info!("Received shutdown signal, stopping ABCI server");
                    break;
                }
            }
        }

        info!("ABCI server shutdown complete");
        Ok(())
    }
}

#[tonic::async_trait]
impl AbciService for AbciServer {
    /// Echo returns the same message as provided
    async fn echo(
        &self,
        request: Request<EchoRequest>,
    ) -> std::result::Result<Response<EchoResponse>, Status> {
        let req = request.into_inner();
        debug!("ABCI Echo: {}", req.message);

        Ok(Response::new(EchoResponse {
            message: req.message,
        }))
    }

    /// Flush is a no-op
    async fn flush(
        &self,
        _request: Request<FlushRequest>,
    ) -> std::result::Result<Response<FlushResponse>, Status> {
        debug!("ABCI Flush");
        Ok(Response::new(FlushResponse {}))
    }

    /// Info returns information about the application state
    async fn info(
        &self,
        _request: Request<InfoRequest>,
    ) -> std::result::Result<Response<InfoResponse>, Status> {
        debug!("ABCI Info");

        let app = self.app.read().await;
        let height = app.get_height();
        let app_hash = app.get_last_app_hash().to_vec();

        Ok(Response::new(InfoResponse {
            data: "helium".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            app_version: 1,
            last_block_height: height as i64,
            last_block_app_hash: app_hash,
        }))
    }

    /// InitChain is called once upon genesis
    async fn init_chain(
        &self,
        request: Request<InitChainRequest>,
    ) -> std::result::Result<Response<InitChainResponse>, Status> {
        let req = request.into_inner();
        info!(
            "ABCI InitChain: chain_id={}, height={}",
            req.chain_id, req.initial_height
        );

        // Validate chain ID
        if req.chain_id != self.chain_id {
            return Err(Status::invalid_argument(format!(
                "Invalid chain ID: expected {}, got {}",
                self.chain_id, req.chain_id
            )));
        }

        let mut app = self.app.write().await;

        // Initialize the chain with genesis data
        app.init_chain(req.chain_id.clone(), &req.app_state_bytes)
            .map_err(|e| Status::internal(format!("Failed to initialize chain: {e}")))?;

        // Store initial height
        let initial_height = if req.initial_height > 0 {
            req.initial_height
        } else {
            1
        };

        drop(app);

        // Initialize consensus parameters if provided
        if let Some(params) = req.consensus_params.clone() {
            if let Err(e) = self.consensus_manager.init(params).await {
                error!("Failed to initialize consensus parameters: {}", e);
                return Err(Status::internal(format!(
                    "Failed to initialize consensus parameters: {e}"
                )));
            }
            info!("Initialized consensus parameters from genesis");
        }

        // Initialize validators if provided
        if !req.validators.is_empty() {
            for validator in &req.validators {
                if let Err(e) = self
                    .validator_manager
                    .update_validator(
                        validator.address.clone(),
                        validator.power,
                        vec![], // Public key would be provided separately in a real implementation
                        initial_height as u64,
                    )
                    .await
                {
                    error!("Failed to initialize validator: {}", e);
                    return Err(Status::internal(format!(
                        "Failed to initialize validator: {e}"
                    )));
                }
            }
            info!(
                "Initialized {} validators from genesis",
                req.validators.len()
            );
        }

        // Get initial validator set
        let validators = self.validator_manager.get_validators().await;

        Ok(Response::new(InitChainResponse {
            consensus_params: req.consensus_params,
            validators,
            app_hash: vec![],
        }))
    }

    /// Query allows the application to expose information
    async fn query(
        &self,
        request: Request<QueryRequest>,
    ) -> std::result::Result<Response<abci::QueryResponse>, Status> {
        let req = request.into_inner();
        debug!("ABCI Query: path={}, height={}", req.path, req.height);

        let app = self.app.read().await;

        // Parse query path (e.g., /cosmos.bank.v1beta1.Query/Balance)
        let parts: Vec<&str> = req.path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(Status::invalid_argument("Empty query path"));
        }

        // Route query based on path structure
        let response = if req.path.starts_with("/cosmos.") || req.path.starts_with("/helium.") {
            // gRPC-style query paths for modules
            match app.query(req.path.clone(), &req.data, req.height as u64, req.prove) {
                Ok(result) => result,
                Err(e) => {
                    return Ok(Response::new(abci::QueryResponse {
                        code: 1,
                        log: e.to_string(),
                        info: String::new(),
                        index: 0,
                        key: req.data,
                        value: vec![],
                        proof_ops: None,
                        height: req.height,
                        codespace: String::new(),
                    }));
                }
            }
        } else {
            // Legacy query paths
            match parts[0] {
                "app" => {
                    // Application-specific queries
                    app.query(req.path, &req.data, req.height as u64, req.prove)
                        .map_err(|e| Status::internal(format!("Query failed: {e}")))?
                }
                "store" => {
                    // Direct store queries
                    // Format: /store/{store_name}/key or /store/{store_name}/subspace
                    if parts.len() < 3 {
                        return Err(Status::invalid_argument(
                            "Store query requires format: /store/{store_name}/{key|subspace}",
                        ));
                    }

                    let store_name = parts[1];
                    let query_type = parts[2];

                    match query_type {
                        "key" => {
                            // Query a specific key from the store
                            let query_path =
                                format!("/cosmos.base.store.v1beta1.Query/Get/{store_name}");
                            app.query(query_path, &req.data, req.height as u64, req.prove)
                                .unwrap_or_else(|e| helium_baseapp::QueryResponse {
                                    code: 1,
                                    log: format!("Store key query failed: {e}"),
                                    value: vec![],
                                    height: req.height as u64,
                                    proof: None,
                                })
                        }
                        "subspace" => {
                            // Query a range of keys with a prefix
                            let query_path =
                                format!("/cosmos.base.store.v1beta1.Query/List/{store_name}");
                            app.query(query_path, &req.data, req.height as u64, false)
                                .unwrap_or_else(|e| helium_baseapp::QueryResponse {
                                    code: 1,
                                    log: format!("Store subspace query failed: {e}"),
                                    value: vec![],
                                    height: req.height as u64,
                                    proof: None,
                                })
                        }
                        _ => helium_baseapp::QueryResponse {
                            code: 1,
                            log: format!("Unknown store query type: {query_type}"),
                            value: vec![],
                            height: req.height as u64,
                            proof: None,
                        },
                    }
                }
                "custom" => {
                    // Custom application queries
                    app.query(req.path, &req.data, req.height as u64, req.prove)
                        .map_err(|e| Status::internal(format!("Query failed: {e}")))?
                }
                _ => {
                    return Err(Status::unimplemented(format!(
                        "Unknown query path: {}",
                        req.path
                    )));
                }
            }
        };

        let query_key = req.data.clone();
        Ok(Response::new(abci::QueryResponse {
            code: response.code,
            log: response.log,
            info: String::new(),
            index: 0,
            key: req.data,
            value: response.value,
            proof_ops: response.proof.map(|proof_bytes| {
                // Convert proof bytes to ProofOps
                // For now, we'll create a simple proof op
                // In a real implementation, this would parse the actual merkle proof
                let proof_op = abci::ProofOp {
                    r#type: "iavl:v".to_string(),
                    key: query_key.clone(),
                    data: proof_bytes,
                };
                abci::ProofOps {
                    ops: vec![proof_op],
                }
            }),
            height: response.height as i64,
            codespace: String::new(),
        }))
    }

    /// CheckTx validates a transaction for the mempool
    async fn check_tx(
        &self,
        request: Request<CheckTxRequest>,
    ) -> std::result::Result<Response<CheckTxResponse>, Status> {
        let req = request.into_inner();
        debug!("ABCI CheckTx: {} bytes, type={}", req.tx.len(), req.r#type);

        let app = self.app.read().await;
        let result = app
            .check_tx(&req.tx)
            .map_err(|e| Status::internal(format!("CheckTx failed: {e}")))?;

        Ok(Response::new(CheckTxResponse {
            code: result.code,
            data: vec![],
            log: result.log,
            info: String::new(),
            gas_wanted: result.gas_wanted as i64,
            gas_used: result.gas_used as i64,
            events: convert_events(result.events),
            codespace: String::new(),
        }))
    }

    /// Commit persists the application state
    async fn commit(
        &self,
        _request: Request<CommitRequest>,
    ) -> std::result::Result<Response<CommitResponse>, Status> {
        debug!("ABCI Commit");

        let mut app = self.app.write().await;
        let _app_hash = app
            .commit()
            .map_err(|e| Status::internal(format!("Commit failed: {e}")))?;

        let height = app.get_height();

        // Drop the write lock before creating snapshot to avoid holding it too long
        drop(app);

        // Create snapshot if configured and at the right interval
        if let Some(ref snapshot_manager) = self.snapshot_manager {
            if self.config.snapshot_interval > 0
                && height.is_multiple_of(self.config.snapshot_interval)
            {
                info!("Creating snapshot at height {}", height);

                // Create snapshot asynchronously to avoid blocking consensus
                let app_clone = self.app.clone();
                let snapshot_manager_clone = snapshot_manager.clone();
                let height_clone = height;

                tokio::spawn(async move {
                    match snapshot_manager_clone
                        .create_snapshot(app_clone, height_clone)
                        .await
                    {
                        Ok(metadata) => {
                            info!(
                                "Snapshot created at height {} with {} chunks, hash: {}",
                                metadata.height,
                                metadata.chunks,
                                hex::encode(&metadata.hash)
                            );
                        }
                        Err(e) => {
                            error!(
                                "Failed to create snapshot at height {}: {}",
                                height_clone, e
                            );
                        }
                    }
                });
            }
        }

        // Calculate retain height based on configuration
        let retain_height = if self.config.retain_blocks > 0 {
            height.saturating_sub(self.config.retain_blocks) as i64
        } else {
            0 // Retain all blocks
        };

        Ok(Response::new(CommitResponse { retain_height }))
    }

    /// ListSnapshots returns available snapshots
    async fn list_snapshots(
        &self,
        _request: Request<ListSnapshotsRequest>,
    ) -> std::result::Result<Response<ListSnapshotsResponse>, Status> {
        debug!("ABCI ListSnapshots");

        let snapshots = if let Some(ref snapshot_manager) = self.snapshot_manager {
            let snapshot_list = snapshot_manager.list_snapshots().await;
            snapshot_list
                .into_iter()
                .map(|metadata| Snapshot {
                    height: metadata.height,
                    format: metadata.format,
                    chunks: metadata.chunks,
                    hash: metadata.hash,
                    metadata: metadata.metadata,
                })
                .collect()
        } else {
            vec![]
        };

        Ok(Response::new(ListSnapshotsResponse { snapshots }))
    }

    /// OfferSnapshot is called when a snapshot is available from peers
    async fn offer_snapshot(
        &self,
        request: Request<OfferSnapshotRequest>,
    ) -> std::result::Result<Response<OfferSnapshotResponse>, Status> {
        let req = request.into_inner();
        let snapshot = req
            .snapshot
            .ok_or_else(|| Status::invalid_argument("Missing snapshot"))?;

        debug!(
            "ABCI OfferSnapshot: height={}, format={}, chunks={}",
            snapshot.height, snapshot.format, snapshot.chunks
        );

        // Check if we have snapshot support enabled
        let _snapshot_manager = match &self.snapshot_manager {
            Some(manager) => manager,
            None => {
                info!("Snapshot support not enabled, rejecting offer");
                return Ok(Response::new(OfferSnapshotResponse {
                    result: OfferSnapshotResult::Abort.into(),
                }));
            }
        };

        // Get current height
        let app = self.app.read().await;
        let current_height = app.get_height();
        drop(app);

        // Validate snapshot
        if snapshot.height == 0 {
            return Ok(Response::new(OfferSnapshotResponse {
                result: OfferSnapshotResult::Reject.into(),
            }));
        }

        // Check if snapshot is too old
        if current_height > 0 && snapshot.height < current_height {
            info!(
                "Rejecting snapshot at height {} (current height: {})",
                snapshot.height, current_height
            );
            return Ok(Response::new(OfferSnapshotResponse {
                result: OfferSnapshotResult::Reject.into(),
            }));
        }

        // Check format
        if snapshot.format != 1 {
            info!(
                "Rejecting snapshot with unsupported format: {}",
                snapshot.format
            );
            return Ok(Response::new(OfferSnapshotResponse {
                result: OfferSnapshotResult::RejectFormat.into(),
            }));
        }

        // Accept the snapshot for now
        // In production, you would want to verify the app hash
        info!(
            "Accepting snapshot at height {} with {} chunks",
            snapshot.height, snapshot.chunks
        );

        Ok(Response::new(OfferSnapshotResponse {
            result: OfferSnapshotResult::Accept.into(),
        }))
    }

    /// LoadSnapshotChunk loads a chunk of a snapshot
    async fn load_snapshot_chunk(
        &self,
        request: Request<LoadSnapshotChunkRequest>,
    ) -> std::result::Result<Response<LoadSnapshotChunkResponse>, Status> {
        let req = request.into_inner();
        debug!(
            "ABCI LoadSnapshotChunk: height={}, format={}, chunk={}",
            req.height, req.format, req.chunk
        );

        let snapshot_manager = match &self.snapshot_manager {
            Some(manager) => manager,
            None => {
                return Ok(Response::new(LoadSnapshotChunkResponse { chunk: vec![] }));
            }
        };

        // Load the requested chunk
        match snapshot_manager.load_chunk(req.height, req.chunk).await {
            Ok(chunk_data) => {
                debug!(
                    "Loaded chunk {} for snapshot at height {} ({} bytes)",
                    req.chunk,
                    req.height,
                    chunk_data.len()
                );
                Ok(Response::new(LoadSnapshotChunkResponse {
                    chunk: chunk_data,
                }))
            }
            Err(e) => {
                error!(
                    "Failed to load chunk {} for snapshot at height {}: {}",
                    req.chunk, req.height, e
                );
                Ok(Response::new(LoadSnapshotChunkResponse { chunk: vec![] }))
            }
        }
    }

    /// ApplySnapshotChunk applies a chunk of a snapshot
    async fn apply_snapshot_chunk(
        &self,
        request: Request<ApplySnapshotChunkRequest>,
    ) -> std::result::Result<Response<ApplySnapshotChunkResponse>, Status> {
        let req = request.into_inner();
        debug!(
            "ABCI ApplySnapshotChunk: chunk index={}, {} bytes, sender={}",
            req.index,
            req.chunk.len(),
            req.sender
        );

        // Check if we have snapshot support
        let _snapshot_manager = match &self.snapshot_manager {
            Some(manager) => manager,
            None => {
                return Ok(Response::new(ApplySnapshotChunkResponse {
                    result: ApplySnapshotChunkResult::AbortResult.into(),
                    refetch_chunks: vec![],
                    reject_senders: vec![],
                }));
            }
        };

        // In a real implementation, we would:
        // 1. Store the chunk temporarily
        // 2. Verify chunk integrity
        // 3. When all chunks are received, reconstruct and apply the snapshot
        // 4. Verify the final state hash matches

        // For now, we'll accept chunks but not actually apply them
        // This prevents the node from getting stuck during state sync
        info!(
            "Received chunk {} ({} bytes) - snapshot restoration not fully implemented",
            req.index,
            req.chunk.len()
        );

        Ok(Response::new(ApplySnapshotChunkResponse {
            result: ApplySnapshotChunkResult::AcceptResult.into(),
            refetch_chunks: vec![],
            reject_senders: vec![],
        }))
    }

    /// PrepareProposal allows the application to modify transactions before proposing a block
    async fn prepare_proposal(
        &self,
        request: Request<PrepareProposalRequest>,
    ) -> std::result::Result<Response<PrepareProposalResponse>, Status> {
        let req = request.into_inner();
        info!(
            "ABCI PrepareProposal: height={}, {} txs, max_bytes={}",
            req.height,
            req.txs.len(),
            req.max_tx_bytes
        );

        let app = self.app.read().await;

        // Transaction selection and reordering logic
        let mut selected_txs = Vec::new();
        let mut total_bytes = 0i64;

        // Sort transactions by gas price (descending) for better block rewards
        let mut tx_candidates: Vec<(Vec<u8>, u64, f64)> = Vec::new();

        for tx in req.txs {
            // Check transaction validity
            match app.check_tx(&tx) {
                Ok(result) if result.code == 0 => {
                    // Transaction is valid
                    // Calculate gas price (fee / gas_wanted)
                    // For now, we'll use a simple heuristic based on gas_wanted
                    // In a real implementation, we would decode the tx to get the fee
                    let gas_wanted = result.gas_wanted.max(1); // Avoid division by zero

                    // TODO: Decode transaction to extract actual fee amount
                    // For now, use a placeholder calculation
                    let estimated_fee = gas_wanted as f64 * 0.01; // Placeholder fee calculation
                    let gas_price = estimated_fee / gas_wanted as f64;

                    tx_candidates.push((tx, gas_wanted, gas_price));
                }
                Ok(result) => {
                    debug!(
                        "Excluding invalid transaction from proposal: code={}, log={}",
                        result.code, result.log
                    );
                }
                Err(e) => {
                    debug!("Failed to check transaction: {}", e);
                }
            }
        }

        drop(app);

        // Sort by gas price (priority) - higher gas price transactions first
        tx_candidates.sort_by(|a, b| {
            b.2.partial_cmp(&a.2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.1.cmp(&a.1)) // Secondary sort by gas_wanted if prices are equal
        });

        let num_candidates = tx_candidates.len();

        // Select transactions that fit within the byte limit
        for (tx, _gas, _price) in tx_candidates {
            let tx_size = tx.len() as i64;
            if total_bytes + tx_size <= req.max_tx_bytes {
                total_bytes += tx_size;
                selected_txs.push(tx);
            } else {
                debug!(
                    "Transaction doesn't fit in block: size={}, remaining_space={}",
                    tx_size,
                    req.max_tx_bytes - total_bytes
                );
                break;
            }
        }
        info!(
            "PrepareProposal selected {} transactions ({} bytes) from {} candidates",
            selected_txs.len(),
            total_bytes,
            num_candidates
        );

        Ok(Response::new(PrepareProposalResponse { txs: selected_txs }))
    }

    /// ProcessProposal allows the application to validate a proposed block
    async fn process_proposal(
        &self,
        request: Request<ProcessProposalRequest>,
    ) -> std::result::Result<Response<ProcessProposalResponse>, Status> {
        let req = request.into_inner();
        info!(
            "ABCI ProcessProposal: height={}, {} txs, proposer={}",
            req.height,
            req.txs.len(),
            hex::encode(&req.proposer_address)
        );

        // Verify proposer is a valid validator (skip check if no validators are set yet)
        let total_power = self.validator_manager.get_total_power().await;
        if total_power > 0
            && !self
                .validator_manager
                .is_validator(&req.proposer_address)
                .await
        {
            error!(
                "Proposal from non-validator address: {}",
                hex::encode(&req.proposer_address)
            );
            return Ok(Response::new(ProcessProposalResponse {
                status: ProcessProposalStatus::RejectProposal.into(),
            }));
        }

        let app = self.app.read().await;

        // Validate all transactions in the proposal
        let mut invalid_count = 0;
        let mut total_gas_wanted = 0u64;

        for (idx, tx) in req.txs.iter().enumerate() {
            match app.check_tx(tx) {
                Ok(result) => {
                    if result.code != 0 {
                        error!(
                            "Invalid transaction {} in proposal: code={}, log={}",
                            idx, result.code, result.log
                        );
                        invalid_count += 1;
                    } else {
                        total_gas_wanted = total_gas_wanted.saturating_add(result.gas_wanted);
                    }
                }
                Err(e) => {
                    error!("Failed to validate transaction {} in proposal: {}", idx, e);
                    invalid_count += 1;
                }
            }
        }

        drop(app);

        // Reject proposal if it contains invalid transactions
        let status = if invalid_count > 0 {
            error!(
                "Rejecting proposal with {} invalid transactions out of {}",
                invalid_count,
                req.txs.len()
            );
            ProcessProposalStatus::RejectProposal
        } else {
            // Additional validation checks could be added here:
            // - Check if proposer is authorized
            // - Verify block doesn't exceed gas limits
            // - Check for duplicate transactions
            // - Validate against application-specific rules

            info!(
                "Accepting valid proposal with {} transactions (total gas: {})",
                req.txs.len(),
                total_gas_wanted
            );
            ProcessProposalStatus::AcceptProposal
        };

        Ok(Response::new(ProcessProposalResponse {
            status: status.into(),
        }))
    }

    /// ExtendVote allows applications to include additional data in precommit votes
    async fn extend_vote(
        &self,
        request: Request<ExtendVoteRequest>,
    ) -> std::result::Result<Response<ExtendVoteResponse>, Status> {
        let req = request.into_inner();
        debug!(
            "ABCI ExtendVote: height={}, hash={}",
            req.height,
            hex::encode(&req.hash)
        );

        // Vote extensions can be used for various purposes:
        // 1. Oracle data inclusion
        // 2. Threshold decryption shares
        // 3. Cross-chain communication
        // 4. Additional consensus information

        // For now, we'll create a simple vote extension with app-specific data
        let vote_extension = if self.config.chain_id.contains("oracle") {
            // Example: Include oracle price data in vote extensions
            let oracle_data = serde_json::json!({
                "timestamp": req.time.as_ref().map(|t| t.seconds).unwrap_or(0),
                "height": req.height,
                "prices": {
                    "ATOM/USD": "10.50",
                    "ETH/USD": "2500.00"
                }
            });

            serde_json::to_vec(&oracle_data).unwrap_or_default()
        } else {
            // No vote extension for non-oracle chains
            vec![]
        };

        Ok(Response::new(ExtendVoteResponse { vote_extension }))
    }

    /// VerifyVoteExtension verifies application-specific vote extension data
    async fn verify_vote_extension(
        &self,
        request: Request<VerifyVoteExtensionRequest>,
    ) -> std::result::Result<Response<VerifyVoteExtensionResponse>, Status> {
        let req = request.into_inner();
        debug!(
            "ABCI VerifyVoteExtension: height={}, validator={}, extension_size={}",
            req.height,
            hex::encode(&req.validator_address),
            req.vote_extension.len()
        );

        // Verify vote extension based on chain type
        let status = if self.config.chain_id.contains("oracle") && !req.vote_extension.is_empty() {
            // Verify oracle data format
            match serde_json::from_slice::<serde_json::Value>(&req.vote_extension) {
                Ok(data) => {
                    // Basic validation of oracle data structure
                    if data.get("timestamp").is_some()
                        && data.get("height").is_some()
                        && data.get("prices").is_some()
                    {
                        debug!(
                            "Valid oracle vote extension from validator {}",
                            hex::encode(&req.validator_address)
                        );
                        VerifyVoteExtensionStatus::AcceptVote
                    } else {
                        debug!(
                            "Invalid oracle data structure from validator {}",
                            hex::encode(&req.validator_address)
                        );
                        VerifyVoteExtensionStatus::RejectVote
                    }
                }
                Err(e) => {
                    debug!("Failed to parse vote extension: {}", e);
                    VerifyVoteExtensionStatus::RejectVote
                }
            }
        } else if !self.config.chain_id.contains("oracle") && req.vote_extension.is_empty() {
            // Non-oracle chains should have empty extensions
            VerifyVoteExtensionStatus::AcceptVote
        } else {
            // Mismatch between chain type and extension presence
            debug!("Vote extension mismatch for chain {}", self.config.chain_id);
            VerifyVoteExtensionStatus::RejectVote
        };

        Ok(Response::new(VerifyVoteExtensionResponse {
            status: status.into(),
        }))
    }

    /// FinalizeBlock delivers a decided block to the application
    async fn finalize_block(
        &self,
        request: Request<FinalizeBlockRequest>,
    ) -> std::result::Result<Response<FinalizeBlockResponse>, Status> {
        let req = request.into_inner();
        info!(
            "ABCI FinalizeBlock: height={}, {} txs, time={}",
            req.height,
            req.txs.len(),
            req.time.as_ref().map(|t| t.seconds).unwrap_or(0)
        );

        // Process evidence of misbehavior first
        if !req.misbehavior.is_empty() {
            for evidence in &req.misbehavior {
                info!(
                    "Processing evidence: type={:?}, validator={}, height={}",
                    evidence.r#type,
                    hex::encode(
                        evidence
                            .validator
                            .as_ref()
                            .map(|v| &v.address)
                            .unwrap_or(&vec![])
                    ),
                    evidence.height
                );

                // Handle slashing for misbehavior
                if let Some(validator) = &evidence.validator {
                    // Default slash fraction based on misbehavior type
                    let slash_fraction = match evidence.r#type {
                        1 => 0.01, // DUPLICATE_VOTE: 1% slash
                        2 => 0.05, // LIGHT_CLIENT_ATTACK: 5% slash
                        _ => 0.0,
                    };

                    if slash_fraction > 0.0 {
                        if let Err(e) = self
                            .validator_manager
                            .slash_validator(&validator.address, slash_fraction, req.height as u64)
                            .await
                        {
                            error!("Failed to slash validator: {}", e);
                        }
                    }
                }
            }
        }

        let mut app = self.app.write().await;

        // Convert timestamp
        let block_time = req.time.as_ref().map(|t| t.seconds as u64).unwrap_or(0);

        // Process the block
        let tx_results = app
            .finalize_block(req.height as u64, block_time, req.txs)
            .map_err(|e| Status::internal(format!("FinalizeBlock failed: {e}")))?;

        // Get app hash before dropping the lock
        let app_hash = app.get_last_app_hash().to_vec();
        drop(app);

        // Convert transaction results
        let tx_results = tx_results
            .into_iter()
            .map(|result| ExecTxResult {
                code: result.code,
                data: vec![],
                log: result.log,
                info: String::new(),
                gas_wanted: result.gas_wanted as i64,
                gas_used: result.gas_used as i64,
                events: convert_events(result.events),
                codespace: String::new(),
            })
            .collect();

        // Get pending validator updates
        let validator_updates = self.validator_manager.take_pending_updates().await;

        // Get pending consensus parameter updates
        let consensus_param_updates = self.consensus_manager.take_pending_updates().await;

        // Create block events
        let mut events = vec![];

        // Add validator update events
        if !validator_updates.is_empty() {
            let update_event = abci::Event {
                r#type: "validator_updates".to_string(),
                attributes: validator_updates
                    .iter()
                    .map(|v| abci::EventAttribute {
                        key: "address".to_string(),
                        value: hex::encode(&v.address),
                        index: true,
                    })
                    .collect(),
            };
            events.push(update_event);
        }

        // Add consensus param update event
        if consensus_param_updates.is_some() {
            events.push(abci::Event {
                r#type: "consensus_param_updates".to_string(),
                attributes: vec![abci::EventAttribute {
                    key: "updated".to_string(),
                    value: "true".to_string(),
                    index: true,
                }],
            });
        }

        Ok(Response::new(FinalizeBlockResponse {
            events,
            tx_results,
            validator_updates,
            consensus_param_updates,
            app_hash,
        }))
    }
}

/// Convert internal events to ABCI events
fn convert_events(events: Vec<Event>) -> Vec<abci::Event> {
    events
        .into_iter()
        .map(|event| {
            abci::Event {
                r#type: event.event_type,
                attributes: event
                    .attributes
                    .into_iter()
                    .map(|attr| {
                        abci::EventAttribute {
                            key: attr.key,
                            value: attr.value,
                            index: true, // Index all attributes for now
                        }
                    })
                    .collect(),
            }
        })
        .collect()
}

/// ABCI++ Server Builder
pub struct AbciServerBuilder {
    app: Option<BaseApp>,
    chain_id: Option<String>,
    address: String,
}

impl AbciServerBuilder {
    /// Create a new ABCI server builder
    pub fn new() -> Self {
        Self {
            app: None,
            chain_id: None,
            address: "127.0.0.1:26658".to_string(), // Default ABCI port
        }
    }

    /// Set the base application
    pub fn with_app(mut self, app: BaseApp) -> Self {
        self.app = Some(app);
        self
    }

    /// Set the chain ID
    pub fn with_chain_id(mut self, chain_id: String) -> Self {
        self.chain_id = Some(chain_id);
        self
    }

    /// Set the server address
    pub fn with_address(mut self, address: String) -> Self {
        self.address = address;
        self
    }

    /// Build and start the server
    pub async fn build_and_start(self) -> Result<()> {
        let app = self
            .app
            .ok_or_else(|| AbciError::InvalidRequest("BaseApp not provided".to_string()))?;
        let chain_id = self
            .chain_id
            .ok_or_else(|| AbciError::InvalidRequest("Chain ID not provided".to_string()))?;

        let server = AbciServer::new(app, chain_id);
        server.start(&self.address).await
    }
}

impl Default for AbciServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AbciConfig;

    #[tokio::test]
    async fn test_abci_server_creation() {
        let app = BaseApp::new("test-app".to_string()).expect("Failed to create BaseApp");
        let server = AbciServer::new(app, "test-chain".to_string());
        assert_eq!(server.chain_id, "test-chain");
        assert_eq!(server.initial_height, 1);
    }

    #[tokio::test]
    async fn test_abci_server_with_config() {
        let app = BaseApp::new("test-app".to_string()).expect("Failed to create BaseApp");
        let config = AbciConfig {
            listen_address: "tcp://127.0.0.1:26658".to_string(),
            grpc_address: Some("127.0.0.1:9090".to_string()),
            max_connections: 5,
            flush_interval: 50,
            persist_interval: 10,
            retain_blocks: 100,
            chain_id: "test-chain".to_string(),
            snapshot_dir: None, // Disable snapshots for tests
            snapshot_interval: 0,
            max_snapshots: 0,
            max_validators: 100,
        };
        let server = AbciServer::with_config(app, "test-chain".to_string(), config.clone());
        assert_eq!(server.chain_id, "test-chain");
        assert_eq!(server.config.retain_blocks, 100);
    }

    #[tokio::test]
    async fn test_echo() {
        let app = BaseApp::new("test-app".to_string()).expect("Failed to create BaseApp");
        let server = AbciServer::new(app, "test-chain".to_string());

        let request = Request::new(EchoRequest {
            message: "hello".to_string(),
        });

        let response = server.echo(request).await.unwrap();
        assert_eq!(response.into_inner().message, "hello");
    }

    #[tokio::test]
    async fn test_info() {
        let app = BaseApp::new("test-app".to_string()).expect("Failed to create BaseApp");
        let server = AbciServer::new(app, "test-chain".to_string());

        let request = Request::new(InfoRequest {});
        let response = server.info(request).await.unwrap();
        let info = response.into_inner();

        assert_eq!(info.data, "helium");
        assert_eq!(info.app_version, 1);
        assert_eq!(info.last_block_height, 0);
    }

    #[tokio::test]
    async fn test_init_chain() {
        let app = BaseApp::new("test-app".to_string()).expect("Failed to create BaseApp");
        let server = AbciServer::new(app, "test-chain".to_string());

        let request = Request::new(InitChainRequest {
            time: None,
            chain_id: "test-chain".to_string(),
            consensus_params: None,
            validators: vec![],
            app_state_bytes: vec![],
            initial_height: 1,
        });

        let response = server.init_chain(request).await.unwrap();
        let result = response.into_inner();
        assert!(result.validators.is_empty());
    }

    #[tokio::test]
    async fn test_init_chain_wrong_chain_id() {
        let app = BaseApp::new("test-app".to_string()).expect("Failed to create BaseApp");
        let server = AbciServer::new(app, "test-chain".to_string());

        let request = Request::new(InitChainRequest {
            time: None,
            chain_id: "wrong-chain".to_string(),
            consensus_params: None,
            validators: vec![],
            app_state_bytes: vec![],
            initial_height: 1,
        });

        let response = server.init_chain(request).await;
        assert!(response.is_err());
    }

    #[tokio::test]
    async fn test_check_tx() {
        let app = BaseApp::new("test-app".to_string()).expect("Failed to create BaseApp");
        let server = AbciServer::new(app, "test-chain".to_string());

        let request = Request::new(CheckTxRequest {
            tx: vec![1, 2, 3, 4],
            r#type: 0,
        });

        let response = server.check_tx(request).await.unwrap();
        let result = response.into_inner();
        // Without a valid tx_decoder module, invalid transactions will fail
        // The test transaction [1, 2, 3, 4] is not a valid encoded transaction
        assert_eq!(result.code, 1);
        assert!(result.log.contains("no messages") || result.log.contains("decode failed"));
    }

    #[tokio::test]
    async fn test_query() {
        let app = BaseApp::new("test-app".to_string()).expect("Failed to create BaseApp");
        let server = AbciServer::new(app, "test-chain".to_string());

        let request = Request::new(QueryRequest {
            data: vec![],
            path: "/app/version".to_string(),
            height: 0,
            prove: false,
        });

        let response = server.query(request).await.unwrap();
        let result = response.into_inner();
        assert_eq!(result.code, 0);
    }

    #[tokio::test]
    async fn test_query_invalid_path() {
        let app = BaseApp::new("test-app".to_string()).expect("Failed to create BaseApp");
        let server = AbciServer::new(app, "test-chain".to_string());

        let request = Request::new(QueryRequest {
            data: vec![],
            path: "".to_string(),
            height: 0,
            prove: false,
        });

        let response = server.query(request).await;
        assert!(response.is_err());
    }

    #[tokio::test]
    async fn test_error_codes() {
        assert_eq!(AbciError::InvalidTransaction("test".to_string()).code(), 1);
        assert_eq!(AbciError::InsufficientFees("test".to_string()).code(), 2);
        assert_eq!(AbciError::InvalidSequence("test".to_string()).code(), 3);
        assert_eq!(AbciError::UnknownRequest("test".to_string()).code(), 4);
        assert_eq!(AbciError::InternalError("test".to_string()).code(), 99);
        assert_eq!(
            AbciError::BaseApp(helium_baseapp::BaseAppError::InvalidTx("test".to_string())).code(),
            100
        );
    }
}

/// Handle ABCI connection - placeholder for TCP connection handling
async fn handle_abci_connection(
    _server: AbciServer,
    _stream: tokio::net::TcpStream,
    peer_addr: std::net::SocketAddr,
) -> Result<()> {
    info!("New ABCI connection from {}", peer_addr);

    // TODO: Implement actual ABCI wire protocol handling
    // This would involve:
    // 1. Reading length-prefixed messages
    // 2. Decoding ABCI requests
    // 3. Routing to appropriate methods
    // 4. Encoding and sending responses

    // For now, we're using gRPC via tonic, so this is a placeholder

    Ok(())
}
