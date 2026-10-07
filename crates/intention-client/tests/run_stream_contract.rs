#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Run-stream client fixtures use direct assertions for diagnostics."
)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use intention_client::{RunStreamClient, RunSubscriptionReducer};
use intention_domain::{MessageKindDto, MessageProjectionDto, RunProjectionDto, RunStatusDto};
use intention_proto::{ConfigRevisionId, ErrorDto, RunId, SchemaVersionDto, SessionId, TurnId};
use intention_protocol::{
    ProtocolDaemonMessageDto, ProtocolHelloDto, ProtocolRequestPayloadDto,
    ProtocolResponsePayloadDto, RunStatusFrameDto, RunStreamFrameDto, RunSubscriptionResponseDto,
    RunSubscriptionSnapshotDto, SubscribeRunCommandDto, decode_request_line, encode_response,
};
use intention_transport::{
    AsyncLocalListener, AsyncRequestReceiver, LocalEndpoint, local_protocol_version,
};

const SCHEMA: SchemaVersionDto = intention_protocol::CURRENT_DTO_SCHEMA_VERSION;
/// Bound that turns a hanging subscription call into a visible test failure.
const TEST_REPLY_BOUND: Duration = Duration::from_secs(5);
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
    ProtocolHelloDto::new(local_protocol_version(), name).expect("fixture hello is valid")
}

async fn receive_run_subscription(
    requests: &mut AsyncRequestReceiver,
) -> (u64, SubscribeRunCommandDto) {
    let line = requests.receive_line().await.expect("request arrives");
    let request = decode_request_line(&line).expect("request decodes");
    let id = request.id();
    match request.into_payload() {
        ProtocolRequestPayloadDto::RunSubscription(subscription) => (id, subscription),
        other => panic!("expected a run subscription request, got {other:?}"),
    }
}

fn message(
    session_id: SessionId,
    run_id: Option<RunId>,
    kind: MessageKindDto,
    text: &str,
) -> MessageProjectionDto {
    MessageProjectionDto::new(session_id, run_id, kind, text, None, None, None)
        .expect("fixture transcript row is valid")
}

fn run(session_id: SessionId, run_id: RunId, status: RunStatusDto) -> RunProjectionDto {
    RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        status,
        ConfigRevisionId::new(),
    )
}

fn snapshot(
    session_id: SessionId,
    run_id: RunId,
    status: RunStatusDto,
    messages: Vec<MessageProjectionDto>,
) -> RunSubscriptionResponseDto {
    RunSubscriptionResponseDto::snapshot(
        RunSubscriptionSnapshotDto::new(run(session_id, run_id, status), messages)
            .expect("fixture run snapshot is valid"),
    )
}

#[test]
fn reducer_applies_snapshot_content_and_status_frames_without_positions() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut reducer = RunSubscriptionReducer::new(session_id, run_id);
    assert!(reducer.run().is_none());
    assert_eq!(
        reducer
            .apply_frame(RunStreamFrameDto::Status(RunStatusFrameDto::new(
                session_id,
                run_id,
                RunStatusDto::Running,
            )))
            .expect_err("a status frame before the snapshot is a protocol violation")
            .code(),
        "invalid_local_protocol_response"
    );

    reducer
        .apply_initial(snapshot(
            session_id,
            run_id,
            RunStatusDto::Running,
            vec![message(
                session_id,
                Some(run_id),
                MessageKindDto::User,
                "build it",
            )],
        ))
        .expect("current-state snapshot applies");
    assert_eq!(reducer.status(), Some(RunStatusDto::Running));
    assert_eq!(reducer.run().expect("snapshot run exists").run_id(), run_id);
    assert_eq!(reducer.messages().len(), 1);

    reducer
        .apply_frame(RunStreamFrameDto::Content(message(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            "working",
        )))
        .expect("content frame applies");
    reducer
        .apply_frame(RunStreamFrameDto::Status(RunStatusFrameDto::new(
            session_id,
            run_id,
            RunStatusDto::Completed,
        )))
        .expect("status frame applies");
    assert_eq!(reducer.status(), Some(RunStatusDto::Completed));
    assert_eq!(reducer.messages().len(), 2);
    assert_eq!(reducer.messages()[1].text(), "working");
}

