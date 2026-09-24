#![allow(
    clippy::expect_used,
    reason = "Control-plane contract fixtures use expect for precise diagnostics."
)]

//! Slice 2 control-plane wire contract evidence.

use intention_protocol::contract_families::{
    AcceptProviderCatalogRemovalCommandDto, AdmitRecoveredRunCommandDto, ArbitraryHeaderPolicyDto,
    ConfigurationEditCommandDto, ConfigurationEditOperationDto, ConfigurationProjectionDto,
    GetConfigurationProjectionQueryDto, GetPricingPolicyQueryDto, GetProviderCatalogQueryDto,
    GetProviderCatalogStatusQueryDto, GetProviderDiscoveryStatusQueryDto,
    GetProviderHealthEvidenceQueryDto, GetProviderUsageQueryDto, GetSessionProviderProfileQueryDto,
    ProviderCatalogActivationState, ProviderCatalogPageDto, ProviderCatalogStatusDto,
    ProviderDiscoveryAttemptDto, ProviderHealthEvidenceDto, ProviderHealthProjectionDto,
    ProviderReadinessDto, ProviderReasoningCatalogProjectionDto, RawTomlEditCommandDto,
    ReconcileUnavailableQueueCommandDto, RejectProviderCatalogCandidateCommandDto,
    ReloadConfigurationCommandDto, ReloadTransactionDto, ResolvedProviderProfileDto,
    RotateProviderCredentialsCommandDto, ServerSideParserConfigDto,
    SetSessionProviderProfileCommandDto, UsageAggregationDto,
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

/// The raw wire frame of one valid provider catalog entry.
const VALID_CATALOG_ENTRY: &str = concat!(
    r#"{"profile_id":"profile-1","profile_revision_id":"rev-1","#,
    r#""display_name":"Provider One","enabled":true,"provider_kind_id":"responses","#,
    r#""kind_descriptor_revision_id":"kind-rev-1","model_id":"model-1","#,
    r#""normalized_endpoint":"https://provider.example","#,
    r#""effective_execution_policy":"execution-policy","capability_subset":["text"],"#,
    r#""credential_transport_mode":"safe_header","#,
    r#""credential_transport_safe_header_name":"x-safe-header","#,
    r#""credential_configured":true,"driver_declared_capabilities":["text"],"readiness":"ready"}"#
);

/// The raw wire frame of one valid pending-removal catalog status.
const CATALOG_STATUS_PENDING_REMOVAL: &str = concat!(
    r#"{"schema_version":"1.1","activation_state":"pending_removal","#,
    r#""degraded_reason":"removal_candidate_pending","#,
    r#""active_catalog_revision_id":"catalog-rev-1","#,
    r#""candidate_catalog_revision_id":"catalog-rev-2","#,
    r#""active_default_profile_id":"profile-1","#,
    r#""removal_impact":{"affected_profile_ids":["profile-1"],"#,
    r#""safe_impact_summary":"one profile affected"},"provider_profiles_negotiated":true}"#
);

/// The raw wire frame of one valid configuration projection.
const CONFIGURATION_PROJECTION: &str = concat!(
    r#"{"schema_version":"1.1","applied_config_revision_id":"config-rev-1","#,
    r#""provider_kind":"openrouter","model_id":"model-1","credential_configured":true,"#,
    r#""provider_execution_policy":"execution-policy","reload_status":"active"}"#
);

/// The raw wire frame of one valid usage aggregation.
const USAGE_AGGREGATION: &str = concat!(
    r#"{"profile_id":"profile-1","provider_profile_revision_id":"rev-1","#,
    r#""model_id":"model-1","request_count":12,"input_units":1000,"output_units":500,"#,
    r#""reasoning_units":250,"usage_period_start":100,"usage_period_end":200}"#
);

/// The raw wire frame of one valid health projection with one observation.
const HEALTH_PROJECTION: &str = concat!(
    r#"{"provider_id":"profile-1","observations":[{"profile_id":"profile-1","#,
    r#""provider_profile_revision_id":"rev-1","health_attempt_id":"attempt-1","#,
    r#""check_contract_revision":"health-check-v1","observed_availability":"available","#,
    r#""observed_at":100,"failure_category":null,"safe_diagnostic_code":null}],"#,
    r#""safe_reason_code":null,"observed_at":100}"#
);

/// The raw wire frame of one safely rejected reload transaction.
const RELOAD_TRANSACTION_REJECTED: &str = concat!(
    r#"{"transaction_id":"transaction-1","previous_config_revision":"config-rev-1","#,
    r#""candidate_config_revision":"config-rev-2","validation_result":"invalid","#,
    r#""commit_outcome":"rejected","safe_failure_code":null,"safe_failure_detail":null}"#
);

/// The raw wire frame of one unavailable health observation without a category.
const HEALTH_EVIDENCE_UNAVAILABLE: &str = concat!(
    r#"{"profile_id":"profile-1","provider_profile_revision_id":"rev-1","#,
    r#""health_attempt_id":"attempt-1","check_contract_revision":"health-check-v1","#,
    r#""observed_availability":"unavailable","observed_at":100,"failure_category":null,"#,
    r#""safe_diagnostic_code":null}"#
);

/// Builds the raw wire frame of one catalog page carrying `entries`.
fn catalog_page_wire(entries: &str) -> String {
    format!(
        r#"{{"schema_version":"1.1","catalog_revision_id":"catalog-rev-1","entries":[{entries}],"next_page_token":null,"has_more":false}}"#
    )
}

/// Decodes a raw wire frame and returns the typed decode-error text.
fn decode_error<T: serde::de::DeserializeOwned + core::fmt::Debug>(wire: &str) -> String {
    serde_json::from_str::<T>(wire)
        .expect_err("an invalid control-plane frame must not decode")
        .to_string()
}

#[test]
fn control_plane_decode_rejects_invalid_catalog_frames_with_typed_codes() {
    // Every fixture below is raw wire text, never a typed struct, so a passing
    // assertion proves the decode path itself enforces the invariant.
    let blank_text = catalog_page_wire(&VALID_CATALOG_ENTRY.replace("\"Provider One\"", "\"   \""));
    let over_long_name = "a".repeat(257);
    let over_long_text = catalog_page_wire(
        &VALID_CATALOG_ENTRY.replace("\"Provider One\"", &format!("\"{over_long_name}\"")),
    );
    let credential_shaped =
        catalog_page_wire(&VALID_CATALOG_ENTRY.replace("\"Provider One\"", "\"sk-live-secret\""));
    let duplicated = catalog_page_wire(&format!("{VALID_CATALOG_ENTRY},{VALID_CATALOG_ENTRY}"));
    let higher_profile = VALID_CATALOG_ENTRY.replace("\"profile-1\"", "\"profile-2\"");
    let unsorted = catalog_page_wire(&format!("{higher_profile},{VALID_CATALOG_ENTRY}"));
    let valid_page = catalog_page_wire(VALID_CATALOG_ENTRY);
    let missing_token = valid_page.replace("\"has_more\":false", "\"has_more\":true");
    let stale_token = valid_page.replace(
        "\"next_page_token\":null",
        "\"next_page_token\":\"opaque-page-cursor-01\"",
    );
    let wrong_version =
        valid_page.replace("\"schema_version\":\"1.1\"", "\"schema_version\":\"9.9\"");
    let cases: [(&str, &str); 8] = [
        (&blank_text, "provider_catalog_entry_invalid"),
        (&over_long_text, "provider_catalog_entry_invalid"),
        (&credential_shaped, "credentials_forbidden"),
        (&duplicated, "provider_catalog_unsorted"),
        (&unsorted, "provider_catalog_unsorted"),
        (&missing_token, "provider_catalog_invalid"),
        (&stale_token, "provider_catalog_invalid"),
        (&wrong_version, "incompatible_protocol_version"),
    ];
    for (wire, code) in cases {
        let error = decode_error::<ProviderCatalogPageDto>(wire);
        assert!(error.contains(code), "expected {code} in {error}");
    }
}

#[test]
fn control_plane_decode_rejects_invalid_family_frames_with_typed_codes() {
    let blank_reload_status = CONFIGURATION_PROJECTION.replace("\"active\"", "\"   \"");
    let wrong_version = CONFIGURATION_PROJECTION
        .replace("\"schema_version\":\"1.1\"", "\"schema_version\":\"9.9\"");
    /// One rejection case: the raw frame, the expected error code, and its decoder.
    type RejectionCase<'a> = (&'a str, &'static str, fn(&str) -> String);
    let cases: [RejectionCase<'_>; 14] = [
        (
            &blank_reload_status,
            "configuration_projection_invalid",
            decode_error::<ConfigurationProjectionDto>,
        ),
        (
            &wrong_version,
            "incompatible_protocol_version",
            decode_error::<ConfigurationProjectionDto>,
        ),
        (
            RELOAD_TRANSACTION_REJECTED,
            "configuration_reload_invalid",
            decode_error::<ReloadTransactionDto>,
        ),
        (
            HEALTH_EVIDENCE_UNAVAILABLE,
            "provider_health_evidence_invalid",
            decode_error::<ProviderHealthEvidenceDto>,
        ),
        (
            r#"{"profile_id":"profile-1","provider_profile_revision_id":"rev-1","model_id":"model-1","request_count":1,"input_units":1,"output_units":1,"reasoning_units":1,"usage_period_start":200,"usage_period_end":100}"#,
            "provider_usage_invalid",
            decode_error::<UsageAggregationDto>,
        ),
        (
            r#"{"provider_kind_id":"responses","model_id":"model-1","supported_effort_levels":["low","low"],"responses_reasoning_modes":["standard"],"projection_revision":"projection-1"}"#,
            "provider_reasoning_catalog_invalid",
            decode_error::<ProviderReasoningCatalogProjectionDto>,
        ),
        (
            r#"{"policy_revision":"policy-1","kind_descriptor_revision_id":"kind-rev-1","allowed_header_names":[]}"#,
            "arbitrary_header_policy_invalid",
            decode_error::<ArbitraryHeaderPolicyDto>,
        ),
        (
            r#"{"operation_id":"operation-1","expected_config_revision":"config-rev-1","operations":[]}"#,
            "configuration_edit_invalid",
            decode_error::<ConfigurationEditCommandDto>,
        ),
        (
            r#"{"vllm":{"parser_id":"   ","bounded_limits":"limits-1"}}"#,
            "server_side_parser_invalid",
            decode_error::<ServerSideParserConfigDto>,
        ),
        (
            r#"{"kind":"resolved","data":{"profile_id":"sk-live-secret","profile_revision_id":"rev-1"}}"#,
            "credentials_forbidden",
            decode_error::<ResolvedProviderProfileDto>,
        ),
        (
            r#"{"schema_version":"1.1","activation_state":"pending_removal","degraded_reason":null,"active_catalog_revision_id":"catalog-rev-1","candidate_catalog_revision_id":"catalog-rev-2","active_default_profile_id":null,"removal_impact":null,"provider_profiles_negotiated":true}"#,
            "provider_catalog_status_invalid",
            decode_error::<ProviderCatalogStatusDto>,
        ),
        (
            r#"{"schema_version":"1.1","page_token":"   ","expected_catalog_revision_id":null}"#,
            "invalid_page_token",
            decode_error::<GetProviderCatalogQueryDto>,
        ),
        (
            r#"{"attempt_id":"attempt-1","discovery_scope":"all","phase":"started","started_at":100,"safe_status":"   "}"#,
            "provider_discovery_invalid",
            decode_error::<ProviderDiscoveryAttemptDto>,
        ),
        (
            r#"{"operation_id":"operation-1","expected_config_revision":"config-rev-1","candidate_content":"   "}"#,
            "raw_toml_edit_invalid",
            decode_error::<RawTomlEditCommandDto>,
        ),
    ];
    for (wire, code, decode) in cases {
        let error = decode(wire);
        assert!(error.contains(code), "expected {code} in {error}");
    }
}

#[test]
fn control_plane_decode_rejects_a_non_current_schema_version_for_every_family() {
    fn reject_flipped<T>(value: &T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + core::fmt::Debug,
    {
        let wire = serde_json::to_string(value).expect("control-plane value encodes");
        let flipped = wire.replace("\"schema_version\":\"1.1\"", "\"schema_version\":\"9.9\"");
        assert_ne!(
            wire, flipped,
            "the encoded frame must carry the exact current schema version"
        );
        let error = decode_error::<T>(&flipped);
        assert!(
            error.contains("incompatible_protocol_version"),
            "expected a typed version rejection in {error}"
        );
    }

    reject_flipped(&catalog_query());
    reject_flipped(&ProviderCatalogPageDto {
        schema_version: "1.1".to_owned(),
        catalog_revision_id: "catalog-rev-1".to_owned(),
        entries: Vec::new(),
        next_page_token: None,
        has_more: false,
    });
    reject_flipped(&GetProviderCatalogStatusQueryDto {
        schema_version: "1.1".to_owned(),
    });
    reject_flipped(&ProviderCatalogStatusDto {
        schema_version: "1.1".to_owned(),
        activation_state: ProviderCatalogActivationState::Active,
        degraded_reason: None,
        active_catalog_revision_id: Some("catalog-rev-1".to_owned()),
        candidate_catalog_revision_id: None,
        active_default_profile_id: Some("profile-1".to_owned()),
        removal_impact: None,
        provider_profiles_negotiated: true,
    });
    reject_flipped(&set_profile_command());
    reject_flipped(&GetSessionProviderProfileQueryDto {
        schema_version: "1.1".to_owned(),
        session_id: "session-1".to_owned(),
    });
    reject_flipped(&GetProviderUsageQueryDto {
        schema_version: "1.1".to_owned(),
        profile_id: "profile-1".to_owned(),
        usage_period_start: 100,
        usage_period_end: 200,
    });
    reject_flipped(&GetProviderHealthEvidenceQueryDto {
        schema_version: "1.1".to_owned(),
        provider_id: "profile-1".to_owned(),
    });
    reject_flipped(&GetProviderDiscoveryStatusQueryDto {
        schema_version: "1.1".to_owned(),
        attempt_id: None,
    });
    reject_flipped(&GetPricingPolicyQueryDto {
        schema_version: "1.1".to_owned(),
        model_id: None,
    });
    reject_flipped(&GetConfigurationProjectionQueryDto {
        schema_version: "1.1".to_owned(),
    });
    reject_flipped(&ConfigurationProjectionDto {
        schema_version: "1.1".to_owned(),
        applied_config_revision_id: "config-rev-1".to_owned(),
        provider_kind: "openrouter".to_owned(),
        model_id: "model-1".to_owned(),
        credential_configured: true,
        provider_execution_policy: "execution-policy".to_owned(),
        reload_status: "active".to_owned(),
    });
}

#[test]
fn converted_control_plane_families_decode_valid_raw_frames_unchanged() {
    let page_wire = catalog_page_wire(VALID_CATALOG_ENTRY);
    let page: ProviderCatalogPageDto =
        serde_json::from_str(&page_wire).expect("valid catalog page decodes");
    assert_eq!(page.schema_version, "1.1");
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].readiness, ProviderReadinessDto::Ready);
    assert_eq!(
        serde_json::to_string(&page).expect("catalog page encodes"),
        page_wire
    );

    let status: ProviderCatalogStatusDto =
        serde_json::from_str(CATALOG_STATUS_PENDING_REMOVAL).expect("valid catalog status decodes");
    assert_eq!(
        status.activation_state,
        ProviderCatalogActivationState::PendingRemoval
    );
    assert_eq!(
        serde_json::to_string(&status).expect("catalog status encodes"),
        CATALOG_STATUS_PENDING_REMOVAL
    );

    let projection: ConfigurationProjectionDto = serde_json::from_str(CONFIGURATION_PROJECTION)
        .expect("valid configuration projection decodes");
    assert_eq!(projection.schema_version, "1.1");
    assert_eq!(projection.reload_status, "active");
    assert_eq!(
        serde_json::to_string(&projection).expect("configuration projection encodes"),
        CONFIGURATION_PROJECTION
    );

    let usage: UsageAggregationDto =
        serde_json::from_str(USAGE_AGGREGATION).expect("valid usage aggregation decodes");
    assert_eq!(usage.request_count, 12);
    assert_eq!(
        serde_json::to_string(&usage).expect("usage aggregation encodes"),
        USAGE_AGGREGATION
    );

    let health: ProviderHealthProjectionDto =
        serde_json::from_str(HEALTH_PROJECTION).expect("valid health projection decodes");
    assert_eq!(health.observations.len(), 1);
    assert_eq!(
        health.observations[0].check_contract_revision,
        "health-check-v1"
    );
    assert_eq!(
        serde_json::to_string(&health).expect("health projection encodes"),
        HEALTH_PROJECTION
    );
}
