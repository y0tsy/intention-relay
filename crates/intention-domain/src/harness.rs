//! Durable continual-harness domain vocabulary, rules, and bounds.
//!
//! Owner: architecture 26 (continual harness), with the closed safe failures of
//! ADR 0030 and the Slice 3 harness activation of ADR 0044 under the ADR 0035
//! slice sequence. This
//! module owns the durable rule, revision, trigger, dossier, checkpoint, and
//! conclusion vocabulary, trigger capture and coalescing, catch-up and
//! redelivery outcomes, equal-interval and project-time-zone calendar
//! semantics, read-and-delegate execution-class resolution, the code-owned
//! harness bounds, the closed `harness_*` safe-failure set, and the accepted
//! autonomous harness goal mode (EXC-041) and post-disconnect contract
//! (EXC-042) as domain rules.
//!
//! Every durable record is typed, immutable, and credential-free: it carries
//! safe identity, revision, digest, and bound values only, never raw content,
//! credentials, paths, grants, provider resources, process handles, or
//! implementation state. Durable transactions, scheduling ticks, journal
//! persistence, run admission, and recovery live in `intention-application`,
//! `intention-storage-sqlite`, and `intention-daemon`.
//!
//! The closed harness safe failures are exactly the fifteen `harness_*` codes
//! in [`HARNESS_CLOSED_SAFE_FAILURES`]. Structural input validation that is not
//! one of those safe failures uses the separate `invalid_harness_*` codes
//! below, which never enter the closed `harness_*` vocabulary.

use crate::canonical::{Digest256, contains_control_or_nul, contains_credential_shape};
use crate::slice3_selections::{
    HARNESS_MAX_CAUSE_DEPTH, HARNESS_MAX_CHECKPOINT_BYTES, HARNESS_MAX_CONCLUSION_BYTES,
    HARNESS_MAX_CONCURRENT, HARNESS_MAX_DOSSIER_BYTES, HARNESS_MAX_NARROWED_TOOLS,
    HARNESS_MAX_TOTAL_LAUNCHES, HarnessExecutionClassV1, HarnessSourceKindV1,
    HarnessTriggerReasonV1,
};
use intention_types::{DtoResult, ErrorDto};

/// Maximum durable harness rules in one daemon.
pub const HARNESS_MAX_RULES: u64 = 64;

/// Maximum explicitly named sources in one rule and explicit sources in one
/// dossier.
pub const HARNESS_MAX_SOURCES: u64 = 16;

/// Minimum equal-interval cadence in milliseconds (one minute).
pub const HARNESS_MIN_INTERVAL_MS: u64 = 60_000;

/// Maximum direct successors of one terminal outcome.
pub const HARNESS_MAX_SUCCESSORS: u64 = 16;

/// Maximum typed references in one dossier.
pub const HARNESS_MAX_DOSSIER_REFERENCES: u64 = 64;

/// Closed safe failure: the daemon-wide harness rule bound was exceeded.
pub const HARNESS_RULE_LIMIT_EXCEEDED: &str = "harness_rule_limit_exceeded";

/// Closed safe failure: the source bound of one rule was exceeded.
pub const HARNESS_SOURCE_LIMIT_EXCEEDED: &str = "harness_source_limit_exceeded";

/// Closed safe failure: the concurrent non-terminal work bound was exceeded.
pub const HARNESS_CONCURRENCY_LIMIT_EXCEEDED: &str = "harness_concurrency_limit_exceeded";

/// Closed safe failure: an equal interval is shorter than one minute.
pub const HARNESS_INTERVAL_TOO_SHORT: &str = "harness_interval_too_short";

/// Closed safe failure: a calendar input is invalid or self-contradictory.
pub const HARNESS_SCHEDULE_INVALID: &str = "harness_schedule_invalid";

/// Closed safe failure: a completion link would create a cause cycle.
pub const HARNESS_TRIGGER_CYCLE: &str = "harness_trigger_cycle";

/// Closed safe failure: a dossier exceeds its byte bound.
pub const HARNESS_DOSSIER_TOO_LARGE: &str = "harness_dossier_too_large";

/// Closed safe failure: a selected trigger source is unavailable.
pub const HARNESS_SOURCE_UNAVAILABLE: &str = "harness_source_unavailable";

/// Closed safe failure: a verified checkpoint exceeds its byte bound.
pub const HARNESS_CHECKPOINT_TOO_LARGE: &str = "harness_checkpoint_too_large";

/// Closed safe failure: a verified checkpoint is missing or unavailable.
pub const HARNESS_CHECKPOINT_UNAVAILABLE: &str = "harness_checkpoint_unavailable";

/// Closed safe failure: a safe conclusion exceeds its byte bound.
pub const HARNESS_RESULT_TOO_LARGE: &str = "harness_result_too_large";

/// Closed safe failure: an operation targets a non-active rule.
pub const HARNESS_NOT_ACTIVE: &str = "harness_not_active";

/// Closed safe failure: an operation targets an archived rule.
pub const HARNESS_ARCHIVED: &str = "harness_archived";

/// Closed safe failure: a changed revision reuse or identity mismatch.
pub const HARNESS_REVISION_CONFLICT: &str = "harness_revision_conflict";

/// Closed safe failure: the completion-cause chain bound was exceeded.
pub const HARNESS_CAUSE_CHAIN_LIMIT_EXCEEDED: &str = "harness_cause_chain_limit_exceeded";

/// The closed `harness_*` safe failures owned by architecture 26 and ADR 0030.
///
/// Every entry is a known typed pre-effect rejection: no content is truncated
/// or partly committed, and no external provider, tool, kernel, process,
/// network, or scheduler action occurs in the transition transaction. Each code
/// exists exactly once and is never accompanied by a second harness failure
/// vocabulary.
pub const HARNESS_CLOSED_SAFE_FAILURES: [&str; 15] = [
    HARNESS_RULE_LIMIT_EXCEEDED,
    HARNESS_SOURCE_LIMIT_EXCEEDED,
    HARNESS_CONCURRENCY_LIMIT_EXCEEDED,
    HARNESS_INTERVAL_TOO_SHORT,
    HARNESS_SCHEDULE_INVALID,
    HARNESS_TRIGGER_CYCLE,
    HARNESS_DOSSIER_TOO_LARGE,
    HARNESS_SOURCE_UNAVAILABLE,
    HARNESS_CHECKPOINT_TOO_LARGE,
    HARNESS_CHECKPOINT_UNAVAILABLE,
    HARNESS_RESULT_TOO_LARGE,
    HARNESS_NOT_ACTIVE,
    HARNESS_ARCHIVED,
    HARNESS_REVISION_CONFLICT,
    HARNESS_CAUSE_CHAIN_LIMIT_EXCEEDED,
];

/// Structural rejection for an invalid read-and-delegate class resolution.
///
/// This code is not part of the closed `harness_*` safe-failure vocabulary: it
/// reports malformed or widened class input before any durable transition.
pub const INVALID_HARNESS_CLASS_RESOLUTION: &str = "invalid_harness_class_resolution";

/// Structural rejection for an observed post-disconnect contract violation.
///
/// This code is not part of the closed `harness_*` safe-failure vocabulary: it
/// reports an observed contract state that would resume old external work.
pub const INVALID_HARNESS_DISCONNECT_CONTRACT: &str = "invalid_harness_disconnect_contract";

/// Maximum characters of one project time-zone identifier.
const MAX_TIME_ZONE_CHARS: usize = 256;

/// The credential rejection code shared with every other safe boundary.
const CREDENTIALS_FORBIDDEN: &str = "credentials_forbidden";

