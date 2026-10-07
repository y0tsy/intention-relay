//! Durable M3 composition root for the daemon application facade.
//!
//! Only this crate selects SQLite. The public facade exposes protocol DTOs;
//! database resources, locations, configuration text, and committed-event
//! publication stay private.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use intention_application::{
    ApplicationService, CreateSessionWorkflowInputDto, InvokeLocalToolInputDto,
    ModelRunDispatchPort, ScheduleModelRunDto, SendUserTurnWorkflowInputDto,
    ToolResultPublicationPort, WorkspaceBoundaryPort,
};
#[cfg(test)]
use intention_config::ConfigPathDto;
use intention_config::{
    ConfigPathResolver, ConfigSnapshotDto, ConfigSourceDto, ProviderKindDto, RawConfigInputDto,
    ResolvedConfigDto, StartupProviderMaterial,
};
use intention_domain::run_status_is_terminal;
use intention_hooks::{
    Hook, Outcome as HookOutcome, Phase, PhaseContext, Registry as HookRegistry,
};
use intention_proto::RunStatusDto;
#[cfg(test)]
use intention_proto::SendUserTurnOutcomeDto;
use intention_proto::{
    ConfigRevisionId, CorrelationIdDto, DtoResult, ErrorDto, RunId, SchemaVersionDto, SessionId,
    TimestampDto,
};
#[cfg(test)]
use intention_proto::{CreateSessionCommandDto, RunModeDto, WorkspaceRootDto};
use intention_proto::{
    DaemonHealthDto, DaemonReadinessDto, ProtocolAcceptedDto, ProtocolAcceptedResultDto,
    ProtocolCommandDto, ProtocolCommandResultDto, ProtocolQueryDto, ProtocolQueryResultDto,
    SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
};
#[cfg(test)]
use intention_proto::{ProjectId, WorkspaceId};
use intention_providers::GenericChatDriver;
use intention_providers::OpenRouterDriver;
use intention_providers::{ModelCancellationSignal, ModelExecutionDriver};
#[cfg(any(test, feature = "test-support"))]
use intention_providers::{ModelCapabilitiesDto, ModelDriver, ModelEventStream};
use intention_runtime::{
    ModelRunCommitObserver, ModelRunExecutionInputDto, ModelRunExecutionOutcomeDto,
    ModelRunExecutionService, ModelTimePort, ToolExecutionPort, fail_starting_run,
};
use intention_storage::{
    RecoverUnfinishedRunsInputDto, StorageRepositoryDto, ToolResultEvidenceDto,
};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
#[cfg(test)]
use intention_tools::ToolResult;
use intention_tools::{CancellationSignal, ToolInput};
use intention_workspace::WorkspaceRoot;

const SCHEMA_VERSION: SchemaVersionDto = intention_proto::CURRENT_DTO_SCHEMA_VERSION;
const PROTOCOL_VERSION: intention_proto::ProtocolVersionDto =
    intention_proto::CURRENT_PROTOCOL_VERSION;
/// The single live configuration snapshot schema (intention-config current schema).
const CONFIG_SCHEMA_VERSION: SchemaVersionDto = SchemaVersionDto::new(1, 0);
const DATABASE_FILENAME: &str = "intention-relay.sqlite";
/// The retained bounded size of a current-state snapshot's recent transcript.
const SESSION_SNAPSHOT_MESSAGES: u32 = 256;

/// The terminal outcome of one facade local tool invocation.
///
/// Re-exported for the daemon host, which maps the outcome onto the
/// model-visible tool-result fact without depending on the application crate
/// directly.
pub use intention_application::LocalToolInvocationOutcomeDto;

/// Public M3 daemon application facade over a private durable composition.
#[derive(Clone)]
pub struct DaemonApplicationFacade {
    inner: Arc<FacadeInner>,
}

struct FacadeInner {
    repository: SqliteStorageRepository,
    config_snapshot: ConfigSnapshotDto,
    _selected_provider: SelectedProvider,
    dispatch: PrivateModelRunDispatch,
    command_gate: Mutex<()>,
    tool_cancellations: Mutex<HashMap<(SessionId, RunId), LocalToolCancellationEntry>>,
}

enum SelectedProvider {
    OpenRouter(OpenRouterDriver),
    GenericChat(GenericChatDriver),
    #[cfg(any(test, feature = "test-support"))]
    TestSupport(Arc<dyn ModelExecutionDriver + Send + Sync>),
}

#[cfg(any(test, feature = "test-support"))]
struct TestSupportUnconfiguredDriver;

#[cfg(any(test, feature = "test-support"))]
impl ModelDriver for TestSupportUnconfiguredDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(false, false, false, false, false, false)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl ModelExecutionDriver for TestSupportUnconfiguredDriver {
    fn execute(
        &self,
        _request: intention_providers::ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        Box::pin(futures_util::stream::empty())
    }
}

impl SelectedProvider {
    fn from_startup_material(material: StartupProviderMaterial) -> DtoResult<Self> {
        match material.safe_resolved().provider().kind() {
            ProviderKindDto::Openrouter => {
                OpenRouterDriver::from_startup_material(material).map(Self::OpenRouter)
            }
            ProviderKindDto::GenericChatCompletionApi => {
                GenericChatDriver::from_startup_material(material).map(Self::GenericChat)
            }
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    fn for_test_support(driver: Arc<dyn ModelExecutionDriver + Send + Sync>) -> Self {
        Self::TestSupport(driver)
    }

    fn driver(&self) -> &(dyn ModelExecutionDriver + Send + Sync) {
        match self {
            Self::OpenRouter(driver) => driver,
            Self::GenericChat(driver) => driver,
            #[cfg(any(test, feature = "test-support"))]
            Self::TestSupport(driver) => driver.as_ref(),
        }
    }

    const fn safe_kind(&self) -> Option<ProviderKindDto> {
        match self {
            Self::OpenRouter(driver) => {
                let _ = driver;
                Some(ProviderKindDto::Openrouter)
            }
            Self::GenericChat(driver) => {
                let _ = driver;
                Some(ProviderKindDto::GenericChatCompletionApi)
            }
            #[cfg(any(test, feature = "test-support"))]
            Self::TestSupport(driver) => {
                let _ = driver;
                None
            }
        }
    }
}

#[derive(Default)]
struct PrivateModelRunDispatch {
    #[cfg(test)]
    admitted: Mutex<Vec<ScheduleModelRunDto>>,
}

impl PrivateModelRunDispatch {
    #[cfg(test)]
    fn admitted(&self) -> DtoResult<Vec<ScheduleModelRunDto>> {
        self.admitted
            .lock()
            .map(|admitted| admitted.clone())
            .map_err(|_| {
                ErrorDto::unavailable(
                    "daemon_dispatch_unavailable",
                    "daemon model-run dispatch is unavailable",
                )
            })
    }
}

impl ModelRunDispatchPort for PrivateModelRunDispatch {
    fn dispatch_model_run(&self, input: ScheduleModelRunDto) -> DtoResult<()> {
        // Lane E admits a post-commit scheduling payload only. Provider execution,
        // including an outbound request, remains owned by the future daemon host.
        #[cfg(not(test))]
        let _input = input;
        #[cfg(test)]
        self.admitted
            .lock()
            .map_err(|_| {
                ErrorDto::unavailable(
                    "daemon_dispatch_unavailable",
                    "daemon model-run dispatch is unavailable",
                )
            })?
            .push(input);
        Ok(())
    }
}

struct SafeWorkspaceBoundary;
impl WorkspaceBoundaryPort for SafeWorkspaceBoundary {
    fn resolve(&self, workspace: &WorkspaceRoot) -> DtoResult<WorkspaceRoot> {
        // The session's declared anchor is re-authorized between the two
        // workspace-resolution phases, and the invocation addresses only the
        // root this boundary returns.
        workspace.rebind()
    }
}

struct SafeObserverHook;
impl Hook for SafeObserverHook {
    fn id(&self) -> &'static str {
        "safe-production-observer"
    }
    fn phases(&self) -> &'static [Phase] {
        static P: [Phase; 1] = [Phase::BeforeToolInvocation];
        &P
    }
    fn priority(&self) -> u32 {
        0
    }
    fn run(&self, _: &PhaseContext) -> DtoResult<HookOutcome> {
        Ok(HookOutcome::Continue)
    }
}

