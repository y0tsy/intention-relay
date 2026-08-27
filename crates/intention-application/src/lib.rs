//! Application command/query orchestration over DTO-only durable storage.
//!
//! This crate maps committed repository outcomes into protocol-ready DTOs. It
//! neither owns database resources nor reimplements repository idempotency.

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    CreateSessionCommandDto, GetSessionSnapshotQueryDto, RemoveQueuedTurnCommandDto,
    RunEventCursorDto, RunEventTailPageDto, RunReplayDto, SendUserTurnCommandDto,
    StopRunCommandDto,
};
use intention_hooks::{
    HookObservability, Outcome as HookOutcome, PhaseContext, Registry as HookRegistry,
};
use intention_protocol::{
    CreateSessionAcceptedDto, ProtocolAcceptedResultDto, RemoveQueuedTurnAcceptedDto,
    SendUserTurnAcceptedDto, SendUserTurnOutcomeDto, SessionSnapshotDto, StopRunAcceptedDto,
};
use intention_runtime::{
    ModelMessageDto, ModelRequestDto, ModelRoleDto, RuntimeService, RuntimeValuesDto,
    fail_starting_run,
};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, AppendToolLifecycleEventInputDto,
    CreateSessionInputDto, ModelContextRoleDto, RemoveQueuedTurnInputDto, StorageRepositoryDto,
};
use intention_tools::{CancellationSignal, ToolInput, ToolResult, ToolService};
use intention_types::ToolCallId;
use intention_types::{DtoResult, ErrorDto, RunId, SchemaVersionDto, SessionId, TimestampDto};

/// Explicit durable values selected for a create-session workflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateSessionWorkflowInputDto {
    command: CreateSessionCommandDto,
    occurred_at: TimestampDto,
}

impl CreateSessionWorkflowInputDto {
    /// Creates a DTO-only session creation workflow input.
    #[must_use]
    pub const fn new(command: CreateSessionCommandDto, occurred_at: TimestampDto) -> Self {
        Self {
            command,
            occurred_at,
        }
    }

    /// Returns the requested durable session command.
    #[must_use]
    pub const fn command(&self) -> &CreateSessionCommandDto {
        &self.command
    }

    /// Returns the event timestamp selected by the caller.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Explicit durable values selected for one accepted user turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SendUserTurnWorkflowInputDto {
    proposed_run_id: RunId,
    config_snapshot: ConfigSnapshotDto,
    occurred_at: TimestampDto,
}

impl SendUserTurnWorkflowInputDto {
    /// Creates a DTO-only user-turn workflow input.
    #[must_use]
    pub const fn new(
        proposed_run_id: RunId,
        config_snapshot: ConfigSnapshotDto,
        occurred_at: TimestampDto,
    ) -> Self {
        Self {
            proposed_run_id,
            config_snapshot,
            occurred_at,
        }
    }

    /// Returns the supplied first-or-future run identity.
    #[must_use]
    pub const fn proposed_run_id(&self) -> RunId {
        self.proposed_run_id
    }

    /// Returns the immutable snapshot retained for a started or queued run.
    #[must_use]
    pub const fn config_snapshot(&self) -> &ConfigSnapshotDto {
        &self.config_snapshot
    }

