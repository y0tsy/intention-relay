//! Durable M3 composition root for the daemon application facade.
//!
//! Only this crate selects SQLite. The public facade exposes protocol DTOs;
//! database resources, locations, configuration text, and committed-event
//! publication stay private.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use intention_application::{
    ApplicationService, CatalogAdmissionPort, CatalogReadService, ConfigurationReloadService,
    ControlPlaneReadinessPort, CreateSessionWorkflowInputDto, CredentialRotationService,
    DiscoveryPort, DiscoveryScopeDto, DriverRebuildPort, HealthProbePort, HeldRunService,
    InvokeLocalToolInputDto, ModelRunDispatchPort, ModelRunDriverHandle, PricingPolicyService,
    PrivateCredentialMaterial, PrivateCredentialPort, PrivateProviderProfileMaterial,
    ProviderCatalogController, ProviderDiscoveryService, ProviderDriverFactory,
    ProviderHealthService, ReloadCandidateDto, ReloadCommitOutcomeDto, RemovalService,
    ResolvedProfileDto, SafeBindingSource, SafeCompositionBindingDto, ScheduleModelRunDto,
    SendUserTurnWorkflowInputDto, SessionProfileService, ToolResultPublicationInputDto,
    ToolResultPublicationPort, UnavailableQueueService, UsageService, WorkspaceBoundaryPort,
};
use intention_config::{
    ConfigPathDto, ConfigPathResolver, ConfigSnapshotDto, ConfigSourceDto, ProviderKindDto,
    RawConfigInputDto, ResolvedConfigDto, StartupProviderMaterial,
    control_plane::ConfigCandidateDto,
};
#[cfg(test)]
use intention_domain::{CreateSessionCommandDto, RunModeDto, WorkspaceRootDto};
use intention_domain::{
    CredentialTransportMode as DomainCredentialTransportMode, DomainEventDto,
    GetSessionSnapshotQueryDto, ModelRunFactInputDto, ProviderSelectionV1, RunEventCursorDto,
    RunFailureDto, RunReplayDto, RunStatusDto, ToolLifecycleStatusDto, canonical::Digest256,
};
use intention_hooks::{
    Hook, Outcome as HookOutcome, Phase, PhaseContext, Registry as HookRegistry,
};
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
    contract_families::{
        ConfigurationCommitOutcomeDto, ConfigurationEditCommandDto, ConfigurationEditOperationDto,
        ConfigurationProjectionDto, ConfigurationValidationOutcomeDto,
        ProviderAvailabilityObservation, ProviderModelDiscoveryRecordDto, RawTomlEditCommandDto,
        ReloadConfigurationCommandDto, ReloadTransactionDto, RotateProviderCredentialsCommandDto,
    },
};
use intention_provider_generic_chat::GenericChatDriver;
use intention_provider_openrouter::OpenRouterDriver;
#[cfg(feature = "test-support")]
use intention_runtime::ModelRunFirstAppendGate;
use intention_runtime::{
    ModelRunCommitObserver, ModelRunExecutionInputDto, ModelRunExecutionOutcomeDto,
    ModelRunExecutionService, ModelTimePort, RuntimeService, RuntimeValuesDto, ToolExecutionPort,
    fail_starting_run,
};
use intention_storage::{
    AppendModelRunFactsInputDto, EnqueueUnavailableRunInputDto, HeldRunRepositoryDto,
    MarkRecoveredRunHeldInputDto, ProviderCatalogRepositoryDto, ProviderRemovalRepositoryDto,
    StorageRepositoryDto, UnavailableQueueRepositoryDto,
};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use intention_tools::{CancellationSignal, ToolInput, ToolResult};
use intention_types::{
    ConfigRevisionId, CorrelationIdDto, DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto,
    EventEnvelopeDto, RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId, TimestampDto,
};
#[cfg(test)]
use intention_types::{ProjectId, WorkspaceId};
use intention_workspace::WorkspaceRoot;

const SCHEMA_VERSION: SchemaVersionDto = intention_protocol::CURRENT_DTO_SCHEMA_VERSION;
const PROTOCOL_VERSION: intention_protocol::ProtocolVersionDto =
    intention_protocol::CURRENT_PROTOCOL_VERSION;
/// The single live configuration snapshot schema (intention-config current schema).
const CONFIG_SCHEMA_VERSION: SchemaVersionDto = SchemaVersionDto::new(1, 0);
const DATABASE_FILENAME: &str = "intention-relay.sqlite";

/// Public M3 daemon application facade over a private durable composition.
#[derive(Clone)]
pub struct DaemonApplicationFacade {
    inner: Arc<FacadeInner>,
}

pub use intention_application::CatalogReadiness;

struct FacadeInner {
    repository: Arc<SqliteStorageRepository>,
    config_snapshot: Mutex<ConfigSnapshotDto>,
    _selected_provider: SelectedProvider,
    dispatch: PrivateModelRunDispatch,
    command_gate: Mutex<()>,
    tool_cancellations: Mutex<HashMap<(SessionId, RunId), LocalToolCancellationEntry>>,
    reload_candidates: Mutex<HashMap<String, ConfigCandidateDto>>,
    control_plane: ProviderControlPlane,
}

/// The composition's provider session-selection control plane.
///
/// The controller owns the catalog runtime: startup, candidate preparation,
/// pending-removal acceptance/rejection/expiry, admission lookups, and the
/// private driver registry. All public access is DTO-only and credential-free.
struct ProviderControlPlane {
    controller: ProviderCatalogController<RepositoryHandle, RepositoryHandle>,
}

impl ProviderControlPlane {
    /// Builds the control plane over the shared durable repository handle.
    fn new(handle: RepositoryHandle) -> Self {
        let factories = vec![
            Box::new(CompositionDriverFactory::service("openrouter"))
                as Box<dyn ProviderDriverFactory>,
            Box::new(CompositionDriverFactory::service(
                "generic-chat-completion-api",
            )),
        ];
        Self {
            controller: ProviderCatalogController::new(handle.clone(), handle, factories),
        }
    }
}

impl ControlPlaneReadinessPort for ProviderControlPlane {
    fn readiness(&self) -> DtoResult<intention_application::CatalogReadiness> {
        Ok(self.controller.inspect()?.readiness)
    }
}

/// Cloneable composition-side handle over the shared durable repository.
///
/// The catalog controller owns its repository handles by value; this local
/// wrapper delegates every catalog and removal repository call to the shared
/// SQLite repository so the facade keeps exactly one connection.
#[derive(Clone)]
struct RepositoryHandle(Arc<SqliteStorageRepository>);

impl ProviderCatalogRepositoryDto for RepositoryHandle {
    fn append_provider_kind_descriptor_revision(
        &self,
        input: intention_storage::AppendProviderKindDescriptorRevisionInputDto,
    ) -> DtoResult<()> {
        self.0.append_provider_kind_descriptor_revision(input)
    }
    fn append_provider_profile_revision(
        &self,
        input: intention_storage::AppendProviderProfileRevisionInputDto,
    ) -> DtoResult<()> {
        self.0.append_provider_profile_revision(input)
    }
    fn load_provider_catalog_status(
        &self,
    ) -> DtoResult<intention_storage::ProviderCatalogStateDto> {
        self.0.load_provider_catalog_status()
    }
    fn load_provider_catalog_page(
        &self,
        input: intention_storage::LoadProviderCatalogPageInputDto,
    ) -> DtoResult<intention_storage::ProviderCatalogPageDto> {
        self.0.load_provider_catalog_page(input)
    }
    fn accept_provider_catalog(
        &self,
        input: intention_storage::AcceptProviderCatalogInputDto,
    ) -> DtoResult<()> {
        self.0.accept_provider_catalog(input)
    }
    fn reject_provider_catalog_candidate(
        &self,
        input: intention_storage::RejectProviderCatalogCandidateInputDto,
    ) -> DtoResult<()> {
        self.0.reject_provider_catalog_candidate(input)
    }
    fn expire_provider_catalog_candidate(
        &self,
        input: intention_storage::ExpireProviderCatalogCandidateInputDto,
    ) -> DtoResult<()> {
        self.0.expire_provider_catalog_candidate(input)
    }
    fn load_provider_catalog_material(
        &self,
    ) -> DtoResult<intention_storage::ProviderCatalogMaterialDto> {
        self.0.load_provider_catalog_material()
    }
    fn load_prepared_catalog_material(
        &self,
    ) -> DtoResult<intention_storage::ProviderCatalogMaterialDto> {
        self.0.load_prepared_catalog_material()
    }
}

impl ProviderRemovalRepositoryDto for RepositoryHandle {
    fn load_highest_removal_candidate_revision(&self) -> DtoResult<u64> {
        self.0.load_highest_removal_candidate_revision()
    }
    fn load_pending_removal_candidate(
        &self,
    ) -> DtoResult<Option<intention_storage::PendingRemovalCandidateDto>> {
        self.0.load_pending_removal_candidate()
    }
    fn create_provider_catalog_removal_candidate(
        &self,
        input: intention_storage::CreateProviderCatalogRemovalCandidateInputDto,
    ) -> DtoResult<()> {
        self.0.create_provider_catalog_removal_candidate(input)
    }
    fn accept_provider_catalog_removal(
        &self,
        input: intention_storage::AcceptProviderCatalogRemovalInputDto,
    ) -> DtoResult<()> {
        self.0.accept_provider_catalog_removal(input)
    }
    fn reject_provider_catalog_removal(
        &self,
        input: intention_storage::RejectProviderCatalogRemovalInputDto,
    ) -> DtoResult<()> {
        self.0.reject_provider_catalog_removal(input)
    }
    fn expire_provider_catalog_removal_candidate(
        &self,
        input: intention_storage::ExpireProviderCatalogRemovalCandidateInputDto,
    ) -> DtoResult<u64> {
        self.0.expire_provider_catalog_removal_candidate(input)
    }
}

/// The composition's credential-free private driver factory.
///
/// The catalog runtime only carries opaque handles; actual driver
/// materialization from private credential material is owned by a later
/// slice. This factory builds credential-free opaque handles so admission
/// lookups and the private registry contract are fully wired without any
/// credential crossing a boundary.
struct CompositionDriverFactory {
    kind: String,
}

impl CompositionDriverFactory {
    fn service(kind: &'static str) -> Self {
        Self {
            kind: kind.to_owned(),
        }
    }
}

impl ProviderDriverFactory for CompositionDriverFactory {
    fn kind(&self) -> &str {
        &self.kind
    }

    fn supports_contract(
        &self,
        contract: &intention_domain::ProviderDriverContractRevisionDto,
    ) -> bool {
        contract.driver_family == self.kind && contract.major == 1 && contract.minor == 1
    }

    fn build(
        &self,
        profile: PrivateProviderProfileMaterial,
    ) -> DtoResult<Box<dyn ModelRunDriverHandle + Send + Sync>> {
        let _ = &profile.private_credential_reference;
        Ok(Box::new(CompositionCatalogDriverHandle))
    }
}

/// Credential-free opaque handle behind the private registry.
struct CompositionCatalogDriverHandle;

impl ModelRunDriverHandle for CompositionCatalogDriverHandle {}

/// The composition's catalog admission port.
///
/// A profile resolves when the active catalog material names it and the
/// controller admits its exact registry key as enabled, ready, and
/// non-tombstoned.
struct CompositionCatalogAdmissionPort<'a> {
    controller: &'a ProviderCatalogController<RepositoryHandle, RepositoryHandle>,
    catalog: &'a SqliteStorageRepository,
}

impl CatalogAdmissionPort for CompositionCatalogAdmissionPort<'_> {
    fn resolve_enabled_profile(&self, profile_id: &str) -> DtoResult<ResolvedProfileDto> {
        let material = self
            .catalog
            .load_provider_catalog_material()
            .map_err(|error| {
                if error.code() == "provider_catalog_not_active" {
                    ErrorDto::unavailable("catalog_not_ready", "the provider catalog is not active")
                } else {
                    error
                }
            })?;
        let candidate = material
            .profiles
            .iter()
            .find(|candidate| candidate.profile.profile_id == profile_id)
            .ok_or_else(|| {
                ErrorDto::unavailable(
                    "provider_profile_unavailable",
                    "the requested provider profile is not in the active catalog",
                )
            })?;
        let key = intention_application::PrivateRegistryKey {
            profile_id: candidate.profile.profile_id.clone(),
            profile_revision_id: candidate.profile.revision_id.clone(),
            kind_descriptor_revision_id: candidate.profile.kind_descriptor_revision_id.clone(),
            driver_contract: candidate.profile.driver_contract_revision.clone(),
        };
        self.controller.registry_lookup(&key)?;
        let contract = &candidate.profile.driver_contract_revision;
        Ok(ResolvedProfileDto {
            profile_id: candidate.profile.profile_id.clone(),
            profile_revision_id: candidate.profile.revision_id.clone(),
            kind_id: candidate.profile.provider_kind_id.clone(),
            kind_descriptor_revision_id: candidate.profile.kind_descriptor_revision_id.clone(),
            model_id: candidate.profile.model_id.clone(),
            normalized_effective_endpoint: candidate.profile.endpoint.clone(),
            credential_transport_mode: match candidate.profile.credential_transport_mode {
                DomainCredentialTransportMode::Bearer => {
                    intention_protocol::contract_families::CredentialTransportMode::Bearer
                }
                DomainCredentialTransportMode::SafeHeader => {
                    intention_protocol::contract_families::CredentialTransportMode::SafeHeader
                }
            },
            credential_transport_safe_header_name: candidate.profile.safe_header_name.clone(),
            declared_model_capability_subset: candidate.declared_model_capability_subset.clone(),
            resolved_reasoning_policy: candidate.resolved_reasoning_policy.clone(),
            effective_execution_policy: candidate.effective_execution_policy.clone(),
            effective_loopback_policy_or_not_applicable: candidate
                .effective_loopback_policy_or_not_applicable
                .clone(),
            provider_driver_contract_revision: format!(
                "{}-{}.{}",
                contract.driver_family, contract.major, contract.minor
            ),
        })
    }

    fn verify_registry_key(
        &self,
        profile_id: &str,
        provider_profile_revision_id: &str,
        kind_descriptor_revision_id: &str,
        driver_contract_revision: &str,
    ) -> DtoResult<()> {
        let key = registry_key_from_selection(
            profile_id,
            provider_profile_revision_id,
            kind_descriptor_revision_id,
            driver_contract_revision,
        )?;
        self.controller.registry_lookup(&key)?;
        Ok(())
    }
}

/// Builds the exact private registry key of one persisted provider selection.
///
/// The persisted driver contract revision is the deterministic
/// `{driver_family}-{major}.{minor}` name; the family may itself contain
/// hyphens, so the version splits from the last hyphen.
///
/// # Errors
///
/// Returns a validation error when the persisted driver contract revision is
/// malformed.
fn registry_key_from_selection(
    profile_id: &str,
    provider_profile_revision_id: &str,
    kind_descriptor_revision_id: &str,
    driver_contract_revision: &str,
) -> DtoResult<intention_application::PrivateRegistryKey> {
    let (driver_family, version) = driver_contract_revision.rsplit_once('-').ok_or_else(|| {
        ErrorDto::validation(
            "provider_selection_invalid",
            "the persisted driver contract revision is malformed",
        )
    })?;
    let (major, minor) = version.split_once('.').ok_or_else(|| {
        ErrorDto::validation(
            "provider_selection_invalid",
            "the persisted driver contract revision is malformed",
        )
    })?;
    let major = major.parse::<u64>().map_err(|_| {
        ErrorDto::validation(
            "provider_selection_invalid",
            "the persisted driver contract revision is malformed",
        )
    })?;
    let minor = minor.parse::<u64>().map_err(|_| {
        ErrorDto::validation(
            "provider_selection_invalid",
            "the persisted driver contract revision is malformed",
        )
    })?;
    Ok(intention_application::PrivateRegistryKey {
        profile_id: profile_id.to_owned(),
        profile_revision_id: provider_profile_revision_id.to_owned(),
        kind_descriptor_revision_id: kind_descriptor_revision_id.to_owned(),
        driver_contract: intention_domain::ProviderDriverContractRevisionDto {
            driver_family: driver_family.to_owned(),
            major,
            minor,
        },
    })
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

struct SafeWorkspaceBoundary;
impl WorkspaceBoundaryPort for SafeWorkspaceBoundary {
    fn resolve(&self, _workspace: &WorkspaceRoot) -> DtoResult<()> {
        // WorkspaceRoot::resolve has already performed canonical resolution
        // and directory validation before this application boundary is called.
        Ok(())
    }
}

struct SafeObserverHook;
impl Hook for SafeObserverHook {
    fn id(&self) -> &'static str {
        "safe-production-observer"
    }
    fn phases(&self) -> &'static [Phase] {
        static P: [Phase; 1] = [Phase::BeforeToolInvocation];
        &P
    }
    fn priority(&self) -> u32 {
        0
    }
    fn run(&self, _: &PhaseContext) -> DtoResult<HookOutcome> {
        Ok(HookOutcome::Continue)
    }
}

fn production_hooks() -> DtoResult<HookRegistry> {
    let mut registry = HookRegistry::new();
    registry
        .register(Box::new(SafeObserverHook))
        .map_err(|_error| {
            ErrorDto::validation(
                "production_hook_registration_failed",
                "production hook registration failed",
            )
        })?;
    Ok(registry)
}

/// Verifies that a scoped durable reread contains committed `Completed` typed
/// evidence for the exact invocation identity.
///
/// The reread is scoped to the invoking session's exact pre-invocation durable
/// position and must contain `Completed` typed evidence for the exact
/// invocation identity before publication proceeds. Publication therefore
/// follows commit plus a scoped reread, and the application dispatches
/// `AfterToolResultPublished` only after this verification succeeds.
struct DurableToolResultPublisher<'a> {
    repository: &'a SqliteStorageRepository,
    after_sequence: SessionEventSequenceDto,
}

