mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{
    FIXTURE_ATTEMPT_WINDOW, FakeRepository, RecordedFinish, RecordedTransition,
    RecordingCommitObserver, VirtualTime,
};
use futures_util::stream;
use intention_config::ConfigSnapshotDto;
use intention_engine::{
    ModelRunExecutionInputDto, ModelRunExecutionOutcomeDto, ModelRunExecutionService,
    ModelTextDeltaPort, RunCancellation, SessionCommitDto, ToolExecutionPort, ToolResultOutcomeDto,
};
use intention_proto::{DtoResult, ErrorDto, RunId, SessionId, TextDeltaChannelDto};
use intention_proto::{MessageKindDto, RunStatusDto};
use intention_providers::{
    FinishReasonDto, ModelCancellationSignal, ModelCapabilitiesDto, ModelEventDto,
    ModelEventStream, ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ModelRoleDto,
    ProviderErrorDto, ToolCallDto, UsageDto,
};
use intention_test_support::{
    ScriptedDriver, fixture_snapshot_with_context_window, fixture_snapshot_with_model, run_ready,
};

fn request(run_id: RunId, model: &str) -> ModelRequestDto {
    ModelRequestDto::new(
        run_id,
        model,
        vec![ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid")],
        None,
    )
    .expect("request is valid")
}

/// A tool executor that the M4 execution fixtures never invoke.
///
/// The tool executor is mandatory on every executor; these fixtures exercise
/// provider rounds that never emit a tool call.
struct NeverInvokedToolExecutor;

impl ToolExecutionPort for NeverInvokedToolExecutor {
    fn execute_tool(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _call: ToolCallDto,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = DtoResult<ToolResultOutcomeDto>> + Send + '_>,
    > {
        Box::pin(async {
            Err(ErrorDto::validation(
                "fixture_tool_unexpected",
                "m4 execution fixtures never invoke the tool executor",
            ))
        })
    }
}

/// Captures every transient text-delta observation of one execution.
struct CapturingTextDeltaPort {
    deltas: Mutex<Vec<(u32, TextDeltaChannelDto, String)>>,
}

impl CapturingTextDeltaPort {
    /// Creates an empty capturing port.
    const fn new() -> Self {
        Self {
            deltas: Mutex::new(Vec::new()),
        }
    }

    /// Returns every observed `(step, channel, text)` triple in publication
    /// order.
    #[must_use]
    fn deltas(&self) -> Vec<(u32, TextDeltaChannelDto, String)> {
        self.deltas
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone()
    }
}

impl ModelTextDeltaPort for CapturingTextDeltaPort {
    fn text_delta(&self, step: u32, channel: TextDeltaChannelDto, text: &str) {
        self.deltas
            .lock()
            .expect("the fixture mutex is not poisoned")
            .push((step, channel, text.to_owned()));
    }
}

/// One scripted provider step: the silence that precedes it and its event.
type SilentStep = (Duration, Result<ModelEventDto, ProviderErrorDto>);

/// A scripted provider round that reports its own silences.
///
/// A step's silence advances the manual clock while the stream is pending, so
/// the runtime's armed delays observe the gap exactly as they observe real
/// wall-clock time. A round whose script is exhausted keeps advancing the clock
/// and never yields again, which models a provider that stops producing output
/// mid-answer. Every execution replays the same script, so a retried attempt
/// stalls identically.
struct SilentDriver {
    steps: Vec<SilentStep>,
    stall: Duration,
    clock: Arc<VirtualTime>,
    executions: AtomicUsize,
}

impl SilentDriver {
    /// Creates a driver replaying one scripted round per provider execution.
    #[must_use]
    fn new(steps: Vec<SilentStep>, stall: Duration, clock: &Arc<VirtualTime>) -> Self {
        Self {
            steps,
            stall,
            clock: Arc::clone(clock),
            executions: AtomicUsize::new(0),
        }
    }

    /// Returns how many provider executions started.
    #[must_use]
    fn executions(&self) -> usize {
        self.executions.load(Ordering::Relaxed)
    }
}

impl ModelExecutionDriver for SilentDriver {
    fn capabilities(&self) -> ModelCapabilitiesDto {
        ModelCapabilitiesDto::new(true)
    }

    fn execute(
        &self,
        _request: ModelRequestDto,
        _cancellation: ModelCancellationSignal,
    ) -> ModelEventStream {
        self.executions.fetch_add(1, Ordering::Relaxed);
        let steps = self.steps.clone();
        let stall = self.stall;
        let clock = Arc::clone(&self.clock);
        Box::pin(stream::unfold(
            (steps.into_iter(), true, clock, stall),
            |(mut steps, first, clock, stall)| async move {
                let Some((silence, event)) = steps.next() else {
                    loop {
                        clock.advance(stall);
                        silence_poll().await;
                    }
                };
                if !first {
                    clock.advance(silence);
                    silence_poll().await;
                }
                Some((event, (steps, false, clock, stall)))
            },
        ))
    }
}

/// Returns after one pending poll, waking itself, so the round's select
/// observes the advanced clock before the stream delivers its next event.
async fn silence_poll() {
    let mut yielded = false;
    std::future::poll_fn(|cx| {
        if yielded {
            std::task::Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    })
    .await;
}

/// Executes one scripted round through a manual clock.
fn execute_silent(
    repository: &FakeRepository,
    driver: &SilentDriver,
    request: ModelRequestDto,
    config: ConfigSnapshotDto,
    clock: &VirtualTime,
) -> DtoResult<ModelRunExecutionOutcomeDto> {
    run_ready(
        ModelRunExecutionService::new(
            repository,
            driver,
            clock,
            &RecordingCommitObserver::new(),
            &NeverInvokedToolExecutor,
        )
        .execute(ModelRunExecutionInputDto::new(
            repository.session_id,
            repository.run_id,
            request,
            config,
            RunCancellation::new(),
        )),
    )
}

fn execute(
    repository: &FakeRepository,
    driver: &ScriptedDriver,
    request: ModelRequestDto,
    config: ConfigSnapshotDto,
    signal: RunCancellation,
) -> DtoResult<ModelRunExecutionOutcomeDto> {
    let clock = VirtualTime::new();
    let tool_executor = NeverInvokedToolExecutor;
    let observer = RecordingCommitObserver::new();
    run_ready(
        ModelRunExecutionService::new(repository, driver, &clock, &observer, &tool_executor)
            .execute(ModelRunExecutionInputDto::new(
                repository.session_id,
                repository.run_id,
                request,
                config,
                signal,
            )),
    )
}

#[test]
fn observer_receives_only_committed_transcript_rows_and_statuses() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::text_delta("complete").expect("text is valid")),
        Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
    ]);
    let clock = VirtualTime::new();
    let observer = RecordingCommitObserver::new();

    let outcome = run_ready(
        ModelRunExecutionService::new(
            &repository,
            &driver,
            &clock,
            &observer,
            &NeverInvokedToolExecutor,
        )
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            RunCancellation::new(),
        )),
    )
    .expect("execution completes");

    let committed_run = repository.projection();
    assert_eq!(committed_run.status(), RunStatusDto::Completed);
    assert!(
        committed_run.finished_at().is_some(),
        "the completed run carries its committed finish time"
    );
    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { run: committed_run }
    );
    let commits = observer.commits();
    assert_eq!(
        commits.len(),
        3,
        "running status, step row, completed status"
    );
    assert!(
        matches!(
            &commits[0],
            SessionCommitDto::Status(run)
                if run.session_id() == session_id
                    && run.run_id() == run_id
                    && run.status() == RunStatusDto::Running
        ),
        "the running transition is observed with its committed projection"
    );
    let committed = repository.committed_messages();
    assert_eq!(committed.len(), 1);
    assert_eq!(commits[1], SessionCommitDto::Content(committed[0].clone()));
    assert_eq!(committed[0].session_id(), session_id);
    assert_eq!(committed[0].run_id(), Some(run_id));
    assert_eq!(committed[0].kind(), MessageKindDto::Assistant);
    assert_eq!(committed[0].text(), "complete");
    assert!(committed[0].id().value() > 0);
    assert_eq!(commits[2], SessionCommitDto::Status(committed_run));
    assert_eq!(
        repository
            .messages
            .lock()
            .expect("the fixture mutex is not poisoned")
            .len(),
        1
    );
}

