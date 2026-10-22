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

use intention_config::{
    ConfigPathDto, ConfigPathResolver, ConfigSnapshotDto, ProviderKindDto, RawConfigInputDto,
    ResolvedConfigDto, StartupProviderMaterial,
};
#[cfg(test)]
use intention_engine::ToolInvocationRequestDto;
use intention_engine::{ApplicationService, SessionCommitDto, SessionCommitObserver};
use intention_proto::DaemonHealthDto;
#[cfg(test)]
use intention_proto::WorkspaceRootDto;
use intention_proto::{
    CancelRunCommandDto, ConfigRevisionId, CreateSessionAcceptedDto, CreateSessionCommandDto,
    DtoResult, ErrorDto, ProtocolResultDto, RemoveTurnAcceptedDto, RemoveTurnCommandDto,
    RunCancelledAcceptedDto, RunId, SendUserTurnAcceptedDto, SendUserTurnCommandDto,
    SendUserTurnOutcomeDto, SessionId, SessionSnapshotDto, SessionSummariesDto, TimestampDto,
};
use intention_providers::GenericChatDriver;
use intention_providers::ModelExecutionDriver;
use intention_providers::OpenRouterDriver;
#[cfg(any(test, feature = "test-support"))]
use intention_providers::{ModelCancellationSignal, ModelCapabilitiesDto, ModelEventStream};
use intention_storage::{
    AcceptedTurnOutcomeDto, SqliteDatabaseLocationDto, SqliteStorageRepository,
    StorageRepositoryDto,
};
#[cfg(test)]
use intention_tools::WorkspaceRoot;
use intention_transport::MAX_TRANSCRIPT_SNAPSHOT_BYTES;

const DATABASE_FILENAME: &str = "intention-relay.sqlite";
/// The retained bounded size of a current-state snapshot's recent transcript.
///
/// The row bound is additionally held to `MAX_TRANSCRIPT_SNAPSHOT_BYTES`, the
/// representation budget derived from the single transport envelope cap.
pub const SESSION_SNAPSHOT_MESSAGES: u32 = 256;

/// The retained bounded window of one current session list.
///
/// The storage read reports every session beyond this window as its explicit
/// `omitted` count, so a bounded list is never a silent truncation.
pub const SESSION_LIST_ROWS: u32 = 256;

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
    selected_provider: ConfiguredProvider,
    command_gate: CommandGate,
}

/// The one durable-command admission gate of this composition root.
///
/// A durable command is a multi-step sequence: it reads current durable state
/// and then commits a transition, so two commands that interleave their steps
/// could both decide from the same pre-state and break the single-writer rule
/// the durable run registry depends on. The gate serializes exactly those
/// sequences against each other. A pure read — the session snapshot, the run
/// snapshot a subscription answers with, and the interrupt validation — never
/// takes it. `run` is the gate's only entry point, so a command path cannot run
/// ungated by construction.
///
/// Ordering: the gate is an independent lock, and it is never held together
/// with the daemon host's registry lock. Every gated closure reaches durable
/// storage, which takes the repository's one connection lock, so the only
/// ordering relations are gate → storage and registry → storage: storage is
/// always the last lock taken, and no lock is taken while the connection lock
/// is held.
pub struct CommandGate {
    inner: Mutex<()>,
}

impl CommandGate {
    const fn new() -> Self {
        Self {
            inner: Mutex::new(()),
        }
    }

    /// Runs one durable multi-step command sequence as the single writer.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when an earlier command panicked
    /// while holding the gate.
    pub fn run<T>(&self, command: impl FnOnce() -> DtoResult<T>) -> DtoResult<T> {
        let _gate = self.inner.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_command_unavailable",
                "daemon command is unavailable",
            )
        })?;
        command()
    }
}

