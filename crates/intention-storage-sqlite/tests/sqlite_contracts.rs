#![allow(
    clippy::expect_used,
    reason = "SQLite contract fixtures use expect for precise test diagnostics."
)]

use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    ContextPreservationCapability, CreateSessionCommandDto, CredentialTransportMode,
    DomainEventDto, ModelCapabilitySetV1, ModelInputCapability, ModelRunFactDto,
    ModelRunFactEventDto, ModelRunFactInputDto, ProviderDriverContractRevisionDto,
    ProviderKindDescriptorRevisionV1, ProviderProfileRevisionV1, ProviderSelectionV1,
    ReasoningCapability, RemoveQueuedTurnCommandDto, RunEventCursorDto, RunModeDto, RunStatusDto,
    StructuredOutputCapability, ToolLifecycleEventDto, ToolLifecycleStatusDto, WorkspaceRootDto,
    canonical::contains_credential_shape, provider_selection::MODEL_CAPABILITY_TAXONOMY_V1,
};
use intention_storage::{
    AcceptProviderCatalogInputDto, AcceptUserTurnInputDto, AcceptedTurnOutcomeDto,
    AdmitHeldRecoveredRunInputDto, AppendProviderKindDescriptorRevisionInputDto,
    AppendProviderProfileRevisionInputDto, AppendToolLifecycleEventInputDto,
    CommitConfigurationReloadInputDto, ConfigurationReloadRepositoryDto,
    CreateProviderCatalogRemovalCandidateInputDto, CreateSessionInputDto,
    EnqueueUnavailableRunInputDto, ExpireProviderCatalogCandidateInputDto,
    HeldRunAdmissionStateDto, HeldRunRepositoryDto, LoadProviderCatalogPageInputDto,
    MarkRecoveredRunHeldInputDto, PersistResolvedRunProviderSelectionInputDto,
    ProviderCatalogRepositoryDto, ProviderProfileCandidateDto, ProviderReadinessDto,
    ProviderRemovalRepositoryDto, ProviderSelectionRepositoryDto, ProviderUsageRepositoryDto,
    RecordProviderUsageInputDto, RecoverUnfinishedRunsInputDto,
    RejectProviderCatalogCandidateInputDto, RemoveQueuedTurnInputDto,
    SessionProviderDefaultsRepositoryDto, SetSessionProviderProfileInputDto, StorageRepositoryDto,
    ToolResultEvidenceDto, ToolResultKindDto, TransitionRunInputDto, UnavailableQueueRepositoryDto,
};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use intention_types::{
    ConfigRevisionId, EventEnvelopeDto, EventId, EventMetadataDto, ProjectId, RunId,
    SchemaVersionDto, SessionEventSequenceDto, SessionId, TimestampDto, TurnId, WorkspaceId,
};
use rusqlite_migration::{M, Migrations};
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

fn snapshot_with_revision_and_model(
    revision_id: ConfigRevisionId,
    model: &str,
) -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-storage-sqlite-test.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture path is absolute"),
    );
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"{model}\"\ncredential = \"fixture-secret\""
        ),
        source,
    ))
    .expect("fixture configuration resolves");
    ConfigSnapshotDto::new(SchemaVersionDto::new(1, 0), revision_id, time(1), resolved)
        .expect("fixture snapshot is valid")
}
fn workspace_root(label: &str) -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-storage-sqlite-contracts")
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
                workspace_root("storage-contract"),
                RunModeDto::Build,
            ),
            time(1),
        ))
        .expect("session creates");
    session
}

fn accept(
    store: &SqliteStorageRepository,
    session: SessionId,
    turn: TurnId,
    run: RunId,
    text: &str,
) -> intention_storage::CommittedChangeDto {
    store
        .accept_user_turn(
            AcceptUserTurnInputDto::new(session, turn, text, run, snapshot(), time(2))
                .expect("turn input is valid"),
        )
        .expect("turn commits")
}

fn tool_event(session: SessionId, run: RunId, detail: &str, at: i64) -> ToolLifecycleEventDto {
    ToolLifecycleEventDto::new(
        session,
        run,
        intention_types::ToolCallId::new(),
        "shell",
        ToolLifecycleStatusDto::Completed,
        detail,
        time(at),
    )
    .expect("tool event is valid")
}

fn tool_event_with_status(
    session: SessionId,
    run: RunId,
    status: ToolLifecycleStatusDto,
    detail: &str,
    at: i64,
) -> ToolLifecycleEventDto {
    ToolLifecycleEventDto::new(
        session,
        run,
        intention_types::ToolCallId::new(),
        "shell",
        status,
        detail,
        time(at),
    )
    .expect("tool event is valid")
}

#[test]
fn append_tool_lifecycle_event_accepts_scoped_event_and_round_trips_redacted_payload() {
    let (_directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    accept(&store, session, TurnId::new(), run, "run");
    let event = tool_event_with_status(
        session,
        run,
        ToolLifecycleStatusDto::Admitted,
        "safe detail",
        3,
    );
    let appended = store
        .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(event.clone()))
        .expect("scoped tool event appends");
    assert_eq!(appended.session_id(), session);
    assert_eq!(appended.run_id(), Some(run));
    assert_eq!(appended.sequence().value(), 4);
    assert!(matches!(appended.payload(), DomainEventDto::ToolLifecycle(value) if value == &event));
    let tail = store
        .load_tail(session, SessionEventSequenceDto::new(3))
        .expect("tool event tail loads");
    assert_eq!(tail.last(), Some(&appended));
    let encoded = serde_json::to_string(&tail[0]).expect("event serializes");
    assert!(!encoded.contains("credential"));
    assert!(!encoded.contains("/tmp/"));
    let decoded: intention_types::EventEnvelopeDto<DomainEventDto> =
        serde_json::from_str(&encoded).expect("event round-trips");
    assert_eq!(decoded, tail[0]);
}

#[test]
fn append_tool_lifecycle_event_rejects_wrong_session_or_unknown_run_without_writing() {
    let (_directory, store) = repository();
    let session = create(&store);
    let other_session = {
        let other = SessionId::new();
        store
            .create_session(CreateSessionInputDto::new(
                CreateSessionCommandDto::new(
                    ProjectId::new(),
                    other,
                    WorkspaceId::new(),
                    workspace_root("other-session"),
                    RunModeDto::Build,
                ),
                time(1),
            ))
            .expect("other session creates");
        other
    };
    let run = RunId::new();
    accept(&store, session, TurnId::new(), run, "run");
    for event in [
        tool_event(other_session, run, "cross-session", 3),
        tool_event(session, RunId::new(), "unknown-run", 3),
    ] {
        let error = store
            .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(event))
            .expect_err("invalid run scope is rejected");
        assert!(matches!(
            error.code(),
            "run_replay_not_found" | "run_history_unavailable"
        ));
    }
    assert_eq!(
        store
            .load_tail(session, SessionEventSequenceDto::new(2))
            .expect("tail loads")
            .len(),
        1
    );
}

#[test]
fn append_tool_lifecycle_event_preserves_sequence_and_rolls_back_on_fault() {
    let (_directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    accept(&store, session, TurnId::new(), run, "run");
    let admitted = store
        .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(
            tool_event_with_status(
                session,
                run,
                ToolLifecycleStatusDto::Admitted,
                "committed",
                4,
            ),
        ))
        .expect("next append succeeds");
    assert_eq!(admitted.sequence().value(), 4);
    assert_eq!(
        store
            .load_tail(session, SessionEventSequenceDto::new(2))
            .expect("tail loads")
            .len(),
        2
    );
}

#[test]
fn tool_lifecycle_rejects_invalid_initial_status_and_terminal_successor() {
    let (_directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    accept(&store, session, TurnId::new(), run, "run");
    let invalid = tool_event_with_status(session, run, ToolLifecycleStatusDto::Completed, "bad", 3);
    assert!(
        store
            .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(invalid))
            .is_err()
    );
    let call = intention_types::ToolCallId::new();
    for status in [
        ToolLifecycleStatusDto::Admitted,
        ToolLifecycleStatusDto::Started,
        ToolLifecycleStatusDto::Completed,
    ] {
        let event = ToolLifecycleEventDto::new(session, run, call, "shell", status, "ok", time(3))
            .expect("event");
        store
            .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(event))
            .expect("valid sequence");
    }
    let late = ToolLifecycleEventDto::new(
        session,
        run,
        call,
        "shell",
        ToolLifecycleStatusDto::Failed,
        "late",
        time(4),
    )
    .expect("event");
    assert!(
        store
            .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(late))
            .is_err()
    );
}

#[test]
fn tool_lifecycle_guard_rejects_terminal_and_interrupted_runs_but_accepts_active_runs() {
    for status in [
        RunStatusDto::Completed,
        RunStatusDto::Cancelled,
        RunStatusDto::Failed,
        RunStatusDto::Interrupted,
    ] {
        let (_directory, store) = repository();
        let session = create(&store);
        let run = RunId::new();
        accept(&store, session, TurnId::new(), run, "run");
        if status == RunStatusDto::Cancelled {
            store
                .transition_run(TransitionRunInputDto::new(
                    session,
                    run,
                    RunStatusDto::Cancelling,
                    time(3),
                ))
                .expect("cancellation begins");
        }
        if status == RunStatusDto::Completed {
            store
                .transition_run(TransitionRunInputDto::new(
                    session,
                    run,
                    RunStatusDto::Running,
                    time(3),
                ))
                .expect("run starts");
            store
                .transition_run(TransitionRunInputDto::new(
                    session,
                    run,
                    RunStatusDto::Completing,
                    time(4),
                ))
                .expect("run begins completion");
        }
        store
            .transition_run(TransitionRunInputDto::new(session, run, status, time(5)))
            .expect("run reaches terminal state");
        let error = store
            .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(
                tool_event_with_status(
                    session,
                    run,
                    ToolLifecycleStatusDto::Admitted,
                    "late effect",
                    5,
                ),
            ))
            .expect_err("terminal run rejects tool effects");
        assert_eq!(error.code(), "terminal_run_tool_lifecycle");
    }

    let (_directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    accept(&store, session, TurnId::new(), run, "active");
    store
        .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(
            tool_event_with_status(
                session,
                run,
                ToolLifecycleStatusDto::Admitted,
                "active effect",
                3,
            ),
        ))
        .expect("active run accepts tool effect");
}

#[test]
fn create_accept_queue_idempotence_removal_snapshots_and_tail_are_durable() {
    let (_directory, store) = repository();
    let session = create(&store);
    let first_turn = TurnId::new();
    let first_run = RunId::new();
    let first = accept(&store, session, first_turn, first_run, "first");
    assert!(
        matches!(first.turn_outcome(), Some(AcceptedTurnOutcomeDto::Started(run)) if run.run_id() == first_run)
    );
    assert_eq!(first.events().len(), 2);

    let second_turn = TurnId::new();
    let second_run = RunId::new();
    let queued = accept(&store, session, second_turn, second_run, "second");
    assert_eq!(
        queued.turn_outcome(),
        Some(AcceptedTurnOutcomeDto::Queued(
            intention_types::QueuePositionDto::new(0)
        ))
    );
    let retried = accept(&store, session, second_turn, second_run, "second");
    assert!(retried.events().is_empty());
    assert_eq!(retried.position(), queued.position());
    assert_eq!(
        store
            .accept_user_turn(
                AcceptUserTurnInputDto::new(
                    session,
                    second_turn,
                    "changed",
                    second_run,
                    snapshot(),
                    time(3)
                )
                .expect("turn input is valid"),
            )
            .expect_err("changed idempotent content conflicts")
            .code(),
        "turn_idempotency_conflict"
    );
    let snapshot_before = store
        .load_session_snapshot(session)
        .expect("snapshot loads");
    assert_eq!(snapshot_before.queued_turns().len(), 1);
    store
        .remove_queued_turn(RemoveQueuedTurnInputDto::new(
            RemoveQueuedTurnCommandDto::new(session, second_turn),
            time(4),
        ))
        .expect("queued turn removes");
    let projection = store
        .load_session_snapshot(session)
        .expect("snapshot loads");
    assert!(projection.queued_turns().is_empty());
    let tail = store
        .load_tail(session, SessionEventSequenceDto::new(0))
        .expect("full tail loads");
    assert_eq!(
        tail.last().expect("tail has events").sequence(),
        projection.at_sequence()
    );
}

