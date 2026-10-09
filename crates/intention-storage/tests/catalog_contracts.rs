#![allow(
    clippy::expect_used,
    reason = "Catalog contract fixtures use expect for precise test diagnostics."
)]

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{
    catalog_revision, create_session, empty_catalog_revision, kind_descriptor, open, profile_id,
    profile_policy, profile_revision_with_descriptor, reopen, repository, selection, time,
    transfer,
};
use intention_proto::provider::{
    CatalogRevisionId, CredentialTransportDto, ProviderDiscoveryAttemptId,
    ProviderDiscoveryResultDto, ProviderKindDescriptorRevisionId, ProviderModelRecordDto,
    ProviderProfileId, ProviderProfileRevisionId, ProviderProfileRevisionV1,
    ReasoningHistoryManifestId, ReasoningHistoryRecordReferenceDto, ReasoningHistorySourceEntryDto,
    ResolvedRunProviderSelectionDto,
};
use intention_proto::{
    ConfigRevisionId, ErrorCategoryDto, ErrorRetryDto, FinishReasonDto, IdempotencyKey,
    MessageKindDto, MessageProjectionDto, RunId, RunStatusDto, SchemaVersionDto, SessionId,
    UsageDto,
};
use intention_storage::{
    AcceptedTurnOutcomeDto, ProviderCatalogAuditRecordKindDto, ProviderCatalogRevisionDto,
    ProviderCatalogStateDto, ProviderDiscoveryAttemptDto, ProviderDiscoveryAttemptStateDto,
    ProviderDiscoveryFailureDto, ProviderProfilePolicyEntryDto, RunOutcomeDto,
    SessionProviderProfileDto, SqliteStorageRepository, StorageRepositoryDto,
};
use tempfile::TempDir;

/// Returns the database path of one temporary storage directory.
fn database_path(directory: &TempDir) -> std::path::PathBuf {
    directory.path().join("storage.sqlite")
}

/// Returns the raw committed audit kinds in append order.
fn audit_kinds(directory: &TempDir) -> Vec<String> {
    let connection =
        sqlite::Connection::open(database_path(directory)).expect("database reopens for audit");
    let mut statement = connection
        .prepare("SELECT record_kind FROM provider_catalog_audit ORDER BY id")
        .expect("audit catalogue prepares");
    let kinds = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("audit catalogue reads")
        .map(|row| row.expect("audit kind reads"))
        .collect();
    drop(statement);
    drop(connection);
    kinds
}

/// Returns the raw committed schema object names of one type.
fn schema_names(directory: &TempDir, object_type: &str) -> Vec<String> {
    let connection =
        sqlite::Connection::open(database_path(directory)).expect("database reopens for schema");
    let mut statement = connection
        .prepare("SELECT name FROM sqlite_master WHERE type=?1 ORDER BY name")
        .expect("schema catalogue prepares");
    let names = statement
        .query_map([object_type], |row| row.get::<_, String>(0))
        .expect("schema catalogue reads")
        .map(|row| row.expect("schema name reads"))
        .collect();
    drop(statement);
    drop(connection);
    names
}

/// Returns one raw committed row count for a table.
fn raw_count(directory: &TempDir, table: &str) -> i64 {
    let connection =
        sqlite::Connection::open(database_path(directory)).expect("database reopens for count");
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count query executes")
}

/// Returns the raw committed integer count of rows matching one predicate.
fn raw_count_where(directory: &TempDir, table: &str, predicate: &str) -> i64 {
    let connection =
        sqlite::Connection::open(database_path(directory)).expect("database reopens for count");
    connection
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE {predicate}"),
            [],
            |row| row.get(0),
        )
        .expect("conditional count query executes")
}

/// Returns every non-null text cell of the selected columns in row order.
fn raw_text_columns(directory: &TempDir, table: &str, columns: &[&str]) -> Vec<String> {
    let connection =
        sqlite::Connection::open(database_path(directory)).expect("database reopens for dump");
    let projection = columns.join(", ");
    let mut statement = connection
        .prepare(&format!("SELECT {projection} FROM {table}"))
        .expect("dump query prepares");
    let rows = statement
        .query_map([], |row| {
            let mut values = Vec::new();
            for index in 0..columns.len() {
                values.push(row.get::<_, Option<String>>(index)?);
            }
            Ok(values)
        })
        .expect("dump query reads");
    let mut values = Vec::new();
    for row in rows {
        for value in row.expect("dump row reads").into_iter().flatten() {
            values.push(value);
        }
    }
    values
}

/// Returns the started run identity of an accepted turn outcome.
fn started(outcome: AcceptedTurnOutcomeDto) -> RunId {
    match outcome {
        AcceptedTurnOutcomeDto::Started { run, .. } => run.run_id(),
        AcceptedTurnOutcomeDto::Pending(_) => unreachable!("the turn must start a run"),
    }
}

/// Returns one valid terminal run outcome.
fn outcome(status: RunStatusDto, usage: Option<UsageDto>) -> RunOutcomeDto {
    RunOutcomeDto::new(status, usage, Some(FinishReasonDto::Stop), None, None)
        .expect("fixture outcome is valid")
}

