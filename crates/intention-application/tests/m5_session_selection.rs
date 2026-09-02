#![allow(
    clippy::expect_used,
    reason = "Focused session-selection fixtures use expect for precise diagnostics."
)]

//! Zone 5 Slice 2 session-selection integration tests.
//!
//! The in-memory fake storage below mirrors the durable backend invariants:
//! optimistic session-default revisions, at most one pending removal
//! candidate, idempotent held-run admission, and atomicity through
//! `Result`-returning methods (the fakes error on conflict).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use intention_application::{
    ApplicationService, CatalogAdmissionPort, CatalogProviderDeclarationDto, CatalogReadService,
    CatalogReadiness, CatalogSourceInputDto, ControlPlaneReadinessPort, DegradedModeService,
    HeldRunService, ModelRunDispatchPort, ModelRunDriverHandle, ProviderCatalogController,
    ProviderDriverFactory, RemovalService, ResolvedProfileDto, ScheduleModelRunDto,
    SelectionResolutionService, SendUserTurnWorkflowInputDto, SessionProfileService,
    UnavailableQueueService, UsageService,
};
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
    control_plane::{ConfigCandidateDto, parse_candidate},
};
use intention_domain::{
    ContextPreservationCapability, CredentialTransportMode as DomainCredentialTransportMode,
    DomainEventDto, ModelCapabilitySetV1, ModelInputCapability, ProviderDriverContractRevisionDto,
    ProviderKindDescriptorRevisionV1, ProviderProfileRevisionV1, ProviderSelectionV1,
    ReasoningCapability, RunModeDto, SendUserTurnCommandDto, SessionProjectionDto,
    StructuredOutputCapability, WorkspaceRootDto, provider_selection::MODEL_CAPABILITY_TAXONOMY_V1,
};
use intention_protocol::contract_families::{
    AcceptProviderCatalogRemovalAcceptedDto, AcceptProviderCatalogRemovalCommandDto,
    AdmitRecoveredRunAcceptedDto, AdmitRecoveredRunCommandDto, CredentialTransportMode,
    GetProviderCatalogQueryDto, GetProviderCatalogStatusQueryDto, GetProviderUsageQueryDto,
    GetSessionProviderProfileQueryDto, ProviderCatalogActivationState,
    ProviderCatalogDegradedReason, ProviderProfileUnavailableReason,
    ReconcileUnavailableQueueCommandDto, RejectProviderCatalogCandidateAcceptedDto,
    RejectProviderCatalogCandidateCommandDto, ResolvedProviderProfileDto,
    SetSessionProviderProfileCommandDto,
};
use intention_runtime::{ModelMessageDto, ModelRequestDto, ModelRoleDto};
use intention_storage::{
    AcceptProviderCatalogInputDto, AcceptProviderCatalogRemovalInputDto, AcceptUserTurnInputDto,
    AcceptedTurnOutcomeDto, AdmitHeldRecoveredRunInputDto,
    AppendProviderKindDescriptorRevisionInputDto, AppendProviderProfileRevisionInputDto,
    CommittedChangeDto, CreateProviderCatalogRemovalCandidateInputDto, CreateSessionInputDto,
    EnqueueUnavailableRunInputDto, ExpireProviderCatalogCandidateInputDto,
    ExpireProviderCatalogRemovalCandidateInputDto, HeldRecoveredRunDto, HeldRunAdmissionStateDto,
    HeldRunRepositoryDto, LoadProviderCatalogPageInputDto, LoadUnavailableQueuePageInputDto,
    PersistResolvedRunProviderSelectionInputDto, PromoteUnavailableRunsInputDto,
    PromoteUnavailableRunsOutcomeDto, ProviderCatalogMaterialDto, ProviderCatalogPageDto,
    ProviderCatalogProfileEntryDto, ProviderCatalogRemovalCandidateDto,
    ProviderCatalogRemovalStatusDto, ProviderCatalogRepositoryDto, ProviderCatalogStateDto,
    ProviderCatalogStatusDto as DurableCatalogStatusDto, ProviderKindDescriptorCandidateDto,
    ProviderProfileCandidateDto, ProviderReadinessDto, ProviderRemovalRepositoryDto,
    ProviderSelectionRepositoryDto, ProviderUsageAggregateDto, ProviderUsageRepositoryDto,
    QueueReconciliationMarkerDto, ReconcileUnavailableQueueInputDto,
    ReconcileUnavailableQueueOutcomeDto, RecordProviderUsageInputDto,
    RecoverUnfinishedRunsInputDto, RejectProviderCatalogCandidateInputDto,
    RejectProviderCatalogRemovalInputDto, RemoveQueuedTurnInputDto, SessionProviderDefaultDto,
    SessionProviderDefaultsRepositoryDto, SetSessionProviderProfileInputDto,
    SetSessionProviderProfileOutcomeDto, StorageRepositoryDto, TransitionRunInputDto,
    UnavailableQueueRepositoryDto, UnavailableQueueStateDto, UnavailableRunQueueEntryDto,
};
use intention_types::{
    ConfigRevisionId, DtoResult, ErrorCategoryDto, ErrorDto, EventEnvelopeDto, ProjectId,
    QueuePositionDto, RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId, TimestampDto,
    TurnId, WorkspaceId,
};

const CREDENTIAL: &str = "sk-test-sweep-12345";
const ENDPOINT: &str = "https://api.example.invalid/v1";

// ============================================================================
// Config fixtures (catalog runtime style)
// ============================================================================

fn time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid")
}

fn explicit_source() -> ConfigSourceDto {
    ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-application-session-selection.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture path is absolute"),
    )
}

fn raw_config(kind: &str, model: &str, endpoint: &str) -> String {
    format!(
        "schema_version = 1\n[provider]\nkind = \"{kind}\"\nmodel = \"{model}\"\ncredential = \"{CREDENTIAL}\"\nendpoint = \"{endpoint}\"\n"
    )
}

fn snapshot(
    kind: &str,
    model: &str,
    endpoint: &str,
    revision: ConfigRevisionId,
) -> ConfigSnapshotDto {
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        raw_config(kind, model, endpoint),
        explicit_source(),
    ))
    .expect("fixture config resolves");
    ConfigSnapshotDto::new(SchemaVersionDto::new(1, 0), revision, time(), resolved)
        .expect("fixture snapshot is valid")
}

fn fixture_snapshot() -> ConfigSnapshotDto {
    snapshot("openrouter", "fixture", ENDPOINT, ConfigRevisionId::new())
}

fn candidate(
    kind: &str,
    model: &str,
    endpoint: &str,
    previous: &ConfigSnapshotDto,
) -> ConfigCandidateDto {
    parse_candidate(
        RawConfigInputDto::new(raw_config(kind, model, endpoint), explicit_source()),
        previous,
    )
    .expect("fixture candidate parses")
}

fn declaration(
    kind: &str,
    model: &str,
    endpoint: Option<&str>,
    enabled: bool,
) -> CatalogProviderDeclarationDto {
    CatalogProviderDeclarationDto {
        kind: kind.to_owned(),
        model: model.to_owned(),
        endpoint: endpoint.map(str::to_owned),
        declared_model_capability_subset: vec![
            "text_input".to_owned(),
            "text_streaming".to_owned(),
        ],
        enabled,
    }
}

fn source(
    operation_id: &str,
    raw_size: u64,
    providers: Vec<CatalogProviderDeclarationDto>,
    candidate: ConfigCandidateDto,
    previous: ConfigSnapshotDto,
) -> CatalogSourceInputDto {
    CatalogSourceInputDto {
        operation_id: operation_id.to_owned(),
        raw_config_size_bytes: raw_size,
        providers,
        candidate,
        previous,
    }
}

// ============================================================================
// Domain seeds
// ============================================================================

fn standard_envelope() -> ModelCapabilitySetV1 {
    ModelCapabilitySetV1 {
        taxonomy_version: MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
        input: ModelInputCapability::TextOnly,
        text_streaming: true,
        structured_output: StructuredOutputCapability::Unsupported,
        reasoning: ReasoningCapability::TextualReasoningV1,
        tool_exchange: false,
        context_preservation: ContextPreservationCapability::LocalDurableHistoryV1 {
            reasoning_input_contract: "reasoning-history-transfer-v1".to_owned(),
        },
    }
}

fn seed_kind_descriptor(kind: &str, transport_contract: &str) -> ProviderKindDescriptorRevisionV1 {
    ProviderKindDescriptorRevisionV1 {
        kind_id: kind.to_owned(),
        descriptor_family: format!("{kind}-descriptor-v1"),
        ordered_protocol_part_revisions: vec!["protocol-parts-v1".to_owned()],
        endpoint_policy: "https-only".to_owned(),
        credential_transport_contract: transport_contract.to_owned(),
        model_capability_envelope: standard_envelope(),
        driver_contract_family: kind.to_owned(),
    }
}

fn seed_kind_candidate(kind: &str, transport_contract: &str) -> ProviderKindDescriptorCandidateDto {
    ProviderKindDescriptorCandidateDto {
        descriptor_revision_id: format!("kind-{kind}-v1"),
        descriptor: seed_kind_descriptor(kind, transport_contract),
    }
}

fn seed_profile(
    kind: &str,
    model: &str,
    profile_id: &str,
    revision_id: &str,
    kind_descriptor_revision_id: &str,
) -> ProviderProfileCandidateDto {
    ProviderProfileCandidateDto {
        profile: ProviderProfileRevisionV1 {
            profile_id: profile_id.to_owned(),
            revision_id: revision_id.to_owned(),
            provider_kind_id: kind.to_owned(),
            model_id: model.to_owned(),
            endpoint: ENDPOINT.to_owned(),
            credential_transport_mode: DomainCredentialTransportMode::Bearer,
            safe_header_name: None,
            capability_taxonomy_revision: MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
            reasoning_compatibility_id: None,
            kind_descriptor_revision_id: kind_descriptor_revision_id.to_owned(),
            driver_contract_revision: ProviderDriverContractRevisionDto {
                driver_family: kind.to_owned(),
                major: 1,
                minor: 0,
            },
        },
        declared_model_capability_subset: vec![
            "text_input".to_owned(),
            "text_streaming".to_owned(),
        ],
        resolved_reasoning_policy: "textual-reasoning-v1".to_owned(),
        effective_execution_policy: "execution-timeout-30-attempts-2".to_owned(),
        effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
        display_name: None,
        enabled: true,
        credential_configured: false,
        readiness: ProviderReadinessDto::Ready,
    }
}

/// A profile candidate with an explicit projected readiness.
const fn with_readiness(
    mut candidate: ProviderProfileCandidateDto,
    readiness: ProviderReadinessDto,
) -> ProviderProfileCandidateDto {
    candidate.readiness = readiness;
    candidate
}

/// A profile candidate carrying the safe-header credential transport.
fn safe_header_candidate(
    mut candidate: ProviderProfileCandidateDto,
) -> ProviderProfileCandidateDto {
    candidate.profile.credential_transport_mode = DomainCredentialTransportMode::SafeHeader;
    candidate.profile.safe_header_name = Some("x-auth-header".to_owned());
    candidate
}

/// The safe resolved projection used by the admission port fakes.
fn resolved_profile(profile_id: &str, revision: &str) -> ResolvedProfileDto {
    ResolvedProfileDto {
        profile_id: profile_id.to_owned(),
        profile_revision_id: revision.to_owned(),
        kind_id: "responses".to_owned(),
        kind_descriptor_revision_id: "kind-responses-v1".to_owned(),
        model_id: "model-a".to_owned(),
        normalized_effective_endpoint: ENDPOINT.to_owned(),
        credential_transport_mode: CredentialTransportMode::Bearer,
        credential_transport_safe_header_name: None,
        declared_model_capability_subset: vec![
            "text_input".to_owned(),
            "text_streaming".to_owned(),
        ],
        resolved_reasoning_policy: "textual-reasoning-v1".to_owned(),
        effective_execution_policy: "execution-timeout-30-attempts-2".to_owned(),
        effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
        provider_driver_contract_revision: "responses-1.0".to_owned(),
    }
}

/// The resolved profile projection with the safe-header transport.
fn safe_header_profile(profile_id: &str, revision: &str) -> ResolvedProfileDto {
    let mut profile = resolved_profile(profile_id, revision);
    profile.credential_transport_mode = CredentialTransportMode::SafeHeader;
    profile.credential_transport_safe_header_name = Some("x-auth-header".to_owned());
    profile
}

/// A valid persisted immutable provider selection for one run.
fn selection(profile_id: &str, kind: &str) -> ProviderSelectionV1 {
    ProviderSelectionV1 {
        selection_canonicalization_version: "provider-selection-v1".to_owned(),
        profile_id: profile_id.to_owned(),
        provider_profile_revision_id: "rev-0001".to_owned(),
        kind_id: kind.to_owned(),
        kind_descriptor_revision_id: "kind-responses-v1".to_owned(),
        model_id: "model-a".to_owned(),
        normalized_effective_endpoint: ENDPOINT.to_owned(),
        credential_transport_mode: DomainCredentialTransportMode::Bearer,
        credential_transport_safe_header_name: None,
        declared_model_capability_subset: vec![
            "text_input".to_owned(),
            "text_streaming".to_owned(),
        ],
        resolved_reasoning_policy: "textual-reasoning-v1".to_owned(),
        effective_execution_policy: "execution-timeout-30-attempts-2".to_owned(),
        effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
        provider_driver_contract_revision: "responses-1.0".to_owned(),
        selection_source: Some("session_default".to_owned()),
    }
}

