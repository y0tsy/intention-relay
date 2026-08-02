//! Application command/query orchestration over DTO-only durable storage.
//!
//! This crate maps committed repository outcomes into protocol-ready DTOs. It
//! neither owns database resources nor reimplements repository idempotency.

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    CreateSessionCommandDto, GetSessionSnapshotQueryDto, RemoveQueuedTurnCommandDto,
    SendUserTurnCommandDto, StopRunCommandDto,
};
use intention_protocol::{
    CreateSessionAcceptedDto, ProtocolAcceptedResultDto, RemoveQueuedTurnAcceptedDto,
    SendUserTurnAcceptedDto, SendUserTurnOutcomeDto, SessionSnapshotDto, StopRunAcceptedDto,
};
use intention_runtime::{RuntimeService, RuntimeValuesDto};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, CreateSessionInputDto,
    RemoveQueuedTurnInputDto, StorageRepositoryDto,
};
use intention_types::{DtoResult, ErrorDto, RunId, SchemaVersionDto, TimestampDto};

/// Explicit durable values selected for a create-session workflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateSessionWorkflowInputDto {
    command: CreateSessionCommandDto,
    occurred_at: TimestampDto,
}

impl CreateSessionWorkflowInputDto {
    /// Creates a DTO-only session creation workflow input.
    #[must_use]
    pub const fn new(command: CreateSessionCommandDto, occurred_at: TimestampDto) -> Self {
        Self {
            command,
            occurred_at,
        }
    }

    /// Returns the requested durable session command.
    #[must_use]
    pub const fn command(&self) -> &CreateSessionCommandDto {
        &self.command
    }

    /// Returns the event timestamp selected by the caller.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Explicit durable values selected for one accepted user turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SendUserTurnWorkflowInputDto {
    proposed_run_id: RunId,
    config_snapshot: ConfigSnapshotDto,
    occurred_at: TimestampDto,
}

impl SendUserTurnWorkflowInputDto {
    /// Creates a DTO-only user-turn workflow input.
    #[must_use]
    pub const fn new(
        proposed_run_id: RunId,
        config_snapshot: ConfigSnapshotDto,
        occurred_at: TimestampDto,
    ) -> Self {
        Self {
            proposed_run_id,
            config_snapshot,
            occurred_at,
        }
    }

    /// Returns the supplied first-or-future run identity.
    #[must_use]
    pub const fn proposed_run_id(&self) -> RunId {
        self.proposed_run_id
    }

    /// Returns the immutable snapshot retained for a started or queued run.
    #[must_use]
    pub const fn config_snapshot(&self) -> &ConfigSnapshotDto {
        &self.config_snapshot
    }

    /// Returns the selected durable event timestamp.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// DTO-only application facade over one semantic storage repository.
pub struct ApplicationService<'a, Repository> {
    repository: &'a Repository,
}

impl<'a, Repository> ApplicationService<'a, Repository>
where
    Repository: StorageRepositoryDto,
{
    /// Creates an application facade around a DTO-only durable repository.
    #[must_use]
    pub const fn new(repository: &'a Repository) -> Self {
        Self { repository }
    }

    /// Creates a durable session and maps its committed evidence for protocol use.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when durable session creation fails.
    pub fn create_session(
        &self,
        input: CreateSessionWorkflowInputDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let change = self.repository.create_session(CreateSessionInputDto::new(
            input.command.clone(),
            input.occurred_at,
        ))?;
        Ok(ProtocolAcceptedResultDto::CreateSession(
            CreateSessionAcceptedDto::new(
                input.command.project_id(),
                input.command.workspace_id(),
                input.command.session_id(),
                change.position(),
            ),
        ))
    }

    /// Accepts a user turn and maps the repository's durable started/queued outcome.
    ///
    /// The repository is the sole idempotency authority. Repeating identical
    /// content returns its committed result; conflicting content remains a typed
    /// repository conflict.
    ///
    /// # Errors
    ///
    /// Returns typed validation, conflict, or availability errors from the
    /// storage contract, or an internal consistency error for a malformed
    /// accepted-turn commit.
    pub fn send_user_turn(
        &self,
        command: SendUserTurnCommandDto,
        input: SendUserTurnWorkflowInputDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let change = self
            .repository
            .accept_user_turn(AcceptUserTurnInputDto::new(
                command.session_id(),
                command.turn_id(),
                command.content(),
                input.proposed_run_id,
                input.config_snapshot,
                input.occurred_at,
            )?)?;
        let outcome = change.turn_outcome().ok_or_else(|| {
            ErrorDto::validation(
                "missing_accepted_turn_outcome",
                "durable turn acceptance did not include an outcome",
            )
        })?;
        let outcome = match outcome {
            AcceptedTurnOutcomeDto::Started(run) => SendUserTurnOutcomeDto::Started {
                run_id: run.run_id(),
                config_revision_id: run.config_revision_id(),
            },
            AcceptedTurnOutcomeDto::Queued(queue_position) => {
                SendUserTurnOutcomeDto::Queued { queue_position }
            }
        };
        Ok(ProtocolAcceptedResultDto::SendUserTurn(
            SendUserTurnAcceptedDto::new(
                command.session_id(),
                command.turn_id(),
                change.position(),
                outcome,
            ),
        ))
    }

    /// Removes one unstarted queued turn and maps its committed evidence.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when no queued turn can be removed.
    pub fn remove_queued_turn(
        &self,
        command: RemoveQueuedTurnCommandDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let change = self
            .repository
            .remove_queued_turn(RemoveQueuedTurnInputDto::new(command, occurred_at))?;
        Ok(ProtocolAcceptedResultDto::RemoveQueuedTurn(
            RemoveQueuedTurnAcceptedDto::new(
                command.session_id(),
                command.turn_id(),
                change.position(),
            ),
        ))
    }

    /// Stops a run through the deterministic runtime lifecycle service.
    ///
    /// # Errors
    ///
    /// Returns the typed lifecycle or storage error when cancellation cannot be
    /// committed.
    pub fn stop_run(
        &self,
        command: StopRunCommandDto,
        values: RuntimeValuesDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let change = RuntimeService::new(self.repository, values)
            .stop_run(command.session_id(), command.run_id())?;
        Ok(ProtocolAcceptedResultDto::StopRun(StopRunAcceptedDto::new(
            command.session_id(),
            command.run_id(),
            change.position(),
        )))
    }

    /// Loads the current durable session projection as a versioned protocol snapshot.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when the requested session is absent
    /// or cannot be loaded.
    pub fn get_session_snapshot(
        &self,
        query: GetSessionSnapshotQueryDto,
    ) -> DtoResult<SessionSnapshotDto> {
        let projection = self.repository.load_session_snapshot(query.session_id())?;
        SessionSnapshotDto::with_projection(
            SchemaVersionDto::new(1, 0),
            query.session_id(),
            projection.at_sequence(),
            projection,
        )
    }
}
