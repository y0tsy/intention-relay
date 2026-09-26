//! Programmatic-caller policy, admission decisions, and reservations.
//!
//! Owner: architecture 27 (programmatic-caller policy and admission), with
//! ADR 0044 activation. This module owns the two closed root origins with
//! durable provenance, policy identity/scope revisions, narrowing-only
//! inheritance, effective snapshot resolution, the four admission decisions
//! with the interactive local-read baseline, exact confirmations, corridors,
//! policy lifecycle, policy drafts, per-run and calendar limits,
//! admission-time reservations, recovery dispositions, and the closed
//! `programmatic_policy_*` pre-effect failure set. Durable transactions live
//! in `intention-application` and `intention-storage-sqlite`.
//!
//! # Closed roots and durable provenance
//!
//! Exactly two root origins exist: `InteractiveUser` (an ordinary
//! user-admitted run and all of its descendants) and `ContinualHarness` (one
//! separately admitted harness launch and its descendants). No protocol peer,
//! detached Python task, child agent, MCP service, provider, bridge channel,
//! queued item, replay, or daemon recovery ever becomes an independent root,
//! and provenance is created once before `ToolCallStarted` and cannot be
//! acquired or re-rooted later.
//!
//! # CON-071 boundary
//!
//! The programmatic-caller policy gates ordinary fresh runs. A separately
//! issued Mandate direct admission under architecture 15 stays ungated by
//! this module (see [`validate_harness_delegation`]), and a continual harness
//! never calls `ask_user`: it either fits the user-approved corridor already
//! selected by its harness rule or fails before child admission.
//!
//! # Digests
//!
//! The snapshot, revision, provenance, draft, corridor, and policy-identity
//! digests in this module are deterministic domain identities over bounded,
//! credential-free fields. Their canonical record and wire encodings arrive
//! with the later wire wave; no digest here is a substitute for the frozen
//! `run-execution-meaning-v4` record codec owned by `run_execution_meaning`.

use intention_types::{DtoResult, ErrorDto};

use crate::canonical::{Digest256, contains_control_or_nul, contains_credential_shape};
use crate::slice3_selections::ProgrammaticCallerRootOriginV1;

/// Maximum characters of one programmatic-policy scalar text value.
pub const MAX_POLICY_TEXT_CHARS: usize = 256;
/// Maximum selected actions for one programmatic-policy run.
pub const MAX_POLICY_ACTIONS_PER_RUN: u64 = 256;
/// Maximum selected concurrent actions for one programmatic-policy run.
pub const MAX_POLICY_CONCURRENT_ACTIONS_PER_RUN: u64 = 16;
/// Minimum calendar action limit of one policy identity.
pub const MIN_CALENDAR_ACTION_LIMIT: u64 = 1;
/// Maximum calendar action limit of one policy identity.
pub const MAX_CALENDAR_ACTION_LIMIT: u64 = 4096;
/// Maximum durable policies in one project.
pub const MAX_POLICIES_PER_PROJECT: usize = 64;
/// Maximum durable policies attached to one goal.
pub const MAX_POLICIES_PER_GOAL: usize = 32;
/// Maximum own or inherited policy references in one session.
pub const MAX_POLICY_REFERENCES_PER_SESSION: usize = 32;
/// Maximum rules in one policy revision.
pub const MAX_RULES_PER_POLICY_REVISION: usize = 32;
/// Maximum selectors in one rule or corridor selector set.
pub const MAX_SELECTORS_PER_RULE: usize = 16;
/// Maximum typed input constraints in one rule or corridor selector.
pub const MAX_TYPED_INPUT_CONSTRAINTS_PER_RULE: usize = 16;
/// Maximum unfinished corridors in one root tree.
pub const MAX_UNFINISHED_CORRIDORS_PER_ROOT_TREE: usize = 16;
/// Maximum awaiting policy confirmations in one root tree.
pub const MAX_AWAITING_CONFIRMATIONS_PER_ROOT_TREE: usize = 16;
/// Maximum encoded bytes of one policy, revision, corridor, draft, or safe
/// admission evidence record (512 KiB).
pub const MAX_POLICY_RECORD_BYTES: u64 = 512 * 1024;
/// Maximum encoded bytes of one effective policy snapshot (1 MiB).
pub const MAX_EFFECTIVE_POLICY_SNAPSHOT_BYTES: u64 = 1024 * 1024;
/// Maximum actions of the code-owned interactive local-read baseline.
pub const INTERACTIVE_LOCAL_READ_BASELINE_ACTIONS: u64 = 256;
/// Maximum concurrent actions of the code-owned interactive local-read
/// baseline.
pub const INTERACTIVE_LOCAL_READ_BASELINE_CONCURRENT_ACTIONS: u64 = 16;
/// The closed direct-local-read tool set of the first policy scope.
pub const DIRECT_LOCAL_READ_TOOL_IDS: [&str; 5] = ["read", "glob", "grep", "expand", "retrieve"];
/// The registered user-interaction tool identity.
pub const USER_INTERACTION_TOOL_ID: &str = "ask_user";
/// The registered child-delegation tool identity.
pub const HARNESS_DELEGATION_TOOL_ID: &str = "sub_agent";

// The closed architecture 27 safe-failure set. Each code is declared exactly
// once; every one is a known pre-effect rejection.
/// A code-owned policy, rule, selector, or scope bound was exceeded.
pub const PROGRAMMATIC_POLICY_LIMIT_EXCEEDED: &str = "programmatic_policy_limit_exceeded";
/// One effective policy snapshot exceeds its 1 MiB bound.
pub const PROGRAMMATIC_POLICY_SNAPSHOT_TOO_LARGE: &str = "programmatic_policy_snapshot_too_large";
/// No applicable immutable snapshot exists for the requested root origin.
pub const PROGRAMMATIC_POLICY_SNAPSHOT_UNAVAILABLE: &str =
    "programmatic_policy_snapshot_unavailable";
/// One revision, base state, or identity is not the exact expected revision.
pub const PROGRAMMATIC_POLICY_REVISION_CONFLICT: &str = "programmatic_policy_revision_conflict";
/// A child or live tightening attempted to widen an inherited selection.
pub const PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN: &str =
    "programmatic_policy_inheritance_widening_forbidden";
/// One root origin is invalid for the requested decision or call shape.
pub const PROGRAMMATIC_POLICY_ORIGIN_INVALID: &str = "programmatic_policy_origin_invalid";
/// The selected policy, scope, tool, or state does not apply to this call.
pub const PROGRAMMATIC_POLICY_NOT_APPLICABLE: &str = "programmatic_policy_not_applicable";
/// A suspended policy denies every not-yet-started matching call.
pub const PROGRAMMATIC_POLICY_SUSPENDED: &str = "programmatic_policy_suspended";
/// A revoked policy denies every new admission.
pub const PROGRAMMATIC_POLICY_REVOKED: &str = "programmatic_policy_revoked";
/// The call requires an exact or bounded confirmation it does not carry.
pub const PROGRAMMATIC_POLICY_CONFIRMATION_REQUIRED: &str =
    "programmatic_policy_confirmation_required";
/// One exact confirmation is outside its validity window.
pub const PROGRAMMATIC_POLICY_CONFIRMATION_EXPIRED: &str =
    "programmatic_policy_confirmation_expired";
/// Only the `InteractiveUser` root run may start `ask_user`.
pub const PROGRAMMATIC_POLICY_ROOT_ONLY_INTERACTION: &str =
    "programmatic_policy_root_only_interaction";
/// One corridor is absent, expired, detached, or outside its root tree.
pub const PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE: &str =
    "programmatic_policy_corridor_unavailable";
/// One corridor exhausted its shared action or concurrency allocation.
pub const PROGRAMMATIC_POLICY_CORRIDOR_EXHAUSTED: &str = "programmatic_policy_corridor_exhausted";
/// One call does not satisfy the selected descriptor input constraint family.
pub const PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH: &str =
    "programmatic_policy_input_constraint_mismatch";
/// The per-run action or concurrency limit is exhausted.
pub const PROGRAMMATIC_POLICY_RUN_LIMIT_EXCEEDED: &str = "programmatic_policy_run_limit_exceeded";
/// The calendar action limit is exhausted.
pub const PROGRAMMATIC_POLICY_CALENDAR_LIMIT_EXCEEDED: &str =
    "programmatic_policy_calendar_limit_exceeded";
/// The required policy counter is unavailable or incompatible.
pub const PROGRAMMATIC_POLICY_COUNTER_UNAVAILABLE: &str = "programmatic_policy_counter_unavailable";
/// One reservation conflicts with an accepted reservation for the same call.
pub const PROGRAMMATIC_POLICY_RESERVATION_CONFLICT: &str =
    "programmatic_policy_reservation_conflict";
/// A harness delegation does not fit its user-approved corridor.
pub const PROGRAMMATIC_POLICY_HARNESS_DELEGATION_FORBIDDEN: &str =
    "programmatic_policy_harness_delegation_forbidden";
/// One pending draft already conflicts for the owner scope.
pub const PROGRAMMATIC_POLICY_DRAFT_CONFLICT: &str = "programmatic_policy_draft_conflict";
/// One policy draft exceeds its 512 KiB bound.
pub const PROGRAMMATIC_POLICY_DRAFT_TOO_LARGE: &str = "programmatic_policy_draft_too_large";

/// Every closed architecture 27 programmatic-policy failure code, in the
/// architecture 27 declaration order.
pub const PROGRAMMATIC_POLICY_FAILURE_CODES: [&str; 22] = [
    PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
    PROGRAMMATIC_POLICY_SNAPSHOT_TOO_LARGE,
    PROGRAMMATIC_POLICY_SNAPSHOT_UNAVAILABLE,
    PROGRAMMATIC_POLICY_REVISION_CONFLICT,
    PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
    PROGRAMMATIC_POLICY_ORIGIN_INVALID,
    PROGRAMMATIC_POLICY_NOT_APPLICABLE,
    PROGRAMMATIC_POLICY_SUSPENDED,
    PROGRAMMATIC_POLICY_REVOKED,
    PROGRAMMATIC_POLICY_CONFIRMATION_REQUIRED,
    PROGRAMMATIC_POLICY_CONFIRMATION_EXPIRED,
    PROGRAMMATIC_POLICY_ROOT_ONLY_INTERACTION,
    PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE,
    PROGRAMMATIC_POLICY_CORRIDOR_EXHAUSTED,
    PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH,
    PROGRAMMATIC_POLICY_RUN_LIMIT_EXCEEDED,
    PROGRAMMATIC_POLICY_CALENDAR_LIMIT_EXCEEDED,
    PROGRAMMATIC_POLICY_COUNTER_UNAVAILABLE,
    PROGRAMMATIC_POLICY_RESERVATION_CONFLICT,
    PROGRAMMATIC_POLICY_HARNESS_DELEGATION_FORBIDDEN,
    PROGRAMMATIC_POLICY_DRAFT_CONFLICT,
    PROGRAMMATIC_POLICY_DRAFT_TOO_LARGE,
];

/// The registered MCP tool identity.
const MCP_TOOL_ID: &str = "mcp";
/// The registered shell tool identity.
const EXECUTE_TOOL_ID: &str = "execute";
/// The repository-wide safe credential rejection code.
const CREDENTIALS_FORBIDDEN: &str = "credentials_forbidden";

/// Builds one typed pre-effect policy rejection.
fn policy_error(code: &'static str, message: &'static str) -> ErrorDto {
    ErrorDto::validation(code, message)
}

/// Validates one bounded, credential-free policy scalar text value.
///
/// # Errors
///
/// Returns `programmatic_policy_not_applicable` for a blank value,
/// `programmatic_policy_limit_exceeded` for an over-long or control-bearing
/// value, and `credentials_forbidden` for a credential-shaped value.
fn validate_policy_text(value: &str) -> DtoResult<()> {
    if value.trim().is_empty() {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_NOT_APPLICABLE,
            "policy text must name a selection",
        ));
    }
    if value.chars().count() > MAX_POLICY_TEXT_CHARS || contains_control_or_nul(value) {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
            "policy text must stay inside its closed bound",
        ));
    }
    if contains_credential_shape(value) {
        return Err(policy_error(
            CREDENTIALS_FORBIDDEN,
            "credentials are forbidden",
        ));
    }
    Ok(())
}

/// Validates one typed input constraint scalar value or family name.
///
/// # Errors
///
/// Returns `programmatic_policy_input_constraint_mismatch` for a blank or
/// control-bearing value, `programmatic_policy_limit_exceeded` for an
/// over-long value, and `credentials_forbidden` for a credential-shaped
/// value.
fn validate_constraint_text(value: &str) -> DtoResult<()> {
    if value.trim().is_empty() || contains_control_or_nul(value) {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH,
            "typed input constraints must be non-empty safe text",
        ));
    }
    if value.chars().count() > MAX_POLICY_TEXT_CHARS {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
            "typed input constraints must stay inside their closed bound",
        ));
    }
    if contains_credential_shape(value) {
        return Err(policy_error(
            CREDENTIALS_FORBIDDEN,
            "credentials are forbidden",
        ));
    }
    Ok(())
}

/// Compares two raw UUID values without a trait call.
const fn same_uuid(left: [u8; 16], right: [u8; 16]) -> bool {
    let mut index = 0;
    while index < 16 {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// Appends one length-framed field to a deterministic digest input.
fn push_field(input: &mut Vec<u8>, field: &[u8]) {
    input.extend_from_slice(&u64::try_from(field.len()).unwrap_or(u64::MAX).to_be_bytes());
    input.extend_from_slice(field);
}

/// Appends one big-endian `u64` to a deterministic digest input.
fn push_u64(input: &mut Vec<u8>, value: u64) {
    input.extend_from_slice(&value.to_be_bytes());
}

/// Appends one length-framed count to a deterministic digest input.
fn push_count(input: &mut Vec<u8>, count: usize) {
    push_u64(input, u64::try_from(count).unwrap_or(u64::MAX));
}

/// Appends one length-framed text field to a deterministic digest input.
fn push_text(input: &mut Vec<u8>, value: &str) {
    push_field(input, value.as_bytes());
}

/// Appends one raw UUID field to a deterministic digest input.
fn push_uuid(input: &mut Vec<u8>, value: [u8; 16]) {
    push_field(input, &value);
}

/// Appends one optional UUID field to a deterministic digest input.
fn push_optional_uuid(input: &mut Vec<u8>, value: Option<[u8; 16]>) {
    match value {
        Some(bytes) => {
            push_field(input, &[1]);
            push_uuid(input, bytes);
        }
        None => push_field(input, &[0]),
    }
}

/// Appends one optional text field to a deterministic digest input.
fn push_optional_text(input: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(text) => {
            push_field(input, &[1]);
            push_text(input, text);
        }
        None => push_field(input, &[0]),
    }
}

/// Appends one policy scope to a deterministic digest input.
fn push_scope(input: &mut Vec<u8>, scope: &ProgrammaticCallerPolicyScopeV1) {
    match scope {
        ProgrammaticCallerPolicyScopeV1::Project { project_id } => {
            push_field(input, &[0]);
            push_uuid(input, *project_id);
        }
        ProgrammaticCallerPolicyScopeV1::Goal {
            project_id,
            goal_id,
        } => {
            push_field(input, &[1]);
            push_uuid(input, *project_id);
            push_uuid(input, *goal_id);
        }
        ProgrammaticCallerPolicyScopeV1::Session {
            project_id,
            policy_owner_session_id,
        } => {
            push_field(input, &[2]);
            push_uuid(input, *project_id);
            push_uuid(input, *policy_owner_session_id);
        }
    }
}

/// Appends one rule selector to a deterministic digest input.
fn push_selector(input: &mut Vec<u8>, selector: &ProgrammaticRuleSelectorV1) {
    match selector {
        ProgrammaticRuleSelectorV1::EffectProfile {
            required_effect_flags,
        } => {
            push_field(input, &[0]);
            push_count(input, required_effect_flags.len());
            for flag in required_effect_flags {
                push_field(input, &[flag.restrictiveness_code()]);
            }
        }
        ProgrammaticRuleSelectorV1::ExactTool {
            tool_id,
            descriptor_revision,
        } => {
            push_field(input, &[1]);
            push_text(input, tool_id);
            push_u64(input, *descriptor_revision);
        }
        ProgrammaticRuleSelectorV1::McpMethod { reference } => {
            push_field(input, &[2]);
            push_uuid(input, reference.connection_reference);
            push_text(input, &reference.method_reference);
            push_text(input, &reference.method_schema_revision);
        }
    }
}

/// Appends one typed input constraint selection to a deterministic digest
/// input.
fn push_constraint(input: &mut Vec<u8>, constraint: &DescriptorInputConstraintSelectionV1) {
    push_text(input, &constraint.family);
    push_u64(input, constraint.revision);
    push_count(input, constraint.typed_values.len());
    for value in &constraint.typed_values {
        push_text(input, value);
    }
}

/// Appends one admission rule entry to a deterministic digest input.
fn push_rule(input: &mut Vec<u8>, rule: &ProgrammaticAdmissionRuleEntryV1) {
    push_field(input, &[rule.decision.restrictiveness()]);
    push_count(input, rule.selectors.len());
    for selector in &rule.selectors {
        push_selector(input, selector);
    }
    push_count(input, rule.typed_input_constraints.len());
    for constraint in &rule.typed_input_constraints {
        push_constraint(input, constraint);
    }
}

/// Appends the closed root-origin rules to a deterministic digest input.
fn push_origin_rules(input: &mut Vec<u8>, rules: &[ProgrammaticRootOriginRuleV1]) {
    push_count(input, rules.len());
    for rule in rules {
        push_field(input, &[rule.root_origin_kind.code()]);
        push_field(input, &[rule.maximum_decision.restrictiveness()]);
    }
}

/// The closed root-origin kinds of the first policy scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticRootOriginKindV1 {
    /// The root of an ordinary user-admitted run and all of its descendants.
    InteractiveUser,
    /// The root of one separately admitted harness launch and descendants.
    ContinualHarness,
}

impl ProgrammaticRootOriginKindV1 {
    /// Returns the closed origin kind of one daemon-assigned root origin.
    #[must_use]
    pub const fn of(origin: &ProgrammaticCallerRootOriginV1) -> Self {
        match origin {
            ProgrammaticCallerRootOriginV1::InteractiveUser { .. } => Self::InteractiveUser,
            ProgrammaticCallerRootOriginV1::ContinualHarness { .. } => Self::ContinualHarness,
        }
    }

    /// Whether this origin is the ordinary interactive user root.
    #[must_use]
    pub const fn is_interactive_user(self) -> bool {
        matches!(self, Self::InteractiveUser)
    }

    /// The stable closed code of this origin kind.
    const fn code(self) -> u8 {
        match self {
            Self::InteractiveUser => 0,
            Self::ContinualHarness => 1,
        }
    }
}

/// The closed calling-path candidates of one programmatic action.
///
/// Only the interactive user turn and the continually admitted harness launch
/// may originate a root; every other path remains a descendant of whichever
/// root admitted it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticCallerPathV1 {
    /// An ordinary user turn that roots an interactive user run.
    InteractiveUserTurn,
    /// A separately admitted continual-harness launch that roots its subtree.
    ContinualHarnessLaunch,
    /// A local protocol peer adapter.
    ProtocolPeer,
    /// A detached Python facade task.
    DetachedPythonTask,
    /// A child agent or delegated verifier.
    ChildAgent,
    /// An MCP service.
    McpService,
    /// A provider or provider adapter.
    Provider,
    /// A bridge channel or queued bridge run grant.
    BridgeChannel,
    /// A queued work item.
    QueuedItem,
    /// A replayed historical action.
    Replay,
    /// A daemon recovery pass.
    DaemonRecovery,
}

