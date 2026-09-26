//! Unified verification records: authority, audit baseline, evidence, verdict,
//! target mutation, and reconciliation.
//!
//! Owner: architecture 17 (delegated verifier authority) with architecture
//! 28's Goal-facing surface and ADR 0044 unification. This module owns the
//! `VerifierAuthorityV1`, `VerifierAuditBaselineV1`, evidence, verdict,
//! target-mutation, and reconciliation records with architecture 14 framing
//! conventions, the architecture 17 operation set, stale-baseline fail-closed
//! validation, user-conflict precedence, the fail-closed mandatory-selection
//! rule, and the Goal-facing public projections.
//!
//! Every record is credential-free, typed, immutable at its selected
//! revision, digest-bearing, and carries only safe identity, revision,
//! digest, and frozen reference values: never raw content, credentials,
//! paths, grants, provider resources, process handles, or implementation
//! state. The records use the anonymous tag-zero version-one canonical frame
//! because they are not tag-registry records; their ledger tags remain
//! reserved for the later storage/wire wave.
//!
//! `Pause`/`Resume` are user-lifecycle operations and carry no verifier
//! authority: the closed verifier operation set is exactly
//! [`VerifierOperationV1`]. Mandate-scoped authority transactions, storage,
//! scheduling, and wire DTOs belong to M11 and later waves.

use crate::canonical::{
    CanonicalError, CanonicalIdentityInput, CanonicalRecordReader, Digest256, NamespacedDigest,
    WireType, decode_list_items, decode_u64, decode_uuid_list, encode_list_items, encode_u64,
};
use crate::run_execution_meaning::{record, uuid};
use intention_types::{DtoResult, ErrorDto};

/// Maximum frozen Goal references in one verifier frozen-reference set.
pub const VERIFIER_MAX_FROZEN_GOAL_REFERENCES: usize = 32;

/// Maximum gate and evidence contract references in one verifier
/// frozen-reference set.
pub const VERIFIER_MAX_FROZEN_CONTRACT_REFERENCES: usize = 32;

/// Maximum evidence references carried by one verifier verdict or target
/// mutation.
pub const VERIFIER_MAX_EVIDENCE_REFERENCES: usize = 512;

/// Maximum encoded bytes of one verifier canonical-style record (512 KiB).
pub const MAX_VERIFIER_RECORD_BYTES: usize = 512 * 1024;

/// The closed separately issued delegated verifier operation set.
///
/// `Pause` and `Resume` are user-lifecycle operations owned by architecture
/// 13 and are deliberately absent: they never appear as verifier operations
/// and never require verifier authority.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerifierOperationV1 {
    /// Move the target to `NeedsRework` after qualifying fail evidence.
    MarkNeedsRework,
    /// Complete the target after unconditional pass evidence, no unresolved
    /// uncertainty, and required graph terminalization closure.
    MarkComplete,
    /// Stop the target without asserting completion.
    Stop,
    /// Create an immutable future target revision without rewriting history.
    ReviseFull,
    /// Resolve the exact target uncertainty into later fresh work or a stop.
    ResolveUnknownEffect,
}

impl VerifierOperationV1 {
    /// The complete closed verifier operation set in canonical order.
    pub const ALL: [Self; 5] = [
        Self::MarkNeedsRework,
        Self::MarkComplete,
        Self::Stop,
        Self::ReviseFull,
        Self::ResolveUnknownEffect,
    ];

    /// Returns the stable numeric discriminant of this operation.
    #[must_use]
    pub const fn discriminant(self) -> u64 {
        match self {
            Self::MarkNeedsRework => 0,
            Self::MarkComplete => 1,
            Self::Stop => 2,
            Self::ReviseFull => 3,
            Self::ResolveUnknownEffect => 4,
        }
    }

    /// Returns the stable snake_case code of this operation.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::MarkNeedsRework => "mark_needs_rework",
            Self::MarkComplete => "mark_complete",
            Self::Stop => "stop",
            Self::ReviseFull => "revise_full",
            Self::ResolveUnknownEffect => "resolve_unknown_effect",
        }
    }

    /// Decodes one closed operation discriminant; unknown values fail closed.
    #[must_use]
    pub const fn from_discriminant(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::MarkNeedsRework),
            1 => Some(Self::MarkComplete),
            2 => Some(Self::Stop),
            3 => Some(Self::ReviseFull),
            4 => Some(Self::ResolveUnknownEffect),
            _ => None,
        }
    }
}

/// One user-lifecycle operation with no verifier authority.
///
/// Architecture 13 owns these operations; they are never verifier powers and
/// no verifier authority path exists for them.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UserLifecycleOperationV1 {
    /// Pause the target through user lifecycle authority.
    Pause,
    /// Resume the target through user lifecycle authority.
    Resume,
}

impl UserLifecycleOperationV1 {
    /// The complete closed user-lifecycle operation set in canonical order.
    pub const ALL: [Self; 2] = [Self::Pause, Self::Resume];

    /// Returns the stable snake_case code of this operation.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Resume => "resume",
        }
    }

    /// User-lifecycle operations never require or consume verifier authority.
    #[must_use]
    pub const fn requires_verifier_authority(self) -> bool {
        false
    }

    /// User-lifecycle operations never map to a verifier operation.
    #[must_use]
    pub const fn verifier_operation(self) -> Option<VerifierOperationV1> {
        None
    }
}

/// The closed Mandate target lifecycle of one verifier baseline.
///
/// The variants mirror the architecture 13 Mandate lifecycle exactly.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerifierTargetLifecycleV1 {
    /// No admissible run exists yet.
    Draft,
    /// The target can admit one fresh run when an eligible reason exists.
    Active,
    /// Exactly one non-terminal target run exists.
    Working,
    /// The target is paused by the user.
    Paused,
    /// The target is in mandatory uncertainty quarantine.
    PausedAwaitingDecision,
    /// The target needs rework before further completion.
    NeedsRework,
    /// The target asserts full objective acceptance.
    Completed,
    /// The target asserts no completion.
    Stopped,
    /// The target is inert historical presentation.
    Archived,
}

impl VerifierTargetLifecycleV1 {
    /// The complete closed target lifecycle set in canonical order.
    pub const ALL: [Self; 9] = [
        Self::Draft,
        Self::Active,
        Self::Working,
        Self::Paused,
        Self::PausedAwaitingDecision,
        Self::NeedsRework,
        Self::Completed,
        Self::Stopped,
        Self::Archived,
    ];

    /// Returns the stable numeric discriminant of this lifecycle state.
    #[must_use]
    pub const fn discriminant(self) -> u64 {
        match self {
            Self::Draft => 0,
            Self::Active => 1,
            Self::Working => 2,
            Self::Paused => 3,
            Self::PausedAwaitingDecision => 4,
            Self::NeedsRework => 5,
            Self::Completed => 6,
            Self::Stopped => 7,
            Self::Archived => 8,
        }
    }

    /// Returns whether this lifecycle state admits no further ordinary work.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Stopped | Self::Archived)
    }

    /// Decodes one closed lifecycle discriminant; unknown values fail closed.
    #[must_use]
    pub const fn from_discriminant(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::Draft),
            1 => Some(Self::Active),
            2 => Some(Self::Working),
            3 => Some(Self::Paused),
            4 => Some(Self::PausedAwaitingDecision),
            5 => Some(Self::NeedsRework),
            6 => Some(Self::Completed),
            7 => Some(Self::Stopped),
            8 => Some(Self::Archived),
            _ => None,
        }
    }
}

/// The closed audit evidence kinds of one verifier audit.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerifierEvidenceKindV1 {
    /// Unconditional pass evidence.
    UnconditionalPass,
    /// Qualifying fail evidence.
    QualifyingFail,
    /// Inconclusive evidence that supports no operation.
    Inconclusive,
    /// Evidence of required graph terminalization closure.
    GraphTerminalizationClosure,
    /// Evidence that the audit contract reconciliation standard holds.
    ReconciliationStandardProof,
}

impl VerifierEvidenceKindV1 {
    /// The complete closed evidence-kind set in canonical order.
    pub const ALL: [Self; 5] = [
        Self::UnconditionalPass,
        Self::QualifyingFail,
        Self::Inconclusive,
        Self::GraphTerminalizationClosure,
        Self::ReconciliationStandardProof,
    ];

    /// Returns the stable numeric discriminant of this evidence kind.
    #[must_use]
    pub const fn discriminant(self) -> u64 {
        match self {
            Self::UnconditionalPass => 0,
            Self::QualifyingFail => 1,
            Self::Inconclusive => 2,
            Self::GraphTerminalizationClosure => 3,
            Self::ReconciliationStandardProof => 4,
        }
    }

    /// Returns the stable snake_case code of this evidence kind.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnconditionalPass => "unconditional_pass",
            Self::QualifyingFail => "qualifying_fail",
            Self::Inconclusive => "inconclusive",
            Self::GraphTerminalizationClosure => "graph_terminalization_closure",
            Self::ReconciliationStandardProof => "reconciliation_standard_proof",
        }
    }

    /// Decodes one closed evidence-kind discriminant; unknown values fail
    /// closed.
    #[must_use]
    pub const fn from_discriminant(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::UnconditionalPass),
            1 => Some(Self::QualifyingFail),
            2 => Some(Self::Inconclusive),
            3 => Some(Self::GraphTerminalizationClosure),
            4 => Some(Self::ReconciliationStandardProof),
            _ => None,
        }
    }

    /// Returns whether this evidence kind qualifies for `operation`.
    #[must_use]
    pub const fn supports(self, operation: VerifierOperationV1) -> bool {
        matches!(
            (self, operation),
            (Self::UnconditionalPass, VerifierOperationV1::MarkComplete)
                | (Self::QualifyingFail, VerifierOperationV1::MarkNeedsRework)
                | (
                    Self::GraphTerminalizationClosure,
                    VerifierOperationV1::MarkComplete
                )
                | (
                    Self::ReconciliationStandardProof,
                    VerifierOperationV1::ResolveUnknownEffect
                )
        )
    }
}

/// The closed verifier audit verdict kinds.
///
/// Architecture 28 names this closed set `VerificationAuditVerdictDto`. A
/// verdict is durable evidence only: it neither schedules work nor reserves
/// or mutates a target.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerificationAuditVerdictDto {
    /// The audit passed.
    Pass,
    /// The audit failed.
    Fail,
    /// The audit was inconclusive.
    Inconclusive,
    /// The frozen target revision is stale.
    TargetRevisionStale,
    /// The target is unavailable.
    TargetUnavailable,
    /// The verifier is unavailable.
    VerifierUnavailable,
    /// The verifier's own external effect is unknown.
    VerifierExternalEffectUnknown,
}

impl VerificationAuditVerdictDto {
    /// The complete closed verdict set in canonical order.
    pub const ALL: [Self; 7] = [
        Self::Pass,
        Self::Fail,
        Self::Inconclusive,
        Self::TargetRevisionStale,
        Self::TargetUnavailable,
        Self::VerifierUnavailable,
        Self::VerifierExternalEffectUnknown,
    ];

    /// Returns the stable numeric discriminant of this verdict.
    #[must_use]
    pub const fn discriminant(self) -> u64 {
        match self {
            Self::Pass => 0,
            Self::Fail => 1,
            Self::Inconclusive => 2,
            Self::TargetRevisionStale => 3,
            Self::TargetUnavailable => 4,
            Self::VerifierUnavailable => 5,
            Self::VerifierExternalEffectUnknown => 6,
        }
    }

    /// Returns the stable snake_case code of this verdict.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Inconclusive => "inconclusive",
            Self::TargetRevisionStale => "target_revision_stale",
            Self::TargetUnavailable => "target_unavailable",
            Self::VerifierUnavailable => "verifier_unavailable",
            Self::VerifierExternalEffectUnknown => "verifier_external_effect_unknown",
        }
    }

    /// Decodes one closed verdict discriminant; unknown values fail closed.
    #[must_use]
    pub const fn from_discriminant(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::Pass),
            1 => Some(Self::Fail),
            2 => Some(Self::Inconclusive),
            3 => Some(Self::TargetRevisionStale),
            4 => Some(Self::TargetUnavailable),
            5 => Some(Self::VerifierUnavailable),
            6 => Some(Self::VerifierExternalEffectUnknown),
            _ => None,
        }
    }
}

/// The closed authority consumption rule selected at issuance.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerifierAuthorityConsumptionRuleV1 {
    /// The authority is consumed by one applied mutation.
    SingleUse,
    /// The authority stays usable while active, unrevoked, and unexpired.
    ReusableWhileActive,
}

impl VerifierAuthorityConsumptionRuleV1 {
    /// Returns the stable numeric discriminant of this rule.
    #[must_use]
    pub const fn discriminant(self) -> u64 {
        match self {
            Self::SingleUse => 0,
            Self::ReusableWhileActive => 1,
        }
    }

    /// Decodes one closed consumption-rule discriminant; unknown values fail
    /// closed.
    #[must_use]
    pub const fn from_discriminant(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::SingleUse),
            1 => Some(Self::ReusableWhileActive),
            _ => None,
        }
    }
}

/// The closed authority consumption state.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerifierAuthorityConsumptionV1 {
    /// The authority is not yet consumed.
    Unconsumed,
    /// The authority was consumed by one applied target mutation.
    Consumed {
        /// The applied mutation that consumed this authority.
        mutation_reference: [u8; 16],
    },
}

/// The closed reconciliation outcome of one exact unknown-effect
/// reconciliation.
///
/// The closed set never asserts rollback, absence, idempotence, repeatability,
/// or safe replay of the old external effect.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerifierReconciliationOutcomeV1 {
    /// The target may admit later fresh work with a new run identity.
    ActiveForLaterFreshWork,
    /// The target is stopped without asserting completion.
    Stopped,
}

impl VerifierReconciliationOutcomeV1 {
    /// The complete closed reconciliation outcome set in canonical order.
    pub const ALL: [Self; 2] = [Self::ActiveForLaterFreshWork, Self::Stopped];

    /// Returns the stable numeric discriminant of this outcome.
    #[must_use]
    pub const fn discriminant(self) -> u64 {
        match self {
            Self::ActiveForLaterFreshWork => 0,
            Self::Stopped => 1,
        }
    }

    /// Decodes one closed outcome discriminant; unknown values fail closed.
    #[must_use]
    pub const fn from_discriminant(value: u64) -> Option<Self> {
        match value {
            0 => Some(Self::ActiveForLaterFreshWork),
            1 => Some(Self::Stopped),
            _ => None,
        }
    }
}

/// The closed issuance, expiry, revocation, and consumption state of one
/// delegated verifier authority revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierAuthorityLifecycleV1 {
    /// The issuance time in Unix milliseconds.
    pub issued_at_ms: u64,
    /// The optional expiry time in Unix milliseconds, exclusive.
    pub expires_at_ms: Option<u64>,
    /// The optional revocation time in Unix milliseconds.
    pub revoked_at_ms: Option<u64>,
    /// The optional immutable revocation reference.
    pub revocation_reference: Option<[u8; 16]>,
    /// The selected consumption rule.
    pub consumption_rule: VerifierAuthorityConsumptionRuleV1,
    /// The closed consumption state.
    pub consumption: VerifierAuthorityConsumptionV1,
}

impl VerifierAuthorityLifecycleV1 {
    /// Validates the closed issuance, expiry, revocation, and consumption
    /// rules.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for an unpaired revocation, a
    /// revocation before issuance, an empty expiry window, or a consumed
    /// reusable authority.
    pub const fn validate(&self) -> Result<(), CanonicalError> {
        if self.revoked_at_ms.is_some() != self.revocation_reference.is_some() {
            return Err(CanonicalError::InvalidField);
        }
        if matches!(self.revoked_at_ms, Some(revoked_at_ms) if revoked_at_ms < self.issued_at_ms) {
            return Err(CanonicalError::InvalidField);
        }
        if matches!(self.expires_at_ms, Some(expires_at_ms) if expires_at_ms <= self.issued_at_ms) {
            return Err(CanonicalError::InvalidField);
        }
        if matches!(
            self.consumption,
            VerifierAuthorityConsumptionV1::Consumed { .. }
        ) && !matches!(
            self.consumption_rule,
            VerifierAuthorityConsumptionRuleV1::SingleUse
        ) {
            return Err(CanonicalError::InvalidField);
        }
        Ok(())
    }

    /// Returns whether this authority revision is revoked.
    #[must_use]
    pub const fn is_revoked(&self) -> bool {
        self.revoked_at_ms.is_some()
    }

    /// Returns whether this authority revision is expired at `now_ms`.
    #[must_use]
    pub const fn is_expired_at(&self, now_ms: u64) -> bool {
        matches!(self.expires_at_ms, Some(expires_at_ms) if now_ms >= expires_at_ms)
    }

    /// Returns whether this authority revision is consumed.
    #[must_use]
    pub const fn is_consumed(&self) -> bool {
        matches!(
            self.consumption,
            VerifierAuthorityConsumptionV1::Consumed { .. }
        )
    }

