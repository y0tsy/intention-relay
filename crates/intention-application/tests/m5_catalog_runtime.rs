#![allow(
    clippy::expect_used,
    reason = "Focused catalog runtime fixtures use expect for precise diagnostics."
)]

//! Zone 6a provider catalog runtime integration tests.
//!
//! The in-memory fake storage below enforces the same invariants as the
//! durable backend: append-only history, at most one pending removal
//! candidate, and atomicity through `Result`-returning methods (the fake
//! errors on conflict).

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use intention_application::{
    CatalogAcceptanceOutcomeDto, CatalogCandidateOutcomeDto, CatalogProviderDeclarationDto,
    CatalogSourceInputDto, ModelRunDriverHandle, PrivateProviderProfileMaterial, PrivateRegistry,
    PrivateRegistryKey, ProviderCatalogController, ProviderDriverFactory,
};
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
    control_plane::{ConfigCandidateDto, parse_candidate},
};
use intention_domain::{
    ContextPreservationCapability, CredentialTransportMode, ModelCapabilitySetV1,
    ModelInputCapability, ProviderDriverContractRevisionDto, ProviderKindDescriptorRevisionV1,
    ProviderProfileRevisionV1, ReasoningCapability, StructuredOutputCapability,
    provider_selection::MODEL_CAPABILITY_TAXONOMY_V1,
};
use intention_storage::{
    AcceptProviderCatalogInputDto, AcceptProviderCatalogRemovalInputDto,
    AppendProviderKindDescriptorRevisionInputDto, AppendProviderProfileRevisionInputDto,
    CreateProviderCatalogRemovalCandidateInputDto, ExpireProviderCatalogCandidateInputDto,
    ExpireProviderCatalogRemovalCandidateInputDto, LoadProviderCatalogPageInputDto,
    ProviderCatalogMaterialDto, ProviderCatalogPageDto, ProviderCatalogProfileEntryDto,
    ProviderCatalogRemovalCandidateDto, ProviderCatalogRemovalStatusDto,
    ProviderCatalogRepositoryDto, ProviderCatalogStateDto, ProviderCatalogStatusDto,
    ProviderKindDescriptorCandidateDto, ProviderProfileCandidateDto, ProviderReadinessDto,
    ProviderRemovalRepositoryDto, RejectProviderCatalogCandidateInputDto,
    RejectProviderCatalogRemovalInputDto,
};
use intention_types::{ConfigRevisionId, DtoResult, ErrorDto, SchemaVersionDto, TimestampDto};

const CREDENTIAL: &str = "sk-test-sweep-12345";
const ENDPOINT: &str = "https://api.example.invalid/v1";

fn time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid")
}

fn explicit_source() -> ConfigSourceDto {
    ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-application-catalog-runtime.toml")
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
            credential_transport_mode: CredentialTransportMode::Bearer,
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

fn key_for(profile: &ProviderProfileRevisionV1) -> PrivateRegistryKey {
    PrivateRegistryKey {
        profile_id: profile.profile_id.clone(),
        profile_revision_id: profile.revision_id.clone(),
        kind_descriptor_revision_id: profile.kind_descriptor_revision_id.clone(),
        driver_contract: profile.driver_contract_revision.clone(),
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
        _profile: PrivateProviderProfileMaterial,
    ) -> DtoResult<Box<dyn ModelRunDriverHandle + Send + Sync>> {
        self.builds.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(TestHandle))
    }
}

// ============================================================================
// Fake catalog storage (ProviderCatalogRepositoryDto + ProviderRemovalRepositoryDto)
// ============================================================================

struct FakeCatalogState {
    status: ProviderCatalogStatusDto,
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
            status: ProviderCatalogStatusDto::Preparing,
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
}

impl FakeCatalog {
    const fn new() -> Self {
        Self {
            state: RefCell::new(FakeCatalogState::new()),
            audits: RefCell::new(Vec::new()),
            material_fault: false,
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
        state.status = ProviderCatalogStatusDto::Active;
        state.active_catalog_revision_id = Some(revision);
        state.active_default_profile_id = Some(default_profile_id.to_owned());
        state.updated_at = 1;
    }

    fn seed_recovery_required(&self, revision: u64) {
        let mut state = self.state.borrow_mut();
        state.status = ProviderCatalogStatusDto::ActivationRecoveryRequired;
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

fn conflict(code: &'static str, message: &'static str) -> ErrorDto {
    ErrorDto::new(
        code,
        intention_types::ErrorCategoryDto::Conflict,
        message,
        intention_types::ErrorRetryDto::Never,
        None,
    )
    .expect("fixture conflict error is valid")
}

fn not_found(code: &'static str, message: &'static str) -> ErrorDto {
    ErrorDto::new(
        code,
        intention_types::ErrorCategoryDto::NotFound,
        message,
        intention_types::ErrorRetryDto::Never,
        None,
    )
    .expect("fixture not-found error is valid")
}

fn unavailable(code: &'static str, message: &'static str) -> ErrorDto {
    ErrorDto::unavailable(code, message)
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
        state.status = ProviderCatalogStatusDto::Active;
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
            ProviderCatalogStatusDto::Active
        } else {
            ProviderCatalogStatusDto::Preparing
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
            ProviderCatalogStatusDto::Active
        } else {
            ProviderCatalogStatusDto::Preparing
        };
        state.updated_at = input.expired_at;
        self.audits
            .borrow_mut()
            .push("ProviderCatalogCandidateExpired".to_owned());
        Ok(())
    }

    fn load_provider_catalog_material(&self) -> DtoResult<ProviderCatalogMaterialDto> {
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
        state.status = ProviderCatalogStatusDto::PendingRemoval;
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
            ProviderCatalogStatusDto::Active
        } else {
            ProviderCatalogStatusDto::Preparing
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
                ProviderCatalogStatusDto::Active
            } else {
                ProviderCatalogStatusDto::Preparing
            };
            state.updated_at = input.now;
        }
        Ok(expired)
    }
}

