//! Shared bootstrap, dispatch, subscription, and reconnect client for local adapters.
//!
//! Adapters use this crate instead of direct daemon, runtime, storage, or
//! transport implementation access. It retains only reconnect projection state;
//! daemon authority remains remote.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use intention_domain::{ModelRunFactDto, ModelRunFactInputDto, RunEventCursorDto, RunSnapshotDto};
use intention_protocol::{
    DaemonHealthDto, DaemonReadinessDto, ProtocolAcceptedResultDto, ProtocolCapabilityDto,
    ProtocolCommandDto, ProtocolCommandResultDto, ProtocolDaemonFrameDto, ProtocolHelloDto,
    ProtocolMessageDto, ProtocolQueryDto, ProtocolQueryResultDto, ProtocolRequestEnvelopeDto,
    ProtocolRequestPayloadDto, ProtocolResponsePayloadDto, ProtocolVersionDto, RunLiveBatchDto,
    RunResyncDto, RunResyncReasonDto, RunStreamFrameDto, RunSubscriptionRequestEnvelopeDto,
    RunSubscriptionResponseDto, SessionSnapshotDto, SessionSubscriptionResponseDto,
    SubscribeRunCommandDto, SubscribeSessionCommandDto,
    contract_families::{
        AcceptProviderCatalogRemovalAcceptedDto, AcceptProviderCatalogRemovalCommandDto,
        AdmitRecoveredRunAcceptedDto, AdmitRecoveredRunCommandDto, ConfigurationEditCommandDto,
        ConfigurationProjectionDto, CredentialRotationResultDto,
        GetConfigurationProjectionQueryDto, GetPricingPolicyQueryDto, GetProviderCatalogQueryDto,
        GetProviderCatalogStatusQueryDto, GetProviderDiscoveryStatusQueryDto,
        GetProviderHealthEvidenceQueryDto, GetProviderUsageQueryDto,
        GetSessionProviderProfileQueryDto, PricingProjectionDto, ProviderCatalogPageDto,
        ProviderCatalogStatusDto, ProviderDiscoveryProjectionDto, ProviderHealthProjectionDto,
        RawTomlEditCommandDto, ReconcileUnavailableQueueAcceptedDto,
        ReconcileUnavailableQueueCommandDto, RejectProviderCatalogCandidateAcceptedDto,
        RejectProviderCatalogCandidateCommandDto, ReloadConfigurationCommandDto,
        ReloadTransactionDto, RotateProviderCredentialsCommandDto, SessionProviderProfileDto,
        SetSessionProviderProfileAcceptedDto, SetSessionProviderProfileCommandDto,
        UsageAggregationDto,
    },
};
use intention_transport::{
    AsyncDaemonFrameReceiver, AsyncLocalClientConnection, AsyncRequestSender, LocalConnection,
    LocalEndpoint, local_protocol_version, negotiate_client,
};
use intention_types::{
    CorrelationIdDto, DtoResult, ErrorCategoryDto, ErrorDto, EventEnvelopeDto, EventId,
    SchemaVersionDto, SessionEventSequenceDto, SessionId,
};

const SCHEMA_VERSION: SchemaVersionDto = intention_protocol::CURRENT_DTO_SCHEMA_VERSION;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
const STARTUP_RETRY: Duration = Duration::from_millis(25);
const REQUIRED_CAPABILITIES: [ProtocolCapabilityDto; 3] = [
    ProtocolCapabilityDto::SessionSubscriptions,
    ProtocolCapabilityDto::CorrelatedRequests,
    ProtocolCapabilityDto::DaemonHealth,
];
const RUN_STREAM_CAPABILITIES: [ProtocolCapabilityDto; 1] =
    [ProtocolCapabilityDto::RunStreamSubscriptions];

/// Launches a daemon process after bootstrap has acquired the startup lock.
pub trait DaemonLauncher: Send + Sync {
    /// Starts one daemon host for `endpoint`.
    ///
    /// # Errors
    ///
    /// Returns only a safe typed launch error. Readiness is verified separately
    /// by `IntentionClient` through protocol negotiation and health query.
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
                ErrorDto::new(
                    "local_daemon_launch_failed",
                    ErrorCategoryDto::Unavailable,
                    "the local daemon could not be started",
                    intention_types::ErrorRetryDto::Manual,
                    None,
                )
                .unwrap_or_else(|_| unavailable("local_daemon_launch_failed"))
            })
    }
}

struct NegotiatedConnection {
    connection: LocalConnection,
    daemon_version: ProtocolVersionDto,
}

/// The connected shared-client facade exposed to presentation adapters.
pub struct IntentionClient {
    endpoint: LocalEndpoint,
    hello: ProtocolHelloDto,
    launcher: Box<dyn DaemonLauncher>,
}

impl IntentionClient {
    /// Creates a typed client with the adapter metadata used in protocol hello.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the adapter metadata is invalid.
    pub fn new(
        endpoint: LocalEndpoint,
        adapter_name: impl Into<String>,
        launcher: Box<dyn DaemonLauncher>,
    ) -> DtoResult<Self> {
        let hello = ProtocolHelloDto::new(
            local_protocol_version(),
            REQUIRED_CAPABILITIES.to_vec(),
            adapter_name,
        )?;
        Ok(Self {
            endpoint,
            hello,
            launcher,
        })
    }

