//! MPP (Machine Payments Protocol) support for purl.
//!
//! This module provides integration with the MPP protocol, which uses
//! HTTP 402 Payment Required responses with `WWW-Authenticate: Payment` headers.

pub mod challenge;
pub(crate) mod policy;
pub mod protocol;
pub mod settlement;

pub use challenge::MppChallenge;
pub use protocol::MppProtocol;
pub use settlement::MppSettlementResponse;
