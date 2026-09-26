//! Durable Goal-domain repository contracts for architecture 28.
//!
//! Every type below is a DTO-only durable record for the Slice 3 Goal surface
//! activated by ADR 0044: Goal identities and immutable revisions, reference
//! and executable gate records, typed gate templates and results, memory
//! cards, Skill and role cards, coalesced refinement proposals,
//! conversation-summary compaction records, and the unified verification
//! authority, baseline, evidence, verdict, and target-mutation records.
//! Identities are canonical daemon-assigned text values, content is bounded
//! credential-free safe data, and digests are canonical
//! `sha256:<64 lowercase hex>` text.
//!
//! Goal bodies, transcripts, gate command lines, workspace paths, grants,
//! credentials, provider resources, and verifier evidence payloads never cross
//! this boundary; they are represented only by their safe digest, byte size,
//! and bounded typed references, and a credential-shaped value is rejected
//! before it can reach a row.

use intention_domain::goal_domain::{GOAL_MAX_FULL_RECORD_BYTES, GOAL_MAX_GATES_PER_GOAL};
use intention_domain::verification::{
    VERIFIER_MAX_EVIDENCE_REFERENCES, VERIFIER_MAX_FROZEN_CONTRACT_REFERENCES,
    VERIFIER_MAX_FROZEN_GOAL_REFERENCES,
};
use intention_types::DtoResult;

use crate::{validate_safe_digest, validate_safe_label, validate_safe_labels};

/// The maximum page size of one durable Goal-domain read.
pub const MAX_GOAL_RECORD_PAGE: u64 = 64;
/// The maximum count of selected parent-chain references.
pub const MAX_GOAL_PARENT_CHAIN: usize = 16;
/// The maximum count of explicit session links of one project Goal.
pub const MAX_GOAL_SESSION_LINKS: usize = 64;
/// The maximum count of required gate references of one Goal revision.
pub const MAX_GOAL_REQUIRED_GATES: usize = GOAL_MAX_GATES_PER_GOAL;
/// The maximum count of rule references of one Goal revision.
pub const MAX_GOAL_RULE_REFERENCES: usize = 32;
/// The maximum count of selected card references of one Goal.
pub const MAX_GOAL_CARD_REFERENCES: usize = 32;
/// The maximum count of exact evidence references of one Goal record.
pub const MAX_GOAL_EVIDENCE_REFERENCES: usize = VERIFIER_MAX_EVIDENCE_REFERENCES;
/// The maximum count of typed edits of one refinement draft.
pub const MAX_GOAL_DRAFT_EDITS: usize = 128;
/// The maximum byte size of one Goal-domain safe text value (512 KiB).
pub const MAX_GOAL_SAFE_TEXT_BYTES: usize = GOAL_MAX_FULL_RECORD_BYTES as usize;
/// The maximum count of tool identifiers of one role card.
pub const MAX_GOAL_ROLE_TOOLS: usize = 16;
/// The maximum count of frozen Goal references of one verifier record.
pub const MAX_VERIFIER_FROZEN_GOAL_REFERENCES: usize = VERIFIER_MAX_FROZEN_GOAL_REFERENCES;
/// The maximum count of frozen contract references of one verifier record.
pub const MAX_VERIFIER_FROZEN_CONTRACT_REFERENCES: usize = VERIFIER_MAX_FROZEN_CONTRACT_REFERENCES;
/// The maximum count of evidence references of one verifier verdict or
/// mutation.
pub const MAX_VERIFIER_EVIDENCE_REFERENCES: usize = VERIFIER_MAX_EVIDENCE_REFERENCES;

/// Validates one bounded credential-free Goal-domain safe text value.
///
/// # Errors
///
/// Returns `invalid_code` for a blank, control-bearing, or credential-shaped
/// value and `too_large_code` for a value over `max_bytes`.
fn validate_goal_text(
    invalid_code: &'static str,
    too_large_code: &'static str,
    value: &str,
    max_bytes: usize,
) -> DtoResult<()> {
    if value.trim().is_empty() || intention_domain::canonical::contains_control_or_nul(value) {
        return Err(intention_types::ErrorDto::validation(
            invalid_code,
            "Goal-domain text must be non-blank and control-free",
        ));
    }
    if value.len() > max_bytes {
        return Err(intention_types::ErrorDto::validation(
            too_large_code,
            "Goal-domain text exceeds its byte bound",
        ));
    }
    if intention_domain::canonical::contains_credential_shape(value) {
        return Err(intention_types::ErrorDto::validation(
            "credentials_forbidden",
            "credentials are forbidden",
        ));
    }
    Ok(())
}

/// Validates one ordered exact evidence set with unique identity keys.
///
/// # Errors
///
/// Returns `invalid_code` for an inexact or duplicated reference and
/// `too_large_code` when the set bound is exceeded.
fn validate_evidence_set(
    invalid_code: &'static str,
    too_large_code: &'static str,
    evidence: &[GoalEvidenceReferenceDto],
    max: usize,
) -> DtoResult<()> {
    if evidence.len() > max {
        return Err(intention_types::ErrorDto::validation(
            too_large_code,
            "the evidence set exceeds its closed bound",
        ));
    }
    let mut seen: Vec<&str> = Vec::new();
    for reference in evidence {
        reference.validate(invalid_code)?;
        if seen.contains(&reference.evidence_id.as_str()) {
            return Err(intention_types::ErrorDto::validation(
                invalid_code,
                "an evidence set names one revision of any evidence identity",
            ));
        }
        seen.push(&reference.evidence_id);
    }
    Ok(())
}

/// The exactly-one scope of one Goal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GoalScopeDto {
    /// A project Goal; it reaches a session only through an explicit link.
    Project {
        /// The owning project identity.
        project_id: String,
    },
    /// A session Goal; it belongs only to its one session.
    Session {
        /// The owning project identity.
        project_id: String,
        /// The owning session identity.
        session_id: String,
    },
}

impl GoalScopeDto {
    /// Returns the owning project identity.
    #[must_use]
    pub fn project_id(&self) -> &str {
        match self {
            Self::Project { project_id } | Self::Session { project_id, .. } => project_id,
        }
    }
    /// Returns the owning session identity of a session Goal.
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Project { .. } => None,
            Self::Session { session_id, .. } => Some(session_id),
        }
    }
    /// Returns whether this is a project Goal.
    #[must_use]
    pub const fn is_project(&self) -> bool {
        matches!(self, Self::Project { .. })
    }
    /// Returns the stable durable scope discriminator.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Project { .. } => "project",
            Self::Session { .. } => "session",
        }
    }
    /// Validates this Goal scope identity.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_active` for a blank, over-long, control-bearing, or
    /// credential-shaped identity.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("goal_not_active", self.project_id())?;
        if let Some(session_id) = self.session_id() {
            validate_safe_label("goal_not_active", session_id)?;
        }
        Ok(())
    }
}

/// The exactly-one durable owner scope of one memory, Skill, or card record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GoalRecordScopeDto {
    /// A project record.
    Project {
        /// The owning project identity.
        project_id: String,
    },
    /// A Goal record.
    Goal {
        /// The owning Goal identity.
        goal_id: String,
    },
    /// A session record that stays in its session.
    Session {
        /// The owning session identity.
        session_id: String,
    },
}

impl GoalRecordScopeDto {
    /// Returns the stable durable scope discriminator.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Project { .. } => "project",
            Self::Goal { .. } => "goal",
            Self::Session { .. } => "session",
        }
    }
    /// Returns the single owner identity of this scope.
    #[must_use]
    pub fn owner_id(&self) -> &str {
        match self {
            Self::Project { project_id } => project_id,
            Self::Goal { goal_id } => goal_id,
            Self::Session { session_id } => session_id,
        }
    }
    /// Validates this owner scope identity.
    ///
    /// # Errors
    ///
    /// Returns `invalid_code` for a blank, over-long, control-bearing, or
    /// credential-shaped identity.
    pub fn validate(&self, invalid_code: &'static str) -> DtoResult<()> {
        validate_safe_label(invalid_code, self.owner_id())
    }
}

/// The closed Goal work lifecycle states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalLifecycleStateDto {
    /// The Goal accepts ordinary and verification work.
    Active,
    /// A required gate failed; only a new run restores work.
    NeedsRework,
    /// The user paused the Goal and its non-terminal subtree.
    Paused,
    /// The user stopped the Goal; stop is terminal.
    Stopped,
    /// The terminal Goal is archived as readable history.
    Archived,
}

impl GoalLifecycleStateDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::NeedsRework => "needs_rework",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
            Self::Archived => "archived",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_active` for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "active" => Ok(Self::Active),
            "needs_rework" => Ok(Self::NeedsRework),
            "paused" => Ok(Self::Paused),
            "stopped" => Ok(Self::Stopped),
            "archived" => Ok(Self::Archived),
            _ => Err(intention_types::ErrorDto::validation(
                "goal_not_active",
                "the durable Goal lifecycle state is unknown",
            )),
        }
    }
    /// Returns whether no further work lifecycle transition is valid.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Stopped | Self::Archived)
    }
}

/// The closed kinds of durable reference accepted as gate evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalEvidenceKindDto {
    /// A terminal child Goal user-decision result.
    TerminalChildResult,
    /// An accepted user declaration.
    AcceptedUserDeclaration,
    /// A terminal registered-tool result.
    TerminalRegisteredToolResult,
    /// An executable gate result with its distinct provenance.
    ExecutableGateResult,
}

impl GoalEvidenceKindDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::TerminalChildResult => "terminal_child_result",
            Self::AcceptedUserDeclaration => "accepted_user_declaration",
            Self::TerminalRegisteredToolResult => "terminal_registered_tool_result",
            Self::ExecutableGateResult => "executable_gate_result",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_failed` for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "terminal_child_result" => Ok(Self::TerminalChildResult),
            "accepted_user_declaration" => Ok(Self::AcceptedUserDeclaration),
            "terminal_registered_tool_result" => Ok(Self::TerminalRegisteredToolResult),
            "executable_gate_result" => Ok(Self::ExecutableGateResult),
            _ => Err(intention_types::ErrorDto::validation(
                "goal_gate_failed",
                "the durable gate evidence kind is unknown",
            )),
        }
    }
}

/// The closed exception kinds a user may accept.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalGateExceptionKindDto {
    /// A known failed gate.
    Failed,
    /// A known unavailable gate or template.
    Unavailable,
    /// An expired gate evidence revision.
    Expired,
    /// A gate whose external effect is ambiguous.
    ExternallyAmbiguous,
}

impl GoalGateExceptionKindDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::Unavailable => "unavailable",
            Self::Expired => "expired",
            Self::ExternallyAmbiguous => "externally_ambiguous",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `goal_acceptance_exception_invalid` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "failed" => Ok(Self::Failed),
            "unavailable" => Ok(Self::Unavailable),
            "expired" => Ok(Self::Expired),
            "externally_ambiguous" => Ok(Self::ExternallyAmbiguous),
            _ => Err(intention_types::ErrorDto::validation(
                "goal_acceptance_exception_invalid",
                "the durable gate exception kind is unknown",
            )),
        }
    }
}

/// One exact selected successful evidence reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalEvidenceReferenceDto {
    /// The daemon-assigned evidence identity.
    pub evidence_id: String,
    /// The exact immutable evidence revision.
    pub revision: u64,
    /// The closed evidence kind.
    pub kind: GoalEvidenceKindDto,
}

impl GoalEvidenceReferenceDto {
    /// Validates this exact evidence reference.
    ///
    /// # Errors
    ///
    /// Returns `invalid_code` for a blank identity, a zero revision, or a
    /// credential-shaped or over-long identity.
    pub fn validate(&self, invalid_code: &'static str) -> DtoResult<()> {
        validate_safe_label(invalid_code, &self.evidence_id)?;
        if self.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                invalid_code,
                "an evidence reference is exact and nonzero",
            ));
        }
        Ok(())
    }
}

/// One recorded gate exception with its safe evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalGateExceptionDto {
    /// The gate identity.
    pub gate_id: String,
    /// The exact gate revision.
    pub gate_revision: u64,
    /// The closed exception kind.
    pub kind: GoalGateExceptionKindDto,
    /// The safe evidence reference of the exception.
    pub evidence: GoalEvidenceReferenceDto,
}

impl GoalGateExceptionDto {
    /// Validates this recorded exception.
    ///
    /// # Errors
    ///
    /// Returns `goal_acceptance_exception_invalid` for an inexact gate or
    /// evidence reference.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("goal_acceptance_exception_invalid", &self.gate_id)?;
        if self.gate_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "goal_acceptance_exception_invalid",
                "an exception names an exact nonzero gate revision",
            ));
        }
        self.evidence.validate("goal_acceptance_exception_invalid")
    }
}

/// The technical readiness of one Goal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GoalReadinessStateDto {
    /// The exact evidence set is incomplete.
    NotReady,
    /// Every required element has selected successful evidence.
    Ready {
        /// The selected successful evidence references.
        verified_evidence_set: Vec<GoalEvidenceReferenceDto>,
    },
}