    /// Connects to a ready daemon or serializes exactly one local process launch.
    ///
    /// The client attempts IPC before spawning, retries an already-starting
    /// daemon without spawning, then uses a process-wide advisory lock only when
    /// the endpoint is unavailable. Readiness requires compatible hello,
    /// capabilities, a correlated health query, and `Ready` state.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error if bootstrap, launch, negotiation, or readiness
    /// cannot complete before the bounded deadline.
    pub fn connect_or_bootstrap(&self) -> DtoResult<DaemonHealthDto> {
        match self.connect_ready() {
            Ok(health) => return Ok(health),
            Err(error) if is_daemon_starting(&error) => return self.wait_for_ready(),
            Err(error) if !is_daemon_unavailable(&error) => return Err(error),
            Err(_) => {}
        }

        let _lock = StartupLock::acquire(&self.endpoint)?;
        match self.connect_ready() {
            Ok(health) => return Ok(health),
            Err(error) if is_daemon_starting(&error) => return self.wait_for_ready(),
            Err(error) if !is_daemon_unavailable(&error) => return Err(error),
            Err(_) => {}
        }
        self.launcher.launch(&self.endpoint)?;
        self.wait_for_ready()
    }

    /// Queries the daemon-owned health projection after a fresh negotiated connection.
    ///
    /// # Errors
    ///
    /// Returns a safe typed transport, protocol, or non-ready daemon error.
    pub fn health(&self) -> DtoResult<DaemonHealthDto> {
        self.connect_ready()
    }

