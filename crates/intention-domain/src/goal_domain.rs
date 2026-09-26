//! Goal identity, scope, tree, lifecycle, readiness, gates, working records,
//! and the closed Goal-domain failure set.
//!
//! Owner: architecture 28 (Goal domain and verification), with architecture
//! 21 context, memory, and compaction semantics, ADR 0023 directions, and the
//! ADR 0044 Slice 3 activation. This module owns Goal identity, scope, and
//! tree vocabulary with obligatory children and explicit link rules;
//! lifecycle, readiness, and user-decision state machines with exception
//! evidence; the leading-goal admission rules over the frozen
//! [`GoalRunSelectionV1`]; reference and executable gates with user-created
//! typed templates; working memory cards, Skills, roles, gate templates,
//! refinement drafts, and the compaction working form; the code-owned bounds;
//! and the closed `goal_*`/`memory_*`/`skill_*`/`delegation_*`/`compaction_*`/
//! `refinement_draft_*` pre-effect failure set.
//!
//! The module is pure domain logic: it performs no I/O, no storage, no wire
//! encoding, and no current-state reconstruction. Durable admission
//! transactions live in `intention-application` and
//! `intention-storage-sqlite`; delegated Verification Mandate authority,
//! target-set, verdict, and mutation records live in `crate::verification`;
//! model-request context projection stays Milestone 12.

use std::collections::HashSet;

use intention_types::{DtoResult, ErrorDto};

use crate::canonical::{Digest256, contains_control_or_nul, contains_credential_shape};

pub use crate::slice3_selections::{
    GOAL_MAX_COMPONENT_REFERENCES, GOAL_MAX_CONTEXT_BYTES, GOAL_MAX_EVIDENCE_REFERENCES,
    GOAL_MAX_GATE_REVISIONS, GOAL_MAX_MEMORY_CARDS as GOAL_MAX_ACTIVE_MEMORY_CARDS,
    GOAL_MAX_PARENT_CHAIN, GOAL_MAX_REVEALED_RECORDS,
    GOAL_MAX_SKILL_ROLE_CARDS as GOAL_MAX_SELECTED_SKILL_ROLE_CARDS,
    GOAL_MAX_TARGET_SNAPSHOT_BYTES, GoalCardReferenceV1, GoalGateRevisionReferenceV1,
    GoalRevisionReferenceV1, GoalRunKindV1, GoalRunSelectionBoundsV1, GoalRunSelectionV1,
    GoalScopeLinkProvenanceV1,
};

/// The closed Goal-domain failure code for an aggregate limit.
pub const GOAL_LIMIT_EXCEEDED: &str = "goal_limit_exceeded";
/// The closed Goal-domain failure code for the Goal-tree depth bound.
pub const GOAL_TREE_DEPTH_LIMIT_EXCEEDED: &str = "goal_tree_depth_limit_exceeded";
/// The closed Goal-domain failure code for the direct-child bound.
pub const GOAL_CHILD_LIMIT_EXCEEDED: &str = "goal_child_limit_exceeded";
/// The closed Goal-domain failure code for the session-link bound.
pub const GOAL_SESSION_LINK_LIMIT_EXCEEDED: &str = "goal_session_link_limit_exceeded";
/// The closed Goal-domain failure code for a self-link, cycle, duplicate
/// direct child, or link rule violation.
pub const GOAL_CYCLE_DETECTED: &str = "goal_cycle_detected";
/// The closed Goal-domain failure code for a non-Active or non-admitting Goal.
pub const GOAL_NOT_ACTIVE: &str = "goal_not_active";
/// The closed Goal-domain failure code for an inexact or stale revision.
pub const GOAL_REVISION_CONFLICT: &str = "goal_revision_conflict";
/// The closed Goal-domain failure code for an oversized Goal snapshot.
pub const GOAL_SNAPSHOT_TOO_LARGE: &str = "goal_snapshot_too_large";
/// The closed Goal-domain failure code for a missing Goal snapshot.
pub const GOAL_SNAPSHOT_UNAVAILABLE: &str = "goal_snapshot_unavailable";
/// The closed Goal-domain failure code for the gate bound.
pub const GOAL_GATE_LIMIT_EXCEEDED: &str = "goal_gate_limit_exceeded";
/// The closed Goal-domain failure code for an unavailable gate or template.
pub const GOAL_GATE_UNAVAILABLE: &str = "goal_gate_unavailable";
/// The closed Goal-domain failure code for a failed or invalid gate outcome.
pub const GOAL_GATE_FAILED: &str = "goal_gate_failed";
/// The closed Goal-domain failure code for an incomplete readiness claim.
pub const GOAL_NOT_READY: &str = "goal_not_ready";
/// The closed Goal-domain failure code for an invalid exception set.
pub const GOAL_ACCEPTANCE_EXCEPTION_INVALID: &str = "goal_acceptance_exception_invalid";
/// The closed Goal-domain failure code for archiving a non-terminal Goal.
pub const GOAL_ARCHIVE_NOT_TERMINAL: &str = "goal_archive_not_terminal";
/// The closed Goal-domain failure code for the active memory card bound.
pub const MEMORY_ENTRY_LIMIT_EXCEEDED: &str = "memory_entry_limit_exceeded";
/// The closed Goal-domain failure code for an oversized memory record.
pub const MEMORY_ENTRY_TOO_LARGE: &str = "memory_entry_too_large";
/// The closed Goal-domain failure code for an unavailable memory reference.
pub const MEMORY_REFERENCE_UNAVAILABLE: &str = "memory_reference_unavailable";
/// The closed Goal-domain failure code for an invalid replacement or
/// rollback relation.
pub const MEMORY_REPLACEMENT_CONFLICT: &str = "memory_replacement_conflict";
/// The closed Goal-domain failure code for an oversized Skill record.
pub const SKILL_ENTRY_TOO_LARGE: &str = "skill_entry_too_large";
/// The closed Goal-domain failure code for an unavailable Skill reference.
pub const SKILL_REFERENCE_UNAVAILABLE: &str = "skill_reference_unavailable";
/// The closed Goal-domain failure code for an invalid role record.
pub const DELEGATION_ROLE_INVALID: &str = "delegation_role_invalid";
/// The closed Goal-domain failure code for a role that widens its parent.
pub const DELEGATION_ROLE_WIDENING_FORBIDDEN: &str = "delegation_role_widening_forbidden";
/// The closed Goal-domain failure code for an oversized summary.
pub const COMPACTION_SUMMARY_TOO_LARGE: &str = "compaction_summary_too_large";
/// The closed Goal-domain failure code for an unavailable summary reference.
pub const COMPACTION_SUMMARY_UNAVAILABLE: &str = "compaction_summary_unavailable";
/// The closed Goal-domain failure code for an unavailable history range.
pub const COMPACTION_HISTORY_UNAVAILABLE: &str = "compaction_history_unavailable";
/// The closed Goal-domain failure code for a pending-draft or stale-base
/// conflict.
pub const REFINEMENT_DRAFT_CONFLICT: &str = "refinement_draft_conflict";
/// The closed Goal-domain failure code for an oversized refinement draft.
pub const REFINEMENT_DRAFT_TOO_LARGE: &str = "refinement_draft_too_large";
/// The shared cross-cutting failure code for credential-shaped content.
pub const CREDENTIALS_FORBIDDEN: &str = "credentials_forbidden";

/// The closed Goal-domain failure set in architecture 28 order.
///
/// Every code exists exactly once in this module and is a typed pre-effect
/// rejection through [`ErrorDto`]. The set discloses no credential, path, raw
/// external response, private process resource, grant, full record content,
/// proposal text, provider resource, or implementation detail.
pub const GOAL_FAILURE_CODES: &[&str] = &[
    GOAL_LIMIT_EXCEEDED,
    GOAL_TREE_DEPTH_LIMIT_EXCEEDED,
    GOAL_CHILD_LIMIT_EXCEEDED,
    GOAL_SESSION_LINK_LIMIT_EXCEEDED,
    GOAL_CYCLE_DETECTED,
    GOAL_NOT_ACTIVE,
    GOAL_REVISION_CONFLICT,
    GOAL_SNAPSHOT_TOO_LARGE,
    GOAL_SNAPSHOT_UNAVAILABLE,
    GOAL_GATE_LIMIT_EXCEEDED,
    GOAL_GATE_UNAVAILABLE,
    GOAL_GATE_FAILED,
    GOAL_NOT_READY,
    GOAL_ACCEPTANCE_EXCEPTION_INVALID,
    GOAL_ARCHIVE_NOT_TERMINAL,
    MEMORY_ENTRY_LIMIT_EXCEEDED,
    MEMORY_ENTRY_TOO_LARGE,
    MEMORY_REFERENCE_UNAVAILABLE,
    MEMORY_REPLACEMENT_CONFLICT,
    SKILL_ENTRY_TOO_LARGE,
    SKILL_REFERENCE_UNAVAILABLE,
    DELEGATION_ROLE_INVALID,
    DELEGATION_ROLE_WIDENING_FORBIDDEN,
    COMPACTION_SUMMARY_TOO_LARGE,
    COMPACTION_SUMMARY_UNAVAILABLE,
    COMPACTION_HISTORY_UNAVAILABLE,
    REFINEMENT_DRAFT_CONFLICT,
    REFINEMENT_DRAFT_TOO_LARGE,
];

/// Maximum Goals in one project.
pub const GOAL_MAX_GOALS_PER_PROJECT: usize = 256;
/// Maximum Goals in one session.
pub const GOAL_MAX_GOALS_PER_SESSION: usize = 64;
/// Maximum Goal-tree depth, counting the root Goal as depth one.
pub const GOAL_MAX_TREE_DEPTH: u32 = 16;
/// Maximum direct children of one Goal.
pub const GOAL_MAX_DIRECT_CHILDREN: u32 = 32;
/// Maximum explicit session links of one project Goal.
pub const GOAL_MAX_SESSION_LINKS_PER_PROJECT_GOAL: usize = 64;
/// Maximum gates of one Goal.
pub const GOAL_MAX_GATES_PER_GOAL: usize = 32;
/// Maximum bytes of one full memory, skill, role, template, summary, draft, or
/// safe gate result record (512 KiB).
pub const GOAL_MAX_FULL_RECORD_BYTES: u64 = 512 * 1024;
/// Pending proposal bound per owner scope and record kind.
pub const GOAL_MAX_PENDING_PROPOSALS: usize = 1;

/// Returns one byte count as a saturating `u64`.
fn byte_len(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

/// Returns one bound as a saturating `usize`.
fn bound_as_usize(bound: u64) -> usize {
    usize::try_from(bound).unwrap_or(usize::MAX)
}

/// Validates one Goal-domain text value against the safe-record policy.
///
/// # Errors
///
/// Returns `invalid_code` for a blank or control-bearing value,
/// [`CREDENTIALS_FORBIDDEN`] for a credential-shaped value, and
/// `too_large_code` for a value over `max_bytes`.
fn validate_goal_text(
    value: &str,
    max_bytes: u64,
    invalid_code: &'static str,
    too_large_code: &'static str,
) -> DtoResult<()> {
    if value.trim().is_empty() || contains_control_or_nul(value) {
        return Err(ErrorDto::validation(
            invalid_code,
            "goal-domain text must be non-blank and control-free",
        ));
    }
    if contains_credential_shape(value) {
        return Err(ErrorDto::validation(
            CREDENTIALS_FORBIDDEN,
            "credentials are forbidden",
        ));
    }
    if byte_len(value.len()) > max_bytes {
        return Err(ErrorDto::validation(
            too_large_code,
            "goal-domain content exceeds its byte bound",
        ));
    }
    Ok(())
}

/// The exactly-one scope of one Goal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalScopeDto {
    /// A project Goal; it reaches a session only through an explicit durable
    /// link and never by implication.
    Project {
        /// The owning project identity.
        project_id: [u8; 16],
    },
    /// A session Goal; it belongs only to its one session.
    Session {
        /// The owning project identity.
        project_id: [u8; 16],
        /// The owning session identity.
        session_id: [u8; 16],
    },
}

impl GoalScopeDto {
    /// Returns the owning project identity.
    #[must_use]
    pub const fn project_id(&self) -> [u8; 16] {
        match self {
            Self::Project { project_id } | Self::Session { project_id, .. } => *project_id,
        }
    }

    /// Returns the owning session identity for a session Goal.
    #[must_use]
    pub const fn session_id(&self) -> Option<[u8; 16]> {
        match self {
            Self::Project { .. } => None,
            Self::Session { session_id, .. } => Some(*session_id),
        }
    }

    /// Returns whether this is a project Goal.
    #[must_use]
    pub const fn is_project(&self) -> bool {
        matches!(self, Self::Project { .. })
    }
}

/// The closed Goal work lifecycle states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalLifecycleStateDto {
    /// The Goal accepts ordinary and verification work.
    Active,
    /// A required gate failed; only an explicit new run restores work.
    NeedsRework,
    /// The user paused the Goal and its non-terminal subtree.
    Paused,
    /// The user stopped the Goal; stop is terminal.
    Stopped,
    /// The terminal Goal is archived as readable history.
    Archived,
}

impl GoalLifecycleStateDto {
    /// Returns whether no further work lifecycle transition is valid.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped | Self::Archived)
    }
}

/// The closed kinds of durable reference accepted as gate evidence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GoalEvidenceKindV1 {
    /// A terminal child Goal user-decision result.
    TerminalChildResult,
    /// An accepted user declaration.
    AcceptedUserDeclaration,
    /// A terminal registered-tool result.
    TerminalRegisteredToolResult,
    /// An executable gate result with its distinct provenance.
    ExecutableGateResult,
}

/// One exact selected successful evidence reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalEvidenceReferenceV1 {
    /// The daemon-assigned evidence identity.
    pub evidence_id: [u8; 16],
    /// The exact immutable evidence revision.
    pub revision: u64,
    /// The closed evidence kind.
    pub kind: GoalEvidenceKindV1,
}

impl GoalEvidenceReferenceV1 {
    /// Creates one exact selected evidence reference.
    #[must_use]
    pub const fn new(evidence_id: [u8; 16], revision: u64, kind: GoalEvidenceKindV1) -> Self {
        Self {
            evidence_id,
            revision,
            kind,
        }
    }

    /// Returns whether the reference is exact and nonzero.
    fn is_exact(&self) -> bool {
        self.evidence_id != [0; 16] && self.revision > 0
    }
}

/// The technical readiness of one Goal.
///
/// `Ready` means the exact effective revision, every obligatory child, and
/// every required gate have selected successful evidence. It is not a model
/// statement and is not synonymous with user acceptance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GoalReadinessStateDto {
    /// The exact evidence set is incomplete.
    NotReady,
    /// Every required element has selected successful evidence.
    Ready {
        /// The selected successful evidence references.
        verified_evidence_set: Vec<GoalEvidenceReferenceV1>,
    },
}

impl GoalReadinessStateDto {
    /// Creates a ready state from selected successful evidence.
    ///
    /// # Errors
    ///
    /// Returns [`GOAL_NOT_READY`] for an empty or inexact evidence set and
    /// [`GOAL_REVISION_CONFLICT`] when one evidence identity is selected at
    /// more than one revision.
    pub fn ready(verified_evidence_set: Vec<GoalEvidenceReferenceV1>) -> DtoResult<Self> {
        let readiness = Self::Ready {
            verified_evidence_set,
        };
        readiness.validate()?;
        Ok(readiness)
    }

    /// Validates this readiness state.
    ///
    /// # Errors
    ///
    /// Returns [`GOAL_NOT_READY`] for an empty or inexact evidence set,
    /// [`GOAL_LIMIT_EXCEEDED`] when the evidence bound is exceeded, and
    /// [`GOAL_REVISION_CONFLICT`] when one evidence identity is selected at
    /// more than one revision.
    pub fn validate(&self) -> DtoResult<()> {
        let Self::Ready {
            verified_evidence_set,
        } = self
        else {
            return Ok(());
        };
        if verified_evidence_set.is_empty() {
            return Err(ErrorDto::validation(
                GOAL_NOT_READY,
                "Ready requires at least one selected successful evidence reference",
            ));
        }
        if verified_evidence_set.len() > GOAL_MAX_EVIDENCE_REFERENCES {
            return Err(ErrorDto::validation(
                GOAL_LIMIT_EXCEEDED,
                "the readiness evidence set exceeds the selected evidence bound",
            ));
        }
        let mut seen = HashSet::new();
        for evidence in verified_evidence_set {
            if !evidence.is_exact() {
                return Err(ErrorDto::validation(
                    GOAL_NOT_READY,
                    "readiness evidence must name an exact nonzero evidence revision",
                ));
            }
            if !seen.insert(evidence.evidence_id) {
                return Err(ErrorDto::validation(
                    GOAL_REVISION_CONFLICT,
                    "a readiness set selects one revision of any evidence identity",
                ));
            }
        }
        Ok(())
    }

    /// Returns whether this Goal claims technical readiness.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    /// Returns the selected successful evidence references.
    #[must_use]
    pub fn verified_evidence_set(&self) -> &[GoalEvidenceReferenceV1] {
        match self {
            Self::NotReady => &[],
            Self::Ready {
                verified_evidence_set,
            } => verified_evidence_set,
        }
    }
}

/// The closed exception kinds a user may accept.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalGateExceptionKindV1 {
    /// A known failed gate.
    Failed,
    /// A known unavailable gate or template.
    Unavailable,
    /// An expired gate evidence revision.
    Expired,
    /// A gate whose external effect is ambiguous.
    ExternallyAmbiguous,
}

/// One recorded gate exception with its safe evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalGateExceptionV1 {
    /// The gate identity.
    pub gate_id: [u8; 16],
    /// The exact gate revision.
    pub gate_revision: u64,
    /// The closed exception kind.
    pub kind: GoalGateExceptionKindV1,
    /// The safe evidence reference of the exception.
    pub evidence: GoalEvidenceReferenceV1,
}

impl GoalGateExceptionV1 {
    /// Returns whether the exception names an exact gate and evidence.
    fn is_exact(&self) -> bool {
        self.gate_id != [0; 16] && self.gate_revision > 0 && self.evidence.is_exact()
    }
}

/// One inherited obligatory-child exception carried upward.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalInheritedExceptionV1 {
    /// The originating child Goal identity.
    pub child_goal_id: [u8; 16],
    /// The child exception itself.
    pub exception: GoalGateExceptionV1,
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
        exception_evidence_set: Vec<GoalGateExceptionV1>,
    },
}

impl GoalUserDecisionStateDto {
    /// Creates a validated acceptance-with-exception state.
    ///
    /// # Errors
    ///
    /// Returns [`GOAL_ACCEPTANCE_EXCEPTION_INVALID`] for an empty, inexact, or
    /// duplicate-gate exception set and [`GOAL_GATE_LIMIT_EXCEEDED`] when the
    /// exception set exceeds the gate bound.
    pub fn accepted_with_exception(
        exception_evidence_set: Vec<GoalGateExceptionV1>,
    ) -> DtoResult<Self> {
        let state = Self::AcceptedWithException {
            exception_evidence_set,
        };
        state.validate()?;
        Ok(state)
    }

    /// Validates this user-decision state.
    ///
    /// # Errors
    ///
    /// Returns [`GOAL_ACCEPTANCE_EXCEPTION_INVALID`] for an empty, inexact, or
    /// duplicate-gate exception set and [`GOAL_GATE_LIMIT_EXCEEDED`] when the
    /// exception set exceeds the gate bound.
    pub fn validate(&self) -> DtoResult<()> {
        let Self::AcceptedWithException {
            exception_evidence_set,
        } = self
        else {
            return Ok(());
        };
        if exception_evidence_set.is_empty() {
            return Err(ErrorDto::validation(
                GOAL_ACCEPTANCE_EXCEPTION_INVALID,
                "an exception set must name at least one known gate exception",
            ));
        }
        if exception_evidence_set.len() > GOAL_MAX_GATES_PER_GOAL {
            return Err(ErrorDto::validation(
                GOAL_GATE_LIMIT_EXCEEDED,
                "the exception set exceeds the gate bound of one Goal",
            ));
        }
        let mut seen = HashSet::new();
        for exception in exception_evidence_set {
            if !exception.is_exact() {
                return Err(ErrorDto::validation(
                    GOAL_ACCEPTANCE_EXCEPTION_INVALID,
                    "an exception must name an exact nonzero gate and evidence revision",
                ));
            }
            if !seen.insert(exception.gate_id) {
                return Err(ErrorDto::validation(
                    GOAL_ACCEPTANCE_EXCEPTION_INVALID,
                    "an exception set names each gate once",
                ));
            }
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
    pub fn exception_evidence_set(&self) -> &[GoalGateExceptionV1] {
        match self {
            Self::AcceptedWithException {
                exception_evidence_set,
            } => exception_evidence_set,
            Self::Unaccepted | Self::Accepted => &[],
        }
    }
}

/// One coherent Goal acceptance and evidence record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalDto {
    goal_id: [u8; 16],
    scope: GoalScopeDto,
    active_revision: u64,
    lifecycle_state: GoalLifecycleStateDto,
    readiness_state: GoalReadinessStateDto,
    user_decision_state: GoalUserDecisionStateDto,
}

impl GoalDto {
    /// Creates one coherent Goal record.
    ///
    /// # Errors
    ///
    /// Returns [`GOAL_NOT_ACTIVE`] for a zero goal identity,
    /// [`GOAL_REVISION_CONFLICT`] for a zero active revision,
    /// [`GOAL_NOT_READY`] when readiness or acceptance claims are incoherent,
    /// [`GOAL_ACCEPTANCE_EXCEPTION_INVALID`] when an exception set accompanies
    /// readiness, and the nested readiness or exception validation failures.
    pub fn new(
        goal_id: [u8; 16],
        scope: GoalScopeDto,
        active_revision: u64,
        lifecycle_state: GoalLifecycleStateDto,
        readiness_state: GoalReadinessStateDto,
        user_decision_state: GoalUserDecisionStateDto,
    ) -> DtoResult<Self> {
        if goal_id == [0; 16] {
            return Err(ErrorDto::validation(
                GOAL_NOT_ACTIVE,
                "goal identity must be daemon-assigned and nonzero",
            ));
        }
        if active_revision == 0 {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "the active Goal revision must be positive",
            ));
        }
        readiness_state.validate()?;
        user_decision_state.validate()?;
        if lifecycle_state == GoalLifecycleStateDto::NeedsRework && readiness_state.is_ready() {
            return Err(ErrorDto::validation(
                GOAL_NOT_READY,
                "a NeedsRework Goal cannot claim readiness until an explicit new run",
            ));
        }
        match &user_decision_state {
            GoalUserDecisionStateDto::Accepted => {
                if !readiness_state.is_ready() {
                    return Err(ErrorDto::validation(
                        GOAL_NOT_READY,
                        "user acceptance requires a Ready Goal",
                    ));
                }
            }
            GoalUserDecisionStateDto::AcceptedWithException { .. } => {
                if readiness_state.is_ready() {
                    return Err(ErrorDto::validation(
                        GOAL_ACCEPTANCE_EXCEPTION_INVALID,
                        "an exception never creates Ready",
                    ));
                }
            }
            GoalUserDecisionStateDto::Unaccepted => {}
        }
        Ok(Self {
            goal_id,
            scope,
            active_revision,
            lifecycle_state,
            readiness_state,
            user_decision_state,
        })
    }

    /// Returns the daemon-assigned Goal identity.
    #[must_use]
    pub const fn goal_id(&self) -> [u8; 16] {
        self.goal_id
    }

    /// Returns the exactly-one Goal scope.
    #[must_use]
    pub const fn scope(&self) -> GoalScopeDto {
        self.scope
    }

    /// Returns the exact active revision.
    #[must_use]
    pub const fn active_revision(&self) -> u64 {
        self.active_revision
    }

    /// Returns the Goal work lifecycle state.
    #[must_use]
    pub const fn lifecycle_state(&self) -> GoalLifecycleStateDto {
        self.lifecycle_state
    }

    /// Returns the technical readiness state.
    #[must_use]
    pub const fn readiness_state(&self) -> &GoalReadinessStateDto {
        &self.readiness_state
    }

    /// Returns the separate user-decision state.
    #[must_use]
    pub const fn user_decision_state(&self) -> &GoalUserDecisionStateDto {
        &self.user_decision_state
    }
}

/// One immutable Goal revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalRevisionDto {
    /// The Goal identity.
    pub goal_id: [u8; 16],
    /// The immutable revision number.
    pub revision: u64,
    /// The bounded safe title.
    pub title: String,
    /// The bounded safe objective.
    pub objective: String,
    /// The inherited rule references of the exact parent revision.
    pub inherited_rule_references: Vec<[u8; 16]>,
    /// The local rule references added by this revision.
    pub local_rule_references: Vec<[u8; 16]>,
    /// The required gate revision references.
    pub required_gate_references: Vec<GoalGateRevisionReferenceV1>,
    /// The canonical revision digest.
    pub canonical_revision_digest: Digest256,
}