#[test]
fn observer_receives_no_content_when_a_message_commit_fails() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    *repository
        .append_failure
        .lock()
        .expect("the fixture mutex is not poisoned") = Some(ErrorDto::unavailable(
        "append_failed",
        "append fails before commit",
    ));
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::text_delta("lost").expect("text is valid")),
        Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
    ]);
    let clock = VirtualTime::new();
    let observer = RecordingCommitObserver::new();

    let error = run_ready(
        ModelRunExecutionService::new(
            &repository,
            &driver,
            &clock,
            &observer,
            &NeverInvokedToolExecutor,
        )
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            RunCancellation::new(),
        )),
    )
    .expect_err("failed append must abort execution");

    assert_eq!(error.code(), "append_failed");
    assert!(
        observer
            .commits()
            .iter()
            .all(|commit| !matches!(commit, SessionCommitDto::Content(_))),
        "an uncommitted row is never published"
    );
    assert!(
        repository
            .messages
            .lock()
            .expect("the fixture mutex is not poisoned")
            .is_empty()
    );
    assert!(
        repository
            .finishes
            .lock()
            .expect("the fixture mutex is not poisoned")
            .is_empty()
    );
}

#[test]
fn streams_commit_one_assistant_step_with_reasoning_and_complete() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let content = format!("{}€tail", "a".repeat(4094));
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::text_delta(content.clone()).expect("text is valid")),
        Ok(ModelEventDto::reasoning_delta("why").expect("reasoning is valid")),
        Ok(ModelEventDto::usage(
            UsageDto::reported(1, 2, 3).expect("usage is valid"),
        )),
        Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
    ]);
    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("run completes");
    let ModelRunExecutionOutcomeDto::Completed { run } = outcome else {
        unreachable!("a stop reason completes the run");
    };
    assert_eq!(run.status(), RunStatusDto::Completed);
    let messages = repository.committed_messages();
    assert_eq!(
        messages.len(),
        1,
        "the whole step commits as one row with its reasoning"
    );
    assert_eq!(messages[0].session_id(), session_id);
    assert_eq!(messages[0].run_id(), Some(run_id));
    assert_eq!(messages[0].kind(), MessageKindDto::Assistant);
    assert_eq!(messages[0].text(), content);
    assert_eq!(messages[0].reasoning(), Some("why"));
    assert!(messages[0].id().value() > 0);
    let finishes = repository
        .finishes
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
    assert_eq!(finishes[0].finish_reason(), Some(FinishReasonDto::Stop));
    assert_eq!(
        finishes[0].usage(),
        Some(&UsageDto::reported(1, 2, 3).expect("usage is valid"))
    );
    assert_eq!(
        repository
            .transitions
            .lock()
            .expect("the fixture mutex is not poisoned")
            .iter()
            .map(RecordedTransition::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Running]
    );
}

