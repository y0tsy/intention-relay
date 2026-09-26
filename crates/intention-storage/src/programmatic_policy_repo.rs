//! Durable programmatic-caller policy and admission contracts for
//! architecture 27.
//!
//! Every type below is a DTO-only durable record for the Slice 3 policy
//! surface activated by ADR 0044: policy identities and immutable revisions,
//! effective snapshots frozen at admission, exact confirmations, bounded
//! corridors, inactive model drafts, shared counters, and admission
//! reservations. Identities are canonical daemon-assigned text values,
//! content is bounded credential-free safe data, and digests are canonical
//! `sha256:<64 lowercase hex>` text.
//!
//! Policy bodies, raw tool input, workspace paths, grants, credentials,
//! provider resources, and counter history never cross this boundary; a
//! credential-shaped value is rejected before it can reach a row.

use intention_types::DtoResult;

use crate::{
    validate_safe_content, validate_safe_digest, validate_safe_label, validate_safe_labels,
};

/// The maximum page size of one durable policy read.
pub const MAX_POLICY_RECORD_PAGE: u64 = 64;
/// The maximum count of bounded selector references on one corridor.
pub const MAX_POLICY_SELECTORS: usize = 16;
/// The maximum count of inherited policy references on one revision.
pub const MAX_POLICY_INHERITED_REFERENCES: usize = 32;
/// The maximum count of selected policy references on one snapshot.
pub const MAX_POLICY_SNAPSHOT_REFERENCES: usize = 64;

/// The exactly-one durable owner scope of one policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgrammaticPolicyScopeDto {
    /// A project policy that applies to every current and future session.
    Project {
        /// The owning project identity.
        project_id: String,
    },
    /// A Goal policy applicable through the selected goal chain.
    Goal {
        /// The owning project identity.
        project_id: String,
        /// The owning Goal identity.
        goal_id: String,
    },
    /// A session policy applicable to its owner session.
    Session {
        /// The owning project identity.
        project_id: String,
        /// The policy owner session identity.
        owner_session_id: String,
    },
}

impl ProgrammaticPolicyScopeDto {
    /// Returns the owning project identity.
    #[must_use]
    pub fn project_id(&self) -> &str {
        match self {
            Self::Project { project_id }
            | Self::Goal { project_id, .. }
            | Self::Session { project_id, .. } => project_id,
        }
    }
    /// Returns the owning Goal identity of a Goal policy.
    #[must_use]
    pub fn goal_id(&self) -> Option<&str> {
        match self {
            Self::Goal { goal_id, .. } => Some(goal_id),
            Self::Project { .. } | Self::Session { .. } => None,
        }
    }
    /// Returns the policy owner session identity of a session policy.
    #[must_use]
    pub fn owner_session_id(&self) -> Option<&str> {
        match self {
            Self::Session {
                owner_session_id, ..
            } => Some(owner_session_id),
            Self::Project { .. } | Self::Goal { .. } => None,
        }
    }
    /// Returns the stable durable scope discriminator.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Project { .. } => "project",
            Self::Goal { .. } => "goal",
            Self::Session { .. } => "session",
        }
    }
    /// Validates this owner scope.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_not_applicable` for a blank, over-long,
    /// control-bearing, or credential-shaped scope identity.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("programmatic_policy_not_applicable", self.project_id())?;
        for identity in [self.goal_id(), self.owner_session_id()]
            .into_iter()
            .flatten()
        {
            validate_safe_label("programmatic_policy_not_applicable", identity)?;
        }
        Ok(())
    }
}

/// The closed root-origin kinds of one programmatic action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticRootOriginKindDto {
    /// The root of an ordinary user-admitted run and all of its descendants.
    InteractiveUser,
    /// The root of one separately admitted harness launch and descendants.
    ContinualHarness,
}

impl ProgrammaticRootOriginKindDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::InteractiveUser => "interactive_user",
            Self::ContinualHarness => "continual_harness",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_origin_invalid` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "interactive_user" => Ok(Self::InteractiveUser),
            "continual_harness" => Ok(Self::ContinualHarness),
            _ => Err(intention_types::ErrorDto::validation(
                "programmatic_policy_origin_invalid",
                "the durable programmatic root origin kind is unknown",
            )),
        }
    }
}

/// The closed admission decisions of one policy rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticAdmissionDecisionDto {
    /// The call is prohibited.
    Prohibited,
    /// A narrow direct local read admitted only for `InteractiveUser`.
    DirectLocalRead,
    /// One exact durable user confirmation is required.
    ExactConfirmationRequired,
    /// A bounded user-approved confirmation corridor is required.
    BoundedConfirmationRequired,
}

impl ProgrammaticAdmissionDecisionDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Prohibited => "prohibited",
            Self::DirectLocalRead => "direct_local_read",
            Self::ExactConfirmationRequired => "exact_confirmation_required",
            Self::BoundedConfirmationRequired => "bounded_confirmation_required",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_input_constraint_mismatch` for an unknown
    /// name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "prohibited" => Ok(Self::Prohibited),
            "direct_local_read" => Ok(Self::DirectLocalRead),
            "exact_confirmation_required" => Ok(Self::ExactConfirmationRequired),
            "bounded_confirmation_required" => Ok(Self::BoundedConfirmationRequired),
            _ => Err(intention_types::ErrorDto::validation(
                "programmatic_policy_input_constraint_mismatch",
                "the durable admission decision is unknown",
            )),
        }
    }
}

/// The immutable calendar period kind of one policy identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticCalendarPeriodKindDto {
    /// One calendar day window.
    Day,
    /// One calendar week window.
    Week,
    /// One calendar month window.
    Month,
}

impl ProgrammaticCalendarPeriodKindDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_revision_conflict` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "day" => Ok(Self::Day),
            "week" => Ok(Self::Week),
            "month" => Ok(Self::Month),
            _ => Err(intention_types::ErrorDto::validation(
                "programmatic_policy_revision_conflict",
                "the durable calendar period kind is unknown",
            )),
        }
    }
}

/// The closed durable lifecycle state of one policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticPolicyLifecycleStateDto {
    /// The policy admits through its active revision.
    Active,
    /// The policy blocks every not-yet-started matching call.
    Suspended,
    /// The policy denies new reservations permanently.
    Revoked,
    /// The terminal policy is retained as readable history.
    Archived,
}

impl ProgrammaticPolicyLifecycleStateDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Revoked => "revoked",
            Self::Archived => "archived",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_not_applicable` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "active" => Ok(Self::Active),
            "suspended" => Ok(Self::Suspended),
            "revoked" => Ok(Self::Revoked),
            "archived" => Ok(Self::Archived),
            _ => Err(intention_types::ErrorDto::validation(
                "programmatic_policy_not_applicable",
                "the durable policy lifecycle state is unknown",
            )),
        }
    }
}

/// The closed lifecycle operation applied to one policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticPolicyLifecycleOperationDto {
    /// Suspend every not-yet-started matching call.
    Suspend,
    /// Resume a suspended policy on its then-current active revision.
    Resume,
    /// Atomically create a disabled revision and enter `Revoked`.
    Revoke,
    /// Archive a revoked policy with no active dependent tree.
    Archive,
}

impl ProgrammaticPolicyLifecycleOperationDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Suspend => "suspend",
            Self::Resume => "resume",
            Self::Revoke => "revoke",
            Self::Archive => "archive",
        }
    }
}

/// The closed state of one exact durable confirmation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticConfirmationStateDto {
    /// The confirmation awaits the root-only user decision.
    Awaiting,
    /// The user accepted the exact binding.
    Accepted,
    /// The user rejected the exact binding.
    Rejected,
    /// The confirmation expired without a decision.
    Expired,
    /// The confirmation was cancelled before a decision.
    Cancelled,
}

impl ProgrammaticConfirmationStateDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Awaiting => "awaiting",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_confirmation_required` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "awaiting" => Ok(Self::Awaiting),
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            "expired" => Ok(Self::Expired),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(intention_types::ErrorDto::validation(
                "programmatic_policy_confirmation_required",
                "the durable confirmation state is unknown",
            )),
        }
    }
}

/// The closed state of one bounded confirmation corridor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticCorridorStateDto {
    /// The corridor admits bounded actions of one active root tree.
    Active,
    /// The corridor expired at root terminalization or daemon restart.
    Expired,
    /// The corridor consumed its maximum action count.
    Exhausted,
    /// The corridor was revoked or suspended with its policy.
    Revoked,
}

impl ProgrammaticCorridorStateDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Expired => "expired",
            Self::Exhausted => "exhausted",
            Self::Revoked => "revoked",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_corridor_unavailable` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "active" => Ok(Self::Active),
            "expired" => Ok(Self::Expired),
            "exhausted" => Ok(Self::Exhausted),
            "revoked" => Ok(Self::Revoked),
            _ => Err(intention_types::ErrorDto::validation(
                "programmatic_policy_corridor_unavailable",
                "the durable corridor state is unknown",
            )),
        }
    }
}

/// The closed states of one admission reservation.
///
/// A reservation is created `Reserved` before `ToolCallStarted`, released on a
/// known pre-effect outcome, made permanently consumed on `ToolCallStarted`,
/// released by recovery before the start, or retained as an unknown external
/// effect that is never retried.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticReservationStateDto {
    /// The reservation is outstanding before `ToolCallStarted`.
    Reserved,
    /// The call reached `ToolCallStarted`; the consumption is permanent.
    PermanentOnStart,
    /// A known pre-effect outcome released the reservation atomically.
    ReleasedOnKnownPreEffect,
    /// Recovery released the reservation before any start.
    InterruptedBeforeStart,
    /// A started ambiguous action retains its unknown-effect evidence.
    ExternalEffectUnknown,
}

impl ProgrammaticReservationStateDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::PermanentOnStart => "permanent_on_start",
            Self::ReleasedOnKnownPreEffect => "released_on_known_pre_effect",
            Self::InterruptedBeforeStart => "interrupted_before_start",
            Self::ExternalEffectUnknown => "external_effect_unknown",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_reservation_conflict` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "reserved" => Ok(Self::Reserved),
            "permanent_on_start" => Ok(Self::PermanentOnStart),
            "released_on_known_pre_effect" => Ok(Self::ReleasedOnKnownPreEffect),
            "interrupted_before_start" => Ok(Self::InterruptedBeforeStart),
            "external_effect_unknown" => Ok(Self::ExternalEffectUnknown),
            _ => Err(intention_types::ErrorDto::validation(
                "programmatic_policy_reservation_conflict",
                "the durable reservation state is unknown",
            )),
        }
    }
    /// Whether this state still consumes an outstanding reservation.
    #[must_use]
    pub const fn consumes_outstanding_reservation(self) -> bool {
        matches!(self, Self::Reserved)
    }
}

/// The closed durable state of one inactive policy draft.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticPolicyDraftStateDto {
    /// The draft awaits the root-only user decision.
    Pending,
    /// The user accepted the draft and an immutable revision was created.
    Accepted,
    /// The user rejected the draft; no policy changed.
    Rejected,
}

impl ProgrammaticPolicyDraftStateDto {
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
    /// Returns `programmatic_policy_draft_conflict` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "pending" => Ok(Self::Pending),
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            _ => Err(intention_types::ErrorDto::validation(
                "programmatic_policy_draft_conflict",
                "the durable policy draft state is unknown",
            )),
        }
    }
}

/// The typed coalescing outcome of one incoming policy proposal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticDraftCoalescingDto {
    /// The proposal started a new pending draft.
    CreatedNew,
    /// The equal proposal added evidence to the one pending draft.
    CoalescedIntoPending,
}

impl ProgrammaticDraftCoalescingDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CreatedNew => "created_new",
            Self::CoalescedIntoPending => "coalesced_into_pending",
        }
    }
}

/// One immutable revision of a durable programmatic-caller policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticPolicyRevisionRecordDto {
    /// The owning policy identity.
    pub policy_id: String,
    /// The immutable revision number, starting at one.
    pub revision: u64,
    /// The ordered root-origin applicability rules.
    pub root_origin_rules: Vec<ProgrammaticRootOriginRuleRecordDto>,
    /// The ordered admission rule decisions of this revision.
    pub admission_decisions: Vec<ProgrammaticAdmissionDecisionDto>,
    /// The selected per-run action limit.
    pub max_actions_per_run: u64,
    /// The selected per-run concurrency limit.
    pub max_concurrent_actions_per_run: u64,
    /// The immutable calendar period kind of the owning policy.
    pub calendar_period_kind: ProgrammaticCalendarPeriodKindDto,
    /// The selected calendar action limit for one window.
    pub calendar_max_actions: u64,
    /// The immutable inherited policy references.
    pub inherited_policy_references: Vec<ProgrammaticPolicyRevisionReferenceDto>,
    /// The canonical revision digest.
    pub canonical_revision_digest: String,
}

/// One ordered root-origin applicability rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgrammaticRootOriginRuleRecordDto {
    /// The closed root-origin kind the rule applies to.
    pub root_origin_kind: ProgrammaticRootOriginKindDto,
    /// The most permissive decision the rule allows.
    pub maximum_decision: ProgrammaticAdmissionDecisionDto,
}

/// One immutable reference to a policy revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticPolicyRevisionReferenceDto {
    /// The referenced policy identity.
    pub policy_id: String,
    /// The exact referenced revision.
    pub revision: u64,
}

impl ProgrammaticPolicyRevisionReferenceDto {
    /// Validates this immutable revision reference.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_revision_conflict` for a blank identity or
    /// a zero revision.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("programmatic_policy_revision_conflict", &self.policy_id)?;
        if self.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_revision_conflict",
                "a policy revision reference is exact and nonzero",
            ));
        }
        Ok(())
    }
}

