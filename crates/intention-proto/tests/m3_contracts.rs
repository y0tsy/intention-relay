#![allow(
    clippy::expect_used,
    reason = "M3 contract fixtures use expect for precise test diagnostics."
)]

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{fixture_message, fixture_projection};

use intention_proto::{
    BoundedText, ConfigRevisionId, CreateSessionAcceptedDto, ExecuteResult,
    InterruptRunAcceptedDto, MessageId, MessageKindDto, MessageProjectionDto,
    PendingTurnProjectionDto, ProtocolResultDto, RemoveTurnAcceptedDto, RunId, RunModeDto,
    RunProjectionDto, RunStatusDto, SendUserTurnAcceptedDto, SendUserTurnOutcomeDto,
    SessionProjectionDto, SessionSnapshotDto, TextResult, ToolCallId, ToolCallRowDto, ToolId,
    ToolIdentityDto, ToolInput, ToolResult, ToolResultMetadataEntryDto, ToolResultRowDto,
    ToolResultStatusDto, ToolRowDto, TurnId, WorkspaceRootDto, validate_run_status_transition,
};
use intention_proto::{ProjectId, SessionId, WorkspaceId};

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-proto-m3-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

#[test]
fn typed_results_carry_required_durable_evidence() {
    let session_id = SessionId::new();
    let workspace_id = WorkspaceId::new();
    let created = CreateSessionAcceptedDto::new(ProjectId::new(), workspace_id, session_id);
    let started = SendUserTurnAcceptedDto::new(
        session_id,
        TurnId::new(),
        SendUserTurnOutcomeDto::Started {
            run_id: RunId::new(),
            config_revision_id: ConfigRevisionId::new(),
        },
    );
    let pending =
        SendUserTurnAcceptedDto::new(session_id, TurnId::new(), SendUserTurnOutcomeDto::Pending);
    let removed = RemoveTurnAcceptedDto::new(session_id, TurnId::new());
    let interrupted = InterruptRunAcceptedDto::new(session_id, RunId::new());

    for result in [
        ProtocolResultDto::SessionCreated(created),
        ProtocolResultDto::TurnAccepted(started),
        ProtocolResultDto::TurnAccepted(pending),
        ProtocolResultDto::TurnRemoved(removed),
        ProtocolResultDto::RunInterrupted(interrupted),
    ] {
        let encoded = serde_json::to_string(&result).expect("result serializes");
        assert!(
            !encoded.contains("sequence"),
            "acceptance evidence carries no committed sequence"
        );
        assert_eq!(
            serde_json::from_str::<ProtocolResultDto>(&encoded).expect("result decodes"),
            result
        );
    }
}

#[test]
fn snapshots_validate_the_required_m3_projection() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let snapshot = SessionSnapshotDto::with_projection(
        session_id,
        fixture_projection(session_id),
        vec![fixture_message(session_id, run_id)],
    )
    .expect("matching projection is valid");
    assert_eq!(snapshot.projection().session_id(), session_id);
    assert_eq!(snapshot.session_id(), session_id);
    assert_eq!(snapshot.messages().len(), 1);
    assert!(
        serde_json::from_str::<SessionSnapshotDto>(
            r#"{"session_id":"11111111-1111-4111-8111-111111111111","projection":null,"messages":[]}"#
        )
        .is_err(),
        "a snapshot without its required projection must fail closed"
    );
    assert!(
        serde_json::from_str::<SessionSnapshotDto>(
            r#"{"session_id":"11111111-1111-4111-8111-111111111111","projection":{"project_id":"11111111-1111-4111-8111-111111111111","session_id":"11111111-1111-4111-8111-111111111111","workspace_id":"11111111-1111-4111-8111-111111111111","workspace_root":"/fixture","mode":"build","pending_turns":[]}}"#
        )
        .is_err(),
        "a snapshot without its required messages must fail closed"
    );
}