/// Returns one credential-free config snapshot carrying the fixture window.
fn snapshot() -> intention_config::ConfigSnapshotDto {
    intention_config::ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        time(1),
        intention_config::ContextWindowPolicyDto::default_policy(),
    )
    .expect("fixture snapshot is valid")
}

/// Accepts one turn with the supplied resolved selection.
fn accept(
    store: &SqliteStorageRepository,
    session: SessionId,
    run: RunId,
    selection: ResolvedRunProviderSelectionDto,
) -> AcceptedTurnOutcomeDto {
    store
        .accept_user_turn(
            session,
            IdempotencyKey::new(),
            "turn",
            run,
            snapshot(),
            selection,
            None,
            time(2),
        )
        .expect("turn commits")
}

/// Returns one reasoning history manifest referencing one source step.
fn manifest(session: SessionId) -> intention_proto::provider::ReasoningHistoryManifestDto {
    intention_proto::provider::ReasoningHistoryManifestDto::new(
        ReasoningHistoryManifestId::new(),
        transfer(),
        Some("fixture-compatibility-v1".to_owned()),
        vec![ReasoningHistorySourceEntryDto::new(
            session,
            RunId::new(),
            Some(1),
            vec![
                ReasoningHistoryRecordReferenceDto::new(
                    intention_proto::provider::ReasoningFragmentCategoryDto::Primary,
                    5,
                )
                .expect("fixture reference is valid"),
            ],
        )],
        5,
    )
    .expect("fixture manifest is valid")
}

/// Returns one discovery record set whose content exceeds the durable bound.
fn oversized_discovery_records() -> Vec<ProviderModelRecordDto> {
    let chunk = "x".repeat(600 * 1024);
    vec![ProviderModelRecordDto::new(chunk, None).expect("fixture record is valid")]
}

#[test]
fn current_schema_declares_every_control_plane_table_and_index() {
    let directory = TempDir::new().expect("temporary directory exists");
    let store = open(&directory);
    drop(store);

    let tables = schema_names(&directory, "table");
    for expected in [
        "provider_kind_revisions",
        "provider_kind_descriptors",
        "provider_profile_revisions",
        "provider_profile_policies",
        "provider_catalog_revisions",
        "provider_catalog_kinds",
        "provider_catalog_profiles",
        "provider_catalog_state",
        "provider_catalog_audit",
        "provider_profile_tombstones",
        "provider_kind_tombstones",
        "provider_discovery_attempts",
        "provider_discovery_result_records",
        "run_provider_selections",
        "reasoning_history_manifests",
        "reasoning_history_manifest_entries",
        "reasoning_history_bounds",
    ] {
        assert!(
            tables.iter().any(|name| name == expected),
            "the current schema declares {expected}: {tables:?}"
        );
    }
    let indexes = schema_names(&directory, "index");
    for expected in [
        "messages_session_id_id",
        "messages_session_run_id_id",
        "one_active_run_per_session",
        "provider_discovery_attempts_state",
        "run_provider_selections_usage",
    ] {
        assert!(
            indexes.iter().any(|name| name == expected),
            "the current schema declares {expected}: {indexes:?}"
        );
    }
    let connection =
        sqlite::Connection::open(database_path(&directory)).expect("database reopens for stamp");
    let stamp: i32 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("the schema stamp reads");
    assert_eq!(stamp, 4, "the created database carries the current stamp");
}

#[test]
fn catalog_acceptance_activation_and_audit_follow_the_closed_lifecycle() {
    let (directory, store) = repository();
    let catalog_revision_id = CatalogRevisionId::new();
    let state = store
        .accept_catalog_revision(catalog_revision("main", catalog_revision_id))
        .expect("catalog revision accepts");
    assert_eq!(
        state.accepted_catalog_revision_id(),
        Some(catalog_revision_id)
    );
    assert_eq!(state.activated_catalog_revision_id(), None);
    assert!(
        state.requires_activation_recovery(),
        "an accepted revision that is not activated requires recovery"
    );
    assert_eq!(
        audit_kinds(&directory),
        vec![
            ProviderCatalogAuditRecordKindDto::ProviderCatalogCandidatePrepared.as_str(),
            ProviderCatalogAuditRecordKindDto::ProviderCatalogAccepted.as_str(),
        ]
    );

    let activated = store
        .mark_catalog_activated(catalog_revision_id, time(4))
        .expect("the accepted catalog activates");
    assert_eq!(
        activated.activated_catalog_revision_id(),
        Some(catalog_revision_id)
    );
    assert!(!activated.requires_activation_recovery());
    assert_eq!(
        store
            .mark_catalog_activated(catalog_revision_id, time(5))
            .expect("repeating the activation is idempotent"),
        activated
    );
    assert_eq!(
        audit_kinds(&directory),
        vec![
            ProviderCatalogAuditRecordKindDto::ProviderCatalogCandidatePrepared.as_str(),
            ProviderCatalogAuditRecordKindDto::ProviderCatalogAccepted.as_str(),
            ProviderCatalogAuditRecordKindDto::ProviderCatalogActivated.as_str(),
        ],
        "the idempotent activation writes no second audit record"
    );

    let error = store
        .mark_catalog_activated(CatalogRevisionId::new(), time(6))
        .expect_err("only the accepted revision activates");
    assert_eq!(error.code(), "provider_catalog_changed");
    assert_eq!(error.category(), ErrorCategoryDto::Conflict);
    assert_eq!(error.retry(), ErrorRetryDto::Never);
    assert_eq!(
        store.load_catalog_state().expect("state loads"),
        activated,
        "the rejected activation changed nothing"
    );
}

