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
    CreateSessionInputDto, ModelContextRoleDto, ProviderCatalogRepositoryDto,
    RemoveQueuedTurnInputDto, SessionProviderDefaultsRepositoryDto, StorageRepositoryDto,
    ToolResultEvidenceDto, ToolResultKindDto,
};
use intention_tools::{CancellationSignal, ToolInput, ToolResult, ToolService};
use intention_types::ToolCallId;
use intention_types::{DtoResult, ErrorDto, RunId, SchemaVersionDto, SessionId, TimestampDto};

pub mod provider_catalog;
pub mod provider_control_plane;
pub mod provider_gate;
pub mod provider_registry;
pub mod session_selection;

pub use provider_catalog::{
    CatalogAcceptanceOutcomeDto, CatalogCandidateOutcomeDto, CatalogProviderDeclarationDto,
    CatalogSourceInputDto, CatalogStartupOutcomeDto, ProviderAdmissionDto,
    ProviderCatalogController, ProviderCatalogProjectionDto,
};
pub use provider_control_plane::{
    ConfigurationReloadService, CredentialRotationService, DiscoveryPort, DiscoveryScopeDto,
    DriverRebuildPort, HealthProbePort, PricingPolicyService, PrivateCredentialMaterial,
    PrivateCredentialPort, ProviderDiscoveryService, ProviderHealthService, ReloadCandidateDto,
    ReloadCommitOutcomeDto, SafeBindingSource, SafeCompositionBindingDto,
};
pub use provider_gate::{CatalogReadiness, ControlPlaneGate, ControlPlaneState};
pub use provider_registry::{
    MAX_ACTIVE_PRIVATE_ENTRIES, ModelRunDriverHandle, PrivateProviderProfileMaterial,
    PrivateRegistry, PrivateRegistryKey, ProviderDriverFactory, private_credential_reference,
};
pub use session_selection::{
    CatalogAdmissionPort, CatalogReadService, ControlPlaneReadinessPort, DegradedModeService,
    HeldRunService, RemovalService, ResolvedProfileDto, SelectionResolutionService,
    SessionProfileService, UnavailableQueueService, UsageService,
};

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

/// Publication seam invoked after the durable result has been committed.
pub trait ToolResultPublicationPort {
    /// Publishes the committed tool result.
    ///
    /// # Errors
    ///
    /// Returns the typed error reported by the daemon-owned publication boundary.
    fn publish_tool_result(&self, input: &ToolResultPublicationInputDto) -> DtoResult<()>;
}

/// Typed identity and payload passed to the publication boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolResultPublicationInputDto {
    session_id: SessionId,
    run_id: RunId,
    call_id: ToolCallId,
    result: ToolResult,
}

impl ToolResultPublicationInputDto {
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
        result: ToolResult,
    ) -> Self {
        Self {
            session_id,
            run_id,
            call_id,
            result,
        }
    }
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }
    #[must_use]
    pub const fn call_id(&self) -> ToolCallId {
        self.call_id
    }
    #[must_use]
    pub const fn result(&self) -> &ToolResult {
        &self.result
    }
}

impl ToolResultPublicationPort for () {
    fn publish_tool_result(&self, _: &ToolResultPublicationInputDto) -> DtoResult<()> {
        Ok(())
    }
}

impl HookObservationPort for () {
    fn observe_hook_failure(&self, _: HookObservability) {}
}

/// Composition-owned boundary that resolves the authorized workspace before
/// the post-resolution hook phase. Canonical paths never enter hook contexts.
pub trait WorkspaceBoundaryPort {
    /// Resolves the authorized workspace before post-resolution hooks run.
    ///
    /// # Errors
    ///
    /// Returns a typed workspace-resolution error when the workspace cannot be
    /// authorized or prepared for the invocation.
    fn resolve(&self, workspace: &intention_workspace::WorkspaceRoot) -> DtoResult<()>;
}

impl WorkspaceBoundaryPort for () {
    fn resolve(&self, _: &intention_workspace::WorkspaceRoot) -> DtoResult<()> {
        Ok(())
    }
}

