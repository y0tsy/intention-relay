#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Session-selection client fixtures use direct assertions and controlled fixture servers."
)]

//! Slice 2 session-selection client contract tests.
//!
//! A local fixture daemon (bound before the client connects, matching the
//! existing client-contract pattern) asserts the exact protocol variant each
//! client method issues and replies with the matching typed payload. The
//! fake-secret sweep proves credential-shaped commands fail closed
//! client-side and that no captured request or decoded projection ever
//! carries a fake secret.

use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

use intention_client::{DaemonLauncher, IntentionClient};
use intention_protocol::contract_families::{
    AcceptProviderCatalogRemovalAcceptedDto, AcceptProviderCatalogRemovalCommandDto,
    AdmitRecoveredRunAcceptedDto, AdmitRecoveredRunCommandDto, CredentialTransportMode,
    GetProviderCatalogQueryDto, GetProviderCatalogStatusQueryDto, GetProviderUsageQueryDto,
    ProviderCatalogActivationState, ProviderCatalogEntryDto, ProviderCatalogPageDto,
    ProviderCatalogStatusDto, ProviderProfileUnavailableReason, ProviderReadinessDto,
    ReconcileUnavailableQueueAcceptedDto, ReconcileUnavailableQueueCommandDto,
    RejectProviderCatalogCandidateAcceptedDto, RejectProviderCatalogCandidateCommandDto,
    ResolvedProviderProfileDto, SessionProviderProfileDto, SetSessionProviderProfileAcceptedDto,
    SetSessionProviderProfileCommandDto, UsageAggregationDto,
};
use intention_protocol::{
    ProtocolAcceptedDto, ProtocolAcceptedResultDto, ProtocolCommandResultDto, ProtocolHelloDto,
    ProtocolMessageDto, ProtocolQueryResultDto, ProtocolRequestPayloadDto,
    ProtocolResponseEnvelopeDto, ProtocolResponsePayloadDto,
};
use intention_transport::{LocalEndpoint, LocalListener, local_protocol_version, negotiate_daemon};
use intention_types::{DtoResult, ErrorDto, SchemaVersionDto, SessionId};

const SCHEMA_VERSION: SchemaVersionDto = intention_protocol::CURRENT_DTO_SCHEMA_VERSION;
const FAKE_SECRET: &str = "sk-zone5-client-fake-secret";
const SCHEMA_TEXT: &str = "1.1";

/// The exact protocol request variant one fixture connection must receive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExpectedRequest {
    SetSessionProviderProfile,
    GetSessionProviderProfile,
    ListProviderProfiles,
    ProviderCatalogStatus,
    ReconcileUnavailableQueue,
    AcceptProviderCatalogRemoval,
    RejectProviderCatalogCandidate,
    AdmitRecoveredRun,
    ProviderUsage,
}

/// The canned daemon reply for one fixture connection.
#[derive(Clone, Debug)]
enum FixtureReply {
    SetSessionProviderProfile(SetSessionProviderProfileAcceptedDto),
    SessionProviderProfile(SessionProviderProfileDto),
    CatalogPage(ProviderCatalogPageDto),
    CatalogStatus(ProviderCatalogStatusDto),
    ReconcileUnavailableQueue(ReconcileUnavailableQueueAcceptedDto),
    AcceptProviderCatalogRemoval(AcceptProviderCatalogRemovalAcceptedDto),
    RejectProviderCatalogCandidate(RejectProviderCatalogCandidateAcceptedDto),
    AdmitRecoveredRun(AdmitRecoveredRunAcceptedDto),
    Usage(UsageAggregationDto),
    CommandRejected(ErrorDto),
    QueryRejected(ErrorDto),
}

/// A launcher that must never run: the fixture server is bound before the
/// client connects, so direct session-selection calls never bootstrap.
struct UnusedLauncher;

impl DaemonLauncher for UnusedLauncher {
    fn launch(&self, _endpoint: &LocalEndpoint) -> DtoResult<()> {
        Err(ErrorDto::unavailable(
            "fixture_launch_unexpected",
            "fixture launcher must not run for direct session-selection calls",
        ))
    }
}

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);

fn endpoint() -> LocalEndpoint {
    let sequence = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    LocalEndpoint::from_instance_id(format!(
        "client-session-selection-{}-{sequence}",
        std::process::id()
    ))
    .expect("fixture instance name is valid")
}

fn fixture_client(endpoint: LocalEndpoint) -> IntentionClient {
    IntentionClient::new(
        endpoint,
        "fixture-session-selection-client",
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
        "fixture-session-selection-daemon",
    )
    .expect("fixture daemon hello is valid")
}