#[test]
fn catalog_revision_load_returns_membership_and_current_policy() {
    let (_directory, store) = repository();
    let revision = catalog_revision("main", CatalogRevisionId::new());
    let catalog_revision_id = revision.catalog_revision_id();
    store
        .accept_catalog_revision(revision.clone())
        .expect("catalog revision accepts");
    let loaded = store
        .load_catalog_revision(catalog_revision_id)
        .expect("the committed revision loads");
    assert_eq!(loaded, revision);
    assert_eq!(
        loaded.default_profile_id().map(ProviderProfileId::as_str),
        Some("main")
    );
    assert_eq!(
        store
            .load_catalog_revision(CatalogRevisionId::new())
            .expect_err("an unknown revision is not found")
            .code(),
        "provider_catalog_revision_not_found"
    );

    // Current policy is not revision identity: the loaded revision reports the
    // new display name while the revision identity stays the same.
    store
        .store_profile_policy(
            profile_id("main"),
            profile_policy("Renamed Profile", false),
            time(7),
        )
        .expect("the member policy stores");
    let reloaded = store
        .load_catalog_revision(catalog_revision_id)
        .expect("the committed revision still loads");
    assert_eq!(
        reloaded.profile_policies()[0].policy().display_name(),
        "Renamed Profile"
    );
    assert!(!reloaded.profile_policies()[0].policy().enabled());
    assert_eq!(
        reloaded.profile_revisions()[0].revision_id(),
        revision.profile_revisions()[0].revision_id(),
        "a policy change never changes the profile revision"
    );
    assert_eq!(
        store
            .store_profile_policy(
                profile_id("absent"),
                profile_policy("Absent", true),
                time(8)
            )
            .expect_err("a non-member profile has no policy")
            .code(),
        "provider_profile_not_found"
    );
}

#[test]
fn kind_and_profile_revision_history_is_immutable() {
    let (_directory, store) = repository();
    let first = catalog_revision("main", CatalogRevisionId::new());
    let descriptor_revision_id = first.kind_descriptors()[0].descriptor_revision_id();
    store
        .accept_catalog_revision(first)
        .expect("the first catalog revision accepts");

    // The same kind identity with a changed declaration fails closed.
    let changed = ProviderCatalogRevisionDto::new(
        CatalogRevisionId::new(),
        Some(profile_id("main")),
        time(2),
        vec![kind_descriptor(descriptor_revision_id, "changed-family")],
        vec![profile_revision_with_descriptor(
            "main",
            descriptor_revision_id,
        )],
        vec![ProviderProfilePolicyEntryDto::new(
            profile_id("main"),
            profile_policy("Fixture Profile", true),
        )],
    )
    .expect("the changed candidate is coherent");
    let error = store
        .accept_catalog_revision(changed)
        .expect_err("a kind declaration is immutable");
    assert_eq!(error.code(), "provider_kind_immutable_mismatch");
    assert_eq!(error.category(), ErrorCategoryDto::Conflict);

    // A different descriptor identity with the same semantic declaration is
    // accepted as a new revision alias.
    let aliased_descriptor =
        kind_descriptor(ProviderKindDescriptorRevisionId::new(), "fixture-family");
    let aliased_descriptor_id = aliased_descriptor.descriptor_revision_id();
    let aliased = ProviderCatalogRevisionDto::new(
        CatalogRevisionId::new(),
        Some(profile_id("main")),
        time(2),
        vec![aliased_descriptor],
        vec![profile_revision_with_descriptor(
            "main",
            aliased_descriptor_id,
        )],
        vec![ProviderProfilePolicyEntryDto::new(
            profile_id("main"),
            profile_policy("Fixture Profile", true),
        )],
    )
    .expect("the aliased candidate is coherent");
    store
        .accept_catalog_revision(aliased)
        .expect("an equal declaration under a new revision identity accepts");
}

