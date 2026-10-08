//! Versioned public local-protocol DTOs for Intention Relay.
//!
//! This module defines typed wire contracts only. It contains no socket
//! framing, client bootstrap, daemon lifecycle, runtime actors, or
//! presentation logic.

use crate::{
    ConfigRevisionId, CorrelationIdDto, DtoResult, ErrorDto, ProjectId, RunId, SchemaVersionDto,
    SessionId, TurnId, WorkspaceId,
};
use crate::{
    CreateSessionCommandDto, GetSessionSnapshotQueryDto, InterruptRunCommandDto,
    MessageProjectionDto, RemoveTurnCommandDto, RunModeDto, RunProjectionDto, RunStatusDto,
    SendUserTurnCommandDto, SessionProjectionDto,
};
use serde::{Deserialize, Deserializer, Serialize, de};

/// The protocol version negotiated before a client uses local transport.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolVersionDto {
    major: u16,
    minor: u16,
}

impl ProtocolVersionDto {
    /// Creates an explicit local protocol version.
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Returns the incompatible-on-change protocol component.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    /// Returns the additive-compatible protocol component.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }
}

/// The currently activated local protocol version.
///
/// The wire speaks JSON-RPC 2.0 over NDJSON under the single-version policy:
/// hello accepts only the exact current version, previously required fields
/// stay required, and decoding tolerates unknown additive fields. There is no
/// capability negotiation; a differing version is answered with a typed
/// JSON-RPC mismatch error before the daemon closes the connection.
pub const CURRENT_PROTOCOL_VERSION: ProtocolVersionDto = ProtocolVersionDto::new(1, 0);
/// The currently activated public DTO schema version.
pub const CURRENT_DTO_SCHEMA_VERSION: SchemaVersionDto = SchemaVersionDto::new(1, 1);

/// Decodes one payload `schema_version` that must equal the current version.
///
/// The public DTO schema compares by exact equality, with no same-major
/// tolerance, so a received payload that declares any other version fails at
/// the decode boundary with the typed `incompatible_dto_schema_version` error.
fn current_schema_version<'de, D>(deserializer: D) -> Result<SchemaVersionDto, D::Error>
where
    D: Deserializer<'de>,
{
    let version = SchemaVersionDto::deserialize(deserializer)?;
    if version == CURRENT_DTO_SCHEMA_VERSION {
        Ok(version)
    } else {
        Err(de::Error::custom(ErrorDto::validation(
            "incompatible_dto_schema_version",
            "the DTO schema version must equal the current version",
        )))
    }
}

pub use crate::jsonrpc::{
    JSONRPC_INVALID_PARAMS, JSONRPC_INVALID_REQUEST, JSONRPC_METHOD_NOT_FOUND, JSONRPC_PARSE_ERROR,
    JSONRPC_VERSION, JSONRPC_VERSION_MISMATCH, JsonRpcErrorDto, JsonRpcNotificationDto,
    JsonRpcRequestDto, JsonRpcRequestFailure, JsonRpcResponseDto, is_notification_line,
};

/// A safe metadata handshake exchanged before any protocol command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProtocolHelloDto {
    version: ProtocolVersionDto,
    adapter_name: String,
}

impl<'de> Deserialize<'de> for ProtocolHelloDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProtocolHelloDto {
            version: ProtocolVersionDto,
            adapter_name: String,
        }

        let raw = RawProtocolHelloDto::deserialize(deserializer)?;
        Self::new(raw.version, raw.adapter_name).map_err(de::Error::custom)
    }
}

impl ProtocolHelloDto {
    /// Creates a handshake with a non-empty adapter metadata name.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the adapter name is blank.
    pub fn new(version: ProtocolVersionDto, adapter_name: impl Into<String>) -> DtoResult<Self> {
        let adapter_name = adapter_name.into();
        if adapter_name.trim().is_empty() {
            Err(ErrorDto::validation(
                "invalid_adapter_name",
                "adapter name must not be empty",
            ))
        } else {
            Ok(Self {
                version,
                adapter_name,
            })
        }
    }

    /// Returns the peer protocol version.
    #[must_use]
    pub const fn version(&self) -> ProtocolVersionDto {
        self.version
    }

    /// Returns the safe local adapter metadata name.
    #[must_use]
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }
}

/// The daemon's current readiness for requests after protocol negotiation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonReadinessDto {
    /// The daemon has started but cannot yet serve session requests.
    Starting,
    /// The daemon can serve requests for its supported protocol version.
    Ready,
    /// The daemon is stopping and should not accept new work.
    Draining,
    /// The daemon cannot currently serve requests.
    Unavailable,
}

/// A versioned, credential-free health projection from the daemon authority.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonHealthDto {
    #[serde(deserialize_with = "current_schema_version")]
    schema_version: SchemaVersionDto,
    protocol_version: ProtocolVersionDto,
    readiness: DaemonReadinessDto,
}

impl DaemonHealthDto {
    /// Creates a typed daemon health and readiness projection.
    #[must_use]
    pub const fn new(
        schema_version: SchemaVersionDto,
        protocol_version: ProtocolVersionDto,
        readiness: DaemonReadinessDto,
    ) -> Self {
        Self {
            schema_version,
            protocol_version,
            readiness,
        }
    }

    /// Returns the projection schema version.
    #[must_use]
    pub const fn schema_version(self) -> SchemaVersionDto {
        self.schema_version
    }

    /// Returns the current daemon readiness state.
    #[must_use]
    pub const fn readiness(self) -> DaemonReadinessDto {
        self.readiness
    }
}

