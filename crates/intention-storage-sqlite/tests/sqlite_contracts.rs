#![allow(
    clippy::expect_used,
    reason = "SQLite contract fixtures use expect for precise test diagnostics."
)]

use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    CreateSessionCommandDto, RemoveQueuedTurnCommandDto, RunModeDto, RunStatusDto, WorkspaceRootDto,
};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, CreateSessionInputDto,
    RecoverUnfinishedRunsInputDto, RemoveQueuedTurnInputDto, StorageRepositoryDto,
    TransitionRunInputDto,
};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use intention_types::{
    ConfigRevisionId, ProjectId, RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId,
    TimestampDto, TurnId, WorkspaceId,
};
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
