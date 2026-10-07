//! Session, pending-turn, and transcript projections shared across boundaries.

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    ConfigRevisionId, DtoResult, ErrorDto, ProjectId, RunId, SessionId, ToolCallId, TurnId,
    WorkspaceId,
};
use crate::{RunModeDto, RunProjectionDto, WorkspaceRootDto};

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
