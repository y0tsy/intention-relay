mod common;

use std::fs;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{FakeRepository, RecordingCommitObserver, row_id, workspace_root};
use intention_config::ConfigSnapshotDto;
use intention_engine::{
    ApplicationService, ModelMessageDto, ModelRoleDto, RunCancellation, SessionCommitDto,
    SessionCommitObserver, ToolInvocationRequestDto, ToolResultOutcomeDto,
};
use intention_proto::{
    BoundedText, TextResult, ToolCallRowDto, ToolIdentityDto, ToolInput, ToolResult,
    ToolResultRowDto, ToolResultStatusDto, ToolRowDto,
};
use intention_proto::{
    CreateSessionCommandDto, CreateSessionFirstTurnDto, MessageKindDto, MessageProjectionDto,
    PendingTurnProjectionDto, RunModeDto, RunProjectionDto, RunStatusDto, SendUserTurnCommandDto,
    SessionProjectionDto, WorkspaceRootDto,
};
use intention_proto::{
    ErrorCategoryDto, ErrorDto, ProjectId, RunId, SessionId, TimestampDto, ToolCallId, TurnId,
    WorkspaceBindingDto, WorkspaceId,
};
use intention_storage::{AcceptedTurnOutcomeDto, StartingRunModelContextDto};
use intention_test_support::fixture_snapshot;
use intention_tools::WorkspaceRoot;

/// Builds the canonical model arguments document of one `read` call.
fn read_arguments(path: &str) -> String {
    format!("{{\"path\":\"{path}\"}}")
}

fn invoke_read_input(path: &str) -> ToolInvocationRequestDto {
    invoke_read_input_in_workspace(
        &WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy()).expect("workspace"),
        )
        .expect("workspace is valid"),
        path,
    )
}

fn invoke_read_input_in_workspace(root: &WorkspaceRoot, path: &str) -> ToolInvocationRequestDto {
    ToolInvocationRequestDto::new(
        root.clone(),
        SessionId::new(),
        RunId::new(),
        ToolCallId::new(),
        "read",
        fixture_time(),
    )
    .with_arguments_json(read_arguments(path))
}
fn fixture_time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid")
}

/// Unwraps the content of one completed invocation outcome; any other outcome
/// is a fixture error.
fn completed_outcome(outcome: ToolResultOutcomeDto) -> String {
    match outcome {
        ToolResultOutcomeDto::Completed { content } => content,
        other => unreachable!("unexpected non-completed invocation: {other:?}"),
    }
}

fn projection(session_id: SessionId, active_run: Option<RunProjectionDto>) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
        active_run,
    )
    .expect("fixture projection is valid")
}

fn hello_tool_root(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("intention-publish-{tag}-{}", SessionId::new()));
    fs::create_dir_all(&root).expect("root");
    fs::write(root.join("hello.txt"), "hello").expect("hello fixture");
    root
}

fn hello_workspace(root: &std::path::Path) -> WorkspaceRoot {
    WorkspaceRoot::resolve(&WorkspaceRootDto::parse(root.to_string_lossy()).expect("dto"))
        .expect("workspace")
}

fn cancelled_execute_arguments() -> String {
    r#"{"command":"sleep 1"}"#.to_owned()
}

/// The arguments of an execute call that reports its own start and keeps
/// running.
///
/// The command writes `sentinel.txt` into the invocation's working directory
/// and then waits, so a fixture can synchronize on the observed effect before
/// it cancels the call.
fn started_execute_arguments() -> String {
    if cfg!(windows) {
        r#"{"command":"echo started> sentinel.txt & ping -n 2 127.0.0.1"}"#.to_owned()
    } else {
        r#"{"command":"printf x > sentinel.txt; sleep 2"}"#.to_owned()
    }
}

/// The arguments of an execute call whose own declared deadline ends the call.
fn timed_out_execute_arguments() -> String {
    if cfg!(windows) {
        r#"{"command":"ping -n 5 127.0.0.1","timeout_seconds":1}"#.to_owned()
    } else {
        r#"{"command":"sleep 5","timeout_seconds":1}"#.to_owned()
    }
}

#[test]
fn local_tool_success_records_admission_and_completion() {
    let root = hello_tool_root("success");
    let session = SessionId::new();
    let run = RunId::new();
    let call = ToolCallId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let result = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            ToolInvocationRequestDto::new(
                hello_workspace(&root),
                session,
                run,
                call,
                "read",
                fixture_time(),
            )
            .with_arguments_json(read_arguments("hello.txt")),
            &RecordingCommitObserver::new(),
        )
        .expect("tool succeeds");
    assert_eq!(completed_outcome(result), "hello");

    // Exactly two transactions commit: the call row before dispatch, then the
    // terminal result row with its answering transcript row.
    let messages = repository.committed_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].kind(), MessageKindDto::ToolCall);
    assert_eq!(messages[0].tool_call_id(), Some(call));
    assert_eq!(
        messages[0].tool_id().map(ToolIdentityDto::wire_name),
        Some("read")
    );
    assert_eq!(messages[1].kind(), MessageKindDto::ToolResult);
    assert_eq!(messages[1].text(), "hello");
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Completed);
    assert_eq!(results[0].content(), "hello");
    assert_eq!(results[0].session_id(), session);
    assert_eq!(results[0].run_id(), run);
    assert_eq!(results[0].call_id(), call);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_empty_search_result_still_commits_one_terminal_answer() {
    let root = hello_tool_root("empty-glob");
    let session = SessionId::new();
    let run = RunId::new();
    let call = ToolCallId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let result = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            ToolInvocationRequestDto::new(
                hello_workspace(&root),
                session,
                run,
                call,
                "glob",
                fixture_time(),
            )
            .with_arguments_json(r#"{"pattern":"*.none"}"#.to_owned()),
            &RecordingCommitObserver::new(),
        )
        .expect("an empty search result answers its call instead of failing");

    // An ordinary no-match search renders its canonical placeholder, and the
    // call row committed before dispatch is answered by exactly one terminal
    // result row.
    assert_eq!(completed_outcome(result), "[no paths]");
    let messages = repository.committed_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].kind(), MessageKindDto::ToolCall);
    assert_eq!(messages[0].tool_call_id(), Some(call));
    assert_eq!(messages[1].kind(), MessageKindDto::ToolResult);
    assert_eq!(messages[1].text(), "[no paths]");
    assert_eq!(messages[1].tool_call_id(), Some(call));
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Completed);
    assert_eq!(results[0].content(), "[no paths]");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_tool_rejects_storage_before_execution() {
    let root = hello_tool_root("storage-reject");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository
        .commit_error
        .lock()
        .expect("the fixture mutex is not poisoned") =
        Some(ErrorDto::unavailable("storage_down", "storage unavailable"));
    let error = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            invoke_read_input_in_workspace(&hello_workspace(&root), "hello.txt"),
            &RecordingCommitObserver::new(),
        )
        .expect_err("storage failure is propagated");
    assert_eq!(error.code(), "storage_down");
    // The call row commits before dispatch, so a refused admission leaves no
    // trace and the readable file was never invoked.
    assert!(repository.committed_messages().is_empty());
    assert!(repository.committed_results().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn committed_rows_name_the_logical_path_and_never_the_absolute_root() {
    let root = std::env::temp_dir().join(format!("intention-redaction-{}", SessionId::new()));
    fs::create_dir_all(&root).expect("root");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            invoke_read_input_in_workspace(
                &WorkspaceRoot::resolve(
                    &WorkspaceRootDto::parse(root.to_string_lossy()).expect("workspace"),
                )
                .expect("resolved workspace"),
                "missing",
            ),
            &RecordingCommitObserver::new(),
        )
        .expect("a missing file is a durable safe failure");
    let ToolResultOutcomeDto::Failed { error, .. } = outcome else {
        unreachable!("a missing file fails the call");
    };
    let rendered = format!(
        "{error:?} {:?} {:?}",
        repository.committed_messages(),
        repository.committed_results()
    );
    assert!(!rendered.contains(&root.to_string_lossy().to_string()));
    assert!(rendered.contains("unable to read missing:"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn unknown_tool_calls_commit_a_guided_failure_instead_of_stopping() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            ToolInvocationRequestDto::new(
                WorkspaceRoot::resolve(
                    &WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy())
                        .expect("workspace dto"),
                )
                .expect("workspace is valid"),
                SessionId::new(),
                RunId::new(),
                ToolCallId::new(),
                "frobnicate",
                fixture_time(),
            )
            .with_arguments_json(r#"{"path":"missing"}"#),
            &RecordingCommitObserver::new(),
        )
        .expect("an unknown tool is a durable, model-visible failure");
    let ToolResultOutcomeDto::Failed { error, content } = outcome else {
        unreachable!("an unknown tool fails the call");
    };
    assert_eq!(error.code(), "unknown_tool");
    assert!(content.contains("read, write, edit, execute, glob, grep"));
    // The call row and exactly one terminal Failed result commit; the model's
    // mistake never stops the run.
    let messages = repository.committed_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].kind(), MessageKindDto::ToolCall);
    assert_eq!(
        messages[0].tool_id().map(ToolIdentityDto::wire_name),
        Some("frobnicate")
    );
    assert_eq!(messages[1].kind(), MessageKindDto::ToolResult);
    assert_eq!(messages[1].text(), content);
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Failed);
}