/// A subscription request scoped to one durable session.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubscribeSessionCommandDto {
    #[serde(deserialize_with = "current_schema_version")]
    schema_version: SchemaVersionDto,
    session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run_id: Option<RunId>,
    requested_mode: RunModeDto,
}

impl SubscribeSessionCommandDto {
    /// Creates a typed session-wide subscription request.
    ///
    /// Use [`Self::with_run_id`] to scope a subscription to a particular run.
    #[must_use]
    pub const fn new(
        schema_version: SchemaVersionDto,
        session_id: SessionId,
        requested_mode: RunModeDto,
    ) -> Self {
        Self {
            schema_version,
            session_id,
            run_id: None,
            requested_mode,
        }
    }

    /// Creates a typed subscription request with an optional run scope.
    #[must_use]
    pub const fn with_run_id(
        schema_version: SchemaVersionDto,
        session_id: SessionId,
        run_id: Option<RunId>,
        requested_mode: RunModeDto,
    ) -> Self {
        Self {
            schema_version,
            session_id,
            run_id,
            requested_mode,
        }
    }

    /// Returns the request schema version.
    #[must_use]
    pub const fn schema_version(self) -> SchemaVersionDto {
        self.schema_version
    }

    /// Returns the subscribed session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }

    /// Returns the optional run scope requested by the adapter.
    #[must_use]
    pub const fn run_id(self) -> Option<RunId> {
        self.run_id
    }

    /// Returns the requesting adapter's current mode projection.
    #[must_use]
    pub const fn requested_mode(self) -> RunModeDto {
        self.requested_mode
    }
}

/// A dedicated run-stream subscription request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubscribeRunCommandDto {
    #[serde(deserialize_with = "current_schema_version")]
    schema_version: SchemaVersionDto,
    session_id: SessionId,
    run_id: RunId,
}

impl SubscribeRunCommandDto {
    /// Creates a run-scoped subscription request.
    #[must_use]
    pub const fn new(
        schema_version: SchemaVersionDto,
        session_id: SessionId,
        run_id: RunId,
    ) -> Self {
        Self {
            schema_version,
            session_id,
            run_id,
        }
    }

    /// Returns the request schema version.
    #[must_use]
    pub const fn schema_version(self) -> SchemaVersionDto {
        self.schema_version
    }

    /// Returns the scoped session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }

    /// Returns the scoped run identity.
    #[must_use]
    pub const fn run_id(self) -> RunId {
        self.run_id
    }
}

/// A committed run-status change published as an uncorrelated run-stream frame.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunStatusFrameDto {
    session_id: SessionId,
    run_id: RunId,
    status: RunStatusDto,
}

impl RunStatusFrameDto {
    /// Creates a committed status frame for one run.
    #[must_use]
    pub const fn new(session_id: SessionId, run_id: RunId, status: RunStatusDto) -> Self {
        Self {
            session_id,
            run_id,
            status,
        }
    }

    /// Returns the scoped session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }

    /// Returns the scoped run identity.
    #[must_use]
    pub const fn run_id(self) -> RunId {
        self.run_id
    }

    /// Returns the committed run lifecycle status.
    #[must_use]
    pub const fn status(self) -> RunStatusDto {
        self.status
    }
}

/// A server-originated, uncorrelated run-stream frame.
///
/// The wire tag is `kind` with `content` for one committed transcript row and
/// `status` for one committed run-status change; the payload travels in `data`
/// and carries no event position.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum RunStreamFrameDto {
    /// One committed transcript row.
    Content(MessageProjectionDto),
    /// One committed run-status change.
    Status(RunStatusFrameDto),
}

/// The compact current run state returned by a dedicated run subscription.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunSubscriptionSnapshotDto {
    run: RunProjectionDto,
    messages: Vec<MessageProjectionDto>,
}

impl<'de> Deserialize<'de> for RunSubscriptionSnapshotDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawRunSubscriptionSnapshotDto {
            run: RunProjectionDto,
            messages: Vec<MessageProjectionDto>,
        }
        let raw = RawRunSubscriptionSnapshotDto::deserialize(deserializer)?;
        Self::new(raw.run, raw.messages).map_err(de::Error::custom)
    }
}

impl RunSubscriptionSnapshotDto {
    /// Creates a coherent run snapshot scoped to one session and run.
    ///
    /// # Errors
    ///
    /// Returns a validation error when a carried transcript row belongs to a
    /// different session or to a different run.
    pub fn new(run: RunProjectionDto, messages: Vec<MessageProjectionDto>) -> DtoResult<Self> {
        if messages.iter().any(|message| {
            message.session_id() != run.session_id()
                || message
                    .run_id()
                    .is_some_and(|run_id| run_id != run.run_id())
        }) {
            return Err(ErrorDto::validation(
                "invalid_run_subscription_snapshot",
                "run snapshot messages must belong to the snapshot session and run",
            ));
        }
        Ok(Self { run, messages })
    }

    /// Returns the current run projection.
    #[must_use]
    pub const fn run(&self) -> &RunProjectionDto {
        &self.run
    }

    /// Returns the bounded committed transcript rows of the run.
    #[must_use]
    pub fn messages(&self) -> &[MessageProjectionDto] {
        &self.messages
    }
}

/// The correlated first reply to a dedicated run subscription request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum RunSubscriptionResponseDto {
    /// The current run state; later committed state arrives as live frames.
    Snapshot(RunSubscriptionSnapshotDto),
    /// A safe scoped request error.
    Error(ErrorDto),
}

