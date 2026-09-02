//! Deterministic M3 run lifecycle decisions over DTO-only storage.
//!
//! This crate has no provider, tool, timer, worker-loop, or scheduling
//! dependency. It decides durable transitions and delegates atomic commits to
//! the semantic storage repository.

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    ModelRunFactInputDto, RunEventCursorDto, RunFailureDto, RunProjectionDto, RunStatusDto,
    ToolResultOutcomeDto, validate_run_status_transition,
};
pub use intention_model::{
    ModelCancellationSignal, ModelEventDto, ModelExecutionDriver, ModelMessageDto, ModelRequestDto,
    ModelRoleDto, ModelStreamLifecycleDto,
};
use intention_storage::{
    AppendModelRunFactsInputDto, AppendModelRunFactsOutcomeDto, CommittedChangeDto,
    RecoverUnfinishedRunsInputDto, StorageRepositoryDto, TransitionRunInputDto,
};
use intention_types::{
    AssistantTurnId, DtoResult, ErrorDto, ErrorRetryDto, RunId, SessionId, TimestampDto,
    ToolCallDto,
};

/// Explicit values for deterministic runtime lifecycle decisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeValuesDto {
    next_run_id: RunId,
    config_snapshot: ConfigSnapshotDto,
    occurred_at: TimestampDto,
}

impl RuntimeValuesDto {
    /// Creates deterministic DTO-only lifecycle values.
    #[must_use]
    pub const fn new(
        next_run_id: RunId,
        config_snapshot: ConfigSnapshotDto,
        occurred_at: TimestampDto,
    ) -> Self {
        Self {
            next_run_id,
            config_snapshot,
            occurred_at,
        }
    }

    /// Returns the supplied identity for the next promoted run.
    ///
    /// Queue promotion does not use this identity: queued turns retain their
    /// already-persisted proposed run identity.
    #[must_use]
    pub const fn next_run_id(&self) -> RunId {
        self.next_run_id
    }

    /// Returns the immutable snapshot to attach to a newly promoted run.
    #[must_use]
    pub const fn config_snapshot(&self) -> &ConfigSnapshotDto {
        &self.config_snapshot
    }

    /// Returns the explicit timestamp for lifecycle commits and recovery.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Deterministic lifecycle service over a DTO-only storage repository.
pub struct RuntimeService<'a, Repository> {
    repository: &'a Repository,
    values: RuntimeValuesDto,
}

impl<'a, Repository> RuntimeService<'a, Repository>
where
    Repository: StorageRepositoryDto,
{
    /// Creates a runtime lifecycle service with caller-supplied deterministic values.
    #[must_use]
    pub const fn new(repository: &'a Repository, values: RuntimeValuesDto) -> Self {
        Self { repository, values }
    }

    /// Returns whether a durable state graph edge is declared by the domain.
    #[must_use]
    pub fn can_transition(from: RunStatusDto, to: RunStatusDto) -> bool {
        validate_run_status_transition(from, to).is_ok()
    }

    /// Commits cancellation for an active run.
    ///
    /// A starting run follows `Starting -> Cancelling`; final cancellation and
    /// any queue promotion are committed later through [`Self::complete_terminal`].
    ///
    /// # Errors
    ///
    /// Returns a typed domain transition or repository error.
    pub fn stop_run(&self, session_id: SessionId, run_id: RunId) -> DtoResult<CommittedChangeDto> {
        let active = self.active_run(session_id, run_id)?;
        self.transition(
            session_id,
            run_id,
            active.status(),
            RunStatusDto::Cancelling,
        )
    }

    /// Commits a terminal transition and atomically promotes the next queued turn.
    ///
    /// # Errors
    ///
    /// Returns typed domain transition or repository errors, including invalid
    /// terminal status requests.
    pub fn complete_terminal(
        &self,
        session_id: SessionId,
        run_id: RunId,
        terminal_status: RunStatusDto,
    ) -> DtoResult<CommittedChangeDto> {
        if !terminal_status.is_terminal() {
            return Err(intention_types::ErrorDto::validation(
                "invalid_terminal_run_status",
                "runtime completion requires a terminal run status",
            ));
        }
        let projection = self.repository.load_session_snapshot(session_id)?;
        let active = self.active_run_from_projection(&projection, run_id)?;
        self.transition(session_id, run_id, active.status(), terminal_status)
    }

    /// Marks all unfinished durable runs interrupted before an owning facade is ready.
    ///
    /// External execution is deliberately never resumed by recovery.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when durable recovery cannot complete.
    pub fn recover_before_ready(&self) -> DtoResult<Vec<CommittedChangeDto>> {
        self.repository
            .recover_unfinished_runs(RecoverUnfinishedRunsInputDto::new(self.values.occurred_at))
    }

    fn active_run(&self, session_id: SessionId, run_id: RunId) -> DtoResult<RunProjectionDto> {
        let projection = self.repository.load_session_snapshot(session_id)?;
        self.active_run_from_projection(&projection, run_id)
    }

    fn active_run_from_projection(
        &self,
        projection: &intention_domain::SessionProjectionDto,
        run_id: RunId,
    ) -> DtoResult<RunProjectionDto> {
        projection
            .active_run()
            .filter(|active| active.run_id() == run_id)
            .ok_or_else(|| {
                intention_types::ErrorDto::validation(
                    "active_run_not_found",
                    "the requested run is not active in the session",
                )
            })
    }

    fn transition(
        &self,
        session_id: SessionId,
        run_id: RunId,
        from: RunStatusDto,
        to: RunStatusDto,
    ) -> DtoResult<CommittedChangeDto> {
        validate_run_status_transition(from, to)?;
        self.repository.transition_run(TransitionRunInputDto::new(
            session_id,
            run_id,
            to,
            self.values.occurred_at,
        ))
    }
}

