//! Durable M3 composition root for the daemon application facade.
//!
//! Only this module selects SQLite. The public facade exposes protocol DTOs;
//! database resources, locations, configuration text, and committed-event
//! publication stay private.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use intention_config::ConfigPathDto;
use intention_config::{
    ConfigPathResolver, ConfigSnapshotDto, ConfigSourceDto, ProviderKindDto, RawConfigInputDto,
    ResolvedConfigDto, StartupProviderMaterial,
};
use intention_domain::run_status_is_terminal;
use intention_engine::{
    ApplicationService, ModelRunDispatchPort, ToolInvocationRequestDto, WorkspaceBoundaryPort,
};
use intention_engine::{
    ModelRunCommitObserver, ModelRunExecutionInputDto, ModelRunExecutionOutcomeDto,
    ModelRunExecutionService, ModelTimePort, ToolExecutionPort, fail_starting_run,
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
#[cfg(test)]
use intention_storage::ToolResultEvidenceDto;
use intention_storage::{
    RecoverUnfinishedRunsInputDto, SqliteDatabaseLocationDto, SqliteStorageRepository,
    StorageRepositoryDto,
};
#[cfg(test)]
use intention_tools::ToolResult;
use intention_tools::{CancellationSignal, ToolInput, WorkspaceRoot};
use intention_transport::MAX_TRANSCRIPT_SNAPSHOT_BYTES;

const SCHEMA_VERSION: SchemaVersionDto = intention_proto::CURRENT_DTO_SCHEMA_VERSION;
/// The single live configuration snapshot schema (intention-config current schema).
const CONFIG_SCHEMA_VERSION: SchemaVersionDto = SchemaVersionDto::new(1, 0);
const DATABASE_FILENAME: &str = "intention-relay.sqlite";
/// The retained bounded size of a current-state snapshot's recent transcript.
///
/// The row bound is additionally held to `MAX_TRANSCRIPT_SNAPSHOT_BYTES`, the
/// representation budget derived from the single transport envelope cap.
const SESSION_SNAPSHOT_MESSAGES: u32 = 256;

// The terminal outcome of one facade local tool invocation: the daemon host
// maps it onto the model-visible tool-result fact without naming a provider SDK.
use intention_engine::LocalToolInvocationOutcomeDto;

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
    admitted: Mutex<Vec<ModelRunExecutionInputDto>>,
}

