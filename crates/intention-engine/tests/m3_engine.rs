#![allow(
    clippy::expect_used,
    reason = "Focused application fixtures use expect to provide precise test failures."
)]

mod common;

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use common::{FakeRepository, RecordingCommitObserver, workspace_root};
use intention_config::ConfigSnapshotDto;
use intention_engine::reasoning::build_history_manifest;
use intention_engine::{
    ApplicationService, ModelRunCommitDto, ModelRunCommitObserver, RunCancellation,
    ToolInvocationRequestDto, ToolResultOutcomeDto,
};
use intention_proto::provider::{ReasoningHistoryManifestDto, ReasoningHistoryTransferDto};
use intention_proto::{
    CreateSessionCommandDto, InterruptRunCommandDto, MessageKindDto, MessageProjectionDto,
    PendingTurnProjectionDto, RemoveTurnCommandDto, RunModeDto, RunProjectionDto, RunStatusDto,
    SendUserTurnCommandDto, SessionProjectionDto, WorkspaceRootDto,
};
use intention_proto::{
    ErrorDto, IdempotencyKey, ProjectId, RunId, SessionId, TimestampDto, ToolCallId, TurnId,
    WorkspaceId,
};
use intention_storage::{StartingRunModelContextDto, ToolResultStatusDto};
use intention_test_support::fixture_snapshot;
use intention_tools::{BoundedText, ExecuteInput, GlobInput, ReadInput, ToolInput, WorkspaceRoot};

fn invoke_read_input(path: &str) -> ToolInvocationRequestDto {
    ToolInvocationRequestDto::new(
        WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy()).expect("workspace"),
        )
        .expect("workspace is valid"),
        SessionId::new(),
        RunId::new(),
        ToolCallId::new(),
        "read",
        ToolInput::Read(ReadInput {
            path: intention_proto::WorkspaceRelativePathDto::parse(path).expect("path"),
        }),
        fixture_time(),
    )
}

fn invoke_read_input_in_workspace(root: &WorkspaceRoot, path: &str) -> ToolInvocationRequestDto {
    ToolInvocationRequestDto::new(
        root.clone(),
        SessionId::new(),
        RunId::new(),
        ToolCallId::new(),
        "read",
        ToolInput::Read(ReadInput {
            path: intention_proto::WorkspaceRelativePathDto::parse(path).expect("path"),
        }),
        fixture_time(),
    )
}
fn fixture_time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid")
}

/// Returns one exact fixture history manifest over a single completed response.
fn fixture_history_manifest(session_id: SessionId, reasoning: &str) -> ReasoningHistoryManifestDto {
    let step = intention_storage::ReasoningHistorySourceStepDto::new(
        session_id,
        RunId::new(),
        3,
        Some(reasoning.to_owned()),
    )
    .expect("the fixture source step is valid");
    build_history_manifest(
        &[step],
        &ReasoningHistoryTransferDto::textual_history_v1("fixture-compatibility-v1")
            .expect("the fixture transfer contract is valid"),
        Some("fixture-compatibility-v1".to_owned()),
    )
    .expect("the fixture manifest builds")
}

/// Unwraps the content of one completed invocation outcome; any other outcome
/// is a fixture error.
fn completed_outcome(outcome: ToolResultOutcomeDto) -> String {
    match outcome {
        ToolResultOutcomeDto::Completed { content } => content,
        other => unreachable!("unexpected non-completed invocation: {other:?}"),
    }
}

fn projection(
    session_id: SessionId,
    active_run: Option<RunProjectionDto>,
    pending_turns: Vec<PendingTurnProjectionDto>,
) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
        active_run.as_ref().map(|run| run.config_revision_id()),
        active_run,
        pending_turns,
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

fn managed_read_input(path: &str) -> ToolInput {
    ToolInput::Read(ReadInput {
        path: intention_proto::WorkspaceRelativePathDto::parse(path).expect("path"),
    })
}

fn cancelled_execute_input() -> ToolInput {
    ToolInput::Execute(ExecuteInput {
        program: BoundedText::new("sh").expect("program"),
        args: vec![
            BoundedText::new("-c").expect("arg"),
            BoundedText::new("sleep 1").expect("arg"),
        ],
    })
}