/// Converts one collection length into the code-owned bound value domain.
fn bounded_count(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

/// Validates the daemon-wide harness rule bound.
///
/// # Errors
///
/// Returns `harness_rule_limit_exceeded` when `count` exceeds the code-owned
/// limit.
pub fn validate_harness_rule_count(count: u64) -> DtoResult<()> {
    if count > HARNESS_MAX_RULES {
        return Err(ErrorDto::validation(
            HARNESS_RULE_LIMIT_EXCEEDED,
            "the daemon harness rule bound is exceeded",
        ));
    }
    Ok(())
}

/// Validates the source bound of one rule or dossier.
///
/// # Errors
///
/// Returns `harness_source_limit_exceeded` when `count` exceeds the code-owned
/// limit.
pub fn validate_harness_source_count(count: u64) -> DtoResult<()> {
    if count > HARNESS_MAX_SOURCES {
        return Err(ErrorDto::validation(
            HARNESS_SOURCE_LIMIT_EXCEEDED,
            "the harness source bound is exceeded",
        ));
    }
    Ok(())
}

/// Validates the concurrent non-terminal work bound of one harness subtree.
///
/// # Errors
///
/// Returns `harness_concurrency_limit_exceeded` when `count` exceeds the
/// code-owned limit.
pub fn validate_harness_concurrency_count(count: u64) -> DtoResult<()> {
    if count > HARNESS_MAX_CONCURRENT {
        return Err(ErrorDto::validation(
            HARNESS_CONCURRENCY_LIMIT_EXCEEDED,
            "the harness concurrency bound is exceeded",
        ));
    }
    Ok(())
}

/// Validates one equal-interval cadence against the one-minute minimum.
///
/// # Errors
///
/// Returns `harness_interval_too_short` when `interval_ms` is below the
/// code-owned minimum.
pub fn validate_harness_interval_ms(interval_ms: u64) -> DtoResult<()> {
    if interval_ms < HARNESS_MIN_INTERVAL_MS {
        return Err(ErrorDto::validation(
            HARNESS_INTERVAL_TOO_SHORT,
            "the equal interval is below the one-minute minimum",
        ));
    }
    Ok(())
}

/// Validates one dossier byte size.
///
/// # Errors
///
/// Returns `harness_dossier_too_large` when `bytes` exceeds the code-owned
/// limit; an oversized dossier is rejected rather than truncated.
pub fn validate_harness_dossier_bytes(bytes: u64) -> DtoResult<()> {
    if bytes > HARNESS_MAX_DOSSIER_BYTES {
        return Err(ErrorDto::validation(
            HARNESS_DOSSIER_TOO_LARGE,
            "the dossier exceeds its byte bound",
        ));
    }
    Ok(())
}

/// Validates one verified-checkpoint byte size.
///
/// # Errors
///
/// Returns `harness_checkpoint_too_large` when `bytes` exceeds the code-owned
/// limit; an oversized checkpoint retains the previous verified checkpoint.
pub fn validate_harness_checkpoint_bytes(bytes: u64) -> DtoResult<()> {
    if bytes > HARNESS_MAX_CHECKPOINT_BYTES {
        return Err(ErrorDto::validation(
            HARNESS_CHECKPOINT_TOO_LARGE,
            "the checkpoint exceeds its byte bound",
        ));
    }
    Ok(())
}

/// Validates one safe-conclusion byte size.
///
/// # Errors
///
/// Returns `harness_result_too_large` when `bytes` exceeds the code-owned
/// limit; an oversized conclusion is rejected rather than truncated.
pub fn validate_harness_conclusion_bytes(bytes: u64) -> DtoResult<()> {
    if bytes > HARNESS_MAX_CONCLUSION_BYTES {
        return Err(ErrorDto::validation(
            HARNESS_RESULT_TOO_LARGE,
            "the safe conclusion exceeds its byte bound",
        ));
    }
    Ok(())
}

/// Validates one completion-cause chain depth.
///
/// # Errors
///
/// Returns `harness_cause_chain_limit_exceeded` when `depth` exceeds the
/// code-owned limit.
pub fn validate_harness_cause_depth(depth: u64) -> DtoResult<()> {
    if depth > HARNESS_MAX_CAUSE_DEPTH {
        return Err(ErrorDto::validation(
            HARNESS_CAUSE_CHAIN_LIMIT_EXCEEDED,
            "the completion-cause chain depth is exceeded",
        ));
    }
    Ok(())
}

/// Validates the direct-successor fan-out of one terminal outcome.
///
/// # Errors
///
/// Returns `harness_cause_chain_limit_exceeded` when `count` exceeds the
/// code-owned limit.
pub fn validate_harness_successor_count(count: u64) -> DtoResult<()> {
    if count > HARNESS_MAX_SUCCESSORS {
        return Err(ErrorDto::validation(
            HARNESS_CAUSE_CHAIN_LIMIT_EXCEEDED,
            "the completion-cause successor bound is exceeded",
        ));
    }
    Ok(())
}

/// Validates the total launches from one original cause.
///
/// # Errors
///
/// Returns `harness_cause_chain_limit_exceeded` when `total` exceeds the
/// code-owned limit.
pub fn validate_harness_total_launches(total: u64) -> DtoResult<()> {
    if total > HARNESS_MAX_TOTAL_LAUNCHES {
        return Err(ErrorDto::validation(
            HARNESS_CAUSE_CHAIN_LIMIT_EXCEEDED,
            "the total launch bound of the original cause is exceeded",
        ));
    }
    Ok(())
}

/// The two scopes at which a continual harness exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessRuleScopeV1 {
    /// A project-scoped harness.
    Project {
        /// The owning project identity.
        project_id: [u8; 16],
    },
    /// An ordinary user-session-scoped harness.
    UserSession {
        /// The owning project identity.
        project_id: [u8; 16],
        /// The linked ordinary user session identity.
        session_id: [u8; 16],
    },
}

impl HarnessRuleScopeV1 {
    /// Returns the owning project identity.
    #[must_use]
    pub const fn project_id(&self) -> [u8; 16] {
        match self {
            Self::Project { project_id } | Self::UserSession { project_id, .. } => *project_id,
        }
    }

    /// Returns the linked user session identity of a session-scoped harness.
    #[must_use]
    pub const fn session_id(&self) -> Option<[u8; 16]> {
        match self {
            Self::Project { .. } => None,
            Self::UserSession { session_id, .. } => Some(*session_id),
        }
    }

    /// Whether this harness is linked to an ordinary user session.
    #[must_use]
    pub const fn is_session_linked(&self) -> bool {
        matches!(self, Self::UserSession { .. })
    }
}

/// The durable lifecycle state of one harness rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessRuleLifecycleStateV1 {
    /// The rule captures and admits automatic and explicit triggers.
    Active,
    /// Automation is paused: automatic sources are captured and coalesced but
    /// do not launch, while an explicit user launch remains allowed.
    Paused,
    /// The rule is archived with retention: no operation changes it, no
    /// automatic launch occurs, and restoring a linked session never launches
    /// work. The rule and its journal remain durable.
    Archived,
}

impl HarnessRuleLifecycleStateV1 {
    /// Whether the rule is active.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Whether the rule is archived.
    #[must_use]
    pub const fn is_archived(self) -> bool {
        matches!(self, Self::Archived)
    }

    /// Whether automatic sources may admit launches from this state.
    #[must_use]
    pub const fn permits_automatic_launch(self) -> bool {
        matches!(self, Self::Active)
    }
}

/// One typed operation against a harness rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessRuleOperationV1 {
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

/// The origin of one harness launch request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessLaunchOriginV1 {
    /// One automatic source of the rule.
    AutomaticSource,
    /// One explicit user-origin launch operation.
    ExplicitUserLaunch,
}

/// Validates one harness rule operation against the rule's durable state.
///
/// A pause is automation paused: automatic sources are captured and coalesced
/// but do not launch, while an explicit user launch remains allowed as a
/// separate user-origin operation. Archiving is pause with retention and is
/// rejected while the harness still has an active run; cancellation follows the
/// ordinary two-step path first. An archived rule rejects every operation.
///
/// # Errors
///
/// Returns `harness_not_active` when the operation requires an active rule or
/// an active run that the rule does not have, and `harness_archived` when the
/// operation targets an archived rule.
pub fn validate_harness_rule_operation(
    state: HarnessRuleLifecycleStateV1,
    has_active_run: bool,
    operation: HarnessRuleOperationV1,
) -> DtoResult<()> {
    if state.is_archived() {
        return Err(ErrorDto::validation(
            HARNESS_ARCHIVED,
            "an archived harness rule retains state and rejects operations",
        ));
    }
    let permitted = match operation {
        HarnessRuleOperationV1::UpdateRevision | HarnessRuleOperationV1::ExplicitLaunch => true,
        HarnessRuleOperationV1::Pause => state.is_active(),
        HarnessRuleOperationV1::Resume => !state.is_active(),
        HarnessRuleOperationV1::CancelActiveRun => has_active_run,
        HarnessRuleOperationV1::Archive => !has_active_run,
    };
    if permitted {
        Ok(())
    } else {
        Err(ErrorDto::validation(
            HARNESS_NOT_ACTIVE,
            "the rule is not in the state this operation requires",
        ))
    }
}

/// Applies one validated harness rule operation.
///
/// The lifecycle operations pause, resume, and archive produce a new rule
/// value; update-as-revision, explicit launch, and cancel-active-run leave the
/// rule unchanged because their owning workflow applies them (a revision update
/// goes through [`revise_harness_rule`]).
///
/// # Errors
///
/// Returns the errors of [`validate_harness_rule_operation`].
pub fn apply_harness_rule_operation(
    rule: &HarnessRuleV1,
    has_active_run: bool,
    operation: HarnessRuleOperationV1,
) -> DtoResult<HarnessRuleV1> {
    validate_harness_rule_operation(rule.lifecycle_state, has_active_run, operation)?;
    let mut updated = rule.clone();
    updated.lifecycle_state = match operation {
        HarnessRuleOperationV1::Pause => HarnessRuleLifecycleStateV1::Paused,
        HarnessRuleOperationV1::Resume => HarnessRuleLifecycleStateV1::Active,
        HarnessRuleOperationV1::Archive => HarnessRuleLifecycleStateV1::Archived,
        HarnessRuleOperationV1::UpdateRevision
        | HarnessRuleOperationV1::ExplicitLaunch
        | HarnessRuleOperationV1::CancelActiveRun => rule.lifecycle_state,
    };
    Ok(updated)
}

/// One durable equal-interval schedule anchored on a fixed cadence.
///
/// A long run never shifts the grid: slots stay `anchor + k * interval`, and
/// missed slots coalesce into one pending or catch-up reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessIntervalScheduleV1 {
    anchor_ms: u64,
    interval_ms: u64,
}

impl HarnessIntervalScheduleV1 {
    /// Creates one equal-interval schedule from a durable anchor.
    ///
    /// # Errors
    ///
    /// Returns `harness_interval_too_short` when the interval is below the
    /// one-minute minimum.
    pub fn new(anchor_ms: u64, interval_ms: u64) -> DtoResult<Self> {
        validate_harness_interval_ms(interval_ms)?;
        Ok(Self {
            anchor_ms,
            interval_ms,
        })
    }

    /// Returns the durable anchor of the schedule grid.
    #[must_use]
    pub const fn anchor_ms(&self) -> u64 {
        self.anchor_ms
    }

    /// Returns the fixed cadence in milliseconds.
    #[must_use]
    pub const fn interval_ms(&self) -> u64 {
        self.interval_ms
    }

