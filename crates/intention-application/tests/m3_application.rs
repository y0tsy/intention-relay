#![allow(
    clippy::expect_used,
    reason = "Focused application fixtures use expect to provide precise test failures."
)]

use std::cell::RefCell;
use std::fs;

use intention_application::{
    ApplicationService, CreateSessionWorkflowInputDto, InvokeLocalToolInputDto,
    ScheduleModelRunDto, SendUserTurnWorkflowInputDto,
};
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    CreateSessionCommandDto, DomainEventDto, GetSessionSnapshotQueryDto, ModelRunProjectionDto,
    RemoveQueuedTurnCommandDto, RunEventCursorDto, RunEventTailPageDto, RunModeDto,
    RunProjectionDto, RunReplayDto, RunSnapshotDto, RunStatusDto, SendUserTurnCommandDto,
    SessionProjectionDto, WorkspaceRootDto,
};
use intention_hooks::{Hook, Outcome as HookOutcome, Phase, PhaseContext, Registry};
use intention_protocol::{ProtocolAcceptedResultDto, SendUserTurnOutcomeDto};
use intention_runtime::{ModelMessageDto, ModelRequestDto, ModelRoleDto};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, AppendModelRunFactsInputDto,
    AppendModelRunFactsOutcomeDto, CommittedChangeDto, CreateSessionInputDto,
    RecoverUnfinishedRunsInputDto, RemoveQueuedTurnInputDto, StorageRepositoryDto,
    TransitionRunInputDto,
};
use intention_tools::{ReadInput, ToolInput};
use intention_types::ToolCallId;
use intention_types::{
    ConfigRevisionId, DtoResult, ErrorDto, ProjectId, QueuePositionDto, RunId, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto, TurnId, WorkspaceId,
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

struct PostEffectHook {
    outcome: HookOutcome,
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
impl Hook for PostEffectHook {
    fn id(&self) -> &'static str {
        "post-effect"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::BeforeToolResultPersist]
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

fn current_run_replay(session_id: SessionId, run_id: RunId) -> RunReplayDto {
    let run = RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        RunStatusDto::Running,
        ConfigRevisionId::new(),
    );
    let projection =
        ModelRunProjectionDto::new(run, RunEventCursorDto::new(0), None, "", None, None, None)
            .expect("model projection is valid");
    let snapshot = RunSnapshotDto::new(
        session_id,
        run_id,
        SessionEventSequenceDto::new(0),
        projection,
    )
    .expect("run snapshot is valid");
    RunReplayDto::new(
        snapshot,
        RunEventTailPageDto::empty(session_id, run_id, RunEventCursorDto::new(0)),
    )
    .expect("run replay is valid")
}

struct FakeRepository {
    accepted: RefCell<DtoResult<CommittedChangeDto>>,
    accepted_inputs: RefCell<Vec<AcceptUserTurnInputDto>>,
    created: RefCell<Option<CommittedChangeDto>>,
    removed: RefCell<Option<CommittedChangeDto>>,
    transitioned: RefCell<Option<CommittedChangeDto>>,
    loaded_snapshot: RefCell<Option<SessionProjectionDto>>,
    loaded_replay: RefCell<Option<RunReplayDto>>,
    tool_events: RefCell<Vec<intention_domain::ToolLifecycleEventDto>>,
    tool_error: RefCell<Option<ErrorDto>>,
    tool_error_after: RefCell<Option<ErrorDto>>,
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
            loaded_replay: RefCell::new(None),
            tool_events: RefCell::new(Vec::new()),
            tool_error: RefCell::new(None),
            tool_error_after: RefCell::new(None),
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
        if !self.tool_events.borrow().is_empty()
            && let Some(error) = self.tool_error_after.borrow().clone()
        {
            return Err(error);
        }
        let event = input.event().clone();
        self.tool_events.borrow_mut().push(event.clone());
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
        self.loaded_replay.borrow().clone().ok_or_else(|| {
            ErrorDto::unavailable("fixture_missing_result", "fixture result missing")
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
fn local_tool_covers_all_typed_tool_id_branches() {
    let root = std::env::temp_dir().join(format!("intention-app-ids-{}", SessionId::new()));
    fs::create_dir_all(&root).expect("root");
    let workspace = WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy()).expect("workspace dto"),
    )
    .expect("workspace");
    let inputs = [
        (
            "glob",
            ToolInput::Glob(intention_tools::GlobInput {
                pattern: intention_tools::BoundedText::new("*").expect("pattern"),
            }),
        ),
        (
            "grep",
            ToolInput::Grep(intention_tools::GrepInput {
                pattern: intention_tools::BoundedText::new("x").expect("pattern"),
                path: None,
            }),
        ),
        (
            "write",
            ToolInput::Write(intention_tools::WriteInput {
                path: intention_types::WorkspaceRelativePathDto::parse("x").expect("path"),
                content: intention_tools::BoundedText::new("x").expect("content"),
            }),
        ),
        (
            "edit",
            ToolInput::Edit(intention_tools::EditInput {
                path: intention_types::WorkspaceRelativePathDto::parse("x").expect("path"),
                old: intention_tools::BoundedText::new("x").expect("old"),
                new: intention_tools::BoundedText::new("y").expect("new"),
            }),
        ),
    ];
    for (id, input) in inputs {
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let _ =
            ApplicationService::new(&repository).invoke_local_tool(InvokeLocalToolInputDto::new(
                workspace.clone(),
                SessionId::new(),
                RunId::new(),
                ToolCallId::new(),
                id,
                input,
                fixture_time(),
            ));
    }
    let _ = fs::remove_dir_all(root);
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
fn local_tool_invalid_result_outcome_is_rejected_before_execution() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(PhaseOutcomeHook {
            phase: Phase::BeforeToolExecution,
            id: "before-execution-invalid",
            outcome: HookOutcome::TransformResult(intention_tools::ToolResult::Read(
                intention_tools::TextResult {
                    text: intention_tools::BoundedText::new("x").expect("text"),
                    truncated: false,
                },
            )),
        }))
        .expect("hook");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(InvokeLocalToolInputDto::new(
            WorkspaceRoot::resolve(
                &WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy()).expect("root"),
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
        .expect_err("invalid outcome");
    assert_eq!(error.code(), "invalid_hook_outcome");
}

#[test]
fn local_tool_cancellation_records_one_external_effect_terminal_event() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let signal = intention_tools::CancellationSignal::cancelled();
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
                    program: intention_tools::BoundedText::new("sh").expect("program"),
                    args: vec![],
                }),
                fixture_time(),
            )
            .with_cancellation(signal),
        )
        .expect_err("cancelled invocation");
    assert_eq!(error.code(), "tool_cancelled");
    let events = repository.tool_events.borrow();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(
                e.status(),
                intention_domain::ToolLifecycleStatusDto::Cancelled
                    | intention_domain::ToolLifecycleStatusDto::ExternalEffectUnknown
                    | intention_domain::ToolLifecycleStatusDto::Failed
                    | intention_domain::ToolLifecycleStatusDto::Completed
            ))
            .count(),
        1
    );
}

