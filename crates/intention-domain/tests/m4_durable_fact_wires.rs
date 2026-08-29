#![allow(
    clippy::expect_used,
    reason = "M4 domain wire fixtures use expect for precise diagnostics."
)]

use intention_domain::{
    ModelRunFactDto, ModelRunFactInputDto, ModelRunProjectionDto, RunEventCursorDto,
    RunEventTailPageDto, RunFailureDto, RunReplayDto, RunSnapshotDto, ToolResultOutcomeDto,
};
use intention_types::{
    AssistantTurnId, ConfigRevisionId, CorrelationIdDto, FinishReasonDto, ProviderErrorDto, RunId,
    SessionEventSequenceDto, SessionId, ToolCallDto, ToolCallId, TurnId, UsageDto,
};

fn projection(session_id: SessionId, run_id: RunId) -> ModelRunProjectionDto {
    ModelRunProjectionDto::new(
        intention_domain::RunProjectionDto::new(
            session_id,
            run_id,
            TurnId::new(),
            intention_domain::RunStatusDto::Running,
            ConfigRevisionId::new(),
        ),
        RunEventCursorDto::new(2),
        Some(AssistantTurnId::new()),
        "answer",
        Some(UsageDto::reported(2, 3, 5).expect("usage is valid")),
        Some(FinishReasonDto::Stop),
        None,
    )
    .expect("projection is valid")
}