/// Whether one calling-path candidate may originate an independent root.
#[must_use]
pub const fn path_may_root(path: ProgrammaticCallerPathV1) -> bool {
    matches!(
        path,
        ProgrammaticCallerPathV1::InteractiveUserTurn
            | ProgrammaticCallerPathV1::ContinualHarnessLaunch
    )
}

/// The closed declared tool-effect flags a policy selector can require.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticToolEffectFlagV1 {
    /// Reads or discloses local content.
    LocalRead,
    /// Writes or edits local content.
    LocalWrite,
    /// Executes a local command or process.
    LocalExecute,
    /// Performs a network call.
    NetworkAccess,
    /// Delegates to a child agent.
    ChildDelegation,
    /// Interacts with the user.
    UserInteraction,
    /// Invokes one selected MCP method.
    McpInvocation,
}

impl ProgrammaticToolEffectFlagV1 {
    /// The stable closed code of this effect flag.
    const fn restrictiveness_code(self) -> u8 {
        match self {
            Self::LocalRead => 0,
            Self::LocalWrite => 1,
            Self::LocalExecute => 2,
            Self::NetworkAccess => 3,
            Self::ChildDelegation => 4,
            Self::UserInteraction => 5,
            Self::McpInvocation => 6,
        }
    }
}

/// The closed admission decisions of the first policy scope.
///
/// Restrictiveness ascends from `DirectLocalRead` through
/// `BoundedConfirmationRequired` and `ExactConfirmationRequired` to
/// `Prohibited`; the most restrictive applicable decision wins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticAdmissionRuleV1 {
    /// The call is denied.
    Prohibited,
    /// The call is admitted directly for the closed interactive read set.
    DirectLocalRead,
    /// The call requires a user-approved bounded corridor.
    BoundedConfirmationRequired,
    /// The call requires one exact user confirmation.
    ExactConfirmationRequired,
}

impl ProgrammaticAdmissionRuleV1 {
    /// The stable closed restrictiveness rank; a larger rank permits less.
    #[must_use]
    pub const fn restrictiveness(self) -> u8 {
        match self {
            Self::DirectLocalRead => 0,
            Self::BoundedConfirmationRequired => 1,
            Self::ExactConfirmationRequired => 2,
            Self::Prohibited => 3,
        }
    }

    /// Returns the most restrictive of two decisions.
    #[must_use]
    pub const fn most_restrictive(self, other: Self) -> Self {
        if self.restrictiveness() >= other.restrictiveness() {
            self
        } else {
            other
        }
    }
}

/// The code-owned `InteractiveLocalReadBaselineV1` selection.
///
/// It applies only to the `InteractiveUser` root run itself when no durable
/// policy is applicable, has no calendar counter, and cannot create a
/// corridor. A durable policy may only narrow it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InteractiveLocalReadBaselineV1 {
    /// The maximum actions of the one root run.
    pub maximum_action_count: u64,
    /// The maximum concurrent actions of the one root run.
    pub maximum_concurrent_actions: u64,
}

impl InteractiveLocalReadBaselineV1 {
    /// The frozen baseline values: 256 actions and 16 concurrent actions.
    #[must_use]
    pub const fn v1() -> Self {
        Self {
            maximum_action_count: INTERACTIVE_LOCAL_READ_BASELINE_ACTIONS,
            maximum_concurrent_actions: INTERACTIVE_LOCAL_READ_BASELINE_CONCURRENT_ACTIONS,
        }
    }

    /// Validates this baseline against its frozen code-owned bound.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_limit_exceeded` when a bound is zero or
    /// exceeds the frozen baseline.
    pub fn validate(&self) -> DtoResult<()> {
        if self.maximum_action_count == 0
            || self.maximum_action_count > INTERACTIVE_LOCAL_READ_BASELINE_ACTIONS
            || self.maximum_concurrent_actions == 0
            || self.maximum_concurrent_actions > INTERACTIVE_LOCAL_READ_BASELINE_CONCURRENT_ACTIONS
        {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
                "the interactive local-read baseline must not exceed its frozen 256/16 bound",
            ));
        }
        Ok(())
    }

    /// Returns the baseline bounds as run limits.
    #[must_use]
    const fn as_run_limits(self) -> ProgrammaticRunLimitsV1 {
        ProgrammaticRunLimitsV1 {
            max_actions: self.maximum_action_count,
            max_concurrent_actions: self.maximum_concurrent_actions,
        }
    }
}

/// Resolves the policy-less interactive local-read baseline for one call.
///
/// The baseline applies only to the `InteractiveUser` root run itself, never
/// automatically to a descendant, and never to a `ContinualHarness` root.
///
/// # Errors
///
/// Returns `programmatic_policy_origin_invalid` when the root origin is a
/// continual harness.
pub fn resolve_interactive_local_read_baseline(
    origin: &ProgrammaticCallerRootOriginV1,
    is_root_run_call: bool,
) -> DtoResult<Option<InteractiveLocalReadBaselineV1>> {
    if ProgrammaticRootOriginKindV1::of(origin) == ProgrammaticRootOriginKindV1::ContinualHarness {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_ORIGIN_INVALID,
            "a harness root has no interactive local-read baseline",
        ));
    }
    if is_root_run_call {
        Ok(Some(InteractiveLocalReadBaselineV1::v1()))
    } else {
        Ok(None)
    }
}

/// One per-run action and concurrency limit selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgrammaticRunLimitsV1 {
    /// The maximum actions of one run.
    pub max_actions: u64,
    /// The maximum concurrent actions of one run.
    pub max_concurrent_actions: u64,
}

impl ProgrammaticRunLimitsV1 {
    /// The maximum selectable run limits: 256 actions and 16 concurrent
    /// actions.
    #[must_use]
    pub const fn maximum_v1() -> Self {
        Self {
            max_actions: MAX_POLICY_ACTIONS_PER_RUN,
            max_concurrent_actions: MAX_POLICY_CONCURRENT_ACTIONS_PER_RUN,
        }
    }

    /// Creates one bounded per-run limit selection.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_limit_exceeded` when a bound is zero or
    /// exceeds its code-owned maximum.
    pub fn new(max_actions: u64, max_concurrent_actions: u64) -> DtoResult<Self> {
        if max_actions == 0
            || max_actions > MAX_POLICY_ACTIONS_PER_RUN
            || max_concurrent_actions == 0
            || max_concurrent_actions > MAX_POLICY_CONCURRENT_ACTIONS_PER_RUN
        {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
                "run limits must stay inside the closed 256/16 policy bound",
            ));
        }
        Ok(Self {
            max_actions,
            max_concurrent_actions,
        })
    }

    /// Returns the smaller non-zero intersection of two limit selections.
    #[must_use]
    pub const fn narrowed(self, other: Self) -> Self {
        Self {
            max_actions: if self.max_actions < other.max_actions {
                self.max_actions
            } else {
                other.max_actions
            },
            max_concurrent_actions: if self.max_concurrent_actions < other.max_concurrent_actions {
                self.max_concurrent_actions
            } else {
                other.max_concurrent_actions
            },
        }
    }
}

/// The closed immutable calendar period kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticCalendarPeriodKindV1 {
    /// One project-calendar day.
    Day,
    /// One project-calendar week.
    Week,
    /// One project-calendar month.
    Month,
}

impl ProgrammaticCalendarPeriodKindV1 {
    /// The stable closed code of this period kind.
    const fn code(self) -> u8 {
        match self {
            Self::Day => 0,
            Self::Week => 1,
            Self::Month => 2,
        }
    }
}

/// One immutable calendar action limit selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgrammaticCalendarLimitV1 {
    /// The immutable period kind selected at policy creation.
    pub period_kind: ProgrammaticCalendarPeriodKindV1,
    /// The calendar action limit from 1 through 4,096.
    pub max_actions: u64,
}

impl ProgrammaticCalendarLimitV1 {
    /// Creates one bounded calendar limit selection.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_limit_exceeded` when the action limit is
    /// outside the closed 1..=4,096 bound.
    pub fn new(period_kind: ProgrammaticCalendarPeriodKindV1, max_actions: u64) -> DtoResult<Self> {
        if !(MIN_CALENDAR_ACTION_LIMIT..=MAX_CALENDAR_ACTION_LIMIT).contains(&max_actions) {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
                "the calendar action limit must stay inside the closed 1..=4096 bound",
            ));
        }
        Ok(Self {
            period_kind,
            max_actions,
        })
    }
}

/// Validates one calendar revision against its parent.
///
/// # Errors
///
/// Returns `programmatic_policy_revision_conflict` when the immutable period
/// kind changed and `programmatic_policy_inheritance_widening_forbidden` when
/// the numeric limit increased.
pub fn validate_calendar_revision(
    current: &ProgrammaticCalendarLimitV1,
    next: &ProgrammaticCalendarLimitV1,
) -> DtoResult<()> {
    if current.period_kind != next.period_kind {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_REVISION_CONFLICT,
            "a revision cannot change the immutable calendar period kind",
        ));
    }
    if next.max_actions > current.max_actions {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
            "a revision may only narrow the numeric calendar limit",
        ));
    }
    Ok(())
}

/// The closed durable policy scopes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticCallerPolicyScopeV1 {
    /// Applies to every current and future session in one project.
    Project {
        /// The owning project identity.
        project_id: [u8; 16],
    },
    /// Applies only through the selected leading goal or its frozen ancestor
    /// chain.
    Goal {
        /// The owning project identity.
        project_id: [u8; 16],
        /// The selected goal identity.
        goal_id: [u8; 16],
    },
    /// Applies to its owner session and, through an explicit immutable
    /// reference only, to a fork of that session.
    Session {
        /// The owning project identity.
        project_id: [u8; 16],
        /// The owner session identity.
        policy_owner_session_id: [u8; 16],
    },
}

impl ProgrammaticCallerPolicyScopeV1 {
    /// Returns the owning project identity of this scope.
    #[must_use]
    pub const fn project_id(&self) -> [u8; 16] {
        match *self {
            Self::Project { project_id }
            | Self::Goal { project_id, .. }
            | Self::Session { project_id, .. } => project_id,
        }
    }
}

/// Validates that one durable scope is applicable to one admission context.
///
/// # Errors
///
/// Returns `programmatic_policy_not_applicable` when the scope's project,
/// selected goal chain, or explicit session inheritance does not cover the
/// context.
pub fn validate_scope_applicability(
    scope: &ProgrammaticCallerPolicyScopeV1,
    project_id: [u8; 16],
    session_id: [u8; 16],
    leading_goal_chain: &[[u8; 16]],
    inherited_from_source_session: bool,
) -> DtoResult<()> {
    if scope.project_id() != project_id {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_NOT_APPLICABLE,
            "a policy scope never crosses its project",
        ));
    }
    let applicable = match scope {
        ProgrammaticCallerPolicyScopeV1::Project { .. } => true,
        ProgrammaticCallerPolicyScopeV1::Goal { goal_id, .. } => {
            leading_goal_chain.contains(goal_id)
        }
        ProgrammaticCallerPolicyScopeV1::Session {
            policy_owner_session_id,
            ..
        } => same_uuid(*policy_owner_session_id, session_id) || inherited_from_source_session,
    };
    if applicable {
        Ok(())
    } else {
        Err(policy_error(
            PROGRAMMATIC_POLICY_NOT_APPLICABLE,
            "the selected goal chain or explicit session inheritance does not cover this context",
        ))
    }
}

/// One descriptor-declared closed typed input constraint selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescriptorInputConstraintSelectionV1 {
    /// The descriptor-declared closed constraint family name.
    pub family: String,
    /// The exact constraint family revision.
    pub revision: u64,
    /// The bounded typed values inside the selected family.
    pub typed_values: Vec<String>,
}

impl DescriptorInputConstraintSelectionV1 {
    /// Creates one bounded typed input constraint selection.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_input_constraint_mismatch` for a blank or
    /// control-bearing family or value or a zero family revision,
    /// `programmatic_policy_limit_exceeded` for an over-long text value or an
    /// over-limit typed-value list, and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn new(
        family: impl Into<String>,
        revision: u64,
        typed_values: Vec<String>,
    ) -> DtoResult<Self> {
        let family = family.into();
        validate_constraint_text(&family)?;
        if revision == 0 {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH,
                "a typed input constraint family requires an exact revision",
            ));
        }
        if typed_values.len() > MAX_TYPED_INPUT_CONSTRAINTS_PER_RULE {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
                "one typed input constraint selection may carry at most 16 values",
            ));
        }
        for value in &typed_values {
            validate_constraint_text(value)?;
        }
        Ok(Self {
            family,
            revision,
            typed_values,
        })
    }
}

/// One exact selected MCP connection, method, and schema revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticMcpMethodReferenceV1 {
    /// The selected MCP connection reference.
    pub connection_reference: [u8; 16],
    /// The exact selected method reference.
    pub method_reference: String,
    /// The exact discovered method schema revision.
    pub method_schema_revision: String,
}

impl ProgrammaticMcpMethodReferenceV1 {
    /// Creates one exact MCP method reference.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_not_applicable` for a blank method value,
    /// `programmatic_policy_limit_exceeded` for an over-long or
    /// control-bearing value, and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn new(
        connection_reference: [u8; 16],
        method_reference: impl Into<String>,
        method_schema_revision: impl Into<String>,
    ) -> DtoResult<Self> {
        let method_reference = method_reference.into();
        let method_schema_revision = method_schema_revision.into();
        validate_policy_text(&method_reference)?;
        validate_policy_text(&method_schema_revision)?;
        Ok(Self {
            connection_reference,
            method_reference,
            method_schema_revision,
        })
    }
}

/// The closed rule and corridor selectors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgrammaticRuleSelectorV1 {
    /// Selects calls whose declared effect flags contain every listed flag.
    EffectProfile {
        /// The required declared effect flags.
        required_effect_flags: Vec<ProgrammaticToolEffectFlagV1>,
    },
    /// Selects one exact registered tool and its selected descriptor revision.
    ExactTool {
        /// The exact registered tool identity.
        tool_id: String,
        /// The exact selected descriptor revision.
        descriptor_revision: u64,
    },
    /// Selects exactly one MCP connection, method, and schema revision.
    McpMethod {
        /// The exact selected MCP method reference.
        reference: ProgrammaticMcpMethodReferenceV1,
    },
}

impl ProgrammaticRuleSelectorV1 {
    /// Creates one effect-profile selector.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_not_applicable` for an empty or
    /// duplicated flag list and `programmatic_policy_limit_exceeded` for an
    /// over-limit list.
    pub fn effect_profile(
        required_effect_flags: Vec<ProgrammaticToolEffectFlagV1>,
    ) -> DtoResult<Self> {
        if required_effect_flags.is_empty() {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_NOT_APPLICABLE,
                "an effect selector must require at least one declared flag",
            ));
        }
        if required_effect_flags.len() > MAX_SELECTORS_PER_RULE {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
                "an effect selector may require at most 16 flags",
            ));
        }
        for (index, flag) in required_effect_flags.iter().enumerate() {
            if required_effect_flags[..index].contains(flag) {
                return Err(policy_error(
                    PROGRAMMATIC_POLICY_NOT_APPLICABLE,
                    "an effect selector never repeats one declared flag",
                ));
            }
        }
        Ok(Self::EffectProfile {
            required_effect_flags,
        })
    }

    /// Creates one exact-tool selector.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_not_applicable` for a blank tool identity
    /// or a zero descriptor revision, `programmatic_policy_limit_exceeded`
    /// for an over-long or control-bearing identity, and
    /// `credentials_forbidden` for a credential-shaped identity.
    pub fn exact_tool(tool_id: impl Into<String>, descriptor_revision: u64) -> DtoResult<Self> {
        let tool_id = tool_id.into();
        validate_policy_text(&tool_id)?;
        if descriptor_revision == 0 {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_NOT_APPLICABLE,
                "an exact tool selector requires an exact descriptor revision",
            ));
        }
        Ok(Self::ExactTool {
            tool_id,
            descriptor_revision,
        })
    }

    /// Creates one exact MCP method selector.
    ///
    /// # Errors
    ///
    /// Returns the same typed failures as [`ProgrammaticMcpMethodReferenceV1::new`].
    pub fn mcp_method(
        connection_reference: [u8; 16],
        method_reference: impl Into<String>,
        method_schema_revision: impl Into<String>,
    ) -> DtoResult<Self> {
        Ok(Self::McpMethod {
            reference: ProgrammaticMcpMethodReferenceV1::new(
                connection_reference,
                method_reference,
                method_schema_revision,
            )?,
        })
    }

    /// Whether this selector covers the given call.
    #[must_use]
    pub fn matches(&self, call: &ProgrammaticAdmissionCallV1) -> bool {
        match self {
            Self::EffectProfile {
                required_effect_flags,
            } => required_effect_flags
                .iter()
                .all(|flag| call.effect_flags.contains(flag)),
            Self::ExactTool {
                tool_id,
                descriptor_revision,
            } => call.tool_id == *tool_id && call.descriptor_revision == *descriptor_revision,
            Self::McpMethod { reference } => call.mcp_method.as_ref() == Some(reference),
        }
    }

    /// Whether this selector selects no more than its parent selector.
    ///
    /// A child may require additional declared effects, repeat one exact
    /// selector, or repeat one exact MCP method reference. A different tool,
    /// descriptor revision, or MCP revision is not provably narrower and
    /// therefore widens.
    #[must_use]
    pub fn is_narrower_or_equal_than(&self, parent: &Self) -> bool {
        match (self, parent) {
            (
                Self::EffectProfile {
                    required_effect_flags: child,
                },
                Self::EffectProfile {
                    required_effect_flags: parent,
                },
            ) => parent.iter().all(|flag| child.contains(flag)),
            (
                Self::ExactTool {
                    tool_id: child_tool,
                    descriptor_revision: child_revision,
                },
                Self::ExactTool {
                    tool_id: parent_tool,
                    descriptor_revision: parent_revision,
                },
            ) => child_tool == parent_tool && child_revision == parent_revision,
            (Self::McpMethod { reference: child }, Self::McpMethod { reference: parent }) => {
                child == parent
            }
            _ => false,
        }
    }
}

/// Validates that one child selector selects no more than its parent.
///
/// # Errors
///
/// Returns `programmatic_policy_inheritance_widening_forbidden` when the
/// child selector is not provably narrower or equal.
pub fn validate_selector_narrowing(
    parent: &ProgrammaticRuleSelectorV1,
    child: &ProgrammaticRuleSelectorV1,
) -> DtoResult<()> {
    if child.is_narrower_or_equal_than(parent) {
        Ok(())
    } else {
        Err(policy_error(
            PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
            "a child selection cannot add a tool, effect, or revision",
        ))
    }
}

/// The daemon-assigned identity of one programmatic caller context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgrammaticCallContextV1 {
    /// The closed root-origin kind of the root tree.
    pub root_origin: ProgrammaticRootOriginKindV1,
    /// The root session identity of this tree.
    pub root_session_id: [u8; 16],
    /// The root run identity of this tree.
    pub root_run_id: [u8; 16],
    /// The session identity of the current call.
    pub current_session_id: [u8; 16],
    /// The run identity of the current call.
    pub current_run_id: [u8; 16],
    /// The current tool-call identity.
    pub tool_call_id: [u8; 16],
}

impl ProgrammaticCallContextV1 {
    /// Whether this call is executed by the root run itself.
    #[must_use]
    pub const fn is_root_run_call(&self) -> bool {
        same_uuid(self.current_session_id, self.root_session_id)
            && same_uuid(self.current_run_id, self.root_run_id)
    }
}