impl ProgrammaticPolicyRevisionRecordDto {
    /// Validates this immutable policy revision record.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_revision_conflict` for a zero revision or
    /// an unchanged calendar period kind, `programmatic_policy_origin_invalid`
    /// for an empty, duplicated, or over-limit root-origin rule set,
    /// `programmatic_policy_limit_exceeded` when a rule, limit, or inheritance
    /// bound is exceeded, and `programmatic_policy_snapshot_too_large` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("programmatic_policy_revision_conflict", &self.policy_id)?;
        validate_safe_digest(
            "programmatic_policy_revision_conflict",
            &self.canonical_revision_digest,
        )?;
        if self.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_revision_conflict",
                "a policy revision starts at revision one",
            ));
        }
        if self.root_origin_rules.is_empty() || self.root_origin_rules.len() > 2 {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_origin_invalid",
                "a revision declares at most the two closed root origins",
            ));
        }
        for (index, rule) in self.root_origin_rules.iter().enumerate() {
            if self.root_origin_rules[..index]
                .iter()
                .any(|other| other.root_origin_kind == rule.root_origin_kind)
            {
                return Err(intention_types::ErrorDto::validation(
                    "programmatic_policy_origin_invalid",
                    "a revision declares each closed root origin at most once",
                ));
            }
        }
        if self.admission_decisions.len()
            > intention_domain::programmatic_policy::MAX_RULES_PER_POLICY_REVISION
            || self.inherited_policy_references.len() > MAX_POLICY_INHERITED_REFERENCES
        {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_limit_exceeded",
                "a revision stays inside its closed rule and inheritance bounds",
            ));
        }
        if self.max_actions_per_run == 0
            || self.max_actions_per_run
                > intention_domain::programmatic_policy::MAX_POLICY_ACTIONS_PER_RUN
            || self.max_concurrent_actions_per_run == 0
            || self.max_concurrent_actions_per_run
                > intention_domain::programmatic_policy::MAX_POLICY_CONCURRENT_ACTIONS_PER_RUN
        {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_limit_exceeded",
                "the selected run limits stay inside the code-owned maxima",
            ));
        }
        if self.calendar_max_actions
            < intention_domain::programmatic_policy::MIN_CALENDAR_ACTION_LIMIT
            || self.calendar_max_actions
                > intention_domain::programmatic_policy::MAX_CALENDAR_ACTION_LIMIT
        {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_limit_exceeded",
                "the selected calendar limit stays inside the code-owned range",
            ));
        }
        for reference in &self.inherited_policy_references {
            reference.validate()?;
        }
        Ok(())
    }
}

/// One durable programmatic-caller policy identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticPolicyRecordDto {
    /// The daemon-assigned policy identity.
    pub policy_id: String,
    /// The immutable owner scope.
    pub scope: ProgrammaticPolicyScopeDto,
    /// The immutable calendar period kind selected at creation.
    pub calendar_period_kind: ProgrammaticCalendarPeriodKindDto,
    /// The closed lifecycle state.
    pub lifecycle_state: ProgrammaticPolicyLifecycleStateDto,
    /// The current active immutable revision.
    pub active_revision: u64,
    /// The canonical policy identity digest.
    pub canonical_policy_digest: String,
    /// The last durable policy update time in Unix milliseconds.
    pub updated_at_ms: u64,
}

impl ProgrammaticPolicyRecordDto {
    /// Validates this durable policy identity.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_revision_conflict` for a zero active
    /// revision and the scope and digest validation failures.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("programmatic_policy_revision_conflict", &self.policy_id)?;
        validate_safe_digest(
            "programmatic_policy_revision_conflict",
            &self.canonical_policy_digest,
        )?;
        self.scope.validate()?;
        if self.active_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_revision_conflict",
                "a policy starts at active revision one",
            ));
        }
        Ok(())
    }
}

/// Input creating one durable policy identity with its first revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateProgrammaticPolicyInputDto {
    /// The durable policy identity.
    pub policy: ProgrammaticPolicyRecordDto,
    /// The first immutable revision bound to that policy.
    pub revision: ProgrammaticPolicyRevisionRecordDto,
}

