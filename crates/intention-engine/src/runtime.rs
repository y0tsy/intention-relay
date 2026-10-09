//! Deterministic model-run execution over DTO-only current-state storage.
//!
//! This crate has no provider, tool, timer, worker-loop, or scheduling
//! dependency. It commits one transcript row, run transition, or terminal run
//! outcome per repository transaction and publishes only the values those
//! commits returned. Assistant text and reasoning accumulate in memory and
//! become one `assistant` row per completed model step; a crash mid-step loses
//! the in-flight step text.

use intention_proto::provider::ResolvedRunProviderSelectionDto;
use intention_proto::{
    DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto, FinishReasonDto, ProviderErrorDto, RunId,
    SessionId, TimestampDto, ToolCallDto, UsageDto,
};
use intention_proto::{MessageKindDto, MessageProjectionDto, RunProjectionDto, RunStatusDto};
pub use intention_providers::{
    AssistantReasoningDto, ModelCancellationSignal, ModelCancelledFuture, ModelEventDto,
    ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ModelRoleDto, ModelStreamLifecycleDto,
    ModelToolDefinitionDto,
};
use intention_storage::{RunOutcomeDto, StorageRepositoryDto};
use intention_tools::CancellationSignal as ToolCancellationSignal;

use crate::context_window::ContextWindowState;
use crate::reasoning::RunReasoningAggregate;

/// The durable context notice recorded when an interrupted call produced no
/// final result of its own.
pub const INTERRUPT_NOTICE: &str = "[The call was stopped before a final result.]";

/// The durable mark of a model answer stopped before it finished.
///
/// A model step stopped by the user or the environment commits the text it had
/// produced up to that moment with this marker on its own line, so a reader can
/// tell the answer was cut; a step with no text commits nothing.
pub const INTERRUPTED_MARKER: &str = "[interrupted]";

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
    let outcome = RunOutcomeDto::new(
        RunStatusDto::Failed,
        None,
        None,
        Some(failure_code.into()),
        Some("the starting run could not be scheduled".to_owned()),
    )?;
    repository.finish_run(session_id, run_id, outcome, occurred_at)
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

/// One run-scoped cancellation handle shared by the model stream and every
/// tool invocation of the run.
///
/// The host creates one handle per admitted run, embeds it in the execution
/// input, and signals it once for an interrupt: the model stream awaits the
/// provider-neutral cancellation future while the blocking tool path observes
/// the synchronous signal. The engine's interruption boundaries reset the
/// handle, so the same run observes the next interrupt with fresh state and no
/// per-invocation registration exists.
#[derive(Clone, Default)]
pub struct RunCancellation {
    model: ModelCancellationSignal,
    tool: ToolCancellationSignal,
}

/// The cancellation handle carries interior shared state with no safe debug
/// rendering, so it renders its current observation only.
impl std::fmt::Debug for RunCancellation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RunCancellation")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl RunCancellation {
    /// Creates an active handle for one admitted run.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation of the run's in-flight operation.
    pub fn cancel(&self) {
        self.model.cancel();
        self.tool.cancel();
    }

    /// Clears the request so the continuing run observes the next interrupt.
    pub fn reset(&self) {
        self.model.reset();
        self.tool.reset();
    }

    /// Returns whether cancellation has been requested for this run.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.model.is_cancelled() || self.tool.is_cancelled()
    }

    /// Returns a fresh independently awaitable future that completes on
    /// cancellation.
    #[must_use]
    pub fn cancelled(&self) -> ModelCancelledFuture {
        self.model.cancelled()
    }

    /// Returns the provider-neutral signal the model stream observes.
    #[must_use]
    pub fn model_signal(&self) -> ModelCancellationSignal {
        self.model.clone()
    }

    /// Returns the synchronous signal one blocking tool invocation observes.
    #[must_use]
    pub fn tool_signal(&self) -> ToolCancellationSignal {
        self.tool.clone()
    }
}

/// Immutable caller-selected input for one model execution.
#[derive(Clone)]
pub struct ModelRunExecutionInputDto {
    session_id: SessionId,
    run_id: RunId,
    request: ModelRequestDto,
    selection: ResolvedRunProviderSelectionDto,
    cancellation: RunCancellation,
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
            .field("selection", &self.selection)
            .finish_non_exhaustive()
    }
}

