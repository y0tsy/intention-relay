//! Slice 2 session-selection runtime services (zone 5).
//!
//! This module implements the provider session-selection control plane:
//! session provider defaults, per-turn selection resolution, the
//! unavailable-provider queue, provider usage aggregation, catalog reads, the
//! removal lifecycle, held recovered-run admission, and the degraded-mode
//! gate. Every public boundary is DTO-only and credential-free; no private
//! driver handle, credential, SDK resource, or raw configuration crosses a
//! public boundary.
//!
//! Durable event-stream append for control-plane events (for example
//! `SessionProviderProfileChangedEventDto`) is not yet exposed by the storage
//! surface; the services construct the protocol event DTOs where applicable
//! and the durable append is a later storage zone.

use intention_domain::{
    CredentialTransportMode as DomainCredentialTransportMode, ProviderSelectionV1,
    SendUserTurnCommandDto, canonical::CanonicalError,
};
use intention_protocol::contract_families::{
    AcceptProviderCatalogRemovalAcceptedDto, AcceptProviderCatalogRemovalCommandDto,
    AdmitRecoveredRunAcceptedDto, AdmitRecoveredRunCommandDto, CredentialTransportMode,
    GetProviderCatalogQueryDto, GetProviderCatalogStatusQueryDto, GetProviderUsageQueryDto,
    GetSessionProviderProfileQueryDto, MAX_UNAVAILABLE_QUEUE_PROMOTIONS,
    ProviderCatalogActivationState, ProviderCatalogDegradedReason, ProviderCatalogEntryDto,
    ProviderCatalogPageDto, ProviderCatalogStatusDto, ProviderProfileUnavailableReason,
    ProviderReadinessDto, ReconcileUnavailableQueueAcceptedDto,
    ReconcileUnavailableQueueCommandDto, RejectProviderCatalogCandidateAcceptedDto,
    RejectProviderCatalogCandidateCommandDto, ResolvedProviderProfileDto,
    ResolvedRunProviderSelectionDto, SessionProviderProfileDto,
    SetSessionProviderProfileAcceptedDto, SetSessionProviderProfileCommandDto, UsageAggregationDto,
};
use intention_storage::{
    AdmitHeldRecoveredRunInputDto, HeldRunAdmissionStateDto, HeldRunRepositoryDto,
    LoadProviderCatalogPageInputDto, PromoteUnavailableRunsInputDto,
    PromoteUnavailableRunsOutcomeDto, ProviderCatalogRepositoryDto, ProviderRemovalRepositoryDto,
    ProviderSelectionRepositoryDto, ProviderUsageRepositoryDto, ReconcileUnavailableQueueInputDto,
    RecordProviderUsageInputDto, SessionProviderDefaultsRepositoryDto,
    SetSessionProviderProfileInputDto, UnavailableQueueRepositoryDto,
};
use intention_types::{DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto, RunId, SessionId};

use crate::provider_catalog::ProviderCatalogController;
use crate::provider_gate::CatalogReadiness;
use crate::{ModelRunDispatchPort, ScheduleModelRunDto};

/// The deterministic selection canonicalization version of Slice 2 selections.
const SELECTION_CANONICALIZATION_VERSION: &str = "provider-selection-v1";
/// The closed value carried when a session has no provider profile intent.
const PROFILE_UNSET: &str = "unset";
/// The maximum provider catalog page size used by read services.
const MAX_CATALOG_PAGE_SIZE: u64 = 256;
/// The maximum reconciliation batch per pass.
const MAX_RECONCILIATION_BATCH: u64 = 32;

/// Maps one domain canonical error to its typed boundary error.
fn domain_error(error: CanonicalError) -> ErrorDto {
    match error {
        CanonicalError::CredentialsForbidden => {
            ErrorDto::validation("credentials_forbidden", "credentials are forbidden")
        }
        CanonicalError::InvalidEndpoint => {
            ErrorDto::validation("invalid_endpoint", "endpoint is invalid")
        }
        CanonicalError::InvalidProviderKind => {
            ErrorDto::validation("invalid_provider_kind", "provider kind is invalid")
        }
        CanonicalError::ProviderProfileRevisionInvalid => ErrorDto::validation(
            "provider_profile_revision_invalid",
            "provider profile revision is invalid",
        ),
        CanonicalError::DuplicateOrDescendingField => ErrorDto::validation(
            "provider_selection_invalid",
            "provider selection fields are noncanonical",
        ),
        CanonicalError::OverLimit | CanonicalError::Truncated => ErrorDto::validation(
            "provider_selection_invalid",
            "provider selection exceeds its canonical record bound",
        ),
        CanonicalError::InvalidMagic
        | CanonicalError::InvalidVersion
        | CanonicalError::InvalidTag
        | CanonicalError::InvalidField
        | CanonicalError::InvalidWireType
        | CanonicalError::InvalidUtf8
        | CanonicalError::InvalidBool
        | CanonicalError::InvalidOptional
        | CanonicalError::UnknownField(_)
        | CanonicalError::TrailingBytes
        | CanonicalError::DigestMismatch
        | CanonicalError::InvalidDigest
        | CanonicalError::ContextSourceManifestInvalid
        | CanonicalError::ModelContextProjectionInvalid
        | CanonicalError::ModelContextProjectionTooLarge
        | CanonicalError::LegacySelectionReferenceInvalid
        | CanonicalError::ProviderKindImmutableMismatch
        | CanonicalError::ProviderKindHasDependents
        | CanonicalError::ReasoningHistoryUnavailable
        | CanonicalError::ReasoningHistoryIncompatible
        | CanonicalError::ReasoningHistoryTooLarge
        | CanonicalError::ReasoningOutputLimitExceeded
        | CanonicalError::ProviderReasoningStreamInvalid => ErrorDto::validation(
            "provider_selection_invalid",
            "provider selection record is invalid",
        ),
    }
}