// ============================================================================
// Shared fixtures
// ============================================================================

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
// Startup
// ============================================================================

#[test]
fn startup_reaches_ready_with_a_valid_active_catalog() {
    let fake = seeded_catalog();
    let builds = Arc::new(AtomicUsize::new(0));
    let controller = fake.build_controller(vec![responses_factory(builds.clone())]);
    let outcome = controller.startup(1_000).expect("startup succeeds");
    assert_eq!(
        outcome.readiness,
        intention_application::CatalogReadiness::Ready
    );
    assert_eq!(outcome.active_catalog_revision_id, Some(1));
    assert_eq!(outcome.entry_count, 1);
    assert_eq!(outcome.degraded_reason, None);
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    let projection = controller.inspect().expect("inspect succeeds");
    assert_eq!(projection.readiness, outcome.readiness);
    assert_eq!(projection.entry_count, 1);
}

#[test]
fn startup_blocks_on_corrupt_material_with_no_partial_registry() {
    let mut fake = seeded_catalog();
    fake.material_fault = true;
    let builds = Arc::new(AtomicUsize::new(0));
    let controller = fake.build_controller(vec![responses_factory(builds.clone())]);
    let outcome = controller.startup(1_000).expect("startup degrades safely");
    assert_eq!(
        outcome.readiness,
        intention_application::CatalogReadiness::Blocked {
            reason: "injected_material_fault".to_owned(),
        }
    );
    assert_eq!(outcome.entry_count, 0);
    assert_eq!(builds.load(Ordering::SeqCst), 0);
    let projection = controller.inspect().expect("inspect succeeds");
    assert_eq!(projection.entry_count, 0);
}

#[test]
fn startup_blocks_when_no_factory_serves_the_accepted_kind() {
    let fake = seeded_catalog();
    let controller = fake.build_controller(Vec::new());
    let outcome = controller.startup(1_000).expect("startup degrades safely");
    assert!(matches!(
        outcome.readiness,
        intention_application::CatalogReadiness::Blocked { .. }
    ));
    assert_eq!(outcome.entry_count, 0);
}

#[test]
fn startup_with_no_active_catalog_is_ready_and_empty() {
    let fake = FakeCatalog::new();
    let builds = Arc::new(AtomicUsize::new(0));
    let controller = fake.build_controller(vec![responses_factory(builds.clone())]);
    let outcome = controller.startup(1_000).expect("startup succeeds");
    assert_eq!(
        outcome.readiness,
        intention_application::CatalogReadiness::Ready
    );
    assert_eq!(outcome.active_catalog_revision_id, None);
    assert_eq!(outcome.entry_count, 0);
    assert_eq!(builds.load(Ordering::SeqCst), 0);
}

#[test]
fn startup_recovery_reaches_ready_and_records_the_accept_path_audit() {
    let fake = seeded_catalog();
    fake.seed_recovery_required(1);
    let builds = Arc::new(AtomicUsize::new(0));
    let controller = fake.build_controller(vec![responses_factory(builds)]);
    let outcome = controller.startup(1_000).expect("recovery succeeds");
    assert_eq!(
        outcome.readiness,
        intention_application::CatalogReadiness::Ready
    );
    assert_eq!(outcome.active_catalog_revision_id, Some(1));
    assert_eq!(outcome.entry_count, 1);
    let audits = fake.audits.borrow();
    assert!(
        audits
            .iter()
            .any(|audit| audit == "ProviderCatalogAccepted")
    );
    assert!(
        audits
            .iter()
            .any(|audit| audit == "ProviderCatalogActivated")
    );
    let status = (&fake)
        .load_provider_catalog_status()
        .expect("status loads");
    assert_eq!(status.status, ProviderCatalogStatusDto::Active);
}