    /// Returns the newest grid slot at or before `now_ms`.
    #[must_use]
    pub const fn slot_at_or_before(&self, now_ms: u64) -> Option<u64> {
        if now_ms < self.anchor_ms {
            return None;
        }
        let elapsed = now_ms - self.anchor_ms;
        let slots = elapsed / self.interval_ms;
        Some(
            self.anchor_ms
                .saturating_add(slots.saturating_mul(self.interval_ms)),
        )
    }

    /// Returns the first grid slot strictly after `after_ms`.
    #[must_use]
    pub const fn next_slot_after(&self, after_ms: u64) -> u64 {
        if after_ms < self.anchor_ms {
            return self.anchor_ms;
        }
        let elapsed = after_ms - self.anchor_ms;
        let slots = (elapsed / self.interval_ms).saturating_add(1);
        self.anchor_ms
            .saturating_add(slots.saturating_mul(self.interval_ms))
    }

    /// Returns the one slot a capture pass may admit, if any.
    ///
    /// The newest slot at or before `now_ms` is selected, so every missed slot
    /// coalesces into one catch-up reason. A clock that moved backwards or a
    /// slot already captured returns `None`: an already captured durable
    /// trigger is never repeated.
    #[must_use]
    pub const fn next_capture(
        &self,
        last_captured_slot_ms: Option<u64>,
        now_ms: u64,
    ) -> Option<u64> {
        match self.slot_at_or_before(now_ms) {
            None => None,
            Some(candidate) => match last_captured_slot_ms {
                Some(last) if candidate <= last => None,
                _ => Some(candidate),
            },
        }
    }
}

/// One closed typed project-time-zone calendar schedule.
///
/// Both the closed typed form and a standard five-part calendar expression are
/// accepted, but they canonicalize to one record; contradictory equivalent
/// inputs are rejected before admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessCalendarScheduleV1 {
    minute: Option<u32>,
    hour: Option<u32>,
    day_of_month: Option<u32>,
    month: Option<u32>,
    day_of_week: Option<u32>,
}

impl HarnessCalendarScheduleV1 {
    /// The unrestricted calendar form (`* * * * *`).
    #[must_use]
    pub const fn every_minute() -> Self {
        Self {
            minute: None,
            hour: None,
            day_of_month: None,
            month: None,
            day_of_week: None,
        }
    }

    /// Creates one closed typed calendar form.
    ///
    /// Each field is either unrestricted or one exact value; day-of-week `7` is
    /// the Sunday alias of `0`.
    ///
    /// # Errors
    ///
    /// Returns `harness_schedule_invalid` for an out-of-range field or a
    /// calendar day that does not exist in the selected month.
    pub fn typed(
        minute: Option<u32>,
        hour: Option<u32>,
        day_of_month: Option<u32>,
        month: Option<u32>,
        day_of_week: Option<u32>,
    ) -> DtoResult<Self> {
        Self::validated(&Self {
            minute,
            hour,
            day_of_month,
            month,
            day_of_week,
        })
    }

    /// Canonicalizes one typed form, one five-part expression, or both.
    ///
    /// The accepted expression grammar of this first scope is five
    /// whitespace-separated parts, each `*` or one exact literal in its field
    /// range. Both inputs canonicalize to one record and are rejected when they
    /// disagree.
    ///
    /// # Errors
    ///
    /// Returns `harness_schedule_invalid` when neither input is present, a
    /// field is out of range, a day does not exist in its month, the expression
    /// is not five literal-or-`*` parts, or equivalent inputs disagree.
    pub fn canonicalize(typed: Option<&Self>, expression: Option<&str>) -> DtoResult<Self> {
        let typed = typed.map(Self::validated).transpose()?;
        let parsed = expression.map(Self::parse_expression).transpose()?;
        match (typed, parsed) {
            (Some(typed), None) => Ok(typed),
            (None, Some(parsed)) => Ok(parsed),
            (Some(typed), Some(parsed)) if typed == parsed => Ok(typed),
            (Some(_), Some(_)) => Err(ErrorDto::validation(
                HARNESS_SCHEDULE_INVALID,
                "equivalent calendar inputs disagree after canonicalization",
            )),
            (None, None) => Err(ErrorDto::validation(
                HARNESS_SCHEDULE_INVALID,
                "a calendar source needs one typed or expression form",
            )),
        }
    }

    /// Returns the minute field, or `None` for every minute.
    #[must_use]
    pub const fn minute(&self) -> Option<u32> {
        self.minute
    }

    /// Returns the hour field, or `None` for every hour.
    #[must_use]
    pub const fn hour(&self) -> Option<u32> {
        self.hour
    }

    /// Returns the day-of-month field, or `None` for every day.
    #[must_use]
    pub const fn day_of_month(&self) -> Option<u32> {
        self.day_of_month
    }

    /// Returns the month field, or `None` for every month.
    #[must_use]
    pub const fn month(&self) -> Option<u32> {
        self.month
    }

    /// Returns the day-of-week field (0 is Sunday), or `None` for every day.
    #[must_use]
    pub const fn day_of_week(&self) -> Option<u32> {
        self.day_of_week
    }

    /// Validates and normalizes one calendar record.
    fn validated(fields: &Self) -> DtoResult<Self> {
        let minute = Self::validated_field(fields.minute, 0, 59)?;
        let hour = Self::validated_field(fields.hour, 0, 23)?;
        let day_of_month = Self::validated_field(fields.day_of_month, 1, 31)?;
        let month = Self::validated_field(fields.month, 1, 12)?;
        let day_of_week = Self::validated_field(fields.day_of_week, 0, 7)?
            .map(|value| if value == 7 { 0 } else { value });
        if let (Some(month), Some(day_of_month)) = (month, day_of_month)
            && day_of_month > days_in_month(month)
        {
            return Err(ErrorDto::validation(
                HARNESS_SCHEDULE_INVALID,
                "the selected day does not exist in the selected month",
            ));
        }
        Ok(Self {
            minute,
            hour,
            day_of_month,
            month,
            day_of_week,
        })
    }

    /// Validates one optional calendar field against its range.
    fn validated_field(value: Option<u32>, min: u32, max: u32) -> DtoResult<Option<u32>> {
        match value {
            Some(value) if value < min || value > max => Err(ErrorDto::validation(
                HARNESS_SCHEDULE_INVALID,
                "a calendar field is outside its range",
            )),
            other => Ok(other),
        }
    }

    /// Parses one five-part calendar expression.
    fn parse_expression(expression: &str) -> DtoResult<Self> {
        let parts: Vec<&str> = expression.split_ascii_whitespace().collect();
        let [minute, hour, day_of_month, month, day_of_week] = parts.as_slice() else {
            return Err(ErrorDto::validation(
                HARNESS_SCHEDULE_INVALID,
                "a calendar expression needs five whitespace-separated parts",
            ));
        };
        Self::validated(&Self {
            minute: Self::parse_part(minute, 0, 59)?,
            hour: Self::parse_part(hour, 0, 23)?,
            day_of_month: Self::parse_part(day_of_month, 1, 31)?,
            month: Self::parse_part(month, 1, 12)?,
            day_of_week: Self::parse_part(day_of_week, 0, 7)?,
        })
    }

    /// Parses one `*` or literal calendar part.
    fn parse_part(part: &str, min: u32, max: u32) -> DtoResult<Option<u32>> {
        if part == "*" {
            return Ok(None);
        }
        let Ok(value) = part.parse::<u32>() else {
            return Err(ErrorDto::validation(
                HARNESS_SCHEDULE_INVALID,
                "a calendar part must be `*` or one literal value",
            ));
        };
        Self::validated_field(Some(value), min, max)
    }
}

/// The number of days in one calendar month, tolerating leap February.
const fn days_in_month(month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => 29,
    }
}

/// One observed project-time-zone daylight-saving transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessDstTransitionV1 {
    transition_at_ms: i64,
    offset_before_seconds: i32,
    offset_after_seconds: i32,
}

impl HarnessDstTransitionV1 {
    /// Creates one daylight-saving transition observation.
    ///
    /// # Errors
    ///
    /// Returns `harness_schedule_invalid` when both offsets are equal, because
    /// an equal offset is not a transition.
    pub fn new(
        transition_at_ms: i64,
        offset_before_seconds: i32,
        offset_after_seconds: i32,
    ) -> DtoResult<Self> {
        if offset_before_seconds == offset_after_seconds {
            return Err(ErrorDto::validation(
                HARNESS_SCHEDULE_INVALID,
                "a daylight-saving transition changes its offset",
            ));
        }
        Ok(Self {
            transition_at_ms,
            offset_before_seconds,
            offset_after_seconds,
        })
    }

    /// Returns the UTC instant at which the offset changes.
    #[must_use]
    pub const fn transition_at_ms(&self) -> i64 {
        self.transition_at_ms
    }

    /// Returns the offset in seconds east of UTC before the transition.
    #[must_use]
    pub const fn offset_before_seconds(&self) -> i32 {
        self.offset_before_seconds
    }

    /// Returns the offset in seconds east of UTC after the transition.
    #[must_use]
    pub const fn offset_after_seconds(&self) -> i32 {
        self.offset_after_seconds
    }
}

/// The resolved UTC instant of one requested local calendar time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessLocalTimeResolutionV1 {
    /// The local time exists exactly once.
    Exact {
        /// The resolved UTC instant in Unix milliseconds.
        utc_ms: i64,
    },
    /// The local time does not exist; it moves forward to the first valid time.
    ForwardedToFirstValid {
        /// The resolved UTC instant in Unix milliseconds.
        utc_ms: i64,
    },
    /// The local time occurs twice; it fires once at the earlier instant.
    RepeatedFiresOnce {
        /// The resolved UTC instant in Unix milliseconds.
        utc_ms: i64,
    },
}