#[test]
fn malformed_provider_and_eof_streams_safely_fail_without_committing_content() {
    for (events, expected_code) in [
        (vec![Ok(ModelEventDto::started())], "provider_stream_ended"),
        (
            vec![
                Ok(ModelEventDto::started()),
                Err(ProviderErrorDto::unavailable("provider_down", false).expect("error is valid")),
            ],
            "provider_down",
        ),
    ] {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let config = fixture_snapshot_with_model("fixture");
        let repository = FakeRepository::new(session_id, run_id, config.clone());
        let driver = ScriptedDriver::new(events);
        let outcome = execute(
            &repository,
            &driver,
            request(run_id, "fixture"),
            config,
            RunCancellation::new(),
        )
        .expect("safe failure commits");
        let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
            unreachable!("a malformed stream fails the run");
        };
        assert_eq!(error.code(), expected_code);
        assert_eq!(run.status(), RunStatusDto::Failed);
        assert!(
            repository
                .messages
                .lock()
                .expect("the fixture mutex is not poisoned")
                .is_empty(),
            "an invalid stream never commits assistant content"
        );
        let finishes = repository
            .finishes
            .lock()
            .expect("the fixture mutex is not poisoned")
            .clone();
        assert_eq!(finishes.len(), 1);
        assert_eq!(finishes[0].error_code(), Some(expected_code));
    }
}

#[test]
fn interruption_records_a_notice_and_continues_the_same_run() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = RunCancellation::new();
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("partial answer").expect("text is valid")),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("final answer").expect("text is valid")),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    // The interrupt is requested while the first provider stream is live,
    // after its partial text was delivered.
    driver.cancel_during_stream(1, signal.model_signal());
    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        signal,
    )
    .expect("the interrupted run continues and completes");
    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(driver.executions(), 2);
    let messages = repository
        .messages
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text().to_owned()))
            .collect::<Vec<_>>(),
        vec![
            (
                MessageKindDto::Assistant,
                format!("partial answer\n{}", intention_engine::INTERRUPTED_MARKER),
            ),
            (
                MessageKindDto::Notice,
                intention_engine::INTERRUPT_NOTICE.to_owned(),
            ),
            (MessageKindDto::Assistant, "final answer".to_owned()),
        ],
        "the stopped step commits its text with the interruption marker and the durable notice"
    );
    assert_eq!(messages[1].run_id(), Some(run_id));
    drop(messages);
    // The continuation request carries the notice, so the model knows its
    // previous call was stopped before a final result.
    let requests = driver.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].messages().iter().any(|message| {
        message.role() == ModelRoleDto::Notice
            && message.content() == intention_engine::INTERRUPT_NOTICE
    }));
    drop(requests);
    let finishes = repository
        .finishes
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(finishes.len(), 1, "one terminal outcome commits");
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
    assert_eq!(
        repository
            .transitions
            .lock()
            .expect("the fixture mutex is not poisoned")
            .iter()
            .map(RecordedTransition::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Running]
    );
}