/// The one provider driver slot this composition root serves.
///
/// A production build holds one of the configured providers; a test or
/// test-support build additionally holds the driver the caller injected, so no
/// production shape carries a test-only variant.
enum ConfiguredProvider {
    OpenRouter(OpenRouterDriver),
    GenericChat(GenericChatDriver),
    /// The driver a test or test-support caller supplied.
    #[cfg(any(test, feature = "test-support"))]
    Injected(Arc<dyn ModelExecutionDriver + Send + Sync>),
}

#[cfg(any(test, feature = "test-support"))]
struct TestSupportUnconfiguredDriver;

#[cfg(any(test, feature = "test-support"))]
impl ModelExecutionDriver for TestSupportUnconfiguredDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(false)
    }

    fn execute(
        &self,
        _request: intention_providers::ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        Box::pin(futures_util::stream::empty())
    }
}

impl ConfiguredProvider {
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

    fn driver(&self) -> &(dyn ModelExecutionDriver + Send + Sync) {
        match self {
            Self::OpenRouter(driver) => driver,
            Self::GenericChat(driver) => driver,
            #[cfg(any(test, feature = "test-support"))]
            Self::Injected(driver) => driver.as_ref(),
        }
    }

    /// Holds the driver a test or test-support caller supplied.
    #[cfg(any(test, feature = "test-support"))]
    const fn for_test_support(driver: Arc<dyn ModelExecutionDriver + Send + Sync>) -> Self {
        Self::Injected(driver)
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

    /// Returns the durable-command admission gate to the daemon host.
    pub(crate) fn command_gate(&self) -> &CommandGate {
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
            ConfiguredProvider::for_test_support(driver),
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
        selected_provider: ConfiguredProvider,
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
                command_gate: CommandGate::new(),
            }),
        };
        facade.recover_before_ready()?;
        Ok(facade)
    }

    /// Returns a credential-free ready health projection.
    #[must_use]
    pub const fn health(&self) -> DaemonHealthDto {
        DaemonHealthDto::ready()
    }

    /// Loads one coherent current-state session snapshot.
    ///
    /// This is the single session read: recent transcript rows are bounded by
    /// the retained delivery bound and the byte budget derived from the
    /// transport envelope cap, which charges the encoded projection as well as
    /// the transcript rows. The same snapshot answers a session subscription,
    /// which the daemon host reads from this same method.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the session projection or its
    /// transcript rows cannot be read.
    pub fn session_snapshot(&self, session_id: SessionId) -> DtoResult<SessionSnapshotDto> {
        let projection = self.inner.repository.load_session_projection(session_id)?;
        // The effective provider model is this daemon's resolved configuration
        // selection: storage cannot know it. Both the one-shot session read and
        // the subscription-open snapshot answer through this one method, so the
        // two can never disagree about the model.
        let projection = projection
            .with_provider_model(self.inner.config_snapshot.resolved().provider().model())?;
        let projection_bytes =
            CountingWriter::encoded_bytes(|writer| serde_json::to_writer(writer, &projection))?;
        let messages = bounded_snapshot_messages(
            projection_bytes,
            self.inner
                .repository
                .load_recent_messages(session_id, SESSION_SNAPSHOT_MESSAGES)?,
        )?;
        SessionSnapshotDto::with_projection(session_id, projection, messages)
    }

    /// Lists the current durable sessions newest-first in one bounded window.
    ///
    /// This is the one session list: it reports which sessions exist, with the
    /// newest durable update first and the durable identity breaking ties, and
    /// how many sessions exist beyond the window. A session a client selects is
    /// then read through the full session snapshot.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when durable storage cannot be read.
    pub fn list_sessions(&self, limit: u32) -> DtoResult<SessionSummariesDto> {
        self.inner.repository.list_sessions(limit)
    }

    /// Creates one durable session and assembles its typed reply evidence.
    ///
    /// The creation and the first turn the command carries commit inside one
    /// command gate, so a prompt that needs a session is one command: the
    /// session commits first and the turn is accepted against it. The committed
    /// turn reaches `observer` the way every other durable commit does.
    ///
    /// # Errors
    ///
    /// Returns the typed durable failure when session creation is rejected, or
    /// the typed admission failure when a carried first turn is rejected.
    pub fn create_session(
        &self,
        command: CreateSessionCommandDto,
        observer: &dyn SessionCommitObserver,
    ) -> DtoResult<ProtocolResultDto> {
        self.inner.command_gate.run(|| {
            let created = ApplicationService::new(&self.inner.repository).create_session(
                command,
                self.inner.config_snapshot.clone(),
                now()?,
                observer,
            )?;
            Ok(ProtocolResultDto::SessionCreated(
                CreateSessionAcceptedDto::new(
                    created.session().project_id(),
                    created.session().workspace_id(),
                    created.session().session_id(),
                    created.first_turn().map(turn_acceptance),
                ),
            ))
        })
    }

    /// Accepts one user turn and assembles its typed reply evidence.
    ///
    /// The proposed run identity is derived from the caller's idempotency key,
    /// so a repeated command proposes the identity the durable turn already
    /// recorded and the repository replays its outcome instead of reporting a
    /// conflict. Both identities are canonical UUIDs, so one caller identity
    /// selects exactly one run.
    ///
    /// `observer` receives the committed acceptance the way every other durable
    /// commit reaches the host's one sink, so a session's subscribers learn of
    /// the turn and the run it started.
    ///
    /// # Errors
    ///
    /// Returns the typed admission failure when the turn is rejected.
    pub fn send_user_turn(
        &self,
        command: SendUserTurnCommandDto,
        observer: &dyn SessionCommitObserver,
    ) -> DtoResult<ProtocolResultDto> {
        self.inner.command_gate.run(|| {
            let proposed_run_id = RunId::parse(&command.idempotency_key().to_string())?;
            let outcome = ApplicationService::new(&self.inner.repository).send_user_turn(
                command,
                proposed_run_id,
                self.inner.config_snapshot.clone(),
                now()?,
                observer,
            )?;
            Ok(turn_accepted(&outcome))
        })
    }

    /// Removes one not-yet-seen pending user turn and assembles its typed reply evidence.
    ///
    /// # Errors
    ///
    /// Returns the typed durable failure when no pending turn can be removed.
    pub fn remove_turn(&self, command: RemoveTurnCommandDto) -> DtoResult<ProtocolResultDto> {
        self.inner.command_gate.run(|| {
            let turn =
                ApplicationService::new(&self.inner.repository).remove_turn(command, now()?)?;
            Ok(ProtocolResultDto::TurnRemoved(RemoveTurnAcceptedDto::new(
                turn.session_id(),
                turn.turn_id(),
            )))
        })
    }

    /// Validates one stop request and assembles its typed reply evidence.
    ///
    /// The stop validates the exact active run and commits nothing itself, so
    /// it is a pure read and never enters the command gate. The daemon host
    /// signals the registered execution with the terminal intent afterwards,
    /// and the host registry lock is what orders that signal against admission.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error when the exact run is not active.
    pub fn cancel_run(&self, command: CancelRunCommandDto) -> DtoResult<ProtocolResultDto> {
        let run = ApplicationService::new(&self.inner.repository)
            .active_run(command.session_id(), command.run_id())?;
        Ok(ProtocolResultDto::RunCancelled(
            RunCancelledAcceptedDto::new(run.session_id(), run.run_id()),
        ))
    }

    fn recover_before_ready(&self) -> DtoResult<()> {
        let _interrupted = self.inner.repository.recover_unfinished_runs(now()?)?;
        Ok(())
    }
}