/// Runs application hooks, forwarding tolerated fail-open metadata to the
/// caller-owned observation boundary and returning only the safe outcome.
fn dispatch_hooks<O: HookObservationPort>(
    registry: &HookRegistry,
    context: &PhaseContext,
    observer: &O,
) -> DtoResult<HookOutcome> {
    let dispatched = registry.dispatch_with_observability(context)?;
    for observation in dispatched.failures {
        observer.observe_hook_failure(observation);
    }
    Ok(dispatched.outcome)
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
    workspace_boundary: Box<dyn WorkspaceBoundaryPort + 'a>,
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
        self.invoke_local_tool_with_publication(input, &())
    }

    /// Executes, durably commits, publishes, then dispatches the after-publish hook.
    ///
    /// # Errors
    ///
    /// Returns the typed validation, storage, tool execution, publication, or
    /// post-publish hook error.
    pub fn invoke_local_tool_with_publication<P: ToolResultPublicationPort>(
        &self,
        input: InvokeLocalToolInputDto,
        publisher: &P,
    ) -> DtoResult<ToolResult> {
        self.invoke_local_tool_through_ports(input, publisher, &())
    }

    /// Executes one invocation while tolerated fail-open hook failures reach the
    /// supplied application observation boundary.
    ///
    /// Observations carry only safe hook identity metadata; hook payloads and
    /// typed error content never cross this boundary.
    ///
    /// # Errors
    ///
    /// Returns the typed validation, storage, or tool execution error.
    pub fn invoke_local_tool_with_observation<O: HookObservationPort>(
        &self,
        input: InvokeLocalToolInputDto,
        observer: &O,
    ) -> DtoResult<ToolResult> {
        self.invoke_local_tool_through_ports(input, &(), observer)
    }

    fn invoke_local_tool_through_ports<P: ToolResultPublicationPort, O: HookObservationPort>(
        &self,
        input: InvokeLocalToolInputDto,
        publisher: &P,
        observer: &O,
    ) -> DtoResult<ToolResult> {
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
        match dispatch_hooks(&self.hooks, &invocation, observer) {
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
        self.workspace_boundary
            .resolve(&workspace)
            .inspect_err(|error| {
                let _ = append_tool_rejected(
                    self.repository,
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    error,
                    occurred_at,
                );
            })?;
        match dispatch_hooks(&self.hooks, &workspace_context, observer) {
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
        match dispatch_hooks(&self.hooks, &resolved, observer) {
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
        let transformed_input = match dispatch_hooks(&self.hooks, &before_execution, observer) {
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
                match dispatch_hooks(&self.hooks, &context, observer) {
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
            ] {
                let context = result_phase_context(phase, call_id, &value);
                let outcome = match dispatch_hooks(&self.hooks, &context, observer) {
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
        // The typed result evidence commits atomically with its terminal
        // lifecycle event at this boundary, before any publication.
        let evidence = durable_tool_result_evidence(
            session_id,
            run_id,
            call_id,
            &tool_id,
            result.as_ref(),
            occurred_at,
        )?;
        let event = intention_domain::ToolLifecycleEventDto::new(
            session_id,
            run_id,
            call_id,
            tool_id,
            status,
            detail,
            occurred_at,
        )?;
        let append = AppendToolLifecycleEventInputDto::new(event).with_result(evidence)?;
        self.repository.append_tool_lifecycle_event(append)?;
        if let Ok(value) = &result {
            let publication =
                ToolResultPublicationInputDto::new(session_id, run_id, call_id, value.clone());
            publisher.publish_tool_result(&publication)?;
            let context = PhaseContext::Published {
                call: call_id,
                result: value.clone(),
            };
            match dispatch_hooks(&self.hooks, &context, observer)? {
                HookOutcome::Continue => {}
                HookOutcome::TransformResult(_) | HookOutcome::TransformInput(_) => {
                    return Err(ErrorDto::validation(
                        "invalid_hook_outcome",
                        "published result cannot be transformed",
                    ));
                }
                HookOutcome::Reject(error) => return Err(error),
            }
        }
        result
    }
    /// Creates an application facade around a DTO-only durable repository.
    #[must_use]
    pub fn new(repository: &'a Repository) -> Self {
        Self {
            repository,
            hooks: HookRegistry::new(),
            workspace_boundary: Box::new(()),
        }
    }

    /// Creates an application facade with the supplied lifecycle hooks.
    #[must_use]
    pub fn with_hooks(repository: &'a Repository, hooks: HookRegistry) -> Self {
        Self {
            repository,
            hooks,
            workspace_boundary: Box::new(()),
        }
    }

    #[must_use]
    pub fn with_workspace_boundary<B: WorkspaceBoundaryPort + 'a>(mut self, boundary: B) -> Self {
        self.workspace_boundary = Box::new(boundary);
        self
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

    /// Accepts and schedules a user turn after resolving its provider selection.
    ///
    /// Selection resolution runs before the durable commit; scheduling
    /// follows the same post-commit rules as the selection-less scheduling
    /// path it replaced. The repository is the sole idempotency authority.
    ///
    /// # Errors
    ///
    /// Returns the typed resolution error before any commit, or an admission
    /// or malformed durable-acceptance error.
    pub fn send_user_turn_and_schedule_with_provider_selection<Dispatch>(
        &self,
        command: SendUserTurnCommandDto,
        input: SendUserTurnWorkflowInputDto,
        port: &impl CatalogAdmissionPort,
        dispatch: &Dispatch,
    ) -> DtoResult<ProtocolAcceptedResultDto>
    where
        Dispatch: ModelRunDispatchPort,
        Repository: SessionProviderDefaultsRepositoryDto + ProviderCatalogRepositoryDto,
    {
        let occurred_at = input.occurred_at();
        let accept = self.accept_user_turn_input_with_selection(&command, &input, port)?;
        let change = self.repository.accept_user_turn(accept)?;
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

    /// Resolves and builds the durable turn-acceptance input for one turn.
    ///
    /// The effective profile is resolved before any durable commit; a failed
    /// or absent resolution returns its typed error so no commit happens.
    /// Every accepted turn carries a resolved provider selection.
    fn accept_user_turn_input_with_selection(
        &self,
        command: &SendUserTurnCommandDto,
        input: &SendUserTurnWorkflowInputDto,
        port: &impl CatalogAdmissionPort,
    ) -> DtoResult<AcceptUserTurnInputDto>
    where
        Repository: SessionProviderDefaultsRepositoryDto + ProviderCatalogRepositoryDto,
    {
        let session_default = self
            .repository
            .get_session_provider_profile(command.session_id())?
            .map(|default| default.profile_id);
        let global_default = self
            .repository
            .load_provider_catalog_status()
            .ok()
            .and_then(|state| state.active_default_profile_id);
        let selection = crate::SelectionResolutionService.resolve_for_turn(
            command,
            session_default,
            global_default,
            port,
        )?;
        let base = AcceptUserTurnInputDto::new(
            command.session_id(),
            command.turn_id(),
            command.content(),
            input.proposed_run_id,
            input.config_snapshot.clone(),
            input.occurred_at,
        )?;
        Ok(
            base.with_provider_selection(crate::session_selection::provider_selection_from(
                &selection,
            )?),
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
    append_tool_terminal(
        r,
        ToolTerminalInput {
            session_id: s,
            run_id: run,
            call_id: call,
            tool_id: id,
            error: e,
            status: intention_domain::ToolLifecycleStatusDto::Failed,
            occurred_at: at,
        },
    )
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
    let evidence = durable_tool_result_evidence(
        input.session_id,
        input.run_id,
        input.call_id,
        input.tool_id,
        Err(input.error),
        input.occurred_at,
    )?;
    let event = intention_domain::ToolLifecycleEventDto::new(
        input.session_id,
        input.run_id,
        input.call_id,
        input.tool_id.to_owned(),
        input.status,
        input.error.code(),
        input.occurred_at,
    )?;
    let append = AppendToolLifecycleEventInputDto::new(event).with_result(evidence)?;
    r.append_tool_lifecycle_event(append).map(|_| ())
}

/// Durable canonical result-document ceiling; mirrors the storage evidence bound.
const MAX_DURABLE_TOOL_RESULT_BYTES: usize = 512 * 1024;
/// Characters reserved for closing tokens when truncating a durable document.
const DOCUMENT_RESERVE_BYTES: usize = 64;
/// Raw-byte share of the durable bound granted to one truncated text value.
///
/// Escaped control characters expand at most sixfold, so one eighth of the
/// bound can never exceed it after escaping.
const TRUNCATED_TEXT_BUDGET_DIVISOR: usize = 8;

/// Builds the typed result evidence committed with one terminal lifecycle event.
///
/// Successful outcomes serialize their typed result; failed, cancelled, and
/// unknown-effect outcomes serialize their stable terminal classification. The
/// evidence carries the exact session/run/call identity of the invocation.
fn durable_tool_result_evidence(
    session_id: SessionId,
    run_id: RunId,
    call_id: ToolCallId,
    tool_id: &str,
    outcome: Result<&ToolResult, &ErrorDto>,
    occurred_at: TimestampDto,
) -> DtoResult<ToolResultEvidenceDto> {
    let kind = ToolResultKindDto::parse(tool_id)?;
    let content = match outcome {
        Ok(result) => canonical_tool_result_document(result),
        Err(error) => canonical_failure_document(terminal_error_tag(error), error.code()),
    };
    ToolResultEvidenceDto::new(session_id, run_id, call_id, kind, content, occurred_at)
}

/// Maps a terminal error to its closed durable document discriminator.
fn terminal_error_tag(error: &ErrorDto) -> &'static str {
    match terminal_status_for_error(error) {
        intention_domain::ToolLifecycleStatusDto::Cancelled => "cancelled",
        intention_domain::ToolLifecycleStatusDto::ExternalEffectUnknown => {
            "external_effect_unknown"
        }
        _ => "failed",
    }
}

/// Serializes one typed terminal failure into its canonical durable document.
fn canonical_failure_document(tag: &str, code: &str) -> String {
    let mut document = String::new();
    document.push_str("{\"result\":\"");
    document.push_str(tag);
    document.push_str("\",\"value\":{\"code\":");
    write_json_string(&mut document, code);
    document.push_str("}}");
    document
}

/// Serializes one typed result into its bounded canonical durable document.
///
/// The document mirrors the typed result wire shape (`{"result":kind,"value":…}`).
/// Oversized text, path lists, and match lists are cut in place with an honest
/// `truncated` marker so the document always fits the durable bound.
fn canonical_tool_result_document(result: &ToolResult) -> String {
    let mut document = String::new();
    match result {
        ToolResult::Read(value) | ToolResult::Execute(value) => {
            let tag = match result {
                ToolResult::Read(_) => "read",
                _ => "execute",
            };
            document.push_str("{\"result\":\"");
            document.push_str(tag);
            document.push_str("\",\"value\":{\"text\":");
            let budget = MAX_DURABLE_TOOL_RESULT_BYTES
                .saturating_sub(document.len() + DOCUMENT_RESERVE_BYTES)
                .max(DOCUMENT_RESERVE_BYTES)
                / TRUNCATED_TEXT_BUDGET_DIVISOR;
            let complete = write_bounded_json_string(&mut document, value.text.as_str(), budget);
            document.push_str(",\"truncated\":");
            document.push_str(if value.truncated || !complete {
                "true"
            } else {
                "false"
            });
            document.push_str("}}");
        }
        ToolResult::Glob(value) => {
            document.push_str("{\"result\":\"glob\",\"value\":{\"paths\":[");
            let mut emitted = 0_usize;
            for path in &value.paths {
                if !append_fitting_json_string(&mut document, path.as_str(), &mut emitted) {
                    break;
                }
            }
            finish_truncated_array(
                &mut document,
                value.truncated || emitted < value.paths.len(),
            );
        }
        ToolResult::Grep(value) => {
            document.push_str("{\"result\":\"grep\",\"value\":{\"matches\":[");
            let mut emitted = 0_usize;
            for matched in &value.matches {
                let mut probe = String::new();
                probe.push_str("{\"path\":");
                write_json_string(&mut probe, matched.path.as_str());
                probe.push_str(",\"line\":");
                probe.push_str(&matched.line.to_string());
                probe.push_str(",\"column\":");
                probe.push_str(&matched.column.to_string());
                probe.push_str(",\"fragment\":");
                write_json_string(&mut probe, matched.fragment.as_str());
                probe.push('}');
                if document.len() + probe.len() + DOCUMENT_RESERVE_BYTES
                    > MAX_DURABLE_TOOL_RESULT_BYTES
                {
                    break;
                }
                if emitted > 0 {
                    document.push(',');
                }
                document.push_str(&probe);
                emitted += 1;
            }
            finish_truncated_array(
                &mut document,
                value.truncated || emitted < value.matches.len(),
            );
        }
        ToolResult::Write(value) | ToolResult::Edit(value) => {
            let tag = match result {
                ToolResult::Write(_) => "write",
                _ => "edit",
            };
            document.push_str("{\"result\":\"");
            document.push_str(tag);
            document.push_str("\",\"value\":{\"bytes\":");
            document.push_str(&value.bytes.to_string());
            document.push_str("}}");
        }
    }
    document
}

/// Closes one result array document with its honest truncation marker.
fn finish_truncated_array(document: &mut String, truncated: bool) {
    document.push_str("],\"truncated\":");
    document.push_str(if truncated { "true" } else { "false" });
    document.push_str("}}");
}

/// Appends one escaped JSON string element when it still fits the durable bound.
///
/// Returns whether the element was appended.
fn append_fitting_json_string(document: &mut String, value: &str, emitted: &mut usize) -> bool {
    let mut probe = String::new();
    write_json_string(&mut probe, value);
    if document.len() + probe.len() + DOCUMENT_RESERVE_BYTES > MAX_DURABLE_TOOL_RESULT_BYTES {
        return false;
    }
    if *emitted > 0 {
        document.push(',');
    }
    document.push_str(&probe);
    *emitted += 1;
    true
}

/// Writes one compact JSON string with serde-compatible escaping.
fn write_json_string(document: &mut String, value: &str) {
    document.push('"');
    for character in value.chars() {
        push_escaped_character(document, character);
    }
    document.push('"');
}

/// Writes one JSON string cut to the raw-byte budget at a character boundary.
///
/// Returns whether the entire value fit without cutting.
fn write_bounded_json_string(document: &mut String, value: &str, raw_budget: usize) -> bool {
    document.push('"');
    let mut raw = 0_usize;
    let mut complete = true;
    for character in value.chars() {
        if raw + character.len_utf8() > raw_budget {
            complete = false;
            break;
        }
        raw += character.len_utf8();
        push_escaped_character(document, character);
    }
    document.push('"');
    complete
}

fn push_escaped_character(document: &mut String, character: char) {
    match character {
        '"' => document.push_str("\\\""),
        '\\' => document.push_str("\\\\"),
        '\u{8}' => document.push_str("\\b"),
        '\t' => document.push_str("\\t"),
        '\n' => document.push_str("\\n"),
        '\u{c}' => document.push_str("\\f"),
        '\r' => document.push_str("\\r"),
        control if (control as u32) < 0x20 => {
            const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
            let code = control as u32;
            document.push_str("\\u00");
            document.push(HEX_DIGITS[(code >> 4) as usize] as char);
            document.push(HEX_DIGITS[(code & 0xf) as usize] as char);
        }
        other => document.push(other),
    }
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
    use super::{
        MAX_DURABLE_TOOL_RESULT_BYTES, canonical_failure_document, canonical_tool_result_document,
        durable_tool_result_evidence, expected_tool_id, result_phase_context, terminal_error_tag,
        terminal_status_for_error,
    };
    use intention_domain::ToolLifecycleStatusDto;
    use intention_hooks::{Phase, PhaseContext};
    use intention_tools::{ToolInput, ToolResult};
    use intention_types::{ErrorDto, RunId, SessionId, TimestampDto, ToolCallId};

    fn bounded(value: &str) -> intention_tools::BoundedText {
        intention_tools::BoundedText::new(value)
            .unwrap_or_else(|_| unreachable!("fixture tool text is bounded"))
    }

    fn relative(value: &str) -> intention_types::WorkspaceRelativePathDto {
        intention_types::WorkspaceRelativePathDto::parse(value)
            .unwrap_or_else(|_| unreachable!("fixture relative path is valid"))
    }

    fn fixture_time() -> TimestampDto {
        TimestampDto::from_unix_seconds(1)
            .unwrap_or_else(|_| unreachable!("fixture timestamp is valid"))
    }

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

    #[test]
    fn canonical_documents_cover_each_typed_result_family() {
        let read = ToolResult::Read(intention_tools::TextResult {
            text: bounded("hello"),
            truncated: false,
        });
        assert_eq!(
            canonical_tool_result_document(&read),
            "{\"result\":\"read\",\"value\":{\"text\":\"hello\",\"truncated\":false}}"
        );
        let execute = ToolResult::Execute(intention_tools::TextResult {
            text: bounded("done"),
            truncated: true,
        });
        assert_eq!(
            canonical_tool_result_document(&execute),
            "{\"result\":\"execute\",\"value\":{\"text\":\"done\",\"truncated\":true}}"
        );
        let glob = ToolResult::Glob(intention_tools::PathsResult {
            paths: vec![relative("src/a.rs"), relative("src/b.rs")],
            truncated: false,
        });
        assert_eq!(
            canonical_tool_result_document(&glob),
            "{\"result\":\"glob\",\"value\":{\"paths\":[\"src/a.rs\",\"src/b.rs\"],\"truncated\":false}}"
        );
        let grep = ToolResult::Grep(intention_tools::GrepResult {
            matches: vec![intention_tools::GrepMatch {
                path: relative("src/a.rs"),
                line: 3,
                column: 5,
                fragment: bounded("needle"),
            }],
            truncated: false,
        });
        assert_eq!(
            canonical_tool_result_document(&grep),
            "{\"result\":\"grep\",\"value\":{\"matches\":[{\"path\":\"src/a.rs\",\"line\":3,\"column\":5,\"fragment\":\"needle\"}],\"truncated\":false}}"
        );
        let write = ToolResult::Write(intention_tools::WriteResult { bytes: 17 });
        assert_eq!(
            canonical_tool_result_document(&write),
            "{\"result\":\"write\",\"value\":{\"bytes\":17}}"
        );
        let edit = ToolResult::Edit(intention_tools::WriteResult { bytes: 2 });
        assert_eq!(
            canonical_tool_result_document(&edit),
            "{\"result\":\"edit\",\"value\":{\"bytes\":2}}"
        );
    }

    #[test]
    fn canonical_documents_escape_json_special_characters() {
        let read = ToolResult::Read(intention_tools::TextResult {
            text: bounded("quote\"back\\slash\nend\u{1}"),
            truncated: false,
        });
        assert_eq!(
            canonical_tool_result_document(&read),
            "{\"result\":\"read\",\"value\":{\"text\":\"quote\\\"back\\\\slash\\nend\\u0001\",\"truncated\":false}}"
        );
    }

    #[test]
    fn oversized_text_truncates_within_the_durable_bound() {
        let read = ToolResult::Read(intention_tools::TextResult {
            text: bounded(&"x".repeat(1024 * 1024)),
            truncated: false,
        });
        let document = canonical_tool_result_document(&read);
        assert!(document.len() <= MAX_DURABLE_TOOL_RESULT_BYTES);
        assert!(document.starts_with("{\"result\":\"read\",\"value\":{\"text\":\""));
        assert!(document.ends_with("\"truncated\":true}}"));
    }

    #[test]
    fn oversized_path_lists_truncate_within_the_durable_bound() {
        let paths = (0..20_000)
            .map(|index| relative(&format!("dir-{index}/long-file-name-{index}.txt")))
            .collect();
        let glob = ToolResult::Glob(intention_tools::PathsResult {
            paths,
            truncated: false,
        });
        let document = canonical_tool_result_document(&glob);
        assert!(document.len() <= MAX_DURABLE_TOOL_RESULT_BYTES);
        assert!(document.starts_with("{\"result\":\"glob\",\"value\":{\"paths\":[\""));
        assert!(document.ends_with("\"truncated\":true}}"));
    }

    #[test]
    fn oversized_match_lists_truncate_within_the_durable_bound() {
        let matches = (0..512)
            .map(|index| intention_tools::GrepMatch {
                path: relative(&format!("dir-{index}/file.rs")),
                line: index + 1,
                column: 1,
                fragment: bounded(&"y".repeat(64 * 1024)),
            })
            .collect();
        let grep = ToolResult::Grep(intention_tools::GrepResult {
            matches,
            truncated: false,
        });
        let document = canonical_tool_result_document(&grep);
        assert!(document.len() <= MAX_DURABLE_TOOL_RESULT_BYTES);
        assert!(document.starts_with("{\"result\":\"grep\",\"value\":{\"matches\":[{\"path\":"));
        assert!(document.ends_with("\"truncated\":true}}"));
    }

    #[test]
    fn failure_documents_carry_the_terminal_discriminator_and_code() {
        assert_eq!(
            canonical_failure_document("failed", "workspace_path_unavailable"),
            "{\"result\":\"failed\",\"value\":{\"code\":\"workspace_path_unavailable\"}}"
        );
        assert_eq!(
            terminal_error_tag(&ErrorDto::validation("tool_cancelled", "x")),
            "cancelled"
        );
        assert_eq!(
            terminal_error_tag(&ErrorDto::validation(
                "tool_execute_external_effect_unknown",
                "x"
            )),
            "external_effect_unknown"
        );
        assert_eq!(
            terminal_error_tag(&ErrorDto::validation("other", "x")),
            "failed"
        );
    }

    #[test]
    fn durable_evidence_binds_exact_identity_kind_and_time() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let call_id = ToolCallId::new();
        let read = ToolResult::Read(intention_tools::TextResult {
            text: bounded("hello"),
            truncated: false,
        });
        let evidence = durable_tool_result_evidence(
            session_id,
            run_id,
            call_id,
            "read",
            Ok(&read),
            fixture_time(),
        )
        .unwrap_or_else(|_| unreachable!("typed result evidence is valid"));
        assert_eq!(evidence.session_id(), session_id);
        assert_eq!(evidence.run_id(), run_id);
        assert_eq!(evidence.call_id(), call_id);
        assert_eq!(evidence.kind(), intention_storage::ToolResultKindDto::Read);
        assert_eq!(
            evidence.content(),
            "{\"result\":\"read\",\"value\":{\"text\":\"hello\",\"truncated\":false}}"
        );
        assert_eq!(evidence.occurred_at(), fixture_time());
        let cancelled = durable_tool_result_evidence(
            session_id,
            run_id,
            call_id,
            "execute",
            Err(&ErrorDto::validation("tool_cancelled", "x")),
            fixture_time(),
        )
        .unwrap_or_else(|_| unreachable!("failure evidence is valid"));
        assert_eq!(
            cancelled.kind(),
            intention_storage::ToolResultKindDto::Execute
        );
        assert_eq!(
            cancelled.content(),
            "{\"result\":\"cancelled\",\"value\":{\"code\":\"tool_cancelled\"}}"
        );
        assert!(
            durable_tool_result_evidence(
                session_id,
                run_id,
                call_id,
                "unknown",
                Ok(&read),
                fixture_time(),
            )
            .is_err()
        );
    }
}
