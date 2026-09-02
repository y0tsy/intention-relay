#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Focused daemon tool-loop fixtures use assertion conveniences for precise diagnostics."
)]

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::stream;
use intention::DaemonApplicationFacade;
use intention_application::ScheduleModelRunDto;
use intention_config::ConfigSnapshotDto;
use intention_daemon::DaemonToolExecutor;
use intention_domain::{
    ModelRunFactInputDto, RunEventCursorDto, RunStatusDto, SendUserTurnCommandDto,
    ToolResultOutcomeDto, WorkspaceRootDto,
};
use intention_model::{
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelDriver, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ModelRoleDto,
    ToolCallDto,
};
use intention_protocol::{
    ProtocolAcceptedResultDto, ProtocolCommandDto, ProtocolCommandResultDto, SendUserTurnOutcomeDto,
};
use intention_runtime::{
    ModelRunCommitDto, ModelRunCommitObserver, ModelRunExecutionOutcomeDto, ModelSleepFuture,
    ModelTimePort,
};
use intention_types::{RunId, SessionId, TimestampDto, ToolCallId, TurnId};
use tempfile::TempDir;

/// Emits one scripted event round per provider execution and records requests.
struct ScriptedDriver {
    rounds: Mutex<VecDeque<Vec<ModelEventDto>>>,
    executions: Mutex<usize>,
    requests: Mutex<Vec<ModelRequestDto>>,
}

impl ScriptedDriver {
    fn with_rounds(rounds: Vec<Vec<ModelEventDto>>) -> Self {
        Self {
            rounds: Mutex::new(rounds.into()),
            executions: Mutex::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn executions(&self) -> usize {
        *self
            .executions
            .lock()
            .expect("driver recorder remains available")
    }

    fn requests(&self) -> Vec<ModelRequestDto> {
        self.requests
            .lock()
            .expect("driver request recorder remains available")
            .clone()
    }
}

impl ModelDriver for ScriptedDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true, true, true, false, false, true)
    }
}

impl ModelExecutionDriver for ScriptedDriver {
    fn execute(
        &self,
        request: ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        *self
            .executions
            .lock()
            .expect("driver recorder remains available") += 1;
        self.requests
            .lock()
            .expect("driver request recorder remains available")
            .push(request);
        let events = self
            .rounds
            .lock()
            .expect("scripted rounds remain available")
            .pop_front()
            .expect("scripted round exists");
        Box::pin(stream::iter(events.into_iter().map(Ok)))
    }
}

struct TokioTime;

impl ModelTimePort for TokioTime {
    fn now(&self) -> TimestampDto {
        TimestampDto::from_unix_seconds(2).expect("fixture timestamp is valid")
    }

    fn sleep(&self, duration: Duration) -> ModelSleepFuture<'_> {
        Box::pin(tokio::time::sleep(duration))
    }
}

#[derive(Default)]
struct RecordingObserver {
    commits: Mutex<Vec<ModelRunCommitDto>>,
}

impl ModelRunCommitObserver for RecordingObserver {
    fn observe_model_run_commit(&self, committed: ModelRunCommitDto) {
        self.commits
            .lock()
            .expect("observer recorder remains available")
            .push(committed);
    }
}

fn fixture_facade(
    driver: Arc<dyn ModelExecutionDriver + Send + Sync>,
) -> (TempDir, DaemonApplicationFacade, ConfigSnapshotDto) {
    let directory = TempDir::new().expect("temporary directory exists");
    let snapshot = intention_test_snapshot();
    let facade = DaemonApplicationFacade::open_for_test_support_with_driver(
        directory.path().join("tool-loop.sqlite"),
        snapshot.clone(),
        driver,
    )
    .expect("fixture facade opens");
    facade
        .seed_fixture_catalog_for_test_support(
            "seed-1",
            "openrouter",
            "fixture",
            "https://api.example.invalid/v1",
        )
        .expect("fixture catalog seeds");
    (directory, facade, snapshot)
}

