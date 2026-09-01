//! Schema-4 control-plane repository contracts for the provider catalog,
//! unavailable-provider queue, provider usage, and provider catalog removal
//! traits. Every repository method, its conflict and validation branches, the
//! typed JSON codecs, and the credential-free storage property are exercised
//! here; the defaults, resolved-selection, held-run, and reload traits live in
//! `sqlite_contracts.rs`.

#![allow(
    clippy::expect_used,
    reason = "SQLite contract fixtures use expect for precise test diagnostics."
)]

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    ContextPreservationCapability, CreateSessionCommandDto, CredentialTransportMode,
    ModelCapabilitySetV1, ModelInputCapability, ProviderDriverContractRevisionDto,
    ProviderKindDescriptorRevisionV1, ProviderProfileRevisionV1, ReasoningCapability, RunModeDto,
    RunStatusDto, StructuredOutputCapability, WorkspaceRootDto,
    canonical::contains_credential_shape, provider_selection::MODEL_CAPABILITY_TAXONOMY_V1,
};
use intention_storage::{
    AcceptProviderCatalogInputDto, AcceptProviderCatalogRemovalInputDto, AcceptUserTurnInputDto,
    AppendProviderKindDescriptorRevisionInputDto, AppendProviderProfileRevisionInputDto,
    CreateProviderCatalogRemovalCandidateInputDto, CreateSessionInputDto,
    EnqueueUnavailableRunInputDto, ExpireProviderCatalogCandidateInputDto,
    ExpireProviderCatalogRemovalCandidateInputDto, LoadProviderCatalogPageInputDto,
    LoadUnavailableQueuePageInputDto, PromoteUnavailableRunsInputDto, ProviderCatalogRepositoryDto,
    ProviderCatalogStatusDto, ProviderKindDescriptorCandidateDto, ProviderProfileCandidateDto,
    ProviderReadinessDto, ProviderRemovalRepositoryDto, ProviderUsageEventInputDto,
    ProviderUsageRepositoryDto, ReconcileUnavailableQueueInputDto, RecordProviderUsageInputDto,
    RejectProviderCatalogCandidateInputDto, RejectProviderCatalogRemovalInputDto,
    StorageRepositoryDto, TransitionRunInputDto, UnavailableQueueRepositoryDto,
    UnavailableQueueStateDto,
};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use intention_types::{ProjectId, RunId, SessionId, TimestampDto, TurnId, WorkspaceId};
use tempfile::TempDir;

fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture timestamp is valid")
}

fn snapshot() -> ConfigSnapshotDto {
    serde_json::from_str(include_str!(
        "../../intention-config/tests/fixtures/config-snapshot-v1.json"
    ))
    .expect("safe configuration snapshot decodes")
}

fn workspace_root(label: &str) -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-storage-sqlite-repos")
            .join(label)
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

fn repository() -> (TempDir, SqliteStorageRepository) {
    let directory = TempDir::new().expect("temporary directory exists");
    let location = directory
        .path()
        .join("storage.sqlite")
        .to_string_lossy()
        .into_owned();
    let store = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(location).expect("temp location is absolute"),
    )
    .expect("database opens");
    (directory, store)
}

fn create(store: &SqliteStorageRepository) -> SessionId {
    let session = SessionId::new();
    store
        .create_session(CreateSessionInputDto::new(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session,
                WorkspaceId::new(),
                workspace_root(&format!("control-plane-repos-{}", session)),
                RunModeDto::Build,
            ),
            time(1),
        ))
        .expect("session creates");
    session
}

fn accept(store: &SqliteStorageRepository, session: SessionId, run: RunId, text: &str) {
    store
        .accept_user_turn(
            AcceptUserTurnInputDto::new(session, TurnId::new(), text, run, snapshot(), time(2))
                .expect("turn input is valid"),
        )
        .expect("turn commits");
}

/// Accepts one turn and drives its run to a terminal state, returning the run.
fn accept_terminal_run(store: &SqliteStorageRepository, session: SessionId) -> RunId {
    let run = RunId::new();
    accept(store, session, run, "queued run");
    store
        .transition_run(TransitionRunInputDto::new(
            session,
            run,
            RunStatusDto::Failed,
            time(5),
        ))
        .expect("run reaches terminal state");
    run
}

fn fixture_capability_envelope() -> ModelCapabilitySetV1 {
    ModelCapabilitySetV1 {
        taxonomy_version: MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
        input: ModelInputCapability::TextOnly,
        text_streaming: true,
        structured_output: StructuredOutputCapability::Unsupported,
        reasoning: ReasoningCapability::TextualReasoningV1,
        tool_exchange: false,
        context_preservation: ContextPreservationCapability::LocalDurableHistoryV1 {
            reasoning_input_contract: "reasoning-history-transfer-v1".to_owned(),
        },
    }
}

fn fixture_kind_descriptor(kind_id: &str) -> ProviderKindDescriptorRevisionV1 {
    ProviderKindDescriptorRevisionV1 {
        kind_id: kind_id.to_owned(),
        descriptor_family: "responses-descriptor".to_owned(),
        ordered_protocol_part_revisions: vec!["parts-v1".to_owned()],
        endpoint_policy: "https-only".to_owned(),
        credential_transport_contract: "bearer-or-safe-header".to_owned(),
        model_capability_envelope: fixture_capability_envelope(),
        driver_contract_family: "responses".to_owned(),
    }
}

fn fixture_profile(profile_id: &str, revision_id: &str) -> ProviderProfileRevisionV1 {
    ProviderProfileRevisionV1 {
        profile_id: profile_id.to_owned(),
        revision_id: revision_id.to_owned(),
        provider_kind_id: "responses".to_owned(),
        model_id: "gpt-4.1".to_owned(),
        endpoint: "https://api.example.com/v1".to_owned(),
        credential_transport_mode: CredentialTransportMode::Bearer,
        safe_header_name: None,
        capability_taxonomy_revision: MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
        reasoning_compatibility_id: Some("reasoning-compat-v1".to_owned()),
        kind_descriptor_revision_id: "kd-1".to_owned(),
        driver_contract_revision: ProviderDriverContractRevisionDto {
            driver_family: "responses".to_owned(),
            major: 1,
            minor: 0,
        },
    }
}

fn fixture_profile_candidate(profile_id: &str, revision_id: &str) -> ProviderProfileCandidateDto {
    fixture_profile_candidate_with(
        profile_id,
        revision_id,
        CredentialTransportMode::Bearer,
        None,
        ProviderReadinessDto::Ready,
        "kd-1",
    )
}

fn fixture_profile_candidate_with(
    profile_id: &str,
    revision_id: &str,
    transport: CredentialTransportMode,
    safe_header_name: Option<&str>,
    readiness: ProviderReadinessDto,
    kind_descriptor_revision_id: &str,
) -> ProviderProfileCandidateDto {
    let mut profile = fixture_profile(profile_id, revision_id);
    profile.credential_transport_mode = transport;
    profile.safe_header_name = safe_header_name.map(str::to_owned);
    profile.kind_descriptor_revision_id = kind_descriptor_revision_id.to_owned();
    ProviderProfileCandidateDto {
        profile,
        declared_model_capability_subset: vec![
            "text_input".to_owned(),
            "text_streaming".to_owned(),
        ],
        resolved_reasoning_policy: "textual-reasoning-v1".to_owned(),
        effective_execution_policy: "ordinary".to_owned(),
        effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
        display_name: Some(profile_id.to_owned()),
        enabled: true,
        credential_configured: true,
        readiness,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Test fixtures pass flat candidate identity values for precise diagnostics."
)]
fn prepare_candidate(
    store: &SqliteStorageRepository,
    revision: u64,
    operation_id: &str,
    kind_id: &str,
    descriptor_revision_id: &str,
    profile_id: &str,
    profile_revision_id: &str,
    accepted_at: i64,
) {
    store
        .append_provider_kind_descriptor_revision(AppendProviderKindDescriptorRevisionInputDto {
            descriptor_revision_id: descriptor_revision_id.to_owned(),
            descriptor: fixture_kind_descriptor(kind_id),
            catalog_revision_id: revision,
            accepted_at,
            operation_id: operation_id.to_owned(),
        })
        .expect("fixture kind descriptor prepares");
    store
        .append_provider_profile_revision(AppendProviderProfileRevisionInputDto {
            profile: fixture_profile_candidate(profile_id, profile_revision_id),
            catalog_revision_id: revision,
            accepted_at,
            operation_id: operation_id.to_owned(),
        })
        .expect("fixture profile prepares");
}

#[expect(
    clippy::too_many_arguments,
    reason = "Test fixtures pass flat candidate identity values for precise diagnostics."
)]
fn accept_candidate(
    store: &SqliteStorageRepository,
    revision: u64,
    handle: &str,
    operation_id: &str,
    kind_id: &str,
    descriptor_revision_id: &str,
    profile_id: &str,
    profile_revision_id: &str,
    accepted_at: i64,
) {
    accept_catalog_with(
        store,
        revision,
        handle,
        operation_id,
        vec![ProviderKindDescriptorCandidateDto {
            descriptor_revision_id: descriptor_revision_id.to_owned(),
            descriptor: fixture_kind_descriptor(kind_id),
        }],
        vec![fixture_profile_candidate(profile_id, profile_revision_id)],
        profile_id,
        accepted_at,
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "Test fixtures pass flat candidate identity values for precise diagnostics."
)]
fn accept_catalog_with(
    store: &SqliteStorageRepository,
    revision: u64,
    handle: &str,
    operation_id: &str,
    kinds: Vec<ProviderKindDescriptorCandidateDto>,
    profiles: Vec<ProviderProfileCandidateDto>,
    default_profile_id: &str,
    accepted_at: i64,
) {
    store
        .accept_provider_catalog(AcceptProviderCatalogInputDto {
            catalog_revision_id: revision,
            candidate_handle: handle.to_owned(),
            kind_descriptors: kinds,
            profiles,
            default_profile_id: default_profile_id.to_owned(),
            accepted_at,
            operation_id: operation_id.to_owned(),
        })
        .expect("fixture catalog accepts");
}