impl GoalRevisionDto {
    /// Validates this immutable revision.
    ///
    /// # Errors
    ///
    /// Returns [`GOAL_NOT_ACTIVE`] for a missing goal or rule identity,
    /// [`GOAL_REVISION_CONFLICT`] for a zero or duplicate gate revision,
    /// [`GOAL_GATE_LIMIT_EXCEEDED`] when the required gate bound is exceeded,
    /// [`GOAL_LIMIT_EXCEEDED`] for oversized text, and
    /// [`CREDENTIALS_FORBIDDEN`] for credential-shaped text.
    pub fn validate(&self) -> DtoResult<()> {
        if self.goal_id == [0; 16] {
            return Err(ErrorDto::validation(
                GOAL_NOT_ACTIVE,
                "a Goal revision requires its daemon-assigned Goal identity",
            ));
        }
        if self.revision == 0 {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "a Goal revision number is positive",
            ));
        }
        validate_goal_text(
            &self.title,
            GOAL_MAX_FULL_RECORD_BYTES,
            GOAL_REVISION_CONFLICT,
            GOAL_LIMIT_EXCEEDED,
        )?;
        validate_goal_text(
            &self.objective,
            GOAL_MAX_FULL_RECORD_BYTES,
            GOAL_REVISION_CONFLICT,
            GOAL_LIMIT_EXCEEDED,
        )?;
        if self.required_gate_references.len() > GOAL_MAX_GATES_PER_GOAL {
            return Err(ErrorDto::validation(
                GOAL_GATE_LIMIT_EXCEEDED,
                "the required gate bound of one Goal is exceeded",
            ));
        }
        let mut seen = HashSet::new();
        for reference in &self.required_gate_references {
            if reference.gate_reference == [0; 16] || reference.revision == 0 {
                return Err(ErrorDto::validation(
                    GOAL_REVISION_CONFLICT,
                    "a required gate reference is exact and nonzero",
                ));
            }
            if !seen.insert(reference.gate_reference) {
                return Err(ErrorDto::validation(
                    GOAL_REVISION_CONFLICT,
                    "a revision names one revision of any required gate identity",
                ));
            }
        }
        for reference in self
            .inherited_rule_references
            .iter()
            .chain(&self.local_rule_references)
        {
            if *reference == [0; 16] {
                return Err(ErrorDto::validation(
                    GOAL_NOT_ACTIVE,
                    "a rule reference is daemon-assigned and nonzero",
                ));
            }
        }
        Ok(())
    }
}

/// One obligatory parent-to-child Goal link.
///
/// Every child is an obligatory component: the parent is not technically
/// ready until each child reaches a terminal user-decision state, so there is
/// no optional-child variant and no `required = false` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalParentLinkDto {
    /// The parent Goal identity.
    pub parent_goal_id: [u8; 16],
    /// The child Goal identity.
    pub child_goal_id: [u8; 16],
    /// The exact child revision at link time.
    pub child_revision_at_link: u64,
    /// The canonical link digest.
    pub canonical_link_digest: Digest256,
}

/// One explicit durable project-Goal-to-session link.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalSessionLinkDto {
    /// The daemon-assigned Goal link identity.
    pub link_id: [u8; 16],
    /// The linked project Goal identity.
    pub project_goal_id: [u8; 16],
    /// The linked session identity.
    pub session_id: [u8; 16],
    /// The revision from which the link is effective.
    pub effective_from_revision: u64,
    /// The canonical link digest.
    pub canonical_link_digest: Digest256,
}

/// The typed disposition of one admissible child link.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalChildLinkDispositionV1 {
    /// The owner session already holds the explicit durable link.
    ExistingSessionLink,
    /// The child creation must atomically record the required session link.
    CreateSessionLinkAtomically,
}

/// Validates one lifecycle transition.
///
/// Archive and restore are validated separately by [`validate_goal_archive`]
/// and [`validate_goal_restore`].
///
/// # Errors
///
/// Returns [`GOAL_NOT_ACTIVE`] when the transition is not a declared edge.
pub fn validate_goal_lifecycle_transition(
    from: GoalLifecycleStateDto,
    to: GoalLifecycleStateDto,
) -> DtoResult<()> {
    use GoalLifecycleStateDto::{Active, Archived, NeedsRework, Paused, Stopped};
    let allowed = matches!(
        (from, to),
        (Active, NeedsRework)
            | (Active, Paused)
            | (Active, Stopped)
            | (NeedsRework, Active)
            | (NeedsRework, Paused)
            | (NeedsRework, Stopped)
            | (Paused, Active)
            | (Paused, Stopped)
            | (Archived, Active)
            | (Archived, Stopped)
    );
    if allowed {
        Ok(())
    } else {
        Err(ErrorDto::validation(
            GOAL_NOT_ACTIVE,
            "the Goal lifecycle transition is not permitted",
        ))
    }
}

/// Validates archiving one terminal idle Goal.
///
/// # Errors
///
/// Returns [`GOAL_ARCHIVE_NOT_TERMINAL`] when the Goal is already archived or
/// is neither stopped nor user-accepted.
pub fn validate_goal_archive(
    lifecycle_state: GoalLifecycleStateDto,
    user_decision_state: &GoalUserDecisionStateDto,
) -> DtoResult<()> {
    if lifecycle_state == GoalLifecycleStateDto::Archived {
        return Err(ErrorDto::validation(
            GOAL_ARCHIVE_NOT_TERMINAL,
            "the Goal is already archived",
        ));
    }
    if lifecycle_state == GoalLifecycleStateDto::Stopped || user_decision_state.is_terminal() {
        return Ok(());
    }
    Err(ErrorDto::validation(
        GOAL_ARCHIVE_NOT_TERMINAL,
        "only a terminal idle Goal may be archived",
    ))
}

/// Validates restoring one archived Goal.
///
/// Restore changes presentation only and never launches a run.
///
/// # Errors
///
/// Returns [`GOAL_ARCHIVE_NOT_TERMINAL`] when the Goal is not archived.
pub fn validate_goal_restore(lifecycle_state: GoalLifecycleStateDto) -> DtoResult<()> {
    if lifecycle_state == GoalLifecycleStateDto::Archived {
        Ok(())
    } else {
        Err(ErrorDto::validation(
            GOAL_ARCHIVE_NOT_TERMINAL,
            "only an archived Goal may be restored",
        ))
    }
}

/// Validates one readiness claim against obligatory children and gates.
///
/// # Errors
///
/// Returns [`GOAL_NOT_READY`] for an empty evidence set, an unresolved
/// obligatory child, or a missing required-gate evidence, and the nested
/// readiness validation failures.
pub fn validate_goal_readiness_claim(
    verified_evidence_set: Vec<GoalEvidenceReferenceV1>,
    unresolved_obligatory_children: &[[u8; 16]],
    required_gate_evidence_missing: bool,
) -> DtoResult<GoalReadinessStateDto> {
    if verified_evidence_set.is_empty() {
        return Err(ErrorDto::validation(
            GOAL_NOT_READY,
            "Ready requires selected successful evidence",
        ));
    }
    if !unresolved_obligatory_children.is_empty() {
        return Err(ErrorDto::validation(
            GOAL_NOT_READY,
            "every obligatory child must reach a terminal user-decision state",
        ));
    }
    if required_gate_evidence_missing {
        return Err(ErrorDto::validation(
            GOAL_NOT_READY,
            "every required gate must have selected successful evidence",
        ));
    }
    GoalReadinessStateDto::ready(verified_evidence_set)
}

/// The closed user acceptance decisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GoalAcceptanceDecisionV1 {
    /// Accept a Ready Goal.
    Accept,
    /// Accept with an explicit gate exception set.
    AcceptWithException {
        /// The accepted exception evidence set.
        exception_evidence_set: Vec<GoalGateExceptionV1>,
    },
}

/// One user acceptance request with its exception context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalAcceptanceRequestV1 {
    /// The requested decision.
    pub decision: GoalAcceptanceDecisionV1,
    /// The current technical readiness.
    pub readiness_state: GoalReadinessStateDto,
    /// The inherited obligatory-child exceptions.
    pub inherited_child_exceptions: Vec<GoalInheritedExceptionV1>,
    /// The active obligatory children without a terminal decision.
    pub unresolved_obligatory_children: Vec<[u8; 16]>,
}

/// Validates one user acceptance decision.
///
/// # Errors
///
/// Returns [`GOAL_NOT_READY`] when plain acceptance is requested for a
/// non-Ready Goal or with an unresolved child, and
/// [`GOAL_ACCEPTANCE_EXCEPTION_INVALID`] for an invalid exception set, a Ready
/// Goal carrying exceptions, an omitted inherited child exception, or a
/// bypassed active obligatory child.
pub fn validate_goal_acceptance(
    request: &GoalAcceptanceRequestV1,
) -> DtoResult<GoalUserDecisionStateDto> {
    match &request.decision {
        GoalAcceptanceDecisionV1::Accept => {
            if !request.inherited_child_exceptions.is_empty() {
                return Err(ErrorDto::validation(
                    GOAL_ACCEPTANCE_EXCEPTION_INVALID,
                    "a Ready acceptance carries no inherited child exception",
                ));
            }
            if !request.unresolved_obligatory_children.is_empty() {
                return Err(ErrorDto::validation(
                    GOAL_NOT_READY,
                    "every obligatory child must reach a terminal user-decision state",
                ));
            }
            if !request.readiness_state.is_ready() {
                return Err(ErrorDto::validation(
                    GOAL_NOT_READY,
                    "user acceptance requires a Ready Goal",
                ));
            }
            Ok(GoalUserDecisionStateDto::Accepted)
        }
        GoalAcceptanceDecisionV1::AcceptWithException {
            exception_evidence_set,
        } => {
            let decision =
                GoalUserDecisionStateDto::accepted_with_exception(exception_evidence_set.clone())?;
            if request.readiness_state.is_ready() {
                return Err(ErrorDto::validation(
                    GOAL_ACCEPTANCE_EXCEPTION_INVALID,
                    "an exception never creates Ready",
                ));
            }
            if !request.unresolved_obligatory_children.is_empty() {
                return Err(ErrorDto::validation(
                    GOAL_ACCEPTANCE_EXCEPTION_INVALID,
                    "an exception cannot omit, cancel, or bypass an active obligatory child",
                ));
            }
            for inherited in &request.inherited_child_exceptions {
                if inherited.child_goal_id == [0; 16]
                    || !exception_evidence_set.contains(&inherited.exception)
                {
                    return Err(ErrorDto::validation(
                        GOAL_ACCEPTANCE_EXCEPTION_INVALID,
                        "the parent exception set must explicitly include each inherited child exception",
                    ));
                }
            }
            Ok(decision)
        }
    }
}

/// Validates that a candidate Goal is not its own ancestor.
///
/// # Errors
///
/// Returns [`GOAL_NOT_ACTIVE`] for a missing daemon-assigned identity and
/// [`GOAL_CYCLE_DETECTED`] when the candidate appears in its own ancestor
/// chain.
pub fn validate_goal_no_cycle(
    candidate_goal_id: [u8; 16],
    ancestor_chain: &[[u8; 16]],
) -> DtoResult<()> {
    if candidate_goal_id == [0; 16] || ancestor_chain.contains(&[0; 16]) {
        return Err(ErrorDto::validation(
            GOAL_NOT_ACTIVE,
            "goal identities must be daemon-assigned and nonzero",
        ));
    }
    if ancestor_chain.contains(&candidate_goal_id) {
        return Err(ErrorDto::validation(
            GOAL_CYCLE_DETECTED,
            "a Goal cannot be its own ancestor",
        ));
    }
    Ok(())
}

/// Validates one obligatory parent-to-child link against the scope rules.
///
/// # Errors
///
/// Returns [`GOAL_CYCLE_DETECTED`] for a self-link, a cross-project link, a
/// session Goal owning a project or foreign-session child, or an out-of-link
/// session child whose link creation is not allowed, and
/// [`GOAL_REVISION_CONFLICT`] for an unset child revision.
pub fn validate_goal_child_link(
    link: &GoalParentLinkDto,
    parent_scope: GoalScopeDto,
    child_scope: GoalScopeDto,
    owner_session_link_present: bool,
    allow_atomic_session_link: bool,
) -> DtoResult<GoalChildLinkDispositionV1> {
    if link.parent_goal_id == link.child_goal_id {
        return Err(ErrorDto::validation(
            GOAL_CYCLE_DETECTED,
            "a Goal cannot be its own child",
        ));
    }
    if link.child_revision_at_link == 0 {
        return Err(ErrorDto::validation(
            GOAL_REVISION_CONFLICT,
            "the child revision at link time must be exact and nonzero",
        ));
    }
    if parent_scope.project_id() != child_scope.project_id() {
        return Err(ErrorDto::validation(
            GOAL_CYCLE_DETECTED,
            "a Goal child never crosses a project",
        ));
    }
    match (parent_scope, child_scope) {
        (GoalScopeDto::Project { .. }, GoalScopeDto::Project { .. }) => {
            Ok(GoalChildLinkDispositionV1::ExistingSessionLink)
        }
        (GoalScopeDto::Project { .. }, GoalScopeDto::Session { .. }) => {
            if owner_session_link_present {
                Ok(GoalChildLinkDispositionV1::ExistingSessionLink)
            } else if allow_atomic_session_link {
                Ok(GoalChildLinkDispositionV1::CreateSessionLinkAtomically)
            } else {
                Err(ErrorDto::validation(
                    GOAL_CYCLE_DETECTED,
                    "a session child requires the explicit durable link of its owner session",
                ))
            }
        }
        (
            GoalScopeDto::Session {
                session_id: parent_session,
                ..
            },
            GoalScopeDto::Session {
                session_id: child_session,
                ..
            },
        ) => {
            if parent_session == child_session {
                Ok(GoalChildLinkDispositionV1::ExistingSessionLink)
            } else {
                Err(ErrorDto::validation(
                    GOAL_CYCLE_DETECTED,
                    "a session Goal cannot own another session's child",
                ))
            }
        }
        (GoalScopeDto::Session { .. }, GoalScopeDto::Project { .. }) => Err(ErrorDto::validation(
            GOAL_CYCLE_DETECTED,
            "a session Goal cannot own a project child",
        )),
    }
}

/// Validates one new child attachment against depth and child bounds.
///
/// Returns the child depth, counting the root Goal as depth one.
///
/// # Errors
///
/// Returns [`GOAL_TREE_DEPTH_LIMIT_EXCEEDED`] for a missing parent depth or an
/// attachment below the depth bound and [`GOAL_CHILD_LIMIT_EXCEEDED`] when the
/// parent already holds the direct-child bound.
pub fn validate_goal_tree_attachment(
    parent_depth: u32,
    parent_direct_children: u32,
) -> DtoResult<u32> {
    if parent_depth == 0 || parent_depth >= GOAL_MAX_TREE_DEPTH {
        return Err(ErrorDto::validation(
            GOAL_TREE_DEPTH_LIMIT_EXCEEDED,
            "the Goal-tree depth bound is exceeded",
        ));
    }
    if parent_direct_children >= GOAL_MAX_DIRECT_CHILDREN {
        return Err(ErrorDto::validation(
            GOAL_CHILD_LIMIT_EXCEEDED,
            "the direct-child bound of one Goal is exceeded",
        ));
    }
    Ok(parent_depth + 1)
}

/// Validates the direct children of one Goal.
///
/// # Errors
///
/// Returns [`GOAL_CHILD_LIMIT_EXCEEDED`] when the direct-child bound is
/// exceeded, [`GOAL_NOT_ACTIVE`] for a missing goal identity, and
/// [`GOAL_CYCLE_DETECTED`] for a duplicate direct child.
pub fn validate_goal_direct_children(children: &[GoalParentLinkDto]) -> DtoResult<()> {
    if children.len() > bound_as_usize(u64::from(GOAL_MAX_DIRECT_CHILDREN)) {
        return Err(ErrorDto::validation(
            GOAL_CHILD_LIMIT_EXCEEDED,
            "the direct-child bound of one Goal is exceeded",
        ));
    }
    let mut seen = HashSet::new();
    for child in children {
        if child.parent_goal_id == [0; 16] || child.child_goal_id == [0; 16] {
            return Err(ErrorDto::validation(
                GOAL_NOT_ACTIVE,
                "a parent link names daemon-assigned nonzero identities",
            ));
        }
        if child.child_revision_at_link == 0 {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "the child revision at link time must be exact and nonzero",
            ));
        }
        if !seen.insert(child.child_goal_id) {
            return Err(ErrorDto::validation(
                GOAL_CYCLE_DETECTED,
                "a duplicate direct child is rejected before a partial record",
            ));
        }
    }
    Ok(())
}

/// Validates the Goal allocation of one project and one session.
///
/// # Errors
///
/// Returns [`GOAL_LIMIT_EXCEEDED`] when the project or session Goal bound is
/// exceeded.
pub fn validate_goal_allocation(goals_in_project: usize, goals_in_session: usize) -> DtoResult<()> {
    if goals_in_project > GOAL_MAX_GOALS_PER_PROJECT
        || goals_in_session > GOAL_MAX_GOALS_PER_SESSION
    {
        return Err(ErrorDto::validation(
            GOAL_LIMIT_EXCEEDED,
            "the project or session Goal bound is exceeded",
        ));
    }
    Ok(())
}

/// Validates the explicit session links of one project Goal.
///
/// # Errors
///
/// Returns [`GOAL_SESSION_LINK_LIMIT_EXCEEDED`] when the link bound is
/// exceeded, [`GOAL_NOT_ACTIVE`] for a foreign goal or invalid link identity,
/// and [`GOAL_REVISION_CONFLICT`] for a duplicate session link.
pub fn validate_goal_session_links(
    links: &[GoalSessionLinkDto],
    project_goal_id: [u8; 16],
) -> DtoResult<()> {
    if links.len() > GOAL_MAX_SESSION_LINKS_PER_PROJECT_GOAL {
        return Err(ErrorDto::validation(
            GOAL_SESSION_LINK_LIMIT_EXCEEDED,
            "the session link bound of one project Goal is exceeded",
        ));
    }
    let mut seen = HashSet::new();
    for link in links {
        if link.link_id == [0; 16]
            || link.project_goal_id != project_goal_id
            || link.session_id == [0; 16]
            || link.effective_from_revision == 0
        {
            return Err(ErrorDto::validation(
                GOAL_NOT_ACTIVE,
                "a session link must be exact, effective, and owned by this project Goal",
            ));
        }
        if !seen.insert(link.session_id) {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "one project Goal holds one link per session identity",
            ));
        }
    }
    Ok(())
}

/// One user-created gate definition with its daemon-assigned identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalGateDefinitionV1 {
    /// The daemon-assigned gate identity.
    pub gate_id: [u8; 16],
    /// The closed gate definition.
    pub gate: VerificationGateDto,
}

/// The closed verification gate definitions.
///
/// A `ReferenceGate` validates an exact durable reference. An `ExecutableGate`
/// names a user-created typed template at one exact revision; it carries no
/// raw shell text, arbitrary path/URL/header map, executable code, opaque
/// JSON, or model-supplied provider resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationGateDto {
    /// A gate over an exact durable reference.
    ReferenceGate {
        /// The exact evidence contract revision.
        evidence_contract_revision: u64,
        /// The accepted durable reference kinds.
        accepted_reference_kinds: Vec<GoalEvidenceKindV1>,
    },
    /// A gate over one user-created typed template revision.
    ExecutableGate {
        /// The template identity.
        template_id: [u8; 16],
        /// The exact template revision.
        template_revision: u64,
    },
}

impl VerificationGateDto {
    /// Validates this gate definition.
    ///
    /// # Errors
    ///
    /// Returns [`GOAL_GATE_UNAVAILABLE`] for a missing contract revision,
    /// empty or duplicate accepted kinds, or a missing template identity or
    /// revision.
    pub fn validate(&self) -> DtoResult<()> {
        match self {
            Self::ReferenceGate {
                evidence_contract_revision,
                accepted_reference_kinds,
            } => {
                if *evidence_contract_revision == 0 || accepted_reference_kinds.is_empty() {
                    return Err(ErrorDto::validation(
                        GOAL_GATE_UNAVAILABLE,
                        "a reference gate requires an exact evidence contract and accepted kinds",
                    ));
                }
                let mut seen = HashSet::new();
                if accepted_reference_kinds
                    .iter()
                    .any(|kind| !seen.insert(*kind))
                {
                    return Err(ErrorDto::validation(
                        GOAL_GATE_UNAVAILABLE,
                        "a reference gate names each accepted evidence kind once",
                    ));
                }
                Ok(())
            }
            Self::ExecutableGate {
                template_id,
                template_revision,
            } => {
                if *template_id == [0; 16] || *template_revision == 0 {
                    return Err(ErrorDto::validation(
                        GOAL_GATE_UNAVAILABLE,
                        "an executable gate requires an exact nonzero template revision",
                    ));
                }
                Ok(())
            }
        }
    }

    /// Returns whether this gate executes a user-created template.
    #[must_use]
    pub const fn is_executable(&self) -> bool {
        matches!(self, Self::ExecutableGate { .. })
    }
}

/// The closed template scope families.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalTemplateScopeDto {
    /// A project-scoped template.
    Project {
        /// The owning project identity.
        project_id: [u8; 16],
    },
    /// A Goal-scoped template.
    Goal {
        /// The owning Goal identity.
        goal_id: [u8; 16],
    },
    /// A session-scoped template.
    Session {
        /// The owning session identity.
        session_id: [u8; 16],
    },
}

/// The closed typed input families of a gate template.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalGateInputFamilyV1 {
    /// One bounded closed text input.
    ClosedTextV1,
    /// One exact set of workspace-relative logical paths.
    ClosedPathSetV1,
}

/// The closed gate-template lifecycle states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalTemplateLifecycleStateDto {
    /// The template may be selected by future gates.
    Enabled,
    /// The template is archived and readable history.
    Archived,
}

/// The provenance of one gate template creation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalTemplateProvenanceV1 {
    /// The user created the template directly.
    User,
    /// The model prepared a template proposal.
    ModelProposal {
        /// The accepted refinement draft identity.
        draft_id: [u8; 16],
        /// Whether the user explicitly confirmed the proposal.
        accepted_by_user: bool,
    },
}

/// One user-created immutable gate template card.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalGateTemplateCardV1 {
    /// The template identity.
    pub template_id: [u8; 16],
    /// The immutable template revision.
    pub revision: u64,
    /// The template scope.
    pub scope: GoalTemplateScopeDto,
    /// The registered capability reference.
    pub capability_reference: [u8; 16],
    /// The one closed typed input family.
    pub input_family: GoalGateInputFamilyV1,
    /// Whether execution requires explicit user confirmation.
    pub requires_confirmation: bool,
    /// The canonical card digest.
    pub canonical_digest: Digest256,
}

impl GoalGateTemplateCardV1 {
    /// Validates this template card.
    ///
    /// # Errors
    ///
    /// Returns [`GOAL_GATE_UNAVAILABLE`] for a missing template, capability,
    /// revision, or scope identity.
    pub fn validate(&self) -> DtoResult<()> {
        if self.template_id == [0; 16] || self.capability_reference == [0; 16] || self.revision == 0
        {
            return Err(ErrorDto::validation(
                GOAL_GATE_UNAVAILABLE,
                "a gate template requires daemon-assigned identities and an exact revision",
            ));
        }
        let scope_identity = match self.scope {
            GoalTemplateScopeDto::Project { project_id } => project_id,
            GoalTemplateScopeDto::Goal { goal_id } => goal_id,
            GoalTemplateScopeDto::Session { session_id } => session_id,
        };
        if scope_identity == [0; 16] {
            return Err(ErrorDto::validation(
                GOAL_GATE_UNAVAILABLE,
                "a gate template requires an exact owner scope identity",
            ));
        }
        Ok(())
    }
}

/// Validates the gate definitions of one Goal.
///
/// # Errors
///
/// Returns [`GOAL_GATE_LIMIT_EXCEEDED`] when the gate bound is exceeded,
/// [`GOAL_GATE_UNAVAILABLE`] for an unusable gate definition, and
/// [`GOAL_REVISION_CONFLICT`] for a duplicate gate identity.
pub fn validate_goal_gate_definitions(definitions: &[GoalGateDefinitionV1]) -> DtoResult<()> {
    if definitions.len() > GOAL_MAX_GATES_PER_GOAL {
        return Err(ErrorDto::validation(
            GOAL_GATE_LIMIT_EXCEEDED,
            "the gate bound of one Goal is exceeded",
        ));
    }
    let mut seen = HashSet::new();
    for definition in definitions {
        if definition.gate_id == [0; 16] {
            return Err(ErrorDto::validation(
                GOAL_GATE_UNAVAILABLE,
                "a gate definition requires its daemon-assigned identity",
            ));
        }
        definition.gate.validate()?;
        if !seen.insert(definition.gate_id) {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "a Goal names one revision of any gate identity",
            ));
        }
    }
    Ok(())
}

/// Validates the selected gate revision references of one Goal.
///
/// # Errors
///
/// Returns [`GOAL_GATE_LIMIT_EXCEEDED`] when the selected gate revision bound
/// is exceeded and [`GOAL_REVISION_CONFLICT`] for an inexact or duplicate gate
/// reference.
pub fn validate_goal_gate_revisions(references: &[GoalGateRevisionReferenceV1]) -> DtoResult<()> {
    if references.len() > GOAL_MAX_GATE_REVISIONS || references.len() > GOAL_MAX_GATES_PER_GOAL {
        return Err(ErrorDto::validation(
            GOAL_GATE_LIMIT_EXCEEDED,
            "the selected gate revision bound of one Goal is exceeded",
        ));
    }
    let mut seen = HashSet::new();
    for reference in references {
        if reference.gate_reference == [0; 16] || reference.revision == 0 {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "a selected gate reference is exact and nonzero",
            ));
        }
        if !seen.insert(reference.gate_reference) {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "a Goal selects one revision of any gate identity",
            ));
        }
    }
    Ok(())
}