/// One typed programmatic action considered for admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticAdmissionCallV1 {
    /// The daemon-assigned root and current context.
    pub context: ProgrammaticCallContextV1,
    /// The exact registered tool identity.
    pub tool_id: String,
    /// The selected descriptor revision.
    pub descriptor_revision: u64,
    /// The declared effect flags of the selected descriptor.
    pub effect_flags: Vec<ProgrammaticToolEffectFlagV1>,
    /// The exact selected MCP method reference when the tool is `mcp`.
    pub mcp_method: Option<ProgrammaticMcpMethodReferenceV1>,
    /// The descriptor-selected typed input constraint values of this call.
    pub typed_input_constraints: Vec<DescriptorInputConstraintSelectionV1>,
    /// The typed input digest of this call.
    pub typed_input_digest: Digest256,
}

impl ProgrammaticAdmissionCallV1 {
    /// Creates one typed admission call.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_not_applicable` for a blank tool identity,
    /// a zero descriptor revision, a repeated effect flag, or an MCP method
    /// reference on a non-MCP tool, `programmatic_policy_limit_exceeded` for
    /// over-limit effect flags or typed input constraints, and
    /// `credentials_forbidden` for a credential-shaped tool identity.
    pub fn new(
        context: ProgrammaticCallContextV1,
        tool_id: impl Into<String>,
        descriptor_revision: u64,
        effect_flags: Vec<ProgrammaticToolEffectFlagV1>,
        mcp_method: Option<ProgrammaticMcpMethodReferenceV1>,
        typed_input_constraints: Vec<DescriptorInputConstraintSelectionV1>,
        typed_input_digest: Digest256,
    ) -> DtoResult<Self> {
        let tool_id = tool_id.into();
        validate_policy_text(&tool_id)?;
        if descriptor_revision == 0 {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_NOT_APPLICABLE,
                "an admission call requires an exact descriptor revision",
            ));
        }
        if mcp_method.is_some() && tool_id != MCP_TOOL_ID {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_NOT_APPLICABLE,
                "an MCP method reference belongs only to the mcp tool",
            ));
        }
        if effect_flags.len() > MAX_SELECTORS_PER_RULE {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
                "one admission call may declare at most 16 effect flags",
            ));
        }
        for (index, flag) in effect_flags.iter().enumerate() {
            if effect_flags[..index].contains(flag) {
                return Err(policy_error(
                    PROGRAMMATIC_POLICY_NOT_APPLICABLE,
                    "one admission call never repeats a declared effect flag",
                ));
            }
        }
        if typed_input_constraints.len() > MAX_TYPED_INPUT_CONSTRAINTS_PER_RULE {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
                "one admission call may carry at most 16 typed input constraints",
            ));
        }
        Ok(Self {
            context,
            tool_id,
            descriptor_revision,
            effect_flags,
            mcp_method,
            typed_input_constraints,
            typed_input_digest,
        })
    }

    /// Whether this call is executed by the root run itself.
    #[must_use]
    pub const fn is_root_run_call(&self) -> bool {
        self.context.is_root_run_call()
    }
}

/// One rule entry of a durable policy revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticAdmissionRuleEntryV1 {
    /// The closed decision this entry applies to every covered call.
    pub decision: ProgrammaticAdmissionRuleV1,
    /// The selectors whose union covers the calls of this entry.
    pub selectors: Vec<ProgrammaticRuleSelectorV1>,
    /// The descriptor-selected typed input constraints of this rule.
    pub typed_input_constraints: Vec<DescriptorInputConstraintSelectionV1>,
}

impl ProgrammaticAdmissionRuleEntryV1 {
    /// Creates one bounded admission rule entry.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_not_applicable` when no selector is
    /// present and `programmatic_policy_limit_exceeded` when the selector or
    /// typed input constraint count exceeds its code-owned bound.
    pub fn new(
        decision: ProgrammaticAdmissionRuleV1,
        selectors: Vec<ProgrammaticRuleSelectorV1>,
        typed_input_constraints: Vec<DescriptorInputConstraintSelectionV1>,
    ) -> DtoResult<Self> {
        if selectors.is_empty() {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_NOT_APPLICABLE,
                "an admission rule requires at least one selector",
            ));
        }
        if selectors.len() > MAX_SELECTORS_PER_RULE
            || typed_input_constraints.len() > MAX_TYPED_INPUT_CONSTRAINTS_PER_RULE
        {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
                "one admission rule stays inside its closed selector and constraint bounds",
            ));
        }
        Ok(Self {
            decision,
            selectors,
            typed_input_constraints,
        })
    }

    /// Whether at least one selector of this entry covers the given call.
    #[must_use]
    pub fn matches(&self, call: &ProgrammaticAdmissionCallV1) -> bool {
        self.selectors.iter().any(|selector| selector.matches(call))
    }

    /// The effective decision of this entry for one root origin kind.
    ///
    /// `DirectLocalRead` is interactive-only; a harness-rooted call can never
    /// take it and stays closed instead.
    #[must_use]
    pub const fn decision_for(
        &self,
        root_origin_kind: ProgrammaticRootOriginKindV1,
    ) -> ProgrammaticAdmissionRuleV1 {
        match (root_origin_kind, self.decision) {
            (
                ProgrammaticRootOriginKindV1::ContinualHarness,
                ProgrammaticAdmissionRuleV1::DirectLocalRead,
            ) => ProgrammaticAdmissionRuleV1::Prohibited,
            (_, decision) => decision,
        }
    }
}

/// One root-origin applicability rule of a policy revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgrammaticRootOriginRuleV1 {
    /// The closed root-origin kind this rule applies to.
    pub root_origin_kind: ProgrammaticRootOriginKindV1,
    /// The most permissive decision any call under this origin may receive.
    pub maximum_decision: ProgrammaticAdmissionRuleV1,
}

impl ProgrammaticRootOriginRuleV1 {
    /// Creates one root-origin applicability rule.
    #[must_use]
    pub const fn new(
        root_origin_kind: ProgrammaticRootOriginKindV1,
        maximum_decision: ProgrammaticAdmissionRuleV1,
    ) -> Self {
        Self {
            root_origin_kind,
            maximum_decision,
        }
    }
}

/// One immutable reference to a policy revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgrammaticCallerPolicyRevisionReferenceV1 {
    /// The policy identity.
    pub policy_id: [u8; 16],
    /// The exact immutable revision.
    pub revision: u64,
}

/// One immutable revision of a durable programmatic-caller policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticCallerPolicyRevisionV1 {
    /// The owning policy identity.
    pub policy_id: [u8; 16],
    /// The immutable revision number, starting at one.
    pub revision: u64,
    /// The root-origin applicability rules.
    pub root_origin_rules: Vec<ProgrammaticRootOriginRuleV1>,
    /// The admission rule entries.
    pub admission_rules: Vec<ProgrammaticAdmissionRuleEntryV1>,
    /// The selected per-run limits.
    pub per_run_limits: ProgrammaticRunLimitsV1,
    /// The selected calendar limit.
    pub calendar_limit: ProgrammaticCalendarLimitV1,
    /// The immutable inherited policy references.
    pub inherited_policy_references: Vec<ProgrammaticCallerPolicyRevisionReferenceV1>,
    /// The deterministic digest of this revision's safe fields.
    pub canonical_revision_digest: Digest256,
}

impl ProgrammaticCallerPolicyRevisionV1 {
    /// Creates one bounded immutable policy revision.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_revision_conflict` for a zero revision,
    /// `programmatic_policy_origin_invalid` for an empty, duplicated, or
    /// over-limit root-origin rule set, and
    /// `programmatic_policy_limit_exceeded` when the admission rule or
    /// inherited reference count exceeds its bound.
    pub fn new(
        policy_id: [u8; 16],
        revision: u64,
        root_origin_rules: Vec<ProgrammaticRootOriginRuleV1>,
        admission_rules: Vec<ProgrammaticAdmissionRuleEntryV1>,
        per_run_limits: ProgrammaticRunLimitsV1,
        calendar_limit: ProgrammaticCalendarLimitV1,
        inherited_policy_references: Vec<ProgrammaticCallerPolicyRevisionReferenceV1>,
    ) -> DtoResult<Self> {
        if revision == 0 {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_REVISION_CONFLICT,
                "a policy revision starts at revision one",
            ));
        }
        if root_origin_rules.is_empty() || root_origin_rules.len() > 2 {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_ORIGIN_INVALID,
                "a revision declares at most the two closed root origins",
            ));
        }
        for (index, rule) in root_origin_rules.iter().enumerate() {
            if root_origin_rules[..index]
                .iter()
                .any(|other| other.root_origin_kind == rule.root_origin_kind)
            {
                return Err(policy_error(
                    PROGRAMMATIC_POLICY_ORIGIN_INVALID,
                    "a revision declares each closed root origin at most once",
                ));
            }
        }
        if admission_rules.len() > MAX_RULES_PER_POLICY_REVISION
            || inherited_policy_references.len() > MAX_POLICY_REFERENCES_PER_SESSION
        {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
                "a revision stays inside its closed rule and inheritance bounds",
            ));
        }
        let mut revision_record = Self {
            policy_id,
            revision,
            root_origin_rules,
            admission_rules,
            per_run_limits,
            calendar_limit,
            inherited_policy_references,
            canonical_revision_digest: Digest256::sha256(&[]),
        };
        revision_record.canonical_revision_digest = policy_revision_digest(&revision_record);
        Ok(revision_record)
    }
}

/// Recomputes the deterministic digest of one policy revision.
#[must_use]
pub fn policy_revision_digest(revision: &ProgrammaticCallerPolicyRevisionV1) -> Digest256 {
    let mut input = Vec::new();
    push_text(&mut input, "programmatic-caller-policy-revision-v1");
    push_uuid(&mut input, revision.policy_id);
    push_u64(&mut input, revision.revision);
    push_origin_rules(&mut input, &revision.root_origin_rules);
    push_count(&mut input, revision.admission_rules.len());
    for rule in &revision.admission_rules {
        push_rule(&mut input, rule);
    }
    push_u64(&mut input, revision.per_run_limits.max_actions);
    push_u64(&mut input, revision.per_run_limits.max_concurrent_actions);
    push_field(&mut input, &[revision.calendar_limit.period_kind.code()]);
    push_u64(&mut input, revision.calendar_limit.max_actions);
    push_count(&mut input, revision.inherited_policy_references.len());
    for reference in &revision.inherited_policy_references {
        push_uuid(&mut input, reference.policy_id);
        push_u64(&mut input, reference.revision);
    }
    Digest256::sha256(&input)
}

/// Validates that one child revision intersection narrows its parent.
///
/// # Errors
///
/// Returns `programmatic_policy_revision_conflict` when the child calendar
/// changes the immutable period kind and
/// `programmatic_policy_inheritance_widening_forbidden` when a run limit, the
/// calendar limit, or the decision ceiling widens.
pub fn validate_revision_narrowing(
    parent_per_run_limits: ProgrammaticRunLimitsV1,
    parent_calendar_limit: &ProgrammaticCalendarLimitV1,
    parent_decision_ceiling: ProgrammaticAdmissionRuleV1,
    child_per_run_limits: ProgrammaticRunLimitsV1,
    child_calendar_limit: &ProgrammaticCalendarLimitV1,
    child_decision_ceiling: ProgrammaticAdmissionRuleV1,
) -> DtoResult<()> {
    if child_per_run_limits.max_actions > parent_per_run_limits.max_actions
        || child_per_run_limits.max_concurrent_actions
            > parent_per_run_limits.max_concurrent_actions
    {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
            "a child revision may only narrow the parent run limits",
        ));
    }
    validate_calendar_revision(parent_calendar_limit, child_calendar_limit)?;
    if child_decision_ceiling.restrictiveness() < parent_decision_ceiling.restrictiveness() {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
            "a child revision may only add restriction to the parent decision",
        ));
    }
    Ok(())
}

/// One durable programmatic-caller policy identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgrammaticCallerPolicyV1 {
    /// The daemon-assigned policy identity.
    pub policy_id: [u8; 16],
    /// The immutable owner scope.
    pub scope: ProgrammaticCallerPolicyScopeV1,
    /// The immutable calendar period kind selected at creation.
    pub calendar_period_kind: ProgrammaticCalendarPeriodKindV1,
    /// The closed lifecycle state.
    pub lifecycle_state: ProgrammaticCallerPolicyLifecycleStateV1,
    /// The current active immutable revision.
    pub active_revision: u64,
    /// The deterministic digest of this policy's safe identity.
    pub canonical_policy_digest: Digest256,
}

impl ProgrammaticCallerPolicyV1 {
    /// Creates one active policy identity.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_revision_conflict` when the active
    /// revision is zero.
    pub fn new(
        policy_id: [u8; 16],
        scope: ProgrammaticCallerPolicyScopeV1,
        calendar_period_kind: ProgrammaticCalendarPeriodKindV1,
        active_revision: u64,
    ) -> DtoResult<Self> {
        if active_revision == 0 {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_REVISION_CONFLICT,
                "a policy starts at active revision one",
            ));
        }
        Ok(Self {
            policy_id,
            scope,
            calendar_period_kind,
            lifecycle_state: ProgrammaticCallerPolicyLifecycleStateV1::Active,
            active_revision,
            canonical_policy_digest: policy_identity_digest(
                policy_id,
                &scope,
                calendar_period_kind,
                active_revision,
            ),
        })
    }
}

/// Recomputes the deterministic digest of one policy identity.
#[must_use]
pub fn policy_identity_digest(
    policy_id: [u8; 16],
    scope: &ProgrammaticCallerPolicyScopeV1,
    calendar_period_kind: ProgrammaticCalendarPeriodKindV1,
    active_revision: u64,
) -> Digest256 {
    let mut input = Vec::new();
    push_text(&mut input, "programmatic-caller-policy-identity-v1");
    push_uuid(&mut input, policy_id);
    push_scope(&mut input, scope);
    push_field(&mut input, &[calendar_period_kind.code()]);
    push_u64(&mut input, active_revision);
    Digest256::sha256(&input)
}

/// One applicable policy selected for one root origin at admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticCallerApplicablePolicyV1 {
    /// The policy identity.
    pub policy_id: [u8; 16],
    /// The exact immutable revision.
    pub revision: u64,
    /// The policy's immutable owner scope.
    pub scope: ProgrammaticCallerPolicyScopeV1,
    /// The selected immutable revision record.
    pub revision_record: ProgrammaticCallerPolicyRevisionV1,
}

impl ProgrammaticCallerApplicablePolicyV1 {
    /// Creates one coherent applicable policy selection.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_revision_conflict` when the revision
    /// record identity does not match the selected policy and revision.
    pub fn new(
        policy_id: [u8; 16],
        revision: u64,
        scope: ProgrammaticCallerPolicyScopeV1,
        revision_record: ProgrammaticCallerPolicyRevisionV1,
    ) -> DtoResult<Self> {
        if revision_record.policy_id != policy_id || revision_record.revision != revision {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_REVISION_CONFLICT,
                "the revision record must be the exact selected policy revision",
            ));
        }
        Ok(Self {
            policy_id,
            revision,
            scope,
            revision_record,
        })
    }
}

/// One calendar counter identity belonging to a policy identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgrammaticCalendarCounterReferenceV1 {
    /// The policy identity that owns the calendar counter.
    pub policy_id: [u8; 16],
    /// The immutable calendar period kind.
    pub period_kind: ProgrammaticCalendarPeriodKindV1,
}

/// The immutable effective policy snapshot recorded alongside one run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveProgrammaticCallerPolicySnapshotV1 {
    /// The resolved root-origin kind.
    pub root_origin_kind: ProgrammaticRootOriginKindV1,
    /// The selected policy revision references in canonical order.
    pub policy_references: Vec<ProgrammaticCallerPolicyRevisionReferenceV1>,
    /// The scope provenance of each selected policy.
    pub scope_provenance: Vec<ProgrammaticCallerPolicyScopeV1>,
    /// The most restrictive selected decision ceiling.
    pub decision_ceiling: ProgrammaticAdmissionRuleV1,
    /// The ordered narrowing result of every selected rule.
    pub ordered_rules: Vec<ProgrammaticAdmissionRuleEntryV1>,
    /// The narrowest selected per-run limits.
    pub run_limits: ProgrammaticRunLimitsV1,
    /// The selected calendar limits in canonical policy order.
    pub calendar_limits: Vec<ProgrammaticCalendarLimitV1>,
    /// The calendar counter identities of every selected policy.
    pub calendar_counter_references: Vec<ProgrammaticCalendarCounterReferenceV1>,
    /// The code-owned interactive baseline when the origin is interactive.
    pub baseline: Option<InteractiveLocalReadBaselineV1>,
    /// The deterministic digest of this snapshot's safe fields.
    pub snapshot_digest: Digest256,
}

/// Resolves the immutable effective policy snapshot for one root origin.
///
/// Applied policies intersect by taking the most restrictive decision, the
/// smallest run and calendar bounds, one calendar counter identity per policy
/// identity, and the code-owned interactive baseline as the interactive
/// floor. A policy-less or origin-less `InteractiveUser` resolution receives
/// the narrow direct-local-read baseline; a `ContinualHarness` resolution
/// without an applicable policy fails closed.
///
/// # Errors
///
/// Returns `programmatic_policy_snapshot_unavailable` when no selected policy
/// covers the requested root origin and the origin has no code-owned
/// baseline.
pub fn resolve_effective_policy_snapshot(
    root_origin_kind: ProgrammaticRootOriginKindV1,
    applicable: &[ProgrammaticCallerApplicablePolicyV1],
) -> DtoResult<EffectiveProgrammaticCallerPolicySnapshotV1> {
    let mut usable: Vec<&ProgrammaticCallerApplicablePolicyV1> = applicable
        .iter()
        .filter(|policy| {
            policy
                .revision_record
                .root_origin_rules
                .iter()
                .any(|rule| rule.root_origin_kind == root_origin_kind)
        })
        .collect();
    usable.sort_by_key(|policy| policy.policy_id);

    if usable.is_empty() {
        if root_origin_kind == ProgrammaticRootOriginKindV1::ContinualHarness {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_SNAPSHOT_UNAVAILABLE,
                "a harness root needs one separately selected applicable policy",
            ));
        }
        let baseline = InteractiveLocalReadBaselineV1::v1();
        let mut snapshot = EffectiveProgrammaticCallerPolicySnapshotV1 {
            root_origin_kind,
            policy_references: Vec::new(),
            scope_provenance: Vec::new(),
            decision_ceiling: ProgrammaticAdmissionRuleV1::DirectLocalRead,
            ordered_rules: Vec::new(),
            run_limits: baseline.as_run_limits(),
            calendar_limits: Vec::new(),
            calendar_counter_references: Vec::new(),
            baseline: Some(baseline),
            snapshot_digest: Digest256::sha256(&[]),
        };
        snapshot.snapshot_digest = effective_policy_snapshot_digest(&snapshot);
        return Ok(snapshot);
    }

    let mut decision_ceiling: Option<ProgrammaticAdmissionRuleV1> = None;
    let mut run_limits = ProgrammaticRunLimitsV1::maximum_v1();
    let mut policy_references = Vec::new();
    let mut scope_provenance = Vec::new();
    let mut ordered_rules = Vec::new();
    let mut calendar_limits = Vec::new();
    let mut calendar_counter_references = Vec::new();
    for policy in &usable {
        for rule in &policy.revision_record.root_origin_rules {
            if rule.root_origin_kind == root_origin_kind {
                decision_ceiling =
                    Some(decision_ceiling.map_or(rule.maximum_decision, |current| {
                        current.most_restrictive(rule.maximum_decision)
                    }));
            }
        }
        run_limits = run_limits.narrowed(policy.revision_record.per_run_limits);
        policy_references.push(ProgrammaticCallerPolicyRevisionReferenceV1 {
            policy_id: policy.policy_id,
            revision: policy.revision,
        });
        scope_provenance.push(policy.scope);
        ordered_rules.extend(policy.revision_record.admission_rules.iter().cloned());
        calendar_limits.push(policy.revision_record.calendar_limit);
        calendar_counter_references.push(ProgrammaticCalendarCounterReferenceV1 {
            policy_id: policy.policy_id,
            period_kind: policy.revision_record.calendar_limit.period_kind,
        });
    }
    let Some(decision_ceiling) = decision_ceiling else {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_SNAPSHOT_UNAVAILABLE,
            "no selected policy covers the requested root origin",
        ));
    };
    let baseline = if root_origin_kind == ProgrammaticRootOriginKindV1::InteractiveUser {
        let baseline = InteractiveLocalReadBaselineV1::v1();
        run_limits = run_limits.narrowed(baseline.as_run_limits());
        Some(baseline)
    } else {
        None
    };
    let mut snapshot = EffectiveProgrammaticCallerPolicySnapshotV1 {
        root_origin_kind,
        policy_references,
        scope_provenance,
        decision_ceiling,
        ordered_rules,
        run_limits,
        calendar_limits,
        calendar_counter_references,
        baseline,
        snapshot_digest: Digest256::sha256(&[]),
    };
    snapshot.snapshot_digest = effective_policy_snapshot_digest(&snapshot);
    Ok(snapshot)
}

