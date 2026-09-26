//! Durable continual-harness repository contracts for architecture 26.
//!
//! Every type below is a DTO-only durable record for the Slice 3 harness
//! surface activated by ADR 0044: rules and their immutable revisions, durable
//! coalesced trigger reasons, launch counters, dossiers, verified checkpoints,
//! and the durable harness journal. Identities are canonical daemon-assigned
//! text values, content is bounded credential-free safe data, and digests are
//! canonical `sha256:<64 lowercase hex>` text.
//!
//! Task content, transcripts, dossier bodies, workspace paths, grants, and
//! provider resources never cross this boundary: they are represented only by
//! their safe digest, byte size, and bounded typed references, and a
//! credential-shaped value is rejected before it can reach a row.

use intention_types::DtoResult;

use crate::{
    validate_safe_content, validate_safe_digest, validate_safe_label, validate_safe_labels,
};

/// The maximum page size of one durable harness journal read.
pub const MAX_HARNESS_JOURNAL_PAGE: u64 = 64;
/// The maximum count of bounded typed references on one trigger reason.
pub const MAX_HARNESS_TRIGGER_REFERENCES: usize = 64;
/// The maximum count of explicit sources on one rule revision.
pub const MAX_HARNESS_RULE_SOURCES: usize = 16;

/// The two scopes at which a continual harness exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HarnessRuleScopeDto {
    /// A project-scoped harness.
    Project {
        /// The owning project identity.
        project_id: String,
    },
    /// An ordinary user-session-scoped harness.
    UserSession {
        /// The owning project identity.
        project_id: String,
        /// The linked ordinary user session identity.
        session_id: String,
    },
}

impl HarnessRuleScopeDto {
    /// Returns the owning project identity.
    #[must_use]
    pub fn project_id(&self) -> &str {
        match self {
            Self::Project { project_id } | Self::UserSession { project_id, .. } => project_id,
        }
    }
    /// Returns the linked user session identity of a session-scoped harness.
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::Project { .. } => None,
            Self::UserSession { session_id, .. } => Some(session_id),
        }
    }
    /// Returns the stable durable scope discriminator.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Project { .. } => "project",
            Self::UserSession { .. } => "user_session",
        }
    }
    /// Validates this scope identity.
    ///
    /// # Errors
    ///
    /// Returns `invalid_harness_rule` for a blank, over-long, control-bearing,
    /// or credential-shaped identity.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("invalid_harness_rule", self.project_id())?;
        if let Some(session_id) = self.session_id() {
            validate_safe_label("invalid_harness_rule", session_id)?;
        }
        Ok(())
    }
}

/// The durable lifecycle state of one harness rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessRuleLifecycleStateDto {
    /// The rule captures and admits automatic and explicit triggers.
    Active,
    /// Automation is paused: automatic sources coalesce but do not launch.
    Paused,
    /// The rule is archived with full retention.
    Archived,
}

impl HarnessRuleLifecycleStateDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Archived => "archived",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `harness_not_active` for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "active" => Ok(Self::Active),
            "paused" => Ok(Self::Paused),
            "archived" => Ok(Self::Archived),
            _ => Err(intention_types::ErrorDto::validation(
                "harness_not_active",
                "the durable harness rule lifecycle state is unknown",
            )),
        }
    }
}

/// One typed operation against a durable harness rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessRuleOperationDto {
    /// Update the rule as a new immutable revision.
    UpdateRevision,
    /// Pause automatic launch while capture and coalescing continue.
    Pause,
    /// Resume a paused rule.
    Resume,
    /// Admit one separate explicit user launch.
    ExplicitLaunch,
    /// Cancel the rule's active run through the ordinary two-step lifecycle.
    CancelActiveRun,
    /// Archive a rule without an active run with full retention.
    Archive,
}

impl HarnessRuleOperationDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::UpdateRevision => "update_revision",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::ExplicitLaunch => "explicit_launch",
            Self::CancelActiveRun => "cancel_active_run",
            Self::Archive => "archive",
        }
    }
}

/// The closed source kinds of one durable harness rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessSourceKindDto {
    /// An explicit user launch.
    ExplicitUserLaunch,
    /// A project-time-zone calendar slot.
    CalendarTime,
    /// A fixed equal interval from a durable anchor.
    FixedInterval,
    /// A selected known terminal outcome of another harness or session.
    TerminalOutcomeLink,
}

impl HarnessSourceKindDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ExplicitUserLaunch => "explicit_user_launch",
            Self::CalendarTime => "calendar_time",
            Self::FixedInterval => "fixed_interval",
            Self::TerminalOutcomeLink => "terminal_outcome_link",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `harness_source_unavailable` for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "explicit_user_launch" => Ok(Self::ExplicitUserLaunch),
            "calendar_time" => Ok(Self::CalendarTime),
            "fixed_interval" => Ok(Self::FixedInterval),
            "terminal_outcome_link" => Ok(Self::TerminalOutcomeLink),
            _ => Err(intention_types::ErrorDto::validation(
                "harness_source_unavailable",
                "the durable harness source kind is unknown",
            )),
        }
    }
}