const MAX_ASSISTANT_CONTENT_BYTES: usize = 4 * 1024;
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// Appends one atomic manual-retry failure for exactly a current starting run.
///
/// This narrow helper is used by application scheduling when a committed run
/// cannot acquire context or enter the daemon-owned dispatch queue.
///
/// # Errors
///
/// Returns a typed error when the exact run is unavailable, no longer
/// `Starting`, or the atomic failure append cannot commit.
pub fn fail_starting_run<Repository>(
    repository: &Repository,
    session_id: SessionId,
    run_id: RunId,
    failure_code: impl Into<String>,
    occurred_at: TimestampDto,
) -> DtoResult<AppendModelRunFactsOutcomeDto>
where
    Repository: StorageRepositoryDto,
{
    let replay = repository.load_current_run_replay(session_id, run_id)?;
    if replay.snapshot().run_projection().status() != RunStatusDto::Starting {
        return Err(ErrorDto::validation(
            "invalid_starting_run_failure_state",
            "scheduling failure requires the exact run to remain starting",
        ));
    }
    let failure = RunFailureDto::new(failure_code, ErrorRetryDto::Manual, None)?;
    repository.append_model_run_facts(AppendModelRunFactsInputDto::new(
        session_id,
        run_id,
        replay.snapshot().cursor(),
        vec![ModelRunFactInputDto::failed(failure)],
        Some(RunStatusDto::Failed),
        occurred_at,
    )?)
}

/// Provider-neutral clock and delay boundary for model execution.
///
/// Implementations return a fresh sleep future for every call. Production
/// composition supplies the private Tokio-backed adapter; deterministic tests
/// supply a manual clock without exposing either implementation here.
pub trait ModelTimePort {
    /// Returns the safe current timestamp for a durable runtime decision.
    fn now(&self) -> TimestampDto;

    /// Returns a fresh future that completes after the supplied duration.
    fn sleep(&self, duration: std::time::Duration) -> ModelSleepFuture<'_>;
}

/// Provider-neutral delay future owned by a [`ModelTimePort`].
pub type ModelSleepFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>;

/// Optional deterministic seam immediately before an executor's first durable append.
///
/// Production composition supplies no gate. Daemon-host outcome fixtures use a
/// gate to make the narrow `Starting`/`Cancelling` admission race observable
/// without changing provider, storage, or transport behavior.
pub trait ModelRunFirstAppendGate: Send + Sync {
    /// Waits after the initial durable `Starting` replay and preflight, but
    /// before the first `Starting -> Running` fact append.
    fn wait_before_first_append(&self) -> ModelSleepFuture<'_>;
}

/// Executes one provider-normalized tool call for the model-tool loop.
///
/// Implementations run the call in an isolated, bounded scope and return only
/// credential-free outcome evidence. Production composition supplies an
/// executor bound to the daemon's reviewed tool registry; deterministic
/// fixtures supply scripted outcomes without exposing any tool implementation
/// here.
pub trait ToolExecutionPort: Send + Sync {
    /// Executes the call and returns the bounded, credential-free outcome.
    fn execute_tool(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call: ToolCallDto,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = DtoResult<ToolResultOutcomeDto>> + Send + '_>,
    >;
}

/// Immutable caller-selected input for one model execution.
#[derive(Clone)]
pub struct ModelRunExecutionInputDto {
    session_id: SessionId,
    run_id: RunId,
    request: ModelRequestDto,
    safe_config: ConfigSnapshotDto,
    cancellation: ModelCancellationSignal,
}

impl ModelRunExecutionInputDto {
    /// Creates complete execution input without credentials or provider choice.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        run_id: RunId,
        request: ModelRequestDto,
        safe_config: ConfigSnapshotDto,
        cancellation: ModelCancellationSignal,
    ) -> Self {
        Self {
            session_id,
            run_id,
            request,
            safe_config,
            cancellation,
        }
    }

    /// Returns the run's owning session.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the run being executed.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the selected provider-neutral request.
    #[must_use]
    pub const fn request(&self) -> &ModelRequestDto {
        &self.request
    }

    /// Returns the caller-selected durable configuration selection.
    #[must_use]
    pub const fn safe_config(&self) -> &ConfigSnapshotDto {
        &self.safe_config
    }
}

/// Safe terminal evidence from one model execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelRunExecutionOutcomeDto {
    /// The provider finished and the run reached completed state.
    Completed { cursor: RunEventCursorDto },
    /// The run safely reached failed state.
    Failed { cursor: RunEventCursorDto },
    /// Direct cancellation reached cancelled state.
    Cancelled { cursor: RunEventCursorDto },
}

/// Safe evidence that a model execution write or state transition committed.
///
/// The snapshot is limited to safe run identity, cursor, and status evidence.
/// Implementers independently reload the durable run scope before publication;
/// this observer receives neither a repository transaction nor provider/runtime
/// resources, so it cannot publish an uncommitted mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelRunCommitDto {
    session_id: SessionId,
    run_id: RunId,
    cursor: RunEventCursorDto,
    snapshot: intention_domain::RunSnapshotDto,
}