// ============================================================================
// Counting driver factory
// ============================================================================

struct TestHandle;

impl ModelRunDriverHandle for TestHandle {}

struct CountingFactory {
    kind: String,
    contract_family: String,
    contract_major: u64,
    max_minor: u64,
    builds: Arc<AtomicUsize>,
}

impl CountingFactory {
    fn new(
        kind: &str,
        contract_family: &str,
        contract_major: u64,
        max_minor: u64,
        builds: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            kind: kind.to_owned(),
            contract_family: contract_family.to_owned(),
            contract_major,
            max_minor,
            builds,
        }
    }
}

impl ProviderDriverFactory for CountingFactory {
    fn kind(&self) -> &str {
        &self.kind
    }

    fn supports_contract(&self, contract: &ProviderDriverContractRevisionDto) -> bool {
        contract.driver_family == self.contract_family
            && contract.major == self.contract_major
            && contract.minor <= self.max_minor
    }

    fn build(
        &self,
        _profile: intention_application::PrivateProviderProfileMaterial,
    ) -> DtoResult<Box<dyn ModelRunDriverHandle + Send + Sync>> {
        self.builds.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(TestHandle))
    }
}

fn responses_factory(builds: Arc<AtomicUsize>) -> Box<dyn ProviderDriverFactory> {
    Box::new(CountingFactory::new("responses", "responses", 1, 2, builds))
}

fn generic_chat_factory(builds: Arc<AtomicUsize>) -> Box<dyn ProviderDriverFactory> {
    Box::new(CountingFactory::new(
        "generic-chat-completion-api",
        "generic-chat-completion-api",
        1,
        2,
        builds,
    ))
}

fn both_factories(builds: Arc<AtomicUsize>) -> Vec<Box<dyn ProviderDriverFactory>> {
    vec![
        responses_factory(builds.clone()),
        generic_chat_factory(builds),
    ]
}

// ============================================================================
// Error helpers
// ============================================================================

fn conflict(code: &'static str, message: &'static str) -> ErrorDto {
    ErrorDto::new(
        code,
        ErrorCategoryDto::Conflict,
        message,
        intention_types::ErrorRetryDto::Never,
        None,
    )
    .expect("fixture conflict error is valid")
}

fn not_found(code: &'static str, message: &'static str) -> ErrorDto {
    ErrorDto::new(
        code,
        ErrorCategoryDto::NotFound,
        message,
        intention_types::ErrorRetryDto::Never,
        None,
    )
    .expect("fixture not-found error is valid")
}

fn unavailable(code: &'static str, message: &'static str) -> ErrorDto {
    ErrorDto::unavailable(code, message)
}

// ============================================================================
// Fake catalog storage (ProviderCatalogRepositoryDto + ProviderRemovalRepositoryDto)
// ============================================================================

struct FakeCatalogState {
    status: DurableCatalogStatusDto,
    active_catalog_revision_id: Option<u64>,
    candidate_catalog_revision_id: Option<u64>,
    active_default_profile_id: Option<String>,
    candidate_handle: Option<String>,
    degraded_reason: Option<String>,
    updated_at: i64,
    kind_records: Vec<(u64, ProviderKindDescriptorCandidateDto)>,
    profile_records: Vec<(u64, ProviderProfileCandidateDto)>,
    active_kinds: Vec<ProviderKindDescriptorCandidateDto>,
    active_profiles: Vec<ProviderProfileCandidateDto>,
    profile_tombstones: Vec<String>,
    kind_tombstones: Vec<String>,
    removal_candidates: Vec<ProviderCatalogRemovalCandidateDto>,
}

impl FakeCatalogState {
    const fn new() -> Self {
        Self {
            status: DurableCatalogStatusDto::Preparing,
            active_catalog_revision_id: None,
            candidate_catalog_revision_id: None,
            active_default_profile_id: None,
            candidate_handle: None,
            degraded_reason: None,
            updated_at: 0,
            kind_records: Vec::new(),
            profile_records: Vec::new(),
            active_kinds: Vec::new(),
            active_profiles: Vec::new(),
            profile_tombstones: Vec::new(),
            kind_tombstones: Vec::new(),
            removal_candidates: Vec::new(),
        }
    }
}

struct FakeCatalog {
    state: RefCell<FakeCatalogState>,
    audits: RefCell<Vec<String>>,
    material_fault: bool,
    material_override: RefCell<Option<ProviderCatalogMaterialDto>>,
}

impl FakeCatalog {
    const fn new() -> Self {
        Self {
            state: RefCell::new(FakeCatalogState::new()),
            audits: RefCell::new(Vec::new()),
            material_fault: false,
            material_override: RefCell::new(None),
        }
    }

    fn seed_active(
        &self,
        revision: u64,
        kinds: Vec<ProviderKindDescriptorCandidateDto>,
        profiles: Vec<ProviderProfileCandidateDto>,
        default_profile_id: &str,
    ) {
        let mut state = self.state.borrow_mut();
        for kind in &kinds {
            append_kind_record(&mut state, revision, kind).expect("fixture kind record appends");
        }
        for profile in &profiles {
            append_profile_record(&mut state, revision, profile)
                .expect("fixture profile record appends");
        }
        state.active_kinds = kinds;
        state.active_profiles = profiles;
        state.status = DurableCatalogStatusDto::Active;
        state.active_catalog_revision_id = Some(revision);
        state.active_default_profile_id = Some(default_profile_id.to_owned());
        state.updated_at = 1;
    }

    fn seed_recovery_required(&self, revision: u64) {
        let mut state = self.state.borrow_mut();
        state.status = DurableCatalogStatusDto::ActivationRecoveryRequired;
        state.active_catalog_revision_id = Some(revision);
        state.candidate_catalog_revision_id = Some(revision);
        state.updated_at = 1;
    }

    fn active_material(&self) -> DtoResult<ProviderCatalogMaterialDto> {
        let state = self.state.borrow();
        material_at(&state)
    }

    fn build_controller(
        &self,
        factories: Vec<Box<dyn ProviderDriverFactory>>,
    ) -> ProviderCatalogController<&Self, &Self> {
        ProviderCatalogController::new(self, self, factories)
    }
}

fn append_kind_record(
    state: &mut FakeCatalogState,
    revision: u64,
    kind: &ProviderKindDescriptorCandidateDto,
) -> DtoResult<()> {
    if let Some((_, existing)) = state.kind_records.iter().find(|(_, existing)| {
        existing.descriptor_revision_id == kind.descriptor_revision_id
            && existing.descriptor.kind_id == kind.descriptor.kind_id
    }) {
        if existing.descriptor != kind.descriptor {
            return Err(conflict(
                "provider_kind_descriptor_revision_conflict",
                "the kind descriptor revision identity is already bound to different bytes",
            ));
        }
        return Ok(());
    }
    state.kind_records.push((revision, kind.clone()));
    Ok(())
}

fn append_profile_record(
    state: &mut FakeCatalogState,
    revision: u64,
    profile: &ProviderProfileCandidateDto,
) -> DtoResult<()> {
    if let Some((_, existing)) = state.profile_records.iter().find(|(_, existing)| {
        existing.profile.profile_id == profile.profile.profile_id
            && existing.profile.revision_id == profile.profile.revision_id
    }) {
        if existing.profile != profile.profile {
            return Err(conflict(
                "provider_profile_revision_conflict",
                "the profile revision identity is already bound to different bytes",
            ));
        }
        return Ok(());
    }
    state.profile_records.push((revision, profile.clone()));
    Ok(())
}

fn material_at(state: &FakeCatalogState) -> DtoResult<ProviderCatalogMaterialDto> {
    let Some(active) = state.active_catalog_revision_id else {
        return Err(not_found(
            "provider_catalog_not_active",
            "no active provider catalog is committed",
        ));
    };
    Ok(ProviderCatalogMaterialDto {
        catalog_revision_id: active,
        default_profile_id: state.active_default_profile_id.clone(),
        kind_descriptors: state.active_kinds.clone(),
        profiles: state.active_profiles.clone(),
    })
}

impl ProviderCatalogRepositoryDto for &FakeCatalog {
    fn append_provider_kind_descriptor_revision(
        &self,
        input: AppendProviderKindDescriptorRevisionInputDto,
    ) -> DtoResult<()> {
        let mut state = self.state.borrow_mut();
        let kind = ProviderKindDescriptorCandidateDto {
            descriptor_revision_id: input.descriptor_revision_id,
            descriptor: input.descriptor,
        };
        append_kind_record(&mut state, input.catalog_revision_id, &kind)?;
        state.candidate_catalog_revision_id = Some(input.catalog_revision_id);
        state.updated_at = input.accepted_at;
        self.audits
            .borrow_mut()
            .push("ProviderCatalogCandidatePrepared".to_owned());
        Ok(())
    }

    fn append_provider_profile_revision(
        &self,
        input: AppendProviderProfileRevisionInputDto,
    ) -> DtoResult<()> {
        let mut state = self.state.borrow_mut();
        append_profile_record(&mut state, input.catalog_revision_id, &input.profile)?;
        state.candidate_catalog_revision_id = Some(input.catalog_revision_id);
        state.updated_at = input.accepted_at;
        self.audits
            .borrow_mut()
            .push("ProviderCatalogCandidatePrepared".to_owned());
        Ok(())
    }

    fn load_provider_catalog_status(&self) -> DtoResult<ProviderCatalogStateDto> {
        let state = self.state.borrow();
        Ok(ProviderCatalogStateDto {
            active_catalog_revision_id: state.active_catalog_revision_id,
            candidate_catalog_revision_id: state.candidate_catalog_revision_id,
            status: state.status,
            active_default_profile_id: state.active_default_profile_id.clone(),
            candidate_handle: state.candidate_handle.clone(),
            degraded_reason: state.degraded_reason.clone(),
            updated_at: state.updated_at,
        })
    }

    fn load_provider_catalog_page(
        &self,
        input: LoadProviderCatalogPageInputDto,
    ) -> DtoResult<ProviderCatalogPageDto> {
        let state = self.state.borrow();
        let entries = state
            .active_profiles
            .iter()
            .map(|profile| ProviderCatalogProfileEntryDto {
                profile_id: profile.profile.profile_id.clone(),
                profile_revision_id: profile.profile.revision_id.clone(),
                kind_id: profile.profile.provider_kind_id.clone(),
                kind_descriptor_revision_id: profile.profile.kind_descriptor_revision_id.clone(),
                display_name: profile.display_name.clone(),
                enabled: profile.enabled,
                credential_configured: profile.credential_configured,
                readiness: profile.readiness,
                safe_projection_json: String::new(),
            })
            .collect::<Vec<_>>();
        let limit = usize::try_from(input.limit).unwrap_or(usize::MAX);
        let has_more = entries.len() > limit;
        let bounded = entries.into_iter().take(limit).collect();
        Ok(ProviderCatalogPageDto {
            entries: bounded,
            next_token: None,
            has_more,
        })
    }

    fn accept_provider_catalog(&self, input: AcceptProviderCatalogInputDto) -> DtoResult<()> {
        let mut state = self.state.borrow_mut();
        if state.candidate_catalog_revision_id != Some(input.catalog_revision_id) {
            return Err(conflict(
                "provider_catalog_revision_conflict",
                "the accepted catalog revision does not match the prepared candidate",
            ));
        }
        if state
            .candidate_handle
            .as_ref()
            .is_some_and(|handle| handle != &input.candidate_handle)
        {
            return Err(conflict(
                "provider_catalog_candidate_conflict",
                "the accepted candidate handle does not match the prepared candidate",
            ));
        }
        for kind in &input.kind_descriptors {
            append_kind_record(&mut state, input.catalog_revision_id, kind)?;
        }
        for profile in &input.profiles {
            append_profile_record(&mut state, input.catalog_revision_id, profile)?;
        }
        let new_profile_ids = input
            .profiles
            .iter()
            .map(|profile| profile.profile.profile_id.clone())
            .collect::<Vec<_>>();
        let new_kind_ids = input
            .kind_descriptors
            .iter()
            .map(|kind| kind.descriptor.kind_id.clone())
            .collect::<Vec<_>>();
        let mut tombstone_profiles = Vec::new();
        let mut tombstone_kinds = Vec::new();
        for profile in &state.active_profiles {
            if !new_profile_ids.contains(&profile.profile.profile_id) {
                tombstone_profiles.push(profile.profile.profile_id.clone());
            }
        }
        for kind in &state.active_kinds {
            if !new_kind_ids.contains(&kind.descriptor.kind_id) {
                tombstone_kinds.push(kind.descriptor.kind_id.clone());
            }
        }
        state.profile_tombstones.extend(tombstone_profiles);
        state.kind_tombstones.extend(tombstone_kinds);
        state.active_kinds = input.kind_descriptors.clone();
        state.active_profiles = input.profiles.clone();
        state.status = DurableCatalogStatusDto::Active;
        state.active_catalog_revision_id = Some(input.catalog_revision_id);
        state.candidate_catalog_revision_id = None;
        state.candidate_handle = None;
        state.active_default_profile_id = Some(input.default_profile_id);
        state.degraded_reason = None;
        state.updated_at = input.accepted_at;
        self.audits
            .borrow_mut()
            .push("ProviderCatalogAccepted".to_owned());
        self.audits
            .borrow_mut()
            .push("ProviderCatalogActivated".to_owned());
        Ok(())
    }