/// The closed task mode of one durable rule revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessTaskModeDto {
    /// The rule repeats its immutable task.
    RepeatedTask,
    /// The rule continues against one active Goal (EXC-041).
    GoalDirected,
}

impl HarnessTaskModeDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::RepeatedTask => "repeated_task",
            Self::GoalDirected => "goal_directed",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `harness_revision_conflict` for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "repeated_task" => Ok(Self::RepeatedTask),
            "goal_directed" => Ok(Self::GoalDirected),
            _ => Err(intention_types::ErrorDto::validation(
                "harness_revision_conflict",
                "the durable harness task mode is unknown",
            )),
        }
    }
}

/// The closed presentation mode of one durable rule revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessPresentationModeDto {
    /// Output stays in the harness journal.
    JournalOnly,
    /// Output also publishes one compact safe linked-activity entry.
    JournalAndActivityEntry,
}

impl HarnessPresentationModeDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::JournalOnly => "journal_only",
            Self::JournalAndActivityEntry => "journal_and_activity_entry",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `harness_revision_conflict` for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "journal_only" => Ok(Self::JournalOnly),
            "journal_and_activity_entry" => Ok(Self::JournalAndActivityEntry),
            _ => Err(intention_types::ErrorDto::validation(
                "harness_revision_conflict",
                "the durable harness presentation mode is unknown",
            )),
        }
    }
}

/// The closed execution class inherited by a harness launch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessExecutionClassDto {
    /// The narrow light class.
    Light,
    /// The medium class.
    Medium,
    /// The heavy class.
    Heavy,
}

impl HarnessExecutionClassDto {
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
    /// Returns `invalid_harness_class_resolution` for an unknown name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "light" => Ok(Self::Light),
            "medium" => Ok(Self::Medium),
            "heavy" => Ok(Self::Heavy),
            _ => Err(intention_types::ErrorDto::validation(
                "invalid_harness_class_resolution",
                "the durable harness execution class is unknown",
            )),
        }
    }
}

/// The closed outcome of one durable trigger capture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessTriggerCaptureOutcomeDto {
    /// A new single pending reason was captured.
    Captured,
    /// The observation coalesced into the existing pending reason.
    Coalesced,
    /// The same reason was redelivered; nothing new is captured.
    Redelivered,
    /// A coalesced catch-up reason was captured after downtime or a pause.
    CatchUp,
}

impl HarnessTriggerCaptureOutcomeDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Captured => "captured",
            Self::Coalesced => "coalesced",
            Self::Redelivered => "redelivered",
            Self::CatchUp => "catch_up",
        }
    }
    /// Whether this capture wrote a new or extended pending reason.
    #[must_use]
    pub const fn changed_pending_reason(self) -> bool {
        !matches!(self, Self::Redelivered)
    }
}

/// The durable state of one harness trigger reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessTriggerReasonStateDto {
    /// The reason is the single pending reason of its rule.
    Pending,
    /// The reason was consumed by exactly one admitted launch.
    Admitted,
}

impl HarnessTriggerReasonStateDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Admitted => "admitted",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `harness_source_unavailable` for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "pending" => Ok(Self::Pending),
            "admitted" => Ok(Self::Admitted),
            _ => Err(intention_types::ErrorDto::validation(
                "harness_source_unavailable",
                "the durable trigger reason state is unknown",
            )),
        }
    }
}

/// The closed run outcomes a harness checkpoint decision observes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessRunOutcomeDto {
    /// The run completed successfully.
    Completed,
    /// The run failed safely.
    Failed,
    /// The run was cancelled.
    Cancelled,
    /// Daemon recovery ended the unfinished run without retrying it.
    Interrupted,
    /// A tool or external effect of the run could not be confirmed.
    ExternalEffectUnknown,
}

impl HarnessRunOutcomeDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::ExternalEffectUnknown => "external_effect_unknown",
        }
    }
}

/// The disposition of one verified-checkpoint decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessCheckpointDispositionDto {
    /// The completely validated candidate replaced the current checkpoint.
    Replaced,
    /// The previous verified checkpoint was retained.
    RetainedPrevious,
}

impl HarnessCheckpointDispositionDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Replaced => "replaced",
            Self::RetainedPrevious => "retained_previous",
        }
    }
}

