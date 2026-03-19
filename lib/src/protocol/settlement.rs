//! Unified settlement response spanning all payment protocols.
//!
//! This type provides a single enum that can represent settlement responses
//! from any supported protocol (x402 v1, x402 v2, MPP), with protocol-agnostic
//! accessor methods.

use crate::mpp::MppSettlementResponse;
use crate::x402::{v1, v2};
use serde::{Deserialize, Serialize};
use std::any::Any;

use super::PaymentReceipt;

/// Settlement Response (unified across all payment protocols)
///
/// Can represent x402 v1, x402 v2, and MPP protocol responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SettlementResponse {
    V1(v1::SettlementResponse),
    V2(v2::SettlementResponse),
    Mpp(MppSettlementResponse),
}

impl SettlementResponse {
    /// Check if the settlement was successful
    pub fn is_success(&self) -> bool {
        match self {
            SettlementResponse::V1(v1) => v1.success.unwrap_or(false),
            SettlementResponse::V2(v2) => v2.success,
            SettlementResponse::Mpp(mpp) => mpp.status == "success",
        }
    }

    /// Get the error reason, if any
    pub fn error_reason(&self) -> Option<&str> {
        match self {
            SettlementResponse::V1(v1) => v1.error_reason.as_deref(),
            SettlementResponse::V2(v2) => v2.error_reason.as_deref(),
            SettlementResponse::Mpp(_) => None,
        }
    }

    /// Get the transaction hash
    pub fn transaction(&self) -> &str {
        match self {
            SettlementResponse::V1(v1) => &v1.transaction,
            SettlementResponse::V2(v2) => &v2.transaction,
            SettlementResponse::Mpp(mpp) => &mpp.reference,
        }
    }

    /// Get the network (in original format - v1 or v2)
    pub fn network(&self) -> &str {
        match self {
            SettlementResponse::V1(v1) => &v1.network,
            SettlementResponse::V2(v2) => &v2.network,
            SettlementResponse::Mpp(mpp) => {
                if let Some(ref network) = mpp.network {
                    network.as_str()
                } else {
                    // Fallback for receipts without network info
                    &mpp.method
                }
            }
        }
    }

    /// Get the payer address, if available
    pub fn payer(&self) -> Option<&str> {
        match self {
            SettlementResponse::V1(v1) => {
                if v1.payer.is_empty() {
                    None
                } else {
                    Some(&v1.payer)
                }
            }
            SettlementResponse::V2(v2) => v2.payer.as_deref(),
            SettlementResponse::Mpp(_) => None,
        }
    }
}

impl PaymentReceipt for SettlementResponse {
    fn is_success(&self) -> bool {
        SettlementResponse::is_success(self)
    }

    fn transaction(&self) -> &str {
        SettlementResponse::transaction(self)
    }

    fn network(&self) -> &str {
        SettlementResponse::network(self)
    }

    fn error_reason(&self) -> Option<&str> {
        SettlementResponse::error_reason(self)
    }

    fn payer(&self) -> Option<&str> {
        SettlementResponse::payer(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_v1_settlement_response() {
        let json = r#"{
            "success": true,
            "transaction": "0xabc123",
            "network": "base-sepolia",
            "payer": "0x1234..."
        }"#;

        let response: SettlementResponse = serde_json::from_str(json).expect("should parse");
        assert!(response.is_success());
        assert_eq!(response.transaction(), "0xabc123");
        assert_eq!(response.network(), "base-sepolia");
        assert_eq!(response.payer(), Some("0x1234..."));
    }

    #[test]
    fn test_parse_v2_settlement_response() {
        let json = r#"{
            "success": true,
            "transaction": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "network": "eip155:84532",
            "payer": "0x857b06519E91e3A54538791bDbb0E22373e36b66"
        }"#;

        let response: SettlementResponse = serde_json::from_str(json).expect("should parse");
        assert!(response.is_success());
        assert_eq!(
            response.transaction(),
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        );
        assert_eq!(response.network(), "eip155:84532");
    }

    #[test]
    fn test_payment_receipt_trait() {
        let settlement = SettlementResponse::V1(v1::SettlementResponse {
            success: Some(true),
            error_reason: None,
            transaction: "0xabc123".to_string(),
            network: "base".to_string(),
            payer: "0x1234".to_string(),
        });
        let receipt: &dyn PaymentReceipt = &settlement;

        assert!(receipt.is_success());
        assert_eq!(receipt.transaction(), "0xabc123");
        assert_eq!(receipt.network(), "base");
        assert!(receipt.error_reason().is_none());
        assert_eq!(receipt.payer(), Some("0x1234"));
    }

