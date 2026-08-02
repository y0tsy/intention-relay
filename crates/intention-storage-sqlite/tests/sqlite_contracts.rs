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
    PromotedQueuedTurnInputDto, RecoverUnfinishedRunsInputDto, RemoveQueuedTurnInputDto,
    StorageRepositoryDto, TransitionRunInputDto,
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
                WorkspaceRootDto::parse("/workspace/storage-contract").expect("absolute root"),
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
fn terminal_transition_promotes_queue_and_recovery_interrupts_unfinished_runs() {
    let (_directory, store) = repository();
    let session = create(&store);
    let active_turn = TurnId::new();
    let active_run = RunId::new();
    accept(&store, session, active_turn, active_run, "active");
    let queued_turn = TurnId::new();
    let queued_run = RunId::new();
    accept(&store, session, queued_turn, queued_run, "queued");
    let transition = store
        .transition_run(TransitionRunInputDto::new(
            session,
            active_run,
            RunStatusDto::Failed,
            time(4),
            Some(PromotedQueuedTurnInputDto::new(queued_turn)),
        ))
        .expect("terminal transition promotes queued turn");
    assert_eq!(transition.events().len(), 2);
    assert_eq!(
        store
            .load_session_snapshot(session)
            .expect("snapshot loads")
            .active_run()
            .expect("promoted run is active")
            .run_id(),
        queued_run
    );
    let recovered = store
        .recover_unfinished_runs(RecoverUnfinishedRunsInputDto::new(time(5)))
        .expect("recovery commits");
    assert_eq!(recovered.len(), 1);
    assert!(
        store
            .load_session_snapshot(session)
            .expect("snapshot loads")
            .active_run()
            .is_none()
    );
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
            Some(PromotedQueuedTurnInputDto::new(queued_turn)),
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