/// Converts one protocol resolved selection into its durable domain selection.
///
/// # Errors
///
/// Returns the protocol selection's own validation error, or the domain
/// selection's canonical validation error.
pub(crate) fn provider_selection_from(
    value: &ResolvedRunProviderSelectionDto,
) -> DtoResult<ProviderSelectionV1> {
    value.validate()?;
    let selection = ProviderSelectionV1 {
        selection_canonicalization_version: value.selection_canonicalization_version.clone(),
        profile_id: value.profile_id.clone(),
        provider_profile_revision_id: value.provider_profile_revision_id.clone(),
        kind_id: value.kind_id.clone(),
        kind_descriptor_revision_id: value.kind_descriptor_revision_id.clone(),
        model_id: value.model_id.clone(),
        normalized_effective_endpoint: value.normalized_effective_endpoint.clone(),
        credential_transport_mode: match value.credential_transport_mode {
            CredentialTransportMode::Bearer => DomainCredentialTransportMode::Bearer,
            CredentialTransportMode::SafeHeader => DomainCredentialTransportMode::SafeHeader,
        },
        credential_transport_safe_header_name: value.credential_transport_safe_header_name.clone(),
        declared_model_capability_subset: value.declared_model_capability_subset.clone(),
        resolved_reasoning_policy: value.resolved_reasoning_policy.clone(),
        effective_execution_policy: value.effective_execution_policy.clone(),
        effective_loopback_policy_or_not_applicable: value
            .effective_loopback_policy_or_not_applicable
            .clone(),
        provider_driver_contract_revision: value.provider_driver_contract_revision.clone(),
        selection_source: value.selection_source.clone(),
    };
    selection.validate().map_err(domain_error)?;
    Ok(selection)
}

/// The complete credential-free resolved projection of one enabled profile.
///
/// Every field mirrors the durable `ProviderSelectionV1` payload minus the
/// selection provenance, so a resolved profile can be turned into a selection
/// for any provenance without re-reading storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedProfileDto {
    pub profile_id: String,
    pub profile_revision_id: String,
    pub kind_id: String,
    pub kind_descriptor_revision_id: String,
    pub model_id: String,
    pub normalized_effective_endpoint: String,
    pub credential_transport_mode: CredentialTransportMode,
    pub credential_transport_safe_header_name: Option<String>,
    pub declared_model_capability_subset: Vec<String>,
    pub resolved_reasoning_policy: String,
    pub effective_execution_policy: String,
    pub effective_loopback_policy_or_not_applicable: String,
    pub provider_driver_contract_revision: String,
}

/// Maps one durable profile candidate to its resolved projection.
fn resolved_profile_from_candidate(
    candidate: &intention_storage::ProviderProfileCandidateDto,
) -> ResolvedProfileDto {
    let profile = &candidate.profile;
    let contract = &profile.driver_contract_revision;
    ResolvedProfileDto {
        profile_id: profile.profile_id.clone(),
        profile_revision_id: profile.revision_id.clone(),
        kind_id: profile.provider_kind_id.clone(),
        kind_descriptor_revision_id: profile.kind_descriptor_revision_id.clone(),
        model_id: profile.model_id.clone(),
        normalized_effective_endpoint: profile.endpoint.clone(),
        credential_transport_mode: match profile.credential_transport_mode {
            DomainCredentialTransportMode::Bearer => CredentialTransportMode::Bearer,
            DomainCredentialTransportMode::SafeHeader => CredentialTransportMode::SafeHeader,
        },
        credential_transport_safe_header_name: profile.safe_header_name.clone(),
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
    }
}

/// Resolves one enabled, ready profile to its complete safe projection.
///
/// Implementations must fail closed with `provider_profile_unavailable`,
/// `provider_profile_tombstoned`, `provider_configuration_unavailable`, or
/// `execution_not_ready` when the profile cannot serve. No credential, path,
/// or private handle crosses this boundary.
pub trait CatalogAdmissionPort {
    /// Resolves the active enabled revision of one profile.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable error when the catalog is not ready, the
    /// profile is unknown, disabled, tombstoned, or its driver is not
    /// admitted.
    fn resolve_enabled_profile(&self, profile_id: &str) -> DtoResult<ResolvedProfileDto>;

    /// Verifies one exact persisted provider-selection registry key.
    ///
    /// The key is the full admission identity of the persisted selection:
    /// profile id, provider profile revision id, kind descriptor revision id,
    /// and driver contract revision. Implementations must fail closed when
    /// the catalog is not ready, the exact key is not admitted, the entry is
    /// disabled or tombstoned, or the persisted driver contract is not the
    /// admitted one.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable or validation error when the exact key
    /// cannot be admitted.
    fn verify_registry_key(
        &self,
        profile_id: &str,
        provider_profile_revision_id: &str,
        kind_descriptor_revision_id: &str,
        driver_contract_revision: &str,
    ) -> DtoResult<()>;
}

/// Reads the current control-plane catalog readiness.
pub trait ControlPlaneReadinessPort {
    /// Returns the current catalog readiness.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the readiness cannot be read.
    fn readiness(&self) -> DtoResult<CatalogReadiness>;
}

/// The degraded-mode gate: rejects provider state changes while degraded.
pub struct DegradedModeService;

