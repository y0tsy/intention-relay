#![allow(
    clippy::expect_used,
    reason = "Focused application fixtures use expect to provide precise test failures."
)]

use std::cell::RefCell;
use std::fs;
use std::sync::{Arc, Mutex};

use intention_application::{
    ApplicationService, CreateSessionWorkflowInputDto, HookObservationPort,
    InvokeLocalToolInputDto, LocalToolInvocationOutcomeDto, ModelRunDispatchPort,
    ScheduleModelRunDto, SendUserTurnWorkflowInputDto, ToolResultPublicationPort,
    WorkspaceBoundaryPort,
};
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    CreateSessionCommandDto, GetSessionSnapshotQueryDto, InterruptRunCommandDto, MessageKindDto,
    MessageProjectionDto, PendingTurnProjectionDto, RemoveTurnCommandDto, RunModeDto,
    RunProjectionDto, RunStatusDto, SendUserTurnCommandDto, SessionProjectionDto,
    ToolResultStatusDto, WorkspaceRootDto,
};
use intention_hooks::{
    FailurePolicy, Hook, HookObservability, Outcome as HookOutcome, Phase, PhaseContext, Registry,
};
use intention_protocol::{
    CURRENT_DTO_SCHEMA_VERSION, ProtocolAcceptedResultDto, SendUserTurnOutcomeDto,
};
use intention_runtime::{ModelMessageDto, ModelRequestDto, ModelRoleDto};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, AppendMessageInputDto,
    ConsumePendingUserTurnsInputDto, CreateSessionInputDto, FinishRunInputDto,
    RecoverUnfinishedRunsInputDto, RemoveTurnInputDto, StartingRunModelContextDto,
    StorageRepositoryDto, ToolResultEvidenceDto, TransitionRunInputDto, WriteToolResultInputDto,
};
use intention_tools::{
    BoundedText, CancellationSignal, ExecuteInput, ReadInput, TextResult, ToolInput, ToolResult,
};
use intention_types::{
    ConfigRevisionId, DtoResult, ErrorDto, IdempotencyKey, ProjectId, RunId, SchemaVersionDto,
    SessionId, TimestampDto, ToolCallId, TurnId, WorkspaceId,
};
use intention_workspace::WorkspaceRoot;

struct RejectHook;
impl Hook for RejectHook {
    fn id(&self) -> &'static str {
        "reject-local-tool"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::BeforeToolExecution]
    }
    fn priority(&self) -> u32 {
        0
    }
    fn run(&self, _: &PhaseContext) -> DtoResult<HookOutcome> {
        Ok(HookOutcome::Reject(ErrorDto::validation(
            "blocked_by_hook",
            "blocked",
        )))
    }
}

struct DispatchErrorHook {
    phase: Phase,
}

fn invoke_read_input(path: &str) -> InvokeLocalToolInputDto {
    InvokeLocalToolInputDto::new(
        WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy()).expect("workspace"),
        )
        .expect("workspace is valid"),
        SessionId::new(),
        RunId::new(),
        ToolCallId::new(),
        "read",
        ToolInput::Read(ReadInput {
            path: intention_types::WorkspaceRelativePathDto::parse(path).expect("path"),
        }),
        fixture_time(),
    )
}

fn invoke_read_input_in_workspace(root: &WorkspaceRoot, path: &str) -> InvokeLocalToolInputDto {
    InvokeLocalToolInputDto::new(
        root.clone(),
        SessionId::new(),
        RunId::new(),
        ToolCallId::new(),
        "read",
        ToolInput::Read(ReadInput {
            path: intention_types::WorkspaceRelativePathDto::parse(path).expect("path"),
        }),
        fixture_time(),
    )
}

impl Hook for DispatchErrorHook {
    fn id(&self) -> &'static str {
        "dispatch-error"
    }
    fn phases(&self) -> &'static [Phase] {
        Box::leak(vec![self.phase].into_boxed_slice())
    }
    fn priority(&self) -> u32 {
        0
    }
    fn run(&self, _: &PhaseContext) -> DtoResult<HookOutcome> {
        Err(ErrorDto::unavailable("hook_failed", "hook failed"))
    }
}

struct PhaseOutcomeHook {
    phase: Phase,
    outcome: HookOutcome,
    id: &'static str,
}
impl Hook for PhaseOutcomeHook {
    fn id(&self) -> &'static str {
        self.id
    }
    fn phases(&self) -> &'static [Phase] {
        Box::leak(vec![self.phase].into_boxed_slice())
    }
    fn priority(&self) -> u32 {
        0
    }
    fn run(&self, _: &PhaseContext) -> DtoResult<HookOutcome> {
        Ok(self.outcome.clone())
    }
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

fn snapshot() -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-application-test.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture path is absolute"),
    );
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-secret\"",
        source,
    ))
    .expect("fixture config resolves");
    ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        fixture_time(),
        resolved,
    )
    .expect("fixture snapshot is valid")
}

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-application-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
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

/// Fixture repository over the current-state transactional repository contract.
///
/// It records every committed transcript row and tool result, serves the
/// command/query fixtures, and can fail selected commit ordinals so ordering
/// guarantees stay observable.
struct FakeRepository {
    created: RefCell<Option<SessionProjectionDto>>,
    accepted: RefCell<DtoResult<AcceptedTurnOutcomeDto>>,
    accepted_inputs: RefCell<Vec<AcceptUserTurnInputDto>>,
    removed: RefCell<Option<PendingTurnProjectionDto>>,
    loaded_projection: RefCell<Option<SessionProjectionDto>>,
    recent_messages: RefCell<Vec<MessageProjectionDto>>,
    starting_context: RefCell<Option<StartingRunModelContextDto>>,
    run: RefCell<Option<RunProjectionDto>>,
    finishes: RefCell<Vec<FinishRunInputDto>>,
    messages: RefCell<Vec<MessageProjectionDto>>,
    tool_results: RefCell<Vec<ToolResultEvidenceDto>>,
    commit_calls: RefCell<usize>,
    commit_failures: RefCell<Vec<usize>>,
    commit_error: RefCell<Option<ErrorDto>>,
}

impl FakeRepository {
    const fn with_accepted(accepted: DtoResult<AcceptedTurnOutcomeDto>) -> Self {
        Self {
            created: RefCell::new(None),
            accepted: RefCell::new(accepted),
            accepted_inputs: RefCell::new(Vec::new()),
            removed: RefCell::new(None),
            loaded_projection: RefCell::new(None),
            recent_messages: RefCell::new(Vec::new()),
            starting_context: RefCell::new(None),
            run: RefCell::new(None),
            finishes: RefCell::new(Vec::new()),
            messages: RefCell::new(Vec::new()),
            tool_results: RefCell::new(Vec::new()),
            commit_calls: RefCell::new(0),
            commit_failures: RefCell::new(Vec::new()),
            commit_error: RefCell::new(None),
        }
    }

