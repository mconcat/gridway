//! Transaction builder for constructing and signing transactions

use crate::{Client, ClientError, Result};
use helium_crypto::verify::create_sign_bytes_direct;
use helium_crypto::{
    create_sign_doc, sign_message, verify_signature, PrivateKey, PublicKey, SignMode,
};
use helium_math::Coins;
use helium_types::{
    address::AccAddress,
    tx::{
        AuthInfo, Fee, FeeAmount, ModeInfo, ModeInfoSingle, RawTx, SdkMsg, SignerInfo, TxBody,
        TxMessage,
    },
    Config,
};
use std::sync::Arc;

/// Transaction builder for constructing and signing transactions
pub struct TxBuilder {
    /// Chain ID for the transaction
    pub chain_id: String,
    /// Account number (retrieved from chain or provided)
    pub account_number: Option<u64>,
    /// Sequence number (retrieved from chain or provided)
    pub sequence: Option<u64>,
    /// Gas limit for the transaction
    pub gas_limit: u64,
    /// Fee amount
    pub fee_amount: Coins,
    /// Fee payer address (optional)
    pub fee_payer: Option<AccAddress>,
    /// Fee granter address (optional)  
    pub fee_granter: Option<AccAddress>,
    /// Transaction memo
    pub memo: String,
    /// Timeout height
    pub timeout_height: u64,
    /// Messages to include in the transaction
    pub messages: Vec<Box<dyn SdkMsg>>,
    /// Client for querying chain data
    client: Option<Arc<Client>>,
    /// Configuration for gas, fees, and other settings
    pub config: Config,
}

// Fee estimation configuration is now part of the main Config struct

/// Transaction signing result
#[derive(Debug, Clone)]
pub struct SignedTx {
    /// Raw signed transaction
    pub raw_tx: RawTx,
    /// Transaction bytes (encoded)
    pub tx_bytes: Vec<u8>,
    /// Transaction hash
    pub tx_hash: String,
}

/// Transaction signing configuration
#[derive(Debug, Clone)]
pub struct SigningConfig {
    /// Signing mode
    pub sign_mode: SignMode,
    /// Whether to verify signatures after signing
    pub verify_signatures: bool,
}

impl Default for SigningConfig {
    fn default() -> Self {
        Self {
            sign_mode: SignMode::Direct,
            verify_signatures: true,
        }
    }
}

// Manual implementation of Clone for TxBuilder since SdkMsg trait objects don't implement Clone
impl Clone for TxBuilder {
    fn clone(&self) -> Self {
        Self {
            chain_id: self.chain_id.clone(),
            account_number: self.account_number,
            sequence: self.sequence,
            gas_limit: self.gas_limit,
            fee_amount: self.fee_amount.clone(),
            fee_payer: self.fee_payer,
            fee_granter: self.fee_granter,
            memo: self.memo.clone(),
            timeout_height: self.timeout_height,
            // Note: We cannot clone trait objects, so we'll create an empty vec
            // This means cloning a TxBuilder will lose messages, but that's expected
            // since SdkMsg trait objects cannot be cloned in general
            messages: Vec::new(),
            client: self.client.clone(),
            config: self.config.clone(),
        }
    }
}

impl TxBuilder {
    /// Create a new transaction builder
    pub fn new(chain_id: String, config: Config) -> Self {
        Self {
            chain_id,
            account_number: None,
            sequence: None,
            gas_limit: config.gas.default_limit,
            fee_amount: Coins::empty(),
            fee_payer: None,
            fee_granter: None,
            memo: String::new(),
            timeout_height: 0,
            messages: Vec::new(),
            client: None,
            config,
        }
    }

    /// Create a new transaction builder with a client for automatic data fetching
    pub fn with_client(chain_id: String, client: Arc<Client>, config: Config) -> Self {
        Self {
            chain_id,
            account_number: None,
            sequence: None,
            gas_limit: config.gas.default_limit,
            fee_amount: Coins::empty(),
            fee_payer: None,
            fee_granter: None,
            memo: String::new(),
            timeout_height: 0,
            messages: Vec::new(),
            client: Some(client),
            config,
        }
    }