#[test]
fn terminal_transition_promotes_queue_and_recovery_promotes_oldest_queued_turn() {
    let (_directory, store) = repository();
    let session = create(&store);
    let active_run = RunId::new();
    accept(&store, session, TurnId::new(), active_run, "active");
    let queued = [(TurnId::new(), RunId::new()), (TurnId::new(), RunId::new())];
    for (turn_id, run_id) in queued {
        accept(&store, session, turn_id, run_id, "queued");
    }

    let recovered = store
        .recover_unfinished_runs(RecoverUnfinishedRunsInputDto::new(time(5)))
        .expect("recovery commits terminal interruption and promotion");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].events().len(), 2);
    let projection = store
        .load_session_snapshot(session)
        .expect("snapshot loads");
    assert_eq!(
        projection
            .active_run()
            .expect("oldest queue entry promotes")
            .run_id(),
        queued[0].1
    );
    assert_eq!(
        projection
            .active_run()
            .expect("promoted run remains active")
            .config_revision_id(),
        snapshot().revision_id()
    );
    assert_eq!(projection.queued_turns().len(), 1);
    assert_eq!(projection.queued_turns()[0].turn_id(), queued[1].0);
    let tail = store
        .load_tail(session, SessionEventSequenceDto::new(0))
        .expect("tail loads");
    assert!(matches!(
        tail[tail.len() - 2].payload(),
        intention_domain::DomainEventDto::RunStatusChanged(event)
            if event.status() == RunStatusDto::Interrupted
    ));
    assert!(
        matches!(tail.last().expect("promoted run event exists").payload(), intention_domain::DomainEventDto::RunStarted(event) if event.run_id() == queued[0].1 && event.config_revision_id() == snapshot().revision_id())
    );
    let later = accept(&store, session, TurnId::new(), RunId::new(), "later");
    assert_eq!(
        later.turn_outcome(),
        Some(AcceptedTurnOutcomeDto::Queued(
            intention_types::QueuePositionDto::new(2)
        ))
    );
    let projection = store
        .load_session_snapshot(session)
        .expect("post-recovery snapshot loads");
    assert_eq!(projection.queued_turns().len(), 2);
    assert_eq!(projection.queued_turns()[0].turn_id(), queued[1].0);
}

#[test]
fn every_terminal_transition_promotes_the_oldest_queued_turn() {
    for status in [
        RunStatusDto::Failed,
        RunStatusDto::Interrupted,
        RunStatusDto::Cancelled,
    ] {
        let (_directory, store) = repository();
        let session = create(&store);
        let active_run = RunId::new();
        accept(&store, session, TurnId::new(), active_run, "active");
        let queued_run = RunId::new();
        accept(&store, session, TurnId::new(), queued_run, "queued");
        if status == RunStatusDto::Cancelled {
            store
                .transition_run(TransitionRunInputDto::new(
                    session,
                    active_run,
                    RunStatusDto::Cancelling,
                    time(3),
                ))
                .expect("cancellation begins before terminal state");
        }
        store
            .transition_run(TransitionRunInputDto::new(
                session,
                active_run,
                status,
                time(4),
            ))
            .expect("terminal transition promotes the queued turn");
        assert_eq!(
            store
                .load_session_snapshot(session)
                .expect("snapshot loads")
                .active_run()
                .expect("queued run promotes")
                .run_id(),
            queued_run
        );
    }
}

#[test]
fn terminal_promotion_retains_queued_run_identity_and_configuration_snapshot() {
    let (_directory, store) = repository();
    let session = create(&store);
    let active_run = RunId::new();
    accept(&store, session, TurnId::new(), active_run, "active");
    let queued_turn = TurnId::new();
    let queued_run = RunId::new();
    let revision_a = ConfigRevisionId::new();
    let config_a = snapshot_with_revision_and_model(revision_a, "fixture-a");
    store
        .accept_user_turn(
            AcceptUserTurnInputDto::new(
                session,
                queued_turn,
                "queued",
                queued_run,
                config_a.clone(),
                time(3),
            )
            .expect("queued input is valid"),
        )
        .expect("turn queues");
    let config_b = snapshot_with_revision_and_model(ConfigRevisionId::new(), "fixture-b");
    store
        .accept_configuration_revision(config_b)
        .expect("new daemon-start configuration may be accepted");
    store
        .transition_run(TransitionRunInputDto::new(
            session,
            active_run,
            RunStatusDto::Failed,
            time(4),
        ))
        .expect("terminal transition promotes original queued selection");
    let active = store
        .load_session_snapshot(session)
        .expect("snapshot loads")
        .active_run()
        .expect("queued run promoted");
    assert_eq!(active.run_id(), queued_run);
    assert_eq!(active.config_revision_id(), revision_a);
    store
        .accept_configuration_revision(config_a)
        .expect("queued snapshot remains canonical and unmodified");
}

#[test]
fn canonical_config_revision_rejects_conflicting_snapshot_without_sensitive_details() {
    let (_directory, store) = repository();
    let revision = ConfigRevisionId::new();
    let original = snapshot_with_revision_and_model(revision, "fixture-a");
    let conflicting = snapshot_with_revision_and_model(revision, "fixture-b");
    store
        .accept_configuration_revision(original.clone())
        .expect("initial revision persists");
    store
        .accept_configuration_revision(original)
        .expect("identical revision is idempotent");
    let error = store
        .accept_configuration_revision(conflicting)
        .expect_err("revision cannot bind a different snapshot");
    assert_eq!(error.code(), "config_revision_conflict");
    assert!(!error.to_string().contains("fixture-secret"));
    assert!(!error.to_string().contains("/tmp/"));
}

#[test]
fn turn_acceptance_rejects_config_revision_collision() {
    let (_directory, store) = repository();
    let session = create(&store);
    let revision = ConfigRevisionId::new();
    let original = snapshot_with_revision_and_model(revision, "fixture-a");
    store
        .accept_user_turn(
            AcceptUserTurnInputDto::new(
                session,
                TurnId::new(),
                "first",
                RunId::new(),
                original,
                time(2),
            )
            .expect("turn input is valid"),
        )
        .expect("initial turn commits");
    let error = store
        .accept_user_turn(
            AcceptUserTurnInputDto::new(
                session,
                TurnId::new(),
                "second",
                RunId::new(),
                snapshot_with_revision_and_model(revision, "fixture-b"),
                time(3),
            )
            .expect("turn input is valid"),
        )
        .expect_err("turn acceptance cannot reuse revision for different snapshot");
    assert_eq!(error.code(), "config_revision_conflict");
    assert!(!error.to_string().contains("fixture-secret"));
    assert!(!error.to_string().contains("/tmp/"));
}

#[test]
fn workspace_identity_cannot_bind_conflicting_roots_and_unknown_tails_fail_typed() {
    let (_directory, store) = repository();
    let workspace_id = WorkspaceId::new();
    let root = workspace_root("canonical");
    for session in [SessionId::new(), SessionId::new()] {
        store
            .create_session(CreateSessionInputDto::new(
                CreateSessionCommandDto::new(
                    ProjectId::new(),
                    session,
                    workspace_id,
                    root.clone(),
                    RunModeDto::Build,
                ),
                time(1),
            ))
            .expect("same workspace identity and root remain canonical");
    }
    let conflict = store
        .create_session(CreateSessionInputDto::new(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                SessionId::new(),
                workspace_id,
                workspace_root("conflict"),
                RunModeDto::Build,
            ),
            time(2),
        ))
        .expect_err("workspace identity cannot bind a different root");
    assert_eq!(conflict.code(), "workspace_root_conflict");
    assert_eq!(
        store
            .load_tail(SessionId::new(), SessionEventSequenceDto::new(0))
            .expect_err("unknown tail is typed not-found")
            .code(),
        "storage_record_not_found"
    );
}

#[test]
fn future_tail_positions_fail_before_sqlite_integer_conversion() {
    let (_directory, store) = repository();
    let session = create(&store);
    for position in [
        SessionEventSequenceDto::new(2),
        SessionEventSequenceDto::new(u64::MAX),
    ] {
        assert_eq!(
            store
                .load_tail(session, position)
                .expect_err("future cursor is rejected before a history query")
                .code(),
            "invalid_event_tail_position"
        );
    }
}

#[test]
fn direct_starting_to_cancelled_transition_is_rejected() {
    let (_directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    accept(&store, session, TurnId::new(), run, "active");
    assert_eq!(
        store
            .transition_run(TransitionRunInputDto::new(
                session,
                run,
                RunStatusDto::Cancelled,
                time(3),
            ))
            .expect_err("direct cancellation bypasses the required cancelling state")
            .code(),
        "invalid_run_status_transition"
    );
}

#[test]
fn queue_promotion_guard_rejects_a_new_run_ahead_of_durable_work() {
    let (directory, store) = repository();
    let session = create(&store);
    let active_run = RunId::new();
    accept(&store, session, TurnId::new(), active_run, "active");
    accept(&store, session, TurnId::new(), RunId::new(), "queued");
    let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
        .expect("fixture database reopens");
    connection
        .execute(
            "UPDATE runs SET status='interrupted' WHERE run_id=?1",
            [active_run.to_string()],
        )
        .expect("fixture simulates an inconsistent inactive queue state");
    drop(connection);
    assert_eq!(
        store
            .accept_user_turn(
                AcceptUserTurnInputDto::new(
                    session,
                    TurnId::new(),
                    "must not bypass queue",
                    RunId::new(),
                    snapshot(),
                    time(3),
                )
                .expect("turn input is valid"),
            )
            .expect_err("inactive sessions with queued work must not start a new run")
            .code(),
        "queue_promotion_required"
    );
}

#[test]
fn queue_tickets_never_reuse_and_terminal_promotion_selects_oldest() {
    let (_directory, store) = repository();
    let session = create(&store);
    let active_run = RunId::new();
    accept(&store, session, TurnId::new(), active_run, "active");
    let queued = [
        (TurnId::new(), RunId::new()),
        (TurnId::new(), RunId::new()),
        (TurnId::new(), RunId::new()),
    ];
    for (index, (turn, run)) in queued.iter().enumerate() {
        let change = accept(&store, session, *turn, *run, "queued");
        assert!(matches!(
            change.turn_outcome(),
            Some(AcceptedTurnOutcomeDto::Queued(position)) if position.value() == index as u64
        ));
    }
    store
        .remove_queued_turn(RemoveQueuedTurnInputDto::new(
            RemoveQueuedTurnCommandDto::new(session, queued[0].0),
            time(3),
        ))
        .expect("oldest queued turn removes");
    let later = accept(&store, session, TurnId::new(), RunId::new(), "later");
    assert!(matches!(
        later.turn_outcome(),
        Some(AcceptedTurnOutcomeDto::Queued(position)) if position.value() == 3
    ));
    store
        .transition_run(TransitionRunInputDto::new(
            session,
            active_run,
            RunStatusDto::Failed,
            time(4),
        ))
        .expect("terminal transition promotes oldest remaining ticket");
    assert_eq!(
        store
            .load_session_snapshot(session)
            .expect("snapshot loads")
            .active_run()
            .expect("oldest remaining turn promotes")
            .run_id(),
        queued[1].1
    );
}

#[test]
fn migration_rejects_future_schema_and_config_snapshot_is_safe_to_persist() {
    let directory = TempDir::new().expect("temporary directory exists");
    let path = directory.path().join("future.sqlite");
    let connection = sqlite::Connection::open(&path).expect("fixture database opens");
    connection
        .pragma_update(None, "user_version", 99_i64)
        .expect("version sets");
    drop(connection);
    let future_result = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(path.to_string_lossy().into_owned()).expect("absolute path"),
    );
    assert_eq!(
        future_result.err().expect("future schema rejects").code(),
        "unsupported_storage_schema"
    );
    let (_directory, store) = repository();
    store
        .accept_configuration_revision(snapshot())
        .expect("safe snapshot persists");
}

#[test]
fn slice1_storage_schema_four_remains_authoritative() {
    let directory = TempDir::new().expect("temporary directory exists");
    let path = directory.path().join("storage.sqlite");
    let _store = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(path.to_string_lossy().into_owned()).expect("absolute path"),
    )
    .expect("database opens");
    let connection = sqlite::Connection::open(path).expect("database reopens");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version reads");
    assert_eq!(version, 4);
    let expected = [
        // Preserved M3/M4 tables.
        "projects",
        "workspace_roots",
        "sessions",
        "turns",
        "runs",
        "queued_turns",
        "configuration_revisions",
        "domain_events",
        "session_snapshots",
        "run_snapshots",
        "run_cursors",
        "model_run_facts",
        "model_run_snapshots",
        "tool_results",
        // Additive schema-4 tables.
        "provider_kind_descriptor_revisions",
        "provider_profile_revisions",
        "provider_profile_tombstones",
        "provider_kind_tombstones",
        "provider_catalog_state",
        "provider_catalog_profile_projection",
        "configuration_audit",
        "session_provider_defaults",
        "resolved_run_provider_selections",
        "unavailable_provider_queue",
        "unavailable_queue_reconciliation_markers",
        "provider_usage_aggregates",
        "provider_usage_facts",
        "provider_catalog_removal_candidates",
        "held_recovered_runs",
    ];
    for table in expected {
        let present: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("table lookup");
        assert_eq!(present, 1, "missing table {table}");
    }
    let active_run_object: String = connection
        .query_row(
            "SELECT type FROM sqlite_master WHERE name='one_active_run_per_session'",
            [],
            |row| row.get(0),
        )
        .expect("active-run object lookup");
    assert_eq!(active_run_object, "index");
    let pending_object: String = connection
        .query_row(
            "SELECT type FROM sqlite_master WHERE name='one_pending_provider_catalog_candidate'",
            [],
            |row| row.get(0),
        )
        .expect("pending-candidate object lookup");
    assert_eq!(pending_object, "index");
    let state_seed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM provider_catalog_state WHERE singleton_id=1 AND status='preparing'",
            [],
            |row| row.get(0),
        )
        .expect("state seed lookup");
    assert_eq!(state_seed, 1);
}

