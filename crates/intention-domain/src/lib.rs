//! Domain DTO foundations and value validation for Intention Relay.
//!
//! This crate establishes typed domain vocabulary only. Durable state transitions,
//! storage transactions, queue policy, and runtime actors remain in later milestones.

use std::fmt::{Display, Formatter};

use intention_types::{
    ConfigRevisionId, DtoResult, ErrorDto, PlanId, ProjectId, RunId, SessionId, TimestampDto,
    TurnId,
};
use serde::{Deserialize, Deserializer, Serialize, de};

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
    /// The input was accepted but no run actor has started it.
    Queued,
    /// The run is initializing its immutable context.
    Starting,
    /// The run is actively receiving model or tool work.
    Running,
    /// The run requires a user answer or permission result.
    WaitingInput,
    /// The run is committing its terminal result.
    Completing,
    /// A cancellation request is in progress.
    Cancelling,
    /// The run completed successfully.
    Completed,
    /// The run was cancelled by user or policy.
    Cancelled,
    /// The run encountered an unrecoverable safe failure.
    Failed,
    /// Daemon recovery ended an unfinished run without retrying it.
    Interrupted,
}

/// The durable lifecycle status for a physical plan.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatusDto {
    /// The plan exists and can be edited.
    Drafting,
    /// The plan body is being revised.
    Revising,
    /// The plan awaits a user decision.
    Submitted,
    /// The user accepted the plan.
    Approved,
    /// The user rejected the plan and may provide feedback.
    Rejected,
    /// A later plan superseded this plan.
    Superseded,
    /// The plan was explicitly abandoned.
    Abandoned,
}

/// A typed workspace path declared by the session boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkspaceRootDto(String);

impl<'de> Deserialize<'de> for WorkspaceRootDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

impl WorkspaceRootDto {
    /// Parses an absolute, non-empty workspace path without resolving it.
    ///
    /// Resolution, symlink policy, and filesystem containment checks belong to
    /// `intention-workspace` in M5.
    ///
    /// # Errors
    ///
    /// Returns a validation error if `value` is empty or not absolute.
    pub fn parse(value: impl Into<String>) -> DtoResult<Self> {
        let value = value.into();
        if value.trim().is_empty() || !value.starts_with('/') {
            Err(ErrorDto::validation(
                "invalid_workspace_root",
                "workspace root must be a non-empty absolute path",
            ))
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the declared workspace path without attempting filesystem access.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for WorkspaceRootDto {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A command requesting that the daemon accept a new user turn.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SendUserTurnCommandDto {
    session_id: SessionId,
    turn_id: TurnId,
    content: String,
}

impl<'de> Deserialize<'de> for SendUserTurnCommandDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawSendUserTurnCommandDto {
            session_id: SessionId,
            turn_id: TurnId,
            content: String,
        }

        let raw = RawSendUserTurnCommandDto::deserialize(deserializer)?;
        Self::new(raw.session_id, raw.turn_id, raw.content).map_err(de::Error::custom)
    }
}

impl SendUserTurnCommandDto {
    /// Creates a command with a non-empty user message.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the requested content is empty.
    pub fn new(
        session_id: SessionId,
        turn_id: TurnId,
        content: impl Into<String>,
    ) -> DtoResult<Self> {
        let content = content.into();
        if content.trim().is_empty() {
            Err(ErrorDto::validation(
                "invalid_turn_content",
                "user turn content must not be empty",
            ))
        } else {
            Ok(Self {
                session_id,
                turn_id,
                content,
            })
        }
    }

    /// Returns the target session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the stable requested turn identity.
    #[must_use]
    pub const fn turn_id(&self) -> TurnId {
        self.turn_id
    }

    /// Returns the requested user-authored content.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// A command requesting cancellation of an active run.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StopRunCommandDto {
    session_id: SessionId,
    run_id: RunId,
}

impl StopRunCommandDto {
    /// Creates a typed cancellation request.
    #[must_use]
    pub const fn new(session_id: SessionId, run_id: RunId) -> Self {
        Self { session_id, run_id }
    }

    /// Returns the session that owns the target run.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }

    /// Returns the active run requested for cancellation.
    #[must_use]
    pub const fn run_id(self) -> RunId {
        self.run_id
    }
}

/// A query requesting one current session projection or snapshot.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GetSessionSnapshotQueryDto {
    session_id: SessionId,
}

impl GetSessionSnapshotQueryDto {
    /// Creates a typed session projection query.
    #[must_use]
    pub const fn new(session_id: SessionId) -> Self {
        Self { session_id }
    }

