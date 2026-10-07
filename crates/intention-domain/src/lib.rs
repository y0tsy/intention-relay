//! Domain DTO foundations and value validation for Intention Relay.
//!
//! This crate establishes typed domain vocabulary only. Durable state transitions,
//! storage transactions, queue policy, and runtime actors remain in later milestones.

use std::fmt::{Display, Formatter};

use intention_types::{
    ConfigRevisionId, DtoResult, ErrorDto, IdempotencyKey, ProjectId, RunId, SessionId, ToolCallId,
    TurnId, WorkspaceId,
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
    /// The run is initializing its immutable context.
    Starting,
    /// The run is actively receiving model or tool work.
    Running,
    /// The run requires a user answer or permission result.
    WaitingInput,
    /// The run completed successfully.
    Completed,
    /// The run encountered an unrecoverable safe failure.
    Failed,
    /// Daemon recovery ended an unfinished run without retrying it.
    Interrupted,
}

impl RunStatusDto {
    /// Returns whether no future status transition is valid from this status.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Interrupted)
    }
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
            | (RunStatusDto::Starting, RunStatusDto::WaitingInput)
            | (RunStatusDto::Starting, RunStatusDto::Failed)
            | (RunStatusDto::Starting, RunStatusDto::Interrupted)
            | (RunStatusDto::Running, RunStatusDto::WaitingInput)
            | (RunStatusDto::Running, RunStatusDto::Completed)
            | (RunStatusDto::Running, RunStatusDto::Failed)
            | (RunStatusDto::Running, RunStatusDto::Interrupted)
            | (RunStatusDto::WaitingInput, RunStatusDto::Running)
            | (RunStatusDto::WaitingInput, RunStatusDto::Failed)
            | (RunStatusDto::WaitingInput, RunStatusDto::Interrupted)
    );
    if allowed {
        Ok(())
    } else {
        Err(ErrorDto::new(
            "invalid_run_status_transition",
            intention_types::ErrorCategoryDto::Conflict,
            "run status transition is not permitted",
            intention_types::ErrorRetryDto::Never,
            None,
        )?)
    }
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
    /// Parses an absolute, non-empty native workspace path without resolving it.
    ///
    /// Resolution and root validation belong to `intention-workspace`, where
    /// the root is an addressing anchor rather than a containment boundary
    /// (architecture 05).
    ///
    /// # Errors
    ///
    /// Returns a validation error if `value` is empty or not absolute.
    pub fn parse(value: impl Into<String>) -> DtoResult<Self> {
        let value = value.into();
        if value.trim().is_empty() || !std::path::Path::new(&value).is_absolute() {
            Err(ErrorDto::validation(
                "invalid_workspace_root",
                "workspace root must be a non-empty absolute native path",
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

/// A command requesting a new durable session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateSessionCommandDto {
    project_id: ProjectId,
    session_id: SessionId,
    workspace_id: WorkspaceId,
    workspace_root: WorkspaceRootDto,
    mode: RunModeDto,
}

impl CreateSessionCommandDto {
    /// Creates a typed durable session request with daemon-owned stable identities.
    #[must_use]
    pub const fn new(
        project_id: ProjectId,
        session_id: SessionId,
        workspace_id: WorkspaceId,
        workspace_root: WorkspaceRootDto,
        mode: RunModeDto,
    ) -> Self {
        Self {
            project_id,
            session_id,
            workspace_id,
            workspace_root,
            mode,
        }
    }

    /// Returns the session's owning project identity.
    #[must_use]
    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    /// Returns the requested durable session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the daemon-owned stable workspace identity.
    #[must_use]
    pub const fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
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
}

/// A command requesting removal of one not-yet-seen pending turn.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoveTurnCommandDto {
    session_id: SessionId,
    turn_id: TurnId,
}

impl RemoveTurnCommandDto {
    /// Creates a typed pending-turn removal request.
    #[must_use]
    pub const fn new(session_id: SessionId, turn_id: TurnId) -> Self {
        Self {
            session_id,
            turn_id,
        }
    }

    /// Returns the owning durable session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }

    /// Returns the pending user turn identity.
    #[must_use]
    pub const fn turn_id(self) -> TurnId {
        self.turn_id
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

/// A safe current projection of one pending user turn.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PendingTurnProjectionDto {
    session_id: SessionId,
    turn_id: TurnId,
    content: String,
}

impl<'de> Deserialize<'de> for PendingTurnProjectionDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawPendingTurnProjectionDto {
            session_id: SessionId,
            turn_id: TurnId,
            content: String,
        }

        let raw = RawPendingTurnProjectionDto::deserialize(deserializer)?;
        Self::new(raw.session_id, raw.turn_id, raw.content).map_err(de::Error::custom)
    }
}

impl PendingTurnProjectionDto {
    /// Creates one pending turn projection with non-empty user content.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the pending content is blank.
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

    /// Returns the owning durable session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the pending user turn identity.
    #[must_use]
    pub const fn turn_id(&self) -> TurnId {
        self.turn_id
    }

    /// Returns the user-authored content that is not yet in the run context.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// A safe complete public session state projection at one durable event position.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionProjectionDto {
    project_id: ProjectId,
    session_id: SessionId,
    workspace_id: WorkspaceId,
    workspace_root: WorkspaceRootDto,
    mode: RunModeDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config_revision_id: Option<ConfigRevisionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_run: Option<RunProjectionDto>,
    pending_turns: Vec<PendingTurnProjectionDto>,
}

impl<'de> Deserialize<'de> for SessionProjectionDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawSessionProjectionDto {
            project_id: ProjectId,
            session_id: SessionId,
            workspace_id: WorkspaceId,
            workspace_root: WorkspaceRootDto,
            mode: RunModeDto,
            #[serde(default)]
            config_revision_id: Option<ConfigRevisionId>,
            #[serde(default)]
            active_run: Option<RunProjectionDto>,
            pending_turns: Vec<PendingTurnProjectionDto>,
        }

        let raw = RawSessionProjectionDto::deserialize(deserializer)?;
        Self::new(
            raw.project_id,
            raw.session_id,
            raw.workspace_id,
            raw.workspace_root,
            raw.mode,
            raw.config_revision_id,
            raw.active_run,
            raw.pending_turns,
        )
        .map_err(de::Error::custom)
    }
}