#[test]
fn model_fact_inputs_and_assigned_facts_cover_every_typed_variant_and_wire_validation() {
    let correlation = CorrelationIdDto::new();
    let failure = RunFailureDto::from_provider(
        ProviderErrorDto::unavailable("provider_unavailable", true, Some(correlation))
            .expect("provider error is valid"),
    );
    for kind in [
        intention_domain::ModelRunFactKindDto::ProviderAttemptStarted,
        intention_domain::ModelRunFactKindDto::ProviderAttemptFailed,
        intention_domain::ModelRunFactKindDto::RetryScheduled,
        intention_domain::ModelRunFactKindDto::AssistantContentAppended,
        intention_domain::ModelRunFactKindDto::ReasoningDeltaRecorded,
        intention_domain::ModelRunFactKindDto::UsageRecorded,
        intention_domain::ModelRunFactKindDto::ToolCallRecorded,
        intention_domain::ModelRunFactKindDto::ToolResultRecorded,
        intention_domain::ModelRunFactKindDto::Finished,
        intention_domain::ModelRunFactKindDto::Failed,
    ] {
        assert!(!kind.as_str().is_empty());
    }
    assert_eq!(
        intention_domain::ModelRunFactKindDto::ToolResultRecorded.as_str(),
        "tool_result_recorded"
    );
    assert_eq!(failure.code(), "provider_unavailable");
    assert_eq!(failure.retry(), intention_types::ErrorRetryDto::Delayed);
    assert_eq!(failure.correlation_id(), Some(correlation));
    let explicit_failure = RunFailureDto::new(
        "explicit_failure",
        intention_types::ErrorRetryDto::Never,
        None,
    )
    .expect("explicit failure is valid");
    assert_eq!(explicit_failure.code(), "explicit_failure");
    assert_eq!(
        explicit_failure.retry(),
        intention_types::ErrorRetryDto::Never
    );
    assert!(RunFailureDto::new(" ", intention_types::ErrorRetryDto::Never, None).is_err());

    let inputs = vec![
        ModelRunFactInputDto::provider_attempt_started(1).expect("start is valid"),
        ModelRunFactInputDto::provider_attempt_failed(1, failure.clone())
            .expect("failure is valid"),
        ModelRunFactInputDto::retry_scheduled(1, 2).expect("retry is valid"),
        ModelRunFactInputDto::assistant_content_appended(AssistantTurnId::new(), "text")
            .expect("content is valid"),
        ModelRunFactInputDto::reasoning_delta_recorded("reasoning").expect("reasoning is valid"),
        ModelRunFactInputDto::usage_recorded(UsageDto::NotReported),
        ModelRunFactInputDto::tool_call_recorded(
            ToolCallDto::new(ToolCallId::new(), "inspect", "{}").expect("tool call is valid"),
        ),
        ModelRunFactInputDto::tool_result_recorded(
            ToolCallId::new(),
            ToolResultOutcomeDto::succeeded("done").expect("outcome is valid"),
        )
        .expect("tool result fact is valid"),
        ModelRunFactInputDto::finished(FinishReasonDto::Length),
        ModelRunFactInputDto::failed(failure),
    ];
    for (index, input) in inputs.into_iter().enumerate() {
        let fact = ModelRunFactDto::new(RunEventCursorDto::new(index as u64 + 1), input)
            .expect("assigned fact is valid");
        let wire = serde_json::to_string(&fact).expect("fact serializes");
        let decoded: ModelRunFactDto = serde_json::from_str(&wire).expect("fact deserializes");
        assert_eq!(decoded, fact);
    }
    assert!(
        ModelRunFactDto::new(
            RunEventCursorDto::new(0),
            ModelRunFactInputDto::finished(FinishReasonDto::Stop),
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<ModelRunFactInputDto>(
            r#"{"kind":"provider_attempt_started","attempt":0}"#
        )
        .is_err()
    );
}

#[test]
fn snapshots_tails_and_replays_reject_mismatches_and_round_trip() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let projection_value = projection(session_id, run_id);
    assert!(projection_value.assistant_turn_id().is_some());
    assert_eq!(
        projection_value.usage(),
        Some(UsageDto::reported(2, 3, 5).expect("usage is valid"))
    );
    assert_eq!(
        projection_value.finish_reason(),
        Some(FinishReasonDto::Stop)
    );
    assert_eq!(projection_value.failure(), None);
    let snapshot = RunSnapshotDto::new(
        session_id,
        run_id,
        SessionEventSequenceDto::new(8),
        projection_value,
    )
    .expect("snapshot is valid");
    assert_eq!(snapshot.projection().cursor().value(), 2);
    let first = ModelRunFactDto::new(
        RunEventCursorDto::new(3),
        ModelRunFactInputDto::usage_recorded(UsageDto::NotReported),
    )
    .expect("fact is valid");
    let tail = RunEventTailPageDto::new(
        session_id,
        run_id,
        RunEventCursorDto::new(2),
        vec![first],
        RunEventCursorDto::new(3),
        false,
    )
    .expect("tail is valid");
    let event = intention_domain::ModelRunFactEventDto::new(
        session_id,
        run_id,
        ModelRunFactDto::new(
            RunEventCursorDto::new(1),
            ModelRunFactInputDto::finished(FinishReasonDto::Stop),
        )
        .expect("event fact is valid"),
        intention_types::TimestampDto::from_unix_seconds(1).expect("time is valid"),
    );
    assert_eq!(event.session_id(), session_id);
    assert_eq!(event.run_id(), run_id);
    assert_eq!(event.occurred_at().unix_seconds(), 1);
    assert!(
        RunSnapshotDto::new(
            session_id,
            RunId::new(),
            SessionEventSequenceDto::new(8),
            projection(session_id, run_id),
        )
        .is_err()
    );
    let snapshot_tail = RunEventTailPageDto::empty(session_id, run_id, snapshot.cursor());
    let replay = RunReplayDto::new(snapshot.clone(), snapshot_tail).expect("replay is valid");
    let decoded: RunReplayDto =
        serde_json::from_str(&serde_json::to_string(&replay).expect("replay serializes"))
            .expect("replay deserializes");
    assert_eq!(decoded, replay);
    assert!(
        RunEventTailPageDto::new(
            session_id,
            run_id,
            RunEventCursorDto::new(2),
            vec![tail.facts()[0].clone()],
            RunEventCursorDto::new(4),
            false,
        )
        .is_err()
    );
    assert!(
        RunEventTailPageDto::new(
            session_id,
            run_id,
            RunEventCursorDto::new(2),
            (0..257)
                .map(|index| {
                    ModelRunFactDto::new(
                        RunEventCursorDto::new(index + 3),
                        ModelRunFactInputDto::usage_recorded(UsageDto::NotReported),
                    )
                    .expect("bounded fact is valid")
                })
                .collect(),
            RunEventCursorDto::new(259),
            false,
        )
        .is_err()
    );
    let discontinuous = ModelRunFactDto::new(
        RunEventCursorDto::new(4),
        ModelRunFactInputDto::usage_recorded(UsageDto::NotReported),
    )
    .expect("fact is valid");
    assert!(
        RunEventTailPageDto::new(
            session_id,
            run_id,
            RunEventCursorDto::new(2),
            vec![discontinuous],
            RunEventCursorDto::new(4),
            false,
        )
        .is_err()
    );
    assert!(
        RunEventTailPageDto::new(
            session_id,
            run_id,
            RunEventCursorDto::new(u64::MAX),
            Vec::new(),
            RunEventCursorDto::new(u64::MAX),
            false,
        )
        .is_ok()
    );
    let overflowing = ModelRunFactDto::new(
        RunEventCursorDto::new(1),
        ModelRunFactInputDto::usage_recorded(UsageDto::NotReported),
    )
    .expect("fact is valid");
    assert!(
        RunEventTailPageDto::new(
            session_id,
            run_id,
            RunEventCursorDto::new(u64::MAX),
            vec![overflowing],
            RunEventCursorDto::new(u64::MAX),
            false,
        )
        .is_err()
    );
    let mismatched_tail = RunEventTailPageDto::empty(session_id, RunId::new(), snapshot.cursor());
    assert!(RunReplayDto::new(snapshot, mismatched_tail).is_err());
}