impl DegradedModeService {
    /// Rejects every provider state change while the catalog is degraded.
    ///
    /// Degraded states are `Blocked`, `PendingRemoval`, and
    /// `ActivationRecoveryRequired`. Health, safe catalog status/validation,
    /// session/run/tree reads, and the accept/reject of the one pending
    /// removal candidate remain allowed.
    ///
    /// # Errors
    ///
    /// Returns `execution_not_ready` while the catalog is degraded.
    pub fn assert_execution_ready(&self, readiness: &CatalogReadiness) -> DtoResult<()> {
        match readiness {
            CatalogReadiness::Ready
            | CatalogReadiness::Uninitialized
            | CatalogReadiness::Loading => Ok(()),
            CatalogReadiness::Blocked { .. }
            | CatalogReadiness::PendingRemoval { .. }
            | CatalogReadiness::ActivationRecoveryRequired { .. } => Err(ErrorDto::new(
                "execution_not_ready",
                ErrorCategoryDto::Unavailable,
                "the provider control plane is degraded and read-only",
                ErrorRetryDto::Delayed,
                None,
            )?),
        }
    }
}

/// Resolves the effective provider selection for one fresh run.
pub struct SelectionResolutionService;

impl SelectionResolutionService {
    /// Resolves the effective profile for one user turn.
    ///
    /// The effective profile is the explicit per-turn override, then the
    /// session durable default, then the global catalog default. When no
    /// profile applies (the legacy path) this returns `Ok(None)` and the
    /// caller keeps the legacy selection-less commit. An expected profile
    /// revision mismatch rejects before any durable commit with
    /// `provider_profile_revision_mismatch`; a registry failure maps to
    /// `provider_profile_runtime_unavailable`.
    ///
    /// # Errors
    ///
    /// Returns the typed resolution error of the admission port or the
    /// revision-mismatch validation error.
    pub fn resolve_for_turn(
        &self,
        command: &SendUserTurnCommandDto,
        session_default: Option<String>,
        global_default: Option<String>,
        port: &impl CatalogAdmissionPort,
    ) -> DtoResult<Option<ResolvedRunProviderSelectionDto>> {
        let (profile_id, expected_revision, source) =
            if let Some(override_id) = command.profile_override() {
                (
                    override_id.to_owned(),
                    command.expected_profile_revision().map(str::to_owned),
                    "per_turn_override",
                )
            } else if let Some(default) = session_default {
                (default, None, "session_default")
            } else if let Some(default) = global_default {
                (default, None, "global_default")
            } else {
                return Ok(None);
            };
        let resolved = port
            .resolve_enabled_profile(&profile_id)
            .map_err(runtime_unavailable)?;
        if let Some(expected) = &expected_revision
            && expected != &resolved.profile_revision_id
        {
            return Err(ErrorDto::validation(
                "provider_profile_revision_mismatch",
                "the resolved provider profile revision does not match the expected revision",
            ));
        }
        let selection = resolved_selection(resolved, source)?;
        Ok(Some(selection))
    }

    /// Resolves one explicit fork override, when present.
    ///
    /// # Errors
    ///
    /// Returns the typed resolution error of the admission port or the
    /// revision-mismatch validation error.
    pub fn resolve_for_override(
        &self,
        profile_override: Option<&str>,
        expected_profile_revision: Option<&str>,
        port: &impl CatalogAdmissionPort,
    ) -> DtoResult<Option<ResolvedRunProviderSelectionDto>> {
        let Some(profile_id) = profile_override else {
            return Ok(None);
        };
        let resolved = port
            .resolve_enabled_profile(profile_id)
            .map_err(runtime_unavailable)?;
        if let Some(expected) = expected_profile_revision
            && expected != resolved.profile_revision_id
        {
            return Err(ErrorDto::validation(
                "provider_profile_revision_mismatch",
                "the resolved provider profile revision does not match the expected revision",
            ));
        }
        let selection = resolved_selection(resolved, "fork_override")?;
        Ok(Some(selection))
    }
}

/// Maps one admission failure to the closed runtime-unavailable error.
fn runtime_unavailable(error: ErrorDto) -> ErrorDto {
    match error.code() {
        "provider_admission_not_found"
        | "provider_profile_unavailable"
        | "provider_profile_tombstoned" => ErrorDto::new(
            "provider_profile_runtime_unavailable",
            ErrorCategoryDto::Unavailable,
            "the resolved provider profile cannot serve this run",
            ErrorRetryDto::Delayed,
            None,
        )
        .unwrap_or_else(|_| {
            ErrorDto::unavailable(
                "provider_profile_runtime_unavailable",
                "the resolved provider profile cannot serve this run",
            )
        }),
        _ => error,
    }
}

/// Builds the validated protocol selection from one resolved profile.
fn resolved_selection(
    resolved: ResolvedProfileDto,
    source: &str,
) -> DtoResult<ResolvedRunProviderSelectionDto> {
    let selection = ResolvedRunProviderSelectionDto {
        selection_canonicalization_version: SELECTION_CANONICALIZATION_VERSION.to_owned(),
        profile_id: resolved.profile_id,
        provider_profile_revision_id: resolved.profile_revision_id,
        kind_id: resolved.kind_id,
        kind_descriptor_revision_id: resolved.kind_descriptor_revision_id,
        model_id: resolved.model_id,
        normalized_effective_endpoint: resolved.normalized_effective_endpoint,
        credential_transport_mode: resolved.credential_transport_mode,
        credential_transport_safe_header_name: resolved.credential_transport_safe_header_name,
        declared_model_capability_subset: resolved.declared_model_capability_subset,
        resolved_reasoning_policy: resolved.resolved_reasoning_policy,
        effective_execution_policy: resolved.effective_execution_policy,
        effective_loopback_policy_or_not_applicable: resolved
            .effective_loopback_policy_or_not_applicable,
        provider_driver_contract_revision: resolved.provider_driver_contract_revision,
        selection_source: Some(source.to_owned()),
    };
    selection.validate()?;
    Ok(selection)
}