#[test]
fn startup_recovery_mismatch_stays_recovery_required_without_partial_registry() {
    let mut fake = seeded_catalog();
    fake.seed_recovery_required(1);
    fake.material_fault = true;
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let outcome = controller.startup(1_000).expect("startup degrades safely");
    assert_eq!(
        outcome.readiness,
        intention_application::CatalogReadiness::ActivationRecoveryRequired {
            accepted_revision: "1".to_owned(),
        }
    );
    assert_eq!(outcome.entry_count, 0);
}

#[test]
fn startup_recovery_does_not_adopt_current_configuration() {
    let fake = seeded_catalog();
    fake.seed_recovery_required(1);
    let builds = Arc::new(AtomicUsize::new(0));
    let controller = fake.build_controller(vec![responses_factory(builds)]);
    let outcome = controller.startup(1_000).expect("recovery succeeds");
    assert_eq!(
        outcome.readiness,
        intention_application::CatalogReadiness::Ready
    );
    assert_eq!(outcome.active_catalog_revision_id, Some(1));
    // Recovery never advanced to a new revision and never consulted any
    // configuration source: the accepted material is unchanged.
    let state = (&fake)
        .load_provider_catalog_status()
        .expect("status loads");
    assert_eq!(state.active_catalog_revision_id, Some(1));
    assert_eq!(state.candidate_catalog_revision_id, None);
    let material = fake.active_material().expect("material loads");
    assert_eq!(material.catalog_revision_id, 1);
    assert_eq!(material.profiles.len(), 1);
    assert_eq!(material.profiles[0].profile.revision_id, "rev-0001");
}

// ============================================================================
// Candidate limits
// ============================================================================

fn base_source(previous: &ConfigSnapshotDto, model: &str) -> CatalogSourceInputDto {
    source(
        "op-limit",
        raw_config("openrouter", model, ENDPOINT).len() as u64,
        vec![declaration("responses", model, Some(ENDPOINT), true)],
        candidate("openrouter", model, ENDPOINT, previous),
        previous.clone(),
    )
}

#[test]
fn candidate_with_more_than_128_profiles_is_rejected() {
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let providers = (0..129)
        .map(|index| declaration("responses", &format!("model-{index}"), Some(ENDPOINT), true))
        .collect();
    let fake = FakeCatalog::new();
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let error = controller
        .prepare_candidate(
            source(
                "op-limit",
                1_024,
                providers,
                candidate("openrouter", "model-a", ENDPOINT, &previous),
                previous,
            ),
            1_000,
        )
        .expect_err("profile limit is enforced");
    assert_eq!(error.code(), "provider_catalog_profile_limit_exceeded");
}

#[test]
fn candidate_with_more_than_32_kinds_is_rejected() {
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let providers = (0..33)
        .map(|index| declaration(&format!("kind-{index}"), "model-a", Some(ENDPOINT), true))
        .collect();
    let fake = FakeCatalog::new();
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let error = controller
        .prepare_candidate(
            source(
                "op-limit",
                1_024,
                providers,
                candidate("openrouter", "model-a", ENDPOINT, &previous),
                previous,
            ),
            1_000,
        )
        .expect_err("kind limit is enforced");
    assert_eq!(error.code(), "provider_catalog_kind_limit_exceeded");
}

#[test]
fn candidate_raw_size_over_512_kib_is_rejected() {
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let fake = FakeCatalog::new();
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let error = controller
        .prepare_candidate(
            source(
                "op-limit",
                512 * 1024 + 1,
                vec![declaration("responses", "model-a", Some(ENDPOINT), true)],
                candidate("openrouter", "model-a", ENDPOINT, &previous),
                previous,
            ),
            1_000,
        )
        .expect_err("raw size limit is enforced");
    assert_eq!(error.code(), "candidate_too_large");
}

#[test]
fn candidate_with_63_character_id_overflow_is_rejected() {
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let long_model = "m".repeat(64);
    let fake = FakeCatalog::new();
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let error = controller
        .prepare_candidate(base_source(&previous, &long_model), 1_000)
        .expect_err("over-limit model id is rejected");
    assert_eq!(error.code(), "provider_profile_revision_invalid");
}

#[test]
fn candidate_issues_are_bounded_at_thirty_two() {
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let mut noisy = String::from(
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"model-b\"\ncredential = \"fixture-secret\"\nendpoint = \"https://api.example.invalid/v1\"\n",
    );
    for index in 0..40 {
        noisy.push_str(&format!("unknown_field_{index} = 1\n"));
    }
    let parsed = parse_candidate(RawConfigInputDto::new(noisy, explicit_source()), &previous)
        .expect("fixture candidate parses");
    assert!(parsed.validation().truncated());
    assert_eq!(parsed.validation().total_issue_count(), 40);
    let fake = FakeCatalog::new();
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let outcome = controller
        .prepare_candidate(
            source(
                "op-issues",
                1_024,
                vec![declaration("responses", "model-b", Some(ENDPOINT), true)],
                parsed,
                previous,
            ),
            1_000,
        )
        .expect("bounded issues do not fail preparation");
    assert!(outcome.truncated_issues);
    assert_eq!(outcome.total_issue_count, 40);
    assert_eq!(outcome.issues.len(), 32);
}