/// A commit sink that records nothing.
///
/// A caller that drives the facade without a live host - the shared fixtures and
/// the in-crate tests - still hands every committed value to the one
/// publication seam, without asserting on what it observes.
#[derive(Debug)]
pub struct SilentCommitObserver;

impl SilentCommitObserver {
    /// Creates the silent sink.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for SilentCommitObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionCommitObserver for SilentCommitObserver {
    fn observe_session_commit(&self, _committed: &SessionCommitDto) {}
}

/// Assembles the wire acceptance evidence of one committed user-turn outcome.
///
/// The engine returns the committed durable outcome; the boundary that owns the
/// typed wire vocabulary converts it once here.
const fn turn_accepted(outcome: &AcceptedTurnOutcomeDto) -> ProtocolResultDto {
    ProtocolResultDto::TurnAccepted(turn_acceptance(outcome))
}

/// Converts one committed durable turn outcome into its wire acceptance evidence.
const fn turn_acceptance(outcome: &AcceptedTurnOutcomeDto) -> SendUserTurnAcceptedDto {
    match outcome {
        AcceptedTurnOutcomeDto::Started { run, .. } => SendUserTurnAcceptedDto::new(
            run.session_id(),
            run.turn_id(),
            SendUserTurnOutcomeDto::Started {
                run_id: run.run_id(),
                config_revision_id: run.config_revision_id(),
            },
        ),
        AcceptedTurnOutcomeDto::Pending(turn) => SendUserTurnAcceptedDto::new(
            turn.session_id(),
            turn.turn_id(),
            SendUserTurnOutcomeDto::Pending,
        ),
    }
}

