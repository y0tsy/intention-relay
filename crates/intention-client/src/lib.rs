//! Shared bootstrap, dispatch, subscription, and reconnect client for local adapters.
//!
//! Adapters use this crate instead of direct daemon, runtime, storage, or
//! transport implementation access. It exposes typed operations over domain
//! identifiers and retains only the committed run projection and transcript
//! rows the daemon reports; daemon authority remains remote.

use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use intention_proto::{
    AcceptProviderCatalogRemovalCommandDto, ApplyConfigurationDocumentCommandDto,
    ApplyConfigurationEditsCommandDto, CheckProviderHealthCommandDto, ClientRequestDto,
    ConfigurationEditAcceptedDto, ConfigurationEditDto, ConfigurationReloadAcceptedDto,
    CreateSessionAcceptedDto, CreateSessionCommandDto, CredentialRotationAcceptedDto,
    DaemonHealthDto, DiscoverProviderModelsCommandDto, DtoResult, ErrorCategoryDto, ErrorDto,
    GetSessionProviderProfileQueryDto, GetSessionSnapshotQueryDto, IdempotencyKey,
    InterruptRunAcceptedDto, InterruptRunCommandDto, ListProviderCatalogQueryDto,
    MessageProjectionDto, ProtocolResultDto, ProviderCatalogCandidateRejectedDto,
    ProviderCatalogPageDto, ProviderCatalogRemovalAcceptedDto, ProviderCatalogStatusDto,
    ProviderDiscoveryResultDto, ProviderHealthEvidenceDto, ProviderProfileId,
    ProviderProfileOverrideDto, RejectProviderCatalogCandidateCommandDto,
    ReloadConfigurationCommandDto, RemoveTurnAcceptedDto, RemoveTurnCommandDto,
    RotateProviderCredentialCommandDto, RunId, RunProjectionDto, RunStatusDto, RunStreamFrameDto,
    RunSubscriptionSnapshotDto, SendUserTurnCommandDto, SendUserTurnOutcomeDto, SessionId,
    SessionProviderProfileProjectionDto, SessionSnapshotDto, SetSessionProviderProfileAcceptedDto,
    SetSessionProviderProfileCommandDto, SubscribeRunCommandDto, TurnId, decode_response,
    encode_request, parse_run_frame,
};
use intention_transport::{
    AsyncLocalClientConnection, AsyncMessageReceiver, AsyncMessageSender, LocalEndpoint,
};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_RETRY: Duration = Duration::from_millis(25);
/// Bounded wait for one correlated command or query reply.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Bounded wait for the correlated first reply to a run-stream subscription.
const STREAM_REPLY_TIMEOUT: Duration = Duration::from_secs(30);
/// The request identity of the single request one connection carries.
const REQUEST_ID: u64 = 1;

/// Launches a daemon process after bootstrap has acquired the startup lock.
pub trait DaemonLauncher: Send + Sync {
    /// Starts one daemon host for `endpoint`.
    ///
    /// # Errors
    ///
    /// Returns only a safe typed launch error. Readiness is verified separately
    /// by `IntentionClient` through a health request on a fresh connection.
    fn launch(&self, endpoint: &LocalEndpoint) -> DtoResult<()>;
}

/// A process launcher for the thin `intention-daemon` binary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessDaemonLauncher {
    program: String,
}

impl ProcessDaemonLauncher {
    /// Configures a non-empty daemon program path or command name.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the configured program is blank.
    pub fn new(program: impl Into<String>) -> DtoResult<Self> {
        let program = program.into();
        if program.trim().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_daemon_program",
                "daemon program must not be empty",
            ));
        }
        Ok(Self { program })
    }
}

impl DaemonLauncher for ProcessDaemonLauncher {
    fn launch(&self, endpoint: &LocalEndpoint) -> DtoResult<()> {
        Command::new(&self.program)
            .arg(endpoint.instance_id())
            .spawn()
            .map(|_| ())
            .map_err(|_| {
                ErrorDto::unavailable(
                    "local_daemon_launch_failed",
                    "the local daemon could not be started",
                )
            })
    }
}

/// One established connection with its typed directions.
struct LocalLink {
    sender: AsyncMessageSender,
    receiver: AsyncMessageReceiver,
}

/// The connected shared-client facade exposed to presentation adapters.
pub struct IntentionClient {
    endpoint: LocalEndpoint,
    launcher: Box<dyn DaemonLauncher>,
}