impl CreateProgrammaticPolicyInputDto {
    /// Validates this creation input as one coherent policy.
    ///
    /// # Errors
    ///
    /// Returns the record validation failures and
    /// `programmatic_policy_revision_conflict` when the revision does not
    /// belong to the policy, is not revision one, or changes the immutable
    /// calendar period kind.
    pub fn validate(&self) -> DtoResult<()> {
        self.policy.validate()?;
        self.revision.validate()?;
        if self.revision.policy_id != self.policy.policy_id
            || self.revision.revision != 1
            || self.policy.active_revision != 1
            || self.revision.calendar_period_kind != self.policy.calendar_period_kind
        {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_revision_conflict",
                "a new policy starts at its own revision one and keeps its period kind",
            ));
        }
        Ok(())
    }
}

/// Input appending one immutable policy revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendProgrammaticPolicyRevisionInputDto {
    /// The owning policy identity.
    pub policy_id: String,
    /// The exact active revision the caller observed.
    pub expected_revision: u64,
    /// The next immutable revision to append and activate.
    pub revision: ProgrammaticPolicyRevisionRecordDto,
}

impl AppendProgrammaticPolicyRevisionInputDto {
    /// Validates this revision append.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_revision_conflict` when the append does
    /// not continue the exact observed active revision of the same policy.
    pub fn validate(&self) -> DtoResult<()> {
        self.revision.validate()?;
        let next = self.expected_revision.checked_add(1).ok_or_else(|| {
            intention_types::ErrorDto::validation(
                "programmatic_policy_revision_conflict",
                "the active revision cannot be continued",
            )
        })?;
        if self.revision.policy_id != self.policy_id || self.revision.revision != next {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_revision_conflict",
                "the append does not continue the active policy revision",
            ));
        }
        Ok(())
    }
}

/// Input applying one lifecycle operation to a durable policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransitionProgrammaticPolicyLifecycleInputDto {
    /// The owning policy identity.
    pub policy_id: String,
    /// The exact active revision the caller observed.
    pub expected_revision: u64,
    /// The typed lifecycle operation to apply.
    pub operation: ProgrammaticPolicyLifecycleOperationDto,
    /// Whether any active root tree currently selected this policy.
    pub has_active_dependent_tree: bool,
    /// The durable operation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// The immutable effective policy snapshot frozen at admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticPolicySnapshotRecordDto {
    /// The daemon-assigned snapshot identity.
    pub snapshot_id: String,
    /// The resolved root-origin kind.
    pub root_origin_kind: ProgrammaticRootOriginKindDto,
    /// The selected policy revision references in canonical order.
    pub policy_references: Vec<ProgrammaticPolicyRevisionReferenceDto>,
    /// The most restrictive selected decision ceiling.
    pub decision_ceiling: ProgrammaticAdmissionDecisionDto,
    /// The narrowest selected per-run action limit.
    pub max_actions_per_run: u64,
    /// The narrowest selected per-run concurrency limit.
    pub max_concurrent_actions_per_run: u64,
    /// The selected calendar limits in canonical policy order.
    pub calendar_limits: Vec<ProgrammaticCalendarLimitRecordDto>,
    /// The calendar counter identities of every selected policy.
    pub calendar_counter_references: Vec<ProgrammaticCalendarCounterReferenceDto>,
    /// The code-owned interactive baseline action bound when it applies.
    pub baseline_max_actions: Option<u64>,
    /// The code-owned interactive baseline concurrency bound when it applies.
    pub baseline_max_concurrent_actions: Option<u64>,
    /// The canonical snapshot digest.
    pub snapshot_digest: String,
    /// The admission time in Unix milliseconds.
    pub created_at_ms: u64,
}

/// One selected calendar limit of one policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgrammaticCalendarLimitRecordDto {
    /// The owning policy identity hash is named by the snapshot references.
    pub period_kind: ProgrammaticCalendarPeriodKindDto,
    /// The selected action limit for one window.
    pub max_actions: u64,
}

/// One calendar counter identity of one selected policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticCalendarCounterReferenceDto {
    /// The policy identity that owns the calendar counter.
    pub policy_id: String,
    /// The immutable calendar period kind.
    pub period_kind: ProgrammaticCalendarPeriodKindDto,
}

impl ProgrammaticPolicySnapshotRecordDto {
    /// Validates this immutable effective snapshot.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_snapshot_unavailable` for an empty policy
    /// reference set, `programmatic_policy_snapshot_too_large` when the
    /// reference bound is exceeded, and `programmatic_policy_limit_exceeded`
    /// for unusable bounds.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label(
            "programmatic_policy_snapshot_unavailable",
            &self.snapshot_id,
        )?;
        validate_safe_digest(
            "programmatic_policy_snapshot_unavailable",
            &self.snapshot_digest,
        )?;
        if self.policy_references.is_empty() {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_snapshot_unavailable",
                "an effective snapshot names at least one selected policy",
            ));
        }
        if self.policy_references.len() > MAX_POLICY_SNAPSHOT_REFERENCES
            || self.calendar_limits.len() > MAX_POLICY_SNAPSHOT_REFERENCES
            || self.calendar_counter_references.len() > MAX_POLICY_SNAPSHOT_REFERENCES
        {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_snapshot_too_large",
                "an effective snapshot stays inside its selected-record bounds",
            ));
        }
        for reference in &self.policy_references {
            reference.validate()?;
        }
        for counter in &self.calendar_counter_references {
            validate_safe_label(
                "programmatic_policy_counter_unavailable",
                &counter.policy_id,
            )?;
        }
        if self.max_actions_per_run == 0 || self.max_concurrent_actions_per_run == 0 {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_limit_exceeded",
                "an effective snapshot carries nonzero run bounds",
            ));
        }
        Ok(())
    }
}