#[test]
fn a_bound_profile_revision_never_changes_its_meaning() {
    let (_directory, store) = repository();
    let first = catalog_revision("main", CatalogRevisionId::new());
    let profile = first.profile_revisions()[0].clone();
    let profile_revision_id: ProviderProfileRevisionId = profile.revision_id();
    store
        .accept_catalog_revision(first)
        .expect("the first catalog revision accepts");

    let changed = ProviderProfileRevisionV1::new(
        profile_id("main"),
        profile_revision_id,
        profile.kind_id().clone(),
        profile.kind_descriptor_revision_id(),
        "changed-model",
        None,
        CredentialTransportDto::bearer(),
        profile.declared_model_capability_subset().clone(),
        profile.resolved_reasoning_policy().clone(),
        profile.effective_execution_policy(),
        profile.effective_loopback_policy(),
    )
    .expect("the changed profile revision is coherent");
    let candidate = ProviderCatalogRevisionDto::new(
        CatalogRevisionId::new(),
        Some(profile_id("main")),
        time(2),
        vec![kind_descriptor(
            profile.kind_descriptor_revision_id(),
            "fixture-family",
        )],
        vec![changed],
        vec![ProviderProfilePolicyEntryDto::new(
            profile_id("main"),
            profile_policy("Fixture Profile", true),
        )],
    )
    .expect("the changed candidate is coherent");
    let error = store
        .accept_catalog_revision(candidate)
        .expect_err("a bound profile revision keeps its meaning");
    assert_eq!(error.code(), "provider_profile_revision_mismatch");
    assert_eq!(error.category(), ErrorCategoryDto::Conflict);
    assert_eq!(error.retry(), ErrorRetryDto::Never);
}

#[test]
fn removal_acceptance_tombstones_omitted_members() {
    let (directory, store) = repository();
    let first = CatalogRevisionId::new();
    store
        .accept_catalog_revision(catalog_revision("main", first))
        .expect("the first catalog revision accepts");
    store
        .mark_catalog_activated(first, time(4))
        .expect("the first catalog activates");
    let session = create_session(&store, "removal-default");
    store
        .set_session_provider_profile(session, profile_id("main"), 0, time(4))
        .expect("the session default binds the member profile");

    let second = CatalogRevisionId::new();
    let state = store
        .accept_catalog_revision(empty_catalog_revision(second))
        .expect("the removing catalog revision accepts");
    assert_eq!(state.accepted_catalog_revision_id(), Some(second));
    assert_eq!(
        state.activated_catalog_revision_id(),
        Some(first),
        "acceptance alone never activates"
    );
    // A catalog change never cascades into a session's durable default.
    let session_profile = store
        .load_session_provider_profile(session)
        .expect("the durable session default loads");
    assert_eq!(
        session_profile
            .provider_profile_id()
            .map(ProviderProfileId::as_str),
        Some("main")
    );
    assert_eq!(session_profile.session_projection_revision(), 1);
    assert_eq!(raw_count(&directory, "provider_profile_tombstones"), 1);
    assert_eq!(raw_count(&directory, "provider_kind_tombstones"), 1);
    assert_eq!(
        audit_kinds(&directory)[3..],
        vec![
            ProviderCatalogAuditRecordKindDto::ProviderCatalogCandidatePrepared.as_str(),
            ProviderCatalogAuditRecordKindDto::ProviderCatalogRemovalPending.as_str(),
            ProviderCatalogAuditRecordKindDto::ProviderCatalogRemovalAccepted.as_str(),
            ProviderCatalogAuditRecordKindDto::ProviderCatalogAccepted.as_str(),
        ]
    );

    // A reintroduced identifier is admitted again, and its next removal
    // records a fresh append-only history row.
    let reintroduced = catalog_revision("main", CatalogRevisionId::new());
    store
        .accept_catalog_revision(reintroduced.clone())
        .expect("a reintroduced profile is admitted again");
    assert_eq!(
        store
            .load_catalog_revision(reintroduced.catalog_revision_id())
            .expect("the reintroduced revision loads"),
        reintroduced
    );
    store
        .accept_catalog_revision(empty_catalog_revision(CatalogRevisionId::new()))
        .expect("the second removal accepts");
    assert_eq!(
        raw_count(&directory, "provider_profile_tombstones"),
        2,
        "each removal records its own history row"
    );
}

#[test]
fn rejected_candidate_records_rejection_audit_without_acceptance() {
    let (directory, store) = repository();
    store
        .record_catalog_candidate_rejected(CatalogRevisionId::new(), time(3))
        .expect("the rejection records");
    assert_eq!(
        audit_kinds(&directory),
        vec![
            ProviderCatalogAuditRecordKindDto::ProviderCatalogRemovalPending.as_str(),
            ProviderCatalogAuditRecordKindDto::ProviderCatalogCandidateRejected.as_str(),
        ]
    );
    assert_eq!(
        store.load_catalog_state().expect("state loads"),
        ProviderCatalogStateDto::new(None, None).expect("the empty state is coherent")
    );
}