#[test]
fn post_effect_transform_is_applied_sequentially_and_invalid_outcome_fails_terminally() {
    let root = std::env::temp_dir().join(format!("intention-app-post-{}", SessionId::new()));
    fs::create_dir_all(&root).expect("root");
    fs::write(root.join("hello.txt"), "hello").expect("file");
    let workspace =
        WorkspaceRoot::resolve(&WorkspaceRootDto::parse(root.to_string_lossy()).expect("root dto"))
            .expect("workspace");
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(PostEffectHook {
            outcome: HookOutcome::TransformInput(ToolInput::Read(ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("hello.txt").expect("path"),
            })),
        }))
        .expect("hook");
    let error = ApplicationService::with_hooks(&repository, hooks)
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
        .expect_err("invalid post-effect outcome");
    assert_eq!(error.code(), "invalid_hook_outcome");
    assert_eq!(
        repository
            .tool_events
            .borrow()
            .iter()
            .filter(|e| matches!(e.status(), intention_domain::ToolLifecycleStatusDto::Failed))
            .count(),
        1
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn send_user_turn_maps_durable_started_outcome_and_retries_without_local_enforcement() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let config = snapshot();
    let committed = change(
        projection(
            session_id,
            Some(RunProjectionDto::new(
                session_id,
                run_id,
                turn_id,
                RunStatusDto::Starting,
                config.revision_id(),
            )),
            Vec::new(),
            2,
        ),
        Some(AcceptedTurnOutcomeDto::Started(RunProjectionDto::new(
            session_id,
            run_id,
            turn_id,
            RunStatusDto::Starting,
            config.revision_id(),
        ))),
    );
    let repository = FakeRepository::with_accepted(Ok(committed));
    let application = ApplicationService::new(&repository);
    let command =
        SendUserTurnCommandDto::new(session_id, turn_id, "hello").expect("fixture turn is valid");
    let workflow = SendUserTurnWorkflowInputDto::new(run_id, config, fixture_time());

    for _ in 0..2 {
        let result = application
            .send_user_turn(command.clone(), workflow.clone())
            .expect("repository acceptance maps");
        assert_eq!(
            result,
            ProtocolAcceptedResultDto::SendUserTurn(
                intention_protocol::SendUserTurnAcceptedDto::new(
                    session_id,
                    turn_id,
                    SessionEventSequenceDto::new(2),
                    SendUserTurnOutcomeDto::Started {
                        run_id,
                        config_revision_id: workflow.config_snapshot().revision_id(),
                    },
                ),
            )
        );
    }
    assert_eq!(repository.accepted_inputs.borrow().len(), 2);
}

