//! Deterministic model-run execution over DTO-only storage.
//!
//! This crate has no provider, tool, timer, worker-loop, or scheduling
//! dependency. It decides durable transitions and delegates atomic commits to
//! the semantic storage repository.

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    ModelRunFactInputDto, RunEventCursorDto, RunFailureDto, RunStatusDto, ToolResultOutcomeDto,
};
pub use intention_model::{
    AssistantReasoningDto, ModelCancellationSignal, ModelEventDto, ModelExecutionDriver,
    ModelMessageDto, ModelRequestDto, ModelRoleDto, ModelStreamLifecycleDto,
    ModelToolDefinitionDto,
};
use intention_storage::{
    AppendModelRunFactsInputDto, AppendModelRunFactsOutcomeDto, AppendPendingUserTurnsInputDto,
    StorageRepositoryDto, TransitionRunInputDto,
};
use intention_types::{
    AssistantTurnId, DtoResult, ErrorDto, ErrorRetryDto, FinishReasonDto, RunId, SessionId,
    TimestampDto, ToolCallDto,
};

mod context_window;

use context_window::ContextWindowState;

/// The durable context notice recorded when an interrupted call produced no
/// final result of its own.
pub const INTERRUPT_NOTICE: &str = "[The call was stopped before a final result.]";

/// The partial tool result recorded when an interrupt arrives before a
/// model-requested tool call starts.
pub const TOOL_INTERRUPT_NOTICE: &str = "[The tool call was stopped before a final result.]";