/// Recomputes the deterministic digest of one effective policy snapshot.
#[must_use]
pub fn effective_policy_snapshot_digest(
    snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
) -> Digest256 {
    let mut input = Vec::new();
    push_text(&mut input, "programmatic-caller-policy-snapshot-v1");
    push_field(&mut input, &[snapshot.root_origin_kind.code()]);
    push_count(&mut input, snapshot.policy_references.len());
    for reference in &snapshot.policy_references {
        push_uuid(&mut input, reference.policy_id);
        push_u64(&mut input, reference.revision);
    }
    push_count(&mut input, snapshot.scope_provenance.len());
    for scope in &snapshot.scope_provenance {
        push_scope(&mut input, scope);
    }
    push_field(&mut input, &[snapshot.decision_ceiling.restrictiveness()]);
    push_count(&mut input, snapshot.ordered_rules.len());
    for rule in &snapshot.ordered_rules {
        push_rule(&mut input, rule);
    }
    push_u64(&mut input, snapshot.run_limits.max_actions);
    push_u64(&mut input, snapshot.run_limits.max_concurrent_actions);
    push_count(&mut input, snapshot.calendar_limits.len());
    for limit in &snapshot.calendar_limits {
        push_field(&mut input, &[limit.period_kind.code()]);
        push_u64(&mut input, limit.max_actions);
    }
    push_count(&mut input, snapshot.calendar_counter_references.len());
    for counter in &snapshot.calendar_counter_references {
        push_uuid(&mut input, counter.policy_id);
        push_field(&mut input, &[counter.period_kind.code()]);
    }
    match snapshot.baseline {
        Some(baseline) => {
            push_field(&mut input, &[1]);
            push_u64(&mut input, baseline.maximum_action_count);
            push_u64(&mut input, baseline.maximum_concurrent_actions);
        }
        None => push_field(&mut input, &[0]),
    }
    Digest256::sha256(&input)
}

/// Returns the narrowest selected calendar action limit of one snapshot.
#[must_use]
pub fn effective_calendar_action_limit(
    snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
) -> Option<u64> {
    snapshot
        .calendar_limits
        .iter()
        .map(|limit| limit.max_actions)
        .min()
}

/// Validates the encoded size of one effective policy snapshot.
///
/// # Errors
///
/// Returns `programmatic_policy_snapshot_too_large` when the encoded snapshot
/// exceeds 1 MiB.
pub fn validate_effective_policy_snapshot_size(encoded_bytes: u64) -> DtoResult<()> {
    if encoded_bytes > MAX_EFFECTIVE_POLICY_SNAPSHOT_BYTES {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_SNAPSHOT_TOO_LARGE,
            "one effective policy snapshot must not exceed 1 MiB",
        ));
    }
    Ok(())
}

/// Validates the encoded size of one policy, revision, corridor, draft, or
/// safe admission evidence record.
///
/// # Errors
///
/// Returns `programmatic_policy_limit_exceeded` when the encoded record
/// exceeds 512 KiB.
pub fn validate_policy_record_size(encoded_bytes: u64) -> DtoResult<()> {
    if encoded_bytes > MAX_POLICY_RECORD_BYTES {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
            "one policy record must not exceed 512 KiB",
        ));
    }
    Ok(())
}

/// Validates the encoded size of one policy draft.
///
/// # Errors
///
/// Returns `programmatic_policy_draft_too_large` when the encoded draft
/// exceeds 512 KiB.
pub fn validate_draft_size(encoded_bytes: u64) -> DtoResult<()> {
    if encoded_bytes > MAX_POLICY_RECORD_BYTES {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_DRAFT_TOO_LARGE,
            "one policy draft must not exceed 512 KiB",
        ));
    }
    Ok(())
}

/// Resolves the effective decision of one call under one snapshot.
///
/// The result is the most restrictive of the code-owned baseline (interactive
/// root-run reads only), the snapshot decision ceiling, and every matching
/// rule. A call no selected rule covers stays `Prohibited`.
#[must_use]
pub fn resolve_snapshot_decision(
    snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
    call: &ProgrammaticAdmissionCallV1,
) -> ProgrammaticAdmissionRuleV1 {
    let mut decision = if snapshot.baseline.is_some()
        && call.context.root_origin == ProgrammaticRootOriginKindV1::InteractiveUser
        && call.context.is_root_run_call()
        && DIRECT_LOCAL_READ_TOOL_IDS.contains(&call.tool_id.as_str())
    {
        ProgrammaticAdmissionRuleV1::DirectLocalRead
    } else {
        ProgrammaticAdmissionRuleV1::Prohibited
    };
    decision = decision.most_restrictive(snapshot.decision_ceiling);
    for rule in &snapshot.ordered_rules {
        if rule.matches(call) {
            decision = decision.most_restrictive(rule.decision_for(call.context.root_origin));
        }
    }
    decision
}

/// Validates the closed direct-local-read decision for one tool and origin.
///
/// # Errors
///
/// Returns `programmatic_policy_origin_invalid` for a harness root and
/// `programmatic_policy_not_applicable` for a tool outside the closed direct
/// local-read set.
pub fn validate_direct_local_read_tool(
    root_origin_kind: ProgrammaticRootOriginKindV1,
    tool_id: &str,
) -> DtoResult<()> {
    if root_origin_kind == ProgrammaticRootOriginKindV1::ContinualHarness {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_ORIGIN_INVALID,
            "direct local read is valid only for the interactive user root",
        ));
    }
    if DIRECT_LOCAL_READ_TOOL_IDS.contains(&tool_id) {
        Ok(())
    } else {
        Err(policy_error(
            PROGRAMMATIC_POLICY_NOT_APPLICABLE,
            "the direct-local-read decision covers only read, glob, grep, expand, and retrieve",
        ))
    }
}

/// Validates that one tool may participate in a bounded confirmation corridor.
///
/// `execute` never participates in a bounded corridor, and every other
/// admitted tool needs a descriptor-declared closed typed input constraint
/// family.
///
/// # Errors
///
/// Returns `programmatic_policy_not_applicable` for `execute` and
/// `programmatic_policy_input_constraint_mismatch` when the selected
/// descriptor declares no compatible typed input constraint family.
pub fn validate_bounded_corridor_admission(
    tool_id: &str,
    has_descriptor_constraint_family: bool,
) -> DtoResult<()> {
    if tool_id == EXECUTE_TOOL_ID {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_NOT_APPLICABLE,
            "execute never receives a direct or bounded-corridor admission",
        ));
    }
    if has_descriptor_constraint_family {
        Ok(())
    } else {
        Err(policy_error(
            PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH,
            "a bounded corridor needs a descriptor-declared typed input constraint family",
        ))
    }
}

/// Validates the `ask_user` root-only interaction rule.
///
/// Only the `InteractiveUser` root run may start `ask_user`; a descendant
/// fails with [`PROGRAMMATIC_POLICY_ROOT_ONLY_INTERACTION`] and a continual
/// harness never awaits user interaction.
///
/// # Errors
///
/// Returns `programmatic_policy_root_only_interaction` for a descendant and
/// `programmatic_policy_harness_delegation_forbidden` for a harness root.
pub fn validate_interaction_admission(
    root_origin_kind: ProgrammaticRootOriginKindV1,
    tool_id: &str,
    is_root_run_call: bool,
) -> DtoResult<()> {
    if tool_id != USER_INTERACTION_TOOL_ID {
        return Ok(());
    }
    match root_origin_kind {
        ProgrammaticRootOriginKindV1::ContinualHarness => Err(policy_error(
            PROGRAMMATIC_POLICY_HARNESS_DELEGATION_FORBIDDEN,
            "a continual harness never calls ask_user",
        )),
        ProgrammaticRootOriginKindV1::InteractiveUser if is_root_run_call => Ok(()),
        ProgrammaticRootOriginKindV1::InteractiveUser => Err(policy_error(
            PROGRAMMATIC_POLICY_ROOT_ONLY_INTERACTION,
            "only the root run starts the user interaction",
        )),
    }
}

/// Validates one harness delegation against its user-approved corridor.
///
/// The separately issued ordinary and Mandate direct admission paths stay
/// ungated by this rule (CON-071).
///
/// # Errors
///
/// Returns `programmatic_policy_harness_delegation_forbidden` when a harness
/// `sub_agent` does not fit an already selected user-approved corridor.
pub fn validate_harness_delegation(
    root_origin_kind: ProgrammaticRootOriginKindV1,
    tool_id: &str,
    corridor_approved: bool,
) -> DtoResult<()> {
    if root_origin_kind == ProgrammaticRootOriginKindV1::ContinualHarness
        && tool_id == HARNESS_DELEGATION_TOOL_ID
        && !corridor_approved
    {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_HARNESS_DELEGATION_FORBIDDEN,
            "harness child admission must fit the user-approved corridor",
        ));
    }
    Ok(())
}

/// One exact confirmation binding of one bound call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticConfirmationBindingV1 {
    /// The root session identity of the confirmed tree.
    pub root_session_id: [u8; 16],
    /// The root run identity of the confirmed tree.
    pub root_run_id: [u8; 16],
    /// The one confirmed tool-call identity.
    pub tool_call_id: [u8; 16],
    /// The exact confirmed tool identity.
    pub tool_id: String,
    /// The exact confirmed descriptor revision.
    pub descriptor_revision: u64,
    /// The exact confirmed MCP method reference, when present.
    pub mcp_method: Option<ProgrammaticMcpMethodReferenceV1>,
    /// The exact confirmed typed input digest.
    pub typed_input_digest: Digest256,
    /// The confirmed effective policy snapshot digest.
    pub policy_snapshot_digest: Digest256,
    /// The confirmed selected run limits.
    pub run_limits: ProgrammaticRunLimitsV1,
}

impl ProgrammaticConfirmationBindingV1 {
    /// Derives the single exact binding of one call under one snapshot.
    #[must_use]
    pub fn from_call(
        call: &ProgrammaticAdmissionCallV1,
        policy_snapshot_digest: Digest256,
        run_limits: ProgrammaticRunLimitsV1,
    ) -> Self {
        Self {
            root_session_id: call.context.root_session_id,
            root_run_id: call.context.root_run_id,
            tool_call_id: call.context.tool_call_id,
            tool_id: call.tool_id.clone(),
            descriptor_revision: call.descriptor_revision,
            mcp_method: call.mcp_method.clone(),
            typed_input_digest: call.typed_input_digest,
            policy_snapshot_digest,
            run_limits,
        }
    }
}

/// One durable exact user confirmation.
///
/// It is bound to exactly one tool call, root tree, tool, descriptor
/// revision, optional MCP method reference, typed input digest, policy
/// snapshot digest, and selected run limits. It cannot be replayed for
/// another call, input, descendant tree, revision, or later run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticExactConfirmationV1 {
    /// The durable confirmation identity.
    pub confirmation_reference: [u8; 16],
    /// The one exact bound call.
    pub binding: ProgrammaticConfirmationBindingV1,
    /// The confirmation time in Unix milliseconds.
    pub confirmed_at_ms: u64,
    /// The confirmation expiry time in Unix milliseconds.
    pub expires_at_ms: u64,
}

impl ProgrammaticExactConfirmationV1 {
    /// Creates one exact confirmation with a non-empty validity window.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_confirmation_expired` when the expiry is
    /// not later than the confirmation time.
    pub fn new(
        confirmation_reference: [u8; 16],
        binding: ProgrammaticConfirmationBindingV1,
        confirmed_at_ms: u64,
        expires_at_ms: u64,
    ) -> DtoResult<Self> {
        if expires_at_ms <= confirmed_at_ms {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_CONFIRMATION_EXPIRED,
                "one exact confirmation needs a positive validity window",
            ));
        }
        Ok(Self {
            confirmation_reference,
            binding,
            confirmed_at_ms,
            expires_at_ms,
        })
    }

    /// Whether this confirmation carries exactly the given binding.
    #[must_use]
    pub fn matches_binding(&self, binding: &ProgrammaticConfirmationBindingV1) -> bool {
        self.binding == *binding
    }

    /// Validates this confirmation for one call at one time.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_confirmation_required` when the call is
    /// not the one exact bound call and
    /// `programmatic_policy_confirmation_expired` when the call time is
    /// outside the confirmation's validity window.
    pub fn authorizes(
        &self,
        call: &ProgrammaticAdmissionCallV1,
        snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
        now_ms: u64,
    ) -> DtoResult<()> {
        let expected = ProgrammaticConfirmationBindingV1::from_call(
            call,
            snapshot.snapshot_digest,
            snapshot.run_limits,
        );
        if self.binding != expected {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_CONFIRMATION_REQUIRED,
                "the exact confirmation binds only its one exact call",
            ));
        }
        if now_ms < self.confirmed_at_ms || now_ms > self.expires_at_ms {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_CONFIRMATION_EXPIRED,
                "the exact confirmation is outside its validity window",
            ));
        }
        Ok(())
    }
}

/// The active root tree of one bounded confirmation corridor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgrammaticCorridorRootV1 {
    /// The one active root session identity.
    pub root_session_id: [u8; 16],
    /// The one active root run identity.
    pub root_run_id: [u8; 16],
    /// The root-origin kind of the active tree.
    pub root_origin_kind: ProgrammaticRootOriginKindV1,
    /// The effective policy snapshot digest selected by the root.
    pub effective_policy_snapshot_digest: Digest256,
}

/// The bounded selectors of one user-approved corridor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticCorridorSelectorsV1 {
    /// The required declared effect flags of every covered call.
    pub required_effect_selectors: Vec<ProgrammaticToolEffectFlagV1>,
    /// The exact tool or MCP method selectors of every covered call.
    pub exact_tool_or_mcp_method_selectors: Vec<ProgrammaticRuleSelectorV1>,
    /// The descriptor-selected typed input constraints of every covered call.
    pub descriptor_input_constraint_selections: Vec<DescriptorInputConstraintSelectionV1>,
}

impl ProgrammaticCorridorSelectorsV1 {
    /// Creates one bounded corridor selector set.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_corridor_unavailable` when no selector is
    /// present and `programmatic_policy_limit_exceeded` when a selector or
    /// constraint count exceeds its code-owned bound.
    pub fn new(
        required_effect_selectors: Vec<ProgrammaticToolEffectFlagV1>,
        exact_tool_or_mcp_method_selectors: Vec<ProgrammaticRuleSelectorV1>,
        descriptor_input_constraint_selections: Vec<DescriptorInputConstraintSelectionV1>,
    ) -> DtoResult<Self> {
        if required_effect_selectors.is_empty() && exact_tool_or_mcp_method_selectors.is_empty() {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE,
                "a corridor must select at least one effect or exact tool",
            ));
        }
        if required_effect_selectors.len() > MAX_SELECTORS_PER_RULE
            || exact_tool_or_mcp_method_selectors.len() > MAX_SELECTORS_PER_RULE
            || descriptor_input_constraint_selections.len() > MAX_TYPED_INPUT_CONSTRAINTS_PER_RULE
        {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
                "a corridor stays inside its closed selector and constraint bounds",
            ));
        }
        for (index, flag) in required_effect_selectors.iter().enumerate() {
            if required_effect_selectors[..index].contains(flag) {
                return Err(policy_error(
                    PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE,
                    "a corridor never repeats one required effect flag",
                ));
            }
        }
        Ok(Self {
            required_effect_selectors,
            exact_tool_or_mcp_method_selectors,
            descriptor_input_constraint_selections,
        })
    }
}

/// One user-approved bounded authorization corridor.
///
/// It belongs to one active root tree and reaches every descendant in that
/// tree. No child can copy, widen, detach, retain, or apply it to a sibling
/// root, another harness launch, or a later run, and it always expires at run
/// terminalization without any post-hoc extension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticAuthorizationCorridorV1 {
    /// The one active root session identity.
    pub root_session_id: [u8; 16],
    /// The one active root run identity.
    pub root_run_id: [u8; 16],
    /// The root-origin kind of the active tree.
    pub root_origin_kind: ProgrammaticRootOriginKindV1,
    /// The effective policy snapshot digest selected by the root.
    pub effective_policy_snapshot_digest: Digest256,
    /// The required declared effect flags of every covered call.
    pub required_effect_selectors: Vec<ProgrammaticToolEffectFlagV1>,
    /// The exact tool or MCP method selectors of every covered call.
    pub exact_tool_or_mcp_method_selectors: Vec<ProgrammaticRuleSelectorV1>,
    /// The descriptor-selected typed input constraints of every covered call.
    pub descriptor_input_constraint_selections: Vec<DescriptorInputConstraintSelectionV1>,
    /// The shared maximum action count.
    pub maximum_action_count: u64,
    /// The shared maximum concurrent actions.
    pub maximum_concurrent_actions: u64,
    /// The closed root-terminal expiry flag; always true in version one.
    pub expires_at_run_terminal: bool,
    /// The user confirmation that approved this corridor.
    pub confirmation_reference: [u8; 16],
    /// The deterministic digest of this corridor's safe fields.
    pub canonical_corridor_digest: Digest256,
}

impl ProgrammaticAuthorizationCorridorV1 {
    /// Creates one bounded corridor that expires at run terminalization.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_limit_exceeded` when a maximum is zero or
    /// exceeds the closed 256/16 bound.
    pub fn new(
        root: ProgrammaticCorridorRootV1,
        selectors: ProgrammaticCorridorSelectorsV1,
        maximum_action_count: u64,
        maximum_concurrent_actions: u64,
        confirmation_reference: [u8; 16],
    ) -> DtoResult<Self> {
        if maximum_action_count == 0
            || maximum_action_count > MAX_POLICY_ACTIONS_PER_RUN
            || maximum_concurrent_actions == 0
            || maximum_concurrent_actions > MAX_POLICY_CONCURRENT_ACTIONS_PER_RUN
        {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
                "a corridor stays inside the closed 256/16 action bound",
            ));
        }
        let mut corridor = Self {
            root_session_id: root.root_session_id,
            root_run_id: root.root_run_id,
            root_origin_kind: root.root_origin_kind,
            effective_policy_snapshot_digest: root.effective_policy_snapshot_digest,
            required_effect_selectors: selectors.required_effect_selectors,
            exact_tool_or_mcp_method_selectors: selectors.exact_tool_or_mcp_method_selectors,
            descriptor_input_constraint_selections: selectors
                .descriptor_input_constraint_selections,
            maximum_action_count,
            maximum_concurrent_actions,
            expires_at_run_terminal: true,
            confirmation_reference,
            canonical_corridor_digest: Digest256::sha256(&[]),
        };
        corridor.canonical_corridor_digest = corridor_digest(&corridor);
        Ok(corridor)
    }
}