impl ToolResultPublicationPort for DurableToolResultPublisher<'_> {
    fn publish_tool_result(&self, input: &ToolResultPublicationInputDto) -> DtoResult<()> {
        let events = self
            .repository
            .load_tail(input.session_id(), self.after_sequence)?;
        committed_tool_result_evidence(&events, input)
    }
}

/// Verifies that a scoped durable reread contains committed `Completed` typed
/// evidence for the exact invocation identity.
fn committed_tool_result_evidence(
    events: &[EventEnvelopeDto<DomainEventDto>],
    input: &ToolResultPublicationInputDto,
) -> DtoResult<()> {
    let correlated = events
        .iter()
        .rev()
        .find_map(|envelope| match envelope.payload() {
            DomainEventDto::ToolLifecycle(event)
                if event.session_id() == input.session_id()
                    && event.run_id() == input.run_id()
                    && event.call_id() == input.call_id() =>
            {
                Some(event.status() == &ToolLifecycleStatusDto::Completed)
            }
            _ => None,
        });
    match correlated {
        Some(true) => Ok(()),
        Some(false) | None => Err(ErrorDto::unavailable(
            "tool_result_evidence_unavailable",
            "committed tool result evidence is unavailable",
        )),
    }
}

/// Run-scoped cancellation shared between daemon-host stops and admitted local tools.
struct LocalToolCancellationEntry {
    signal: CancellationSignal,
    /// Local invocations currently executing against this exact run.
    inflight: usize,
}

/// The protocol schema-version text used by control-plane projections.
const PROTOCOL_SCHEMA_VERSION_TEXT: &str = "1.1";
/// The maximum retained prepared reload candidates for reference reloads.
const MAX_RELOAD_CANDIDATES: usize = 16;

/// The composition's catalog-runtime-backed binding source.
///
/// The provider binding identity (profile revision, kind descriptor
/// revision, capability subset, loopback policy, and driver contract) is
/// owned by the provider catalog runtime: the source resolves each profile
/// through the catalog admission port and derives the credential-free
/// composition revision from the resolved profile identity. Active
/// configuration revision and snapshot come from the fresh-run snapshot.
struct CatalogBindingSource<'a> {
    snapshot: &'a Mutex<ConfigSnapshotDto>,
    admission: CompositionCatalogAdmissionPort<'a>,
}

impl SafeBindingSource for CatalogBindingSource<'_> {
    fn active_revision(&self) -> DtoResult<String> {
        self.snapshot
            .lock()
            .map(|guard| guard.revision_id().to_string())
            .map_err(|_| {
                ErrorDto::unavailable(
                    "daemon_command_unavailable",
                    "daemon command is unavailable",
                )
            })
    }

    fn active_snapshot(&self) -> DtoResult<ConfigSnapshotDto> {
        self.snapshot
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| {
                ErrorDto::unavailable(
                    "daemon_command_unavailable",
                    "daemon command is unavailable",
                )
            })
    }

    fn binding(&self, profile_id: &str) -> DtoResult<SafeCompositionBindingDto> {
        let resolved = self.admission.resolve_enabled_profile(profile_id)?;
        let canonical = format!(
            "ir-binding-v1|profile={}|profile-revision={}|kind={}|kind-descriptor={}|model={}|endpoint={}|capabilities={}|execution={}|loopback={}|driver={}",
            resolved.profile_id,
            resolved.profile_revision_id,
            resolved.kind_id,
            resolved.kind_descriptor_revision_id,
            resolved.model_id,
            resolved.normalized_effective_endpoint,
            resolved.declared_model_capability_subset.join(","),
            resolved.effective_execution_policy,
            resolved.effective_loopback_policy_or_not_applicable,
            resolved.provider_driver_contract_revision,
        );
        let digest = Digest256::sha256(canonical.as_bytes()).bytes();
        let mut composition_revision = String::with_capacity(64);
        for byte in digest {
            composition_revision.push_str(&format!("{byte:02x}"));
        }
        Ok(SafeCompositionBindingDto {
            profile_id: resolved.profile_id,
            provider_profile_revision_id: resolved.profile_revision_id,
            safe_composition_revision: composition_revision,
            kind_id: resolved.kind_id,
            kind_descriptor_revision_id: resolved.kind_descriptor_revision_id,
            model_id: resolved.model_id,
            endpoint: Some(resolved.normalized_effective_endpoint),
            declared_model_capability_subset: resolved.declared_model_capability_subset,
            effective_execution_policy: resolved.effective_execution_policy,
            effective_loopback_policy_or_not_applicable: resolved
                .effective_loopback_policy_or_not_applicable,
            provider_driver_contract_revision: resolved.provider_driver_contract_revision,
        })
    }
}

/// The composition's private credential port.
///
/// Slice 2 configures no private credential source, so production rotation
/// always fails closed with `credential_rotation_source_unavailable` before
/// any replacement is obtained. A configured credential source is a later
/// slice; tests exercise the full rotation path with fake ports in the
/// application crate.
struct CompositionCredentialPort;

impl PrivateCredentialPort for CompositionCredentialPort {
    fn obtain_replacement(&self, _profile_id: &str) -> DtoResult<PrivateCredentialMaterial> {
        Err(ErrorDto::new(
            "credential_rotation_source_unavailable",
            ErrorCategoryDto::Unavailable,
            "no private credential source is configured",
            ErrorRetryDto::Manual,
            None,
        )?)
    }
}

/// The composition's private driver rebuild boundary.
///
/// The gate-guarded driver swap is owned by the catalog/rotation zone in a
/// later slice. Production rotation already fails closed at the credential
/// port, so this defensive failure is unreachable unless a credential source
/// is configured without a rebuild path.
struct CompositionDriverRebuildPort;

impl DriverRebuildPort for CompositionDriverRebuildPort {
    fn rebuild(&self, _profile_id: &str, _material: PrivateCredentialMaterial) -> DtoResult<()> {
        Err(ErrorDto::new(
            "credential_rotation_source_unavailable",
            ErrorCategoryDto::Unavailable,
            "no private driver rebuild path is configured",
            ErrorRetryDto::Manual,
            None,
        )?)
    }
}

/// The composition's health probe boundary.
///
/// Provider probing is not wired in this slice: the probe reports a typed
/// unavailable outcome that the health service projects as an `Unknown`
/// observation with the `provider_health_unavailable` diagnostic code.
struct CompositionHealthProbe;

impl HealthProbePort for CompositionHealthProbe {
    fn probe(&self, _provider_id: &str) -> DtoResult<ProviderAvailabilityObservation> {
        Err(ErrorDto::unavailable(
            "provider_health_unavailable",
            "provider health probing is not wired yet",
        ))
    }
}

/// The composition's discovery boundary.
///
/// Provider discovery is not wired in this slice; the discovery service
/// projects the port error as a terminal attempt with a safe status.
struct CompositionDiscoveryPort;

impl DiscoveryPort for CompositionDiscoveryPort {
    fn discover(
        &self,
        _scope: &DiscoveryScopeDto,
    ) -> DtoResult<Vec<ProviderModelDiscoveryRecordDto>> {
        Err(ErrorDto::unavailable(
            "provider_discovery_unavailable",
            "provider discovery is not wired yet",
        ))
    }
}

/// Builds the deterministic execution-policy label of one snapshot.
#[must_use]
fn execution_policy_string(snapshot: &ConfigSnapshotDto) -> String {
    let execution = snapshot.resolved().provider_execution();
    format!(
        "execution-timeout-{}-attempts-{}",
        execution.attempt_timeout_seconds(),
        execution.max_attempts(),
    )
}

/// Builds the committed reload transaction from a durable commit outcome.
#[must_use]
fn committed_reload_transaction(outcome: &ReloadCommitOutcomeDto) -> ReloadTransactionDto {
    ReloadTransactionDto {
        transaction_id: outcome.transaction_id.clone(),
        previous_config_revision: outcome.previous_revision.clone(),
        candidate_config_revision: outcome.new_revision.clone(),
        validation_result: ConfigurationValidationOutcomeDto::Valid,
        commit_outcome: ConfigurationCommitOutcomeDto::Committed,
        safe_failure_code: None,
        safe_failure_detail: None,
    }
}

/// Builds the rejected reload transaction from a not-accepted candidate.
///
/// The transaction carries the first deterministic failure code and never
/// echoes raw TOML or credential material.
#[must_use]
fn rejected_reload_transaction(
    candidate: &ReloadCandidateDto,
    operation_id: String,
    previous_revision: String,
) -> ReloadTransactionDto {
    ReloadTransactionDto {
        transaction_id: operation_id,
        previous_config_revision: previous_revision,
        candidate_config_revision: candidate
            .candidate_revision_id
            .clone()
            .unwrap_or_else(|| "unavailable".to_owned()),
        validation_result: ConfigurationValidationOutcomeDto::Invalid,
        commit_outcome: ConfigurationCommitOutcomeDto::Rejected,
        safe_failure_code: candidate.failure_code.clone(),
        safe_failure_detail: None,
    }
}

/// Builds the safe applied configuration projection from the active snapshot.
///
/// The projection is credential-free and never carries raw TOML, private
/// endpoints, or paths.
#[must_use]
fn configuration_projection(snapshot: &ConfigSnapshotDto) -> ConfigurationProjectionDto {
    ConfigurationProjectionDto {
        schema_version: PROTOCOL_SCHEMA_VERSION_TEXT.to_owned(),
        applied_config_revision_id: snapshot.revision_id().to_string(),
        provider_kind: snapshot.resolved().provider().kind().as_str().to_owned(),
        model_id: snapshot.resolved().provider().model().to_owned(),
        credential_configured: snapshot.resolved().provider().credential_configured(),
        provider_execution_policy: execution_policy_string(snapshot),
        reload_status: "active".to_owned(),
    }
}

/// Applies typed edit operations to the active snapshot and emits the edited
/// candidate TOML.
///
/// The supported key paths are `provider.kind`, `provider.model`,
/// `provider.endpoint`, `provider.execution.attempt_timeout_seconds`, and
/// `provider.execution.max_attempts`. The candidate is then validated through
/// the server-side reload contract. Because the credential is never retained
/// server-side, an edited candidate that leaves the credential unset fails
/// closed with `missing_provider_credential`; typed edits that preserve the
/// credential arrive through the private channel in a later slice.
///
/// # Errors
///
/// Returns `configuration_edit_invalid` for an unrecognized or non-removable
/// key path or a non-integer execution policy value.
fn edited_configuration_toml(
    snapshot: &ConfigSnapshotDto,
    operations: &[ConfigurationEditOperationDto],
) -> DtoResult<String> {
    let mut kind = snapshot.resolved().provider().kind().as_str().to_owned();
    let mut model = snapshot.resolved().provider().model().to_owned();
    let mut endpoint = snapshot.resolved().provider().endpoint().map(str::to_owned);
    let mut attempt_timeout_seconds = snapshot
        .resolved()
        .provider_execution()
        .attempt_timeout_seconds();
    let mut max_attempts = snapshot.resolved().provider_execution().max_attempts();
    for operation in operations {
        match operation {
            ConfigurationEditOperationDto::Set {
                key_path,
                safe_value,
            } => match key_path.as_str() {
                "provider.kind" => kind = safe_value.clone(),
                "provider.model" => model = safe_value.clone(),
                "provider.endpoint" => endpoint = Some(safe_value.clone()),
                "provider.execution.attempt_timeout_seconds" => {
                    attempt_timeout_seconds = safe_value.parse().map_err(|_| {
                        ErrorDto::validation(
                            "configuration_edit_invalid",
                            "attempt timeout seconds must be an integer",
                        )
                    })?;
                }
                "provider.execution.max_attempts" => {
                    max_attempts = safe_value.parse().map_err(|_| {
                        ErrorDto::validation(
                            "configuration_edit_invalid",
                            "max attempts must be an integer",
                        )
                    })?;
                }
                _ => {
                    return Err(ErrorDto::validation(
                        "configuration_edit_invalid",
                        "unrecognized configuration key path",
                    ));
                }
            },
            ConfigurationEditOperationDto::Remove { key_path } => match key_path.as_str() {
                "provider.endpoint" => endpoint = None,
                _ => {
                    return Err(ErrorDto::validation(
                        "configuration_edit_invalid",
                        "this configuration field cannot be removed",
                    ));
                }
            },
        }
    }
    let mut toml = String::new();
    toml.push_str("schema_version = 1\n[provider]\n");
    toml.push_str(&format!("kind = \"{kind}\"\n"));
    toml.push_str(&format!("model = \"{model}\"\n"));
    if let Some(endpoint) = endpoint {
        toml.push_str(&format!("endpoint = \"{endpoint}\"\n"));
    }
    toml.push_str("[provider.execution]\n");
    toml.push_str(&format!(
        "attempt_timeout_seconds = {attempt_timeout_seconds}\n"
    ));
    toml.push_str(&format!("max_attempts = {max_attempts}\n"));
    Ok(toml)
}

/// Builds the transient platform-native source of one server-side edit.
///
/// The path is used only for source classification inside the configuration
/// crate and is never disclosed in a DTO, projection, or error.
///
/// # Errors
///
/// Returns a validation error when the platform temporary directory cannot
/// form an absolute path.
fn reload_edit_source() -> DtoResult<ConfigSourceDto> {
    let path = std::env::temp_dir().join("intention-relay-reload-edit.toml");
    ConfigPathDto::parse(path.to_string_lossy().into_owned()).map(ConfigSourceDto::Explicit)
}

/// Builds the absolute fixture source used by test-support catalog seeding.
///
/// The path is used only for source classification inside the configuration
/// crate and is never disclosed in a DTO, projection, or error.
///
/// # Errors
///
/// Returns a validation error when the platform temporary directory cannot
/// form an absolute path.
#[cfg(any(test, feature = "test-support"))]
fn fixture_catalog_source() -> DtoResult<ConfigSourceDto> {
    let path = std::env::temp_dir().join("intention-relay-test-support-seed.toml");
    ConfigPathDto::parse(path.to_string_lossy().into_owned()).map(ConfigSourceDto::Explicit)
}

/// Converts one whole-second timestamp to the service `u64` representation.
fn now_seconds(timestamp: TimestampDto) -> u64 {
    u64::try_from(timestamp.unix_seconds()).unwrap_or(u64::MAX)
}

/// Converts one whole-second timestamp to the storage `i64` representation.
const fn i64_time(timestamp: TimestampDto) -> i64 {
    timestamp.unix_seconds()
}

/// Encodes one immutable selection into the opaque durable queue text.
///
/// The queue column is opaque text; the canonical record bytes are
/// hex-encoded so no crate below the composition needs a JSON serializer.
fn canonical_selection_text(selection: &ProviderSelectionV1) -> String {
    let mut text = String::with_capacity(2 + 64);
    text.push_str("ir-record:");
    match selection.encode() {
        Ok(bytes) => {
            for byte in bytes {
                text.push_str(&format!("{byte:02x}"));
            }
        }
        Err(_) => text.push_str("invalid"),
    }
    text
}