    /// Queries the current M2 session snapshot fixture.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable, rejected, or invalid-response error.
    pub fn session_snapshot(&self, session_id: SessionId) -> DtoResult<SessionSnapshotDto> {
        let response = self.request(ProtocolRequestPayloadDto::Query(
            ProtocolQueryDto::GetSessionSnapshot(
                intention_domain::GetSessionSnapshotQueryDto::new(session_id),
            ),
        ))?;
        match response {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::SessionSnapshot(
                snapshot,
            )) => Ok(snapshot),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error)) => {
                Err(error)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Obtains a consistent snapshot-and-tail or typed resync response.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable or invalid-response error. A server-directed
    /// resync is returned as data so adapters can discard local projection state.
    pub fn subscribe(
        &self,
        subscription: SubscribeSessionCommandDto,
    ) -> DtoResult<SessionSubscriptionResponseDto> {
        let response = self.request(ProtocolRequestPayloadDto::Command(
            intention_protocol::ProtocolCommandDto::SubscribeSession(subscription),
        ))?;
        match response {
            ProtocolResponsePayloadDto::Subscription(response) => Ok(response),
            _ => Err(invalid_response()),
        }
    }

    /// Reloads daemon configuration from a prepared candidate reference.
    ///
    /// # Errors
    ///
    /// Returns the typed rejected transaction outcome, or a typed unavailable
    /// or invalid-response error.
    pub fn reload_configuration(
        &self,
        command: ReloadConfigurationCommandDto,
    ) -> DtoResult<ReloadTransactionDto> {
        command.validate()?;
        match self.command_result(ProtocolCommandDto::ReloadConfiguration(command))? {
            ProtocolAcceptedResultDto::ReloadConfiguration(transaction) => Ok(transaction),
            _ => Err(invalid_response()),
        }
    }

    /// Submits a bounded, credential-free raw TOML configuration edit.
    ///
    /// The daemon parses and validates the candidate server-side and never
    /// echoes the raw content back. The response is the durable reload
    /// transaction outcome.
    ///
    /// # Errors
    ///
    /// Returns the typed rejected transaction outcome, or a typed unavailable
    /// or invalid-response error.
    pub fn submit_raw_toml_edit(
        &self,
        command: RawTomlEditCommandDto,
    ) -> DtoResult<ReloadTransactionDto> {
        command.validate()?;
        match self.command_result(ProtocolCommandDto::SubmitRawTomlEdit(command))? {
            ProtocolAcceptedResultDto::ReloadConfiguration(transaction) => Ok(transaction),
            _ => Err(invalid_response()),
        }
    }

    /// Applies typed, credential-free configuration edit operations.
    ///
    /// # Errors
    ///
    /// Returns the typed rejected transaction outcome, or a typed unavailable
    /// or invalid-response error.
    pub fn apply_configuration_edit(
        &self,
        command: ConfigurationEditCommandDto,
    ) -> DtoResult<ReloadTransactionDto> {
        command.validate()?;
        match self.command_result(ProtocolCommandDto::ApplyConfigurationEdit(command))? {
            ProtocolAcceptedResultDto::ReloadConfiguration(transaction) => Ok(transaction),
            _ => Err(invalid_response()),
        }
    }

    /// Rotates one provider's private credential material.
    ///
    /// The command carries no credential material; the replacement arrives
    /// through the daemon's private channel.
    ///
    /// # Errors
    ///
    /// Returns the typed rejected rotation outcome, or a typed unavailable or
    /// invalid-response error.
    pub fn rotate_credential(
        &self,
        command: RotateProviderCredentialsCommandDto,
    ) -> DtoResult<CredentialRotationResultDto> {
        command.validate()?;
        match self.command_result(ProtocolCommandDto::RotateProviderCredentials(command))? {
            ProtocolAcceptedResultDto::RotateProviderCredentials(result) => Ok(result),
            _ => Err(invalid_response()),
        }
    }

    /// Queries non-authorizing health evidence for one provider.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable, rejected, or invalid-response error.
    pub fn provider_health(
        &self,
        query: GetProviderHealthEvidenceQueryDto,
    ) -> DtoResult<ProviderHealthProjectionDto> {
        query.validate()?;
        let response = self.request(ProtocolRequestPayloadDto::Query(
            ProtocolQueryDto::GetProviderHealthEvidence(query),
        ))?;
        match response {
            ProtocolResponsePayloadDto::QueryResult(
                ProtocolQueryResultDto::ProviderHealthEvidence(projection),
            ) => Ok(projection),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error)) => {
                Err(error)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Queries the status of one provider discovery attempt.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable, rejected, or invalid-response error.
    pub fn discovery_status(
        &self,
        query: GetProviderDiscoveryStatusQueryDto,
    ) -> DtoResult<ProviderDiscoveryProjectionDto> {
        query.validate()?;
        let response = self.request(ProtocolRequestPayloadDto::Query(
            ProtocolQueryDto::GetProviderDiscoveryStatus(query),
        ))?;
        match response {
            ProtocolResponsePayloadDto::QueryResult(
                ProtocolQueryResultDto::ProviderDiscoveryStatus(projection),
            ) => Ok(projection),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error)) => {
                Err(error)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Queries the safe non-authorizing pricing policy projection.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable, rejected, or invalid-response error.
    pub fn pricing(&self, query: GetPricingPolicyQueryDto) -> DtoResult<PricingProjectionDto> {
        query.validate()?;
        let response = self.request(ProtocolRequestPayloadDto::Query(
            ProtocolQueryDto::GetPricingPolicy(query),
        ))?;
        match response {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::PricingPolicy(
                projection,
            )) => Ok(projection),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error)) => {
                Err(error)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Queries the safe applied configuration projection.
    ///
    /// The projection never carries raw TOML, credentials, private endpoints,
    /// or paths.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable, rejected, or invalid-response error.
    pub fn configuration_projection(&self) -> DtoResult<ConfigurationProjectionDto> {
        let query = GetConfigurationProjectionQueryDto {
            schema_version: schema_version_string(),
        };
        query.validate()?;
        let response = self.request(ProtocolRequestPayloadDto::Query(
            ProtocolQueryDto::GetConfigurationProjection(query),
        ))?;
        match response {
            ProtocolResponsePayloadDto::QueryResult(
                ProtocolQueryResultDto::ConfigurationProjection(projection),
            ) => Ok(projection),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error)) => {
                Err(error)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Binds one session's durable provider profile intent.
    ///
    /// # Errors
    ///
    /// Returns the typed rejection error, or a typed unavailable or
    /// invalid-response error.
    pub fn set_session_provider_profile(
        &self,
        command: SetSessionProviderProfileCommandDto,
    ) -> DtoResult<SetSessionProviderProfileAcceptedDto> {
        command.validate()?;
        match self.command_result(ProtocolCommandDto::SetSessionProviderProfile(command))? {
            ProtocolAcceptedResultDto::SetSessionProviderProfile(accepted) => Ok(accepted),
            _ => Err(invalid_response()),
        }
    }

    /// Queries one session's durable provider profile projection.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable, rejected, or invalid-response error.
    pub fn session_provider_profile(
        &self,
        session_id: SessionId,
    ) -> DtoResult<SessionProviderProfileDto> {
        let query = GetSessionProviderProfileQueryDto {
            schema_version: schema_version_string(),
            session_id: session_id.to_string(),
        };
        query.validate()?;
        let response = self.request(ProtocolRequestPayloadDto::Query(
            ProtocolQueryDto::GetSessionProviderProfile(query),
        ))?;
        match response {
            ProtocolResponsePayloadDto::QueryResult(
                ProtocolQueryResultDto::SessionProviderProfile(projection),
            ) => Ok(projection),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error)) => {
                Err(error)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Loads one paged provider catalog projection.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable, rejected, or invalid-response error.
    pub fn list_provider_profiles(
        &self,
        query: GetProviderCatalogQueryDto,
    ) -> DtoResult<ProviderCatalogPageDto> {
        query.validate()?;
        let response = self.request(ProtocolRequestPayloadDto::Query(
            ProtocolQueryDto::GetProviderCatalog(query),
        ))?;
        match response {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::ProviderCatalog(
                page,
            )) => Ok(page),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error)) => {
                Err(error)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Queries the provider catalog activation and degradation status.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable, rejected, or invalid-response error.
    pub fn provider_catalog_status(
        &self,
        query: GetProviderCatalogStatusQueryDto,
    ) -> DtoResult<ProviderCatalogStatusDto> {
        query.validate()?;
        let response = self.request(ProtocolRequestPayloadDto::Query(
            ProtocolQueryDto::GetProviderCatalogStatus(query),
        ))?;
        match response {
            ProtocolResponsePayloadDto::QueryResult(
                ProtocolQueryResultDto::ProviderCatalogStatus(status),
            ) => Ok(status),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error)) => {
                Err(error)
            }
            _ => Err(invalid_response()),
        }
    }

    /// Reconciles one bounded page of a session's unavailable-run queue.
    ///
    /// # Errors
    ///
    /// Returns the typed rejection error, or a typed unavailable or
    /// invalid-response error.
    pub fn reconcile_unavailable_queue(
        &self,
        command: ReconcileUnavailableQueueCommandDto,
    ) -> DtoResult<ReconcileUnavailableQueueAcceptedDto> {
        command.validate()?;
        match self.command_result(ProtocolCommandDto::ReconcileUnavailableQueue(command))? {
            ProtocolAcceptedResultDto::ReconcileUnavailableQueue(accepted) => Ok(accepted),
            _ => Err(invalid_response()),
        }
    }

    /// Accepts one pending provider catalog removal.
    ///
    /// # Errors
    ///
    /// Returns the typed rejection error, or a typed unavailable or
    /// invalid-response error.
    pub fn accept_provider_catalog_removal(
        &self,
        command: AcceptProviderCatalogRemovalCommandDto,
    ) -> DtoResult<AcceptProviderCatalogRemovalAcceptedDto> {
        command.validate()?;
        match self.command_result(ProtocolCommandDto::AcceptProviderCatalogRemoval(command))? {
            ProtocolAcceptedResultDto::AcceptProviderCatalogRemoval(accepted) => Ok(accepted),
            _ => Err(invalid_response()),
        }
    }

    /// Rejects one pending provider catalog removal candidate.
    ///
    /// # Errors
    ///
    /// Returns the typed rejection error, or a typed unavailable or
    /// invalid-response error.
    pub fn reject_provider_catalog_candidate(
        &self,
        command: RejectProviderCatalogCandidateCommandDto,
    ) -> DtoResult<RejectProviderCatalogCandidateAcceptedDto> {
        command.validate()?;
        match self.command_result(ProtocolCommandDto::RejectProviderCatalogCandidate(command))? {
            ProtocolAcceptedResultDto::RejectProviderCatalogCandidate(accepted) => Ok(accepted),
            _ => Err(invalid_response()),
        }
    }

    /// Admits one held recovered run back into its session.
    ///
    /// # Errors
    ///
    /// Returns the typed rejection error, or a typed unavailable or
    /// invalid-response error.
    pub fn admit_recovered_run(
        &self,
        command: AdmitRecoveredRunCommandDto,
    ) -> DtoResult<AdmitRecoveredRunAcceptedDto> {
        command.validate()?;
        match self.command_result(ProtocolCommandDto::AdmitRecoveredRun(command))? {
            ProtocolAcceptedResultDto::AdmitRecoveredRun(accepted) => Ok(accepted),
            _ => Err(invalid_response()),
        }
    }

    /// Queries one provider's usage aggregation over a period.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable, rejected, or invalid-response error.
    pub fn provider_usage(
        &self,
        query: GetProviderUsageQueryDto,
    ) -> DtoResult<UsageAggregationDto> {
        query.validate()?;
        let response = self.request(ProtocolRequestPayloadDto::Query(
            ProtocolQueryDto::GetProviderUsage(query),
        ))?;
        match response {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::ProviderUsage(
                usage,
            )) => Ok(usage),
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error)) => {
                Err(error)
            }
            _ => Err(invalid_response()),
        }
    }

    fn connect_ready(&self) -> DtoResult<DaemonHealthDto> {
        let mut connection = self.connect()?;
        let health = self.request_on(
            &mut connection,
            ProtocolRequestPayloadDto::Query(ProtocolQueryDto::GetDaemonHealth),
        )?;
        match health {
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::DaemonHealth(
                health,
            )) => {
                if health.protocol_version() != connection.daemon_version {
                    return Err(invalid_response());
                }
                match health.readiness() {
                    DaemonReadinessDto::Ready => Ok(health),
                    DaemonReadinessDto::Starting => Err(ErrorDto::new(
                        "local_daemon_starting",
                        ErrorCategoryDto::Unavailable,
                        "the local daemon is starting",
                        intention_types::ErrorRetryDto::Delayed,
                        None,
                    )
                    .unwrap_or_else(|_| unavailable("local_daemon_starting"))),
                    DaemonReadinessDto::Draining | DaemonReadinessDto::Unavailable => {
                        Err(ErrorDto::new(
                            "local_daemon_not_ready",
                            ErrorCategoryDto::Unavailable,
                            "the local daemon is not ready to serve requests",
                            intention_types::ErrorRetryDto::Delayed,
                            None,
                        )
                        .unwrap_or_else(|_| unavailable("local_daemon_not_ready")))
                    }
                }
            }
            ProtocolResponsePayloadDto::QueryResult(ProtocolQueryResultDto::Rejected(error)) => {
                Err(error)
            }
            _ => Err(invalid_response()),
        }
    }

    fn request(&self, payload: ProtocolRequestPayloadDto) -> DtoResult<ProtocolResponsePayloadDto> {
        let mut connection = self.connect()?;
        self.request_on(&mut connection, payload)
    }

    /// Sends one control-plane command and returns its typed accepted result.
    ///
    /// A rejected command propagates the daemon's typed error; an acceptance
    /// without an operation-specific result is an invalid response.
    fn command_result(&self, command: ProtocolCommandDto) -> DtoResult<ProtocolAcceptedResultDto> {
        let response = self.request(ProtocolRequestPayloadDto::Command(command))?;
        match response {
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Accepted(
                accepted,
            )) => accepted.result().cloned().ok_or_else(invalid_response),
            ProtocolResponsePayloadDto::CommandResult(ProtocolCommandResultDto::Rejected(
                error,
            )) => Err(error),
            _ => Err(invalid_response()),
        }
    }

    fn connect(&self) -> DtoResult<NegotiatedConnection> {
        let mut connection = LocalConnection::connect(&self.endpoint)?;
        let remote = negotiate_client(&mut connection, self.hello.clone())?;
        if !REQUIRED_CAPABILITIES
            .iter()
            .all(|capability| remote.capabilities().contains(capability))
        {
            return Err(ErrorDto::unavailable(
                "incompatible_protocol_capabilities",
                "the local daemon lacks a required protocol capability",
            ));
        }
        Ok(NegotiatedConnection {
            connection,
            daemon_version: remote.version(),
        })
    }

    fn request_on(
        &self,
        connection: &mut NegotiatedConnection,
        payload: ProtocolRequestPayloadDto,
    ) -> DtoResult<ProtocolResponsePayloadDto> {
        let correlation_id = CorrelationIdDto::new();
        let request = ProtocolRequestEnvelopeDto::new(
            local_protocol_version(),
            correlation_id,
            ProtocolMessageDto::new(SCHEMA_VERSION, payload),
        );
        connection.connection.send_request(&request)?;
        let response = connection.connection.receive_response()?;
        if response.correlation_id() != correlation_id
            || response.protocol_version() != connection.daemon_version
        {
            return Err(invalid_response());
        }
        Ok(response.message().payload().clone())
    }

    fn wait_for_ready(&self) -> DtoResult<DaemonHealthDto> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            match self.connect_ready() {
                Ok(health) => return Ok(health),
                Err(error)
                    if (is_daemon_unavailable(&error) || is_daemon_starting(&error))
                        && Instant::now() < deadline =>
                {
                    thread::sleep(STARTUP_RETRY);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// An opt-in asynchronous facade for dedicated run-stream subscriptions.
///
/// It negotiates only the run-stream capability and never changes the M3
/// synchronous [`IntentionClient`] connection or capability requirements.
pub struct RunStreamClient {
    endpoint: LocalEndpoint,
    hello: ProtocolHelloDto,
}

impl RunStreamClient {
    /// Creates an async run-stream client with safe adapter metadata.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the adapter metadata is blank.
    pub fn new(endpoint: LocalEndpoint, adapter_name: impl Into<String>) -> DtoResult<Self> {
        Ok(Self {
            endpoint,
            hello: ProtocolHelloDto::new(
                local_protocol_version(),
                RUN_STREAM_CAPABILITIES.to_vec(),
                adapter_name,
            )?,
        })
    }

    /// Connects, subscribes, and applies the correlated authoritative first reply.
    ///
    /// # Errors
    ///
    /// Returns a typed protocol, transport, or scoped-response error.
    pub async fn subscribe(
        &self,
        subscription: SubscribeRunCommandDto,
    ) -> DtoResult<RunStreamSubscription> {
        let connection = AsyncLocalClientConnection::connect(&self.endpoint).await?;
        let (remote, mut requests, mut frames) = connection
            .negotiate_daemon_frames(self.hello.clone())
            .await?;
        if !RUN_STREAM_CAPABILITIES
            .iter()
            .all(|capability| remote.capabilities().contains(capability))
        {
            return Err(ErrorDto::unavailable(
                "incompatible_protocol_capabilities",
                "the local daemon lacks a required protocol capability",
            ));
        }
        let correlation_id = CorrelationIdDto::new();
        let request = RunSubscriptionRequestEnvelopeDto::new(
            local_protocol_version(),
            correlation_id,
            ProtocolMessageDto::new(SCHEMA_VERSION, subscription),
        );
        requests.send_run_subscription(&request).await?;
        let initial = frames.receive().await?;
        let response = match initial {
            ProtocolDaemonFrameDto::Response(response)
                if response.correlation_id() == correlation_id
                    && response.protocol_version() == remote.version()
                    && response.message().schema_version() == subscription.schema_version() =>
            {
                response
            }
            _ => return Err(invalid_response()),
        };
        let initial = match response.message().payload() {
            ProtocolResponsePayloadDto::RunSubscription(response) => response.clone(),
            _ => return Err(invalid_response()),
        };
        let mut reducer =
            RunSubscriptionReducer::new(subscription.session_id(), subscription.run_id());
        reducer.apply_initial(initial)?;
        Ok(RunStreamSubscription {
            requests,
            frames,
            daemon_version: remote.version(),
            schema_version: subscription.schema_version(),
            reducer,
        })
    }
}

/// An established run-stream subscription with opaque transport resources.
pub struct RunStreamSubscription {
    requests: AsyncRequestSender,
    frames: AsyncDaemonFrameReceiver,
    daemon_version: ProtocolVersionDto,
    schema_version: SchemaVersionDto,
    reducer: RunSubscriptionReducer,
}

impl RunStreamSubscription {
    /// Receives and applies one uncorrelated daemon run-stream frame.
    ///
    /// A returned resync is locally generated for a detected cursor gap. A
    /// daemon-originated matching resync clears the reducer and returns `None`.
    ///
    /// # Errors
    ///
    /// Returns a typed framing, protocol, or scope-validation error.
    pub async fn receive(&mut self) -> DtoResult<Option<RunResyncDto>> {
        match self.frames.receive().await? {
            ProtocolDaemonFrameDto::RunStream(frame) => self.reducer.apply_frame(frame),
            ProtocolDaemonFrameDto::Response(_) => Err(invalid_response()),
        }
    }

    /// Sends a new subscription request from the last valid cursor and applies
    /// its immediate correlated replay, resync, or error response.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, protocol, or scoped-response error. The
    /// current reducer state is retained if the reply is invalid or rejected.
    pub async fn request_replay(&mut self) -> DtoResult<()> {
        let subscription = SubscribeRunCommandDto::new(
            self.schema_version,
            self.reducer.session_id(),
            self.reducer.run_id(),
            self.reducer.last_cursor(),
        );
        let correlation_id = CorrelationIdDto::new();
        let request = RunSubscriptionRequestEnvelopeDto::new(
            local_protocol_version(),
            correlation_id,
            ProtocolMessageDto::new(self.schema_version, subscription),
        );
        self.requests.send_run_subscription(&request).await?;
        let response = match self.frames.receive().await? {
            ProtocolDaemonFrameDto::Response(response)
                if response.correlation_id() == correlation_id
                    && response.protocol_version() == self.daemon_version
                    && response.message().schema_version() == self.schema_version =>
            {
                response
            }
            _ => return Err(invalid_response()),
        };
        let response = match response.message().payload() {
            ProtocolResponsePayloadDto::RunSubscription(response) => response.clone(),
            _ => return Err(invalid_response()),
        };
        self.reducer.apply_initial(response)
    }

    /// Returns the state reducer for this fixed run scope.
    #[must_use]
    pub const fn reducer(&self) -> &RunSubscriptionReducer {
        &self.reducer
    }

    /// Returns mutable reducer state for adapter-directed recovery.
    #[must_use]
    pub const fn reducer_mut(&mut self) -> &mut RunSubscriptionReducer {
        &mut self.reducer
    }
}

/// A run-scoped reducer preserving daemon-authoritative snapshots and cursor order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunSubscriptionReducer {
    session_id: intention_types::SessionId,
    run_id: intention_types::RunId,
    snapshot: Option<RunSnapshotDto>,
    last_cursor: Option<RunEventCursorDto>,
    reasoning_content: String,
    historical_reasoning_cursors: BTreeSet<RunEventCursorDto>,
    history_unavailable: bool,
}

impl RunSubscriptionReducer {
    /// Creates empty local state fixed to one session and run.
    #[must_use]
    pub const fn new(
        session_id: intention_types::SessionId,
        run_id: intention_types::RunId,
    ) -> Self {
        Self {
            session_id,
            run_id,
            snapshot: None,
            last_cursor: None,
            reasoning_content: String::new(),
            historical_reasoning_cursors: BTreeSet::new(),
            history_unavailable: false,
        }
    }

    /// Applies the correlated first reply for this subscription.
    ///
    /// # Errors
    ///
    /// Returns a typed scoped-response error without mutation for wrong scope or
    /// an incomplete replay tail.
    pub fn apply_initial(&mut self, response: RunSubscriptionResponseDto) -> DtoResult<()> {
        match response {
            RunSubscriptionResponseDto::Replay(replay) => {
                self.ensure_scope(replay.snapshot().session_id(), replay.snapshot().run_id())?;
                let tail = replay.tail();
                if tail.session_id() != self.session_id
                    || tail.run_id() != self.run_id
                    || tail.after_cursor() != replay.snapshot().cursor()
                    || tail.has_more()
                {
                    return Err(ErrorDto::validation(
                        "invalid_run_subscription",
                        "run replay must contain one complete matching tail",
                    ));
                }
                let mut next = Self::new(self.session_id, self.run_id);
                next.snapshot = Some(replay.snapshot().clone());
                next.last_cursor = Some(replay.snapshot().cursor());
                next.apply_replay_tail(tail.facts())?;
                *self = next;
                Ok(())
            }
            RunSubscriptionResponseDto::Resync(resync) => self.apply_resync(resync),
            RunSubscriptionResponseDto::Error(error) => Err(error),
        }
    }

    /// Applies one uncorrelated run-stream frame.
    ///
    /// # Errors
    ///
    /// Returns a typed scoped-response error without mutation for wrong scope or
    /// frames sent after an initial unavailable-history response.
    pub fn apply_frame(&mut self, frame: RunStreamFrameDto) -> DtoResult<Option<RunResyncDto>> {
        if self.history_unavailable {
            return Err(invalid_response());
        }
        match frame {
            RunStreamFrameDto::LiveBatch(batch) => self.apply_live_batch(batch),
            RunStreamFrameDto::Snapshot(frame) => {
                self.ensure_scope(frame.session_id(), frame.run_id())?;
                self.snapshot = Some(frame.snapshot().clone());
                self.last_cursor = Some(frame.snapshot().cursor());
                self.historical_reasoning_cursors.clear();
                Ok(None)
            }
            RunStreamFrameDto::Resync(resync) => {
                self.apply_resync(resync)?;
                Ok(None)
            }
        }
    }

    /// Applies a live batch, returning a local cursor-gap resync without mutation.
    ///
    /// # Errors
    ///
    /// Returns a typed scope-validation error without mutation for another run.
    pub fn apply_live_batch(&mut self, batch: RunLiveBatchDto) -> DtoResult<Option<RunResyncDto>> {
        self.ensure_scope(batch.session_id(), batch.run_id())?;
        let last = self.last_cursor.map_or(0, RunEventCursorDto::value);
        if let Some(first_new) = batch
            .facts()
            .iter()
            .find(|fact| fact.cursor().value() > last)
            && first_new.cursor().value() != last.saturating_add(1)
        {
            return Ok(Some(RunResyncDto::new(
                self.session_id,
                self.run_id,
                RunResyncReasonDto::CursorGap,
            )));
        }
        let snapshot_cursor = self
            .snapshot
            .as_ref()
            .map_or(RunEventCursorDto::new(0), RunSnapshotDto::cursor);
        let mut next_cursor = self.last_cursor;
        let mut appended_reasoning = String::new();
        let mut historical_cursors = self.historical_reasoning_cursors.clone();
        for fact in batch.facts() {
            if fact.cursor().value() <= snapshot_cursor.value() {
                if let ModelRunFactInputDto::ReasoningDeltaRecorded { content, .. } = fact.input()
                    && historical_cursors.insert(fact.cursor())
                {
                    appended_reasoning.push_str(content);
                }
                continue;
            }
            if fact.cursor().value() <= last {
                continue;
            }
            next_cursor = Some(fact.cursor());
        }
        self.reasoning_content.push_str(&appended_reasoning);
        self.historical_reasoning_cursors = historical_cursors;
        self.last_cursor = next_cursor;
        Ok(None)
    }

    fn apply_replay_tail(&mut self, facts: &[ModelRunFactDto]) -> DtoResult<()> {
        let snapshot_cursor = self
            .snapshot
            .as_ref()
            .map_or(RunEventCursorDto::new(0), RunSnapshotDto::cursor);
        let mut next_cursor = snapshot_cursor;
        let mut reasoning_content = String::new();
        let mut reasoning_cursors = BTreeSet::new();
        for fact in facts {
            let expected_cursor = next_cursor.value().checked_add(1).ok_or_else(|| {
                ErrorDto::validation("invalid_run_subscription", "run replay cursor overflow")
            })?;
            if fact.cursor().value() != expected_cursor {
                return Err(ErrorDto::validation(
                    "invalid_run_subscription",
                    "run replay tail requires contiguous facts",
                ));
            }
            if let ModelRunFactInputDto::ReasoningDeltaRecorded { content, .. } = fact.input()
                && reasoning_cursors.insert(fact.cursor())
            {
                reasoning_content.push_str(content);
            }
            next_cursor = fact.cursor();
        }
        self.reasoning_content = reasoning_content;
        self.historical_reasoning_cursors = reasoning_cursors;
        self.last_cursor = Some(next_cursor);
        Ok(())
    }

    fn apply_resync(&mut self, resync: RunResyncDto) -> DtoResult<()> {
        self.ensure_scope(resync.session_id(), resync.run_id())?;
        self.snapshot = None;
        self.last_cursor = None;
        self.reasoning_content.clear();
        self.historical_reasoning_cursors.clear();
        self.history_unavailable = resync.reason() == RunResyncReasonDto::HistoryUnavailable;
        Ok(())
    }

    fn ensure_scope(
        &self,
        session_id: intention_types::SessionId,
        run_id: intention_types::RunId,
    ) -> DtoResult<()> {
        if session_id == self.session_id && run_id == self.run_id {
            Ok(())
        } else {
            Err(ErrorDto::validation(
                "invalid_run_subscription",
                "run subscription data belongs to another run scope",
            ))
        }
    }

    /// Returns the fixed session identity.
    #[must_use]
    pub const fn session_id(&self) -> intention_types::SessionId {
        self.session_id
    }
    /// Returns the fixed run identity.
    #[must_use]
    pub const fn run_id(&self) -> intention_types::RunId {
        self.run_id
    }
    /// Returns the authoritative daemon snapshot, if one has been accepted.
    #[must_use]
    pub fn snapshot(&self) -> Option<RunSnapshotDto> {
        self.snapshot.clone()
    }
    /// Returns the last accepted durable run cursor.
    #[must_use]
    pub const fn last_cursor(&self) -> Option<RunEventCursorDto> {
        self.last_cursor
    }
    /// Returns tail-only historical reasoning accepted after an authoritative snapshot.
    #[must_use]
    pub fn reasoning_content(&self) -> &str {
        &self.reasoning_content
    }
    /// Reports whether the initial reply failed closed for unavailable history.
    #[must_use]
    pub const fn history_unavailable(&self) -> bool {
        self.history_unavailable
    }
}

/// Stateful recovery for one replay-only snapshot-and-tail subscription.
///
/// The local transport deliberately closes every request connection. This handle
/// therefore records the latest accepted sequence and creates a fresh negotiated
/// subscription request after a disconnect. It does not imply a live stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionSubscriptionRecovery {
    schema_version: SchemaVersionDto,
    session_id: SessionId,
    run_id: Option<intention_types::RunId>,
    requested_mode: intention_domain::RunModeDto,
    reducer: SessionSubscriptionReducer,
}