/// The closed record kinds of the durable harness journal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessJournalRecordKindDto {
    /// One durable trigger reason was captured or coalesced.
    TriggerCaptured,
    /// One launch was admitted against the pending reason.
    LaunchAdmitted,
    /// One pending reason was retained while no capacity was available.
    LaunchRetained,
    /// One harness run reached a known outcome.
    RunTerminal,
    /// One verified checkpoint replaced the current checkpoint.
    CheckpointAccepted,
    /// One checkpoint candidate was rejected and the previous one retained.
    CheckpointRetained,
    /// One safe conclusion was published after durable commit and reread.
    ConclusionPublished,
}

impl HarnessJournalRecordKindDto {
    /// Returns the stable durable discriminator name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::TriggerCaptured => "trigger_captured",
            Self::LaunchAdmitted => "launch_admitted",
            Self::LaunchRetained => "launch_retained",
            Self::RunTerminal => "run_terminal",
            Self::CheckpointAccepted => "checkpoint_accepted",
            Self::CheckpointRetained => "checkpoint_retained",
            Self::ConclusionPublished => "conclusion_published",
        }
    }
    /// Parses the stable durable discriminator name.
    ///
    /// # Errors
    ///
    /// Returns `storage_decode_failed` for an unknown discriminator name.
    pub fn parse(name: &str) -> DtoResult<Self> {
        match name {
            "trigger_captured" => Ok(Self::TriggerCaptured),
            "launch_admitted" => Ok(Self::LaunchAdmitted),
            "launch_retained" => Ok(Self::LaunchRetained),
            "run_terminal" => Ok(Self::RunTerminal),
            "checkpoint_accepted" => Ok(Self::CheckpointAccepted),
            "checkpoint_retained" => Ok(Self::CheckpointRetained),
            "conclusion_published" => Ok(Self::ConclusionPublished),
            _ => Err(intention_types::ErrorDto::validation(
                "storage_decode_failed",
                "the durable harness journal record kind is unknown",
            )),
        }
    }
}

/// One durable harness rule with its active revision and lifecycle state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessRuleRecordDto {
    /// The daemon-assigned harness identity.
    pub harness_id: String,
    /// The exactly-one project or user-session scope.
    pub scope: HarnessRuleScopeDto,
    /// The closed lifecycle state.
    pub lifecycle_state: HarnessRuleLifecycleStateDto,
    /// The current immutable revision number.
    pub active_revision: u64,
    /// The daemon-owned service session identity of this rule.
    pub service_session_id: String,
    /// The last durable rule update time in Unix milliseconds.
    pub updated_at_ms: u64,
}

impl HarnessRuleRecordDto {
    /// Validates this durable rule record.
    ///
    /// # Errors
    ///
    /// Returns `invalid_harness_rule` for a blank identity or a service session
    /// that is not the rule's own scope session, and
    /// `harness_revision_conflict` for a zero active revision.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("invalid_harness_rule", &self.harness_id)?;
        validate_safe_label("invalid_harness_rule", &self.service_session_id)?;
        self.scope.validate()?;
        if self.active_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "harness_revision_conflict",
                "a durable harness rule has a live active revision",
            ));
        }
        Ok(())
    }
}