/// Recomputes the deterministic digest of one corridor.
fn corridor_digest(corridor: &ProgrammaticAuthorizationCorridorV1) -> Digest256 {
    let mut input = Vec::new();
    push_text(&mut input, "programmatic-caller-corridor-v1");
    push_uuid(&mut input, corridor.root_session_id);
    push_uuid(&mut input, corridor.root_run_id);
    push_field(&mut input, &[corridor.root_origin_kind.code()]);
    push_field(
        &mut input,
        corridor.effective_policy_snapshot_digest.bytes().as_slice(),
    );
    push_count(&mut input, corridor.required_effect_selectors.len());
    for flag in &corridor.required_effect_selectors {
        push_field(&mut input, &[flag.restrictiveness_code()]);
    }
    push_count(
        &mut input,
        corridor.exact_tool_or_mcp_method_selectors.len(),
    );
    for selector in &corridor.exact_tool_or_mcp_method_selectors {
        push_selector(&mut input, selector);
    }
    push_count(
        &mut input,
        corridor.descriptor_input_constraint_selections.len(),
    );
    for constraint in &corridor.descriptor_input_constraint_selections {
        push_constraint(&mut input, constraint);
    }
    push_u64(&mut input, corridor.maximum_action_count);
    push_u64(&mut input, corridor.maximum_concurrent_actions);
    push_field(&mut input, &[u8::from(corridor.expires_at_run_terminal)]);
    push_uuid(&mut input, corridor.confirmation_reference);
    Digest256::sha256(&input)
}

/// Validates whether one corridor is still active.
///
/// # Errors
///
/// Returns `programmatic_policy_corridor_unavailable` when the corridor is
/// not bound to the root run terminal.
pub fn validate_corridor_active(expires_at_run_terminal: bool) -> DtoResult<()> {
    if expires_at_run_terminal {
        Ok(())
    } else {
        Err(policy_error(
            PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE,
            "a corridor expires at root terminalization and never reopens",
        ))
    }
}

/// Validates one proposed corridor extension.
///
/// # Errors
///
/// Returns `programmatic_policy_inheritance_widening_forbidden` when a
/// proposed maximum exceeds the approved corridor maximum.
pub fn validate_corridor_extension(
    corridor: &ProgrammaticAuthorizationCorridorV1,
    proposed_maximum_action_count: u64,
    proposed_maximum_concurrent_actions: u64,
) -> DtoResult<()> {
    if proposed_maximum_action_count > corridor.maximum_action_count
        || proposed_maximum_concurrent_actions > corridor.maximum_concurrent_actions
    {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
            "a corridor never extends its approved bounds after approval",
        ));
    }
    Ok(())
}

/// Validates that one call stays inside one active corridor.
///
/// # Errors
///
/// Returns `programmatic_policy_corridor_unavailable` when the call is
/// outside the corridor's root tree or selectors and
/// `programmatic_policy_input_constraint_mismatch` when a selected typed
/// input constraint is not satisfied.
pub fn validate_corridor_use(
    corridor: &ProgrammaticAuthorizationCorridorV1,
    call: &ProgrammaticAdmissionCallV1,
) -> DtoResult<()> {
    if !same_uuid(call.context.root_session_id, corridor.root_session_id)
        || !same_uuid(call.context.root_run_id, corridor.root_run_id)
        || call.context.root_origin != corridor.root_origin_kind
    {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE,
            "a corridor belongs to one active root tree",
        ));
    }
    for flag in &corridor.required_effect_selectors {
        if !call.effect_flags.contains(flag) {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE,
                "the call does not satisfy the corridor effect selector",
            ));
        }
    }
    if !corridor.exact_tool_or_mcp_method_selectors.is_empty()
        && !corridor
            .exact_tool_or_mcp_method_selectors
            .iter()
            .any(|selector| selector.matches(call))
    {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE,
            "the call does not satisfy any corridor exact selector",
        ));
    }
    for constraint in &corridor.descriptor_input_constraint_selections {
        if !call.typed_input_constraints.contains(constraint) {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH,
                "the call does not satisfy a selected typed input constraint",
            ));
        }
    }
    Ok(())
}

/// Validates that one descendant selector set only narrows its corridor.
///
/// # Errors
///
/// Returns `programmatic_policy_inheritance_widening_forbidden` when the
/// child omits a required corridor effect, adds a selector the corridor does
/// not cover, or adds a typed input constraint the corridor does not select.
pub fn validate_corridor_child_selectors(
    corridor: &ProgrammaticAuthorizationCorridorV1,
    child: &ProgrammaticCorridorSelectorsV1,
) -> DtoResult<()> {
    for flag in &corridor.required_effect_selectors {
        if !child.required_effect_selectors.contains(flag) {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
                "a child may only require the corridor's declared effects",
            ));
        }
    }
    for selector in &child.exact_tool_or_mcp_method_selectors {
        if !corridor
            .exact_tool_or_mcp_method_selectors
            .iter()
            .any(|parent| selector.is_narrower_or_equal_than(parent))
        {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
                "a child may only repeat or narrow a corridor exact selector",
            ));
        }
    }
    for constraint in &child.descriptor_input_constraint_selections {
        if !corridor
            .descriptor_input_constraint_selections
            .iter()
            .any(|parent| constraint_is_narrower_or_equal(constraint, parent))
        {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
                "a child may only narrow a corridor typed input constraint",
            ));
        }
    }
    Ok(())
}

/// Whether one constraint selection selects no more than its parent.
fn constraint_is_narrower_or_equal(
    child: &DescriptorInputConstraintSelectionV1,
    parent: &DescriptorInputConstraintSelectionV1,
) -> bool {
    child.family == parent.family
        && child.revision == parent.revision
        && child
            .typed_values
            .iter()
            .all(|value| parent.typed_values.contains(value))
}

/// The shared remaining allocation of one approved corridor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgrammaticCorridorUsageV1 {
    /// The approved corridor digest this usage belongs to.
    pub corridor_digest: Digest256,
    /// The remaining shared action count.
    pub remaining_actions: u64,
    /// The remaining shared concurrency count.
    pub remaining_concurrent_actions: u64,
}

impl ProgrammaticCorridorUsageV1 {
    /// Creates the full remaining allocation of one approved corridor.
    #[must_use]
    pub const fn new(corridor: &ProgrammaticAuthorizationCorridorV1) -> Self {
        Self {
            corridor_digest: corridor.canonical_corridor_digest,
            remaining_actions: corridor.maximum_action_count,
            remaining_concurrent_actions: corridor.maximum_concurrent_actions,
        }
    }

    /// Reserves one action and one concurrency slot before `ToolCallStarted`.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_corridor_unavailable` for a different
    /// corridor and `programmatic_policy_corridor_exhausted` when the shared
    /// action or concurrency allocation is exhausted.
    pub fn reserve(&mut self, corridor: &ProgrammaticAuthorizationCorridorV1) -> DtoResult<()> {
        self.ensure_corridor(corridor)?;
        if self.remaining_actions == 0 || self.remaining_concurrent_actions == 0 {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_CORRIDOR_EXHAUSTED,
                "the shared corridor allocation is exhausted",
            ));
        }
        self.remaining_actions -= 1;
        self.remaining_concurrent_actions -= 1;
        Ok(())
    }

    /// Releases one known pre-effect reservation of this corridor.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_corridor_unavailable` for a different
    /// corridor and `programmatic_policy_reservation_conflict` when no
    /// matching reservation is outstanding.
    pub fn release_unstarted(
        &mut self,
        corridor: &ProgrammaticAuthorizationCorridorV1,
    ) -> DtoResult<()> {
        self.ensure_corridor(corridor)?;
        if self.remaining_actions >= corridor.maximum_action_count
            || self.remaining_concurrent_actions >= corridor.maximum_concurrent_actions
        {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_RESERVATION_CONFLICT,
                "no unstarted corridor reservation is outstanding",
            ));
        }
        self.remaining_actions += 1;
        self.remaining_concurrent_actions += 1;
        Ok(())
    }

    /// Consumes one started action and releases only its concurrency slot.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_corridor_unavailable` for a different
    /// corridor and `programmatic_policy_reservation_conflict` when no
    /// reservation is outstanding.
    pub fn consume_started(
        &mut self,
        corridor: &ProgrammaticAuthorizationCorridorV1,
    ) -> DtoResult<()> {
        self.ensure_corridor(corridor)?;
        if self.remaining_concurrent_actions >= corridor.maximum_concurrent_actions {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_RESERVATION_CONFLICT,
                "no started corridor reservation is outstanding",
            ));
        }
        self.remaining_concurrent_actions += 1;
        Ok(())
    }

    /// Ensures this usage record belongs to the given corridor.
    fn ensure_corridor(&self, corridor: &ProgrammaticAuthorizationCorridorV1) -> DtoResult<()> {
        if self.corridor_digest == corridor.canonical_corridor_digest {
            Ok(())
        } else {
            Err(policy_error(
                PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE,
                "this allocation belongs to a different corridor",
            ))
        }
    }
}

/// Validates the unfinished corridor bound of one root tree.
///
/// # Errors
///
/// Returns `programmatic_policy_limit_exceeded` when the count exceeds 16.
pub fn validate_unfinished_corridors_per_root_tree(count: usize) -> DtoResult<()> {
    if count > MAX_UNFINISHED_CORRIDORS_PER_ROOT_TREE {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
            "one root tree keeps at most 16 unfinished corridors",
        ));
    }
    Ok(())
}

/// Validates the awaiting confirmation bound of one root tree.
///
/// # Errors
///
/// Returns `programmatic_policy_limit_exceeded` when the count exceeds 16.
pub fn validate_awaiting_confirmations_per_root_tree(count: usize) -> DtoResult<()> {
    if count > MAX_AWAITING_CONFIRMATIONS_PER_ROOT_TREE {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
            "one root tree keeps at most 16 awaiting confirmations",
        ));
    }
    Ok(())
}

/// The closed durable policy lifecycle states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticCallerPolicyLifecycleStateV1 {
    /// The policy admits through its current active revision.
    Active,
    /// A live suspension denies every not-yet-started matching call.
    Suspended,
    /// The policy is revoked and denies every new admission.
    Revoked,
    /// A revoked policy is retained for readable history only.
    Archived,
}

impl ProgrammaticCallerPolicyLifecycleStateV1 {
    /// Whether the closed lifecycle permits this transition.
    #[must_use]
    const fn permits_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Active, Self::Suspended)
                | (Self::Suspended, Self::Active)
                | (Self::Active, Self::Revoked)
                | (Self::Suspended, Self::Revoked)
                | (Self::Revoked, Self::Archived)
        )
    }

    /// Whether this state is the revoked terminal state.
    #[must_use]
    const fn is_revoked(self) -> bool {
        matches!(self, Self::Revoked)
    }
}

/// Validates one closed durable policy lifecycle transition.
///
/// # Errors
///
/// Returns `programmatic_policy_revoked` for any transition out of
/// `Revoked` and `programmatic_policy_not_applicable` for every other
/// undeclared transition, including archiving a not-yet-revoked policy.
pub fn validate_policy_lifecycle_transition(
    from: ProgrammaticCallerPolicyLifecycleStateV1,
    to: ProgrammaticCallerPolicyLifecycleStateV1,
) -> DtoResult<()> {
    if from.permits_transition_to(to) {
        return Ok(());
    }
    if from.is_revoked() {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_REVOKED,
            "revocation is never undone by reactivating or restoring a policy",
        ));
    }
    Err(policy_error(
        PROGRAMMATIC_POLICY_NOT_APPLICABLE,
        "the requested policy lifecycle transition is not declared",
    ))
}

/// Validates one live policy state against one not-yet-started call.
///
/// A live suspension or revocation takes effect without restart and blocks
/// every not-yet-started matching call. An action that already reached
/// `ToolCallStarted` is never rolled back here; its independently selected
/// recovery evidence owns the outcome.
///
/// # Errors
///
/// Returns `programmatic_policy_suspended`, `programmatic_policy_revoked`, or
/// `programmatic_policy_not_applicable` for a denied present-time state.
pub fn validate_live_policy_state(
    lifecycle_state: ProgrammaticCallerPolicyLifecycleStateV1,
    tool_call_started: bool,
) -> DtoResult<()> {
    if tool_call_started {
        return Ok(());
    }
    match lifecycle_state {
        ProgrammaticCallerPolicyLifecycleStateV1::Active => Ok(()),
        ProgrammaticCallerPolicyLifecycleStateV1::Suspended => Err(policy_error(
            PROGRAMMATIC_POLICY_SUSPENDED,
            "a suspended policy denies every not-yet-started matching call",
        )),
        ProgrammaticCallerPolicyLifecycleStateV1::Revoked => Err(policy_error(
            PROGRAMMATIC_POLICY_REVOKED,
            "a revoked policy denies every new admission",
        )),
        ProgrammaticCallerPolicyLifecycleStateV1::Archived => Err(policy_error(
            PROGRAMMATIC_POLICY_NOT_APPLICABLE,
            "an archived policy grants no authority",
        )),
    }
}

/// Validates one live tightening decision.
///
/// # Errors
///
/// Returns `programmatic_policy_inheritance_widening_forbidden` when the
/// tightened decision is weaker than the current decision.
pub fn tighten_admission_rule(
    current: ProgrammaticAdmissionRuleV1,
    tightened: ProgrammaticAdmissionRuleV1,
) -> DtoResult<ProgrammaticAdmissionRuleV1> {
    if tightened.restrictiveness() >= current.restrictiveness() {
        Ok(tightened)
    } else {
        Err(policy_error(
            PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
            "live tightening may only add restriction",
        ))
    }
}

/// The proposed content of one inactive policy draft.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticPolicyDraftContentV1 {
    /// The proposed root-origin applicability rules.
    pub root_origin_rules: Vec<ProgrammaticRootOriginRuleV1>,
    /// The proposed admission rule entries.
    pub admission_rules: Vec<ProgrammaticAdmissionRuleEntryV1>,
    /// The proposed per-run limits.
    pub run_limits: ProgrammaticRunLimitsV1,
    /// The proposed calendar limit.
    pub calendar_limit: ProgrammaticCalendarLimitV1,
}

/// One inactive programmatic-caller policy draft.
///
/// A draft is not a policy, card, target-snapshot input, corridor,
/// confirmation, tool selection, or authority. The daemon records it before
/// the root-only user question; the user may accept, edit and accept, or
/// reject it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticCallerPolicyDraftV1 {
    /// The daemon-assigned draft identity.
    pub draft_reference: [u8; 16],
    /// The proposed owner scope.
    pub scope: ProgrammaticCallerPolicyScopeV1,
    /// The exact base policy revision the draft was proposed against.
    pub base_policy_revision: Option<ProgrammaticCallerPolicyRevisionReferenceV1>,
    /// The proposed root-origin applicability rules.
    pub root_origin_rules: Vec<ProgrammaticRootOriginRuleV1>,
    /// The proposed admission rule entries.
    pub admission_rules: Vec<ProgrammaticAdmissionRuleEntryV1>,
    /// The proposed per-run limits.
    pub run_limits: ProgrammaticRunLimitsV1,
    /// The proposed calendar limit.
    pub calendar_limit: ProgrammaticCalendarLimitV1,
    /// The digest of the safe rationale.
    pub rationale_digest: Digest256,
    /// The deterministic digest of this draft's proposed content.
    pub canonical_draft_digest: Digest256,
}

impl ProgrammaticCallerPolicyDraftV1 {
    /// Creates one bounded inactive policy draft.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_origin_invalid` for an empty, duplicated,
    /// or over-limit root-origin rule set, `programmatic_policy_limit_exceeded`
    /// for an over-limit admission rule set or invalid rationale, and
    /// `credentials_forbidden` for a credential-shaped rationale.
    pub fn new(
        draft_reference: [u8; 16],
        scope: ProgrammaticCallerPolicyScopeV1,
        base_policy_revision: Option<ProgrammaticCallerPolicyRevisionReferenceV1>,
        content: ProgrammaticPolicyDraftContentV1,
        rationale: &str,
    ) -> DtoResult<Self> {
        validate_policy_text(rationale)?;
        if content.root_origin_rules.is_empty() || content.root_origin_rules.len() > 2 {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_ORIGIN_INVALID,
                "a draft declares at most the two closed root origins",
            ));
        }
        for (index, rule) in content.root_origin_rules.iter().enumerate() {
            if content.root_origin_rules[..index]
                .iter()
                .any(|other| other.root_origin_kind == rule.root_origin_kind)
            {
                return Err(policy_error(
                    PROGRAMMATIC_POLICY_ORIGIN_INVALID,
                    "a draft declares each closed root origin at most once",
                ));
            }
        }
        if content.admission_rules.len() > MAX_RULES_PER_POLICY_REVISION {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
                "a draft stays inside the closed rule bound",
            ));
        }
        let rationale_digest = Digest256::sha256(rationale.as_bytes());
        let mut draft = Self {
            draft_reference,
            scope,
            base_policy_revision,
            root_origin_rules: content.root_origin_rules,
            admission_rules: content.admission_rules,
            run_limits: content.run_limits,
            calendar_limit: content.calendar_limit,
            rationale_digest,
            canonical_draft_digest: Digest256::sha256(&[]),
        };
        draft.canonical_draft_digest = draft_digest(&draft);
        Ok(draft)
    }
}

/// Recomputes the deterministic digest of one draft's proposed content.
fn draft_digest(draft: &ProgrammaticCallerPolicyDraftV1) -> Digest256 {
    let mut input = Vec::new();
    push_text(&mut input, "programmatic-caller-policy-draft-v1");
    push_scope(&mut input, &draft.scope);
    match draft.base_policy_revision {
        Some(reference) => {
            push_field(&mut input, &[1]);
            push_uuid(&mut input, reference.policy_id);
            push_u64(&mut input, reference.revision);
        }
        None => push_field(&mut input, &[0]),
    }
    push_origin_rules(&mut input, &draft.root_origin_rules);
    push_count(&mut input, draft.admission_rules.len());
    for rule in &draft.admission_rules {
        push_rule(&mut input, rule);
    }
    push_u64(&mut input, draft.run_limits.max_actions);
    push_u64(&mut input, draft.run_limits.max_concurrent_actions);
    push_field(&mut input, &[draft.calendar_limit.period_kind.code()]);
    push_u64(&mut input, draft.calendar_limit.max_actions);
    push_field(&mut input, draft.rationale_digest.bytes().as_slice());
    Digest256::sha256(&input)
}

/// The outcome of proposing one draft for one owner scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticDraftProposalOutcomeV1 {
    /// No draft was pending for the scope; the proposal becomes pending.
    Pending,
    /// An equal proposal was already pending; evidence coalesces into it.
    Coalesced,
}

/// Proposes one draft against at most one pending draft for the same scope.
///
/// # Errors
///
/// Returns `programmatic_policy_draft_conflict` when a different draft is
/// already pending for the same owner scope.
pub fn propose_policy_draft(
    existing_pending: Option<&ProgrammaticCallerPolicyDraftV1>,
    proposed: &ProgrammaticCallerPolicyDraftV1,
) -> DtoResult<ProgrammaticDraftProposalOutcomeV1> {
    let Some(existing) = existing_pending else {
        return Ok(ProgrammaticDraftProposalOutcomeV1::Pending);
    };
    if existing.scope != proposed.scope {
        return Ok(ProgrammaticDraftProposalOutcomeV1::Pending);
    }
    if existing.canonical_draft_digest == proposed.canonical_draft_digest {
        return Ok(ProgrammaticDraftProposalOutcomeV1::Coalesced);
    }
    Err(policy_error(
        PROGRAMMATIC_POLICY_DRAFT_CONFLICT,
        "at most one pending draft exists per owner scope",
    ))
}

