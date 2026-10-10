#![allow(
    clippy::expect_used,
    reason = "Execution fixtures use expect to provide precise failures."
)]

mod common;

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use common::{
    FakeRepository, ImmediateTime, RecordedFinish, RecordedTransition, RecordingCommitObserver,
};
use intention_config::ConfigSnapshotDto;
use intention_engine::{
    ModelRunCommitDto, ModelRunExecutionInputDto, ModelRunExecutionOutcomeDto,
    ModelRunExecutionService, ModelTextDeltaPort, RunCancellation, ToolExecutionPort,
    ToolResultOutcomeDto,
};
use intention_proto::{DtoResult, ErrorDto, RunId, SessionId, TextDeltaChannelDto};
use intention_proto::{MessageKindDto, MessageProjectionDto, RunProjectionDto, RunStatusDto};
use intention_providers::{
    FinishReasonDto, ModelEventDto, ModelMessageDto, ModelRequestDto, ModelRoleDto,
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
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl ModelTextDeltaPort for CapturingTextDeltaPort {
    fn text_delta(&self, step: u32, channel: TextDeltaChannelDto, text: &str) {
        self.deltas
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((step, channel, text.to_owned()));
    }
}

fn execute(
    repository: &FakeRepository,
    driver: &ScriptedDriver,
    request: ModelRequestDto,
    config: ConfigSnapshotDto,
    signal: RunCancellation,
) -> DtoResult<ModelRunExecutionOutcomeDto> {
    let clock = ImmediateTime::new();
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
    let clock = ImmediateTime::new();
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

    assert_eq!(
        outcome,
        ModelRunExecutionOutcomeDto::Completed {
            run: RunProjectionDto::new(
                session_id,
                run_id,
                repository.projection().turn_id(),
                RunStatusDto::Completed,
                repository.config.revision_id(),
            )
        }
    );
    let commits = observer.commits();
    assert_eq!(
        commits.len(),
        3,
        "running status, step row, completed status"
    );
    assert_eq!(
        commits[0],
        ModelRunCommitDto::Status {
            session_id,
            run_id,
            status: RunStatusDto::Running,
        }
    );
    assert_eq!(
        commits[1],
        ModelRunCommitDto::Content(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::Assistant,
                "complete",
                None,
                None,
                None,
            )
            .expect("fixture message is valid")
        )
    );
    assert_eq!(
        commits[2],
        ModelRunCommitDto::Status {
            session_id,
            run_id,
            status: RunStatusDto::Completed,
        }
    );
    assert_eq!(
        repository
            .messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
        .unwrap_or_else(PoisonError::into_inner) = Some(ErrorDto::unavailable(
        "append_failed",
        "append fails before commit",
    ));
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::text_delta("lost").expect("text is valid")),
        Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
    ]);
    let clock = ImmediateTime::new();
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
            .all(|commit| !matches!(commit, ModelRunCommitDto::Content(_))),
        "an uncommitted row is never published"
    );
    assert!(
        repository
            .messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    );
    assert!(
        repository
            .finishes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
    let messages = repository
        .messages
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(
        messages.as_slice(),
        &[MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            content,
            Some("why".to_owned()),
            None,
            None,
        )
        .expect("fixture message is valid")],
        "the whole step commits as one row with its reasoning"
    );
    drop(messages);
    let finishes = repository
        .finishes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
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
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(RecordedTransition::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Running]
    );
}