fn load_platform_provider_configuration() -> DtoResult<(ConfigSnapshotDto, ConfiguredProvider)> {
    load_provider_configuration(&ConfigPathResolver::resolve(None)?)
}

fn load_provider_configuration(
    path: &ConfigPathDto,
) -> DtoResult<(ConfigSnapshotDto, ConfiguredProvider)> {
    let raw_toml = fs::read_to_string(path.as_str()).map_err(|_| {
        ErrorDto::unavailable(
            "daemon_configuration_read_unavailable",
            "daemon configuration could not be read",
        )
    })?;
    let material = ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(raw_toml))?;
    let snapshot = ConfigSnapshotDto::new(
        ConfigRevisionId::new(),
        now()?,
        material.safe_resolved().clone(),
    );
    let selected_provider = ConfiguredProvider::from_startup_material(material)?;
    Ok((snapshot, selected_provider))
}

/// Counts the bytes one typed value encodes to without buffering them.
///
/// The snapshot budget needs the encoded length only, so encoding writes into
/// this counter instead of one intermediate `Vec<u8>` per charged value.
struct CountingWriter(usize);

impl CountingWriter {
    /// Returns the encoded byte count of one value written by `encode`.
    ///
    /// # Errors
    ///
    /// Returns the typed local encode failure when the value cannot be encoded.
    fn encoded_bytes(
        encode: impl FnOnce(&mut Self) -> Result<(), serde_json::Error>,
    ) -> DtoResult<usize> {
        let mut counter = Self(0);
        encode(&mut counter).map_err(|_| encode_failure())?;
        Ok(counter.0)
    }
}

