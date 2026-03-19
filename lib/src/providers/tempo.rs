//! Tempo payment provider.
//!
//! This module provides integration with the Tempo blockchain using MPP (Machine Payments Protocol).
//! It wraps `mpp::client::TempoProvider` and implements purl's `PaymentProvider` trait.

use crate::config::{Config, WalletConfig};
use crate::currency::Currency;
use crate::error::{PurlError, Result};
use crate::mpp::MppChallenge;
use crate::network::{get_network, ChainType, Network};
use crate::payment_provider::{DryRunInfo, NetworkBalance, PaymentProvider};
use crate::protocol::PaymentChallenge;
use crate::PaymentPayload;
use alloy::primitives::Address;
use alloy::providers::ProviderBuilder;
use alloy::sol;
use async_trait::async_trait;
use std::str::FromStr;

const PROVIDER_NAME: &str = "Tempo";

/// Tempo payment provider using MPP protocol.
///
/// This provider handles payments on the Tempo blockchain, using TIP-20 stablecoins.
/// It reuses EVM keystores since Tempo uses EVM-compatible addresses.
#[derive(Default)]
pub struct TempoProvider;

impl TempoProvider {
    pub fn new() -> Self {
        Self
    }

    /// Load EVM signer from config (reused for Tempo since it uses EVM addresses)
    fn load_signer(config: &Config) -> Result<alloy_signer_local::PrivateKeySigner> {
        use crate::signer::WalletSource;
        let evm_config = config.require_evm()?;
        evm_config.load_signer(config.password.as_deref())
    }
}

#[async_trait]
impl PaymentProvider for TempoProvider {
    fn supports_network(&self, network: &str) -> bool {
        get_network(network)
            .map(|n| n.chain_type == ChainType::Tempo)
            .unwrap_or(false)
    }

    async fn create_payment(
        &self,
        challenge: &dyn PaymentChallenge,
        config: &Config,
    ) -> Result<PaymentPayload> {
        let signer = Self::load_signer(config)?;

        // Resolve RPC URL from network registry based on the challenge's network
        let rpc_url = get_network(challenge.network())
            .map(|n| n.rpc_url.clone())
            .unwrap_or_else(|| "https://rpc.tempo.xyz".to_string());

        // Create mpp-rs TempoProvider
        let mpp_provider =
            mpp::client::TempoProvider::new(signer.clone(), &rpc_url).map_err(|e| {
                PurlError::InvalidConfig(format!("Failed to create Tempo provider: {}", e))
            })?;

        // Require native MPP challenge - no protocol conversion
        let mpp_challenge = challenge
            .as_any()
            .downcast_ref::<MppChallenge>()
            .ok_or_else(|| {
                PurlError::InvalidConfig("Tempo provider requires an MppChallenge".to_string())
            })?;

        // Execute payment using native MPP challenge
        use mpp::client::PaymentProvider as MppPaymentProvider;
        let credential = mpp_provider
            .pay(&mpp_challenge.inner)
            .await
            .map_err(|e| PurlError::Signing(format!("Tempo payment failed: {}", e)))?;

        // Format as Authorization header value
        let auth_header = mpp::format_authorization(&credential)
            .map_err(|e| PurlError::Signing(format!("Failed to format credential: {}", e)))?;

        // Create a purl PaymentPayload that carries the MPP credential
        // We store the full Authorization header value so it can be used directly
        let payload_json = serde_json::json!({
            "mpp_authorization": auth_header,
            "source": credential.source,
        });

        let payment_payload = PaymentPayload::new_v1(
            challenge.scheme().to_string(),
            challenge.network().to_string(),
            payload_json,
        );

        Ok(payment_payload)
    }

    fn name(&self) -> &str {
        PROVIDER_NAME
    }

    fn dry_run(&self, challenge: &dyn PaymentChallenge, config: &Config) -> Result<DryRunInfo> {
        let evm_config = config.require_evm()?;

        // Validate amount is a parseable number
        let _: u128 = challenge.amount().parse().map_err(|_| {
            PurlError::InvalidAmount("The server provided an invalid payment amount.".to_string())
        })?;

        Ok(DryRunInfo {
            provider: PROVIDER_NAME.to_owned(),
            network: challenge.network().to_string(),
            amount: challenge.amount().to_string(),
            asset: challenge.asset().to_string(),
            from: evm_config.get_address()?,
            to: challenge.recipient().to_string(),
            estimated_fee: Some("0".to_string()), // Server pays gas on Tempo
        })
    }

    fn get_address(&self, config: &Config) -> Result<String> {
        // Reuse EVM address since Tempo uses EVM-compatible addresses
        config.require_evm()?.get_address()
    }

    async fn get_balance(
        &self,
        address: &str,
        network: Network,
        currency: Currency,
    ) -> Result<NetworkBalance> {
        // TIP-20 is ERC20-compatible, so we can use the standard balanceOf interface
        sol! {
            #[sol(rpc)]
            interface IERC20 {
                function balanceOf(address account) external view returns (uint256);
            }
        }

        // Get the token configuration for this network (pathUSD for Tempo Moderato)
        let token_config = network.default_token_config().ok_or_else(|| {
            PurlError::UnsupportedToken(format!(
                "{} is not supported on {}",
                currency.symbol, network
            ))
        })?;

        let network_info = network.info();
        let provider =
            ProviderBuilder::new().connect_http(network_info.rpc_url.parse().map_err(|_| {
                PurlError::InvalidConfig(format!(
                    "Invalid network configuration for {}. This is an internal error.",
                    network
                ))
            })?);

        let wallet_address = Address::from_str(address)
            .map_err(|_| PurlError::invalid_address(format!("Invalid address: {}", address)))?;
        let token_address = Address::from_str(token_config.address).map_err(|_| {
            PurlError::invalid_address(format!(
                "Invalid {} token configuration for {}. This is an internal error.",
                token_config.currency.symbol, network
            ))
        })?;

        let contract = IERC20::new(token_address, &provider);

        let balance = contract
            .balanceOf(wallet_address)
            .call()
            .await
            .map_err(|e| {
                PurlError::BalanceQuery(format!(
                    "Could not fetch balance from {}. The network may be unavailable: {}",
                    network, e
                ))
            })?;

        let balance_atomic: u128 = balance.to_string().parse().unwrap_or(0);
        let balance_human = token_config.currency.format_atomic(balance_atomic);

        Ok(NetworkBalance {
            network: network.to_string(),
            balance_atomic: balance.to_string(),
            balance_human,
            asset: token_config.currency.symbol.to_string(),
        })
    }
}