/// Binds one fixture daemon that accepts one connection, asserts the exact
/// expected request variant, and replies with the canned reply.
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
            intention_protocol::ProtocolCommandDto::SetSessionProviderProfile(_),
        ) => ExpectedRequest::SetSessionProviderProfile,
        ProtocolRequestPayloadDto::Query(
            intention_protocol::ProtocolQueryDto::GetSessionProviderProfile(_),
        ) => ExpectedRequest::GetSessionProviderProfile,
        ProtocolRequestPayloadDto::Query(
            intention_protocol::ProtocolQueryDto::GetProviderCatalog(_),
        ) => ExpectedRequest::ListProviderProfiles,
        ProtocolRequestPayloadDto::Query(
            intention_protocol::ProtocolQueryDto::GetProviderCatalogStatus(_),
        ) => ExpectedRequest::ProviderCatalogStatus,
        ProtocolRequestPayloadDto::Command(
            intention_protocol::ProtocolCommandDto::ReconcileUnavailableQueue(_),
        ) => ExpectedRequest::ReconcileUnavailableQueue,
        ProtocolRequestPayloadDto::Command(
            intention_protocol::ProtocolCommandDto::AcceptProviderCatalogRemoval(_),
        ) => ExpectedRequest::AcceptProviderCatalogRemoval,
        ProtocolRequestPayloadDto::Command(
            intention_protocol::ProtocolCommandDto::RejectProviderCatalogCandidate(_),
        ) => ExpectedRequest::RejectProviderCatalogCandidate,
        ProtocolRequestPayloadDto::Command(
            intention_protocol::ProtocolCommandDto::AdmitRecoveredRun(_),
        ) => ExpectedRequest::AdmitRecoveredRun,
        ProtocolRequestPayloadDto::Query(
            intention_protocol::ProtocolQueryDto::GetProviderUsage(_),
        ) => ExpectedRequest::ProviderUsage,
        _ => panic!("fixture receives an unexpected session-selection request"),
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
        FixtureReply::SetSessionProviderProfile(accepted) => {
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(
                ProtocolAcceptedDto::with_result(
                    request.correlation_id(),
                    ProtocolAcceptedResultDto::SetSessionProviderProfile(accepted),
                ),
            ))
        }
        FixtureReply::ReconcileUnavailableQueue(accepted) => {
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(
                ProtocolAcceptedDto::with_result(
                    request.correlation_id(),
                    ProtocolAcceptedResultDto::ReconcileUnavailableQueue(accepted),
                ),
            ))
        }
        FixtureReply::AcceptProviderCatalogRemoval(accepted) => {
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(
                ProtocolAcceptedDto::with_result(
                    request.correlation_id(),
                    ProtocolAcceptedResultDto::AcceptProviderCatalogRemoval(accepted),
                ),
            ))
        }
        FixtureReply::RejectProviderCatalogCandidate(accepted) => {
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(
                ProtocolAcceptedDto::with_result(
                    request.correlation_id(),
                    ProtocolAcceptedResultDto::RejectProviderCatalogCandidate(accepted),
                ),
            ))
        }
        FixtureReply::AdmitRecoveredRun(accepted) => ProtocolResponsePayloadDto::CommandResult(
            ProtocolCommandResultDto::Accepted(ProtocolAcceptedDto::with_result(
                request.correlation_id(),
                ProtocolAcceptedResultDto::AdmitRecoveredRun(accepted),
            )),
        ),
        FixtureReply::CommandRejected(error) => {
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Rejected(error))
        }
        FixtureReply::SessionProviderProfile(projection) => {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::SessionProviderProfile(
                projection,
            ))
        }
        FixtureReply::CatalogPage(page) => {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::ProviderCatalog(page))
        }
        FixtureReply::CatalogStatus(status) => ProtocolResponsePayloadDto::QueryResult(
            ProtocolQueryResultDto::ProviderCatalogStatus(status),
        ),
        FixtureReply::Usage(usage) => {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::ProviderUsage(usage))
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

fn resolved_profile() -> ResolvedProviderProfileDto {
    ResolvedProviderProfileDto::Resolved {
        profile_id: "default".to_owned(),
        profile_revision_id: "rev-0123456789abcdef".to_owned(),
    }
}

fn set_accepted() -> SetSessionProviderProfileAcceptedDto {
    SetSessionProviderProfileAcceptedDto {
        session_id: "session-1".to_owned(),
        changed: true,
        resulting_projection_revision: 0,
        resolved: resolved_profile(),
    }
}