impl GoalReadinessStateDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::NotReady => "not_ready",
            Self::Ready { .. } => "ready",
        }
    }
    /// Validates this readiness state.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_ready` for an empty or inexact evidence set,
    /// `goal_limit_exceeded` when the evidence bound is exceeded, and
    /// `goal_revision_conflict` for a duplicated evidence identity.
    pub fn validate(&self) -> DtoResult<()> {
        let Self::Ready {
            verified_evidence_set,
        } = self
        else {
            return Ok(());
        };
        if verified_evidence_set.is_empty() {
            return Err(intention_types::ErrorDto::validation(
                "goal_not_ready",
                "Ready requires at least one selected successful evidence reference",
            ));
        }
        validate_evidence_set(
            "goal_not_ready",
            "goal_limit_exceeded",
            verified_evidence_set,
            MAX_GOAL_EVIDENCE_REFERENCES,
        )
    }
    /// Returns the selected successful evidence references.
    #[must_use]
    pub fn verified_evidence_set(&self) -> &[GoalEvidenceReferenceDto] {
        match self {
            Self::NotReady => &[],
            Self::Ready {
                verified_evidence_set,
            } => verified_evidence_set,
        }
    }
    /// Returns whether this Goal claims technical readiness.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

/// The user decision on one Goal, separate from work state and readiness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GoalUserDecisionStateDto {
    /// The user has not decided.
    Unaccepted,
    /// The user accepted a Ready Goal.
    Accepted,
    /// The user accepted with an explicit gate exception set.
    AcceptedWithException {
        /// The accepted exception evidence set.
        exception_evidence_set: Vec<GoalGateExceptionDto>,
    },
}

impl GoalUserDecisionStateDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Unaccepted => "unaccepted",
            Self::Accepted => "accepted",
            Self::AcceptedWithException { .. } => "accepted_with_exception",
        }
    }
    /// Validates this user-decision state.
    ///
    /// # Errors
    ///
    /// Returns `goal_acceptance_exception_invalid` for an empty, inexact, or
    /// duplicated-gate exception set, `goal_gate_limit_exceeded` when the gate
    /// bound is exceeded, and `credentials_forbidden` for credential-shaped
    /// identities.
    pub fn validate(&self) -> DtoResult<()> {
        let Self::AcceptedWithException {
            exception_evidence_set,
        } = self
        else {
            return Ok(());
        };
        if exception_evidence_set.is_empty() {
            return Err(intention_types::ErrorDto::validation(
                "goal_acceptance_exception_invalid",
                "an exception set names at least one known gate exception",
            ));
        }
        if exception_evidence_set.len() > GOAL_MAX_GATES_PER_GOAL {
            return Err(intention_types::ErrorDto::validation(
                "goal_gate_limit_exceeded",
                "the exception set exceeds the gate bound of one Goal",
            ));
        }
        let mut seen: Vec<&str> = Vec::new();
        for exception in exception_evidence_set {
            exception.validate()?;
            if seen.contains(&exception.gate_id.as_str()) {
                return Err(intention_types::ErrorDto::validation(
                    "goal_acceptance_exception_invalid",
                    "an exception set names each gate once",
                ));
            }
            seen.push(&exception.gate_id);
        }
        Ok(())
    }
    /// Returns whether this decision is terminal for a parent component.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        !matches!(self, Self::Unaccepted)
    }
    /// Returns the accepted exception evidence set.
    #[must_use]
    pub fn exception_evidence_set(&self) -> &[GoalGateExceptionDto] {
        match self {
            Self::AcceptedWithException {
                exception_evidence_set,
            } => exception_evidence_set,
            Self::Unaccepted | Self::Accepted => &[],
        }
    }
}

/// One durable Goal identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalRecordDto {
    /// The daemon-assigned Goal identity.
    pub goal_id: String,
    /// The exactly-one Goal scope.
    pub scope: GoalScopeDto,
    /// The current active immutable revision.
    pub active_revision: u64,
    /// The closed work lifecycle state.
    pub lifecycle_state: GoalLifecycleStateDto,
    /// The technical readiness state.
    pub readiness_state: GoalReadinessStateDto,
    /// The separate user-decision state.
    pub user_decision_state: GoalUserDecisionStateDto,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
    /// The last durable Goal update time in Unix milliseconds.
    pub updated_at_ms: u64,
}

impl GoalRecordDto {
    /// Validates this durable Goal identity.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_active` for a blank identity, the scope validation
    /// failures, `goal_revision_conflict` for a zero active revision, and the
    /// readiness and user-decision validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("goal_not_active", &self.goal_id)?;
        self.scope.validate()?;
        if self.active_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "goal_revision_conflict",
                "a Goal starts at active revision one",
            ));
        }
        self.readiness_state.validate()?;
        self.user_decision_state.validate()
    }
}

/// One immutable Goal revision record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalRevisionRecordDto {
    /// The Goal identity.
    pub goal_id: String,
    /// The immutable revision number, starting at one.
    pub revision: u64,
    /// The bounded safe title.
    pub title: String,
    /// The bounded safe objective.
    pub objective: String,
    /// The inherited rule references of the exact parent revision.
    pub inherited_rule_references: Vec<String>,
    /// The local rule references added by this revision.
    pub local_rule_references: Vec<String>,
    /// The required gate revision references.
    pub required_gate_references: Vec<GoalGateRevisionReferenceDto>,
    /// The canonical revision digest.
    pub canonical_revision_digest: String,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl GoalRevisionRecordDto {
    /// Validates this immutable Goal revision.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_active` for a missing Goal or rule identity,
    /// `goal_revision_conflict` for a zero revision or an inexact gate
    /// reference, `goal_gate_limit_exceeded` when the gate bound is exceeded,
    /// `goal_limit_exceeded` for oversized text, and `credentials_forbidden`
    /// for credential-shaped text.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("goal_not_active", &self.goal_id)?;
        if self.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "goal_revision_conflict",
                "a Goal revision number is positive",
            ));
        }
        validate_goal_text(
            "goal_revision_conflict",
            "goal_limit_exceeded",
            &self.title,
            MAX_GOAL_SAFE_TEXT_BYTES,
        )?;
        validate_goal_text(
            "goal_revision_conflict",
            "goal_limit_exceeded",
            &self.objective,
            MAX_GOAL_SAFE_TEXT_BYTES,
        )?;
        validate_safe_digest("goal_revision_conflict", &self.canonical_revision_digest)?;
        if self.required_gate_references.len() > MAX_GOAL_REQUIRED_GATES {
            return Err(intention_types::ErrorDto::validation(
                "goal_gate_limit_exceeded",
                "the required gate bound of one Goal is exceeded",
            ));
        }
        let mut seen: Vec<&str> = Vec::new();
        for reference in &self.required_gate_references {
            reference.validate()?;
            if seen.contains(&reference.gate_id.as_str()) {
                return Err(intention_types::ErrorDto::validation(
                    "goal_revision_conflict",
                    "a revision names one revision of any required gate identity",
                ));
            }
            seen.push(&reference.gate_id);
        }
        validate_safe_labels(
            "goal_not_active",
            &self.inherited_rule_references,
            MAX_GOAL_RULE_REFERENCES,
        )?;
        validate_safe_labels(
            "goal_not_active",
            &self.local_rule_references,
            MAX_GOAL_RULE_REFERENCES,
        )
    }
}

/// One immutable reference to an exact gate revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalGateRevisionReferenceDto {
    /// The referenced gate identity.
    pub gate_id: String,
    /// The exact referenced revision.
    pub revision: u64,
}

impl GoalGateRevisionReferenceDto {
    /// Validates this exact gate revision reference.
    ///
    /// # Errors
    ///
    /// Returns `goal_revision_conflict` for a blank identity or a zero
    /// revision.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("goal_revision_conflict", &self.gate_id)?;
        if self.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "goal_revision_conflict",
                "a gate revision reference is exact and nonzero",
            ));
        }
        Ok(())
    }
}

/// Input creating one durable Goal and its first immutable revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateGoalInputDto {
    /// The durable Goal identity.
    pub goal: GoalRecordDto,
    /// The first immutable revision bound to that Goal.
    pub revision: GoalRevisionRecordDto,
}

impl CreateGoalInputDto {
    /// Validates this creation input as one coherent Goal.
    ///
    /// # Errors
    ///
    /// Returns the record validation failures and `goal_revision_conflict`
    /// when the revision does not belong to the Goal or is not revision one.
    pub fn validate(&self) -> DtoResult<()> {
        self.goal.validate()?;
        self.revision.validate()?;
        if self.revision.goal_id != self.goal.goal_id
            || self.revision.revision != 1
            || self.goal.active_revision != 1
        {
            return Err(intention_types::ErrorDto::validation(
                "goal_revision_conflict",
                "a new Goal starts at its own revision one",
            ));
        }
        Ok(())
    }
}

/// Input continuing one durable Goal with the next immutable revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendGoalRevisionInputDto {
    /// The owning Goal identity.
    pub goal_id: String,
    /// The exact active revision the caller observed.
    pub expected_revision: u64,
    /// The next immutable revision to append and activate.
    pub revision: GoalRevisionRecordDto,
}

impl AppendGoalRevisionInputDto {
    /// Validates this revision append.
    ///
    /// # Errors
    ///
    /// Returns `goal_revision_conflict` when the append does not continue the
    /// exact observed active revision of the same Goal.
    pub fn validate(&self) -> DtoResult<()> {
        self.revision.validate()?;
        let next = self.expected_revision.checked_add(1).ok_or_else(|| {
            intention_types::ErrorDto::validation(
                "goal_revision_conflict",
                "the active revision cannot be continued",
            )
        })?;
        if self.revision.goal_id != self.goal_id || self.revision.revision != next {
            return Err(intention_types::ErrorDto::validation(
                "goal_revision_conflict",
                "the append does not continue the active Goal revision",
            ));
        }
        Ok(())
    }
}

/// Input applying one closed lifecycle transition to a durable Goal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransitionGoalLifecycleInputDto {
    /// The owning Goal identity.
    pub goal_id: String,
    /// The exact active revision the caller observed.
    pub expected_revision: u64,
    /// The requested closed lifecycle state.
    pub lifecycle_state: GoalLifecycleStateDto,
    /// The durable transition time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// Input recording one readiness claim against an exact Goal revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetGoalReadinessInputDto {
    /// The owning Goal identity.
    pub goal_id: String,
    /// The exact active revision the claim observed.
    pub expected_revision: u64,
    /// The validated readiness state.
    pub readiness_state: GoalReadinessStateDto,
    /// The durable claim time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// Input recording one user decision against an exact Goal revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordGoalUserDecisionInputDto {
    /// The owning Goal identity.
    pub goal_id: String,
    /// The exact active revision the decision observed.
    pub expected_revision: u64,
    /// The validated user-decision state.
    pub user_decision_state: GoalUserDecisionStateDto,
    /// The durable decision time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One obligatory parent-to-child Goal link.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalParentLinkRecordDto {
    /// The parent Goal identity.
    pub parent_goal_id: String,
    /// The child Goal identity.
    pub child_goal_id: String,
    /// The exact child revision at link time.
    pub child_revision_at_link: u64,
    /// The canonical link digest.
    pub canonical_link_digest: String,
    /// The durable link time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl GoalParentLinkRecordDto {
    /// Validates this obligatory parent link.
    ///
    /// # Errors
    ///
    /// Returns `goal_cycle_detected` for a self-link or blank identity,
    /// `goal_revision_conflict` for a zero child revision, and the digest
    /// validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("goal_cycle_detected", &self.parent_goal_id)?;
        validate_safe_label("goal_cycle_detected", &self.child_goal_id)?;
        validate_safe_digest("goal_cycle_detected", &self.canonical_link_digest)?;
        if self.parent_goal_id == self.child_goal_id {
            return Err(intention_types::ErrorDto::validation(
                "goal_cycle_detected",
                "a Goal is never its own parent or child",
            ));
        }
        if self.child_revision_at_link == 0 {
            return Err(intention_types::ErrorDto::validation(
                "goal_revision_conflict",
                "a child link names an exact nonzero child revision",
            ));
        }
        Ok(())
    }
}

/// Input attaching one obligatory child to a durable parent Goal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachGoalChildInputDto {
    /// The parent Goal identity.
    pub parent_goal_id: String,
    /// The child Goal identity.
    pub child_goal_id: String,
    /// The exact child revision at link time.
    pub child_revision_at_link: u64,
    /// The canonical link digest.
    pub canonical_link_digest: String,
    /// The durable link time in Unix milliseconds.
    pub created_at_ms: u64,
}

/// One explicit durable project-Goal-to-session link.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalSessionLinkRecordDto {
    /// The daemon-assigned Goal link identity.
    pub link_id: String,
    /// The linked project Goal identity.
    pub project_goal_id: String,
    /// The linked session identity.
    pub session_id: String,
    /// The revision from which the link is effective.
    pub effective_from_revision: u64,
    /// The canonical link digest.
    pub canonical_link_digest: String,
    /// The durable link time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl GoalSessionLinkRecordDto {
    /// Validates this explicit session link.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_active` for a blank identity,
    /// `goal_revision_conflict` for a zero effective revision, and the digest
    /// validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("goal_not_active", &self.link_id)?;
        validate_safe_label("goal_not_active", &self.project_goal_id)?;
        validate_safe_label("goal_not_active", &self.session_id)?;
        validate_safe_digest("goal_not_active", &self.canonical_link_digest)?;
        if self.effective_from_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "goal_revision_conflict",
                "a session link is effective from an exact nonzero revision",
            ));
        }
        Ok(())
    }
}

