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
    SessionSummariesDto, SessionSummaryDto, SubscribeRunCommandDto, TextDeltaFrameDto,
    decode_request_line, decode_response, encode_reply, encode_request, parse_daemon_message,
    parse_run_frame, run_status_is_terminal, validate_run_status_transition,
};
use intention_proto::{ConfigRevisionId, IdempotencyKey, MessageKindDto, ProjectId, RunId};
use intention_proto::{RunModeDto, SessionId, TurnId, WorkspaceId};

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

fn fixture_session_summary(session_id: SessionId, run_id: RunId) -> SessionSummaryDto {
    SessionSummaryDto::new(
        session_id,
        ProjectId::new(),
        WorkspaceId::new(),
        RunModeDto::Build,
        1_726_000_000,
        Some(fixture_run(session_id, run_id)),
    )
}

fn fixture_session_summaries(session_id: SessionId, run_id: RunId) -> SessionSummariesDto {
    SessionSummariesDto::new(vec![fixture_session_summary(session_id, run_id)], 3)
        .expect("fixture session summaries are valid")
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
        ClientRequestDto::ListSessions => "sessions_listed",
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
        ClientRequestDto::ListSessions,
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
        ProtocolResultDto::SessionsListed(fixture_session_summaries(session_id, run_id)),
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
        8,
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
fn session_lists_round_trip_with_their_omitted_count() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let summaries = fixture_session_summaries(session_id, run_id);
    assert_eq!(summaries.omitted(), 3);
    let summary = summaries.sessions()[0];
    assert_eq!(summary.session_id(), session_id);
    assert_eq!(summary.mode(), RunModeDto::Build);
    assert_eq!(summary.updated_at(), 1_726_000_000);
    assert_eq!(
        summary.active_run().map(RunProjectionDto::run_id),
        Some(run_id)
    );

    let line = serde_json::to_string(&encode_request(9, ClientRequestDto::ListSessions))
        .expect("session list request serializes");
    let value: serde_json::Value = serde_json::from_str(&line).expect("request line is JSON");
    assert_eq!(value["request"]["kind"], "list_sessions");
    assert_eq!(
        decode_request_line(&line)
            .expect("request decodes")
            .request(),
        &ClientRequestDto::ListSessions
    );

    let reply = encode_reply(9, ProtocolResultDto::SessionsListed(summaries.clone()));
    let line = serde_json::to_string(&reply).expect("session list reply serializes");
    let value: serde_json::Value = serde_json::from_str(&line).expect("reply line is JSON");
    assert_eq!(value["data"]["result"]["kind"], "sessions_listed");
    assert_eq!(
        decode_response(&line, 9).expect("the correlated reply decodes"),
        ProtocolResultDto::SessionsListed(summaries.clone())
    );
    assert_eq!(
        serde_json::from_str::<SessionSummariesDto>(
            &serde_json::to_string(&summaries).expect("session summaries serialize")
        )
        .expect("session summaries deserialize"),
        summaries
    );
}

#[test]
fn text_delta_frames_round_trip_as_transient_chunks() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let delta = TextDeltaFrameDto::new(session_id, run_id, 2, "partial output")
        .expect("fixture text delta is valid");
    assert_eq!(delta.session_id(), session_id);
    assert_eq!(delta.run_id(), run_id);
    assert_eq!(delta.step(), 2);
    assert_eq!(delta.text(), "partial output");

    let frame = RunStreamFrameDto::TextDelta(delta);
    let message = ProtocolDaemonMessageDto::frame(frame.clone());
    let line = serde_json::to_string(&message).expect("text delta frame serializes");
    let value: serde_json::Value = serde_json::from_str(&line).expect("frame line is JSON");
    assert_eq!(value["data"]["kind"], "text_delta");
    assert_eq!(value["data"]["data"]["step"], 2);
    assert_eq!(value["data"]["data"]["text"], "partial output");
    assert!(
        !line.contains("cursor") && !line.contains("sequence"),
        "a text delta carries no stream position"
    );
    assert_eq!(parse_run_frame(&line).expect("frame parses"), frame);
    assert!(
        decode_response(&line, 1).is_err(),
        "a text delta is never accepted as a correlated reply"
    );
}