impl PrivateModelRunDispatch {
    #[cfg(test)]
    fn admitted(&self) -> DtoResult<Vec<ModelRunExecutionInputDto>> {
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
    fn dispatch_model_run(&self, input: ModelRunExecutionInputDto) -> DtoResult<()> {
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
        // The session's declared anchor is re-authorized before execution, and
        // the invocation addresses only the root this boundary returns.
        workspace.rebind()
    }
}

/// Run-scoped interruption shared between daemon-host interrupts and admitted local tools.
struct LocalToolCancellationEntry {
    signal: CancellationSignal,
    /// Local invocations currently executing against this exact run.
    inflight: usize,
}

impl DaemonApplicationFacade {
    /// Executes one explicit local tool call through the one-transaction path
    /// and hands every committed transcript row to the publication boundary.
    ///
    /// Publication follows each row's own commit, so every published frame
    /// carries a committed value.
    /// This API is an internal, caller-admitted single invocation and never
    /// starts a loop.
    #[doc(hidden)]
    #[expect(
        clippy::too_many_arguments,
        reason = "The daemon bridge keeps the flat tool-invocation payload in one call."
    )]
    pub fn invoke_local_tool_for_daemon_with_publication<P: ModelRunCommitObserver>(
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
        let result = ApplicationService::new(&self.inner.repository)
            .with_workspace_boundary(SafeWorkspaceBoundary)
            .invoke_local_tool_with_publication(
                ToolInvocationRequestDto::new(
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

    /// Executes one scheduled run through the privately selected provider
    /// driver with the mandatory tool executor.
    ///
    /// This bridge is provider-neutral and safe: it accepts only a model-run
    /// execution input, a time port, committed-observation evidence, and the
    /// tool executor. It does not expose provider SDKs, credentials, Tokio, or
    /// storage resources. Provider-emitted tool calls execute through the
    /// caller-supplied durable tool path.
    #[doc(hidden)]
    pub async fn execute_scheduled_model_run_for_daemon_with_tool_executor<Time>(
        &self,
        input: ModelRunExecutionInputDto,
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
        .execute(input)
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
    /// The snapshot carries the current run projection and the byte-bounded
    /// recent transcript rows of that run; a re-subscribing client receives
    /// current state and continues from live frames.
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
        let messages = bounded_snapshot_messages(self.inner.repository.load_run_messages(
            session_id,
            run_id,
            SESSION_SNAPSHOT_MESSAGES,
        )?)?;
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
        cancellation: ModelCancellationSignal,
    ) -> DtoResult<ModelRunExecutionInputDto> {
        ApplicationService::new(&self.inner.repository).schedule_starting_run(
            session_id,
            run_id,
            cancellation,
        )
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
        DaemonHealthDto::new(SCHEMA_VERSION, DaemonReadinessDto::Ready)
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
    /// Recent transcript rows are bounded by the retained delivery bound and
    /// the byte budget derived from the transport envelope cap; a run-scoped
    /// request returns that run's rows instead of the session tail.
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
            Some(run_id) => bounded_snapshot_messages(self.inner.repository.load_run_messages(
                session_id,
                run_id,
                SESSION_SNAPSHOT_MESSAGES,
            )?)?,
            None => bounded_snapshot_messages(
                self.inner
                    .repository
                    .load_recent_messages(session_id, SESSION_SNAPSHOT_MESSAGES)?,
            )?,
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
                    .create_session(command, timestamp)?
            }
            ProtocolCommandDto::SendUserTurn(command) => {
                let proposed_run_id = RunId::new();
                ApplicationService::new(&self.inner.repository).send_user_turn_and_schedule(
                    command,
                    proposed_run_id,
                    self.inner.config_snapshot.clone(),
                    timestamp,
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

/// Trims one snapshot transcript to the representation budget derived from the
/// single transport envelope cap, keeping the newest committed rows that fit.
///
/// A transcript snapshot is the only legitimate response large enough to
/// approach the envelope cap, so the read path owns the byte budget and the
/// transport owns the cap: no legitimate snapshot can be dropped silently at
/// the connection level.
fn bounded_snapshot_messages(
    mut messages: Vec<intention_proto::MessageProjectionDto>,
) -> DtoResult<Vec<intention_proto::MessageProjectionDto>> {
    let mut budget = MAX_TRANSCRIPT_SNAPSHOT_BYTES;
    let mut keep_from = messages.len();
    for (index, message) in messages.iter().enumerate().rev() {
        let size = serde_json::to_vec(message)
            .map_err(|_| {
                ErrorDto::validation(
                    "local_protocol_encode_failed",
                    "a typed local protocol message could not be encoded",
                )
            })?
            .len()
            .saturating_add(1);
        if size > budget {
            break;
        }
        budget -= size;
        keep_from = index;
    }
    messages.drain(..keep_from);
    Ok(messages)
}

pub fn now() -> DtoResult<TimestampDto> {
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
    use intention_engine::ModelRunCommitDto;
    use intention_proto::{MessageKindDto, MessageProjectionDto, SendUserTurnCommandDto};
    use intention_storage::AppendMessageInputDto;
    use tempfile::TempDir;

    fn test_facade() -> (TempDir, DaemonApplicationFacade) {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test_support(
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
        let facade = DaemonApplicationFacade::open_for_test_support(
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
    fn schedule_starting_run_rejects_unknown_run_without_leaking_storage_details() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test_support(
            directory.path().join("schedule-error.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let error = facade
            .schedule_starting_run_for_daemon(
                SessionId::new(),
                RunId::new(),
                ModelCancellationSignal::new(),
            )
            .expect_err("unknown run cannot be scheduled");
        assert_eq!(error.code(), "run_model_context_unavailable");
        assert!(!error.to_string().contains("schedule-error.sqlite"));
    }

    #[test]
    fn facade_replay_of_the_same_user_turn_conflicts_without_duplicating_transcript_or_dispatch() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test_support(
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
            .schedule_starting_run_for_daemon(session_id, run_id, ModelCancellationSignal::new())
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
    fn provider_composition_selects_and_rejects_without_secret_disclosure() {
        let directory = TempDir::new().expect("temporary directory exists");
        let path = directory.path().join("openrouter.toml");
        fs::write(
            &path,
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"selected-provider-secret\"",
        )
        .expect("fixture config writes");
        let invalid = directory.path().join("invalid.toml");
        fs::write(
            &invalid,
            "schema_version = 1\n[provider]\nkind = \"not-a-provider\"\nmodel = \"fixture\"\ncredential = \"invalid-provider-secret\"",
        )
        .expect("fixture config writes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for file in [&path, &invalid] {
                fs::set_permissions(file, fs::Permissions::from_mode(0o600))
                    .expect("fixture config permissions set");
            }
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

        assert_eq!(
            facade.selected_provider_kind(),
            Some(ProviderKindDto::Openrouter)
        );
        assert_eq!(
            snapshot.resolved().provider().kind(),
            ProviderKindDto::Openrouter
        );
        assert!(snapshot.resolved().provider().credential_configured());
        assert!(
            !snapshot
                .resolved()
                .safe_debug_projection()
                .contains("selected-provider-secret")
        );

        let source = ConfigSourceDto::Explicit(
            ConfigPathDto::parse(invalid.to_string_lossy().into_owned())
                .expect("fixture config path is absolute"),
        );
        let error = load_provider_configuration(source)
            .err()
            .expect("invalid provider configuration must fail safely");
        assert_eq!(error.code(), "invalid_config_schema");
        assert!(!error.to_string().contains("invalid-provider-secret"));
    }

    #[test]
    fn daemon_host_failure_bridges_are_safe() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "bridge");
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
        assert!(
            facade
                .fail_starting_run_for_daemon(session_id, RunId::new(), "fixture_failure")
                .is_err()
        );
    }

    #[test]
    fn command_routes_remove_turn_and_rejects_interrupt_and_subscription() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test_support(
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
            .invoke_local_tool_for_daemon_with_publication(
                session_id,
                run_id,
                mismatched_call,
                "missing-tool",
                read_hello_input(),
                workspace.clone(),
                "{}",
                &(),
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
            .invoke_local_tool_for_daemon_with_publication(
                session_id,
                run_id,
                unavailable_call,
                "read",
                missing,
                workspace,
                "{}",
                &(),
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
            worker_facade.invoke_local_tool_for_daemon_with_publication(
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
                &(),
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
                .invoke_local_tool_for_daemon_with_publication(
                    session_id,
                    run_id,
                    call_id,
                    "read",
                    read_hello_input(),
                    workspace.clone(),
                    "{}",
                    &(),
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
            .invoke_local_tool_for_daemon_with_publication(
                session_id,
                run_id,
                first_call,
                "read",
                read_hello_input(),
                workspace.clone(),
                "{}",
                &(),
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
            .invoke_local_tool_for_daemon_with_publication(
                session_id,
                run_id,
                second_call,
                "read",
                read_hello_input(),
                workspace,
                "{}",
                &(),
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
    fn committed_tool_rows_are_published_in_commit_order() {
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
        // live frame always carries the committed value of its own transaction
        // without any durable re-read.
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
    }

    /// Commits one fixture transcript row bound to the exact run.
    fn append_transcript_row(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        run_id: RunId,
        text: &str,
    ) {
        let message = MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            text,
            None,
            None,
            None,
        )
        .expect("fixture transcript row is valid");
        facade
            .inner
            .repository
            .append_message(AppendMessageInputDto::new(
                message,
                TimestampDto::from_unix_seconds(2).expect("fixture timestamp is valid"),
            ))
            .expect("fixture transcript row commits");
    }

    #[test]
    fn snapshot_reads_stay_below_the_single_transport_envelope_cap() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "snapshot bound");

        // A legitimate 256-row snapshot stays servable in full while it fits
        // the representation budget derived from the envelope cap.
        for index in 0..SESSION_SNAPSHOT_MESSAGES {
            append_transcript_row(
                &facade,
                session_id,
                run_id,
                &format!("row {index} {}", "s".repeat(1024)),
            );
        }
        let servable = facade
            .session_snapshot(session_id, None)
            .expect("the bounded snapshot reads");
        assert_eq!(
            servable.messages().len(),
            usize::try_from(SESSION_SNAPSHOT_MESSAGES).expect("the row bound fits a usize")
        );
        assert!(
            serde_json::to_vec(&servable)
                .expect("the bounded snapshot encodes")
                .len()
                <= intention_transport::MAX_MESSAGE_BYTES
        );

        // Rows whose combined encoding exceeds the budget keep the newest rows
        // that fit instead of producing an unservable snapshot.
        for index in 0..8 {
            append_transcript_row(
                &facade,
                session_id,
                run_id,
                &format!("big {index} {}", "x".repeat(256 * 1024)),
            );
        }
        let bounded = facade
            .session_snapshot(session_id, None)
            .expect("the byte-bounded snapshot reads");
        assert!(
            serde_json::to_vec(&bounded)
                .expect("the byte-bounded snapshot encodes")
                .len()
                <= intention_transport::MAX_MESSAGE_BYTES
        );
        assert!(bounded.messages().len() < 256);
        assert_eq!(
            bounded
                .messages()
                .last()
                .expect("the newest row stays")
                .text(),
            format!("big 7 {}", "x".repeat(256 * 1024))
        );
        assert!(
            bounded
                .messages()
                .iter()
                .all(|message| !message.text().starts_with("row 0 ")),
            "the oldest rows are dropped first"
        );
    }

    /// Records every committed transcript row handed to the commit sink.
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

    impl ModelRunCommitObserver for RecordingPublisher {
        fn observe_model_run_commit(&self, committed: &ModelRunCommitDto) {
            if let ModelRunCommitDto::Content(message) = committed {
                self.publications
                    .lock()
                    .expect("publication lock is available")
                    .push(message.clone());
            }
        }
    }
}