#[test]
fn activation_recovery_records_the_closed_recovery_taxonomy() {
    let (directory, store) = repository();
    let first = CatalogRevisionId::new();
    store
        .accept_catalog_revision(catalog_revision("main", first))
        .expect("the first catalog revision accepts");
    store
        .mark_catalog_activated(first, time(4))
        .expect("the first catalog activates");
    drop(store);

    // A crash after acceptance and before activation leaves the accepted
    // revision unactivated; the reopened repository observes exactly that.
    let reopened = reopen(&directory);
    let second = CatalogRevisionId::new();
    reopened
        .accept_catalog_revision(catalog_revision("main", second))
        .expect("the second catalog revision accepts");
    assert!(
        reopened
            .load_catalog_state()
            .expect("state loads")
            .requires_activation_recovery()
    );

    let recovered = reopened
        .mark_catalog_activated(second, time(9))
        .expect("the exact accepted revision recovers");
    assert_eq!(recovered.activated_catalog_revision_id(), Some(second));
    assert_eq!(
        audit_kinds(&directory)[3..],
        vec![
            ProviderCatalogAuditRecordKindDto::ProviderCatalogCandidatePrepared.as_str(),
            ProviderCatalogAuditRecordKindDto::ProviderCatalogAccepted.as_str(),
            ProviderCatalogAuditRecordKindDto::ProviderCatalogActivationRecoveryRequired.as_str(),
            ProviderCatalogAuditRecordKindDto::ProviderCatalogActivated.as_str(),
            ProviderCatalogAuditRecordKindDto::ProviderCatalogRecoveryCompleted.as_str(),
        ]
    );
    drop(reopened);
    let final_state = reopen(&directory)
        .load_catalog_state()
        .expect("state loads after reopen");
    assert_eq!(final_state.accepted_catalog_revision_id(), Some(second));
    assert_eq!(final_state.activated_catalog_revision_id(), Some(second));
    assert!(!final_state.requires_activation_recovery());
}

#[test]
fn session_provider_profile_is_optimistic_and_idempotent() {
    let (_directory, store) = repository();
    let session = create_session(&store, "session-profile");
    assert_eq!(
        store
            .load_session_provider_profile(session)
            .expect("the durable session default loads"),
        SessionProviderProfileDto::new(session, None, 0)
    );

    let changed = store
        .set_session_provider_profile(session, profile_id("main"), 0, time(4))
        .expect("the session default changes");
    assert!(changed.changed());
    assert_eq!(changed.session_projection_revision(), 1);

    let unchanged = store
        .set_session_provider_profile(session, profile_id("main"), 1, time(5))
        .expect("setting the same profile succeeds without a change");
    assert!(!unchanged.changed());
    assert_eq!(unchanged.session_projection_revision(), 1);

    let conflict = store
        .set_session_provider_profile(session, profile_id("other"), 0, time(6))
        .expect_err("a stale expected revision conflicts");
    assert_eq!(conflict.code(), "session_projection_revision_conflict");
    assert_eq!(conflict.category(), ErrorCategoryDto::Conflict);
    assert_eq!(conflict.retry(), ErrorRetryDto::Never);
    assert_eq!(
        store
            .load_session_provider_profile(session)
            .expect("the conflicted change wrote nothing")
            .provider_profile_id()
            .map(ProviderProfileId::as_str),
        Some("main")
    );

    let second_change = store
        .set_session_provider_profile(session, profile_id("other"), 1, time(7))
        .expect("the next change uses the current revision");
    assert!(second_change.changed());
    assert_eq!(second_change.session_projection_revision(), 2);

    assert_eq!(
        store
            .load_session_provider_profile(SessionId::new())
            .expect_err("an unknown session is hidden")
            .code(),
        "storage_record_not_found"
    );
}

#[test]
fn session_projection_reports_the_durable_default_and_revision() {
    let (_directory, store) = repository();
    let session = create_session(&store, "session-projection");
    store
        .set_session_provider_profile(session, profile_id("main"), 0, time(4))
        .expect("the session default changes");
    let projection = store
        .load_session_projection(session)
        .expect("the session projection loads");
    assert_eq!(
        projection
            .provider_profile_id()
            .map(ProviderProfileId::as_str),
        Some("main")
    );
    assert_eq!(projection.session_projection_revision(), 1);
}

#[test]
fn run_provider_selection_round_trips_and_stays_credential_free() {
    let (directory, store) = repository();
    let session = create_session(&store, "run-selection");
    let run = RunId::new();
    let resolved = selection("main");
    assert_eq!(started(accept(&store, session, run, resolved.clone())), run);

    assert_eq!(
        store
            .load_run_provider_selection(session, run)
            .expect("the committed selection loads"),
        resolved
    );
    assert_eq!(
        store
            .load_session_projection(session)
            .expect("the session projection loads")
            .active_run()
            .expect("the accepted run is active")
            .provider_profile_id()
            .map(ProviderProfileId::as_str),
        Some("main")
    );
    assert_eq!(
        store
            .load_run_projection(session, run)
            .expect("the run projection loads")
            .provider_profile_id()
            .map(ProviderProfileId::as_str),
        Some("main")
    );

    // The selection row is credential-free: the config snapshot's recognizable
    // credential literal never reaches it.
    let values = raw_text_columns(
        &directory,
        "run_provider_selections",
        &[
            "run_id",
            "session_id",
            "profile_id",
            "provider_profile_revision_id",
            "kind_id",
            "kind_descriptor_revision_id",
            "model_id",
            "normalized_effective_endpoint",
            "credential_transport_mode",
            "credential_transport_safe_header_name",
            "declared_model_capability_subset",
            "resolved_reasoning_policy",
            "effective_execution_policy",
            "effective_loopback_policy",
            "provider_driver_contract_revision",
            "selection_source",
        ],
    );
    assert!(!values.is_empty());
    assert!(
        values
            .iter()
            .all(|value| !value.contains("recognizable-fixture-credential"))
    );

    assert_eq!(
        store
            .load_run_provider_selection(session, RunId::new())
            .expect_err("an unknown run has no selection")
            .code(),
        "run_provider_selection_not_found"
    );
    assert_eq!(
        store
            .load_run_provider_selection(SessionId::new(), run)
            .expect_err("a cross-session run has no selection")
            .code(),
        "run_provider_selection_not_found"
    );
}