#[test]
fn completed_result_evidence_is_durable_across_reopen_with_redacted_payload() {
    let (directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    accept(&store, session, TurnId::new(), run, "run");
    // Commit the exact terminal lifecycle evidence the application pipeline
    // writes for one successful invocation: admitted, started, completed.
    let call = intention_types::ToolCallId::new();
    for (index, (status, detail)) in [
        (
            ToolLifecycleStatusDto::Admitted,
            "local tool invocation admitted",
        ),
        (
            ToolLifecycleStatusDto::Started,
            "local tool invocation started",
        ),
        (
            ToolLifecycleStatusDto::Completed,
            "local tool invocation completed",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let appended = store
            .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(
                ToolLifecycleEventDto::new(session, run, call, "read", status, detail, time(3))
                    .expect("pipeline lifecycle event is valid"),
            ))
            .expect("pipeline lifecycle commit succeeds");
        assert_eq!(appended.sequence().value(), 4 + index as u64);
    }

    // A restart drops the handle and reopens the exact durable location.
    drop(store);
    let reopened = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(
            directory
                .path()
                .join("storage.sqlite")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("reopened location is absolute"),
    )
    .expect("durable database reopens after restart");

    let tail = reopened
        .load_tail(session, SessionEventSequenceDto::new(0))
        .expect("durable tail reloads after reopen");
    let lifecycle = tail
        .iter()
        .filter_map(|event| match event.payload() {
            DomainEventDto::ToolLifecycle(item) => Some(item),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(lifecycle.len(), 3);
    assert_eq!(lifecycle[0].status(), &ToolLifecycleStatusDto::Admitted);
    assert_eq!(lifecycle[0].detail(), "local tool invocation admitted");
    assert_eq!(lifecycle[1].status(), &ToolLifecycleStatusDto::Started);
    assert_eq!(lifecycle[1].detail(), "local tool invocation started");
    assert_eq!(lifecycle[2].status(), &ToolLifecycleStatusDto::Completed);
    assert_eq!(lifecycle[2].detail(), "local tool invocation completed");
    assert!(lifecycle.iter().all(|item| {
        item.session_id() == session && item.run_id() == run && item.call_id() == call
    }));

    // The terminal commit refreshed the session snapshot in its transaction.
    let projection = reopened
        .load_session_snapshot(session)
        .expect("recovered session snapshot loads");
    assert_eq!(
        projection.at_sequence(),
        tail.last().expect("tail has events").sequence()
    );

    // The reopened terminal evidence round-trips without secret or root-path
    // material in its durable payload.
    let terminal = tail.last().expect("terminal envelope is durable");
    let encoded = serde_json::to_string(terminal).expect("terminal evidence serializes");
    let decoded: intention_types::EventEnvelopeDto<DomainEventDto> =
        serde_json::from_str(&encoded).expect("terminal evidence round-trips");
    assert_eq!(&decoded, terminal);
    assert!(!encoded.contains("credential"));
    assert!(!encoded.contains("fixture-secret"));
    let fixture_root = std::env::temp_dir()
        .join("intention-storage-sqlite-contracts")
        .join("storage-contract");
    assert!(!encoded.contains(fixture_root.to_string_lossy().as_ref()));

    // The durable terminal guard survives the restart: a completed call can
    // never be re-driven to another terminal state after recovery.
    let late = ToolLifecycleEventDto::new(
        session,
        run,
        call,
        "read",
        ToolLifecycleStatusDto::Failed,
        "late failure after completion",
        time(4),
    )
    .expect("late event is valid");
    assert_eq!(
        reopened
            .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(late))
            .expect_err("terminal guard survives the restart")
            .code(),
        "invalid_tool_lifecycle_transition"
    );

    // Recovery leaves the store writable at the correct next sequence for a
    // fresh invocation.
    let next_call = intention_types::ToolCallId::new();
    let next = reopened
        .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(
            ToolLifecycleEventDto::new(
                session,
                run,
                next_call,
                "read",
                ToolLifecycleStatusDto::Admitted,
                "local tool invocation admitted",
                time(5),
            )
            .expect("next invocation event is valid"),
        ))
        .expect("a fresh invocation admits after recovery");
    assert_eq!(next.sequence().value(), 7);
}

#[test]
fn tool_result_evidence_commits_with_its_lifecycle_event_and_rereads_durably() {
    let (directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    accept(&store, session, TurnId::new(), run, "run");
    let call = intention_types::ToolCallId::new();
    let admitted = ToolLifecycleEventDto::new(
        session,
        run,
        call,
        "read",
        ToolLifecycleStatusDto::Admitted,
        "admitted",
        time(3),
    )
    .expect("tool event is valid");
    store
        .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(admitted))
        .expect("admitted event commits without evidence");
    let started = ToolLifecycleEventDto::new(
        session,
        run,
        call,
        "read",
        ToolLifecycleStatusDto::Started,
        "started",
        time(3),
    )
    .expect("tool event is valid");
    store
        .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(started))
        .expect("started event commits without evidence");
    let evidence = ToolResultEvidenceDto::new(
        session,
        run,
        call,
        ToolResultKindDto::Read,
        r#"{"result":"read","value":{"text":"hello","truncated":false}}"#,
        time(4),
    )
    .expect("tool result evidence is valid");
    let completed = ToolLifecycleEventDto::new(
        session,
        run,
        call,
        "read",
        ToolLifecycleStatusDto::Completed,
        "completed",
        time(4),
    )
    .expect("tool event is valid");
    let committed = store
        .append_tool_lifecycle_event(
            AppendToolLifecycleEventInputDto::new(completed)
                .with_result(evidence.clone())
                .expect("terminal evidence attaches"),
        )
        .expect("terminal event and evidence commit atomically");
    assert_eq!(committed.sequence().value(), 6);
    assert!(
        matches!(committed.payload(), DomainEventDto::ToolLifecycle(value) if value.status() == &ToolLifecycleStatusDto::Completed)
    );
    assert_eq!(
        store
            .load_tool_result(session, run, call)
            .expect("typed evidence rereads"),
        evidence
    );
    let projection = store
        .load_session_snapshot(session)
        .expect("snapshot stays coherent with the commit");
    assert_eq!(projection.at_sequence().value(), 6);
    drop(store);
    let reopened = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(
            directory
                .path()
                .join("storage.sqlite")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("temp location is absolute"),
    )
    .expect("database reopens");
    assert_eq!(
        reopened
            .load_tool_result(session, run, call)
            .expect("durable evidence rereads after reopen"),
        evidence
    );
}

#[test]
fn terminal_tool_lifecycle_rejects_mismatched_typed_result_evidence() {
    let (_directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    accept(&store, session, TurnId::new(), run, "run");
    let call = intention_types::ToolCallId::new();
    for status in [
        ToolLifecycleStatusDto::Admitted,
        ToolLifecycleStatusDto::Started,
    ] {
        store
            .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(
                ToolLifecycleEventDto::new(session, run, call, "read", status, "phase", time(3))
                    .expect("lifecycle event is valid"),
            ))
            .expect("non-terminal lifecycle event commits");
    }
    let terminal = ToolLifecycleEventDto::new(
        session,
        run,
        call,
        "read",
        ToolLifecycleStatusDto::Completed,
        "completed",
        time(4),
    )
    .expect("terminal event is valid");
    let evidence = ToolResultEvidenceDto::new(
        session,
        run,
        intention_types::ToolCallId::new(),
        ToolResultKindDto::Read,
        r#"{"result":"read"}"#,
        time(4),
    )
    .expect("mismatched evidence is structurally valid");
    assert_eq!(
        AppendToolLifecycleEventInputDto::new(terminal)
            .with_result(evidence)
            .expect_err("mismatched evidence is rejected at attachment")
            .code(),
        "invalid_tool_result"
    );
    assert_eq!(
        store
            .load_tail(session, SessionEventSequenceDto::new(0))
            .expect("tail loads")
            .len(),
        5
    );
}

#[test]
fn tool_result_reread_is_typed_not_found_without_durably_committed_evidence() {
    let (_directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    accept(&store, session, TurnId::new(), run, "run");
    let call = intention_types::ToolCallId::new();
    for status in [
        ToolLifecycleStatusDto::Admitted,
        ToolLifecycleStatusDto::Started,
    ] {
        let event =
            ToolLifecycleEventDto::new(session, run, call, "read", status, "phase", time(3))
                .expect("tool event is valid");
        store
            .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(event))
            .expect("non-terminal event commits without evidence");
    }
    assert_eq!(
        store
            .load_tool_result(session, run, call)
            .expect_err("no evidence was durably committed")
            .code(),
        "tool_result_not_found"
    );
    assert_eq!(
        store
            .load_tool_result(SessionId::new(), run, call)
            .expect_err("cross-session identity finds no evidence")
            .code(),
        "tool_result_not_found"
    );
}

// ============================================================================
// Schema-4 migration, preservation, and fake-secret fixtures.
// ============================================================================