impl SessionProjectionDto {
    /// Creates a coherent safe public session projection.
    ///
    /// # Errors
    ///
    /// Returns a validation error when a nested run or pending turn belongs to a
    /// different session, or pending turn identities are not unique.
    #[expect(
        clippy::too_many_arguments,
        reason = "This public wire constructor preserves the established eight-field session projection contract."
    )]
    pub fn new(
        project_id: ProjectId,
        session_id: SessionId,
        workspace_id: WorkspaceId,
        workspace_root: WorkspaceRootDto,
        mode: RunModeDto,
        config_revision_id: Option<ConfigRevisionId>,
        active_run: Option<RunProjectionDto>,
        pending_turns: Vec<PendingTurnProjectionDto>,
    ) -> DtoResult<Self> {
        if active_run.is_some_and(|run| run.session_id() != session_id)
            || pending_turns
                .iter()
                .zip(pending_turns.iter().skip(1))
                .any(|(previous, next)| {
                    previous.session_id() != session_id
                        || next.session_id() != session_id
                        || previous.turn_id() == next.turn_id()
                })
            || pending_turns
                .first()
                .is_some_and(|turn| turn.session_id() != session_id)
        {
            return Err(ErrorDto::validation(
                "invalid_session_projection",
                "nested session state must belong to its session with unique pending turn identities",
            ));
        }
        Ok(Self {
            project_id,
            session_id,
            workspace_id,
            workspace_root,
            mode,
            config_revision_id,
            active_run,
            pending_turns,
        })
    }

    /// Returns the owning project identity.
    #[must_use]
    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }
    /// Returns the durable session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }
    /// Returns the daemon-owned stable workspace identity.
    #[must_use]
    pub const fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }
    /// Returns the declared workspace boundary.
    #[must_use]
    pub const fn workspace_root(&self) -> &WorkspaceRootDto {
        &self.workspace_root
    }
    /// Returns the session run policy mode.
    #[must_use]
    pub const fn mode(&self) -> RunModeDto {
        self.mode
    }
    /// Returns the latest accepted configuration revision, if one exists.
    #[must_use]
    pub const fn config_revision_id(&self) -> Option<ConfigRevisionId> {
        self.config_revision_id
    }
    /// Returns the sole active run, if one exists.
    #[must_use]
    pub const fn active_run(&self) -> Option<RunProjectionDto> {
        self.active_run
    }
    /// Returns pending turns in durable insertion order.
    #[must_use]
    pub fn pending_turns(&self) -> &[PendingTurnProjectionDto] {
        &self.pending_turns
    }
}

