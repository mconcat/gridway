//! Example service implementations for gRPC interfaces

use super::*;
use crate::grpc::{auth, bank, tx};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tracing;

/// Example implementation of the Bank query service
pub struct BankQueryService {
    balances: Arc<RwLock<HashMap<String, HashMap<String, u64>>>>,
}

impl BankQueryService {
    pub fn new() -> Self {
        Self {
            balances: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a balance for testing
    pub async fn set_balance(&self, address: &str, denom: &str, amount: u64) {
        let mut balances = self.balances.write().await;
        balances
            .entry(address.to_string())
            .or_insert_with(HashMap::new)
            .insert(denom.to_string(), amount);
    }
}

impl Default for BankQueryService {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl bank::Query for BankQueryService {
    async fn balance(
        &self,
        request: Request<bank::QueryBalanceRequest>,
    ) -> Result<Response<bank::QueryBalanceResponse>, Status> {
        let req = request.into_inner();
        let balances = self.balances.read().await;

        let balance = balances
            .get(&req.address)
            .and_then(|addr_balances| addr_balances.get(&req.denom))
            .map(|&amount| Coin {
                denom: req.denom,
                amount: amount.to_string(),
            });

        Ok(Response::new(bank::QueryBalanceResponse { balance }))
    }

    async fn all_balances(
        &self,
        request: Request<bank::QueryAllBalancesRequest>,
    ) -> Result<Response<bank::QueryAllBalancesResponse>, Status> {
        let req = request.into_inner();
        let balances_map = self.balances.read().await;

        let balances = balances_map
            .get(&req.address)
            .map(|addr_balances| {
                addr_balances
                    .iter()
                    .map(|(denom, amount)| Coin {
                        denom: denom.clone(),
                        amount: amount.to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(Response::new(bank::QueryAllBalancesResponse {
            balances,
            pagination: None,
        }))
    }

    async fn total_supply(
        &self,
        _request: Request<bank::QueryTotalSupplyRequest>,
    ) -> Result<Response<bank::QueryTotalSupplyResponse>, Status> {
        let balances_map = self.balances.read().await;
        let mut supply_map: HashMap<String, u64> = HashMap::new();

        for addr_balances in balances_map.values() {
            for (denom, amount) in addr_balances {
                *supply_map.entry(denom.clone()).or_insert(0) += amount;
            }
        }

        let supply: Vec<Coin> = supply_map
            .into_iter()
            .map(|(denom, amount)| Coin {
                denom,
                amount: amount.to_string(),
            })
            .collect();

        Ok(Response::new(bank::QueryTotalSupplyResponse {
            supply,
            pagination: None,
        }))
    }

    async fn supply_of(
        &self,
        request: Request<bank::QuerySupplyOfRequest>,
    ) -> Result<Response<bank::QuerySupplyOfResponse>, Status> {
        let req = request.into_inner();
        let balances_map = self.balances.read().await;
        let mut total: u64 = 0;

        for addr_balances in balances_map.values() {
            if let Some(amount) = addr_balances.get(&req.denom) {
                total += amount;
            }
        }

        let amount = if total > 0 {
            Some(Coin {
                denom: req.denom,
                amount: total.to_string(),
            })
        } else {
            None
        };

        Ok(Response::new(bank::QuerySupplyOfResponse { amount }))
    }
}

/// Example implementation of the Auth query service
pub struct AuthQueryService {
    accounts: Arc<RwLock<HashMap<String, BaseAccount>>>,
    params: AuthParams,
}

impl AuthQueryService {
    pub fn new() -> Self {
        Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            params: AuthParams {
                max_memo_characters: 256,
                tx_sig_limit: 7,
                tx_size_cost_per_byte: 10,
                sig_verify_cost_ed25519: 590,
                sig_verify_cost_secp256k1: 1000,
            },
        }
    }

    /// Add an account for testing
    pub async fn set_account(&self, account: BaseAccount) {
        let mut accounts = self.accounts.write().await;
        accounts.insert(account.address.clone(), account);
    }
}

impl Default for AuthQueryService {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl auth::Query for AuthQueryService {
    async fn account(
        &self,
        request: Request<auth::QueryAccountRequest>,
    ) -> Result<Response<auth::QueryAccountResponse>, Status> {
        let req = request.into_inner();
        let accounts = self.accounts.read().await;
        let account = accounts.get(&req.address).cloned();

        Ok(Response::new(auth::QueryAccountResponse { account }))
    }

    async fn accounts(
        &self,
        _request: Request<auth::QueryAccountsRequest>,
    ) -> Result<Response<auth::QueryAccountsResponse>, Status> {
        let accounts_map = self.accounts.read().await;
        let accounts: Vec<BaseAccount> = accounts_map.values().cloned().collect();

        Ok(Response::new(auth::QueryAccountsResponse {
            accounts,
            pagination: None,
        }))
    }

    async fn params(
        &self,
        _request: Request<auth::QueryParamsRequest>,
    ) -> Result<Response<auth::QueryParamsResponse>, Status> {
        Ok(Response::new(auth::QueryParamsResponse {
            params: self.params.clone(),
        }))
    }
}

/// Example implementation of the Tx service
pub struct TxService {
    txs: Arc<RwLock<HashMap<String, (Tx, TxResponse)>>>,
    baseapp: Arc<RwLock<helium_baseapp::BaseApp>>,
}

impl TxService {
    pub fn new() -> Self {
        Self {
            txs: Arc::new(RwLock::new(HashMap::new())),
            baseapp: Arc::new(RwLock::new(
                helium_baseapp::BaseApp::new("tx-service".to_string())
                    .expect("Failed to create BaseApp"),
            )),
        }
    }

    pub fn with_baseapp(baseapp: Arc<RwLock<helium_baseapp::BaseApp>>) -> Self {
        Self {
            txs: Arc::new(RwLock::new(HashMap::new())),
            baseapp,
        }
    }
}

impl Default for TxService {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl tx::Service for TxService {
    async fn simulate(
        &self,
        request: Request<tx::SimulateRequest>,
    ) -> Result<Response<tx::SimulateResponse>, Status> {
        let req = request.into_inner();

        // Use BaseApp for real simulation
        let baseapp = self.baseapp.read().await;

        match baseapp.simulate_tx(&req.tx_bytes) {
            Ok(tx_response) => {
                let gas_info = GasInfo {
                    gas_wanted: tx_response.gas_wanted,
                    gas_used: tx_response.gas_used,
                };

                let result = Result_ {
                    data: vec![],
                    log: tx_response.log,
                    events: tx_response
                        .events
                        .into_iter()
                        .map(|event| Event {
                            type_: event.event_type,
                            attributes: event
                                .attributes
                                .into_iter()
                                .map(|attr| EventAttribute {
                                    key: attr.key,
                                    value: attr.value,
                                    index: true, // Default to true for indexing
                                })
                                .collect(),
                        })
                        .collect(),
                };

                Ok(Response::new(tx::SimulateResponse {
                    gas_info: Some(gas_info),
                    result: Some(result),
                }))
            }
            Err(e) => {
                tracing::error!("Transaction simulation failed: {}", e);
                Err(Status::internal(format!("Simulation failed: {e}")))
            }
        }
    }

    async fn get_tx(
        &self,
        request: Request<tx::GetTxRequest>,
    ) -> Result<Response<tx::GetTxResponse>, Status> {
        let req = request.into_inner();
        let txs = self.txs.read().await;

        if let Some((tx, tx_response)) = txs.get(&req.hash) {
            Ok(Response::new(tx::GetTxResponse {
                tx: Some(tx.clone()),
                tx_response: Some(tx_response.clone()),
            }))
        } else {
            Err(Status::not_found("transaction not found"))
        }
    }

    async fn broadcast_tx(
        &self,
        request: Request<tx::BroadcastTxRequest>,
    ) -> Result<Response<tx::BroadcastTxResponse>, Status> {
        let req = request.into_inner();

        // Generate a mock transaction hash
        let txhash = format!("{:X}", req.tx_bytes.len()); // Simple mock hash

        // Create a mock transaction response
        let tx_response = TxResponse {
            height: 1000,
            txhash: txhash.clone(),
            code: 0,
            data: "".to_string(),
            raw_log: "transaction successful".to_string(),
            logs: vec![],
            info: "".to_string(),
            gas_wanted: 200000,
            gas_used: 150000,
            tx: None,
            timestamp: "2024-01-01T12:00:00Z".to_string(),
            events: vec![Event {
                type_: "transfer".to_string(),
                attributes: vec![
                    EventAttribute {
                        key: "sender".to_string(),
                        value: "cosmos1...".to_string(),
                        index: true,
                    },
                    EventAttribute {
                        key: "recipient".to_string(),
                        value: "cosmos1...".to_string(),
                        index: true,
                    },
                ],
            }],
        };

        // Store the transaction for later retrieval
        let mut txs = self.txs.write().await;
        txs.insert(
            txhash,
            (
                Tx {
                    body: None,
                    auth_info: None,
                    signatures: vec![],
                },
                tx_response.clone(),
            ),
        );

        Ok(Response::new(tx::BroadcastTxResponse {
            tx_response: Some(tx_response),
        }))
    }

    async fn get_txs_event(
        &self,
        _request: Request<tx::GetTxsEventRequest>,
    ) -> Result<Response<tx::GetTxsEventResponse>, Status> {
        let txs_map = self.txs.read().await;
        let mut txs = vec![];
        let mut tx_responses = vec![];

        for (tx, tx_response) in txs_map.values() {
            txs.push(tx.clone());
            tx_responses.push(tx_response.clone());
        }

        Ok(Response::new(tx::GetTxsEventResponse {
            txs,
            tx_responses,
            pagination: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::auth::Query as AuthQuery;
    use crate::grpc::bank::Query as BankQuery;
    use crate::grpc::tx::Service as TxServiceTrait;

    #[tokio::test]
    async fn test_bank_query_service() {
        let service = BankQueryService::new();

        // Set a balance
        service.set_balance("cosmos1abcd", "stake", 1000).await;

        // Query the balance
        let request = Request::new(bank::QueryBalanceRequest {
            address: "cosmos1abcd".to_string(),
            denom: "stake".to_string(),
        });

        let response = service.balance(request).await.unwrap();
        let balance = response.into_inner().balance.unwrap();

        assert_eq!(balance.denom, "stake");
        assert_eq!(balance.amount, "1000");
    }

    #[tokio::test]
    async fn test_auth_query_service() {
        let service = AuthQueryService::new();

        // Add an account
        let account = BaseAccount {
            address: "cosmos1abcd".to_string(),
            pub_key: None,
            account_number: 1,
            sequence: 0,
        };
        service.set_account(account.clone()).await;

        // Query the account
        let request = Request::new(auth::QueryAccountRequest {
            address: "cosmos1abcd".to_string(),
        });

        let response = service.account(request).await.unwrap();
        let retrieved = response.into_inner().account.unwrap();

        assert_eq!(retrieved.address, account.address);
        assert_eq!(retrieved.account_number, account.account_number);
    }

    #[tokio::test]
    async fn test_tx_service() {
        let service = TxService::new();

        // Broadcast a transaction
        let request = Request::new(tx::BroadcastTxRequest {
            tx_bytes: vec![1, 2, 3, 4],
            mode: crate::grpc::BroadcastMode::Sync,
        });

        let response = service.broadcast_tx(request).await.unwrap();
        let tx_response = response.into_inner().tx_response.unwrap();

        assert_eq!(tx_response.code, 0);
        assert!(!tx_response.txhash.is_empty());

        // Retrieve the transaction
        let get_request = Request::new(tx::GetTxRequest {
            hash: tx_response.txhash.clone(),
        });

        let get_response = service.get_tx(get_request).await.unwrap();
        let retrieved = get_response.into_inner().tx_response.unwrap();

        assert_eq!(retrieved.txhash, tx_response.txhash);
    }
}