/// Input creating one explicit durable session link.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateGoalSessionLinkInputDto {
    /// The link identity to record.
    pub link: GoalSessionLinkRecordDto,
}

/// The closed verification gate definitions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GoalGateDefinitionDto {
    /// A gate over an exact durable reference.
    Reference {
        /// The exact evidence contract revision.
        evidence_contract_revision: u64,
        /// The accepted durable reference kinds.
        accepted_reference_kinds: Vec<GoalEvidenceKindDto>,
    },
    /// A gate over one user-created typed template revision.
    Executable {
        /// The template identity.
        template_id: String,
        /// The exact template revision.
        template_revision: u64,
    },
}

impl GoalGateDefinitionDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Reference { .. } => "reference",
            Self::Executable { .. } => "executable",
        }
    }
    /// Returns whether this gate executes a user-created template.
    #[must_use]
    pub const fn is_executable(&self) -> bool {
        matches!(self, Self::Executable { .. })
    }
    /// Validates this gate definition.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` for a missing contract revision, empty
    /// or duplicated accepted kinds, or a missing template identity or
    /// revision.
    pub fn validate(&self) -> DtoResult<()> {
        match self {
            Self::Reference {
                evidence_contract_revision,
                accepted_reference_kinds,
            } => {
                if *evidence_contract_revision == 0 || accepted_reference_kinds.is_empty() {
                    return Err(intention_types::ErrorDto::validation(
                        "goal_gate_unavailable",
                        "a reference gate requires an exact evidence contract and accepted kinds",
                    ));
                }
                let mut seen = Vec::new();
                for kind in accepted_reference_kinds {
                    if seen.contains(kind) {
                        return Err(intention_types::ErrorDto::validation(
                            "goal_gate_unavailable",
                            "a reference gate names each accepted evidence kind once",
                        ));
                    }
                    seen.push(*kind);
                }
                Ok(())
            }
            Self::Executable {
                template_id,
                template_revision,
            } => {
                validate_safe_label("goal_gate_unavailable", template_id)?;
                if *template_revision == 0 {
                    return Err(intention_types::ErrorDto::validation(
                        "goal_gate_unavailable",
                        "an executable gate requires an exact nonzero template revision",
                    ));
                }
                Ok(())
            }
        }
    }
}

/// One durable user-created gate definition with its revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalGateRecordDto {
    /// The daemon-assigned gate identity.
    pub gate_id: String,
    /// The owning Goal identity.
    pub goal_id: String,
    /// The closed gate definition.
    pub definition: GoalGateDefinitionDto,
    /// The current immutable gate revision.
    pub revision: u64,
    /// The canonical revision digest.
    pub canonical_revision_digest: String,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl GoalGateRecordDto {
    /// Validates this durable gate record.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` for a missing identity or definition,
    /// `goal_revision_conflict` for a zero revision, and the digest
    /// validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("goal_gate_unavailable", &self.gate_id)?;
        validate_safe_label("goal_gate_unavailable", &self.goal_id)?;
        validate_safe_digest("goal_gate_unavailable", &self.canonical_revision_digest)?;
        if self.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "goal_revision_conflict",
                "a gate revision starts at one",
            ));
        }
        self.definition.validate()
    }
}

/// Input creating one durable gate with its first revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateGoalGateInputDto {
    /// The durable gate identity.
    pub gate: GoalGateRecordDto,
}

/// Input appending one immutable gate revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendGoalGateRevisionInputDto {
    /// The owning gate identity.
    pub gate_id: String,
    /// The exact gate revision the caller observed.
    pub expected_revision: u64,
    /// The next immutable definition to append and activate.
    pub definition: GoalGateDefinitionDto,
    /// The canonical revision digest.
    pub canonical_revision_digest: String,
    /// The durable append time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

impl AppendGoalGateRevisionInputDto {
    /// Validates this gate revision append.
    ///
    /// # Errors
    ///
    /// Returns `goal_revision_conflict` when the append cannot continue the
    /// exact observed gate revision and `goal_gate_unavailable` for an
    /// unusable definition or digest.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("goal_revision_conflict", &self.gate_id)?;
        validate_safe_digest("goal_revision_conflict", &self.canonical_revision_digest)?;
        self.definition.validate()?;
        self.expected_revision.checked_add(1).ok_or_else(|| {
            intention_types::ErrorDto::validation(
                "goal_revision_conflict",
                "the active gate revision cannot be continued",
            )
        })?;
        Ok(())
    }
}

/// The closed template scope families.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GoalTemplateScopeDto {
    /// A project-scoped template.
    Project {
        /// The owning project identity.
        project_id: String,
    },
    /// A Goal-scoped template.
    Goal {
        /// The owning Goal identity.
        goal_id: String,
    },
    /// A session-scoped template.
    Session {
        /// The owning session identity.
        session_id: String,
    },
}

impl GoalTemplateScopeDto {
    /// Returns the stable durable scope discriminator.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Project { .. } => "project",
            Self::Goal { .. } => "goal",
            Self::Session { .. } => "session",
        }
    }
    /// Returns the single owner identity of this scope.
    #[must_use]
    pub fn owner_id(&self) -> &str {
        match self {
            Self::Project { project_id } => project_id,
            Self::Goal { goal_id } => goal_id,
            Self::Session { session_id } => session_id,
        }
    }
    /// Validates this template scope.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` for a blank, over-long,
    /// control-bearing, or credential-shaped identity.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("goal_gate_unavailable", self.owner_id())
    }
}

/// The closed typed input families of a gate template.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalGateInputFamilyDto {
    /// One bounded closed text input.
    ClosedTextV1,
    /// One exact set of workspace-relative logical paths.
    ClosedPathSetV1,
}

impl GoalGateInputFamilyDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ClosedTextV1 => "closed_text_v1",
            Self::ClosedPathSetV1 => "closed_path_set_v1",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "closed_text_v1" => Ok(Self::ClosedTextV1),
            "closed_path_set_v1" => Ok(Self::ClosedPathSetV1),
            _ => Err(intention_types::ErrorDto::validation(
                "goal_gate_unavailable",
                "the durable gate template input family is unknown",
            )),
        }
    }
}

/// The closed gate-template lifecycle states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalTemplateLifecycleStateDto {
    /// The template may be selected by future gates.
    Enabled,
    /// The template is archived and readable history.
    Archived,
}

impl GoalTemplateLifecycleStateDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Archived => "archived",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "enabled" => Ok(Self::Enabled),
            "archived" => Ok(Self::Archived),
            _ => Err(intention_types::ErrorDto::validation(
                "goal_gate_unavailable",
                "the durable gate template lifecycle state is unknown",
            )),
        }
    }
}

/// The provenance of one gate template creation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalTemplateProvenanceKindDto {
    /// The user created the template directly.
    User,
    /// The model prepared a user-confirmed template proposal.
    ModelProposal,
}

impl GoalTemplateProvenanceKindDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::ModelProposal => "model_proposal",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "user" => Ok(Self::User),
            "model_proposal" => Ok(Self::ModelProposal),
            _ => Err(intention_types::ErrorDto::validation(
                "goal_gate_unavailable",
                "the durable gate template provenance kind is unknown",
            )),
        }
    }
}

/// One user-created immutable gate template card.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalGateTemplateRecordDto {
    /// The template identity.
    pub template_id: String,
    /// The immutable template revision.
    pub revision: u64,
    /// The template scope.
    pub scope: GoalTemplateScopeDto,
    /// The registered capability reference.
    pub capability_reference: String,
    /// The one closed typed input family.
    pub input_family: GoalGateInputFamilyDto,
    /// Whether execution requires explicit user confirmation.
    pub requires_confirmation: bool,
    /// The closed template lifecycle state.
    pub lifecycle_state: GoalTemplateLifecycleStateDto,
    /// The provenance of the creation.
    pub provenance_kind: GoalTemplateProvenanceKindDto,
    /// The accepted refinement draft identity of a model proposal.
    pub provenance_draft_id: Option<String>,
    /// Whether the user explicitly confirmed a model proposal.
    pub provenance_accepted_by_user: bool,
    /// The canonical card digest.
    pub canonical_digest: String,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl GoalGateTemplateRecordDto {
    /// Validates this gate template card.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` for a missing identity, revision,
    /// capability, or scope, and for an unconfirmed or malformed model
    /// proposal.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("goal_gate_unavailable", &self.template_id)?;
        validate_safe_label("goal_gate_unavailable", &self.capability_reference)?;
        validate_safe_digest("goal_gate_unavailable", &self.canonical_digest)?;
        if self.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "goal_gate_unavailable",
                "a gate template requires an exact nonzero revision",
            ));
        }
        self.scope.validate()?;
        match self.provenance_kind {
            GoalTemplateProvenanceKindDto::User => {
                if self.provenance_draft_id.is_some() {
                    return Err(intention_types::ErrorDto::validation(
                        "goal_gate_unavailable",
                        "a directly user-created template names no proposal draft",
                    ));
                }
                Ok(())
            }
            GoalTemplateProvenanceKindDto::ModelProposal => {
                let Some(draft_id) = self.provenance_draft_id.as_deref() else {
                    return Err(intention_types::ErrorDto::validation(
                        "goal_gate_unavailable",
                        "a model-prepared template proposal names its accepted draft",
                    ));
                };
                validate_safe_label("goal_gate_unavailable", draft_id)?;
                if !self.provenance_accepted_by_user {
                    return Err(intention_types::ErrorDto::validation(
                        "goal_gate_unavailable",
                        "a model-prepared template proposal requires explicit user confirmation",
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Input applying one closed lifecycle transition to a gate template.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransitionGoalGateTemplateInputDto {
    /// The template identity.
    pub template_id: String,
    /// The exact template revision.
    pub revision: u64,
    /// The requested closed lifecycle state.
    pub lifecycle_state: GoalTemplateLifecycleStateDto,
    /// The durable transition time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// The closed outcome kinds of one gate evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalGateOutcomeKindDto {
    /// The gate passed with its selected evidence.
    Passed,
    /// The gate failed with its safe evidence.
    Failed,
    /// The gate exceeded its time budget.
    TimedOut,
    /// The gate was cancelled with its run.
    Cancelled,
    /// The gate exceeded its output bound.
    OutputBoundExceeded,
    /// The started external effect could not be proven.
    ExternalEffectUnknown,
    /// The exact durable reference is missing.
    ReferenceUnavailable,
    /// The selected revision is stale.
    RevisionStale,
    /// The executable template is unavailable.
    TemplateUnavailable,
}

impl GoalGateOutcomeKindDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::OutputBoundExceeded => "output_bound_exceeded",
            Self::ExternalEffectUnknown => "external_effect_unknown",
            Self::ReferenceUnavailable => "reference_unavailable",
            Self::RevisionStale => "revision_stale",
            Self::TemplateUnavailable => "template_unavailable",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_failed` for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "passed" => Ok(Self::Passed),
            "failed" => Ok(Self::Failed),
            "timed_out" => Ok(Self::TimedOut),
            "cancelled" => Ok(Self::Cancelled),
            "output_bound_exceeded" => Ok(Self::OutputBoundExceeded),
            "external_effect_unknown" => Ok(Self::ExternalEffectUnknown),
            "reference_unavailable" => Ok(Self::ReferenceUnavailable),
            "revision_stale" => Ok(Self::RevisionStale),
            "template_unavailable" => Ok(Self::TemplateUnavailable),
            _ => Err(intention_types::ErrorDto::validation(
                "goal_gate_failed",
                "the durable gate outcome kind is unknown",
            )),
        }
    }
    /// Returns the closed disposition of this outcome.
    #[must_use]
    pub const fn disposition(self) -> GoalGateOutcomeDispositionDto {
        match self {
            Self::Passed => GoalGateOutcomeDispositionDto::Passed,
            Self::Failed | Self::TimedOut | Self::Cancelled | Self::OutputBoundExceeded => {
                GoalGateOutcomeDispositionDto::Failed
            }
            Self::ReferenceUnavailable | Self::RevisionStale | Self::TemplateUnavailable => {
                GoalGateOutcomeDispositionDto::Unavailable
            }
            Self::ExternalEffectUnknown => GoalGateOutcomeDispositionDto::UnknownEffect,
        }
    }
    /// Returns whether this outcome requires an exact evidence reference.
    #[must_use]
    pub const fn requires_evidence(self) -> bool {
        matches!(
            self,
            Self::Passed | Self::Failed | Self::ExternalEffectUnknown
        )
    }
}

/// The closed disposition of one gate outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalGateOutcomeDispositionDto {
    /// The gate passed.
    Passed,
    /// The gate did not pass definitively.
    Failed,
    /// The gate could not be evaluated.
    Unavailable,
    /// The gate's external effect is unknown.
    UnknownEffect,
}

impl GoalGateOutcomeDispositionDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Unavailable => "unavailable",
            Self::UnknownEffect => "unknown_effect",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_failed` for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "passed" => Ok(Self::Passed),
            "failed" => Ok(Self::Failed),
            "unavailable" => Ok(Self::Unavailable),
            "unknown_effect" => Ok(Self::UnknownEffect),
            _ => Err(intention_types::ErrorDto::validation(
                "goal_gate_failed",
                "the durable gate outcome disposition is unknown",
            )),
        }
    }
}

/// One durable gate evaluation result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalGateResultRecordDto {
    /// The evaluated gate identity.
    pub gate_id: String,
    /// The exact evaluated gate revision.
    pub gate_revision: u64,
    /// The producing run identity.
    pub producing_run_id: String,
    /// The closed outcome kind.
    pub outcome_kind: GoalGateOutcomeKindDto,
    /// The closed outcome disposition.
    pub disposition: GoalGateOutcomeDispositionDto,
    /// The selected safe evidence of a passing, failing, or unknown outcome.
    pub evidence: Option<GoalEvidenceReferenceDto>,
    /// The canonical result digest.
    pub canonical_result_digest: String,
    /// The durable evaluation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

impl GoalGateResultRecordDto {
    /// Validates this gate result.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_failed` for an incoherent outcome, disposition, or
    /// evidence binding, `goal_revision_conflict` for a zero gate revision,
    /// and the digest validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("goal_gate_failed", &self.gate_id)?;
        validate_safe_label("goal_gate_failed", &self.producing_run_id)?;
        validate_safe_digest("goal_gate_failed", &self.canonical_result_digest)?;
        if self.gate_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "goal_revision_conflict",
                "a gate result names an exact nonzero gate revision",
            ));
        }
        if self.disposition != self.outcome_kind.disposition() {
            return Err(intention_types::ErrorDto::validation(
                "goal_gate_failed",
                "a gate result disposition must match its outcome kind",
            ));
        }
        match (&self.evidence, self.outcome_kind.requires_evidence()) {
            (Some(evidence), true) => evidence.validate("goal_gate_failed"),
            (None, false) => Ok(()),
            (Some(_), false) => Err(intention_types::ErrorDto::validation(
                "goal_gate_failed",
                "this gate outcome carries no evidence reference",
            )),
            (None, true) => Err(intention_types::ErrorDto::validation(
                "goal_gate_failed",
                "a passing, failing, or unknown outcome requires exact evidence",
            )),
        }
    }
}