impl SessionSubscriptionRecovery {
    /// Creates recovery state for one typed subscription.
    #[must_use]
    pub const fn new(subscription: SubscribeSessionCommandDto) -> Self {
        let session_id = subscription.session_id();
        Self {
            schema_version: subscription.schema_version(),
            session_id,
            run_id: subscription.run_id(),
            requested_mode: subscription.requested_mode(),
            reducer: SessionSubscriptionReducer::new(session_id),
        }
    }

    /// Requests a fresh snapshot/tail from the last accepted sequence.
    ///
    /// `Ok(false)` means a consistent state was applied. `Ok(true)` means the
    /// daemon required resynchronization and the local projection was cleared.
    ///
    /// # Errors
    ///
    /// Returns a typed transport, protocol, or continuity error.
    pub fn recover(&mut self, client: &IntentionClient) -> DtoResult<bool> {
        let subscription = SubscribeSessionCommandDto::with_run_id(
            self.schema_version,
            self.session_id,
            self.run_id,
            self.reducer.last_sequence(),
            self.requested_mode,
        );
        self.reducer.apply(client.subscribe(subscription)?)
    }

    /// Returns the accepted daemon checkpoint, if recovery has succeeded.
    #[must_use]
    pub fn snapshot(&self) -> Option<SessionSnapshotDto> {
        self.reducer.snapshot()
    }