fn enqueue(
    store: &SqliteStorageRepository,
    session: SessionId,
    run: RunId,
    reason: &str,
    operation_id: &str,
    at: i64,
) {
    store
        .enqueue_unavailable_run(EnqueueUnavailableRunInputDto {
            run_id: run,
            session_id: session,
            profile_id: "profile-a".to_owned(),
            provider_profile_revision_id: "rev-a".to_owned(),
            unavailable_reason: reason.to_owned(),
            first_unavailable_at: at,
            operation_id: operation_id.to_owned(),
            selection_json: "{\"safe\":true}".to_owned(),
        })
        .expect("unavailable run enqueues");
}

fn db_path(directory: &TempDir) -> std::path::PathBuf {
    directory.path().join("storage.sqlite")
}

fn raw_connection(directory: &TempDir) -> sqlite::Connection {
    let connection =
        sqlite::Connection::open(db_path(directory)).expect("database reopens for inspection");
    // Raw inspection connections intentionally bypass foreign keys so that
    // corrupted-identity rows can be written for typed decode tests.
    connection
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("foreign keys disable for raw inspection");
    connection
}

fn query_strings(connection: &sqlite::Connection, sql: &str) -> Vec<String> {
    let mut statement = connection.prepare(sql).expect("inspection query prepares");
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("inspection query runs");
    rows.map(|row| row.expect("inspection row reads")).collect()
}

fn query_count(connection: &sqlite::Connection, sql: &str) -> i64 {
    connection
        .query_row(sql, [], |row| row.get(0))
        .expect("inspection count reads")
}

// ---------------------------------------------------------------------------
// Provider catalog: append, status, material, page, accept, reject, expire.
// ---------------------------------------------------------------------------

#[test]
fn catalog_prepare_accept_and_material_round_trip_durably() {
    let (_directory, store) = repository();
    let preparing = store
        .load_provider_catalog_status()
        .expect("initial catalog status loads");
    assert_eq!(preparing.status, ProviderCatalogStatusDto::Preparing);
    assert_eq!(preparing.active_catalog_revision_id, None);
    assert_eq!(preparing.candidate_catalog_revision_id, None);
    assert_eq!(preparing.updated_at, 0);
    prepare_candidate(
        &store,
        1,
        "op-prep-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    let prepared = store
        .load_provider_catalog_status()
        .expect("prepared catalog status loads");
    assert_eq!(prepared.status, ProviderCatalogStatusDto::Preparing);
    assert_eq!(prepared.candidate_catalog_revision_id, Some(1));
    assert_eq!(prepared.active_catalog_revision_id, None);
    accept_candidate(
        &store,
        1,
        "candidate-1",
        "op-accept-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    let active = store
        .load_provider_catalog_status()
        .expect("active catalog status loads");
    assert_eq!(active.status, ProviderCatalogStatusDto::Active);
    assert_eq!(active.active_catalog_revision_id, Some(1));
    assert_eq!(active.candidate_catalog_revision_id, None);
    assert_eq!(active.candidate_handle, None);
    assert_eq!(
        active.active_default_profile_id.as_deref(),
        Some("profile-a")
    );
    assert_eq!(active.updated_at, 1);
    // The full material reconstructs the accepted kind and profile records,
    // including the typed JSON codec round trip of the capability envelope.
    let material = store
        .load_provider_catalog_material()
        .expect("catalog material loads");
    assert_eq!(material.catalog_revision_id, 1);
    assert_eq!(material.default_profile_id.as_deref(), Some("profile-a"));
    assert_eq!(material.kind_descriptors.len(), 1);
    assert_eq!(material.kind_descriptors[0].descriptor_revision_id, "kd-1");
    assert_eq!(
        material.kind_descriptors[0].descriptor,
        fixture_kind_descriptor("kind-a")
    );
    assert_eq!(material.profiles.len(), 1);
    assert_eq!(
        material.profiles[0].profile,
        fixture_profile("profile-a", "rev-a")
    );
    assert_eq!(
        material.profiles[0].declared_model_capability_subset,
        vec!["text_input".to_owned(), "text_streaming".to_owned()]
    );
    assert_eq!(
        material.profiles[0].resolved_reasoning_policy,
        "textual-reasoning-v1"
    );
    assert_eq!(
        material.profiles[0].display_name.as_deref(),
        Some("profile-a")
    );
    assert!(material.profiles[0].enabled);
    assert!(material.profiles[0].credential_configured);
    assert_eq!(material.profiles[0].readiness, ProviderReadinessDto::Ready);
    // The acceptance audit is ordered: accepted then activated, per operation.
    let connection = raw_connection(&_directory);
    let audits = query_strings(
        &connection,
        "SELECT audit_kind FROM configuration_audit WHERE operation_id='op-accept-1' ORDER BY audit_sequence",
    );
    assert_eq!(
        audits,
        vec![
            "ProviderCatalogAccepted".to_owned(),
            "ProviderCatalogActivated".to_owned()
        ]
    );
    let prepared_audits = query_count(
        &connection,
        "SELECT COUNT(*) FROM configuration_audit WHERE operation_id='op-prep-1' AND audit_kind='ProviderCatalogCandidatePrepared'",
    );
    assert_eq!(prepared_audits, 1);
}

#[test]
fn catalog_append_conflicts_on_digest_and_identity_reuse() {
    let (_directory, store) = repository();
    store
        .append_provider_kind_descriptor_revision(AppendProviderKindDescriptorRevisionInputDto {
            descriptor_revision_id: "kd-1".to_owned(),
            descriptor: fixture_kind_descriptor("kind-a"),
            catalog_revision_id: 1,
            accepted_at: 1,
            operation_id: "op-append-1".to_owned(),
        })
        .expect("kind descriptor appends");
    // The identical append is idempotent.
    store
        .append_provider_kind_descriptor_revision(AppendProviderKindDescriptorRevisionInputDto {
            descriptor_revision_id: "kd-1".to_owned(),
            descriptor: fixture_kind_descriptor("kind-a"),
            catalog_revision_id: 1,
            accepted_at: 2,
            operation_id: "op-append-1b".to_owned(),
        })
        .expect("identical kind descriptor append is idempotent");
    // The same descriptor bytes under a different revision identity conflict
    // on the digest.
    let digest_conflict = store
        .append_provider_kind_descriptor_revision(AppendProviderKindDescriptorRevisionInputDto {
            descriptor_revision_id: "kd-2".to_owned(),
            descriptor: fixture_kind_descriptor("kind-a"),
            catalog_revision_id: 1,
            accepted_at: 3,
            operation_id: "op-append-2".to_owned(),
        })
        .expect_err("kind digest reuse conflicts");
    assert_eq!(
        digest_conflict.code(),
        "provider_kind_descriptor_digest_conflict"
    );
    // Different bytes under the same (kind id, revision id) identity conflict
    // on the identity.
    let mut changed_kind = fixture_kind_descriptor("kind-a");
    changed_kind.endpoint_policy = "https-only-strict".to_owned();
    let identity_conflict = store
        .append_provider_kind_descriptor_revision(AppendProviderKindDescriptorRevisionInputDto {
            descriptor_revision_id: "kd-1".to_owned(),
            descriptor: changed_kind,
            catalog_revision_id: 1,
            accepted_at: 4,
            operation_id: "op-append-3".to_owned(),
        })
        .expect_err("kind identity reuse conflicts");
    assert_eq!(
        identity_conflict.code(),
        "provider_kind_descriptor_revision_conflict"
    );

    store
        .append_provider_profile_revision(AppendProviderProfileRevisionInputDto {
            profile: fixture_profile_candidate("profile-a", "rev-a"),
            catalog_revision_id: 1,
            accepted_at: 5,
            operation_id: "op-append-4".to_owned(),
        })
        .expect("profile appends");
    // The identical profile append is idempotent.
    store
        .append_provider_profile_revision(AppendProviderProfileRevisionInputDto {
            profile: fixture_profile_candidate("profile-a", "rev-a"),
            catalog_revision_id: 1,
            accepted_at: 6,
            operation_id: "op-append-4b".to_owned(),
        })
        .expect("identical profile append is idempotent");
    // Different bytes under the same profile identity conflict on identity.
    let mut changed_profile = fixture_profile_candidate("profile-a", "rev-a");
    changed_profile.profile.endpoint = "https://other.example.com/v1".to_owned();
    let profile_conflict = store
        .append_provider_profile_revision(AppendProviderProfileRevisionInputDto {
            profile: changed_profile,
            catalog_revision_id: 1,
            accepted_at: 7,
            operation_id: "op-append-5".to_owned(),
        })
        .expect_err("profile identity reuse conflicts");
    assert_eq!(
        profile_conflict.code(),
        "provider_profile_revision_conflict"
    );
}

#[test]
fn catalog_page_paginates_sorted_with_token_round_trip() {
    let (_directory, store) = repository();
    prepare_candidate(
        &store,
        1,
        "op-prep-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    accept_catalog_with(
        &store,
        1,
        "candidate-1",
        "op-accept-1",
        vec![ProviderKindDescriptorCandidateDto {
            descriptor_revision_id: "kd-1".to_owned(),
            descriptor: fixture_kind_descriptor("kind-a"),
        }],
        vec![
            fixture_profile_candidate("profile-a", "rev-a"),
            fixture_profile_candidate("profile-b", "rev-b"),
            fixture_profile_candidate("profile-c", "rev-c"),
        ],
        "profile-a",
        1,
    );
    // Limits outside the supported range are rejected before any read.
    for limit in [0_u64, 1025_u64] {
        let error = store
            .load_provider_catalog_page(LoadProviderCatalogPageInputDto { token: None, limit })
            .expect_err("invalid catalog page limit is rejected");
        assert_eq!(error.code(), "invalid_catalog_page_limit");
    }
    // The first page returns the two leading profiles sorted by profile id.
    let first = store
        .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
            token: None,
            limit: 2,
        })
        .expect("first catalog page loads");
    assert!(first.has_more);
    assert!(first.next_token.is_some());
    assert_eq!(
        first
            .entries
            .iter()
            .map(|entry| entry.profile_id.as_str())
            .collect::<Vec<_>>(),
        vec!["profile-a", "profile-b"]
    );
    assert_eq!(first.entries[0].profile_revision_id, "rev-a");
    assert_eq!(first.entries[0].kind_id, "responses");
    assert_eq!(first.entries[0].kind_descriptor_revision_id, "kd-1");
    assert_eq!(first.entries[0].display_name.as_deref(), Some("profile-a"));
    assert!(first.entries[0].enabled);
    assert!(first.entries[0].credential_configured);
    assert_eq!(first.entries[0].readiness, ProviderReadinessDto::Ready);
    assert!(!contains_credential_shape(
        &first.entries[0].safe_projection_json
    ));
    // The token resumes exactly after the last seen profile.
    let second = store
        .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
            token: first.next_token,
            limit: 2,
        })
        .expect("second catalog page loads");
    assert!(!second.has_more);
    assert!(second.next_token.is_none());
    assert_eq!(second.entries.len(), 1);
    assert_eq!(second.entries[0].profile_id, "profile-c");
    // A token past the end returns an empty terminal page.
    let third = store
        .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
            token: Some("{\"after\": \"profile-c\", \"revision\": 1}".to_owned()),
            limit: 2,
        })
        .expect("terminal catalog page loads");
    assert!(third.entries.is_empty());
    assert!(!third.has_more);
    assert!(third.next_token.is_none());
    // A single unbounded page returns everything without a token.
    let all = store
        .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
            token: None,
            limit: 10,
        })
        .expect("full catalog page loads");
    assert_eq!(all.entries.len(), 3);
    assert!(!all.has_more);
    assert!(all.next_token.is_none());
    // The material dedups the shared kind descriptor across the profiles.
    let material = store
        .load_provider_catalog_material()
        .expect("catalog material loads");
    assert_eq!(material.kind_descriptors.len(), 1);
    assert_eq!(material.profiles.len(), 3);
}

