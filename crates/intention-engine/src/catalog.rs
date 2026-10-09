//! Provider catalog preparation, durable acceptance, activation, and recovery.
//!
//! One validated, credential-free configuration document becomes a candidate
//! catalog revision through [`prepare_catalog_candidate`]: code-owned
//! first-party kind descriptors and declared user kinds are resolved, every
//! profile's declared exact-model capability subset is intersected with its
//! kind envelope, and the candidate is classified against the accepted
//! revision. Durable acceptance is a separate commit
//! ([`accept_catalog_revision`]); activation is the commit that follows a
//! successful private-registry swap ([`mark_catalog_activated`]).
//!
//! Catalog identities are derived from catalog meaning, never drawn at random:
//! the derived identity of an equal catalog is stable, so a semantic-equal
//! catalog writes no new revision and a crash between acceptance and activation
//! can be recovered only by the exact accepted revision. The reduction is a
//! local identity mechanism, never a security boundary.
//!
//! No external action happens inside any function of this module, and no
//! credential, raw document, or private resource crosses this boundary.

use std::collections::{BTreeMap, BTreeSet};

use intention_config::catalog::{
    CatalogDeclaredCapabilitySubsetDto, CatalogDeclaredReasoningCapabilityDto, CatalogDocumentDto,
    CatalogUserKindDto,
};
use intention_proto::provider::{
    CatalogRevisionId, ContextPreservationCapabilityDto, CredentialTransportDto,
    CredentialTransportModeDto, LoopbackPolicyDto, ModelCapabilitySetV1, ModelInputKindDto,
    ProviderCapabilityAvailabilityDto, ProviderCatalogCandidateHandleDto,
    ProviderKindDescriptorRevisionId, ProviderKindDescriptorRevisionV1, ProviderKindId,
    ProviderProfileId, ProviderProfilePolicyDto, ProviderProfileRevisionId,
    ProviderProfileRevisionV1, ReasoningCapabilityDto, ReasoningFragmentCategoryDto,
    ReasoningHistoryTransferDto, ResolvedReasoningPolicyDto, ToolExchangeCapabilityDto,
};
use intention_proto::{DtoResult, ErrorDto, TimestampDto};
use intention_providers::{
    driver_contract, first_party_kind_descriptors, validate_capability_subset, validate_user_kind,
};
use intention_storage::{
    ProviderCatalogRevisionDto, ProviderCatalogStateDto, ProviderProfilePolicyEntryDto,
    StorageRepositoryDto,
};

/// The credential-free candidate document plus the accepted revision it replaces.
///
/// The document is the config lane's validated, credential-free
/// [`CatalogDocumentDto`]; the accepted revision is the catalog the daemon holds
/// in durable state, if any. Both inputs are already-validated values, so the
/// conversion is a plain carrier: every semantic check happens in
/// [`prepare_catalog_candidate`].
#[derive(Clone, Debug)]
pub struct CatalogCandidateInputDto {
    document: CatalogDocumentDto,
    active: Option<ProviderCatalogRevisionDto>,
}

impl CatalogCandidateInputDto {
    /// Builds the engine catalog-candidate input from config parts.
    ///
    /// This is the documented conversion for composition: the safe half of
    /// `intention_config::catalog::CatalogCandidate::into_parts_for_catalog`
    /// (the [`CatalogDocumentDto`]) plus the currently accepted revision loaded
    /// through `StorageRepositoryDto::load_catalog_revision`, when one exists.
    /// The credential material never enters this value, and every semantic
    /// check happens in [`prepare_catalog_candidate`].
    #[must_use]
    pub const fn new(
        document: CatalogDocumentDto,
        active: Option<ProviderCatalogRevisionDto>,
    ) -> Self {
        Self { document, active }
    }

    /// Returns the validated, credential-free candidate document.
    #[must_use]
    pub const fn document(&self) -> &CatalogDocumentDto {
        &self.document
    }

    /// Returns the accepted revision the candidate replaces, when one exists.
    #[must_use]
    pub const fn active(&self) -> Option<&ProviderCatalogRevisionDto> {
        self.active.as_ref()
    }
}

/// How a prepared candidate compares to the accepted catalog revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogChangeKindDto {
    /// The candidate carries the same catalog meaning as the accepted revision.
    Unchanged,
    /// Only non-revision-affecting profile policy differs.
    FreshRunsOnly,
    /// Kind, profile, or revision-affecting meaning differs.
    CatalogAffecting,
}

/// One member the candidate omits from the accepted catalog revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogRemovalEntryDto {
    /// The candidate omits one profile the accepted revision declared.
    Profile {
        /// The removed profile identity.
        profile_id: ProviderProfileId,
    },
    /// The candidate omits one user kind the accepted revision declared.
    Kind {
        /// The removed kind identity.
        kind_id: ProviderKindId,
    },
}

impl CatalogRemovalEntryDto {
    /// Returns whether this entry removes one profile.
    #[must_use]
    pub const fn profile_id(&self) -> Option<&ProviderProfileId> {
        match self {
            Self::Profile { profile_id } => Some(profile_id),
            Self::Kind { .. } => None,
        }
    }

    /// Returns whether this entry removes one kind.
    #[must_use]
    pub const fn kind_id(&self) -> Option<&ProviderKindId> {
        match self {
            Self::Kind { kind_id } => Some(kind_id),
            Self::Profile { .. } => None,
        }
    }
}