const MAX_ASSISTANT_CONTENT_BYTES: usize = 4 * 1024;
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// Maximum bytes of one round's accumulated reasoning echo.
///
/// This per-round bound matches the transient `AssistantReasoningDto`
/// representable bound and the durable per-reasoning-fact bound (512 KiB), so
/// an echo inside it is always attachable. The durable per-fact and per-run
/// bounds remain the append authority; a round that crosses this bound
/// terminalizes as a typed failed run instead of aborting `execute` with a
/// DTO validation error (ADR 0008).
const MAX_ROUND_REASONING_ECHO_BYTES: usize = 512 * 1024;

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
    let replay = repository.load_current_run_snapshot(session_id, run_id)?;
    if replay.run_projection().status() != RunStatusDto::Starting {
        return Err(ErrorDto::validation(
            "invalid_starting_run_failure_state",
            "scheduling failure requires the exact run to remain starting",
        ));
    }
    let failure = RunFailureDto::new(failure_code, ErrorRetryDto::Manual, None)?;
    repository.append_model_run_facts(AppendModelRunFactsInputDto::new(
        session_id,
        run_id,
        replay.cursor(),
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
/// observer, and the mandatory tool executor.
pub struct ModelRunExecutionService<'a, Repository, Driver: ?Sized, Time> {
    repository: &'a Repository,
    driver: &'a Driver,
    time: &'a Time,
    observer: Option<&'a dyn ModelRunCommitObserver>,
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
            .load_current_run_snapshot(input.session_id, input.run_id)?;
        let run = replay.run_projection();
        let mut cursor = replay.cursor();
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
        let context_window = persisted.resolved().context_window();
        let assistant_turn_id = AssistantTurnId::new();
        let mut pending_text = String::new();
        let mut durable_output = false;
        // Context additions that must survive a retryable attempt boundary:
        // joined pending user messages and interruption notices. The live
        // context of the run stays continuous across provider attempts.
        let mut extra_messages: Vec<ModelMessageDto> = Vec::new();
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
                    ContextWindowState::new(
                        context_window.window_tokens(),
                        context_window.capacity_tokens(),
                    ),
                    AttemptState {
                        cursor,
                        assistant_turn_id,
                        pending_text: &mut pending_text,
                        durable_output: &mut durable_output,
                    },
                    &mut extra_messages,
                )
                .await?;
            match result {
                AttemptResult::Completed { cursor } => {
                    return Ok(ModelRunExecutionOutcomeDto::Completed { cursor });
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
                        cursor = self
                            .wait_for_retry(&input, cursor, &mut extra_messages)
                            .await?;
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
        mut context_window: ContextWindowState,
        state: AttemptState<'_>,
        extra_messages: &mut Vec<ModelMessageDto>,
    ) -> DtoResult<AttemptResult> {
        let AttemptState {
            mut cursor,
            assistant_turn_id,
            pending_text,
            durable_output,
        } = state;
        let mut messages: Vec<ModelMessageDto> = input.request.messages().to_vec();
        messages.extend(extra_messages.iter().cloned());
        // The starting context is windowed once, before its first provider
        // request, exactly like every later tool-result round.
        context_window.apply(&mut messages)?;
        let mut request = input.request.with_messages(messages.clone())?;
        let mut reasoning_attachments: Vec<AssistantReasoningDto> = Vec::new();
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
                    &mut context_window,
                    cursor,
                )
                .await?;
            match outcome {
                RoundOutcome::Finished {
                    cursor: finished_cursor,
                    reason,
                } => {
                    cursor = finished_cursor;
                    // A pending message is the nearest-boundary continuation:
                    // it joins the live context in FIFO order and the run
                    // continues instead of completing. An interruption that
                    // raced the finish is answered with its notice first.
                    if input.cancellation.is_cancelled() {
                        cursor = self.record_interrupt_notice(input, cursor)?;
                        messages.push(interrupt_notice_message()?);
                        extra_messages.push(interrupt_notice_message()?);
                        context_window.apply(&mut messages)?;
                        request = continuation_request(input, &messages, &reasoning_attachments)?;
                        continue;
                    }
                    let (next_cursor, joined) = self.consume_pending_user_turns(
                        input,
                        cursor,
                        &mut messages,
                        extra_messages,
                    )?;
                    cursor = next_cursor;
                    if joined {
                        context_window.apply(&mut messages)?;
                        request = continuation_request(input, &messages, &reasoning_attachments)?;
                        continue;
                    }
                    cursor = self.append(
                        input.session_id,
                        input.run_id,
                        cursor,
                        vec![ModelRunFactInputDto::finished(reason)],
                        Some(RunStatusDto::Completing),
                    )?;
                    self.transition_completed(input.session_id, input.run_id, cursor)?;
                    return Ok(AttemptResult::Completed { cursor });
                }
                RoundOutcome::Interrupted {
                    cursor: interrupted_cursor,
                } => {
                    cursor = self.record_interrupt_notice(input, interrupted_cursor)?;
                    messages.push(interrupt_notice_message()?);
                    extra_messages.push(interrupt_notice_message()?);
                    context_window.apply(&mut messages)?;
                    request = continuation_request(input, &messages, &reasoning_attachments)?;
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
                    reasoning,
                } => {
                    cursor = calls_cursor;
                    tool_round += 1;
                    messages.push(ModelMessageDto::assistant_tool_calls(None, calls.clone())?);
                    // Attachments are per-round and ordered: each assistant
                    // tool-call message keeps the reasoning of its own round
                    // when later rounds rebuild the continuation request.
                    if let Some(reasoning) = reasoning {
                        reasoning_attachments.push(reasoning);
                    }
                    let mut interrupted_tool = false;
                    for call in calls {
                        let facts = vec![ModelRunFactInputDto::tool_call_recorded(call.clone())];
                        cursor =
                            self.append(input.session_id, input.run_id, cursor, facts, None)?;
                        // An interrupt that arrived before this call started
                        // never begins a new effect: the call is answered with
                        // the stopped-call notice as its partial result, so the
                        // assistant tool-call message stays fully answered.
                        let outcome = if input.cancellation.is_cancelled() {
                            ToolResultOutcomeDto::partial(TOOL_INTERRUPT_NOTICE)?
                        } else {
                            match self
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
                            }
                        };
                        if matches!(outcome, ToolResultOutcomeDto::Partial { .. }) {
                            interrupted_tool = true;
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
                            // A partial tool result answers its call like a
                            // completed one: the model owns the decision about
                            // what the captured output means, and the loop
                            // continues instead of terminalizing the run.
                            ToolResultOutcomeDto::Succeeded { content }
                            | ToolResultOutcomeDto::Partial { content } => {
                                let message =
                                    ModelMessageDto::tool_result(call.call_id(), content)?;
                                messages.push(message);
                                // Every added tool result re-runs the window
                                // pass, so the continuation request carries a
                                // trimmed context and recomputed breakpoints.
                                context_window.apply(&mut messages)?;
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
                    // A tool batch is the second bounded interruption
                    // boundary. A partial tool result already carries the
                    // stopped-call notice, so the run continues to the next
                    // model step; every other in-flight batch position gets
                    // the explicit context notice.
                    if input.cancellation.is_cancelled() {
                        if interrupted_tool {
                            input.cancellation.reset();
                        } else {
                            cursor = self.record_interrupt_notice(input, cursor)?;
                            messages.push(interrupt_notice_message()?);
                            extra_messages.push(interrupt_notice_message()?);
                        }
                    }
                    let (next_cursor, _joined) = self.consume_pending_user_turns(
                        input,
                        cursor,
                        &mut messages,
                        extra_messages,
                    )?;
                    cursor = next_cursor;
                    context_window.apply(&mut messages)?;
                    request = continuation_request(input, &messages, &reasoning_attachments)?;
                }
            }
        }
    }

    /// Appends one durable interruption notice and clears the run's signal.
    ///
    /// The notice tells the model that its current call was stopped before a
    /// final result; the run stays `Running` and continues with the next step.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or storage error when the notice cannot be
    /// appended durably.
    fn record_interrupt_notice(
        &self,
        input: &ModelRunExecutionInputDto,
        cursor: RunEventCursorDto,
    ) -> DtoResult<RunEventCursorDto> {
        let cursor = self.append(
            input.session_id,
            input.run_id,
            cursor,
            vec![ModelRunFactInputDto::interrupt_notice_recorded(
                INTERRUPT_NOTICE,
            )?],
            None,
        )?;
        input.cancellation.reset();
        Ok(cursor)
    }

    /// Appends every pending user message to the live run context.
    ///
    /// The storage transaction marks the turns as appended and assigns their
    /// durable fact cursors atomically, so a message can join exactly one run
    /// context. The returned cursor is the run cursor after the append and the
    /// flag reports whether any message joined.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the pending batch cannot commit.
    fn consume_pending_user_turns(
        &self,
        input: &ModelRunExecutionInputDto,
        cursor: RunEventCursorDto,
        messages: &mut Vec<ModelMessageDto>,
        extra_messages: &mut Vec<ModelMessageDto>,
    ) -> DtoResult<(RunEventCursorDto, bool)> {
        let outcome =
            self.repository
                .append_pending_user_turns(AppendPendingUserTurnsInputDto::new(
                    input.session_id,
                    input.run_id,
                    cursor,
                    self.time.now(),
                ))?;
        let mut joined = false;
        for fact in outcome.facts() {
            if let ModelRunFactInputDto::UserMessageAppended { content, .. } = fact.input() {
                let message = ModelMessageDto::new(ModelRoleDto::User, content)?;
                messages.push(message.clone());
                extra_messages.push(message);
                joined = true;
            }
        }
        Ok((outcome.cursor(), joined))
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
        context_window: &mut ContextWindowState,
        mut cursor: RunEventCursorDto,
    ) -> DtoResult<RoundOutcome> {
        use futures_util::{FutureExt, StreamExt, future::Either};

        let mut lifecycle = ModelStreamLifecycleDto::new();
        let request_characters = ContextWindowState::request_characters(&request);
        let mut stream = self.driver.execute(request, input.cancellation.clone());
        let timeout = self
            .time
            .sleep(std::time::Duration::from_secs(u64::from(timeout_seconds)))
            .fuse();
        futures_util::pin_mut!(timeout);
        let mut calls: Vec<ToolCallDto> = Vec::new();
        let mut reasoning_text = String::new();
        let mut reasoning_channel_seen = false;
        let mut reasoning_echo_exceeds_round_bound = false;
        loop {
            if input.cancellation.is_cancelled() {
                drop(stream);
                let cursor = self.flush_text(
                    input.session_id,
                    input.run_id,
                    cursor,
                    assistant_turn_id,
                    pending_text,
                )?;
                return Ok(RoundOutcome::Interrupted { cursor });
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
                    let cursor = self.flush_text(
                        input.session_id,
                        input.run_id,
                        cursor,
                        assistant_turn_id,
                        pending_text,
                    )?;
                    return Ok(RoundOutcome::Interrupted { cursor });
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
                    let reasoning = match round_reasoning_attachment(
                        reasoning_channel_seen,
                        reasoning_text,
                        &calls,
                        reasoning_echo_exceeds_round_bound,
                    ) {
                        Ok(reasoning) => reasoning,
                        Err(_) => return unrepresentable_reasoning_round(cursor),
                    };
                    return Ok(RoundOutcome::ToolCalls {
                        cursor,
                        calls,
                        reasoning,
                    });
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
                ModelEventDto::ReasoningDelta { content } => {
                    // The reasoning channel marks a presence even when it
                    // carries no text: the continuation request must send the
                    // channel back on the assistant tool-call message. Empty
                    // fragments never become durable facts because the fact
                    // constructors reject blank content.
                    reasoning_channel_seen = true;
                    if !content.is_empty() {
                        // The accumulated echo is bounded per round at the
                        // attachment's representable bound. Once it is crossed
                        // the round is unrepresentable and terminalizes as a
                        // typed failed run at round end; the echo is never
                        // truncated and the durable per-fact and per-run bounds
                        // stay with the append authority (ADR 0008).
                        if reasoning_echo_exceeds_round_bound
                            || reasoning_text.len() + content.len() > MAX_ROUND_REASONING_ECHO_BYTES
                        {
                            reasoning_echo_exceeds_round_bound = true;
                        } else {
                            reasoning_text.push_str(&content);
                        }
                        cursor = self.append(
                            input.session_id,
                            input.run_id,
                            cursor,
                            vec![ModelRunFactInputDto::reasoning_delta_recorded(content)?],
                            None,
                        )?;
                        *durable_output = true;
                    }
                }
                ModelEventDto::Usage { usage } => {
                    cursor = self.append(
                        input.session_id,
                        input.run_id,
                        cursor,
                        vec![ModelRunFactInputDto::usage_recorded(usage)],
                        None,
                    )?;
                    context_window.observe_usage(usage, request_characters);
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
                        return Ok(RoundOutcome::Finished { cursor, reason });
                    }
                    let reasoning = match round_reasoning_attachment(
                        reasoning_channel_seen,
                        reasoning_text,
                        &calls,
                        reasoning_echo_exceeds_round_bound,
                    ) {
                        Ok(reasoning) => reasoning,
                        Err(_) => return unrepresentable_reasoning_round(cursor),
                    };
                    return Ok(RoundOutcome::ToolCalls {
                        cursor,
                        calls,
                        reasoning,
                    });
                }
            }
        }
    }

    /// Waits out one scheduled retry delay, or handles an interruption.
    ///
    /// An interruption during the wait appends its durable notice, clears the
    /// signal, and returns so the next provider attempt starts immediately.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the notice cannot be appended.
    #[expect(
        clippy::future_not_send,
        reason = "The DTO-only execution service accepts deterministic non-Sync test repositories; daemon composition owns any Send runtime boundary."
    )]
    async fn wait_for_retry(
        &self,
        input: &ModelRunExecutionInputDto,
        cursor: RunEventCursorDto,
        extra_messages: &mut Vec<ModelMessageDto>,
    ) -> DtoResult<RunEventCursorDto> {
        use futures_util::{FutureExt, future::Either};

        if input.cancellation.is_cancelled() {
            let message = interrupt_notice_message()?;
            let cursor = self.record_interrupt_notice(input, cursor)?;
            extra_messages.push(message);
            return Ok(cursor);
        }
        let delay = self.time.sleep(RETRY_DELAY).fuse();
        let cancelled = input.cancellation.cancelled().fuse();
        futures_util::pin_mut!(delay, cancelled);
        match futures_util::future::select(cancelled, delay).await {
            Either::Left(((), _)) => {
                let message = interrupt_notice_message()?;
                let cursor = self.record_interrupt_notice(input, cursor)?;
                extra_messages.push(message);
                Ok(cursor)
            }
            Either::Right(((), _)) => Ok(cursor),
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
            .load_current_run_snapshot(session_id, run_id)?;
        self.observe_snapshot(session_id, run_id, cursor, replay);
        Ok(())
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
    /// The provider finished the round without tool calls. The caller owns
    /// the completion decision, because pending user messages continue the
    /// run instead of completing it.
    Finished {
        cursor: RunEventCursorDto,
        reason: FinishReasonDto,
    },
    /// The round was interrupted before a final result; the run continues.
    Interrupted { cursor: RunEventCursorDto },
    Failed {
        cursor: RunEventCursorDto,
        failure: RunFailureDto,
        retryable: bool,
    },
    ToolCalls {
        cursor: RunEventCursorDto,
        calls: Vec<ToolCallDto>,
        reasoning: Option<AssistantReasoningDto>,
    },
}

/// Builds one round's transient reasoning attachment for the tool-loop
/// continuation.
///
/// The attachment is `Some` whenever the round observed the provider's
/// reasoning channel, even when that channel carried no text: the continuation
/// request must send the channel back on the assistant tool-call message that
/// continues the same run. A round without the channel produces `None`.
///
/// # Errors
///
/// Returns a validation error when the round's accumulated echo cannot form a
/// valid attachment: it crossed the per-round attachment bound, or the
/// attachment DTO rejects its control characters. Callers record the dedicated
/// typed failed run instead of propagating the validation error.
fn round_reasoning_attachment(
    reasoning_channel_seen: bool,
    text: String,
    calls: &[ToolCallDto],
    echo_exceeds_round_bound: bool,
) -> DtoResult<Option<AssistantReasoningDto>> {
    if !reasoning_channel_seen {
        return Ok(None);
    }
    if echo_exceeds_round_bound {
        return Err(ErrorDto::validation(
            "invalid_round_reasoning_echo",
            "the round's reasoning echo exceeds the per-round attachment bound",
        ));
    }
    let tool_call_ids = calls.iter().map(ToolCallDto::call_id).collect();
    AssistantReasoningDto::new(tool_call_ids, text).map(Some)
}

/// Terminalizes a round whose accumulated reasoning echo cannot become the
/// continuation attachment as a durable typed failed run.
///
/// The echo crossed the per-round attachment bound or carries a control
/// character the attachment DTO rejects. The run fails with the dedicated
/// `reasoning_attachment_unrepresentable` code instead of aborting `execute`
/// with a DTO validation error, and the echo is never truncated or silently
/// omitted (ADR 0008).
///
/// # Errors
///
/// Returns a validation error only when the static failure code is rejected.
fn unrepresentable_reasoning_round(cursor: RunEventCursorDto) -> DtoResult<RoundOutcome> {
    Ok(RoundOutcome::Failed {
        cursor,
        failure: RunFailureDto::new(
            "reasoning_attachment_unrepresentable",
            ErrorRetryDto::Never,
            None,
        )?,
        retryable: false,
    })
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

/// Returns the durable context notice for one interrupted call.
///
/// # Errors
///
/// Returns a validation error only when the static notice is rejected.
fn interrupt_notice_message() -> DtoResult<ModelMessageDto> {
    ModelMessageDto::new(ModelRoleDto::Notice, INTERRUPT_NOTICE)
}

/// Rebuilds the continuation request from the current live run context.
///
/// # Errors
///
/// Returns a validation error when the rebuilt request violates its contract.
fn continuation_request(
    input: &ModelRunExecutionInputDto,
    messages: &[ModelMessageDto],
    reasoning: &[AssistantReasoningDto],
) -> DtoResult<ModelRequestDto> {
    input
        .request
        .with_messages(messages.to_vec())?
        .with_assistant_reasoning(reasoning.to_vec())
}