#[test]
fn tool_result_fact_wire_shape_and_outcome_bounds_are_validated() {
    let outcome = ToolResultOutcomeDto::succeeded("done").expect("outcome is valid");
    assert!(ToolResultOutcomeDto::succeeded(" ").is_err());
    assert!(
        ToolResultOutcomeDto::succeeded("x".repeat(4 * 1024 + 1)).is_err(),
        "tool result content must not exceed 4 KiB"
    );
    let decoded: ToolResultOutcomeDto =
        serde_json::from_str(&serde_json::to_string(&outcome).expect("outcome serializes"))
            .expect("succeeded outcome deserializes");
    assert_eq!(decoded, outcome);
    let failed = ToolResultOutcomeDto::failed(
        RunFailureDto::new("tool_failed", intention_types::ErrorRetryDto::Never, None)
            .expect("failure is valid"),
    );
    let decoded: ToolResultOutcomeDto =
        serde_json::from_str(&serde_json::to_string(&failed).expect("failed outcome serializes"))
            .expect("failed outcome deserializes");
    assert_eq!(decoded, failed);

    let call_id = ToolCallId::new();
    let input = ModelRunFactInputDto::tool_result_recorded(call_id, outcome)
        .expect("tool result fact is valid");
    assert_eq!(
        input.kind(),
        intention_domain::ModelRunFactKindDto::ToolResultRecorded
    );
    assert_eq!(
        serde_json::to_value(&input).expect("fact serializes"),
        serde_json::json!({
            "kind": "tool_result_recorded",
            "call_id": call_id,
            "outcome": {"state": "succeeded", "content": "done"}
        })
    );
    let fact =
        ModelRunFactDto::new(RunEventCursorDto::new(1), input).expect("assigned fact is valid");
    let decoded: ModelRunFactDto =
        serde_json::from_str(&serde_json::to_string(&fact).expect("fact serializes"))
            .expect("fact deserializes");
    assert_eq!(decoded, fact);

    let legacy: ModelRunFactDto = serde_json::from_value(serde_json::json!({
        "cursor": 1,
        "kind": "tool_call_recorded",
        "call": {"call_id": call_id, "name": "inspect", "arguments_json": "{}"}
    }))
    .expect("legacy tool-call wire still decodes");
    assert_eq!(
        legacy.kind(),
        intention_domain::ModelRunFactKindDto::ToolCallRecorded
    );
}
