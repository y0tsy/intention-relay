#![allow(
    clippy::expect_used,
    reason = "Control-plane contract fixtures use expect for precise diagnostics."
)]

//! Slice 2 control-plane wire contract evidence.

use intention_protocol::contract_families::{
    AcceptProviderCatalogRemovalCommandDto, AdmitRecoveredRunCommandDto,
    ConfigurationEditCommandDto, ConfigurationEditOperationDto, GetProviderCatalogQueryDto,
    GetProviderCatalogStatusQueryDto, GetProviderUsageQueryDto, GetSessionProviderProfileQueryDto,
    RawTomlEditCommandDto, ReconcileUnavailableQueueCommandDto,
    RejectProviderCatalogCandidateCommandDto, ReloadConfigurationCommandDto,
    RotateProviderCredentialsCommandDto, SetSessionProviderProfileCommandDto,
};
use intention_protocol::{
    ProtocolAcceptedDto, ProtocolAcceptedResultDto, ProtocolCommandDto, ProtocolMessageDto,
    ProtocolQueryDto, ProtocolQueryResultDto, ProtocolRequestEnvelopeDto,
    ProtocolRequestPayloadDto, ProtocolResponseEnvelopeDto, ProtocolResponsePayloadDto,
    ProtocolVersionDto,
};
use intention_types::{CorrelationIdDto, SchemaVersionDto};

fn set_profile_command() -> SetSessionProviderProfileCommandDto {
    SetSessionProviderProfileCommandDto {
        schema_version: "1.1".to_owned(),
        session_id: "session-1".to_owned(),
        profile_id: "profile-1".to_owned(),
        expected_session_projection_revision: 7,
        operation_id: "operation-1".to_owned(),
    }
}

fn catalog_query() -> GetProviderCatalogQueryDto {
    GetProviderCatalogQueryDto {
        schema_version: "1.1".to_owned(),
        page_token: None,
        expected_catalog_revision_id: None,
    }
}