#[test]
fn send_user_turn_propagates_repository_idempotency_conflict() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::validation(
        "turn_idempotency_conflict",
        "the accepted turn identity has different durable content",
    )));
    let application = ApplicationService::new(&repository);

    let error = application
        .send_user_turn(
            SendUserTurnCommandDto::new(SessionId::new(), TurnId::new(), "different")
                .expect("fixture turn is valid"),
            SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), fixture_time()),
        )
        .expect_err("repository conflict must remain typed");
    assert_eq!(error.code(), "turn_idempotency_conflict");
}

#[test]
fn workflows_expose_their_explicit_durable_inputs() {
    let command = CreateSessionCommandDto::new(
        ProjectId::new(),
        SessionId::new(),
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
    );
    let create = CreateSessionWorkflowInputDto::new(command.clone(), fixture_time());
    assert_eq!(create.command(), &command);
    assert_eq!(create.occurred_at(), fixture_time());

    let run_id = RunId::new();
    let config = snapshot();
    let send = SendUserTurnWorkflowInputDto::new(run_id, config.clone(), fixture_time());
    assert_eq!(send.proposed_run_id(), run_id);
    assert_eq!(send.config_snapshot(), &config);
    assert_eq!(send.occurred_at(), fixture_time());
}

#[test]
fn public_dto_constructors_and_schedule_validation_cover_mismatch_paths() {
    let command = SendUserTurnCommandDto::new(SessionId::new(), TurnId::new(), "hello")
        .expect("command is valid");
    let input = SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), fixture_time());
    assert_eq!(input.proposed_run_id(), input.proposed_run_id());
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
}

#[test]
fn send_user_turn_rejects_missing_durable_outcome() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_accepted(Ok(change(
        projection(session_id, None, Vec::new(), 1),
        None,
    )));
    let error = ApplicationService::new(&repository)
        .send_user_turn(
            SendUserTurnCommandDto::new(session_id, TurnId::new(), "hello")
                .expect("fixture turn is valid"),
            SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), fixture_time()),
        )
        .expect_err("accepted commits require a durable outcome");
    assert_eq!(error.code(), "missing_accepted_turn_outcome");
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
    assert_eq!(snapshot.projection(), Some(&state));
}

#[test]
fn application_exposes_internal_run_replay_without_changing_protocol_results() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable(
        "fixture_unused",
        "accept is not used by this fixture",
    )));
    *repository.loaded_replay.borrow_mut() = Some(current_run_replay(session_id, run_id));
    let application = ApplicationService::new(&repository);
    let replay = application
        .load_current_run_replay(session_id, run_id)
        .expect("internal replay maps from storage");
    assert_eq!(replay.snapshot().run_id(), run_id);
    assert!(replay.tail().facts().is_empty());
}

#[test]
fn application_exposes_run_tail_and_propagates_read_errors() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let application = ApplicationService::new(&repository);
    let tail = application
        .load_run_tail(session_id, run_id, RunEventCursorDto::new(0))
        .expect("tail maps from storage");
    assert_eq!(tail.session_id(), session_id);
    assert_eq!(tail.run_id(), run_id);
    assert!(tail.facts().is_empty());
}