#[test]
fn an_accepted_turn_publishes_its_committed_row_and_run_once() {
    // The turn path hands its committed values to the same sink the model-run
    // and tool paths use: a session's subscribers see the prompt row first and
    // then the run it started, exactly once each.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let message = MessageProjectionDto::new(
        row_id(),
        session_id,
        Some(run_id),
        MessageKindDto::User,
        "hello",
        None,
        None,
    )
    .expect("fixture user row is valid");
    let run = RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        RunStatusDto::Starting,
        None,
        fixture_time(),
        None,
        0,
    )
    .expect("fixture run projection is valid");
    let repository = FakeRepository::with_accepted(Ok(AcceptedTurnOutcomeDto::Started {
        run,
        message: message.clone(),
    }));
    let observer = RecordingCommitObserver::new();
    let accepted = ApplicationService::new(&repository)
        .send_user_turn(
            SendUserTurnCommandDto::new(session_id, "hello").expect("command is valid"),
            fixture_snapshot(),
            fixture_time(),
            &observer,
        )
        .expect("the accepted turn is returned");
    assert!(matches!(accepted, AcceptedTurnOutcomeDto::Started { .. }));
    assert_eq!(
        observer.commits(),
        vec![
            SessionCommitDto::Content(message),
            SessionCommitDto::Status(run),
        ],
        "the committed prompt row precedes the run it started"
    );
}

#[test]
fn a_queued_turn_publishes_its_acceptance_alone() {
    // A queued turn has no transcript row yet: it becomes visible to a
    // session's subscribers when its row commits as it joins the run, so this
    // path publishes nothing at acceptance.
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Ok(AcceptedTurnOutcomeDto::Pending(
        PendingTurnProjectionDto::new(session_id, TurnId::new(), "queued")
            .expect("fixture pending turn is valid"),
    )));
    let observer = RecordingCommitObserver::new();
    let accepted = ApplicationService::new(&repository)
        .send_user_turn(
            SendUserTurnCommandDto::new(session_id, "queued").expect("command is valid"),
            fixture_snapshot(),
            fixture_time(),
            &observer,
        )
        .expect("the queued turn is returned");
    assert!(matches!(accepted, AcceptedTurnOutcomeDto::Pending(_)));
    assert!(
        observer.commits().is_empty(),
        "a queued turn commits no row and publishes nothing"
    );
}

#[test]
fn send_user_turn_parameters_and_schedule_validation_cover_the_durable_selection() {
    let command = SendUserTurnCommandDto::new(SessionId::new(), "hello").expect("command is valid");
    let config = fixture_snapshot();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable(
        "turn_admission_unavailable",
        "the durable session refused the turn",
    )));
    let error = ApplicationService::new(&repository)
        .send_user_turn(
            command,
            config.clone(),
            fixture_time(),
            &RecordingCommitObserver::new(),
        )
        .expect_err("admission failure is propagated");
    assert_eq!(error.code(), "turn_admission_unavailable");
    let inputs = repository
        .accepted_inputs
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(inputs[0].config_snapshot().resolved(), config.resolved());
    assert_eq!(inputs[0].occurred_at(), fixture_time());
    drop(inputs);

    // A durable context for another run cannot become the requested schedule.
    let session_id = SessionId::new();
    let requested_run = RunId::new();
    *repository
        .starting_context
        .lock()
        .expect("the fixture mutex is not poisoned") =
        Some(starting_context(session_id, RunId::new(), &config));
    let error = ApplicationService::new(&repository)
        .schedule_starting_run(session_id, requested_run, RunCancellation::new())
        .expect_err("mismatched schedule is rejected");
    assert_eq!(error.code(), "invalid_model_run_schedule");

    // A context that names the requested run reconstructs the durable
    // selection, its message list, and its advertised tools into the execution
    // input.
    let matching_run = RunId::new();
    *repository
        .starting_context
        .lock()
        .expect("the fixture mutex is not poisoned") =
        Some(starting_context(session_id, matching_run, &config));
    let scheduled = ApplicationService::new(&repository)
        .schedule_starting_run(session_id, matching_run, RunCancellation::new())
        .expect("matching schedule is accepted");
    assert_eq!(scheduled.session_id(), session_id);
    assert_eq!(scheduled.run_id(), matching_run);
    assert_eq!(scheduled.safe_config(), &config);
    let request = scheduled.request();
    assert_eq!(request.run_id(), matching_run);
    assert_eq!(request.model(), "fixture");
    assert_eq!(request.messages().len(), 3);
    assert!(!request.tools().is_empty());
}

/// Builds one committed tool row of a previous run in the fixture session.
fn tool_row_fixture(
    session_id: SessionId,
    run_id: RunId,
    kind: MessageKindDto,
    text: &str,
    call_id: ToolCallId,
) -> MessageProjectionDto {
    let identity = ToolIdentityDto::parse("read").expect("the fixture tool is declared");
    let tool = match kind {
        MessageKindDto::ToolCall => ToolRowDto::Call(
            ToolCallRowDto::new(
                call_id,
                identity,
                ToolInput::from_arguments_json("read", text).ok(),
            )
            .expect("fixture call row is valid"),
        ),
        MessageKindDto::ToolResult => ToolRowDto::Result(
            ToolResultRowDto::new(
                call_id,
                identity,
                ToolResultStatusDto::Completed,
                Some(ToolResult::Read(TextResult {
                    text: BoundedText::new(text).expect("fixture text is bounded"),
                    truncated: false,
                })),
                Vec::new(),
            )
            .expect("fixture result row is valid"),
        ),
        _ => unreachable!("the tool-row fixture serves tool and result rows"),
    };
    MessageProjectionDto::new(
        row_id(),
        session_id,
        Some(run_id),
        kind,
        text,
        None,
        Some(tool),
    )
    .expect("fixture tool row is valid")
}