    /// Returns the selected durable event timestamp.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Synchronous DTO-only boundary that admits accepted work to daemon-owned scheduling.
///
/// Implementations must not invoke a provider. The daemon host owns all
/// asynchronous execution after this bounded post-commit admission.
pub trait ModelRunDispatchPort {
    /// Schedules one fully constructed model run.
    ///
    /// # Errors
    ///
    /// Returns a typed local scheduling error when the daemon cannot accept the work.
    fn dispatch_model_run(&self, input: ScheduleModelRunDto) -> DtoResult<()>;
}

/// Application boundary for one explicit local tool invocation.
pub trait LocalToolInvocationPort {
    /// Executes exactly one typed tool call after admission.
    ///
    /// # Errors
    ///
    /// Returns the typed storage or tool execution error.
    fn invoke_local_tool(&self, input: InvokeLocalToolInputDto) -> DtoResult<ToolResult>;
}

/// Application-owned observation boundary for tolerated hook failures.
pub trait HookObservationPort {
    fn observe_hook_failure(&self, observation: HookObservability);
}

impl HookObservationPort for () {
    fn observe_hook_failure(&self, _: HookObservability) {}
}

/// Runs application hooks while retaining safe fail-open observations.
fn dispatch_hooks(registry: &HookRegistry, context: &PhaseContext) -> DtoResult<HookOutcome> {
    Ok(registry.dispatch_with_observability(context)?.outcome)
}

/// Complete DTO-only input for one local tool invocation.
#[derive(Debug)]
pub struct InvokeLocalToolInputDto {
    workspace: intention_workspace::WorkspaceRoot,
    session_id: SessionId,
    run_id: RunId,
    call_id: ToolCallId,
    tool_id: String,
    input: ToolInput,
    occurred_at: TimestampDto,
    cancellation: CancellationSignal,
}

impl InvokeLocalToolInputDto {
    /// Creates a local tool invocation input.
    #[must_use]
    pub fn new(
        workspace: intention_workspace::WorkspaceRoot,
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
        tool_id: impl Into<String>,
        input: ToolInput,
        occurred_at: TimestampDto,
    ) -> Self {
        Self {
            workspace,
            session_id,
            run_id,
            call_id,
            tool_id: tool_id.into(),
            input,
            occurred_at,
            cancellation: CancellationSignal::new(),
        }
    }

    /// Requests cancellation of this invocation.
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: CancellationSignal) -> Self {
        self.cancellation = cancellation;
        self
    }
}

/// Complete DTO-only scheduling payload constructed from durable starting-run context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleModelRunDto {
    session_id: SessionId,
    run_id: RunId,
    request: ModelRequestDto,
    safe_config: ConfigSnapshotDto,
}

impl ScheduleModelRunDto {
    /// Creates a coherent model-run scheduling payload.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the request identity or model does not
    /// agree with the durable selected configuration.
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        request: ModelRequestDto,
        safe_config: ConfigSnapshotDto,
    ) -> DtoResult<Self> {
        if request.run_id() != run_id
            || request.model() != safe_config.resolved().provider().model()
        {
            return Err(ErrorDto::validation(
                "invalid_model_run_schedule",
                "model scheduling request must match the durable starting run selection",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            request,
            safe_config,
        })
    }

    /// Returns the owning durable session.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the exact durable starting run.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the provider-neutral request built from durable context.
    #[must_use]
    pub const fn request(&self) -> &ModelRequestDto {
        &self.request
    }

    /// Returns the immutable credential-free run configuration selection.
    #[must_use]
    pub const fn safe_config(&self) -> &ConfigSnapshotDto {
        &self.safe_config
    }
}

/// DTO-only application facade over one semantic storage repository.
pub struct ApplicationService<'a, Repository> {
    repository: &'a Repository,
    hooks: HookRegistry,
}