    #[test]
    fn test_payment_receipt_trait_v2() {
        let settlement = SettlementResponse::V2(v2::SettlementResponse {
            success: false,
            error_reason: Some("Insufficient funds".to_string()),
            transaction: "0xdef456".to_string(),
            network: "eip155:84532".to_string(),
            payer: Some("0x5678".to_string()),
        });
        let receipt: &dyn PaymentReceipt = &settlement;

        assert!(!receipt.is_success());
        assert_eq!(receipt.transaction(), "0xdef456");
        assert_eq!(receipt.network(), "eip155:84532");
        assert_eq!(receipt.error_reason(), Some("Insufficient funds"));
        assert_eq!(receipt.payer(), Some("0x5678"));
    }

    #[test]
    fn test_payment_receipt_trait_mpp_mainnet() {
        let settlement = SettlementResponse::Mpp(MppSettlementResponse {
            status: "success".to_string(),
            method: "tempo".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            reference: "0xabc789".to_string(),
            network: Some("eip155:4217".to_string()),
        });
        let receipt: &dyn PaymentReceipt = &settlement;

        assert!(receipt.is_success());
        assert_eq!(receipt.transaction(), "0xabc789");
        assert_eq!(receipt.network(), "eip155:4217");
        assert!(receipt.error_reason().is_none());
        assert!(receipt.payer().is_none());
    }

    #[test]
    fn test_payment_receipt_trait_mpp_testnet() {
        let settlement = SettlementResponse::Mpp(MppSettlementResponse {
            status: "success".to_string(),
            method: "tempo".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            reference: "0xabc789".to_string(),
            network: Some("eip155:42431".to_string()),
        });
        let receipt: &dyn PaymentReceipt = &settlement;

        assert!(receipt.is_success());
        assert_eq!(receipt.transaction(), "0xabc789");
        assert_eq!(receipt.network(), "eip155:42431");
    }

    #[test]
    fn test_mpp_settlement_failed() {
        let settlement = SettlementResponse::Mpp(MppSettlementResponse {
            status: "failed".to_string(),
            method: "tempo".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            reference: "0xfailed".to_string(),
            network: Some("eip155:42431".to_string()),
        });

        assert!(!settlement.is_success());
        assert_eq!(settlement.transaction(), "0xfailed");
        // MPP doesn't have error_reason
        assert!(settlement.error_reason().is_none());
    }

    #[test]
    fn test_mpp_settlement_no_network_falls_back_to_method() {
        let settlement = SettlementResponse::Mpp(MppSettlementResponse {
            status: "success".to_string(),
            method: "other-method".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            reference: "0xabc".to_string(),
            network: None,
        });

        // Without network field, falls back to method string
        assert_eq!(settlement.network(), "other-method");
    }

    #[test]
    fn test_mpp_settlement_serialization_roundtrip() {
        let original = MppSettlementResponse {
            status: "success".to_string(),
            method: "tempo".to_string(),
            timestamp: "2025-06-15T10:30:00Z".to_string(),
            reference: "0xabc123def456".to_string(),
            network: Some("eip155:4217".to_string()),
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: MppSettlementResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.status, "success");
        assert_eq!(deserialized.method, "tempo");
        assert_eq!(deserialized.timestamp, "2025-06-15T10:30:00Z");
        assert_eq!(deserialized.reference, "0xabc123def456");
        assert_eq!(deserialized.network, Some("eip155:4217".to_string()));
    }

    #[test]
    fn test_mpp_settlement_deserialize_without_network() {
        // Receipts from older servers may not include the network field
        let json = r#"{
            "status": "success",
            "method": "tempo",
            "timestamp": "2025-01-01T00:00:00Z",
            "reference": "0xabc"
        }"#;
        let deserialized: MppSettlementResponse = serde_json::from_str(json).unwrap();
        assert_eq!(deserialized.network, None);
    }

    #[test]
    fn test_settlement_response_as_any_downcast() {
        let settlement = SettlementResponse::Mpp(MppSettlementResponse {
            status: "success".to_string(),
            method: "tempo".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            reference: "0xabc789".to_string(),
            network: Some("eip155:4217".to_string()),
        });

        let receipt: &dyn PaymentReceipt = &settlement;
        let downcasted = receipt.as_any().downcast_ref::<SettlementResponse>();
        assert!(downcasted.is_some());
    }
}