/// An execute invocation that reports its own start and keeps running.
///
/// The child writes `sentinel.txt` into the invocation's working directory and
/// then waits, so a fixture can synchronize on the observed effect before it
/// cancels the call.
fn started_execute_input() -> ToolInput {
    ToolInput::Execute(ExecuteInput {
        program: BoundedText::new(if cfg!(windows) { "cmd" } else { "sh" }).expect("program"),
        args: if cfg!(windows) {
            vec![
                BoundedText::new("/C").expect("arg"),
                BoundedText::new("echo started> sentinel.txt & ping -n 2 127.0.0.1").expect("arg"),
            ]
        } else {
            vec![
                BoundedText::new("-c").expect("arg"),
                BoundedText::new("printf x > sentinel.txt; sleep 2").expect("arg"),
            ]
        },
    })
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
                managed_read_input("hello.txt"),
                fixture_time(),
            ),
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
    assert_eq!(messages[0].tool_id(), Some("read"));
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
                ToolInput::Glob(GlobInput {
                    pattern: BoundedText::new("*.none").expect("pattern"),
                }),
                fixture_time(),
            ),
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
        .unwrap_or_else(PoisonError::into_inner) =
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
fn committed_rows_redact_absolute_workspace_root_and_os_error_text() {
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
    let ToolResultOutcomeDto::Failed { error } = outcome else {
        unreachable!("a missing file fails the call");
    };
    let rendered = format!(
        "{error:?} {:?} {:?}",
        repository.committed_messages(),
        repository.committed_results()
    );
    assert!(!rendered.contains(&root.to_string_lossy().to_string()));
    assert!(!rendered.contains("No such file or directory"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_tool_rejects_unknown_or_mismatched_id_before_effects() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let error = ApplicationService::new(&repository)
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
                "unknown",
                managed_read_input("missing"),
                fixture_time(),
            ),
            &RecordingCommitObserver::new(),
        )
        .expect_err("mismatched tool id is rejected");
    assert_eq!(error.code(), "tool_id_mismatch");
    assert!(repository.committed_messages().is_empty());
    assert!(repository.committed_results().is_empty());
}

#[test]
fn send_user_turn_parameters_and_schedule_validation_cover_the_durable_selection() {
    let command = SendUserTurnCommandDto::new(SessionId::new(), IdempotencyKey::new(), "hello")
        .expect("command is valid");
    let proposed_run_id = RunId::new();
    let config = fixture_snapshot();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable(
        "turn_admission_unavailable",
        "the durable session refused the turn",
    )));
    let selection = repository.selection();
    let history = fixture_history_manifest(command.session_id(), "why");
    let error = ApplicationService::new(&repository)
        .send_user_turn(
            command,
            proposed_run_id,
            config.clone(),
            selection.clone(),
            Some(history.clone()),
            fixture_time(),
        )
        .expect_err("admission failure is propagated");
    assert_eq!(error.code(), "turn_admission_unavailable");
    let inputs = repository
        .accepted_inputs
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(inputs[0].proposed_run_id(), proposed_run_id);
    assert_eq!(inputs[0].config_snapshot(), &config);
    assert_eq!(
        inputs[0].selection(),
        &selection,
        "the accepted turn carries the exact resolved selection"
    );
    assert_eq!(
        inputs[0].reasoning_history(),
        Some(&history),
        "the accepted turn carries the committed typed history manifest"
    );
    assert_eq!(inputs[0].occurred_at(), fixture_time());
    drop(inputs);

    // A durable context for another run cannot become the requested schedule.
    let session_id = SessionId::new();
    let requested_run = RunId::new();
    *repository
        .starting_context
        .lock()
        .unwrap_or_else(PoisonError::into_inner) =
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
        .unwrap_or_else(PoisonError::into_inner) =
        Some(starting_context(session_id, matching_run, &config));
    let scheduled = ApplicationService::new(&repository)
        .schedule_starting_run(session_id, matching_run, RunCancellation::new())
        .expect("matching schedule is accepted");
    assert_eq!(scheduled.session_id(), session_id);
    assert_eq!(scheduled.run_id(), matching_run);
    assert_eq!(
        scheduled.selection(),
        &selection,
        "the schedule carries the run's persisted exact selection"
    );
    let request = scheduled.request();
    assert_eq!(request.run_id(), matching_run);
    assert_eq!(request.model(), "fixture");
    assert_eq!(request.messages().len(), 3);
    assert!(!request.tools().is_empty());
}

#[test]
fn interrupt_run_workflow_maps_durable_results() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot();
    let expected = RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        RunStatusDto::Starting,
        config.revision_id(),
    );
    let state = projection(session_id, Some(expected.clone()), Vec::new());
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable(
        "fixture_unused",
        "accept is not used by this fixture",
    )));
    *repository
        .loaded_projection
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(state);
    let application = ApplicationService::new(&repository);

    let interrupted = application
        .interrupt_run(InterruptRunCommandDto::new(session_id, run_id))
        .expect("interrupt maps");
    assert_eq!(
        interrupted, expected,
        "the engine returns the exact active run projection"
    );
}

