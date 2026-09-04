#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Control-plane client fixtures use direct assertions and controlled fixture servers."
)]

//! Zone 4 control-plane client contract tests.
//!
//! A local fixture daemon (bound before the client connects, matching the
//! existing client-contract pattern) asserts the exact protocol variant each
//! client method issues and replies with the matching typed payload. The
//! fake-secret sweep proves credential-shaped commands fail closed
//! client-side and that no captured request or decoded projection ever
//! carries a fake secret. The negotiation-gate test proves an unsupported
//! peer fails closed.

use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use intention_client::{DaemonLauncher, IntentionClient};
use intention_protocol::contract_families::{
    ConfigurationEditCommandDto, ConfigurationEditOperationDto, ConfigurationProjectionDto,
    CredentialRotationResultDto, GetPricingPolicyQueryDto, GetProviderDiscoveryStatusQueryDto,
    GetProviderHealthEvidenceQueryDto, PricingObservationDto, PricingProjectionDto,
    ProviderAvailabilityObservation, ProviderDiscoveryProjectionDto, ProviderHealthEvidenceDto,
    ProviderHealthProjectionDto, ProviderModelDiscoveryRecordDto, RawTomlEditCommandDto,
    ReloadConfigurationCommandDto, ReloadTransactionDto, RotateProviderCredentialsCommandDto,
};
use intention_protocol::{
    ProtocolAcceptedDto, ProtocolAcceptedResultDto, ProtocolCommandResultDto, ProtocolHelloDto,
    ProtocolMessageDto, ProtocolQueryResultDto, ProtocolRequestPayloadDto,
    ProtocolResponseEnvelopeDto, ProtocolResponsePayloadDto, ProtocolVersionDto,
};
use intention_transport::{LocalEndpoint, LocalListener, local_protocol_version, negotiate_daemon};
use intention_types::{DtoResult, ErrorDto, SchemaVersionDto};

const SCHEMA_VERSION: SchemaVersionDto = intention_protocol::CURRENT_DTO_SCHEMA_VERSION;
const FAKE_SECRET: &str = "sk-zone4-client-fake-secret";

/// The exact protocol request variant one fixture connection must receive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExpectedRequest {
    ReloadConfiguration,
    SubmitRawTomlEdit,
    ApplyConfigurationEdit,
    RotateProviderCredentials,
    ProviderHealth,
    DiscoveryStatus,
    Pricing,
    ConfigurationProjection,
}

/// The canned daemon reply for one fixture connection.
#[derive(Clone, Debug)]
enum FixtureReply {
    ReloadTransaction(ReloadTransactionDto),
    RotationResult(CredentialRotationResultDto),
    HealthProjection(ProviderHealthProjectionDto),
    DiscoveryProjection(ProviderDiscoveryProjectionDto),
    PricingProjection(PricingProjectionDto),
    ConfigurationProjection(ConfigurationProjectionDto),
    CommandRejected(ErrorDto),
    QueryRejected(ErrorDto),
}

/// A launcher that must never run: the fixture server is bound before the
/// client connects, so direct control-plane calls never bootstrap.
struct UnusedLauncher;

impl DaemonLauncher for UnusedLauncher {
    fn launch(&self, _endpoint: &LocalEndpoint) -> DtoResult<()> {
        Err(ErrorDto::unavailable(
            "fixture_launch_unexpected",
            "fixture launcher must not run for direct control-plane calls",
        ))
    }
}

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);

fn endpoint() -> LocalEndpoint {
    let sequence = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    LocalEndpoint::from_instance_id(format!(
        "client-control-plane-{}-{sequence}",
        std::process::id()
    ))
    .expect("fixture instance name is valid")
}

fn fixture_client(endpoint: LocalEndpoint) -> IntentionClient {
    IntentionClient::new(
        endpoint,
        "fixture-control-plane-client",
        Box::new(UnusedLauncher),
    )
    .expect("fixture client is valid")
}