#[test]
fn session_snapshot_validation_covers_m3_failure_boundaries() {
    let session_id = SessionId::new();

    assert_eq!(
        SessionSnapshotDto::with_projection(
            session_id,
            fixture_projection(SessionId::new()),
            Vec::new(),
        )
        .expect_err("projection session mismatch rejects")
        .code(),
        "invalid_session_snapshot_projection"
    );
    assert_eq!(
        SessionSnapshotDto::with_projection(
            session_id,
            fixture_projection(session_id),
            vec![fixture_message(SessionId::new(), RunId::new())],
        )
        .expect_err("a transcript row from another session rejects")
        .code(),
        "invalid_session_snapshot_projection"
    );
}

#[test]
fn a_session_projection_reports_its_omitted_pending_turn_count() {
    let session_id = SessionId::new();
    let projection = SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
        None,
        None,
        Vec::new(),
    )
    .expect("projection is valid");
    assert_eq!(
        projection.pending_turns_omitted(),
        0,
        "an untrimmed projection omits nothing"
    );

    let trimmed = projection.with_pending_turns_omitted(7);
    assert_eq!(trimmed.pending_turns_omitted(), 7);
    assert!(trimmed.pending_turns().is_empty());
    let decoded: SessionProjectionDto = serde_json::from_str(
        &serde_json::to_string(&trimmed).expect("the trimmed projection serializes"),
    )
    .expect("the trimmed projection decodes");
    assert_eq!(decoded, trimmed);
    assert_eq!(
        decoded.pending_turns_omitted(),
        7,
        "the omitted count survives the wire"
    );

    let absent = serde_json::from_value::<SessionProjectionDto>(serde_json::json!({
        "project_id": ProjectId::new(),
        "session_id": session_id,
        "workspace_id": WorkspaceId::new(),
        "workspace_root": workspace_root(),
        "mode": "build",
        "pending_turns": []
    }))
    .expect("a projection without the count decodes");
    assert_eq!(
        absent.pending_turns_omitted(),
        0,
        "a projection that carries no count omits nothing"
    );
}

