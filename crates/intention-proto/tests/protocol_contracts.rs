#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first typed-wire contract evidence.

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{fixture_message, fixture_projection, fixture_workspace_root};

use intention_proto::{
    ClientRequestDto, CreateSessionAcceptedDto, CreateSessionCommandDto, DaemonHealthDto, ErrorDto,
    GetSessionSnapshotQueryDto, InterruptRunAcceptedDto, InterruptRunCommandDto,
    ProtocolDaemonMessageDto, ProtocolResultDto, RemoveTurnAcceptedDto, RemoveTurnCommandDto,
    RunProjectionDto, RunStatusDto, RunStreamFrameDto, RunSubscriptionSnapshotDto,
    SendUserTurnAcceptedDto, SendUserTurnCommandDto, SendUserTurnOutcomeDto, SessionSnapshotDto,
    SubscribeRunCommandDto, decode_request_line, decode_response, encode_reply, encode_request,
    parse_daemon_message, parse_run_frame,
};
use intention_proto::{ConfigRevisionId, IdempotencyKey, ProjectId, RunId, SessionId, TurnId};
use intention_proto::{RunModeDto, WorkspaceId};

fn fixture_run(session_id: SessionId, run_id: RunId) -> RunProjectionDto {
    RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        RunStatusDto::Running,
        ConfigRevisionId::new(),
    )
}

fn fixture_snapshot(session_id: SessionId, run_id: RunId) -> SessionSnapshotDto {
    SessionSnapshotDto::with_projection(
        session_id,
        fixture_projection(session_id),
        vec![fixture_message(session_id, run_id)],
    )
    .expect("fixture snapshot is valid")
}

fn fixture_run_snapshot(session_id: SessionId, run_id: RunId) -> RunSubscriptionSnapshotDto {
    RunSubscriptionSnapshotDto::new(
        fixture_run(session_id, run_id),
        vec![fixture_message(session_id, run_id)],
    )
    .expect("fixture run snapshot is valid")
}

/// Returns the wire kind one request is answered with.
///
/// The match is deliberately wildcard-free: adding a request variant fails to
/// compile until its own result kind is named here, so the wire can never grow
/// a request whose answer is unnamed.
const fn result_kind(request: &ClientRequestDto) -> &'static str {
    match request {
        ClientRequestDto::CreateSession(_) => "session_created",
        ClientRequestDto::SendUserTurn(_) => "turn_accepted",
        ClientRequestDto::RemoveTurn(_) => "turn_removed",
        ClientRequestDto::InterruptRun(_) => "run_interrupted",
        ClientRequestDto::GetSessionSnapshot(_) => "session_snapshot",
        ClientRequestDto::GetDaemonHealth => "daemon_health",
        ClientRequestDto::SubscribeRun(_) => "run_subscribed",
    }
}

fn fixture_requests(session_id: SessionId, run_id: RunId) -> Vec<ClientRequestDto> {
    vec![
        ClientRequestDto::CreateSession(CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            fixture_workspace_root(),
            RunModeDto::Build,
        )),
        ClientRequestDto::SendUserTurn(
            SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "hello")
                .expect("fixture turn is valid"),
        ),
        ClientRequestDto::RemoveTurn(RemoveTurnCommandDto::new(session_id, TurnId::new())),
        ClientRequestDto::InterruptRun(InterruptRunCommandDto::new(session_id, run_id)),
        ClientRequestDto::GetSessionSnapshot(GetSessionSnapshotQueryDto::new(session_id)),
        ClientRequestDto::GetDaemonHealth,
        ClientRequestDto::SubscribeRun(SubscribeRunCommandDto::new(session_id, run_id)),
    ]
}

fn fixture_results(session_id: SessionId, run_id: RunId) -> Vec<ProtocolResultDto> {
    vec![
        ProtocolResultDto::SessionCreated(CreateSessionAcceptedDto::new(
            ProjectId::new(),
            WorkspaceId::new(),
            session_id,
        )),
        ProtocolResultDto::TurnAccepted(SendUserTurnAcceptedDto::new(
            session_id,
            TurnId::new(),
            SendUserTurnOutcomeDto::Pending,
        )),
        ProtocolResultDto::TurnRemoved(RemoveTurnAcceptedDto::new(session_id, TurnId::new())),
        ProtocolResultDto::RunInterrupted(InterruptRunAcceptedDto::new(session_id, run_id)),
        ProtocolResultDto::SessionSnapshot(fixture_snapshot(session_id, run_id)),
        ProtocolResultDto::DaemonHealth(DaemonHealthDto::ready()),
        ProtocolResultDto::RunSubscribed(fixture_run_snapshot(session_id, run_id)),
    ]
}

