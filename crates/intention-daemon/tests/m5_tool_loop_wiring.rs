#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Focused daemon tool-loop fixtures use assertion conveniences for precise diagnostics."
)]

mod common;

use std::sync::Arc;

use common::{RecordingObserver, TokioTime, create_session, fixture_facade, schedule, started_run};
use intention_daemon::DaemonApplicationFacade;
use intention_daemon::DaemonToolExecutor;
use intention_engine::ModelRunExecutionOutcomeDto;
use intention_proto::ToolCallId;
use intention_proto::{MessageKindDto, MessageProjectionDto, RunId, RunStatusDto, SessionId};
use intention_providers::{
    FinishReasonDto, ModelEventDto, ModelMessageDto, ModelRoleDto, ModelToolDefinitionDto,
    ToolCallDto,
};
use intention_test_support::{FIXTURE_CREDENTIAL, ScriptedDriver};
use tempfile::TempDir;

/// The exact model-visible tool definitions the application advertises.
fn advertised_tool_definitions() -> Vec<ModelToolDefinitionDto> {
    intention_tools::model_visible_descriptors()
        .iter()
        .map(|descriptor| {
            ModelToolDefinitionDto::new(
                descriptor.id().as_str(),
                descriptor.description(),
                descriptor
                    .input_schema()
                    .expect("model-visible descriptors advertise input schemas"),
            )
            .expect("fixture tool definition is valid")
        })
        .collect()
}

/// Reads the committed transcript rows of one run through the daemon seam.
fn run_transcript(
    facade: &DaemonApplicationFacade,
    session_id: SessionId,
    run_id: RunId,
) -> Vec<MessageProjectionDto> {
    facade
        .load_run_messages_for_daemon(session_id, run_id, 64)
        .expect("run transcript reads")
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
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::tool_call(call.clone())),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]));
    let (_database_directory, facade, snapshot) = fixture_facade("tool-loop", driver.clone());
    let session_id = SessionId::new();
    create_session(&facade, session_id, workspace_directory.path());
    let run_id = started_run(&facade, session_id);

    let executor = DaemonToolExecutor::new(facade.clone());
    let observer = RecordingObserver::default();
    let outcome = facade
        .execute_scheduled_model_run_for_daemon_with_tool_executor(
            schedule(
                session_id,
                run_id,
                snapshot,
                Some(advertised_tool_definitions()),
            ),
            &TokioTime,
            &observer,
            &executor,
        )
        .await
        .expect("the real read tool loop completes");

    let ModelRunExecutionOutcomeDto::Completed { run } = outcome else {
        panic!("the read tool round completes the run: {outcome:?}")
    };
    assert_eq!(run.run_id(), run_id);
    assert_eq!(run.status(), RunStatusDto::Completed);
    assert_eq!(
        driver.executions(),
        2,
        "the follow-up round runs once; the same call is never re-executed"
    );
    let requests = driver.requests();
    assert_eq!(requests.len(), 2);
    let advertised_names: Vec<&str> = requests[0]
        .tools()
        .iter()
        .map(|definition| definition.name())
        .collect();
    let model_visible_names: Vec<&str> = intention_tools::model_visible_descriptors()
        .iter()
        .map(|descriptor| descriptor.id().as_str())
        .collect();
    assert_eq!(
        advertised_names, model_visible_names,
        "round one observes every model-visible tool definition in registry order"
    );
    // The continuation request closes its stable window prefix with the
    // runtime-recomputed prompt-cache breakpoint.
    let mut expected_result =
        ModelMessageDto::tool_result(call.call_id(), "hello from e2e").expect("message is valid");
    expected_result.set_cache_control(true);
    assert_eq!(
        requests[1].messages(),
        vec![
            ModelMessageDto::new(ModelRoleDto::User, "turn").expect("message is valid"),
            ModelMessageDto::assistant_tool_calls(None, vec![call.clone()])
                .expect("message is valid"),
            expected_result,
        ]
    );

    // The committed transcript is the current durable record of the tool round:
    // one call row and one result row for the invocation, exactly once.
    let messages = run_transcript(&facade, session_id, run_id);
    assert_eq!(
        messages
            .iter()
            .map(MessageProjectionDto::kind)
            .collect::<Vec<_>>(),
        vec![
            MessageKindDto::User,
            MessageKindDto::ToolCall,
            MessageKindDto::ToolResult,
        ],
        "the run transcript records the accepted turn, the call, and its single result"
    );
    assert_eq!(messages[0].text(), "turn");
    assert_eq!(messages[1].tool_id(), Some("read"));
    assert_eq!(messages[1].tool_call_id(), Some(call.call_id()));
    assert_eq!(
        messages[1].text(),
        r#"{"path":"hello.txt"}"#,
        "the durable tool call keeps the canonical arguments document"
    );
    assert_eq!(messages[2].tool_id(), Some("read"));
    assert_eq!(messages[2].tool_call_id(), Some(call.call_id()));
    assert_eq!(
        messages[2].text(),
        "hello from e2e",
        "the tool result row carries the exact file content"
    );
    assert_eq!(
        facade
            .load_run_projection_for_daemon(session_id, run_id)
            .expect("completed run projection reads")
            .status(),
        RunStatusDto::Completed
    );
    assert_eq!(
        observer.observed_statuses(),
        vec![RunStatusDto::Running, RunStatusDto::Completed],
        "the runtime publishes each committed status exactly once, in order"
    );
    let transcript_json = serde_json::to_string(&messages).expect("transcript serializes");
    assert!(
        !transcript_json.contains(&workspace_directory.path().to_string_lossy().into_owned()),
        "the durable transcript never discloses the workspace absolute path"
    );
    assert!(
        !transcript_json.contains(FIXTURE_CREDENTIAL),
        "the durable transcript never discloses the provider credential"
    );
}

