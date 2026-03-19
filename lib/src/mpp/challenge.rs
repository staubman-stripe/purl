//! MPP Challenge wrapper implementing purl's PaymentChallenge trait.
//!
//! This module provides a wrapper around mpp::PaymentChallenge that implements
//! purl's protocol-agnostic PaymentChallenge trait, allowing native MPP challenges
//! to flow through the purl payment pipeline without lossy conversion.

use crate::protocol::PaymentChallenge;
use std::any::Any;
use std::fmt;

/// Wrapper around mpp::PaymentChallenge that implements purl's PaymentChallenge trait.
///
/// This wrapper preserves the native MPP challenge data, allowing the Tempo provider
/// to downcast and use the original challenge directly without reconstruction.
pub struct MppChallenge {
    /// The native MPP challenge
    pub inner: mpp::PaymentChallenge,
    /// Network identifier (e.g., "eip155:42431" for Tempo Moderato)
    pub network: String,
    /// Payment amount in atomic units
    pub amount: String,
    /// Asset/currency identifier
    pub asset: String,
    /// Recipient address
    pub recipient: String,
    /// Resource URL
    pub resource: String,
    /// Description of the payment
    pub description: String,
}

impl fmt::Debug for MppChallenge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MppChallenge")
            .field("network", &self.network)
            .field("amount", &self.amount)
            .field("asset", &self.asset)
            .field("recipient", &self.recipient)
            .field("resource", &self.resource)
            .field("description", &self.description)
            .field("inner.id", &self.inner.id)
            .finish()
    }
}

impl MppChallenge {
    /// Create a new MppChallenge from a native mpp::PaymentChallenge.
    ///
    /// Extracts relevant fields from the challenge's request payload.
    pub fn from_mpp_challenge(challenge: mpp::PaymentChallenge) -> crate::error::Result<Self> {
        use crate::error::PurlError;

        // Decode the request field to get payment details
        let request: serde_json::Value = challenge
            .request
            .decode_value()
            .map_err(|e| PurlError::Http(format!("Failed to decode MPP request: {}", e)))?;

        // Extract fields from the request
        let amount = request
            .get("amount")
            .and_then(|v| v.as_str())
            .unwrap_or("0")
            .to_string();
        let asset = request
            .get("currency")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let recipient = request
            .get("recipient")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Get chain ID from methodDetails if available
        let chain_id = request
            .get("methodDetails")
            .and_then(|md| md.get("chainId"))
            .and_then(|v| v.as_u64())
            .unwrap_or(42431); // Default to Tempo Moderato

        // Determine network from method and chain ID
        let network = format!("eip155:{}", chain_id);

        let resource = challenge.realm.clone();
        let description = challenge.description.clone().unwrap_or_default();

        Ok(Self {
            inner: challenge,
            network,
            amount,
            asset,
            recipient,
            resource,
            description,
        })
    }
}

impl PaymentChallenge for MppChallenge {
    fn network(&self) -> &str {
        &self.network
    }

    fn amount(&self) -> &str {
        &self.amount
    }

    fn asset(&self) -> &str {
        &self.asset
    }

    fn recipient(&self) -> &str {
        &self.recipient
    }

    fn is_evm(&self) -> bool {
        // MPP challenges on Tempo use EVM-compatible addresses
        self.network.starts_with("eip155:")
    }

    fn is_solana(&self) -> bool {
        false
    }

    fn is_tempo(&self) -> bool {
        crate::network::is_tempo_network(&self.network) || self.inner.method.as_str() == "tempo"
    }

    fn max_timeout_seconds(&self) -> u64 {
        // MPP challenges may have an expires field, but default to 300 seconds
        300
    }

    fn scheme(&self) -> &str {
        "mpp"
    }

    fn resource(&self) -> &str {
        &self.resource
    }

