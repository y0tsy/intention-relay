//! Typed local-protocol DTOs for Intention Relay.
//!
//! One typed message travels per NDJSON line: a correlated client request, a
//! correlated daemon reply or rejection, or an uncorrelated committed run
//! frame. The typed request and result enums are the complete wire contract:
//! there is no method table, no second error channel, and no version
//! negotiation. This module defines typed wire contracts only. It contains no
//! socket framing, client bootstrap, daemon lifecycle, runtime actors, or
//! presentation logic.

use crate::{
    AcceptProviderCatalogRemovalCommandDto, ApplyConfigurationDocumentCommandDto,
    ApplyConfigurationEditsCommandDto, CheckProviderHealthCommandDto, ConfigRevisionId,
    ConfigurationEditAcceptedDto, ConfigurationReloadAcceptedDto, CreateSessionCommandDto,
    CredentialRotationAcceptedDto, DiscoverProviderModelsCommandDto, DtoResult, ErrorDto,
    GetSessionProviderProfileQueryDto, GetSessionSnapshotQueryDto, InterruptRunCommandDto,
    ListProviderCatalogQueryDto, MessageProjectionDto, ProjectId,
    ProviderCatalogCandidateRejectedDto, ProviderCatalogPageDto, ProviderCatalogRemovalAcceptedDto,
    ProviderCatalogStatusDto, ProviderDiscoveryResultDto, ProviderHealthEvidenceDto,
    RejectProviderCatalogCandidateCommandDto, ReloadConfigurationCommandDto, RemoveTurnCommandDto,
    RotateProviderCredentialCommandDto, RunId, RunProjectionDto, SendUserTurnCommandDto, SessionId,
    SessionProjectionDto, SessionProviderProfileProjectionDto,
    SetSessionProviderProfileAcceptedDto, SetSessionProviderProfileCommandDto, TurnId, WorkspaceId,
};
use serde::{Deserialize, Deserializer, Serialize};

/// The daemon's readiness for requests.
///
/// A daemon answers health only after durable recovery completed and it can
/// serve requests, so this is the only readiness state a production path
/// produces; another state is added when a drain path exists.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonReadinessDto {
    /// The daemon can serve requests.
    Ready,
}

/// A version-free, credential-free health projection from the daemon authority.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonHealthDto {
    readiness: DaemonReadinessDto,
}

impl DaemonHealthDto {
    /// Creates the ready health projection a serving daemon answers with.
    #[must_use]
    pub const fn ready() -> Self {
        Self {
            readiness: DaemonReadinessDto::Ready,
        }
    }

    /// Returns the daemon readiness state.
    #[must_use]
    pub const fn readiness(self) -> DaemonReadinessDto {
        self.readiness
    }
}

/// A subscription request scoped to one durable session and run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubscribeRunCommandDto {
    session_id: SessionId,
    run_id: RunId,
}

impl SubscribeRunCommandDto {
    /// Creates a run-scoped subscription request.
    #[must_use]
    pub const fn new(session_id: SessionId, run_id: RunId) -> Self {
        Self { session_id, run_id }
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

/// A server-originated, uncorrelated committed run-stream frame.
///
/// The wire tag is `kind` with `content` for one committed transcript row and
/// `status` for one committed run projection; the payload travels in `data` and
/// carries no event position. A status frame carries the committed projection
/// itself, never a status delta a client would have to merge.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum RunStreamFrameDto {
    /// One committed transcript row.
    Content(MessageProjectionDto),
    /// The committed run projection after one status change.
    Status(RunProjectionDto),
}

/// The current run state returned by a dedicated run subscription.
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
        Self::new(raw.run, raw.messages).map_err(serde::de::Error::custom)
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

/// A current session projection with its committed transcript rows.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionSnapshotDto {
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
            session_id: SessionId,
            projection: SessionProjectionDto,
            messages: Vec<MessageProjectionDto>,
        }
        let raw = RawSessionSnapshotDto::deserialize(deserializer)?;
        Self::with_projection(raw.session_id, raw.projection, raw.messages)
            .map_err(serde::de::Error::custom)
    }
}