    fn reject_provider_catalog_candidate(
        &self,
        input: RejectProviderCatalogCandidateInputDto,
    ) -> DtoResult<()> {
        let mut state = self.state.borrow_mut();
        if state.candidate_catalog_revision_id != Some(input.catalog_revision_id) {
            return Err(conflict(
                "provider_catalog_candidate_conflict",
                "the rejected candidate does not match the prepared candidate",
            ));
        }
        state.candidate_catalog_revision_id = None;
        state.candidate_handle = None;
        state.status = if state.active_catalog_revision_id.is_some() {
            DurableCatalogStatusDto::Active
        } else {
            DurableCatalogStatusDto::Preparing
        };
        state.updated_at = input.rejected_at;
        self.audits
            .borrow_mut()
            .push("ProviderCatalogCandidateRejected".to_owned());
        Ok(())
    }

    fn expire_provider_catalog_candidate(
        &self,
        input: ExpireProviderCatalogCandidateInputDto,
    ) -> DtoResult<()> {
        let mut state = self.state.borrow_mut();
        if state.candidate_catalog_revision_id != Some(input.catalog_revision_id) {
            return Err(conflict(
                "provider_catalog_candidate_conflict",
                "the expired candidate does not match the prepared candidate",
            ));
        }
        state.candidate_catalog_revision_id = None;
        state.candidate_handle = None;
        state.status = if state.active_catalog_revision_id.is_some() {
            DurableCatalogStatusDto::Active
        } else {
            DurableCatalogStatusDto::Preparing
        };
        state.updated_at = input.expired_at;
        self.audits
            .borrow_mut()
            .push("ProviderCatalogCandidateExpired".to_owned());
        Ok(())
    }

    fn load_provider_catalog_material(&self) -> DtoResult<ProviderCatalogMaterialDto> {
        if let Some(material) = self.material_override.borrow().clone() {
            return Ok(material);
        }
        if self.material_fault {
            return Err(unavailable(
                "injected_material_fault",
                "the accepted provider catalog material is unavailable",
            ));
        }
        let state = self.state.borrow();
        material_at(&state)
    }
}

impl ProviderRemovalRepositoryDto for &FakeCatalog {
    fn create_provider_catalog_removal_candidate(
        &self,
        input: CreateProviderCatalogRemovalCandidateInputDto,
    ) -> DtoResult<()> {
        let mut state = self.state.borrow_mut();
        if state
            .removal_candidates
            .iter()
            .any(|candidate| candidate.status == ProviderCatalogRemovalStatusDto::Pending)
        {
            return Err(conflict(
                "provider_catalog_removal_pending_exists",
                "a pending provider catalog removal candidate already exists",
            ));
        }
        let expires_at = input
            .created_at
            .checked_add(30 * 60)
            .ok_or_else(|| unavailable("removal_candidate_expiry_overflow", "expiry overflow"))?;
        state
            .removal_candidates
            .push(ProviderCatalogRemovalCandidateDto {
                candidate_handle: input.candidate_handle,
                candidate_catalog_revision_id: input.candidate_catalog_revision_id,
                active_catalog_revision_id: input.active_catalog_revision_id,
                created_at: input.created_at,
                expires_at,
                source_recheck: input.source_recheck,
                status: ProviderCatalogRemovalStatusDto::Pending,
                candidate_json: input.candidate_json,
                operation_id: Some(input.operation_id),
                completed_at: None,
            });
        state.status = DurableCatalogStatusDto::PendingRemoval;
        state.updated_at = input.created_at;
        self.audits
            .borrow_mut()
            .push("ProviderCatalogRemovalPending".to_owned());
        Ok(())
    }

    fn accept_provider_catalog_removal(
        &self,
        input: AcceptProviderCatalogRemovalInputDto,
    ) -> DtoResult<()> {
        let mut state = self.state.borrow_mut();
        let candidate = state
            .removal_candidates
            .iter_mut()
            .find(|candidate| candidate.candidate_handle == input.candidate_handle)
            .ok_or_else(|| {
                not_found(
                    "provider_catalog_removal_not_found",
                    "the requested provider catalog removal candidate does not exist",
                )
            })?;
        if candidate.status != ProviderCatalogRemovalStatusDto::Pending {
            return Err(conflict(
                "provider_catalog_removal_not_pending",
                "the provider catalog removal candidate is no longer pending",
            ));
        }
        candidate.status = ProviderCatalogRemovalStatusDto::Accepted;
        candidate.operation_id = Some(input.operation_id);
        candidate.completed_at = Some(input.accepted_at);
        self.audits
            .borrow_mut()
            .push("ProviderCatalogRemovalAccepted".to_owned());
        Ok(())
    }

    fn reject_provider_catalog_removal(
        &self,
        input: RejectProviderCatalogRemovalInputDto,
    ) -> DtoResult<()> {
        let mut state = self.state.borrow_mut();
        let candidate = state
            .removal_candidates
            .iter_mut()
            .find(|candidate| candidate.candidate_handle == input.candidate_handle)
            .ok_or_else(|| {
                not_found(
                    "provider_catalog_removal_not_found",
                    "the requested provider catalog removal candidate does not exist",
                )
            })?;
        if candidate.status != ProviderCatalogRemovalStatusDto::Pending {
            return Err(conflict(
                "provider_catalog_removal_not_pending",
                "the provider catalog removal candidate is no longer pending",
            ));
        }
        candidate.status = ProviderCatalogRemovalStatusDto::Rejected;
        candidate.operation_id = Some(input.operation_id);
        candidate.completed_at = Some(input.rejected_at);
        state.status = if state.active_catalog_revision_id.is_some() {
            DurableCatalogStatusDto::Active
        } else {
            DurableCatalogStatusDto::Preparing
        };
        state.updated_at = input.rejected_at;
        self.audits
            .borrow_mut()
            .push("ProviderCatalogCandidateRejected".to_owned());
        Ok(())
    }

    fn expire_provider_catalog_removal_candidate(
        &self,
        input: ExpireProviderCatalogRemovalCandidateInputDto,
    ) -> DtoResult<u64> {
        let mut state = self.state.borrow_mut();
        let mut expired = 0_u64;
        for candidate in state.removal_candidates.iter_mut() {
            if candidate.status == ProviderCatalogRemovalStatusDto::Pending
                && candidate.expires_at <= input.now
            {
                candidate.status = ProviderCatalogRemovalStatusDto::Expired;
                candidate.operation_id = Some(input.operation_id.clone());
                candidate.completed_at = Some(input.now);
                self.audits
                    .borrow_mut()
                    .push("ProviderCatalogCandidateExpired".to_owned());
                expired = expired.saturating_add(1);
            }
        }
        if expired > 0 {
            state.status = if state.active_catalog_revision_id.is_some() {
                DurableCatalogStatusDto::Active
            } else {
                DurableCatalogStatusDto::Preparing
            };
            state.updated_at = input.now;
        }
        Ok(expired)
    }
}

// By-value trait implementations so `&FakeCatalog` coerces to the trait
// objects taken by the session-selection services; the reference
// implementations above own the in-memory behavior.
impl ProviderCatalogRepositoryDto for FakeCatalog {
    fn append_provider_kind_descriptor_revision(
        &self,
        input: AppendProviderKindDescriptorRevisionInputDto,
    ) -> DtoResult<()> {
        <&Self as ProviderCatalogRepositoryDto>::append_provider_kind_descriptor_revision(
            &self, input,
        )
    }

    fn append_provider_profile_revision(
        &self,
        input: AppendProviderProfileRevisionInputDto,
    ) -> DtoResult<()> {
        <&Self as ProviderCatalogRepositoryDto>::append_provider_profile_revision(&self, input)
    }

    fn load_provider_catalog_status(&self) -> DtoResult<ProviderCatalogStateDto> {
        <&Self as ProviderCatalogRepositoryDto>::load_provider_catalog_status(&self)
    }

    fn load_provider_catalog_page(
        &self,
        input: LoadProviderCatalogPageInputDto,
    ) -> DtoResult<ProviderCatalogPageDto> {
        <&Self as ProviderCatalogRepositoryDto>::load_provider_catalog_page(&self, input)
    }

    fn accept_provider_catalog(&self, input: AcceptProviderCatalogInputDto) -> DtoResult<()> {
        <&Self as ProviderCatalogRepositoryDto>::accept_provider_catalog(&self, input)
    }

    fn reject_provider_catalog_candidate(
        &self,
        input: RejectProviderCatalogCandidateInputDto,
    ) -> DtoResult<()> {
        <&Self as ProviderCatalogRepositoryDto>::reject_provider_catalog_candidate(&self, input)
    }

    fn expire_provider_catalog_candidate(
        &self,
        input: ExpireProviderCatalogCandidateInputDto,
    ) -> DtoResult<()> {
        <&Self as ProviderCatalogRepositoryDto>::expire_provider_catalog_candidate(&self, input)
    }

    fn load_provider_catalog_material(&self) -> DtoResult<ProviderCatalogMaterialDto> {
        <&Self as ProviderCatalogRepositoryDto>::load_provider_catalog_material(&self)
    }
}

impl ProviderRemovalRepositoryDto for FakeCatalog {
    fn create_provider_catalog_removal_candidate(
        &self,
        input: CreateProviderCatalogRemovalCandidateInputDto,
    ) -> DtoResult<()> {
        <&Self as ProviderRemovalRepositoryDto>::create_provider_catalog_removal_candidate(
            &self, input,
        )
    }

    fn accept_provider_catalog_removal(
        &self,
        input: AcceptProviderCatalogRemovalInputDto,
    ) -> DtoResult<()> {
        <&Self as ProviderRemovalRepositoryDto>::accept_provider_catalog_removal(&self, input)
    }

    fn reject_provider_catalog_removal(
        &self,
        input: RejectProviderCatalogRemovalInputDto,
    ) -> DtoResult<()> {
        <&Self as ProviderRemovalRepositoryDto>::reject_provider_catalog_removal(&self, input)
    }

    fn expire_provider_catalog_removal_candidate(
        &self,
        input: ExpireProviderCatalogRemovalCandidateInputDto,
    ) -> DtoResult<u64> {
        <&Self as ProviderRemovalRepositoryDto>::expire_provider_catalog_removal_candidate(
            &self, input,
        )
    }
}

fn seeded_catalog() -> FakeCatalog {
    let fake = FakeCatalog::new();
    fake.seed_active(
        1,
        vec![seed_kind_candidate("responses", "bearer")],
        vec![seed_profile(
            "responses",
            "model-a",
            "default",
            "rev-0001",
            "kind-responses-v1",
        )],
        "default",
    );
    fake
}

// ============================================================================
// Session-selection storage fakes
// ============================================================================

struct FakeDefaults {
    defaults: RefCell<HashMap<SessionId, SessionProviderDefaultDto>>,
    injected_failure: RefCell<Option<ErrorDto>>,
}

impl FakeDefaults {
    fn new() -> Self {
        Self {
            defaults: RefCell::new(HashMap::new()),
            injected_failure: RefCell::new(None),
        }
    }

    fn fail_with(&self, error: ErrorDto) {
        *self.injected_failure.borrow_mut() = Some(error);
    }

    fn seed(&self, session_id: SessionId, profile_id: &str, projection_revision: u64) {
        self.defaults.borrow_mut().insert(
            session_id,
            SessionProviderDefaultDto {
                session_id,
                profile_id: profile_id.to_owned(),
                projection_revision,
                last_operation_id: "seeded".to_owned(),
                updated_at: 1,
            },
        );
    }
}

impl SessionProviderDefaultsRepositoryDto for FakeDefaults {
    fn get_session_provider_profile(
        &self,
        session_id: SessionId,
    ) -> DtoResult<Option<SessionProviderDefaultDto>> {
        Ok(self.defaults.borrow().get(&session_id).cloned())
    }

    fn set_session_provider_profile(
        &self,
        input: SetSessionProviderProfileInputDto,
    ) -> DtoResult<SetSessionProviderProfileOutcomeDto> {
        if let Some(error) = self.injected_failure.borrow().clone() {
            return Err(error);
        }
        let mut defaults = self.defaults.borrow_mut();
        let existing = defaults.get(&input.session_id).cloned();
        match existing {
            Some(existing) => {
                if existing.last_operation_id == input.operation_id {
                    return Ok(SetSessionProviderProfileOutcomeDto {
                        changed: false,
                        projection_revision: existing.projection_revision,
                    });
                }
                if existing.projection_revision != input.expected_projection_revision {
                    return Err(ErrorDto::validation(
                        "session_provider_default_stale",
                        "the session provider default changed concurrently",
                    ));
                }
                if existing.profile_id == input.profile_id {
                    defaults.insert(
                        input.session_id,
                        SessionProviderDefaultDto {
                            session_id: input.session_id,
                            profile_id: existing.profile_id.clone(),
                            projection_revision: existing.projection_revision,
                            last_operation_id: input.operation_id,
                            updated_at: input.updated_at,
                        },
                    );
                    return Ok(SetSessionProviderProfileOutcomeDto {
                        changed: false,
                        projection_revision: existing.projection_revision,
                    });
                }
                let projection_revision = existing.projection_revision.saturating_add(1);
                defaults.insert(
                    input.session_id,
                    SessionProviderDefaultDto {
                        session_id: input.session_id,
                        profile_id: input.profile_id,
                        projection_revision,
                        last_operation_id: input.operation_id,
                        updated_at: input.updated_at,
                    },
                );
                Ok(SetSessionProviderProfileOutcomeDto {
                    changed: true,
                    projection_revision,
                })
            }
            None => {
                defaults.insert(
                    input.session_id,
                    SessionProviderDefaultDto {
                        session_id: input.session_id,
                        profile_id: input.profile_id,
                        projection_revision: input.expected_projection_revision,
                        last_operation_id: input.operation_id,
                        updated_at: input.updated_at,
                    },
                );
                Ok(SetSessionProviderProfileOutcomeDto {
                    changed: true,
                    projection_revision: input.expected_projection_revision,
                })
            }
        }
    }
}

