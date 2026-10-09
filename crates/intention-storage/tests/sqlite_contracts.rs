#![allow(
    clippy::expect_used,
    reason = "SQLite contract fixtures use expect for precise test diagnostics."
)]

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{create_session, open, reopen, repository, time, workspace_root};

use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_proto::{
    ConfigRevisionId, ErrorCategoryDto, ErrorRetryDto, FinishReasonDto, IdempotencyKey, ProjectId,
    RunId, SchemaVersionDto, SessionId, ToolCallId, UsageDto, WorkspaceId,
};
use intention_proto::{
    CreateSessionCommandDto, MessageKindDto, MessageProjectionDto, PendingTurnProjectionDto,
    RemoveTurnCommandDto, RunModeDto, RunProjectionDto, RunStatusDto,
};
use intention_storage::{
    AcceptedTurnOutcomeDto, SqliteStorageRepository, StorageRepositoryDto, ToolResultEvidenceDto,
    ToolResultMetadataEntryDto, ToolResultStatusDto,
};
use tempfile::TempDir;

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
                .join("intention-storage-test.toml")
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

fn create(store: &SqliteStorageRepository) -> SessionId {
    create_session(store, &SessionId::new().to_string())
}

fn accept(
    store: &SqliteStorageRepository,
    session: SessionId,
    key: IdempotencyKey,
    run: RunId,
    text: &str,
) -> AcceptedTurnOutcomeDto {
    store
        .accept_user_turn(session, key, text, run, snapshot(), time(2))
        .expect("turn commits")
}

fn started(outcome: AcceptedTurnOutcomeDto) -> (RunProjectionDto, MessageProjectionDto) {
    match outcome {
        AcceptedTurnOutcomeDto::Started { run, message } => (run, message),
        AcceptedTurnOutcomeDto::Pending(_) => unreachable!("the turn must start a run"),
    }
}

fn pending(outcome: AcceptedTurnOutcomeDto) -> PendingTurnProjectionDto {
    match outcome {
        AcceptedTurnOutcomeDto::Pending(turn) => turn,
        AcceptedTurnOutcomeDto::Started { .. } => unreachable!("the turn must stay pending"),
    }
}

fn append(
    store: &SqliteStorageRepository,
    message: MessageProjectionDto,
    event_time: i64,
) -> MessageProjectionDto {
    store
        .append_message(message, time(event_time))
        .expect("transcript row commits")
}

#[test]
fn current_database_reopen_preserves_committed_rows() {
    let directory = TempDir::new().expect("temporary directory exists");
    let store = open(&directory);
    let session = create(&store);
    let run = RunId::new();
    let (_, message) = started(accept(&store, session, IdempotencyKey::new(), run, "kept"));
    assert_eq!(message.text(), "kept");
    drop(store);

    let reopened = reopen(&directory);
    assert_eq!(
        reopened
            .load_run_messages(session, run, 10)
            .expect("transcript loads after reopen")
            .iter()
            .map(MessageProjectionDto::text)
            .collect::<Vec<_>>(),
        vec!["kept"]
    );
    assert_eq!(
        reopened
            .load_session_projection(session)
            .expect("session projection loads after reopen")
            .active_run()
            .expect("the accepted run stays active")
            .run_id(),
        run
    );
}

#[test]
fn a_stale_schema_stamp_discards_the_database_once_and_recreates_it() {
    let directory = TempDir::new().expect("temporary directory exists");
    let store = open(&directory);
    let session = create(&store);
    drop(store);

    // A database written before the stamp existed carries the SQLite default
    // `user_version = 0`; the stamp alone decides, so its old objects are
    // discarded instead of migrated.
    let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
        .expect("database reopens for stamp mutation");
    connection
        .execute_batch(
            "CREATE TABLE session_snapshots (
               id TEXT PRIMARY KEY,
               body TEXT NOT NULL
             );
             PRAGMA user_version = 0;",
        )
        .expect("the legacy shape and the stale stamp apply");
    drop(connection);

    let recreated = open(&directory);
    assert_eq!(
        recreated
            .load_session_projection(session)
            .expect_err("the discarded database no longer holds its old rows")
            .code(),
        "storage_record_not_found"
    );
    let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
        .expect("recreated database reopens for inspection");
    let legacy: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'session_snapshots'",
            [],
            |row| row.get(0),
        )
        .expect("legacy table lookup runs");
    assert_eq!(legacy, 0, "the discarded database's old objects are gone");
    drop(connection);

    // The recreated database carries the current stamp, so the discard happens
    // exactly once and committed rows survive the next open.
    let session = create(&recreated);
    drop(recreated);
    assert_eq!(
        reopen(&directory)
            .load_session_projection(session)
            .expect("the recreated database survives the next open")
            .session_id(),
        session
    );
}