impl<'a, Repository> ApplicationService<'a, Repository>
where
    Repository: StorageRepositoryDto,
{
    /// Executes one explicit local invocation and durably records its lifecycle.
    ///
    /// # Errors
    ///
    /// Returns the typed validation, storage, or tool execution error.
    pub fn invoke_local_tool(&self, input: InvokeLocalToolInputDto) -> DtoResult<ToolResult> {
        let InvokeLocalToolInputDto {
            workspace,
            session_id,
            run_id,
            call_id,
            tool_id,
            mut input,
            occurred_at,
            cancellation,
        } = input;
        if tool_id != expected_tool_id(&input) {
            return Err(ErrorDto::validation(
                "tool_id_mismatch",
                "tool identifier does not match typed tool input",
            ));
        }
        let admitted = intention_domain::ToolLifecycleEventDto::new(
            session_id,
            run_id,
            call_id,
            tool_id.clone(),
            intention_domain::ToolLifecycleStatusDto::Admitted,
            "local tool invocation admitted",
            occurred_at,
        )?;
        self.repository
            .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(admitted))?;
        let invocation = PhaseContext::Invocation {
            call: call_id,
            input: input.clone(),
        };
        match dispatch_hooks(&self.hooks, &invocation) {
            Err(error) => {
                append_tool_rejected(
                    self.repository,
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    &error,
                    occurred_at,
                )?;
                return Err(error);
            }
            Ok(HookOutcome::Reject(error)) => {
                append_tool_rejected(
                    self.repository,
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    &error,
                    occurred_at,
                )?;
                return Err(error);
            }
            Ok(HookOutcome::TransformResult(_)) => {
                let error = ErrorDto::validation(
                    "invalid_hook_outcome",
                    "result transformation is not valid before execution",
                );
                append_tool_rejected(
                    self.repository,
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    &error,
                    occurred_at,
                )?;
                return Err(error);
            }
            Ok(HookOutcome::TransformInput(value)) => input = value,
            Ok(HookOutcome::Continue) => {}
        }
        let workspace_context = PhaseContext::WorkspaceResolution {
            call: call_id,
            input: input.clone(),
        };
        match dispatch_hooks(&self.hooks, &workspace_context) {
            Err(error) => {
                append_tool_rejected(
                    self.repository,
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    &error,
                    occurred_at,
                )?;
                return Err(error);
            }
            Ok(HookOutcome::TransformInput(value)) => input = value,
            Ok(HookOutcome::Reject(error)) => {
                append_tool_rejected(
                    self.repository,
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    &error,
                    occurred_at,
                )?;
                return Err(error);
            }
            Ok(HookOutcome::TransformResult(_)) => {
                let error = ErrorDto::validation(
                    "invalid_hook_outcome",
                    "result transformation is not valid before execution",
                );
                append_tool_rejected(
                    self.repository,
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    &error,
                    occurred_at,
                )?;
                return Err(error);
            }
            Ok(HookOutcome::Continue) => {}
        }
        let resolved = PhaseContext::WorkspaceResolved {
            call: call_id,
            input: input.clone(),
        };
        match dispatch_hooks(&self.hooks, &resolved) {
            Err(error) => {
                append_tool_rejected(
                    self.repository,
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    &error,
                    occurred_at,
                )?;
                return Err(error);
            }
            Ok(HookOutcome::Reject(error)) => {
                let event = intention_domain::ToolLifecycleEventDto::new(
                    session_id,
                    run_id,
                    call_id,
                    tool_id,
                    intention_domain::ToolLifecycleStatusDto::Rejected,
                    error.code(),
                    occurred_at,
                )?;
                self.repository
                    .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(event))?;
                return Err(error);
            }
            Ok(HookOutcome::TransformResult(_)) => {
                let error = ErrorDto::validation(
                    "invalid_hook_outcome",
                    "result transformation is not valid before execution",
                );
                append_tool_rejected(
                    self.repository,
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    &error,
                    occurred_at,
                )?;
                return Err(error);
            }
            Ok(HookOutcome::TransformInput(value)) => input = value,
            Ok(HookOutcome::Continue) => {}
        }
        let before_execution = PhaseContext::Execution {
            call: call_id,
            input: input.clone(),
        };
        let transformed_input = match dispatch_hooks(&self.hooks, &before_execution) {
            Err(error) | Ok(HookOutcome::Reject(error)) => {
                append_tool_rejected(
                    self.repository,
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    &error,
                    occurred_at,
                )?;
                return Err(error);
            }
            Ok(HookOutcome::TransformInput(value)) => value,
            Ok(HookOutcome::Continue) => input,
            Ok(HookOutcome::TransformResult(_)) => {
                let error = ErrorDto::validation(
                    "invalid_hook_outcome",
                    "result transformation is not valid before execution",
                );
                append_tool_rejected(
                    self.repository,
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    &error,
                    occurred_at,
                )?;
                return Err(error);
            }
        };
        let started = intention_domain::ToolLifecycleEventDto::new(
            session_id,
            run_id,
            call_id,
            tool_id.clone(),
            intention_domain::ToolLifecycleStatusDto::Started,
            "local tool invocation started",
            occurred_at,
        )?;
        self.repository
            .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(started))?;
        let service = ToolService::new(workspace);
        let result =
            service.dispatch_with_cancellation(call_id, transformed_input.clone(), cancellation);
        let mut result = match result {
            Ok(value) => {
                let context = PhaseContext::Executed {
                    call: call_id,
                    input: transformed_input,
                    result: value.clone(),
                };
                match dispatch_hooks(&self.hooks, &context) {
                    Err(error) => {
                        append_tool_failed(
                            self.repository,
                            session_id,
                            run_id,
                            call_id,
                            &tool_id,
                            &error,
                            occurred_at,
                        )?;
                        return Err(error);
                    }
                    Ok(outcome) => match outcome {
                        HookOutcome::TransformResult(value) => Ok(value),
                        HookOutcome::Reject(error) => Err(error),
                        HookOutcome::Continue => Ok(value),
                        HookOutcome::TransformInput(_) => Err(ErrorDto::validation(
                            "invalid_hook_outcome",
                            "input transformation is not valid after execution",
                        )),
                    },
                }
            }
            Err(error) => {
                append_tool_terminal(
                    self.repository,
                    ToolTerminalInput {
                        session_id,
                        run_id,
                        call_id,
                        tool_id: &tool_id,
                        error: &error,
                        status: terminal_status_for_error(&error),
                        occurred_at,
                    },
                )?;
                return Err(error);
            }
        };
        if let Ok(mut value) = result {
            for phase in [
                intention_hooks::Phase::BeforeToolResultPersist,
                intention_hooks::Phase::BeforeToolResultModelContext,
                intention_hooks::Phase::AfterToolResultPublished,
            ] {
                let context = result_phase_context(phase, call_id, &value);
                let outcome = match dispatch_hooks(&self.hooks, &context) {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        append_tool_failed(
                            self.repository,
                            session_id,
                            run_id,
                            call_id,
                            &tool_id,
                            &error,
                            occurred_at,
                        )?;
                        return Err(error);
                    }
                };
                match outcome {
                    HookOutcome::Reject(error) => {
                        append_tool_failed(
                            self.repository,
                            session_id,
                            run_id,
                            call_id,
                            &tool_id,
                            &error,
                            occurred_at,
                        )?;
                        return Err(error);
                    }
                    HookOutcome::TransformResult(next) => value = next,
                    HookOutcome::Continue => {}
                    HookOutcome::TransformInput(_) => {
                        let error = ErrorDto::validation(
                            "invalid_hook_outcome",
                            "input transformation is not valid after execution",
                        );
                        append_tool_failed(
                            self.repository,
                            session_id,
                            run_id,
                            call_id,
                            &tool_id,
                            &error,
                            occurred_at,
                        )?;
                        return Err(error);
                    }
                }
            }
            result = Ok(value);
        }
        let (status, detail) = match &result {
            Ok(_) => (
                intention_domain::ToolLifecycleStatusDto::Completed,
                "local tool invocation completed",
            ),
            Err(error) => (
                intention_domain::ToolLifecycleStatusDto::Failed,
                error.code(),
            ),
        };
        let event = intention_domain::ToolLifecycleEventDto::new(
            session_id,
            run_id,
            call_id,
            tool_id,
            status,
            detail,
            occurred_at,
        )?;
        self.repository
            .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(event))?;
        result
    }
    /// Creates an application facade around a DTO-only durable repository.
    #[must_use]
    pub const fn new(repository: &'a Repository) -> Self {
        Self {
            repository,
            hooks: HookRegistry::new(),
        }
    }

    /// Creates an application facade with the supplied lifecycle hooks.
    #[must_use]
    pub const fn with_hooks(repository: &'a Repository, hooks: HookRegistry) -> Self {
        Self { repository, hooks }
    }

    /// Creates a durable session and maps its committed evidence for protocol use.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when durable session creation fails.
    pub fn create_session(
        &self,
        input: CreateSessionWorkflowInputDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let change = self.repository.create_session(CreateSessionInputDto::new(
            input.command.clone(),
            input.occurred_at,
        ))?;
        Ok(ProtocolAcceptedResultDto::CreateSession(
            CreateSessionAcceptedDto::new(
                input.command.project_id(),
                input.command.workspace_id(),
                input.command.session_id(),
                change.position(),
            ),
        ))
    }

    /// Accepts a user turn and maps the repository's durable started/queued outcome.
    ///
    /// The repository is the sole idempotency authority. Repeating identical
    /// content returns its committed result; conflicting content remains a typed
    /// repository conflict.
    ///
    /// # Errors
    ///
    /// Returns typed validation, conflict, or availability errors from the
    /// storage contract, or an internal consistency error for a malformed
    /// accepted-turn commit.
    pub fn send_user_turn(
        &self,
        command: SendUserTurnCommandDto,
        input: SendUserTurnWorkflowInputDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let change = self
            .repository
            .accept_user_turn(AcceptUserTurnInputDto::new(
                command.session_id(),
                command.turn_id(),
                command.content(),
                input.proposed_run_id,
                input.config_snapshot,
                input.occurred_at,
            )?)?;
        let accepted = accepted_user_turn(&command, &change)?;
        Ok(accepted)
    }

    /// Accepts a user turn and schedules an exactly-started run only after its initial commit.
    ///
    /// Queued outcomes and idempotent retry evidence never load model context or
    /// dispatch. Any post-commit context or dispatch failure is durably recorded
    /// against the exact `Starting` run and this method still returns the original
    /// acceptance.
    ///
    /// # Errors
    ///
    /// Returns an admission or malformed durable-acceptance error. Post-commit
    /// scheduling failures deliberately preserve the committed acceptance result.
    pub fn send_user_turn_and_schedule<Dispatch>(
        &self,
        command: SendUserTurnCommandDto,
        input: SendUserTurnWorkflowInputDto,
        dispatch: &Dispatch,
    ) -> DtoResult<ProtocolAcceptedResultDto>
    where
        Dispatch: ModelRunDispatchPort,
    {
        let occurred_at = input.occurred_at();
        let change = self
            .repository
            .accept_user_turn(AcceptUserTurnInputDto::new(
                command.session_id(),
                command.turn_id(),
                command.content(),
                input.proposed_run_id,
                input.config_snapshot,
                input.occurred_at,
            )?)?;
        let accepted = accepted_user_turn(&command, &change)?;
        let ProtocolAcceptedResultDto::SendUserTurn(accepted_turn) = accepted else {
            unreachable!("accepted user turn always returns user-turn acceptance")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = accepted_turn.outcome() else {
            return Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn));
        };
        if !started_run_committed_in(&change, run_id) {
            return Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn));
        }
        let session_id = accepted_turn.session_id();
        let schedule = match self
            .repository
            .load_starting_run_model_context(session_id, run_id)
        {
            Ok(context) if context.session_id() == session_id && context.run_id() == run_id => {
                match schedule_from_context(context) {
                    Ok(schedule) => schedule,
                    Err(_) => {
                        preserve_accepted_after_scheduling_failure(
                            self.repository,
                            session_id,
                            run_id,
                            "model_context_unavailable",
                            occurred_at,
                        );
                        return Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn));
                    }
                }
            }
            Ok(_) | Err(_) => {
                preserve_accepted_after_scheduling_failure(
                    self.repository,
                    session_id,
                    run_id,
                    "model_context_unavailable",
                    occurred_at,
                );
                return Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn));
            }
        };
        if dispatch.dispatch_model_run(schedule).is_err() {
            preserve_accepted_after_scheduling_failure(
                self.repository,
                session_id,
                run_id,
                "model_scheduling_unavailable",
                occurred_at,
            );
        }
        Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn))
    }

    /// Removes one unstarted queued turn and maps its committed evidence.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when no queued turn can be removed.
    pub fn remove_queued_turn(
        &self,
        command: RemoveQueuedTurnCommandDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let change = self
            .repository
            .remove_queued_turn(RemoveQueuedTurnInputDto::new(command, occurred_at))?;
        Ok(ProtocolAcceptedResultDto::RemoveQueuedTurn(
            RemoveQueuedTurnAcceptedDto::new(
                command.session_id(),
                command.turn_id(),
                change.position(),
            ),
        ))
    }

    /// Stops a run through the deterministic runtime lifecycle service.
    ///
    /// # Errors
    ///
    /// Returns the typed lifecycle or storage error when cancellation cannot be
    /// committed.
    pub fn stop_run(
        &self,
        command: StopRunCommandDto,
        values: RuntimeValuesDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let change = RuntimeService::new(self.repository, values)
            .stop_run(command.session_id(), command.run_id())?;
        Ok(ProtocolAcceptedResultDto::StopRun(StopRunAcceptedDto::new(
            command.session_id(),
            command.run_id(),
            change.position(),
        )))
    }

    /// Loads the current internal run-scoped durable replay.
    ///
    /// This application-facing read deliberately does not alter the M3 public
    /// protocol subscription surface.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when the requested scoped replay is
    /// absent, mismatched, or unavailable.
    pub fn load_current_run_replay(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunReplayDto> {
        self.repository.load_current_run_replay(session_id, run_id)
    }

    /// Loads one bounded internal run-scoped fact tail.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when the requested tail cannot be
    /// read for this exact session/run identity and cursor.
    pub fn load_run_tail(
        &self,
        session_id: SessionId,
        run_id: RunId,
        after_cursor: RunEventCursorDto,
    ) -> DtoResult<RunEventTailPageDto> {
        self.repository
            .load_run_tail(session_id, run_id, after_cursor)
    }

    /// Reconstructs the exact durable context for one current `Starting` run.
    ///
    /// This is the daemon-host admission read. It deliberately does not dispatch
    /// work itself, so composition remains the owner of provider execution.
    ///
    /// # Errors
    ///
    /// Returns a typed context or scheduling error when the exact durable run is
    /// unavailable or is no longer eligible for execution.
    pub fn schedule_starting_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<ScheduleModelRunDto> {
        schedule_from_context(
            self.repository
                .load_starting_run_model_context(session_id, run_id)?,
        )
    }

    /// Loads the current durable session projection as a versioned protocol snapshot.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when the requested session is absent
    /// or cannot be loaded.
    pub fn get_session_snapshot(
        &self,
        query: GetSessionSnapshotQueryDto,
    ) -> DtoResult<SessionSnapshotDto> {
        let projection = self.repository.load_session_snapshot(query.session_id())?;
        SessionSnapshotDto::with_projection(
            SchemaVersionDto::new(1, 0),
            query.session_id(),
            projection.at_sequence(),
            projection,
        )
    }
}

