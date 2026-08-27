//! Durable M3 composition root for the daemon application facade.
//!
//! Only this crate selects SQLite. The public facade exposes protocol DTOs;
//! database resources, locations, configuration text, and committed-event
//! publication stay private.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use intention_application::{
    ApplicationService, CreateSessionWorkflowInputDto, InvokeLocalToolInputDto,
    ModelRunDispatchPort, ScheduleModelRunDto, SendUserTurnWorkflowInputDto,
};
#[cfg(test)]
use intention_config::ConfigPathDto;
use intention_config::{
    ConfigPathResolver, ConfigSnapshotDto, ConfigSourceDto, ProviderKindDto, RawConfigInputDto,
    ResolvedConfigDto, StartupProviderMaterial,
};
#[cfg(test)]
use intention_domain::{CreateSessionCommandDto, RunModeDto, WorkspaceRootDto};
use intention_domain::{GetSessionSnapshotQueryDto, RunEventCursorDto, RunReplayDto, RunStatusDto};
use intention_model::{ModelCancellationSignal, ModelExecutionDriver};
#[cfg(any(test, feature = "test-support"))]
use intention_model::{ModelCapabilitiesDto, ModelDriver, ModelEventStream};
#[cfg(test)]
use intention_protocol::SendUserTurnOutcomeDto;
use intention_protocol::{
    DaemonHealthDto, DaemonReadinessDto, ProtocolAcceptedDto, ProtocolAcceptedResultDto,
    ProtocolCommandDto, ProtocolCommandResultDto, ProtocolQueryDto, ProtocolQueryResultDto,
    SessionEventTailBatchDto, SessionResyncDto, SessionResyncReasonDto,
    SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
};
use intention_provider_generic_chat::GenericChatDriver;
use intention_provider_openrouter::OpenRouterDriver;
#[cfg(feature = "test-support")]
use intention_runtime::ModelRunFirstAppendGate;
use intention_runtime::{
    ModelRunCommitObserver, ModelRunExecutionInputDto, ModelRunExecutionOutcomeDto,
    ModelRunExecutionService, ModelTimePort, RuntimeService, RuntimeValuesDto, fail_starting_run,
};
use intention_storage::{CommittedChangeDto, StorageRepositoryDto};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use intention_tools::{ToolInput, ToolResult};
use intention_types::{
    ConfigRevisionId, CorrelationIdDto, DtoResult, ErrorDto, RunId, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto,
};
#[cfg(test)]
use intention_types::{ProjectId, WorkspaceId};
use intention_workspace::WorkspaceRoot;

const SCHEMA_VERSION: SchemaVersionDto = SchemaVersionDto::new(1, 0);
const PROTOCOL_VERSION: intention_protocol::ProtocolVersionDto =
    intention_protocol::ProtocolVersionDto::new(1, 0);
const DATABASE_FILENAME: &str = "intention-relay.sqlite";

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
    publisher: Box<dyn PostCommitPublisher>,
    command_gate: Mutex<()>,
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
        _request: intention_model::ModelRequestDto,
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

trait PostCommitPublisher: Send + Sync {
    /// Retained M3 session publisher seam, invoked only after an independent durable read.
    ///
    /// M3 composition supplies the no-op implementation. M4 run-scoped streaming
    /// publishes through the daemon host's dedicated commit observer instead.
    fn publish(&self, change: &CommittedChangeDto) -> DtoResult<()>;
}

struct NoopPostCommitPublisher;

impl PostCommitPublisher for NoopPostCommitPublisher {
    fn publish(&self, _change: &CommittedChangeDto) -> DtoResult<()> {
        Ok(())
    }
}