/// Validates one user-driven gate-template creation.
///
/// A model may only prepare a template proposal under the proposal rules; it
/// never enables a template without explicit user confirmation.
///
/// # Errors
///
/// Returns [`GOAL_GATE_UNAVAILABLE`] for an unusable template card, a missing
/// proposal identity, or an unconfirmed model proposal.
pub fn validate_gate_template_creation(
    template: &GoalGateTemplateCardV1,
    provenance: &GoalTemplateProvenanceV1,
) -> DtoResult<()> {
    template.validate()?;
    match provenance {
        GoalTemplateProvenanceV1::User => Ok(()),
        GoalTemplateProvenanceV1::ModelProposal {
            draft_id,
            accepted_by_user,
        } => {
            if *draft_id == [0; 16] || !*accepted_by_user {
                Err(ErrorDto::validation(
                    GOAL_GATE_UNAVAILABLE,
                    "a model-prepared template proposal requires explicit user confirmation",
                ))
            } else {
                Ok(())
            }
        }
    }
}

/// Validates gate-template scope applicability for one run.
///
/// # Errors
///
/// Returns [`GOAL_GATE_UNAVAILABLE`] when the template scope is outside the
/// current project, selected Goal chain, or session.
pub fn validate_gate_template_scope(
    scope: &GoalTemplateScopeDto,
    context: &GoalApplicabilityContextV1,
) -> DtoResult<()> {
    match scope {
        GoalTemplateScopeDto::Project { project_id } if *project_id == context.project_id => Ok(()),
        GoalTemplateScopeDto::Goal { goal_id } if context.selected_goal_ids.contains(goal_id) => {
            Ok(())
        }
        GoalTemplateScopeDto::Session { session_id } if *session_id == context.session_id => Ok(()),
        _ => Err(ErrorDto::validation(
            GOAL_GATE_UNAVAILABLE,
            "the gate template scope is unavailable in this run",
        )),
    }
}

/// Validates one explicit gate-template lifecycle transition.
///
/// # Errors
///
/// Returns [`GOAL_GATE_UNAVAILABLE`] for any undeclared create, archive, or
/// restore edge.
pub fn validate_gate_template_lifecycle_transition(
    from: Option<GoalTemplateLifecycleStateDto>,
    to: GoalTemplateLifecycleStateDto,
) -> DtoResult<()> {
    use GoalTemplateLifecycleStateDto::{Archived, Enabled};
    let allowed = matches!(
        (from, to),
        (None, Enabled) | (Some(Enabled), Archived) | (Some(Archived), Enabled)
    );
    if allowed {
        Ok(())
    } else {
        Err(ErrorDto::validation(
            GOAL_GATE_UNAVAILABLE,
            "the gate template lifecycle transition is not permitted",
        ))
    }
}

/// Validates that one gate reference resolves against the exact template.
///
/// # Errors
///
/// Returns [`GOAL_GATE_UNAVAILABLE`] for a missing, stale, or incompatible
/// template revision.
pub fn validate_gate_template_reference(
    reference: &GoalGateRevisionReferenceV1,
    template: Option<&GoalGateTemplateCardV1>,
) -> DtoResult<()> {
    let Some(template) = template else {
        return Err(ErrorDto::validation(
            GOAL_GATE_UNAVAILABLE,
            "the executable gate template is unavailable",
        ));
    };
    template.validate()?;
    if (template.template_id, template.revision) != (reference.gate_reference, reference.revision) {
        return Err(ErrorDto::validation(
            GOAL_GATE_UNAVAILABLE,
            "the gate template revision is stale or incompatible",
        ));
    }
    Ok(())
}

/// Validates the preconditions of one gate evaluation.
///
/// # Errors
///
/// Returns [`GOAL_GATE_UNAVAILABLE`] for an unusable gate definition or a
/// missing, stale, or incompatible executable template revision.
pub fn validate_gate_execution_preconditions(
    gate: &VerificationGateDto,
    template: Option<&GoalGateTemplateCardV1>,
) -> DtoResult<()> {
    gate.validate()?;
    if let VerificationGateDto::ExecutableGate {
        template_id,
        template_revision,
    } = gate
    {
        let Some(template) = template else {
            return Err(ErrorDto::validation(
                GOAL_GATE_UNAVAILABLE,
                "the executable gate template is unavailable",
            ));
        };
        template.validate()?;
        if (template.template_id, template.revision) != (*template_id, *template_revision) {
            return Err(ErrorDto::validation(
                GOAL_GATE_UNAVAILABLE,
                "the gate template revision is stale or incompatible",
            ));
        }
    }
    Ok(())
}

/// Validates one reference-gate evidence reference.
///
/// # Errors
///
/// Returns [`GOAL_GATE_FAILED`] when the gate is not a reference gate or does
/// not accept the evidence kind.
pub fn validate_reference_gate_evidence(
    gate: &VerificationGateDto,
    evidence: &GoalEvidenceReferenceV1,
) -> DtoResult<()> {
    let VerificationGateDto::ReferenceGate {
        accepted_reference_kinds,
        ..
    } = gate
    else {
        return Err(ErrorDto::validation(
            GOAL_GATE_FAILED,
            "an executable gate does not validate a durable reference directly",
        ));
    };
    if !accepted_reference_kinds.contains(&evidence.kind) {
        return Err(ErrorDto::validation(
            GOAL_GATE_FAILED,
            "the reference gate does not accept this evidence kind",
        ));
    }
    Ok(())
}

/// One typed outcome of one gate evaluation.
///
/// Every outcome is known and closed. No outcome triggers a hidden retry or a
/// next-phase readiness claim; retries occur only before the first
/// irreversible durable fact and never repeat a started external action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalGateOutcomeV1 {
    /// The gate passed with its selected evidence.
    Passed {
        /// The selected successful evidence.
        evidence: GoalEvidenceReferenceV1,
    },
    /// The gate failed with its safe evidence.
    Failed {
        /// The safe failure evidence.
        evidence: GoalEvidenceReferenceV1,
    },
    /// The gate exceeded its time budget.
    TimedOut,
    /// The gate was cancelled with its run.
    Cancelled,
    /// The gate exceeded its output bound.
    OutputBoundExceeded,
    /// The started external effect could not be proven.
    ExternalEffectUnknown {
        /// The known pre-effect or unknown-effect evidence.
        evidence: GoalEvidenceReferenceV1,
    },
    /// The exact durable reference is missing.
    ReferenceUnavailable,
    /// The selected revision is stale.
    RevisionStale,
    /// The executable template is unavailable.
    TemplateUnavailable,
}

/// The closed disposition of one gate outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalGateOutcomeDispositionV1 {
    /// The gate passed.
    Passed,
    /// The gate did not pass definitively.
    Failed,
    /// The gate could not be evaluated.
    Unavailable,
    /// The gate's external effect is unknown.
    UnknownEffect,
}

impl GoalGateOutcomeV1 {
    /// Returns the closed disposition of this outcome.
    #[must_use]
    pub const fn disposition(&self) -> GoalGateOutcomeDispositionV1 {
        match self {
            Self::Passed { .. } => GoalGateOutcomeDispositionV1::Passed,
            Self::Failed { .. } | Self::TimedOut | Self::Cancelled | Self::OutputBoundExceeded => {
                GoalGateOutcomeDispositionV1::Failed
            }
            Self::ReferenceUnavailable | Self::RevisionStale | Self::TemplateUnavailable => {
                GoalGateOutcomeDispositionV1::Unavailable
            }
            Self::ExternalEffectUnknown { .. } => GoalGateOutcomeDispositionV1::UnknownEffect,
        }
    }

    /// Returns whether this outcome is a selected success.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Passed { .. })
    }
}

/// Validates one gate outcome and returns its closed disposition.
///
/// # Errors
///
/// Returns [`GOAL_GATE_FAILED`] when a passing, failing, or unknown-effect
/// outcome does not name an exact nonzero evidence reference.
pub fn validate_gate_outcome(
    outcome: &GoalGateOutcomeV1,
) -> DtoResult<GoalGateOutcomeDispositionV1> {
    match outcome {
        GoalGateOutcomeV1::Passed { evidence }
        | GoalGateOutcomeV1::Failed { evidence }
        | GoalGateOutcomeV1::ExternalEffectUnknown { evidence } => {
            if !evidence.is_exact() {
                return Err(ErrorDto::validation(
                    GOAL_GATE_FAILED,
                    "gate outcome evidence must name an exact nonzero reference",
                ));
            }
        }
        GoalGateOutcomeV1::TimedOut
        | GoalGateOutcomeV1::Cancelled
        | GoalGateOutcomeV1::OutputBoundExceeded
        | GoalGateOutcomeV1::ReferenceUnavailable
        | GoalGateOutcomeV1::RevisionStale
        | GoalGateOutcomeV1::TemplateUnavailable => {}
    }
    Ok(outcome.disposition())
}

/// Applies one required-gate outcome to the Goal lifecycle.
///
/// # Errors
///
/// Returns [`GOAL_NOT_ACTIVE`] unless the Goal is Active or NeedsRework.
pub fn apply_required_gate_outcome(
    current: GoalLifecycleStateDto,
    disposition: GoalGateOutcomeDispositionV1,
) -> DtoResult<GoalLifecycleStateDto> {
    if !matches!(
        current,
        GoalLifecycleStateDto::Active | GoalLifecycleStateDto::NeedsRework
    ) {
        return Err(ErrorDto::validation(
            GOAL_NOT_ACTIVE,
            "only an Active or NeedsRework Goal evaluates a required gate",
        ));
    }
    Ok(match disposition {
        GoalGateOutcomeDispositionV1::Failed | GoalGateOutcomeDispositionV1::UnknownEffect => {
            GoalLifecycleStateDto::NeedsRework
        }
        GoalGateOutcomeDispositionV1::Passed | GoalGateOutcomeDispositionV1::Unavailable => current,
    })
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

/// The exactly-one durable scope of one memory, skill, or card record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalRecordScopeDto {
    /// A project record.
    Project {
        /// The owning project identity.
        project_id: [u8; 16],
    },
    /// A Goal record.
    Goal {
        /// The owning Goal identity.
        goal_id: [u8; 16],
    },
    /// A session record that stays in its session.
    Session {
        /// The owning session identity.
        session_id: [u8; 16],
    },
}

/// One bounded safe memory card.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalMemoryCardV1 {
    /// The daemon-assigned record identity.
    pub record_id: [u8; 16],
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
    pub retained_content_reference: [u8; 16],
    /// The canonical card digest.
    pub canonical_digest: Digest256,
}

impl GoalMemoryCardV1 {
    /// Validates this memory card.
    ///
    /// # Errors
    ///
    /// Returns [`MEMORY_REFERENCE_UNAVAILABLE`] for a missing identity,
    /// revision, retained-content reference, title, or purpose,
    /// [`MEMORY_ENTRY_TOO_LARGE`] when the card exceeds 512 KiB, and
    /// [`CREDENTIALS_FORBIDDEN`] for credential-shaped text.
    pub fn validate(&self) -> DtoResult<()> {
        if self.record_id == [0; 16]
            || self.revision == 0
            || self.retained_content_reference == [0; 16]
        {
            return Err(ErrorDto::validation(
                MEMORY_REFERENCE_UNAVAILABLE,
                "a memory card requires an exact record, revision, and retained-content reference",
            ));
        }
        validate_goal_text(
            &self.title,
            GOAL_MAX_FULL_RECORD_BYTES,
            MEMORY_REFERENCE_UNAVAILABLE,
            MEMORY_ENTRY_TOO_LARGE,
        )?;
        validate_goal_text(
            &self.safe_purpose,
            GOAL_MAX_FULL_RECORD_BYTES,
            MEMORY_REFERENCE_UNAVAILABLE,
            MEMORY_ENTRY_TOO_LARGE,
        )?;
        let card_bytes =
            byte_len(self.title.len()).saturating_add(byte_len(self.safe_purpose.len()));
        if card_bytes > GOAL_MAX_FULL_RECORD_BYTES {
            return Err(ErrorDto::validation(
                MEMORY_ENTRY_TOO_LARGE,
                "the memory card exceeds its 512 KiB bound",
            ));
        }
        Ok(())
    }
}

/// One explicit typed memory replacement relation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalMemoryReplacementLinkV1 {
    /// The replaced record identity.
    pub replaced_record_id: [u8; 16],
    /// The exact replaced revision.
    pub replaced_revision: u64,
    /// The replacement record identity.
    pub replacement_record_id: [u8; 16],
    /// The exact replacement revision.
    pub replacement_revision: u64,
}

/// One explicit typed memory rollback relation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalMemoryRollbackLinkV1 {
    /// The current record identity.
    pub current_record_id: [u8; 16],
    /// The exact current revision.
    pub current_revision: u64,
    /// The explicitly restored earlier record identity.
    pub restored_record_id: [u8; 16],
    /// The exact restored revision.
    pub restored_revision: u64,
}

/// The applicability context of one run for cards and templates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalApplicabilityContextV1 {
    /// The current project identity.
    pub project_id: [u8; 16],
    /// The current session identity.
    pub session_id: [u8; 16],
    /// The ordered selected Goal chain of the run.
    pub selected_goal_ids: Vec<[u8; 16]>,
}

/// Validates the applicability of one memory card.
///
/// A session-scope record stays in its session, a project record reaches a
/// session only through the selected Goal chain, and a Goal record must be in
/// the selected chain.
///
/// # Errors
///
/// Returns [`MEMORY_REFERENCE_UNAVAILABLE`] when the card scope is not
/// applicable to this run.
pub fn validate_memory_card_applicability(
    card: &GoalMemoryCardV1,
    context: &GoalApplicabilityContextV1,
) -> DtoResult<()> {
    match card.scope {
        GoalRecordScopeDto::Project { project_id } if project_id == context.project_id => Ok(()),
        GoalRecordScopeDto::Goal { goal_id } if context.selected_goal_ids.contains(&goal_id) => {
            Ok(())
        }
        GoalRecordScopeDto::Session { session_id } if session_id == context.session_id => Ok(()),
        _ => Err(ErrorDto::validation(
            MEMORY_REFERENCE_UNAVAILABLE,
            "the memory record is not applicable to this run",
        )),
    }
}

/// Validates the active applicable memory cards of one run.
///
/// # Errors
///
/// Returns [`MEMORY_ENTRY_LIMIT_EXCEEDED`] when the active card bound of 128 is
/// exceeded and the per-card applicability and revision
/// [`GOAL_REVISION_CONFLICT`] failures.
pub fn validate_memory_card_set(
    cards: &[GoalMemoryCardV1],
    context: &GoalApplicabilityContextV1,
) -> DtoResult<()> {
    if cards.len() > bound_as_usize(GOAL_MAX_ACTIVE_MEMORY_CARDS) {
        return Err(ErrorDto::validation(
            MEMORY_ENTRY_LIMIT_EXCEEDED,
            "the active applicable memory card bound of one run is exceeded",
        ));
    }
    let mut seen = HashSet::new();
    for card in cards {
        card.validate()?;
        validate_memory_card_applicability(card, context)?;
        if !seen.insert(card.record_id) {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "the run selects one revision of any memory record identity",
            ));
        }
    }
    Ok(())
}

/// Validates one explicit full-record disclosure against a frozen reference.
///
/// # Errors
///
/// Returns [`MEMORY_REFERENCE_UNAVAILABLE`] when the requested reference is
/// not the exact retained-content reference of the card.
pub fn validate_memory_disclosure(
    card: &GoalMemoryCardV1,
    retained_content_reference: [u8; 16],
) -> DtoResult<()> {
    if retained_content_reference == [0; 16]
        || retained_content_reference != card.retained_content_reference
    {
        return Err(ErrorDto::validation(
            MEMORY_REFERENCE_UNAVAILABLE,
            "full memory content is revealed only against its exact frozen reference",
        ));
    }
    card.validate()
}

/// Validates one explicit typed memory replacement relation.
///
/// # Errors
///
/// Returns [`MEMORY_REPLACEMENT_CONFLICT`] for a missing revision, a
/// self- or equal-revision replacement, or a replacement that does not advance
/// the same record identity.
pub fn validate_memory_replacement(link: &GoalMemoryReplacementLinkV1) -> DtoResult<()> {
    if link.replaced_record_id == [0; 16]
        || link.replacement_record_id == [0; 16]
        || link.replaced_revision == 0
        || link.replacement_revision == 0
    {
        return Err(ErrorDto::validation(
            MEMORY_REPLACEMENT_CONFLICT,
            "a memory replacement requires exact nonzero record identities and revisions",
        ));
    }
    if link.replaced_record_id == link.replacement_record_id
        && link.replacement_revision <= link.replaced_revision
    {
        return Err(ErrorDto::validation(
            MEMORY_REPLACEMENT_CONFLICT,
            "a same-record replacement must advance to a newer immutable revision",
        ));
    }
    Ok(())
}

/// Validates one explicit typed memory rollback relation.
///
/// Rollback creates a new immutable revision linked to an earlier record and
/// never rewrites a historical selection.
///
/// # Errors
///
/// Returns [`MEMORY_REPLACEMENT_CONFLICT`] for a missing revision or a
/// same-record rollback that does not select an earlier revision.
pub fn validate_memory_rollback(link: &GoalMemoryRollbackLinkV1) -> DtoResult<()> {
    if link.current_record_id == [0; 16]
        || link.restored_record_id == [0; 16]
        || link.current_revision == 0
        || link.restored_revision == 0
    {
        return Err(ErrorDto::validation(
            MEMORY_REPLACEMENT_CONFLICT,
            "a memory rollback requires exact nonzero record identities and revisions",
        ));
    }
    if link.current_record_id == link.restored_record_id
        && link.restored_revision >= link.current_revision
    {
        return Err(ErrorDto::validation(
            MEMORY_REPLACEMENT_CONFLICT,
            "a same-record rollback selects an earlier immutable revision",
        ));
    }
    Ok(())
}

/// One bounded Skill card with its exact revision and content reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalSkillCardV1 {
    /// The daemon-assigned Skill identity.
    pub skill_id: [u8; 16],
    /// The exact immutable Skill revision.
    pub revision: u64,
    /// The canonical lowercase letters/numbers/hyphens name.
    pub canonical_name: String,
    /// The bounded safe routing description.
    pub description: String,
    /// The exactly-one durable owner scope.
    pub owner_scope: GoalRecordScopeDto,
    /// The typed retained-content reference of the Skill body.
    pub content_reference: [u8; 16],
    /// The canonical card digest.
    pub canonical_digest: Digest256,
}

/// Validates one canonical Skill name.
///
/// # Errors
///
/// Returns [`SKILL_REFERENCE_UNAVAILABLE`] for a blank, control-bearing,
/// credential-shaped, or non-canonical name and [`SKILL_ENTRY_TOO_LARGE`] when
/// the name exceeds the record bound.
pub fn validate_goal_skill_name(name: &str) -> DtoResult<()> {
    validate_goal_text(
        name,
        GOAL_MAX_FULL_RECORD_BYTES,
        SKILL_REFERENCE_UNAVAILABLE,
        SKILL_ENTRY_TOO_LARGE,
    )?;
    let mut characters = name.chars();
    let valid_first = matches!(
        characters.next(),
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit()
    );
    let valid_rest = characters.all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
    });
    if !valid_first || !valid_rest || name.ends_with('-') {
        return Err(ErrorDto::validation(
            SKILL_REFERENCE_UNAVAILABLE,
            "a Skill name is a canonical lowercase letters/numbers/hyphens identifier",
        ));
    }
    Ok(())
}

/// Validates one Skill body or card byte size.
///
/// # Errors
///
/// Returns [`SKILL_ENTRY_TOO_LARGE`] when the content exceeds 512 KiB.
pub fn validate_skill_content_size(content_bytes: u64) -> DtoResult<()> {
    if content_bytes > GOAL_MAX_FULL_RECORD_BYTES {
        return Err(ErrorDto::validation(
            SKILL_ENTRY_TOO_LARGE,
            "Skill content exceeds its 512 KiB bound",
        ));
    }
    Ok(())
}

impl GoalSkillCardV1 {
    /// Validates this Skill card.
    ///
    /// # Errors
    ///
    /// Returns [`SKILL_REFERENCE_UNAVAILABLE`] for a missing identity,
    /// revision, content reference, name, description, or scope identity,
    /// [`SKILL_ENTRY_TOO_LARGE`] when the card exceeds 512 KiB, and
    /// [`CREDENTIALS_FORBIDDEN`] for credential-shaped text.
    pub fn validate(&self) -> DtoResult<()> {
        if self.skill_id == [0; 16] || self.revision == 0 || self.content_reference == [0; 16] {
            return Err(ErrorDto::validation(
                SKILL_REFERENCE_UNAVAILABLE,
                "a Skill card requires an exact skill, revision, and content reference",
            ));
        }
        validate_goal_skill_name(&self.canonical_name)?;
        validate_goal_text(
            &self.description,
            GOAL_MAX_FULL_RECORD_BYTES,
            SKILL_REFERENCE_UNAVAILABLE,
            SKILL_ENTRY_TOO_LARGE,
        )?;
        let scope_identity = match self.owner_scope {
            GoalRecordScopeDto::Project { project_id } => project_id,
            GoalRecordScopeDto::Goal { goal_id } => goal_id,
            GoalRecordScopeDto::Session { session_id } => session_id,
        };
        if scope_identity == [0; 16] {
            return Err(ErrorDto::validation(
                SKILL_REFERENCE_UNAVAILABLE,
                "a Skill card requires an exact owner scope identity",
            ));
        }
        Ok(())
    }
}

/// Validates the applicability of one Skill card.
///
/// # Errors
///
/// Returns [`SKILL_REFERENCE_UNAVAILABLE`] when the card scope is not
/// applicable to this run.
pub fn validate_skill_card_applicability(
    card: &GoalSkillCardV1,
    context: &GoalApplicabilityContextV1,
) -> DtoResult<()> {
    match card.owner_scope {
        GoalRecordScopeDto::Project { project_id } if project_id == context.project_id => Ok(()),
        GoalRecordScopeDto::Goal { goal_id } if context.selected_goal_ids.contains(&goal_id) => {
            Ok(())
        }
        GoalRecordScopeDto::Session { session_id } if session_id == context.session_id => Ok(()),
        _ => Err(ErrorDto::validation(
            SKILL_REFERENCE_UNAVAILABLE,
            "the Skill card is not applicable to this run",
        )),
    }
}

/// Validates one explicit Skill full-record disclosure.
///
/// # Errors
///
/// Returns [`SKILL_REFERENCE_UNAVAILABLE`] when the requested reference is not
/// the exact frozen content reference of the card.
pub fn validate_skill_disclosure(
    card: &GoalSkillCardV1,
    content_reference: [u8; 16],
) -> DtoResult<()> {
    if content_reference == [0; 16] || content_reference != card.content_reference {
        return Err(ErrorDto::validation(
            SKILL_REFERENCE_UNAVAILABLE,
            "a Skill body is disclosed only against its exact frozen reference",
        ));
    }
    card.validate()
}

/// The closed `sub_agent` execution classes a role may name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalRoleClassV1 {
    /// The light inherited class.
    Light,
    /// The medium inherited class.
    Medium,
    /// The heavy inherited class.
    Heavy,
}

impl GoalRoleClassV1 {
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

/// One named reusable child-role card.
///
/// A concrete use may only narrow the task, class, context/result limits, and
/// tool subset of the selected role; it cannot add a tool, raise a class,
/// widen scope, bypass a gate, change a provider, or weaken policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalRoleCardV1 {
    /// The daemon-assigned role identity.
    pub role_id: [u8; 16],
    /// The exact immutable role revision.
    pub revision: u64,
    /// The canonical role name.
    pub canonical_name: String,
    /// The bounded narrowed task text.
    pub task: String,
    /// The permitted child execution class.
    pub permitted_class: GoalRoleClassV1,
    /// The narrowed registered tool subset.
    pub tool_subset: Vec<String>,
    /// The narrowed context byte limit.
    pub context_limit_bytes: u64,
    /// The narrowed result byte limit.
    pub result_limit_bytes: u64,
}

impl GoalRoleCardV1 {
    /// Validates this role card.
    ///
    /// # Errors
    ///
    /// Returns [`DELEGATION_ROLE_INVALID`] for a missing identity, revision,
    /// name, or task, a zero limit, or a blank or duplicate tool identifier,
    /// and [`CREDENTIALS_FORBIDDEN`] for credential-shaped text.
    pub fn validate(&self) -> DtoResult<()> {
        if self.role_id == [0; 16] || self.revision == 0 {
            return Err(ErrorDto::validation(
                DELEGATION_ROLE_INVALID,
                "a role card requires a daemon-assigned identity and exact revision",
            ));
        }
        validate_goal_text(
            &self.canonical_name,
            GOAL_MAX_FULL_RECORD_BYTES,
            DELEGATION_ROLE_INVALID,
            DELEGATION_ROLE_INVALID,
        )?;
        validate_goal_text(
            &self.task,
            GOAL_MAX_FULL_RECORD_BYTES,
            DELEGATION_ROLE_INVALID,
            DELEGATION_ROLE_INVALID,
        )?;
        if self.context_limit_bytes == 0 || self.result_limit_bytes == 0 {
            return Err(ErrorDto::validation(
                DELEGATION_ROLE_INVALID,
                "a role card requires positive context and result limits",
            ));
        }
        let mut seen = HashSet::new();
        if self
            .tool_subset
            .iter()
            .any(|tool| tool.trim().is_empty() || !seen.insert(tool.as_str()))
        {
            return Err(ErrorDto::validation(
                DELEGATION_ROLE_INVALID,
                "a role tool subset holds unique non-blank registered tool identifiers",
            ));
        }
        Ok(())
    }
}