impl std::io::Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(buffer.len());
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Trims one snapshot transcript to the representation budget derived from the
/// single transport envelope cap, keeping the newest committed rows that fit.
///
/// The bound is the snapshot's liveness guarantee: a transcript snapshot is the
/// only legitimate response large enough to approach the envelope cap, so the
/// read path trims it here and the transport keeps the cap. Without the bound a
/// legitimate bounded-row snapshot could exceed the cap and be rejected at the
/// connection level, so it stays.
///
/// Each row is charged its exact encoded size, because the size decides which
/// rows survive: a cheaper estimate would either cut rows that fit or let the
/// snapshot cross the cap. The loop walks newest-first and stops at the first
/// row that does not fit, so only the served rows and the first rejected one
/// are ever encoded. The encoded projection that travels beside the transcript
/// is subtracted first, and the newest row is kept unconditionally: a row that
/// alone exceeds the budget must reach the transport, which answers with its
/// typed over-size failure instead of serving an empty transcript.
///
/// # Errors
///
/// Returns the typed local encode failure when a transcript row cannot be
/// encoded.
pub fn bounded_snapshot_messages(
    projection_bytes: usize,
    mut messages: Vec<intention_proto::MessageProjectionDto>,
) -> DtoResult<Vec<intention_proto::MessageProjectionDto>> {
    let mut budget = MAX_TRANSCRIPT_SNAPSHOT_BYTES.saturating_sub(projection_bytes);
    let newest = messages.len().saturating_sub(1);
    let mut keep_from = messages.len();
    for (index, message) in messages.iter().enumerate().rev() {
        let size = CountingWriter::encoded_bytes(|writer| serde_json::to_writer(writer, message))?
            .saturating_add(1);
        if size > budget && index != newest {
            break;
        }
        budget = budget.saturating_sub(size);
        keep_from = index;
    }
    messages.drain(..keep_from);
    Ok(messages)
}