/// One fully validated catalog candidate ready for durable acceptance.
///
/// The preparation carries the closed change classification, the removal impact
/// against the accepted revision, and every resolved credential-free member of
/// the candidate revision. It owns no external resource and performs no durable
/// write.
#[derive(Clone, Debug)]
pub struct CatalogPreparationDto {
    candidate_revision_id: CatalogRevisionId,
    expected_active_revision_id: Option<CatalogRevisionId>,
    change: CatalogChangeKindDto,
    removals: Vec<CatalogRemovalEntryDto>,
    default_profile_id: Option<ProviderProfileId>,
    kind_descriptors: Vec<ProviderKindDescriptorRevisionV1>,
    profile_revisions: Vec<ProviderProfileRevisionV1>,
    profile_policies: Vec<ProviderProfilePolicyEntryDto>,
}

impl CatalogPreparationDto {
    /// Returns the candidate's derived catalog revision identity.
    #[must_use]
    pub const fn candidate_revision_id(&self) -> CatalogRevisionId {
        self.candidate_revision_id
    }

    /// Returns the accepted revision the candidate was prepared against.
    #[must_use]
    pub const fn expected_active_revision_id(&self) -> Option<CatalogRevisionId> {
        self.expected_active_revision_id
    }

    /// Returns the closed change classification of the candidate.
    #[must_use]
    pub const fn change(&self) -> CatalogChangeKindDto {
        self.change
    }

    /// Returns the accepted members the candidate omits, sorted by identity.
    #[must_use]
    pub fn removals(&self) -> &[CatalogRemovalEntryDto] {
        &self.removals
    }

    /// Returns whether the candidate omits at least one accepted member.
    #[must_use]
    pub const fn is_removal_candidate(&self) -> bool {
        !self.removals.is_empty()
    }

    /// Returns the candidate's global default profile, when it declares one.
    #[must_use]
    pub const fn default_profile_id(&self) -> Option<&ProviderProfileId> {
        self.default_profile_id.as_ref()
    }

    /// Returns the candidate's resolved kind descriptors, ordered by kind id.
    #[must_use]
    pub fn kind_descriptors(&self) -> &[ProviderKindDescriptorRevisionV1] {
        &self.kind_descriptors
    }

    /// Returns the candidate's resolved profile revisions, ordered by profile id.
    #[must_use]
    pub fn profile_revisions(&self) -> &[ProviderProfileRevisionV1] {
        &self.profile_revisions
    }

    /// Returns the candidate's profile policies, ordered by profile id.
    #[must_use]
    pub fn profile_policies(&self) -> &[ProviderProfilePolicyEntryDto] {
        &self.profile_policies
    }

    /// Returns the pending-candidate handle for accept/reject commands.
    ///
    /// A handle exists only when an accepted revision exists: a first catalog
    /// has no removal candidate, so it never needs one, and the handle contract
    /// requires two distinct revision identities.
    #[must_use]
    pub fn candidate_handle(&self) -> Option<ProviderCatalogCandidateHandleDto> {
        let expected = self.expected_active_revision_id?;
        ProviderCatalogCandidateHandleDto::new(self.candidate_revision_id, expected).ok()
    }

    /// Returns whether the candidate carries the accepted revision's meaning.
    #[must_use]
    pub fn matches_revision(&self, revision: &ProviderCatalogRevisionDto) -> bool {
        revision.default_profile_id() == self.default_profile_id.as_ref()
            && revision.profile_policies() == self.profile_policies.as_slice()
            && kind_meaning_equals(revision.kind_descriptors(), &self.kind_descriptors)
            && profile_meaning_equals(revision.profile_revisions(), &self.profile_revisions)
    }

    /// Builds the durable catalog revision of this preparation.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the prepared members cannot form a
    /// coherent catalog revision, which preparation already rejected.
    pub fn revision_at(&self, captured_at: TimestampDto) -> DtoResult<ProviderCatalogRevisionDto> {
        ProviderCatalogRevisionDto::new(
            self.candidate_revision_id,
            self.default_profile_id.clone(),
            captured_at,
            self.kind_descriptors.clone(),
            self.profile_revisions.clone(),
            self.profile_policies.clone(),
        )
    }
}

/// Prepares one credential-free catalog candidate for durable acceptance.
///
/// Validation resolves the candidate's kind descriptors (code-owned first-party
/// descriptors plus declared user-kind compositions), intersects every
/// profile's declared exact-model capability subset with its kind envelope,
/// enforces endpoint and credential-transport policy, verifies kind
/// immutability against the accepted revision, and computes the closed change
/// classification plus the removal impact.
///
/// # Errors
///
/// Returns a typed validation error when a profile references a kind the
/// candidate does not declare (`provider_kind_has_dependents`), when an already
/// accepted kind identity changes its declaration
/// (`provider_kind_immutable_mismatch`), when a declared capability set,
/// endpoint, credential transport, or default profile is not representable, or
/// when a code-owned descriptor or user-kind composition is not valid.
pub fn prepare_catalog_candidate(
    input: &CatalogCandidateInputDto,
) -> DtoResult<CatalogPreparationDto> {
    let document = input.document();
    let resolved_kinds = candidate_kind_descriptors(document)?;
    let kind_descriptors = resolved_kinds.values().cloned().collect::<Vec<_>>();
    let default_profile_id = document.default_profile().cloned();
    let profile_revisions = candidate_profile_revisions(document, &resolved_kinds)?;
    let profile_policies = candidate_profile_policies(document)?;
    if let Some(default) = default_profile_id.as_ref()
        && !profile_revisions
            .iter()
            .any(|profile| profile.profile_id() == default)
    {
        return Err(ErrorDto::validation(
            "invalid_provider_default_profile",
            "the global default profile must be a declared member of the catalog",
        ));
    }
    let active = input.active();
    if let Some(active) = active {
        ensure_kind_immutability(active, &resolved_kinds)?;
    }
    let change = classify_change(
        active,
        default_profile_id.as_ref(),
        &kind_descriptors,
        &profile_revisions,
        &profile_policies,
    );
    let removals = removal_entries(active, &kind_descriptors, &profile_revisions);
    let candidate_revision_id = catalog_revision_identity(
        default_profile_id.as_ref(),
        &kind_descriptors,
        &profile_revisions,
        &profile_policies,
    )?;
    Ok(CatalogPreparationDto {
        candidate_revision_id,
        expected_active_revision_id: active.map(ProviderCatalogRevisionDto::catalog_revision_id),
        change,
        removals,
        default_profile_id,
        kind_descriptors,
        profile_revisions,
        profile_policies,
    })
}