/// The session provider default service.
pub struct SessionProfileService<'a> {
    defaults: &'a dyn SessionProviderDefaultsRepositoryDto,
    catalog: &'a dyn ProviderCatalogRepositoryDto,
    readiness: &'a dyn ControlPlaneReadinessPort,
}

impl<'a> SessionProfileService<'a> {
    /// Creates a session profile service over the durable default repository,
    /// the catalog repository, and the control-plane readiness port.
    #[must_use]
    pub fn new(
        defaults: &'a dyn SessionProviderDefaultsRepositoryDto,
        catalog: &'a dyn ProviderCatalogRepositoryDto,
        readiness: &'a dyn ControlPlaneReadinessPort,
    ) -> Self {
        Self {
            defaults,
            catalog,
            readiness,
        }
    }

    /// Binds one session's durable provider profile intent.
    ///
    /// The requested profile must resolve against the active catalog as
    /// enabled and available. The expected session projection revision is
    /// compared optimistically; a mismatch rejects with
    /// `session_profile_revision_mismatch` before any write. The operation is
    /// idempotent by operation identity, and a same-profile request is a
    /// `changed = false` no-op that never rewrites existing runs or queued
    /// turns.
    ///
    /// # Errors
    ///
    /// Returns `execution_not_ready` while degraded, the admission port's
    /// typed resolution error, `session_profile_revision_mismatch` for a stale
    /// expected revision, or the durable storage error.
    pub fn set(
        &self,
        command: SetSessionProviderProfileCommandDto,
        port: &impl CatalogAdmissionPort,
        now: u64,
    ) -> DtoResult<SetSessionProviderProfileAcceptedDto> {
        command.validate()?;
        DegradedModeService.assert_execution_ready(&self.readiness.readiness()?)?;
        let session_id = SessionId::parse(&command.session_id)?;
        let resolved = port.resolve_enabled_profile(&command.profile_id)?;
        let outcome = self
            .defaults
            .set_session_provider_profile(SetSessionProviderProfileInputDto {
                session_id,
                profile_id: command.profile_id.clone(),
                expected_projection_revision: command.expected_session_projection_revision,
                operation_id: command.operation_id.clone(),
                updated_at: i64_time(now),
            })
            .map_err(|error| {
                if error.code() == "session_provider_default_stale" {
                    ErrorDto::validation(
                        "session_profile_revision_mismatch",
                        "the session provider profile changed concurrently",
                    )
                } else {
                    error
                }
            })?;
        Ok(SetSessionProviderProfileAcceptedDto {
            session_id: command.session_id,
            changed: outcome.changed,
            resulting_projection_revision: outcome.projection_revision,
            resolved: ResolvedProviderProfileDto::Resolved {
                profile_id: resolved.profile_id,
                profile_revision_id: resolved.profile_revision_id,
            },
        })
    }

    /// Loads one session's durable provider profile projection.
    ///
    /// The projection carries the durable intent, the current safe resolved
    /// entry/revision or a closed unavailability reason, the session
    /// projection revision, and the global default from the catalog status.
    ///
    /// # Errors
    ///
    /// Returns the typed storage or resolution error.
    pub fn get(
        &self,
        query: GetSessionProviderProfileQueryDto,
        port: &impl CatalogAdmissionPort,
    ) -> DtoResult<SessionProviderProfileDto> {
        query.validate()?;
        let session_id = SessionId::parse(&query.session_id)?;
        let global_default = self
            .catalog
            .load_provider_catalog_status()
            .ok()
            .and_then(|state| state.active_default_profile_id)
            .unwrap_or_else(|| PROFILE_UNSET.to_owned());
        let default = self.defaults.get_session_provider_profile(session_id)?;
        let profile_id = default
            .as_ref()
            .map(|default| default.profile_id.clone())
            .unwrap_or_else(|| global_default.clone());
        let resolved = match port.resolve_enabled_profile(&profile_id) {
            Ok(resolved) => ResolvedProviderProfileDto::Resolved {
                profile_id: resolved.profile_id,
                profile_revision_id: resolved.profile_revision_id,
            },
            Err(error) => ResolvedProviderProfileDto::Unavailable(unavailable_reason_for(&error)),
        };
        let projection = SessionProviderProfileDto {
            session_id: query.session_id,
            profile_id,
            resolved,
            session_projection_revision: default
                .as_ref()
                .map_or(0, |default| default.projection_revision),
            global_default_profile_id: global_default,
        };
        projection.validate()?;
        Ok(projection)
    }
}

/// Maps one resolution error to its closed unavailability reason.
fn unavailable_reason_for(error: &ErrorDto) -> ProviderProfileUnavailableReason {
    match error.code() {
        "catalog_not_ready" | "execution_not_ready" | "provider_catalog_not_active" => {
            ProviderProfileUnavailableReason::CatalogNotActive
        }
        "provider_profile_tombstoned" => ProviderProfileUnavailableReason::ProfileDisabled,
        "provider_configuration_unavailable" | "provider_profile_runtime_unavailable" => {
            ProviderProfileUnavailableReason::ProviderUnavailable
        }
        "provider_profile_unavailable" => ProviderProfileUnavailableReason::ProfileDisabled,
        _ => ProviderProfileUnavailableReason::ProfileNotFound,
    }
}