#[test]
fn an_interrupt_before_the_first_round_still_records_a_notice_and_continues() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = RunCancellation::new();
    signal.cancel();
    let driver = ScriptedDriver::with_rounds(vec![
        Vec::new(),
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);

    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        signal,
    )
    .expect("a pre-requested interrupt is handled and the run continues");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        driver.executions(),
        2,
        "the interrupted round is followed by the next provider step"
    );
    let messages = repository
        .messages
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![(MessageKindDto::Notice, intention_engine::INTERRUPT_NOTICE)],
        "an interrupted step without text commits only its notice"
    );
    drop(messages);
    let finishes = repository
        .finishes
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
}

#[test]
fn an_interrupt_whose_notice_cannot_commit_surfaces_the_storage_error() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = RunCancellation::new();
    // The stopped step commits its text first; the interruption notice append
    // then fails as the second committed row.
    *repository
        .append_failure_at
        .lock()
        .expect("the fixture mutex is not poisoned") = Some((
        2,
        ErrorDto::unavailable(
            "fixture_notice_unavailable",
            "the interrupt notice could not be stored",
        ),
    ));
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::text_delta("late").expect("text is valid")),
    ]);
    driver.cancel_during_stream(1, signal.model_signal());

    let error = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        signal,
    )
    .expect_err("an uncommittable notice is a typed error, never a silent stop");
    assert_eq!(error.code(), "fixture_notice_unavailable");
    assert_eq!(driver.executions(), 1);
    assert_eq!(
        repository
            .messages
            .lock()
            .expect("the fixture mutex is not poisoned")
            .len(),
        1
    );
    assert!(
        repository
            .finishes
            .lock()
            .expect("the fixture mutex is not poisoned")
            .is_empty()
    );
}

#[test]
fn retry_is_ordered_once_and_waits_exactly_250_milliseconds() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::reasoning_delta("why").expect("reasoning is valid")),
            Err(ProviderErrorDto::unavailable("provider_down", true).expect("error is valid")),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let clock = VirtualTime::new();
    let outcome = run_ready(
        ModelRunExecutionService::new(
            &repository,
            &driver,
            &clock,
            &RecordingCommitObserver::new(),
            &NeverInvokedToolExecutor,
        )
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            RunCancellation::new(),
        )),
    )
    .expect("retry completes");
    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(driver.executions(), 2);
    assert_eq!(
        clock
            .sleeps()
            .iter()
            .filter(|&&duration| duration == Duration::from_millis(250))
            .count(),
        1
    );
    assert_eq!(
        repository
            .transitions
            .lock()
            .expect("the fixture mutex is not poisoned")
            .iter()
            .map(RecordedTransition::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Running]
    );
    let finishes = repository
        .finishes
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
    assert!(
        repository
            .messages
            .lock()
            .expect("the fixture mutex is not poisoned")
            .is_empty(),
        "an uncommitted reasoning echo never becomes a transcript row"
    );
}