impl RunSubscriptionResponseDto {
    /// Creates a current run-state snapshot response.
    #[must_use]
    pub const fn snapshot(snapshot: RunSubscriptionSnapshotDto) -> Self {
        Self::Snapshot(snapshot)
    }

    /// Creates a typed run-subscription error response.
    #[must_use]
    pub const fn error(error: ErrorDto) -> Self {
        Self::Error(error)
    }
}

/// A typed protocol command wrapper with no transport-specific resources.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProtocolCommandDto {
    /// Requests durable creation of a session.
    CreateSession(CreateSessionCommandDto),
    /// Sends an accepted user turn to the daemon authority.
    SendUserTurn(SendUserTurnCommandDto),
    /// Requests removal of a not-yet-seen pending user turn.
    RemoveTurn(RemoveTurnCommandDto),
    /// Requests interruption of an active daemon-owned run's current operation.
    InterruptRun(InterruptRunCommandDto),
    /// Begins a session subscription answered by the current session state.
    SubscribeSession(SubscribeSessionCommandDto),
}

/// A typed protocol query wrapper with no transport-specific resources.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProtocolQueryDto {
    /// Obtains the daemon's latest health and readiness projection.
    GetDaemonHealth,
    /// Obtains the latest durable session projection.
    GetSessionSnapshot(GetSessionSnapshotQueryDto),
}

/// A typed command result independent of a transport codec.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
pub enum ProtocolCommandResultDto {
    /// The daemon accepted the command and will publish resulting state separately.
    Accepted(ProtocolAcceptedDto),
    /// The command was safely rejected before execution.
    Rejected(ErrorDto),
}

/// The immutable correlation data returned for an accepted command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProtocolAcceptedDto {
    correlation_id: CorrelationIdDto,
    result: ProtocolAcceptedResultDto,
}

impl<'de> Deserialize<'de> for ProtocolAcceptedDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProtocolAcceptedDto {
            correlation_id: CorrelationIdDto,
            result: ProtocolAcceptedResultDto,
        }
        let raw = RawProtocolAcceptedDto::deserialize(deserializer)?;
        Ok(Self {
            correlation_id: raw.correlation_id,
            result: raw.result,
        })
    }
}

impl ProtocolAcceptedDto {
    /// Creates an acceptance with one operation-specific typed result.
    #[must_use]
    pub const fn with_result(
        correlation_id: CorrelationIdDto,
        result: ProtocolAcceptedResultDto,
    ) -> Self {
        Self {
            correlation_id,
            result,
        }
    }

    /// Returns the opaque canonical correlation reference.
    #[must_use]
    pub const fn correlation_id(&self) -> CorrelationIdDto {
        self.correlation_id
    }

    /// Returns operation-specific acceptance evidence.
    ///
    /// `result` is a required field of the current DTO shape, so the accessor
    /// returns a direct reference; there is no absent-result state to model.
    #[must_use]
    pub const fn result(&self) -> &ProtocolAcceptedResultDto {
        &self.result
    }
}

/// Typed acceptance evidence for a state-changing protocol operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProtocolAcceptedResultDto {
    /// A durable session was created.
    CreateSession(CreateSessionAcceptedDto),
    /// A user turn was accepted and either started a run or became a pending message.
    SendUserTurn(SendUserTurnAcceptedDto),
    /// A pending user turn was removed.
    RemoveTurn(RemoveTurnAcceptedDto),
    /// An interruption request was accepted for a run.
    InterruptRun(InterruptRunAcceptedDto),
}

/// Typed acceptance evidence for a created session.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateSessionAcceptedDto {
    project_id: ProjectId,
    workspace_id: WorkspaceId,
    session_id: SessionId,
}
impl CreateSessionAcceptedDto {
    /// Creates durable session acceptance evidence.
    #[must_use]
    pub const fn new(
        project_id: ProjectId,
        workspace_id: WorkspaceId,
        session_id: SessionId,
    ) -> Self {
        Self {
            project_id,
            workspace_id,
            session_id,
        }
    }
    /// Returns the owning project identity.
    #[must_use]
    pub const fn project_id(self) -> ProjectId {
        self.project_id
    }
    /// Returns the daemon-owned workspace identity.
    #[must_use]
    pub const fn workspace_id(self) -> WorkspaceId {
        self.workspace_id
    }
    /// Returns the durable created session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }
}

/// The durable disposition of an accepted M3 user turn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SendUserTurnOutcomeDto {
    /// The accepted turn started a run with its immutable configuration revision.
    Started {
        run_id: RunId,
        config_revision_id: ConfigRevisionId,
    },
    /// The accepted turn is a pending message that joins the active run context.
    Pending,
}

/// Typed acceptance evidence for one user turn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SendUserTurnAcceptedDto {
    session_id: SessionId,
    turn_id: TurnId,
    outcome: SendUserTurnOutcomeDto,
}
impl SendUserTurnAcceptedDto {
    /// Creates complete durable turn-acceptance evidence.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        turn_id: TurnId,
        outcome: SendUserTurnOutcomeDto,
    ) -> Self {
        Self {
            session_id,
            turn_id,
            outcome,
        }
    }
    /// Returns the owning session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }
    /// Returns the accepted turn identity.
    #[must_use]
    pub const fn turn_id(self) -> TurnId {
        self.turn_id
    }
    /// Returns whether the turn started a run or became a pending message.
    #[must_use]
    pub const fn outcome(self) -> SendUserTurnOutcomeDto {
        self.outcome
    }
}