/// A command requesting that the daemon accept a new user turn.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SendUserTurnCommandDto {
    session_id: SessionId,
    idempotency_key: IdempotencyKey,
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
            idempotency_key: IdempotencyKey,
            content: String,
        }

        let raw = RawSendUserTurnCommandDto::deserialize(deserializer)?;
        Self::new(raw.session_id, raw.idempotency_key, raw.content).map_err(de::Error::custom)
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
        idempotency_key: IdempotencyKey,
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
                idempotency_key,
                content,
            })
        }
    }

    /// Returns the target session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the caller-supplied repeatable-operation identity.
    #[must_use]
    pub const fn idempotency_key(&self) -> IdempotencyKey {
        self.idempotency_key
    }

    /// Returns the requested user-authored content.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
}

/// A command requesting interruption of an active run's current operation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InterruptRunCommandDto {
    session_id: SessionId,
    run_id: RunId,
}

impl InterruptRunCommandDto {
    /// Creates a typed interruption request.
    #[must_use]
    pub const fn new(session_id: SessionId, run_id: RunId) -> Self {
        Self { session_id, run_id }
    }

    /// Returns the session that owns the target run.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }

    /// Returns the active run requested for interruption.
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

/// Validates a plan lifecycle transition.
///
/// # Errors
///
/// Returns a conflict error when the requested transition is not permitted.
pub fn validate_plan_status_transition(
    from: Option<PlanStatusDto>,
    to: PlanStatusDto,
) -> DtoResult<()> {
    let allowed = matches!(
        (from, to),
        (None, PlanStatusDto::Drafting)
            | (
                Some(PlanStatusDto::Drafting),
                PlanStatusDto::Revising | PlanStatusDto::Submitted | PlanStatusDto::Abandoned
            )
            | (
                Some(PlanStatusDto::Revising),
                PlanStatusDto::Revising | PlanStatusDto::Submitted | PlanStatusDto::Abandoned
            )
            | (
                Some(PlanStatusDto::Submitted),
                PlanStatusDto::Approved | PlanStatusDto::Rejected | PlanStatusDto::Abandoned
            )
            | (
                Some(PlanStatusDto::Rejected),
                PlanStatusDto::Revising | PlanStatusDto::Abandoned
            )
    );
    if allowed {
        Ok(())
    } else {
        Err(ErrorDto::new(
            "invalid_plan_status_transition",
            intention_types::ErrorCategoryDto::Conflict,
            "plan status transition is not permitted",
            intention_types::ErrorRetryDto::Never,
            None,
        )?)
    }
}

/// The terminal outcome recorded for one local tool result.
///
/// The taxonomy is deliberately closed to terminal outcomes: a call records
/// exactly one result row, and admission, rejection, and start evidence are
/// not persisted.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatusDto {
    /// The tool completed and produced a normalized safe result.
    Completed,
    /// The tool reported a safe failure outcome.
    Failed,
    /// The tool stopped because its run was cancelled.
    Cancelled,
    /// The tool stopped before a final outcome; its captured output is partial.
    Partial,
}