/// The unavailable-provider queue service.
pub struct UnavailableQueueService<'a> {
    queue: &'a dyn UnavailableQueueRepositoryDto,
}

impl<'a> UnavailableQueueService<'a> {
    /// Creates an unavailable-queue service over the durable queue repository.
    #[must_use]
    pub const fn new(queue: &'a dyn UnavailableQueueRepositoryDto) -> Self {
        Self { queue }
    }

    /// Promotes up to eight queued unavailable runs in FIFO order.
    ///
    /// Called on terminal transitions. The storage enforces the closed
    /// eight-run batch, preserves each entry's original run identity and
    /// profile (never rerouting to the current default or a new revision),
    /// and creates a reconciliation marker when the queue is exhausted. No
    /// provider call is made here: a still-unavailable provider keeps the
    /// promoted run in the daemon's fail-closed admission path.
    ///
    /// # Errors
    ///
    /// Returns the durable storage error.
    pub fn promote(
        &self,
        session_id: SessionId,
        run_id: RunId,
        now: u64,
    ) -> DtoResult<PromoteUnavailableRunsOutcomeDto> {
        self.queue
            .promote_unavailable_runs(PromoteUnavailableRunsInputDto {
                now: i64_time(now),
                operation_id: format!("promote-{session_id}-{run_id}-{now}"),
                max: MAX_UNAVAILABLE_QUEUE_PROMOTIONS,
            })
    }

    /// Reconciles one bounded page of the unavailable queue.
    ///
    /// The storage reconciles up to 32 entries per page, terminalizes only
    /// entries whose runs reached a terminal state, and never reroutes. The
    /// service then promotes up to the closed eight-run bound so a recovered
    /// provider can resume the first available entries.
    ///
    /// # Errors
    ///
    /// Returns `execution_not_ready` while degraded, or the durable storage
    /// error.
    pub fn reconcile(
        &self,
        command: ReconcileUnavailableQueueCommandDto,
        readiness: &dyn ControlPlaneReadinessPort,
        now: u64,
    ) -> DtoResult<ReconcileUnavailableQueueAcceptedDto> {
        command.validate()?;
        DegradedModeService.assert_execution_ready(&readiness.readiness()?)?;
        let session_id = SessionId::parse(&command.session_id)?;
        self.queue
            .reconcile_unavailable_queue(ReconcileUnavailableQueueInputDto {
                now: i64_time(now),
                operation_id: command.operation_id.clone(),
                max: MAX_RECONCILIATION_BATCH,
            })?;
        let promoted = self
            .queue
            .promote_unavailable_runs(PromoteUnavailableRunsInputDto {
                now: i64_time(now),
                operation_id: format!("{}-promote", command.operation_id),
                max: MAX_UNAVAILABLE_QUEUE_PROMOTIONS,
            })?;
        let page_cursor = self
            .queue
            .load_queue_reconciliation_marker(session_id)?
            .and_then(|marker| marker.next_page_cursor);
        let accepted = ReconcileUnavailableQueueAcceptedDto {
            session_id: command.session_id,
            page_cursor,
            promoted_count: u64::try_from(promoted.promoted.len()).unwrap_or(u64::MAX),
        };
        accepted.validate()?;
        Ok(accepted)
    }
}

/// The provider usage aggregation service.
pub struct UsageService<'a> {
    usage: &'a dyn ProviderUsageRepositoryDto,
}

impl<'a> UsageService<'a> {
    /// Creates a usage service over the durable usage repository.
    #[must_use]
    pub const fn new(usage: &'a dyn ProviderUsageRepositoryDto) -> Self {
        Self { usage }
    }

    /// Records a batch of provider usage events with no double counting.
    ///
    /// The storage deduplicates by `(run_id, usage_event_id)` identity and
    /// never carries price, currency, or cost values.
    ///
    /// # Errors
    ///
    /// Returns the durable storage error.
    pub fn record(&self, input: RecordProviderUsageInputDto) -> DtoResult<()> {
        self.usage.record_provider_usage(input)
    }

    /// Loads the usage aggregation of one provider profile over a period.
    ///
    /// # Errors
    ///
    /// Returns the typed query validation or durable storage error.
    pub fn by_profile(&self, query: GetProviderUsageQueryDto) -> DtoResult<UsageAggregationDto> {
        query.validate()?;
        let aggregates = self
            .usage
            .load_provider_usage_by_profile(query.profile_id.clone())?;
        aggregate_usage(&query, aggregates)
    }

    /// Loads the usage aggregation of one profile revision and model.
    ///
    /// # Errors
    ///
    /// Returns the typed query validation or durable storage error.
    pub fn by_revision_and_model(
        &self,
        query: &GetProviderUsageQueryDto,
        provider_profile_revision_id: String,
        model_id: String,
    ) -> DtoResult<UsageAggregationDto> {
        query.validate()?;
        let aggregates = self
            .usage
            .load_provider_usage_by_revision_and_model(provider_profile_revision_id, model_id)?;
        aggregate_usage(query, aggregates)
    }
}

