//! Deterministic M3 run lifecycle decisions over DTO-only storage.
//!
//! This crate has no provider, tool, timer, worker-loop, or scheduling
//! dependency. It decides durable transitions and delegates atomic commits to
//! the semantic storage repository.

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    ModelRunFactInputDto, RunEventCursorDto, RunFailureDto, RunProjectionDto, RunStatusDto,
    validate_run_status_transition,
};
use intention_model::{
    ModelCancellationSignal, ModelEventDto, ModelExecutionDriver, ModelRequestDto,
    ModelStreamLifecycleDto,
};
use intention_storage::{
    AppendModelRunFactsInputDto, CommittedChangeDto, RecoverUnfinishedRunsInputDto,
    StorageRepositoryDto, TransitionRunInputDto,
};
use intention_types::{
    AssistantTurnId, DtoResult, ErrorDto, ErrorRetryDto, RunId, SessionId, TimestampDto,
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

/// DTO-only executor over injected storage, selected driver, and time port.
pub struct ModelRunExecutionService<'a, Repository, Driver, Time> {
    repository: &'a Repository,
    driver: &'a Driver,
    time: &'a Time,
}

impl<'a, Repository, Driver, Time> ModelRunExecutionService<'a, Repository, Driver, Time>
where
    Repository: StorageRepositoryDto,
    Driver: ModelExecutionDriver,
    Time: ModelTimePort,
{
    /// Creates an executor without selecting providers or owning an async runtime.
    #[must_use]
    pub const fn new(repository: &'a Repository, driver: &'a Driver, time: &'a Time) -> Self {
        Self {
            repository,
            driver,
            time,
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

        let policy = persisted.resolved().provider_execution();
        let assistant_turn_id = AssistantTurnId::new();
        let mut pending_text = String::new();
        let mut durable_output = false;
        for attempt in 1..=u16::from(policy.max_attempts()) {
            cursor = self.append(
                input.session_id,
                input.run_id,
                cursor,
                vec![ModelRunFactInputDto::provider_attempt_started(attempt)?],
                (attempt == 1).then_some(RunStatusDto::Running),
            )?;
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
        use futures_util::{FutureExt, StreamExt, future::Either};

        let mut lifecycle = ModelStreamLifecycleDto::new();
        let mut stream = self
            .driver
            .execute(input.request.clone(), input.cancellation.clone());
        let timeout = self
            .time
            .sleep(std::time::Duration::from_secs(u64::from(timeout_seconds)))
            .fuse();
        futures_util::pin_mut!(timeout);
        loop {
            if input.cancellation.is_cancelled() {
                drop(stream);
                return self.cancel_attempt(input.session_id, input.run_id, cursor);
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
                    return self.cancel_attempt(input.session_id, input.run_id, cursor);
                }
                Either::Right((Either::Left((item, _)), _)) => item,
                Either::Right((Either::Right(((), _)), _)) => {
                    drop(stream);
                    return Ok(AttemptResult::Failed {
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
                    return Ok(AttemptResult::Failed {
                        cursor,
                        retryable: error.retry() == ErrorRetryDto::Delayed,
                        failure: RunFailureDto::from_provider(error),
                    });
                }
                None => {
                    return Ok(AttemptResult::Failed {
                        cursor,
                        failure: RunFailureDto::new(
                            "provider_stream_ended",
                            ErrorRetryDto::Never,
                            None,
                        )?,
                        retryable: false,
                    });
                }
            };
            if let Err(error) = lifecycle.accept(&event) {
                return Ok(AttemptResult::Failed {
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
                ModelEventDto::ReasoningDelta { content } => {
                    cursor = self.append(
                        input.session_id,
                        input.run_id,
                        cursor,
                        vec![ModelRunFactInputDto::reasoning_delta_recorded(content)?],
                        None,
                    )?;
                    *durable_output = true;
                }
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
                    cursor = self.append(
                        input.session_id,
                        input.run_id,
                        cursor,
                        vec![
                            ModelRunFactInputDto::tool_call_recorded(call),
                            ModelRunFactInputDto::failed(RunFailureDto::new(
                                "tool_execution_unavailable",
                                ErrorRetryDto::Never,
                                None,
                            )?),
                        ],
                        Some(RunStatusDto::Failed),
                    )?;
                    drop(stream);
                    return Ok(AttemptResult::FailedTerminal { cursor });
                }
                ModelEventDto::Finished { reason } => {
                    cursor = self.flush_text(
                        input.session_id,
                        input.run_id,
                        cursor,
                        assistant_turn_id,
                        pending_text,
                    )?;
                    cursor = self.append(
                        input.session_id,
                        input.run_id,
                        cursor,
                        vec![ModelRunFactInputDto::finished(reason)],
                        Some(RunStatusDto::Completing),
                    )?;
                    self.repository.transition_run(TransitionRunInputDto::new(
                        input.session_id,
                        input.run_id,
                        RunStatusDto::Completed,
                        self.time.now(),
                    ))?;
                    return Ok(AttemptResult::Completed { cursor });
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
        Ok(self
            .repository
            .append_model_run_facts(AppendModelRunFactsInputDto::new(
                session_id,
                run_id,
                cursor,
                facts,
                status,
                self.time.now(),
            )?)?
            .cursor())
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
        }
        match self.repository.transition_run(TransitionRunInputDto::new(
            session_id,
            run_id,
            RunStatusDto::Cancelled,
            self.time.now(),
        )) {
            Ok(_) => Ok(ModelRunExecutionOutcomeDto::Cancelled { cursor }),
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