/// The one typed failure of a local protocol value that cannot be encoded.
fn encode_failure() -> ErrorDto {
    ErrorDto::validation(
        "local_protocol_encode_failed",
        "a typed local protocol message could not be encoded",
    )
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

    use super::*;

    use intention_engine::{RunCancellation, SessionCommitDto};
    use intention_proto::{
        MessageId, MessageKindDto, MessageProjectionDto, NewMessageDto, SendUserTurnCommandDto,
        ToolResultStatusDto,
    };
    use intention_test_support::{fixture_session_command, fixture_snapshot};
    use tempfile::TempDir;

    fn test_facade() -> (TempDir, DaemonApplicationFacade) {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test_support(
            directory.path().join("facade.sqlite"),
            fixture_snapshot(),
        )
        .expect("durable facade opens");
        (directory, facade)
    }

    fn create(facade: &DaemonApplicationFacade, session_id: SessionId) {
        let accepted = facade
            .create_session(
                fixture_session_command(session_id, "composition"),
                &SilentCommitObserver::new(),
            )
            .expect("fixture session creates");
        assert!(matches!(accepted, ProtocolResultDto::SessionCreated(_)));
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

    /// Starts one durable run through a direct user turn and returns its identity.
    fn started_run(facade: &DaemonApplicationFacade, session_id: SessionId, label: &str) -> RunId {
        let accepted = send_user_turn(facade, session_id, label).expect("fixture turn is accepted");
        let ProtocolResultDto::TurnAccepted(turn) = accepted else {
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
    ) -> DtoResult<ProtocolResultDto> {
        facade.send_user_turn(
            SendUserTurnCommandDto::new(
                session_id,
                intention_proto::IdempotencyKey::new(),
                content,
            )
            .expect("fixture user turn is valid"),
            &RecordingPublisher::new(),
        )
    }

    /// Reads the committed transcript rows of one run from the same repository
    /// read the run subscription snapshot uses.
    fn run_messages(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        run_id: RunId,
    ) -> Vec<intention_proto::MessageProjectionDto> {
        facade
            .inner
            .repository
            .load_run_messages(session_id, run_id, SESSION_SNAPSHOT_MESSAGES)
            .expect("committed run transcript reads")
    }

    /// Reads the committed tool-result row of one exact invocation.
    fn tool_evidence(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        run_id: RunId,
        call_id: intention_proto::ToolCallId,
    ) -> intention_proto::MessageProjectionDto {
        run_messages(facade, session_id, run_id)
            .into_iter()
            .find(|message| {
                message.kind() == MessageKindDto::ToolResult
                    && message.tool_call_id() == Some(call_id)
            })
            .expect("the committed tool-result row exists")
    }

    #[test]
    fn facade_replay_of_the_same_user_turn_returns_the_recorded_outcome() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test_support(
            directory.path().join("idempotent-turn.sqlite"),
            fixture_snapshot(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);
        let key = intention_proto::IdempotencyKey::new();
        let command = SendUserTurnCommandDto::new(session_id, key, "idempotent turn")
            .expect("fixture user turn is valid");

        let initial = facade
            .send_user_turn(command.clone(), &RecordingPublisher::new())
            .expect("the first user turn is accepted");
        let ProtocolResultDto::TurnAccepted(initial_turn) = initial else {
            unreachable!("the first user turn returns user-turn evidence")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = initial_turn.outcome() else {
            unreachable!("the first user turn starts a run")
        };
        let committed = run_messages(&facade, session_id, run_id);

        // The proposed run identity is derived from the caller's idempotency
        // key, so a repeated command proposes an identity the durable turn
        // already recorded and fails as a typed conflict.
        let error = facade
            .send_user_turn(command, &RecordingPublisher::new())
            .expect_err("a repeated command proposes a bound run identity");
        assert_eq!(error.code(), "turn_identity_conflict");
        assert_eq!(
            run_messages(&facade, session_id, run_id),
            committed,
            "the repeated command duplicates no transcript row"
        );
    }

    #[test]
    fn every_created_session_joins_the_one_binding_of_its_workspace_root() {
        let (_directory, facade) = test_facade();
        let first = SessionId::new();
        create(&facade, first);
        let binding = facade
            .list_sessions(SESSION_LIST_ROWS)
            .expect("sessions list")
            .sessions()
            .first()
            .map(|summary| (summary.project_id(), summary.workspace_id()))
            .expect("the first session is listed");

        // Every later creation proposes a fresh project and workspace identity
        // for the same root, exactly as the terminal client does; the facade
        // resolves them to the durable binding of the root instead of rejecting
        // the second identity, so one root holds an unbounded number of
        // sessions.
        let second = SessionId::new();
        create(&facade, second);
        let third = SessionId::new();
        create(&facade, third);

        let listed = facade
            .list_sessions(SESSION_LIST_ROWS)
            .expect("sessions list");
        let created = listed
            .sessions()
            .iter()
            .map(|summary| summary.session_id())
            .collect::<Vec<_>>();
        assert_eq!(created.len(), 3, "every created session is listed");
        for expected in [first, second, third] {
            assert!(
                created.contains(&expected),
                "the created session {} is listed",
                expected
            );
        }
        assert!(
            listed
                .sessions()
                .iter()
                .all(|summary| (summary.project_id(), summary.workspace_id()) == binding),
            "one root keeps one durable project and workspace identity"
        );
    }

    #[test]
    fn an_over_budget_newest_row_is_kept_and_the_projection_is_charged() {
        let mut next_id = 0;
        let mut row = |text: String| {
            next_id += 1;
            MessageProjectionDto::new(
                MessageId::new(next_id).expect("fixture row identity is valid"),
                SessionId::new(),
                None,
                MessageKindDto::User,
                text,
                None,
                None,
            )
            .expect("fixture transcript row is valid")
        };
        // A single row larger than the whole budget is kept: the transport owns
        // the envelope cap and answers with its typed over-size failure instead
        // of the peer receiving a valid but empty transcript.
        let oversized = row("x".repeat(MAX_TRANSCRIPT_SNAPSHOT_BYTES));
        let kept = bounded_snapshot_messages(0, vec![oversized.clone()])
            .expect("the over-budget snapshot bounds");
        assert_eq!(kept, vec![oversized]);

        // The projection travelling beside the transcript is charged against
        // the same budget, so it displaces the oldest rows first.
        let rows: Vec<MessageProjectionDto> = (0..4)
            .map(|index| row(format!("row {index} {}", "y".repeat(200 * 1024))))
            .collect();
        let unbounded =
            bounded_snapshot_messages(0, rows.clone()).expect("the uncharged read bounds");
        let charged =
            bounded_snapshot_messages(400 * 1024, rows.clone()).expect("the charged read bounds");
        assert!(charged.len() < unbounded.len());
        assert_eq!(
            charged.last().map(|message| message.text().to_owned()),
            rows.last().map(|message| message.text().to_owned()),
            "the newest row is always kept"
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

        let source = ConfigPathDto::parse(path.to_string_lossy().into_owned())
            .expect("fixture config path is absolute");
        let (snapshot, selected_provider) =
            load_provider_configuration(&source).expect("valid provider config composes");
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
        let encoded = serde_json::to_string(&snapshot).expect("safe snapshot serializes");
        assert!(!encoded.contains("selected-provider-secret"));

        let source = ConfigPathDto::parse(invalid.to_string_lossy().into_owned())
            .expect("fixture config path is absolute");
        let error = load_provider_configuration(&source)
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
                    now().expect("fixture clock reads"),
                )
                .with_arguments_json(if cfg!(windows) {
                    r#"{"command":"echo started> sentinel.txt & ping -n 2 127.0.0.1"}"#
                } else {
                    r#"{"command":"printf x > sentinel.txt; sleep 2"}"#
                })
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
        let Some(intention_proto::ToolRowDto::Result(row)) = evidence.tool() else {
            unreachable!("the committed result row carries its result evidence")
        };
        assert_eq!(
            row.status(),
            ToolResultStatusDto::Partial,
            "the stopped execute commits its partial result"
        );
        assert_eq!(
            facade
                .session_snapshot(session_id)
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
                        now().expect("fixture clock reads"),
                    )
                    .with_arguments_json(r#"{"path":"hello.txt"}"#)
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
                .session_snapshot(session_id)
                .expect("the interrupted run stays readable")
                .projection()
                .active_run()
                .map(|run| run.run_id()),
            Some(run_id),
            "a late interrupt never rewrites completed evidence or terminalizes the run"
        );
        for call_id in call_ids {
            let evidence = tool_evidence(&facade, session_id, run_id, call_id);
            let Some(intention_proto::ToolRowDto::Result(row)) = evidence.tool() else {
                unreachable!("the committed result row carries its result evidence")
            };
            assert_eq!(row.status(), ToolResultStatusDto::Completed);
            assert_eq!(
                evidence
                    .tool_id()
                    .map(intention_proto::ToolIdentityDto::wire_name),
                Some("read")
            );
            assert_eq!(evidence.text(), "hello");
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
                    now().expect("fixture clock reads"),
                )
                .with_arguments_json(r#"{"path":"hello.txt"}"#),
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
        let message = NewMessageDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            text,
            None,
            None,
        )
        .expect("fixture transcript row is valid");
        facade
            .inner
            .repository
            .append_message(
                message,
                TimestampDto::from_unix_seconds(2).expect("fixture timestamp is valid"),
            )
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
            .session_snapshot(session_id)
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
            .session_snapshot(session_id)
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

    #[test]
    fn session_snapshots_carry_the_effective_provider_model() {
        // The snapshot read behind both the one-shot request and the
        // subscription-open reply announces the model the session's resolved
        // configuration executes under, so a window never renders a front-end
        // default next to a session that runs something else.
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let _ = started_run(&facade, session_id, "model announcement");

        let snapshot = facade
            .session_snapshot(session_id)
            .expect("the fixture snapshot reads");
        assert_eq!(snapshot.projection().provider_model(), Some("fixture"));
        assert_eq!(
            facade
                .session_snapshot(session_id)
                .expect("the fixture snapshot re-reads")
                .projection(),
            snapshot.projection(),
            "every snapshot consumer reads the same filled projection"
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

    impl SessionCommitObserver for RecordingPublisher {
        fn observe_session_commit(&self, committed: &SessionCommitDto) {
            if let SessionCommitDto::Content(message) = committed {
                self.publications
                    .lock()
                    .expect("publication lock is available")
                    .push(message.clone());
            }
        }
    }
}
