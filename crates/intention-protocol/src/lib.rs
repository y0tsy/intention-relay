//! Versioned public local-protocol DTOs for Intention Relay.
//!
//! This crate defines typed wire contracts only. It contains no socket framing,
//! client bootstrap, daemon lifecycle, runtime actors, or presentation logic.

use crate::contract_families::{
    AcceptProviderCatalogRemovalAcceptedDto, AcceptProviderCatalogRemovalCommandDto,
    AdmitRecoveredRunAcceptedDto, AdmitRecoveredRunCommandDto, ConfigurationEditCommandDto,
    ConfigurationProjectionDto, CredentialRotationResultDto, GetConfigurationProjectionQueryDto,
    GetPricingPolicyQueryDto, GetProviderCatalogQueryDto, GetProviderCatalogStatusQueryDto,
    GetProviderDiscoveryStatusQueryDto, GetProviderHealthEvidenceQueryDto,
    GetProviderUsageQueryDto, GetSessionProviderProfileQueryDto, PricingProjectionDto,
    ProviderCatalogPageDto, ProviderCatalogStatusDto, ProviderDiscoveryProjectionDto,
    ProviderHealthProjectionDto, RawTomlEditCommandDto, ReconcileUnavailableQueueAcceptedDto,
    ReconcileUnavailableQueueCommandDto, RejectProviderCatalogCandidateAcceptedDto,
    RejectProviderCatalogCandidateCommandDto, ReloadConfigurationCommandDto, ReloadTransactionDto,
    RotateProviderCredentialsCommandDto, SessionProviderProfileDto,
    SetSessionProviderProfileAcceptedDto, SetSessionProviderProfileCommandDto, UsageAggregationDto,
};
use intention_domain::{
    CreateSessionCommandDto, DomainEventDto, GetSessionSnapshotQueryDto, ModelRunFactDto,
    RemoveQueuedTurnCommandDto, RunEventCursorDto, RunModeDto, RunReplayDto, RunSnapshotDto,
    SendUserTurnCommandDto, SessionProjectionDto, StopRunCommandDto,
};
use intention_types::{
    ConfigRevisionId, CorrelationIdDto, DtoResult, ErrorDto, EventEnvelopeDto, ProjectId,
    QueuePositionDto, RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId, TurnId,
    WorkspaceId,
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

/// A feature the local adapter and daemon both understand.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolCapabilityDto {
    /// The peer can subscribe to ordered session snapshots and event tails.
    SessionSubscriptions,
    /// The peer can exchange correlation-bound request and response envelopes.
    CorrelatedRequests,
    /// The peer can obtain daemon health and readiness projections.
    DaemonHealth,
    /// The peer can subscribe to dedicated persistent run streams.
    RunStreamSubscriptions,
    ProviderProfilesV1,
    SessionForkV1,
    NormalizedReasoningStreamV1,
    AgentActivityV1,
    UserNotificationsV1,
    DaemonToolGatewayV1,
    ModelToolLoopV1,
}

/// The currently activated local protocol version.
///
/// Slice 2 stays on protocol 1.1 and DTO schema 1.1 under the ADR 0038
/// single-version policy: negotiation accepts only the exact current 1.1
/// versions, previously required fields stay required, and fields that became
/// mandatory are mandatory on the wire. Decoding tolerates unknown additive
/// fields (forward compatibility) but no longer accepts older shapes as if
/// they were current; prior same-major (1.0-to-1.1) compatibility and 1.0
/// fixtures were removed with that machinery.
pub const CURRENT_PROTOCOL_VERSION: ProtocolVersionDto = ProtocolVersionDto::new(1, 1);
/// The currently activated public DTO schema version.
pub const CURRENT_DTO_SCHEMA_VERSION: SchemaVersionDto = SchemaVersionDto::new(1, 1);

/// Capabilities introduced after the M4 protocol surface.
pub const POST_M5_CAPABILITIES: [ProtocolCapabilityDto; 7] = [
    ProtocolCapabilityDto::ProviderProfilesV1,
    ProtocolCapabilityDto::SessionForkV1,
    ProtocolCapabilityDto::NormalizedReasoningStreamV1,
    ProtocolCapabilityDto::AgentActivityV1,
    ProtocolCapabilityDto::UserNotificationsV1,
    ProtocolCapabilityDto::DaemonToolGatewayV1,
    ProtocolCapabilityDto::ModelToolLoopV1,
];

pub mod contract_families;
pub mod negotiation;
pub use negotiation::ProtocolNegotiationResultDto;

/// A safe metadata handshake exchanged before any protocol command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProtocolHelloDto {
    version: ProtocolVersionDto,
    capabilities: Vec<ProtocolCapabilityDto>,
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
            capabilities: Vec<ProtocolCapabilityDto>,
            adapter_name: String,
        }

        let raw = RawProtocolHelloDto::deserialize(deserializer)?;
        Self::new(raw.version, raw.capabilities, raw.adapter_name).map_err(de::Error::custom)
    }
}

impl ProtocolHelloDto {
    /// Creates a handshake with a non-empty adapter metadata name.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the adapter name is blank.
    pub fn new(
        version: ProtocolVersionDto,
        capabilities: Vec<ProtocolCapabilityDto>,
        adapter_name: impl Into<String>,
    ) -> DtoResult<Self> {
        let adapter_name = adapter_name.into();
        if adapter_name.trim().is_empty() {
            Err(ErrorDto::validation(
                "invalid_adapter_name",
                "adapter name must not be empty",
            ))
        } else {
            Ok(Self {
                version,
                capabilities,
                adapter_name,
            })
        }
    }

    /// Returns the peer protocol version.
    #[must_use]
    pub const fn version(&self) -> ProtocolVersionDto {
        self.version
    }

    /// Returns the peer's explicitly declared capabilities.
    #[must_use]
    pub fn capabilities(&self) -> &[ProtocolCapabilityDto] {
        &self.capabilities
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

    /// Returns the protocol version served by the daemon.
    #[must_use]
    pub const fn protocol_version(self) -> ProtocolVersionDto {
        self.protocol_version
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
    schema_version: SchemaVersionDto,
    session_id: SessionId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    run_id: Option<RunId>,
    after_sequence: Option<SessionEventSequenceDto>,
    requested_mode: RunModeDto,
}

impl SubscribeSessionCommandDto {
    /// Creates a typed session-wide subscription request.
    ///
    /// This preserves the version-one constructor shape. Use
    /// [`Self::with_run_id`] to scope a subscription to a particular run.
    #[must_use]
    pub const fn new(
        schema_version: SchemaVersionDto,
        session_id: SessionId,
        after_sequence: Option<SessionEventSequenceDto>,
        requested_mode: RunModeDto,
    ) -> Self {
        Self {
            schema_version,
            session_id,
            run_id: None,
            after_sequence,
            requested_mode,
        }
    }

    /// Creates a typed subscription request with an optional run scope.
    #[must_use]
    pub const fn with_run_id(
        schema_version: SchemaVersionDto,
        session_id: SessionId,
        run_id: Option<RunId>,
        after_sequence: Option<SessionEventSequenceDto>,
        requested_mode: RunModeDto,
    ) -> Self {
        Self {
            schema_version,
            session_id,
            run_id,
            after_sequence,
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

    /// Returns the last durable sequence already observed, if any.
    #[must_use]
    pub const fn after_sequence(self) -> Option<SessionEventSequenceDto> {
        self.after_sequence
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
    schema_version: SchemaVersionDto,
    session_id: SessionId,
    run_id: RunId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    after_cursor: Option<RunEventCursorDto>,
}

impl SubscribeRunCommandDto {
    /// Creates a run-scoped subscription request from an optional known cursor.
    #[must_use]
    pub const fn new(
        schema_version: SchemaVersionDto,
        session_id: SessionId,
        run_id: RunId,
        after_cursor: Option<RunEventCursorDto>,
    ) -> Self {
        Self {
            schema_version,
            session_id,
            run_id,
            after_cursor,
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

    /// Returns the last accepted run-fact cursor, if any.
    #[must_use]
    pub const fn after_cursor(self) -> Option<RunEventCursorDto> {
        self.after_cursor
    }
}

/// The closed reason a run subscriber must discard its current run state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunResyncReasonDto {
    /// The requested run history is unavailable for a contiguous replay.
    HistoryUnavailable,
    /// The requested run cursor is invalid.
    InvalidCursor,
    /// The receiver observed a non-contiguous fact range.
    CursorGap,
    /// The daemon could not retain this subscriber safely.
    SubscriberTooSlow,
}

/// A typed run-scoped resynchronization instruction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunResyncDto {
    session_id: SessionId,
    run_id: RunId,
    reason: RunResyncReasonDto,
}

impl RunResyncDto {
    /// Creates a typed run resynchronization instruction.
    #[must_use]
    pub const fn new(session_id: SessionId, run_id: RunId, reason: RunResyncReasonDto) -> Self {
        Self {
            session_id,
            run_id,
            reason,
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

    /// Returns the closed resynchronization reason.
    #[must_use]
    pub const fn reason(self) -> RunResyncReasonDto {
        self.reason
    }
}

/// A non-empty, contiguous run-fact range emitted after durable commit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunLiveBatchDto {
    session_id: SessionId,
    run_id: RunId,
    after_cursor: RunEventCursorDto,
    facts: Vec<ModelRunFactDto>,
    next_after_cursor: RunEventCursorDto,
}

impl<'de> Deserialize<'de> for RunLiveBatchDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawRunLiveBatchDto {
            session_id: SessionId,
            run_id: RunId,
            after_cursor: RunEventCursorDto,
            facts: Vec<ModelRunFactDto>,
            next_after_cursor: RunEventCursorDto,
        }
        let raw = RawRunLiveBatchDto::deserialize(deserializer)?;
        Self::new(
            raw.session_id,
            raw.run_id,
            raw.after_cursor,
            raw.facts,
            raw.next_after_cursor,
        )
        .map_err(de::Error::custom)
    }
}

impl RunLiveBatchDto {
    /// Creates a non-empty contiguous range of positive run facts.
    ///
    /// # Errors
    ///
    /// Returns a validation error when facts are empty, non-contiguous, or the
    /// continuation does not equal the final fact cursor.
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        after_cursor: RunEventCursorDto,
        facts: Vec<ModelRunFactDto>,
        next_after_cursor: RunEventCursorDto,
    ) -> DtoResult<Self> {
        if facts.is_empty() {
            return Err(ErrorDto::validation(
                "invalid_run_live_batch",
                "run live batches must contain at least one fact",
            ));
        }
        let mut expected = after_cursor.value();
        for fact in &facts {
            expected = expected.checked_add(1).ok_or_else(|| {
                ErrorDto::validation("invalid_run_live_batch", "run fact cursor overflow")
            })?;
            if fact.cursor().value() != expected {
                return Err(ErrorDto::validation(
                    "invalid_run_live_batch",
                    "run live batch facts must be contiguous after the cursor",
                ));
            }
        }
        if next_after_cursor.value() != expected {
            return Err(ErrorDto::validation(
                "invalid_run_live_batch",
                "run live batch continuation must equal its final fact cursor",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            after_cursor,
            facts,
            next_after_cursor,
        })
    }

    /// Returns the scoped session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }
    /// Returns the scoped run identity.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }
    /// Returns the cursor preceding this range.
    #[must_use]
    pub const fn after_cursor(&self) -> RunEventCursorDto {
        self.after_cursor
    }
    /// Returns the contiguous durable facts.
    #[must_use]
    pub fn facts(&self) -> &[ModelRunFactDto] {
        &self.facts
    }
    /// Returns the cursor after the range.
    #[must_use]
    pub const fn next_after_cursor(&self) -> RunEventCursorDto {
        self.next_after_cursor
    }
}

/// A daemon-authoritative run snapshot emitted for status-only durable commits.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunSnapshotFrameDto {
    snapshot: RunSnapshotDto,
}

impl<'de> Deserialize<'de> for RunSnapshotFrameDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawRunSnapshotFrameDto {
            snapshot: RunSnapshotDto,
        }
        let raw = RawRunSnapshotFrameDto::deserialize(deserializer)?;
        Ok(Self::new(raw.snapshot))
    }
}

impl RunSnapshotFrameDto {
    /// Creates an authoritative status snapshot frame.
    #[must_use]
    pub const fn new(snapshot: RunSnapshotDto) -> Self {
        Self { snapshot }
    }
    /// Returns the authoritative daemon snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> &RunSnapshotDto {
        &self.snapshot
    }
    /// Returns the scoped session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.snapshot.session_id()
    }
    /// Returns the scoped run identity.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.snapshot.run_id()
    }
}