/// One exact durable user confirmation bound to one tool call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticPolicyConfirmationRecordDto {
    /// The daemon-assigned confirmation identity.
    pub confirmation_id: String,
    /// The root session identity of the confirmed tree.
    pub root_session_id: String,
    /// The root run identity of the confirmed tree.
    pub root_run_id: String,
    /// The one bound tool-call identity.
    pub tool_call_id: String,
    /// The selected tool identifier.
    pub tool_id: String,
    /// The exact selected descriptor revision.
    pub descriptor_revision: String,
    /// The selected MCP method reference when the tool is `mcp`.
    pub mcp_method_reference: Option<String>,
    /// The exact typed input digest of the bound call.
    pub typed_input_digest: String,
    /// The applicable policy snapshot digest of the bound call.
    pub policy_snapshot_digest: String,
    /// The current durable confirmation state.
    pub state: ProgrammaticConfirmationStateDto,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
    /// The durable decision time in Unix milliseconds, when one exists.
    pub decided_at_ms: Option<u64>,
}

impl ProgrammaticPolicyConfirmationRecordDto {
    /// Validates this exact confirmation binding.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_confirmation_required` for a missing
    /// identity binding and `programmatic_policy_confirmation_expired` for a
    /// decision time without a decided state.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label(
            "programmatic_policy_confirmation_required",
            &self.confirmation_id,
        )?;
        for identity in [
            &self.root_session_id,
            &self.root_run_id,
            &self.tool_call_id,
            &self.tool_id,
            &self.descriptor_revision,
        ] {
            validate_safe_label("programmatic_policy_confirmation_required", identity)?;
        }
        validate_safe_digest(
            "programmatic_policy_confirmation_required",
            &self.typed_input_digest,
        )?;
        validate_safe_digest(
            "programmatic_policy_confirmation_required",
            &self.policy_snapshot_digest,
        )?;
        if let Some(reference) = &self.mcp_method_reference {
            validate_safe_label("programmatic_policy_confirmation_required", reference)?;
        }
        if self.state == ProgrammaticConfirmationStateDto::Awaiting && self.decided_at_ms.is_some()
        {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_confirmation_expired",
                "an awaiting confirmation carries no decision time",
            ));
        }
        Ok(())
    }
}

/// Input deciding one exact durable confirmation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecideProgrammaticPolicyConfirmationInputDto {
    /// The bound confirmation identity.
    pub confirmation_id: String,
    /// The exact bound tool-call identity the decision must match.
    pub tool_call_id: String,
    /// The exact typed input digest the decision must match.
    pub typed_input_digest: String,
    /// The closed decision state.
    pub state: ProgrammaticConfirmationStateDto,
    /// The durable decision time in Unix milliseconds.
    pub decided_at_ms: u64,
}

/// One bounded confirmation corridor of one active root tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticAuthorizationCorridorRecordDto {
    /// The daemon-assigned corridor identity and canonical digest.
    pub corridor_digest: String,
    /// The root session identity of the corridor's active tree.
    pub root_session_id: String,
    /// The root run identity of the corridor's active tree.
    pub root_run_id: String,
    /// The resolved root-origin kind.
    pub root_origin_kind: ProgrammaticRootOriginKindDto,
    /// The effective policy snapshot reference of the corridor.
    pub policy_snapshot_reference: String,
    /// The required effect selectors.
    pub required_effect_selectors: Vec<String>,
    /// The exact tool or MCP method selectors.
    pub exact_tool_or_method_selectors: Vec<String>,
    /// The selected descriptor input constraint selections.
    pub descriptor_input_constraint_selections: Vec<String>,
    /// The maximum action count of the corridor.
    pub maximum_action_count: u64,
    /// The maximum concurrent action count of the corridor.
    pub maximum_concurrent_actions: u64,
    /// The bound confirmation reference.
    pub confirmation_reference: String,
    /// The closed corridor state.
    pub state: ProgrammaticCorridorStateDto,
    /// The consumed action count.
    pub consumed_action_count: u64,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl ProgrammaticAuthorizationCorridorRecordDto {
    /// Validates this bounded corridor record.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_corridor_unavailable` for a missing
    /// binding, `programmatic_policy_corridor_exhausted` when the consumed
    /// count exceeds the bound, and `programmatic_policy_limit_exceeded` for
    /// an over-limit selector count.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_digest(
            "programmatic_policy_corridor_unavailable",
            &self.corridor_digest,
        )?;
        for identity in [
            &self.root_session_id,
            &self.root_run_id,
            &self.policy_snapshot_reference,
            &self.confirmation_reference,
        ] {
            validate_safe_label("programmatic_policy_corridor_unavailable", identity)?;
        }
        for selectors in [
            &self.required_effect_selectors,
            &self.exact_tool_or_method_selectors,
            &self.descriptor_input_constraint_selections,
        ] {
            if selectors.is_empty() || selectors.len() > MAX_POLICY_SELECTORS {
                return Err(intention_types::ErrorDto::validation(
                    "programmatic_policy_limit_exceeded",
                    "a corridor carries at most sixteen bounded selectors per family",
                ));
            }
            validate_safe_labels(
                "programmatic_policy_corridor_unavailable",
                selectors,
                MAX_POLICY_SELECTORS,
            )?;
        }
        if self.maximum_action_count == 0
            || self.maximum_concurrent_actions == 0
            || self.maximum_concurrent_actions > self.maximum_action_count
        {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_limit_exceeded",
                "a corridor carries coherent nonzero action bounds",
            ));
        }
        if self.consumed_action_count > self.maximum_action_count {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_corridor_exhausted",
                "a corridor never consumes more than its remaining count",
            ));
        }
        Ok(())
    }
}