/// Accepts one prepared catalog revision in one storage transaction.
///
/// An unchanged preparation returns the already-accepted revision identity
/// without a durable write: a semantic-equal catalog writes no new revision.
/// Every other preparation commits its kinds, profile revisions, policies, and
/// membership, then advances the accepted pointer and appends the catalog audit
/// records. No external action happens inside this call.
///
/// # Errors
///
/// Returns a typed validation or conflict error when the revision cannot be
/// accepted under the single-version catalog law, or an unavailable error when
/// durable storage fails.
pub fn accept_catalog_revision<Repository>(
    repository: &Repository,
    preparation: &CatalogPreparationDto,
    occurred_at: TimestampDto,
) -> DtoResult<CatalogRevisionId>
where
    Repository: StorageRepositoryDto,
{
    if let Some(active) = preparation.expected_active_revision_id()
        && preparation.change() == CatalogChangeKindDto::Unchanged
    {
        return Ok(active);
    }
    let state = repository.accept_catalog_revision(preparation.revision_at(occurred_at)?)?;
    state
        .accepted_catalog_revision_id()
        .ok_or_else(catalog_acceptance_missing)
}

/// Marks the exact accepted catalog revision as the active one.
///
/// # Errors
///
/// Returns a conflict error when the addressed revision is not the accepted
/// one, or an unavailable error when durable storage fails.
pub fn mark_catalog_activated<Repository>(
    repository: &Repository,
    catalog_revision_id: CatalogRevisionId,
    occurred_at: TimestampDto,
) -> DtoResult<ProviderCatalogStateDto>
where
    Repository: StorageRepositoryDto,
{
    repository.mark_catalog_activated(catalog_revision_id, occurred_at)
}

/// The closed outcome of one activation-recovery attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogActivationRecoveryDto {
    /// The exact accepted revision was re-derived and is now active.
    Recovered {
        /// The committed accepted/activated pointer pair.
        state: ProviderCatalogStateDto,
    },
    /// The current candidate is not the accepted revision; recovery stays required.
    Mismatch {
        /// The accepted revision that stays unactivated.
        accepted_catalog_revision_id: CatalogRevisionId,
    },
}

impl CatalogActivationRecoveryDto {
    /// Returns whether the exact accepted revision became active.
    #[must_use]
    pub const fn recovered_exactly(self) -> bool {
        matches!(self, Self::Recovered { .. })
    }
}

/// Recovers one accepted-but-unactivated catalog revision.
///
/// Recovery activates the accepted revision only when the re-derived candidate
/// carries exactly the accepted revision's meaning. A changed source document
/// never replaces the accepted revision: the daemon stays
/// `activation_recovery_required` until the exact accepted catalog is
/// re-derived, and no partial state is committed by a mismatch.
///
/// # Errors
///
/// Returns a conflict error when the accepted pointer no longer addresses the
/// supplied revision, or an unavailable error when durable storage fails.
pub fn recover_catalog_activation<Repository>(
    repository: &Repository,
    accepted: &ProviderCatalogRevisionDto,
    candidate: &CatalogPreparationDto,
    occurred_at: TimestampDto,
) -> DtoResult<CatalogActivationRecoveryDto>
where
    Repository: StorageRepositoryDto,
{
    if !candidate.matches_revision(accepted) {
        return Ok(CatalogActivationRecoveryDto::Mismatch {
            accepted_catalog_revision_id: accepted.catalog_revision_id(),
        });
    }
    let state = mark_catalog_activated(repository, accepted.catalog_revision_id(), occurred_at)?;
    Ok(CatalogActivationRecoveryDto::Recovered { state })
}

/// Records one rejected pending removal candidate.
///
/// A rejection never emits acceptance or activation, drops the process-local
/// candidate, and leaves the accepted catalog active with the closed
/// `removal_candidate_rejected` degraded reason.
///
/// # Errors
///
/// Returns an unavailable error when durable storage fails.
pub fn reject_catalog_candidate<Repository>(
    repository: &Repository,
    handle: ProviderCatalogCandidateHandleDto,
    occurred_at: TimestampDto,
) -> DtoResult<intention_proto::provider::ProviderCatalogCandidateRejectedDto>
where
    Repository: StorageRepositoryDto,
{
    repository.record_catalog_candidate_rejected(handle.candidate_revision_id(), occurred_at)?;
    Ok(
        intention_proto::provider::ProviderCatalogCandidateRejectedDto::new(Some(
            handle.expected_active_revision_id(),
        )),
    )
}

