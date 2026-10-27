//! Run policy, durable lifecycle status, and run projection values.

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    DtoResult, ErrorCategoryDto, ErrorDto, RunId, SessionId, TimestampDto, TurnId, UsageDto,
};

/// The agent policy active for a run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RunModeDto {
    /// Research and author a physical plan while ordinary project mutation is denied.
    Plan,
    /// Perform work through the configured Build-mode tool policy.
    Build,
}

impl RunModeDto {
    /// Returns the canonical durable string representation of this run mode.
    ///
    /// The representation is persisted verbatim, so it must stay byte-identical
    /// across releases.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Build => "build",
        }
    }

    /// Parses the canonical durable string representation of a run mode.
    ///
    /// # Errors
    ///
    /// Returns a safe internal error when `value` is not a declared durable run mode.
    pub fn parse(value: &str) -> DtoResult<Self> {
        match value {
            "plan" => Ok(Self::Plan),
            "build" => Ok(Self::Build),
            _ => Err(ErrorDto::new(
                "invalid_run_mode",
                ErrorCategoryDto::Internal,
                "the durable run mode is not declared",
                false,
            )?),
        }
    }

    /// The declared durable run modes, in `as_str` order.
    ///
    /// This is the single source the command grammar renders in its usage and
    /// rejection text, and the proto tests pin it to the wire spelling of every
    /// member.
    pub const ALL: [Self; 2] = [Self::Plan, Self::Build];

    /// The durable run mode a session starts with when its creation command declares none.
    ///
    /// The application service resolves an absent caller mode to this value, so
    /// no front end decides the durable policy of a created session.
    pub const DEFAULT: Self = Self::Build;
}

/// The durable lifecycle status for a run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RunStatusDto {
    /// The run is initializing its immutable context.
    Starting,
    /// The run is actively receiving model or tool work.
    Running,
    /// The run completed successfully.
    Completed,
    /// The run encountered an unrecoverable safe failure.
    Failed,
    /// Daemon recovery ended an unfinished run without retrying it.
    Interrupted,
}

impl RunStatusDto {
    /// Returns the canonical durable string representation of this run status.
    ///
    /// The representation is persisted verbatim, so it must stay byte-identical
    /// across releases.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    /// Parses the canonical durable string representation of a run status.
    ///
    /// # Errors
    ///
    /// Returns a safe internal error when `value` is not a declared durable run status.
    pub fn parse(value: &str) -> DtoResult<Self> {
        match value {
            "starting" => Ok(Self::Starting),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(ErrorDto::new(
                "invalid_run_status",
                ErrorCategoryDto::Internal,
                "the durable run status is not declared",
                false,
            )?),
        }
    }

    /// The durable terminal run statuses, in `as_str` order.
    ///
    /// This is the single source for the storage terminal-status predicate that
    /// excludes finished runs, and the proto tests pin it to
    /// [`run_status_is_terminal`] and to the wire spelling of every member.
    pub const TERMINAL: [Self; 3] = [Self::Completed, Self::Failed, Self::Interrupted];
}

/// Returns whether no future run status transition is valid from `status`.
#[must_use]
pub const fn run_status_is_terminal(status: RunStatusDto) -> bool {
    matches!(
        status,
        RunStatusDto::Completed | RunStatusDto::Failed | RunStatusDto::Interrupted
    )
}

/// Validates one durable run lifecycle transition without accessing runtime or storage state.
///
/// # Errors
///
/// Returns a conflict error when `to` is not a declared successor of `from`.
pub fn validate_run_status_transition(from: RunStatusDto, to: RunStatusDto) -> DtoResult<()> {
    let allowed = matches!(
        (from, to),
        (RunStatusDto::Starting, RunStatusDto::Running)
            | (RunStatusDto::Starting, RunStatusDto::Failed)
            | (RunStatusDto::Starting, RunStatusDto::Interrupted)
            | (RunStatusDto::Running, RunStatusDto::Completed)
            | (RunStatusDto::Running, RunStatusDto::Failed)
            | (RunStatusDto::Running, RunStatusDto::Interrupted)
    );
    if allowed {
        Ok(())
    } else {
        Err(ErrorDto::new(
            "invalid_run_status_transition",
            ErrorCategoryDto::Conflict,
            "run status transition is not permitted",
            false,
        )?)
    }
}

/// A safe current projection of one durable run.
///
/// The projection carries whole committed facts: the durable lifecycle status,
/// the run's committed token usage (absent until a terminal outcome records
/// it), the committed start and finish times in whole Unix seconds, and how
/// many committed tool calls the run has not answered yet. `finished_at` is
/// present exactly when the status is terminal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RunProjectionDto {
    session_id: SessionId,
    run_id: RunId,
    turn_id: TurnId,
    status: RunStatusDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    usage: Option<UsageDto>,
    started_at: TimestampDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    finished_at: Option<TimestampDto>,
    open_tool_calls: u32,
}