impl HarnessLocalTimeResolutionV1 {
    /// Returns the resolved UTC instant in Unix milliseconds.
    #[must_use]
    pub const fn utc_ms(self) -> i64 {
        match self {
            Self::Exact { utc_ms }
            | Self::ForwardedToFirstValid { utc_ms }
            | Self::RepeatedFiresOnce { utc_ms } => utc_ms,
        }
    }
}

/// Resolves one requested local wall-clock time against a project-time-zone
/// transition.
///
/// A nonexistent local time (a spring-forward gap) moves forward to the first
/// valid time, and a repeated local time (a fall-back overlap) fires once at
/// its earlier instant. No time-zone database or external clock is consulted;
/// the caller supplies the transition observation.
#[must_use]
pub fn resolve_harness_local_time(
    requested_local_ms: i64,
    transition: &HarnessDstTransitionV1,
) -> HarnessLocalTimeResolutionV1 {
    let before_shift = i64::from(transition.offset_before_seconds).saturating_mul(1_000);
    let after_shift = i64::from(transition.offset_after_seconds).saturating_mul(1_000);
    let local_before = transition.transition_at_ms.saturating_add(before_shift);
    let local_after = transition.transition_at_ms.saturating_add(after_shift);
    if transition.offset_after_seconds > transition.offset_before_seconds {
        if requested_local_ms >= local_before && requested_local_ms < local_after {
            return HarnessLocalTimeResolutionV1::ForwardedToFirstValid {
                utc_ms: transition.transition_at_ms,
            };
        }
        let shift = if requested_local_ms < local_before {
            before_shift
        } else {
            after_shift
        };
        HarnessLocalTimeResolutionV1::Exact {
            utc_ms: requested_local_ms.saturating_sub(shift),
        }
    } else if requested_local_ms >= local_after && requested_local_ms < local_before {
        HarnessLocalTimeResolutionV1::RepeatedFiresOnce {
            utc_ms: requested_local_ms.saturating_sub(before_shift),
        }
    } else {
        let shift = if requested_local_ms < local_after {
            before_shift
        } else {
            after_shift
        };
        HarnessLocalTimeResolutionV1::Exact {
            utc_ms: requested_local_ms.saturating_sub(shift),
        }
    }
}

/// Validates one project time-zone identifier.
///
/// # Errors
///
/// Returns `harness_schedule_invalid` for an empty, over-long, control-bearing,
/// or non-identifier time zone, and `credentials_forbidden` for a
/// credential-shaped value.
pub fn validate_harness_time_zone(time_zone: &str) -> DtoResult<()> {
    if contains_credential_shape(time_zone) {
        return Err(ErrorDto::validation(
            CREDENTIALS_FORBIDDEN,
            "credentials are forbidden",
        ));
    }
    if time_zone.is_empty()
        || time_zone.chars().count() > MAX_TIME_ZONE_CHARS
        || contains_control_or_nul(time_zone)
        || !time_zone.bytes().all(is_time_zone_byte)
    {
        return Err(ErrorDto::validation(
            HARNESS_SCHEDULE_INVALID,
            "the project time zone is not a valid identifier",
        ));
    }
    Ok(())
}

/// Whether `byte` is one time-zone identifier character.
const fn is_time_zone_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'+' | b'-')
}

/// Returns the project time zone applied by one rule in its lifecycle state.
///
/// A non-archived rule follows a future project time-zone change, while an
/// archived rule retains the zone recorded by its revision.
///
/// # Errors
///
/// Returns the errors of [`validate_harness_time_zone`] for either zone.
pub fn applied_harness_time_zone<'a>(
    state: HarnessRuleLifecycleStateV1,
    revision_time_zone: &'a str,
    project_time_zone: &'a str,
) -> DtoResult<&'a str> {
    validate_harness_time_zone(revision_time_zone)?;
    validate_harness_time_zone(project_time_zone)?;
    if state.is_archived() {
        Ok(revision_time_zone)
    } else {
        Ok(project_time_zone)
    }
}

/// One known closed terminal outcome a completion link may select.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessTerminalOutcomeV1 {
    /// The source run completed successfully.
    Completed,
    /// The source run failed safely.
    Failed,
    /// The source run was cancelled.
    Cancelled,
    /// The source run was interrupted by daemon recovery.
    Interrupted,
}

impl HarnessTerminalOutcomeV1 {
    /// The canonical order rank of this outcome.
    const fn rank(self) -> u8 {
        match self {
            Self::Completed => 0,
            Self::Failed => 1,
            Self::Cancelled => 2,
            Self::Interrupted => 3,
        }
    }
}

/// One completion link from a known terminal outcome of another harness or
/// session.
///
/// The first scope reacts only to the closed known terminal outcomes: never to
/// partial output, provider fragments, process signals, or unverified external
/// effects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessCompletionLinkV1 {
    source_reference: [u8; 16],
    outcomes: Vec<HarnessTerminalOutcomeV1>,
}

impl HarnessCompletionLinkV1 {
    /// Creates one completion link with an explicit outcome selection.
    ///
    /// Duplicate outcomes collapse into one canonical, rank-ordered selection.
    ///
    /// # Errors
    ///
    /// Returns `harness_source_unavailable` when no known terminal outcome is
    /// selected.
    pub fn new(
        source_reference: [u8; 16],
        outcomes: Vec<HarnessTerminalOutcomeV1>,
    ) -> DtoResult<Self> {
        if outcomes.is_empty() {
            return Err(ErrorDto::validation(
                HARNESS_SOURCE_UNAVAILABLE,
                "a completion link selects at least one known terminal outcome",
            ));
        }
        let mut outcomes = outcomes;
        outcomes.sort_unstable_by_key(|outcome| outcome.rank());
        outcomes.dedup();
        Ok(Self {
            source_reference,
            outcomes,
        })
    }

    /// Returns the referenced source identity.
    #[must_use]
    pub const fn source_reference(&self) -> [u8; 16] {
        self.source_reference
    }

    /// Returns the allowed known terminal outcomes in canonical order.
    #[must_use]
    pub fn outcomes(&self) -> &[HarnessTerminalOutcomeV1] {
        &self.outcomes
    }
}

/// Validates that one completion link may be added to a rule.
///
/// `cause_ancestors` is the ordered cause chain from the original cause through
/// the link owner. Adding the link's source extends that chain by one
/// generation, so the prospective depth is checked against the code-owned
/// bound.
///
/// # Errors
///
/// Returns `harness_trigger_cycle` for a self link or a link to an existing
/// cause ancestor, and `harness_cause_chain_limit_exceeded` when the extended
/// chain exceeds the code-owned depth.
pub fn validate_harness_completion_link(
    link: &HarnessCompletionLinkV1,
    owner: [u8; 16],
    cause_ancestors: &[[u8; 16]],
) -> DtoResult<()> {
    if link.source_reference == owner || cause_ancestors.contains(&link.source_reference) {
        return Err(ErrorDto::validation(
            HARNESS_TRIGGER_CYCLE,
            "the completion link would create a cause cycle",
        ));
    }
    validate_harness_cause_depth(bounded_count(cause_ancestors.len()).saturating_add(1))
}

/// Validates that one selected source kind is currently available.
///
/// # Errors
///
/// Returns `harness_source_unavailable` for an unavailable terminal-outcome
/// link: the referenced prior harness or session is unknown or no longer
/// available.
pub fn validate_harness_source_available(
    kind: HarnessSourceKindV1,
    source_available: bool,
) -> DtoResult<()> {
    if kind == HarnessSourceKindV1::TerminalOutcomeLink && !source_available {
        return Err(ErrorDto::validation(
            HARNESS_SOURCE_UNAVAILABLE,
            "the selected completion source is unavailable",
        ));
    }
    Ok(())
}

/// One explicitly named trigger source of a harness rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HarnessRuleSourceV1 {
    /// An explicit user launch.
    ExplicitUserLaunch,
    /// A project-time-zone calendar slot.
    CalendarTime(HarnessCalendarScheduleV1),
    /// A fixed equal interval.
    FixedInterval(HarnessIntervalScheduleV1),
    /// A selected known terminal outcome of another harness or session.
    TerminalOutcomeLink(HarnessCompletionLinkV1),
}

impl HarnessRuleSourceV1 {
    /// Returns the closed frozen source kind of this source.
    #[must_use]
    pub const fn kind(&self) -> HarnessSourceKindV1 {
        match self {
            Self::ExplicitUserLaunch => HarnessSourceKindV1::ExplicitUserLaunch,
            Self::CalendarTime(_) => HarnessSourceKindV1::CalendarTime,
            Self::FixedInterval(_) => HarnessSourceKindV1::FixedInterval,
            Self::TerminalOutcomeLink(_) => HarnessSourceKindV1::TerminalOutcomeLink,
        }
    }
}

/// The closed task mode of one rule revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessTaskModeV1 {
    /// The rule repeats its immutable task.
    RepeatedTask,
    /// The rule continues against an active goal (EXC-041).
    GoalDirected,
}

/// The presentation mode of one rule revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessPresentationModeV1 {
    /// Output stays in the harness journal.
    JournalOnly,
    /// Output also publishes one compact safe linked-activity entry.
    JournalAndActivityEntry,
}

impl HarnessPresentationModeV1 {
    /// Whether this mode publishes the compact linked-activity entry.
    ///
    /// The entry is not a user message and does not enter a later model
    /// request by itself.
    #[must_use]
    pub const fn publishes_activity_entry(self) -> bool {
        matches!(self, Self::JournalAndActivityEntry)
    }
}