    /// Returns the next one-based commit ordinal, or the selected injected failure.
    fn next_commit(&self) -> DtoResult<usize> {
        if let Some(error) = self.commit_error.borrow().clone() {
            return Err(error);
        }
        let ordinal = {
            let mut calls = self.commit_calls.borrow_mut();
            *calls += 1;
            *calls
        };
        if self.commit_failures.borrow().contains(&ordinal) {
            return Err(ErrorDto::unavailable(
                "append_unavailable",
                "append refused at the selected call",
            ));
        }
        Ok(ordinal)
    }

    fn committed_messages(&self) -> Vec<MessageProjectionDto> {
        self.messages.borrow().clone()
    }

    fn committed_results(&self) -> Vec<ToolResultEvidenceDto> {
        self.tool_results.borrow().clone()
    }

    fn completed_result_count(&self) -> usize {
        self.committed_results()
            .iter()
            .filter(|row| row.status() == ToolResultStatusDto::Completed)
            .count()
    }
}

impl StorageRepositoryDto for FakeRepository {
    fn create_session(&self, _input: CreateSessionInputDto) -> DtoResult<SessionProjectionDto> {
        self.created.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
        })
    }

    fn accept_user_turn(&self, input: AcceptUserTurnInputDto) -> DtoResult<AcceptedTurnOutcomeDto> {
        self.accepted_inputs.borrow_mut().push(input);
        self.accepted.borrow().clone()
    }

    fn remove_turn(&self, _input: RemoveTurnInputDto) -> DtoResult<PendingTurnProjectionDto> {
        self.removed.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
        })
    }

    fn consume_pending_user_turns(
        &self,
        _input: ConsumePendingUserTurnsInputDto,
    ) -> DtoResult<Vec<MessageProjectionDto>> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "pending joins are not used by this fixture",
        ))
    }

    fn transition_run(&self, _input: TransitionRunInputDto) -> DtoResult<RunProjectionDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "transitions are not used by this fixture",
        ))
    }

    fn finish_run(&self, input: FinishRunInputDto) -> DtoResult<RunProjectionDto> {
        let run = (*self.run.borrow()).ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture run missing")
        })?;
        let status = input.status();
        self.finishes.borrow_mut().push(input);
        Ok(RunProjectionDto::new(
            run.session_id(),
            run.run_id(),
            run.turn_id(),
            status,
            run.config_revision_id(),
        ))
    }

    fn append_message(&self, input: AppendMessageInputDto) -> DtoResult<MessageProjectionDto> {
        self.next_commit()?;
        let message = input.message().clone();
        self.messages.borrow_mut().push(message.clone());
        Ok(message)
    }

    fn write_tool_result(
        &self,
        input: WriteToolResultInputDto,
    ) -> DtoResult<ToolResultEvidenceDto> {
        self.next_commit()?;
        let evidence = input.evidence().clone();
        self.tool_results.borrow_mut().push(evidence.clone());
        self.messages.borrow_mut().push(input.message().clone());
        Ok(evidence)
    }

    fn load_tool_result(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _call_id: ToolCallId,
    ) -> DtoResult<ToolResultEvidenceDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "tool result loads are not used by this fixture",
        ))
    }

    fn load_run_config_snapshot(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<ConfigSnapshotDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "run configuration loads are not used by this fixture",
        ))
    }

    fn load_starting_run_model_context(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<StartingRunModelContextDto> {
        self.starting_context.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable(
                "run_model_context_unavailable",
                "the durable run model context is unavailable",
            )
        })
    }

    fn load_run_projection(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<RunProjectionDto> {
        (*self.run.borrow())
            .ok_or_else(|| ErrorDto::unavailable("fixture_missing_result", "fixture run missing"))
    }

    fn load_session_projection(&self, _session_id: SessionId) -> DtoResult<SessionProjectionDto> {
        self.loaded_projection.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
        })
    }

    fn load_recent_messages(
        &self,
        _session_id: SessionId,
        _limit: u32,
    ) -> DtoResult<Vec<MessageProjectionDto>> {
        Ok(self.recent_messages.borrow().clone())
    }

    fn load_run_messages(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _limit: u32,
    ) -> DtoResult<Vec<MessageProjectionDto>> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "run message loads are not used by this fixture",
        ))
    }

    fn recover_unfinished_runs(
        &self,
        _input: RecoverUnfinishedRunsInputDto,
    ) -> DtoResult<Vec<RunProjectionDto>> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "recovery is not used by this fixture",
        ))
    }

    fn accept_configuration_revision(&self, _snapshot: ConfigSnapshotDto) -> DtoResult<()> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "configuration acceptance is not used by this fixture",
        ))
    }
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
        path: intention_types::WorkspaceRelativePathDto::parse(path).expect("path"),
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

fn sleeping_execute_input() -> ToolInput {
    ToolInput::Execute(ExecuteInput {
        program: BoundedText::new(if cfg!(windows) { "ping" } else { "sh" }).expect("program"),
        args: vec![
            BoundedText::new(if cfg!(windows) { "-n" } else { "-c" }).expect("arg"),
            BoundedText::new(if cfg!(windows) { "2" } else { "sleep 1" }).expect("arg"),
            #[cfg(windows)]
            BoundedText::new("127.0.0.1").expect("arg"),
        ],
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
        .invoke_local_tool(InvokeLocalToolInputDto::new(
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
fn committed_rows_preserve_exact_correlation_identity_across_terminal_outcome() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();
    let error = ApplicationService::new(&repository)
        .invoke_local_tool(InvokeLocalToolInputDto::new(
            WorkspaceRoot::resolve(
                &WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy())
                    .expect("workspace"),
            )
            .expect("workspace"),
            session_id,
            run_id,
            call_id,
            "read",
            managed_read_input("missing"),
            fixture_time(),
        ))
        .expect_err("missing file fails");
    assert_eq!(error.code(), "tool_read_failed");
    let messages = repository.committed_messages();
    assert_eq!(messages.len(), 2);
    assert!(
        messages
            .iter()
            .all(|row| row.session_id() == session_id && row.run_id() == Some(run_id))
    );
    assert!(
        messages
            .iter()
            .all(|row| row.tool_call_id() == Some(call_id))
    );
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Failed);
    assert_eq!(results[0].session_id(), session_id);
    assert_eq!(results[0].run_id(), run_id);
    assert_eq!(results[0].call_id(), call_id);
    assert_eq!(results[0].tool_id(), "read");
    assert_eq!(results[0].content(), "tool_read_failed");
}

#[test]
fn local_tool_rejects_unknown_or_mismatched_id_before_effects() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let error = ApplicationService::new(&repository)
        .invoke_local_tool(InvokeLocalToolInputDto::new(
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
fn local_tool_hook_rejection_is_durable_and_skips_execution() {
    let root = hello_tool_root("hook-reject");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(RejectHook))
        .expect("hook registers");
    // The readable file would have completed; the hook rejection is recorded
    // as the terminal failed result of the already-committed call.
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(invoke_read_input_in_workspace(
            &hello_workspace(&root),
            "hello.txt",
        ))
        .expect_err("hook rejects");
    assert_eq!(error.code(), "blocked_by_hook");
    let messages = repository.committed_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].kind(), MessageKindDto::ToolCall);
    assert_eq!(messages[1].kind(), MessageKindDto::ToolResult);
    assert_eq!(messages[1].text(), "blocked_by_hook");
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Failed);
    assert_eq!(results[0].content(), "blocked_by_hook");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn public_dto_constructors_and_schedule_validation_cover_mismatch_paths() {
    let command = SendUserTurnCommandDto::new(SessionId::new(), IdempotencyKey::new(), "hello")
        .expect("command is valid");
    let input = SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), fixture_time());
    assert_eq!(input.occurred_at(), fixture_time());
    assert_eq!(input.config_snapshot().resolved(), snapshot().resolved());
    let request = ModelRequestDto::new(
        RunId::new(),
        "fixture",
        vec![ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid")],
        None,
        None,
    )
    .expect("request is valid");
    let error = ScheduleModelRunDto::new(command.session_id(), RunId::new(), request, snapshot())
        .expect_err("mismatched schedule is rejected");
    assert_eq!(error.code(), "invalid_model_run_schedule");

    let matching_run = RunId::new();
    let matching_request = ModelRequestDto::new(
        matching_run,
        "fixture",
        vec![ModelMessageDto::new(ModelRoleDto::Assistant, "answer").expect("message")],
        None,
        None,
    )
    .expect("request is valid");
    let scheduled = ScheduleModelRunDto::new(
        command.session_id(),
        matching_run,
        matching_request,
        snapshot(),
    )
    .expect("matching schedule is accepted");
    assert_eq!(scheduled.run_id(), matching_run);
    assert_eq!(scheduled.request().messages().len(), 1);
    assert_eq!(
        scheduled.safe_config().resolved().provider().model(),
        "fixture"
    );

    // A matching run identity whose model disagrees with the durable
    // selection is rejected through the other validation operand.
    let wrong_model_request = ModelRequestDto::new(
        matching_run,
        "other",
        vec![ModelMessageDto::new(ModelRoleDto::Assistant, "answer").expect("message")],
        None,
        None,
    )
    .expect("request is valid");
    let error = ScheduleModelRunDto::new(
        command.session_id(),
        matching_run,
        wrong_model_request,
        snapshot(),
    )
    .expect_err("model mismatch is rejected");
    assert_eq!(error.code(), "invalid_model_run_schedule");
}