/// One credential-free structured metadata entry of a tool result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolResultMetadataEntryDto {
    key: String,
    value: String,
}

impl<'de> Deserialize<'de> for ToolResultMetadataEntryDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawToolResultMetadataEntryDto {
            key: String,
            value: String,
        }

        let raw = RawToolResultMetadataEntryDto::deserialize(deserializer)?;
        Self::new(raw.key, raw.value).map_err(de::Error::custom)
    }
}

impl ToolResultMetadataEntryDto {
    /// Creates one metadata entry with a non-blank key.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the key is blank or either field
    /// contains a NUL character.
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> DtoResult<Self> {
        let key = key.into();
        let value = value.into();
        if key.trim().is_empty() || key.contains('\0') || value.contains('\0') {
            return Err(ErrorDto::validation(
                "invalid_tool_result_metadata",
                "tool result metadata must have a non-blank key",
            ));
        }
        Ok(Self { key, value })
    }

    /// Returns the stable metadata key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns the metadata value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// The closed transcript message kind.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKindDto {
    /// User-authored content admitted as a run message.
    User,
    /// Assistant content committed once per completed model step.
    Assistant,
    /// One model-requested tool call with its canonical arguments document.
    ToolCall,
    /// The normalized safe result answering one tool call.
    ToolResult,
    /// Daemon-authored notice content bound to the run.
    Notice,
}

/// One committed transcript row, in durable insertion order.
///
/// The transcript is the canonical record of user, assistant, tool-call,
/// tool-result, and notice content; a row carries no event position, no
/// projection identity, and no provider resource.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MessageProjectionDto {
    session_id: SessionId,
    run_id: Option<RunId>,
    kind: MessageKindDto,
    text: String,
    reasoning: Option<String>,
    tool_call_id: Option<ToolCallId>,
    tool_id: Option<String>,
}

impl<'de> Deserialize<'de> for MessageProjectionDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawMessageProjectionDto {
            session_id: SessionId,
            #[serde(default)]
            run_id: Option<RunId>,
            kind: MessageKindDto,
            text: String,
            #[serde(default)]
            reasoning: Option<String>,
            #[serde(default)]
            tool_call_id: Option<ToolCallId>,
            #[serde(default)]
            tool_id: Option<String>,
        }

        let raw = RawMessageProjectionDto::deserialize(deserializer)?;
        Self::new(
            raw.session_id,
            raw.run_id,
            raw.kind,
            raw.text,
            raw.reasoning,
            raw.tool_call_id,
            raw.tool_id,
        )
        .map_err(de::Error::custom)
    }
}