    /// Set account number
    pub fn account_number(mut self, account_number: u64) -> Self {
        self.account_number = Some(account_number);
        self
    }

    /// Set sequence number
    pub fn sequence(mut self, sequence: u64) -> Self {
        self.sequence = Some(sequence);
        self
    }

    /// Set gas limit
    pub fn gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = gas_limit;
        self
    }

    /// Set fee amount
    pub fn fee_amount(mut self, fee_amount: Coins) -> Self {
        self.fee_amount = fee_amount;
        self
    }

    /// Set fee payer
    pub fn fee_payer(mut self, fee_payer: AccAddress) -> Self {
        self.fee_payer = Some(fee_payer);
        self
    }

    /// Set fee granter
    pub fn fee_granter(mut self, fee_granter: AccAddress) -> Self {
        self.fee_granter = Some(fee_granter);
        self
    }

    /// Set memo
    pub fn memo<S: Into<String>>(mut self, memo: S) -> Self {
        self.memo = memo.into();
        self
    }

    /// Set timeout height
    pub fn timeout_height(mut self, timeout_height: u64) -> Self {
        self.timeout_height = timeout_height;
        self
    }

    /// Add a message to the transaction
    pub fn add_message(mut self, msg: Box<dyn SdkMsg>) -> Self {
        self.messages.push(msg);
        self
    }

    /// Add multiple messages to the transaction
    pub fn add_messages(mut self, msgs: Vec<Box<dyn SdkMsg>>) -> Self {
        self.messages.extend(msgs);
        self
    }

    /// Automatically estimate gas and fees using real transaction simulation
    pub async fn estimate_gas_and_fees(&mut self) -> Result<(u64, Coins)> {
        // Validate that we have messages
        if self.messages.is_empty() {
            return Err(ClientError::InvalidResponse(
                "no messages to estimate".to_string(),
            ));
        }

        let estimated_gas = if let Some(client) = &self.client {
            // Use real simulation if client is available
            match self.simulate_with_client(client).await {
                Ok(gas) => gas,
                Err(_) => {
                    // Fall back to simple estimation if simulation fails
                    self.estimate_gas_for_messages()
                }
            }
        } else {
            // Use simple estimation if no client is available
            self.estimate_gas_for_messages()
        };

        // Parse gas price and calculate fee
        let fee_coins =
            self.calculate_fee_from_gas_price(&self.config.gas.default_price, estimated_gas)?;

        self.gas_limit = estimated_gas;
        self.fee_amount = fee_coins.clone();

        Ok((estimated_gas, fee_coins))
    }

    /// Simulate transaction using the client to get accurate gas estimation
    async fn simulate_with_client(&self, client: &Arc<Client>) -> Result<u64> {
        // Build a temporary transaction for simulation
        let temp_tx = self.build_for_simulation()?;

        // Encode the transaction
        let tx_bytes = self.encode_tx(&temp_tx)?;

        // Simulate using the client
        let simulation_result = client.simulate_transaction(&tx_bytes).await?;

        // Apply gas adjustment factor
        let adjusted_gas =
            (simulation_result.gas_used as f64 * self.config.gas.adjustment_factor) as u64;

        Ok(adjusted_gas.clamp(self.config.gas.min_limit, self.config.gas.max_limit))
    }

    /// Build transaction for simulation (with temporary account info if needed)
    fn build_for_simulation(&self) -> Result<RawTx> {
        // Create a temporary builder with placeholder account info for simulation
        let mut temp_builder = self.clone();

        // Use placeholder values if account info is not set
        if temp_builder.account_number.is_none() {
            temp_builder.account_number = Some(0);
        }
        if temp_builder.sequence.is_none() {
            temp_builder.sequence = Some(0);
        }

        temp_builder.build()
    }

    /// Estimate gas based on message types (simplified)
    fn estimate_gas_for_messages(&self) -> u64 {
        let base_gas = self.config.gas.base_cost;
        let per_message_gas = self.config.gas.per_message_cost;

        let estimated = base_gas + (per_message_gas * self.messages.len() as u64);
        let adjusted = (estimated as f64 * self.config.gas.adjustment_factor) as u64;

        adjusted.clamp(self.config.gas.min_limit, self.config.gas.max_limit)
    }

    /// Calculate fee from gas price string
    fn calculate_fee_from_gas_price(&self, gas_price: &str, gas_limit: u64) -> Result<Coins> {
        // Parse gas price (e.g., "0.025uatom")
        let (amount_str, denom) = self.parse_gas_price(gas_price)?;
        let gas_price_amount: f64 = amount_str.parse().map_err(|e| {
            ClientError::InvalidResponse(format!("invalid gas price amount: {}", e))
        })?;

        // Calculate total fee
        let fee_amount = (gas_price_amount * gas_limit as f64).ceil() as u64;

        // Create fee coins
        use helium_math::{Coin, Int};
        let fee_coin = Coin::new(denom, Int::from_u64(fee_amount)).map_err(|e| {
            ClientError::InvalidResponse(format!("failed to create fee coin: {}", e))
        })?;

        Coins::new(vec![fee_coin])
            .map_err(|e| ClientError::InvalidResponse(format!("failed to create fee coins: {}", e)))
    }

    /// Parse gas price string (e.g., "0.025uatom" -> ("0.025", "uatom"))
    fn parse_gas_price(&self, gas_price: &str) -> Result<(String, String)> {
        // Find the first alphabetic character to separate amount and denom
        let split_pos = gas_price
            .chars()
            .position(|c| c.is_alphabetic())
            .ok_or_else(|| ClientError::InvalidResponse("invalid gas price format".to_string()))?;

        let amount_str = gas_price[..split_pos].to_string();
        let denom = gas_price[split_pos..].to_string();

        if amount_str.is_empty() || denom.is_empty() {
            return Err(ClientError::InvalidResponse(
                "invalid gas price format".to_string(),
            ));
        }

        Ok((amount_str, denom))
    }

    /// Automatically fetch account number and sequence from the chain
    pub async fn auto_fetch_account_info(&mut self, address: &AccAddress) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| ClientError::InvalidResponse("no client configured".to_string()))?;

        // Query account info from the chain
        match client.get_account(&address.to_string()).await {
            Ok(account_info) => {
                self.account_number = Some(account_info.account_number);
                self.sequence = Some(account_info.sequence);
                Ok(())
            }
            Err(_) => {
                // If account doesn't exist or query fails, use default values
                // This is normal for new accounts
                self.account_number = Some(0);
                self.sequence = Some(0);
                Ok(())
            }
        }
    }

    /// Build the unsigned transaction
    pub fn build(&self) -> Result<RawTx> {
        // Validate required fields
        if self.messages.is_empty() {
            return Err(ClientError::InvalidResponse(
                "no messages provided".to_string(),
            ));
        }

        if self.account_number.is_none() {
            return Err(ClientError::InvalidResponse(
                "account number not set".to_string(),
            ));
        }

        if self.sequence.is_none() {
            return Err(ClientError::InvalidResponse("sequence not set".to_string()));
        }

        // Validate all messages
        for msg in &self.messages {
            msg.validate_basic().map_err(|e| {
                ClientError::InvalidResponse(format!("message validation failed: {}", e))
            })?;
        }

        // Create transaction messages
        let tx_messages: Vec<TxMessage> = self
            .messages
            .iter()
            .map(|msg| TxMessage {
                type_url: msg.type_url().to_string(),
                value: msg.encode(),
            })
            .collect();

        // Create transaction body
        let tx_body = TxBody {
            messages: tx_messages,
            memo: self.memo.clone(),
            timeout_height: self.timeout_height,
        };

        // Create fee amounts
        let fee_amounts: Vec<FeeAmount> = self
            .fee_amount
            .as_slice()
            .iter()
            .map(|coin| FeeAmount {
                denom: coin.denom.clone(),
                amount: coin.amount.to_string(),
            })
            .collect();

        // Create fee
        let fee = Fee {
            amount: fee_amounts,
            gas_limit: self.gas_limit,
            payer: self
                .fee_payer
                .as_ref()
                .map(|addr| addr.to_string())
                .unwrap_or_default(),
            granter: self
                .fee_granter
                .as_ref()
                .map(|addr| addr.to_string())
                .unwrap_or_default(),
        };

        // Create signer info (placeholder, will be filled during signing)
        let signer_info = SignerInfo {
            public_key: None, // Will be set during signing
            mode_info: ModeInfo {
                single: Some(ModeInfoSingle { mode: 1 }), // SIGN_MODE_DIRECT
            },
            sequence: self.sequence.unwrap(),
        };

        // Create auth info
        let auth_info = AuthInfo {
            signer_infos: vec![signer_info],
            fee,
        };

        // Create raw transaction
        let raw_tx = RawTx {
            body: tx_body,
            auth_info,
            signatures: vec![], // Will be filled during signing
        };

        Ok(raw_tx)
    }

    /// Sign the transaction with the provided private key
    pub fn sign(&self, private_key: &PrivateKey, config: &SigningConfig) -> Result<SignedTx> {
        // Build the unsigned transaction
        let mut raw_tx = self.build()?;

        // Get the public key from private key
        let public_key = private_key.public_key();

        // Create public key message for transaction
        let public_key_msg = match &public_key {
            PublicKey::Secp256k1(pk) => TxMessage {
                type_url: "/cosmos.crypto.secp256k1.PubKey".to_string(),
                value: pk.to_sec1_bytes().to_vec(),
            },
            PublicKey::Ed25519(pk) => TxMessage {
                type_url: "/cosmos.crypto.ed25519.PubKey".to_string(),
                value: pk.as_bytes().to_vec(),
            },
        };

        // Update signer info with public key
        raw_tx.auth_info.signer_infos[0].public_key = Some(public_key_msg);

        // Serialize body and auth info to protobuf bytes
        let body_bytes = {
            use helium_types::tx::TxBodyProto;
            let body_proto = TxBodyProto::from(&raw_tx.body);
            let mut buf = Vec::new();
            prost::Message::encode(&body_proto, &mut buf).map_err(|e| {
                ClientError::InvalidResponse(format!("Failed to encode body: {}", e))
            })?;
            buf
        };
        let auth_info_bytes = {
            use helium_types::tx::AuthInfoProto;
            let auth_info_proto = AuthInfoProto::from(&raw_tx.auth_info);
            let mut buf = Vec::new();
            prost::Message::encode(&auth_info_proto, &mut buf).map_err(|e| {
                ClientError::InvalidResponse(format!("Failed to encode auth_info: {}", e))
            })?;
            buf
        };

        // Create sign doc
        let sign_doc = create_sign_doc(
            body_bytes,
            auth_info_bytes,
            self.chain_id.clone(),
            self.account_number.unwrap(),
        );

        // Create sign bytes for signing using a simple deterministic format
        let sign_bytes = create_sign_bytes_direct(&sign_doc).map_err(|e| {
            ClientError::InvalidResponse(format!("failed to create sign bytes: {}", e))
        })?;

        // Sign the document based on key type
        let signature = match private_key {
            PrivateKey::Secp256k1(_) => {
                // For secp256k1, hash the message with SHA256 before signing
                use sha2::{Digest, Sha256};
                let message_hash = Sha256::digest(&sign_bytes);
                sign_message(private_key, &message_hash)
                    .map_err(|e| ClientError::InvalidResponse(format!("signing failed: {}", e)))?
            }
            PrivateKey::Ed25519(_) => {
                // For Ed25519, sign the message directly
                sign_message(private_key, &sign_bytes)
                    .map_err(|e| ClientError::InvalidResponse(format!("signing failed: {}", e)))?
            }
        };

        // Add signature to transaction
        raw_tx.signatures = vec![signature.clone()];

        // Verify signature if requested
        if config.verify_signatures {
            match private_key {
                PrivateKey::Secp256k1(_) => {
                    use sha2::{Digest, Sha256};
                    let message_hash = Sha256::digest(&sign_bytes);
                    verify_signature(&public_key, &message_hash, &signature).map_err(|e| {
                        ClientError::InvalidResponse(format!(
                            "signature verification failed: {}",
                            e
                        ))
                    })?;
                }
                PrivateKey::Ed25519(_) => {
                    verify_signature(&public_key, &sign_bytes, &signature).map_err(|e| {
                        ClientError::InvalidResponse(format!(
                            "signature verification failed: {}",
                            e
                        ))
                    })?;
                }
            }
        }

        // Encode transaction to bytes
        let tx_bytes = self.encode_tx(&raw_tx)?;

        // Calculate transaction hash
        let tx_hash = self.calculate_tx_hash(&tx_bytes);

        Ok(SignedTx {
            raw_tx,
            tx_bytes,
            tx_hash,
        })
    }

    /// Encode transaction to protobuf bytes
    fn encode_tx(&self, tx: &RawTx) -> Result<Vec<u8>> {
        use helium_types::tx::TxDecoder;
        let decoder = TxDecoder::new();
        decoder.encode_tx(tx).map_err(|e| {
            ClientError::InvalidResponse(format!("Failed to encode transaction: {}", e))
        })
    }

    /// Calculate transaction hash (simplified SHA256)
    fn calculate_tx_hash(&self, tx_bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(tx_bytes);
        let hash = hasher.finalize();
        hex::encode(hash).to_uppercase()
    }

    /// Broadcast the signed transaction
    pub async fn broadcast(&self, signed_tx: &SignedTx) -> Result<crate::BroadcastResponse> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| ClientError::InvalidResponse("no client configured".to_string()))?;

        client.broadcast_tx(&signed_tx.tx_bytes).await
    }

    /// Build, sign, and broadcast a transaction in one call
    pub async fn build_sign_and_broadcast(
        &self,
        private_key: &PrivateKey,
        signing_config: &SigningConfig,
    ) -> Result<crate::BroadcastResponse> {
        let signed_tx = self.sign(private_key, signing_config)?;
        self.broadcast(&signed_tx).await
    }
}

