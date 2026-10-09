#![allow(
    clippy::expect_used,
    reason = "Execution fixtures use expect to provide precise failures."
)]

mod common;

use std::sync::PoisonError;
use std::time::Duration;

use common::{FakeRepository, ImmediateTime, RecordedTransition, RecordingCommitObserver};
use intention_engine::{
    ModelRunCommitDto, ModelRunExecutionInputDto, ModelRunExecutionOutcomeDto,
    ModelRunExecutionService, RunCancellation, ToolExecutionPort, ToolResultOutcomeDto,
};
use intention_proto::provider::ReasoningFragmentCategoryDto;
use intention_proto::{DtoResult, ErrorDto, RunId, SessionId};
use intention_proto::{MessageKindDto, MessageProjectionDto, RunProjectionDto, RunStatusDto};
use intention_providers::{
    FinishReasonDto, ModelEventDto, ModelMessageDto, ModelRequestDto, ModelRoleDto,
    ProviderErrorDto, ToolCallDto, UsageDto,
};
use intention_test_support::{
    ScriptedDriver, fixture_selection, fixture_snapshot, fixture_snapshot_with_context_window,
    run_ready,
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

fn execute(
    repository: &FakeRepository,
    driver: &ScriptedDriver,
    request: ModelRequestDto,
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
                repository.selection(),
                signal,
            )),
    )
}

#[test]
fn observer_receives_only_committed_transcript_rows_and_statuses() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
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
            repository.selection(),
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
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
            repository.selection(),
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
    let content = format!("{}€tail", "a".repeat(4094));
    let driver = ScriptedDriver::new(vec![
        Ok(ModelEventDto::started()),
        Ok(ModelEventDto::text_delta(content.clone()).expect("text is valid")),
        Ok(
            ModelEventDto::reasoning_delta(ReasoningFragmentCategoryDto::Primary, "why")
                .expect("reasoning is valid"),
        ),
        Ok(ModelEventDto::usage(
            UsageDto::reported(1, 2, 3).expect("usage is valid"),
        )),
        Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
    ]);
    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
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
        let config = fixture_snapshot();
        let repository = FakeRepository::new(session_id, run_id, config.clone());
        let driver = ScriptedDriver::new(events);
        let outcome = execute(
            &repository,
            &driver,
            request(run_id, "fixture"),
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
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
    let outcome = execute(&repository, &driver, request(run_id, "fixture"), signal)
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
    let signal = RunCancellation::new();
    signal.cancel();
    let driver = ScriptedDriver::with_rounds(vec![
        Vec::new(),
        vec![
            Ok(ModelEventDto::started()),
            Ok(ModelEventDto::finished(FinishReasonDto::Stop)),
        ],
    ]);

    let outcome = execute(&repository, &driver, request(run_id, "fixture"), signal)
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
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

    let error = execute(&repository, &driver, request(run_id, "fixture"), signal)
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
    let driver = ScriptedDriver::with_rounds(vec![
        vec![
            Ok(ModelEventDto::started()),
            Ok(
                ModelEventDto::reasoning_delta(ReasoningFragmentCategoryDto::Primary, "why")
                    .expect("reasoning is valid"),
            ),
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
            repository.selection(),
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
    // A persisted selection that disagrees with the requested selection is
    // a safe terminal failure with no provider work.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let repository = FakeRepository::with_selection(
        session_id,
        run_id,
        fixture_snapshot(),
        fixture_selection("fixture-openrouter", "openrouter", "persisted")
            .expect("the fixture selection is valid"),
    );
    let driver = ScriptedDriver::new(Vec::new());
    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "current"),
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
    let driver = ScriptedDriver::new(Vec::new());
    let outcome = execute(
        &repository,
        &driver,
        request(RunId::new(), "fixture"),
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
fn execution_requires_the_exact_persisted_selection_and_never_a_model_match() {
    // The persisted selection is verified exactly: a run whose requested model
    // matches but whose exact selection differs is never rerouted.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
    let persisted = repository.selection();
    let requested = intention_test_support::fixture_selection(
        "another-profile",
        persisted.kind_id().as_str(),
        persisted.model_id(),
    )
    .expect("the fixture selection is valid");
    assert_eq!(requested.model_id(), persisted.model_id());
    assert_ne!(requested, persisted);

    let driver = ScriptedDriver::new(Vec::new());
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
            requested,
            RunCancellation::new(),
        )),
    )
    .expect("a selection mismatch safely fails");
    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("a selection mismatch fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_configuration_unavailable");
    assert_eq!(
        driver.executions(),
        0,
        "the mismatch is rejected before any provider work"
    );
    drop(repository);

    // A run that carries no persisted selection fails the same way.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let repository = FakeRepository::new(session_id, run_id, fixture_snapshot());
    let requested = repository.selection();
    repository.clear_selection();
    let driver = ScriptedDriver::new(Vec::new());
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
            requested,
            RunCancellation::new(),
        )),
    )
    .expect("an absent persisted selection becomes a durable safe failure");
    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("an absent persisted selection fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_configuration_unavailable");
    assert_eq!(driver.executions(), 0);
}

#[test]
fn execution_rejects_non_starting_run_before_configuration_or_provider_work() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
    *repository
        .status
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = RunStatusDto::Running;
    let driver = ScriptedDriver::new(Vec::new());

    let error = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
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
            repository.selection(),
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
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
            repository.selection(),
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
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
            repository.selection(),
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
    let driver = ScriptedDriver::completed_text();

    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
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
            repository.selection(),
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
    let config = fixture_snapshot();
    let repository = FakeRepository::new(session_id, run_id, config);
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
            repository.selection(),
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
fn the_run_owns_its_window_policy_and_the_input_cannot_supply_one() {
    // The window policy now comes from the run's own committed configuration
    // revision, and the execution input carries no window at all: a caller
    // cannot supply, override, or disagree with it. A run whose committed
    // revision cannot be read therefore fails before any provider work.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let repository = FakeRepository::new(
        session_id,
        run_id,
        fixture_snapshot_with_context_window(Some(60)),
    );
    *repository
        .config_error
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(ErrorDto::unavailable(
        "configuration_not_found",
        "the run's committed configuration revision is unavailable",
    ));
    let driver = ScriptedDriver::new(Vec::new());

    let outcome = execute(
        &repository,
        &driver,
        request(run_id, "fixture"),
        RunCancellation::new(),
    )
    .expect("an unreadable window policy safely fails");

    let ModelRunExecutionOutcomeDto::Failed { run, error } = outcome else {
        unreachable!("an unreadable window policy fails the run");
    };
    assert_eq!(run.status(), RunStatusDto::Failed);
    assert_eq!(error.code(), "provider_configuration_unavailable");
    assert_eq!(
        driver.executions(),
        0,
        "the missing window policy is rejected before any provider work"
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