/// The closed user decision applied to one pending policy draft.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticPolicyDraftDecisionV1 {
    /// Accept the draft as proposed.
    Accept,
    /// Edit the proposal and accept the edited revision.
    EditAndAccept,
    /// Reject the draft without changing any policy.
    Reject,
}

/// What one draft decision produces.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticPolicyDraftOutcomeV1 {
    /// The user rejected the draft; no policy changed.
    NoPolicyChange,
    /// A new policy identity was created from the draft.
    PolicyCreated,
    /// A later immutable revision of an existing policy was created.
    RevisionCreated,
}

/// Applies one user decision to one pending draft.
///
/// # Errors
///
/// Returns `programmatic_policy_revision_conflict` when acceptance does not
/// match the exact base revision of an existing policy.
pub fn apply_policy_draft_decision(
    decision: ProgrammaticPolicyDraftDecisionV1,
    existing_policy: Option<([u8; 16], u64)>,
    accepted_base_revision: Option<u64>,
) -> DtoResult<ProgrammaticPolicyDraftOutcomeV1> {
    if decision == ProgrammaticPolicyDraftDecisionV1::Reject {
        return Ok(ProgrammaticPolicyDraftOutcomeV1::NoPolicyChange);
    }
    let Some((_policy_id, current_revision)) = existing_policy else {
        return Ok(ProgrammaticPolicyDraftOutcomeV1::PolicyCreated);
    };
    if accepted_base_revision != Some(current_revision) {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_REVISION_CONFLICT,
            "acceptance checks the exact base state of the existing policy",
        ));
    }
    Ok(ProgrammaticPolicyDraftOutcomeV1::RevisionCreated)
}

/// Validates the policy bound of one project.
///
/// # Errors
///
/// Returns `programmatic_policy_limit_exceeded` when the count exceeds 64.
pub fn validate_policies_in_project(count: usize) -> DtoResult<()> {
    if count > MAX_POLICIES_PER_PROJECT {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
            "one project keeps at most 64 policies",
        ));
    }
    Ok(())
}

/// Validates the policy bound attached to one goal.
///
/// # Errors
///
/// Returns `programmatic_policy_limit_exceeded` when the count exceeds 32.
pub fn validate_policies_attached_to_goal(count: usize) -> DtoResult<()> {
    if count > MAX_POLICIES_PER_GOAL {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
            "one goal keeps at most 32 attached policies",
        ));
    }
    Ok(())
}

/// Validates the own or inherited policy reference bound of one session.
///
/// # Errors
///
/// Returns `programmatic_policy_limit_exceeded` when the count exceeds 32.
pub fn validate_session_policy_references(count: usize) -> DtoResult<()> {
    if count > MAX_POLICY_REFERENCES_PER_SESSION {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED,
            "one session keeps at most 32 policy references",
        ));
    }
    Ok(())
}

/// Validates that one fork keeps its source policy references unchanged.
///
/// # Errors
///
/// Returns `programmatic_policy_inheritance_widening_forbidden` when the fork
/// drops or substitutes a source reference, which would grant a fresh
/// allowance.
pub fn validate_fork_policy_inheritance(
    source: &[ProgrammaticCallerPolicyRevisionReferenceV1],
    fork: &[ProgrammaticCallerPolicyRevisionReferenceV1],
) -> DtoResult<()> {
    for reference in source {
        if !fork.contains(reference) {
            return Err(policy_error(
                PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN,
                "a fork keeps the immutable source policy references and their identities",
            ));
        }
    }
    Ok(())
}

/// The single exact binding of one admission-time reservation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticReservationBindingV1 {
    /// The owning policy identity.
    pub policy_id: [u8; 16],
    /// The exact policy revision.
    pub policy_revision: u64,
    /// The root run identity that consumes the reservation.
    pub root_run_id: [u8; 16],
    /// The one reserved tool-call identity.
    pub tool_call_id: [u8; 16],
    /// The exact typed input digest of the reserved call.
    pub typed_input_digest: Digest256,
}

/// One admission-time policy counter reservation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticLimitReservationV1 {
    /// The durable reservation identity.
    pub reservation_reference: [u8; 16],
    /// The one exact reserved call binding.
    pub binding: ProgrammaticReservationBindingV1,
    /// The calendar counter identity this reservation consumes.
    pub calendar_counter_reference: [u8; 16],
    /// The reservation time in Unix milliseconds.
    pub reserved_at_ms: u64,
}

/// The run and calendar counter state of one policy identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticLimitStateV1 {
    /// The selected per-run limits.
    pub run_limits: ProgrammaticRunLimitsV1,
    /// The selected effective calendar action limit.
    pub calendar_limit: u64,
    /// The permanently consumed run actions.
    pub run_started_actions: u64,
    /// The outstanding run reservations.
    pub run_reserved_actions: u64,
    /// The started run actions that have not terminally finished.
    pub run_in_flight_actions: u64,
    /// The permanently consumed calendar actions.
    pub calendar_started_actions: u64,
    /// The outstanding calendar reservations.
    pub calendar_reserved_actions: u64,
    /// The accepted reservation references outstanding in this state.
    pub accepted_reservation_references: Vec<[u8; 16]>,
}

impl ProgrammaticLimitStateV1 {
    /// Creates one empty counter state for one selected policy identity.
    #[must_use]
    pub const fn new(run_limits: ProgrammaticRunLimitsV1, calendar_limit: u64) -> Self {
        Self {
            run_limits,
            calendar_limit,
            run_started_actions: 0,
            run_reserved_actions: 0,
            run_in_flight_actions: 0,
            calendar_started_actions: 0,
            calendar_reserved_actions: 0,
            accepted_reservation_references: Vec::new(),
        }
    }
}

/// Reserves one run and calendar action before `ToolCallStarted`.
///
/// An equal repeated operation reads the accepted idempotent binding and does
/// not reserve a second unit. Every limit is verified before any counter
/// mutates, so the transaction writes all reservations or none of them.
///
/// # Errors
///
/// Returns `programmatic_policy_reservation_conflict` when the same tool call
/// is already bound to a different input, `programmatic_policy_run_limit_exceeded`
/// when the per-run action or concurrency limit is exhausted, and
/// `programmatic_policy_calendar_limit_exceeded` when the calendar limit is
/// exhausted.
pub fn reserve_limit_action(
    state: &mut ProgrammaticLimitStateV1,
    binding: ProgrammaticReservationBindingV1,
    existing: &[ProgrammaticLimitReservationV1],
    reservation_reference: [u8; 16],
    calendar_counter_reference: [u8; 16],
    reserved_at_ms: u64,
) -> DtoResult<ProgrammaticLimitReservationV1> {
    for reservation in existing {
        if same_uuid(reservation.binding.tool_call_id, binding.tool_call_id) {
            if reservation.binding == binding {
                return Ok(reservation.clone());
            }
            return Err(policy_error(
                PROGRAMMATIC_POLICY_RESERVATION_CONFLICT,
                "one tool call never re-binds to a different typed input",
            ));
        }
    }
    if state
        .accepted_reservation_references
        .contains(&reservation_reference)
    {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_RESERVATION_CONFLICT,
            "one reservation reference is accepted at most once",
        ));
    }
    let run_actions_exhausted = state
        .run_started_actions
        .checked_add(state.run_reserved_actions)
        .is_none_or(|used| used >= state.run_limits.max_actions);
    if run_actions_exhausted {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_RUN_LIMIT_EXCEEDED,
            "the per-run action limit is exhausted",
        ));
    }
    let run_concurrency_exhausted = state
        .run_in_flight_actions
        .checked_add(state.run_reserved_actions)
        .and_then(|used| used.checked_add(1))
        .is_none_or(|used| used > state.run_limits.max_concurrent_actions);
    if run_concurrency_exhausted {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_RUN_LIMIT_EXCEEDED,
            "the per-run concurrency limit is exhausted",
        ));
    }
    let calendar_exhausted = state
        .calendar_started_actions
        .checked_add(state.calendar_reserved_actions)
        .is_none_or(|used| used >= state.calendar_limit);
    if calendar_exhausted {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_CALENDAR_LIMIT_EXCEEDED,
            "the calendar action limit is exhausted",
        ));
    }
    state.run_reserved_actions += 1;
    state.calendar_reserved_actions += 1;
    state
        .accepted_reservation_references
        .push(reservation_reference);
    Ok(ProgrammaticLimitReservationV1 {
        reservation_reference,
        binding,
        calendar_counter_reference,
        reserved_at_ms,
    })
}

/// Releases one known pre-effect reservation atomically.
///
/// # Errors
///
/// Returns `programmatic_policy_reservation_conflict` when the reservation is
/// not outstanding in this state.
pub fn release_unstarted_reservation(
    state: &mut ProgrammaticLimitStateV1,
    reservation: &ProgrammaticLimitReservationV1,
) -> DtoResult<()> {
    if state.run_reserved_actions == 0
        || state.calendar_reserved_actions == 0
        || !state
            .accepted_reservation_references
            .contains(&reservation.reservation_reference)
    {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_RESERVATION_CONFLICT,
            "no matching unstarted reservation is outstanding",
        ));
    }
    state.run_reserved_actions -= 1;
    state.calendar_reserved_actions -= 1;
    state
        .accepted_reservation_references
        .retain(|reference| *reference != reservation.reservation_reference);
    Ok(())
}

/// Commits one reservation to permanent consumption on `ToolCallStarted`.
///
/// # Errors
///
/// Returns `programmatic_policy_reservation_conflict` when the reservation is
/// not outstanding in this state.
pub fn commit_reservation_started(
    state: &mut ProgrammaticLimitStateV1,
    reservation: &ProgrammaticLimitReservationV1,
) -> DtoResult<()> {
    if state.run_reserved_actions == 0
        || state.calendar_reserved_actions == 0
        || !state
            .accepted_reservation_references
            .contains(&reservation.reservation_reference)
    {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_RESERVATION_CONFLICT,
            "no matching reservation is outstanding",
        ));
    }
    state.run_reserved_actions -= 1;
    state.run_started_actions += 1;
    state.run_in_flight_actions += 1;
    state.calendar_reserved_actions -= 1;
    state.calendar_started_actions += 1;
    state
        .accepted_reservation_references
        .retain(|reference| *reference != reservation.reservation_reference);
    Ok(())
}

/// Releases the concurrency slot of one terminally finished started action.
///
/// # Errors
///
/// Returns `programmatic_policy_reservation_conflict` when no started action
/// is in flight.
pub fn finish_started_action(
    state: &mut ProgrammaticLimitStateV1,
    _reservation: &ProgrammaticLimitReservationV1,
) -> DtoResult<()> {
    if state.run_in_flight_actions == 0 {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_RESERVATION_CONFLICT,
            "no started action is in flight",
        ));
    }
    state.run_in_flight_actions -= 1;
    Ok(())
}

/// Validates that one required policy counter is available.
///
/// # Errors
///
/// Returns `programmatic_policy_counter_unavailable` when no compatible
/// counter state exists.
pub fn validate_counter_available(state: Option<&ProgrammaticLimitStateV1>) -> DtoResult<()> {
    if state.is_some() {
        Ok(())
    } else {
        Err(policy_error(
            PROGRAMMATIC_POLICY_COUNTER_UNAVAILABLE,
            "an unavailable or incompatible counter blocks only the dependent action",
        ))
    }
}

/// The closed recovery dispositions of one recovered action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticRecoveryDispositionV1 {
    /// The action never started; its reservation is released atomically.
    InterruptedBeforeStart,
    /// A started action's external effect cannot be proven; it is never
    /// retried.
    ExternalEffectUnknown,
}

/// Returns the recovery disposition of one recovered action.
#[must_use]
pub const fn recovery_disposition_for(
    tool_call_started: bool,
) -> ProgrammaticRecoveryDispositionV1 {
    if tool_call_started {
        ProgrammaticRecoveryDispositionV1::ExternalEffectUnknown
    } else {
        ProgrammaticRecoveryDispositionV1::InterruptedBeforeStart
    }
}

/// Applies one recovery disposition to the policy counter state.
///
/// An unstarted reservation is released atomically; a started ambiguous
/// action keeps its permanent calendar and run consumption and is never
/// retried or re-reserved.
///
/// # Errors
///
/// Returns `programmatic_policy_reservation_conflict` when an unstarted
/// reservation is not outstanding.
pub fn apply_recovery_disposition(
    state: &mut ProgrammaticLimitStateV1,
    reservation: &ProgrammaticLimitReservationV1,
    tool_call_started: bool,
) -> DtoResult<ProgrammaticRecoveryDispositionV1> {
    if tool_call_started {
        return Ok(ProgrammaticRecoveryDispositionV1::ExternalEffectUnknown);
    }
    release_unstarted_reservation(state, reservation)?;
    Ok(ProgrammaticRecoveryDispositionV1::InterruptedBeforeStart)
}

/// The optional immutable calling-path links of one provenance record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticProvenanceLinksV1 {
    /// The immutable parent-link references of this calling path.
    pub parent_link_references: Vec<[u8; 16]>,
    /// The bridge operation reference when the call crossed the bridge.
    pub bridge_operation_reference: Option<[u8; 16]>,
    /// The leading goal reference when the tree is goal-directed.
    pub leading_goal_reference: Option<[u8; 16]>,
}

/// The immutable daemon-assigned provenance of one programmatic action.
///
/// This record carries safe identity, revision, digest, and reference values
/// only. It records no raw input, workspace path, grant, credential,
/// provider value, Python value, socket, process handle, external response,
/// or implementation resource.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticCallerProvenanceV1 {
    /// The one daemon-assigned root origin.
    pub root_origin: ProgrammaticCallerRootOriginV1,
    /// The root session identity.
    pub root_session_id: [u8; 16],
    /// The root run identity.
    pub root_run_id: [u8; 16],
    /// The current session identity.
    pub current_session_id: [u8; 16],
    /// The current run identity.
    pub current_run_id: [u8; 16],
    /// The immutable parent-link references of this calling path.
    pub parent_link_references: Vec<[u8; 16]>,
    /// The bridge operation reference when present.
    pub bridge_operation_reference: Option<[u8; 16]>,
    /// The leading goal reference when present.
    pub leading_goal_reference: Option<[u8; 16]>,
    /// The current tool-call identity.
    pub tool_call_id: [u8; 16],
    /// The exact selected registered tool identity.
    pub selected_tool_id: String,
    /// The exact selected descriptor revision.
    pub selected_descriptor_revision: u64,
    /// The selected MCP method reference when present.
    pub selected_mcp_method_reference: Option<String>,
    /// The typed input digest of this action.
    pub typed_input_digest: Digest256,
    /// The effective policy snapshot reference of this action.
    pub effective_policy_snapshot_reference: [u8; 16],
    /// The admission basis reference of this action.
    pub admission_basis_reference: [u8; 16],
    /// The limit reservation references of this action.
    pub limit_reservation_references: Vec<[u8; 16]>,
    /// The deterministic digest of this provenance record.
    pub canonical_provenance_digest: Digest256,
}

/// Creates the daemon-assigned provenance of one programmatic action.
///
/// The record is created once before `ToolCallStarted`, together with the
/// admission outcome it explains. A root-run call has no parent link; a
/// descendant call always has at least one and can never claim root identity
/// or a different root origin.
///
/// # Errors
///
/// Returns `programmatic_policy_origin_invalid` when the root origin does not
/// match the call context, when a root-run call carries parent links, or when
/// a descendant call carries none.
pub fn new_provenance(
    root_origin: ProgrammaticCallerRootOriginV1,
    call: &ProgrammaticAdmissionCallV1,
    links: ProgrammaticProvenanceLinksV1,
    effective_policy_snapshot_reference: [u8; 16],
    admission_basis_reference: [u8; 16],
    limit_reservation_references: Vec<[u8; 16]>,
) -> DtoResult<ProgrammaticCallerProvenanceV1> {
    if ProgrammaticRootOriginKindV1::of(&root_origin) != call.context.root_origin {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_ORIGIN_INVALID,
            "the daemon-assigned root origin must match the calling context",
        ));
    }
    if call.context.is_root_run_call() && !links.parent_link_references.is_empty() {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_ORIGIN_INVALID,
            "a root-run call has no parent link",
        ));
    }
    if !call.context.is_root_run_call() && links.parent_link_references.is_empty() {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_ORIGIN_INVALID,
            "a descendant call always continues an existing root tree",
        ));
    }
    let mut provenance = ProgrammaticCallerProvenanceV1 {
        root_origin,
        root_session_id: call.context.root_session_id,
        root_run_id: call.context.root_run_id,
        current_session_id: call.context.current_session_id,
        current_run_id: call.context.current_run_id,
        parent_link_references: links.parent_link_references,
        bridge_operation_reference: links.bridge_operation_reference,
        leading_goal_reference: links.leading_goal_reference,
        tool_call_id: call.context.tool_call_id,
        selected_tool_id: call.tool_id.clone(),
        selected_descriptor_revision: call.descriptor_revision,
        selected_mcp_method_reference: call
            .mcp_method
            .as_ref()
            .map(|method| method.method_reference.clone()),
        typed_input_digest: call.typed_input_digest,
        effective_policy_snapshot_reference,
        admission_basis_reference,
        limit_reservation_references,
        canonical_provenance_digest: Digest256::sha256(&[]),
    };
    provenance.canonical_provenance_digest = provenance_digest(&provenance);
    Ok(provenance)
}

/// Recomputes the deterministic digest of one provenance record.
#[must_use]
pub fn provenance_digest(provenance: &ProgrammaticCallerProvenanceV1) -> Digest256 {
    let mut input = Vec::new();
    push_text(&mut input, "programmatic-caller-provenance-v1");
    push_field(
        &mut input,
        &[ProgrammaticRootOriginKindV1::of(&provenance.root_origin).code()],
    );
    push_uuid(&mut input, provenance.root_session_id);
    push_uuid(&mut input, provenance.root_run_id);
    push_uuid(&mut input, provenance.current_session_id);
    push_uuid(&mut input, provenance.current_run_id);
    push_count(&mut input, provenance.parent_link_references.len());
    for link in &provenance.parent_link_references {
        push_uuid(&mut input, *link);
    }
    push_optional_uuid(&mut input, provenance.bridge_operation_reference);
    push_optional_uuid(&mut input, provenance.leading_goal_reference);
    push_uuid(&mut input, provenance.tool_call_id);
    push_text(&mut input, &provenance.selected_tool_id);
    push_u64(&mut input, provenance.selected_descriptor_revision);
    push_optional_text(
        &mut input,
        provenance.selected_mcp_method_reference.as_deref(),
    );
    push_field(&mut input, provenance.typed_input_digest.bytes().as_slice());
    push_uuid(&mut input, provenance.effective_policy_snapshot_reference);
    push_uuid(&mut input, provenance.admission_basis_reference);
    push_count(&mut input, provenance.limit_reservation_references.len());
    for reference in &provenance.limit_reservation_references {
        push_uuid(&mut input, *reference);
    }
    Digest256::sha256(&input)
}