impl ModelRunCommitDto {
    /// Creates provider-neutral committed execution evidence.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        run_id: RunId,
        cursor: RunEventCursorDto,
        snapshot: intention_domain::RunSnapshotDto,
    ) -> Self {
        Self {
            session_id,
            run_id,
            cursor,
            snapshot,
        }
    }

    /// Returns the owning session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the committed run identity.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the latest durable run cursor known to the executor.
    #[must_use]
    pub const fn cursor(&self) -> RunEventCursorDto {
        self.cursor
    }

    /// Returns a committed safe run snapshot suitable for independent reread.
    #[must_use]
    pub const fn snapshot(&self) -> &intention_domain::RunSnapshotDto {
        &self.snapshot
    }
}

/// Receives only durable model-execution commit evidence after a successful write.
///
/// A daemon publisher uses this provider-neutral seam to independently reread
/// the run scope before delivering a live update.
pub trait ModelRunCommitObserver: Send + Sync {
    /// Observes a successful fact append or execution-driven state transition.
    fn observe_model_run_commit(&self, committed: ModelRunCommitDto);
}

/// DTO-only executor over injected storage, selected driver, time port, optional
/// observer and gate, and the mandatory tool executor.
pub struct ModelRunExecutionService<'a, Repository, Driver: ?Sized, Time> {
    repository: &'a Repository,
    driver: &'a Driver,
    time: &'a Time,
    observer: Option<&'a dyn ModelRunCommitObserver>,
    first_append_gate: Option<&'a dyn ModelRunFirstAppendGate>,
    tool_executor: &'a dyn ToolExecutionPort,
}