#[test]
fn pre_execution_hook_matrix_covers_errors_transforms_and_rejections_per_phase() {
    for phase in [
        Phase::BeforeToolInvocation,
        Phase::BeforeWorkspaceResolution,
        Phase::AfterWorkspaceResolution,
        Phase::BeforeToolExecution,
    ] {
        // Operational hook failures fail closed and record the terminal failed
        // result of the committed call without starting execution.
        let root = hello_tool_root("matrix-error");
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let mut hooks = Registry::new();
        hooks
            .register(Box::new(DispatchErrorHook { phase }))
            .expect("hook registers");
        let error = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(invoke_read_input_in_workspace(
                &hello_workspace(&root),
                "hello.txt",
            ))
            .expect_err("hook dispatch errors fail closed");
        assert_eq!(error.code(), "hook_failed");
        assert_eq!(repository.committed_messages().len(), 2);
        let results = repository.committed_results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status(), ToolResultStatusDto::Failed);
        assert_eq!(results[0].content(), "hook_failed");
        let _ = fs::remove_dir_all(root);

        // Input transformations reroute the invocation to an existing file and
        // the tool executes with the transformed input.
        let root = hello_tool_root("matrix-input");
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let mut hooks = Registry::new();
        hooks
            .register(Box::new(PhaseOutcomeHook {
                phase,
                id: "matrix-input",
                outcome: HookOutcome::TransformInput(managed_read_input("hello.txt")),
            }))
            .expect("hook registers");
        let result = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(invoke_read_input_in_workspace(
                &hello_workspace(&root),
                "missing-before-transform.txt",
            ))
            .expect("transformed input is executed");
        let result = completed_outcome(result);
        assert_eq!(result, hello_read_result());
        assert_eq!(repository.committed_results()[0].content(), "hello");
        let _ = fs::remove_dir_all(root);

        // Result transformations are incompatible before execution: the hook
        // registry fails closed before the invocation starts and the tolerated
        // typed failure is durably recorded as the call's terminal result.
        let root = hello_tool_root("matrix-result");
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let mut hooks = Registry::new();
        hooks
            .register(Box::new(PhaseOutcomeHook {
                phase,
                id: "matrix-result",
                outcome: HookOutcome::TransformResult(ToolResult::Read(TextResult {
                    text: BoundedText::new("changed").expect("text"),
                    truncated: false,
                })),
            }))
            .expect("hook registers");
        let error = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(invoke_read_input_in_workspace(
                &hello_workspace(&root),
                "hello.txt",
            ))
            .expect_err("result transformations before execution are invalid");
        assert_eq!(error.code(), "invalid_hook_outcome");
        assert_eq!(
            error.message(),
            "hook outcome is incompatible with its phase"
        );
        assert_eq!(
            repository.committed_results()[0].content(),
            "invalid_hook_outcome"
        );
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn executed_phase_hook_outcomes_cover_invalid_input_and_error_paths() {
    let root = hello_tool_root("executed-invalid");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(PhaseOutcomeHook {
            phase: Phase::AfterToolExecution,
            id: "executed-invalid-input",
            outcome: HookOutcome::TransformInput(managed_read_input("other.txt")),
        }))
        .expect("hook registers");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(invoke_read_input_in_workspace(
            &hello_workspace(&root),
            "hello.txt",
        ))
        .expect_err("input transformations after execution are invalid");
    assert_eq!(error.code(), "invalid_hook_outcome");
    assert_eq!(
        error.message(),
        "input transformation is incompatible with its phase"
    );
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Failed);
    assert_eq!(results[0].content(), "invalid_hook_outcome");
    let _ = fs::remove_dir_all(root);

    let root = hello_tool_root("executed-error");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(DispatchErrorHook {
            phase: Phase::AfterToolExecution,
        }))
        .expect("hook registers");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(invoke_read_input_in_workspace(
            &hello_workspace(&root),
            "hello.txt",
        ))
        .expect_err("post-execution hook errors fail closed");
    assert_eq!(error.code(), "hook_failed");
    assert_eq!(
        repository.committed_results()[0].status(),
        ToolResultStatusDto::Failed
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn post_execution_result_phases_cover_rejection_and_invalid_input() {
    for phase in [
        Phase::BeforeToolResultPersist,
        Phase::BeforeToolResultModelContext,
    ] {
        // Hook rejections after execution record the failed terminal result
        // without any completed result.
        let root = hello_tool_root("post-reject");
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let mut hooks = Registry::new();
        hooks
            .register(Box::new(PhaseOutcomeHook {
                phase,
                id: "post-reject",
                outcome: HookOutcome::Reject(ErrorDto::validation(
                    "result_phase_blocked",
                    "hook blocks the committed result",
                )),
            }))
            .expect("hook registers");
        let error = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(invoke_read_input_in_workspace(
                &hello_workspace(&root),
                "hello.txt",
            ))
            .expect_err("post-execution rejections surface");
        assert_eq!(error.code(), "result_phase_blocked");
        let results = repository.committed_results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status(), ToolResultStatusDto::Failed);
        assert_eq!(results[0].content(), "result_phase_blocked");
        assert_eq!(repository.completed_result_count(), 0);
        let _ = fs::remove_dir_all(root);

        // Input transformations remain invalid for result phases.
        let root = hello_tool_root("post-input");
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let mut hooks = Registry::new();
        hooks
            .register(Box::new(PhaseOutcomeHook {
                phase,
                id: "post-input",
                outcome: HookOutcome::TransformInput(managed_read_input("other.txt")),
            }))
            .expect("hook registers");
        let error = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(invoke_read_input_in_workspace(
                &hello_workspace(&root),
                "hello.txt",
            ))
            .expect_err("input transformations in result phases are invalid");
        assert_eq!(error.code(), "invalid_hook_outcome");
        assert_eq!(
            error.message(),
            "input transformation is incompatible with its phase"
        );
        let results = repository.committed_results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status(), ToolResultStatusDto::Failed);
        assert_eq!(results[0].content(), "invalid_hook_outcome");
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn interrupt_and_snapshot_workflows_map_durable_results() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = snapshot();
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
    let messages = vec![
        MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::User,
            "hello",
            None,
            None,
            None,
        )
        .expect("fixture row is valid"),
    ];
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable(
        "fixture_unused",
        "accept is not used by this fixture",
    )));
    *repository.loaded_projection.borrow_mut() = Some(state.clone());
    *repository.recent_messages.borrow_mut() = messages.clone();
    let application = ApplicationService::new(&repository);

    let interrupted = application
        .interrupt_run(InterruptRunCommandDto::new(session_id, run_id))
        .expect("interrupt maps");
    assert!(matches!(
        interrupted,
        ProtocolAcceptedResultDto::InterruptRun(value)
            if value.session_id() == session_id && value.run_id() == run_id
    ));

    let snapshot = application
        .get_session_snapshot(GetSessionSnapshotQueryDto::new(session_id))
        .expect("snapshot maps");
    assert_eq!(snapshot.session_id(), session_id);
    assert_eq!(snapshot.schema_version(), CURRENT_DTO_SCHEMA_VERSION);
    assert_eq!(snapshot.projection(), &state);
    assert_eq!(snapshot.messages(), messages.as_slice());
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
        .create_session(CreateSessionWorkflowInputDto::new(create, fixture_time()))
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
fn local_tool_after_execution_transform_is_applied() {
    let root = hello_tool_root("execution-transform");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(PhaseOutcomeHook {
            phase: Phase::AfterToolExecution,
            id: "execution-transform",
            outcome: HookOutcome::TransformResult(ToolResult::Read(TextResult {
                text: BoundedText::new("changed").expect("text"),
                truncated: false,
            })),
        }))
        .expect("hook");
    let result = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(invoke_read_input_in_workspace(
            &hello_workspace(&root),
            "hello.txt",
        ))
        .expect("transformed read succeeds");
    let result = completed_outcome(result);
    assert_eq!(
        result,
        ToolResult::Read(TextResult {
            text: BoundedText::new("changed").expect("text"),
            truncated: false,
        })
    );
    assert_eq!(repository.committed_results()[0].content(), "changed");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_tool_covers_workspace_reject_and_all_post_execution_outcomes() {
    let root = hello_tool_root("branches");
    let workspace = hello_workspace(&root);

    for (phase, outcome, expected) in [
        (
            Phase::BeforeWorkspaceResolution,
            HookOutcome::Reject(ErrorDto::validation("workspace_blocked", "blocked")),
            "workspace_blocked",
        ),
        (
            Phase::AfterWorkspaceResolution,
            HookOutcome::TransformResult(ToolResult::Read(TextResult {
                text: BoundedText::new("x").expect("text"),
                truncated: false,
            })),
            "invalid_hook_outcome",
        ),
        (
            Phase::AfterToolExecution,
            HookOutcome::Reject(ErrorDto::validation("result_blocked", "blocked")),
            "result_blocked",
        ),
        (
            Phase::BeforeToolResultModelContext,
            HookOutcome::TransformInput(managed_read_input("hello.txt")),
            "invalid_hook_outcome",
        ),
    ] {
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let mut hooks = Registry::new();
        hooks
            .register(Box::new(PhaseOutcomeHook {
                phase,
                id: "branch",
                outcome,
            }))
            .expect("hook");
        let error = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(InvokeLocalToolInputDto::new(
                workspace.clone(),
                SessionId::new(),
                RunId::new(),
                ToolCallId::new(),
                "read",
                managed_read_input("hello.txt"),
                fixture_time(),
            ))
            .expect_err("hook branch rejects");
        assert_eq!(error.code(), expected);
        let results = repository.committed_results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status(), ToolResultStatusDto::Failed);
        assert_eq!(results[0].content(), expected);
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_tool_covers_dispatch_errors_and_post_effect_result_transforms() {
    let root = hello_tool_root("dispatch-errors");
    let workspace = hello_workspace(&root);
    for phase in [
        Phase::BeforeToolExecution,
        Phase::BeforeWorkspaceResolution,
        Phase::AfterWorkspaceResolution,
        Phase::AfterToolExecution,
        Phase::BeforeToolResultPersist,
        Phase::BeforeToolResultModelContext,
        Phase::AfterToolResultPublished,
    ] {
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let mut hooks = Registry::new();
        hooks
            .register(Box::new(DispatchErrorHook { phase }))
            .expect("hook");
        let error = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(InvokeLocalToolInputDto::new(
                workspace.clone(),
                SessionId::new(),
                RunId::new(),
                ToolCallId::new(),
                "read",
                managed_read_input("hello.txt"),
                fixture_time(),
            ))
            .expect_err("dispatch error");
        assert_eq!(error.code(), "hook_failed");
    }
    for phase in [
        Phase::BeforeToolResultPersist,
        Phase::BeforeToolResultModelContext,
    ] {
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let mut hooks = Registry::new();
        hooks
            .register(Box::new(PhaseOutcomeHook {
                phase,
                id: "post-transform",
                outcome: HookOutcome::TransformResult(ToolResult::Read(TextResult {
                    text: BoundedText::new("changed").expect("text"),
                    truncated: false,
                })),
            }))
            .expect("hook");
        let result = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(InvokeLocalToolInputDto::new(
                workspace.clone(),
                SessionId::new(),
                RunId::new(),
                ToolCallId::new(),
                "read",
                managed_read_input("hello.txt"),
                fixture_time(),
            ))
            .expect("transformed result");
        let result = completed_outcome(result);
        assert_eq!(
            result,
            ToolResult::Read(TextResult {
                text: BoundedText::new("changed").expect("text"),
                truncated: false,
            })
        );
        let results = repository.committed_results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status(), ToolResultStatusDto::Completed);
        assert_eq!(results[0].content(), "changed");
    }

    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(PhaseOutcomeHook {
            phase: Phase::AfterToolResultPublished,
            id: "published-continue",
            outcome: HookOutcome::Continue,
        }))
        .expect("hook");
    let result = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(InvokeLocalToolInputDto::new(
            workspace,
            SessionId::new(),
            RunId::new(),
            ToolCallId::new(),
            "read",
            managed_read_input("hello.txt"),
            fixture_time(),
        ))
        .expect("published Continue is valid");
    let result = completed_outcome(result);
    assert!(matches!(result, ToolResult::Read(_)));
    assert_eq!(repository.completed_result_count(), 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_tool_covers_invocation_and_pre_effect_hook_errors_and_rejections() {
    let root = hello_tool_root("pre-hooks");
    let workspace = hello_workspace(&root);
    for phase in [
        Phase::BeforeToolInvocation,
        Phase::BeforeWorkspaceResolution,
        Phase::BeforeToolExecution,
    ] {
        let outcome = HookOutcome::Reject(ErrorDto::validation("hook_rejected", "rejected"));
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let mut hooks = Registry::new();
        hooks
            .register(Box::new(PhaseOutcomeHook {
                phase,
                outcome,
                id: "reject",
            }))
            .expect("hook");
        let error = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(invoke_read_input_in_workspace(&workspace, "hello.txt"))
            .expect_err("hook rejection");
        assert_eq!(error.code(), "hook_rejected");
        assert_eq!(repository.committed_results()[0].content(), "hook_rejected");

        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let mut hooks = Registry::new();
        hooks
            .register(Box::new(DispatchErrorHook { phase }))
            .expect("hook");
        let error = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(invoke_read_input_in_workspace(&workspace, "hello.txt"))
            .expect_err("hook error");
        assert_eq!(error.code(), "hook_failed");
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_tool_records_partial_terminal_status_on_interruption() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    // The cancellation is observed after the child has been spawned, so the
    // call ends with whatever output was captured before the stop.
    let signal = CancellationSignal::new();
    let cancellation = signal.clone();
    let canceller = std::thread::spawn(move || {
        // Wait for a confirmed child spawn instead of racing a fixed sleep:
        // the cancellation then provably lands while the external process is
        // running, so the interruption cause is an observed stop.
        assert!(
            cancellation.wait_until_spawn_observed(std::time::Duration::from_secs(10)),
            "execute child was never observed after spawn"
        );
        cancellation.cancel();
    });
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool(
            InvokeLocalToolInputDto::new(
                WorkspaceRoot::resolve(
                    &WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy()).expect("root"),
                )
                .expect("workspace"),
                SessionId::new(),
                RunId::new(),
                ToolCallId::new(),
                "execute",
                sleeping_execute_input(),
                fixture_time(),
            )
            .with_cancellation(signal),
        )
        .expect("an interrupted execute is a partial outcome");
    canceller.join().expect("cancellation helper completes");
    let LocalToolInvocationOutcomeDto::Partial { stopped, .. } = outcome else {
        unreachable!("the stopped execute must be a partial outcome")
    };
    assert!(stopped);
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Partial);
    assert!(results[0].content().contains(
        "[The tool call was stopped before a final result; the output above is partial.]"
    ));
    assert_eq!(repository.completed_result_count(), 0);
}

