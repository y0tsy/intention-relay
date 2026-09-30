#![allow(
    clippy::expect_used,
    reason = "Focused application fixtures use expect to provide precise test failures."
)]

use std::cell::RefCell;
use std::fs;

use intention_application::{
    ApplicationService, CreateSessionWorkflowInputDto, HookObservationPort,
    InvokeLocalToolInputDto, ModelRunDispatchPort, ScheduleModelRunDto,
    SendUserTurnWorkflowInputDto, ToolResultPublicationInputDto, ToolResultPublicationPort,
    WorkspaceBoundaryPort,
};
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    CreateSessionCommandDto, DomainEventDto, GetSessionSnapshotQueryDto,
    RemoveQueuedTurnCommandDto, RunEventCursorDto, RunEventTailPageDto, RunModeDto,
    RunProjectionDto, RunReplayDto, RunStartedEventDto, RunStatusDto, SendUserTurnCommandDto,
    SessionProjectionDto, WorkspaceRootDto,
};
use intention_hooks::{
    FailurePolicy, Hook, HookObservability, Outcome as HookOutcome, Phase, PhaseContext, Registry,
};
use intention_protocol::{ProtocolAcceptedResultDto, SendUserTurnOutcomeDto};
use intention_runtime::{ModelMessageDto, ModelRequestDto, ModelRoleDto};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, AppendModelRunFactsInputDto,
    AppendModelRunFactsOutcomeDto, CommittedChangeDto, CreateSessionInputDto,
    ModelContextMessageDto, ModelContextRoleDto, RecoverUnfinishedRunsInputDto,
    RemoveQueuedTurnInputDto, StartingRunModelContextDto, StorageRepositoryDto,
    ToolResultEvidenceDto, ToolResultKindDto, TransitionRunInputDto,
};
use intention_tools::{ReadInput, ToolInput};
use intention_types::ToolCallId;
use intention_types::{
    ConfigRevisionId, DtoResult, ErrorDto, EventEnvelopeDto, EventId, EventMetadataDto, ProjectId,
    QueuePositionDto, RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId, TimestampDto,
    TurnId, WorkspaceId,
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
    queued_turns: Vec<intention_domain::QueuedTurnProjectionDto>,
    position: u64,
) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
        active_run.map(RunProjectionDto::config_revision_id),
        active_run,
        queued_turns,
        SessionEventSequenceDto::new(position),
    )
    .expect("fixture projection is valid")
}

fn change(
    projection: SessionProjectionDto,
    outcome: Option<AcceptedTurnOutcomeDto>,
) -> CommittedChangeDto {
    CommittedChangeDto::new(
        projection.clone(),
        projection.at_sequence(),
        Vec::new(),
        outcome,
    )
    .expect("fixture change is valid")
}

struct FakeRepository {
    accepted: RefCell<DtoResult<CommittedChangeDto>>,
    accepted_inputs: RefCell<Vec<AcceptUserTurnInputDto>>,
    created: RefCell<Option<CommittedChangeDto>>,
    removed: RefCell<Option<CommittedChangeDto>>,
    transitioned: RefCell<Option<CommittedChangeDto>>,
    loaded_snapshot: RefCell<Option<SessionProjectionDto>>,
    starting_context: RefCell<Option<StartingRunModelContextDto>>,
    tool_events: RefCell<Vec<intention_domain::ToolLifecycleEventDto>>,
    result_evidence: RefCell<Vec<Option<ToolResultEvidenceDto>>>,
    tool_error: RefCell<Option<ErrorDto>>,
    append_calls: RefCell<usize>,
    append_failures: RefCell<Vec<usize>>,
}

impl FakeRepository {
    const fn with_accepted(accepted: DtoResult<CommittedChangeDto>) -> Self {
        Self {
            accepted: RefCell::new(accepted),
            accepted_inputs: RefCell::new(Vec::new()),
            created: RefCell::new(None),
            removed: RefCell::new(None),
            transitioned: RefCell::new(None),
            loaded_snapshot: RefCell::new(None),
            starting_context: RefCell::new(None),
            tool_events: RefCell::new(Vec::new()),
            result_evidence: RefCell::new(Vec::new()),
            tool_error: RefCell::new(None),
            append_calls: RefCell::new(0),
            append_failures: RefCell::new(Vec::new()),
        }
    }
}

impl StorageRepositoryDto for FakeRepository {
    fn append_tool_lifecycle_event(
        &self,
        input: intention_storage::AppendToolLifecycleEventInputDto,
    ) -> DtoResult<intention_types::EventEnvelopeDto<DomainEventDto>> {
        if let Some(error) = self.tool_error.borrow().clone() {
            return Err(error);
        }
        let call = {
            let mut calls = self.append_calls.borrow_mut();
            *calls += 1;
            *calls
        };
        if self.append_failures.borrow().contains(&call) {
            return Err(ErrorDto::unavailable(
                "append_unavailable",
                "append refused at the selected call",
            ));
        }
        let event = input.event().clone();
        self.tool_events.borrow_mut().push(event.clone());
        self.result_evidence
            .borrow_mut()
            .push(input.result().cloned());
        Ok(intention_types::EventEnvelopeDto::new(
            intention_types::EventMetadataDto::new(
                SchemaVersionDto::new(1, 0),
                intention_types::EventId::new(),
                event.session_id(),
                Some(event.run_id()),
                None,
                SessionEventSequenceDto::new(self.tool_events.borrow().len() as u64 + 1),
                event.occurred_at(),
            ),
            DomainEventDto::ToolLifecycle(event),
        ))
    }
    fn create_session(&self, _input: CreateSessionInputDto) -> DtoResult<CommittedChangeDto> {
        self.created.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
        })
    }

    fn accept_user_turn(&self, input: AcceptUserTurnInputDto) -> DtoResult<CommittedChangeDto> {
        self.accepted_inputs.borrow_mut().push(input);
        self.accepted.borrow().clone()
    }

    fn remove_queued_turn(
        &self,
        _input: RemoveQueuedTurnInputDto,
    ) -> DtoResult<CommittedChangeDto> {
        self.removed.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
        })
    }

    fn transition_run(&self, _input: TransitionRunInputDto) -> DtoResult<CommittedChangeDto> {
        self.transitioned.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
        })
    }

    fn append_model_run_facts(
        &self,
        _input: AppendModelRunFactsInputDto,
    ) -> DtoResult<AppendModelRunFactsOutcomeDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "model facts are not used by this fixture",
        ))
    }

    fn load_current_run_replay(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<RunReplayDto> {
        Err(ErrorDto::unavailable(
            "fixture_missing_result",
            "fixture result missing",
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

    fn load_run_tail(
        &self,
        session_id: SessionId,
        run_id: RunId,
        _after_cursor: RunEventCursorDto,
    ) -> DtoResult<RunEventTailPageDto> {
        Ok(RunEventTailPageDto::empty(
            session_id,
            run_id,
            RunEventCursorDto::new(0),
        ))
    }

    fn recover_unfinished_runs(
        &self,
        _input: RecoverUnfinishedRunsInputDto,
    ) -> DtoResult<Vec<CommittedChangeDto>> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "recovery is not used by this fixture",
        ))
    }

    fn load_session_snapshot(&self, _session_id: SessionId) -> DtoResult<SessionProjectionDto> {
        self.loaded_snapshot.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
        })
    }

    fn load_tail(
        &self,
        _session_id: SessionId,
        _after_sequence: SessionEventSequenceDto,
    ) -> DtoResult<Vec<intention_types::EventEnvelopeDto<DomainEventDto>>> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "tail is not used by this fixture",
        ))
    }

    fn accept_configuration_revision(&self, _snapshot: ConfigSnapshotDto) -> DtoResult<()> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "config is not used by this fixture",
        ))
    }
}