/// Aggregates one period's durable usage aggregates into one projection.
fn aggregate_usage(
    query: &GetProviderUsageQueryDto,
    aggregates: Vec<intention_storage::ProviderUsageAggregateDto>,
) -> DtoResult<UsageAggregationDto> {
    let mut request_count = 0_u64;
    let mut input_units = 0_u64;
    let mut output_units = 0_u64;
    let mut reasoning_units = 0_u64;
    let mut revision = String::new();
    let mut model = String::new();
    for aggregate in aggregates {
        if aggregate.usage_period_start < i64_time(query.usage_period_start)
            || aggregate.usage_period_end > i64_time(query.usage_period_end)
        {
            continue;
        }
        request_count = request_count.saturating_add(aggregate.request_count);
        input_units = input_units.saturating_add(aggregate.input_units);
        output_units = output_units.saturating_add(aggregate.output_units);
        reasoning_units = reasoning_units.saturating_add(aggregate.reasoning_units);
        revision = aggregate.provider_profile_revision_id;
        model = aggregate.model_id;
    }
    let aggregation = UsageAggregationDto {
        profile_id: query.profile_id.clone(),
        provider_profile_revision_id: revision,
        model_id: model,
        request_count,
        input_units,
        output_units,
        reasoning_units,
        usage_period_start: query.usage_period_start,
        usage_period_end: query.usage_period_end,
    };
    aggregation.validate()?;
    Ok(aggregation)
}

/// The provider catalog read service.
pub struct CatalogReadService<'a, Catalog, Removal>
where
    Catalog: ProviderCatalogRepositoryDto,
    Removal: ProviderRemovalRepositoryDto,
{
    catalog: &'a dyn ProviderCatalogRepositoryDto,
    controller: &'a ProviderCatalogController<Catalog, Removal>,
}

impl<'a, Catalog, Removal> CatalogReadService<'a, Catalog, Removal>
where
    Catalog: ProviderCatalogRepositoryDto,
    Removal: ProviderRemovalRepositoryDto,
{
    /// Creates a catalog read service over the durable catalog repository and
    /// the catalog runtime controller.
    #[must_use]
    pub fn new(
        catalog: &'a dyn ProviderCatalogRepositoryDto,
        controller: &'a ProviderCatalogController<Catalog, Removal>,
    ) -> Self {
        Self {
            catalog,
            controller,
        }
    }

    /// Loads one paged, profile-id-sorted provider catalog projection.
    ///
    /// The page is bounded and carries an opaque continuation token. A stale
    /// expected catalog revision rejects with `catalog_page_token_stale`
    /// before any read.
    ///
    /// # Errors
    ///
    /// Returns the typed query validation, stale-token, or durable storage
    /// error.
    pub fn list_profiles(
        &self,
        query: GetProviderCatalogQueryDto,
    ) -> DtoResult<ProviderCatalogPageDto> {
        query.validate()?;
        let status = self.catalog.load_provider_catalog_status()?;
        let active_revision = status
            .active_catalog_revision_id
            .map_or_else(String::new, |revision| revision.to_string());
        if let Some(expected) = &query.expected_catalog_revision_id
            && expected != &active_revision
        {
            return Err(ErrorDto::validation(
                "catalog_page_token_stale",
                "the provider catalog changed since the page token was issued",
            ));
        }
        let page = self
            .catalog
            .load_provider_catalog_page(LoadProviderCatalogPageInputDto {
                token: query.page_token,
                limit: MAX_CATALOG_PAGE_SIZE,
            })?;
        let material = self.catalog.load_provider_catalog_material()?;
        let entries = page
            .entries
            .into_iter()
            .map(|entry| catalog_entry(entry, &material))
            .collect::<DtoResult<Vec<_>>>()?;
        let projection = ProviderCatalogPageDto {
            schema_version: query.schema_version,
            catalog_revision_id: active_revision,
            entries,
            next_page_token: page.next_token,
            has_more: page.has_more,
        };
        projection.validate()?;
        Ok(projection)
    }

    /// Loads the provider catalog activation and degradation status.
    ///
    /// The activation state and degraded reason derive from the controller
    /// readiness; the active/candidate revisions and default come from the
    /// durable catalog state.
    ///
    /// # Errors
    ///
    /// Returns the typed query validation or durable storage error.
    pub fn status(
        &self,
        query: GetProviderCatalogStatusQueryDto,
    ) -> DtoResult<ProviderCatalogStatusDto> {
        query.validate()?;
        let projection = self.controller.inspect()?;
        let state = self.catalog.load_provider_catalog_status()?;
        let (activation_state, degraded_reason) = match &projection.readiness {
            CatalogReadiness::Ready => (ProviderCatalogActivationState::Active, None),
            CatalogReadiness::Uninitialized | CatalogReadiness::Loading => {
                (ProviderCatalogActivationState::Preparing, None)
            }
            CatalogReadiness::PendingRemoval { .. } => (
                ProviderCatalogActivationState::PendingRemoval,
                Some(ProviderCatalogDegradedReason::RemovalCandidatePending),
            ),
            CatalogReadiness::ActivationRecoveryRequired { .. } => (
                ProviderCatalogActivationState::ActivationRecoveryRequired,
                Some(ProviderCatalogDegradedReason::ActivationRecoveryRequired),
            ),
            CatalogReadiness::Blocked { reason } => (
                ProviderCatalogActivationState::PendingRemoval,
                Some(match reason.as_str() {
                    "removal_candidate_rejected" => {
                        ProviderCatalogDegradedReason::RemovalCandidateRejected
                    }
                    "removal_candidate_expired" => {
                        ProviderCatalogDegradedReason::RemovalCandidateExpired
                    }
                    _ => ProviderCatalogDegradedReason::ActivationRecoveryRequired,
                }),
            ),
        };
        let status = ProviderCatalogStatusDto {
            schema_version: query.schema_version,
            activation_state,
            degraded_reason,
            active_catalog_revision_id: state
                .active_catalog_revision_id
                .map(|revision| revision.to_string()),
            candidate_catalog_revision_id: state
                .candidate_catalog_revision_id
                .map(|revision| revision.to_string()),
            active_default_profile_id: state.active_default_profile_id,
            removal_impact: None,
            provider_profiles_negotiated: true,
        };
        status.validate()?;
        Ok(status)
    }
}