/// One immutable harness rule revision record.
///
/// The revision carries the immutable task digest, the inherited class, the
/// closed task and presentation modes, the explicit sources, and the applied
/// project time zone. It never carries raw task text, paths, grants, provider
/// resources, or implementation state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessRuleRevisionRecordDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The immutable revision number, starting at one.
    pub revision: u64,
    /// The immutable task digest of this revision.
    pub task_digest: String,
    /// The inherited harness execution class.
    pub class: HarnessExecutionClassDto,
    /// The closed task mode.
    pub task_mode: HarnessTaskModeDto,
    /// The closed presentation mode.
    pub presentation_mode: HarnessPresentationModeDto,
    /// The applied project time zone of this revision.
    pub applied_time_zone: String,
    /// The ordered closed source kinds of this revision.
    pub source_kinds: Vec<HarnessSourceKindDto>,
    /// The bounded typed references named by the sources.
    pub source_references: Vec<String>,
    /// The durable equal-interval anchor when one source is an interval.
    pub interval_anchor_ms: Option<u64>,
    /// The fixed cadence in milliseconds when one source is an interval.
    pub interval_ms: Option<u64>,
    /// The canonical calendar expression when one source is calendar time.
    pub calendar_expression: Option<String>,
    /// The referenced terminal-outcome link source when one exists.
    pub completion_link_reference: Option<String>,
    /// The selected known terminal outcomes of a completion link.
    pub completion_outcomes: Vec<String>,
    /// The canonical revision digest.
    pub canonical_revision_digest: String,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl HarnessRuleRevisionRecordDto {
    /// Validates this immutable rule revision.
    ///
    /// # Errors
    ///
    /// Returns `harness_revision_conflict` for a zero revision,
    /// `harness_source_unavailable` for a revision that names no source,
    /// `harness_source_limit_exceeded` when the source bound is exceeded, and
    /// `harness_interval_too_short` for an interval below one minute.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("harness_revision_conflict", &self.harness_id)?;
        if self.revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "harness_revision_conflict",
                "a rule revision starts at one",
            ));
        }
        validate_safe_digest("harness_revision_conflict", &self.task_digest)?;
        validate_safe_digest("harness_revision_conflict", &self.canonical_revision_digest)?;
        validate_safe_label("harness_schedule_invalid", &self.applied_time_zone)?;
        if self.source_kinds.is_empty() || self.source_kinds.len() > MAX_HARNESS_RULE_SOURCES {
            return Err(intention_types::ErrorDto::validation(
                "harness_source_unavailable",
                "a rule revision names between one and sixteen explicit sources",
            ));
        }
        validate_safe_labels(
            "harness_source_unavailable",
            &self.source_references,
            MAX_HARNESS_TRIGGER_REFERENCES,
        )?;
        if let Some(interval_ms) = self.interval_ms {
            intention_domain::harness::validate_harness_interval_ms(interval_ms)?;
            if self.interval_anchor_ms.is_none() {
                return Err(intention_types::ErrorDto::validation(
                    "harness_schedule_invalid",
                    "an equal-interval source requires its durable anchor",
                ));
            }
        }
        if self
            .source_kinds
            .iter()
            .any(|kind| matches!(kind, HarnessSourceKindDto::CalendarTime))
        {
            let expression = self.calendar_expression.as_deref().ok_or_else(|| {
                intention_types::ErrorDto::validation(
                    "harness_schedule_invalid",
                    "a calendar source requires its canonical expression",
                )
            })?;
            validate_safe_label("harness_schedule_invalid", expression)?;
        }
        validate_safe_labels(
            "harness_source_unavailable",
            &self.completion_outcomes,
            MAX_HARNESS_RULE_SOURCES,
        )?;
        if let Some(reference) = &self.completion_link_reference {
            validate_safe_label("harness_source_unavailable", reference)?;
        }
        Ok(())
    }
}

/// Input creating one durable harness rule and its first immutable revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateHarnessRuleInputDto {
    /// The durable rule identity, scope, lifecycle, and service session.
    pub rule: HarnessRuleRecordDto,
    /// The first immutable revision bound to that rule.
    pub revision: HarnessRuleRevisionRecordDto,
}

impl CreateHarnessRuleInputDto {
    /// Validates this creation input as one coherent rule.
    ///
    /// # Errors
    ///
    /// Returns the rule and revision validation failures, and
    /// `harness_revision_conflict` when the revision does not belong to the
    /// rule or is not revision one.
    pub fn validate(&self) -> DtoResult<()> {
        self.rule.validate()?;
        self.revision.validate()?;
        if self.revision.harness_id != self.rule.harness_id
            || self.revision.revision != 1
            || self.rule.active_revision != 1
        {
            return Err(intention_types::ErrorDto::validation(
                "harness_revision_conflict",
                "a new harness rule starts at its own revision one",
            ));
        }
        Ok(())
    }
}

/// Input continuing one durable harness rule with the next immutable revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviseHarnessRuleInputDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The exact active revision the caller observed.
    pub expected_revision: u64,
    /// The next immutable revision to append and activate.
    pub revision: HarnessRuleRevisionRecordDto,
}

impl ReviseHarnessRuleInputDto {
    /// Validates this revision update.
    ///
    /// # Errors
    ///
    /// Returns `harness_revision_conflict` when the update does not continue
    /// the exact observed active revision of the same harness.
    pub fn validate(&self) -> DtoResult<()> {
        self.revision.validate()?;
        let next = self.expected_revision.checked_add(1).ok_or_else(|| {
            intention_types::ErrorDto::validation(
                "harness_revision_conflict",
                "the active revision cannot be continued",
            )
        })?;
        if self.revision.harness_id != self.harness_id || self.revision.revision != next {
            return Err(intention_types::ErrorDto::validation(
                "harness_revision_conflict",
                "the update does not continue the active rule revision",
            ));
        }
        Ok(())
    }
}