fn intention_test_snapshot() -> ConfigSnapshotDto {
    let source = intention_config::ConfigSourceDto::Explicit(
        intention_config::ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-daemon-tool-loop.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture source is absolute"),
    );
    let resolved = intention_config::ResolvedConfigDto::parse_resolve(
        intention_config::RawConfigInputDto::new(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential\"",
            source,
        ),
    )
    .expect("fixture configuration resolves");
    ConfigSnapshotDto::new(
        intention_types::SchemaVersionDto::new(1, 0),
        intention_types::ConfigRevisionId::new(),
        TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid"),
        resolved,
    )
    .expect("fixture snapshot is valid")
}

fn schedule(
    session_id: SessionId,
    run_id: RunId,
    snapshot: ConfigSnapshotDto,
) -> ScheduleModelRunDto {
    ScheduleModelRunDto::new(
        session_id,
        run_id,
        ModelRequestDto::new(
            run_id,
            "fixture",
            vec![ModelMessageDto::new(ModelRoleDto::User, "turn").expect("message is valid")],
            None,
            None,
        )
        .expect("request is valid"),
        snapshot,
    )
    .expect("schedule is valid")
}

fn create_session(
    facade: &DaemonApplicationFacade,
    session_id: SessionId,
    workspace: &std::path::Path,
) {
    let create = ProtocolCommandDto::CreateSession(intention_domain::CreateSessionCommandDto::new(
        intention_types::ProjectId::new(),
        session_id,
        intention_types::WorkspaceId::new(),
        WorkspaceRootDto::parse(workspace.to_string_lossy().into_owned())
            .expect("fixture workspace is absolute"),
        intention_domain::RunModeDto::Build,
    ));
    assert!(matches!(
        facade.command(create),
        ProtocolCommandResultDto::Accepted(_)
    ));
}

fn started_run(facade: &DaemonApplicationFacade, session_id: SessionId) -> RunId {
    let result = facade.command(ProtocolCommandDto::SendUserTurn(
        SendUserTurnCommandDto::new(session_id, TurnId::new(), "turn").expect("turn is valid"),
    ));
    let ProtocolCommandResultDto::Accepted(accepted) = result else {
        panic!("fixture turn starts")
    };
    let Some(ProtocolAcceptedResultDto::SendUserTurn(turn)) = accepted.result() else {
        panic!("fixture result is a turn")
    };
    let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
        panic!("first turn starts")
    };
    run_id
}

fn run_tail(
    facade: &DaemonApplicationFacade,
    session_id: SessionId,
    run_id: RunId,
) -> Vec<intention_domain::ModelRunFactDto> {
    facade
        .load_run_tail_for_daemon(session_id, run_id, RunEventCursorDto::new(0))
        .expect("run tail reads")
        .facts()
        .to_vec()
}