    /// Returns the target session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }
}

/// A command requesting allocation of a new physical plan.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreatePlanCommandDto {
    session_id: SessionId,
    plan_id: PlanId,
}

impl CreatePlanCommandDto {
    /// Creates a typed plan allocation request.
    #[must_use]
    pub const fn new(session_id: SessionId, plan_id: PlanId) -> Self {
        Self {
            session_id,
            plan_id,
        }
    }

    /// Returns the target session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }

    /// Returns the stable requested plan identity.
    #[must_use]
    pub const fn plan_id(self) -> PlanId {
        self.plan_id
    }
}

/// An immutable fact carried by a domain event envelope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum DomainEventDto {
    /// A new session became durable with its declared workspace and policy mode.
    SessionCreated(SessionCreatedEventDto),
    /// A run state changed through a later application/runtime workflow.
    RunStatusChanged(RunStatusChangedEventDto),
    /// A configuration revision was accepted by a later persistence workflow.
    ConfigurationRevisionAccepted(ConfigurationRevisionAcceptedEventDto),
    /// A plan status changed through a later plan policy workflow.
    PlanStatusChanged(PlanStatusChangedEventDto),
}

/// The fact that a session was created with a mandatory workspace boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionCreatedEventDto {
    project_id: ProjectId,
    session_id: SessionId,
    workspace_root: WorkspaceRootDto,
    mode: RunModeDto,
    occurred_at: TimestampDto,
}

impl SessionCreatedEventDto {
    /// Creates the session-creation event payload.
    #[must_use]
    pub const fn new(
        project_id: ProjectId,
        session_id: SessionId,
        workspace_root: WorkspaceRootDto,
        mode: RunModeDto,
        occurred_at: TimestampDto,
    ) -> Self {
        Self {
            project_id,
            session_id,
            workspace_root,
            mode,
            occurred_at,
        }
    }

    /// Returns the owning project identity.
    #[must_use]
    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    /// Returns the created session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the declared workspace boundary.
    #[must_use]
    pub const fn workspace_root(&self) -> &WorkspaceRootDto {
        &self.workspace_root
    }

    /// Returns the initial run policy mode.
    #[must_use]
    pub const fn mode(&self) -> RunModeDto {
        self.mode
    }

    /// Returns the event occurrence time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// A future durable run lifecycle transition payload.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunStatusChangedEventDto {
    session_id: SessionId,
    run_id: RunId,
    status: RunStatusDto,
    occurred_at: TimestampDto,
}

impl RunStatusChangedEventDto {
    /// Creates a typed run-status transition fact.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        run_id: RunId,
        status: RunStatusDto,
        occurred_at: TimestampDto,
    ) -> Self {
        Self {
            session_id,
            run_id,
            status,
            occurred_at,
        }
    }
}

/// A future durable configuration-revision acceptance payload.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfigurationRevisionAcceptedEventDto {
    session_id: SessionId,
    revision: ConfigRevisionId,
    occurred_at: TimestampDto,
}

impl ConfigurationRevisionAcceptedEventDto {
    /// Creates a typed configuration-revision fact.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        revision: ConfigRevisionId,
        occurred_at: TimestampDto,
    ) -> Self {
        Self {
            session_id,
            revision,
            occurred_at,
        }
    }
}

/// A future durable plan-status transition payload.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlanStatusChangedEventDto {
    session_id: SessionId,
    plan_id: PlanId,
    status: PlanStatusDto,
    occurred_at: TimestampDto,
}