#[test]
fn model_execution_preconditions_fail_without_provider_calls() {
    // A persisted configuration that disagrees with the requested selection is
    // a safe terminal failure with no provider work.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let repository =
        FakeRepository::new(session_id, run_id, fixture_snapshot_with_model("persisted"));
    let driver = ScriptedDriver::new(Vec::new());
    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "current"),
        fixture_snapshot_with_model("current"),
        RunCancellation::new(),
    )
    .expect("mismatch safely fails");
    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("a configuration mismatch fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_configuration_unavailable");
    assert_eq!(driver.executions(), 0);
    let finishes = repository
        .finishes
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(finishes.len(), 1);
    assert_eq!(
        finishes[0].error_code(),
        Some("provider_configuration_unavailable")
    );
    drop(finishes);
    drop(repository);

    // An unavailable persisted configuration behaves the same way.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    *repository
        .config_error
        .lock()
        .expect("the fixture mutex is not poisoned") = Some(ErrorDto::unavailable(
        "configuration_not_found",
        "the persisted configuration is unavailable",
    ));
    let driver = ScriptedDriver::new(Vec::new());
    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("configuration absence becomes durable safe failure");
    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("configuration absence fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_configuration_unavailable");
    assert_eq!(driver.executions(), 0);
    let finishes = repository
        .finishes
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(finishes.len(), 1);
    assert_eq!(
        finishes[0].error_code(),
        Some("provider_configuration_unavailable")
    );
    drop(finishes);
    drop(repository);

    // A request that names another run is rejected the same way.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::new(Vec::new());
    let outcome = execute(
        &repository,
        &driver,
        request(RunId::new(), "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("wrong request identity becomes durable safe failure");
    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("a wrong request identity fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_configuration_unavailable");
    assert_eq!(driver.executions(), 0);
    let finishes = repository
        .finishes
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(finishes.len(), 1);
    assert_eq!(
        finishes[0].error_code(),
        Some("provider_configuration_unavailable")
    );
}

#[test]
fn execution_rejects_non_starting_run_before_configuration_or_provider_work() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    *repository
        .status
        .lock()
        .expect("the fixture mutex is not poisoned") = RunStatusDto::Running;
    let driver = ScriptedDriver::new(Vec::new());

    let error = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect_err("a non-starting run cannot execute");

    assert_eq!(error.code(), "invalid_model_run_execution_state");
    assert_eq!(driver.executions(), 0);
    assert!(
        repository
            .messages
            .lock()
            .expect("the fixture mutex is not poisoned")
            .is_empty()
    );
    assert!(
        repository
            .finishes
            .lock()
            .expect("the fixture mutex is not poisoned")
            .is_empty()
    );
    assert!(
        repository
            .transitions
            .lock()
            .expect("the fixture mutex is not poisoned")
            .is_empty()
    );
}

#[test]
fn retryable_failure_after_a_committed_step_does_not_retry() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = RunCancellation::new();
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("partial answer").expect("text is valid")),
        ],
        vec![Err(
            ProviderErrorDto::unavailable("provider_down", true).expect("error is valid")
        )],
    ]);
    driver.cancel_during_stream(1, signal.model_signal());
    let clock = VirtualTime::new();

    let outcome = run_ready(
        ModelRunExecutionService::new(
            &repository,
            &driver,
            &clock,
            &RecordingCommitObserver::new(),
            &NeverInvokedToolExecutor,
        )
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            signal,
        )),
    )
    .expect("a committed step suppresses the retry");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("the retryable failure terminalizes the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_down");
    assert_eq!(driver.executions(), 2);
    assert_eq!(
        clock
            .sleeps()
            .iter()
            .filter(|&&duration| duration == Duration::from_millis(250))
            .count(),
        0,
        "no retry delay runs after committed content"
    );
    assert_eq!(
        repository
            .messages
            .lock()
            .expect("the fixture mutex is not poisoned")
            .len(),
        2
    );
    let finishes = repository
        .finishes
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Failed);
    assert_eq!(finishes[0].error_code(), Some("provider_down"));
}

#[test]
fn exhausted_retryable_failure_stops_after_second_attempt() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::with_rounds(vec![
        vec![Err(
            ProviderErrorDto::unavailable("provider_down", true).expect("error is valid")
        )],
        vec![Err(
            ProviderErrorDto::unavailable("provider_down", true).expect("error is valid")
        )],
    ]);
    let clock = VirtualTime::new();

    let outcome = run_ready(
        ModelRunExecutionService::new(
            &repository,
            &driver,
            &clock,
            &RecordingCommitObserver::new(),
            &NeverInvokedToolExecutor,
        )
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            RunCancellation::new(),
        )),
    )
    .expect("second retryable failure becomes terminal");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("an exhausted attempt budget fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_down");
    assert_eq!(driver.executions(), 2);
    assert_eq!(
        clock
            .sleeps()
            .iter()
            .filter(|&&duration| duration == Duration::from_millis(250))
            .count(),
        1,
        "exactly one retry delay runs"
    );
    let finishes = repository
        .finishes
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].error_code(), Some("provider_down"));
}

#[test]
fn provider_timeout_retries_then_records_a_terminal_timeout_failure() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::new(Vec::new());
    // The stream yields nothing at all, so the attempt's idle window expires
    // without a single progress event.
    driver.stay_pending();
    let clock = VirtualTime::new();

    let outcome = run_ready(
        ModelRunExecutionService::new(
            &repository,
            &driver,
            &clock,
            &RecordingCommitObserver::new(),
            &NeverInvokedToolExecutor,
        )
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            RunCancellation::new(),
        )),
    )
    .expect("exhausted timeouts become a durable failure");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("exhausted timeouts fail the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_attempt_timed_out");
    assert_eq!(driver.executions(), 2);
    assert_eq!(
        clock.sleeps().as_slice(),
        &[
            Duration::from_secs(30),
            Duration::from_millis(250),
            Duration::from_secs(30)
        ]
    );
    let finishes = repository
        .finishes
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].error_code(), Some("provider_attempt_timed_out"));
    assert!(
        repository
            .messages
            .lock()
            .expect("the fixture mutex is not poisoned")
            .is_empty(),
        "a timed-out attempt commits no transcript row"
    );
}