/// The workspace hook owner: it validates the two workspace-resolution phases
/// around the application's exact `resolve_path` boundary.
struct WorkspaceResolutionHook;
impl Hook for WorkspaceResolutionHook {
    fn id(&self) -> &'static str {
        "workspace-resolution-owner"
    }
    fn phases(&self) -> &'static [Phase] {
        static P: [Phase; 2] = [
            Phase::BeforeWorkspaceResolution,
            Phase::AfterWorkspaceResolution,
        ];
        &P
    }
    fn priority(&self) -> u32 {
        0
    }
    fn run(&self, _: &PhaseContext) -> DtoResult<HookOutcome> {
        Ok(HookOutcome::Continue)
    }
}

fn production_hooks() -> DtoResult<HookRegistry> {
    let mut registry = HookRegistry::new();
    registry
        .register(Box::new(SafeObserverHook))
        .and_then(|()| registry.register(Box::new(WorkspaceResolutionHook)))
        .map_err(|_error| {
            ErrorDto::validation(
                "production_hook_registration_failed",
                "production hook registration failed",
            )
        })?;
    Ok(registry)
}

/// Run-scoped interruption shared between daemon-host interrupts and admitted local tools.
struct LocalToolCancellationEntry {
    signal: CancellationSignal,
    /// Local invocations currently executing against this exact run.
    inflight: usize,
}

