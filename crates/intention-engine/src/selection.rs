//! Ordered provider-selection resolution, durable session defaults, and usage reads.
//!
//! One accepted turn or run carries exactly one
//! [`ResolvedRunProviderSelectionDto`]. Resolution walks the closed source order
//! — turn override, durable session default, global default — and classifies
//! every rejection before any commit: an expected-revision mismatch fails with
//! `provider_profile_revision_mismatch`, and a profile the composition cannot
//! drive fails with `provider_profile_runtime_unavailable`. An exact selection
//! that becomes unavailable later fails the admitted run with
//! `provider_configuration_unavailable` at execution admission instead.
//!
//! The module owns no durable write of its own: the session default command
//! delegates its optimistic commit to storage and publishes the typed change
//! value through the composition's session-event port after that commit.

use intention_proto::provider::{
    ProviderProfileId, ProviderProfileOverrideDto, ProviderProfilePolicyDto,
    ProviderProfileRevisionV1, ProviderSelectionSourceDto, ProviderSelectionUnavailabilityDto,
    ResolvedRunProviderSelectionDto, SessionProviderProfileChangedDto,
    SetSessionProviderProfileAcceptedDto, SetSessionProviderProfileCommandDto,
};
use intention_proto::{DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto, TimestampDto};
use intention_providers::driver_contract;
use intention_storage::{
    ProfileUsageAggregateDto, ProviderCatalogRevisionDto, SessionProviderProfileChangeDto,
    StorageRepositoryDto,
};

/// Liveness check over the composition's private provider driver registry.
///
/// The registry is private to composition: it holds one driver client per exact
/// `(ProfileId, ProviderProfileRevisionId, ProviderKindDescriptorRevisionId,
/// ProviderDriverContractRevisionDto)` identity. The engine asks the
/// composition only whether an exact selection has a live entry; no driver,
/// handle, or SDK resource crosses this boundary.
pub trait ProviderDriverRegistry {
    /// Returns whether the exact selection has a live private driver entry.
    fn has_live_driver(&self, selection: &ResolvedRunProviderSelectionDto) -> bool;
}

/// The ordered inputs of one selection resolution.
///
/// The caller supplies the turn's optional override, the durable session
/// default, the catalog's global default, the accepted catalog revision, and
/// the composition's registry liveness check. Every field is a safe,
/// credential-free value.
pub struct SelectionResolutionInputDto<'a> {
    turn_override: Option<ProviderProfileOverrideDto>,
    session_default_profile_id: Option<ProviderProfileId>,
    global_default_profile_id: Option<ProviderProfileId>,
    active_catalog: Option<ProviderCatalogRevisionDto>,
    registry: &'a dyn ProviderDriverRegistry,
}

impl<'a> SelectionResolutionInputDto<'a> {
    /// Creates one resolution input in the frozen source order.
    #[must_use]
    pub const fn new(
        turn_override: Option<ProviderProfileOverrideDto>,
        session_default_profile_id: Option<ProviderProfileId>,
        global_default_profile_id: Option<ProviderProfileId>,
        active_catalog: Option<ProviderCatalogRevisionDto>,
        registry: &'a dyn ProviderDriverRegistry,
    ) -> Self {
        Self {
            turn_override,
            session_default_profile_id,
            global_default_profile_id,
            active_catalog,
            registry,
        }
    }

    /// Returns the turn's explicit override, when one was supplied.
    #[must_use]
    pub const fn turn_override(&self) -> Option<&ProviderProfileOverrideDto> {
        self.turn_override.as_ref()
    }
}

/// The closed outcome of one selection resolution.
#[expect(
    clippy::large_enum_variant,
    reason = "One exact selection travels by value per turn admission; boxing it would add indirection to every dispatch site without changing the resolved meaning."
)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SelectionResolutionDto {
    /// One exact selection is backed by a live private driver entry.
    Selected(ResolvedRunProviderSelectionDto),
    /// The effective profile exists but cannot serve this turn.
    Unavailable {
        /// The effective profile identity.
        profile_id: ProviderProfileId,
        /// The provenance of the effective profile.
        source: ProviderSelectionSourceDto,
        /// The closed reason the exact selection is unavailable.
        reason: ProviderSelectionUnavailabilityDto,
    },
    /// No override, session default, or global default declared any profile.
    Unconfigured,
}

