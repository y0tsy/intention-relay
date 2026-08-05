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
    ApplicationService, CreateSessionWorkflowInputDto, ModelRunDispatchPort, ScheduleModelRunDto,
    SendUserTurnWorkflowInputDto,
};
#[cfg(test)]
use intention_config::ConfigPathDto;
use intention_config::{
    ConfigPathResolver, ConfigSnapshotDto, ConfigSourceDto, ProviderKindDto, RawConfigInputDto,
    ResolvedConfigDto, StartupProviderMaterial,
};
use intention_domain::GetSessionSnapshotQueryDto;
#[cfg(test)]
use intention_domain::{CreateSessionCommandDto, RunModeDto, WorkspaceRootDto};
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
use intention_runtime::{RuntimeService, RuntimeValuesDto};
use intention_storage::{CommittedChangeDto, StorageRepositoryDto};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use intention_types::{
    ConfigRevisionId, CorrelationIdDto, DtoResult, ErrorDto, RunId, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto,
};
#[cfg(test)]
use intention_types::{ProjectId, WorkspaceId};

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
    TestSupport,
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
    const fn for_test_support() -> Self {
        Self::TestSupport
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
            Self::TestSupport => None,
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
    /// Receives an event batch only after an independent durable read observed it.
    // @todo(m4-streaming)
    fn publish(&self, change: &CommittedChangeDto) -> DtoResult<()>;
}

struct NoopPostCommitPublisher;

impl PostCommitPublisher for NoopPostCommitPublisher {
    fn publish(&self, _change: &CommittedChangeDto) -> DtoResult<()> {
        Ok(())
    }
}

impl DaemonApplicationFacade {
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
    pub fn open_for_test_support(
        database_location: impl AsRef<Path>,
        config_snapshot: ConfigSnapshotDto,
    ) -> DtoResult<Self> {
        Self::open_with_selected_provider(
            database_location,
            config_snapshot,
            SelectedProvider::for_test_support(),
            Box::new(NoopPostCommitPublisher),
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
            SelectedProvider::for_test_support(),
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
            SelectedProvider::for_test_support(),
            publisher,
        )
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
    // @todo(m4-streaming)
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
    // @todo(m4-streaming)
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
}