/// The silence each scripted provider chunk follows in the idle-window tests.
const CHUNK_SILENCE: Duration = Duration::from_secs(20);

#[test]
fn continuous_provider_progress_outlives_the_attempt_timeout_and_completes() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let clock = Arc::new(VirtualTime::new().holding([FIXTURE_ATTEMPT_WINDOW]));
    // Every chunk follows a 20-second silence, so the stream runs for 100
    // seconds while never falling silent for the 30-second configured window.
    let driver = SilentDriver::new(
        vec![
            (Duration::ZERO, Ok(ModelEventDto::started())),
            (
                CHUNK_SILENCE,
                Ok(ModelEventDto::text_delta("still ").expect("text is valid")),
            ),
            (
                CHUNK_SILENCE,
                Ok(ModelEventDto::reasoning_delta("working").expect("reasoning is valid")),
            ),
            (
                CHUNK_SILENCE,
                Ok(ModelEventDto::text_delta("making ").expect("text is valid")),
            ),
            (
                CHUNK_SILENCE,
                Ok(ModelEventDto::usage(
                    UsageDto::reported(1, 2, 3).expect("usage is valid"),
                )),
            ),
            (
                CHUNK_SILENCE,
                Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
            ),
        ],
        CHUNK_SILENCE,
        &clock,
    );

    let outcome = execute_silent(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        &clock,
    )
    .expect("a continuously producing stream completes");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        driver.executions(),
        1,
        "a stream that keeps producing progress is never retried"
    );
    assert_eq!(
        clock.elapsed(),
        Duration::from_secs(100),
        "the round outlived the configured 30-second window more than three times over"
    );
    assert_eq!(
        clock.sleeps(),
        vec![Duration::from_secs(30); 6],
        "every delivered event re-arms one fresh idle window"
    );
    let messages = repository
        .messages
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text().to_owned()))
            .collect::<Vec<_>>(),
        vec![(MessageKindDto::Assistant, "still making ".to_owned())],
        "the slow stream commits its whole answer once"
    );
}

#[test]
fn a_silent_stall_expires_the_idle_window_armed_after_the_last_event() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let clock = Arc::new(VirtualTime::new().holding([FIXTURE_ATTEMPT_WINDOW]));
    // A usage fact arrives after a 10-second silence and the provider then goes
    // silent for good: the window that expires is the fresh one armed after
    // that last event, not one armed when the attempt started.
    let driver = SilentDriver::new(
        vec![
            (Duration::ZERO, Ok(ModelEventDto::started())),
            (
                Duration::from_secs(10),
                Ok(ModelEventDto::usage(
                    UsageDto::reported(1, 2, 3).expect("usage is valid"),
                )),
            ),
        ],
        Duration::from_secs(10),
        &clock,
    );

    let outcome = execute_silent(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        &clock,
    )
    .expect("exhausted silent stalls become a durable failure");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("exhausted silent stalls fail the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_attempt_timed_out");
    assert_eq!(
        driver.executions(),
        2,
        "a silent attempt keeps the blank-text budget open and the retry stalls too"
    );
    assert_eq!(
        clock.elapsed(),
        Duration::from_secs(80) + Duration::from_millis(250),
        "each attempt expired 30 seconds after its own last event, not 30 seconds after its start"
    );
    assert_eq!(
        clock.sleeps(),
        vec![
            Duration::from_secs(30),
            Duration::from_secs(30),
            Duration::from_secs(30),
            Duration::from_millis(250),
            Duration::from_secs(30),
            Duration::from_secs(30),
            Duration::from_secs(30),
        ],
        "each delivered event arms one fresh window and exactly one retry delay runs"
    );
    let finishes = repository
        .finishes
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].error_code(), Some("provider_attempt_timed_out"));
    assert!(
        repository
            .messages
            .lock()
            .expect("the fixture mutex is not poisoned")
            .is_empty(),
        "a stalled attempt commits no transcript row"
    );
}