/// Asserts that every replayed assistant tool-call message stays answered by
/// the tool-role messages that immediately follow it.
fn assert_tool_calls_stay_answered(messages: &[ModelMessageDto]) {
    for (index, message) in messages.iter().enumerate() {
        let Some(calls) = message.tool_calls() else {
            continue;
        };
        let replies = messages[index + 1..]
            .iter()
            .take_while(|reply| reply.role() == ModelRoleDto::Tool)
            .collect::<Vec<_>>();
        assert_eq!(replies.len(), calls.len(), "every tool call stays answered");
        assert!(
            calls
                .iter()
                .zip(&replies)
                .all(|(call, reply)| reply.tool_call_id() == Some(call.call_id())),
            "each tool reply answers its own call"
        );
    }
}

#[test]
fn starting_run_context_rebuilds_the_committed_tool_exchange() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let previous_run = RunId::new();
    let config = fixture_snapshot();
    let call_id = ToolCallId::new();
    let context = StartingRunModelContextDto::new(
        session_id,
        run_id,
        config,
        vec![
            MessageProjectionDto::new(
                row_id(),
                session_id,
                None,
                MessageKindDto::User,
                "Read hello.txt",
                None,
                None,
            )
            .expect("context message is valid"),
            tool_row_fixture(
                session_id,
                previous_run,
                MessageKindDto::ToolCall,
                r#"{"path":"hello.txt"}"#,
                call_id,
            ),
            tool_row_fixture(
                session_id,
                previous_run,
                MessageKindDto::ToolResult,
                "hello world",
                call_id,
            ),
            MessageProjectionDto::new(
                row_id(),
                session_id,
                None,
                MessageKindDto::Assistant,
                "done",
                None,
                None,
            )
            .expect("context message is valid"),
            MessageProjectionDto::new(
                row_id(),
                session_id,
                Some(run_id),
                MessageKindDto::User,
                "latest",
                None,
                None,
            )
            .expect("context message is valid"),
        ],
    )
    .expect("fixture context is valid");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository
        .starting_context
        .lock()
        .expect("the fixture mutex is not poisoned") = Some(context);

    let scheduled = ApplicationService::new(&repository)
        .schedule_starting_run(session_id, run_id, RunCancellation::new())
        .expect("a committed tool exchange schedules the next run");
    let messages = scheduled.request().messages();
    assert_eq!(
        messages.len(),
        5,
        "every committed row becomes its model message"
    );
    assert_eq!(messages[0].role(), ModelRoleDto::User);
    assert_eq!(messages[0].content(), "Read hello.txt");
    assert_eq!(messages[1].role(), ModelRoleDto::Assistant);
    let calls = messages[1]
        .tool_calls()
        .expect("the call row carries its tool call");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].call_id(), call_id);
    assert_eq!(calls[0].name(), "read");
    assert_eq!(calls[0].arguments_json(), r#"{"path":"hello.txt"}"#);
    assert_eq!(messages[2].role(), ModelRoleDto::Tool);
    assert_eq!(messages[2].tool_call_id(), Some(call_id));
    assert_eq!(messages[2].content(), "hello world");
    assert_eq!(messages[3].role(), ModelRoleDto::Assistant);
    assert_eq!(messages[3].content(), "done");
    assert_eq!(messages[4].role(), ModelRoleDto::User);
    assert_eq!(messages[4].content(), "latest");
    assert_tool_calls_stay_answered(messages);
}

#[test]
fn starting_run_context_skips_reasoning_only_assistant_rows() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot();
    let context = StartingRunModelContextDto::new(
        session_id,
        run_id,
        config,
        vec![
            MessageProjectionDto::new(
                row_id(),
                session_id,
                None,
                MessageKindDto::User,
                "first user",
                None,
                None,
            )
            .expect("context message is valid"),
            // A completed step that reasoned without answering: durable
            // transcript evidence that never enters the model context.
            MessageProjectionDto::new(
                row_id(),
                session_id,
                None,
                MessageKindDto::Assistant,
                "",
                Some("step reasoning".to_owned()),
                None,
            )
            .expect("a reasoning-only assistant row is coherent"),
            MessageProjectionDto::new(
                row_id(),
                session_id,
                None,
                MessageKindDto::Assistant,
                "the answer",
                None,
                None,
            )
            .expect("context message is valid"),
            MessageProjectionDto::new(
                row_id(),
                session_id,
                Some(run_id),
                MessageKindDto::User,
                "current user",
                None,
                None,
            )
            .expect("context message is valid"),
        ],
    )
    .expect("fixture context is valid");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository
        .starting_context
        .lock()
        .expect("the fixture mutex is not poisoned") = Some(context);

    let scheduled = ApplicationService::new(&repository)
        .schedule_starting_run(session_id, run_id, RunCancellation::new())
        .expect("the reasoning-only row never blocks scheduling");
    let messages = scheduled.request().messages();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.role(), message.content()))
            .collect::<Vec<_>>(),
        vec![
            (ModelRoleDto::User, "first user"),
            (ModelRoleDto::Assistant, "the answer"),
            (ModelRoleDto::User, "current user"),
        ],
        "reasoning is never replayed into the model context"
    );
}

#[test]
fn starting_run_context_omits_unpaired_tool_rows() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let previous_run = RunId::new();
    let config = fixture_snapshot();
    let unanswered = ToolCallId::new();
    let orphaned = ToolCallId::new();
    let answered = ToolCallId::new();
    let context = StartingRunModelContextDto::new(
        session_id,
        run_id,
        config,
        vec![
            MessageProjectionDto::new(
                row_id(),
                session_id,
                None,
                MessageKindDto::User,
                "first",
                None,
                None,
            )
            .expect("context message is valid"),
            // A committed call whose result never committed: the crash window.
            tool_row_fixture(
                session_id,
                previous_run,
                MessageKindDto::ToolCall,
                r#"{"path":"unanswered.txt"}"#,
                unanswered,
            ),
            // A committed result whose call row does not exist.
            tool_row_fixture(
                session_id,
                previous_run,
                MessageKindDto::ToolResult,
                "orphan result",
                orphaned,
            ),
            tool_row_fixture(
                session_id,
                previous_run,
                MessageKindDto::ToolCall,
                r#"{"path":"hello.txt"}"#,
                answered,
            ),
            tool_row_fixture(
                session_id,
                previous_run,
                MessageKindDto::ToolResult,
                "hello world",
                answered,
            ),
            MessageProjectionDto::new(
                row_id(),
                session_id,
                None,
                MessageKindDto::Assistant,
                "done",
                None,
                None,
            )
            .expect("context message is valid"),
            MessageProjectionDto::new(
                row_id(),
                session_id,
                Some(run_id),
                MessageKindDto::User,
                "latest",
                None,
                None,
            )
            .expect("context message is valid"),
        ],
    )
    .expect("fixture context is valid");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository
        .starting_context
        .lock()
        .expect("the fixture mutex is not poisoned") = Some(context);

    let scheduled = ApplicationService::new(&repository)
        .schedule_starting_run(session_id, run_id, RunCancellation::new())
        .expect("unpaired tool rows never fail the schedule");
    let messages = scheduled.request().messages();
    assert_eq!(
        messages
            .iter()
            .map(ModelMessageDto::role)
            .collect::<Vec<_>>(),
        vec![
            ModelRoleDto::User,
            ModelRoleDto::Assistant,
            ModelRoleDto::Tool,
            ModelRoleDto::Assistant,
            ModelRoleDto::User,
        ],
        "only the paired exchange replays, in durable order"
    );
    let calls = messages[1]
        .tool_calls()
        .expect("the paired call replays as an assistant tool call");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].call_id(), answered);
    assert_eq!(messages[2].tool_call_id(), Some(answered));
    assert_eq!(messages[2].content(), "hello world");
    assert_tool_calls_stay_answered(messages);
    assert!(
        messages.iter().all(|message| {
            message.tool_call_id() != Some(unanswered)
                && message.tool_call_id() != Some(orphaned)
                && message
                    .tool_calls()
                    .is_none_or(|calls| calls.iter().all(|call| call.call_id() == answered))
        }),
        "no unpaired row survives into the model context"
    );
}