fn daemon_hello() -> ProtocolHelloDto {
    ProtocolHelloDto::new(
        local_protocol_version(),
        vec![
            intention_protocol::ProtocolCapabilityDto::SessionSubscriptions,
            intention_protocol::ProtocolCapabilityDto::CorrelatedRequests,
            intention_protocol::ProtocolCapabilityDto::DaemonHealth,
            intention_protocol::ProtocolCapabilityDto::ProviderProfilesV1,
        ],
        "fixture-control-plane-daemon",
    )
    .expect("fixture daemon hello is valid")
}

/// Binds one fixture daemon that accepts one connection, asserts the exact
/// expected control-plane request variant, and replies with the canned reply.
fn start_fixture_server(
    endpoint: LocalEndpoint,
    expected: ExpectedRequest,
    reply: FixtureReply,
) -> thread::JoinHandle<()> {
    let listener = LocalListener::bind(endpoint).expect("fixture listener binds");
    thread::spawn(move || {
        let connection = listener.accept().expect("fixture client connects");
        serve_fixture_connection(connection, expected, reply);
    })
}

fn serve_fixture_connection(
    mut connection: intention_transport::LocalConnection,
    expected: ExpectedRequest,
    reply: FixtureReply,
) {
    negotiate_daemon(&mut connection, daemon_hello()).expect("fixture hello negotiates");
    let request = connection
        .receive_request()
        .expect("fixture request arrives");
    let received = match request.message().payload() {
        ProtocolRequestPayloadDto::Command(
            intention_protocol::ProtocolCommandDto::ReloadConfiguration(_),
        ) => ExpectedRequest::ReloadConfiguration,
        ProtocolRequestPayloadDto::Command(
            intention_protocol::ProtocolCommandDto::SubmitRawTomlEdit(_),
        ) => ExpectedRequest::SubmitRawTomlEdit,
        ProtocolRequestPayloadDto::Command(
            intention_protocol::ProtocolCommandDto::ApplyConfigurationEdit(_),
        ) => ExpectedRequest::ApplyConfigurationEdit,
        ProtocolRequestPayloadDto::Command(
            intention_protocol::ProtocolCommandDto::RotateProviderCredentials(_),
        ) => ExpectedRequest::RotateProviderCredentials,
        ProtocolRequestPayloadDto::Query(
            intention_protocol::ProtocolQueryDto::GetProviderHealthEvidence(_),
        ) => ExpectedRequest::ProviderHealth,
        ProtocolRequestPayloadDto::Query(
            intention_protocol::ProtocolQueryDto::GetProviderDiscoveryStatus(_),
        ) => ExpectedRequest::DiscoveryStatus,
        ProtocolRequestPayloadDto::Query(
            intention_protocol::ProtocolQueryDto::GetPricingPolicy(_),
        ) => ExpectedRequest::Pricing,
        ProtocolRequestPayloadDto::Query(
            intention_protocol::ProtocolQueryDto::GetConfigurationProjection(_),
        ) => ExpectedRequest::ConfigurationProjection,
        _ => panic!("fixture receives an unexpected control-plane request"),
    };
    assert_eq!(
        received, expected,
        "client must issue the exact expected variant"
    );
    let captured = format!("{:?}", request.message().payload());
    assert!(
        !captured.contains(FAKE_SECRET),
        "the captured request must never carry the fake secret"
    );
    let payload = match reply {
        FixtureReply::ReloadTransaction(transaction) => ProtocolResponsePayloadDto::CommandResult(
            ProtocolCommandResultDto::Accepted(ProtocolAcceptedDto::with_result(
                request.correlation_id(),
                ProtocolAcceptedResultDto::ReloadConfiguration(transaction),
            )),
        ),
        FixtureReply::RotationResult(result) => ProtocolResponsePayloadDto::CommandResult(
            ProtocolCommandResultDto::Accepted(ProtocolAcceptedDto::with_result(
                request.correlation_id(),
                ProtocolAcceptedResultDto::RotateProviderCredentials(result),
            )),
        ),
        FixtureReply::CommandRejected(error) => {
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Rejected(error))
        }
        FixtureReply::HealthProjection(projection) => ProtocolResponsePayloadDto::QueryResult(
            ProtocolQueryResultDto::ProviderHealthEvidence(projection),
        ),
        FixtureReply::DiscoveryProjection(projection) => ProtocolResponsePayloadDto::QueryResult(
            ProtocolQueryResultDto::ProviderDiscoveryStatus(projection),
        ),
        FixtureReply::PricingProjection(projection) => ProtocolResponsePayloadDto::QueryResult(
            ProtocolQueryResultDto::PricingPolicy(projection),
        ),
        FixtureReply::ConfigurationProjection(projection) => {
            ProtocolResponsePayloadDto::QueryResult(
                ProtocolQueryResultDto::ConfigurationProjection(projection),
            )
        }
        FixtureReply::QueryRejected(error) => {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error))
        }
    };
    connection
        .send_response(&ProtocolResponseEnvelopeDto::new(
            local_protocol_version(),
            request.correlation_id(),
            ProtocolMessageDto::new(SCHEMA_VERSION, payload),
        ))
        .expect("fixture response sends");
}

