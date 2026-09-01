#![allow(
    clippy::expect_used,
    reason = "Run-stream protocol fixtures use expect for precise diagnostics."
)]

use intention_domain::{
    ModelRunFactDto, ModelRunFactInputDto, ModelRunProjectionDto, RunEventCursorDto,
    RunEventTailPageDto, RunProjectionDto, RunSnapshotDto,
};
use intention_protocol::{
    ProtocolCapabilityDto, RunLiveBatchDto, RunResyncDto, RunResyncReasonDto, RunSnapshotFrameDto,
    RunStreamFrameDto, RunSubscriptionResponseDto, SubscribeRunCommandDto,
};
use intention_types::{
    ConfigRevisionId, RunId, SchemaVersionDto, SessionEventSequenceDto, SessionId, TurnId,
};

fn snapshot(session_id: SessionId, run_id: RunId, cursor: u64) -> RunSnapshotDto {
    let projection = ModelRunProjectionDto::new(
        RunProjectionDto::new(
            session_id,
            run_id,
            TurnId::new(),
            intention_domain::RunStatusDto::Running,
            ConfigRevisionId::new(),
        ),
        RunEventCursorDto::new(cursor),
        None,
        "",
        None,
        None,
        None,
    )
    .expect("fixture model projection is valid");
    RunSnapshotDto::new(
        session_id,
        run_id,
        SessionEventSequenceDto::new(3),
        projection,
    )
    .expect("fixture run snapshot is valid")
}

fn fact(cursor: u64) -> ModelRunFactDto {
    ModelRunFactDto::new(
        RunEventCursorDto::new(cursor),
        ModelRunFactInputDto::provider_attempt_started(1).expect("fixture attempt is valid"),
    )
    .expect("fixture fact is valid")
}

#[test]
fn run_stream_dtos_round_trip_and_preserve_all_resync_reasons() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let subscription = SubscribeRunCommandDto::new(
        SchemaVersionDto::new(1, 1),
        session_id,
        run_id,
        Some(RunEventCursorDto::new(2)),
    );
    let live = RunLiveBatchDto::new(
        session_id,
        run_id,
        RunEventCursorDto::new(2),
        vec![fact(3)],
        RunEventCursorDto::new(3),
    )
    .expect("contiguous batch is valid");
    let replay = intention_domain::RunReplayDto::new(
        snapshot(session_id, run_id, 2),
        RunEventTailPageDto::new(
            session_id,
            run_id,
            RunEventCursorDto::new(2),
            vec![fact(3)],
            RunEventCursorDto::new(3),
            false,
        )
        .expect("replay tail is contiguous"),
    )
    .expect("replay is coherent");
    let subscription_value = serde_json::to_value(subscription).expect("subscription serializes");
    let replay_value = serde_json::to_value(RunSubscriptionResponseDto::Replay(replay))
        .expect("replay serializes");
    let live_value =
        serde_json::to_value(RunStreamFrameDto::LiveBatch(live)).expect("batch serializes");
    let snapshot_value = serde_json::to_value(RunStreamFrameDto::Snapshot(
        RunSnapshotFrameDto::new(snapshot(session_id, run_id, 2)),
    ))
    .expect("snapshot frame serializes");
    assert_eq!(
        serde_json::from_value::<SubscribeRunCommandDto>(subscription_value)
            .expect("subscription deserializes"),
        subscription
    );
    let _: RunSubscriptionResponseDto =
        serde_json::from_value(replay_value).expect("replay deserializes");
    let _: RunStreamFrameDto = serde_json::from_value(live_value).expect("batch deserializes");
    let _: RunStreamFrameDto =
        serde_json::from_value(snapshot_value).expect("snapshot frame deserializes");
    for reason in [
        RunResyncReasonDto::HistoryUnavailable,
        RunResyncReasonDto::InvalidCursor,
        RunResyncReasonDto::CursorGap,
        RunResyncReasonDto::SubscriberTooSlow,
    ] {
        let resync = RunResyncDto::new(session_id, run_id, reason);
        assert_eq!(
            serde_json::from_value::<RunResyncDto>(
                serde_json::to_value(resync).expect("resync serializes"),
            )
            .expect("resync deserializes"),
            resync
        );
    }
    assert!(serde_json::from_str::<RunResyncReasonDto>("\"future_reason\"").is_err());
    assert_eq!(
        ProtocolCapabilityDto::RunStreamSubscriptions,
        ProtocolCapabilityDto::RunStreamSubscriptions
    );
}

#[test]
fn run_live_batches_reject_empty_gapped_and_invalid_continuations_without_scope_leaks() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    for (facts, next) in [
        (Vec::new(), 0),
        (vec![fact(2)], 2),
        (vec![fact(1), fact(3)], 3),
    ] {
        assert_eq!(
            RunLiveBatchDto::new(
                session_id,
                run_id,
                RunEventCursorDto::new(0),
                facts,
                RunEventCursorDto::new(next),
            )
            .expect_err("invalid batch is rejected")
            .code(),
            "invalid_run_live_batch"
        );
    }
    let value = serde_json::json!({
        "session_id": session_id,
        "run_id": run_id,
        "after_cursor": 0,
        "facts": [{"cursor": 1, "kind": "provider_attempt_started", "attempt": 1}],
        "next_after_cursor": 1,
        "unknown_additive": true,
    });
    assert!(serde_json::from_value::<RunLiveBatchDto>(value).is_ok());
}