struct FakeAdmissionPort {
    profiles: RefCell<HashMap<String, DtoResult<ResolvedProfileDto>>>,
    verify_registry_key_fails: bool,
}

impl FakeAdmissionPort {
    fn new() -> Self {
        Self {
            profiles: RefCell::new(HashMap::new()),
            verify_registry_key_fails: false,
        }
    }

    fn with(profile_id: &str, result: DtoResult<ResolvedProfileDto>) -> Self {
        let port = Self::new();
        port.add(profile_id, result);
        port
    }

    fn add(&self, profile_id: &str, result: DtoResult<ResolvedProfileDto>) {
        self.profiles
            .borrow_mut()
            .insert(profile_id.to_owned(), result);
    }

    const fn fail_registry_verification(&mut self) {
        self.verify_registry_key_fails = true;
    }
}

impl CatalogAdmissionPort for FakeAdmissionPort {
    fn resolve_enabled_profile(&self, profile_id: &str) -> DtoResult<ResolvedProfileDto> {
        self.profiles
            .borrow()
            .get(profile_id)
            .cloned()
            .unwrap_or_else(|| {
                Err(ErrorDto::unavailable(
                    "provider_profile_unavailable",
                    "the provider profile is unavailable",
                ))
            })
    }

    fn verify_registry_key(
        &self,
        _profile_id: &str,
        _provider_profile_revision_id: &str,
        _kind_descriptor_revision_id: &str,
        _driver_contract_revision: &str,
    ) -> DtoResult<()> {
        if self.verify_registry_key_fails {
            Err(ErrorDto::unavailable(
                "provider_profile_unavailable",
                "the exact registry key is not admitted",
            ))
        } else {
            Ok(())
        }
    }
}

struct FakeReadinessPort {
    readiness: RefCell<CatalogReadiness>,
}

impl FakeReadinessPort {
    const fn new(readiness: CatalogReadiness) -> Self {
        Self {
            readiness: RefCell::new(readiness),
        }
    }
}

impl ControlPlaneReadinessPort for FakeReadinessPort {
    fn readiness(&self) -> DtoResult<CatalogReadiness> {
        Ok(self.readiness.borrow().clone())
    }
}

struct FakeQueue {
    entries: RefCell<Vec<UnavailableRunQueueEntryDto>>,
    marker: RefCell<Option<QueueReconciliationMarkerDto>>,
    next_queue_id: RefCell<i64>,
}

impl FakeQueue {
    const fn new() -> Self {
        Self {
            entries: RefCell::new(Vec::new()),
            marker: RefCell::new(None),
            next_queue_id: RefCell::new(1),
        }
    }

    fn seed(
        &self,
        run_id: RunId,
        session_id: SessionId,
        profile_id: &str,
        unavailable_reason: &str,
    ) {
        let queue_id = *self.next_queue_id.borrow();
        *self.next_queue_id.borrow_mut() += 1;
        self.entries.borrow_mut().push(UnavailableRunQueueEntryDto {
            queue_id,
            run_id,
            session_id,
            profile_id: profile_id.to_owned(),
            provider_profile_revision_id: "rev-0001".to_owned(),
            unavailable_reason: unavailable_reason.to_owned(),
            first_unavailable_at: 1,
            promotion_attempts: 0,
            state: UnavailableQueueStateDto::Queued,
            last_operation_id: None,
            selection_json: String::new(),
        });
    }
}

impl UnavailableQueueRepositoryDto for FakeQueue {
    fn enqueue_unavailable_run(&self, input: EnqueueUnavailableRunInputDto) -> DtoResult<()> {
        let queue_id = *self.next_queue_id.borrow();
        *self.next_queue_id.borrow_mut() += 1;
        self.entries.borrow_mut().push(UnavailableRunQueueEntryDto {
            queue_id,
            run_id: input.run_id,
            session_id: input.session_id,
            profile_id: input.profile_id,
            provider_profile_revision_id: input.provider_profile_revision_id,
            unavailable_reason: input.unavailable_reason,
            first_unavailable_at: input.first_unavailable_at,
            promotion_attempts: 0,
            state: UnavailableQueueStateDto::Queued,
            last_operation_id: Some(input.operation_id),
            selection_json: input.selection_json,
        });
        Ok(())
    }

    fn load_unavailable_queue_page(
        &self,
        input: LoadUnavailableQueuePageInputDto,
    ) -> DtoResult<Vec<UnavailableRunQueueEntryDto>> {
        let entries = self
            .entries
            .borrow()
            .iter()
            .filter(|entry| {
                entry.state == UnavailableQueueStateDto::Queued
                    && input
                        .after_queue_id
                        .is_none_or(|after| entry.queue_id > after)
            })
            .cloned()
            .collect();
        Ok(entries)
    }

    fn promote_unavailable_runs(
        &self,
        input: PromoteUnavailableRunsInputDto,
    ) -> DtoResult<PromoteUnavailableRunsOutcomeDto> {
        let mut entries = self.entries.borrow_mut();
        let max = usize::try_from(input.max).unwrap_or(usize::MAX);
        let mut promoted = Vec::new();
        let mut remaining = Vec::new();
        for mut entry in std::mem::take(&mut *entries) {
            if entry.state == UnavailableQueueStateDto::Queued && promoted.len() < max {
                entry.state = UnavailableQueueStateDto::Promoted;
                entry.last_operation_id = Some(input.operation_id.clone());
                entry.promotion_attempts = entry.promotion_attempts.saturating_add(1);
                promoted.push(entry);
            } else {
                remaining.push(entry);
            }
        }
        *entries = remaining;
        let marker_created = !promoted.is_empty();
        if marker_created {
            *self.marker.borrow_mut() = Some(QueueReconciliationMarkerDto {
                marker_id: 1,
                session_id: promoted[0].session_id,
                created_at: input.now,
                reason: "queue_exhausted".to_owned(),
                next_page_cursor: Some("next-page-1".to_owned()),
                resolved_at: None,
            });
        }
        Ok(PromoteUnavailableRunsOutcomeDto {
            promoted,
            reconciliation_marker_created: marker_created,
        })
    }

    fn reconcile_unavailable_queue(
        &self,
        input: ReconcileUnavailableQueueInputDto,
    ) -> DtoResult<ReconcileUnavailableQueueOutcomeDto> {
        let mut entries = self.entries.borrow_mut();
        let max = usize::try_from(input.max).unwrap_or(usize::MAX);
        let mut processed = Vec::new();
        let mut terminalized = Vec::new();
        let mut remaining = Vec::new();
        for mut entry in std::mem::take(&mut *entries) {
            processed.push(entry.clone());
            if entry.state == UnavailableQueueStateDto::Queued
                && terminalized.len() < max
                && entry.unavailable_reason.starts_with("terminal-")
            {
                entry.state = UnavailableQueueStateDto::Terminalized;
                entry.last_operation_id = Some(input.operation_id.clone());
                terminalized.push(entry.clone());
            }
            remaining.push(entry);
        }
        *entries = remaining;
        Ok(ReconcileUnavailableQueueOutcomeDto {
            processed,
            terminalized,
        })
    }

    fn load_queue_reconciliation_marker(
        &self,
        _session_id: SessionId,
    ) -> DtoResult<Option<QueueReconciliationMarkerDto>> {
        Ok(self.marker.borrow().clone())
    }
}

struct FakeUsage {
    records: RefCell<Vec<RecordProviderUsageInputDto>>,
    aggregates: RefCell<Vec<ProviderUsageAggregateDto>>,
}

impl FakeUsage {
    const fn new() -> Self {
        Self {
            records: RefCell::new(Vec::new()),
            aggregates: RefCell::new(Vec::new()),
        }
    }

    fn seed_aggregate(
        &self,
        profile_id: &str,
        revision_id: &str,
        model_id: &str,
        period_start: i64,
        period_end: i64,
    ) {
        self.aggregates
            .borrow_mut()
            .push(ProviderUsageAggregateDto {
                profile_id: profile_id.to_owned(),
                provider_profile_revision_id: revision_id.to_owned(),
                model_id: model_id.to_owned(),
                usage_period_start: period_start,
                usage_period_end: period_end,
                request_count: 5,
                input_units: 10,
                output_units: 20,
                reasoning_units: 30,
                last_run_id: None,
                updated_at: period_end,
            });
    }
}

impl ProviderUsageRepositoryDto for FakeUsage {
    fn record_provider_usage(&self, input: RecordProviderUsageInputDto) -> DtoResult<()> {
        self.records.borrow_mut().push(input);
        Ok(())
    }

    fn load_provider_usage_by_profile(
        &self,
        profile_id: String,
    ) -> DtoResult<Vec<ProviderUsageAggregateDto>> {
        Ok(self
            .aggregates
            .borrow()
            .iter()
            .filter(|aggregate| aggregate.profile_id == profile_id)
            .cloned()
            .collect())
    }

    fn load_provider_usage_by_revision_and_model(
        &self,
        provider_profile_revision_id: String,
        model_id: String,
    ) -> DtoResult<Vec<ProviderUsageAggregateDto>> {
        Ok(self
            .aggregates
            .borrow()
            .iter()
            .filter(|aggregate| {
                aggregate.provider_profile_revision_id == provider_profile_revision_id
                    && aggregate.model_id == model_id
            })
            .cloned()
            .collect())
    }
}

struct FakeHeld {
    held: RefCell<HashMap<RunId, HeldRecoveredRunDto>>,
}

impl FakeHeld {
    fn new() -> Self {
        Self {
            held: RefCell::new(HashMap::new()),
        }
    }

    fn seed(&self, record: HeldRecoveredRunDto) {
        self.held.borrow_mut().insert(record.run_id, record);
    }
}

impl HeldRunRepositoryDto for FakeHeld {
    fn mark_recovered_run_held(
        &self,
        input: intention_storage::MarkRecoveredRunHeldInputDto,
    ) -> DtoResult<()> {
        self.held.borrow_mut().insert(
            input.run_id,
            HeldRecoveredRunDto {
                run_id: input.run_id,
                session_id: input.session_id,
                held_at: input.held_at,
                reason: "unavailable".to_owned(),
                admission_state: HeldRunAdmissionStateDto::Held,
                admission_operation_id: Some(input.operation_id),
                admitted_at: None,
            },
        );
        Ok(())
    }

    fn admit_held_recovered_run(&self, input: AdmitHeldRecoveredRunInputDto) -> DtoResult<()> {
        let mut held = self.held.borrow_mut();
        let Some(record) = held.get_mut(&input.run_id) else {
            return Err(not_found(
                "held_run_not_found",
                "the held recovered run does not exist",
            ));
        };
        if record.admission_state == HeldRunAdmissionStateDto::Rejected {
            return Err(conflict(
                "held_run_rejected",
                "the held recovered run was already rejected",
            ));
        }
        record.admission_state = HeldRunAdmissionStateDto::Admitted;
        record.admission_operation_id = Some(input.operation_id);
        record.admitted_at = Some(input.admitted_at);
        Ok(())
    }

    fn load_held_recovered_run(&self, run_id: RunId) -> DtoResult<Option<HeldRecoveredRunDto>> {
        Ok(self.held.borrow().get(&run_id).cloned())
    }
}

struct FakeSelections {
    selections: RefCell<HashMap<(SessionId, RunId), ProviderSelectionV1>>,
    fail_load: bool,
}

impl FakeSelections {
    fn new() -> Self {
        Self {
            selections: RefCell::new(HashMap::new()),
            fail_load: false,
        }
    }

    fn seed(&self, session_id: SessionId, run_id: RunId, selection: ProviderSelectionV1) {
        self.selections
            .borrow_mut()
            .insert((session_id, run_id), selection);
    }
}

impl ProviderSelectionRepositoryDto for FakeSelections {
    fn persist_resolved_run_provider_selection(
        &self,
        input: PersistResolvedRunProviderSelectionInputDto,
    ) -> DtoResult<()> {
        self.selections
            .borrow_mut()
            .insert((input.session_id, input.run_id), input.selection);
        Ok(())
    }

    fn load_resolved_run_provider_selection(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<Option<ProviderSelectionV1>> {
        if self.fail_load {
            return Err(ErrorDto::unavailable(
                "storage_decode_failed",
                "the persisted selection record is malformed",
            ));
        }
        Ok(self.selections.borrow().get(&(session_id, run_id)).cloned())
    }
}

struct FakeDispatch {
    calls: RefCell<Vec<ScheduleModelRunDto>>,
}

impl FakeDispatch {
    const fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.borrow().len()
    }
}

impl ModelRunDispatchPort for FakeDispatch {
    fn dispatch_model_run(&self, input: ScheduleModelRunDto) -> DtoResult<()> {
        self.calls.borrow_mut().push(input);
        Ok(())
    }
}

// ============================================================================
// ApplicationService repository fake (provider_selection_from coverage)
// ============================================================================

struct FakeAppRepo {
    catalog: FakeCatalog,
    defaults: FakeDefaults,
    accepted: RefCell<Option<AcceptUserTurnInputDto>>,
    change: CommittedChangeDto,
}