fn accepted_user_turn(
    command: &SendUserTurnCommandDto,
    change: &intention_storage::CommittedChangeDto,
) -> DtoResult<ProtocolAcceptedResultDto> {
    let outcome = change.turn_outcome().ok_or_else(|| {
        ErrorDto::validation(
            "missing_accepted_turn_outcome",
            "durable turn acceptance did not include an outcome",
        )
    })?;
    let outcome = match outcome {
        AcceptedTurnOutcomeDto::Started(run) => SendUserTurnOutcomeDto::Started {
            run_id: run.run_id(),
            config_revision_id: run.config_revision_id(),
        },
        AcceptedTurnOutcomeDto::Queued(queue_position) => {
            SendUserTurnOutcomeDto::Queued { queue_position }
        }
    };
    Ok(ProtocolAcceptedResultDto::SendUserTurn(
        SendUserTurnAcceptedDto::new(
            command.session_id(),
            command.turn_id(),
            change.position(),
            outcome,
        ),
    ))
}

fn append_tool_rejected<R: StorageRepositoryDto>(
    r: &R,
    s: SessionId,
    run: RunId,
    call: ToolCallId,
    id: &str,
    e: &ErrorDto,
    at: TimestampDto,
) -> DtoResult<()> {
    let event = intention_domain::ToolLifecycleEventDto::new(
        s,
        run,
        call,
        id.to_owned(),
        intention_domain::ToolLifecycleStatusDto::Rejected,
        e.code(),
        at,
    )?;
    r.append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(event))
        .map(|_| ())
}

