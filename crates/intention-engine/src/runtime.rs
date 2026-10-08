//! Deterministic model-run execution over DTO-only current-state storage.
//!
//! This crate has no provider, tool, timer, worker-loop, or scheduling
//! dependency. It commits one transcript row, run transition, or terminal run
//! outcome per repository transaction and publishes only the values those
//! commits returned. Assistant text and reasoning accumulate in memory and
//! become one `assistant` row per completed model step; a crash mid-step loses
//! the in-flight step text.

use intention_config::ConfigSnapshotDto;
use intention_proto::{
    DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto, FinishReasonDto, ProviderErrorDto, RunId,
    SessionId, TimestampDto, ToolCallDto, UsageDto,
};
use intention_proto::{MessageKindDto, MessageProjectionDto, RunProjectionDto, RunStatusDto};
pub use intention_providers::{
    AssistantReasoningDto, ModelCancellationSignal, ModelEventDto, ModelExecutionDriver,
    ModelMessageDto, ModelRequestDto, ModelRoleDto, ModelStreamLifecycleDto,
    ModelToolDefinitionDto,
};
use intention_storage::{
    AppendMessageInputDto, ConsumePendingUserTurnsInputDto, FinishRunInputDto,
    StorageRepositoryDto, TransitionRunInputDto,
};

use crate::context_window::ContextWindowState;

/// The durable context notice recorded when an interrupted call produced no
/// final result of its own.
pub const INTERRUPT_NOTICE: &str = "[The call was stopped before a final result.]";

/// The partial tool result recorded when an interrupt arrives before a
/// model-requested tool call starts.
pub const TOOL_INTERRUPT_NOTICE: &str = "[The tool call was stopped before a final result.]";

/// The bounded delay between two provider attempts.
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

/// Maximum bytes of one round's accumulated reasoning echo.
///
/// This per-round bound matches the transient `AssistantReasoningDto`
/// representable bound, so an echo inside it is always attachable. A round
/// that crosses this bound terminalizes as a typed failed run instead of
/// aborting `execute` with a DTO validation error (architecture 08).
const MAX_ROUND_REASONING_ECHO_BYTES: usize = 512 * 1024;