/// One immutable harness rule revision.
///
/// A revision carries the immutable task digest, the inherited class, the task
/// mode, the explicit sources, the presentation mode, and the applied project
/// time zone. It never carries raw task text, paths, grants, provider
/// resources, or implementation state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessRuleRevisionV1 {
    harness_id: [u8; 16],
    revision: u64,
    task_digest: Digest256,
    class: HarnessExecutionClassV1,
    task_mode: HarnessTaskModeV1,
    sources: Vec<HarnessRuleSourceV1>,
    presentation_mode: HarnessPresentationModeV1,
    applied_time_zone: String,
}

impl HarnessRuleRevisionV1 {
    /// Creates one immutable rule revision.
    ///
    /// # Errors
    ///
    /// Returns `harness_revision_conflict` for the zero revision,
    /// `harness_source_unavailable` when the rule names no source,
    /// `harness_source_limit_exceeded` when it names more than the code-owned
    /// source bound, and the errors of [`validate_harness_time_zone`] for the
    /// applied zone.
    #[expect(
        clippy::too_many_arguments,
        reason = "The closed eight-field rule-revision record keeps one validating constructor."
    )]
    pub fn new(
        harness_id: [u8; 16],
        revision: u64,
        task_digest: Digest256,
        class: HarnessExecutionClassV1,
        task_mode: HarnessTaskModeV1,
        sources: Vec<HarnessRuleSourceV1>,
        presentation_mode: HarnessPresentationModeV1,
        applied_time_zone: String,
    ) -> DtoResult<Self> {
        if revision == 0 {
            return Err(ErrorDto::validation(
                HARNESS_REVISION_CONFLICT,
                "a rule revision starts at one",
            ));
        }
        if sources.is_empty() {
            return Err(ErrorDto::validation(
                HARNESS_SOURCE_UNAVAILABLE,
                "a rule names at least one explicit source",
            ));
        }
        validate_harness_source_count(bounded_count(sources.len()))?;
        validate_harness_time_zone(&applied_time_zone)?;
        Ok(Self {
            harness_id,
            revision,
            task_digest,
            class,
            task_mode,
            sources,
            presentation_mode,
            applied_time_zone,
        })
    }

    /// Returns the harness identity owning this revision.
    #[must_use]
    pub const fn harness_id(&self) -> [u8; 16] {
        self.harness_id
    }

    /// Returns the immutable revision number.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the immutable task digest of this revision.
    #[must_use]
    pub const fn task_digest(&self) -> Digest256 {
        self.task_digest
    }

    /// Returns the inherited harness class.
    #[must_use]
    pub const fn class(&self) -> HarnessExecutionClassV1 {
        self.class
    }

    /// Returns the closed task mode.
    #[must_use]
    pub const fn task_mode(&self) -> HarnessTaskModeV1 {
        self.task_mode
    }

    /// Returns the explicitly named sources.
    #[must_use]
    pub fn sources(&self) -> &[HarnessRuleSourceV1] {
        &self.sources
    }

    /// Returns the number of explicitly named sources.
    #[must_use]
    pub fn source_count(&self) -> u64 {
        bounded_count(self.sources.len())
    }

    /// Returns the presentation mode of this revision.
    #[must_use]
    pub const fn presentation_mode(&self) -> HarnessPresentationModeV1 {
        self.presentation_mode
    }

    /// Returns the applied project time zone of this revision.
    #[must_use]
    pub fn applied_time_zone(&self) -> &str {
        &self.applied_time_zone
    }
}

/// Continues one rule revision as the next immutable revision.
///
/// A run already admitted under a revision keeps that revision, while a
/// coalesced but not-yet-admitted reason uses the newest active revision.
///
/// # Errors
///
/// Returns `harness_revision_conflict` when `expected_revision` is not the
/// active revision, when `updated` belongs to another harness, or when
/// `updated` does not carry exactly the next revision number.
pub fn revise_harness_rule(
    active: &HarnessRuleRevisionV1,
    expected_revision: u64,
    updated: HarnessRuleRevisionV1,
) -> DtoResult<HarnessRuleRevisionV1> {
    let next = active.revision.checked_add(1).ok_or_else(|| {
        ErrorDto::validation(
            HARNESS_REVISION_CONFLICT,
            "the active revision cannot be continued",
        )
    })?;
    if expected_revision != active.revision
        || updated.harness_id != active.harness_id
        || updated.revision != next
    {
        return Err(ErrorDto::validation(
            HARNESS_REVISION_CONFLICT,
            "the update does not continue the active rule revision",
        ));
    }
    Ok(updated)
}

/// One durable harness rule with its active revision and lifecycle state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessRuleV1 {
    harness_id: [u8; 16],
    scope: HarnessRuleScopeV1,
    active_revision: HarnessRuleRevisionV1,
    lifecycle_state: HarnessRuleLifecycleStateV1,
    service_session_id: [u8; 16],
}

impl HarnessRuleV1 {
    /// Creates one active harness rule around its active revision.
    ///
    /// # Errors
    ///
    /// Returns `harness_revision_conflict` when the revision belongs to another
    /// harness identity.
    pub fn new(
        harness_id: [u8; 16],
        scope: HarnessRuleScopeV1,
        active_revision: HarnessRuleRevisionV1,
        service_session_id: [u8; 16],
    ) -> DtoResult<Self> {
        if active_revision.harness_id != harness_id {
            return Err(ErrorDto::validation(
                HARNESS_REVISION_CONFLICT,
                "a rule and its active revision share one harness identity",
            ));
        }
        Ok(Self {
            harness_id,
            scope,
            active_revision,
            lifecycle_state: HarnessRuleLifecycleStateV1::Active,
            service_session_id,
        })
    }

    /// Returns the harness identity.
    #[must_use]
    pub const fn harness_id(&self) -> [u8; 16] {
        self.harness_id
    }

    /// Returns the project or ordinary user-session scope.
    #[must_use]
    pub const fn scope(&self) -> HarnessRuleScopeV1 {
        self.scope
    }

    /// Returns the immutable active revision.
    #[must_use]
    pub const fn active_revision(&self) -> &HarnessRuleRevisionV1 {
        &self.active_revision
    }

    /// Returns the durable lifecycle state.
    #[must_use]
    pub const fn lifecycle_state(&self) -> HarnessRuleLifecycleStateV1 {
        self.lifecycle_state
    }

    /// Returns the daemon-owned service session identity of this rule.
    #[must_use]
    pub const fn service_session_id(&self) -> [u8; 16] {
        self.service_session_id
    }

    /// Returns the newest active revision used by a coalesced, not-yet-admitted
    /// trigger reason.
    #[must_use]
    pub const fn revision_for_pending_reason(&self) -> u64 {
        self.active_revision.revision
    }
}

/// One durable observation delivered to a rule's trigger capture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessTriggerObservationV1 {
    reason_id: [u8; 16],
    source_kind: HarnessSourceKindV1,
    observed_at_ms: u64,
}

impl HarnessTriggerObservationV1 {
    /// Creates one durable trigger observation.
    #[must_use]
    pub const fn new(
        reason_id: [u8; 16],
        source_kind: HarnessSourceKindV1,
        observed_at_ms: u64,
    ) -> Self {
        Self {
            reason_id,
            source_kind,
            observed_at_ms,
        }
    }

    /// Returns the stable reason identity of this observation.
    #[must_use]
    pub const fn reason_id(&self) -> [u8; 16] {
        self.reason_id
    }

    /// Returns the closed source kind of this observation.
    #[must_use]
    pub const fn source_kind(&self) -> HarnessSourceKindV1 {
        self.source_kind
    }

    /// Returns the observation time in Unix milliseconds.
    #[must_use]
    pub const fn observed_at_ms(&self) -> u64 {
        self.observed_at_ms
    }
}

/// The outcome of one durable trigger capture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessTriggerCaptureOutcomeV1 {
    /// A new single pending reason was captured.
    Captured,
    /// The observation coalesced into the existing pending reason.
    Coalesced,
    /// The same reason was redelivered; nothing new is captured.
    Redelivered,
    /// A coalesced catch-up reason was captured after downtime or a pause.
    CatchUp,
}

/// One durable trigger capture result carrying the single pending reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessTriggerCaptureV1 {
    reason: HarnessTriggerReasonV1,
    outcome: HarnessTriggerCaptureOutcomeV1,
}

impl HarnessTriggerCaptureV1 {
    /// Returns the one pending reason of the rule after capture.
    #[must_use]
    pub const fn reason(&self) -> &HarnessTriggerReasonV1 {
        &self.reason
    }

    /// Returns the capture outcome.
    #[must_use]
    pub const fn outcome(&self) -> HarnessTriggerCaptureOutcomeV1 {
        self.outcome
    }
}