/// The closed kinds of durable memory records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryKindDto {
    /// A durable fact.
    Fact,
    /// A durable decision.
    Decision,
    /// A durable preference.
    Preference,
    /// A durable past failure.
    PastFailure,
}

impl MemoryKindDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Decision => "decision",
            Self::Preference => "preference",
            Self::PastFailure => "past_failure",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `memory_reference_unavailable` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "fact" => Ok(Self::Fact),
            "decision" => Ok(Self::Decision),
            "preference" => Ok(Self::Preference),
            "past_failure" => Ok(Self::PastFailure),
            _ => Err(intention_types::ErrorDto::validation(
                "memory_reference_unavailable",
                "the durable memory kind is unknown",
            )),
        }
    }
}

/// One bounded durable memory card revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalMemoryCardRecordDto {
    /// The daemon-assigned record identity.
    pub record_id: String,
    /// The exact immutable revision.
    pub revision: u64,
    /// The closed memory kind.
    pub kind: MemoryKindDto,
    /// The exactly-one record scope.
    pub scope: GoalRecordScopeDto,
    /// The bounded safe title.
    pub title: String,
    /// The bounded safe purpose.
    pub safe_purpose: String,
    /// The typed retained-content reference.
    pub retained_content_reference: String,
    /// The canonical card digest.
    pub canonical_digest: String,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl GoalMemoryCardRecordDto {
    /// Validates this memory card revision.
    ///
    /// # Errors
    ///
    /// Returns `memory_reference_unavailable` for a missing identity,
    /// revision, retained-content reference, title, or purpose,
    /// `memory_entry_too_large` when the card exceeds its bound, and
    /// `credentials_forbidden` for credential-shaped text.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("memory_reference_unavailable", &self.record_id)?;
        validate_safe_label(
            "memory_reference_unavailable",
            &self.retained_content_reference,
        )?;
        validate_safe_digest("memory_reference_unavailable", &self.canonical_digest)?;
        if self.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "memory_reference_unavailable",
                "a memory card requires an exact nonzero revision",
            ));
        }
        self.scope.validate("memory_reference_unavailable")?;
        validate_goal_text(
            "memory_reference_unavailable",
            "memory_entry_too_large",
            &self.title,
            MAX_GOAL_SAFE_TEXT_BYTES,
        )?;
        validate_goal_text(
            "memory_reference_unavailable",
            "memory_entry_too_large",
            &self.safe_purpose,
            MAX_GOAL_SAFE_TEXT_BYTES,
        )
    }
}

/// Input replacing one memory card revision with an explicit typed relation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalMemoryCardReplacementInputDto {
    /// The replaced record identity.
    pub replaced_record_id: String,
    /// The exact replaced revision.
    pub replaced_revision: u64,
    /// The replacement card revision to commit.
    pub replacement: GoalMemoryCardRecordDto,
    /// The durable replacement time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

impl GoalMemoryCardReplacementInputDto {
    /// Validates this explicit replacement relation and its card.
    ///
    /// # Errors
    ///
    /// Returns `memory_replacement_conflict` for a missing revision, a
    /// same-record replacement that does not advance, or an equal-identity
    /// replacement, and the card validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("memory_replacement_conflict", &self.replaced_record_id)?;
        if self.replaced_revision == 0 || self.replacement.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "memory_replacement_conflict",
                "a memory replacement requires exact nonzero revisions",
            ));
        }
        self.replacement.validate()?;
        if self.replaced_record_id == self.replacement.record_id
            && self.replacement.revision <= self.replaced_revision
        {
            return Err(intention_types::ErrorDto::validation(
                "memory_replacement_conflict",
                "a same-record replacement must advance to a newer immutable revision",
            ));
        }
        Ok(())
    }
}

/// Input restoring one earlier memory card revision with a new revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalMemoryCardRollbackInputDto {
    /// The restored record identity.
    pub restored_record_id: String,
    /// The exact restored revision.
    pub restored_revision: u64,
    /// The new immutable card revision linked to the restored revision.
    pub replacement: GoalMemoryCardRecordDto,
    /// The durable rollback time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

impl GoalMemoryCardRollbackInputDto {
    /// Validates this explicit rollback relation and its card.
    ///
    /// # Errors
    ///
    /// Returns `memory_replacement_conflict` for a missing revision or a
    /// rollback that does not create a later revision of the restored record,
    /// and the card validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("memory_replacement_conflict", &self.restored_record_id)?;
        if self.restored_revision == 0 || self.replacement.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "memory_replacement_conflict",
                "a memory rollback requires exact nonzero revisions",
            ));
        }
        self.replacement.validate()?;
        if self.replacement.record_id != self.restored_record_id
            || self.replacement.revision <= self.restored_revision
        {
            return Err(intention_types::ErrorDto::validation(
                "memory_replacement_conflict",
                "a rollback creates a later revision of the restored record",
            ));
        }
        Ok(())
    }
}

/// One bounded durable Skill card revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalSkillCardRecordDto {
    /// The daemon-assigned Skill identity.
    pub skill_id: String,
    /// The exact immutable Skill revision.
    pub revision: u64,
    /// The canonical lowercase letters/numbers/hyphens name.
    pub canonical_name: String,
    /// The bounded safe routing description.
    pub description: String,
    /// The exactly-one durable owner scope.
    pub owner_scope: GoalRecordScopeDto,
    /// The typed retained-content reference of the Skill body.
    pub content_reference: String,
    /// The canonical card digest.
    pub canonical_digest: String,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl GoalSkillCardRecordDto {
    /// Validates this Skill card revision.
    ///
    /// # Errors
    ///
    /// Returns `skill_reference_unavailable` for a missing identity,
    /// revision, content reference, name, description, or non-canonical name,
    /// `skill_entry_too_large` when the card exceeds its bound, and
    /// `credentials_forbidden` for credential-shaped text.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("skill_reference_unavailable", &self.skill_id)?;
        validate_safe_label("skill_reference_unavailable", &self.content_reference)?;
        validate_safe_digest("skill_reference_unavailable", &self.canonical_digest)?;
        if self.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "skill_reference_unavailable",
                "a Skill card requires an exact nonzero revision",
            ));
        }
        validate_goal_text(
            "skill_reference_unavailable",
            "skill_entry_too_large",
            &self.canonical_name,
            MAX_GOAL_SAFE_TEXT_BYTES,
        )?;
        let mut characters = self.canonical_name.chars();
        let valid_first = matches!(
            characters.next(),
            Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit()
        );
        let valid_rest = characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        });
        if !valid_first || !valid_rest || self.canonical_name.ends_with('-') {
            return Err(intention_types::ErrorDto::validation(
                "skill_reference_unavailable",
                "a Skill name is a canonical lowercase letters/numbers/hyphens identifier",
            ));
        }
        validate_goal_text(
            "skill_reference_unavailable",
            "skill_entry_too_large",
            &self.description,
            MAX_GOAL_SAFE_TEXT_BYTES,
        )?;
        self.owner_scope.validate("skill_reference_unavailable")
    }
}

/// The closed `sub_agent` execution classes a role may name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalRoleClassDto {
    /// The light inherited class.
    Light,
    /// The medium inherited class.
    Medium,
    /// The heavy inherited class.
    Heavy,
}

impl GoalRoleClassDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Medium => "medium",
            Self::Heavy => "heavy",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `delegation_role_invalid` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "light" => Ok(Self::Light),
            "medium" => Ok(Self::Medium),
            "heavy" => Ok(Self::Heavy),
            _ => Err(intention_types::ErrorDto::validation(
                "delegation_role_invalid",
                "the durable role class is unknown",
            )),
        }
    }
    /// Returns the closed class order, light first.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Light => 0,
            Self::Medium => 1,
            Self::Heavy => 2,
        }
    }
}

/// One named reusable child-role card revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalRoleCardRecordDto {
    /// The daemon-assigned role identity.
    pub role_id: String,
    /// The exact immutable role revision.
    pub revision: u64,
    /// The canonical role name.
    pub canonical_name: String,
    /// The bounded narrowed task text.
    pub task: String,
    /// The permitted child execution class.
    pub permitted_class: GoalRoleClassDto,
    /// The narrowed registered tool subset.
    pub tool_subset: Vec<String>,
    /// The narrowed context byte limit.
    pub context_limit_bytes: u64,
    /// The narrowed result byte limit.
    pub result_limit_bytes: u64,
    /// The canonical card digest.
    pub canonical_digest: String,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl GoalRoleCardRecordDto {
    /// Validates this role card revision.
    ///
    /// # Errors
    ///
    /// Returns `delegation_role_invalid` for a missing identity, revision,
    /// name, or task, a zero limit, a blank or duplicated tool identifier, or
    /// an over-limit tool subset, and `credentials_forbidden` for
    /// credential-shaped text.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("delegation_role_invalid", &self.role_id)?;
        validate_safe_digest("delegation_role_invalid", &self.canonical_digest)?;
        if self.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "delegation_role_invalid",
                "a role card requires an exact nonzero revision",
            ));
        }
        validate_goal_text(
            "delegation_role_invalid",
            "delegation_role_invalid",
            &self.canonical_name,
            MAX_GOAL_SAFE_TEXT_BYTES,
        )?;
        validate_goal_text(
            "delegation_role_invalid",
            "delegation_role_invalid",
            &self.task,
            MAX_GOAL_SAFE_TEXT_BYTES,
        )?;
        if self.context_limit_bytes == 0 || self.result_limit_bytes == 0 {
            return Err(intention_types::ErrorDto::validation(
                "delegation_role_invalid",
                "a role card requires positive context and result limits",
            ));
        }
        validate_safe_labels(
            "delegation_role_invalid",
            &self.tool_subset,
            MAX_GOAL_ROLE_TOOLS,
        )?;
        let mut seen: Vec<&str> = Vec::new();
        for tool in &self.tool_subset {
            if seen.contains(&tool.as_str()) {
                return Err(intention_types::ErrorDto::validation(
                    "delegation_role_invalid",
                    "a role tool subset holds unique registered tool identifiers",
                ));
            }
            seen.push(tool);
        }
        Ok(())
    }
}

/// The closed Goal milestones that may trigger one model proposal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalMilestoneDto {
    /// Technical readiness was established.
    TechnicalReadiness,
    /// The user acceptance decision is proposed.
    UserAcceptance,
    /// The user acceptance-with-exception decision is proposed.
    AcceptanceWithException,
    /// The stop decision is proposed.
    Stop,
    /// A required gate failed.
    RequiredGateFailure,
    /// An obligatory child reached a terminal outcome.
    ObligatoryChildTerminalOutcome,
}