/// Input applying one typed lifecycle operation to a durable harness rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransitionHarnessRuleLifecycleInputDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The exact active revision the caller observed.
    pub expected_revision: u64,
    /// The typed operation to apply.
    pub operation: HarnessRuleOperationDto,
    /// Whether the rule currently owns a non-terminal run.
    pub has_active_run: bool,
    /// The durable operation time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One durable coalesced harness trigger reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessTriggerReasonRecordDto {
    /// The stable daemon-assigned reason identity.
    pub reason_id: String,
    /// The owning harness identity.
    pub harness_id: String,
    /// The closed source kind that produced the reason.
    pub source_kind: HarnessSourceKindDto,
    /// The originating immutable rule revision.
    pub rule_revision: u64,
    /// The first observation time in Unix milliseconds.
    pub first_observed_at_ms: u64,
    /// The last observation time in Unix milliseconds.
    pub last_observed_at_ms: u64,
    /// The coalesced observation count, always at least one.
    pub coalesced_count: u64,
    /// The applied project time zone at capture time.
    pub applied_time_zone: String,
    /// The cause-chain reference of this reason, when one exists.
    pub cause_chain_reference: Option<String>,
    /// The bounded typed references carried by the reason.
    pub bounded_references: Vec<String>,
    /// The durable reason state.
    pub state: HarnessTriggerReasonStateDto,
}

impl HarnessTriggerReasonRecordDto {
    /// Validates this durable trigger reason.
    ///
    /// # Errors
    ///
    /// Returns `harness_source_unavailable` for missing identities or an empty
    /// observation window and `harness_revision_conflict` for a zero rule
    /// revision.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("harness_source_unavailable", &self.reason_id)?;
        validate_safe_label("harness_source_unavailable", &self.harness_id)?;
        validate_safe_label("harness_schedule_invalid", &self.applied_time_zone)?;
        validate_safe_labels(
            "harness_source_unavailable",
            &self.bounded_references,
            MAX_HARNESS_TRIGGER_REFERENCES,
        )?;
        if self.rule_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "harness_revision_conflict",
                "a trigger reason originates from a live rule revision",
            ));
        }
        if self.coalesced_count == 0 || self.last_observed_at_ms < self.first_observed_at_ms {
            return Err(intention_types::ErrorDto::validation(
                "harness_source_unavailable",
                "a coalesced reason counts at least one observation in one window",
            ));
        }
        Ok(())
    }
}

/// Input capturing one durable trigger observation before any admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureHarnessTriggerInputDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The stable reason identity of this observation.
    pub reason_id: String,
    /// The closed source kind of this observation.
    pub source_kind: HarnessSourceKindDto,
    /// The originating immutable rule revision.
    pub rule_revision: u64,
    /// The observation time in Unix milliseconds.
    pub observed_at_ms: u64,
    /// The applied project time zone at capture time.
    pub applied_time_zone: String,
    /// The cause-chain reference of this observation, when one exists.
    pub cause_chain_reference: Option<String>,
    /// The bounded typed references carried by this observation.
    pub bounded_references: Vec<String>,
    /// The number of missed slots when this observation is a coalesced
    /// catch-up after downtime or a pause; zero for an ordinary observation.
    pub catch_up_missed_slots: u64,
}

/// The durable outcome of one redelivery-safe trigger capture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessTriggerCaptureRecordDto {
    /// The capture outcome.
    pub outcome: HarnessTriggerCaptureOutcomeDto,
    /// The single pending reason of the rule after capture.
    pub reason: HarnessTriggerReasonRecordDto,
}

/// Input consuming the single pending reason with exactly one admitted launch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordHarnessLaunchInputDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The exact pending reason identity to consume.
    pub reason_id: String,
    /// The cause-chain depth of this launch.
    pub cause_chain_depth: u64,
    /// Whether this launch is a direct successor of a terminal outcome.
    pub direct_successor: bool,
    /// The durable launch time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// One durable harness launch, trigger, and capacity counter record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessCounterRecordDto {
    /// The owning harness identity hash is not stored; the record is per rule.
    pub cause_chain_depth: u64,
    /// The current non-terminal work count of the harness subtree.
    pub concurrent_non_terminal: u64,
    /// The total launches of the original cause.
    pub total_launches: u64,
    /// The direct successors launched from one terminal outcome.
    pub direct_successors: u64,
    /// The last counter update time in Unix milliseconds.
    pub updated_at_ms: u64,
}

/// The outcome of one admitted harness launch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordHarnessLaunchOutcomeDto {
    /// The consumed reason in its admitted state.
    pub reason: HarnessTriggerReasonRecordDto,
    /// The resulting durable counters of the rule.
    pub counters: HarnessCounterRecordDto,
}

