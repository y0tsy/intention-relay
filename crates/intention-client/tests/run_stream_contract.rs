#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Run-stream client fixtures use direct assertions for diagnostics."
)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use intention_client::{RunStreamClient, RunSubscriptionReducer};
use intention_domain::{
    ModelRunFactDto, ModelRunFactInputDto, ModelRunProjectionDto, RunEventCursorDto,
    RunEventTailPageDto, RunProjectionDto, RunSnapshotDto,
};
use intention_protocol::{
    ProtocolCapabilityDto, ProtocolDaemonFrameDto, ProtocolHelloDto, ProtocolMessageDto,
    ProtocolResponseEnvelopeDto, ProtocolResponsePayloadDto, RunLiveBatchDto, RunResyncDto,
    RunResyncReasonDto, RunSnapshotFrameDto, RunStreamFrameDto, RunSubscriptionResponseDto,
    SubscribeRunCommandDto,
};
use intention_transport::{AsyncLocalListener, LocalEndpoint, local_protocol_version};
use intention_types::{
    ConfigRevisionId, CorrelationIdDto, ErrorDto, RunId, SchemaVersionDto, SessionEventSequenceDto,
    SessionId, TurnId,
};

const SCHEMA: SchemaVersionDto = intention_protocol::CURRENT_DTO_SCHEMA_VERSION;
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);

fn endpoint() -> LocalEndpoint {
    let sequence = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time follows epoch")
        .as_nanos();
    LocalEndpoint::from_instance_id(format!("run-stream-fixture-{nanos}-{sequence}"))
        .expect("fixture endpoint is valid")
}

fn hello(name: &str) -> ProtocolHelloDto {
    ProtocolHelloDto::new(
        local_protocol_version(),
        vec![ProtocolCapabilityDto::RunStreamSubscriptions],
        name,
    )
    .expect("fixture hello is valid")
}

fn snapshot(
    session_id: SessionId,
    run_id: RunId,
    cursor: u64,
    status: intention_domain::RunStatusDto,
) -> RunSnapshotDto {
    RunSnapshotDto::new(
        session_id,
        run_id,
        SessionEventSequenceDto::new(5),
        ModelRunProjectionDto::new(
            RunProjectionDto::new(
                session_id,
                run_id,
                TurnId::new(),
                status,
                ConfigRevisionId::new(),
            ),
            RunEventCursorDto::new(cursor),
            None,
            "",
            None,
            None,
            None,
        )
        .expect("projection is valid"),
    )
    .expect("snapshot is valid")
}

fn fact(cursor: u64, reasoning: Option<&str>) -> ModelRunFactDto {
    let input = reasoning.map_or_else(
        || ModelRunFactInputDto::provider_attempt_started(1).expect("attempt is valid"),
        |value| {
            ModelRunFactInputDto::reasoning_delta_recorded(value)
                .expect("reasoning fixture is valid")
        },
    );
    ModelRunFactDto::new(RunEventCursorDto::new(cursor), input).expect("fact is valid")
}

fn replay_with_tail(
    session_id: SessionId,
    run_id: RunId,
    snapshot_cursor: u64,
    tail: Vec<ModelRunFactDto>,
) -> RunSubscriptionResponseDto {
    let snapshot = snapshot(
        session_id,
        run_id,
        snapshot_cursor,
        intention_domain::RunStatusDto::Running,
    );
    let next_cursor = tail
        .last()
        .map_or(snapshot_cursor, |fact| fact.cursor().value());
    RunSubscriptionResponseDto::Replay(
        intention_domain::RunReplayDto::new(
            snapshot,
            RunEventTailPageDto::new(
                session_id,
                run_id,
                RunEventCursorDto::new(snapshot_cursor),
                tail,
                RunEventCursorDto::new(next_cursor),
                false,
            )
            .expect("replay tail is coherent"),
        )
        .expect("replay is coherent"),
    )
}

fn replay(session_id: SessionId, run_id: RunId, cursor: u64) -> RunSubscriptionResponseDto {
    replay_with_tail(session_id, run_id, cursor, Vec::new())
}