#[test]
fn reducer_rejects_frames_from_another_scope_without_mutation() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let other_run = RunId::new();
    let mut reducer = RunSubscriptionReducer::new(session_id, run_id);
    reducer
        .apply_initial(snapshot(
            session_id,
            run_id,
            RunStatusDto::Running,
            Vec::new(),
        ))
        .expect("fixture snapshot applies");

    assert_eq!(
        reducer
            .apply_frame(RunStreamFrameDto::Status(RunStatusFrameDto::new(
                session_id,
                other_run,
                RunStatusDto::Completed,
            )))
            .expect_err("a status frame from another run is rejected")
            .code(),
        "invalid_run_subscription"
    );
    assert_eq!(
        reducer
            .apply_frame(RunStreamFrameDto::Content(message(
                session_id,
                Some(other_run),
                MessageKindDto::Notice,
                "stray",
            )))
            .expect_err("a content frame from another run is rejected")
            .code(),
        "invalid_run_subscription"
    );
    assert_eq!(
        reducer
            .apply_frame(RunStreamFrameDto::Content(message(
                SessionId::new(),
                Some(run_id),
                MessageKindDto::Notice,
                "stray",
            )))
            .expect_err("a content frame from another session is rejected")
            .code(),
        "invalid_run_subscription"
    );
    assert_eq!(reducer.status(), Some(RunStatusDto::Running));
    assert!(reducer.messages().is_empty());

    reducer
        .apply_frame(RunStreamFrameDto::Content(message(
            session_id,
            None,
            MessageKindDto::Notice,
            "session notice",
        )))
        .expect("a session-level row without a run identity applies");
    assert_eq!(reducer.messages().len(), 1);

    let mut fresh = RunSubscriptionReducer::new(session_id, run_id);
    assert_eq!(
        fresh
            .apply_initial(snapshot(
                session_id,
                other_run,
                RunStatusDto::Running,
                Vec::new(),
            ))
            .expect_err("a snapshot from another run is rejected")
            .code(),
        "invalid_run_subscription"
    );
    assert!(fresh.run().is_none());
    assert_eq!(
        fresh
            .apply_initial(RunSubscriptionResponseDto::Error(ErrorDto::validation(
                "run_subscribe_rejected",
                "fixture rejected",
            )))
            .expect_err("a carried subscription error is returned")
            .code(),
        "run_subscribe_rejected"
    );
}

