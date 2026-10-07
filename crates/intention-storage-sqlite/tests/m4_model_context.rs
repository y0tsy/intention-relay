#![allow(
    clippy::expect_used,
    reason = "M4 SQLite model-context fixtures use expect for precise diagnostics."
)]

use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    CreateSessionCommandDto, MessageKindDto, MessageProjectionDto, RunModeDto, RunStatusDto,
    WorkspaceRootDto,
};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, AppendMessageInputDto, CreateSessionInputDto,
    FinishRunInputDto, StorageRepositoryDto, TransitionRunInputDto,
};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use intention_types::{
    ConfigRevisionId, FinishReasonDto, IdempotencyKey, ProjectId, RunId, SchemaVersionDto,
    SessionId, TimestampDto, ToolCallId, UsageDto, WorkspaceId,
};
use tempfile::TempDir;

#[test]
fn starting_run_model_context_rebuilds_the_committed_transcript_in_insertion_order() {
    let (_directory, repository) = repository();
    let session_id = create_session(&repository, "model-context");

    let first_run = start_run(&repository, session_id, "first user", "first-model", 2);
    let call_id = ToolCallId::new();
    append(
        &repository,
        session_id,
        Some(first_run),
        MessageKindDto::ToolCall,
        r#"{"path":"src/lib.rs"}"#,
        None,
        Some(call_id),
        Some("read"),
        3,
    );
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
        .transition_run(TransitionRunInputDto::new(
            session_id,
            first_run,
            RunStatusDto::Running,
            time(3),
        ))
        .expect("first run starts");
    repository
        .finish_run(
            FinishRunInputDto::new(
                session_id,
                first_run,
                RunStatusDto::Completed,
                Some(UsageDto::reported(1, 1, 2).expect("fixture usage is consistent")),
                Some(FinishReasonDto::Stop),
                None,
                None,
                time(4),
            )
            .expect("first run outcome is valid"),
        )
        .expect("first run completes");

    let starting_run = start_run(&repository, session_id, "current user", "current-model", 5);
    let context = repository
        .load_starting_run_model_context(session_id, starting_run)
        .expect("starting run context loads");

    assert_eq!(context.session_id(), session_id);
    assert_eq!(context.run_id(), starting_run);
    assert_eq!(
        context.safe_config().resolved().provider().model(),
        "current-model"
    );
    assert_eq!(
        context
            .messages()
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![
            (MessageKindDto::User, "first user"),
            (MessageKindDto::ToolCall, r#"{"path":"src/lib.rs"}"#),
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
    let run_id = start_run(&repository, session_id, "first user", "current-model", 2);
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
    let run_id = start_run(&repository, session_id, "owner user", "owner-model", 2);
    let other_session_id = create_session(&repository, "other");
    repository
        .transition_run(TransitionRunInputDto::new(
            session_id,
            run_id,
            RunStatusDto::Running,
            time(3),
        ))
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

fn repository() -> (TempDir, SqliteStorageRepository) {
    let directory = TempDir::new().expect("temporary directory exists");
    let repository = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(
            directory
                .path()
                .join("storage.sqlite")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("database path is absolute"),
    )
    .expect("database opens");
    (directory, repository)
}

fn create_session(repository: &SqliteStorageRepository, label: &str) -> SessionId {
    let session_id = SessionId::new();
    repository
        .create_session(CreateSessionInputDto::new(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                WorkspaceRootDto::parse(
                    std::env::temp_dir()
                        .join("intention-m4-model-context")
                        .join(label)
                        .to_string_lossy()
                        .into_owned(),
                )
                .expect("workspace root is absolute"),
                RunModeDto::Build,
            ),
            time(1),
        ))
        .expect("session creates");
    session_id
}

fn start_run(
    repository: &SqliteStorageRepository,
    session_id: SessionId,
    content: &str,
    model: &str,
    event_time: i64,
) -> RunId {
    let run_id = RunId::new();
    let outcome = repository
        .accept_user_turn(
            AcceptUserTurnInputDto::new(
                session_id,
                IdempotencyKey::new(),
                content,
                run_id,
                snapshot(model),
                time(event_time),
            )
            .expect("turn input is valid"),
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
        .append_message(AppendMessageInputDto::new(message, time(event_time)))
        .expect("fixture transcript row commits");
}

fn snapshot(model: &str) -> ConfigSnapshotDto {
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"{model}\"\ncredential = \"recognizable-fixture-credential\""
        ),
        ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("model-context.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("configuration path is absolute"),
        ),
    ))
    .expect("safe configuration resolves");
    ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        time(1),
        resolved,
    )
    .expect("safe snapshot is valid")
}

fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture time is valid")
}