fn session_profile() -> SessionProviderProfileDto {
    SessionProviderProfileDto {
        session_id: "session-1".to_owned(),
        profile_id: "default".to_owned(),
        resolved: resolved_profile(),
        session_projection_revision: 0,
        global_default_profile_id: "default".to_owned(),
    }
}

fn catalog_page() -> ProviderCatalogPageDto {
    ProviderCatalogPageDto {
        schema_version: SCHEMA_TEXT.to_owned(),
        catalog_revision_id: "1".to_owned(),
        entries: vec![ProviderCatalogEntryDto {
            profile_id: "default".to_owned(),
            profile_revision_id: "rev-0123456789abcdef".to_owned(),
            display_name: "default".to_owned(),
            enabled: true,
            provider_kind_id: "openrouter".to_owned(),
            kind_descriptor_revision_id: "kind-rev-1".to_owned(),
            model_id: "fixture-model".to_owned(),
            normalized_endpoint: Some("https://api.example.invalid/v1".to_owned()),
            effective_execution_policy: "execution-timeout-60-attempts-3".to_owned(),
            capability_subset: vec!["text_input".to_owned(), "text_streaming".to_owned()],
            credential_transport_mode: CredentialTransportMode::Bearer,
            credential_transport_safe_header_name: None,
            credential_configured: false,
            driver_declared_capabilities: Vec::new(),
            readiness: ProviderReadinessDto::Ready,
        }],
        next_page_token: None,
        has_more: false,
    }
}

fn catalog_status() -> ProviderCatalogStatusDto {
    ProviderCatalogStatusDto {
        schema_version: SCHEMA_TEXT.to_owned(),
        activation_state: ProviderCatalogActivationState::Active,
        degraded_reason: None,
        active_catalog_revision_id: Some("1".to_owned()),
        candidate_catalog_revision_id: None,
        active_default_profile_id: Some("default".to_owned()),
        removal_impact: None,
        provider_profiles_negotiated: true,
    }
}

fn reconcile_accepted() -> ReconcileUnavailableQueueAcceptedDto {
    ReconcileUnavailableQueueAcceptedDto {
        session_id: "session-1".to_owned(),
        page_cursor: None,
        promoted_count: 2,
    }
}

fn removal_accepted() -> AcceptProviderCatalogRemovalAcceptedDto {
    AcceptProviderCatalogRemovalAcceptedDto {
        candidate_handle: "catalog-2".to_owned(),
        active_catalog_revision_id: "2".to_owned(),
    }
}

fn rejection_accepted() -> RejectProviderCatalogCandidateAcceptedDto {
    RejectProviderCatalogCandidateAcceptedDto {
        candidate_handle: "catalog-2".to_owned(),
    }
}

fn admission_accepted() -> AdmitRecoveredRunAcceptedDto {
    AdmitRecoveredRunAcceptedDto {
        session_id: "session-1".to_owned(),
        run_id: "run-1".to_owned(),
    }
}

fn usage() -> UsageAggregationDto {
    UsageAggregationDto {
        profile_id: "default".to_owned(),
        provider_profile_revision_id: "rev-0123456789abcdef".to_owned(),
        model_id: "fixture-model".to_owned(),
        request_count: 3,
        input_units: 30,
        output_units: 15,
        reasoning_units: 0,
        usage_period_start: 0,
        usage_period_end: 100,
    }
}

/// Runs one client method against a fixture daemon bound on the same endpoint
/// the client uses, asserting the exact request variant and returning the
/// canned reply.
fn run_fixture(
    expected: ExpectedRequest,
    reply: FixtureReply,
    invoke: impl FnOnce(&IntentionClient),
) {
    let fixture_endpoint = endpoint();
    let server = start_fixture_server(fixture_endpoint.clone(), expected, reply);
    let client = fixture_client(fixture_endpoint);
    invoke(&client);
    server.join().expect("fixture server completes");
}

#[test]
fn set_session_provider_profile_issues_the_exact_command_variant() {
    run_fixture(
        ExpectedRequest::SetSessionProviderProfile,
        FixtureReply::SetSessionProviderProfile(set_accepted()),
        |client| {
            let result = client
                .set_session_provider_profile(SetSessionProviderProfileCommandDto {
                    schema_version: SCHEMA_TEXT.to_owned(),
                    session_id: "session-1".to_owned(),
                    profile_id: "default".to_owned(),
                    expected_session_projection_revision: 0,
                    operation_id: "op-1".to_owned(),
                })
                .expect("client set decodes the typed acceptance");
            assert!(result.changed);
            assert!(matches!(
                result.resolved,
                ResolvedProviderProfileDto::Resolved { .. }
            ));
        },
    );
}