#[test]
fn promoted_pending_turn_keeps_its_own_committed_selection() {
    let (_directory, store) = repository();
    let session = create_session(&store, "pending-selection");
    let active_run = started(accept(&store, session, RunId::new(), selection("main")));
    let queued_run = RunId::new();
    let queued_selection = selection("other");
    assert!(matches!(
        accept(&store, session, queued_run, queued_selection.clone()),
        AcceptedTurnOutcomeDto::Pending(_)
    ));
    assert_eq!(
        store
            .load_run_provider_selection(session, queued_run)
            .expect("the queued turn's selection is already durable"),
        queued_selection
    );

    store
        .transition_run(session, active_run, RunStatusDto::Failed, time(3))
        .expect("the active run ends");
    let promoted_run = started(accept(&store, session, RunId::new(), selection("third")));
    assert_eq!(
        promoted_run, queued_run,
        "the oldest pending turn is promoted"
    );
    assert_eq!(
        store
            .load_run_provider_selection(session, promoted_run)
            .expect("the promoted run loads its own selection"),
        queued_selection,
        "the promoted run keeps the selection committed with its own turn"
    );
}

#[test]
fn discovery_attempt_follows_before_start_started_terminal_states() {
    let (_directory, store) = repository();
    let attempt_id = ProviderDiscoveryAttemptId::new();
    let prepared = store
        .begin_discovery_attempt(attempt_id, profile_id("main"), time(2))
        .expect("the attempt begins before start");
    assert_eq!(prepared.state(), ProviderDiscoveryAttemptStateDto::Prepared);
    assert_eq!(prepared.started_at(), None);
    assert_eq!(prepared.terminated_at(), None);
    assert_eq!(
        store
            .begin_discovery_attempt(attempt_id, profile_id("main"), time(2))
            .expect_err("a discovery attempt identity is used once")
            .code(),
        "discovery_attempt_conflict"
    );
    assert_eq!(
        store
            .complete_discovery_attempt(attempt_id, Vec::new(), time(3))
            .expect_err("a before-start attempt cannot complete")
            .code(),
        "discovery_attempt_state_conflict"
    );

    let started = store
        .mark_discovery_started(attempt_id, time(3))
        .expect("the attempt starts");
    assert_eq!(started.state(), ProviderDiscoveryAttemptStateDto::Started);
    assert_eq!(started.started_at(), Some(time(3)));
    assert_eq!(
        store
            .mark_discovery_started(attempt_id, time(4))
            .expect_err("a started attempt cannot start twice")
            .code(),
        "discovery_attempt_state_conflict"
    );

    let records = vec![
        ProviderModelRecordDto::new("fixture-model-a", Some("Model A".to_owned()))
            .expect("fixture record is valid"),
        ProviderModelRecordDto::new("fixture-model-b", None).expect("fixture record is valid"),
    ];
    let result = store
        .complete_discovery_attempt(attempt_id, records.clone(), time(4))
        .expect("the attempt completes");
    assert_eq!(
        result,
        ProviderDiscoveryResultDto::new(attempt_id, records).expect("fixture result is valid")
    );
    assert_eq!(
        store
            .fail_discovery_attempt(
                attempt_id,
                ProviderDiscoveryFailureDto::new("provider_unavailable", "safe failure")
                    .expect("fixture failure is valid"),
                time(5),
            )
            .expect_err("a terminal attempt accepts no failure")
            .code(),
        "discovery_attempt_state_conflict"
    );

    let failed_id = ProviderDiscoveryAttemptId::new();
    store
        .begin_discovery_attempt(failed_id, profile_id("main"), time(5))
        .expect("the second attempt begins");
    let failure = ProviderDiscoveryFailureDto::new("provider_unauthorized", "safe failure")
        .expect("fixture failure is valid");
    let failed = store
        .fail_discovery_attempt(failed_id, failure.clone(), time(6))
        .expect("a before-start failure stays known");
    assert_eq!(failed.state(), ProviderDiscoveryAttemptStateDto::Failed);
    assert_eq!(failed.started_at(), None);
    assert_eq!(failed.terminated_at(), Some(time(6)));
    assert_eq!(failed.failure(), Some(&failure));

    assert_eq!(
        store
            .mark_discovery_started(ProviderDiscoveryAttemptId::new(), time(7))
            .expect_err("an unknown attempt is not found")
            .code(),
        "discovery_attempt_not_found"
    );
}