#[test]
fn catalog_page_tokens_reject_stale_and_malformed_tokens() {
    let (_directory, store) = repository();
    prepare_candidate(
        &store,
        1,
        "op-prep-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    accept_candidate(
        &store,
        1,
        "candidate-1",
        "op-accept-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    // Well-formed tokens parse and page even with whitespace, escapes, and
    // non-string cursor values (read leniently as "no cursor").
    for token in [
        " {\"after\": null, \"revision\": 1}",
        "{\"after\": \"a\\\"b\", \"revision\": 1}",
        "{\"after\": [[1]], \"revision\": 1}",
        "{\"after\": [1,2], \"revision\": 1}",
        "{\"after\": 123, \"revision\": 1}",
        "{\"after\": -1, \"revision\": 1}",
        "{\"after\": \"x\" , \"revision\": 1}",
    ] {
        store
            .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
                token: Some(token.to_owned()),
                limit: 10,
            })
            .expect("well-formed token pages");
    }
    // Every malformed shape is rejected with the typed token validation error.
    for token in [
        "not-json",
        "[1,2]",
        "{",
        "{}",
        "{\"after\": \"x\"",
        "{\"after\" 1}",
        "{\"after\": 1 2}",
        "{1: 2}",
        "{\"after\": \"abc",
        "{\"after\": x, \"revision\": 1}",
        "{\"after\": [1",
        "{\"after\": tru, \"revision\": 1}",
        "{\"after\": fals, \"revision\": 1}",
        "{\"after\": nul, \"revision\": 1}",
        "{\"after\": null, \"revision\": \"x\"}",
        "{\"after\": null}",
        "{\"after\": \"x\", \"revision\": 1",
    ] {
        let error = store
            .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
                token: Some(token.to_owned()),
                limit: 10,
            })
            .expect_err("malformed page token is rejected");
        assert_eq!(error.code(), "invalid_catalog_page_token");
    }
    // A token issued against a prior catalog revision goes stale.
    prepare_candidate(
        &store,
        2,
        "op-prep-2",
        "kind-b",
        "kd-2",
        "profile-b",
        "rev-b",
        2,
    );
    accept_candidate(
        &store,
        2,
        "candidate-2",
        "op-accept-2",
        "kind-b",
        "kd-2",
        "profile-b",
        "rev-b",
        2,
    );
    let stale = store
        .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
            token: Some("{\"after\": \"profile-a\", \"revision\": 1}".to_owned()),
            limit: 10,
        })
        .expect_err("stale page token is rejected");
    assert_eq!(stale.code(), "catalog_page_token_stale");
}

#[test]
fn catalog_accept_rejects_mismatched_revision_and_handle_without_writing() {
    let (_directory, store) = repository();
    prepare_candidate(
        &store,
        1,
        "op-prep-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    // A revision that does not match the prepared candidate conflicts before
    // any row is written.
    let revision_error = store
        .accept_provider_catalog(AcceptProviderCatalogInputDto {
            catalog_revision_id: 2,
            candidate_handle: "candidate-1".to_owned(),
            kind_descriptors: vec![ProviderKindDescriptorCandidateDto {
                descriptor_revision_id: "kd-1".to_owned(),
                descriptor: fixture_kind_descriptor("kind-a"),
            }],
            profiles: vec![fixture_profile_candidate("profile-a", "rev-a")],
            default_profile_id: "profile-a".to_owned(),
            accepted_at: 1,
            operation_id: "op-accept-bad-revision".to_owned(),
        })
        .expect_err("mismatched catalog revision conflicts");
    assert_eq!(revision_error.code(), "provider_catalog_revision_conflict");
    let state = store
        .load_provider_catalog_status()
        .expect("catalog state reloads");
    assert_eq!(state.candidate_catalog_revision_id, Some(1));
    assert_eq!(state.active_catalog_revision_id, None);
    let connection = raw_connection(&_directory);
    assert_eq!(
        query_count(
            &connection,
            "SELECT COUNT(*) FROM configuration_audit WHERE operation_id='op-accept-bad-revision'"
        ),
        0
    );
    assert_eq!(
        query_count(
            &connection,
            "SELECT COUNT(*) FROM provider_catalog_profile_projection"
        ),
        0
    );
}

#[test]
fn catalog_accept_covers_readiness_transport_and_tombstone_removal() {
    let (_directory, store) = repository();
    prepare_candidate(
        &store,
        1,
        "op-prep-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    accept_candidate(
        &store,
        1,
        "candidate-1",
        "op-accept-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    // Revision two replaces the catalog: profile-a and kind-a leave, the new
    // profiles exercise disabled/unavailable readiness and safe-header
    // transport, and the new kind exercises disabled reasoning. Only the kind
    // is prepared here; the acceptance inserts the profile revisions itself.
    store
        .append_provider_kind_descriptor_revision(AppendProviderKindDescriptorRevisionInputDto {
            descriptor_revision_id: "kd-2".to_owned(),
            descriptor: fixture_kind_descriptor("kind-b"),
            catalog_revision_id: 2,
            accepted_at: 2,
            operation_id: "op-prep-2".to_owned(),
        })
        .expect("fixture kind descriptor prepares");
    let mut kind_disabled_reasoning = fixture_kind_descriptor("kind-c");
    kind_disabled_reasoning.model_capability_envelope.reasoning = ReasoningCapability::Disabled;
    accept_catalog_with(
        &store,
        2,
        "candidate-2",
        "op-accept-2",
        vec![
            ProviderKindDescriptorCandidateDto {
                descriptor_revision_id: "kd-2".to_owned(),
                descriptor: fixture_kind_descriptor("kind-b"),
            },
            ProviderKindDescriptorCandidateDto {
                descriptor_revision_id: "kd-3".to_owned(),
                descriptor: kind_disabled_reasoning,
            },
        ],
        vec![
            fixture_profile_candidate_with(
                "profile-b",
                "rev-b",
                CredentialTransportMode::Bearer,
                None,
                ProviderReadinessDto::Ready,
                "kd-2",
            ),
            fixture_profile_candidate_with(
                "profile-c",
                "rev-c",
                CredentialTransportMode::Bearer,
                None,
                ProviderReadinessDto::Disabled,
                "kd-2",
            ),
            fixture_profile_candidate_with(
                "profile-d",
                "rev-d",
                CredentialTransportMode::SafeHeader,
                Some("x-provider-header"),
                ProviderReadinessDto::Unavailable,
                "kd-3",
            ),
        ],
        "profile-b",
        2,
    );
    // The previous profile and kind are tombstoned by the acceptance.
    let connection = raw_connection(&_directory);
    assert_eq!(
        query_count(
            &connection,
            "SELECT COUNT(*) FROM provider_profile_tombstones WHERE removed_catalog_revision_id=2"
        ),
        1
    );
    assert_eq!(
        query_count(
            &connection,
            "SELECT COUNT(*) FROM provider_kind_tombstones WHERE removed_catalog_revision_id=2"
        ),
        1
    );
    // The page reports the projected readiness of every entry, sorted.
    let page = store
        .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
            token: None,
            limit: 10,
        })
        .expect("replacement catalog page loads");
    assert_eq!(page.entries.len(), 3);
    assert_eq!(page.entries[0].profile_id, "profile-b");
    assert_eq!(page.entries[0].readiness, ProviderReadinessDto::Ready);
    assert_eq!(page.entries[1].profile_id, "profile-c");
    assert_eq!(page.entries[1].readiness, ProviderReadinessDto::Disabled);
    assert_eq!(page.entries[2].profile_id, "profile-d");
    assert_eq!(page.entries[2].readiness, ProviderReadinessDto::Unavailable);
    // The material round trips the disabled-reasoning envelope and the
    // safe-header transport, deduplicating the shared kind descriptor.
    let material = store
        .load_provider_catalog_material()
        .expect("replacement catalog material loads");
    assert_eq!(material.catalog_revision_id, 2);
    assert_eq!(material.default_profile_id.as_deref(), Some("profile-b"));
    assert_eq!(material.kind_descriptors.len(), 2);
    assert_eq!(
        material.kind_descriptors[1]
            .descriptor
            .model_capability_envelope
            .reasoning,
        ReasoningCapability::Disabled
    );
    assert_eq!(material.profiles.len(), 3);
    assert_eq!(
        material.profiles[2].profile.credential_transport_mode,
        CredentialTransportMode::SafeHeader
    );
    assert_eq!(
        material.profiles[2].profile.safe_header_name.as_deref(),
        Some("x-provider-header")
    );
}