#[test]
fn local_tool_success_records_admission_and_completion() {
    let root_path = std::env::temp_dir().join(format!("intention-app-{}", SessionId::new()));
    fs::create_dir_all(&root_path).expect("fixture workspace can be created");
    fs::write(root_path.join("hello.txt"), "hello").expect("fixture file can be written");
    let root = WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root_path.to_string_lossy()).expect("workspace dto"),
    )
    .expect("workspace is valid");
    let session = SessionId::new();
    let run = RunId::new();
    let call = ToolCallId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    // The fake only records lifecycle inputs; make the append operation succeed.
    let result = ApplicationService::new(&repository)
        .invoke_local_tool(InvokeLocalToolInputDto::new(
            root,
            session,
            run,
            call,
            "read",
            ToolInput::Read(ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("hello.txt").expect("path"),
            }),
            fixture_time(),
        ))
        .expect("tool succeeds");
    assert!(matches!(result, intention_tools::ToolResult::Read(_)));
    assert_eq!(repository.tool_events.borrow().len(), 3);
}

#[test]
fn local_tool_rejects_storage_before_execution() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository.tool_error.borrow_mut() =
        Some(ErrorDto::unavailable("storage_down", "storage unavailable"));
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
            "read",
            ToolInput::Read(ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("missing").expect("path"),
            }),
            fixture_time(),
        ))
        .expect_err("storage failure is propagated");
    assert_eq!(error.code(), "storage_down");
}

#[test]
fn lifecycle_details_redact_absolute_workspace_root_and_os_error_text() {
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
    let details = repository
        .tool_events
        .borrow()
        .iter()
        .map(|event| event.detail().to_owned())
        .collect::<Vec<_>>();
    let rendered = format!("{error:?} {details:?}");
    assert!(!rendered.contains(&root.to_string_lossy().to_string()));
    assert!(!rendered.contains("No such file or directory"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn lifecycle_events_preserve_exact_correlation_identity_across_terminal_outcome() {
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
            ToolInput::Read(ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("missing").expect("path"),
            }),
            fixture_time(),
        ))
        .expect_err("missing file fails");
    assert_eq!(error.code(), "tool_read_failed");
    let events = repository.tool_events.borrow();
    assert!(events.len() >= 3);
    assert!(events.iter().all(|event| {
        event.session_id() == session_id && event.run_id() == run_id && event.call_id() == call_id
    }));
    assert!(matches!(
        events.last().expect("terminal event").status(),
        intention_domain::ToolLifecycleStatusDto::Failed
    ));
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
            ToolInput::Read(ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("missing").expect("path"),
            }),
            fixture_time(),
        ))
        .expect_err("mismatched tool id is rejected");
    assert_eq!(error.code(), "tool_id_mismatch");
    assert!(repository.tool_events.borrow().is_empty());
}

#[test]
fn local_tool_hook_rejection_is_durable_and_skips_execution() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(RejectHook))
        .expect("hook registers");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(InvokeLocalToolInputDto::new(
            WorkspaceRoot::resolve(
                &WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy())
                    .expect("workspace dto"),
            )
            .expect("workspace"),
            SessionId::new(),
            RunId::new(),
            ToolCallId::new(),
            "read",
            ToolInput::Read(ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("missing").expect("path"),
            }),
            fixture_time(),
        ))
        .expect_err("hook rejects");
    assert_eq!(error.code(), "blocked_by_hook");
    let events = repository.tool_events.borrow();
    assert_eq!(events.len(), 2);
    assert_eq!(
        *events[1].status(),
        intention_domain::ToolLifecycleStatusDto::Rejected
    );
    assert_eq!(events[1].detail(), "blocked_by_hook");
}

#[test]
fn public_dto_constructors_and_schedule_validation_cover_mismatch_paths() {
    let command = SendUserTurnCommandDto::new(SessionId::new(), TurnId::new(), "hello")
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
        // Operational hook failures fail closed and record a durable rejection
        // without starting execution.
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
                outcome: HookOutcome::TransformInput(ToolInput::Read(ReadInput {
                    path: intention_types::WorkspaceRelativePathDto::parse("hello.txt")
                        .expect("path"),
                })),
            }))
            .expect("hook registers");
        let result = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(invoke_read_input_in_workspace(
                &hello_workspace(&root),
                "missing-before-transform.txt",
            ))
            .expect("transformed input is executed");
        assert_eq!(result, hello_read_result());
        let _ = fs::remove_dir_all(root);

        // Result transformations are incompatible before execution: the hook
        // registry fails closed before the invocation starts and the tolerated
        // typed failure is durably recorded as a rejection.
        let root = hello_tool_root("matrix-result");
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let mut hooks = Registry::new();
        hooks
            .register(Box::new(PhaseOutcomeHook {
                phase,
                id: "matrix-result",
                outcome: HookOutcome::TransformResult(intention_tools::ToolResult::Read(
                    intention_tools::TextResult {
                        text: intention_tools::BoundedText::new("changed").expect("text"),
                        truncated: false,
                    },
                )),
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
            outcome: HookOutcome::TransformInput(ToolInput::Read(ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("other.txt").expect("path"),
            })),
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
    let events = repository.tool_events.borrow();
    assert_eq!(
        events.last().expect("terminal event").status(),
        &intention_domain::ToolLifecycleStatusDto::Failed
    );
    drop(events);
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
    let _ = fs::remove_dir_all(root);
}

#[test]
fn post_execution_result_phases_cover_rejection_and_invalid_input() {
    for phase in [
        Phase::BeforeToolResultPersist,
        Phase::BeforeToolResultModelContext,
    ] {
        // Hook rejections after execution durably record the failure without a
        // completed terminal event.
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
        let events = repository.tool_events.borrow();
        assert_eq!(
            events.last().expect("terminal event").status(),
            &intention_domain::ToolLifecycleStatusDto::Failed
        );
        drop(events);
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
                outcome: HookOutcome::TransformInput(ToolInput::Read(ReadInput {
                    path: intention_types::WorkspaceRelativePathDto::parse("other.txt")
                        .expect("path"),
                })),
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
        let events = repository.tool_events.borrow();
        assert_eq!(
            events.last().expect("terminal event").status(),
            &intention_domain::ToolLifecycleStatusDto::Failed
        );
        drop(events);
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn stop_and_snapshot_workflows_map_durable_results() {
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
        5,
    );
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable(
        "fixture_unused",
        "accept is not used by this fixture",
    )));
    *repository.loaded_snapshot.borrow_mut() = Some(state.clone());
    *repository.transitioned.borrow_mut() = Some(change(state.clone(), None));
    let application = ApplicationService::new(&repository);

    let stopped = application
        .stop_run(
            intention_domain::StopRunCommandDto::new(session_id, run_id),
            intention_runtime::RuntimeValuesDto::new(RunId::new(), config, fixture_time()),
        )
        .expect("stop maps");
    assert!(matches!(
        stopped,
        ProtocolAcceptedResultDto::StopRun(value)
            if value.session_id() == session_id && value.run_id() == run_id
    ));

    let snapshot = application
        .get_session_snapshot(GetSessionSnapshotQueryDto::new(session_id))
        .expect("snapshot maps");
    assert_eq!(snapshot.session_id(), session_id);
    assert_eq!(snapshot.projection(), &state);
}