#[test]
fn a_completed_step_never_carries_the_interrupted_marker() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::completed_text();

    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        config,
        RunCancellation::new(),
    )
    .expect("a completed run commits its step unchanged");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    let messages = repository
        .messages
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].kind(), MessageKindDto::Assistant);
    assert_eq!(messages[0].text(), "complete response");
    assert!(
        !messages[0]
            .text()
            .contains(intention_engine::INTERRUPTED_MARKER),
        "only a stopped step carries the interruption marker"
    );
}

#[test]
fn retryable_failure_with_uncommitted_text_commits_the_text_and_does_not_retry() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::with_rounds(vec![vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::text_delta("partial answer").expect("text is valid")),
        Err(ProviderErrorDto::unavailable("provider_down", true).expect("error is valid")),
    ]]);
    let clock = VirtualTime::new();

    let outcome = run_ready(
        ModelRunExecutionService::new(
            &repository,
            &driver,
            &clock,
            &RecordingCommitObserver::new(),
            &NeverInvokedToolExecutor,
        )
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            RunCancellation::new(),
        )),
    )
    .expect("uncommitted text closes the attempt budget");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("the retryable failure terminalizes the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_down");
    assert_eq!(
        driver.executions(),
        1,
        "uncommitted text forbids a second attempt"
    );
    assert!(
        clock
            .sleeps()
            .iter()
            .filter(|&&duration| duration == Duration::from_millis(250))
            .count()
            == 0,
        "no retry delay runs once the text is committed"
    );
    let messages = repository
        .messages
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![(MessageKindDto::Assistant, "partial answer")],
        "the uncommitted step text becomes the failed run's last row"
    );
}

#[test]
fn whitespace_only_text_keeps_the_attempt_budget_open() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("   ").expect("text is valid")),
            Err(ProviderErrorDto::unavailable("provider_down", true).expect("error is valid")),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let clock = VirtualTime::new();

    let outcome = run_ready(
        ModelRunExecutionService::new(
            &repository,
            &driver,
            &clock,
            &RecordingCommitObserver::new(),
            &NeverInvokedToolExecutor,
        )
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            RunCancellation::new(),
        )),
    )
    .expect("whitespace-only text is no durable output and retries");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(driver.executions(), 2);
    assert_eq!(
        clock.sleeps().as_slice(),
        &[
            Duration::from_secs(30),
            Duration::from_secs(30),
            Duration::from_secs(30),
            Duration::from_millis(250),
            Duration::from_secs(30),
            Duration::from_secs(30)
        ],
        "every delivered event re-arms the idle window and exactly one retry delay runs"
    );
    assert!(
        repository
            .messages
            .lock()
            .expect("the fixture mutex is not poisoned")
            .is_empty(),
        "whitespace-only text commits no transcript row"
    );
}

#[test]
fn a_persisted_window_policy_disagreement_fails_the_run_before_provider_work() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let repository = FakeRepository::new(
        session_id,
        run_id,
        fixture_snapshot_with_context_window("fixture", Some(60)),
    );
    let driver = ScriptedDriver::new(Vec::new());

    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        fixture_snapshot_with_context_window("fixture", Some(80)),
        RunCancellation::new(),
    )
    .expect("a window policy disagreement safely fails");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("a window policy disagreement fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_configuration_unavailable");
    assert_eq!(
        driver.executions(),
        0,
        "the disagreement is rejected before any provider work"
    );
    let finishes = repository
        .finishes
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(finishes.len(), 1);
    assert_eq!(
        finishes[0].error_code(),
        Some("provider_configuration_unavailable")
    );
}

#[test]
fn text_delta_port_observes_both_channels_with_their_model_step_index() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = RunCancellation::new();
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::reasoning_delta("why").expect("reasoning is valid")),
            Ok(ModelEventDto::text_delta("partial ").expect("text is valid")),
            Ok(ModelEventDto::text_delta("answer").expect("text is valid")),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("final answer").expect("text is valid")),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    // The interrupt lands after both chunks of the first round were delivered,
    // so the run continues with a second model step of its own.
    driver.cancel_during_stream(3, signal.model_signal());
    let clock = VirtualTime::new();
    let observer = RecordingCommitObserver::new();
    let deltas = CapturingTextDeltaPort::new();

    let outcome = run_ready(
        ModelRunExecutionService::new(
            &repository,
            &driver,
            &clock,
            &observer,
            &NeverInvokedToolExecutor,
        )
        .with_text_delta_port(&deltas)
        .execute(ModelRunExecutionInputDto::new(
            session_id,
            run_id,
            request(run_id, "fixture"),
            config,
            signal,
        )),
    )
    .expect("the interrupted run continues and completes");

    assert!(matches!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed { .. }
    ));
    assert_eq!(
        deltas.deltas(),
        vec![
            (0, TextDeltaChannelDto::Reasoning, "why".to_owned()),
            (0, TextDeltaChannelDto::Answer, "partial ".to_owned()),
            (0, TextDeltaChannelDto::Answer, "answer".to_owned()),
            (1, TextDeltaChannelDto::Answer, "final answer".to_owned()),
        ],
        "every chunk is published once with its own step and channel, and the \
         reasoning channel never becomes answer text"
    );
    let messages = repository
        .messages
        .lock()
        .expect("the fixture mutex is not poisoned")
        .clone();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text().to_owned()))
            .collect::<Vec<_>>(),
        vec![
            (
                MessageKindDto::Assistant,
                format!("partial answer\n{}", intention_engine::INTERRUPTED_MARKER),
            ),
            (
                MessageKindDto::Notice,
                intention_engine::INTERRUPT_NOTICE.to_owned(),
            ),
            (MessageKindDto::Assistant, "final answer".to_owned()),
        ],
        "each committed step row holds exactly the chunks published for its step"
    );
}