#[test]
fn every_request_round_trips_with_its_named_result_kind() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let requests = fixture_requests(session_id, run_id);
    let results = fixture_results(session_id, run_id);
    let mut kinds = std::collections::BTreeSet::new();
    for (position, request) in requests.iter().enumerate() {
        let id = position as u64 + 1;
        let line = serde_json::to_string(&encode_request(id, request.clone()))
            .expect("request serializes");
        let value: serde_json::Value =
            serde_json::from_str(&line).expect("the request line is JSON");
        assert_eq!(value["id"], id);
        assert!(
            value["request"]["data"].is_object() || value["request"]["data"].is_null(),
            "a typed request carries its payload in data"
        );
        // The answer to this request carries the result kind the wire names for
        // it, so a request can never be answered with another operation's result.
        let reply = encode_reply(id, results[position].clone());
        let reply_value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&reply).expect("reply serializes"))
                .expect("the reply line is JSON");
        assert_eq!(
            reply_value["data"]["result"]["kind"],
            result_kind(request),
            "each request names the result kind its answer carries"
        );

        let decoded = decode_request_line(&line).expect("request decodes");
        assert_eq!(decoded.id(), id);
        assert_eq!(decoded.request(), request);
        kinds.insert(
            value["request"]["kind"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        );
    }
    assert_eq!(
        kinds.len(),
        7,
        "the wire implements exactly one request kind per operation"
    );
}

#[test]
fn every_result_round_trips_through_the_typed_reply() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    for (position, result) in fixture_results(session_id, run_id).into_iter().enumerate() {
        let id = position as u64 + 71;
        let line =
            serde_json::to_string(&encode_reply(id, result.clone())).expect("reply serializes");
        let decoded = decode_response(&line, id).expect("the correlated reply decodes");
        assert_eq!(decoded, result);
        assert_eq!(
            parse_run_frame(&line)
                .expect_err("a reply is never accepted as a frame")
                .code(),
            "invalid_local_protocol_response"
        );
        assert!(
            decode_response(&line, id + 1).is_err(),
            "an uncorrelated reply fails closed"
        );
    }
}

#[test]
fn rejections_carry_the_request_identity_and_surface_the_typed_error() {
    let correlated = ProtocolDaemonMessageDto::rejection(
        Some(7),
        ErrorDto::validation("fixture_rejected", "fixture rejection"),
    );
    let line = serde_json::to_string(&correlated).expect("rejection serializes");
    assert_eq!(
        decode_response(&line, 7)
            .expect_err("the correlated rejection surfaces its error")
            .code(),
        "fixture_rejected"
    );
    assert!(
        decode_response(&line, 8).is_err(),
        "a rejection for another identity fails closed"
    );

    let identity_less = ProtocolDaemonMessageDto::rejection(
        None,
        ErrorDto::validation(
            "invalid_local_protocol_message",
            "a local protocol request was invalid",
        ),
    );
    let line = serde_json::to_string(&identity_less).expect("rejection serializes");
    assert_eq!(
        decode_response(&line, 11)
            .expect_err("an undecodable request still surfaces its typed rejection")
            .code(),
        "invalid_local_protocol_message"
    );
}

#[test]
fn frames_carry_committed_values_without_positions() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let content = RunStreamFrameDto::Content(fixture_message(session_id, run_id));
    let status = RunStreamFrameDto::Status(fixture_run(session_id, run_id));

    for (frame, kind) in [(&content, "content"), (&status, "status")] {
        let message = ProtocolDaemonMessageDto::frame(frame.clone());
        let line = serde_json::to_string(&message).expect("frame serializes");
        let value: serde_json::Value = serde_json::from_str(&line).expect("frame line is JSON");
        assert_eq!(value["kind"], "frame");
        assert_eq!(value["data"]["kind"], kind);
        assert!(value["data"]["data"].is_object());
        assert!(
            !line.contains("cursor") && !line.contains("sequence"),
            "a committed frame carries no position"
        );
        assert_eq!(parse_run_frame(&line).expect("frame parses"), *frame);
        assert!(
            decode_response(&line, 1).is_err(),
            "a frame is never accepted as a correlated reply"
        );
    }
}

#[test]
fn malformed_wire_lines_fail_closed() {
    assert!(
        serde_json::from_str::<ClientRequestDto>(r#"{"kind":"unknown","data":{}}"#).is_err(),
        "a closed request table rejects unknown kinds"
    );
    assert!(
        serde_json::from_str::<ProtocolResultDto>(r#"{"kind":"unknown","data":{}}"#).is_err(),
        "a closed result table rejects unknown kinds"
    );
    assert!(
        serde_json::from_str::<RunStreamFrameDto>(r#"{"kind":"unknown","data":{}}"#).is_err(),
        "a closed frame table rejects unknown kinds"
    );
    assert!(
        serde_json::from_str::<ClientRequestDto>(
            r#"{"kind":"send_user_turn","data":{"session_id":"11111111-1111-4111-8111-111111111111","idempotency_key":"11111111-1111-4111-8111-111111111111","content":" "}}"#
        )
        .is_err(),
        "a blank turn command fails closed"
    );
    assert!(
        serde_json::from_str::<ProtocolResultDto>(
            r#"{"kind":"session_snapshot","data":{"session_id":"11111111-1111-4111-8111-111111111111"}}"#
        )
        .is_err(),
        "a session snapshot without its projection and messages fails closed"
    );
    assert_eq!(
        decode_request_line("{")
            .expect_err("a malformed line fails closed")
            .code(),
        "invalid_local_protocol_message"
    );
    assert_eq!(
        parse_daemon_message("{")
            .expect_err("foreign bytes are a stale-peer failure")
            .code(),
        "stale_daemon_protocol"
    );
}