#[test]
fn local_tool_covers_workspace_resolved_error_and_rejection() {
    let outcome = HookOutcome::Reject(ErrorDto::validation("resolved_blocked", "blocked"));
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(PhaseOutcomeHook {
            phase: Phase::AfterWorkspaceResolution,
            outcome,
            id: "resolved-reject",
        }))
        .expect("hook");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(invoke_read_input("missing"))
        .expect_err("resolved rejection");
    assert_eq!(error.code(), "resolved_blocked");

    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(DispatchErrorHook {
            phase: Phase::AfterWorkspaceResolution,
        }))
        .expect("hook");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(invoke_read_input("missing"))
        .expect_err("resolved dispatch error");
    assert_eq!(error.code(), "hook_failed");
}

#[test]
fn cancelled_tool_lifecycle_is_terminal_and_not_completed_or_replayed() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool(
            InvokeLocalToolInputDto::new(
                WorkspaceRoot::resolve(
                    &WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy())
                        .expect("workspace dto"),
                )
                .expect("workspace"),
                session_id,
                run_id,
                call_id,
                "execute",
                cancelled_execute_input(),
                fixture_time(),
            )
            .with_cancellation(CancellationSignal::cancelled()),
        )
        .expect("a pre-start cancellation is a partial outcome");
    assert_eq!(
        outcome,
        LocalToolInvocationOutcomeDto::Partial {
            stopped: true,
            result: None,
        }
    );

    // The call row is durable before the cancellation is observed, and the
    // partial result is the one terminal row the call records.
    let messages = repository.committed_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].kind(), MessageKindDto::ToolCall);
    assert!(
        messages
            .iter()
            .all(|row| row.session_id() == session_id && row.run_id() == Some(run_id))
    );
    assert!(
        messages
            .iter()
            .all(|row| row.tool_call_id() == Some(call_id))
    );
    assert_eq!(repository.completed_result_count(), 0);
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Partial);
    assert_eq!(
        results[0].content(),
        "[The tool call was stopped before a final result.]"
    );
}