impl FakeAppRepo {
    const fn new(catalog: FakeCatalog, defaults: FakeDefaults, change: CommittedChangeDto) -> Self {
        Self {
            catalog,
            defaults,
            accepted: RefCell::new(None),
            change,
        }
    }
}

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
        .expect("fixture workspace is valid")
}

fn queued_change(session_id: SessionId) -> CommittedChangeDto {
    let projection = SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
        None,
        None,
        Vec::new(),
        SessionEventSequenceDto::new(3),
    )
    .expect("fixture projection is valid");
    CommittedChangeDto::new(
        projection.clone(),
        projection.at_sequence(),
        Vec::new(),
        Some(AcceptedTurnOutcomeDto::Queued(QueuePositionDto::new(0))),
    )
    .expect("fixture queued change is valid")
}

impl StorageRepositoryDto for FakeAppRepo {
    fn create_session(&self, _input: CreateSessionInputDto) -> DtoResult<CommittedChangeDto> {
        Err(unavailable("fixture_unused", "session storage is unused"))
    }

    fn accept_user_turn(&self, input: AcceptUserTurnInputDto) -> DtoResult<CommittedChangeDto> {
        *self.accepted.borrow_mut() = Some(input);
        Ok(self.change.clone())
    }

    fn remove_queued_turn(
        &self,
        _input: RemoveQueuedTurnInputDto,
    ) -> DtoResult<CommittedChangeDto> {
        Err(unavailable("fixture_unused", "turn storage is unused"))
    }

    fn transition_run(&self, _input: TransitionRunInputDto) -> DtoResult<CommittedChangeDto> {
        Err(unavailable("fixture_unused", "run storage is unused"))
    }

    fn recover_unfinished_runs(
        &self,
        _input: RecoverUnfinishedRunsInputDto,
    ) -> DtoResult<Vec<CommittedChangeDto>> {
        Err(unavailable("fixture_unused", "recovery storage is unused"))
    }

    fn load_session_snapshot(&self, _session_id: SessionId) -> DtoResult<SessionProjectionDto> {
        Err(unavailable("fixture_unused", "session storage is unused"))
    }

    fn load_tail(
        &self,
        _session_id: SessionId,
        _after_sequence: SessionEventSequenceDto,
    ) -> DtoResult<Vec<EventEnvelopeDto<DomainEventDto>>> {
        Err(unavailable("fixture_unused", "tail storage is unused"))
    }

    fn accept_configuration_revision(&self, _snapshot: ConfigSnapshotDto) -> DtoResult<()> {
        Err(unavailable("fixture_unused", "config storage is unused"))
    }
}

impl SessionProviderDefaultsRepositoryDto for FakeAppRepo {
    fn get_session_provider_profile(
        &self,
        session_id: SessionId,
    ) -> DtoResult<Option<SessionProviderDefaultDto>> {
        self.defaults.get_session_provider_profile(session_id)
    }

    fn set_session_provider_profile(
        &self,
        input: SetSessionProviderProfileInputDto,
    ) -> DtoResult<SetSessionProviderProfileOutcomeDto> {
        self.defaults.set_session_provider_profile(input)
    }
}

impl ProviderCatalogRepositoryDto for FakeAppRepo {
    fn append_provider_kind_descriptor_revision(
        &self,
        input: AppendProviderKindDescriptorRevisionInputDto,
    ) -> DtoResult<()> {
        self.catalog.append_provider_kind_descriptor_revision(input)
    }

    fn append_provider_profile_revision(
        &self,
        input: AppendProviderProfileRevisionInputDto,
    ) -> DtoResult<()> {
        self.catalog.append_provider_profile_revision(input)
    }

    fn load_provider_catalog_status(&self) -> DtoResult<ProviderCatalogStateDto> {
        self.catalog.load_provider_catalog_status()
    }

    fn load_provider_catalog_page(
        &self,
        input: LoadProviderCatalogPageInputDto,
    ) -> DtoResult<ProviderCatalogPageDto> {
        self.catalog.load_provider_catalog_page(input)
    }

    fn accept_provider_catalog(&self, input: AcceptProviderCatalogInputDto) -> DtoResult<()> {
        self.catalog.accept_provider_catalog(input)
    }

    fn reject_provider_catalog_candidate(
        &self,
        input: RejectProviderCatalogCandidateInputDto,
    ) -> DtoResult<()> {
        self.catalog.reject_provider_catalog_candidate(input)
    }

    fn expire_provider_catalog_candidate(
        &self,
        input: ExpireProviderCatalogCandidateInputDto,
    ) -> DtoResult<()> {
        self.catalog.expire_provider_catalog_candidate(input)
    }

    fn load_provider_catalog_material(&self) -> DtoResult<ProviderCatalogMaterialDto> {
        self.catalog.load_provider_catalog_material()
    }
}

// ============================================================================
// Shared session/run/schedule fixtures
// ============================================================================

fn command(
    profile_override: Option<&str>,
    expected_revision: Option<&str>,
) -> SendUserTurnCommandDto {
    let base = SendUserTurnCommandDto::new(SessionId::new(), TurnId::new(), "hello")
        .expect("fixture command is valid");
    match profile_override {
        Some(profile) => base
            .with_profile_override(profile, expected_revision.map(str::to_owned))
            .expect("fixture override is valid"),
        None => base,
    }
}

fn schedule(session_id: SessionId, run_id: RunId) -> ScheduleModelRunDto {
    let request = ModelRequestDto::new(
        run_id,
        "fixture",
        vec![ModelMessageDto::new(ModelRoleDto::User, "first").expect("fixture message is valid")],
        None,
        None,
    )
    .expect("fixture request is valid");
    ScheduleModelRunDto::new(session_id, run_id, request, fixture_snapshot())
        .expect("fixture schedule is valid")
}

fn held_record(
    run_id: RunId,
    session_id: SessionId,
    state: HeldRunAdmissionStateDto,
    operation_id: Option<&str>,
) -> HeldRecoveredRunDto {
    HeldRecoveredRunDto {
        run_id,
        session_id,
        held_at: 100,
        reason: "unavailable".to_owned(),
        admission_state: state,
        admission_operation_id: operation_id.map(str::to_owned),
        admitted_at: None,
    }
}

fn admit_command(
    session_id: SessionId,
    run_id: RunId,
    operation_id: &str,
) -> AdmitRecoveredRunCommandDto {
    AdmitRecoveredRunCommandDto {
        session_id: session_id.to_string(),
        run_id: run_id.to_string(),
        operation_id: operation_id.to_owned(),
    }
}

type RemovalController<'a> = ProviderCatalogController<&'a FakeCatalog, &'a FakeCatalog>;

fn removal_controller(
    fake: &FakeCatalog,
    now: u64,
) -> (
    RemovalController<'_>,
    intention_application::CatalogCandidateOutcomeDto,
) {
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let controller = fake.build_controller(both_factories(Arc::new(AtomicUsize::new(0))));
    controller.startup(1_000).expect("startup succeeds");
    let outcome = controller
        .prepare_candidate(
            source(
                "op-removal",
                1_024,
                vec![declaration(
                    "generic-chat-completion-api",
                    "model-b",
                    Some(ENDPOINT),
                    true,
                )],
                candidate(
                    "generic-chat-completion-api",
                    "model-b",
                    ENDPOINT,
                    &previous,
                ),
                previous,
            ),
            now,
        )
        .expect("removal candidate prepares");
    (controller, outcome)
}

// ============================================================================
// Degraded-mode gate
// ============================================================================

#[test]
fn degraded_mode_allows_ready_uninitialized_and_loading() {
    let service = DegradedModeService;
    assert!(
        service
            .assert_execution_ready(&CatalogReadiness::Ready)
            .is_ok()
    );
    assert!(
        service
            .assert_execution_ready(&CatalogReadiness::Uninitialized)
            .is_ok()
    );
    assert!(
        service
            .assert_execution_ready(&CatalogReadiness::Loading)
            .is_ok()
    );
}

#[test]
fn degraded_mode_rejects_blocked_pending_removal_and_recovery_required() {
    let service = DegradedModeService;
    for readiness in [
        CatalogReadiness::Blocked {
            reason: "removal_candidate_rejected".to_owned(),
        },
        CatalogReadiness::PendingRemoval {
            candidate_revision: "2".to_owned(),
            expires_at: 1_000,
        },
        CatalogReadiness::ActivationRecoveryRequired {
            accepted_revision: "1".to_owned(),
        },
    ] {
        let error = service
            .assert_execution_ready(&readiness)
            .expect_err("degraded states are rejected");
        assert_eq!(error.code(), "execution_not_ready");
    }
}

// ============================================================================
// provider_selection_from through the live user-turn variant
// ============================================================================

#[test]
fn provider_selection_from_attaches_the_resolved_selection_to_the_durable_input() {
    let session_id = SessionId::new();
    let catalog = seeded_catalog();
    let defaults = FakeDefaults::new();
    let repo = FakeAppRepo::new(catalog, defaults, queued_change(session_id));
    let dispatch = FakeDispatch::new();
    let port = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let turn = SendUserTurnCommandDto::new(session_id, TurnId::new(), "hello")
        .expect("fixture command is valid")
        .with_profile_override("default", None)
        .expect("fixture override is valid");
    let result = ApplicationService::new(&repo)
        .send_user_turn_and_schedule_with_provider_selection(
            turn,
            SendUserTurnWorkflowInputDto::new(RunId::new(), fixture_snapshot(), time()),
            &port,
            &dispatch,
        )
        .expect("selected turn acceptance succeeds");
    assert!(matches!(
        result,
        intention_protocol::ProtocolAcceptedResultDto::SendUserTurn(_)
    ));
    let accepted_borrow = repo.accepted.borrow();
    let accepted_ref = accepted_borrow
        .as_ref()
        .expect("durable input was recorded");
    let attached = accepted_ref
        .provider_selection()
        .expect("resolved selection is attached");
    assert_eq!(attached.profile_id, "default");
    assert_eq!(attached.provider_profile_revision_id, "rev-0001");
    assert_eq!(
        attached.selection_source.as_deref(),
        Some("per_turn_override")
    );
    assert_eq!(
        dispatch.call_count(),
        0,
        "a queued outcome never enters the scheduling path"
    );
}

#[test]
fn provider_selection_from_maps_domain_validation_failure_to_provider_profile_revision_invalid() {
    let session_id = SessionId::new();
    let catalog = seeded_catalog();
    let defaults = FakeDefaults::new();
    let repo = FakeAppRepo::new(catalog, defaults, queued_change(session_id));
    let dispatch = FakeDispatch::new();
    // The resolved profile id passes the protocol bound but violates the
    // canonical domain 63-character bound, so the failure is raised by the
    // durable selection validation inside provider_selection_from.
    let over_long = "p".repeat(100);
    let port = FakeAdmissionPort::with("default", Ok(resolved_profile(&over_long, "rev-0001")));
    let turn = SendUserTurnCommandDto::new(session_id, TurnId::new(), "hello")
        .expect("fixture command is valid")
        .with_profile_override("default", None)
        .expect("fixture override is valid");
    let error = ApplicationService::new(&repo)
        .send_user_turn_and_schedule_with_provider_selection(
            turn,
            SendUserTurnWorkflowInputDto::new(RunId::new(), fixture_snapshot(), time()),
            &port,
            &dispatch,
        )
        .expect_err("over-bound profile id is rejected");
    assert_eq!(error.code(), "provider_profile_revision_invalid");
    assert!(repo.accepted.borrow().is_none());
    assert_eq!(dispatch.call_count(), 0);
}

#[test]
fn provider_selection_from_preserves_the_safe_header_transport() {
    let session_id = SessionId::new();
    let catalog = seeded_catalog();
    let defaults = FakeDefaults::new();
    let repo = FakeAppRepo::new(catalog, defaults, queued_change(session_id));
    let dispatch = FakeDispatch::new();
    let port = FakeAdmissionPort::with("default", Ok(safe_header_profile("default", "rev-0001")));
    let turn = SendUserTurnCommandDto::new(session_id, TurnId::new(), "hello")
        .expect("fixture command is valid")
        .with_profile_override("default", None)
        .expect("fixture override is valid");
    ApplicationService::new(&repo)
        .send_user_turn_and_schedule_with_provider_selection(
            turn,
            SendUserTurnWorkflowInputDto::new(RunId::new(), fixture_snapshot(), time()),
            &port,
            &dispatch,
        )
        .expect("safe-header selection acceptance succeeds");
    let accepted_borrow = repo.accepted.borrow();
    let attached = accepted_borrow
        .as_ref()
        .expect("durable input was recorded")
        .provider_selection()
        .expect("resolved selection is attached");
    assert_eq!(
        attached.credential_transport_mode,
        DomainCredentialTransportMode::SafeHeader
    );
    assert_eq!(
        attached.credential_transport_safe_header_name.as_deref(),
        Some("x-auth-header")
    );
    assert_eq!(dispatch.call_count(), 0);
}

// ============================================================================
// Selection resolution
// ============================================================================

#[test]
fn resolve_for_turn_uses_the_per_turn_override() {
    let port = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let resolved = SelectionResolutionService
        .resolve_for_turn(
            &command(Some("default"), None),
            Some("session-default".to_owned()),
            Some("global-default".to_owned()),
            &port,
        )
        .expect("override resolves");
    assert_eq!(resolved.profile_id, "default");
    assert_eq!(
        resolved.selection_source.as_deref(),
        Some("per_turn_override")
    );
}

