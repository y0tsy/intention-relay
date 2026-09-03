//! The provider catalog runtime controller (zone 6a).
//!
//! This controller owns the catalog runtime: all-or-nothing startup,
//! candidate preparation, pending-removal acceptance/rejection/expiry,
//! admission lookups, and the private driver registry. Every public method is
//! DTO-only and credential-free; no private handle, credential, SDK resource,
//! or raw configuration crosses a public boundary.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use intention_config::control_plane::{
    CandidateIssueDto, ConfigCandidateDto, MAX_CANDIDATE_RAW_BYTES, classify_changed_fields,
    semantic_equivalence,
};
use intention_domain::{
    ContextPreservationCapability, CredentialTransportMode, ModelCapabilitySelectionV1,
    ModelCapabilitySetV1, ModelInputCapability, ProviderDriverContractRevisionDto,
    ProviderKindDescriptorRevisionV1, ProviderProfileRevisionV1, ProviderSelectionV1,
    ReasoningCapability, StructuredOutputCapability, canonical::CanonicalError,
    canonical::Digest256, provider_catalog::MAX_CATALOG_ISSUES,
    provider_catalog::MAX_PROVIDER_KINDS, provider_catalog::MAX_PROVIDER_PROFILES,
    provider_selection::MODEL_CAPABILITY_TAXONOMY_V1,
    provider_selection::PROVIDER_SELECTION_CANONICALIZATION_VERSION, validate_provider_kind_id,
    validate_provider_kind_removal, validate_provider_kind_revision_immutability,
};
use intention_storage::{
    AcceptProviderCatalogInputDto, AcceptProviderCatalogRemovalInputDto,
    AppendProviderKindDescriptorRevisionInputDto, AppendProviderProfileRevisionInputDto,
    CreateProviderCatalogRemovalCandidateInputDto, ExpireProviderCatalogRemovalCandidateInputDto,
    PendingRemovalCandidateDto, ProviderCatalogMaterialDto, ProviderCatalogRemovalStatusDto,
    ProviderCatalogRepositoryDto, ProviderCatalogStatusDto, ProviderKindDescriptorCandidateDto,
    ProviderProfileCandidateDto, ProviderReadinessDto, ProviderRemovalRepositoryDto,
    RejectProviderCatalogRemovalInputDto,
};
use intention_types::{DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto};

use crate::provider_gate::{CatalogReadiness, ControlPlaneGate};
use crate::provider_registry::{
    ModelRunDriverHandle, PrivateProviderProfileMaterial, PrivateRegistry, PrivateRegistryKey,
    ProviderDriverFactory, private_credential_reference,
};

/// The provider catalog removal candidate lifetime in seconds (30 minutes).
const REMOVAL_CANDIDATE_LIFETIME_SECONDS: u64 = 30 * 60;
/// The deterministic resolved reasoning policy of Slice 2 catalog profiles.
const RESOLVED_REASONING_POLICY: &str = "textual-reasoning-v1";
/// The deterministic loopback policy of Slice 2 catalog profiles.
const LOOPBACK_POLICY_NOT_APPLICABLE: &str = "not-applicable";
/// The deterministic first-party default profile id.
const DEFAULT_PROFILE_ID: &str = "default";

/// One credential-free provider declaration inside a catalog source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogProviderDeclarationDto {
    /// The provider kind as declared before config-crate normalization.
    pub kind: String,
    /// The configured model identifier.
    pub model: String,
    /// The configured endpoint, or `None` for the deterministic placeholder.
    pub endpoint: Option<String>,
    /// The declared model capability subset to intersect with the descriptor envelope.
    pub declared_model_capability_subset: Vec<String>,
    /// Whether the derived profile is enabled for admission.
    pub enabled: bool,
}

/// Credential-free input to prepare one catalog candidate.
///
/// The raw candidate document is parsed upstream by the reload/rotation zone
/// through [`intention_config::control_plane::parse_candidate`]; this DTO
/// carries only the size of the raw document, the declared provider kind, the
/// safe parsed candidate, and the active snapshot for comparison. Raw TOML and
/// credential material never cross this boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogSourceInputDto {
    /// The caller-owned operation identity for audit idempotency.
    pub operation_id: String,
    /// The byte size of the raw candidate document (size-only disclosure).
    pub raw_config_size_bytes: u64,
    /// The provider declarations derived from the resolved candidate.
    pub providers: Vec<CatalogProviderDeclarationDto>,
    /// The already-parsed credential-free reload candidate.
    pub candidate: ConfigCandidateDto,
    /// The active configuration snapshot for semantic comparison.
    pub previous: intention_config::ConfigSnapshotDto,
}

/// The outcome of one catalog candidate preparation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogCandidateOutcomeDto {
    /// Whether the candidate changed catalog semantics (a new revision exists).
    pub changed: bool,
    /// The prepared catalog revision, when one was prepared.
    pub catalog_revision_id: Option<u64>,
    /// The pending removal candidate handle, when a removal is pending.
    pub candidate_handle: Option<String>,
    /// Whether the candidate entered the pending-removal state.
    pub pending_removal: bool,
    /// The pending removal expiry time, when one applies.
    pub removal_expires_at: Option<u64>,
    /// The catalog readiness after preparation.
    pub readiness: CatalogReadiness,
    /// The bounded safe validation issues of the candidate.
    pub issues: Vec<CandidateIssueDto>,
    /// Whether more issues exist than the bounded list carries.
    pub truncated_issues: bool,
    /// The complete issue count before bounding.
    pub total_issue_count: u32,
}

/// The outcome of one pending-removal acceptance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogAcceptanceOutcomeDto {
    /// The committed catalog revision.
    pub catalog_revision_id: u64,
    /// The accepted removal candidate handle.
    pub candidate_handle: String,
    /// The catalog readiness after acceptance.
    pub readiness: CatalogReadiness,
    /// The number of active private registry entries after acceptance.
    pub entry_count: usize,
    /// The profile ids tombstoned by this acceptance.
    pub removed_profile_ids: Vec<String>,
    /// The kind ids tombstoned by this acceptance.
    pub removed_kind_ids: Vec<String>,
}

/// The all-or-nothing catalog startup outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogStartupOutcomeDto {
    /// The catalog readiness after startup.
    pub readiness: CatalogReadiness,
    /// The active catalog revision, when one is applied.
    pub active_catalog_revision_id: Option<u64>,
    /// The number of active private registry entries.
    pub entry_count: usize,
    /// The safe degraded reason, when the catalog is degraded.
    pub degraded_reason: Option<String>,
}

/// A safe projection of the provider catalog runtime state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCatalogProjectionDto {
    /// The current catalog readiness.
    pub readiness: CatalogReadiness,
    /// The active catalog revision, when one is applied.
    pub active_catalog_revision_id: Option<u64>,
    /// The prepared-but-not-accepted candidate revision, when one exists.
    pub candidate_catalog_revision_id: Option<u64>,
    /// The active default profile id, when one exists.
    pub active_default_profile_id: Option<String>,
    /// The safe degraded reason, when the catalog is degraded.
    pub degraded_reason: Option<String>,
    /// The number of active private registry entries.
    pub entry_count: usize,
}

/// One credential-free provider admission decision.
///
/// No private handle crosses this boundary; the controller pairs the DTO with
/// the private handle internally for later runtime wiring.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderAdmissionDto {
    pub profile_id: String,
    pub profile_revision_id: String,
    pub descriptor_revision_id: String,
    pub driver_contract: ProviderDriverContractRevisionDto,
    pub selection_digest: String,
}

/// The in-memory prepared candidate retained for pending-removal acceptance.
pub(crate) struct PreparedCandidate {
    pub(crate) candidate_handle: String,
    pub(crate) catalog_revision_id: u64,
    pub(crate) kind_descriptors: Vec<ProviderKindDescriptorCandidateDto>,
    pub(crate) profiles: Vec<ProviderProfileCandidateDto>,
    pub(crate) default_profile_id: String,
    pub(crate) removed_profile_ids: Vec<String>,
    pub(crate) removed_kind_ids: Vec<String>,
}

/// One admission-context entry paired with the private registry.
struct AdmissionEntry {
    dto: ProviderAdmissionDto,
    enabled: bool,
}