impl GoalMilestoneDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::TechnicalReadiness => "technical_readiness",
            Self::UserAcceptance => "user_acceptance",
            Self::AcceptanceWithException => "acceptance_with_exception",
            Self::Stop => "stop",
            Self::RequiredGateFailure => "required_gate_failure",
            Self::ObligatoryChildTerminalOutcome => "obligatory_child_terminal_outcome",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `refinement_draft_conflict` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "technical_readiness" => Ok(Self::TechnicalReadiness),
            "user_acceptance" => Ok(Self::UserAcceptance),
            "acceptance_with_exception" => Ok(Self::AcceptanceWithException),
            "stop" => Ok(Self::Stop),
            "required_gate_failure" => Ok(Self::RequiredGateFailure),
            "obligatory_child_terminal_outcome" => Ok(Self::ObligatoryChildTerminalOutcome),
            _ => Err(intention_types::ErrorDto::validation(
                "refinement_draft_conflict",
                "the durable Goal milestone is unknown",
            )),
        }
    }
}

/// The closed kinds of one bounded refinement edit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefinementEditKindDto {
    /// A readiness claim edit.
    ReadinessClaim,
    /// A user acceptance decision edit.
    AcceptanceDecision,
    /// An exception set edit.
    ExceptionSet,
    /// A stop decision edit.
    StopDecision,
    /// A required-gate evidence edit.
    RequiredGateEvidence,
    /// An obligatory-child outcome reference edit.
    ChildOutcomeReference,
}

impl RefinementEditKindDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ReadinessClaim => "readiness_claim",
            Self::AcceptanceDecision => "acceptance_decision",
            Self::ExceptionSet => "exception_set",
            Self::StopDecision => "stop_decision",
            Self::RequiredGateEvidence => "required_gate_evidence",
            Self::ChildOutcomeReference => "child_outcome_reference",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `refinement_draft_conflict` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "readiness_claim" => Ok(Self::ReadinessClaim),
            "acceptance_decision" => Ok(Self::AcceptanceDecision),
            "exception_set" => Ok(Self::ExceptionSet),
            "stop_decision" => Ok(Self::StopDecision),
            "required_gate_evidence" => Ok(Self::RequiredGateEvidence),
            "child_outcome_reference" => Ok(Self::ChildOutcomeReference),
            _ => Err(intention_types::ErrorDto::validation(
                "refinement_draft_conflict",
                "the durable refinement edit kind is unknown",
            )),
        }
    }
}

/// One bounded typed refinement edit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefinementEditRecordDto {
    /// The closed edit kind.
    pub kind: RefinementEditKindDto,
    /// The exact evidence reference of the edit.
    pub evidence: GoalEvidenceReferenceDto,
}

/// The closed durable states of one refinement draft.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefinementDraftStateDto {
    /// The draft awaits the explicit user decision.
    Pending,
    /// The user accepted the draft; a new Goal revision was created.
    Accepted,
    /// The user rejected the draft; no active record changed.
    Rejected,
}

impl RefinementDraftStateDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `refinement_draft_conflict` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "pending" => Ok(Self::Pending),
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            _ => Err(intention_types::ErrorDto::validation(
                "refinement_draft_conflict",
                "the durable refinement draft state is unknown",
            )),
        }
    }
}

/// One coalesced model refinement proposal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefinementDraftRecordDto {
    /// The daemon-assigned draft identity.
    pub draft_id: String,
    /// The selected source run identity.
    pub source_run_id: String,
    /// The selected leading Goal identity.
    pub leading_goal_id: String,
    /// The durable Goal milestone.
    pub milestone: GoalMilestoneDto,
    /// The exact base Goal revision.
    pub base_goal_revision: u64,
    /// The exact base record reference.
    pub base_record_reference: String,
    /// The exact base record revision.
    pub base_record_revision: u64,
    /// The bounded typed edit set.
    pub edits: Vec<RefinementEditRecordDto>,
    /// The evidence references of the proposal.
    pub evidence_references: Vec<GoalEvidenceReferenceDto>,
    /// The bounded safe rationale.
    pub safe_rationale: String,
    /// The canonical draft digest.
    pub canonical_digest: String,
    /// The durable draft state.
    pub state: RefinementDraftStateDto,
    /// The coalesced evidence count.
    pub coalesced_evidence_count: u64,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
    /// The durable decision time in Unix milliseconds, when one exists.
    pub decided_at_ms: Option<u64>,
}

impl RefinementDraftRecordDto {
    /// Validates this refinement draft.
    ///
    /// # Errors
    ///
    /// Returns `refinement_draft_conflict` for a missing identity, base
    /// revision, edit, exact evidence, or non-blank rationale,
    /// `refinement_draft_too_large` when the draft safe content exceeds its
    /// bound, `goal_limit_exceeded` when the edit or evidence bound is
    /// exceeded, and `credentials_forbidden` for credential-shaped text.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("refinement_draft_conflict", &self.draft_id)?;
        validate_safe_label("refinement_draft_conflict", &self.source_run_id)?;
        validate_safe_label("refinement_draft_conflict", &self.leading_goal_id)?;
        validate_safe_label("refinement_draft_conflict", &self.base_record_reference)?;
        validate_safe_digest("refinement_draft_conflict", &self.canonical_digest)?;
        if self.base_goal_revision == 0 || self.base_record_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "refinement_draft_conflict",
                "a refinement draft requires exact nonzero base revisions",
            ));
        }
        if self.edits.is_empty() {
            return Err(intention_types::ErrorDto::validation(
                "refinement_draft_conflict",
                "a refinement draft carries at least one typed edit",
            ));
        }
        if self.edits.len() > MAX_GOAL_DRAFT_EDITS
            || self.evidence_references.len() > MAX_GOAL_EVIDENCE_REFERENCES
        {
            return Err(intention_types::ErrorDto::validation(
                "goal_limit_exceeded",
                "the refinement draft edit or evidence bound is exceeded",
            ));
        }
        for edit in &self.edits {
            edit.evidence.validate("refinement_draft_conflict")?;
        }
        validate_evidence_set(
            "refinement_draft_conflict",
            "goal_limit_exceeded",
            &self.evidence_references,
            MAX_GOAL_EVIDENCE_REFERENCES,
        )?;
        validate_goal_text(
            "refinement_draft_conflict",
            "refinement_draft_too_large",
            &self.safe_rationale,
            MAX_GOAL_SAFE_TEXT_BYTES,
        )?;
        match (self.state, self.decided_at_ms) {
            (RefinementDraftStateDto::Pending, None)
            | (RefinementDraftStateDto::Accepted | RefinementDraftStateDto::Rejected, Some(_)) => {
                Ok(())
            }
            _ => Err(intention_types::ErrorDto::validation(
                "refinement_draft_conflict",
                "a decided draft carries a decision time and a pending draft does not",
            )),
        }
    }
}

/// One immutable compacted conversation summary revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationSummaryRecordDto {
    /// The daemon-assigned summary identity.
    pub summary_id: String,
    /// The immutable summary revision.
    pub revision: u64,
    /// The exactly-one owner scope of the summary.
    pub scope: GoalRecordScopeDto,
    /// The previous summary identity when one continues a chain.
    pub previous_summary_reference: Option<String>,
    /// The first source history reference of the covered range.
    pub source_range_start: String,
    /// The last source history reference of the covered range.
    pub source_range_end: String,
    /// The bounded safe summary content.
    pub safe_content: String,
    /// The canonical summary digest.
    pub canonical_digest: String,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl ConversationSummaryRecordDto {
    /// Validates this summary record.
    ///
    /// # Errors
    ///
    /// Returns `compaction_summary_unavailable` for a missing identity,
    /// revision, or safe content, `compaction_history_unavailable` for a
    /// missing source range reference, `compaction_summary_too_large` when the
    /// content exceeds its bound, and `credentials_forbidden` for
    /// credential-shaped content.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("compaction_summary_unavailable", &self.summary_id)?;
        validate_safe_digest("compaction_summary_unavailable", &self.canonical_digest)?;
        if self.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "compaction_summary_unavailable",
                "a conversation summary requires an exact nonzero revision",
            ));
        }
        self.scope.validate("compaction_summary_unavailable")?;
        if let Some(previous) = &self.previous_summary_reference {
            validate_safe_label("compaction_summary_unavailable", previous)?;
        }
        validate_safe_label("compaction_history_unavailable", &self.source_range_start)?;
        validate_safe_label("compaction_history_unavailable", &self.source_range_end)?;
        validate_goal_text(
            "compaction_summary_unavailable",
            "compaction_summary_too_large",
            &self.safe_content,
            MAX_GOAL_SAFE_TEXT_BYTES,
        )
    }
}

/// Input recording one completed history reference of a compaction suffix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordCompactionSuffixReferenceInputDto {
    /// The exactly-one owner scope of the working form.
    pub scope: GoalRecordScopeDto,
    /// The completed history reference to append.
    pub history_reference: String,
    /// The durable append time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One durable working compaction form.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalCompactionWorkingFormRecordDto {
    /// The cumulative current summary when one exists.
    pub current_summary: Option<ConversationSummaryRecordDto>,
    /// The exact completed history references after the current summary.
    pub uncompacted_suffix: Vec<String>,
}

/// An immutable reference to one exact authority revision and digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierAuthorityReferenceDto {
    /// The authority identity.
    pub authority_id: String,
    /// The exact immutable authority revision.
    pub authority_revision: u64,
    /// The canonical authority digest.
    pub canonical_authority_digest: String,
}

impl VerifierAuthorityReferenceDto {
    /// Validates this exact authority reference.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` for a missing identity or digest
    /// and `verifier_authority_revision_mismatch` for a zero revision.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("verifier_authority_invalid", &self.authority_id)?;
        validate_safe_digest(
            "verifier_authority_invalid",
            &self.canonical_authority_digest,
        )?;
        if self.authority_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "verifier_authority_revision_mismatch",
                "an authority reference names an exact nonzero revision",
            ));
        }
        Ok(())
    }
}

/// An immutable reference to one exact target-set revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierTargetSetReferenceDto {
    /// The target-set identity.
    pub target_set_id: String,
    /// The canonical target-set digest.
    pub canonical_target_set_digest: String,
}

impl VerifierTargetSetReferenceDto {
    /// Validates this exact target-set reference.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` for a missing identity or digest.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("verifier_authority_invalid", &self.target_set_id)?;
        validate_safe_digest(
            "verifier_authority_invalid",
            &self.canonical_target_set_digest,
        )
    }
}

/// An immutable reference to one audit or gate/evidence contract revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierContractReferenceDto {
    /// The contract identity.
    pub contract_id: String,
    /// The exact immutable contract revision.
    pub contract_revision: u64,
    /// The canonical contract digest.
    pub canonical_contract_digest: String,
}

impl VerifierContractReferenceDto {
    /// Validates this exact contract reference.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` for a missing identity or digest
    /// and `verifier_authority_contract_mismatch` for a zero revision.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("verifier_authority_invalid", &self.contract_id)?;
        validate_safe_digest(
            "verifier_authority_invalid",
            &self.canonical_contract_digest,
        )?;
        if self.contract_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "verifier_authority_contract_mismatch",
                "a contract reference names an exact nonzero revision",
            ));
        }
        Ok(())
    }
}

/// An immutable reference to one exact target revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierTargetReferenceDto {
    /// The target Mandate identity.
    pub target_mandate_id: String,
    /// The exact frozen target revision.
    pub target_revision: u64,
}

impl VerifierTargetReferenceDto {
    /// Validates this exact target reference.
    ///
    /// # Errors
    ///
    /// Returns `verifier_target_lifecycle_invalid` for a missing identity or
    /// `verifier_baseline_stale_target_revision` for a zero revision.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("verifier_target_lifecycle_invalid", &self.target_mandate_id)?;
        if self.target_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "verifier_baseline_stale_target_revision",
                "a target reference names an exact nonzero revision",
            ));
        }
        Ok(())
    }
}

/// One frozen Goal reference of a verifier record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierGoalReferenceDto {
    /// The frozen Goal identity.
    pub goal_id: String,
    /// The exact frozen Goal revision.
    pub goal_revision: u64,
    /// The canonical Goal revision digest.
    pub canonical_goal_revision_digest: String,
}

impl VerifierGoalReferenceDto {
    /// Validates this frozen Goal reference.
    ///
    /// # Errors
    ///
    /// Returns `verifier_baseline_stale_target_identity` for a missing
    /// identity or digest and `verifier_baseline_stale_target_revision` for a
    /// zero revision.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("verifier_baseline_stale_target_identity", &self.goal_id)?;
        validate_safe_digest(
            "verifier_baseline_stale_target_identity",
            &self.canonical_goal_revision_digest,
        )?;
        if self.goal_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "verifier_baseline_stale_target_revision",
                "a frozen Goal reference names an exact nonzero revision",
            ));
        }
        Ok(())
    }
}

/// The frozen Goal and contract references of one verifier record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierFrozenReferencesDto {
    /// The frozen Goal references.
    pub goal_references: Vec<VerifierGoalReferenceDto>,
    /// The frozen gate and evidence contract references.
    pub contract_references: Vec<VerifierContractReferenceDto>,
}

impl VerifierFrozenReferencesDto {
    /// Validates these frozen references.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` when a frozen list exceeds its
    /// bound and the nested reference validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        if self.goal_references.len() > MAX_VERIFIER_FROZEN_GOAL_REFERENCES
            || self.contract_references.len() > MAX_VERIFIER_FROZEN_CONTRACT_REFERENCES
        {
            return Err(intention_types::ErrorDto::validation(
                "verifier_authority_invalid",
                "the frozen reference bound is exceeded",
            ));
        }
        for reference in &self.goal_references {
            reference.validate()?;
        }
        for reference in &self.contract_references {
            reference.validate()?;
        }
        Ok(())
    }
}

