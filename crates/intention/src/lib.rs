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
    ApplicationService, CreateSessionWorkflowInputDto, SendUserTurnWorkflowInputDto,
};
use intention_config::{
    ConfigPathDto, ConfigPathResolver, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto,
    ResolvedConfigDto,
};
use intention_domain::{
    CreateSessionCommandDto, GetSessionSnapshotQueryDto, RunModeDto, WorkspaceRootDto,
};
use intention_protocol::{
    DaemonHealthDto, DaemonReadinessDto, ProtocolAcceptedDto, ProtocolAcceptedResultDto,
    ProtocolCommandDto, ProtocolCommandResultDto, ProtocolQueryDto, ProtocolQueryResultDto,
    SessionEventTailBatchDto, SessionResyncDto, SessionResyncReasonDto, SessionSnapshotDto,
    SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
};
use intention_runtime::{RuntimeService, RuntimeValuesDto};
use intention_storage::{CommittedChangeDto, StorageRepositoryDto};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use intention_types::{
    ConfigRevisionId, CorrelationIdDto, DtoResult, ErrorDto, ProjectId, RunId, SchemaVersionDto,
    SessionEventSequenceDto, SessionId, TimestampDto, WorkspaceId,
};

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
    publisher: Box<dyn PostCommitPublisher>,
    command_gate: Mutex<()>,
    fixture_session_id: Option<SessionId>,
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
        let config_snapshot = load_platform_config_snapshot()?;
        Self::open_with_publisher(
            platform_database_location()?,
            config_snapshot,
            Box::new(NoopPostCommitPublisher),
        )
    }

    /// Opens a caller-provided absolute database exclusively for tests or controlled fixtures.
    ///
    /// # Errors
    ///
    /// Returns a safe typed storage or recovery error. The supplied local path is
    /// never retained in a public DTO or error.
    pub fn open_for_test(
        database_location: impl AsRef<Path>,
        config_snapshot: ConfigSnapshotDto,
    ) -> DtoResult<Self> {
        Self::open_with_publisher(
            database_location,
            config_snapshot,
            Box::new(NoopPostCommitPublisher),
        )
    }

    fn open_with_publisher(
        database_location: impl AsRef<Path>,
        config_snapshot: ConfigSnapshotDto,
        publisher: Box<dyn PostCommitPublisher>,
    ) -> DtoResult<Self> {
        let location = SqliteDatabaseLocationDto::new(
            database_location.as_ref().to_string_lossy().into_owned(),
        )?;
        let repository = SqliteStorageRepository::open(location)?;
        repository.accept_configuration_revision(config_snapshot.clone())?;
        let facade = Self {
            inner: Arc::new(FacadeInner {
                repository,
                config_snapshot,
                publisher,
                command_gate: Mutex::new(()),
                fixture_session_id: None,
            }),
        };
        facade.recover_before_ready()?;
        Ok(facade)
    }

    /// Creates a durable fixture database and initial session for M2 transport tests.
    #[must_use]
    pub fn new_fixture_with_session_id(session_id: SessionId) -> Self {
        let directory =
            std::env::temp_dir().join(format!("intention-relay-fixture-{}", RunId::new()));
        fs::create_dir_all(&directory)
            .unwrap_or_else(|_| unreachable!("fixture durable storage directory must open"));
        let mut facade =
            Self::open_for_test(directory.join(DATABASE_FILENAME), fixture_config_snapshot())
                .unwrap_or_else(|_| unreachable!("fixture durable storage must open"));
        let command = CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse("/m2-fixture-workspace")
                .unwrap_or_else(|_| unreachable!("fixture workspace must be valid")),
            RunModeDto::Build,
        );
        let _ = facade.command(ProtocolCommandDto::CreateSession(command));
        Arc::get_mut(&mut facade.inner)
            .unwrap_or_else(|| unreachable!("fixture facade is not shared during initialization"))
            .fixture_session_id = Some(session_id);
        facade
    }

    /// Creates a durable fixture session for existing M2 transport tests.
    #[must_use]
    pub fn new_fixture() -> Self {
        Self::new_fixture_with_session_id(SessionId::new())
    }

    /// Returns a credential-free ready health projection.
    #[must_use]
    pub const fn health(&self) -> DaemonHealthDto {
        DaemonHealthDto::new(SCHEMA_VERSION, PROTOCOL_VERSION, DaemonReadinessDto::Ready)
    }

    /// Returns an explicit M2 fixture identity for compatibility-only tests.
    #[must_use]
    pub fn fixture_session_id(&self) -> SessionId {
        self.inner.fixture_session_id.unwrap_or_else(SessionId::new)
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
        if let Some(run_id) = command.run_id() {
            let run_belongs_to_session = self
                .inner
                .repository
                .load_tail(command.session_id(), SessionEventSequenceDto::new(0))
                .is_ok_and(|events| events.iter().any(|event| event.run_id() == Some(run_id)));
            if !run_belongs_to_session {
                return resync(
                    command.session_id(),
                    SessionResyncReasonDto::HistoryUnavailable,
                );
            }

            // A session projection includes unrelated active and queued state, and a
            // run-filtered tail cannot remain contiguous in session sequence order.
            // The current protocol has no run-scoped snapshot/tail representation.
            return resync(
                command.session_id(),
                SessionResyncReasonDto::HistoryUnavailable,
            );
        }
        if requested_after == current.at_sequence() {
            let tail = SessionEventTailBatchDto::new(
                SCHEMA_VERSION,
                command.session_id(),
                requested_after,
                Vec::new(),
            )
            .unwrap_or_else(|_| unreachable!("empty durable tail must be valid"));
            return SessionSubscriptionResponseDto::snapshot_and_tail(current, tail)
                .unwrap_or_else(|_| unreachable!("current snapshot and empty tail must agree"));
        }
        let events = match self
            .inner
            .repository
            .load_tail(command.session_id(), requested_after)
        {
            Ok(events) => events,
            Err(_) => {
                return resync(
                    command.session_id(),
                    SessionResyncReasonDto::HistoryUnavailable,
                );
            }
        };
        let snapshot =
            SessionSnapshotDto::new(SCHEMA_VERSION, command.session_id(), requested_after);
        let tail = match SessionEventTailBatchDto::new(
            SCHEMA_VERSION,
            command.session_id(),
            requested_after,
            events,
        ) {
            Ok(tail) => tail,
            Err(_) => {
                return resync(
                    command.session_id(),
                    SessionResyncReasonDto::HistoryUnavailable,
                );
            }
        };
        SessionSubscriptionResponseDto::snapshot_and_tail(snapshot, tail).unwrap_or_else(|_| {
            unreachable!("durable replay snapshot and tail share session and checkpoint")
        })
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

    /// Returns the fixed fixture workspace without a production-path claim.
    ///
    /// # Errors
    ///
    /// Returns only a DTO validation error if the fixed fixture value changes.
    pub fn fixture_workspace(&self) -> DtoResult<WorkspaceRootDto> {
        WorkspaceRootDto::parse("/m2-fixture-workspace")
    }

    /// Returns the fixed fixture mode without a production-state claim.
    #[must_use]
    pub const fn fixture_mode(&self) -> RunModeDto {
        RunModeDto::Build
    }

    /// Generates a fixture-only project identity.
    #[must_use]
    pub fn fixture_project_id(&self) -> ProjectId {
        ProjectId::new()
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
                ApplicationService::new(&self.inner.repository).send_user_turn(
                    command,
                    SendUserTurnWorkflowInputDto::new(
                        RunId::new(),
                        self.inner.config_snapshot.clone(),
                        timestamp,
                    ),
                )?
            }
            ProtocolCommandDto::RemoveQueuedTurn(command) => {
                ApplicationService::new(&self.inner.repository)
                    .remove_queued_turn(command, timestamp)?
            }
            ProtocolCommandDto::StopRun(command) => ApplicationService::new(&self.inner.repository)
                .stop_run(
                    command,
                    RuntimeValuesDto::new(
                        RunId::new(),
                        self.inner.config_snapshot.clone(),
                        timestamp,
                    ),
                )?,
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

fn load_platform_config_snapshot() -> DtoResult<ConfigSnapshotDto> {
    load_config_snapshot(ConfigPathResolver::resolve(None)?)
}

fn load_config_snapshot(source: ConfigSourceDto) -> DtoResult<ConfigSnapshotDto> {
    #[cfg(unix)]
    intention_config::ensure_user_only_permissions(source.path())?;
    let raw_toml = fs::read_to_string(source.path().as_str()).map_err(|_| {
        ErrorDto::unavailable(
            "daemon_configuration_read_unavailable",
            "daemon configuration could not be read",
        )
    })?;
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(raw_toml, source))?;
    ConfigSnapshotDto::new(SCHEMA_VERSION, ConfigRevisionId::new(), now()?, resolved)
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

fn fixture_config_snapshot() -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse("/tmp/intention-relay-fixture.toml")
            .unwrap_or_else(|_| unreachable!("fixture config path must be absolute")),
    );
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential\"",
        source,
    ))
    .unwrap_or_else(|_| unreachable!("fixture config must resolve"));
    ConfigSnapshotDto::new(
        SCHEMA_VERSION,
        ConfigRevisionId::new(),
        TimestampDto::from_unix_seconds(1)
            .unwrap_or_else(|_| unreachable!("fixture timestamp must be valid")),
        resolved,
    )
    .unwrap_or_else(|_| unreachable!("fixture snapshot must be valid"))
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

    fn create(facade: &DaemonApplicationFacade, session_id: SessionId) {
        let accepted = facade.command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                WorkspaceRootDto::parse("/workspace/intention-composition")
                    .expect("fixture workspace is absolute"),
                RunModeDto::Build,
            ),
        ));
        assert!(matches!(accepted, ProtocolCommandResultDto::Accepted(_)));
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
    fn fixture_helpers_expose_durable_m2_compatibility_values() {
        let facade = DaemonApplicationFacade::new_fixture();
        let session_id = facade.fixture_session_id();
        assert_eq!(facade.health().readiness(), DaemonReadinessDto::Ready);
        assert_eq!(
            facade
                .fixture_workspace()
                .expect("fixture workspace resolves")
                .as_str(),
            "/m2-fixture-workspace"
        );
        assert_eq!(facade.fixture_mode(), RunModeDto::Build);
        let _ = facade.fixture_project_id();
        assert!(matches!(
            facade.query(ProtocolQueryDto::GetSessionSnapshot(
                GetSessionSnapshotQueryDto::new(session_id)
            )),
            ProtocolQueryResultDto::SessionSnapshot(_)
        ));
        assert!(matches!(
            facade.command(ProtocolCommandDto::SubscribeSession(
                SubscribeSessionCommandDto::new(
                    SCHEMA_VERSION,
                    session_id,
                    None,
                    RunModeDto::Build,
                )
            )),
            ProtocolCommandResultDto::Rejected(error)
                if error.code() == "invalid_subscription_dispatch"
        ));
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
                WorkspaceRootDto::parse("/workspace/intention-composition")
                    .expect("fixture workspace is absolute"),
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