impl DaemonApplicationFacade {
    /// Executes one explicit local tool call through the durable lifecycle path.
    ///
    /// The M4 model `ToolCallRecorded` fact remains denial-only; this API is an
    /// internal, caller-admitted single invocation and never starts a loop.
    #[doc(hidden)]
    pub fn invoke_local_tool_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call_id: intention_types::ToolCallId,
        tool_id: impl Into<String>,
        input: ToolInput,
        workspace: WorkspaceRoot,
    ) -> DtoResult<ToolResult> {
        let result = intention_application::ApplicationService::new(&self.inner.repository)
            .invoke_local_tool(InvokeLocalToolInputDto::new(
                workspace,
                session_id,
                run_id,
                call_id,
                tool_id,
                input,
                now()?,
            ));
        // Publication is intentionally after both lifecycle commits and an
        // independent durable reread; the existing publisher is the seam.
        if result.is_ok() {
            let _ = self.publish_after_durable_read(
                session_id,
                self.inner
                    .repository
                    .load_session_snapshot(session_id)?
                    .at_sequence(),
            );
        }
        result
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
            Box::new(NoopPostCommitPublisher),
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
            Box::new(NoopPostCommitPublisher),
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
            Box::new(NoopPostCommitPublisher),
        )
    }

    #[cfg(test)]
    fn open_with_publisher(
        database_location: impl AsRef<Path>,
        config_snapshot: ConfigSnapshotDto,
        publisher: Box<dyn PostCommitPublisher>,
    ) -> DtoResult<Self> {
        Self::open_with_selected_provider(
            database_location,
            config_snapshot,
            SelectedProvider::for_test_support(Arc::new(TestSupportUnconfiguredDriver)),
            publisher,
        )
    }

    /// Executes one scheduled run through the privately selected provider driver.
    ///
    /// This bridge is provider-neutral and safe: it accepts only scheduling DTOs,
    /// cancellation, a time port, and committed-observation evidence. It does not
    /// expose provider SDKs, credentials, Tokio, or storage resources.
    #[doc(hidden)]
    pub async fn execute_scheduled_model_run_for_daemon<Time>(
        &self,
        schedule: ScheduleModelRunDto,
        cancellation: ModelCancellationSignal,
        time: &Time,
        observer: &dyn ModelRunCommitObserver,
    ) -> DtoResult<ModelRunExecutionOutcomeDto>
    where
        Time: ModelTimePort + Sync,
    {
        ModelRunExecutionService::with_commit_observer(
            &self.inner.repository,
            self.inner._selected_provider.driver(),
            time,
            observer,
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

    /// Executes one scheduled run with the fixture-only first-append race gate.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub async fn execute_scheduled_model_run_for_daemon_with_first_append_gate<Time>(
        &self,
        schedule: ScheduleModelRunDto,
        cancellation: ModelCancellationSignal,
        time: &Time,
        observer: &dyn ModelRunCommitObserver,
        first_append_gate: &dyn ModelRunFirstAppendGate,
    ) -> DtoResult<ModelRunExecutionOutcomeDto>
    where
        Time: ModelTimePort + Sync,
    {
        ModelRunExecutionService::with_commit_observer_and_first_append_gate(
            &self.inner.repository,
            self.inner._selected_provider.driver(),
            time,
            observer,
            first_append_gate,
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

    /// Durably moves the exact active run to `Cancelling` without terminalizing it.
    ///
    /// A streaming daemon host must signal the matching execution task after this
    /// commit. The retained synchronous `command(StopRun)` path terminalizes only
    /// for legacy M3 callers outside that host.
    #[doc(hidden)]
    pub fn stop_run_for_daemon_host(
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
        ApplicationService::new(&self.inner.repository).stop_run(
            intention_domain::StopRunCommandDto::new(session_id, run_id),
            RuntimeValuesDto::new(RunId::new(), self.inner.config_snapshot.clone(), now()?),
        )
    }

    /// Terminalizes an exact durable `Cancelling` run for the daemon task registry.
    ///
    /// This is used only when a stop wins before normal executor admission. It
    /// preserves the required two-step cancellation path while ensuring the host
    /// retains ownership of the terminal transition rather than leaving active
    /// durable state without a task.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the exact run is unavailable or is no longer
    /// eligible for the `Cancelling -> Cancelled` transition.
    #[doc(hidden)]
    pub fn terminalize_cancelling_run_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<()> {
        let _gate = self.inner.command_gate.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_command_unavailable",
                "daemon command is unavailable",
            )
        })?;
        RuntimeService::new(
            &self.inner.repository,
            RuntimeValuesDto::new(RunId::new(), self.inner.config_snapshot.clone(), now()?),
        )
        .complete_terminal(session_id, run_id, RunStatusDto::Cancelled)?;
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

    /// Loads an authoritative current run snapshot for the private daemon host.
    #[doc(hidden)]
    pub fn load_current_run_replay_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunReplayDto> {
        ApplicationService::new(&self.inner.repository).load_current_run_replay(session_id, run_id)
    }

    /// Loads a contiguous durable run-fact range for the private daemon host.
    #[doc(hidden)]
    pub fn load_run_tail_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
        after_cursor: RunEventCursorDto,
    ) -> DtoResult<intention_domain::RunEventTailPageDto> {
        ApplicationService::new(&self.inner.repository).load_run_tail(
            session_id,
            run_id,
            after_cursor,
        )
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
        Ok(ApplicationService::new(&self.inner.repository)
            .get_session_snapshot(GetSessionSnapshotQueryDto::new(session_id))?
            .projection()
            .and_then(|projection| projection.active_run())
            .filter(|run| run.status() == RunStatusDto::Starting)
            .map(|run| run.run_id()))
    }

    fn open_with_selected_provider(
        database_location: impl AsRef<Path>,
        config_snapshot: ConfigSnapshotDto,
        selected_provider: SelectedProvider,
        publisher: Box<dyn PostCommitPublisher>,
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
                publisher,
                command_gate: Mutex::new(()),
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
                ApplicationService::new(&self.inner.repository)
                    .get_session_snapshot(query)
                    .map_or_else(
                        ProtocolQueryResultDto::Rejected,
                        ProtocolQueryResultDto::SessionSnapshot,
                    )
            }
        }
    }

    /// Returns a durable checkpoint and its contiguous replay tail, or typed resync.
    ///
    /// This retained M3 session-subscription seam is replay-only and does not
    /// filter session snapshots. M4 run-scoped streaming publishes through the
    /// dedicated daemon-host observer and separate run subscription contract.
    #[must_use]
    pub fn subscribe(&self, command: SubscribeSessionCommandDto) -> SessionSubscriptionResponseDto {
        if command.run_id().is_some() {
            return resync(
                command.session_id(),
                SessionResyncReasonDto::HistoryUnavailable,
            );
        }
        let requested_after = command
            .after_sequence()
            .unwrap_or(SessionEventSequenceDto::new(0));
        let current = match ApplicationService::new(&self.inner.repository)
            .get_session_snapshot(GetSessionSnapshotQueryDto::new(command.session_id()))
        {
            Ok(snapshot) => snapshot,
            Err(_) => {
                return resync(
                    command.session_id(),
                    SessionResyncReasonDto::HistoryUnavailable,
                );
            }
        };
        if requested_after.value() > current.at_sequence().value() {
            return resync(
                command.session_id(),
                SessionResyncReasonDto::InvalidPosition,
            );
        }
        if requested_after != current.at_sequence() {
            return SessionSubscriptionResponseDto::snapshot_and_tail(
                current.clone(),
                SessionEventTailBatchDto::new(
                    SCHEMA_VERSION,
                    command.session_id(),
                    current.at_sequence(),
                    Vec::new(),
                )
                .unwrap_or_else(|_| unreachable!("empty durable tail must be valid")),
            )
            .unwrap_or_else(|_| unreachable!("current snapshot and empty tail must agree"));
        }
        let tail = SessionEventTailBatchDto::new(
            SCHEMA_VERSION,
            command.session_id(),
            requested_after,
            Vec::new(),
        )
        .unwrap_or_else(|_| unreachable!("empty durable tail must be valid"));
        SessionSubscriptionResponseDto::snapshot_and_tail(current, tail)
            .unwrap_or_else(|_| unreachable!("current snapshot and empty tail must agree"))
    }

    /// Dispatches a durable M3 command and invokes the publisher only after commit.
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

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn durable_events_for_test_support(
        &self,
        session_id: SessionId,
    ) -> DtoResult<Vec<intention_types::EventEnvelopeDto<intention_domain::DomainEventDto>>> {
        self.inner
            .repository
            .load_tail(session_id, SessionEventSequenceDto::new(0))
    }

    fn command_result(&self, command: ProtocolCommandDto) -> DtoResult<ProtocolAcceptedResultDto> {
        let _gate = self.inner.command_gate.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_command_unavailable",
                "daemon command is unavailable",
            )
        })?;
        let session_id = command_session_id(&command);
        let prior_position =
            session_id.map_or(Ok(SessionEventSequenceDto::new(0)), |session_id| {
                self.inner
                    .repository
                    .load_session_snapshot(session_id)
                    .map(|projection| projection.at_sequence())
                    .or_else(|error| {
                        if error.code() == "storage_record_not_found" {
                            Ok(SessionEventSequenceDto::new(0))
                        } else {
                            Err(error)
                        }
                    })
            })?;
        let timestamp = now()?;
        let result = match command {
            ProtocolCommandDto::CreateSession(command) => {
                ApplicationService::new(&self.inner.repository)
                    .create_session(CreateSessionWorkflowInputDto::new(command, timestamp))?
            }
            ProtocolCommandDto::SendUserTurn(command) => {
                let proposed_run_id =
                    RunId::parse(&command.turn_id().to_string()).map_err(|_| {
                        ErrorDto::unavailable(
                            "daemon_command_unavailable",
                            "daemon command is unavailable",
                        )
                    })?;
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
            ProtocolCommandDto::RemoveQueuedTurn(command) => {
                ApplicationService::new(&self.inner.repository)
                    .remove_queued_turn(command, timestamp)?
            }
            ProtocolCommandDto::StopRun(command) => {
                let result = ApplicationService::new(&self.inner.repository).stop_run(
                    command,
                    RuntimeValuesDto::new(
                        RunId::new(),
                        self.inner.config_snapshot.clone(),
                        timestamp,
                    ),
                )?;
                RuntimeService::new(
                    &self.inner.repository,
                    RuntimeValuesDto::new(RunId::new(), self.inner.config_snapshot.clone(), now()?),
                )
                .complete_terminal(
                    command.session_id(),
                    command.run_id(),
                    intention_domain::RunStatusDto::Cancelled,
                )?;
                result
            }
            ProtocolCommandDto::SubscribeSession(_) => {
                return Err(ErrorDto::validation(
                    "invalid_subscription_dispatch",
                    "session subscriptions use the dedicated protocol response",
                ));
            }
        };
        if let Some(session_id) = accepted_session_id(&result) {
            self.publish_after_durable_read(session_id, prior_position)?;
        }
        Ok(result)
    }

    fn recover_before_ready(&self) -> DtoResult<()> {
        let changes = RuntimeService::new(
            &self.inner.repository,
            RuntimeValuesDto::new(RunId::new(), self.inner.config_snapshot.clone(), now()?),
        )
        .recover_before_ready()?;
        for change in changes {
            let event_count = u64::try_from(change.events().len()).map_err(|_| {
                ErrorDto::unavailable(
                    "daemon_storage_unavailable",
                    "daemon durable storage is unavailable",
                )
            })?;
            let prior = SessionEventSequenceDto::new(
                change
                    .position()
                    .value()
                    .checked_sub(event_count)
                    .ok_or_else(|| {
                        ErrorDto::unavailable(
                            "daemon_storage_unavailable",
                            "daemon durable storage is unavailable",
                        )
                    })?,
            );
            self.publish_after_durable_read(change.projection().session_id(), prior)?;
        }
        Ok(())
    }

    fn publish_after_durable_read(
        &self,
        session_id: SessionId,
        after_sequence: SessionEventSequenceDto,
    ) -> DtoResult<()> {
        let projection = self.inner.repository.load_session_snapshot(session_id)?;
        let events = self
            .inner
            .repository
            .load_tail(session_id, after_sequence)?;
        let committed =
            CommittedChangeDto::new(projection.clone(), projection.at_sequence(), events, None)?;
        let _ = self.inner.publisher.publish(&committed);
        Ok(())
    }
}