#[test]
fn create_workflow_maps_the_committed_session_projection() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository
        .created
        .lock()
        .expect("the fixture mutex is not poisoned") = Ok(projection(session_id, None));
    let application = ApplicationService::new(&repository);
    let create = CreateSessionCommandDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        Some(RunModeDto::Build),
        None,
    );

    let observer = RecordingCommitObserver::new();
    let created = application
        .create_session(create, fixture_snapshot(), fixture_time(), &observer)
        .expect("create maps");
    assert_eq!(created.session().session_id(), session_id);
}

#[test]
fn session_creation_joins_the_durable_binding_of_its_workspace_root() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository
        .created
        .lock()
        .expect("the fixture mutex is not poisoned") = Ok(projection(session_id, None));
    let bound_project = ProjectId::new();
    let bound_workspace = WorkspaceId::new();
    *repository
        .workspace_binding
        .lock()
        .expect("the fixture mutex is not poisoned") =
        Some(WorkspaceBindingDto::new(bound_project, bound_workspace));
    let create = CreateSessionCommandDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        Some(RunModeDto::Build),
        None,
    );

    ApplicationService::new(&repository)
        .create_session(
            create,
            fixture_snapshot(),
            fixture_time(),
            &RecordingCommitObserver::new(),
        )
        .expect("create maps");

    let forwarded = repository
        .create_commands
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(forwarded.len(), 1, "one creation reaches the repository");
    assert_eq!(
        forwarded[0].project_id(),
        bound_project,
        "the durable project binding replaces the proposed identity"
    );
    assert_eq!(
        forwarded[0].workspace_id(),
        bound_workspace,
        "the durable workspace binding replaces the proposed identity"
    );
    assert_eq!(
        forwarded[0].session_id(),
        session_id,
        "the requested session identity is kept: every creation is a fresh session"
    );
    assert_eq!(forwarded[0].workspace_root(), &workspace_root());
    assert_eq!(forwarded[0].mode(), Some(RunModeDto::Build));
}

#[test]
fn session_creation_keeps_its_identities_for_an_unbound_workspace_root() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository
        .created
        .lock()
        .expect("the fixture mutex is not poisoned") = Ok(projection(session_id, None));
    let project_id = ProjectId::new();
    let workspace_id = WorkspaceId::new();
    let create = CreateSessionCommandDto::new(
        project_id,
        session_id,
        workspace_id,
        workspace_root(),
        Some(RunModeDto::Plan),
        None,
    );

    ApplicationService::new(&repository)
        .create_session(
            create,
            fixture_snapshot(),
            fixture_time(),
            &RecordingCommitObserver::new(),
        )
        .expect("create maps");

    let forwarded = repository
        .create_commands
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(forwarded.len(), 1);
    assert_eq!(
        forwarded[0].project_id(),
        project_id,
        "an unbound root establishes the proposed binding"
    );
    assert_eq!(forwarded[0].workspace_id(), workspace_id);
    assert_eq!(forwarded[0].mode(), Some(RunModeDto::Plan));
}

#[test]
fn the_core_default_resolves_absent_to_build() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository
        .created
        .lock()
        .expect("the fixture mutex is not poisoned") = Ok(projection(session_id, None));
    let create = CreateSessionCommandDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        None,
        None,
    );

    ApplicationService::new(&repository)
        .create_session(
            create,
            fixture_snapshot(),
            fixture_time(),
            &RecordingCommitObserver::new(),
        )
        .expect("create maps");

    let forwarded = repository
        .create_commands
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(forwarded.len(), 1, "one creation reaches the repository");
    assert_eq!(
        forwarded[0].mode(),
        Some(RunModeDto::DEFAULT),
        "the core resolves a command that declares no mode to the durable default before the write"
    );
    assert_eq!(RunModeDto::DEFAULT, RunModeDto::Build);
}

/// Builds one combined creation command carrying a first turn.
fn create_with_first_turn(session_id: SessionId, content: &str) -> CreateSessionCommandDto {
    CreateSessionCommandDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        Some(RunModeDto::Build),
        Some(
            CreateSessionFirstTurnDto::new(content)
                .expect("fixture first-turn content is non-blank"),
        ),
    )
}

/// Builds the durable conflict the repository reports for a repeated session id.
fn session_already_exists() -> ErrorDto {
    ErrorDto::new(
        "session_already_exists",
        ErrorCategoryDto::Conflict,
        "the durable session already exists",
        false,
    )
    .expect("fixture conflict is valid")
}

/// Returns one pending first-turn outcome for its session.
fn pending_first_turn(session_id: SessionId, content: &str) -> AcceptedTurnOutcomeDto {
    AcceptedTurnOutcomeDto::Pending(
        PendingTurnProjectionDto::new(session_id, TurnId::new(), content)
            .expect("fixture pending turn is valid"),
    )
}

#[test]
fn create_session_with_turn_commits_the_session_before_the_turn() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Ok(pending_first_turn(session_id, "first")));
    *repository
        .created
        .lock()
        .expect("the fixture mutex is not poisoned") = Ok(projection(session_id, None));
    let observer = RecordingCommitObserver::new();
    let created = ApplicationService::new(&repository)
        .create_session(
            create_with_first_turn(session_id, "first"),
            fixture_snapshot(),
            fixture_time(),
            &observer,
        )
        .expect("the combined creation commits");
    assert_eq!(created.session().session_id(), session_id);
    assert!(matches!(
        created.first_turn(),
        Some(AcceptedTurnOutcomeDto::Pending(_))
    ));
    assert_eq!(
        repository
            .calls
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone(),
        vec!["create_session", "accept_user_turn"],
        "the durable session commits before its first turn"
    );
    let inputs = repository
        .accepted_inputs
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(inputs.len(), 1, "one turn reaches the repository");
    assert_eq!(
        inputs[0].config_snapshot().resolved(),
        fixture_snapshot().resolved(),
        "the carried first turn freezes the supplied configuration"
    );
}