#[test]
fn create_and_remove_workflows_map_committed_results() {
    let session_id = SessionId::new();
    let queued_turn = TurnId::new();
    let repository = FakeRepository::with_accepted(Ok(change(
        projection(session_id, None, Vec::new(), 3),
        Some(AcceptedTurnOutcomeDto::Queued(QueuePositionDto::new(4))),
    )));
    *repository.created.borrow_mut() =
        Some(change(projection(session_id, None, Vec::new(), 1), None));
    *repository.removed.borrow_mut() =
        Some(change(projection(session_id, None, Vec::new(), 4), None));
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
        ProtocolAcceptedResultDto::CreateSession(_)
    ));
    let removed = application
        .remove_queued_turn(
            RemoveQueuedTurnCommandDto::new(session_id, queued_turn),
            fixture_time(),
        )
        .expect("removal maps");
    assert!(matches!(
        removed,
        ProtocolAcceptedResultDto::RemoveQueuedTurn(_)
    ));
}

#[test]
fn local_tool_after_execution_transform_is_applied() {
    let root = std::env::temp_dir().join(format!("intention-app-hooks-{}", SessionId::new()));
    fs::create_dir_all(&root).expect("root");
    fs::write(root.join("hello.txt"), "hello").expect("file");
    let workspace =
        WorkspaceRoot::resolve(&WorkspaceRootDto::parse(root.to_string_lossy()).expect("dto"))
            .expect("workspace");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(PhaseOutcomeHook {
            phase: Phase::AfterToolExecution,
            id: "execution-transform",
            outcome: HookOutcome::TransformResult(intention_tools::ToolResult::Read(
                intention_tools::TextResult {
                    text: intention_tools::BoundedText::new("changed").expect("text"),
                    truncated: false,
                },
            )),
        }))
        .expect("hook");
    let result = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(InvokeLocalToolInputDto::new(
            workspace,
            SessionId::new(),
            RunId::new(),
            ToolCallId::new(),
            "read",
            ToolInput::Read(ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("hello.txt").expect("path"),
            }),
            fixture_time(),
        ))
        .expect("transformed read succeeds");
    assert_eq!(
        result,
        intention_tools::ToolResult::Read(intention_tools::TextResult {
            text: intention_tools::BoundedText::new("changed").expect("text"),
            truncated: false,
        })
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_tool_covers_workspace_reject_and_all_post_execution_outcomes() {
    let root = std::env::temp_dir().join(format!("intention-app-branches-{}", SessionId::new()));
    fs::create_dir_all(&root).expect("root");
    fs::write(root.join("hello.txt"), "hello").expect("file");
    let workspace =
        WorkspaceRoot::resolve(&WorkspaceRootDto::parse(root.to_string_lossy()).expect("dto"))
            .expect("workspace");

    for (phase, outcome, expected) in [
        (
            Phase::BeforeWorkspaceResolution,
            HookOutcome::Reject(ErrorDto::validation("workspace_blocked", "blocked")),
            "workspace_blocked",
        ),
        (
            Phase::AfterWorkspaceResolution,
            HookOutcome::TransformResult(intention_tools::ToolResult::Read(
                intention_tools::TextResult {
                    text: intention_tools::BoundedText::new("x").expect("text"),
                    truncated: false,
                },
            )),
            "invalid_hook_outcome",
        ),
        (
            Phase::AfterToolExecution,
            HookOutcome::Reject(ErrorDto::validation("result_blocked", "blocked")),
            "result_blocked",
        ),
        (
            Phase::BeforeToolResultModelContext,
            HookOutcome::TransformInput(ToolInput::Read(ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("hello.txt").expect("path"),
            })),
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
                ToolInput::Read(ReadInput {
                    path: intention_types::WorkspaceRelativePathDto::parse("hello.txt")
                        .expect("path"),
                }),
                fixture_time(),
            ))
            .expect_err("hook branch rejects");
        assert_eq!(error.code(), expected);
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_tool_covers_dispatch_errors_and_post_effect_result_transforms() {
    let root = std::env::temp_dir().join(format!("intention-app-dispatch-{}", SessionId::new()));
    fs::create_dir_all(&root).expect("root");
    fs::write(root.join("hello.txt"), "hello").expect("file");
    let workspace =
        WorkspaceRoot::resolve(&WorkspaceRootDto::parse(root.to_string_lossy()).expect("dto"))
            .expect("workspace");
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
                ToolInput::Read(ReadInput {
                    path: intention_types::WorkspaceRelativePathDto::parse("hello.txt")
                        .expect("path"),
                }),
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
                outcome: HookOutcome::TransformResult(intention_tools::ToolResult::Read(
                    intention_tools::TextResult {
                        text: intention_tools::BoundedText::new("changed").expect("text"),
                        truncated: false,
                    },
                )),
            }))
            .expect("hook");
        let result = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(InvokeLocalToolInputDto::new(
                workspace.clone(),
                SessionId::new(),
                RunId::new(),
                ToolCallId::new(),
                "read",
                ToolInput::Read(ReadInput {
                    path: intention_types::WorkspaceRelativePathDto::parse("hello.txt")
                        .expect("path"),
                }),
                fixture_time(),
            ))
            .expect("transformed result");
        assert_eq!(
            result,
            intention_tools::ToolResult::Read(intention_tools::TextResult {
                text: intention_tools::BoundedText::new("changed").expect("text"),
                truncated: false,
            })
        );
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
            ToolInput::Read(ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("hello.txt").expect("path"),
            }),
            fixture_time(),
        ))
        .expect("published Continue is valid");
    assert!(matches!(result, intention_tools::ToolResult::Read(_)));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_tool_covers_invocation_and_pre_effect_hook_errors_and_rejections() {
    let root = std::env::temp_dir().join(format!("intention-app-pre-hooks-{}", SessionId::new()));
    fs::create_dir_all(&root).expect("root");
    fs::write(root.join("hello.txt"), "hello").expect("file");
    let workspace = WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy()).expect("workspace"),
    )
    .expect("workspace is valid");
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
fn local_tool_records_external_effect_unknown_terminal_status() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    // The cancellation is observed after the child has been spawned, so the
    // tool cannot know whether the external process produced an effect.
    let signal = intention_tools::CancellationSignal::new();
    let cancellation = signal.clone();
    let canceller = std::thread::spawn(move || {
        // Wait for a confirmed child spawn instead of racing a fixed sleep:
        // the cancellation then provably lands while the external process is
        // running, preserving the unknown-effect classification everywhere.
        assert!(
            cancellation.wait_until_spawn_observed(std::time::Duration::from_secs(10)),
            "execute child was never observed after spawn"
        );
        cancellation.cancel();
    });
    let error = ApplicationService::new(&repository)
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
                ToolInput::Execute(intention_tools::ExecuteInput {
                    program: intention_tools::BoundedText::new(if cfg!(windows) {
                        "ping"
                    } else {
                        "sh"
                    })
                    .expect("program"),
                    args: vec![
                        intention_tools::BoundedText::new(if cfg!(windows) { "-n" } else { "-c" })
                            .expect("arg"),
                        intention_tools::BoundedText::new(if cfg!(windows) {
                            "2"
                        } else {
                            "sleep 1"
                        })
                        .expect("arg"),
                        #[cfg(windows)]
                        intention_tools::BoundedText::new("127.0.0.1").expect("arg"),
                    ],
                }),
                fixture_time(),
            )
            .with_cancellation(signal),
        )
        .expect_err("external effect is unknown");
    canceller.join().expect("cancellation helper completes");
    assert_eq!(error.code(), "tool_execute_external_effect_unknown");
    assert!(repository.tool_events.borrow().iter().any(|event| matches!(
        event.status(),
        intention_domain::ToolLifecycleStatusDto::ExternalEffectUnknown
    )));
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
    let error = ApplicationService::new(&repository)
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
                ToolInput::Execute(intention_tools::ExecuteInput {
                    program: intention_tools::BoundedText::new("sh").expect("program"),
                    args: vec![
                        intention_tools::BoundedText::new("-c").expect("arg"),
                        intention_tools::BoundedText::new("sleep 1").expect("arg"),
                    ],
                }),
                fixture_time(),
            )
            .with_cancellation(intention_tools::CancellationSignal::cancelled()),
        )
        .expect_err("cancelled invocation fails safely");
    assert_eq!(error.code(), "tool_cancelled");

    let events = repository.tool_events.borrow();
    assert!(
        !events.is_empty(),
        "admission is durable before cancellation"
    );
    assert!(events.iter().all(|event| event.session_id() == session_id));
    assert!(events.iter().all(|event| event.run_id() == run_id));
    assert!(events.iter().all(|event| event.call_id() == call_id));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event.status(),
                intention_domain::ToolLifecycleStatusDto::Completed
            ))
            .count(),
        0,
        "cancellation cannot produce a duplicate completion"
    );
    assert!(events.iter().any(|event| matches!(
        event.status(),
        intention_domain::ToolLifecycleStatusDto::Cancelled
            | intention_domain::ToolLifecycleStatusDto::ExternalEffectUnknown
            | intention_domain::ToolLifecycleStatusDto::Failed
    )));
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
    let root = std::env::temp_dir().join(format!("intention-app-failopen-{}", SessionId::new()));
    fs::create_dir_all(&root).expect("root");
    fs::write(root.join("hello.txt"), "hello").expect("file");
    let workspace =
        WorkspaceRoot::resolve(&WorkspaceRootDto::parse(root.to_string_lossy()).expect("dto"))
            .expect("workspace");
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
            InvokeLocalToolInputDto::new(
                workspace,
                SessionId::new(),
                RunId::new(),
                ToolCallId::new(),
                "read",
                ToolInput::Read(ReadInput {
                    path: intention_types::WorkspaceRelativePathDto::parse("hello.txt")
                        .expect("path"),
                }),
                fixture_time(),
            ),
            &observer,
        )
        .expect("fail-open failures continue execution");
    assert!(matches!(result, intention_tools::ToolResult::Read(_)));

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
    // Observations stay redacted: hook payloads, error codes, secrets, and
    // local filesystem details never cross the application boundary.
    let rendered = format!("{observations:?}");
    assert!(!rendered.contains("FAKE_SECRET"));
    assert!(!rendered.contains(&root.to_string_lossy().to_string()));
    assert!(!rendered.contains("fail_open_failure"));

    let events = repository.tool_events.borrow();
    assert_eq!(events.len(), 3);
    assert!(matches!(
        events[2].status(),
        intention_domain::ToolLifecycleStatusDto::Completed
    ));
    let _ = fs::remove_dir_all(root);
}

