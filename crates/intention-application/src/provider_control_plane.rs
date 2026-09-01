//! Zone 4 control-plane runtime services.
//!
//! This module implements the control-plane runtime: controlled live reload
//! (prepare and commit), provider credential rotation, non-authorizing
//! provider health checks, additive provider model discovery, and the
//! code-owned pricing policy projection. Every public boundary is DTO-only
//! and credential-free: private credential material exists only inside
//! [`PrivateCredentialMaterial`], which deliberately implements no `Debug`,
//! `Display`, or serde traits and never crosses a DTO, log, or error boundary.
//!
//! The services never create a `RunId`, reason, lifecycle event, scheduler
//! candidate, or routing/selection decision: health evidence and discovery
//! records are observations, pricing projections are non-authorizing, and
//! rotation replaces private material without ever changing the safe
//! composition.

use intention_config::{
    ConfigSnapshotDto, RawConfigInputDto,
    control_plane::{
        CandidateIssueDto, ConfigCandidateDto, classify_changed_fields, parse_candidate,
        reject_catalog_affecting_edits, semantic_equivalence,
    },
};
use intention_domain::canonical::Digest256;
use intention_protocol::contract_families::{
    CredentialRotationResultDto, PricingClassification, PricingObservationDto,
    PricingProjectionDto, ProviderAvailabilityObservation, ProviderDiscoveryPhase,
    ProviderDiscoveryProjectionDto, ProviderHealthEvidenceDto, ProviderHealthFailureCategory,
    ProviderHealthProjectionDto, ProviderModelDiscoveryRecordDto,
    RotateProviderCredentialsCommandDto,
};
use intention_storage::{CommitConfigurationReloadInputDto, ConfigurationReloadRepositoryDto};
use intention_types::{DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto};

/// The deterministic check-contract revision of zone-4 health evidence.
const HEALTH_CHECK_CONTRACT_REVISION: &str = "health-check-v1";
/// The deterministic failure category mapped from an `Unavailable` probe in
/// this slice: the probe port reports only the availability observation, so
/// the service maps unavailability to the closed service-unavailable
/// category.
const UNAVAILABLE_FAILURE_CATEGORY: ProviderHealthFailureCategory =
    ProviderHealthFailureCategory::ServiceUnavailable;
/// The deterministic diagnostic code of an `Unavailable` health observation.
const UNAVAILABLE_DIAGNOSTIC_CODE: &str = "provider_health_unavailable";
/// The maximum retained pricing observations in one projection.
const MAX_PRICING_OBSERVATIONS: usize = 256;
/// The maximum retained discovery records in one projection.
const MAX_DISCOVERY_RECORDS: usize = 256;
/// The static pricing disclaimer carried by every projection.
const PRICING_DISCLAIMER: &str =
    "pricing observations are non-authorizing; they never gate admission, quotas, or reservations";

/// The credential-free safe composition binding of one provider profile.
///
/// Every field is safe, non-secret data. The binding is the frozen meaning a
/// credential rotation must preserve: when any field departs from the
/// recorded binding, rotation is refused before any replacement occurs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafeCompositionBindingDto {
    /// The bound provider profile identity.
    pub profile_id: String,
    /// The bound provider profile revision identity.
    pub provider_profile_revision_id: String,
    /// The deterministic credential-free composition revision of the binding.
    pub safe_composition_revision: String,
    /// The bound provider kind identity.
    pub kind_id: String,
    /// The bound kind descriptor revision identity.
    pub kind_descriptor_revision_id: String,
    /// The bound model identity.
    pub model_id: String,
    /// The bound non-secret normalized endpoint, when one applies.
    pub endpoint: Option<String>,
    /// The bound declared model capability subset.
    pub declared_model_capability_subset: Vec<String>,
    /// The bound effective execution policy.
    pub effective_execution_policy: String,
    /// The bound loopback policy, or the closed `not-applicable` value.
    pub effective_loopback_policy_or_not_applicable: String,
    /// The bound provider driver contract revision.
    pub provider_driver_contract_revision: String,
}

/// The safe binding source of the active configuration and provider selection.
///
/// The composition root owns the durable active snapshot and the catalog
/// admission bindings; this trait lets the services read them without owning
/// storage or catalog resources. Implementations must be credential-free.
pub trait SafeBindingSource: Send + Sync {
    /// Returns the active configuration revision identity.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the active configuration cannot be read.
    fn active_revision(&self) -> DtoResult<String>;

    /// Returns the active credential-free configuration snapshot.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the active snapshot cannot be read.
    fn active_snapshot(&self) -> DtoResult<ConfigSnapshotDto>;

    /// Returns the safe composition binding of one provider profile.
    ///
    /// # Errors
    ///
    /// Returns a typed error such as `provider_profile_unavailable` when the
    /// profile is unknown, tombstoned, or its binding cannot be read.
    fn binding(&self, profile_id: &str) -> DtoResult<SafeCompositionBindingDto>;
}

/// The credential-free acceptance projection of one reload candidate.
///
/// `accepted` records whether the candidate may be applied; when rejected,
/// `failure_code` carries the first deterministic rejection code. The parsed
/// candidate itself is retained privately for the commit step; raw TOML and
/// credential material never appear.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReloadCandidateDto {
    /// Whether the candidate may be committed.
    pub accepted: bool,
    /// The fresh candidate revision identity, when one was parsed.
    pub candidate_revision_id: Option<String>,
    /// Whether the candidate changes any semantic field relative to the
    /// previous snapshot.
    pub changed_semantics: bool,
    /// The closed categories of changed fields.
    pub changed_field_categories: Vec<String>,
    /// The bounded deterministic validation issues of the candidate.
    pub issues: Vec<CandidateIssueDto>,
    /// Whether more issues exist than the bounded list carries.
    pub truncated_issues: bool,
    /// The complete issue count before bounding.
    pub total_issue_count: u32,
    /// The typed rejection code, when the candidate was not accepted.
    pub failure_code: Option<String>,
    /// The parsed credential-free candidate retained for the commit step.
    candidate: ConfigCandidateDto,
}

impl ReloadCandidateDto {
    /// Returns the parsed credential-free candidate for the commit step.
    #[must_use]
    pub const fn candidate(&self) -> &ConfigCandidateDto {
        &self.candidate
    }
}