#[test]
fn session_projection_keeps_its_closed_shape_and_optional_state() {
    let project_id = ProjectId::new();
    let session_id = SessionId::new();
    let workspace_id = WorkspaceId::new();

    let first = PendingTurnProjectionDto::new(session_id, TurnId::new(), "first")
        .expect("pending turn is valid");
    let second = PendingTurnProjectionDto::new(session_id, TurnId::new(), "second")
        .expect("pending turn is valid");
    let ordered = SessionProjectionDto::new(
        project_id,
        session_id,
        workspace_id,
        workspace_root(),
        RunModeDto::Build,
        None,
        None,
        vec![first.clone(), second],
    )
    .expect("ordered pending turns are coherent");
    assert_eq!(ordered.pending_turns().len(), 2);
    assert_eq!(ordered.pending_turns()[0].content(), "first");
    assert_eq!(ordered.pending_turns()[1].content(), "second");
    assert!(
        SessionProjectionDto::new(
            project_id,
            session_id,
            workspace_id,
            workspace_root(),
            RunModeDto::Build,
            None,
            None,
            vec![first.clone(), first],
        )
        .is_err()
    );
    let third = PendingTurnProjectionDto::new(session_id, TurnId::new(), "third")
        .expect("pending turn is valid");
    let fourth = PendingTurnProjectionDto::new(session_id, TurnId::new(), "fourth")
        .expect("pending turn is valid");
    assert!(
        SessionProjectionDto::new(
            project_id,
            session_id,
            workspace_id,
            workspace_root(),
            RunModeDto::Build,
            None,
            None,
            vec![third.clone(), fourth, third],
        )
        .is_err(),
        "a repeated pending turn identity is rejected wherever it appears"
    );
    assert!(PendingTurnProjectionDto::new(session_id, TurnId::new(), "   ").is_err());
    assert!(
        serde_json::from_value::<PendingTurnProjectionDto>(serde_json::json!({
            "session_id": session_id,
            "turn_id": TurnId::new(),
            "content": " "
        }))
        .is_err()
    );

    let foreign_turn =
        PendingTurnProjectionDto::new(SessionId::new(), TurnId::new(), "other session")
            .expect("pending fixture is valid");
    assert!(
        SessionProjectionDto::new(
            project_id,
            session_id,
            workspace_id,
            workspace_root(),
            RunModeDto::Build,
            None,
            None,
            vec![foreign_turn],
        )
        .is_err()
    );

    let foreign_run = RunProjectionDto::new(
        SessionId::new(),
        RunId::new(),
        TurnId::new(),
        RunStatusDto::Running,
        ConfigRevisionId::new(),
    );
    assert!(
        SessionProjectionDto::new(
            project_id,
            session_id,
            workspace_id,
            workspace_root(),
            RunModeDto::Build,
            None,
            Some(foreign_run),
            Vec::new(),
        )
        .is_err()
    );

    let revision = ConfigRevisionId::new();
    let run = RunProjectionDto::new(
        session_id,
        RunId::new(),
        TurnId::new(),
        RunStatusDto::Running,
        revision,
    );
    assert_eq!(run.session_id(), session_id);
    assert_eq!(run.status(), RunStatusDto::Running);
    assert_eq!(run.config_revision_id(), revision);
    let decoded_run: RunProjectionDto =
        serde_json::from_str(&serde_json::to_string(&run).expect("run serializes"))
            .expect("run decodes");
    assert_eq!(decoded_run, run);

    let pending = PendingTurnProjectionDto::new(session_id, TurnId::new(), "pending")
        .expect("pending turn is valid");
    let projection = SessionProjectionDto::new(
        project_id,
        session_id,
        workspace_id,
        workspace_root(),
        RunModeDto::Plan,
        Some(revision),
        Some(run),
        vec![pending],
    )
    .expect("projection is valid");
    assert_eq!(projection.project_id(), project_id);
    assert_eq!(projection.session_id(), session_id);
    assert_eq!(projection.workspace_id(), workspace_id);
    assert_eq!(projection.workspace_root(), &workspace_root());
    assert_eq!(projection.mode(), RunModeDto::Plan);
    assert_eq!(projection.config_revision_id(), Some(revision));
    assert_eq!(projection.active_run(), Some(run));
    assert_eq!(projection.pending_turns().len(), 1);
    let decoded: SessionProjectionDto =
        serde_json::from_str(&serde_json::to_string(&projection).expect("projection serializes"))
            .expect("projection decodes");
    assert_eq!(decoded, projection);

    let minimal = serde_json::json!({
        "project_id": project_id,
        "session_id": session_id,
        "workspace_id": workspace_id,
        "workspace_root": workspace_root(),
        "mode": "build",
        "pending_turns": []
    });
    let decoded: SessionProjectionDto =
        serde_json::from_value(minimal).expect("optional state defaults to absent");
    assert_eq!(decoded.config_revision_id(), None);
    assert_eq!(decoded.active_run(), None);
    assert!(decoded.pending_turns().is_empty());
}