fn transaction(committed: bool) -> ReloadTransactionDto {
    ReloadTransactionDto {
        transaction_id: "operation-1".to_owned(),
        previous_config_revision: "revision-1".to_owned(),
        candidate_config_revision: "revision-2".to_owned(),
        validation_result:
            intention_protocol::contract_families::ConfigurationValidationOutcomeDto::Valid,
        commit_outcome: if committed {
            intention_protocol::contract_families::ConfigurationCommitOutcomeDto::Committed
        } else {
            intention_protocol::contract_families::ConfigurationCommitOutcomeDto::Rejected
        },
        safe_failure_code: if committed {
            None
        } else {
            Some("fixture_rejected".to_owned())
        },
        safe_failure_detail: None,
    }
}

fn rotation_result() -> CredentialRotationResultDto {
    CredentialRotationResultDto {
        operation_id: "operation-1".to_owned(),
        profile_id: "default".to_owned(),
        safe_credential_composition_revision: "composition-rev-1".to_owned(),
        rotated: true,
    }
}

fn health_projection() -> ProviderHealthProjectionDto {
    ProviderHealthProjectionDto {
        provider_id: "default".to_owned(),
        observations: vec![ProviderHealthEvidenceDto {
            profile_id: "default".to_owned(),
            provider_profile_revision_id: "health-profile-0123456789abcdef".to_owned(),
            health_attempt_id:
                "health-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
            check_contract_revision: "health-check-v1".to_owned(),
            observed_availability: ProviderAvailabilityObservation::Available,
            observed_at: 100,
            failure_category: None,
            safe_diagnostic_code: None,
        }],
        safe_reason_code: None,
        observed_at: 100,
    }
}

fn discovery_projection() -> ProviderDiscoveryProjectionDto {
    ProviderDiscoveryProjectionDto {
        attempt_id: Some("attempt-1".to_owned()),
        phase: Some(intention_protocol::contract_families::ProviderDiscoveryPhase::Terminal),
        records: vec![ProviderModelDiscoveryRecordDto {
            discovery_scope: "all".to_owned(),
            model_id: "gpt-4o".to_owned(),
            capability_records: vec!["text_input".to_owned()],
            source_attempt_id: "attempt-1".to_owned(),
            discovered_at: 100,
        }],
        safe_status: Some("completed".to_owned()),
    }
}

fn pricing_projection() -> PricingProjectionDto {
    PricingProjectionDto {
        observations: vec![PricingObservationDto {
            provider_kind_id: "openrouter".to_owned(),
            model_id: "model-a".to_owned(),
            bounded_numeric_value: 42,
            classification:
                intention_protocol::contract_families::PricingClassification::CapacityObservation,
            observed_at: 100,
        }],
        policy_classification: Some(
            intention_protocol::contract_families::PricingClassification::CapacityObservation,
        ),
        disclaimer: Some(
            "pricing observations are non-authorizing and never gate admission".to_owned(),
        ),
    }
}

