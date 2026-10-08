//! Durable M3 composition root for the daemon host.
//!
//! Only this module selects SQLite and the provider driver. Raw configuration,
//! database resources, locations, and the selected driver stay private: the
//! host reaches the engine, the repository, and the selected driver through the
//! crate-private accessors below, and the wire surface is served from this one
//! composition.

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
use intention_engine::ApplicationService;
#[cfg(test)]
use intention_engine::{ModelRunCommitObserver, ToolInvocationRequestDto};
#[cfg(test)]
use intention_proto::SendUserTurnOutcomeDto;
use intention_proto::{
    ConfigRevisionId, CorrelationIdDto, DtoResult, ErrorDto, RunId, SchemaVersionDto, SessionId,
    TimestampDto,
};
#[cfg(test)]
use intention_proto::{
    CreateSessionCommandDto, ProjectId, RunModeDto, WorkspaceId, WorkspaceRootDto,
};
use intention_proto::{
    DaemonHealthDto, DaemonReadinessDto, ProtocolAcceptedDto, ProtocolAcceptedResultDto,
    ProtocolCommandDto, ProtocolCommandResultDto, ProtocolQueryDto, ProtocolQueryResultDto,
    SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
};
use intention_providers::GenericChatDriver;
use intention_providers::ModelExecutionDriver;
use intention_providers::OpenRouterDriver;
#[cfg(any(test, feature = "test-support"))]
use intention_providers::{ModelCancellationSignal, ModelCapabilitiesDto, ModelEventStream};
#[cfg(test)]
use intention_storage::ToolResultEvidenceDto;
use intention_storage::{
    RecoverUnfinishedRunsInputDto, SqliteDatabaseLocationDto, SqliteStorageRepository,
    StorageRepositoryDto,
};
#[cfg(test)]
use intention_tools::{ToolInput, WorkspaceRoot};
use intention_transport::MAX_TRANSCRIPT_SNAPSHOT_BYTES;

const SCHEMA_VERSION: SchemaVersionDto = intention_proto::CURRENT_DTO_SCHEMA_VERSION;
/// The single live configuration snapshot schema (intention-config current schema).
const CONFIG_SCHEMA_VERSION: SchemaVersionDto = SchemaVersionDto::new(1, 0);
const DATABASE_FILENAME: &str = "intention-relay.sqlite";
/// The retained bounded size of a current-state snapshot's recent transcript.
///
/// The row bound is additionally held to `MAX_TRANSCRIPT_SNAPSHOT_BYTES`, the
/// representation budget derived from the single transport envelope cap.
pub const SESSION_SNAPSHOT_MESSAGES: u32 = 256;

/// The one daemon composition root.
///
/// It owns durable storage, the configuration snapshot, the selected provider
/// driver, and command-admission serialization. The daemon host reaches the
/// engine and the repository through the crate-private accessors instead of a
/// `*_for_daemon*` bridge surface.
#[derive(Clone)]
pub struct DaemonApplicationFacade {
    inner: Arc<FacadeInner>,
}

struct FacadeInner {
    repository: SqliteStorageRepository,
    config_snapshot: ConfigSnapshotDto,
    selected_provider: SelectedProvider,
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
impl ModelExecutionDriver for TestSupportUnconfiguredDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(false, false, false, false, false, false)
    }

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
        let selected = match material.safe_resolved().provider().kind() {
            ProviderKindDto::Openrouter => {
                OpenRouterDriver::from_startup_material(material).map(Self::OpenRouter)?
            }
            ProviderKindDto::GenericChatCompletionApi => {
                GenericChatDriver::from_startup_material(material).map(Self::GenericChat)?
            }
        };
        // The runtime streams text with tool calls, so the single capability
        // negotiation happens once at provider selection.
        selected
            .driver()
            .capabilities()
            .ensure_runtime_requirements()?;
        Ok(selected)
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
}

impl DaemonApplicationFacade {
    /// Returns the one durable repository this composition root owns.
    pub(crate) fn repository(&self) -> &SqliteStorageRepository {
        &self.inner.repository
    }

    /// Returns the selected provider driver to the daemon host.
    pub(crate) fn driver(&self) -> &(dyn ModelExecutionDriver + Send + Sync) {
        self.inner.selected_provider.driver()
    }

