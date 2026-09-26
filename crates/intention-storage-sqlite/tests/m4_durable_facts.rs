#![allow(
    clippy::expect_used,
    reason = "M4 SQLite outcome fixtures use expect for precise diagnostics."
)]

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    CreateSessionCommandDto, DomainEventDto, ModelRunFactInputDto, RunEventCursorDto, RunModeDto,
    RunStatusDto, WorkspaceRootDto,
};
use intention_storage::{
    AcceptUserTurnInputDto, AppendModelRunFactsInputDto, CreateSessionInputDto,
    StorageRepositoryDto, TransitionRunInputDto,
};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use intention_types::{ProjectId, RunId, SessionId, TimestampDto, TurnId, WorkspaceId};
use tempfile::TempDir;

fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture time is valid")
}

fn snapshot() -> ConfigSnapshotDto {
    serde_json::from_str(include_str!(
        "../../intention-config/tests/fixtures/config-snapshot-v1.json"
    ))
    .expect("safe snapshot decodes")
}

fn temporary_repository() -> (TempDir, SqliteStorageRepository) {
    let directory = TempDir::new().expect("temporary directory exists");
    let path = directory.path().join("storage.sqlite");
    let repository = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(path.to_string_lossy().into_owned())
            .expect("database path is absolute"),
    )
    .expect("database opens");
    (directory, repository)
}

fn create_started_run(repository: &SqliteStorageRepository) -> (SessionId, RunId) {
    let session_id = SessionId::new();
    repository
        .create_session(CreateSessionInputDto::new(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                WorkspaceRootDto::parse(
                    std::env::temp_dir()
                        .join("intention-m4-facts")
                        .to_string_lossy()
                        .into_owned(),
                )
                .expect("workspace root is absolute"),
                RunModeDto::Build,
            ),
            time(1),
        ))
        .expect("session creates");
    let run_id = RunId::new();
    repository
        .accept_user_turn(
            AcceptUserTurnInputDto::new(
                session_id,
                TurnId::new(),
                "turn",
                run_id,
                snapshot(),
                time(2),
            )
            .expect("turn input is valid"),
        )
        .expect("turn starts");
    (session_id, run_id)
}

fn append(
    repository: &SqliteStorageRepository,
    session_id: SessionId,
    run_id: RunId,
    expected: u64,
    facts: Vec<ModelRunFactInputDto>,
) -> intention_storage::AppendModelRunFactsOutcomeDto {
    repository
        .append_model_run_facts(
            AppendModelRunFactsInputDto::new(
                session_id,
                run_id,
                RunEventCursorDto::new(expected),
                facts,
                None,
                time(3),
            )
            .expect("append input is valid"),
        )
        .expect("facts append")
}

#[test]
fn durable_model_facts_are_replayed_with_cursor_boundaries_and_no_cross_session_leak() {
    let (_directory, repository) = temporary_repository();
    let (session_id, run_id) = create_started_run(&repository);
    let outcome = append(
        &repository,
        session_id,
        run_id,
        0,
        vec![
            ModelRunFactInputDto::provider_attempt_started(1).expect("attempt is valid"),
            ModelRunFactInputDto::assistant_content_appended(
                intention_types::AssistantTurnId::new(),
                "hello",
            )
            .expect("content is valid"),
        ],
    );
    assert_eq!(outcome.cursor().value(), 2);
    let replay = repository
        .load_current_run_replay(session_id, run_id)
        .expect("current replay loads");
    assert_eq!(replay.snapshot().cursor().value(), 2);
    assert!(replay.tail().facts().is_empty());
    let tail = repository
        .load_run_tail(session_id, run_id, RunEventCursorDto::new(0))
        .expect("run tail loads");
    assert_eq!(tail.facts().len(), 2);
    assert_eq!(tail.next_after_cursor().value(), 2);
    for (count, expected_more) in [(257_usize, true), (256_usize, false)] {
        let (_directory, bounded_repository) = temporary_repository();
        let (bounded_session_id, bounded_run_id) = create_started_run(&bounded_repository);
        let facts = (0..count)
            .map(|_| ModelRunFactInputDto::reasoning_delta_recorded("x").expect("fact is valid"))
            .collect();
        append(
            &bounded_repository,
            bounded_session_id,
            bounded_run_id,
            0,
            facts,
        );
        let page = bounded_repository
            .load_run_tail(
                bounded_session_id,
                bounded_run_id,
                RunEventCursorDto::new(0),
            )
            .expect("bounded tail loads");
        assert_eq!(page.facts().len(), 256);
        assert_eq!(page.has_more(), expected_more);
    }
    assert_eq!(
        repository
            .load_run_tail(session_id, run_id, RunEventCursorDto::new(u64::MAX))
            .expect_err("overflow future cursor fails before history query")
            .code(),
        "invalid_run_event_cursor"
    );
    assert_eq!(
        repository
            .load_current_run_replay(SessionId::new(), run_id)
            .expect_err("cross-session replay fails")
            .code(),
        "run_replay_not_found"
    );
}