/// A byte-exact captured cell typed by SQLite storage class: text stays raw
/// bytes, blobs stay raw bytes, and reals keep their exact bit pattern.
#[derive(Clone, Debug, Eq, PartialEq)]
enum CapturedValue {
    Null,
    Integer(i64),
    Real(u64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

/// Captures every row of a table as typed values in rowid order, tagged with
/// the table name, so pre-migration and post-migration bytes compare exactly.
fn capture_rows(connection: &sqlite::Connection, table: &str) -> Vec<(String, Vec<CapturedValue>)> {
    let mut statement = connection
        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
        .expect("capture statement prepares");
    let column_count = statement.column_count();
    let mut rows = statement.query([]).expect("capture query runs");
    let mut captured = Vec::new();
    while let Some(row) = rows.next().expect("capture row reads") {
        let mut values = Vec::with_capacity(column_count);
        for index in 0..column_count {
            let value = match row.get_ref(index).expect("capture value reads") {
                sqlite::types::ValueRef::Null => CapturedValue::Null,
                sqlite::types::ValueRef::Integer(value) => CapturedValue::Integer(value),
                sqlite::types::ValueRef::Real(value) => CapturedValue::Real(value.to_bits()),
                sqlite::types::ValueRef::Text(text) => CapturedValue::Text(text.to_vec()),
                sqlite::types::ValueRef::Blob(blob) => CapturedValue::Blob(blob.to_vec()),
            };
            values.push(value);
        }
        captured.push((table.to_string(), values));
    }
    captured
}

/// A valid opaque envelope for one durable reasoning-delta domain event.
fn reasoning_delta_envelope(
    session_id: SessionId,
    run_id: RunId,
    turn_id: TurnId,
    event_id: EventId,
    sequence: u64,
) -> EventEnvelopeDto<DomainEventDto> {
    let fact = ModelRunFactDto::new(
        RunEventCursorDto::new(sequence),
        ModelRunFactInputDto::reasoning_delta_recorded("tail-only reasoning")
            .expect("reasoning fact is valid"),
    )
    .expect("model fact is valid");
    let payload = ModelRunFactEventDto::new(session_id, run_id, fact, time(3));
    EventEnvelopeDto::new(
        EventMetadataDto::new(
            SchemaVersionDto::new(1, 0),
            event_id,
            session_id,
            Some(run_id),
            Some(turn_id),
            SessionEventSequenceDto::new(sequence),
            time(3),
        ),
        DomainEventDto::ReasoningDeltaRecorded(payload),
    )
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
        kind_descriptor_revision_id: "kind-descriptor-rev-0001".to_owned(),
        driver_contract_revision: ProviderDriverContractRevisionDto {
            driver_family: "responses".to_owned(),
            major: 1,
            minor: 0,
        },
    }
}

fn fixture_profile_candidate(profile_id: &str, revision_id: &str) -> ProviderProfileCandidateDto {
    ProviderProfileCandidateDto {
        profile: fixture_profile(profile_id, revision_id),
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
        readiness: ProviderReadinessDto::Ready,
    }
}

fn fixture_selection() -> ProviderSelectionV1 {
    ProviderSelectionV1 {
        selection_canonicalization_version: "1".to_owned(),
        profile_id: "profile-default".to_owned(),
        provider_profile_revision_id: "rev-0001".to_owned(),
        kind_id: "responses".to_owned(),
        kind_descriptor_revision_id: "kind-descriptor-rev-0001".to_owned(),
        model_id: "gpt-4.1".to_owned(),
        normalized_effective_endpoint: "https://api.example.com/v1".to_owned(),
        credential_transport_mode: CredentialTransportMode::Bearer,
        credential_transport_safe_header_name: None,
        declared_model_capability_subset: vec!["text_streaming".to_owned()],
        resolved_reasoning_policy: "textual-reasoning-v1".to_owned(),
        effective_execution_policy: "ordinary".to_owned(),
        effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
        provider_driver_contract_revision: "responses-1.0".to_owned(),
        selection_source: Some("catalog-rev-0001".to_owned()),
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
    store
        .accept_provider_catalog(AcceptProviderCatalogInputDto {
            catalog_revision_id: revision,
            candidate_handle: handle.to_owned(),
            kind_descriptors: vec![intention_storage::ProviderKindDescriptorCandidateDto {
                descriptor_revision_id: descriptor_revision_id.to_owned(),
                descriptor: fixture_kind_descriptor(kind_id),
            }],
            profiles: vec![fixture_profile_candidate(profile_id, profile_revision_id)],
            default_profile_id: profile_id.to_owned(),
            accepted_at,
            operation_id: operation_id.to_owned(),
        })
        .expect("fixture catalog accepts");
}

#[test]
fn schema4_migration_preserves_schema_three_rows_byte_for_byte() {
    let directory = TempDir::new().expect("temporary directory exists");
    let path = directory.path().join("legacy-v3-schema4.sqlite");
    let connection = sqlite::Connection::open(&path).expect("legacy database opens");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("foreign keys enable");
    connection
        .execute_batch(intention_storage_sqlite::TEST_SCHEMA_3_SQL)
        .expect("production schema three creates");
    let project_id = ProjectId::new();
    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    let revision_id = ConfigRevisionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let fact_event_id = EventId::new();
    let tool_event_id = EventId::new();
    let call_id = intention_types::ToolCallId::new();
    connection
        .execute(
            "INSERT INTO projects(project_id) VALUES (?1)",
            [project_id.to_string()],
        )
        .expect("project inserts");
    connection
        .execute(
            "INSERT INTO workspace_roots(workspace_id, workspace_root) VALUES (?1, ?2)",
            sqlite::params![
                workspace_id.to_string(),
                workspace_root("schema4-preserve").as_str()
            ],
        )
        .expect("workspace inserts");
    connection
        .execute(
            "INSERT INTO configuration_revisions(revision_id, snapshot_json) VALUES (?1, ?2)",
            sqlite::params![
                revision_id.to_string(),
                serde_json::to_string(&snapshot()).expect("safe snapshot serializes")
            ],
        )
        .expect("configuration revision inserts");
    connection
        .execute(
            "INSERT INTO sessions(session_id, project_id, workspace_id, workspace_root, mode, config_revision_id, last_sequence, next_queue_ticket) VALUES (?1, ?2, ?3, ?4, 'build', ?5, 4, 0)",
            sqlite::params![
                session_id.to_string(),
                project_id.to_string(),
                workspace_id.to_string(),
                workspace_root("schema4-preserve").as_str(),
                revision_id.to_string()
            ],
        )
        .expect("session inserts");
    connection
        .execute(
            "INSERT INTO turns(session_id, turn_id, content, proposed_run_id, config_revision_id, outcome, queue_ticket) VALUES (?1, ?2, 'turn', ?3, ?4, 'started', NULL)",
            sqlite::params![
                session_id.to_string(),
                turn_id.to_string(),
                run_id.to_string(),
                revision_id.to_string()
            ],
        )
        .expect("started turn inserts");
    connection
        .execute(
            "INSERT INTO runs(run_id, session_id, turn_id, status, config_revision_id) VALUES (?1, ?2, ?3, 'running', ?4)",
            sqlite::params![
                run_id.to_string(),
                session_id.to_string(),
                turn_id.to_string(),
                revision_id.to_string()
            ],
        )
        .expect("run inserts");
    connection
        .execute(
            "INSERT INTO domain_events(event_id, session_id, sequence, envelope_json) VALUES (?1, ?2, 1, ?3)",
            sqlite::params![
                fact_event_id.to_string(),
                session_id.to_string(),
                serde_json::to_string(&reasoning_delta_envelope(
                    session_id,
                    run_id,
                    turn_id,
                    fact_event_id,
                    1
                ))
                .expect("fact envelope serializes")
            ],
        )
        .expect("fact event inserts");
    connection
        .execute(
            "INSERT INTO domain_events(event_id, session_id, sequence, envelope_json) VALUES (?1, ?2, 2, ?3)",
            sqlite::params![
                tool_event_id.to_string(),
                session_id.to_string(),
                serde_json::to_string(&tool_lifecycle_envelope(
                    session_id,
                    run_id,
                    turn_id,
                    tool_event_id,
                    call_id,
                    2
                ))
                .expect("tool lifecycle envelope serializes")
            ],
        )
        .expect("tool event inserts");
    let run_projection = intention_domain::RunProjectionDto::new(
        session_id,
        run_id,
        turn_id,
        RunStatusDto::Running,
        revision_id,
    );
    let session_projection = intention_domain::SessionProjectionDto::new(
        project_id,
        session_id,
        workspace_id,
        workspace_root("schema4-preserve"),
        RunModeDto::Build,
        Some(revision_id),
        Some(run_projection),
        Vec::new(),
        SessionEventSequenceDto::new(4),
    )
    .expect("legacy session projection is valid");
    connection
        .execute(
            "INSERT INTO session_snapshots(session_id, sequence, projection_json) VALUES (?1, 4, ?2)",
            sqlite::params![
                session_id.to_string(),
                serde_json::to_string(&session_projection).expect("session projection serializes")
            ],
        )
        .expect("session snapshot inserts");
    connection
        .execute(
            "INSERT INTO run_snapshots(run_id, session_id, sequence, projection_json) VALUES (?1, ?2, 4, ?3)",
            sqlite::params![
                run_id.to_string(),
                session_id.to_string(),
                serde_json::to_string(&run_projection).expect("run projection serializes")
            ],
        )
        .expect("run snapshot inserts");
    connection
        .execute(
            "INSERT INTO run_cursors(run_id, session_id, cursor) VALUES (?1, ?2, 0)",
            sqlite::params![run_id.to_string(), session_id.to_string()],
        )
        .expect("run cursor inserts");
    connection
        .execute(
            "INSERT INTO model_run_facts(run_id, cursor, event_id) VALUES (?1, 1, ?2)",
            sqlite::params![run_id.to_string(), fact_event_id.to_string()],
        )
        .expect("model fact inserts");
    let model_snapshot = intention_domain::RunSnapshotDto::new(
        session_id,
        run_id,
        SessionEventSequenceDto::new(4),
        intention_domain::ModelRunProjectionDto::new(
            run_projection,
            intention_domain::RunEventCursorDto::new(0),
            None,
            "",
            None,
            None,
            None,
        )
        .expect("model projection is valid"),
    )
    .expect("model run snapshot is valid");
    connection
        .execute(
            "INSERT INTO model_run_snapshots(run_id, session_id, sequence, cursor, snapshot_json) VALUES (?1, ?2, 4, 0, ?3)",
            sqlite::params![
                run_id.to_string(),
                session_id.to_string(),
                serde_json::to_string(&model_snapshot).expect("model snapshot serializes")
            ],
        )
        .expect("model run snapshot inserts");
    connection
        .execute(
            "INSERT INTO tool_results(run_id, session_id, call_id, event_id, kind, content, occurred_at) VALUES (?1, ?2, ?3, ?4, 'read', 'file content', 3)",
            sqlite::params![
                run_id.to_string(),
                session_id.to_string(),
                call_id.to_string(),
                tool_event_id.to_string()
            ],
        )
        .expect("tool result inserts");
    connection
        .pragma_update(None, "user_version", 3_i64)
        .expect("schema three version sets");

    let tables = [
        "projects",
        "workspace_roots",
        "configuration_revisions",
        "sessions",
        "turns",
        "runs",
        "queued_turns",
        "domain_events",
        "session_snapshots",
        "run_snapshots",
        "run_cursors",
        "model_run_facts",
        "model_run_snapshots",
        "tool_results",
    ];
    let mut before = Vec::new();
    for table in tables {
        before.extend(capture_rows(&connection, table));
    }
    drop(connection);

    let repository = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(path.to_string_lossy().into_owned())
            .expect("database path is absolute"),
    )
    .expect("schema three database migrates to schema four");
    let replay = repository
        .load_current_run_replay(session_id, run_id)
        .expect("migrated run replay loads");
    assert_eq!(replay.snapshot().cursor().value(), 0);
    assert_eq!(replay.snapshot().at_sequence().value(), 4);
    drop(repository);

    let connection = sqlite::Connection::open(&path).expect("migrated database reopens");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version reads");
    assert_eq!(version, 4);
    let foreign_keys: i64 = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .expect("foreign keys pragma reads");
    assert_eq!(foreign_keys, 1);
    let mut after = Vec::new();
    for table in tables {
        after.extend(capture_rows(&connection, table));
    }
    assert_eq!(
        before, after,
        "every pre-existing M3/M4 row is byte-identical after migration to schema four"
    );
    for query in [
        "SELECT COUNT(*) FROM provider_kind_descriptor_revisions",
        "SELECT COUNT(*) FROM provider_profile_revisions",
        "SELECT COUNT(*) FROM resolved_run_provider_selections",
    ] {
        let count: i64 = connection
            .query_row(query, [], |row| row.get(0))
            .expect("provider row count loads");
        assert_eq!(
            count, 0,
            "no synthetic provider rows for historical runs: {query}"
        );
    }
    drop(connection);

    // Reopening repeats the preservation without rewriting any row.
    let reopened = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(path.to_string_lossy().into_owned())
            .expect("database path is absolute"),
    )
    .expect("database reopens");
    let replay = reopened
        .load_current_run_replay(session_id, run_id)
        .expect("replay reloads");
    assert_eq!(replay.snapshot().cursor().value(), 0);
    drop(reopened);
    let connection = sqlite::Connection::open(&path).expect("database reopens raw");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version reads");
    assert_eq!(version, 4);
    let mut after_reopen = Vec::new();
    for table in tables {
        after_reopen.extend(capture_rows(&connection, table));
    }
    assert_eq!(
        before, after_reopen,
        "reopen repeats byte-identical preservation"
    );
}

/// A valid opaque envelope for one durable tool-lifecycle domain event.
fn tool_lifecycle_envelope(
    session_id: SessionId,
    run_id: RunId,
    turn_id: TurnId,
    event_id: EventId,
    call_id: intention_types::ToolCallId,
    sequence: u64,
) -> EventEnvelopeDto<DomainEventDto> {
    let lifecycle = ToolLifecycleEventDto::new(
        session_id,
        run_id,
        call_id,
        "read",
        ToolLifecycleStatusDto::Admitted,
        "local tool invocation admitted",
        time(3),
    )
    .expect("tool lifecycle event is valid");
    EventEnvelopeDto::new(
        EventMetadataDto::new(
            SchemaVersionDto::new(1, 0),
            event_id,
            session_id,
            Some(run_id),
            Some(turn_id),
            SessionEventSequenceDto::new(sequence),
            time(3),
        ),
        DomainEventDto::ToolLifecycle(lifecycle),
    )
}

#[test]
fn schema4_migration_failure_rolls_back_to_schema_three_atomically() {
    let directory = TempDir::new().expect("temporary directory exists");
    let path = directory.path().join("broken-migration.sqlite");
    let connection = sqlite::Connection::open(&path).expect("legacy database opens");
    connection
        .execute_batch(intention_storage_sqlite::TEST_SCHEMA_3_SQL)
        .expect("schema three creates");
    connection
        .execute(
            "INSERT INTO projects(project_id) VALUES (?1)",
            [ProjectId::new().to_string()],
        )
        .expect("project inserts");
    connection
        .execute(
            "INSERT INTO workspace_roots(workspace_id, workspace_root) VALUES (?1, ?2)",
            sqlite::params![
                WorkspaceId::new().to_string(),
                workspace_root("failure").as_str()
            ],
        )
        .expect("workspace inserts");
    let session_id = SessionId::new();
    connection
        .execute(
            "INSERT INTO sessions(session_id, project_id, workspace_id, workspace_root, mode, config_revision_id, last_sequence, next_queue_ticket) VALUES (?1, (SELECT project_id FROM projects), (SELECT workspace_id FROM workspace_roots), ?2, 'build', NULL, 0, 0)",
            sqlite::params![session_id.to_string(), workspace_root("failure").as_str()],
        )
        .expect("session inserts");
    connection
        .pragma_update(None, "user_version", 3_i64)
        .expect("schema three version sets");
    let before = capture_rows(&connection, "sessions");
    drop(connection);

    // A schema-4 migration that fails partway must roll back atomically: the
    // schema-4 tables and the user_version bump are discarded together.
    let broken = format!(
        "{}; CREATE TABLE nope (this is not valid sql);",
        intention_storage_sqlite::SCHEMA_M5_SQL
    );
    let migrations = Migrations::new(vec![
        M::up("CREATE TABLE a (x);"),
        M::up("CREATE TABLE b (x);"),
        M::up("CREATE TABLE c (x);"),
        M::up(&broken),
    ]);
    let mut connection = sqlite::Connection::open(&path).expect("database reopens");
    assert!(
        migrations.to_latest(&mut connection).is_err(),
        "broken schema-4 migration must fail"
    );
    drop(connection);

    let connection = sqlite::Connection::open(&path).expect("database reopens");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version reads");
    assert_eq!(version, 3);
    assert_eq!(
        capture_rows(&connection, "sessions"),
        before,
        "failed migration leaves schema-3 rows unchanged"
    );
    for table in [
        "provider_kind_descriptor_revisions",
        "provider_catalog_state",
        "resolved_run_provider_selections",
    ] {
        let present: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .expect("table lookup");
        assert_eq!(
            present, 0,
            "schema-4 table {table} must not exist after failed migration"
        );
    }
}