/// A server-originated, uncorrelated run-stream frame.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum RunStreamFrameDto {
    /// A contiguous range of committed run facts.
    LiveBatch(RunLiveBatchDto),
    /// An authoritative run status snapshot.
    Snapshot(RunSnapshotFrameDto),
    /// An instruction to clear local run state.
    Resync(RunResyncDto),
}

impl<'de> Deserialize<'de> for RunStreamFrameDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", content = "data", rename_all = "snake_case")]
        enum RawRunStreamFrameDto {
            LiveBatch(RunLiveBatchDto),
            Snapshot(RunSnapshotFrameDto),
            Resync(RunResyncDto),
        }
        Ok(match RawRunStreamFrameDto::deserialize(deserializer)? {
            RawRunStreamFrameDto::LiveBatch(batch) => Self::LiveBatch(batch),
            RawRunStreamFrameDto::Snapshot(snapshot) => Self::Snapshot(snapshot),
            RawRunStreamFrameDto::Resync(resync) => Self::Resync(resync),
        })
    }
}

/// The correlated first reply to a dedicated run subscription request.
#[expect(
    clippy::large_enum_variant,
    reason = "The replay is a stable public wire DTO and must remain unboxed for the same Rust and JSON shape."
)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum RunSubscriptionResponseDto {
    /// The authoritative current run snapshot and strict-after tail.
    Replay(RunReplayDto),
    /// A typed initial resynchronization requirement.
    Resync(RunResyncDto),
    /// A safe scoped request error, including `run_replay_not_found`.
    Error(ErrorDto),
}

impl<'de> Deserialize<'de> for RunSubscriptionResponseDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", content = "data", rename_all = "snake_case")]
        #[expect(
            clippy::large_enum_variant,
            reason = "The private intermediary mirrors the public unboxed replay DTO for validated wire decoding."
        )]
        enum RawRunSubscriptionResponseDto {
            Replay(RunReplayDto),
            Resync(RunResyncDto),
            Error(ErrorDto),
        }
        Ok(
            match RawRunSubscriptionResponseDto::deserialize(deserializer)? {
                RawRunSubscriptionResponseDto::Replay(replay) => Self::Replay(replay),
                RawRunSubscriptionResponseDto::Resync(resync) => Self::Resync(resync),
                RawRunSubscriptionResponseDto::Error(error) => Self::Error(error),
            },
        )
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
    /// Requests removal of an unstarted queued user turn.
    RemoveQueuedTurn(RemoveQueuedTurnCommandDto),
    /// Requests cancellation of an active daemon-owned run.
    StopRun(StopRunCommandDto),
    /// Begins a typed session event subscription.
    SubscribeSession(SubscribeSessionCommandDto),
    /// Binds a session's durable provider profile intent (control plane).
    SetSessionProviderProfile(SetSessionProviderProfileCommandDto),
    /// Accepts the removal of a prepared provider catalog candidate.
    AcceptProviderCatalogRemoval(AcceptProviderCatalogRemovalCommandDto),
    /// Rejects a provider catalog removal candidate.
    RejectProviderCatalogCandidate(RejectProviderCatalogCandidateCommandDto),
    /// Reconciles a session's unavailable-run queue in bounded pages.
    ReconcileUnavailableQueue(ReconcileUnavailableQueueCommandDto),
    /// Admits a recovered run back into its session without rerouting.
    AdmitRecoveredRun(AdmitRecoveredRunCommandDto),
    /// Reloads daemon configuration from a candidate snapshot or edit.
    ReloadConfiguration(ReloadConfigurationCommandDto),
    /// Rotates provider credentials without ever carrying their material.
    RotateProviderCredentials(RotateProviderCredentialsCommandDto),
    /// Submits a bounded, credential-free raw TOML configuration edit.
    SubmitRawTomlEdit(RawTomlEditCommandDto),
    /// Applies typed, credential-free configuration edit operations.
    ApplyConfigurationEdit(ConfigurationEditCommandDto),
}

/// A typed protocol query wrapper with no transport-specific resources.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProtocolQueryDto {
    /// Obtains the daemon's latest health and readiness projection.
    GetDaemonHealth,
    /// Obtains the latest durable session projection.
    GetSessionSnapshot(GetSessionSnapshotQueryDto),
    /// Obtains a paged provider catalog projection (control plane).
    GetProviderCatalog(GetProviderCatalogQueryDto),
    /// Obtains the provider catalog activation and degradation status.
    GetProviderCatalogStatus(GetProviderCatalogStatusQueryDto),
    /// Obtains one session's durable provider profile projection.
    GetSessionProviderProfile(GetSessionProviderProfileQueryDto),
    /// Obtains a provider usage aggregation for one period.
    GetProviderUsage(GetProviderUsageQueryDto),
    /// Obtains non-authorizing health evidence for one provider (control plane).
    GetProviderHealthEvidence(GetProviderHealthEvidenceQueryDto),
    /// Obtains the status of one provider discovery attempt (control plane).
    GetProviderDiscoveryStatus(GetProviderDiscoveryStatusQueryDto),
    /// Obtains the safe non-authorizing pricing policy projection (control plane).
    GetPricingPolicy(GetPricingPolicyQueryDto),
    /// Obtains the safe applied configuration projection (control plane).
    GetConfigurationProjection(GetConfigurationProjectionQueryDto),
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
    /// A user turn was accepted and either started or queued.
    SendUserTurn(SendUserTurnAcceptedDto),
    /// A queued user turn was removed.
    RemoveQueuedTurn(RemoveQueuedTurnAcceptedDto),
    /// A stop request was durably accepted for a run.
    StopRun(StopRunAcceptedDto),
    /// A session provider profile was durably set or confirmed.
    SetSessionProviderProfile(SetSessionProviderProfileAcceptedDto),
    /// A provider catalog removal was accepted.
    AcceptProviderCatalogRemoval(AcceptProviderCatalogRemovalAcceptedDto),
    /// A provider catalog candidate was rejected.
    RejectProviderCatalogCandidate(RejectProviderCatalogCandidateAcceptedDto),
    /// An unavailable-run queue reconciliation page was accepted.
    ReconcileUnavailableQueue(ReconcileUnavailableQueueAcceptedDto),
    /// A recovered run was admitted back into its session.
    AdmitRecoveredRun(AdmitRecoveredRunAcceptedDto),
    /// A configuration reload transaction reached a durable outcome.
    ReloadConfiguration(ReloadTransactionDto),
    /// A provider credential rotation reached a durable outcome.
    RotateProviderCredentials(CredentialRotationResultDto),
}