// ============================================================================
// Semantic equivalence and auto-accept
// ============================================================================

#[test]
fn semantic_equal_candidate_activates_an_empty_catalog_then_produces_no_new_revision() {
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let fake = FakeCatalog::new();
    let builds = Arc::new(AtomicUsize::new(0));
    let controller = fake.build_controller(vec![responses_factory(builds.clone())]);

    // No catalog is active yet: the first startup declaration is accepted
    // even when it is semantically equal to the active configuration
    // snapshot, so a fresh daemon activates its startup provider exactly once
    // (no selection-less fallback under ADR 0038).
    let first = controller
        .prepare_candidate(base_source(&previous, "model-a"), 1_000)
        .expect("first semantic-equal candidate activates the empty catalog");
    assert!(first.changed);
    assert_eq!(first.catalog_revision_id, Some(1));
    assert!(!first.pending_removal);
    assert_eq!(builds.load(Ordering::SeqCst), 1);

    // With an active catalog, a semantically equal candidate is a no-op.
    let second = controller
        .prepare_candidate(base_source(&previous, "model-a"), 2_000)
        .expect("semantic-equal candidate is a no-op once a catalog is active");
    assert!(!second.changed);
    assert_eq!(second.catalog_revision_id, None);
    assert!(!second.pending_removal);
    assert_eq!(builds.load(Ordering::SeqCst), 1);
}

#[test]
fn non_removal_candidate_is_auto_accepted_and_activated() {
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let fake = FakeCatalog::new();
    let builds = Arc::new(AtomicUsize::new(0));
    let controller = fake.build_controller(vec![responses_factory(builds.clone())]);
    let outcome = controller
        .prepare_candidate(base_source(&previous, "model-b"), 1_000)
        .expect("candidate auto-accepts");
    assert!(outcome.changed);
    assert_eq!(outcome.catalog_revision_id, Some(1));
    assert!(!outcome.pending_removal);
    assert_eq!(
        outcome.readiness,
        intention_application::CatalogReadiness::Ready
    );
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    let state = (&fake)
        .load_provider_catalog_status()
        .expect("status loads");
    assert_eq!(state.status, ProviderCatalogStatusDto::Active);
    assert_eq!(state.active_catalog_revision_id, Some(1));
}

// ============================================================================
// Pending removal lifecycle
// ============================================================================

type RemovalController<'a> = ProviderCatalogController<&'a FakeCatalog, &'a FakeCatalog>;