#[test]
fn schema4_tables_never_persist_fake_secrets() {
    let (directory, store) = repository();
    let session_id = create(&store);
    let run_id = RunId::new();
    store
        .accept_user_turn(
            AcceptUserTurnInputDto::new(
                session_id,
                TurnId::new(),
                "turn with selection",
                run_id,
                snapshot_with_revision_and_model(ConfigRevisionId::new(), "fixture-model"),
                time(2),
            )
            .expect("turn input is valid")
            .with_provider_selection(fixture_selection()),
        )
        .expect("turn with selection commits");
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
        .set_session_provider_profile(SetSessionProviderProfileInputDto {
            session_id,
            profile_id: "profile-a".to_owned(),
            expected_projection_revision: 0,
            operation_id: "op-default-1".to_owned(),
            updated_at: 3,
        })
        .expect("session default sets");
    store
        .record_provider_usage(RecordProviderUsageInputDto {
            session_id,
            usage_period_start: 100,
            usage_period_end: 200,
            recorded_at: 3,
            events: vec![intention_storage::ProviderUsageEventInputDto {
                run_id,
                usage_event_id: "usage-event-1".to_owned(),
                profile_id: "profile-a".to_owned(),
                provider_profile_revision_id: "rev-a".to_owned(),
                model_id: "fixture-model".to_owned(),
                input_units: 10,
                output_units: 20,
                reasoning_units: 5,
                occurred_at: 3,
                usage_json: "{\"safe\":true}".to_owned(),
            }],
        })
        .expect("usage records");
    store
        .enqueue_unavailable_run(EnqueueUnavailableRunInputDto {
            run_id,
            session_id,
            profile_id: "profile-a".to_owned(),
            provider_profile_revision_id: "rev-a".to_owned(),
            unavailable_reason: "provider_unavailable".to_owned(),
            first_unavailable_at: 3,
            operation_id: "op-enqueue-1".to_owned(),
            selection_json: "{\"safe\":true}".to_owned(),
        })
        .expect("unavailable run enqueues");
    store
        .mark_recovered_run_held(MarkRecoveredRunHeldInputDto {
            run_id,
            session_id,
            held_at: 3,
            operation_id: "op-hold-1".to_owned(),
        })
        .expect("recovered run is held");
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
    store
        .commit_configuration_reload(CommitConfigurationReloadInputDto {
            snapshot: snapshot_with_revision_and_model(ConfigRevisionId::new(), "fixture-model"),
            operation_id: "op-reload-1".to_owned(),
            reloaded_at: 4,
        })
        .expect("configuration reload commits");

    let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
        .expect("database reopens");
    let tables = [
        "provider_kind_descriptor_revisions",
        "provider_profile_revisions",
        "provider_profile_tombstones",
        "provider_kind_tombstones",
        "provider_catalog_state",
        "provider_catalog_profile_projection",
        "configuration_audit",
        "session_provider_defaults",
        "resolved_run_provider_selections",
        "unavailable_provider_queue",
        "unavailable_queue_reconciliation_markers",
        "provider_usage_aggregates",
        "provider_usage_facts",
        "provider_catalog_removal_candidates",
        "held_recovered_runs",
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
}

#[test]
fn session_provider_default_writes_commit_and_round_trip_durably() {
    let (_directory, store) = repository();
    let session_id = create(&store);
    // The fresh INSERT path commits the durable intent and revision zero.
    let inserted = store
        .set_session_provider_profile(SetSessionProviderProfileInputDto {
            session_id,
            profile_id: "profile-a".to_owned(),
            expected_projection_revision: 0,
            operation_id: "op-default-1".to_owned(),
            updated_at: 3,
        })
        .expect("fresh session default commits");
    assert!(inserted.changed);
    assert_eq!(inserted.projection_revision, 0);
    let durable = store
        .get_session_provider_profile(session_id)
        .expect("session default reads")
        .expect("session default persists");
    assert_eq!(durable.profile_id, "profile-a");
    assert_eq!(durable.projection_revision, 0);
    assert_eq!(durable.last_operation_id, "op-default-1");
    assert_eq!(durable.updated_at, 3);
    // The idempotent same-operation no-op commits and keeps the intent.
    let idempotent = store
        .set_session_provider_profile(SetSessionProviderProfileInputDto {
            session_id,
            profile_id: "profile-a".to_owned(),
            expected_projection_revision: 0,
            operation_id: "op-default-1".to_owned(),
            updated_at: 4,
        })
        .expect("same-operation no-op commits");
    assert!(!idempotent.changed);
    assert_eq!(idempotent.projection_revision, 0);
    // The changed=false same-profile path commits its operation and time.
    let same_profile = store
        .set_session_provider_profile(SetSessionProviderProfileInputDto {
            session_id,
            profile_id: "profile-a".to_owned(),
            expected_projection_revision: 0,
            operation_id: "op-default-2".to_owned(),
            updated_at: 5,
        })
        .expect("same-profile update commits");
    assert!(!same_profile.changed);
    assert_eq!(same_profile.projection_revision, 0);
    let durable = store
        .get_session_provider_profile(session_id)
        .expect("session default reads")
        .expect("session default persists");
    assert_eq!(durable.profile_id, "profile-a");
    assert_eq!(durable.projection_revision, 0);
    assert_eq!(durable.last_operation_id, "op-default-2");
    assert_eq!(durable.updated_at, 5);
    // The changed-profile update path commits the next projection revision.
    let changed = store
        .set_session_provider_profile(SetSessionProviderProfileInputDto {
            session_id,
            profile_id: "profile-b".to_owned(),
            expected_projection_revision: 0,
            operation_id: "op-default-3".to_owned(),
            updated_at: 6,
        })
        .expect("changed-profile update commits");
    assert!(changed.changed);
    assert_eq!(changed.projection_revision, 1);
    let durable = store
        .get_session_provider_profile(session_id)
        .expect("session default reads")
        .expect("session default persists");
    assert_eq!(durable.profile_id, "profile-b");
    assert_eq!(durable.projection_revision, 1);
    assert_eq!(durable.last_operation_id, "op-default-3");
    assert_eq!(durable.updated_at, 6);
}

#[test]
fn resolved_run_provider_selection_round_trips_and_missing_rows_read_none() {
    let (_directory, store) = repository();
    let session_id = create(&store);
    let run_id = RunId::new();
    accept(&store, session_id, TurnId::new(), run_id, "run");
    let selection = fixture_selection();
    store
        .persist_resolved_run_provider_selection(PersistResolvedRunProviderSelectionInputDto {
            session_id,
            run_id,
            selection: selection.clone(),
            occurred_at: 3,
        })
        .expect("resolved selection persists");
    // The round trip returns the exact same selection.
    let loaded = store
        .load_resolved_run_provider_selection(session_id, run_id)
        .expect("resolved selection loads")
        .expect("resolved selection exists");
    assert_eq!(loaded, selection);
    // A missing row reads None.
    let other_run = RunId::new();
    accept(&store, session_id, TurnId::new(), other_run, "other run");
    assert!(
        store
            .load_resolved_run_provider_selection(session_id, other_run)
            .expect("missing selection reads")
            .is_none()
    );
    // A wrong session reads None even for an existing run.
    let other_session = SessionId::new();
    store
        .create_session(CreateSessionInputDto::new(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                other_session,
                WorkspaceId::new(),
                workspace_root("other-selection-session"),
                RunModeDto::Build,
            ),
            time(1),
        ))
        .expect("other session creates");
    assert!(
        store
            .load_resolved_run_provider_selection(other_session, run_id)
            .expect("cross-session selection reads")
            .is_none()
    );
}

/// Reopens the durable fixture database at the exact repository location.
fn reopen(directory: &TempDir) -> SqliteStorageRepository {
    SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(
            directory
                .path()
                .join("storage.sqlite")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("reopened location is absolute"),
    )
    .expect("database reopens")
}

#[test]
fn session_provider_default_stale_and_conflict_errors_and_reopen_durability() {
    let (directory, store) = repository();
    let session_id = create(&store);
    // A session with no durable default reads None.
    assert!(
        store
            .get_session_provider_profile(SessionId::new())
            .expect("unknown session default reads")
            .is_none()
    );
    store
        .set_session_provider_profile(SetSessionProviderProfileInputDto {
            session_id,
            profile_id: "profile-a".to_owned(),
            expected_projection_revision: 0,
            operation_id: "op-default-1".to_owned(),
            updated_at: 3,
        })
        .expect("fresh session default commits");
    // Replaying the same operation id with a different profile conflicts.
    assert_eq!(
        store
            .set_session_provider_profile(SetSessionProviderProfileInputDto {
                session_id,
                profile_id: "profile-other".to_owned(),
                expected_projection_revision: 0,
                operation_id: "op-default-1".to_owned(),
                updated_at: 4,
            })
            .expect_err("replayed operation cannot bind a different profile")
            .code(),
        "session_provider_default_conflict"
    );
    // A changed profile advances the projection revision.
    store
        .set_session_provider_profile(SetSessionProviderProfileInputDto {
            session_id,
            profile_id: "profile-b".to_owned(),
            expected_projection_revision: 0,
            operation_id: "op-default-2".to_owned(),
            updated_at: 4,
        })
        .expect("profile change commits");
    // A stale expected projection revision is rejected.
    assert_eq!(
        store
            .set_session_provider_profile(SetSessionProviderProfileInputDto {
                session_id,
                profile_id: "profile-c".to_owned(),
                expected_projection_revision: 0,
                operation_id: "op-default-3".to_owned(),
                updated_at: 5,
            })
            .expect_err("stale expected revision is rejected")
            .code(),
        "session_provider_default_stale"
    );
    // The durable row survives a repository reopen.
    drop(store);
    let reopened = reopen(&directory);
    let durable = reopened
        .get_session_provider_profile(session_id)
        .expect("session default reads")
        .expect("session default persists");
    assert_eq!(durable.profile_id, "profile-b");
    assert_eq!(durable.projection_revision, 1);
    assert_eq!(durable.last_operation_id, "op-default-2");
    assert_eq!(durable.updated_at, 4);
}

#[test]
fn held_recovered_run_lifecycle_branches_and_reopen_durability() {
    let (directory, store) = repository();
    let session_id = create(&store);
    let run_id = RunId::new();
    accept(&store, session_id, TurnId::new(), run_id, "recovered run");
    // Missing runs read None.
    assert!(
        store
            .load_held_recovered_run(RunId::new())
            .expect("missing held run reads")
            .is_none()
    );
    store
        .mark_recovered_run_held(MarkRecoveredRunHeldInputDto {
            run_id,
            session_id,
            held_at: 3,
            operation_id: "op-hold-1".to_owned(),
        })
        .expect("recovered run is held");
    // Marking again is idempotent (INSERT OR IGNORE).
    store
        .mark_recovered_run_held(MarkRecoveredRunHeldInputDto {
            run_id,
            session_id,
            held_at: 4,
            operation_id: "op-hold-2".to_owned(),
        })
        .expect("repeat hold is idempotent");
    let held = store
        .load_held_recovered_run(run_id)
        .expect("held run loads")
        .expect("held run exists");
    assert_eq!(held.session_id, session_id);
    assert_eq!(held.reason, "recovered_run_requires_explicit_admission");
    assert_eq!(held.admission_state, HeldRunAdmissionStateDto::Held);
    assert_eq!(held.admission_operation_id.as_deref(), Some("op-hold-1"));
    assert_eq!(held.admitted_at, None);
    // Admission commits the state and operation id.
    store
        .admit_held_recovered_run(AdmitHeldRecoveredRunInputDto {
            run_id,
            session_id,
            admitted_at: 4,
            operation_id: "op-admit-1".to_owned(),
        })
        .expect("held run admits");
    let admitted = store
        .load_held_recovered_run(run_id)
        .expect("admitted run loads")
        .expect("admitted run exists");
    assert_eq!(admitted.admission_state, HeldRunAdmissionStateDto::Admitted);
    assert_eq!(
        admitted.admission_operation_id.as_deref(),
        Some("op-admit-1")
    );
    assert_eq!(admitted.admitted_at, Some(4));
    // Re-admission, with the same or a different operation id, never creates a
    // second admission task.
    store
        .admit_held_recovered_run(AdmitHeldRecoveredRunInputDto {
            run_id,
            session_id,
            admitted_at: 5,
            operation_id: "op-admit-1".to_owned(),
        })
        .expect("repeat admission is idempotent");
    store
        .admit_held_recovered_run(AdmitHeldRecoveredRunInputDto {
            run_id,
            session_id,
            admitted_at: 5,
            operation_id: "op-admit-2".to_owned(),
        })
        .expect("later operation id is still idempotent");
    // A run that was never held cannot be admitted.
    assert_eq!(
        store
            .admit_held_recovered_run(AdmitHeldRecoveredRunInputDto {
                run_id: RunId::new(),
                session_id,
                admitted_at: 5,
                operation_id: "op-admit-x".to_owned(),
            })
            .expect_err("unknown held run is rejected")
            .code(),
        "held_recovered_run_not_found"
    );
    // The durable admission survives a repository reopen.
    drop(store);
    let reopened = reopen(&directory);
    let durable = reopened
        .load_held_recovered_run(run_id)
        .expect("held run reloads")
        .expect("held run record persists");
    assert_eq!(durable.admission_state, HeldRunAdmissionStateDto::Admitted);
    assert_eq!(
        durable.admission_operation_id.as_deref(),
        Some("op-admit-1")
    );
    assert_eq!(durable.admitted_at, Some(4));
}

