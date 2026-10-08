#![allow(
    clippy::expect_used,
    reason = "Focused application fixtures use expect to provide precise test failures."
)]

mod common;

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use common::{FakeRepository, RecordingDispatchPort, workspace_root};
use intention_config::ConfigSnapshotDto;
use intention_domain::ToolResultStatusDto;
use intention_engine::{
    ApplicationService, LocalToolInvocationOutcomeDto, ModelCancellationSignal, ModelRunCommitDto,
    ModelRunCommitObserver, ToolInvocationRequestDto, WorkspaceBoundaryPort,
};
use intention_engine::{ModelMessageDto, ModelRoleDto};
use intention_proto::{
    CreateSessionCommandDto, InterruptRunCommandDto, MessageKindDto, MessageProjectionDto,
    PendingTurnProjectionDto, RemoveTurnCommandDto, RunModeDto, RunProjectionDto, RunStatusDto,
    SendUserTurnCommandDto, SessionProjectionDto, WorkspaceRootDto,
};
use intention_proto::{
    DtoResult, ErrorDto, IdempotencyKey, ProjectId, RunId, SessionId, TimestampDto, ToolCallId,
    TurnId, WorkspaceId,
};
use intention_proto::{ProtocolAcceptedResultDto, SendUserTurnOutcomeDto};
use intention_storage::{AcceptedTurnOutcomeDto, StartingRunModelContextDto};
use intention_test_support::fixture_snapshot;
use intention_tools::{
    BoundedText, CancellationSignal, ExecuteInput, ReadInput, TextResult, ToolInput, ToolResult,
    WorkspaceRoot,
};

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

/// Unwraps one completed invocation outcome; a partial outcome is a fixture error.
fn completed_outcome(outcome: LocalToolInvocationOutcomeDto) -> ToolResult {
    match outcome {
        LocalToolInvocationOutcomeDto::Completed(result) => result,
        LocalToolInvocationOutcomeDto::Partial { .. } => {
            unreachable!("unexpected partial invocation in a completed fixture")
        }
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
        active_run.map(RunProjectionDto::config_revision_id),
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

fn hello_read_result() -> ToolResult {
    ToolResult::Read(TextResult {
        text: BoundedText::new("hello").expect("text"),
        truncated: false,
    })
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
        .invoke_local_tool(ToolInvocationRequestDto::new(
            hello_workspace(&root),
            session,
            run,
            call,
            "read",
            managed_read_input("hello.txt"),
            fixture_time(),
        ))
        .expect("tool succeeds");
    let result = completed_outcome(result);
    assert_eq!(result, hello_read_result());

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
fn local_tool_rejects_storage_before_execution() {
    let root = hello_tool_root("storage-reject");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository.commit_error.borrow_mut() =
        Some(ErrorDto::unavailable("storage_down", "storage unavailable"));
    let error = ApplicationService::new(&repository)
        .invoke_local_tool(invoke_read_input_in_workspace(
            &hello_workspace(&root),
            "hello.txt",
        ))
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
    let error = ApplicationService::new(&repository)
        .invoke_local_tool(invoke_read_input_in_workspace(
            &WorkspaceRoot::resolve(
                &WorkspaceRootDto::parse(root.to_string_lossy()).expect("workspace"),
            )
            .expect("resolved workspace"),
            "missing",
        ))
        .expect_err("read must fail");
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
        .invoke_local_tool(ToolInvocationRequestDto::new(
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
        ))
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
    let error = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            command,
            proposed_run_id,
            config.clone(),
            fixture_time(),
            &RecordingDispatchPort::default(),
        )
        .expect_err("admission failure is propagated");
    assert_eq!(error.code(), "turn_admission_unavailable");
    let inputs = repository.accepted_inputs.borrow();
    assert_eq!(inputs[0].proposed_run_id(), proposed_run_id);
    assert_eq!(inputs[0].config_snapshot().resolved(), config.resolved());
    assert_eq!(inputs[0].occurred_at(), fixture_time());
    drop(inputs);

    // A durable context for another run cannot become the requested schedule.
    let session_id = SessionId::new();
    let requested_run = RunId::new();
    *repository.starting_context.borrow_mut() =
        Some(starting_context(session_id, RunId::new(), &config));
    let error = ApplicationService::new(&repository)
        .schedule_starting_run(session_id, requested_run, ModelCancellationSignal::new())
        .expect_err("mismatched schedule is rejected");
    assert_eq!(error.code(), "invalid_model_run_schedule");

    // A context that names the requested run reconstructs the durable
    // selection and its message list into the execution input.
    let matching_run = RunId::new();
    *repository.starting_context.borrow_mut() =
        Some(starting_context(session_id, matching_run, &config));
    let scheduled = ApplicationService::new(&repository)
        .schedule_starting_run(session_id, matching_run, ModelCancellationSignal::new())
        .expect("matching schedule is accepted");
    assert_eq!(scheduled.session_id(), session_id);
    assert_eq!(scheduled.run_id(), matching_run);
    assert_eq!(scheduled.request().messages().len(), 3);
    assert_eq!(
        scheduled.safe_config().resolved().provider().model(),
        "fixture"
    );
}

#[test]
fn interrupt_run_workflow_maps_durable_results() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot();
    let state = projection(
        session_id,
        Some(RunProjectionDto::new(
            session_id,
            run_id,
            TurnId::new(),
            RunStatusDto::Starting,
            config.revision_id(),
        )),
        Vec::new(),
    );
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable(
        "fixture_unused",
        "accept is not used by this fixture",
    )));
    *repository.loaded_projection.borrow_mut() = Some(state);
    let application = ApplicationService::new(&repository);

    let interrupted = application
        .interrupt_run(InterruptRunCommandDto::new(session_id, run_id))
        .expect("interrupt maps");
    assert!(matches!(
        interrupted,
        ProtocolAcceptedResultDto::InterruptRun(value)
            if value.session_id() == session_id && value.run_id() == run_id
    ));
}