/// Input consuming one action of a bounded corridor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsumeProgrammaticCorridorActionInputDto {
    /// The corridor digest identity.
    pub corridor_digest: String,
    /// The exact root run identity the corridor belongs to.
    pub root_run_id: String,
    /// The durable consumption time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One inactive model-prepared policy draft awaiting a user decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticPolicyDraftRecordDto {
    /// The daemon-assigned draft identity.
    pub draft_id: String,
    /// The immutable owner scope of the draft.
    pub scope: ProgrammaticPolicyScopeDto,
    /// The owner-scope record kind of the proposal.
    pub record_kind: String,
    /// The exact base policy revision.
    pub base_revision: u64,
    /// The bounded typed evidence references of the proposal.
    pub evidence_references: Vec<String>,
    /// The bounded safe rationale.
    pub safe_rationale: String,
    /// The canonical draft digest.
    pub canonical_draft_digest: String,
    /// The durable draft state.
    pub state: ProgrammaticPolicyDraftStateDto,
    /// The coalesced evidence count.
    pub coalesced_evidence_count: u64,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
    /// The durable decision time in Unix milliseconds, when one exists.
    pub decided_at_ms: Option<u64>,
}

impl ProgrammaticPolicyDraftRecordDto {
    /// Validates this inactive policy draft.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_draft_conflict` for a missing identity or
    /// an empty edit set and `programmatic_policy_draft_too_large` when the
    /// safe content exceeds its bound.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("programmatic_policy_draft_conflict", &self.draft_id)?;
        validate_safe_label("programmatic_policy_draft_conflict", &self.record_kind)?;
        validate_safe_digest(
            "programmatic_policy_draft_conflict",
            &self.canonical_draft_digest,
        )?;
        self.scope.validate()?;
        if self.base_revision == 0 || self.evidence_references.is_empty() {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_draft_conflict",
                "a draft carries exact base revisions and evidence",
            ));
        }
        if self.evidence_references.len() > MAX_POLICY_SELECTORS {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_draft_too_large",
                "the draft evidence bound is exceeded",
            ));
        }
        validate_safe_labels(
            "programmatic_policy_draft_conflict",
            &self.evidence_references,
            MAX_POLICY_SELECTORS,
        )?;
        validate_safe_content(
            "programmatic_policy_draft_conflict",
            "programmatic_policy_draft_too_large",
            &self.safe_rationale,
        )
    }
}

/// The durable counter state of one policy identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticPolicyCounterRecordDto {
    /// The owning policy identity.
    pub policy_id: String,
    /// The permanently consumed run actions.
    pub run_started_actions: u64,
    /// The outstanding run reservations.
    pub run_reserved_actions: u64,
    /// The started run actions that have not terminally finished.
    pub run_in_flight_actions: u64,
    /// The permanently consumed calendar actions of the current window.
    pub calendar_started_actions: u64,
    /// The outstanding calendar reservations of the current window.
    pub calendar_reserved_actions: u64,
    /// The current calendar window start in Unix milliseconds.
    pub calendar_window_start_ms: u64,
    /// The current calendar window end in Unix milliseconds.
    pub calendar_window_end_ms: u64,
    /// The project time zone retained by the current window.
    pub calendar_window_time_zone: String,
    /// The last counter update time in Unix milliseconds.
    pub updated_at_ms: u64,
}

impl ProgrammaticPolicyCounterRecordDto {
    /// Validates this durable counter state.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_counter_unavailable` for an incoherent
    /// window or a blank policy identity.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("programmatic_policy_counter_unavailable", &self.policy_id)?;
        validate_safe_label(
            "programmatic_policy_counter_unavailable",
            &self.calendar_window_time_zone,
        )?;
        if self.calendar_window_end_ms <= self.calendar_window_start_ms {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_counter_unavailable",
                "a calendar window has a positive bounded duration",
            ));
        }
        Ok(())
    }
}

/// One admission-time policy counter reservation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticPolicyReservationRecordDto {
    /// The durable reservation identity.
    pub reservation_reference: String,
    /// The owning policy identity.
    pub policy_id: String,
    /// The exact policy revision that admitted the call.
    pub policy_revision: u64,
    /// The root run identity that consumes the reservation.
    pub root_run_id: String,
    /// The one reserved tool-call identity.
    pub tool_call_id: String,
    /// The exact typed input digest of the reserved call.
    pub typed_input_digest: String,
    /// The calendar counter identity this reservation consumes.
    pub calendar_counter_reference: String,
    /// The reservation time in Unix milliseconds.
    pub reserved_at_ms: u64,
    /// The durable reservation state.
    pub state: ProgrammaticReservationStateDto,
    /// The terminal state time in Unix milliseconds, when one exists.
    pub finished_at_ms: Option<u64>,
}

impl ProgrammaticPolicyReservationRecordDto {
    /// Validates this admission reservation.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_reservation_conflict` for a missing exact
    /// binding and `programmatic_policy_origin_invalid` for an unusable
    /// identity.
    pub fn validate(&self) -> DtoResult<()> {
        for identity in [
            &self.reservation_reference,
            &self.policy_id,
            &self.root_run_id,
            &self.tool_call_id,
            &self.calendar_counter_reference,
        ] {
            validate_safe_label("programmatic_policy_origin_invalid", identity)?;
        }
        validate_safe_digest(
            "programmatic_policy_reservation_conflict",
            &self.typed_input_digest,
        )?;
        if self.policy_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "programmatic_policy_reservation_conflict",
                "a reservation names its exact admitting revision",
            ));
        }
        Ok(())
    }
}

/// Input atomically reserving one run and calendar action before
/// `ToolCallStarted`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReserveProgrammaticPolicyActionInputDto {
    /// The reservation to create or replay.
    pub reservation: ProgrammaticPolicyReservationRecordDto,
    /// The per-run action limit of the selected effective policy.
    pub max_actions_per_run: u64,
    /// The per-run concurrency limit of the selected effective policy.
    pub max_concurrent_actions_per_run: u64,
    /// The effective calendar action limit of the current window.
    pub calendar_max_actions: u64,
    /// The current calendar window start in Unix milliseconds.
    pub calendar_window_start_ms: u64,
    /// The current calendar window end in Unix milliseconds.
    pub calendar_window_end_ms: u64,
    /// The project time zone of the current window.
    pub calendar_window_time_zone: String,
}