#[test]
fn discovery_attempts_terminalize_on_open_and_on_explicit_recovery() {
    let (directory, store) = repository();
    let prepared_id = ProviderDiscoveryAttemptId::new();
    store
        .begin_discovery_attempt(prepared_id, profile_id("main"), time(2))
        .expect("the before-start attempt begins");
    let started_id = ProviderDiscoveryAttemptId::new();
    store
        .begin_discovery_attempt(started_id, profile_id("main"), time(2))
        .expect("the started attempt begins");
    store
        .mark_discovery_started(started_id, time(3))
        .expect("the attempt starts");
    let completed_id = ProviderDiscoveryAttemptId::new();
    store
        .begin_discovery_attempt(completed_id, profile_id("main"), time(2))
        .expect("the completing attempt begins");
    store
        .mark_discovery_started(completed_id, time(3))
        .expect("the completing attempt starts");
    store
        .complete_discovery_attempt(completed_id, Vec::new(), time(4))
        .expect("the completing attempt completes");

    // Opening terminalizes each unfinished attempt at its own last known
    // evidence time and leaves terminal attempts untouched.
    drop(store);
    let reopened = reopen(&directory);
    assert_eq!(
        raw_count_where(
            &directory,
            "provider_discovery_attempts",
            &format!("id='{prepared_id}' AND state='interrupted' AND terminated_at=2"),
        ),
        1,
        "a before-start attempt is terminalized at its prepared time"
    );
    assert_eq!(
        raw_count_where(
            &directory,
            "provider_discovery_attempts",
            &format!("id='{started_id}' AND state='interrupted' AND terminated_at=3"),
        ),
        1,
        "a started attempt is terminalized at its started time"
    );
    assert_eq!(
        raw_count_where(
            &directory,
            "provider_discovery_attempts",
            &format!("id='{completed_id}' AND state='completed'"),
        ),
        1,
        "a terminal attempt stays terminal"
    );
    assert_eq!(
        reopened
            .recover_unfinished_discovery_attempts(time(9))
            .expect("recovery with nothing left to terminalize commits")
            .len(),
        0
    );

    // The explicit recovery records the caller-supplied recovery time.
    let third = ProviderDiscoveryAttemptId::new();
    reopened
        .begin_discovery_attempt(third, profile_id("main"), time(10))
        .expect("the third attempt begins");
    let recovered = reopened
        .recover_unfinished_discovery_attempts(time(11))
        .expect("recovery terminalizes the unfinished attempt");
    assert_eq!(recovered.len(), 1);
    let recovered: &ProviderDiscoveryAttemptDto = &recovered[0];
    assert_eq!(recovered.attempt_id(), third);
    assert_eq!(
        recovered.state(),
        ProviderDiscoveryAttemptStateDto::Interrupted
    );
    assert_eq!(recovered.terminated_at(), Some(time(11)));
}

#[test]
fn discovery_result_bound_rejects_an_oversized_record_set() {
    let (_directory, store) = repository();
    let attempt_id = ProviderDiscoveryAttemptId::new();
    store
        .begin_discovery_attempt(attempt_id, profile_id("main"), time(2))
        .expect("the attempt begins");
    store
        .mark_discovery_started(attempt_id, time(3))
        .expect("the attempt starts");
    let error = store
        .complete_discovery_attempt(attempt_id, oversized_discovery_records(), time(4))
        .expect_err("an over-bound result set rejects whole");
    assert_eq!(error.code(), "discovery_result_too_large");
    assert_eq!(error.category(), ErrorCategoryDto::Validation);
    assert_eq!(error.retry(), ErrorRetryDto::Manual);
    // The attempt stays unfinished and records no partial result row.
    let reopened_attempt = store
        .fail_discovery_attempt(
            attempt_id,
            ProviderDiscoveryFailureDto::new("provider_unavailable", "safe failure")
                .expect("fixture failure is valid"),
            time(5),
        )
        .expect("the rejected attempt still fails normally");
    assert_eq!(
        reopened_attempt.state(),
        ProviderDiscoveryAttemptStateDto::Failed
    );
}

#[test]
fn profile_usage_aggregates_per_revision_and_model_without_price() {
    let (directory, store) = repository();
    let session = create_session(&store, "usage");
    let resolved = selection("main");

    let reported_run = started(accept(&store, session, RunId::new(), resolved.clone()));
    store
        .transition_run(session, reported_run, RunStatusDto::Running, time(3))
        .expect("the reported run starts");
    store
        .finish_run(
            session,
            reported_run,
            outcome(
                RunStatusDto::Completed,
                Some(UsageDto::reported(10, 5, 15).expect("fixture usage is consistent")),
            ),
            time(4),
        )
        .expect("the reported run completes");

    let unreported_run = started(accept(&store, session, RunId::new(), resolved));
    store
        .transition_run(session, unreported_run, RunStatusDto::Running, time(5))
        .expect("the unreported run starts");
    store
        .finish_run(
            session,
            unreported_run,
            outcome(RunStatusDto::Failed, Some(UsageDto::NotReported)),
            time(6),
        )
        .expect("the unreported run completes");

    let aggregates = store
        .load_profile_usage(profile_id("main"))
        .expect("the profile usage aggregates load");
    assert_eq!(aggregates.len(), 1, "one bounded aggregate per identity");
    let aggregate = &aggregates[0];
    assert_eq!(aggregate.reported_runs(), 1);
    assert_eq!(aggregate.unreported_runs(), 1);
    assert_eq!(aggregate.input_tokens(), 10);
    assert_eq!(aggregate.output_tokens(), 5);
    assert_eq!(aggregate.total_tokens(), 15);
    assert_eq!(aggregate.model_id(), "fixture-model-main");
    // No price, currency, or estimated cost exists anywhere in the row.
    let values = raw_text_columns(&directory, "run_provider_selections", &["selection_source"]);
    assert!(
        values
            .iter()
            .all(|value| !value.contains("price") && !value.contains("cost"))
    );
    assert!(
        store
            .load_profile_usage(profile_id("absent"))
            .expect("an unused profile has no aggregates")
            .is_empty()
    );
}