#[test]
fn malformed_provider_and_eof_streams_safely_fail_without_committing_content() {
    for (events, expected_code) in [
        (
            vec![Ok(
                ModelEventDto::text_delta("invalid").expect("text is valid")
            )],
            "invalid_model_stream_order",
        ),
        (vec![Ok(ModelEventDto::started())], "provider_stream_ended"),
        (
            vec![
                Ok(ModelEventDto::started()),
                Err(ProviderErrorDto::unavailable("provider_down", false, None)
                    .expect("error is valid")),
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
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty(),
            "an invalid stream never commits assistant content"
        );
        let finishes = repository
            .finishes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
        .unwrap_or_else(PoisonError::into_inner)
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
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(finishes.len(), 1, "one terminal outcome commits");
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
    assert_eq!(
        repository
            .transitions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
        .unwrap_or_else(PoisonError::into_inner)
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
        .unwrap_or_else(PoisonError::into_inner)
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
        .unwrap_or_else(PoisonError::into_inner) = Some((
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
            .unwrap_or_else(PoisonError::into_inner)
            .len(),
        1
    );
    assert!(
        repository
            .finishes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
            Err(ProviderErrorDto::unavailable("provider_down", true, None).expect("error is valid")),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let clock = ImmediateTime::new();
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
            .sleeps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|&&duration| duration == Duration::from_millis(250))
            .count(),
        1
    );
    assert_eq!(
        repository
            .transitions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(RecordedTransition::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Running]
    );
    let finishes = repository
        .finishes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
    assert!(
        repository
            .messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
        .unwrap_or_else(PoisonError::into_inner)
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
        .unwrap_or_else(PoisonError::into_inner) = Some(ErrorDto::unavailable(
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
        .unwrap_or_else(PoisonError::into_inner)
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
        .unwrap_or_else(PoisonError::into_inner)
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
        .unwrap_or_else(PoisonError::into_inner) = RunStatusDto::Running;
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
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    );
    assert!(
        repository
            .finishes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    );
    assert!(
        repository
            .transitions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
        vec![Err(ProviderErrorDto::unavailable(
            "provider_down",
            true,
            None,
        )
        .expect("error is valid"))],
    ]);
    driver.cancel_during_stream(1, signal.model_signal());
    let clock = ImmediateTime::new();

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
            .sleeps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
            .unwrap_or_else(PoisonError::into_inner)
            .len(),
        2
    );
    let finishes = repository
        .finishes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
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
        vec![Err(ProviderErrorDto::unavailable(
            "provider_down",
            true,
            None,
        )
        .expect("error is valid"))],
        vec![Err(ProviderErrorDto::unavailable(
            "provider_down",
            true,
            None,
        )
        .expect("error is valid"))],
    ]);
    let clock = ImmediateTime::new();

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
            .sleeps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|&&duration| duration == Duration::from_millis(250))
            .count(),
        1,
        "exactly one retry delay runs"
    );
    let finishes = repository
        .finishes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
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
    driver.stay_pending();
    let clock = ImmediateTime::new();

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
        clock
            .sleeps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_slice(),
        &[
            Duration::from_secs(30),
            Duration::from_millis(250),
            Duration::from_secs(30)
        ]
    );
    let finishes = repository
        .finishes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].error_code(), Some("provider_attempt_timed_out"));
    assert!(
        repository
            .messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty(),
        "a timed-out attempt commits no transcript row"
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
        .unwrap_or_else(PoisonError::into_inner)
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
        Err(ProviderErrorDto::unavailable("provider_down", true, None).expect("error is valid")),
    ]]);
    let clock = ImmediateTime::new();

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
            .sleeps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|&&duration| duration == Duration::from_millis(250))
            .count()
            == 0,
        "no retry delay runs once the text is committed"
    );
    let messages = repository
        .messages
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
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
            Err(ProviderErrorDto::unavailable("provider_down", true, None).expect("error is valid")),
        ],
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);
    let clock = ImmediateTime::new();

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
        clock
            .sleeps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_slice(),
        &[
            Duration::from_secs(30),
            Duration::from_millis(250),
            Duration::from_secs(30)
        ],
        "exactly one retry delay runs"
    );
    assert!(
        repository
            .messages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
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
        .unwrap_or_else(PoisonError::into_inner)
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
    let clock = ImmediateTime::new();
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
        .unwrap_or_else(PoisonError::into_inner)
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
    Vec<ModelRunCommitDto>,
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
    let clock = ImmediateTime::new();
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
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .map(|message| (message.kind(), message.text().to_owned()))
        .collect();
    let finishes = repository
        .finishes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .map(RecordedFinish::status)
        .collect();
    (messages, observer.commits(), finishes)
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