fn configuration_projection() -> ConfigurationProjectionDto {
    ConfigurationProjectionDto {
        schema_version: "1.1".to_owned(),
        applied_config_revision_id: "revision-1".to_owned(),
        provider_kind: "openrouter".to_owned(),
        model_id: "model-a".to_owned(),
        credential_configured: true,
        provider_execution_policy: "execution-timeout-30-attempts-2".to_owned(),
        reload_status: "active".to_owned(),
    }
}

#[test]
fn reload_configuration_issues_the_reload_command_and_decodes_the_transaction() {
    let fixture_endpoint = endpoint();
    let server = start_fixture_server(
        fixture_endpoint.clone(),
        ExpectedRequest::ReloadConfiguration,
        FixtureReply::ReloadTransaction(transaction(true)),
    );
    let outcome = fixture_client(fixture_endpoint)
        .reload_configuration(ReloadConfigurationCommandDto {
            candidate_snapshot_reference: Some("operation-1".to_owned()),
            candidate_edit_reference: None,
            expected_active_config_revision: "revision-1".to_owned(),
            operation_id: "operation-1".to_owned(),
            origin: intention_protocol::contract_families::ConfigurationOriginDto::Admin,
        })
        .expect("reload transaction decodes");
    assert_eq!(outcome.transaction_id, "operation-1");
    assert_eq!(outcome.previous_config_revision, "revision-1");
    assert_eq!(outcome.candidate_config_revision, "revision-2");
    assert_eq!(
        outcome.commit_outcome,
        intention_protocol::contract_families::ConfigurationCommitOutcomeDto::Committed
    );
    server.join().expect("fixture server completes");
}

#[test]
fn raw_toml_edit_and_typed_edit_issue_their_exact_commands() {
    let raw_endpoint = endpoint();
    let raw_server = start_fixture_server(
        raw_endpoint.clone(),
        ExpectedRequest::SubmitRawTomlEdit,
        FixtureReply::ReloadTransaction(transaction(true)),
    );
    let outcome = fixture_client(raw_endpoint)
        .submit_raw_toml_edit(RawTomlEditCommandDto {
            operation_id: "operation-1".to_owned(),
            expected_config_revision: "revision-1".to_owned(),
            candidate_content: "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"model-b\"\ncredential = \"fake-plain\"\n"
                .to_owned(),
        })
        .expect("raw edit transaction decodes");
    assert_eq!(outcome.transaction_id, "operation-1");
    raw_server.join().expect("fixture server completes");

    let typed_endpoint = endpoint();
    let typed_server = start_fixture_server(
        typed_endpoint.clone(),
        ExpectedRequest::ApplyConfigurationEdit,
        FixtureReply::ReloadTransaction(transaction(false)),
    );
    let rejected = fixture_client(typed_endpoint)
        .apply_configuration_edit(ConfigurationEditCommandDto {
            operation_id: "operation-2".to_owned(),
            expected_config_revision: "revision-1".to_owned(),
            operations: vec![ConfigurationEditOperationDto::Set {
                key_path: "provider.model".to_owned(),
                safe_value: "model-b".to_owned(),
            }],
        })
        .expect("typed edit transaction decodes");
    assert_eq!(
        rejected.commit_outcome,
        intention_protocol::contract_families::ConfigurationCommitOutcomeDto::Rejected
    );
    typed_server.join().expect("fixture server completes");
}