/// Typed acceptance evidence for a created session.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateSessionAcceptedDto {
    project_id: ProjectId,
    workspace_id: WorkspaceId,
    session_id: SessionId,
    committed_sequence: SessionEventSequenceDto,
}
impl CreateSessionAcceptedDto {
    /// Creates durable session acceptance evidence.
    #[must_use]
    pub const fn new(
        project_id: ProjectId,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        committed_sequence: SessionEventSequenceDto,
    ) -> Self {
        Self {
            project_id,
            workspace_id,
            session_id,
            committed_sequence,
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
    /// Returns the final sequence committed by this operation.
    #[must_use]
    pub const fn committed_sequence(self) -> SessionEventSequenceDto {
        self.committed_sequence
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
    /// The accepted turn was committed behind active work at its stable queue ticket.
    Queued { queue_position: QueuePositionDto },
}

/// Typed acceptance evidence for one user turn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SendUserTurnAcceptedDto {
    session_id: SessionId,
    turn_id: TurnId,
    committed_sequence: SessionEventSequenceDto,
    outcome: SendUserTurnOutcomeDto,
}
impl SendUserTurnAcceptedDto {
    /// Creates complete durable turn-acceptance evidence.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        turn_id: TurnId,
        committed_sequence: SessionEventSequenceDto,
        outcome: SendUserTurnOutcomeDto,
    ) -> Self {
        Self {
            session_id,
            turn_id,
            committed_sequence,
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
    /// Returns the final sequence committed by this operation.
    #[must_use]
    pub const fn committed_sequence(self) -> SessionEventSequenceDto {
        self.committed_sequence
    }
    /// Returns whether the turn started a run or received a stable queue ticket.
    #[must_use]
    pub const fn outcome(self) -> SendUserTurnOutcomeDto {
        self.outcome
    }
}

/// Typed acceptance evidence for a removed queued turn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoveQueuedTurnAcceptedDto {
    session_id: SessionId,
    turn_id: TurnId,
    committed_sequence: SessionEventSequenceDto,
}
impl RemoveQueuedTurnAcceptedDto {
    /// Creates complete queued-turn removal acceptance evidence.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        turn_id: TurnId,
        committed_sequence: SessionEventSequenceDto,
    ) -> Self {
        Self {
            session_id,
            turn_id,
            committed_sequence,
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
    /// Returns the final sequence committed by this operation.
    #[must_use]
    pub const fn committed_sequence(self) -> SessionEventSequenceDto {
        self.committed_sequence
    }
}

/// Typed acceptance evidence for a stop request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StopRunAcceptedDto {
    session_id: SessionId,
    run_id: RunId,
    committed_sequence: SessionEventSequenceDto,
}
impl StopRunAcceptedDto {
    /// Creates complete stop-request acceptance evidence.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        run_id: RunId,
        committed_sequence: SessionEventSequenceDto,
    ) -> Self {
        Self {
            session_id,
            run_id,
            committed_sequence,
        }
    }
    /// Returns the owning session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }
    /// Returns the stopping run identity.
    #[must_use]
    pub const fn run_id(self) -> RunId {
        self.run_id
    }
    /// Returns the final sequence committed by this operation.
    #[must_use]
    pub const fn committed_sequence(self) -> SessionEventSequenceDto {
        self.committed_sequence
    }
}

/// A versioned current position for a durable session projection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionSnapshotDto {
    schema_version: SchemaVersionDto,
    session_id: SessionId,
    at_sequence: SessionEventSequenceDto,
    projection: SessionProjectionDto,
}

impl<'de> Deserialize<'de> for SessionSnapshotDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawSessionSnapshotDto {
            schema_version: SchemaVersionDto,
            session_id: SessionId,
            at_sequence: SessionEventSequenceDto,
            projection: SessionProjectionDto,
        }
        let raw = RawSessionSnapshotDto::deserialize(deserializer)?;
        Self::with_projection(
            raw.schema_version,
            raw.session_id,
            raw.at_sequence,
            raw.projection,
        )
        .map_err(de::Error::custom)
    }
}

impl SessionSnapshotDto {
    /// Creates a session snapshot containing a coherent state projection.
    ///
    /// # Errors
    ///
    /// Returns a validation error when projection session identity or durable
    /// sequence differs from this snapshot checkpoint.
    pub fn with_projection(
        schema_version: SchemaVersionDto,
        session_id: SessionId,
        at_sequence: SessionEventSequenceDto,
        projection: SessionProjectionDto,
    ) -> DtoResult<Self> {
        if projection.session_id() != session_id || projection.at_sequence() != at_sequence {
            return Err(ErrorDto::validation(
                "invalid_session_snapshot_projection",
                "snapshot projection must share the snapshot session and sequence",
            ));
        }
        Ok(Self {
            schema_version,
            session_id,
            at_sequence,
            projection,
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
    /// Returns the durable event sequence included by the snapshot.
    #[must_use]
    pub const fn at_sequence(&self) -> SessionEventSequenceDto {
        self.at_sequence
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
}

/// A validated, ordered event tail for one durable session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionEventTailBatchDto {
    schema_version: SchemaVersionDto,
    session_id: SessionId,
    after_sequence: SessionEventSequenceDto,
    events: Vec<EventEnvelopeDto<DomainEventDto>>,
}

impl<'de> Deserialize<'de> for SessionEventTailBatchDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawSessionEventTailBatchDto {
            schema_version: SchemaVersionDto,
            session_id: SessionId,
            after_sequence: SessionEventSequenceDto,
            events: Vec<EventEnvelopeDto<DomainEventDto>>,
        }

        let raw = RawSessionEventTailBatchDto::deserialize(deserializer)?;
        Self::new(
            raw.schema_version,
            raw.session_id,
            raw.after_sequence,
            raw.events,
        )
        .map_err(de::Error::custom)
    }
}

impl SessionEventTailBatchDto {
    /// Creates a contiguous ordered event tail after a known durable position.
    ///
    /// # Errors
    ///
    /// Returns a validation error when an event belongs to another session or the
    /// tail is not contiguous from `after_sequence`.
    pub fn new(
        schema_version: SchemaVersionDto,
        session_id: SessionId,
        after_sequence: SessionEventSequenceDto,
        events: Vec<EventEnvelopeDto<DomainEventDto>>,
    ) -> DtoResult<Self> {
        let mut expected = after_sequence.value();
        for event in &events {
            expected = expected.checked_add(1).ok_or_else(|| {
                ErrorDto::validation(
                    "invalid_event_tail",
                    "event tail cannot follow the maximum sequence position",
                )
            })?;
            if event.session_id() != session_id || event.sequence().value() != expected {
                return Err(ErrorDto::validation(
                    "invalid_event_tail",
                    "event tail must be contiguous and scoped to its session",
                ));
            }
        }
        Ok(Self {
            schema_version,
            session_id,
            after_sequence,
            events,
        })
    }

    /// Returns the tail schema version.
    #[must_use]
    pub const fn schema_version(&self) -> SchemaVersionDto {
        self.schema_version
    }

    /// Returns the session to which every event in the tail belongs.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the durable position immediately preceding the first event.
    #[must_use]
    pub const fn after_sequence(&self) -> SessionEventSequenceDto {
        self.after_sequence
    }

    /// Returns the validated ordered events in this tail batch.
    #[must_use]
    pub fn events(&self) -> &[EventEnvelopeDto<DomainEventDto>] {
        &self.events
    }

    /// Returns the durable position after the last event in this batch.
    #[must_use]
    pub fn next_after_sequence(&self) -> SessionEventSequenceDto {
        self.events
            .last()
            .map_or(self.after_sequence, EventEnvelopeDto::sequence)
    }
}

/// The reviewed reason why a subscriber must obtain a fresh snapshot.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionResyncReasonDto {
    /// The requested event history is no longer available for replay.
    HistoryUnavailable,
    /// The request did not identify a usable contiguous event position.
    InvalidPosition,
}

/// A typed instruction to discard local subscription state and resynchronize.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionResyncDto {
    schema_version: SchemaVersionDto,
    session_id: SessionId,
    reason: SessionResyncReasonDto,
}

impl SessionResyncDto {
    /// Creates a safe, typed session resynchronization instruction.
    #[must_use]
    pub const fn new(
        schema_version: SchemaVersionDto,
        session_id: SessionId,
        reason: SessionResyncReasonDto,
    ) -> Self {
        Self {
            schema_version,
            session_id,
            reason,
        }
    }

    /// Returns the resynchronization schema version.
    #[must_use]
    pub const fn schema_version(self) -> SchemaVersionDto {
        self.schema_version
    }

    /// Returns the session that must be resynchronized.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }

    /// Returns the reviewed typed reason for resynchronization.
    #[must_use]
    pub const fn reason(self) -> SessionResyncReasonDto {
        self.reason
    }
}

/// A subscription response containing either a consistent snapshot and tail or a resync instruction.
///
/// `SnapshotAndTail` deliberately retains value fields: boxing either field
/// would change the public Rust DTO contract while providing no wire-format
/// benefit, because serde already serializes the same tagged value.
#[expect(
    clippy::large_enum_variant,
    reason = "SnapshotAndTail is an established public DTO variant; boxing its value fields would break Rust consumers without changing the serde wire format."
)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum SessionSubscriptionResponseDto {
    /// A snapshot checkpoint and its contiguous tail from that checkpoint.
    SnapshotAndTail {
        /// The consistent session checkpoint.
        snapshot: SessionSnapshotDto,
        /// Events immediately following the snapshot checkpoint.
        tail: SessionEventTailBatchDto,
    },
    /// The subscriber must discard local state and request a new snapshot.
    ResyncRequired(SessionResyncDto),
}

