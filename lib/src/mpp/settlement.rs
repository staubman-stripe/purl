//! MPP settlement response type.

use serde::{Deserialize, Serialize};

/// MPP settlement response (from Payment-Receipt header).
///
/// MPP receipts have a different shape than x402 settlement responses,
/// using `status`/`method`/`timestamp`/`reference` fields per the IETF spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MppSettlementResponse {
    /// Receipt status ("success" or "failed")
    pub status: String,
    /// Payment method used (e.g., "tempo")
    pub method: String,
    /// Timestamp (ISO 8601)
    pub timestamp: String,
    /// Transaction hash or reference
    pub reference: String,
    /// Network identifier (e.g., "eip155:4217" for mainnet, "eip155:42431" for testnet).
    /// Populated from the challenge that initiated this payment, since MPP receipts
    /// don't include network information themselves.
    #[serde(default)]
    pub network: Option<String>,
}