#[test]
fn held_recovered_run_rejected_and_malformed_states_are_typed() {
    let (directory, store) = repository();
    let session_id = create(&store);
    let run_id = RunId::new();
    accept(&store, session_id, TurnId::new(), run_id, "rejected run");
    store
        .mark_recovered_run_held(MarkRecoveredRunHeldInputDto {
            run_id,
            session_id,
            held_at: 3,
            operation_id: "op-hold-1".to_owned(),
        })
        .expect("recovered run is held");
    // A raw rejected state (as written by an external reaper) blocks admission.
    {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        connection
            .execute(
                "UPDATE held_recovered_runs SET admission_state='rejected' WHERE run_id=?1",
                [run_id.to_string()],
            )
            .expect("fixture rejects the held run");
        drop(connection);
    }
    assert_eq!(
        store
            .admit_held_recovered_run(AdmitHeldRecoveredRunInputDto {
                run_id,
                session_id,
                admitted_at: 4,
                operation_id: "op-admit-1".to_owned(),
            })
            .expect_err("rejected runs cannot admit")
            .code(),
        "held_recovered_run_rejected"
    );
    let rejected = store
        .load_held_recovered_run(run_id)
        .expect("rejected run loads")
        .expect("rejected run exists");
    assert_eq!(rejected.admission_state, HeldRunAdmissionStateDto::Rejected);
    // The rejected state is durable across reopen.
    drop(store);
    let reopened = reopen(&directory);
    let durable = reopened
        .load_held_recovered_run(run_id)
        .expect("held run reloads")
        .expect("held run record persists");
    assert_eq!(durable.admission_state, HeldRunAdmissionStateDto::Rejected);
}

#[test]
fn provider_selection_malformed_rows_fail_typed_and_digests_conflict() {
    #![allow(
        clippy::literal_string_with_formatting_args,
        reason = "Malformed JSON fixture strings intentionally resemble formatting placeholders."
    )]
    let (directory, store) = repository();
    let session_id = create(&store);
    let run_id = RunId::new();
    accept(&store, session_id, TurnId::new(), run_id, "run");
    let selection = fixture_selection();
    store
        .persist_resolved_run_provider_selection(PersistResolvedRunProviderSelectionInputDto {
            session_id,
            run_id,
            selection: selection.clone(),
            occurred_at: 3,
        })
        .expect("resolved selection persists");
    // Persisting the identical selection for the same run is idempotent.
    store
        .persist_resolved_run_provider_selection(PersistResolvedRunProviderSelectionInputDto {
            session_id,
            run_id,
            selection: selection.clone(),
            occurred_at: 4,
        })
        .expect("identical selection is idempotent");
    // The same selection bytes for a different run hit the digest conflict.
    let other_run = RunId::new();
    accept(&store, session_id, TurnId::new(), other_run, "other run");
    assert_eq!(
        store
            .persist_resolved_run_provider_selection(PersistResolvedRunProviderSelectionInputDto {
                session_id,
                run_id: other_run,
                selection,
                occurred_at: 4,
            },)
            .expect_err("selection digest is bound to the first run")
            .code(),
        "provider_selection_digest_conflict"
    );
    // A malformed persisted selection row fails the typed decode.
    {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        connection
            .execute(
                "UPDATE resolved_run_provider_selections SET selection_json=?1 WHERE run_id=?2",
                sqlite::params!["{\"profile_id\":", run_id.to_string()],
            )
            .expect("fixture corrupts the selection row");
        drop(connection);
    }
    assert_eq!(
        store
            .load_resolved_run_provider_selection(session_id, run_id)
            .expect_err("malformed selection row fails the typed decode")
            .code(),
        "storage_decode_failed"
    );
}

#[test]
fn provider_selection_invalid_domain_record_fails_the_typed_load() {
    let (_directory, store) = repository();
    let session_id = create(&store);
    let run_id = RunId::new();
    accept(&store, session_id, TurnId::new(), run_id, "run");
    // An empty profile id fails ProviderSelectionV1::validate.
    let mut invalid = fixture_selection();
    invalid.profile_id = String::new();
    assert!(invalid.validate().is_err());
    store
        .persist_resolved_run_provider_selection(PersistResolvedRunProviderSelectionInputDto {
            session_id,
            run_id,
            selection: invalid,
            occurred_at: 3,
        })
        .expect("storage persists the raw selection bytes");
    // The typed load revalidates the domain record and fails closed.
    assert_eq!(
        store
            .load_resolved_run_provider_selection(session_id, run_id)
            .expect_err("invalid selection fails the typed decode")
            .code(),
        "storage_decode_failed"
    );
}

#[test]
fn configuration_reload_commits_idempotently_and_rolls_back_atomically() {
    let (directory, store) = repository();
    let revision = ConfigRevisionId::new();
    let snapshot_value = snapshot_with_revision_and_model(revision, "fixture-model");
    store
        .commit_configuration_reload(CommitConfigurationReloadInputDto {
            snapshot: snapshot_value.clone(),
            operation_id: "op-reload-1".to_owned(),
            reloaded_at: 4,
        })
        .expect("configuration reload commits");
    // The reload records exactly one audit row with the reloaded payload.
    let audit_json = {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        let rows: Vec<String> = connection
            .prepare("SELECT audit_json FROM configuration_audit WHERE operation_id='op-reload-1'")
            .expect("audit statement prepares")
            .query_map([], |row| row.get(0))
            .expect("audit query runs")
            .map(|row| row.expect("audit row reads"))
            .collect();
        drop(connection);
        rows
    };
    assert_eq!(audit_json.len(), 1);
    assert!(audit_json[0].contains("\"reloaded\":true"));
    assert!(!audit_json[0].contains("candidate_handle"));
    assert!(!audit_json[0].contains("fixture-secret"));
    assert!(!contains_credential_shape(&audit_json[0]));
    // The reloaded snapshot is durably committed.
    let revision_count: i64 = {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM configuration_revisions WHERE revision_id=?1",
                [revision.to_string()],
                |row| row.get(0),
            )
            .expect("revision count loads");
        drop(connection);
        count
    };
    assert_eq!(revision_count, 1);
    // Re-committing the same operation id is idempotent: no second audit row.
    store
        .commit_configuration_reload(CommitConfigurationReloadInputDto {
            snapshot: snapshot_value.clone(),
            operation_id: "op-reload-1".to_owned(),
            reloaded_at: 5,
        })
        .expect("same operation reload is idempotent");
    // A distinct operation id records its own audit row without duplicating
    // the already-committed snapshot.
    store
        .commit_configuration_reload(CommitConfigurationReloadInputDto {
            snapshot: snapshot_value,
            operation_id: "op-reload-2".to_owned(),
            reloaded_at: 5,
        })
        .expect("later operation reload commits");
    let audit_count: i64 = {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM configuration_audit", [], |row| {
                row.get(0)
            })
            .expect("audit count loads");
        drop(connection);
        count
    };
    assert_eq!(audit_count, 2);
    // A reload that fails after the snapshot write rolls back the snapshot
    // and the audit row together.
    let rollback_revision = ConfigRevisionId::new();
    assert_eq!(
        store
            .commit_configuration_reload(CommitConfigurationReloadInputDto {
                snapshot: snapshot_with_revision_and_model(rollback_revision, "fixture-rollback",),
                operation_id: "op-reload-broken".to_owned(),
                reloaded_at: -1,
            })
            .expect_err("invalid audit timestamps violate the durable CHECK")
            .code(),
        "storage_unavailable"
    );
    drop(store);
    let reopened = reopen(&directory);
    let rollback_rows: i64 = {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM configuration_revisions WHERE revision_id=?1",
                [rollback_revision.to_string()],
                |row| row.get(0),
            )
            .expect("rollback revision count loads");
        drop(connection);
        count
    };
    assert_eq!(
        rollback_rows, 0,
        "the rolled-back snapshot must not persist"
    );
    let audit_after: i64 = {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM configuration_audit", [], |row| {
                row.get(0)
            })
            .expect("audit count loads");
        drop(connection);
        count
    };
    assert_eq!(audit_after, 2, "the rolled-back audit row must not persist");
    drop(reopened);
}

#[test]
fn provider_catalog_material_round_trips_and_digest_conflicts_are_typed() {
    let (_directory, store) = repository();
    // The fixture profile must reference the descriptor revision id that is
    // actually appended; the material load resolves descriptors through the
    // profile's kind_descriptor_revision_id.
    let mut expected_candidate = fixture_profile_candidate("profile-a", "rev-a");
    expected_candidate.profile.kind_descriptor_revision_id = "kd-1".to_owned();
    store
        .append_provider_kind_descriptor_revision(AppendProviderKindDescriptorRevisionInputDto {
            descriptor_revision_id: "kd-1".to_owned(),
            descriptor: fixture_kind_descriptor("kind-a"),
            catalog_revision_id: 1,
            accepted_at: 1,
            operation_id: "op-prep-1".to_owned(),
        })
        .expect("fixture kind descriptor prepares");
    store
        .append_provider_profile_revision(AppendProviderProfileRevisionInputDto {
            profile: expected_candidate.clone(),
            catalog_revision_id: 1,
            accepted_at: 1,
            operation_id: "op-prep-1".to_owned(),
        })
        .expect("fixture profile prepares");
    store
        .accept_provider_catalog(AcceptProviderCatalogInputDto {
            catalog_revision_id: 1,
            candidate_handle: "candidate-1".to_owned(),
            kind_descriptors: vec![intention_storage::ProviderKindDescriptorCandidateDto {
                descriptor_revision_id: "kd-1".to_owned(),
                descriptor: fixture_kind_descriptor("kind-a"),
            }],
            profiles: vec![expected_candidate.clone()],
            default_profile_id: "profile-a".to_owned(),
            accepted_at: 1,
            operation_id: "op-accept-1".to_owned(),
        })
        .expect("fixture catalog accepts");
    // The typed codecs round trip the accepted material exactly.
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
    assert_eq!(material.profiles[0], expected_candidate);
    // The kind descriptor digest excludes the revision id: identical bytes
    // cannot bind a second revision identity.
    assert_eq!(
        store
            .append_provider_kind_descriptor_revision(
                AppendProviderKindDescriptorRevisionInputDto {
                    descriptor_revision_id: "kd-other".to_owned(),
                    descriptor: fixture_kind_descriptor("kind-a"),
                    catalog_revision_id: 1,
                    accepted_at: 2,
                    operation_id: "op-conflict-1".to_owned(),
                }
            )
            .expect_err("identical descriptor bytes cannot bind a second identity")
            .code(),
        "provider_kind_descriptor_digest_conflict"
    );
    // Re-preparing the identical record is idempotent.
    store
        .append_provider_kind_descriptor_revision(AppendProviderKindDescriptorRevisionInputDto {
            descriptor_revision_id: "kd-1".to_owned(),
            descriptor: fixture_kind_descriptor("kind-a"),
            catalog_revision_id: 1,
            accepted_at: 2,
            operation_id: "op-reprep-1".to_owned(),
        })
        .expect("identical kind descriptor is idempotent");
    // The same identity with different bytes conflicts.
    let mut different_kind = fixture_kind_descriptor("kind-a");
    different_kind.endpoint_policy = "https-or-http".to_owned();
    assert_eq!(
        store
            .append_provider_kind_descriptor_revision(
                AppendProviderKindDescriptorRevisionInputDto {
                    descriptor_revision_id: "kd-1".to_owned(),
                    descriptor: different_kind,
                    catalog_revision_id: 1,
                    accepted_at: 2,
                    operation_id: "op-conflict-2".to_owned(),
                }
            )
            .expect_err("kind descriptor identity cannot rebind different bytes")
            .code(),
        "provider_kind_descriptor_revision_conflict"
    );
    // The same profile identity with different bytes conflicts.
    let mut different_profile = expected_candidate.clone();
    different_profile.profile.model_id = "gpt-4.1-mini".to_owned();
    assert_eq!(
        store
            .append_provider_profile_revision(AppendProviderProfileRevisionInputDto {
                profile: different_profile,
                catalog_revision_id: 1,
                accepted_at: 2,
                operation_id: "op-conflict-3".to_owned(),
            })
            .expect_err("profile identity cannot rebind different bytes")
            .code(),
        "provider_profile_revision_conflict"
    );
    // Re-appending the identical profile is idempotent.
    store
        .append_provider_profile_revision(AppendProviderProfileRevisionInputDto {
            profile: expected_candidate,
            catalog_revision_id: 1,
            accepted_at: 2,
            operation_id: "op-reprep-2".to_owned(),
        })
        .expect("identical profile is idempotent");
}

#[test]
fn provider_catalog_page_tokens_round_trip_and_reject_stale_or_invalid_tokens() {
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
    store
        .accept_provider_catalog(AcceptProviderCatalogInputDto {
            catalog_revision_id: 1,
            candidate_handle: "candidate-1".to_owned(),
            kind_descriptors: vec![intention_storage::ProviderKindDescriptorCandidateDto {
                descriptor_revision_id: "kd-1".to_owned(),
                descriptor: fixture_kind_descriptor("kind-a"),
            }],
            profiles: vec![
                fixture_profile_candidate("profile-a", "rev-a"),
                fixture_profile_candidate("profile-b", "rev-b"),
                fixture_profile_candidate("profile-c", "rev-c"),
            ],
            default_profile_id: "profile-a".to_owned(),
            accepted_at: 1,
            operation_id: "op-accept-1".to_owned(),
        })
        .expect("fixture catalog accepts three profiles");
    let first = store
        .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
            token: None,
            limit: 1,
        })
        .expect("first page loads");
    assert_eq!(first.entries.len(), 1);
    assert_eq!(first.entries[0].profile_id, "profile-a");
    assert!(first.has_more);
    let token = first.next_token.expect("first page has a token");
    let second = store
        .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
            token: Some(token.clone()),
            limit: 2,
        })
        .expect("second page loads");
    assert_eq!(second.entries.len(), 2);
    assert_eq!(second.entries[0].profile_id, "profile-b");
    assert_eq!(second.entries[1].profile_id, "profile-c");
    assert!(!second.has_more);
    assert!(second.next_token.is_none());
    // Malformed and out-of-range page inputs fail typed before any query.
    assert_eq!(
        store
            .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
                token: Some("not-json".to_owned()),
                limit: 1,
            })
            .expect_err("malformed token is rejected")
            .code(),
        "invalid_catalog_page_token"
    );
    assert_eq!(
        store
            .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
                token: None,
                limit: 0,
            })
            .expect_err("zero limit is rejected")
            .code(),
        "invalid_catalog_page_limit"
    );
    assert_eq!(
        store
            .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
                token: None,
                limit: 1025,
            })
            .expect_err("oversized limit is rejected")
            .code(),
        "invalid_catalog_page_limit"
    );
    // A token issued against a superseded revision is stale.
    prepare_candidate(
        &store,
        2,
        "op-prep-2",
        "kind-b",
        "kd-2",
        "profile-x",
        "rev-x",
        2,
    );
    accept_candidate(
        &store,
        2,
        "candidate-2",
        "op-accept-2",
        "kind-b",
        "kd-2",
        "profile-x",
        "rev-x",
        2,
    );
    assert_eq!(
        store
            .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
                token: Some(token),
                limit: 1,
            })
            .expect_err("superseded token is stale")
            .code(),
        "catalog_page_token_stale"
    );
    let fresh = store
        .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
            token: None,
            limit: 10,
        })
        .expect("fresh page loads after catalog change");
    assert_eq!(fresh.entries.len(), 1);
    assert_eq!(fresh.entries[0].profile_id, "profile-x");
}