struct CapturingPublisher {
    publications: RefCell<Vec<ToolResultPublicationInputDto>>,
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

    fn published(&self) -> Vec<ToolResultPublicationInputDto> {
        self.publications.borrow().clone()
    }
}

impl ToolResultPublicationPort for CapturingPublisher {
    fn publish_tool_result(&self, input: &ToolResultPublicationInputDto) -> DtoResult<()> {
        self.publications.borrow_mut().push(input.clone());
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

fn hello_read_result() -> intention_tools::ToolResult {
    intention_tools::ToolResult::Read(intention_tools::TextResult {
        text: intention_tools::BoundedText::new("hello").expect("text"),
        truncated: false,
    })
}

fn completed_terminal_event_count(repository: &FakeRepository) -> usize {
    repository
        .tool_events
        .borrow()
        .iter()
        .filter(|event| {
            matches!(
                event.status(),
                intention_domain::ToolLifecycleStatusDto::Completed
            )
        })
        .count()
}

#[test]
fn publication_failure_propagates_after_the_durable_completed_commit() {
    let root = hello_tool_root("failure");
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

    // The committed result reached the boundary exactly once even though the
    // caller sees the publication error.
    assert_eq!(publisher.published().len(), 1);
    // Terminal completion stays durable; no extra failure event is appended.
    let events = repository.tool_events.borrow();
    assert_eq!(events.len(), 3);
    assert_eq!(
        events.last().expect("terminal event").status(),
        &intention_domain::ToolLifecycleStatusDto::Completed
    );
    drop(events);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn after_publish_hook_rejection_surfaces_after_the_completed_commit() {
    let root = hello_tool_root("reject");
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
    // Publication already happened before the post-publish hook ran.
    assert_eq!(publisher.published().len(), 1);
    // The completed commit is preserved and not duplicated as a failure.
    let events = repository.tool_events.borrow();
    assert_eq!(events.len(), 3);
    assert_eq!(
        events.last().expect("terminal event").status(),
        &intention_domain::ToolLifecycleStatusDto::Completed
    );
    drop(events);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn after_publish_transform_outcomes_are_invalidated_without_extra_failures() {
    // Result transformations reach the application boundary and are refused
    // after publication; input transformations are already refused by the hook
    // registry as incompatible with the published phase. Both stay fail-closed
    // without appending any post-completion lifecycle record.
    let outcomes = [
        (
            HookOutcome::TransformResult(intention_tools::ToolResult::Read(
                intention_tools::TextResult {
                    text: intention_tools::BoundedText::new("changed").expect("text"),
                    truncated: false,
                },
            )),
            "published result cannot be transformed",
        ),
        (
            HookOutcome::TransformInput(ToolInput::Read(ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("elsewhere").expect("path"),
            })),
            "input transformation is incompatible with its phase",
        ),
    ];
    for (outcome, expected_message) in outcomes {
        let root = hello_tool_root("transform");
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
        // Completion stays durable and exactly one terminal record exists.
        let events = repository.tool_events.borrow();
        assert_eq!(events.len(), 3);
        assert_eq!(
            events.last().expect("terminal event").status(),
            &intention_domain::ToolLifecycleStatusDto::Completed
        );
        drop(events);
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn after_publish_hook_error_fails_closed_on_the_completed_commit() {
    let root = hello_tool_root("dispatch-error");
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
    let events = repository.tool_events.borrow();
    assert_eq!(events.len(), 3);
    assert_eq!(
        events.last().expect("terminal event").status(),
        &intention_domain::ToolLifecycleStatusDto::Completed
    );
    drop(events);
    assert_eq!(completed_terminal_event_count(&repository), 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn fail_open_failures_in_the_published_phase_reach_the_observer() {
    let root = hello_tool_root("observation");
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
    let events = repository.tool_events.borrow();
    assert_eq!(events.len(), 3);
    assert_eq!(
        events.last().expect("terminal event").status(),
        &intention_domain::ToolLifecycleStatusDto::Completed
    );
    drop(events);
    let _ = fs::remove_dir_all(root);
}

enum MatrixHook {
    None,
    DispatchError(Phase),
    Reject(Phase),
}

#[test]
fn selected_append_failures_propagate_from_each_lifecycle_commit_point() {
    let scenarios: Vec<(&str, usize, MatrixHook, usize)> = vec![
        (
            "invocation-dispatch-error",
            2,
            MatrixHook::DispatchError(Phase::BeforeToolInvocation),
            1,
        ),
        (
            "invocation-rejection",
            2,
            MatrixHook::Reject(Phase::BeforeToolInvocation),
            1,
        ),
        (
            "workspace-resolution-dispatch-error",
            2,
            MatrixHook::DispatchError(Phase::BeforeWorkspaceResolution),
            1,
        ),
        (
            "workspace-resolution-rejection",
            2,
            MatrixHook::Reject(Phase::BeforeWorkspaceResolution),
            1,
        ),
        (
            "workspace-resolved-dispatch-error",
            2,
            MatrixHook::DispatchError(Phase::AfterWorkspaceResolution),
            1,
        ),
        (
            "workspace-resolved-rejection",
            2,
            MatrixHook::Reject(Phase::AfterWorkspaceResolution),
            1,
        ),
        ("started-commit", 2, MatrixHook::None, 1),
        (
            "executed-dispatch-error",
            3,
            MatrixHook::DispatchError(Phase::AfterToolExecution),
            2,
        ),
        (
            "persist-dispatch-error",
            3,
            MatrixHook::DispatchError(Phase::BeforeToolResultPersist),
            2,
        ),
        (
            "model-context-dispatch-error",
            3,
            MatrixHook::DispatchError(Phase::BeforeToolResultModelContext),
            2,
        ),
        (
            "persist-rejection",
            3,
            MatrixHook::Reject(Phase::BeforeToolResultPersist),
            2,
        ),
        (
            "model-context-rejection",
            3,
            MatrixHook::Reject(Phase::BeforeToolResultModelContext),
            2,
        ),
        ("completed-commit", 3, MatrixHook::None, 2),
    ];
    for (label, failing_call, hook, expected_events) in scenarios {
        let root = hello_tool_root("append-failure");
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        *repository.append_failures.borrow_mut() = vec![failing_call];
        let mut hooks = Registry::new();
        match hook {
            MatrixHook::None => {}
            MatrixHook::DispatchError(phase) => {
                hooks
                    .register(Box::new(DispatchErrorHook { phase }))
                    .expect("hook registers");
            }
            MatrixHook::Reject(phase) => {
                hooks
                    .register(Box::new(PhaseOutcomeHook {
                        phase,
                        id: "append-failure-reject",
                        outcome: HookOutcome::Reject(ErrorDto::validation(
                            "append_scenario_blocked",
                            "blocked",
                        )),
                    }))
                    .expect("hook registers");
            }
        }
        let error = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(invoke_read_input_in_workspace(
                &hello_workspace(&root),
                "hello.txt",
            ))
            .expect_err("the selected append failure must propagate");
        assert_eq!(error.code(), "append_unavailable", "scenario {label}");
        assert_eq!(
            repository.tool_events.borrow().len(),
            expected_events,
            "scenario {label}"
        );
        let _ = fs::remove_dir_all(root);
    }
}

/// Publication probe that records the durable evidence length at publish time.
struct TerminalOrderingProbe<'a> {
    repository: &'a FakeRepository,
    publications: RefCell<Vec<ToolResultPublicationInputDto>>,
    evidence_at_publish: RefCell<Vec<usize>>,
}

impl<'a> TerminalOrderingProbe<'a> {
    const fn new(repository: &'a FakeRepository) -> Self {
        Self {
            repository,
            publications: RefCell::new(Vec::new()),
            evidence_at_publish: RefCell::new(Vec::new()),
        }
    }
}

impl ToolResultPublicationPort for TerminalOrderingProbe<'_> {
    fn publish_tool_result(&self, input: &ToolResultPublicationInputDto) -> DtoResult<()> {
        self.evidence_at_publish
            .borrow_mut()
            .push(self.repository.tool_events.borrow().len());
        self.publications.borrow_mut().push(input.clone());
        Ok(())
    }
}

/// Asserts exactly one terminal event exists, is last, and correlates exactly.
fn assert_single_terminal_event(
    repository: &FakeRepository,
    session_id: SessionId,
    run_id: RunId,
    call_id: ToolCallId,
    status: &intention_domain::ToolLifecycleStatusDto,
) {
    let events = repository.tool_events.borrow();
    let terminal = events
        .iter()
        .filter(|event| {
            matches!(
                event.status(),
                intention_domain::ToolLifecycleStatusDto::Completed
                    | intention_domain::ToolLifecycleStatusDto::Failed
                    | intention_domain::ToolLifecycleStatusDto::Cancelled
                    | intention_domain::ToolLifecycleStatusDto::ExternalEffectUnknown
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(terminal.len(), 1, "exactly one terminal event is persisted");
    let terminal = terminal[0];
    assert_eq!(events.last(), Some(terminal), "terminal evidence is last");
    assert_eq!(terminal.status(), status);
    assert_eq!(terminal.session_id(), session_id);
    assert_eq!(terminal.run_id(), run_id);
    assert_eq!(terminal.call_id(), call_id);
}

#[test]
fn every_terminal_outcome_persists_one_correlated_event_before_publication() {
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
                ToolInput::Read(ReadInput {
                    path: intention_types::WorkspaceRelativePathDto::parse("hello.txt")
                        .expect("path"),
                }),
                fixture_time(),
            ),
            &publisher,
        )
        .expect("read succeeds");
    assert_eq!(result, hello_read_result());
    assert_single_terminal_event(
        &repository,
        session_id,
        run_id,
        call_id,
        &intention_domain::ToolLifecycleStatusDto::Completed,
    );
    // Three events are durable (admitted, started, completed) when publication
    // runs, proving the terminal commit precedes the publication boundary.
    assert_eq!(*publisher.evidence_at_publish.borrow(), vec![3]);
    let publications = publisher.publications.borrow();
    assert_eq!(publications.len(), 1);
    assert_eq!(publications[0].session_id(), session_id);
    assert_eq!(publications[0].run_id(), run_id);
    assert_eq!(publications[0].call_id(), call_id);
    assert_eq!(publications[0].result(), &hello_read_result());
    drop(publications);
    drop(publisher);
    drop(repository);

    // A failed outcome persists correlated Failed evidence and never publishes.
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
                ToolInput::Read(ReadInput {
                    path: intention_types::WorkspaceRelativePathDto::parse("missing.txt")
                        .expect("path"),
                }),
                fixture_time(),
            ),
            &publisher,
        )
        .expect_err("missing file fails");
    assert_eq!(error.code(), "tool_read_failed");
    assert_single_terminal_event(
        &repository,
        session_id,
        run_id,
        call_id,
        &intention_domain::ToolLifecycleStatusDto::Failed,
    );
    assert!(publisher.publications.borrow().is_empty());
    assert!(publisher.evidence_at_publish.borrow().is_empty());
    drop(publisher);
    drop(repository);

    // A cancelled outcome persists correlated Cancelled evidence and never
    // reaches the publication boundary.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let error = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            InvokeLocalToolInputDto::new(
                hello_workspace(&root),
                session_id,
                run_id,
                call_id,
                "execute",
                ToolInput::Execute(intention_tools::ExecuteInput {
                    program: intention_tools::BoundedText::new("sh").expect("program"),
                    args: vec![],
                }),
                fixture_time(),
            )
            .with_cancellation(intention_tools::CancellationSignal::cancelled()),
            &publisher,
        )
        .expect_err("cancelled invocation fails");
    assert_eq!(error.code(), "tool_cancelled");
    assert_single_terminal_event(
        &repository,
        session_id,
        run_id,
        call_id,
        &intention_domain::ToolLifecycleStatusDto::Cancelled,
    );
    assert!(publisher.publications.borrow().is_empty());
    assert!(publisher.evidence_at_publish.borrow().is_empty());
    drop(publisher);
    drop(repository);

    // An unknown external-effect outcome persists correlated evidence that
    // never reaches the publication boundary either.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let signal = intention_tools::CancellationSignal::new();
    let cancellation = signal.clone();
    let canceller = std::thread::spawn(move || {
        // Wait for a confirmed child spawn instead of racing a fixed sleep:
        // the cancellation then provably lands while the external process is
        // running, preserving the unknown-effect classification everywhere.
        assert!(
            cancellation.wait_until_spawn_observed(std::time::Duration::from_secs(10)),
            "execute child was never observed after spawn"
        );
        cancellation.cancel();
    });
    let error = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            InvokeLocalToolInputDto::new(
                hello_workspace(&root),
                session_id,
                run_id,
                call_id,
                "execute",
                ToolInput::Execute(intention_tools::ExecuteInput {
                    program: intention_tools::BoundedText::new(if cfg!(windows) {
                        "ping"
                    } else {
                        "sh"
                    })
                    .expect("program"),
                    args: vec![
                        intention_tools::BoundedText::new(if cfg!(windows) { "-n" } else { "-c" })
                            .expect("arg"),
                        intention_tools::BoundedText::new(if cfg!(windows) {
                            "2"
                        } else {
                            "sleep 1"
                        })
                        .expect("arg"),
                        #[cfg(windows)]
                        intention_tools::BoundedText::new("127.0.0.1").expect("arg"),
                    ],
                }),
                fixture_time(),
            )
            .with_cancellation(signal),
            &publisher,
        )
        .expect_err("external effect is unknown");
    canceller.join().expect("cancellation helper completes");
    assert_eq!(error.code(), "tool_execute_external_effect_unknown");
    assert_single_terminal_event(
        &repository,
        session_id,
        run_id,
        call_id,
        &intention_domain::ToolLifecycleStatusDto::ExternalEffectUnknown,
    );
    assert!(publisher.publications.borrow().is_empty());
    assert!(publisher.evidence_at_publish.borrow().is_empty());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn terminal_commits_carry_typed_result_evidence_before_publication() {
    let root = hello_tool_root("evidence");
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();

    // Success: the terminal Completed commit atomically carries the typed
    // result document with the exact invocation identity.
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
                ToolInput::Read(ReadInput {
                    path: intention_types::WorkspaceRelativePathDto::parse("hello.txt")
                        .expect("path"),
                }),
                fixture_time(),
            ),
            &publisher,
        )
        .expect("read succeeds");
    let evidence = repository.result_evidence.borrow();
    assert_eq!(evidence.len(), 3);
    assert!(evidence[..2].iter().all(Option::is_none));
    let completed = evidence[2]
        .as_ref()
        .expect("terminal evidence commits atomically");
    assert_eq!(completed.session_id(), session_id);
    assert_eq!(completed.run_id(), run_id);
    assert_eq!(completed.call_id(), call_id);
    assert_eq!(completed.kind(), ToolResultKindDto::Read);
    assert_eq!(
        completed.content(),
        "{\"result\":\"read\",\"value\":{\"text\":\"hello\",\"truncated\":false}}"
    );
    assert_eq!(completed.occurred_at(), fixture_time());
    drop(evidence);
    // The evidence-carrying terminal commit is durable before publication.
    assert_eq!(*publisher.evidence_at_publish.borrow(), vec![3]);
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
                ToolInput::Read(ReadInput {
                    path: intention_types::WorkspaceRelativePathDto::parse("missing.txt")
                        .expect("path"),
                }),
                fixture_time(),
            ),
            &publisher,
        )
        .expect_err("missing file fails");
    assert_eq!(error.code(), "tool_read_failed");
    let evidence = repository.result_evidence.borrow();
    assert!(evidence[..2].iter().all(Option::is_none));
    let failed = evidence[2]
        .as_ref()
        .expect("failed evidence commits atomically");
    assert_eq!(failed.kind(), ToolResultKindDto::Read);
    assert_eq!(
        failed.content(),
        "{\"result\":\"failed\",\"value\":{\"code\":\"tool_read_failed\"}}"
    );
    drop(evidence);
    assert!(publisher.publications.borrow().is_empty());
    drop(publisher);
    drop(repository);

    // Cancellation: the terminal Cancelled commit classifies the cancellation.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let error = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            InvokeLocalToolInputDto::new(
                hello_workspace(&root),
                session_id,
                run_id,
                call_id,
                "execute",
                ToolInput::Execute(intention_tools::ExecuteInput {
                    program: intention_tools::BoundedText::new("sh").expect("program"),
                    args: vec![],
                }),
                fixture_time(),
            )
            .with_cancellation(intention_tools::CancellationSignal::cancelled()),
            &publisher,
        )
        .expect_err("cancelled invocation fails");
    assert_eq!(error.code(), "tool_cancelled");
    let evidence = repository.result_evidence.borrow();
    let cancelled = evidence[2]
        .as_ref()
        .expect("cancelled evidence commits atomically");
    assert_eq!(cancelled.kind(), ToolResultKindDto::Execute);
    assert_eq!(
        cancelled.content(),
        "{\"result\":\"cancelled\",\"value\":{\"code\":\"tool_cancelled\"}}"
    );
    drop(evidence);
    assert!(publisher.publications.borrow().is_empty());
    drop(publisher);
    drop(repository);

    // Unknown external effect: the terminal commit classifies the uncertainty.
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let publisher = TerminalOrderingProbe::new(&repository);
    let signal = intention_tools::CancellationSignal::new();
    let cancellation = signal.clone();
    let canceller = std::thread::spawn(move || {
        // Wait for a confirmed child spawn instead of racing a fixed sleep:
        // the cancellation then provably lands while the external process is
        // running, preserving the unknown-effect classification everywhere.
        assert!(
            cancellation.wait_until_spawn_observed(std::time::Duration::from_secs(10)),
            "execute child was never observed after spawn"
        );
        cancellation.cancel();
    });
    let error = ApplicationService::new(&repository)
        .invoke_local_tool_with_publication(
            InvokeLocalToolInputDto::new(
                hello_workspace(&root),
                session_id,
                run_id,
                call_id,
                "execute",
                ToolInput::Execute(intention_tools::ExecuteInput {
                    program: intention_tools::BoundedText::new(if cfg!(windows) {
                        "ping"
                    } else {
                        "sh"
                    })
                    .expect("program"),
                    args: vec![
                        intention_tools::BoundedText::new(if cfg!(windows) { "-n" } else { "-c" })
                            .expect("arg"),
                        intention_tools::BoundedText::new(if cfg!(windows) {
                            "2"
                        } else {
                            "sleep 1"
                        })
                        .expect("arg"),
                        #[cfg(windows)]
                        intention_tools::BoundedText::new("127.0.0.1").expect("arg"),
                    ],
                }),
                fixture_time(),
            )
            .with_cancellation(signal),
            &publisher,
        )
        .expect_err("external effect is unknown");
    canceller.join().expect("cancellation helper completes");
    assert_eq!(error.code(), "tool_execute_external_effect_unknown");
    let evidence = repository.result_evidence.borrow();
    let unknown = evidence[2]
        .as_ref()
        .expect("unknown-effect evidence commits atomically");
    assert_eq!(unknown.kind(), ToolResultKindDto::Execute);
    assert_eq!(
        unknown.content(),
        "{\"result\":\"external_effect_unknown\",\"value\":{\"code\":\"tool_execute_external_effect_unknown\"}}"
    );
    drop(evidence);
    assert!(publisher.publications.borrow().is_empty());

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
    fn resolve(&self, _: &WorkspaceRoot) -> DtoResult<()> {
        Err(ErrorDto::unavailable(
            "workspace_boundary_unavailable",
            "workspace boundary refused the invocation",
        ))
    }
}