#[test]
fn control_plane_commands_and_queries_round_trip_through_wire_envelopes() {
    let schema = SchemaVersionDto::new(1, 1);
    let version = ProtocolVersionDto::new(1, 1);
    let correlation_id = CorrelationIdDto::new();
    let commands = [
        ProtocolCommandDto::SetSessionProviderProfile(set_profile_command()),
        ProtocolCommandDto::AcceptProviderCatalogRemoval(AcceptProviderCatalogRemovalCommandDto {
            candidate_handle: "candidate-1".to_owned(),
            expected_active_catalog_revision_id: "catalog-rev-1".to_owned(),
            expected_candidate_catalog_revision_id: "catalog-rev-2".to_owned(),
            operation_id: "operation-1".to_owned(),
            source_recheck: true,
        }),
        ProtocolCommandDto::RejectProviderCatalogCandidate(
            RejectProviderCatalogCandidateCommandDto {
                candidate_handle: "candidate-1".to_owned(),
                expected_active_catalog_revision_id: "catalog-rev-1".to_owned(),
                operation_id: "operation-1".to_owned(),
            },
        ),
        ProtocolCommandDto::ReconcileUnavailableQueue(ReconcileUnavailableQueueCommandDto {
            session_id: "session-1".to_owned(),
            operation_id: "operation-1".to_owned(),
            page_cursor: Some("opaque-page-cursor-01".to_owned()),
        }),
        ProtocolCommandDto::AdmitRecoveredRun(AdmitRecoveredRunCommandDto {
            session_id: "session-1".to_owned(),
            run_id: "run-1".to_owned(),
            operation_id: "operation-1".to_owned(),
        }),
        ProtocolCommandDto::ReloadConfiguration(ReloadConfigurationCommandDto {
            candidate_snapshot_reference: Some("snapshot-1".to_owned()),
            candidate_edit_reference: None,
            expected_active_config_revision: "config-rev-1".to_owned(),
            operation_id: "operation-1".to_owned(),
            origin: intention_protocol::contract_families::ConfigurationOriginDto::Admin,
        }),
        ProtocolCommandDto::RotateProviderCredentials(RotateProviderCredentialsCommandDto {
            profile_id: "profile-1".to_owned(),
            provider_profile_revision_id: "rev-1".to_owned(),
            expected_credential_composition_revision: "composition-1".to_owned(),
            operation_id: "operation-1".to_owned(),
        }),
        ProtocolCommandDto::SubmitRawTomlEdit(RawTomlEditCommandDto {
            operation_id: "operation-1".to_owned(),
            expected_config_revision: "config-rev-1".to_owned(),
            candidate_content: "[daemon]\nmax_parallel_runs = 2\n".to_owned(),
        }),
        ProtocolCommandDto::ApplyConfigurationEdit(ConfigurationEditCommandDto {
            operation_id: "operation-1".to_owned(),
            expected_config_revision: "config-rev-1".to_owned(),
            operations: vec![ConfigurationEditOperationDto::Set {
                key_path: "daemon.max_parallel_runs".to_owned(),
                safe_value: "2".to_owned(),
            }],
        }),
    ];
    for command in commands {
        let envelope = ProtocolRequestEnvelopeDto::new(
            version,
            correlation_id,
            ProtocolMessageDto::new(schema, ProtocolRequestPayloadDto::Command(command)),
        );
        let wire = serde_json::to_vec(&envelope).expect("command envelope encodes");
        let decoded: ProtocolRequestEnvelopeDto =
            serde_json::from_slice(&wire).expect("command envelope decodes");
        assert_eq!(decoded, envelope);
    }

    let queries = [
        ProtocolQueryDto::GetProviderCatalog(catalog_query()),
        ProtocolQueryDto::GetProviderCatalogStatus(GetProviderCatalogStatusQueryDto {
            schema_version: "1.1".to_owned(),
        }),
        ProtocolQueryDto::GetSessionProviderProfile(GetSessionProviderProfileQueryDto {
            schema_version: "1.1".to_owned(),
            session_id: "session-1".to_owned(),
        }),
        ProtocolQueryDto::GetProviderUsage(GetProviderUsageQueryDto {
            schema_version: "1.1".to_owned(),
            profile_id: "profile-1".to_owned(),
            usage_period_start: 100,
            usage_period_end: 200,
        }),
        ProtocolQueryDto::GetProviderHealthEvidence(
            intention_protocol::contract_families::GetProviderHealthEvidenceQueryDto {
                schema_version: "1.1".to_owned(),
                provider_id: "profile-1".to_owned(),
            },
        ),
        ProtocolQueryDto::GetProviderDiscoveryStatus(
            intention_protocol::contract_families::GetProviderDiscoveryStatusQueryDto {
                schema_version: "1.1".to_owned(),
                attempt_id: Some("attempt-1".to_owned()),
            },
        ),
        ProtocolQueryDto::GetPricingPolicy(
            intention_protocol::contract_families::GetPricingPolicyQueryDto {
                schema_version: "1.1".to_owned(),
                model_id: Some("model-1".to_owned()),
            },
        ),
        ProtocolQueryDto::GetConfigurationProjection(
            intention_protocol::contract_families::GetConfigurationProjectionQueryDto {
                schema_version: "1.1".to_owned(),
            },
        ),
    ];
    for query in queries {
        let envelope = ProtocolRequestEnvelopeDto::new(
            version,
            correlation_id,
            ProtocolMessageDto::new(schema, ProtocolRequestPayloadDto::Query(query)),
        );
        let wire = serde_json::to_vec(&envelope).expect("query envelope encodes");
        let decoded: ProtocolRequestEnvelopeDto =
            serde_json::from_slice(&wire).expect("query envelope decodes");
        assert_eq!(decoded, envelope);
    }
}