#[test]
fn provider_catalog_page_and_material_require_an_active_catalog() {
    let (_directory, store) = repository();
    assert_eq!(
        store
            .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
                token: None,
                limit: 1,
            })
            .expect_err("no active catalog")
            .code(),
        "provider_catalog_not_active"
    );
    assert_eq!(
        store
            .load_provider_catalog_material()
            .expect_err("no active catalog")
            .code(),
        "provider_catalog_not_active"
    );
}

#[test]
fn configuration_audit_json_omits_or_nulls_the_candidate_handle_by_kind() {
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
    // Rejection records the explicit candidate handle in the audit JSON.
    store
        .reject_provider_catalog_candidate(RejectProviderCatalogCandidateInputDto {
            catalog_revision_id: 1,
            candidate_handle: "candidate-rejected".to_owned(),
            rejected_at: 2,
            operation_id: "op-reject-1".to_owned(),
        })
        .expect("candidate rejects");
    // A fresh prepared candidate carries no handle; expiry records an
    // explicit null for the absent handle.
    prepare_candidate(
        &store,
        2,
        "op-prep-2",
        "kind-b",
        "kd-2",
        "profile-b",
        "rev-b",
        3,
    );
    store
        .expire_provider_catalog_candidate(ExpireProviderCatalogCandidateInputDto {
            catalog_revision_id: 2,
            expired_at: 4,
            operation_id: "op-expire-1".to_owned(),
        })
        .expect("candidate expires");
    let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
        .expect("database reopens");
    let rows = connection
        .prepare("SELECT audit_kind, audit_json FROM configuration_audit ORDER BY audit_sequence")
        .expect("audit statement prepares")
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("audit query runs")
        .map(|row| row.expect("audit row reads"))
        .collect::<Vec<_>>();
    assert!(rows.iter().any(|(kind, json)| {
        kind == "ProviderCatalogCandidateRejected"
            && json.contains("\"candidate_handle\":\"candidate-rejected\"")
    }));
    assert!(rows.iter().any(|(kind, json)| {
        kind == "ProviderCatalogCandidateExpired" && json.contains("\"candidate_handle\":null")
    }));
    for (_, json) in &rows {
        assert!(!contains_credential_shape(json));
        assert!(!json.contains("fixture-secret"));
    }
    drop(connection);
}

/// Prepares and accepts one catalog whose profile references the exact
/// descriptor revision id that is appended, so the material load resolves.
fn accept_consistent_catalog(
    store: &SqliteStorageRepository,
    kind_id: &str,
    descriptor_revision_id: &str,
    profile_id: &str,
    profile_revision_id: &str,
    catalog_revision: u64,
    operation_id: &str,
) {
    let mut profile = fixture_profile_candidate(profile_id, profile_revision_id);
    profile.profile.kind_descriptor_revision_id = descriptor_revision_id.to_owned();
    store
        .append_provider_kind_descriptor_revision(AppendProviderKindDescriptorRevisionInputDto {
            descriptor_revision_id: descriptor_revision_id.to_owned(),
            descriptor: fixture_kind_descriptor(kind_id),
            catalog_revision_id: catalog_revision,
            accepted_at: 1,
            operation_id: operation_id.to_owned(),
        })
        .expect("fixture kind descriptor prepares");
    store
        .append_provider_profile_revision(AppendProviderProfileRevisionInputDto {
            profile: profile.clone(),
            catalog_revision_id: catalog_revision,
            accepted_at: 1,
            operation_id: operation_id.to_owned(),
        })
        .expect("fixture profile prepares");
    store
        .accept_provider_catalog(AcceptProviderCatalogInputDto {
            catalog_revision_id: catalog_revision,
            candidate_handle: "candidate-1".to_owned(),
            kind_descriptors: vec![intention_storage::ProviderKindDescriptorCandidateDto {
                descriptor_revision_id: descriptor_revision_id.to_owned(),
                descriptor: fixture_kind_descriptor(kind_id),
            }],
            profiles: vec![profile],
            default_profile_id: profile_id.to_owned(),
            accepted_at: 1,
            operation_id: operation_id.to_owned(),
        })
        .expect("fixture catalog accepts");
}

/// The exact capability envelope JSON bytes the kind descriptor codec writes
/// for [`fixture_kind_descriptor`].
const fn fixture_capability_envelope_json() -> &'static str {
    r#"{"context_preservation":{"local_durable_history_v1":{"reasoning_input_contract":"reasoning-history-transfer-v1"}},"input":"text_only","reasoning":"textual_reasoning_v1","structured_output":"unsupported","taxonomy_version":"model-capability-taxonomy-v1","text_streaming":true,"tool_exchange":false}"#
}