fn send_command(session_id: SessionId, turn_id: TurnId) -> SendUserTurnCommandDto {
    SendUserTurnCommandDto::new(session_id, turn_id, "latest").expect("fixture command is valid")
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
            ModelContextMessageDto::new(ModelContextRoleDto::User, "first")
                .expect("context message is valid"),
            ModelContextMessageDto::new(ModelContextRoleDto::Assistant, "answer")
                .expect("context message is valid"),
            ModelContextMessageDto::new(ModelContextRoleDto::User, "latest")
                .expect("context message is valid"),
        ],
    )
    .expect("fixture context is valid")
}

fn started_change(
    session_id: SessionId,
    run: RunProjectionDto,
    position: u64,
) -> CommittedChangeDto {
    let state = projection(session_id, Some(run), Vec::new(), position);
    let run_started = EventEnvelopeDto::new(
        EventMetadataDto::new(
            SchemaVersionDto::new(1, 0),
            EventId::new(),
            session_id,
            Some(run.run_id()),
            Some(run.turn_id()),
            SessionEventSequenceDto::new(position),
            fixture_time(),
        ),
        DomainEventDto::RunStarted(RunStartedEventDto::new(
            session_id,
            run.run_id(),
            run.turn_id(),
            run.config_revision_id(),
            fixture_time(),
        )),
    );
    CommittedChangeDto::new(
        state.clone(),
        state.at_sequence(),
        vec![run_started],
        Some(AcceptedTurnOutcomeDto::Started(run)),
    )
    .expect("fixture started change is valid")
}