impl SessionSnapshotDto {
    /// Creates a session snapshot containing a coherent state projection.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the projection session identity or any
    /// carried transcript row differs from this snapshot session.
    pub fn with_projection(
        session_id: SessionId,
        projection: SessionProjectionDto,
        messages: Vec<MessageProjectionDto>,
    ) -> DtoResult<Self> {
        if projection.session_id() != session_id
            || messages
                .iter()
                .any(|message| message.session_id() != session_id)
        {
            return Err(ErrorDto::validation(
                "invalid_session_snapshot_projection",
                "snapshot projection and transcript rows must share the snapshot session",
            ));
        }
        Ok(Self {
            session_id,
            projection,
            messages,
        })
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

/// One typed request from a local adapter to the daemon.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ClientRequestDto {
    /// Requests durable creation of a session.
    CreateSession(CreateSessionCommandDto),
    /// Sends an accepted user turn to the daemon authority.
    SendUserTurn(SendUserTurnCommandDto),
    /// Requests removal of a not-yet-seen pending user turn.
    RemoveTurn(RemoveTurnCommandDto),
    /// Requests interruption of an active daemon-owned run's current operation.
    InterruptRun(InterruptRunCommandDto),
    /// Obtains the latest durable session projection.
    GetSessionSnapshot(GetSessionSnapshotQueryDto),
    /// Obtains the daemon's latest health projection.
    GetDaemonHealth,
    /// Begins a dedicated run-stream subscription.
    SubscribeRun(SubscribeRunCommandDto),
    /// Lists one bounded page of the active provider catalog.
    ListProviderCatalog(ListProviderCatalogQueryDto),
    /// Obtains the current provider catalog status.
    GetProviderCatalogStatus,
    /// Changes the durable session default provider profile.
    SetSessionProviderProfile(SetSessionProviderProfileCommandDto),
    /// Obtains one session's provider profile projection.
    GetSessionProviderProfile(GetSessionProviderProfileQueryDto),
    /// Accepts one pending catalog removal.
    AcceptProviderCatalogRemoval(AcceptProviderCatalogRemovalCommandDto),
    /// Rejects one pending catalog candidate.
    RejectProviderCatalogCandidate(RejectProviderCatalogCandidateCommandDto),
    /// Reloads configuration through the private loading boundary.
    ReloadConfiguration(ReloadConfigurationCommandDto),
    /// Rotates one profile's private credential material.
    RotateProviderCredential(RotateProviderCredentialCommandDto),
    /// Checks one profile's live provider health.
    CheckProviderHealth(CheckProviderHealthCommandDto),
    /// Begins one provider/model discovery attempt.
    DiscoverProviderModels(DiscoverProviderModelsCommandDto),
    /// Applies one credential-free configuration document.
    ApplyConfigurationDocument(ApplyConfigurationDocumentCommandDto),
    /// Applies closed typed configuration edits.
    ApplyConfigurationEdits(ApplyConfigurationEditsCommandDto),
}

/// One correlated client request line.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolRequestDto {
    id: u64,
    request: ClientRequestDto,
}

impl ProtocolRequestDto {
    /// Creates a request from its wire identity and typed body.
    #[must_use]
    pub const fn new(id: u64, request: ClientRequestDto) -> Self {
        Self { id, request }
    }

    /// Returns the request identity the daemon echo carries.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Returns the typed request body.
    #[must_use]
    pub const fn request(&self) -> &ClientRequestDto {
        &self.request
    }

    /// Consumes the request line and returns its typed body.
    #[must_use]
    pub fn into_request(self) -> ClientRequestDto {
        self.request
    }
}