impl DaemonApplicationFacade {
    /// Executes one explicit local tool call through the one-transaction path
    /// without publishing its committed rows.
    ///
    /// This API is an internal, caller-admitted single invocation and never
    /// starts a loop. The daemon host uses
    /// [`Self::invoke_local_tool_for_daemon_with_publication`] so every
    /// committed transcript row reaches the live subscribers.
    #[doc(hidden)]
    #[expect(
        clippy::too_many_arguments,
        reason = "The daemon bridge keeps the flat tool-invocation payload in one call."
    )]
    pub fn invoke_local_tool_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call_id: intention_proto::ToolCallId,
        tool_id: impl Into<String>,
        input: ToolInput,
        workspace: WorkspaceRoot,
        arguments_json: impl Into<String>,
    ) -> DtoResult<LocalToolInvocationOutcomeDto> {
        self.invoke_local_tool_for_daemon_with_publication(
            session_id,
            run_id,
            call_id,
            tool_id,
            input,
            workspace,
            arguments_json,
            &(),
        )
    }

    /// Executes one explicit local tool call through the one-transaction path
    /// and hands every committed transcript row to the publication boundary.
    ///
    /// Publication follows each row's own commit, so every published frame
    /// carries a committed value, and the application dispatches
    /// `AfterToolResultPublished` only after the terminal row was published.
    /// This API is an internal, caller-admitted single invocation and never
    /// starts a loop.
    #[doc(hidden)]
    #[expect(
        clippy::too_many_arguments,
        reason = "The daemon bridge keeps the flat tool-invocation payload in one call."
    )]
    pub fn invoke_local_tool_for_daemon_with_publication<P: ToolResultPublicationPort>(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call_id: intention_proto::ToolCallId,
        tool_id: impl Into<String>,
        input: ToolInput,
        workspace: WorkspaceRoot,
        arguments_json: impl Into<String>,
        publisher: &P,
    ) -> DtoResult<LocalToolInvocationOutcomeDto> {
        let cancellation = self.bind_local_tool_cancellation(session_id, run_id)?;
        let result = intention_application::ApplicationService::with_hooks(
            &self.inner.repository,
            production_hooks()?,
        )
        .with_workspace_boundary(SafeWorkspaceBoundary)
        .invoke_local_tool_with_publication(
            InvokeLocalToolInputDto::new(
                workspace,
                session_id,
                run_id,
                call_id,
                tool_id,
                input,
                now()?,
            )
            .with_arguments_json(arguments_json)
            .with_cancellation(cancellation),
            publisher,
        );
        self.release_local_tool_cancellation(session_id, run_id);
        result
    }

    /// Binds one local invocation to its run's shared interruption signal.
    ///
    /// The signal is fresh for every invocation, so an interruption reaches
    /// only the tool that is actually in flight and the next tool of the same
    /// continuing run starts unencumbered.
    fn bind_local_tool_cancellation(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<CancellationSignal> {
        let mut registry = self.inner.tool_cancellations.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_command_unavailable",
                "daemon command is unavailable",
            )
        })?;
        let entry =
            registry
                .entry((session_id, run_id))
                .or_insert_with(|| LocalToolCancellationEntry {
                    signal: CancellationSignal::new(),
                    inflight: 0,
                });
        entry.inflight += 1;
        let signal = entry.signal.clone();
        drop(registry);
        Ok(signal)
    }

    /// Releases one finished local invocation from its run's shared signal.
    ///
    /// The binding is dropped whenever no invocation remains in flight: an
    /// interruption applies to the operation it caught, and the run continues
    /// with a fresh signal for its next tool call.
    fn release_local_tool_cancellation(&self, session_id: SessionId, run_id: RunId) {
        if let Ok(mut registry) = self.inner.tool_cancellations.lock()
            && let Some(entry) = registry.get_mut(&(session_id, run_id))
        {
            entry.inflight = entry.inflight.saturating_sub(1);
            if entry.inflight == 0 {
                registry.remove(&(session_id, run_id));
            }
        }
    }
    /// Loads platform configuration, opens platform state storage, and recovers before ready.
    ///
    /// Raw TOML, credentials, and configuration paths remain inside this method's
    /// private loading boundary and are never included in public values or errors.
    ///
    /// # Errors
    ///
    /// Returns safe typed failures when platform configuration cannot be resolved,
    /// permission-checked, read, validated, persisted, or recovered.
    pub fn open_platform() -> DtoResult<Self> {
        let (config_snapshot, selected_provider) = load_platform_provider_configuration()?;
        Self::open_with_selected_provider(
            platform_database_location()?,
            config_snapshot,
            selected_provider,
        )
    }

    /// Opens a caller-provided absolute database exclusively for tests or controlled fixtures.
    ///
    /// # Errors
    ///
    /// Returns a safe typed storage or recovery error. The supplied local path is
    /// never retained in a public DTO or error.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn open_for_test_support_with_driver(
        database_location: impl AsRef<Path>,
        config_snapshot: ConfigSnapshotDto,
        driver: Arc<dyn ModelExecutionDriver + Send + Sync>,
    ) -> DtoResult<Self> {
        Self::open_with_selected_provider(
            database_location,
            config_snapshot,
            SelectedProvider::for_test_support(driver),
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn open_for_test_support(
        database_location: impl AsRef<Path>,
        config_snapshot: ConfigSnapshotDto,
    ) -> DtoResult<Self> {
        Self::open_for_test_support_with_driver(
            database_location,
            config_snapshot,
            Arc::new(TestSupportUnconfiguredDriver),
        )
    }

    #[cfg(test)]
    fn open_for_test(
        database_location: impl AsRef<Path>,
        config_snapshot: ConfigSnapshotDto,
    ) -> DtoResult<Self> {
        Self::open_with_selected_provider(
            database_location,
            config_snapshot,
            SelectedProvider::for_test_support(Arc::new(TestSupportUnconfiguredDriver)),
        )
    }

    /// Resolves the authoritative workspace root of one durable session.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error when the session is unknown or its declared
    /// workspace cannot be resolved.
    #[doc(hidden)]
    pub fn resolve_workspace_root_for_daemon(
        &self,
        session_id: SessionId,
    ) -> DtoResult<WorkspaceRoot> {
        let projection = self.inner.repository.load_session_projection(session_id)?;
        WorkspaceRoot::resolve(projection.workspace_root())
    }

    /// Reads the committed structured evidence of one tool call for the daemon
    /// host's publication path.
    ///
    /// The host verifies a committed tool-result row against this read before it
    /// broadcasts the row, so the durable structured view is read by production
    /// code rather than written only.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the committed row does not exist.
    #[doc(hidden)]
    pub fn load_tool_result_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call_id: intention_proto::ToolCallId,
    ) -> DtoResult<ToolResultEvidenceDto> {
        self.inner
            .repository
            .load_tool_result(session_id, run_id, call_id)
    }

    /// Executes one scheduled run through the privately selected provider
    /// driver with the mandatory tool executor.
    ///
    /// This bridge is provider-neutral and safe: it accepts only scheduling DTOs,
    /// cancellation, a time port, committed-observation evidence, and the tool
    /// executor. It does not expose provider SDKs, credentials, Tokio, or
    /// storage resources. Provider-emitted tool calls execute through the
    /// caller-supplied durable tool path.
    #[doc(hidden)]
    pub async fn execute_scheduled_model_run_for_daemon_with_tool_executor<Time>(
        &self,
        schedule: ScheduleModelRunDto,
        cancellation: ModelCancellationSignal,
        time: &Time,
        observer: &dyn ModelRunCommitObserver,
        tool_executor: &dyn ToolExecutionPort,
    ) -> DtoResult<ModelRunExecutionOutcomeDto>
    where
        Time: ModelTimePort + Sync,
    {
        ModelRunExecutionService::with_commit_observer(
            &self.inner.repository,
            self.inner._selected_provider.driver(),
            time,
            observer,
            tool_executor,
        )
        .execute(ModelRunExecutionInputDto::new(
            schedule.session_id(),
            schedule.run_id(),
            schedule.request().clone(),
            schedule.safe_config().clone(),
            cancellation,
        ))
        .await
    }

    /// Accepts an interruption for the exact active run and reaches its
    /// in-flight local tool invocation.
    ///
    /// The run stays active: the daemon host signals the matching execution
    /// task after this validation, the interrupted operation ends with a
    /// partial result and a context notice, and the model receives the next
    /// step in the same run.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error when the exact run is not active.
    #[doc(hidden)]
    pub fn interrupt_run_for_daemon_host(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let _gate = self.inner.command_gate.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_command_unavailable",
                "daemon command is unavailable",
            )
        })?;
        let accepted = ApplicationService::new(&self.inner.repository).interrupt_run(
            intention_proto::InterruptRunCommandDto::new(session_id, run_id),
        )?;
        // An invocation bound to this exact run observes the interruption and
        // ends with a partial result; the binding is released when that
        // invocation returns, so the continuing run's next tool starts fresh.
        if let Ok(mut registry) = self.inner.tool_cancellations.lock()
            && let Some(entry) = registry.get_mut(&(session_id, run_id))
        {
            entry.signal.cancel();
        }
        Ok(accepted)
    }

    /// Terminalizes one still-active run as durably `Failed` for the daemon
    /// task registry.
    ///
    /// An executor error must never leave a `Starting`/`Running` run without
    /// an owner: this bridge commits the terminal `Failed` run row.
    /// The failure code is the executor error's stable code, so deterministic
    /// bound and semantic failures (for example
    /// `reasoning_output_limit_exceeded`) become the durable failed outcome
    /// (PR24-012). Runs already terminal are a no-op.
    ///
    /// # Errors
    ///
    /// Returns the repository's typed error when the terminal run row cannot
    /// commit.
    #[doc(hidden)]
    pub fn fail_active_run_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
        failure_code: &str,
    ) -> DtoResult<()> {
        let _gate = self.inner.command_gate.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_command_unavailable",
                "daemon command is unavailable",
            )
        })?;
        let run = self
            .inner
            .repository
            .load_run_projection(session_id, run_id)?;
        if run_status_is_terminal(run.status()) {
            return Ok(());
        }
        self.inner
            .repository
            .finish_run(intention_storage::FinishRunInputDto::new(
                session_id,
                run_id,
                RunStatusDto::Failed,
                None,
                None,
                Some(failure_code.to_owned()),
                Some("the scheduled run execution failed".to_owned()),
                now()?,
            )?)?;
        if let Ok(mut registry) = self.inner.tool_cancellations.lock() {
            registry.remove(&(session_id, run_id));
        }
        Ok(())
    }

    /// Records a safe terminal scheduling failure for an exact unadmitted run.
    ///
    /// This private daemon-host bridge preserves the already accepted user turn
    /// when durable context reconstruction cannot produce executable work.
    #[doc(hidden)]
    pub fn fail_starting_run_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
        failure_code: &'static str,
    ) -> DtoResult<()> {
        let _gate = self.inner.command_gate.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_command_unavailable",
                "daemon command is unavailable",
            )
        })?;
        fail_starting_run(
            &self.inner.repository,
            session_id,
            run_id,
            failure_code,
            now()?,
        )?;
        Ok(())
    }

    /// Loads the authoritative current run projection for the private daemon host.
    #[doc(hidden)]
    pub fn load_run_projection_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<intention_proto::RunProjectionDto> {
        self.inner
            .repository
            .load_run_projection(session_id, run_id)
    }

    /// Loads one coherent current-state run snapshot for the private daemon host.
    ///
    /// The snapshot carries the current run projection and the bounded recent
    /// transcript rows of that run; a re-subscribing client receives current
    /// state and continues from live frames.
    #[doc(hidden)]
    pub fn load_run_snapshot_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<intention_proto::RunSubscriptionSnapshotDto> {
        let run = self
            .inner
            .repository
            .load_run_projection(session_id, run_id)?;
        let messages = self.inner.repository.load_run_messages(
            session_id,
            run_id,
            SESSION_SNAPSHOT_MESSAGES,
        )?;
        intention_proto::RunSubscriptionSnapshotDto::new(run, messages)
    }

    /// Loads the committed transcript rows of one run for the private daemon host.
    #[doc(hidden)]
    pub fn load_run_messages_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
        limit: u32,
    ) -> DtoResult<Vec<intention_proto::MessageProjectionDto>> {
        self.inner
            .repository
            .load_run_messages(session_id, run_id, limit)
    }

    /// Builds the exact durable scheduling input for a current `Starting` run.
    #[doc(hidden)]
    pub fn schedule_starting_run_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<ScheduleModelRunDto> {
        ApplicationService::new(&self.inner.repository).schedule_starting_run(session_id, run_id)
    }

    /// Returns the currently active durable run when it is eligible for host admission.
    #[doc(hidden)]
    pub fn current_starting_run_for_daemon(
        &self,
        session_id: SessionId,
    ) -> DtoResult<Option<RunId>> {
        Ok(self
            .inner
            .repository
            .load_session_projection(session_id)?
            .active_run()
            .filter(|run| run.status() == RunStatusDto::Starting)
            .map(|run| run.run_id()))
    }

    fn open_with_selected_provider(
        database_location: impl AsRef<Path>,
        config_snapshot: ConfigSnapshotDto,
        selected_provider: SelectedProvider,
    ) -> DtoResult<Self> {
        if selected_provider
            .safe_kind()
            .is_some_and(|kind| kind != config_snapshot.resolved().provider().kind())
        {
            return Err(ErrorDto::validation(
                "invalid_selected_provider",
                "selected provider does not match configuration",
            ));
        }
        let location = SqliteDatabaseLocationDto::new(
            database_location.as_ref().to_string_lossy().into_owned(),
        )?;
        let repository = SqliteStorageRepository::open(location)?;
        repository.accept_configuration_revision(config_snapshot.clone())?;
        let facade = Self {
            inner: Arc::new(FacadeInner {
                repository,
                config_snapshot,
                _selected_provider: selected_provider,
                dispatch: PrivateModelRunDispatch::default(),
                command_gate: Mutex::new(()),
                tool_cancellations: Mutex::new(HashMap::new()),
            }),
        };
        facade.recover_before_ready()?;
        Ok(facade)
    }

    #[cfg(test)]
    fn selected_provider_kind(&self) -> Option<ProviderKindDto> {
        self.inner._selected_provider.safe_kind()
    }

    /// Returns a credential-free ready health projection.
    #[must_use]
    pub const fn health(&self) -> DaemonHealthDto {
        DaemonHealthDto::new(SCHEMA_VERSION, PROTOCOL_VERSION, DaemonReadinessDto::Ready)
    }

    /// Dispatches a typed durable M3 query.
    #[must_use]
    pub fn query(&self, query: ProtocolQueryDto) -> ProtocolQueryResultDto {
        match query {
            ProtocolQueryDto::GetDaemonHealth => {
                ProtocolQueryResultDto::DaemonHealth(self.health())
            }
            ProtocolQueryDto::GetSessionSnapshot(query) => {
                self.session_snapshot(query.session_id(), None).map_or_else(
                    ProtocolQueryResultDto::Rejected,
                    ProtocolQueryResultDto::SessionSnapshot,
                )
            }
        }
    }

    /// Returns the current durable session snapshot; a re-subscribing client
    /// re-reads current state and continues live.
    #[must_use]
    pub fn subscribe(&self, command: SubscribeSessionCommandDto) -> SessionSubscriptionResponseDto {
        match self.session_snapshot(command.session_id(), command.run_id()) {
            Ok(snapshot) => SessionSubscriptionResponseDto::Snapshot(snapshot),
            Err(error) => SessionSubscriptionResponseDto::Error(error),
        }
    }

    /// Loads one coherent current-state session snapshot.
    ///
    /// Recent transcript rows are bounded by the retained delivery bound; a
    /// run-scoped request returns that run's rows instead of the session tail.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the session projection or its
    /// transcript rows cannot be read, or when a run-scoped request names a run
    /// that is unknown or belongs to another session.
    pub fn session_snapshot(
        &self,
        session_id: SessionId,
        run_id: Option<RunId>,
    ) -> DtoResult<intention_proto::SessionSnapshotDto> {
        let projection = self.inner.repository.load_session_projection(session_id)?;
        let messages = match run_id {
            Some(run_id) => self.inner.repository.load_run_messages(
                session_id,
                run_id,
                SESSION_SNAPSHOT_MESSAGES,
            )?,
            None => self
                .inner
                .repository
                .load_recent_messages(session_id, SESSION_SNAPSHOT_MESSAGES)?,
        };
        intention_proto::SessionSnapshotDto::with_projection(
            SCHEMA_VERSION,
            session_id,
            projection,
            messages,
        )
    }

    /// Dispatches a durable M3 command.
    #[must_use]
    pub fn command(&self, command: ProtocolCommandDto) -> ProtocolCommandResultDto {
        let result = self.command_result(command);
        match result {
            Ok(result) => ProtocolCommandResultDto::Accepted(ProtocolAcceptedDto::with_result(
                CorrelationIdDto::new(),
                result,
            )),
            Err(error) => ProtocolCommandResultDto::Rejected(error),
        }
    }

    fn command_result(&self, command: ProtocolCommandDto) -> DtoResult<ProtocolAcceptedResultDto> {
        let _gate = self.inner.command_gate.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_command_unavailable",
                "daemon command is unavailable",
            )
        })?;
        let timestamp = now()?;
        let result = match command {
            ProtocolCommandDto::CreateSession(command) => {
                ApplicationService::new(&self.inner.repository)
                    .create_session(CreateSessionWorkflowInputDto::new(command, timestamp))?
            }
            ProtocolCommandDto::SendUserTurn(command) => {
                let proposed_run_id = RunId::new();
                ApplicationService::new(&self.inner.repository).send_user_turn_and_schedule(
                    command,
                    SendUserTurnWorkflowInputDto::new(
                        proposed_run_id,
                        self.inner.config_snapshot.clone(),
                        timestamp,
                    ),
                    &self.inner.dispatch,
                )?
            }
            ProtocolCommandDto::RemoveTurn(command) => {
                ApplicationService::new(&self.inner.repository).remove_turn(command, timestamp)?
            }
            ProtocolCommandDto::InterruptRun(_) => {
                return Err(ErrorDto::validation(
                    "invalid_interrupt_dispatch",
                    "run interrupts use the daemon host interrupt path",
                ));
            }
            ProtocolCommandDto::SubscribeSession(_) => {
                return Err(ErrorDto::validation(
                    "invalid_subscription_dispatch",
                    "session subscriptions use the dedicated protocol response",
                ));
            }
        };
        Ok(result)
    }

    fn recover_before_ready(&self) -> DtoResult<()> {
        let _interrupted = self
            .inner
            .repository
            .recover_unfinished_runs(RecoverUnfinishedRunsInputDto::new(now()?))?;
        Ok(())
    }
}