/// Typed acceptance evidence for a removed pending turn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoveTurnAcceptedDto {
    session_id: SessionId,
    turn_id: TurnId,
}
impl RemoveTurnAcceptedDto {
    /// Creates complete pending-turn removal acceptance evidence.
    #[must_use]
    pub const fn new(session_id: SessionId, turn_id: TurnId) -> Self {
        Self {
            session_id,
            turn_id,
        }
    }
    /// Returns the owning session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }
    /// Returns the removed turn identity.
    #[must_use]
    pub const fn turn_id(self) -> TurnId {
        self.turn_id
    }
}

/// Typed acceptance evidence for a run interruption request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InterruptRunAcceptedDto {
    session_id: SessionId,
    run_id: RunId,
}
impl InterruptRunAcceptedDto {
    /// Creates complete interruption acceptance evidence.
    #[must_use]
    pub const fn new(session_id: SessionId, run_id: RunId) -> Self {
        Self { session_id, run_id }
    }
    /// Returns the owning session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }
    /// Returns the interrupted run identity.
    #[must_use]
    pub const fn run_id(self) -> RunId {
        self.run_id
    }
}

/// A versioned current session projection with its committed transcript rows.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionSnapshotDto {
    schema_version: SchemaVersionDto,
    session_id: SessionId,
    projection: SessionProjectionDto,
    messages: Vec<MessageProjectionDto>,
}

impl<'de> Deserialize<'de> for SessionSnapshotDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawSessionSnapshotDto {
            #[serde(deserialize_with = "current_schema_version")]
            schema_version: SchemaVersionDto,
            session_id: SessionId,
            projection: SessionProjectionDto,
            messages: Vec<MessageProjectionDto>,
        }
        let raw = RawSessionSnapshotDto::deserialize(deserializer)?;
        Self::with_projection(
            raw.schema_version,
            raw.session_id,
            raw.projection,
            raw.messages,
        )
        .map_err(de::Error::custom)
    }
}

impl SessionSnapshotDto {
    /// Creates a session snapshot containing a coherent state projection.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the projection session identity differs
    /// from this snapshot session.
    pub fn with_projection(
        schema_version: SchemaVersionDto,
        session_id: SessionId,
        projection: SessionProjectionDto,
        messages: Vec<MessageProjectionDto>,
    ) -> DtoResult<Self> {
        if projection.session_id() != session_id {
            return Err(ErrorDto::validation(
                "invalid_session_snapshot_projection",
                "snapshot projection must share the snapshot session",
            ));
        }
        Ok(Self {
            schema_version,
            session_id,
            projection,
            messages,
        })
    }

    /// Returns the snapshot schema version.
    #[must_use]
    pub const fn schema_version(&self) -> SchemaVersionDto {
        self.schema_version
    }
    /// Returns the durable session identity represented by the snapshot.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }
    /// Returns the public state projection carried by the snapshot.
    ///
    /// `projection` is a required field of the current DTO shape, so the
    /// accessor returns a direct reference; there is no absent-projection
    /// state to model.
    #[must_use]
    pub const fn projection(&self) -> &SessionProjectionDto {
        &self.projection
    }
    /// Returns the bounded committed transcript rows of the session.
    #[must_use]
    pub fn messages(&self) -> &[MessageProjectionDto] {
        &self.messages
    }
}

/// A subscription response containing either a current snapshot or a safe error.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum SessionSubscriptionResponseDto {
    /// A consistent current session snapshot.
    Snapshot(SessionSnapshotDto),
    /// A safe scoped subscription error.
    Error(ErrorDto),
}

impl SessionSubscriptionResponseDto {
    /// Creates a consistent session snapshot response.
    #[must_use]
    pub const fn snapshot(snapshot: SessionSnapshotDto) -> Self {
        Self::Snapshot(snapshot)
    }

    /// Creates a typed session-subscription error response.
    #[must_use]
    pub const fn error(error: ErrorDto) -> Self {
        Self::Error(error)
    }
}

/// A typed query result independent of a transport codec.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
pub enum ProtocolQueryResultDto {
    /// A current daemon health projection.
    DaemonHealth(DaemonHealthDto),
    /// A current session checkpoint.
    SessionSnapshot(SessionSnapshotDto),
    /// The query was safely rejected before execution.
    Rejected(ErrorDto),
}

/// A typed request payload carried by one JSON-RPC request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProtocolRequestPayloadDto {
    /// A daemon command.
    Command(ProtocolCommandDto),
    /// A daemon query.
    Query(ProtocolQueryDto),
    /// A dedicated run-stream subscription request.
    RunSubscription(SubscribeRunCommandDto),
}

/// A typed response payload carried by one JSON-RPC response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProtocolResponsePayloadDto {
    /// A response to a daemon command.
    CommandResult(ProtocolCommandResultDto),
    /// A response to a daemon query.
    QueryResult(ProtocolQueryResultDto),
    /// A response to a session subscription request.
    Subscription(SessionSubscriptionResponseDto),
    /// A correlated first reply to a run-stream subscription request.
    RunSubscription(RunSubscriptionResponseDto),
}

/// The method name of the mandatory version handshake.
pub const PROTOCOL_HELLO_METHOD: &str = "hello";
/// The notification method carrying one run-stream frame.
pub const RUN_FRAME_METHOD: &str = "run.frame";