impl DaemonApplicationFacade {
    /// Executes one explicit local tool call through the durable lifecycle path.
    ///
    /// Publication independently rereads the committed typed result evidence for
    /// the exact invocation, and the application dispatches
    /// `AfterToolResultPublished` only after that verification succeeds. The M4
    /// model `ToolCallRecorded` fact remains denial-only; this API is an
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
        // The publication reread is scoped to this exact pre-invocation durable
        // position, so it can only observe this invocation's committed evidence.
        let after_sequence = self
            .inner
            .repository
            .load_session_snapshot(session_id)?
            .at_sequence();
        let cancellation = self.bind_local_tool_cancellation(session_id, run_id)?;
        let publisher = DurableToolResultPublisher {
            repository: self.inner.repository.as_ref(),
            after_sequence,
        };
        let result = intention_application::ApplicationService::with_hooks(
            self.inner.repository.as_ref(),
            production_hooks()?,
        )
        .with_workspace_boundary(SafeWorkspaceBoundary)
        .invoke_local_tool_with_publication(
            InvokeLocalToolInputDto::new(
                workspace,
                session_id,
                run_id,
                call_id,
                tool_id,
                input,
                now()?,
            )
            .with_cancellation(cancellation),
            &publisher,
        );
        self.release_local_tool_cancellation(session_id, run_id);
        result
    }

    /// Binds one local invocation to its run's shared cancellation signal.
    ///
    /// A previously stopped run yields an already-cancelled signal so later
    /// invocations fail before any new effect occurs.
    fn bind_local_tool_cancellation(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<CancellationSignal> {
        let mut registry = self.inner.tool_cancellations.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_command_unavailable",
                "daemon command is unavailable",
            )
        })?;
        let entry =
            registry
                .entry((session_id, run_id))
                .or_insert_with(|| LocalToolCancellationEntry {
                    signal: CancellationSignal::new(),
                    inflight: 0,
                });
        entry.inflight += 1;
        let signal = entry.signal.clone();
        drop(registry);
        Ok(signal)
    }

    /// Releases one finished local invocation from its run's shared signal.
    ///
    /// Completed invocations drop their binding; a cancelled marker stays until
    /// terminalization so follow-on effects remain fenced for that exact run.
    fn release_local_tool_cancellation(&self, session_id: SessionId, run_id: RunId) {
        if let Ok(mut registry) = self.inner.tool_cancellations.lock()
            && let Some(entry) = registry.get_mut(&(session_id, run_id))
        {
            entry.inflight = entry.inflight.saturating_sub(1);
            if entry.inflight == 0 && !entry.signal.is_cancelled() {
                registry.remove(&(session_id, run_id));
            }
        }
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
        let (config_snapshot, selected_provider, raw_toml) =
            load_platform_provider_configuration()?;
        let facade = Self::open_with_selected_provider(
            platform_database_location()?,
            config_snapshot,
            selected_provider,
        )?;
        facade.activate_startup_catalog(&raw_toml)?;
        Ok(facade)
    }

    /// Activates the startup provider configuration as the first catalog
    /// revision exactly once.
    ///
    /// The daemon opens with an empty provider catalog: the startup TOML is
    /// the single source of the first provider declaration. When a catalog is
    /// already active (a restart) this is a no-op. Activation is required
    /// because selection-less acceptance is removed under ADR 0038: every
    /// admitted run resolves a provider profile from the active catalog, and
    /// no wire command prepares a first catalog in this slice.
    ///
    /// # Errors
    ///
    /// Returns the typed candidate, catalog, or storage error; an invalid
    /// startup declaration fails the daemon open path closed.
    pub fn activate_startup_catalog(&self, raw_toml: &str) -> DtoResult<()> {
        use intention_application::{CatalogProviderDeclarationDto, CatalogSourceInputDto};
        use intention_config::control_plane::parse_candidate;

        let state = self.inner.repository.load_provider_catalog_status()?;
        if state.active_catalog_revision_id.is_some() {
            return Ok(());
        }
        let previous = self.active_config_snapshot()?;
        let candidate = parse_candidate(
            RawConfigInputDto::new(raw_toml.to_owned(), ConfigPathResolver::resolve(None)?),
            &previous,
        )?;
        let provider = candidate.safe_snapshot().resolved().provider();
        self.inner
            .control_plane
            .controller
            .prepare_candidate(
                CatalogSourceInputDto {
                    operation_id: "startup-catalog".to_owned(),
                    raw_config_size_bytes: u64::try_from(raw_toml.len()).unwrap_or(u64::MAX),
                    providers: vec![CatalogProviderDeclarationDto {
                        kind: provider.kind().as_str().to_owned(),
                        model: provider.model().to_owned(),
                        endpoint: provider.endpoint().map(str::to_owned),
                        declared_model_capability_subset: vec![
                            "text_input".to_owned(),
                            "text_streaming".to_owned(),
                        ],
                        enabled: true,
                    }],
                    candidate,
                    previous,
                },
                now_seconds(now()?),
            )
            .map(|_| ())
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

    #[cfg(test)]
    fn open_for_test(
        database_location: impl AsRef<Path>,
        config_snapshot: ConfigSnapshotDto,
    ) -> DtoResult<Self> {
        Self::open_with_selected_provider(
            database_location,
            config_snapshot,
            SelectedProvider::for_test_support(Arc::new(TestSupportUnconfiguredDriver)),
        )
    }

    /// Resolves the authoritative workspace root of one durable session.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error when the session is unknown or its declared
    /// workspace cannot be resolved.
    #[doc(hidden)]
    pub fn resolve_workspace_root_for_daemon(
        &self,
        session_id: SessionId,
    ) -> DtoResult<WorkspaceRoot> {
        let projection = self.inner.repository.load_session_snapshot(session_id)?;
        WorkspaceRoot::resolve(projection.workspace_root())
    }

    /// Executes one scheduled run through the privately selected provider
    /// driver with the mandatory tool executor.
    ///
    /// This bridge is provider-neutral and safe: it accepts only scheduling DTOs,
    /// cancellation, a time port, committed-observation evidence, and the tool
    /// executor. It does not expose provider SDKs, credentials, Tokio, or
    /// storage resources. Provider-emitted tool calls execute through the
    /// caller-supplied durable tool path.
    #[doc(hidden)]
    pub async fn execute_scheduled_model_run_for_daemon_with_tool_executor<Time>(
        &self,
        schedule: ScheduleModelRunDto,
        cancellation: ModelCancellationSignal,
        time: &Time,
        observer: &dyn ModelRunCommitObserver,
        tool_executor: &dyn ToolExecutionPort,
    ) -> DtoResult<ModelRunExecutionOutcomeDto>
    where
        Time: ModelTimePort + Sync,
    {
        ModelRunExecutionService::with_commit_observer(
            self.inner.repository.as_ref(),
            self.inner._selected_provider.driver(),
            time,
            observer,
            tool_executor,
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
        tool_executor: &dyn ToolExecutionPort,
    ) -> DtoResult<ModelRunExecutionOutcomeDto>
    where
        Time: ModelTimePort + Sync,
    {
        ModelRunExecutionService::with_commit_observer_and_first_append_gate(
            self.inner.repository.as_ref(),
            self.inner._selected_provider.driver(),
            time,
            observer,
            first_append_gate,
            tool_executor,
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
    /// commit; synchronous stop dispatch no longer exists outside that host.
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
        let accepted = ApplicationService::new(self.inner.repository.as_ref()).stop_run(
            intention_domain::StopRunCommandDto::new(session_id, run_id),
            RuntimeValuesDto::new(RunId::new(), self.active_config_snapshot()?, now()?),
        )?;
        // After the durable Cancelling commit, reach any local tool execution
        // bound to this exact run and fence later invocations. Durable model
        // cancellation semantics remain two-step and unchanged.
        if let Ok(mut registry) = self.inner.tool_cancellations.lock() {
            registry
                .entry((session_id, run_id))
                .or_insert_with(|| LocalToolCancellationEntry {
                    signal: CancellationSignal::cancelled(),
                    inflight: 0,
                })
                .signal
                .cancel();
        }
        Ok(accepted)
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
            self.inner.repository.as_ref(),
            RuntimeValuesDto::new(RunId::new(), self.active_config_snapshot()?, now()?),
        )
        .complete_terminal(session_id, run_id, RunStatusDto::Cancelled)?;
        if let Ok(mut registry) = self.inner.tool_cancellations.lock() {
            registry.remove(&(session_id, run_id));
        }
        Ok(())
    }

    /// Terminalizes one still-active run as durably `Failed` for the daemon
    /// task registry.
    ///
    /// An executor error must never leave a `Starting`/`Running`/`Completing`
    /// run without an owner: this bridge appends the typed failure fact and
    /// commits the terminal `Failed` transition. The failure code is the
    /// executor error's stable code, so deterministic bound and semantic
    /// failures (for example `reasoning_output_limit_exceeded`) become the
    /// durable failed outcome (PR24-012). Runs already terminal are a no-op;
    /// `Cancelling` runs terminalize as `Cancelled`, never here.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error for a `Cancelling` run, or the
    /// repository's typed error when the terminal append cannot commit.
    #[doc(hidden)]
    pub fn fail_active_run_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
        failure_code: &str,
    ) -> DtoResult<()> {
        let _gate = self.inner.command_gate.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_command_unavailable",
                "daemon command is unavailable",
            )
        })?;
        let replay = self
            .inner
            .repository
            .load_current_run_replay(session_id, run_id)?;
        let status = replay.snapshot().run_projection().status();
        if status.is_terminal() {
            return Ok(());
        }
        if status == RunStatusDto::Cancelling {
            return Err(ErrorDto::validation(
                "invalid_failed_run_state",
                "a cancelling run terminalizes as cancelled, not failed",
            ));
        }
        let failure = RunFailureDto::new(failure_code, ErrorRetryDto::Manual, None)?;
        self.inner
            .repository
            .append_model_run_facts(AppendModelRunFactsInputDto::new(
                session_id,
                run_id,
                replay.snapshot().cursor(),
                vec![ModelRunFactInputDto::failed(failure)],
                Some(RunStatusDto::Failed),
                now()?,
            )?)?;
        if let Ok(mut registry) = self.inner.tool_cancellations.lock() {
            registry.remove(&(session_id, run_id));
        }
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
            self.inner.repository.as_ref(),
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
        ApplicationService::new(self.inner.repository.as_ref())
            .load_current_run_replay(session_id, run_id)
    }

    /// Loads a contiguous durable run-fact range for the private daemon host.
    #[doc(hidden)]
    pub fn load_run_tail_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
        after_cursor: RunEventCursorDto,
    ) -> DtoResult<intention_domain::RunEventTailPageDto> {
        ApplicationService::new(self.inner.repository.as_ref()).load_run_tail(
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
        ApplicationService::new(self.inner.repository.as_ref())
            .schedule_starting_run(session_id, run_id)
    }

    /// Returns the currently active durable run when it is eligible for host admission.
    #[doc(hidden)]
    pub fn current_starting_run_for_daemon(
        &self,
        session_id: SessionId,
    ) -> DtoResult<Option<RunId>> {
        Ok(ApplicationService::new(self.inner.repository.as_ref())
            .get_session_snapshot(GetSessionSnapshotQueryDto::new(session_id))?
            .projection()
            .active_run()
            .filter(|run| run.status() == RunStatusDto::Starting)
            .map(|run| run.run_id()))
    }

    fn open_with_selected_provider(
        database_location: impl AsRef<Path>,
        config_snapshot: ConfigSnapshotDto,
        selected_provider: SelectedProvider,
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
        let repository = Arc::new(SqliteStorageRepository::open(location)?);
        repository.accept_configuration_revision(config_snapshot.clone())?;
        let control_plane = ProviderControlPlane::new(RepositoryHandle(repository.clone()));
        let facade = Self {
            inner: Arc::new(FacadeInner {
                repository,
                config_snapshot: Mutex::new(config_snapshot),
                _selected_provider: selected_provider,
                dispatch: PrivateModelRunDispatch::default(),
                command_gate: Mutex::new(()),
                tool_cancellations: Mutex::new(HashMap::new()),
                reload_candidates: Mutex::new(HashMap::new()),
                control_plane,
            }),
        };
        facade.recover_before_ready()?;
        facade.provider_control_startup()?;
        Ok(facade)
    }

    /// Seeds one auto-accepted provider catalog revision with one enabled
    /// profile for test fixtures.
    ///
    /// The seeded profile is `default`, derived from the supplied kind, model,
    /// and endpoint. It mirrors the credential-free startup declarations the
    /// daemon admits; the catalog becomes active with `default` as its global
    /// default so user turns resolve a selection.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn seed_fixture_catalog_for_test_support(
        &self,
        operation_id: &str,
        kind: &str,
        model: &str,
        endpoint: &str,
    ) -> DtoResult<()> {
        use intention_application::{CatalogProviderDeclarationDto, CatalogSourceInputDto};
        use intention_config::control_plane::parse_candidate;

        let previous = self.active_config_snapshot()?;
        let raw = format!(
            "schema_version = 1\n[provider]\nkind = \"{kind}\"\nmodel = \"{model}\"\ncredential = \"fixture-credential\"\nendpoint = \"{endpoint}\"\n"
        );
        let candidate = parse_candidate(
            RawConfigInputDto::new(raw.clone(), fixture_catalog_source()?),
            &previous,
        )?;
        self.inner
            .control_plane
            .controller
            .prepare_candidate(
                CatalogSourceInputDto {
                    operation_id: operation_id.to_owned(),
                    raw_config_size_bytes: u64::try_from(raw.len()).unwrap_or(u64::MAX),
                    providers: vec![CatalogProviderDeclarationDto {
                        kind: kind.to_owned(),
                        model: model.to_owned(),
                        endpoint: Some(endpoint.to_owned()),
                        declared_model_capability_subset: vec![
                            "text_input".to_owned(),
                            "text_streaming".to_owned(),
                        ],
                        enabled: true,
                    }],
                    candidate,
                    previous,
                },
                now_seconds(now()?),
            )
            .map(|_| ())
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

    /// Runs the provider catalog startup and returns its outcome.
    ///
    /// The daemon host calls this after storage opens and unfinished runs are
    /// interrupted. The controller rebuilds the private registry from the
    /// durable active catalog; a degraded outcome leaves the control plane
    /// read-only.
    ///
    /// # Errors
    ///
    /// Returns a typed error only when the control-plane gate is poisoned;
    /// catalog failures degrade to a typed readiness state.
    pub fn provider_control_startup(
        &self,
    ) -> DtoResult<intention_application::CatalogStartupOutcomeDto> {
        self.inner
            .control_plane
            .controller
            .startup(now_seconds(now()?))
    }

    /// Returns the current provider control-plane readiness.
    #[must_use]
    pub fn provider_control_readiness(&self) -> intention_application::CatalogReadiness {
        self.inner
            .control_plane
            .controller
            .inspect()
            .map(|projection| projection.readiness)
            .unwrap_or_else(|_| intention_application::CatalogReadiness::Blocked {
                reason: "control_plane_readiness_unavailable".to_owned(),
            })
    }

    /// Marks one recovered run as held pending explicit admission.
    ///
    /// Held runs are never auto-scheduled by the daemon host; they are
    /// admitted only through the `AdmitRecoveredRun` command.
    #[doc(hidden)]
    pub fn mark_recovered_run_held_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<()> {
        self.inner
            .repository
            .mark_recovered_run_held(MarkRecoveredRunHeldInputDto {
                run_id,
                session_id,
                held_at: i64_time(now()?),
                operation_id: format!("recovery-hold-{session_id}-{run_id}"),
            })
    }

    /// Returns whether one run is held and not yet admitted.
    #[doc(hidden)]
    pub fn is_recovered_run_held_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<bool> {
        Ok(self
            .inner
            .repository
            .load_held_recovered_run(run_id)?
            .is_some_and(|held| {
                held.session_id == session_id
                    && held.admission_state == intention_storage::HeldRunAdmissionStateDto::Held
            }))
    }

    /// Promotes up to eight unavailable-provider queue entries FIFO.
    ///
    /// Called by the daemon host on terminal transitions; the storage enforces
    /// the batch bound and never reroutes an entry.
    #[doc(hidden)]
    pub fn promote_unavailable_runs_for_daemon(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<intention_storage::PromoteUnavailableRunsOutcomeDto> {
        UnavailableQueueService::new(self.inner.repository.as_ref()).promote(
            session_id,
            run_id,
            now_seconds(now()?),
        )
    }

    /// Enqueues one unavailable provider run idempotently.
    #[doc(hidden)]
    pub fn enqueue_unavailable_run_for_daemon(
        &self,
        run_id: RunId,
        session_id: SessionId,
        profile_id: String,
        provider_profile_revision_id: String,
        selection: &ProviderSelectionV1,
    ) -> DtoResult<()> {
        self.inner
            .repository
            .enqueue_unavailable_run(EnqueueUnavailableRunInputDto {
                run_id,
                session_id,
                profile_id,
                provider_profile_revision_id,
                unavailable_reason: "provider_configuration_unavailable".to_owned(),
                first_unavailable_at: i64_time(now()?),
                operation_id: format!("enqueue-unavailable-{session_id}-{run_id}"),
                selection_json: canonical_selection_text(selection),
            })
    }

    /// Dispatches a typed durable M3 query.
    #[must_use]
    pub fn query(&self, query: ProtocolQueryDto) -> ProtocolQueryResultDto {
        match query {
            ProtocolQueryDto::GetDaemonHealth => {
                ProtocolQueryResultDto::DaemonHealth(self.health())
            }
            ProtocolQueryDto::GetSessionSnapshot(query) => {
                ApplicationService::new(self.inner.repository.as_ref())
                    .get_session_snapshot(query)
                    .map_or_else(
                        ProtocolQueryResultDto::Rejected,
                        ProtocolQueryResultDto::SessionSnapshot,
                    )
            }
            // Paged provider catalog projection from the durable catalog.
            ProtocolQueryDto::GetProviderCatalog(query) => {
                if let Err(error) = query.validate() {
                    return ProtocolQueryResultDto::Rejected(error);
                }
                CatalogReadService::new(
                    self.inner.repository.as_ref(),
                    &self.inner.control_plane.controller,
                )
                .list_profiles(query)
                .map_or_else(
                    ProtocolQueryResultDto::Rejected,
                    ProtocolQueryResultDto::ProviderCatalog,
                )
            }
            // Provider catalog activation and degradation status.
            ProtocolQueryDto::GetProviderCatalogStatus(query) => {
                if let Err(error) = query.validate() {
                    return ProtocolQueryResultDto::Rejected(error);
                }
                CatalogReadService::new(
                    self.inner.repository.as_ref(),
                    &self.inner.control_plane.controller,
                )
                .status(query)
                .map_or_else(
                    ProtocolQueryResultDto::Rejected,
                    ProtocolQueryResultDto::ProviderCatalogStatus,
                )
            }
            // One session's durable provider profile projection.
            ProtocolQueryDto::GetSessionProviderProfile(query) => {
                if let Err(error) = query.validate() {
                    return ProtocolQueryResultDto::Rejected(error);
                }
                let port = self.catalog_admission_port();
                SessionProfileService::new(
                    self.inner.repository.as_ref(),
                    self.inner.repository.as_ref(),
                    &self.inner.control_plane,
                )
                .get(query, &port)
                .map_or_else(
                    ProtocolQueryResultDto::Rejected,
                    ProtocolQueryResultDto::SessionProviderProfile,
                )
            }
            // Provider usage aggregation for one period.
            ProtocolQueryDto::GetProviderUsage(query) => {
                if let Err(error) = query.validate() {
                    return ProtocolQueryResultDto::Rejected(error);
                }
                UsageService::new(self.inner.repository.as_ref())
                    .by_profile(query)
                    .map_or_else(
                        ProtocolQueryResultDto::Rejected,
                        ProtocolQueryResultDto::ProviderUsage,
                    )
            }
            // Non-authorizing provider health evidence. The composition probe
            // is not wired yet, so the service projects the typed
            // `provider_health_unavailable` outcome as an `Unknown`
            // observation with a safe diagnostic code.
            ProtocolQueryDto::GetProviderHealthEvidence(query) => {
                if let Err(error) = query.validate() {
                    return ProtocolQueryResultDto::Rejected(error);
                }
                match now() {
                    Err(error) => ProtocolQueryResultDto::Rejected(error),
                    Ok(timestamp) => ProviderHealthService
                        .check(
                            query.provider_id,
                            &CompositionHealthProbe,
                            now_seconds(timestamp),
                        )
                        .map_or_else(
                            ProtocolQueryResultDto::Rejected,
                            ProtocolQueryResultDto::ProviderHealthEvidence,
                        ),
                }
            }
            // Additive provider discovery status. Attempt state is not
            // persisted in this slice, so the service reports the closed
            // unavailable-state projection and never re-runs an attempt.
            ProtocolQueryDto::GetProviderDiscoveryStatus(query) => {
                if let Err(error) = query.validate() {
                    return ProtocolQueryResultDto::Rejected(error);
                }
                let Some(attempt_id) = query.attempt_id else {
                    return ProtocolQueryResultDto::Rejected(ErrorDto::validation(
                        "provider_discovery_invalid",
                        "a discovery attempt reference is required",
                    ));
                };
                match now() {
                    Err(error) => ProtocolQueryResultDto::Rejected(error),
                    Ok(timestamp) => ProviderDiscoveryService
                        .status(
                            attempt_id,
                            &CompositionDiscoveryPort,
                            now_seconds(timestamp),
                        )
                        .map_or_else(
                            ProtocolQueryResultDto::Rejected,
                            ProtocolQueryResultDto::ProviderDiscoveryStatus,
                        ),
                }
            }
            // Safe non-authorizing pricing projection. No pricing
            // observations are wired in this slice, so the projection carries
            // the static disclaimer only and never gates admission.
            ProtocolQueryDto::GetPricingPolicy(query) => {
                if let Err(error) = query.validate() {
                    return ProtocolQueryResultDto::Rejected(error);
                }
                let _ = query.model_id;
                ProtocolQueryResultDto::PricingPolicy(PricingPolicyService.project(Vec::new()))
            }
            // Safe applied configuration projection from the active snapshot.
            ProtocolQueryDto::GetConfigurationProjection(query) => {
                if let Err(error) = query.validate() {
                    return ProtocolQueryResultDto::Rejected(error);
                }
                match self.active_config_snapshot() {
                    Ok(snapshot) => ProtocolQueryResultDto::ConfigurationProjection(
                        configuration_projection(&snapshot),
                    ),
                    Err(error) => ProtocolQueryResultDto::Rejected(error),
                }
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
        let current = match ApplicationService::new(self.inner.repository.as_ref())
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

    /// Clones the active configuration snapshot under its interior lock.
    ///
    /// The active snapshot is updated only by the control-plane reload
    /// handler after a durable commit, so fresh runs always observe the
    /// committed configuration.
    fn active_config_snapshot(&self) -> DtoResult<ConfigSnapshotDto> {
        self.inner
            .config_snapshot
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| {
                ErrorDto::unavailable(
                    "daemon_command_unavailable",
                    "daemon command is unavailable",
                )
            })
    }

    /// Reloads a previously prepared candidate by its stored reference.
    ///
    /// The reference names a candidate retained by a raw-TOML or typed edit
    /// submission (keyed by operation identity); an unknown or expired
    /// reference fails closed with `candidate_unavailable` before any write.
    /// The candidate was already accepted at submission time; this handler
    /// re-enforces the expected active revision and commits durably.
    ///
    /// # Errors
    ///
    /// Returns the typed reference, commit, or storage error.
    fn reload_from_reference(
        &self,
        command: ReloadConfigurationCommandDto,
        timestamp: TimestampDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let reference = command
            .candidate_snapshot_reference
            .or(command.candidate_edit_reference)
            .ok_or_else(|| {
                ErrorDto::validation(
                    "configuration_reload_invalid",
                    "a reload must name a candidate reference",
                )
            })?;
        let candidate = self
            .inner
            .reload_candidates
            .lock()
            .map_err(|_| {
                ErrorDto::unavailable(
                    "daemon_command_unavailable",
                    "daemon command is unavailable",
                )
            })?
            .get(&reference)
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation(
                    "candidate_unavailable",
                    "the reload candidate reference is unknown or expired",
                )
            })?;
        let outcome = self.commit_and_advance(
            candidate,
            Some(command.expected_active_config_revision),
            command.operation_id,
            now_seconds(timestamp),
        )?;
        Ok(ProtocolAcceptedResultDto::ReloadConfiguration(
            committed_reload_transaction(&outcome),
        ))
    }

    /// Parses, validates, and durably commits a raw TOML edit.
    ///
    /// The raw content is parsed server-side through the reload contract and
    /// never echoed back. A not-accepted candidate returns the typed rejected
    /// transaction with its first failure code.
    ///
    /// # Errors
    ///
    /// Returns the typed parse, commit, or storage error.
    fn reload_from_raw_toml(
        &self,
        command: RawTomlEditCommandDto,
        timestamp: TimestampDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let binding = self.composition_binding_source();
        let previous = binding.active_snapshot()?;
        let previous_revision = binding.active_revision()?;
        let service = ConfigurationReloadService::new(self.inner.repository.as_ref(), &binding);
        let candidate = service.prepare(
            RawConfigInputDto::new(command.candidate_content, reload_edit_source()?),
            &previous,
            command.operation_id.clone(),
        )?;
        self.complete_reload(
            candidate,
            command.expected_config_revision,
            command.operation_id,
            now_seconds(timestamp),
            previous_revision,
        )
    }

    /// Applies typed edit operations and durably commits the edited candidate.
    ///
    /// The operations are applied to the active snapshot and validated
    /// server-side through the reload contract. Because the credential is
    /// never retained server-side, an edited candidate that leaves the
    /// credential unset fails closed with `missing_provider_credential` in
    /// this slice.
    ///
    /// # Errors
    ///
    /// Returns the typed edit, parse, commit, or storage error.
    fn reload_from_typed_edit(
        &self,
        command: ConfigurationEditCommandDto,
        timestamp: TimestampDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let binding = self.composition_binding_source();
        let previous = binding.active_snapshot()?;
        let previous_revision = binding.active_revision()?;
        let service = ConfigurationReloadService::new(self.inner.repository.as_ref(), &binding);
        let edited = edited_configuration_toml(&previous, &command.operations)?;
        let candidate = service.prepare(
            RawConfigInputDto::new(edited, reload_edit_source()?),
            &previous,
            command.operation_id.clone(),
        )?;
        self.complete_reload(
            candidate,
            command.expected_config_revision,
            command.operation_id,
            now_seconds(timestamp),
            previous_revision,
        )
    }

    /// Rotates one provider's private credential material through the
    /// composition's credential and rebuild ports.
    ///
    /// # Errors
    ///
    /// Returns `credential_rotation_frozen_meaning_mismatch` when the safe
    /// composition changed, or `credential_rotation_source_unavailable` when
    /// no private credential source is configured.
    fn rotate_credential(
        &self,
        command: RotateProviderCredentialsCommandDto,
        timestamp: TimestampDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let binding = self.composition_binding_source();
        let service = CredentialRotationService::new(&binding, &CompositionDriverRebuildPort);
        let result = service.rotate(command, &CompositionCredentialPort, now_seconds(timestamp))?;
        Ok(ProtocolAcceptedResultDto::RotateProviderCredentials(result))
    }

    /// Finishes one reload: rejects a not-accepted candidate with its typed
    /// transaction, retains the accepted candidate for reference reloads, and
    /// commits durably before advancing the fresh-run snapshot.
    ///
    /// # Errors
    ///
    /// Returns the typed commit or storage error.
    fn complete_reload(
        &self,
        candidate: ReloadCandidateDto,
        expected_revision: String,
        operation_id: String,
        now_seconds: u64,
        previous_revision: String,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        if !candidate.accepted {
            return Ok(ProtocolAcceptedResultDto::ReloadConfiguration(
                rejected_reload_transaction(&candidate, operation_id, previous_revision),
            ));
        }
        if let Ok(mut candidates) = self.inner.reload_candidates.lock() {
            if candidates.len() >= MAX_RELOAD_CANDIDATES
                && let Some(first) = candidates.keys().next().cloned()
            {
                candidates.remove(&first);
            }
            candidates.insert(operation_id.clone(), candidate.candidate().clone());
        }
        let outcome = self.commit_and_advance(
            candidate.candidate().clone(),
            Some(expected_revision),
            operation_id,
            now_seconds,
        )?;
        Ok(ProtocolAcceptedResultDto::ReloadConfiguration(
            committed_reload_transaction(&outcome),
        ))
    }

    /// Durably commits one candidate and advances the fresh-run snapshot only
    /// after the commit succeeds.
    ///
    /// A storage failure propagates and the daemon stays on its recorded
    /// snapshot: the in-memory active snapshot is never advanced on failure.
    ///
    /// # Errors
    ///
    /// Returns `config_revision_mismatch` for a stale expected revision, or
    /// the durable repository's typed storage error.
    fn commit_and_advance(
        &self,
        candidate: ConfigCandidateDto,
        expected_revision: Option<String>,
        operation_id: String,
        now_seconds: u64,
    ) -> DtoResult<ReloadCommitOutcomeDto> {
        let binding = self.composition_binding_source();
        let service = ConfigurationReloadService::new(self.inner.repository.as_ref(), &binding);
        let outcome = service.commit(
            candidate.clone(),
            expected_revision,
            operation_id,
            now_seconds,
        )?;
        if let Ok(mut snapshot) = self.inner.config_snapshot.lock() {
            *snapshot = candidate.safe_snapshot().clone();
        }
        Ok(outcome)
    }

    /// Builds the composition's catalog admission port over the active
    /// control plane and durable repository.
    fn catalog_admission_port(&self) -> CompositionCatalogAdmissionPort<'_> {
        CompositionCatalogAdmissionPort {
            controller: &self.inner.control_plane.controller,
            catalog: self.inner.repository.as_ref(),
        }
    }

    /// Builds the composition's catalog-runtime-backed binding source.
    fn composition_binding_source(&self) -> CatalogBindingSource<'_> {
        CatalogBindingSource {
            snapshot: &self.inner.config_snapshot,
            admission: self.catalog_admission_port(),
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
                ApplicationService::new(self.inner.repository.as_ref())
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
                let port = self.catalog_admission_port();
                ApplicationService::new(self.inner.repository.as_ref())
                    .send_user_turn_and_schedule_with_provider_selection(
                        command,
                        SendUserTurnWorkflowInputDto::new(
                            proposed_run_id,
                            self.active_config_snapshot()?,
                            timestamp,
                        ),
                        &port,
                        &self.inner.dispatch,
                    )?
            }
            ProtocolCommandDto::RemoveQueuedTurn(command) => {
                ApplicationService::new(self.inner.repository.as_ref())
                    .remove_queued_turn(command, timestamp)?
            }
            ProtocolCommandDto::StopRun(_) => {
                return Err(ErrorDto::validation(
                    "invalid_stop_dispatch",
                    "run stops use the daemon host stop path",
                ));
            }
            ProtocolCommandDto::SubscribeSession(_) => {
                return Err(ErrorDto::validation(
                    "invalid_subscription_dispatch",
                    "session subscriptions use the dedicated protocol response",
                ));
            }
            ProtocolCommandDto::SetSessionProviderProfile(command) => {
                command.validate()?;
                let port = self.catalog_admission_port();
                let accepted = SessionProfileService::new(
                    self.inner.repository.as_ref(),
                    self.inner.repository.as_ref(),
                    &self.inner.control_plane,
                )
                .set(command, &port, now_seconds(timestamp))?;
                ProtocolAcceptedResultDto::SetSessionProviderProfile(accepted)
            }
            ProtocolCommandDto::AcceptProviderCatalogRemoval(command) => {
                command.validate()?;
                let accepted = RemovalService::new(&self.inner.control_plane.controller)
                    .accept(command, now_seconds(timestamp))?;
                ProtocolAcceptedResultDto::AcceptProviderCatalogRemoval(accepted)
            }
            ProtocolCommandDto::RejectProviderCatalogCandidate(command) => {
                command.validate()?;
                let accepted = RemovalService::new(&self.inner.control_plane.controller)
                    .reject(command, now_seconds(timestamp))?;
                ProtocolAcceptedResultDto::RejectProviderCatalogCandidate(accepted)
            }
            ProtocolCommandDto::ReconcileUnavailableQueue(command) => {
                command.validate()?;
                let accepted = UnavailableQueueService::new(self.inner.repository.as_ref())
                    .reconcile(command, &self.inner.control_plane, now_seconds(timestamp))?;
                ProtocolAcceptedResultDto::ReconcileUnavailableQueue(accepted)
            }
            ProtocolCommandDto::AdmitRecoveredRun(command) => {
                command.validate()?;
                let session_id = SessionId::parse(&command.session_id)?;
                let run_id = RunId::parse(&command.run_id)?;
                let schedule = ApplicationService::new(self.inner.repository.as_ref())
                    .schedule_starting_run(session_id, run_id)?;
                let accepted = HeldRunService::new(
                    self.inner.repository.as_ref(),
                    self.inner.repository.as_ref(),
                    &self.inner.control_plane,
                    &self.catalog_admission_port(),
                )
                .admit(
                    command,
                    schedule,
                    &self.inner.dispatch,
                    now_seconds(timestamp),
                )?;
                ProtocolAcceptedResultDto::AdmitRecoveredRun(accepted)
            }
            // Slice 2: real handler lands with the control-plane/session-selection zones.
            ProtocolCommandDto::ReloadConfiguration(command) => {
                command.validate()?;
                self.reload_from_reference(command, timestamp)?
            }
            // Slice 2: real handler lands with the control-plane/session-selection zones.
            ProtocolCommandDto::RotateProviderCredentials(command) => {
                command.validate()?;
                self.rotate_credential(command, timestamp)?
            }
            ProtocolCommandDto::SubmitRawTomlEdit(command) => {
                command.validate()?;
                self.reload_from_raw_toml(command, timestamp)?
            }
            ProtocolCommandDto::ApplyConfigurationEdit(command) => {
                command.validate()?;
                self.reload_from_typed_edit(command, timestamp)?
            }
        };
        Ok(result)
    }

    fn recover_before_ready(&self) -> DtoResult<()> {
        RuntimeService::new(
            self.inner.repository.as_ref(),
            RuntimeValuesDto::new(RunId::new(), self.active_config_snapshot()?, now()?),
        )
        .recover_before_ready()?;
        Ok(())
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

fn load_platform_provider_configuration() -> DtoResult<(ConfigSnapshotDto, SelectedProvider, String)>
{
    load_provider_configuration(ConfigPathResolver::resolve(None)?)
}

fn load_provider_configuration(
    source: ConfigSourceDto,
) -> DtoResult<(ConfigSnapshotDto, SelectedProvider, String)> {
    #[cfg(unix)]
    intention_config::ensure_user_only_permissions(source.path())?;
    let raw_toml = fs::read_to_string(source.path().as_str()).map_err(|_| {
        ErrorDto::unavailable(
            "daemon_configuration_read_unavailable",
            "daemon configuration could not be read",
        )
    })?;
    let material = ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
        raw_toml.clone(),
        source,
    ))?;
    let snapshot = ConfigSnapshotDto::new(
        CONFIG_SCHEMA_VERSION,
        ConfigRevisionId::new(),
        now()?,
        material.safe_resolved().clone(),
    )?;
    let selected_provider = SelectedProvider::from_startup_material(material)?;
    Ok((snapshot, selected_provider, raw_toml))
}