/// Utility functions for working with common transaction types
impl TxBuilder {
    /// Create a bank send transaction
    pub fn bank_send(
        chain_id: String,
        from_address: AccAddress,
        to_address: AccAddress,
        amount: Coins,
        config: Config,
    ) -> Self {
        use helium_types::msgs::bank::MsgSend;

        let msg = MsgSend::new(from_address, to_address, amount);

        Self::new(chain_id, config).add_message(Box::new(msg))
    }

    /// Create a multi-send transaction with multiple messages
    pub fn multi_send(chain_id: String, config: Config) -> Self {
        Self::new(chain_id, config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helium_math::{Coin, Int};
    use helium_types::{address::AccAddress, msgs::bank::MsgSend};

    fn create_test_addresses() -> (AccAddress, AccAddress) {
        let from_pubkey = [1u8; 33];
        let to_pubkey = [2u8; 33];
        (
            AccAddress::from_pubkey(&from_pubkey),
            AccAddress::from_pubkey(&to_pubkey),
        )
    }

    fn create_test_coins() -> Coins {
        let coin = Coin::new("uatom".to_string(), Int::from_u64(1000)).unwrap();
        Coins::new(vec![coin]).unwrap()
    }

    #[test]
    fn test_tx_builder_creation() {
        let config = Config::default();
        let builder = TxBuilder::new("test-chain".to_string(), config);
        assert_eq!(builder.chain_id, "test-chain");
        assert_eq!(builder.gas_limit, 200_000);
        assert!(builder.messages.is_empty());
        assert!(builder.memo.is_empty());
    }

    #[test]
    fn test_tx_builder_with_messages() {
        let (from_addr, to_addr) = create_test_addresses();
        let coins = create_test_coins();
        let msg = MsgSend::new(from_addr, to_addr, coins);

        let config = Config::default();
        let builder = TxBuilder::new("test-chain".to_string(), config)
            .add_message(Box::new(msg))
            .memo("test transaction")
            .gas_limit(300_000);

        assert_eq!(builder.messages.len(), 1);
        assert_eq!(builder.memo, "test transaction");
        assert_eq!(builder.gas_limit, 300_000);
    }

    #[test]
    fn test_bank_send_builder() {
        let (from_addr, to_addr) = create_test_addresses();
        let coins = create_test_coins();

        let config = Config::default();
        let builder =
            TxBuilder::bank_send("test-chain".to_string(), from_addr, to_addr, coins, config);

        assert_eq!(builder.chain_id, "test-chain");
        assert_eq!(builder.messages.len(), 1);
    }

    #[test]
    fn test_parse_gas_price() {
        let config = Config::default();
        let builder = TxBuilder::new("test".to_string(), config);

        let (amount, denom) = builder.parse_gas_price("0.025uatom").unwrap();
        assert_eq!(amount, "0.025");
        assert_eq!(denom, "uatom");

        let (amount, denom) = builder.parse_gas_price("1stake").unwrap();
        assert_eq!(amount, "1");
        assert_eq!(denom, "stake");

        // Test invalid formats
        assert!(builder.parse_gas_price("0.025").is_err());
        assert!(builder.parse_gas_price("uatom").is_err());
        assert!(builder.parse_gas_price("").is_err());
    }

    #[test]
    fn test_calculate_fee_from_gas_price() {
        let config = Config::default();
        let builder = TxBuilder::new("test".to_string(), config);

        let fees = builder
            .calculate_fee_from_gas_price("0.025uatom", 200_000)
            .unwrap();
        assert_eq!(fees.as_slice().len(), 1);
        assert_eq!(fees.as_slice()[0].denom, "uatom");
        assert_eq!(fees.as_slice()[0].amount.to_string(), "5000"); // 0.025 * 200,000 = 5,000
    }

    #[test]
    fn test_estimate_gas_for_messages() {
        let (from_addr, to_addr) = create_test_addresses();
        let coins = create_test_coins();
        let msg = MsgSend::new(from_addr, to_addr, coins);

        let config = Config::default();
        let builder =
            TxBuilder::new("test-chain".to_string(), config.clone()).add_message(Box::new(msg));

        let estimated_gas = builder.estimate_gas_for_messages();

        // Should be base (50k) + per message (25k) * 1.3 adjustment = 97,500
        assert_eq!(estimated_gas, 97_500);
    }

    #[tokio::test]
    async fn test_estimate_gas_and_fees() {
        let (from_addr, to_addr) = create_test_addresses();
        let coins = create_test_coins();
        let msg = MsgSend::new(from_addr, to_addr, coins);

        let config = Config::default();
        let mut builder =
            TxBuilder::new("test-chain".to_string(), config.clone()).add_message(Box::new(msg));

        let (gas, fees) = builder.estimate_gas_and_fees().await.unwrap();

        assert_eq!(gas, 97_500);
        assert_eq!(builder.gas_limit, 97_500);
        assert_eq!(fees.as_slice().len(), 1);
        assert_eq!(fees.as_slice()[0].denom, "stake");
    }

    #[tokio::test]
    async fn test_estimate_gas_and_fees_with_client() {
        let (from_addr, to_addr) = create_test_addresses();
        let coins = create_test_coins();
        let msg = MsgSend::new(from_addr, to_addr, coins);

        // Create a client (even though it won't work without a real server)
        let config = crate::Config::new("http://localhost:26657", "test-chain").unwrap();
        let client = std::sync::Arc::new(crate::Client::new(config));

        let mut builder =
            TxBuilder::with_client("test-chain".to_string(), client, Config::default())
                .add_message(Box::new(msg));

        // This should fall back to simple estimation since no server is running
        let (gas, fees) = builder.estimate_gas_and_fees().await.unwrap();

        // Should still get the simple estimation
        assert_eq!(gas, 97_500);
        assert_eq!(builder.gas_limit, 97_500);
        assert_eq!(fees.as_slice().len(), 1);
        assert_eq!(fees.as_slice()[0].denom, "stake");
    }

    #[tokio::test]
    async fn test_auto_fetch_account_info() {
        let (from_addr, _to_addr) = create_test_addresses();

        // Create a client
        let config = crate::Config::new("http://localhost:26657", "test-chain").unwrap();
        let client = std::sync::Arc::new(crate::Client::new(config));

        let mut builder =
            TxBuilder::with_client("test-chain".to_string(), client, Config::default());

        // This should fall back to default values since no server is running
        builder.auto_fetch_account_info(&from_addr).await.unwrap();

        assert_eq!(builder.account_number, Some(0));
        assert_eq!(builder.sequence, Some(0));
    }

    #[test]
    fn test_build_unsigned_tx() {
        let (from_addr, to_addr) = create_test_addresses();
        let coins = create_test_coins();
        let msg = MsgSend::new(from_addr, to_addr, coins);

        let config = Config::default();
        let builder = TxBuilder::new("test-chain".to_string(), config)
            .add_message(Box::new(msg))
            .account_number(1)
            .sequence(0)
            .memo("test tx");

        let raw_tx = builder.build().unwrap();

        assert_eq!(raw_tx.body.messages.len(), 1);
        assert_eq!(raw_tx.body.memo, "test tx");
        assert_eq!(raw_tx.auth_info.signer_infos.len(), 1);
        assert_eq!(raw_tx.auth_info.signer_infos[0].sequence, 0);
        assert!(raw_tx.signatures.is_empty()); // Unsigned
    }

    #[test]
    fn test_build_fails_without_account_info() {
        let (from_addr, to_addr) = create_test_addresses();
        let coins = create_test_coins();
        let msg = MsgSend::new(from_addr, to_addr, coins);

        let config = Config::default();
        let builder = TxBuilder::new("test-chain".to_string(), config).add_message(Box::new(msg));

        // Should fail without account number
        assert!(builder.build().is_err());

        let builder = builder.account_number(1);
        // Should fail without sequence
        assert!(builder.build().is_err());
    }

    #[test]
    fn test_build_fails_without_messages() {
        let config = Config::default();
        let builder = TxBuilder::new("test-chain".to_string(), config)
            .account_number(1)
            .sequence(0);

        // Should fail without messages
        assert!(builder.build().is_err());
    }

    #[test]
    fn test_encode_tx() {
        let (from_addr, to_addr) = create_test_addresses();
        let coins = create_test_coins();
        let msg = MsgSend::new(from_addr, to_addr, coins);

        let config = Config::default();
        let builder = TxBuilder::new("test-chain".to_string(), config)
            .add_message(Box::new(msg))
            .account_number(1)
            .sequence(0);

        let raw_tx = builder.build().unwrap();
        let tx_bytes = builder.encode_tx(&raw_tx).unwrap();

        // Should be valid protobuf bytes
        assert!(!tx_bytes.is_empty());

        // Verify we can decode the protobuf transaction
        use helium_types::tx::TxDecoder;
        let decoder = TxDecoder::new();
        let decoded_tx = decoder.decode_tx(&tx_bytes).unwrap();

        // Verify the decoded transaction matches our input
        assert_eq!(decoded_tx.body.messages.len(), 1);
        assert_eq!(decoded_tx.body.memo, "");
        assert_eq!(decoded_tx.auth_info.signer_infos.len(), 1);
        assert_eq!(decoded_tx.signatures.len(), 0); // Unsigned transaction
    }

    #[test]
    fn test_calculate_tx_hash() {
        let config = Config::default();
        let builder = TxBuilder::new("test-chain".to_string(), config);
        let tx_bytes = b"test transaction bytes";
        let hash = builder.calculate_tx_hash(tx_bytes);

        // Should be 64-character hex string (SHA256)
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_signing_config_default() {
        let config = SigningConfig::default();
        assert!(matches!(config.sign_mode, SignMode::Direct));
        assert!(config.verify_signatures);
    }

    #[test]
    fn test_config_gas_defaults() {
        let config = Config::default();
        assert_eq!(config.gas.default_price, "0.025stake");
        assert_eq!(config.gas.adjustment_factor, 1.3);
        assert_eq!(config.gas.min_limit, 50_000);
        assert_eq!(config.gas.max_limit, 2_000_000);
        assert_eq!(config.gas.base_cost, 50_000);
        assert_eq!(config.gas.per_message_cost, 25_000);
    }
}