/// Maps one durable catalog page entry plus the active material to its
/// protocol projection.
fn catalog_entry(
    entry: intention_storage::ProviderCatalogProfileEntryDto,
    material: &intention_storage::ProviderCatalogMaterialDto,
) -> DtoResult<ProviderCatalogEntryDto> {
    let candidate = material
        .profiles
        .iter()
        .find(|candidate| {
            candidate.profile.profile_id == entry.profile_id
                && candidate.profile.revision_id == entry.profile_revision_id
        })
        .ok_or_else(|| {
            ErrorDto::unavailable(
                "provider_catalog_projection_invalid",
                "the durable catalog projection is invalid",
            )
        })?;
    let projection = resolved_profile_from_candidate(candidate);
    let projected = ProviderCatalogEntryDto {
        profile_id: entry.profile_id,
        profile_revision_id: entry.profile_revision_id,
        display_name: entry
            .display_name
            .unwrap_or_else(|| projection.profile_id.clone()),
        enabled: entry.enabled,
        provider_kind_id: projection.kind_id,
        kind_descriptor_revision_id: projection.kind_descriptor_revision_id,
        model_id: projection.model_id,
        normalized_endpoint: Some(projection.normalized_effective_endpoint),
        effective_execution_policy: projection.effective_execution_policy,
        capability_subset: projection.declared_model_capability_subset,
        credential_transport_mode: projection.credential_transport_mode,
        credential_transport_safe_header_name: projection.credential_transport_safe_header_name,
        credential_configured: entry.credential_configured,
        driver_declared_capabilities: Vec::new(),
        readiness: match entry.readiness {
            intention_storage::ProviderReadinessDto::Ready => ProviderReadinessDto::Ready,
            intention_storage::ProviderReadinessDto::Disabled => ProviderReadinessDto::Disabled,
            intention_storage::ProviderReadinessDto::Unavailable => {
                ProviderReadinessDto::Unavailable
            }
        },
    };
    projected.validate()?;
    Ok(projected)
}

/// The provider catalog removal lifecycle service.
pub struct RemovalService<'a, Catalog, Removal>
where
    Catalog: ProviderCatalogRepositoryDto,
    Removal: ProviderRemovalRepositoryDto,
{
    controller: &'a ProviderCatalogController<Catalog, Removal>,
}

impl<'a, Catalog, Removal> RemovalService<'a, Catalog, Removal>
where
    Catalog: ProviderCatalogRepositoryDto,
    Removal: ProviderRemovalRepositoryDto,
{
    /// Creates a removal service over the catalog runtime controller.
    #[must_use]
    pub const fn new(controller: &'a ProviderCatalogController<Catalog, Removal>) -> Self {
        Self { controller }
    }

    /// Accepts one pending provider catalog removal.
    ///
    /// The controller atomically accepts the removal, writes tombstones and
    /// the ordered audit, and activates the exact registry. Acceptance is the
    /// only provider state change allowed while degraded besides rejection.
    ///
    /// # Errors
    ///
    /// Returns the typed command validation or controller error.
    pub fn accept(
        &self,
        command: AcceptProviderCatalogRemovalCommandDto,
        now: u64,
    ) -> DtoResult<AcceptProviderCatalogRemovalAcceptedDto> {
        command.validate()?;
        let outcome = self.controller.accept_pending(
            command.candidate_handle.clone(),
            command.expected_active_catalog_revision_id.clone(),
            command.expected_candidate_catalog_revision_id.clone(),
            command.operation_id.clone(),
            now,
        )?;
        Ok(AcceptProviderCatalogRemovalAcceptedDto {
            candidate_handle: command.candidate_handle,
            active_catalog_revision_id: outcome.catalog_revision_id.to_string(),
        })
    }

    /// Rejects one pending provider catalog removal candidate.
    ///
    /// The controller drops the candidate, records the rejection audit, and
    /// degrades the catalog to read-only with `removal_candidate_rejected`.
    ///
    /// # Errors
    ///
    /// Returns the typed command validation or controller error.
    pub fn reject(
        &self,
        command: RejectProviderCatalogCandidateCommandDto,
        now: u64,
    ) -> DtoResult<RejectProviderCatalogCandidateAcceptedDto> {
        command.validate()?;
        self.controller.reject_pending(
            command.candidate_handle.clone(),
            command.operation_id.clone(),
            now,
        )?;
        Ok(RejectProviderCatalogCandidateAcceptedDto {
            candidate_handle: command.candidate_handle,
        })
    }

    /// Expires pending removal candidates past their 30-minute lifetime.
    ///
    /// Returns the number expired; expiry degrades the catalog to read-only
    /// with `removal_candidate_expired`.
    ///
    /// # Errors
    ///
    /// Returns the typed controller error.
    pub fn expire(&self, now: u64) -> DtoResult<u64> {
        self.controller.expire_pending(now)
    }
}

/// The held recovered-run admission service.
pub struct HeldRunService<'a> {
    held: &'a dyn HeldRunRepositoryDto,
    selections: &'a dyn ProviderSelectionRepositoryDto,
    readiness: &'a dyn ControlPlaneReadinessPort,
    admission: &'a dyn CatalogAdmissionPort,
}

impl<'a> HeldRunService<'a> {
    /// Creates a held-run service over the durable held-run repository, the
    /// resolved-selection repository, the control-plane readiness port, and
    /// the catalog admission port.
    #[must_use]
    pub const fn new(
        held: &'a dyn HeldRunRepositoryDto,
        selections: &'a dyn ProviderSelectionRepositoryDto,
        readiness: &'a dyn ControlPlaneReadinessPort,
        admission: &'a dyn CatalogAdmissionPort,
    ) -> Self {
        Self {
            held,
            selections,
            readiness,
            admission,
        }
    }