struct FailOpenFailingHook {
    hook_id: &'static str,
    revision: u32,
}
impl Hook for FailOpenFailingHook {
    fn id(&self) -> &'static str {
        self.hook_id
    }
    fn registration_revision(&self) -> u32 {
        self.revision
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::BeforeWorkspaceResolution]
    }
    fn priority(&self) -> u32 {
        0
    }
    fn failure_policy(&self, _: Phase) -> FailurePolicy {
        FailurePolicy::FailOpen
    }
    fn run(&self, _: &PhaseContext) -> DtoResult<HookOutcome> {
        Err(ErrorDto::validation(
            "fail_open_failure",
            "FAKE_SECRET_9f3a workspace detail",
        ))
    }
}

struct RecordingObserver {
    observations: RefCell<Vec<HookObservability>>,
}
impl HookObservationPort for RecordingObserver {
    fn observe_hook_failure(&self, observation: HookObservability) {
        self.observations.borrow_mut().push(observation);
    }
}

#[test]
fn fail_open_hook_failures_reach_the_observation_boundary_with_redacted_metadata() {
    let root = hello_tool_root("fail-open");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(FailOpenFailingHook {
            hook_id: "fail-open-alpha",
            revision: 5,
        }))
        .expect("hook registers");
    hooks
        .register(Box::new(FailOpenFailingHook {
            hook_id: "fail-open-beta",
            revision: 7,
        }))
        .expect("hook registers");
    let observer = RecordingObserver {
        observations: RefCell::new(Vec::new()),
    };
    let result = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool_with_observation(
            invoke_read_input_in_workspace(&hello_workspace(&root), "hello.txt"),
            &observer,
        )
        .expect("fail-open failures continue execution");
    let result = completed_outcome(result);
    assert!(matches!(result, ToolResult::Read(_)));

    // Metadata is not discarded: every tolerated failure reaches the boundary
    // with its exact safe identity, in registry-deterministic order.
    let observations = observer.observations.borrow();
    assert_eq!(
        *observations,
        vec![
            HookObservability {
                hook_id: "fail-open-alpha",
                registration_revision: 5,
                phase: Phase::BeforeWorkspaceResolution,
                failure_policy: FailurePolicy::FailOpen,
            },
            HookObservability {
                hook_id: "fail-open-beta",
                registration_revision: 7,
                phase: Phase::BeforeWorkspaceResolution,
                failure_policy: FailurePolicy::FailOpen,
            },
        ]
    );
    // The committed rows stay redacted: the tolerated hook error's code and
    // message detail, and absolute filesystem paths, never cross the durable
    // boundary even though the failure carried them as input.
    let messages = repository.committed_messages();
    assert_eq!(messages.len(), 2);
    assert_eq!(repository.completed_result_count(), 1);
    let rendered = format!("{messages:?} {:?}", repository.committed_results());
    assert!(!rendered.contains("FAKE_SECRET"));
    assert!(!rendered.contains(&root.to_string_lossy().to_string()));
    assert!(!rendered.contains("fail_open_failure"));
    let _ = fs::remove_dir_all(root);
}