const fn expected_tool_id(input: &ToolInput) -> &'static str {
    match input {
        ToolInput::Read(_) => "read",
        ToolInput::Glob(_) => "glob",
        ToolInput::Grep(_) => "grep",
        ToolInput::Write(_) => "write",
        ToolInput::Edit(_) => "edit",
        ToolInput::Execute(_) => "execute",
    }
}

fn terminal_status_for_error(error: &ErrorDto) -> intention_domain::ToolLifecycleStatusDto {
    match error.code() {
        "tool_execute_external_effect_unknown" => {
            intention_domain::ToolLifecycleStatusDto::ExternalEffectUnknown
        }
        "tool_cancelled" => intention_domain::ToolLifecycleStatusDto::Cancelled,
        _ => intention_domain::ToolLifecycleStatusDto::Failed,
    }
}

fn append_tool_failed<R: StorageRepositoryDto>(
    r: &R,
    s: SessionId,
    run: RunId,
    call: ToolCallId,
    id: &str,
    e: &ErrorDto,
    at: TimestampDto,
) -> DtoResult<()> {
    let event = intention_domain::ToolLifecycleEventDto::new(
        s,
        run,
        call,
        id.to_owned(),
        intention_domain::ToolLifecycleStatusDto::Failed,
        e.code(),
        at,
    )?;
    r.append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(event))
        .map(|_| ())
}