#[test]
fn session_summaries_and_text_deltas_validate_their_required_shape() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    assert_eq!(
        SessionSummariesDto::new(
            vec![SessionSummaryDto::new(
                SessionId::new(),
                ProjectId::new(),
                WorkspaceId::new(),
                RunModeDto::Build,
                1_726_000_000,
                Some(fixture_run(session_id, run_id)),
            )],
            0,
        )
        .expect_err("an active run must belong to its summary session")
        .code(),
        "invalid_session_summaries"
    );
    assert_eq!(
        SessionSummariesDto::new(
            vec![
                fixture_session_summary(session_id, run_id),
                fixture_session_summary(session_id, run_id),
            ],
            0,
        )
        .expect_err("a session list carries each session once")
        .code(),
        "invalid_session_summaries"
    );
    assert_eq!(
        TextDeltaFrameDto::new(session_id, run_id, 0, "")
            .expect_err("an empty text delta is rejected")
            .code(),
        "invalid_text_delta"
    );
    assert!(
        serde_json::from_str::<RunStreamFrameDto>(
            r#"{"kind":"text_delta","data":{"session_id":"11111111-1111-4111-8111-111111111111","run_id":"11111111-1111-4111-8111-111111111111","step":0,"text":""}}"#
        )
        .is_err(),
        "an empty text delta fails closed on the wire"
    );
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

    // A current envelope whose payload does not decode is this wire's own
    // failure, not a stale peer: the client must not answer it by launching a
    // daemon against the peer that answered.
    assert_eq!(
        parse_daemon_message(
            r#"{"kind":"reply","data":{"id":1,"result":{"kind":"daemon_health","data":{}}}}"#
        )
        .expect_err("a current envelope with an invalid payload fails closed")
        .code(),
        "invalid_local_protocol_response"
    );
    for foreign in [
        r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
        r#"{"kind":"hello","data":{}}"#,
        r#"{"id":1,"request":{"kind":"get_daemon_health","data":null}}"#,
    ] {
        assert_eq!(
            parse_daemon_message(foreign)
                .expect_err("foreign bytes are a stale-peer failure")
                .code(),
            "stale_daemon_protocol",
            "{foreign} is not an envelope of the current wire"
        );
    }
}

#[test]
fn durable_enum_spellings_match_their_wire_spelling() {
    for status in [
        RunStatusDto::Starting,
        RunStatusDto::Running,
        RunStatusDto::Completed,
        RunStatusDto::Failed,
        RunStatusDto::Interrupted,
    ] {
        assert_eq!(
            serde_json::to_value(status).expect("status serializes"),
            serde_json::json!(status.as_str()),
            "the wire spelling of {status:?} is its durable spelling"
        );
        assert_eq!(RunStatusDto::parse(status.as_str()).ok(), Some(status));
    }
    for mode in [RunModeDto::Plan, RunModeDto::Build] {
        assert_eq!(
            serde_json::to_value(mode).expect("mode serializes"),
            serde_json::json!(mode.as_str()),
            "the wire spelling of {mode:?} is its durable spelling"
        );
        assert_eq!(RunModeDto::parse(mode.as_str()).ok(), Some(mode));
    }
    for kind in [
        MessageKindDto::User,
        MessageKindDto::Assistant,
        MessageKindDto::ToolCall,
        MessageKindDto::ToolResult,
        MessageKindDto::Notice,
    ] {
        assert_eq!(
            serde_json::to_value(kind).expect("kind serializes"),
            serde_json::json!(kind.as_str()),
            "the wire spelling of {kind:?} is its durable spelling"
        );
        assert_eq!(MessageKindDto::parse(kind.as_str()).ok(), Some(kind));
    }
}

#[test]
fn terminal_run_statuses_are_the_transition_predicate_set() {
    let statuses = [
        RunStatusDto::Starting,
        RunStatusDto::Running,
        RunStatusDto::Completed,
        RunStatusDto::Failed,
        RunStatusDto::Interrupted,
    ];
    for status in statuses {
        assert_eq!(
            RunStatusDto::TERMINAL.contains(&status),
            run_status_is_terminal(status),
            "{status:?} is terminal in both the list and the predicate"
        );
    }
    for terminal in RunStatusDto::TERMINAL {
        for successor in statuses {
            assert!(
                validate_run_status_transition(terminal, successor).is_err(),
                "a terminal status has no declared successor: {terminal:?} -> {successor:?}"
            );
        }
    }
}
