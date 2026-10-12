//! Session turn commands and queries shared across boundaries.

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    DtoResult, ErrorDto, IdempotencyKey, ProjectId, RunId, SessionId, TurnId, WorkspaceId,
};
use crate::{RunModeDto, WorkspaceRootDto};

/// A command requesting a new durable session.
///
/// The project and workspace identities are the daemon-owned identities of the
/// declared workspace root: a root with no durable binding establishes one from
/// these values, while a bound root keeps its binding and treats them as
/// advisory, so any number of sessions can join one root.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateSessionCommandDto {
    project_id: ProjectId,
    session_id: SessionId,
    workspace_id: WorkspaceId,
    workspace_root: WorkspaceRootDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<RunModeDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_turn: Option<CreateSessionFirstTurnDto>,
}

impl CreateSessionCommandDto {
    /// Creates a typed durable session request with daemon-owned stable identities.
    ///
    /// `mode` is the run policy the caller declared, or `None` to leave the
    /// durable default to the core. `first_turn` is the user turn the command
    /// commits as the created session's first turn, or `None` to create an
    /// empty session.
    #[must_use]
    pub const fn new(
        project_id: ProjectId,
        session_id: SessionId,
        workspace_id: WorkspaceId,
        workspace_root: WorkspaceRootDto,
        mode: Option<RunModeDto>,
        first_turn: Option<CreateSessionFirstTurnDto>,
    ) -> Self {
        Self {
            project_id,
            session_id,
            workspace_id,
            workspace_root,
            mode,
            first_turn,
        }
    }

    /// Returns the proposed project identity of the declared workspace root.
    #[must_use]
    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    /// Returns the requested durable session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the proposed workspace identity of the declared workspace root.
    ///
    /// The value is advisory once the root carries a durable binding: the
    /// daemon resolves the command to the binding before creation.
    #[must_use]
    pub const fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    /// Returns the declared workspace boundary.
    #[must_use]
    pub const fn workspace_root(&self) -> &WorkspaceRootDto {
        &self.workspace_root
    }

    /// Returns the run policy the caller declared, if any.
    ///
    /// The core resolves an absent value to [`RunModeDto::DEFAULT`] before the
    /// durable session is written, so no front end decides the session policy.
    #[must_use]
    pub const fn mode(&self) -> Option<RunModeDto> {
        self.mode
    }

    /// Returns the first user turn the command carries, if any.
    ///
    /// The turn joins the session this command creates; it carries no session
    /// identity of its own.
    #[must_use]
    pub const fn first_turn(&self) -> Option<&CreateSessionFirstTurnDto> {
        self.first_turn.as_ref()
    }

    /// Returns the durable run policy of the session this command creates.
    ///
    /// An absent caller mode resolves to [`RunModeDto::DEFAULT`], the one core
    /// declaration of the durable default.
    #[must_use]
    pub const fn resolved_mode(&self) -> RunModeDto {
        match self.mode {
            Some(mode) => mode,
            None => RunModeDto::DEFAULT,
        }
    }

    /// Returns this command carrying the durable run policy the core resolved.
    #[must_use]
    pub const fn with_mode(mut self, mode: RunModeDto) -> Self {
        self.mode = Some(mode);
        self
    }
}

/// One user turn a session-creation command carries as the session's first turn.
///
/// The turn names no session of its own: the session the command creates is the
/// one it joins. Its content is validated the way every user turn's is, so a
/// blank prompt is never a turn.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CreateSessionFirstTurnDto {
    idempotency_key: IdempotencyKey,
    content: String,
}

impl<'de> Deserialize<'de> for CreateSessionFirstTurnDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawCreateSessionFirstTurnDto {
            idempotency_key: IdempotencyKey,
            content: String,
        }

        let raw = RawCreateSessionFirstTurnDto::deserialize(deserializer)?;
        Self::new(raw.idempotency_key, raw.content).map_err(de::Error::custom)
    }
}

impl CreateSessionFirstTurnDto {
    /// Creates a first turn with a non-empty user message.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the requested content is empty.
    pub fn new(idempotency_key: IdempotencyKey, content: impl Into<String>) -> DtoResult<Self> {
        let content = content.into();
        if content.trim().is_empty() {
            Err(ErrorDto::validation(
                "invalid_turn_content",
                "user turn content must not be empty",
            ))
        } else {
            Ok(Self {
                idempotency_key,
                content,
            })
        }
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

/// A command requesting that an active run stop for good.
///
/// The daemon signals the run's in-flight operation and the run ends
/// `interrupted` instead of continuing to the next model step.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CancelRunCommandDto {
    session_id: SessionId,
    run_id: RunId,
}

impl CancelRunCommandDto {
    /// Creates a typed stop request.
    #[must_use]
    pub const fn new(session_id: SessionId, run_id: RunId) -> Self {
        Self { session_id, run_id }
    }

    /// Returns the session that owns the target run.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }

    /// Returns the active run requested to stop.
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