    /// Encodes this lifecycle into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`] and other
    /// `CanonicalError` values only for impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        record(
            0,
            1,
            vec![
                (1, WireType::U64, encode_u64(self.issued_at_ms)),
                (
                    2,
                    WireType::Optional,
                    encode_optional_u64(self.expires_at_ms),
                ),
                (
                    3,
                    WireType::Optional,
                    encode_optional_u64(self.revoked_at_ms),
                ),
                (
                    4,
                    WireType::Optional,
                    encode_optional_uuid(self.revocation_reference),
                ),
                (
                    5,
                    WireType::U64,
                    encode_u64(self.consumption_rule.discriminant()),
                ),
                (6, WireType::Record, self.consumption.encode()?),
            ],
        )
    }

    /// Decodes this lifecycle from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown nested version,
    /// `CanonicalError::InvalidField` for an absent, malformed, or unknown
    /// field, the validation errors of [`Self::validate`], and other
    /// `CanonicalError` values for malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 6)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let lifecycle = Self {
            issued_at_ms: decode_u64(
                reader
                    .field(1, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            expires_at_ms: decode_optional_u64(
                reader
                    .field(2, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            revoked_at_ms: decode_optional_u64(
                reader
                    .field(3, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            revocation_reference: decode_optional_uuid(
                reader
                    .field(4, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            consumption_rule: VerifierAuthorityConsumptionRuleV1::from_discriminant(decode_u64(
                reader
                    .field(5, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?)
            .ok_or(CanonicalError::InvalidField)?,
            consumption: VerifierAuthorityConsumptionV1::decode(
                reader
                    .field(6, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        lifecycle.validate()?;
        Ok(lifecycle)
    }
}

impl VerifierAuthorityConsumptionV1 {
    /// Encodes this consumption state into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` or
    /// `CanonicalError::OverLimit` only if the fixed field table were
    /// noncanonical or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        match self {
            Self::Unconsumed => record(0, 1, vec![(1, WireType::U64, encode_u64(0))]),
            Self::Consumed { mutation_reference } => record(
                0,
                1,
                vec![
                    (1, WireType::U64, encode_u64(1)),
                    (2, WireType::Uuid, mutation_reference.to_vec()),
                ],
            ),
        }
    }

    /// Decodes this consumption state from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown nested version and
    /// `CanonicalError::InvalidField` for an absent, malformed, or unknown
    /// field.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 2)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let discriminant = decode_u64(
            reader
                .field(1, WireType::U64)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        match discriminant {
            0 => Ok(Self::Unconsumed),
            1 => Ok(Self::Consumed {
                mutation_reference: uuid(
                    reader
                        .field(2, WireType::Uuid)?
                        .ok_or(CanonicalError::InvalidField)?,
                )?,
            }),
            _ => Err(CanonicalError::InvalidField),
        }
    }
}

/// An immutable reference to one explicitly enumerated verifier target set.
///
/// Target-set membership is resolved against the referenced immutable record
/// by the later authority transaction; the reference itself never expands
/// through ancestry, descendants, siblings, Goals, sessions, or evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierTargetSetReferenceV1 {
    /// The target-set identity.
    pub target_set_id: [u8; 16],
    /// The canonical target-set digest.
    pub target_set_digest: Digest256,
}

impl VerifierTargetSetReferenceV1 {
    /// Encodes this reference into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` or
    /// `CanonicalError::OverLimit` only if the fixed field table were
    /// noncanonical or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        record(
            0,
            1,
            vec![
                (1, WireType::Uuid, self.target_set_id.to_vec()),
                (2, WireType::Digest, self.target_set_digest.bytes().to_vec()),
            ],
        )
    }

    /// Decodes this reference from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown nested version,
    /// `CanonicalError::InvalidField` for an absent or malformed field, and
    /// other `CanonicalError` values for malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 2)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        Ok(Self {
            target_set_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            target_set_digest: Digest256::from_bytes(
                reader
                    .field(2, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        })
    }
}

/// An immutable reference to one audit or gate/evidence contract revision.
///
/// Required evidence kinds, completion standards, and reconciliation
/// standards live in the referenced contract record; the reference carries
/// only safe identity, revision, and digest values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierContractReferenceV1 {
    /// The contract identity.
    pub contract_id: [u8; 16],
    /// The exact immutable contract revision.
    pub contract_revision: u64,
    /// The canonical contract digest.
    pub contract_digest: Digest256,
}

impl VerifierContractReferenceV1 {
    /// Validates this reference.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a zero contract revision.
    pub const fn validate(&self) -> Result<(), CanonicalError> {
        if self.contract_revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        Ok(())
    }

    /// Encodes this reference into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`] and other
    /// `CanonicalError` values only for impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        record(
            0,
            1,
            vec![
                (1, WireType::Uuid, self.contract_id.to_vec()),
                (2, WireType::U64, encode_u64(self.contract_revision)),
                (3, WireType::Digest, self.contract_digest.bytes().to_vec()),
            ],
        )
    }

    /// Decodes this reference from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown nested version,
    /// `CanonicalError::InvalidField` for an absent, malformed, or zero
    /// revision field, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 3)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let reference = Self {
            contract_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            contract_revision: decode_u64(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            contract_digest: Digest256::from_bytes(
                reader
                    .field(3, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        reference.validate()?;
        Ok(reference)
    }
}

/// An immutable reference to one exact authority revision and digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierAuthorityReferenceV1 {
    /// The authority identity.
    pub authority_id: [u8; 16],
    /// The exact immutable authority revision.
    pub authority_revision: u64,
    /// The canonical authority digest.
    pub authority_digest: Digest256,
}

impl VerifierAuthorityReferenceV1 {
    /// Validates this reference.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a zero authority revision.
    pub const fn validate(&self) -> Result<(), CanonicalError> {
        if self.authority_revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        Ok(())
    }

    /// Encodes this reference into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`] and other
    /// `CanonicalError` values only for impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        record(
            0,
            1,
            vec![
                (1, WireType::Uuid, self.authority_id.to_vec()),
                (2, WireType::U64, encode_u64(self.authority_revision)),
                (3, WireType::Digest, self.authority_digest.bytes().to_vec()),
            ],
        )
    }

    /// Decodes this reference from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown nested version,
    /// `CanonicalError::InvalidField` for an absent, malformed, or zero
    /// revision field, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 3)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let reference = Self {
            authority_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            authority_revision: decode_u64(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            authority_digest: Digest256::from_bytes(
                reader
                    .field(3, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        reference.validate()?;
        Ok(reference)
    }
}

/// An immutable reference to one exact target revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierTargetReferenceV1 {
    /// The target Mandate identity.
    pub target_mandate_id: [u8; 16],
    /// The exact frozen target revision.
    pub target_revision: u64,
}

impl VerifierTargetReferenceV1 {
    /// Validates this reference.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a zero target revision.
    pub const fn validate(&self) -> Result<(), CanonicalError> {
        if self.target_revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        Ok(())
    }

    /// Encodes this reference into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`] and other
    /// `CanonicalError` values only for impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        record(
            0,
            1,
            vec![
                (1, WireType::Uuid, self.target_mandate_id.to_vec()),
                (2, WireType::U64, encode_u64(self.target_revision)),
            ],
        )
    }

    /// Decodes this reference from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown nested version,
    /// `CanonicalError::InvalidField` for an absent, malformed, or zero
    /// revision field, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 2)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let reference = Self {
            target_mandate_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            target_revision: decode_u64(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        reference.validate()?;
        Ok(reference)
    }
}

/// The frozen target revision and aggregate sequence of one audit baseline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierTargetRevisionAndSequenceV1 {
    /// The exact target revision.
    pub revision: u64,
    /// The aggregate target sequence at the frozen revision.
    pub aggregate_sequence: u64,
}

impl VerifierTargetRevisionAndSequenceV1 {
    /// Validates this frozen revision tuple.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a zero target revision.
    pub const fn validate(&self) -> Result<(), CanonicalError> {
        if self.revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        Ok(())
    }

    /// Encodes this tuple into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`] and other
    /// `CanonicalError` values only for impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        record(
            0,
            1,
            vec![
                (1, WireType::U64, encode_u64(self.revision)),
                (2, WireType::U64, encode_u64(self.aggregate_sequence)),
            ],
        )
    }

    /// Decodes this tuple from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown nested version,
    /// `CanonicalError::InvalidField` for an absent, malformed, or zero
    /// revision field, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 2)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let tuple = Self {
            revision: decode_u64(
                reader
                    .field(1, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            aggregate_sequence: decode_u64(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        tuple.validate()?;
        Ok(tuple)
    }
}

/// An immutable frozen Goal revision reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierGoalReferenceV1 {
    /// The Goal identity.
    pub goal_id: [u8; 16],
    /// The exact immutable Goal revision.
    pub goal_revision: u64,
}

impl VerifierGoalReferenceV1 {
    /// Validates this reference.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a zero Goal revision.
    pub const fn validate(&self) -> Result<(), CanonicalError> {
        if self.goal_revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        Ok(())
    }

    /// Encodes this reference into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`] and other
    /// `CanonicalError` values only for impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        record(
            0,
            1,
            vec![
                (1, WireType::Uuid, self.goal_id.to_vec()),
                (2, WireType::U64, encode_u64(self.goal_revision)),
            ],
        )
    }

    /// Decodes this reference from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown nested version,
    /// `CanonicalError::InvalidField` for an absent, malformed, or zero
    /// revision field, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 2)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let reference = Self {
            goal_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            goal_revision: decode_u64(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        reference.validate()?;
        Ok(reference)
    }
}

/// The frozen Goal and gate/evidence contract references of one verifier
/// baseline.
///
/// Keys are unique: one Goal identity and one contract identity appear at
/// most once, in declared order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierFrozenGoalGateEvidenceReferencesV1 {
    /// The frozen Goal revisions, in declared order.
    pub goal_references: Vec<VerifierGoalReferenceV1>,
    /// The frozen gate and evidence contract revisions, in declared order.
    pub gate_evidence_contract_references: Vec<VerifierContractReferenceV1>,
}

impl VerifierFrozenGoalGateEvidenceReferencesV1 {
    /// Validates the closed bounds and unique keys of these frozen references.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::OverLimit` when either list exceeds its
    /// code-owned bound and `CanonicalError::InvalidField` for a duplicate
    /// Goal or contract identity or an invalid nested reference.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        if self.goal_references.len() > VERIFIER_MAX_FROZEN_GOAL_REFERENCES
            || self.gate_evidence_contract_references.len()
                > VERIFIER_MAX_FROZEN_CONTRACT_REFERENCES
        {
            return Err(CanonicalError::OverLimit);
        }
        for (index, reference) in self.goal_references.iter().enumerate() {
            if self.goal_references[..index]
                .iter()
                .any(|earlier| earlier.goal_id == reference.goal_id)
            {
                return Err(CanonicalError::InvalidField);
            }
            reference.validate()?;
        }
        for (index, reference) in self.gate_evidence_contract_references.iter().enumerate() {
            if self.gate_evidence_contract_references[..index]
                .iter()
                .any(|earlier| earlier.contract_id == reference.contract_id)
            {
                return Err(CanonicalError::InvalidField);
            }
            reference.validate()?;
        }
        Ok(())
    }

    /// Encodes these frozen references into their nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`] and other
    /// `CanonicalError` values for the nested component failures or
    /// impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        let goals = self
            .goal_references
            .iter()
            .map(VerifierGoalReferenceV1::encode)
            .collect::<Result<Vec<_>, _>>()?;
        let contracts = self
            .gate_evidence_contract_references
            .iter()
            .map(VerifierContractReferenceV1::encode)
            .collect::<Result<Vec<_>, _>>()?;
        record(
            0,
            1,
            vec![
                (1, WireType::List, encode_list_items(&goals)),
                (2, WireType::List, encode_list_items(&contracts)),
            ],
        )
    }

    /// Decodes these frozen references from their nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown nested version,
    /// `CanonicalError::InvalidField` for an absent or malformed field or
    /// duplicate key, `CanonicalError::OverLimit` for an over-limit list, and
    /// other `CanonicalError` values for malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 2)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let goal_references = decode_list_items(
            reader
                .field(1, WireType::List)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .iter()
        .map(|item| VerifierGoalReferenceV1::decode(item))
        .collect::<Result<Vec<_>, _>>()?;
        let gate_evidence_contract_references = decode_list_items(
            reader
                .field(2, WireType::List)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .iter()
        .map(|item| VerifierContractReferenceV1::decode(item))
        .collect::<Result<Vec<_>, _>>()?;
        let references = Self {
            goal_references,
            gate_evidence_contract_references,
        };
        references.validate()?;
        Ok(references)
    }
}

/// The exact idempotent operation identity of one verifier action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierOperationIdentityV1 {
    /// The stable idempotent operation identity.
    pub operation_id: [u8; 16],
    /// The semantic operation digest.
    pub operation_digest: Digest256,
}

/// The separately issued, revisioned, target-scoped delegated verifier
/// authority (architecture 17 `VerifierAuthorityV1`).
///
/// One authority is usable only while its revision is active, unrevoked,
/// unexpired, and unconsumed where required, and only by the named verifier
/// Mandate. It never targets its own verifier Mandate, and its target set
/// never expands through parenthood, ancestry, descendants, siblings, Goals,
/// sessions, branches, activity, or shared evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierAuthorityV1 {
    /// The stable authority identity.
    pub authority_id: [u8; 16],
    /// The immutable authority revision.
    pub authority_revision: u64,
    /// The exact verifier Mandate that owns this authority.
    pub verifier_mandate_id: [u8; 16],
    /// The immutable explicitly enumerated target-set reference.
    pub immutable_target_set_reference: VerifierTargetSetReferenceV1,
    /// The closed allowed-operation set in canonical discriminant order.
    pub allowed_operations: Vec<VerifierOperationV1>,
    /// The immutable audit contract reference.
    pub audit_contract_reference: VerifierContractReferenceV1,
    /// The issuance, expiry, revocation, and consumption state.
    pub lifecycle: VerifierAuthorityLifecycleV1,
    canonical_digest: Digest256,
}

impl VerifierAuthorityV1 {
    /// Creates one immutable authority revision and computes its canonical
    /// identity digest.
    ///
    /// The allowed-operation set is normalized into canonical discriminant
    /// order; the closed set never contains a user-lifecycle operation.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` for a zero revision, an empty or
    /// duplicated operation set, an invalid lifecycle, or a nested invalid
    /// reference, and `verifier_authority_invalid` with an over-limit
    /// message when a field exceeds its code-owned bound.
    pub fn new(
        authority_id: [u8; 16],
        authority_revision: u64,
        verifier_mandate_id: [u8; 16],
        immutable_target_set_reference: VerifierTargetSetReferenceV1,
        mut allowed_operations: Vec<VerifierOperationV1>,
        audit_contract_reference: VerifierContractReferenceV1,
        lifecycle: VerifierAuthorityLifecycleV1,
    ) -> DtoResult<Self> {
        allowed_operations.sort_by_key(|operation| operation.discriminant());
        let candidate = Self {
            authority_id,
            authority_revision,
            verifier_mandate_id,
            immutable_target_set_reference,
            allowed_operations,
            audit_contract_reference,
            lifecycle,
            canonical_digest: zero_digest()
                .map_err(|error| verifier_error(error, "verifier_authority_invalid"))?,
        };
        candidate
            .validate()
            .map_err(|error| verifier_error(error, "verifier_authority_invalid"))?;
        let digest = candidate
            .identity_digest()
            .map_err(|error| verifier_error(error, "verifier_authority_invalid"))?;
        Ok(Self {
            canonical_digest: digest.digest,
            ..candidate
        })
    }

    /// Returns the canonical authority digest.
    #[must_use]
    pub const fn canonical_digest(&self) -> Digest256 {
        self.canonical_digest
    }

    /// Returns whether this revision allows `operation`.
    #[must_use]
    pub fn allows_operation(&self, operation: VerifierOperationV1) -> bool {
        self.allowed_operations.contains(&operation)
    }