/// Commits one terminal `Failed` outcome for exactly a current starting run.
///
/// This narrow helper is used by application scheduling when a committed run
/// cannot acquire context or enter the daemon-owned dispatch queue.
///
/// # Errors
///
/// Returns a typed error when the exact run is unavailable, no longer
/// `Starting`, or the terminal outcome cannot commit.
pub fn fail_starting_run<Repository>(
    repository: &Repository,
    session_id: SessionId,
    run_id: RunId,
    failure_code: impl Into<String>,
    occurred_at: TimestampDto,
) -> DtoResult<RunProjectionDto>
where
    Repository: StorageRepositoryDto,
{
    let run = repository.load_run_projection(session_id, run_id)?;
    if run.status() != RunStatusDto::Starting {
        return Err(ErrorDto::validation(
            "invalid_starting_run_failure_state",
            "scheduling failure requires the exact run to remain starting",
        ));
    }
    repository.finish_run(FinishRunInputDto::new(
        session_id,
        run_id,
        RunStatusDto::Failed,
        None,
        None,
        Some(failure_code.into()),
        Some("the starting run could not be scheduled".to_owned()),
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

/// Safe terminal outcome of one tool call returned by the execution port.
///
/// The port selects the bounded, credential-free content the runtime commits as
/// the call's durable result and delivers to the model. A `Failed` outcome
/// carries the safe error that terminalizes the run; `Completed`, `Partial`,
/// and `Cancelled` keep the run going with the returned content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolResultOutcomeDto {
    /// The call completed with bounded model-visible content.
    Completed {
        /// The bounded, credential-free result content.
        content: String,
    },
    /// The call stopped before a final result; content carries the captured
    /// output with its interruption notice.
    Partial {
        /// The bounded captured output with its interruption notice.
        content: String,
    },
    /// The call was cancelled before it produced a final result.
    Cancelled {
        /// The bounded, credential-free content answering the call.
        content: String,
    },
    /// The call failed safely; the error terminalizes the run.
    Failed {
        /// The safe failure that terminalizes the run.
        error: ErrorDto,
    },
}

impl ToolResultOutcomeDto {
    /// Creates a completed tool outcome with non-blank content.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the content is blank.
    pub fn completed(content: impl Into<String>) -> DtoResult<Self> {
        Ok(Self::Completed {
            content: tool_result_content(content.into())?,
        })
    }

    /// Creates a partial tool outcome with non-blank content.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the content is blank.
    pub fn partial(content: impl Into<String>) -> DtoResult<Self> {
        Ok(Self::Partial {
            content: tool_result_content(content.into())?,
        })
    }

    /// Creates a cancelled tool outcome with non-blank content.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the content is blank.
    pub fn cancelled(content: impl Into<String>) -> DtoResult<Self> {
        Ok(Self::Cancelled {
            content: tool_result_content(content.into())?,
        })
    }

    /// Creates a safe failed tool outcome.
    #[must_use]
    pub const fn failed(error: ErrorDto) -> Self {
        Self::Failed { error }
    }
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

/// The cancellation handle carries interior shared state with no safe debug
/// rendering, so the input renders its durable selection only.
impl std::fmt::Debug for ModelRunExecutionInputDto {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ModelRunExecutionInputDto")
            .field("session_id", &self.session_id)
            .field("run_id", &self.run_id)
            .field("request", &self.request)
            .field("safe_config", &self.safe_config)
            .finish_non_exhaustive()
    }
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

/// Safe terminal evidence from one model execution, carrying the committed run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelRunExecutionOutcomeDto {
    /// The provider finished and the run reached completed state.
    Completed {
        /// The committed run projection.
        run: RunProjectionDto,
    },
    /// The run safely reached failed state.
    Failed {
        /// The committed run projection.
        run: RunProjectionDto,
        /// The safe failure recorded for the run.
        error: ErrorDto,
    },
}

/// Safe evidence that one model-execution commit happened.
///
/// The content variant carries the transcript row the repository committed;
/// the status variant carries the run status the repository committed. The
/// observer receives neither a repository transaction nor provider/runtime
/// resources, so it cannot publish an uncommitted mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelRunCommitDto {
    /// One committed transcript row.
    Content(MessageProjectionDto),
    /// One committed run status.
    Status {
        /// The owning session identity.
        session_id: SessionId,
        /// The committed run identity.
        run_id: RunId,
        /// The committed run status.
        status: RunStatusDto,
    },
}

/// Receives only durable commit evidence after a successful write.
///
/// A daemon publisher uses this provider-neutral seam to deliver live updates
/// from the committed values the repository returned. The model-run executor
/// and the tool-invocation path both hand their committed rows to this one
/// sink, so publication observes only durable values and never fails a commit.
pub trait ModelRunCommitObserver: Send + Sync {
    /// Observes one committed transcript row or run status.
    fn observe_model_run_commit(&self, commit: &ModelRunCommitDto);
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
    /// Returns typed storage or validation errors without retrying writes.
    #[expect(
        clippy::future_not_send,
        reason = "The DTO-only execution service accepts deterministic non-Sync test repositories; daemon composition owns any Send runtime boundary."
    )]
    pub async fn execute(
        &self,
        input: ModelRunExecutionInputDto,
    ) -> DtoResult<ModelRunExecutionOutcomeDto> {
        let run = self
            .repository
            .load_run_projection(input.session_id, input.run_id)?;
        if run.status() != RunStatusDto::Starting {
            return Err(ErrorDto::validation(
                "invalid_model_run_execution_state",
                "model execution requires a starting run",
            ));
        }
        if input.request.run_id() != input.run_id
            || input.request.model() != input.safe_config.resolved().provider().model()
        {
            return self.failed_outcome(&input, provider_configuration_unavailable());
        }
        let persisted = match self
            .repository
            .load_run_config_snapshot(input.session_id, input.run_id)
        {
            Ok(snapshot) => snapshot,
            Err(_) => {
                return self.failed_outcome(&input, provider_configuration_unavailable());
            }
        };
        if !same_execution_selection(&persisted, &input.safe_config) {
            return self.failed_outcome(&input, provider_configuration_unavailable());
        }
        if let Err(error) = self.driver.preflight(&input.request) {
            return self.failed_outcome(&input, error);
        }

        let policy = persisted.resolved().provider_execution();
        let context_window = persisted.resolved().context_window();
        // Uncommitted assistant text stays live across a retryable attempt
        // boundary, while its absence keeps the attempt budget open.
        let mut pending_text = String::new();
        let mut usage: Option<UsageDto> = None;
        let mut durable_output = false;
        // Context additions that must survive a retryable attempt boundary:
        // joined pending user messages and interruption notices. The live
        // context of the run stays continuous across provider attempts.
        let mut extra_messages: Vec<ModelMessageDto> = Vec::new();
        for attempt in 1..=u16::from(policy.max_attempts()) {
            if attempt == 1 {
                let running = self.repository.transition_run(TransitionRunInputDto::new(
                    input.session_id,
                    input.run_id,
                    RunStatusDto::Running,
                    self.time.now(),
                ))?;
                self.publish_status(input.session_id, input.run_id, running.status());
            }
            let result = self
                .drive_attempt(
                    &input,
                    policy.attempt_timeout_seconds(),
                    ContextWindowState::new(context_window.window_tokens()),
                    AttemptState {
                        pending_text: &mut pending_text,
                        usage: &mut usage,
                        durable_output: &mut durable_output,
                    },
                    &mut extra_messages,
                )
                .await?;
            match result {
                AttemptResult::Completed { run } => {
                    return Ok(ModelRunExecutionOutcomeDto::Completed { run });
                }
                AttemptResult::FailedTerminal { run, error } => {
                    return Ok(ModelRunExecutionOutcomeDto::Failed { run, error });
                }
                AttemptResult::Failed { error, retryable } => {
                    let retry = retryable
                        && !durable_output
                        && pending_text.is_empty()
                        && attempt < u16::from(policy.max_attempts());
                    if retry {
                        self.wait_for_retry(&input, &mut extra_messages).await?;
                    } else {
                        self.commit_step(&input, &mut pending_text, None)?;
                        let run = self.finish_run(
                            &input,
                            RunStatusDto::Failed,
                            None,
                            Some(&error),
                            usage.as_ref(),
                        )?;
                        return Ok(ModelRunExecutionOutcomeDto::Failed { run, error });
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
            pending_text,
            usage,
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
                    pending_text,
                    usage,
                    durable_output,
                    &mut context_window,
                )
                .await?;
            match outcome {
                RoundOutcome::Finished { reason, assistant } => {
                    *durable_output |= assistant.is_some();
                    // A pending message is the nearest-boundary continuation:
                    // it joins the live context in FIFO order and the run
                    // continues instead of completing. An interruption that
                    // raced the finish is answered with its notice first.
                    if input.cancellation.is_cancelled() {
                        self.record_interrupt_notice(input)?;
                        messages.push(interrupt_notice_message()?);
                        extra_messages.push(interrupt_notice_message()?);
                        context_window.apply(&mut messages)?;
                        request = continuation_request(input, &messages, &reasoning_attachments)?;
                        continue;
                    }
                    if self.consume_pending_user_turns(input, &mut messages, extra_messages)? {
                        context_window.apply(&mut messages)?;
                        request = continuation_request(input, &messages, &reasoning_attachments)?;
                        continue;
                    }
                    let run = self.finish_run(
                        input,
                        RunStatusDto::Completed,
                        Some(reason),
                        None,
                        usage.as_ref(),
                    )?;
                    return Ok(AttemptResult::Completed { run });
                }
                RoundOutcome::Interrupted { assistant } => {
                    *durable_output |= assistant.is_some();
                    self.record_interrupt_notice(input)?;
                    messages.push(interrupt_notice_message()?);
                    extra_messages.push(interrupt_notice_message()?);
                    context_window.apply(&mut messages)?;
                    request = continuation_request(input, &messages, &reasoning_attachments)?;
                }
                RoundOutcome::Failed { error, retryable } => {
                    if tool_round == 0 {
                        return Ok(AttemptResult::Failed { error, retryable });
                    }
                    self.commit_step(input, pending_text, None)?;
                    let run = self.finish_run(
                        input,
                        RunStatusDto::Failed,
                        None,
                        Some(&error),
                        usage.as_ref(),
                    )?;
                    return Ok(AttemptResult::FailedTerminal { run, error });
                }
                RoundOutcome::ToolCalls {
                    calls,
                    reasoning,
                    assistant,
                } => {
                    *durable_output |= assistant.is_some();
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
                        // An interrupt that arrived before this call started
                        // never begins a new effect: the call is answered with
                        // the stopped-call notice as its partial result, so the
                        // assistant tool-call message stays fully answered. The
                        // tool path owns every dispatched call's durable
                        // tool-call and tool-result rows.
                        let outcome = if input.cancellation.is_cancelled() {
                            ToolResultOutcomeDto::partial(TOOL_INTERRUPT_NOTICE)?
                        } else {
                            let outcome = self
                                .tool_executor
                                .execute_tool(input.session_id, input.run_id, call.clone())
                                .await;
                            // The tool path committed this call's evidence, so
                            // the run holds irreversible output from here on.
                            *durable_output = true;
                            match outcome {
                                Ok(outcome) => outcome,
                                Err(error) => {
                                    // A tool infrastructure error is a typed failed
                                    // tool result committed by the tool path: the
                                    // run terminalizes without retrying.
                                    let run = self.finish_run(
                                        input,
                                        RunStatusDto::Failed,
                                        None,
                                        Some(&error),
                                        usage.as_ref(),
                                    )?;
                                    return Ok(AttemptResult::FailedTerminal { run, error });
                                }
                            }
                        };
                        match outcome {
                            ToolResultOutcomeDto::Failed { error } => {
                                let run = self.finish_run(
                                    input,
                                    RunStatusDto::Failed,
                                    None,
                                    Some(&error),
                                    usage.as_ref(),
                                )?;
                                return Ok(AttemptResult::FailedTerminal { run, error });
                            }
                            ToolResultOutcomeDto::Completed { content, .. } => {
                                messages
                                    .push(ModelMessageDto::tool_result(call.call_id(), content)?);
                            }
                            ToolResultOutcomeDto::Partial { content, .. } => {
                                interrupted_tool = true;
                                messages
                                    .push(ModelMessageDto::tool_result(call.call_id(), content)?);
                            }
                            ToolResultOutcomeDto::Cancelled { content, .. } => {
                                messages
                                    .push(ModelMessageDto::tool_result(call.call_id(), content)?);
                            }
                        }
                        // Every added tool result re-runs the window pass, so
                        // the continuation request carries a trimmed context
                        // and recomputed breakpoints.
                        context_window.apply(&mut messages)?;
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
                            self.record_interrupt_notice(input)?;
                            messages.push(interrupt_notice_message()?);
                            extra_messages.push(interrupt_notice_message()?);
                        }
                    }
                    self.consume_pending_user_turns(input, &mut messages, extra_messages)?;
                    context_window.apply(&mut messages)?;
                    request = continuation_request(input, &messages, &reasoning_attachments)?;
                }
            }
        }
    }

    /// Commits one durable interruption notice and clears the run's signal.
    ///
    /// The notice tells the model that its current call was stopped before a
    /// final result; the run stays `Running` and continues with the next step.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or storage error when the notice cannot be
    /// committed.
    fn record_interrupt_notice(
        &self,
        input: &ModelRunExecutionInputDto,
    ) -> DtoResult<MessageProjectionDto> {
        input.cancellation.reset();
        let message = MessageProjectionDto::new(
            input.session_id,
            Some(input.run_id),
            MessageKindDto::Notice,
            INTERRUPT_NOTICE,
            None,
            None,
            None,
        )?;
        let committed = self
            .repository
            .append_message(AppendMessageInputDto::new(message, self.time.now()))?;
        self.publish_content(&committed);
        Ok(committed)
    }

    /// Commits every pending user message into the live run context.
    ///
    /// The storage transaction marks the turns as appended in the same commit
    /// that appends their user messages, so a turn joins exactly one run
    /// context. Every returned message is published exactly once, and the flag
    /// reports whether any message joined.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the pending batch cannot commit.
    fn consume_pending_user_turns(
        &self,
        input: &ModelRunExecutionInputDto,
        messages: &mut Vec<ModelMessageDto>,
        extra_messages: &mut Vec<ModelMessageDto>,
    ) -> DtoResult<bool> {
        let joined_messages =
            self.repository
                .consume_pending_user_turns(ConsumePendingUserTurnsInputDto::new(
                    input.session_id,
                    input.run_id,
                    self.time.now(),
                ))?;
        let mut joined = false;
        for message in &joined_messages {
            self.publish_content(message);
            let live = ModelMessageDto::new(ModelRoleDto::User, message.text())?;
            messages.push(live.clone());
            extra_messages.push(live);
            joined = true;
        }
        Ok(joined)
    }

    /// Drives one provider round and commits the completed model step.
    ///
    /// Tool-call events are only collected here; durable recording and
    /// execution happen in [`Self::drive_attempt`] against the mandatory tool
    /// executor. The round's accumulated text and reasoning commit once, when
    /// the round closes, as one assistant transcript row.
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
        pending_text: &mut String,
        usage: &mut Option<UsageDto>,
        durable_output: &mut bool,
        context_window: &mut ContextWindowState,
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
                let assistant =
                    self.commit_step(input, pending_text, step_reasoning(&reasoning_text))?;
                *durable_output |= assistant.is_some();
                return Ok(RoundOutcome::Interrupted { assistant });
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
                    let assistant =
                        self.commit_step(input, pending_text, step_reasoning(&reasoning_text))?;
                    *durable_output |= assistant.is_some();
                    return Ok(RoundOutcome::Interrupted { assistant });
                }
                Either::Right((Either::Left((item, _)), _)) => item,
                Either::Right((Either::Right(((), _)), _)) => {
                    drop(stream);
                    return Ok(RoundOutcome::Failed {
                        error: ErrorDto::unavailable(
                            "provider_attempt_timed_out",
                            "the provider attempt timed out",
                        ),
                        retryable: true,
                    });
                }
            };
            let event = match event_or_timeout {
                Some(Ok(event)) => event,
                Some(Err(error)) => {
                    return Ok(RoundOutcome::Failed {
                        retryable: error.retry() == ErrorRetryDto::Delayed,
                        error: provider_failure(&error)?,
                    });
                }
                None => {
                    if calls.is_empty() {
                        return Ok(RoundOutcome::Failed {
                            error: ErrorDto::unavailable(
                                "provider_stream_ended",
                                "the provider stream ended without a final result",
                            ),
                            retryable: false,
                        });
                    }
                    let reasoning = match round_reasoning_attachment(
                        reasoning_channel_seen,
                        &reasoning_text,
                        &calls,
                        reasoning_echo_exceeds_round_bound,
                    ) {
                        Ok(reasoning) => reasoning,
                        Err(_) => return Ok(unrepresentable_reasoning_round()),
                    };
                    let assistant =
                        self.commit_step(input, pending_text, step_reasoning(&reasoning_text))?;
                    *durable_output |= assistant.is_some();
                    return Ok(RoundOutcome::ToolCalls {
                        calls,
                        reasoning,
                        assistant,
                    });
                }
            };
            if let Err(error) = lifecycle.accept(&event) {
                return Ok(RoundOutcome::Failed {
                    error,
                    retryable: false,
                });
            }
            match event {
                ModelEventDto::Started => {}
                ModelEventDto::TextDelta { content } => {
                    pending_text.push_str(&content);
                }
                ModelEventDto::ReasoningDelta { content } => {
                    // The reasoning channel marks a presence even when it
                    // carries no text: the continuation request must send the
                    // channel back on the assistant tool-call message. Empty
                    // fragments never become durable content.
                    reasoning_channel_seen = true;
                    if !content.is_empty() {
                        // The accumulated echo is bounded per round at the
                        // attachment's representable bound. Once it is crossed
                        // the round is unrepresentable and terminalizes as a
                        // typed failed run at round end; the echo is never
                        // truncated.
                        if reasoning_echo_exceeds_round_bound
                            || reasoning_text.len() + content.len() > MAX_ROUND_REASONING_ECHO_BYTES
                        {
                            reasoning_echo_exceeds_round_bound = true;
                        } else {
                            reasoning_text.push_str(&content);
                        }
                    }
                }
                ModelEventDto::Usage { usage: reported } => {
                    context_window.observe_usage(reported, request_characters);
                    if matches!(reported, UsageDto::Reported { .. }) {
                        *usage = Some(reported);
                    }
                }
                ModelEventDto::ToolCall { call } => {
                    calls.push(call);
                }
                ModelEventDto::Finished { reason } => {
                    if calls.is_empty() {
                        let assistant =
                            self.commit_step(input, pending_text, step_reasoning(&reasoning_text))?;
                        *durable_output |= assistant.is_some();
                        return Ok(RoundOutcome::Finished { reason, assistant });
                    }
                    let reasoning = match round_reasoning_attachment(
                        reasoning_channel_seen,
                        &reasoning_text,
                        &calls,
                        reasoning_echo_exceeds_round_bound,
                    ) {
                        Ok(reasoning) => reasoning,
                        Err(_) => return Ok(unrepresentable_reasoning_round()),
                    };
                    let assistant =
                        self.commit_step(input, pending_text, step_reasoning(&reasoning_text))?;
                    *durable_output |= assistant.is_some();
                    return Ok(RoundOutcome::ToolCalls {
                        calls,
                        reasoning,
                        assistant,
                    });
                }
            }
        }
    }

    /// Waits out one scheduled retry delay, or handles an interruption.
    ///
    /// An interruption during the wait commits its notice, clears the signal,
    /// and returns so the next provider attempt starts immediately.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the notice cannot be committed.
    #[expect(
        clippy::future_not_send,
        reason = "The DTO-only execution service accepts deterministic non-Sync test repositories; daemon composition owns any Send runtime boundary."
    )]
    async fn wait_for_retry(
        &self,
        input: &ModelRunExecutionInputDto,
        extra_messages: &mut Vec<ModelMessageDto>,
    ) -> DtoResult<()> {
        use futures_util::{FutureExt, future::Either};

        if input.cancellation.is_cancelled() {
            let message = interrupt_notice_message()?;
            self.record_interrupt_notice(input)?;
            extra_messages.push(message);
            return Ok(());
        }
        let delay = self.time.sleep(RETRY_DELAY).fuse();
        let cancelled = input.cancellation.cancelled().fuse();
        futures_util::pin_mut!(delay, cancelled);
        match futures_util::future::select(cancelled, delay).await {
            Either::Left(((), _)) => {
                let message = interrupt_notice_message()?;
                self.record_interrupt_notice(input)?;
                extra_messages.push(message);
                Ok(())
            }
            Either::Right(((), _)) => Ok(()),
        }
    }

    /// Commits the accumulated assistant step as one transcript row.
    ///
    /// The step's text and reasoning accumulate in memory and become one
    /// committed `assistant` message; a step without non-blank text commits
    /// nothing, because the transcript shape requires assistant content.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or storage error when the row cannot commit.
    fn commit_step(
        &self,
        input: &ModelRunExecutionInputDto,
        pending_text: &mut String,
        reasoning: Option<&str>,
    ) -> DtoResult<Option<MessageProjectionDto>> {
        if pending_text.trim().is_empty() {
            pending_text.clear();
            return Ok(None);
        }
        let message = MessageProjectionDto::new(
            input.session_id,
            Some(input.run_id),
            MessageKindDto::Assistant,
            std::mem::take(pending_text),
            reasoning.map(str::to_owned),
            None,
            None,
        )?;
        let committed = self
            .repository
            .append_message(AppendMessageInputDto::new(message, self.time.now()))?;
        self.publish_content(&committed);
        Ok(Some(committed))
    }

    /// Commits one terminal run outcome and publishes the committed status.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or storage error when the outcome cannot commit.
    fn finish_run(
        &self,
        input: &ModelRunExecutionInputDto,
        status: RunStatusDto,
        finish_reason: Option<FinishReasonDto>,
        error: Option<&ErrorDto>,
        usage: Option<&UsageDto>,
    ) -> DtoResult<RunProjectionDto> {
        let run = self.repository.finish_run(FinishRunInputDto::new(
            input.session_id,
            input.run_id,
            status,
            usage.copied(),
            finish_reason,
            error.map(|error| error.code().to_owned()),
            error.map(|error| error.message().to_owned()),
            self.time.now(),
        )?)?;
        self.publish_status(input.session_id, input.run_id, run.status());
        Ok(run)
    }

    /// Commits one terminal `Failed` outcome carrying the supplied safe error.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the outcome cannot commit.
    fn failed_outcome(
        &self,
        input: &ModelRunExecutionInputDto,
        error: ErrorDto,
    ) -> DtoResult<ModelRunExecutionOutcomeDto> {
        let run = self.finish_run(input, RunStatusDto::Failed, None, Some(&error), None)?;
        Ok(ModelRunExecutionOutcomeDto::Failed { run, error })
    }

    /// Publishes one committed transcript row.
    fn publish_content(&self, message: &MessageProjectionDto) {
        if let Some(observer) = self.observer {
            observer.observe_model_run_commit(&ModelRunCommitDto::Content(message.clone()));
        }
    }

    /// Publishes one committed run status.
    fn publish_status(&self, session_id: SessionId, run_id: RunId, status: RunStatusDto) {
        if let Some(observer) = self.observer {
            observer.observe_model_run_commit(&ModelRunCommitDto::Status {
                session_id,
                run_id,
                status,
            });
        }
    }
}