/// One bounded harness dossier record built at admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessDossierRecordDto {
    /// The daemon-assigned dossier identity.
    pub dossier_id: String,
    /// The owning harness identity.
    pub harness_id: String,
    /// The immutable rule revision that admitted the launch.
    pub rule_revision: u64,
    /// The direct trigger reason of this dossier.
    pub reason_id: String,
    /// The immutable task digest of the admitting revision.
    pub task_digest: String,
    /// The immutable task layer, up to sixteen explicit durable sources.
    pub source_references: Vec<String>,
    /// The bounded typed references of the summary layer.
    pub typed_references: Vec<String>,
    /// The latest verified checkpoint reference included in the dossier.
    pub checkpoint_reference: Option<String>,
    /// The complete dossier size in bytes, never truncated.
    pub dossier_bytes: u64,
    /// The canonical dossier digest.
    pub canonical_dossier_digest: String,
    /// The admission time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl HarnessDossierRecordDto {
    /// Validates this bounded dossier record.
    ///
    /// # Errors
    ///
    /// Returns `harness_dossier_too_large` when the dossier exceeds its
    /// code-owned 512 KiB bound or the reference bound is exceeded, and
    /// `harness_source_unavailable` for missing identities.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("harness_source_unavailable", &self.dossier_id)?;
        validate_safe_label("harness_source_unavailable", &self.harness_id)?;
        validate_safe_label("harness_source_unavailable", &self.reason_id)?;
        validate_safe_digest("harness_source_unavailable", &self.task_digest)?;
        validate_safe_digest("harness_source_unavailable", &self.canonical_dossier_digest)?;
        if self.rule_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "harness_revision_conflict",
                "a dossier belongs to an exact admitting revision",
            ));
        }
        if self.source_references.len() > MAX_HARNESS_RULE_SOURCES
            || self.typed_references.len() > MAX_HARNESS_TRIGGER_REFERENCES
        {
            return Err(intention_types::ErrorDto::validation(
                "harness_dossier_too_large",
                "a dossier stays inside its explicit-source and typed-reference bounds",
            ));
        }
        validate_safe_labels(
            "harness_source_unavailable",
            &self.source_references,
            MAX_HARNESS_RULE_SOURCES,
        )?;
        validate_safe_labels(
            "harness_source_unavailable",
            &self.typed_references,
            MAX_HARNESS_TRIGGER_REFERENCES,
        )?;
        intention_domain::harness::validate_harness_dossier_bytes(self.dossier_bytes)?;
        Ok(())
    }
}

/// One verified harness checkpoint record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessCheckpointRecordDto {
    /// The daemon-assigned checkpoint identity.
    pub checkpoint_id: String,
    /// The owning harness identity.
    pub harness_id: String,
    /// The immutable rule revision that produced the checkpoint.
    pub rule_revision: u64,
    /// The producing run identity.
    pub producing_run_id: String,
    /// The checkpoint revision, versioned from one.
    pub checkpoint_revision: u64,
    /// The canonical checkpoint content digest.
    pub content_digest: String,
    /// The checkpoint size in bytes, never truncated.
    pub checkpoint_bytes: u64,
    /// Whether this checkpoint is the rule's current verified checkpoint.
    pub is_current: bool,
    /// The durable creation time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl HarnessCheckpointRecordDto {
    /// Validates this verified checkpoint record.
    ///
    /// # Errors
    ///
    /// Returns `harness_checkpoint_too_large` when the checkpoint exceeds its
    /// code-owned 512 KiB bound and `harness_revision_conflict` for missing
    /// identities or a zero revision.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("harness_checkpoint_unavailable", &self.checkpoint_id)?;
        validate_safe_label("harness_checkpoint_unavailable", &self.harness_id)?;
        validate_safe_label("harness_checkpoint_unavailable", &self.producing_run_id)?;
        validate_safe_digest("harness_checkpoint_unavailable", &self.content_digest)?;
        if self.rule_revision == 0 || self.checkpoint_revision == 0 {
            return Err(intention_types::ErrorDto::validation(
                "harness_revision_conflict",
                "a checkpoint is versioned from one inside a live revision",
            ));
        }
        intention_domain::harness::validate_harness_checkpoint_bytes(self.checkpoint_bytes)?;
        Ok(())
    }
}

/// Input committing the verified-checkpoint decision of one harness run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitHarnessCheckpointInputDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The producing run identity.
    pub producing_run_id: String,
    /// The known outcome of the producing run.
    pub run_outcome: HarnessRunOutcomeDto,
    /// The completely validated candidate of a successful run, when present.
    pub candidate: Option<HarnessCheckpointRecordDto>,
    /// The durable decision time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// The outcome of one verified-checkpoint decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitHarnessCheckpointOutcomeDto {
    /// Whether the candidate replaced the checkpoint or the previous one stayed.
    pub disposition: HarnessCheckpointDispositionDto,
    /// The current verified checkpoint after the decision, when any exists.
    pub current: Option<HarnessCheckpointRecordDto>,
}

/// One bounded user-visible safe harness conclusion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessConclusionRecordDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The producing run identity.
    pub producing_run_id: String,
    /// The canonical conclusion content digest.
    pub content_digest: String,
    /// The conclusion size in bytes, never truncated.
    pub conclusion_bytes: u64,
    /// The selected presentation mode of the admitting revision.
    pub presentation_mode: HarnessPresentationModeDto,
    /// The durable publication time in Unix milliseconds.
    pub created_at_ms: u64,
}