impl IntentionClient {
    /// Creates a typed client for one local endpoint and its launcher seam.
    #[must_use]
    pub fn new(endpoint: LocalEndpoint, launcher: Box<dyn DaemonLauncher>) -> Self {
        Self { endpoint, launcher }
    }

    /// Connects to a ready daemon or serializes exactly one local process launch.
    ///
    /// The client attempts the connection before spawning, then uses a
    /// process-wide advisory lock only when the endpoint is unavailable.
    /// Readiness is a healthy projection on a fresh connection.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error if bootstrap, launch, connection, or readiness
    /// cannot complete before the bounded deadline.
    pub async fn connect_or_bootstrap(&self) -> DtoResult<DaemonHealthDto> {
        match self.connect_ready().await {
            Ok(health) => return Ok(health),
            Err(error) if !is_unavailable(&error) => return Err(error),
            Err(_) => {}
        }

        let _lock = StartupLock::acquire(&self.endpoint).await?;
        match self.connect_ready().await {
            Ok(health) => return Ok(health),
            Err(error) if !is_unavailable(&error) => return Err(error),
            Err(_) => {}
        }
        self.launcher.launch(&self.endpoint)?;
        self.wait_for_ready().await
    }

    /// Waits for the daemon to report ready health within the bootstrap budget.
    ///
    /// The wait retries an unavailable daemon with the same bounded budget and
    /// backoff used by [`IntentionClient::connect_or_bootstrap`]; it never
    /// launches a process.
    ///
    /// # Errors
    ///
    /// Returns the typed error that ended the wait: the last unavailable error
    /// when the budget expires first, or the daemon's typed rejection.
    pub async fn await_ready(&self) -> DtoResult<DaemonHealthDto> {
        self.wait_for_ready().await
    }

    /// Creates a durable session and returns the daemon's acceptance evidence.
    ///
    /// The command carries no session identity to compare: the daemon assigns
    /// the session, project, and workspace, so acceptance evidence is the only
    /// thing the reply can be checked for.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not a session creation.
    pub async fn create_session(
        &self,
        command: CreateSessionCommandDto,
    ) -> DtoResult<CreateSessionAcceptedDto> {
        match self
            .request(ClientRequestDto::CreateSession(command))
            .await?
        {
            ProtocolResultDto::SessionCreated(created) => Ok(created),
            _ => Err(invalid_response()),
        }
    }

    /// Sends one user turn and returns the daemon's durable turn outcome.
    ///
    /// The daemon proposes the run identity: a turn that starts a run reports
    /// `Started` with that run, and a turn that joins the active run context
    /// reports `Pending`.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error for blank content, the daemon's typed
    /// rejection, a typed transport or timeout error, or an invalid-response
    /// error when the reply is not an accepted turn of this session.
    pub async fn send_user_turn(
        &self,
        session_id: SessionId,
        idempotency_key: IdempotencyKey,
        content: String,
    ) -> DtoResult<SendUserTurnOutcomeDto> {
        let command = SendUserTurnCommandDto::new(session_id, idempotency_key, content)?;
        match self
            .request(ClientRequestDto::SendUserTurn(command))
            .await?
        {
            ProtocolResultDto::TurnAccepted(turn) if turn.session_id() == session_id => {
                Ok(turn.outcome())
            }
            _ => Err(invalid_response()),
        }
    }