/// The closed separately issued delegated verifier operation set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifierOperationDto {
    /// Move the target to `NeedsRework` after qualifying fail evidence.
    MarkNeedsRework,
    /// Complete the target after unconditional pass evidence.
    MarkComplete,
    /// Stop the target without asserting completion.
    Stop,
    /// Create an immutable future target revision without rewriting history.
    ReviseFull,
    /// Resolve the exact target uncertainty into later fresh work or a stop.
    ResolveUnknownEffect,
}

impl VerifierOperationDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::MarkNeedsRework => "mark_needs_rework",
            Self::MarkComplete => "mark_complete",
            Self::Stop => "stop",
            Self::ReviseFull => "revise_full",
            Self::ResolveUnknownEffect => "resolve_unknown_effect",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_operation_not_allowed` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "mark_needs_rework" => Ok(Self::MarkNeedsRework),
            "mark_complete" => Ok(Self::MarkComplete),
            "stop" => Ok(Self::Stop),
            "revise_full" => Ok(Self::ReviseFull),
            "resolve_unknown_effect" => Ok(Self::ResolveUnknownEffect),
            _ => Err(intention_types::ErrorDto::validation(
                "verifier_authority_operation_not_allowed",
                "the durable verifier operation is unknown",
            )),
        }
    }
}

/// The closed Mandate target lifecycle of one verifier baseline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifierTargetLifecycleDto {
    /// No admissible run exists yet.
    Draft,
    /// The target can admit one fresh run.
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

impl VerifierTargetLifecycleDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Working => "working",
            Self::Paused => "paused",
            Self::PausedAwaitingDecision => "paused_awaiting_decision",
            Self::NeedsRework => "needs_rework",
            Self::Completed => "completed",
            Self::Stopped => "stopped",
            Self::Archived => "archived",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `verifier_target_lifecycle_invalid` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "draft" => Ok(Self::Draft),
            "active" => Ok(Self::Active),
            "working" => Ok(Self::Working),
            "paused" => Ok(Self::Paused),
            "paused_awaiting_decision" => Ok(Self::PausedAwaitingDecision),
            "needs_rework" => Ok(Self::NeedsRework),
            "completed" => Ok(Self::Completed),
            "stopped" => Ok(Self::Stopped),
            "archived" => Ok(Self::Archived),
            _ => Err(intention_types::ErrorDto::validation(
                "verifier_target_lifecycle_invalid",
                "the durable target lifecycle is unknown",
            )),
        }
    }
    /// Returns whether this lifecycle state admits no further work.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Stopped | Self::Archived)
    }
}

/// The closed audit evidence kinds of one verifier audit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifierEvidenceKindDto {
    /// Unconditional pass evidence.
    UnconditionalPass,
    /// Qualifying fail evidence.
    QualifyingFail,
    /// Inconclusive evidence that supports no operation.
    Inconclusive,
    /// Evidence of required graph terminalization closure.
    GraphTerminalizationClosure,
    /// Evidence that the audit reconciliation standard holds.
    ReconciliationStandardProof,
}

impl VerifierEvidenceKindDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::UnconditionalPass => "unconditional_pass",
            Self::QualifyingFail => "qualifying_fail",
            Self::Inconclusive => "inconclusive",
            Self::GraphTerminalizationClosure => "graph_terminalization_closure",
            Self::ReconciliationStandardProof => "reconciliation_standard_proof",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `verifier_qualifying_fail_evidence_missing` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "unconditional_pass" => Ok(Self::UnconditionalPass),
            "qualifying_fail" => Ok(Self::QualifyingFail),
            "inconclusive" => Ok(Self::Inconclusive),
            "graph_terminalization_closure" => Ok(Self::GraphTerminalizationClosure),
            "reconciliation_standard_proof" => Ok(Self::ReconciliationStandardProof),
            _ => Err(intention_types::ErrorDto::validation(
                "verifier_qualifying_fail_evidence_missing",
                "the durable verifier evidence kind is unknown",
            )),
        }
    }
}

/// The closed verifier audit verdict kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
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
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `verifier_selection_unsupported` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "pass" => Ok(Self::Pass),
            "fail" => Ok(Self::Fail),
            "inconclusive" => Ok(Self::Inconclusive),
            "target_revision_stale" => Ok(Self::TargetRevisionStale),
            "target_unavailable" => Ok(Self::TargetUnavailable),
            "verifier_unavailable" => Ok(Self::VerifierUnavailable),
            "verifier_external_effect_unknown" => Ok(Self::VerifierExternalEffectUnknown),
            _ => Err(intention_types::ErrorDto::validation(
                "verifier_selection_unsupported",
                "the durable verification verdict is unknown",
            )),
        }
    }
}

/// The closed authority consumption rule selected at issuance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifierAuthorityConsumptionRuleDto {
    /// The authority is consumed by one applied mutation.
    SingleUse,
    /// The authority stays usable while active, unrevoked, and unexpired.
    ReusableWhileActive,
}

impl VerifierAuthorityConsumptionRuleDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SingleUse => "single_use",
            Self::ReusableWhileActive => "reusable_while_active",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "single_use" => Ok(Self::SingleUse),
            "reusable_while_active" => Ok(Self::ReusableWhileActive),
            _ => Err(intention_types::ErrorDto::validation(
                "verifier_authority_invalid",
                "the durable authority consumption rule is unknown",
            )),
        }
    }
}

/// The closed authority consumption state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifierAuthorityConsumptionStateDto {
    /// The authority is not yet consumed.
    Unconsumed,
    /// The authority was consumed by one applied target mutation.
    Consumed,
}

impl VerifierAuthorityConsumptionStateDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unconsumed => "unconsumed",
            Self::Consumed => "consumed",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "unconsumed" => Ok(Self::Unconsumed),
            "consumed" => Ok(Self::Consumed),
            _ => Err(intention_types::ErrorDto::validation(
                "verifier_authority_invalid",
                "the durable authority consumption state is unknown",
            )),
        }
    }
}

/// One separately issued, revisioned, target-scoped delegated verifier
/// authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierAuthorityRecordDto {
    /// The stable authority identity.
    pub authority_id: String,
    /// The immutable authority revision.
    pub authority_revision: u64,
    /// The exact verifier Mandate that owns this authority.
    pub verifier_mandate_id: String,
    /// The immutable explicitly enumerated target-set reference.
    pub immutable_target_set_reference: VerifierTargetSetReferenceDto,
    /// The closed allowed-operation set.
    pub allowed_operations: Vec<VerifierOperationDto>,
    /// The immutable audit contract reference.
    pub audit_contract_reference: VerifierContractReferenceDto,
    /// The issuance time in Unix milliseconds.
    pub issued_at_ms: u64,
    /// The optional exclusive expiry time in Unix milliseconds.
    pub expires_at_ms: Option<u64>,
    /// The optional revocation time in Unix milliseconds.
    pub revoked_at_ms: Option<u64>,
    /// The optional immutable revocation reference.
    pub revocation_reference: Option<String>,
    /// The selected consumption rule.
    pub consumption_rule: VerifierAuthorityConsumptionRuleDto,
    /// The closed consumption state.
    pub consumption_state: VerifierAuthorityConsumptionStateDto,
    /// The applied mutation that consumed this authority, when consumed.
    pub consumed_by_mutation_reference: Option<String>,
    /// The canonical authority digest.
    pub canonical_authority_digest: String,
}

impl VerifierAuthorityRecordDto {
    /// Validates this delegated verifier authority revision.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` for a missing identity, an empty
    /// or over-limit operation set, or an invalid lifecycle,
    /// `verifier_authority_revision_mismatch` for a zero revision, and the
    /// nested reference and digest validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("verifier_authority_invalid", &self.authority_id)?;
        validate_safe_label("verifier_authority_invalid", &self.verifier_mandate_id)?;
        validate_safe_digest(
            "verifier_authority_invalid",
            &self.canonical_authority_digest,
        )?;
        if self.authority_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "verifier_authority_revision_mismatch",
                "an authority revision is exact and nonzero",
            ));
        }
        self.immutable_target_set_reference.validate()?;
        self.audit_contract_reference.validate()?;
        if self.allowed_operations.is_empty() || self.allowed_operations.len() > 5 {
            return Err(intention_types::ErrorDto::validation(
                "verifier_authority_invalid",
                "the closed allowed-operation set holds one to five operations",
            ));
        }
        let mut seen = Vec::new();
        for operation in &self.allowed_operations {
            if seen.contains(operation) {
                return Err(intention_types::ErrorDto::validation(
                    "verifier_authority_invalid",
                    "the allowed-operation set names each operation once",
                ));
            }
            seen.push(*operation);
        }
        if self.revoked_at_ms.is_some() != self.revocation_reference.is_some() {
            return Err(intention_types::ErrorDto::validation(
                "verifier_authority_revoked",
                "a revocation time and reference are recorded together",
            ));
        }
        if let Some(revoked_at_ms) = self.revoked_at_ms
            && revoked_at_ms < self.issued_at_ms
        {
            return Err(intention_types::ErrorDto::validation(
                "verifier_authority_invalid",
                "a revocation never precedes issuance",
            ));
        }
        if let Some(expires_at_ms) = self.expires_at_ms
            && expires_at_ms <= self.issued_at_ms
        {
            return Err(intention_types::ErrorDto::validation(
                "verifier_authority_invalid",
                "an expiry window is positive and bounded",
            ));
        }
        match self.consumption_state {
            VerifierAuthorityConsumptionStateDto::Unconsumed => {
                if self.consumed_by_mutation_reference.is_some() {
                    return Err(intention_types::ErrorDto::validation(
                        "verifier_authority_invalid",
                        "an unconsumed authority names no consuming mutation",
                    ));
                }
                Ok(())
            }
            VerifierAuthorityConsumptionStateDto::Consumed => {
                let Some(reference) = self.consumed_by_mutation_reference.as_deref() else {
                    return Err(intention_types::ErrorDto::validation(
                        "verifier_authority_invalid",
                        "a consumed authority names its consuming mutation",
                    ));
                };
                validate_safe_label("verifier_authority_invalid", reference)?;
                if self.consumption_rule != VerifierAuthorityConsumptionRuleDto::SingleUse {
                    return Err(intention_types::ErrorDto::validation(
                        "verifier_authority_consumed",
                        "only a single-use authority is consumed by one mutation",
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Input revoking one authority revision without rewriting its history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevokeVerifierAuthorityInputDto {
    /// The authority identity.
    pub authority_id: String,
    /// The exact authority revision.
    pub authority_revision: u64,
    /// The immutable revocation reference.
    pub revocation_reference: String,
    /// The durable revocation time in Unix milliseconds.
    pub revoked_at_ms: u64,
}

impl RevokeVerifierAuthorityInputDto {
    /// Validates this revocation input.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` for a missing identity or
    /// reference and `verifier_authority_revision_mismatch` for a zero
    /// revision.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("verifier_authority_invalid", &self.authority_id)?;
        validate_safe_label("verifier_authority_invalid", &self.revocation_reference)?;
        if self.authority_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "verifier_authority_revision_mismatch",
                "a revocation names an exact nonzero authority revision",
            ));
        }
        Ok(())
    }
}

/// One immutable verifier audit baseline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierAuditBaselineRecordDto {
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceDto,
    /// The exact verifier Mandate revision at baseline creation.
    pub verifier_mandate_revision: u64,
    /// The target Mandate identity.
    pub target_mandate_id: String,
    /// The frozen target revision.
    pub target_revision: u64,
    /// The frozen aggregate target sequence.
    pub target_sequence: u64,
    /// The frozen target lifecycle.
    pub target_lifecycle: VerifierTargetLifecycleDto,
    /// The frozen Goal and gate/evidence contract references.
    pub frozen_references: VerifierFrozenReferencesDto,
    /// The exact unknown-effect reference when the target is uncertain.
    pub optional_unknown_effect_reference: Option<String>,
    /// The exact audit contract revision and digest.
    pub audit_contract_reference: VerifierContractReferenceDto,
    /// The target graph epoch where the target participates in a child graph.
    pub graph_epoch: Option<u64>,
    /// The canonical baseline digest.
    pub canonical_baseline_digest: String,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl VerifierAuditBaselineRecordDto {
    /// Validates this immutable audit baseline.
    ///
    /// # Errors
    ///
    /// Returns `verifier_baseline_invalid` for a missing digest, a zero
    /// revision, or an invalid nested reference,
    /// `verifier_baseline_stale_target_revision` for a zero target revision,
    /// and the frozen-reference validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("verifier_baseline_invalid", &self.target_mandate_id)?;
        validate_safe_digest("verifier_baseline_invalid", &self.canonical_baseline_digest)?;
        if self.verifier_mandate_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "verifier_baseline_invalid",
                "a baseline names an exact nonzero verifier Mandate revision",
            ));
        }
        if self.target_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "verifier_baseline_stale_target_revision",
                "a baseline names an exact nonzero target revision",
            ));
        }
        if let Some(reference) = &self.optional_unknown_effect_reference {
            validate_safe_label("verifier_baseline_invalid", reference)?;
        }
        self.authority_reference.validate()?;
        self.audit_contract_reference.validate()?;
        self.frozen_references.validate()
    }
}