/// Returns the candidate's resolved kind descriptors, ordered by kind id.
///
/// Every code-owned first-party descriptor is always present: it names a
/// driver the tree can serve, so it is never a removal candidate. Declared user
/// kinds are validated through the closed composition matrix.
///
/// # Errors
///
/// Returns a typed validation error when a code-owned descriptor or user-kind
/// composition cannot form a valid descriptor revision.
fn candidate_kind_descriptors(
    document: &CatalogDocumentDto,
) -> DtoResult<BTreeMap<ProviderKindId, ProviderKindDescriptorRevisionV1>> {
    let mut descriptors = BTreeMap::new();
    for descriptor in first_party_kind_descriptors()? {
        descriptors.insert(descriptor.kind_id().clone(), descriptor);
    }
    for declared in document.user_kinds() {
        descriptors.insert(declared.kind_id().clone(), user_kind_descriptor(declared)?);
    }
    Ok(descriptors)
}

/// Returns the validated descriptor revision of one declared user kind.
///
/// # Errors
///
/// Returns a typed validation error when the composition cannot form a valid
/// descriptor revision.
fn user_kind_descriptor(
    declared: &CatalogUserKindDto,
) -> DtoResult<ProviderKindDescriptorRevisionV1> {
    let descriptor = validate_user_kind(declared.composition())?;
    if descriptor.kind_id() != declared.kind_id() {
        return Err(ErrorDto::validation(
            "invalid_provider_kind",
            "a user-kind composition must declare its own kind identity",
        ));
    }
    Ok(descriptor)
}

/// Returns the candidate's resolved profile revisions, ordered by profile id.
///
/// # Errors
///
/// Returns `provider_kind_has_dependents` when a profile references a kind the
/// candidate does not declare, and the typed failures of capability,
/// endpoint, and credential-transport resolution.
fn candidate_profile_revisions(
    document: &CatalogDocumentDto,
    kind_descriptors: &BTreeMap<ProviderKindId, ProviderKindDescriptorRevisionV1>,
) -> DtoResult<Vec<ProviderProfileRevisionV1>> {
    let mut profiles = document.profiles().to_vec();
    profiles.sort_by(|left, right| left.profile_id().cmp(right.profile_id()));
    let mut revisions = Vec::with_capacity(profiles.len());
    for profile in profiles {
        let declaration = profile.declaration();
        let Some(descriptor) = kind_descriptors.get(declaration.kind_id()) else {
            // The candidate drops a kind a profile still points at: only a
            // candidate that removes or reassigns every dependent may remove a
            // kind, and a dangling profile is never admitted.
            return Err(ErrorDto::validation(
                "provider_kind_has_dependents",
                "a profile references a kind this candidate does not declare",
            ));
        };
        validate_declaration_policies(descriptor, declaration)?;
        let declared = declared_capability_set(
            descriptor.model_capability_envelope(),
            declaration.declared_capabilities(),
        )?;
        let capabilities = resolved_capability_set(descriptor, &declared)?;
        let credential_transport = CredentialTransportDto::new(
            declaration.credential_transport_mode(),
            declaration
                .credential_transport_safe_header_name()
                .map(str::to_owned),
        )
        .map_err(|_| invalid_provider_credential_transport())?;
        let reasoning_policy = resolved_reasoning_policy(
            declaration.reasoning_effort(),
            declaration.declared_capabilities(),
            &capabilities,
        )?;
        let revision_id = profile_revision_identity(
            declaration.profile_id(),
            declaration.kind_id(),
            descriptor.descriptor_revision_id(),
            declaration.model_id(),
            declaration.normalized_effective_endpoint(),
            &credential_transport,
            &capabilities,
            &reasoning_policy,
            *declaration.effective_execution_policy(),
            declaration.effective_loopback_policy(),
        )?;
        revisions.push(ProviderProfileRevisionV1::new(
            declaration.profile_id().clone(),
            revision_id,
            declaration.kind_id().clone(),
            descriptor.descriptor_revision_id(),
            declaration.model_id(),
            declaration
                .normalized_effective_endpoint()
                .map(str::to_owned),
            credential_transport,
            capabilities,
            reasoning_policy,
            *declaration.effective_execution_policy(),
            declaration.effective_loopback_policy(),
        )?);
    }
    Ok(revisions)
}

/// Returns the candidate's profile policies, ordered by profile id.
///
/// # Errors
///
/// Returns a typed validation error when a declared display name is not a safe
/// presentation name.
fn candidate_profile_policies(
    document: &CatalogDocumentDto,
) -> DtoResult<Vec<ProviderProfilePolicyEntryDto>> {
    let mut policies = Vec::with_capacity(document.profiles().len());
    for profile in document.profiles() {
        let policy = ProviderProfilePolicyDto::new(
            profile.display_name(),
            profile.enabled(),
            profile.pricing().cloned(),
        )?;
        policies.push(ProviderProfilePolicyEntryDto::new(
            profile.profile_id().clone(),
            policy,
        ));
    }
    policies.sort_by(|left, right| left.profile_id().cmp(right.profile_id()));
    Ok(policies)
}

/// Validates one profile declaration against its kind descriptor policies.
///
/// # Errors
///
/// Returns a typed validation error when the declared endpoint or credential
/// transport is not representable by the kind descriptor.
fn validate_declaration_policies(
    descriptor: &ProviderKindDescriptorRevisionV1,
    declaration: &intention_config::catalog::CatalogProfileDeclarationDto,
) -> DtoResult<()> {
    let endpoint_policy = descriptor.endpoint_policy();
    match declaration.normalized_effective_endpoint() {
        Some(_) if !endpoint_policy.permits_override() => {
            return Err(invalid_provider_endpoint(
                "the kind descriptor does not permit an endpoint override",
            ));
        }
        None if endpoint_policy.requires_explicit_endpoint() => {
            return Err(invalid_provider_endpoint(
                "the kind descriptor requires an explicit endpoint",
            ));
        }
        _ => {}
    }
    if declaration.effective_loopback_policy() == LoopbackPolicyDto::ExplicitLoopback
        && !endpoint_policy.permits_loopback_http()
    {
        return Err(invalid_provider_endpoint(
            "the kind descriptor does not permit a loopback HTTP endpoint",
        ));
    }
    let contract = descriptor.credential_transport_contract();
    if !contract.supports(declaration.credential_transport_mode()) {
        return Err(invalid_provider_credential_transport());
    }
    if declaration.credential_transport_mode() == CredentialTransportModeDto::SafeHeader
        && declaration.credential_transport_safe_header_name() != contract.safe_header_name()
    {
        return Err(invalid_provider_credential_transport());
    }
    Ok(())
}