fn load_platform_provider_configuration() -> DtoResult<(ConfigSnapshotDto, SelectedProvider)> {
    load_provider_configuration(ConfigPathResolver::resolve(None)?)
}

fn load_provider_configuration(
    source: ConfigSourceDto,
) -> DtoResult<(ConfigSnapshotDto, SelectedProvider)> {
    #[cfg(unix)]
    intention_config::ensure_user_only_permissions(source.path())?;
    let raw_toml = fs::read_to_string(source.path().as_str()).map_err(|_| {
        ErrorDto::unavailable(
            "daemon_configuration_read_unavailable",
            "daemon configuration could not be read",
        )
    })?;
    let material =
        ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(raw_toml, source))?;
    let snapshot = ConfigSnapshotDto::new(
        CONFIG_SCHEMA_VERSION,
        ConfigRevisionId::new(),
        now()?,
        material.safe_resolved().clone(),
    )?;
    let selected_provider = SelectedProvider::from_startup_material(material)?;
    Ok((snapshot, selected_provider))
}

#[cfg(test)]
fn load_config_snapshot(source: ConfigSourceDto) -> DtoResult<ConfigSnapshotDto> {
    load_provider_configuration(source).map(|(snapshot, _)| snapshot)
}

fn now() -> DtoResult<TimestampDto> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| {
            ErrorDto::unavailable("daemon_clock_unavailable", "daemon clock is unavailable")
        })?
        .as_secs();
    TimestampDto::from_unix_seconds(i64::try_from(seconds).map_err(|_| {
        ErrorDto::unavailable("daemon_clock_unavailable", "daemon clock is unavailable")
    })?)
}

fn platform_database_location() -> DtoResult<PathBuf> {
    let base = platform_state_directory()?;
    fs::create_dir_all(&base).map_err(|_| unavailable_storage())?;
    Ok(base.join(DATABASE_FILENAME))
}

fn platform_state_directory() -> DtoResult<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|path| path.join(".local/state"))
            })
            .map(|path| path.join("intention-relay"))
            .ok_or_else(unavailable_storage)
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|path| path.join("Library/Application Support/intention-relay"))
            .ok_or_else(unavailable_storage)
    }
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|path| path.join("intention-relay"))
            .ok_or_else(unavailable_storage)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err(unavailable_storage())
    }
}