#[tokio::test]
async fn daemon_tool_executor_executes_real_read_tool_through_loop() {
    let workspace_directory = TempDir::new().expect("temporary workspace exists");
    std::fs::write(
        workspace_directory.path().join("hello.txt"),
        "hello from e2e",
    )
    .expect("workspace fixture writes");
    let call = ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"hello.txt"}"#)
        .expect("fixture call is valid");
    let driver = Arc::new(ScriptedDriver::with_rounds(vec![
        vec![
            ModelEventDto::started(),
            ModelEventDto::tool_call(call.clone()),
        ],
        vec![
            ModelEventDto::started(),
            ModelEventDto::finished(FinishReasonDto::Stop),
        ],
    ]));
    let (_database_directory, facade, snapshot) = fixture_facade(driver.clone());
    let session_id = SessionId::new();
    create_session(&facade, session_id, workspace_directory.path());
    let run_id = started_run(&facade, session_id);

    let executor = DaemonToolExecutor::new(facade.clone());
    let observer = RecordingObserver::default();
    let outcome = facade
        .execute_scheduled_model_run_for_daemon_with_tool_executor(
            schedule(session_id, run_id, snapshot),
            ModelCancellationSignal::new(),
            &TokioTime,
            &observer,
            &executor,
        )
        .await
        .expect("the real read tool loop completes");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            cursor: RunEventCursorDto::new(4)
        }
    );
    assert_eq!(
        driver.executions(),
        2,
        "the follow-up round runs once; the same call is never re-executed"
    );
    let requests = driver.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].messages(),
        vec![
            ModelMessageDto::new(ModelRoleDto::User, "turn").expect("message is valid"),
            ModelMessageDto::assistant_tool_calls(None, vec![call.clone()])
                .expect("message is valid"),
            ModelMessageDto::tool_result(call.call_id(), "hello from e2e")
                .expect("message is valid"),
        ]
    );

    let facts = run_tail(&facade, session_id, run_id);
    assert_eq!(facts.len(), 4);
    assert!(matches!(
        facts[0].input(),
        ModelRunFactInputDto::ProviderAttemptStarted { attempt: 1 }
    ));
    assert!(matches!(
        facts[1].input(),
        ModelRunFactInputDto::ToolCallRecorded { call: recorded } if *recorded == call
    ));
    assert!(matches!(
        facts[2].input(),
        ModelRunFactInputDto::ToolResultRecorded {
            call_id,
            outcome: ToolResultOutcomeDto::Succeeded { content },
        } if *call_id == call.call_id() && content == "hello from e2e"
    ));
    assert!(matches!(
        facts[3].input(),
        ModelRunFactInputDto::Finished { .. }
    ));

    let replay = facade
        .load_current_run_replay_for_daemon(session_id, run_id)
        .expect("completed run replay reads");
    assert_eq!(
        replay.snapshot().run_projection().status(),
        RunStatusDto::Completed
    );
    let facts_json = serde_json::to_string(&facts).expect("facts serialize");
    assert!(
        !facts_json.contains(&workspace_directory.path().to_string_lossy().into_owned()),
        "durable facts never disclose the workspace absolute path"
    );
    assert!(
        !facts_json.contains("fixture-credential"),
        "durable facts never disclose the provider credential"
    );
}

#[tokio::test]
async fn daemon_tool_executor_missing_file_returns_typed_failure() {
    let workspace_directory = TempDir::new().expect("temporary workspace exists");
    let call = ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"missing.txt"}"#)
        .expect("fixture call is valid");
    let driver = Arc::new(ScriptedDriver::with_rounds(vec![vec![
        ModelEventDto::started(),
        ModelEventDto::tool_call(call.clone()),
    ]]));
    let (_database_directory, facade, snapshot) = fixture_facade(driver.clone());
    let session_id = SessionId::new();
    create_session(&facade, session_id, workspace_directory.path());
    let run_id = started_run(&facade, session_id);

    let executor = DaemonToolExecutor::new(facade.clone());
    let observer = RecordingObserver::default();
    let outcome = facade
        .execute_scheduled_model_run_for_daemon_with_tool_executor(
            schedule(session_id, run_id, snapshot),
            ModelCancellationSignal::new(),
            &TokioTime,
            &observer,
            &executor,
        )
        .await
        .expect("the missing-file tool loop commits a typed failure");

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Failed {
            cursor: RunEventCursorDto::new(4)
        }
    );
    assert_eq!(driver.executions(), 1, "no retry follows the typed failure");
    let facts = run_tail(&facade, session_id, run_id);
    assert_eq!(facts.len(), 4);
    assert!(matches!(
        facts[1].input(),
        ModelRunFactInputDto::ToolCallRecorded { .. }
    ));
    assert!(matches!(
        facts[2].input(),
        ModelRunFactInputDto::ToolResultRecorded {
            call_id,
            outcome: ToolResultOutcomeDto::Failed { failure },
        } if *call_id == call.call_id() && failure.code() == "workspace_path_unavailable"
    ));
    assert!(matches!(
        facts[3].input(),
        ModelRunFactInputDto::Failed { failure }
            if failure.code() == "workspace_path_unavailable"
    ));
    let replay = facade
        .load_current_run_replay_for_daemon(session_id, run_id)
        .expect("failed run replay reads");
    assert_eq!(
        replay.snapshot().run_projection().status(),
        RunStatusDto::Failed
    );
}