/// The durable outcome of one configuration reload commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReloadCommitOutcomeDto {
    /// The caller-owned operation identity, reused as the transaction id.
    pub transaction_id: String,
    /// The active configuration revision before the commit.
    pub previous_revision: String,
    /// The committed configuration revision.
    pub new_revision: String,
    /// The closed categories of fields changed by the commit.
    pub changed_field_categories: Vec<String>,
    /// Whether the reload affects fresh runs only (always true: active runs
    /// keep their immutable per-run snapshot).
    pub fresh_runs_only: bool,
}

/// The controlled live-reload service.
///
/// `prepare` parses and validates a raw TOML candidate through the
/// configuration crate and projects the acceptance outcome; `commit` applies
/// the parsed candidate through the durable reload repository, fail-closed on
/// any storage error so the daemon stays on its recorded snapshot.
pub struct ConfigurationReloadService<'a> {
    repository: &'a dyn ConfigurationReloadRepositoryDto,
    binding: &'a dyn SafeBindingSource,
}

impl<'a> ConfigurationReloadService<'a> {
    /// Creates a reload service over a durable reload repository and the
    /// active-configuration binding source.
    #[must_use]
    pub fn new(
        repository: &'a dyn ConfigurationReloadRepositoryDto,
        binding: &'a dyn SafeBindingSource,
    ) -> Self {
        Self {
            repository,
            binding,
        }
    }

    /// Parses, validates, and projects one reload candidate.
    ///
    /// Raw content larger than the reload bound or carrying credential-shaped
    /// content fails closed with the typed configuration error. A candidate
    /// with validation issues is not accepted and carries the first issue
    /// code; a candidate that changes catalog-affecting fields (provider
    /// kind) is not accepted with `catalog_change_requires_restart`.
    /// `operation_id` is accepted for caller audit continuity and consumed by
    /// [`Self::commit`]; it never appears in the projection.
    ///
    /// # Errors
    ///
    /// Returns `candidate_too_large` or `credentials_forbidden` from the
    /// configuration parse contract.
    pub fn prepare(
        &self,
        raw: RawConfigInputDto,
        previous: &ConfigSnapshotDto,
        operation_id: String,
    ) -> DtoResult<ReloadCandidateDto> {
        let _operation_id = operation_id;
        let candidate = parse_candidate(raw, previous)?;
        let issues = candidate.validation().issues().to_vec();
        let failure_code = if !issues.is_empty() {
            Some(issues[0].code().to_owned())
        } else {
            reject_catalog_affecting_edits(&candidate, previous)
                .err()
                .map(|error| error.code().to_owned())
        };
        let changed_field_categories = classify_changed_fields(candidate.safe_snapshot(), previous);
        Ok(ReloadCandidateDto {
            accepted: failure_code.is_none(),
            candidate_revision_id: Some(candidate.candidate_revision_id().to_owned()),
            changed_semantics: !semantic_equivalence(candidate.safe_snapshot(), previous),
            changed_field_categories,
            issues,
            truncated_issues: candidate.validation().truncated(),
            total_issue_count: candidate.validation().total_issue_count(),
            failure_code,
            candidate,
        })
    }

    /// Durably commits one prepared candidate.
    ///
    /// A stale `expected_active_revision` fails closed with
    /// `config_revision_mismatch` before any write. The commit persists the
    /// candidate snapshot, its audit row, and the active-state update
    /// atomically through the durable reload repository; a storage failure
    /// propagates and the daemon stays on its recorded snapshot. The returned
    /// outcome carries `fresh_runs_only = true`: the reload affects fresh runs
    /// only, because active runs keep their immutable per-run snapshot.
    ///
    /// # Errors
    ///
    /// Returns `config_revision_mismatch` for a stale expected revision, the
    /// binding source's error when the active revision cannot be read, or the
    /// durable repository's typed error when the atomic commit fails.
    pub fn commit(
        &self,
        candidate: ConfigCandidateDto,
        expected_active_revision: Option<String>,
        operation_id: String,
        now: u64,
    ) -> DtoResult<ReloadCommitOutcomeDto> {
        let previous_revision = self.binding.active_revision()?;
        if let Some(expected) = expected_active_revision
            && expected != previous_revision
        {
            return Err(ErrorDto::new(
                "config_revision_mismatch",
                ErrorCategoryDto::Conflict,
                "the active configuration revision does not match the expected revision",
                ErrorRetryDto::Manual,
                None,
            )?);
        }
        let reloaded_at = i64::try_from(now).unwrap_or(i64::MAX);
        self.repository
            .commit_configuration_reload(CommitConfigurationReloadInputDto {
                snapshot: candidate.safe_snapshot().clone(),
                operation_id: operation_id.clone(),
                reloaded_at,
            })?;
        let previous_snapshot = self.binding.active_snapshot()?;
        let changed_field_categories =
            classify_changed_fields(candidate.safe_snapshot(), &previous_snapshot);
        Ok(ReloadCommitOutcomeDto {
            transaction_id: operation_id,
            previous_revision,
            new_revision: candidate.candidate_revision_id().to_owned(),
            changed_field_categories,
            fresh_runs_only: true,
        })
    }
}

/// Opaque private replacement credential material.
///
/// The wrapper deliberately implements no `Debug`, `Display`, or serde
/// traits: it is the private side of the credential port and never crosses a
/// DTO, log, or error boundary. Only composition-owned ports construct and
/// consume it.
pub struct PrivateCredentialMaterial {
    material: Vec<u8>,
}

impl PrivateCredentialMaterial {
    /// Wraps private replacement material obtained through a private channel.
    ///
    /// This constructor is composition-only: the material must never enter a
    /// DTO, log, or error.
    #[must_use]
    pub const fn from_private_bytes(material: Vec<u8>) -> Self {
        Self { material }
    }

    /// Consumes the wrapper and returns the private material bytes.
    ///
    /// This accessor is composition-only: the material must never enter a
    /// DTO, log, or error.
    #[must_use]
    pub fn into_private_bytes(self) -> Vec<u8> {
        self.material
    }
}

/// The private channel through which replacement credential material arrives.
///
/// Implementations supply material out-of-band; the service never requests,
/// carries, or echoes credential text.
pub trait PrivateCredentialPort: Send + Sync {
    /// Obtains the replacement private material for one profile.
    ///
    /// # Errors
    ///
    /// Returns a typed error such as `credential_rotation_source_unavailable`
    /// when no private credential source is configured.
    fn obtain_replacement(&self, profile_id: &str) -> DtoResult<PrivateCredentialMaterial>;
}

/// The composition-owned rebuild boundary for one provider's private driver.
///
/// The service calls this port after replacement material is obtained; the
/// composition swaps the rebuilt driver under the control-plane gate.
pub trait DriverRebuildPort: Send + Sync {
    /// Rebuilds and swaps the private driver of one profile.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the driver cannot be rebuilt or swapped.
    fn rebuild(&self, profile_id: &str, material: PrivateCredentialMaterial) -> DtoResult<()>;
}