impl<'a, Repository, Driver, Time> ModelRunExecutionService<'a, Repository, Driver, Time>
where
    Repository: StorageRepositoryDto,
    Driver: ModelExecutionDriver + ?Sized,
    Time: ModelTimePort,
{
    /// Creates an executor with the mandatory tool executor.
    ///
    /// Provider-emitted tool calls always execute through the supplied
    /// `ToolExecutionPort`; a no-port fallback no longer exists.
    #[must_use]
    pub const fn new(
        repository: &'a Repository,
        driver: &'a Driver,
        time: &'a Time,
        tool_executor: &'a dyn ToolExecutionPort,
    ) -> Self {
        Self {
            repository,
            driver,
            time,
            observer: None,
            first_append_gate: None,
            tool_executor,
        }
    }

    /// Adds a post-commit observer without exposing storage or provider resources.
    #[must_use]
    pub const fn with_commit_observer(
        repository: &'a Repository,
        driver: &'a Driver,
        time: &'a Time,
        observer: &'a dyn ModelRunCommitObserver,
        tool_executor: &'a dyn ToolExecutionPort,
    ) -> Self {
        Self {
            repository,
            driver,
            time,
            observer: Some(observer),
            first_append_gate: None,
            tool_executor,
        }
    }

    /// Adds a deterministic gate before the first durable execution append.
    ///
    /// This is reserved for host-race outcome fixtures. Normal daemon
    /// execution must use [`Self::with_commit_observer`] or [`Self::new`].
    #[must_use]
    pub const fn with_commit_observer_and_first_append_gate(
        repository: &'a Repository,
        driver: &'a Driver,
        time: &'a Time,
        observer: &'a dyn ModelRunCommitObserver,
        first_append_gate: &'a dyn ModelRunFirstAppendGate,
        tool_executor: &'a dyn ToolExecutionPort,
    ) -> Self {
        Self {
            repository,
            driver,
            time,
            observer: Some(observer),
            first_append_gate: Some(first_append_gate),
            tool_executor,
        }
    }

    /// Performs one bounded model execution lifecycle.
    ///
    /// # Errors
    ///
    /// Returns typed storage or cursor-conflict errors without retrying writes.
    #[expect(
        clippy::future_not_send,
        reason = "The DTO-only execution service accepts deterministic non-Sync test repositories; daemon composition owns any Send runtime boundary."
    )]
    pub async fn execute(
        &self,
        input: ModelRunExecutionInputDto,
    ) -> DtoResult<ModelRunExecutionOutcomeDto> {
        let replay = self
            .repository
            .load_current_run_replay(input.session_id, input.run_id)?;
        let run = replay.snapshot().run_projection();
        let mut cursor = replay.snapshot().cursor();
        if run.status() == RunStatusDto::Cancelling || input.cancellation.is_cancelled() {
            return self.cancel(input.session_id, input.run_id, cursor, run.status());
        }
        if run.status() != RunStatusDto::Starting {
            return Err(ErrorDto::validation(
                "invalid_model_run_execution_state",
                "model execution requires a starting run",
            ));
        }
        if input.request.run_id() != input.run_id
            || input.request.model() != input.safe_config.resolved().provider().model()
        {
            cursor = self.configuration_failure(input.session_id, input.run_id, cursor)?;
            return Ok(ModelRunExecutionOutcomeDto::Failed { cursor });
        }
        let persisted = match self
            .repository
            .load_run_config_snapshot(input.session_id, input.run_id)
        {
            Ok(snapshot) => snapshot,
            Err(_) => {
                cursor = self.configuration_failure(input.session_id, input.run_id, cursor)?;
                return Ok(ModelRunExecutionOutcomeDto::Failed { cursor });
            }
        };
        if !same_execution_selection(&persisted, &input.safe_config) {
            cursor = self.configuration_failure(input.session_id, input.run_id, cursor)?;
            return Ok(ModelRunExecutionOutcomeDto::Failed { cursor });
        }
        if let Err(error) = self.driver.preflight(&input.request) {
            cursor = self.fail(
                input.session_id,
                input.run_id,
                cursor,
                failure_from_error(&error)?,
            )?;
            return Ok(ModelRunExecutionOutcomeDto::Failed { cursor });
        }
        if let Some(first_append_gate) = self.first_append_gate {
            first_append_gate.wait_before_first_append().await;
        }

        let policy = persisted.resolved().provider_execution();
        let assistant_turn_id = AssistantTurnId::new();
        let mut pending_text = String::new();
        let mut durable_output = false;
        for attempt in 1..=u16::from(policy.max_attempts()) {
            cursor = match self.append(
                input.session_id,
                input.run_id,
                cursor,
                vec![ModelRunFactInputDto::provider_attempt_started(attempt)?],
                (attempt == 1).then_some(RunStatusDto::Running),
            ) {
                Ok(cursor) => cursor,
                Err(error) => match self.cancel_after_append_race(&input)? {
                    Some(outcome) => return Ok(outcome),
                    None => return Err(error),
                },
            };
            let result = self
                .drive_attempt(
                    &input,
                    policy.attempt_timeout_seconds(),
                    AttemptState {
                        cursor,
                        assistant_turn_id,
                        pending_text: &mut pending_text,
                        durable_output: &mut durable_output,
                    },
                )
                .await?;
            match result {
                AttemptResult::Completed { cursor } => {
                    return Ok(ModelRunExecutionOutcomeDto::Completed { cursor });
                }
                AttemptResult::Cancelled { cursor } => {
                    return Ok(ModelRunExecutionOutcomeDto::Cancelled { cursor });
                }
                AttemptResult::FailedTerminal { cursor } => {
                    return Ok(ModelRunExecutionOutcomeDto::Failed { cursor });
                }
                AttemptResult::Failed {
                    cursor: failure_cursor,
                    failure,
                    retryable,
                } => {
                    let retry = retryable
                        && !durable_output
                        && pending_text.is_empty()
                        && attempt < u16::from(policy.max_attempts());
                    if retry {
                        cursor = self.append(
                            input.session_id,
                            input.run_id,
                            failure_cursor,
                            vec![
                                ModelRunFactInputDto::provider_attempt_failed(attempt, failure)?,
                                ModelRunFactInputDto::retry_scheduled(attempt, attempt + 1)?,
                            ],
                            None,
                        )?;
                        if let Some(outcome) = self.wait_for_retry(&input, cursor).await? {
                            return Ok(outcome);
                        }
                    } else {
                        let mut cursor = self.flush_text(
                            input.session_id,
                            input.run_id,
                            failure_cursor,
                            assistant_turn_id,
                            &mut pending_text,
                        )?;
                        cursor = self.append(
                            input.session_id,
                            input.run_id,
                            cursor,
                            vec![
                                ModelRunFactInputDto::provider_attempt_failed(
                                    attempt,
                                    failure.clone(),
                                )?,
                                ModelRunFactInputDto::failed(failure),
                            ],
                            Some(RunStatusDto::Failed),
                        )?;
                        return Ok(ModelRunExecutionOutcomeDto::Failed { cursor });
                    }
                }
            }
        }
        unreachable!("validated provider execution policy supplies at least one attempt")
    }

    #[expect(
        clippy::future_not_send,
        reason = "The DTO-only execution service accepts deterministic non-Sync test repositories; daemon composition owns any Send runtime boundary."
    )]
    async fn drive_attempt(
        &self,
        input: &ModelRunExecutionInputDto,
        timeout_seconds: u8,
        state: AttemptState<'_>,
    ) -> DtoResult<AttemptResult> {
        let AttemptState {
            mut cursor,
            assistant_turn_id,
            pending_text,
            durable_output,
        } = state;
        let mut request = input.request.clone();
        let mut messages: Vec<ModelMessageDto> = input.request.messages().to_vec();
        let mut tool_round = 0u8;
        loop {
            let outcome = self
                .drive_provider_round(
                    request.clone(),
                    input,
                    timeout_seconds,
                    assistant_turn_id,
                    pending_text,
                    durable_output,
                    cursor,
                )
                .await?;
            match outcome {
                RoundOutcome::Completed { cursor } => {
                    return Ok(AttemptResult::Completed { cursor });
                }
                RoundOutcome::Cancelled { cursor } => {
                    return self.cancel_attempt(input.session_id, input.run_id, cursor);
                }
                RoundOutcome::Failed {
                    cursor: failed_cursor,
                    failure,
                    retryable,
                } => {
                    if tool_round == 0 {
                        return Ok(AttemptResult::Failed {
                            cursor: failed_cursor,
                            failure,
                            retryable,
                        });
                    }
                    let facts = vec![ModelRunFactInputDto::failed(failure)];
                    let cursor = self.append(
                        input.session_id,
                        input.run_id,
                        failed_cursor,
                        facts,
                        Some(RunStatusDto::Failed),
                    )?;
                    return Ok(AttemptResult::FailedTerminal { cursor });
                }
                RoundOutcome::ToolCalls {
                    cursor: calls_cursor,
                    calls,
                } => {
                    cursor = calls_cursor;
                    tool_round += 1;
                    messages.push(ModelMessageDto::assistant_tool_calls(None, calls.clone())?);
                    for call in calls {
                        let facts = vec![ModelRunFactInputDto::tool_call_recorded(call.clone())];
                        cursor =
                            self.append(input.session_id, input.run_id, cursor, facts, None)?;
                        if input.cancellation.is_cancelled() {
                            return self.cancel_attempt(input.session_id, input.run_id, cursor);
                        }
                        let outcome = match self
                            .tool_executor
                            .execute_tool(input.session_id, input.run_id, call.clone())
                            .await
                        {
                            Ok(outcome) => outcome,
                            Err(error) => {
                                // A tool infrastructure error is a typed failed
                                // tool result: record it first, then terminalize.
                                let failure = failure_from_error(&error)?;
                                let outcome = ToolResultOutcomeDto::failed(failure.clone());
                                let fact = ModelRunFactInputDto::tool_result_recorded(
                                    call.call_id(),
                                    outcome,
                                )?;
                                cursor = self.append(
                                    input.session_id,
                                    input.run_id,
                                    cursor,
                                    vec![fact],
                                    None,
                                )?;
                                *durable_output = true;
                                let facts = vec![ModelRunFactInputDto::failed(failure)];
                                cursor = self.append(
                                    input.session_id,
                                    input.run_id,
                                    cursor,
                                    facts,
                                    Some(RunStatusDto::Failed),
                                )?;
                                return Ok(AttemptResult::FailedTerminal { cursor });
                            }
                        };
                        if input.cancellation.is_cancelled() {
                            return self.cancel_attempt(input.session_id, input.run_id, cursor);
                        }
                        let fact = ModelRunFactInputDto::tool_result_recorded(
                            call.call_id(),
                            outcome.clone(),
                        )?;
                        let facts = vec![fact];
                        cursor =
                            self.append(input.session_id, input.run_id, cursor, facts, None)?;
                        *durable_output = true;
                        match outcome {
                            ToolResultOutcomeDto::Succeeded { content } => {
                                let message =
                                    ModelMessageDto::tool_result(call.call_id(), content)?;
                                messages.push(message);
                            }
                            ToolResultOutcomeDto::Failed { failure } => {
                                let facts = vec![ModelRunFactInputDto::failed(failure)];
                                cursor = self.append(
                                    input.session_id,
                                    input.run_id,
                                    cursor,
                                    facts,
                                    Some(RunStatusDto::Failed),
                                )?;
                                return Ok(AttemptResult::FailedTerminal { cursor });
                            }
                        }
                    }
                    request = input.request.with_messages(messages.clone())?;
                }
            }
        }
    }

    /// Drives one provider round: a single stream with its own start event.
    ///
    /// Tool-call events are only collected here; durable recording and
    /// execution happen in [`Self::drive_attempt`] against the mandatory tool
    /// executor.
    #[expect(
        clippy::too_many_arguments,
        reason = "The round helper carries the attempt's mutable state explicitly so the caller owns the tool loop."
    )]
    #[expect(
        clippy::future_not_send,
        reason = "The DTO-only execution service accepts deterministic non-Sync test repositories; daemon composition owns any Send runtime boundary."
    )]
    async fn drive_provider_round(
        &self,
        request: ModelRequestDto,
        input: &ModelRunExecutionInputDto,
        timeout_seconds: u8,
        assistant_turn_id: AssistantTurnId,
        pending_text: &mut String,
        durable_output: &mut bool,
        mut cursor: RunEventCursorDto,
    ) -> DtoResult<RoundOutcome> {
        use futures_util::{FutureExt, StreamExt, future::Either};

        let mut lifecycle = ModelStreamLifecycleDto::new();
        let mut stream = self.driver.execute(request, input.cancellation.clone());
        let timeout = self
            .time
            .sleep(std::time::Duration::from_secs(u64::from(timeout_seconds)))
            .fuse();
        futures_util::pin_mut!(timeout);
        let mut calls: Vec<ToolCallDto> = Vec::new();
        loop {
            if input.cancellation.is_cancelled() {
                drop(stream);
                return Ok(RoundOutcome::Cancelled { cursor });
            }
            let next = stream.next().fuse();
            let cancelled = input.cancellation.cancelled().fuse();
            futures_util::pin_mut!(next, cancelled);
            let event_or_timeout = match futures_util::future::select(
                cancelled,
                futures_util::future::select(next, &mut timeout),
            )
            .await
            {
                Either::Left(((), _)) => {
                    drop(stream);
                    return Ok(RoundOutcome::Cancelled { cursor });
                }
                Either::Right((Either::Left((item, _)), _)) => item,
                Either::Right((Either::Right(((), _)), _)) => {
                    drop(stream);
                    return Ok(RoundOutcome::Failed {
                        cursor,
                        failure: RunFailureDto::new(
                            "provider_attempt_timed_out",
                            ErrorRetryDto::Delayed,
                            None,
                        )?,
                        retryable: true,
                    });
                }
            };
            let event = match event_or_timeout {
                Some(Ok(event)) => event,
                Some(Err(error)) => {
                    return Ok(RoundOutcome::Failed {
                        cursor,
                        retryable: error.retry() == ErrorRetryDto::Delayed,
                        failure: RunFailureDto::from_provider(error),
                    });
                }
                None => {
                    if calls.is_empty() {
                        return Ok(RoundOutcome::Failed {
                            cursor,
                            failure: RunFailureDto::new(
                                "provider_stream_ended",
                                ErrorRetryDto::Never,
                                None,
                            )?,
                            retryable: false,
                        });
                    }
                    return Ok(RoundOutcome::ToolCalls { cursor, calls });
                }
            };
            if let Err(error) = lifecycle.accept(&event) {
                return Ok(RoundOutcome::Failed {
                    cursor,
                    failure: failure_from_error(&error)?,
                    retryable: false,
                });
            }
            match event {
                ModelEventDto::Started => {}
                ModelEventDto::TextDelta { content } => {
                    pending_text.push_str(&content);
                    let next_cursor = self.flush_full_text(
                        input.session_id,
                        input.run_id,
                        cursor,
                        assistant_turn_id,
                        pending_text,
                    )?;
                    *durable_output |= next_cursor != cursor;
                    cursor = next_cursor;
                }
                ModelEventDto::ReasoningDelta { category, content } => {
                    let category = match category {
                        intention_model::ReasoningFragmentCategoryDto::Primary => {
                            intention_domain::ReasoningDeltaCategory::Primary
                        }
                        intention_model::ReasoningFragmentCategoryDto::Detail => {
                            intention_domain::ReasoningDeltaCategory::Detail
                        }
                    };
                    cursor = self.append(
                        input.session_id,
                        input.run_id,
                        cursor,
                        vec![ModelRunFactInputDto::reasoning_delta_recorded_categorized(
                            category, content,
                        )?],
                        None,
                    )?;
                    *durable_output = true;
                }
                ModelEventDto::ReasoningSummaryDelta { content } => {
                    cursor = self.append(
                        input.session_id,
                        input.run_id,
                        cursor,
                        vec![ModelRunFactInputDto::reasoning_summary_delta_recorded(
                            content,
                        )?],
                        None,
                    )?;
                    *durable_output = true;
                }
                // Slice 2: the per-fact 512 KiB reasoning bound is enforced by
                // the domain constructors above; the combined per-run 4 MiB
                // bound (`intention_domain::validate_reasoning_fact_output_bound`)
                // needs per-run reasoning accounting and lands with the
                // control-plane/session-selection zones (the controller
                // decides on the 4 MiB follow-up).
                ModelEventDto::Usage { usage } => {
                    cursor = self.append(
                        input.session_id,
                        input.run_id,
                        cursor,
                        vec![ModelRunFactInputDto::usage_recorded(usage)],
                        None,
                    )?;
                    *durable_output = true;
                }
                ModelEventDto::ToolCall { call } => {
                    cursor = self.flush_text(
                        input.session_id,
                        input.run_id,
                        cursor,
                        assistant_turn_id,
                        pending_text,
                    )?;
                    calls.push(call);
                }
                ModelEventDto::Finished { reason } => {
                    cursor = self.flush_text(
                        input.session_id,
                        input.run_id,
                        cursor,
                        assistant_turn_id,
                        pending_text,
                    )?;
                    if calls.is_empty() {
                        cursor = self.append(
                            input.session_id,
                            input.run_id,
                            cursor,
                            vec![ModelRunFactInputDto::finished(reason)],
                            Some(RunStatusDto::Completing),
                        )?;
                        self.transition_completed(input.session_id, input.run_id, cursor)?;
                        return Ok(RoundOutcome::Completed { cursor });
                    }
                    return Ok(RoundOutcome::ToolCalls { cursor, calls });
                }
            }
        }
    }

    #[expect(
        clippy::future_not_send,
        reason = "The DTO-only execution service accepts deterministic non-Sync test repositories; daemon composition owns any Send runtime boundary."
    )]
    async fn wait_for_retry(
        &self,
        input: &ModelRunExecutionInputDto,
        cursor: RunEventCursorDto,
    ) -> DtoResult<Option<ModelRunExecutionOutcomeDto>> {
        use futures_util::{FutureExt, future::Either};

        if input.cancellation.is_cancelled() {
            return self
                .cancel(
                    input.session_id,
                    input.run_id,
                    cursor,
                    RunStatusDto::Running,
                )
                .map(Some);
        }
        let delay = self.time.sleep(RETRY_DELAY).fuse();
        let cancelled = input.cancellation.cancelled().fuse();
        futures_util::pin_mut!(delay, cancelled);
        match futures_util::future::select(cancelled, delay).await {
            Either::Left(((), _)) => self
                .cancel(
                    input.session_id,
                    input.run_id,
                    cursor,
                    RunStatusDto::Running,
                )
                .map(Some),
            Either::Right(((), _)) => Ok(None),
        }
    }

    fn append(
        &self,
        session_id: SessionId,
        run_id: RunId,
        cursor: RunEventCursorDto,
        facts: Vec<ModelRunFactInputDto>,
        status: Option<RunStatusDto>,
    ) -> DtoResult<RunEventCursorDto> {
        let outcome = self
            .repository
            .append_model_run_facts(AppendModelRunFactsInputDto::new(
                session_id,
                run_id,
                cursor,
                facts,
                status,
                self.time.now(),
            )?)?;
        let cursor = outcome.cursor();
        self.observe_snapshot(session_id, run_id, cursor, outcome.snapshot().clone());
        Ok(cursor)
    }

    /// Resolves the narrow admission race where StopRun commits `Cancelling`
    /// after the executor's initial replay but before its first append.
    ///
    /// The failed append must not strand a durable cancelling run: the task
    /// re-reads its exact scope and remains the owner of the terminal
    /// cancellation transition. Unrelated write failures remain errors.
    fn cancel_after_append_race(
        &self,
        input: &ModelRunExecutionInputDto,
    ) -> DtoResult<Option<ModelRunExecutionOutcomeDto>> {
        let replay = self
            .repository
            .load_current_run_replay(input.session_id, input.run_id)?;
        let snapshot = replay.snapshot();
        let status = snapshot.run_projection().status();
        if status == RunStatusDto::Cancelling || input.cancellation.is_cancelled() {
            self.cancel(input.session_id, input.run_id, snapshot.cursor(), status)
                .map(Some)
        } else {
            Ok(None)
        }
    }

    fn transition_completed(
        &self,
        session_id: SessionId,
        run_id: RunId,
        cursor: RunEventCursorDto,
    ) -> DtoResult<()> {
        self.repository.transition_run(TransitionRunInputDto::new(
            session_id,
            run_id,
            RunStatusDto::Completed,
            self.time.now(),
        ))?;
        self.observe_current_replay(session_id, run_id, cursor)
    }

    fn observe_snapshot(
        &self,
        session_id: SessionId,
        run_id: RunId,
        cursor: RunEventCursorDto,
        snapshot: intention_domain::RunSnapshotDto,
    ) {
        if let Some(observer) = self.observer {
            observer.observe_model_run_commit(ModelRunCommitDto::new(
                session_id, run_id, cursor, snapshot,
            ));
        }
    }

    fn observe_current_replay(
        &self,
        session_id: SessionId,
        run_id: RunId,
        cursor: RunEventCursorDto,
    ) -> DtoResult<()> {
        let replay = self
            .repository
            .load_current_run_replay(session_id, run_id)?;
        self.observe_snapshot(session_id, run_id, cursor, replay.snapshot().clone());
        Ok(())
    }

    fn observe_transition(
        &self,
        session_id: SessionId,
        run_id: RunId,
        cursor: RunEventCursorDto,
    ) -> DtoResult<()> {
        self.observe_current_replay(session_id, run_id, cursor)
    }

    fn fail(
        &self,
        session_id: SessionId,
        run_id: RunId,
        cursor: RunEventCursorDto,
        failure: RunFailureDto,
    ) -> DtoResult<RunEventCursorDto> {
        self.append(
            session_id,
            run_id,
            cursor,
            vec![ModelRunFactInputDto::failed(failure)],
            Some(RunStatusDto::Failed),
        )
    }

    fn configuration_failure(
        &self,
        session_id: SessionId,
        run_id: RunId,
        cursor: RunEventCursorDto,
    ) -> DtoResult<RunEventCursorDto> {
        self.fail(
            session_id,
            run_id,
            cursor,
            RunFailureDto::new(
                "provider_configuration_unavailable",
                ErrorRetryDto::Never,
                None,
            )?,
        )
    }

    fn cancel_attempt(
        &self,
        session_id: SessionId,
        run_id: RunId,
        cursor: RunEventCursorDto,
    ) -> DtoResult<AttemptResult> {
        self.cancel(session_id, run_id, cursor, RunStatusDto::Running)
            .map(|outcome| match outcome {
                ModelRunExecutionOutcomeDto::Cancelled { cursor } => {
                    AttemptResult::Cancelled { cursor }
                }
                ModelRunExecutionOutcomeDto::Completed { .. }
                | ModelRunExecutionOutcomeDto::Failed { .. } => {
                    unreachable!("cancel only produces a cancelled outcome")
                }
            })
    }

    fn cancel(
        &self,
        session_id: SessionId,
        run_id: RunId,
        cursor: RunEventCursorDto,
        status: RunStatusDto,
    ) -> DtoResult<ModelRunExecutionOutcomeDto> {
        if status != RunStatusDto::Cancelling {
            self.repository.transition_run(TransitionRunInputDto::new(
                session_id,
                run_id,
                RunStatusDto::Cancelling,
                self.time.now(),
            ))?;
            self.observe_transition(session_id, run_id, cursor)?;
        }
        match self.repository.transition_run(TransitionRunInputDto::new(
            session_id,
            run_id,
            RunStatusDto::Cancelled,
            self.time.now(),
        )) {
            Ok(_) => {
                self.observe_transition(session_id, run_id, cursor)?;
                Ok(ModelRunExecutionOutcomeDto::Cancelled { cursor })
            }
            Err(_) => {
                let cursor = self.fail(
                    session_id,
                    run_id,
                    cursor,
                    RunFailureDto::new("provider_cancellation_failed", ErrorRetryDto::Never, None)?,
                )?;
                Ok(ModelRunExecutionOutcomeDto::Failed { cursor })
            }
        }
    }

    fn flush_full_text(
        &self,
        session_id: SessionId,
        run_id: RunId,
        mut cursor: RunEventCursorDto,
        assistant_turn_id: AssistantTurnId,
        pending: &mut String,
    ) -> DtoResult<RunEventCursorDto> {
        while pending.len() >= MAX_ASSISTANT_CONTENT_BYTES {
            let end = valid_boundary_at_or_before(pending, MAX_ASSISTANT_CONTENT_BYTES);
            let content = pending.drain(..end).collect::<String>();
            cursor = self.append(
                session_id,
                run_id,
                cursor,
                vec![ModelRunFactInputDto::assistant_content_appended(
                    assistant_turn_id,
                    content,
                )?],
                None,
            )?;
        }
        Ok(cursor)
    }

    fn flush_text(
        &self,
        session_id: SessionId,
        run_id: RunId,
        cursor: RunEventCursorDto,
        assistant_turn_id: AssistantTurnId,
        pending: &mut String,
    ) -> DtoResult<RunEventCursorDto> {
        let cursor =
            self.flush_full_text(session_id, run_id, cursor, assistant_turn_id, pending)?;
        if pending.is_empty() {
            return Ok(cursor);
        }
        let content = std::mem::take(pending);
        self.append(
            session_id,
            run_id,
            cursor,
            vec![ModelRunFactInputDto::assistant_content_appended(
                assistant_turn_id,
                content,
            )?],
            None,
        )
    }
}