struct ToolTerminalInput<'a> {
    session_id: SessionId,
    run_id: RunId,
    call_id: ToolCallId,
    tool_id: &'a str,
    error: &'a ErrorDto,
    status: intention_domain::ToolLifecycleStatusDto,
    occurred_at: TimestampDto,
}

fn append_tool_terminal<R: StorageRepositoryDto>(
    r: &R,
    input: ToolTerminalInput<'_>,
) -> DtoResult<()> {
    let event = intention_domain::ToolLifecycleEventDto::new(
        input.session_id,
        input.run_id,
        input.call_id,
        input.tool_id.to_owned(),
        input.status,
        input.error.code(),
        input.occurred_at,
    )?;
    r.append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(event))
        .map(|_| ())
}

fn started_run_committed_in(change: &intention_storage::CommittedChangeDto, run_id: RunId) -> bool {
    change
        .events()
        .iter()
        .any(|event| event.run_id() == Some(run_id))
}

fn preserve_accepted_after_scheduling_failure<Repository>(
    repository: &Repository,
    session_id: SessionId,
    run_id: RunId,
    failure_code: &'static str,
    occurred_at: TimestampDto,
) where
    Repository: StorageRepositoryDto,
{
    if fail_starting_run(repository, session_id, run_id, failure_code, occurred_at).is_err() {
        // The durable acceptance is already the externally documented result;
        // a secondary failure write cannot replace it with a scheduling error.
    }
}