#[test]
fn create_accept_pending_idempotence_and_removal_are_typed_and_durable() {
    let (directory, store) = repository();
    let session = create(&store);
    assert_eq!(
        store
            .accept_user_turn(
                session,
                IdempotencyKey::new(),
                " ",
                RunId::new(),
                snapshot(),
                time(2),
            )
            .expect_err("blank turn content rejects")
            .code(),
        "invalid_turn_content"
    );
    let first_run = RunId::new();
    let (_, first_message) = started(accept(
        &store,
        session,
        IdempotencyKey::new(),
        first_run,
        "first",
    ));
    assert_eq!(first_message.kind(), MessageKindDto::User);
    assert_eq!(first_message.run_id(), Some(first_run));

    let second_key = IdempotencyKey::new();
    let second_run = RunId::new();
    let queued = pending(
        store
            .accept_user_turn(
                session,
                second_key,
                "second",
                second_run,
                snapshot(),
                time(3),
            )
            .expect("turn queues"),
    );
    assert_eq!(queued.content(), "second");
    assert_eq!(queued.session_id(), session);
    let replayed = pending(
        store
            .accept_user_turn(
                session,
                second_key,
                "second",
                second_run,
                snapshot(),
                time(4),
            )
            .expect("pending replay is accepted"),
    );
    assert_eq!(replayed.turn_id(), queued.turn_id());

    for (content, run, candidate) in [
        ("changed content", second_run, snapshot()),
        ("second", RunId::new(), snapshot()),
        (
            "second",
            second_run,
            snapshot_with_revision_and_model(ConfigRevisionId::new(), "fixture-other"),
        ),
    ] {
        assert_eq!(
            store
                .accept_user_turn(session, second_key, content, run, candidate, time(5))
                .expect_err("different durable content under one key conflicts")
                .code(),
            "turn_idempotency_conflict"
        );
    }

    // The queue, the active run, and the committed transcript survive a reopen.
    drop(store);
    let reopened = reopen(&directory);
    let projection = reopened
        .load_session_projection(session)
        .expect("session projection loads");
    assert_eq!(
        projection
            .active_run()
            .expect("the first turn owns the active run")
            .run_id(),
        first_run
    );
    assert_eq!(projection.pending_turns().len(), 1);
    assert_eq!(projection.pending_turns()[0].turn_id(), queued.turn_id());
    assert_eq!(
        reopened
            .load_run_messages(session, first_run, 10)
            .expect("transcript loads")
            .iter()
            .map(MessageProjectionDto::text)
            .collect::<Vec<_>>(),
        vec!["first"]
    );

    // Removal commits the removed state and returns the pending projection.
    let removed = reopened
        .remove_turn(
            RemoveTurnCommandDto::new(session, queued.turn_id()),
            time(5),
        )
        .expect("pending turn removes");
    assert_eq!(removed, queued);
    assert!(
        reopened
            .load_session_projection(session)
            .expect("session projection loads")
            .pending_turns()
            .is_empty()
    );
    assert_eq!(
        reopened
            .accept_user_turn(
                session,
                second_key,
                "second",
                second_run,
                snapshot(),
                time(6)
            )
            .expect_err("a removed key stays bound to its durable state")
            .code(),
        "turn_idempotency_conflict"
    );
    assert_eq!(
        reopened
            .remove_turn(
                RemoveTurnCommandDto::new(session, queued.turn_id()),
                time(6)
            )
            .expect_err("a removed turn cannot be removed twice")
            .code(),
        "pending_turn_not_found"
    );
}