/// Captures one durable trigger observation before any admission.
///
/// A redelivered reason updates nothing: redelivery never creates a second run.
/// A new observation coalesces into the single pending reason of the rule,
/// keeping its identity and source kind while extending the observation window
/// and coalesced count.
#[must_use]
pub fn capture_harness_trigger(
    pending: Option<&HarnessTriggerReasonV1>,
    observation: &HarnessTriggerObservationV1,
) -> HarnessTriggerCaptureV1 {
    match pending {
        None => HarnessTriggerCaptureV1 {
            reason: HarnessTriggerReasonV1 {
                reason_id: observation.reason_id,
                source_kind: observation.source_kind,
                first_observed_at_ms: observation.observed_at_ms,
                last_observed_at_ms: observation.observed_at_ms,
                coalesced_count: 1,
            },
            outcome: HarnessTriggerCaptureOutcomeV1::Captured,
        },
        Some(existing) if existing.reason_id == observation.reason_id => HarnessTriggerCaptureV1 {
            reason: *existing,
            outcome: HarnessTriggerCaptureOutcomeV1::Redelivered,
        },
        Some(existing) => HarnessTriggerCaptureV1 {
            reason: HarnessTriggerReasonV1 {
                reason_id: existing.reason_id,
                source_kind: existing.source_kind,
                first_observed_at_ms: existing
                    .first_observed_at_ms
                    .min(observation.observed_at_ms),
                last_observed_at_ms: existing.last_observed_at_ms.max(observation.observed_at_ms),
                coalesced_count: existing.coalesced_count.saturating_add(1),
            },
            outcome: HarnessTriggerCaptureOutcomeV1::Coalesced,
        },
    }
}

/// Captures one coalesced catch-up reason after daemon downtime or a pause.
///
/// At most one catch-up reason exists per rule; a burst of every missed slot is
/// forbidden. Returns `None` when no slot was missed or when the pending reason
/// already covers this catch-up.
#[must_use]
pub fn capture_harness_catch_up(
    pending: Option<&HarnessTriggerReasonV1>,
    reason_id: [u8; 16],
    source_kind: HarnessSourceKindV1,
    missed_slots: u64,
    first_missed_at_ms: u64,
    last_missed_at_ms: u64,
) -> Option<HarnessTriggerCaptureV1> {
    if missed_slots == 0 {
        return None;
    }
    let first = first_missed_at_ms.min(last_missed_at_ms);
    let last = first_missed_at_ms.max(last_missed_at_ms);
    let reason = pending.map_or_else(
        || HarnessTriggerReasonV1 {
            reason_id,
            source_kind,
            first_observed_at_ms: first,
            last_observed_at_ms: last,
            coalesced_count: missed_slots,
        },
        |existing| HarnessTriggerReasonV1 {
            reason_id: existing.reason_id,
            source_kind: existing.source_kind,
            first_observed_at_ms: existing.first_observed_at_ms.min(first),
            last_observed_at_ms: existing.last_observed_at_ms.max(last),
            coalesced_count: existing.coalesced_count.saturating_add(missed_slots),
        },
    );
    Some(HarnessTriggerCaptureV1 {
        reason,
        outcome: HarnessTriggerCaptureOutcomeV1::CatchUp,
    })
}

/// One durable trigger reason record with its rule and cause provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessTriggerRecordV1 {
    reason: HarnessTriggerReasonV1,
    originating_rule_revision: u64,
    applied_time_zone: String,
    cause_chain_reference: Option<[u8; 16]>,
}

impl HarnessTriggerRecordV1 {
    /// Creates one durable trigger record.
    ///
    /// # Errors
    ///
    /// Returns `harness_revision_conflict` for the zero originating revision and
    /// the errors of [`validate_harness_time_zone`] for the applied zone.
    pub fn new(
        reason: HarnessTriggerReasonV1,
        originating_rule_revision: u64,
        applied_time_zone: String,
        cause_chain_reference: Option<[u8; 16]>,
    ) -> DtoResult<Self> {
        if originating_rule_revision == 0 {
            return Err(ErrorDto::validation(
                HARNESS_REVISION_CONFLICT,
                "a trigger reason originates from a live rule revision",
            ));
        }
        validate_harness_time_zone(&applied_time_zone)?;
        Ok(Self {
            reason,
            originating_rule_revision,
            applied_time_zone,
            cause_chain_reference,
        })
    }

    /// Returns the frozen durable reason.
    #[must_use]
    pub const fn reason(&self) -> &HarnessTriggerReasonV1 {
        &self.reason
    }

    /// Returns the originating rule revision.
    #[must_use]
    pub const fn originating_rule_revision(&self) -> u64 {
        self.originating_rule_revision
    }

    /// Returns the applied project time zone at capture time.
    #[must_use]
    pub fn applied_time_zone(&self) -> &str {
        &self.applied_time_zone
    }

    /// Returns the cause-chain reference of this reason, when one exists.
    #[must_use]
    pub const fn cause_chain_reference(&self) -> Option<[u8; 16]> {
        self.cause_chain_reference
    }
}

/// The admission capacity of one trigger reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessAdmissionCapacityV1 {
    rule_slot_available: bool,
    concurrency_slot_available: bool,
}

impl HarnessAdmissionCapacityV1 {
    /// Creates one capacity observation.
    #[must_use]
    pub const fn available(rule_slot_available: bool, concurrency_slot_available: bool) -> Self {
        Self {
            rule_slot_available,
            concurrency_slot_available,
        }
    }

    /// Whether the rule has no non-terminal launch.
    #[must_use]
    pub const fn rule_slot_available(&self) -> bool {
        self.rule_slot_available
    }

    /// Whether the daemon has a free concurrency slot.
    #[must_use]
    pub const fn concurrency_slot_available(&self) -> bool {
        self.concurrency_slot_available
    }

    /// Whether both slots are available.
    #[must_use]
    pub const fn has_capacity(&self) -> bool {
        self.rule_slot_available && self.concurrency_slot_available
    }
}

/// The outcome of one trigger admission attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessTriggerAdmissionV1 {
    /// The reason admits exactly one new independent launch.
    Admitted(HarnessTriggerReasonV1),
    /// The reason is retained while a slot is unavailable; nothing launched.
    Retained(HarnessTriggerReasonV1),
}

impl HarnessTriggerAdmissionV1 {
    /// Whether the reason admitted a launch.
    #[must_use]
    pub const fn is_admitted(&self) -> bool {
        matches!(self, Self::Admitted(_))
    }

    /// Returns the durable reason of this attempt.
    #[must_use]
    pub const fn reason(&self) -> &HarnessTriggerReasonV1 {
        match self {
            Self::Admitted(reason) | Self::Retained(reason) => reason,
        }
    }
}

/// Admits or retains one pending trigger reason.
///
/// Waiting for a free concurrency slot retains the coalesced reason rather than
/// dropping it. Automatic sources are rejected while the rule is paused or
/// archived, while an explicit user launch remains allowed while paused as a
/// separate user-origin operation.
///
/// # Errors
///
/// Returns `harness_archived` for an archived rule and `harness_not_active` for
/// an automatic source of a paused rule.
pub fn admit_harness_trigger(
    pending: &HarnessTriggerReasonV1,
    state: HarnessRuleLifecycleStateV1,
    origin: HarnessLaunchOriginV1,
    capacity: HarnessAdmissionCapacityV1,
) -> DtoResult<HarnessTriggerAdmissionV1> {
    if state.is_archived() {
        return Err(ErrorDto::validation(
            HARNESS_ARCHIVED,
            "an archived harness rule admits no launch",
        ));
    }
    if origin == HarnessLaunchOriginV1::AutomaticSource && !state.permits_automatic_launch() {
        return Err(ErrorDto::validation(
            HARNESS_NOT_ACTIVE,
            "automatic sources capture and coalesce but do not launch while paused",
        ));
    }
    if capacity.has_capacity() {
        Ok(HarnessTriggerAdmissionV1::Admitted(*pending))
    } else {
        Ok(HarnessTriggerAdmissionV1::Retained(*pending))
    }
}

/// One bounded two-layer dossier built at admission.
///
/// The first layer is the immutable task digest of the active rule revision;
/// the second is a fresh bounded safe summary built from the explicit durable
/// sources, the direct trigger reason, and the latest verified checkpoint when
/// present. A dossier excludes live conversation context, transcripts, raw
/// provider items, reasoning text, credentials, grants, paths, and
/// implementation resources.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessDossierV1 {
    task_digest: Digest256,
    trigger_reason_id: [u8; 16],
    source_references: Vec<[u8; 16]>,
    typed_references: Vec<[u8; 16]>,
    checkpoint_reference: Option<[u8; 16]>,
    applied_time_zone: String,
    dossier_bytes: u64,
    dossier_digest: Digest256,
}

