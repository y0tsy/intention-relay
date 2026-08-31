use crate::{ProtocolCapabilityDto, ProtocolVersionDto};
use intention_types::{DtoResult, ErrorDto, SchemaVersionDto};
use serde::{Deserialize, Serialize};

/// Result of deterministic capability negotiation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolNegotiationResultDto {
    pub protocol_version: ProtocolVersionDto,
    pub schema_version: SchemaVersionDto,
    pub negotiated_capabilities: Vec<ProtocolCapabilityDto>,
}

/// Intersects peer capabilities in the stable protocol declaration order.
///
/// # Errors
///
/// Returns a validation error with code `duplicate_protocol_capability` when
/// either peer declares a duplicate capability.
pub fn intersect_capabilities(
    local: &[ProtocolCapabilityDto],
    remote: &[ProtocolCapabilityDto],
) -> DtoResult<Vec<ProtocolCapabilityDto>> {
    for entries in [local, remote] {
        for (i, capability) in entries.iter().enumerate() {
            if entries[..i].contains(capability) {
                return Err(ErrorDto::validation(
                    "duplicate_protocol_capability",
                    "duplicate protocol capability",
                ));
            }
        }
    }
    Ok(crate::POST_M5_CAPABILITIES
        .iter()
        .copied()
        .chain([
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::CorrelatedRequests,
            ProtocolCapabilityDto::DaemonHealth,
            ProtocolCapabilityDto::RunStreamSubscriptions,
        ])
        .filter(|c| local.contains(c) && remote.contains(c))
        .collect())
}

/// Checks that a negotiated family capability is present.
///
/// The daemon tool gateway additionally requires the model tool loop: a
/// gateway negotiated without `model_tool_loop_v1` fails closed with
/// `model_tool_loop_required`, because post-M4 tool-loop facts are delivered
/// through the gateway.
///
/// # Errors
///
/// Returns a validation error naming the missing capability, or
/// `model_tool_loop_required` when the gateway capability is present without
/// the model tool loop.
pub fn require_capability(
    capabilities: &[ProtocolCapabilityDto],
    capability: ProtocolCapabilityDto,
) -> DtoResult<()> {
    if capabilities.contains(&capability) {
        if capability == ProtocolCapabilityDto::DaemonToolGatewayV1
            && !capabilities.contains(&ProtocolCapabilityDto::ModelToolLoopV1)
        {
            return Err(ErrorDto::validation(
                "model_tool_loop_required",
                "daemon tool gateway requires the model tool loop capability",
            ));
        }
        return Ok(());
    }
    let code = match capability {
        ProtocolCapabilityDto::ProviderProfilesV1 => "provider_profiles_capability_required",
        ProtocolCapabilityDto::SessionForkV1 => "session_fork_capability_required",
        ProtocolCapabilityDto::NormalizedReasoningStreamV1 => {
            "normalized_reasoning_stream_required"
        }
        ProtocolCapabilityDto::AgentActivityV1 => "agent_activity_capability_required",
        ProtocolCapabilityDto::UserNotificationsV1 => "user_notifications_capability_required",
        ProtocolCapabilityDto::DaemonToolGatewayV1 => "daemon_tool_gateway_capability_required",
        ProtocolCapabilityDto::ModelToolLoopV1 => "model_tool_loop_required",
        _ => "execution_meaning_capability_required",
    };
    Err(ErrorDto::validation(
        code,
        "required protocol capability was not negotiated",
    ))
}