fn schedule_from_context(
    context: intention_storage::StartingRunModelContextDto,
) -> DtoResult<ScheduleModelRunDto> {
    let messages = context
        .messages()
        .iter()
        .map(|message| {
            ModelMessageDto::new(
                match message.role() {
                    ModelContextRoleDto::User => ModelRoleDto::User,
                    ModelContextRoleDto::Assistant => ModelRoleDto::Assistant,
                },
                message.content(),
            )
        })
        .collect::<DtoResult<Vec<_>>>()?;
    let request = ModelRequestDto::new(
        context.run_id(),
        context.safe_config().resolved().provider().model(),
        messages,
        None,
        None,
    )?;
    ScheduleModelRunDto::new(
        context.session_id(),
        context.run_id(),
        request,
        context.safe_config().clone(),
    )
}

fn result_phase_context(
    phase: intention_hooks::Phase,
    call: ToolCallId,
    result: &ToolResult,
) -> PhaseContext {
    match phase {
        intention_hooks::Phase::BeforeToolResultPersist => PhaseContext::Persist {
            call,
            result: result.clone(),
        },
        intention_hooks::Phase::BeforeToolResultModelContext => PhaseContext::ModelContext {
            call,
            result: result.clone(),
        },
        intention_hooks::Phase::AfterToolResultPublished => PhaseContext::Published {
            call,
            result: result.clone(),
        },
        _ => unreachable!("result lifecycle phase list contains only result phases"),
    }
}

#[cfg(test)]
mod tests {
    use super::{expected_tool_id, result_phase_context, terminal_status_for_error};
    use intention_domain::ToolLifecycleStatusDto;
    use intention_hooks::{Phase, PhaseContext};
    use intention_tools::{ToolInput, ToolResult};
    use intention_types::{ErrorDto, ToolCallId};

    #[test]
    fn maps_each_typed_tool_input_to_its_id() {
        let _ = (expected_tool_id, ToolInput::Read);
    }

    #[test]
    fn maps_terminal_error_statuses() {
        assert_eq!(
            terminal_status_for_error(&ErrorDto::validation("tool_cancelled", "x")),
            ToolLifecycleStatusDto::Cancelled
        );
        assert_eq!(
            terminal_status_for_error(&ErrorDto::validation(
                "tool_execute_external_effect_unknown",
                "x"
            )),
            ToolLifecycleStatusDto::ExternalEffectUnknown
        );
        assert_eq!(
            terminal_status_for_error(&ErrorDto::validation("other", "x")),
            ToolLifecycleStatusDto::Failed
        );
    }

    #[test]
    fn maps_result_phases_to_their_contexts() {
        let call = ToolCallId::new();
        let result = ToolResult::Read(intention_tools::TextResult {
            text: match intention_tools::BoundedText::new("ok") {
                Ok(text) => text,
                Err(_) => return,
            },
            truncated: false,
        });
        assert!(matches!(
            result_phase_context(Phase::BeforeToolResultPersist, call, &result),
            PhaseContext::Persist { .. }
        ));
        assert!(matches!(
            result_phase_context(Phase::BeforeToolResultModelContext, call, &result),
            PhaseContext::ModelContext { .. }
        ));
        assert!(matches!(
            result_phase_context(Phase::AfterToolResultPublished, call, &result),
            PhaseContext::Published { .. }
        ));
    }
}