/// One immutable verifier audit evidence record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierAuditEvidenceRecordDto {
    /// The stable evidence identity.
    pub evidence_id: String,
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceDto,
    /// The exact target revision.
    pub target_reference: VerifierTargetReferenceDto,
    /// The frozen Goal and gate/evidence contract references.
    pub frozen_references: VerifierFrozenReferencesDto,
    /// The closed evidence kind.
    pub evidence_kind: VerifierEvidenceKindDto,
    /// The safe retained-content reference; never raw content.
    pub retained_content_reference: String,
    /// The canonical evidence digest.
    pub canonical_evidence_digest: String,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl VerifierAuditEvidenceRecordDto {
    /// Validates this immutable audit evidence record.
    ///
    /// # Errors
    ///
    /// Returns `verifier_evidence_invalid` for a missing identity or digest
    /// and the nested reference and frozen-reference validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("verifier_evidence_invalid", &self.evidence_id)?;
        validate_safe_label(
            "verifier_evidence_invalid",
            &self.retained_content_reference,
        )?;
        validate_safe_digest("verifier_evidence_invalid", &self.canonical_evidence_digest)?;
        self.authority_reference.validate()?;
        self.target_reference.validate()?;
        self.frozen_references.validate()
    }
}

/// One immutable verifier audit verdict record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierAuditVerdictRecordDto {
    /// The stable verdict identity.
    pub verdict_id: String,
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceDto,
    /// The exact target revision.
    pub target_reference: VerifierTargetReferenceDto,
    /// The exact frozen baseline digest.
    pub baseline_digest: String,
    /// The closed verdict.
    pub verdict: VerificationAuditVerdictDto,
    /// The audit evidence references, in declared order.
    pub evidence_references: Vec<String>,
    /// The canonical verdict digest.
    pub canonical_verdict_digest: String,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl VerifierAuditVerdictRecordDto {
    /// Validates this immutable verdict record.
    ///
    /// # Errors
    ///
    /// Returns `verifier_verdict_invalid` for a missing identity or digest,
    /// an over-limit or duplicated evidence reference, and the nested
    /// reference validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("verifier_verdict_invalid", &self.verdict_id)?;
        validate_safe_digest("verifier_verdict_invalid", &self.baseline_digest)?;
        validate_safe_digest("verifier_verdict_invalid", &self.canonical_verdict_digest)?;
        self.authority_reference.validate()?;
        self.target_reference.validate()?;
        if self.evidence_references.len() > MAX_VERIFIER_EVIDENCE_REFERENCES {
            return Err(intention_types::ErrorDto::validation(
                "verifier_verdict_invalid",
                "the verdict evidence bound is exceeded",
            ));
        }
        validate_safe_labels(
            "verifier_verdict_invalid",
            &self.evidence_references,
            MAX_VERIFIER_EVIDENCE_REFERENCES,
        )?;
        let mut seen: Vec<&str> = Vec::new();
        for reference in &self.evidence_references {
            if seen.contains(&reference.as_str()) {
                return Err(intention_types::ErrorDto::validation(
                    "verifier_verdict_invalid",
                    "a verdict names each evidence reference once",
                ));
            }
            seen.push(reference);
        }
        Ok(())
    }
}

/// The idempotent operation identity of one target mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierOperationIdentityDto {
    /// The stable idempotent operation identity.
    pub operation_id: String,
    /// The semantic operation digest.
    pub operation_digest: String,
}

impl VerifierOperationIdentityDto {
    /// Validates this idempotent operation identity.
    ///
    /// # Errors
    ///
    /// Returns `verifier_mutation_invalid` for a missing identity or digest.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("verifier_mutation_invalid", &self.operation_id)?;
        validate_safe_digest("verifier_mutation_invalid", &self.operation_digest)
    }
}

/// One immutable target-mutation request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifierTargetMutationRecordDto {
    /// The stable mutation identity.
    pub mutation_id: String,
    /// The exact authority revision and digest.
    pub authority_reference: VerifierAuthorityReferenceDto,
    /// The exact audit contract revision and digest.
    pub audit_contract_reference: VerifierContractReferenceDto,
    /// The exact target revision.
    pub target_reference: VerifierTargetReferenceDto,
    /// The closed verifier operation.
    pub operation: VerifierOperationDto,
    /// The audit evidence references, in declared order, without duplicates.
    pub audit_evidence_references: Vec<String>,
    /// The expected target revision of the frozen baseline.
    pub expected_target_revision: u64,
    /// The expected aggregate target sequence of the frozen baseline.
    pub expected_target_sequence: u64,
    /// The exact frozen baseline digest.
    pub expected_baseline_digest: String,
    /// The idempotent operation identity and semantic digest.
    pub idempotency: VerifierOperationIdentityDto,
    /// The canonical mutation digest.
    pub canonical_mutation_digest: String,
    /// The durable application time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl VerifierTargetMutationRecordDto {
    /// Validates this target-mutation request.
    ///
    /// # Errors
    ///
    /// Returns `verifier_mutation_invalid` for a missing identity or digest,
    /// a zero expected revision, an over-limit or duplicated evidence
    /// reference, and the nested reference validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("verifier_mutation_invalid", &self.mutation_id)?;
        validate_safe_digest("verifier_mutation_invalid", &self.expected_baseline_digest)?;
        validate_safe_digest("verifier_mutation_invalid", &self.canonical_mutation_digest)?;
        if self.expected_target_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "verifier_mutation_invalid",
                "a mutation names an exact nonzero expected target revision",
            ));
        }
        self.authority_reference.validate()?;
        self.audit_contract_reference.validate()?;
        self.target_reference.validate()?;
        self.idempotency.validate()?;
        if self.audit_evidence_references.len() > MAX_VERIFIER_EVIDENCE_REFERENCES {
            return Err(intention_types::ErrorDto::validation(
                "verifier_mutation_invalid",
                "the mutation evidence bound is exceeded",
            ));
        }
        validate_safe_labels(
            "verifier_mutation_invalid",
            &self.audit_evidence_references,
            MAX_VERIFIER_EVIDENCE_REFERENCES,
        )?;
        let mut seen: Vec<&str> = Vec::new();
        for reference in &self.audit_evidence_references {
            if seen.contains(&reference.as_str()) {
                return Err(intention_types::ErrorDto::validation(
                    "verifier_mutation_invalid",
                    "a mutation names each evidence reference once",
                ));
            }
            seen.push(reference);
        }
        Ok(())
    }
}

/// The outcome of one atomic target-mutation application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplyVerifierMutationOutcomeDto {
    /// The committed mutation record.
    pub mutation: VerifierTargetMutationRecordDto,
    /// The authority revision after the mutation.
    pub authority: VerifierAuthorityRecordDto,
    /// Whether an equal repeated operation replayed an existing binding.
    pub replayed: bool,
}

/// DTO-only repository contract for durable Goal identities and revisions.
pub trait GoalRepositoryDto {
    /// Creates one durable Goal identity with its first immutable revision.
    ///
    /// # Errors
    ///
    /// Returns `goal_limit_exceeded` when the project or session Goal bound is
    /// exceeded, `goal_revision_conflict` when the identity is already bound
    /// to different content, or an unavailable error when the atomic commit
    /// fails.
    fn create_goal(&self, input: CreateGoalInputDto) -> DtoResult<GoalRecordDto>;
    /// Loads one durable Goal identity.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_active` when the Goal does not exist, or an
    /// unavailable error when it cannot be read.
    fn load_goal(&self, goal_id: String) -> DtoResult<GoalRecordDto>;
    /// Loads one immutable Goal revision.
    ///
    /// # Errors
    ///
    /// Returns `goal_revision_conflict` when the exact revision is absent, or
    /// an unavailable error when it cannot be read.
    fn load_goal_revision(
        &self,
        goal_id: String,
        revision: u64,
    ) -> DtoResult<GoalRevisionRecordDto>;
    /// Appends one immutable revision and moves the active revision in one
    /// atomic transaction guarded by the exact expected revision.
    ///
    /// # Errors
    ///
    /// Returns `goal_revision_conflict` when the expected revision is stale or
    /// the identity is already bound to different revision content, or an
    /// unavailable error when the atomic commit fails.
    fn append_goal_revision(&self, input: AppendGoalRevisionInputDto) -> DtoResult<GoalRecordDto>;
    /// Applies one closed lifecycle transition to a durable Goal.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_active` when the transition is not declared or the
    /// Goal is unknown, `goal_archive_not_terminal` when an archive is
    /// attempted on a non-terminal Goal, `goal_revision_conflict` for a stale
    /// expected revision, or an unavailable error when the atomic commit
    /// fails.
    fn transition_goal_lifecycle(
        &self,
        input: TransitionGoalLifecycleInputDto,
    ) -> DtoResult<GoalRecordDto>;
    /// Records one readiness claim against an exact Goal revision.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_ready` for an incoherent claim,
    /// `goal_revision_conflict` for a stale expected revision, or an
    /// unavailable error when the atomic commit fails.
    fn set_goal_readiness(&self, input: SetGoalReadinessInputDto) -> DtoResult<GoalRecordDto>;
    /// Records one user decision against an exact Goal revision.
    ///
    /// # Errors
    ///
    /// Returns `goal_acceptance_exception_invalid` for an incoherent decision,
    /// `goal_not_ready` when an acceptance claims an unready Goal,
    /// `goal_revision_conflict` for a stale expected revision, or an
    /// unavailable error when the atomic commit fails.
    fn record_goal_user_decision(
        &self,
        input: RecordGoalUserDecisionInputDto,
    ) -> DtoResult<GoalRecordDto>;
    /// Attaches one obligatory child Goal link atomically.
    ///
    /// # Errors
    ///
    /// Returns `goal_tree_depth_limit_exceeded`, `goal_child_limit_exceeded`,
    /// or `goal_cycle_detected` when the tree rules reject the link,
    /// `goal_not_active` when either Goal is unknown, or an unavailable error
    /// when the atomic commit fails.
    fn attach_goal_child(
        &self,
        input: AttachGoalChildInputDto,
    ) -> DtoResult<GoalParentLinkRecordDto>;
    /// Loads the direct children of one Goal in link order.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the links cannot be read.
    fn list_goal_children(&self, parent_goal_id: String)
    -> DtoResult<Vec<GoalParentLinkRecordDto>>;
    /// Loads the ancestry depth of one Goal, counting the root as one.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_active` when the Goal does not exist and
    /// `goal_cycle_detected` when the stored graph holds a cycle, or an
    /// unavailable error when the graph cannot be read.
    fn load_goal_tree_depth(&self, goal_id: String) -> DtoResult<u64>;
    /// Creates one explicit durable project-Goal-to-session link atomically.
    ///
    /// # Errors
    ///
    /// Returns `goal_session_link_limit_exceeded` when the link bound is
    /// exceeded, `goal_revision_conflict` when the link already exists at a
    /// different revision, or an unavailable error when the atomic commit
    /// fails.
    fn create_goal_session_link(
        &self,
        input: CreateGoalSessionLinkInputDto,
    ) -> DtoResult<GoalSessionLinkRecordDto>;
    /// Loads one explicit durable session link.
    ///
    /// # Errors
    ///
    /// Returns `goal_not_active` when the link does not exist, or an
    /// unavailable error when it cannot be read.
    fn load_goal_session_link(&self, link_id: String) -> DtoResult<GoalSessionLinkRecordDto>;
    /// Loads the explicit session links of one project Goal in creation order.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the links cannot be read.
    fn load_goal_session_links(
        &self,
        project_goal_id: String,
    ) -> DtoResult<Vec<GoalSessionLinkRecordDto>>;
    /// Counts the durable Goals owned by one project.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the count cannot be read.
    fn count_goals_in_project(&self, project_id: String) -> DtoResult<u64>;
    /// Counts the durable session Goals owned by one session.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the count cannot be read.
    fn count_goals_in_session(&self, session_id: String) -> DtoResult<u64>;
}