impl<'de> Deserialize<'de> for RunProjectionDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawRunProjectionDto {
            session_id: SessionId,
            run_id: RunId,
            turn_id: TurnId,
            status: RunStatusDto,
            #[serde(default)]
            usage: Option<UsageDto>,
            started_at: TimestampDto,
            #[serde(default)]
            finished_at: Option<TimestampDto>,
            open_tool_calls: u32,
        }

        let raw = RawRunProjectionDto::deserialize(deserializer)?;
        Self::new(
            raw.session_id,
            raw.run_id,
            raw.turn_id,
            raw.status,
            raw.usage,
            raw.started_at,
            raw.finished_at,
            raw.open_tool_calls,
        )
        .map_err(de::Error::custom)
    }
}

impl RunProjectionDto {
    /// Creates the safe public projection of one M3 run lifecycle.
    ///
    /// # Errors
    ///
    /// Returns a validation error when a terminal status and a finish time
    /// disagree, or when the finish time precedes the start time.
    #[expect(
        clippy::too_many_arguments,
        reason = "This public wire constructor preserves the established eight-field run projection contract."
    )]
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        turn_id: TurnId,
        status: RunStatusDto,
        usage: Option<UsageDto>,
        started_at: TimestampDto,
        finished_at: Option<TimestampDto>,
        open_tool_calls: u32,
    ) -> DtoResult<Self> {
        if run_status_is_terminal(status) != finished_at.is_some() {
            return Err(ErrorDto::validation(
                "invalid_run_projection",
                "a terminal run projection requires its finish time and a live one must not carry one",
            ));
        }
        if finished_at.is_some_and(|finished| finished < started_at) {
            return Err(ErrorDto::validation(
                "invalid_run_projection",
                "a run finish time never precedes its start time",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            turn_id,
            status,
            usage,
            started_at,
            finished_at,
            open_tool_calls,
        })
    }

    /// Returns the owning session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }

    /// Returns the durable run identity.
    #[must_use]
    pub const fn run_id(self) -> RunId {
        self.run_id
    }

    /// Returns the causal user turn identity.
    #[must_use]
    pub const fn turn_id(self) -> TurnId {
        self.turn_id
    }

    /// Returns the durable lifecycle status.
    #[must_use]
    pub const fn status(self) -> RunStatusDto {
        self.status
    }

    /// Returns the committed token usage, absent until a terminal outcome
    /// records it.
    #[must_use]
    pub const fn usage(self) -> Option<UsageDto> {
        self.usage
    }

    /// Returns when the run was admitted, in whole Unix seconds.
    #[must_use]
    pub const fn started_at(self) -> TimestampDto {
        self.started_at
    }

    /// Returns when the run reached a terminal status, absent while it is live.
    #[must_use]
    pub const fn finished_at(self) -> Option<TimestampDto> {
        self.finished_at
    }

    /// Returns how many committed tool calls the run has not answered yet.
    #[must_use]
    pub const fn open_tool_calls(self) -> u32 {
        self.open_tool_calls
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_run_modes_round_trip_their_canonical_strings() {
        assert_eq!(RunModeDto::Plan.as_str(), "plan");
        assert_eq!(RunModeDto::Build.as_str(), "build");
        for mode in [RunModeDto::Plan, RunModeDto::Build] {
            assert_eq!(RunModeDto::parse(mode.as_str()).ok(), Some(mode));
        }
        let rejected = RunModeDto::parse("Plan");
        assert_eq!(
            rejected.as_ref().err().map(ErrorDto::code),
            Some("invalid_run_mode")
        );
        assert_eq!(
            rejected.as_ref().err().map(ErrorDto::category),
            Some(ErrorCategoryDto::Internal)
        );
        assert_eq!(
            rejected.as_ref().err().map(ErrorDto::retryable),
            Some(false)
        );
    }

    #[test]
    fn every_declared_run_mode_round_trips() {
        assert_eq!(
            RunModeDto::ALL.map(RunModeDto::as_str),
            ["plan", "build"],
            "the declared set is every durable run mode in as_str order"
        );
        for mode in RunModeDto::ALL {
            assert_eq!(RunModeDto::parse(mode.as_str()).ok(), Some(mode));
        }
        assert!(
            RunModeDto::ALL.contains(&RunModeDto::DEFAULT),
            "the durable default is a declared run mode"
        );
    }

    #[test]
    fn durable_run_statuses_round_trip_their_canonical_strings() {
        assert_eq!(RunStatusDto::Starting.as_str(), "starting");
        assert_eq!(RunStatusDto::Running.as_str(), "running");
        assert_eq!(RunStatusDto::Completed.as_str(), "completed");
        assert_eq!(RunStatusDto::Failed.as_str(), "failed");
        assert_eq!(RunStatusDto::Interrupted.as_str(), "interrupted");
        for status in [
            RunStatusDto::Starting,
            RunStatusDto::Running,
            RunStatusDto::Completed,
            RunStatusDto::Failed,
            RunStatusDto::Interrupted,
        ] {
            assert_eq!(RunStatusDto::parse(status.as_str()).ok(), Some(status));
        }
        let rejected = RunStatusDto::parse("running ");
        assert_eq!(
            rejected.as_ref().err().map(ErrorDto::code),
            Some("invalid_run_status")
        );
        assert_eq!(
            rejected.as_ref().err().map(ErrorDto::category),
            Some(ErrorCategoryDto::Internal)
        );
        assert_eq!(
            rejected.as_ref().err().map(ErrorDto::retryable),
            Some(false)
        );
    }
}