/// The provider credential rotation service.
///
/// Rotation verifies the frozen safe composition before any replacement:
/// the command's profile identity, profile revision, and expected composition
/// revision must match the recorded binding, and every safe selected field
/// (kind, model, endpoint, capability subset, execution policy, loopback
/// policy, and profile/descriptor/driver revisions) must be present and
/// unchanged. On any mismatch the rotation is rejected before replacement
/// with `credential_rotation_frozen_meaning_mismatch`. Replacement material
/// arrives through the private credential port and the driver is rebuilt
/// through the composition-owned rebuild port. The service never resumes,
/// retries, reattaches, or replays a rotation.
pub struct CredentialRotationService<'a> {
    binding: &'a dyn SafeBindingSource,
    rebuild: &'a dyn DriverRebuildPort,
}

impl<'a> CredentialRotationService<'a> {
    /// Creates a rotation service over the binding source and rebuild port.
    #[must_use]
    pub fn new(binding: &'a dyn SafeBindingSource, rebuild: &'a dyn DriverRebuildPort) -> Self {
        Self { binding, rebuild }
    }

    /// Rotates one provider's private credential material.
    ///
    /// `_now` is reserved for audit timestamps in a later slice; the rotation
    /// result itself is credential-free.
    ///
    /// # Errors
    ///
    /// Returns the command's validation error, the binding source's error,
    /// `credential_rotation_frozen_meaning_mismatch` when the safe
    /// composition changed or is incomplete, the credential port's error when
    /// no replacement is available, or the rebuild port's error when the
    /// driver swap fails. Every failure happens before or at the replacement
    /// boundary; nothing is retried or resumed.
    pub fn rotate(
        &self,
        command: RotateProviderCredentialsCommandDto,
        port: &impl PrivateCredentialPort,
        _now: u64,
    ) -> DtoResult<CredentialRotationResultDto> {
        command.validate()?;
        let binding = self.binding.binding(&command.profile_id)?;
        let profile_matches = command.profile_id == binding.profile_id
            && command.provider_profile_revision_id == binding.provider_profile_revision_id;
        let composition_matches =
            command.expected_credential_composition_revision == binding.safe_composition_revision;
        let fields_complete = !binding.kind_id.trim().is_empty()
            && !binding.kind_descriptor_revision_id.trim().is_empty()
            && !binding.model_id.trim().is_empty()
            && binding
                .endpoint
                .as_deref()
                .is_none_or(|endpoint| !endpoint.trim().is_empty())
            && !binding.declared_model_capability_subset.is_empty()
            && !binding.effective_execution_policy.trim().is_empty()
            && !binding
                .effective_loopback_policy_or_not_applicable
                .trim()
                .is_empty()
            && !binding.provider_driver_contract_revision.trim().is_empty();
        let meaning_frozen = profile_matches && composition_matches && fields_complete;
        if !meaning_frozen {
            return Err(ErrorDto::new(
                "credential_rotation_frozen_meaning_mismatch",
                ErrorCategoryDto::Policy,
                "the safe provider composition changed or is incomplete; rotation is refused before any replacement",
                ErrorRetryDto::Manual,
                None,
            )?);
        }
        let material = port.obtain_replacement(&command.profile_id)?;
        self.rebuild.rebuild(&command.profile_id, material)?;
        Ok(CredentialRotationResultDto {
            operation_id: command.operation_id,
            profile_id: command.profile_id,
            safe_credential_composition_revision: binding.safe_composition_revision,
            rotated: true,
        })
    }
}

/// The probe boundary of one provider health check.
pub trait HealthProbePort: Send + Sync {
    /// Probes one provider and returns the closed availability observation.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the probe itself cannot complete; the
    /// service maps the error to an `Unknown` observation with a safe
    /// diagnostic code.
    fn probe(&self, provider_id: &str) -> DtoResult<ProviderAvailabilityObservation>;
}

/// The non-authorizing provider health check service.
///
/// The service records what a check observed as typed evidence and never
/// creates a `RunId`, reason, lifecycle event, or scheduler candidate.
/// Restoration of a provider therefore only permits reevaluation; it never
/// routes or admits by itself.
pub struct ProviderHealthService;

impl ProviderHealthService {
    /// Runs one non-authorizing health check and projects its evidence.
    ///
    /// The attempt id is a deterministic hash of the provider identity and
    /// check time; the check-contract revision is `health-check-v1`. An
    /// `Unavailable` probe maps to the closed service-unavailable category
    /// with a safe diagnostic code; a probe error maps to an `Unknown`
    /// observation carrying the probe's typed code.
    ///
    /// # Errors
    ///
    /// Returns `provider_health_invalid` for a blank or over-long provider
    /// identity.
    pub fn check(
        &self,
        provider_id: String,
        port: &impl HealthProbePort,
        now: u64,
    ) -> DtoResult<ProviderHealthProjectionDto> {
        if provider_id.trim().is_empty() || provider_id.chars().count() > 63 {
            return Err(ErrorDto::validation(
                "provider_health_invalid",
                "provider identity is invalid",
            ));
        }
        let health_attempt_id = deterministic_attempt_id(&provider_id, now, "health");
        let (availability, failure_category, safe_diagnostic_code, safe_reason_code) =
            match port.probe(&provider_id) {
                Ok(ProviderAvailabilityObservation::Available) => {
                    (ProviderAvailabilityObservation::Available, None, None, None)
                }
                Ok(ProviderAvailabilityObservation::Unavailable) => (
                    ProviderAvailabilityObservation::Unavailable,
                    Some(UNAVAILABLE_FAILURE_CATEGORY),
                    Some(UNAVAILABLE_DIAGNOSTIC_CODE.to_owned()),
                    Some(UNAVAILABLE_DIAGNOSTIC_CODE.to_owned()),
                ),
                Ok(ProviderAvailabilityObservation::Unknown) => {
                    (ProviderAvailabilityObservation::Unknown, None, None, None)
                }
                Err(error) => (
                    ProviderAvailabilityObservation::Unknown,
                    None,
                    Some(error.code().to_owned()),
                    Some(error.code().to_owned()),
                ),
            };
        let evidence = ProviderHealthEvidenceDto {
            profile_id: provider_id.clone(),
            provider_profile_revision_id: deterministic_profile_revision(&provider_id),
            health_attempt_id,
            check_contract_revision: HEALTH_CHECK_CONTRACT_REVISION.to_owned(),
            observed_availability: availability,
            observed_at: now,
            failure_category,
            safe_diagnostic_code,
        };
        evidence.validate()?;
        Ok(ProviderHealthProjectionDto {
            provider_id,
            observations: vec![evidence],
            safe_reason_code,
            observed_at: now,
        })
    }
}