/// Validates that one descendant provenance continues its parent root tree.
///
/// # Errors
///
/// Returns `programmatic_policy_origin_invalid` when the roots differ, when
/// the descendant carries no parent link, or when the descendant claims root
/// identity.
pub fn validate_provenance_continuity(
    parent: &ProgrammaticCallerProvenanceV1,
    child: &ProgrammaticCallerProvenanceV1,
) -> DtoResult<()> {
    if child.root_origin != parent.root_origin
        || !same_uuid(child.root_session_id, parent.root_session_id)
        || !same_uuid(child.root_run_id, parent.root_run_id)
    {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_ORIGIN_INVALID,
            "one calling path never re-roots into a different root tree",
        ));
    }
    if child.parent_link_references.is_empty() {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_ORIGIN_INVALID,
            "a descendant provenance always carries at least one parent link",
        ));
    }
    if same_uuid(child.current_session_id, child.root_session_id)
        && same_uuid(child.current_run_id, child.root_run_id)
    {
        return Err(policy_error(
            PROGRAMMATIC_POLICY_ORIGIN_INVALID,
            "a descendant provenance can never claim root identity",
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
    fn only_the_two_closed_paths_may_root() {
        let mut roots = Vec::new();
        for path in [
            ProgrammaticCallerPathV1::InteractiveUserTurn,
            ProgrammaticCallerPathV1::ContinualHarnessLaunch,
            ProgrammaticCallerPathV1::ProtocolPeer,
            ProgrammaticCallerPathV1::DetachedPythonTask,
            ProgrammaticCallerPathV1::ChildAgent,
            ProgrammaticCallerPathV1::McpService,
            ProgrammaticCallerPathV1::Provider,
            ProgrammaticCallerPathV1::BridgeChannel,
            ProgrammaticCallerPathV1::QueuedItem,
            ProgrammaticCallerPathV1::Replay,
            ProgrammaticCallerPathV1::DaemonRecovery,
        ] {
            if path_may_root(path) {
                roots.push(path);
            }
        }
        assert_eq!(roots.len(), 2);
    }

    #[test]
    fn failure_code_table_is_unique_and_complete() {
        let mut seen = std::collections::BTreeSet::new();
        for code in PROGRAMMATIC_POLICY_FAILURE_CODES {
            assert!(code.starts_with("programmatic_policy_"));
            assert!(seen.insert(code));
        }
        assert_eq!(seen.len(), 22);
    }

    const POLICY_PROJECT_ID: [u8; 16] = [0x01; 16];

    fn project_scope() -> ProgrammaticCallerPolicyScopeV1 {
        ProgrammaticCallerPolicyScopeV1::Project {
            project_id: POLICY_PROJECT_ID,
        }
    }

    fn session_scope() -> ProgrammaticCallerPolicyScopeV1 {
        ProgrammaticCallerPolicyScopeV1::Session {
            project_id: POLICY_PROJECT_ID,
            policy_owner_session_id: [0x02; 16],
        }
    }

    fn constraint(family: &str, values: &[&str]) -> DescriptorInputConstraintSelectionV1 {
        DescriptorInputConstraintSelectionV1::new(
            family,
            1,
            values.iter().map(|value| (*value).to_owned()).collect(),
        )
        .expect("fixture typed input constraint is valid")
    }

    fn all_effect_flags() -> Vec<ProgrammaticToolEffectFlagV1> {
        vec![
            ProgrammaticToolEffectFlagV1::LocalRead,
            ProgrammaticToolEffectFlagV1::LocalWrite,
            ProgrammaticToolEffectFlagV1::LocalExecute,
            ProgrammaticToolEffectFlagV1::NetworkAccess,
            ProgrammaticToolEffectFlagV1::ChildDelegation,
            ProgrammaticToolEffectFlagV1::UserInteraction,
            ProgrammaticToolEffectFlagV1::McpInvocation,
        ]
    }

    fn mcp_method_reference() -> ProgrammaticMcpMethodReferenceV1 {
        ProgrammaticMcpMethodReferenceV1::new([0x0a; 16], "tools/list", "rev-1")
            .expect("fixture MCP method reference is valid")
    }

    fn exact_read_selector() -> ProgrammaticRuleSelectorV1 {
        ProgrammaticRuleSelectorV1::exact_tool("read", 1)
            .expect("fixture exact tool selector is valid")
    }

    fn interactive_context() -> ProgrammaticCallContextV1 {
        ProgrammaticCallContextV1 {
            root_origin: ProgrammaticRootOriginKindV1::InteractiveUser,
            root_session_id: [0x10; 16],
            root_run_id: [0x11; 16],
            current_session_id: [0x10; 16],
            current_run_id: [0x11; 16],
            tool_call_id: [0x12; 16],
        }
    }

    fn descendant_context() -> ProgrammaticCallContextV1 {
        ProgrammaticCallContextV1 {
            current_session_id: [0x13; 16],
            current_run_id: [0x14; 16],
            tool_call_id: [0x15; 16],
            ..interactive_context()
        }
    }

    fn admission_call(
        context: ProgrammaticCallContextV1,
        tool_id: &str,
        effect_flags: Vec<ProgrammaticToolEffectFlagV1>,
        mcp_method: Option<ProgrammaticMcpMethodReferenceV1>,
        typed_input_constraints: Vec<DescriptorInputConstraintSelectionV1>,
    ) -> ProgrammaticAdmissionCallV1 {
        ProgrammaticAdmissionCallV1::new(
            context,
            tool_id,
            1,
            effect_flags,
            mcp_method,
            typed_input_constraints,
            Digest256::sha256(b"typed-input"),
        )
        .expect("fixture admission call is valid")
    }

    fn interactive_origin() -> ProgrammaticCallerRootOriginV1 {
        ProgrammaticCallerRootOriginV1::InteractiveUser {
            originating_turn_id: [0x16; 16],
        }
    }

    fn harness_origin() -> ProgrammaticCallerRootOriginV1 {
        ProgrammaticCallerRootOriginV1::ContinualHarness {
            harness_id: [0x17; 16],
            rule_revision: 1,
            trigger_reason_id: [0x18; 16],
        }
    }

    fn provenance_links(
        parent_link_references: Vec<[u8; 16]>,
        bridge_operation_reference: Option<[u8; 16]>,
        leading_goal_reference: Option<[u8; 16]>,
    ) -> ProgrammaticProvenanceLinksV1 {
        ProgrammaticProvenanceLinksV1 {
            parent_link_references,
            bridge_operation_reference,
            leading_goal_reference,
        }
    }

    fn policy_revision(policy_id: [u8; 16]) -> ProgrammaticCallerPolicyRevisionV1 {
        ProgrammaticCallerPolicyRevisionV1::new(
            policy_id,
            1,
            vec![
                ProgrammaticRootOriginRuleV1::new(
                    ProgrammaticRootOriginKindV1::InteractiveUser,
                    ProgrammaticAdmissionRuleV1::BoundedConfirmationRequired,
                ),
                ProgrammaticRootOriginRuleV1::new(
                    ProgrammaticRootOriginKindV1::ContinualHarness,
                    ProgrammaticAdmissionRuleV1::Prohibited,
                ),
            ],
            vec![
                ProgrammaticAdmissionRuleEntryV1::new(
                    ProgrammaticAdmissionRuleV1::BoundedConfirmationRequired,
                    vec![
                        ProgrammaticRuleSelectorV1::effect_profile(all_effect_flags())
                            .expect("fixture effect selector is valid"),
                        exact_read_selector(),
                        ProgrammaticRuleSelectorV1::mcp_method([0x0a; 16], "tools/list", "rev-1")
                            .expect("fixture MCP method selector is valid"),
                    ],
                    vec![constraint("command", &["ls", "cat"])],
                )
                .expect("fixture admission rule is valid"),
            ],
            ProgrammaticRunLimitsV1::new(128, 8).expect("fixture run limits are valid"),
            ProgrammaticCalendarLimitV1::new(ProgrammaticCalendarPeriodKindV1::Week, 512)
                .expect("fixture calendar limit is valid"),
            vec![ProgrammaticCallerPolicyRevisionReferenceV1 {
                policy_id: [0x31; 16],
                revision: 2,
            }],
        )
        .expect("fixture policy revision is valid")
    }

    fn applicable_policy(
        policy_id: [u8; 16],
        scope: ProgrammaticCallerPolicyScopeV1,
    ) -> ProgrammaticCallerApplicablePolicyV1 {
        ProgrammaticCallerApplicablePolicyV1::new(policy_id, 1, scope, policy_revision(policy_id))
            .expect("fixture applicable policy is valid")
    }

    fn corridor() -> ProgrammaticAuthorizationCorridorV1 {
        ProgrammaticAuthorizationCorridorV1::new(
            ProgrammaticCorridorRootV1 {
                root_session_id: [0x10; 16],
                root_run_id: [0x11; 16],
                root_origin_kind: ProgrammaticRootOriginKindV1::InteractiveUser,
                effective_policy_snapshot_digest: Digest256::sha256(b"snapshot"),
            },
            ProgrammaticCorridorSelectorsV1::new(
                vec![ProgrammaticToolEffectFlagV1::LocalRead],
                vec![exact_read_selector()],
                vec![constraint("command", &["ls", "cat"])],
            )
            .expect("fixture corridor selectors are valid"),
            64,
            4,
            [0x40; 16],
        )
        .expect("fixture corridor is valid")
    }

    fn draft_content(
        root_origin_rules: Vec<ProgrammaticRootOriginRuleV1>,
        admission_rules: Vec<ProgrammaticAdmissionRuleEntryV1>,
    ) -> ProgrammaticPolicyDraftContentV1 {
        ProgrammaticPolicyDraftContentV1 {
            root_origin_rules,
            admission_rules,
            run_limits: ProgrammaticRunLimitsV1::new(64, 4).expect("fixture run limits are valid"),
            calendar_limit: ProgrammaticCalendarLimitV1::new(
                ProgrammaticCalendarPeriodKindV1::Day,
                64,
            )
            .expect("fixture calendar limit is valid"),
        }
    }

    #[test]
    fn policy_and_constraint_text_reject_empty_over_long_control_and_credential_values() {
        assert_eq!(
            validate_policy_text("   ")
                .expect_err("blank policy text must fail")
                .code(),
            PROGRAMMATIC_POLICY_NOT_APPLICABLE
        );
        assert_eq!(
            validate_policy_text(&"a".repeat(MAX_POLICY_TEXT_CHARS + 1))
                .expect_err("over-long policy text must fail")
                .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            validate_policy_text("read\nwrite")
                .expect_err("control-bearing policy text must fail")
                .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            validate_policy_text("sk-live-token")
                .expect_err("credential-shaped policy text must fail")
                .code(),
            CREDENTIALS_FORBIDDEN
        );
        assert_eq!(
            validate_constraint_text(&"a".repeat(MAX_POLICY_TEXT_CHARS + 1))
                .expect_err("over-long constraint text must fail")
                .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            validate_constraint_text("api_key=abc")
                .expect_err("credential-shaped constraint text must fail")
                .code(),
            CREDENTIALS_FORBIDDEN
        );
        assert!(validate_policy_text("read").is_ok());
        assert!(validate_constraint_text("command").is_ok());
    }

    #[test]
    fn run_and_calendar_limits_stay_inside_their_closed_bounds() {
        assert_eq!(
            ProgrammaticRunLimitsV1::new(0, 1)
                .expect_err("zero run actions must fail")
                .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticRunLimitsV1::new(MAX_POLICY_ACTIONS_PER_RUN + 1, 1)
                .expect_err("an over-limit run action count must fail")
                .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticRunLimitsV1::new(1, 0)
                .expect_err("zero concurrent actions must fail")
                .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticRunLimitsV1::new(1, MAX_POLICY_CONCURRENT_ACTIONS_PER_RUN + 1)
                .expect_err("an over-limit concurrency count must fail")
                .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticCalendarLimitV1::new(ProgrammaticCalendarPeriodKindV1::Day, 0)
                .expect_err("a zero calendar limit must fail")
                .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticCalendarLimitV1::new(
                ProgrammaticCalendarPeriodKindV1::Day,
                MAX_CALENDAR_ACTION_LIMIT + 1,
            )
            .expect_err("an over-limit calendar limit must fail")
            .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticRunLimitsV1::new(MAX_POLICY_ACTIONS_PER_RUN, 1)
                .expect("the maximum run limits are valid")
                .max_actions,
            MAX_POLICY_ACTIONS_PER_RUN
        );
    }

    #[test]
    fn rule_selectors_reject_empty_over_limit_duplicate_and_zero_revision_selections() {
        assert_eq!(
            ProgrammaticRuleSelectorV1::effect_profile(Vec::new())
                .expect_err("an effect selector needs one flag")
                .code(),
            PROGRAMMATIC_POLICY_NOT_APPLICABLE
        );
        assert_eq!(
            ProgrammaticRuleSelectorV1::effect_profile(vec![
                ProgrammaticToolEffectFlagV1::LocalRead;
                MAX_SELECTORS_PER_RULE + 1
            ])
            .expect_err("an effect selector stays inside its flag bound")
            .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticRuleSelectorV1::effect_profile(vec![
                ProgrammaticToolEffectFlagV1::LocalRead,
                ProgrammaticToolEffectFlagV1::LocalRead,
            ])
            .expect_err("an effect selector never repeats a flag")
            .code(),
            PROGRAMMATIC_POLICY_NOT_APPLICABLE
        );
        assert_eq!(
            ProgrammaticRuleSelectorV1::exact_tool("read", 0)
                .expect_err("an exact tool selector needs a descriptor revision")
                .code(),
            PROGRAMMATIC_POLICY_NOT_APPLICABLE
        );
        assert_eq!(
            ProgrammaticRuleSelectorV1::mcp_method([0x0a; 16], "", "rev-1")
                .expect_err("an MCP method selector needs a method reference")
                .code(),
            PROGRAMMATIC_POLICY_NOT_APPLICABLE
        );
    }

    #[test]
    fn selector_narrowing_covers_mcp_methods_and_mixed_selector_kinds() {
        let parent = ProgrammaticRuleSelectorV1::mcp_method([0x0a; 16], "tools/list", "rev-1")
            .expect("fixture MCP method selector is valid");
        let child = ProgrammaticRuleSelectorV1::mcp_method([0x0a; 16], "tools/list", "rev-1")
            .expect("fixture MCP method selector is valid");
        assert!(child.is_narrower_or_equal_than(&parent));
        assert!(validate_selector_narrowing(&parent, &child).is_ok());

        let different_method =
            ProgrammaticRuleSelectorV1::mcp_method([0x0a; 16], "tools/call", "rev-2")
                .expect("fixture MCP method selector is valid");
        assert!(!different_method.is_narrower_or_equal_than(&parent));
        assert_eq!(
            validate_selector_narrowing(&parent, &different_method)
                .expect_err("a different MCP method widens")
                .code(),
            PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN
        );

        let exact_tool = exact_read_selector();
        assert!(!exact_tool.is_narrower_or_equal_than(&parent));
        assert!(!parent.is_narrower_or_equal_than(&exact_tool));
    }

    #[test]
    fn admission_calls_reject_invalid_shapes_and_report_root_run_calls() {
        let typed_input_digest = Digest256::sha256(b"typed-input");
        assert_eq!(
            ProgrammaticAdmissionCallV1::new(
                interactive_context(),
                "read",
                0,
                Vec::new(),
                None,
                Vec::new(),
                typed_input_digest,
            )
            .expect_err("an admission call needs a descriptor revision")
            .code(),
            PROGRAMMATIC_POLICY_NOT_APPLICABLE
        );
        assert_eq!(
            ProgrammaticAdmissionCallV1::new(
                interactive_context(),
                "read",
                1,
                Vec::new(),
                Some(mcp_method_reference()),
                Vec::new(),
                typed_input_digest,
            )
            .expect_err("an MCP method reference belongs only to the mcp tool")
            .code(),
            PROGRAMMATIC_POLICY_NOT_APPLICABLE
        );
        assert_eq!(
            ProgrammaticAdmissionCallV1::new(
                interactive_context(),
                "read",
                1,
                vec![ProgrammaticToolEffectFlagV1::LocalRead; MAX_SELECTORS_PER_RULE + 1],
                None,
                Vec::new(),
                typed_input_digest,
            )
            .expect_err("an admission call stays inside its effect bound")
            .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticAdmissionCallV1::new(
                interactive_context(),
                "read",
                1,
                vec![
                    ProgrammaticToolEffectFlagV1::LocalRead,
                    ProgrammaticToolEffectFlagV1::LocalRead,
                ],
                None,
                Vec::new(),
                typed_input_digest,
            )
            .expect_err("an admission call never repeats an effect flag")
            .code(),
            PROGRAMMATIC_POLICY_NOT_APPLICABLE
        );
        assert_eq!(
            ProgrammaticAdmissionCallV1::new(
                interactive_context(),
                "read",
                1,
                Vec::new(),
                None,
                vec![constraint("command", &["ls"]); MAX_TYPED_INPUT_CONSTRAINTS_PER_RULE + 1],
                typed_input_digest,
            )
            .expect_err("an admission call stays inside its constraint bound")
            .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );

        let root_call = admission_call(
            interactive_context(),
            "read",
            vec![ProgrammaticToolEffectFlagV1::LocalRead],
            None,
            Vec::new(),
        );
        assert!(root_call.is_root_run_call());
        assert!(root_call.context.is_root_run_call());
        let descendant_call =
            admission_call(descendant_context(), "read", Vec::new(), None, Vec::new());
        assert!(!descendant_call.is_root_run_call());
    }

    #[test]
    fn admission_rule_entries_require_a_selector_and_stay_inside_their_bounds() {
        assert_eq!(
            ProgrammaticAdmissionRuleEntryV1::new(
                ProgrammaticAdmissionRuleV1::Prohibited,
                Vec::new(),
                Vec::new(),
            )
            .expect_err("an admission rule needs a selector")
            .code(),
            PROGRAMMATIC_POLICY_NOT_APPLICABLE
        );
        assert_eq!(
            ProgrammaticAdmissionRuleEntryV1::new(
                ProgrammaticAdmissionRuleV1::Prohibited,
                vec![exact_read_selector(); MAX_SELECTORS_PER_RULE + 1],
                Vec::new(),
            )
            .expect_err("an admission rule stays inside its selector bound")
            .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticAdmissionRuleEntryV1::new(
                ProgrammaticAdmissionRuleV1::Prohibited,
                vec![exact_read_selector()],
                vec![constraint("command", &["ls"]); MAX_TYPED_INPUT_CONSTRAINTS_PER_RULE + 1],
            )
            .expect_err("an admission rule stays inside its constraint bound")
            .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
    }

    #[test]
    fn policy_revision_digests_cover_every_selector_origin_and_reference_shape() {
        let revision = policy_revision([0x21; 16]);
        assert_eq!(
            policy_revision_digest(&revision),
            revision.canonical_revision_digest
        );
        assert_ne!(
            policy_identity_digest(
                [0x21; 16],
                &session_scope(),
                ProgrammaticCalendarPeriodKindV1::Week,
                1,
            ),
            policy_identity_digest(
                [0x21; 16],
                &project_scope(),
                ProgrammaticCalendarPeriodKindV1::Week,
                1,
            )
        );
    }

    #[test]
    fn policy_revisions_reject_an_empty_or_duplicated_root_origin_set() {
        let run_limits = ProgrammaticRunLimitsV1::new(8, 2).expect("fixture run limits are valid");
        let calendar_limit =
            ProgrammaticCalendarLimitV1::new(ProgrammaticCalendarPeriodKindV1::Day, 8)
                .expect("fixture calendar limit is valid");
        assert_eq!(
            ProgrammaticCallerPolicyRevisionV1::new(
                [0x21; 16],
                1,
                Vec::new(),
                Vec::new(),
                run_limits,
                calendar_limit,
                Vec::new(),
            )
            .expect_err("a revision declares at least one root origin")
            .code(),
            PROGRAMMATIC_POLICY_ORIGIN_INVALID
        );
        assert_eq!(
            ProgrammaticCallerPolicyRevisionV1::new(
                [0x21; 16],
                1,
                vec![
                    ProgrammaticRootOriginRuleV1::new(
                        ProgrammaticRootOriginKindV1::InteractiveUser,
                        ProgrammaticAdmissionRuleV1::Prohibited,
                    ),
                    ProgrammaticRootOriginRuleV1::new(
                        ProgrammaticRootOriginKindV1::InteractiveUser,
                        ProgrammaticAdmissionRuleV1::Prohibited,
                    ),
                ],
                Vec::new(),
                run_limits,
                calendar_limit,
                Vec::new(),
            )
            .expect_err("a revision declares each root origin at most once")
            .code(),
            PROGRAMMATIC_POLICY_ORIGIN_INVALID
        );
    }

    #[test]
    fn applicable_policies_require_the_exact_selected_revision() {
        let revision_record = policy_revision([0x21; 16]);
        assert_eq!(
            ProgrammaticCallerApplicablePolicyV1::new(
                [0x22; 16],
                1,
                project_scope(),
                revision_record.clone(),
            )
            .expect_err("the revision record must be the exact selected revision")
            .code(),
            PROGRAMMATIC_POLICY_REVISION_CONFLICT
        );
        assert_eq!(
            ProgrammaticCallerApplicablePolicyV1::new(
                [0x21; 16],
                2,
                project_scope(),
                revision_record.clone(),
            )
            .expect_err("the revision record must be the exact selected revision")
            .code(),
            PROGRAMMATIC_POLICY_REVISION_CONFLICT
        );
        let policy = ProgrammaticCallerApplicablePolicyV1::new(
            [0x21; 16],
            1,
            project_scope(),
            revision_record,
        )
        .expect("the exact selected revision is applicable");
        assert_eq!(policy.policy_id, [0x21; 16]);
        assert_eq!(policy.revision, 1);
    }

    #[test]
    fn effective_snapshots_intersect_two_policies_and_keep_the_most_restrictive_decision() {
        let first = applicable_policy([0x21; 16], session_scope());
        let second = applicable_policy([0x22; 16], project_scope());
        let snapshot = resolve_effective_policy_snapshot(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            &[second.clone(), first.clone()],
        )
        .expect("two applicable policies resolve a snapshot");
        assert_eq!(snapshot.policy_references.len(), 2);
        assert_eq!(snapshot.policy_references[0].policy_id, [0x21; 16]);
        assert_eq!(snapshot.scope_provenance.len(), 2);
        assert_eq!(
            snapshot.decision_ceiling,
            ProgrammaticAdmissionRuleV1::BoundedConfirmationRequired
        );
        assert_eq!(snapshot.run_limits.max_actions, 128);
        assert_eq!(snapshot.calendar_limits.len(), 2);
        assert_eq!(snapshot.calendar_counter_references.len(), 2);
        assert_eq!(snapshot.ordered_rules.len(), 2);
        assert!(snapshot.baseline.is_some());
        assert_eq!(effective_calendar_action_limit(&snapshot), Some(512));
        assert_eq!(
            effective_policy_snapshot_digest(&snapshot),
            snapshot.snapshot_digest
        );

        let harness = resolve_effective_policy_snapshot(
            ProgrammaticRootOriginKindV1::ContinualHarness,
            &[first, second],
        )
        .expect("both policies declare the harness origin");
        assert_eq!(
            harness.decision_ceiling,
            ProgrammaticAdmissionRuleV1::Prohibited
        );
        assert!(harness.baseline.is_none());
        assert_eq!(harness.calendar_limits.len(), 2);
    }

    #[test]
    fn policy_record_and_draft_size_checks_accept_their_exact_bounds() {
        assert!(validate_policy_record_size(MAX_POLICY_RECORD_BYTES).is_ok());
        assert!(validate_draft_size(MAX_POLICY_RECORD_BYTES).is_ok());
        assert!(
            validate_effective_policy_snapshot_size(MAX_EFFECTIVE_POLICY_SNAPSHOT_BYTES).is_ok()
        );
        assert_eq!(
            validate_draft_size(MAX_POLICY_RECORD_BYTES + 1)
                .expect_err("an over-limit draft must fail")
                .code(),
            PROGRAMMATIC_POLICY_DRAFT_TOO_LARGE
        );
    }

    #[test]
    fn corridor_selectors_and_bounds_stay_closed() {
        assert_eq!(
            ProgrammaticCorridorSelectorsV1::new(Vec::new(), Vec::new(), Vec::new())
                .expect_err("a corridor selects at least one effect or exact tool")
                .code(),
            PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE
        );
        assert_eq!(
            ProgrammaticCorridorSelectorsV1::new(
                vec![ProgrammaticToolEffectFlagV1::LocalRead; MAX_SELECTORS_PER_RULE + 1],
                Vec::new(),
                Vec::new(),
            )
            .expect_err("a corridor stays inside its effect bound")
            .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticCorridorSelectorsV1::new(
                vec![ProgrammaticToolEffectFlagV1::LocalRead],
                vec![exact_read_selector(); MAX_SELECTORS_PER_RULE + 1],
                Vec::new(),
            )
            .expect_err("a corridor stays inside its exact selector bound")
            .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticCorridorSelectorsV1::new(
                vec![ProgrammaticToolEffectFlagV1::LocalRead],
                Vec::new(),
                vec![constraint("command", &["ls"]); MAX_TYPED_INPUT_CONSTRAINTS_PER_RULE + 1],
            )
            .expect_err("a corridor stays inside its constraint bound")
            .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticCorridorSelectorsV1::new(
                vec![
                    ProgrammaticToolEffectFlagV1::LocalRead,
                    ProgrammaticToolEffectFlagV1::LocalRead,
                ],
                Vec::new(),
                Vec::new(),
            )
            .expect_err("a corridor never repeats a required effect flag")
            .code(),
            PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE
        );

        let root = ProgrammaticCorridorRootV1 {
            root_session_id: [0x10; 16],
            root_run_id: [0x11; 16],
            root_origin_kind: ProgrammaticRootOriginKindV1::InteractiveUser,
            effective_policy_snapshot_digest: Digest256::sha256(b"snapshot"),
        };
        let selectors = ProgrammaticCorridorSelectorsV1::new(
            vec![ProgrammaticToolEffectFlagV1::LocalRead],
            vec![exact_read_selector()],
            Vec::new(),
        )
        .expect("fixture corridor selectors are valid");
        assert_eq!(
            ProgrammaticAuthorizationCorridorV1::new(root, selectors.clone(), 0, 4, [0x40; 16])
                .expect_err("a corridor needs a non-zero action bound")
                .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticAuthorizationCorridorV1::new(
                root,
                selectors.clone(),
                MAX_POLICY_ACTIONS_PER_RUN + 1,
                4,
                [0x40; 16],
            )
            .expect_err("a corridor stays inside its action bound")
            .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticAuthorizationCorridorV1::new(root, selectors.clone(), 64, 0, [0x40; 16])
                .expect_err("a corridor needs a non-zero concurrency bound")
                .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
        assert_eq!(
            ProgrammaticAuthorizationCorridorV1::new(
                root,
                selectors,
                64,
                MAX_POLICY_CONCURRENT_ACTIONS_PER_RUN + 1,
                [0x40; 16],
            )
            .expect_err("a corridor stays inside its concurrency bound")
            .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );
    }

    #[test]
    fn corridor_use_and_child_selectors_stay_inside_the_approved_corridor() {
        let corridor = corridor();
        assert_eq!(
            validate_corridor_use(
                &corridor,
                &admission_call(
                    interactive_context(),
                    "read",
                    Vec::new(),
                    None,
                    vec![constraint("command", &["ls"])],
                ),
            )
            .expect_err("the call must satisfy the corridor effect selector")
            .code(),
            PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE
        );
        assert_eq!(
            validate_corridor_use(
                &corridor,
                &admission_call(
                    interactive_context(),
                    "glob",
                    vec![ProgrammaticToolEffectFlagV1::LocalRead],
                    None,
                    vec![constraint("command", &["ls"])],
                ),
            )
            .expect_err("the call must satisfy a corridor exact selector")
            .code(),
            PROGRAMMATIC_POLICY_CORRIDOR_UNAVAILABLE
        );
        assert_eq!(
            validate_corridor_use(
                &corridor,
                &admission_call(
                    interactive_context(),
                    "read",
                    vec![ProgrammaticToolEffectFlagV1::LocalRead],
                    None,
                    Vec::new(),
                ),
            )
            .expect_err("the call must satisfy every selected typed input constraint")
            .code(),
            PROGRAMMATIC_POLICY_INPUT_CONSTRAINT_MISMATCH
        );
        assert!(
            validate_corridor_use(
                &corridor,
                &admission_call(
                    interactive_context(),
                    "read",
                    vec![ProgrammaticToolEffectFlagV1::LocalRead],
                    None,
                    vec![constraint("command", &["ls", "cat"])],
                ),
            )
            .is_ok()
        );

        let missing_effect = ProgrammaticCorridorSelectorsV1::new(
            Vec::new(),
            vec![exact_read_selector()],
            Vec::new(),
        )
        .expect("fixture corridor selectors are valid");
        assert_eq!(
            validate_corridor_child_selectors(&corridor, &missing_effect)
                .expect_err("a child may only require the corridor's declared effects")
                .code(),
            PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN
        );

        let widened_constraint = ProgrammaticCorridorSelectorsV1::new(
            vec![ProgrammaticToolEffectFlagV1::LocalRead],
            vec![exact_read_selector()],
            vec![constraint("other-family", &["ls"])],
        )
        .expect("fixture corridor selectors are valid");
        assert_eq!(
            validate_corridor_child_selectors(&corridor, &widened_constraint)
                .expect_err("a child may only narrow a corridor typed input constraint")
                .code(),
            PROGRAMMATIC_POLICY_INHERITANCE_WIDENING_FORBIDDEN
        );

        let narrowed = ProgrammaticCorridorSelectorsV1::new(
            vec![ProgrammaticToolEffectFlagV1::LocalRead],
            vec![exact_read_selector()],
            vec![constraint("command", &["ls"])],
        )
        .expect("fixture corridor selectors are valid");
        assert!(validate_corridor_child_selectors(&corridor, &narrowed).is_ok());
    }

    #[test]
    fn corridor_usage_rejects_outstanding_state_conflicts() {
        let corridor = corridor();
        let mut usage = ProgrammaticCorridorUsageV1::new(&corridor);
        assert_eq!(usage.remaining_actions, 64);
        assert_eq!(
            usage
                .release_unstarted(&corridor)
                .expect_err("no unstarted corridor reservation is outstanding")
                .code(),
            PROGRAMMATIC_POLICY_RESERVATION_CONFLICT
        );
        assert_eq!(
            usage
                .consume_started(&corridor)
                .expect_err("no started corridor reservation is outstanding")
                .code(),
            PROGRAMMATIC_POLICY_RESERVATION_CONFLICT
        );
        usage
            .reserve(&corridor)
            .expect("one corridor reserve succeeds");
        assert_eq!(usage.remaining_actions, 63);
        assert_eq!(usage.remaining_concurrent_actions, 3);
        usage
            .release_unstarted(&corridor)
            .expect("the unstarted corridor reserve releases");
        assert_eq!(usage.remaining_actions, 64);
    }

    #[test]
    fn policy_drafts_reject_invalid_origin_sets_and_rule_bounds() {
        assert_eq!(
            ProgrammaticCallerPolicyDraftV1::new(
                [0x50; 16],
                project_scope(),
                None,
                draft_content(Vec::new(), Vec::new()),
                "safe rationale",
            )
            .expect_err("a draft declares at least one root origin")
            .code(),
            PROGRAMMATIC_POLICY_ORIGIN_INVALID
        );
        assert_eq!(
            ProgrammaticCallerPolicyDraftV1::new(
                [0x50; 16],
                project_scope(),
                None,
                draft_content(
                    vec![
                        ProgrammaticRootOriginRuleV1::new(
                            ProgrammaticRootOriginKindV1::InteractiveUser,
                            ProgrammaticAdmissionRuleV1::Prohibited,
                        ),
                        ProgrammaticRootOriginRuleV1::new(
                            ProgrammaticRootOriginKindV1::InteractiveUser,
                            ProgrammaticAdmissionRuleV1::Prohibited,
                        ),
                    ],
                    Vec::new(),
                ),
                "safe rationale",
            )
            .expect_err("a draft declares each root origin at most once")
            .code(),
            PROGRAMMATIC_POLICY_ORIGIN_INVALID
        );
        assert_eq!(
            ProgrammaticCallerPolicyDraftV1::new(
                [0x50; 16],
                project_scope(),
                None,
                draft_content(
                    vec![ProgrammaticRootOriginRuleV1::new(
                        ProgrammaticRootOriginKindV1::InteractiveUser,
                        ProgrammaticAdmissionRuleV1::Prohibited,
                    )],
                    vec![
                        ProgrammaticAdmissionRuleEntryV1::new(
                            ProgrammaticAdmissionRuleV1::Prohibited,
                            vec![exact_read_selector()],
                            Vec::new(),
                        )
                        .expect("fixture admission rule is valid");
                        MAX_RULES_PER_POLICY_REVISION + 1
                    ],
                ),
                "safe rationale",
            )
            .expect_err("a draft stays inside the closed rule bound")
            .code(),
            PROGRAMMATIC_POLICY_LIMIT_EXCEEDED
        );

        let draft = ProgrammaticCallerPolicyDraftV1::new(
            [0x50; 16],
            session_scope(),
            Some(ProgrammaticCallerPolicyRevisionReferenceV1 {
                policy_id: [0x21; 16],
                revision: 1,
            }),
            draft_content(
                vec![ProgrammaticRootOriginRuleV1::new(
                    ProgrammaticRootOriginKindV1::InteractiveUser,
                    ProgrammaticAdmissionRuleV1::Prohibited,
                )],
                Vec::new(),
            ),
            "safe rationale",
        )
        .expect("fixture draft is valid");
        assert_eq!(draft_digest(&draft), draft.canonical_draft_digest);
        assert_eq!(
            propose_policy_draft(None, &draft).expect("the first proposal is pending"),
            ProgrammaticDraftProposalOutcomeV1::Pending
        );
        assert_eq!(
            propose_policy_draft(Some(&draft), &draft).expect("an equal proposal coalesces"),
            ProgrammaticDraftProposalOutcomeV1::Coalesced
        );
    }

    #[test]
    fn reservations_enforce_single_binding_concurrency_and_outstanding_state() {
        let run_limits = ProgrammaticRunLimitsV1::new(MAX_POLICY_ACTIONS_PER_RUN, 1)
            .expect("fixture run limits are valid");
        let mut state = ProgrammaticLimitStateV1::new(run_limits, 64);
        let binding = |tool_call_id: [u8; 16]| ProgrammaticReservationBindingV1 {
            policy_id: [0x21; 16],
            policy_revision: 1,
            root_run_id: [0x11; 16],
            tool_call_id,
            typed_input_digest: Digest256::sha256(b"typed-input"),
        };
        let reservation = reserve_limit_action(
            &mut state,
            binding([0x12; 16]),
            &[],
            [0x60; 16],
            [0x61; 16],
            1_000,
        )
        .expect("the first reservation is accepted");
        assert_eq!(
            reserve_limit_action(
                &mut state,
                binding([0x13; 16]),
                std::slice::from_ref(&reservation),
                [0x62; 16],
                [0x61; 16],
                1_001,
            )
            .expect_err("the single concurrency slot is already reserved")
            .code(),
            PROGRAMMATIC_POLICY_RUN_LIMIT_EXCEEDED
        );
        assert_eq!(
            reserve_limit_action(
                &mut state,
                binding([0x14; 16]),
                std::slice::from_ref(&reservation),
                [0x60; 16],
                [0x61; 16],
                1_002,
            )
            .expect_err("one reservation reference is accepted at most once")
            .code(),
            PROGRAMMATIC_POLICY_RESERVATION_CONFLICT
        );

        let unknown = ProgrammaticLimitReservationV1 {
            reservation_reference: [0x63; 16],
            binding: binding([0x15; 16]),
            calendar_counter_reference: [0x61; 16],
            reserved_at_ms: 1_003,
        };
        assert_eq!(
            release_unstarted_reservation(&mut state, &unknown)
                .expect_err("no matching unstarted reservation is outstanding")
                .code(),
            PROGRAMMATIC_POLICY_RESERVATION_CONFLICT
        );
        assert_eq!(
            finish_started_action(&mut state, &reservation)
                .expect_err("no started action is in flight")
                .code(),
            PROGRAMMATIC_POLICY_RESERVATION_CONFLICT
        );
        commit_reservation_started(&mut state, &reservation)
            .expect("the outstanding reservation commits");
        assert_eq!(state.run_started_actions, 1);
        assert_eq!(state.run_in_flight_actions, 1);
        finish_started_action(&mut state, &reservation).expect("the started action finishes");
        assert_eq!(state.run_in_flight_actions, 0);
        assert_eq!(
            recovery_disposition_for(false),
            ProgrammaticRecoveryDispositionV1::InterruptedBeforeStart
        );
        assert_eq!(
            recovery_disposition_for(true),
            ProgrammaticRecoveryDispositionV1::ExternalEffectUnknown
        );
    }

    #[test]
    fn provenance_records_reject_invalid_root_shapes_and_continuity() {
        let root_call = admission_call(
            interactive_context(),
            "read",
            vec![ProgrammaticToolEffectFlagV1::LocalRead],
            None,
            Vec::new(),
        );
        let mcp_call = admission_call(
            interactive_context(),
            "mcp",
            vec![ProgrammaticToolEffectFlagV1::LocalRead],
            Some(mcp_method_reference()),
            Vec::new(),
        );
        assert_eq!(
            new_provenance(
                harness_origin(),
                &root_call,
                provenance_links(Vec::new(), None, None),
                [0x80; 16],
                [0x81; 16],
                Vec::new(),
            )
            .expect_err("the daemon-assigned root origin must match the calling context")
            .code(),
            PROGRAMMATIC_POLICY_ORIGIN_INVALID
        );
        assert_eq!(
            new_provenance(
                interactive_origin(),
                &root_call,
                provenance_links(vec![[0x82; 16]], None, None),
                [0x80; 16],
                [0x81; 16],
                Vec::new(),
            )
            .expect_err("a root-run call has no parent link")
            .code(),
            PROGRAMMATIC_POLICY_ORIGIN_INVALID
        );
        let descendant_call =
            admission_call(descendant_context(), "read", Vec::new(), None, Vec::new());
        assert_eq!(
            new_provenance(
                interactive_origin(),
                &descendant_call,
                provenance_links(Vec::new(), None, None),
                [0x80; 16],
                [0x81; 16],
                Vec::new(),
            )
            .expect_err("a descendant call always continues an existing root tree")
            .code(),
            PROGRAMMATIC_POLICY_ORIGIN_INVALID
        );

        let provenance = new_provenance(
            interactive_origin(),
            &mcp_call,
            provenance_links(Vec::new(), Some([0x83; 16]), Some([0x84; 16])),
            [0x80; 16],
            [0x81; 16],
            vec![[0x85; 16]],
        )
        .expect("a root-run call with optional links is valid");
        assert_eq!(
            provenance_digest(&provenance),
            provenance.canonical_provenance_digest
        );
        assert_eq!(
            provenance.selected_mcp_method_reference.as_deref(),
            Some("tools/list")
        );

        let mut child = provenance.clone();
        child.parent_link_references = vec![[0x86; 16]];
        child.current_session_id = [0x13; 16];
        child.current_run_id = [0x14; 16];
        assert!(validate_provenance_continuity(&provenance, &child).is_ok());

        let mut no_parent_link = child.clone();
        no_parent_link.parent_link_references = Vec::new();
        assert_eq!(
            validate_provenance_continuity(&provenance, &no_parent_link)
                .expect_err("a descendant provenance always carries a parent link")
                .code(),
            PROGRAMMATIC_POLICY_ORIGIN_INVALID
        );

        let mut root_claiming = child;
        root_claiming.current_session_id = provenance.root_session_id;
        root_claiming.current_run_id = provenance.root_run_id;
        assert_eq!(
            validate_provenance_continuity(&provenance, &root_claiming)
                .expect_err("a descendant provenance can never claim root identity")
                .code(),
            PROGRAMMATIC_POLICY_ORIGIN_INVALID
        );
    }
}