#[test]
fn reasoning_history_commits_with_the_run_and_source_read_is_completed_steps_only() {
    let (directory, store) = repository();
    let session = create_session(&store, "reasoning");
    let run = started(accept(&store, session, RunId::new(), selection("main")));
    store
        .append_message(
            MessageProjectionDto::new(
                session,
                Some(run),
                MessageKindDto::Assistant,
                "answer",
                Some("whole reasoning".to_owned()),
                None,
                None,
            )
            .expect("fixture assistant row is valid"),
            time(3),
        )
        .expect("the assistant step commits");
    store
        .transition_run(session, run, RunStatusDto::Running, time(4))
        .expect("the run starts");
    store
        .finish_run(
            session,
            run,
            outcome(RunStatusDto::Completed, Some(UsageDto::NotReported)),
            time(5),
        )
        .expect("the run completes");

    // An assistant step of a failed run is not a completed source.
    let failed_run = started(accept(&store, session, RunId::new(), selection("main")));
    store
        .append_message(
            MessageProjectionDto::new(
                session,
                Some(failed_run),
                MessageKindDto::Assistant,
                "partial",
                Some("partial reasoning".to_owned()),
                None,
                None,
            )
            .expect("fixture assistant row is valid"),
            time(6),
        )
        .expect("the failed step commits");
    store
        .transition_run(session, failed_run, RunStatusDto::Running, time(7))
        .expect("the failed run starts");
    store
        .finish_run(
            session,
            failed_run,
            outcome(RunStatusDto::Failed, Some(UsageDto::NotReported)),
            time(8),
        )
        .expect("the failed run completes");

    let source = store
        .load_reasoning_history_source(session)
        .expect("the committed source loads");
    assert_eq!(source.len(), 1);
    assert_eq!(source[0].run_id(), run);
    assert_eq!(source[0].reasoning(), Some("whole reasoning"));
    assert_eq!(
        store
            .load_reasoning_history_source(SessionId::new())
            .expect_err("an unknown session has no source")
            .code(),
        "storage_record_not_found"
    );

    // A manifest commits with the run start, its entries, and its bound record.
    let manifest_session = create_session(&store, "manifest");
    let manifest = manifest(manifest_session);
    store
        .accept_user_turn(
            manifest_session,
            IdempotencyKey::new(),
            "turn",
            RunId::new(),
            snapshot(),
            selection("main"),
            Some(manifest),
            time(9),
        )
        .expect("the dependent turn commits with its manifest");
    assert_eq!(raw_count(&directory, "reasoning_history_manifests"), 1);
    assert_eq!(
        raw_count(&directory, "reasoning_history_manifest_entries"),
        1
    );
    assert_eq!(raw_count(&directory, "reasoning_history_bounds"), 1);
    assert_eq!(
        raw_count_where(
            &directory,
            "reasoning_history_bounds",
            "source_entry_count=1 AND aggregate_size_bytes=5"
        ),
        1,
        "the bound audit record carries only the closed counts"
    );
}

#[test]
fn reasoning_history_over_the_fixed_bound_is_rejected() {
    let (directory, store) = repository();
    let session = create_session(&store, "over-bound");
    let oversized = intention_proto::provider::ReasoningHistoryManifestDto::new(
        ReasoningHistoryManifestId::new(),
        transfer(),
        Some("fixture-compatibility-v1".to_owned()),
        Vec::new(),
        4 * 1024 * 1024 + 1,
    )
    .expect("the oversized manifest is structurally valid");
    let error = store
        .accept_user_turn(
            session,
            IdempotencyKey::new(),
            "turn",
            RunId::new(),
            snapshot(),
            selection("main"),
            Some(oversized),
            time(2),
        )
        .expect_err("an over-bound history rejects whole");
    assert_eq!(error.code(), "reasoning_history_too_large");
    assert_eq!(error.category(), ErrorCategoryDto::Validation);
    assert_eq!(error.retry(), ErrorRetryDto::Manual);
    assert_eq!(raw_count(&directory, "turns"), 0, "nothing was committed");
    assert_eq!(raw_count(&directory, "reasoning_history_manifests"), 0);
}