/// Validates one concrete role use against its selected base role.
///
/// `None` accepts any valid role card; `Some(base)` rejects a concrete use
/// that adds a tool, raises the class, or widens a context or result limit.
///
/// # Errors
///
/// Returns [`DELEGATION_ROLE_INVALID`] for an invalid concrete or base role,
/// and [`DELEGATION_ROLE_WIDENING_FORBIDDEN`] when the concrete use widens the
/// selected base role.
pub fn validate_role_narrowing(
    base: Option<&GoalRoleCardV1>,
    concrete: &GoalRoleCardV1,
) -> DtoResult<()> {
    concrete.validate()?;
    let Some(base) = base else {
        return Ok(());
    };
    base.validate()?;
    if concrete.permitted_class.rank() > base.permitted_class.rank() {
        return Err(ErrorDto::validation(
            DELEGATION_ROLE_WIDENING_FORBIDDEN,
            "a concrete role use cannot raise the permitted class",
        ));
    }
    if concrete
        .tool_subset
        .iter()
        .any(|tool| !base.tool_subset.contains(tool))
    {
        return Err(ErrorDto::validation(
            DELEGATION_ROLE_WIDENING_FORBIDDEN,
            "a concrete role use cannot add a tool",
        ));
    }
    if concrete.context_limit_bytes > base.context_limit_bytes
        || concrete.result_limit_bytes > base.result_limit_bytes
    {
        return Err(ErrorDto::validation(
            DELEGATION_ROLE_WIDENING_FORBIDDEN,
            "a concrete role use cannot widen a context or result limit",
        ));
    }
    Ok(())
}

/// Validates the selected Skill and role cards of one run.
///
/// # Errors
///
/// Returns [`GOAL_LIMIT_EXCEEDED`] when the combined 32-card bound is
/// exceeded, the per-card validation failures, and
/// [`GOAL_REVISION_CONFLICT`] for a duplicate selected card identity.
pub fn validate_skill_role_card_set(
    skills: &[GoalSkillCardV1],
    roles: &[GoalRoleCardV1],
    context: &GoalApplicabilityContextV1,
) -> DtoResult<()> {
    let total = skills.len().saturating_add(roles.len());
    if total > bound_as_usize(GOAL_MAX_SELECTED_SKILL_ROLE_CARDS) {
        return Err(ErrorDto::validation(
            GOAL_LIMIT_EXCEEDED,
            "the selected skill and role card bound of one run is exceeded",
        ));
    }
    let mut seen_skills = HashSet::new();
    for skill in skills {
        skill.validate()?;
        validate_skill_card_applicability(skill, context)?;
        if !seen_skills.insert(skill.skill_id) {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "the run selects one revision of any Skill identity",
            ));
        }
    }
    let mut seen_roles = HashSet::new();
    for role in roles {
        role.validate()?;
        if !seen_roles.insert(role.role_id) {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "the run selects one revision of any role identity",
            ));
        }
    }
    Ok(())
}

/// One frozen child-delegation snapshot.
///
/// The snapshot carries the parent task, leading-Goal identity and revision,
/// effective required constraints, applicable cards, the selected role when
/// any, and only the required safe references; never a full parent transcript
/// or live target context. Later parent edits do not change it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalDelegationSnapshotV1 {
    /// The parent task text.
    pub parent_task: String,
    /// The leading Goal identity.
    pub leading_goal_id: [u8; 16],
    /// The exact leading Goal revision.
    pub goal_revision: u64,
    /// The effective required constraints.
    pub required_constraints: Vec<String>,
    /// The applicable memory card references.
    pub memory_card_references: Vec<GoalCardReferenceV1>,
    /// The applicable Skill card references.
    pub skill_card_references: Vec<GoalCardReferenceV1>,
    /// The selected role card when one applies.
    pub selected_role: Option<GoalRoleCardV1>,
    /// The selected effective programmatic-caller policy snapshot reference.
    pub policy_snapshot_reference: [u8; 16],
    /// The selected agent-activity selection reference.
    pub activity_selection_reference: [u8; 16],
    /// The canonical snapshot digest.
    pub canonical_digest: Digest256,
}

impl GoalDelegationSnapshotV1 {
    /// Validates this frozen delegation snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`GOAL_SNAPSHOT_UNAVAILABLE`] for a missing task or reference,
    /// [`GOAL_REVISION_CONFLICT`] for an inexact revision or duplicate card,
    /// [`GOAL_SNAPSHOT_TOO_LARGE`] when the snapshot text exceeds its bound,
    /// [`GOAL_LIMIT_EXCEEDED`] when the card bound is exceeded, and the nested
    /// role validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        if self.leading_goal_id == [0; 16]
            || self.policy_snapshot_reference == [0; 16]
            || self.activity_selection_reference == [0; 16]
        {
            return Err(ErrorDto::validation(
                GOAL_SNAPSHOT_UNAVAILABLE,
                "a delegation snapshot requires daemon-assigned nonzero references",
            ));
        }
        validate_goal_text(
            &self.parent_task,
            GOAL_MAX_TARGET_SNAPSHOT_BYTES,
            GOAL_SNAPSHOT_UNAVAILABLE,
            GOAL_SNAPSHOT_TOO_LARGE,
        )?;
        if self.goal_revision == 0 {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "a delegation snapshot names an exact nonzero leading Goal revision",
            ));
        }
        let mut text_bytes = byte_len(self.parent_task.len());
        for constraint in &self.required_constraints {
            validate_goal_text(
                constraint,
                GOAL_MAX_TARGET_SNAPSHOT_BYTES,
                GOAL_SNAPSHOT_UNAVAILABLE,
                GOAL_SNAPSHOT_TOO_LARGE,
            )?;
            text_bytes = text_bytes.saturating_add(byte_len(constraint.len()));
        }
        if text_bytes > GOAL_MAX_TARGET_SNAPSHOT_BYTES {
            return Err(ErrorDto::validation(
                GOAL_SNAPSHOT_TOO_LARGE,
                "the delegation snapshot text exceeds its bound",
            ));
        }
        let card_count = self
            .memory_card_references
            .len()
            .saturating_add(self.skill_card_references.len());
        if card_count > bound_as_usize(GOAL_MAX_SELECTED_SKILL_ROLE_CARDS) {
            return Err(ErrorDto::validation(
                GOAL_LIMIT_EXCEEDED,
                "the delegation card bound is exceeded",
            ));
        }
        let mut seen = HashSet::new();
        for card in self
            .memory_card_references
            .iter()
            .chain(&self.skill_card_references)
        {
            if card.card_reference == [0; 16] || card.revision == 0 {
                return Err(ErrorDto::validation(
                    GOAL_SNAPSHOT_UNAVAILABLE,
                    "a delegation card reference is exact and nonzero",
                ));
            }
            if !seen.insert(card.card_reference) {
                return Err(ErrorDto::validation(
                    GOAL_REVISION_CONFLICT,
                    "a delegation snapshot names one revision of any card identity",
                ));
            }
        }
        if let Some(role) = &self.selected_role {
            role.validate()?;
        }
        Ok(())
    }
}

/// Validates one child delegation snapshot against its parent role.
///
/// # Errors
///
/// Returns the nested snapshot validation failures and
/// [`DELEGATION_ROLE_WIDENING_FORBIDDEN`] when the selected concrete role
/// widens the selected parent role.
pub fn validate_goal_delegation_snapshot(
    snapshot: &GoalDelegationSnapshotV1,
    parent_role: Option<&GoalRoleCardV1>,
) -> DtoResult<()> {
    snapshot.validate()?;
    if let Some(role) = &snapshot.selected_role {
        validate_role_narrowing(parent_role, role)?;
    }
    Ok(())
}

/// Validates one leading-goal admission over the frozen selection.
///
/// The admission validates the Goal and its explicit session link, the exact
/// frozen revision, the complete target snapshot, all references and bounds,
/// and DAG integrity. It never reconstructs missing meaning from current
/// state: a stale revision, an unavailable link, or an over-limit or
/// duplicate reference fails closed before external work.
///
/// # Errors
///
/// Returns [`GOAL_NOT_ACTIVE`] for a non-Active Goal, a foreign scope, or an
/// unavailable session link, [`GOAL_REVISION_CONFLICT`] for an inexact or
/// stale frozen revision and duplicate selected identities,
/// [`GOAL_TREE_DEPTH_LIMIT_EXCEEDED`],
/// [`GOAL_GATE_LIMIT_EXCEEDED`], [`MEMORY_ENTRY_LIMIT_EXCEEDED`],
/// [`GOAL_LIMIT_EXCEEDED`], [`GOAL_CYCLE_DETECTED`],
/// [`GOAL_SNAPSHOT_UNAVAILABLE`], and [`GOAL_SNAPSHOT_TOO_LARGE`] for the
/// respective pre-effect rejections.
pub fn validate_goal_run_admission(
    goal: &GoalDto,
    session_links: &[GoalSessionLinkDto],
    target_snapshot_bytes: u64,
    selection: &GoalRunSelectionV1,
) -> DtoResult<()> {
    if selection.leading_goal_id != goal.goal_id() {
        return Err(ErrorDto::validation(
            GOAL_NOT_ACTIVE,
            "the run selection names a different leading Goal",
        ));
    }
    if goal.lifecycle_state() != GoalLifecycleStateDto::Active {
        return Err(ErrorDto::validation(
            GOAL_NOT_ACTIVE,
            "only an Active Goal admits an ordinary or verification run",
        ));
    }
    if selection.goal_revision != goal.active_revision() {
        return Err(ErrorDto::validation(
            GOAL_REVISION_CONFLICT,
            "the frozen Goal revision is stale; current state never repairs an admitted selection",
        ));
    }
    let provenance = selection.scope_link_provenance;
    if provenance.project_id != goal.scope().project_id() {
        return Err(ErrorDto::validation(
            GOAL_NOT_ACTIVE,
            "the selection scope belongs to a different project",
        ));
    }
    match goal.scope() {
        GoalScopeDto::Session { session_id, .. } => {
            if provenance.session_id != Some(session_id) || provenance.link_id.is_some() {
                return Err(ErrorDto::validation(
                    GOAL_NOT_ACTIVE,
                    "a session Goal admits only its own session without a project link",
                ));
            }
        }
        GoalScopeDto::Project { .. } => {
            let (Some(session_id), Some(link_id)) = (provenance.session_id, provenance.link_id)
            else {
                return Err(ErrorDto::validation(
                    GOAL_NOT_ACTIVE,
                    "a project Goal enters a session only through an explicit durable link",
                ));
            };
            let linked = session_links.iter().any(|link| {
                link.link_id == link_id
                    && link.project_goal_id == goal.goal_id()
                    && link.session_id == session_id
                    && link.effective_from_revision > 0
            });
            if !linked {
                return Err(ErrorDto::validation(
                    GOAL_NOT_ACTIVE,
                    "the explicit project Goal session link is unavailable",
                ));
            }
        }
    }
    let bounds = selection.bounds;
    if bounds.target_snapshot_bytes == 0
        || bounds.target_snapshot_bytes > GOAL_MAX_TARGET_SNAPSHOT_BYTES
        || bounds.max_memory_cards == 0
        || bounds.max_memory_cards > GOAL_MAX_ACTIVE_MEMORY_CARDS
        || bounds.max_skill_role_cards == 0
        || bounds.max_skill_role_cards > GOAL_MAX_SELECTED_SKILL_ROLE_CARDS
        || bounds.context_bytes == 0
        || bounds.context_bytes > GOAL_MAX_CONTEXT_BYTES
    {
        return Err(ErrorDto::validation(
            GOAL_LIMIT_EXCEEDED,
            "the selected Goal-run bounds exceed the code-owned limits",
        ));
    }
    if target_snapshot_bytes == 0 {
        return Err(ErrorDto::validation(
            GOAL_SNAPSHOT_UNAVAILABLE,
            "the canonical target snapshot is unavailable",
        ));
    }
    if target_snapshot_bytes > bounds.target_snapshot_bytes
        || target_snapshot_bytes > GOAL_MAX_TARGET_SNAPSHOT_BYTES
    {
        return Err(ErrorDto::validation(
            GOAL_SNAPSHOT_TOO_LARGE,
            "the canonical target snapshot exceeds its bound",
        ));
    }
    if selection.parent_revision_chain.len().saturating_add(1)
        > bound_as_usize(u64::from(GOAL_MAX_TREE_DEPTH))
    {
        return Err(ErrorDto::validation(
            GOAL_TREE_DEPTH_LIMIT_EXCEEDED,
            "the selected parent revision chain exceeds the Goal-tree depth bound",
        ));
    }
    if selection.parent_revision_chain.len() > GOAL_MAX_PARENT_CHAIN
        || selection.obligatory_component_references.len() > GOAL_MAX_COMPONENT_REFERENCES
        || selection.valid_evidence_references.len() > GOAL_MAX_EVIDENCE_REFERENCES
        || selection.revealed_full_record_references.len() > GOAL_MAX_REVEALED_RECORDS
    {
        return Err(ErrorDto::validation(
            GOAL_LIMIT_EXCEEDED,
            "a selected Goal-run list exceeds its code-owned bound",
        ));
    }
    if selection.selected_gate_revisions.len() > GOAL_MAX_GATE_REVISIONS
        || selection.selected_gate_revisions.len() > GOAL_MAX_GATES_PER_GOAL
    {
        return Err(ErrorDto::validation(
            GOAL_GATE_LIMIT_EXCEEDED,
            "the selected gate revision bound is exceeded",
        ));
    }
    let memory_cards = u64::try_from(selection.selected_memory_cards.len()).unwrap_or(u64::MAX);
    if memory_cards > bounds.max_memory_cards || memory_cards > GOAL_MAX_ACTIVE_MEMORY_CARDS {
        return Err(ErrorDto::validation(
            MEMORY_ENTRY_LIMIT_EXCEEDED,
            "the selected active memory card bound is exceeded",
        ));
    }
    let skill_role_cards = u64::try_from(
        selection
            .selected_skill_cards
            .len()
            .saturating_add(selection.selected_role_cards.len()),
    )
    .unwrap_or(u64::MAX);
    if skill_role_cards > bounds.max_skill_role_cards
        || skill_role_cards > GOAL_MAX_SELECTED_SKILL_ROLE_CARDS
    {
        return Err(ErrorDto::validation(
            GOAL_LIMIT_EXCEEDED,
            "the selected skill and role card bound is exceeded",
        ));
    }
    let mut chain = HashSet::new();
    for reference in &selection.parent_revision_chain {
        if reference.revision == 0 || reference.goal_id == [0; 16] {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "every parent revision chain reference is exact and nonzero",
            ));
        }
        if reference.goal_id == selection.leading_goal_id || !chain.insert(reference.goal_id) {
            return Err(ErrorDto::validation(
                GOAL_CYCLE_DETECTED,
                "the parent revision chain repeats an identity or names the leading Goal",
            ));
        }
    }
    let mut components = HashSet::new();
    for component in &selection.obligatory_component_references {
        if *component == [0; 16] {
            return Err(ErrorDto::validation(
                GOAL_NOT_ACTIVE,
                "an obligatory component reference is daemon-assigned and nonzero",
            ));
        }
        if *component == selection.leading_goal_id || !components.insert(*component) {
            return Err(ErrorDto::validation(
                GOAL_CYCLE_DETECTED,
                "an obligatory component repeats an identity or names the leading Goal",
            ));
        }
    }
    let mut gates = HashSet::new();
    for reference in &selection.selected_gate_revisions {
        if reference.revision == 0 || reference.gate_reference == [0; 16] {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "every selected gate revision reference is exact and nonzero",
            ));
        }
        if !gates.insert(reference.gate_reference) {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "a selection names one revision of any gate identity",
            ));
        }
    }
    let mut cards = HashSet::new();
    for card in selection
        .selected_memory_cards
        .iter()
        .chain(&selection.selected_skill_cards)
        .chain(&selection.selected_role_cards)
    {
        if card.card_reference == [0; 16] || card.revision == 0 {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "every selected card reference is exact and nonzero",
            ));
        }
        if !cards.insert(card.card_reference) {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "a selection names one revision of any card identity",
            ));
        }
    }
    let mut evidence = HashSet::new();
    for reference in &selection.valid_evidence_references {
        if *reference == [0; 16] || !evidence.insert(*reference) {
            return Err(ErrorDto::validation(
                GOAL_REVISION_CONFLICT,
                "valid gate evidence references are exact and unique",
            ));
        }
    }
    Ok(())
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

/// The closed kinds of one bounded refinement edit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefinementEditKindV1 {
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

/// One bounded typed refinement edit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RefinementEditV1 {
    /// The closed edit kind.
    pub kind: RefinementEditKindV1,
    /// The exact evidence reference of the edit.
    pub evidence: GoalEvidenceReferenceV1,
}

/// One coalesced model refinement proposal.
///
/// A draft is not a current record, never appears in cards or model context,
/// grants no execution authority, and remains pending until an explicit user
/// decision. At most one coalesced pending draft exists per Goal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefinementDraftDto {
    /// The daemon-assigned draft identity.
    pub draft_id: [u8; 16],
    /// The selected source run identity.
    pub source_run_id: [u8; 16],
    /// The selected leading Goal identity.
    pub leading_goal_id: [u8; 16],
    /// The durable Goal milestone.
    pub milestone: GoalMilestoneDto,
    /// The exact base Goal revision.
    pub base_goal_revision: u64,
    /// The exact base record reference.
    pub base_record_reference: [u8; 16],
    /// The exact base record revision.
    pub base_record_revision: u64,
    /// The bounded typed edit set.
    pub edits: Vec<RefinementEditV1>,
    /// The evidence references of the proposal.
    pub evidence_references: Vec<GoalEvidenceReferenceV1>,
    /// The bounded safe rationale.
    pub safe_rationale: String,
    /// The canonical draft digest.
    pub canonical_digest: Digest256,
}

impl RefinementDraftDto {
    /// Validates this refinement draft.
    ///
    /// # Errors
    ///
    /// Returns [`REFINEMENT_DRAFT_CONFLICT`] for a missing identity, base
    /// revision, edit, exact evidence, or non-blank rationale,
    /// [`REFINEMENT_DRAFT_TOO_LARGE`] when the draft safe content exceeds
    /// 512 KiB, [`GOAL_LIMIT_EXCEEDED`] when the edit or evidence bound is
    /// exceeded, and [`CREDENTIALS_FORBIDDEN`] for credential-shaped text.
    pub fn validate(&self) -> DtoResult<()> {
        if self.draft_id == [0; 16]
            || self.source_run_id == [0; 16]
            || self.leading_goal_id == [0; 16]
            || self.base_record_reference == [0; 16]
        {
            return Err(ErrorDto::validation(
                REFINEMENT_DRAFT_CONFLICT,
                "a refinement draft requires daemon-assigned identities",
            ));
        }
        if self.base_goal_revision == 0 || self.base_record_revision == 0 {
            return Err(ErrorDto::validation(
                REFINEMENT_DRAFT_CONFLICT,
                "a refinement draft requires exact nonzero base revisions",
            ));
        }
        if self.edits.is_empty() {
            return Err(ErrorDto::validation(
                REFINEMENT_DRAFT_CONFLICT,
                "a refinement draft carries at least one typed edit",
            ));
        }
        if self.edits.len() > GOAL_MAX_EVIDENCE_REFERENCES
            || self.evidence_references.len() > GOAL_MAX_EVIDENCE_REFERENCES
        {
            return Err(ErrorDto::validation(
                GOAL_LIMIT_EXCEEDED,
                "the refinement draft edit or evidence bound is exceeded",
            ));
        }
        for edit in &self.edits {
            if !edit.evidence.is_exact() {
                return Err(ErrorDto::validation(
                    REFINEMENT_DRAFT_CONFLICT,
                    "every refinement edit carries exact nonzero evidence",
                ));
            }
        }
        let mut seen = HashSet::new();
        for reference in &self.evidence_references {
            if !reference.is_exact() || !seen.insert(reference.evidence_id) {
                return Err(ErrorDto::validation(
                    REFINEMENT_DRAFT_CONFLICT,
                    "a refinement draft names one revision of exact evidence identities",
                ));
            }
        }
        validate_goal_text(
            &self.safe_rationale,
            GOAL_MAX_FULL_RECORD_BYTES,
            REFINEMENT_DRAFT_CONFLICT,
            REFINEMENT_DRAFT_TOO_LARGE,
        )
    }
}

/// The typed coalescing outcome of one incoming refinement proposal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalDraftCoalescingV1 {
    /// The proposal starts a new pending draft.
    CreateNew,
    /// The equal proposal adds evidence to the one pending draft.
    CoalesceInto {
        /// The pending draft identity.
        draft_id: [u8; 16],
    },
}

/// Coalesces one incoming refinement proposal with the pending draft.
///
/// The owner scope of a Goal-domain proposal is the Goal and its record kind
/// is the Goal record, so at most one coalesced pending draft exists per Goal.
/// A later equal proposal adds evidence to that draft rather than producing an
/// unbounded queue; an unequal proposal for the same Goal is a typed conflict.
///
/// # Errors
///
/// Returns the incoming draft validation failures and
/// [`REFINEMENT_DRAFT_CONFLICT`] when an unequal proposal arrives for a Goal
/// with a pending draft.
pub fn coalesce_refinement_draft(
    pending: Option<&RefinementDraftDto>,
    incoming: &RefinementDraftDto,
) -> DtoResult<GoalDraftCoalescingV1> {
    incoming.validate()?;
    let Some(pending) = pending else {
        return Ok(GoalDraftCoalescingV1::CreateNew);
    };
    if pending.leading_goal_id != incoming.leading_goal_id {
        return Ok(GoalDraftCoalescingV1::CreateNew);
    }
    let equal = pending.milestone == incoming.milestone
        && pending.base_goal_revision == incoming.base_goal_revision
        && pending.base_record_reference == incoming.base_record_reference
        && pending.base_record_revision == incoming.base_record_revision;
    if equal {
        Ok(GoalDraftCoalescingV1::CoalesceInto {
            draft_id: pending.draft_id,
        })
    } else {
        Err(ErrorDto::validation(
            REFINEMENT_DRAFT_CONFLICT,
            "only one coalesced pending draft may exist per Goal",
        ))
    }
}

/// Validates the pending refinement drafts of one daemon scope.
///
/// # Errors
///
/// Returns the draft validation failures and
/// [`REFINEMENT_DRAFT_CONFLICT`] when a Goal holds more than
/// [`GOAL_MAX_PENDING_PROPOSALS`] pending draft.
pub fn validate_pending_draft_limit(drafts: &[RefinementDraftDto]) -> DtoResult<()> {
    let mut seen: Vec<[u8; 16]> = Vec::new();
    for draft in drafts {
        draft.validate()?;
        let pending = seen
            .iter()
            .filter(|goal_id| **goal_id == draft.leading_goal_id)
            .count();
        if pending >= GOAL_MAX_PENDING_PROPOSALS {
            return Err(ErrorDto::validation(
                REFINEMENT_DRAFT_CONFLICT,
                "the pending proposal bound of one coalesced draft per Goal is exceeded",
            ));
        }
        seen.push(draft.leading_goal_id);
    }
    Ok(())
}

/// The closed user decisions on one pending refinement draft.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RefinementDecisionV1 {
    /// Accept the pending draft.
    Accept,
    /// Edit and accept the pending draft.
    EditAndAccept {
        /// The bounded typed replacement edits.
        edits: Vec<RefinementEditV1>,
    },
    /// Reject the pending draft.
    Reject,
}

/// The typed resolution of one pending refinement draft.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GoalRefinementResolutionV1 {
    /// The draft was accepted, creating a new immutable Goal revision.
    Accepted {
        /// The new immutable Goal revision.
        goal_revision: u64,
    },
    /// The draft was edited and accepted.
    EditAccepted {
        /// The accepted bounded typed edits.
        edits: Vec<RefinementEditV1>,
        /// The new immutable Goal revision.
        goal_revision: u64,
    },
    /// The draft was rejected; no active record changed.
    Rejected,
}