#[test]
fn resolve_for_turn_uses_the_session_default_when_no_override() {
    let port = FakeAdmissionPort::with(
        "session-default",
        Ok(resolved_profile("session-default", "rev-0002")),
    );
    let resolved = SelectionResolutionService
        .resolve_for_turn(
            &command(None, None),
            Some("session-default".to_owned()),
            Some("global-default".to_owned()),
            &port,
        )
        .expect("session default resolves");
    assert_eq!(resolved.profile_id, "session-default");
    assert_eq!(
        resolved.selection_source.as_deref(),
        Some("session_default")
    );
}

#[test]
fn resolve_for_turn_uses_the_global_default_when_no_override_or_session_default() {
    let port = FakeAdmissionPort::with(
        "global-default",
        Ok(resolved_profile("global-default", "rev-0003")),
    );
    let resolved = SelectionResolutionService
        .resolve_for_turn(
            &command(None, None),
            None,
            Some("global-default".to_owned()),
            &port,
        )
        .expect("global default resolves");
    assert_eq!(resolved.profile_id, "global-default");
    assert_eq!(resolved.selection_source.as_deref(), Some("global_default"));
}

#[test]
fn resolve_for_turn_rejects_when_no_profile_applies() {
    let port = FakeAdmissionPort::new();
    let error = SelectionResolutionService
        .resolve_for_turn(&command(None, None), None, None, &port)
        .expect_err("a turn without any applicable profile is rejected");
    assert_eq!(error.code(), "provider_profile_runtime_unavailable");
    assert_eq!(error.category(), ErrorCategoryDto::Unavailable);
}

#[test]
fn resolve_for_turn_rejects_an_expected_revision_mismatch() {
    let port = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let error = SelectionResolutionService
        .resolve_for_turn(
            &command(Some("default"), Some("rev-0009")),
            None,
            None,
            &port,
        )
        .expect_err("revision mismatch is rejected");
    assert_eq!(error.code(), "provider_profile_revision_mismatch");
}

#[test]
fn resolve_for_turn_maps_admission_failures_to_runtime_unavailable() {
    let port = FakeAdmissionPort::with(
        "default",
        Err(ErrorDto::unavailable(
            "provider_admission_not_found",
            "the resolved provider profile is not admitted",
        )),
    );
    let error = SelectionResolutionService
        .resolve_for_turn(&command(Some("default"), None), None, None, &port)
        .expect_err("admission failure is mapped");
    assert_eq!(error.code(), "provider_profile_runtime_unavailable");
    assert_eq!(error.category(), ErrorCategoryDto::Unavailable);
}

#[test]
fn resolve_for_turn_passes_through_unrelated_port_errors() {
    let port = FakeAdmissionPort::with(
        "default",
        Err(ErrorDto::unavailable(
            "execution_not_ready",
            "the provider control plane is degraded",
        )),
    );
    let error = SelectionResolutionService
        .resolve_for_turn(&command(Some("default"), None), None, None, &port)
        .expect_err("unrelated error passes through");
    assert_eq!(error.code(), "execution_not_ready");
}

// ============================================================================
// Session provider profiles
// ============================================================================

fn set_command(
    session_id: SessionId,
    profile_id: &str,
    expected_revision: u64,
    operation_id: &str,
) -> SetSessionProviderProfileCommandDto {
    SetSessionProviderProfileCommandDto {
        schema_version: "session-provider-profile-v1".to_owned(),
        session_id: session_id.to_string(),
        profile_id: profile_id.to_owned(),
        expected_session_projection_revision: expected_revision,
        operation_id: operation_id.to_owned(),
    }
}

#[test]
fn set_binds_a_new_session_default_and_reports_changed() {
    let defaults = FakeDefaults::new();
    let catalog = seeded_catalog();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let port = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let service = SessionProfileService::new(&defaults, &catalog, &readiness);
    let accepted = service
        .set(
            set_command(SessionId::new(), "default", 0, "op-bind"),
            &port,
            1_000,
        )
        .expect("binding succeeds");
    assert!(accepted.changed);
    assert_eq!(accepted.resulting_projection_revision, 0);
    assert!(matches!(
        accepted.resolved,
        ResolvedProviderProfileDto::Resolved {
            profile_id,
            profile_revision_id
        } if profile_id == "default" && profile_revision_id == "rev-0001"
    ));
}

#[test]
fn set_same_profile_is_an_idempotent_no_op() {
    let session_id = SessionId::new();
    let defaults = FakeDefaults::new();
    defaults.seed(session_id, "default", 0);
    let catalog = seeded_catalog();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let port = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let service = SessionProfileService::new(&defaults, &catalog, &readiness);
    let accepted = service
        .set(
            set_command(session_id, "default", 0, "op-same"),
            &port,
            1_000,
        )
        .expect("same-profile set succeeds");
    assert!(!accepted.changed);
    assert_eq!(accepted.resulting_projection_revision, 0);
}

#[test]
fn set_rejects_while_the_control_plane_is_degraded() {
    let defaults = FakeDefaults::new();
    let catalog = seeded_catalog();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Blocked {
        reason: "removal_candidate_rejected".to_owned(),
    });
    let port = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let service = SessionProfileService::new(&defaults, &catalog, &readiness);
    let error = service
        .set(
            set_command(SessionId::new(), "default", 0, "op-degraded"),
            &port,
            1_000,
        )
        .expect_err("degraded set is rejected");
    assert_eq!(error.code(), "execution_not_ready");
}

#[test]
fn set_rejects_a_stale_expected_projection_revision() {
    let session_id = SessionId::new();
    let defaults = FakeDefaults::new();
    defaults.seed(session_id, "default", 1);
    let catalog = seeded_catalog();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let port = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let service = SessionProfileService::new(&defaults, &catalog, &readiness);
    let error = service
        .set(
            set_command(session_id, "default", 0, "op-stale"),
            &port,
            1_000,
        )
        .expect_err("stale expected revision is rejected");
    assert_eq!(error.code(), "session_profile_revision_mismatch");
}

#[test]
fn set_rejects_an_invalid_command() {
    let defaults = FakeDefaults::new();
    let catalog = seeded_catalog();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let port = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let service = SessionProfileService::new(&defaults, &catalog, &readiness);
    let mut command = set_command(SessionId::new(), "default", 0, "op-invalid");
    command.session_id.clear();
    let error = service
        .set(command, &port, 1_000)
        .expect_err("blank session id is rejected");
    assert_eq!(error.code(), "set_session_provider_profile_invalid");
}

#[test]
fn set_passes_through_admission_port_failures() {
    let defaults = FakeDefaults::new();
    let catalog = seeded_catalog();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let port = FakeAdmissionPort::with(
        "default",
        Err(ErrorDto::unavailable(
            "provider_profile_unavailable",
            "the provider profile is unavailable",
        )),
    );
    let service = SessionProfileService::new(&defaults, &catalog, &readiness);
    let error = service
        .set(
            set_command(SessionId::new(), "default", 0, "op-admission"),
            &port,
            1_000,
        )
        .expect_err("admission failure passes through");
    assert_eq!(error.code(), "provider_profile_unavailable");
}

#[test]
fn set_passes_through_non_stale_storage_failures() {
    let defaults = FakeDefaults::new();
    defaults.fail_with(ErrorDto::unavailable("storage_down", "storage is down"));
    let catalog = seeded_catalog();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let port = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let service = SessionProfileService::new(&defaults, &catalog, &readiness);
    let error = service
        .set(
            set_command(SessionId::new(), "default", 0, "op-storage"),
            &port,
            1_000,
        )
        .expect_err("non-stale storage failures pass through");
    assert_eq!(error.code(), "storage_down");
}

fn get_query(session_id: SessionId) -> GetSessionProviderProfileQueryDto {
    GetSessionProviderProfileQueryDto {
        schema_version: "session-provider-profile-v1".to_owned(),
        session_id: session_id.to_string(),
    }
}

#[test]
fn get_resolves_the_durable_session_default() {
    let session_id = SessionId::new();
    let defaults = FakeDefaults::new();
    defaults.seed(session_id, "default", 3);
    let catalog = seeded_catalog();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let port = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let service = SessionProfileService::new(&defaults, &catalog, &readiness);
    let projection = service
        .get(get_query(session_id), &port)
        .expect("projection loads");
    assert_eq!(projection.profile_id, "default");
    assert_eq!(projection.session_projection_revision, 3);
    assert_eq!(projection.global_default_profile_id, "default");
    assert!(matches!(
        projection.resolved,
        ResolvedProviderProfileDto::Resolved {
            profile_revision_id,
            ..
        } if profile_revision_id == "rev-0001"
    ));
}

#[test]
fn get_reports_a_closed_reason_when_the_profile_is_unavailable() {
    let session_id = SessionId::new();
    let defaults = FakeDefaults::new();
    defaults.seed(session_id, "default", 0);
    let catalog = seeded_catalog();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let port = FakeAdmissionPort::with(
        "default",
        Err(ErrorDto::unavailable(
            "provider_profile_unavailable",
            "the provider profile is unavailable",
        )),
    );
    let service = SessionProfileService::new(&defaults, &catalog, &readiness);
    let projection = service
        .get(get_query(session_id), &port)
        .expect("projection loads");
    assert!(matches!(
        projection.resolved,
        ResolvedProviderProfileDto::Unavailable(ProviderProfileUnavailableReason::ProfileDisabled)
    ));
}

#[test]
fn get_falls_back_to_the_global_default_without_a_durable_default() {
    let session_id = SessionId::new();
    let defaults = FakeDefaults::new();
    let catalog = seeded_catalog();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let port = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let service = SessionProfileService::new(&defaults, &catalog, &readiness);
    let projection = service
        .get(get_query(session_id), &port)
        .expect("projection loads");
    assert_eq!(projection.profile_id, "default");
    assert_eq!(projection.session_projection_revision, 0);
    assert_eq!(projection.global_default_profile_id, "default");
}

#[test]
fn get_reports_unset_when_no_global_default_exists() {
    let session_id = SessionId::new();
    let defaults = FakeDefaults::new();
    let catalog = FakeCatalog::new();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let port = FakeAdmissionPort::new();
    let service = SessionProfileService::new(&defaults, &catalog, &readiness);
    let projection = service
        .get(get_query(session_id), &port)
        .expect("projection loads");
    assert_eq!(projection.profile_id, "unset");
    assert_eq!(projection.global_default_profile_id, "unset");
    assert!(matches!(
        projection.resolved,
        ResolvedProviderProfileDto::Unavailable(_)
    ));
}

fn unavailable_reason(code: &'static str) -> Option<ProviderProfileUnavailableReason> {
    let session_id = SessionId::new();
    let defaults = FakeDefaults::new();
    defaults.seed(session_id, "default", 0);
    let catalog = seeded_catalog();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let port = FakeAdmissionPort::with(
        "default",
        Err(ErrorDto::unavailable(
            code,
            "the provider profile cannot be resolved",
        )),
    );
    let service = SessionProfileService::new(&defaults, &catalog, &readiness);
    let projection = service
        .get(get_query(session_id), &port)
        .expect("projection loads");
    match projection.resolved {
        ResolvedProviderProfileDto::Unavailable(reason) => Some(reason),
        ResolvedProviderProfileDto::Resolved { .. } => None,
    }
}

#[test]
fn get_maps_catalog_not_ready_to_catalog_not_active() {
    assert_eq!(
        unavailable_reason("catalog_not_ready"),
        Some(ProviderProfileUnavailableReason::CatalogNotActive)
    );
}

#[test]
fn get_maps_tombstoned_profiles_to_disabled() {
    assert_eq!(
        unavailable_reason("provider_profile_tombstoned"),
        Some(ProviderProfileUnavailableReason::ProfileDisabled)
    );
}

#[test]
fn get_maps_configuration_unavailable_to_provider_unavailable() {
    assert_eq!(
        unavailable_reason("provider_configuration_unavailable"),
        Some(ProviderProfileUnavailableReason::ProviderUnavailable)
    );
}

#[test]
fn get_maps_runtime_unavailable_to_provider_unavailable() {
    assert_eq!(
        unavailable_reason("provider_profile_runtime_unavailable"),
        Some(ProviderProfileUnavailableReason::ProviderUnavailable)
    );
}

#[test]
fn get_maps_unknown_failures_to_profile_not_found() {
    assert_eq!(
        unavailable_reason("some_unknown_code"),
        Some(ProviderProfileUnavailableReason::ProfileNotFound)
    );
}

// ============================================================================
// Unavailable-provider queue
// ============================================================================

#[test]
fn promote_delegates_to_the_queue_and_passes_the_outcome_through() {
    let session_id = SessionId::new();
    let queue = FakeQueue::new();
    queue.seed(RunId::new(), session_id, "default", "unavailable");
    queue.seed(RunId::new(), session_id, "default", "unavailable");
    let outcome = UnavailableQueueService::new(&queue)
        .promote(session_id, RunId::new(), 1_000)
        .expect("promotion succeeds");
    assert_eq!(outcome.promoted.len(), 2);
    assert!(outcome.reconciliation_marker_created);
    assert!(
        outcome
            .promoted
            .iter()
            .all(|entry| entry.state == UnavailableQueueStateDto::Promoted)
    );
}

fn reconcile_command(
    session_id: SessionId,
    operation_id: &str,
) -> ReconcileUnavailableQueueCommandDto {
    ReconcileUnavailableQueueCommandDto {
        session_id: session_id.to_string(),
        operation_id: operation_id.to_owned(),
        page_cursor: None,
    }
}