#[test]
fn a_repeated_creation_reads_the_durable_session_back_and_accepts_its_turn() {
    let session_id = SessionId::new();
    let pending = PendingTurnProjectionDto::new(session_id, TurnId::new(), "first")
        .expect("fixture pending turn is valid");
    let repository =
        FakeRepository::with_accepted(Ok(AcceptedTurnOutcomeDto::Pending(pending.clone())));
    // An earlier command already committed this session: the creation reports
    // its durable conflict and the existing session is read back instead.
    *repository
        .created
        .lock()
        .expect("the fixture mutex is not poisoned") = Err(session_already_exists());
    *repository
        .loaded_projection
        .lock()
        .expect("the fixture mutex is not poisoned") = Some(projection(session_id, None));
    let created = ApplicationService::new(&repository)
        .create_session(
            create_with_first_turn(session_id, "first"),
            fixture_snapshot(),
            fixture_time(),
            &RecordingCommitObserver::new(),
        )
        .expect("an existing session is read back");
    assert_eq!(created.session().session_id(), session_id);
    let Some(AcceptedTurnOutcomeDto::Pending(accepted)) = created.first_turn() else {
        unreachable!("the carried turn outcome is the pending turn")
    };
    assert_eq!(accepted.turn_id(), pending.turn_id());
    assert_eq!(
        repository
            .calls
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone(),
        vec!["create_session", "accept_user_turn"],
        "the carried turn is accepted as a new turn of the existing session"
    );
}

#[test]
fn a_typed_first_turn_rejection_surfaces_after_the_session_commits() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::validation(
        "invalid_turn_content",
        "user turn content exceeds the durable canonical size limit",
    )));
    *repository
        .created
        .lock()
        .expect("the fixture mutex is not poisoned") = Ok(projection(session_id, None));
    let error = ApplicationService::new(&repository)
        .create_session(
            create_with_first_turn(session_id, &"x".repeat(512 * 1024 + 1)),
            fixture_snapshot(),
            fixture_time(),
            &RecordingCommitObserver::new(),
        )
        .expect_err("an oversized first turn is rejected");
    assert_eq!(error.code(), "invalid_turn_content");
    assert_eq!(
        repository
            .create_commands
            .lock()
            .expect("the fixture mutex is not poisoned")
            .len(),
        1,
        "the session committed before the turn was rejected"
    );
}

#[test]
fn a_failed_turn_leaves_the_committed_session_unchanged() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable(
        "turn_admission_unavailable",
        "the turn could not be admitted",
    )));
    *repository
        .created
        .lock()
        .expect("the fixture mutex is not poisoned") = Ok(projection(session_id, None));
    let application = ApplicationService::new(&repository);
    let error = application
        .create_session(
            create_with_first_turn(session_id, "first"),
            fixture_snapshot(),
            fixture_time(),
            &RecordingCommitObserver::new(),
        )
        .expect_err("the rejected first turn surfaces");
    assert_eq!(error.code(), "turn_admission_unavailable");
    assert_eq!(
        repository
            .create_commands
            .lock()
            .expect("the fixture mutex is not poisoned")
            .len(),
        1,
        "the session stays durable although the carried turn was rejected"
    );

    // A later creation names the same durable session: the existing session is
    // read back and the carried turn is accepted as its new turn.
    *repository
        .created
        .lock()
        .expect("the fixture mutex is not poisoned") = Err(session_already_exists());
    *repository
        .loaded_projection
        .lock()
        .expect("the fixture mutex is not poisoned") = Some(projection(session_id, None));
    *repository
        .accepted
        .lock()
        .expect("the fixture mutex is not poisoned") = Ok(pending_first_turn(session_id, "first"));
    let accepted = application
        .create_session(
            create_with_first_turn(session_id, "first"),
            fixture_snapshot(),
            fixture_time(),
            &RecordingCommitObserver::new(),
        )
        .expect("the existing session accepts the carried turn");
    assert_eq!(accepted.session().session_id(), session_id);
    assert!(matches!(
        accepted.first_turn(),
        Some(AcceptedTurnOutcomeDto::Pending(_))
    ));
}

#[test]
fn committed_rows_reach_the_commit_sink_only_after_their_own_commit() {
    let root = hello_tool_root("publication-order");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            invoke_read_input_in_workspace(&hello_workspace(&root), "hello.txt"),
            &publisher,
        )
        .expect("an infallible commit sink publishes every committed row");
    assert!(matches!(outcome, ToolResultOutcomeDto::Completed { .. }));

    // The sink observed the committed call row before execution and the
    // committed terminal row after its own commit, in that order: a committed
    // row can no longer be refused and fail the invocation after the commit.
    publisher.assert_call_row_then_one_terminal();
    let _ = fs::remove_dir_all(root);
}

#[test]
fn selected_commit_failures_propagate_from_each_commit_point() {
    // The call row commits first and the terminal result row second; each
    // selected failure must surface and leave only the earlier commit durable.
    let scenarios: Vec<(&str, usize, &str, usize)> = vec![
        ("tool-call-commit", 1, "hello.txt", 0),
        ("completed-commit", 2, "hello.txt", 1),
        ("tool-failure-commit", 2, "missing.txt", 1),
    ];
    for (label, failing_call, path, expected_messages) in scenarios {
        let root = hello_tool_root("commit-failure");
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        *repository
            .commit_failures
            .lock()
            .expect("the fixture mutex is not poisoned") = vec![failing_call];
        let error = ApplicationService::new(&repository)
            .invoke_local_tool_with_publication(
                invoke_read_input_in_workspace(&hello_workspace(&root), path),
                &RecordingCommitObserver::new(),
            )
            .expect_err("the selected commit failure must propagate");
        assert_eq!(error.code(), "append_unavailable", "scenario {label}");
        assert_eq!(
            repository.committed_messages().len(),
            expected_messages,
            "scenario {label}"
        );
        assert!(
            repository.committed_results().is_empty(),
            "scenario {label}"
        );
        let _ = fs::remove_dir_all(root);
    }
}

/// Publication probe that records the committed result-row count at publish time.
struct TerminalOrderingProbe<'a> {
    committed_result_rows: &'a AtomicUsize,
    publications: Mutex<Vec<MessageProjectionDto>>,
    results_at_publish: Mutex<Vec<usize>>,
}

impl<'a> TerminalOrderingProbe<'a> {
    const fn new(repository: &'a FakeRepository) -> Self {
        Self {
            committed_result_rows: &repository.committed_result_rows,
            publications: Mutex::new(Vec::new()),
            results_at_publish: Mutex::new(Vec::new()),
        }
    }

    fn published(&self) -> Vec<MessageProjectionDto> {
        self.publications
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone()
    }

    /// Asserts the boundary saw the committed call row before execution and the
    /// committed terminal row after its own commit, in that order.
    fn assert_call_row_then_one_terminal(&self) {
        let published = self.published();
        assert_eq!(published.len(), 2, "the call row and one terminal row");
        assert_eq!(published[0].kind(), MessageKindDto::ToolCall);
        assert_eq!(published[1].kind(), MessageKindDto::ToolResult);
        assert_eq!(
            *self
                .results_at_publish
                .lock()
                .expect("the fixture mutex is not poisoned"),
            vec![0, 1],
            "the terminal row publishes after its own durable commit"
        );
    }
}

impl SessionCommitObserver for TerminalOrderingProbe<'_> {
    fn observe_session_commit(&self, commit: &SessionCommitDto) {
        let SessionCommitDto::Content(message) = commit else {
            return;
        };
        self.results_at_publish
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push(self.committed_result_rows.load(Ordering::SeqCst));
        self.publications
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push(message.clone());
    }
}