#[test]
fn turn_idempotency_replay_reports_current_run_status_and_committed_message() {
    let (_directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    let key = IdempotencyKey::new();
    let (_, message) = started(
        store
            .accept_user_turn(session, key, "idempotent", run, snapshot(), time(2))
            .expect("turn commits"),
    );
    store
        .transition_run(session, run, RunStatusDto::Running, time(3))
        .expect("run starts");

    let (replayed_run, replayed_message) = started(
        store
            .accept_user_turn(session, key, "idempotent", run, snapshot(), time(4))
            .expect("turn replay is accepted"),
    );
    assert_eq!(replayed_run.run_id(), run);
    assert_eq!(replayed_run.status(), RunStatusDto::Running);
    assert_eq!(replayed_message, message);
    assert_eq!(
        store
            .load_run_messages(session, run, 10)
            .expect("transcript loads")
            .len(),
        1,
        "the idempotent replay commits no second transcript row"
    );
}

#[test]
fn pending_messages_never_start_runs_and_recovery_interrupts_unfinished_work() {
    let (_directory, store) = repository();
    let session = create(&store);
    let active_run = RunId::new();
    let _ = accept(&store, session, IdempotencyKey::new(), active_run, "active");
    let queued_runs = [RunId::new(), RunId::new()];
    let queued = [
        pending(accept(
            &store,
            session,
            IdempotencyKey::new(),
            queued_runs[0],
            "first pending",
        )),
        pending(accept(
            &store,
            session,
            IdempotencyKey::new(),
            queued_runs[1],
            "second pending",
        )),
    ];
    assert_eq!(
        store
            .load_session_projection(session)
            .expect("session projection loads")
            .pending_turns(),
        queued.as_slice(),
        "pending turns do not start runs while one is active"
    );

    let recovered = store
        .recover_unfinished_runs(time(5))
        .expect("recovery commits");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].run_id(), active_run);
    assert_eq!(recovered[0].status(), RunStatusDto::Interrupted);

    // After recovery the next accepted message finds an idle session with
    // pending work: the oldest pending message starts the run and the new
    // message stays pending.
    let (promoted, promoted_message) = started(accept(
        &store,
        session,
        IdempotencyKey::new(),
        RunId::new(),
        "later",
    ));
    assert_eq!(promoted.run_id(), queued_runs[0]);
    assert_eq!(promoted.turn_id(), queued[0].turn_id());
    assert_eq!(promoted_message.text(), "first pending");
    let projection = store
        .load_session_projection(session)
        .expect("session projection loads");
    let ordered = projection
        .pending_turns()
        .iter()
        .map(PendingTurnProjectionDto::turn_id)
        .collect::<Vec<_>>();
    assert_eq!(ordered.len(), 2);
    assert_eq!(ordered[0], queued[1].turn_id());
    assert_eq!(projection.pending_turns()[1].content(), "later");
}

#[test]
fn idle_admission_retains_the_oldest_pending_message_selection() {
    let (_directory, store) = repository();
    let session = create(&store);
    let active_run = RunId::new();
    let _ = accept(&store, session, IdempotencyKey::new(), active_run, "active");
    let pending_run = RunId::new();
    let revision_a = ConfigRevisionId::new();
    let config_a = snapshot_with_revision_and_model(revision_a, "fixture-a");
    let queued = pending(
        store
            .accept_user_turn(
                session,
                IdempotencyKey::new(),
                "pending",
                pending_run,
                config_a.clone(),
                time(3),
            )
            .expect("turn becomes pending"),
    );
    let config_b = snapshot_with_revision_and_model(ConfigRevisionId::new(), "fixture-b");
    store
        .accept_configuration_revision(config_b)
        .expect("new daemon-start configuration may be accepted");
    store
        .transition_run(session, active_run, RunStatusDto::Failed, time(4))
        .expect("terminal transition commits");

    let (run, message) = started(accept(
        &store,
        session,
        IdempotencyKey::new(),
        RunId::new(),
        "later",
    ));
    assert_eq!(run.run_id(), pending_run);
    assert_eq!(run.turn_id(), queued.turn_id());
    assert_eq!(run.config_revision_id(), revision_a);
    assert_eq!(message.text(), "pending");
    assert_eq!(message.run_id(), Some(pending_run));
    store
        .accept_configuration_revision(config_a)
        .expect("pending snapshot remains canonical and unmodified");
}