/// The outcome of one atomic reservation attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReserveProgrammaticPolicyActionOutcomeDto {
    /// The accepted reservation record.
    pub reservation: ProgrammaticPolicyReservationRecordDto,
    /// The resulting durable counters.
    pub counters: ProgrammaticPolicyCounterRecordDto,
    /// Whether the equal repeated operation replayed an existing binding.
    pub replayed: bool,
}

/// Input releasing one known pre-effect reservation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseProgrammaticPolicyReservationInputDto {
    /// The exact reservation reference to release.
    pub reservation_reference: String,
    /// The exact bound tool-call identity the release must match.
    pub tool_call_id: String,
    /// The durable release time in Unix milliseconds.
    pub released_at_ms: u64,
}

/// Input making one reservation permanently consumed at `ToolCallStarted`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitProgrammaticReservationStartedInputDto {
    /// The exact reservation reference to commit.
    pub reservation_reference: String,
    /// The exact bound tool-call identity the commit must match.
    pub tool_call_id: String,
    /// The durable start time in Unix milliseconds.
    pub started_at_ms: u64,
}

/// Input applying one recovery disposition to a reservation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoverProgrammaticPolicyReservationInputDto {
    /// The exact reservation reference to recover.
    pub reservation_reference: String,
    /// Whether the recovered action reached `ToolCallStarted`.
    pub tool_call_started: bool,
    /// The durable recovery time in Unix milliseconds.
    pub recovered_at_ms: u64,
}

/// DTO-only repository contract for durable policy identities, revisions,
/// snapshots, and lifecycle.
pub trait ProgrammaticPolicyRepositoryDto {
    /// Creates one durable policy identity with its first immutable revision.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_limit_exceeded` when the project or goal
    /// policy bound is exceeded, `programmatic_policy_revision_conflict` when
    /// the identity is already bound to different content, or an unavailable
    /// error when the atomic commit fails.
    fn create_programmatic_policy(
        &self,
        input: CreateProgrammaticPolicyInputDto,
    ) -> DtoResult<ProgrammaticPolicyRecordDto>;
    /// Loads one durable policy identity.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_not_applicable` when the policy does not
    /// exist, or an unavailable error when it cannot be read.
    fn load_programmatic_policy(&self, policy_id: String)
    -> DtoResult<ProgrammaticPolicyRecordDto>;
    /// Loads one immutable policy revision.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_revision_conflict` when the exact revision
    /// is absent, or an unavailable error when it cannot be read.
    fn load_programmatic_policy_revision(
        &self,
        policy_id: String,
        revision: u64,
    ) -> DtoResult<ProgrammaticPolicyRevisionRecordDto>;
    /// Appends one immutable revision and moves the active revision in one
    /// atomic transaction guarded by the exact expected revision.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_revision_conflict` when the expected
    /// revision is stale or the identity is already bound to different content,
    /// or an unavailable error when the atomic commit fails.
    fn append_programmatic_policy_revision(
        &self,
        input: AppendProgrammaticPolicyRevisionInputDto,
    ) -> DtoResult<ProgrammaticPolicyRecordDto>;
    /// Applies one lifecycle operation to a durable policy.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_revision_conflict` for a stale expected
    /// revision, `programmatic_policy_suspended` for an operation that the
    /// durable state does not permit, `programmatic_policy_revoked` for a
    /// reactivation attempt, or an unavailable error when the atomic commit
    /// fails.
    fn transition_programmatic_policy_lifecycle(
        &self,
        input: TransitionProgrammaticPolicyLifecycleInputDto,
    ) -> DtoResult<ProgrammaticPolicyRecordDto>;
    /// Counts the durable policies owned by one project.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the count cannot be read.
    fn count_programmatic_policies_in_project(&self, project_id: String) -> DtoResult<u64>;
    /// Counts the durable policies attached to one Goal.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the count cannot be read.
    fn count_programmatic_policies_on_goal(&self, goal_id: String) -> DtoResult<u64>;
    /// Stores one immutable effective policy snapshot frozen at admission.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_snapshot_too_large` when the snapshot
    /// exceeds its bound, `programmatic_policy_snapshot_unavailable` when it
    /// cannot be reconstituted, or an unavailable error when the commit fails.
    fn store_programmatic_policy_snapshot(
        &self,
        input: ProgrammaticPolicySnapshotRecordDto,
    ) -> DtoResult<ProgrammaticPolicySnapshotRecordDto>;
    /// Loads one immutable effective policy snapshot.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_snapshot_unavailable` when the snapshot is
    /// absent, or an unavailable error when it cannot be read.
    fn load_programmatic_policy_snapshot(
        &self,
        snapshot_id: String,
    ) -> DtoResult<ProgrammaticPolicySnapshotRecordDto>;
}