#[test]
fn session_provider_profile_query_issues_the_exact_variant() {
    run_fixture(
        ExpectedRequest::GetSessionProviderProfile,
        FixtureReply::SessionProviderProfile(session_profile()),
        |client| {
            let result = client
                .session_provider_profile(
                    SessionId::parse("8e4f0f0e-9a3b-4c2d-8f1a-2b3c4d5e6f70")
                        .expect("fixture session id"),
                )
                .expect("client query decodes the typed projection");
            assert_eq!(result.profile_id, "default");
            assert_eq!(result.global_default_profile_id, "default");
        },
    );
}

#[test]
fn list_provider_profiles_issues_the_exact_query_variant() {
    run_fixture(
        ExpectedRequest::ListProviderProfiles,
        FixtureReply::CatalogPage(catalog_page()),
        |client| {
            let result = client
                .list_provider_profiles(GetProviderCatalogQueryDto {
                    schema_version: SCHEMA_TEXT.to_owned(),
                    page_token: None,
                    expected_catalog_revision_id: None,
                })
                .expect("client query decodes the catalog page");
            assert_eq!(result.entries.len(), 1);
            assert_eq!(result.entries[0].profile_id, "default");
            assert!(!result.has_more);
        },
    );
}

#[test]
fn provider_catalog_status_issues_the_exact_query_variant() {
    run_fixture(
        ExpectedRequest::ProviderCatalogStatus,
        FixtureReply::CatalogStatus(catalog_status()),
        |client| {
            let result = client
                .provider_catalog_status(GetProviderCatalogStatusQueryDto {
                    schema_version: SCHEMA_TEXT.to_owned(),
                })
                .expect("client query decodes the catalog status");
            assert_eq!(
                result.activation_state,
                ProviderCatalogActivationState::Active
            );
            assert_eq!(result.active_default_profile_id.as_deref(), Some("default"));
        },
    );
}

#[test]
fn reconcile_unavailable_queue_issues_the_exact_command_variant() {
    run_fixture(
        ExpectedRequest::ReconcileUnavailableQueue,
        FixtureReply::ReconcileUnavailableQueue(reconcile_accepted()),
        |client| {
            let result = client
                .reconcile_unavailable_queue(ReconcileUnavailableQueueCommandDto {
                    session_id: "session-1".to_owned(),
                    operation_id: "op-reconcile".to_owned(),
                    page_cursor: None,
                })
                .expect("client command decodes the typed acceptance");
            assert_eq!(result.promoted_count, 2);
        },
    );
}

#[test]
fn accept_provider_catalog_removal_issues_the_exact_command_variant() {
    run_fixture(
        ExpectedRequest::AcceptProviderCatalogRemoval,
        FixtureReply::AcceptProviderCatalogRemoval(removal_accepted()),
        |client| {
            let result = client
                .accept_provider_catalog_removal(AcceptProviderCatalogRemovalCommandDto {
                    candidate_handle: "catalog-2".to_owned(),
                    expected_active_catalog_revision_id: "1".to_owned(),
                    expected_candidate_catalog_revision_id: "2".to_owned(),
                    operation_id: "op-accept".to_owned(),
                    source_recheck: false,
                })
                .expect("client command decodes the typed acceptance");
            assert_eq!(result.active_catalog_revision_id, "2");
        },
    );
}

#[test]
fn reject_provider_catalog_candidate_issues_the_exact_command_variant() {
    run_fixture(
        ExpectedRequest::RejectProviderCatalogCandidate,
        FixtureReply::RejectProviderCatalogCandidate(rejection_accepted()),
        |client| {
            let result = client
                .reject_provider_catalog_candidate(RejectProviderCatalogCandidateCommandDto {
                    candidate_handle: "catalog-2".to_owned(),
                    expected_active_catalog_revision_id: "1".to_owned(),
                    operation_id: "op-reject".to_owned(),
                })
                .expect("client command decodes the typed acceptance");
            assert_eq!(result.candidate_handle, "catalog-2");
        },
    );
}

#[test]
fn admit_recovered_run_issues_the_exact_command_variant() {
    run_fixture(
        ExpectedRequest::AdmitRecoveredRun,
        FixtureReply::AdmitRecoveredRun(admission_accepted()),
        |client| {
            let result = client
                .admit_recovered_run(AdmitRecoveredRunCommandDto {
                    session_id: "session-1".to_owned(),
                    run_id: "run-1".to_owned(),
                    operation_id: "op-admit".to_owned(),
                })
                .expect("client command decodes the typed acceptance");
            assert_eq!(result.run_id, "run-1");
        },
    );
}