#[test]
fn send_user_turn_and_schedule_rejects_acceptance_without_a_turn_outcome() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Ok(change(
        projection(session_id, None, Vec::new(), 3),
        None,
    )));
    let dispatch = RecordingDispatchPort::default();
    let error = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            send_command(session_id, TurnId::new()),
            SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), fixture_time()),
            &dispatch,
        )
        .expect_err("acceptance without durable outcome evidence is malformed");
    assert_eq!(error.code(), "missing_accepted_turn_outcome");
    assert!(dispatch.inputs.borrow().is_empty());
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
            send_command(session_id, TurnId::new()),
            SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), fixture_time()),
            &dispatch,
        )
        .expect_err("admission failure is propagated");
    assert_eq!(error.code(), "turn_admission_denied");
    assert_eq!(repository.accepted_inputs.borrow().len(), 1);
    assert!(dispatch.inputs.borrow().is_empty());
}

#[test]
fn send_user_turn_and_schedule_returns_queued_acceptance_without_dispatching() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Ok(change(
        projection(session_id, None, Vec::new(), 3),
        Some(AcceptedTurnOutcomeDto::Queued(QueuePositionDto::new(4))),
    )));
    let dispatch = RecordingDispatchPort::default();
    let accepted = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            send_command(session_id, TurnId::new()),
            SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), fixture_time()),
            &dispatch,
        )
        .expect("queued acceptance is returned unchanged");
    assert!(matches!(
        accepted,
        ProtocolAcceptedResultDto::SendUserTurn(value)
            if value.outcome()
                == SendUserTurnOutcomeDto::Queued { queue_position: QueuePositionDto::new(4) }
    ));
    assert_eq!(repository.accepted_inputs.borrow().len(), 1);
    assert!(dispatch.inputs.borrow().is_empty());
}