/// DTO-only repository contract for exact confirmations, bounded corridors, and
/// inactive policy drafts.
pub trait ProgrammaticConfirmationRepositoryDto {
    /// Creates one exact durable confirmation bound to one tool call.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_confirmation_required` when the identity or
    /// binding is unusable, `programmatic_policy_limit_exceeded` when the
    /// awaiting-confirmation bound of the root tree is exceeded, or an
    /// unavailable error when the atomic commit fails.
    fn create_programmatic_policy_confirmation(
        &self,
        input: ProgrammaticPolicyConfirmationRecordDto,
    ) -> DtoResult<ProgrammaticPolicyConfirmationRecordDto>;
    /// Loads one durable confirmation by its bound tool-call identity.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the confirmation cannot be read.
    fn load_programmatic_policy_confirmation(
        &self,
        tool_call_id: String,
    ) -> DtoResult<Option<ProgrammaticPolicyConfirmationRecordDto>>;
    /// Decides one exact durable confirmation without replaying its binding.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_confirmation_required` when the binding
    /// does not match, `programmatic_policy_confirmation_expired` when the
    /// confirmation is no longer awaiting a decision, or an unavailable error
    /// when the atomic commit fails.
    fn decide_programmatic_policy_confirmation(
        &self,
        input: DecideProgrammaticPolicyConfirmationInputDto,
    ) -> DtoResult<ProgrammaticPolicyConfirmationRecordDto>;
    /// Creates one bounded confirmation corridor of one active root tree.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_corridor_unavailable` when the binding is
    /// unusable, `programmatic_policy_limit_exceeded` when the unfinished
    /// corridor bound of the root tree is exceeded, or an unavailable error
    /// when the atomic commit fails.
    fn create_programmatic_authorization_corridor(
        &self,
        input: ProgrammaticAuthorizationCorridorRecordDto,
    ) -> DtoResult<ProgrammaticAuthorizationCorridorRecordDto>;
    /// Loads the active corridor of one root tree, if any.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the corridor cannot be read.
    fn load_active_programmatic_corridor(
        &self,
        root_run_id: String,
    ) -> DtoResult<Option<ProgrammaticAuthorizationCorridorRecordDto>>;
    /// Consumes one action of a bounded corridor atomically.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_corridor_unavailable` when no active
    /// corridor matches, `programmatic_policy_corridor_exhausted` when the
    /// corridor has no remaining action, or an unavailable error when the
    /// atomic commit fails.
    fn consume_programmatic_corridor_action(
        &self,
        input: ConsumeProgrammaticCorridorActionInputDto,
    ) -> DtoResult<ProgrammaticAuthorizationCorridorRecordDto>;
    /// Closes every corridor of one root tree as expired or revoked.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the atomic update fails.
    fn close_programmatic_corridors_for_root(
        &self,
        root_run_id: String,
        state: ProgrammaticCorridorStateDto,
        closed_at_ms: u64,
    ) -> DtoResult<u64>;
    /// Proposes one inactive policy draft with evidence coalescing.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_draft_conflict` when an unequal proposal
    /// already owns the pending slot, `programmatic_policy_draft_too_large`
    /// when the draft exceeds its bound, or an unavailable error when the
    /// atomic commit fails.
    fn propose_programmatic_policy_draft(
        &self,
        input: ProgrammaticPolicyDraftRecordDto,
    ) -> DtoResult<ProgrammaticPolicyDraftRecordDto>;
    /// Loads the single pending draft of one owner scope and record kind.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the draft cannot be read.
    fn load_pending_programmatic_policy_draft(
        &self,
        scope: ProgrammaticPolicyScopeDto,
        record_kind: String,
    ) -> DtoResult<Option<ProgrammaticPolicyDraftRecordDto>>;
    /// Decides one pending policy draft without changing an active policy.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_draft_conflict` when the draft is not
    /// pending or the exact base state is stale, or an unavailable error when
    /// the atomic commit fails.
    fn decide_programmatic_policy_draft(
        &self,
        draft_id: String,
        state: ProgrammaticPolicyDraftStateDto,
        decided_at_ms: u64,
    ) -> DtoResult<ProgrammaticPolicyDraftRecordDto>;
}

/// DTO-only repository contract for shared policy counters and admission
/// reservations.
pub trait ProgrammaticAdmissionRepositoryDto {
    /// Loads the durable counter state of one policy, if any.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the counters cannot be read.
    fn load_programmatic_policy_counters(
        &self,
        policy_id: String,
    ) -> DtoResult<Option<ProgrammaticPolicyCounterRecordDto>>;
    /// Atomically reserves one run and calendar action before
    /// `ToolCallStarted`.
    ///
    /// An equal repeated operation reads the accepted idempotent binding and
    /// reserves no second unit. Every bound is verified before any counter
    /// mutates, and the transaction writes all reservations or none of them.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_reservation_conflict` when the same tool
    /// call is already bound to a different input,
    /// `programmatic_policy_run_limit_exceeded` or
    /// `programmatic_policy_calendar_limit_exceeded` when a bound is
    /// exhausted, `programmatic_policy_counter_unavailable` when no compatible
    /// counter exists, or an unavailable error when the atomic commit fails.
    fn reserve_programmatic_policy_action(
        &self,
        input: ReserveProgrammaticPolicyActionInputDto,
    ) -> DtoResult<ReserveProgrammaticPolicyActionOutcomeDto>;
    /// Loads one reservation by its reference, if any.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the reservation cannot be read.
    fn load_programmatic_policy_reservation(
        &self,
        reservation_reference: String,
    ) -> DtoResult<Option<ProgrammaticPolicyReservationRecordDto>>;
    /// Loads every reservation of one root run in creation order.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the reservations cannot be read.
    fn load_programmatic_policy_reservations_for_run(
        &self,
        root_run_id: String,
    ) -> DtoResult<Vec<ProgrammaticPolicyReservationRecordDto>>;
    /// Releases one known pre-effect reservation atomically.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_reservation_conflict` when the reservation
    /// is not outstanding, or an unavailable error when the atomic commit
    /// fails.
    fn release_programmatic_policy_reservation(
        &self,
        input: ReleaseProgrammaticPolicyReservationInputDto,
    ) -> DtoResult<ProgrammaticPolicyReservationRecordDto>;
    /// Makes one reservation permanently consumed at `ToolCallStarted`.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_reservation_conflict` when the reservation
    /// is not outstanding, or an unavailable error when the atomic commit
    /// fails.
    fn commit_programmatic_reservation_started(
        &self,
        input: CommitProgrammaticReservationStartedInputDto,
    ) -> DtoResult<ProgrammaticPolicyReservationRecordDto>;
    /// Applies one recovery disposition to an outstanding reservation.
    ///
    /// An unstarted reservation is released atomically as
    /// `interrupted_before_start`; a started ambiguous action keeps its
    /// permanent consumption and becomes `external_effect_unknown` without
    /// being retried.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_reservation_conflict` when no matching
    /// outstanding reservation exists, or an unavailable error when the atomic
    /// commit fails.
    fn recover_programmatic_policy_reservation(
        &self,
        input: RecoverProgrammaticPolicyReservationInputDto,
    ) -> DtoResult<ProgrammaticPolicyReservationRecordDto>;
}