struct AttemptState<'a> {
    cursor: RunEventCursorDto,
    assistant_turn_id: AssistantTurnId,
    pending_text: &'a mut String,
    durable_output: &'a mut bool,
}

enum AttemptResult {
    Completed {
        cursor: RunEventCursorDto,
    },
    Cancelled {
        cursor: RunEventCursorDto,
    },
    Failed {
        cursor: RunEventCursorDto,
        failure: RunFailureDto,
        retryable: bool,
    },
    FailedTerminal {
        cursor: RunEventCursorDto,
    },
}

/// The outcome of one provider round, carrying the round's ending cursor.
enum RoundOutcome {
    Completed {
        cursor: RunEventCursorDto,
    },
    Cancelled {
        cursor: RunEventCursorDto,
    },
    Failed {
        cursor: RunEventCursorDto,
        failure: RunFailureDto,
        retryable: bool,
    },
    ToolCalls {
        cursor: RunEventCursorDto,
        calls: Vec<ToolCallDto>,
    },
}

const fn valid_boundary_at_or_before(value: &str, maximum: usize) -> usize {
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn same_execution_selection(persisted: &ConfigSnapshotDto, current: &ConfigSnapshotDto) -> bool {
    let persisted_provider = persisted.resolved().provider();
    let current_provider = current.resolved().provider();
    let persisted_execution = persisted.resolved().provider_execution();
    let current_execution = current.resolved().provider_execution();
    persisted_provider.kind() == current_provider.kind()
        && persisted_provider.model() == current_provider.model()
        && persisted_provider.endpoint() == current_provider.endpoint()
        && persisted_execution.attempt_timeout_seconds()
            == current_execution.attempt_timeout_seconds()
        && persisted_execution.max_attempts() == current_execution.max_attempts()
}

fn failure_from_error(error: &ErrorDto) -> DtoResult<RunFailureDto> {
    RunFailureDto::new(error.code(), error.retry(), error.correlation_id())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Accessor fixtures use expect to provide precise test failure messages."
    )]

    use super::*;
    use intention_config::{
        ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
    };
    use intention_model::{ModelCancellationSignal, ModelMessageDto, ModelRequestDto};
    use intention_types::{ConfigRevisionId, SchemaVersionDto, TimestampDto};

    fn fixture_input() -> ModelRunExecutionInputDto {
        let session_id = SessionId::parse("11111111-1111-4111-8111-111111111111")
            .expect("fixture session id is valid");
        let run_id =
            RunId::parse("22222222-2222-4222-8222-222222222222").expect("fixture run id is valid");
        let request = ModelRequestDto::new(
            run_id,
            "fixture-model",
            vec![
                ModelMessageDto::new(intention_model::ModelRoleDto::User, "fixture turn")
                    .expect("fixture message is valid"),
            ],
            None,
            None,
        )
        .expect("fixture request is valid");
        let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture-model\"\ncredential = \"fixture-credential-not-real-12345\"\n"
                .to_owned(),
            ConfigSourceDto::Explicit(
                ConfigPathDto::parse(
                    std::env::temp_dir()
                        .join("intention-runtime-accessor-fixture.toml")
                        .to_string_lossy()
                        .into_owned(),
                )
                .expect("fixture path is absolute"),
            ),
        ))
        .expect("fixture configuration resolves");
        let safe_config = ConfigSnapshotDto::new(
            SchemaVersionDto::new(1, 0),
            ConfigRevisionId::parse("33333333-3333-4333-8333-333333333333")
                .expect("fixture revision is a canonical UUID"),
            TimestampDto::from_unix_seconds(1_700_000_000).expect("fixture timestamp is valid"),
            resolved,
        )
        .expect("fixture snapshot is valid");
        ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request,
            safe_config,
            ModelCancellationSignal::new(),
        )
    }

    #[test]
    fn execution_input_accessors_expose_all_fields() {
        let input = fixture_input();
        assert_eq!(
            input.session_id(),
            SessionId::parse("11111111-1111-4111-8111-111111111111")
                .expect("fixture session id is valid")
        );
        assert_eq!(
            input.run_id(),
            RunId::parse("22222222-2222-4222-8222-222222222222").expect("fixture run id is valid")
        );
        assert_eq!(input.request().model(), "fixture-model");
        assert_eq!(
            input.safe_config().resolved().provider().model(),
            "fixture-model"
        );
        assert!(
            !input
                .safe_config()
                .resolved()
                .provider()
                .credential_configured()
                || input.safe_config().resolved().provider().kind().as_str() == "openrouter",
            "the safe configuration is the credential-free resolved projection"
        );
    }

    #[test]
    fn runtime_values_accessors_expose_all_fields() {
        let values = RuntimeValuesDto::new(
            RunId::parse("44444444-4444-4444-8444-444444444444").expect("fixture run id is valid"),
            fixture_input().safe_config().clone(),
            TimestampDto::from_unix_seconds(1_700_000_001).expect("fixture timestamp is valid"),
        );
        assert_eq!(
            values.next_run_id(),
            RunId::parse("44444444-4444-4444-8444-444444444444").expect("fixture run id is valid")
        );
        assert_eq!(
            values.config_snapshot().resolved().provider().model(),
            "fixture-model"
        );
        assert_eq!(
            values.occurred_at(),
            TimestampDto::from_unix_seconds(1_700_000_001).expect("fixture timestamp is valid")
        );
    }
}