impl<'de> Deserialize<'de> for SessionSubscriptionResponseDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", content = "data", rename_all = "snake_case")]
        #[expect(
            clippy::large_enum_variant,
            reason = "The private deserialization intermediary mirrors the public unboxed DTO so serde can validate snapshot and tail coherence after decoding."
        )]
        enum RawSessionSubscriptionResponseDto {
            SnapshotAndTail {
                snapshot: SessionSnapshotDto,
                tail: SessionEventTailBatchDto,
            },
            ResyncRequired(SessionResyncDto),
        }

        match RawSessionSubscriptionResponseDto::deserialize(deserializer)? {
            RawSessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail } => {
                Self::snapshot_and_tail(snapshot, tail).map_err(de::Error::custom)
            }
            RawSessionSubscriptionResponseDto::ResyncRequired(resync) => {
                Ok(Self::ResyncRequired(resync))
            }
        }
    }
}

impl SessionSubscriptionResponseDto {
    /// Creates a consistent snapshot and tail response.
    ///
    /// # Errors
    ///
    /// Returns a validation error unless the tail starts exactly at the snapshot
    /// position and both values identify the same session.
    pub fn snapshot_and_tail(
        snapshot: SessionSnapshotDto,
        tail: SessionEventTailBatchDto,
    ) -> DtoResult<Self> {
        let session_matches = snapshot.session_id() == tail.session_id();
        let position_matches = snapshot.at_sequence() == tail.after_sequence();
        if !(session_matches && position_matches) {
            return Err(ErrorDto::validation(
                "invalid_subscription_response",
                "snapshot and tail must share one session and event position",
            ));
        }
        Ok(Self::SnapshotAndTail { snapshot, tail })
    }

    /// Creates a typed subscription resynchronization response.
    #[must_use]
    pub const fn resync_required(resync: SessionResyncDto) -> Self {
        Self::ResyncRequired(resync)
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
    /// A paged provider catalog projection (control plane).
    ProviderCatalog(ProviderCatalogPageDto),
    /// The provider catalog activation and degradation status.
    ProviderCatalogStatus(ProviderCatalogStatusDto),
    /// The durable provider profile projection of one session.
    SessionProviderProfile(SessionProviderProfileDto),
    /// A provider usage aggregation for one period.
    ProviderUsage(UsageAggregationDto),
    /// Non-authorizing provider health evidence (control plane).
    ProviderHealthEvidence(ProviderHealthProjectionDto),
    /// The status of one provider discovery attempt (control plane).
    ProviderDiscoveryStatus(ProviderDiscoveryProjectionDto),
    /// The safe non-authorizing pricing policy projection (control plane).
    PricingPolicy(PricingProjectionDto),
    /// The safe applied configuration projection (control plane).
    ConfigurationProjection(ConfigurationProjectionDto),
    /// The query was safely rejected before execution.
    Rejected(ErrorDto),
}

/// A request payload carried in a correlated wire message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProtocolRequestPayloadDto {
    /// A daemon command.
    Command(ProtocolCommandDto),
    /// A daemon query.
    Query(ProtocolQueryDto),
}

/// A response payload carried in a correlated wire message.
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

/// A correlated initial run-stream subscription request envelope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunSubscriptionRequestEnvelopeDto {
    protocol_version: ProtocolVersionDto,
    correlation_id: CorrelationIdDto,
    message: ProtocolMessageDto<SubscribeRunCommandDto>,
}

impl RunSubscriptionRequestEnvelopeDto {
    /// Creates a correlated, schema-versioned run subscription request.
    #[must_use]
    pub const fn new(
        protocol_version: ProtocolVersionDto,
        correlation_id: CorrelationIdDto,
        message: ProtocolMessageDto<SubscribeRunCommandDto>,
    ) -> Self {
        Self {
            protocol_version,
            correlation_id,
            message,
        }
    }

    /// Returns the negotiated protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> ProtocolVersionDto {
        self.protocol_version
    }
    /// Returns the request correlation identity.
    #[must_use]
    pub const fn correlation_id(&self) -> CorrelationIdDto {
        self.correlation_id
    }
    /// Returns the versioned subscription command.
    #[must_use]
    pub const fn message(&self) -> &ProtocolMessageDto<SubscribeRunCommandDto> {
        &self.message
    }
}

/// A daemon-to-client frame that distinguishes correlated replies from stream frames.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProtocolDaemonFrameDto {
    /// A correlation-bound response envelope.
    Response(ProtocolResponseEnvelopeDto),
    /// An uncorrelated run-stream frame.
    RunStream(RunStreamFrameDto),
}

/// A schema-versioned typed message payload suitable for a local wire codec.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolMessageDto<T> {
    schema_version: SchemaVersionDto,
    payload: T,
}

impl<T> ProtocolMessageDto<T> {
    /// Creates a versioned typed protocol message.
    #[must_use]
    pub const fn new(schema_version: SchemaVersionDto, payload: T) -> Self {
        Self {
            schema_version,
            payload,
        }
    }

    /// Returns the message schema version.
    #[must_use]
    pub const fn schema_version(&self) -> SchemaVersionDto {
        self.schema_version
    }

    /// Returns the typed message payload.
    #[must_use]
    pub const fn payload(&self) -> &T {
        &self.payload
    }

    /// Consumes the message and returns its typed payload.
    #[must_use]
    pub fn into_payload(self) -> T {
        self.payload
    }
}

/// A correlated client-to-daemon local protocol request envelope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolRequestEnvelopeDto {
    protocol_version: ProtocolVersionDto,
    correlation_id: CorrelationIdDto,
    message: ProtocolMessageDto<ProtocolRequestPayloadDto>,
}

impl ProtocolRequestEnvelopeDto {
    /// Creates a versioned request envelope with a canonical correlation ID.
    #[must_use]
    pub const fn new(
        protocol_version: ProtocolVersionDto,
        correlation_id: CorrelationIdDto,
        message: ProtocolMessageDto<ProtocolRequestPayloadDto>,
    ) -> Self {
        Self {
            protocol_version,
            correlation_id,
            message,
        }
    }

    /// Returns the negotiated wire protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> ProtocolVersionDto {
        self.protocol_version
    }

    /// Returns the request-response correlation identifier.
    #[must_use]
    pub const fn correlation_id(&self) -> CorrelationIdDto {
        self.correlation_id
    }

    /// Returns the schema-versioned request message.
    #[must_use]
    pub const fn message(&self) -> &ProtocolMessageDto<ProtocolRequestPayloadDto> {
        &self.message
    }
}

/// A correlated daemon-to-client local protocol response envelope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolResponseEnvelopeDto {
    protocol_version: ProtocolVersionDto,
    correlation_id: CorrelationIdDto,
    message: ProtocolMessageDto<ProtocolResponsePayloadDto>,
}

impl ProtocolResponseEnvelopeDto {
    /// Creates a versioned response envelope echoing a request correlation ID.
    #[must_use]
    pub const fn new(
        protocol_version: ProtocolVersionDto,
        correlation_id: CorrelationIdDto,
        message: ProtocolMessageDto<ProtocolResponsePayloadDto>,
    ) -> Self {
        Self {
            protocol_version,
            correlation_id,
            message,
        }
    }