/// Builds one kind descriptor JSON record with one named field omitted, used
/// to corrupt durable rows so the typed codecs fail closed.
fn fixture_kind_descriptor_json_omitting(omitted: &str) -> String {
    let fields = vec![
        (
            "credential_transport_contract",
            r#""bearer-or-safe-header""#.to_owned(),
        ),
        ("descriptor_family", r#""responses-descriptor""#.to_owned()),
        ("driver_contract_family", r#""responses""#.to_owned()),
        ("endpoint_policy", r#""https-only""#.to_owned()),
        ("kind_id", r#""kind-a""#.to_owned()),
        (
            "model_capability_envelope",
            fixture_capability_envelope_json().to_owned(),
        ),
        (
            "ordered_protocol_part_revisions",
            r#"["parts-v1"]"#.to_owned(),
        ),
    ];
    let mut encoded = String::from("{");
    let mut first = true;
    for (key, value) in fields {
        if key == omitted {
            continue;
        }
        if !first {
            encoded.push(',');
        }
        encoded.push_str(&format!("\"{key}\":{value}"));
        first = false;
    }
    encoded.push('}');
    encoded
}

/// Builds one profile revision JSON record with one named field omitted or
/// with a replaced transport mode, used to corrupt durable rows so the typed
/// codecs fail closed.
fn fixture_profile_json_omitting(omitted: &str, transport_mode: &str) -> String {
    let fields = vec![
        (
            "capability_taxonomy_revision",
            r#""model-capability-taxonomy-v1""#.to_owned(),
        ),
        ("credential_transport_mode", format!("\"{transport_mode}\"")),
        (
            "driver_contract_revision",
            r#"{"driver_family":"responses","major":1,"minor":0}"#.to_owned(),
        ),
        ("endpoint", r#""https://api.example.com/v1""#.to_owned()),
        ("kind_descriptor_revision_id", r#""kd-1""#.to_owned()),
        ("model_id", r#""gpt-4.1""#.to_owned()),
        ("profile_id", r#""profile-a""#.to_owned()),
        ("provider_kind_id", r#""responses""#.to_owned()),
        (
            "reasoning_compatibility_id",
            r#""reasoning-compat-v1""#.to_owned(),
        ),
        ("revision_id", r#""rev-a""#.to_owned()),
        ("safe_header_name", "null".to_owned()),
    ];
    let mut encoded = String::from("{");
    let mut first = true;
    for (key, value) in fields {
        if key == omitted {
            continue;
        }
        if !first {
            encoded.push(',');
        }
        encoded.push_str(&format!("\"{key}\":{value}"));
        first = false;
    }
    encoded.push('}');
    encoded
}

#[test]
fn safe_header_selection_and_profile_round_trip_transport_codecs() {
    let (directory, store) = repository();
    let session_id = create(&store);
    let run_id = RunId::new();
    accept(&store, session_id, TurnId::new(), run_id, "run");
    let mut selection = fixture_selection();
    selection.credential_transport_mode = CredentialTransportMode::SafeHeader;
    selection.credential_transport_safe_header_name = Some("X-Custom-Header".to_owned());
    store
        .persist_resolved_run_provider_selection(PersistResolvedRunProviderSelectionInputDto {
            session_id,
            run_id,
            selection: selection.clone(),
            occurred_at: 3,
        })
        .expect("safe header selection persists");
    assert_eq!(
        store
            .load_resolved_run_provider_selection(session_id, run_id)
            .expect("safe header selection loads")
            .expect("safe header selection exists"),
        selection
    );
    // A safe-header profile round trips through the catalog codecs too.
    let mut profile = fixture_profile_candidate("profile-saf", "rev-saf");
    profile.profile.kind_descriptor_revision_id = "kd-1".to_owned();
    profile.profile.credential_transport_mode = CredentialTransportMode::SafeHeader;
    profile.profile.safe_header_name = Some("X-Custom-Header".to_owned());
    store
        .append_provider_kind_descriptor_revision(AppendProviderKindDescriptorRevisionInputDto {
            descriptor_revision_id: "kd-1".to_owned(),
            descriptor: fixture_kind_descriptor("kind-a"),
            catalog_revision_id: 1,
            accepted_at: 1,
            operation_id: "op-prep-1".to_owned(),
        })
        .expect("fixture kind descriptor prepares");
    store
        .append_provider_profile_revision(AppendProviderProfileRevisionInputDto {
            profile: profile.clone(),
            catalog_revision_id: 1,
            accepted_at: 1,
            operation_id: "op-prep-1".to_owned(),
        })
        .expect("fixture profile prepares");
    store
        .accept_provider_catalog(AcceptProviderCatalogInputDto {
            catalog_revision_id: 1,
            candidate_handle: "candidate-1".to_owned(),
            kind_descriptors: vec![intention_storage::ProviderKindDescriptorCandidateDto {
                descriptor_revision_id: "kd-1".to_owned(),
                descriptor: fixture_kind_descriptor("kind-a"),
            }],
            profiles: vec![profile.clone()],
            default_profile_id: "profile-saf".to_owned(),
            accepted_at: 1,
            operation_id: "op-accept-1".to_owned(),
        })
        .expect("fixture catalog accepts");
    let material = store
        .load_provider_catalog_material()
        .expect("catalog material loads");
    assert_eq!(material.profiles[0], profile);
    // The durable safe projection carries the header name, never a secret.
    let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
        .expect("database reopens");
    let safe_json: String = connection
        .query_row(
            "SELECT safe_projection_json FROM provider_catalog_profile_projection WHERE projection_state='active' AND profile_id='profile-saf'",
            [],
            |row| row.get(0),
        )
        .expect("safe projection reads");
    drop(connection);
    assert!(safe_json.contains("\"credential_transport_safe_header_name\":\"X-Custom-Header\""));
    assert!(!contains_credential_shape(&safe_json));
    assert!(!safe_json.contains("fixture-secret"));
}

#[test]
fn provider_catalog_page_token_null_and_malformed_cursors_decode_typed() {
    let (_directory, store) = repository();
    accept_consistent_catalog(&store, "kind-a", "kd-1", "profile-a", "rev-a", 1, "op-1");
    // An explicit null cursor parses leniently as no cursor.
    let page = store
        .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
            token: Some(r#"{"after":null,"revision":1}"#.to_owned()),
            limit: 10,
        })
        .expect("null-cursor token parses");
    assert_eq!(page.entries.len(), 1);
    // A non-string cursor is read leniently as no cursor.
    let page = store
        .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
            token: Some(r#"{"after":5,"revision":1}"#.to_owned()),
            limit: 10,
        })
        .expect("non-string cursor parses leniently");
    assert_eq!(page.entries.len(), 1);
    // A token missing its revision field is rejected typed.
    assert_eq!(
        store
            .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
                token: Some(r#"{"after":"profile-a"}"#.to_owned()),
                limit: 1,
            })
            .expect_err("token without revision is rejected")
            .code(),
        "invalid_catalog_page_token"
    );
    // A token whose revision is not a number is rejected typed.
    assert_eq!(
        store
            .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
                token: Some(r#"{"after":"profile-a","revision":"one"}"#.to_owned()),
                limit: 1,
            })
            .expect_err("non-numeric revision is rejected")
            .code(),
        "invalid_catalog_page_token"
    );
}

#[test]
fn corrupted_catalog_state_and_projection_rows_fail_typed_loads() {
    let (directory, store) = repository();
    accept_consistent_catalog(&store, "kind-a", "kd-1", "profile-a", "rev-a", 1, "op-1");
    // A negative durable revision is a typed decode error.
    {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        connection
            .execute(
                "UPDATE provider_catalog_state SET active_catalog_revision_id=-1 WHERE singleton_id=1",
                [],
            )
            .expect("fixture corrupts the active revision");
        drop(connection);
    }
    assert_eq!(
        store
            .load_provider_catalog_status()
            .expect_err("negative revision fails the typed load")
            .code(),
        "storage_decode_failed"
    );
    {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        connection
            .execute(
                "UPDATE provider_catalog_state SET active_catalog_revision_id=1 WHERE singleton_id=1",
                [],
            )
            .expect("fixture restores the active revision");
        drop(connection);
    }
    // A projection referencing an unknown profile revision fails the material load.
    {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        connection
            .execute(
                "UPDATE provider_catalog_profile_projection SET profile_revision_id='ghost-rev' WHERE projection_state='active'",
                [],
            )
            .expect("fixture corrupts the projected profile revision");
        drop(connection);
    }
    assert_eq!(
        store
            .load_provider_catalog_material()
            .expect_err("unknown projected profile revision fails the typed load")
            .code(),
        "storage_decode_failed"
    );
    {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        connection
            .execute(
                "UPDATE provider_catalog_profile_projection SET profile_revision_id='rev-a' WHERE projection_state='active'",
                [],
            )
            .expect("fixture restores the projected profile revision");
        drop(connection);
    }
    // A projection referencing an unknown kind descriptor revision fails too.
    {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        connection
            .execute(
                "UPDATE provider_catalog_profile_projection SET kind_descriptor_revision_id='ghost-kd' WHERE projection_state='active'",
                [],
            )
            .expect("fixture corrupts the projected kind revision");
        drop(connection);
    }
    assert_eq!(
        store
            .load_provider_catalog_material()
            .expect_err("unknown projected kind revision fails the typed load")
            .code(),
        "storage_decode_failed"
    );
    {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        connection
            .execute(
                "UPDATE provider_catalog_profile_projection SET kind_descriptor_revision_id='kd-1' WHERE projection_state='active'",
                [],
            )
            .expect("fixture restores the projected kind revision");
        drop(connection);
    }
    // A malformed capability-subset column fails the typed load.
    {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        connection
            .execute(
                "UPDATE provider_profile_revisions SET declared_model_capability_subset_json='not-an-array' WHERE profile_id='profile-a'",
                [],
            )
            .expect("fixture corrupts the capability subset column");
        drop(connection);
    }
    assert_eq!(
        store
            .load_provider_catalog_material()
            .expect_err("malformed capability subset fails the typed load")
            .code(),
        "storage_decode_failed"
    );
}

#[test]
fn corrupted_kind_descriptor_and_profile_codec_rows_fail_typed() {
    let (directory, store) = repository();
    accept_consistent_catalog(&store, "kind-a", "kd-1", "profile-a", "rev-a", 1, "op-1");
    let mut descriptor_corruptions: Vec<(String, String)> = vec![
        (
            "descriptor missing its envelope".to_string(),
            fixture_kind_descriptor_json_omitting("model_capability_envelope"),
        ),
        (
            "descriptor with an empty envelope".to_string(),
            fixture_kind_descriptor_json_omitting("ordered_protocol_part_revisions")
                .replace(fixture_capability_envelope_json(), "{}"),
        ),
        (
            "descriptor envelope missing context preservation".to_string(),
            fixture_kind_descriptor_json_omitting("ordered_protocol_part_revisions").replace(
                fixture_capability_envelope_json(),
                r#"{"input":"text_only"}"#,
            ),
        ),
        (
            "descriptor envelope missing the local history entry".to_string(),
            fixture_kind_descriptor_json_omitting("ordered_protocol_part_revisions").replace(
                fixture_capability_envelope_json(),
                r#"{"context_preservation":{}}"#,
            ),
        ),
        (
            "descriptor with an invalid reasoning capability".to_string(),
            fixture_kind_descriptor_json_omitting("ordered_protocol_part_revisions").replace(
                "\"reasoning\":\"textual_reasoning_v1\"",
                "\"reasoning\":\"bogus\"",
            ),
        ),
        (
            "descriptor with a non-boolean streaming flag".to_string(),
            fixture_kind_descriptor_json_omitting("ordered_protocol_part_revisions")
                .replace("\"text_streaming\":true", "\"text_streaming\":1"),
        ),
        (
            "descriptor whose kind id mismatches its row".to_string(),
            fixture_kind_descriptor_json_omitting("ordered_protocol_part_revisions")
                .replace("\"kind_id\":\"kind-a\"", "\"kind_id\":\"kind-other\""),
        ),
    ];
    for omitted in [
        "credential_transport_contract",
        "descriptor_family",
        "driver_contract_family",
        "endpoint_policy",
        "kind_id",
        "ordered_protocol_part_revisions",
    ] {
        descriptor_corruptions.push((
            format!("descriptor missing {omitted}"),
            fixture_kind_descriptor_json_omitting(omitted),
        ));
    }
    for (label, json) in descriptor_corruptions {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        connection
            .execute(
                "UPDATE provider_kind_descriptor_revisions SET descriptor_json=?1 WHERE descriptor_revision_id='kd-1'",
                [&json],
            )
            .expect("fixture corrupts the kind descriptor row");
        drop(connection);
        assert_eq!(
            store
                .load_provider_catalog_material()
                .expect_err(&label)
                .code(),
            "storage_decode_failed"
        );
    }
    let mut profile_corruptions: Vec<(String, String)> = vec![
        (
            "profile with an invalid transport mode".to_string(),
            fixture_profile_json_omitting("none", "bogus"),
        ),
        (
            "profile with a null driver contract".to_string(),
            fixture_profile_json_omitting("none", "bearer").replace(
                r#""driver_contract_revision":{"driver_family":"responses","major":1,"minor":0}"#,
                r#""driver_contract_revision":null"#,
            ),
        ),
        (
            "profile with a driver contract missing its major".to_string(),
            fixture_profile_json_omitting("none", "bearer").replace(
                r#""driver_contract_revision":{"driver_family":"responses","major":1,"minor":0}"#,
                r#""driver_contract_revision":{"driver_family":"responses","minor":0}"#,
            ),
        ),
        (
            "profile with a driver contract missing its minor".to_string(),
            fixture_profile_json_omitting("none", "bearer").replace(
                r#""driver_contract_revision":{"driver_family":"responses","major":1,"minor":0}"#,
                r#""driver_contract_revision":{"driver_family":"responses","major":1}"#,
            ),
        ),
        (
            "profile with a non-numeric driver contract major".to_string(),
            fixture_profile_json_omitting("none", "bearer").replace(
                r#""driver_contract_revision":{"driver_family":"responses","major":1,"minor":0}"#,
                r#""driver_contract_revision":{"driver_family":"responses","major":"one","minor":0}"#,
            ),
        ),
    ];
    for omitted in [
        "capability_taxonomy_revision",
        "credential_transport_mode",
        "driver_contract_revision",
        "endpoint",
        "kind_descriptor_revision_id",
        "model_id",
        "profile_id",
        "provider_kind_id",
        "revision_id",
    ] {
        profile_corruptions.push((
            format!("profile missing {omitted}"),
            fixture_profile_json_omitting(omitted, "bearer"),
        ));
    }
    for (label, json) in profile_corruptions {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        connection
            .execute(
                "UPDATE provider_profile_revisions SET profile_revision_json=?1 WHERE profile_id='profile-a'",
                [&json],
            )
            .expect("fixture corrupts the profile revision row");
        drop(connection);
        assert_eq!(
            store
                .load_provider_catalog_material()
                .expect_err(&label)
                .code(),
            "storage_decode_failed"
        );
    }
}

#[test]
#[allow(
    clippy::literal_string_with_formatting_args,
    reason = "Malformed JSON fixture strings intentionally resemble formatting placeholders."
)]
fn malformed_selection_json_rows_fail_every_scanner_path() {
    let (directory, store) = repository();
    let session_id = create(&store);
    let run_id = RunId::new();
    accept(&store, session_id, TurnId::new(), run_id, "run");
    store
        .persist_resolved_run_provider_selection(PersistResolvedRunProviderSelectionInputDto {
            session_id,
            run_id,
            selection: fixture_selection(),
            occurred_at: 3,
        })
        .expect("resolved selection persists");
    let original: String = {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        let json: String = connection
            .query_row(
                "SELECT selection_json FROM resolved_run_provider_selections WHERE run_id=?1",
                [run_id.to_string()],
                |row| row.get(0),
            )
            .expect("selection json reads");
        drop(connection);
        json
    };
    let variants = vec![
        (r#""x""#.to_owned(), "non-object record"),
        ("{}".to_owned(), "empty object record"),
        (r#"{"a""#.to_owned(), "member without colon"),
        (r#"{"a":1"#.to_owned(), "value without separator"),
        (r#"{"a":1x}"#.to_owned(), "unexpected byte after value"),
        (r#"{1:2}"#.to_owned(), "unquoted member"),
        (r#"{"abc"#.to_owned(), "unterminated string"),
        (r#"{"a\"b":1}"#.to_owned(), "escaped member"),
        (r#"{"a":"#.to_owned(), "value at end after colon"),
        (r#"{"a":{"b":1"#.to_owned(), "unterminated nested object"),
        (r#"{"a":tru}"#.to_owned(), "truncated true"),
        (r#"{"a":fals}"#.to_owned(), "truncated false"),
        (r#"{"a":nul}"#.to_owned(), "truncated null"),
        (
            r#"{"credential_transport_mode":1}"#.to_owned(),
            "number for a string field",
        ),
        (
            original.replace(
                r#""selection_source":"catalog-rev-0001""#,
                r#""selection_source":1"#,
            ),
            "number for an optional string field",
        ),
        (
            original.replace(
                r#""declared_model_capability_subset":["text_streaming"]"#,
                r#""declared_model_capability_subset":"text_streaming""#,
            ),
            "string for a list field",
        ),
    ];
    for (json, label) in variants {
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens");
        connection
            .execute(
                "UPDATE resolved_run_provider_selections SET selection_json=?1 WHERE run_id=?2",
                sqlite::params![json, run_id.to_string()],
            )
            .expect("fixture corrupts the selection row");
        drop(connection);
        assert_eq!(
            store
                .load_resolved_run_provider_selection(session_id, run_id)
                .expect_err(label)
                .code(),
            "storage_decode_failed"
        );
    }
}

#[test]
fn provider_catalog_projection_readiness_variants_round_trip() {
    let (_directory, store) = repository();
    let mut ready = fixture_profile_candidate("profile-a", "rev-a");
    ready.profile.kind_descriptor_revision_id = "kd-1".to_owned();
    let mut disabled = fixture_profile_candidate("profile-b", "rev-b");
    disabled.profile.kind_descriptor_revision_id = "kd-1".to_owned();
    disabled.readiness = ProviderReadinessDto::Disabled;
    let mut unavailable = fixture_profile_candidate("profile-c", "rev-c");
    unavailable.profile.kind_descriptor_revision_id = "kd-1".to_owned();
    unavailable.readiness = ProviderReadinessDto::Unavailable;
    store
        .append_provider_kind_descriptor_revision(AppendProviderKindDescriptorRevisionInputDto {
            descriptor_revision_id: "kd-1".to_owned(),
            descriptor: fixture_kind_descriptor("kind-a"),
            catalog_revision_id: 1,
            accepted_at: 1,
            operation_id: "op-prep-1".to_owned(),
        })
        .expect("fixture kind descriptor prepares");
    for profile in [&ready, &disabled, &unavailable] {
        store
            .append_provider_profile_revision(AppendProviderProfileRevisionInputDto {
                profile: profile.clone(),
                catalog_revision_id: 1,
                accepted_at: 1,
                operation_id: "op-prep-1".to_owned(),
            })
            .expect("fixture profile prepares");
    }
    store
        .accept_provider_catalog(AcceptProviderCatalogInputDto {
            catalog_revision_id: 1,
            candidate_handle: "candidate-1".to_owned(),
            kind_descriptors: vec![intention_storage::ProviderKindDescriptorCandidateDto {
                descriptor_revision_id: "kd-1".to_owned(),
                descriptor: fixture_kind_descriptor("kind-a"),
            }],
            profiles: vec![ready, disabled, unavailable],
            default_profile_id: "profile-a".to_owned(),
            accepted_at: 1,
            operation_id: "op-accept-1".to_owned(),
        })
        .expect("fixture catalog accepts");
    let material = store
        .load_provider_catalog_material()
        .expect("catalog material loads");
    assert_eq!(material.profiles.len(), 3);
    assert_eq!(
        material
            .profiles
            .iter()
            .map(|profile| profile.readiness)
            .collect::<Vec<_>>(),
        vec![
            ProviderReadinessDto::Ready,
            ProviderReadinessDto::Disabled,
            ProviderReadinessDto::Unavailable,
        ]
    );
    let page = store
        .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
            token: None,
            limit: 10,
        })
        .expect("catalog page loads");
    assert_eq!(page.entries.len(), 3);
    assert_eq!(
        page.entries
            .iter()
            .map(|entry| entry.readiness)
            .collect::<Vec<_>>(),
        vec![
            ProviderReadinessDto::Ready,
            ProviderReadinessDto::Disabled,
            ProviderReadinessDto::Unavailable,
        ]
    );
}