/// Mutable per-attempt state carried across the provider attempt loop.
struct AttemptState<'a> {
    /// Accumulated uncommitted assistant text of the current model step.
    pending_text: &'a mut String,
    /// The last reported provider usage of the run.
    usage: &'a mut Option<UsageDto>,
    /// Whether this run committed irreversible content in an earlier step.
    durable_output: &'a mut bool,
}

/// The outcome of one provider attempt.
enum AttemptResult {
    /// The run reached completed state with its committed projection.
    Completed { run: RunProjectionDto },
    /// The attempt failed without terminalizing the run; the caller owns retry.
    Failed { error: ErrorDto, retryable: bool },
    /// The attempt failed and the run reached failed state.
    FailedTerminal {
        run: RunProjectionDto,
        error: ErrorDto,
    },
}

/// The outcome of one provider round.
enum RoundOutcome {
    /// The provider finished the round without tool calls. The caller owns
    /// the completion decision, because pending user messages continue the
    /// run instead of completing it. The committed assistant step row is
    /// carried when the step had text.
    Finished {
        reason: FinishReasonDto,
        assistant: Option<MessageProjectionDto>,
    },
    /// The round was interrupted before a final result; the run continues.
    Interrupted {
        assistant: Option<MessageProjectionDto>,
    },
    /// The round failed safely; no step row committed with it.
    Failed { error: ErrorDto, retryable: bool },
    /// The round closed with model-requested tool calls.
    ToolCalls {
        calls: Vec<ToolCallDto>,
        reasoning: Option<AssistantReasoningDto>,
        assistant: Option<MessageProjectionDto>,
    },
}