/// The typed result of one client request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProtocolResultDto {
    /// A durable session was created.
    SessionCreated(CreateSessionAcceptedDto),
    /// A user turn was accepted and either started a run or became a pending message.
    TurnAccepted(SendUserTurnAcceptedDto),
    /// A pending user turn was removed.
    TurnRemoved(RemoveTurnAcceptedDto),
    /// An interruption request was accepted for a run.
    RunInterrupted(InterruptRunAcceptedDto),
    /// A current durable session projection.
    SessionSnapshot(SessionSnapshotDto),
    /// The daemon health projection.
    DaemonHealth(DaemonHealthDto),
    /// The current run state of a new subscription.
    RunSubscribed(RunSubscriptionSnapshotDto),
    /// One bounded page of the active provider catalog.
    ProviderCatalogPage(ProviderCatalogPageDto),
    /// The current provider catalog status.
    ProviderCatalogStatus(ProviderCatalogStatusDto),
    /// Acceptance evidence for a session default profile change.
    SessionProviderProfileSet(SetSessionProviderProfileAcceptedDto),
    /// One session's provider profile projection.
    SessionProviderProfile(SessionProviderProfileProjectionDto),
    /// Acceptance evidence for an accepted catalog removal.
    ProviderCatalogRemovalAccepted(ProviderCatalogRemovalAcceptedDto),
    /// Evidence for a rejected catalog candidate.
    ProviderCatalogCandidateRejected(ProviderCatalogCandidateRejectedDto),
    /// Acceptance evidence for a controlled configuration reload.
    ConfigurationReloaded(ConfigurationReloadAcceptedDto),
    /// Acceptance evidence for a credential rotation.
    ProviderCredentialRotated(CredentialRotationAcceptedDto),
    /// Non-authorizing provider health evidence.
    ProviderHealth(ProviderHealthEvidenceDto),
    /// One non-authorizing discovery result.
    ProviderModelsDiscovered(ProviderDiscoveryResultDto),
    /// Acceptance evidence for an applied configuration document.
    ConfigurationDocumentApplied(ConfigurationEditAcceptedDto),
    /// Acceptance evidence for applied typed configuration edits.
    ConfigurationEditsApplied(ConfigurationEditAcceptedDto),
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

/// The durable disposition of an accepted user turn.
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

/// One correlated daemon reply carrying the request's typed result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolReplyDto {
    id: u64,
    result: ProtocolResultDto,
}

impl ProtocolReplyDto {
    /// Creates a reply for one request identity.
    #[must_use]
    pub const fn new(id: u64, result: ProtocolResultDto) -> Self {
        Self { id, result }
    }

    /// Returns the echoed request identity.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Returns the typed result.
    #[must_use]
    pub const fn result(&self) -> &ProtocolResultDto {
        &self.result
    }

    /// Consumes the reply and returns its typed result.
    #[must_use]
    pub fn into_result(self) -> ProtocolResultDto {
        self.result
    }
}

/// One correlated daemon rejection carrying a request's typed error.
///
/// The identity is absent only when the request line itself could not be
/// decoded, so there is no request identity to echo.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolRejectionDto {
    id: Option<u64>,
    error: ErrorDto,
}

impl ProtocolRejectionDto {
    /// Creates a rejection; `id` is absent for an undecodable request line.
    #[must_use]
    pub const fn new(id: Option<u64>, error: ErrorDto) -> Self {
        Self { id, error }
    }

    /// Returns the echoed request identity when one could be recovered.
    #[must_use]
    pub const fn id(&self) -> Option<u64> {
        self.id
    }

    /// Returns the typed safe error.
    #[must_use]
    pub const fn error(&self) -> &ErrorDto {
        &self.error
    }

    /// Consumes the rejection and returns its typed error.
    #[must_use]
    pub fn into_error(self) -> ErrorDto {
        self.error
    }
}

/// One daemon-to-client line: a correlated reply, a correlated rejection, or an
/// uncorrelated committed run frame.
#[expect(
    clippy::large_enum_variant,
    reason = "One committed projection travels by value per daemon envelope; boxing the reply would add indirection to every dispatch site without changing the wire."
)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProtocolDaemonMessageDto {
    /// A correlated reply carrying one request's typed result.
    Reply(ProtocolReplyDto),
    /// A correlated rejection carrying one request's typed error.
    Rejection(ProtocolRejectionDto),
    /// A committed run frame published by an established subscription.
    Frame(RunStreamFrameDto),
}

impl ProtocolDaemonMessageDto {
    /// Creates the reply for one request identity.
    #[must_use]
    pub const fn reply(id: u64, result: ProtocolResultDto) -> Self {
        Self::Reply(ProtocolReplyDto::new(id, result))
    }

    /// Creates the rejection for one request identity.
    #[must_use]
    pub const fn rejection(id: Option<u64>, error: ErrorDto) -> Self {
        Self::Rejection(ProtocolRejectionDto::new(id, error))
    }

    /// Creates the frame carrying one committed run-stream value.
    #[must_use]
    pub const fn frame(frame: RunStreamFrameDto) -> Self {
        Self::Frame(frame)
    }
}

/// Encodes one typed request as its correlated wire message.
#[must_use]
pub const fn encode_request(id: u64, request: ClientRequestDto) -> ProtocolRequestDto {
    ProtocolRequestDto::new(id, request)
}