#[test]
fn interrupt_run_rejects_a_run_that_is_not_active() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable(
        "fixture_unused",
        "accept is not used by this fixture",
    )));
    *repository.loaded_projection.borrow_mut() = Some(projection(session_id, None, Vec::new()));
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
    *repository.created.borrow_mut() = Some(projection(session_id, None, Vec::new()));
    *repository.removed.borrow_mut() = Some(
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
    assert!(matches!(
        created,
        ProtocolAcceptedResultDto::CreateSession(value) if value.session_id() == session_id
    ));
    let removed = application
        .remove_turn(
            RemoveTurnCommandDto::new(session_id, pending_turn),
            fixture_time(),
        )
        .expect("removal maps");
    assert!(matches!(
        removed,
        ProtocolAcceptedResultDto::RemoveTurn(value)
            if value.session_id() == session_id && value.turn_id() == pending_turn
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
    assert!(matches!(
        outcome,
        LocalToolInvocationOutcomeDto::Completed(ToolResult::Read(_))
    ));

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
        *repository.commit_failures.borrow_mut() = vec![failing_call];
        let error = ApplicationService::new(&repository)
            .invoke_local_tool(invoke_read_input_in_workspace(
                &hello_workspace(&root),
                path,
            ))
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
    let result = completed_outcome(result);
    assert_eq!(result, hello_read_result());
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
    let error = ApplicationService::new(&repository)
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
        .expect_err("missing file fails");
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
            .with_cancellation(CancellationSignal::cancelled()),
            &publisher,
        )
        .expect("a pre-start cancellation is a partial outcome");
    assert_eq!(
        outcome,
        LocalToolInvocationOutcomeDto::Partial {
            stopped: true,
            result: None,
        }
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
    let signal = CancellationSignal::new();
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
    let LocalToolInvocationOutcomeDto::Partial { stopped, result } = outcome else {
        unreachable!("cancellation must interrupt the invocation");
    };
    assert!(stopped);
    assert!(matches!(result, Some(ToolResult::Execute(_))));
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
        .invoke_local_tool(
            invoke_read_input_in_workspace(&hello_workspace(&root), "hello.txt")
                .with_arguments_json(r#"{"path":"hello.txt"}"#),
        )
        .expect("read succeeds");
    let messages = repository.committed_messages();
    assert_eq!(messages[0].kind(), MessageKindDto::ToolCall);
    assert_eq!(messages[0].text(), r#"{"path":"hello.txt"}"#);
    assert_eq!(messages[0].tool_id(), Some("read"));

    // A caller without the model's arguments still commits a well-formed row.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    ApplicationService::new(&repository)
        .invoke_local_tool(ToolInvocationRequestDto::new(
            hello_workspace(&root),
            SessionId::new(),
            RunId::new(),
            call,
            "read",
            managed_read_input("hello.txt"),
            fixture_time(),
        ))
        .expect("read succeeds");
    assert_eq!(repository.committed_messages()[0].text(), "{}");
    let _ = fs::remove_dir_all(root);
}

struct RejectingWorkspaceBoundary;

impl WorkspaceBoundaryPort for RejectingWorkspaceBoundary {
    fn resolve(&self, _: &WorkspaceRoot) -> DtoResult<WorkspaceRoot> {
        Err(ErrorDto::unavailable(
            "workspace_boundary_unavailable",
            "workspace boundary refused the invocation",
        ))
    }
}

fn send_command(session_id: SessionId) -> SendUserTurnCommandDto {
    SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "latest")
        .expect("fixture command is valid")
}

const fn starting_run(
    session_id: SessionId,
    run_id: RunId,
    turn_id: TurnId,
    config: &ConfigSnapshotDto,
) -> RunProjectionDto {
    RunProjectionDto::new(
        session_id,
        run_id,
        turn_id,
        RunStatusDto::Starting,
        config.revision_id(),
    )
}

fn latest_message(session_id: SessionId, run_id: RunId) -> MessageProjectionDto {
    MessageProjectionDto::new(
        session_id,
        Some(run_id),
        MessageKindDto::User,
        "latest",
        None,
        None,
        None,
    )
    .expect("fixture message is valid")
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
fn send_user_turn_and_schedule_returns_queued_acceptance_without_dispatching() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let repository = FakeRepository::with_accepted(Ok(AcceptedTurnOutcomeDto::Pending(
        PendingTurnProjectionDto::new(session_id, turn_id, "latest")
            .expect("fixture pending turn is valid"),
    )));
    let dispatch = RecordingDispatchPort::default();
    let accepted = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            send_command(session_id),
            RunId::new(),
            fixture_snapshot(),
            fixture_time(),
            &dispatch,
        )
        .expect("queued acceptance is returned unchanged");
    assert!(matches!(
        accepted,
        ProtocolAcceptedResultDto::SendUserTurn(value)
            if value.session_id() == session_id
                && value.turn_id() == turn_id
                && value.outcome() == SendUserTurnOutcomeDto::Pending
    ));
    assert_eq!(repository.accepted_inputs.borrow().len(), 1);
    assert!(dispatch.inputs.borrow().is_empty());
    assert!(repository.finishes.borrow().is_empty());
}

#[test]
fn send_user_turn_and_schedule_dispatches_the_committed_starting_run() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot();
    let run = starting_run(session_id, run_id, turn_id, &config);
    let repository = FakeRepository::with_accepted(Ok(AcceptedTurnOutcomeDto::Started {
        run,
        message: latest_message(session_id, run_id),
    }));
    *repository.starting_context.borrow_mut() = Some(starting_context(session_id, run_id, &config));
    let dispatch = RecordingDispatchPort::default();
    let accepted = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            send_command(session_id),
            run_id,
            config.clone(),
            fixture_time(),
            &dispatch,
        )
        .expect("started acceptance is returned unchanged");
    assert!(matches!(
        accepted,
        ProtocolAcceptedResultDto::SendUserTurn(value)
            if value.outcome()
                == SendUserTurnOutcomeDto::Started {
                    run_id,
                    config_revision_id: config.revision_id(),
                }
    ));
    let inputs = dispatch.inputs.borrow();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].session_id(), session_id);
    assert_eq!(inputs[0].run_id(), run_id);
    assert_eq!(inputs[0].safe_config(), &config);
    let request = inputs[0].request();
    assert_eq!(request.run_id(), run_id);
    assert_eq!(request.model(), "fixture");
    assert_eq!(
        request.messages(),
        [
            ModelMessageDto::new(ModelRoleDto::User, "first").expect("message is valid"),
            ModelMessageDto::new(ModelRoleDto::Assistant, "answer").expect("message is valid"),
            ModelMessageDto::new(ModelRoleDto::User, "latest").expect("message is valid"),
        ]
        .as_slice()
    );
    assert!(!request.tools().is_empty());
    assert!(repository.finishes.borrow().is_empty());
}

