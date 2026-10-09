#![allow(
    clippy::expect_used,
    reason = "M4 SQLite model-context fixtures use expect for precise diagnostics."
)]

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{create_session, repository, selection, time};

use intention_config::{ConfigSnapshotDto, ContextWindowPolicyDto};
use intention_proto::{
    ConfigRevisionId, FinishReasonDto, IdempotencyKey, RunId, SchemaVersionDto, SessionId,
    ToolCallId, UsageDto,
};
use intention_proto::{MessageKindDto, MessageProjectionDto, RunStatusDto};
use intention_storage::{
    AcceptedTurnOutcomeDto, RunOutcomeDto, SqliteStorageRepository, StorageRepositoryDto,
};

#[test]
fn starting_run_model_context_rebuilds_the_committed_transcript_in_insertion_order() {
    let (_directory, repository) = repository();
    let session_id = create_session(&repository, "model-context");

    let first_run = start_run(&repository, session_id, "first user", 2);
    let call_id = ToolCallId::new();
    append(
        &repository,
        session_id,
        Some(first_run),
        MessageKindDto::ToolResult,
        "file contents",
        None,
        Some(call_id),
        Some("read"),
        3,
    );
    append(
        &repository,
        session_id,
        Some(first_run),
        MessageKindDto::Notice,
        "[The call was stopped before a final result.]",
        None,
        None,
        None,
        3,
    );
    append(
        &repository,
        session_id,
        Some(first_run),
        MessageKindDto::Assistant,
        "first assistant",
        Some("first reasoning"),
        None,
        None,
        3,
    );
    repository
        .transition_run(session_id, first_run, RunStatusDto::Running, time(3))
        .expect("first run starts");
    repository
        .finish_run(
            session_id,
            first_run,
            RunOutcomeDto::new(
                RunStatusDto::Completed,
                Some(UsageDto::reported(1, 1, 2).expect("fixture usage is consistent")),
                Some(FinishReasonDto::Stop),
                None,
                None,
            )
            .expect("fixture outcome is valid"),
            time(4),
        )
        .expect("first run completes");

    let starting_run = start_run(&repository, session_id, "current user", 5);
    let context = repository
        .load_starting_run_model_context(session_id, starting_run)
        .expect("starting run context loads");

    assert_eq!(context.session_id(), session_id);
    assert_eq!(context.run_id(), starting_run);
    assert_eq!(
        context.safe_config().context_window().window_tokens(),
        250_000,
        "the starting context carries the run's committed window policy"
    );
    assert_eq!(
        context
            .messages()
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![
            (MessageKindDto::User, "first user"),
            (MessageKindDto::ToolResult, "file contents"),
            (
                MessageKindDto::Notice,
                "[The call was stopped before a final result.]"
            ),
            (MessageKindDto::Assistant, "first assistant"),
            (MessageKindDto::User, "current user"),
        ]
    );
    let assistant = context
        .messages()
        .iter()
        .find(|message| message.kind() == MessageKindDto::Assistant)
        .expect("the completed assistant step is part of the context");
    assert_eq!(assistant.reasoning(), Some("first reasoning"));
    assert_eq!(assistant.run_id(), Some(first_run));
    let last = context.messages().last().expect("current user is present");
    assert_eq!(last.kind(), MessageKindDto::User);
    assert_eq!(last.run_id(), Some(starting_run));
    let encoded = serde_json::to_string(context.safe_config()).expect("safe config serializes");
    assert!(!encoded.contains("recognizable-fixture-credential"));
    assert!(!encoded.contains("model-context.toml"));
}

#[test]
fn starting_run_context_ends_at_the_target_run_user_turn() {
    let (_directory, repository) = repository();
    let session_id = create_session(&repository, "boundary");
    let run_id = start_run(&repository, session_id, "first user", 2);
    // A message committed after the run's starting turn (for example a pending
    // message consumed at a boundary) never extends the starting context.
    append(
        &repository,
        session_id,
        Some(run_id),
        MessageKindDto::User,
        "joined user",
        None,
        None,
        None,
        3,
    );

    let context = repository
        .load_starting_run_model_context(session_id, run_id)
        .expect("starting run context loads");
    assert_eq!(
        context
            .messages()
            .iter()
            .map(MessageProjectionDto::text)
            .collect::<Vec<_>>(),
        vec!["first user"]
    );
}