#[test]
fn reducer_handles_replay_duplicates_gaps_wrong_scope_resync_history_and_snapshot_status() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut reducer = RunSubscriptionReducer::new(session_id, run_id);
    reducer
        .apply_initial(replay(session_id, run_id, 2))
        .expect("replay applies");
    assert_eq!(reducer.last_cursor(), Some(RunEventCursorDto::new(2)));
    assert!(
        reducer
            .apply_live_batch(
                RunLiveBatchDto::new(
                    session_id,
                    run_id,
                    RunEventCursorDto::new(1),
                    vec![fact(2, None)],
                    RunEventCursorDto::new(2)
                )
                .expect("stale batch validates")
            )
            .expect("stale batch applies")
            .is_none()
    );
    assert!(
        reducer
            .apply_live_batch(
                RunLiveBatchDto::new(
                    session_id,
                    run_id,
                    RunEventCursorDto::new(3),
                    vec![fact(4, None)],
                    RunEventCursorDto::new(4)
                )
                .expect("gapped batch validates")
            )
            .expect("gap produces resync")
            .is_some()
    );
    assert_eq!(reducer.last_cursor(), Some(RunEventCursorDto::new(2)));
    assert_eq!(
        reducer
            .apply_frame(RunStreamFrameDto::Resync(RunResyncDto::new(
                SessionId::new(),
                run_id,
                RunResyncReasonDto::InvalidCursor
            )))
            .expect_err("wrong scope rejects")
            .code(),
        "invalid_run_subscription"
    );
    reducer
        .apply_frame(RunStreamFrameDto::Snapshot(RunSnapshotFrameDto::new(
            snapshot(
                session_id,
                run_id,
                2,
                intention_domain::RunStatusDto::Completed,
            ),
        )))
        .expect("snapshot frame applies");
    assert_eq!(
        reducer
            .snapshot()
            .expect("snapshot exists")
            .run_projection()
            .status(),
        intention_domain::RunStatusDto::Completed
    );
    reducer
        .apply_initial(RunSubscriptionResponseDto::Resync(RunResyncDto::new(
            session_id,
            run_id,
            RunResyncReasonDto::HistoryUnavailable,
        )))
        .expect("history resync applies");
    assert!(reducer.snapshot().is_none());
    assert!(reducer.history_unavailable());
    assert!(
        reducer
            .apply_frame(RunStreamFrameDto::LiveBatch(
                RunLiveBatchDto::new(
                    session_id,
                    run_id,
                    RunEventCursorDto::new(0),
                    vec![fact(1, None)],
                    RunEventCursorDto::new(1)
                )
                .expect("batch is valid")
            ))
            .is_err()
    );
}

#[test]
fn reducer_applies_historical_reasoning_without_double_applying_snapshot_facts() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut reducer = RunSubscriptionReducer::new(session_id, run_id);
    reducer
        .apply_initial(replay(session_id, run_id, 3))
        .expect("replay applies");
    let history = RunLiveBatchDto::new(
        session_id,
        run_id,
        RunEventCursorDto::new(0),
        vec![fact(1, Some("think")), fact(2, None), fact(3, None)],
        RunEventCursorDto::new(3),
    )
    .expect("history validates");
    reducer
        .apply_live_batch(history.clone())
        .expect("history applies");
    reducer
        .apply_live_batch(history)
        .expect("duplicate history applies");
    assert_eq!(reducer.reasoning_content(), "think");
    assert_eq!(reducer.last_cursor(), Some(RunEventCursorDto::new(3)));
}

#[test]
fn reducer_applies_replay_tail_atomically_and_preserves_tail_only_reasoning() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut reducer = RunSubscriptionReducer::new(session_id, run_id);
    reducer
        .apply_initial(replay_with_tail(
            session_id,
            run_id,
            2,
            vec![fact(3, Some("think")), fact(4, None)],
        ))
        .expect("replay tail applies");
    assert_eq!(reducer.last_cursor(), Some(RunEventCursorDto::new(4)));
    assert_eq!(reducer.reasoning_content(), "think");
    assert_eq!(
        reducer.snapshot().expect("snapshot exists").cursor(),
        RunEventCursorDto::new(2)
    );
}

