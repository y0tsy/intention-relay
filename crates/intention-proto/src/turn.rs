//! Session turn commands and queries shared across boundaries.

use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    DtoResult, ErrorDto, IdempotencyKey, ProjectId, ProviderProfileOverrideDto, RunId, SessionId,
    TurnId, WorkspaceId,
};
use crate::{RunModeDto, WorkspaceRootDto};

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

/// A command requesting that the daemon accept a new user turn.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SendUserTurnCommandDto {
    session_id: SessionId,
    idempotency_key: IdempotencyKey,
    content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provider_profile: Option<ProviderProfileOverrideDto>,
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
            #[serde(default)]
            provider_profile: Option<ProviderProfileOverrideDto>,
        }

        let raw = RawSendUserTurnCommandDto::deserialize(deserializer)?;
        Self::new(raw.session_id, raw.idempotency_key, raw.content)
            .map(|command| command.with_provider_profile(raw.provider_profile))
            .map_err(de::Error::custom)
    }
}

impl SendUserTurnCommandDto {
    /// Creates a command with a non-empty user message and no profile override.
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
                provider_profile: None,
            })
        }
    }

    /// Returns the same command carrying one optional profile override.
    ///
    /// A per-turn override changes only the run this command starts.
    #[must_use]
    pub fn with_provider_profile(
        mut self,
        provider_profile: Option<ProviderProfileOverrideDto>,
    ) -> Self {
        self.provider_profile = provider_profile;
        self
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

    /// Returns the optional explicit profile override for the started run.
    #[must_use]
    pub const fn provider_profile(&self) -> Option<&ProviderProfileOverrideDto> {
        self.provider_profile.as_ref()
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