#[test]
fn send_user_turn_and_schedule_dispatches_the_committed_starting_run() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let config = snapshot();
    let run = starting_run(session_id, run_id, turn_id, &config);
    let repository = FakeRepository::with_accepted(Ok(started_change(session_id, run, 2)));
    *repository.starting_context.borrow_mut() = Some(starting_context(session_id, run_id, &config));
    let dispatch = RecordingDispatchPort::default();
    let accepted = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            send_command(session_id, turn_id),
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
}

#[test]
fn send_user_turn_and_schedule_requires_the_started_run_event_in_the_commit() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let config = snapshot();
    let run = starting_run(session_id, run_id, turn_id, &config);
    let repository = FakeRepository::with_accepted(Ok(change(
        projection(session_id, Some(run), Vec::new(), 3),
        Some(AcceptedTurnOutcomeDto::Started(run)),
    )));
    let dispatch = RecordingDispatchPort::default();
    let accepted = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            send_command(session_id, turn_id),
            SendUserTurnWorkflowInputDto::new(run_id, config.clone(), fixture_time()),
            &dispatch,
        )
        .expect("uncommitted run evidence preserves the acceptance");
    assert!(matches!(
        accepted,
        ProtocolAcceptedResultDto::SendUserTurn(value)
            if value.outcome()
                == SendUserTurnOutcomeDto::Started {
                    run_id,
                    config_revision_id: config.revision_id(),
                }
    ));
    assert!(dispatch.inputs.borrow().is_empty());
}