    /// Returns the negotiated wire protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> ProtocolVersionDto {
        self.protocol_version
    }

    /// Returns the echoed request-response correlation identifier.
    #[must_use]
    pub const fn correlation_id(&self) -> CorrelationIdDto {
        self.correlation_id
    }

    /// Returns the schema-versioned response message.
    #[must_use]
    pub const fn message(&self) -> &ProtocolMessageDto<ProtocolResponsePayloadDto> {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;
    use intention_types::{EventId, EventMetadataDto, TimestampDto, TurnId};

    fn fixture_workspace_root() -> intention_domain::WorkspaceRootDto {
        intention_domain::WorkspaceRootDto::parse(
            std::env::temp_dir()
                .join("intention-protocol-unit-workspace")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture workspace root is valid")
    }

    fn fixture_projection(
        session_id: SessionId,
        at_sequence: SessionEventSequenceDto,
    ) -> SessionProjectionDto {
        SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            fixture_workspace_root(),
            RunModeDto::Build,
            None,
            None,
            Vec::new(),
            at_sequence,
        )
        .expect("fixture projection is valid")
    }

    fn fixture_event(session_id: SessionId, sequence: u64) -> EventEnvelopeDto<DomainEventDto> {
        let occurred_at = TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid");
        EventEnvelopeDto::new(
            EventMetadataDto::new(
                SchemaVersionDto::new(1, 1),
                EventId::new(),
                session_id,
                None,
                None,
                SessionEventSequenceDto::new(sequence),
                occurred_at,
            ),
            DomainEventDto::RunStatusChanged(intention_domain::RunStatusChangedEventDto::new(
                session_id,
                RunId::new(),
                intention_domain::RunStatusDto::Running,
                occurred_at,
            )),
        )
    }

    #[test]
    fn protocol_versions_and_hello_validate_all_paths() {
        let version = ProtocolVersionDto::new(1, 2);
        assert_eq!(version.major(), 1);
        assert_eq!(version.minor(), 2);
        // Negotiation accepts only the exact current version: a differing
        // minor is rejected exactly like a differing major (no same-major
        // tolerance remains).
        assert_ne!(version, crate::CURRENT_PROTOCOL_VERSION);
        assert_ne!(
            ProtocolVersionDto::new(2, 0),
            crate::CURRENT_PROTOCOL_VERSION
        );
        let hello = ProtocolHelloDto::new(
            version,
            vec![ProtocolCapabilityDto::SessionSubscriptions],
            "fixture",
        )
        .expect("fixture hello is valid");
        assert_eq!(hello.version(), version);
        assert_eq!(hello.adapter_name(), "fixture");
        assert_eq!(
            ProtocolHelloDto::new(version, Vec::new(), " ")
                .expect_err("blank adapter must fail")
                .code(),
            "invalid_adapter_name"
        );
    }

    #[test]
    fn current_versions_are_protocol_and_dto_schema_1_1() {
        assert_eq!(CURRENT_PROTOCOL_VERSION, ProtocolVersionDto::new(1, 1));
        assert_eq!(CURRENT_DTO_SCHEMA_VERSION, SchemaVersionDto::new(1, 1));
    }

    #[test]
    fn protocol_wrappers_and_results_preserve_domain_dtos() {
        let session_id = SessionId::new();
        let subscription = SubscribeSessionCommandDto::with_run_id(
            SchemaVersionDto::new(1, 1),
            session_id,
            Some(RunId::new()),
            Some(SessionEventSequenceDto::new(4)),
            RunModeDto::Plan,
        );
        assert_eq!(subscription.session_id(), session_id);
        assert!(subscription.run_id().is_some());
        assert_eq!(subscription.requested_mode(), RunModeDto::Plan);

        let commands = [
            ProtocolCommandDto::SendUserTurn(
                SendUserTurnCommandDto::new(session_id, TurnId::new(), "hello")
                    .expect("fixture turn is valid"),
            ),
            ProtocolCommandDto::StopRun(StopRunCommandDto::new(session_id, RunId::new())),
            ProtocolCommandDto::SubscribeSession(subscription),
        ];
        for command in commands {
            let encoded = serde_json::to_string(&command).expect("command serialization succeeds");
            let _: ProtocolCommandDto =
                serde_json::from_str(&encoded).expect("command parsing succeeds");
        }
        let accepted = ProtocolAcceptedDto::with_result(
            CorrelationIdDto::new(),
            ProtocolAcceptedResultDto::StopRun(StopRunAcceptedDto::new(
                session_id,
                RunId::new(),
                SessionEventSequenceDto::new(1),
            )),
        );
        assert_eq!(accepted.correlation_id(), accepted.correlation_id());
        let result = ProtocolCommandResultDto::Accepted(accepted);
        let encoded = serde_json::to_string(&result).expect("result serialization succeeds");
        let _: ProtocolCommandResultDto =
            serde_json::from_str(&encoded).expect("result parsing succeeds");
    }

    #[test]
    fn tails_and_subscription_responses_validate_continuity() {
        let schema = SchemaVersionDto::new(1, 1);
        let session_id = SessionId::new();
        let snapshot = SessionSnapshotDto::with_projection(
            schema,
            session_id,
            SessionEventSequenceDto::new(2),
            fixture_projection(session_id, SessionEventSequenceDto::new(2)),
        )
        .expect("fixture snapshot is valid");
        let tail = SessionEventTailBatchDto::new(
            schema,
            session_id,
            snapshot.at_sequence(),
            vec![fixture_event(session_id, 3)],
        )
        .expect("contiguous tail is valid");
        assert_eq!(tail.next_after_sequence(), SessionEventSequenceDto::new(3));
        assert!(SessionSubscriptionResponseDto::snapshot_and_tail(snapshot, tail).is_ok());
        assert!(
            SessionEventTailBatchDto::new(
                schema,
                session_id,
                SessionEventSequenceDto::new(2),
                vec![fixture_event(session_id, 4)],
            )
            .is_err()
        );
    }

    #[test]
    fn all_protocol_accessors_and_envelope_variants_are_exercised() {
        let schema = SchemaVersionDto::new(1, 1);
        let version = ProtocolVersionDto::new(3, 4);
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let cursor = RunEventCursorDto::new(9);
        let run_sub = SubscribeRunCommandDto::new(schema, session_id, run_id, Some(cursor));
        assert_eq!(run_sub.schema_version(), schema);
        assert_eq!(run_sub.session_id(), session_id);
        assert_eq!(run_sub.run_id(), run_id);
        assert_eq!(run_sub.after_cursor(), Some(cursor));

        let resync = RunResyncDto::new(session_id, run_id, RunResyncReasonDto::CursorGap);
        assert_eq!(resync.session_id(), session_id);
        assert_eq!(resync.run_id(), run_id);
        assert_eq!(resync.reason(), RunResyncReasonDto::CursorGap);

        let frame = RunStreamFrameDto::Resync(resync);
        let response = RunSubscriptionResponseDto::Resync(resync);
        let request = RunSubscriptionRequestEnvelopeDto::new(
            version,
            CorrelationIdDto::new(),
            ProtocolMessageDto::new(schema, run_sub),
        );
        assert_eq!(request.protocol_version(), version);
        assert!(request.correlation_id() == request.correlation_id());
        assert_eq!(request.message().schema_version(), schema);
        assert_eq!(request.message().payload().run_id(), run_id);
        let decoded_frame: RunStreamFrameDto =
            serde_json::from_value(serde_json::to_value(frame).expect("frame serializes"))
                .expect("frame decodes");
        let decoded_response: RunSubscriptionResponseDto =
            serde_json::from_value(serde_json::to_value(response).expect("response serializes"))
                .expect("response decodes");
        assert!(matches!(decoded_frame, RunStreamFrameDto::Resync(_)));
        assert!(matches!(
            decoded_response,
            RunSubscriptionResponseDto::Resync(_)
        ));

        let query = ProtocolQueryDto::GetDaemonHealth;
        let response_payload =
            ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Resync(resync));
        let envelope = ProtocolResponseEnvelopeDto::new(
            version,
            CorrelationIdDto::new(),
            ProtocolMessageDto::new(schema, response_payload),
        );
        assert_eq!(envelope.protocol_version(), version);
        assert_eq!(envelope.message().schema_version(), schema);
        assert!(matches!(query, ProtocolQueryDto::GetDaemonHealth));
        let _ = envelope.message().payload();
    }

    #[test]
    fn protocol_capabilities_readiness_and_acceptance_accessors_cover_all_variants() {
        let version = ProtocolVersionDto::new(1, 1);
        let capabilities = [
            ProtocolCapabilityDto::SessionSubscriptions,
            ProtocolCapabilityDto::CorrelatedRequests,
            ProtocolCapabilityDto::DaemonHealth,
            ProtocolCapabilityDto::RunStreamSubscriptions,
        ];
        let hello =
            ProtocolHelloDto::new(version, capabilities.to_vec(), "adapter").expect("valid hello");
        assert_eq!(hello.capabilities(), capabilities);

        for readiness in [
            DaemonReadinessDto::Starting,
            DaemonReadinessDto::Ready,
            DaemonReadinessDto::Draining,
            DaemonReadinessDto::Unavailable,
        ] {
            assert_eq!(
                DaemonHealthDto::new(SchemaVersionDto::new(1, 1), version, readiness).readiness(),
                readiness
            );
        }

        let session = SessionId::new();
        let turn = TurnId::new();
        let started = SendUserTurnAcceptedDto::new(
            session,
            turn,
            SessionEventSequenceDto::new(1),
            SendUserTurnOutcomeDto::Started {
                run_id: RunId::new(),
                config_revision_id: ConfigRevisionId::new(),
            },
        );
        let queued = SendUserTurnAcceptedDto::new(
            session,
            turn,
            SessionEventSequenceDto::new(2),
            SendUserTurnOutcomeDto::Queued {
                queue_position: QueuePositionDto::new(1),
            },
        );
        assert_eq!(started.session_id(), session);
        assert_eq!(started.turn_id(), turn);
        assert_eq!(
            started.committed_sequence(),
            SessionEventSequenceDto::new(1)
        );
        assert!(matches!(
            started.outcome(),
            SendUserTurnOutcomeDto::Started { .. }
        ));
        assert!(matches!(
            queued.outcome(),
            SendUserTurnOutcomeDto::Queued { .. }
        ));
    }

    #[test]
    fn acceptance_evidence_and_payload_accessors_preserve_values() {
        let session = SessionId::new();
        let run = RunId::new();
        let project = ProjectId::new();
        let workspace = WorkspaceId::new();
        let seq = SessionEventSequenceDto::new(7);
        let created = CreateSessionAcceptedDto::new(project, workspace, session, seq);
        assert_eq!(created.project_id(), project);
        assert_eq!(created.workspace_id(), workspace);
        assert_eq!(created.session_id(), session);
        assert_eq!(created.committed_sequence(), seq);
        let removed = RemoveQueuedTurnAcceptedDto::new(session, TurnId::new(), seq);
        assert_eq!(removed.session_id(), session);
        assert_eq!(removed.committed_sequence(), seq);
        let stopped = StopRunAcceptedDto::new(session, run, seq);
        assert_eq!(stopped.session_id(), session);
        assert_eq!(stopped.run_id(), run);
        assert_eq!(stopped.committed_sequence(), seq);

        let correlation = CorrelationIdDto::new();
        let accepted = ProtocolAcceptedDto::with_result(
            correlation,
            ProtocolAcceptedResultDto::CreateSession(created),
        );
        assert_eq!(accepted.correlation_id(), correlation);
        assert!(matches!(
            accepted.result(),
            ProtocolAcceptedResultDto::CreateSession(_)
        ));
    }

    #[test]
    fn protocol_round_trip_covers_all_closed_enum_shapes() {
        let session = SessionId::new();
        let run = RunId::new();
        let schema = SchemaVersionDto::new(1, 1);
        let commands = vec![
            ProtocolCommandDto::CreateSession(CreateSessionCommandDto::new(
                ProjectId::new(),
                session,
                WorkspaceId::new(),
                intention_domain::WorkspaceRootDto::parse(
                    std::env::temp_dir()
                        .join("intention-protocol-workspace")
                        .to_string_lossy(),
                )
                .expect("root"),
                RunModeDto::Build,
            )),
            ProtocolCommandDto::RemoveQueuedTurn(RemoveQueuedTurnCommandDto::new(
                session,
                TurnId::new(),
            )),
        ];
        for value in commands {
            let wire = serde_json::to_vec(&value).expect("command encodes");
            assert_eq!(
                serde_json::from_slice::<ProtocolCommandDto>(&wire).expect("command decodes"),
                value
            );
        }
        for value in [
            ProtocolQueryDto::GetDaemonHealth,
            ProtocolQueryDto::GetSessionSnapshot(GetSessionSnapshotQueryDto::new(session)),
        ] {
            let wire = serde_json::to_vec(&value).expect("query encodes");
            assert_eq!(
                serde_json::from_slice::<ProtocolQueryDto>(&wire).expect("query decodes"),
                value
            );
        }
        let error = ErrorDto::validation("rejected", "rejected");
        for value in [
            ProtocolCommandResultDto::Rejected(error.clone()),
            ProtocolCommandResultDto::Accepted(ProtocolAcceptedDto::with_result(
                CorrelationIdDto::new(),
                ProtocolAcceptedResultDto::StopRun(StopRunAcceptedDto::new(
                    session,
                    run,
                    SessionEventSequenceDto::new(1),
                )),
            )),
        ] {
            let wire = serde_json::to_vec(&value).expect("result encodes");
            let _: ProtocolCommandResultDto =
                serde_json::from_slice(&wire).expect("result decodes");
        }
        for value in [
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Rejected(
                error.clone(),
            )),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(
                error.clone(),
            )),
            ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Error(error)),
        ] {
            let wire = serde_json::to_vec(&value).expect("payload encodes");
            let _: ProtocolResponsePayloadDto =
                serde_json::from_slice(&wire).expect("payload decodes");
        }
        let _ = (schema, run);
    }

    #[test]
    fn remaining_constructor_and_deserialization_error_paths_are_checked() {
        let schema = SchemaVersionDto::new(1, 1);
        let session = SessionId::new();
        let run = RunId::new();
        let bad_tail = serde_json::json!({
            "schema_version": {"major": 1, "minor": 1},
            "session_id": session,
            "after_sequence": 4,
            "events": [{
                "metadata": {
                    "schema_version": {"major": 1, "minor": 1},
                    "event_id": EventId::new(), "session_id": SessionId::new(),
                    "run_id": null, "turn_id": null, "sequence": 5,
                    "occurred_at": 1
                },
                "payload": {"kind": "run_status_changed", "data": {
                    "session_id": session, "run_id": run, "status": "running", "occurred_at": 1
                }}
            }]
        });
        assert!(serde_json::from_value::<SessionEventTailBatchDto>(bad_tail).is_err());

        let snapshot = SessionSnapshotDto::with_projection(
            schema,
            session,
            SessionEventSequenceDto::new(1),
            fixture_projection(session, SessionEventSequenceDto::new(1)),
        )
        .expect("fixture snapshot is valid");
        let tail = SessionEventTailBatchDto::new(
            schema,
            SessionId::new(),
            snapshot.at_sequence(),
            Vec::new(),
        )
        .expect("empty tail fixture");
        assert!(SessionSubscriptionResponseDto::snapshot_and_tail(snapshot, tail).is_err());

        let batch = RunLiveBatchDto::new(
            session,
            run,
            RunEventCursorDto::new(u64::MAX),
            vec![],
            RunEventCursorDto::new(u64::MAX),
        );
        assert!(batch.is_err());
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

    #[test]
    fn control_plane_command_variants_round_trip_through_wire_envelopes() {
        use crate::contract_families::{
            AcceptProviderCatalogRemovalCommandDto, AdmitRecoveredRunCommandDto,
            ConfigurationEditCommandDto, ConfigurationEditOperationDto, RawTomlEditCommandDto,
            ReconcileUnavailableQueueCommandDto, RejectProviderCatalogCandidateCommandDto,
            ReloadConfigurationCommandDto, RotateProviderCredentialsCommandDto,
            SetSessionProviderProfileCommandDto,
        };
        let commands = vec![
            ProtocolCommandDto::SetSessionProviderProfile(SetSessionProviderProfileCommandDto {
                schema_version: "1.1".to_owned(),
                session_id: "session-1".to_owned(),
                profile_id: "profile-1".to_owned(),
                expected_session_projection_revision: 7,
                operation_id: "operation-1".to_owned(),
            }),
            ProtocolCommandDto::AcceptProviderCatalogRemoval(
                AcceptProviderCatalogRemovalCommandDto {
                    candidate_handle: "candidate-1".to_owned(),
                    expected_active_catalog_revision_id: "catalog-rev-1".to_owned(),
                    expected_candidate_catalog_revision_id: "catalog-rev-2".to_owned(),
                    operation_id: "operation-1".to_owned(),
                    source_recheck: true,
                },
            ),
            ProtocolCommandDto::RejectProviderCatalogCandidate(
                RejectProviderCatalogCandidateCommandDto {
                    candidate_handle: "candidate-1".to_owned(),
                    expected_active_catalog_revision_id: "catalog-rev-1".to_owned(),
                    operation_id: "operation-1".to_owned(),
                },
            ),
            ProtocolCommandDto::ReconcileUnavailableQueue(ReconcileUnavailableQueueCommandDto {
                session_id: "session-1".to_owned(),
                operation_id: "operation-1".to_owned(),
                page_cursor: None,
            }),
            ProtocolCommandDto::AdmitRecoveredRun(AdmitRecoveredRunCommandDto {
                session_id: "session-1".to_owned(),
                run_id: "run-1".to_owned(),
                operation_id: "operation-1".to_owned(),
            }),
            ProtocolCommandDto::ReloadConfiguration(ReloadConfigurationCommandDto {
                candidate_snapshot_reference: Some("snapshot-1".to_owned()),
                candidate_edit_reference: None,
                expected_active_config_revision: "config-rev-1".to_owned(),
                operation_id: "operation-1".to_owned(),
                origin: crate::contract_families::ConfigurationOriginDto::Admin,
            }),
            ProtocolCommandDto::RotateProviderCredentials(RotateProviderCredentialsCommandDto {
                profile_id: "profile-1".to_owned(),
                provider_profile_revision_id: "rev-1".to_owned(),
                expected_credential_composition_revision: "composition-1".to_owned(),
                operation_id: "operation-1".to_owned(),
            }),
            ProtocolCommandDto::SubmitRawTomlEdit(RawTomlEditCommandDto {
                operation_id: "operation-1".to_owned(),
                expected_config_revision: "config-rev-1".to_owned(),
                candidate_content: "[daemon]\nmax_parallel_runs = 2\n".to_owned(),
            }),
            ProtocolCommandDto::ApplyConfigurationEdit(ConfigurationEditCommandDto {
                operation_id: "operation-1".to_owned(),
                expected_config_revision: "config-rev-1".to_owned(),
                operations: vec![ConfigurationEditOperationDto::Set {
                    key_path: "daemon.max_parallel_runs".to_owned(),
                    safe_value: "2".to_owned(),
                }],
            }),
        ];
        let schema = SchemaVersionDto::new(1, 1);
        let version = ProtocolVersionDto::new(1, 1);
        for command in commands {
            let envelope = ProtocolRequestEnvelopeDto::new(
                version,
                CorrelationIdDto::new(),
                ProtocolMessageDto::new(schema, ProtocolRequestPayloadDto::Command(command)),
            );
            let wire = serde_json::to_vec(&envelope).expect("command envelope encodes");
            let decoded: ProtocolRequestEnvelopeDto =
                serde_json::from_slice(&wire).expect("command envelope decodes");
            assert_eq!(decoded, envelope);
        }
    }

    #[test]
    fn control_plane_query_and_result_variants_round_trip_through_wire_payloads() {
        use crate::contract_families::{
            AcceptProviderCatalogRemovalAcceptedDto, AdmitRecoveredRunAcceptedDto,
            GetProviderCatalogQueryDto, GetProviderCatalogStatusQueryDto, GetProviderUsageQueryDto,
            GetSessionProviderProfileQueryDto, ReconcileUnavailableQueueAcceptedDto,
            RejectProviderCatalogCandidateAcceptedDto, ReloadTransactionDto,
            ResolvedProviderProfileDto, SetSessionProviderProfileAcceptedDto, UsageAggregationDto,
        };
        let queries = vec![
            ProtocolQueryDto::GetProviderCatalog(GetProviderCatalogQueryDto {
                schema_version: "1.1".to_owned(),
                page_token: None,
                expected_catalog_revision_id: None,
            }),
            ProtocolQueryDto::GetProviderCatalogStatus(GetProviderCatalogStatusQueryDto {
                schema_version: "1.1".to_owned(),
            }),
            ProtocolQueryDto::GetSessionProviderProfile(GetSessionProviderProfileQueryDto {
                schema_version: "1.1".to_owned(),
                session_id: "session-1".to_owned(),
            }),
            ProtocolQueryDto::GetProviderUsage(GetProviderUsageQueryDto {
                schema_version: "1.1".to_owned(),
                profile_id: "profile-1".to_owned(),
                usage_period_start: 100,
                usage_period_end: 200,
            }),
        ];
        for query in queries {
            let wire = serde_json::to_vec(&query).expect("query encodes");
            assert_eq!(
                serde_json::from_slice::<ProtocolQueryDto>(&wire).expect("query decodes"),
                query
            );
        }

        let accepted_results = vec![
            ProtocolAcceptedResultDto::SetSessionProviderProfile(
                SetSessionProviderProfileAcceptedDto {
                    session_id: "session-1".to_owned(),
                    changed: true,
                    resulting_projection_revision: 8,
                    resolved: ResolvedProviderProfileDto::Resolved {
                        profile_id: "profile-1".to_owned(),
                        profile_revision_id: "rev-1".to_owned(),
                    },
                },
            ),
            ProtocolAcceptedResultDto::AcceptProviderCatalogRemoval(
                AcceptProviderCatalogRemovalAcceptedDto {
                    candidate_handle: "candidate-1".to_owned(),
                    active_catalog_revision_id: "catalog-rev-1".to_owned(),
                },
            ),
            ProtocolAcceptedResultDto::RejectProviderCatalogCandidate(
                RejectProviderCatalogCandidateAcceptedDto {
                    candidate_handle: "candidate-1".to_owned(),
                },
            ),
            ProtocolAcceptedResultDto::ReconcileUnavailableQueue(
                ReconcileUnavailableQueueAcceptedDto {
                    session_id: "session-1".to_owned(),
                    page_cursor: None,
                    promoted_count: 2,
                },
            ),
            ProtocolAcceptedResultDto::AdmitRecoveredRun(AdmitRecoveredRunAcceptedDto {
                session_id: "session-1".to_owned(),
                run_id: "run-1".to_owned(),
            }),
            ProtocolAcceptedResultDto::ReloadConfiguration(ReloadTransactionDto {
                transaction_id: "transaction-1".to_owned(),
                previous_config_revision: "config-rev-1".to_owned(),
                candidate_config_revision: "config-rev-2".to_owned(),
                validation_result:
                    crate::contract_families::ConfigurationValidationOutcomeDto::Valid,
                commit_outcome: crate::contract_families::ConfigurationCommitOutcomeDto::Committed,
                safe_failure_code: None,
                safe_failure_detail: None,
            }),
            ProtocolAcceptedResultDto::RotateProviderCredentials(
                crate::contract_families::CredentialRotationResultDto {
                    operation_id: "operation-1".to_owned(),
                    profile_id: "profile-1".to_owned(),
                    safe_credential_composition_revision: "composition-2".to_owned(),
                    rotated: true,
                },
            ),
        ];
        for result in accepted_results {
            let accepted = ProtocolAcceptedDto::with_result(CorrelationIdDto::new(), result);
            let wire = serde_json::to_vec(&accepted).expect("accepted result encodes");
            let decoded: ProtocolAcceptedDto =
                serde_json::from_slice(&wire).expect("accepted result decodes");
            assert_eq!(decoded, accepted);
        }

        let query_results = vec![
            ProtocolQueryResultDto::ProviderCatalog(
                crate::contract_families::ProviderCatalogPageDto {
                    schema_version: "1.1".to_owned(),
                    catalog_revision_id: "catalog-rev-1".to_owned(),
                    entries: Vec::new(),
                    next_page_token: None,
                    has_more: false,
                },
            ),
            ProtocolQueryResultDto::ProviderCatalogStatus(
                crate::contract_families::ProviderCatalogStatusDto {
                    schema_version: "1.1".to_owned(),
                    activation_state:
                        crate::contract_families::ProviderCatalogActivationState::Active,
                    degraded_reason: None,
                    active_catalog_revision_id: Some("catalog-rev-1".to_owned()),
                    candidate_catalog_revision_id: None,
                    active_default_profile_id: Some("profile-1".to_owned()),
                    removal_impact: None,
                    provider_profiles_negotiated: true,
                },
            ),
            ProtocolQueryResultDto::SessionProviderProfile(
                crate::contract_families::SessionProviderProfileDto {
                    session_id: "session-1".to_owned(),
                    profile_id: "profile-1".to_owned(),
                    resolved: ResolvedProviderProfileDto::Resolved {
                        profile_id: "profile-1".to_owned(),
                        profile_revision_id: "rev-1".to_owned(),
                    },
                    session_projection_revision: 8,
                    global_default_profile_id: "profile-default".to_owned(),
                },
            ),
            ProtocolQueryResultDto::ProviderUsage(UsageAggregationDto {
                profile_id: "profile-1".to_owned(),
                provider_profile_revision_id: "rev-1".to_owned(),
                model_id: "model-1".to_owned(),
                request_count: 12,
                input_units: 1000,
                output_units: 500,
                reasoning_units: 250,
                usage_period_start: 100,
                usage_period_end: 200,
            }),
        ];
        for result in query_results {
            let wire = serde_json::to_vec(&result).expect("query result encodes");
            let decoded: ProtocolQueryResultDto =
                serde_json::from_slice(&wire).expect("query result decodes");
            assert_eq!(decoded, result);
        }
    }

    #[test]
    fn control_plane_variants_use_stable_snake_case_wire_names() {
        use crate::contract_families::SetSessionProviderProfileCommandDto;
        let names = [
            (
                ProtocolCommandDto::SetSessionProviderProfile(
                    SetSessionProviderProfileCommandDto {
                        schema_version: "1.1".to_owned(),
                        session_id: "session-1".to_owned(),
                        profile_id: "profile-1".to_owned(),
                        expected_session_projection_revision: 7,
                        operation_id: "operation-1".to_owned(),
                    },
                ),
                "set_session_provider_profile",
            ),
            (
                ProtocolCommandDto::AcceptProviderCatalogRemoval(
                    crate::contract_families::AcceptProviderCatalogRemovalCommandDto {
                        candidate_handle: "candidate-1".to_owned(),
                        expected_active_catalog_revision_id: "catalog-rev-1".to_owned(),
                        expected_candidate_catalog_revision_id: "catalog-rev-2".to_owned(),
                        operation_id: "operation-1".to_owned(),
                        source_recheck: true,
                    },
                ),
                "accept_provider_catalog_removal",
            ),
            (
                ProtocolCommandDto::RejectProviderCatalogCandidate(
                    crate::contract_families::RejectProviderCatalogCandidateCommandDto {
                        candidate_handle: "candidate-1".to_owned(),
                        expected_active_catalog_revision_id: "catalog-rev-1".to_owned(),
                        operation_id: "operation-1".to_owned(),
                    },
                ),
                "reject_provider_catalog_candidate",
            ),
            (
                ProtocolCommandDto::ReconcileUnavailableQueue(
                    crate::contract_families::ReconcileUnavailableQueueCommandDto {
                        session_id: "session-1".to_owned(),
                        operation_id: "operation-1".to_owned(),
                        page_cursor: None,
                    },
                ),
                "reconcile_unavailable_queue",
            ),
            (
                ProtocolCommandDto::AdmitRecoveredRun(
                    crate::contract_families::AdmitRecoveredRunCommandDto {
                        session_id: "session-1".to_owned(),
                        run_id: "run-1".to_owned(),
                        operation_id: "operation-1".to_owned(),
                    },
                ),
                "admit_recovered_run",
            ),
            (
                ProtocolCommandDto::ReloadConfiguration(
                    crate::contract_families::ReloadConfigurationCommandDto {
                        candidate_snapshot_reference: Some("snapshot-1".to_owned()),
                        candidate_edit_reference: None,
                        expected_active_config_revision: "config-rev-1".to_owned(),
                        operation_id: "operation-1".to_owned(),
                        origin: crate::contract_families::ConfigurationOriginDto::User,
                    },
                ),
                "reload_configuration",
            ),
            (
                ProtocolCommandDto::RotateProviderCredentials(
                    crate::contract_families::RotateProviderCredentialsCommandDto {
                        profile_id: "profile-1".to_owned(),
                        provider_profile_revision_id: "rev-1".to_owned(),
                        expected_credential_composition_revision: "composition-1".to_owned(),
                        operation_id: "operation-1".to_owned(),
                    },
                ),
                "rotate_provider_credentials",
            ),
            (
                ProtocolCommandDto::SubmitRawTomlEdit(
                    crate::contract_families::RawTomlEditCommandDto {
                        operation_id: "operation-1".to_owned(),
                        expected_config_revision: "config-rev-1".to_owned(),
                        candidate_content: "max_parallel_runs = 2".to_owned(),
                    },
                ),
                "submit_raw_toml_edit",
            ),
            (
                ProtocolCommandDto::ApplyConfigurationEdit(
                    crate::contract_families::ConfigurationEditCommandDto {
                        operation_id: "operation-1".to_owned(),
                        expected_config_revision: "config-rev-1".to_owned(),
                        operations: vec![
                            crate::contract_families::ConfigurationEditOperationDto::Remove {
                                key_path: "daemon.max_parallel_runs".to_owned(),
                            },
                        ],
                    },
                ),
                "apply_configuration_edit",
            ),
        ];
        for (command, expected) in names {
            let wire = serde_json::to_string(&command).expect("command serializes");
            assert!(
                wire.contains(&format!("\"kind\":\"{expected}\"")),
                "command wire must carry the {expected} kind, got {wire}"
            );
        }
        for (query, expected) in [
            (
                ProtocolQueryDto::GetProviderCatalog(
                    crate::contract_families::GetProviderCatalogQueryDto {
                        schema_version: "1.1".to_owned(),
                        page_token: None,
                        expected_catalog_revision_id: None,
                    },
                ),
                "get_provider_catalog",
            ),
            (
                ProtocolQueryDto::GetProviderCatalogStatus(
                    crate::contract_families::GetProviderCatalogStatusQueryDto {
                        schema_version: "1.1".to_owned(),
                    },
                ),
                "get_provider_catalog_status",
            ),
            (
                ProtocolQueryDto::GetSessionProviderProfile(
                    crate::contract_families::GetSessionProviderProfileQueryDto {
                        schema_version: "1.1".to_owned(),
                        session_id: "session-1".to_owned(),
                    },
                ),
                "get_session_provider_profile",
            ),
            (
                ProtocolQueryDto::GetProviderUsage(
                    crate::contract_families::GetProviderUsageQueryDto {
                        schema_version: "1.1".to_owned(),
                        profile_id: "profile-1".to_owned(),
                        usage_period_start: 100,
                        usage_period_end: 200,
                    },
                ),
                "get_provider_usage",
            ),
        ] {
            let wire = serde_json::to_string(&query).expect("query serializes");
            assert!(
                wire.contains(&format!("\"kind\":\"{expected}\"")),
                "query wire must carry the {expected} kind, got {wire}"
            );
        }
    }

    #[test]
    fn health_discovery_pricing_and_configuration_projection_wire_surface_round_trips() {
        use crate::contract_families::{
            GetConfigurationProjectionQueryDto, GetPricingPolicyQueryDto,
            GetProviderDiscoveryStatusQueryDto, GetProviderHealthEvidenceQueryDto,
            PricingClassification, PricingObservationDto, PricingProjectionDto,
            ProviderAvailabilityObservation, ProviderDiscoveryPhase,
            ProviderDiscoveryProjectionDto, ProviderHealthEvidenceDto, ProviderHealthProjectionDto,
            ProviderModelDiscoveryRecordDto,
        };
        let queries = vec![
            ProtocolQueryDto::GetProviderHealthEvidence(GetProviderHealthEvidenceQueryDto {
                schema_version: "1.1".to_owned(),
                provider_id: "profile-1".to_owned(),
            }),
            ProtocolQueryDto::GetProviderDiscoveryStatus(GetProviderDiscoveryStatusQueryDto {
                schema_version: "1.1".to_owned(),
                attempt_id: Some("attempt-1".to_owned()),
            }),
            ProtocolQueryDto::GetPricingPolicy(GetPricingPolicyQueryDto {
                schema_version: "1.1".to_owned(),
                model_id: Some("model-1".to_owned()),
            }),
            ProtocolQueryDto::GetConfigurationProjection(GetConfigurationProjectionQueryDto {
                schema_version: "1.1".to_owned(),
            }),
        ];
        for query in queries {
            let wire = serde_json::to_vec(&query).expect("query encodes");
            assert_eq!(
                serde_json::from_slice::<ProtocolQueryDto>(&wire).expect("query decodes"),
                query
            );
        }
        let results = vec![
            ProtocolQueryResultDto::ProviderHealthEvidence(ProviderHealthProjectionDto {
                provider_id: "profile-1".to_owned(),
                observations: vec![ProviderHealthEvidenceDto {
                    profile_id: "profile-1".to_owned(),
                    provider_profile_revision_id: "rev-1".to_owned(),
                    health_attempt_id: "attempt-1".to_owned(),
                    check_contract_revision: "health-check-v1".to_owned(),
                    observed_availability: ProviderAvailabilityObservation::Available,
                    observed_at: 100,
                    failure_category: None,
                    safe_diagnostic_code: None,
                }],
                safe_reason_code: None,
                observed_at: 100,
            }),
            ProtocolQueryResultDto::ProviderDiscoveryStatus(ProviderDiscoveryProjectionDto {
                attempt_id: Some("attempt-1".to_owned()),
                phase: Some(ProviderDiscoveryPhase::Terminal),
                records: vec![ProviderModelDiscoveryRecordDto {
                    discovery_scope: "all".to_owned(),
                    model_id: "gpt-4o".to_owned(),
                    capability_records: vec!["text_input".to_owned()],
                    source_attempt_id: "attempt-1".to_owned(),
                    discovered_at: 100,
                }],
                safe_status: Some("completed".to_owned()),
            }),
            ProtocolQueryResultDto::PricingPolicy(PricingProjectionDto {
                observations: vec![PricingObservationDto {
                    provider_kind_id: "openrouter".to_owned(),
                    model_id: "model-1".to_owned(),
                    bounded_numeric_value: 42,
                    classification: PricingClassification::CapacityObservation,
                    observed_at: 100,
                }],
                policy_classification: Some(PricingClassification::CapacityObservation),
                disclaimer: Some(
                    "pricing observations are non-authorizing and never gate admission".to_owned(),
                ),
            }),
            ProtocolQueryResultDto::ConfigurationProjection(
                crate::contract_families::ConfigurationProjectionDto {
                    schema_version: "1.1".to_owned(),
                    applied_config_revision_id: "config-rev-1".to_owned(),
                    provider_kind: "openrouter".to_owned(),
                    model_id: "model-1".to_owned(),
                    credential_configured: true,
                    provider_execution_policy: "execution-timeout-30-attempts-2".to_owned(),
                    reload_status: "active".to_owned(),
                },
            ),
        ];
        for result in results {
            let wire = serde_json::to_vec(&result).expect("query result encodes");
            assert_eq!(
                serde_json::from_slice::<ProtocolQueryResultDto>(&wire)
                    .expect("query result decodes"),
                result
            );
        }
        for (query, expected) in [
            (
                ProtocolQueryDto::GetProviderHealthEvidence(GetProviderHealthEvidenceQueryDto {
                    schema_version: "1.1".to_owned(),
                    provider_id: "profile-1".to_owned(),
                }),
                "get_provider_health_evidence",
            ),
            (
                ProtocolQueryDto::GetProviderDiscoveryStatus(GetProviderDiscoveryStatusQueryDto {
                    schema_version: "1.1".to_owned(),
                    attempt_id: None,
                }),
                "get_provider_discovery_status",
            ),
            (
                ProtocolQueryDto::GetPricingPolicy(GetPricingPolicyQueryDto {
                    schema_version: "1.1".to_owned(),
                    model_id: None,
                }),
                "get_pricing_policy",
            ),
            (
                ProtocolQueryDto::GetConfigurationProjection(GetConfigurationProjectionQueryDto {
                    schema_version: "1.1".to_owned(),
                }),
                "get_configuration_projection",
            ),
        ] {
            let wire = serde_json::to_string(&query).expect("query serializes");
            assert!(
                wire.contains(&format!("\"kind\":\"{expected}\"")),
                "query wire must carry the {expected} kind, got {wire}"
            );
        }
    }

    #[test]
    fn health_discovery_pricing_and_projection_dtos_validate_fail_closed() {
        use crate::contract_families::{
            ConfigurationProjectionDto, GetPricingPolicyQueryDto,
            GetProviderDiscoveryStatusQueryDto, GetProviderHealthEvidenceQueryDto,
            PricingProjectionDto, ProviderAvailabilityObservation, ProviderDiscoveryProjectionDto,
            ProviderHealthProjectionDto,
        };
        let health = GetProviderHealthEvidenceQueryDto {
            schema_version: "1.1".to_owned(),
            provider_id: "p".repeat(64),
        };
        assert_eq!(
            health
                .validate()
                .expect_err("over-long provider id is rejected")
                .code(),
            "provider_health_invalid"
        );
        let discovery = GetProviderDiscoveryStatusQueryDto {
            schema_version: "1.1".to_owned(),
            attempt_id: Some("sk-test-sweep".to_owned()),
        };
        assert_eq!(
            discovery
                .validate()
                .expect_err("credential-shaped attempt id is rejected")
                .code(),
            "credentials_forbidden"
        );
        let pricing = GetPricingPolicyQueryDto {
            schema_version: "1.1".to_owned(),
            model_id: Some("bearer token".to_owned()),
        };
        assert_eq!(
            pricing
                .validate()
                .expect_err("credential-shaped model id is rejected")
                .code(),
            "credentials_forbidden"
        );
        let projection = ProviderHealthProjectionDto {
            provider_id: "profile-1".to_owned(),
            observations: vec![crate::contract_families::ProviderHealthEvidenceDto {
                profile_id: "profile-1".to_owned(),
                provider_profile_revision_id: "rev-1".to_owned(),
                health_attempt_id: "attempt-1".to_owned(),
                check_contract_revision: "health-check-v1".to_owned(),
                observed_availability: ProviderAvailabilityObservation::Available,
                observed_at: 1,
                failure_category: Some(
                    crate::contract_families::ProviderHealthFailureCategory::RequestTimeout,
                ),
                safe_diagnostic_code: None,
            }],
            safe_reason_code: Some("ok".to_owned()),
            observed_at: 1,
        };
        assert!(
            projection.validate().is_err(),
            "an available observation with a failure category must fail validation"
        );
        let configuration = ConfigurationProjectionDto {
            schema_version: "1.1".to_owned(),
            applied_config_revision_id: "rev-1".to_owned(),
            provider_kind: "openrouter".to_owned(),
            model_id: "sk-test-sweep".to_owned(),
            credential_configured: true,
            provider_execution_policy: "execution-timeout-30-attempts-2".to_owned(),
            reload_status: "active".to_owned(),
        };
        assert_eq!(
            configuration
                .validate()
                .expect_err("credential-shaped model id is rejected")
                .code(),
            "credentials_forbidden"
        );
        let discovery_projection = ProviderDiscoveryProjectionDto {
            attempt_id: Some("attempt-1".to_owned()),
            phase: None,
            records: Vec::new(),
            safe_status: Some("ok".to_owned()),
        };
        assert!(discovery_projection.validate().is_ok());
        let pricing_projection = PricingProjectionDto {
            observations: Vec::new(),
            policy_classification: None,
            disclaimer: Some("safe".to_owned()),
        };
        assert!(pricing_projection.validate().is_ok());
        let _ = (projection, pricing_projection);
    }
}