/// Returns the declared exact-model capability subset of one profile.
///
/// The declared set stays exactly what the document declares: the closed
/// taxonomy fixes the model input kind, structured output, and the reasoning
/// input contract, while text streaming, textual reasoning, and tool exchange
/// come from the declaration. The subsequent intersection rejects a component
/// the kind envelope cannot represent instead of narrowing it silently.
///
/// # Errors
///
/// Returns a typed validation error when the declared capability set is not
/// coherent under the current taxonomy version.
fn declared_capability_set(
    envelope: &ModelCapabilitySetV1,
    declared: &CatalogDeclaredCapabilitySubsetDto,
) -> DtoResult<ModelCapabilitySetV1> {
    let reasoning = match declared.reasoning() {
        CatalogDeclaredReasoningCapabilityDto::Disabled => ReasoningCapabilityDto::Disabled,
        CatalogDeclaredReasoningCapabilityDto::TextualReasoningV1 {
            supported_efforts,
            summary_support,
        } => ReasoningCapabilityDto::textual_reasoning_v1(
            supported_efforts.clone(),
            *summary_support,
        )?,
    };
    let tool_exchange = if declared.tool_exchange() {
        envelope.tool_exchange().clone()
    } else {
        ToolExchangeCapabilityDto::Disabled
    };
    ModelCapabilitySetV1::new(
        envelope.taxonomy_version(),
        envelope.input(),
        availability(declared.text_streaming()),
        envelope.structured_output(),
        reasoning,
        tool_exchange,
        envelope.context_preservation().clone(),
    )
}

/// Returns the resolved exact-model capability set of one profile.
///
/// A first-party kind is resolved through the code-owned compatibility matrix,
/// which also proves a driver contract serves the resolved set. A user kind has
/// no code-owned driver contract, so admission validates the declared set
/// against the composition envelope only: every run against it is still blocked
/// before effect, because no live driver entry can exist for it.
///
/// # Errors
///
/// Returns a typed validation error when the declared set is not representable
/// by the kind envelope, or when no supported driver contract covers the
/// resolved set.
fn resolved_capability_set(
    descriptor: &ProviderKindDescriptorRevisionV1,
    declared: &ModelCapabilitySetV1,
) -> DtoResult<ModelCapabilitySetV1> {
    if driver_contract(descriptor.kind_id()).is_some() {
        return validate_capability_subset(descriptor, declared);
    }
    admit_user_kind_subset(descriptor, declared)
}

/// Intersects one declared subset with a driverless user-kind envelope.
///
/// This mirrors the compatibility matrix's closed intersection without the
/// driver-contract requirement: a user-kind descriptor is admitted, never
/// executed, so only envelope representability is checked.
///
/// # Errors
///
/// Returns a typed validation error when the declared subset enables a
/// component the composition envelope does not represent.
fn admit_user_kind_subset(
    descriptor: &ProviderKindDescriptorRevisionV1,
    declared: &ModelCapabilitySetV1,
) -> DtoResult<ModelCapabilitySetV1> {
    let envelope = descriptor.model_capability_envelope();
    if declared.taxonomy_version() != envelope.taxonomy_version() {
        return Err(invalid_capability_set());
    }
    if declared.text_streaming() == ProviderCapabilityAvailabilityDto::Enabled
        && envelope.text_streaming() == ProviderCapabilityAvailabilityDto::Disabled
    {
        return Err(invalid_capability_set());
    }
    let reasoning = match (envelope.reasoning(), declared.reasoning()) {
        (_, ReasoningCapabilityDto::Disabled) => ReasoningCapabilityDto::Disabled,
        (ReasoningCapabilityDto::Disabled, ReasoningCapabilityDto::TextualReasoningV1 { .. }) => {
            return Err(invalid_capability_set());
        }
        (
            ReasoningCapabilityDto::TextualReasoningV1 {
                supported_efforts: envelope_efforts,
                summary_support: envelope_summary,
            },
            ReasoningCapabilityDto::TextualReasoningV1 {
                supported_efforts,
                summary_support,
            },
        ) => {
            if !supported_efforts
                .iter()
                .all(|effort| envelope_efforts.contains(effort))
                || (*summary_support && !envelope_summary)
            {
                return Err(invalid_capability_set());
            }
            ReasoningCapabilityDto::textual_reasoning_v1(
                supported_efforts.clone(),
                *summary_support,
            )?
        }
    };
    if declared.tool_exchange() != &ToolExchangeCapabilityDto::Disabled {
        return Err(invalid_capability_set());
    }
    if declared.context_preservation() != envelope.context_preservation() {
        return Err(invalid_capability_set());
    }
    ModelCapabilitySetV1::new(
        envelope.taxonomy_version(),
        envelope.input(),
        declared.text_streaming(),
        envelope.structured_output(),
        reasoning,
        ToolExchangeCapabilityDto::Disabled,
        envelope.context_preservation().clone(),
    )
}

