//! Application command/query orchestration over DTO-only durable storage.
//!
//! This crate maps committed repository outcomes into protocol-ready DTOs. It
//! neither owns database resources nor reimplements repository idempotency.

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    CreateSessionCommandDto, GetSessionSnapshotQueryDto, RemoveQueuedTurnCommandDto,
    RunEventCursorDto, RunEventTailPageDto, RunReplayDto, SendUserTurnCommandDto,
    StopRunCommandDto,
};
use intention_protocol::{
    CreateSessionAcceptedDto, ProtocolAcceptedResultDto, RemoveQueuedTurnAcceptedDto,
    SendUserTurnAcceptedDto, SendUserTurnOutcomeDto, SessionSnapshotDto, StopRunAcceptedDto,
};
use intention_runtime::{
    ModelMessageDto, ModelRequestDto, ModelRoleDto, RuntimeService, RuntimeValuesDto,
    fail_starting_run,
};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, CreateSessionInputDto, ModelContextRoleDto,
    RemoveQueuedTurnInputDto, StorageRepositoryDto,
};
use intention_types::{DtoResult, ErrorDto, RunId, SchemaVersionDto, SessionId, TimestampDto};

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

/// Synchronous DTO-only boundary that admits accepted work to daemon-owned scheduling.
///
/// Implementations must not invoke a provider. The daemon host owns all
/// asynchronous execution after this bounded post-commit admission.
pub trait ModelRunDispatchPort {
    /// Schedules one fully constructed model run.
    ///
    /// # Errors
    ///
    /// Returns a typed local scheduling error when the daemon cannot accept the work.
    fn dispatch_model_run(&self, input: ScheduleModelRunDto) -> DtoResult<()>;
}

/// Complete DTO-only scheduling payload constructed from durable starting-run context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleModelRunDto {
    session_id: SessionId,
    run_id: RunId,
    request: ModelRequestDto,
    safe_config: ConfigSnapshotDto,
}

impl ScheduleModelRunDto {
    /// Creates a coherent model-run scheduling payload.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the request identity or model does not
    /// agree with the durable selected configuration.
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        request: ModelRequestDto,
        safe_config: ConfigSnapshotDto,
    ) -> DtoResult<Self> {
        if request.run_id() != run_id
            || request.model() != safe_config.resolved().provider().model()
        {
            return Err(ErrorDto::validation(
                "invalid_model_run_schedule",
                "model scheduling request must match the durable starting run selection",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            request,
            safe_config,
        })
    }

    /// Returns the owning durable session.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the exact durable starting run.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the provider-neutral request built from durable context.
    #[must_use]
    pub const fn request(&self) -> &ModelRequestDto {
        &self.request
    }

    /// Returns the immutable credential-free run configuration selection.
    #[must_use]
    pub const fn safe_config(&self) -> &ConfigSnapshotDto {
        &self.safe_config
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
        let accepted = accepted_user_turn(&command, &change)?;
        Ok(accepted)
    }

    /// Accepts a user turn and schedules an exactly-started run only after its initial commit.
    ///
    /// Queued outcomes and idempotent retry evidence never load model context or
    /// dispatch. Any post-commit context or dispatch failure is durably recorded
    /// against the exact `Starting` run and this method still returns the original
    /// acceptance.
    ///
    /// # Errors
    ///
    /// Returns an admission or malformed durable-acceptance error. Post-commit
    /// scheduling failures deliberately preserve the committed acceptance result.
    pub fn send_user_turn_and_schedule<Dispatch>(
        &self,
        command: SendUserTurnCommandDto,
        input: SendUserTurnWorkflowInputDto,
        dispatch: &Dispatch,
    ) -> DtoResult<ProtocolAcceptedResultDto>
    where
        Dispatch: ModelRunDispatchPort,
    {
        let occurred_at = input.occurred_at();
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
        let accepted = accepted_user_turn(&command, &change)?;
        let ProtocolAcceptedResultDto::SendUserTurn(accepted_turn) = accepted else {
            unreachable!("accepted user turn always returns user-turn acceptance")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = accepted_turn.outcome() else {
            return Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn));
        };
        if !started_run_committed_in(&change, run_id) {
            return Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn));
        }
        let session_id = accepted_turn.session_id();
        let schedule = match self
            .repository
            .load_starting_run_model_context(session_id, run_id)
        {
            Ok(context) if context.session_id() == session_id && context.run_id() == run_id => {
                match schedule_from_context(context) {
                    Ok(schedule) => schedule,
                    Err(_) => {
                        preserve_accepted_after_scheduling_failure(
                            self.repository,
                            session_id,
                            run_id,
                            "model_context_unavailable",
                            occurred_at,
                        );
                        return Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn));
                    }
                }
            }
            Ok(_) | Err(_) => {
                preserve_accepted_after_scheduling_failure(
                    self.repository,
                    session_id,
                    run_id,
                    "model_context_unavailable",
                    occurred_at,
                );
                return Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn));
            }
        };
        if dispatch.dispatch_model_run(schedule).is_err() {
            preserve_accepted_after_scheduling_failure(
                self.repository,
                session_id,
                run_id,
                "model_scheduling_unavailable",
                occurred_at,
            );
        }
        Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn))
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

    /// Loads the current internal run-scoped durable replay.
    ///
    /// This application-facing read deliberately does not alter the M3 public
    /// protocol subscription surface.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when the requested scoped replay is
    /// absent, mismatched, or unavailable.
    pub fn load_current_run_replay(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunReplayDto> {
        self.repository.load_current_run_replay(session_id, run_id)
    }

    /// Loads one bounded internal run-scoped fact tail.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when the requested tail cannot be
    /// read for this exact session/run identity and cursor.
    pub fn load_run_tail(
        &self,
        session_id: SessionId,
        run_id: RunId,
        after_cursor: RunEventCursorDto,
    ) -> DtoResult<RunEventTailPageDto> {
        self.repository
            .load_run_tail(session_id, run_id, after_cursor)
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

fn accepted_user_turn(
    command: &SendUserTurnCommandDto,
    change: &intention_storage::CommittedChangeDto,
) -> DtoResult<ProtocolAcceptedResultDto> {
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

fn started_run_committed_in(change: &intention_storage::CommittedChangeDto, run_id: RunId) -> bool {
    change
        .events()
        .iter()
        .any(|event| event.run_id() == Some(run_id))
}

fn preserve_accepted_after_scheduling_failure<Repository>(
    repository: &Repository,
    session_id: SessionId,
    run_id: RunId,
    failure_code: &'static str,
    occurred_at: TimestampDto,
) where
    Repository: StorageRepositoryDto,
{
    if fail_starting_run(repository, session_id, run_id, failure_code, occurred_at).is_err() {
        // The durable acceptance is already the externally documented result;
        // a secondary failure write cannot replace it with a scheduling error.
    }
}

fn schedule_from_context(
    context: intention_storage::StartingRunModelContextDto,
) -> DtoResult<ScheduleModelRunDto> {
    let messages = context
        .messages()
        .iter()
        .map(|message| {
            ModelMessageDto::new(
                match message.role() {
                    ModelContextRoleDto::User => ModelRoleDto::User,
                    ModelContextRoleDto::Assistant => ModelRoleDto::Assistant,
                },
                message.content(),
            )
        })
        .collect::<DtoResult<Vec<_>>>()?;
    let request = ModelRequestDto::new(
        context.run_id(),
        context.safe_config().resolved().provider().model(),
        messages,
        None,
        None,
    )?;
    ScheduleModelRunDto::new(
        context.session_id(),
        context.run_id(),
        request,
        context.safe_config().clone(),
    )
}