#[test]
fn reconcile_terminalizes_promotes_and_reports_the_page_cursor() {
    let session_id = SessionId::new();
    let queue = FakeQueue::new();
    queue.seed(RunId::new(), session_id, "default", "terminal-run");
    queue.seed(RunId::new(), session_id, "default", "unavailable");
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let accepted = UnavailableQueueService::new(&queue)
        .reconcile(
            reconcile_command(session_id, "op-reconcile"),
            &readiness,
            1_000,
        )
        .expect("reconciliation succeeds");
    assert_eq!(accepted.promoted_count, 1);
    assert_eq!(accepted.page_cursor.as_deref(), Some("next-page-1"));
}

#[test]
fn reconcile_rejects_while_the_control_plane_is_degraded() {
    let session_id = SessionId::new();
    let queue = FakeQueue::new();
    let readiness = FakeReadinessPort::new(CatalogReadiness::PendingRemoval {
        candidate_revision: "2".to_owned(),
        expires_at: 1_000,
    });
    let error = UnavailableQueueService::new(&queue)
        .reconcile(
            reconcile_command(session_id, "op-degraded"),
            &readiness,
            1_000,
        )
        .expect_err("degraded reconciliation is rejected");
    assert_eq!(error.code(), "execution_not_ready");
}

#[test]
fn reconcile_rejects_an_invalid_command() {
    let session_id = SessionId::new();
    let queue = FakeQueue::new();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let mut command = reconcile_command(session_id, "op-invalid");
    command.session_id.clear();
    let error = UnavailableQueueService::new(&queue)
        .reconcile(command, &readiness, 1_000)
        .expect_err("blank session id is rejected");
    assert_eq!(error.code(), "unavailable_queue_invalid");
}

// ============================================================================
// Provider usage
// ============================================================================

fn usage_query(profile_id: &str) -> GetProviderUsageQueryDto {
    GetProviderUsageQueryDto {
        schema_version: "provider-usage-v1".to_owned(),
        profile_id: profile_id.to_owned(),
        usage_period_start: 0,
        usage_period_end: 500,
    }
}

#[test]
fn record_passes_through_to_usage_storage() {
    let usage = FakeUsage::new();
    let session_id = SessionId::new();
    let input = RecordProviderUsageInputDto {
        session_id,
        usage_period_start: 0,
        usage_period_end: 100,
        recorded_at: 50,
        events: Vec::new(),
    };
    UsageService::new(&usage)
        .record(input)
        .expect("recording succeeds");
    let recorded = usage.records.borrow();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].session_id, session_id);
}

#[test]
fn by_profile_aggregates_in_period_aggregates_only() {
    let usage = FakeUsage::new();
    usage.seed_aggregate("default", "rev-0001", "model-a", 100, 200);
    usage.seed_aggregate("default", "rev-0001", "model-a", 600, 700);
    usage.seed_aggregate("other", "rev-0001", "model-a", 100, 200);
    let aggregation = UsageService::new(&usage)
        .by_profile(usage_query("default"))
        .expect("aggregation succeeds");
    assert_eq!(aggregation.request_count, 5);
    assert_eq!(aggregation.input_units, 10);
    assert_eq!(aggregation.output_units, 20);
    assert_eq!(aggregation.reasoning_units, 30);
    assert_eq!(aggregation.provider_profile_revision_id, "rev-0001");
    assert_eq!(aggregation.model_id, "model-a");
}

#[test]
fn by_profile_rejects_an_aggregation_with_no_in_period_entries() {
    // With no aggregates the projection carries blank revision/model fields,
    // which the closed aggregation DTO validation rejects.
    let usage = FakeUsage::new();
    let error = UsageService::new(&usage)
        .by_profile(usage_query("default"))
        .expect_err("an empty aggregation is rejected");
    assert_eq!(error.code(), "provider_usage_invalid");
}

#[test]
fn by_revision_and_model_aggregates_in_period_aggregates_only() {
    let usage = FakeUsage::new();
    usage.seed_aggregate("default", "rev-0001", "model-a", 100, 200);
    usage.seed_aggregate("default", "rev-0001", "model-a", 600, 700);
    usage.seed_aggregate("default", "rev-0002", "model-a", 100, 200);
    let aggregation = UsageService::new(&usage)
        .by_revision_and_model(
            &usage_query("default"),
            "rev-0001".to_owned(),
            "model-a".to_owned(),
        )
        .expect("aggregation succeeds");
    assert_eq!(aggregation.request_count, 5);
    assert_eq!(aggregation.input_units, 10);
    assert_eq!(aggregation.provider_profile_revision_id, "rev-0001");
}

#[test]
fn by_profile_rejects_an_invalid_query() {
    let usage = FakeUsage::new();
    let mut query = usage_query("default");
    query.usage_period_end = 0;
    query.usage_period_start = 100;
    let error = UsageService::new(&usage)
        .by_profile(query)
        .expect_err("inverted period is rejected");
    assert_eq!(error.code(), "provider_usage_invalid");
}

// ============================================================================
// Catalog reads
// ============================================================================

fn catalog_query(expected_revision: Option<&str>) -> GetProviderCatalogQueryDto {
    GetProviderCatalogQueryDto {
        schema_version: "provider-catalog-v1".to_owned(),
        page_token: None,
        expected_catalog_revision_id: expected_revision.map(str::to_owned),
    }
}

#[test]
fn list_profiles_maps_entries_from_the_active_material() {
    let fake = seeded_catalog();
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let service = CatalogReadService::new(&fake, &controller);
    let page = service
        .list_profiles(catalog_query(None))
        .expect("catalog page loads");
    assert_eq!(page.catalog_revision_id, "1");
    assert!(!page.has_more);
    assert!(page.next_page_token.is_none());
    assert_eq!(page.entries.len(), 1);
    let entry = &page.entries[0];
    assert_eq!(entry.profile_id, "default");
    assert_eq!(entry.profile_revision_id, "rev-0001");
    assert_eq!(entry.display_name, "default");
    assert!(entry.enabled);
    assert_eq!(entry.provider_kind_id, "responses");
    assert_eq!(entry.model_id, "model-a");
    assert_eq!(entry.normalized_endpoint.as_deref(), Some(ENDPOINT));
    assert_eq!(
        entry.credential_transport_mode,
        CredentialTransportMode::Bearer
    );
    assert_eq!(
        entry.readiness,
        intention_protocol::contract_families::ProviderReadinessDto::Ready
    );
}

#[test]
fn list_profiles_maps_disabled_and_unavailable_readiness() {
    let fake = FakeCatalog::new();
    fake.seed_active(
        1,
        vec![seed_kind_candidate("responses", "bearer")],
        vec![
            with_readiness(
                seed_profile(
                    "responses",
                    "model-a",
                    "profile-a",
                    "rev-a",
                    "kind-responses-v1",
                ),
                ProviderReadinessDto::Ready,
            ),
            with_readiness(
                seed_profile(
                    "responses",
                    "model-b",
                    "profile-b",
                    "rev-b",
                    "kind-responses-v1",
                ),
                ProviderReadinessDto::Disabled,
            ),
            with_readiness(
                seed_profile(
                    "responses",
                    "model-c",
                    "profile-c",
                    "rev-c",
                    "kind-responses-v1",
                ),
                ProviderReadinessDto::Unavailable,
            ),
        ],
        "profile-a",
    );
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let service = CatalogReadService::new(&fake, &controller);
    let page = service
        .list_profiles(catalog_query(None))
        .expect("catalog page loads");
    let readiness = page
        .entries
        .iter()
        .map(|entry| entry.readiness)
        .collect::<Vec<_>>();
    assert_eq!(
        readiness,
        vec![
            intention_protocol::contract_families::ProviderReadinessDto::Ready,
            intention_protocol::contract_families::ProviderReadinessDto::Disabled,
            intention_protocol::contract_families::ProviderReadinessDto::Unavailable,
        ]
    );
}

#[test]
fn list_profiles_maps_the_safe_header_transport() {
    let fake = FakeCatalog::new();
    fake.seed_active(
        1,
        vec![seed_kind_candidate("responses", "bearer")],
        vec![safe_header_candidate(seed_profile(
            "responses",
            "model-a",
            "default",
            "rev-0001",
            "kind-responses-v1",
        ))],
        "default",
    );
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let service = CatalogReadService::new(&fake, &controller);
    let page = service
        .list_profiles(catalog_query(None))
        .expect("catalog page loads");
    let entry = &page.entries[0];
    assert_eq!(
        entry.credential_transport_mode,
        CredentialTransportMode::SafeHeader
    );
    assert_eq!(
        entry.credential_transport_safe_header_name.as_deref(),
        Some("x-auth-header")
    );
}

#[test]
fn list_profiles_rejects_a_stale_expected_catalog_revision() {
    let fake = seeded_catalog();
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let service = CatalogReadService::new(&fake, &controller);
    let error = service
        .list_profiles(catalog_query(Some("2")))
        .expect_err("stale expected revision is rejected");
    assert_eq!(error.code(), "catalog_page_token_stale");
}

#[test]
fn list_profiles_rejects_a_page_entry_missing_from_the_material() {
    let fake = seeded_catalog();
    let mut material = fake.active_material().expect("material loads");
    material.profiles = vec![seed_profile(
        "responses",
        "model-x",
        "ghost",
        "rev-0009",
        "kind-responses-v1",
    )];
    *fake.material_override.borrow_mut() = Some(material);
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let service = CatalogReadService::new(&fake, &controller);
    let error = service
        .list_profiles(catalog_query(None))
        .expect_err("material mismatch is rejected");
    assert_eq!(error.code(), "provider_catalog_projection_invalid");
}

fn status_query() -> GetProviderCatalogStatusQueryDto {
    GetProviderCatalogStatusQueryDto {
        schema_version: "provider-catalog-status-v1".to_owned(),
    }
}

#[test]
fn status_reports_active_when_the_catalog_is_ready() {
    let fake = seeded_catalog();
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    controller.startup(1_000).expect("startup succeeds");
    let service = CatalogReadService::new(&fake, &controller);
    let status = service.status(status_query()).expect("status loads");
    assert_eq!(
        status.activation_state,
        ProviderCatalogActivationState::Active
    );
    assert_eq!(status.degraded_reason, None);
    assert_eq!(status.active_catalog_revision_id.as_deref(), Some("1"));
    assert_eq!(status.active_default_profile_id.as_deref(), Some("default"));
}

#[test]
fn status_reports_preparing_when_the_catalog_is_uninitialized() {
    let fake = seeded_catalog();
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let service = CatalogReadService::new(&fake, &controller);
    let status = service.status(status_query()).expect("status loads");
    assert_eq!(
        status.activation_state,
        ProviderCatalogActivationState::Preparing
    );
    assert_eq!(status.degraded_reason, None);
}

#[test]
fn status_reports_pending_removal_while_a_candidate_is_pending() {
    let fake = seeded_catalog();
    let (controller, _) = removal_controller(&fake, 1_000);
    let service = CatalogReadService::new(&fake, &controller);
    let status = service.status(status_query()).expect("status loads");
    assert_eq!(
        status.activation_state,
        ProviderCatalogActivationState::PendingRemoval
    );
    assert_eq!(
        status.degraded_reason,
        Some(ProviderCatalogDegradedReason::RemovalCandidatePending)
    );
    assert_eq!(status.candidate_catalog_revision_id.as_deref(), Some("2"));
}

#[test]
fn status_reports_activation_recovery_required_after_a_recovery_mismatch() {
    let mut fake = seeded_catalog();
    fake.seed_recovery_required(1);
    fake.material_fault = true;
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    controller.startup(1_000).expect("startup degrades safely");
    let service = CatalogReadService::new(&fake, &controller);
    let status = service.status(status_query()).expect("status loads");
    assert_eq!(
        status.activation_state,
        ProviderCatalogActivationState::ActivationRecoveryRequired
    );
    assert_eq!(
        status.degraded_reason,
        Some(ProviderCatalogDegradedReason::ActivationRecoveryRequired)
    );
}

#[test]
fn status_reports_removal_candidate_rejected_when_blocked_by_rejection() {
    let fake = seeded_catalog();
    let (controller, outcome) = removal_controller(&fake, 1_000);
    let handle = outcome.candidate_handle.expect("removal handle exists");
    controller
        .reject_pending(handle, "op-reject".to_owned(), 1_100)
        .expect("rejection commits");
    let service = CatalogReadService::new(&fake, &controller);
    let status = service.status(status_query()).expect("status loads");
    assert_eq!(
        status.activation_state,
        ProviderCatalogActivationState::PendingRemoval
    );
    assert_eq!(
        status.degraded_reason,
        Some(ProviderCatalogDegradedReason::RemovalCandidateRejected)
    );
}

#[test]
fn status_reports_removal_candidate_expired_when_blocked_by_expiry() {
    let fake = seeded_catalog();
    let (controller, _) = removal_controller(&fake, 1_000);
    controller
        .expire_pending(1_000 + 30 * 60)
        .expect("expiry runs");
    let service = CatalogReadService::new(&fake, &controller);
    let status = service.status(status_query()).expect("status loads");
    assert_eq!(
        status.activation_state,
        ProviderCatalogActivationState::PendingRemoval
    );
    assert_eq!(
        status.degraded_reason,
        Some(ProviderCatalogDegradedReason::RemovalCandidateExpired)
    );
}