impl HarnessConclusionRecordDto {
    /// Validates this bounded safe conclusion.
    ///
    /// # Errors
    ///
    /// Returns `harness_result_too_large` when the conclusion exceeds its
    /// code-owned 512 KiB bound and `harness_source_unavailable` for a missing
    /// identity.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("harness_source_unavailable", &self.producing_run_id)?;
        validate_safe_digest("harness_source_unavailable", &self.content_digest)?;
        intention_domain::harness::validate_harness_conclusion_bytes(self.conclusion_bytes)?;
        Ok(())
    }
}

/// One durable, readable harness journal record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessJournalRecordDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The monotonic durable journal sequence of this rule.
    pub sequence: u64,
    /// The closed journal record kind.
    pub record_kind: HarnessJournalRecordKindDto,
    /// The bounded safe summary of the record, never a raw transcript.
    pub safe_summary: String,
    /// The canonical digest of the underlying durable record.
    pub canonical_record_digest: String,
    /// The durable record time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

impl HarnessJournalRecordDto {
    /// Validates this durable journal record.
    ///
    /// # Errors
    ///
    /// Returns `harness_revision_conflict` for a zero sequence and
    /// `harness_source_unavailable` for unusable summary or digest text.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_label("harness_source_unavailable", &self.harness_id)?;
        validate_safe_content(
            "harness_source_unavailable",
            "harness_result_too_large",
            &self.safe_summary,
        )?;
        validate_safe_digest("harness_source_unavailable", &self.canonical_record_digest)?;
        if self.sequence == 0 {
            return Err(intention_types::ErrorDto::validation(
                "harness_revision_conflict",
                "a durable journal sequence starts at one",
            ));
        }
        Ok(())
    }
}

/// Input appending one durable harness journal record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendHarnessJournalRecordInputDto {
    /// The owning harness identity.
    pub harness_id: String,
    /// The closed journal record kind.
    pub record_kind: HarnessJournalRecordKindDto,
    /// The bounded safe summary of the record, never a raw transcript.
    pub safe_summary: String,
    /// The canonical digest of the underlying durable record.
    pub canonical_record_digest: String,
    /// The durable record time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// Input loading a bounded page of one rule's durable journal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoadHarnessJournalInputDto {
    /// The owning harness identity is supplied separately to the loader.
    pub after_sequence: u64,
    /// The maximum number of records to return.
    pub limit: u64,
}

/// DTO-only repository contract for durable harness rules and revisions.
pub trait HarnessRuleRepositoryDto {
    /// Creates one durable harness rule with its first immutable revision.
    ///
    /// # Errors
    ///
    /// Returns `harness_rule_limit_exceeded` when the project rule bound is
    /// exceeded, `harness_revision_conflict` when the identity or revision is
    /// already bound to different content, and an unavailable error when the
    /// atomic commit fails.
    fn create_harness_rule(
        &self,
        input: CreateHarnessRuleInputDto,
    ) -> DtoResult<HarnessRuleRecordDto>;
    /// Loads one durable harness rule by identity.
    ///
    /// # Errors
    ///
    /// Returns `harness_not_active` when the rule does not exist, or an
    /// unavailable error when the rule cannot be read.
    fn load_harness_rule(&self, harness_id: String) -> DtoResult<HarnessRuleRecordDto>;
    /// Loads one immutable rule revision.
    ///
    /// # Errors
    ///
    /// Returns `harness_revision_conflict` when the exact revision is absent,
    /// or an unavailable error when the revision cannot be read.
    fn load_harness_rule_revision(
        &self,
        harness_id: String,
        revision: u64,
    ) -> DtoResult<HarnessRuleRevisionRecordDto>;
    /// Appends one immutable revision and moves the active revision in one
    /// atomic transaction guarded by the exact expected revision.
    ///
    /// # Errors
    ///
    /// Returns `harness_revision_conflict` when the expected revision is stale
    /// or the identity is already bound to different revision content, or an
    /// unavailable error when the atomic commit fails.
    fn revise_harness_rule(
        &self,
        input: ReviseHarnessRuleInputDto,
    ) -> DtoResult<HarnessRuleRecordDto>;
    /// Applies one validated lifecycle operation to a durable rule.
    ///
    /// # Errors
    ///
    /// Returns `harness_not_active` when the operation does not match the
    /// durable state, `harness_archived` for an archived rule,
    /// `harness_revision_conflict` for a stale expected revision, or an
    /// unavailable error when the atomic commit fails.
    fn transition_harness_rule_lifecycle(
        &self,
        input: TransitionHarnessRuleLifecycleInputDto,
    ) -> DtoResult<HarnessRuleRecordDto>;
    /// Counts the durable harness rules owned by one project.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the count cannot be read.
    fn count_harness_rules(&self, project_id: String) -> DtoResult<u64>;
}

