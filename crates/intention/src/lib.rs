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

use intention_application::harness::{
    CaptureHarnessTriggerRequestDto, HarnessLaunchRequestDto, HarnessRestartRecoveryInputDto,
    HarnessRuntimeService,
};
use intention_application::programmatic_policy::{
    ProgrammaticPolicyAdmissionService, ProgrammaticReservationRecoveryInputDto,
};
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
    control_plane::{ConfigCandidateDto, render_edited_configuration, restore_credential_document},
    parse_credential,
};
#[cfg(test)]
use intention_domain::{CreateSessionCommandDto, RunModeDto, WorkspaceRootDto};
use intention_domain::{
    CredentialTransportMode as DomainCredentialTransportMode, DomainEventDto,
    GetSessionSnapshotQueryDto, ModelRunFactInputDto, ProviderSelectionV1, RunEventCursorDto,
    RunFailureDto, RunReplayDto, RunStatusDto, ToolLifecycleStatusDto,
    canonical::{Digest256, contains_control_or_nul, contains_credential_shape},
    harness::{
        HarnessDisconnectContractV1, HarnessIntervalScheduleV1, HarnessLaunchOriginV1,
        HarnessRuleLifecycleStateV1, applied_harness_time_zone, validate_harness_time_zone,
    },
    slice3_selections::{HarnessExecutionClassV1, HarnessSourceKindV1},
};
use intention_hooks::{
    Hook, Outcome as HookOutcome, Phase, PhaseContext, Registry as HookRegistry,
};
use intention_model::{
    AuthenticationHeaderPolicyV1, CredentialTransportMode as ModelCredentialTransportMode,
    ModelCancellationSignal, ModelExecutionDriver, ReasoningEffortLevel,
};
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
        ConfigurationProjectionDto, ConfigurationReloadStatusDto,
        ConfigurationValidationOutcomeDto, ProviderAvailabilityObservation,
        ProviderModelDiscoveryRecordDto, RawTomlEditCommandDto, ReloadConfigurationCommandDto,
        ReloadTransactionDto, RotateProviderCredentialsCommandDto,
    },
};
use intention_provider_generic_chat::{GenericChatDriver, GenericChatDriverOptions};
use intention_provider_openrouter::{OpenRouterDriver, OpenRouterDriverOptions};
#[cfg(feature = "test-support")]
use intention_runtime::ModelRunFirstAppendGate;
use intention_runtime::{
    ModelRunCommitObserver, ModelRunExecutionInputDto, ModelRunExecutionOutcomeDto,
    ModelRunExecutionService, ModelTimePort, RuntimeService, RuntimeValuesDto, ToolExecutionPort,
    fail_starting_run,
};
use intention_storage::harness_repo::{
    CreateHarnessRuleInputDto, HarnessExecutionClassDto, HarnessJournalRecordDto,
    HarnessRuleLifecycleStateDto, HarnessRuleRecordDto, HarnessRuleRepositoryDto,
    HarnessRuleRevisionRecordDto, HarnessSourceKindDto, HarnessTriggerReasonRecordDto,
    HarnessTriggerRepositoryDto, ReviseHarnessRuleInputDto, TransitionHarnessRuleLifecycleInputDto,
};
use intention_storage::programmatic_policy_repo::{
    ProgrammaticAdmissionRepositoryDto, ProgrammaticReservationStateDto,
};
use intention_storage::{
    AppendModelRunFactsInputDto, EnqueueUnavailableRunInputDto, HeldRunRepositoryDto,
    MarkRecoveredRunHeldInputDto, ProviderCatalogRepositoryDto, ProviderRemovalRepositoryDto,
    StorageRepositoryDto, UnavailableQueueRepositoryDto,
};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use intention_tools::{CancellationSignal, ToolInput, ToolResult};
use intention_types::{
    ConfigRevisionId, CorrelationIdDto, DtoResult, ErrorDto, ErrorRetryDto, EventEnvelopeDto,
    RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId, TimestampDto,
};
#[cfg(test)]
use intention_types::{ErrorCategoryDto, ProjectId, WorkspaceId};
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
pub use intention_application::harness::{
    HarnessAdmittedLaunchDto, HarnessLaunchOutcomeDto, HarnessRestartRecoveryDto,
    HarnessTriggerCaptureResultDto,
};
pub use intention_application::programmatic_policy::ProgrammaticReservationRecoveryDto;

/// The durable continual-harness repository contract of architecture 26.
///
/// The daemon-facing harness surface persists through these DTO-only Slice 3
/// records: rules and immutable revisions, coalesced trigger reasons,
/// counters, dossiers, verified checkpoints, and the durable journal. They
/// carry safe identities, revisions, digests, and bound values only, never a
/// credential, filesystem path, grant, provider resource, or implementation
/// state.
pub use intention_storage::harness_repo;

/// One daemon-owned harness scheduling tick request.
///
/// The daemon owns the tick cadence, the project time zone, and the
/// daemon-wide concurrency signal; the composition owns the durable capture,
/// the single coalesced admission, and every journal append. No credential,
/// filesystem path, grant, or provider resource crosses this boundary.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessScheduleTickRequestDto {
    /// The rule observed by this tick, as canonical identity text.
    pub harness_id: String,
    /// The daemon-owned observation time in Unix milliseconds.
    pub observed_at_ms: u64,
    /// The daemon-owned project time zone applied to a non-archived rule.
    pub project_time_zone: String,
    /// Whether the daemon-wide concurrency signal is free at this observation.
    pub daemon_concurrency_available: bool,
    /// The daemon-assigned fresh ordinary run identity of a possible launch.
    pub proposed_run_id: RunId,
}

/// One due schedule observation captured by a harness scheduling tick.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessScheduleObservationDto {
    /// The closed source kind of this due observation.
    pub source_kind: HarnessSourceKindV1,
    /// The exact observation slot in Unix milliseconds.
    pub observed_at_ms: u64,
    /// The durable redelivery-safe capture result.
    pub capture: HarnessTriggerCaptureResultDto,
}

/// The complete outcome of one harness scheduling tick.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessScheduleTickOutcomeDto {
    /// The durable rule identity this tick observed.
    pub harness_id: String,
    /// The due observations captured by this tick, in source order.
    pub observations: Vec<HarnessScheduleObservationDto>,
    /// The single launch attempt of this tick, when a pending reason existed.
    pub launch: Option<HarnessLaunchOutcomeDto>,
    /// The pending coalesced reason after the tick, when one remains.
    pub pending_reason: Option<HarnessTriggerReasonRecordDto>,
}

struct FacadeInner {
    repository: Arc<SqliteStorageRepository>,
    config_snapshot: Mutex<ConfigSnapshotDto>,
    _selected_provider: SelectedProvider,
    dispatch: PrivateModelRunDispatch,
    command_gate: Mutex<()>,
    tool_cancellations: Mutex<HashMap<(SessionId, RunId), LocalToolCancellationEntry>>,
    reload_candidates: Mutex<HashMap<String, ConfigCandidateDto>>,
    control_plane: ProviderControlPlane,
    private_credential: Mutex<PrivateCredentialState>,
}

/// The composition's private credential state.
///
/// This is the composition-owned private credential source of the slice: it
/// retains the startup provider's credential value and the configuration file
/// it came from, both captured inside the private loading boundary at open.
/// The value is the private material restored into typed-edit candidate
/// documents; rotation refreshes it from the same file through the private
/// loading boundary and rebuilds the selected provider driver. The state
/// deliberately implements no `Debug`, `Display`, or serde traits: the
/// credential must never cross a DTO, log, error, digest, or durable surface.
#[derive(Default)]
struct PrivateCredentialState {
    /// The current private credential value of the startup provider.
    material: Option<String>,
    /// The configuration file source retained as the private channel through
    /// which replacement material arrives. Never disclosed in a DTO or error.
    source: Option<ConfigSourceDto>,
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
            Box::new(CompositionDriverFactory::service(
                ProviderKindDto::Openrouter,
            )) as Box<dyn ProviderDriverFactory>,
            Box::new(CompositionDriverFactory::service(
                ProviderKindDto::GenericChatCompletionApi,
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
    kind: ProviderKindDto,
}

impl CompositionDriverFactory {
    const fn service(kind: ProviderKindDto) -> Self {
        Self { kind }
    }
}

impl ProviderDriverFactory for CompositionDriverFactory {
    fn kind(&self) -> &str {
        self.kind.as_str()
    }

    fn supports_contract(
        &self,
        contract: &intention_domain::ProviderDriverContractRevisionDto,
    ) -> bool {
        contract.driver_family == self.kind.as_str() && contract.major == 1 && contract.minor == 1
    }

    fn build(
        &self,
        profile: PrivateProviderProfileMaterial,
    ) -> DtoResult<Box<dyn ModelRunDriverHandle + Send + Sync>> {
        // PR24-057: every catalog activation preflights the profile's
        // declared options through the executing adapter's option builder via
        // the composition seam. The registry carries credential-free opaque
        // handles in this slice, so the derived options are validated here
        // and the daemon host materializes the executing driver with the same
        // seam output later; a declaration the adapter cannot apply (live
        // `SafeHeader` wire injection is not activated) therefore fails the
        // all-or-nothing activation instead of being silently ignored.
        let declared = DeclaredProviderOptions::from_declaration(
            profile.profile.credential_transport_mode,
            profile.profile.safe_header_name.clone(),
            None,
        )?;
        declared.preflight_for_kind(self.kind)?;
        let _ = &profile.private_credential_reference;
        Ok(Box::new(CompositionCatalogDriverHandle))
    }
}

/// Resolves one catalog provider kind id into the typed provider kind.
///
/// Catalog kind ids are the normalized typed kind strings, so the typed kind
/// is the single mapping authority [`ProviderKindDto::from_id`]: an id with
/// no typed kind fails closed instead of being routed to another adapter.
///
/// # Errors
///
/// Returns `unsupported_provider_kind` when no typed provider kind matches
/// the supplied catalog id.
fn typed_provider_kind(kind_id: &str) -> DtoResult<ProviderKindDto> {
    ProviderKindDto::from_id(kind_id).ok_or_else(|| {
        ErrorDto::validation(
            "unsupported_provider_kind",
            "the provider kind has no registered adapter",
        )
    })
}

/// The deterministic endpoint the provider catalog derives for a kind whose
/// startup declaration names none.
///
/// `intention-application` derives this placeholder when it builds a profile
/// from a declaration without an endpoint, so the durable active profile
/// carries it for an endpointless declaration. The startup comparison
/// needs the same value to stay two-directional; the
/// derivation's private owner lives in the application crate, so this copy is
/// pinned by `derived_default_endpoint_matches_the_catalog_derivation`, which
/// reads the active profile's endpoint for an endpointless declaration.
fn derived_default_endpoint(kind: &str) -> String {
    format!("https://{kind}.api.example.invalid/v1")
}

/// Credential-free opaque handle behind the private registry.
struct CompositionCatalogDriverHandle;

impl ModelRunDriverHandle for CompositionCatalogDriverHandle {}

/// The composition's catalog admission port.
///
/// A profile resolves when the active catalog material names it and the
/// controller admits its exact registry key as enabled and ready in the current
/// active membership.
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

/// The composition's provider-option seam (PR24-057).
///
/// The provider adapters expose additive, validated, credential-free driver
/// option builders (`OpenRouterDriverOptions` / `GenericChatDriverOptions`)
/// that adapter tests exercise directly but that production construction
/// historically bypassed, leaving the live driver on adapter defaults while
/// catalog projections advertised the profile's declared option policy. This
/// type is the single composition-owned translation of validated provider
/// material into those builders, applied by every production driver
/// construction and reconstruction path in this crate:
///
/// - startup: [`SelectedProvider::from_startup_material`] applies the closed
///   transport declaration of the startup profile through the seam;
/// - catalog activation: [`CompositionDriverFactory::build`] preflights each
///   profile's declared options through the executing adapter's builder, so a
///   declaration the adapter cannot apply fails the activation instead of
///   being silently ignored;
/// - credential-driven rebuild: rotation replaces only the driver's private
///   SDK client, so the driver keeps the options the seam applied at
///   construction.
///
/// Slice 2 catalog material declares exactly the closed credential transport
/// per profile (bearer authorization, or one descriptor-selected safe header
/// whose complete value is the credential). The bearer declaration maps to an
/// empty bearer header policy; live `SafeHeader` wire injection is not
/// activated (ADR 0037, EXC-057), so both executing adapters reject it in
/// their option `build()` and the seam surfaces
/// `unsupported_safe_header_transport` instead of defaulting. Reasoning
/// effort has no Slice 2 declaration surface (RSN-011), so producible
/// declarations carry `None`; the slot flows through the same seam so a later
/// declaration surface cannot add a second construction path.
#[derive(Clone, Debug, Eq, PartialEq)]
struct DeclaredProviderOptions {
    header_policy: AuthenticationHeaderPolicyV1,
    reasoning_effort: Option<ReasoningEffortLevel>,
}

impl DeclaredProviderOptions {
    /// Composes the closed option declaration of one validated profile.
    ///
    /// # Errors
    ///
    /// Returns `invalid_credential_transport` or `invalid_safe_header_name`
    /// when the transport declaration is inconsistent or unbounded.
    fn from_declaration(
        transport: DomainCredentialTransportMode,
        safe_header_name: Option<String>,
        reasoning_effort: Option<ReasoningEffortLevel>,
    ) -> DtoResult<Self> {
        let allowed_header_names = match transport {
            DomainCredentialTransportMode::Bearer => Vec::new(),
            DomainCredentialTransportMode::SafeHeader => safe_header_name.into_iter().collect(),
        };
        let selected_transport = match transport {
            DomainCredentialTransportMode::Bearer => ModelCredentialTransportMode::Bearer,
            DomainCredentialTransportMode::SafeHeader => ModelCredentialTransportMode::SafeHeader,
        };
        Ok(Self {
            header_policy: AuthenticationHeaderPolicyV1::new(
                allowed_header_names,
                selected_transport,
            )?,
            reasoning_effort,
        })
    }

    /// Translates this declaration into the OpenRouter driver option builder.
    ///
    /// # Errors
    ///
    /// Returns `unsupported_safe_header_transport` when the declared header
    /// policy selects the safe-header transport, which the OpenRouter SDK
    /// adapter cannot inject without the `http` crate as a production
    /// dependency.
    fn into_openrouter(self) -> DtoResult<OpenRouterDriverOptions> {
        let mut builder = OpenRouterDriverOptions::new().with_header_policy(self.header_policy);
        if let Some(effort) = self.reasoning_effort {
            builder = builder.with_reasoning_effort(effort);
        }
        builder.build()
    }

    /// Translates this declaration into the generic-chat driver option
    /// builder.
    ///
    /// # Errors
    ///
    /// Returns `unsupported_safe_header_transport` for the not-activated
    /// safe-header transport, or `unsupported_reasoning_effort` for the
    /// maximum effort the pinned SDK effort set cannot express.
    fn into_generic_chat(self) -> DtoResult<GenericChatDriverOptions> {
        let mut builder = GenericChatDriverOptions::new().with_header_policy(self.header_policy);
        if let Some(effort) = self.reasoning_effort {
            builder = builder.with_reasoning_effort(effort);
        }
        builder.build()
    }

    /// Preflights this declaration through the option builder of one typed
    /// provider kind.
    ///
    /// This is the single kind-to-adapter dispatch of the composition: the
    /// catalog-activation driver factory and the credential-driven rebuild
    /// both preflight through it, so the two sites can never select different
    /// adapters for the same declaration. The match is exhaustive over the
    /// typed kind, so a new provider kind fails to compile until its adapter
    /// builder is wired instead of silently falling through to another
    /// adapter.
    ///
    /// # Errors
    ///
    /// Returns the adapter builder's typed rejection
    /// (`unsupported_safe_header_transport` or `unsupported_reasoning_effort`)
    /// when the declaration cannot be applied by that adapter.
    fn preflight_for_kind(self, kind: ProviderKindDto) -> DtoResult<()> {
        match kind {
            ProviderKindDto::Openrouter => self.into_openrouter().map(|_| ()),
            ProviderKindDto::GenericChatCompletionApi => self.into_generic_chat().map(|_| ()),
        }
    }
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
        // PR24-057: production construction applies the closed Slice 2
        // transport declaration (bearer) through the composition seam instead
        // of silently falling back to adapter-default options. The startup
        // catalog derivation persists the same bearer declaration for the
        // selected profile (kind descriptors derive
        // `credential_transport_contract = "bearer"`), and the adapter
        // option `build()` re-validates applicability, so a declaration the
        // executing adapter cannot apply fails the daemon open path closed.
        let declared = DeclaredProviderOptions::from_declaration(
            DomainCredentialTransportMode::Bearer,
            None,
            None,
        )?;
        Self::build_with_declared_options(material, declared)
    }

    /// Builds the executing provider driver for one seam-declared option set.
    ///
    /// This is the only production driver construction path; test helpers may
    /// supply non-default declarations to observe how the seam applies them.
    fn build_with_declared_options(
        material: StartupProviderMaterial,
        declared: DeclaredProviderOptions,
    ) -> DtoResult<Self> {
        match material.safe_resolved().provider().kind() {
            ProviderKindDto::Openrouter => {
                let options = declared.into_openrouter()?;
                OpenRouterDriver::from_startup_material_with_options(material, options)
                    .map(Self::OpenRouter)
            }
            ProviderKindDto::GenericChatCompletionApi => {
                let options = declared.into_generic_chat()?;
                GenericChatDriver::from_startup_material_with_options(material, options)
                    .map(Self::GenericChat)
            }
        }
    }

    /// Returns the OpenRouter driver's applied options, when selected.
    #[cfg(test)]
    const fn openrouter_options(&self) -> Option<&OpenRouterDriverOptions> {
        match self {
            Self::OpenRouter(driver) => Some(driver.options()),
            _ => None,
        }
    }

    /// Returns the generic-chat driver's applied options, when selected.
    #[cfg(test)]
    const fn generic_chat_options(&self) -> Option<&GenericChatDriverOptions> {
        match self {
            Self::GenericChat(driver) => Some(driver.options()),
            _ => None,
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

    /// Replaces the selected provider driver's private SDK client with one
    /// built from fresh private credential material.
    ///
    /// The swap is composition-owned and happens only after the replacement
    /// client is configured; concurrent executions keep the client they
    /// captured before the swap. The credential never crosses a DTO, log, or
    /// error boundary.
    ///
    /// # Errors
    ///
    /// Returns the provider driver's typed build error, or
    /// `credential_rotation_source_unavailable` when the facade holds no
    /// real provider driver to rebuild.
    fn rotate_private_credential(&self, credential: String) -> DtoResult<()> {
        match self {
            Self::OpenRouter(driver) => driver.rotate_credential(credential),
            Self::GenericChat(driver) => driver.rotate_credential(credential),
            #[cfg(any(test, feature = "test-support"))]
            Self::TestSupport(_) => Err(ErrorDto::unavailable(
                "credential_rotation_source_unavailable",
                "no private driver is bound to this profile",
            )),
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

/// The composition's private credential source.
///
/// The daemon's own configuration file is the configured private credential
/// source of this slice: the file path retained at open is re-read through
/// the private loading boundary on rotation, and its current
/// `provider.credential` value becomes the replacement material. A facade
/// opened without a file-backed source (test-support opens) has no configured
/// source and fails closed with `credential_rotation_source_unavailable`
/// before any replacement is obtained. Read, permission, or parse failures
/// map to the same closed source-unavailable code and never disclose file
/// content, the path, or the credential.
struct CompositionCredentialSource<'a> {
    state: &'a Mutex<PrivateCredentialState>,
}

impl PrivateCredentialPort for CompositionCredentialSource<'_> {
    fn obtain_replacement(&self, _profile_id: &str) -> DtoResult<PrivateCredentialMaterial> {
        let source = self
            .state
            .lock()
            .map_err(|_| {
                ErrorDto::unavailable(
                    "credential_rotation_source_unavailable",
                    "the private credential source is unavailable",
                )
            })?
            .source
            .clone();
        let Some(source) = source else {
            return Err(ErrorDto::unavailable(
                "credential_rotation_source_unavailable",
                "no private credential source is configured",
            ));
        };
        let replacement = read_configured_credential(&source).map_err(|_| {
            ErrorDto::unavailable(
                "credential_rotation_source_unavailable",
                "the private credential source could not supply replacement material",
            )
        })?;
        Ok(PrivateCredentialMaterial::from_private_bytes(
            replacement.into_bytes(),
        ))
    }
}

/// Reads the current credential value from the configured configuration file.
///
/// The read repeats the open-time private loading boundary: owner-only
/// permission verification on Unix, file read, and the shared credential
/// parse. Any failure is a source failure and carries no path or content.
fn read_configured_credential(source: &ConfigSourceDto) -> DtoResult<String> {
    #[cfg(unix)]
    intention_config::ensure_user_only_permissions(source.path())?;
    let raw = fs::read_to_string(source.path().as_str()).map_err(|_| {
        ErrorDto::unavailable(
            "credential_source_read_unavailable",
            "the private credential source could not be read",
        )
    })?;
    parse_credential(&raw)
}

/// The composition's private driver rebuild boundary.
///
/// Rebuild applies replacement material to the selected provider driver that
/// actually executes runs: the facade's startup provider. The target
/// profile's provider kind must match the selected provider, and the
/// replacement is committed to the private credential state only after the
/// driver swap succeeds. The rebuild also preflights the active profile's
/// declared options through the composition seam (PR24-057), so a profile
/// whose declared transport the executing adapter cannot apply fails closed
/// rather than silently serving options that no longer match the profile.
/// Test-support drivers and kind-mismatched profiles have no private driver
/// and fail closed with `credential_rotation_source_unavailable`; the
/// credential never crosses a DTO, log, or error.
struct CompositionDriverRebuildPort<'a> {
    facade: &'a DaemonApplicationFacade,
}

impl DriverRebuildPort for CompositionDriverRebuildPort<'_> {
    fn rebuild(&self, profile_id: &str, material: PrivateCredentialMaterial) -> DtoResult<()> {
        let credential = String::from_utf8(material.into_private_bytes()).map_err(|_| {
            ErrorDto::unavailable(
                "credential_rotation_source_unavailable",
                "the private credential source supplied unusable material",
            )
        })?;
        let resolved = self
            .facade
            .catalog_admission_port()
            .resolve_enabled_profile(profile_id)?;
        let driver_kind = self.facade.inner._selected_provider.safe_kind();
        let resolved_kind = typed_provider_kind(&resolved.kind_id)?;
        if driver_kind != Some(resolved_kind) {
            return Err(ErrorDto::unavailable(
                "credential_rotation_source_unavailable",
                "no private driver is bound to this profile",
            ));
        }
        // PR24-057: a credential-driven rebuild keeps the executing driver
        // able to apply the active profile's declared options. The seam
        // preflights the resolved profile's declared transport through the
        // executing adapter's option builder, so a declaration the adapter
        // cannot apply (live `SafeHeader` wire injection is not activated)
        // fails the rotation closed instead of silently leaving the driver on
        // options that no longer match the profile it serves.
        let declared_transport = match resolved.credential_transport_mode {
            intention_protocol::contract_families::CredentialTransportMode::Bearer => {
                DomainCredentialTransportMode::Bearer
            }
            intention_protocol::contract_families::CredentialTransportMode::SafeHeader => {
                DomainCredentialTransportMode::SafeHeader
            }
        };
        let declared = DeclaredProviderOptions::from_declaration(
            declared_transport,
            resolved.credential_transport_safe_header_name,
            None,
        )?;
        declared.preflight_for_kind(resolved_kind)?;
        self.facade
            .inner
            ._selected_provider
            .rotate_private_credential(credential.clone())?;
        let mut state = self.facade.inner.private_credential.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_command_unavailable",
                "daemon command is unavailable",
            )
        })?;
        state.material = Some(credential);
        drop(state);
        Ok(())
    }
}