#[test]
fn send_user_turn_and_schedule_preserves_acceptance_when_context_is_unusable() {
    for mismatched in [false, true] {
        let session_id = SessionId::new();
        let turn_id = TurnId::new();
        let run_id = RunId::new();
        let config = fixture_snapshot();
        let run = starting_run(session_id, run_id, turn_id, &config);
        let repository = FakeRepository::with_accepted(Ok(AcceptedTurnOutcomeDto::Started {
            run,
            message: latest_message(session_id, run_id),
        }));
        *repository.run.borrow_mut() = Some(run);
        if mismatched {
            *repository.starting_context.borrow_mut() =
                Some(starting_context(SessionId::new(), RunId::new(), &config));
        }
        let dispatch = RecordingDispatchPort::default();
        let accepted = ApplicationService::new(&repository)
            .send_user_turn_and_schedule(
                send_command(session_id),
                run_id,
                config,
                fixture_time(),
                &dispatch,
            )
            .expect("post-commit context failure preserves the acceptance");
        assert!(matches!(
            accepted,
            ProtocolAcceptedResultDto::SendUserTurn(_)
        ));
        assert!(dispatch.inputs.borrow().is_empty());
        // The exact starting run is terminalized once so the committed
        // acceptance never leaves an unschedulable live run behind.
        let finishes = repository.finishes.borrow();
        assert_eq!(finishes.len(), 1);
        assert_eq!(finishes[0].session_id(), session_id);
        assert_eq!(finishes[0].run_id(), run_id);
        assert_eq!(finishes[0].status(), RunStatusDto::Failed);
        assert_eq!(finishes[0].error_code(), Some("model_context_unavailable"));
    }
}