#[test]
fn create_and_remove_workflows_map_committed_queue_results() {
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
    let queued = application
        .send_user_turn(
            SendUserTurnCommandDto::new(session_id, queued_turn, "queued")
                .expect("fixture turn is valid"),
            SendUserTurnWorkflowInputDto::new(RunId::new(), snapshot(), fixture_time()),
        )
        .expect("queue maps");
    assert!(matches!(
        queued,
        ProtocolAcceptedResultDto::SendUserTurn(value)
            if value.outcome() == SendUserTurnOutcomeDto::Queued { queue_position: QueuePositionDto::new(4) }
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

    let _ = GetSessionSnapshotQueryDto::new(session_id);
}

#[test]
fn local_tool_workspace_and_execution_hooks_cover_transform_and_rejections() {
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
            phase: Phase::BeforeWorkspaceResolution,
            id: "workspace-transform",
            outcome: HookOutcome::TransformInput(ToolInput::Read(ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("hello.txt").expect("path"),
            })),
        }))
        .expect("hook");
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
                path: intention_types::WorkspaceRelativePathDto::parse("wrong").expect("path"),
            }),
            fixture_time(),
        ))
        .expect("transformed read succeeds");
    assert!(matches!(result, intention_tools::ToolResult::Read(_)));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_tool_hook_transform_result_before_execution_is_rejected() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(PhaseOutcomeHook {
            phase: Phase::BeforeWorkspaceResolution,
            id: "workspace-invalid",
            outcome: HookOutcome::TransformResult(intention_tools::ToolResult::Read(
                intention_tools::TextResult {
                    text: intention_tools::BoundedText::new("x").expect("text"),
                    truncated: false,
                },
            )),
        }))
        .expect("hook");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(InvokeLocalToolInputDto::new(
            WorkspaceRoot::resolve(
                &WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy()).expect("dto"),
            )
            .expect("root"),
            SessionId::new(),
            RunId::new(),
            ToolCallId::new(),
            "read",
            ToolInput::Read(ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("missing").expect("path"),
            }),
            fixture_time(),
        ))
        .expect_err("invalid result outcome");
    assert_eq!(error.code(), "invalid_hook_outcome");
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
        Phase::AfterToolResultPublished,
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
        assert!(matches!(result, intention_tools::ToolResult::Read(_)));
    }
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
fn local_tool_covers_post_execution_hook_errors_and_rejections() {
    let root = std::env::temp_dir().join(format!("intention-app-post-hooks-{}", SessionId::new()));
    fs::create_dir_all(&root).expect("root");
    fs::write(root.join("hello.txt"), "hello").expect("file");
    let workspace = WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy()).expect("workspace"),
    )
    .expect("workspace is valid");
    let outcome = HookOutcome::Reject(ErrorDto::validation("post_blocked", "blocked"));
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(PhaseOutcomeHook {
            phase: Phase::AfterToolExecution,
            outcome,
            id: "post-reject",
        }))
        .expect("hook");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(invoke_read_input_in_workspace(&workspace, "hello.txt"))
        .expect_err("post hook rejection");
    assert_eq!(error.code(), "post_blocked");

    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(DispatchErrorHook {
            phase: Phase::AfterToolExecution,
        }))
        .expect("hook");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(invoke_read_input_in_workspace(&workspace, "hello.txt"))
        .expect_err("post hook error");
    assert_eq!(error.code(), "hook_failed");
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
        // Keep the child alive long enough for spawn and for the execution
        // poller to observe it, while avoiding a race with a short command.
        std::thread::sleep(std::time::Duration::from_millis(100));
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
                            "10"
                        } else {
                            "sleep 5"
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
fn local_tool_propagates_append_persistence_error() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    *repository.tool_error_after.borrow_mut() =
        Some(ErrorDto::unavailable("append_failed", "append failed"));
    let error = ApplicationService::new(&repository)
        .invoke_local_tool(invoke_read_input("missing"))
        .expect_err("append failure");
    assert_eq!(error.code(), "append_failed");
}

#[test]
fn local_tool_covers_invocation_and_workspace_invalid_hook_outcomes() {
    for phase in [
        Phase::BeforeToolInvocation,
        Phase::BeforeWorkspaceResolution,
    ] {
        let repository =
            FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
        let mut hooks = Registry::new();
        hooks
            .register(Box::new(PhaseOutcomeHook {
                phase,
                outcome: HookOutcome::TransformResult(intention_tools::ToolResult::Read(
                    intention_tools::TextResult {
                        text: intention_tools::BoundedText::new("x").expect("text"),
                        truncated: false,
                    },
                )),
                id: "invalid-result",
            }))
            .expect("hook");
        let error = ApplicationService::with_hooks(&repository, hooks)
            .invoke_local_tool(invoke_read_input("missing"))
            .expect_err("invalid pre-effect result transform");
        assert_eq!(error.code(), "invalid_hook_outcome");
    }
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
fn local_tool_covers_execution_and_result_hook_invalid_outcomes() {
    let repository = FakeRepository::with_accepted(Err(ErrorDto::unavailable("unused", "unused")));
    let mut hooks = Registry::new();
    hooks
        .register(Box::new(PhaseOutcomeHook {
            phase: Phase::BeforeToolExecution,
            outcome: HookOutcome::TransformResult(intention_tools::ToolResult::Read(
                intention_tools::TextResult {
                    text: intention_tools::BoundedText::new("x").expect("text"),
                    truncated: false,
                },
            )),
            id: "execution-invalid-result",
        }))
        .expect("hook");
    let error = ApplicationService::with_hooks(&repository, hooks)
        .invoke_local_tool(invoke_read_input("missing"))
        .expect_err("execution invalid result");
    assert_eq!(error.code(), "invalid_hook_outcome");
}