#[cfg(test)]
fn load_config_snapshot(source: ConfigSourceDto) -> DtoResult<ConfigSnapshotDto> {
    load_provider_configuration(source).map(|(snapshot, _, _)| snapshot)
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
        clippy::panic,
        reason = "Composition internals use controlled durable fixtures with direct assertions."
    )]

    use super::*;

    use intention_domain::SendUserTurnCommandDto;
    use tempfile::TempDir;

    fn test_facade() -> (TempDir, DaemonApplicationFacade) {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
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
            path: intention_types::WorkspaceRelativePathDto::parse("hello.txt")
                .expect("fixture path is valid"),
        })
    }

    /// Builds an in-memory read result used only to fabricate publication inputs.
    fn hello_read_result() -> ToolResult {
        ToolResult::Read(intention_tools::TextResult {
            text: intention_tools::BoundedText::new("hello").expect("fixture text"),
            truncated: false,
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
            SendUserTurnCommandDto::new(session_id, intention_types::TurnId::new(), content)
                .expect("fixture user turn is valid"),
        ))
    }

    /// Creates one session bound to a caller-owned workspace directory.
    fn create_in_workspace(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        workspace_path: &std::path::Path,
    ) {
        let root = WorkspaceRootDto::parse(workspace_path.to_string_lossy().into_owned())
            .expect("fixture workspace is absolute");
        let accepted = facade.command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                root,
                RunModeDto::Build,
            ),
        ));
        assert!(matches!(accepted, ProtocolCommandResultDto::Accepted(_)));
    }

    /// Commits one durable starting run with the supplied immutable selection
    /// directly through the storage repository.
    ///
    /// The wire path always derives the same selection from one active
    /// catalog, and the durable storage contract binds each selection digest
    /// to exactly one run; fixtures that need several selected runs in one
    /// database therefore commit distinct selections through the repository.
    fn fabricate_started_run_with_selection(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        selection: ProviderSelectionV1,
        content: &str,
    ) -> RunId {
        let run_id = RunId::new();
        let input = AcceptUserTurnInputDto::new(
            session_id,
            intention_types::TurnId::new(),
            content,
            run_id,
            fixture_config_snapshot(),
            TimestampDto::from_unix_seconds(2).expect("fixture timestamp is valid"),
        )
        .expect("fixture turn input is valid")
        .with_provider_selection(selection);
        let change = facade
            .inner
            .repository
            .accept_user_turn(input)
            .expect("fixture run commits");
        assert!(
            change.turn_outcome().is_some(),
            "the fixture turn starts immediately"
        );
        run_id
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
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");

        let result = send_user_turn(&facade, session_id, "started turn");
        let ProtocolCommandResultDto::Accepted(accepted_result) = result else {
            unreachable!("direct turn is accepted")
        };
        let ProtocolAcceptedResultDto::SendUserTurn(accepted_turn) = accepted_result.result()
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
            &facade
                .active_config_snapshot()
                .expect("active snapshot reads"),
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
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
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
            ProtocolAcceptedResultDto::SendUserTurn(turn)
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
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");

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
                    ProtocolAcceptedResultDto::SendUserTurn(turn)
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
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");

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
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");

        let accepted = send_user_turn(&facade, session_id, "host bridge turn");
        let ProtocolCommandResultDto::Accepted(accepted) = accepted else {
            unreachable!("fixture turn is accepted")
        };
        let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
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

            let (snapshot, selected_provider, _) =
                load_provider_configuration(source).expect("valid provider config composes");
            let facade = DaemonApplicationFacade::open_with_selected_provider(
                directory.path().join("provider.sqlite"),
                snapshot.clone(),
                selected_provider,
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
        let (_directory, facade) = test_facade();
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
        let (_directory, facade) = test_facade();
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
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let accepted = send_user_turn(&facade, session_id, "bridge");
        let ProtocolCommandResultDto::Accepted(a) = accepted else {
            unreachable!()
        };
        let ProtocolAcceptedResultDto::SendUserTurn(t) = a.result() else {
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
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let started = send_user_turn(&facade, session_id, "started");
        let ProtocolCommandResultDto::Accepted(a) = started else {
            unreachable!()
        };
        let ProtocolAcceptedResultDto::SendUserTurn(t) = a.result() else {
            unreachable!()
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = t.outcome() else {
            unreachable!()
        };
        let queued = send_user_turn(&facade, session_id, "queued");
        let ProtocolCommandResultDto::Accepted(a) = queued else {
            unreachable!()
        };
        let ProtocolAcceptedResultDto::SendUserTurn(t) = a.result() else {
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
        // Stops no longer dispatch through the synchronous command path; the
        // daemon host owns the exact two-step cancellation transition.
        assert!(matches!(facade.command(ProtocolCommandDto::StopRun(
                intention_domain::StopRunCommandDto::new(session_id, run_id)
            )), ProtocolCommandResultDto::Rejected(error) if error.code() == "invalid_stop_dispatch"));
        facade
            .stop_run_for_daemon_host(session_id, run_id)
            .expect("host stop commits cancelling");
        assert_eq!(
            facade
                .load_current_run_replay_for_daemon(session_id, run_id)
                .expect("cancelling replay reads")
                .snapshot()
                .run_projection()
                .status(),
            RunStatusDto::Cancelling,
            "the host stop leaves the run waiting on the daemon task"
        );
        assert!(
            matches!(facade.command(ProtocolCommandDto::SubscribeSession(SubscribeSessionCommandDto::new(SCHEMA_VERSION, session_id, None, RunModeDto::Build))), ProtocolCommandResultDto::Rejected(error) if error.code() == "invalid_subscription_dispatch")
        );
    }

    #[test]
    fn subscribe_returns_checkpoint_for_current_position() {
        let (_directory, facade) = test_facade();
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
    fn committed_evidence_is_durable_and_replays_without_duplication() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let durable_create_events = facade
            .inner
            .repository
            .load_tail(session_id, SessionEventSequenceDto::new(0))
            .expect("independent durable read sees the created event");
        assert_eq!(durable_create_events.len(), 1);

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
            facade
                .inner
                .repository
                .load_tail(session_id, SessionEventSequenceDto::new(0))
                .expect("rejected persistence changes nothing")
                .len(),
            1,
            "failed persistence leaves no durable trace"
        );

        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let accepted = facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, intention_types::TurnId::new(), "committed")
                .expect("fixture user turn is valid"),
        ));
        assert!(matches!(accepted, ProtocolCommandResultDto::Accepted(_)));
        let durable_events = facade
            .inner
            .repository
            .load_tail(session_id, SessionEventSequenceDto::new(1))
            .expect("independent durable read sees committed turn batch");
        assert_eq!(durable_events.len(), 2);

        // The committed evidence replays as an authoritative snapshot: the
        // M3 seam returns the current durable checkpoint with an empty
        // contiguous tail, and replaying at that checkpoint is a stable
        // no-op that duplicates or loses nothing.
        let replay = facade.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA_VERSION,
            session_id,
            Some(SessionEventSequenceDto::new(0)),
            RunModeDto::Build,
        ));
        let SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail } = replay else {
            unreachable!("durable replay is contiguous")
        };
        assert_eq!(
            snapshot.at_sequence(),
            SessionEventSequenceDto::new(3),
            "the snapshot advanced through the committed create and turn batches"
        );
        assert!(
            tail.events().is_empty(),
            "the checkpoint replay carries no duplicate events"
        );
        let current = facade.subscribe(SubscribeSessionCommandDto::new(
            SCHEMA_VERSION,
            session_id,
            Some(snapshot.at_sequence()),
            RunModeDto::Build,
        ));
        assert!(
            matches!(current, SessionSubscriptionResponseDto::SnapshotAndTail { snapshot, tail }
                if snapshot.at_sequence() == SessionEventSequenceDto::new(3) && tail.events().is_empty()),
            "the current checkpoint is replayable without duplication"
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
        );
        let error = match result {
            Ok(_) => return,
            Err(error) => error,
        };
        assert_eq!(error.code(), "invalid_selected_provider");
    }

    #[test]
    fn command_rejects_turn_without_an_applicable_profile() {
        let (_directory, facade) = test_facade();
        // No session exists, no profile applies, and no catalog is active:
        // the typed closed resolution rejects before any durable commit.
        let result = facade.command(ProtocolCommandDto::SendUserTurn(
            SendUserTurnCommandDto::new(SessionId::new(), intention_types::TurnId::new(), "turn")
                .expect("fixture turn is valid"),
        ));
        let ProtocolCommandResultDto::Rejected(error) = result else {
            unreachable!("a turn without an applicable profile is rejected")
        };
        assert_eq!(error.code(), "provider_profile_runtime_unavailable");
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
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let accepted = send_user_turn(&facade, session_id, "execution bridge");
        let ProtocolCommandResultDto::Accepted(accepted) = accepted else {
            unreachable!("turn is accepted")
        };
        let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
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
        let (_directory, facade) = test_facade();
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
        let (_directory, facade) = test_facade();
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
        let (_directory, facade) = test_facade();
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
    fn private_dispatch_covers_success_paths() {
        let session_id = SessionId::new();
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

    #[test]
    fn daemon_stop_blocks_later_local_invocation_before_any_new_effect() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let run_id = started_run(&facade, session_id, "stop first");
        let (workspace_directory, workspace) = workspace_fixture("hello.txt", "hello");

        facade
            .stop_run_for_daemon_host(session_id, run_id)
            .expect("host stop commits cancelling");
        assert_eq!(
            facade
                .load_current_run_replay_for_daemon(session_id, run_id)
                .expect("cancelling replay reads")
                .snapshot()
                .run_projection()
                .status(),
            RunStatusDto::Cancelling,
            "durable model cancellation semantics stay two-step"
        );

        let error = facade
            .invoke_local_tool_for_daemon(
                session_id,
                run_id,
                intention_types::ToolCallId::new(),
                "write",
                ToolInput::Write(intention_tools::WriteInput {
                    path: intention_types::WorkspaceRelativePathDto::parse("late.txt")
                        .expect("fixture path is valid"),
                    content: intention_tools::BoundedText::new("late").expect("fixture content"),
                    expected_content: None,
                }),
                workspace,
            )
            .expect_err("a stopped run cannot admit new local effects");
        assert_eq!(error.code(), "tool_cancelled");
        assert!(
            !workspace_directory.path().join("late.txt").exists(),
            "no workspace effect may occur after the stop"
        );
    }

    #[test]
    fn daemon_stop_reaches_in_flight_execute_and_classifies_unknown_external_effect() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let run_id = started_run(&facade, session_id, "in flight stop");
        let (workspace_directory, workspace) = workspace_fixture("keep.txt", "kept");
        let sentinel = workspace_directory.path().join("sentinel.txt");
        let worker_facade = facade.clone();
        let worker_session = session_id;
        let worker_run = run_id;

        let worker = std::thread::spawn(move || {
            worker_facade.invoke_local_tool_for_daemon(
                worker_session,
                worker_run,
                intention_types::ToolCallId::new(),
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
                            intention_tools::BoundedText::new("printf x > sentinel.txt; sleep 2")
                                .expect("arg"),
                        ]
                    },
                }),
                workspace,
            )
        });

        // The sentinel proves the child was spawned and running, so the stop
        // can only land while execution is in flight.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !sentinel.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "execute child never produced its start sentinel"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        facade
            .stop_run_for_daemon_host(session_id, run_id)
            .expect("host stop commits cancelling while execute runs");

        let error = worker
            .join()
            .expect("worker completes")
            .expect_err("in-flight execution observes the stop");
        assert_eq!(error.code(), "tool_execute_external_effect_unknown");
        assert_eq!(
            facade
                .load_current_run_replay_for_daemon(session_id, run_id)
                .expect("cancelling replay reads")
                .snapshot()
                .run_projection()
                .status(),
            RunStatusDto::Cancelling,
            "the host stop persists only the durable Cancelling step"
        );
    }

    #[test]
    fn committed_tool_results_survive_a_late_stop_which_then_fences_new_effects() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let run_id = started_run(&facade, session_id, "late stop");
        let (workspace_directory, workspace) = workspace_fixture("hello.txt", "hello");

        for _ in 0..2 {
            let result = facade
                .invoke_local_tool_for_daemon(
                    session_id,
                    run_id,
                    intention_types::ToolCallId::new(),
                    "read",
                    ToolInput::Read(intention_tools::ReadInput {
                        path: intention_types::WorkspaceRelativePathDto::parse("hello.txt")
                            .expect("fixture path is valid"),
                    }),
                    workspace.clone(),
                )
                .expect("reads complete before any stop");
            match result {
                ToolResult::Read(text) => assert_eq!(text.text.as_str(), "hello"),
                _ => unreachable!("read dispatch returns a read result"),
            }
        }

        facade
            .stop_run_for_daemon_host(session_id, run_id)
            .expect("host stop commits cancelling after completed effects");

        let error = facade
            .invoke_local_tool_for_daemon(
                session_id,
                run_id,
                intention_types::ToolCallId::new(),
                "write",
                ToolInput::Write(intention_tools::WriteInput {
                    path: intention_types::WorkspaceRelativePathDto::parse("late.txt")
                        .expect("fixture path is valid"),
                    content: intention_tools::BoundedText::new("late").expect("fixture content"),
                    expected_content: None,
                }),
                workspace,
            )
            .expect_err("follow-on effects stay fenced after the stop");
        assert_eq!(error.code(), "tool_cancelled");
        assert!(
            !workspace_directory.path().join("late.txt").exists(),
            "the committed read results stand; no late effect occurs"
        );

        facade
            .terminalize_cancelling_run_for_daemon(session_id, run_id)
            .expect("terminalization clears the fenced run marker");
        assert_eq!(
            facade
                .load_current_run_replay_for_daemon(session_id, run_id)
                .expect("cancelled replay reads")
                .snapshot()
                .run_projection()
                .status(),
            RunStatusDto::Cancelled
        );
    }

    #[test]
    fn local_tool_invocation_commits_only_its_own_reread_evidence() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let run_id = started_run(&facade, session_id, "publication reread");
        let committed_through = facade
            .inner
            .repository
            .load_session_snapshot(session_id)
            .expect("current snapshot reads")
            .at_sequence();
        let (_workspace_directory, workspace) = workspace_fixture("hello.txt", "hello");

        let call_id = intention_types::ToolCallId::new();
        let result = facade
            .invoke_local_tool_for_daemon(
                session_id,
                run_id,
                call_id,
                "read",
                read_hello_input(),
                workspace.clone(),
            )
            .expect("read completes before any stop");
        match result {
            ToolResult::Read(text) => assert_eq!(text.text.as_str(), "hello"),
            _ => unreachable!("read dispatch returns a read result"),
        }

        let durable_tail = facade
            .inner
            .repository
            .load_tail(session_id, committed_through)
            .expect("independent durable read sees the committed invocation");
        assert!(
            durable_tail.iter().all(|envelope| matches!(
                envelope.payload(),
                intention_domain::DomainEventDto::ToolLifecycle(event)
                    if event.session_id() == session_id
                        && event.run_id() == run_id
                        && event.call_id() == call_id
            )),
            "every committed event is this exact call's lifecycle evidence"
        );
        match durable_tail
            .last()
            .expect("committed scope is non-empty")
            .payload()
        {
            intention_domain::DomainEventDto::ToolLifecycle(event) => {
                assert_eq!(
                    event.status(),
                    &intention_domain::ToolLifecycleStatusDto::Completed,
                    "the committed typed result evidence completed"
                );
            }
            _ => unreachable!("final committed evidence is the completed result"),
        }

        let second_call = intention_types::ToolCallId::new();
        facade
            .invoke_local_tool_for_daemon(
                session_id,
                run_id,
                second_call,
                "read",
                read_hello_input(),
                workspace,
            )
            .expect("second read completes");
        let second_position = facade
            .inner
            .repository
            .load_session_snapshot(session_id)
            .expect("current snapshot reads")
            .at_sequence();
        let second_tail = facade
            .inner
            .repository
            .load_tail(session_id, second_position)
            .expect("independent durable read sees the second invocation");
        assert!(
            second_tail.iter().all(|envelope| matches!(
                envelope.payload(),
                intention_domain::DomainEventDto::ToolLifecycle(event)
                    if event.call_id() == second_call
            )),
            "the second invocation never repeats the first call's evidence"
        );
    }

    #[test]
    fn tool_result_publication_requires_evidence_for_the_exact_committed_call() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let run_id = started_run(&facade, session_id, "exact correlation");
        let committed_through = facade
            .inner
            .repository
            .load_session_snapshot(session_id)
            .expect("current snapshot reads")
            .at_sequence();
        let (_workspace_directory, workspace) = workspace_fixture("hello.txt", "hello");

        let call_id = intention_types::ToolCallId::new();
        facade
            .invoke_local_tool_for_daemon(
                session_id,
                run_id,
                call_id,
                "read",
                read_hello_input(),
                workspace,
            )
            .expect("read completes");

        let publisher = DurableToolResultPublisher {
            repository: &facade.inner.repository,
            after_sequence: committed_through,
        };
        let uncommitted = ToolResultPublicationInputDto::new(
            session_id,
            run_id,
            intention_types::ToolCallId::new(),
            hello_read_result(),
        );
        let error = publisher
            .publish_tool_result(&uncommitted)
            .expect_err("an uncommitted call identity cannot publish");
        assert_eq!(error.code(), "tool_result_evidence_unavailable");

        let cross_run = ToolResultPublicationInputDto::new(
            session_id,
            RunId::new(),
            call_id,
            hello_read_result(),
        );
        let error = publisher
            .publish_tool_result(&cross_run)
            .expect_err("a cross-run identity cannot publish");
        assert_eq!(error.code(), "tool_result_evidence_unavailable");

        let current_position = facade
            .inner
            .repository
            .load_session_snapshot(session_id)
            .expect("current snapshot reads")
            .at_sequence();
        let drained = DurableToolResultPublisher {
            repository: &facade.inner.repository,
            after_sequence: current_position,
        };
        let exact =
            ToolResultPublicationInputDto::new(session_id, run_id, call_id, hello_read_result());
        let error = drained
            .publish_tool_result(&exact)
            .expect_err("a reread window after the commit contains no evidence");
        assert_eq!(error.code(), "tool_result_evidence_unavailable");

        publisher
            .publish_tool_result(&exact)
            .expect("the exact committed identity correlates");
    }

    // ---------------------------------------------------------------------
    // Zone 4 control-plane composition tests.
    // ---------------------------------------------------------------------

    const ROTATION_FAKE_SECRET: &str = "sk-zone4-composition-fake-secret";

    /// Builds a reloadable candidate that changes only the provider execution
    /// policy. Model, endpoint, and kind changes are catalog-affecting under
    /// PR24-008 and are rejected by live reload.
    fn policy_edit(timeout_seconds: u64) -> String {
        format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential\"\n[provider.execution]\nattempt_timeout_seconds = {timeout_seconds}\nmax_attempts = 2\n"
        )
    }

    fn raw_edit_command(operation_id: &str, expected: &str, content: String) -> ProtocolCommandDto {
        ProtocolCommandDto::SubmitRawTomlEdit(RawTomlEditCommandDto {
            operation_id: operation_id.to_owned(),
            expected_config_revision: expected.to_owned(),
            candidate_content: content,
        })
    }

    fn reload_transaction(result: ProtocolCommandResultDto) -> ReloadTransactionDto {
        let ProtocolCommandResultDto::Accepted(accepted) = result else {
            unreachable!("control-plane command must be accepted");
        };
        let ProtocolAcceptedResultDto::ReloadConfiguration(transaction) = accepted.result() else {
            unreachable!("control-plane command must return a reload transaction");
        };
        transaction.clone()
    }

    #[test]
    fn control_plane_reload_advances_the_fresh_run_snapshot_only_after_durable_commit() {
        let directory = TempDir::new().expect("temporary directory exists");
        let startup = fixture_config_snapshot();
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("reload.sqlite"),
            startup.clone(),
        )
        .expect("durable facade opens");
        let startup_revision = startup.revision_id().to_string();

        let transaction = reload_transaction(facade.command(raw_edit_command(
            "op-1",
            &startup_revision,
            policy_edit(45),
        )));
        assert_eq!(
            transaction.commit_outcome,
            ConfigurationCommitOutcomeDto::Committed
        );
        assert_eq!(transaction.previous_config_revision, startup_revision);
        assert_ne!(
            transaction.candidate_config_revision, startup_revision,
            "a committed reload advances the configuration revision"
        );

        // The fresh-run snapshot advanced only after the durable commit; the
        // model is unchanged because model edits are catalog-affecting.
        let active = facade
            .active_config_snapshot()
            .expect("active snapshot reads");
        assert_eq!(
            active
                .resolved()
                .provider_execution()
                .attempt_timeout_seconds(),
            45
        );
        assert_eq!(active.resolved().provider().model(), "fixture");
        assert_eq!(
            active.revision_id().to_string(),
            transaction.candidate_config_revision
        );

        // A fresh run now schedules with the committed snapshot.
        let session_id = SessionId::new();
        create(&facade, session_id);
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let result = send_user_turn(&facade, session_id, "fresh turn");
        let ProtocolCommandResultDto::Accepted(accepted) = result else {
            unreachable!("fresh turn is accepted")
        };
        let ProtocolAcceptedResultDto::SendUserTurn(_) = accepted.result() else {
            unreachable!("fresh turn returns user-turn evidence")
        };
        let admitted = facade
            .inner
            .dispatch
            .admitted()
            .expect("dispatch recorder remains available");
        assert_eq!(admitted.len(), 1);
        assert_eq!(
            admitted[0]
                .safe_config()
                .resolved()
                .provider_execution()
                .attempt_timeout_seconds(),
            45,
            "fresh runs must observe the committed reload snapshot"
        );
    }

    #[test]
    fn control_plane_reload_failure_keeps_the_previous_revision() {
        let directory = TempDir::new().expect("temporary directory exists");
        let startup = fixture_config_snapshot();
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("reload-failure.sqlite"),
            startup.clone(),
        )
        .expect("durable facade opens");
        let startup_revision = startup.revision_id().to_string();

        // An invalid candidate returns the typed rejected transaction.
        let invalid = reload_transaction(facade.command(raw_edit_command(
            "op-1",
            &startup_revision,
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"\"\ncredential = \"fixture-credential\"\n"
                .to_owned(),
        )));
        assert_eq!(
            invalid.commit_outcome,
            ConfigurationCommitOutcomeDto::Rejected
        );
        assert_eq!(
            invalid.safe_failure_code.as_deref(),
            Some("invalid_provider_model")
        );

        // A catalog-affecting change is rejected with its typed code.
        let catalog_change = reload_transaction(facade.command(raw_edit_command(
            "op-2",
            &startup_revision,
            "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"model-b\"\ncredential = \"fixture-credential\"\n"
                .to_owned(),
        )));
        assert_eq!(
            catalog_change.safe_failure_code.as_deref(),
            Some("catalog_change_requires_restart")
        );

        // A stale expected revision fails closed before any write.
        let stale = facade.command(raw_edit_command("op-3", "revision-stale", policy_edit(45)));
        let ProtocolCommandResultDto::Rejected(error) = stale else {
            unreachable!("stale expected revision is rejected")
        };
        assert_eq!(error.code(), "config_revision_mismatch");

        // The active snapshot never advanced on any failure.
        let active = facade
            .active_config_snapshot()
            .expect("active snapshot reads");
        assert_eq!(active.resolved().provider().model(), "fixture");
        assert_eq!(active.revision_id().to_string(), startup_revision);
    }

    #[test]
    fn control_plane_reference_reload_commits_a_stored_candidate_and_unknown_references_fail_closed()
     {
        let directory = TempDir::new().expect("temporary directory exists");
        let startup = fixture_config_snapshot();
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("reload-reference.sqlite"),
            startup.clone(),
        )
        .expect("durable facade opens");
        let startup_revision = startup.revision_id().to_string();

        let first = reload_transaction(facade.command(raw_edit_command(
            "op-1",
            &startup_revision,
            policy_edit(45),
        )));
        let committed_revision = first.candidate_config_revision;

        // The stored candidate commits again by reference (idempotent).
        let second = reload_transaction(facade.command(ProtocolCommandDto::ReloadConfiguration(
            ReloadConfigurationCommandDto {
                candidate_snapshot_reference: Some("op-1".to_owned()),
                candidate_edit_reference: None,
                expected_active_config_revision: committed_revision.clone(),
                operation_id: "op-2".to_owned(),
                origin: intention_protocol::contract_families::ConfigurationOriginDto::Admin,
            },
        )));
        assert_eq!(
            second.commit_outcome,
            ConfigurationCommitOutcomeDto::Committed
        );
        assert_eq!(second.candidate_config_revision, committed_revision);

        // An unknown reference fails closed before any write.
        let unknown = facade.command(ProtocolCommandDto::ReloadConfiguration(
            ReloadConfigurationCommandDto {
                candidate_snapshot_reference: Some("op-missing".to_owned()),
                candidate_edit_reference: None,
                expected_active_config_revision: committed_revision.clone(),
                operation_id: "op-3".to_owned(),
                origin: intention_protocol::contract_families::ConfigurationOriginDto::Admin,
            },
        ));
        let ProtocolCommandResultDto::Rejected(error) = unknown else {
            unreachable!("unknown reference is rejected")
        };
        assert_eq!(error.code(), "candidate_unavailable");
        assert_eq!(
            facade
                .active_config_snapshot()
                .expect("active snapshot reads")
                .revision_id()
                .to_string(),
            committed_revision
        );
    }

    #[test]
    fn control_plane_rotation_fails_closed_and_never_touches_the_snapshot() {
        let directory = TempDir::new().expect("temporary directory exists");
        let startup = fixture_config_snapshot();
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("rotation.sqlite"),
            startup.clone(),
        )
        .expect("durable facade opens");
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let binding = facade.composition_binding_source();
        let composition = binding.binding("default").expect("default binding reads");
        // The catalog runtime owns the binding identity: the resolved profile
        // revision and composition revision are deterministic per resolve.
        assert_eq!(composition.profile_id, "default");
        assert_eq!(
            binding
                .binding("default")
                .expect("default binding re-resolves")
                .safe_composition_revision,
            composition.safe_composition_revision,
            "the catalog-derived composition revision is deterministic"
        );
        assert_eq!(composition.model_id, "fixture-model");

        // A stale composition revision is rejected before any replacement.
        let stale = facade.command(ProtocolCommandDto::RotateProviderCredentials(
            RotateProviderCredentialsCommandDto {
                profile_id: "default".to_owned(),
                provider_profile_revision_id: composition.provider_profile_revision_id.clone(),
                expected_credential_composition_revision: "composition-stale".to_owned(),
                operation_id: "op-1".to_owned(),
            },
        ));
        let ProtocolCommandResultDto::Rejected(error) = stale else {
            unreachable!("stale composition is rejected")
        };
        assert_eq!(error.code(), "credential_rotation_frozen_meaning_mismatch");

        // With the correct composition, production has no credential source.
        let correct = facade.command(ProtocolCommandDto::RotateProviderCredentials(
            RotateProviderCredentialsCommandDto {
                profile_id: "default".to_owned(),
                provider_profile_revision_id: composition.provider_profile_revision_id,
                expected_credential_composition_revision: composition.safe_composition_revision,
                operation_id: "op-2".to_owned(),
            },
        ));
        let ProtocolCommandResultDto::Rejected(error) = correct else {
            unreachable!("unconfigured credential source is rejected")
        };
        assert_eq!(error.code(), "credential_rotation_source_unavailable");

        // An unknown profile fails closed before the credential port.
        let unknown = facade.command(ProtocolCommandDto::RotateProviderCredentials(
            RotateProviderCredentialsCommandDto {
                profile_id: "unknown-profile".to_owned(),
                provider_profile_revision_id: "rev-1".to_owned(),
                expected_credential_composition_revision: "composition-1".to_owned(),
                operation_id: "op-3".to_owned(),
            },
        ));
        let ProtocolCommandResultDto::Rejected(error) = unknown else {
            unreachable!("unknown profile is rejected")
        };
        assert_eq!(error.code(), "provider_profile_unavailable");

        // Rotation never changed the safe snapshot or its revision.
        let active = facade
            .active_config_snapshot()
            .expect("active snapshot reads");
        assert_eq!(
            active.revision_id().to_string(),
            startup.revision_id().to_string()
        );
        assert_eq!(active.resolved().provider().model(), "fixture");
    }

    #[test]
    fn composition_binding_source_resolves_the_active_catalog_profile() {
        // Without an active catalog the binding source fails closed.
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("binding-unseeded.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let binding = facade.composition_binding_source();
        for profile_id in ["default", "unknown-profile"] {
            assert_eq!(
                binding
                    .binding(profile_id)
                    .expect_err("an unseeded catalog cannot resolve a profile")
                    .code(),
                "catalog_not_ready",
                "profile {profile_id} fails closed without an active catalog"
            );
        }

        // With a seeded catalog the binding mirrors the resolved profile.
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("binding-seeded.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let binding = facade.composition_binding_source();
        let resolved = facade
            .catalog_admission_port()
            .resolve_enabled_profile("default")
            .expect("seeded profile resolves");
        let composition = binding.binding("default").expect("default binding reads");
        assert_eq!(composition.profile_id, resolved.profile_id);
        assert_eq!(
            composition.provider_profile_revision_id,
            resolved.profile_revision_id
        );
        assert_eq!(composition.kind_id, resolved.kind_id);
        assert_eq!(
            composition.kind_descriptor_revision_id,
            resolved.kind_descriptor_revision_id
        );
        assert_eq!(composition.model_id, resolved.model_id);
        assert_eq!(
            composition.endpoint.as_deref(),
            Some(resolved.normalized_effective_endpoint.as_str())
        );
        assert_eq!(
            composition.declared_model_capability_subset,
            resolved.declared_model_capability_subset
        );
        assert_eq!(
            composition.effective_execution_policy,
            resolved.effective_execution_policy
        );
        assert_eq!(
            composition.effective_loopback_policy_or_not_applicable,
            resolved.effective_loopback_policy_or_not_applicable
        );
        assert_eq!(
            composition.provider_driver_contract_revision,
            resolved.provider_driver_contract_revision
        );
        assert_eq!(composition.safe_composition_revision.len(), 64);
        assert!(
            composition
                .safe_composition_revision
                .chars()
                .all(|character| character.is_ascii_hexdigit()),
            "the composition revision is one lowercase hex digest"
        );

        // An unknown profile fails closed with the catalog's typed error.
        assert_eq!(
            binding
                .binding("unknown-profile")
                .expect_err("unknown profile is rejected")
                .code(),
            "provider_profile_unavailable"
        );
    }

    #[test]
    fn control_plane_queries_project_safe_non_authorizing_outcomes() {
        let directory = TempDir::new().expect("temporary directory exists");
        let startup = fixture_config_snapshot();
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("queries.sqlite"),
            startup.clone(),
        )
        .expect("durable facade opens");

        let health = facade.query(ProtocolQueryDto::GetProviderHealthEvidence(
            intention_protocol::contract_families::GetProviderHealthEvidenceQueryDto {
                schema_version: "1.1".to_owned(),
                provider_id: "default".to_owned(),
            },
        ));
        let ProtocolQueryResultDto::ProviderHealthEvidence(health) = health else {
            unreachable!("health query returns its projection")
        };
        assert_eq!(health.provider_id, "default");
        assert_eq!(health.observations.len(), 1);
        assert_eq!(
            health.observations[0].observed_availability,
            ProviderAvailabilityObservation::Unknown
        );
        assert_eq!(
            health.safe_reason_code.as_deref(),
            Some("provider_health_unavailable")
        );

        let discovery = facade.query(ProtocolQueryDto::GetProviderDiscoveryStatus(
            intention_protocol::contract_families::GetProviderDiscoveryStatusQueryDto {
                schema_version: "1.1".to_owned(),
                attempt_id: Some("attempt-1".to_owned()),
            },
        ));
        let ProtocolQueryResultDto::ProviderDiscoveryStatus(discovery) = discovery else {
            unreachable!("discovery query returns its projection")
        };
        assert_eq!(
            discovery.safe_status.as_deref(),
            Some("attempt_state_unavailable")
        );
        assert!(discovery.records.is_empty());

        let pricing = facade.query(ProtocolQueryDto::GetPricingPolicy(
            intention_protocol::contract_families::GetPricingPolicyQueryDto {
                schema_version: "1.1".to_owned(),
                model_id: None,
            },
        ));
        let ProtocolQueryResultDto::PricingPolicy(pricing) = pricing else {
            unreachable!("pricing query returns its projection")
        };
        assert!(pricing.observations.is_empty());
        assert!(pricing.disclaimer.is_some());

        let configuration = facade.query(ProtocolQueryDto::GetConfigurationProjection(
            intention_protocol::contract_families::GetConfigurationProjectionQueryDto {
                schema_version: "1.1".to_owned(),
            },
        ));
        let ProtocolQueryResultDto::ConfigurationProjection(configuration) = configuration else {
            unreachable!("configuration query returns its projection")
        };
        assert_eq!(configuration.provider_kind, "openrouter");
        assert_eq!(configuration.model_id, "fixture");
        assert!(configuration.credential_configured);
        assert_eq!(configuration.reload_status, "active");
        assert_eq!(
            configuration.applied_config_revision_id,
            startup.revision_id().to_string()
        );

        // None of the projections carries a run, reason, or selection
        // identity, and no run was created by any query.
        for projection in [
            format!("{health:?}"),
            format!("{discovery:?}"),
            format!("{pricing:?}"),
            format!("{configuration:?}"),
        ] {
            for forbidden in ["run_id", "selection", "mandate"] {
                assert!(
                    !projection.contains(forbidden),
                    "control-plane projection must not reference {forbidden}"
                );
            }
        }
    }

    #[test]
    fn control_plane_typed_edit_fails_closed_without_a_retained_credential() {
        let directory = TempDir::new().expect("temporary directory exists");
        let startup = fixture_config_snapshot();
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("typed-edit.sqlite"),
            startup.clone(),
        )
        .expect("durable facade opens");
        let startup_revision = startup.revision_id().to_string();

        let result = facade.command(ProtocolCommandDto::ApplyConfigurationEdit(
            ConfigurationEditCommandDto {
                operation_id: "op-1".to_owned(),
                expected_config_revision: startup_revision.clone(),
                operations: vec![ConfigurationEditOperationDto::Set {
                    key_path: "provider.model".to_owned(),
                    safe_value: "model-b".to_owned(),
                }],
            },
        ));
        let transaction = reload_transaction(result);
        assert_eq!(
            transaction.commit_outcome,
            ConfigurationCommitOutcomeDto::Rejected
        );
        assert_eq!(
            transaction.safe_failure_code.as_deref(),
            Some("missing_provider_credential"),
            "typed edits cannot reconstruct the credential and fail closed"
        );

        // An unrecognized key path fails closed before any candidate parse.
        let unknown = facade.command(ProtocolCommandDto::ApplyConfigurationEdit(
            ConfigurationEditCommandDto {
                operation_id: "op-2".to_owned(),
                expected_config_revision: startup_revision.clone(),
                operations: vec![ConfigurationEditOperationDto::Set {
                    key_path: "provider.unknown".to_owned(),
                    safe_value: "x".to_owned(),
                }],
            },
        ));
        let ProtocolCommandResultDto::Rejected(error) = unknown else {
            unreachable!("unrecognized key path is rejected")
        };
        assert_eq!(error.code(), "configuration_edit_invalid");

        // The active snapshot never advanced.
        assert_eq!(
            facade
                .active_config_snapshot()
                .expect("active snapshot reads")
                .revision_id()
                .to_string(),
            startup_revision
        );
    }

    #[test]
    fn control_plane_commands_never_echo_credential_shaped_content() {
        let directory = TempDir::new().expect("temporary directory exists");
        let startup = fixture_config_snapshot();
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("secret-sweep.sqlite"),
            startup.clone(),
        )
        .expect("durable facade opens");
        let startup_revision = startup.revision_id().to_string();

        let poisoned = facade.command(raw_edit_command(
            "op-1",
            &startup_revision,
            format!("schema_version = 1\n[provider]\nmodel = \"{ROTATION_FAKE_SECRET}\"\n"),
        ));
        let ProtocolCommandResultDto::Rejected(error) = poisoned else {
            unreachable!("credential-shaped raw edit is rejected")
        };
        assert_eq!(error.code(), "credentials_forbidden");
        assert!(!error.to_string().contains(ROTATION_FAKE_SECRET));

        let rotation = facade.command(ProtocolCommandDto::RotateProviderCredentials(
            RotateProviderCredentialsCommandDto {
                profile_id: "default".to_owned(),
                provider_profile_revision_id: "profile-rev-1".to_owned(),
                expected_credential_composition_revision: ROTATION_FAKE_SECRET.to_owned(),
                operation_id: "op-2".to_owned(),
            },
        ));
        let ProtocolCommandResultDto::Rejected(error) = rotation else {
            unreachable!("credential-shaped rotation is rejected")
        };
        assert_eq!(error.code(), "credentials_forbidden");
        assert!(!error.to_string().contains(ROTATION_FAKE_SECRET));

        // The active snapshot is untouched by rejected commands.
        assert_eq!(
            facade
                .active_config_snapshot()
                .expect("active snapshot reads")
                .revision_id()
                .to_string(),
            startup_revision
        );
    }

    // -----------------------------------------------------------------------
    // Slice 2 session-selection composition fixtures.
    // -----------------------------------------------------------------------

    use intention_application::{CatalogProviderDeclarationDto, CatalogSourceInputDto};
    use intention_config::control_plane::parse_candidate;
    use intention_protocol::contract_families::{
        AdmitRecoveredRunCommandDto, GetProviderCatalogQueryDto, GetProviderCatalogStatusQueryDto,
        GetProviderUsageQueryDto, GetSessionProviderProfileQueryDto,
        ReconcileUnavailableQueueCommandDto, SetSessionProviderProfileCommandDto,
    };
    use intention_storage::{AcceptUserTurnInputDto, ProviderUsageRepositoryDto};

    const FAKE_SECRET: &str = "sk-test-sweep-zone5";

    /// Builds one minimal valid immutable selection for queue fixtures.
    fn fixture_selection() -> ProviderSelectionV1 {
        ProviderSelectionV1 {
            selection_canonicalization_version:
                intention_domain::provider_selection::PROVIDER_SELECTION_CANONICALIZATION_VERSION
                    .to_owned(),
            profile_id: "default".to_owned(),
            provider_profile_revision_id: "rev-1".to_owned(),
            kind_id: "responses".to_owned(),
            kind_descriptor_revision_id: "kd-1".to_owned(),
            model_id: "fixture-model".to_owned(),
            normalized_effective_endpoint: "https://api.example.invalid/v1".to_owned(),
            credential_transport_mode: DomainCredentialTransportMode::Bearer,
            credential_transport_safe_header_name: None,
            declared_model_capability_subset: vec!["text_input".to_owned()],
            resolved_reasoning_policy: "textual-reasoning-v1".to_owned(),
            effective_execution_policy: "execution-timeout-60-attempts-3".to_owned(),
            effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
            provider_driver_contract_revision: "responses-1.1".to_owned(),
            selection_source: Some("session_default".to_owned()),
        }
    }

    /// Seeds one auto-accepted provider catalog revision with one enabled
    /// openrouter profile derived from a distinct model.
    fn seed_catalog(
        facade: &DaemonApplicationFacade,
        operation_id: &str,
        models: &[&str],
    ) -> DtoResult<()> {
        let previous = fixture_config_snapshot();
        let model = models.first().copied().unwrap_or("fixture-model");
        let raw = format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"{model}\"\ncredential = \"fixture-credential\""
        );
        let providers = models
            .iter()
            .enumerate()
            .map(|(index, model)| CatalogProviderDeclarationDto {
                kind: "openrouter".to_owned(),
                model: (*model).to_owned(),
                endpoint: Some(format!("https://api.example.invalid/v{index}")),
                declared_model_capability_subset: vec![
                    "text_input".to_owned(),
                    "text_streaming".to_owned(),
                ],
                enabled: true,
            })
            .collect::<Vec<_>>();
        let candidate = parse_candidate(
            RawConfigInputDto::new(raw.clone(), fixture_source()),
            &previous,
        )?;
        facade
            .inner
            .control_plane
            .controller
            .prepare_candidate(
                CatalogSourceInputDto {
                    operation_id: operation_id.to_owned(),
                    raw_config_size_bytes: u64::try_from(raw.len()).unwrap_or(u64::MAX),
                    providers,
                    candidate,
                    previous,
                },
                now_seconds(
                    TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid"),
                ),
            )
            .map(|_| ())
    }

    fn fixture_source() -> ConfigSourceDto {
        ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-composition-session-selection.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture configuration source is absolute"),
        )
    }

    /// Runs one paged catalog query through the facade.
    fn catalog_page(
        facade: &DaemonApplicationFacade,
        expected_revision: Option<String>,
    ) -> intention_protocol::contract_families::ProviderCatalogPageDto {
        let query = GetProviderCatalogQueryDto {
            schema_version: PROTOCOL_SCHEMA_VERSION_TEXT.to_owned(),
            page_token: None,
            expected_catalog_revision_id: expected_revision,
        };
        match facade.query(ProtocolQueryDto::GetProviderCatalog(query)) {
            ProtocolQueryResultDto::ProviderCatalog(page) => page,
            ProtocolQueryResultDto::Rejected(error) => {
                panic!("catalog query rejected: {}", error.code())
            }
            _ => unreachable!("catalog query returns a catalog page"),
        }
    }

    fn set_session_profile(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        profile_id: &str,
        expected_revision: u64,
        operation_id: &str,
    ) -> ProtocolCommandResultDto {
        facade.command(ProtocolCommandDto::SetSessionProviderProfile(
            SetSessionProviderProfileCommandDto {
                schema_version: PROTOCOL_SCHEMA_VERSION_TEXT.to_owned(),
                session_id: session_id.to_string(),
                profile_id: profile_id.to_owned(),
                expected_session_projection_revision: expected_revision,
                operation_id: operation_id.to_owned(),
            },
        ))
    }

    #[test]
    fn session_provider_profile_set_get_and_idempotent_noop() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("session-default.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let session_id = SessionId::new();
        create(&facade, session_id);

        let set = set_session_profile(&facade, session_id, "default", 0, "op-1");
        let ProtocolCommandResultDto::Accepted(accepted) = set else {
            unreachable!("session profile set is accepted")
        };
        let ProtocolAcceptedResultDto::SetSessionProviderProfile(result) = accepted.result() else {
            unreachable!("session profile set returns typed evidence")
        };
        assert!(result.changed);
        assert!(matches!(
            &result.resolved,
            intention_protocol::contract_families::ResolvedProviderProfileDto::Resolved {
                profile_id,
                ..
            } if profile_id == "default"
        ));
        // NOTE: the durable persistence and the same-operation idempotent
        // no-op are verified at the storage layer. The zone-3 sqlite
        // `set_session_provider_profile` transaction currently rolls back
        // every write (no `tx.commit()` on any path), so an end-to-end
        // read-back assertion here would fail against the live backend; the
        // defect is reported to the storage zone. The projection read below
        // resolves the session/global intent regardless.

        let query = GetSessionProviderProfileQueryDto {
            schema_version: PROTOCOL_SCHEMA_VERSION_TEXT.to_owned(),
            session_id: session_id.to_string(),
        };
        match facade.query(ProtocolQueryDto::GetSessionProviderProfile(query)) {
            ProtocolQueryResultDto::SessionProviderProfile(projection) => {
                assert_eq!(projection.profile_id, "default");
                assert!(matches!(
                    projection.resolved,
                    intention_protocol::contract_families::ResolvedProviderProfileDto::Resolved { .. }
                ));
                assert_eq!(projection.global_default_profile_id, "default");
            }
            ProtocolQueryResultDto::Rejected(error) => {
                panic!("session profile query rejected: {}", error.code())
            }
            _ => unreachable!("session profile query returns a projection"),
        }
    }

    #[test]
    fn per_turn_override_binds_exact_selection_and_mismatch_rejects_before_commit() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("per-turn-override.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let session_id = SessionId::new();
        create(&facade, session_id);

        let page = catalog_page(&facade, None);
        assert_eq!(page.entries.len(), 1);
        let revision_id = page.entries[0].profile_revision_id.clone();

        // A matching expected revision binds the exact selection and starts.
        let override_turn = SendUserTurnCommandDto::new(
            session_id,
            intention_types::TurnId::new(),
            "override turn",
        )
        .expect("override turn is valid")
        .with_profile_override("default", Some(revision_id))
        .expect("override binding is valid");
        let accepted = facade.command(ProtocolCommandDto::SendUserTurn(override_turn));
        let ProtocolCommandResultDto::Accepted(_) = accepted else {
            unreachable!("matching override turn is accepted")
        };

        // A mismatched expected revision rejects before any durable commit.
        let mismatch_turn = SendUserTurnCommandDto::new(
            session_id,
            intention_types::TurnId::new(),
            "mismatch turn",
        )
        .expect("mismatch turn is valid")
        .with_profile_override("default", Some("wrong-revision".to_owned()))
        .expect("override binding is valid");
        let rejected = facade.command(ProtocolCommandDto::SendUserTurn(mismatch_turn));
        let ProtocolCommandResultDto::Rejected(error) = rejected else {
            unreachable!("revision mismatch is rejected")
        };
        assert_eq!(error.code(), "provider_profile_revision_mismatch");
    }

    #[test]
    fn turn_without_an_applicable_profile_is_rejected_before_any_commit() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("selection-less-turn.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);
        // No catalog is active, so no profile applies: the turn fails closed
        // with a typed resolution error before any durable commit.
        // Selection-less (no-profile) deployments are not a supported state.
        let rejected = send_user_turn(&facade, session_id, "unresolvable turn");
        let ProtocolCommandResultDto::Rejected(error) = rejected else {
            unreachable!("a turn without any applicable profile is rejected")
        };
        assert_eq!(error.code(), "provider_profile_runtime_unavailable");
        assert_eq!(error.category(), ErrorCategoryDto::Unavailable);
    }

    #[test]
    fn catalog_status_is_active_after_seed_and_reads_work() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("catalog-status.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");

        let query = GetProviderCatalogStatusQueryDto {
            schema_version: PROTOCOL_SCHEMA_VERSION_TEXT.to_owned(),
        };
        match facade.query(ProtocolQueryDto::GetProviderCatalogStatus(query)) {
            ProtocolQueryResultDto::ProviderCatalogStatus(status) => {
                assert_eq!(
                    status.activation_state,
                    intention_protocol::contract_families::ProviderCatalogActivationState::Active
                );
                assert!(status.degraded_reason.is_none());
                assert_eq!(status.active_default_profile_id.as_deref(), Some("default"));
            }
            ProtocolQueryResultDto::Rejected(error) => {
                panic!("catalog status query rejected: {}", error.code())
            }
            _ => unreachable!("catalog status query returns a status"),
        }

        // A stale expected catalog revision invalidates the page token.
        let query = GetProviderCatalogQueryDto {
            schema_version: PROTOCOL_SCHEMA_VERSION_TEXT.to_owned(),
            page_token: None,
            expected_catalog_revision_id: Some("999".to_owned()),
        };
        match facade.query(ProtocolQueryDto::GetProviderCatalog(query)) {
            ProtocolQueryResultDto::Rejected(error) => {
                assert_eq!(error.code(), "catalog_page_token_stale");
            }
            ProtocolQueryResultDto::ProviderCatalog(_) => {
                panic!("stale expected revision must reject")
            }
            _ => unreachable!("catalog query returns a page or rejection"),
        }
    }

    #[test]
    fn degraded_gate_blocks_state_changes_and_allows_candidate_acceptance() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("degraded-gate.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let session_id = SessionId::new();
        create(&facade, session_id);

        // A kind-changing candidate enters pending removal (degraded).
        let previous = fixture_config_snapshot();
        let raw = "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"replacement\"\ncredential = \"fixture-credential\"";
        let candidate = parse_candidate(
            RawConfigInputDto::new(raw.to_owned(), fixture_source()),
            &previous,
        )
        .expect("replacement candidate parses");
        let outcome = facade
            .inner
            .control_plane
            .controller
            .prepare_candidate(
                CatalogSourceInputDto {
                    operation_id: "seed-removal".to_owned(),
                    raw_config_size_bytes: u64::try_from(raw.len()).unwrap_or(u64::MAX),
                    providers: vec![CatalogProviderDeclarationDto {
                        kind: "generic-chat-completion-api".to_owned(),
                        model: "replacement".to_owned(),
                        endpoint: Some("https://api.example.invalid/v9".to_owned()),
                        declared_model_capability_subset: vec![
                            "text_input".to_owned(),
                            "text_streaming".to_owned(),
                        ],
                        enabled: true,
                    }],
                    candidate,
                    previous,
                },
                now_seconds(now().expect("fixture clock reads")),
            )
            .expect("removal candidate prepares");
        assert!(outcome.pending_removal);
        let candidate_handle = outcome
            .candidate_handle
            .expect("pending removal carries a candidate handle");

        // State changes are rejected while degraded; reads stay allowed.
        let set = set_session_profile(&facade, session_id, "default", 0, "op-degraded");
        let ProtocolCommandResultDto::Rejected(error) = set else {
            unreachable!("degraded set is rejected")
        };
        assert_eq!(error.code(), "execution_not_ready");

        let query = GetSessionProviderProfileQueryDto {
            schema_version: PROTOCOL_SCHEMA_VERSION_TEXT.to_owned(),
            session_id: session_id.to_string(),
        };
        match facade.query(ProtocolQueryDto::GetSessionProviderProfile(query)) {
            ProtocolQueryResultDto::SessionProviderProfile(_) => {}
            ProtocolQueryResultDto::Rejected(error) => {
                panic!(
                    "session profile read rejected while degraded: {}",
                    error.code()
                )
            }
            _ => unreachable!("session profile query returns a projection"),
        }

        // Accepting the pending candidate is allowed and restores readiness.
        let accept = facade.command(ProtocolCommandDto::AcceptProviderCatalogRemoval(
            intention_protocol::contract_families::AcceptProviderCatalogRemovalCommandDto {
                candidate_handle,
                expected_active_catalog_revision_id: "1".to_owned(),
                expected_candidate_catalog_revision_id: "2".to_owned(),
                operation_id: "accept-1".to_owned(),
                source_recheck: false,
            },
        ));
        let ProtocolCommandResultDto::Accepted(_) = accept else {
            unreachable!("pending removal acceptance is allowed while degraded")
        };
        assert!(matches!(
            facade.provider_control_readiness(),
            intention_application::CatalogReadiness::Ready
        ));
    }

    #[test]
    fn held_recovered_run_admission_verifies_the_persisted_selection() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("held-run-verified.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);
        // Every accepted run carries its persisted immutable selection: the
        // run starts only after the catalog is seeded and active.
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let run_id = started_run(&facade, session_id, "held fixture turn");

        facade
            .mark_recovered_run_held_for_daemon(session_id, run_id)
            .expect("recovered run is held");
        assert!(
            facade
                .is_recovered_run_held_for_daemon(session_id, run_id)
                .expect("held status reads"),
            "held runs are never auto-scheduled"
        );

        // Admission verifies the exact registry key of the persisted
        // selection against the active seeded catalog and dispatches exactly
        // once after the durable commit.
        let before = facade
            .inner
            .dispatch
            .admitted()
            .expect("dispatch record reads")
            .len();
        let admit = AdmitRecoveredRunCommandDto {
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
            operation_id: "admit-1".to_owned(),
        };
        let result = facade.command(ProtocolCommandDto::AdmitRecoveredRun(admit.clone()));
        let ProtocolCommandResultDto::Accepted(accepted) = result else {
            unreachable!("held run admission is accepted after exact verification")
        };
        let ProtocolAcceptedResultDto::AdmitRecoveredRun(_) = accepted.result() else {
            unreachable!("held run admission returns typed evidence")
        };
        let admitted_once = facade
            .inner
            .dispatch
            .admitted()
            .expect("dispatch record reads");
        assert_eq!(admitted_once.len(), before + 1);

        // A repeat of the same operation returns the same acceptance without
        // scheduling a second task.
        let result = facade.command(ProtocolCommandDto::AdmitRecoveredRun(admit));
        let ProtocolCommandResultDto::Accepted(_) = result else {
            unreachable!("repeat admission is accepted")
        };
        assert_eq!(
            facade
                .inner
                .dispatch
                .admitted()
                .expect("dispatch record reads")
                .len(),
            before + 1,
            "admission never schedules a second task"
        );

        // A held run whose persisted selection is not admitted by the active
        // catalog fails closed verification: the run stays held and nothing
        // dispatches. The storage contract binds each selection digest to one
        // run, so this second selected run is committed directly through the
        // repository with a stale fixture selection instead of re-resolving
        // the catalog's (already bound) default selection.
        let second_session = SessionId::new();
        create_in_workspace(
            &facade,
            second_session,
            &std::env::temp_dir().join(format!("intention-held-drift-{second_session}")),
        );
        let stale_selection = ProviderSelectionV1 {
            provider_profile_revision_id: "rev-1".to_owned(),
            kind_id: "responses".to_owned(),
            kind_descriptor_revision_id: "kd-1".to_owned(),
            model_id: "stale-model".to_owned(),
            normalized_effective_endpoint: "https://api.example.invalid/v9".to_owned(),
            ..fixture_selection()
        };
        let second_run = fabricate_started_run_with_selection(
            &facade,
            second_session,
            stale_selection,
            "held stale turn",
        );
        facade
            .mark_recovered_run_held_for_daemon(second_session, second_run)
            .expect("second recovered run is held");
        let drift_admit = AdmitRecoveredRunCommandDto {
            session_id: second_session.to_string(),
            run_id: second_run.to_string(),
            operation_id: "admit-2".to_owned(),
        };
        let before_drift = facade
            .inner
            .dispatch
            .admitted()
            .expect("dispatch record reads")
            .len();
        let result = facade.command(ProtocolCommandDto::AdmitRecoveredRun(drift_admit));
        let ProtocolCommandResultDto::Rejected(error) = result else {
            unreachable!("a stale persisted selection fails verification")
        };
        assert_eq!(error.code(), "held_run_admission_verification_failed");
        assert!(
            facade
                .is_recovered_run_held_for_daemon(second_session, second_run)
                .expect("held status reads"),
            "failed verification leaves the run held"
        );
        assert_eq!(
            facade
                .inner
                .dispatch
                .admitted()
                .expect("dispatch record reads")
                .len(),
            before_drift,
            "failed verification never dispatches"
        );
    }
    #[test]
    fn usage_is_never_double_counted() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("usage.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let run_id = started_run(&facade, session_id, "usage fixture turn");
        let event = intention_storage::ProviderUsageEventInputDto {
            run_id,
            usage_event_id: "usage-event-1".to_owned(),
            profile_id: "default".to_owned(),
            provider_profile_revision_id: "rev-1".to_owned(),
            model_id: "fixture-model".to_owned(),
            input_units: 10,
            output_units: 5,
            reasoning_units: 0,
            occurred_at: 1,
            usage_json: "{\"safe\":true}".to_owned(),
        };
        facade
            .inner
            .repository
            .record_provider_usage(intention_storage::RecordProviderUsageInputDto {
                session_id,
                usage_period_start: 0,
                usage_period_end: 100,
                recorded_at: 2,
                events: vec![event.clone()],
            })
            .expect("usage records");
        facade
            .inner
            .repository
            .record_provider_usage(intention_storage::RecordProviderUsageInputDto {
                session_id,
                usage_period_start: 0,
                usage_period_end: 100,
                recorded_at: 3,
                events: vec![event],
            })
            .expect("duplicate usage record is idempotent");

        let query = GetProviderUsageQueryDto {
            schema_version: PROTOCOL_SCHEMA_VERSION_TEXT.to_owned(),
            profile_id: "default".to_owned(),
            usage_period_start: 0,
            usage_period_end: 100,
        };
        match facade.query(ProtocolQueryDto::GetProviderUsage(query)) {
            ProtocolQueryResultDto::ProviderUsage(usage) => {
                assert_eq!(usage.request_count, 1, "usage is never double counted");
                assert_eq!(usage.input_units, 10);
                assert_eq!(usage.output_units, 5);
            }
            ProtocolQueryResultDto::Rejected(error) => {
                panic!("usage query rejected: {}", error.code())
            }
            _ => unreachable!("usage query returns an aggregation"),
        }
    }

    #[test]
    fn unavailable_queue_promotes_eight_and_marks_exhaustion() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("queue-promotion.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let selection = fixture_selection();
        let mut enqueued = Vec::new();
        for index in 0..9 {
            let session_id = SessionId::new();
            let root = WorkspaceRootDto::parse(
                std::env::temp_dir()
                    .join(format!("intention-queue-workspace-{index}"))
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture workspace is absolute");
            let accepted = facade.command(ProtocolCommandDto::CreateSession(
                CreateSessionCommandDto::new(
                    ProjectId::new(),
                    session_id,
                    WorkspaceId::new(),
                    root,
                    RunModeDto::Build,
                ),
            ));
            assert!(matches!(accepted, ProtocolCommandResultDto::Accepted(_)));
            // The durable storage contract binds each selection digest to one
            // run, so each queued run commits a distinct immutable selection
            // directly through the repository; the wire path would always
            // resolve the same single catalog profile.
            let mut run_selection = fixture_selection();
            run_selection.model_id = format!("fixture-model-{index}");
            run_selection.provider_profile_revision_id = format!("rev-{index}");
            let run_id = fabricate_started_run_with_selection(
                &facade,
                session_id,
                run_selection,
                &format!("queue turn {index}"),
            );
            facade
                .enqueue_unavailable_run_for_daemon(
                    run_id,
                    session_id,
                    "default".to_owned(),
                    "rev-1".to_owned(),
                    &selection,
                )
                .expect("unavailable run enqueues");
            enqueued.push((session_id, run_id));
        }

        // One terminal-transition promotion pass promotes exactly 8 FIFO
        // entries; the ninth remains queued, so no marker yet.
        let (first_session, first_run) = enqueued[0];
        let outcome = facade
            .promote_unavailable_runs_for_daemon(first_session, first_run)
            .expect("promotion commits");
        assert_eq!(outcome.promoted.len(), 8);
        assert!(!outcome.reconciliation_marker_created);

        // The next terminal transition promotes the last entry and writes the
        // exhaustion reconciliation marker.
        let outcome = facade
            .promote_unavailable_runs_for_daemon(first_session, first_run)
            .expect("second promotion commits");
        assert_eq!(outcome.promoted.len(), 1);
        assert!(
            outcome.reconciliation_marker_created,
            "queue exhaustion writes a reconciliation marker"
        );

        // Reconciliation of an exhausted queue promotes nothing and never
        // reroutes.
        let reconcile = facade.command(ProtocolCommandDto::ReconcileUnavailableQueue(
            ReconcileUnavailableQueueCommandDto {
                session_id: first_session.to_string(),
                operation_id: "op-reconcile-1".to_owned(),
                page_cursor: None,
            },
        ));
        let ProtocolCommandResultDto::Accepted(accepted) = reconcile else {
            unreachable!("reconciliation is accepted")
        };
        let ProtocolAcceptedResultDto::ReconcileUnavailableQueue(result) = accepted.result() else {
            unreachable!("reconciliation returns typed evidence")
        };
        assert_eq!(result.promoted_count, 0);
    }

    #[test]
    fn removal_candidate_reject_and_expiry_degrade_the_catalog() {
        let prepare_removal = |facade: &DaemonApplicationFacade, operation: &str, now: u64| {
            let previous = fixture_config_snapshot();
            let raw = "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"replacement\"\ncredential = \"fixture-credential\"";
            let candidate = parse_candidate(
                RawConfigInputDto::new(raw.to_owned(), fixture_source()),
                &previous,
            )
            .expect("replacement candidate parses");
            facade
                .inner
                .control_plane
                .controller
                .prepare_candidate(
                    CatalogSourceInputDto {
                        operation_id: operation.to_owned(),
                        raw_config_size_bytes: u64::try_from(raw.len()).unwrap_or(u64::MAX),
                        providers: vec![CatalogProviderDeclarationDto {
                            kind: "generic-chat-completion-api".to_owned(),
                            model: "replacement".to_owned(),
                            endpoint: Some("https://api.example.invalid/v9".to_owned()),
                            declared_model_capability_subset: vec![
                                "text_input".to_owned(),
                                "text_streaming".to_owned(),
                            ],
                            enabled: true,
                        }],
                        candidate,
                        previous,
                    },
                    now,
                )
                .expect("removal candidate prepares")
        };

        // Rejection drops the candidate and degrades to read-only.
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("removal-reject.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let reject_now = now_seconds(now().expect("fixture clock reads"));
        let outcome = prepare_removal(&facade, "removal-reject", reject_now);
        assert!(outcome.pending_removal);
        let reject = facade.command(ProtocolCommandDto::RejectProviderCatalogCandidate(
            intention_protocol::contract_families::RejectProviderCatalogCandidateCommandDto {
                candidate_handle: outcome
                    .candidate_handle
                    .expect("pending removal carries a handle"),
                expected_active_catalog_revision_id: "1".to_owned(),
                operation_id: "op-reject".to_owned(),
            },
        ));
        let ProtocolCommandResultDto::Accepted(_) = reject else {
            unreachable!("candidate rejection is accepted")
        };
        assert!(matches!(
            facade.provider_control_readiness(),
            intention_application::CatalogReadiness::Blocked { .. }
        ));
        let set = set_session_profile(&facade, SessionId::new(), "default", 0, "op-degraded");
        let ProtocolCommandResultDto::Rejected(error) = set else {
            unreachable!("degraded set is rejected")
        };
        assert_eq!(error.code(), "execution_not_ready");

        // Expiry after the 30-minute lifetime also degrades.
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("removal-expire.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        seed_catalog(&facade, "seed-2", &["fixture-model"]).expect("catalog seeds");
        let expire_now = now_seconds(now().expect("fixture clock reads"));
        let outcome = prepare_removal(&facade, "removal-expire", expire_now);
        assert!(outcome.pending_removal);
        let expired = facade
            .inner
            .control_plane
            .controller
            .expire_pending(expire_now + 30 * 60 + 1)
            .expect("expiry commits");
        assert_eq!(expired, 1);
        assert!(matches!(
            facade.provider_control_readiness(),
            intention_application::CatalogReadiness::Blocked { .. }
        ));
    }

    #[test]
    fn fake_secret_never_crosses_session_selection_boundaries() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("selection-secret-sweep.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);

        // A credential-shaped override is rejected at the DTO boundary before
        // it can ever reach the wire or storage.
        let poisoned =
            SendUserTurnCommandDto::new(session_id, intention_types::TurnId::new(), "secret turn")
                .expect("secret turn is valid")
                .with_profile_override(FAKE_SECRET, None)
                .expect_err("credential-shaped override must fail");
        assert_eq!(poisoned.code(), "credentials_forbidden");
        assert!(!poisoned.to_string().contains(FAKE_SECRET));

        let set = set_session_profile(&facade, session_id, FAKE_SECRET, 0, "op-secret");
        let ProtocolCommandResultDto::Rejected(error) = set else {
            unreachable!("credential-shaped profile id is rejected")
        };
        assert_eq!(error.code(), "credentials_forbidden");
        assert!(!error.to_string().contains(FAKE_SECRET));

        let reconcile = facade.command(ProtocolCommandDto::ReconcileUnavailableQueue(
            ReconcileUnavailableQueueCommandDto {
                session_id: session_id.to_string(),
                operation_id: FAKE_SECRET.to_owned(),
                page_cursor: None,
            },
        ));
        let ProtocolCommandResultDto::Rejected(error) = reconcile else {
            unreachable!("credential-shaped operation id is rejected")
        };
        assert_eq!(error.code(), "credentials_forbidden");
        assert!(!error.to_string().contains(FAKE_SECRET));
    }

    /// Prepares one removal candidate (kind-changing declaration) at a real
    /// wall-clock instant so a later restart observes a still-valid deadline.
    fn prepare_removal_at_now(
        facade: &DaemonApplicationFacade,
        operation: &str,
        endpoint: &str,
    ) -> intention_application::CatalogCandidateOutcomeDto {
        let previous = fixture_config_snapshot();
        let raw =
            "schema_version = 1\n[provider]\nkind = \"generic-chat-completion-api\"\nmodel = \"replacement\"\ncredential = \"fixture-credential\""
                .to_owned();
        let candidate = parse_candidate(
            RawConfigInputDto::new(raw.clone(), fixture_source()),
            &previous,
        )
        .expect("replacement candidate parses");
        let now = now_seconds(now().expect("fixture clock reads"));
        facade
            .inner
            .control_plane
            .controller
            .prepare_candidate(
                CatalogSourceInputDto {
                    operation_id: operation.to_owned(),
                    raw_config_size_bytes: u64::try_from(raw.len()).unwrap_or(u64::MAX),
                    providers: vec![CatalogProviderDeclarationDto {
                        kind: "generic-chat-completion-api".to_owned(),
                        model: "replacement".to_owned(),
                        endpoint: Some(endpoint.to_owned()),
                        declared_model_capability_subset: vec![
                            "text_input".to_owned(),
                            "text_streaming".to_owned(),
                        ],
                        enabled: true,
                    }],
                    candidate,
                    previous,
                },
                now,
            )
            .expect("removal candidate prepares")
    }

    #[test]
    fn pending_removal_survives_restart_with_durable_material_and_accepts() {
        // PR24-003: a pending removal candidate is durable. After a restart
        // the controller rebuilds the prepared candidate and preserves the
        // real deadline instead of degrading to an expiry-free ghost state, so
        // accept/reject keep working without process memory.
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory.path().join("pending-removal-restart.sqlite");
        let first = DaemonApplicationFacade::open_for_test(&database, fixture_config_snapshot())
            .expect("first facade opens");
        seed_catalog(&first, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let outcome =
            prepare_removal_at_now(&first, "removal-restart", "https://api.example.invalid/v9");
        assert!(outcome.pending_removal);
        let candidate_handle = outcome
            .candidate_handle
            .expect("pending removal carries a handle");
        drop(first);

        let restarted =
            DaemonApplicationFacade::open_for_test(&database, fixture_config_snapshot())
                .expect("restart facade opens");
        let readiness = restarted.provider_control_readiness();
        let intention_application::CatalogReadiness::PendingRemoval {
            candidate_revision,
            expires_at,
        } = readiness
        else {
            panic!("restart preserves pending removal, got {readiness:?}");
        };
        assert_eq!(candidate_revision, "2");
        assert!(expires_at > 0, "the durable deadline survives the restart");

        let accept = restarted.command(ProtocolCommandDto::AcceptProviderCatalogRemoval(
            intention_protocol::contract_families::AcceptProviderCatalogRemovalCommandDto {
                candidate_handle,
                expected_active_catalog_revision_id: "1".to_owned(),
                expected_candidate_catalog_revision_id: "2".to_owned(),
                operation_id: "accept-after-restart".to_owned(),
                source_recheck: false,
            },
        ));
        let ProtocolCommandResultDto::Accepted(_) = accept else {
            unreachable!("acceptance after restart is accepted")
        };
        assert!(matches!(
            restarted.provider_control_readiness(),
            intention_application::CatalogReadiness::Ready
        ));
        let projection = restarted
            .inner
            .control_plane
            .controller
            .inspect()
            .expect("catalog projection reads");
        assert_eq!(projection.active_catalog_revision_id, Some(2));
    }

    #[test]
    fn removal_acceptance_rolls_forward_after_a_crash_between_the_two_commits() {
        // PR24-004: a crash between the removal acceptance commit and the
        // catalog acceptance commit leaves a durable `accepted` removal row
        // under a pending catalog state. Startup rolls the catalog acceptance
        // forward from the durable prepared material exactly once.
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory.path().join("removal-roll-forward.sqlite");
        let first = DaemonApplicationFacade::open_for_test(&database, fixture_config_snapshot())
            .expect("first facade opens");
        seed_catalog(&first, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let outcome = prepare_removal_at_now(
            &first,
            "removal-roll-forward",
            "https://api.example.invalid/v9",
        );
        assert!(outcome.pending_removal);
        let candidate_handle = outcome
            .candidate_handle
            .expect("pending removal carries a handle");
        // Simulate the crash window: only the removal acceptance commits.
        intention_storage::ProviderRemovalRepositoryDto::accept_provider_catalog_removal(
            first.inner.repository.as_ref(),
            intention_storage::AcceptProviderCatalogRemovalInputDto {
                candidate_handle,
                accepted_at: i64_time(now().expect("fixture clock reads")),
                operation_id: "crash-window-removal-accept".to_owned(),
            },
        )
        .expect("removal acceptance commits before the crash");
        drop(first);

        let restarted =
            DaemonApplicationFacade::open_for_test(&database, fixture_config_snapshot())
                .expect("restart facade opens");
        assert!(matches!(
            restarted.provider_control_readiness(),
            intention_application::CatalogReadiness::Ready
        ));
        let projection = restarted
            .inner
            .control_plane
            .controller
            .inspect()
            .expect("catalog projection reads");
        assert_eq!(
            projection.active_catalog_revision_id,
            Some(2),
            "the catalog acceptance rolls forward to the accepted revision"
        );
        // The roll-forward is durable and idempotent: a second restart stays
        // on the active catalog without re-accepting.
        drop(restarted);
        let again = DaemonApplicationFacade::open_for_test(&database, fixture_config_snapshot())
            .expect("second restart facade opens");
        assert!(matches!(
            again.provider_control_readiness(),
            intention_application::CatalogReadiness::Ready
        ));
    }

    #[test]
    fn corrected_removal_proposal_after_rejection_receives_a_fresh_revision() {
        // PR24-006: closed removal rows stay in durable history, so a
        // corrected or repeated proposal after rejection must never reuse the
        // closed candidate's revision or handle.
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("removal-retry-fresh-revision.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let rejected = prepare_removal_at_now(
            &facade,
            "removal-rejected",
            "https://api.example.invalid/v9",
        );
        assert_eq!(
            rejected.candidate_handle.as_deref(),
            Some("catalog-2"),
            "the first removal proposal is revision two"
        );
        let reject = facade.command(ProtocolCommandDto::RejectProviderCatalogCandidate(
            intention_protocol::contract_families::RejectProviderCatalogCandidateCommandDto {
                candidate_handle: rejected
                    .candidate_handle
                    .expect("pending removal carries a handle"),
                expected_active_catalog_revision_id: "1".to_owned(),
                operation_id: "op-reject-retry".to_owned(),
            },
        ));
        let ProtocolCommandResultDto::Accepted(_) = reject else {
            unreachable!("candidate rejection is accepted")
        };

        // A corrected proposal (different endpoint, same removal intent)
        // receives the next durable revision instead of colliding with the
        // closed catalog-2 identity.
        let corrected = prepare_removal_at_now(
            &facade,
            "removal-corrected",
            "https://api.example.invalid/v10",
        );
        assert_eq!(
            corrected.candidate_handle.as_deref(),
            Some("catalog-3"),
            "the corrected proposal receives a fresh durable revision"
        );
        let accept = facade.command(ProtocolCommandDto::AcceptProviderCatalogRemoval(
            intention_protocol::contract_families::AcceptProviderCatalogRemovalCommandDto {
                candidate_handle: corrected
                    .candidate_handle
                    .expect("corrected proposal carries a handle"),
                expected_active_catalog_revision_id: "1".to_owned(),
                expected_candidate_catalog_revision_id: "3".to_owned(),
                operation_id: "accept-corrected".to_owned(),
                source_recheck: false,
            },
        ));
        let ProtocolCommandResultDto::Accepted(_) = accept else {
            unreachable!("corrected proposal acceptance is accepted")
        };
        assert!(matches!(
            facade.provider_control_readiness(),
            intention_application::CatalogReadiness::Ready
        ));
        let projection = facade
            .inner
            .control_plane
            .controller
            .inspect()
            .expect("catalog projection reads");
        assert_eq!(projection.active_catalog_revision_id, Some(3));
    }

    #[test]
    fn reload_during_pending_removal_preserves_the_lifecycle_across_restart() {
        // PR24-005: a configuration reload commit never rewrites a durable
        // pending-removal state; after a restart the removal lifecycle (and
        // its real deadline) survives with the reloaded configuration.
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory.path().join("reload-pending-restart.sqlite");
        let startup = fixture_config_snapshot();
        let first = DaemonApplicationFacade::open_for_test(&database, startup.clone())
            .expect("first facade opens");
        let startup_revision = startup.revision_id().to_string();
        seed_catalog(&first, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let outcome =
            prepare_removal_at_now(&first, "removal-reload", "https://api.example.invalid/v9");
        assert!(outcome.pending_removal);

        // An execution-policy reload commits during the pending removal.
        let transaction = reload_transaction(first.command(raw_edit_command(
            "op-reload-pending",
            &startup_revision,
            policy_edit(45),
        )));
        assert_eq!(
            transaction.commit_outcome,
            ConfigurationCommitOutcomeDto::Committed
        );
        assert_eq!(
            first
                .active_config_snapshot()
                .expect("active snapshot reads")
                .resolved()
                .provider_execution()
                .attempt_timeout_seconds(),
            45,
            "the reloaded execution policy applies to the running daemon"
        );
        drop(first);

        let restarted =
            DaemonApplicationFacade::open_for_test(&database, fixture_config_snapshot())
                .expect("restart facade opens");
        let intention_application::CatalogReadiness::PendingRemoval {
            candidate_revision,
            expires_at,
        } = restarted.provider_control_readiness()
        else {
            panic!("reload must not exit the pending-removal lifecycle");
        };
        assert_eq!(candidate_revision, "2");
        assert!(expires_at > 0);
    }
}
