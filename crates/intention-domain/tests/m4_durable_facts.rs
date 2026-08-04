#![allow(
    clippy::expect_used,
    reason = "M4 domain contract fixtures use expect for precise diagnostics."
)]

use intention_domain::{
    DomainEventDto, ModelRunFactDto, ModelRunFactEventDto, ModelRunFactInputDto,
    ModelRunFactKindDto, ModelRunProjectionDto, RunEventCursorDto, RunFailureDto, RunProjectionDto,
    RunSnapshotDto,
};
use intention_types::{
    AssistantTurnId, ConfigRevisionId, ErrorRetryDto, RunId, SessionEventSequenceDto, SessionId,
    TimestampDto, TurnId,
};

fn time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1).expect("fixture time is valid")
}

fn run(session_id: SessionId, run_id: RunId) -> RunProjectionDto {
    RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        intention_domain::RunStatusDto::Running,
        ConfigRevisionId::new(),
    )
}

#[test]
fn model_fact_inputs_validate_durable_ordering_and_safe_payload_bounds() {
    assert_eq!(RunEventCursorDto::new(0).value(), 0);
    assert!(ModelRunFactInputDto::provider_attempt_started(0).is_err());
    assert!(
        ModelRunFactInputDto::provider_attempt_failed(
            2,
            RunFailureDto::new("provider_unavailable", ErrorRetryDto::Delayed, None)
                .expect("safe failure is valid"),
        )
        .is_ok()
    );
    assert!(ModelRunFactInputDto::retry_scheduled(2, 2).is_err());
    assert!(ModelRunFactInputDto::assistant_content_appended(AssistantTurnId::new(), " ").is_err());
    assert!(
        ModelRunFactInputDto::assistant_content_appended(
            AssistantTurnId::new(),
            "x".repeat(4 * 1024 + 1),
        )
        .is_err()
    );
    assert!(ModelRunFactInputDto::reasoning_delta_recorded(" ").is_err());
}

#[test]
fn model_projection_snapshot_and_events_keep_safe_terminal_and_tail_only_shapes() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let assistant_turn_id = AssistantTurnId::new();
    let projection = ModelRunProjectionDto::new(
        run(session_id, run_id),
        RunEventCursorDto::new(3),
        Some(assistant_turn_id),
        "answer",
        None,
        None,
        None,
    )
    .expect("projection is coherent");
    assert_eq!(projection.assistant_content(), "answer");
    assert!(projection.reasoning_content().is_none());
    assert!(
        ModelRunProjectionDto::new(
            run(session_id, run_id),
            RunEventCursorDto::new(3),
            None,
            "",
            None,
            Some(intention_types::FinishReasonDto::Stop),
            Some(
                RunFailureDto::new("provider_failed", ErrorRetryDto::Never, None)
                    .expect("safe failure is valid"),
            ),
        )
        .is_err()
    );
    let snapshot = RunSnapshotDto::new(
        session_id,
        run_id,
        SessionEventSequenceDto::new(9),
        projection,
    )
    .expect("safe snapshot is coherent");
    assert_eq!(snapshot.cursor().value(), 3);
    assert_eq!(snapshot.run_projection().run_id(), run_id);

    let fact = ModelRunFactDto::new(
        RunEventCursorDto::new(1),
        ModelRunFactInputDto::provider_attempt_started(1).expect("attempt is valid"),
    )
    .expect("cursor-assigned fact is valid");
    let event = DomainEventDto::ProviderAttemptStarted(ModelRunFactEventDto::new(
        session_id,
        run_id,
        fact,
        time(),
    ));
    assert!(matches!(event, DomainEventDto::ProviderAttemptStarted(_)));
    assert_eq!(ModelRunFactKindDto::Finished.as_str(), "finished");
}