#[test]
fn rotate_credential_issues_the_rotation_command_and_decodes_the_result() {
    let fixture_endpoint = endpoint();
    let server = start_fixture_server(
        fixture_endpoint.clone(),
        ExpectedRequest::RotateProviderCredentials,
        FixtureReply::RotationResult(rotation_result()),
    );
    let result = fixture_client(fixture_endpoint)
        .rotate_credential(RotateProviderCredentialsCommandDto {
            profile_id: "default".to_owned(),
            provider_profile_revision_id: "profile-rev-1".to_owned(),
            expected_credential_composition_revision: "composition-rev-1".to_owned(),
            operation_id: "operation-1".to_owned(),
        })
        .expect("rotation result decodes");
    assert!(result.rotated);
    assert_eq!(result.profile_id, "default");
    assert_eq!(
        result.safe_credential_composition_revision,
        "composition-rev-1"
    );
    assert!(!format!("{result:?}").contains(FAKE_SECRET));
    server.join().expect("fixture server completes");
}

#[test]
fn control_plane_queries_decode_safe_projections() {
    let health_endpoint = endpoint();
    let health_server = start_fixture_server(
        health_endpoint.clone(),
        ExpectedRequest::ProviderHealth,
        FixtureReply::HealthProjection(health_projection()),
    );
    let health = fixture_client(health_endpoint)
        .provider_health(GetProviderHealthEvidenceQueryDto {
            schema_version: "1.1".to_owned(),
            provider_id: "default".to_owned(),
        })
        .expect("health projection decodes");
    assert_eq!(health.provider_id, "default");
    assert_eq!(health.observations.len(), 1);
    assert_eq!(
        health.observations[0].check_contract_revision,
        "health-check-v1"
    );
    assert!(!format!("{health:?}").contains(FAKE_SECRET));
    health_server.join().expect("fixture server completes");

    let discovery_endpoint = endpoint();
    let discovery_server = start_fixture_server(
        discovery_endpoint.clone(),
        ExpectedRequest::DiscoveryStatus,
        FixtureReply::DiscoveryProjection(discovery_projection()),
    );
    let discovery = fixture_client(discovery_endpoint)
        .discovery_status(GetProviderDiscoveryStatusQueryDto {
            schema_version: "1.1".to_owned(),
            attempt_id: Some("attempt-1".to_owned()),
        })
        .expect("discovery projection decodes");
    assert_eq!(discovery.records.len(), 1);
    assert_eq!(discovery.records[0].model_id, "gpt-4o");
    assert!(!format!("{discovery:?}").contains(FAKE_SECRET));
    discovery_server.join().expect("fixture server completes");

    let pricing_endpoint = endpoint();
    let pricing_server = start_fixture_server(
        pricing_endpoint.clone(),
        ExpectedRequest::Pricing,
        FixtureReply::PricingProjection(pricing_projection()),
    );
    let pricing = fixture_client(pricing_endpoint)
        .pricing(GetPricingPolicyQueryDto {
            schema_version: "1.1".to_owned(),
            model_id: Some("model-a".to_owned()),
        })
        .expect("pricing projection decodes");
    assert_eq!(pricing.observations.len(), 1);
    assert_eq!(pricing.observations[0].bounded_numeric_value, 42);
    assert!(pricing.disclaimer.is_some());
    assert!(!format!("{pricing:?}").contains(FAKE_SECRET));
    pricing_server.join().expect("fixture server completes");

    let configuration_endpoint = endpoint();
    let configuration_server = start_fixture_server(
        configuration_endpoint.clone(),
        ExpectedRequest::ConfigurationProjection,
        FixtureReply::ConfigurationProjection(configuration_projection()),
    );
    let configuration = fixture_client(configuration_endpoint)
        .configuration_projection()
        .expect("configuration projection decodes");
    assert_eq!(configuration.provider_kind, "openrouter");
    assert_eq!(configuration.model_id, "model-a");
    assert!(configuration.credential_configured);
    assert_eq!(configuration.reload_status, "active");
    assert!(!format!("{configuration:?}").contains(FAKE_SECRET));
    configuration_server
        .join()
        .expect("fixture server completes");
}