/// Asserts exactly one terminal-result row exists with exact correlation and
/// that its answering transcript row committed with it.
fn assert_single_terminal_result(
    repository: &FakeRepository,
    session_id: SessionId,
    run_id: RunId,
    call_id: ToolCallId,
    tool_id: &str,
    status: ToolResultStatusDto,
) {
    let results = repository.committed_results();
    assert_eq!(
        results.len(),
        1,
        "exactly one terminal result row is committed"
    );
    let terminal = &results[0];
    assert_eq!(terminal.status(), status);
    assert_eq!(terminal.session_id(), session_id);
    assert_eq!(terminal.run_id(), run_id);
    assert_eq!(terminal.call_id(), call_id);
    assert_eq!(terminal.tool_id(), tool_id);
    let messages = repository.committed_messages();
    assert!(
        messages.iter().all(|row| row.session_id() == session_id
            && row.run_id() == Some(run_id)
            && row.tool_call_id() == Some(call_id)),
        "every committed row carries the exact invocation identity"
    );
    let answering = messages
        .last()
        .expect("the terminal result commits with its answering row");
    assert_eq!(answering.kind(), MessageKindDto::ToolResult);
    assert_eq!(
        answering.tool_id().map(ToolIdentityDto::wire_name),
        Some(tool_id)
    );
    assert_eq!(answering.text(), terminal.content());
}

#[test]
fn every_terminal_outcome_persists_one_correlated_result_before_publication() {
    let root = hello_tool_root("terminal-matrix");
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();

    // A successful outcome publishes only after its terminal Completed commit.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let result = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            ToolInvocationRequestDto::new(
                hello_workspace(&root),
                session_id,
                run_id,
                call_id,
                "read",
                fixture_time(),
            )
            .with_arguments_json(read_arguments("hello.txt")),
            &publisher,
        )
        .expect("read succeeds");
    assert_eq!(completed_outcome(result), "hello");
    assert_single_terminal_result(
        &repository,
        session_id,
        run_id,
        call_id,
        "read",
        ToolResultStatusDto::Completed,
    );
    // The terminal commit atomically carries the rendered result content with
    // the exact invocation identity.
    let completed = &repository.committed_results()[0];
    assert_eq!(completed.content(), "hello");
    assert_eq!(completed.occurred_at(), fixture_time());
    assert!(completed.metadata().is_empty());
    // Exactly one terminal result row exists when publication runs, proving
    // the terminal commit precedes the publication boundary.
    assert_eq!(
        *publisher
            .results_at_publish
            .lock()
            .expect("the fixture mutex is not poisoned"),
        vec![0, 1]
    );
    let publications = publisher.published();
    assert_eq!(publications.len(), 2);
    let terminal = &publications[1];
    assert_eq!(terminal.kind(), MessageKindDto::ToolResult);
    assert_eq!(terminal.session_id(), session_id);
    assert_eq!(terminal.run_id(), Some(run_id));
    assert_eq!(terminal.tool_call_id(), Some(call_id));
    assert_eq!(terminal.text(), "hello");
    drop(publisher);
    drop(repository);

    // A failed outcome commits correlated Failed evidence and never publishes.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            ToolInvocationRequestDto::new(
                hello_workspace(&root),
                session_id,
                run_id,
                call_id,
                "read",
                fixture_time(),
            )
            .with_arguments_json(read_arguments("missing.txt")),
            &publisher,
        )
        .expect("a missing file is a durable safe failure");
    let ToolResultOutcomeDto::Failed { error, content } = outcome else {
        unreachable!("a missing file fails the call");
    };
    assert_eq!(error.code(), "tool_read_failed");
    assert!(content.contains("tool_read_failed"));
    assert!(
        !content.contains(&root.to_string_lossy().to_string()),
        "guidance never carries the absolute workspace root"
    );
    assert!(
        content.contains("unable to read missing.txt:"),
        "guidance names the workspace-relative path it addressed"
    );
    assert!(
        content.contains("os error"),
        "guidance carries the OS error verbatim"
    );
    assert_single_terminal_result(
        &repository,
        session_id,
        run_id,
        call_id,
        "read",
        ToolResultStatusDto::Failed,
    );
    // The terminal Failed commit carries the model-visible failure guidance.
    let failed = &repository.committed_results()[0];
    assert!(failed.content().starts_with("the tool call failed\n"));
    assert!(failed.content().contains("tool_read_failed"));
    assert!(failed.metadata().is_empty());
    publisher.assert_call_row_then_one_terminal();
    drop(publisher);
    drop(repository);

    // A pre-start cancellation is a Partial outcome: correlated Partial
    // evidence is durable and the publication boundary is never reached.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let cancelled = RunCancellation::new();
    cancelled.cancel();
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            ToolInvocationRequestDto::new(
                hello_workspace(&root),
                session_id,
                run_id,
                call_id,
                "execute",
                fixture_time(),
            )
            .with_arguments_json(cancelled_execute_arguments())
            .with_cancellation(cancelled),
            &publisher,
        )
        .expect("a pre-start cancellation is a partial outcome");
    assert_eq!(
        outcome,
        ToolResultOutcomeDto::partial("[The tool call was stopped before a final result.]")
            .expect("partial content is valid")
    );
    assert_single_terminal_result(
        &repository,
        session_id,
        run_id,
        call_id,
        "execute",
        ToolResultStatusDto::Partial,
    );
    // The terminal Partial commit classifies the interrupted call with the
    // exact stopped notice, records the caller's stop, and never records a
    // completed row.
    assert_eq!(
        repository.committed_results()[0].content(),
        "[The tool call was stopped before a final result.]"
    );
    let stopped = &repository.committed_results()[0];
    assert_eq!(
        stopped
            .metadata()
            .iter()
            .map(|entry| (entry.key(), entry.value()))
            .collect::<Vec<_>>(),
        vec![("interruption", "tool_cancelled")],
        "a stopped call records the caller's stop as its durable cause"
    );
    assert_eq!(repository.completed_result_count(), 0);
    publisher.assert_call_row_then_one_terminal();
    drop(publisher);
    drop(repository);

    // An interrupted external process is a Partial outcome carrying the
    // captured output; correlated Partial evidence is durable and the call
    // never reaches the publication boundary.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let signal = RunCancellation::new();
    let cancellation = signal.clone();
    let sentinel = root.join("sentinel.txt");
    let canceller = std::thread::spawn(move || {
        // The sentinel proves the child started, so the cancellation can only
        // land while the execution is in flight.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !sentinel.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "execute child never produced its start sentinel"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        cancellation.cancel();
    });
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            ToolInvocationRequestDto::new(
                hello_workspace(&root),
                session_id,
                run_id,
                call_id,
                "execute",
                fixture_time(),
            )
            .with_arguments_json(started_execute_arguments())
            .with_cancellation(signal),
            &publisher,
        )
        .expect("an interrupted execute is a partial outcome");
    canceller.join().expect("cancellation helper completes");
    let ToolResultOutcomeDto::Partial { content } = outcome else {
        unreachable!("cancellation must interrupt the invocation");
    };
    assert!(content.contains(
        "[The tool call was stopped before a final result; the output above is partial.]"
    ));
    assert_single_terminal_result(
        &repository,
        session_id,
        run_id,
        call_id,
        "execute",
        ToolResultStatusDto::Partial,
    );
    // The terminal Partial commit keeps the captured output, records the
    // caller's stop, and never records a completed row.
    let partial = &repository.committed_results()[0];
    assert!(partial.content().contains("stdout:"));
    assert!(partial.content().contains(
        "[The tool call was stopped before a final result; the output above is partial.]"
    ));
    assert_eq!(
        partial
            .metadata()
            .iter()
            .map(|entry| (entry.key(), entry.value()))
            .collect::<Vec<_>>(),
        vec![("interruption", "tool_cancelled")],
        "an interrupted process keeps the caller's stop as its durable cause"
    );
    assert_eq!(repository.completed_result_count(), 0);
    publisher.assert_call_row_then_one_terminal();

    let _ = fs::remove_dir_all(root);
}