/// Cross-run comparable durable evidence of one fixture execution.
type RoundEvidence = (
    Vec<(MessageKindDto, String)>,
    Vec<RunStatusDto>,
    Vec<RunStatusDto>,
);

/// Runs the two-round interrupted fixture, optionally observing its text.
fn interrupted_round_evidence(
    session_id: SessionId,
    run_id: RunId,
    port: Option<&CapturingTextDeltaPort>,
) -> RoundEvidence {
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    let signal = RunCancellation::new();
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("partial answer").expect("text is valid")),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::text_delta("final answer").expect("text is valid")),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    driver.cancel_during_stream(1, signal.model_signal());
    let clock = VirtualTime::new();
    let observer = RecordingCommitObserver::new();
    let service = ModelRunExecutionService::new(
        &repository,
        &driver,
        &clock,
        &observer,
        &NeverInvokedToolExecutor,
    );
    let service = match port {
        Some(port) => service.with_text_delta_port(port),
        None => service,
    };
    run_ready(service.execute(ModelRunExecutionInputDto::new(
        session_id,
        run_id,
        request(run_id, "fixture"),
        config,
        signal,
    )))
    .expect("the interrupted run continues and completes");
    let messages = repository
        .messages
        .lock()
        .expect("the fixture mutex is not poisoned")
        .iter()
        .map(|message| (message.kind(), message.text().to_owned()))
        .collect();
    let finishes = repository
        .finishes
        .lock()
        .expect("the fixture mutex is not poisoned")
        .iter()
        .map(RecordedFinish::status)
        .collect();
    // The published statuses are the comparable part of a status commit: the
    // projection also carries the identities this helper generates per run.
    let statuses = observer
        .commits()
        .into_iter()
        .filter_map(|commit| match commit {
            SessionCommitDto::Status(run) => Some(run.status()),
            SessionCommitDto::Content(_) => None,
        })
        .collect();
    (messages, statuses, finishes)
}

#[test]
fn an_unattached_text_delta_port_keeps_the_run_behavior_unchanged() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let observed = CapturingTextDeltaPort::new();

    let with_port = interrupted_round_evidence(session_id, run_id, Some(&observed));
    let without_port = interrupted_round_evidence(session_id, run_id, None);

    assert_eq!(
        with_port, without_port,
        "the transient port changes neither the committed rows nor the published statuses"
    );
    assert_eq!(
        observed.deltas(),
        vec![
            (0, TextDeltaChannelDto::Answer, "partial answer".to_owned()),
            (1, TextDeltaChannelDto::Answer, "final answer".to_owned()),
        ],
        "the attached port observed both model steps of the unchanged run"
    );
}

#[test]
fn one_interrupt_request_offers_exactly_one_notice_to_the_run_boundaries() {
    let cancellation = RunCancellation::new();

    assert!(
        !cancellation.take_interrupt_notice(),
        "an active run owes no notice"
    );

    cancellation.cancel();
    assert!(
        cancellation.take_interrupt_notice(),
        "the first boundary to reach the request takes its notice"
    );
    assert!(
        !cancellation.take_interrupt_notice(),
        "however many boundaries observe the sticky signal, one request offers one notice"
    );
    assert!(
        cancellation.is_cancelled(),
        "the sticky signal still tells every in-flight observer the run was interrupted"
    );

    cancellation.reset();
    assert!(
        !cancellation.is_cancelled(),
        "the boundary that ends the interruption clears it for the continuation round"
    );

    cancellation.cancel();
    assert!(
        cancellation.take_interrupt_notice(),
        "the next request records its own notice"
    );
}