impl HarnessDossierV1 {
    /// Creates one bounded dossier, canonicalizing its references.
    ///
    /// Explicit sources and typed references canonicalize to ascending unique
    /// order.
    ///
    /// # Errors
    ///
    /// Returns `harness_dossier_too_large` when a reference list or the byte
    /// size exceeds its code-owned bound, and the errors of
    /// [`validate_harness_time_zone`] for the applied zone.
    #[expect(
        clippy::too_many_arguments,
        reason = "The closed eight-field dossier record keeps one validating constructor."
    )]
    pub fn new(
        task_digest: Digest256,
        trigger_reason_id: [u8; 16],
        source_references: Vec<[u8; 16]>,
        typed_references: Vec<[u8; 16]>,
        checkpoint_reference: Option<[u8; 16]>,
        applied_time_zone: String,
        dossier_bytes: u64,
        dossier_digest: Digest256,
    ) -> DtoResult<Self> {
        let mut source_references = source_references;
        source_references.sort_unstable();
        source_references.dedup();
        let mut typed_references = typed_references;
        typed_references.sort_unstable();
        typed_references.dedup();
        if bounded_count(source_references.len()) > HARNESS_MAX_SOURCES
            || bounded_count(typed_references.len()) > HARNESS_MAX_DOSSIER_REFERENCES
        {
            return Err(ErrorDto::validation(
                HARNESS_DOSSIER_TOO_LARGE,
                "the dossier reference bound is exceeded",
            ));
        }
        validate_harness_dossier_bytes(dossier_bytes)?;
        validate_harness_time_zone(&applied_time_zone)?;
        Ok(Self {
            task_digest,
            trigger_reason_id,
            source_references,
            typed_references,
            checkpoint_reference,
            applied_time_zone,
            dossier_bytes,
            dossier_digest,
        })
    }

    /// Returns the immutable task digest of the active rule revision.
    #[must_use]
    pub const fn task_digest(&self) -> Digest256 {
        self.task_digest
    }

    /// Returns the direct trigger reason identity.
    #[must_use]
    pub const fn trigger_reason_id(&self) -> [u8; 16] {
        self.trigger_reason_id
    }

    /// Returns the canonical explicit source references.
    #[must_use]
    pub fn source_references(&self) -> &[[u8; 16]] {
        &self.source_references
    }

    /// Returns the canonical typed references.
    #[must_use]
    pub fn typed_references(&self) -> &[[u8; 16]] {
        &self.typed_references
    }

    /// Returns the latest verified checkpoint reference, when present.
    #[must_use]
    pub const fn checkpoint_reference(&self) -> Option<[u8; 16]> {
        self.checkpoint_reference
    }

    /// Returns the applied project time zone.
    #[must_use]
    pub fn applied_time_zone(&self) -> &str {
        &self.applied_time_zone
    }

    /// Returns the dossier byte size.
    #[must_use]
    pub const fn dossier_bytes(&self) -> u64 {
        self.dossier_bytes
    }

    /// Returns the dossier digest.
    #[must_use]
    pub const fn dossier_digest(&self) -> Digest256 {
        self.dossier_digest
    }
}

/// The terminal outcome of one harness run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessRunOutcomeV1 {
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

/// One verified harness checkpoint.
///
/// A checkpoint is typed, versioned, digest-protected, linked to its producing
/// run, and bounded. Only a completely validated checkpoint reaches this type,
/// and an older checkpoint is never presented as the state of the current run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessVerifiedCheckpointV1 {
    checkpoint_id: [u8; 16],
    producing_run_id: [u8; 16],
    revision: u64,
    content_digest: Digest256,
    checkpoint_bytes: u64,
}

impl HarnessVerifiedCheckpointV1 {
    /// Creates one verified checkpoint.
    ///
    /// # Errors
    ///
    /// Returns `harness_revision_conflict` for the zero revision and
    /// `harness_checkpoint_too_large` when the byte size exceeds its code-owned
    /// bound.
    pub fn new(
        checkpoint_id: [u8; 16],
        producing_run_id: [u8; 16],
        revision: u64,
        content_digest: Digest256,
        checkpoint_bytes: u64,
    ) -> DtoResult<Self> {
        if revision == 0 {
            return Err(ErrorDto::validation(
                HARNESS_REVISION_CONFLICT,
                "a checkpoint is versioned from one",
            ));
        }
        validate_harness_checkpoint_bytes(checkpoint_bytes)?;
        Ok(Self {
            checkpoint_id,
            producing_run_id,
            revision,
            content_digest,
            checkpoint_bytes,
        })
    }

    /// Returns the checkpoint identity.
    #[must_use]
    pub const fn checkpoint_id(&self) -> [u8; 16] {
        self.checkpoint_id
    }

    /// Returns the producing run identity.
    #[must_use]
    pub const fn producing_run_id(&self) -> [u8; 16] {
        self.producing_run_id
    }

    /// Returns the checkpoint revision.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the checkpoint content digest.
    #[must_use]
    pub const fn content_digest(&self) -> Digest256 {
        self.content_digest
    }

    /// Returns the checkpoint byte size.
    #[must_use]
    pub const fn checkpoint_bytes(&self) -> u64 {
        self.checkpoint_bytes
    }

    /// Whether this checkpoint was produced by `run_id`.
    #[must_use]
    pub fn is_produced_by(&self, run_id: [u8; 16]) -> bool {
        self.producing_run_id == run_id
    }
}

/// Resolves the verified checkpoint after one run outcome.
///
/// A successful run may replace the checkpoint only with a completely validated
/// candidate. Failure, cancellation, interruption, `ExternalEffectUnknown`, or
/// an absent candidate retains the previous verified checkpoint and never
/// reruns the producing run.
///
/// # Errors
///
/// Returns `harness_checkpoint_unavailable` when a completed run supplies no
/// validated checkpoint.
pub fn verified_checkpoint_after_run(
    previous: Option<&HarnessVerifiedCheckpointV1>,
    outcome: HarnessRunOutcomeV1,
    candidate: Option<HarnessVerifiedCheckpointV1>,
) -> DtoResult<Option<HarnessVerifiedCheckpointV1>> {
    match outcome {
        HarnessRunOutcomeV1::Completed => candidate.map_or_else(
            || {
                Err(ErrorDto::validation(
                    HARNESS_CHECKPOINT_UNAVAILABLE,
                    "a completed run supplies its completely validated checkpoint",
                ))
            },
            |checkpoint| Ok(Some(checkpoint)),
        ),
        _ => Ok(previous.copied()),
    }
}

/// One bounded user-visible safe conclusion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessConclusionV1 {
    producing_run_id: [u8; 16],
    content_digest: Digest256,
    conclusion_bytes: u64,
}

impl HarnessConclusionV1 {
    /// Creates one safe conclusion.
    ///
    /// # Errors
    ///
    /// Returns `harness_result_too_large` when the byte size exceeds its
    /// code-owned bound; an oversized conclusion is rejected rather than
    /// truncated.
    pub fn new(
        producing_run_id: [u8; 16],
        content_digest: Digest256,
        conclusion_bytes: u64,
    ) -> DtoResult<Self> {
        validate_harness_conclusion_bytes(conclusion_bytes)?;
        Ok(Self {
            producing_run_id,
            content_digest,
            conclusion_bytes,
        })
    }

    /// Returns the producing run identity.
    #[must_use]
    pub const fn producing_run_id(&self) -> [u8; 16] {
        self.producing_run_id
    }

    /// Returns the conclusion content digest.
    #[must_use]
    pub const fn content_digest(&self) -> Digest256 {
        self.content_digest
    }

    /// Returns the conclusion byte size.
    #[must_use]
    pub const fn conclusion_bytes(&self) -> u64 {
        self.conclusion_bytes
    }
}

/// One registered read-and-delegate tool available to a harness launch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessReadDelegateToolV1 {
    /// The registered `read` tool.
    Read,
    /// The registered `glob` tool.
    Glob,
    /// The registered `grep` tool.
    Grep,
    /// The registered `expand` tool.
    Expand,
    /// The registered `retrieve` tool.
    Retrieve,
    /// The registered `sub_agent` tool, reachable only through the corridor.
    SubAgent,
}

impl HarnessReadDelegateToolV1 {
    /// The closed read-and-delegate tool set in registry order.
    pub const ALL: [Self; 6] = [
        Self::Read,
        Self::Glob,
        Self::Grep,
        Self::Expand,
        Self::Retrieve,
        Self::SubAgent,
    ];

    /// Returns the registered tool identifier.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Glob => "glob",
            Self::Grep => "grep",
            Self::Expand => "expand",
            Self::Retrieve => "retrieve",
            Self::SubAgent => "sub_agent",
        }
    }

    /// Returns the closed tool of one registered identifier.
    #[must_use]
    pub fn from_id(tool_id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|tool| tool.id() == tool_id)
    }
}

/// Returns the narrowing rank of one harness execution class.
///
/// Ranks order the classes from the narrowest to the widest so an inherited
/// base and a configured harness class can only be narrowed.
#[must_use]
pub const fn harness_class_rank(class: HarnessExecutionClassV1) -> u8 {
    match class {
        HarnessExecutionClassV1::Light => 0,
        HarnessExecutionClassV1::Medium => 1,
        HarnessExecutionClassV1::Heavy => 2,
    }
}

/// Resolves one harness class by inherited narrowing.
///
/// The resolved class is the narrower of the requested and configured classes;
/// class resolution never widens the requested class.
#[must_use]
pub const fn resolve_harness_class(
    requested: HarnessExecutionClassV1,
    configured: HarnessExecutionClassV1,
) -> HarnessExecutionClassV1 {
    if harness_class_rank(configured) <= harness_class_rank(requested) {
        configured
    } else {
        requested
    }
}

/// Validates that one class resolution did not widen the requested class.
///
/// # Errors
///
/// Returns `invalid_harness_class_resolution` when the resolved class is wider
/// than the requested class.
pub fn validate_harness_class_narrowing(
    requested: HarnessExecutionClassV1,
    resolved: HarnessExecutionClassV1,
) -> DtoResult<()> {
    if harness_class_rank(resolved) > harness_class_rank(requested) {
        return Err(ErrorDto::validation(
            INVALID_HARNESS_CLASS_RESOLUTION,
            "a harness class resolution never widens the requested class",
        ));
    }
    Ok(())
}

/// The fixed selection of one programmatic-caller corridor that gates
/// `sub_agent` use.
///
/// The corridor fixes the permitted class, depth, and child count; a launch
/// creates a fresh run-bound use of that selection and cannot widen it. A
/// harness never prepares a new corridor or receives a fallback authorization
/// when the corridor is absent, expired, suspended, revoked, exhausted, or
/// incompatible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessSubAgentCorridorV1 {
    corridor_reference: [u8; 16],
    permitted_class: HarnessExecutionClassV1,
    permitted_depth: u64,
    permitted_child_count: u64,
}