#[test]
fn control_plane_acceptance_and_query_results_round_trip_through_wire_payloads() {
    let schema = SchemaVersionDto::new(1, 1);
    let version = ProtocolVersionDto::new(1, 1);
    let correlation_id = CorrelationIdDto::new();

    let accepted = ProtocolAcceptedDto::with_result(
        correlation_id,
        ProtocolAcceptedResultDto::SetSessionProviderProfile(
            intention_protocol::contract_families::SetSessionProviderProfileAcceptedDto {
                session_id: "session-1".to_owned(),
                changed: false,
                resulting_projection_revision: 8,
                resolved:
                    intention_protocol::contract_families::ResolvedProviderProfileDto::Resolved {
                        profile_id: "profile-1".to_owned(),
                        profile_revision_id: "rev-1".to_owned(),
                    },
            },
        ),
    );
    let envelope = ProtocolResponseEnvelopeDto::new(
        version,
        correlation_id,
        ProtocolMessageDto::new(
            schema,
            ProtocolResponsePayloadDto::CommandResult(
                intention_protocol::ProtocolCommandResultDto::Accepted(accepted),
            ),
        ),
    );
    let wire = serde_json::to_vec(&envelope).expect("accepted envelope encodes");
    let decoded: ProtocolResponseEnvelopeDto =
        serde_json::from_slice(&wire).expect("accepted envelope decodes");
    assert_eq!(decoded, envelope);

    let query_results = [
        ProtocolQueryResultDto::ProviderCatalog(
            intention_protocol::contract_families::ProviderCatalogPageDto {
                schema_version: "1.1".to_owned(),
                catalog_revision_id: "catalog-rev-1".to_owned(),
                entries: Vec::new(),
                next_page_token: None,
                has_more: false,
            },
        ),
        ProtocolQueryResultDto::ProviderCatalogStatus(
            intention_protocol::contract_families::ProviderCatalogStatusDto {
                schema_version: "1.1".to_owned(),
                activation_state:
                    intention_protocol::contract_families::ProviderCatalogActivationState::Active,
                degraded_reason: None,
                active_catalog_revision_id: Some("catalog-rev-1".to_owned()),
                candidate_catalog_revision_id: None,
                active_default_profile_id: Some("profile-1".to_owned()),
                removal_impact: None,
                provider_profiles_negotiated: true,
            },
        ),
        ProtocolQueryResultDto::SessionProviderProfile(
            intention_protocol::contract_families::SessionProviderProfileDto {
                session_id: "session-1".to_owned(),
                profile_id: "profile-1".to_owned(),
                resolved:
                    intention_protocol::contract_families::ResolvedProviderProfileDto::Resolved {
                        profile_id: "profile-1".to_owned(),
                        profile_revision_id: "rev-1".to_owned(),
                    },
                session_projection_revision: 8,
                global_default_profile_id: "profile-default".to_owned(),
            },
        ),
        ProtocolQueryResultDto::ProviderUsage(
            intention_protocol::contract_families::UsageAggregationDto {
                profile_id: "profile-1".to_owned(),
                provider_profile_revision_id: "rev-1".to_owned(),
                model_id: "model-1".to_owned(),
                request_count: 12,
                input_units: 1000,
                output_units: 500,
                reasoning_units: 250,
                usage_period_start: 100,
                usage_period_end: 200,
            },
        ),
        ProtocolQueryResultDto::ProviderHealthEvidence(
            intention_protocol::contract_families::ProviderHealthProjectionDto {
                provider_id: "profile-1".to_owned(),
                observations: vec![
                    intention_protocol::contract_families::ProviderHealthEvidenceDto {
                        profile_id: "profile-1".to_owned(),
                        provider_profile_revision_id: "rev-1".to_owned(),
                        health_attempt_id: "attempt-1".to_owned(),
                        check_contract_revision: "health-check-v1".to_owned(),
                        observed_availability:
                            intention_protocol::contract_families::ProviderAvailabilityObservation::Available,
                        observed_at: 100,
                        failure_category: None,
                        safe_diagnostic_code: None,
                    },
                ],
                safe_reason_code: None,
                observed_at: 100,
            },
        ),
        ProtocolQueryResultDto::ProviderDiscoveryStatus(
            intention_protocol::contract_families::ProviderDiscoveryProjectionDto {
                attempt_id: Some("attempt-1".to_owned()),
                phase: Some(
                    intention_protocol::contract_families::ProviderDiscoveryPhase::Terminal,
                ),
                records: vec![
                    intention_protocol::contract_families::ProviderModelDiscoveryRecordDto {
                        discovery_scope: "all".to_owned(),
                        model_id: "gpt-4o".to_owned(),
                        capability_records: vec!["text_input".to_owned()],
                        source_attempt_id: "attempt-1".to_owned(),
                        discovered_at: 100,
                    },
                ],
                safe_status: Some("completed".to_owned()),
            },
        ),
        ProtocolQueryResultDto::PricingPolicy(
            intention_protocol::contract_families::PricingProjectionDto {
                observations: vec![
                    intention_protocol::contract_families::PricingObservationDto {
                        provider_kind_id: "openrouter".to_owned(),
                        model_id: "model-1".to_owned(),
                        bounded_numeric_value: 42,
                        classification: intention_protocol::contract_families::PricingClassification::CapacityObservation,
                        observed_at: 100,
                    },
                ],
                policy_classification: Some(
                    intention_protocol::contract_families::PricingClassification::CapacityObservation,
                ),
                disclaimer: Some(
                    "pricing observations are non-authorizing and never gate admission"
                        .to_owned(),
                ),
            },
        ),
        ProtocolQueryResultDto::ConfigurationProjection(
            intention_protocol::contract_families::ConfigurationProjectionDto {
                schema_version: "1.1".to_owned(),
                applied_config_revision_id: "config-rev-1".to_owned(),
                provider_kind: "openrouter".to_owned(),
                model_id: "model-1".to_owned(),
                credential_configured: true,
                provider_execution_policy: "execution-timeout-30-attempts-2".to_owned(),
                reload_status: "active".to_owned(),
            },
        ),
    ];
    for result in query_results {
        let envelope = ProtocolResponseEnvelopeDto::new(
            version,
            correlation_id,
            ProtocolMessageDto::new(schema, ProtocolResponsePayloadDto::QueryResult(result)),
        );
        let wire = serde_json::to_vec(&envelope).expect("query result envelope encodes");
        let decoded: ProtocolResponseEnvelopeDto =
            serde_json::from_slice(&wire).expect("query result envelope decodes");
        assert_eq!(decoded, envelope);
    }
}