#[test]
fn send_user_turn_and_schedule_preserves_acceptance_when_context_is_unusable() {
    for mismatched in [false, true] {
        let session_id = SessionId::new();
        let turn_id = TurnId::new();
        let run_id = RunId::new();
        let config = snapshot();
        let run = starting_run(session_id, run_id, turn_id, &config);
        let repository = FakeRepository::with_accepted(Ok(started_change(session_id, run, 2)));
        if mismatched {
            *repository.starting_context.borrow_mut() =
                Some(starting_context(SessionId::new(), RunId::new(), &config));
        }
        let dispatch = RecordingDispatchPort::default();
        let accepted = ApplicationService::new(&repository)
            .send_user_turn_and_schedule(
                send_command(session_id, turn_id),
                SendUserTurnWorkflowInputDto::new(run_id, config, fixture_time()),
                &dispatch,
            )
            .expect("post-commit context failure preserves the acceptance");
        assert!(matches!(
            accepted,
            ProtocolAcceptedResultDto::SendUserTurn(_)
        ));
        assert!(dispatch.inputs.borrow().is_empty());
    }
}

#[test]
fn send_user_turn_and_schedule_preserves_acceptance_when_dispatch_fails() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let config = snapshot();
    let run = starting_run(session_id, run_id, turn_id, &config);
    let repository = FakeRepository::with_accepted(Ok(started_change(session_id, run, 2)));
    *repository.starting_context.borrow_mut() = Some(starting_context(session_id, run_id, &config));
    let dispatch = RecordingDispatchPort::default();
    *dispatch.failure.borrow_mut() = Some(ErrorDto::unavailable(
        "dispatch_unavailable",
        "the daemon refused the scheduled run",
    ));
    let accepted = ApplicationService::new(&repository)
        .send_user_turn_and_schedule(
            send_command(session_id, turn_id),
            SendUserTurnWorkflowInputDto::new(run_id, config, fixture_time()),
            &dispatch,
        )
        .expect("post-commit dispatch failure preserves the acceptance");
    assert!(matches!(
        accepted,
        ProtocolAcceptedResultDto::SendUserTurn(_)
    ));
    assert_eq!(dispatch.inputs.borrow().len(), 1);
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
    let events = repository.tool_events.borrow();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[1].status(),
        &intention_domain::ToolLifecycleStatusDto::Rejected
    );
    assert_eq!(events[1].detail(), "workspace_boundary_unavailable");
}

#[test]
fn terminal_append_failure_propagates_from_the_tool_error_path() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository.append_failures.borrow_mut() = vec![3];
    let error = ApplicationService::new(&repository)
        .invoke_local_tool(invoke_read_input("missing"))
        .expect_err("terminal append failure replaces the tool error");
    assert_eq!(error.code(), "append_unavailable");
    assert_eq!(repository.tool_events.borrow().len(), 2);
}

#[test]
fn pre_execution_rejection_append_failure_propagates() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository.append_failures.borrow_mut() = vec![2];
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(PhaseOutcomeHook {
            phase: Phase::BeforeToolExecution,
            id: "rejection-append-failure",
            outcome: HookOutcome::Reject(ErrorDto::validation(
                "execution_blocked",
                "blocked before execution",
            )),
        }))
        .expect("hook registers");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(invoke_read_input("missing"))
        .expect_err("rejection append failure is propagated");
    assert_eq!(error.code(), "append_unavailable");
    assert_eq!(repository.tool_events.borrow().len(), 1);
}

#[test]
fn lifecycle_evidence_escapes_json_control_characters_in_tool_text() {
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
    assert!(matches!(result, intention_tools::ToolResult::Read(_)));
    let evidence = repository.result_evidence.borrow();
    let completed = evidence
        .last()
        .expect("terminal evidence exists")
        .as_ref()
        .expect("terminal evidence carries the result document");
    assert_eq!(
        completed.content(),
        "{\"result\":\"read\",\"value\":{\"text\":\"\\b\\t\\f\\r\",\"truncated\":false}}"
    );
    drop(evidence);
    let _ = fs::remove_dir_all(root);
}