#[test]
fn status_rejects_an_unknown_blocked_reason_as_an_inconsistent_projection() {
    let mut fake = seeded_catalog();
    fake.material_fault = true;
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    controller.startup(1_000).expect("startup degrades safely");
    let projection = controller.inspect().expect("inspect succeeds");
    assert!(matches!(
        projection.readiness,
        CatalogReadiness::Blocked { reason } if reason == "injected_material_fault"
    ));
    let service = CatalogReadService::new(&fake, &controller);
    let error = service
        .status(status_query())
        .expect_err("unknown blocked reason produces an inconsistent status");
    assert_eq!(error.code(), "provider_catalog_status_invalid");
}

// ============================================================================
// Removal lifecycle
// ============================================================================

fn accept_command(handle: &str) -> AcceptProviderCatalogRemovalCommandDto {
    AcceptProviderCatalogRemovalCommandDto {
        candidate_handle: handle.to_owned(),
        expected_active_catalog_revision_id: "1".to_owned(),
        expected_candidate_catalog_revision_id: "2".to_owned(),
        operation_id: "op-accept".to_owned(),
        source_recheck: true,
    }
}

#[test]
fn removal_accept_commits_through_the_controller() {
    let fake = seeded_catalog();
    let (controller, outcome) = removal_controller(&fake, 1_000);
    let handle = outcome.candidate_handle.expect("removal handle exists");
    let accepted: AcceptProviderCatalogRemovalAcceptedDto = RemovalService::new(&controller)
        .accept(accept_command(&handle), 1_100)
        .expect("removal acceptance commits");
    assert_eq!(accepted.candidate_handle, handle);
    assert_eq!(accepted.active_catalog_revision_id, "2");
}

#[test]
fn removal_accept_rejects_an_invalid_command() {
    let fake = seeded_catalog();
    let (controller, _) = removal_controller(&fake, 1_000);
    let error = RemovalService::new(&controller)
        .accept(accept_command(""), 1_100)
        .expect_err("blank candidate handle is rejected");
    assert_eq!(error.code(), "provider_catalog_removal_invalid");
}

#[test]
fn removal_reject_commits_and_degrades_the_catalog() {
    let fake = seeded_catalog();
    let (controller, outcome) = removal_controller(&fake, 1_000);
    let handle = outcome.candidate_handle.expect("removal handle exists");
    let accepted: RejectProviderCatalogCandidateAcceptedDto = RemovalService::new(&controller)
        .reject(
            RejectProviderCatalogCandidateCommandDto {
                candidate_handle: handle.clone(),
                expected_active_catalog_revision_id: "1".to_owned(),
                operation_id: "op-reject".to_owned(),
            },
            1_100,
        )
        .expect("removal rejection commits");
    assert_eq!(accepted.candidate_handle, handle);
    let projection = controller.inspect().expect("inspect succeeds");
    assert!(matches!(
        projection.readiness,
        CatalogReadiness::Blocked {
            reason
        } if reason == "removal_candidate_rejected"
    ));
}

#[test]
fn removal_expire_passes_through_the_expired_count() {
    let fake = seeded_catalog();
    let (controller, _) = removal_controller(&fake, 1_000);
    let expired = RemovalService::new(&controller)
        .expire(1_000 + 30 * 60)
        .expect("expiry runs");
    assert_eq!(expired, 1);
}

// ============================================================================
// Held recovered-run admission
// ============================================================================

#[test]
fn admit_verifies_commits_and_dispatches_exactly_once() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let held = FakeHeld::new();
    held.seed(held_record(
        run_id,
        session_id,
        HeldRunAdmissionStateDto::Held,
        None,
    ));
    let selections = FakeSelections::new();
    selections.seed(session_id, run_id, selection("default", "responses"));
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let admission = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let dispatch = FakeDispatch::new();
    let accepted: AdmitRecoveredRunAcceptedDto =
        HeldRunService::new(&held, &selections, &readiness, &admission)
            .admit(
                admit_command(session_id, run_id, "op-admit"),
                schedule(session_id, run_id),
                &dispatch,
                1_000,
            )
            .expect("admission succeeds");
    assert_eq!(accepted.session_id, session_id.to_string());
    assert_eq!(accepted.run_id, run_id.to_string());
    assert_eq!(dispatch.call_count(), 1);
    let record = held
        .load_held_recovered_run(run_id)
        .expect("held record loads")
        .expect("held record exists");
    assert_eq!(record.admission_state, HeldRunAdmissionStateDto::Admitted);
    assert_eq!(record.admission_operation_id.as_deref(), Some("op-admit"));
    assert_eq!(record.admitted_at, Some(1_000));
}

#[test]
fn admit_repeat_same_operation_is_idempotent_without_a_second_dispatch() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let held = FakeHeld::new();
    held.seed(held_record(
        run_id,
        session_id,
        HeldRunAdmissionStateDto::Admitted,
        Some("op-admit"),
    ));
    let selections = FakeSelections::new();
    selections.seed(session_id, run_id, selection("default", "responses"));
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let admission = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let dispatch = FakeDispatch::new();
    HeldRunService::new(&held, &selections, &readiness, &admission)
        .admit(
            admit_command(session_id, run_id, "op-admit"),
            schedule(session_id, run_id),
            &dispatch,
            1_000,
        )
        .expect("repeat admission is idempotent");
    assert_eq!(dispatch.call_count(), 0);
}

#[test]
fn admit_already_admitted_different_operation_conflicts() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let held = FakeHeld::new();
    held.seed(held_record(
        run_id,
        session_id,
        HeldRunAdmissionStateDto::Admitted,
        Some("op-first"),
    ));
    let selections = FakeSelections::new();
    selections.seed(session_id, run_id, selection("default", "responses"));
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let admission = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let dispatch = FakeDispatch::new();
    let error = HeldRunService::new(&held, &selections, &readiness, &admission)
        .admit(
            admit_command(session_id, run_id, "op-second"),
            schedule(session_id, run_id),
            &dispatch,
            1_000,
        )
        .expect_err("a different operation cannot re-admit");
    assert_eq!(error.code(), "provider_admission_not_found");
    assert_eq!(error.category(), ErrorCategoryDto::Conflict);
    assert_eq!(dispatch.call_count(), 0);
}

#[test]
fn admit_rejected_state_is_unavailable() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let held = FakeHeld::new();
    held.seed(held_record(
        run_id,
        session_id,
        HeldRunAdmissionStateDto::Rejected,
        None,
    ));
    let selections = FakeSelections::new();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let admission = FakeAdmissionPort::new();
    let dispatch = FakeDispatch::new();
    let error = HeldRunService::new(&held, &selections, &readiness, &admission)
        .admit(
            admit_command(session_id, run_id, "op-rejected"),
            schedule(session_id, run_id),
            &dispatch,
            1_000,
        )
        .expect_err("rejected runs cannot be admitted");
    assert_eq!(error.code(), "provider_admission_not_found");
    assert_eq!(dispatch.call_count(), 0);
}

#[test]
fn admit_not_held_run_is_unavailable() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let held = FakeHeld::new();
    let selections = FakeSelections::new();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let admission = FakeAdmissionPort::new();
    let dispatch = FakeDispatch::new();
    let error = HeldRunService::new(&held, &selections, &readiness, &admission)
        .admit(
            admit_command(session_id, run_id, "op-missing"),
            schedule(session_id, run_id),
            &dispatch,
            1_000,
        )
        .expect_err("unknown runs are not admitted");
    assert_eq!(error.code(), "provider_admission_not_found");
    assert_eq!(dispatch.call_count(), 0);
}

#[test]
fn admit_wrong_session_is_invalid() {
    let run_id = RunId::new();
    let held = FakeHeld::new();
    held.seed(held_record(
        run_id,
        SessionId::new(),
        HeldRunAdmissionStateDto::Held,
        None,
    ));
    let selections = FakeSelections::new();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let admission = FakeAdmissionPort::new();
    let dispatch = FakeDispatch::new();
    let error = HeldRunService::new(&held, &selections, &readiness, &admission)
        .admit(
            admit_command(SessionId::new(), run_id, "op-session"),
            schedule(SessionId::new(), run_id),
            &dispatch,
            1_000,
        )
        .expect_err("cross-session admission is invalid");
    assert_eq!(error.code(), "recovered_run_admission_invalid");
    assert_eq!(dispatch.call_count(), 0);
}

#[test]
fn admit_rejects_while_the_control_plane_is_degraded() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let held = FakeHeld::new();
    held.seed(held_record(
        run_id,
        session_id,
        HeldRunAdmissionStateDto::Held,
        None,
    ));
    let selections = FakeSelections::new();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Blocked {
        reason: "removal_candidate_expired".to_owned(),
    });
    let admission = FakeAdmissionPort::new();
    let dispatch = FakeDispatch::new();
    let error = HeldRunService::new(&held, &selections, &readiness, &admission)
        .admit(
            admit_command(session_id, run_id, "op-degraded"),
            schedule(session_id, run_id),
            &dispatch,
            1_000,
        )
        .expect_err("degraded admission is rejected");
    assert_eq!(error.code(), "execution_not_ready");
    assert_eq!(dispatch.call_count(), 0);
}

#[test]
fn admit_missing_persisted_selection_fails_verification_and_stays_held() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let held = FakeHeld::new();
    held.seed(held_record(
        run_id,
        session_id,
        HeldRunAdmissionStateDto::Held,
        None,
    ));
    let selections = FakeSelections::new();
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let admission = FakeAdmissionPort::new();
    let dispatch = FakeDispatch::new();
    let error = HeldRunService::new(&held, &selections, &readiness, &admission)
        .admit(
            admit_command(session_id, run_id, "op-no-selection"),
            schedule(session_id, run_id),
            &dispatch,
            1_000,
        )
        .expect_err("a run without a persisted selection is not admitted");
    assert_eq!(error.code(), "held_run_admission_verification_failed");
    let record = held
        .load_held_recovered_run(run_id)
        .expect("held record loads")
        .expect("held record exists");
    assert_eq!(record.admission_state, HeldRunAdmissionStateDto::Held);
    assert_eq!(dispatch.call_count(), 0);
}

#[test]
fn admit_invalid_persisted_selection_fails_verification() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let held = FakeHeld::new();
    held.seed(held_record(
        run_id,
        session_id,
        HeldRunAdmissionStateDto::Held,
        None,
    ));
    let selections = FakeSelections::new();
    selections.seed(session_id, run_id, selection("default", "openai"));
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let admission = FakeAdmissionPort::new();
    let dispatch = FakeDispatch::new();
    let error = HeldRunService::new(&held, &selections, &readiness, &admission)
        .admit(
            admit_command(session_id, run_id, "op-invalid-selection"),
            schedule(session_id, run_id),
            &dispatch,
            1_000,
        )
        .expect_err("an invalid persisted selection is not admitted");
    assert_eq!(error.code(), "held_run_admission_verification_failed");
    assert_eq!(dispatch.call_count(), 0);
}

#[test]
fn admit_registry_key_verification_failure_fails_verification() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let held = FakeHeld::new();
    held.seed(held_record(
        run_id,
        session_id,
        HeldRunAdmissionStateDto::Held,
        None,
    ));
    let selections = FakeSelections::new();
    selections.seed(session_id, run_id, selection("default", "responses"));
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let mut admission =
        FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    admission.fail_registry_verification();
    let dispatch = FakeDispatch::new();
    let error = HeldRunService::new(&held, &selections, &readiness, &admission)
        .admit(
            admit_command(session_id, run_id, "op-registry"),
            schedule(session_id, run_id),
            &dispatch,
            1_000,
        )
        .expect_err("an unadmitted registry key is not verified");
    assert_eq!(error.code(), "held_run_admission_verification_failed");
    assert_eq!(dispatch.call_count(), 0);
}

#[test]
fn admit_selection_load_failure_fails_verification() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let held = FakeHeld::new();
    held.seed(held_record(
        run_id,
        session_id,
        HeldRunAdmissionStateDto::Held,
        None,
    ));
    let mut selections = FakeSelections::new();
    selections.fail_load = true;
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let admission = FakeAdmissionPort::new();
    let dispatch = FakeDispatch::new();
    let error = HeldRunService::new(&held, &selections, &readiness, &admission)
        .admit(
            admit_command(session_id, run_id, "op-load-failure"),
            schedule(session_id, run_id),
            &dispatch,
            1_000,
        )
        .expect_err("a selection load failure is not verified");
    assert_eq!(error.code(), "held_run_admission_verification_failed");
    assert_eq!(dispatch.call_count(), 0);
}

#[test]
fn admit_schedule_identity_mismatch_is_invalid_after_the_commit() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let other_run_id = RunId::new();
    let held = FakeHeld::new();
    held.seed(held_record(
        run_id,
        session_id,
        HeldRunAdmissionStateDto::Held,
        None,
    ));
    let selections = FakeSelections::new();
    selections.seed(session_id, run_id, selection("default", "responses"));
    let readiness = FakeReadinessPort::new(CatalogReadiness::Ready);
    let admission = FakeAdmissionPort::with("default", Ok(resolved_profile("default", "rev-0001")));
    let dispatch = FakeDispatch::new();
    let error = HeldRunService::new(&held, &selections, &readiness, &admission)
        .admit(
            admit_command(session_id, run_id, "op-mismatch"),
            schedule(session_id, other_run_id),
            &dispatch,
            1_000,
        )
        .expect_err("a mismatched schedule is rejected after the commit");
    assert_eq!(error.code(), "recovered_run_admission_invalid");
    // The durable admission committed before the schedule identity check.
    let record = held
        .load_held_recovered_run(run_id)
        .expect("held record loads")
        .expect("held record exists");
    assert_eq!(record.admission_state, HeldRunAdmissionStateDto::Admitted);
    assert_eq!(dispatch.call_count(), 0);
}
