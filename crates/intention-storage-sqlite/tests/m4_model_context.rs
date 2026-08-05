#![allow(
    clippy::expect_used,
    reason = "M4 SQLite model-context fixtures use expect for precise diagnostics."
)]

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Instant;

use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    CreateSessionCommandDto, ModelRunFactInputDto, RunEventCursorDto, RunModeDto, RunStatusDto,
    WorkspaceRootDto,
};
use intention_storage::{
    AcceptUserTurnInputDto, AppendModelRunFactsInputDto, CreateSessionInputDto,
    ModelContextRoleDto, StorageRepositoryDto, TransitionRunInputDto,
};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use intention_types::{
    AssistantTurnId, ConfigRevisionId, ProjectId, RunId, SessionId, TimestampDto, TurnId,
    WorkspaceId,
};
use tempfile::TempDir;

#[test]
fn starting_run_model_context_uses_durable_start_order_and_completed_assistant_text() {
    let (_directory, repository) = repository();
    let session_id = create_session(&repository, "model-context");

    let first_run = start_run(&repository, session_id, "first user", "first-model", 2);
    start_and_finish(
        &repository,
        session_id,
        first_run,
        Some("first assistant"),
        3,
    );

    let second_run = start_run(&repository, session_id, "second user", "second-model", 4);
    start_and_finish(&repository, session_id, second_run, None, 5);

    let partial_run = start_run(&repository, session_id, "partial user", "partial-model", 6);
    append_assistant_content(&repository, session_id, partial_run, "partial assistant", 7);
    repository
        .transition_run(TransitionRunInputDto::new(
            session_id,
            partial_run,
            RunStatusDto::Failed,
            time(7),
        ))
        .expect("partial run fails");

    let starting_run = start_run(&repository, session_id, "current user", "current-model", 8);
    let context = repository
        .load_starting_run_model_context(session_id, starting_run)
        .expect("starting run context loads");

    assert_eq!(context.run_id(), starting_run);
    assert_eq!(
        context.safe_config().resolved().provider().model(),
        "current-model"
    );
    assert_eq!(
        context
            .messages()
            .iter()
            .map(|message| (message.role(), message.content()))
            .collect::<Vec<_>>(),
        vec![
            (ModelContextRoleDto::User, "first user"),
            (ModelContextRoleDto::Assistant, "first assistant"),
            (ModelContextRoleDto::User, "second user"),
            (ModelContextRoleDto::User, "partial user"),
            (ModelContextRoleDto::User, "current user"),
        ]
    );
    assert_eq!(
        context
            .messages()
            .last()
            .expect("current user is present")
            .role(),
        ModelContextRoleDto::User
    );
    assert_eq!(
        context
            .messages()
            .last()
            .expect("current user is present")
            .content(),
        "current user"
    );
    let encoded = serde_json::to_string(context.safe_config()).expect("safe config serializes");
    assert!(!encoded.contains("recognizable-fixture-credential"));
    assert!(!encoded.contains("model-context.toml"));
}

#[test]
fn context_read_never_returns_starting_context_after_concurrent_terminalization() {
    for iteration in 0..32 {
        let directory = TempDir::new().expect("temporary directory exists");
        let location = SqliteDatabaseLocationDto::new(
            directory
                .path()
                .join("storage.sqlite")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("database path is absolute");
        let reader_repository =
            SqliteStorageRepository::open(location.clone()).expect("reader opens");
        let writer_repository = SqliteStorageRepository::open(location).expect("writer opens");
        let session_id = create_session(&reader_repository, "concurrent-context");
        let run_id = start_run(
            &reader_repository,
            session_id,
            "current user",
            "current-model",
            2,
        );

        let reader_repository = Arc::new(reader_repository);
        let writer_repository = Arc::new(writer_repository);
        let ready = Arc::new(Barrier::new(2));
        let reader_ready = Arc::clone(&ready);
        let reader = {
            let repository = Arc::clone(&reader_repository);
            thread::spawn(move || {
                reader_ready.wait();
                let context = repository.load_starting_run_model_context(session_id, run_id);
                (Instant::now(), context)
            })
        };
        let writer_ready = Arc::clone(&ready);
        let writer = {
            let repository = Arc::clone(&writer_repository);
            thread::spawn(move || {
                writer_ready.wait();
                repository
                    .transition_run(TransitionRunInputDto::new(
                        session_id,
                        run_id,
                        RunStatusDto::Interrupted,
                        time(3),
                    ))
                    .expect("writer terminalizes run");
                Instant::now()
            })
        };

        let (reader_finished, context) = reader.join().expect("reader thread completes");
        let writer_finished = writer.join().expect("writer thread completes");
        if writer_finished < reader_finished {
            let error = context.expect_err(
                "a read completing after terminalization cannot return a starting context",
            );
            assert_eq!(
                error.code(),
                "run_model_context_unavailable",
                "iteration {iteration}"
            );
        }
    }
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
    repository
        .accept_user_turn(
            AcceptUserTurnInputDto::new(
                session_id,
                TurnId::new(),
                content,
                run_id,
                snapshot(model),
                time(event_time),
            )
            .expect("turn input is valid"),
        )
        .expect("turn starts");
    run_id
}

fn append_assistant_content(
    repository: &SqliteStorageRepository,
    session_id: SessionId,
    run_id: RunId,
    assistant_content: &str,
    event_time: i64,
) {
    repository
        .append_model_run_facts(
            AppendModelRunFactsInputDto::new(
                session_id,
                run_id,
                RunEventCursorDto::new(0),
                vec![
                    ModelRunFactInputDto::provider_attempt_started(1).expect("attempt is valid"),
                    ModelRunFactInputDto::assistant_content_appended(
                        AssistantTurnId::new(),
                        assistant_content,
                    )
                    .expect("assistant content is valid"),
                ],
                Some(RunStatusDto::Running),
                time(event_time),
            )
            .expect("fact input is valid"),
        )
        .expect("partial assistant content persists");
}

fn start_and_finish(
    repository: &SqliteStorageRepository,
    session_id: SessionId,
    run_id: RunId,
    assistant_content: Option<&str>,
    event_time: i64,
) {
    let mut facts =
        vec![ModelRunFactInputDto::provider_attempt_started(1).expect("attempt is valid")];
    if let Some(assistant_content) = assistant_content {
        facts.push(
            ModelRunFactInputDto::assistant_content_appended(
                AssistantTurnId::new(),
                assistant_content,
            )
            .expect("assistant content is valid"),
        );
    }
    repository
        .append_model_run_facts(
            AppendModelRunFactsInputDto::new(
                session_id,
                run_id,
                RunEventCursorDto::new(0),
                facts,
                Some(RunStatusDto::Running),
                time(event_time),
            )
            .expect("fact input is valid"),
        )
        .expect("assistant content persists");
    repository
        .transition_run(TransitionRunInputDto::new(
            session_id,
            run_id,
            RunStatusDto::Completing,
            time(event_time),
        ))
        .expect("run begins completion");
    repository
        .transition_run(TransitionRunInputDto::new(
            session_id,
            run_id,
            RunStatusDto::Completed,
            time(event_time),
        ))
        .expect("run completes");
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
        intention_types::SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        time(1),
        resolved,
    )
    .expect("safe snapshot is valid")
}

fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture time is valid")
}