/// Resolves one explicit user decision on a pending refinement draft.
///
/// Acceptance validates the exact base revision and creates a new immutable
/// record revision; a stale base is a typed conflict. Rejection changes no
/// active record and is valid even after the base advanced.
///
/// # Errors
///
/// Returns the draft validation failures and
/// [`REFINEMENT_DRAFT_CONFLICT`] for a stale base, an empty edit-and-accept,
/// inexact edit evidence, or an unadvanceable revision.
pub fn resolve_refinement_draft(
    pending: &RefinementDraftDto,
    decision: RefinementDecisionV1,
    current_goal_revision: u64,
) -> DtoResult<GoalRefinementResolutionV1> {
    pending.validate()?;
    if decision == RefinementDecisionV1::Reject {
        return Ok(GoalRefinementResolutionV1::Rejected);
    }
    if current_goal_revision == 0 || current_goal_revision != pending.base_goal_revision {
        return Err(ErrorDto::validation(
            REFINEMENT_DRAFT_CONFLICT,
            "the refinement draft base revision is stale",
        ));
    }
    let goal_revision = current_goal_revision.checked_add(1).ok_or_else(|| {
        ErrorDto::validation(
            REFINEMENT_DRAFT_CONFLICT,
            "the Goal revision cannot advance",
        )
    })?;
    match decision {
        RefinementDecisionV1::Reject => Ok(GoalRefinementResolutionV1::Rejected),
        RefinementDecisionV1::Accept => Ok(GoalRefinementResolutionV1::Accepted { goal_revision }),
        RefinementDecisionV1::EditAndAccept { edits } => {
            if edits.is_empty() {
                return Err(ErrorDto::validation(
                    REFINEMENT_DRAFT_CONFLICT,
                    "edit-and-accept carries at least one typed edit",
                ));
            }
            for edit in &edits {
                if !edit.evidence.is_exact() {
                    return Err(ErrorDto::validation(
                        REFINEMENT_DRAFT_CONFLICT,
                        "every accepted edit carries exact nonzero evidence",
                    ));
                }
            }
            Ok(GoalRefinementResolutionV1::EditAccepted {
                edits,
                goal_revision,
            })
        }
    }
}

/// One immutable compacted conversation summary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationSummaryDto {
    /// The daemon-assigned summary identity.
    pub summary_id: [u8; 16],
    /// The immutable summary revision.
    pub revision: u64,
    /// The previous summary reference when one continues a chain.
    pub previous_summary_reference: Option<[u8; 16]>,
    /// The first source history reference of the covered range.
    pub source_range_start: [u8; 16],
    /// The last source history reference of the covered range.
    pub source_range_end: [u8; 16],
    /// The bounded safe summary content.
    pub safe_content: String,
    /// The canonical summary digest.
    pub canonical_digest: Digest256,
}

impl ConversationSummaryDto {
    /// Validates this summary record.
    ///
    /// # Errors
    ///
    /// Returns [`COMPACTION_SUMMARY_UNAVAILABLE`] for a missing identity,
    /// revision, or safe content, [`COMPACTION_HISTORY_UNAVAILABLE`] for a
    /// missing source reference, [`COMPACTION_SUMMARY_TOO_LARGE`] when the
    /// content exceeds 512 KiB, and [`CREDENTIALS_FORBIDDEN`] for
    /// credential-shaped content.
    pub fn validate(&self) -> DtoResult<()> {
        if self.summary_id == [0; 16] || self.revision == 0 {
            return Err(ErrorDto::validation(
                COMPACTION_SUMMARY_UNAVAILABLE,
                "a conversation summary requires a daemon-assigned identity and exact revision",
            ));
        }
        if self.source_range_start == [0; 16] || self.source_range_end == [0; 16] {
            return Err(ErrorDto::validation(
                COMPACTION_HISTORY_UNAVAILABLE,
                "a conversation summary covers an exact completed history range",
            ));
        }
        validate_goal_text(
            &self.safe_content,
            GOAL_MAX_FULL_RECORD_BYTES,
            COMPACTION_SUMMARY_UNAVAILABLE,
            COMPACTION_SUMMARY_TOO_LARGE,
        )
    }
}

/// The working compaction form: one cumulative current summary plus the later
/// uncompacted suffix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalCompactionWorkingFormV1 {
    /// The cumulative current summary when one exists.
    pub current_summary: Option<ConversationSummaryDto>,
    /// The exact completed history references after the current summary.
    pub uncompacted_suffix: Vec<[u8; 16]>,
}

/// The closed origins of one compaction request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalCompactionOriginV1 {
    /// Inside an active user-admitted run.
    ActiveRun,
    /// Before a user-admitted run exists.
    BeforeAdmission,
    /// After the run terminalized.
    AfterTerminalization,
    /// After a daemon restart.
    AfterRestart,
}

/// Validates one summary extension of the working form.
///
/// A new revision uses the previous selected summary plus the next bounded
/// completed source range and runs only inside an active user-admitted run; it
/// executes no registered tool, creates no service run, and never becomes
/// recovery or continuation state.
///
/// # Errors
///
/// Returns [`COMPACTION_HISTORY_UNAVAILABLE`] outside an active admitted run,
/// with nothing completed to compact, when the range does not start at the
/// first uncompacted fact, or when it ends outside the completed suffix,
/// [`COMPACTION_SUMMARY_UNAVAILABLE`] for a predecessor or revision mismatch,
/// and the nested summary validation failures.
pub fn validate_compaction_extension(
    working: &GoalCompactionWorkingFormV1,
    next: &ConversationSummaryDto,
    origin: GoalCompactionOriginV1,
) -> DtoResult<()> {
    if origin != GoalCompactionOriginV1::ActiveRun {
        return Err(ErrorDto::validation(
            COMPACTION_HISTORY_UNAVAILABLE,
            "compaction runs only inside an active user-admitted run",
        ));
    }
    let Some(suffix_start) = working.uncompacted_suffix.first() else {
        return Err(ErrorDto::validation(
            COMPACTION_HISTORY_UNAVAILABLE,
            "no completed uncompacted range is available",
        ));
    };
    if *suffix_start != next.source_range_start
        || !working.uncompacted_suffix.contains(&next.source_range_end)
    {
        return Err(ErrorDto::validation(
            COMPACTION_HISTORY_UNAVAILABLE,
            "the new summary must cover the next bounded completed source range",
        ));
    }
    match &working.current_summary {
        Some(current) => {
            if next.revision != current.revision.saturating_add(1)
                || next.previous_summary_reference != Some(current.summary_id)
            {
                return Err(ErrorDto::validation(
                    COMPACTION_SUMMARY_UNAVAILABLE,
                    "a summary revision continues the exact previous selected summary",
                ));
            }
        }
        None => {
            if next.revision != 1 || next.previous_summary_reference.is_some() {
                return Err(ErrorDto::validation(
                    COMPACTION_SUMMARY_UNAVAILABLE,
                    "the first summary starts the chain at revision one",
                ));
            }
        }
    }
    next.validate()
}

/// Validates one immutable correction of an earlier summary.
///
/// A correction creates a separately immutable later revision with the exact
/// source and predecessor references; the earlier summary remains historical
/// evidence.
///
/// # Errors
///
/// Returns the corrected summary validation failures and
/// [`COMPACTION_SUMMARY_UNAVAILABLE`] when the identity, revision, source
/// range, or predecessor reference differs from the corrected summary.
pub fn validate_conversation_summary_correction(
    previous: &ConversationSummaryDto,
    corrected: &ConversationSummaryDto,
) -> DtoResult<()> {
    corrected.validate()?;
    if corrected.summary_id != previous.summary_id
        || corrected.revision != previous.revision.saturating_add(1)
        || corrected.source_range_start != previous.source_range_start
        || corrected.source_range_end != previous.source_range_end
        || corrected.previous_summary_reference != previous.previous_summary_reference
    {
        return Err(ErrorDto::validation(
            COMPACTION_SUMMARY_UNAVAILABLE,
            "a correction creates a later revision with the exact source and predecessor references",
        ));
    }
    Ok(())
}