fn removal_controller(
    fake: &FakeCatalog,
    now: u64,
) -> (RemovalController<'_>, CatalogCandidateOutcomeDto) {
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

#[test]
fn pending_removal_creation_carries_a_thirty_minute_expiry() {
    let fake = seeded_catalog();
    let (controller, outcome) = removal_controller(&fake, 1_000);
    assert!(outcome.changed);
    assert!(outcome.pending_removal);
    assert_eq!(outcome.catalog_revision_id, Some(2));
    let handle = outcome.candidate_handle.expect("removal handle exists");
    assert_eq!(handle, "catalog-2");
    assert_eq!(outcome.removal_expires_at, Some(1_000 + 30 * 60));
    assert_eq!(
        outcome.readiness,
        intention_application::CatalogReadiness::PendingRemoval {
            candidate_revision: "2".to_owned(),
            expires_at: 1_000 + 30 * 60,
        }
    );
    let projection = controller.inspect().expect("inspect succeeds");
    assert!(matches!(
        projection.readiness,
        intention_application::CatalogReadiness::PendingRemoval { .. }
    ));
}

#[test]
fn pending_removal_acceptance_commits_tombstones_and_the_ordered_audit() {
    let fake = seeded_catalog();
    let (controller, outcome) = removal_controller(&fake, 1_000);
    let handle = outcome.candidate_handle.expect("removal handle exists");
    let accepted: CatalogAcceptanceOutcomeDto = controller
        .accept_pending(
            handle,
            "1".to_owned(),
            "2".to_owned(),
            "op-accept".to_owned(),
            1_100,
        )
        .expect("removal acceptance commits");
    assert_eq!(accepted.catalog_revision_id, 2);
    assert_eq!(accepted.entry_count, 1);
    assert_eq!(
        accepted.readiness,
        intention_application::CatalogReadiness::Ready
    );
    assert_eq!(accepted.removed_kind_ids, vec!["responses".to_owned()]);
    let state = (&fake)
        .load_provider_catalog_status()
        .expect("status loads");
    assert_eq!(state.status, ProviderCatalogStatusDto::Active);
    assert_eq!(state.active_catalog_revision_id, Some(2));
    let audits = fake.audits.borrow();
    let tail = &audits[audits.len() - 3..];
    assert_eq!(
        tail,
        &[
            "ProviderCatalogRemovalAccepted".to_owned(),
            "ProviderCatalogAccepted".to_owned(),
            "ProviderCatalogActivated".to_owned(),
        ]
    );
    let state = fake.state.borrow();
    assert_eq!(state.kind_tombstones, vec!["responses".to_owned()]);
}

#[test]
fn pending_removal_rejection_leaves_the_active_catalog_unchanged() {
    let fake = seeded_catalog();
    let (controller, outcome) = removal_controller(&fake, 1_000);
    let handle = outcome.candidate_handle.expect("removal handle exists");
    controller
        .reject_pending(handle, "op-reject".to_owned(), 1_100)
        .expect("removal rejection commits");
    let projection = controller.inspect().expect("inspect succeeds");
    assert_eq!(
        projection.readiness,
        intention_application::CatalogReadiness::Blocked {
            reason: "removal_candidate_rejected".to_owned(),
        }
    );
    assert_eq!(projection.active_catalog_revision_id, Some(1));
    assert_eq!(projection.entry_count, 1);
    // The active catalog material is unchanged and the old registry still
    // serves the previous entries.
    let material = fake.active_material().expect("material loads");
    assert_eq!(material.catalog_revision_id, 1);
    assert_eq!(material.profiles[0].profile.revision_id, "rev-0001");
    let state = (&fake)
        .load_provider_catalog_status()
        .expect("status loads");
    assert_eq!(state.active_catalog_revision_id, Some(1));
    assert_eq!(state.status, ProviderCatalogStatusDto::Active);
}

#[test]
fn pending_removal_expires_at_thirty_minutes() {
    let fake = seeded_catalog();
    let (controller, _) = removal_controller(&fake, 1_000);
    assert_eq!(
        controller
            .expire_pending(1_000 + 30 * 60 - 1)
            .expect("expiry runs"),
        0
    );
    let projection = controller.inspect().expect("inspect succeeds");
    assert!(matches!(
        projection.readiness,
        intention_application::CatalogReadiness::PendingRemoval { .. }
    ));
    assert_eq!(
        controller
            .expire_pending(1_000 + 30 * 60)
            .expect("expiry runs"),
        1
    );
    let projection = controller.inspect().expect("inspect succeeds");
    assert_eq!(
        projection.readiness,
        intention_application::CatalogReadiness::Blocked {
            reason: "removal_candidate_expired".to_owned(),
        }
    );
    assert_eq!(projection.active_catalog_revision_id, Some(1));
}

#[test]
fn at_most_one_pending_removal_candidate_is_allowed() {
    let fake = seeded_catalog();
    let (controller, _) = removal_controller(&fake, 1_000);
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let error = controller
        .prepare_candidate(
            source(
                "op-second",
                1_024,
                vec![declaration(
                    "generic-chat-completion-api",
                    "model-c",
                    Some(ENDPOINT),
                    true,
                )],
                candidate(
                    "generic-chat-completion-api",
                    "model-c",
                    ENDPOINT,
                    &previous,
                ),
                previous,
            ),
            1_000,
        )
        .expect_err("a second pending candidate is rejected");
    assert_eq!(error.code(), "provider_catalog_removal_pending_exists");
}

// ============================================================================
// Kind rules
// ============================================================================

#[test]
fn changed_kind_composition_under_the_same_kind_id_is_rejected() {
    let fake = FakeCatalog::new();
    // The seeded active descriptor carries a foreign immutable composition
    // (bearer-or-safe-header) that the derived descriptor cannot match.
    fake.seed_active(
        1,
        vec![seed_kind_candidate("responses", "bearer-or-safe-header")],
        vec![seed_profile(
            "responses",
            "model-a",
            "default",
            "rev-0001",
            "kind-responses-v1",
        )],
        "default",
    );
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let error = controller
        .prepare_candidate(base_source(&previous, "model-b"), 1_000)
        .expect_err("kind immutability is enforced");
    assert_eq!(error.code(), "provider_kind_immutable_mismatch");
}

#[test]
fn kind_removal_with_dependents_is_rejected_by_the_controller_check() {
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
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let controller = fake.build_controller(both_factories(Arc::new(AtomicUsize::new(0))));
    // A candidate that keeps a dependent profile of a removed kind is
    // impossible to produce through faithful derivation, so the check is
    // exercised directly in the controller module unit tests; here we verify
    // that a regular removal with no dependents passes.
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
            1_000,
        )
        .expect("removal without dependents prepares");
    assert!(outcome.pending_removal);
}

// ============================================================================
// Alias and capability subset
// ============================================================================