struct CapturingPublisher {
    publications: RefCell<Vec<MessageProjectionDto>>,
    failure: RefCell<Option<ErrorDto>>,
}

impl CapturingPublisher {
    const fn recording() -> Self {
        Self {
            publications: RefCell::new(Vec::new()),
            failure: RefCell::new(None),
        }
    }

    const fn failing(error: ErrorDto) -> Self {
        Self {
            publications: RefCell::new(Vec::new()),
            failure: RefCell::new(Some(error)),
        }
    }

    fn published(&self) -> Vec<MessageProjectionDto> {
        self.publications.borrow().clone()
    }
}

impl ToolResultPublicationPort for CapturingPublisher {
    fn publish_committed_message(&self, message: &MessageProjectionDto) -> DtoResult<()> {
        self.publications.borrow_mut().push(message.clone());
        self.failure
            .borrow()
            .as_ref()
            .map_or(Ok(()), |error| Err(error.clone()))
    }
}

struct FailOpenPublishedHook;

impl Hook for FailOpenPublishedHook {
    fn id(&self) -> &'static str {
        "fail-open-published"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::AfterToolResultPublished]
    }
    fn priority(&self) -> u32 {
        0
    }
    fn failure_policy(&self, _: Phase) -> FailurePolicy {
        FailurePolicy::FailOpen
    }
    fn run(&self, _: &PhaseContext) -> DtoResult<HookOutcome> {
        Err(ErrorDto::validation(
            "fail_open_published",
            "post-publish tolerated failure",
        ))
    }
}

#[test]
fn publication_failure_propagates_after_the_committed_tool_call_row() {
    let root = hello_tool_root("publication-failure");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = CapturingPublisher::failing(ErrorDto::unavailable(
        "publication_unavailable",
        "publication boundary refused the committed result",
    ));
    let error = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            invoke_read_input_in_workspace(&hello_workspace(&root), "hello.txt"),
            &publisher,
        )
        .expect_err("publication failure must surface");
    assert_eq!(error.code(), "publication_unavailable");

    // The committed call row reached the boundary exactly once; the caller sees
    // the publication error and the tool never dispatches.
    let published = publisher.published();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].kind(), MessageKindDto::ToolCall);
    // The committed call row stays durable and no terminal row is committed.
    assert!(repository.committed_results().is_empty());
    assert_eq!(repository.completed_result_count(), 0);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn after_publish_hook_rejection_surfaces_after_the_completed_commit() {
    let root = hello_tool_root("publish-reject");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(PhaseOutcomeHook {
            phase: Phase::AfterToolResultPublished,
            id: "published-reject",
            outcome: HookOutcome::Reject(ErrorDto::validation(
                "after_publish_blocked",
                "hook refuses after publication",
            )),
        }))
        .expect("hook registers");
    let publisher = CapturingPublisher::recording();
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool_with_publication(
            invoke_read_input_in_workspace(&hello_workspace(&root), "hello.txt"),
            &publisher,
        )
        .expect_err("post-publish rejection surfaces");
    assert_eq!(error.code(), "after_publish_blocked");
    assert_eq!(error.message(), "hook refuses after publication");
    // The committed call and result rows were published before the post-publish
    // hook ran.
    let published = publisher.published();
    assert_eq!(published.len(), 2);
    assert_eq!(published[0].kind(), MessageKindDto::ToolCall);
    assert_eq!(published[1].kind(), MessageKindDto::ToolResult);
    // The completed commit is preserved and not duplicated as a failure.
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Completed);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn after_publish_transform_outcomes_are_invalidated_without_extra_failures() {
    // Result transformations reach the application boundary and are refused
    // after publication; input transformations are already refused by the hook
    // registry as incompatible with the published phase. Both stay fail-closed
    // without committing any post-completion row.
    let outcomes = [
        (
            HookOutcome::TransformResult(ToolResult::Read(TextResult {
                text: BoundedText::new("changed").expect("text"),
                truncated: false,
            })),
            "published result cannot be transformed",
        ),
        (
            HookOutcome::TransformInput(managed_read_input("elsewhere")),
            "input transformation is incompatible with its phase",
        ),
    ];
    for (outcome, expected_message) in outcomes {
        let root = hello_tool_root("publish-transform");
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let mut hooks = Registry::new();
        hooks
            .register(Box::new(PhaseOutcomeHook {
                phase: Phase::AfterToolResultPublished,
                id: "published-transform",
                outcome,
            }))
            .expect("hook registers");
        let error = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool_with_publication(
                invoke_read_input_in_workspace(&hello_workspace(&root), "hello.txt"),
                &(),
            )
            .expect_err("published results cannot be transformed");
        assert_eq!(error.code(), "invalid_hook_outcome");
        assert_eq!(error.message(), expected_message);
        // Completion stays durable and exactly one terminal row exists.
        let results = repository.committed_results();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status(), ToolResultStatusDto::Completed);
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn after_publish_hook_error_fails_closed_on_the_completed_commit() {
    let root = hello_tool_root("publish-dispatch-error");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(DispatchErrorHook {
            phase: Phase::AfterToolResultPublished,
        }))
        .expect("hook registers");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool_with_publication(
            invoke_read_input_in_workspace(&hello_workspace(&root), "hello.txt"),
            &(),
        )
        .expect_err("post-publish dispatch error surfaces");
    assert_eq!(error.code(), "hook_failed");
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Completed);
    assert_eq!(repository.completed_result_count(), 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn fail_open_failures_in_the_published_phase_reach_the_observer() {
    let root = hello_tool_root("published-observation");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(FailOpenPublishedHook))
        .expect("hook registers");
    let observer = RecordingObserver {
        observations: RefCell::new(Vec::new()),
    };
    let result = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool_with_observation(
            invoke_read_input_in_workspace(&hello_workspace(&root), "hello.txt"),
            &observer,
        )
        .expect("fail-open failures after publication stay tolerated");
    let result = completed_outcome(result);
    assert_eq!(result, hello_read_result());
    // The tolerated post-publish failure is forwarded with safe identity only.
    assert_eq!(
        *observer.observations.borrow(),
        vec![HookObservability {
            hook_id: "fail-open-published",
            registration_revision: 1,
            phase: Phase::AfterToolResultPublished,
            failure_policy: FailurePolicy::FailOpen,
        }]
    );
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status(), ToolResultStatusDto::Completed);
    let _ = fs::remove_dir_all(root);
}