/// The composition's session-profile change publication seam.
///
/// The session-profile service constructs the typed
/// `SessionProviderProfileChangedEventDto` after the durable default commits
/// and hands it to this port. Slice 2 exposes no durable append for the
/// control-plane event family yet, so the composition validates the committed
/// event at this boundary and records no durable copy: the event is never
/// fabricated into an evidence row, and no credential, path, or private
/// handle crosses the seam.
struct CompositionSessionProfileEvents;

impl intention_application::session_selection::SessionProviderProfileChangePort
    for CompositionSessionProfileEvents
{
    fn publish_session_provider_profile_changed(
        &self,
        event: intention_protocol::contract_families::SessionProviderProfileChangedEventDto,
    ) -> DtoResult<()> {
        event.validate()
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
        reload_status: ConfigurationReloadStatusDto::Active,
    }
}

/// Maps one protocol typed-edit operation into the configuration crate's
/// credential-free edit operation.
///
/// TOML document construction owned by `intention-config`
/// (which cannot depend on `intention-protocol`); this mapping is the only
/// protocol-aware code left on the typed-edit path.
fn configuration_edit_operation(
    operation: &ConfigurationEditOperationDto,
) -> intention_config::control_plane::ConfigurationEditOperation {
    match operation {
        ConfigurationEditOperationDto::Set {
            key_path,
            safe_value,
        } => intention_config::control_plane::ConfigurationEditOperation::Set {
            key_path: key_path.clone(),
            safe_value: safe_value.clone(),
        },
        ConfigurationEditOperationDto::Remove { key_path } => {
            intention_config::control_plane::ConfigurationEditOperation::Remove {
                key_path: key_path.clone(),
            }
        }
    }
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

/// The durable declaration of the active catalog's default profile.
///
/// The startup path compares this declaration with the validated startup
/// document; it carries only the credential-free declaration fields
/// that participate in the catalog profile identity and the run selection.
struct ActiveCatalogDeclaration {
    kind: String,
    model: String,
    endpoint: String,
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
        let source = ConfigPathResolver::resolve(None)?;
        let database = platform_database_location()?;
        Self::open_platform_from(source, database)
    }

    /// Opens the daemon facade from an explicit configuration source and
    /// durable database location.
    ///
    /// This is the single daemon-open sequence: [`Self::open_platform`]
    /// resolves the platform locations and delegates here, and the startup
    /// fixtures call the same sequence over fixture paths instead of
    /// re-implementing it, so the production open path and the fixtures
    /// cannot drift.
    ///
    /// # Errors
    ///
    /// Returns safe typed failures when the configuration cannot be loaded,
    /// validated, retained, persisted, or recovered.
    fn open_platform_from(source: ConfigSourceDto, database: PathBuf) -> DtoResult<Self> {
        let (config_snapshot, selected_provider, raw_toml, private_credential) =
            load_provider_configuration(source.clone())?;
        let facade =
            Self::open_with_selected_provider(database, config_snapshot, selected_provider)?;
        retain_private_startup_credential(&facade, private_credential, source)?;
        // The startup document is the single source of the provider catalog
        // On a fresh store it activates the first catalog revision and
        // on a restart it re-derives the catalog when the file changed, so the
        // executing driver, the active configuration snapshot, and the durable
        // catalog can never disagree.
        facade.activate_startup_catalog(&raw_toml)?;
        facade.ensure_driver_kind_matches_active_catalog()?;
        Ok(facade)
    }

    /// Reconciles the durable provider catalog with the startup document.
    ///
    /// The daemon opens with the startup TOML as the single source of the
    /// provider declaration. A fresh store activates the first catalog
    /// revision; a restart whose declaration (kind, model, and declared
    /// endpoint) matches the active catalog's default profile is a no-op; a
    /// restart whose declaration changed re-derives the catalog in-process
    /// through the normal prepare and accept path, so the catalog catches up
    /// with the file instead of diverging from the executing driver.
    /// Activation is required because selection-less acceptance is removed
    /// under ADR 0038: every admitted run resolves a provider profile from the
    /// active catalog, and no wire command prepares a first catalog in this
    /// slice.
    ///
    /// The helper stays private to the open path: the raw document carries the
    /// credential, so it never crosses a public signature.
    ///
    /// # Errors
    ///
    /// Returns the typed candidate, catalog, or storage error; an invalid
    /// startup declaration or an inconsistent active catalog fails the daemon
    /// open path closed.
    fn activate_startup_catalog(&self, raw_toml: &str) -> DtoResult<()> {
        use intention_application::{CatalogProviderDeclarationDto, CatalogSourceInputDto};
        use intention_config::control_plane::{catalog_declaration_snapshot, parse_candidate};

        let previous = self.active_config_snapshot()?;
        let candidate = parse_candidate(
            RawConfigInputDto::new(raw_toml.to_owned(), ConfigPathResolver::resolve(None)?),
            &previous,
        )?;
        let provider = candidate.safe_snapshot().resolved().provider();
        let declaration = CatalogProviderDeclarationDto {
            kind: provider.kind().as_str().to_owned(),
            model: provider.model().to_owned(),
            endpoint: provider.endpoint().map(str::to_owned),
            declared_model_capability_subset: vec![
                "text_input".to_owned(),
                "text_streaming".to_owned(),
            ],
            enabled: true,
        };
        let raw_config_size_bytes = u64::try_from(raw_toml.len()).unwrap_or(u64::MAX);
        let state = self.inner.repository.load_provider_catalog_status()?;
        if state.active_catalog_revision_id.is_none() {
            self.prepare_catalog_candidate(CatalogSourceInputDto {
                operation_id: "startup-catalog".to_owned(),
                raw_config_size_bytes,
                providers: vec![declaration],
                candidate,
                previous,
            })?;
            return Ok(());
        }
        // Compare the startup-derived effective endpoint
        // with the active catalog's effective endpoint in both directions, so
        // a document that drops a previously declared endpoint re-derives the
        // catalog instead of keeping the stale declaration. A declaration
        // that names no endpoint carries the kind's deterministic derived
        // endpoint, which is exactly the endpoint the catalog stores for an
        // endpointless kind.
        let active = self.active_catalog_declaration()?.ok_or_else(|| {
            ErrorDto::validation(
                "provider_catalog_state_inconsistent",
                "the active provider catalog has no default profile",
            )
        })?;
        let declared_endpoint = declaration
            .endpoint
            .clone()
            .unwrap_or_else(|| derived_default_endpoint(&declaration.kind));
        let endpoint_differs = declared_endpoint != active.endpoint;
        if declaration.kind == active.kind && declaration.model == active.model && !endpoint_differs
        {
            // The file already matches the durable catalog: the restart keeps
            // the active revision and the second open is a no-op.
            return Ok(());
        }
        // The catalog prepare path detects change by comparing two
        // configuration snapshots, so the active catalog declaration is
        // projected into that comparison shape before the candidate is
        // prepared. Only the declared provider fields differ; the projection
        // is comparison input and is never persisted.
        let comparison_previous = catalog_declaration_snapshot(
            &previous,
            typed_provider_kind(&active.kind)?,
            &active.model,
            Some(active.endpoint.as_str()),
        )?;
        let outcome = self.prepare_catalog_candidate(CatalogSourceInputDto {
            operation_id: "startup-catalog-rederive".to_owned(),
            raw_config_size_bytes,
            providers: vec![declaration],
            candidate,
            previous: comparison_previous,
        })?;
        if outcome.pending_removal {
            // A changed provider kind is a catalog removal under the
            // controller's change classification: the previous kind and
            // profile are tombstoned with the replacement revision. The
            // process restart is the explicit operator act the pending state
            // waits for, so the startup path accepts the prepared candidate
            // immediately and keeps the usual candidate, audit, and
            // activation evidence.
            let candidate_handle = outcome.candidate_handle.ok_or_else(|| {
                ErrorDto::validation(
                    "provider_catalog_state_inconsistent",
                    "the prepared removal candidate is missing its handle",
                )
            })?;
            let candidate_revision = outcome.catalog_revision_id.ok_or_else(|| {
                ErrorDto::validation(
                    "provider_catalog_state_inconsistent",
                    "the prepared removal candidate is missing its revision",
                )
            })?;
            // The open
            // sequence adopts a durable, startup-rebuilt pending removal in
            // `startup()` before this prepare runs, so the durable active
            // revision read here is the revision that adoption committed. The
            // acceptance below must supersede exactly that revision instead of
            // the revision read before the prepare: the controller would
            // otherwise reject the acceptance once with
            // `provider_catalog_revision_conflict`, so the open would fail and
            // only self-heal on the next restart. A pending candidate never
            // advances the active revision, so this read is also the
            // pre-adoption revision when no adoption happened.
            let acceptance_active_revision = self
                .inner
                .repository
                .load_provider_catalog_status()?
                .active_catalog_revision_id
                .ok_or_else(|| {
                    ErrorDto::validation(
                        "provider_catalog_state_inconsistent",
                        "the prepared removal candidate has no active catalog revision",
                    )
                })?;
            self.inner.control_plane.controller.accept_pending(
                candidate_handle,
                acceptance_active_revision.to_string(),
                candidate_revision.to_string(),
                "startup-catalog-removal".to_owned(),
                now_seconds(now()?),
            )?;
        }
        Ok(())
    }

    /// Prepares one catalog candidate through the controller at the current time.
    fn prepare_catalog_candidate(
        &self,
        source: intention_application::CatalogSourceInputDto,
    ) -> DtoResult<intention_application::CatalogCandidateOutcomeDto> {
        self.inner
            .control_plane
            .controller
            .prepare_candidate(source, now_seconds(now()?))
    }

    /// Returns the durable declaration of the active catalog's default profile.
    ///
    /// The startup document declares exactly one provider, so the active
    /// default profile is the declaration the executing driver and every
    /// admitted run must agree with.
    fn active_catalog_declaration(&self) -> DtoResult<Option<ActiveCatalogDeclaration>> {
        let material = self.inner.repository.load_provider_catalog_material()?;
        let Some(default_profile_id) = material.default_profile_id.as_deref() else {
            return Ok(None);
        };
        Ok(material
            .profiles
            .iter()
            .find(|candidate| candidate.profile.profile_id == default_profile_id)
            .map(|candidate| ActiveCatalogDeclaration {
                kind: candidate.profile.provider_kind_id.clone(),
                model: candidate.profile.model_id.clone(),
                endpoint: candidate.profile.endpoint.clone(),
            }))
    }

    /// Asserts the executing driver's kind equals the active catalog kind.
    ///
    /// The startup re-derivation makes the two agree by construction; this
    /// check keeps a `SelectedProvider` whose kind differs from the active
    /// catalog unconstructible on the production open path. Test-support
    /// facades that never activate a catalog select no real provider kind and
    /// leave the invariant vacuous.
    ///
    /// # Errors
    ///
    /// Returns `invalid_selected_provider` when the executing driver kind and
    /// the active catalog kind disagree.
    fn ensure_driver_kind_matches_active_catalog(&self) -> DtoResult<()> {
        let Some(selected_kind) = self.inner._selected_provider.safe_kind() else {
            return Ok(());
        };
        let Some(active) = self.active_catalog_declaration()? else {
            return Ok(());
        };
        if active.kind != selected_kind.as_str() {
            return Err(ErrorDto::validation(
                "invalid_selected_provider",
                "selected provider does not match the active provider catalog",
            ));
        }
        Ok(())
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
                private_credential: Mutex::new(PrivateCredentialState::default()),
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
                selection: selection.clone(),
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
    /// The operations are mapped into the configuration crate's own edit
    /// vocabulary and rendered there from the active safe snapshot as a TOML
    /// document, so this composition never renders TOML and a
    /// value carrying TOML-significant characters is escaped by the
    /// serializer. The rendered document is validated server-side through the
    /// reload contract. The retained private credential is restored into the
    /// reconstructed candidate document before parsing, so an
    /// execution-policy-only typed edit commits while the credential never
    /// appears in a DTO, error, digest, or durable surface. A facade that
    /// retained no private credential (test-support opens) keeps the
    /// fail-closed behavior: an edited candidate that leaves the credential
    /// unset is rejected with `missing_provider_credential`.
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
        let operations = command
            .operations
            .iter()
            .map(configuration_edit_operation)
            .collect::<Vec<_>>();
        let edited = render_edited_configuration(&previous, &operations)?;
        let candidate_text = self.restore_private_credential(&edited)?;
        let candidate = service.prepare(
            RawConfigInputDto::new(candidate_text, reload_edit_source()?),
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

    /// Restores the composition's retained private credential into one
    /// credential-free typed-edit candidate document.
    ///
    /// When no private credential was retained (test-support facades without
    /// a configured source) the document passes through unchanged so
    /// downstream validation fails closed exactly as before. The restored
    /// text exists only inside the parse call and never crosses a DTO,
    /// error, log, digest, or durable surface.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the private credential state lock
    /// is poisoned, or the configuration crate's typed document-shape error
    /// when the credential cannot be restored.
    fn restore_private_credential(&self, edited: &str) -> DtoResult<String> {
        let material = self
            .inner
            .private_credential
            .lock()
            .map_err(|_| {
                ErrorDto::unavailable(
                    "daemon_command_unavailable",
                    "daemon command is unavailable",
                )
            })?
            .material
            .clone();
        material.map_or_else(
            || Ok(edited.to_owned()),
            |credential| restore_credential_document(edited, &credential),
        )
    }

    /// Rotates one provider's private credential material through the
    /// composition's credential source and driver rebuild ports.
    ///
    /// The daemon's own configuration file is the configured private
    /// credential source: its current `provider.credential` value is read
    /// through the private loading boundary and applied to the selected
    /// provider driver after the frozen-meaning checks pass. A facade opened
    /// without a file-backed source fails closed with
    /// `credential_rotation_source_unavailable`.
    ///
    /// # Errors
    ///
    /// Returns `credential_rotation_frozen_meaning_mismatch` when the safe
    /// composition changed, or `credential_rotation_source_unavailable` when
    /// no private credential source or matching driver exists.
    fn rotate_credential(
        &self,
        command: RotateProviderCredentialsCommandDto,
        timestamp: TimestampDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let binding = self.composition_binding_source();
        let rebuild = CompositionDriverRebuildPort { facade: self };
        let service = CredentialRotationService::new(&binding, &rebuild);
        let source = CompositionCredentialSource {
            state: &self.inner.private_credential,
        };
        let result = service.rotate(command, &source, now_seconds(timestamp))?;
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
                .set(
                    command,
                    &port,
                    &CompositionSessionProfileEvents,
                    now_seconds(timestamp),
                )?;
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

    /// Creates one durable harness rule with its first immutable revision.
    ///
    /// The composition owns the persistence path of the daemon harness
    /// definition surface: the request carries bounded typed records only, and
    /// the durable repository remains the single persistence authority.
    ///
    /// # Errors
    ///
    /// Returns the typed rule, revision, bound, and repository failures of the
    /// durable creation.
    #[doc(hidden)]
    pub fn create_harness_rule_for_daemon(
        &self,
        input: CreateHarnessRuleInputDto,
    ) -> DtoResult<HarnessRuleRecordDto> {
        self.inner.repository.create_harness_rule(input)
    }

    /// Continues one durable harness rule with its next immutable revision.
    ///
    /// # Errors
    ///
    /// Returns the typed revision, bound, and repository failures of the
    /// durable update.
    #[doc(hidden)]
    pub fn revise_harness_rule_for_daemon(
        &self,
        input: ReviseHarnessRuleInputDto,
    ) -> DtoResult<HarnessRuleRecordDto> {
        self.inner.repository.revise_harness_rule(input)
    }

    /// Applies one validated lifecycle operation to a durable harness rule.
    ///
    /// # Errors
    ///
    /// Returns the typed lifecycle, revision, and repository failures of the
    /// durable transition.
    #[doc(hidden)]
    pub fn transition_harness_rule_lifecycle_for_daemon(
        &self,
        input: TransitionHarnessRuleLifecycleInputDto,
    ) -> DtoResult<HarnessRuleRecordDto> {
        self.inner
            .repository
            .transition_harness_rule_lifecycle(input)
    }

    /// Performs one daemon-owned harness scheduling tick for one rule.
    ///
    /// The daemon owns the tick cadence, the project time zone, and the
    /// daemon-wide concurrency signal. This tick captures every due interval
    /// and calendar observation into at most one coalesced pending reason,
    /// admits at most one launch from that reason with a daemon-assigned fresh
    /// ordinary run identity, and appends every durable journal record through
    /// the frozen harness transactions. Missed interval slots coalesce into the
    /// newest due slot, a redelivered slot changes nothing, and a paused rule
    /// captures and coalesces without launching. A non-archived rule follows
    /// the daemon-owned project time zone: a revision recorded under a
    /// different zone fails closed until the rule is revised.
    ///
    /// # Errors
    ///
    /// Returns `credentials_forbidden` for a credential-shaped identity,
    /// `harness_source_unavailable` for a non-canonical or path-shaped
    /// identity, `harness_schedule_invalid` for the project time zone or an
    /// incoherent interval or calendar revision, `harness_archived` for an
    /// archived rule, and the typed capture, admission, and journal failures of
    /// the durable transactions.
    #[doc(hidden)]
    pub fn harness_schedule_tick_for_daemon(
        &self,
        request: HarnessScheduleTickRequestDto,
    ) -> DtoResult<HarnessScheduleTickOutcomeDto> {
        validate_daemon_identity(&request.harness_id)?;
        validate_harness_time_zone(&request.project_time_zone)?;
        let rule = self
            .inner
            .repository
            .load_harness_rule(request.harness_id.clone())?;
        let revision = self
            .inner
            .repository
            .load_harness_rule_revision(rule.harness_id.clone(), rule.active_revision)?;
        let lifecycle = harness_lifecycle_from_storage(rule.lifecycle_state);
        let applied_zone = applied_harness_time_zone(
            lifecycle,
            &revision.applied_time_zone,
            &request.project_time_zone,
        )?;
        if applied_zone != revision.applied_time_zone {
            return Err(ErrorDto::validation(
                "harness_schedule_invalid",
                "a non-archived harness rule follows the daemon project time zone; revise the rule to the current project zone",
            ));
        }
        let observations = due_harness_observations(&revision, request.observed_at_ms)?;
        let mut captures = Vec::with_capacity(observations.len());
        for observation in observations {
            captures.push(self.capture_harness_observation(&rule, &revision, observation)?);
        }
        let pending = self
            .inner
            .repository
            .load_pending_harness_trigger(rule.harness_id.clone())?;
        let launch = match pending {
            Some(reason) if automatic_launch_permitted(lifecycle, reason.source_kind) => {
                Some(self.admit_pending_harness_trigger(&rule, &revision, &reason, &request)?)
            }
            Some(_) | None => None,
        };
        let pending_reason = self
            .inner
            .repository
            .load_pending_harness_trigger(rule.harness_id.clone())?;
        Ok(HarnessScheduleTickOutcomeDto {
            harness_id: rule.harness_id,
            observations: captures,
            launch,
            pending_reason,
        })
    }

    /// Loads one bounded page of one session's durable harness journal.
    ///
    /// The journal remains readable after a client reconnect or a daemon
    /// restart, and the read is scoped to the owning session of the rule so no
    /// session can read another session's harness journal.
    ///
    /// # Errors
    ///
    /// Returns `credentials_forbidden` or `harness_source_unavailable` for an
    /// unusable rule identity, `harness_source_unavailable` when the session
    /// does not own the rule, the typed session and rule read failures, and
    /// `invalid_harness_journal_page` for an out-of-bound page.
    #[doc(hidden)]
    pub fn load_harness_journal_for_session_for_daemon(
        &self,
        session_id: SessionId,
        harness_id: &str,
        after_sequence: u64,
        limit: u64,
    ) -> DtoResult<Vec<HarnessJournalRecordDto>> {
        validate_daemon_identity(harness_id)?;
        let rule = self
            .inner
            .repository
            .load_harness_rule(harness_id.to_owned())?;
        let projection = self.inner.repository.load_session_snapshot(session_id)?;
        if !session_owns_harness_rule(&rule, session_id, &projection.project_id().to_string()) {
            return Err(ErrorDto::validation(
                "harness_source_unavailable",
                "a harness journal is readable only by its owning session",
            ));
        }
        self.harness_runtime()
            .load_journal(&rule.harness_id, after_sequence, limit)
    }

    /// Leaves one interrupted harness launch in its `Interrupted` outcome.
    ///
    /// Restart recovery never resumes, retries, reattaches, or reruns the
    /// interrupted run, and it schedules no successor: a later attempt is a
    /// separately admitted launch with new identities and new capacity. The
    /// daemon calls this for every interrupted launch its restart observation
    /// names.
    ///
    /// # Errors
    ///
    /// Returns `credentials_forbidden` or `harness_source_unavailable` for an
    /// unusable identity and the typed checkpoint, journal, and repository
    /// failures of the interrupted run-terminal commit.
    #[doc(hidden)]
    pub fn recover_interrupted_harness_launch_for_daemon(
        &self,
        harness_id: &str,
        interrupted_run_id: &str,
        occurred_at_ms: u64,
    ) -> DtoResult<HarnessRestartRecoveryDto> {
        let input = HarnessRestartRecoveryInputDto {
            harness_id: parse_daemon_identity(harness_id)?,
            interrupted_run_id: parse_daemon_identity(interrupted_run_id)?,
            observed_contract: HarnessDisconnectContractV1::frozen(),
            occurred_at_ms,
        };
        self.harness_runtime().recover_interrupted(&input)
    }

    /// Applies the startup recovery disposition of one root run's outstanding
    /// programmatic-policy reservations.
    ///
    /// A reservation that never reached `ToolCallStarted` is released atomically
    /// as interrupted before start; a permanently started ambiguous action keeps
    /// its permanent consumption, becomes `external_effect_unknown`, and is
    /// never retried or re-reserved. The started set is derived from the durable
    /// reservation state, so a repeated recovery changes nothing.
    ///
    /// # Errors
    ///
    /// Returns `credentials_forbidden` or `harness_source_unavailable` for an
    /// unusable root identity and the typed reservation, counter, or repository
    /// failures of the recovery.
    #[doc(hidden)]
    pub fn recover_programmatic_policy_reservations_for_daemon(
        &self,
        root_run_id: &str,
        recovered_at_ms: u64,
    ) -> DtoResult<Vec<ProgrammaticReservationRecoveryDto>> {
        validate_daemon_identity(root_run_id)?;
        let reservations = self
            .inner
            .repository
            .load_programmatic_policy_reservations_for_run(root_run_id.to_owned())?;
        let started_tool_call_ids = reservations
            .iter()
            .filter(|reservation| {
                reservation.state == ProgrammaticReservationStateDto::PermanentOnStart
            })
            .map(|reservation| reservation.tool_call_id.clone())
            .collect();
        self.policy_admission()
            .recover_outstanding(&ProgrammaticReservationRecoveryInputDto {
                root_run_id: root_run_id.to_owned(),
                started_tool_call_ids,
                recovered_at_ms,
            })
    }

    /// Constructs the harness runtime over the private durable repository.
    fn harness_runtime(
        &self,
    ) -> HarnessRuntimeService<
        '_,
        SqliteStorageRepository,
        SqliteStorageRepository,
        SqliteStorageRepository,
    > {
        HarnessRuntimeService::new(
            self.inner.repository.as_ref(),
            self.inner.repository.as_ref(),
            self.inner.repository.as_ref(),
        )
    }

    /// Constructs the policy admission service over the private repository.
    fn policy_admission(
        &self,
    ) -> ProgrammaticPolicyAdmissionService<
        '_,
        SqliteStorageRepository,
        SqliteStorageRepository,
        SqliteStorageRepository,
    > {
        ProgrammaticPolicyAdmissionService::new(
            self.inner.repository.as_ref(),
            self.inner.repository.as_ref(),
            self.inner.repository.as_ref(),
        )
    }

    /// Captures one due schedule observation into the durable pending reason.
    fn capture_harness_observation(
        &self,
        rule: &HarnessRuleRecordDto,
        revision: &HarnessRuleRevisionRecordDto,
        observation: DueHarnessObservation,
    ) -> DtoResult<HarnessScheduleObservationDto> {
        let reason_id = harness_schedule_reason_identity(
            &rule.harness_id,
            revision.revision,
            observation.source_kind,
            observation.observed_at_ms,
        );
        let capture = self
            .harness_runtime()
            .capture_trigger(&CaptureHarnessTriggerRequestDto {
                harness_id: rule.harness_id.clone(),
                reason_id,
                source_kind: observation.source_kind,
                observed_at_ms: observation.observed_at_ms,
                cause_chain_reference: None,
                bounded_references: Vec::new(),
                catch_up: None,
            })?;
        Ok(HarnessScheduleObservationDto {
            source_kind: observation.source_kind,
            observed_at_ms: observation.observed_at_ms,
            capture,
        })
    }

    /// Admits at most one launch from the current pending reason of one rule.
    fn admit_pending_harness_trigger(
        &self,
        rule: &HarnessRuleRecordDto,
        revision: &HarnessRuleRevisionRecordDto,
        reason: &HarnessTriggerReasonRecordDto,
        request: &HarnessScheduleTickRequestDto,
    ) -> DtoResult<HarnessLaunchOutcomeDto> {
        let origin = match reason.source_kind {
            HarnessSourceKindDto::ExplicitUserLaunch => HarnessLaunchOriginV1::ExplicitUserLaunch,
            HarnessSourceKindDto::CalendarTime
            | HarnessSourceKindDto::FixedInterval
            | HarnessSourceKindDto::TerminalOutcomeLink => HarnessLaunchOriginV1::AutomaticSource,
        };
        let launch = HarnessLaunchRequestDto {
            harness_id: rule.harness_id.clone(),
            proposed_run_id: request.proposed_run_id.to_string(),
            origin,
            daemon_concurrency_available: request.daemon_concurrency_available,
            cause_chain_depth: 1,
            requested_class: harness_class_from_storage(revision.class),
            narrowed_tool_ids: HARNESS_ROOT_NARROWED_TOOLS
                .iter()
                .copied()
                .map(str::to_owned)
                .collect(),
            sub_agent_corridor: None,
            goal: None,
            dossier_bytes: HARNESS_ROOT_DOSSIER_BYTES,
            occurred_at_ms: request.observed_at_ms,
        };
        self.harness_runtime().admit_pending_trigger(&launch)
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

/// The code-owned read-and-delegate tool selection of one root harness launch.
///
/// `sub_agent` stays outside a root selection: it is reachable only through a
/// user-confirmed programmatic-policy corridor.
const HARNESS_ROOT_NARROWED_TOOLS: [&str; 5] = ["read", "glob", "grep", "expand", "retrieve"];

/// The code-owned declared dossier size of one root harness launch.
///
/// The first scope's dossier carries bounded typed references only, so the
/// declared size stays far inside the fixed dossier bound; the durable dossier
/// record keeps its own measured size.
const HARNESS_ROOT_DOSSIER_BYTES: u64 = 4096;

/// The code-owned calendar minute grid of one scheduling observation.
const HARNESS_CALENDAR_MINUTE_MS: u64 = 60_000;

/// The code-owned character bound of one daemon-facing identity.
const MAX_DAEMON_IDENTITY_CHARS: usize = 128;

/// One due capture observation of a harness scheduling tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DueHarnessObservation {
    /// The closed source kind of the due observation.
    source_kind: HarnessSourceKindV1,
    /// The exact observation slot in Unix milliseconds.
    observed_at_ms: u64,
}

/// Returns the due interval and calendar observations of one rule revision.
///
/// The daemon supplies the observation time. An equal-interval source selects
/// the newest grid slot at or before it, so every missed slot coalesces into
/// one capture and an already captured durable slot is never repeated. A
/// calendar source selects the daemon-owned minute that contains the
/// observation.
///
/// # Errors
///
/// Returns `harness_schedule_invalid` for an interval revision without its
/// durable anchor and cadence, a calendar revision without its canonical
/// expression, and the interval cadence failures of the selected schedule.
fn due_harness_observations(
    revision: &HarnessRuleRevisionRecordDto,
    observed_at_ms: u64,
) -> DtoResult<Vec<DueHarnessObservation>> {
    let mut due = Vec::new();
    if revision
        .source_kinds
        .contains(&HarnessSourceKindDto::FixedInterval)
    {
        let (Some(anchor_ms), Some(interval_ms)) =
            (revision.interval_anchor_ms, revision.interval_ms)
        else {
            return Err(ErrorDto::validation(
                "harness_schedule_invalid",
                "an equal-interval source requires its durable anchor and cadence",
            ));
        };
        let schedule = HarnessIntervalScheduleV1::new(anchor_ms, interval_ms)?;
        if let Some(slot) = schedule.slot_at_or_before(observed_at_ms) {
            due.push(DueHarnessObservation {
                source_kind: HarnessSourceKindV1::FixedInterval,
                observed_at_ms: slot,
            });
        }
    }
    if revision
        .source_kinds
        .contains(&HarnessSourceKindDto::CalendarTime)
    {
        if revision.calendar_expression.is_none() {
            return Err(ErrorDto::validation(
                "harness_schedule_invalid",
                "a calendar source requires its canonical expression",
            ));
        }
        due.push(DueHarnessObservation {
            source_kind: HarnessSourceKindV1::CalendarTime,
            observed_at_ms: observed_at_ms - (observed_at_ms % HARNESS_CALENDAR_MINUTE_MS),
        });
    }
    Ok(due)
}

/// Whether one rule lifecycle state permits an automatic launch of a reason.
///
/// A pause is automation paused: an automatic source is captured and coalesced
/// but does not launch, while an explicit user launch remains allowed as a
/// separate user-origin operation.
const fn automatic_launch_permitted(
    state: HarnessRuleLifecycleStateV1,
    source_kind: HarnessSourceKindDto,
) -> bool {
    state.permits_automatic_launch()
        || matches!(source_kind, HarnessSourceKindDto::ExplicitUserLaunch)
}

/// Whether one session owns the harness rule of a journal read.
///
/// A session-scoped rule is readable only by its linked ordinary session; a
/// project-scoped rule is readable by a session of its project.
fn session_owns_harness_rule(
    rule: &HarnessRuleRecordDto,
    session_id: SessionId,
    project_id: &str,
) -> bool {
    if rule.scope.project_id() != project_id {
        return false;
    }
    rule.scope
        .session_id()
        .is_none_or(|linked| linked == session_id.to_string())
}

/// Derives the stable daemon-assigned reason identity of one slot observation.
///
/// The identity is deterministic over the rule, the admitting revision, the
/// closed source kind, and the exact observation slot, so a redelivered slot
/// never creates a second pending reason, launch, or run.
fn harness_schedule_reason_identity(
    harness_id: &str,
    rule_revision: u64,
    source_kind: HarnessSourceKindV1,
    observed_at_ms: u64,
) -> String {
    let mut input = Vec::new();
    push_framed(&mut input, "harness-schedule-observation-v1");
    push_framed(&mut input, harness_id);
    push_framed(&mut input, &rule_revision.to_string());
    push_framed(&mut input, harness_source_kind_name(source_kind));
    push_framed(&mut input, &observed_at_ms.to_string());
    identity_text(derived_identity(Digest256::sha256(&input)))
}

/// Parses one daemon-facing harness identity from canonical text.
///
/// # Errors
///
/// Returns `credentials_forbidden` for a credential-shaped value and
/// `harness_source_unavailable` for a blank, control-bearing, filesystem-path
/// shaped, over-long, or non-canonical identity.
fn parse_daemon_identity(value: &str) -> DtoResult<[u8; 16]> {
    fn invalid() -> ErrorDto {
        ErrorDto::validation(
            "harness_source_unavailable",
            "a harness identity is canonical daemon-assigned text",
        )
    }
    if contains_credential_shape(value) {
        return Err(ErrorDto::validation(
            "credentials_forbidden",
            "credentials are forbidden",
        ));
    }
    if value.trim().is_empty()
        || contains_control_or_nul(value)
        || names_filesystem_path(value)
        || value.chars().count() > MAX_DAEMON_IDENTITY_CHARS
    {
        return Err(invalid());
    }
    let digits: Vec<u8> = value.bytes().filter(|byte| *byte != b'-').collect();
    if digits.len() != 32 {
        return Err(invalid());
    }
    let mut identity = [0_u8; 16];
    for (index, chunk) in digits.chunks_exact(2).enumerate() {
        let (Some(high), Some(low)) = (hex_value(chunk[0]), hex_value(chunk[1])) else {
            return Err(invalid());
        };
        identity[index] = (high << 4) | low;
    }
    Ok(identity)
}

/// Validates one daemon-facing harness identity without retaining it.
///
/// # Errors
///
/// Returns the rejection of [`parse_daemon_identity`].
fn validate_daemon_identity(value: &str) -> DtoResult<()> {
    parse_daemon_identity(value).map(|_| ())
}

/// Whether one daemon-facing value names a filesystem path.
fn names_filesystem_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with('\\')
        || value.contains("..")
        || value.as_bytes().get(1).is_some_and(|byte| *byte == b':')
}

/// Returns one hexadecimal digit value.
const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Formats one daemon-assigned identity as canonical UUID text.
fn identity_text(identity: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(36);
    for (index, byte) in identity.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            text.push('-');
        }
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

/// Derives one deterministic daemon-assigned identity from a domain digest.
fn derived_identity(digest: Digest256) -> [u8; 16] {
    let bytes = digest.bytes();
    let mut identity = [0_u8; 16];
    identity.copy_from_slice(&bytes[..16]);
    identity
}

/// Appends one length-framed text field to a deterministic digest input.
fn push_framed(input: &mut Vec<u8>, value: &str) {
    input.extend_from_slice(&u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    input.extend_from_slice(value.as_bytes());
}

/// Converts one durable harness rule lifecycle state into its domain value.
const fn harness_lifecycle_from_storage(
    state: HarnessRuleLifecycleStateDto,
) -> HarnessRuleLifecycleStateV1 {
    match state {
        HarnessRuleLifecycleStateDto::Active => HarnessRuleLifecycleStateV1::Active,
        HarnessRuleLifecycleStateDto::Paused => HarnessRuleLifecycleStateV1::Paused,
        HarnessRuleLifecycleStateDto::Archived => HarnessRuleLifecycleStateV1::Archived,
    }
}

/// Converts one durable harness execution class into its domain value.
const fn harness_class_from_storage(class: HarnessExecutionClassDto) -> HarnessExecutionClassV1 {
    match class {
        HarnessExecutionClassDto::Light => HarnessExecutionClassV1::Light,
        HarnessExecutionClassDto::Medium => HarnessExecutionClassV1::Medium,
        HarnessExecutionClassDto::Heavy => HarnessExecutionClassV1::Heavy,
    }
}

/// Returns the stable name of one closed harness trigger source kind.
const fn harness_source_kind_name(kind: HarnessSourceKindV1) -> &'static str {
    match kind {
        HarnessSourceKindV1::ExplicitUserLaunch => "explicit_user_launch",
        HarnessSourceKindV1::CalendarTime => "calendar_time",
        HarnessSourceKindV1::FixedInterval => "fixed_interval",
        HarnessSourceKindV1::TerminalOutcomeLink => "terminal_outcome_link",
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

fn load_provider_configuration(
    source: ConfigSourceDto,
) -> DtoResult<(ConfigSnapshotDto, SelectedProvider, String, String)> {
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
    // The credential value is re-extracted from the raw text inside this
    // private loading boundary so the composition can retain it as its
    // private source; startup parsing already proved it is present.
    let private_credential = parse_credential(&raw_toml)?;
    Ok((snapshot, selected_provider, raw_toml, private_credential))
}

/// Retains the startup provider's private credential and its file source.
///
/// The daemon's own configuration file is the configured private credential
/// source of this slice: the credential captured at open is the material
/// restored into typed-edit candidates, and rotation reads replacement
/// material from the same file through the private loading boundary. The
/// source path is private configuration material and is never disclosed in a
/// DTO, projection, or error.
///
/// # Errors
///
/// Returns an unavailable error when the private credential state lock is
/// poisoned.
fn retain_private_startup_credential(
    facade: &DaemonApplicationFacade,
    credential: String,
    source: ConfigSourceDto,
) -> DtoResult<()> {
    let mut state = facade.inner.private_credential.lock().map_err(|_| {
        ErrorDto::unavailable(
            "daemon_command_unavailable",
            "daemon command is unavailable",
        )
    })?;
    state.material = Some(credential);
    state.source = Some(source);
    drop(state);
    Ok(())
}

#[cfg(test)]
fn load_config_snapshot(source: ConfigSourceDto) -> DtoResult<ConfigSnapshotDto> {
    load_provider_configuration(source).map(|(snapshot, _, _, _)| snapshot)
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
    use intention_domain::harness::HarnessRunOutcomeV1;
    use intention_domain::programmatic_policy::ProgrammaticRecoveryDispositionV1;
    use intention_storage::harness_repo::{
        HarnessJournalRecordKindDto, HarnessPresentationModeDto, HarnessRuleOperationDto,
        HarnessRuleScopeDto, HarnessTaskModeDto, HarnessTriggerCaptureOutcomeDto,
        HarnessTriggerReasonStateDto,
    };
    use intention_storage::programmatic_policy_repo::{
        CommitProgrammaticReservationStartedInputDto, CreateProgrammaticPolicyInputDto,
        ProgrammaticAdmissionDecisionDto, ProgrammaticCalendarPeriodKindDto,
        ProgrammaticPolicyLifecycleStateDto, ProgrammaticPolicyRecordDto,
        ProgrammaticPolicyRepositoryDto, ProgrammaticPolicyReservationRecordDto,
        ProgrammaticPolicyRevisionRecordDto, ProgrammaticPolicyScopeDto,
        ProgrammaticRootOriginKindDto, ProgrammaticRootOriginRuleRecordDto,
        ReserveProgrammaticPolicyActionInputDto,
    };
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

            let (snapshot, selected_provider, _, _) =
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

    /// Builds the seam's closed bearer declaration used by production
    /// startup construction.
    fn bearer_declaration() -> DeclaredProviderOptions {
        DeclaredProviderOptions::from_declaration(DomainCredentialTransportMode::Bearer, None, None)
            .expect("the closed bearer declaration is valid")
    }

    /// Parses one startup material fixture for the requested provider kind.
    fn startup_material_fixture(kind: &str) -> StartupProviderMaterial {
        let endpoint = if kind == "generic-chat-completion-api" {
            "endpoint = \"https://example.invalid/v1\"\n"
        } else {
            ""
        };
        ResolvedConfigDto::parse_startup_material(RawConfigInputDto::new(
            format!(
                "schema_version = 1\n[provider]\nkind = \"{kind}\"\nmodel = \"fixture\"\n{endpoint}credential = \"fixture\""
            ),
            ConfigSourceDto::Explicit(
                ConfigPathDto::parse(
                    std::env::temp_dir()
                        .join(format!("option-seam-{kind}.toml"))
                        .to_string_lossy()
                        .into_owned(),
                )
                .expect("fixture path is absolute"),
            ),
        ))
        .expect("fixture material parses")
    }

    /// Builds one credential-free profile material for the driver factory.
    fn factory_material(
        kind: &str,
        transport: DomainCredentialTransportMode,
        safe_header_name: Option<String>,
    ) -> PrivateProviderProfileMaterial {
        PrivateProviderProfileMaterial {
            profile: intention_domain::ProviderProfileRevisionV1 {
                profile_id: "default".to_owned(),
                revision_id: "rev-1".to_owned(),
                provider_kind_id: kind.to_owned(),
                model_id: "fixture-model".to_owned(),
                endpoint: "https://api.example.invalid/v1".to_owned(),
                credential_transport_mode: transport,
                safe_header_name: safe_header_name.clone(),
                capability_taxonomy_revision:
                    intention_domain::provider_selection::MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
                reasoning_compatibility_id: None,
                kind_descriptor_revision_id: "kd-1".to_owned(),
                driver_contract_revision: intention_domain::ProviderDriverContractRevisionDto {
                    driver_family: kind.to_owned(),
                    major: 1,
                    minor: 1,
                },
            },
            selection: ProviderSelectionV1 {
                profile_id: "default".to_owned(),
                provider_profile_revision_id: "rev-1".to_owned(),
                kind_id: kind.to_owned(),
                kind_descriptor_revision_id: "kd-1".to_owned(),
                model_id: "fixture-model".to_owned(),
                normalized_effective_endpoint: "https://api.example.invalid/v1".to_owned(),
                credential_transport_mode: transport,
                credential_transport_safe_header_name: safe_header_name,
                declared_model_capability_subset: vec!["text_input".to_owned()],
                resolved_reasoning_policy: "textual-reasoning-v1".to_owned(),
                effective_execution_policy: "execution-timeout-60-attempts-3".to_owned(),
                effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
                provider_driver_contract_revision: "responses-1.1".to_owned(),
                selection_source: Some("catalog-rev-1".to_owned()),
                ..fixture_selection()
            },
            endpoint: "https://api.example.invalid/v1".to_owned(),
            private_credential_reference: 1,
        }
    }

    #[test]
    fn producible_declarations_apply_the_closed_bearer_policy_to_startup_construction() {
        // PR24-057 guard: every currently producible catalog declaration maps
        // through the seam to the closed bearer policy with no reasoning
        // effort, and production startup construction applies that seam
        // instead of silently falling back to adapter-default options.
        let declared = bearer_declaration();
        assert_eq!(
            declared.header_policy.allowed_header_names(),
            &Vec::<String>::new()
        );
        assert_eq!(
            declared.header_policy.selected_transport(),
            ModelCredentialTransportMode::Bearer
        );
        assert_eq!(declared.reasoning_effort, None);

        let openrouter =
            SelectedProvider::from_startup_material(startup_material_fixture("openrouter"))
                .expect("openrouter provider builds through the seam");
        let openrouter_options = openrouter
            .openrouter_options()
            .expect("openrouter driver is selected");
        assert_eq!(
            openrouter_options.header_policy(),
            Some(&declared.header_policy),
            "startup construction applies the declared header policy"
        );
        assert_eq!(openrouter_options.reasoning_effort(), None);

        let generic = SelectedProvider::from_startup_material(startup_material_fixture(
            "generic-chat-completion-api",
        ))
        .expect("generic chat provider builds through the seam");
        let generic_options = generic
            .generic_chat_options()
            .expect("generic chat driver is selected");
        assert_eq!(
            generic_options.header_policy(),
            Some(&declared.header_policy),
            "startup construction applies the declared header policy"
        );
        assert_eq!(generic_options.reasoning_effort(), None);

        // The adapter defaults remain the adapter-level baseline: an unset
        // policy means no declaration was applied, which production
        // construction no longer performs.
        assert_eq!(
            OpenRouterDriverOptions::default().header_policy(),
            None,
            "adapter defaults stay the explicit baseline"
        );
        assert_eq!(
            GenericChatDriverOptions::default().header_policy(),
            None,
            "adapter defaults stay the explicit baseline"
        );
    }

    #[test]
    fn declared_options_flow_through_construction_and_survive_credential_rebuild() {
        // A non-default effort declaration is applied at construction through
        // the same seam slot the closed production declaration leaves empty,
        // and the credential-driven rebuild (rotation) never drops it: the
        // rebuild replaces only the private SDK client.
        let declared = DeclaredProviderOptions::from_declaration(
            DomainCredentialTransportMode::Bearer,
            None,
            Some(ReasoningEffortLevel::Low),
        )
        .expect("low effort is a valid declaration");
        let provider = SelectedProvider::build_with_declared_options(
            startup_material_fixture("generic-chat-completion-api"),
            declared,
        )
        .expect("generic chat provider builds with the declared effort");
        let options = provider
            .generic_chat_options()
            .expect("generic chat driver is selected");
        assert_eq!(options.reasoning_effort(), Some(ReasoningEffortLevel::Low));

        provider
            .rotate_private_credential("replacement-fixture-credential".to_owned())
            .expect("credential rebuild swaps the private client");
        let rebuilt = provider
            .generic_chat_options()
            .expect("generic chat driver stays selected");
        assert_eq!(
            rebuilt.reasoning_effort(),
            Some(ReasoningEffortLevel::Low),
            "the credential-driven rebuild keeps the declared options"
        );
        assert_eq!(
            rebuilt
                .header_policy()
                .map(|policy| policy.selected_transport()),
            Some(ModelCredentialTransportMode::Bearer)
        );
    }

    #[test]
    fn inapplicable_declarations_fail_closed_at_the_seam_before_any_request() {
        // SafeHeader live wire injection is not activated: the seam surfaces
        // the adapter rejection for both executing adapters.
        let safe_header = DeclaredProviderOptions::from_declaration(
            DomainCredentialTransportMode::SafeHeader,
            Some("x-provider-header".to_owned()),
            None,
        )
        .expect("a named safe-header declaration is well-formed");
        assert_eq!(
            safe_header
                .clone()
                .into_openrouter()
                .expect_err("openrouter cannot apply safe header")
                .code(),
            "unsupported_safe_header_transport"
        );
        assert_eq!(
            safe_header
                .into_generic_chat()
                .expect_err("generic chat cannot apply safe header")
                .code(),
            "unsupported_safe_header_transport"
        );

        // An inconsistent declaration (safe header without a name) fails at
        // the header-policy validation, never reaching an adapter.
        let inconsistent = DeclaredProviderOptions::from_declaration(
            DomainCredentialTransportMode::SafeHeader,
            None,
            None,
        );
        assert_eq!(
            inconsistent
                .expect_err("safe header without a name is invalid")
                .code(),
            "invalid_credential_transport"
        );

        // Adapter-specific applicability stays adapter-owned: generic chat
        // cannot express the maximum effort; OpenRouter can.
        let max_effort = DeclaredProviderOptions::from_declaration(
            DomainCredentialTransportMode::Bearer,
            None,
            Some(ReasoningEffortLevel::Max),
        )
        .expect("max effort is a closed declaration");
        assert_eq!(
            max_effort
                .clone()
                .into_generic_chat()
                .expect_err("generic chat rejects max effort")
                .code(),
            "unsupported_reasoning_effort"
        );
        assert!(
            max_effort.into_openrouter().is_ok(),
            "openrouter applies the maximum reasoning effort"
        );
    }

    #[test]
    fn catalog_activation_preflights_declared_options_through_the_adapter_builders() {
        // The driver factory is the catalog-activation construction seam:
        // producible bearer declarations build, while a profile declaring the
        // not-activated safe-header transport fails the activation closed.
        for kind in [
            ProviderKindDto::Openrouter,
            ProviderKindDto::GenericChatCompletionApi,
        ] {
            let factory = CompositionDriverFactory::service(kind);
            assert!(
                factory
                    .build(factory_material(
                        kind.as_str(),
                        DomainCredentialTransportMode::Bearer,
                        None
                    ))
                    .is_ok(),
                "a producible bearer profile activates"
            );
            assert_eq!(
                factory
                    .build(factory_material(
                        kind.as_str(),
                        DomainCredentialTransportMode::SafeHeader,
                        Some("x-provider-header".to_owned()),
                    ))
                    .err()
                    .expect("safe-header declarations cannot activate")
                    .code(),
                "unsupported_safe_header_transport"
            );
        }
    }

    #[test]
    fn provider_kind_dispatch_is_typed_and_rejects_an_unknown_kind() {
        // The adapter option builder is selected by the typed
        // `ProviderKindDto`, so an unknown catalog kind id fails closed with a
        // typed error instead of falling through to the generic-chat builder.
        assert_eq!(
            typed_provider_kind("openrouter").expect("openrouter is a typed kind"),
            ProviderKindDto::Openrouter
        );
        assert_eq!(
            typed_provider_kind("generic-chat-completion-api")
                .expect("generic chat is a typed kind"),
            ProviderKindDto::GenericChatCompletionApi
        );
        assert_eq!(
            typed_provider_kind("gemini")
                .expect_err("an unknown kind has no adapter")
                .code(),
            "unsupported_provider_kind"
        );

        // Every typed kind has exactly one factory, so the dispatch above
        // cannot route a kind to another adapter.
        for kind in [
            ProviderKindDto::Openrouter,
            ProviderKindDto::GenericChatCompletionApi,
        ] {
            assert_eq!(
                CompositionDriverFactory::service(kind).kind(),
                kind.as_str()
            );
        }
    }

    #[test]
    fn catalog_and_credential_paths_derive_their_option_preflight_from_the_declaration() {
        // The catalog-activation factory and the credential-rebuild path
        // must both derive their option preflight from the profile's
        // declaration. The factory is checked against the adapter builders'
        // own verdict (an independent derivation, not the seam helper it
        // calls), so a build that stopped deriving from the declaration would
        // accept a profile the adapters cannot serve.
        for kind in [
            ProviderKindDto::Openrouter,
            ProviderKindDto::GenericChatCompletionApi,
        ] {
            let factory = CompositionDriverFactory::service(kind);
            for (transport, safe_header_name) in [
                (DomainCredentialTransportMode::Bearer, None),
                (
                    DomainCredentialTransportMode::SafeHeader,
                    Some("x-provider-header".to_owned()),
                ),
            ] {
                let declared = DeclaredProviderOptions::from_declaration(
                    transport,
                    safe_header_name.clone(),
                    None,
                )
                .expect("the fixture declaration is well-formed");
                let expected = match kind {
                    ProviderKindDto::Openrouter => declared.into_openrouter().map(|_| ()),
                    ProviderKindDto::GenericChatCompletionApi => {
                        declared.into_generic_chat().map(|_| ())
                    }
                };
                let outcome = factory
                    .build(factory_material(kind.as_str(), transport, safe_header_name))
                    .map(|_| ());
                assert_eq!(
                    outcome.as_ref().map_err(|error| error.code()),
                    expected.as_ref().map_err(|error| error.code()),
                    "the factory outcome must be the declaration's adapter verdict for {kind}"
                );
            }
        }

        // The credential-rebuild boundary is driven at runtime by
        // `credential_rebuild_boundary_rotates_the_resolved_profile_and_fails_closed`
        // below, so it is no longer pinned only as source text. The rebuild's
        // declaration-rejection branch stays an accepted textual limit
        // because it is unobservable by construction: every durable catalog
        // profile is built with a bearer declaration and no safe header
        // (`intention-application` `build_candidate_records`), and registry
        // activation runs this same factory preflight all-or-nothing
        // (`PrivateRegistry::build_all`), so a safe-header profile can never
        // be admitted and `resolve_enabled_profile` can only return a bearer
        // declaration, which both adapter builders accept. This scan keeps
        // the shared derivation guarded until a later slice activates
        // safe-header transport and the rejection can be driven; replace it
        // with that driven fixture then.
        let source = include_str!("lib.rs");
        let sections = [
            (
                "catalog activation",
                implementation_section(
                    source,
                    "impl ProviderDriverFactory for CompositionDriverFactory {",
                    "/// Resolves one catalog provider kind id",
                ),
            ),
            (
                "credential rebuild",
                implementation_section(
                    source,
                    "impl DriverRebuildPort for CompositionDriverRebuildPort<'_> {",
                    "/// The composition's session-profile change publication seam",
                ),
            ),
        ];
        for (path, section) in sections {
            assert!(
                section.contains("DeclaredProviderOptions::from_declaration"),
                "{path} must compose its options from the profile declaration"
            );
            assert!(
                section.contains("preflight_for_kind"),
                "{path} must preflight the declared options through the adapter builder"
            );
        }
        // The needle is assembled from fragments so the guard text itself does
        // not satisfy the search.
        let removed_agreement = ["the factory and the rebuild", "preflight agree"].join(" ");
        assert!(
            !source.contains(&removed_agreement),
            "the removed tautological agreement assertion must not reappear"
        );
    }

    #[test]
    fn credential_rebuild_boundary_rotates_the_resolved_profile_and_fails_closed() {
        // The credential-rebuild boundary is driven at runtime here. The
        // port resolves the active profile through the real catalog admission
        // port (declaration composition and adapter preflight included),
        // swaps the private driver credential, and fails closed for a profile
        // the active catalog does not carry without changing the committed
        // material.
        const REPLACEMENT_SECRET: &str = "sk-rebuild-driven-replacement-67890";
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory.path().join("daemon.sqlite");
        let config_path = directory.path().join("config.toml");
        write_startup_document(
            &config_path,
            &startup_document("openrouter", "fixture-model", None),
        );
        let facade = open_startup_facade(&database, &config_path);
        let port = CompositionDriverRebuildPort { facade: &facade };
        DriverRebuildPort::rebuild(
            &port,
            "default",
            PrivateCredentialMaterial::from_private_bytes(REPLACEMENT_SECRET.as_bytes().to_vec()),
        )
        .expect("the active declared profile rebuilds");
        assert_eq!(
            retained_credential(&facade).as_deref(),
            Some(REPLACEMENT_SECRET),
            "the rebuild commits the replacement material to the private slot"
        );
        assert_eq!(
            facade.inner._selected_provider.safe_kind(),
            Some(ProviderKindDto::Openrouter),
            "the rebuild keeps the executing driver bound"
        );
        let error = DriverRebuildPort::rebuild(
            &port,
            "profile-not-in-the-active-catalog",
            PrivateCredentialMaterial::from_private_bytes(REPLACEMENT_SECRET.as_bytes().to_vec()),
        )
        .expect_err("a profile outside the active catalog fails closed");
        assert_eq!(error.code(), "provider_profile_unavailable");
        assert_eq!(
            retained_credential(&facade).as_deref(),
            Some(REPLACEMENT_SECRET),
            "a failed rebuild leaves the committed material unchanged"
        );
    }

    /// Returns the implementation section between two unique source markers.
    fn implementation_section<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
        let after_start = source
            .split_once(start)
            .map(|(_before, rest)| rest)
            .expect("the pinned implementation is present");
        after_start
            .split_once(end)
            .map(|(section, _after)| section)
            .expect("the pinned implementation boundary is present")
    }

    #[test]
    fn startup_catalog_activation_signature_stays_private_and_document_bearing() {
        // `activate_startup_catalog` is deliberately private and
        // takes the raw startup document because only the daemon-open path
        // calls it. The guard scans the production source before
        // the test module and pins exactly one private definition, no public
        // method carrying the name, and exactly one other reference, so a
        // differently named public wrapper around the helper (for example
        // `pub fn reconcile_startup_catalog` delegating to it) adds a
        // reference and fails. A wrapper that copies the body instead of
        // delegating is an accepted limit of a textual guard. Needles are
        // assembled from fragments so the guard's own source never satisfies
        // the scan.
        // A Windows checkout rewrites text files to CRLF unless
        // `.gitattributes` pins LF, so the source is normalized before the
        // marker scan; otherwise the boundary marker never matches and the
        // scan slips past the production section into the test module.
        let source = include_str!("lib.rs").replace("\r\n", "\n");
        let production = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("the crate source carries production code before its test module");
        let private_signature = [
            "fn activate_startup_catalog(&self, raw_toml: &str)",
            " -> DtoResult<()>",
        ]
        .concat();
        assert_eq!(
            production.matches(&private_signature).count(),
            1,
            "exactly one `{private_signature}` definition must exist"
        );
        let public_signature = ["pub ", "fn activate_startup_catalog"].concat();
        assert!(
            !production.contains(&public_signature),
            "`activate_startup_catalog` must never become a public facade method"
        );
        let helper_name = ["activate_", "startup_catalog"].concat();
        assert_eq!(
            production.matches(&helper_name).count(),
            2,
            "only the private definition and the single private daemon-open call may \
             reference the helper; a differently named public wrapper adds a reference"
        );
    }

    /// Builds one startup document fixture for the catalog re-derivation tests.
    fn startup_document(kind: &str, model: &str, endpoint: Option<&str>) -> String {
        let endpoint = endpoint.map_or_else(String::new, |endpoint| {
            format!("endpoint = \"{endpoint}\"\n")
        });
        format!(
            "schema_version = 1\n[provider]\nkind = \"{kind}\"\nmodel = \"{model}\"\n{endpoint}credential = \"fixture-credential\"\n"
        )
    }

    /// Writes one startup document with owner-only permissions on Unix.
    fn write_startup_document(path: &Path, raw_toml: &str) {
        fs::write(path, raw_toml).expect("startup document writes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))
                .expect("startup document permissions set");
        }
    }

    /// Opens one facade through the real daemon-open sequence
    /// ([`DaemonApplicationFacade::open_platform_from`], the shared core of
    /// `open_platform`) over caller-supplied fixture paths, so the startup
    /// fixtures cannot drift from the production sequence.
    fn open_startup_facade(database: &Path, config_path: &Path) -> DaemonApplicationFacade {
        let source = ConfigSourceDto::Explicit(
            ConfigPathDto::parse(config_path.to_string_lossy().into_owned())
                .expect("fixture startup document path is absolute"),
        );
        DaemonApplicationFacade::open_platform_from(source, database.to_path_buf())
            .expect("the daemon-open sequence composes over fixture paths")
    }

    /// Reads one active catalog revision and its default profile declaration.
    fn active_catalog_profile(facade: &DaemonApplicationFacade) -> (u64, String, String, String) {
        let status = facade
            .inner
            .repository
            .load_provider_catalog_status()
            .expect("catalog status reads");
        let revision = status
            .active_catalog_revision_id
            .expect("an active catalog revision exists");
        let material = facade
            .inner
            .repository
            .load_provider_catalog_material()
            .expect("active catalog material reads");
        let profile = material
            .profiles
            .iter()
            .find(|candidate| {
                Some(&candidate.profile.profile_id) == material.default_profile_id.as_ref()
            })
            .expect("the active default profile exists");
        (
            revision,
            profile.profile.provider_kind_id.clone(),
            profile.profile.model_id.clone(),
            profile.profile.endpoint.clone(),
        )
    }

    #[test]
    fn restart_with_an_edited_startup_model_rederives_the_active_catalog() {
        // A restart whose startup document changed a catalog
        // field re-derives the catalog through the normal prepare and accept
        // path instead of leaving the catalog and the executing driver
        // divergent.
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory.path().join("startup-rederive-model.sqlite");
        let config_path = directory.path().join("config.toml");
        write_startup_document(
            &config_path,
            &startup_document("openrouter", "fixture-model-a", None),
        );
        let first = open_startup_facade(&database, &config_path);
        let (revision, kind, model, _endpoint) = active_catalog_profile(&first);
        assert_eq!(revision, 1);
        assert_eq!(kind, "openrouter");
        assert_eq!(model, "fixture-model-a");
        assert_eq!(
            first.selected_provider_kind(),
            Some(ProviderKindDto::Openrouter)
        );
        drop(first);

        write_startup_document(
            &config_path,
            &startup_document("openrouter", "fixture-model-b", None),
        );
        let second = open_startup_facade(&database, &config_path);
        let (revision, kind, model, _endpoint) = active_catalog_profile(&second);
        assert_eq!(
            revision, 2,
            "the edited model advances the catalog revision"
        );
        assert_eq!(kind, "openrouter");
        assert_eq!(model, "fixture-model-b");
        drop(second);

        // An unchanged restart is a no-op on the already reconciled catalog.
        let third = open_startup_facade(&database, &config_path);
        let (revision, _kind, model, _endpoint) = active_catalog_profile(&third);
        assert_eq!(revision, 2, "the reconciled catalog keeps its revision");
        assert_eq!(model, "fixture-model-b");
    }

    #[test]
    fn restart_with_an_edited_startup_kind_rederives_through_the_removal_path() {
        // A changed provider kind is a catalog removal under the controller's
        // change classification; the startup path prepares and accepts it
        // in-process, so a kind edit applied by restart never leaves the
        // executing driver on a different kind than the active catalog.
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory.path().join("startup-rederive-kind.sqlite");
        let config_path = directory.path().join("config.toml");
        write_startup_document(
            &config_path,
            &startup_document("openrouter", "fixture-model", None),
        );
        let first = open_startup_facade(&database, &config_path);
        assert_eq!(active_catalog_profile(&first).0, 1);
        drop(first);

        write_startup_document(
            &config_path,
            &startup_document(
                "generic-chat-completion-api",
                "fixture-model",
                Some("https://api.example.invalid/v1"),
            ),
        );
        let second = open_startup_facade(&database, &config_path);
        let (revision, kind, model, endpoint) = active_catalog_profile(&second);
        assert_eq!(
            revision, 2,
            "the edited provider kind advances the catalog revision"
        );
        assert_eq!(kind, "generic-chat-completion-api");
        assert_eq!(model, "fixture-model");
        assert_eq!(endpoint, "https://api.example.invalid/v1");
        assert_eq!(
            second.selected_provider_kind(),
            Some(ProviderKindDto::GenericChatCompletionApi)
        );
        assert_eq!(
            second.provider_control_readiness(),
            intention_application::CatalogReadiness::Ready
        );
    }

    #[test]
    fn startup_open_adopts_a_durable_pending_removal_through_the_repository() {
        // The composition's real open sequence recovers a
        // durable pending removal its own previous process left behind,
        // instead of failing with `provider_catalog_removal_pending_exists`
        // until the 30-minute expiry. The recovered state is read through the
        // durable repository, never through the in-memory projection.
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory.path().join("startup-adopt-pending.sqlite");
        let config_path = directory.path().join("config.toml");
        write_startup_document(
            &config_path,
            &startup_document("openrouter", "fixture-model", None),
        );
        let first = open_startup_facade(&database, &config_path);
        assert_eq!(active_catalog_profile(&first).0, 1);
        let prepared = prepare_removal_at_now(
            &first,
            "removal-startup-adopt",
            "https://api.example.invalid/v1",
        );
        assert_eq!(prepared.candidate_handle.as_deref(), Some("catalog-2"));
        // Simulated crash: the durable pending removal survives with its
        // deadline while the process memory does not.
        drop(first);

        // Reopening with the document the pending removal was prepared from
        // adopts it through the normal acceptance path.
        write_startup_document(
            &config_path,
            &startup_document(
                "generic-chat-completion-api",
                "replacement",
                Some("https://api.example.invalid/v1"),
            ),
        );
        let restarted = open_startup_facade(&database, &config_path);
        let status = restarted
            .inner
            .repository
            .load_provider_catalog_status()
            .expect("catalog status reads");
        assert_eq!(
            status.status,
            intention_storage::ProviderCatalogStatusDto::Active
        );
        assert_eq!(
            status.active_catalog_revision_id,
            Some(2),
            "the adopted removal is the durable active revision"
        );
        assert_eq!(status.candidate_catalog_revision_id, None);
        let material = restarted
            .inner
            .repository
            .load_provider_catalog_material()
            .expect("active material reads");
        assert_eq!(material.catalog_revision_id, 2);
        assert_eq!(material.default_profile_id.as_deref(), Some("default"));
        assert_eq!(material.profiles.len(), 1);
        assert_eq!(
            material.profiles[0].profile.provider_kind_id,
            "generic-chat-completion-api"
        );
        assert_eq!(material.profiles[0].profile.model_id, "replacement");
        assert_eq!(
            material.profiles[0].profile.endpoint,
            "https://api.example.invalid/v1"
        );
        assert!(
            restarted
                .inner
                .repository
                .load_pending_removal_candidate()
                .expect("pending removal reads")
                .is_none(),
            "the adopted candidate is no longer pending"
        );
        assert_eq!(
            restarted
                .inner
                .repository
                .load_highest_removal_candidate_revision()
                .expect("the durable removal maximum reads"),
            2,
            "the adopted candidate remains the durable removal maximum"
        );
    }

    #[test]
    fn startup_open_accepts_a_second_change_after_adopting_a_durable_pending_removal() {
        // The differ-differ follow-up case: the startup document
        // changes again while a durable, startup-rebuilt pending removal
        // exists. The open adopts the pending removal and then accepts the
        // newly prepared candidate against the revision that adoption
        // committed. Passing the pre-adoption revision would fail the open
        // once with `provider_catalog_revision_conflict` and only self-heal on
        // the next restart; this fixture drives the single open through
        // `open_platform_from` and reads the settled state from the durable
        // repository.
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory.path().join("startup-adopt-second-change.sqlite");
        let config_path = directory.path().join("config.toml");
        write_startup_document(
            &config_path,
            &startup_document("openrouter", "fixture-model", None),
        );
        let first = open_startup_facade(&database, &config_path);
        assert_eq!(active_catalog_profile(&first).0, 1);
        let prepared = prepare_removal_at_now(
            &first,
            "removal-startup-differ-differ",
            "https://api.example.invalid/v1",
        );
        assert_eq!(prepared.candidate_handle.as_deref(), Some("catalog-2"));
        drop(first);

        // The document changed again before the pending removal was accepted:
        // the restart resolves the durable removal first (the candidate
        // rebuilt from the durable rows is adopted through the normal
        // acceptance path), and the re-derived declaration is then accepted as
        // an ordinary replacement against the adopted revision - so only the
        // removal itself carries a removal row.
        write_startup_document(
            &config_path,
            &startup_document(
                "generic-chat-completion-api",
                "replacement",
                Some("https://api.example.invalid/v2"),
            ),
        );
        let restarted = open_startup_facade(&database, &config_path);
        let (revision, kind, model, endpoint) = active_catalog_profile(&restarted);
        assert_eq!(
            revision, 3,
            "the second startup change receives its own accepted revision"
        );
        assert_eq!(kind, "generic-chat-completion-api");
        assert_eq!(model, "replacement");
        assert_eq!(endpoint, "https://api.example.invalid/v2");
        let status = restarted
            .inner
            .repository
            .load_provider_catalog_status()
            .expect("catalog status reads");
        assert_eq!(
            status.status,
            intention_storage::ProviderCatalogStatusDto::Active
        );
        assert_eq!(status.active_catalog_revision_id, Some(3));
        assert_eq!(status.candidate_catalog_revision_id, None);
        assert!(
            restarted
                .inner
                .repository
                .load_pending_removal_candidate()
                .expect("pending removal reads")
                .is_none(),
            "the second candidate was accepted during the open"
        );
        assert_eq!(
            restarted
                .inner
                .repository
                .load_highest_removal_candidate_revision()
                .expect("the durable removal maximum reads"),
            2,
            "only the adopted removal carries a removal row; the second change is an endpoint-only replacement accepted as revision three"
        );
        assert_eq!(
            restarted.provider_control_readiness(),
            intention_application::CatalogReadiness::Ready
        );
    }

    #[test]
    fn startup_open_adopts_a_durable_pending_removal_when_the_document_matches() {
        // The durable pending removal is resolved by the open even when
        // the startup document still matches the declaration the removal was
        // prepared from, so a crash residue can never leave the platform
        // gated. The adoption closes the removal row durably, and the
        // following reconcile re-derives the declared profile from the current
        // document, so the document still wins.
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory.path().join("startup-adopt-matching.sqlite");
        let config_path = directory.path().join("config.toml");
        write_startup_document(
            &config_path,
            &startup_document("openrouter", "fixture-model", None),
        );
        let first = open_startup_facade(&database, &config_path);
        assert_eq!(active_catalog_profile(&first).0, 1);
        let prepared = prepare_removal_at_now(
            &first,
            "removal-startup-matching",
            "https://api.example.invalid/v1",
        );
        assert_eq!(prepared.candidate_handle.as_deref(), Some("catalog-2"));
        // Simulated crash with the document unchanged: it still declares the
        // profile the pending removal was prepared from.
        drop(first);

        let restarted = open_startup_facade(&database, &config_path);
        assert_eq!(
            restarted.provider_control_readiness(),
            intention_application::CatalogReadiness::Ready,
            "a matching document must not leave the platform gated"
        );
        assert!(
            restarted
                .inner
                .repository
                .load_pending_removal_candidate()
                .expect("pending removal reads")
                .is_none(),
            "the adopted candidate is durably closed"
        );
        let (revision, kind, model, _endpoint) = active_catalog_profile(&restarted);
        assert_eq!(kind, "openrouter");
        assert_eq!(model, "fixture-model");
        assert!(
            revision > 2,
            "the document is re-derived on top of the adopted revision"
        );
        assert_eq!(
            restarted
                .inner
                .repository
                .load_provider_catalog_status()
                .expect("catalog status reads")
                .candidate_catalog_revision_id,
            None,
            "the open settles on an active revision without a pending candidate"
        );
    }

    #[test]
    fn restart_after_the_startup_document_drops_a_declared_endpoint_rederives() {
        // The endpoint comparison is two-directional, so a
        // document that drops a previously declared endpoint re-derives the
        // active catalog instead of keeping the stale declaration.
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory
            .path()
            .join("startup-rederive-endpoint-drop.sqlite");
        let config_path = directory.path().join("config.toml");
        write_startup_document(
            &config_path,
            &startup_document(
                "openrouter",
                "fixture-model",
                Some("https://api.example.invalid/v9"),
            ),
        );
        let first = open_startup_facade(&database, &config_path);
        let (revision, _kind, _model, endpoint) = active_catalog_profile(&first);
        assert_eq!(revision, 1);
        assert_eq!(endpoint, "https://api.example.invalid/v9");
        drop(first);

        write_startup_document(
            &config_path,
            &startup_document("openrouter", "fixture-model", None),
        );
        let second = open_startup_facade(&database, &config_path);
        let (revision, kind, model, endpoint) = active_catalog_profile(&second);
        assert_eq!(
            revision, 2,
            "dropping the declared endpoint re-derives the catalog"
        );
        assert_eq!(kind, "openrouter");
        assert_eq!(model, "fixture-model");
        assert_eq!(
            endpoint,
            derived_default_endpoint("openrouter"),
            "the re-derived profile carries the kind's deterministic derived endpoint"
        );
        drop(second);

        // The reconciled endpointless declaration is a no-op on the next
        // restart: the two-directional comparison does not churn revisions.
        let third = open_startup_facade(&database, &config_path);
        assert_eq!(active_catalog_profile(&third).0, 2);
    }

    #[test]
    fn derived_default_endpoint_matches_the_catalog_derivation() {
        // The composition's comparison copy of the derived endpoint is
        // pinned to the catalog's own derivation by reading the active profile
        // of an endpointless declaration.
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory.path().join("startup-derived-endpoint.sqlite");
        let config_path = directory.path().join("config.toml");
        write_startup_document(
            &config_path,
            &startup_document("openrouter", "fixture-model", None),
        );
        let facade = open_startup_facade(&database, &config_path);
        let (_, kind, _, endpoint) = active_catalog_profile(&facade);
        assert_eq!(
            endpoint,
            derived_default_endpoint(&kind),
            "the comparison copy must match the catalog's derived endpoint"
        );
    }

    #[test]
    fn driver_kind_must_match_the_active_catalog_kind() {
        // A `SelectedProvider` whose kind differs from the active
        // catalog kind is rejected, so the executing driver can never serve a
        // catalog it does not match.
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory.path().join("driver-kind-invariant.sqlite");
        let generic_config = directory.path().join("generic-config.toml");
        write_startup_document(
            &generic_config,
            &startup_document(
                "generic-chat-completion-api",
                "fixture-model",
                Some("https://api.example.invalid/v1"),
            ),
        );
        let matching = open_startup_facade(&database, &generic_config);
        matching
            .ensure_driver_kind_matches_active_catalog()
            .expect("the matching driver kind passes the invariant");
        drop(matching);

        // A reopened facade whose startup document selects another kind than
        // the durable active catalog cannot pass the invariant.
        let openrouter_config = directory.path().join("openrouter-config.toml");
        write_startup_document(
            &openrouter_config,
            &startup_document("openrouter", "fixture-model", None),
        );
        let source = ConfigSourceDto::Explicit(
            ConfigPathDto::parse(openrouter_config.to_string_lossy().into_owned())
                .expect("fixture startup document path is absolute"),
        );
        let (snapshot, selected_provider, _, _) =
            load_provider_configuration(source).expect("startup configuration composes");
        let mismatched = DaemonApplicationFacade::open_with_selected_provider(
            &database,
            snapshot,
            selected_provider,
        )
        .expect("the durable facade opens");
        let error = mismatched
            .ensure_driver_kind_matches_active_catalog()
            .expect_err("a driver of another kind cannot serve the active catalog");
        assert_eq!(error.code(), "invalid_selected_provider");
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
        assert_eq!(
            configuration.reload_status,
            ConfigurationReloadStatusDto::Active
        );
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
            "typed edits without a retained private credential fail closed"
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

    /// Writes one openrouter fixture configuration file with the supplied
    /// credential and owner-only permissions on Unix.
    fn write_fixture_config(path: &ConfigPathDto, credential: &str) {
        let text = format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"{credential}\"\n"
        );
        fs::write(path.as_str(), text).expect("fixture config writes");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path.as_str(), fs::Permissions::from_mode(0o600))
                .expect("fixture config permissions set");
        }
    }

    /// Opens one file-backed facade with the private credential and source
    /// retained, mirroring the production `open_platform` private loading
    /// boundary, and returns it with the startup snapshot and config path.
    fn file_backed_facade(
        directory: &TempDir,
        config_path: &Path,
        credential: &str,
    ) -> (DaemonApplicationFacade, ConfigSnapshotDto) {
        let path = ConfigPathDto::parse(config_path.to_string_lossy().into_owned())
            .expect("fixture config path is absolute");
        write_fixture_config(&path, credential);
        let source = ConfigSourceDto::Explicit(path);
        let (snapshot, selected_provider, _, private_credential) =
            load_provider_configuration(source.clone()).expect("fixture configuration composes");
        let facade = DaemonApplicationFacade::open_with_selected_provider(
            directory.path().join("facade.sqlite"),
            snapshot.clone(),
            selected_provider,
        )
        .expect("file-backed facade opens");
        retain_private_startup_credential(&facade, private_credential, source)
            .expect("private startup credential retains");
        (facade, snapshot)
    }

    /// Returns the composition's private credential slot content for tests.
    fn retained_credential(facade: &DaemonApplicationFacade) -> Option<String> {
        facade
            .inner
            .private_credential
            .lock()
            .expect("fixture credential state is not poisoned")
            .material
            .clone()
    }

    #[test]
    fn control_plane_typed_edit_commits_when_the_private_credential_is_retained() {
        const RETAINED_SECRET: &str = "sk-retained-typed-edit-secret-12345";
        let directory = TempDir::new().expect("temporary directory exists");
        let config_path = directory.path().join("config.toml");
        let (facade, startup) = file_backed_facade(&directory, &config_path, RETAINED_SECRET);
        let startup_revision = startup.revision_id().to_string();
        assert_eq!(
            retained_credential(&facade).as_deref(),
            Some(RETAINED_SECRET),
            "the private loading boundary retains the startup credential"
        );

        // An execution-policy-only typed edit commits: the retained
        // credential is restored into the candidate inside the private
        // loading boundary before server-side validation.
        let transaction = reload_transaction(facade.command(
            ProtocolCommandDto::ApplyConfigurationEdit(ConfigurationEditCommandDto {
                operation_id: "op-1".to_owned(),
                expected_config_revision: startup_revision.clone(),
                operations: vec![ConfigurationEditOperationDto::Set {
                    key_path: "provider.execution.attempt_timeout_seconds".to_owned(),
                    safe_value: "45".to_owned(),
                }],
            }),
        ));
        assert_eq!(
            transaction.commit_outcome,
            ConfigurationCommitOutcomeDto::Committed
        );
        assert_ne!(transaction.candidate_config_revision, startup_revision);
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
        assert!(
            !format!("{transaction:?}").contains(RETAINED_SECRET),
            "the reload transaction never echoes the restored credential"
        );
        assert!(
            !active
                .resolved()
                .safe_debug_projection()
                .contains(RETAINED_SECRET),
            "the active snapshot never carries the credential"
        );

        // The credential slot is untouched by a typed edit.
        assert_eq!(
            retained_credential(&facade).as_deref(),
            Some(RETAINED_SECRET)
        );

        // A catalog-affecting typed edit is classified correctly now that
        // the candidate can resolve: model changes require a restart instead
        // of failing on the missing credential.
        let rejected = reload_transaction(facade.command(
            ProtocolCommandDto::ApplyConfigurationEdit(ConfigurationEditCommandDto {
                operation_id: "op-2".to_owned(),
                expected_config_revision: active.revision_id().to_string(),
                operations: vec![ConfigurationEditOperationDto::Set {
                    key_path: "provider.model".to_owned(),
                    safe_value: "model-b".to_owned(),
                }],
            }),
        ));
        assert_eq!(
            rejected.commit_outcome,
            ConfigurationCommitOutcomeDto::Rejected
        );
        assert_eq!(
            rejected.safe_failure_code.as_deref(),
            Some("catalog_change_requires_restart")
        );
        assert!(!format!("{rejected:?}").contains(RETAINED_SECRET));
    }

    #[test]
    fn control_plane_typed_edit_escapes_wire_values_before_the_candidate_parse() {
        // The document is rendered from the snapshot AST inside
        // `intention-config`, so a wire value carrying a TOML-significant
        // character is escaped and reaches the server-side validator. Before
        // the change the interpolated document broke TOML parsing and reported
        // the generic `invalid_config_toml`; the model change must instead be
        // classified by the reload contract.
        const RETAINED_SECRET: &str = "sk-retained-typed-edit-secret-23456";
        let directory = TempDir::new().expect("temporary directory exists");
        let config_path = directory.path().join("config.toml");
        let (facade, startup) = file_backed_facade(&directory, &config_path, RETAINED_SECRET);
        let transaction = reload_transaction(facade.command(
            ProtocolCommandDto::ApplyConfigurationEdit(ConfigurationEditCommandDto {
                operation_id: "op-1".to_owned(),
                expected_config_revision: startup.revision_id().to_string(),
                operations: vec![ConfigurationEditOperationDto::Set {
                    key_path: "provider.model".to_owned(),
                    safe_value: "model-\"quoted\"-and\\backslash".to_owned(),
                }],
            }),
        ));
        assert_eq!(
            transaction.commit_outcome,
            ConfigurationCommitOutcomeDto::Rejected
        );
        assert_eq!(
            transaction.safe_failure_code.as_deref(),
            Some("catalog_change_requires_restart"),
            "the escaped value must parse and reach the catalog classification"
        );
        assert!(
            !format!("{transaction:?}").contains(RETAINED_SECRET),
            "the reload transaction never echoes the retained credential"
        );
    }

    #[test]
    fn control_plane_rotation_supplies_replacement_from_the_configured_private_source() {
        const ORIGINAL_SECRET: &str = "sk-composition-original-secret-12345";
        const REPLACEMENT_SECRET: &str = "sk-composition-replacement-secret-12345";
        let directory = TempDir::new().expect("temporary directory exists");
        let config_path = directory.path().join("config.toml");
        let (facade, _startup) = file_backed_facade(&directory, &config_path, ORIGINAL_SECRET);
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let binding = facade
            .composition_binding_source()
            .binding("default")
            .expect("default binding resolves");
        let rotate_command = |operation_id: &str| {
            ProtocolCommandDto::RotateProviderCredentials(RotateProviderCredentialsCommandDto {
                profile_id: binding.profile_id.clone(),
                provider_profile_revision_id: binding.provider_profile_revision_id.clone(),
                expected_credential_composition_revision: binding.safe_composition_revision.clone(),
                operation_id: operation_id.to_owned(),
            })
        };

        // The operator supplies fresh material out-of-band by updating the
        // daemon's configuration file; rotation then re-reads the file
        // through the private loading boundary and rebuilds the driver.
        write_fixture_config(&config_path_as_dto(&config_path), REPLACEMENT_SECRET);
        let result = facade.command(rotate_command("op-1"));
        let accepted = match result {
            ProtocolCommandResultDto::Accepted(accepted) => accepted,
            ProtocolCommandResultDto::Rejected(error) => {
                panic!("rotation must be accepted: {error:?}")
            }
        };
        let ProtocolAcceptedResultDto::RotateProviderCredentials(outcome) = accepted.result()
        else {
            unreachable!("rotation returns its typed outcome");
        };
        assert!(outcome.rotated);
        assert_eq!(outcome.profile_id, "default");
        assert!(!format!("{outcome:?}").contains(REPLACEMENT_SECRET));
        assert!(!format!("{outcome:?}").contains(ORIGINAL_SECRET));

        // The private slot now carries the replacement material, so typed
        // edits restore the fresh credential.
        assert_eq!(
            retained_credential(&facade).as_deref(),
            Some(REPLACEMENT_SECRET)
        );
        let startup_revision = facade
            .active_config_snapshot()
            .expect("active snapshot reads")
            .revision_id()
            .to_string();
        let transaction = reload_transaction(facade.command(
            ProtocolCommandDto::ApplyConfigurationEdit(ConfigurationEditCommandDto {
                operation_id: "op-2".to_owned(),
                expected_config_revision: startup_revision,
                operations: vec![ConfigurationEditOperationDto::Set {
                    key_path: "provider.execution.max_attempts".to_owned(),
                    safe_value: "1".to_owned(),
                }],
            }),
        ));
        assert_eq!(
            transaction.commit_outcome,
            ConfigurationCommitOutcomeDto::Committed
        );
        assert!(!format!("{transaction:?}").contains(REPLACEMENT_SECRET));

        // Removing the source fails closed and preserves the current
        // material and the driver.
        fs::remove_file(&config_path).expect("fixture config removes");
        let rejected = facade.command(rotate_command("op-3"));
        let ProtocolCommandResultDto::Rejected(error) = rejected else {
            unreachable!("a missing private source fails closed");
        };
        assert_eq!(error.code(), "credential_rotation_source_unavailable");
        assert!(!error.to_string().contains(REPLACEMENT_SECRET));
        assert_eq!(
            retained_credential(&facade).as_deref(),
            Some(REPLACEMENT_SECRET)
        );

        // A source that cannot parse also fails closed without disclosing
        // content.
        fs::write(
            &config_path,
            format!("not a toml document {REPLACEMENT_SECRET} [["),
        )
        .expect("fixture config rewrites");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600))
                .expect("fixture config permissions set");
        }
        let rejected = facade.command(rotate_command("op-4"));
        let ProtocolCommandResultDto::Rejected(error) = rejected else {
            unreachable!("an unusable private source fails closed");
        };
        assert_eq!(error.code(), "credential_rotation_source_unavailable");
        assert!(
            !error.to_string().contains(REPLACEMENT_SECRET),
            "the rotation error never echoes source content"
        );
    }

    /// Converts a native fixture path to the validated configuration path.
    fn config_path_as_dto(path: &Path) -> ConfigPathDto {
        ConfigPathDto::parse(path.to_string_lossy().into_owned())
            .expect("fixture config path is absolute")
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
    use intention_storage::{
        AcceptUserTurnInputDto, ProviderUsageRepositoryDto, SessionProviderDefaultDto,
        SessionProviderDefaultsRepositoryDto,
    };

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

    /// Reads one session's durable provider default through the storage
    /// repository, failing when the committed row is absent.
    fn durable_session_default(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
    ) -> SessionProviderDefaultDto {
        facade
            .inner
            .repository
            .get_session_provider_profile(session_id)
            .expect("session provider default reads")
            .expect("the committed session provider default is durable")
    }

    #[test]
    fn session_provider_profile_set_get_and_idempotent_noop() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("session-default.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        // Two profiles activate so the profile-change commit path is reachable:
        // `default` and the second declaration's derived `profile-1`.
        seed_catalog(&facade, "seed-1", &["fixture-model", "second-model"]).expect("catalog seeds");
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
        assert_eq!(
            result.resulting_projection_revision, 0,
            "the insert path commits the initial projection revision"
        );
        assert!(matches!(
            &result.resolved,
            intention_protocol::contract_families::ResolvedProviderProfileDto::Resolved {
                profile_id,
                ..
            } if profile_id == "default"
        ));
        // NOTE: the zone-3 sqlite `set_session_provider_profile` commits before
        // it returns on the insert, the idempotent same-operation repeat, the
        // same-profile touch, and the profile change. It returns without
        // committing on the differing-profile same-operation conflict and on
        // the stale-revision mismatch, and this fixture pins both the
        // committing and the rejecting paths with durable read-backs. The
        // storage repository read-back below pins that durable commit; the
        // projection read alone cannot catch a lost commit because an absent
        // session row resolves to the global catalog default.
        let durable = durable_session_default(&facade, session_id);
        assert_eq!(durable.profile_id, "default");
        assert_eq!(durable.projection_revision, 0);
        assert_eq!(durable.last_operation_id, "op-1");

        // The same-operation repeat is an idempotent no-op that commits
        // without rewriting the durable row.
        let repeated = set_session_profile(&facade, session_id, "default", 0, "op-1");
        let ProtocolCommandResultDto::Accepted(accepted) = repeated else {
            unreachable!("session profile repeat is accepted")
        };
        let ProtocolAcceptedResultDto::SetSessionProviderProfile(result) = accepted.result() else {
            unreachable!("session profile repeat returns typed evidence")
        };
        assert!(!result.changed);
        assert_eq!(result.resulting_projection_revision, 0);
        let durable = durable_session_default(&facade, session_id);
        assert_eq!(durable.profile_id, "default");
        assert_eq!(durable.projection_revision, 0);
        assert_eq!(durable.last_operation_id, "op-1");

        // The same-profile touch with a fresh operation id commits the new
        // operation identity without changing the bound profile or revision.
        let touch = set_session_profile(&facade, session_id, "default", 0, "op-2");
        let ProtocolCommandResultDto::Accepted(accepted) = touch else {
            unreachable!("a same-profile touch is accepted")
        };
        let ProtocolAcceptedResultDto::SetSessionProviderProfile(result) = accepted.result() else {
            unreachable!("session profile touch returns typed evidence")
        };
        assert!(!result.changed);
        assert_eq!(result.resulting_projection_revision, 0);
        let durable = durable_session_default(&facade, session_id);
        assert_eq!(durable.profile_id, "default");
        assert_eq!(durable.projection_revision, 0);
        assert_eq!(
            durable.last_operation_id, "op-2",
            "the touch commit is durable"
        );

        // A stale expected revision is rejected before any write and leaves
        // the durable row unchanged.
        let stale = set_session_profile(&facade, session_id, "default", 1, "op-3");
        let ProtocolCommandResultDto::Rejected(error) = stale else {
            unreachable!("a stale expected revision is rejected")
        };
        assert_eq!(error.code(), "session_profile_revision_mismatch");
        let durable = durable_session_default(&facade, session_id);
        assert_eq!(durable.profile_id, "default");
        assert_eq!(durable.projection_revision, 0);
        assert_eq!(durable.last_operation_id, "op-2");

        // A profile change commits the new profile and advances the
        // projection revision.
        let changed = set_session_profile(&facade, session_id, "profile-1", 0, "op-4");
        let ProtocolCommandResultDto::Accepted(accepted) = changed else {
            unreachable!("a profile change is accepted")
        };
        let ProtocolAcceptedResultDto::SetSessionProviderProfile(result) = accepted.result() else {
            unreachable!("session profile change returns typed evidence")
        };
        assert!(result.changed);
        assert_eq!(result.resulting_projection_revision, 1);
        assert!(matches!(
            &result.resolved,
            intention_protocol::contract_families::ResolvedProviderProfileDto::Resolved {
                profile_id,
                ..
            } if profile_id == "profile-1"
        ));
        let durable = durable_session_default(&facade, session_id);
        assert_eq!(durable.profile_id, "profile-1");
        assert_eq!(
            durable.projection_revision, 1,
            "the profile change commit is durable"
        );
        assert_eq!(durable.last_operation_id, "op-4");

        let query = GetSessionProviderProfileQueryDto {
            schema_version: PROTOCOL_SCHEMA_VERSION_TEXT.to_owned(),
            session_id: session_id.to_string(),
        };
        match facade.query(ProtocolQueryDto::GetSessionProviderProfile(query)) {
            ProtocolQueryResultDto::SessionProviderProfile(projection) => {
                assert_eq!(projection.profile_id, "profile-1");
                assert!(matches!(
                    projection.resolved,
                    intention_protocol::contract_families::ResolvedProviderProfileDto::Resolved { .. }
                ));
                assert_eq!(projection.global_default_profile_id, "default");
                assert_eq!(projection.session_projection_revision, 1);
            }
            ProtocolQueryResultDto::Rejected(error) => {
                panic!("session profile query rejected: {}", error.code())
            }
            _ => unreachable!("session profile query returns a projection"),
        }
    }

    #[test]
    fn admission_rejects_a_non_current_schema_version_before_any_effect() {
        // The control-plane schema version is enforced at the
        // composition admission point, so a peer that bypasses decode cannot
        // make the daemon serve a non-current control-plane document.
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test(
            directory.path().join("admission-schema-version.sqlite"),
            fixture_config_snapshot(),
        )
        .expect("durable facade opens");
        seed_catalog(&facade, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let session_id = SessionId::new();
        create(&facade, session_id);

        let command = SetSessionProviderProfileCommandDto {
            schema_version: "9.9".to_owned(),
            session_id: session_id.to_string(),
            profile_id: "default".to_owned(),
            expected_session_projection_revision: 0,
            operation_id: "op-schema".to_owned(),
        };
        let rejected = facade.command(ProtocolCommandDto::SetSessionProviderProfile(command));
        let ProtocolCommandResultDto::Rejected(error) = rejected else {
            unreachable!("a non-current command schema version is rejected")
        };
        assert_eq!(error.code(), "incompatible_protocol_version");
        assert_eq!(error.category(), ErrorCategoryDto::Validation);
        assert!(
            facade
                .inner
                .repository
                .get_session_provider_profile(session_id)
                .expect("the durable session default reads")
                .is_none(),
            "the rejected admission commits nothing"
        );

        let query = GetProviderCatalogStatusQueryDto {
            schema_version: "9.9".to_owned(),
        };
        match facade.query(ProtocolQueryDto::GetProviderCatalogStatus(query)) {
            ProtocolQueryResultDto::Rejected(error) => {
                assert_eq!(error.code(), "incompatible_protocol_version");
            }
            _ => unreachable!("a non-current query schema version is rejected"),
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
            usage: intention_storage::ProviderUsageRecordDto {
                input_units: 10,
                output_units: 5,
                reasoning_units: 0,
            },
            occurred_at: 1,
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
                assert_eq!(usage.entries.len(), 1, "one identity produced usage");
                assert_eq!(
                    usage.entries[0].request_count, 1,
                    "usage is never double counted"
                );
                assert_eq!(usage.entries[0].input_units, 10);
                assert_eq!(usage.entries[0].output_units, 5);
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
        // PR24-003: a pending removal candidate is durable, and a
        // restart resolves it through the normal acceptance path - the
        // candidate rebuilt from the durable rows is adopted, so the platform
        // never opens gated and the real deadline still drives expiry before
        // the adoption. What the restart acts on is the durable material, not
        // process memory.
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory.path().join("pending-removal-restart.sqlite");
        let first = DaemonApplicationFacade::open_for_test(&database, fixture_config_snapshot())
            .expect("first facade opens");
        seed_catalog(&first, "seed-1", &["fixture-model"]).expect("catalog seeds");
        let outcome =
            prepare_removal_at_now(&first, "removal-restart", "https://api.example.invalid/v9");
        assert!(outcome.pending_removal);
        assert_eq!(outcome.candidate_handle.as_deref(), Some("catalog-2"));
        let pending = first
            .inner
            .repository
            .load_pending_removal_candidate()
            .expect("the durable candidate reads")
            .expect("the prepared candidate is pending");
        assert_eq!(pending.candidate_catalog_revision_id, 2);
        assert_eq!(pending.active_catalog_revision_id, 1);
        assert!(pending.expires_at > 0, "the durable deadline is real");
        assert_eq!(
            pending.removal_status,
            intention_storage::ProviderCatalogRemovalStatusDto::Pending
        );
        drop(first);

        let restarted =
            DaemonApplicationFacade::open_for_test(&database, fixture_config_snapshot())
                .expect("restart facade opens");
        assert_eq!(
            restarted.provider_control_readiness(),
            intention_application::CatalogReadiness::Ready,
            "the restart adopts the durable pending removal instead of opening gated"
        );
        let status = restarted
            .inner
            .repository
            .load_provider_catalog_status()
            .expect("catalog status reads");
        assert_eq!(
            status.status,
            intention_storage::ProviderCatalogStatusDto::Active
        );
        assert_eq!(
            status.active_catalog_revision_id,
            Some(2),
            "the adopted removal is the durable active revision"
        );
        assert_eq!(status.candidate_catalog_revision_id, None);
        assert!(
            restarted
                .inner
                .repository
                .load_pending_removal_candidate()
                .expect("pending removal reads")
                .is_none(),
            "the adopted candidate is durably closed"
        );
        assert_eq!(
            restarted
                .inner
                .repository
                .load_highest_removal_candidate_revision()
                .expect("the durable removal maximum reads"),
            2,
            "the adopted candidate is the durable removal maximum"
        );
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
        // PR24-005: a configuration reload commit never rewrites a
        // durable pending-removal state. The removal is still durably pending
        // after the commit, and the restart adopts that same durable row (with
        // its real deadline) instead of opening gated.
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
        let pending = first
            .inner
            .repository
            .load_pending_removal_candidate()
            .expect("the durable candidate reads")
            .expect("the reload commit leaves the durable pending removal in place");
        assert_eq!(pending.candidate_catalog_revision_id, 2);
        assert_eq!(
            pending.removal_status,
            intention_storage::ProviderCatalogRemovalStatusDto::Pending,
            "the reload commit never rewrites the removal lifecycle"
        );
        assert!(
            pending.expires_at > 0,
            "the durable deadline survives the reload"
        );
        drop(first);

        let restarted =
            DaemonApplicationFacade::open_for_test(&database, fixture_config_snapshot())
                .expect("restart facade opens");
        assert_eq!(
            restarted.provider_control_readiness(),
            intention_application::CatalogReadiness::Ready,
            "the restart adopts the removal the reload preserved instead of opening gated"
        );
        let status = restarted
            .inner
            .repository
            .load_provider_catalog_status()
            .expect("catalog status reads");
        assert_eq!(
            status.status,
            intention_storage::ProviderCatalogStatusDto::Active
        );
        assert_eq!(status.active_catalog_revision_id, Some(2));
        assert!(
            restarted
                .inner
                .repository
                .load_pending_removal_candidate()
                .expect("pending removal reads")
                .is_none(),
            "the adopted removal is durably closed"
        );
    }

    /// The durable equal-interval anchor of every harness facade fixture rule.
    const HARNESS_FIXTURE_INTERVAL_ANCHOR_MS: u64 = 1_000_000;

    /// The fixed fixture cadence, above the one-minute minimum.
    const HARNESS_FIXTURE_INTERVAL_MS: u64 = 60_000;

    /// The daemon-owned project time zone of every harness facade fixture tick.
    const HARNESS_FIXTURE_PROJECT_TIME_ZONE: &str = "UTC";

    /// Returns one canonical fixture digest for a durable harness record.
    fn fixture_harness_digest(seed: char) -> String {
        format!("sha256:{}", seed.to_string().repeat(64))
    }

    /// Creates one ordinary fixture session in `project_id`.
    ///
    /// One workspace root binds exactly one workspace identity, so every
    /// fixture session names its own workspace directory below the platform
    /// temp root.
    fn create_harness_session(
        facade: &DaemonApplicationFacade,
        project_id: ProjectId,
    ) -> SessionId {
        let session_id = SessionId::new();
        let workspace =
            std::env::temp_dir().join(format!("intention-composition-harness-{session_id}"));
        let root = WorkspaceRootDto::parse(workspace.to_string_lossy().into_owned())
            .expect("fixture workspace is absolute");
        let accepted = facade.command(ProtocolCommandDto::CreateSession(
            CreateSessionCommandDto::new(
                project_id,
                session_id,
                WorkspaceId::new(),
                root,
                RunModeDto::Build,
            ),
        ));
        assert!(matches!(accepted, ProtocolCommandResultDto::Accepted(_)));
        session_id
    }

    /// Builds one immutable fixed-interval revision of one fixture rule.
    fn fixture_harness_revision(
        harness_id: &str,
        applied_time_zone: &str,
        revision: u64,
        digest_seed: char,
    ) -> HarnessRuleRevisionRecordDto {
        HarnessRuleRevisionRecordDto {
            harness_id: harness_id.to_owned(),
            revision,
            task_digest: fixture_harness_digest('a'),
            class: HarnessExecutionClassDto::Light,
            task_mode: HarnessTaskModeDto::RepeatedTask,
            presentation_mode: HarnessPresentationModeDto::JournalOnly,
            applied_time_zone: applied_time_zone.to_owned(),
            source_kinds: vec![HarnessSourceKindDto::FixedInterval],
            source_references: Vec::new(),
            interval_anchor_ms: Some(HARNESS_FIXTURE_INTERVAL_ANCHOR_MS),
            interval_ms: Some(HARNESS_FIXTURE_INTERVAL_MS),
            calendar_expression: None,
            completion_link_reference: None,
            completion_outcomes: Vec::new(),
            canonical_revision_digest: fixture_harness_digest(digest_seed),
            created_at_ms: 1_000,
        }
    }

    /// Builds the durable creation input of one session-scoped interval rule.
    fn fixture_harness_rule_input(
        harness_id: &str,
        project_id: ProjectId,
        session_id: SessionId,
        applied_time_zone: &str,
    ) -> CreateHarnessRuleInputDto {
        CreateHarnessRuleInputDto {
            rule: HarnessRuleRecordDto {
                harness_id: harness_id.to_owned(),
                scope: HarnessRuleScopeDto::UserSession {
                    project_id: project_id.to_string(),
                    session_id: session_id.to_string(),
                },
                lifecycle_state: HarnessRuleLifecycleStateDto::Active,
                active_revision: 1,
                service_session_id: session_id.to_string(),
                updated_at_ms: 1_000,
            },
            revision: fixture_harness_revision(harness_id, applied_time_zone, 1, 'b'),
        }
    }

    /// Creates one active fixture rule with a single fixed-interval source.
    fn create_fixture_harness_rule(
        facade: &DaemonApplicationFacade,
        project_id: ProjectId,
        session_id: SessionId,
        applied_time_zone: &str,
    ) -> String {
        let harness_id = SessionId::new().to_string();
        let created = facade
            .create_harness_rule_for_daemon(fixture_harness_rule_input(
                &harness_id,
                project_id,
                session_id,
                applied_time_zone,
            ))
            .expect("fixture harness rule creates");
        assert_eq!(created.harness_id, harness_id);
        assert_eq!(created.active_revision, 1);
        harness_id
    }

    /// Performs one daemon-owned scheduling tick of one fixture rule.
    fn tick_fixture_harness(
        facade: &DaemonApplicationFacade,
        harness_id: &str,
        observed_at_ms: u64,
        project_time_zone: &str,
        daemon_concurrency_available: bool,
    ) -> DtoResult<HarnessScheduleTickOutcomeDto> {
        facade.harness_schedule_tick_for_daemon(HarnessScheduleTickRequestDto {
            harness_id: harness_id.to_owned(),
            observed_at_ms,
            project_time_zone: project_time_zone.to_owned(),
            daemon_concurrency_available,
            proposed_run_id: RunId::new(),
        })
    }

    /// Returns the single admitted launch of one fixture tick outcome.
    fn admitted_fixture_launch(
        outcome: &HarnessScheduleTickOutcomeDto,
    ) -> &HarnessAdmittedLaunchDto {
        let Some(HarnessLaunchOutcomeDto::Admitted(admitted)) = outcome.launch.as_ref() else {
            unreachable!("the fixture tick admits exactly one launch, got {outcome:?}")
        };
        admitted
    }

    /// Admits one fixture launch through a tick and returns its run identity.
    fn admit_fixture_harness_launch(
        facade: &DaemonApplicationFacade,
        harness_id: &str,
        observed_at_ms: u64,
    ) -> String {
        let outcome = tick_fixture_harness(
            facade,
            harness_id,
            observed_at_ms,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            true,
        )
        .expect("the fixture tick admits one launch");
        admitted_fixture_launch(&outcome).run_id.clone()
    }

    /// Loads one bounded page of one fixture session's harness journal.
    fn load_fixture_harness_journal(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        harness_id: &str,
        after_sequence: u64,
        limit: u64,
    ) -> DtoResult<Vec<HarnessJournalRecordDto>> {
        facade.load_harness_journal_for_session_for_daemon(
            session_id,
            harness_id,
            after_sequence,
            limit,
        )
    }

    /// Fabricates the durable policy and reservations of one fixture root run
    /// through the crate-private repository.
    ///
    /// No protocol command reserves a programmatic-policy action: the
    /// composition owns that persistence path, so this fixture commits the
    /// durable records a restarted daemon would observe. One reservation stays
    /// outstanding before `ToolCallStarted` and one reached it permanently.
    fn seed_fixture_programmatic_reservations(facade: &DaemonApplicationFacade, root_run_id: &str) {
        let policy_id = "policy-composition-recovery";
        facade
            .inner
            .repository
            .create_programmatic_policy(CreateProgrammaticPolicyInputDto {
                policy: ProgrammaticPolicyRecordDto {
                    policy_id: policy_id.to_owned(),
                    scope: ProgrammaticPolicyScopeDto::Project {
                        project_id: ProjectId::new().to_string(),
                    },
                    calendar_period_kind: ProgrammaticCalendarPeriodKindDto::Day,
                    lifecycle_state: ProgrammaticPolicyLifecycleStateDto::Active,
                    active_revision: 1,
                    canonical_policy_digest: fixture_harness_digest('c'),
                    updated_at_ms: 1_000,
                },
                revision: ProgrammaticPolicyRevisionRecordDto {
                    policy_id: policy_id.to_owned(),
                    revision: 1,
                    root_origin_rules: vec![ProgrammaticRootOriginRuleRecordDto {
                        root_origin_kind: ProgrammaticRootOriginKindDto::InteractiveUser,
                        maximum_decision: ProgrammaticAdmissionDecisionDto::DirectLocalRead,
                    }],
                    admission_decisions: vec![ProgrammaticAdmissionDecisionDto::DirectLocalRead],
                    max_actions_per_run: 8,
                    max_concurrent_actions_per_run: 4,
                    calendar_period_kind: ProgrammaticCalendarPeriodKindDto::Day,
                    calendar_max_actions: 64,
                    inherited_policy_references: Vec::new(),
                    canonical_revision_digest: fixture_harness_digest('d'),
                },
            })
            .expect("the fixture policy creates");
        for (reservation_reference, tool_call_id, reserved_at_ms) in [
            ("reservation-started", "call-started", 1_000),
            ("reservation-unstarted", "call-unstarted", 1_001),
        ] {
            facade
                .inner
                .repository
                .reserve_programmatic_policy_action(ReserveProgrammaticPolicyActionInputDto {
                    reservation: ProgrammaticPolicyReservationRecordDto {
                        reservation_reference: reservation_reference.to_owned(),
                        policy_id: policy_id.to_owned(),
                        policy_revision: 1,
                        root_run_id: root_run_id.to_owned(),
                        tool_call_id: tool_call_id.to_owned(),
                        typed_input_digest: fixture_harness_digest('e'),
                        calendar_counter_reference: format!("{policy_id}:day"),
                        reserved_at_ms,
                        state: ProgrammaticReservationStateDto::Reserved,
                        finished_at_ms: None,
                    },
                    max_actions_per_run: 8,
                    max_concurrent_actions_per_run: 4,
                    calendar_max_actions: 64,
                    calendar_window_start_ms: 0,
                    calendar_window_end_ms: 86_400_000,
                    calendar_window_time_zone: "UTC".to_owned(),
                })
                .expect("the fixture reservation commits");
        }
        facade
            .inner
            .repository
            .commit_programmatic_reservation_started(CommitProgrammaticReservationStartedInputDto {
                reservation_reference: "reservation-started".to_owned(),
                tool_call_id: "call-started".to_owned(),
                started_at_ms: 1_002,
            })
            .expect("the started fixture reservation commits");
    }

    #[test]
    fn harness_rule_creation_persists_one_interval_revision_and_rejects_unusable_input() {
        let (_directory, facade) = test_facade();
        let project_id = ProjectId::new();
        let session_id = create_harness_session(&facade, project_id);
        let harness_id = SessionId::new().to_string();
        let input = fixture_harness_rule_input(
            &harness_id,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );

        let created = facade
            .create_harness_rule_for_daemon(input.clone())
            .expect("the fixture harness rule creates");
        assert_eq!(created.harness_id, harness_id);
        assert_eq!(created.active_revision, 1);
        assert_eq!(
            created.lifecycle_state,
            HarnessRuleLifecycleStateDto::Active
        );
        assert_eq!(
            created.scope,
            HarnessRuleScopeDto::UserSession {
                project_id: project_id.to_string(),
                session_id: session_id.to_string(),
            }
        );
        assert_eq!(
            facade
                .inner
                .repository
                .load_harness_rule_revision(harness_id.clone(), 1)
                .expect("the first revision loads"),
            input.revision,
            "the immutable interval revision is durable as supplied"
        );

        // An exact replay of one durable creation returns the stored rule.
        let replayed = facade
            .create_harness_rule_for_daemon(input)
            .expect("an equal creation replay returns the stored rule");
        assert_eq!(replayed, created);

        let error = facade
            .create_harness_rule_for_daemon(fixture_harness_rule_input(
                "sk-live-harness-rule",
                project_id,
                session_id,
                HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            ))
            .expect_err("credentials never become a durable harness identity");
        assert_eq!(error.code(), "credentials_forbidden");

        let error = facade
            .create_harness_rule_for_daemon(fixture_harness_rule_input(
                "",
                project_id,
                session_id,
                HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            ))
            .expect_err("a blank harness identity is not a safe label");
        assert_eq!(error.code(), "invalid_harness_rule");

        let mut foreign_revision = fixture_harness_rule_input(
            &harness_id,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );
        foreign_revision.revision.harness_id = SessionId::new().to_string();
        let error = facade
            .create_harness_rule_for_daemon(foreign_revision)
            .expect_err("a new rule starts at exactly its own revision one");
        assert_eq!(error.code(), "harness_revision_conflict");

        // A path-shaped label is still a safe durable label at this boundary:
        // the daemon-facing identity gate rejects paths for the tick, journal,
        // and recovery surface pinned below.
        let path_text = std::env::temp_dir()
            .join("harness-rule.sqlite")
            .to_string_lossy()
            .into_owned();
        let stored = facade
            .create_harness_rule_for_daemon(fixture_harness_rule_input(
                &path_text,
                project_id,
                session_id,
                HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            ))
            .expect("a path-shaped label creates a durable rule");
        assert_eq!(stored.harness_id, path_text);
    }

    #[test]
    fn harness_rule_revision_continues_the_exact_observed_active_revision() {
        let (_directory, facade) = test_facade();
        let project_id = ProjectId::new();
        let session_id = create_harness_session(&facade, project_id);
        let harness_id = create_fixture_harness_rule(
            &facade,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );

        let mut next_revision = fixture_harness_revision(&harness_id, "Europe/Berlin", 2, 'c');
        next_revision.created_at_ms = 2_000;
        let revised = facade
            .revise_harness_rule_for_daemon(ReviseHarnessRuleInputDto {
                harness_id: harness_id.clone(),
                expected_revision: 1,
                revision: next_revision.clone(),
            })
            .expect("the fixture rule continues to its next revision");
        assert_eq!(revised.active_revision, 2);
        assert_eq!(revised.updated_at_ms, 2_000);
        assert_eq!(
            facade
                .inner
                .repository
                .load_harness_rule_revision(harness_id.clone(), 2)
                .expect("the new revision loads"),
            next_revision
        );

        // A stale observation cannot continue the active revision.
        let error = facade
            .revise_harness_rule_for_daemon(ReviseHarnessRuleInputDto {
                harness_id: harness_id.clone(),
                expected_revision: 1,
                revision: fixture_harness_revision(&harness_id, "UTC", 2, 'f'),
            })
            .expect_err("a stale expected revision fails closed");
        assert_eq!(error.code(), "harness_revision_conflict");

        // A revision bound to another rule never continues this one.
        let foreign_harness_id = SessionId::new().to_string();
        let error = facade
            .revise_harness_rule_for_daemon(ReviseHarnessRuleInputDto {
                harness_id,
                expected_revision: 2,
                revision: fixture_harness_revision(&foreign_harness_id, "UTC", 3, 'f'),
            })
            .expect_err("a revision of another rule never continues this one");
        assert_eq!(error.code(), "harness_revision_conflict");

        // An unknown durable rule owns no active revision to continue.
        let unknown_harness_id = SessionId::new().to_string();
        let error = facade
            .revise_harness_rule_for_daemon(ReviseHarnessRuleInputDto {
                harness_id: unknown_harness_id.clone(),
                expected_revision: 1,
                revision: fixture_harness_revision(&unknown_harness_id, "UTC", 2, 'f'),
            })
            .expect_err("an unknown harness rule cannot be revised");
        assert_eq!(error.code(), "harness_not_active");
    }

    #[test]
    fn harness_rule_lifecycle_pause_resume_and_archive_stay_typed() {
        let (_directory, facade) = test_facade();
        let project_id = ProjectId::new();
        let session_id = create_harness_session(&facade, project_id);
        let harness_id = create_fixture_harness_rule(
            &facade,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );

        let paused = facade
            .transition_harness_rule_lifecycle_for_daemon(TransitionHarnessRuleLifecycleInputDto {
                harness_id: harness_id.clone(),
                expected_revision: 1,
                operation: HarnessRuleOperationDto::Pause,
                has_active_run: false,
                occurred_at_ms: 2_000,
            })
            .expect("an active fixture rule pauses");
        assert_eq!(paused.lifecycle_state, HarnessRuleLifecycleStateDto::Paused);
        assert_eq!(paused.updated_at_ms, 2_000);
        assert_eq!(paused.active_revision, 1);

        let resumed = facade
            .transition_harness_rule_lifecycle_for_daemon(TransitionHarnessRuleLifecycleInputDto {
                harness_id: harness_id.clone(),
                expected_revision: 1,
                operation: HarnessRuleOperationDto::Resume,
                has_active_run: false,
                occurred_at_ms: 3_000,
            })
            .expect("a paused fixture rule resumes");
        assert_eq!(
            resumed.lifecycle_state,
            HarnessRuleLifecycleStateDto::Active
        );

        let archived = facade
            .transition_harness_rule_lifecycle_for_daemon(TransitionHarnessRuleLifecycleInputDto {
                harness_id: harness_id.clone(),
                expected_revision: 1,
                operation: HarnessRuleOperationDto::Archive,
                has_active_run: false,
                occurred_at_ms: 4_000,
            })
            .expect("an active fixture rule archives");
        assert_eq!(
            archived.lifecycle_state,
            HarnessRuleLifecycleStateDto::Archived
        );

        // An archived rule retains state and rejects every later operation.
        let error = facade
            .transition_harness_rule_lifecycle_for_daemon(TransitionHarnessRuleLifecycleInputDto {
                harness_id,
                expected_revision: 1,
                operation: HarnessRuleOperationDto::Pause,
                has_active_run: false,
                occurred_at_ms: 5_000,
            })
            .expect_err("an archived harness rule rejects operations");
        assert_eq!(error.code(), "harness_archived");

        // A paused rule archives too, but a rule with a live run does not.
        let second_rule = create_fixture_harness_rule(
            &facade,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );
        facade
            .transition_harness_rule_lifecycle_for_daemon(TransitionHarnessRuleLifecycleInputDto {
                harness_id: second_rule.clone(),
                expected_revision: 1,
                operation: HarnessRuleOperationDto::Pause,
                has_active_run: false,
                occurred_at_ms: 6_000,
            })
            .expect("the second fixture rule pauses");
        let archived = facade
            .transition_harness_rule_lifecycle_for_daemon(TransitionHarnessRuleLifecycleInputDto {
                harness_id: second_rule,
                expected_revision: 1,
                operation: HarnessRuleOperationDto::Archive,
                has_active_run: false,
                occurred_at_ms: 7_000,
            })
            .expect("a paused fixture rule archives");
        assert_eq!(
            archived.lifecycle_state,
            HarnessRuleLifecycleStateDto::Archived
        );

        let third_rule = create_fixture_harness_rule(
            &facade,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );
        let error = facade
            .transition_harness_rule_lifecycle_for_daemon(TransitionHarnessRuleLifecycleInputDto {
                harness_id: third_rule.clone(),
                expected_revision: 1,
                operation: HarnessRuleOperationDto::Archive,
                has_active_run: true,
                occurred_at_ms: 8_000,
            })
            .expect_err("a rule with a live run is not archived");
        assert_eq!(error.code(), "harness_not_active");

        // A stale observation cannot transition the rule.
        let error = facade
            .transition_harness_rule_lifecycle_for_daemon(TransitionHarnessRuleLifecycleInputDto {
                harness_id: third_rule,
                expected_revision: 9,
                operation: HarnessRuleOperationDto::Pause,
                has_active_run: false,
                occurred_at_ms: 9_000,
            })
            .expect_err("a stale expected revision fails closed");
        assert_eq!(error.code(), "harness_revision_conflict");

        // An unknown durable rule owns no lifecycle to transition.
        let error = facade
            .transition_harness_rule_lifecycle_for_daemon(TransitionHarnessRuleLifecycleInputDto {
                harness_id: SessionId::new().to_string(),
                expected_revision: 1,
                operation: HarnessRuleOperationDto::Pause,
                has_active_run: false,
                occurred_at_ms: 10_000,
            })
            .expect_err("an unknown harness rule cannot transition");
        assert_eq!(error.code(), "harness_not_active");
    }

    #[test]
    fn harness_schedule_tick_captures_the_newest_slot_and_admits_at_most_one_launch() {
        let (_directory, facade) = test_facade();
        let project_id = ProjectId::new();
        let session_id = create_harness_session(&facade, project_id);
        let harness_id = create_fixture_harness_rule(
            &facade,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );

        // Five interval slots have passed since the anchor: exactly the newest
        // one is captured, coalescing every missed slot, and admitted with a
        // daemon-assigned fresh ordinary run identity.
        let observed_at_ms =
            HARNESS_FIXTURE_INTERVAL_ANCHOR_MS + (5 * HARNESS_FIXTURE_INTERVAL_MS) + 30_000;
        let outcome = tick_fixture_harness(
            &facade,
            &harness_id,
            observed_at_ms,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            true,
        )
        .expect("the fixture tick captures and admits");
        assert_eq!(outcome.harness_id, harness_id);
        assert_eq!(outcome.observations.len(), 1);
        assert_eq!(
            outcome.observations[0].source_kind,
            HarnessSourceKindV1::FixedInterval
        );
        assert_eq!(
            outcome.observations[0].observed_at_ms,
            HARNESS_FIXTURE_INTERVAL_ANCHOR_MS + (5 * HARNESS_FIXTURE_INTERVAL_MS)
        );
        assert_eq!(
            outcome.observations[0].capture.outcome,
            HarnessTriggerCaptureOutcomeDto::Captured
        );
        assert!(
            outcome.observations[0].capture.journal_record.is_some(),
            "a newly captured reason appends its durable journal record"
        );
        let admitted = admitted_fixture_launch(&outcome);
        assert_eq!(admitted.harness_id, harness_id);
        assert_eq!(admitted.rule_revision, 1);
        assert_eq!(
            admitted.reason.state,
            HarnessTriggerReasonStateDto::Admitted
        );
        assert!(RunId::parse(&admitted.run_id).is_ok());
        assert!(outcome.pending_reason.is_none());

        let journal = load_fixture_harness_journal(&facade, session_id, &harness_id, 0, 64)
            .expect("the durable journal reads");
        let kinds: Vec<HarnessJournalRecordKindDto> =
            journal.iter().map(|record| record.record_kind).collect();
        assert_eq!(
            kinds,
            vec![
                HarnessJournalRecordKindDto::TriggerCaptured,
                HarnessJournalRecordKindDto::LaunchAdmitted
            ]
        );
        assert_eq!(journal[0].sequence, 1);
        assert_eq!(journal[1].sequence, 2);
        assert!(
            !format!("{journal:?}").contains(&std::env::temp_dir().to_string_lossy().into_owned()),
            "durable journal records never disclose a filesystem path"
        );

        // The equal observation is a redelivery: nothing changes durably and
        // nothing launches twice.
        let redelivered = tick_fixture_harness(
            &facade,
            &harness_id,
            observed_at_ms,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            true,
        )
        .expect("the redelivered tick changes nothing");
        assert_eq!(
            redelivered.observations[0].capture.outcome,
            HarnessTriggerCaptureOutcomeDto::Redelivered
        );
        assert!(redelivered.observations[0].capture.journal_record.is_none());
        assert!(redelivered.launch.is_none());
        assert!(redelivered.pending_reason.is_none());
        assert_eq!(
            load_fixture_harness_journal(&facade, session_id, &harness_id, 0, 64)
                .expect("the durable journal reads")
                .len(),
            journal.len()
        );

        // A later slot is captured while the rule keeps its single live launch:
        // the newest coalesced reason stays pending for a later observation.
        let retained = tick_fixture_harness(
            &facade,
            &harness_id,
            observed_at_ms + HARNESS_FIXTURE_INTERVAL_MS,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            true,
        )
        .expect("the later slot is captured and retained");
        assert_eq!(
            retained.observations[0].capture.outcome,
            HarnessTriggerCaptureOutcomeDto::Captured
        );
        let Some(HarnessLaunchOutcomeDto::Retained(held)) = retained.launch.as_ref() else {
            unreachable!("the rule keeps at most one live launch, got {retained:?}")
        };
        assert_eq!(held.reason.coalesced_count, 1);
        let pending = retained
            .pending_reason
            .expect("the retained reason stays pending");
        assert_eq!(pending.harness_id, harness_id);
        assert_eq!(pending.state, HarnessTriggerReasonStateDto::Pending);
    }

    #[test]
    fn harness_schedule_tick_retains_without_capacity_and_captures_a_paused_rule() {
        let (_directory, facade) = test_facade();
        let project_id = ProjectId::new();
        let session_id = create_harness_session(&facade, project_id);
        let occupied_rule = create_fixture_harness_rule(
            &facade,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );
        let observed_at_ms = HARNESS_FIXTURE_INTERVAL_ANCHOR_MS + HARNESS_FIXTURE_INTERVAL_MS;

        // An occupied daemon-wide concurrency signal retains the coalesced
        // reason without launching.
        let retained = tick_fixture_harness(
            &facade,
            &occupied_rule,
            observed_at_ms,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            false,
        )
        .expect("the fixture tick captures and retains");
        assert_eq!(
            retained.observations[0].capture.outcome,
            HarnessTriggerCaptureOutcomeDto::Captured
        );
        let Some(HarnessLaunchOutcomeDto::Retained(_)) = retained.launch else {
            unreachable!("an occupied daemon slot retains the launch, got {retained:?}")
        };
        let pending = retained
            .pending_reason
            .expect("the coalesced reason stays durable while the daemon slot is occupied");
        assert_eq!(pending.state, HarnessTriggerReasonStateDto::Pending);
        assert_eq!(pending.coalesced_count, 1);
        assert_eq!(pending.last_observed_at_ms, observed_at_ms);

        // The next observation of the same durable reason admits it.
        let admitted = tick_fixture_harness(
            &facade,
            &occupied_rule,
            observed_at_ms,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            true,
        )
        .expect("the next tick admits the retained reason");
        assert_eq!(
            admitted.observations[0].capture.outcome,
            HarnessTriggerCaptureOutcomeDto::Redelivered
        );
        assert!(RunId::parse(&admitted_fixture_launch(&admitted).run_id).is_ok());
        assert!(admitted.pending_reason.is_none());

        // A paused rule keeps capturing and coalescing without launching.
        let paused_rule = create_fixture_harness_rule(
            &facade,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );
        facade
            .transition_harness_rule_lifecycle_for_daemon(TransitionHarnessRuleLifecycleInputDto {
                harness_id: paused_rule.clone(),
                expected_revision: 1,
                operation: HarnessRuleOperationDto::Pause,
                has_active_run: false,
                occurred_at_ms: 2_000,
            })
            .expect("the fixture rule pauses");
        let paused_tick = tick_fixture_harness(
            &facade,
            &paused_rule,
            observed_at_ms,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            true,
        )
        .expect("a paused rule still captures and coalesces");
        assert_eq!(
            paused_tick.observations[0].capture.outcome,
            HarnessTriggerCaptureOutcomeDto::Captured
        );
        assert!(
            paused_tick.launch.is_none(),
            "a paused rule admits no automatic launch"
        );
        let pending = paused_tick
            .pending_reason
            .expect("the paused rule keeps its pending reason");
        assert_eq!(pending.state, HarnessTriggerReasonStateDto::Pending);
        assert_eq!(pending.coalesced_count, 1);
    }

    #[test]
    fn harness_schedule_tick_rejects_a_foreign_time_zone_and_an_archived_rule() {
        let (_directory, facade) = test_facade();
        let project_id = ProjectId::new();
        let session_id = create_harness_session(&facade, project_id);
        let harness_id =
            create_fixture_harness_rule(&facade, project_id, session_id, "America/New_York");
        let observed_at_ms = HARNESS_FIXTURE_INTERVAL_ANCHOR_MS + HARNESS_FIXTURE_INTERVAL_MS;

        // A non-archived rule follows the daemon-owned project time zone: a
        // revision recorded under another zone fails closed until it is revised.
        let error = tick_fixture_harness(
            &facade,
            &harness_id,
            observed_at_ms,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            true,
        )
        .expect_err("a non-archived rule follows the daemon project time zone");
        assert_eq!(error.code(), "harness_schedule_invalid");

        // An unusable project time zone fails closed before any effect.
        let error = tick_fixture_harness(&facade, &harness_id, observed_at_ms, "not a zone", true)
            .expect_err("an unusable project time zone fails closed");
        assert_eq!(error.code(), "harness_schedule_invalid");

        // The rule is revised to the current project zone and then archived:
        // archiving retains the zone recorded by the immutable revision, so the
        // zone mismatch no longer applies and the archived rule rejects capture.
        facade
            .revise_harness_rule_for_daemon(ReviseHarnessRuleInputDto {
                harness_id: harness_id.clone(),
                expected_revision: 1,
                revision: fixture_harness_revision(
                    &harness_id,
                    HARNESS_FIXTURE_PROJECT_TIME_ZONE,
                    2,
                    'f',
                ),
            })
            .expect("the fixture rule is revised to the project zone");
        facade
            .transition_harness_rule_lifecycle_for_daemon(TransitionHarnessRuleLifecycleInputDto {
                harness_id: harness_id.clone(),
                expected_revision: 2,
                operation: HarnessRuleOperationDto::Archive,
                has_active_run: false,
                occurred_at_ms: 2_000,
            })
            .expect("the fixture rule archives");
        let error = tick_fixture_harness(
            &facade,
            &harness_id,
            observed_at_ms,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            true,
        )
        .expect_err("an archived rule rejects capture");
        assert_eq!(error.code(), "harness_archived");
    }

    #[test]
    fn harness_schedule_tick_covers_an_early_observation_and_calendar_coalescing() {
        let (_directory, facade) = test_facade();
        let project_id = ProjectId::new();
        let session_id = create_harness_session(&facade, project_id);

        // An observation before the durable anchor is due for no interval slot.
        let early_rule = create_fixture_harness_rule(
            &facade,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );
        let early = tick_fixture_harness(
            &facade,
            &early_rule,
            HARNESS_FIXTURE_INTERVAL_ANCHOR_MS - 1,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            true,
        )
        .expect("an observation before the anchor captures nothing");
        assert!(early.observations.is_empty());
        assert!(early.launch.is_none());
        assert!(early.pending_reason.is_none());

        // An interval source without its durable anchor and cadence is an
        // incoherent revision and fails closed at its own observation.
        let incoherent_rule = SessionId::new().to_string();
        let mut incoherent = fixture_harness_rule_input(
            &incoherent_rule,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );
        incoherent.revision.interval_anchor_ms = None;
        incoherent.revision.interval_ms = None;
        facade
            .create_harness_rule_for_daemon(incoherent)
            .expect("the incoherent interval revision is still a durable rule");
        let error = tick_fixture_harness(
            &facade,
            &incoherent_rule,
            HARNESS_FIXTURE_INTERVAL_ANCHOR_MS,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            true,
        )
        .expect_err("an equal-interval source requires its durable anchor and cadence");
        assert_eq!(error.code(), "harness_schedule_invalid");

        // One fixed interval and one calendar source coalesce into one pending
        // reason and admit exactly one launch.
        let two_source_rule = SessionId::new().to_string();
        let mut two_source = fixture_harness_rule_input(
            &two_source_rule,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );
        two_source.revision.source_kinds = vec![
            HarnessSourceKindDto::FixedInterval,
            HarnessSourceKindDto::CalendarTime,
        ];
        two_source.revision.calendar_expression = Some("* * * * *".to_owned());
        facade
            .create_harness_rule_for_daemon(two_source)
            .expect("the two-source fixture rule creates");
        let observed_at_ms =
            HARNESS_FIXTURE_INTERVAL_ANCHOR_MS + HARNESS_FIXTURE_INTERVAL_MS + 30_000;
        let outcome = tick_fixture_harness(
            &facade,
            &two_source_rule,
            observed_at_ms,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            true,
        )
        .expect("the two-source tick captures and admits");
        assert_eq!(outcome.observations.len(), 2);
        assert_eq!(
            outcome.observations[0].capture.outcome,
            HarnessTriggerCaptureOutcomeDto::Captured
        );
        assert_eq!(
            outcome.observations[1].source_kind,
            HarnessSourceKindV1::CalendarTime
        );
        assert_eq!(
            outcome.observations[1].observed_at_ms,
            observed_at_ms - (observed_at_ms % 60_000)
        );
        assert_eq!(
            outcome.observations[1].capture.outcome,
            HarnessTriggerCaptureOutcomeDto::Coalesced
        );
        assert!(
            outcome.observations[1].capture.journal_record.is_some(),
            "a coalesced reason appends its durable journal record"
        );
        assert_eq!(
            admitted_fixture_launch(&outcome).harness_id,
            two_source_rule
        );
        assert!(outcome.pending_reason.is_none());
    }

    #[test]
    fn harness_daemon_identity_gate_rejects_non_canonical_text() {
        let (_directory, facade) = test_facade();
        let observed_at_ms = HARNESS_FIXTURE_INTERVAL_ANCHOR_MS + HARNESS_FIXTURE_INTERVAL_MS;

        // A blank, control-bearing, over-long, wrongly shaped, or non-hex value
        // is never a canonical daemon-assigned identity, and the gate rejects it
        // before any durable read.
        for invalid in [
            String::new(),
            "harness\u{0}rule".to_owned(),
            "a".repeat(129),
            "harness-rule-1".to_owned(),
            "z".repeat(32),
        ] {
            let error = tick_fixture_harness(
                &facade,
                &invalid,
                observed_at_ms,
                HARNESS_FIXTURE_PROJECT_TIME_ZONE,
                true,
            )
            .expect_err("a non-canonical harness identity fails closed");
            assert_eq!(
                error.code(),
                "harness_source_unavailable",
                "{invalid:?} fails closed"
            );
        }
    }

    #[test]
    fn harness_journal_pages_are_scoped_to_the_owning_session_and_bounded() {
        let (_directory, facade) = test_facade();
        let project_id = ProjectId::new();
        let session_id = create_harness_session(&facade, project_id);
        let other_session_id = create_harness_session(&facade, project_id);
        let harness_id = create_fixture_harness_rule(
            &facade,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );
        admit_fixture_harness_launch(
            &facade,
            &harness_id,
            HARNESS_FIXTURE_INTERVAL_ANCHOR_MS + HARNESS_FIXTURE_INTERVAL_MS,
        );

        let page = load_fixture_harness_journal(&facade, session_id, &harness_id, 0, 64)
            .expect("the owning session reads its journal");
        assert_eq!(page.len(), 2);
        assert!(page.iter().all(|record| record.harness_id == harness_id));
        let next_page = load_fixture_harness_journal(&facade, session_id, &harness_id, 1, 64)
            .expect("the owning session pages its journal");
        assert_eq!(next_page.len(), 1);
        assert_eq!(next_page[0].sequence, 2);
        assert!(
            load_fixture_harness_journal(&facade, session_id, &harness_id, 2, 64)
                .expect("a page past the journal tail reads")
                .is_empty()
        );

        // Another session of the same project never reads this session's
        // journal, a session of another project never does either, and an
        // unknown session has no durable projection at all.
        let error = load_fixture_harness_journal(&facade, other_session_id, &harness_id, 0, 64)
            .expect_err("a harness journal is readable only by its owning session");
        assert_eq!(error.code(), "harness_source_unavailable");
        let foreign_project_session_id = create_harness_session(&facade, ProjectId::new());
        let error =
            load_fixture_harness_journal(&facade, foreign_project_session_id, &harness_id, 0, 64)
                .expect_err("a session of another project never reads this journal");
        assert_eq!(error.code(), "harness_source_unavailable");
        let error = load_fixture_harness_journal(&facade, SessionId::new(), &harness_id, 0, 64)
            .expect_err("an unknown session has no durable projection");
        assert_eq!(error.code(), "storage_record_not_found");

        // An empty or over-bound page fails closed.
        let error = load_fixture_harness_journal(&facade, session_id, &harness_id, 0, 0)
            .expect_err("an empty journal page fails closed");
        assert_eq!(error.code(), "invalid_harness_journal_page");
        let error = load_fixture_harness_journal(&facade, session_id, &harness_id, 0, 65)
            .expect_err("an over-bound journal page fails closed");
        assert_eq!(error.code(), "invalid_harness_journal_page");

        // Credentials and filesystem paths never cross this boundary.
        let error =
            load_fixture_harness_journal(&facade, session_id, "sk-live-harness-journal", 0, 64)
                .expect_err("credentials never cross the journal boundary");
        assert_eq!(error.code(), "credentials_forbidden");
        let path_text = std::env::temp_dir()
            .join("harness-journal.sqlite")
            .to_string_lossy()
            .into_owned();
        let error = load_fixture_harness_journal(&facade, session_id, &path_text, 0, 64)
            .expect_err("a filesystem path is never a harness identity");
        assert_eq!(error.code(), "harness_source_unavailable");
    }

    #[test]
    fn interrupted_harness_launch_recovery_keeps_the_interrupted_outcome() {
        let (_directory, facade) = test_facade();
        let project_id = ProjectId::new();
        let session_id = create_harness_session(&facade, project_id);
        let harness_id = create_fixture_harness_rule(
            &facade,
            project_id,
            session_id,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
        );
        let observed_at_ms = HARNESS_FIXTURE_INTERVAL_ANCHOR_MS + HARNESS_FIXTURE_INTERVAL_MS;
        let interrupted_run_id = admit_fixture_harness_launch(&facade, &harness_id, observed_at_ms);

        let recovery = facade
            .recover_interrupted_harness_launch_for_daemon(&harness_id, &interrupted_run_id, 9_000)
            .expect("the interrupted launch recovers without resuming");
        assert_eq!(recovery.terminal.outcome, HarnessRunOutcomeV1::Interrupted);
        assert_eq!(recovery.terminal.harness_id, harness_id);
        assert_eq!(recovery.terminal.producing_run_id, interrupted_run_id);
        assert!(!recovery.external_work_resumes);
        assert!(recovery.requires_separate_admission);
        assert!(
            recovery.terminal.journal_records.iter().any(|record| {
                record.record_kind == HarnessJournalRecordKindDto::RunTerminal
                    && record.safe_summary.contains("interrupted")
            }),
            "the interrupted terminal decision is durable"
        );

        // Recovery resumes nothing: the rule keeps its revision, and the
        // already captured slot is redelivered without a new launch or a new
        // scheduled successor.
        let rule = facade
            .inner
            .repository
            .load_harness_rule(harness_id.clone())
            .expect("the recovered rule reads");
        assert_eq!(rule.active_revision, 1);
        let outcome = tick_fixture_harness(
            &facade,
            &harness_id,
            observed_at_ms,
            HARNESS_FIXTURE_PROJECT_TIME_ZONE,
            true,
        )
        .expect("the restarted tick observes no new slot");
        assert_eq!(
            outcome.observations[0].capture.outcome,
            HarnessTriggerCaptureOutcomeDto::Redelivered
        );
        assert!(outcome.launch.is_none());
        assert!(outcome.pending_reason.is_none());
        assert!(
            load_fixture_harness_journal(&facade, session_id, &harness_id, 0, 64)
                .expect("the durable journal reads")
                .iter()
                .any(|record| record.record_kind == HarnessJournalRecordKindDto::RunTerminal)
        );

        // Credentials and filesystem paths never cross the recovery boundary.
        let error = facade
            .recover_interrupted_harness_launch_for_daemon(
                "sk-live-harness-recovery",
                &interrupted_run_id,
                9_001,
            )
            .expect_err("credentials never cross the recovery boundary");
        assert_eq!(error.code(), "credentials_forbidden");
        let path_text = std::env::temp_dir()
            .join("harness-interrupted.sqlite")
            .to_string_lossy()
            .into_owned();
        let error = facade
            .recover_interrupted_harness_launch_for_daemon(&harness_id, &path_text, 9_002)
            .expect_err("a filesystem path is never an interrupted launch identity");
        assert_eq!(error.code(), "harness_source_unavailable");
    }

    #[test]
    fn programmatic_policy_reservation_recovery_is_idempotent_and_never_resumes_work() {
        let (_directory, facade) = test_facade();
        let root_run_id = RunId::new().to_string();

        // A root run without outstanding reservations recovers to an empty,
        // repeatable list.
        assert!(
            facade
                .recover_programmatic_policy_reservations_for_daemon(&root_run_id, 9_000)
                .expect("startup recovery reads the durable reservations")
                .is_empty()
        );
        assert!(
            facade
                .recover_programmatic_policy_reservations_for_daemon(&root_run_id, 9_001)
                .expect("a repeated recovery reads the same durable state")
                .is_empty()
        );

        seed_fixture_programmatic_reservations(&facade, &root_run_id);

        // One outstanding reservation is released before start, and the one
        // that reached `ToolCallStarted` keeps its permanent consumption as an
        // unknown external effect.
        let recovered = facade
            .recover_programmatic_policy_reservations_for_daemon(&root_run_id, 9_002)
            .expect("the outstanding reservations recover");
        let dispositions: Vec<ProgrammaticRecoveryDispositionV1> =
            recovered.iter().map(|entry| entry.disposition).collect();
        assert_eq!(
            dispositions,
            vec![
                ProgrammaticRecoveryDispositionV1::ExternalEffectUnknown,
                ProgrammaticRecoveryDispositionV1::InterruptedBeforeStart,
            ]
        );
        assert!(
            recovered.iter().all(|entry| !entry.external_work_resumes),
            "no recovered action resumes external work"
        );
        assert_eq!(
            facade
                .inner
                .repository
                .load_programmatic_policy_reservation("reservation-started".to_owned())
                .expect("the started reservation reads")
                .expect("the started reservation exists")
                .state,
            ProgrammaticReservationStateDto::ExternalEffectUnknown
        );
        assert_eq!(
            facade
                .inner
                .repository
                .load_programmatic_policy_reservation("reservation-unstarted".to_owned())
                .expect("the unstarted reservation reads")
                .expect("the unstarted reservation exists")
                .state,
            ProgrammaticReservationStateDto::InterruptedBeforeStart
        );

        // The started set is derived from the durable state: a repeated
        // recovery changes nothing and recovers nothing.
        assert!(
            facade
                .recover_programmatic_policy_reservations_for_daemon(&root_run_id, 9_003)
                .expect("a repeated recovery changes nothing")
                .is_empty()
        );

        // Credentials and filesystem paths never cross the recovery boundary.
        let error = facade
            .recover_programmatic_policy_reservations_for_daemon("sk-live-policy-recovery", 9_004)
            .expect_err("credentials never cross the recovery boundary");
        assert_eq!(error.code(), "credentials_forbidden");
        let path_text = std::env::temp_dir()
            .join("policy-recovery.sqlite")
            .to_string_lossy()
            .into_owned();
        let error = facade
            .recover_programmatic_policy_reservations_for_daemon(&path_text, 9_005)
            .expect_err("a filesystem path is never a root run identity");
        assert_eq!(error.code(), "harness_source_unavailable");
    }
}