#[test]
fn catalog_reject_and_expire_clear_prepared_candidates() {
    let (_directory, store) = repository();
    prepare_candidate(
        &store,
        1,
        "op-prep-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    // Rejecting a non-prepared revision conflicts without clearing.
    let mismatch = store
        .reject_provider_catalog_candidate(RejectProviderCatalogCandidateInputDto {
            catalog_revision_id: 2,
            candidate_handle: "candidate-1".to_owned(),
            rejected_at: 2,
            operation_id: "op-reject-bad".to_owned(),
        })
        .expect_err("rejecting a non-prepared revision conflicts");
    assert_eq!(mismatch.code(), "provider_catalog_candidate_conflict");
    // Rejecting the prepared candidate clears it and returns to preparing.
    store
        .reject_provider_catalog_candidate(RejectProviderCatalogCandidateInputDto {
            catalog_revision_id: 1,
            candidate_handle: "candidate-1".to_owned(),
            rejected_at: 2,
            operation_id: "op-reject-1".to_owned(),
        })
        .expect("prepared candidate rejects");
    let state = store
        .load_provider_catalog_status()
        .expect("catalog state reloads");
    assert_eq!(state.status, ProviderCatalogStatusDto::Preparing);
    assert_eq!(state.candidate_catalog_revision_id, None);
    let connection = raw_connection(&_directory);
    assert_eq!(
        query_count(
            &connection,
            "SELECT COUNT(*) FROM configuration_audit WHERE operation_id='op-reject-1' AND audit_kind='ProviderCatalogCandidateRejected'"
        ),
        1
    );
    // Expiring a non-prepared revision conflicts.
    prepare_candidate(
        &store,
        2,
        "op-prep-2",
        "kind-b",
        "kd-2",
        "profile-b",
        "rev-b",
        2,
    );
    let mismatch = store
        .expire_provider_catalog_candidate(ExpireProviderCatalogCandidateInputDto {
            catalog_revision_id: 3,
            expired_at: 3,
            operation_id: "op-expire-bad".to_owned(),
        })
        .expect_err("expiring a non-prepared revision conflicts");
    assert_eq!(mismatch.code(), "provider_catalog_candidate_conflict");
    store
        .expire_provider_catalog_candidate(ExpireProviderCatalogCandidateInputDto {
            catalog_revision_id: 2,
            expired_at: 3,
            operation_id: "op-expire-1".to_owned(),
        })
        .expect("prepared candidate expires");
    let state = store
        .load_provider_catalog_status()
        .expect("catalog state reloads");
    assert_eq!(state.status, ProviderCatalogStatusDto::Preparing);
    assert_eq!(state.candidate_catalog_revision_id, None);
    assert_eq!(
        query_count(
            &connection,
            "SELECT COUNT(*) FROM configuration_audit WHERE operation_id='op-expire-1' AND audit_kind='ProviderCatalogCandidateExpired'"
        ),
        1
    );
    // Rejecting a candidate while an active catalog exists restores active.
    prepare_candidate(
        &store,
        3,
        "op-prep-3",
        "kind-c",
        "kd-3",
        "profile-c",
        "rev-c",
        3,
    );
    accept_candidate(
        &store,
        3,
        "candidate-3",
        "op-accept-3",
        "kind-c",
        "kd-3",
        "profile-c",
        "rev-c",
        3,
    );
    prepare_candidate(
        &store,
        4,
        "op-prep-4",
        "kind-d",
        "kd-4",
        "profile-d",
        "rev-d",
        4,
    );
    store
        .reject_provider_catalog_candidate(RejectProviderCatalogCandidateInputDto {
            catalog_revision_id: 4,
            candidate_handle: "candidate-4".to_owned(),
            rejected_at: 5,
            operation_id: "op-reject-2".to_owned(),
        })
        .expect("candidate rejects over an active catalog");
    let state = store
        .load_provider_catalog_status()
        .expect("catalog state reloads");
    assert_eq!(state.status, ProviderCatalogStatusDto::Active);
    assert_eq!(state.active_catalog_revision_id, Some(3));
    assert_eq!(state.candidate_catalog_revision_id, None);
}

#[test]
fn catalog_page_and_material_require_an_active_catalog() {
    let (_directory, store) = repository();
    let error = store
        .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
            token: None,
            limit: 10,
        })
        .expect_err("page without an active catalog is not found");
    assert_eq!(error.code(), "provider_catalog_not_active");
    let error = store
        .load_provider_catalog_material()
        .expect_err("material without an active catalog is not found");
    assert_eq!(error.code(), "provider_catalog_not_active");
}

#[test]
fn catalog_revision_outside_sqlite_range_is_rejected_typed() {
    let (_directory, store) = repository();
    // A catalog revision that cannot round-trip through SQLite is rejected
    // with the typed decode error and nothing durable is written.
    let error = store
        .append_provider_kind_descriptor_revision(AppendProviderKindDescriptorRevisionInputDto {
            descriptor_revision_id: "kd-1".to_owned(),
            descriptor: fixture_kind_descriptor("kind-a"),
            catalog_revision_id: u64::MAX,
            accepted_at: 1,
            operation_id: "op-out-of-range".to_owned(),
        })
        .expect_err("out-of-range catalog revision is rejected");
    assert_eq!(error.code(), "storage_decode_failed");
    let error = store
        .append_provider_profile_revision(AppendProviderProfileRevisionInputDto {
            profile: fixture_profile_candidate("profile-a", "rev-a"),
            catalog_revision_id: u64::MAX,
            accepted_at: 1,
            operation_id: "op-out-of-range".to_owned(),
        })
        .expect_err("out-of-range profile revision is rejected");
    assert_eq!(error.code(), "storage_decode_failed");
    // A negative acceptance timestamp cannot become a durable tombstone time.
    prepare_candidate(
        &store,
        1,
        "op-prep-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    accept_candidate(
        &store,
        1,
        "candidate-1",
        "op-accept-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    prepare_candidate(
        &store,
        2,
        "op-prep-2",
        "kind-b",
        "kd-2",
        "profile-b",
        "rev-b",
        2,
    );
    let error = store
        .accept_provider_catalog(AcceptProviderCatalogInputDto {
            catalog_revision_id: 2,
            candidate_handle: "candidate-2".to_owned(),
            kind_descriptors: vec![ProviderKindDescriptorCandidateDto {
                descriptor_revision_id: "kd-2".to_owned(),
                descriptor: fixture_kind_descriptor("kind-b"),
            }],
            profiles: vec![fixture_profile_candidate("profile-b", "rev-b")],
            default_profile_id: "profile-b".to_owned(),
            accepted_at: -1,
            operation_id: "op-accept-negative-time".to_owned(),
        })
        .expect_err("negative acceptance timestamp is rejected");
    assert_eq!(error.code(), "storage_decode_failed");
    let state = store
        .load_provider_catalog_status()
        .expect("catalog state reloads");
    assert_eq!(state.active_catalog_revision_id, Some(1));
}

// ---------------------------------------------------------------------------
// Provider catalog removal lifecycle.
// ---------------------------------------------------------------------------