enum MatrixHook {
    None,
    Reject(Phase),
}

#[test]
fn selected_commit_failures_propagate_from_each_commit_point() {
    // The call row commits first and the terminal result row second; each
    // selected failure must surface and leave only the earlier commit durable.
    let scenarios: Vec<(&str, usize, MatrixHook, usize)> = vec![
        ("tool-call-commit", 1, MatrixHook::None, 0),
        ("completed-commit", 2, MatrixHook::None, 1),
        (
            "invocation-rejection-commit",
            2,
            MatrixHook::Reject(Phase::BeforeToolInvocation),
            1,
        ),
        (
            "execution-rejection-commit",
            2,
            MatrixHook::Reject(Phase::BeforeToolExecution),
            1,
        ),
    ];
    for (label, failing_call, hook, expected_messages) in scenarios {
        let root = hello_tool_root("commit-failure");
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        *repository.commit_failures.borrow_mut() = vec![failing_call];
        let mut hooks = Registry::new();
        if let MatrixHook::Reject(phase) = hook {
            hooks
                .register(Box::new(PhaseOutcomeHook {
                    phase,
                    id: "commit-failure-reject",
                    outcome: HookOutcome::Reject(ErrorDto::validation(
                        "commit_scenario_blocked",
                        "blocked",
                    )),
                }))
                .expect("hook registers");
        }
        let error = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(invoke_read_input_in_workspace(
                &hello_workspace(&root),
                "hello.txt",
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
    repository: &'a FakeRepository,
    publications: RefCell<Vec<MessageProjectionDto>>,
    results_at_publish: RefCell<Vec<usize>>,
}

impl<'a> TerminalOrderingProbe<'a> {
    const fn new(repository: &'a FakeRepository) -> Self {
        Self {
            repository,
            publications: RefCell::new(Vec::new()),
            results_at_publish: RefCell::new(Vec::new()),
        }
    }

    fn published(&self) -> Vec<MessageProjectionDto> {
        self.publications.borrow().clone()
    }

    /// Asserts the boundary saw the committed call row before execution and the
    /// committed terminal row after its own commit, in that order.
    fn assert_call_row_then_one_terminal(&self) {
        let published = self.published();
        assert_eq!(published.len(), 2, "the call row and one terminal row");
        assert_eq!(published[0].kind(), MessageKindDto::ToolCall);
        assert_eq!(published[1].kind(), MessageKindDto::ToolResult);
        assert_eq!(
            *self.results_at_publish.borrow(),
            vec![0, 1],
            "the terminal row publishes after its own durable commit"
        );
    }
}

impl ToolResultPublicationPort for TerminalOrderingProbe<'_> {
    fn publish_committed_message(&self, message: &MessageProjectionDto) -> DtoResult<()> {
        self.results_at_publish
            .borrow_mut()
            .push(self.repository.committed_results().len());
        self.publications.borrow_mut().push(message.clone());
        Ok(())
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
    let answering = messages
        .last()
        .expect("the terminal result commits with its answering row");
    assert_eq!(answering.kind(), MessageKindDto::ToolResult);
    assert_eq!(answering.tool_call_id(), Some(call_id));
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
            InvokeLocalToolInputDto::new(
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
    // Exactly one terminal result row exists when publication runs, proving
    // the terminal commit precedes the publication boundary.
    assert_eq!(*publisher.results_at_publish.borrow(), vec![0, 1]);
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
            InvokeLocalToolInputDto::new(
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
    publisher.assert_call_row_then_one_terminal();
    drop(publisher);
    drop(repository);

    // A pre-start cancellation is a Partial outcome: correlated Partial
    // evidence is durable and the publication boundary is never reached.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            InvokeLocalToolInputDto::new(
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
    let canceller = std::thread::spawn(move || {
        assert!(
            cancellation.wait_until_spawn_observed(std::time::Duration::from_secs(10)),
            "execute child was never observed after spawn"
        );
        cancellation.cancel();
    });
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            InvokeLocalToolInputDto::new(
                hello_workspace(&root),
                session_id,
                run_id,
                call_id,
                "execute",
                sleeping_execute_input(),
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
    publisher.assert_call_row_then_one_terminal();

    let _ = fs::remove_dir_all(root);
}

#[test]
fn terminal_commits_carry_typed_result_evidence_before_publication() {
    let root = hello_tool_root("evidence");
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();

    // Success: the terminal Completed commit atomically carries the rendered
    // result content with the exact invocation identity.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            InvokeLocalToolInputDto::new(
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
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    let completed = &results[0];
    assert_eq!(completed.status(), ToolResultStatusDto::Completed);
    assert_eq!(completed.content(), "hello");
    assert_eq!(completed.tool_id(), "read");
    assert_eq!(completed.occurred_at(), fixture_time());
    assert!(completed.metadata().is_empty());
    // The evidence-carrying terminal commit is durable before publication.
    assert_eq!(*publisher.results_at_publish.borrow(), vec![0, 1]);
    drop(publisher);
    drop(repository);

    // Failure: the terminal Failed commit classifies the safe error code.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let error = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            InvokeLocalToolInputDto::new(
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
    let results = repository.committed_results();
    assert_eq!(results.len(), 1);
    let failed = &results[0];
    assert_eq!(failed.status(), ToolResultStatusDto::Failed);
    assert_eq!(failed.content(), "tool_read_failed");
    assert!(failed.metadata().is_empty());
    publisher.assert_call_row_then_one_terminal();
    drop(publisher);
    drop(repository);

    // A pre-start cancellation: the terminal Partial commit classifies the
    // interrupted call with the exact stopped notice.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            InvokeLocalToolInputDto::new(
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
    let results = repository.committed_results();
    let partial = &results[0];
    assert_eq!(partial.status(), ToolResultStatusDto::Partial);
    assert_eq!(
        partial.content(),
        "[The tool call was stopped before a final result.]"
    );
    publisher.assert_call_row_then_one_terminal();
    drop(publisher);
    drop(repository);

    // An interrupted external process: the terminal Partial commit keeps the
    // captured output and never publishes.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let signal = CancellationSignal::new();
    let cancellation = signal.clone();
    let canceller = std::thread::spawn(move || {
        assert!(
            cancellation.wait_until_spawn_observed(std::time::Duration::from_secs(10)),
            "execute child was never observed after spawn"
        );
        cancellation.cancel();
    });
    let outcome = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            InvokeLocalToolInputDto::new(
                hello_workspace(&root),
                session_id,
                run_id,
                call_id,
                "execute",
                sleeping_execute_input(),
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
    let results = repository.committed_results();
    let partial = &results[0];
    assert_eq!(partial.status(), ToolResultStatusDto::Partial);
    assert!(partial.content().contains("stdout:"));
    assert!(partial.content().contains(
        "[The tool call was stopped before a final result; the output above is partial.]"
    ));
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
        .invoke_local_tool(InvokeLocalToolInputDto::new(
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

#[derive(Default)]
struct RecordingDispatchPort {
    inputs: RefCell<Vec<ScheduleModelRunDto>>,
    failure: RefCell<Option<ErrorDto>>,
}

impl ModelRunDispatchPort for RecordingDispatchPort {
    fn dispatch_model_run(&self, input: ScheduleModelRunDto) -> DtoResult<()> {
        self.inputs.borrow_mut().push(input);
        self.failure.borrow_mut().take().map_or(Ok(()), Err)
    }
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

/// Records the dispatch order of the hook phases it declares.
struct OrderRecordingHook {
    order: Arc<Mutex<Vec<&'static str>>>,
}

impl Hook for OrderRecordingHook {
    fn id(&self) -> &'static str {
        "order-recorder"
    }

    fn phases(&self) -> &'static [Phase] {
        &[
            Phase::BeforeToolInvocation,
            Phase::BeforeWorkspaceResolution,
            Phase::AfterWorkspaceResolution,
            Phase::BeforeToolExecution,
            Phase::AfterToolExecution,
            Phase::BeforeToolResultPersist,
            Phase::BeforeToolResultModelContext,
            Phase::AfterToolResultPublished,
        ]
    }

    fn priority(&self) -> u32 {
        0
    }

    fn run(&self, context: &PhaseContext) -> DtoResult<HookOutcome> {
        self.order
            .lock()
            .expect("order lock is available")
            .push(phase_name(context));
        Ok(HookOutcome::Continue)
    }
}

const fn phase_name(context: &PhaseContext) -> &'static str {
    match context {
        PhaseContext::Invocation { .. } => "invocation",
        PhaseContext::WorkspaceResolution { .. } => "workspace_resolution",
        PhaseContext::WorkspaceResolved { .. } => "workspace_resolved",
        PhaseContext::Execution { .. } => "execution",
        PhaseContext::Executed { .. } => "executed",
        PhaseContext::Persist { .. } => "persist",
        PhaseContext::ModelContext { .. } => "model_context",
        PhaseContext::Published { .. } => "published",
    }
}

/// Records its own resolution between the two workspace hook phases.
struct OrderRecordingBoundary {
    order: Arc<Mutex<Vec<&'static str>>>,
}

impl WorkspaceBoundaryPort for OrderRecordingBoundary {
    fn resolve(&self, workspace: &WorkspaceRoot) -> DtoResult<WorkspaceRoot> {
        self.order
            .lock()
            .expect("order lock is available")
            .push("boundary");
        Ok(workspace.clone())
    }
}

#[test]
fn hook_phases_dispatch_in_order_around_identity_validation_and_the_workspace_boundary() {
    let root = hello_tool_root("phase-order");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(OrderRecordingHook {
            order: Arc::clone(&order),
        }))
        .expect("hook registers");
    ApplicationService::with_hooks(&repository, hooks)
        .with_workspace_boundary(OrderRecordingBoundary {
            order: Arc::clone(&order),
        })
        .invoke_local_tool(invoke_read_input_in_workspace(
            &hello_workspace(&root),
            "hello.txt",
        ))
        .expect("read succeeds");
    assert_eq!(
        *order.lock().expect("order lock is available"),
        vec![
            "invocation",
            "workspace_resolution",
            "boundary",
            "workspace_resolved",
            "execution",
            "executed",
            "persist",
            "model_context",
            "published",
        ],
        "the entry phase precedes the boundary, and the terminal phases follow execution"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_invocation_phase_dispatches_before_identity_validation() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let order = Arc::new(Mutex::new(Vec::new()));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(OrderRecordingHook {
            order: Arc::clone(&order),
        }))
        .expect("hook registers");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(InvokeLocalToolInputDto::new(
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
    assert_eq!(
        *order.lock().expect("order lock is available"),
        vec!["invocation"],
        "the entry phase runs before the identity check rejects the call"
    );
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
fn send_user_turn_and_schedule_propagates_admission_failures() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::validation(
        "turn_admission_denied",
        "the durable session refused the turn",
    )));
    let dispatch = RecordingDispatchPort::default();
    let error = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            send_command(session_id),
            SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), fixture_time()),
            &dispatch,
        )
        .expect_err("admission failure is propagated");
    assert_eq!(error.code(), "turn_admission_denied");
    let inputs = repository.accepted_inputs.borrow();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].session_id(), session_id);
    assert_eq!(inputs[0].content(), "latest");
    drop(inputs);
    assert!(dispatch.inputs.borrow().is_empty());
    assert!(repository.finishes.borrow().is_empty());
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
            SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), fixture_time()),
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
    let config = snapshot();
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
            SendUserTurnWorkflowInputDto::new(run_id, config.clone(), fixture_time()),
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
        let config = snapshot();
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
                SendUserTurnWorkflowInputDto::new(run_id, config, fixture_time()),
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
    let config = snapshot();
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
            SendUserTurnWorkflowInputDto::new(run_id, config, fixture_time()),
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
    let config = snapshot();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository.starting_context.borrow_mut() = Some(starting_context(session_id, run_id, &config));
    let scheduled = ApplicationService::new(&repository)
        .schedule_starting_run(session_id, run_id)
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
fn schedule_starting_run_propagates_context_load_errors() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let error = ApplicationService::new(&repository)
        .schedule_starting_run(SessionId::new(), RunId::new())
        .expect_err("missing durable context is propagated");
    assert_eq!(error.code(), "run_model_context_unavailable");
}

#[test]
fn create_session_propagates_repository_errors() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let command = CreateSessionCommandDto::new(
        ProjectId::new(),
        SessionId::new(),
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
    );
    let error = ApplicationService::new(&repository)
        .create_session(CreateSessionWorkflowInputDto::new(command, fixture_time()))
        .expect_err("repository failure is propagated");
    assert_eq!(error.code(), "fixture_missing_result");
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
fn pre_execution_rejection_commit_failure_propagates() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository.commit_failures.borrow_mut() = vec![2];
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(PhaseOutcomeHook {
            phase: Phase::BeforeToolExecution,
            id: "rejection-commit-failure",
            outcome: HookOutcome::Reject(ErrorDto::validation(
                "execution_blocked",
                "blocked before execution",
            )),
        }))
        .expect("hook registers");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(invoke_read_input("missing"))
        .expect_err("rejection commit failure is propagated");
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
