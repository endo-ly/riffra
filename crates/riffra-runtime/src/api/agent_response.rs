//! Compact Control responses shared by agent-facing adapters.
//!
//! Mutation results may carry the full canonical session for GUI consumers.
//! Agent adapters replace that heavyweight payload with the mutation receipt
//! while leaving explicit whole-state deliveries untouched.

use riffra_control::{CommandResult, ControlResponse};
use serde_json::{Map, Value, json};

/// Removes heavyweight canonical session data from successful mutation
/// responses while preserving the explicit mutation receipt.
///
/// Results that exist to deliver the canonical state (`session` and
/// `hostBootstrap`) are returned unchanged.
pub fn compact_agent_response(mut response: ControlResponse) -> ControlResponse {
    let Some(result) = response.result.take() else {
        return response;
    };
    if !response.ok
        || matches!(result.result_type.as_str(), "session" | "hostBootstrap")
        || !carries_canonical_session(&result.value)
    {
        response.result = Some(result);
        return response;
    }
    let mut receipt = match result.value {
        Value::Object(mut value) => {
            value.remove("canonical");
            value
        }
        _ => Map::new(),
    };
    let created_entity_ids = receipt
        .remove("createdEntityIds")
        .unwrap_or_else(|| json!({}));
    receipt.insert("createdEntityIds".into(), created_entity_ids);
    receipt.insert(
        "sequence".into(),
        response
            .sequence
            .map_or(Value::Null, |sequence| json!(sequence)),
    );
    response.result = Some(CommandResult {
        result_type: "mutation".into(),
        value: Value::Object(receipt),
    });
    response
}

fn carries_canonical_session(value: &Value) -> bool {
    value
        .get("canonical")
        .and_then(|canonical| canonical.get("session"))
        .is_some_and(|session| session.get("arrangement").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(result_type: &str, value: Value) -> ControlResponse {
        ControlResponse::success(
            "request-1",
            7,
            CommandResult {
                result_type: result_type.into(),
                value,
            },
        )
    }

    #[test]
    fn canonical_state_deliveries_are_not_compacted() {
        // Arrange
        let original = response(
            "hostBootstrap",
            json!({"canonical": {"session": {"arrangement": {"tracks": []}}}}),
        );

        // Act
        let compacted = compact_agent_response(original.clone());

        // Assert
        assert_eq!(compacted, original);
    }

    #[test]
    fn mutation_receipts_drop_the_canonical_session() {
        // Arrange
        let original = response(
            "arrangementMutation",
            json!({
                "canonical": {"session": {"arrangement": {"tracks": [{"id": "track:1"}]}}},
                "createdEntityIds": {"tracks": ["track:1"]},
            }),
        );

        // Act
        let compacted = compact_agent_response(original);

        // Assert
        assert_eq!(compacted.sequence, Some(7));
        let result = compacted.result.unwrap();
        assert_eq!(result.result_type, "mutation");
        assert_eq!(
            result.value,
            json!({"createdEntityIds": {"tracks": ["track:1"]}, "sequence": 7})
        );
    }

    #[test]
    fn unrelated_results_keep_their_payload() {
        // Arrange
        let original = response("tracks", json!({"tracks": [{"id": "track:1"}]}));

        // Act
        let compacted = compact_agent_response(original.clone());

        // Assert
        assert_eq!(compacted, original);
    }
}