/// The built private registry entry map (opaque handles).
type BuiltRegistryEntries =
    HashMap<PrivateRegistryKey, Arc<dyn ModelRunDriverHandle + Send + Sync>>;
/// The admission context map paired with the private registry.
type AdmissionContext = HashMap<PrivateRegistryKey, AdmissionEntry>;

/// The controller-side tombstone sets for admission checks.
#[derive(Default)]
struct TombstoneSets {
    profile_ids: HashSet<String>,
    kind_ids: HashSet<String>,
}

/// The provider catalog runtime controller.
pub struct ProviderCatalogController<Catalog, Removal>
where
    Catalog: ProviderCatalogRepositoryDto,
    Removal: ProviderRemovalRepositoryDto,
{
    catalog: Catalog,
    removal: Removal,
    factories: Vec<Box<dyn ProviderDriverFactory>>,
    gate: ControlPlaneGate,
    registry: PrivateRegistry,
    admissions: Mutex<HashMap<PrivateRegistryKey, AdmissionEntry>>,
    tombstones: Mutex<TombstoneSets>,
}

impl<Catalog, Removal> ProviderCatalogController<Catalog, Removal>
where
    Catalog: ProviderCatalogRepositoryDto,
    Removal: ProviderRemovalRepositoryDto,
{
    /// Creates a catalog controller over the supplied storage repositories and
    /// registered driver factories.
    #[must_use]
    pub fn new(
        catalog: Catalog,
        removal: Removal,
        factories: Vec<Box<dyn ProviderDriverFactory>>,
    ) -> Self {
        Self {
            catalog,
            removal,
            factories,
            gate: ControlPlaneGate::new(),
            registry: PrivateRegistry::new(),
            admissions: Mutex::new(HashMap::new()),
            tombstones: Mutex::new(TombstoneSets::default()),
        }
    }

    /// Runs the all-or-nothing catalog startup.
    ///
    /// A valid active catalog is rebuilt into the private registry and the
    /// controller reaches `Ready`. An `activation_recovery_required` state is
    /// recovered exactly from the accepted revisions (never from current
    /// configuration); a mismatch or reconstruction failure leaves the
    /// controller in `ActivationRecoveryRequired` or `Blocked` with no partial
    /// registry. Any corruption or unsupported version degrades to `Blocked`.
    ///
    /// Pending removal is durable: startup drives expiry from the durable
    /// deadline, rolls forward an already-accepted removal whose catalog
    /// acceptance never committed (PR24-004), and otherwise rebuilds the
    /// prepared candidate and its real deadline from durable rows so
    /// accept/reject never depend on process memory (PR24-003).
    ///
    /// # Errors
    ///
    /// Returns an unavailable error only when the control-plane gate is
    /// poisoned; all catalog failures degrade to a typed readiness state.
    pub fn startup(&self, now: u64) -> DtoResult<CatalogStartupOutcomeDto> {
        let mut state = match self.catalog.load_provider_catalog_status() {
            Ok(state) => state,
            Err(error) => {
                return self.blocked(error.code());
            }
        };
        // One prepared candidate may exist per pending-removal state; a row
        // missing under a pending state is inconsistent.
        let mut pending_rebuild: Option<PendingRemovalCandidateDto> = None;
        if state.status == ProviderCatalogStatusDto::PendingRemoval {
            let Some(pending) = self.removal.load_pending_removal_candidate()? else {
                return self.blocked("catalog_state_inconsistent_missing_removal_row");
            };
            if pending.removal_status == ProviderCatalogRemovalStatusDto::Accepted {
                // The removal acceptance committed but the catalog acceptance
                // did not: roll the acceptance forward from the durable
                // prepared material (PR24-004).
                return self.roll_forward_acceptance(&pending, now);
            }
            if pending.expires_at <= i64_time(now) {
                self.expire_pending(now)?;
                state = match self.catalog.load_provider_catalog_status() {
                    Ok(state) => state,
                    Err(error) => {
                        return self.blocked(error.code());
                    }
                };
            } else {
                pending_rebuild = Some(pending);
            }
        }
        let status = state.status;
        let active_revision = state.active_catalog_revision_id;
        if status == ProviderCatalogStatusDto::ActivationRecoveryRequired {
            return self.startup_recovery(active_revision, now);
        }
        let Some(active) = active_revision else {
            // A pending removal requires an active baseline catalog.
            if pending_rebuild.is_some() {
                return self.blocked("catalog_state_inconsistent_pending_without_active");
            }
            // No active catalog: the empty registry is ready.
            self.gate.run_exclusive(|gate| {
                gate.readiness = CatalogReadiness::Ready;
                gate.applied_revision = None;
                gate.active_default_profile_id = None;
                gate.candidate_catalog_revision_id = state.candidate_catalog_revision_id;
                gate.degraded_reason = None;
                gate.prepared = None;
                Ok(())
            })?;
            return self.startup_outcome();
        };
        let material = match self.catalog.load_provider_catalog_material() {
            Ok(material) if material.catalog_revision_id == active => material,
            Ok(_) => return self.blocked("catalog_state_inconsistent"),
            Err(error) => return self.blocked(error.code()),
        };
        let (built, admissions) = match self.build_registry_from_material(&material) {
            Ok(built) => built,
            Err(error) => return self.blocked(error.code()),
        };
        // Rebuild the prepared candidate (when one is still pending) from the
        // durable candidate material so a later accept/reject works after a
        // restart, and preserve the real durable deadline in readiness.
        let (readiness, prepared) = if let Some(pending) = &pending_rebuild {
            let candidate_material = match self.catalog.load_prepared_catalog_material() {
                Ok(candidate_material)
                    if candidate_material.catalog_revision_id
                        == pending.candidate_catalog_revision_id =>
                {
                    candidate_material
                }
                Ok(_) | Err(_) => {
                    return self.blocked("catalog_state_inconsistent_candidate_material");
                }
            };
            let deadline = u64::try_from(pending.expires_at).unwrap_or(0);
            let rebuilt = PreparedCandidate {
                candidate_handle: pending.candidate_handle.clone(),
                catalog_revision_id: pending.candidate_catalog_revision_id,
                kind_descriptors: candidate_material.kind_descriptors,
                profiles: candidate_material.profiles,
                default_profile_id: DEFAULT_PROFILE_ID.to_owned(),
                removed_profile_ids: pending.removed_profile_ids.clone(),
                removed_kind_ids: pending.removed_kind_ids.clone(),
            };
            (
                CatalogReadiness::PendingRemoval {
                    candidate_revision: pending.candidate_catalog_revision_id.to_string(),
                    expires_at: deadline,
                },
                Some(rebuilt),
            )
        } else {
            (CatalogReadiness::Ready, None)
        };
        self.gate.run_exclusive(|gate| {
            gate.readiness = readiness;
            gate.applied_revision = Some(active);
            gate.active_default_profile_id = material.default_profile_id.clone();
            gate.candidate_catalog_revision_id = state.candidate_catalog_revision_id;
            gate.degraded_reason = state.degraded_reason.clone();
            gate.prepared = prepared;
            Ok(())
        })?;
        self.activate_registry(built, admissions)?;
        self.startup_outcome()
    }

    /// Rolls forward one removal acceptance whose catalog acceptance never
    /// committed (crash between the two durable commits).
    ///
    /// The prepared material is durable, so the catalog acceptance, private
    /// registry build, and gate activation are completed exactly once from
    /// that material; repeated roll-forward attempts are idempotent through
    /// the storage acceptance path (PR24-004).
    fn roll_forward_acceptance(
        &self,
        pending: &PendingRemovalCandidateDto,
        now: u64,
    ) -> DtoResult<CatalogStartupOutcomeDto> {
        let material = match self.catalog.load_prepared_catalog_material() {
            Ok(material)
                if material.catalog_revision_id == pending.candidate_catalog_revision_id =>
            {
                material
            }
            Ok(_) | Err(_) => return self.blocked("catalog_state_inconsistent_roll_forward"),
        };
        let accepted_at = i64_time(now);
        let operation_id = format!("recovery-roll-forward-{}", pending.candidate_handle);
        if self
            .catalog
            .accept_provider_catalog(AcceptProviderCatalogInputDto {
                catalog_revision_id: pending.candidate_catalog_revision_id,
                candidate_handle: pending.candidate_handle.clone(),
                kind_descriptors: material.kind_descriptors.clone(),
                profiles: material.profiles.clone(),
                default_profile_id: DEFAULT_PROFILE_ID.to_owned(),
                accepted_at,
                operation_id,
            })
            .is_err()
        {
            return self.blocked("activation_recovery_failed");
        }
        let (built, admissions) = match self.build_registry_from_candidate(
            &material.kind_descriptors,
            &material.profiles,
            pending.candidate_catalog_revision_id,
        ) {
            Ok(built) => built,
            Err(_) => return self.blocked("activation_recovery_failed"),
        };
        self.gate.run_exclusive(|gate| {
            gate.readiness = CatalogReadiness::Ready;
            gate.applied_revision = Some(pending.candidate_catalog_revision_id);
            gate.active_default_profile_id = Some(DEFAULT_PROFILE_ID.to_owned());
            gate.candidate_catalog_revision_id = None;
            gate.degraded_reason = None;
            gate.prepared = None;
            Ok(())
        })?;
        self.activate_registry(built, admissions)?;
        self.startup_outcome()
    }

    /// Attempts exact activation recovery from the accepted revisions.
    fn startup_recovery(
        &self,
        accepted: Option<u64>,
        now: u64,
    ) -> DtoResult<CatalogStartupOutcomeDto> {
        let Some(accepted) = accepted else {
            return self.recovery_required(0, "activation_recovery_unavailable");
        };
        let material = match self.catalog.load_provider_catalog_material() {
            Ok(material) if material.catalog_revision_id == accepted => material,
            _ => return self.recovery_required(accepted, "activation_recovery_mismatch"),
        };
        let (built, admissions) = match self.build_registry_from_material(&material) {
            Ok(built) => built,
            Err(_) => return self.recovery_required(accepted, "activation_recovery_failed"),
        };
        // Record the recovery through the accept/append path: idempotent
        // re-append and re-accept persist the recovery evidence and restore
        // the durable active state.
        if self.record_recovery(&material, accepted, now).is_err() {
            return self.recovery_required(accepted, "activation_recovery_failed");
        }
        self.gate.run_exclusive(|gate| {
            gate.readiness = CatalogReadiness::Ready;
            gate.applied_revision = Some(accepted);
            gate.active_default_profile_id = material.default_profile_id.clone();
            gate.candidate_catalog_revision_id = None;
            gate.degraded_reason = None;
            gate.prepared = None;
            Ok(())
        })?;
        self.activate_registry(built, admissions)?;
        self.startup_outcome()
    }

    /// Re-appends and re-accepts the accepted material to persist recovery
    /// evidence through the standard audit path.
    fn record_recovery(
        &self,
        material: &ProviderCatalogMaterialDto,
        accepted: u64,
        now: u64,
    ) -> DtoResult<()> {
        let operation_id = format!("recovery-{accepted}-{now}");
        for kind in &material.kind_descriptors {
            self.catalog.append_provider_kind_descriptor_revision(
                AppendProviderKindDescriptorRevisionInputDto {
                    descriptor_revision_id: kind.descriptor_revision_id.clone(),
                    descriptor: kind.descriptor.clone(),
                    catalog_revision_id: accepted,
                    accepted_at: i64_time(now),
                    operation_id: operation_id.clone(),
                },
            )?;
        }
        for profile in &material.profiles {
            self.catalog.append_provider_profile_revision(
                AppendProviderProfileRevisionInputDto {
                    profile: profile.clone(),
                    catalog_revision_id: accepted,
                    accepted_at: i64_time(now),
                    operation_id: operation_id.clone(),
                },
            )?;
        }
        self.catalog
            .accept_provider_catalog(AcceptProviderCatalogInputDto {
                catalog_revision_id: accepted,
                candidate_handle: format!("recovery-{accepted}"),
                kind_descriptors: material.kind_descriptors.clone(),
                profiles: material.profiles.clone(),
                default_profile_id: material
                    .default_profile_id
                    .clone()
                    .unwrap_or_else(|| DEFAULT_PROFILE_ID.to_owned()),
                accepted_at: i64_time(now),
                operation_id,
            })?;
        Ok(())
    }

    /// Returns a safe projection of the catalog runtime state.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the control-plane gate is poisoned.
    pub fn inspect(&self) -> DtoResult<ProviderCatalogProjectionDto> {
        self.gate.read(|state| ProviderCatalogProjectionDto {
            readiness: state.readiness.clone(),
            active_catalog_revision_id: state.applied_revision,
            candidate_catalog_revision_id: state.candidate_catalog_revision_id,
            active_default_profile_id: state.active_default_profile_id.clone(),
            degraded_reason: state.degraded_reason.clone(),
            entry_count: self.registry.len(),
        })
    }

    /// Prepares one catalog candidate from a credential-free source.
    ///
    /// The candidate is validated against the catalog limits, the `openai`
    /// alias is rejected, the capability subset is intersected with the
    /// descriptor envelope, the driver contract is verified against the
    /// registered factories, and kind immutability and removal rules are
    /// enforced. A semantically equal candidate produces no new revision. A
    /// candidate that removes active profiles or kinds enters the
    /// pending-removal state with a 30-minute expiry; any other candidate is
    /// auto-accepted and its private registry is activated.
    ///
    /// # Errors
    ///
    /// Returns a typed validation, policy, conflict, or storage error. No
    /// driver build is invoked on any preflight failure.
    pub fn prepare_candidate(
        &self,
        source: CatalogSourceInputDto,
        now: u64,
    ) -> DtoResult<CatalogCandidateOutcomeDto> {
        // Production expiry driver: an overdue pending candidate degrades
        // before any new proposal so a stale pending row never blocks a
        // corrected candidate (PR24-003).
        self.expire_pending(now)?;
        self.gate.run_exclusive(|gate| {
            if matches!(gate.readiness, CatalogReadiness::PendingRemoval { .. }) {
                return Err(catalog_error(
                    "provider_catalog_removal_pending_exists",
                    ErrorCategoryDto::Conflict,
                    "a pending provider catalog removal candidate already exists",
                ));
            }
            validate_candidate_limits(&source)?;
            // The catalog runtime classifies only a provider-kind change as a
            // removal-signaling configuration change: model/endpoint changes
            // are catalog replacement material for this path. Live-reload
            // admission of model/endpoint/kind changes is rejected earlier at
            // the reload boundary (PR24-008); this controller never receives
            // such a rejected candidate.
            let kind_changed =
                classify_changed_fields(source.candidate.safe_snapshot(), &source.previous)
                    .iter()
                    .any(|category| category == "provider_kind");
            let active = self.load_active_material()?;
            // A semantically equal candidate produces no new revision once a
            // catalog is active. When no catalog is active yet, the first
            // startup declaration is still accepted: the daemon activates its
            // startup provider configuration exactly once (ADR 0038 single
            // execution path; no selection-less fallback).
            if active.is_some()
                && semantic_equivalence(source.candidate.safe_snapshot(), &source.previous)
            {
                let (issues, truncated, total) = bounded_issues(&source.candidate);
                return Ok(CatalogCandidateOutcomeDto {
                    changed: false,
                    catalog_revision_id: None,
                    candidate_handle: None,
                    pending_removal: false,
                    removal_expires_at: None,
                    readiness: gate.readiness.clone(),
                    issues,
                    truncated_issues: truncated,
                    total_issue_count: total,
                });
            }
            let resolved = source.candidate.safe_snapshot().resolved();
            let (kind_descriptors, profiles) = self.build_candidate_records(&source, resolved)?;
            if let Some(active) = &active {
                for kind in &kind_descriptors {
                    if let Some(previous) = active
                        .kind_descriptors
                        .iter()
                        .find(|previous| previous.descriptor.kind_id == kind.descriptor.kind_id)
                    {
                        validate_provider_kind_revision_immutability(
                            &previous.descriptor,
                            &kind.descriptor,
                        )
                        .map_err(domain_error)?;
                    }
                }
            }
            let active_kinds = active
                .as_ref()
                .map(|material| {
                    material
                        .kind_descriptors
                        .iter()
                        .map(|kind| kind.descriptor.kind_id.clone())
                        .collect::<HashSet<_>>()
                })
                .unwrap_or_default();
            let active_profiles = active
                .as_ref()
                .map(|material| {
                    material
                        .profiles
                        .iter()
                        .map(|profile| profile.profile.profile_id.clone())
                        .collect::<HashSet<_>>()
                })
                .unwrap_or_default();
            let candidate_kinds = kind_descriptors
                .iter()
                .map(|kind| kind.descriptor.kind_id.clone())
                .collect::<HashSet<_>>();
            let candidate_profiles = profiles
                .iter()
                .map(|profile| profile.profile.profile_id.clone())
                .collect::<HashSet<_>>();
            let removed_kind_ids = active_kinds
                .difference(&candidate_kinds)
                .cloned()
                .collect::<Vec<_>>();
            let removed_profile_ids = active_profiles
                .difference(&candidate_profiles)
                .cloned()
                .collect::<Vec<_>>();
            validate_kind_removals(&removed_kind_ids, &profiles)?;
            let removal =
                kind_changed || !removed_kind_ids.is_empty() || !removed_profile_ids.is_empty();
            // Candidate revisions are durable monotonic identities: closed
            // removal rows stay in history, so a corrected or repeated
            // proposal after rejection/expiry must never reuse the closed
            // candidate's revision or handle (PR24-006).
            let applied_next = gate
                .applied_revision
                .map_or(1, |revision| revision.saturating_add(1));
            let highest_removal = self
                .removal
                .load_highest_removal_candidate_revision()
                .unwrap_or(0);
            let next_revision = applied_next.max(highest_removal.saturating_add(1));
            let candidate_handle = format!("catalog-{next_revision}");
            self.persist_prepared_candidate(
                &kind_descriptors,
                &profiles,
                next_revision,
                &source.operation_id,
                now,
            )?;
            let (issues, truncated, total) = bounded_issues(&source.candidate);
            if removal {
                let expires_at = now.saturating_add(REMOVAL_CANDIDATE_LIFETIME_SECONDS);
                let candidate_json =
                    removal_candidate_json(next_revision, &removed_profile_ids, &removed_kind_ids);
                self.removal.create_provider_catalog_removal_candidate(
                    CreateProviderCatalogRemovalCandidateInputDto {
                        candidate_handle: candidate_handle.clone(),
                        candidate_catalog_revision_id: next_revision,
                        active_catalog_revision_id: gate.applied_revision.unwrap_or(0),
                        created_at: i64_time(now),
                        source_recheck: "health-recheck".to_owned(),
                        candidate_json,
                        operation_id: source.operation_id.clone(),
                    },
                )?;
                gate.readiness = CatalogReadiness::PendingRemoval {
                    candidate_revision: next_revision.to_string(),
                    expires_at,
                };
                gate.candidate_catalog_revision_id = Some(next_revision);
                gate.prepared = Some(PreparedCandidate {
                    candidate_handle: candidate_handle.clone(),
                    catalog_revision_id: next_revision,
                    kind_descriptors,
                    profiles,
                    default_profile_id: DEFAULT_PROFILE_ID.to_owned(),
                    removed_profile_ids,
                    removed_kind_ids,
                });
                return Ok(CatalogCandidateOutcomeDto {
                    changed: true,
                    catalog_revision_id: Some(next_revision),
                    candidate_handle: Some(candidate_handle),
                    pending_removal: true,
                    removal_expires_at: Some(expires_at),
                    readiness: CatalogReadiness::PendingRemoval {
                        candidate_revision: next_revision.to_string(),
                        expires_at,
                    },
                    issues,
                    truncated_issues: truncated,
                    total_issue_count: total,
                });
            }
            self.catalog
                .accept_provider_catalog(AcceptProviderCatalogInputDto {
                    catalog_revision_id: next_revision,
                    candidate_handle,
                    kind_descriptors: kind_descriptors.clone(),
                    profiles: profiles.clone(),
                    default_profile_id: DEFAULT_PROFILE_ID.to_owned(),
                    accepted_at: i64_time(now),
                    operation_id: source.operation_id.clone(),
                })?;
            let (built, admissions) =
                self.build_registry_from_candidate(&kind_descriptors, &profiles, next_revision)?;
            self.activate_registry(built, admissions)?;
            let active_profile_ids = profiles
                .iter()
                .map(|profile| profile.profile.profile_id.clone())
                .collect::<Vec<_>>();
            let active_kind_ids = kind_descriptors
                .iter()
                .map(|kind| kind.descriptor.kind_id.clone())
                .collect::<Vec<_>>();
            self.record_tombstones(
                &removed_profile_ids,
                &removed_kind_ids,
                &active_profile_ids,
                &active_kind_ids,
            );
            gate.readiness = CatalogReadiness::Ready;
            gate.applied_revision = Some(next_revision);
            gate.active_default_profile_id = Some(DEFAULT_PROFILE_ID.to_owned());
            gate.candidate_catalog_revision_id = None;
            gate.degraded_reason = None;
            gate.prepared = None;
            Ok(CatalogCandidateOutcomeDto {
                changed: true,
                catalog_revision_id: Some(next_revision),
                candidate_handle: None,
                pending_removal: false,
                removal_expires_at: None,
                readiness: CatalogReadiness::Ready,
                issues,
                truncated_issues: truncated,
                total_issue_count: total,
            })
        })
    }

    /// Accepts one pending removal candidate atomically.
    ///
    /// The removal acceptance is committed through storage (removal accepted
    /// audit, then catalog accepted and activated audits), then the private
    /// registry is swapped. On a crash after acceptance, the startup path
    /// recovers the accepted catalog.
    ///
    /// # Errors
    ///
    /// Returns a not-found or conflict error when the candidate handle or the
    /// expected revisions do not match the prepared candidate, or a storage
    /// error when the atomic commit fails.
    pub fn accept_pending(
        &self,
        candidate_handle: String,
        expected_active: String,
        expected_candidate: String,
        operation_id: String,
        now: u64,
    ) -> DtoResult<CatalogAcceptanceOutcomeDto> {
        // Production expiry driver: an overdue candidate expires before it can
        // be accepted (PR24-003).
        self.expire_pending(now)?;
        self.gate.run_exclusive(|gate| {
            let prepared = gate.prepared.as_ref().ok_or_else(|| {
                catalog_error(
                    "provider_catalog_removal_not_pending",
                    ErrorCategoryDto::Conflict,
                    "no pending provider catalog removal candidate is prepared",
                )
            })?;
            if prepared.candidate_handle != candidate_handle {
                return Err(catalog_error(
                    "provider_catalog_removal_not_found",
                    ErrorCategoryDto::NotFound,
                    "the requested provider catalog removal candidate does not exist",
                ));
            }
            let active = gate
                .applied_revision
                .map_or_else(String::new, |revision| revision.to_string());
            if active != expected_active
                || prepared.catalog_revision_id.to_string() != expected_candidate
            {
                return Err(catalog_error(
                    "provider_catalog_revision_conflict",
                    ErrorCategoryDto::Conflict,
                    "the accepted catalog revision does not match the prepared candidate",
                ));
            }
            self.removal
                .accept_provider_catalog_removal(AcceptProviderCatalogRemovalInputDto {
                    candidate_handle: candidate_handle.clone(),
                    accepted_at: i64_time(now),
                    operation_id: operation_id.clone(),
                })?;
            self.catalog
                .accept_provider_catalog(AcceptProviderCatalogInputDto {
                    catalog_revision_id: prepared.catalog_revision_id,
                    candidate_handle: candidate_handle.clone(),
                    kind_descriptors: prepared.kind_descriptors.clone(),
                    profiles: prepared.profiles.clone(),
                    default_profile_id: prepared.default_profile_id.clone(),
                    accepted_at: i64_time(now),
                    operation_id,
                })?;
            let (built, admissions) = self.build_registry_from_candidate(
                &prepared.kind_descriptors,
                &prepared.profiles,
                prepared.catalog_revision_id,
            )?;
            self.activate_registry(built, admissions)?;
            let removed_profile_ids = prepared.removed_profile_ids.clone();
            let removed_kind_ids = prepared.removed_kind_ids.clone();
            let active_profile_ids = prepared
                .profiles
                .iter()
                .map(|profile| profile.profile.profile_id.clone())
                .collect::<Vec<_>>();
            let active_kind_ids = prepared
                .kind_descriptors
                .iter()
                .map(|kind| kind.descriptor.kind_id.clone())
                .collect::<Vec<_>>();
            let revision = prepared.catalog_revision_id;
            let default_profile_id = prepared.default_profile_id.clone();
            self.record_tombstones(
                &removed_profile_ids,
                &removed_kind_ids,
                &active_profile_ids,
                &active_kind_ids,
            );
            gate.readiness = CatalogReadiness::Ready;
            gate.applied_revision = Some(revision);
            gate.active_default_profile_id = Some(default_profile_id);
            gate.candidate_catalog_revision_id = None;
            gate.degraded_reason = None;
            gate.prepared = None;
            Ok(CatalogAcceptanceOutcomeDto {
                catalog_revision_id: revision,
                candidate_handle,
                readiness: CatalogReadiness::Ready,
                entry_count: self.registry.len(),
                removed_profile_ids,
                removed_kind_ids,
            })
        })
    }

    /// Rejects one pending removal candidate.
    ///
    /// Rejection leaves the active catalog unchanged and degrades the
    /// readiness to `Blocked` with `removal_candidate_rejected`.
    ///
    /// # Errors
    ///
    /// Returns a not-found or conflict error when the candidate handle does
    /// not match the prepared candidate, or a storage error when the
    /// rejection cannot be committed.
    pub fn reject_pending(
        &self,
        candidate_handle: String,
        operation_id: String,
        now: u64,
    ) -> DtoResult<()> {
        // Production expiry driver: an overdue candidate expires before it can
        // be rejected (PR24-003).
        self.expire_pending(now)?;
        self.gate.run_exclusive(|gate| {
            let prepared = gate.prepared.as_ref().ok_or_else(|| {
                catalog_error(
                    "provider_catalog_removal_not_pending",
                    ErrorCategoryDto::Conflict,
                    "no pending provider catalog removal candidate is prepared",
                )
            })?;
            if prepared.candidate_handle != candidate_handle {
                return Err(catalog_error(
                    "provider_catalog_removal_not_found",
                    ErrorCategoryDto::NotFound,
                    "the requested provider catalog removal candidate does not exist",
                ));
            }
            self.removal
                .reject_provider_catalog_removal(RejectProviderCatalogRemovalInputDto {
                    candidate_handle,
                    rejected_at: i64_time(now),
                    operation_id,
                })?;
            gate.readiness = CatalogReadiness::Blocked {
                reason: "removal_candidate_rejected".to_owned(),
            };
            gate.degraded_reason = Some("removal_candidate_rejected".to_owned());
            gate.candidate_catalog_revision_id = None;
            gate.prepared = None;
            Ok(())
        })
    }

    /// Expires overdue pending removal candidates and returns the number
    /// expired. Expiry leaves the active catalog unchanged and degrades the
    /// readiness to `Blocked` with `removal_candidate_expired`.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the expiry cannot be committed or the
    /// gate lock is poisoned.
    pub fn expire_pending(&self, now: u64) -> DtoResult<u64> {
        self.gate.run_exclusive(|gate| {
            // The durable pending state is the authority, not the in-memory
            // readiness: startup drives expiry before the gate has been
            // rebuilt, so an overdue candidate must expire there too
            // (PR24-003).
            let durable_pending = self
                .catalog
                .load_provider_catalog_status()
                .is_ok_and(|state| state.status == ProviderCatalogStatusDto::PendingRemoval);
            if !matches!(gate.readiness, CatalogReadiness::PendingRemoval { .. })
                && !durable_pending
            {
                return Ok(0);
            }
            let expired = self.removal.expire_provider_catalog_removal_candidate(
                ExpireProviderCatalogRemovalCandidateInputDto {
                    now: i64_time(now),
                    operation_id: format!("expire-{now}"),
                },
            )?;
            if expired > 0 {
                gate.readiness = CatalogReadiness::Blocked {
                    reason: "removal_candidate_expired".to_owned(),
                };
                gate.degraded_reason = Some("removal_candidate_expired".to_owned());
                gate.candidate_catalog_revision_id = None;
                gate.prepared = None;
            }
            Ok(expired)
        })
    }

    /// Resolves one provider admission under the gate.
    ///
    /// Admission requires an exact registry key match, an enabled profile, no
    /// tombstone, and `Ready` readiness. The returned DTO carries no private
    /// handle; the controller pairs the DTO with the private handle internally
    /// for later runtime wiring.
    ///
    /// # Errors
    ///
    /// Returns `catalog_not_ready` when the catalog is not ready,
    /// `provider_admission_not_found` when the key is not admitted,
    /// `provider_profile_unavailable` when the profile is disabled, or
    /// `provider_profile_tombstoned` when the profile is tombstoned.
    pub fn registry_lookup(&self, key: &PrivateRegistryKey) -> DtoResult<ProviderAdmissionDto> {
        self.gate.read(|state| {
            if !matches!(state.readiness, CatalogReadiness::Ready) {
                return Err(ErrorDto::unavailable(
                    "catalog_not_ready",
                    "the provider catalog is not ready for admission",
                ));
            }
            let admissions = self.admissions.lock().map_err(|_| {
                ErrorDto::unavailable(
                    "provider_admission_unavailable",
                    "the admission context lock is poisoned",
                )
            })?;
            let entry = admissions.get(key).ok_or_else(|| {
                catalog_error(
                    "provider_admission_not_found",
                    ErrorCategoryDto::NotFound,
                    "the requested provider profile is not admitted by the active catalog",
                )
            })?;
            if !entry.enabled {
                return Err(ErrorDto::unavailable(
                    "provider_profile_unavailable",
                    "the provider profile is disabled",
                ));
            }
            if self.is_tombstoned(&entry.dto.profile_id) {
                return Err(ErrorDto::unavailable(
                    "provider_profile_tombstoned",
                    "the provider profile is tombstoned",
                ));
            }
            let dto = entry.dto.clone();
            drop(admissions);
            Ok(dto)
        })?
    }

    /// Loads the active catalog material, treating a not-active catalog as
    /// the empty active material.
    fn load_active_material(&self) -> DtoResult<Option<ProviderCatalogMaterialDto>> {
        match self.catalog.load_provider_catalog_material() {
            Ok(material) => Ok(Some(material)),
            Err(error) if error.code() == "provider_catalog_not_active" => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Builds the candidate kind descriptors and profiles from the source.
    fn build_candidate_records(
        &self,
        source: &CatalogSourceInputDto,
        resolved: &intention_config::ResolvedConfigDto,
    ) -> DtoResult<(
        Vec<ProviderKindDescriptorCandidateDto>,
        Vec<ProviderProfileCandidateDto>,
    )> {
        let mut kind_descriptors = Vec::new();
        let mut profiles = Vec::new();
        let mut seen_kinds = HashSet::new();
        for (index, declaration) in source.providers.iter().enumerate() {
            let descriptor = derive_kind_descriptor(declaration)?;
            validate_capability_subset(
                &declaration.declared_model_capability_subset,
                &descriptor.model_capability_envelope,
            )?;
            let driver_contract = ProviderDriverContractRevisionDto {
                driver_family: declaration.kind.clone(),
                major: 1,
                minor: 1,
            };
            let factory = self
                .factories
                .iter()
                .find(|factory| factory.kind() == declaration.kind)
                .ok_or_else(|| {
                    ErrorDto::unavailable(
                        "provider_driver_unavailable",
                        "no registered driver factory serves the provider kind",
                    )
                })?;
            if !factory.supports_contract(&driver_contract) {
                return Err(ErrorDto::validation(
                    "provider_driver_contract_incompatible",
                    "the registered driver factory does not support the profile driver contract",
                ));
            }
            if seen_kinds.insert(descriptor.kind_id.clone()) {
                let kind_revision_id = kind_descriptor_revision_id(&descriptor);
                kind_descriptors.push(ProviderKindDescriptorCandidateDto {
                    descriptor_revision_id: kind_revision_id.clone(),
                    descriptor: descriptor.clone(),
                });
            }
            let profile_id = if index == 0 {
                DEFAULT_PROFILE_ID.to_owned()
            } else {
                format!("profile-{index}")
            };
            let endpoint = declaration
                .endpoint
                .clone()
                .unwrap_or_else(|| default_endpoint(&declaration.kind));
            let revision_id = profile_revision_id(
                &declaration.kind,
                &declaration.model,
                &endpoint,
                &declaration.declared_model_capability_subset,
            );
            let profile = derive_profile(
                declaration,
                &driver_contract,
                profile_id,
                revision_id,
                endpoint,
                kind_descriptor_revision_id(&descriptor),
            )?;
            profiles.push(candidate_profile(profile, declaration, resolved));
        }
        Ok((kind_descriptors, profiles))
    }

    /// Persists the prepared candidate records through the append path.
    fn persist_prepared_candidate(
        &self,
        kind_descriptors: &[ProviderKindDescriptorCandidateDto],
        profiles: &[ProviderProfileCandidateDto],
        catalog_revision_id: u64,
        operation_id: &str,
        now: u64,
    ) -> DtoResult<()> {
        for kind in kind_descriptors {
            self.catalog.append_provider_kind_descriptor_revision(
                AppendProviderKindDescriptorRevisionInputDto {
                    descriptor_revision_id: kind.descriptor_revision_id.clone(),
                    descriptor: kind.descriptor.clone(),
                    catalog_revision_id,
                    accepted_at: i64_time(now),
                    operation_id: operation_id.to_owned(),
                },
            )?;
        }
        for profile in profiles {
            self.catalog.append_provider_profile_revision(
                AppendProviderProfileRevisionInputDto {
                    profile: profile.clone(),
                    catalog_revision_id,
                    accepted_at: i64_time(now),
                    operation_id: operation_id.to_owned(),
                },
            )?;
        }
        Ok(())
    }

    /// Builds the private registry and admission context from candidate
    /// records (all-or-nothing; no partial activation).
    fn build_registry_from_candidate(
        &self,
        kind_descriptors: &[ProviderKindDescriptorCandidateDto],
        profiles: &[ProviderProfileCandidateDto],
        catalog_revision_id: u64,
    ) -> DtoResult<(BuiltRegistryEntries, AdmissionContext)> {
        let material = ProviderCatalogMaterialDto {
            catalog_revision_id,
            default_profile_id: Some(DEFAULT_PROFILE_ID.to_owned()),
            kind_descriptors: kind_descriptors.to_vec(),
            profiles: profiles.to_vec(),
        };
        self.build_registry_from_material(&material)
    }

    /// Builds the private registry and admission context from accepted
    /// material (all-or-nothing; no partial activation).
    fn build_registry_from_material(
        &self,
        material: &ProviderCatalogMaterialDto,
    ) -> DtoResult<(BuiltRegistryEntries, AdmissionContext)> {
        let mut materials = Vec::new();
        let mut admissions = HashMap::new();
        for candidate in &material.profiles {
            let profile = &candidate.profile;
            let _descriptor = material
                .kind_descriptors
                .iter()
                .find(|kind| kind.descriptor_revision_id == profile.kind_descriptor_revision_id)
                .ok_or_else(|| {
                    ErrorDto::validation(
                        "catalog_corrupt",
                        "a profile references an unknown kind descriptor revision",
                    )
                })?;
            let key = PrivateRegistryKey {
                profile_id: profile.profile_id.clone(),
                profile_revision_id: profile.revision_id.clone(),
                kind_descriptor_revision_id: profile.kind_descriptor_revision_id.clone(),
                driver_contract: profile.driver_contract_revision.clone(),
            };
            let selection = selection_from_candidate(candidate, material.catalog_revision_id)?;
            let private_material = PrivateProviderProfileMaterial {
                profile: profile.clone(),
                selection,
                endpoint: profile.endpoint.clone(),
                private_credential_reference: private_credential_reference(&profile.revision_id),
            };
            let dto = admission_dto(&key, &private_material.selection);
            materials.push((key.clone(), private_material));
            admissions.insert(
                key,
                AdmissionEntry {
                    dto,
                    enabled: candidate.enabled,
                },
            );
        }
        let built = PrivateRegistry::build_all(&self.factories, materials)?;
        Ok((built, admissions))
    }

    /// Atomically installs the built registry and its admission context.
    fn activate_registry(
        &self,
        built: BuiltRegistryEntries,
        admissions: AdmissionContext,
    ) -> DtoResult<()> {
        self.registry.activate(built)?;
        let mut current = self.admissions.lock().map_err(|_| {
            ErrorDto::unavailable(
                "provider_admission_unavailable",
                "the admission context lock is poisoned",
            )
        })?;
        *current = admissions;
        drop(current);
        Ok(())
    }

    /// Records tombstoned profile and kind ids on the controller side and
    /// clears any tombstone for identifiers the accepted material
    /// reintroduces.
    ///
    /// Durable tombstones are append-only removal-history events; admission
    /// authority is the current active membership. An identifier removed by
    /// an earlier catalog and present again in an accepted catalog is
    /// therefore admitted again (PR24-017).
    fn record_tombstones(
        &self,
        removed_profile_ids: &[String],
        removed_kind_ids: &[String],
        active_profile_ids: &[String],
        active_kind_ids: &[String],
    ) {
        if let Ok(mut tombstones) = self.tombstones.lock() {
            tombstones
                .profile_ids
                .extend(removed_profile_ids.iter().cloned());
            tombstones.kind_ids.extend(removed_kind_ids.iter().cloned());
            for reintroduced in active_profile_ids {
                tombstones.profile_ids.remove(reintroduced);
            }
            for reintroduced in active_kind_ids {
                tombstones.kind_ids.remove(reintroduced);
            }
        }
    }

    /// Returns whether one profile id is tombstoned on the controller side.
    fn is_tombstoned(&self, profile_id: &str) -> bool {
        self.tombstones
            .lock()
            .is_ok_and(|tombstones| tombstones.profile_ids.contains(profile_id))
    }

    /// Returns the current startup outcome from the gate state.
    fn startup_outcome(&self) -> DtoResult<CatalogStartupOutcomeDto> {
        self.gate.read(|state| CatalogStartupOutcomeDto {
            readiness: state.readiness.clone(),
            active_catalog_revision_id: state.applied_revision,
            entry_count: self.registry.len(),
            degraded_reason: state.degraded_reason.clone(),
        })
    }

    /// Degrades the controller to a typed blocked readiness.
    fn blocked(&self, reason: &str) -> DtoResult<CatalogStartupOutcomeDto> {
        self.gate.run_exclusive(|gate| {
            gate.readiness = CatalogReadiness::Blocked {
                reason: reason.to_owned(),
            };
            gate.degraded_reason = Some(reason.to_owned());
            gate.prepared = None;
            Ok(())
        })?;
        self.startup_outcome()
    }

    /// Leaves the controller in activation-recovery-required readiness.
    fn recovery_required(
        &self,
        accepted: u64,
        reason: &str,
    ) -> DtoResult<CatalogStartupOutcomeDto> {
        self.gate.run_exclusive(|gate| {
            gate.readiness = CatalogReadiness::ActivationRecoveryRequired {
                accepted_revision: accepted.to_string(),
            };
            gate.degraded_reason = Some(reason.to_owned());
            gate.prepared = None;
            Ok(())
        })?;
        self.startup_outcome()
    }
}

/// Validates the catalog candidate limits before any record construction.
fn validate_candidate_limits(source: &CatalogSourceInputDto) -> DtoResult<()> {
    if source.raw_config_size_bytes > u64::try_from(MAX_CANDIDATE_RAW_BYTES).unwrap_or(u64::MAX) {
        return Err(ErrorDto::validation(
            "candidate_too_large",
            "candidate raw configuration exceeds the 512 KiB reload limit",
        ));
    }
    if source.providers.len() > MAX_PROVIDER_PROFILES {
        return Err(ErrorDto::validation(
            "provider_catalog_profile_limit_exceeded",
            "catalog candidate exceeds the provider profile limit",
        ));
    }
    let distinct_kinds = source
        .providers
        .iter()
        .map(|declaration| declaration.kind.as_str())
        .collect::<HashSet<_>>();
    if distinct_kinds.len() > MAX_PROVIDER_KINDS {
        return Err(ErrorDto::validation(
            "provider_catalog_kind_limit_exceeded",
            "catalog candidate exceeds the provider kind limit",
        ));
    }
    for declaration in &source.providers {
        if declaration.kind == "openai" {
            return Err(catalog_error(
                "legacy_config_cannot_represent_active_catalog",
                ErrorCategoryDto::Policy,
                "the openai provider alias cannot represent an active catalog in this slice",
            ));
        }
        validate_provider_kind_id(&declaration.kind).map_err(domain_error)?;
    }
    Ok(())
}

/// Bounds the candidate validation issues at the catalog issue limit.
fn bounded_issues(candidate: &ConfigCandidateDto) -> (Vec<CandidateIssueDto>, bool, u32) {
    let total = candidate.validation().total_issue_count();
    let mut issues = candidate.validation().issues().to_vec();
    let truncated = candidate.validation().truncated() || issues.len() > MAX_CATALOG_ISSUES;
    issues.truncate(MAX_CATALOG_ISSUES);
    (issues, truncated, total)
}

/// Validates that no removed kind retains a dependent profile in the candidate.
fn validate_kind_removals(
    removed_kind_ids: &[String],
    candidate_profiles: &[ProviderProfileCandidateDto],
) -> DtoResult<()> {
    for kind_id in removed_kind_ids {
        let dependents = candidate_profiles
            .iter()
            .filter(|profile| profile.profile.provider_kind_id == *kind_id)
            .map(|profile| profile.profile.clone())
            .collect::<Vec<_>>();
        validate_provider_kind_removal(kind_id, &dependents).map_err(domain_error)?;
    }
    Ok(())
}

/// Derives the deterministic kind descriptor revision for one declaration.
fn derive_kind_descriptor(
    declaration: &CatalogProviderDeclarationDto,
) -> DtoResult<ProviderKindDescriptorRevisionV1> {
    let kind_id = declaration.kind.clone();
    validate_provider_kind_id(&kind_id).map_err(domain_error)?;
    let descriptor = ProviderKindDescriptorRevisionV1 {
        kind_id,
        descriptor_family: format!("{}-descriptor-v1", declaration.kind),
        ordered_protocol_part_revisions: vec!["protocol-parts-v1".to_owned()],
        endpoint_policy: "https-only".to_owned(),
        credential_transport_contract: "bearer".to_owned(),
        model_capability_envelope: standard_capability_envelope(),
        driver_contract_family: declaration.kind.clone(),
    };
    descriptor.validate().map_err(domain_error)?;
    Ok(descriptor)
}

/// Derives the deterministic profile revision for one declaration.
fn derive_profile(
    declaration: &CatalogProviderDeclarationDto,
    driver_contract: &ProviderDriverContractRevisionDto,
    profile_id: String,
    revision_id: String,
    endpoint: String,
    kind_descriptor_revision_id: String,
) -> DtoResult<ProviderProfileRevisionV1> {
    let profile = ProviderProfileRevisionV1 {
        profile_id,
        revision_id,
        provider_kind_id: declaration.kind.clone(),
        model_id: declaration.model.clone(),
        endpoint,
        credential_transport_mode: CredentialTransportMode::Bearer,
        safe_header_name: None,
        capability_taxonomy_revision: MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
        reasoning_compatibility_id: None,
        kind_descriptor_revision_id,
        driver_contract_revision: driver_contract.clone(),
    };
    profile.validate().map_err(domain_error)?;
    Ok(profile)
}

/// Builds the storage candidate profile DTO for one derived profile.
fn candidate_profile(
    profile: ProviderProfileRevisionV1,
    declaration: &CatalogProviderDeclarationDto,
    resolved: &intention_config::ResolvedConfigDto,
) -> ProviderProfileCandidateDto {
    ProviderProfileCandidateDto {
        profile,
        declared_model_capability_subset: declaration.declared_model_capability_subset.clone(),
        resolved_reasoning_policy: RESOLVED_REASONING_POLICY.to_owned(),
        effective_execution_policy: execution_policy(resolved),
        effective_loopback_policy_or_not_applicable: LOOPBACK_POLICY_NOT_APPLICABLE.to_owned(),
        display_name: None,
        enabled: declaration.enabled,
        credential_configured: false,
        readiness: if declaration.enabled {
            ProviderReadinessDto::Ready
        } else {
            ProviderReadinessDto::Disabled
        },
    }
}

/// Reconstructs the deterministic selection for one accepted candidate profile.
fn selection_from_candidate(
    candidate: &ProviderProfileCandidateDto,
    catalog_revision_id: u64,
) -> DtoResult<ProviderSelectionV1> {
    let profile = &candidate.profile;
    let selection = ProviderSelectionV1 {
        selection_canonicalization_version: PROVIDER_SELECTION_CANONICALIZATION_VERSION.to_owned(),
        profile_id: profile.profile_id.clone(),
        provider_profile_revision_id: profile.revision_id.clone(),
        kind_id: profile.provider_kind_id.clone(),
        kind_descriptor_revision_id: profile.kind_descriptor_revision_id.clone(),
        model_id: profile.model_id.clone(),
        normalized_effective_endpoint: profile.endpoint.clone(),
        credential_transport_mode: profile.credential_transport_mode,
        credential_transport_safe_header_name: profile.safe_header_name.clone(),
        declared_model_capability_subset: candidate.declared_model_capability_subset.clone(),
        resolved_reasoning_policy: candidate.resolved_reasoning_policy.clone(),
        effective_execution_policy: candidate.effective_execution_policy.clone(),
        effective_loopback_policy_or_not_applicable: candidate
            .effective_loopback_policy_or_not_applicable
            .clone(),
        provider_driver_contract_revision: driver_contract_name(&profile.driver_contract_revision),
        selection_source: Some(format!("catalog-rev-{catalog_revision_id}")),
    };
    selection.validate().map_err(domain_error)?;
    Ok(selection)
}

/// Builds the credential-free admission DTO for one registry key.
fn admission_dto(
    key: &PrivateRegistryKey,
    selection: &ProviderSelectionV1,
) -> ProviderAdmissionDto {
    ProviderAdmissionDto {
        profile_id: key.profile_id.clone(),
        profile_revision_id: key.profile_revision_id.clone(),
        descriptor_revision_id: key.kind_descriptor_revision_id.clone(),
        driver_contract: key.driver_contract.clone(),
        selection_digest: selection_digest(selection),
    }
}

/// Computes the controller-owned deterministic credential-free selection digest.
fn selection_digest(selection: &ProviderSelectionV1) -> String {
    let canonical = format!(
        "ir-selection-v1|profile={}|revision={}|kind={}|kind_descriptor_revision={}|model={}|endpoint={}|transport={}|header={}|subset={}|reasoning={}|execution={}|loopback={}|contract={}|source={}",
        selection.profile_id,
        selection.provider_profile_revision_id,
        selection.kind_id,
        selection.kind_descriptor_revision_id,
        selection.model_id,
        selection.normalized_effective_endpoint,
        transport_name(selection.credential_transport_mode),
        selection
            .credential_transport_safe_header_name
            .as_deref()
            .unwrap_or(""),
        selection.declared_model_capability_subset.join(","),
        selection.resolved_reasoning_policy,
        selection.effective_execution_policy,
        selection.effective_loopback_policy_or_not_applicable,
        selection.provider_driver_contract_revision,
        selection.selection_source.as_deref().unwrap_or(""),
    );
    digest_hex(Digest256::sha256(canonical.as_bytes()))
}

/// Validates that the declared capability subset is inside the descriptor
/// envelope.
fn validate_capability_subset(
    declared: &[String],
    envelope: &ModelCapabilitySetV1,
) -> DtoResult<()> {
    let selection = ModelCapabilitySelectionV1 {
        taxonomy_version: MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
        descriptor_capability_envelope: envelope.clone(),
        selected_capabilities: declared.to_vec(),
    };
    selection.validate().map_err(domain_error)
}

/// The closed Slice 2 model capability envelope.
fn standard_capability_envelope() -> ModelCapabilitySetV1 {
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

/// The deterministic execution policy string for one resolved configuration.
fn execution_policy(resolved: &intention_config::ResolvedConfigDto) -> String {
    format!(
        "execution-timeout-{}-attempts-{}",
        resolved.provider_execution().attempt_timeout_seconds(),
        resolved.provider_execution().max_attempts(),
    )
}

/// The deterministic driver contract name for one contract revision.
fn driver_contract_name(contract: &ProviderDriverContractRevisionDto) -> String {
    format!(
        "{}-{}.{}",
        contract.driver_family, contract.major, contract.minor
    )
}

/// The deterministic profile revision identity for one declaration.
fn profile_revision_id(kind: &str, model: &str, endpoint: &str, subset: &[String]) -> String {
    let canonical = format!(
        "ir-profile-v1|kind={kind}|model={model}|endpoint={endpoint}|subset={}",
        subset.join(",")
    );
    format!(
        "profile-{}",
        &digest_hex(Digest256::sha256(canonical.as_bytes()))[..16]
    )
}

/// The deterministic kind descriptor revision identity for one descriptor.
fn kind_descriptor_revision_id(descriptor: &ProviderKindDescriptorRevisionV1) -> String {
    let canonical = format!(
        "ir-kind-v1|kind={}|family={}|endpoint_policy={}|transport_contract={}|driver_family={}",
        descriptor.kind_id,
        descriptor.descriptor_family,
        descriptor.endpoint_policy,
        descriptor.credential_transport_contract,
        descriptor.driver_contract_family,
    );
    format!(
        "kind-{}",
        &digest_hex(Digest256::sha256(canonical.as_bytes()))[..16]
    )
}

/// Formats one digest as sixty-four lowercase hexadecimal characters.
fn digest_hex(digest: Digest256) -> String {
    digest
        .bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The deterministic placeholder endpoint for a kind without a configured
/// endpoint. The composition root or provider descriptor resolves real
/// defaults in a later slice.
fn default_endpoint(kind: &str) -> String {
    format!("https://{kind}.api.example.invalid/v1")
}

/// The stable transport-mode name.
const fn transport_name(mode: CredentialTransportMode) -> &'static str {
    match mode {
        CredentialTransportMode::Bearer => "bearer",
        CredentialTransportMode::SafeHeader => "safe_header",
    }
}

/// The safe opaque removal candidate JSON (credential-free).
fn removal_candidate_json(
    catalog_revision_id: u64,
    removed_profile_ids: &[String],
    removed_kind_ids: &[String],
) -> String {
    let profiles = removed_profile_ids
        .iter()
        .map(|id| json_string(id))
        .collect::<Vec<_>>()
        .join(",");
    let kinds = removed_kind_ids
        .iter()
        .map(|id| json_string(id))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"catalog_revision_id\":{catalog_revision_id},\"removed_profiles\":[{profiles}],\"removed_kinds\":[{kinds}],\"default_profile_id\":\"default\"}}"
    )
}

/// Escapes one safe identifier for inclusion in opaque JSON.
fn json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            other => escaped.push(other),
        }
    }
    format!("\"{escaped}\"")
}