#[test]
fn durable_model_fact_failures_are_safe_and_terminal_runs_reject_new_facts() {
    let (_directory, repository) = temporary_repository();
    let (session_id, run_id) = create_started_run(&repository);
    append(
        &repository,
        session_id,
        run_id,
        0,
        vec![ModelRunFactInputDto::provider_attempt_started(1).expect("attempt is valid")],
    );
    assert_eq!(
        repository
            .append_model_run_facts(
                AppendModelRunFactsInputDto::new(
                    session_id,
                    run_id,
                    RunEventCursorDto::new(0),
                    vec![ModelRunFactInputDto::usage_recorded(
                        intention_types::UsageDto::NotReported,
                    )],
                    None,
                    time(3),
                )
                .expect("input is valid"),
            )
            .expect_err("stale cursor conflicts")
            .code(),
        "run_event_cursor_conflict"
    );
    let too_large = AppendModelRunFactsInputDto::new(
        session_id,
        run_id,
        RunEventCursorDto::new(1),
        vec![
            ModelRunFactInputDto::reasoning_delta_recorded("x".repeat(512 * 1024))
                .expect("individual domain fact accepts large text before storage bound"),
        ],
        None,
        time(3),
    )
    .expect("input construction defers canonical size validation");
    assert_eq!(
        repository
            .append_model_run_facts(too_large)
            .expect_err("canonical fact cap rejects")
            .code(),
        "run_fact_too_large"
    );
    repository
        .transition_run(TransitionRunInputDto::new(
            session_id,
            run_id,
            RunStatusDto::Failed,
            time(4),
        ))
        .expect("run fails");
    assert_eq!(
        repository
            .append_model_run_facts(
                AppendModelRunFactsInputDto::new(
                    session_id,
                    run_id,
                    RunEventCursorDto::new(1),
                    vec![ModelRunFactInputDto::usage_recorded(
                        intention_types::UsageDto::NotReported,
                    )],
                    None,
                    time(5),
                )
                .expect("input is valid"),
            )
            .expect_err("terminal run rejects facts")
            .code(),
        "invalid_run_event_cursor"
    );
}

#[test]
fn terminal_model_fact_append_promotes_the_oldest_queued_turn() {
    let (_directory, repository) = temporary_repository();
    let (session_id, run_id) = create_started_run(&repository);
    let queued_turn_id = TurnId::new();
    let queued_run_id = RunId::new();
    let queued_snapshot = snapshot();
    repository
        .accept_user_turn(
            AcceptUserTurnInputDto::new(
                session_id,
                queued_turn_id,
                "queued turn",
                queued_run_id,
                queued_snapshot.clone(),
                time(3),
            )
            .expect("queued turn input is valid"),
        )
        .expect("turn queues behind active run");

    let outcome = repository
        .append_model_run_facts(
            AppendModelRunFactsInputDto::new(
                session_id,
                run_id,
                RunEventCursorDto::new(0),
                vec![ModelRunFactInputDto::usage_recorded(
                    intention_types::UsageDto::NotReported,
                )],
                Some(RunStatusDto::Failed),
                time(4),
            )
            .expect("terminal append input is valid"),
        )
        .expect("terminal facts append and promote");
    assert_eq!(
        outcome.snapshot().run_projection().status(),
        RunStatusDto::Failed
    );
    let session = repository
        .load_session_snapshot(session_id)
        .expect("session snapshot loads");
    assert_eq!(
        session.active_run().expect("queued run promoted").run_id(),
        queued_run_id
    );
    assert_eq!(
        session
            .active_run()
            .expect("queued run remains active")
            .turn_id(),
        queued_turn_id
    );
    let promoted = repository
        .load_current_run_replay(session_id, queued_run_id)
        .expect("promoted run snapshot loads");
    assert_eq!(promoted.snapshot().cursor().value(), 0);
    assert_eq!(
        promoted.snapshot().run_projection().status(),
        RunStatusDto::Starting
    );
    assert_eq!(
        promoted.snapshot().run_projection().config_revision_id(),
        queued_snapshot.revision_id()
    );
    let events = repository
        .load_tail(session_id, intention_types::SessionEventSequenceDto::new(0))
        .expect("session events load");
    let terminal_index = events
        .iter()
        .position(|event| {
            matches!(
                event.payload(),
                DomainEventDto::RunStatusChanged(status)
                    if event.run_id() == Some(run_id) && status.status() == RunStatusDto::Failed
            )
        })
        .expect("terminal status event is present");
    let promoted_index = events
        .iter()
        .position(|event| {
            matches!(
                event.payload(),
                DomainEventDto::RunStarted(started) if started.run_id() == queued_run_id
            )
        })
        .expect("promoted run event is present");
    assert!(terminal_index < promoted_index);
}