/// The closed discovery scope of one provider discovery attempt.
///
/// The scope is safe, non-secret data: model identities are data, never
/// routing decisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiscoveryScopeDto {
    /// Discover every model the provider exposes.
    AllModels,
    /// Discover the models of one provider kind.
    Kind {
        /// The provider kind identity.
        kind_id: String,
    },
    /// Discover one model identity.
    Model {
        /// The model identity.
        model_id: String,
    },
}

impl DiscoveryScopeDto {
    /// Returns the deterministic safe scope label for attempt records.
    #[must_use]
    pub fn as_str(&self) -> String {
        match self {
            Self::AllModels => "all".to_owned(),
            Self::Kind { kind_id } => format!("kind:{kind_id}"),
            Self::Model { model_id } => format!("model:{model_id}"),
        }
    }
}

/// The discovery boundary of one provider model discovery attempt.
pub trait DiscoveryPort: Send + Sync {
    /// Discovers the model records of one scope.
    ///
    /// # Errors
    ///
    /// Returns a typed error when discovery cannot complete; the service maps
    /// the error to a terminal projection with a safe status.
    fn discover(
        &self,
        scope: &DiscoveryScopeDto,
    ) -> DtoResult<Vec<ProviderModelDiscoveryRecordDto>>;
}

/// The additive provider model discovery service.
///
/// Discovery records are additive observations about model identities and
/// never route traffic. The attempt lifecycle is closed: an attempt reaches
/// the terminal phase exactly when the discovery port returned or errored.
/// There is no automatic continuation, and the service never touches any
/// selection, routing, or admission state.
pub struct ProviderDiscoveryService;

impl ProviderDiscoveryService {
    /// Starts one discovery attempt and projects its terminal outcome.
    ///
    /// The attempt id is a deterministic hash of the scope label and attempt
    /// time. Records returned by the port are validated and bounded; invalid
    /// records are dropped so the projection stays safe and additive.
    ///
    /// # Errors
    ///
    /// Returns `provider_discovery_invalid` for an over-long or control-bearing
    /// scope label.
    pub fn start(
        &self,
        scope: DiscoveryScopeDto,
        port: &impl DiscoveryPort,
        now: u64,
    ) -> DtoResult<ProviderDiscoveryProjectionDto> {
        let scope_label = scope.as_str();
        if scope_label.chars().count() > 256 || scope_label.chars().any(char::is_control) {
            return Err(ErrorDto::validation(
                "provider_discovery_invalid",
                "discovery scope is invalid",
            ));
        }
        let attempt_id = deterministic_attempt_id(&scope_label, now, "discovery");
        match port.discover(&scope) {
            Ok(records) => {
                let mut kept = Vec::new();
                for record in records.into_iter().take(MAX_DISCOVERY_RECORDS) {
                    if record.validate().is_ok() {
                        kept.push(record);
                    }
                }
                Ok(ProviderDiscoveryProjectionDto {
                    attempt_id: Some(attempt_id),
                    phase: Some(ProviderDiscoveryPhase::Terminal),
                    records: kept,
                    safe_status: Some("completed".to_owned()),
                })
            }
            Err(error) => Ok(ProviderDiscoveryProjectionDto {
                attempt_id: Some(attempt_id),
                phase: Some(ProviderDiscoveryPhase::Terminal),
                records: Vec::new(),
                safe_status: Some(error.code().to_owned()),
            }),
        }
    }

    /// Reports the status of one discovery attempt.
    ///
    /// Attempt state is not persisted in this slice, so status returns the
    /// closed unavailable-state projection with a safe status and never
    /// re-runs or continues the attempt.
    ///
    /// # Errors
    ///
    /// Returns `provider_discovery_invalid` for a blank, over-long, or
    /// control-bearing attempt reference.
    pub fn status(
        &self,
        attempt_id: String,
        _port: &impl DiscoveryPort,
        _now: u64,
    ) -> DtoResult<ProviderDiscoveryProjectionDto> {
        if attempt_id.trim().is_empty()
            || attempt_id.chars().count() > 256
            || attempt_id.chars().any(char::is_control)
        {
            return Err(ErrorDto::validation(
                "provider_discovery_invalid",
                "discovery attempt reference is invalid",
            ));
        }
        Ok(ProviderDiscoveryProjectionDto {
            attempt_id: Some(attempt_id),
            phase: None,
            records: Vec::new(),
            safe_status: Some("attempt_state_unavailable".to_owned()),
        })
    }
}

/// The code-owned pricing policy service.
///
/// Classification is deterministic and owned by this code, never by the
/// observation payload: a zero bounded value is an intrinsic representation
/// bound, a value below the 1,000,000 capacity ceiling is a capacity
/// observation, and any larger value follows the provider's published product
/// policy. The service never gates Mandate admission, tool admission, or
/// scheduler eligibility; it has no such dependency.
pub struct PricingPolicyService;

impl PricingPolicyService {
    /// Classifies one pricing observation with the code-owned rule.
    #[must_use]
    pub const fn classify(&self, value: &PricingObservationDto) -> PricingClassification {
        match value.bounded_numeric_value {
            0 => PricingClassification::IntrinsicRepresentationBound,
            1..=999_999 => PricingClassification::CapacityObservation,
            _ => PricingClassification::ProductPolicy,
        }
    }

    /// Projects a bounded, safe, non-authorizing pricing policy.
    ///
    /// Observations are validated, bounded at 256 entries, and re-classified
    /// with the code-owned rule; invalid observations are dropped. The
    /// projection always carries the static disclaimer and never acts as an
    /// admission ceiling, quota, or reservation.
    #[must_use]
    pub fn project(&self, observations: Vec<PricingObservationDto>) -> PricingProjectionDto {
        let mut bounded = Vec::with_capacity(observations.len().min(MAX_PRICING_OBSERVATIONS));
        for mut observation in observations.into_iter().take(MAX_PRICING_OBSERVATIONS) {
            if observation.validate().is_ok() {
                observation.classification = self.classify(&observation);
                bounded.push(observation);
            }
        }
        let policy_classification = bounded.last().map(|observation| observation.classification);
        PricingProjectionDto {
            observations: bounded,
            policy_classification,
            disclaimer: Some(PRICING_DISCLAIMER.to_owned()),
        }
    }
}