#[test]
fn model_context_rejects_unknown_cross_session_and_non_starting_runs_safely() {
    let (_directory, repository) = repository();
    let session_id = create_session(&repository, "owner");
    let run_id = start_run(&repository, session_id, "owner user", 2);
    let other_session_id = create_session(&repository, "other");
    repository
        .transition_run(session_id, run_id, RunStatusDto::Running, time(3))
        .expect("run becomes non-starting");

    let errors = [
        repository
            .load_starting_run_model_context(SessionId::new(), run_id)
            .expect_err("unknown session is hidden"),
        repository
            .load_starting_run_model_context(other_session_id, run_id)
            .expect_err("cross-session run is hidden"),
        repository
            .load_starting_run_model_context(session_id, RunId::new())
            .expect_err("unknown run is hidden"),
        repository
            .load_starting_run_model_context(session_id, run_id)
            .expect_err("non-starting run is unavailable"),
    ];
    for error in errors {
        assert_eq!(error.code(), "run_model_context_unavailable");
        let rendered = error.to_string();
        assert!(!rendered.contains("recognizable-fixture-credential"));
        assert!(!rendered.contains("model-context.toml"));
        assert!(!rendered.contains("sqlite"));
    }
}

fn start_run(
    repository: &SqliteStorageRepository,
    session_id: SessionId,
    content: &str,
    event_time: i64,
) -> RunId {
    let run_id = RunId::new();
    let outcome = repository
        .accept_user_turn(
            session_id,
            IdempotencyKey::new(),
            content,
            run_id,
            snapshot(),
            selection("fixture"),
            None,
            time(event_time),
        )
        .expect("turn starts");
    match outcome {
        AcceptedTurnOutcomeDto::Started { run, message } => {
            assert_eq!(run.run_id(), run_id);
            assert_eq!(message.text(), content);
            run_id
        }
        AcceptedTurnOutcomeDto::Pending(_) => unreachable!("a fresh session starts the run"),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "The transcript fixture keeps one flat row builder."
)]
fn append(
    repository: &SqliteStorageRepository,
    session_id: SessionId,
    run_id: Option<RunId>,
    kind: MessageKindDto,
    text: &str,
    reasoning: Option<&str>,
    tool_call_id: Option<ToolCallId>,
    tool_id: Option<&str>,
    event_time: i64,
) {
    let message = MessageProjectionDto::new(
        session_id,
        run_id,
        kind,
        text,
        reasoning.map(str::to_owned),
        tool_call_id,
        tool_id.map(str::to_owned),
    )
    .expect("fixture transcript row is valid");
    repository
        .append_message(message, time(event_time))
        .expect("fixture transcript row commits");
}

#[test]
fn starting_run_context_reads_without_taking_a_write_lock() {
    let (directory, repository) = repository();
    let session_id = create_session(&repository, "read-lock");
    let run_id = start_run(&repository, session_id, "user", 2);

    // Another connection holds the database's write lock. The pure read takes
    // no transaction of its own, so it still succeeds; an immediate transaction
    // would have taken a RESERVED lock and surfaced as `storage_busy`.
    let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
        .expect("database reopens for the lock fixture");
    connection
        .execute_batch("BEGIN IMMEDIATE")
        .expect("the write lock is taken");
    let context = repository
        .load_starting_run_model_context(session_id, run_id)
        .expect("the context read takes no write lock");
    assert_eq!(context.run_id(), run_id);
    connection
        .execute_batch("ROLLBACK")
        .expect("the write lock releases");
}

fn snapshot() -> ConfigSnapshotDto {
    ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        time(1),
        ContextWindowPolicyDto::default_policy(),
    )
    .expect("safe snapshot is valid")
}