/// Returns the resolved reasoning policy of one profile.
///
/// The closed effort set narrows to the declared exact-model subset, summary
/// support stays the declared capability, and the cross-turn transfer contract
/// is the resolved capability set's reasoning input contract: a profile can
/// receive exactly the history material its kind descriptor permits.
///
/// # Errors
///
/// Returns a typed validation error when the resolved policy is not
/// representable by the resolved capability set.
fn resolved_reasoning_policy(
    effort: Option<intention_proto::provider::ReasoningEffortLevelDto>,
    declared: &CatalogDeclaredCapabilitySubsetDto,
    capabilities: &ModelCapabilitySetV1,
) -> DtoResult<ResolvedReasoningPolicyDto> {
    let reasoning_declared = matches!(
        declared.reasoning(),
        CatalogDeclaredReasoningCapabilityDto::TextualReasoningV1 { .. }
    );
    let summary_support = matches!(
        declared.reasoning(),
        CatalogDeclaredReasoningCapabilityDto::TextualReasoningV1 {
            summary_support: true,
            ..
        }
    );
    let transfer = capabilities.reasoning_input_contract().clone();
    // A textual transfer needs transferable material: the closed category set
    // is the closed dialect's own fact, and a reasoning-capable exact model
    // produces it as well.
    let supported_categories =
        if reasoning_declared || !matches!(transfer, ReasoningHistoryTransferDto::Disabled) {
            vec![
                ReasoningFragmentCategoryDto::Primary,
                ReasoningFragmentCategoryDto::Detail,
            ]
        } else {
            Vec::new()
        };
    ResolvedReasoningPolicyDto::new(effort, summary_support, transfer, supported_categories)
}

/// Returns the closed availability of one declared capability.
const fn availability(declared: bool) -> ProviderCapabilityAvailabilityDto {
    if declared {
        ProviderCapabilityAvailabilityDto::Enabled
    } else {
        ProviderCapabilityAvailabilityDto::Disabled
    }
}

/// Enforces kind immutability against the accepted catalog revision.
///
/// A kind identity keeps the declaration it was first accepted with; the valid
/// path to changed closed parts is a new kind identity plus reassignment.
///
/// # Errors
///
/// Returns `provider_kind_immutable_mismatch` when an accepted kind id resolves
/// to a different declaration.
fn ensure_kind_immutability(
    active: &ProviderCatalogRevisionDto,
    candidate: &BTreeMap<ProviderKindId, ProviderKindDescriptorRevisionV1>,
) -> DtoResult<()> {
    for accepted in active.kind_descriptors() {
        let Some(candidate) = candidate.get(accepted.kind_id()) else {
            // The kind is being removed: the removal impact carries that fact,
            // and the storage path writes the removal tombstone.
            continue;
        };
        if !kind_declaration_equals(accepted, candidate) {
            return Err(ErrorDto::validation(
                "provider_kind_immutable_mismatch",
                "a provider kind identity must keep its first accepted declaration",
            ));
        }
    }
    Ok(())
}

/// Returns the closed change classification of one prepared candidate.
fn classify_change(
    active: Option<&ProviderCatalogRevisionDto>,
    default_profile_id: Option<&ProviderProfileId>,
    kind_descriptors: &[ProviderKindDescriptorRevisionV1],
    profile_revisions: &[ProviderProfileRevisionV1],
    profile_policies: &[ProviderProfilePolicyEntryDto],
) -> CatalogChangeKindDto {
    let Some(active) = active else {
        return CatalogChangeKindDto::CatalogAffecting;
    };
    let same_kinds = kind_meaning_equals(active.kind_descriptors(), kind_descriptors);
    let same_profiles = profile_meaning_equals(active.profile_revisions(), profile_revisions);
    let same_default = active.default_profile_id() == default_profile_id;
    if same_kinds && same_profiles && same_default && active.profile_policies() == profile_policies
    {
        CatalogChangeKindDto::Unchanged
    } else if same_kinds && same_profiles && same_default {
        CatalogChangeKindDto::FreshRunsOnly
    } else {
        CatalogChangeKindDto::CatalogAffecting
    }
}

/// Returns the accepted members the candidate omits, sorted by identity.
fn removal_entries(
    active: Option<&ProviderCatalogRevisionDto>,
    kind_descriptors: &[ProviderKindDescriptorRevisionV1],
    profile_revisions: &[ProviderProfileRevisionV1],
) -> Vec<CatalogRemovalEntryDto> {
    let Some(active) = active else {
        return Vec::new();
    };
    let candidate_profiles = profile_revisions
        .iter()
        .map(ProviderProfileRevisionV1::profile_id)
        .collect::<BTreeSet<_>>();
    let candidate_kinds = kind_descriptors
        .iter()
        .map(ProviderKindDescriptorRevisionV1::kind_id)
        .collect::<BTreeSet<_>>();
    let mut removals = Vec::new();
    for profile in active.profile_revisions() {
        if !candidate_profiles.contains(profile.profile_id()) {
            removals.push(CatalogRemovalEntryDto::Profile {
                profile_id: profile.profile_id().clone(),
            });
        }
    }
    for descriptor in active.kind_descriptors() {
        if !candidate_kinds.contains(descriptor.kind_id()) {
            removals.push(CatalogRemovalEntryDto::Kind {
                kind_id: descriptor.kind_id().clone(),
            });
        }
    }
    removals.sort_by_cached_key(removal_key);
    removals
}

/// Returns the stable sort key of one removal entry.
fn removal_key(entry: &CatalogRemovalEntryDto) -> (bool, String) {
    match entry {
        CatalogRemovalEntryDto::Profile { profile_id } => (false, profile_id.as_str().to_owned()),
        CatalogRemovalEntryDto::Kind { kind_id } => (true, kind_id.as_str().to_owned()),
    }
}

