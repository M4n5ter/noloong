use noloong_agent::interaction::{
    INTERACTION_ERROR_UNAUTHORIZED, InteractionAuthorityCapability, InteractionCapabilityPolicy,
    InteractionClientInfo, InteractionUxCapabilities, JsonRpcResponse, JsonRpcResponsePayload,
};
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn interaction_capability_grants_intersect_authority_and_ux() {
    let policy = InteractionCapabilityPolicy {
        allowed_authority: [
            InteractionAuthorityCapability::AgentRun,
            InteractionAuthorityCapability::ApprovalResolve,
        ]
        .into_iter()
        .collect(),
        allowed_ux: InteractionUxCapabilities {
            raw_events: true,
            display_events: true,
            stream_text: true,
            edit_message: false,
            markdown: true,
            max_message_bytes: Some(4_096),
        },
    };
    let client = InteractionClientInfo {
        name: "telegram-bridge".into(),
        requested_authority: [
            InteractionAuthorityCapability::AgentRun,
            InteractionAuthorityCapability::SessionDelete,
        ]
        .into_iter()
        .collect(),
        requested_ux: InteractionUxCapabilities {
            raw_events: false,
            display_events: true,
            stream_text: true,
            edit_message: true,
            markdown: true,
            max_message_bytes: Some(8_192),
        },
        ..InteractionClientInfo::default()
    };

    let grant = policy.grant(&client);

    assert_eq!(
        grant.authority,
        BTreeSet::from([InteractionAuthorityCapability::AgentRun])
    );
    assert!(!grant.ux.raw_events);
    assert!(grant.ux.display_events);
    assert!(grant.ux.stream_text);
    assert!(!grant.ux.edit_message);
    assert!(grant.ux.markdown);
    assert_eq!(grant.ux.max_message_bytes, Some(4_096));
}

#[test]
fn interaction_authority_capability_serializes_as_stable_string() {
    let value = serde_json::to_value(InteractionAuthorityCapability::SubagentSpawn).unwrap();

    assert_eq!(value, json!("subagent.spawn"));
    assert_eq!(
        serde_json::from_value::<InteractionAuthorityCapability>(value).unwrap(),
        InteractionAuthorityCapability::SubagentSpawn
    );
}

#[test]
fn interaction_authorize_reports_stable_unauthorized_error() {
    let grant = InteractionCapabilityPolicy::allow_all().grant(&InteractionClientInfo {
        name: "readonly".into(),
        requested_authority: [InteractionAuthorityCapability::AgentRun]
            .into_iter()
            .collect(),
        ..InteractionClientInfo::default()
    });

    let error = InteractionCapabilityPolicy::authorize(
        &grant,
        "manifest/apply_approved",
        InteractionAuthorityCapability::ManifestApply,
    )
    .expect_err("missing capability should be rejected");

    assert_eq!(error.code, INTERACTION_ERROR_UNAUTHORIZED);
    assert_eq!(error.data.unwrap()["requiredCapability"], "manifest.apply");
}

#[test]
fn interaction_wire_serde_round_trips_jsonrpc_error() {
    let response = JsonRpcResponse::error(
        json!(7),
        noloong_agent::interaction::InteractionError::not_found("session not found"),
    );
    let encoded = serde_json::to_value(&response).unwrap();

    assert_eq!(encoded["jsonrpc"], "2.0");
    assert_eq!(encoded["id"], 7);
    assert_eq!(encoded["error"]["code"], -32072);
    assert_eq!(
        serde_json::from_value::<JsonRpcResponse>(encoded).unwrap(),
        response
    );
    assert!(matches!(
        response.payload,
        JsonRpcResponsePayload::Error { .. }
    ));
}