impl MessageProjectionDto {
    /// Creates one coherent committed transcript row.
    ///
    /// A user, assistant, or notice row carries non-blank content and no tool
    /// identity; a tool-call row carries its call identity, its wire tool name,
    /// and the canonical arguments document; a tool-result row carries its call
    /// identity, its wire tool name, and non-blank content. Only assistant rows
    /// may carry reasoning text.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the row violates those rules or any
    /// carried value contains a NUL byte.
    pub fn new(
        session_id: SessionId,
        run_id: Option<RunId>,
        kind: MessageKindDto,
        text: impl Into<String>,
        reasoning: Option<String>,
        tool_call_id: Option<ToolCallId>,
        tool_id: Option<String>,
    ) -> DtoResult<Self> {
        let text = text.into();
        if text.contains('\0')
            || reasoning
                .as_deref()
                .is_some_and(|value| value.contains('\0'))
        {
            return Err(ErrorDto::validation(
                "invalid_message",
                "transcript content must be free of NUL bytes",
            ));
        }
        let tool_identity =
            tool_call_id.is_some() && tool_id.as_ref().is_some_and(|id| !id.trim().is_empty());
        let shape_valid = match kind {
            MessageKindDto::User | MessageKindDto::Notice => {
                !text.trim().is_empty()
                    && reasoning.is_none()
                    && tool_call_id.is_none()
                    && tool_id.is_none()
            }
            MessageKindDto::Assistant => {
                !text.trim().is_empty() && tool_call_id.is_none() && tool_id.is_none()
            }
            MessageKindDto::ToolCall => tool_identity && reasoning.is_none(),
            MessageKindDto::ToolResult => {
                tool_identity && reasoning.is_none() && !text.trim().is_empty()
            }
        };
        if !shape_valid {
            return Err(ErrorDto::validation(
                "invalid_message",
                "transcript rows must satisfy their closed kind shape",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            kind,
            text,
            reasoning,
            tool_call_id,
            tool_id,
        })
    }

    /// Returns the owning durable session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the owning run identity, when the row is bound to one run.
    #[must_use]
    pub const fn run_id(&self) -> Option<RunId> {
        self.run_id
    }

    /// Returns the closed transcript kind.
    #[must_use]
    pub const fn kind(&self) -> MessageKindDto {
        self.kind
    }

    /// Returns the committed content or canonical arguments document.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the whole reasoning text of one assistant step, when recorded.
    #[must_use]
    pub fn reasoning(&self) -> Option<&str> {
        self.reasoning.as_deref()
    }

    /// Returns the tool-call identity carried by a tool row.
    #[must_use]
    pub const fn tool_call_id(&self) -> Option<ToolCallId> {
        self.tool_call_id
    }

    /// Returns the wire tool name carried by a tool row.
    #[must_use]
    pub fn tool_id(&self) -> Option<&str> {
        self.tool_id.as_deref()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;

    fn fixture_workspace_root() -> WorkspaceRootDto {
        WorkspaceRootDto::parse(
            std::env::temp_dir()
                .join("intention-domain-unit-workspace")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("native fixture workspace is absolute")
    }

    #[test]
    fn run_and_plan_statuses_round_trip_through_wire_values() {
        for mode in [RunModeDto::Plan, RunModeDto::Build] {
            let encoded = serde_json::to_string(&mode).expect("mode serialization succeeds");
            let decoded: RunModeDto =
                serde_json::from_str(&encoded).expect("mode parsing succeeds");
            assert_eq!(decoded, mode);
        }
        for status in [
            RunStatusDto::Starting,
            RunStatusDto::Running,
            RunStatusDto::WaitingInput,
            RunStatusDto::Completed,
            RunStatusDto::Failed,
            RunStatusDto::Interrupted,
        ] {
            let encoded = serde_json::to_string(&status).expect("status serialization succeeds");
            let decoded: RunStatusDto =
                serde_json::from_str(&encoded).expect("status parsing succeeds");
            assert_eq!(decoded, status);
        }
        for status in [PlanStatusDto::Drafting, PlanStatusDto::Approved] {
            let encoded = serde_json::to_string(&status).expect("status serialization succeeds");
            let decoded: PlanStatusDto =
                serde_json::from_str(&encoded).expect("status parsing succeeds");
            assert_eq!(decoded, status);
        }

        let session_id = SessionId::new();
        let run_id = RunId::new();
        let interrupt = InterruptRunCommandDto::new(session_id, run_id);
        assert_eq!(interrupt.session_id(), session_id);
        assert_eq!(interrupt.run_id(), run_id);
        let query = GetSessionSnapshotQueryDto::new(session_id);
        assert_eq!(query.session_id(), session_id);
    }

    #[test]
    fn session_projection_rejects_a_pending_turn_from_another_session() {
        let session_id = SessionId::new();
        let projection = SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            fixture_workspace_root(),
            RunModeDto::Build,
            None,
            None,
            vec![
                PendingTurnProjectionDto::new(SessionId::new(), TurnId::new(), "hello")
                    .expect("pending turn is non-empty"),
            ],
        );
        assert!(projection.is_err());

        let projection = SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            fixture_workspace_root(),
            RunModeDto::Build,
            None,
            Some(RunProjectionDto::new(
                session_id,
                RunId::new(),
                TurnId::new(),
                RunStatusDto::Running,
                ConfigRevisionId::new(),
            )),
            Vec::new(),
        )
        .expect("coherent session projection is valid");
        assert_eq!(projection.session_id(), session_id);
        assert_eq!(
            projection.active_run().map(RunProjectionDto::status),
            Some(RunStatusDto::Running)
        );
    }

    #[test]
    fn run_status_transitions_accept_only_declared_edges() {
        let statuses = [
            RunStatusDto::Starting,
            RunStatusDto::Running,
            RunStatusDto::WaitingInput,
            RunStatusDto::Completed,
            RunStatusDto::Failed,
            RunStatusDto::Interrupted,
        ];
        for from in statuses {
            for to in statuses {
                let allowed = matches!(
                    (from, to),
                    (RunStatusDto::Starting, RunStatusDto::Running)
                        | (RunStatusDto::Starting, RunStatusDto::WaitingInput)
                        | (RunStatusDto::Starting, RunStatusDto::Failed)
                        | (RunStatusDto::Starting, RunStatusDto::Interrupted)
                        | (RunStatusDto::Running, RunStatusDto::WaitingInput)
                        | (RunStatusDto::Running, RunStatusDto::Completed)
                        | (RunStatusDto::Running, RunStatusDto::Failed)
                        | (RunStatusDto::Running, RunStatusDto::Interrupted)
                        | (RunStatusDto::WaitingInput, RunStatusDto::Running)
                        | (RunStatusDto::WaitingInput, RunStatusDto::Failed)
                        | (RunStatusDto::WaitingInput, RunStatusDto::Interrupted)
                );
                assert_eq!(
                    validate_run_status_transition(from, to).is_ok(),
                    allowed,
                    "unexpected run transition: {from:?} -> {to:?}"
                );
            }
        }
    }

    #[test]
    fn transcript_rows_validate_their_closed_shapes() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let call_id = ToolCallId::new();
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::User,
                "hello",
                None,
                None,
                None,
            )
            .is_ok()
        );
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::Assistant,
                "answer",
                Some("reasoning".to_owned()),
                None,
                None,
            )
            .is_ok()
        );
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::ToolCall,
                "{\"path\":\"src/lib.rs\"}",
                None,
                Some(call_id),
                Some("read".to_owned()),
            )
            .is_ok()
        );
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::ToolResult,
                "contents",
                Some("reasoning".to_owned()),
                Some(call_id),
                Some("read".to_owned()),
            )
            .is_err()
        );
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::ToolResult,
                " ",
                None,
                Some(call_id),
                Some("read".to_owned()),
            )
            .is_err()
        );
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::Notice,
                "notice",
                None,
                Some(call_id),
                Some("read".to_owned()),
            )
            .is_err()
        );
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::ToolCall,
                "{}",
                None,
                None,
                Some("read".to_owned()),
            )
            .is_err()
        );
        let decoded: MessageProjectionDto = serde_json::from_str(
            &serde_json::to_string(
                &MessageProjectionDto::new(
                    session_id,
                    None,
                    MessageKindDto::Notice,
                    "a notice",
                    None,
                    None,
                    None,
                )
                .expect("notice row is valid"),
            )
            .expect("notice row serializes"),
        )
        .expect("notice row deserializes");
        assert_eq!(decoded.kind(), MessageKindDto::Notice);
        assert_eq!(decoded.run_id(), None);
    }
}