/// Returns whether two kind descriptor lists carry the same declaration meaning.
fn kind_meaning_equals(
    left: &[ProviderKindDescriptorRevisionV1],
    right: &[ProviderKindDescriptorRevisionV1],
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| kind_declaration_equals(left, right))
}

/// Returns whether two kind descriptors declare the same closed parts.
fn kind_declaration_equals(
    left: &ProviderKindDescriptorRevisionV1,
    right: &ProviderKindDescriptorRevisionV1,
) -> bool {
    left.kind_id() == right.kind_id()
        && left.descriptor_family() == right.descriptor_family()
        && left.ordered_protocol_part_revisions() == right.ordered_protocol_part_revisions()
        && left.endpoint_policy() == right.endpoint_policy()
        && left.credential_transport_contract() == right.credential_transport_contract()
        && left.model_capability_envelope() == right.model_capability_envelope()
        && left.driver_contract_family() == right.driver_contract_family()
}

/// Returns whether two profile revision lists carry the same revision meaning.
fn profile_meaning_equals(
    left: &[ProviderProfileRevisionV1],
    right: &[ProviderProfileRevisionV1],
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| profile_revision_equals(left, right))
}

/// Returns whether two profile revisions carry the same revision-affecting
/// meaning, ignoring the immutable revision identity itself.
fn profile_revision_equals(
    left: &ProviderProfileRevisionV1,
    right: &ProviderProfileRevisionV1,
) -> bool {
    left.profile_id() == right.profile_id()
        && left.kind_id() == right.kind_id()
        && left.kind_descriptor_revision_id() == right.kind_descriptor_revision_id()
        && left.model_id() == right.model_id()
        && left.normalized_effective_endpoint() == right.normalized_effective_endpoint()
        && left.credential_transport() == right.credential_transport()
        && left.declared_model_capability_subset() == right.declared_model_capability_subset()
        && left.resolved_reasoning_policy() == right.resolved_reasoning_policy()
        && left.effective_execution_policy() == right.effective_execution_policy()
        && left.effective_loopback_policy() == right.effective_loopback_policy()
}

/// Returns the derived catalog revision identity of one candidate meaning.
///
/// # Errors
///
/// Returns a validation error when the derived identity is not a canonical UUID.
fn catalog_revision_identity(
    default_profile_id: Option<&ProviderProfileId>,
    kind_descriptors: &[ProviderKindDescriptorRevisionV1],
    profile_revisions: &[ProviderProfileRevisionV1],
    profile_policies: &[ProviderProfilePolicyEntryDto],
) -> DtoResult<CatalogRevisionId> {
    let mut identity = IdentityWriter::new();
    identity.token("provider-catalog-revision-v1");
    match default_profile_id {
        Some(profile_id) => identity.token(profile_id.as_str()),
        None => identity.token(""),
    }
    for descriptor in kind_descriptors {
        identity.token(descriptor.kind_id().as_str());
        identity.token(&descriptor.descriptor_revision_id().to_string());
    }
    for profile in profile_revisions {
        identity.token(profile.profile_id().as_str());
        identity.token(&profile.revision_id().to_string());
    }
    for entry in profile_policies {
        identity.token(entry.profile_id().as_str());
        identity.token(entry.policy().display_name());
        identity.flag(entry.policy().enabled());
        match entry.policy().pricing() {
            Some(pricing) => {
                identity.flag(true);
                identity.decimal(pricing.input_per_million_tokens());
                identity.decimal(pricing.output_per_million_tokens());
            }
            None => identity.flag(false),
        }
    }
    CatalogRevisionId::parse(&identity.into_uuid())
}

/// Returns the derived profile revision identity of one profile meaning.
///
/// # Errors
///
/// Returns a validation error when the derived identity is not a canonical UUID.
#[expect(
    clippy::too_many_arguments,
    reason = "The derivation covers every revision-affecting profile field at one call site."
)]
fn profile_revision_identity(
    profile_id: &ProviderProfileId,
    kind_id: &ProviderKindId,
    kind_descriptor_revision_id: ProviderKindDescriptorRevisionId,
    model_id: &str,
    endpoint: Option<&str>,
    credential_transport: &CredentialTransportDto,
    capabilities: &ModelCapabilitySetV1,
    reasoning_policy: &ResolvedReasoningPolicyDto,
    execution_policy: intention_proto::provider::ProviderExecutionPolicyDto,
    loopback_policy: LoopbackPolicyDto,
) -> DtoResult<ProviderProfileRevisionId> {
    let mut identity = IdentityWriter::new();
    identity.token("provider-profile-revision-v1");
    identity.token(profile_id.as_str());
    identity.token(kind_id.as_str());
    identity.token(&kind_descriptor_revision_id.to_string());
    identity.token(model_id);
    identity.token(endpoint.unwrap_or_default());
    identity.token(credential_transport.mode().as_str());
    identity.token(credential_transport.safe_header_name().unwrap_or_default());
    write_capabilities(&mut identity, capabilities);
    write_reasoning_policy(&mut identity, reasoning_policy);
    identity.number(u64::from(execution_policy.attempt_timeout_seconds()));
    identity.number(u64::from(execution_policy.max_attempts()));
    identity.token(match loopback_policy {
        LoopbackPolicyDto::NotApplicable => "not-applicable",
        LoopbackPolicyDto::ExplicitLoopback => "explicit-loopback",
    });
    ProviderProfileRevisionId::parse(&identity.into_uuid())
}