#[tokio::test]
async fn daemon_tool_executor_missing_file_returns_typed_failure() {
    let workspace_directory = TempDir::new().expect("temporary workspace exists");
    let call = ToolCallDto::new(ToolCallId::new(), "read", r#"{"path":"missing.txt"}"#)
        .expect("fixture call is valid");
    let driver = Arc::new(ScriptedDriver::with_rounds(vec![vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::tool_call(call.clone())),
    ]]));
    let (_database_directory, facade, snapshot) = fixture_facade("tool-loop", driver.clone());
    let session_id = SessionId::new();
    create_session(&facade, session_id, workspace_directory.path());
    let run_id = started_run(&facade, session_id);

    let executor = DaemonToolExecutor::new(facade.clone());
    let observer = RecordingObserver::default();
    let outcome = facade
        .execute_scheduled_model_run_for_daemon_with_tool_executor(
            schedule(
                session_id,
                run_id,
                snapshot,
                Some(advertised_tool_definitions()),
            ),
            &TokioTime,
            &observer,
            &executor,
        )
        .await
        .expect("the missing-file tool loop commits a typed failure");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        panic!("the missing-file round fails the run: {outcome:?}")
    };
    assert_eq!(run.run_id(), run_id);
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "tool_read_failed");
    assert_eq!(driver.executions(), 1, "no retry follows the typed failure");

    let messages = run_transcript(&facade, session_id, run_id);
    assert_eq!(
        messages
            .iter()
            .map(MessageProjectionDto::kind)
            .collect::<Vec<_>>(),
        vec![
            MessageKindDto::User,
            MessageKindDto::ToolCall,
            MessageKindDto::ToolResult,
        ],
        "the failed run records the turn, the call, and its typed failure result"
    );
    assert_eq!(messages[1].tool_call_id(), Some(call.call_id()));
    assert_eq!(
        messages[1].text(),
        r#"{"path":"missing.txt"}"#,
        "the denied tool call keeps the canonical arguments document"
    );
    assert_eq!(messages[2].tool_id(), Some("read"));
    assert_eq!(messages[2].tool_call_id(), Some(call.call_id()));
    assert_eq!(
        messages[2].text(),
        "tool_read_failed",
        "the durable result row answers the call with the typed failure"
    );
    assert_eq!(
        facade
            .load_run_projection_for_daemon(session_id, run_id)
            .expect("failed run projection reads")
            .status(),
        RunStatusDto::Failed
    );
    assert_eq!(
        observer.observed_statuses(),
        vec![RunStatusDto::Running, RunStatusDto::Failed],
        "the runtime publishes the failed terminal status exactly once"
    );
}