impl SelectionResolutionDto {
    /// Returns the exact selection of one acceptance, or the typed rejection.
    ///
    /// Acceptance rejects an unavailable or unconfigured selection before any
    /// durable commit: a registry failure is
    /// `provider_profile_runtime_unavailable`, and a catalog with no effective
    /// profile at all is `provider_configuration_unavailable`.
    ///
    /// # Errors
    ///
    /// Returns the closed rejection of an unavailable or unconfigured
    /// resolution.
    pub fn into_accepted_selection(self) -> DtoResult<ResolvedRunProviderSelectionDto> {
        match self {
            Self::Selected(selection) => Ok(selection),
            Self::Unavailable { .. } => Err(ErrorDto::unavailable(
                "provider_profile_runtime_unavailable",
                "the resolved provider profile has no live runtime entry",
            )),
            Self::Unconfigured => Err(ErrorDto::unavailable(
                "provider_configuration_unavailable",
                "no provider profile is configured for this turn",
            )),
        }
    }

    /// Returns the resolved exact selection, when one resolved.
    #[must_use]
    pub const fn selection(&self) -> Option<&ResolvedRunProviderSelectionDto> {
        match self {
            Self::Selected(selection) => Some(selection),
            Self::Unavailable { .. } | Self::Unconfigured => None,
        }
    }
}

/// Resolves the exact provider selection of one turn or run.
///
/// The source order is frozen: an explicit turn override wins, then the durable
/// session default, then the global default. A turn override carrying an
/// expected profile revision is checked against the resolved exact revision and
/// rejected before any commit. A profile that is absent, disabled, has no
/// code-owned driver contract, or has no live registry entry resolves as an
/// [`SelectionResolutionDto::Unavailable`] outcome instead of a silent
/// fallback: there is no fallback chain.
///
/// # Errors
///
/// Returns `provider_profile_revision_mismatch` when the turn override's
/// expected profile revision is not the resolved revision.
pub fn resolve_selection(
    input: SelectionResolutionInputDto<'_>,
) -> DtoResult<SelectionResolutionDto> {
    let (profile_id, source) = if let Some(turn_override) = input.turn_override.as_ref() {
        (
            turn_override.profile_id(),
            ProviderSelectionSourceDto::TurnOverride,
        )
    } else if let Some(session_default) = input.session_default_profile_id.as_ref() {
        (session_default, ProviderSelectionSourceDto::SessionDefault)
    } else if let Some(global_default) = input.global_default_profile_id.as_ref() {
        (global_default, ProviderSelectionSourceDto::GlobalDefault)
    } else {
        return Ok(SelectionResolutionDto::Unconfigured);
    };
    let Some((revision, enabled)) = resolved_profile(input.active_catalog.as_ref(), profile_id)
    else {
        return Ok(SelectionResolutionDto::Unavailable {
            profile_id: profile_id.clone(),
            source,
            reason: ProviderSelectionUnavailabilityDto::Missing,
        });
    };
    if !enabled {
        return Ok(SelectionResolutionDto::Unavailable {
            profile_id: profile_id.clone(),
            source,
            reason: ProviderSelectionUnavailabilityDto::Disabled,
        });
    }
    if let Some(expected) = input
        .turn_override
        .as_ref()
        .and_then(ProviderProfileOverrideDto::expected_profile_revision_id)
        && expected != revision.revision_id()
    {
        return Err(ErrorDto::validation(
            "provider_profile_revision_mismatch",
            "the expected provider profile revision is not the resolved revision",
        ));
    }
    let Some(driver_contract) = driver_contract(revision.kind_id()) else {
        // A kind with no code-owned driver contract can be admitted to a
        // catalog but never driven, so the exact selection has no live entry.
        return Ok(SelectionResolutionDto::Unavailable {
            profile_id: profile_id.clone(),
            source,
            reason: ProviderSelectionUnavailabilityDto::RuntimeUnavailable,
        });
    };
    let selection =
        ResolvedRunProviderSelectionDto::from_profile_revision(revision, driver_contract, source);
    if !input.registry.has_live_driver(&selection) {
        return Ok(SelectionResolutionDto::Unavailable {
            profile_id: profile_id.clone(),
            source,
            reason: ProviderSelectionUnavailabilityDto::RuntimeUnavailable,
        });
    }
    Ok(SelectionResolutionDto::Selected(selection))
}