/// One implemented JSON-RPC method of the local protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolMethodDto {
    /// Durable session creation.
    SessionCreate,
    /// User-turn delivery to the daemon authority.
    TurnSend,
    /// Removal of a not-yet-seen pending user turn.
    TurnRemove,
    /// Interruption of an active daemon-owned run's current operation.
    RunInterrupt,
    /// One-shot current-state session snapshot subscription.
    SessionSubscribe,
    /// Dedicated run-stream subscription.
    RunSubscribe,
    /// Daemon health and readiness projection.
    DaemonHealth,
    /// Durable session snapshot projection.
    SessionSnapshot,
}

impl ProtocolMethodDto {
    /// Every implemented method.
    ///
    /// The wire-name table has exactly one source, [`Self::as_str`], and the
    /// one-to-one coverage of this list is pinned by a wildcard-free
    /// conformance test, so a method name and a variant can never drift.
    const ALL: [Self; 8] = [
        Self::SessionCreate,
        Self::TurnSend,
        Self::TurnRemove,
        Self::RunInterrupt,
        Self::SessionSubscribe,
        Self::RunSubscribe,
        Self::DaemonHealth,
        Self::SessionSnapshot,
    ];

    /// Returns the wire method name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionCreate => "session.create",
            Self::TurnSend => "turn.send",
            Self::TurnRemove => "turn.remove",
            Self::RunInterrupt => "run.interrupt",
            Self::SessionSubscribe => "session.subscribe",
            Self::RunSubscribe => "run.subscribe",
            Self::DaemonHealth => "daemon.health",
            Self::SessionSnapshot => "session.snapshot",
        }
    }

    /// Returns the unique method that carries a typed request payload.
    #[must_use]
    pub const fn for_payload(payload: &ProtocolRequestPayloadDto) -> Self {
        match payload {
            ProtocolRequestPayloadDto::Command(command) => match command {
                ProtocolCommandDto::CreateSession(_) => Self::SessionCreate,
                ProtocolCommandDto::SendUserTurn(_) => Self::TurnSend,
                ProtocolCommandDto::RemoveTurn(_) => Self::TurnRemove,
                ProtocolCommandDto::InterruptRun(_) => Self::RunInterrupt,
                ProtocolCommandDto::SubscribeSession(_) => Self::SessionSubscribe,
            },
            ProtocolRequestPayloadDto::Query(query) => match query {
                ProtocolQueryDto::GetDaemonHealth => Self::DaemonHealth,
                ProtocolQueryDto::GetSessionSnapshot(_) => Self::SessionSnapshot,
            },
            ProtocolRequestPayloadDto::RunSubscription(_) => Self::RunSubscribe,
        }
    }

    /// Reports whether this method carries the given request payload.
    #[must_use]
    pub const fn accepts_request(self, payload: &ProtocolRequestPayloadDto) -> bool {
        matches!(
            (self, payload),
            (
                Self::SessionCreate,
                ProtocolRequestPayloadDto::Command(ProtocolCommandDto::CreateSession(_))
            ) | (
                Self::TurnSend,
                ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SendUserTurn(_))
            ) | (
                Self::TurnRemove,
                ProtocolRequestPayloadDto::Command(ProtocolCommandDto::RemoveTurn(_))
            ) | (
                Self::RunInterrupt,
                ProtocolRequestPayloadDto::Command(ProtocolCommandDto::InterruptRun(_))
            ) | (
                Self::SessionSubscribe,
                ProtocolRequestPayloadDto::Command(ProtocolCommandDto::SubscribeSession(_))
            ) | (
                Self::RunSubscribe,
                ProtocolRequestPayloadDto::RunSubscription(_)
            ) | (
                Self::DaemonHealth,
                ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth)
            ) | (
                Self::SessionSnapshot,
                ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetSessionSnapshot(_))
            )
        )
    }

    /// Reports whether this method carries the given response payload.
    #[must_use]
    pub const fn accepts_response(self, payload: &ProtocolResponsePayloadDto) -> bool {
        match self {
            Self::SessionCreate | Self::TurnSend | Self::TurnRemove | Self::RunInterrupt => {
                matches!(payload, ProtocolResponsePayloadDto::CommandResult(_))
            }
            Self::DaemonHealth | Self::SessionSnapshot => {
                matches!(payload, ProtocolResponsePayloadDto::QueryResult(_))
            }
            Self::SessionSubscribe => {
                matches!(payload, ProtocolResponsePayloadDto::Subscription(_))
            }
            Self::RunSubscribe => matches!(payload, ProtocolResponsePayloadDto::RunSubscription(_)),
        }
    }

    /// Returns the payload a parameterless request of this method carries.
    ///
    /// Only `daemon.health` is genuinely parameterless; every other
    /// method requires its typed payload, so an absent `params` member stays an
    /// invalid-params failure.
    const fn empty_payload(self) -> Option<ProtocolRequestPayloadDto> {
        match self {
            Self::DaemonHealth => Some(ProtocolRequestPayloadDto::Query(
                ProtocolQueryDto::GetDaemonHealth,
            )),
            Self::SessionCreate
            | Self::TurnSend
            | Self::TurnRemove
            | Self::RunInterrupt
            | Self::SessionSubscribe
            | Self::RunSubscribe
            | Self::SessionSnapshot => None,
        }
    }

    fn parse_with_id(method: &str, id: u64) -> Result<Self, JsonRpcRequestFailure> {
        Self::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.as_str() == method)
            .ok_or_else(|| {
                JsonRpcRequestFailure::new(Some(id), JsonRpcErrorDto::method_not_found(method))
            })
    }
}