/// Converts one whole-second Unix time to the storage `i64` representation.
fn i64_time(now: u64) -> i64 {
    i64::try_from(now).unwrap_or(i64::MAX)
}

/// Maps one domain canonical error to a typed validation error.
fn domain_error(error: CanonicalError) -> ErrorDto {
    ErrorDto::new(
        error.code(),
        ErrorCategoryDto::Validation,
        "catalog candidate record failed domain validation",
        ErrorRetryDto::Never,
        None,
    )
    .unwrap_or_else(|_| {
        ErrorDto::validation(
            "catalog_validation_failed",
            "catalog candidate validation failed",
        )
    })
}

/// Builds one typed catalog error with a static safe message.
fn catalog_error(
    code: &'static str,
    category: ErrorCategoryDto,
    message: &'static str,
) -> ErrorDto {
    ErrorDto::new(code, category, message, ErrorRetryDto::Never, None)
        .unwrap_or_else(|_| ErrorDto::validation(code, message))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;
    use intention_domain::provider_catalog::validate_provider_kind_revision_immutability;

    #[test]
    fn kind_removal_with_dependents_is_rejected() {
        let dependents = vec![ProviderProfileCandidateDto {
            profile: ProviderProfileRevisionV1 {
                profile_id: "default".to_owned(),
                revision_id: "rev-1".to_owned(),
                provider_kind_id: "responses".to_owned(),
                model_id: "model-1".to_owned(),
                endpoint: "https://api.example.invalid/v1".to_owned(),
                credential_transport_mode: CredentialTransportMode::Bearer,
                safe_header_name: None,
                capability_taxonomy_revision: MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
                reasoning_compatibility_id: None,
                kind_descriptor_revision_id: "kind-1".to_owned(),
                driver_contract_revision: ProviderDriverContractRevisionDto {
                    driver_family: "responses".to_owned(),
                    major: 1,
                    minor: 0,
                },
            },
            declared_model_capability_subset: vec!["text_input".to_owned()],
            resolved_reasoning_policy: RESOLVED_REASONING_POLICY.to_owned(),
            effective_execution_policy: "execution-timeout-30-attempts-2".to_owned(),
            effective_loopback_policy_or_not_applicable: LOOPBACK_POLICY_NOT_APPLICABLE.to_owned(),
            display_name: None,
            enabled: true,
            credential_configured: false,
            readiness: ProviderReadinessDto::Ready,
        }];
        assert_eq!(
            validate_kind_removals(&["responses".to_owned()], &dependents)
                .expect_err("removal with dependents is rejected")
                .code(),
            "provider_kind_has_dependents"
        );
        assert!(validate_kind_removals(&["other-kind".to_owned()], &dependents).is_ok());
    }

    #[test]
    fn kind_immutability_mismatch_is_rejected() {
        let previous = ProviderKindDescriptorRevisionV1 {
            kind_id: "responses".to_owned(),
            descriptor_family: "responses-descriptor".to_owned(),
            ordered_protocol_part_revisions: vec!["parts-v1".to_owned()],
            endpoint_policy: "https-only".to_owned(),
            credential_transport_contract: "bearer-or-safe-header".to_owned(),
            model_capability_envelope: standard_capability_envelope(),
            driver_contract_family: "responses".to_owned(),
        };
        let next = derive_kind_descriptor(&CatalogProviderDeclarationDto {
            kind: "responses".to_owned(),
            model: "model-1".to_owned(),
            endpoint: None,
            declared_model_capability_subset: vec!["text_input".to_owned()],
            enabled: true,
        })
        .expect("derived descriptor is valid");
        assert_eq!(
            validate_provider_kind_revision_immutability(&previous, &next)
                .expect_err("changed composition is rejected")
                .code(),
            "provider_kind_immutable_mismatch"
        );
        assert!(validate_provider_kind_revision_immutability(&next, &next).is_ok());
    }

    #[test]
    fn capability_subset_outside_the_envelope_is_rejected() {
        let envelope = standard_capability_envelope();
        assert!(
            validate_capability_subset(
                &["text_input".to_owned(), "text_streaming".to_owned()],
                &envelope,
            )
            .is_ok()
        );
        assert_eq!(
            validate_capability_subset(&["structured_output".to_owned()], &envelope)
                .expect_err("structured output is unsupported")
                .code(),
            "provider_profile_revision_invalid"
        );
    }

    #[test]
    fn selection_digest_is_credential_free_and_deterministic() {
        let selection = selection_from_candidate(
            &ProviderProfileCandidateDto {
                profile: ProviderProfileRevisionV1 {
                    profile_id: "default".to_owned(),
                    revision_id: "profile-0123456789abcdef".to_owned(),
                    provider_kind_id: "responses".to_owned(),
                    model_id: "model-1".to_owned(),
                    endpoint: "https://api.example.invalid/v1".to_owned(),
                    credential_transport_mode: CredentialTransportMode::Bearer,
                    safe_header_name: None,
                    capability_taxonomy_revision: MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
                    reasoning_compatibility_id: None,
                    kind_descriptor_revision_id: "kind-0123456789abcdef".to_owned(),
                    driver_contract_revision: ProviderDriverContractRevisionDto {
                        driver_family: "responses".to_owned(),
                        major: 1,
                        minor: 0,
                    },
                },
                declared_model_capability_subset: vec!["text_input".to_owned()],
                resolved_reasoning_policy: RESOLVED_REASONING_POLICY.to_owned(),
                effective_execution_policy: "execution-timeout-30-attempts-2".to_owned(),
                effective_loopback_policy_or_not_applicable: LOOPBACK_POLICY_NOT_APPLICABLE
                    .to_owned(),
                display_name: None,
                enabled: true,
                credential_configured: false,
                readiness: ProviderReadinessDto::Ready,
            },
            1,
        )
        .expect("selection is valid");
        let digest = selection_digest(&selection);
        assert_eq!(digest.len(), 64);
        assert_eq!(selection_digest(&selection), digest);
        assert!(!digest.contains("sk-test"));
    }
}