#[test]
fn golden_hello_fixtures_remain_decodable_at_the_current_version() {
    // The Slice 2 control-plane surface is additive: the committed M3/M4/M5
    // hello goldens must keep decoding identically at protocol 1.1.
    for fixture in [
        include_str!("fixtures/goldens/hello-current-version-v1.json"),
        include_str!("fixtures/goldens/hello-unnegotiated-capability-v1.json"),
    ] {
        let hello: intention_protocol::ProtocolHelloDto =
            serde_json::from_str(fixture).expect("golden hello fixture must decode");
        assert_eq!(hello.version(), ProtocolVersionDto::new(1, 1));
        assert_eq!(
            hello.version(),
            intention_protocol::CURRENT_PROTOCOL_VERSION,
            "golden hello fixtures must carry the exact current protocol version"
        );
    }
    // The incompatible-major fixture is intentionally 2.0 and must keep
    // decoding to that version so the version gate still rejects it.
    let incompatible: intention_protocol::ProtocolHelloDto = serde_json::from_str(include_str!(
        "fixtures/goldens/hello-incompatible-major-v2.json"
    ))
    .expect("incompatible-major fixture must decode");
    assert_eq!(incompatible.version(), ProtocolVersionDto::new(2, 0));
    // Unknown control-plane variants fail closed instead of silently
    // decoding into an unrelated shape.
    assert!(
        serde_json::from_str::<ProtocolRequestPayloadDto>(
            r#"{"kind":"command","data":{"kind":"set_session_provider_profile","data":{"schema_version":"1.1","session_id":"session-1","profile_id":"profile-1","expected_session_projection_revision":7,"operation_id":"operation-1","future_additive":true}}}"#
        )
        .is_ok()
    );
    assert!(
        serde_json::from_str::<ProtocolRequestPayloadDto>(
            r#"{"kind":"command","data":{"kind":"unknown_control_plane_command","data":{}}}"#
        )
        .is_err()
    );
}