    /// Validates the closed authority rules.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a zero revision, an empty
    /// or non-canonical operation set, or an invalid nested reference, and
    /// `CanonicalError::OverLimit` for an over-limit record.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        if self.authority_revision == 0
            || self.allowed_operations.is_empty()
            || self.allowed_operations.len() > VerifierOperationV1::ALL.len()
        {
            return Err(CanonicalError::InvalidField);
        }
        for (index, operation) in self.allowed_operations.iter().enumerate() {
            if index > 0
                && self.allowed_operations[index - 1].discriminant() >= operation.discriminant()
            {
                return Err(CanonicalError::InvalidField);
            }
        }
        self.audit_contract_reference.validate()?;
        self.lifecycle.validate()?;
        Ok(())
    }

    /// Computes the namespaced identity digest over this record's
    /// identity-bearing fields; the digest field itself is excluded by
    /// construction.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidDigest` only if the fixed namespace
    /// were invalid, and the identity input's own encoding errors, which are
    /// impossible for a canonical field stream.
    pub fn identity_digest(&self) -> Result<NamespacedDigest, CanonicalError> {
        let mut input = CanonicalIdentityInput::new();
        for (number, wire_type, value) in self.identity_fields()? {
            input = input.field(number, wire_type, value)?;
        }
        Digest256::for_namespace("verifier-authority-v1", &input.encode()?)
    }

    /// Returns the identity-bearing canonical field table shared by
    /// [`Self::encode`] and [`Self::identity_digest`].
    fn identity_fields(&self) -> Result<Vec<(u32, WireType, Vec<u8>)>, CanonicalError> {
        Ok(vec![
            (1, WireType::Uuid, self.authority_id.to_vec()),
            (2, WireType::U64, encode_u64(self.authority_revision)),
            (3, WireType::Uuid, self.verifier_mandate_id.to_vec()),
            (
                4,
                WireType::Record,
                self.immutable_target_set_reference.encode()?,
            ),
            (
                5,
                WireType::List,
                encode_operations(&self.allowed_operations)?,
            ),
            (6, WireType::Record, self.audit_contract_reference.encode()?),
            (7, WireType::Record, self.lifecycle.encode()?),
        ])
    }

    /// Encodes this authority into its canonical-style record bytes.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`],
    /// `CanonicalError::OverLimit` when the record exceeds its byte bound,
    /// and other `CanonicalError` values for the nested component failures.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        let mut fields = self.identity_fields()?;
        fields.push((8, WireType::Digest, self.canonical_digest.bytes().to_vec()));
        let bytes = record(0, 1, fields)?;
        if bytes.len() > MAX_VERIFIER_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        Ok(bytes)
    }

    /// Decodes this authority from its canonical-style record bytes and
    /// verifies its canonical digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown frame,
    /// `CanonicalError::InvalidField` for an absent, malformed, or
    /// non-canonical field, the validation errors of [`Self::validate`],
    /// `CanonicalError::DigestMismatch` when the stored digest does not match
    /// the identity bytes, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        if bytes.len() > MAX_VERIFIER_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        let reader = CanonicalRecordReader::new(bytes, 8)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let record = Self {
            authority_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            authority_revision: decode_u64(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            verifier_mandate_id: uuid(
                reader
                    .field(3, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            immutable_target_set_reference: VerifierTargetSetReferenceV1::decode(
                reader
                    .field(4, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            allowed_operations: decode_operations(
                reader
                    .field(5, WireType::List)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            audit_contract_reference: VerifierContractReferenceV1::decode(
                reader
                    .field(6, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            lifecycle: VerifierAuthorityLifecycleV1::decode(
                reader
                    .field(7, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            canonical_digest: Digest256::from_bytes(
                reader
                    .field(8, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        record.validate()?;
        verify_digest(record.identity_digest()?, record.canonical_digest)?;
        Ok(record)
    }

    /// Projects the Goal-facing safe authority surface.
    #[must_use]
    pub fn project(&self) -> VerificationMandateAuthorityDto {
        VerificationMandateAuthorityDto {
            authority_id: self.authority_id,
            verifier_mandate_id: self.verifier_mandate_id,
            authority_revision: self.authority_revision,
            target_set_reference: self.immutable_target_set_reference,
            allowed_operations: self.allowed_operations.clone(),
            audit_contract_reference: self.audit_contract_reference,
            canonical_authority_digest: self.canonical_digest,
        }
    }
}

/// The frozen audit baseline of one verifier authority against one target.
///
/// A baseline is fresh only when every element of its tuple matches the
/// committed state: exact target identity, target revision, aggregate
/// sequence, lifecycle, authority revision and digest, audit contract, graph
/// epoch where applicable, and operation idempotency identity and digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierAuditBaselineV1 {
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceV1,
    /// The exact verifier Mandate revision at baseline creation.
    pub verifier_mandate_revision: u64,
    /// The target Mandate identity.
    pub target_mandate_id: [u8; 16],
    /// The frozen target revision and aggregate sequence.
    pub target_revision_and_sequence: VerifierTargetRevisionAndSequenceV1,
    /// The frozen target lifecycle.
    pub target_lifecycle: VerifierTargetLifecycleV1,
    /// The frozen Goal and gate/evidence contract references.
    pub frozen_references: VerifierFrozenGoalGateEvidenceReferencesV1,
    /// The exact unknown-effect reference when the target is uncertain.
    pub optional_unknown_effect_reference: Option<[u8; 16]>,
    /// The exact audit contract revision and digest.
    pub audit_contract_reference: VerifierContractReferenceV1,
    /// The target graph epoch where the target participates in a child graph.
    pub graph_epoch: Option<u64>,
    canonical_digest: Digest256,
}

impl VerifierAuditBaselineV1 {
    /// Creates one immutable audit baseline and computes its canonical
    /// identity digest.
    ///
    /// # Errors
    ///
    /// Returns `verifier_baseline_invalid` for a zero revision, an invalid
    /// nested reference, a duplicate frozen key, or an over-limit frozen
    /// list.
    #[expect(
        clippy::too_many_arguments,
        reason = "The closed architecture 17 baseline field list stays one flat validating constructor."
    )]
    pub fn new(
        authority_reference: VerifierAuthorityReferenceV1,
        verifier_mandate_revision: u64,
        target_mandate_id: [u8; 16],
        target_revision_and_sequence: VerifierTargetRevisionAndSequenceV1,
        target_lifecycle: VerifierTargetLifecycleV1,
        frozen_references: VerifierFrozenGoalGateEvidenceReferencesV1,
        optional_unknown_effect_reference: Option<[u8; 16]>,
        audit_contract_reference: VerifierContractReferenceV1,
        graph_epoch: Option<u64>,
    ) -> DtoResult<Self> {
        let candidate = Self {
            authority_reference,
            verifier_mandate_revision,
            target_mandate_id,
            target_revision_and_sequence,
            target_lifecycle,
            frozen_references,
            optional_unknown_effect_reference,
            audit_contract_reference,
            graph_epoch,
            canonical_digest: zero_digest()
                .map_err(|error| verifier_error(error, "verifier_baseline_invalid"))?,
        };
        candidate
            .validate()
            .map_err(|error| verifier_error(error, "verifier_baseline_invalid"))?;
        let digest = candidate
            .identity_digest()
            .map_err(|error| verifier_error(error, "verifier_baseline_invalid"))?;
        Ok(Self {
            canonical_digest: digest.digest,
            ..candidate
        })
    }

    /// Returns the canonical baseline digest.
    #[must_use]
    pub const fn canonical_digest(&self) -> Digest256 {
        self.canonical_digest
    }

    /// Validates the closed baseline rules.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a zero revision or an
    /// invalid nested reference and `CanonicalError::OverLimit` for an
    /// over-limit frozen list.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        if self.verifier_mandate_revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        self.authority_reference.validate()?;
        self.target_revision_and_sequence.validate()?;
        self.frozen_references.validate()?;
        self.audit_contract_reference.validate()?;
        Ok(())
    }

    /// Computes the namespaced identity digest over this record's
    /// identity-bearing fields; the digest field itself is excluded by
    /// construction.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidDigest` only if the fixed namespace
    /// were invalid, and the identity input's own encoding errors, which are
    /// impossible for a canonical field stream.
    pub fn identity_digest(&self) -> Result<NamespacedDigest, CanonicalError> {
        let mut input = CanonicalIdentityInput::new();
        for (number, wire_type, value) in self.identity_fields()? {
            input = input.field(number, wire_type, value)?;
        }
        Digest256::for_namespace("verifier-audit-baseline-v1", &input.encode()?)
    }

    /// Returns the identity-bearing canonical field table shared by
    /// [`Self::encode`] and [`Self::identity_digest`].
    fn identity_fields(&self) -> Result<Vec<(u32, WireType, Vec<u8>)>, CanonicalError> {
        Ok(vec![
            (1, WireType::Record, self.authority_reference.encode()?),
            (2, WireType::U64, encode_u64(self.verifier_mandate_revision)),
            (3, WireType::Uuid, self.target_mandate_id.to_vec()),
            (
                4,
                WireType::Record,
                self.target_revision_and_sequence.encode()?,
            ),
            (
                5,
                WireType::U64,
                encode_u64(self.target_lifecycle.discriminant()),
            ),
            (6, WireType::Record, self.frozen_references.encode()?),
            (
                7,
                WireType::Optional,
                encode_optional_uuid(self.optional_unknown_effect_reference),
            ),
            (8, WireType::Record, self.audit_contract_reference.encode()?),
            (9, WireType::Optional, encode_optional_u64(self.graph_epoch)),
        ])
    }

    /// Encodes this baseline into its canonical-style record bytes.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`],
    /// `CanonicalError::OverLimit` when the record exceeds its byte bound,
    /// and other `CanonicalError` values for the nested component failures.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        let mut fields = self.identity_fields()?;
        fields.push((10, WireType::Digest, self.canonical_digest.bytes().to_vec()));
        let bytes = record(0, 1, fields)?;
        if bytes.len() > MAX_VERIFIER_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        Ok(bytes)
    }

    /// Decodes this baseline from its canonical-style record bytes and
    /// verifies its canonical digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown frame,
    /// `CanonicalError::InvalidField` for an absent or malformed field, the
    /// validation errors of [`Self::validate`],
    /// `CanonicalError::DigestMismatch` when the stored digest does not match
    /// the identity bytes, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        if bytes.len() > MAX_VERIFIER_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        let reader = CanonicalRecordReader::new(bytes, 10)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let record = Self {
            authority_reference: VerifierAuthorityReferenceV1::decode(
                reader
                    .field(1, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            verifier_mandate_revision: decode_u64(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            target_mandate_id: uuid(
                reader
                    .field(3, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            target_revision_and_sequence: VerifierTargetRevisionAndSequenceV1::decode(
                reader
                    .field(4, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            target_lifecycle: VerifierTargetLifecycleV1::from_discriminant(decode_u64(
                reader
                    .field(5, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?)
            .ok_or(CanonicalError::InvalidField)?,
            frozen_references: VerifierFrozenGoalGateEvidenceReferencesV1::decode(
                reader
                    .field(6, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            optional_unknown_effect_reference: decode_optional_uuid(
                reader
                    .field(7, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            audit_contract_reference: VerifierContractReferenceV1::decode(
                reader
                    .field(8, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            graph_epoch: decode_optional_u64(
                reader
                    .field(9, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            canonical_digest: Digest256::from_bytes(
                reader
                    .field(10, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        record.validate()?;
        verify_digest(record.identity_digest()?, record.canonical_digest)?;
        Ok(record)
    }
}

/// One immutable verifier audit evidence record.
///
/// Evidence is durable and non-authorizing: it grants no operation, no
/// target mutation, and no scheduling effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierAuditEvidenceV1 {
    /// The stable evidence identity.
    pub evidence_id: [u8; 16],
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceV1,
    /// The exact target revision.
    pub target_reference: VerifierTargetReferenceV1,
    /// The frozen Goal and gate/evidence contract references.
    pub frozen_references: VerifierFrozenGoalGateEvidenceReferencesV1,
    /// The closed evidence kind.
    pub evidence_kind: VerifierEvidenceKindV1,
    /// The safe retained-content reference; never raw content.
    pub retained_content_reference: [u8; 16],
    canonical_digest: Digest256,
}

impl VerifierAuditEvidenceV1 {
    /// Creates one immutable evidence record and computes its canonical
    /// identity digest.
    ///
    /// # Errors
    ///
    /// Returns `verifier_evidence_invalid` for an invalid nested reference,
    /// a duplicate frozen key, an over-limit frozen list, or an over-limit
    /// record.
    pub fn new(
        evidence_id: [u8; 16],
        authority_reference: VerifierAuthorityReferenceV1,
        target_reference: VerifierTargetReferenceV1,
        frozen_references: VerifierFrozenGoalGateEvidenceReferencesV1,
        evidence_kind: VerifierEvidenceKindV1,
        retained_content_reference: [u8; 16],
    ) -> DtoResult<Self> {
        let candidate = Self {
            evidence_id,
            authority_reference,
            target_reference,
            frozen_references,
            evidence_kind,
            retained_content_reference,
            canonical_digest: zero_digest()
                .map_err(|error| verifier_error(error, "verifier_evidence_invalid"))?,
        };
        candidate
            .validate()
            .map_err(|error| verifier_error(error, "verifier_evidence_invalid"))?;
        let digest = candidate
            .identity_digest()
            .map_err(|error| verifier_error(error, "verifier_evidence_invalid"))?;
        Ok(Self {
            canonical_digest: digest.digest,
            ..candidate
        })
    }

    /// Returns the canonical evidence digest.
    #[must_use]
    pub const fn canonical_digest(&self) -> Digest256 {
        self.canonical_digest
    }

    /// Validates the closed evidence rules.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for an invalid nested reference
    /// or duplicate frozen key and `CanonicalError::OverLimit` for an
    /// over-limit frozen list.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        self.authority_reference.validate()?;
        self.target_reference.validate()?;
        self.frozen_references.validate()?;
        Ok(())
    }

    /// Computes the namespaced identity digest over this record's
    /// identity-bearing fields; the digest field itself is excluded by
    /// construction.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidDigest` only if the fixed namespace
    /// were invalid, and the identity input's own encoding errors, which are
    /// impossible for a canonical field stream.
    pub fn identity_digest(&self) -> Result<NamespacedDigest, CanonicalError> {
        let mut input = CanonicalIdentityInput::new();
        for (number, wire_type, value) in self.identity_fields()? {
            input = input.field(number, wire_type, value)?;
        }
        Digest256::for_namespace("verifier-audit-evidence-v1", &input.encode()?)
    }

    /// Returns the identity-bearing canonical field table shared by
    /// [`Self::encode`] and [`Self::identity_digest`].
    fn identity_fields(&self) -> Result<Vec<(u32, WireType, Vec<u8>)>, CanonicalError> {
        Ok(vec![
            (1, WireType::Uuid, self.evidence_id.to_vec()),
            (2, WireType::Record, self.authority_reference.encode()?),
            (3, WireType::Record, self.target_reference.encode()?),
            (4, WireType::Record, self.frozen_references.encode()?),
            (
                5,
                WireType::U64,
                encode_u64(self.evidence_kind.discriminant()),
            ),
            (6, WireType::Uuid, self.retained_content_reference.to_vec()),
        ])
    }

    /// Encodes this evidence into its canonical-style record bytes.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`],
    /// `CanonicalError::OverLimit` when the record exceeds its byte bound,
    /// and other `CanonicalError` values for the nested component failures.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        let mut fields = self.identity_fields()?;
        fields.push((7, WireType::Digest, self.canonical_digest.bytes().to_vec()));
        let bytes = record(0, 1, fields)?;
        if bytes.len() > MAX_VERIFIER_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        Ok(bytes)
    }

    /// Decodes this evidence from its canonical-style record bytes and
    /// verifies its canonical digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown frame,
    /// `CanonicalError::InvalidField` for an absent or malformed field, the
    /// validation errors of [`Self::validate`],
    /// `CanonicalError::DigestMismatch` when the stored digest does not match
    /// the identity bytes, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        if bytes.len() > MAX_VERIFIER_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        let reader = CanonicalRecordReader::new(bytes, 7)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let record = Self {
            evidence_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            authority_reference: VerifierAuthorityReferenceV1::decode(
                reader
                    .field(2, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            target_reference: VerifierTargetReferenceV1::decode(
                reader
                    .field(3, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            frozen_references: VerifierFrozenGoalGateEvidenceReferencesV1::decode(
                reader
                    .field(4, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            evidence_kind: VerifierEvidenceKindV1::from_discriminant(decode_u64(
                reader
                    .field(5, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?)
            .ok_or(CanonicalError::InvalidField)?,
            retained_content_reference: uuid(
                reader
                    .field(6, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            canonical_digest: Digest256::from_bytes(
                reader
                    .field(7, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        record.validate()?;
        verify_digest(record.identity_digest()?, record.canonical_digest)?;
        Ok(record)
    }

    /// Projects the Goal-facing safe evidence surface.
    #[must_use]
    pub fn project(&self) -> VerificationAuditEvidenceDto {
        VerificationAuditEvidenceDto {
            evidence_id: self.evidence_id,
            authority_reference: self.authority_reference,
            target_reference: self.target_reference,
            frozen_goal_references: self.frozen_references.goal_references.clone(),
            frozen_gate_evidence_contract_references: self
                .frozen_references
                .gate_evidence_contract_references
                .clone(),
            evidence_kind: self.evidence_kind,
            retained_content_reference: self.retained_content_reference,
            canonical_evidence_digest: self.canonical_digest,
        }
    }
}

/// One immutable verifier audit verdict record.
///
/// A verdict is durable evidence only: it neither schedules work nor reserves
/// or mutates a target, and dependent verifier work still validates the exact
/// authority, baseline, and operation prerequisites.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierAuditVerdictRecordV1 {
    /// The stable verdict identity.
    pub verdict_id: [u8; 16],
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceV1,
    /// The exact target revision.
    pub target_reference: VerifierTargetReferenceV1,
    /// The exact frozen baseline digest the verdict was reached against.
    pub baseline_digest: Digest256,
    /// The closed verdict.
    pub verdict: VerificationAuditVerdictDto,
    /// The audit evidence references, in declared order, without duplicates.
    pub evidence_references: Vec<[u8; 16]>,
    canonical_digest: Digest256,
}

impl VerifierAuditVerdictRecordV1 {
    /// Creates one immutable verdict record and computes its canonical
    /// identity digest.
    ///
    /// # Errors
    ///
    /// Returns `verifier_verdict_invalid` for an invalid nested reference, a
    /// duplicate evidence reference, or an over-limit evidence list.
    pub fn new(
        verdict_id: [u8; 16],
        authority_reference: VerifierAuthorityReferenceV1,
        target_reference: VerifierTargetReferenceV1,
        baseline_digest: Digest256,
        verdict: VerificationAuditVerdictDto,
        evidence_references: Vec<[u8; 16]>,
    ) -> DtoResult<Self> {
        let candidate = Self {
            verdict_id,
            authority_reference,
            target_reference,
            baseline_digest,
            verdict,
            evidence_references,
            canonical_digest: zero_digest()
                .map_err(|error| verifier_error(error, "verifier_verdict_invalid"))?,
        };
        candidate
            .validate()
            .map_err(|error| verifier_error(error, "verifier_verdict_invalid"))?;
        let digest = candidate
            .identity_digest()
            .map_err(|error| verifier_error(error, "verifier_verdict_invalid"))?;
        Ok(Self {
            canonical_digest: digest.digest,
            ..candidate
        })
    }

    /// Returns the canonical verdict digest.
    #[must_use]
    pub const fn canonical_digest(&self) -> Digest256 {
        self.canonical_digest
    }

    /// Validates the closed verdict rules.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for an invalid nested reference
    /// or a duplicate evidence reference and `CanonicalError::OverLimit` for
    /// an over-limit evidence list.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        self.authority_reference.validate()?;
        self.target_reference.validate()?;
        validate_evidence_references(&self.evidence_references)?;
        Ok(())
    }

    /// Computes the namespaced identity digest over this record's
    /// identity-bearing fields; the digest field itself is excluded by
    /// construction.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidDigest` only if the fixed namespace
    /// were invalid, and the identity input's own encoding errors, which are
    /// impossible for a canonical field stream.
    pub fn identity_digest(&self) -> Result<NamespacedDigest, CanonicalError> {
        let mut input = CanonicalIdentityInput::new();
        for (number, wire_type, value) in self.identity_fields()? {
            input = input.field(number, wire_type, value)?;
        }
        Digest256::for_namespace("verifier-audit-verdict-v1", &input.encode()?)
    }

    /// Returns the identity-bearing canonical field table shared by
    /// [`Self::encode`] and [`Self::identity_digest`].
    fn identity_fields(&self) -> Result<Vec<(u32, WireType, Vec<u8>)>, CanonicalError> {
        Ok(vec![
            (1, WireType::Uuid, self.verdict_id.to_vec()),
            (2, WireType::Record, self.authority_reference.encode()?),
            (3, WireType::Record, self.target_reference.encode()?),
            (4, WireType::Digest, self.baseline_digest.bytes().to_vec()),
            (5, WireType::U64, encode_u64(self.verdict.discriminant())),
            (
                6,
                WireType::List,
                encode_uuid_list(&self.evidence_references),
            ),
        ])
    }

    /// Encodes this verdict into its canonical-style record bytes.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`],
    /// `CanonicalError::OverLimit` when the record exceeds its byte bound,
    /// and other `CanonicalError` values for the nested component failures.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        let mut fields = self.identity_fields()?;
        fields.push((7, WireType::Digest, self.canonical_digest.bytes().to_vec()));
        let bytes = record(0, 1, fields)?;
        if bytes.len() > MAX_VERIFIER_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        Ok(bytes)
    }

    /// Decodes this verdict from its canonical-style record bytes and
    /// verifies its canonical digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown frame,
    /// `CanonicalError::InvalidField` for an absent or malformed field, the
    /// validation errors of [`Self::validate`],
    /// `CanonicalError::DigestMismatch` when the stored digest does not match
    /// the identity bytes, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        if bytes.len() > MAX_VERIFIER_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        let reader = CanonicalRecordReader::new(bytes, 7)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let record = Self {
            verdict_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            authority_reference: VerifierAuthorityReferenceV1::decode(
                reader
                    .field(2, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            target_reference: VerifierTargetReferenceV1::decode(
                reader
                    .field(3, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            baseline_digest: Digest256::from_bytes(
                reader
                    .field(4, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            verdict: VerificationAuditVerdictDto::from_discriminant(decode_u64(
                reader
                    .field(5, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?)
            .ok_or(CanonicalError::InvalidField)?,
            evidence_references: decode_uuid_list(
                reader
                    .field(6, WireType::List)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            canonical_digest: Digest256::from_bytes(
                reader
                    .field(7, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        record.validate()?;
        verify_digest(record.identity_digest()?, record.canonical_digest)?;
        Ok(record)
    }

    /// Projects the Goal-facing safe verdict surface.
    #[must_use]
    pub fn project(&self) -> VerificationVerdictDto {
        VerificationVerdictDto {
            verdict_id: self.verdict_id,
            authority_reference: self.authority_reference,
            target_reference: self.target_reference,
            baseline_digest: self.baseline_digest,
            verdict: self.verdict,
            evidence_references: self.evidence_references.clone(),
            canonical_verdict_digest: self.canonical_digest,
        }
    }
}

/// One immutable verifier target-mutation request.
///
/// The request carries the exact frozen baseline digest and idempotent
/// operation identity; the later authority transaction validates the
/// authority, baseline, evidence, verdict, target revision/sequence/lifecycle,
/// graph closure, exact uncertainty, and idempotency identity/digest before
/// it commits all or nothing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierTargetMutationV1 {
    /// The stable mutation identity.
    pub mutation_id: [u8; 16],
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceV1,
    /// The exact audit contract revision and digest.
    pub audit_contract_reference: VerifierContractReferenceV1,
    /// The exact target revision.
    pub target_reference: VerifierTargetReferenceV1,
    /// The closed verifier operation.
    pub operation: VerifierOperationV1,
    /// The audit evidence references, in declared order, without duplicates.
    pub audit_evidence_references: Vec<[u8; 16]>,
    /// The expected target revision of the frozen baseline.
    pub expected_target_revision: u64,
    /// The expected aggregate target sequence of the frozen baseline.
    pub expected_target_sequence: u64,
    /// The exact frozen baseline digest.
    pub expected_baseline_digest: Digest256,
    /// The idempotent operation identity and semantic digest.
    pub idempotency: VerifierOperationIdentityV1,
    canonical_digest: Digest256,
}

impl VerifierTargetMutationV1 {
    /// Creates one immutable target-mutation request and computes its
    /// canonical identity digest.
    ///
    /// # Errors
    ///
    /// Returns `verifier_mutation_invalid` for a zero expected revision, an
    /// invalid nested contract or authority reference, a duplicate evidence
    /// reference, or an over-limit evidence list.
    #[expect(
        clippy::too_many_arguments,
        reason = "The closed architecture 28 mutation field list stays one flat validating constructor."
    )]
    pub fn new(
        mutation_id: [u8; 16],
        authority_reference: VerifierAuthorityReferenceV1,
        audit_contract_reference: VerifierContractReferenceV1,
        target_reference: VerifierTargetReferenceV1,
        operation: VerifierOperationV1,
        audit_evidence_references: Vec<[u8; 16]>,
        expected_target_revision: u64,
        expected_target_sequence: u64,
        expected_baseline_digest: Digest256,
        idempotency: VerifierOperationIdentityV1,
    ) -> DtoResult<Self> {
        let candidate = Self {
            mutation_id,
            authority_reference,
            audit_contract_reference,
            target_reference,
            operation,
            audit_evidence_references,
            expected_target_revision,
            expected_target_sequence,
            expected_baseline_digest,
            idempotency,
            canonical_digest: zero_digest()
                .map_err(|error| verifier_error(error, "verifier_mutation_invalid"))?,
        };
        candidate
            .validate()
            .map_err(|error| verifier_error(error, "verifier_mutation_invalid"))?;
        let digest = candidate
            .identity_digest()
            .map_err(|error| verifier_error(error, "verifier_mutation_invalid"))?;
        Ok(Self {
            canonical_digest: digest.digest,
            ..candidate
        })
    }

    /// Returns the canonical mutation digest.
    #[must_use]
    pub const fn canonical_digest(&self) -> Digest256 {
        self.canonical_digest
    }

    /// Validates the closed mutation rules.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a zero expected revision or
    /// an invalid nested reference or a duplicate evidence reference and
    /// `CanonicalError::OverLimit` for an over-limit evidence list.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        if self.expected_target_revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        self.authority_reference.validate()?;
        self.audit_contract_reference.validate()?;
        self.target_reference.validate()?;
        validate_evidence_references(&self.audit_evidence_references)?;
        Ok(())
    }

    /// Computes the namespaced identity digest over this record's
    /// identity-bearing fields; the digest field itself is excluded by
    /// construction.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidDigest` only if the fixed namespace
    /// were invalid, and the identity input's own encoding errors, which are
    /// impossible for a canonical field stream.
    pub fn identity_digest(&self) -> Result<NamespacedDigest, CanonicalError> {
        let mut input = CanonicalIdentityInput::new();
        for (number, wire_type, value) in self.identity_fields()? {
            input = input.field(number, wire_type, value)?;
        }
        Digest256::for_namespace("verifier-target-mutation-v1", &input.encode()?)
    }

    /// Returns the identity-bearing canonical field table shared by
    /// [`Self::encode`] and [`Self::identity_digest`].
    fn identity_fields(&self) -> Result<Vec<(u32, WireType, Vec<u8>)>, CanonicalError> {
        Ok(vec![
            (1, WireType::Uuid, self.mutation_id.to_vec()),
            (2, WireType::Record, self.authority_reference.encode()?),
            (3, WireType::Record, self.audit_contract_reference.encode()?),
            (4, WireType::Record, self.target_reference.encode()?),
            (5, WireType::U64, encode_u64(self.operation.discriminant())),
            (
                6,
                WireType::List,
                encode_uuid_list(&self.audit_evidence_references),
            ),
            (7, WireType::U64, encode_u64(self.expected_target_revision)),
            (8, WireType::U64, encode_u64(self.expected_target_sequence)),
            (
                9,
                WireType::Digest,
                self.expected_baseline_digest.bytes().to_vec(),
            ),
            (10, WireType::Record, self.idempotency.encode()?),
        ])
    }

    /// Encodes this mutation into its canonical-style record bytes.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`],
    /// `CanonicalError::OverLimit` when the record exceeds its byte bound,
    /// and other `CanonicalError` values for the nested component failures.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        let mut fields = self.identity_fields()?;
        fields.push((11, WireType::Digest, self.canonical_digest.bytes().to_vec()));
        let bytes = record(0, 1, fields)?;
        if bytes.len() > MAX_VERIFIER_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        Ok(bytes)
    }

    /// Decodes this mutation from its canonical-style record bytes and
    /// verifies its canonical digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown frame,
    /// `CanonicalError::InvalidField` for an absent or malformed field, the
    /// validation errors of [`Self::validate`],
    /// `CanonicalError::DigestMismatch` when the stored digest does not match
    /// the identity bytes, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        if bytes.len() > MAX_VERIFIER_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        let reader = CanonicalRecordReader::new(bytes, 11)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let record = Self {
            mutation_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            authority_reference: VerifierAuthorityReferenceV1::decode(
                reader
                    .field(2, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            audit_contract_reference: VerifierContractReferenceV1::decode(
                reader
                    .field(3, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            target_reference: VerifierTargetReferenceV1::decode(
                reader
                    .field(4, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            operation: VerifierOperationV1::from_discriminant(decode_u64(
                reader
                    .field(5, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?)
            .ok_or(CanonicalError::InvalidField)?,
            audit_evidence_references: decode_uuid_list(
                reader
                    .field(6, WireType::List)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            expected_target_revision: decode_u64(
                reader
                    .field(7, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            expected_target_sequence: decode_u64(
                reader
                    .field(8, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            expected_baseline_digest: Digest256::from_bytes(
                reader
                    .field(9, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            idempotency: VerifierOperationIdentityV1::decode(
                reader
                    .field(10, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            canonical_digest: Digest256::from_bytes(
                reader
                    .field(11, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        record.validate()?;
        verify_digest(record.identity_digest()?, record.canonical_digest)?;
        Ok(record)
    }
}

impl VerifierOperationIdentityV1 {
    /// Encodes this operation identity into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` or
    /// `CanonicalError::OverLimit` only if the fixed field table were
    /// noncanonical or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        record(
            0,
            1,
            vec![
                (1, WireType::Uuid, self.operation_id.to_vec()),
                (2, WireType::Digest, self.operation_digest.bytes().to_vec()),
            ],
        )
    }

    /// Decodes this operation identity from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown nested version,
    /// `CanonicalError::InvalidField` for an absent or malformed field, and
    /// other `CanonicalError` values for malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 2)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        Ok(Self {
            operation_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            operation_digest: Digest256::from_bytes(
                reader
                    .field(2, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        })
    }
}

/// One immutable exact unknown-effect reconciliation record.
///
/// The same durable record family serves user reconciliation without
/// verifier authority (`authority_reference` is absent) and delegated
/// verifier reconciliation. The closed outcome never asserts rollback,
/// absence, idempotence, repeatability, or safe replay of the old external
/// effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierReconciliationV1 {
    /// The stable reconciliation identity.
    pub reconciliation_id: [u8; 16],
    /// The exact authority revision when a verifier performed the
    /// reconciliation; absent for user reconciliation.
    pub authority_reference: Option<VerifierAuthorityReferenceV1>,
    /// The exact target revision.
    pub target_reference: VerifierTargetReferenceV1,
    /// The exact frozen baseline digest the reconciliation was proven on.
    pub baseline_digest: Digest256,
    /// The exact target unknown-effect reference named by the reconciliation.
    pub target_unknown_effect_reference: [u8; 16],
    /// The closed reconciliation outcome.
    pub outcome: VerifierReconciliationOutcomeV1,
    canonical_digest: Digest256,
}

impl VerifierReconciliationV1 {
    /// Creates one immutable reconciliation record and computes its canonical
    /// identity digest.
    ///
    /// # Errors
    ///
    /// Returns `verifier_reconciliation_invalid` for an invalid nested target
    /// or authority reference.
    pub fn new(
        reconciliation_id: [u8; 16],
        authority_reference: Option<VerifierAuthorityReferenceV1>,
        target_reference: VerifierTargetReferenceV1,
        baseline_digest: Digest256,
        target_unknown_effect_reference: [u8; 16],
        outcome: VerifierReconciliationOutcomeV1,
    ) -> DtoResult<Self> {
        let candidate = Self {
            reconciliation_id,
            authority_reference,
            target_reference,
            baseline_digest,
            target_unknown_effect_reference,
            outcome,
            canonical_digest: zero_digest()
                .map_err(|error| verifier_error(error, "verifier_reconciliation_invalid"))?,
        };
        candidate
            .validate()
            .map_err(|error| verifier_error(error, "verifier_reconciliation_invalid"))?;
        let digest = candidate
            .identity_digest()
            .map_err(|error| verifier_error(error, "verifier_reconciliation_invalid"))?;
        Ok(Self {
            canonical_digest: digest.digest,
            ..candidate
        })
    }

    /// Returns the canonical reconciliation digest.
    #[must_use]
    pub const fn canonical_digest(&self) -> Digest256 {
        self.canonical_digest
    }

    /// Validates the closed reconciliation rules.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for an invalid nested target or
    /// authority reference.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        if let Some(reference) = self.authority_reference.as_ref() {
            reference.validate()?;
        }
        self.target_reference.validate()?;
        Ok(())
    }

    /// Computes the namespaced identity digest over this record's
    /// identity-bearing fields; the digest field itself is excluded by
    /// construction.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidDigest` only if the fixed namespace
    /// were invalid, and the identity input's own encoding errors, which are
    /// impossible for a canonical field stream.
    pub fn identity_digest(&self) -> Result<NamespacedDigest, CanonicalError> {
        let mut input = CanonicalIdentityInput::new();
        for (number, wire_type, value) in self.identity_fields()? {
            input = input.field(number, wire_type, value)?;
        }
        Digest256::for_namespace("verifier-reconciliation-v1", &input.encode()?)
    }

    /// Returns the identity-bearing canonical field table shared by
    /// [`Self::encode`] and [`Self::identity_digest`].
    fn identity_fields(&self) -> Result<Vec<(u32, WireType, Vec<u8>)>, CanonicalError> {
        Ok(vec![
            (1, WireType::Uuid, self.reconciliation_id.to_vec()),
            (
                2,
                WireType::Optional,
                encode_optional_authority_reference(&self.authority_reference)?,
            ),
            (3, WireType::Record, self.target_reference.encode()?),
            (4, WireType::Digest, self.baseline_digest.bytes().to_vec()),
            (
                5,
                WireType::Uuid,
                self.target_unknown_effect_reference.to_vec(),
            ),
            (6, WireType::U64, encode_u64(self.outcome.discriminant())),
        ])
    }

    /// Encodes this reconciliation into its canonical-style record bytes.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`],
    /// `CanonicalError::OverLimit` when the record exceeds its byte bound,
    /// and other `CanonicalError` values for the nested component failures.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        let mut fields = self.identity_fields()?;
        fields.push((7, WireType::Digest, self.canonical_digest.bytes().to_vec()));
        let bytes = record(0, 1, fields)?;
        if bytes.len() > MAX_VERIFIER_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        Ok(bytes)
    }

    /// Decodes this reconciliation from its canonical-style record bytes and
    /// verifies its canonical digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown frame,
    /// `CanonicalError::InvalidField` for an absent or malformed field, the
    /// validation errors of [`Self::validate`],
    /// `CanonicalError::DigestMismatch` when the stored digest does not match
    /// the identity bytes, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        if bytes.len() > MAX_VERIFIER_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        let reader = CanonicalRecordReader::new(bytes, 7)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let record = Self {
            reconciliation_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            authority_reference: decode_optional_authority_reference(
                reader
                    .field(2, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            target_reference: VerifierTargetReferenceV1::decode(
                reader
                    .field(3, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            baseline_digest: Digest256::from_bytes(
                reader
                    .field(4, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            target_unknown_effect_reference: uuid(
                reader
                    .field(5, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            outcome: VerifierReconciliationOutcomeV1::from_discriminant(decode_u64(
                reader
                    .field(6, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?)
            .ok_or(CanonicalError::InvalidField)?,
            canonical_digest: Digest256::from_bytes(
                reader
                    .field(7, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        record.validate()?;
        verify_digest(record.identity_digest()?, record.canonical_digest)?;
        Ok(record)
    }

    /// Projects the Goal-facing safe reconciliation surface.
    #[must_use]
    pub const fn project(&self) -> VerificationReconciliationDto {
        VerificationReconciliationDto {
            reconciliation_id: self.reconciliation_id,
            authority_reference: self.authority_reference,
            target_reference: self.target_reference,
            baseline_digest: self.baseline_digest,
            target_unknown_effect_reference: self.target_unknown_effect_reference,
            outcome: self.outcome,
            canonical_reconciliation_digest: self.canonical_digest,
        }
    }
}

/// The fail-closed recovery rule every stale baseline and lost verifier
/// conflict shares.
///
/// A losing verifier action performs a scoped reread and never merges,
/// retargets, substitutes current state, changes the selected operation, or
/// retries with changed meaning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierFailClosedRecoveryV1 {
    /// A scoped durable reread is required before any further action.
    pub scoped_reread_required: bool,
    /// Best-effort merging of the losing action into committed state is
    /// forbidden.
    pub may_merge: bool,
    /// Retargeting the action to another target is forbidden.
    pub may_retarget: bool,
    /// Selecting a different operation is forbidden.
    pub may_change_operation: bool,
    /// Substituting current state for the frozen baseline is forbidden.
    pub may_substitute_current_state: bool,
    /// Retrying with changed meaning is forbidden.
    pub may_retry_with_changed_meaning: bool,
}

impl VerifierFailClosedRecoveryV1 {
    /// The single fail-closed recovery rule.
    pub const FAIL_CLOSED: Self = Self {
        scoped_reread_required: true,
        may_merge: false,
        may_retarget: false,
        may_change_operation: false,
        may_substitute_current_state: false,
        may_retry_with_changed_meaning: false,
    };
}

/// One concurrent committed mutation competing with a verifier action.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerifierCompetingMutationV1 {
    /// A user lifecycle mutation (pause, resume, stop, archive).
    UserLifecycle,
    /// A user Mandate revision mutation.
    UserRevision,
    /// A user unknown-effect reconciliation mutation.
    UserReconciliation,
    /// A user authority revocation mutation.
    UserRevocation,
    /// A user authority-revision mutation.
    UserAuthorityRevision,
    /// A direct-parent child control mutation.
    ParentChildControl,
    /// A daemon safety-cascade terminalization mutation.
    DaemonSafetyCascade,
    /// A peer verifier mutation on a disjoint authority.
    VerifierPeer,
}

impl VerifierCompetingMutationV1 {
    /// The complete closed competing-mutation set in canonical order.
    pub const ALL: [Self; 8] = [
        Self::UserLifecycle,
        Self::UserRevision,
        Self::UserReconciliation,
        Self::UserRevocation,
        Self::UserAuthorityRevision,
        Self::ParentChildControl,
        Self::DaemonSafetyCascade,
        Self::VerifierPeer,
    ];

    /// Returns whether this competing mutation is an architecture 13 user
    /// mutation, which always wins an optimistic conflict.
    #[must_use]
    pub const fn is_user_mutation(self) -> bool {
        matches!(
            self,
            Self::UserLifecycle
                | Self::UserRevision
                | Self::UserReconciliation
                | Self::UserRevocation
                | Self::UserAuthorityRevision
        )
    }
}

/// The closed winner of one verifier optimistic conflict.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerifierConflictWinnerV1 {
    /// The committed user mutation wins.
    UserMutation,
    /// The other committed concurrent mutation wins.
    CommittedConcurrentMutation,
}

/// The outcome of one verifier optimistic conflict.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierConflictResolutionV1 {
    /// The committed winning mutation class.
    pub winner: VerifierConflictWinnerV1,
    /// Whether the competing verifier action applied; always false.
    pub verifier_action_applied: bool,
    /// The shared fail-closed recovery rule for the losing verifier action.
    pub recovery: VerifierFailClosedRecoveryV1,
}

/// Resolves one optimistic conflict between a verifier action and a
/// concurrent committed mutation.
///
/// User lifecycle, revision, reconciliation, revocation, and authority
/// revision mutations win; any other committed concurrent mutation also
/// leaves the verifier action unapplied. The losing verifier action performs
/// a scoped reread and cannot merge, retarget, substitute current state,
/// change the operation, or retry with changed meaning.
#[must_use]
pub const fn resolve_verifier_optimistic_conflict(
    competing: VerifierCompetingMutationV1,
) -> VerifierConflictResolutionV1 {
    let winner = if competing.is_user_mutation() {
        VerifierConflictWinnerV1::UserMutation
    } else {
        VerifierConflictWinnerV1::CommittedConcurrentMutation
    };
    VerifierConflictResolutionV1 {
        winner,
        verifier_action_applied: false,
        recovery: VerifierFailClosedRecoveryV1::FAIL_CLOSED,
    }
}

/// One element of the architecture 17 stale-baseline tuple.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerifierBaselineElementV1 {
    /// The exact target identity.
    TargetIdentity,
    /// The exact target revision.
    TargetRevision,
    /// The exact aggregate target sequence.
    TargetSequence,
    /// The exact target lifecycle.
    TargetLifecycle,
    /// The exact authority identity.
    AuthorityIdentity,
    /// The exact authority revision.
    AuthorityRevision,
    /// The exact authority digest.
    AuthorityDigest,
    /// The exact audit contract reference.
    AuditContract,
    /// The target graph epoch where applicable.
    GraphEpoch,
    /// The operation idempotency identity.
    OperationIdentity,
    /// The operation idempotency digest.
    OperationDigest,
}

impl VerifierBaselineElementV1 {
    /// The complete stale-baseline tuple in canonical check order.
    pub const ALL: [Self; 11] = [
        Self::TargetIdentity,
        Self::TargetRevision,
        Self::TargetSequence,
        Self::TargetLifecycle,
        Self::AuthorityIdentity,
        Self::AuthorityRevision,
        Self::AuthorityDigest,
        Self::AuditContract,
        Self::GraphEpoch,
        Self::OperationIdentity,
        Self::OperationDigest,
    ];

    /// Returns the stable machine-readable code of this stale element.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::TargetIdentity => "verifier_baseline_stale_target_identity",
            Self::TargetRevision => "verifier_baseline_stale_target_revision",
            Self::TargetSequence => "verifier_baseline_stale_target_sequence",
            Self::TargetLifecycle => "verifier_baseline_stale_target_lifecycle",
            Self::AuthorityIdentity => "verifier_baseline_stale_authority_identity",
            Self::AuthorityRevision => "verifier_baseline_stale_authority_revision",
            Self::AuthorityDigest => "verifier_baseline_stale_authority_digest",
            Self::AuditContract => "verifier_baseline_stale_audit_contract",
            Self::GraphEpoch => "verifier_baseline_stale_graph_epoch",
            Self::OperationIdentity => "verifier_baseline_stale_operation_identity",
            Self::OperationDigest => "verifier_baseline_stale_operation_digest",
        }
    }
}

/// One typed pre-mutation stale-baseline failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierBaselineStaleV1 {
    /// The exact stale tuple element.
    pub element: VerifierBaselineElementV1,
}

impl VerifierBaselineStaleV1 {
    /// Creates one typed stale-baseline failure.
    #[must_use]
    pub const fn new(element: VerifierBaselineElementV1) -> Self {
        Self { element }
    }

    /// Returns the stable machine-readable code of this stale failure.
    #[must_use]
    pub const fn code(self) -> &'static str {
        self.element.code()
    }

    /// Returns the shared fail-closed recovery rule.
    #[must_use]
    pub const fn recovery(self) -> VerifierFailClosedRecoveryV1 {
        VerifierFailClosedRecoveryV1::FAIL_CLOSED
    }
}

/// The closed outcome of one baseline freshness check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifierBaselineCheckV1 {
    /// Every baseline tuple element matches the committed state.
    Fresh,
    /// The equal operation identity and digest already committed; the saved
    /// result is returned without another mutation.
    IdempotentReplay,
}

/// The current committed state one verifier baseline is validated against.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierCommittedStateV1 {
    /// The committed target identity.
    pub target_mandate_id: [u8; 16],
    /// The committed target revision.
    pub target_revision: u64,
    /// The committed aggregate target sequence.
    pub target_aggregate_sequence: u64,
    /// The committed target lifecycle.
    pub target_lifecycle: VerifierTargetLifecycleV1,
    /// The committed authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceV1,
    /// The committed audit contract reference.
    pub audit_contract_reference: VerifierContractReferenceV1,
    /// The committed target graph epoch where applicable.
    pub graph_epoch: Option<u64>,
    /// The already committed operation identity for this target, if any.
    pub committed_operation: Option<VerifierOperationIdentityV1>,
}

/// Evaluates one verifier audit baseline against the committed state.
///
/// The baseline is fresh only when every tuple element matches: exact target
/// identity, target revision, aggregate sequence, lifecycle, authority
/// identity, authority revision, authority digest, audit contract, graph
/// epoch where applicable, and operation idempotency identity and digest. An
/// equal operation identity and digest already committed returns
/// [`VerifierBaselineCheckV1::IdempotentReplay`]. Any other mismatch is a
/// typed stale failure; the system never best-effort merges, retargets,
/// substitutes current state, or retries with changed meaning.
///
/// # Errors
///
/// Returns [`VerifierBaselineStaleV1`] naming the exact stale tuple element.
pub fn evaluate_baseline_freshness(
    baseline: &VerifierAuditBaselineV1,
    operation: &VerifierOperationIdentityV1,
    committed: &VerifierCommittedStateV1,
) -> Result<VerifierBaselineCheckV1, VerifierBaselineStaleV1> {
    if committed.committed_operation == Some(*operation) {
        return Ok(VerifierBaselineCheckV1::IdempotentReplay);
    }
    if baseline.target_mandate_id != committed.target_mandate_id {
        return stale(VerifierBaselineElementV1::TargetIdentity);
    }
    if baseline.target_revision_and_sequence.revision != committed.target_revision {
        return stale(VerifierBaselineElementV1::TargetRevision);
    }
    if baseline.target_revision_and_sequence.aggregate_sequence
        != committed.target_aggregate_sequence
    {
        return stale(VerifierBaselineElementV1::TargetSequence);
    }
    if baseline.target_lifecycle != committed.target_lifecycle {
        return stale(VerifierBaselineElementV1::TargetLifecycle);
    }
    if baseline.authority_reference.authority_id != committed.authority_reference.authority_id {
        return stale(VerifierBaselineElementV1::AuthorityIdentity);
    }
    if baseline.authority_reference.authority_revision
        != committed.authority_reference.authority_revision
    {
        return stale(VerifierBaselineElementV1::AuthorityRevision);
    }
    if baseline.authority_reference.authority_digest
        != committed.authority_reference.authority_digest
    {
        return stale(VerifierBaselineElementV1::AuthorityDigest);
    }
    if baseline.audit_contract_reference != committed.audit_contract_reference {
        return stale(VerifierBaselineElementV1::AuditContract);
    }
    if baseline.graph_epoch != committed.graph_epoch {
        return stale(VerifierBaselineElementV1::GraphEpoch);
    }
    if let Some(committed_operation) = committed.committed_operation {
        if committed_operation.operation_id != operation.operation_id {
            return stale(VerifierBaselineElementV1::OperationIdentity);
        }
        return stale(VerifierBaselineElementV1::OperationDigest);
    }
    Ok(VerifierBaselineCheckV1::Fresh)
}

/// Validates one verifier audit baseline against the committed state and maps
/// a stale tuple element to its closed safe failure.
///
/// # Errors
///
/// Returns `ErrorDto::validation` with the stable stale code of the exact
/// element that mismatched.
pub fn validate_baseline_freshness(
    baseline: &VerifierAuditBaselineV1,
    operation: &VerifierOperationIdentityV1,
    committed: &VerifierCommittedStateV1,
) -> DtoResult<VerifierBaselineCheckV1> {
    evaluate_baseline_freshness(baseline, operation, committed).map_err(|failure| {
        ErrorDto::validation(
            failure.code(),
            "the verifier audit baseline is stale; the action fails closed before any effect",
        )
    })
}

/// The closed presence and validity state of the mandatory verifier nested
/// selection in one `VerifierMandate` payload.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerifierMandatorySelectionStateV1 {
    /// The mandatory selection is present, canonical, current, and supported.
    Valid,
    /// The mandatory selection is missing.
    Missing,
    /// The mandatory selection is corrupt.
    Corrupt,
    /// The mandatory selection is stale.
    Stale,
    /// The mandatory selection is unsupported.
    Unsupported,
}

impl VerifierMandatorySelectionStateV1 {
    /// The complete closed selection-state set in canonical order.
    pub const ALL: [Self; 5] = [
        Self::Valid,
        Self::Missing,
        Self::Corrupt,
        Self::Stale,
        Self::Unsupported,
    ];

    /// Returns the closed defect of this state, or `None` when valid.
    #[must_use]
    pub const fn defect(self) -> Option<VerifierSelectionDefectV1> {
        match self {
            Self::Valid => None,
            Self::Missing => Some(VerifierSelectionDefectV1::Missing),
            Self::Corrupt => Some(VerifierSelectionDefectV1::Corrupt),
            Self::Stale => Some(VerifierSelectionDefectV1::Stale),
            Self::Unsupported => Some(VerifierSelectionDefectV1::Unsupported),
        }
    }
}

/// One closed defect of the mandatory verifier nested selection.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerifierSelectionDefectV1 {
    /// The mandatory selection is missing.
    Missing,
    /// The mandatory selection is corrupt.
    Corrupt,
    /// The mandatory selection is stale.
    Stale,
    /// The mandatory selection is unsupported.
    Unsupported,
}

impl VerifierSelectionDefectV1 {
    /// The complete closed selection-defect set in canonical order.
    pub const ALL: [Self; 4] = [Self::Missing, Self::Corrupt, Self::Stale, Self::Unsupported];

    /// Returns the stable machine-readable code of this defect.
    #[must_use]
    pub const fn error_code(self) -> &'static str {
        match self {
            Self::Missing => "verifier_selection_missing",
            Self::Corrupt => "verifier_selection_corrupt",
            Self::Stale => "verifier_selection_stale",
            Self::Unsupported => "verifier_selection_unsupported",
        }
    }

    /// A verifier payload defect never downgrades to `Mandate` or `Ordinary`
    /// execution.
    #[must_use]
    pub const fn permits_downgrade(self) -> bool {
        false
    }
}

/// Validates the mandatory verifier nested selection of one payload.
///
/// A verifier payload with a missing, corrupt, stale, or unsupported
/// mandatory selection cannot downgrade to `Mandate` or `Ordinary`
/// execution.
///
/// # Errors
///
/// Returns `ErrorDto::validation` with the stable defect code when the
/// mandatory selection is not valid.
pub fn validate_verifier_mandatory_selection(
    state: VerifierMandatorySelectionStateV1,
) -> DtoResult<()> {
    state.defect().map_or(Ok(()), |defect| {
        Err(ErrorDto::validation(
            defect.error_code(),
            "a verifier payload with missing, corrupt, stale, or unsupported mandatory selection cannot downgrade to Mandate or Ordinary execution",
        ))
    })
}

/// Validates one exact authority revision against one authority reference.
///
/// # Errors
///
/// Returns a validation error naming the exact mismatched identity, revision,
/// or digest.
pub fn validate_verifier_authority_reference(
    authority: &VerifierAuthorityV1,
    expected: &VerifierAuthorityReferenceV1,
) -> DtoResult<()> {
    if expected.authority_id != authority.authority_id {
        return Err(ErrorDto::validation(
            "verifier_authority_identity_mismatch",
            "the authority reference names a different authority",
        ));
    }
    if expected.authority_revision != authority.authority_revision {
        return Err(ErrorDto::validation(
            "verifier_authority_revision_mismatch",
            "the authority reference names a different revision",
        ));
    }
    if expected.authority_digest != authority.canonical_digest() {
        return Err(ErrorDto::validation(
            "verifier_authority_digest_mismatch",
            "the authority reference digest does not match the exact authority revision",
        ));
    }
    Ok(())
}

/// Validates one delegated authority revision for one exact target operation
/// before any verifier effect.
///
/// The check fails closed on a verifier-identity mismatch, a self-target, a
/// revoked, expired, or consumed lifecycle, an operation outside the closed
/// allowed set, or an audit-contract mismatch. Target-set membership is
/// resolved by the later authority transaction against the referenced
/// immutable target set.
///
/// # Errors
///
/// Returns a validation error naming the first closed check that failed.
pub fn validate_verifier_authority_use(
    authority: &VerifierAuthorityV1,
    verifier_mandate_id: [u8; 16],
    target: &VerifierTargetReferenceV1,
    operation: VerifierOperationV1,
    audit_contract_reference: &VerifierContractReferenceV1,
    now_ms: u64,
) -> DtoResult<()> {
    if authority.verifier_mandate_id != verifier_mandate_id {
        return Err(ErrorDto::validation(
            "verifier_authority_verifier_mismatch",
            "the authority is owned by a different verifier Mandate",
        ));
    }
    if target.target_mandate_id == authority.verifier_mandate_id {
        return Err(ErrorDto::validation(
            "verifier_authority_self_target",
            "a verifier cannot target itself",
        ));
    }
    if authority.lifecycle.is_revoked() {
        return Err(ErrorDto::validation(
            "verifier_authority_revoked",
            "the authority revision is revoked",
        ));
    }
    if authority.lifecycle.is_expired_at(now_ms) {
        return Err(ErrorDto::validation(
            "verifier_authority_expired",
            "the authority revision is expired",
        ));
    }
    if authority.lifecycle.is_consumed() {
        return Err(ErrorDto::validation(
            "verifier_authority_consumed",
            "the authority revision is consumed",
        ));
    }
    if !authority.allows_operation(operation) {
        return Err(ErrorDto::validation(
            "verifier_authority_operation_not_allowed",
            "the authority revision does not allow the requested operation",
        ));
    }
    if authority.audit_contract_reference != *audit_contract_reference {
        return Err(ErrorDto::validation(
            "verifier_authority_contract_mismatch",
            "the requested audit contract is not the authority contract",
        ));
    }
    Ok(())
}

/// The frozen evidence and lifecycle context one verifier operation is
/// validated against before any target mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierOperationPreconditionContextV1 {
    /// Whether the exact authority revision allows this operation.
    pub authority_includes_operation: bool,
    /// The frozen target lifecycle of the audit baseline.
    pub target_lifecycle: VerifierTargetLifecycleV1,
    /// Whether qualifying fail evidence is present.
    pub qualifying_fail_evidence: bool,
    /// Whether unconditional pass evidence is present.
    pub unconditional_pass_evidence: bool,
    /// Whether the target still has unresolved uncertainty.
    pub unresolved_target_uncertainty: bool,
    /// Whether required graph terminalization closure is durable.
    pub graph_terminalization_closed: bool,
    /// Whether the audit contract and evidence prove the reconciliation
    /// standard.
    pub reconciliation_standard_proven: bool,
    /// The unknown-effect reference frozen in the audit baseline.
    pub baseline_unknown_effect_reference: Option<[u8; 16]>,
    /// The exact target uncertainty named by the reconciliation request.
    pub exact_uncertainty_reference: Option<[u8; 16]>,
    /// The closed reconciliation outcome when one is requested.
    pub reconciliation_outcome: Option<VerifierReconciliationOutcomeV1>,
}

impl VerifierOperationPreconditionContextV1 {
    /// A fail-closed context with no authority, no evidence, and no
    /// uncertainty.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            authority_includes_operation: false,
            target_lifecycle: VerifierTargetLifecycleV1::Active,
            qualifying_fail_evidence: false,
            unconditional_pass_evidence: false,
            unresolved_target_uncertainty: false,
            graph_terminalization_closed: false,
            reconciliation_standard_proven: false,
            baseline_unknown_effect_reference: None,
            exact_uncertainty_reference: None,
            reconciliation_outcome: None,
        }
    }
}

/// One closed verifier-operation precondition failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerifierPreconditionFailureV1 {
    /// The authority revision does not include the operation.
    AuthorityMissingOperation,
    /// The target lifecycle does not permit the operation.
    TargetLifecycleInvalid,
    /// Qualifying fail evidence is missing.
    QualifyingFailEvidenceMissing,
    /// Unconditional pass evidence is missing.
    UnconditionalPassEvidenceMissing,
    /// Unresolved target uncertainty blocks the operation.
    UnresolvedTargetUncertainty,
    /// Required graph terminalization closure is missing.
    GraphTerminalizationClosureMissing,
    /// The reconciliation standard is not proven on the unchanged baseline.
    ReconciliationStandardUnproven,
    /// The exact uncertainty does not match the frozen baseline.
    ReconciliationUncertaintyMismatch,
    /// A closed reconciliation outcome is missing.
    ReconciliationOutcomeMissing,
}

impl VerifierPreconditionFailureV1 {
    /// The complete closed precondition-failure set in canonical order.
    pub const ALL: [Self; 9] = [
        Self::AuthorityMissingOperation,
        Self::TargetLifecycleInvalid,
        Self::QualifyingFailEvidenceMissing,
        Self::UnconditionalPassEvidenceMissing,
        Self::UnresolvedTargetUncertainty,
        Self::GraphTerminalizationClosureMissing,
        Self::ReconciliationStandardUnproven,
        Self::ReconciliationUncertaintyMismatch,
        Self::ReconciliationOutcomeMissing,
    ];

    /// Returns the stable machine-readable code of this failure.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::AuthorityMissingOperation => "verifier_authority_operation_missing",
            Self::TargetLifecycleInvalid => "verifier_target_lifecycle_invalid",
            Self::QualifyingFailEvidenceMissing => "verifier_qualifying_fail_evidence_missing",
            Self::UnconditionalPassEvidenceMissing => "verifier_pass_evidence_missing",
            Self::UnresolvedTargetUncertainty => "verifier_target_uncertainty_unresolved",
            Self::GraphTerminalizationClosureMissing => "verifier_graph_closure_missing",
            Self::ReconciliationStandardUnproven => "verifier_reconciliation_standard_unproven",
            Self::ReconciliationUncertaintyMismatch => {
                "verifier_reconciliation_uncertainty_mismatch"
            }
            Self::ReconciliationOutcomeMissing => "verifier_reconciliation_outcome_missing",
        }
    }

    /// Returns the stable safe message of this failure.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::AuthorityMissingOperation => {
                "the authority revision does not include the verifier operation"
            }
            Self::TargetLifecycleInvalid => {
                "the frozen target lifecycle does not permit the verifier operation"
            }
            Self::QualifyingFailEvidenceMissing => {
                "the verifier operation requires qualifying fail evidence"
            }
            Self::UnconditionalPassEvidenceMissing => {
                "the verifier operation requires unconditional pass evidence"
            }
            Self::UnresolvedTargetUncertainty => {
                "unresolved target uncertainty blocks this verifier operation"
            }
            Self::GraphTerminalizationClosureMissing => {
                "the verifier operation requires durable graph terminalization closure"
            }
            Self::ReconciliationStandardUnproven => {
                "the audit contract and evidence do not prove the reconciliation standard"
            }
            Self::ReconciliationUncertaintyMismatch => {
                "the reconciliation uncertainty does not match the frozen baseline"
            }
            Self::ReconciliationOutcomeMissing => "the reconciliation requires one closed outcome",
        }
    }
}

/// The closed, non-authorizing effect one permitted verifier operation
/// produces.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerifierOperationEffectV1 {
    /// The target moves to `NeedsRework`; no trigger and no run is created.
    TargetNeedsRework,
    /// The target moves to `Completed`.
    TargetCompleted,
    /// The target moves to `Stopped` without asserting completion.
    TargetStopped,
    /// A future immutable target revision is created; no admitted run or
    /// historical evidence is rewritten.
    FutureImmutableRevisionCreated,
    /// The target resolves to `Active` for later fresh work with a new run
    /// identity.
    TargetActiveForLaterFreshWork,
}

/// Validates one verifier operation against its closed preconditions.
///
/// Each permitted operation yields its non-authorizing effect; every failed
/// precondition is a typed pre-mutation rejection. `MarkComplete` requires
/// unconditional pass evidence, no unresolved target uncertainty, and
/// required graph terminalization closure. `ReviseFull` requires its own
/// authority and creates only a future immutable revision. `Stop` never
/// asserts completion and never bypasses exact uncertainty reconciliation.
/// `ResolveUnknownEffect` names the exact target uncertainty on the unchanged
/// baseline and yields only `Active` for later fresh work or `Stopped`.
///
/// # Errors
///
/// Returns `ErrorDto::validation` with the stable code of the first failed
/// precondition.
pub fn validate_verifier_operation_preconditions(
    operation: VerifierOperationV1,
    context: &VerifierOperationPreconditionContextV1,
) -> DtoResult<VerifierOperationEffectV1> {
    if !context.authority_includes_operation {
        return Err(precondition_error(
            VerifierPreconditionFailureV1::AuthorityMissingOperation,
        ));
    }
    let outcome = match operation {
        VerifierOperationV1::MarkNeedsRework => validate_mark_needs_rework(context),
        VerifierOperationV1::MarkComplete => validate_mark_complete(context),
        VerifierOperationV1::Stop => validate_stop(context),
        VerifierOperationV1::ReviseFull => validate_revise_full(context),
        VerifierOperationV1::ResolveUnknownEffect => validate_resolve_unknown_effect(context),
    };
    outcome.map_err(precondition_error)
}

/// Validates the `MarkNeedsRework` preconditions.
const fn validate_mark_needs_rework(
    context: &VerifierOperationPreconditionContextV1,
) -> Result<VerifierOperationEffectV1, VerifierPreconditionFailureV1> {
    if !matches!(
        context.target_lifecycle,
        VerifierTargetLifecycleV1::Active | VerifierTargetLifecycleV1::Paused
    ) {
        return Err(VerifierPreconditionFailureV1::TargetLifecycleInvalid);
    }
    if !context.qualifying_fail_evidence {
        return Err(VerifierPreconditionFailureV1::QualifyingFailEvidenceMissing);
    }
    Ok(VerifierOperationEffectV1::TargetNeedsRework)
}

/// Validates the `MarkComplete` preconditions.
const fn validate_mark_complete(
    context: &VerifierOperationPreconditionContextV1,
) -> Result<VerifierOperationEffectV1, VerifierPreconditionFailureV1> {
    if !matches!(
        context.target_lifecycle,
        VerifierTargetLifecycleV1::Active
            | VerifierTargetLifecycleV1::Working
            | VerifierTargetLifecycleV1::Paused
            | VerifierTargetLifecycleV1::NeedsRework
    ) {
        return Err(VerifierPreconditionFailureV1::TargetLifecycleInvalid);
    }
    if !context.unconditional_pass_evidence {
        return Err(VerifierPreconditionFailureV1::UnconditionalPassEvidenceMissing);
    }
    if context.unresolved_target_uncertainty {
        return Err(VerifierPreconditionFailureV1::UnresolvedTargetUncertainty);
    }
    if !context.graph_terminalization_closed {
        return Err(VerifierPreconditionFailureV1::GraphTerminalizationClosureMissing);
    }
    Ok(VerifierOperationEffectV1::TargetCompleted)
}

/// Validates the `Stop` preconditions.
const fn validate_stop(
    context: &VerifierOperationPreconditionContextV1,
) -> Result<VerifierOperationEffectV1, VerifierPreconditionFailureV1> {
    if !matches!(
        context.target_lifecycle,
        VerifierTargetLifecycleV1::Active
            | VerifierTargetLifecycleV1::Working
            | VerifierTargetLifecycleV1::Paused
            | VerifierTargetLifecycleV1::NeedsRework
    ) {
        return Err(VerifierPreconditionFailureV1::TargetLifecycleInvalid);
    }
    if context.unresolved_target_uncertainty {
        return Err(VerifierPreconditionFailureV1::UnresolvedTargetUncertainty);
    }
    Ok(VerifierOperationEffectV1::TargetStopped)
}

/// Validates the `ReviseFull` preconditions.
fn validate_revise_full(
    context: &VerifierOperationPreconditionContextV1,
) -> Result<VerifierOperationEffectV1, VerifierPreconditionFailureV1> {
    if context.target_lifecycle == VerifierTargetLifecycleV1::Archived {
        return Err(VerifierPreconditionFailureV1::TargetLifecycleInvalid);
    }
    Ok(VerifierOperationEffectV1::FutureImmutableRevisionCreated)
}

/// Validates the `ResolveUnknownEffect` preconditions.
fn validate_resolve_unknown_effect(
    context: &VerifierOperationPreconditionContextV1,
) -> Result<VerifierOperationEffectV1, VerifierPreconditionFailureV1> {
    if context.target_lifecycle != VerifierTargetLifecycleV1::PausedAwaitingDecision {
        return Err(VerifierPreconditionFailureV1::TargetLifecycleInvalid);
    }
    if !context.reconciliation_standard_proven {
        return Err(VerifierPreconditionFailureV1::ReconciliationStandardUnproven);
    }
    if context.exact_uncertainty_reference.is_none()
        || context.exact_uncertainty_reference != context.baseline_unknown_effect_reference
    {
        return Err(VerifierPreconditionFailureV1::ReconciliationUncertaintyMismatch);
    }
    match context.reconciliation_outcome {
        Some(VerifierReconciliationOutcomeV1::ActiveForLaterFreshWork) => {
            Ok(VerifierOperationEffectV1::TargetActiveForLaterFreshWork)
        }
        Some(VerifierReconciliationOutcomeV1::Stopped) => {
            Ok(VerifierOperationEffectV1::TargetStopped)
        }
        None => Err(VerifierPreconditionFailureV1::ReconciliationOutcomeMissing),
    }
}

/// Projects one authority into the Goal-facing safe surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationMandateAuthorityDto {
    /// The stable authority identity.
    pub authority_id: [u8; 16],
    /// The exact verifier Mandate that owns the authority.
    pub verifier_mandate_id: [u8; 16],
    /// The immutable authority revision.
    pub authority_revision: u64,
    /// The immutable target-set reference.
    pub target_set_reference: VerifierTargetSetReferenceV1,
    /// The closed allowed operation set.
    pub allowed_operations: Vec<VerifierOperationV1>,
    /// The immutable audit contract reference.
    pub audit_contract_reference: VerifierContractReferenceV1,
    /// The canonical authority digest.
    pub canonical_authority_digest: Digest256,
}

/// Projects one evidence record into the Goal-facing safe surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationAuditEvidenceDto {
    /// The stable evidence identity.
    pub evidence_id: [u8; 16],
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceV1,
    /// The exact target revision.
    pub target_reference: VerifierTargetReferenceV1,
    /// The frozen Goal revisions.
    pub frozen_goal_references: Vec<VerifierGoalReferenceV1>,
    /// The frozen gate and evidence contract revisions.
    pub frozen_gate_evidence_contract_references: Vec<VerifierContractReferenceV1>,
    /// The closed evidence kind.
    pub evidence_kind: VerifierEvidenceKindV1,
    /// The safe retained-content reference; never raw content.
    pub retained_content_reference: [u8; 16],
    /// The canonical evidence digest.
    pub canonical_evidence_digest: Digest256,
}

/// Projects one verdict record into the Goal-facing safe surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationVerdictDto {
    /// The stable verdict identity.
    pub verdict_id: [u8; 16],
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceV1,
    /// The exact target revision.
    pub target_reference: VerifierTargetReferenceV1,
    /// The exact frozen baseline digest.
    pub baseline_digest: Digest256,
    /// The closed verdict.
    pub verdict: VerificationAuditVerdictDto,
    /// The audit evidence references, in declared order.
    pub evidence_references: Vec<[u8; 16]>,
    /// The canonical verdict digest.
    pub canonical_verdict_digest: Digest256,
}

/// Projects one reconciliation record into the Goal-facing safe surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationReconciliationDto {
    /// The stable reconciliation identity.
    pub reconciliation_id: [u8; 16],
    /// The exact authority revision when a verifier reconciled; absent for
    /// user reconciliation.
    pub authority_reference: Option<VerifierAuthorityReferenceV1>,
    /// The exact target revision.
    pub target_reference: VerifierTargetReferenceV1,
    /// The exact frozen baseline digest.
    pub baseline_digest: Digest256,
    /// The exact target unknown-effect reference.
    pub target_unknown_effect_reference: [u8; 16],
    /// The closed reconciliation outcome.
    pub outcome: VerifierReconciliationOutcomeV1,
    /// The canonical reconciliation digest.
    pub canonical_reconciliation_digest: Digest256,
}

/// Encodes one strict list of verifier operations as nested anonymous
/// discriminant records.
///
/// # Errors
///
/// Returns `CanonicalError::DuplicateOrDescendingField` or
/// `CanonicalError::OverLimit` only if the fixed field table were noncanonical
/// or the list exceeded the codec size bounds; both are impossible by
/// construction.
fn encode_operations(operations: &[VerifierOperationV1]) -> Result<Vec<u8>, CanonicalError> {
    let items = operations
        .iter()
        .map(|operation| {
            record(
                0,
                1,
                vec![(1, WireType::U64, encode_u64(operation.discriminant()))],
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(encode_list_items(&items))
}

/// Decodes one strict list of verifier operations.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidTag` for an unknown nested version,
/// `CanonicalError::InvalidField` for an absent, malformed, or unknown
/// discriminant, and other `CanonicalError` values for malformed list
/// framing.
fn decode_operations(bytes: &[u8]) -> Result<Vec<VerifierOperationV1>, CanonicalError> {
    let mut operations = Vec::new();
    for item in decode_list_items(bytes)? {
        let reader = CanonicalRecordReader::new(&item, 1)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let discriminant = decode_u64(
            reader
                .field(1, WireType::U64)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        operations.push(
            VerifierOperationV1::from_discriminant(discriminant)
                .ok_or(CanonicalError::InvalidField)?,
        );
    }
    Ok(operations)
}

/// Encodes one strict list of raw UUID values.
fn encode_uuid_list(items: &[[u8; 16]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + items.len() * 16);
    out.extend_from_slice(&(items.len() as u32).to_be_bytes());
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// Encodes an optional UUID as a one-byte presence marker followed by the
/// sixteen raw bytes when present.
fn encode_optional_uuid(value: Option<[u8; 16]>) -> Vec<u8> {
    value.map_or_else(
        || vec![0],
        |bytes| {
            let mut out = vec![1];
            out.extend_from_slice(&bytes);
            out
        },
    )
}

/// Decodes an optional UUID from its presence-marker framing.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidOptional` when the marker is unknown or
/// the payload length is not sixteen bytes for a present value.
fn decode_optional_uuid(bytes: &[u8]) -> Result<Option<[u8; 16]>, CanonicalError> {
    match bytes.split_first() {
        Some((0, [])) => Ok(None),
        Some((1, rest)) if rest.len() == 16 => Ok(Some(uuid(rest)?)),
        _ => Err(CanonicalError::InvalidOptional),
    }
}

/// Encodes an optional unsigned value as a one-byte presence marker followed
/// by minimal big-endian bytes when present.
fn encode_optional_u64(value: Option<u64>) -> Vec<u8> {
    value.map_or_else(
        || vec![0],
        |number| {
            let mut out = vec![1];
            out.extend_from_slice(&encode_u64(number));
            out
        },
    )
}

/// Decodes an optional unsigned value from its presence-marker framing.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidOptional` when the marker is unknown and
/// the nested value errors of `decode_u64` otherwise.
fn decode_optional_u64(bytes: &[u8]) -> Result<Option<u64>, CanonicalError> {
    match bytes.split_first() {
        Some((0, [])) => Ok(None),
        Some((1, rest)) => Ok(Some(decode_u64(rest)?)),
        _ => Err(CanonicalError::InvalidOptional),
    }
}

/// Encodes an optional authority reference as a one-byte presence marker
/// followed by the nested canonical record bytes when present.
///
/// # Errors
///
/// Returns the nested reference's own encoding errors.
fn encode_optional_authority_reference(
    value: &Option<VerifierAuthorityReferenceV1>,
) -> Result<Vec<u8>, CanonicalError> {
    match value {
        None => Ok(vec![0]),
        Some(reference) => {
            let mut out = vec![1];
            out.extend_from_slice(&reference.encode()?);
            Ok(out)
        }
    }
}

/// Decodes an optional authority reference from its presence-marker framing.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidOptional` when the marker is unknown or a
/// closed marker carries payload bytes, and the nested reference's own
/// decoding errors otherwise.
fn decode_optional_authority_reference(
    bytes: &[u8],
) -> Result<Option<VerifierAuthorityReferenceV1>, CanonicalError> {
    match bytes.split_first() {
        Some((0, [])) => Ok(None),
        Some((1, rest)) => Ok(Some(VerifierAuthorityReferenceV1::decode(rest)?)),
        _ => Err(CanonicalError::InvalidOptional),
    }
}

/// Validates one ordered evidence-reference list with unique keys.
///
/// # Errors
///
/// Returns `CanonicalError::OverLimit` when the list exceeds its code-owned
/// bound and `CanonicalError::InvalidField` for a duplicate reference.
fn validate_evidence_references(references: &[[u8; 16]]) -> Result<(), CanonicalError> {
    if references.len() > VERIFIER_MAX_EVIDENCE_REFERENCES {
        return Err(CanonicalError::OverLimit);
    }
    for (index, reference) in references.iter().enumerate() {
        if references[..index].contains(reference) {
            return Err(CanonicalError::InvalidField);
        }
    }
    Ok(())
}

/// Verifies that one stored record digest matches its identity bytes.
///
/// # Errors
///
/// Returns `CanonicalError::DigestMismatch` when the two digests differ.
fn verify_digest(identity: NamespacedDigest, stored: Digest256) -> Result<(), CanonicalError> {
    if identity.digest == stored {
        Ok(())
    } else {
        Err(CanonicalError::DigestMismatch)
    }
}

/// Returns the fail-closed placeholder digest used while a record's identity
/// digest is computed.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidDigest` only if the fixed thirty-two-byte
/// input were not thirty-two bytes, which is impossible by construction.
fn zero_digest() -> Result<Digest256, CanonicalError> {
    Digest256::from_bytes(&[0; 32])
}

/// Maps one canonical codec failure to a closed safe verifier error.
fn verifier_error(error: CanonicalError, code: &'static str) -> ErrorDto {
    if error == CanonicalError::OverLimit {
        ErrorDto::validation(code, "verifier record exceeds a code-owned bound")
    } else {
        ErrorDto::validation(code, "verifier record is invalid")
    }
}

/// Builds one typed stale-baseline failure.
const fn stale(
    element: VerifierBaselineElementV1,
) -> Result<VerifierBaselineCheckV1, VerifierBaselineStaleV1> {
    Err(VerifierBaselineStaleV1 { element })
}

/// Maps one precondition failure to its closed safe error.
fn precondition_error(failure: VerifierPreconditionFailureV1) -> ErrorDto {
    ErrorDto::validation(failure.code(), failure.message())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test that may fail while it builds or decodes canonical fixture
    /// bytes.
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Returns one presence marker followed by the given payload bytes.
    fn marker_with_payload(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![marker];
        bytes.extend_from_slice(payload);
        bytes
    }

    /// Returns one distinct evidence reference for the given index.
    fn numbered_reference(index: usize) -> [u8; 16] {
        let mut reference = [0u8; 16];
        reference[..8].copy_from_slice(&(index as u64).to_be_bytes());
        reference
    }

    fn authority_reference() -> VerifierAuthorityReferenceV1 {
        VerifierAuthorityReferenceV1 {
            authority_id: [0x11; 16],
            authority_revision: 7,
            authority_digest: Digest256::sha256(b"verifier-authority"),
        }
    }

    fn contract_reference() -> VerifierContractReferenceV1 {
        VerifierContractReferenceV1 {
            contract_id: [0x22; 16],
            contract_revision: 3,
            contract_digest: Digest256::sha256(b"verifier-contract"),
        }
    }

    fn target_reference() -> VerifierTargetReferenceV1 {
        VerifierTargetReferenceV1 {
            target_mandate_id: [0x33; 16],
            target_revision: 5,
        }
    }

    fn target_set_reference() -> VerifierTargetSetReferenceV1 {
        VerifierTargetSetReferenceV1 {
            target_set_id: [0x44; 16],
            target_set_digest: Digest256::sha256(b"verifier-target-set"),
        }
    }

    fn operation_identity() -> VerifierOperationIdentityV1 {
        VerifierOperationIdentityV1 {
            operation_id: [0x55; 16],
            operation_digest: Digest256::sha256(b"verifier-operation"),
        }
    }

    fn goal_reference() -> VerifierGoalReferenceV1 {
        VerifierGoalReferenceV1 {
            goal_id: [0x66; 16],
            goal_revision: 2,
        }
    }

    fn empty_frozen_references() -> VerifierFrozenGoalGateEvidenceReferencesV1 {
        VerifierFrozenGoalGateEvidenceReferencesV1 {
            goal_references: Vec::new(),
            gate_evidence_contract_references: Vec::new(),
        }
    }

    fn lifecycle() -> VerifierAuthorityLifecycleV1 {
        VerifierAuthorityLifecycleV1 {
            issued_at_ms: 1_000,
            expires_at_ms: Some(9_000),
            revoked_at_ms: None,
            revocation_reference: None,
            consumption_rule: VerifierAuthorityConsumptionRuleV1::SingleUse,
            consumption: VerifierAuthorityConsumptionV1::Unconsumed,
        }
    }

    fn authority() -> Result<VerifierAuthorityV1, ErrorDto> {
        VerifierAuthorityV1::new(
            [0x77; 16],
            2,
            [0x88; 16],
            target_set_reference(),
            vec![
                VerifierOperationV1::MarkComplete,
                VerifierOperationV1::MarkNeedsRework,
            ],
            contract_reference(),
            lifecycle(),
        )
    }

    fn baseline() -> Result<VerifierAuditBaselineV1, ErrorDto> {
        VerifierAuditBaselineV1::new(
            authority_reference(),
            2,
            [0x33; 16],
            VerifierTargetRevisionAndSequenceV1 {
                revision: 4,
                aggregate_sequence: 9,
            },
            VerifierTargetLifecycleV1::Active,
            empty_frozen_references(),
            None,
            contract_reference(),
            None,
        )
    }

    fn evidence() -> Result<VerifierAuditEvidenceV1, ErrorDto> {
        VerifierAuditEvidenceV1::new(
            [0x99; 16],
            authority_reference(),
            target_reference(),
            empty_frozen_references(),
            VerifierEvidenceKindV1::UnconditionalPass,
            [0xaa; 16],
        )
    }

    fn verdict_record() -> Result<VerifierAuditVerdictRecordV1, ErrorDto> {
        VerifierAuditVerdictRecordV1::new(
            [0xbb; 16],
            authority_reference(),
            target_reference(),
            Digest256::sha256(b"verifier-baseline"),
            VerificationAuditVerdictDto::Pass,
            vec![[0xcc; 16], [0xdd; 16]],
        )
    }

    fn mutation() -> Result<VerifierTargetMutationV1, ErrorDto> {
        VerifierTargetMutationV1::new(
            [0xee; 16],
            authority_reference(),
            contract_reference(),
            target_reference(),
            VerifierOperationV1::MarkNeedsRework,
            vec![[0xcc; 16]],
            4,
            9,
            Digest256::sha256(b"verifier-baseline"),
            operation_identity(),
        )
    }

    fn reconciliation() -> Result<VerifierReconciliationV1, ErrorDto> {
        VerifierReconciliationV1::new(
            [0x12; 16],
            Some(authority_reference()),
            target_reference(),
            Digest256::sha256(b"verifier-baseline"),
            [0x13; 16],
            VerifierReconciliationOutcomeV1::Stopped,
        )
    }

    /// Returns the complete valid field table of one record: its identity
    /// fields plus the trailing digest field.
    fn complete_fields(
        mut fields: Vec<(u32, WireType, Vec<u8>)>,
        digest_field: u32,
        digest: Digest256,
    ) -> Vec<(u32, WireType, Vec<u8>)> {
        fields.push((digest_field, WireType::Digest, digest.bytes().to_vec()));
        fields
    }

    /// Returns one framing-valid payload that the field's own decoder must
    /// reject and the exact error it must report.
    fn malformed_payload(wire_type: WireType) -> (Vec<u8>, CanonicalError) {
        match wire_type {
            WireType::U64 => (vec![0xff; 9], CanonicalError::InvalidField),
            WireType::Uuid => (vec![0xff; 5], CanonicalError::InvalidField),
            WireType::Digest => (vec![0xff; 5], CanonicalError::InvalidDigest),
            WireType::Record => (vec![0xff; 2], CanonicalError::Truncated),
            WireType::List => (vec![0xff; 2], CanonicalError::Truncated),
            WireType::Optional => (vec![0, 0], CanonicalError::InvalidOptional),
            WireType::Bool | WireType::Utf8 | WireType::Bytes => {
                (vec![0xff; 2], CanonicalError::InvalidField)
            }
        }
    }

    /// Asserts that every field of one record rejects a malformed payload in
    /// its own nested decoder.
    fn assert_every_field_rejects_its_own_malformed_payload<T>(
        valid_fields: &[(u32, WireType, Vec<u8>)],
        decode: fn(&[u8]) -> Result<T, CanonicalError>,
    ) -> TestResult
    where
        T: std::fmt::Debug + PartialEq,
    {
        for (index, (number, wire_type, _)) in valid_fields.iter().enumerate() {
            let (payload, expected) = malformed_payload(*wire_type);
            let mut fields = valid_fields.to_vec();
            fields[index].2 = payload;
            let bytes = record(0, 1, fields)?;
            assert_eq!(
                decode(&bytes),
                Err(expected),
                "field {number} of wire type {wire_type:?} must reject its malformed payload"
            );
        }
        Ok(())
    }

    #[test]
    fn target_lifecycle_discriminants_terminality_and_decoders_are_stable() {
        for (index, lifecycle) in VerifierTargetLifecycleV1::ALL.iter().enumerate() {
            assert_eq!(lifecycle.discriminant(), index as u64);
            assert_eq!(
                VerifierTargetLifecycleV1::from_discriminant(index as u64),
                Some(*lifecycle)
            );
        }
        assert_eq!(VerifierTargetLifecycleV1::from_discriminant(9), None);
        assert!(VerifierTargetLifecycleV1::Completed.is_terminal());
        assert!(VerifierTargetLifecycleV1::Stopped.is_terminal());
        assert!(VerifierTargetLifecycleV1::Archived.is_terminal());
        assert!(!VerifierTargetLifecycleV1::Draft.is_terminal());
        assert!(!VerifierTargetLifecycleV1::Active.is_terminal());
        assert!(!VerifierTargetLifecycleV1::Working.is_terminal());
        assert!(!VerifierTargetLifecycleV1::Paused.is_terminal());
        assert!(!VerifierTargetLifecycleV1::PausedAwaitingDecision.is_terminal());
        assert!(!VerifierTargetLifecycleV1::NeedsRework.is_terminal());
    }

    #[test]
    fn evidence_kind_discriminants_codes_and_decoders_are_stable() {
        let codes = [
            "unconditional_pass",
            "qualifying_fail",
            "inconclusive",
            "graph_terminalization_closure",
            "reconciliation_standard_proof",
        ];
        for (index, kind) in VerifierEvidenceKindV1::ALL.iter().enumerate() {
            assert_eq!(kind.discriminant(), index as u64);
            assert_eq!(kind.code(), codes[index]);
            assert_eq!(
                VerifierEvidenceKindV1::from_discriminant(index as u64),
                Some(*kind)
            );
        }
        assert_eq!(VerifierEvidenceKindV1::from_discriminant(5), None);
    }

    #[test]
    fn audit_verdict_discriminants_codes_and_decoders_are_stable() {
        let codes = [
            "pass",
            "fail",
            "inconclusive",
            "target_revision_stale",
            "target_unavailable",
            "verifier_unavailable",
            "verifier_external_effect_unknown",
        ];
        for (index, verdict) in VerificationAuditVerdictDto::ALL.iter().enumerate() {
            assert_eq!(verdict.discriminant(), index as u64);
            assert_eq!(verdict.code(), codes[index]);
            assert_eq!(
                VerificationAuditVerdictDto::from_discriminant(index as u64),
                Some(*verdict)
            );
        }
        assert_eq!(VerificationAuditVerdictDto::from_discriminant(7), None);
    }

    #[test]
    fn consumption_rule_and_reconciliation_outcome_discriminants_fail_closed() {
        assert_eq!(
            VerifierAuthorityConsumptionRuleV1::SingleUse.discriminant(),
            0
        );
        assert_eq!(
            VerifierAuthorityConsumptionRuleV1::ReusableWhileActive.discriminant(),
            1
        );
        assert_eq!(
            VerifierAuthorityConsumptionRuleV1::from_discriminant(0),
            Some(VerifierAuthorityConsumptionRuleV1::SingleUse)
        );
        assert_eq!(
            VerifierAuthorityConsumptionRuleV1::from_discriminant(1),
            Some(VerifierAuthorityConsumptionRuleV1::ReusableWhileActive)
        );
        assert_eq!(
            VerifierAuthorityConsumptionRuleV1::from_discriminant(2),
            None
        );

        for (index, outcome) in VerifierReconciliationOutcomeV1::ALL.iter().enumerate() {
            assert_eq!(outcome.discriminant(), index as u64);
            assert_eq!(
                VerifierReconciliationOutcomeV1::from_discriminant(index as u64),
                Some(*outcome)
            );
        }
        assert_eq!(VerifierReconciliationOutcomeV1::from_discriminant(2), None);
    }

    #[test]
    fn optional_uuid_and_u64_framing_reject_unknown_markers_and_payloads() {
        assert_eq!(decode_optional_uuid(&[0]), Ok(None));
        assert_eq!(
            decode_optional_uuid(&marker_with_payload(1, &[0xaa; 16])),
            Ok(Some([0xaa; 16]))
        );
        assert_eq!(
            decode_optional_uuid(&[]),
            Err(CanonicalError::InvalidOptional)
        );
        assert_eq!(
            decode_optional_uuid(&[0, 0]),
            Err(CanonicalError::InvalidOptional)
        );
        assert_eq!(
            decode_optional_uuid(&[2]),
            Err(CanonicalError::InvalidOptional)
        );
        assert_eq!(
            decode_optional_uuid(&marker_with_payload(1, &[0xaa; 5])),
            Err(CanonicalError::InvalidOptional)
        );

        assert_eq!(decode_optional_u64(&[0]), Ok(None));
        assert_eq!(decode_optional_u64(&[1, 0x00]), Ok(Some(0)));
        assert_eq!(
            decode_optional_u64(&marker_with_payload(1, &[0xff; 9])),
            Err(CanonicalError::InvalidField)
        );
        assert_eq!(
            decode_optional_u64(&[]),
            Err(CanonicalError::InvalidOptional)
        );
        assert_eq!(
            decode_optional_u64(&[0, 0]),
            Err(CanonicalError::InvalidOptional)
        );
        assert_eq!(decode_optional_u64(&[1]), Err(CanonicalError::InvalidField));
    }

    #[test]
    fn optional_authority_reference_framing_rejects_unknown_markers() -> TestResult {
        let reference = authority_reference();
        assert_eq!(encode_optional_authority_reference(&None)?, vec![0]);
        let encoded = encode_optional_authority_reference(&Some(reference))?;
        assert_eq!(encoded.first(), Some(&1u8));
        assert_eq!(
            decode_optional_authority_reference(&encoded)?,
            Some(reference)
        );
        assert_eq!(
            decode_optional_authority_reference(&[]),
            Err(CanonicalError::InvalidOptional)
        );
        assert_eq!(
            decode_optional_authority_reference(&[0, 0]),
            Err(CanonicalError::InvalidOptional)
        );
        assert_eq!(
            decode_optional_authority_reference(&[1]),
            Err(CanonicalError::Truncated)
        );
        Ok(())
    }

    #[test]
    fn evidence_reference_lists_enforce_their_bound_and_unique_keys() {
        assert_eq!(validate_evidence_references(&[]), Ok(()));
        let at_bound: Vec<[u8; 16]> = (0..VERIFIER_MAX_EVIDENCE_REFERENCES)
            .map(numbered_reference)
            .collect();
        assert_eq!(validate_evidence_references(&at_bound), Ok(()));

        let over_bound: Vec<[u8; 16]> = (0..=VERIFIER_MAX_EVIDENCE_REFERENCES)
            .map(numbered_reference)
            .collect();
        assert_eq!(
            validate_evidence_references(&over_bound),
            Err(CanonicalError::OverLimit)
        );
        assert_eq!(
            validate_evidence_references(&[[0x01; 16], [0x01; 16]]),
            Err(CanonicalError::InvalidField)
        );
    }

    #[test]
    fn operation_list_codec_rejects_unknown_frames_and_malformed_items() -> TestResult {
        let operations = VerifierOperationV1::ALL.to_vec();
        assert_eq!(
            decode_operations(&encode_operations(&operations)?)?,
            operations
        );

        let wrong_frame =
            encode_list_items(&[record(1, 1, vec![(1, WireType::U64, encode_u64(0))])?]);
        assert_eq!(
            decode_operations(&wrong_frame),
            Err(CanonicalError::InvalidTag)
        );

        let malformed_value =
            encode_list_items(&[record(0, 1, vec![(1, WireType::U64, vec![0xff; 9])])?]);
        assert_eq!(
            decode_operations(&malformed_value),
            Err(CanonicalError::InvalidField)
        );

        let missing_value = encode_list_items(&[record(0, 1, Vec::new())?]);
        assert_eq!(
            decode_operations(&missing_value),
            Err(CanonicalError::InvalidField)
        );

        let unknown_operation =
            encode_list_items(&[record(0, 1, vec![(1, WireType::U64, encode_u64(5))])?]);
        assert_eq!(
            decode_operations(&unknown_operation),
            Err(CanonicalError::InvalidField)
        );

        assert_eq!(
            decode_operations(&[0xff; 2]),
            Err(CanonicalError::Truncated)
        );
        Ok(())
    }

    #[test]
    fn stale_failure_keeps_its_exact_element_and_fail_closed_recovery() {
        let failure = VerifierBaselineStaleV1::new(VerifierBaselineElementV1::GraphEpoch);
        assert_eq!(failure.element, VerifierBaselineElementV1::GraphEpoch);
        assert_eq!(failure.code(), "verifier_baseline_stale_graph_epoch");
        assert_eq!(
            failure.recovery(),
            VerifierFailClosedRecoveryV1::FAIL_CLOSED
        );
    }

    #[test]
    fn verifier_records_reject_bytes_over_their_encoded_bound() {
        let oversized = vec![0u8; MAX_VERIFIER_RECORD_BYTES + 1];
        assert_eq!(
            VerifierAuthorityV1::decode(&oversized),
            Err(CanonicalError::OverLimit)
        );
        assert_eq!(
            VerifierAuditBaselineV1::decode(&oversized),
            Err(CanonicalError::OverLimit)
        );
        assert_eq!(
            VerifierAuditEvidenceV1::decode(&oversized),
            Err(CanonicalError::OverLimit)
        );
        assert_eq!(
            VerifierAuditVerdictRecordV1::decode(&oversized),
            Err(CanonicalError::OverLimit)
        );
        assert_eq!(
            VerifierTargetMutationV1::decode(&oversized),
            Err(CanonicalError::OverLimit)
        );
        assert_eq!(
            VerifierReconciliationV1::decode(&oversized),
            Err(CanonicalError::OverLimit)
        );
    }

    #[test]
    fn authority_lifecycle_decode_rejects_every_field_fault() -> TestResult {
        let lifecycle = lifecycle();
        let fields = vec![
            (1, WireType::U64, encode_u64(lifecycle.issued_at_ms)),
            (
                2,
                WireType::Optional,
                encode_optional_u64(lifecycle.expires_at_ms),
            ),
            (
                3,
                WireType::Optional,
                encode_optional_u64(lifecycle.revoked_at_ms),
            ),
            (
                4,
                WireType::Optional,
                encode_optional_uuid(lifecycle.revocation_reference),
            ),
            (
                5,
                WireType::U64,
                encode_u64(lifecycle.consumption_rule.discriminant()),
            ),
            (6, WireType::Record, lifecycle.consumption.encode()?),
        ];
        assert_eq!(
            VerifierAuthorityLifecycleV1::decode(&record(0, 1, fields.clone())?)?,
            lifecycle
        );
        assert_eq!(
            VerifierAuthorityLifecycleV1::decode(&record(1, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &fields,
            VerifierAuthorityLifecycleV1::decode,
        )
    }

    #[test]
    fn authority_consumption_decode_rejects_every_field_fault() -> TestResult {
        let unconsumed = VerifierAuthorityConsumptionV1::Unconsumed;
        assert_eq!(
            VerifierAuthorityConsumptionV1::decode(&unconsumed.encode()?)?,
            unconsumed
        );
        let consumed = VerifierAuthorityConsumptionV1::Consumed {
            mutation_reference: [0x42; 16],
        };
        assert_eq!(
            VerifierAuthorityConsumptionV1::decode(&consumed.encode()?)?,
            consumed
        );

        assert_eq!(
            VerifierAuthorityConsumptionV1::decode(&record(4, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_eq!(
            VerifierAuthorityConsumptionV1::decode(&record(
                0,
                1,
                vec![(1, WireType::U64, vec![0xff; 9])],
            )?),
            Err(CanonicalError::InvalidField)
        );
        assert_eq!(
            VerifierAuthorityConsumptionV1::decode(&record(
                0,
                1,
                vec![
                    (1, WireType::U64, encode_u64(1)),
                    (2, WireType::Uuid, vec![0xff; 5]),
                ],
            )?),
            Err(CanonicalError::InvalidField)
        );
        assert_eq!(
            VerifierAuthorityConsumptionV1::decode(&record(
                0,
                1,
                vec![(1, WireType::U64, encode_u64(2))],
            )?),
            Err(CanonicalError::InvalidField)
        );
        Ok(())
    }

    #[test]
    fn small_verifier_references_reject_every_field_fault() -> TestResult {
        let target_set = target_set_reference();
        let target_set_fields = vec![
            (1, WireType::Uuid, target_set.target_set_id.to_vec()),
            (
                2,
                WireType::Digest,
                target_set.target_set_digest.bytes().to_vec(),
            ),
        ];
        assert_eq!(
            VerifierTargetSetReferenceV1::decode(&record(0, 1, target_set_fields.clone())?)?,
            target_set
        );
        assert_eq!(
            VerifierTargetSetReferenceV1::decode(&record(1, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &target_set_fields,
            VerifierTargetSetReferenceV1::decode,
        )?;

        let contract = contract_reference();
        let contract_fields = vec![
            (1, WireType::Uuid, contract.contract_id.to_vec()),
            (2, WireType::U64, encode_u64(contract.contract_revision)),
            (
                3,
                WireType::Digest,
                contract.contract_digest.bytes().to_vec(),
            ),
        ];
        assert_eq!(
            VerifierContractReferenceV1::decode(&record(0, 1, contract_fields.clone())?)?,
            contract
        );
        assert_eq!(
            VerifierContractReferenceV1::decode(&record(1, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &contract_fields,
            VerifierContractReferenceV1::decode,
        )?;

        let authority = authority_reference();
        let authority_fields = vec![
            (1, WireType::Uuid, authority.authority_id.to_vec()),
            (2, WireType::U64, encode_u64(authority.authority_revision)),
            (
                3,
                WireType::Digest,
                authority.authority_digest.bytes().to_vec(),
            ),
        ];
        assert_eq!(
            VerifierAuthorityReferenceV1::decode(&record(0, 1, authority_fields.clone())?)?,
            authority
        );
        assert_eq!(
            VerifierAuthorityReferenceV1::decode(&record(1, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &authority_fields,
            VerifierAuthorityReferenceV1::decode,
        )?;

        let target = target_reference();
        let target_fields = vec![
            (1, WireType::Uuid, target.target_mandate_id.to_vec()),
            (2, WireType::U64, encode_u64(target.target_revision)),
        ];
        assert_eq!(
            VerifierTargetReferenceV1::decode(&record(0, 1, target_fields.clone())?)?,
            target
        );
        assert_eq!(
            VerifierTargetReferenceV1::decode(&record(1, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &target_fields,
            VerifierTargetReferenceV1::decode,
        )
    }

    #[test]
    fn revision_sequence_and_goal_references_reject_every_field_fault() -> TestResult {
        let sequence = VerifierTargetRevisionAndSequenceV1 {
            revision: 4,
            aggregate_sequence: 9,
        };
        let sequence_fields = vec![
            (1, WireType::U64, encode_u64(sequence.revision)),
            (2, WireType::U64, encode_u64(sequence.aggregate_sequence)),
        ];
        assert_eq!(
            VerifierTargetRevisionAndSequenceV1::decode(&record(0, 1, sequence_fields.clone())?)?,
            sequence
        );
        assert_eq!(
            VerifierTargetRevisionAndSequenceV1::decode(&record(1, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &sequence_fields,
            VerifierTargetRevisionAndSequenceV1::decode,
        )?;

        let goal = goal_reference();
        let goal_fields = vec![
            (1, WireType::Uuid, goal.goal_id.to_vec()),
            (2, WireType::U64, encode_u64(goal.goal_revision)),
        ];
        assert_eq!(
            VerifierGoalReferenceV1::decode(&record(0, 1, goal_fields.clone())?)?,
            goal
        );
        assert_eq!(
            VerifierGoalReferenceV1::decode(&record(1, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &goal_fields,
            VerifierGoalReferenceV1::decode,
        )
    }

    #[test]
    fn frozen_reference_lists_reject_every_field_fault() -> TestResult {
        let frozen = VerifierFrozenGoalGateEvidenceReferencesV1 {
            goal_references: vec![goal_reference()],
            gate_evidence_contract_references: vec![contract_reference()],
        };
        let fields = vec![
            (
                1,
                WireType::List,
                encode_list_items(&[goal_reference().encode()?]),
            ),
            (
                2,
                WireType::List,
                encode_list_items(&[contract_reference().encode()?]),
            ),
        ];
        assert_eq!(
            VerifierFrozenGoalGateEvidenceReferencesV1::decode(&record(0, 1, fields.clone())?)?,
            frozen
        );
        assert_eq!(
            VerifierFrozenGoalGateEvidenceReferencesV1::decode(&record(1, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &fields,
            VerifierFrozenGoalGateEvidenceReferencesV1::decode,
        )
    }

    #[test]
    fn nested_references_reject_zero_revisions() -> TestResult {
        let zero_contract = record(
            0,
            1,
            vec![
                (1, WireType::Uuid, vec![0x22; 16]),
                (2, WireType::U64, encode_u64(0)),
                (
                    3,
                    WireType::Digest,
                    Digest256::sha256(b"verifier-contract").bytes().to_vec(),
                ),
            ],
        )?;
        assert_eq!(
            VerifierContractReferenceV1 {
                contract_id: [0x22; 16],
                contract_revision: 0,
                contract_digest: Digest256::sha256(b"verifier-contract"),
            }
            .validate(),
            Err(CanonicalError::InvalidField)
        );
        assert_eq!(
            VerifierContractReferenceV1::decode(&zero_contract),
            Err(CanonicalError::InvalidField)
        );

        let zero_authority = record(
            0,
            1,
            vec![
                (1, WireType::Uuid, vec![0x11; 16]),
                (2, WireType::U64, encode_u64(0)),
                (
                    3,
                    WireType::Digest,
                    Digest256::sha256(b"verifier-authority").bytes().to_vec(),
                ),
            ],
        )?;
        assert_eq!(
            VerifierAuthorityReferenceV1 {
                authority_id: [0x11; 16],
                authority_revision: 0,
                authority_digest: Digest256::sha256(b"verifier-authority"),
            }
            .validate(),
            Err(CanonicalError::InvalidField)
        );
        assert_eq!(
            VerifierAuthorityReferenceV1::decode(&zero_authority),
            Err(CanonicalError::InvalidField)
        );

        let zero_target = record(
            0,
            1,
            vec![
                (1, WireType::Uuid, vec![0x33; 16]),
                (2, WireType::U64, encode_u64(0)),
            ],
        )?;
        assert_eq!(
            VerifierTargetReferenceV1 {
                target_mandate_id: [0x33; 16],
                target_revision: 0,
            }
            .validate(),
            Err(CanonicalError::InvalidField)
        );
        assert_eq!(
            VerifierTargetReferenceV1::decode(&zero_target),
            Err(CanonicalError::InvalidField)
        );

        let zero_goal = record(
            0,
            1,
            vec![
                (1, WireType::Uuid, vec![0x66; 16]),
                (2, WireType::U64, encode_u64(0)),
            ],
        )?;
        assert_eq!(
            VerifierGoalReferenceV1 {
                goal_id: [0x66; 16],
                goal_revision: 0,
            }
            .validate(),
            Err(CanonicalError::InvalidField)
        );
        assert_eq!(
            VerifierGoalReferenceV1::decode(&zero_goal),
            Err(CanonicalError::InvalidField)
        );
        Ok(())
    }

    #[test]
    fn authority_record_decode_rejects_every_field_fault() -> TestResult {
        let authority = authority()?;
        let fields = complete_fields(
            authority.identity_fields()?,
            8,
            authority.canonical_digest(),
        );
        assert_eq!(record(0, 1, fields.clone())?, authority.encode()?);
        assert_eq!(
            VerifierAuthorityV1::decode(&authority.encode()?)?,
            authority
        );
        assert_eq!(
            VerifierAuthorityV1::decode(&record(9, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(&fields, VerifierAuthorityV1::decode)
    }

    #[test]
    fn authority_validation_rejects_zero_revision_and_uncanonical_operation_order() -> TestResult {
        let authority = authority()?;
        let fields = complete_fields(
            authority.identity_fields()?,
            8,
            authority.canonical_digest(),
        );

        let mut zero_revision = fields.clone();
        zero_revision[1].2 = encode_u64(0);
        assert_eq!(
            VerifierAuthorityV1::decode(&record(0, 1, zero_revision)?),
            Err(CanonicalError::InvalidField)
        );

        let mut unordered_operations = fields;
        unordered_operations[4].2 = encode_list_items(&[
            record(0, 1, vec![(1, WireType::U64, encode_u64(1))])?,
            record(0, 1, vec![(1, WireType::U64, encode_u64(0))])?,
        ]);
        assert_eq!(
            VerifierAuthorityV1::decode(&record(0, 1, unordered_operations)?),
            Err(CanonicalError::InvalidField)
        );
        Ok(())
    }

    #[test]
    fn baseline_record_decode_rejects_every_field_fault() -> TestResult {
        let baseline = baseline()?;
        let fields = complete_fields(baseline.identity_fields()?, 10, baseline.canonical_digest());
        assert_eq!(record(0, 1, fields.clone())?, baseline.encode()?);
        assert_eq!(
            VerifierAuditBaselineV1::decode(&baseline.encode()?)?,
            baseline
        );
        assert_eq!(
            VerifierAuditBaselineV1::decode(&record(9, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &fields,
            VerifierAuditBaselineV1::decode,
        )?;

        let mut zero_mandate_revision = fields;
        zero_mandate_revision[1].2 = encode_u64(0);
        assert_eq!(
            VerifierAuditBaselineV1::decode(&record(0, 1, zero_mandate_revision)?),
            Err(CanonicalError::InvalidField)
        );
        Ok(())
    }

    #[test]
    fn evidence_record_decode_rejects_every_field_fault() -> TestResult {
        let evidence = evidence()?;
        let fields = complete_fields(evidence.identity_fields()?, 7, evidence.canonical_digest());
        assert_eq!(record(0, 1, fields.clone())?, evidence.encode()?);
        assert_eq!(
            VerifierAuditEvidenceV1::decode(&evidence.encode()?)?,
            evidence
        );
        assert_eq!(
            VerifierAuditEvidenceV1::decode(&record(9, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &fields,
            VerifierAuditEvidenceV1::decode,
        )
    }

    #[test]
    fn verdict_record_decode_rejects_every_field_fault() -> TestResult {
        let verdict = verdict_record()?;
        let fields = complete_fields(verdict.identity_fields()?, 7, verdict.canonical_digest());
        assert_eq!(record(0, 1, fields.clone())?, verdict.encode()?);
        assert_eq!(
            VerifierAuditVerdictRecordV1::decode(&verdict.encode()?)?,
            verdict
        );
        assert_eq!(
            VerifierAuditVerdictRecordV1::decode(&record(9, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &fields,
            VerifierAuditVerdictRecordV1::decode,
        )
    }

    #[test]
    fn mutation_record_decode_rejects_every_field_fault() -> TestResult {
        let mutation = mutation()?;
        let fields = complete_fields(mutation.identity_fields()?, 11, mutation.canonical_digest());
        assert_eq!(record(0, 1, fields.clone())?, mutation.encode()?);
        assert_eq!(
            VerifierTargetMutationV1::decode(&mutation.encode()?)?,
            mutation
        );
        assert_eq!(
            VerifierTargetMutationV1::decode(&record(9, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &fields,
            VerifierTargetMutationV1::decode,
        )?;

        let mut zero_expected_revision = fields;
        zero_expected_revision[6].2 = encode_u64(0);
        assert_eq!(
            VerifierTargetMutationV1::decode(&record(0, 1, zero_expected_revision)?),
            Err(CanonicalError::InvalidField)
        );
        Ok(())
    }

    #[test]
    fn operation_identity_decode_rejects_every_field_fault() -> TestResult {
        let identity = operation_identity();
        let fields = vec![
            (1, WireType::Uuid, identity.operation_id.to_vec()),
            (
                2,
                WireType::Digest,
                identity.operation_digest.bytes().to_vec(),
            ),
        ];
        assert_eq!(
            VerifierOperationIdentityV1::decode(&record(0, 1, fields.clone())?)?,
            identity
        );
        assert_eq!(
            VerifierOperationIdentityV1::decode(&record(4, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &fields,
            VerifierOperationIdentityV1::decode,
        )
    }

    #[test]
    fn reconciliation_record_decode_rejects_every_field_fault() -> TestResult {
        let reconciliation = reconciliation()?;
        let fields = complete_fields(
            reconciliation.identity_fields()?,
            7,
            reconciliation.canonical_digest(),
        );
        assert_eq!(record(0, 1, fields.clone())?, reconciliation.encode()?);
        assert_eq!(
            VerifierReconciliationV1::decode(&reconciliation.encode()?)?,
            reconciliation
        );
        assert_eq!(
            VerifierReconciliationV1::decode(&record(9, 1, Vec::new())?),
            Err(CanonicalError::InvalidTag)
        );
        assert_every_field_rejects_its_own_malformed_payload(
            &fields,
            VerifierReconciliationV1::decode,
        )
    }
}
