#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "M4 domain contract fixtures use expect for precise diagnostics."
)]

use intention_domain::{
    ModelRunFactDto, ModelRunFactInputDto, ModelRunFactKindDto, ModelRunProjectionDto,
    RunEventCursorDto, RunFailureDto, RunProjectionDto, RunSnapshotDto, ToolResultOutcomeDto,
};
use intention_types::{
    AssistantTurnId, ConfigRevisionId, ErrorRetryDto, RunId, SessionEventSequenceDto, SessionId,
    ToolCallId, TurnId,
};

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
fn model_projection_and_snapshot_keep_safe_terminal_shapes() {
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

    assert_eq!(ModelRunFactKindDto::Finished.as_str(), "finished");
}

#[test]
fn model_fact_boundaries_and_projection_identity_validation_are_enforced() {
    assert!(ModelRunFactInputDto::retry_scheduled(0, 1).is_err());
    assert!(ModelRunFactInputDto::retry_scheduled(u16::MAX, 0).is_err());
    assert!(
        ModelRunFactInputDto::provider_attempt_failed(
            0,
            RunFailureDto::new("x", ErrorRetryDto::Never, None).unwrap()
        )
        .is_err()
    );
    let session = SessionId::new();
    let run_id = RunId::new();
    let other = RunId::new();
    let p = run(session, run_id);
    assert!(
        ModelRunProjectionDto::new(p, RunEventCursorDto::new(1), None, "text", None, None, None)
            .is_err()
    );
    let projection =
        ModelRunProjectionDto::new(p, RunEventCursorDto::new(1), None, "", None, None, None)
            .unwrap();
    assert!(
        RunSnapshotDto::new(session, other, SessionEventSequenceDto::new(1), projection).is_err()
    );
}

#[test]
fn model_fact_inputs_report_their_kinds() {
    let failure = RunFailureDto::new("failed", ErrorRetryDto::Never, None).expect("failure");
    assert!(
        ModelRunFactInputDto::usage_recorded(intention_types::UsageDto::NotReported).kind()
            == ModelRunFactKindDto::UsageRecorded
    );
    assert!(
        ModelRunFactInputDto::tool_call_recorded(
            intention_types::ToolCallDto::new(ToolCallId::new(), "tool", "{}").expect("call")
        )
        .kind()
            == ModelRunFactKindDto::ToolCallRecorded
    );
    assert_eq!(
        ModelRunFactInputDto::tool_result_recorded(
            ToolCallId::new(),
            ToolResultOutcomeDto::succeeded("done").expect("outcome"),
        )
        .expect("result fact")
        .kind(),
        ModelRunFactKindDto::ToolResultRecorded
    );
    assert_eq!(
        ModelRunFactInputDto::finished(intention_types::FinishReasonDto::Stop).kind(),
        ModelRunFactKindDto::Finished
    );
    assert_eq!(
        ModelRunFactInputDto::failed(failure).kind(),
        ModelRunFactKindDto::Failed
    );
}

#[test]
fn model_fact_wire_decoders_validate_each_constructor_and_reject_unknown_fields() {
    let failure = serde_json::json!({"code":"failed","retry":"never"});
    let cases = [
        serde_json::json!({"cursor":1,"kind":"provider_attempt_started","attempt":1}),
        serde_json::json!({"cursor":1,"kind":"provider_attempt_failed","attempt":1,"failure":failure}),
        serde_json::json!({"cursor":1,"kind":"retry_scheduled","failed_attempt":1,"next_attempt":2}),
        serde_json::json!({"cursor":1,"kind":"assistant_content_appended","assistant_turn_id":AssistantTurnId::new(),"content":"answer"}),
        serde_json::json!({"cursor":1,"kind":"reasoning_delta_recorded","content":"thought"}),
        serde_json::json!({"cursor":1,"kind":"usage_recorded","usage":{"state":"not_reported"}}),
        serde_json::json!({"cursor":1,"kind":"tool_call_recorded","call":{"call_id":ToolCallId::new(),"name":"inspect","arguments_json":"{}"}}),
        serde_json::json!({"cursor":1,"kind":"tool_result_recorded","call_id":ToolCallId::new(),"outcome":{"state":"succeeded","content":"ok"}}),
        serde_json::json!({"cursor":1,"kind":"finished","reason":"stop"}),
        serde_json::json!({"cursor":1,"kind":"failed","failure":{"code":"failed","retry":"never"}}),
    ];
    for value in cases {
        let fact: ModelRunFactDto = serde_json::from_value(value).expect("valid fact wire");
        assert_eq!(fact.cursor().value(), 1);
    }
    let unknown =
        serde_json::json!({"cursor":1,"kind":"provider_attempt_started","attempt":1,"extra":true});
    assert!(serde_json::from_value::<ModelRunFactDto>(unknown).is_ok());
    assert!(
        serde_json::from_value::<RunFailureDto>(
            serde_json::json!({"code":"x","retry":"never","extra":true})
        )
        .is_err()
    );
}
