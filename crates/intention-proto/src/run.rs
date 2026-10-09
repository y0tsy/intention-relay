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