    fn extra(&self) -> Option<&serde_json::Value> {
        // MPP doesn't use the extra field in the same way
        None
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn mime_type(&self) -> &str {
        "application/json"
    }

    fn protocol_name(&self) -> &str {
        "mpp"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_challenge(method: &str, request_json: serde_json::Value) -> MppChallenge {
        use mpp::protocol::core::Base64UrlJson;
        let request = Base64UrlJson::from_value(&request_json).unwrap();
        let inner = mpp::PaymentChallenge::new(
            "test-id".to_string(),
            "https://example.com/api",
            method,
            "charge",
            request,
        );
        MppChallenge::from_mpp_challenge(inner).unwrap()
    }

    #[test]
    fn test_mpp_challenge_is_tempo() {
        let challenge = make_challenge(
            "tempo",
            serde_json::json!({
                "amount": "1000",
                "currency": "USDC",
                "recipient": "0x1234",
            }),
        );

        assert!(challenge.is_tempo());
        assert!(challenge.is_evm());
        assert!(!challenge.is_solana());
        assert_eq!(challenge.scheme(), "mpp");
        assert_eq!(challenge.amount(), "1000");
        assert_eq!(challenge.asset(), "USDC");
    }

    #[test]
    fn test_mpp_challenge_fields() {
        let challenge = make_challenge(
            "tempo",
            serde_json::json!({
                "amount": "5000000",
                "currency": "pathUSD",
                "recipient": "0xabcdef1234567890abcdef1234567890abcdef12",
            }),
        );

        assert_eq!(challenge.amount(), "5000000");
        assert_eq!(challenge.asset(), "pathUSD");
        assert_eq!(
            challenge.recipient(),
            "0xabcdef1234567890abcdef1234567890abcdef12"
        );
        assert_eq!(challenge.resource(), "https://example.com/api");
        assert_eq!(challenge.mime_type(), "application/json");
        assert_eq!(challenge.max_timeout_seconds(), 300);
        assert!(challenge.extra().is_none());
    }

    #[test]
    fn test_mpp_challenge_network_from_chain_id() {
        // With explicit chainId for mainnet
        let challenge = make_challenge(
            "tempo",
            serde_json::json!({
                "amount": "1000",
                "currency": "USDC",
                "recipient": "0x1234",
                "methodDetails": { "chainId": 4217 },
            }),
        );
        assert_eq!(challenge.network(), "eip155:4217");
        assert!(challenge.is_tempo());

        // With explicit chainId for testnet
        let challenge = make_challenge(
            "tempo",
            serde_json::json!({
                "amount": "1000",
                "currency": "USDC",
                "recipient": "0x1234",
                "methodDetails": { "chainId": 42431 },
            }),
        );
        assert_eq!(challenge.network(), "eip155:42431");
        assert!(challenge.is_tempo());

        // Without methodDetails - defaults to 42431 for backward compat
        let challenge = make_challenge(
            "tempo",
            serde_json::json!({
                "amount": "1000",
                "currency": "USDC",
                "recipient": "0x1234",
            }),
        );
        assert_eq!(challenge.network(), "eip155:42431");
    }

    #[test]
    fn test_mpp_challenge_missing_fields_default() {
        // Missing all optional fields
        let challenge = make_challenge("tempo", serde_json::json!({}));

        assert_eq!(challenge.amount(), "0");
        assert_eq!(challenge.asset(), "");
        assert_eq!(challenge.recipient(), "");
        assert_eq!(challenge.description(), "");
    }

    #[test]
    fn test_mpp_challenge_non_tempo_method() {
        // A non-tempo method should still produce an eip155 network
        let challenge = make_challenge(
            "other-method",
            serde_json::json!({
                "amount": "100",
                "currency": "USDC",
                "recipient": "0x1234",
                "methodDetails": { "chainId": 1 },
            }),
        );

        assert_eq!(challenge.network(), "eip155:1");
        // is_tempo checks both network and method
        assert!(!challenge.is_tempo());
        assert!(challenge.is_evm());
    }

    #[test]
    fn test_mpp_challenge_as_any_downcast() {
        let challenge = make_challenge(
            "tempo",
            serde_json::json!({
                "amount": "1000",
                "currency": "USDC",
                "recipient": "0x1234",
            }),
        );

        // Should be able to downcast from trait object
        let trait_obj: &dyn PaymentChallenge = &challenge;
        let downcasted = trait_obj.as_any().downcast_ref::<MppChallenge>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().amount(), "1000");
    }

    #[test]
    fn test_mpp_challenge_debug_format() {
        let challenge = make_challenge(
            "tempo",
            serde_json::json!({
                "amount": "1000",
                "currency": "USDC",
                "recipient": "0x1234",
            }),
        );

        let debug = format!("{:?}", challenge);
        assert!(debug.contains("MppChallenge"));
        assert!(debug.contains("eip155:42431"));
        assert!(debug.contains("USDC"));
    }
}