    /// Returns the latest locally accepted daemon event sequence.
    #[must_use]
    pub const fn last_sequence(&self) -> Option<SessionEventSequenceDto> {
        self.reducer.last_sequence()
    }
}

/// A sequence-aware local reducer for snapshot-plus-tail subscription recovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionSubscriptionReducer {
    session_id: SessionId,
    snapshot: Option<SessionSnapshotDto>,
    last_sequence: Option<SessionEventSequenceDto>,
    seen_events: BTreeSet<EventId>,
}

impl SessionSubscriptionReducer {
    /// Creates an empty local reducer for one daemon-owned session.
    #[must_use]
    pub const fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            snapshot: None,
            last_sequence: None,
            seen_events: BTreeSet::new(),
        }
    }

    /// Applies a complete subscription recovery response.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the response belongs to another session or
    /// contains a non-contiguous tail. A resync instruction clears local state and
    /// is returned as `Ok(true)`.
    pub fn apply(&mut self, response: SessionSubscriptionResponseDto) -> DtoResult<bool> {
        match response {
            SessionSubscriptionResponseDto::ResyncRequired(resync) => {
                if resync.session_id() != self.session_id {
                    return Err(ErrorDto::validation(
                        "invalid_subscription_session",
                        "subscription response belongs to another session",
                    ));
                }
                self.snapshot = None;
                self.last_sequence = None;
                self.seen_events.clear();
                Ok(true)
            }
            SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail } => {
                if snapshot.session_id() != self.session_id || tail.session_id() != self.session_id
                {
                    return Err(ErrorDto::validation(
                        "invalid_subscription_session",
                        "subscription response belongs to another session",
                    ));
                }
                let snapshot_sequence = snapshot.at_sequence();
                self.snapshot = Some(snapshot);
                self.last_sequence = Some(snapshot_sequence);
                self.seen_events.clear();
                for event in tail.events() {
                    self.apply_event(event)?;
                }
                Ok(false)
            }
        }
    }

    /// Applies one ordered live event, ignoring a duplicate or stale sequence.
    ///
    /// # Errors
    ///
    /// Returns a validation error for another session or a non-contiguous future
    /// sequence, which tells the adapter to request snapshot recovery.
    pub fn apply_event(
        &mut self,
        event: &EventEnvelopeDto<intention_domain::DomainEventDto>,
    ) -> DtoResult<()> {
        if event.session_id() != self.session_id {
            return Err(ErrorDto::validation(
                "invalid_subscription_session",
                "subscription event belongs to another session",
            ));
        }
        if self.seen_events.contains(&event.event_id()) {
            return Ok(());
        }
        let expected = self.last_sequence.map_or(0, SessionEventSequenceDto::value);
        if event.sequence().value() <= expected {
            return Ok(());
        }
        if event.sequence().value() != expected.saturating_add(1) {
            return Err(ErrorDto::validation(
                "subscription_sequence_gap",
                "subscription event sequence requires snapshot recovery",
            ));
        }
        self.seen_events.insert(event.event_id());
        self.last_sequence = Some(event.sequence());
        Ok(())
    }

    /// Returns the last applied daemon sequence, if a snapshot has been accepted.
    #[must_use]
    pub const fn last_sequence(&self) -> Option<SessionEventSequenceDto> {
        self.last_sequence
    }

    /// Returns the current accepted snapshot checkpoint.
    #[must_use]
    pub fn snapshot(&self) -> Option<SessionSnapshotDto> {
        self.snapshot.clone()
    }
}