/// Decodes one request line.
///
/// # Errors
///
/// Returns a typed validation error when the line is not a current typed
/// request. The caller answers the failure with an identity-less rejection and
/// keeps the connection serving.
pub fn decode_request_line(line: &str) -> DtoResult<ProtocolRequestDto> {
    serde_json::from_str(line).map_err(|_| {
        ErrorDto::validation(
            "invalid_local_protocol_message",
            "a local protocol request was invalid",
        )
    })
}

/// Encodes one typed reply as its correlated wire message.
#[must_use]
pub const fn encode_reply(id: u64, result: ProtocolResultDto) -> ProtocolDaemonMessageDto {
    ProtocolDaemonMessageDto::reply(id, result)
}

/// The envelope kinds of the current daemon wire.
const DAEMON_MESSAGE_KINDS: [&str; 3] = ["reply", "rejection", "frame"];

/// Parses one daemon line.
///
/// # Errors
///
/// Returns a typed `stale_daemon_protocol` unavailable error when the line is
/// not an envelope of the current wire, so an adapter can offer a safe
/// restart/reconnect action instead of reinterpreting foreign bytes. A line
/// that names a current envelope kind whose payload does not decode is this
/// wire's own failure, so it returns the typed
/// `invalid_local_protocol_response` error instead of blaming a stale peer.
pub fn parse_daemon_message(line: &str) -> DtoResult<ProtocolDaemonMessageDto> {
    serde_json::from_str(line).map_err(|_| match envelope_kind(line) {
        Some(kind) if DAEMON_MESSAGE_KINDS.contains(&kind) => invalid_wire_response(),
        _ => ErrorDto::unavailable(
            "stale_daemon_protocol",
            "the local daemon does not speak the current local wire protocol",
        ),
    })
}

/// Returns the envelope kind one daemon line names, when it names one.
fn envelope_kind(line: &str) -> Option<&str> {
    #[derive(Deserialize)]
    struct EnvelopeKindDto<'a> {
        kind: &'a str,
    }

    serde_json::from_str::<EnvelopeKindDto<'_>>(line)
        .ok()
        .map(|envelope| envelope.kind)
}

/// Decodes the correlated answer to one request.
///
/// An identity-less rejection is the daemon's answer to a request line it could
/// not decode, so it carries no identity to match; accepting it for `id` is
/// unambiguous only while one connection carries one in-flight request, which
/// is the client's dispatch convention.
///
/// # Errors
///
/// Returns the typed rejection the daemon carried for this request identity,
/// and an `invalid_local_protocol_response` error when the line is not the
/// correlated answer to `id`.
pub fn decode_response(line: &str, id: u64) -> DtoResult<ProtocolResultDto> {
    match parse_daemon_message(line)? {
        ProtocolDaemonMessageDto::Reply(reply) if reply.id() == id => Ok(reply.into_result()),
        // The daemon answers an undecodable request line without an identity,
        // and one in-flight request per connection makes that answer this one's.
        ProtocolDaemonMessageDto::Rejection(rejection)
            if rejection.id().is_none() || rejection.id() == Some(id) =>
        {
            Err(rejection.into_error())
        }
        _ => Err(invalid_wire_response()),
    }
}

/// Parses one uncorrelated committed run-frame line.
///
/// # Errors
///
/// Returns an `invalid_local_protocol_response` error when the line is not a
/// committed run frame, so a reply is never accepted as a frame.
pub fn parse_run_frame(line: &str) -> DtoResult<RunStreamFrameDto> {
    match parse_daemon_message(line)? {
        ProtocolDaemonMessageDto::Frame(frame) => Ok(frame),
        _ => Err(invalid_wire_response()),
    }
}

