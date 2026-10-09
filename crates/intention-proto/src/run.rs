//! Run policy, durable lifecycle status, and run projection values.

use serde::{Deserialize, Serialize};

use crate::{
    ConfigRevisionId, DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto, RunId, SessionId,
    TurnId,
};

/// The agent policy active for a run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
                ErrorRetryDto::Never,
                None,
            )?),
        }
    }
}

/// The durable lifecycle status for a run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
                ErrorRetryDto::Never,
                None,
            )?),
        }
    }
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
            ErrorRetryDto::Never,
            None,
        )?)
    }
}

/// A safe current projection of one durable run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunProjectionDto {
    session_id: SessionId,
    run_id: RunId,
    turn_id: TurnId,
    status: RunStatusDto,
    config_revision_id: ConfigRevisionId,
}

impl RunProjectionDto {
    /// Creates the safe public projection of one M3 run lifecycle.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        run_id: RunId,
        turn_id: TurnId,
        status: RunStatusDto,
        config_revision_id: ConfigRevisionId,
    ) -> Self {
        Self {
            session_id,
            run_id,
            turn_id,
            status,
            config_revision_id,
        }
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

    /// Returns the immutable configuration revision selected by this run.
    #[must_use]
    pub const fn config_revision_id(self) -> ConfigRevisionId {
        self.config_revision_id
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
            rejected.as_ref().err().map(ErrorDto::retry),
            Some(ErrorRetryDto::Never)
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
            rejected.as_ref().err().map(ErrorDto::retry),
            Some(ErrorRetryDto::Never)
        );
    }
}