/// A decoded client request with its JSON-RPC identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolRequestDto {
    id: u64,
    payload: ProtocolRequestPayloadDto,
}

impl ProtocolRequestDto {
    /// Creates a decoded request from its wire identity and typed payload.
    #[must_use]
    pub const fn new(id: u64, payload: ProtocolRequestPayloadDto) -> Self {
        Self { id, payload }
    }

    /// Returns the request identity that responses must echo.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Returns the typed request payload.
    #[must_use]
    pub const fn payload(&self) -> &ProtocolRequestPayloadDto {
        &self.payload
    }
}

/// A daemon-to-client message: either a response or a stream notification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolDaemonMessageDto {
    /// A response to one client request.
    Response(JsonRpcResponseDto<ProtocolResponsePayloadDto>),
    /// A `run.frame` notification carrying one run-stream frame.
    Notification(JsonRpcNotificationDto<RunStreamFrameDto>),
}

impl ProtocolDaemonMessageDto {
    /// Creates the notification carrying one run-stream frame.
    #[must_use]
    pub fn run_frame(frame: RunStreamFrameDto) -> Self {
        Self::Notification(JsonRpcNotificationDto::new(RUN_FRAME_METHOD, frame))
    }
}

impl Serialize for ProtocolDaemonMessageDto {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Response(response) => response.serialize(serializer),
            Self::Notification(notification) => notification.serialize(serializer),
        }
    }
}

/// Encodes one typed request payload into a JSON-RPC request envelope.
#[must_use]
pub fn encode_request(
    id: u64,
    payload: ProtocolRequestPayloadDto,
) -> JsonRpcRequestDto<ProtocolRequestPayloadDto> {
    let method = ProtocolMethodDto::for_payload(&payload);
    JsonRpcRequestDto::new(id, method.as_str(), payload)
}

/// Parses one wire line into a decoded client request.
///
/// A line whose request envelope omits the `id` member is a notification, not a
/// request: [`crate::jsonrpc::is_notification_line`] classifies it, and a server must
/// not answer it.
///
/// # Errors
///
/// Returns a parse error for invalid JSON, an invalid-request error for a
/// non-conformant envelope, a method-not-found error for an unknown method,
/// and an invalid-params error when the payload or its method pairing fails.
pub fn decode_request_line(line: &str) -> Result<ProtocolRequestDto, JsonRpcRequestFailure> {
    let header = crate::jsonrpc::JsonRpcRequestHeader::parse(line)?;
    let method = ProtocolMethodDto::parse_with_id(header.method(), header.id())?;
    let request: JsonRpcRequestDto<ProtocolRequestPayloadDto> = match method.empty_payload() {
        Some(empty) => JsonRpcRequestDto::parse_allowing_absent_params(line, empty)?,
        None => JsonRpcRequestDto::parse(line)?,
    };
    if !method.accepts_request(request.params()) {
        return Err(JsonRpcRequestFailure::new(
            Some(request.id()),
            JsonRpcErrorDto::invalid_params(),
        ));
    }
    Ok(ProtocolRequestDto::new(request.id(), request.into_params()))
}

/// Decodes the response to one known request from a wire line.
///
/// # Errors
///
/// Returns the typed daemon error carried by an error response, or an
/// invalid-response error when the identity or payload family differs.
pub fn decode_response(
    line: &str,
    method: ProtocolMethodDto,
    id: u64,
) -> DtoResult<ProtocolResponsePayloadDto> {
    let response: JsonRpcResponseDto<ProtocolResponsePayloadDto> =
        JsonRpcResponseDto::parse(line).map_err(|_| invalid_wire_response())?;
    if response.id() != Some(id) {
        return Err(invalid_wire_response());
    }
    if let Some(error) = response.error_value() {
        return Err(error.to_error());
    }
    let payload = response.into_result().ok_or_else(invalid_wire_response)?;
    if !method.accepts_response(&payload) {
        return Err(invalid_wire_response());
    }
    Ok(payload)
}

/// Encodes one typed response payload into a JSON-RPC response envelope.
#[must_use]
pub fn encode_response(
    id: u64,
    payload: ProtocolResponsePayloadDto,
) -> JsonRpcResponseDto<ProtocolResponsePayloadDto> {
    JsonRpcResponseDto::result(id, payload)
}

/// Parses one `run.frame` notification line.
///
/// # Errors
///
/// Returns an invalid-response error when the line is not a `run.frame`
/// notification.
pub fn parse_run_frame_notification(line: &str) -> DtoResult<RunStreamFrameDto> {
    let notification: JsonRpcNotificationDto<RunStreamFrameDto> =
        JsonRpcNotificationDto::parse(line).map_err(|_| invalid_wire_response())?;
    if notification.method() != RUN_FRAME_METHOD {
        return Err(invalid_wire_response());
    }
    Ok(notification.into_params())
}

fn invalid_wire_response() -> ErrorDto {
    ErrorDto::validation(
        "invalid_local_protocol_response",
        "the local daemon returned an unexpected protocol response",
    )
}

/// Encodes the mandatory client hello request.
#[must_use]
pub fn encode_hello_request(
    id: u64,
    hello: ProtocolHelloDto,
) -> JsonRpcRequestDto<ProtocolHelloDto> {
    JsonRpcRequestDto::new(id, PROTOCOL_HELLO_METHOD, hello)
}