struct StartupLock {
    file: std::fs::File,
}

impl StartupLock {
    fn acquire(endpoint: &LocalEndpoint) -> DtoResult<Self> {
        let path = startup_lock_path(endpoint)?;
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
            .open(&path)
            .map_err(|_| unavailable("startup_lock_unavailable"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .map_err(|_| unavailable("startup_lock_unavailable"))?;
        }
        fs4::FileExt::lock(&file).map_err(|_| unavailable("startup_lock_unavailable"))?;
        Ok(Self { file })
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

fn is_daemon_unavailable(error: &ErrorDto) -> bool {
    matches!(
        error.code(),
        "local_daemon_unavailable" | "local_daemon_connection_unavailable"
    )
}

/// Formats the current DTO schema version as the protocol `major.minor` text.
#[must_use]
fn schema_version_string() -> String {
    format!("{}.{}", SCHEMA_VERSION.major(), SCHEMA_VERSION.minor())
}

fn is_daemon_starting(error: &ErrorDto) -> bool {
    error.code() == "local_daemon_starting"
}

fn invalid_response() -> ErrorDto {
    ErrorDto::validation(
        "invalid_local_protocol_response",
        "the local daemon returned an unexpected protocol response",
    )
}

fn unavailable(code: &'static str) -> ErrorDto {
    ErrorDto::unavailable(code, "the local daemon connection is unavailable")
}