#[test]
fn control_plane_client_propagates_daemon_rejections() {
    let reload_endpoint = endpoint();
    let reload_server = start_fixture_server(
        reload_endpoint.clone(),
        ExpectedRequest::ReloadConfiguration,
        FixtureReply::CommandRejected(ErrorDto::validation(
            "config_revision_mismatch",
            "fixture daemon rejects the reload",
        )),
    );
    let error = fixture_client(reload_endpoint)
        .reload_configuration(ReloadConfigurationCommandDto {
            candidate_snapshot_reference: Some("operation-1".to_owned()),
            candidate_edit_reference: None,
            expected_active_config_revision: "revision-stale".to_owned(),
            operation_id: "operation-1".to_owned(),
            origin: intention_protocol::contract_families::ConfigurationOriginDto::Admin,
        })
        .expect_err("daemon rejection propagates");
    assert_eq!(error.code(), "config_revision_mismatch");
    reload_server.join().expect("fixture server completes");

    let health_endpoint = endpoint();
    let health_server = start_fixture_server(
        health_endpoint.clone(),
        ExpectedRequest::ProviderHealth,
        FixtureReply::QueryRejected(ErrorDto::validation(
            "provider_health_unavailable",
            "fixture daemon rejects the health query",
        )),
    );
    let error = fixture_client(health_endpoint)
        .provider_health(GetProviderHealthEvidenceQueryDto {
            schema_version: "1.1".to_owned(),
            provider_id: "default".to_owned(),
        })
        .expect_err("daemon rejection propagates");
    assert_eq!(error.code(), "provider_health_unavailable");
    health_server.join().expect("fixture server completes");
}

#[test]
fn credential_shaped_commands_fail_closed_client_side_before_connecting() {
    let fixture_endpoint = endpoint();
    let _server = start_fixture_server(
        fixture_endpoint.clone(),
        ExpectedRequest::RotateProviderCredentials,
        FixtureReply::RotationResult(rotation_result()),
    );
    let error = fixture_client(fixture_endpoint)
        .rotate_credential(RotateProviderCredentialsCommandDto {
            profile_id: "default".to_owned(),
            provider_profile_revision_id: "profile-rev-1".to_owned(),
            expected_credential_composition_revision: FAKE_SECRET.to_owned(),
            operation_id: "operation-1".to_owned(),
        })
        .expect_err("credential-shaped command fails closed client-side");
    assert_eq!(error.code(), "credentials_forbidden");
    assert!(!error.to_string().contains(FAKE_SECRET));

    let edit_endpoint = endpoint();
    let _server = start_fixture_server(
        edit_endpoint.clone(),
        ExpectedRequest::SubmitRawTomlEdit,
        FixtureReply::ReloadTransaction(transaction(true)),
    );
    let error = fixture_client(edit_endpoint)
        .submit_raw_toml_edit(RawTomlEditCommandDto {
            operation_id: "operation-1".to_owned(),
            expected_config_revision: "revision-1".to_owned(),
            candidate_content: format!(
                "schema_version = 1\n[provider]\nmodel = \"{FAKE_SECRET}\"\n"
            ),
        })
        .expect_err("credential-shaped raw edit fails closed client-side");
    assert_eq!(error.code(), "credentials_forbidden");
    assert!(!error.to_string().contains(FAKE_SECRET));
}

#[test]
fn unsupported_peer_fails_closed_at_the_negotiation_gate() {
    assert_eq!(
        intention_protocol::negotiation::require_provider_profiles(&[
            intention_protocol::ProtocolCapabilityDto::SessionSubscriptions,
        ])
        .expect_err("missing provider profiles capability fails closed")
        .code(),
        "provider_profiles_capability_required"
    );
    assert!(
        intention_protocol::negotiation::require_provider_profiles(&[
            intention_protocol::ProtocolCapabilityDto::ProviderProfilesV1,
        ])
        .is_ok()
    );
    // The client handshake itself stays at the M3 capability baseline: the
    // control-plane commands are additive and the daemon gate enforces the
    // provider-profiles capability before any effect.
    let _ = ProtocolVersionDto::new(1, 1);
}