#[test]
fn pending_messages_keep_insertion_order_and_removal_allows_the_oldest_to_start() {
    let (_directory, store) = repository();
    let session = create(&store);
    let active_run = RunId::new();
    let _ = accept(&store, session, IdempotencyKey::new(), active_run, "active");
    let queued_runs = [RunId::new(), RunId::new(), RunId::new()];
    let queued = [
        pending(accept(
            &store,
            session,
            IdempotencyKey::new(),
            queued_runs[0],
            "first pending",
        )),
        pending(accept(
            &store,
            session,
            IdempotencyKey::new(),
            queued_runs[1],
            "second pending",
        )),
        pending(accept(
            &store,
            session,
            IdempotencyKey::new(),
            queued_runs[2],
            "third pending",
        )),
    ];
    let projection = store
        .load_session_projection(session)
        .expect("session projection loads");
    assert_eq!(
        projection
            .pending_turns()
            .iter()
            .map(PendingTurnProjectionDto::turn_id)
            .collect::<Vec<_>>(),
        queued
            .iter()
            .map(PendingTurnProjectionDto::turn_id)
            .collect::<Vec<_>>()
    );

    store
        .remove_turn(
            RemoveTurnCommandDto::new(session, queued[0].turn_id()),
            time(3),
        )
        .expect("oldest pending turn removes");
    store
        .transition_run(session, active_run, RunStatusDto::Failed, time(4))
        .expect("terminal transition commits");
    let (promoted, promoted_message) = started(accept(
        &store,
        session,
        IdempotencyKey::new(),
        RunId::new(),
        "later",
    ));
    assert_eq!(promoted.run_id(), queued_runs[1]);
    assert_eq!(promoted.turn_id(), queued[1].turn_id());
    assert_eq!(promoted_message.text(), "second pending");
    let projection = store
        .load_session_projection(session)
        .expect("session projection loads");
    assert_eq!(projection.pending_turns().len(), 2);
    assert_eq!(projection.pending_turns()[0].turn_id(), queued[2].turn_id());
    assert_eq!(projection.pending_turns()[1].content(), "later");
}