#[tokio::test]
async fn subscribe_delivers_current_state_then_live_content_and_status_frames() {
    let endpoint = endpoint();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("peer accepts");
        let (_, mut requests, mut messages) = connection
            .negotiate(hello("scripted-daemon"))
            .await
            .expect("peer negotiates");
        let (request_id, request) = receive_run_subscription(&mut requests).await;
        assert_eq!(request.schema_version(), SCHEMA);
        assert_eq!(request.session_id(), session_id);
        assert_eq!(request.run_id(), run_id);
        messages
            .send_message(&encode_response(
                request_id,
                ProtocolResponsePayloadDto::RunSubscription(snapshot(
                    session_id,
                    run_id,
                    RunStatusDto::Running,
                    vec![message(
                        session_id,
                        Some(run_id),
                        MessageKindDto::User,
                        "build it",
                    )],
                )),
            ))
            .await
            .expect("snapshot reply sends");
        messages
            .send_message(&ProtocolDaemonMessageDto::run_frame(
                RunStreamFrameDto::Content(message(
                    session_id,
                    Some(run_id),
                    MessageKindDto::Assistant,
                    "working",
                )),
            ))
            .await
            .expect("content frame sends");
        messages
            .send_message(&ProtocolDaemonMessageDto::run_frame(
                RunStreamFrameDto::Status(RunStatusFrameDto::new(
                    session_id,
                    run_id,
                    RunStatusDto::Completed,
                )),
            ))
            .await
            .expect("status frame sends");
    });

    let client = RunStreamClient::new(endpoint, "run-stream-client").expect("client is valid");
    let mut subscription = client
        .subscribe(SubscribeRunCommandDto::new(SCHEMA, session_id, run_id))
        .await
        .expect("current-state snapshot reply arrives");
    assert_eq!(subscription.reducer().session_id(), session_id);
    assert_eq!(subscription.reducer().run_id(), run_id);
    assert_eq!(subscription.reducer().status(), Some(RunStatusDto::Running));
    assert_eq!(subscription.reducer().messages().len(), 1);
    assert_eq!(
        subscription.reducer().messages()[0].kind(),
        MessageKindDto::User
    );

    let content = subscription
        .receive()
        .await
        .expect("content frame reads")
        .expect("content frame is not a stream close");
    assert!(matches!(
        content,
        RunStreamFrameDto::Content(ref row) if row.text() == "working"
    ));
    assert_eq!(subscription.reducer().messages().len(), 2);

    let status = subscription
        .receive()
        .await
        .expect("status frame reads")
        .expect("status frame is not a stream close");
    assert_eq!(
        status,
        RunStreamFrameDto::Status(RunStatusFrameDto::new(
            session_id,
            run_id,
            RunStatusDto::Completed,
        ))
    );
    assert_eq!(
        subscription.reducer().status(),
        Some(RunStatusDto::Completed)
    );

    assert!(
        subscription
            .receive()
            .await
            .expect("a closed stream reads as a clean end")
            .is_none(),
        "the closed stream reports no further frames"
    );
    server.await.expect("scripted peer completes");
}

#[tokio::test]
async fn receive_rejects_a_live_frame_from_another_run_scope() {
    let endpoint = endpoint();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let other_run = RunId::new();
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("peer accepts");
        let (_, mut requests, mut messages) = connection
            .negotiate(hello("scripted-daemon"))
            .await
            .expect("peer negotiates");
        let (request_id, _) = receive_run_subscription(&mut requests).await;
        messages
            .send_message(&encode_response(
                request_id,
                ProtocolResponsePayloadDto::RunSubscription(snapshot(
                    session_id,
                    run_id,
                    RunStatusDto::Running,
                    Vec::new(),
                )),
            ))
            .await
            .expect("snapshot reply sends");
        messages
            .send_message(&ProtocolDaemonMessageDto::run_frame(
                RunStreamFrameDto::Status(RunStatusFrameDto::new(
                    session_id,
                    other_run,
                    RunStatusDto::Completed,
                )),
            ))
            .await
            .expect("foreign status frame sends");
    });

    let client = RunStreamClient::new(endpoint, "run-stream-client").expect("client is valid");
    let mut subscription = client
        .subscribe(SubscribeRunCommandDto::new(SCHEMA, session_id, run_id))
        .await
        .expect("current-state snapshot reply arrives");
    let error = subscription
        .receive()
        .await
        .expect_err("a frame from another run scope is rejected");
    assert_eq!(error.code(), "invalid_run_subscription");
    assert_eq!(subscription.reducer().status(), Some(RunStatusDto::Running));
    assert!(subscription.reducer().messages().is_empty());
    server.await.expect("scripted peer completes");
}

#[tokio::test]
async fn subscribe_returns_a_typed_error_when_the_channel_closes_before_the_reply() {
    let endpoint = endpoint();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("peer accepts");
        let (_, mut requests, _messages) = connection
            .negotiate(hello("scripted-daemon"))
            .await
            .expect("peer negotiates");
        let _ = receive_run_subscription(&mut requests).await;
    });

    let client = RunStreamClient::new(endpoint, "run-stream-client").expect("client is valid");
    let error = tokio::time::timeout(
        TEST_REPLY_BOUND,
        client.subscribe(SubscribeRunCommandDto::new(SCHEMA, session_id, run_id)),
    )
    .await
    .expect("a closed channel must not hang the subscriber")
    .err()
    .expect("a closed channel before the reply is a typed error");
    assert_eq!(error.code(), "local_daemon_connection_unavailable");
    server.await.expect("scripted peer completes");
}