/// A call cut by its own declared deadline is a Partial outcome whose notice
/// names the timeout and whose durable evidence records the deadline it hit.
#[test]
fn a_declared_execute_timeout_records_its_deadline_as_partial_evidence() {
    let root = hello_tool_root("declared-timeout");
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            ToolInvocationRequestDto::new(
                hello_workspace(&root),
                session_id,
                run_id,
                call_id,
                "execute",
                fixture_time(),
            )
            .with_arguments_json(timed_out_execute_arguments()),
            &publisher,
        )
        .expect("a declared timeout is a partial outcome");
    let ToolResultOutcomeDto::Partial { content } = outcome else {
        unreachable!("a declared timeout must interrupt the invocation");
    };
    assert!(
        content.contains("hit its declared timeout of 1s and was terminated before a final result"),
        "the model reads the declared deadline it hit: {content}"
    );
    assert!(
        !content.contains("did not receive a final result"),
        "a declared deadline never reads as lost evidence: {content}"
    );
    assert_single_terminal_result(
        &repository,
        session_id,
        run_id,
        call_id,
        "execute",
        ToolResultStatusDto::Partial,
    );
    let partial = &repository.committed_results()[0];
    assert!(partial.content().contains("hit its declared timeout of 1s"));
    assert_eq!(
        partial
            .metadata()
            .iter()
            .map(|entry| (entry.key(), entry.value()))
            .collect::<Vec<_>>(),
        vec![
            ("interruption", "tool_timed_out"),
            ("declared_timeout_seconds", "1"),
        ],
        "the durable row names the cause and the deadline it hit"
    );
    assert_eq!(repository.completed_result_count(), 0);
    publisher.assert_call_row_then_one_terminal();

    let _ = fs::remove_dir_all(root);
}

#[test]
fn tool_call_row_commits_the_canonical_arguments_document() {
    let root = hello_tool_root("arguments");
    let call = ToolCallId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            invoke_read_input_in_workspace(&hello_workspace(&root), "hello.txt"),
            &RecordingCommitObserver::new(),
        )
        .expect("read succeeds");
    let messages = repository.committed_messages();
    assert_eq!(messages[0].kind(), MessageKindDto::ToolCall);
    assert_eq!(messages[0].text(), r#"{"path":"hello.txt"}"#);
    assert_eq!(
        messages[0].tool_id().map(ToolIdentityDto::wire_name),
        Some("read")
    );

    // A caller without the model's arguments still commits the call row; the
    // undecodable call is answered by one terminal Failed result instead of
    // stopping the run.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            ToolInvocationRequestDto::new(
                hello_workspace(&root),
                SessionId::new(),
                RunId::new(),
                call,
                "read",
                fixture_time(),
            ),
            &RecordingCommitObserver::new(),
        )
        .expect("a call without model arguments still commits its row");
    assert_eq!(repository.committed_messages()[0].text(), "{}");
    assert_eq!(repository.committed_messages().len(), 2);
    assert_eq!(
        repository.committed_results()[0].status(),
        ToolResultStatusDto::Failed
    );
    let _ = fs::remove_dir_all(root);
}

fn starting_context(
    session_id: SessionId,
    run_id: RunId,
    config: &ConfigSnapshotDto,
) -> StartingRunModelContextDto {
    StartingRunModelContextDto::new(
        session_id,
        run_id,
        config.clone(),
        vec![
            MessageProjectionDto::new(
                row_id(),
                session_id,
                None,
                MessageKindDto::User,
                "first",
                None,
                None,
            )
            .expect("context message is valid"),
            MessageProjectionDto::new(
                row_id(),
                session_id,
                None,
                MessageKindDto::Assistant,
                "answer",
                None,
                None,
            )
            .expect("context message is valid"),
            MessageProjectionDto::new(
                row_id(),
                session_id,
                Some(run_id),
                MessageKindDto::User,
                "latest",
                None,
                None,
            )
            .expect("context message is valid"),
        ],
    )
    .expect("fixture context is valid")
}

#[test]
fn terminal_commit_failure_propagates_from_the_tool_error_path() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository
        .commit_failures
        .lock()
        .expect("the fixture mutex is not poisoned") = vec![2];
    let error = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            invoke_read_input("missing"),
            &RecordingCommitObserver::new(),
        )
        .expect_err("terminal commit failure replaces the tool error");
    assert_eq!(error.code(), "append_unavailable");
    assert_eq!(repository.committed_messages().len(), 1);
    assert!(repository.committed_results().is_empty());
}

#[test]
fn committed_tool_result_content_preserves_control_characters_as_text() {
    let root = std::env::temp_dir().join(format!("intention-app-escape-{}", SessionId::new()));
    fs::create_dir_all(&root).expect("root");
    fs::write(root.join("control.txt"), "\u{8}\t\u{c}\r").expect("fixture file");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let result = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            invoke_read_input_in_workspace(&hello_workspace(&root), "control.txt"),
            &RecordingCommitObserver::new(),
        )
        .expect("control characters are readable text");
    assert_eq!(completed_outcome(result), "\u{8}\t\u{c}\r");
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Completed);
    assert_eq!(results[0].content(), "\u{8}\t\u{c}\r");
    assert_eq!(repository.committed_messages()[1].text(), "\u{8}\t\u{c}\r");
    let _ = fs::remove_dir_all(root);
}

/// One committed call row carries the typed arguments the dispatch decoded, so
/// no consumer re-decodes the arguments document the row also carries as text.
#[test]
fn a_committed_tool_call_row_carries_its_typed_arguments_projection() {
    let root = hello_tool_root("typed-call");
    let session = SessionId::new();
    let run = RunId::new();
    let call = ToolCallId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let result = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            ToolInvocationRequestDto::new(
                hello_workspace(&root),
                session,
                run,
                call,
                "read",
                fixture_time(),
            )
            .with_arguments_json(read_arguments("hello.txt")),
            &RecordingCommitObserver::new(),
        )
        .expect("tool succeeds");
    assert_eq!(completed_outcome(result), "hello");

    let messages = repository.committed_messages();
    let Some(ToolRowDto::Call(evidence)) = messages[0].tool() else {
        unreachable!("the committed call row carries its typed call evidence")
    };
    assert_eq!(evidence.call_id(), call);
    assert_eq!(evidence.tool_id().wire_name(), "read");
    let expected = ToolInput::from_arguments_json("read", &read_arguments("hello.txt"))
        .expect("fixture arguments decode");
    assert_eq!(
        evidence.arguments(),
        Some(&expected),
        "the committed projection is the decode the dispatch ran"
    );
    let _ = fs::remove_dir_all(root);
}