#[test]
fn run_status_state_machine_accepts_only_declared_edges() {
    let statuses = [
        RunStatusDto::Starting,
        RunStatusDto::Running,
        RunStatusDto::Completed,
        RunStatusDto::Failed,
        RunStatusDto::Interrupted,
    ];
    for from in statuses {
        for to in statuses {
            let expected = matches!(
                (from, to),
                (RunStatusDto::Starting, RunStatusDto::Running)
                    | (RunStatusDto::Starting, RunStatusDto::Failed)
                    | (RunStatusDto::Starting, RunStatusDto::Interrupted)
                    | (RunStatusDto::Running, RunStatusDto::Completed)
                    | (RunStatusDto::Running, RunStatusDto::Failed)
                    | (RunStatusDto::Running, RunStatusDto::Interrupted)
            );
            assert_eq!(
                validate_run_status_transition(from, to).is_ok(),
                expected,
                "unexpected run transition: {from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn transcript_rows_validate_their_closed_kind_shape_and_wire_round_trips() {
    fn read_call(call_id: ToolCallId) -> ToolRowDto {
        ToolRowDto::Call(
            ToolCallRowDto::new(
                call_id,
                ToolIdentityDto::parse("read").expect("the fixture tool is declared"),
                None,
            )
            .expect("fixture call row is valid"),
        )
    }

    fn read_result(call_id: ToolCallId) -> ToolRowDto {
        ToolRowDto::Result(
            ToolResultRowDto::new(
                call_id,
                ToolIdentityDto::parse("read").expect("the fixture tool is declared"),
                ToolResultStatusDto::Completed,
                Some(ToolResult::Read(TextResult {
                    text: BoundedText::new("contents").expect("fixture text is bounded"),
                    truncated: false,
                })),
                Vec::new(),
            )
            .expect("fixture result row is valid"),
        )
    }

    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();
    let message_id = MessageId::new(7).expect("fixture row identity is valid");
    let row =
        |kind: MessageKindDto, text: &str, reasoning: Option<&str>, tool: Option<ToolRowDto>| {
            MessageProjectionDto::new(
                message_id,
                session_id,
                Some(run_id),
                kind,
                text,
                reasoning.map(str::to_owned),
                tool,
            )
        };

    assert!(row(MessageKindDto::User, "hello", None, None).is_ok());
    assert!(row(MessageKindDto::User, " ", None, None).is_err());
    assert!(row(MessageKindDto::User, "hello", Some("thought"), None).is_err());
    assert!(
        row(
            MessageKindDto::User,
            "hello",
            None,
            Some(read_call(call_id))
        )
        .is_err()
    );

    assert!(row(MessageKindDto::Assistant, "answer", Some("reasoning"), None).is_ok());
    assert!(row(MessageKindDto::Assistant, "", None, None).is_err());
    assert!(
        row(MessageKindDto::Assistant, "", Some("reasoning"), None).is_ok(),
        "a reasoning-only step commits its reasoning as its own row"
    );
    assert!(
        row(MessageKindDto::Assistant, "  ", Some("  "), None).is_err(),
        "a row with neither text nor reasoning carries nothing"
    );
    assert!(
        row(
            MessageKindDto::Assistant,
            "answer",
            None,
            Some(read_call(call_id))
        )
        .is_err()
    );

    assert!(
        row(
            MessageKindDto::ToolCall,
            "{}",
            None,
            Some(read_call(call_id))
        )
        .is_ok(),
        "a tool-call row carries its typed call projection"
    );
    assert!(
        row(
            MessageKindDto::ToolCall,
            "{}",
            Some("reasoning"),
            Some(read_call(call_id))
        )
        .is_err(),
        "a call row carries no reasoning text"
    );
    assert!(
        row(MessageKindDto::ToolCall, "{}", None, None).is_err(),
        "a call row without its typed projection is refused"
    );
    assert!(
        row(
            MessageKindDto::ToolCall,
            "{}",
            None,
            Some(read_result(call_id))
        )
        .is_err(),
        "a result projection never answers a call row"
    );

    assert!(
        row(
            MessageKindDto::ToolResult,
            "contents",
            None,
            Some(read_result(call_id))
        )
        .is_ok()
    );
    assert!(
        row(
            MessageKindDto::ToolResult,
            " ",
            None,
            Some(read_result(call_id))
        )
        .is_err(),
        "an answering row carries its content"
    );
    assert!(
        row(MessageKindDto::ToolResult, "contents", None, None).is_err(),
        "a result row without its typed projection is refused"
    );
    assert!(
        row(
            MessageKindDto::ToolResult,
            "contents",
            None,
            Some(read_call(call_id))
        )
        .is_err(),
        "a call projection never answers a result row"
    );

    assert!(row(MessageKindDto::Notice, "notice", None, None).is_ok());
    assert!(
        row(
            MessageKindDto::Notice,
            "notice",
            None,
            Some(read_call(call_id))
        )
        .is_err()
    );

    assert!(row(MessageKindDto::User, "bad\0text", None, None).is_err());
    assert!(
        row(
            MessageKindDto::Assistant,
            "answer",
            Some("bad\0thought"),
            None
        )
        .is_err()
    );
    assert!(
        MessageProjectionDto::new(
            message_id,
            session_id,
            None,
            MessageKindDto::Notice,
            "unbound notice",
            None,
            None,
        )
        .is_ok()
    );

    let round_trip = row(
        MessageKindDto::ToolCall,
        "{\"path\":\"src/lib.rs\"}",
        None,
        Some(read_call(call_id)),
    )
    .expect("tool-call row is valid");
    let decoded: MessageProjectionDto =
        serde_json::from_str(&serde_json::to_string(&round_trip).expect("row serializes"))
            .expect("row decodes");
    assert_eq!(decoded, round_trip);
    assert_eq!(decoded.id(), message_id);
    assert_eq!(decoded.kind(), MessageKindDto::ToolCall);
    assert_eq!(decoded.tool_call_id(), Some(call_id));
    assert_eq!(
        decoded.tool_id().map(ToolIdentityDto::wire_name),
        Some("read")
    );
    assert_eq!(decoded.text(), "{\"path\":\"src/lib.rs\"}");

    let mut additive = serde_json::to_value(&round_trip).expect("row serializes to JSON");
    additive["future_additive_field"] = serde_json::json!(true);
    assert!(serde_json::from_value::<MessageProjectionDto>(additive).is_ok());

    // This wire form omits the optional run_id and reasoning fields and relies
    // on their defaults while carrying its typed tool projection.
    assert!(
        serde_json::from_value::<MessageProjectionDto>(serde_json::json!({
            "id": 7,
            "session_id": session_id,
            "kind": "tool_result",
            "text": "contents",
            "tool": {
                "row": "result",
                "value": {
                    "call_id": call_id,
                    "tool_id": "read",
                    "status": "completed",
                    "result": {
                        "result": "read",
                        "value": {"text": "contents", "truncated": false}
                    }
                }
            }
        }))
        .is_ok()
    );

    let notice = MessageProjectionDto::new(
        message_id,
        session_id,
        None,
        MessageKindDto::Notice,
        "a notice",
        None,
        None,
    )
    .expect("notice row is valid");
    let unbound: MessageProjectionDto =
        serde_json::from_value(serde_json::to_value(&notice).expect("notice serializes"))
            .expect("notice decodes without a run");
    assert_eq!(unbound.run_id(), None);
    assert_eq!(unbound.text(), "a notice");

    assert!(
        serde_json::from_value::<MessageProjectionDto>(serde_json::json!({
            "id": 7,
            "session_id": session_id,
            "kind": "system",
            "text": "unsupported kind"
        }))
        .is_err()
    );
}

/// One committed tool row's typed projection survives the session frame's wire
/// form unchanged, for both a call with typed arguments and a completed result
/// with its status and metadata.
#[test]
fn a_tool_row_projection_round_trips_inside_the_session_frame() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();
    let arguments = ToolInput::from_arguments_json("read", "{\"path\":\"src/lib.rs\"}")
        .expect("fixture arguments decode");
    let metadata = vec![
        ToolResultMetadataEntryDto::new("truncated", "false").expect("fixture metadata is valid"),
    ];
    let rows = [
        (
            MessageKindDto::ToolCall,
            "{\"path\":\"src/lib.rs\"}",
            ToolRowDto::Call(
                ToolCallRowDto::new(
                    call_id,
                    ToolIdentityDto::parse("read").expect("the fixture tool is declared"),
                    Some(arguments),
                )
                .expect("fixture call row is valid"),
            ),
        ),
        (
            MessageKindDto::ToolResult,
            "contents",
            ToolRowDto::Result(
                ToolResultRowDto::new(
                    call_id,
                    ToolIdentityDto::parse("read").expect("the fixture tool is declared"),
                    ToolResultStatusDto::Completed,
                    Some(ToolResult::Read(TextResult {
                        text: BoundedText::new("contents").expect("fixture text is bounded"),
                        truncated: false,
                    })),
                    metadata,
                )
                .expect("fixture result row is valid"),
            ),
        ),
    ];
    for (index, (kind, text, tool)) in rows.into_iter().enumerate() {
        let row = MessageProjectionDto::new(
            MessageId::new(i64::try_from(index).expect("fixture index fits") + 1)
                .expect("fixture row identity is valid"),
            session_id,
            Some(run_id),
            kind,
            text,
            None,
            Some(tool.clone()),
        )
        .expect("fixture row is valid");
        let decoded: MessageProjectionDto =
            serde_json::from_str(&serde_json::to_string(&row).expect("row serializes"))
                .expect("row decodes");
        assert_eq!(decoded, row);
        assert_eq!(decoded.tool(), Some(&tool));
        assert_eq!(decoded.tool_call_id(), Some(call_id));
        assert_eq!(
            decoded.tool_id().map(ToolIdentityDto::wire_name),
            Some("read")
        );
    }
}

/// Every typed argument shape the six declared tools carry decodes into the
/// call row its projection commits, and arguments of another tool's shape are
/// refused rather than attached to the wrong identity.
#[test]
fn a_tool_call_projection_carries_every_typed_argument_shape() {
    let documents = [
        ("read", r#"{"path":"src/lib.rs"}"#),
        (
            "write",
            r#"{"path":"src/lib.rs","content":"new","expected_content":"old"}"#,
        ),
        ("edit", r#"{"path":"src/lib.rs","old":"a","new":"b"}"#),
        (
            "execute",
            r#"{"command":"cargo test","cwd":"src","env":[{"name":"MODE","value":"check"}],"timeout_seconds":5}"#,
        ),
        ("glob", r#"{"pattern":"**/*.rs"}"#),
        (
            "grep",
            r#"{"pattern":"needle","scope":{"kind":"workspace"},"path":"src"}"#,
        ),
    ];
    let mut identities = Vec::new();
    for (tool_id, document) in documents {
        let arguments =
            ToolInput::from_arguments_json(tool_id, document).expect("fixture arguments decode");
        assert_eq!(arguments.tool_id().as_str(), tool_id);
        let call_id = ToolCallId::new();
        let row = ToolRowDto::Call(
            ToolCallRowDto::new(
                call_id,
                ToolIdentityDto::parse(tool_id).expect("the fixture tool is declared"),
                Some(arguments.clone()),
            )
            .expect("fixture call row is valid"),
        );
        assert_eq!(row.call_id(), call_id);
        assert_eq!(row.tool_id().wire_name(), tool_id);
        assert_eq!(
            row.call().and_then(ToolCallRowDto::arguments),
            Some(&arguments)
        );
        let decoded: ToolRowDto =
            serde_json::from_str(&serde_json::to_string(&row).expect("the row serializes"))
                .expect("the row decodes");
        assert_eq!(decoded, row, "{tool_id} round-trips its typed arguments");
        identities.push(row.tool_id().wire_name().to_owned());
    }
    assert_eq!(
        identities,
        ToolId::ALL
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>(),
        "every declared tool carries an argument shape"
    );

    let read = ToolInput::from_arguments_json("read", r#"{"path":"src/lib.rs"}"#)
        .expect("fixture arguments decode");
    assert_eq!(
        ToolCallRowDto::new(
            ToolCallId::new(),
            ToolIdentityDto::parse("glob").expect("the fixture tool is declared"),
            Some(read),
        )
        .expect_err("arguments must belong to the row's declared tool")
        .code(),
        "invalid_tool_projection"
    );
    assert_eq!(
        ToolCallRowDto::new(
            ToolCallId::new(),
            ToolIdentityDto::parse("mystery").expect("an undeclared identity is well-formed"),
            Some(
                ToolInput::from_arguments_json("read", r#"{"path":"src/lib.rs"}"#)
                    .expect("decodes")
            ),
        )
        .expect_err("an undeclared identity carries no typed arguments")
        .code(),
        "invalid_tool_projection"
    );
}

/// One result projection carries the typed payload with the lifecycle status
/// and the approved metadata, and its status rules are closed.
#[test]
fn a_tool_result_projection_carries_status_payload_and_metadata() {
    let call_id = ToolCallId::new();
    let metadata = vec![
        ToolResultMetadataEntryDto::new("truncated", "true").expect("fixture metadata is valid"),
    ];
    let completed = ToolResultRowDto::new(
        call_id,
        ToolIdentityDto::parse("read").expect("the fixture tool is declared"),
        ToolResultStatusDto::Completed,
        Some(ToolResult::Read(TextResult {
            text: BoundedText::new("contents").expect("fixture text is bounded"),
            truncated: true,
        })),
        metadata.clone(),
    )
    .expect("a completed result carries its typed payload");
    assert_eq!(completed.status(), ToolResultStatusDto::Completed);
    assert_eq!(completed.metadata(), metadata.as_slice());
    assert_eq!(
        completed.result(),
        Some(&ToolResult::Read(TextResult {
            text: BoundedText::new("contents").expect("fixture text is bounded"),
            truncated: true,
        }))
    );
    let row = ToolRowDto::Result(completed);
    let decoded: ToolRowDto =
        serde_json::from_str(&serde_json::to_string(&row).expect("the row serializes"))
            .expect("the row decodes");
    assert_eq!(decoded, row);

    let read_payload = ToolResult::Read(TextResult {
        text: BoundedText::new("contents").expect("fixture text is bounded"),
        truncated: false,
    });
    assert_eq!(
        ToolResultRowDto::new(
            call_id,
            ToolIdentityDto::parse("read").expect("the fixture tool is declared"),
            ToolResultStatusDto::Completed,
            None,
            Vec::new(),
        )
        .expect_err("a completed result carries its typed payload")
        .code(),
        "invalid_tool_projection"
    );
    assert_eq!(
        ToolResultRowDto::new(
            call_id,
            ToolIdentityDto::parse("read").expect("the fixture tool is declared"),
            ToolResultStatusDto::Failed,
            Some(read_payload.clone()),
            Vec::new(),
        )
        .expect_err("a failed result carries no payload")
        .code(),
        "invalid_tool_projection"
    );
    assert_eq!(
        ToolResultRowDto::new(
            call_id,
            ToolIdentityDto::parse("glob").expect("the fixture tool is declared"),
            ToolResultStatusDto::Completed,
            Some(read_payload),
            Vec::new(),
        )
        .expect_err("a payload must belong to the row's declared tool")
        .code(),
        "invalid_tool_projection"
    );
    let partial = ToolResultRowDto::new(
        call_id,
        ToolIdentityDto::parse("execute").expect("the fixture tool is declared"),
        ToolResultStatusDto::Partial,
        Some(ToolResult::Execute(ExecuteResult {
            text: TextResult {
                text: BoundedText::new("captured").expect("fixture text is bounded"),
                truncated: false,
            },
            process: None,
        })),
        Vec::new(),
    )
    .expect("a partial result may carry the output it captured");
    assert_eq!(partial.status(), ToolResultStatusDto::Partial);
}