    /// Removes one not-yet-seen pending user turn.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not the removal of this
    /// session's turn.
    pub async fn remove_turn(
        &self,
        session_id: SessionId,
        turn_id: TurnId,
    ) -> DtoResult<RemoveTurnAcceptedDto> {
        match self
            .request(ClientRequestDto::RemoveTurn(RemoveTurnCommandDto::new(
                session_id, turn_id,
            )))
            .await?
        {
            ProtocolResultDto::TurnRemoved(removed)
                if removed.session_id() == session_id && removed.turn_id() == turn_id =>
            {
                Ok(removed)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Requests interruption of one exact active run's current operation.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not an accepted interrupt
    /// of this session's run.
    pub async fn interrupt_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<InterruptRunAcceptedDto> {
        match self
            .request(ClientRequestDto::InterruptRun(InterruptRunCommandDto::new(
                session_id, run_id,
            )))
            .await?
        {
            ProtocolResultDto::RunInterrupted(interrupted)
                if interrupted.session_id() == session_id && interrupted.run_id() == run_id =>
            {
                Ok(interrupted)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Queries the current durable session snapshot.
    ///
    /// This is the single session read: it returns the current durable
    /// projection snapshot, is not a retained connection, and has no cursor or
    /// resume state. Re-reading re-reads current state.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not the requested
    /// session's snapshot.
    pub async fn session_snapshot(&self, session_id: SessionId) -> DtoResult<SessionSnapshotDto> {
        match self
            .request(ClientRequestDto::GetSessionSnapshot(
                GetSessionSnapshotQueryDto::new(session_id),
            ))
            .await?
        {
            ProtocolResultDto::SessionSnapshot(snapshot) if snapshot.session_id() == session_id => {
                Ok(snapshot)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Lists one bounded page of the active provider catalog.
    ///
    /// The page carries the safe credential-free entry projection and an opaque
    /// continuation token that is passed back unchanged for the next page.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection (including a malformed page token or
    /// a token from another catalog revision), a typed transport or timeout
    /// error, or an invalid-response error when the reply is not a catalog page.
    pub async fn list_provider_catalog(
        &self,
        query: ListProviderCatalogQueryDto,
    ) -> DtoResult<ProviderCatalogPageDto> {
        match self
            .request(ClientRequestDto::ListProviderCatalog(query))
            .await?
        {
            ProtocolResultDto::ProviderCatalogPage(page) => Ok(page),
            _ => Err(invalid_response()),
        }
    }

    /// Queries the current safe provider catalog status.
    ///
    /// The status reports the closed activation state, the applicable closed
    /// degraded reason, and safe revision and validation identities.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not a catalog status.
    pub async fn provider_catalog_status(&self) -> DtoResult<ProviderCatalogStatusDto> {
        match self
            .request(ClientRequestDto::GetProviderCatalogStatus)
            .await?
        {
            ProtocolResultDto::ProviderCatalogStatus(status) => Ok(status),
            _ => Err(invalid_response()),
        }
    }

    /// Changes one session's durable default provider profile.
    ///
    /// The command carries the expected durable session projection revision, so
    /// a stale expectation is the daemon's typed conflict; requesting the
    /// profile that is already the default is accepted with `changed = false`.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not the acceptance of this
    /// command's session.
    pub async fn set_session_provider_profile(
        &self,
        command: SetSessionProviderProfileCommandDto,
    ) -> DtoResult<SetSessionProviderProfileAcceptedDto> {
        let session_id = command.session_id();
        match self
            .request(ClientRequestDto::SetSessionProviderProfile(command))
            .await?
        {
            ProtocolResultDto::SessionProviderProfileSet(accepted)
                if accepted.session_id() == session_id =>
            {
                Ok(accepted)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Queries one session's durable provider profile projection.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not the requested
    /// session's provider profile projection.
    pub async fn session_provider_profile(
        &self,
        session_id: SessionId,
    ) -> DtoResult<SessionProviderProfileProjectionDto> {
        match self
            .request(ClientRequestDto::GetSessionProviderProfile(
                GetSessionProviderProfileQueryDto::new(session_id),
            ))
            .await?
        {
            ProtocolResultDto::SessionProviderProfile(profile)
                if profile.session_id() == session_id =>
            {
                Ok(profile)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Re-reads the configuration file through the daemon's private loading boundary.
    ///
    /// A semantically equal configuration is a no-op success; a catalog-affecting
    /// change is rejected with no durable change.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection (including
    /// `catalog_change_requires_restart`), a typed transport or timeout error, or
    /// an invalid-response error when the reply is not a configuration reload.
    pub async fn reload_configuration(
        &self,
        operation_id: IdempotencyKey,
    ) -> DtoResult<ConfigurationReloadAcceptedDto> {
        match self
            .request(ClientRequestDto::ReloadConfiguration(
                ReloadConfigurationCommandDto::new(operation_id),
            ))
            .await?
        {
            ProtocolResultDto::ConfigurationReloaded(accepted) => Ok(accepted),
            _ => Err(invalid_response()),
        }
    }

    /// Rotates one profile's private credential from the daemon's own source.
    ///
    /// The command carries no credential material: the daemon re-reads its
    /// private source inside the loading boundary and rebuilds the profile's
    /// private client only when every safe selected field still matches.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection (including a frozen-meaning mismatch
    /// and an unavailable source), a typed transport or timeout error, or an
    /// invalid-response error when the reply rotates another profile.
    pub async fn rotate_provider_credential(
        &self,
        profile_id: ProviderProfileId,
        operation_id: IdempotencyKey,
    ) -> DtoResult<CredentialRotationAcceptedDto> {
        match self
            .request(ClientRequestDto::RotateProviderCredential(
                RotateProviderCredentialCommandDto::new(profile_id.clone(), operation_id),
            ))
            .await?
        {
            ProtocolResultDto::ProviderCredentialRotated(accepted)
                if accepted.profile_id() == &profile_id =>
            {
                Ok(accepted)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Checks one profile's live provider health.
    ///
    /// Health evidence is non-authorizing operational evidence: it creates no
    /// run, retry, or quota, and it never reroutes a selection.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is another provider's
    /// evidence.
    pub async fn check_provider_health(
        &self,
        profile_id: ProviderProfileId,
    ) -> DtoResult<ProviderHealthEvidenceDto> {
        match self
            .request(ClientRequestDto::CheckProviderHealth(
                CheckProviderHealthCommandDto::new(profile_id.clone()),
            ))
            .await?
        {
            ProtocolResultDto::ProviderHealth(evidence)
                if evidence.provider_id() == &profile_id =>
            {
                Ok(evidence)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Begins one non-authorizing provider/model discovery attempt.
    ///
    /// Discovered records are additive: they never select, repair, or replace a
    /// profile, and the attempt has no automatic continuation.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the reply is not a discovery result.
    pub async fn discover_provider_models(
        &self,
        profile_id: ProviderProfileId,
    ) -> DtoResult<ProviderDiscoveryResultDto> {
        match self
            .request(ClientRequestDto::DiscoverProviderModels(
                DiscoverProviderModelsCommandDto::new(profile_id),
            ))
            .await?
        {
            ProtocolResultDto::ProviderModelsDiscovered(result) => Ok(result),
            _ => Err(invalid_response()),
        }
    }

    /// Applies one credential-free configuration document.
    ///
    /// The daemon validates the candidate server-side, writes the validated
    /// document atomically, and restores the retained private credential inside
    /// its own loading boundary; a candidate carrying credential material is
    /// rejected.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error for a blank document, the daemon's typed
    /// rejection, a typed transport or timeout error, or an invalid-response
    /// error when the reply is not an accepted configuration edit.
    pub async fn apply_configuration_document(
        &self,
        document: String,
        operation_id: IdempotencyKey,
    ) -> DtoResult<ConfigurationEditAcceptedDto> {
        let command = ApplyConfigurationDocumentCommandDto::new(document, operation_id)?;
        match self
            .request(ClientRequestDto::ApplyConfigurationDocument(command))
            .await?
        {
            ProtocolResultDto::ConfigurationDocumentApplied(accepted) => Ok(accepted),
            _ => Err(invalid_response()),
        }
    }

    /// Applies closed typed configuration edits.
    ///
    /// The daemon validates the edits server-side, writes the resulting document
    /// atomically, and commits the new configuration revision.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error when no edit is supplied, the daemon's
    /// typed rejection, a typed transport or timeout error, or an
    /// invalid-response error when the reply is not an accepted configuration
    /// edit.
    pub async fn apply_configuration_edits(
        &self,
        edits: Vec<ConfigurationEditDto>,
        operation_id: IdempotencyKey,
    ) -> DtoResult<ConfigurationEditAcceptedDto> {
        let command = ApplyConfigurationEditsCommandDto::new(edits, operation_id)?;
        match self
            .request(ClientRequestDto::ApplyConfigurationEdits(command))
            .await?
        {
            ProtocolResultDto::ConfigurationEditsApplied(accepted) => Ok(accepted),
            _ => Err(invalid_response()),
        }
    }

    /// Accepts one exact pending catalog removal candidate.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the acceptance is not the requested
    /// candidate's revision.
    pub async fn accept_provider_catalog_removal(
        &self,
        command: AcceptProviderCatalogRemovalCommandDto,
    ) -> DtoResult<ProviderCatalogRemovalAcceptedDto> {
        let candidate_revision_id = command.candidate().candidate_revision_id();
        match self
            .request(ClientRequestDto::AcceptProviderCatalogRemoval(command))
            .await?
        {
            ProtocolResultDto::ProviderCatalogRemovalAccepted(accepted)
                if accepted.catalog_revision_id() == candidate_revision_id =>
            {
                Ok(accepted)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Rejects one pending catalog candidate.
    ///
    /// Rejection leaves the catalog degraded read-only on the active revision
    /// the candidate was prepared against; it publishes no new catalog revision.
    ///
    /// # Errors
    ///
    /// Returns the daemon's typed rejection, a typed transport or timeout error,
    /// or an invalid-response error when the rejection does not report the active
    /// revision the candidate was prepared against.
    pub async fn reject_provider_catalog_candidate(
        &self,
        command: RejectProviderCatalogCandidateCommandDto,
    ) -> DtoResult<ProviderCatalogCandidateRejectedDto> {
        let expected_active_revision_id = command.candidate().expected_active_revision_id();
        match self
            .request(ClientRequestDto::RejectProviderCatalogCandidate(command))
            .await?
        {
            ProtocolResultDto::ProviderCatalogCandidateRejected(rejected)
                if rejected.active_catalog_revision_id() == Some(expected_active_revision_id) =>
            {
                Ok(rejected)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Sends one user turn carrying an explicit provider profile override.
    ///
    /// The override applies only to the run this turn starts, never to the
    /// session default; the daemon resolves and records the exact selection
    /// before the run commits.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error for blank content, the daemon's typed
    /// rejection (including a revision mismatch and an unavailable runtime
    /// selection), a typed transport or timeout error, or an invalid-response
    /// error when the reply is not an accepted turn of this session.
    pub async fn send_user_turn_with_profile(
        &self,
        session_id: SessionId,
        idempotency_key: IdempotencyKey,
        content: String,
        provider_profile: ProviderProfileOverrideDto,
    ) -> DtoResult<SendUserTurnOutcomeDto> {
        let command = SendUserTurnCommandDto::new(session_id, idempotency_key, content)?
            .with_provider_profile(Some(provider_profile));
        match self
            .request(ClientRequestDto::SendUserTurn(command))
            .await?
        {
            ProtocolResultDto::TurnAccepted(turn) if turn.session_id() == session_id => {
                Ok(turn.outcome())
            }
            _ => Err(invalid_response()),
        }
    }

    async fn connect_ready(&self) -> DtoResult<DaemonHealthDto> {
        let link = self.connect().await?;
        match Self::request_on(link, ClientRequestDto::GetDaemonHealth).await? {
            ProtocolResultDto::DaemonHealth(health) => Ok(health),
            _ => Err(invalid_response()),
        }
    }

    async fn request(&self, request: ClientRequestDto) -> DtoResult<ProtocolResultDto> {
        let link = self.connect().await?;
        Self::request_on(link, request).await
    }

    async fn connect(&self) -> DtoResult<LocalLink> {
        let connection = AsyncLocalClientConnection::connect(&self.endpoint).await?;
        let (sender, receiver) = connection.split();
        Ok(LocalLink { sender, receiver })
    }

    /// Sends one request and decodes its correlated answer.
    ///
    /// The link is consumed, so one connection carries exactly one in-flight
    /// request; that convention is what makes the daemon's identity-less
    /// rejection of an undecodable request line unambiguous.
    async fn request_on(
        mut link: LocalLink,
        request: ClientRequestDto,
    ) -> DtoResult<ProtocolResultDto> {
        let message = encode_request(REQUEST_ID, request);
        tokio::time::timeout(REQUEST_TIMEOUT, async {
            link.sender.send_message(&message).await?;
            let line = link.receiver.receive_line().await?;
            decode_response(&line, REQUEST_ID)
        })
        .await
        .map_err(|_| request_timeout())?
    }

    async fn wait_for_ready(&self) -> DtoResult<DaemonHealthDto> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            match self.connect_ready().await {
                Ok(health) => return Ok(health),
                Err(error) if is_unavailable(&error) && Instant::now() < deadline => {
                    tokio::time::sleep(STARTUP_RETRY).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// An asynchronous facade for dedicated run-stream subscriptions.
///
/// It shares the one local link: requests flow to the daemon, and committed
/// `run.frame` values flow back.
pub struct RunStreamClient {
    endpoint: LocalEndpoint,
}

impl RunStreamClient {
    /// Creates a run-stream client for one local endpoint.
    #[must_use]
    pub const fn new(endpoint: LocalEndpoint) -> Self {
        Self { endpoint }
    }

    /// Connects, subscribes, and applies the authoritative first reply.
    ///
    /// The request carries only the session and run identity; its correlated
    /// reply is the current run state, and later committed state arrives as
    /// live `run.frame` messages on the same connection.
    ///
    /// # Errors
    ///
    /// Returns a typed protocol, transport, or scoped-response error, or an
    /// unavailable timeout error when the correlated reply does not arrive
    /// before the bounded deadline.
    pub async fn subscribe(
        &self,
        subscription: SubscribeRunCommandDto,
    ) -> DtoResult<RunStreamSubscription> {
        let session_id = subscription.session_id();
        let run_id = subscription.run_id();
        let connection = AsyncLocalClientConnection::connect(&self.endpoint).await?;
        let (mut sender, mut receiver) = connection.split();
        let request = encode_request(REQUEST_ID, ClientRequestDto::SubscribeRun(subscription));
        sender.send_message(&request).await?;
        let line = tokio::time::timeout(STREAM_REPLY_TIMEOUT, receiver.receive_line())
            .await
            .map_err(|_| stream_reply_timeout())??;
        let ProtocolResultDto::RunSubscribed(snapshot) = decode_response(&line, REQUEST_ID)? else {
            return Err(invalid_response());
        };
        let mut state = RunStreamState::new(session_id, run_id);
        state.apply_initial(snapshot)?;
        Ok(RunStreamSubscription {
            sender,
            receiver,
            state,
        })
    }
}

/// An established run-stream subscription with opaque transport resources.
pub struct RunStreamSubscription {
    /// The send direction is retained so the connection keeps both halves
    /// open while committed frames arrive on the receive direction.
    #[expect(
        dead_code,
        reason = "the send direction is held only to keep the daemon connection open for live frames"
    )]
    sender: AsyncMessageSender,
    receiver: AsyncMessageReceiver,
    state: RunStreamState,
}

impl RunStreamSubscription {
    /// Receives, applies, and returns the next committed run-stream frame.
    ///
    /// Returns `Ok(None)` once the daemon closes the stream; the caller then
    /// re-reads current state by re-subscribing. Content and status frames are
    /// applied to the committed state before they are returned.
    ///
    /// # Errors
    ///
    /// Returns a typed framing, protocol, or scope-validation error without
    /// mutating committed state. Live frames may be arbitrarily sparse, so this
    /// wait is intentionally unbounded; only the correlated reply waits carry a
    /// deadline.
    pub async fn receive(&mut self) -> DtoResult<Option<RunStreamFrameDto>> {
        let line = match self.receiver.receive_line().await {
            Ok(line) => line,
            Err(error) if error.code() == "local_daemon_connection_unavailable" => return Ok(None),
            Err(error) => return Err(error),
        };
        let frame = parse_run_frame(&line)?;
        self.state.apply_frame(frame.clone())?;
        Ok(Some(frame))
    }

    /// Returns the committed state of this fixed run scope.
    #[must_use]
    pub const fn state(&self) -> &RunStreamState {
        &self.state
    }
}

/// The number of newest committed transcript rows one subscription retains.
///
/// Live content frames are unbounded in number, so a long-lived subscription
/// keeps only the newest rows; the durable transcript stays complete, and a
/// re-subscribe re-reads the bounded current-state snapshot.
pub const RETAINED_TRANSCRIPT_MESSAGES: usize = 256;

/// The committed state of one fixed run scope.
///
/// The subscription reply is the current run snapshot, and every later frame
/// carries the committed value of its own scope: a content frame appends its
/// committed transcript row and a status frame replaces the committed run
/// projection. There are no cursors and no positions, so no merge machine is
/// needed: a status frame is idempotent because it replaces the whole run, and
/// a content frame is idempotent only for the newest accepted row — the wire
/// carries no row identity, so the daemon-side watermark remains the authority
/// for a row the snapshot already carried. The retained transcript keeps the
/// newest [`RETAINED_TRANSCRIPT_MESSAGES`] rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunStreamState {
    session_id: SessionId,
    run_id: RunId,
    run: Option<RunProjectionDto>,
    messages: Vec<MessageProjectionDto>,
}

impl RunStreamState {
    /// Creates empty local state fixed to one session and run.
    #[must_use]
    pub const fn new(session_id: SessionId, run_id: RunId) -> Self {
        Self {
            session_id,
            run_id,
            run: None,
            messages: Vec::new(),
        }
    }

    /// Applies the correlated first reply for this subscription.
    ///
    /// A snapshot replaces every value held before it.
    ///
    /// # Errors
    ///
    /// Returns a typed scoped-response error without mutation when the snapshot
    /// belongs to another session or run.
    pub fn apply_initial(&mut self, snapshot: RunSubscriptionSnapshotDto) -> DtoResult<()> {
        self.ensure_scope(snapshot.run().session_id(), Some(snapshot.run().run_id()))?;
        let mut next = Self::new(self.session_id, self.run_id);
        next.run = Some(snapshot.run().clone());
        next.messages = snapshot.messages().to_vec();
        next.retain_newest();
        *self = next;
        Ok(())
    }

    /// Applies one uncorrelated committed frame.
    ///
    /// # Errors
    ///
    /// Returns a typed scoped-response error without mutation for a frame that
    /// belongs to another session or run scope, and an invalid-response error
    /// for a status frame received before the subscription snapshot.
    pub fn apply_frame(&mut self, frame: RunStreamFrameDto) -> DtoResult<()> {
        match frame {
            RunStreamFrameDto::Content(message) => self.apply_content(message),
            RunStreamFrameDto::Status(run) => self.apply_status(run),
        }
    }

    fn apply_content(&mut self, message: MessageProjectionDto) -> DtoResult<()> {
        self.ensure_scope(message.session_id(), message.run_id())?;
        // A content frame repeating the newest accepted row is the frame the
        // subscription snapshot already carried: re-appending it would show the
        // same committed row twice.
        if self.messages.last() == Some(&message) {
            return Ok(());
        }
        self.messages.push(message);
        self.retain_newest();
        Ok(())
    }

    /// Drops the oldest retained rows past [`RETAINED_TRANSCRIPT_MESSAGES`].
    fn retain_newest(&mut self) {
        if self.messages.len() > RETAINED_TRANSCRIPT_MESSAGES {
            let excess = self.messages.len() - RETAINED_TRANSCRIPT_MESSAGES;
            self.messages.drain(..excess);
        }
    }

    fn apply_status(&mut self, run: RunProjectionDto) -> DtoResult<()> {
        self.ensure_scope(run.session_id(), Some(run.run_id()))?;
        if self.run.is_none() {
            return Err(invalid_response());
        }
        self.run = Some(run);
        Ok(())
    }

    fn ensure_scope(&self, session_id: SessionId, run_id: Option<RunId>) -> DtoResult<()> {
        if session_id == self.session_id && run_id.is_none_or(|run_id| run_id == self.run_id) {
            Ok(())
        } else {
            Err(scope_error())
        }
    }

    /// Returns the fixed session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the fixed run identity.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the current run projection, once a snapshot has been accepted.
    #[must_use]
    pub const fn run(&self) -> Option<&RunProjectionDto> {
        self.run.as_ref()
    }

    /// Returns the current run lifecycle status, once a snapshot has been accepted.
    #[must_use]
    pub const fn status(&self) -> Option<RunStatusDto> {
        match self.run.as_ref() {
            Some(run) => Some(run.status()),
            None => None,
        }
    }

    /// Returns the committed transcript rows accepted so far.
    #[must_use]
    pub fn messages(&self) -> &[MessageProjectionDto] {
        &self.messages
    }
}

struct StartupLock {
    file: std::fs::File,
}

impl StartupLock {
    /// Takes the per-endpoint bootstrap lock within the bootstrap budget.
    ///
    /// # Errors
    ///
    /// Returns the typed startup-lock failure when the lock cannot be taken
    /// before the budget expires.
    async fn acquire(endpoint: &LocalEndpoint) -> DtoResult<Self> {
        Self::acquire_path(&startup_lock_path(endpoint)?, STARTUP_TIMEOUT).await
    }

    /// Takes the lock at `path`, retrying a contended lock until `budget` ends.
    ///
    /// The wait is bounded and yields to the runtime between attempts, so a
    /// bootstrap never blocks a task on another process without a deadline.
    ///
    /// # Errors
    ///
    /// Returns the typed startup-lock failure when the lock cannot be taken
    /// before `budget` expires.
    async fn acquire_path(path: &Path, budget: Duration) -> DtoResult<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| unavailable("startup_lock_unavailable"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                    .map_err(|_| unavailable("startup_lock_unavailable"))?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(|_| unavailable("startup_lock_unavailable"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|_| unavailable("startup_lock_unavailable"))?;
        }
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            match fs4::FileExt::try_lock(&file) {
                Ok(()) => return Ok(Self { file }),
                Err(fs4::TryLockError::WouldBlock) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(STARTUP_RETRY).await;
                }
                Err(fs4::TryLockError::WouldBlock) => return Err(startup_lock_timeout()),
                Err(fs4::TryLockError::Error(_)) => {
                    return Err(unavailable("startup_lock_unavailable"));
                }
            }
        }
    }
}

impl Drop for StartupLock {
    fn drop(&mut self) {
        let _ = fs4::FileExt::unlock(&self.file);
    }
}

fn startup_lock_path(endpoint: &LocalEndpoint) -> DtoResult<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    endpoint.instance_id().hash(&mut hasher);
    let endpoint_hash = hasher.finish();
    let base = platform_state_directory()?;
    Ok(base.join(format!("bootstrap-{endpoint_hash:016x}.lock")))
}

fn platform_state_directory() -> DtoResult<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|candidate| candidate.is_absolute())
            .or_else(|| {
                std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .filter(|candidate| candidate.is_absolute())
                    .map(|candidate| candidate.join("intention-relay"))
            })
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|candidate| candidate.is_absolute())
                    .map(|candidate| candidate.join(".config/intention-relay"))
            })
            .ok_or_else(|| unavailable("startup_lock_unavailable"))
    }
    #[cfg(target_os = "macos")]
    {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|candidate| candidate.is_absolute())
            .map(|candidate| candidate.join("Library/Application Support/intention-relay"))
            .ok_or_else(|| unavailable("startup_lock_unavailable"));
    }
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|candidate| candidate.is_absolute())
            .map(|candidate| candidate.join("intention-relay"))
            .ok_or_else(|| unavailable("startup_lock_unavailable"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err(unavailable("startup_lock_unavailable"))
    }
}

/// Reports whether one failure means the daemon endpoint is not yet reachable.
///
/// The decision is the typed error category, not a matched code string: every
/// unavailable failure is retried within the bounded bootstrap budget.
fn is_unavailable(error: &ErrorDto) -> bool {
    error.category() == ErrorCategoryDto::Unavailable
}

fn invalid_response() -> ErrorDto {
    ErrorDto::validation(
        "invalid_local_protocol_response",
        "the local daemon returned an unexpected protocol response",
    )
}

fn scope_error() -> ErrorDto {
    ErrorDto::validation(
        "invalid_run_subscription",
        "run subscription data belongs to another run scope",
    )
}

fn request_timeout() -> ErrorDto {
    ErrorDto::unavailable(
        "local_daemon_reply_timeout",
        "the local daemon did not answer the request in time",
    )
}

fn stream_reply_timeout() -> ErrorDto {
    ErrorDto::unavailable(
        "run_stream_reply_timeout",
        "the local daemon did not answer the run-stream request in time",
    )
}

fn unavailable(code: &'static str) -> ErrorDto {
    ErrorDto::unavailable(code, "the local daemon connection is unavailable")
}

fn startup_lock_timeout() -> ErrorDto {
    ErrorDto::unavailable(
        "local_daemon_startup_timeout",
        "the local daemon bootstrap lock was not released in time",
    )
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "Unit fixtures use direct assertions for precise diagnostics."
    )]

    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn startup_lock_wait_is_bounded_by_its_budget() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after the Unix epoch")
            .as_nanos();
        // The fixture root is its own directory under the temporary directory:
        // the lock path owns its parent, so the fixture never chmods a shared
        // directory such as the platform runtime directory.
        let base = std::env::temp_dir().join(format!(
            "intention-client-lock-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&base).expect("the fixture directory is created");
        let path = base.join("bootstrap.lock");
        let budget = Duration::from_millis(50);
        let held = StartupLock::acquire_path(&path, budget)
            .await
            .expect("an uncontended bootstrap lock is taken");

        let error = match StartupLock::acquire_path(&path, budget).await {
            Ok(_) => panic!("a contended bootstrap lock must end the bounded wait"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "local_daemon_startup_timeout");
        assert_eq!(error.category(), ErrorCategoryDto::Unavailable);

        drop(held);
        let _ = fs::remove_dir_all(&base);
    }
}