#[test]
fn reducer_rejects_replay_with_incomplete_tail_without_mutating_state() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut reducer = RunSubscriptionReducer::new(session_id, run_id);
    reducer
        .apply_initial(replay_with_tail(
            session_id,
            run_id,
            0,
            vec![fact(1, Some("existing"))],
        ))
        .expect("initial replay applies");
    let before = reducer.clone();
    let invalid_tail = RunEventTailPageDto::new(
        session_id,
        run_id,
        RunEventCursorDto::new(1),
        vec![fact(2, Some("partial"))],
        RunEventCursorDto::new(2),
        true,
    )
    .expect("partial replay tail remains a valid domain page");
    let incomplete = RunSubscriptionResponseDto::Replay(
        intention_domain::RunReplayDto::new(
            snapshot(
                session_id,
                run_id,
                1,
                intention_domain::RunStatusDto::Running,
            ),
            invalid_tail,
        )
        .expect("replay shape is coherent"),
    );
    assert_eq!(
        reducer
            .apply_initial(incomplete)
            .expect_err("incomplete replay tail rejects")
            .code(),
        "invalid_run_subscription"
    );
    assert_eq!(reducer, before);
}

#[tokio::test]
async fn request_replay_applies_correlated_response_after_cursor_gap() {
    let endpoint = endpoint();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("peer accepts");
        let (_, mut requests, mut frames) = connection
            .negotiate_daemon_frames(hello("scripted-daemon"))
            .await
            .expect("peer negotiates");
        let initial_request = requests
            .receive_run_subscription()
            .await
            .expect("initial subscription arrives");
        frames
            .send(&ProtocolDaemonFrameDto::Response(
                ProtocolResponseEnvelopeDto::new(
                    local_protocol_version(),
                    initial_request.correlation_id(),
                    ProtocolMessageDto::new(
                        SCHEMA,
                        ProtocolResponsePayloadDto::RunSubscription(replay(session_id, run_id, 0)),
                    ),
                ),
            ))
            .await
            .expect("initial response sends");
        frames
            .send(&ProtocolDaemonFrameDto::RunStream(
                RunStreamFrameDto::LiveBatch(
                    RunLiveBatchDto::new(
                        session_id,
                        run_id,
                        RunEventCursorDto::new(1),
                        vec![fact(2, None)],
                        RunEventCursorDto::new(2),
                    )
                    .expect("gapped batch validates"),
                ),
            ))
            .await
            .expect("gapped frame sends");
        let replay_request = requests
            .receive_run_subscription()
            .await
            .expect("replay subscription arrives");
        assert_eq!(
            replay_request.message().payload().after_cursor(),
            Some(RunEventCursorDto::new(0))
        );
        frames
            .send(&ProtocolDaemonFrameDto::Response(
                ProtocolResponseEnvelopeDto::new(
                    local_protocol_version(),
                    replay_request.correlation_id(),
                    ProtocolMessageDto::new(
                        SCHEMA,
                        ProtocolResponsePayloadDto::RunSubscription(replay_with_tail(
                            session_id,
                            run_id,
                            0,
                            vec![fact(1, Some("recovered")), fact(2, None)],
                        )),
                    ),
                ),
            ))
            .await
            .expect("replay response sends");
    });
    let client = RunStreamClient::new(endpoint, "run-stream-client").expect("client is valid");
    let mut subscription = client
        .subscribe(SubscribeRunCommandDto::new(
            SCHEMA, session_id, run_id, None,
        ))
        .await
        .expect("initial replay arrives");
    assert!(
        subscription
            .receive()
            .await
            .expect("gap is returned")
            .is_some()
    );
    subscription
        .request_replay()
        .await
        .expect("correlated replay applies");
    assert_eq!(
        subscription.reducer().last_cursor(),
        Some(RunEventCursorDto::new(2))
    );
    assert_eq!(subscription.reducer().reasoning_content(), "recovered");
    server.await.expect("scripted peer completes");
}