/// Writes one capability set into an identity reduction.
fn write_capabilities(identity: &mut IdentityWriter, capabilities: &ModelCapabilitySetV1) {
    identity.token(capabilities.taxonomy_version().as_str());
    identity.token(match capabilities.input() {
        ModelInputKindDto::TextOnly => "text-only",
    });
    write_availability(identity, capabilities.text_streaming());
    write_availability(identity, capabilities.structured_output());
    match capabilities.reasoning() {
        ReasoningCapabilityDto::Disabled => identity.token("reasoning-disabled"),
        ReasoningCapabilityDto::TextualReasoningV1 {
            supported_efforts,
            summary_support,
        } => {
            identity.token("textual-reasoning-v1");
            for effort in supported_efforts {
                identity.token(effort.as_str());
            }
            identity.flag(*summary_support);
        }
    }
    match capabilities.tool_exchange() {
        ToolExchangeCapabilityDto::Disabled => identity.token("tool-exchange-disabled"),
        ToolExchangeCapabilityDto::ModelToolLoopV1 {
            translation_revision,
        } => {
            identity.token("model-tool-loop-v1");
            identity.token(translation_revision);
        }
    }
    write_context_preservation(identity, capabilities.context_preservation());
}

/// Writes one reasoning policy into an identity reduction.
fn write_reasoning_policy(identity: &mut IdentityWriter, policy: &ResolvedReasoningPolicyDto) {
    match policy.effort() {
        Some(effort) => identity.token(effort.as_str()),
        None => identity.token(""),
    }
    identity.flag(policy.summary_support());
    write_transfer(identity, policy.transfer());
    for category in policy.supported_categories() {
        identity.token(category_token(*category));
    }
}

/// Returns the canonical durable token of one reasoning fragment category.
const fn category_token(category: ReasoningFragmentCategoryDto) -> &'static str {
    match category {
        ReasoningFragmentCategoryDto::Primary => "primary",
        ReasoningFragmentCategoryDto::Detail => "detail",
    }
}

/// Writes one context-preservation declaration into an identity reduction.
fn write_context_preservation(
    identity: &mut IdentityWriter,
    preservation: &ContextPreservationCapabilityDto,
) {
    identity.token("local-durable-history-v1");
    write_transfer(identity, preservation.reasoning_input_contract());
}

/// Writes one reasoning transfer contract into an identity reduction.
fn write_transfer(identity: &mut IdentityWriter, transfer: &ReasoningHistoryTransferDto) {
    match transfer {
        ReasoningHistoryTransferDto::Disabled => identity.token("disabled"),
        ReasoningHistoryTransferDto::TextualHistoryV1 { compatibility_id } => {
            identity.token("textual-history-v1");
            identity.token(compatibility_id);
        }
    }
}

/// Writes one availability declaration into an identity reduction.
fn write_availability(
    identity: &mut IdentityWriter,
    availability: ProviderCapabilityAvailabilityDto,
) {
    identity.token(match availability {
        ProviderCapabilityAvailabilityDto::Enabled => "enabled",
        ProviderCapabilityAvailabilityDto::Disabled => "disabled",
    });
}

/// One length-prefixed identity reduction.
///
/// Every token carries its own length, so two different field sequences never
/// reduce to the same identity text.
struct IdentityWriter {
    text: String,
}

impl IdentityWriter {
    /// Creates an empty identity reduction.
    const fn new() -> Self {
        Self {
            text: String::new(),
        }
    }

    /// Writes one length-prefixed token.
    fn token(&mut self, value: &str) {
        self.text.push_str(&value.len().to_string());
        self.text.push(':');
        self.text.push_str(value);
        self.text.push('|');
    }

    /// Writes one length-free number.
    fn number(&mut self, value: u64) {
        self.text.push_str(&value.to_string());
        self.text.push('|');
    }

    /// Writes one closed flag.
    fn flag(&mut self, value: bool) {
        self.text.push_str(if value { "1|" } else { "0|" });
    }

    /// Writes one optional declared currency-free price.
    fn decimal(&mut self, value: Option<f64>) {
        match value {
            Some(value) => {
                self.text.push_str(&value.to_bits().to_string());
                self.text.push('|');
            }
            None => self.token(""),
        }
    }

    /// Reduces the identity text to one canonical UUID-shaped identifier.
    fn into_uuid(self) -> String {
        let mut bytes = [0_u8; 16];
        bytes[..8].copy_from_slice(&fnv1a_64(&self.text).to_be_bytes());
        bytes[8..].copy_from_slice(&fnv1a_64(&format!("identity-v1|{}", self.text)).to_be_bytes());
        // The identity stays a canonical version-4 UUID even though it is derived.
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let hex = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    }
}

/// Reduces one identity string to sixty-four bits (FNV-1a).
fn fnv1a_64(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The typed failure for an endpoint the kind descriptor cannot represent.
fn invalid_provider_endpoint(message: &'static str) -> ErrorDto {
    ErrorDto::validation("invalid_provider_endpoint", message)
}

/// The typed failure for a credential transport the kind descriptor cannot represent.
fn invalid_provider_credential_transport() -> ErrorDto {
    ErrorDto::validation(
        "invalid_provider_credential_transport",
        "the declared credential transport is not the kind descriptor's contract",
    )
}

/// The typed failure for a declared capability set the kind envelope cannot represent.
fn invalid_capability_set() -> ErrorDto {
    ErrorDto::validation(
        "invalid_model_capability_set",
        "the declared capability subset is not representable by the kind envelope",
    )
}

/// The typed failure for a durable acceptance that produced no accepted pointer.
fn catalog_acceptance_missing() -> ErrorDto {
    ErrorDto::unavailable(
        "storage_unavailable",
        "catalog acceptance committed without an accepted revision pointer",
    )
}