#[test]
fn control_plane_wire_never_serializes_fake_credentials() {
    const FAKE_SECRETS: [&str; 3] = ["sk-test", "Bearer secret", "api_key"];
    let payloads = [
        serde_json::to_value(ProtocolCommandDto::SetSessionProviderProfile(
            set_profile_command(),
        ))
        .expect("command serializes"),
        serde_json::to_value(ProtocolQueryDto::GetProviderCatalog(catalog_query()))
            .expect("query serializes"),
        serde_json::to_value(ProtocolAcceptedResultDto::ReconcileUnavailableQueue(
            intention_protocol::contract_families::ReconcileUnavailableQueueAcceptedDto {
                session_id: "session-1".to_owned(),
                page_cursor: None,
                promoted_count: 2,
            },
        ))
        .expect("accepted result serializes"),
        serde_json::to_value(ProtocolQueryResultDto::ProviderUsage(
            intention_protocol::contract_families::UsageAggregationDto {
                profile_id: "profile-1".to_owned(),
                provider_profile_revision_id: "rev-1".to_owned(),
                model_id: "model-1".to_owned(),
                request_count: 1,
                input_units: 1,
                output_units: 1,
                reasoning_units: 1,
                usage_period_start: 100,
                usage_period_end: 200,
            },
        ))
        .expect("query result serializes"),
        serde_json::to_value(ProtocolQueryDto::GetProviderHealthEvidence(
            intention_protocol::contract_families::GetProviderHealthEvidenceQueryDto {
                schema_version: "1.1".to_owned(),
                provider_id: "profile-1".to_owned(),
            },
        ))
        .expect("query serializes"),
        serde_json::to_value(ProtocolQueryResultDto::PricingPolicy(
            intention_protocol::contract_families::PricingProjectionDto {
                observations: vec![
                    intention_protocol::contract_families::PricingObservationDto {
                        provider_kind_id: "openrouter".to_owned(),
                        model_id: "model-1".to_owned(),
                        bounded_numeric_value: 1,
                        classification: intention_protocol::contract_families::PricingClassification::CapacityObservation,
                        observed_at: 1,
                    },
                ],
                policy_classification: None,
                disclaimer: Some("safe".to_owned()),
            },
        ))
        .expect("query result serializes"),
        serde_json::to_value(ProtocolQueryResultDto::ConfigurationProjection(
            intention_protocol::contract_families::ConfigurationProjectionDto {
                schema_version: "1.1".to_owned(),
                applied_config_revision_id: "config-rev-1".to_owned(),
                provider_kind: "openrouter".to_owned(),
                model_id: "model-1".to_owned(),
                credential_configured: true,
                provider_execution_policy: "execution-timeout-30-attempts-2".to_owned(),
                reload_status: "active".to_owned(),
            },
        ))
        .expect("query result serializes"),
    ];
    for payload in payloads {
        let json = payload.to_string();
        for secret in FAKE_SECRETS {
            assert!(
                !json.contains(secret),
                "control-plane wire must not contain {secret:?}"
            );
        }
    }
}
