#![allow(
    clippy::expect_used,
    reason = "Execution fixtures use expect to provide precise failures."
)]

mod common;

use std::time::Duration;

use common::{FakeRepository, ImmediateTime, RecordingCommitObserver};
use intention_config::ConfigSnapshotDto;
use intention_engine::{
    ModelRunCommitDto, ModelRunExecutionInputDto, ModelRunExecutionOutcomeDto,
    ModelRunExecutionService, RunCancellation, ToolExecutionPort, ToolResultOutcomeDto,
};
use intention_proto::{DtoResult, ErrorDto, RunId, SessionId};
use intention_proto::{MessageKindDto, MessageProjectionDto, RunProjectionDto, RunStatusDto};
use intention_providers::{
    FinishReasonDto, ModelEventDto, ModelMessageDto, ModelRequestDto, ModelRoleDto,
    ProviderErrorDto, ToolCallDto, UsageDto,
};
use intention_storage::TransitionRunInputDto;
use intention_test_support::{ScriptedDriver, fixture_snapshot_with_model, run_ready};

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
    assert_eq!(repository.messages.borrow().len(), 1);
}

#[test]
fn observer_receives_no_content_when_a_message_commit_fails() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot_with_model("fixture");
    let repository = FakeRepository::new(session_id, run_id, config.clone());
    *repository.append_failure.borrow_mut() = Some(ErrorDto::unavailable(
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
    assert!(repository.messages.borrow().is_empty());
    assert!(repository.finishes.borrow().is_empty());
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
    let messages = repository.messages.borrow();
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
    let finishes = repository.finishes.borrow();
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
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
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
            repository.messages.borrow().is_empty(),
            "an invalid stream never commits assistant content"
        );
        let finishes = repository.finishes.borrow();
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
    let messages = repository.messages.borrow();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![
            (MessageKindDto::Assistant, "partial answer"),
            (MessageKindDto::Notice, intention_engine::INTERRUPT_NOTICE),
            (MessageKindDto::Assistant, "final answer"),
        ],
        "the stopped step commits its text and the durable notice"
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
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1, "one terminal outcome commits");
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
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
    let messages = repository.messages.borrow();
    assert_eq!(
        messages
            .iter()
            .map(|message| (message.kind(), message.text()))
            .collect::<Vec<_>>(),
        vec![(MessageKindDto::Notice, intention_engine::INTERRUPT_NOTICE)],
        "an interrupted step without text commits only its notice"
    );
    drop(messages);
    let finishes = repository.finishes.borrow();
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
    *repository.append_failure_at.borrow_mut() = Some((
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
    assert_eq!(repository.messages.borrow().len(), 1);
    assert!(repository.finishes.borrow().is_empty());
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
            .borrow()
            .iter()
            .filter(|&&duration| duration == Duration::from_millis(250))
            .count(),
        1
    );
    assert_eq!(
        repository
            .transitions
            .borrow()
            .iter()
            .map(TransitionRunInputDto::status)
            .collect::<Vec<_>>(),
        vec![RunStatusDto::Running]
    );
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Completed);
    assert!(
        repository.messages.borrow().is_empty(),
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
    let finishes = repository.finishes.borrow();
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
    *repository.config_error.borrow_mut() = Some(ErrorDto::unavailable(
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
    let finishes = repository.finishes.borrow();
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
    let finishes = repository.finishes.borrow();
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
    *repository.status.borrow_mut() = RunStatusDto::Running;
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
    assert!(repository.messages.borrow().is_empty());
    assert!(repository.finishes.borrow().is_empty());
    assert!(repository.transitions.borrow().is_empty());
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
            .borrow()
            .iter()
            .filter(|&&duration| duration == Duration::from_millis(250))
            .count(),
        0,
        "no retry delay runs after committed content"
    );
    assert_eq!(repository.messages.borrow().len(), 2);
    let finishes = repository.finishes.borrow();
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
            .borrow()
            .iter()
            .filter(|&&duration| duration == Duration::from_millis(250))
            .count(),
        1,
        "exactly one retry delay runs"
    );
    let finishes = repository.finishes.borrow();
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
        clock.sleeps.borrow().as_slice(),
        &[
            Duration::from_secs(30),
            Duration::from_millis(250),
            Duration::from_secs(30)
        ]
    );
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].error_code(), Some("provider_attempt_timed_out"));
    assert!(
        repository.messages.borrow().is_empty(),
        "a timed-out attempt commits no transcript row"
    );
}