fn unavailable_storage() -> ErrorDto {
    ErrorDto::unavailable(
        "daemon_storage_unavailable",
        "daemon durable storage is unavailable",
    )
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Composition internals use controlled durable fixtures."
    )]

    use super::*;

    use intention_domain::ToolResultStatusDto;
    use intention_proto::{
        GetSessionSnapshotQueryDto, MessageKindDto, MessageProjectionDto, SendUserTurnCommandDto,
    };
    use intention_storage::ConsumePendingUserTurnsInputDto;
    use tempfile::TempDir;

    fn test_facade() -> (TempDir, DaemonApplicationFacade) {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("facade.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        (directory, facade)
    }

    fn fixture_workspace_root() -> WorkspaceRootDto {
        WorkspaceRootDto::parse(
            std::env::temp_dir()
                .join("intention-composition-workspace")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("native fixture workspace is absolute")
    }

    fn fixture_config_snapshot() -> ConfigSnapshotDto {
        let source = ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-composition-fixture.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture configuration source is absolute"),
        );
        let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential\"",
            source,
        ))
        .expect("fixture configuration resolves");
        ConfigSnapshotDto::new(
            CONFIG_SCHEMA_VERSION,
            ConfigRevisionId::new(),
            TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid"),
            resolved,
        )
        .expect("fixture snapshot is credential-free")
    }

    fn create(facade: &DaemonApplicationFacade, session_id: SessionId) {
        let accepted = facade.command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                fixture_workspace_root(),
                RunModeDto::Build,
            ),
        ));
        assert!(matches!(accepted, ProtocolCommandResultDto::Accepted(_)));
    }

    /// Creates a workspace directory containing one named file.
    fn workspace_fixture(name: &str, content: &str) -> (TempDir, WorkspaceRoot) {
        let directory = TempDir::new().expect("temporary directory exists");
        fs::write(directory.path().join(name), content).expect("workspace fixture writes");
        let root = WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(directory.path().to_string_lossy().into_owned())
                .expect("fixture workspace dto is absolute"),
        )
        .expect("fixture workspace resolves");
        (directory, root)
    }

    /// Builds the read input for the shared `hello.txt` workspace fixture.
    fn read_hello_input() -> ToolInput {
        ToolInput::Read(intention_tools::ReadInput {
            path: intention_proto::WorkspaceRelativePathDto::parse("hello.txt")
                .expect("fixture path is valid"),
        })
    }

    /// Builds the exact read result the `hello.txt` fixture invocation produces.
    fn hello_read_result() -> ToolResult {
        ToolResult::Read(intention_tools::TextResult {
            text: intention_tools::BoundedText::new("hello").expect("fixture text"),
            truncated: false,
        })
    }

    /// Starts one durable run through a direct user turn and returns its identity.
    fn started_run(facade: &DaemonApplicationFacade, session_id: SessionId, label: &str) -> RunId {
        let accepted = send_user_turn(facade, session_id, label);
        let ProtocolCommandResultDto::Accepted(accepted) = accepted else {
            unreachable!("fixture turn is accepted, got {accepted:?}")
        };
        let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
            unreachable!("fixture turn has user-turn evidence")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
            unreachable!("first fixture turn starts")
        };
        run_id
    }

    fn send_user_turn(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        content: &str,
    ) -> ProtocolCommandResultDto {
        facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(
                session_id,
                intention_proto::IdempotencyKey::new(),
                content,
            )
            .expect("fixture user turn is valid"),
        ))
    }

    /// Reads the committed transcript rows of one run.
    fn run_messages(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        run_id: RunId,
    ) -> Vec<intention_proto::MessageProjectionDto> {
        facade
            .load_run_messages_for_daemon(session_id, run_id, u32::MAX)
            .expect("committed run transcript reads")
    }

    /// Reads the durable tool-result evidence of one exact invocation.
    fn tool_evidence(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        run_id: RunId,
        call_id: intention_proto::ToolCallId,
    ) -> ToolResultEvidenceDto {
        facade
            .inner
            .repository
            .load_tool_result(session_id, run_id, call_id)
            .expect("committed tool evidence reads")
    }

    #[test]
    fn direct_turn_admits_post_commit_dispatch_without_provider_execution() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("dispatch.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);

        let result = send_user_turn(&facade, session_id, "started turn");
        let ProtocolCommandResultDto::Accepted(accepted_result) = result else {
            unreachable!("direct turn is accepted")
        };
        let ProtocolAcceptedResultDto::SendUserTurn(accepted_turn) = accepted_result.result()
        else {
            unreachable!("direct turn returns user-turn evidence")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = accepted_turn.outcome() else {
            unreachable!("first turn starts a run")
        };

        let accepted = facade
            .inner
            .dispatch
            .admitted()
            .expect("dispatch recorder remains available");
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].session_id(), session_id);
        assert_eq!(accepted[0].run_id(), run_id);
        assert_eq!(
            accepted[0].safe_config(),
            &facade.inner.config_snapshot,
            "dispatch retains only the safe durable selection"
        );
        let messages = run_messages(&facade, session_id, run_id);
        assert_eq!(messages.len(), 1, "admission does not execute a provider");
        assert_eq!(messages[0].kind(), MessageKindDto::User);
        assert_eq!(messages[0].text(), "started turn");
        assert_eq!(
            facade
                .load_run_projection_for_daemon(session_id, run_id)
                .expect("the admitted run stays starting")
                .status(),
            RunStatusDto::Starting
        );
    }

    #[test]
    fn health_query_and_snapshot_query_cover_public_read_facade() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("queries.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        assert!(matches!(
            facade.query(ProtocolQueryDto::GetDaemonHealth),
            ProtocolQueryResultDto::DaemonHealth(health)
                if health.readiness() == DaemonReadinessDto::Ready
        ));
        let session_id = SessionId::new();
        create(&facade, session_id);
        assert!(matches!(
            facade.query(ProtocolQueryDto::GetSessionSnapshot(
                GetSessionSnapshotQueryDto::new(session_id)
            )),
            ProtocolQueryResultDto::SessionSnapshot(snapshot)
                if snapshot.session_id() == session_id
                    && snapshot.projection().session_id() == session_id
                    && snapshot.projection().active_run().is_none()
                    && snapshot.messages().is_empty()
        ));
        assert!(matches!(
            facade.query(ProtocolQueryDto::GetSessionSnapshot(
                GetSessionSnapshotQueryDto::new(SessionId::new())
            )),
            ProtocolQueryResultDto::Rejected(error)
                if error.code() == "storage_record_not_found"
        ));
    }

    #[test]
    fn schedule_starting_run_rejects_unknown_run_without_leaking_storage_details() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("schedule-error.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let error = facade
            .schedule_starting_run_for_daemon(SessionId::new(), RunId::new())
            .expect_err("unknown run cannot be scheduled");
        assert_eq!(error.code(), "run_model_context_unavailable");
        assert!(!error.to_string().contains("schedule-error.sqlite"));
    }

    #[test]
    fn facade_replay_of_the_same_user_turn_conflicts_without_duplicating_transcript_or_dispatch() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("idempotent-turn.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);
        let command = SendUserTurnCommandDto::new(
            session_id,
            intention_proto::IdempotencyKey::new(),
            "idempotent turn",
        )
        .expect("fixture user turn is valid");

        let initial = facade.command(ProtocolCommandDto::SendUserTurn(command.clone()));
        let ProtocolCommandResultDto::Accepted(initial) = initial else {
            unreachable!("the first user turn is accepted")
        };
        let ProtocolAcceptedResultDto::SendUserTurn(initial_turn) = initial.result() else {
            unreachable!("the first user turn returns user-turn evidence")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = initial_turn.outcome() else {
            unreachable!("the first user turn starts a run")
        };
        let committed = run_messages(&facade, session_id, run_id);

        // The facade proposes a fresh run identity for every command, so a
        // repeated command conflicts durably instead of starting a second run.
        let replay = facade.command(ProtocolCommandDto::SendUserTurn(command));
        let ProtocolCommandResultDto::Rejected(error) = replay else {
            unreachable!("a repeated command cannot start a second run")
        };
        assert_eq!(error.code(), "turn_idempotency_conflict");
        assert_eq!(
            run_messages(&facade, session_id, run_id),
            committed,
            "the rejected replay duplicates no transcript row"
        );
        assert_eq!(
            facade
                .inner
                .dispatch
                .admitted()
                .expect("dispatch recorder remains available")
                .len(),
            1,
            "the rejected replay never enters the dispatch seam"
        );
    }

    #[test]
    fn idempotent_turn_admission_reuses_the_durable_run_without_duplicate_rows() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let command = SendUserTurnCommandDto::new(
            session_id,
            intention_proto::IdempotencyKey::new(),
            "idempotent turn",
        )
        .expect("fixture user turn is valid");
        // The facade proposes one run identity per command, so replaying that
        // exact workflow input is the durable retry unit.
        let proposed_run_id = RunId::new();
        let workflow = SendUserTurnWorkflowInputDto::new(
            proposed_run_id,
            facade.inner.config_snapshot.clone(),
            TimestampDto::from_unix_seconds(2).expect("fixture timestamp is valid"),
        );
        let service = ApplicationService::new(&facade.inner.repository);

        let initial = service
            .send_user_turn_and_schedule(command.clone(), workflow.clone(), &facade.inner.dispatch)
            .expect("the first user turn is accepted");
        let replay = service
            .send_user_turn_and_schedule(command, workflow, &facade.inner.dispatch)
            .expect("the durable retry replays the committed acceptance");

        assert_eq!(replay, initial);
        assert_eq!(
            run_messages(&facade, session_id, proposed_run_id).len(),
            1,
            "the durable retry adds no second copy of the committed user message"
        );
    }

    #[test]
    fn pending_turn_stays_undispatched_and_promotes_into_the_run_transcript() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("queued-dispatch.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);

        let first = send_user_turn(&facade, session_id, "started turn");
        let ProtocolCommandResultDto::Accepted(first) = first else {
            unreachable!("the first user turn is accepted")
        };
        let ProtocolAcceptedResultDto::SendUserTurn(first_turn) = first.result() else {
            unreachable!("the first user turn returns user-turn evidence")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = first_turn.outcome() else {
            unreachable!("the first user turn starts a run")
        };
        assert_eq!(
            facade
                .inner
                .dispatch
                .admitted()
                .expect("dispatch recorder remains available")
                .len(),
            1
        );

        let pending = send_user_turn(&facade, session_id, "pending turn");
        let ProtocolCommandResultDto::Accepted(pending) = pending else {
            unreachable!("the pending user turn is accepted")
        };
        let ProtocolAcceptedResultDto::SendUserTurn(pending_turn) = pending.result() else {
            unreachable!("the pending user turn returns user-turn evidence")
        };
        assert!(matches!(
            pending_turn.outcome(),
            SendUserTurnOutcomeDto::Pending
        ));
        assert_eq!(
            facade
                .inner
                .dispatch
                .admitted()
                .expect("dispatch recorder remains available")
                .len(),
            1,
            "a pending message never enters the dispatch seam"
        );
        let snapshot = facade
            .session_snapshot(session_id, None)
            .expect("the pending session snapshot reads");
        assert_eq!(snapshot.projection().pending_turns().len(), 1);
        assert_eq!(
            snapshot.projection().pending_turns()[0].content(),
            "pending turn"
        );

        let promoted = facade
            .inner
            .repository
            .consume_pending_user_turns(ConsumePendingUserTurnsInputDto::new(
                session_id,
                run_id,
                TimestampDto::from_unix_seconds(3).expect("fixture timestamp is valid"),
            ))
            .expect("pending turns join the active run context");
        assert_eq!(promoted.len(), 1);
        assert_eq!(promoted[0].text(), "pending turn");
        let messages = run_messages(&facade, session_id, run_id);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].text(), "started turn");
        assert_eq!(messages[1].text(), "pending turn");
        assert!(
            facade
                .session_snapshot(session_id, None)
                .expect("the promoted session snapshot reads")
                .projection()
                .pending_turns()
                .is_empty(),
            "a promoted turn is no longer pending"
        );
    }

    #[test]
    fn daemon_host_bridges_read_the_exact_starting_run_and_interrupt_keeps_it_active() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "host bridge turn");

        assert_eq!(
            facade
                .current_starting_run_for_daemon(session_id)
                .expect("current run reads"),
            Some(run_id)
        );
        let schedule = facade
            .schedule_starting_run_for_daemon(session_id, run_id)
            .expect("durable model context schedules");
        assert_eq!(
            (schedule.session_id(), schedule.run_id()),
            (session_id, run_id)
        );
        let snapshot = facade
            .load_run_snapshot_for_daemon(session_id, run_id)
            .expect("current run snapshot reads");
        assert_eq!(snapshot.run().run_id(), run_id);
        assert_eq!(snapshot.run().status(), RunStatusDto::Starting);
        assert_eq!(snapshot.messages().len(), 1);
        assert_eq!(snapshot.messages()[0].text(), "host bridge turn");

        let interrupt = facade
            .interrupt_run_for_daemon_host(session_id, run_id)
            .expect("host interrupt is accepted");
        assert!(matches!(
            interrupt,
            ProtocolAcceptedResultDto::InterruptRun(accepted)
                if accepted.session_id() == session_id && accepted.run_id() == run_id
        ));
        assert_eq!(
            facade
                .current_starting_run_for_daemon(session_id)
                .expect("the interrupted run stays active"),
            Some(run_id)
        );
        assert_eq!(
            facade
                .load_run_snapshot_for_daemon(session_id, run_id)
                .expect("interrupted run snapshot reads")
                .run()
                .status(),
            RunStatusDto::Starting,
            "an interrupt never terminalizes an active run"
        );
    }

    #[test]
    fn provider_composition_selects_each_valid_kind_without_exposing_credentials() {
        for (filename, provider_toml, expected_kind) in [
            (
                "openrouter.toml",
                "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"selected-provider-secret\"",
                ProviderKindDto::Openrouter,
            ),
            (
                "generic.toml",
                "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"fixture\"\nendpoint = \"https://example.invalid/v1\"\ncredential = \"selected-provider-secret\"",
                ProviderKindDto::GenericChatCompletionApi,
            ),
        ] {
            let directory = TempDir::new().expect("temporary directory exists");
            let path = directory.path().join(filename);
            fs::write(&path, provider_toml).expect("fixture config writes");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                    .expect("fixture config permissions set");
            }
            let source = ConfigSourceDto::Explicit(
                ConfigPathDto::parse(path.to_string_lossy().into_owned())
                    .expect("fixture config path is absolute"),
            );

            let (snapshot, selected_provider) =
                load_provider_configuration(source).expect("valid provider config composes");
            let facade = DaemonApplicationFacade::open_with_selected_provider(
                directory.path().join("provider.sqlite"),
                snapshot.clone(),
                selected_provider,
            )
            .expect("selected provider remains owned by the facade");

            assert_eq!(facade.selected_provider_kind(), Some(expected_kind));
            assert_eq!(snapshot.resolved().provider().kind(), expected_kind);
            assert!(snapshot.resolved().provider().credential_configured());
            assert!(
                !snapshot
                    .resolved()
                    .safe_debug_projection()
                    .contains("selected-provider-secret")
            );
        }
    }

    #[test]
    fn provider_composition_rejects_invalid_configuration_without_secret_disclosure() {
        let directory = TempDir::new().expect("temporary directory exists");
        let path = directory.path().join("invalid.toml");
        fs::write(
            &path,
            "schema_version = 1\n[provider]\nkind = \"not-a-provider\"\nmodel = \"fixture\"\ncredential = \"invalid-provider-secret\"",
        )
        .expect("fixture config writes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .expect("fixture config permissions set");
        }
        let source = ConfigSourceDto::Explicit(
            ConfigPathDto::parse(path.to_string_lossy().into_owned())
                .expect("fixture config path is absolute"),
        );

        let result = load_provider_configuration(source);
        assert!(result.is_err());
        let error = result
            .err()
            .expect("invalid provider configuration must fail safely");

        assert_eq!(error.code(), "invalid_config_schema");
        assert!(!error.to_string().contains("invalid-provider-secret"));
    }

    #[test]
    fn platform_locations_and_configuration_failures_are_safe() {
        let state_directory =
            platform_state_directory().expect("test host has a platform state home");
        assert!(state_directory.is_absolute());
        assert_eq!(
            state_directory.file_name().and_then(|name| name.to_str()),
            Some("intention-relay")
        );

        let missing = TempDir::new()
            .expect("temporary directory exists")
            .path()
            .join("missing.toml");
        let source = ConfigSourceDto::Explicit(
            ConfigPathDto::parse(missing.to_string_lossy().into_owned())
                .expect("missing fixture config path is absolute"),
        );
        assert!(load_config_snapshot(source).is_err());
        assert!(
            DaemonApplicationFacade::open_for_test("relative.sqlite", fixture_config_snapshot())
                .is_err()
        );
    }

    #[test]
    fn subscriptions_handle_current_and_unknown_durable_sessions() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let reply = facade.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA_VERSION,
            session_id,
            RunModeDto::Build,
        ));
        let SessionSubscriptionResponseDto::Snapshot(snapshot) = reply else {
            unreachable!("a known session returns its current snapshot")
        };
        assert_eq!(snapshot.schema_version(), SCHEMA_VERSION);
        assert_eq!(snapshot.session_id(), session_id);
        assert_eq!(snapshot.projection().session_id(), session_id);
        assert!(snapshot.messages().is_empty());
        assert!(matches!(
            facade.subscribe(SubscribeSessionCommandDto::new(
                SCHEMA_VERSION,
                SessionId::new(),
                RunModeDto::Build,
            )),
            SessionSubscriptionResponseDto::Error(error)
                if error.code() == "storage_record_not_found"
        ));
    }

    #[test]
    fn subscription_replies_typed_error_for_unknown_run_scope() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        assert!(
            matches!(
                facade.subscribe(SubscribeSessionCommandDto::with_run_id(
                    SCHEMA_VERSION,
                    session_id,
                    Some(RunId::new()),
                    RunModeDto::Build,
                )),
                SessionSubscriptionResponseDto::Error(error)
                    if error.code() == "storage_record_not_found"
            ),
            "an unknown run scope is rejected without fabricating state"
        );

        let run_id = started_run(&facade, session_id, "scoped subscription");
        let scoped = facade.subscribe(SubscribeSessionCommandDto::with_run_id(
            SCHEMA_VERSION,
            session_id,
            Some(run_id),
            RunModeDto::Build,
        ));
        let SessionSubscriptionResponseDto::Snapshot(snapshot) = scoped else {
            unreachable!("a known run scope returns its current snapshot")
        };
        assert_eq!(snapshot.messages().len(), 1);
        assert_eq!(snapshot.messages()[0].run_id(), Some(run_id));
        assert_eq!(
            snapshot.projection().active_run().map(|run| run.run_id()),
            Some(run_id)
        );
    }

    #[test]
    fn daemon_host_failure_and_interrupt_bridges_are_safe() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "bridge");
        facade
            .interrupt_run_for_daemon_host(session_id, run_id)
            .expect("the interrupt is accepted");
        facade
            .fail_active_run_for_daemon(session_id, run_id, "fixture_failure")
            .expect("the active run terminalizes as failed");
        assert_eq!(
            facade
                .load_run_snapshot_for_daemon(session_id, run_id)
                .expect("the failed run reads")
                .run()
                .status(),
            RunStatusDto::Failed
        );
        let other = RunId::new();
        assert!(
            facade
                .fail_starting_run_for_daemon(session_id, other, "fixture_failure")
                .is_err()
        );
        assert!(
            facade
                .fail_active_run_for_daemon(session_id, other, "fixture_failure")
                .is_err()
        );
        assert!(
            facade
                .load_run_projection_for_daemon(session_id, other)
                .is_err()
        );
    }

    #[test]
    fn command_routes_remove_turn_and_rejects_interrupt_and_subscription() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("routing.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);
        let started = send_user_turn(&facade, session_id, "started");
        let ProtocolCommandResultDto::Accepted(a) = started else {
            unreachable!()
        };
        let ProtocolAcceptedResultDto::SendUserTurn(t) = a.result() else {
            unreachable!()
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = t.outcome() else {
            unreachable!()
        };
        let pending = send_user_turn(&facade, session_id, "pending");
        let ProtocolCommandResultDto::Accepted(a) = pending else {
            unreachable!()
        };
        let ProtocolAcceptedResultDto::SendUserTurn(t) = a.result() else {
            unreachable!()
        };
        let pending_turn_id = t.turn_id();
        let SendUserTurnOutcomeDto::Pending = t.outcome() else {
            unreachable!()
        };
        assert!(matches!(
            facade.command(ProtocolCommandDto::RemoveTurn(
                intention_proto::RemoveTurnCommandDto::new(session_id, pending_turn_id)
            )),
            ProtocolCommandResultDto::Accepted(_)
        ));
        // Interrupts never dispatch through the synchronous command path; the
        // daemon host owns reaching the in-flight operation.
        assert!(matches!(facade.command(ProtocolCommandDto::InterruptRun(
                intention_proto::InterruptRunCommandDto::new(session_id, run_id)
            )), ProtocolCommandResultDto::Rejected(error) if error.code() == "invalid_interrupt_dispatch"));
        facade
            .interrupt_run_for_daemon_host(session_id, run_id)
            .expect("host interrupt is accepted");
        assert_eq!(
            facade
                .load_run_snapshot_for_daemon(session_id, run_id)
                .expect("interrupted run snapshot reads")
                .run()
                .status(),
            RunStatusDto::Starting,
            "the interrupt leaves the run active for its continuation"
        );
        assert!(
            matches!(facade.command(ProtocolCommandDto::SubscribeSession(SubscribeSessionCommandDto::new(SCHEMA_VERSION, session_id, RunModeDto::Build))), ProtocolCommandResultDto::Rejected(error) if error.code() == "invalid_subscription_dispatch")
        );
    }

    #[test]
    fn duplicate_session_creation_is_rejected_and_committed_state_re_reads_stable() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let before = facade
            .session_snapshot(session_id, None)
            .expect("the created session snapshot reads");

        let duplicate = facade.command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                fixture_workspace_root(),
                RunModeDto::Build,
            ),
        ));
        let ProtocolCommandResultDto::Rejected(error) = duplicate else {
            unreachable!("a duplicate durable session identity is rejected")
        };
        assert_eq!(error.code(), "session_already_exists");
        let after = facade
            .session_snapshot(session_id, None)
            .expect("the rejected creation changes nothing");
        assert_eq!(after.projection(), before.projection());
        assert_eq!(after.messages(), before.messages());

        let accepted = facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(
                session_id,
                intention_proto::IdempotencyKey::new(),
                "committed",
            )
            .expect("fixture user turn is valid"),
        ));
        let ProtocolCommandResultDto::Accepted(accepted) = accepted else {
            unreachable!("the committed user turn is accepted")
        };
        let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
            unreachable!("the committed user turn returns user-turn evidence")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
            unreachable!("the committed user turn starts a run")
        };
        let committed = run_messages(&facade, session_id, run_id);
        assert_eq!(committed.len(), 1);
        assert_eq!(committed[0].text(), "committed");
        assert_eq!(
            run_messages(&facade, session_id, run_id),
            committed,
            "committed transcript rows re-read without duplication"
        );
    }

    #[test]
    fn selected_provider_rejects_configuration_kind_mismatch() {
        let result = DaemonApplicationFacade::open_with_selected_provider(
            TempDir::new().expect("temporary directory exists").path().join("mismatch.sqlite"),
            fixture_config_snapshot(),
            SelectedProvider::GenericChat(
                GenericChatDriver::from_startup_material(
                    ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
                        "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"fixture\"\nendpoint = \"https://example.invalid/v1\"\ncredential = \"fixture\"",
                        ConfigSourceDto::Explicit(ConfigPathDto::parse(
                            std::env::temp_dir().join("mismatch.toml").to_string_lossy().into_owned(),
                        ).expect("fixture path is absolute")),
                    )).expect("fixture material parses"),
                ).expect("generic provider builds"),
            ),
        );
        let error = match result {
            Ok(_) => unreachable!("a kind mismatch must not open the facade"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "invalid_selected_provider");
    }

    #[test]
    fn command_rejects_turn_for_unknown_session() {
        let (_directory, facade) = test_facade();
        let result = facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(
                SessionId::new(),
                intention_proto::IdempotencyKey::new(),
                "turn",
            )
            .expect("fixture turn is valid"),
        ));
        let ProtocolCommandResultDto::Rejected(error) = result else {
            unreachable!("an unknown session cannot accept a turn")
        };
        assert_eq!(error.code(), "storage_record_not_found");
    }

    #[test]
    fn daemon_tool_bridge_reports_failures_for_unavailable_calls() {
        let driver = Arc::new(TestSupportUnconfiguredDriver);
        let (_directory, facade) = {
            let directory = TempDir::new().expect("temporary directory exists");
            let facade = DaemonApplicationFacade::open_for_test_support_with_driver(
                directory.path().join("execution.sqlite"),
                fixture_config_snapshot(),
                driver,
            )
            .expect("durable facade opens");
            (directory, facade)
        };
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "execution bridge");
        let (_workspace_directory, workspace) = workspace_fixture("hello.txt", "hello");

        let mismatched_call = intention_proto::ToolCallId::new();
        let error = facade
            .invoke_local_tool_for_daemon(
                session_id,
                run_id,
                mismatched_call,
                "missing-tool",
                read_hello_input(),
                workspace.clone(),
                "{}",
            )
            .expect_err("a tool identity that does not match its typed input is rejected");
        assert_eq!(error.code(), "tool_id_mismatch");
        assert_eq!(
            facade
                .inner
                .repository
                .load_tool_result(session_id, run_id, mismatched_call)
                .expect_err("a rejected identity commits no evidence")
                .code(),
            "tool_result_not_found"
        );

        let unavailable_call = intention_proto::ToolCallId::new();
        let missing = ToolInput::Read(intention_tools::ReadInput {
            path: intention_proto::WorkspaceRelativePathDto::parse("missing.txt")
                .expect("fixture path is valid"),
        });
        let error = facade
            .invoke_local_tool_for_daemon(
                session_id,
                run_id,
                unavailable_call,
                "read",
                missing,
                workspace,
                "{}",
            )
            .expect_err("a read of an unavailable file is a tool failure");
        let evidence = tool_evidence(&facade, session_id, run_id, unavailable_call);
        assert_eq!(evidence.status(), ToolResultStatusDto::Failed);
        assert_eq!(evidence.content(), error.code());
    }

    #[test]
    fn workspace_resolution_reads_the_declared_session_root() {
        let (_directory, facade) = test_facade();
        let (workspace_directory, _workspace) = workspace_fixture("hello.txt", "hello");
        let declared =
            WorkspaceRootDto::parse(workspace_directory.path().to_string_lossy().into_owned())
                .expect("fixture workspace dto is absolute");
        let session_id = SessionId::new();
        let accepted = facade.command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                declared,
                RunModeDto::Build,
            ),
        ));
        assert!(matches!(accepted, ProtocolCommandResultDto::Accepted(_)));

        let resolved = facade
            .resolve_workspace_root_for_daemon(session_id)
            .expect("the declared workspace root resolves");
        assert_eq!(
            resolved.execute_cwd(),
            std::fs::canonicalize(workspace_directory.path())
                .expect("fixture workspace canonicalizes")
                .as_path()
        );
        let error = facade
            .resolve_workspace_root_for_daemon(SessionId::new())
            .expect_err("an unknown session has no workspace root");
        assert_eq!(error.code(), "storage_record_not_found");
    }

    #[test]
    fn facade_rejects_invalid_commands_and_unknown_run_bridges() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        let unknown = RunId::new();

        assert!(matches!(
            facade.command(ProtocolCommandDto::InterruptRun(
                intention_proto::InterruptRunCommandDto::new(session_id, unknown)
            )),
            ProtocolCommandResultDto::Rejected(_)
        ));
        assert!(matches!(
            facade.command(ProtocolCommandDto::RemoveTurn(
                intention_proto::RemoveTurnCommandDto::new(
                    session_id,
                    intention_proto::TurnId::new(),
                )
            )),
            ProtocolCommandResultDto::Rejected(_)
        ));
        assert!(matches!(
            facade.current_starting_run_for_daemon(session_id),
            Err(error) if error.code() == "storage_record_not_found"
        ));
        assert!(
            facade
                .interrupt_run_for_daemon_host(session_id, unknown)
                .is_err()
        );
        assert!(
            facade
                .fail_starting_run_for_daemon(session_id, unknown, "fixture")
                .is_err()
        );
    }

    #[test]
    fn daemon_interrupt_ends_the_in_flight_execute_with_a_partial_result() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "in flight interrupt");
        let (workspace_directory, workspace) = workspace_fixture("keep.txt", "kept");
        let sentinel = workspace_directory.path().join("sentinel.txt");
        let worker_facade = facade.clone();
        let worker_session = session_id;
        let worker_run = run_id;

        let worker = std::thread::spawn(move || {
            worker_facade.invoke_local_tool_for_daemon(
                worker_session,
                worker_run,
                intention_proto::ToolCallId::new(),
                "execute",
                ToolInput::Execute(intention_tools::ExecuteInput {
                    program: intention_tools::BoundedText::new(if cfg!(windows) {
                        "cmd"
                    } else {
                        "sh"
                    })
                    .expect("fixture program"),
                    args: if cfg!(windows) {
                        vec![
                            intention_tools::BoundedText::new("/C").expect("arg"),
                            intention_tools::BoundedText::new(
                                "echo started> sentinel.txt & ping -n 2 127.0.0.1",
                            )
                            .expect("arg"),
                        ]
                    } else {
                        vec![
                            intention_tools::BoundedText::new("-c").expect("arg"),
                            intention_tools::BoundedText::new("printf x > sentinel.txt; sleep 2")
                                .expect("arg"),
                        ]
                    },
                }),
                workspace,
                "{}",
            )
        });

        // The sentinel proves the child was spawned and running, so the
        // interrupt can only land while execution is in flight.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !sentinel.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "execute child never produced its start sentinel"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        facade
            .interrupt_run_for_daemon_host(session_id, run_id)
            .expect("host interrupt reaches the in-flight execute");

        let outcome = worker
            .join()
            .expect("worker completes")
            .expect("in-flight execution observes the interrupt as a partial outcome");
        let LocalToolInvocationOutcomeDto::Partial { stopped, .. } = outcome else {
            unreachable!("the stopped execute must be a partial outcome")
        };
        assert!(stopped);
        assert_eq!(
            facade
                .load_run_snapshot_for_daemon(session_id, run_id)
                .expect("interrupted run snapshot reads")
                .run()
                .status(),
            RunStatusDto::Starting,
            "the interrupted run stays active for the continuing model loop"
        );
    }

    #[test]
    fn committed_tool_results_survive_a_late_interrupt_and_the_run_stays_active() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "late interrupt");
        let (_workspace_directory, workspace) = workspace_fixture("hello.txt", "hello");

        let call_ids = [
            intention_proto::ToolCallId::new(),
            intention_proto::ToolCallId::new(),
        ];
        for call_id in call_ids {
            let result = facade
                .invoke_local_tool_for_daemon(
                    session_id,
                    run_id,
                    call_id,
                    "read",
                    read_hello_input(),
                    workspace.clone(),
                    "{}",
                )
                .expect("reads complete before any interrupt");
            match result {
                LocalToolInvocationOutcomeDto::Completed(ToolResult::Read(text)) => {
                    assert_eq!(text.text.as_str(), "hello")
                }
                _ => unreachable!("read dispatch returns a completed read result"),
            }
        }

        facade
            .interrupt_run_for_daemon_host(session_id, run_id)
            .expect("host interrupt is accepted after completed effects");
        assert_eq!(
            facade
                .load_run_projection_for_daemon(session_id, run_id)
                .expect("the interrupted run stays readable")
                .status(),
            RunStatusDto::Starting,
            "a late interrupt never rewrites completed evidence or terminalizes the run"
        );
        for call_id in call_ids {
            let evidence = tool_evidence(&facade, session_id, run_id, call_id);
            assert_eq!(evidence.status(), ToolResultStatusDto::Completed);
            assert_eq!(evidence.tool_id(), "read");
            assert_eq!(evidence.content(), "hello");
        }
    }

    #[test]
    fn local_tool_invocation_commits_only_its_own_reread_evidence() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "publication reread");
        let (_workspace_directory, workspace) = workspace_fixture("hello.txt", "hello");

        let first_call = intention_proto::ToolCallId::new();
        let result = facade
            .invoke_local_tool_for_daemon(
                session_id,
                run_id,
                first_call,
                "read",
                read_hello_input(),
                workspace.clone(),
                "{}",
            )
            .expect("the first read completes");
        match result {
            LocalToolInvocationOutcomeDto::Completed(ToolResult::Read(text)) => {
                assert_eq!(text.text.as_str(), "hello")
            }
            _ => unreachable!("read dispatch returns a completed read result"),
        }
        let first_evidence = tool_evidence(&facade, session_id, run_id, first_call);
        assert_eq!(first_evidence.call_id(), first_call);
        assert_eq!(first_evidence.status(), ToolResultStatusDto::Completed);
        assert_eq!(
            run_messages(&facade, session_id, run_id)
                .iter()
                .filter(|message| message.tool_call_id() == Some(first_call))
                .count(),
            2,
            "the first call commits exactly its own tool-call and tool-result rows"
        );

        let second_call = intention_proto::ToolCallId::new();
        facade
            .invoke_local_tool_for_daemon(
                session_id,
                run_id,
                second_call,
                "read",
                read_hello_input(),
                workspace,
                "{}",
            )
            .expect("the second read completes");
        let second_evidence = tool_evidence(&facade, session_id, run_id, second_call);
        assert_eq!(second_evidence.call_id(), second_call);
        assert_eq!(
            tool_evidence(&facade, session_id, run_id, first_call),
            first_evidence,
            "the second invocation never rewrites the first call's evidence"
        );
        assert_eq!(
            run_messages(&facade, session_id, run_id)
                .iter()
                .filter(|message| message.tool_call_id() == Some(second_call))
                .count(),
            2,
            "the second call commits only its own rows"
        );
    }

    #[test]
    fn committed_tool_rows_are_published_and_read_back_for_the_exact_call() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "publication path");
        let (_workspace_directory, workspace) = workspace_fixture("hello.txt", "hello");

        let call_id = intention_proto::ToolCallId::new();
        let publisher = RecordingPublisher::new();
        let completed = facade
            .invoke_local_tool_for_daemon_with_publication(
                session_id,
                run_id,
                call_id,
                "read",
                read_hello_input(),
                workspace,
                "{}",
                &publisher,
            )
            .expect("read completes");
        assert_eq!(
            completed,
            LocalToolInvocationOutcomeDto::Completed(hello_read_result())
        );

        // Every committed tool row reached the boundary in commit order, so a
        // live frame always carries a committed value.
        let published = publisher.published();
        assert_eq!(published.len(), 2);
        assert_eq!(published[0].kind(), MessageKindDto::ToolCall);
        assert_eq!(published[0].tool_call_id(), Some(call_id));
        assert_eq!(published[1].kind(), MessageKindDto::ToolResult);
        assert_eq!(published[1].tool_call_id(), Some(call_id));
        assert_eq!(published[1].text(), "hello");
        let committed: Vec<_> = run_messages(&facade, session_id, run_id)
            .into_iter()
            .filter(|message| message.tool_call_id() == Some(call_id))
            .collect();
        assert_eq!(
            published, committed,
            "published rows are the committed rows"
        );

        // The host reads the committed structured evidence of the exact call
        // before it broadcasts that call's result frame.
        let evidence = facade
            .load_tool_result_for_daemon(session_id, run_id, call_id)
            .expect("committed evidence is readable");
        assert_eq!(evidence.call_id(), call_id);
        assert_eq!(evidence.run_id(), run_id);
        assert_eq!(evidence.status(), ToolResultStatusDto::Completed);
        assert_eq!(evidence.content(), "hello");

        // A foreign identity never resolves to this call's evidence.
        let error = facade
            .load_tool_result_for_daemon(session_id, RunId::new(), call_id)
            .expect_err("a cross-run identity cannot read this evidence");
        assert_eq!(error.code(), "tool_result_not_found");
        let error = facade
            .load_tool_result_for_daemon(SessionId::new(), run_id, call_id)
            .expect_err("a cross-session identity cannot read this evidence");
        assert_eq!(error.code(), "tool_result_not_found");
    }

    /// Records every committed transcript row handed to the publication boundary.
    struct RecordingPublisher {
        publications: Mutex<Vec<MessageProjectionDto>>,
    }

    impl RecordingPublisher {
        fn new() -> Self {
            Self {
                publications: Mutex::new(Vec::new()),
            }
        }

        fn published(&self) -> Vec<MessageProjectionDto> {
            self.publications
                .lock()
                .expect("publication lock is available")
                .clone()
        }
    }

    impl ToolResultPublicationPort for RecordingPublisher {
        fn publish_committed_message(&self, message: &MessageProjectionDto) -> DtoResult<()> {
            self.publications
                .lock()
                .expect("publication lock is available")
                .push(message.clone());
            Ok(())
        }
    }
}