    /// Serializes durable command handling against interrupts and failures.
    pub(crate) fn command_gate(&self) -> &Mutex<()> {
        &self.inner.command_gate
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

    fn open_with_selected_provider(
        database_location: impl AsRef<Path>,
        config_snapshot: ConfigSnapshotDto,
        selected_provider: SelectedProvider,
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
                selected_provider,
                command_gate: Mutex::new(()),
            }),
        };
        facade.recover_before_ready()?;
        Ok(facade)
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
                ApplicationService::new(&self.inner.repository).send_user_turn(
                    command,
                    proposed_run_id,
                    self.inner.config_snapshot.clone(),
                    timestamp,
                )?
            }
            ProtocolCommandDto::RemoveTurn(command) => {
                ApplicationService::new(&self.inner.repository).remove_turn(command, timestamp)?
            }
            // Interrupts and subscriptions are routed by the daemon host: an
            // interrupt signals the registered run and a subscription answers
            // with its own dedicated protocol response.
            ProtocolCommandDto::InterruptRun(_) | ProtocolCommandDto::SubscribeSession(_) => {
                return Err(ErrorDto::validation(
                    "daemon_command_route_unavailable",
                    "this command is routed by the daemon host",
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
pub fn bounded_snapshot_messages(
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
    use intention_engine::{ModelRunCommitDto, RunCancellation};
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

    /// Reads the committed transcript rows of one run through the current-state read.
    fn run_messages(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        run_id: RunId,
    ) -> Vec<intention_proto::MessageProjectionDto> {
        facade
            .session_snapshot(session_id, Some(run_id))
            .expect("committed run transcript reads")
            .messages()
            .to_vec()
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
        DaemonApplicationFacade::open_with_selected_provider(
            directory.path().join("provider.sqlite"),
            snapshot.clone(),
            selected_provider,
        )
        .expect("selected provider remains owned by the facade");

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
    fn daemon_interrupt_ends_the_in_flight_execute_with_a_partial_result() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "in flight interrupt");
        let (workspace_directory, workspace) = workspace_fixture("keep.txt", "kept");
        let sentinel = workspace_directory.path().join("sentinel.txt");
        let call_id = intention_proto::ToolCallId::new();
        // The one per-run cancellation handle the daemon host registers is the
        // same handle the in-flight tool invocation observes.
        let cancellation = RunCancellation::new();
        let worker_cancellation = cancellation.clone();
        let worker_facade = facade.clone();
        let worker_session = session_id;
        let worker_run = run_id;

        let worker = std::thread::spawn(move || {
            ApplicationService::new(worker_facade.repository()).invoke_local_tool_with_publication(
                ToolInvocationRequestDto::new(
                    workspace,
                    worker_session,
                    worker_run,
                    call_id,
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
                                intention_tools::BoundedText::new(
                                    "printf x > sentinel.txt; sleep 2",
                                )
                                .expect("arg"),
                            ]
                        },
                    }),
                    now().expect("fixture clock reads"),
                )
                .with_arguments_json("{}")
                .with_cancellation(worker_cancellation),
                &RecordingPublisher::new(),
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

        cancellation.cancel();

        worker
            .join()
            .expect("worker completes")
            .expect("in-flight execution observes the interrupt as a partial outcome");
        let evidence = tool_evidence(&facade, session_id, run_id, call_id);
        assert_eq!(
            evidence.status(),
            ToolResultStatusDto::Partial,
            "the stopped execute commits its partial result"
        );
        assert_eq!(
            facade
                .session_snapshot(session_id, Some(run_id))
                .expect("interrupted run reads")
                .projection()
                .active_run()
                .map(|run| run.run_id()),
            Some(run_id),
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
        let cancellation = RunCancellation::new();

        let call_ids = [
            intention_proto::ToolCallId::new(),
            intention_proto::ToolCallId::new(),
        ];
        for call_id in call_ids {
            ApplicationService::new(facade.repository())
                .invoke_local_tool_with_publication(
                    ToolInvocationRequestDto::new(
                        workspace.clone(),
                        session_id,
                        run_id,
                        call_id,
                        "read",
                        read_hello_input(),
                        now().expect("fixture clock reads"),
                    )
                    .with_arguments_json("{}")
                    .with_cancellation(cancellation.clone()),
                    &RecordingPublisher::new(),
                )
                .expect("reads complete before any interrupt");
        }

        // The interrupt arrives after the effects committed: it never rewrites
        // completed evidence or terminalizes the run.
        cancellation.cancel();
        assert_eq!(
            facade
                .session_snapshot(session_id, Some(run_id))
                .expect("the interrupted run stays readable")
                .projection()
                .active_run()
                .map(|run| run.run_id()),
            Some(run_id),
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
    fn committed_tool_rows_are_published_in_commit_order() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "publication path");
        let (_workspace_directory, workspace) = workspace_fixture("hello.txt", "hello");

        let call_id = intention_proto::ToolCallId::new();
        let publisher = RecordingPublisher::new();
        ApplicationService::new(facade.repository())
            .invoke_local_tool_with_publication(
                ToolInvocationRequestDto::new(
                    workspace,
                    session_id,
                    run_id,
                    call_id,
                    "read",
                    read_hello_input(),
                    now().expect("fixture clock reads"),
                )
                .with_arguments_json("{}"),
                &publisher,
            )
            .expect("read completes");

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