impl PlanStatusChangedEventDto {
    /// Creates a typed plan-status transition fact.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        plan_id: PlanId,
        status: PlanStatusDto,
        occurred_at: TimestampDto,
    ) -> Self {
        Self {
            session_id,
            plan_id,
            status,
            occurred_at,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;
    use intention_types::{EventId, SchemaVersionDto, SessionEventSequenceDto};

    fn fixture_time() -> TimestampDto {
        TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid")
    }

    #[test]
    fn all_domain_statuses_and_commands_round_trip() {
        for mode in [RunModeDto::Plan, RunModeDto::Build] {
            let encoded = serde_json::to_string(&mode).expect("mode serialization succeeds");
            let decoded: RunModeDto =
                serde_json::from_str(&encoded).expect("mode parsing succeeds");
            assert_eq!(decoded, mode);
        }
        for status in [
            RunStatusDto::Queued,
            RunStatusDto::Starting,
            RunStatusDto::Running,
            RunStatusDto::WaitingInput,
            RunStatusDto::Completing,
            RunStatusDto::Cancelling,
            RunStatusDto::Completed,
            RunStatusDto::Cancelled,
            RunStatusDto::Failed,
            RunStatusDto::Interrupted,
        ] {
            let encoded = serde_json::to_string(&status).expect("status serialization succeeds");
            let decoded: RunStatusDto =
                serde_json::from_str(&encoded).expect("status parsing succeeds");
            assert_eq!(decoded, status);
        }
        for status in [
            PlanStatusDto::Drafting,
            PlanStatusDto::Revising,
            PlanStatusDto::Submitted,
            PlanStatusDto::Approved,
            PlanStatusDto::Rejected,
            PlanStatusDto::Superseded,
            PlanStatusDto::Abandoned,
        ] {
            let encoded = serde_json::to_string(&status).expect("status serialization succeeds");
            let decoded: PlanStatusDto =
                serde_json::from_str(&encoded).expect("status parsing succeeds");
            assert_eq!(decoded, status);
        }

        let session_id = SessionId::new();
        let run_id = RunId::new();
        let plan_id = PlanId::new();
        let stop = StopRunCommandDto::new(session_id, run_id);
        assert_eq!(stop.session_id(), session_id);
        assert_eq!(stop.run_id(), run_id);
        let query = GetSessionSnapshotQueryDto::new(session_id);
        assert_eq!(query.session_id(), session_id);
        let plan = CreatePlanCommandDto::new(session_id, plan_id);
        assert_eq!(plan.session_id(), session_id);
        assert_eq!(plan.plan_id(), plan_id);
    }

    #[test]
    fn session_and_future_event_payloads_expose_valid_domain_shapes() {
        let project_id = ProjectId::new();
        let session_id = SessionId::new();
        let workspace = WorkspaceRootDto::parse("/workspace").expect("absolute path is valid");
        assert_eq!(workspace.as_str(), "/workspace");
        assert_eq!(workspace.to_string(), "/workspace");
        let created = SessionCreatedEventDto::new(
            project_id,
            session_id,
            workspace.clone(),
            RunModeDto::Build,
            fixture_time(),
        );
        assert_eq!(created.project_id(), project_id);
        assert_eq!(created.session_id(), session_id);
        assert_eq!(created.workspace_root(), &workspace);
        assert_eq!(created.mode(), RunModeDto::Build);
        assert_eq!(created.occurred_at(), fixture_time());

        let events = [
            DomainEventDto::SessionCreated(created),
            DomainEventDto::RunStatusChanged(RunStatusChangedEventDto::new(
                session_id,
                RunId::new(),
                RunStatusDto::Running,
                fixture_time(),
            )),
            DomainEventDto::ConfigurationRevisionAccepted(
                ConfigurationRevisionAcceptedEventDto::new(
                    session_id,
                    ConfigRevisionId::new(),
                    fixture_time(),
                ),
            ),
            DomainEventDto::PlanStatusChanged(PlanStatusChangedEventDto::new(
                session_id,
                PlanId::new(),
                PlanStatusDto::Approved,
                fixture_time(),
            )),
        ];
        for event in events {
            let envelope = intention_types::EventEnvelopeDto::new(
                intention_types::EventMetadataDto::new(
                    SchemaVersionDto::new(1, 0),
                    EventId::new(),
                    session_id,
                    None,
                    None,
                    SessionEventSequenceDto::new(1),
                    fixture_time(),
                ),
                event,
            );
            let encoded = serde_json::to_string(&envelope).expect("event serialization succeeds");
            let _: intention_types::EventEnvelopeDto<DomainEventDto> =
                serde_json::from_str(&encoded).expect("event parsing succeeds");
        }
    }
}