/// Returns the durable reasoning text of one step, when it carried any.
fn step_reasoning(reasoning: &str) -> Option<&str> {
    (!reasoning.is_empty()).then_some(reasoning)
}

/// Validates one tool result content and returns it unchanged.
///
/// # Errors
///
/// Returns a validation error when the content is blank.
fn tool_result_content(content: String) -> DtoResult<String> {
    if content.trim().is_empty() {
        return Err(ErrorDto::validation(
            "invalid_tool_result_content",
            "tool result content must not be empty",
        ));
    }
    Ok(content)
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
    text: &str,
    calls: &[ToolCallDto],
    echo_exceeds_round_bound: bool,
) -> DtoResult<Option<AssistantReasoningDto>> {
    if !reasoning_channel_seen {
        return Ok(None);
    }
    if echo_exceeds_round_bound {
        return Err(ErrorDto::validation(
            "invalid_round_reasoning_echo",
            "the round reasoning echo exceeds the per-round attachment bound",
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
/// omitted.
fn unrepresentable_reasoning_round() -> RoundOutcome {
    RoundOutcome::Failed {
        error: ErrorDto::validation(
            "reasoning_attachment_unrepresentable",
            "the round reasoning echo cannot become a continuation attachment",
        ),
        retryable: false,
    }
}

/// Converts one provider-normalized safe error into a run failure.
///
/// # Errors
///
/// Returns a validation error only when the provider code cannot form a safe
/// error, which the provider boundary already rejects.
fn provider_failure(error: &ProviderErrorDto) -> DtoResult<ErrorDto> {
    ErrorDto::new(
        error.code(),
        ErrorCategoryDto::Unavailable,
        "the provider stream failed",
        error.retry(),
        error.correlation_id(),
    )
}

/// Returns the safe failure used when the persisted configuration cannot drive
/// the run.
fn provider_configuration_unavailable() -> ErrorDto {
    ErrorDto::unavailable(
        "provider_configuration_unavailable",
        "the provider configuration is unavailable",
    )
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