#[test]
fn provider_usage_issues_the_exact_query_variant() {
    run_fixture(
        ExpectedRequest::ProviderUsage,
        FixtureReply::Usage(usage()),
        |client| {
            let result = client
                .provider_usage(GetProviderUsageQueryDto {
                    schema_version: SCHEMA_TEXT.to_owned(),
                    profile_id: "default".to_owned(),
                    usage_period_start: 0,
                    usage_period_end: 100,
                })
                .expect("client query decodes the usage aggregation");
            assert_eq!(result.request_count, 3);
            assert_eq!(result.input_units, 30);
        },
    );
}

#[test]
fn rejected_commands_and_queries_propagate_the_typed_error() {
    let rejection = ErrorDto::validation("execution_not_ready", "degraded read-only");
    run_fixture(
        ExpectedRequest::SetSessionProviderProfile,
        FixtureReply::CommandRejected(rejection),
        |client| {
            let error = client
                .set_session_provider_profile(SetSessionProviderProfileCommandDto {
                    schema_version: SCHEMA_TEXT.to_owned(),
                    session_id: "session-1".to_owned(),
                    profile_id: "default".to_owned(),
                    expected_session_projection_revision: 0,
                    operation_id: "op-rejected".to_owned(),
                })
                .expect_err("rejected command propagates the typed error");
            assert_eq!(error.code(), "execution_not_ready");
        },
    );

    let query_rejection = ErrorDto::validation("catalog_page_token_stale", "catalog changed");
    run_fixture(
        ExpectedRequest::ListProviderProfiles,
        FixtureReply::QueryRejected(query_rejection),
        |client| {
            let error = client
                .list_provider_profiles(GetProviderCatalogQueryDto {
                    schema_version: SCHEMA_TEXT.to_owned(),
                    page_token: None,
                    expected_catalog_revision_id: None,
                })
                .expect_err("rejected query propagates the typed error");
            assert_eq!(error.code(), "catalog_page_token_stale");
        },
    );
}

#[test]
fn fake_secret_commands_fail_closed_client_side() {
    let fixture_endpoint = endpoint();
    let client = fixture_client(fixture_endpoint);
    let error = client
        .set_session_provider_profile(SetSessionProviderProfileCommandDto {
            schema_version: SCHEMA_TEXT.to_owned(),
            session_id: "session-1".to_owned(),
            profile_id: FAKE_SECRET.to_owned(),
            expected_session_projection_revision: 0,
            operation_id: "op-secret".to_owned(),
        })
        .expect_err("credential-shaped profile id fails closed client-side");
    assert_eq!(error.code(), "credentials_forbidden");
    assert!(!error.to_string().contains(FAKE_SECRET));

    let error = client
        .admit_recovered_run(AdmitRecoveredRunCommandDto {
            session_id: "session-1".to_owned(),
            run_id: "run-1".to_owned(),
            operation_id: FAKE_SECRET.to_owned(),
        })
        .expect_err("credential-shaped operation id fails closed client-side");
    assert_eq!(error.code(), "credentials_forbidden");
    assert!(!error.to_string().contains(FAKE_SECRET));

    let error = client
        .provider_usage(GetProviderUsageQueryDto {
            schema_version: SCHEMA_TEXT.to_owned(),
            profile_id: FAKE_SECRET.to_owned(),
            usage_period_start: 0,
            usage_period_end: 100,
        })
        .expect_err("credential-shaped profile id fails closed client-side");
    assert_eq!(error.code(), "credentials_forbidden");
    assert!(!error.to_string().contains(FAKE_SECRET));
}

#[test]
fn safe_projections_decode_without_credentials() {
    // Every fixture projection validates as credential-free at decode time.
    set_accepted()
        .validate()
        .expect("accepted set projection is valid");
    session_profile()
        .validate()
        .expect("session profile projection is valid");
    catalog_page()
        .validate()
        .expect("catalog page projection is valid");
    catalog_status()
        .validate()
        .expect("catalog status projection is valid");
    reconcile_accepted()
        .validate()
        .expect("reconciliation acceptance is valid");
    removal_accepted()
        .validate()
        .expect("removal acceptance is valid");
    rejection_accepted()
        .validate()
        .expect("rejection acceptance is valid");
    admission_accepted()
        .validate()
        .expect("admission acceptance is valid");
    usage().validate().expect("usage aggregation is valid");
    assert_eq!(
        ProviderProfileUnavailableReason::ProfileDisabled,
        ProviderProfileUnavailableReason::ProfileDisabled
    );
}