fn invalid_wire_response() -> ErrorDto {
    ErrorDto::validation(
        "invalid_local_protocol_response",
        "the local daemon returned an unexpected protocol response",
    )
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
            crate::RunModeDto::Build,
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
            crate::RunStatusDto::Running,
            ConfigRevisionId::new(),
        )
    }

    #[test]
    fn snapshots_round_trip_current_state() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let snapshot = SessionSnapshotDto::with_projection(
            session_id,
            fixture_projection(session_id),
            vec![fixture_message(session_id, run_id)],
        )
        .expect("fixture snapshot is valid");
        assert_eq!(snapshot.messages().len(), 1);
        assert_eq!(
            serde_json::from_str::<SessionSnapshotDto>(
                &serde_json::to_string(&snapshot).expect("snapshot serializes")
            )
            .expect("snapshot deserializes"),
            snapshot
        );

        let run_snapshot = RunSubscriptionSnapshotDto::new(
            fixture_run(session_id, run_id),
            vec![fixture_message(session_id, run_id)],
        )
        .expect("fixture run snapshot is valid");
        let result = ProtocolResultDto::RunSubscribed(run_snapshot);
        assert_eq!(
            serde_json::from_str::<ProtocolResultDto>(
                &serde_json::to_string(&result).expect("run result serializes")
            )
            .expect("run result deserializes"),
            result
        );

        // A bounded read reports its omitted pending turns, and the nested
        // projection carries that count across the wire.
        let trimmed = SessionSnapshotDto::with_projection(
            session_id,
            fixture_projection(session_id).with_pending_turns_omitted(3),
            Vec::new(),
        )
        .expect("a snapshot with a trimmed projection is valid");
        assert_eq!(trimmed.projection().pending_turns_omitted(), 3);
        assert_eq!(
            serde_json::from_str::<SessionSnapshotDto>(
                &serde_json::to_string(&trimmed).expect("snapshot serializes")
            )
            .expect("snapshot deserializes"),
            trimmed
        );
    }

    #[test]
    fn wire_helpers_round_trip_replies_rejections_and_frames() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let frame = RunStreamFrameDto::Status(fixture_run(session_id, run_id));
        let message = ProtocolDaemonMessageDto::frame(frame.clone());
        let line = serde_json::to_string(&message).expect("frame message serializes");
        assert_eq!(parse_run_frame(&line).expect("frame parses"), frame);
        assert!(
            decode_response(&line, 7).is_err(),
            "a frame is never accepted as a correlated reply"
        );

        let reply = encode_reply(
            7,
            ProtocolResultDto::RunSubscribed(
                RunSubscriptionSnapshotDto::new(
                    fixture_run(session_id, run_id),
                    vec![fixture_message(session_id, run_id)],
                )
                .expect("fixture run snapshot is valid"),
            ),
        );
        let line = serde_json::to_string(&reply).expect("reply serializes");
        assert!(matches!(
            decode_response(&line, 7).expect("the correlated reply decodes"),
            ProtocolResultDto::RunSubscribed(_)
        ));
        assert!(
            decode_response(&line, 8).is_err(),
            "an uncorrelated reply fails closed"
        );

        let rejection = ProtocolDaemonMessageDto::rejection(
            Some(7),
            ErrorDto::validation("fixture_rejected", "fixture rejection"),
        );
        let line = serde_json::to_string(&rejection).expect("rejection serializes");
        assert_eq!(
            decode_response(&line, 7)
                .expect_err("a rejection surfaces the typed error")
                .code(),
            "fixture_rejected"
        );

        let identity_less = ProtocolDaemonMessageDto::rejection(
            None,
            ErrorDto::validation("invalid_local_protocol_message", "fixture malformed line"),
        );
        let line = serde_json::to_string(&identity_less).expect("rejection serializes");
        assert_eq!(
            decode_response(&line, 11)
                .expect_err("an identity-less rejection still surfaces its error")
                .code(),
            "invalid_local_protocol_message"
        );

        let request = encode_request(
            3,
            ClientRequestDto::InterruptRun(InterruptRunCommandDto::new(session_id, run_id)),
        );
        let line = serde_json::to_string(&request).expect("request serializes");
        let decoded = decode_request_line(&line).expect("request decodes");
        assert_eq!(decoded.id(), 3);
        assert_eq!(decoded.request(), request.request());

        let foreign = encode_request(4, ClientRequestDto::GetDaemonHealth);
        let line = serde_json::to_string(&foreign).expect("request serializes");
        assert!(
            parse_run_frame(&line).is_err(),
            "a request line is never accepted as a run frame"
        );
        assert_eq!(
            parse_daemon_message(&line)
                .expect_err("a request line is not a daemon message")
                .code(),
            "stale_daemon_protocol"
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
            serde_json::from_str::<ProtocolResultDto>(r#"{"kind":"unknown","data":{}}"#).is_err()
        );
        assert!(
            serde_json::from_str::<ClientRequestDto>(r#"{"kind":"unknown","data":{}}"#).is_err()
        );
        assert_eq!(
            decode_request_line("{")
                .expect_err("malformed lines fail closed")
                .code(),
            "invalid_local_protocol_message"
        );
    }
}