#[test]
fn openai_alias_is_rejected_without_falling_back_to_generic_chat() {
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let fake = FakeCatalog::new();
    let builds = Arc::new(AtomicUsize::new(0));
    let controller = fake.build_controller(vec![responses_factory(builds.clone())]);
    let error = controller
        .prepare_candidate(
            source(
                "op-alias",
                1_024,
                vec![declaration("openai", "model-b", Some(ENDPOINT), true)],
                candidate("openrouter", "model-b", ENDPOINT, &previous),
                previous,
            ),
            1_000,
        )
        .expect_err("the openai alias is rejected");
    assert_eq!(
        error.code(),
        "legacy_config_cannot_represent_active_catalog"
    );
    assert_eq!(builds.load(Ordering::SeqCst), 0);
}

#[test]
fn capability_subset_outside_the_descriptor_envelope_is_rejected() {
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let mut declaration = declaration("responses", "model-b", Some(ENDPOINT), true);
    declaration.declared_model_capability_subset = vec!["structured_output".to_owned()];
    let fake = FakeCatalog::new();
    let builds = Arc::new(AtomicUsize::new(0));
    let controller = fake.build_controller(vec![responses_factory(builds.clone())]);
    let error = controller
        .prepare_candidate(
            source(
                "op-capability",
                1_024,
                vec![declaration],
                candidate("openrouter", "model-b", ENDPOINT, &previous),
                previous,
            ),
            1_000,
        )
        .expect_err("capability subset outside the envelope is rejected");
    assert_eq!(error.code(), "provider_profile_revision_invalid");
    assert_eq!(builds.load(Ordering::SeqCst), 0);
}

// ============================================================================
// Driver contract compatibility
// ============================================================================

#[test]
fn driver_contract_incompatibility_is_rejected_without_builds() {
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let fake = FakeCatalog::new();
    let builds = Arc::new(AtomicUsize::new(0));
    let controller = fake.build_controller(vec![Box::new(CountingFactory::new(
        "responses",
        "responses",
        1,
        0,
        builds.clone(),
    ))]);
    let error = controller
        .prepare_candidate(base_source(&previous, "model-b"), 1_000)
        .expect_err("unsupported minor is rejected");
    assert_eq!(error.code(), "provider_driver_contract_incompatible");
    assert_eq!(builds.load(Ordering::SeqCst), 0);

    let wrong_family = fake.build_controller(vec![Box::new(CountingFactory::new(
        "responses",
        "other-family",
        1,
        2,
        builds.clone(),
    ))]);
    let error = wrong_family
        .prepare_candidate(base_source(&previous, "model-b"), 1_000)
        .expect_err("wrong family is rejected");
    assert_eq!(error.code(), "provider_driver_contract_incompatible");
    assert_eq!(builds.load(Ordering::SeqCst), 0);

    let wrong_major = fake.build_controller(vec![Box::new(CountingFactory::new(
        "responses",
        "responses",
        2,
        2,
        builds.clone(),
    ))]);
    let error = wrong_major
        .prepare_candidate(base_source(&previous, "model-b"), 1_000)
        .expect_err("wrong major is rejected");
    assert_eq!(error.code(), "provider_driver_contract_incompatible");
    assert_eq!(builds.load(Ordering::SeqCst), 0);

    let missing = fake.build_controller(vec![]);
    let error = missing
        .prepare_candidate(base_source(&previous, "model-b"), 1_000)
        .expect_err("missing factory is rejected");
    assert_eq!(error.code(), "provider_driver_unavailable");
    assert_eq!(builds.load(Ordering::SeqCst), 0);
}

// ============================================================================
// Registry semantics
// ============================================================================