#[test]
fn consume_pending_user_turns_appends_ordered_messages_and_marks_turns_appended() {
    let (_directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    let _ = accept(&store, session, IdempotencyKey::new(), run, "active");
    let first_key = IdempotencyKey::new();
    let first_run = RunId::new();
    let first = pending(
        store
            .accept_user_turn(
                session,
                first_key,
                "first pending",
                first_run,
                snapshot(),
                time(3),
            )
            .expect("first turn queues"),
    );
    let second = pending(accept(
        &store,
        session,
        IdempotencyKey::new(),
        RunId::new(),
        "second pending",
    ));

    let consumed = store
        .consume_pending_user_turns(session, run, time(4))
        .expect("pending turns join the run");
    assert_eq!(consumed.len(), 2);
    assert_eq!(consumed[0].text(), "first pending");
    assert_eq!(consumed[0].run_id(), Some(run));
    assert_eq!(consumed[0].kind(), MessageKindDto::User);
    assert_eq!(consumed[1].text(), "second pending");
    assert_eq!(consumed[1].run_id(), Some(run));
    assert_eq!(
        store
            .load_run_messages(session, run, 10)
            .expect("run transcript loads")
            .iter()
            .map(MessageProjectionDto::text)
            .collect::<Vec<_>>(),
        vec!["active", "first pending", "second pending"],
        "joined messages keep insertion order after the starting turn"
    );
    assert!(
        store
            .load_session_projection(session)
            .expect("session projection loads")
            .pending_turns()
            .is_empty()
    );
    // A joined turn replays as its own pending projection, and a second
    // consume joins nothing.
    assert_eq!(
        pending(
            store
                .accept_user_turn(
                    session,
                    first_key,
                    "first pending",
                    first_run,
                    snapshot(),
                    time(5),
                )
                .expect("a joined turn replays as pending")
        )
        .turn_id(),
        first.turn_id()
    );
    assert!(
        store
            .consume_pending_user_turns(session, run, time(6))
            .expect("an empty consume commits")
            .is_empty()
    );
    assert_eq!(second.session_id(), session);
}

#[test]
fn append_message_and_transcript_reads_obey_bounds_and_order() {
    let (_directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    let _ = accept(&store, session, IdempotencyKey::new(), run, "start");
    let call_id = ToolCallId::new();
    let _ = append(
        &store,
        MessageProjectionDto::new(
            session,
            Some(run),
            MessageKindDto::Assistant,
            "answer",
            Some("why".to_owned()),
            None,
            None,
        )
        .expect("assistant row is valid"),
        3,
    );
    let _ = append(
        &store,
        MessageProjectionDto::new(
            session,
            Some(run),
            MessageKindDto::ToolCall,
            r#"{"path":"src/lib.rs"}"#,
            None,
            Some(call_id),
            Some("read".to_owned()),
        )
        .expect("tool call row is valid"),
        3,
    );

    let recent = store
        .load_recent_messages(session, 2)
        .expect("recent transcript loads");
    assert_eq!(
        recent
            .iter()
            .map(MessageProjectionDto::text)
            .collect::<Vec<_>>(),
        vec!["answer", r#"{"path":"src/lib.rs"}"#],
        "recent rows are the newest rows in insertion order"
    );
    let all = store
        .load_recent_messages(session, 10)
        .expect("full transcript loads");
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].text(), "start");
    assert_eq!(all[1].reasoning(), Some("why"));
    assert_eq!(
        store
            .load_run_messages(session, run, 10)
            .expect("run transcript loads")
            .len(),
        3
    );
    assert_eq!(
        store
            .load_recent_messages(session, 0)
            .expect_err("a zero limit is invalid")
            .code(),
        "invalid_message_limit"
    );
    assert_eq!(
        store
            .load_run_messages(session, run, 0)
            .expect_err("a zero limit is invalid")
            .code(),
        "invalid_message_limit"
    );
    assert_eq!(
        store
            .load_recent_messages(SessionId::new(), 1)
            .expect_err("an unknown session is hidden")
            .code(),
        "storage_record_not_found"
    );
    assert_eq!(
        store
            .load_run_messages(session, RunId::new(), 1)
            .expect_err("an unknown run is hidden")
            .code(),
        "storage_record_not_found"
    );
    let other = create(&store);
    assert_eq!(
        store
            .load_run_messages(other, run, 1)
            .expect_err("a cross-session run is hidden")
            .code(),
        "storage_record_not_found"
    );
    assert_eq!(
        store
            .append_message(
                MessageProjectionDto::new(
                    other,
                    Some(run),
                    MessageKindDto::User,
                    "foreign",
                    None,
                    None,
                    None,
                )
                .expect("foreign row is valid"),
                time(5),
            )
            .expect_err("a transcript row cannot address another session's run")
            .code(),
        "storage_record_not_found"
    );
}

#[test]
fn tool_result_evidence_commits_with_its_message_and_rereads_durably() {
    let (directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    let _ = accept(&store, session, IdempotencyKey::new(), run, "run");
    let call_id = ToolCallId::new();
    let evidence = ToolResultEvidenceDto::new(
        session,
        run,
        call_id,
        "read",
        ToolResultStatusDto::Completed,
        r#"{"result":"read","value":{"text":"hello","truncated":false}}"#,
        vec![
            ToolResultMetadataEntryDto::new("truncated", "false")
                .expect("fixture metadata is valid"),
        ],
        time(4),
    )
    .expect("tool result evidence is valid");
    let message = MessageProjectionDto::new(
        session,
        Some(run),
        MessageKindDto::ToolResult,
        evidence.content(),
        None,
        Some(call_id),
        Some("read".to_owned()),
    )
    .expect("answering row is valid");
    for mismatched in [
        MessageProjectionDto::new(
            session,
            Some(run),
            MessageKindDto::Assistant,
            "not a result",
            None,
            None,
            None,
        )
        .expect("assistant row is valid"),
        MessageProjectionDto::new(
            SessionId::new(),
            Some(run),
            MessageKindDto::ToolResult,
            evidence.content(),
            None,
            Some(call_id),
            Some("read".to_owned()),
        )
        .expect("cross-session row is structurally valid"),
        MessageProjectionDto::new(
            session,
            Some(RunId::new()),
            MessageKindDto::ToolResult,
            evidence.content(),
            None,
            Some(call_id),
            Some("read".to_owned()),
        )
        .expect("cross-run row is structurally valid"),
        MessageProjectionDto::new(
            session,
            Some(run),
            MessageKindDto::ToolResult,
            evidence.content(),
            None,
            Some(ToolCallId::new()),
            Some("read".to_owned()),
        )
        .expect("cross-call row is structurally valid"),
        MessageProjectionDto::new(
            session,
            Some(run),
            MessageKindDto::ToolResult,
            evidence.content(),
            None,
            Some(call_id),
            Some("glob".to_owned()),
        )
        .expect("cross-tool row is structurally valid"),
    ] {
        assert_eq!(
            store
                .write_tool_result(evidence.clone(), mismatched)
                .expect_err("a tool result commits only with its own answering row")
                .code(),
            "invalid_tool_result"
        );
    }
    let committed = store
        .write_tool_result(evidence.clone(), message.clone())
        .expect("evidence and its transcript row commit atomically");
    assert_eq!(committed, evidence);
    assert_eq!(
        store
            .load_tool_result(session, run, call_id)
            .expect("typed evidence rereads"),
        evidence
    );
    assert!(
        store
            .load_run_messages(session, run, 10)
            .expect("transcript loads")
            .iter()
            .any(|row| row.kind() == MessageKindDto::ToolResult
                && row.tool_call_id() == Some(call_id)
                && row.text() == evidence.content())
    );
    assert_eq!(
        store
            .write_tool_result(evidence.clone(), message)
            .expect_err("a second result for one call conflicts")
            .code(),
        "tool_result_conflict"
    );
    assert_eq!(
        store
            .load_tool_result(session, run, ToolCallId::new())
            .expect_err("an unknown call has no durable evidence")
            .code(),
        "tool_result_not_found"
    );
    assert_eq!(
        store
            .load_tool_result(SessionId::new(), run, call_id)
            .expect_err("a cross-session identity finds no evidence")
            .code(),
        "tool_result_not_found"
    );

    drop(store);
    let reopened = reopen(&directory);
    assert_eq!(
        reopened
            .load_tool_result(session, run, call_id)
            .expect("durable evidence rereads after reopen"),
        evidence
    );
}

#[test]
fn finish_run_commits_terminal_outcome_and_is_idempotent() {
    let (directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    let _ = accept(&store, session, IdempotencyKey::new(), run, "run");
    store
        .transition_run(session, run, RunStatusDto::Running, time(3))
        .expect("run starts");
    // The repository owns the terminal-safe outcome rule.
    for status in [RunStatusDto::Starting, RunStatusDto::Running] {
        assert_eq!(
            store
                .finish_run(session, run, status, None, None, None, None, time(3))
                .expect_err("a non-terminal outcome rejects")
                .code(),
            "invalid_run_outcome"
        );
    }
    assert_eq!(
        store
            .finish_run(
                session,
                run,
                RunStatusDto::Failed,
                None,
                None,
                Some("provider_failed".to_owned()),
                None,
                time(3),
            )
            .expect_err("an incomplete error pair rejects")
            .code(),
        "invalid_run_outcome"
    );
    assert_eq!(
        store
            .finish_run(
                session,
                run,
                RunStatusDto::Failed,
                None,
                None,
                Some("provider_failed".to_owned()),
                Some("unsafe\0message".to_owned()),
                time(3),
            )
            .expect_err("unsafe error text rejects")
            .code(),
        "invalid_run_outcome"
    );
    let finished = store
        .finish_run(
            session,
            run,
            RunStatusDto::Completed,
            Some(UsageDto::reported(2, 3, 5).expect("fixture usage is consistent")),
            Some(FinishReasonDto::Stop),
            None,
            None,
            time(4),
        )
        .expect("run completes");
    assert_eq!(finished.status(), RunStatusDto::Completed);
    assert_eq!(finished.run_id(), run);

    // The same terminal outcome replays idempotently and changes nothing.
    let replayed = store
        .finish_run(
            session,
            run,
            RunStatusDto::Completed,
            None,
            None,
            None,
            None,
            time(5),
        )
        .expect("repeated terminal outcome is idempotent");
    assert_eq!(replayed, finished);
    assert_eq!(
        store
            .finish_run(
                session,
                run,
                RunStatusDto::Failed,
                None,
                None,
                Some("provider_failed".to_owned()),
                Some("safe failure".to_owned()),
                time(6),
            )
            .expect_err("a completed run cannot become failed")
            .code(),
        "invalid_run_status_transition"
    );

    drop(store);
    let reopened = reopen(&directory);
    let failed_session = create(&reopened);
    let failed_run = RunId::new();
    let _ = accept(
        &reopened,
        failed_session,
        IdempotencyKey::new(),
        failed_run,
        "failing",
    );
    let failed = reopened
        .finish_run(
            failed_session,
            failed_run,
            RunStatusDto::Failed,
            Some(UsageDto::NotReported),
            None,
            Some("provider_failed".to_owned()),
            Some("safe failure".to_owned()),
            time(7),
        )
        .expect("failed run commits");
    assert_eq!(failed.status(), RunStatusDto::Failed);
}

#[test]
fn recovery_marks_every_unfinished_run_in_its_own_transaction() {
    let (_directory, store) = repository();
    let first_session = create(&store);
    let second_session = create(&store);
    let first_run = RunId::new();
    let _ = accept(
        &store,
        first_session,
        IdempotencyKey::new(),
        first_run,
        "first",
    );
    let queued = pending(accept(
        &store,
        first_session,
        IdempotencyKey::new(),
        RunId::new(),
        "pending",
    ));
    let completed_run = RunId::new();
    let _ = accept(
        &store,
        second_session,
        IdempotencyKey::new(),
        completed_run,
        "completed",
    );
    store
        .transition_run(
            second_session,
            completed_run,
            RunStatusDto::Running,
            time(3),
        )
        .expect("run starts");
    store
        .finish_run(
            second_session,
            completed_run,
            RunStatusDto::Completed,
            Some(UsageDto::NotReported),
            Some(FinishReasonDto::Stop),
            None,
            None,
            time(4),
        )
        .expect("run completes");
    let unfinished_run = RunId::new();
    let _ = accept(
        &store,
        second_session,
        IdempotencyKey::new(),
        unfinished_run,
        "unfinished",
    );

    let recovered = store
        .recover_unfinished_runs(time(5))
        .expect("recovery commits");
    let mut recovered_ids = recovered.iter().map(|run| run.run_id()).collect::<Vec<_>>();
    recovered_ids.sort_unstable();
    let mut expected = vec![first_run, unfinished_run];
    expected.sort_unstable();
    assert_eq!(recovered_ids, expected);
    assert!(
        recovered
            .iter()
            .all(|run| run.status() == RunStatusDto::Interrupted)
    );
    assert_eq!(
        store
            .load_run_projection(second_session, completed_run)
            .expect("completed run loads")
            .status(),
        RunStatusDto::Completed,
        "recovery leaves terminal runs untouched"
    );
    let projection = store
        .load_session_projection(first_session)
        .expect("session projection loads");
    assert!(projection.active_run().is_none());
    assert_eq!(projection.pending_turns(), std::slice::from_ref(&queued));
    assert!(
        store
            .recover_unfinished_runs(time(6))
            .expect("a second recovery commits")
            .is_empty(),
        "recovery is idempotent once every run is terminal"
    );
}

#[test]
fn canonical_config_revision_binds_one_snapshot_across_both_entry_points() {
    let (_directory, store) = repository();
    let session = create(&store);
    let revision = ConfigRevisionId::new();
    let original = snapshot_with_revision_and_model(revision, "fixture-a");
    store
        .accept_configuration_revision(original.clone())
        .expect("initial revision persists");
    store
        .accept_configuration_revision(original.clone())
        .expect("identical revision is idempotent");
    let direct = store
        .accept_configuration_revision(snapshot_with_revision_and_model(revision, "fixture-b"))
        .expect_err("revision cannot bind a different snapshot");
    assert_eq!(direct.code(), "config_revision_conflict");
    assert_eq!(direct.category(), ErrorCategoryDto::Conflict);
    assert_eq!(direct.retry(), ErrorRetryDto::Never);
    assert!(!direct.to_string().contains("fixture-secret"));
    assert!(!direct.to_string().contains("/tmp/"));

    // The same rule guards the accept_user_turn path that stores the run's
    // configuration revision.
    store
        .accept_user_turn(
            session,
            IdempotencyKey::new(),
            "first",
            RunId::new(),
            original,
            time(2),
        )
        .expect("the canonical revision serves the accepted turn");
    let error = store
        .accept_user_turn(
            session,
            IdempotencyKey::new(),
            "second",
            RunId::new(),
            snapshot_with_revision_and_model(revision, "fixture-b"),
            time(3),
        )
        .expect_err("turn acceptance cannot reuse revision for different snapshot");
    assert_eq!(error.code(), "config_revision_conflict");
    assert!(!error.to_string().contains("fixture-secret"));
    assert!(!error.to_string().contains("/tmp/"));
}

#[test]
fn workspace_identity_and_root_bindings_conflict_in_both_directions() {
    let (_directory, store) = repository();
    let workspace_id = WorkspaceId::new();
    let root = workspace_root("canonical");
    let first_session = SessionId::new();
    for session in [first_session, SessionId::new()] {
        store
            .create_session(
                CreateSessionCommandDto::new(
                    ProjectId::new(),
                    session,
                    workspace_id,
                    root.clone(),
                    RunModeDto::Build,
                ),
                time(1),
            )
            .expect("same workspace identity and root remain canonical");
    }
    assert_eq!(
        store
            .create_session(
                CreateSessionCommandDto::new(
                    ProjectId::new(),
                    first_session,
                    workspace_id,
                    root.clone(),
                    RunModeDto::Build,
                ),
                time(1),
            )
            .expect_err("a durable session identity is created once")
            .code(),
        "session_already_exists"
    );
    assert_eq!(
        store
            .create_session(
                CreateSessionCommandDto::new(
                    ProjectId::new(),
                    SessionId::new(),
                    workspace_id,
                    workspace_root("conflict"),
                    RunModeDto::Build,
                ),
                time(2),
            )
            .expect_err("workspace identity cannot bind a different root")
            .code(),
        "workspace_root_conflict"
    );
    let conflict = store
        .create_session(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                SessionId::new(),
                WorkspaceId::new(),
                root,
                RunModeDto::Build,
            ),
            time(3),
        )
        .expect_err("a second workspace identity cannot bind the same root");
    assert_eq!(conflict.code(), "workspace_root_conflict");
    assert_eq!(conflict.category(), ErrorCategoryDto::Conflict);
    assert_eq!(conflict.retry(), ErrorRetryDto::Never);
    assert_eq!(
        store
            .load_session_projection(SessionId::new())
            .expect_err("an unknown session projection is typed not-found")
            .code(),
        "storage_record_not_found"
    );
    assert_eq!(
        store
            .load_run_projection(SessionId::new(), RunId::new())
            .expect_err("an unknown run projection is typed not-found")
            .code(),
        "storage_record_not_found"
    );
}

#[test]
fn reused_run_identity_across_sessions_is_a_typed_conflict() {
    let (_directory, store) = repository();
    let first = create(&store);
    let run = RunId::new();
    let _ = accept(&store, first, IdempotencyKey::new(), run, "first");
    let second = create(&store);
    let started_conflict = store
        .accept_user_turn(
            second,
            IdempotencyKey::new(),
            "second",
            run,
            snapshot(),
            time(2),
        )
        .expect_err("the run identity is already durable in another session");
    assert_eq!(started_conflict.code(), "turn_identity_conflict");
    assert_eq!(started_conflict.category(), ErrorCategoryDto::Conflict);
    assert_eq!(started_conflict.retry(), ErrorRetryDto::Never);
    // With an active run in the second session the reused identity would take
    // the queued path; it is rejected before that insert as well.
    let _ = accept(
        &store,
        second,
        IdempotencyKey::new(),
        RunId::new(),
        "own turn",
    );
    let queued_conflict = store
        .accept_user_turn(
            second,
            IdempotencyKey::new(),
            "queued second",
            run,
            snapshot(),
            time(3),
        )
        .expect_err("the run identity stays rejected for a queued turn");
    assert_eq!(queued_conflict.code(), "turn_identity_conflict");
    // A pending turn's own proposed run identity is equally reserved.
    let reserved_run = RunId::new();
    let _ = pending(accept(
        &store,
        second,
        IdempotencyKey::new(),
        reserved_run,
        "pending",
    ));
    assert_eq!(
        store
            .accept_user_turn(
                second,
                IdempotencyKey::new(),
                "duplicate identity",
                reserved_run,
                snapshot(),
                time(4),
            )
            .expect_err("a reserved proposed run identity cannot be reused")
            .code(),
        "turn_identity_conflict"
    );
}

#[test]
fn undeclared_terminal_successors_are_rejected() {
    let (_directory, store) = repository();
    let session = create(&store);
    let run = RunId::new();
    let _ = accept(&store, session, IdempotencyKey::new(), run, "active");
    assert_eq!(
        store
            .transition_run(session, run, RunStatusDto::Completed, time(3))
            .expect_err("a starting run skips the running state")
            .code(),
        "invalid_run_status_transition"
    );
    store
        .transition_run(session, run, RunStatusDto::Running, time(3))
        .expect("run starts");
    store
        .finish_run(
            session,
            run,
            RunStatusDto::Completed,
            Some(UsageDto::NotReported),
            Some(FinishReasonDto::Stop),
            None,
            None,
            time(4),
        )
        .expect("run completes");
}