#[tokio::test]
async fn request_replay_rejects_mismatched_correlation_and_error_reply_without_mutating_state() {
    for response in [
        ProtocolResponsePayloadDto::RunSubscription(replay(SessionId::new(), RunId::new(), 0)),
        ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Error(
            ErrorDto::validation("run_replay_rejected", "fixture replay rejected"),
        )),
    ] {
        let endpoint = endpoint();
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = tokio::spawn(async move {
            let connection = listener.accept().await.expect("peer accepts");
            let (_, mut requests, mut frames) = connection
                .negotiate_daemon_frames(hello("scripted-daemon"))
                .await
                .expect("peer negotiates");
            let initial_request = requests
                .receive_run_subscription()
                .await
                .expect("initial subscription arrives");
            frames
                .send(&ProtocolDaemonFrameDto::Response(
                    ProtocolResponseEnvelopeDto::new(
                        local_protocol_version(),
                        initial_request.correlation_id(),
                        ProtocolMessageDto::new(
                            SCHEMA,
                            ProtocolResponsePayloadDto::RunSubscription(replay(
                                session_id, run_id, 0,
                            )),
                        ),
                    ),
                ))
                .await
                .expect("initial response sends");
            let replay_request = requests
                .receive_run_subscription()
                .await
                .expect("replay subscription arrives");
            let correlation = if matches!(
                &response,
                ProtocolResponsePayloadDto::RunSubscription(RunSubscriptionResponseDto::Error(_))
            ) {
                replay_request.correlation_id()
            } else {
                CorrelationIdDto::new()
            };
            frames
                .send(&ProtocolDaemonFrameDto::Response(
                    ProtocolResponseEnvelopeDto::new(
                        local_protocol_version(),
                        correlation,
                        ProtocolMessageDto::new(SCHEMA, response),
                    ),
                ))
                .await
                .expect("invalid response sends");
        });
        let client = RunStreamClient::new(endpoint, "run-stream-client").expect("client is valid");
        let mut subscription = client
            .subscribe(SubscribeRunCommandDto::new(
                SCHEMA, session_id, run_id, None,
            ))
            .await
            .expect("initial replay arrives");
        let before = subscription.reducer().clone();
        let error = subscription
            .request_replay()
            .await
            .expect_err("invalid replay response rejects");
        assert!(matches!(
            error.code(),
            "invalid_local_protocol_response" | "run_replay_rejected"
        ));
        assert_eq!(subscription.reducer(), &before);
        server.await.expect("scripted peer completes");
    }
}

#[tokio::test]
async fn scripted_async_peer_sends_correlated_replay_then_uncorrelated_live_frame() {
    let endpoint = endpoint();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("peer accepts");
        let (_, mut requests, mut frames) = connection
            .negotiate_daemon_frames(hello("scripted-daemon"))
            .await
            .expect("peer negotiates");
        let request = requests
            .receive_run_subscription()
            .await
            .expect("subscription arrives");
        let correlation = request.correlation_id();
        assert_eq!(request.message().payload().session_id(), session_id);
        let response = ProtocolResponseEnvelopeDto::new(
            local_protocol_version(),
            correlation,
            ProtocolMessageDto::new(
                SCHEMA,
                ProtocolResponsePayloadDto::RunSubscription(replay(session_id, run_id, 0)),
            ),
        );
        frames
            .send(&ProtocolDaemonFrameDto::Response(response))
            .await
            .expect("reply sends");
        frames
            .send(&ProtocolDaemonFrameDto::RunStream(
                RunStreamFrameDto::LiveBatch(
                    RunLiveBatchDto::new(
                        session_id,
                        run_id,
                        RunEventCursorDto::new(0),
                        vec![fact(1, None)],
                        RunEventCursorDto::new(1),
                    )
                    .expect("live batch validates"),
                ),
            ))
            .await
            .expect("live frame sends");
    });
    let client = RunStreamClient::new(endpoint, "run-stream-client").expect("client is valid");
    let mut subscription = client
        .subscribe(SubscribeRunCommandDto::new(
            SCHEMA, session_id, run_id, None,
        ))
        .await
        .expect("initial replay arrives");
    assert_eq!(
        subscription.reducer().last_cursor(),
        Some(RunEventCursorDto::new(0))
    );
    assert!(
        subscription
            .receive()
            .await
            .expect("live frame applies")
            .is_none()
    );
    assert_eq!(
        subscription.reducer().last_cursor(),
        Some(RunEventCursorDto::new(1))
    );
    server.await.expect("scripted peer completes");
}