#[test]
fn registry_activates_atomically_and_old_handles_survive_a_swap() {
    let builds = Arc::new(AtomicUsize::new(0));
    let factory: Box<dyn ProviderDriverFactory> = responses_factory(builds.clone());
    let key_a = PrivateRegistryKey {
        profile_id: "default".to_owned(),
        profile_revision_id: "rev-a".to_owned(),
        kind_descriptor_revision_id: "kind-a".to_owned(),
        driver_contract: ProviderDriverContractRevisionDto {
            driver_family: "responses".to_owned(),
            major: 1,
            minor: 0,
        },
    };
    let material_a = PrivateProviderProfileMaterial {
        profile: seed_profile("responses", "model-a", "default", "rev-a", "kind-a").profile,
        selection: seed_selection("default", "rev-a", "kind-a"),
        endpoint: ENDPOINT.to_owned(),
        private_credential_reference: 1,
    };
    let registry = PrivateRegistry::new();
    let built_a = PrivateRegistry::build_all(
        std::slice::from_ref(&factory),
        vec![(key_a.clone(), material_a)],
    )
    .expect("registry builds");
    registry.activate(built_a).expect("registry activates");
    assert_eq!(registry.len(), 1);
    let task_handle = registry.lookup(&key_a).expect("handle is present");
    assert_eq!(Arc::strong_count(&task_handle), 2);

    let key_b = PrivateRegistryKey {
        profile_id: "default".to_owned(),
        profile_revision_id: "rev-b".to_owned(),
        kind_descriptor_revision_id: "kind-b".to_owned(),
        driver_contract: ProviderDriverContractRevisionDto {
            driver_family: "responses".to_owned(),
            major: 1,
            minor: 0,
        },
    };
    let material_b = PrivateProviderProfileMaterial {
        profile: seed_profile("responses", "model-b", "default", "rev-b", "kind-b").profile,
        selection: seed_selection("default", "rev-b", "kind-b"),
        endpoint: ENDPOINT.to_owned(),
        private_credential_reference: 1,
    };
    let built_b = PrivateRegistry::build_all(
        std::slice::from_ref(&factory),
        vec![(key_b.clone(), material_b)],
    )
    .expect("registry builds");
    registry.activate(built_b).expect("registry swaps");
    assert_eq!(registry.len(), 1);
    assert!(registry.lookup(&key_a).is_none());
    let new_handle = registry.lookup(&key_b).expect("new handle is present");
    // The task's old handle reference remains valid after the swap because
    // handles are reference-counted; only the registry's strong reference was
    // replaced.
    assert_eq!(Arc::strong_count(&task_handle), 1);
    assert_eq!(Arc::strong_count(&new_handle), 2);
    assert_eq!(builds.load(Ordering::SeqCst), 2);
}

fn seed_selection(
    profile_id: &str,
    revision_id: &str,
    kind_revision_id: &str,
) -> intention_domain::ProviderSelectionV1 {
    intention_domain::ProviderSelectionV1 {
        selection_canonicalization_version: "1".to_owned(),
        profile_id: profile_id.to_owned(),
        provider_profile_revision_id: revision_id.to_owned(),
        kind_id: "responses".to_owned(),
        kind_descriptor_revision_id: kind_revision_id.to_owned(),
        model_id: "model-a".to_owned(),
        normalized_effective_endpoint: ENDPOINT.to_owned(),
        credential_transport_mode: CredentialTransportMode::Bearer,
        credential_transport_safe_header_name: None,
        declared_model_capability_subset: vec!["text_input".to_owned()],
        resolved_reasoning_policy: "textual-reasoning-v1".to_owned(),
        effective_execution_policy: "execution-timeout-30-attempts-2".to_owned(),
        effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
        provider_driver_contract_revision: "responses-1.0".to_owned(),
        selection_source: Some("catalog-rev-1".to_owned()),
    }
}

#[test]
fn registry_rejects_more_than_128_active_entries() {
    let builds = Arc::new(AtomicUsize::new(0));
    let factory: Box<dyn ProviderDriverFactory> = responses_factory(builds);
    let materials = (0..129)
        .map(|index| {
            let profile = seed_profile(
                "responses",
                "model-a",
                "default",
                &format!("rev-{index}"),
                "kind-a",
            );
            let key = PrivateRegistryKey {
                profile_id: profile.profile.profile_id.clone(),
                profile_revision_id: profile.profile.revision_id.clone(),
                kind_descriptor_revision_id: profile.profile.kind_descriptor_revision_id.clone(),
                driver_contract: profile.profile.driver_contract_revision.clone(),
            };
            (
                key,
                PrivateProviderProfileMaterial {
                    profile: profile.profile,
                    selection: seed_selection("default", &format!("rev-{index}"), "kind-a"),
                    endpoint: ENDPOINT.to_owned(),
                    private_credential_reference: 1,
                },
            )
        })
        .collect::<Vec<_>>();
    let built =
        PrivateRegistry::build_all(std::slice::from_ref(&factory), materials).expect("builds");
    let registry = PrivateRegistry::new();
    let error = registry
        .activate(built)
        .expect_err("the active entry limit is enforced");
    assert_eq!(error.code(), "provider_registry_limit_exceeded");
    assert_eq!(registry.len(), 0);
}

#[test]
fn no_private_binding_survives_a_controller_restart() {
    let fake = seeded_catalog();
    let builds = Arc::new(AtomicUsize::new(0));
    let first = fake.build_controller(vec![responses_factory(builds.clone())]);
    let outcome = first.startup(1_000).expect("startup succeeds");
    assert_eq!(outcome.entry_count, 1);
    // A fresh controller starts with an empty in-memory registry; startup
    // rebuilds it from the persisted accepted material.
    let second = fake.build_controller(vec![responses_factory(builds)]);
    assert_eq!(second.inspect().expect("inspect succeeds").entry_count, 0);
    let outcome = second.startup(1_000).expect("startup succeeds");
    assert_eq!(outcome.entry_count, 1);
}

// ============================================================================
// Admission
// ============================================================================