#[test]
fn send_user_turn_and_schedule_preserves_acceptance_when_dispatch_fails() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot();
    let run = starting_run(session_id, run_id, turn_id, &config);
    let repository = FakeRepository::with_accepted(Ok(AcceptedTurnOutcomeDto::Started {
        run,
        message: latest_message(session_id, run_id),
    }));
    *repository.starting_context.borrow_mut() = Some(starting_context(session_id, run_id, &config));
    *repository.run.borrow_mut() = Some(run);
    let dispatch = RecordingDispatchPort::default();
    *dispatch.failure.borrow_mut() = Some(ErrorDto::unavailable(
        "dispatch_unavailable",
        "the daemon refused the scheduled run",
    ));
    let accepted = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            send_command(session_id),
            run_id,
            config,
            fixture_time(),
            &dispatch,
        )
        .expect("post-commit dispatch failure preserves the acceptance");
    assert!(matches!(
        accepted,
        ProtocolAcceptedResultDto::SendUserTurn(_)
    ));
    assert_eq!(dispatch.inputs.borrow().len(), 1);
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Failed);
    assert_eq!(
        finishes[0].error_code(),
        Some("model_scheduling_unavailable")
    );
}

#[test]
fn schedule_starting_run_maps_durable_context_into_the_dispatch_dto() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository.starting_context.borrow_mut() = Some(starting_context(session_id, run_id, &config));
    let scheduled = ApplicationService::new(&repository)
        .schedule_starting_run(session_id, run_id, ModelCancellationSignal::new())
        .expect("durable starting context schedules");
    assert_eq!(scheduled.session_id(), session_id);
    assert_eq!(scheduled.run_id(), run_id);
    assert_eq!(scheduled.safe_config(), &config);
    let request = scheduled.request();
    assert_eq!(request.run_id(), run_id);
    assert_eq!(request.model(), "fixture");
    assert_eq!(request.messages().len(), 3);
    assert!(!request.tools().is_empty());
}

#[test]
fn workspace_boundary_failure_is_durably_rejected_before_execution() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let error = ApplicationService::new(&repository)
        .with_workspace_boundary(RejectingWorkspaceBoundary)
        .invoke_local_tool(invoke_read_input("missing"))
        .expect_err("workspace boundary failure is propagated");
    assert_eq!(error.code(), "workspace_boundary_unavailable");
    let messages = repository.committed_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].kind(), MessageKindDto::ToolResult);
    assert_eq!(messages[1].text(), "workspace_boundary_unavailable");
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Failed);
}

#[test]
fn terminal_commit_failure_propagates_from_the_tool_error_path() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository.commit_failures.borrow_mut() = vec![2];
    let error = ApplicationService::new(&repository)
        .invoke_local_tool(invoke_read_input("missing"))
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
        .invoke_local_tool(invoke_read_input_in_workspace(
            &hello_workspace(&root),
            "control.txt",
        ))
        .expect("control characters are readable text");
    let result = completed_outcome(result);
    assert!(matches!(result, ToolResult::Read(_)));
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Completed);
    assert_eq!(results[0].content(), "\u{8}\t\u{c}\r");
    assert_eq!(repository.committed_messages()[1].text(), "\u{8}\t\u{c}\r");
    let _ = fs::remove_dir_all(root);
}