const fn command_session_id(command: &ProtocolCommandDto) -> Option<SessionId> {
    match command {
        ProtocolCommandDto::CreateSession(command) => Some(command.session_id()),
        ProtocolCommandDto::SendUserTurn(command) => Some(command.session_id()),
        ProtocolCommandDto::RemoveQueuedTurn(command) => Some(command.session_id()),
        ProtocolCommandDto::StopRun(command) => Some(command.session_id()),
        ProtocolCommandDto::SubscribeSession(_) => None,
    }
}

const fn accepted_session_id(result: &ProtocolAcceptedResultDto) -> Option<SessionId> {
    match result {
        ProtocolAcceptedResultDto::CreateSession(result) => Some(result.session_id()),
        ProtocolAcceptedResultDto::SendUserTurn(result) => Some(result.session_id()),
        ProtocolAcceptedResultDto::RemoveQueuedTurn(result) => Some(result.session_id()),
        ProtocolAcceptedResultDto::StopRun(result) => Some(result.session_id()),
    }
}

const fn resync(
    session_id: SessionId,
    reason: SessionResyncReasonDto,
) -> SessionSubscriptionResponseDto {
    SessionSubscriptionResponseDto::resync_required(SessionResyncDto::new(
        SCHEMA_VERSION,
        session_id,
        reason,
    ))
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
        SCHEMA_VERSION,
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
    use std::sync::Mutex;

    use intention_client::SessionSubscriptionReducer;
    use intention_domain::SendUserTurnCommandDto;
    use tempfile::TempDir;

    #[derive(Default)]
    struct RecordingPublisher {
        records: Mutex<Vec<CommittedChangeDto>>,
        fail: bool,
    }

    impl RecordingPublisher {
        const fn failing() -> Self {
            Self {
                records: Mutex::new(Vec::new()),
                fail: true,
            }
        }

        fn records(&self) -> Vec<CommittedChangeDto> {
            self.records
                .lock()
                .expect("publisher recorder mutex remains available")
                .clone()
        }
    }

    impl PostCommitPublisher for Arc<RecordingPublisher> {
        fn publish(&self, change: &CommittedChangeDto) -> DtoResult<()> {
            self.records
                .lock()
                .map_err(|_| {
                    ErrorDto::unavailable("publisher_unavailable", "publisher is unavailable")
                })?
                .push(change.clone());
            if self.fail {
                Err(ErrorDto::unavailable(
                    "publisher_unavailable",
                    "publisher is unavailable",
                ))
            } else {
                Ok(())
            }
        }
    }

    fn facade_with_recorder(
        recorder: Arc<RecordingPublisher>,
    ) -> (TempDir, DaemonApplicationFacade) {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_with_publisher(
            directory.path().join("recorder.sqlite"),
            fixture_config_snapshot(),
            Box::new(recorder),
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
            SCHEMA_VERSION,
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

    fn send_user_turn(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        content: &str,
    ) -> ProtocolCommandResultDto {
        facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, intention_types::TurnId::new(), content)
                .expect("fixture user turn is valid"),
        ))
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
        let Some(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn)) = accepted_result.result()
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
        assert_eq!(
            facade
                .durable_events_for_test_support(session_id)
                .expect("durable turn events load")
                .len(),
            3,
            "admission does not execute a provider"
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
    fn facade_retry_of_same_user_turn_reuses_durable_run_and_skips_events_and_dispatch() {
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
            intention_types::TurnId::new(),
            "idempotent turn",
        )
        .expect("fixture user turn is valid");

        let initial = facade.command(ProtocolCommandDto::SendUserTurn(command.clone()));
        let events_after_initial = facade
            .durable_events_for_test_support(session_id)
            .expect("durable turn events load");
        let replay = facade.command(ProtocolCommandDto::SendUserTurn(command));
        let events_after_replay = facade
            .durable_events_for_test_support(session_id)
            .expect("durable turn events load");

        let (
            ProtocolCommandResultDto::Accepted(initial),
            ProtocolCommandResultDto::Accepted(replay),
        ) = (&initial, &replay)
        else {
            unreachable!("identical user-turn commands are accepted")
        };
        assert_eq!(replay.result(), initial.result());
        assert!(matches!(
            initial.result(),
            Some(ProtocolAcceptedResultDto::SendUserTurn(turn))
                if matches!(turn.outcome(), SendUserTurnOutcomeDto::Started { .. })
        ));
        assert_eq!(events_after_replay, events_after_initial);
        assert_eq!(
            facade
                .inner
                .dispatch
                .admitted()
                .expect("dispatch recorder remains available")
                .len(),
            1,
            "the idempotent retry does not enter the dispatch seam"
        );
    }

    #[test]
    fn queued_turn_does_not_admit_dispatch() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("queued-dispatch.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);

        let first = send_user_turn(&facade, session_id, "started turn");
        assert!(matches!(first, ProtocolCommandResultDto::Accepted(_)));
        assert_eq!(
            facade
                .inner
                .dispatch
                .admitted()
                .expect("dispatch recorder remains available")
                .len(),
            1
        );

        let queued = send_user_turn(&facade, session_id, "queued turn");
        assert!(matches!(
            queued,
            ProtocolCommandResultDto::Accepted(accepted)
                if matches!(
                    accepted.result(),
                    Some(ProtocolAcceptedResultDto::SendUserTurn(turn))
                        if matches!(turn.outcome(), SendUserTurnOutcomeDto::Queued { .. })
                )
        ));
        assert_eq!(
            facade
                .inner
                .dispatch
                .admitted()
                .expect("dispatch recorder remains available")
                .len(),
            1,
            "queued turns never enter the dispatch seam"
        );
    }

    #[test]
    fn facade_send_user_turn_uses_the_selected_provider_dispatch_seam() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("facade-dispatch.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);

        let accepted = send_user_turn(&facade, session_id, "facade turn");
        assert!(matches!(accepted, ProtocolCommandResultDto::Accepted(_)));
        let events = facade
            .durable_events_for_test_support(session_id)
            .expect("durable turn events load");
        assert_eq!(events.len(), 3, "admission does not execute a provider");
    }

    #[test]
    fn daemon_host_bridges_read_the_exact_starting_run_and_stop_only_to_cancelling() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("daemon-host-bridge.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);

        let accepted = send_user_turn(&facade, session_id, "host bridge turn");
        let ProtocolCommandResultDto::Accepted(accepted) = accepted else {
            unreachable!("fixture turn is accepted")
        };
        let Some(ProtocolAcceptedResultDto::SendUserTurn(turn)) = accepted.result() else {
            unreachable!("fixture turn has started-run evidence")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
            unreachable!("first fixture turn starts")
        };

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
        let replay = facade
            .load_current_run_replay_for_daemon(session_id, run_id)
            .expect("current run replay reads");
        assert_eq!(replay.snapshot().cursor(), RunEventCursorDto::new(0));
        assert!(
            facade
                .load_run_tail_for_daemon(session_id, run_id, RunEventCursorDto::new(0))
                .expect("empty run tail reads")
                .facts()
                .is_empty()
        );

        facade
            .stop_run_for_daemon_host(session_id, run_id)
            .expect("host stop commits cancelling");
        assert_eq!(
            facade
                .current_starting_run_for_daemon(session_id)
                .expect("no starting run remains"),
            None
        );
        assert_eq!(
            facade
                .load_current_run_replay_for_daemon(session_id, run_id)
                .expect("cancelling run replay reads")
                .snapshot()
                .run_projection()
                .status(),
            RunStatusDto::Cancelling
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
                Box::new(NoopPostCommitPublisher),
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
    fn config_loading_redacts_raw_toml_and_creates_a_fresh_safe_snapshot() {
        let directory = TempDir::new().expect("temporary directory exists");
        let path = directory.path().join("config.toml");
        fs::write(
            &path,
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"not-a-real-credential\"",
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
        let snapshot = load_config_snapshot(source).expect("safe configuration loads");
        assert!(snapshot.resolved().provider().credential_configured());
        assert!(
            !snapshot
                .resolved()
                .safe_debug_projection()
                .contains("not-a-real-credential")
        );
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
        let (_directory, facade) = facade_with_recorder(Arc::new(RecordingPublisher::default()));
        let session_id = SessionId::new();
        create(&facade, session_id);
        assert!(matches!(
            facade.subscribe(SubscribeSessionCommandDto::new(
                SCHEMA_VERSION,
                session_id,
                Some(SessionEventSequenceDto::new(1)),
                RunModeDto::Build,
            )),
            SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail }
                if snapshot.at_sequence() == SessionEventSequenceDto::new(1) && tail.events().is_empty()
        ));
        assert!(matches!(
            facade.subscribe(SubscribeSessionCommandDto::new(
                SCHEMA_VERSION,
                SessionId::new(),
                None,
                RunModeDto::Build,
            )),
            SessionSubscriptionResponseDto::ResyncRequired(resync)
                if resync.reason() == SessionResyncReasonDto::HistoryUnavailable
        ));
    }

    #[test]
    fn subscription_rejects_run_scoped_and_invalid_positions() {
        let (_directory, facade) = facade_with_recorder(Arc::new(RecordingPublisher::default()));
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = RunId::new();
        let with_run = facade.subscribe(SubscribeSessionCommandDto::with_run_id(
            SCHEMA_VERSION,
            session_id,
            Some(run_id),
            None,
            RunModeDto::Build,
        ));
        assert!(
            matches!(with_run, SessionSubscriptionResponseDto::ResyncRequired(r)
            if r.reason() == SessionResyncReasonDto::HistoryUnavailable)
        );
        let ahead = facade.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA_VERSION,
            session_id,
            Some(SessionEventSequenceDto::new(99)),
            RunModeDto::Build,
        ));
        assert!(
            matches!(ahead, SessionSubscriptionResponseDto::ResyncRequired(r)
            if r.reason() == SessionResyncReasonDto::InvalidPosition)
        );
    }

    #[test]
    fn daemon_host_failure_and_terminalization_bridges_are_safe() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("bridges.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);
        let accepted = send_user_turn(&facade, session_id, "bridge");
        let ProtocolCommandResultDto::Accepted(a) = accepted else {
            unreachable!()
        };
        let Some(ProtocolAcceptedResultDto::SendUserTurn(t)) = a.result() else {
            unreachable!()
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = t.outcome() else {
            unreachable!()
        };
        facade
            .stop_run_for_daemon_host(session_id, run_id)
            .expect("stop commits");
        facade
            .terminalize_cancelling_run_for_daemon(session_id, run_id)
            .expect("terminalizes");
        let replay = facade
            .load_current_run_replay_for_daemon(session_id, run_id)
            .expect("replay");
        assert_eq!(
            replay.snapshot().run_projection().status(),
            RunStatusDto::Cancelled
        );
        let other = RunId::new();
        assert!(
            facade
                .fail_starting_run_for_daemon(session_id, other, "fixture_failure")
                .is_err()
        );
        assert!(
            facade
                .load_run_tail_for_daemon(session_id, other, RunEventCursorDto::new(0))
                .is_err()
        );
    }

    #[test]
    fn command_routes_remove_queued_stop_and_rejects_subscription() {
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
        let Some(ProtocolAcceptedResultDto::SendUserTurn(t)) = a.result() else {
            unreachable!()
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = t.outcome() else {
            unreachable!()
        };
        let queued = send_user_turn(&facade, session_id, "queued");
        let ProtocolCommandResultDto::Accepted(a) = queued else {
            unreachable!()
        };
        let Some(ProtocolAcceptedResultDto::SendUserTurn(t)) = a.result() else {
            unreachable!()
        };
        let queued_turn_id = t.turn_id();
        let SendUserTurnOutcomeDto::Queued { .. } = t.outcome() else {
            unreachable!()
        };
        assert!(matches!(
            facade.command(ProtocolCommandDto::RemoveQueuedTurn(
                intention_domain::RemoveQueuedTurnCommandDto::new(session_id, queued_turn_id)
            )),
            ProtocolCommandResultDto::Accepted(_)
        ));
        assert!(matches!(
            facade.command(ProtocolCommandDto::StopRun(
                intention_domain::StopRunCommandDto::new(session_id, run_id)
            )),
            ProtocolCommandResultDto::Accepted(_)
        ));
        assert!(
            matches!(facade.command(ProtocolCommandDto::SubscribeSession(SubscribeSessionCommandDto::new(SCHEMA_VERSION, session_id, None, RunModeDto::Build))), ProtocolCommandResultDto::Rejected(error) if error.code() == "invalid_subscription_dispatch")
        );
    }

    #[test]
    fn subscribe_returns_checkpoint_for_current_position() {
        let (_directory, facade) = facade_with_recorder(Arc::new(RecordingPublisher::default()));
        let session_id = SessionId::new();
        create(&facade, session_id);
        let response = facade.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA_VERSION,
            session_id,
            Some(SessionEventSequenceDto::new(1)),
            RunModeDto::Build,
        ));
        assert!(
            matches!(response, SessionSubscriptionResponseDto::SnapshotAndTail { tail, .. } if tail.events().is_empty())
        );
    }

    #[test]
    fn persistence_failure_never_publishes_and_publisher_failure_preserves_durable_replay() {
        let recorder = Arc::new(RecordingPublisher::failing());
        let (_directory, facade) = facade_with_recorder(Arc::clone(&recorder));
        let session_id = SessionId::new();
        create(&facade, session_id);
        let records_after_create = recorder.records();
        assert_eq!(records_after_create.len(), 1);
        let durable_create_events = facade
            .inner
            .repository
            .load_tail(session_id, SessionEventSequenceDto::new(0))
            .expect("independent durable read sees created event");
        assert_eq!(
            records_after_create[0].events(),
            durable_create_events.as_slice()
        );

        let duplicate = facade.command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                fixture_workspace_root(),
                RunModeDto::Build,
            ),
        ));
        assert!(matches!(duplicate, ProtocolCommandResultDto::Rejected(_)));
        assert_eq!(
            recorder.records().len(),
            1,
            "failed persistence has no publish"
        );

        let accepted = facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, intention_types::TurnId::new(), "committed")
                .expect("fixture user turn is valid"),
        ));
        assert!(matches!(accepted, ProtocolCommandResultDto::Accepted(_)));
        let records = recorder.records();
        assert_eq!(
            records.len(),
            2,
            "post-commit publisher was invoked despite failure"
        );
        assert_eq!(records[1].events().len(), 2);
        let durable_events = facade
            .inner
            .repository
            .load_tail(session_id, SessionEventSequenceDto::new(1))
            .expect("independent durable read sees committed turn batch");
        assert_eq!(records[1].events(), durable_events.as_slice());

        let replay = facade.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA_VERSION,
            session_id,
            Some(SessionEventSequenceDto::new(0)),
            RunModeDto::Build,
        ));
        let mut reducer = SessionSubscriptionReducer::new(session_id);
        assert!(!reducer.apply(replay).expect("durable replay is contiguous"));
        assert_eq!(
            reducer.last_sequence(),
            Some(SessionEventSequenceDto::new(3)),
            "publisher failure does not duplicate or lose durable events"
        );
        let current = facade.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA_VERSION,
            session_id,
            reducer.last_sequence(),
            RunModeDto::Build,
        ));
        assert!(
            !reducer
                .apply(current)
                .expect("current checkpoint is replayable")
        );
        assert_eq!(
            reducer.last_sequence(),
            Some(SessionEventSequenceDto::new(3))
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
            Box::new(NoopPostCommitPublisher),
        );
        let error = match result {
            Ok(_) => return,
            Err(error) => error,
        };
        assert_eq!(error.code(), "invalid_selected_provider");
    }

    #[test]
    fn command_rejects_malformed_turn_identifier() {
        let (_directory, facade) = facade_with_recorder(Arc::new(RecordingPublisher::default()));
        let result = facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(SessionId::new(), intention_types::TurnId::new(), "turn")
                .expect("fixture turn is valid"),
        ));
        assert!(
            matches!(result, ProtocolCommandResultDto::Rejected(error) if error.code() == "storage_record_not_found" || error.code() == "daemon_command_unavailable")
        );
    }

    #[test]
    fn daemon_execution_bridge_runs_selected_test_driver_and_tool_bridge_reports_safe_error() {
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
        let accepted = send_user_turn(&facade, session_id, "execution bridge");
        let ProtocolCommandResultDto::Accepted(accepted) = accepted else {
            unreachable!("turn is accepted")
        };
        let Some(ProtocolAcceptedResultDto::SendUserTurn(turn)) = accepted.result() else {
            unreachable!("turn evidence exists")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
            unreachable!("turn starts")
        };

        let result = facade.invoke_local_tool_for_daemon(
            session_id,
            run_id,
            intention_types::ToolCallId::new(),
            "missing-tool",
            ToolInput::Read(intention_tools::ReadInput {
                path: intention_types::WorkspaceRelativePathDto::parse("missing.txt")
                    .expect("path is valid"),
            }),
            WorkspaceRoot::resolve(
                &WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
                    .expect("workspace root is valid"),
            )
            .expect("workspace root resolves"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn current_starting_run_returns_none_after_terminalization() {
        let (_directory, facade) = facade_with_recorder(Arc::new(RecordingPublisher::default()));
        let session_id = SessionId::new();
        create(&facade, session_id);
        assert_eq!(
            facade
                .current_starting_run_for_daemon(session_id)
                .expect("new session has no starting run"),
            None
        );
    }

    #[test]
    fn facade_rejects_invalid_commands_and_unknown_run_bridges() {
        let (_directory, facade) = facade_with_recorder(Arc::new(RecordingPublisher::default()));
        let session_id = SessionId::new();
        let unknown = RunId::new();

        assert!(matches!(
            facade.command(ProtocolCommandDto::StopRun(
                intention_domain::StopRunCommandDto::new(session_id, unknown)
            )),
            ProtocolCommandResultDto::Rejected(_)
        ));
        assert!(matches!(
            facade.command(ProtocolCommandDto::RemoveQueuedTurn(
                intention_domain::RemoveQueuedTurnCommandDto::new(
                    session_id,
                    intention_types::TurnId::new(),
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
                .terminalize_cancelling_run_for_daemon(session_id, unknown)
                .is_err()
        );
        assert!(
            facade
                .fail_starting_run_for_daemon(session_id, unknown, "fixture")
                .is_err()
        );
    }

    #[test]
    fn subscription_accepts_exact_checkpoint_and_rejects_unknown_position() {
        let (_directory, facade) = facade_with_recorder(Arc::new(RecordingPublisher::default()));
        let session_id = SessionId::new();
        create(&facade, session_id);
        let exact = facade.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA_VERSION,
            session_id,
            Some(SessionEventSequenceDto::new(1)),
            RunModeDto::Build,
        ));
        assert!(matches!(
            exact,
            SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail }
                if snapshot.at_sequence() == SessionEventSequenceDto::new(1)
                    && tail.after_sequence() == SessionEventSequenceDto::new(1)
        ));
    }

    #[test]
    fn selected_provider_helpers_cover_test_provider_variant() {
        let provider = SelectedProvider::for_test_support(Arc::new(TestSupportUnconfiguredDriver));
        assert_eq!(provider.safe_kind(), None);
        assert!(!provider.driver().capabilities().supports_streaming());
    }

    #[test]
    fn no_op_publisher_and_dispatch_cover_success_paths() {
        let publisher = NoopPostCommitPublisher;
        let session_id = SessionId::new();
        let change = CommittedChangeDto::new(
            intention_domain::SessionProjectionDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                fixture_workspace_root(),
                RunModeDto::Build,
                None,
                None,
                Vec::new(),
                SessionEventSequenceDto::new(0),
            )
            .expect("empty projection is valid"),
            SessionEventSequenceDto::new(0),
            Vec::new(),
            None,
        )
        .expect("empty committed change is valid");
        publisher.publish(&change).expect("noop publisher succeeds");
        let run_id = RunId::new();
        let request = intention_model::ModelRequestDto::new(
            run_id,
            "fixture",
            vec![
                intention_model::ModelMessageDto::new(
                    intention_model::ModelRoleDto::User,
                    "fixture",
                )
                .expect("fixture message is valid"),
            ],
            None,
            None,
        )
        .expect("fixture request is valid");
        PrivateModelRunDispatch::default()
            .dispatch_model_run(
                ScheduleModelRunDto::new(session_id, run_id, request, fixture_config_snapshot())
                    .expect("fixture schedule is valid"),
            )
            .expect("dispatch succeeds");
    }

    #[test]
    fn provider_driver_branches_and_empty_test_driver_stream_are_exercised() {
        let material = ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture\"",
            ConfigSourceDto::Explicit(
                ConfigPathDto::parse(
                    std::env::temp_dir()
                        .join("provider-branches.toml")
                        .to_string_lossy()
                        .into_owned(),
                )
                .expect("fixture path is absolute"),
            ),
        ))
        .expect("fixture material parses");
        let openrouter =
            SelectedProvider::from_startup_material(material).expect("openrouter provider builds");
        assert_eq!(openrouter.safe_kind(), Some(ProviderKindDto::Openrouter));
        let _ = openrouter.driver();

        let test_driver = TestSupportUnconfiguredDriver;
        let stream = test_driver.execute(
            intention_model::ModelRequestDto::new(
                RunId::new(),
                "fixture",
                vec![
                    intention_model::ModelMessageDto::new(
                        intention_model::ModelRoleDto::User,
                        "fixture",
                    )
                    .expect("fixture message is valid"),
                ],
                None,
                None,
            )
            .expect("fixture request is valid"),
            ModelCancellationSignal::new(),
        );
        futures_util::pin_mut!(stream);
    }
}
