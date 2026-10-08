#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Run-stream client fixtures use direct assertions for diagnostics."
)]

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{TEST_REPLY_BOUND, endpoint, message};

use intention_client::{RunStreamClient, RunStreamState};
use intention_proto::{
    ClientRequestDto, ProtocolDaemonMessageDto, ProtocolResultDto, RunStreamFrameDto,
    RunSubscriptionSnapshotDto, SubscribeRunCommandDto, decode_request_line, encode_reply,
};
use intention_proto::{ConfigRevisionId, RunId, SessionId, TurnId};
use intention_proto::{MessageKindDto, MessageProjectionDto, RunProjectionDto, RunStatusDto};
use intention_transport::{AsyncLocalListener, AsyncMessageReceiver};

async fn receive_run_subscription(
    requests: &mut AsyncMessageReceiver,
) -> (u64, SubscribeRunCommandDto) {
    let line = requests.receive_line().await.expect("request arrives");
    let request = decode_request_line(&line).expect("request decodes");
    match request.request() {
        ClientRequestDto::SubscribeRun(subscription) => (request.id(), *subscription),
        other => panic!("expected a run subscription request, got {other:?}"),
    }
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
) -> RunSubscriptionSnapshotDto {
    RunSubscriptionSnapshotDto::new(run(session_id, run_id, status), messages)
        .expect("fixture run snapshot is valid")
}

#[test]
fn state_applies_the_subscription_snapshot_then_committed_frames() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = RunStreamState::new(session_id, run_id);
    assert!(state.run().is_none());
    assert_eq!(
        state
            .apply_frame(RunStreamFrameDto::Status(run(
                session_id,
                run_id,
                RunStatusDto::Running,
            )))
            .expect_err("a status frame before the snapshot is a protocol violation")
            .code(),
        "invalid_local_protocol_response"
    );

    state
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
    assert_eq!(state.status(), Some(RunStatusDto::Running));
    assert_eq!(state.run().expect("snapshot run exists").run_id(), run_id);
    assert_eq!(state.messages().len(), 1);

    state
        .apply_frame(RunStreamFrameDto::Content(message(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            "working",
        )))
        .expect("content frame applies");
    state
        .apply_frame(RunStreamFrameDto::Status(run(
            session_id,
            run_id,
            RunStatusDto::Completed,
        )))
        .expect("status frame applies");
    assert_eq!(state.status(), Some(RunStatusDto::Completed));
    assert_eq!(state.messages().len(), 2);
    assert_eq!(state.messages()[1].text(), "working");
}

#[test]
fn state_rejects_frames_from_another_scope_without_mutation() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let other_run = RunId::new();
    let mut state = RunStreamState::new(session_id, run_id);
    state
        .apply_initial(snapshot(
            session_id,
            run_id,
            RunStatusDto::Running,
            Vec::new(),
        ))
        .expect("fixture snapshot applies");

    assert_eq!(
        state
            .apply_frame(RunStreamFrameDto::Status(run(
                session_id,
                other_run,
                RunStatusDto::Completed,
            )))
            .expect_err("a status frame from another run is rejected")
            .code(),
        "invalid_run_subscription"
    );
    assert_eq!(
        state
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
        state
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
    assert_eq!(state.status(), Some(RunStatusDto::Running));
    assert!(state.messages().is_empty());

    state
        .apply_frame(RunStreamFrameDto::Content(message(
            session_id,
            None,
            MessageKindDto::Notice,
            "session notice",
        )))
        .expect("a session-level row without a run identity applies");
    assert_eq!(state.messages().len(), 1);

    let mut fresh = RunStreamState::new(session_id, run_id);
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
}

#[tokio::test]
async fn subscribe_delivers_current_state_then_live_content_and_status_frames() {
    let endpoint = endpoint();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("peer accepts");
        let (mut requests, mut messages) = connection.split();
        let (request_id, request) = receive_run_subscription(&mut requests).await;
        assert_eq!(request.session_id(), session_id);
        assert_eq!(request.run_id(), run_id);
        messages
            .send_message(&encode_reply(
                request_id,
                ProtocolResultDto::RunSubscribed(snapshot(
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
            .send_message(&ProtocolDaemonMessageDto::frame(
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
            .send_message(&ProtocolDaemonMessageDto::frame(RunStreamFrameDto::Status(
                run(session_id, run_id, RunStatusDto::Completed),
            )))
            .await
            .expect("status frame sends");
    });

    let client = RunStreamClient::new(endpoint);
    let mut subscription = client
        .subscribe(SubscribeRunCommandDto::new(session_id, run_id))
        .await
        .expect("current-state snapshot reply arrives");
    assert_eq!(subscription.state().session_id(), session_id);
    assert_eq!(subscription.state().run_id(), run_id);
    assert_eq!(subscription.state().status(), Some(RunStatusDto::Running));
    assert_eq!(subscription.state().messages().len(), 1);
    assert_eq!(
        subscription.state().messages()[0].kind(),
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
    assert_eq!(subscription.state().messages().len(), 2);

    let status = subscription
        .receive()
        .await
        .expect("status frame reads")
        .expect("status frame is not a stream close");
    // The frame carries the committed projection itself; the fixture builds
    // each projection with fresh turn and config identities, so the assertion
    // checks the run scope and terminal status instead of object identity.
    let RunStreamFrameDto::Status(projection) = status else {
        unreachable!("a status frame carries the committed run projection")
    };
    assert_eq!(projection.session_id(), session_id);
    assert_eq!(projection.run_id(), run_id);
    assert_eq!(projection.status(), RunStatusDto::Completed);
    assert_eq!(subscription.state().status(), Some(RunStatusDto::Completed));

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
        let (mut requests, mut messages) = connection.split();
        let (request_id, _) = receive_run_subscription(&mut requests).await;
        messages
            .send_message(&encode_reply(
                request_id,
                ProtocolResultDto::RunSubscribed(snapshot(
                    session_id,
                    run_id,
                    RunStatusDto::Running,
                    Vec::new(),
                )),
            ))
            .await
            .expect("snapshot reply sends");
        messages
            .send_message(&ProtocolDaemonMessageDto::frame(RunStreamFrameDto::Status(
                run(session_id, other_run, RunStatusDto::Completed),
            )))
            .await
            .expect("foreign status frame sends");
    });

    let client = RunStreamClient::new(endpoint);
    let mut subscription = client
        .subscribe(SubscribeRunCommandDto::new(session_id, run_id))
        .await
        .expect("current-state snapshot reply arrives");
    let error = subscription
        .receive()
        .await
        .expect_err("a frame from another run scope is rejected");
    assert_eq!(error.code(), "invalid_run_subscription");
    assert_eq!(subscription.state().status(), Some(RunStatusDto::Running));
    assert!(subscription.state().messages().is_empty());
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
        let (mut requests, _messages) = connection.split();
        let _ = receive_run_subscription(&mut requests).await;
    });

    let client = RunStreamClient::new(endpoint);
    let error = tokio::time::timeout(
        TEST_REPLY_BOUND,
        client.subscribe(SubscribeRunCommandDto::new(session_id, run_id)),
    )
    .await
    .expect("a closed channel must not hang the subscriber")
    .err()
    .expect("a closed channel before the reply is a typed error");
    assert_eq!(error.code(), "local_daemon_connection_unavailable");
    server.await.expect("scripted peer completes");
}