/// Validates one hello request and returns the peer handshake.
///
/// # Errors
///
/// Returns an invalid-request error when the first request is not `hello`,
/// and a protocol-version mismatch error when the peer version differs.
pub fn decode_hello_request(
    request: &JsonRpcRequestDto<ProtocolHelloDto>,
) -> Result<ProtocolHelloDto, JsonRpcErrorDto> {
    if request.method() != PROTOCOL_HELLO_METHOD {
        return Err(JsonRpcErrorDto::new(
            JSONRPC_INVALID_REQUEST,
            "the first request on a connection must be hello",
            Some(ErrorDto::validation(
                "jsonrpc_hello_required",
                "the first request on a connection must be hello",
            )),
        ));
    }
    let hello = request.params();
    if hello.version() != CURRENT_PROTOCOL_VERSION {
        return Err(JsonRpcErrorDto::from_error(
            JSONRPC_VERSION_MISMATCH,
            ErrorDto::unavailable(
                "incompatible_protocol_version",
                "protocol version must equal the current version",
            ),
        ));
    }
    Ok(hello.clone())
}

/// Encodes one hello response carrying the daemon handshake.
#[must_use]
pub fn encode_hello_response(
    id: u64,
    hello: ProtocolHelloDto,
) -> JsonRpcResponseDto<ProtocolHelloDto> {
    JsonRpcResponseDto::result(id, hello)
}