#[test]
fn interrupt_run_rejects_a_run_that_is_not_active() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable(
        "fixture_unused",
        "accept is not used by this fixture",
    )));
    *repository
        .loaded_projection
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(projection(session_id, None, Vec::new()));
    assert_eq!(
        ApplicationService::new(&repository)
            .interrupt_run(InterruptRunCommandDto::new(session_id, RunId::new()))
            .expect_err("an inactive session has no run to interrupt")
            .code(),
        "active_run_not_found"
    );
}

#[test]
fn create_and_remove_workflows_map_committed_results() {
    let session_id = SessionId::new();
    let pending_turn = TurnId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository
        .created
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(projection(session_id, None, Vec::new()));
    *repository
        .removed
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(
        PendingTurnProjectionDto::new(session_id, pending_turn, "later")
            .expect("fixture pending turn is valid"),
    );
    let application = ApplicationService::new(&repository);
    let create = CreateSessionCommandDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
    );

    let created = application
        .create_session(create, fixture_time())
        .expect("create maps");
    assert_eq!(created.session_id(), session_id);
    let removed = application
        .remove_turn(
            RemoveTurnCommandDto::new(session_id, pending_turn),
            fixture_time(),
        )
        .expect("removal maps");
    assert_eq!(removed.session_id(), session_id);
    assert_eq!(removed.turn_id(), pending_turn);
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
            .unwrap_or_else(PoisonError::into_inner) = vec![failing_call];
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
            .unwrap_or_else(PoisonError::into_inner)
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
                .unwrap_or_else(PoisonError::into_inner),
            vec![0, 1],
            "the terminal row publishes after its own durable commit"
        );
    }
}

impl ModelRunCommitObserver for TerminalOrderingProbe<'_> {
    fn observe_model_run_commit(&self, commit: &ModelRunCommitDto) {
        let ModelRunCommitDto::Content(message) = commit else {
            return;
        };
        self.results_at_publish
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(self.committed_result_rows.load(Ordering::SeqCst));
        self.publications
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
    assert_eq!(answering.tool_id(), Some(tool_id));
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
                managed_read_input("hello.txt"),
                fixture_time(),
            ),
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
            .unwrap_or_else(PoisonError::into_inner),
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
                managed_read_input("missing.txt"),
                fixture_time(),
            ),
            &publisher,
        )
        .expect("a missing file is a durable safe failure");
    let ToolResultOutcomeDto::Failed { error } = outcome else {
        unreachable!("a missing file fails the call");
    };
    assert_eq!(error.code(), "tool_read_failed");
    assert_single_terminal_result(
        &repository,
        session_id,
        run_id,
        call_id,
        "read",
        ToolResultStatusDto::Failed,
    );
    // The terminal Failed commit classifies the safe error code.
    let failed = &repository.committed_results()[0];
    assert_eq!(failed.content(), "tool_read_failed");
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
                cancelled_execute_input(),
                fixture_time(),
            )
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
    // exact stopped notice and never records a completed row.
    assert_eq!(
        repository.committed_results()[0].content(),
        "[The tool call was stopped before a final result.]"
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
                started_execute_input(),
                fixture_time(),
            )
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
    // The terminal Partial commit keeps the captured output and never records
    // a completed row.
    let partial = &repository.committed_results()[0];
    assert!(partial.content().contains("stdout:"));
    assert!(partial.content().contains(
        "[The tool call was stopped before a final result; the output above is partial.]"
    ));
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
            invoke_read_input_in_workspace(&hello_workspace(&root), "hello.txt")
                .with_arguments_json(r#"{"path":"hello.txt"}"#),
            &RecordingCommitObserver::new(),
        )
        .expect("read succeeds");
    let messages = repository.committed_messages();
    assert_eq!(messages[0].kind(), MessageKindDto::ToolCall);
    assert_eq!(messages[0].text(), r#"{"path":"hello.txt"}"#);
    assert_eq!(messages[0].tool_id(), Some("read"));

    // A caller without the model's arguments still commits a well-formed row.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            ToolInvocationRequestDto::new(
                hello_workspace(&root),
                SessionId::new(),
                RunId::new(),
                call,
                "read",
                managed_read_input("hello.txt"),
                fixture_time(),
            ),
            &RecordingCommitObserver::new(),
        )
        .expect("read succeeds");
    assert_eq!(repository.committed_messages()[0].text(), "{}");
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
                session_id,
                None,
                MessageKindDto::User,
                "first",
                None,
                None,
                None,
            )
            .expect("context message is valid"),
            MessageProjectionDto::new(
                session_id,
                None,
                MessageKindDto::Assistant,
                "answer",
                None,
                None,
                None,
            )
            .expect("context message is valid"),
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::User,
                "latest",
                None,
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
        .unwrap_or_else(PoisonError::into_inner) = vec![2];
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
