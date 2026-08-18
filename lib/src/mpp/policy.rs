//! Display-safety policy for inbound MPP challenges.
//!
//! purl only signs a payment it can describe to the user first. A challenge that
//! encodes semantics no purl payment view can render is refused here, at parse time,
//! rather than surfacing as a payment the user approved without seeing.
//!
//! Every path that turns a raw `mpp::PaymentChallenge` into something purl acts on
//! goes through [`decode_and_validate`], so the rules below cannot be bypassed by
//! adding a new caller.

use crate::error::{PurlError, Result};

use super::challenge::decode_request;

/// Decode a challenge's request payload and apply purl's display-safety rules.
pub(crate) fn decode_and_validate(challenge: &mpp::PaymentChallenge) -> Result<serde_json::Value> {
    let request = decode_request(challenge)?;
    reject_undisclosed_transfers(challenge, &request)?;
    require_recipient(&request)?;
    Ok(request)
}

/// Reject challenges that move funds purl would not name in the payment view.
///
/// The MPP Tempo signer treats `methodDetails.splits` as additional token transfers.
/// Until every purl payment view can display and confirm that exact transfer plan,
/// accepting such a challenge would let the signer pay undisclosed recipients.
fn reject_undisclosed_transfers(
    challenge: &mpp::PaymentChallenge,
    request: &serde_json::Value,
) -> Result<()> {
    let contains_splits = request
        .get("methodDetails")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|details| details.contains_key("splits"));

    if challenge.method.as_str() == "tempo" && contains_splits {
        return Err(PurlError::Http(
            "Tempo split-payment challenges are not supported".to_string(),
        ));
    }

    Ok(())
}

/// Read the recipient purl will pay out of a decoded MPP request.
///
/// A challenge with no recipient would render as a blank address in every payment
/// view, so purl refuses it instead of asking the user to confirm a payment whose
/// destination it cannot name.
pub(crate) fn require_recipient(request: &serde_json::Value) -> Result<&str> {
    let recipient = request
        .get("recipient")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    if recipient.is_empty() {
        return Err(PurlError::invalid_address(
            "The server did not provide a recipient address for this MPP challenge.",
        ));
    }

    Ok(recipient)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp::protocol::core::Base64UrlJson;

    fn challenge_with(method: &str, request_json: serde_json::Value) -> mpp::PaymentChallenge {
        let request = Base64UrlJson::from_value(&request_json).unwrap();
        mpp::PaymentChallenge::new(
            "test-id".to_string(),
            "https://example.com/api",
            method,
            "charge",
            request,
        )
    }

    #[test]
    fn test_accepts_a_single_recipient_charge() {
        let challenge = challenge_with(
            "tempo",
            serde_json::json!({
                "amount": "1000000",
                "recipient": "0x1111111111111111111111111111111111111111",
            }),
        );

        let request = decode_and_validate(&challenge).unwrap();
        assert_eq!(
            require_recipient(&request).unwrap(),
            "0x1111111111111111111111111111111111111111"
        );
    }

    #[test]
    fn test_rejects_tempo_splits() {
        let challenge = challenge_with(
            "tempo",
            serde_json::json!({
                "amount": "1000000",
                "recipient": "0x1111111111111111111111111111111111111111",
                "methodDetails": {
                    "splits": [{
                        "amount": "999999",
                        "recipient": "0x2222222222222222222222222222222222222222"
                    }]
                }
            }),
        );

        let error = decode_and_validate(&challenge).unwrap_err();
        assert!(error.to_string().contains("split-payment challenges"));
    }

    #[test]
    fn test_rejects_missing_or_blank_recipient() {
        for request_json in [
            serde_json::json!({ "amount": "1000000" }),
            serde_json::json!({ "recipient": "" }),
            serde_json::json!({ "recipient": serde_json::Value::Null }),
        ] {
            let challenge = challenge_with("tempo", request_json.clone());
            let error = decode_and_validate(&challenge).unwrap_err();
            assert!(
                error.to_string().contains("did not provide a recipient"),
                "unexpected error for {request_json}: {error}"
            );
        }
    }

    #[test]
    fn test_reports_splits_before_recipient_when_both_are_wrong() {
        // The more specific rule should win, so the user sees why the shape of the
        // challenge was refused rather than a generic missing-field message.
        let challenge = challenge_with(
            "tempo",
            serde_json::json!({
                "methodDetails": { "splits": [] }
            }),
        );

        let error = decode_and_validate(&challenge).unwrap_err();
        assert!(error.to_string().contains("split-payment challenges"));
    }
}