/// Decodes one hello response into the daemon handshake.
///
/// # Errors
///
/// Returns the typed daemon error carried by an error response, and a
/// protocol-version mismatch error when the daemon version differs.
pub fn decode_hello_response(
    response: &JsonRpcResponseDto<ProtocolHelloDto>,
) -> DtoResult<ProtocolHelloDto> {
    if let Some(error) = response.error_value() {
        return Err(error.to_error());
    }
    let hello = response
        .result_value()
        .cloned()
        .ok_or_else(invalid_wire_response)?;
    if hello.version() != CURRENT_PROTOCOL_VERSION {
        return Err(ErrorDto::unavailable(
            "incompatible_protocol_version",
            "protocol version must equal the current version",
        ));
    }
    Ok(hello)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;
    use crate::MessageKindDto;

    fn fixture_workspace_root() -> crate::WorkspaceRootDto {
        crate::WorkspaceRootDto::parse(
            std::env::temp_dir()
                .join("intention-proto-unit-workspace")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture workspace root is valid")
    }

    fn fixture_projection(session_id: SessionId) -> SessionProjectionDto {
        SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            fixture_workspace_root(),
            RunModeDto::Build,
            None,
            None,
            Vec::new(),
        )
        .expect("fixture projection is valid")
    }

    fn fixture_message(session_id: SessionId, run_id: RunId) -> MessageProjectionDto {
        MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::Notice,
            "fixture notice",
            None,
            None,
            None,
        )
        .expect("fixture message is valid")
    }

    fn fixture_run(session_id: SessionId, run_id: RunId) -> RunProjectionDto {
        RunProjectionDto::new(
            session_id,
            run_id,
            TurnId::new(),
            RunStatusDto::Running,
            ConfigRevisionId::new(),
        )
    }

    #[test]
    fn protocol_versions_and_hello_validate_all_paths() {
        let version = ProtocolVersionDto::new(1, 0);
        assert_eq!(version.major(), 1);
        assert_eq!(version.minor(), 0);
        assert_eq!(version, crate::CURRENT_PROTOCOL_VERSION);
        assert_ne!(
            ProtocolVersionDto::new(1, 1),
            crate::CURRENT_PROTOCOL_VERSION
        );
        let hello = ProtocolHelloDto::new(version, "fixture").expect("fixture hello is valid");
        assert_eq!(hello.version(), version);
        assert_eq!(hello.adapter_name(), "fixture");
        assert_eq!(
            ProtocolHelloDto::new(version, " ")
                .expect_err("blank adapter must fail")
                .code(),
            "invalid_adapter_name"
        );
    }

    #[test]
    fn current_versions_are_protocol_1_0_and_dto_schema_1_1() {
        assert_eq!(CURRENT_PROTOCOL_VERSION, ProtocolVersionDto::new(1, 0));
        assert_eq!(CURRENT_DTO_SCHEMA_VERSION, SchemaVersionDto::new(1, 1));
    }

    #[test]
    fn subscription_responses_round_trip_current_state_snapshots() {
        let schema = SchemaVersionDto::new(1, 1);
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let snapshot = SessionSnapshotDto::with_projection(
            schema,
            session_id,
            fixture_projection(session_id),
            vec![fixture_message(session_id, run_id)],
        )
        .expect("fixture snapshot is valid");
        assert_eq!(snapshot.messages().len(), 1);
        let response = SessionSubscriptionResponseDto::snapshot(snapshot);
        assert_eq!(
            serde_json::from_str::<SessionSubscriptionResponseDto>(
                &serde_json::to_string(&response).expect("response serializes")
            )
            .expect("response deserializes"),
            response
        );

        let run_snapshot = RunSubscriptionSnapshotDto::new(
            fixture_run(session_id, run_id),
            vec![fixture_message(session_id, run_id)],
        )
        .expect("fixture run snapshot is valid");
        let response = RunSubscriptionResponseDto::snapshot(run_snapshot);
        assert_eq!(
            serde_json::from_str::<RunSubscriptionResponseDto>(
                &serde_json::to_string(&response).expect("run response serializes")
            )
            .expect("run response deserializes"),
            response
        );
    }

    #[test]
    fn wire_helpers_round_trip_responses_notifications_and_hello() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let frame = RunStreamFrameDto::Status(RunStatusFrameDto::new(
            session_id,
            run_id,
            RunStatusDto::Completed,
        ));
        let notification = ProtocolDaemonMessageDto::run_frame(frame.clone());
        let line = serde_json::to_string(&notification).expect("notification serializes");
        assert_eq!(
            parse_run_frame_notification(&line).expect("frame parses"),
            frame
        );

        let run_snapshot = RunSubscriptionSnapshotDto::new(
            fixture_run(session_id, run_id),
            vec![fixture_message(session_id, run_id)],
        )
        .expect("fixture run snapshot is valid");
        let response_payload = ProtocolResponsePayloadDto::RunSubscription(
            RunSubscriptionResponseDto::snapshot(run_snapshot),
        );
        let response = encode_response(7, response_payload);
        let line = serde_json::to_string(&response).expect("response serializes");
        let decoded =
            decode_response(&line, ProtocolMethodDto::RunSubscribe, 7).expect("response decodes");
        assert!(matches!(
            decoded,
            ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Snapshot(_))
        ));
        assert!(
            decode_response(&line, ProtocolMethodDto::DaemonHealth, 7).is_err(),
            "a response family mismatch fails closed"
        );

        let failure = JsonRpcResponseDto::<ProtocolResponsePayloadDto>::error(
            Some(7),
            JsonRpcErrorDto::from_error(
                JSONRPC_VERSION_MISMATCH,
                ErrorDto::unavailable(
                    "incompatible_protocol_version",
                    "protocol version must equal the current version",
                ),
            ),
        );
        let line = serde_json::to_string(&failure).expect("error serializes");
        let error = decode_response(&line, ProtocolMethodDto::DaemonHealth, 7)
            .expect_err("error responses surface the typed error");
        assert_eq!(error.code(), "incompatible_protocol_version");

        let unknown = r#"{"jsonrpc":"2.0","id":5,"method":"fixture.unknown","params":null}"#;
        let failure = decode_request_line(unknown).expect_err("unknown methods fail");
        assert_eq!(failure.error().code(), JSONRPC_METHOD_NOT_FOUND);
        assert_eq!(failure.id(), Some(5));

        let mismatched = JsonRpcRequestDto::new(
            6,
            ProtocolMethodDto::DaemonHealth.as_str(),
            ProtocolRequestPayloadDto::Command(ProtocolCommandDto::InterruptRun(
                InterruptRunCommandDto::new(session_id, run_id),
            )),
        );
        let line = serde_json::to_string(&mismatched).expect("mismatched request serializes");
        let failure = decode_request_line(&line).expect_err("method and payload must match");
        assert_eq!(
            failure.error().code(),
            crate::jsonrpc::JSONRPC_INVALID_PARAMS
        );
        assert_eq!(failure.id(), Some(6));

        let hello = ProtocolHelloDto::new(CURRENT_PROTOCOL_VERSION, "fixture").expect("hello");
        let request = encode_hello_request(1, hello.clone());
        let line = serde_json::to_string(&request).expect("hello request serializes");
        let decoded: JsonRpcRequestDto<ProtocolHelloDto> =
            JsonRpcRequestDto::parse(&line).expect("hello parses");
        assert_eq!(
            decode_hello_request(&decoded).expect("hello accepted"),
            hello
        );
        let response = encode_hello_response(1, hello.clone());
        let line = serde_json::to_string(&response).expect("hello response serializes");
        let decoded: JsonRpcResponseDto<ProtocolHelloDto> =
            JsonRpcResponseDto::parse(&line).expect("hello response parses");
        assert_eq!(
            decode_hello_response(&decoded).expect("hello accepted"),
            hello
        );

        let stale = ProtocolHelloDto::new(ProtocolVersionDto::new(1, 1), "fixture")
            .expect("stale hello is well-formed");
        let request = encode_hello_request(1, stale);
        let line = serde_json::to_string(&request).expect("stale hello serializes");
        let decoded: JsonRpcRequestDto<ProtocolHelloDto> =
            JsonRpcRequestDto::parse(&line).expect("stale hello parses");
        let mismatch = decode_hello_request(&decoded).expect_err("version mismatch is typed");
        assert_eq!(mismatch.code(), JSONRPC_VERSION_MISMATCH);
        assert_eq!(
            mismatch.data().map(ErrorDto::code),
            Some("incompatible_protocol_version")
        );
    }

    #[test]
    fn remaining_constructor_and_deserialization_error_paths_are_checked() {
        let session = SessionId::new();
        let run = RunId::new();
        assert_eq!(
            RunSubscriptionSnapshotDto::new(
                fixture_run(session, run),
                vec![fixture_message(SessionId::new(), run)]
            )
            .expect_err("a message from another session is rejected")
            .code(),
            "invalid_run_subscription_snapshot"
        );
        assert_eq!(
            RunSubscriptionSnapshotDto::new(
                fixture_run(session, run),
                vec![fixture_message(session, RunId::new())]
            )
            .expect_err("a message bound to another run is rejected")
            .code(),
            "invalid_run_subscription_snapshot"
        );
        assert!(
            serde_json::from_str::<RunStreamFrameDto>(r#"{"kind":"unknown","data":{}}"#).is_err()
        );
        assert!(
            serde_json::from_str::<RunSubscriptionResponseDto>(r#"{"kind":"unknown","data":{}}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<SessionSubscriptionResponseDto>(
                r#"{"kind":"unknown","data":{}}"#
            )
            .is_err()
        );
    }
}