/// A call whose arguments document never decoded still commits its own durable
/// identity without inventing typed arguments for it, and its failure is a
/// model-visible result row with no payload.
#[test]
fn an_undecodable_call_row_commits_without_a_typed_projection() {
    let root = hello_tool_root("undecodable-call");
    let session = SessionId::new();
    let run = RunId::new();
    let call = ToolCallId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            ToolInvocationRequestDto::new(
                hello_workspace(&root),
                session,
                run,
                call,
                "read",
                fixture_time(),
            )
            .with_arguments_json(r#"{"path":42}"#),
            &RecordingCommitObserver::new(),
        )
        .expect("an undecodable call is a durable, model-visible failure");
    let ToolResultOutcomeDto::Failed { error, .. } = outcome else {
        unreachable!("an undecodable call fails the invocation");
    };
    assert_eq!(error.code(), "invalid_tool_input_json");

    let messages = repository.committed_messages();
    let Some(ToolRowDto::Call(evidence)) = messages[0].tool() else {
        unreachable!("the committed call row carries its typed call evidence")
    };
    assert_eq!(evidence.tool_id().wire_name(), "read");
    assert!(
        evidence.arguments().is_none(),
        "no typed arguments are invented for a document that never decoded"
    );
    let Some(ToolRowDto::Result(result)) = messages[1].tool() else {
        unreachable!("the answering row carries its typed result evidence")
    };
    assert_eq!(result.status(), ToolResultStatusDto::Failed);
    assert!(
        result.result().is_none(),
        "a failed result carries no payload"
    );
    let _ = fs::remove_dir_all(root);
}

/// The terminal result row carries the typed payload together with the
/// lifecycle status and the approved metadata the engine recorded.
#[test]
fn a_committed_tool_result_row_carries_status_and_metadata() {
    let root = hello_tool_root("typed-result");
    // A read past the tool's output bound is truncated, which the engine
    // records as approved metadata; the typed payload carries the same flag.
    std::fs::write(root.join("large.txt"), "x".repeat(64 * 1024 + 1)).expect("large fixture");
    let session = SessionId::new();
    let run = RunId::new();
    let call = ToolCallId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let result = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            ToolInvocationRequestDto::new(
                hello_workspace(&root),
                session,
                run,
                call,
                "read",
                fixture_time(),
            )
            .with_arguments_json(read_arguments("large.txt")),
            &RecordingCommitObserver::new(),
        )
        .expect("tool succeeds");
    assert!(completed_outcome(result).contains("[truncated]"));

    let messages = repository.committed_messages();
    let Some(ToolRowDto::Result(evidence)) = messages[1].tool() else {
        unreachable!("the answering row carries its typed result evidence")
    };
    assert_eq!(evidence.status(), ToolResultStatusDto::Completed);
    assert_eq!(
        evidence
            .metadata()
            .iter()
            .map(|entry| (entry.key(), entry.value()))
            .collect::<Vec<_>>(),
        vec![("truncated", "true")],
        "the approved metadata rides the committed projection"
    );
    let Some(ToolResult::Read(payload)) = evidence.result() else {
        unreachable!("a completed read result carries its typed read payload")
    };
    assert!(payload.truncated, "the typed payload carries the same flag");
    assert_eq!(payload.text.as_str(), "x".repeat(64 * 1024));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn active_run_returns_the_exact_active_run_and_rejects_every_other_lookup() {
    // The terminal stop resolves through this lookup: only the session's own
    // exact active run may be addressed, so another run identity and a session
    // with no active run answer the same typed rejection.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let active = RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        RunStatusDto::Running,
        None,
        fixture_time(),
        None,
        0,
    )
    .expect("fixture run projection is coherent");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository
        .loaded_projection
        .lock()
        .expect("the fixture mutex is not poisoned") = Some(projection(session_id, Some(active)));

    let service = ApplicationService::new(&repository);
    assert_eq!(
        service
            .active_run(session_id, run_id)
            .expect("the session's exact active run is returned")
            .run_id(),
        run_id
    );
    assert_eq!(
        service
            .active_run(session_id, RunId::new())
            .expect_err("a run the session does not own is never its active run")
            .code(),
        "active_run_not_found"
    );

    *repository
        .loaded_projection
        .lock()
        .expect("the fixture mutex is not poisoned") = Some(projection(session_id, None));
    assert_eq!(
        service
            .active_run(session_id, run_id)
            .expect_err("a session without an active run has none to address")
            .code(),
        "active_run_not_found"
    );
}

#[test]
fn a_failed_creation_propagates_without_accepting_its_first_turn() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository
        .created
        .lock()
        .expect("the fixture mutex is not poisoned") = Err(ErrorDto::unavailable(
        "storage_unavailable",
        "the durable store is unavailable",
    ));

    let error = ApplicationService::new(&repository)
        .create_session(
            create_with_first_turn(session_id, "first"),
            fixture_snapshot(),
            fixture_time(),
            &RecordingCommitObserver::new(),
        )
        .expect_err("a creation failure other than an existing session is propagated");
    assert_eq!(error.code(), "storage_unavailable");
    assert_eq!(
        repository
            .calls
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone(),
        vec!["create_session"],
        "a failed creation never reaches the first turn it carried"
    );
}

#[test]
fn starting_run_context_replays_committed_notices_to_the_model() {
    // A notice is durable transcript evidence the model must see: the rebuilt
    // context carries it as a notice-role message in committed order.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let context = StartingRunModelContextDto::new(
        session_id,
        run_id,
        fixture_snapshot(),
        vec![
            MessageProjectionDto::new(
                row_id(),
                session_id,
                None,
                MessageKindDto::User,
                "read the file",
                None,
                None,
            )
            .expect("context message is valid"),
            MessageProjectionDto::new(
                row_id(),
                session_id,
                Some(run_id),
                MessageKindDto::Notice,
                intention_engine::INTERRUPT_NOTICE,
                None,
                None,
            )
            .expect("a committed notice row is coherent"),
            MessageProjectionDto::new(
                row_id(),
                session_id,
                Some(run_id),
                MessageKindDto::User,
                "continue",
                None,
                None,
            )
            .expect("context message is valid"),
        ],
    )
    .expect("fixture context is valid");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository
        .starting_context
        .lock()
        .expect("the fixture mutex is not poisoned") = Some(context);

    let scheduled = ApplicationService::new(&repository)
        .schedule_starting_run(session_id, run_id, RunCancellation::new())
        .expect("a committed notice schedules the next run");
    let messages = scheduled.request().messages();
    assert_eq!(messages.len(), 3, "every committed row replays in order");
    assert_eq!(messages[0].role(), ModelRoleDto::User);
    assert_eq!(messages[1].role(), ModelRoleDto::Notice);
    assert_eq!(messages[1].content(), intention_engine::INTERRUPT_NOTICE);
    assert_eq!(messages[2].role(), ModelRoleDto::User);
    assert_eq!(messages[2].content(), "continue");
}

#[test]
fn a_tool_call_carrying_a_nul_never_commits_evidence() {
    // Durable transcript rows are NUL-free, so a model call whose arguments
    // carry one cannot leave a call row: the invocation fails closed and
    // commits neither the call nor a result.
    let root = hello_tool_root("nul-arguments");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let error = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            invoke_read_input_in_workspace(&hello_workspace(&root), "hello.txt")
                .with_arguments_json("{\"path\":\"hel\0lo.txt\"}"),
            &RecordingCommitObserver::new(),
        )
        .expect_err("a call whose evidence cannot commit fails closed");
    assert_eq!(error.code(), "invalid_message");
    assert!(repository.committed_messages().is_empty());
    assert!(repository.committed_results().is_empty());
    let _ = fs::remove_dir_all(root);
}