/// Requires the model tool loop whenever the daemon tool gateway is negotiated.
///
/// # Errors
///
/// Returns a validation error with code `model_tool_loop_required` when
/// `daemon_tool_gateway_v1` is negotiated without `model_tool_loop_v1`.
pub fn require_gateway_tool_loop(negotiated: &ProtocolNegotiationResultDto) -> DtoResult<()> {
    if negotiated
        .negotiated_capabilities
        .contains(&ProtocolCapabilityDto::DaemonToolGatewayV1)
    {
        require_capability(
            &negotiated.negotiated_capabilities,
            ProtocolCapabilityDto::ModelToolLoopV1,
        )
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;
    use crate::ProtocolCapabilityDto;

    #[test]
    fn duplicate_capabilities_are_rejected_with_stable_code() {
        let duplicate = vec![
            ProtocolCapabilityDto::ProviderProfilesV1,
            ProtocolCapabilityDto::SessionForkV1,
            ProtocolCapabilityDto::ProviderProfilesV1,
        ];
        for (local, remote) in [
            (
                duplicate.clone(),
                vec![ProtocolCapabilityDto::ProviderProfilesV1],
            ),
            (
                vec![ProtocolCapabilityDto::ProviderProfilesV1],
                duplicate.clone(),
            ),
            (duplicate.clone(), duplicate),
        ] {
            assert_eq!(
                intersect_capabilities(&local, &remote)
                    .expect_err("duplicate capability is rejected")
                    .code(),
                "duplicate_protocol_capability"
            );
        }
    }

    #[test]
    fn intersection_ordering_is_deterministic_in_declaration_order() {
        let local = vec![
            ProtocolCapabilityDto::ModelToolLoopV1,
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::ProviderProfilesV1,
            ProtocolCapabilityDto::DaemonHealth,
        ];
        let remote = vec![
            ProtocolCapabilityDto::DaemonHealth,
            ProtocolCapabilityDto::ProviderProfilesV1,
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::AgentActivityV1,
            ProtocolCapabilityDto::ModelToolLoopV1,
        ];
        let negotiated = intersect_capabilities(&local, &remote).expect("no duplicates");
        assert_eq!(
            negotiated,
            vec![
                ProtocolCapabilityDto::ProviderProfilesV1,
                ProtocolCapabilityDto::ModelToolLoopV1,
                ProtocolCapabilityDto::SessionSubscriptions,
                ProtocolCapabilityDto::DaemonHealth,
            ]
        );
        // Repeating the intersection with the same inputs is byte-identical.
        assert_eq!(
            intersect_capabilities(&local, &remote).expect("no duplicates"),
            negotiated
        );
    }

    #[test]
    fn duplicate_capability_across_both_peers_is_negotiated_once() {
        // A capability declared by both peers is not a per-peer duplicate and
        // survives the intersection exactly once.
        let local = vec![ProtocolCapabilityDto::ProviderProfilesV1];
        let remote = vec![ProtocolCapabilityDto::ProviderProfilesV1];
        let negotiated = intersect_capabilities(&local, &remote).expect("no duplicates");
        assert_eq!(negotiated, vec![ProtocolCapabilityDto::ProviderProfilesV1]);
        // Reversing the peer order yields the same deterministic result.
        assert_eq!(
            intersect_capabilities(&remote, &local).expect("no duplicates"),
            negotiated
        );
    }

    #[test]
    fn empty_capability_lists_negotiate_an_empty_intersection() {
        let lone = vec![ProtocolCapabilityDto::ProviderProfilesV1];
        assert!(
            intersect_capabilities(&[], &lone)
                .expect("no duplicates")
                .is_empty()
        );
        assert!(
            intersect_capabilities(&lone, &[])
                .expect("no duplicates")
                .is_empty()
        );
        assert!(
            intersect_capabilities(&[], &[])
                .expect("no duplicates")
                .is_empty()
        );
    }

    #[test]
    fn all_fail_closed_family_error_codes_map_to_capabilities() {
        for (capability, expected_code) in [
            (
                ProtocolCapabilityDto::ProviderProfilesV1,
                "provider_profiles_capability_required",
            ),
            (
                ProtocolCapabilityDto::SessionForkV1,
                "session_fork_capability_required",
            ),
            (
                ProtocolCapabilityDto::NormalizedReasoningStreamV1,
                "normalized_reasoning_stream_required",
            ),
            (
                ProtocolCapabilityDto::AgentActivityV1,
                "agent_activity_capability_required",
            ),
            (
                ProtocolCapabilityDto::UserNotificationsV1,
                "user_notifications_capability_required",
            ),
            (
                ProtocolCapabilityDto::DaemonToolGatewayV1,
                "daemon_tool_gateway_capability_required",
            ),
            (
                ProtocolCapabilityDto::ModelToolLoopV1,
                "model_tool_loop_required",
            ),
            (
                ProtocolCapabilityDto::SessionSubscriptions,
                "execution_meaning_capability_required",
            ),
        ] {
            assert_eq!(
                require_capability(&[], capability)
                    .expect_err("missing capability fails closed")
                    .code(),
                expected_code
            );
        }
    }

    #[test]
    fn gateway_negotiation_requires_the_model_tool_loop() {
        let negotiated = |capabilities: Vec<ProtocolCapabilityDto>| ProtocolNegotiationResultDto {
            protocol_version: ProtocolVersionDto::new(1, 1),
            schema_version: SchemaVersionDto::new(1, 1),
            negotiated_capabilities: capabilities,
        };

        // A gateway negotiated without the loop fails closed before effect.
        assert_eq!(
            require_gateway_tool_loop(&negotiated(vec![
                ProtocolCapabilityDto::DaemonToolGatewayV1,
            ]))
            .expect_err("gateway without tool loop is rejected")
            .code(),
            "model_tool_loop_required"
        );

        // The capability gate itself enforces the same dependency.
        assert_eq!(
            require_capability(
                &[ProtocolCapabilityDto::DaemonToolGatewayV1],
                ProtocolCapabilityDto::DaemonToolGatewayV1,
            )
            .expect_err("gateway gate requires the tool loop")
            .code(),
            "model_tool_loop_required"
        );

        // Both capabilities together pass.
        assert!(
            require_gateway_tool_loop(&negotiated(vec![
                ProtocolCapabilityDto::DaemonToolGatewayV1,
                ProtocolCapabilityDto::ModelToolLoopV1,
            ]))
            .is_ok()
        );

        // Without the gateway the dependency gate does not fire.
        assert!(
            require_gateway_tool_loop(&negotiated(vec![ProtocolCapabilityDto::ModelToolLoopV1,]))
                .is_ok()
        );
        assert!(require_gateway_tool_loop(&negotiated(Vec::new())).is_ok());
    }

    #[test]
    fn hello_serializes_post_m5_capabilities_with_exact_json_names() {
        let hello = crate::ProtocolHelloDto::new(
            crate::ProtocolVersionDto::new(1, 1),
            crate::POST_M5_CAPABILITIES.to_vec(),
            "fixture-adapter",
        )
        .expect("fixture hello is valid");
        let json = serde_json::to_string(&hello).expect("hello serializes");
        for name in [
            "provider_profiles_v1",
            "session_fork_v1",
            "normalized_reasoning_stream_v1",
            "agent_activity_v1",
            "user_notifications_v1",
            "daemon_tool_gateway_v1",
            "model_tool_loop_v1",
        ] {
            assert!(
                json.contains(&format!("\"{name}\"")),
                "hello JSON must carry the {name} capability name"
            );
        }
        assert!(json.contains("\"version\":{\"major\":1,\"minor\":1}"));
        assert!(json.contains("\"adapter_name\":\"fixture-adapter\""));
    }

    #[test]
    fn compatible_minor_hello_fixture_deserializes_at_protocol_1_1() {
        let hello: crate::ProtocolHelloDto = serde_json::from_str(include_str!(
            "../tests/fixtures/goldens/hello-compatible-minor-v1.json"
        ))
        .expect("compatible-minor fixture must decode");
        assert_eq!(hello.version(), crate::ProtocolVersionDto::new(1, 1));
        assert_eq!(
            hello.capabilities(),
            crate::POST_M5_CAPABILITIES.as_slice(),
            "compatible-minor fixture must declare all post-M5 capabilities"
        );
        assert!(
            hello
                .version()
                .ensure_compatible_with(crate::ProtocolVersionDto::new(1, 1))
                .is_ok()
        );
    }

    #[test]
    fn incompatible_major_hello_fixture_fails_version_compatibility() {
        let hello: crate::ProtocolHelloDto = serde_json::from_str(include_str!(
            "../tests/fixtures/goldens/hello-incompatible-major-v2.json"
        ))
        .expect("incompatible-major fixture must decode");
        assert_eq!(hello.version(), crate::ProtocolVersionDto::new(2, 0));
        assert_eq!(
            hello
                .version()
                .ensure_compatible_with(crate::ProtocolVersionDto::new(1, 1))
                .expect_err("protocol major 2 is incompatible with 1.1")
                .code(),
            "incompatible_protocol_version"
        );
    }

    #[test]
    fn unnegotiated_capability_hello_fixture_fails_closed_at_the_gate() {
        let hello: crate::ProtocolHelloDto = serde_json::from_str(include_str!(
            "../tests/fixtures/goldens/hello-unnegotiated-capability-v1.json"
        ))
        .expect("unnegotiated-capability fixture must decode");
        assert!(
            !hello
                .capabilities()
                .contains(&crate::ProtocolCapabilityDto::ModelToolLoopV1),
            "unnegotiated-capability fixture must omit model_tool_loop_v1"
        );
        assert_eq!(
            require_capability(
                hello.capabilities(),
                crate::ProtocolCapabilityDto::ModelToolLoopV1,
            )
            .expect_err("missing model tool loop fails closed")
            .code(),
            "model_tool_loop_required"
        );
    }
}