    /// Admits one held recovered run back into its session.
    ///
    /// Admission verifies the exact session/run identity and operation
    /// idempotency, requires the active catalog readiness, verifies the run's
    /// persisted immutable provider selection against the active catalog
    /// (complete selection, exact registry key admitted, enabled, ready, not
    /// tombstoned, and driver-compatible), commits the admission through the
    /// durable held-run repository, and dispatches the supplied schedule
    /// exactly once after the commit. A repeat of the same operation returns
    /// the same acceptance without dispatching a second time. A run without a
    /// persisted selection (for example a legacy M4 run) or any failed
    /// verification leaves the run held with the closed
    /// `held_run_admission_verification_failed` error and never dispatches.
    ///
    /// # Errors
    ///
    /// Returns `execution_not_ready` while degraded,
    /// `held_run_admission_verification_failed` when the persisted selection
    /// cannot be verified, or the typed storage, identity, or dispatch error.
    pub fn admit<Dispatch>(
        &self,
        command: AdmitRecoveredRunCommandDto,
        schedule: ScheduleModelRunDto,
        dispatch: &Dispatch,
        now: u64,
    ) -> DtoResult<AdmitRecoveredRunAcceptedDto>
    where
        Dispatch: ModelRunDispatchPort,
    {
        command.validate()?;
        let session_id = SessionId::parse(&command.session_id)?;
        let run_id = RunId::parse(&command.run_id)?;
        DegradedModeService.assert_execution_ready(&self.readiness.readiness()?)?;
        let held = self.held.load_held_recovered_run(run_id)?.ok_or_else(|| {
            ErrorDto::unavailable(
                "provider_admission_not_found",
                "the recovered run is not held",
            )
        })?;
        if held.session_id != session_id {
            return Err(ErrorDto::validation(
                "recovered_run_admission_invalid",
                "the held run belongs to a different session",
            ));
        }
        match held.admission_state {
            HeldRunAdmissionStateDto::Admitted => {
                if held.admission_operation_id.as_deref() == Some(&command.operation_id) {
                    return Ok(AdmitRecoveredRunAcceptedDto {
                        session_id: command.session_id,
                        run_id: command.run_id,
                    });
                }
                return Err(ErrorDto::new(
                    "provider_admission_not_found",
                    ErrorCategoryDto::Conflict,
                    "the recovered run is already admitted",
                    ErrorRetryDto::Manual,
                    None,
                )?);
            }
            HeldRunAdmissionStateDto::Rejected => {
                return Err(ErrorDto::unavailable(
                    "provider_admission_not_found",
                    "the recovered run admission was rejected",
                ));
            }
            HeldRunAdmissionStateDto::Held => {}
        }
        self.verify_admission(session_id, run_id)?;
        self.held
            .admit_held_recovered_run(AdmitHeldRecoveredRunInputDto {
                run_id,
                session_id,
                admitted_at: i64_time(now),
                operation_id: command.operation_id.clone(),
            })?;
        if schedule.session_id() != session_id || schedule.run_id() != run_id {
            return Err(ErrorDto::validation(
                "recovered_run_admission_invalid",
                "the admission schedule must match the held run identity",
            ));
        }
        dispatch.dispatch_model_run(schedule)?;
        Ok(AdmitRecoveredRunAcceptedDto {
            session_id: command.session_id,
            run_id: command.run_id,
        })
    }

    /// Verifies the held run's persisted immutable provider selection.
    ///
    /// Admission requires a complete immutable selection: the run must have a
    /// persisted selection whose fields validate, whose exact registry key is
    /// admitted by the active catalog (enabled, ready, not tombstoned), and
    /// whose driver contract is the admitted one. Any failed verification
    /// leaves the run held and returns the closed
    /// `held_run_admission_verification_failed` error.
    ///
    /// # Errors
    ///
    /// Returns the closed `held_run_admission_verification_failed` error on
    /// any failed verification.
    fn verify_admission(&self, session_id: SessionId, run_id: RunId) -> DtoResult<()> {
        let selection = self
            .selections
            .load_resolved_run_provider_selection(session_id, run_id)
            .map_err(|_| held_run_admission_verification_failed())?;
        let Some(selection) = selection else {
            return Err(held_run_admission_verification_failed());
        };
        if selection.validate().is_err()
            || self
                .admission
                .verify_registry_key(
                    &selection.profile_id,
                    &selection.provider_profile_revision_id,
                    &selection.kind_descriptor_revision_id,
                    &selection.provider_driver_contract_revision,
                )
                .is_err()
        {
            return Err(held_run_admission_verification_failed());
        }
        Ok(())
    }
}

/// The closed error returned when a held run's persisted provider selection
/// fails admission verification.
fn held_run_admission_verification_failed() -> ErrorDto {
    ErrorDto::new(
        "held_run_admission_verification_failed",
        ErrorCategoryDto::Conflict,
        "the held run's provider selection failed admission verification",
        ErrorRetryDto::Manual,
        None,
    )
    .unwrap_or_else(|_| {
        ErrorDto::unavailable(
            "held_run_admission_verification_failed",
            "the held run's provider selection failed admission verification",
        )
    })
}

/// Converts one whole-second timestamp to the storage `i64` representation.
fn i64_time(now: u64) -> i64 {
    i64::try_from(now).unwrap_or(i64::MAX)
}