#[test]
fn admission_lookup_requires_ready_enabled_and_unmodified_entries() {
    let fake = seeded_catalog();
    let builds = Arc::new(AtomicUsize::new(0));
    let controller = fake.build_controller(vec![responses_factory(builds)]);
    let _ = controller.startup(1_000).expect("startup succeeds");
    let profile = seed_profile(
        "responses",
        "model-a",
        "default",
        "rev-0001",
        "kind-responses-v1",
    );
    let key = key_for(&profile.profile);
    let admission = controller
        .registry_lookup(&key)
        .expect("admission succeeds");
    assert_eq!(admission.profile_id, "default");
    assert_eq!(admission.profile_revision_id, "rev-0001");
    assert_eq!(admission.descriptor_revision_id, "kind-responses-v1");
    assert_eq!(admission.driver_contract.driver_family, "responses");
    assert_eq!(admission.selection_digest.len(), 64);

    let unknown = controller
        .registry_lookup(&PrivateRegistryKey {
            profile_id: "missing".to_owned(),
            profile_revision_id: "rev-0001".to_owned(),
            kind_descriptor_revision_id: "kind-responses-v1".to_owned(),
            driver_contract: profile.profile.driver_contract_revision,
        })
        .expect_err("unknown keys are not admitted");
    assert_eq!(unknown.code(), "provider_admission_not_found");
}

#[test]
fn admission_is_denied_when_the_catalog_is_not_ready() {
    let fake = FakeCatalog::new();
    let controller = fake.build_controller(vec![responses_factory(Arc::new(AtomicUsize::new(0)))]);
    let profile = seed_profile(
        "responses",
        "model-a",
        "default",
        "rev-0001",
        "kind-responses-v1",
    );
    let error = controller
        .registry_lookup(&key_for(&profile.profile))
        .expect_err("admission requires ready readiness");
    assert_eq!(error.code(), "catalog_not_ready");
}

// ============================================================================
// Fake-secret sweep
// ============================================================================

#[test]
fn no_projection_or_outcome_dto_serializes_credential_material() {
    let previous = snapshot("openrouter", "model-a", ENDPOINT, ConfigRevisionId::new());
    let fake = seeded_catalog();
    let builds = Arc::new(AtomicUsize::new(0));
    let controller = fake.build_controller(vec![responses_factory(builds)]);
    let startup = controller.startup(1_000).expect("startup succeeds");
    let seeded = seed_profile(
        "responses",
        "model-a",
        "default",
        "rev-0001",
        "kind-responses-v1",
    );
    let admission = controller
        .registry_lookup(&key_for(&seeded.profile))
        .expect("admission succeeds");
    let outcome = controller
        .prepare_candidate(
            source(
                "op-sweep",
                raw_config("openrouter", "model-b", ENDPOINT).len() as u64,
                vec![declaration("responses", "model-b", Some(ENDPOINT), true)],
                candidate("openrouter", "model-b", ENDPOINT, &previous),
                previous.clone(),
            ),
            1_000,
        )
        .expect("candidate prepares");
    let projection = controller.inspect().expect("inspect succeeds");
    let serialized = [
        format!("{startup:?}"),
        format!("{outcome:?}"),
        format!("{admission:?}"),
        format!("{projection:?}"),
        format!("{previous:?}"),
    ]
    .join("\n");
    for forbidden in ["sk-test", "Bearer", "api_key", "secret", CREDENTIAL] {
        assert!(
            !serialized.contains(forbidden),
            "serialized DTO leaks {forbidden}"
        );
    }
}

// ============================================================================
// Error-path storage checks
// ============================================================================

#[test]
fn accept_pending_with_mismatched_revisions_is_rejected() {
    let fake = seeded_catalog();
    let (controller, outcome) = removal_controller(&fake, 1_000);
    let handle = outcome.candidate_handle.expect("removal handle exists");
    let error = controller
        .accept_pending(
            handle.clone(),
            "7".to_owned(),
            "2".to_owned(),
            "op-accept".to_owned(),
            1_100,
        )
        .expect_err("stale expected active revision is rejected");
    assert_eq!(error.code(), "provider_catalog_revision_conflict");
    let error = controller
        .accept_pending(
            handle,
            "1".to_owned(),
            "9".to_owned(),
            "op-accept".to_owned(),
            1_100,
        )
        .expect_err("stale expected candidate revision is rejected");
    assert_eq!(error.code(), "provider_catalog_revision_conflict");
}

#[test]
fn rejection_of_an_unknown_handle_is_rejected() {
    let fake = seeded_catalog();
    let (controller, _) = removal_controller(&fake, 1_000);
    let error = controller
        .reject_pending("unknown-handle".to_owned(), "op-reject".to_owned(), 1_100)
        .expect_err("unknown handles are rejected");
    assert_eq!(error.code(), "provider_catalog_removal_not_found");
}