impl ModelRunExecutionInputDto {
    /// Creates complete execution input without credentials or provider choice.
    ///
    /// The selection is the exact resolved selection of the run's persisted
    /// `run_provider_selections` row; the executor verifies it against that row
    /// before any provider work. The window policy is not part of this input:
    /// it comes from the run's committed configuration revision.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        run_id: RunId,
        request: ModelRequestDto,
        selection: ResolvedRunProviderSelectionDto,
        cancellation: RunCancellation,
    ) -> Self {
        Self {
            session_id,
            run_id,
            request,
            selection,
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

    /// Returns the resolved exact provider selection of this run.
    #[must_use]
    pub const fn selection(&self) -> &ResolvedRunProviderSelectionDto {
        &self.selection
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

/// DTO-only executor over injected storage, selected driver, time port,
/// commit observer, and tool executor.
///
/// Every collaborator is shared behind a `Sync` reference: the execution
/// lifecycle hands its futures to a scheduling runtime, so each returned
/// future is `Send` by construction.
pub struct ModelRunExecutionService<'a, Repository, Driver: ?Sized, Time> {
    repository: &'a Repository,
    driver: &'a Driver,
    time: &'a Time,
    observer: &'a dyn ModelRunCommitObserver,
    tool_executor: &'a dyn ToolExecutionPort,
}

impl<'a, Repository, Driver, Time> ModelRunExecutionService<'a, Repository, Driver, Time>
where
    Repository: StorageRepositoryDto + Sync,
    Driver: ModelExecutionDriver + Sync + ?Sized,
    Time: ModelTimePort + Sync,
{
    /// Creates an executor over the injected collaborators.
    ///
    /// Provider-emitted tool calls always execute through the supplied
    /// `ToolExecutionPort`, and every committed value is handed to the
    /// supplied [`ModelRunCommitObserver`]; neither collaborator has a
    /// fallback.
    #[must_use]
    pub const fn new(
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
            observer,
            tool_executor,
        }
    }

    /// Performs one bounded model execution lifecycle.
    ///
    /// # Errors
    ///
    /// Returns typed storage or validation errors without retrying writes.
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
        // Admission verifies the exact persisted selection before any provider
        // work: the request must address the run's own selection, and that
        // selection must be the one the run durably carries. A missing or
        // different selection is never rerouted to a current default.
        if input.request.run_id() != input.run_id
            || input.request.model() != input.selection.model_id()
        {
            return self.failed_outcome(&input, provider_configuration_unavailable());
        }
        let persisted = match self
            .repository
            .load_run_provider_selection(input.session_id, input.run_id)
        {
            Ok(selection) => selection,
            Err(_) => {
                return self.failed_outcome(&input, provider_configuration_unavailable());
            }
        };
        if persisted != input.selection {
            return self.failed_outcome(&input, provider_configuration_unavailable());
        }
        // The window policy comes from the run's own committed configuration
        // revision, never from a live or caller-supplied preference.
        let configuration = match self
            .repository
            .load_run_config_snapshot(input.session_id, input.run_id)
        {
            Ok(snapshot) => snapshot,
            Err(_) => {
                return self.failed_outcome(&input, provider_configuration_unavailable());
            }
        };

        let policy = persisted.effective_execution_policy();
        let context_window = *configuration.context_window();
        // Uncommitted assistant text of the current model step. A retryable
        // attempt failure commits it as the run's last assistant row, so
        // non-blank text closes the attempt budget; blank text commits nothing
        // and keeps that budget open.
        let mut pending_text = String::new();
        let mut usage: Option<UsageDto> = None;
        let mut durable_output = false;
        // The combined reasoning aggregate spans every round and attempt of the
        // run; crossing its fixed bound fails the run typed.
        let mut run_reasoning = RunReasoningAggregate::new();
        // Context additions that must survive a retryable attempt boundary:
        // joined pending user messages and interruption notices. The live
        // context of the run stays continuous across provider attempts.
        let mut extra_messages: Vec<ModelMessageDto> = Vec::new();
        for attempt in 1..=u16::from(policy.max_attempts()) {
            if attempt == 1 {
                let running = self.repository.transition_run(
                    input.session_id,
                    input.run_id,
                    RunStatusDto::Running,
                    self.time.now(),
                )?;
                self.publish_status(input.session_id, input.run_id, running.status());
            }
            let result = self
                .drive_attempt(
                    &input,
                    policy.attempt_timeout_seconds(),
                    RoundState::new(
                        &mut pending_text,
                        &mut usage,
                        &mut durable_output,
                        ContextWindowState::new(context_window.window_tokens()),
                    ),
                    &mut run_reasoning,
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
                    // Non-blank uncommitted text forbids a retry: it becomes the
                    // failed run's last step, so retrying would discard output
                    // the model already produced. Whitespace-only text commits
                    // nothing (`commit_step`), so it keeps the budget open.
                    let retry = retryable
                        && !durable_output
                        && pending_text.trim().is_empty()
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
        // The attempt range is enforced at the configuration boundary, not by
        // the policy type, so a snapshot carrying zero attempts fails the run
        // typed instead of panicking the spawned execution task.
        self.failed_outcome(&input, provider_configuration_unavailable())
    }

    async fn drive_attempt(
        &self,
        input: &ModelRunExecutionInputDto,
        timeout_seconds: u8,
        mut state: RoundState<'_>,
        run_reasoning: &mut RunReasoningAggregate,
        extra_messages: &mut Vec<ModelMessageDto>,
    ) -> DtoResult<AttemptResult> {
        let mut messages: Vec<ModelMessageDto> = input.request.messages().to_vec();
        messages.extend(extra_messages.iter().cloned());
        // The reasoning attachments of every closed tool round are part of the
        // measured request input, so the window accounting observes them from
        // the first pass onwards.
        let mut reasoning_attachments: Vec<AssistantReasoningDto> = Vec::new();
        // The starting context is windowed once, before its first provider
        // request, exactly like every later tool-result round.
        state.apply_window(&mut messages, &reasoning_attachments)?;
        let mut request = input.request.with_messages(messages.clone())?;
        let mut tool_round = 0u8;
        loop {
            let outcome = self
                .drive_provider_round(
                    request.clone(),
                    input,
                    timeout_seconds,
                    &mut state,
                    run_reasoning,
                )
                .await?;
            match outcome {
                RoundOutcome::Finished { reason, assistant } => {
                    state.record_step_commit(assistant.as_ref());
                    // A pending message is the nearest-boundary continuation:
                    // it joins the live context in FIFO order and the run
                    // continues instead of completing. An interruption that
                    // raced the finish is answered with its notice first.
                    if input.cancellation.is_cancelled() {
                        self.record_interrupt_notice(input)?;
                        messages.push(interrupt_notice_message()?);
                        extra_messages.push(interrupt_notice_message()?);
                        state.apply_window(&mut messages, &reasoning_attachments)?;
                        request = continuation_request(input, &messages, &reasoning_attachments)?;
                        continue;
                    }
                    if self.consume_pending_user_turns(input, &mut messages, extra_messages)? {
                        state.apply_window(&mut messages, &reasoning_attachments)?;
                        request = continuation_request(input, &messages, &reasoning_attachments)?;
                        continue;
                    }
                    let run = self.finish_run(
                        input,
                        RunStatusDto::Completed,
                        Some(reason),
                        None,
                        state.reported_usage(),
                    )?;
                    return Ok(AttemptResult::Completed { run });
                }
                RoundOutcome::Interrupted { assistant } => {
                    state.record_step_commit(assistant.as_ref());
                    self.record_interrupt_notice(input)?;
                    messages.push(interrupt_notice_message()?);
                    extra_messages.push(interrupt_notice_message()?);
                    state.apply_window(&mut messages, &reasoning_attachments)?;
                    request = continuation_request(input, &messages, &reasoning_attachments)?;
                }
                RoundOutcome::Failed { error, retryable } => {
                    if tool_round == 0 {
                        return Ok(AttemptResult::Failed { error, retryable });
                    }
                    self.commit_step(input, state.pending_text_mut(), None)?;
                    let run = self.finish_run(
                        input,
                        RunStatusDto::Failed,
                        None,
                        Some(&error),
                        state.reported_usage(),
                    )?;
                    return Ok(AttemptResult::FailedTerminal { run, error });
                }
                RoundOutcome::ToolCalls {
                    calls,
                    reasoning,
                    assistant,
                } => {
                    state.record_step_commit(assistant.as_ref());
                    tool_round += 1;
                    messages.push(ModelMessageDto::assistant_tool_calls(None, calls.clone())?);
                    // Attachments are per-round and ordered: each assistant
                    // tool-call message keeps the reasoning of its own round
                    // when later rounds rebuild the continuation request.
                    if let Some(reasoning) = reasoning {
                        reasoning_attachments.push(reasoning);
                    }
                    for call in calls {
                        // Every call is dispatched through the port, including a
                        // call whose run was already signalled: the port observes
                        // the run's signal before any effect and owns the call's
                        // durable `tool_call` row plus its terminal result row,
                        // so the assistant tool-call message stays fully
                        // answered even when the interruption pre-empts it.
                        let outcome = self
                            .tool_executor
                            .execute_tool(input.session_id, input.run_id, call.clone())
                            .await;
                        // The tool path committed this call's evidence, so
                        // the run holds irreversible output from here on.
                        state.mark_durable_output();
                        let outcome = match outcome {
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
                                    state.reported_usage(),
                                )?;
                                return Ok(AttemptResult::FailedTerminal { run, error });
                            }
                        };
                        match outcome {
                            ToolResultOutcomeDto::Failed { error } => {
                                let run = self.finish_run(
                                    input,
                                    RunStatusDto::Failed,
                                    None,
                                    Some(&error),
                                    state.reported_usage(),
                                )?;
                                return Ok(AttemptResult::FailedTerminal { run, error });
                            }
                            ToolResultOutcomeDto::Completed { content, .. } => {
                                messages
                                    .push(ModelMessageDto::tool_result(call.call_id(), content)?);
                            }
                            ToolResultOutcomeDto::Partial { content, .. } => {
                                state.mark_interrupted_tool();
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
                        state.apply_window(&mut messages, &reasoning_attachments)?;
                    }
                    // A tool batch is the second bounded interruption
                    // boundary. A partial tool result already carries the
                    // stopped-call notice, so the run continues to the next
                    // model step; every other in-flight batch position gets
                    // the explicit context notice.
                    if input.cancellation.is_cancelled() {
                        if state.interrupted_tool() {
                            input.cancellation.reset();
                        } else {
                            self.record_interrupt_notice(input)?;
                            messages.push(interrupt_notice_message()?);
                            extra_messages.push(interrupt_notice_message()?);
                        }
                    }
                    self.consume_pending_user_turns(input, &mut messages, extra_messages)?;
                    state.apply_window(&mut messages, &reasoning_attachments)?;
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
        let committed = self.repository.append_message(message, self.time.now())?;
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
        let joined_messages = self.repository.consume_pending_user_turns(
            input.session_id,
            input.run_id,
            self.time.now(),
        )?;
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
    async fn drive_provider_round(
        &self,
        request: ModelRequestDto,
        input: &ModelRunExecutionInputDto,
        timeout_seconds: u8,
        state: &mut RoundState<'_>,
        run_reasoning: &mut RunReasoningAggregate,
    ) -> DtoResult<RoundOutcome> {
        use futures_util::{FutureExt, StreamExt, future::Either};

        let mut lifecycle = ModelStreamLifecycleDto::new();
        let request_characters = ContextWindowState::request_characters(&request);
        state.begin_round();
        // The reasoning channel of this round carries categorized fragments and
        // summaries; summaries stay at the tail of the round's reasoning text.
        let mut round_reasoning = RoundReasoning::new();
        let mut stream = self
            .driver
            .execute(request, input.cancellation.model_signal());
        let timeout = self
            .time
            .sleep(std::time::Duration::from_secs(u64::from(timeout_seconds)))
            .fuse();
        futures_util::pin_mut!(timeout);
        let mut calls: Vec<ToolCallDto> = Vec::new();
        loop {
            if input.cancellation.is_cancelled() {
                drop(stream);
                let assistant = self.commit_interrupted_step(
                    input,
                    state.pending_text_mut(),
                    round_reasoning.durable_reasoning(),
                )?;
                state.record_step_commit(assistant.as_ref());
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
                    let assistant = self.commit_interrupted_step(
                        input,
                        state.pending_text_mut(),
                        round_reasoning.durable_reasoning(),
                    )?;
                    state.record_step_commit(assistant.as_ref());
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
                    let reasoning = match round_reasoning.attachment(&calls) {
                        Ok(reasoning) => reasoning,
                        Err(_) => return Ok(unrepresentable_reasoning_round()),
                    };
                    let assistant = self.commit_step(
                        input,
                        state.pending_text_mut(),
                        round_reasoning.durable_reasoning(),
                    )?;
                    state.record_step_commit(assistant.as_ref());
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
                    state.push_text(&content);
                }
                ModelEventDto::ReasoningDelta { content, .. } => {
                    // The run's combined bound is checked first: crossing it
                    // fails the whole run typed. The round echo is bounded
                    // separately at the transient attachment's representable
                    // bound, and empty fragments only mark the channel's
                    // presence.
                    if let Err(error) = round_reasoning.observe_fragment(&content, run_reasoning) {
                        return Ok(RoundOutcome::Failed {
                            error,
                            retryable: false,
                        });
                    }
                }
                ModelEventDto::ReasoningSummaryDelta { content } => {
                    // Summaries are the tail of the round's reasoning material:
                    // they never precede the fragments they summarize, and they
                    // never become ordinary assistant text.
                    if let Err(error) = round_reasoning.observe_summary(&content, run_reasoning) {
                        return Ok(RoundOutcome::Failed {
                            error,
                            retryable: false,
                        });
                    }
                }
                ModelEventDto::Usage { usage: reported } => {
                    state.observe_usage(reported, request_characters);
                }
                ModelEventDto::ToolCall { call } => {
                    calls.push(call);
                }
                ModelEventDto::Finished { reason } => {
                    if calls.is_empty() {
                        let assistant = self.commit_step(
                            input,
                            state.pending_text_mut(),
                            round_reasoning.durable_reasoning(),
                        )?;
                        state.record_step_commit(assistant.as_ref());
                        return Ok(RoundOutcome::Finished { reason, assistant });
                    }
                    let reasoning = match round_reasoning.attachment(&calls) {
                        Ok(reasoning) => reasoning,
                        Err(_) => return Ok(unrepresentable_reasoning_round()),
                    };
                    let assistant = self.commit_step(
                        input,
                        state.pending_text_mut(),
                        round_reasoning.durable_reasoning(),
                    )?;
                    state.record_step_commit(assistant.as_ref());
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
    /// committed `assistant` message carrying the step's whole reasoning text;
    /// a step without non-blank text commits nothing, because the transcript
    /// shape requires assistant content.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or storage error when the row cannot commit.
    fn commit_step(
        &self,
        input: &ModelRunExecutionInputDto,
        pending_text: &mut String,
        reasoning: Option<String>,
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
            reasoning,
            None,
            None,
        )?;
        let committed = self.repository.append_message(message, self.time.now())?;
        self.publish_content(&committed);
        Ok(Some(committed))
    }

    /// Commits one stopped model step with its durable interruption marker.
    ///
    /// The text the model produced before the stop becomes durable exactly like
    /// a completed step's text, with [`INTERRUPTED_MARKER`] on its own line; a
    /// blank or whitespace-only step commits nothing, so an interruption that
    /// produced no text leaves only the notice row.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or storage error when the row cannot commit.
    fn commit_interrupted_step(
        &self,
        input: &ModelRunExecutionInputDto,
        pending_text: &mut String,
        reasoning: Option<String>,
    ) -> DtoResult<Option<MessageProjectionDto>> {
        if !pending_text.trim().is_empty() {
            pending_text.push('\n');
            pending_text.push_str(INTERRUPTED_MARKER);
        }
        self.commit_step(input, pending_text, reasoning)
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
        let outcome = RunOutcomeDto::new(
            status,
            usage.copied(),
            finish_reason,
            error.map(|error| error.code().to_owned()),
            error.map(|error| error.message().to_owned()),
        )?;
        let run =
            self.repository
                .finish_run(input.session_id, input.run_id, outcome, self.time.now())?;
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
        self.observer
            .observe_model_run_commit(&ModelRunCommitDto::Content(message.clone()));
    }

    /// Publishes one committed run status.
    fn publish_status(&self, session_id: SessionId, run_id: RunId, status: RunStatusDto) {
        self.observer
            .observe_model_run_commit(&ModelRunCommitDto::Status {
                session_id,
                run_id,
                status,
            });
    }
}

/// Mutable state carried across one attempt's provider rounds and tool batches.
///
/// The step text, the reported usage, and the durable-output flag borrow the
/// execution's live run state for the whole attempt; the window accounting and
/// the loop flags belong to the attempt's current round, and every round
/// observes them through the transitions below instead of raw fields.
struct RoundState<'a> {
    /// Accumulated uncommitted assistant text of the current model step.
    pending_text: &'a mut String,
    /// The last reported provider usage of the run.
    usage: &'a mut Option<UsageDto>,
    /// Whether this run committed irreversible content in an earlier step.
    durable_output: &'a mut bool,
    /// Live context-window accounting of the current attempt.
    context_window: ContextWindowState,
    /// Whether the current tool batch answered an interrupted call partially.
    interrupted_tool: bool,
}

impl<'a> RoundState<'a> {
    /// Creates one attempt's state over the run's live step state.
    const fn new(
        pending_text: &'a mut String,
        usage: &'a mut Option<UsageDto>,
        durable_output: &'a mut bool,
        context_window: ContextWindowState,
    ) -> Self {
        Self {
            pending_text,
            usage,
            durable_output,
            context_window,
            interrupted_tool: false,
        }
    }

    /// Clears the previous round's observations before the next round begins.
    const fn begin_round(&mut self) {
        self.interrupted_tool = false;
    }

    /// Appends one provider text delta to the uncommitted step text.
    fn push_text(&mut self, content: &str) {
        self.pending_text.push_str(content);
    }

    /// Records one provider usage observation.
    ///
    /// The live window always observes the report; a reported usage also
    /// replaces the run's last reported usage, while a not-reported one leaves
    /// that usage untouched.
    const fn observe_usage(&mut self, reported: UsageDto, request_characters: usize) {
        self.context_window
            .observe_usage(reported, request_characters);
        if matches!(reported, UsageDto::Reported { .. }) {
            *self.usage = Some(reported);
        }
    }

    /// Applies one window accounting pass to the continuation messages.
    ///
    /// # Errors
    ///
    /// Returns a validation error only when a compressed result cannot form a
    /// valid tool-role message, which the placeholder construction prevents.
    fn apply_window(
        &self,
        messages: &mut [ModelMessageDto],
        reasoning: &[AssistantReasoningDto],
    ) -> DtoResult<()> {
        self.context_window.apply(messages, reasoning)
    }

    /// Records whether the committed step carried a durable assistant row.
    const fn record_step_commit(&mut self, assistant: Option<&MessageProjectionDto>) {
        *self.durable_output |= assistant.is_some();
    }

    /// Marks that a dispatched tool call committed its evidence.
    const fn mark_durable_output(&mut self) {
        *self.durable_output = true;
    }

    /// Returns the accumulated uncommitted step text for one commit.
    const fn pending_text_mut(&mut self) -> &mut String {
        self.pending_text
    }

    /// Returns the last reported provider usage of the run.
    const fn reported_usage(&self) -> Option<&UsageDto> {
        self.usage.as_ref()
    }

    /// Marks that the current tool batch answered an interrupted call
    /// partially.
    const fn mark_interrupted_tool(&mut self) {
        self.interrupted_tool = true;
    }

    /// Returns whether the current tool batch answered an interrupted call
    /// partially.
    const fn interrupted_tool(&self) -> bool {
        self.interrupted_tool
    }
}

/// The reasoning material one provider round produced.
///
/// Categorized fragments accumulate in arrival order and summaries accumulate
/// apart from them, so the round's reasoning text is always
/// `fragments ++ summaries`: summaries are tail-only on every surface that
/// carries the round's reasoning material. The whole echo is bounded at the
/// transient attachment's representable bound; a round that crosses it is marked
/// unrepresentable and its echo is never truncated.
struct RoundReasoning {
    fragments: String,
    summaries: String,
    channel_seen: bool,
    echo_exceeds_attachment_bound: bool,
}

impl RoundReasoning {
    /// Creates the empty reasoning material of one round.
    const fn new() -> Self {
        Self {
            fragments: String::new(),
            summaries: String::new(),
            channel_seen: false,
            echo_exceeds_attachment_bound: false,
        }
    }

    /// Records one categorized reasoning fragment.
    ///
    /// The fragment marks the round's reasoning presence even when it carries no
    /// text. The run's combined bound is checked first, so an over-bound
    /// fragment fails the run instead of being dropped.
    ///
    /// # Errors
    ///
    /// Returns `reasoning_output_limit_exceeded` when the run's combined bound
    /// is crossed.
    fn observe_fragment(
        &mut self,
        content: &str,
        run_reasoning: &mut RunReasoningAggregate,
    ) -> DtoResult<()> {
        self.channel_seen = true;
        if content.is_empty() {
            return Ok(());
        }
        run_reasoning.observe(content)?;
        self.fragments.push_str(content);
        self.check_attachment_bound();
        Ok(())
    }

    /// Records one reasoning summary delta.
    ///
    /// Summaries accumulate apart from the fragments, so the round's reasoning
    /// text always carries them at its tail.
    ///
    /// # Errors
    ///
    /// Returns `reasoning_output_limit_exceeded` when the run's combined bound
    /// is crossed.
    fn observe_summary(
        &mut self,
        content: &str,
        run_reasoning: &mut RunReasoningAggregate,
    ) -> DtoResult<()> {
        self.channel_seen = true;
        run_reasoning.observe(content)?;
        self.summaries.push_str(content);
        self.check_attachment_bound();
        Ok(())
    }

    /// Marks the echo unrepresentable once the combined echo crosses its bound.
    const fn check_attachment_bound(&mut self) {
        if self.fragments.len() + self.summaries.len() > MAX_ROUND_REASONING_ECHO_BYTES {
            self.echo_exceeds_attachment_bound = true;
        }
    }

    /// Returns the whole reasoning text of the round, when it carried any.
    ///
    /// The text is `fragments ++ summaries`; summaries never precede the
    /// fragments they summarize. The material is bounded by the run's combined
    /// bound, so the echo is never truncated.
    fn durable_reasoning(&self) -> Option<String> {
        if self.fragments.is_empty() && self.summaries.is_empty() {
            return None;
        }
        let mut text = String::with_capacity(self.fragments.len() + self.summaries.len());
        text.push_str(&self.fragments);
        text.push_str(&self.summaries);
        Some(text)
    }

    /// Builds the round's transient reasoning attachment for the tool-loop
    /// continuation.
    ///
    /// The attachment is `Some` whenever the round observed the reasoning
    /// channel, even when that channel carried no text. A round without the
    /// channel produces `None`.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the round's accumulated echo cannot form
    /// a valid attachment: it crossed the per-round attachment bound, or the
    /// attachment DTO rejects its control characters. Callers record the
    /// dedicated typed failed run instead of propagating the validation error.
    fn attachment(&self, calls: &[ToolCallDto]) -> DtoResult<Option<AssistantReasoningDto>> {
        if !self.channel_seen {
            return Ok(None);
        }
        if self.echo_exceeds_attachment_bound {
            return Err(ErrorDto::validation(
                "invalid_round_reasoning_echo",
                "the round reasoning echo exceeds the per-round attachment bound",
            ));
        }
        let tool_call_ids = calls.iter().map(ToolCallDto::call_id).collect();
        let combined = self.durable_reasoning().unwrap_or_default();
        AssistantReasoningDto::new(tool_call_ids, combined).map(Some)
    }
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