/// Returns the exact profile revision and enabled state of one catalog member.
fn resolved_profile<'a>(
    active_catalog: Option<&'a ProviderCatalogRevisionDto>,
    profile_id: &ProviderProfileId,
) -> Option<(&'a ProviderProfileRevisionV1, bool)> {
    let catalog = active_catalog?;
    let revision = catalog
        .profile_revisions()
        .iter()
        .find(|revision| revision.profile_id() == profile_id)?;
    let policy: &ProviderProfilePolicyDto = catalog
        .profile_policies()
        .iter()
        .find(|entry| entry.profile_id() == profile_id)?
        .policy();
    Some((revision, policy.enabled()))
}

/// The composition's validating session-event boundary.
///
/// Publication happens only after a durable commit, carries the exact committed
/// value, and never fails that commit. No durable copy of the value is written;
/// durable delivery belongs to a later milestone.
pub trait SessionEventSink: Send + Sync {
    /// Observes one committed session provider-profile change.
    fn observe_session_provider_profile_change(&self, event: &SessionProviderProfileChangedDto);
}

/// Commits one optimistic durable session provider-profile change.
///
/// Storage owns the optimistic check: a mismatched expected projection revision
/// is a typed conflict that commits nothing, and setting the already-durable
/// profile is a successful `changed = false` outcome that leaves the revision
/// untouched. On a real change the engine constructs and validates the typed
/// change value against the committed outcome, then publishes it to the
/// composition's session-event port.
///
/// # Errors
///
/// Returns the typed storage rejection, including the
/// `session_projection_revision_conflict` of a stale expected revision.
pub fn set_session_provider_profile<Repository>(
    repository: &Repository,
    sink: &dyn SessionEventSink,
    command: &SetSessionProviderProfileCommandDto,
    occurred_at: TimestampDto,
) -> DtoResult<SetSessionProviderProfileAcceptedDto>
where
    Repository: StorageRepositoryDto,
{
    let change = repository.set_session_provider_profile(
        command.session_id(),
        command.profile_id().clone(),
        command.expected_session_projection_revision(),
        occurred_at,
    )?;
    if change.changed() {
        let event = SessionProviderProfileChangedDto::new(
            change.session_id(),
            command.profile_id().clone(),
            change.session_projection_revision(),
        );
        validate_committed_change(&event, command, &change)?;
        sink.observe_session_provider_profile_change(&event);
    }
    Ok(SetSessionProviderProfileAcceptedDto::new(
        change.session_id(),
        change.changed(),
        change.session_projection_revision(),
    ))
}

/// Validates one committed change before its typed value is published.
///
/// The committed outcome must address the command's own session and profile,
/// must advance the projection revision on a real change, and must leave it
/// untouched on a `changed = false` outcome. A violation is an internal
/// inconsistency rather than a caller error: it commits a change whose value
/// cannot be published safely.
///
/// # Errors
///
/// Returns a safe internal error when the committed outcome is incoherent.
fn validate_committed_change(
    event: &SessionProviderProfileChangedDto,
    command: &SetSessionProviderProfileCommandDto,
    change: &SessionProviderProfileChangeDto,
) -> DtoResult<()> {
    let advanced =
        change.session_projection_revision() > command.expected_session_projection_revision();
    let coherent = event.session_id() == command.session_id()
        && event.profile_id() == command.profile_id()
        && event.session_projection_revision() == change.session_projection_revision()
        && advanced;
    if coherent {
        return Ok(());
    }
    Err(ErrorDto::new(
        "invalid_session_provider_profile_change",
        ErrorCategoryDto::Internal,
        "the committed session provider-profile change is incoherent",
        ErrorRetryDto::Never,
        None,
    )?)
}

/// Loads one bounded usage aggregate per `(profile revision, model)` identity.
///
/// Usage stays keyed by the exact profile identity and revision; the read
/// produces no price, currency, or estimated cost. The aggregate set is the
/// storage contract's own current-state read.
///
/// # Errors
///
/// Returns an unavailable error when durable storage cannot be read.
pub fn load_profile_usage<Repository>(
    repository: &Repository,
    profile_id: &ProviderProfileId,
) -> DtoResult<Vec<ProfileUsageAggregateDto>>
where
    Repository: StorageRepositoryDto,
{
    repository.load_profile_usage(profile_id.clone())
}