impl HarnessSubAgentCorridorV1 {
    /// Creates one corridor reference with its fixed limits.
    ///
    /// # Errors
    ///
    /// Returns `invalid_harness_class_resolution` for a zero or over-bound
    /// depth or child count.
    pub fn new(
        corridor_reference: [u8; 16],
        permitted_class: HarnessExecutionClassV1,
        permitted_depth: u64,
        permitted_child_count: u64,
    ) -> DtoResult<Self> {
        if permitted_depth == 0
            || permitted_depth > HARNESS_MAX_CAUSE_DEPTH
            || permitted_child_count == 0
            || permitted_child_count > HARNESS_MAX_CONCURRENT
        {
            return Err(ErrorDto::validation(
                INVALID_HARNESS_CLASS_RESOLUTION,
                "a corridor fixes bounded depth and child count",
            ));
        }
        Ok(Self {
            corridor_reference,
            permitted_class,
            permitted_depth,
            permitted_child_count,
        })
    }

    /// Returns the corridor selection identity.
    #[must_use]
    pub const fn corridor_reference(&self) -> [u8; 16] {
        self.corridor_reference
    }

    /// Returns the class fixed by the corridor.
    #[must_use]
    pub const fn permitted_class(&self) -> HarnessExecutionClassV1 {
        self.permitted_class
    }

    /// Returns the depth fixed by the corridor.
    #[must_use]
    pub const fn permitted_depth(&self) -> u64 {
        self.permitted_depth
    }

    /// Returns the child count fixed by the corridor.
    #[must_use]
    pub const fn permitted_child_count(&self) -> u64 {
        self.permitted_child_count
    }

    /// Validates one fresh run-bound use of this corridor.
    ///
    /// # Errors
    ///
    /// Returns `invalid_harness_class_resolution` when the use widens the
    /// corridor class, depth, or child count.
    pub fn validate_use(
        &self,
        resolved_class: HarnessExecutionClassV1,
        depth: u64,
        child_count: u64,
    ) -> DtoResult<()> {
        if harness_class_rank(resolved_class) > harness_class_rank(self.permitted_class)
            || depth == 0
            || depth > self.permitted_depth
            || child_count > self.permitted_child_count
        {
            return Err(ErrorDto::validation(
                INVALID_HARNESS_CLASS_RESOLUTION,
                "a launch may only narrow the corridor it was admitted under",
            ));
        }
        Ok(())
    }
}

/// Validates one narrowed read-and-delegate tool selection.
///
/// The closed allowed tools are `read`, `glob`, `grep`, `expand`, `retrieve`,
/// and `sub_agent`. Direct write, edit, process start, network retrieval, user
/// interaction, and model-created rule changes are outside this scope, and
/// `sub_agent` is admitted only through the programmatic-policy corridor.
///
/// # Errors
///
/// Returns `invalid_harness_class_resolution` when the selection exceeds its
/// bound, repeats a tool, names a tool outside the closed set, or selects
/// `sub_agent` without a corridor.
pub fn validate_harness_narrowed_tools(
    tool_ids: &[String],
    corridor: Option<&HarnessSubAgentCorridorV1>,
) -> DtoResult<()> {
    if tool_ids.len() > HARNESS_MAX_NARROWED_TOOLS {
        return Err(ErrorDto::validation(
            INVALID_HARNESS_CLASS_RESOLUTION,
            "the narrowed tool selection exceeds its bound",
        ));
    }
    let mut selected_sub_agent = false;
    for (index, tool_id) in tool_ids.iter().enumerate() {
        let Some(tool) = HarnessReadDelegateToolV1::from_id(tool_id) else {
            return Err(ErrorDto::validation(
                INVALID_HARNESS_CLASS_RESOLUTION,
                "a narrowed tool is outside the read-and-delegate set",
            ));
        };
        if tool_ids
            .iter()
            .take(index)
            .any(|earlier| earlier == tool_id)
        {
            return Err(ErrorDto::validation(
                INVALID_HARNESS_CLASS_RESOLUTION,
                "the narrowed tool selection repeats a tool",
            ));
        }
        selected_sub_agent |= tool == HarnessReadDelegateToolV1::SubAgent;
    }
    if selected_sub_agent && corridor.is_none() {
        return Err(ErrorDto::validation(
            INVALID_HARNESS_CLASS_RESOLUTION,
            "sub_agent requires the programmatic-policy corridor",
        ));
    }
    Ok(())
}

/// One separately admitted goal continuation of a goal-directed rule
/// (EXC-041).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessGoalContinuationV1 {
    goal_id: [u8; 16],
    goal_revision: u64,
    rule_revision: u64,
    launch_reference: [u8; 16],
}

impl HarnessGoalContinuationV1 {
    /// Returns the active goal identity.
    #[must_use]
    pub const fn goal_id(&self) -> [u8; 16] {
        self.goal_id
    }

    /// Returns the exact live goal revision.
    #[must_use]
    pub const fn goal_revision(&self) -> u64 {
        self.goal_revision
    }

    /// Returns the rule revision that admitted the continuation.
    #[must_use]
    pub const fn rule_revision(&self) -> u64 {
        self.rule_revision
    }

    /// Returns the separately admitted launch identity of this continuation.
    #[must_use]
    pub const fn launch_reference(&self) -> [u8; 16] {
        self.launch_reference
    }
}

/// Plans one separately admitted goal continuation of a goal-directed rule.
///
/// Goal-directed continuation is never a free-running agent: every continuation
/// is bound to its own separately admitted launch identity and to the exact
/// live goal revision. A repeated-task rule yields no continuation.
///
/// # Errors
///
/// Returns `harness_archived` for an archived rule, `harness_not_active` when no
/// active goal is available to continue, and `harness_revision_conflict` for a
/// zero goal revision.
pub fn plan_harness_goal_continuation(
    revision: &HarnessRuleRevisionV1,
    state: HarnessRuleLifecycleStateV1,
    goal: Option<([u8; 16], u64)>,
    launch_reference: [u8; 16],
) -> DtoResult<Option<HarnessGoalContinuationV1>> {
    if revision.task_mode != HarnessTaskModeV1::GoalDirected {
        return Ok(None);
    }
    if state.is_archived() {
        return Err(ErrorDto::validation(
            HARNESS_ARCHIVED,
            "an archived rule never continues a goal",
        ));
    }
    let Some((goal_id, goal_revision)) = goal else {
        return Err(ErrorDto::validation(
            HARNESS_NOT_ACTIVE,
            "goal mode continues only against an active goal",
        ));
    };
    if goal_revision == 0 {
        return Err(ErrorDto::validation(
            HARNESS_REVISION_CONFLICT,
            "a goal continuation binds one exact live goal revision",
        ));
    }
    Ok(Some(HarnessGoalContinuationV1 {
        goal_id,
        goal_revision,
        rule_revision: revision.revision,
        launch_reference,
    }))
}

/// One client-connection observation of a durable harness rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessClientConnectionV1 {
    /// A client is connected.
    Connected,
    /// No client is connected.
    Disconnected,
}

impl HarnessClientConnectionV1 {
    /// Returns the EXC-042 post-disconnect contract for this connection state.
    ///
    /// The contract is identical for both states by design: capture and
    /// coalescing are client-independent.
    #[must_use]
    pub const fn disconnect_contract(self) -> HarnessDisconnectContractV1 {
        HarnessDisconnectContractV1::frozen()
    }
}

/// The durable EXC-042 post-disconnect contract of one harness rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessDisconnectContractV1 {
    capture_continues: bool,
    journal_readable_after_reconnect: bool,
    requires_separate_admission: bool,
    external_work_resumes: bool,
}

impl HarnessDisconnectContractV1 {
    /// The one live post-disconnect contract.
    ///
    /// Durable capture and coalescing continue without a connected client, the
    /// journal is readable after reconnect, later work is a separately admitted
    /// launch with new identities, and no previously started external work
    /// resumes.
    #[must_use]
    pub const fn frozen() -> Self {
        Self {
            capture_continues: true,
            journal_readable_after_reconnect: true,
            requires_separate_admission: true,
            external_work_resumes: false,
        }
    }

    /// Creates one observed post-disconnect state for validation.
    #[must_use]
    pub const fn new(
        capture_continues: bool,
        journal_readable_after_reconnect: bool,
        requires_separate_admission: bool,
        external_work_resumes: bool,
    ) -> Self {
        Self {
            capture_continues,
            journal_readable_after_reconnect,
            requires_separate_admission,
            external_work_resumes,
        }
    }

    /// Whether durable trigger capture and coalescing continue.
    #[must_use]
    pub const fn capture_continues(&self) -> bool {
        self.capture_continues
    }

    /// Whether the harness journal is readable after reconnect.
    #[must_use]
    pub const fn journal_readable_after_reconnect(&self) -> bool {
        self.journal_readable_after_reconnect
    }

    /// Whether later work needs separate admission with new identities.
    #[must_use]
    pub const fn requires_separate_admission(&self) -> bool {
        self.requires_separate_admission
    }

    /// Whether any previously started external work resumes.
    #[must_use]
    pub const fn external_work_resumes(&self) -> bool {
        self.external_work_resumes
    }
}

/// Validates one observed post-disconnect behavior against the EXC-042
/// contract.
///
/// # Errors
///
/// Returns `invalid_harness_disconnect_contract` when the observed state is not
/// the one live contract, including any state that would resume old external
/// work.
pub fn validate_harness_disconnect_contract(
    observed: &HarnessDisconnectContractV1,
) -> DtoResult<()> {
    if *observed == HarnessDisconnectContractV1::frozen() {
        Ok(())
    } else {
        Err(ErrorDto::validation(
            INVALID_HARNESS_DISCONNECT_CONTRACT,
            "post-disconnect work never resumes old external work",
        ))
    }
}