/// Computes the deterministic attempt identity for one provider/scope and time.
///
/// The identity is a lowercase hex SHA-256 digest of the safe label and the
/// whole-second time, prefixed with the attempt family.
#[must_use]
fn deterministic_attempt_id(label: &str, now: u64, family: &str) -> String {
    let digest = Digest256::sha256(format!("{label}|{now}").as_bytes()).bytes();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("{family}-{hex}")
}

/// Computes the deterministic profile-revision placeholder for health evidence.
///
/// Profile revision identity is owned by the catalog runtime; until the
/// catalog is wired into the health path, the evidence records a deterministic
/// placeholder derived from the provider identity so the evidence DTO remains
/// complete and stable across checks.
#[must_use]
fn deterministic_profile_revision(provider_id: &str) -> String {
    let digest = Digest256::sha256(provider_id.as_bytes()).bytes();
    let mut hex = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("health-profile-{hex}")
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use std::cell::RefCell;

    use intention_config::{ConfigPathDto, ConfigSourceDto};
    use intention_protocol::contract_families::{
        ConfigurationOriginDto, PricingObservationDto, ReloadConfigurationCommandDto,
        ReloadTransactionDto, RotateProviderCredentialsCommandDto,
    };
    use intention_storage::CommitConfigurationReloadInputDto;
    use intention_types::{ConfigRevisionId, SchemaVersionDto, TimestampDto};

    use super::*;

    const FAKE_SECRET: &str = "sk-zone4-fake-secret-sweep";

    fn explicit_source() -> ConfigSourceDto {
        ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("intention-application-zone4-unit.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("fixture path is absolute"),
        )
    }

    fn raw(kind: &str, model: &str) -> String {
        format!(
            "schema_version = 1\n[provider]\nkind = \"{kind}\"\nmodel = \"{model}\"\ncredential = \"fake-plain\"\nendpoint = \"https://api.example.invalid/v1\"\n"
        )
    }

    fn time() -> TimestampDto {
        TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid")
    }

    fn snapshot(kind: &str, model: &str, revision: ConfigRevisionId) -> ConfigSnapshotDto {
        let resolved = intention_config::ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            raw(kind, model),
            explicit_source(),
        ))
        .expect("fixture config resolves");
        ConfigSnapshotDto::new(SchemaVersionDto::new(1, 0), revision, time(), resolved)
            .expect("fixture snapshot is valid")
    }

    struct FakeReloadRepository {
        committed: RefCell<Vec<CommitConfigurationReloadInputDto>>,
        fail: bool,
    }

    impl FakeReloadRepository {
        fn new() -> Self {
            Self {
                committed: RefCell::new(Vec::new()),
                fail: false,
            }
        }
    }

    impl ConfigurationReloadRepositoryDto for FakeReloadRepository {
        fn commit_configuration_reload(
            &self,
            input: CommitConfigurationReloadInputDto,
        ) -> DtoResult<()> {
            if self.fail {
                return Err(ErrorDto::unavailable(
                    "reload_storage_unavailable",
                    "fixture storage failure",
                ));
            }
            self.committed.borrow_mut().push(input);
            Ok(())
        }
    }

    struct FakeBindingSource {
        revision: String,
        snapshot: ConfigSnapshotDto,
        binding: SafeCompositionBindingDto,
    }

    impl FakeBindingSource {
        fn new(revision: &str, snapshot: ConfigSnapshotDto) -> Self {
            let binding = SafeCompositionBindingDto {
                profile_id: "default".to_owned(),
                provider_profile_revision_id: "profile-rev-1".to_owned(),
                safe_composition_revision: "composition-rev-1".to_owned(),
                kind_id: "openrouter".to_owned(),
                kind_descriptor_revision_id: "kind-rev-1".to_owned(),
                model_id: "model-a".to_owned(),
                endpoint: Some("https://api.example.invalid/v1".to_owned()),
                declared_model_capability_subset: vec!["text_input".to_owned()],
                effective_execution_policy: "execution-timeout-30-attempts-2".to_owned(),
                effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
                provider_driver_contract_revision: "driver-1.0".to_owned(),
            };
            Self {
                revision: revision.to_owned(),
                snapshot,
                binding,
            }
        }
    }

    impl SafeBindingSource for FakeBindingSource {
        fn active_revision(&self) -> DtoResult<String> {
            Ok(self.revision.clone())
        }

        fn active_snapshot(&self) -> DtoResult<ConfigSnapshotDto> {
            Ok(self.snapshot.clone())
        }

        fn binding(&self, profile_id: &str) -> DtoResult<SafeCompositionBindingDto> {
            if profile_id == self.binding.profile_id {
                Ok(self.binding.clone())
            } else {
                Err(ErrorDto::unavailable(
                    "provider_profile_unavailable",
                    "fixture profile is unavailable",
                ))
            }
        }
    }

    struct FakeCredentialPort {
        material: Option<Vec<u8>>,
    }

    impl PrivateCredentialPort for FakeCredentialPort {
        fn obtain_replacement(&self, _profile_id: &str) -> DtoResult<PrivateCredentialMaterial> {
            self.material
                .clone()
                .map(PrivateCredentialMaterial::from_private_bytes)
                .ok_or_else(|| {
                    ErrorDto::unavailable(
                        "credential_rotation_source_unavailable",
                        "fixture credential source is unavailable",
                    )
                })
        }
    }

    struct FakeRebuildPort {
        rebuilt: std::sync::Mutex<Vec<String>>,
        fail: bool,
    }

    impl FakeRebuildPort {
        fn new() -> Self {
            Self {
                rebuilt: std::sync::Mutex::new(Vec::new()),
                fail: false,
            }
        }

        fn rebuilt(&self) -> Vec<String> {
            self.rebuilt
                .lock()
                .expect("fixture lock is not poisoned")
                .clone()
        }
    }

    impl DriverRebuildPort for FakeRebuildPort {
        fn rebuild(&self, profile_id: &str, _material: PrivateCredentialMaterial) -> DtoResult<()> {
            if self.fail {
                return Err(ErrorDto::unavailable(
                    "credential_rotation_source_unavailable",
                    "fixture rebuild failure",
                ));
            }
            self.rebuilt
                .lock()
                .expect("fixture lock is not poisoned")
                .push(profile_id.to_owned());
            Ok(())
        }
    }

    struct FakeHealthProbe {
        outcome: DtoResult<ProviderAvailabilityObservation>,
    }

    impl HealthProbePort for FakeHealthProbe {
        fn probe(&self, _provider_id: &str) -> DtoResult<ProviderAvailabilityObservation> {
            self.outcome.clone()
        }
    }

    struct FakeDiscoveryPort {
        outcome: DtoResult<Vec<ProviderModelDiscoveryRecordDto>>,
    }

    impl DiscoveryPort for FakeDiscoveryPort {
        fn discover(
            &self,
            _scope: &DiscoveryScopeDto,
        ) -> DtoResult<Vec<ProviderModelDiscoveryRecordDto>> {
            self.outcome.clone()
        }
    }

    fn discovery_record(model_id: &str) -> ProviderModelDiscoveryRecordDto {
        ProviderModelDiscoveryRecordDto {
            discovery_scope: "all".to_owned(),
            model_id: model_id.to_owned(),
            capability_records: vec!["text_input".to_owned()],
            source_attempt_id: "attempt-1".to_owned(),
            discovered_at: 1,
        }
    }

    #[test]
    fn prepare_accepts_valid_candidate_and_rejects_catalog_affecting_change() {
        let previous = snapshot("openrouter", "model-a", ConfigRevisionId::new());
        let repository = FakeReloadRepository::new();
        let binding = FakeBindingSource::new("revision-1", previous.clone());
        let service = ConfigurationReloadService::new(&repository, &binding);
        let candidate = service
            .prepare(
                RawConfigInputDto::new(raw("openrouter", "model-b"), explicit_source()),
                &previous,
                "operation-1".to_owned(),
            )
            .expect("valid candidate prepares");
        assert!(candidate.accepted);
        assert!(candidate.changed_semantics);
        assert!(candidate.failure_code.is_none());
        assert!(candidate.issues.is_empty());
        assert!(
            candidate
                .changed_field_categories
                .iter()
                .any(|category| category == "model")
        );

        let catalog_change = service
            .prepare(
                RawConfigInputDto::new(
                    raw("generic-chat-completion-api", "model-b"),
                    explicit_source(),
                ),
                &previous,
                "operation-2".to_owned(),
            )
            .expect("candidate parses");
        assert!(!catalog_change.accepted);
        assert_eq!(
            catalog_change.failure_code.as_deref(),
            Some("catalog_change_requires_restart")
        );
    }

    #[test]
    fn prepare_fails_closed_on_credential_shaped_candidate() {
        let previous = snapshot("openrouter", "model-a", ConfigRevisionId::new());
        let repository = FakeReloadRepository::new();
        let binding = FakeBindingSource::new("revision-1", previous.clone());
        let service = ConfigurationReloadService::new(&repository, &binding);
        // A credential-shaped value outside the legitimate credential key
        // (the provider model here) must fail closed with the typed code.
        let poisoned = raw("openrouter", "model-b").replace("model-b", FAKE_SECRET);
        let error = service
            .prepare(
                RawConfigInputDto::new(poisoned, explicit_source()),
                &previous,
                "operation-1".to_owned(),
            )
            .expect_err("credential-shaped candidate fails closed");
        assert_eq!(error.code(), "credentials_forbidden");
        assert!(!error.to_string().contains(FAKE_SECRET));
    }

    #[test]
    fn prepare_reports_validation_issues_with_the_first_failure_code() {
        let previous = snapshot("openrouter", "model-a", ConfigRevisionId::new());
        let repository = FakeReloadRepository::new();
        let binding = FakeBindingSource::new("revision-1", previous.clone());
        let service = ConfigurationReloadService::new(&repository, &binding);
        let invalid = "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"\"\ncredential = \"fake-plain\"\n";
        let candidate = service
            .prepare(
                RawConfigInputDto::new(invalid, explicit_source()),
                &previous,
                "operation-1".to_owned(),
            )
            .expect("invalid candidate still projects");
        assert!(!candidate.accepted);
        assert_eq!(
            candidate.failure_code.as_deref(),
            Some("invalid_provider_model")
        );
        assert!(!candidate.issues.is_empty());
        assert!(!format!("{candidate:?}").contains(FAKE_SECRET));
    }

    #[test]
    fn commit_persists_atomically_and_reports_fresh_runs_only() {
        let previous = snapshot("openrouter", "model-a", ConfigRevisionId::new());
        let repository = FakeReloadRepository::new();
        let binding = FakeBindingSource::new("revision-1", previous.clone());
        let service = ConfigurationReloadService::new(&repository, &binding);
        let candidate = service
            .prepare(
                RawConfigInputDto::new(raw("openrouter", "model-b"), explicit_source()),
                &previous,
                "operation-1".to_owned(),
            )
            .expect("candidate prepares");
        let outcome = service
            .commit(
                candidate.candidate().clone(),
                Some("revision-1".to_owned()),
                "operation-1".to_owned(),
                100,
            )
            .expect("commit succeeds");
        assert_eq!(outcome.transaction_id, "operation-1");
        assert_eq!(outcome.previous_revision, "revision-1");
        assert_eq!(
            outcome.new_revision,
            candidate
                .candidate_revision_id
                .as_deref()
                .unwrap_or_default()
        );
        assert!(outcome.fresh_runs_only);
        let committed = repository.committed.borrow();
        assert_eq!(committed.len(), 1);
        assert_eq!(committed[0].operation_id, "operation-1");
        assert_eq!(committed[0].reloaded_at, 100);
        assert_eq!(
            committed[0].snapshot.resolved().provider().model(),
            "model-b"
        );
    }

    #[test]
    fn commit_rejects_stale_expected_revision_before_any_write() {
        let previous = snapshot("openrouter", "model-a", ConfigRevisionId::new());
        let repository = FakeReloadRepository::new();
        let binding = FakeBindingSource::new("revision-1", previous.clone());
        let service = ConfigurationReloadService::new(&repository, &binding);
        let candidate = service
            .prepare(
                RawConfigInputDto::new(raw("openrouter", "model-b"), explicit_source()),
                &previous,
                "operation-1".to_owned(),
            )
            .expect("candidate prepares");
        let error = service
            .commit(
                candidate.candidate().clone(),
                Some("revision-stale".to_owned()),
                "operation-1".to_owned(),
                100,
            )
            .expect_err("stale expected revision is rejected");
        assert_eq!(error.code(), "config_revision_mismatch");
        assert!(repository.committed.borrow().is_empty());
    }

    #[test]
    fn commit_fails_closed_on_storage_error_and_daemon_stays_on_recorded_snapshot() {
        let previous = snapshot("openrouter", "model-a", ConfigRevisionId::new());
        let repository = FakeReloadRepository {
            fail: true,
            ..FakeReloadRepository::new()
        };
        let binding = FakeBindingSource::new("revision-1", previous.clone());
        let service = ConfigurationReloadService::new(&repository, &binding);
        let candidate = service
            .prepare(
                RawConfigInputDto::new(raw("openrouter", "model-b"), explicit_source()),
                &previous,
                "operation-1".to_owned(),
            )
            .expect("candidate prepares");
        let error = service
            .commit(
                candidate.candidate().clone(),
                Some("revision-1".to_owned()),
                "operation-1".to_owned(),
                100,
            )
            .expect_err("storage failure propagates");
        assert_eq!(error.code(), "reload_storage_unavailable");
        assert!(repository.committed.borrow().is_empty());
    }

    #[test]
    fn rotation_success_never_exposes_the_fake_secret() {
        let previous = snapshot("openrouter", "model-a", ConfigRevisionId::new());
        let binding = FakeBindingSource::new("revision-1", previous);
        let rebuild = FakeRebuildPort::new();
        let service = CredentialRotationService::new(&binding, &rebuild);
        let port = FakeCredentialPort {
            material: Some(FAKE_SECRET.as_bytes().to_vec()),
        };
        let command = RotateProviderCredentialsCommandDto {
            profile_id: "default".to_owned(),
            provider_profile_revision_id: "profile-rev-1".to_owned(),
            expected_credential_composition_revision: "composition-rev-1".to_owned(),
            operation_id: "operation-1".to_owned(),
        };
        let result = service
            .rotate(command, &port, 100)
            .expect("rotation succeeds");
        assert!(result.rotated);
        assert_eq!(result.profile_id, "default");
        assert_eq!(
            result.safe_credential_composition_revision,
            "composition-rev-1"
        );
        assert_eq!(rebuild.rebuilt(), ["default"]);
        assert!(!format!("{result:?}").contains(FAKE_SECRET));
    }

    #[test]
    fn rotation_mismatch_rejects_before_replacement() {
        let previous = snapshot("openrouter", "model-a", ConfigRevisionId::new());
        let binding = FakeBindingSource::new("revision-1", previous);
        let rebuild = FakeRebuildPort::new();
        let service = CredentialRotationService::new(&binding, &rebuild);
        let port = FakeCredentialPort {
            material: Some(FAKE_SECRET.as_bytes().to_vec()),
        };
        for (revision, expected_code) in [
            (
                "composition-stale".to_owned(),
                "credential_rotation_frozen_meaning_mismatch",
            ),
            (
                "composition-rev-1".to_owned(),
                "provider_profile_unavailable",
            ),
        ] {
            let command = RotateProviderCredentialsCommandDto {
                profile_id: if expected_code == "provider_profile_unavailable" {
                    "unknown-profile".to_owned()
                } else {
                    "default".to_owned()
                },
                provider_profile_revision_id: "profile-rev-1".to_owned(),
                expected_credential_composition_revision: revision,
                operation_id: "operation-1".to_owned(),
            };
            let error = service
                .rotate(command, &port, 100)
                .expect_err("mismatch is rejected");
            assert_eq!(error.code(), expected_code);
            assert!(!error.to_string().contains(FAKE_SECRET));
            assert!(rebuild.rebuilt().is_empty());
        }
    }

    #[test]
    fn rotation_obtains_replacement_only_after_the_meaning_is_frozen() {
        let previous = snapshot("openrouter", "model-a", ConfigRevisionId::new());
        let binding = FakeBindingSource::new("revision-1", previous);
        let rebuild = FakeRebuildPort::new();
        let service = CredentialRotationService::new(&binding, &rebuild);
        let port = FakeCredentialPort {
            material: Some(FAKE_SECRET.as_bytes().to_vec()),
        };
        let command = RotateProviderCredentialsCommandDto {
            profile_id: "default".to_owned(),
            provider_profile_revision_id: "profile-rev-1".to_owned(),
            expected_credential_composition_revision: "composition-stale".to_owned(),
            operation_id: "operation-1".to_owned(),
        };
        let error = service
            .rotate(command, &port, 100)
            .expect_err("stale composition is rejected before replacement");
        assert_eq!(error.code(), "credential_rotation_frozen_meaning_mismatch");
        assert!(rebuild.rebuilt().is_empty());
    }

    #[test]
    fn rotation_without_configured_credential_source_fails_closed() {
        let previous = snapshot("openrouter", "model-a", ConfigRevisionId::new());
        let binding = FakeBindingSource::new("revision-1", previous);
        let rebuild = FakeRebuildPort::new();
        let service = CredentialRotationService::new(&binding, &rebuild);
        let port = FakeCredentialPort { material: None };
        let command = RotateProviderCredentialsCommandDto {
            profile_id: "default".to_owned(),
            provider_profile_revision_id: "profile-rev-1".to_owned(),
            expected_credential_composition_revision: "composition-rev-1".to_owned(),
            operation_id: "operation-1".to_owned(),
        };
        let error = service
            .rotate(command, &port, 100)
            .expect_err("missing credential source fails closed");
        assert_eq!(error.code(), "credential_rotation_source_unavailable");
        assert!(rebuild.rebuilt().is_empty());
    }

    #[test]
    fn health_check_projects_typed_evidence_and_never_authorizes() {
        let service = ProviderHealthService;
        let projection = service
            .check(
                "default".to_owned(),
                &FakeHealthProbe {
                    outcome: Ok(ProviderAvailabilityObservation::Available),
                },
                100,
            )
            .expect("health check succeeds");
        assert_eq!(projection.provider_id, "default");
        assert_eq!(projection.observations.len(), 1);
        let evidence = &projection.observations[0];
        assert_eq!(evidence.check_contract_revision, "health-check-v1");
        assert_eq!(
            evidence.observed_availability,
            ProviderAvailabilityObservation::Available
        );
        assert!(evidence.failure_category.is_none());
        assert!(evidence.safe_diagnostic_code.is_none());
        assert!(projection.safe_reason_code.is_none());
        assert!(evidence.health_attempt_id.starts_with("health-"));
        projection.validate().expect("projection is valid");
    }

    #[test]
    fn health_check_maps_unavailable_and_probe_errors_to_closed_evidence() {
        let service = ProviderHealthService;
        let unavailable = service
            .check(
                "default".to_owned(),
                &FakeHealthProbe {
                    outcome: Ok(ProviderAvailabilityObservation::Unavailable),
                },
                100,
            )
            .expect("unavailable check projects");
        assert_eq!(
            unavailable.observations[0].observed_availability,
            ProviderAvailabilityObservation::Unavailable
        );
        assert_eq!(
            unavailable.observations[0].failure_category,
            Some(ProviderHealthFailureCategory::ServiceUnavailable)
        );
        assert_eq!(
            unavailable.safe_reason_code.as_deref(),
            Some("provider_health_unavailable")
        );
        unavailable.validate().expect("projection is valid");

        let errored = service
            .check(
                "default".to_owned(),
                &FakeHealthProbe {
                    outcome: Err(ErrorDto::unavailable(
                        "provider_probe_unavailable",
                        "fixture probe failure",
                    )),
                },
                100,
            )
            .expect("errored probe projects");
        assert_eq!(
            errored.observations[0].observed_availability,
            ProviderAvailabilityObservation::Unknown
        );
        assert_eq!(
            errored.observations[0].safe_diagnostic_code.as_deref(),
            Some("provider_probe_unavailable")
        );
        errored.validate().expect("projection is valid");
    }

    #[test]
    fn health_check_rejects_invalid_provider_identity() {
        let service = ProviderHealthService;
        let error = service
            .check(
                " ".to_owned(),
                &FakeHealthProbe {
                    outcome: Ok(ProviderAvailabilityObservation::Available),
                },
                100,
            )
            .expect_err("blank provider identity is rejected");
        assert_eq!(error.code(), "provider_health_invalid");
    }

    #[test]
    fn discovery_start_projects_terminal_records_and_status_never_continues() {
        let service = ProviderDiscoveryService;
        let records = vec![
            discovery_record("gpt-4o"),
            discovery_record("o3"),
            discovery_record("codex-1"),
        ];
        let projection = service
            .start(
                DiscoveryScopeDto::AllModels,
                &FakeDiscoveryPort {
                    outcome: Ok(records),
                },
                100,
            )
            .expect("discovery starts");
        assert_eq!(projection.phase, Some(ProviderDiscoveryPhase::Terminal));
        assert_eq!(projection.records.len(), 3);
        assert_eq!(projection.records[0].model_id, "gpt-4o");
        assert_eq!(projection.records[1].model_id, "o3");
        assert_eq!(projection.records[2].model_id, "codex-1");
        projection.validate().expect("projection is valid");

        let status = service
            .status(
                projection.attempt_id.expect("attempt id is present"),
                &FakeDiscoveryPort {
                    outcome: Ok(vec![discovery_record("gpt-5")]),
                },
                200,
            )
            .expect("status projects");
        assert_eq!(status.phase, None);
        assert!(status.records.is_empty());
        assert_eq!(
            status.safe_status.as_deref(),
            Some("attempt_state_unavailable")
        );
    }

    #[test]
    fn discovery_errored_attempt_is_terminal_with_safe_status() {
        let service = ProviderDiscoveryService;
        let projection = service
            .start(
                DiscoveryScopeDto::Model {
                    model_id: "gpt-4o".to_owned(),
                },
                &FakeDiscoveryPort {
                    outcome: Err(ErrorDto::unavailable(
                        "provider_discovery_unavailable",
                        "fixture discovery failure",
                    )),
                },
                100,
            )
            .expect("errored discovery projects");
        assert_eq!(projection.phase, Some(ProviderDiscoveryPhase::Terminal));
        assert!(projection.records.is_empty());
        assert_eq!(
            projection.safe_status.as_deref(),
            Some("provider_discovery_unavailable")
        );
        projection.validate().expect("projection is valid");
    }

    #[test]
    fn pricing_projection_is_code_owned_and_non_authorizing() {
        let service = PricingPolicyService;
        let observation = |value: u64| PricingObservationDto {
            provider_kind_id: "openrouter".to_owned(),
            model_id: "model-a".to_owned(),
            bounded_numeric_value: value,
            classification: PricingClassification::ProductPolicy,
            observed_at: 1,
        };
        assert_eq!(
            service.classify(&observation(0)),
            PricingClassification::IntrinsicRepresentationBound
        );
        assert_eq!(
            service.classify(&observation(42)),
            PricingClassification::CapacityObservation
        );
        assert_eq!(
            service.classify(&observation(2_000_000)),
            PricingClassification::ProductPolicy
        );
        let projection = service.project(vec![observation(0), observation(42)]);
        assert_eq!(projection.observations.len(), 2);
        assert_eq!(
            projection.observations[0].classification,
            PricingClassification::IntrinsicRepresentationBound
        );
        assert_eq!(
            projection.observations[1].classification,
            PricingClassification::CapacityObservation
        );
        assert_eq!(
            projection.policy_classification,
            Some(PricingClassification::CapacityObservation)
        );
        assert!(projection.disclaimer.is_some());
        projection.validate().expect("projection is valid");
    }

    #[test]
    fn reload_transaction_dto_never_carries_private_material() {
        let transaction = ReloadTransactionDto {
            transaction_id: "operation-1".to_owned(),
            previous_config_revision: "revision-1".to_owned(),
            candidate_config_revision: "revision-2".to_owned(),
            validation_result:
                intention_protocol::contract_families::ConfigurationValidationOutcomeDto::Valid,
            migration_result: "not-applicable".to_owned(),
            commit_outcome:
                intention_protocol::contract_families::ConfigurationCommitOutcomeDto::Committed,
            safe_failure_code: None,
            safe_failure_detail: None,
        };
        assert!(!format!("{transaction:?}").contains(FAKE_SECRET));
    }

    #[test]
    fn control_plane_dtos_validate_fail_closed_on_credential_shaped_values() {
        let command = ReloadConfigurationCommandDto {
            candidate_snapshot_reference: Some(FAKE_SECRET.to_owned()),
            candidate_edit_reference: None,
            expected_active_config_revision: "revision-1".to_owned(),
            operation_id: "operation-1".to_owned(),
            origin: ConfigurationOriginDto::Admin,
        };
        assert_eq!(
            command
                .validate()
                .expect_err("credential-shaped reference is rejected")
                .code(),
            "credentials_forbidden"
        );
        let rotation = RotateProviderCredentialsCommandDto {
            profile_id: "default".to_owned(),
            provider_profile_revision_id: "profile-rev-1".to_owned(),
            expected_credential_composition_revision: FAKE_SECRET.to_owned(),
            operation_id: "operation-1".to_owned(),
        };
        assert_eq!(
            rotation
                .validate()
                .expect_err("credential-shaped composition revision is rejected")
                .code(),
            "credentials_forbidden"
        );
    }
}