/// DTO-only repository contract for durable gates, templates, and results.
pub trait GoalGateRepositoryDto {
    /// Creates one durable gate with its first immutable revision.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_limit_exceeded` when the gate bound of the Goal is
    /// exceeded, `goal_gate_unavailable` when the Goal is unknown or the
    /// definition is unusable, or an unavailable error when the atomic commit
    /// fails.
    fn create_goal_gate(&self, input: CreateGoalGateInputDto) -> DtoResult<GoalGateRecordDto>;
    /// Loads one durable gate record.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` when the gate does not exist, or an
    /// unavailable error when it cannot be read.
    fn load_goal_gate(&self, gate_id: String) -> DtoResult<GoalGateRecordDto>;
    /// Appends one immutable gate definition revision atomically.
    ///
    /// # Errors
    ///
    /// Returns `goal_revision_conflict` when the expected revision is stale,
    /// `goal_gate_unavailable` when the gate is unknown or the definition is
    /// unusable, or an unavailable error when the atomic commit fails.
    fn append_goal_gate_revision(
        &self,
        input: AppendGoalGateRevisionInputDto,
    ) -> DtoResult<GoalGateRecordDto>;
    /// Creates one user-created immutable gate template revision.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` for an unconfirmed model proposal or an
    /// unusable card, `goal_revision_conflict` when the exact revision is
    /// already bound to different content, or an unavailable error when the
    /// atomic commit fails.
    fn create_goal_gate_template(
        &self,
        input: GoalGateTemplateRecordDto,
    ) -> DtoResult<GoalGateTemplateRecordDto>;
    /// Loads one immutable gate template revision.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` when the exact revision is absent, or
    /// an unavailable error when it cannot be read.
    fn load_goal_gate_template(
        &self,
        template_id: String,
        revision: u64,
    ) -> DtoResult<GoalGateTemplateRecordDto>;
    /// Applies one closed lifecycle transition to a gate template.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` when the exact revision is absent or
    /// the transition is not declared, or an unavailable error when the atomic
    /// commit fails.
    fn transition_goal_gate_template_lifecycle(
        &self,
        input: TransitionGoalGateTemplateInputDto,
    ) -> DtoResult<GoalGateTemplateRecordDto>;
    /// Records one durable gate evaluation result.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_failed` for an incoherent outcome,
    /// `goal_revision_conflict` when the exact result already exists, or an
    /// unavailable error when the atomic commit fails.
    fn record_goal_gate_result(
        &self,
        input: GoalGateResultRecordDto,
    ) -> DtoResult<GoalGateResultRecordDto>;
    /// Loads one durable gate result.
    ///
    /// # Errors
    ///
    /// Returns `goal_gate_unavailable` when the exact result is absent, or an
    /// unavailable error when it cannot be read.
    fn load_goal_gate_result(
        &self,
        gate_id: String,
        gate_revision: u64,
    ) -> DtoResult<GoalGateResultRecordDto>;
}

/// DTO-only repository contract for durable memory, Skill, and role cards.
pub trait GoalCardRepositoryDto {
    /// Stores one bounded memory card revision.
    ///
    /// # Errors
    ///
    /// Returns `memory_entry_limit_exceeded` when the active card bound of the
    /// owner scope is exceeded, `memory_entry_too_large` when the card exceeds
    /// its bound, `memory_reference_unavailable` when the exact revision is
    /// already bound to different content, or an unavailable error when the
    /// atomic commit fails.
    fn store_goal_memory_card(
        &self,
        input: GoalMemoryCardRecordDto,
    ) -> DtoResult<GoalMemoryCardRecordDto>;
    /// Loads one immutable memory card revision.
    ///
    /// # Errors
    ///
    /// Returns `memory_reference_unavailable` when the exact revision is
    /// absent, or an unavailable error when it cannot be read.
    fn load_goal_memory_card(
        &self,
        record_id: String,
        revision: u64,
    ) -> DtoResult<GoalMemoryCardRecordDto>;
    /// Commits one explicit typed replacement relation and its card revision
    /// atomically.
    ///
    /// # Errors
    ///
    /// Returns `memory_replacement_conflict` for an invalid relation or a
    /// missing replaced revision, and the store validation failures.
    fn replace_goal_memory_card(
        &self,
        input: GoalMemoryCardReplacementInputDto,
    ) -> DtoResult<GoalMemoryCardRecordDto>;
    /// Commits one explicit typed rollback relation and its new card revision
    /// atomically.
    ///
    /// # Errors
    ///
    /// Returns `memory_replacement_conflict` for an invalid relation or a
    /// missing restored revision, and the store validation failures.
    fn rollback_goal_memory_card(
        &self,
        input: GoalMemoryCardRollbackInputDto,
    ) -> DtoResult<GoalMemoryCardRecordDto>;
    /// Counts the active memory card identities of one owner scope.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the count cannot be read.
    fn count_active_goal_memory_cards(&self, scope: GoalRecordScopeDto) -> DtoResult<u64>;
    /// Stores one bounded Skill card revision.
    ///
    /// # Errors
    ///
    /// Returns `skill_reference_unavailable` when the exact revision is
    /// already bound to different content, `skill_entry_too_large` when the
    /// card exceeds its bound, or an unavailable error when the atomic commit
    /// fails.
    fn store_goal_skill_card(
        &self,
        input: GoalSkillCardRecordDto,
    ) -> DtoResult<GoalSkillCardRecordDto>;
    /// Loads one immutable Skill card revision.
    ///
    /// # Errors
    ///
    /// Returns `skill_reference_unavailable` when the exact revision is
    /// absent, or an unavailable error when it cannot be read.
    fn load_goal_skill_card(
        &self,
        skill_id: String,
        revision: u64,
    ) -> DtoResult<GoalSkillCardRecordDto>;
    /// Stores one bounded role card revision.
    ///
    /// # Errors
    ///
    /// Returns `delegation_role_invalid` when the exact revision is already
    /// bound to different content, or an unavailable error when the atomic
    /// commit fails.
    fn store_goal_role_card(
        &self,
        input: GoalRoleCardRecordDto,
    ) -> DtoResult<GoalRoleCardRecordDto>;
    /// Loads one immutable role card revision.
    ///
    /// # Errors
    ///
    /// Returns `delegation_role_invalid` when the exact revision is absent, or
    /// an unavailable error when it cannot be read.
    fn load_goal_role_card(
        &self,
        role_id: String,
        revision: u64,
    ) -> DtoResult<GoalRoleCardRecordDto>;
}

/// DTO-only repository contract for coalesced refinement proposals.
pub trait GoalProposalRepositoryDto {
    /// Proposes one inactive refinement draft with evidence coalescing.
    ///
    /// An equal proposal coalesces into the single pending draft of the Goal;
    /// an unequal proposal is a typed conflict until that draft is decided.
    ///
    /// # Errors
    ///
    /// Returns `refinement_draft_conflict` when an unequal proposal already
    /// owns the pending slot, `refinement_draft_too_large` when the draft
    /// exceeds its bound, or an unavailable error when the atomic commit
    /// fails.
    fn propose_refinement_draft(
        &self,
        input: RefinementDraftRecordDto,
    ) -> DtoResult<RefinementDraftRecordDto>;
    /// Loads the single pending refinement draft of one Goal, if any.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the draft cannot be read.
    fn load_pending_refinement_draft(
        &self,
        leading_goal_id: String,
    ) -> DtoResult<Option<RefinementDraftRecordDto>>;
    /// Decides one pending refinement draft without changing an active Goal.
    ///
    /// # Errors
    ///
    /// Returns `refinement_draft_conflict` when the draft is not pending, or
    /// an unavailable error when the atomic commit fails.
    fn decide_refinement_draft(
        &self,
        draft_id: String,
        state: RefinementDraftStateDto,
        decided_at_ms: u64,
    ) -> DtoResult<RefinementDraftRecordDto>;
}

/// DTO-only repository contract for conversation-summary compaction records.
pub trait GoalCompactionRepositoryDto {
    /// Appends one completed history reference of one working-form scope.
    ///
    /// # Errors
    ///
    /// Returns `compaction_history_unavailable` for an unusable reference or
    /// an unavailable error when the atomic append fails.
    fn record_compaction_suffix_reference(
        &self,
        input: RecordCompactionSuffixReferenceInputDto,
    ) -> DtoResult<()>;
    /// Stores one immutable summary revision and advances the working form
    /// atomically.
    ///
    /// # Errors
    ///
    /// Returns `compaction_history_unavailable` when no completed range is
    /// available or the range does not extend the working form,
    /// `compaction_summary_unavailable` for a predecessor mismatch,
    /// `compaction_summary_too_large` when the content exceeds its bound, or
    /// an unavailable error when the atomic commit fails.
    fn store_conversation_summary(
        &self,
        input: ConversationSummaryRecordDto,
    ) -> DtoResult<ConversationSummaryRecordDto>;
    /// Loads one immutable summary revision.
    ///
    /// # Errors
    ///
    /// Returns `compaction_summary_unavailable` when the exact revision is
    /// absent, or an unavailable error when it cannot be read.
    fn load_conversation_summary(
        &self,
        summary_id: String,
        revision: u64,
    ) -> DtoResult<ConversationSummaryRecordDto>;
    /// Loads the durable working compaction form of one owner scope.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the form cannot be read.
    fn load_goal_compaction_working_form(
        &self,
        scope: GoalRecordScopeDto,
    ) -> DtoResult<GoalCompactionWorkingFormRecordDto>;
}

/// DTO-only repository contract for the unified verification records.
pub trait GoalVerificationRepositoryDto {
    /// Records one immutable delegated verifier authority revision.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` when the revision is already bound
    /// to a different digest, or an unavailable error when the atomic commit
    /// fails.
    fn record_verifier_authority(
        &self,
        input: VerifierAuthorityRecordDto,
    ) -> DtoResult<VerifierAuthorityRecordDto>;
    /// Loads one exact verifier authority revision.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_invalid` when the exact revision is absent,
    /// or an unavailable error when it cannot be read.
    fn load_verifier_authority(
        &self,
        authority_id: String,
        authority_revision: u64,
    ) -> DtoResult<VerifierAuthorityRecordDto>;
    /// Revokes one authority revision without rewriting its history.
    ///
    /// # Errors
    ///
    /// Returns `verifier_authority_revoked` when the revision is already
    /// revoked, `verifier_authority_invalid` when it is absent, or an
    /// unavailable error when the atomic commit fails.
    fn revoke_verifier_authority(
        &self,
        input: RevokeVerifierAuthorityInputDto,
    ) -> DtoResult<VerifierAuthorityRecordDto>;
    /// Records one immutable verifier audit baseline.
    ///
    /// # Errors
    ///
    /// Returns `verifier_baseline_invalid` when the baseline is absent or
    /// already bound to a different digest, or an unavailable error when the
    /// atomic commit fails.
    fn record_verifier_audit_baseline(
        &self,
        input: VerifierAuditBaselineRecordDto,
    ) -> DtoResult<VerifierAuditBaselineRecordDto>;
    /// Loads one immutable verifier audit baseline by digest.
    ///
    /// # Errors
    ///
    /// Returns `verifier_baseline_invalid` when the baseline is absent, or an
    /// unavailable error when it cannot be read.
    fn load_verifier_audit_baseline(
        &self,
        canonical_baseline_digest: String,
    ) -> DtoResult<VerifierAuditBaselineRecordDto>;
    /// Records one immutable verifier audit evidence record.
    ///
    /// # Errors
    ///
    /// Returns `verifier_evidence_invalid` when the evidence identity is
    /// already bound to a different digest, or an unavailable error when the
    /// atomic commit fails.
    fn record_verifier_audit_evidence(
        &self,
        input: VerifierAuditEvidenceRecordDto,
    ) -> DtoResult<VerifierAuditEvidenceRecordDto>;
    /// Loads one immutable verifier audit evidence record.
    ///
    /// # Errors
    ///
    /// Returns `verifier_evidence_invalid` when the evidence is absent, or an
    /// unavailable error when it cannot be read.
    fn load_verifier_audit_evidence(
        &self,
        evidence_id: String,
    ) -> DtoResult<VerifierAuditEvidenceRecordDto>;
    /// Records one immutable verifier audit verdict record.
    ///
    /// # Errors
    ///
    /// Returns `verifier_verdict_invalid` when the verdict identity is already
    /// bound to a different digest, or an unavailable error when the atomic
    /// commit fails.
    fn record_verifier_audit_verdict(
        &self,
        input: VerifierAuditVerdictRecordDto,
    ) -> DtoResult<VerifierAuditVerdictRecordDto>;
    /// Loads one immutable verifier audit verdict record.
    ///
    /// # Errors
    ///
    /// Returns `verifier_verdict_invalid` when the verdict is absent, or an
    /// unavailable error when it cannot be read.
    fn load_verifier_audit_verdict(
        &self,
        verdict_id: String,
    ) -> DtoResult<VerifierAuditVerdictRecordDto>;
    /// Applies one target mutation atomically against an active, unrevoked,
    /// unexpired, unconsumed authority and a fresh frozen baseline.
    ///
    /// An earlier consumed authority rejects with
    /// `verifier_authority_consumed`, a revoked authority with
    /// `verifier_authority_revoked`, an expired authority with
    /// `verifier_authority_expired`, a disallowed operation with
    /// `verifier_authority_operation_not_allowed`, and a stale or missing
    /// baseline with `verifier_baseline_stale_target_revision` or
    /// `verifier_baseline_invalid`. An equal repeated operation replays its
    /// existing binding without applying a second mutation.
    ///
    /// # Errors
    ///
    /// Returns the typed authority, baseline, or mutation rejections above, or
    /// an unavailable error when the atomic commit fails.
    fn apply_verifier_target_mutation(
        &self,
        input: VerifierTargetMutationRecordDto,
    ) -> DtoResult<ApplyVerifierMutationOutcomeDto>;
    /// Loads one committed target mutation.
    ///
    /// # Errors
    ///
    /// Returns `verifier_mutation_invalid` when the mutation is absent, or an
    /// unavailable error when it cannot be read.
    fn load_verifier_target_mutation(
        &self,
        mutation_id: String,
    ) -> DtoResult<VerifierTargetMutationRecordDto>;
}