#[test]
fn removal_candidate_creation_enforces_single_pending_and_accept_flow() {
    let (_directory, store) = repository();
    prepare_candidate(
        &store,
        1,
        "op-prep-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    accept_candidate(
        &store,
        1,
        "candidate-1",
        "op-accept-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    store
        .create_provider_catalog_removal_candidate(CreateProviderCatalogRemovalCandidateInputDto {
            candidate_handle: "removal-1".to_owned(),
            candidate_catalog_revision_id: 2,
            active_catalog_revision_id: 1,
            created_at: 100,
            source_recheck: "health-recheck".to_owned(),
            candidate_json: "{\"safe\":true}".to_owned(),
            operation_id: "op-removal-1".to_owned(),
        })
        .expect("removal candidate creates");
    // The catalog state flips to pending removal.
    let state = store
        .load_provider_catalog_status()
        .expect("catalog state reloads");
    assert_eq!(state.status, ProviderCatalogStatusDto::PendingRemoval);
    // At most one pending candidate exists: the partial unique index rejects
    // the second pending insert. The repository maps the constraint violation
    // to `provider_catalog_removal_pending_exists` only when the driver error
    // text names the index; the bundled SQLite names the column instead, so
    // the durable conflict surfaces as a storage error while the invariant
    // still holds.
    store
        .create_provider_catalog_removal_candidate(CreateProviderCatalogRemovalCandidateInputDto {
            candidate_handle: "removal-2".to_owned(),
            candidate_catalog_revision_id: 3,
            active_catalog_revision_id: 1,
            created_at: 101,
            source_recheck: "health-recheck".to_owned(),
            candidate_json: "{\"safe\":true}".to_owned(),
            operation_id: "op-removal-2".to_owned(),
        })
        .expect_err("a second pending removal candidate is rejected");
    let connection = raw_connection(&_directory);
    assert_eq!(
        query_count(
            &connection,
            "SELECT COUNT(*) FROM provider_catalog_removal_candidates WHERE status='pending'"
        ),
        1
    );
    let expires_at: i64 = connection
        .query_row(
            "SELECT expires_at FROM provider_catalog_removal_candidates WHERE candidate_handle='removal-1'",
            [],
            |row| row.get(0),
        )
        .expect("expiry reads");
    assert_eq!(expires_at, 100 + 30 * 60);
    // Accepting the pending candidate closes it with its operation id.
    store
        .accept_provider_catalog_removal(AcceptProviderCatalogRemovalInputDto {
            candidate_handle: "removal-1".to_owned(),
            accepted_at: 110,
            operation_id: "op-removal-accept-1".to_owned(),
        })
        .expect("pending removal candidate accepts");
    let (status, operation_id, completed_at): (String, Option<String>, Option<i64>) = connection
        .query_row(
            "SELECT status, operation_id, completed_at FROM provider_catalog_removal_candidates WHERE candidate_handle='removal-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("removal row reads");
    assert_eq!(status, "accepted");
    assert_eq!(operation_id.as_deref(), Some("op-removal-accept-1"));
    assert_eq!(completed_at, Some(110));
    assert_eq!(
        query_count(
            &connection,
            "SELECT COUNT(*) FROM configuration_audit WHERE operation_id='op-removal-accept-1' AND audit_kind='ProviderCatalogRemovalAccepted'"
        ),
        1
    );
    // Accepting the same candidate twice conflicts: the candidate is closed.
    let again = store
        .accept_provider_catalog_removal(AcceptProviderCatalogRemovalInputDto {
            candidate_handle: "removal-1".to_owned(),
            accepted_at: 111,
            operation_id: "op-removal-accept-1".to_owned(),
        })
        .expect_err("accepting a closed candidate conflicts");
    assert_eq!(again.code(), "provider_catalog_removal_not_pending");
    // Unknown candidates are typed not-found on both accept and reject.
    let unknown = store
        .accept_provider_catalog_removal(AcceptProviderCatalogRemovalInputDto {
            candidate_handle: "removal-unknown".to_owned(),
            accepted_at: 111,
            operation_id: "op-removal-accept-unknown".to_owned(),
        })
        .expect_err("accepting an unknown candidate is not found");
    assert_eq!(unknown.code(), "provider_catalog_removal_not_found");
    let unknown = store
        .reject_provider_catalog_removal(RejectProviderCatalogRemovalInputDto {
            candidate_handle: "removal-unknown".to_owned(),
            rejected_at: 111,
            operation_id: "op-removal-reject-unknown".to_owned(),
        })
        .expect_err("rejecting an unknown candidate is not found");
    assert_eq!(unknown.code(), "provider_catalog_removal_not_found");
}

#[test]
fn removal_candidate_reject_expire_and_non_pending_guards() {
    let (_directory, store) = repository();
    prepare_candidate(
        &store,
        1,
        "op-prep-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    accept_candidate(
        &store,
        1,
        "candidate-1",
        "op-accept-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    store
        .create_provider_catalog_removal_candidate(CreateProviderCatalogRemovalCandidateInputDto {
            candidate_handle: "removal-1".to_owned(),
            candidate_catalog_revision_id: 2,
            active_catalog_revision_id: 1,
            created_at: 100,
            source_recheck: "health-recheck".to_owned(),
            candidate_json: "{\"safe\":true}".to_owned(),
            operation_id: "op-removal-1".to_owned(),
        })
        .expect("removal candidate creates");
    store
        .reject_provider_catalog_removal(RejectProviderCatalogRemovalInputDto {
            candidate_handle: "removal-1".to_owned(),
            rejected_at: 110,
            operation_id: "op-removal-reject-1".to_owned(),
        })
        .expect("pending removal candidate rejects");
    let connection = raw_connection(&_directory);
    let (status, completed_at): (String, Option<i64>) = connection
        .query_row(
            "SELECT status, completed_at FROM provider_catalog_removal_candidates WHERE candidate_handle='removal-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("removal row reads");
    assert_eq!(status, "rejected");
    assert_eq!(completed_at, Some(110));
    // Rejection restores the active catalog state.
    let state = store
        .load_provider_catalog_status()
        .expect("catalog state reloads");
    assert_eq!(state.status, ProviderCatalogStatusDto::Active);
    // Rejecting the closed candidate conflicts.
    let closed = store
        .reject_provider_catalog_removal(RejectProviderCatalogRemovalInputDto {
            candidate_handle: "removal-1".to_owned(),
            rejected_at: 111,
            operation_id: "op-removal-reject-1".to_owned(),
        })
        .expect_err("rejecting a closed candidate conflicts");
    assert_eq!(closed.code(), "provider_catalog_removal_not_pending");
    // Expiry only closes pending candidates past their thirty-minute window.
    store
        .create_provider_catalog_removal_candidate(CreateProviderCatalogRemovalCandidateInputDto {
            candidate_handle: "removal-2".to_owned(),
            candidate_catalog_revision_id: 3,
            active_catalog_revision_id: 1,
            created_at: 200,
            source_recheck: "health-recheck".to_owned(),
            candidate_json: "{\"safe\":true}".to_owned(),
            operation_id: "op-removal-3".to_owned(),
        })
        .expect("second removal candidate creates");
    let before_window = store
        .expire_provider_catalog_removal_candidate(ExpireProviderCatalogRemovalCandidateInputDto {
            now: 200 + 30 * 60 - 1,
            operation_id: "op-removal-expire-early".to_owned(),
        })
        .expect("early expiry runs");
    assert_eq!(before_window, 0);
    let (status,) = connection
        .query_row(
            "SELECT status FROM provider_catalog_removal_candidates WHERE candidate_handle='removal-2'",
            [],
            |row| Ok((row.get::<_, String>(0)?,)),
        )
        .expect("removal row reads");
    assert_eq!(status, "pending");
    let expired = store
        .expire_provider_catalog_removal_candidate(ExpireProviderCatalogRemovalCandidateInputDto {
            now: 200 + 30 * 60,
            operation_id: "op-removal-expire-1".to_owned(),
        })
        .expect("expiry runs");
    assert_eq!(expired, 1);
    let (status, completed_at): (String, Option<i64>) = connection
        .query_row(
            "SELECT status, completed_at FROM provider_catalog_removal_candidates WHERE candidate_handle='removal-2'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("removal row reads");
    assert_eq!(status, "expired");
    assert_eq!(completed_at, Some(200 + 30 * 60));
    // Expiring again closes nothing.
    let none = store
        .expire_provider_catalog_removal_candidate(ExpireProviderCatalogRemovalCandidateInputDto {
            now: 300 + 30 * 60,
            operation_id: "op-removal-expire-2".to_owned(),
        })
        .expect("second expiry runs");
    assert_eq!(none, 0);
    // Accepting the expired candidate conflicts as no longer pending.
    let closed = store
        .accept_provider_catalog_removal(AcceptProviderCatalogRemovalInputDto {
            candidate_handle: "removal-2".to_owned(),
            accepted_at: 300,
            operation_id: "op-removal-accept-expired".to_owned(),
        })
        .expect_err("accepting an expired candidate conflicts");
    assert_eq!(closed.code(), "provider_catalog_removal_not_pending");
}

// ---------------------------------------------------------------------------
// Unavailable-provider queue.
// ---------------------------------------------------------------------------

#[test]
fn unavailable_queue_enqueue_is_idempotent_and_pages_fifo() {
    let (_directory, store) = repository();
    let session = create(&store);
    let first = accept_terminal_run(&store, session);
    let second = accept_terminal_run(&store, session);
    let third = accept_terminal_run(&store, session);
    enqueue(
        &store,
        session,
        first,
        "provider_unavailable",
        "op-enqueue-1",
        3,
    );
    enqueue(
        &store,
        session,
        second,
        "provider_unavailable",
        "op-enqueue-2",
        4,
    );
    enqueue(
        &store,
        session,
        third,
        "provider_unavailable",
        "op-enqueue-3",
        5,
    );
    // Enqueueing the same run again is idempotent.
    enqueue(
        &store,
        session,
        first,
        "provider_unavailable",
        "op-enqueue-1b",
        6,
    );
    let connection = raw_connection(&_directory);
    assert_eq!(
        query_count(
            &connection,
            "SELECT COUNT(*) FROM unavailable_provider_queue"
        ),
        3
    );
    // Limits outside the supported range are rejected.
    for limit in [0_u64, 1025_u64] {
        let error = store
            .load_unavailable_queue_page(LoadUnavailableQueuePageInputDto {
                after_queue_id: None,
                limit,
            })
            .expect_err("invalid queue page limit is rejected");
        assert_eq!(error.code(), "invalid_unavailable_queue_page_limit");
    }
    // The queue pages in FIFO order and preserves every enqueued field.
    let first_page = store
        .load_unavailable_queue_page(LoadUnavailableQueuePageInputDto {
            after_queue_id: None,
            limit: 2,
        })
        .expect("first queue page loads");
    assert_eq!(first_page.len(), 2);
    assert_eq!(first_page[0].run_id, first);
    assert_eq!(first_page[1].run_id, second);
    assert_eq!(first_page[0].session_id, session);
    assert_eq!(first_page[0].profile_id, "profile-a");
    assert_eq!(first_page[0].provider_profile_revision_id, "rev-a");
    assert_eq!(first_page[0].unavailable_reason, "provider_unavailable");
    assert_eq!(first_page[0].first_unavailable_at, 3);
    assert_eq!(first_page[0].promotion_attempts, 0);
    assert_eq!(first_page[0].state, UnavailableQueueStateDto::Queued);
    assert_eq!(
        first_page[0].last_operation_id.as_deref(),
        Some("op-enqueue-1")
    );
    assert_eq!(first_page[0].selection_json, "{\"safe\":true}");
    // The cursor resumes after the last seen queue id.
    let second_page = store
        .load_unavailable_queue_page(LoadUnavailableQueuePageInputDto {
            after_queue_id: Some(first_page[1].queue_id),
            limit: 2,
        })
        .expect("second queue page loads");
    assert_eq!(second_page.len(), 1);
    assert_eq!(second_page[0].run_id, third);
    let empty = store
        .load_unavailable_queue_page(LoadUnavailableQueuePageInputDto {
            after_queue_id: Some(second_page[0].queue_id),
            limit: 2,
        })
        .expect("terminal queue page loads");
    assert!(empty.is_empty());
}

#[test]
fn unavailable_queue_promotion_batches_and_marks_exhaustion() {
    let (_directory, store) = repository();
    let session = create(&store);
    let runs: Vec<RunId> = (0..9)
        .map(|index| {
            let run = accept_terminal_run(&store, session);
            enqueue(
                &store,
                session,
                run,
                "provider_unavailable",
                &format!("op-enqueue-{index}"),
                10 + index,
            );
            run
        })
        .collect();
    // Promotion batch limits are validated.
    for max in [0_u64, 9_u64] {
        let error = store
            .promote_unavailable_runs(PromoteUnavailableRunsInputDto {
                now: 20,
                operation_id: "op-promote-bad".to_owned(),
                max,
            })
            .expect_err("invalid promotion batch is rejected");
        assert_eq!(error.code(), "invalid_unavailable_queue_promotion");
    }
    // Eight of nine runs promote; the queue is not exhausted, so no marker.
    let first = store
        .promote_unavailable_runs(PromoteUnavailableRunsInputDto {
            now: 20,
            operation_id: "op-promote-1".to_owned(),
            max: 8,
        })
        .expect("first promotion pass commits");
    assert_eq!(first.promoted.len(), 8);
    assert!(!first.reconciliation_marker_created);
    for (index, entry) in first.promoted.iter().enumerate() {
        assert_eq!(entry.run_id, runs[index]);
        assert_eq!(entry.state, UnavailableQueueStateDto::Promoted);
        assert_eq!(entry.promotion_attempts, 1);
        assert_eq!(entry.last_operation_id.as_deref(), Some("op-promote-1"));
        assert_eq!(entry.profile_id, "profile-a");
        assert_eq!(entry.provider_profile_revision_id, "rev-a");
        assert_eq!(entry.selection_json, "{\"safe\":true}");
    }
    assert!(
        store
            .load_queue_reconciliation_marker(session)
            .expect("marker loads")
            .is_none()
    );
    // The remaining run still queues behind the promoted entries.
    let page = store
        .load_unavailable_queue_page(LoadUnavailableQueuePageInputDto {
            after_queue_id: None,
            limit: 20,
        })
        .expect("queue page reloads");
    assert_eq!(page.len(), 9);
    assert_eq!(page[8].run_id, runs[8]);
    assert_eq!(page[8].state, UnavailableQueueStateDto::Queued);
    // The final promotion exhausts the queue and creates the marker.
    let second = store
        .promote_unavailable_runs(PromoteUnavailableRunsInputDto {
            now: 30,
            operation_id: "op-promote-2".to_owned(),
            max: 8,
        })
        .expect("second promotion pass commits");
    assert_eq!(second.promoted.len(), 1);
    assert_eq!(second.promoted[0].run_id, runs[8]);
    assert!(second.reconciliation_marker_created);
    let marker = store
        .load_queue_reconciliation_marker(session)
        .expect("marker loads")
        .expect("exhaustion marker exists");
    assert_eq!(marker.session_id, session);
    assert_eq!(marker.created_at, 30);
    assert_eq!(marker.reason, "promotion_exhausted");
    assert_eq!(marker.next_page_cursor, None);
    assert_eq!(marker.resolved_at, None);
    // Promoting the exhausted queue again promotes nothing and keeps the marker.
    let empty = store
        .promote_unavailable_runs(PromoteUnavailableRunsInputDto {
            now: 31,
            operation_id: "op-promote-3".to_owned(),
            max: 8,
        })
        .expect("third promotion pass commits");
    assert!(empty.promoted.is_empty());
    assert!(!empty.reconciliation_marker_created);
}

#[test]
fn unavailable_queue_reconciliation_terminalizes_and_batches() {
    let (_directory, store) = repository();
    let session = create(&store);
    let terminal: Vec<RunId> = (0..3)
        .map(|index| {
            let run = accept_terminal_run(&store, session);
            enqueue(
                &store,
                session,
                run,
                "provider_unavailable",
                &format!("op-enqueue-t-{index}"),
                10 + index,
            );
            run
        })
        .collect();
    // Active runs need their own sessions: only one run may be active per
    // session, and the queue rows reference durable runs.
    let active: Vec<RunId> = (0..2)
        .map(|index| {
            let active_session = create(&store);
            let run = RunId::new();
            accept(&store, active_session, run, "active run");
            enqueue(
                &store,
                active_session,
                run,
                "provider_unavailable",
                &format!("op-enqueue-a-{index}"),
                20 + index,
            );
            run
        })
        .collect();
    // Reconciliation batch limits are validated.
    for max in [0_u64, 33_u64] {
        let error = store
            .reconcile_unavailable_queue(ReconcileUnavailableQueueInputDto {
                now: 30,
                operation_id: "op-reconcile-bad".to_owned(),
                max,
            })
            .expect_err("invalid reconciliation batch is rejected");
        assert_eq!(error.code(), "invalid_unavailable_queue_reconciliation");
    }
    // Terminal runs terminalize; active runs stay queued.
    let outcome = store
        .reconcile_unavailable_queue(ReconcileUnavailableQueueInputDto {
            now: 30,
            operation_id: "op-reconcile-1".to_owned(),
            max: 32,
        })
        .expect("reconciliation commits");
    assert_eq!(outcome.processed.len(), 5);
    assert_eq!(outcome.terminalized.len(), 3);
    for (index, entry) in outcome.terminalized.iter().enumerate() {
        assert_eq!(entry.run_id, terminal[index]);
        assert_eq!(entry.state, UnavailableQueueStateDto::Terminalized);
        assert_eq!(entry.last_operation_id.as_deref(), Some("op-reconcile-1"));
    }
    let page = store
        .load_unavailable_queue_page(LoadUnavailableQueuePageInputDto {
            after_queue_id: None,
            limit: 20,
        })
        .expect("queue page reloads");
    assert_eq!(
        page.iter()
            .filter(|entry| entry.state == UnavailableQueueStateDto::Queued)
            .map(|entry| entry.run_id)
            .collect::<Vec<_>>(),
        active
    );
    assert_eq!(
        page.iter()
            .filter(|entry| entry.state == UnavailableQueueStateDto::Terminalized)
            .count(),
        3
    );
    // A reconciliation batch never processes more than thirty-two entries.
    let big_session = create(&store);
    let big_runs: Vec<RunId> = (0..33)
        .map(|index| {
            let run = accept_terminal_run(&store, big_session);
            enqueue(
                &store,
                big_session,
                run,
                "provider_unavailable",
                &format!("op-enqueue-big-{index}"),
                100 + index,
            );
            run
        })
        .collect();
    let bounded = store
        .reconcile_unavailable_queue(ReconcileUnavailableQueueInputDto {
            now: 200,
            operation_id: "op-reconcile-big-1".to_owned(),
            max: 32,
        })
        .expect("bounded reconciliation commits");
    // The two active runs from the first scenario are processed first in FIFO
    // order and stay queued, so thirty of the thirty-two are terminalized.
    assert_eq!(bounded.processed.len(), 32);
    assert_eq!(bounded.terminalized.len(), 30);
    let remainder = store
        .reconcile_unavailable_queue(ReconcileUnavailableQueueInputDto {
            now: 201,
            operation_id: "op-reconcile-big-2".to_owned(),
            max: 32,
        })
        .expect("remainder reconciliation commits");
    assert_eq!(remainder.processed.len(), 5);
    assert_eq!(remainder.terminalized.len(), 3);
    assert_eq!(remainder.processed[2].run_id, big_runs[30]);
    assert_eq!(remainder.terminalized[0].run_id, big_runs[30]);
}

// ---------------------------------------------------------------------------
// Provider usage.
// ---------------------------------------------------------------------------

fn usage_event(
    run: RunId,
    event_id: &str,
    input_units: u64,
    output_units: u64,
    reasoning_units: u64,
    occurred_at: i64,
) -> ProviderUsageEventInputDto {
    ProviderUsageEventInputDto {
        run_id: run,
        usage_event_id: event_id.to_owned(),
        profile_id: "profile-a".to_owned(),
        provider_profile_revision_id: "rev-a".to_owned(),
        model_id: "model-m".to_owned(),
        input_units,
        output_units,
        reasoning_units,
        occurred_at,
        usage_json: "{\"safe\":true}".to_owned(),
    }
}

#[test]
fn provider_usage_recording_dedups_and_aggregates_across_periods() {
    let (_directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    accept(&store, session, run, "usage run");
    let record = |period_start: i64,
                  period_end: i64,
                  recorded_at: i64,
                  events: Vec<ProviderUsageEventInputDto>| {
        store
            .record_provider_usage(RecordProviderUsageInputDto {
                session_id: session,
                usage_period_start: period_start,
                usage_period_end: period_end,
                recorded_at,
                events,
            })
            .expect("usage records")
    };
    record(
        100,
        200,
        3,
        vec![
            usage_event(run, "usage-event-1", 10, 20, 5, 3),
            usage_event(run, "usage-event-2", 30, 40, 15, 4),
        ],
    );
    // Re-recording the identical events never double counts.
    record(
        100,
        200,
        4,
        vec![
            usage_event(run, "usage-event-1", 10, 20, 5, 3),
            usage_event(run, "usage-event-2", 30, 40, 15, 4),
        ],
    );
    // A second usage period aggregates separately.
    record(
        300,
        400,
        5,
        vec![usage_event(run, "usage-event-3", 1, 2, 3, 5)],
    );
    let connection = raw_connection(&_directory);
    assert_eq!(
        query_count(&connection, "SELECT COUNT(*) FROM provider_usage_facts"),
        3
    );
    let by_profile = store
        .load_provider_usage_by_profile("profile-a".to_owned())
        .expect("usage aggregates load by profile");
    assert_eq!(by_profile.len(), 2);
    assert_eq!(by_profile[0].usage_period_start, 100);
    assert_eq!(by_profile[0].usage_period_end, 200);
    assert_eq!(by_profile[0].request_count, 2);
    assert_eq!(by_profile[0].input_units, 40);
    assert_eq!(by_profile[0].output_units, 60);
    assert_eq!(by_profile[0].reasoning_units, 20);
    assert_eq!(by_profile[0].last_run_id, Some(run));
    assert_eq!(by_profile[0].updated_at, 3);
    assert_eq!(by_profile[0].profile_id, "profile-a");
    assert_eq!(by_profile[0].provider_profile_revision_id, "rev-a");
    assert_eq!(by_profile[0].model_id, "model-m");
    assert_eq!(by_profile[1].usage_period_start, 300);
    assert_eq!(by_profile[1].request_count, 1);
    assert_eq!(by_profile[1].input_units, 1);
    assert_eq!(by_profile[1].output_units, 2);
    assert_eq!(by_profile[1].reasoning_units, 3);
    // The revision-and-model view returns the same aggregates.
    let by_revision = store
        .load_provider_usage_by_revision_and_model("rev-a".to_owned(), "model-m".to_owned())
        .expect("usage aggregates load by revision and model");
    assert_eq!(by_revision, by_profile);
    // Unknown profiles and revisions read empty.
    assert!(
        store
            .load_provider_usage_by_profile("profile-unknown".to_owned())
            .expect("unknown profile reads empty")
            .is_empty()
    );
    assert!(
        store
            .load_provider_usage_by_revision_and_model(
                "rev-unknown".to_owned(),
                "model-m".to_owned()
            )
            .expect("unknown revision reads empty")
            .is_empty()
    );
}

#[test]
fn provider_usage_rejects_units_outside_the_sqlite_range() {
    let (_directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    accept(&store, session, run, "usage run");
    for units in [
        usage_event(run, "usage-overflow-input", u64::MAX, 0, 0, 3),
        usage_event(run, "usage-overflow-output", 0, u64::MAX, 0, 3),
        usage_event(run, "usage-overflow-reasoning", 0, 0, u64::MAX, 3),
    ] {
        let error = store
            .record_provider_usage(RecordProviderUsageInputDto {
                session_id: session,
                usage_period_start: 100,
                usage_period_end: 200,
                recorded_at: 3,
                events: vec![units],
            })
            .expect_err("out-of-range usage units are rejected");
        assert_eq!(error.code(), "storage_decode_failed");
    }
    // Nothing durable was written by the rejected batches.
    let connection = raw_connection(&_directory);
    assert_eq!(
        query_count(&connection, "SELECT COUNT(*) FROM provider_usage_facts"),
        0
    );
    assert_eq!(
        query_count(
            &connection,
            "SELECT COUNT(*) FROM provider_usage_aggregates"
        ),
        0
    );
}

// ---------------------------------------------------------------------------
// Credential-free storage sweep across the queue, usage, and removal paths.
// ---------------------------------------------------------------------------

#[test]
fn queue_usage_removal_tables_never_persist_fake_secrets() {
    let (directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    accept(&store, session, run, "run");
    prepare_candidate(
        &store,
        1,
        "op-prep-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    accept_candidate(
        &store,
        1,
        "candidate-1",
        "op-accept-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    enqueue(
        &store,
        session,
        run,
        "provider_unavailable",
        "op-enqueue-1",
        3,
    );
    store
        .record_provider_usage(RecordProviderUsageInputDto {
            session_id: session,
            usage_period_start: 100,
            usage_period_end: 200,
            recorded_at: 3,
            events: vec![usage_event(run, "usage-event-1", 10, 20, 5, 3)],
        })
        .expect("usage records");
    store
        .create_provider_catalog_removal_candidate(CreateProviderCatalogRemovalCandidateInputDto {
            candidate_handle: "removal-1".to_owned(),
            candidate_catalog_revision_id: 2,
            active_catalog_revision_id: 1,
            created_at: 3,
            source_recheck: "health-recheck".to_owned(),
            candidate_json: "{\"safe\":true}".to_owned(),
            operation_id: "op-removal-1".to_owned(),
        })
        .expect("removal candidate creates");
    let connection = sqlite::Connection::open(db_path(&directory)).expect("database reopens");
    let tables = [
        "provider_catalog_profile_projection",
        "configuration_audit",
        "unavailable_provider_queue",
        "provider_usage_facts",
        "provider_usage_aggregates",
        "provider_catalog_removal_candidates",
    ];
    for table in tables {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table})"))
            .expect("table info prepares");
        let columns = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })
            .expect("table info runs")
            .map(|row| row.expect("table info row reads"))
            .collect::<Vec<_>>();
        drop(statement);
        for (column, type_) in columns {
            if !type_.to_uppercase().contains("TEXT") && !type_.to_uppercase().contains("JSON") {
                continue;
            }
            let mut statement = connection
                .prepare(&format!("SELECT {column} FROM {table}"))
                .expect("inspection query prepares");
            let rows = statement
                .query_map([], |row| row.get::<_, Option<String>>(0))
                .expect("inspection query runs");
            for row in rows {
                if let Some(value) = row.expect("inspection row reads") {
                    assert!(
                        !contains_credential_shape(&value),
                        "credential-shaped value in {table}.{column}: {value}"
                    );
                }
            }
            drop(statement);
        }
    }
    // The repository reads back the same credential-free values.
    let page = store
        .load_unavailable_queue_page(LoadUnavailableQueuePageInputDto {
            after_queue_id: None,
            limit: 10,
        })
        .expect("queue page reloads");
    assert_eq!(page.len(), 1);
    assert!(!contains_credential_shape(&page[0].selection_json));
    assert!(!contains_credential_shape(&page[0].unavailable_reason));
    let aggregates = store
        .load_provider_usage_by_profile("profile-a".to_owned())
        .expect("usage aggregates reload");
    assert_eq!(aggregates.len(), 1);
    assert!(!contains_credential_shape(&aggregates[0].model_id));
}

// ---------------------------------------------------------------------------
// Typed decode failures for corrupted persisted records.
// ---------------------------------------------------------------------------

#[test]
fn corrupted_catalog_records_fail_typed_decode() {
    let (directory, store) = repository();
    prepare_candidate(
        &store,
        1,
        "op-prep-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    accept_candidate(
        &store,
        1,
        "candidate-1",
        "op-accept-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    let connection = raw_connection(&directory);
    let material_fails = || {
        store
            .load_provider_catalog_material()
            .expect_err("corrupted material fails decode")
            .code()
            .to_owned()
    };
    // A kind descriptor record missing every field fails the typed decode.
    connection
        .execute(
            "UPDATE provider_kind_descriptor_revisions SET descriptor_json='{}' WHERE descriptor_revision_id='kd-1'",
            [],
        )
        .expect("kind record corrupts");
    assert_eq!(material_fails(), "storage_decode_failed");
    // A kind descriptor missing the protocol part list fails the list decode.
    connection
        .execute(
            "UPDATE provider_kind_descriptor_revisions SET descriptor_json='{\"credential_transport_contract\":\"bearer-or-safe-header\",\"descriptor_family\":\"responses-descriptor\",\"driver_contract_family\":\"responses\",\"endpoint_policy\":\"https-only\",\"kind_id\":\"kind-a\",\"model_capability_envelope\":{\"context_preservation\":{\"local_durable_history_v1\":{\"reasoning_input_contract\":\"reasoning-history-transfer-v1\"}},\"reasoning\":\"textual_reasoning_v1\",\"taxonomy_version\":\"model-capability-taxonomy-v1\",\"text_streaming\":true,\"tool_exchange\":false}}' WHERE descriptor_revision_id='kd-1'",
            [],
        )
        .expect("kind record corrupts");
    assert_eq!(material_fails(), "storage_decode_failed");
    // A capability envelope missing its context preservation entry fails.
    connection
        .execute(
            "UPDATE provider_kind_descriptor_revisions SET descriptor_json='{\"credential_transport_contract\":\"bearer-or-safe-header\",\"descriptor_family\":\"responses-descriptor\",\"driver_contract_family\":\"responses\",\"endpoint_policy\":\"https-only\",\"kind_id\":\"kind-a\",\"model_capability_envelope\":{},\"ordered_protocol_part_revisions\":[\"parts-v1\"]}' WHERE descriptor_revision_id='kd-1'",
            [],
        )
        .expect("envelope record corrupts");
    assert_eq!(material_fails(), "storage_decode_failed");
    // A capability envelope missing the local history entry fails.
    connection
        .execute(
            "UPDATE provider_kind_descriptor_revisions SET descriptor_json='{\"credential_transport_contract\":\"bearer-or-safe-header\",\"descriptor_family\":\"responses-descriptor\",\"driver_contract_family\":\"responses\",\"endpoint_policy\":\"https-only\",\"kind_id\":\"kind-a\",\"model_capability_envelope\":{\"context_preservation\":{},\"reasoning\":\"textual_reasoning_v1\",\"taxonomy_version\":\"model-capability-taxonomy-v1\",\"text_streaming\":true,\"tool_exchange\":false},\"ordered_protocol_part_revisions\":[\"parts-v1\"]}' WHERE descriptor_revision_id='kd-1'",
            [],
        )
        .expect("envelope record corrupts");
    assert_eq!(material_fails(), "storage_decode_failed");
    // A capability envelope missing the reasoning input contract fails.
    connection
        .execute(
            "UPDATE provider_kind_descriptor_revisions SET descriptor_json='{\"credential_transport_contract\":\"bearer-or-safe-header\",\"descriptor_family\":\"responses-descriptor\",\"driver_contract_family\":\"responses\",\"endpoint_policy\":\"https-only\",\"kind_id\":\"kind-a\",\"model_capability_envelope\":{\"context_preservation\":{\"local_durable_history_v1\":{}},\"reasoning\":\"textual_reasoning_v1\",\"taxonomy_version\":\"model-capability-taxonomy-v1\",\"text_streaming\":true,\"tool_exchange\":false},\"ordered_protocol_part_revisions\":[\"parts-v1\"]}' WHERE descriptor_revision_id='kd-1'",
            [],
        )
        .expect("envelope record corrupts");
    assert_eq!(material_fails(), "storage_decode_failed");
    // A capability envelope missing the streaming boolean fails the bool decode.
    connection
        .execute(
            "UPDATE provider_kind_descriptor_revisions SET descriptor_json='{\"credential_transport_contract\":\"bearer-or-safe-header\",\"descriptor_family\":\"responses-descriptor\",\"driver_contract_family\":\"responses\",\"endpoint_policy\":\"https-only\",\"kind_id\":\"kind-a\",\"model_capability_envelope\":{\"context_preservation\":{\"local_durable_history_v1\":{\"reasoning_input_contract\":\"reasoning-history-transfer-v1\"}},\"reasoning\":\"textual_reasoning_v1\",\"taxonomy_version\":\"model-capability-taxonomy-v1\",\"tool_exchange\":false},\"ordered_protocol_part_revisions\":[\"parts-v1\"]}' WHERE descriptor_revision_id='kd-1'",
            [],
        )
        .expect("envelope record corrupts");
    assert_eq!(material_fails(), "storage_decode_failed");
    // Restore the kind record and corrupt the profile record instead.
    connection
        .execute(
            "UPDATE provider_kind_descriptor_revisions SET descriptor_json='{\"credential_transport_contract\":\"bearer-or-safe-header\",\"descriptor_family\":\"responses-descriptor\",\"driver_contract_family\":\"responses\",\"endpoint_policy\":\"https-only\",\"kind_id\":\"kind-a\",\"model_capability_envelope\":{\"context_preservation\":{\"local_durable_history_v1\":{\"reasoning_input_contract\":\"reasoning-history-transfer-v1\"}},\"reasoning\":\"textual_reasoning_v1\",\"taxonomy_version\":\"model-capability-taxonomy-v1\",\"text_streaming\":true,\"tool_exchange\":false},\"ordered_protocol_part_revisions\":[\"parts-v1\"]}' WHERE descriptor_revision_id='kd-1'",
            [],
        )
        .expect("kind record restores");
    // A profile record missing the driver contract fails the typed decode.
    connection
        .execute(
            "UPDATE provider_profile_revisions SET profile_revision_json='{\"profile_id\":\"profile-a\"}' WHERE profile_id='profile-a' AND profile_revision_id='rev-a'",
            [],
        )
        .expect("profile record corrupts");
    assert_eq!(material_fails(), "storage_decode_failed");
    // A profile driver contract missing the major revision fails the u64 decode.
    connection
        .execute(
            "UPDATE provider_profile_revisions SET profile_revision_json='{\"capability_taxonomy_revision\":\"model-capability-taxonomy-v1\",\"credential_transport_mode\":\"bearer\",\"driver_contract_revision\":{\"driver_family\":\"responses\",\"minor\":0},\"endpoint\":\"https://api.example.com/v1\",\"kind_descriptor_revision_id\":\"kd-1\",\"model_id\":\"gpt-4.1\",\"profile_id\":\"profile-a\",\"provider_kind_id\":\"responses\",\"revision_id\":\"rev-a\"}' WHERE profile_id='profile-a' AND profile_revision_id='rev-a'",
            [],
        )
        .expect("profile record corrupts");
    assert_eq!(material_fails(), "storage_decode_failed");
    // Restore the profile record and corrupt the projection identity instead.
    connection
        .execute(
            "UPDATE provider_profile_revisions SET profile_revision_json='{\"capability_taxonomy_revision\":\"model-capability-taxonomy-v1\",\"credential_transport_mode\":\"bearer\",\"driver_contract_revision\":{\"driver_family\":\"responses\",\"major\":1,\"minor\":0},\"endpoint\":\"https://api.example.com/v1\",\"kind_descriptor_revision_id\":\"kd-1\",\"model_id\":\"gpt-4.1\",\"profile_id\":\"profile-a\",\"provider_kind_id\":\"responses\",\"reasoning_compatibility_id\":\"reasoning-compat-v1\",\"revision_id\":\"rev-a\"}' WHERE profile_id='profile-a' AND profile_revision_id='rev-a'",
            [],
        )
        .expect("profile record restores");
    connection
        .execute(
            "UPDATE provider_catalog_profile_projection SET profile_revision_id='rev-missing' WHERE projection_state='active' AND profile_id='profile-a'",
            [],
        )
        .expect("projection identity corrupts");
    assert_eq!(material_fails(), "storage_decode_failed");
    connection
        .execute(
            "UPDATE provider_catalog_profile_projection SET profile_revision_id='rev-a', kind_descriptor_revision_id='kd-missing' WHERE projection_state='active' AND profile_id='profile-a'",
            [],
        )
        .expect("projection kind identity corrupts");
    assert_eq!(material_fails(), "storage_decode_failed");
    // A kind record whose kind id disagrees with its row identity fails.
    connection
        .execute(
            "UPDATE provider_kind_descriptor_revisions SET kind_id='kind-mismatch' WHERE descriptor_revision_id='kd-1'",
            [],
        )
        .expect("kind identity corrupts");
    assert_eq!(material_fails(), "storage_decode_failed");
}

#[test]
fn corrupted_durable_state_fails_typed_loads() {
    let (directory, store) = repository();
    let session = create(&store);
    let run = accept_terminal_run(&store, session);
    enqueue(
        &store,
        session,
        run,
        "provider_unavailable",
        "op-enqueue-1",
        3,
    );
    prepare_candidate(
        &store,
        1,
        "op-prep-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    accept_candidate(
        &store,
        1,
        "candidate-1",
        "op-accept-1",
        "kind-a",
        "kd-1",
        "profile-a",
        "rev-a",
        1,
    );
    store
        .record_provider_usage(RecordProviderUsageInputDto {
            session_id: session,
            usage_period_start: 100,
            usage_period_end: 200,
            recorded_at: 3,
            events: vec![usage_event(run, "usage-event-1", 10, 20, 5, 3)],
        })
        .expect("usage records");
    store
        .create_provider_catalog_removal_candidate(CreateProviderCatalogRemovalCandidateInputDto {
            candidate_handle: "removal-1".to_owned(),
            candidate_catalog_revision_id: 2,
            active_catalog_revision_id: 1,
            created_at: 100,
            source_recheck: "health-recheck".to_owned(),
            candidate_json: "{\"safe\":true}".to_owned(),
            operation_id: "op-removal-1".to_owned(),
        })
        .expect("removal candidate creates");
    let connection = raw_connection(&directory);
    // An unknown catalog state status fails the typed status decode, while
    // the closed recovery-required status parses to its typed value.
    connection
        .execute(
            "UPDATE provider_catalog_state SET status='activation_recovery_required' WHERE singleton_id=1",
            [],
        )
        .expect("catalog state corrupts");
    assert_eq!(
        store
            .load_provider_catalog_status()
            .expect("recovery-required status is a closed value")
            .status,
        ProviderCatalogStatusDto::ActivationRecoveryRequired
    );
    connection
        .execute(
            "UPDATE provider_catalog_state SET status='active' WHERE singleton_id=1",
            [],
        )
        .expect("catalog state restores");
    // The closed pending-removal status parses; the schema CHECK constraint
    // makes every other status value impossible.
    connection
        .execute(
            "UPDATE provider_catalog_state SET status='pending_removal' WHERE singleton_id=1",
            [],
        )
        .expect("catalog state corrupts");
    assert_eq!(
        store
            .load_provider_catalog_status()
            .expect("pending-removal status is a closed value")
            .status,
        ProviderCatalogStatusDto::PendingRemoval
    );
    connection
        .execute(
            "UPDATE provider_catalog_state SET status='active' WHERE singleton_id=1",
            [],
        )
        .expect("catalog state restores");
    // A projection readiness value outside the closed set is impossible by
    // the CHECK constraint, so the page decode is exercised over the closed
    // values through the accepted catalog instead.
    // An unparsable run identity in the queue fails the typed row decode.
    connection
        .execute(
            "UPDATE unavailable_provider_queue SET run_id='not-a-run-id' WHERE run_id=?1",
            [run.to_string()],
        )
        .expect("queue identity corrupts");
    let error = store
        .load_unavailable_queue_page(LoadUnavailableQueuePageInputDto {
            after_queue_id: None,
            limit: 10,
        })
        .expect_err("bogus queue identity fails decode");
    assert_eq!(error.code(), "storage_unavailable");
    connection
        .execute("DELETE FROM unavailable_provider_queue", [])
        .expect("queue clears");
    // Every closed removal status decodes when the candidate is read for
    // acceptance; only a pending candidate may be accepted.
    for status in ["accepted", "rejected", "expired"] {
        connection
            .execute(
                "UPDATE provider_catalog_removal_candidates SET status=?1 WHERE candidate_handle='removal-1'",
                [status],
            )
            .expect("removal status corrupts");
        let error = store
            .accept_provider_catalog_removal(AcceptProviderCatalogRemovalInputDto {
                candidate_handle: "removal-1".to_owned(),
                accepted_at: 101,
                operation_id: "op-removal-accept-corrupt".to_owned(),
            })
            .expect_err("non-pending removal status fails");
        assert_eq!(error.code(), "provider_catalog_removal_not_pending");
    }
    connection
        .execute(
            "UPDATE provider_catalog_removal_candidates SET status='pending' WHERE candidate_handle='removal-1'",
            [],
        )
        .expect("removal status restores");
    // An unparsable aggregate run identity fails the typed usage load.
    connection
        .execute(
            "UPDATE provider_usage_aggregates SET last_run_id='not-a-run-id'",
            [],
        )
        .expect("usage aggregate identity corrupts");
    let error = store
        .load_provider_usage_by_profile("profile-a".to_owned())
        .expect_err("bogus aggregate identity fails decode");
    assert_eq!(error.code(), "storage_unavailable");
    let error = store
        .load_provider_usage_by_revision_and_model("rev-a".to_owned(), "model-m".to_owned())
        .expect_err("bogus aggregate identity fails decode on the revision view");
    assert_eq!(error.code(), "storage_unavailable");
}