/// DTO-only repository contract for durable harness trigger reasons and
/// counters.
pub trait HarnessTriggerRepositoryDto {
    /// Captures one durable trigger observation before any admission.
    ///
    /// A redelivered reason identity updates nothing; a new observation
    /// coalesces into the single pending reason of the rule.
    ///
    /// # Errors
    ///
    /// Returns `harness_not_active` when no rule is active, and an unavailable
    /// error when the atomic capture fails.
    fn capture_harness_trigger(
        &self,
        input: CaptureHarnessTriggerInputDto,
    ) -> DtoResult<HarnessTriggerCaptureRecordDto>;
    /// Loads the single pending trigger reason of one rule, if any.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the reason cannot be read.
    fn load_pending_harness_trigger(
        &self,
        harness_id: String,
    ) -> DtoResult<Option<HarnessTriggerReasonRecordDto>>;
    /// Loads one durable trigger reason by identity.
    ///
    /// # Errors
    ///
    /// Returns `harness_source_unavailable` when the reason is absent, or an
    /// unavailable error when the reason cannot be read.
    fn load_harness_trigger_reason(
        &self,
        reason_id: String,
    ) -> DtoResult<HarnessTriggerReasonRecordDto>;
    /// Consumes the single pending reason with exactly one admitted launch and
    /// increments the durable cause-chain, concurrency, and launch counters in
    /// one atomic transaction.
    ///
    /// # Errors
    ///
    /// Returns `harness_concurrency_limit_exceeded`,
    /// `harness_cause_chain_limit_exceeded`, `harness_source_unavailable` when
    /// no matching pending reason exists, or an unavailable error when the
    /// atomic commit fails. A limit failure leaves the pending reason retained.
    fn record_harness_launch(
        &self,
        input: RecordHarnessLaunchInputDto,
    ) -> DtoResult<RecordHarnessLaunchOutcomeDto>;
    /// Loads the durable counters of one rule.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the counters cannot be read.
    fn load_harness_counters(&self, harness_id: String) -> DtoResult<HarnessCounterRecordDto>;
}

/// DTO-only repository contract for harness dossiers, verified checkpoints,
/// and the durable journal.
pub trait HarnessCheckpointRepositoryDto {
    /// Stores one bounded credential-free dossier built at admission.
    ///
    /// # Errors
    ///
    /// Returns `harness_dossier_too_large` when the dossier exceeds its bound,
    /// or an unavailable error when the commit fails.
    fn store_harness_dossier(
        &self,
        input: HarnessDossierRecordDto,
    ) -> DtoResult<HarnessDossierRecordDto>;
    /// Loads one durable dossier by identity.
    ///
    /// # Errors
    ///
    /// Returns `harness_source_unavailable` when the dossier is absent, or an
    /// unavailable error when it cannot be read.
    fn load_harness_dossier(&self, dossier_id: String) -> DtoResult<HarnessDossierRecordDto>;
    /// Commits the verified-checkpoint decision of one completed run.
    ///
    /// A successful run replaces the current checkpoint only with a completely
    /// validated candidate; every other outcome retains the previous
    /// checkpoint in the same transaction.
    ///
    /// # Errors
    ///
    /// Returns `harness_checkpoint_unavailable` when a completed run supplies
    /// no validated candidate or an unknown harness is named, or an unavailable
    /// error when the atomic commit fails.
    fn commit_harness_checkpoint(
        &self,
        input: CommitHarnessCheckpointInputDto,
    ) -> DtoResult<CommitHarnessCheckpointOutcomeDto>;
    /// Loads the current verified checkpoint of one rule, if any.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the checkpoint cannot be read.
    fn load_current_harness_checkpoint(
        &self,
        harness_id: String,
    ) -> DtoResult<Option<HarnessCheckpointRecordDto>>;
    /// Appends one durable harness journal record with the next sequence.
    ///
    /// # Errors
    ///
    /// Returns `harness_not_active` when no rule is active, or an unavailable
    /// error when the atomic append fails.
    fn append_harness_journal_record(
        &self,
        input: AppendHarnessJournalRecordInputDto,
    ) -> DtoResult<HarnessJournalRecordDto>;
    /// Loads a bounded duplex page of one rule's durable journal strictly after
    /// the supplied sequence.
    ///
    /// # Errors
    ///
    /// Returns a validation error for an unusable page limit, or an unavailable
    /// error when the journal cannot be read.
    fn load_harness_journal(
        &self,
        harness_id: String,
        input: LoadHarnessJournalInputDto,
    ) -> DtoResult<Vec<HarnessJournalRecordDto>>;
}