/// Validates one fork's inherited summary reference.
///
/// A fork stores only the exact compatible selected summary reference and
/// never reads a current ancestor or imports a future summary.
///
/// # Errors
///
/// Returns the nested summary validation failures and
/// [`COMPACTION_SUMMARY_UNAVAILABLE`] when the inherited reference is not the
/// exact ancestor summary identity and revision.
pub fn validate_fork_summary_inheritance(
    ancestor: &ConversationSummaryDto,
    inherited: &ConversationSummaryDto,
) -> DtoResult<()> {
    ancestor.validate()?;
    inherited.validate()?;
    if inherited.summary_id != ancestor.summary_id || inherited.revision != ancestor.revision {
        return Err(ErrorDto::validation(
            COMPACTION_SUMMARY_UNAVAILABLE,
            "a fork inherits only the exact compatible selected summary reference",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;

    #[test]
    fn private_text_helper_rejects_control_and_credentials() {
        assert!(
            validate_goal_text("safe text", 16, GOAL_LIMIT_EXCEEDED, GOAL_LIMIT_EXCEEDED).is_ok()
        );
        assert_eq!(
            validate_goal_text(
                "control\u{0}text",
                64,
                GOAL_LIMIT_EXCEEDED,
                GOAL_LIMIT_EXCEEDED
            )
            .expect_err("control text")
            .code(),
            GOAL_LIMIT_EXCEEDED
        );
        assert_eq!(
            validate_goal_text(
                "api_key=secret-value",
                64,
                GOAL_LIMIT_EXCEEDED,
                GOAL_LIMIT_EXCEEDED
            )
            .expect_err("credential shape")
            .code(),
            CREDENTIALS_FORBIDDEN
        );
        assert_eq!(
            validate_goal_text("long value", 4, GOAL_LIMIT_EXCEEDED, GOAL_LIMIT_EXCEEDED)
                .expect_err("over bound")
                .code(),
            GOAL_LIMIT_EXCEEDED
        );
    }

    #[test]
    fn goal_scope_accessors_cover_both_scopes() {
        let project = GoalScopeDto::Project {
            project_id: [1; 16],
        };
        let session = GoalScopeDto::Session {
            project_id: [1; 16],
            session_id: [2; 16],
        };
        assert!(project.is_project());
        assert_eq!(project.session_id(), None);
        assert!(!session.is_project());
        assert_eq!(session.project_id(), [1; 16]);
        assert_eq!(session.session_id(), Some([2; 16]));
    }

    fn digest(seed: u8) -> Digest256 {
        Digest256::sha256(&[seed; 32])
    }

    fn record_bound_bytes() -> usize {
        usize::try_from(GOAL_MAX_FULL_RECORD_BYTES).expect("the record bound fits a usize")
    }

    const fn evidence(seed: u8, revision: u64) -> GoalEvidenceReferenceV1 {
        GoalEvidenceReferenceV1::new(
            [seed; 16],
            revision,
            GoalEvidenceKindV1::TerminalChildResult,
        )
    }

    const fn exception(seed: u8) -> GoalGateExceptionV1 {
        GoalGateExceptionV1 {
            gate_id: [seed; 16],
            gate_revision: 1,
            kind: GoalGateExceptionKindV1::Failed,
            evidence: evidence(seed, 1),
        }
    }

    const fn card_reference(seed: u8) -> GoalCardReferenceV1 {
        GoalCardReferenceV1 {
            card_reference: [seed; 16],
            revision: 1,
        }
    }

    fn ready_state() -> GoalReadinessStateDto {
        GoalReadinessStateDto::ready(vec![evidence(1, 1)]).expect("the ready fixture is coherent")
    }

    fn project_scope() -> GoalScopeDto {
        GoalScopeDto::Project {
            project_id: [2; 16],
        }
    }

    fn session_scope(session: u8) -> GoalScopeDto {
        GoalScopeDto::Session {
            project_id: [2; 16],
            session_id: [session; 16],
        }
    }

    fn project_goal_record() -> GoalDto {
        GoalDto::new(
            [1; 16],
            project_scope(),
            3,
            GoalLifecycleStateDto::Active,
            GoalReadinessStateDto::NotReady,
            GoalUserDecisionStateDto::Unaccepted,
        )
        .expect("the project Goal fixture is coherent")
    }

    fn session_goal_record() -> GoalDto {
        GoalDto::new(
            [7; 16],
            session_scope(3),
            1,
            GoalLifecycleStateDto::Active,
            GoalReadinessStateDto::NotReady,
            GoalUserDecisionStateDto::Unaccepted,
        )
        .expect("the session Goal fixture is coherent")
    }

    fn session_link() -> GoalSessionLinkDto {
        GoalSessionLinkDto {
            link_id: [4; 16],
            project_goal_id: [1; 16],
            session_id: [3; 16],
            effective_from_revision: 1,
            canonical_link_digest: digest(58),
        }
    }

    const fn selection_bounds() -> GoalRunSelectionBoundsV1 {
        GoalRunSelectionBoundsV1 {
            max_memory_cards: GOAL_MAX_ACTIVE_MEMORY_CARDS,
            max_skill_role_cards: GOAL_MAX_SELECTED_SKILL_ROLE_CARDS,
            target_snapshot_bytes: GOAL_MAX_TARGET_SNAPSHOT_BYTES,
            context_bytes: GOAL_MAX_CONTEXT_BYTES,
        }
    }

    fn run_selection() -> GoalRunSelectionV1 {
        GoalRunSelectionV1 {
            leading_goal_id: [1; 16],
            goal_revision: 3,
            scope_link_provenance: GoalScopeLinkProvenanceV1 {
                project_id: [2; 16],
                session_id: Some([3; 16]),
                link_id: Some([4; 16]),
            },
            parent_revision_chain: Vec::new(),
            obligatory_component_references: Vec::new(),
            selected_gate_revisions: Vec::new(),
            valid_evidence_references: Vec::new(),
            selected_memory_cards: Vec::new(),
            selected_skill_cards: Vec::new(),
            selected_role_cards: Vec::new(),
            revealed_full_record_references: Vec::new(),
            policy_snapshot_reference: [5; 16],
            activity_selection_reference: [6; 16],
            run_kind: GoalRunKindV1::GoalDirectedOrdinary,
            target_snapshot_digest: digest(50),
            bounds: selection_bounds(),
        }
    }

    fn child_link(parent: u8, child: u8, revision: u64) -> GoalParentLinkDto {
        GoalParentLinkDto {
            parent_goal_id: [parent; 16],
            child_goal_id: [child; 16],
            child_revision_at_link: revision,
            canonical_link_digest: digest(59),
        }
    }

    fn goal_revision() -> GoalRevisionDto {
        GoalRevisionDto {
            goal_id: [1; 16],
            revision: 2,
            title: "Ship the frozen slice".to_owned(),
            objective: "Deliver the frozen Slice 3 contract".to_owned(),
            inherited_rule_references: vec![[3; 16]],
            local_rule_references: vec![[4; 16]],
            required_gate_references: vec![GoalGateRevisionReferenceV1 {
                gate_reference: [5; 16],
                revision: 1,
            }],
            canonical_revision_digest: digest(6),
        }
    }

    fn reference_gate(kinds: Vec<GoalEvidenceKindV1>) -> VerificationGateDto {
        VerificationGateDto::ReferenceGate {
            evidence_contract_revision: 1,
            accepted_reference_kinds: kinds,
        }
    }

    fn executable_gate() -> VerificationGateDto {
        VerificationGateDto::ExecutableGate {
            template_id: [6; 16],
            template_revision: 2,
        }
    }

    fn template_card(scope: GoalTemplateScopeDto) -> GoalGateTemplateCardV1 {
        GoalGateTemplateCardV1 {
            template_id: [7; 16],
            revision: 2,
            scope,
            capability_reference: [8; 16],
            input_family: GoalGateInputFamilyV1::ClosedTextV1,
            requires_confirmation: true,
            canonical_digest: digest(9),
        }
    }

    fn applicability_context() -> GoalApplicabilityContextV1 {
        GoalApplicabilityContextV1 {
            project_id: [2; 16],
            session_id: [3; 16],
            selected_goal_ids: vec![[1; 16]],
        }
    }

    fn memory_card(scope: GoalRecordScopeDto) -> GoalMemoryCardV1 {
        GoalMemoryCardV1 {
            record_id: [11; 16],
            revision: 1,
            kind: MemoryKindDto::Fact,
            scope,
            title: "Goal memory".to_owned(),
            safe_purpose: "Retain the accepted decision".to_owned(),
            retained_content_reference: [12; 16],
            canonical_digest: digest(13),
        }
    }

    fn skill_card(owner_scope: GoalRecordScopeDto) -> GoalSkillCardV1 {
        GoalSkillCardV1 {
            skill_id: [21; 16],
            revision: 1,
            canonical_name: "read-file-1".to_owned(),
            description: "Read one bounded file safely".to_owned(),
            owner_scope,
            content_reference: [22; 16],
            canonical_digest: digest(23),
        }
    }

    fn role_card() -> GoalRoleCardV1 {
        GoalRoleCardV1 {
            role_id: [31; 16],
            revision: 1,
            canonical_name: "reviewer".to_owned(),
            task: "Review the bounded change".to_owned(),
            permitted_class: GoalRoleClassV1::Medium,
            tool_subset: vec!["read".to_owned(), "glob".to_owned()],
            context_limit_bytes: 4_096,
            result_limit_bytes: 2_048,
        }
    }

    fn delegation_snapshot() -> GoalDelegationSnapshotV1 {
        GoalDelegationSnapshotV1 {
            parent_task: "Implement the frozen slice".to_owned(),
            leading_goal_id: [1; 16],
            goal_revision: 3,
            required_constraints: vec!["Preserve the frozen contract".to_owned()],
            memory_card_references: vec![card_reference(41)],
            skill_card_references: vec![card_reference(42)],
            selected_role: None,
            policy_snapshot_reference: [43; 16],
            activity_selection_reference: [44; 16],
            canonical_digest: digest(45),
        }
    }

    fn refinement_edit(seed: u8) -> RefinementEditV1 {
        RefinementEditV1 {
            kind: RefinementEditKindV1::ReadinessClaim,
            evidence: evidence(seed, 1),
        }
    }

    fn refinement_draft() -> RefinementDraftDto {
        RefinementDraftDto {
            draft_id: [51; 16],
            source_run_id: [52; 16],
            leading_goal_id: [1; 16],
            milestone: GoalMilestoneDto::TechnicalReadiness,
            base_goal_revision: 3,
            base_record_reference: [53; 16],
            base_record_revision: 2,
            edits: vec![refinement_edit(54)],
            evidence_references: vec![evidence(55, 1)],
            safe_rationale: "The readiness claim follows the frozen evidence".to_owned(),
            canonical_digest: digest(56),
        }
    }

    fn summary(
        summary_id: u8,
        revision: u64,
        previous: Option<u8>,
        range_start: u8,
        range_end: u8,
    ) -> ConversationSummaryDto {
        ConversationSummaryDto {
            summary_id: [summary_id; 16],
            revision,
            previous_summary_reference: previous.map(|seed| [seed; 16]),
            source_range_start: [range_start; 16],
            source_range_end: [range_end; 16],
            safe_content: "Compacted history".to_owned(),
            canonical_digest: digest(60),
        }
    }

    #[test]
    fn lifecycle_terminal_classification_covers_every_state() {
        for state in [
            GoalLifecycleStateDto::Active,
            GoalLifecycleStateDto::NeedsRework,
            GoalLifecycleStateDto::Paused,
        ] {
            assert!(!state.is_terminal(), "{state:?} is not terminal");
        }
        for state in [
            GoalLifecycleStateDto::Stopped,
            GoalLifecycleStateDto::Archived,
        ] {
            assert!(state.is_terminal(), "{state:?} is terminal");
        }
    }

    #[test]
    fn evidence_reference_constructor_preserves_the_exact_selection() {
        let reference =
            GoalEvidenceReferenceV1::new([7; 16], 3, GoalEvidenceKindV1::ExecutableGateResult);
        assert_eq!(reference.evidence_id, [7; 16]);
        assert_eq!(reference.revision, 3);
        assert_eq!(reference.kind, GoalEvidenceKindV1::ExecutableGateResult);
    }

    #[test]
    fn readiness_validation_enforces_the_evidence_bound_and_exactness() {
        let over_bound: Vec<GoalEvidenceReferenceV1> = (0..=GOAL_MAX_EVIDENCE_REFERENCES)
            .map(|index| {
                let mut identity = [0; 16];
                identity[..8].copy_from_slice(
                    &u64::try_from(index)
                        .expect("the evidence index fits a u64")
                        .to_be_bytes(),
                );
                GoalEvidenceReferenceV1::new(
                    identity,
                    1,
                    GoalEvidenceKindV1::TerminalRegisteredToolResult,
                )
            })
            .collect();
        assert_eq!(over_bound.len(), GOAL_MAX_EVIDENCE_REFERENCES + 1);
        assert_eq!(
            GoalReadinessStateDto::ready(over_bound)
                .expect_err("over-bound readiness evidence")
                .code(),
            GOAL_LIMIT_EXCEEDED
        );
        assert_eq!(
            GoalReadinessStateDto::ready(vec![evidence(0, 1)])
                .expect_err("zero evidence identity")
                .code(),
            GOAL_NOT_READY
        );
        assert_eq!(
            GoalReadinessStateDto::ready(vec![evidence(1, 0)])
                .expect_err("zero evidence revision")
                .code(),
            GOAL_NOT_READY
        );
        assert_eq!(
            GoalReadinessStateDto::ready(Vec::new())
                .expect_err("empty readiness evidence")
                .code(),
            GOAL_NOT_READY
        );
    }

    #[test]
    fn readiness_accessors_report_not_ready_and_ready_sets() {
        assert!(
            GoalReadinessStateDto::NotReady
                .verified_evidence_set()
                .is_empty()
        );
        assert!(!GoalReadinessStateDto::NotReady.is_ready());
        let ready = ready_state();
        assert!(ready.is_ready());
        assert_eq!(ready.verified_evidence_set(), [evidence(1, 1)].as_slice());
    }

    #[test]
    fn exception_decisions_cover_bounds_duplicates_and_accessors() {
        let over_bound: Vec<GoalGateExceptionV1> = (1..=GOAL_MAX_GATES_PER_GOAL + 1)
            .map(|index| exception(u8::try_from(index).expect("the gate seed fits a u8")))
            .collect();
        assert_eq!(
            GoalUserDecisionStateDto::accepted_with_exception(over_bound)
                .expect_err("over-bound exception set")
                .code(),
            GOAL_GATE_LIMIT_EXCEEDED
        );
        assert_eq!(
            GoalUserDecisionStateDto::accepted_with_exception(vec![exception(1), exception(1)])
                .expect_err("duplicate gate exception")
                .code(),
            GOAL_ACCEPTANCE_EXCEPTION_INVALID
        );
        assert_eq!(
            GoalUserDecisionStateDto::accepted_with_exception(vec![GoalGateExceptionV1 {
                gate_revision: 0,
                ..exception(1)
            }])
            .expect_err("inexact gate exception")
            .code(),
            GOAL_ACCEPTANCE_EXCEPTION_INVALID
        );
        assert_eq!(
            GoalUserDecisionStateDto::accepted_with_exception(Vec::new())
                .expect_err("empty exception set")
                .code(),
            GOAL_ACCEPTANCE_EXCEPTION_INVALID
        );
        let accepted_with_exception =
            GoalUserDecisionStateDto::accepted_with_exception(vec![exception(2)])
                .expect("the exception decision fixture is coherent");
        assert_eq!(
            accepted_with_exception.exception_evidence_set(),
            [exception(2)].as_slice()
        );
        assert!(accepted_with_exception.is_terminal());
        assert!(
            GoalUserDecisionStateDto::Accepted
                .exception_evidence_set()
                .is_empty()
        );
        assert!(
            GoalUserDecisionStateDto::Unaccepted
                .exception_evidence_set()
                .is_empty()
        );
        assert!(!GoalUserDecisionStateDto::Unaccepted.is_terminal());
    }

    #[test]
    fn goal_record_admits_ready_acceptance_and_exception_decisions() {
        assert_eq!(
            GoalDto::new(
                [0; 16],
                project_scope(),
                1,
                GoalLifecycleStateDto::Active,
                GoalReadinessStateDto::NotReady,
                GoalUserDecisionStateDto::Unaccepted,
            )
            .expect_err("zero goal identity")
            .code(),
            GOAL_NOT_ACTIVE
        );
        assert_eq!(
            GoalDto::new(
                [1; 16],
                project_scope(),
                0,
                GoalLifecycleStateDto::Active,
                GoalReadinessStateDto::NotReady,
                GoalUserDecisionStateDto::Unaccepted,
            )
            .expect_err("zero active revision")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            GoalDto::new(
                [1; 16],
                project_scope(),
                1,
                GoalLifecycleStateDto::NeedsRework,
                ready_state(),
                GoalUserDecisionStateDto::Unaccepted,
            )
            .expect_err("NeedsRework readiness claim")
            .code(),
            GOAL_NOT_READY
        );
        assert_eq!(
            GoalDto::new(
                [1; 16],
                project_scope(),
                1,
                GoalLifecycleStateDto::Active,
                GoalReadinessStateDto::NotReady,
                GoalUserDecisionStateDto::Accepted,
            )
            .expect_err("acceptance without readiness")
            .code(),
            GOAL_NOT_READY
        );
        assert_eq!(
            GoalDto::new(
                [1; 16],
                project_scope(),
                1,
                GoalLifecycleStateDto::Active,
                ready_state(),
                GoalUserDecisionStateDto::AcceptedWithException {
                    exception_evidence_set: vec![exception(3)],
                },
            )
            .expect_err("exception alongside readiness")
            .code(),
            GOAL_ACCEPTANCE_EXCEPTION_INVALID
        );
        let accepted = GoalDto::new(
            [1; 16],
            project_scope(),
            4,
            GoalLifecycleStateDto::Active,
            ready_state(),
            GoalUserDecisionStateDto::Accepted,
        )
        .expect("the accepted Goal fixture is coherent");
        assert_eq!(accepted.goal_id(), [1; 16]);
        assert_eq!(accepted.scope(), project_scope());
        assert_eq!(accepted.active_revision(), 4);
        assert_eq!(accepted.lifecycle_state(), GoalLifecycleStateDto::Active);
        assert_eq!(accepted.readiness_state(), &ready_state());
        assert_eq!(
            accepted.user_decision_state(),
            &GoalUserDecisionStateDto::Accepted
        );
        let exception_goal = GoalDto::new(
            [1; 16],
            session_scope(3),
            5,
            GoalLifecycleStateDto::Active,
            GoalReadinessStateDto::NotReady,
            GoalUserDecisionStateDto::AcceptedWithException {
                exception_evidence_set: vec![exception(4)],
            },
        )
        .expect("the exception Goal fixture is coherent");
        assert_eq!(
            exception_goal
                .user_decision_state()
                .exception_evidence_set(),
            [exception(4)].as_slice()
        );
        let paused = GoalDto::new(
            [1; 16],
            project_scope(),
            1,
            GoalLifecycleStateDto::Paused,
            GoalReadinessStateDto::NotReady,
            GoalUserDecisionStateDto::Unaccepted,
        )
        .expect("the paused Goal fixture is coherent");
        assert_eq!(paused.lifecycle_state(), GoalLifecycleStateDto::Paused);
        assert!(paused.readiness_state().verified_evidence_set().is_empty());
    }

    #[test]
    fn goal_revision_validation_covers_text_gates_and_rules() {
        assert!(goal_revision().validate().is_ok());
        assert_eq!(
            GoalRevisionDto {
                title: "  ".to_owned(),
                ..goal_revision()
            }
            .validate()
            .expect_err("blank title")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            GoalRevisionDto {
                objective: "api_key=secret".to_owned(),
                ..goal_revision()
            }
            .validate()
            .expect_err("credential-shaped objective")
            .code(),
            CREDENTIALS_FORBIDDEN
        );
        assert_eq!(
            GoalRevisionDto {
                goal_id: [0; 16],
                ..goal_revision()
            }
            .validate()
            .expect_err("zero Goal identity")
            .code(),
            GOAL_NOT_ACTIVE
        );
        assert_eq!(
            GoalRevisionDto {
                revision: 0,
                ..goal_revision()
            }
            .validate()
            .expect_err("zero revision number")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        let over_bound: Vec<GoalGateRevisionReferenceV1> = (1..=GOAL_MAX_GATES_PER_GOAL + 1)
            .map(|index| GoalGateRevisionReferenceV1 {
                gate_reference: [u8::try_from(index).expect("the gate seed fits a u8"); 16],
                revision: 1,
            })
            .collect();
        assert_eq!(
            GoalRevisionDto {
                required_gate_references: over_bound,
                ..goal_revision()
            }
            .validate()
            .expect_err("over-bound required gates")
            .code(),
            GOAL_GATE_LIMIT_EXCEEDED
        );
        assert_eq!(
            GoalRevisionDto {
                required_gate_references: vec![GoalGateRevisionReferenceV1 {
                    revision: 0,
                    ..GoalGateRevisionReferenceV1 {
                        gate_reference: [5; 16],
                        revision: 1,
                    }
                }],
                ..goal_revision()
            }
            .validate()
            .expect_err("inexact required gate")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            GoalRevisionDto {
                required_gate_references: vec![
                    GoalGateRevisionReferenceV1 {
                        gate_reference: [5; 16],
                        revision: 1,
                    },
                    GoalGateRevisionReferenceV1 {
                        gate_reference: [5; 16],
                        revision: 2,
                    },
                ],
                ..goal_revision()
            }
            .validate()
            .expect_err("duplicate required gate identity")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            GoalRevisionDto {
                local_rule_references: vec![[0; 16]],
                ..goal_revision()
            }
            .validate()
            .expect_err("zero local rule reference")
            .code(),
            GOAL_NOT_ACTIVE
        );
        assert_eq!(
            GoalRevisionDto {
                inherited_rule_references: vec![[0; 16]],
                local_rule_references: Vec::new(),
                ..goal_revision()
            }
            .validate()
            .expect_err("zero inherited rule reference")
            .code(),
            GOAL_NOT_ACTIVE
        );
    }

    #[test]
    fn direct_children_validation_accepts_exact_children_and_rejects_broken_links() {
        assert!(validate_goal_direct_children(&[child_link(1, 2, 1), child_link(1, 3, 1)]).is_ok());
        assert_eq!(
            validate_goal_direct_children(&[child_link(1, 0, 1)])
                .expect_err("zero child identity")
                .code(),
            GOAL_NOT_ACTIVE
        );
        assert_eq!(
            validate_goal_direct_children(&[child_link(0, 2, 1)])
                .expect_err("zero parent identity")
                .code(),
            GOAL_NOT_ACTIVE
        );
        assert_eq!(
            validate_goal_direct_children(&[child_link(1, 2, 0)])
                .expect_err("zero child revision")
                .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_goal_direct_children(&[child_link(1, 2, 1), child_link(1, 2, 2)])
                .expect_err("duplicate direct child")
                .code(),
            GOAL_CYCLE_DETECTED
        );
        let over_bound: Vec<GoalParentLinkDto> = (1..=GOAL_MAX_DIRECT_CHILDREN + 1)
            .map(|index| child_link(1, u8::try_from(index).expect("the child seed fits a u8"), 1))
            .collect();
        assert_eq!(
            validate_goal_direct_children(&over_bound)
                .expect_err("over-bound direct children")
                .code(),
            GOAL_CHILD_LIMIT_EXCEEDED
        );
    }

    #[test]
    fn child_link_rules_cover_self_revision_scope_and_session_pairs() {
        let link = child_link(1, 2, 1);
        let other_project = GoalScopeDto::Project {
            project_id: [9; 16],
        };
        assert_eq!(
            validate_goal_child_link(
                &GoalParentLinkDto {
                    child_goal_id: [1; 16],
                    ..link
                },
                project_scope(),
                project_scope(),
                false,
                false,
            )
            .expect_err("self link")
            .code(),
            GOAL_CYCLE_DETECTED
        );
        assert_eq!(
            validate_goal_child_link(
                &GoalParentLinkDto {
                    child_revision_at_link: 0,
                    ..link
                },
                project_scope(),
                project_scope(),
                false,
                false,
            )
            .expect_err("zero child revision at link time")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_goal_child_link(&link, project_scope(), other_project, false, false)
                .expect_err("cross-project child")
                .code(),
            GOAL_CYCLE_DETECTED
        );
        assert_eq!(
            validate_goal_child_link(&link, project_scope(), project_scope(), false, false)
                .expect("project child"),
            GoalChildLinkDispositionV1::ExistingSessionLink
        );
        assert_eq!(
            validate_goal_child_link(&link, project_scope(), session_scope(3), true, false)
                .expect("linked session child"),
            GoalChildLinkDispositionV1::ExistingSessionLink
        );
        assert_eq!(
            validate_goal_child_link(&link, project_scope(), session_scope(3), false, true)
                .expect("atomic session link"),
            GoalChildLinkDispositionV1::CreateSessionLinkAtomically
        );
        assert_eq!(
            validate_goal_child_link(&link, project_scope(), session_scope(3), false, false)
                .expect_err("missing durable session link")
                .code(),
            GOAL_CYCLE_DETECTED
        );
        assert_eq!(
            validate_goal_child_link(&link, session_scope(3), session_scope(3), false, false)
                .expect("same-session child"),
            GoalChildLinkDispositionV1::ExistingSessionLink
        );
        assert_eq!(
            validate_goal_child_link(&link, session_scope(3), session_scope(9), false, false)
                .expect_err("foreign session child")
                .code(),
            GOAL_CYCLE_DETECTED
        );
        assert_eq!(
            validate_goal_child_link(&link, session_scope(3), project_scope(), false, false)
                .expect_err("session parent with project child")
                .code(),
            GOAL_CYCLE_DETECTED
        );
    }

    #[test]
    fn readiness_claim_requires_evidence_free_children_and_gate_evidence() {
        assert_eq!(
            validate_goal_readiness_claim(Vec::new(), &[], false)
                .expect_err("empty evidence set")
                .code(),
            GOAL_NOT_READY
        );
        assert_eq!(
            validate_goal_readiness_claim(vec![evidence(1, 1)], &[[9; 16]], false)
                .expect_err("unresolved obligatory child")
                .code(),
            GOAL_NOT_READY
        );
        assert_eq!(
            validate_goal_readiness_claim(vec![evidence(1, 1)], &[], true)
                .expect_err("missing required gate evidence")
                .code(),
            GOAL_NOT_READY
        );
        assert_eq!(
            validate_goal_readiness_claim(vec![evidence(1, 1)], &[], false)
                .expect("complete readiness claim"),
            ready_state()
        );
    }

    #[test]
    fn acceptance_validation_requires_readiness_and_includes_inherited_exceptions() {
        assert_eq!(
            validate_goal_acceptance(&GoalAcceptanceRequestV1 {
                decision: GoalAcceptanceDecisionV1::Accept,
                readiness_state: GoalReadinessStateDto::NotReady,
                inherited_child_exceptions: Vec::new(),
                unresolved_obligatory_children: vec![[9; 16]],
            })
            .expect_err("unresolved obligatory child")
            .code(),
            GOAL_NOT_READY
        );
        assert_eq!(
            validate_goal_acceptance(&GoalAcceptanceRequestV1 {
                decision: GoalAcceptanceDecisionV1::Accept,
                readiness_state: GoalReadinessStateDto::NotReady,
                inherited_child_exceptions: vec![GoalInheritedExceptionV1 {
                    child_goal_id: [9; 16],
                    exception: exception(10),
                }],
                unresolved_obligatory_children: Vec::new(),
            })
            .expect_err("plain acceptance with an inherited exception")
            .code(),
            GOAL_ACCEPTANCE_EXCEPTION_INVALID
        );
        assert_eq!(
            validate_goal_acceptance(&GoalAcceptanceRequestV1 {
                decision: GoalAcceptanceDecisionV1::Accept,
                readiness_state: GoalReadinessStateDto::NotReady,
                inherited_child_exceptions: Vec::new(),
                unresolved_obligatory_children: Vec::new(),
            })
            .expect_err("acceptance without readiness")
            .code(),
            GOAL_NOT_READY
        );
        assert_eq!(
            validate_goal_acceptance(&GoalAcceptanceRequestV1 {
                decision: GoalAcceptanceDecisionV1::Accept,
                readiness_state: ready_state(),
                inherited_child_exceptions: Vec::new(),
                unresolved_obligatory_children: Vec::new(),
            })
            .expect("plain acceptance"),
            GoalUserDecisionStateDto::Accepted
        );
        assert_eq!(
            validate_goal_acceptance(&GoalAcceptanceRequestV1 {
                decision: GoalAcceptanceDecisionV1::AcceptWithException {
                    exception_evidence_set: vec![exception(4)],
                },
                readiness_state: ready_state(),
                inherited_child_exceptions: Vec::new(),
                unresolved_obligatory_children: Vec::new(),
            })
            .expect_err("exception against a ready Goal")
            .code(),
            GOAL_ACCEPTANCE_EXCEPTION_INVALID
        );
        assert_eq!(
            validate_goal_acceptance(&GoalAcceptanceRequestV1 {
                decision: GoalAcceptanceDecisionV1::AcceptWithException {
                    exception_evidence_set: vec![exception(4)],
                },
                readiness_state: GoalReadinessStateDto::NotReady,
                inherited_child_exceptions: Vec::new(),
                unresolved_obligatory_children: vec![[9; 16]],
            })
            .expect_err("exception bypassing an obligatory child")
            .code(),
            GOAL_ACCEPTANCE_EXCEPTION_INVALID
        );
        assert_eq!(
            validate_goal_acceptance(&GoalAcceptanceRequestV1 {
                decision: GoalAcceptanceDecisionV1::AcceptWithException {
                    exception_evidence_set: vec![exception(5)],
                },
                readiness_state: GoalReadinessStateDto::NotReady,
                inherited_child_exceptions: vec![GoalInheritedExceptionV1 {
                    child_goal_id: [9; 16],
                    exception: exception(4),
                }],
                unresolved_obligatory_children: Vec::new(),
            })
            .expect_err("omitted inherited child exception")
            .code(),
            GOAL_ACCEPTANCE_EXCEPTION_INVALID
        );
        assert_eq!(
            validate_goal_acceptance(&GoalAcceptanceRequestV1 {
                decision: GoalAcceptanceDecisionV1::AcceptWithException {
                    exception_evidence_set: vec![exception(4)],
                },
                readiness_state: GoalReadinessStateDto::NotReady,
                inherited_child_exceptions: vec![GoalInheritedExceptionV1 {
                    child_goal_id: [0; 16],
                    exception: exception(4),
                }],
                unresolved_obligatory_children: Vec::new(),
            })
            .expect_err("zero inherited child identity")
            .code(),
            GOAL_ACCEPTANCE_EXCEPTION_INVALID
        );
        assert_eq!(
            validate_goal_acceptance(&GoalAcceptanceRequestV1 {
                decision: GoalAcceptanceDecisionV1::AcceptWithException {
                    exception_evidence_set: vec![exception(4)],
                },
                readiness_state: GoalReadinessStateDto::NotReady,
                inherited_child_exceptions: vec![GoalInheritedExceptionV1 {
                    child_goal_id: [9; 16],
                    exception: exception(4),
                }],
                unresolved_obligatory_children: Vec::new(),
            })
            .expect("included inherited child exception"),
            GoalUserDecisionStateDto::AcceptedWithException {
                exception_evidence_set: vec![exception(4)],
            }
        );
    }

    #[test]
    fn gate_definitions_and_revisions_enforce_identity_kind_and_revision_rules() {
        let reference = reference_gate(vec![GoalEvidenceKindV1::TerminalChildResult]);
        assert!(reference.validate().is_ok());
        assert!(!reference.is_executable());
        assert_eq!(
            reference_gate(Vec::new())
                .validate()
                .expect_err("empty accepted kinds")
                .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            reference_gate(vec![
                GoalEvidenceKindV1::TerminalChildResult,
                GoalEvidenceKindV1::TerminalChildResult,
            ])
            .validate()
            .expect_err("duplicate accepted kinds")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            VerificationGateDto::ReferenceGate {
                evidence_contract_revision: 0,
                accepted_reference_kinds: vec![GoalEvidenceKindV1::AcceptedUserDeclaration],
            }
            .validate()
            .expect_err("zero evidence contract revision")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert!(executable_gate().validate().is_ok());
        assert!(executable_gate().is_executable());
        assert_eq!(
            VerificationGateDto::ExecutableGate {
                template_id: [0; 16],
                template_revision: 1,
            }
            .validate()
            .expect_err("zero template identity")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            VerificationGateDto::ExecutableGate {
                template_id: [6; 16],
                template_revision: 0,
            }
            .validate()
            .expect_err("zero template revision")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert!(
            validate_goal_gate_definitions(&[GoalGateDefinitionV1 {
                gate_id: [5; 16],
                gate: executable_gate(),
            }])
            .is_ok()
        );
        assert_eq!(
            validate_goal_gate_definitions(&[GoalGateDefinitionV1 {
                gate_id: [0; 16],
                gate: executable_gate(),
            }])
            .expect_err("zero gate definition identity")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            validate_goal_gate_definitions(&[
                GoalGateDefinitionV1 {
                    gate_id: [5; 16],
                    gate: executable_gate(),
                },
                GoalGateDefinitionV1 {
                    gate_id: [5; 16],
                    gate: executable_gate(),
                },
            ])
            .expect_err("duplicate gate definition identity")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        let over_bound: Vec<GoalGateDefinitionV1> = (1..=GOAL_MAX_GATES_PER_GOAL + 1)
            .map(|index| GoalGateDefinitionV1 {
                gate_id: [u8::try_from(index).expect("the gate seed fits a u8"); 16],
                gate: reference_gate(vec![GoalEvidenceKindV1::TerminalChildResult]),
            })
            .collect();
        assert_eq!(
            validate_goal_gate_definitions(&over_bound)
                .expect_err("over-bound gate definitions")
                .code(),
            GOAL_GATE_LIMIT_EXCEEDED
        );
        assert!(
            validate_goal_gate_revisions(&[GoalGateRevisionReferenceV1 {
                gate_reference: [5; 16],
                revision: 1,
            }])
            .is_ok()
        );
        assert_eq!(
            validate_goal_gate_revisions(&[GoalGateRevisionReferenceV1 {
                gate_reference: [0; 16],
                revision: 1,
            }])
            .expect_err("zero selected gate reference")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_goal_gate_revisions(&[
                GoalGateRevisionReferenceV1 {
                    gate_reference: [5; 16],
                    revision: 1,
                },
                GoalGateRevisionReferenceV1 {
                    gate_reference: [5; 16],
                    revision: 2,
                },
            ])
            .expect_err("duplicate selected gate identity")
            .code(),
            GOAL_REVISION_CONFLICT
        );
    }

    #[test]
    fn gate_template_cards_cover_every_scope_and_missing_identities() {
        let project_template = GoalTemplateScopeDto::Project {
            project_id: [2; 16],
        };
        assert!(template_card(project_template).validate().is_ok());
        assert!(
            template_card(GoalTemplateScopeDto::Goal { goal_id: [1; 16] })
                .validate()
                .is_ok()
        );
        assert!(
            template_card(GoalTemplateScopeDto::Session {
                session_id: [3; 16]
            })
            .validate()
            .is_ok()
        );
        assert_eq!(
            GoalGateTemplateCardV1 {
                template_id: [0; 16],
                ..template_card(project_template)
            }
            .validate()
            .expect_err("zero template identity")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            GoalGateTemplateCardV1 {
                capability_reference: [0; 16],
                ..template_card(project_template)
            }
            .validate()
            .expect_err("zero capability reference")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            GoalGateTemplateCardV1 {
                revision: 0,
                ..template_card(project_template)
            }
            .validate()
            .expect_err("zero template revision")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            template_card(GoalTemplateScopeDto::Project {
                project_id: [0; 16]
            })
            .validate()
            .expect_err("zero project scope identity")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            template_card(GoalTemplateScopeDto::Goal { goal_id: [0; 16] })
                .validate()
                .expect_err("zero Goal scope identity")
                .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            template_card(GoalTemplateScopeDto::Session {
                session_id: [0; 16]
            })
            .validate()
            .expect_err("zero session scope identity")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
    }

    #[test]
    fn gate_template_creation_scope_lifecycle_and_reference_rules() {
        let card = template_card(GoalTemplateScopeDto::Project {
            project_id: [2; 16],
        });
        assert!(validate_gate_template_creation(&card, &GoalTemplateProvenanceV1::User).is_ok());
        assert!(
            validate_gate_template_creation(
                &card,
                &GoalTemplateProvenanceV1::ModelProposal {
                    draft_id: [14; 16],
                    accepted_by_user: true,
                },
            )
            .is_ok()
        );
        assert_eq!(
            validate_gate_template_creation(
                &card,
                &GoalTemplateProvenanceV1::ModelProposal {
                    draft_id: [0; 16],
                    accepted_by_user: true,
                },
            )
            .expect_err("zero proposal draft identity")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            validate_gate_template_creation(
                &card,
                &GoalTemplateProvenanceV1::ModelProposal {
                    draft_id: [14; 16],
                    accepted_by_user: false,
                },
            )
            .expect_err("unconfirmed model proposal")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        let context = applicability_context();
        assert!(
            validate_gate_template_scope(
                &GoalTemplateScopeDto::Project {
                    project_id: [2; 16]
                },
                &context
            )
            .is_ok()
        );
        assert!(
            validate_gate_template_scope(
                &GoalTemplateScopeDto::Goal { goal_id: [1; 16] },
                &context
            )
            .is_ok()
        );
        assert!(
            validate_gate_template_scope(
                &GoalTemplateScopeDto::Session {
                    session_id: [3; 16]
                },
                &context
            )
            .is_ok()
        );
        assert_eq!(
            validate_gate_template_scope(
                &GoalTemplateScopeDto::Goal { goal_id: [9; 16] },
                &context
            )
            .expect_err("unselected Goal scope")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            validate_gate_template_scope(
                &GoalTemplateScopeDto::Project {
                    project_id: [9; 16]
                },
                &context
            )
            .expect_err("foreign project scope")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            validate_gate_template_scope(
                &GoalTemplateScopeDto::Session {
                    session_id: [9; 16]
                },
                &context
            )
            .expect_err("foreign session scope")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert!(
            validate_gate_template_lifecycle_transition(
                None,
                GoalTemplateLifecycleStateDto::Enabled
            )
            .is_ok()
        );
        assert!(
            validate_gate_template_lifecycle_transition(
                Some(GoalTemplateLifecycleStateDto::Enabled),
                GoalTemplateLifecycleStateDto::Archived
            )
            .is_ok()
        );
        assert!(
            validate_gate_template_lifecycle_transition(
                Some(GoalTemplateLifecycleStateDto::Archived),
                GoalTemplateLifecycleStateDto::Enabled
            )
            .is_ok()
        );
        assert_eq!(
            validate_gate_template_lifecycle_transition(
                None,
                GoalTemplateLifecycleStateDto::Archived
            )
            .expect_err("undeclared create edge")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            validate_gate_template_lifecycle_transition(
                Some(GoalTemplateLifecycleStateDto::Enabled),
                GoalTemplateLifecycleStateDto::Enabled
            )
            .expect_err("undeclared enable edge")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        let reference = GoalGateRevisionReferenceV1 {
            gate_reference: [7; 16],
            revision: 2,
        };
        assert!(validate_gate_template_reference(&reference, Some(&card)).is_ok());
        assert_eq!(
            validate_gate_template_reference(&reference, None)
                .expect_err("unavailable template")
                .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert_eq!(
            validate_gate_template_reference(
                &GoalGateRevisionReferenceV1 {
                    gate_reference: [7; 16],
                    revision: 3,
                },
                Some(&card),
            )
            .expect_err("stale template revision")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
        assert!(
            validate_gate_execution_preconditions(
                &reference_gate(vec![GoalEvidenceKindV1::TerminalChildResult]),
                None
            )
            .is_ok()
        );
        assert_eq!(
            validate_gate_execution_preconditions(&executable_gate(), None)
                .expect_err("missing executable template")
                .code(),
            GOAL_GATE_UNAVAILABLE
        );
        let executable_card = GoalGateTemplateCardV1 {
            template_id: [6; 16],
            ..card
        };
        assert!(
            validate_gate_execution_preconditions(&executable_gate(), Some(&executable_card))
                .is_ok()
        );
        assert_eq!(
            validate_gate_execution_preconditions(
                &executable_gate(),
                Some(&GoalGateTemplateCardV1 {
                    revision: 3,
                    ..executable_card
                }),
            )
            .expect_err("stale executable template revision")
            .code(),
            GOAL_GATE_UNAVAILABLE
        );
    }

    #[test]
    fn reference_gate_evidence_requires_a_reference_gate_and_accepted_kind() {
        let gate = reference_gate(vec![GoalEvidenceKindV1::TerminalChildResult]);
        assert!(
            validate_reference_gate_evidence(
                &gate,
                &GoalEvidenceReferenceV1::new([1; 16], 1, GoalEvidenceKindV1::TerminalChildResult),
            )
            .is_ok()
        );
        assert_eq!(
            validate_reference_gate_evidence(
                &gate,
                &GoalEvidenceReferenceV1::new([1; 16], 1, GoalEvidenceKindV1::ExecutableGateResult),
            )
            .expect_err("unaccepted evidence kind")
            .code(),
            GOAL_GATE_FAILED
        );
        assert_eq!(
            validate_reference_gate_evidence(
                &executable_gate(),
                &GoalEvidenceReferenceV1::new([1; 16], 1, GoalEvidenceKindV1::TerminalChildResult),
            )
            .expect_err("executable gate validating a durable reference")
            .code(),
            GOAL_GATE_FAILED
        );
    }

    #[test]
    fn gate_outcomes_map_to_closed_dispositions_and_exact_evidence() {
        let cases = [
            (
                GoalGateOutcomeV1::Passed {
                    evidence: evidence(1, 1),
                },
                GoalGateOutcomeDispositionV1::Passed,
            ),
            (
                GoalGateOutcomeV1::Failed {
                    evidence: evidence(1, 1),
                },
                GoalGateOutcomeDispositionV1::Failed,
            ),
            (
                GoalGateOutcomeV1::ExternalEffectUnknown {
                    evidence: evidence(1, 1),
                },
                GoalGateOutcomeDispositionV1::UnknownEffect,
            ),
            (
                GoalGateOutcomeV1::TimedOut,
                GoalGateOutcomeDispositionV1::Failed,
            ),
            (
                GoalGateOutcomeV1::Cancelled,
                GoalGateOutcomeDispositionV1::Failed,
            ),
            (
                GoalGateOutcomeV1::OutputBoundExceeded,
                GoalGateOutcomeDispositionV1::Failed,
            ),
            (
                GoalGateOutcomeV1::ReferenceUnavailable,
                GoalGateOutcomeDispositionV1::Unavailable,
            ),
            (
                GoalGateOutcomeV1::RevisionStale,
                GoalGateOutcomeDispositionV1::Unavailable,
            ),
            (
                GoalGateOutcomeV1::TemplateUnavailable,
                GoalGateOutcomeDispositionV1::Unavailable,
            ),
        ];
        for (outcome, disposition) in cases {
            assert_eq!(
                validate_gate_outcome(&outcome).expect("closed gate outcome"),
                disposition
            );
            assert_eq!(outcome.disposition(), disposition);
        }
        assert!(
            GoalGateOutcomeV1::Passed {
                evidence: evidence(1, 1)
            }
            .is_success()
        );
        assert!(!GoalGateOutcomeV1::TemplateUnavailable.is_success());
        for outcome in [
            GoalGateOutcomeV1::Passed {
                evidence: evidence(0, 1),
            },
            GoalGateOutcomeV1::Failed {
                evidence: evidence(1, 0),
            },
            GoalGateOutcomeV1::ExternalEffectUnknown {
                evidence: evidence(0, 0),
            },
        ] {
            assert_eq!(
                validate_gate_outcome(&outcome)
                    .expect_err("inexact gate outcome evidence")
                    .code(),
                GOAL_GATE_FAILED
            );
        }
        assert_eq!(
            apply_required_gate_outcome(
                GoalLifecycleStateDto::Active,
                GoalGateOutcomeDispositionV1::Failed,
            )
            .expect("failed required gate"),
            GoalLifecycleStateDto::NeedsRework
        );
        assert_eq!(
            apply_required_gate_outcome(
                GoalLifecycleStateDto::NeedsRework,
                GoalGateOutcomeDispositionV1::UnknownEffect,
            )
            .expect("unknown external effect"),
            GoalLifecycleStateDto::NeedsRework
        );
        assert_eq!(
            apply_required_gate_outcome(
                GoalLifecycleStateDto::Active,
                GoalGateOutcomeDispositionV1::Passed,
            )
            .expect("passed required gate"),
            GoalLifecycleStateDto::Active
        );
        assert_eq!(
            apply_required_gate_outcome(
                GoalLifecycleStateDto::Active,
                GoalGateOutcomeDispositionV1::Unavailable,
            )
            .expect("unavailable required gate"),
            GoalLifecycleStateDto::Active
        );
        assert_eq!(
            apply_required_gate_outcome(
                GoalLifecycleStateDto::Paused,
                GoalGateOutcomeDispositionV1::Passed,
            )
            .expect_err("paused Goal evaluating a required gate")
            .code(),
            GOAL_NOT_ACTIVE
        );
    }

    #[test]
    fn memory_cards_cover_identity_and_text_validation() {
        let scope = GoalRecordScopeDto::Project {
            project_id: [2; 16],
        };
        assert!(memory_card(scope).validate().is_ok());
        assert_eq!(
            GoalMemoryCardV1 {
                record_id: [0; 16],
                ..memory_card(scope)
            }
            .validate()
            .expect_err("zero record identity")
            .code(),
            MEMORY_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            GoalMemoryCardV1 {
                revision: 0,
                ..memory_card(scope)
            }
            .validate()
            .expect_err("zero record revision")
            .code(),
            MEMORY_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            GoalMemoryCardV1 {
                retained_content_reference: [0; 16],
                ..memory_card(scope)
            }
            .validate()
            .expect_err("zero retained-content reference")
            .code(),
            MEMORY_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            GoalMemoryCardV1 {
                title: "  ".to_owned(),
                ..memory_card(scope)
            }
            .validate()
            .expect_err("blank title")
            .code(),
            MEMORY_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            GoalMemoryCardV1 {
                safe_purpose: "password=hunter2".to_owned(),
                ..memory_card(scope)
            }
            .validate()
            .expect_err("credential-shaped purpose")
            .code(),
            CREDENTIALS_FORBIDDEN
        );
        assert_eq!(
            GoalMemoryCardV1 {
                title: "a".repeat(record_bound_bytes() + 1),
                ..memory_card(scope)
            }
            .validate()
            .expect_err("over-bound title")
            .code(),
            MEMORY_ENTRY_TOO_LARGE
        );
        assert_eq!(
            GoalMemoryCardV1 {
                title: "a".repeat(300_000),
                safe_purpose: "b".repeat(300_000),
                ..memory_card(scope)
            }
            .validate()
            .expect_err("over-bound memory card")
            .code(),
            MEMORY_ENTRY_TOO_LARGE
        );
    }

    #[test]
    fn memory_cards_cover_applicability_disclosure_and_relations() {
        let context = applicability_context();
        let project_card = memory_card(GoalRecordScopeDto::Project {
            project_id: [2; 16],
        });
        assert!(validate_memory_card_applicability(&project_card, &context).is_ok());
        assert!(
            validate_memory_card_applicability(
                &memory_card(GoalRecordScopeDto::Goal { goal_id: [1; 16] }),
                &context
            )
            .is_ok()
        );
        assert!(
            validate_memory_card_applicability(
                &memory_card(GoalRecordScopeDto::Session {
                    session_id: [3; 16]
                }),
                &context
            )
            .is_ok()
        );
        assert_eq!(
            validate_memory_card_applicability(
                &memory_card(GoalRecordScopeDto::Project {
                    project_id: [9; 16]
                }),
                &context
            )
            .expect_err("foreign project memory record")
            .code(),
            MEMORY_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            validate_memory_card_applicability(
                &memory_card(GoalRecordScopeDto::Goal { goal_id: [9; 16] }),
                &context
            )
            .expect_err("unselected Goal memory record")
            .code(),
            MEMORY_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            validate_memory_card_applicability(
                &memory_card(GoalRecordScopeDto::Session {
                    session_id: [9; 16]
                }),
                &context
            )
            .expect_err("foreign session memory record")
            .code(),
            MEMORY_REFERENCE_UNAVAILABLE
        );
        assert!(
            validate_memory_card_set(
                &[memory_card(GoalRecordScopeDto::Project {
                    project_id: [2; 16]
                })],
                &context
            )
            .is_ok()
        );
        assert_eq!(
            validate_memory_card_set(
                &[
                    memory_card(GoalRecordScopeDto::Project {
                        project_id: [2; 16]
                    }),
                    memory_card(GoalRecordScopeDto::Project {
                        project_id: [2; 16]
                    }),
                ],
                &context,
            )
            .expect_err("duplicate memory record identity")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        let over_bound: Vec<GoalMemoryCardV1> = (1..=GOAL_MAX_ACTIVE_MEMORY_CARDS + 1)
            .map(|index| GoalMemoryCardV1 {
                record_id: [u8::try_from(index).expect("the record seed fits a u8"); 16],
                ..memory_card(GoalRecordScopeDto::Project {
                    project_id: [2; 16],
                })
            })
            .collect();
        assert_eq!(
            validate_memory_card_set(&over_bound, &context)
                .expect_err("over-bound memory card set")
                .code(),
            MEMORY_ENTRY_LIMIT_EXCEEDED
        );
        assert!(validate_memory_disclosure(&project_card, [12; 16]).is_ok());
        assert_eq!(
            validate_memory_disclosure(&project_card, [0; 16])
                .expect_err("zero disclosure reference")
                .code(),
            MEMORY_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            validate_memory_disclosure(&project_card, [19; 16])
                .expect_err("mismatched disclosure reference")
                .code(),
            MEMORY_REFERENCE_UNAVAILABLE
        );
        assert!(
            validate_memory_replacement(&GoalMemoryReplacementLinkV1 {
                replaced_record_id: [11; 16],
                replaced_revision: 1,
                replacement_record_id: [11; 16],
                replacement_revision: 2,
            })
            .is_ok()
        );
        assert!(
            validate_memory_replacement(&GoalMemoryReplacementLinkV1 {
                replaced_record_id: [11; 16],
                replaced_revision: 1,
                replacement_record_id: [15; 16],
                replacement_revision: 1,
            })
            .is_ok()
        );
        assert_eq!(
            validate_memory_replacement(&GoalMemoryReplacementLinkV1 {
                replaced_record_id: [11; 16],
                replaced_revision: 2,
                replacement_record_id: [11; 16],
                replacement_revision: 2,
            })
            .expect_err("non-advancing same-record replacement")
            .code(),
            MEMORY_REPLACEMENT_CONFLICT
        );
        for link in [
            GoalMemoryReplacementLinkV1 {
                replaced_record_id: [0; 16],
                replaced_revision: 1,
                replacement_record_id: [15; 16],
                replacement_revision: 1,
            },
            GoalMemoryReplacementLinkV1 {
                replaced_record_id: [11; 16],
                replaced_revision: 0,
                replacement_record_id: [15; 16],
                replacement_revision: 1,
            },
            GoalMemoryReplacementLinkV1 {
                replaced_record_id: [11; 16],
                replaced_revision: 1,
                replacement_record_id: [0; 16],
                replacement_revision: 1,
            },
            GoalMemoryReplacementLinkV1 {
                replaced_record_id: [11; 16],
                replaced_revision: 1,
                replacement_record_id: [15; 16],
                replacement_revision: 0,
            },
        ] {
            assert_eq!(
                validate_memory_replacement(&link)
                    .expect_err("inexact replacement link")
                    .code(),
                MEMORY_REPLACEMENT_CONFLICT
            );
        }
        assert!(
            validate_memory_rollback(&GoalMemoryRollbackLinkV1 {
                current_record_id: [11; 16],
                current_revision: 3,
                restored_record_id: [11; 16],
                restored_revision: 1,
            })
            .is_ok()
        );
        assert!(
            validate_memory_rollback(&GoalMemoryRollbackLinkV1 {
                current_record_id: [11; 16],
                current_revision: 3,
                restored_record_id: [15; 16],
                restored_revision: 9,
            })
            .is_ok()
        );
        assert_eq!(
            validate_memory_rollback(&GoalMemoryRollbackLinkV1 {
                current_record_id: [11; 16],
                current_revision: 3,
                restored_record_id: [11; 16],
                restored_revision: 3,
            })
            .expect_err("same-record rollback of a non-earlier revision")
            .code(),
            MEMORY_REPLACEMENT_CONFLICT
        );
        for link in [
            GoalMemoryRollbackLinkV1 {
                current_record_id: [0; 16],
                current_revision: 3,
                restored_record_id: [11; 16],
                restored_revision: 1,
            },
            GoalMemoryRollbackLinkV1 {
                current_record_id: [11; 16],
                current_revision: 0,
                restored_record_id: [11; 16],
                restored_revision: 1,
            },
            GoalMemoryRollbackLinkV1 {
                current_record_id: [11; 16],
                current_revision: 3,
                restored_record_id: [0; 16],
                restored_revision: 1,
            },
            GoalMemoryRollbackLinkV1 {
                current_record_id: [11; 16],
                current_revision: 3,
                restored_record_id: [11; 16],
                restored_revision: 0,
            },
        ] {
            assert_eq!(
                validate_memory_rollback(&link)
                    .expect_err("inexact rollback link")
                    .code(),
                MEMORY_REPLACEMENT_CONFLICT
            );
        }
    }

    #[test]
    fn skill_names_content_size_and_card_identity_are_validated() {
        assert!(validate_goal_skill_name("read-file-1").is_ok());
        assert!(validate_goal_skill_name("1-tool").is_ok());
        assert_eq!(
            validate_goal_skill_name("Read-File")
                .expect_err("uppercase Skill name")
                .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            validate_goal_skill_name("read-")
                .expect_err("trailing hyphen")
                .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            validate_goal_skill_name("  ")
                .expect_err("blank Skill name")
                .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            validate_goal_skill_name("api_key=secret")
                .expect_err("credential-shaped Skill name")
                .code(),
            CREDENTIALS_FORBIDDEN
        );
        assert!(validate_skill_content_size(GOAL_MAX_FULL_RECORD_BYTES).is_ok());
        assert_eq!(
            validate_skill_content_size(GOAL_MAX_FULL_RECORD_BYTES + 1)
                .expect_err("over-bound Skill content")
                .code(),
            SKILL_ENTRY_TOO_LARGE
        );
        let scope = GoalRecordScopeDto::Project {
            project_id: [2; 16],
        };
        assert!(skill_card(scope).validate().is_ok());
        assert_eq!(
            GoalSkillCardV1 {
                skill_id: [0; 16],
                ..skill_card(scope)
            }
            .validate()
            .expect_err("zero Skill identity")
            .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            GoalSkillCardV1 {
                revision: 0,
                ..skill_card(scope)
            }
            .validate()
            .expect_err("zero Skill revision")
            .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            GoalSkillCardV1 {
                content_reference: [0; 16],
                ..skill_card(scope)
            }
            .validate()
            .expect_err("zero content reference")
            .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            GoalSkillCardV1 {
                canonical_name: "Read-File".to_owned(),
                ..skill_card(scope)
            }
            .validate()
            .expect_err("non-canonical Skill name")
            .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            GoalSkillCardV1 {
                description: "  ".to_owned(),
                ..skill_card(scope)
            }
            .validate()
            .expect_err("blank description")
            .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            GoalSkillCardV1 {
                description: "token=abc".to_owned(),
                ..skill_card(scope)
            }
            .validate()
            .expect_err("credential-shaped description")
            .code(),
            CREDENTIALS_FORBIDDEN
        );
    }

    #[test]
    fn skill_cards_cover_scope_applicability_and_disclosure() {
        let context = applicability_context();
        assert!(
            skill_card(GoalRecordScopeDto::Goal { goal_id: [1; 16] })
                .validate()
                .is_ok()
        );
        assert!(
            skill_card(GoalRecordScopeDto::Session {
                session_id: [3; 16]
            })
            .validate()
            .is_ok()
        );
        assert_eq!(
            GoalSkillCardV1 {
                owner_scope: GoalRecordScopeDto::Goal { goal_id: [0; 16] },
                ..skill_card(GoalRecordScopeDto::Project {
                    project_id: [2; 16]
                })
            }
            .validate()
            .expect_err("zero Goal scope identity")
            .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            GoalSkillCardV1 {
                owner_scope: GoalRecordScopeDto::Session {
                    session_id: [0; 16]
                },
                ..skill_card(GoalRecordScopeDto::Project {
                    project_id: [2; 16]
                })
            }
            .validate()
            .expect_err("zero session scope identity")
            .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            GoalSkillCardV1 {
                owner_scope: GoalRecordScopeDto::Project {
                    project_id: [0; 16]
                },
                ..skill_card(GoalRecordScopeDto::Project {
                    project_id: [2; 16]
                })
            }
            .validate()
            .expect_err("zero project scope identity")
            .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert!(
            validate_skill_card_applicability(
                &skill_card(GoalRecordScopeDto::Goal { goal_id: [1; 16] }),
                &context
            )
            .is_ok()
        );
        assert!(
            validate_skill_card_applicability(
                &skill_card(GoalRecordScopeDto::Project {
                    project_id: [2; 16]
                }),
                &context
            )
            .is_ok()
        );
        assert!(
            validate_skill_card_applicability(
                &skill_card(GoalRecordScopeDto::Session {
                    session_id: [3; 16]
                }),
                &context
            )
            .is_ok()
        );
        assert_eq!(
            validate_skill_card_applicability(
                &skill_card(GoalRecordScopeDto::Goal { goal_id: [9; 16] }),
                &context
            )
            .expect_err("unselected Goal Skill card")
            .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            validate_skill_card_applicability(
                &skill_card(GoalRecordScopeDto::Project {
                    project_id: [9; 16]
                }),
                &context
            )
            .expect_err("foreign project Skill card")
            .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            validate_skill_card_applicability(
                &skill_card(GoalRecordScopeDto::Session {
                    session_id: [9; 16]
                }),
                &context
            )
            .expect_err("foreign session Skill card")
            .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        let scope = GoalRecordScopeDto::Project {
            project_id: [2; 16],
        };
        assert!(validate_skill_disclosure(&skill_card(scope), [22; 16]).is_ok());
        assert_eq!(
            validate_skill_disclosure(&skill_card(scope), [0; 16])
                .expect_err("zero Skill disclosure reference")
                .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
        assert_eq!(
            validate_skill_disclosure(&skill_card(scope), [24; 16])
                .expect_err("mismatched Skill disclosure reference")
                .code(),
            SKILL_REFERENCE_UNAVAILABLE
        );
    }

    #[test]
    fn skill_and_role_sets_reject_duplicates_and_over_bound_selection() {
        let context = applicability_context();
        let skill_scope = GoalRecordScopeDto::Goal { goal_id: [1; 16] };
        assert!(
            validate_skill_role_card_set(&[skill_card(skill_scope)], &[role_card()], &context)
                .is_ok()
        );
        assert_eq!(
            validate_skill_role_card_set(
                &[skill_card(skill_scope), skill_card(skill_scope)],
                &[],
                &context,
            )
            .expect_err("duplicate selected Skill identity")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_skill_role_card_set(
                &[],
                &[GoalRoleCardV1 {
                    revision: 0,
                    ..role_card()
                }],
                &context,
            )
            .expect_err("invalid selected role")
            .code(),
            DELEGATION_ROLE_INVALID
        );
        assert_eq!(
            validate_skill_role_card_set(&[], &[role_card(), role_card()], &context)
                .expect_err("duplicate selected role identity")
                .code(),
            GOAL_REVISION_CONFLICT
        );
        let over_bound: Vec<GoalSkillCardV1> = (1..=GOAL_MAX_SELECTED_SKILL_ROLE_CARDS + 1)
            .map(|index| GoalSkillCardV1 {
                skill_id: [u8::try_from(index).expect("the Skill seed fits a u8"); 16],
                ..skill_card(skill_scope)
            })
            .collect();
        assert_eq!(
            validate_skill_role_card_set(&over_bound, &[], &context)
                .expect_err("over-bound selected Skill and role set")
                .code(),
            GOAL_LIMIT_EXCEEDED
        );
    }

    #[test]
    fn role_cards_cover_class_rank_identity_limits_and_tool_uniqueness() {
        assert_eq!(GoalRoleClassV1::Light.rank(), 0);
        assert_eq!(GoalRoleClassV1::Medium.rank(), 1);
        assert_eq!(GoalRoleClassV1::Heavy.rank(), 2);
        assert!(role_card().validate().is_ok());
        assert_eq!(
            GoalRoleCardV1 {
                role_id: [0; 16],
                ..role_card()
            }
            .validate()
            .expect_err("zero role identity")
            .code(),
            DELEGATION_ROLE_INVALID
        );
        assert_eq!(
            GoalRoleCardV1 {
                revision: 0,
                ..role_card()
            }
            .validate()
            .expect_err("zero role revision")
            .code(),
            DELEGATION_ROLE_INVALID
        );
        assert_eq!(
            GoalRoleCardV1 {
                canonical_name: "  ".to_owned(),
                ..role_card()
            }
            .validate()
            .expect_err("blank role name")
            .code(),
            DELEGATION_ROLE_INVALID
        );
        assert_eq!(
            GoalRoleCardV1 {
                task: "api_key=secret".to_owned(),
                ..role_card()
            }
            .validate()
            .expect_err("credential-shaped role task")
            .code(),
            CREDENTIALS_FORBIDDEN
        );
        assert_eq!(
            GoalRoleCardV1 {
                context_limit_bytes: 0,
                ..role_card()
            }
            .validate()
            .expect_err("zero context limit")
            .code(),
            DELEGATION_ROLE_INVALID
        );
        assert_eq!(
            GoalRoleCardV1 {
                result_limit_bytes: 0,
                ..role_card()
            }
            .validate()
            .expect_err("zero result limit")
            .code(),
            DELEGATION_ROLE_INVALID
        );
        assert_eq!(
            GoalRoleCardV1 {
                tool_subset: vec!["read".to_owned(), "read".to_owned()],
                ..role_card()
            }
            .validate()
            .expect_err("duplicate role tool")
            .code(),
            DELEGATION_ROLE_INVALID
        );
        assert_eq!(
            GoalRoleCardV1 {
                tool_subset: vec!["  ".to_owned()],
                ..role_card()
            }
            .validate()
            .expect_err("blank role tool")
            .code(),
            DELEGATION_ROLE_INVALID
        );
    }

    #[test]
    fn role_narrowing_rejects_class_tool_and_limit_widening() {
        let base = role_card();
        let concrete = GoalRoleCardV1 {
            role_id: [32; 16],
            ..role_card()
        };
        assert!(validate_role_narrowing(Some(&base), &concrete).is_ok());
        assert!(validate_role_narrowing(None, &concrete).is_ok());
        assert_eq!(
            validate_role_narrowing(
                Some(&base),
                &GoalRoleCardV1 {
                    role_id: [32; 16],
                    permitted_class: GoalRoleClassV1::Heavy,
                    ..role_card()
                },
            )
            .expect_err("raised permitted class")
            .code(),
            DELEGATION_ROLE_WIDENING_FORBIDDEN
        );
        assert_eq!(
            validate_role_narrowing(
                Some(&base),
                &GoalRoleCardV1 {
                    role_id: [32; 16],
                    tool_subset: vec!["read".to_owned(), "write".to_owned()],
                    ..role_card()
                },
            )
            .expect_err("added tool")
            .code(),
            DELEGATION_ROLE_WIDENING_FORBIDDEN
        );
        assert_eq!(
            validate_role_narrowing(
                Some(&base),
                &GoalRoleCardV1 {
                    role_id: [32; 16],
                    context_limit_bytes: 8_192,
                    ..role_card()
                },
            )
            .expect_err("widened context limit")
            .code(),
            DELEGATION_ROLE_WIDENING_FORBIDDEN
        );
        assert_eq!(
            validate_role_narrowing(
                Some(&base),
                &GoalRoleCardV1 {
                    role_id: [32; 16],
                    result_limit_bytes: 8_192,
                    ..role_card()
                },
            )
            .expect_err("widened result limit")
            .code(),
            DELEGATION_ROLE_WIDENING_FORBIDDEN
        );
        assert_eq!(
            validate_role_narrowing(
                Some(&GoalRoleCardV1 {
                    revision: 0,
                    ..role_card()
                }),
                &concrete,
            )
            .expect_err("invalid base role")
            .code(),
            DELEGATION_ROLE_INVALID
        );
    }

    #[test]
    fn delegation_snapshot_validates_references_text_bounds_and_cards() {
        assert_eq!(
            GoalDelegationSnapshotV1 {
                leading_goal_id: [0; 16],
                ..delegation_snapshot()
            }
            .validate()
            .expect_err("zero leading Goal identity")
            .code(),
            GOAL_SNAPSHOT_UNAVAILABLE
        );
        assert_eq!(
            GoalDelegationSnapshotV1 {
                policy_snapshot_reference: [0; 16],
                ..delegation_snapshot()
            }
            .validate()
            .expect_err("zero policy snapshot reference")
            .code(),
            GOAL_SNAPSHOT_UNAVAILABLE
        );
        assert_eq!(
            GoalDelegationSnapshotV1 {
                activity_selection_reference: [0; 16],
                ..delegation_snapshot()
            }
            .validate()
            .expect_err("zero activity selection reference")
            .code(),
            GOAL_SNAPSHOT_UNAVAILABLE
        );
        assert_eq!(
            GoalDelegationSnapshotV1 {
                parent_task: "  ".to_owned(),
                ..delegation_snapshot()
            }
            .validate()
            .expect_err("blank parent task")
            .code(),
            GOAL_SNAPSHOT_UNAVAILABLE
        );
        assert_eq!(
            GoalDelegationSnapshotV1 {
                goal_revision: 0,
                ..delegation_snapshot()
            }
            .validate()
            .expect_err("zero leading Goal revision")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            GoalDelegationSnapshotV1 {
                required_constraints: vec!["api_key=secret".to_owned()],
                ..delegation_snapshot()
            }
            .validate()
            .expect_err("credential-shaped constraint")
            .code(),
            CREDENTIALS_FORBIDDEN
        );
        assert_eq!(
            GoalDelegationSnapshotV1 {
                parent_task: "a".repeat(600_000),
                required_constraints: vec!["b".repeat(500_000)],
                ..delegation_snapshot()
            }
            .validate()
            .expect_err("over-bound snapshot text")
            .code(),
            GOAL_SNAPSHOT_TOO_LARGE
        );
        let over_bound_cards: Vec<GoalCardReferenceV1> = (1..=GOAL_MAX_SELECTED_SKILL_ROLE_CARDS
            + 1)
            .map(|index| GoalCardReferenceV1 {
                card_reference: [u8::try_from(index).expect("the card seed fits a u8"); 16],
                revision: 1,
            })
            .collect();
        assert_eq!(
            GoalDelegationSnapshotV1 {
                memory_card_references: over_bound_cards,
                skill_card_references: Vec::new(),
                ..delegation_snapshot()
            }
            .validate()
            .expect_err("over-bound delegation cards")
            .code(),
            GOAL_LIMIT_EXCEEDED
        );
        assert_eq!(
            GoalDelegationSnapshotV1 {
                memory_card_references: vec![GoalCardReferenceV1 {
                    card_reference: [0; 16],
                    revision: 1,
                }],
                ..delegation_snapshot()
            }
            .validate()
            .expect_err("inexact delegation card reference")
            .code(),
            GOAL_SNAPSHOT_UNAVAILABLE
        );
        assert_eq!(
            GoalDelegationSnapshotV1 {
                memory_card_references: vec![card_reference(41)],
                skill_card_references: vec![GoalCardReferenceV1 {
                    revision: 2,
                    ..card_reference(41)
                }],
                ..delegation_snapshot()
            }
            .validate()
            .expect_err("duplicate delegation card identity")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        let with_role = GoalDelegationSnapshotV1 {
            selected_role: Some(role_card()),
            ..delegation_snapshot()
        };
        assert!(with_role.validate().is_ok());
        assert!(validate_goal_delegation_snapshot(&with_role, None).is_ok());
        assert!(
            validate_goal_delegation_snapshot(&delegation_snapshot(), Some(&role_card())).is_ok()
        );
        assert_eq!(
            validate_goal_delegation_snapshot(
                &GoalDelegationSnapshotV1 {
                    selected_role: Some(GoalRoleCardV1 {
                        role_id: [32; 16],
                        permitted_class: GoalRoleClassV1::Heavy,
                        ..role_card()
                    }),
                    ..delegation_snapshot()
                },
                Some(&role_card()),
            )
            .expect_err("widened delegated role")
            .code(),
            DELEGATION_ROLE_WIDENING_FORBIDDEN
        );
        assert_eq!(
            validate_goal_delegation_snapshot(
                &GoalDelegationSnapshotV1 {
                    selected_role: Some(GoalRoleCardV1 {
                        revision: 0,
                        ..role_card()
                    }),
                    ..delegation_snapshot()
                },
                None,
            )
            .expect_err("invalid delegated role")
            .code(),
            DELEGATION_ROLE_INVALID
        );
    }

    #[test]
    fn goal_run_admission_accepts_exact_project_and_session_goals() {
        assert!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &run_selection(),
            )
            .is_ok()
        );
        assert!(
            validate_goal_run_admission(
                &session_goal_record(),
                &[],
                1_024,
                &GoalRunSelectionV1 {
                    leading_goal_id: [7; 16],
                    goal_revision: 1,
                    scope_link_provenance: GoalScopeLinkProvenanceV1 {
                        project_id: [2; 16],
                        session_id: Some([3; 16]),
                        link_id: None,
                    },
                    ..run_selection()
                },
            )
            .is_ok()
        );
    }

    #[test]
    fn goal_run_admission_rejects_identity_scope_and_link_mismatches() {
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    leading_goal_id: [9; 16],
                    ..run_selection()
                },
            )
            .expect_err("foreign leading Goal")
            .code(),
            GOAL_NOT_ACTIVE
        );
        assert_eq!(
            validate_goal_run_admission(
                &GoalDto::new(
                    [1; 16],
                    project_scope(),
                    3,
                    GoalLifecycleStateDto::Paused,
                    GoalReadinessStateDto::NotReady,
                    GoalUserDecisionStateDto::Unaccepted,
                )
                .expect("the paused Goal fixture is coherent"),
                &[session_link()],
                1_024,
                &run_selection(),
            )
            .expect_err("non-Active leading Goal")
            .code(),
            GOAL_NOT_ACTIVE
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    goal_revision: 4,
                    ..run_selection()
                },
            )
            .expect_err("stale frozen revision")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    scope_link_provenance: GoalScopeLinkProvenanceV1 {
                        project_id: [9; 16],
                        session_id: Some([3; 16]),
                        link_id: Some([4; 16]),
                    },
                    ..run_selection()
                },
            )
            .expect_err("foreign selection project")
            .code(),
            GOAL_NOT_ACTIVE
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    scope_link_provenance: GoalScopeLinkProvenanceV1 {
                        project_id: [2; 16],
                        session_id: None,
                        link_id: None,
                    },
                    ..run_selection()
                },
            )
            .expect_err("project Goal without an explicit link")
            .code(),
            GOAL_NOT_ACTIVE
        );
        assert_eq!(
            validate_goal_run_admission(&project_goal_record(), &[], 1_024, &run_selection())
                .expect_err("unavailable explicit session link")
                .code(),
            GOAL_NOT_ACTIVE
        );
        assert_eq!(
            validate_goal_run_admission(
                &session_goal_record(),
                &[],
                1_024,
                &GoalRunSelectionV1 {
                    leading_goal_id: [7; 16],
                    goal_revision: 1,
                    scope_link_provenance: GoalScopeLinkProvenanceV1 {
                        project_id: [2; 16],
                        session_id: Some([9; 16]),
                        link_id: None,
                    },
                    ..run_selection()
                },
            )
            .expect_err("foreign session Goal")
            .code(),
            GOAL_NOT_ACTIVE
        );
        assert_eq!(
            validate_goal_run_admission(
                &session_goal_record(),
                &[],
                1_024,
                &GoalRunSelectionV1 {
                    leading_goal_id: [7; 16],
                    goal_revision: 1,
                    scope_link_provenance: GoalScopeLinkProvenanceV1 {
                        project_id: [2; 16],
                        session_id: Some([3; 16]),
                        link_id: Some([4; 16]),
                    },
                    ..run_selection()
                },
            )
            .expect_err("session Goal with a project link")
            .code(),
            GOAL_NOT_ACTIVE
        );
    }

    #[test]
    fn goal_run_admission_rejects_out_of_range_bounds_and_lists() {
        assert!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &run_selection(),
            )
            .is_ok()
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    bounds: GoalRunSelectionBoundsV1 {
                        context_bytes: 0,
                        ..selection_bounds()
                    },
                    ..run_selection()
                },
            )
            .expect_err("zero context bound")
            .code(),
            GOAL_LIMIT_EXCEEDED
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    bounds: GoalRunSelectionBoundsV1 {
                        max_memory_cards: GOAL_MAX_ACTIVE_MEMORY_CARDS + 1,
                        ..selection_bounds()
                    },
                    ..run_selection()
                },
            )
            .expect_err("over-bound memory card bound")
            .code(),
            GOAL_LIMIT_EXCEEDED
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    bounds: GoalRunSelectionBoundsV1 {
                        target_snapshot_bytes: 0,
                        ..selection_bounds()
                    },
                    ..run_selection()
                },
            )
            .expect_err("zero target snapshot bound")
            .code(),
            GOAL_LIMIT_EXCEEDED
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                0,
                &run_selection(),
            )
            .expect_err("unavailable target snapshot")
            .code(),
            GOAL_SNAPSHOT_UNAVAILABLE
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                2_048,
                &GoalRunSelectionV1 {
                    bounds: GoalRunSelectionBoundsV1 {
                        target_snapshot_bytes: 1_024,
                        ..selection_bounds()
                    },
                    ..run_selection()
                },
            )
            .expect_err("over-bound target snapshot")
            .code(),
            GOAL_SNAPSHOT_TOO_LARGE
        );
        let deep_chain: Vec<GoalRevisionReferenceV1> = (1..=GOAL_MAX_TREE_DEPTH)
            .map(|index| GoalRevisionReferenceV1 {
                goal_id: [u8::try_from(index).expect("the chain seed fits a u8") + 20; 16],
                revision: 1,
            })
            .collect();
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    parent_revision_chain: deep_chain,
                    ..run_selection()
                },
            )
            .expect_err("over-bound parent revision chain depth")
            .code(),
            GOAL_TREE_DEPTH_LIMIT_EXCEEDED
        );
        let over_bound_evidence: Vec<[u8; 16]> = (1..=GOAL_MAX_EVIDENCE_REFERENCES + 1)
            .map(|index| [u8::try_from(index % 250 + 1).expect("the evidence seed fits a u8"); 16])
            .collect();
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    valid_evidence_references: over_bound_evidence,
                    ..run_selection()
                },
            )
            .expect_err("over-bound selected evidence list")
            .code(),
            GOAL_LIMIT_EXCEEDED
        );
        let over_bound_gates: Vec<GoalGateRevisionReferenceV1> = (1..=GOAL_MAX_GATE_REVISIONS + 1)
            .map(|index| GoalGateRevisionReferenceV1 {
                gate_reference: [u8::try_from(index).expect("the gate seed fits a u8"); 16],
                revision: 1,
            })
            .collect();
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    selected_gate_revisions: over_bound_gates,
                    ..run_selection()
                },
            )
            .expect_err("over-bound selected gate list")
            .code(),
            GOAL_GATE_LIMIT_EXCEEDED
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    bounds: GoalRunSelectionBoundsV1 {
                        max_memory_cards: 1,
                        ..selection_bounds()
                    },
                    selected_memory_cards: vec![card_reference(41), card_reference(42)],
                    ..run_selection()
                },
            )
            .expect_err("over-bound selected memory cards")
            .code(),
            MEMORY_ENTRY_LIMIT_EXCEEDED
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    bounds: GoalRunSelectionBoundsV1 {
                        max_skill_role_cards: 1,
                        ..selection_bounds()
                    },
                    selected_skill_cards: vec![card_reference(41), card_reference(42)],
                    ..run_selection()
                },
            )
            .expect_err("over-bound selected Skill and role cards")
            .code(),
            GOAL_LIMIT_EXCEEDED
        );
    }

    #[test]
    fn goal_run_admission_rejects_inexact_and_duplicate_selected_references() {
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    parent_revision_chain: vec![GoalRevisionReferenceV1 {
                        goal_id: [8; 16],
                        revision: 0,
                    }],
                    ..run_selection()
                },
            )
            .expect_err("inexact parent chain revision")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    parent_revision_chain: vec![GoalRevisionReferenceV1 {
                        goal_id: [0; 16],
                        revision: 1,
                    }],
                    ..run_selection()
                },
            )
            .expect_err("inexact parent chain identity")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    parent_revision_chain: vec![GoalRevisionReferenceV1 {
                        goal_id: [1; 16],
                        revision: 1,
                    }],
                    ..run_selection()
                },
            )
            .expect_err("parent chain naming the leading Goal")
            .code(),
            GOAL_CYCLE_DETECTED
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    parent_revision_chain: vec![
                        GoalRevisionReferenceV1 {
                            goal_id: [8; 16],
                            revision: 1,
                        },
                        GoalRevisionReferenceV1 {
                            goal_id: [8; 16],
                            revision: 2,
                        },
                    ],
                    ..run_selection()
                },
            )
            .expect_err("repeated parent chain identity")
            .code(),
            GOAL_CYCLE_DETECTED
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    obligatory_component_references: vec![[0; 16]],
                    ..run_selection()
                },
            )
            .expect_err("inexact obligatory component reference")
            .code(),
            GOAL_NOT_ACTIVE
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    obligatory_component_references: vec![[1; 16]],
                    ..run_selection()
                },
            )
            .expect_err("obligatory component naming the leading Goal")
            .code(),
            GOAL_CYCLE_DETECTED
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    selected_gate_revisions: vec![GoalGateRevisionReferenceV1 {
                        gate_reference: [9; 16],
                        revision: 0,
                    }],
                    ..run_selection()
                },
            )
            .expect_err("inexact selected gate revision")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    selected_gate_revisions: vec![GoalGateRevisionReferenceV1 {
                        gate_reference: [0; 16],
                        revision: 1,
                    }],
                    ..run_selection()
                },
            )
            .expect_err("inexact selected gate identity")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    selected_gate_revisions: vec![
                        GoalGateRevisionReferenceV1 {
                            gate_reference: [9; 16],
                            revision: 1,
                        },
                        GoalGateRevisionReferenceV1 {
                            gate_reference: [9; 16],
                            revision: 2,
                        },
                    ],
                    ..run_selection()
                },
            )
            .expect_err("repeated selected gate identity")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    selected_memory_cards: vec![GoalCardReferenceV1 {
                        card_reference: [0; 16],
                        revision: 1,
                    }],
                    ..run_selection()
                },
            )
            .expect_err("inexact selected card reference")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    selected_memory_cards: vec![GoalCardReferenceV1 {
                        card_reference: [41; 16],
                        revision: 0,
                    }],
                    ..run_selection()
                },
            )
            .expect_err("inexact selected card revision")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    selected_memory_cards: vec![card_reference(41)],
                    selected_skill_cards: vec![GoalCardReferenceV1 {
                        revision: 2,
                        ..card_reference(41)
                    }],
                    ..run_selection()
                },
            )
            .expect_err("repeated selected card identity")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    valid_evidence_references: vec![[0; 16]],
                    ..run_selection()
                },
            )
            .expect_err("inexact valid evidence reference")
            .code(),
            GOAL_REVISION_CONFLICT
        );
        assert_eq!(
            validate_goal_run_admission(
                &project_goal_record(),
                &[session_link()],
                1_024,
                &GoalRunSelectionV1 {
                    valid_evidence_references: vec![[11; 16], [11; 16]],
                    ..run_selection()
                },
            )
            .expect_err("repeated valid evidence reference")
            .code(),
            GOAL_REVISION_CONFLICT
        );
    }

    #[test]
    fn refinement_drafts_cover_identity_bounds_exactness_and_coalescing() {
        assert!(refinement_draft().validate().is_ok());
        for draft in [
            RefinementDraftDto {
                draft_id: [0; 16],
                ..refinement_draft()
            },
            RefinementDraftDto {
                source_run_id: [0; 16],
                ..refinement_draft()
            },
            RefinementDraftDto {
                leading_goal_id: [0; 16],
                ..refinement_draft()
            },
            RefinementDraftDto {
                base_record_reference: [0; 16],
                ..refinement_draft()
            },
        ] {
            assert_eq!(
                draft
                    .validate()
                    .expect_err("missing refinement draft identity")
                    .code(),
                REFINEMENT_DRAFT_CONFLICT
            );
        }
        assert_eq!(
            RefinementDraftDto {
                base_goal_revision: 0,
                ..refinement_draft()
            }
            .validate()
            .expect_err("zero base Goal revision")
            .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        assert_eq!(
            RefinementDraftDto {
                base_record_revision: 0,
                ..refinement_draft()
            }
            .validate()
            .expect_err("zero base record revision")
            .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        assert_eq!(
            RefinementDraftDto {
                edits: Vec::new(),
                ..refinement_draft()
            }
            .validate()
            .expect_err("empty edit set")
            .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        let over_bound_edits = vec![refinement_edit(54); GOAL_MAX_EVIDENCE_REFERENCES + 1];
        assert_eq!(
            RefinementDraftDto {
                edits: over_bound_edits,
                ..refinement_draft()
            }
            .validate()
            .expect_err("over-bound edit set")
            .code(),
            GOAL_LIMIT_EXCEEDED
        );
        let over_bound_evidence = vec![evidence(55, 1); GOAL_MAX_EVIDENCE_REFERENCES + 1];
        assert_eq!(
            RefinementDraftDto {
                evidence_references: over_bound_evidence,
                ..refinement_draft()
            }
            .validate()
            .expect_err("over-bound evidence set")
            .code(),
            GOAL_LIMIT_EXCEEDED
        );
        assert_eq!(
            RefinementDraftDto {
                edits: vec![RefinementEditV1 {
                    kind: RefinementEditKindV1::StopDecision,
                    evidence: evidence(0, 1),
                }],
                ..refinement_draft()
            }
            .validate()
            .expect_err("inexact edit evidence")
            .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        assert_eq!(
            RefinementDraftDto {
                evidence_references: vec![evidence(0, 1)],
                ..refinement_draft()
            }
            .validate()
            .expect_err("inexact evidence reference")
            .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        assert_eq!(
            RefinementDraftDto {
                evidence_references: vec![evidence(55, 1), evidence(55, 1)],
                ..refinement_draft()
            }
            .validate()
            .expect_err("repeated evidence identity")
            .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        assert_eq!(
            RefinementDraftDto {
                safe_rationale: "  ".to_owned(),
                ..refinement_draft()
            }
            .validate()
            .expect_err("blank rationale")
            .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        assert_eq!(
            RefinementDraftDto {
                safe_rationale: "token=abc".to_owned(),
                ..refinement_draft()
            }
            .validate()
            .expect_err("credential-shaped rationale")
            .code(),
            CREDENTIALS_FORBIDDEN
        );
        assert_eq!(
            coalesce_refinement_draft(None, &refinement_draft()).expect("new pending draft"),
            GoalDraftCoalescingV1::CreateNew
        );
        assert_eq!(
            coalesce_refinement_draft(Some(&refinement_draft()), &refinement_draft())
                .expect("coalesced pending draft"),
            GoalDraftCoalescingV1::CoalesceInto { draft_id: [51; 16] }
        );
        assert_eq!(
            coalesce_refinement_draft(
                Some(&refinement_draft()),
                &RefinementDraftDto {
                    draft_id: [57; 16],
                    leading_goal_id: [2; 16],
                    ..refinement_draft()
                },
            )
            .expect("draft for another Goal"),
            GoalDraftCoalescingV1::CreateNew
        );
        assert_eq!(
            coalesce_refinement_draft(
                Some(&refinement_draft()),
                &RefinementDraftDto {
                    milestone: GoalMilestoneDto::Stop,
                    ..refinement_draft()
                },
            )
            .expect_err("unequal pending proposal")
            .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        assert!(validate_pending_draft_limit(&[refinement_draft()]).is_ok());
        assert_eq!(
            validate_pending_draft_limit(&[refinement_draft(), refinement_draft()])
                .expect_err("second pending draft for one Goal")
                .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        assert!(
            validate_pending_draft_limit(&[
                refinement_draft(),
                RefinementDraftDto {
                    draft_id: [57; 16],
                    leading_goal_id: [2; 16],
                    ..refinement_draft()
                },
            ])
            .is_ok()
        );
    }

    #[test]
    fn refinement_resolution_covers_reject_accept_and_edit_paths() {
        assert_eq!(
            resolve_refinement_draft(&refinement_draft(), RefinementDecisionV1::Reject, 0)
                .expect("rejected draft"),
            GoalRefinementResolutionV1::Rejected
        );
        assert_eq!(
            resolve_refinement_draft(&refinement_draft(), RefinementDecisionV1::Accept, 3)
                .expect("accepted draft"),
            GoalRefinementResolutionV1::Accepted { goal_revision: 4 }
        );
        assert_eq!(
            resolve_refinement_draft(&refinement_draft(), RefinementDecisionV1::Accept, 4)
                .expect_err("stale base revision")
                .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        assert_eq!(
            resolve_refinement_draft(&refinement_draft(), RefinementDecisionV1::Accept, 0)
                .expect_err("zero current revision")
                .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        let exhausted = RefinementDraftDto {
            base_goal_revision: u64::MAX,
            ..refinement_draft()
        };
        assert_eq!(
            resolve_refinement_draft(&exhausted, RefinementDecisionV1::Accept, u64::MAX)
                .expect_err("unadvanceable Goal revision")
                .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        assert_eq!(
            resolve_refinement_draft(
                &refinement_draft(),
                RefinementDecisionV1::EditAndAccept { edits: Vec::new() },
                3,
            )
            .expect_err("empty edit-and-accept set")
            .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        assert_eq!(
            resolve_refinement_draft(
                &refinement_draft(),
                RefinementDecisionV1::EditAndAccept {
                    edits: vec![RefinementEditV1 {
                        kind: RefinementEditKindV1::StopDecision,
                        evidence: evidence(0, 1),
                    }],
                },
                3,
            )
            .expect_err("inexact accepted edit")
            .code(),
            REFINEMENT_DRAFT_CONFLICT
        );
        assert_eq!(
            resolve_refinement_draft(
                &refinement_draft(),
                RefinementDecisionV1::EditAndAccept {
                    edits: vec![refinement_edit(58)],
                },
                3,
            )
            .expect("edited and accepted draft"),
            GoalRefinementResolutionV1::EditAccepted {
                edits: vec![refinement_edit(58)],
                goal_revision: 4,
            }
        );
    }

    #[test]
    fn compaction_extension_enforces_run_origin_range_and_chain() {
        let working = GoalCompactionWorkingFormV1 {
            current_summary: None,
            uncompacted_suffix: vec![[71; 16], [72; 16]],
        };
        assert!(
            validate_compaction_extension(
                &working,
                &summary(73, 1, None, 71, 71),
                GoalCompactionOriginV1::ActiveRun,
            )
            .is_ok()
        );
        assert_eq!(
            validate_compaction_extension(
                &working,
                &summary(73, 1, None, 71, 71),
                GoalCompactionOriginV1::AfterRestart,
            )
            .expect_err("non-active compaction origin")
            .code(),
            COMPACTION_HISTORY_UNAVAILABLE
        );
        assert_eq!(
            validate_compaction_extension(
                &GoalCompactionWorkingFormV1 {
                    current_summary: None,
                    uncompacted_suffix: Vec::new(),
                },
                &summary(73, 1, None, 71, 71),
                GoalCompactionOriginV1::ActiveRun,
            )
            .expect_err("no completed uncompacted range")
            .code(),
            COMPACTION_HISTORY_UNAVAILABLE
        );
        assert_eq!(
            validate_compaction_extension(
                &working,
                &summary(73, 1, None, 72, 72),
                GoalCompactionOriginV1::ActiveRun,
            )
            .expect_err("range not starting at the first uncompacted fact")
            .code(),
            COMPACTION_HISTORY_UNAVAILABLE
        );
        assert_eq!(
            validate_compaction_extension(
                &working,
                &summary(73, 1, None, 71, 99),
                GoalCompactionOriginV1::ActiveRun,
            )
            .expect_err("range ending outside the completed suffix")
            .code(),
            COMPACTION_HISTORY_UNAVAILABLE
        );
        assert_eq!(
            validate_compaction_extension(
                &working,
                &summary(73, 2, None, 71, 71),
                GoalCompactionOriginV1::ActiveRun,
            )
            .expect_err("first summary not at revision one")
            .code(),
            COMPACTION_SUMMARY_UNAVAILABLE
        );
        assert_eq!(
            validate_compaction_extension(
                &working,
                &summary(73, 1, Some(74), 71, 71),
                GoalCompactionOriginV1::ActiveRun,
            )
            .expect_err("first summary carrying a predecessor")
            .code(),
            COMPACTION_SUMMARY_UNAVAILABLE
        );
        let chained = GoalCompactionWorkingFormV1 {
            current_summary: Some(summary(75, 4, None, 60, 70)),
            uncompacted_suffix: vec![[71; 16], [72; 16]],
        };
        assert!(
            validate_compaction_extension(
                &chained,
                &summary(75, 5, Some(75), 71, 72),
                GoalCompactionOriginV1::ActiveRun,
            )
            .is_ok()
        );
        assert_eq!(
            validate_compaction_extension(
                &chained,
                &summary(75, 6, Some(75), 71, 71),
                GoalCompactionOriginV1::ActiveRun,
            )
            .expect_err("summary revision gap")
            .code(),
            COMPACTION_SUMMARY_UNAVAILABLE
        );
        assert_eq!(
            validate_compaction_extension(
                &chained,
                &summary(76, 5, Some(78), 71, 71),
                GoalCompactionOriginV1::ActiveRun,
            )
            .expect_err("summary predecessor mismatch")
            .code(),
            COMPACTION_SUMMARY_UNAVAILABLE
        );
        let previous = summary(77, 2, None, 80, 90);
        assert!(
            validate_conversation_summary_correction(&previous, &summary(77, 3, None, 80, 90))
                .is_ok()
        );
        assert_eq!(
            validate_conversation_summary_correction(&previous, &summary(78, 3, None, 80, 90))
                .expect_err("different corrected summary identity")
                .code(),
            COMPACTION_SUMMARY_UNAVAILABLE
        );
        assert_eq!(
            validate_conversation_summary_correction(&previous, &summary(77, 3, None, 81, 90))
                .expect_err("different corrected source range")
                .code(),
            COMPACTION_SUMMARY_UNAVAILABLE
        );
        assert!(validate_fork_summary_inheritance(&previous, &previous).is_ok());
        assert_eq!(
            validate_fork_summary_inheritance(&previous, &summary(77, 3, None, 80, 90))
                .expect_err("inherited summary revision mismatch")
                .code(),
            COMPACTION_SUMMARY_UNAVAILABLE
        );
        assert_eq!(
            validate_fork_summary_inheritance(&previous, &summary(78, 2, None, 80, 90))
                .expect_err("inherited summary identity mismatch")
                .code(),
            COMPACTION_SUMMARY_UNAVAILABLE
        );
    }

    #[test]
    fn conversation_summary_validation_covers_identity_range_content_and_credentials() {
        assert_eq!(
            ConversationSummaryDto {
                summary_id: [0; 16],
                ..summary(73, 1, None, 71, 71)
            }
            .validate()
            .expect_err("zero summary identity")
            .code(),
            COMPACTION_SUMMARY_UNAVAILABLE
        );
        assert_eq!(
            ConversationSummaryDto {
                revision: 0,
                ..summary(73, 1, None, 71, 71)
            }
            .validate()
            .expect_err("zero summary revision")
            .code(),
            COMPACTION_SUMMARY_UNAVAILABLE
        );
        assert_eq!(
            ConversationSummaryDto {
                source_range_start: [0; 16],
                ..summary(73, 1, None, 71, 71)
            }
            .validate()
            .expect_err("zero source range start")
            .code(),
            COMPACTION_HISTORY_UNAVAILABLE
        );
        assert_eq!(
            ConversationSummaryDto {
                source_range_end: [0; 16],
                ..summary(73, 1, None, 71, 71)
            }
            .validate()
            .expect_err("zero source range end")
            .code(),
            COMPACTION_HISTORY_UNAVAILABLE
        );
        assert_eq!(
            ConversationSummaryDto {
                safe_content: "  ".to_owned(),
                ..summary(73, 1, None, 71, 71)
            }
            .validate()
            .expect_err("blank summary content")
            .code(),
            COMPACTION_SUMMARY_UNAVAILABLE
        );
        assert_eq!(
            ConversationSummaryDto {
                safe_content: "token=abc".to_owned(),
                ..summary(73, 1, None, 71, 71)
            }
            .validate()
            .expect_err("credential-shaped summary content")
            .code(),
            CREDENTIALS_FORBIDDEN
        );
        assert_eq!(
            ConversationSummaryDto {
                safe_content: "a".repeat(record_bound_bytes() + 1),
                ..summary(73, 1, None, 71, 71)
            }
            .validate()
            .expect_err("over-bound summary content")
            .code(),
            COMPACTION_SUMMARY_TOO_LARGE
        );
    }
}
