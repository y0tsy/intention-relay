#![allow(
    clippy::expect_used,
    reason = "M3 contract fixtures use expect for precise test diagnostics."
)]

//! Session, run, and transcript projection contracts.

use intention_domain::{
    PlanStatusDto, validate_plan_status_transition, validate_run_status_transition,
};
use intention_proto::{
    ConfigRevisionId, ProjectId, RunId, SessionId, ToolCallId, TurnId, WorkspaceId,
};
use intention_proto::{
    MessageKindDto, MessageProjectionDto, PendingTurnProjectionDto, RunModeDto, RunProjectionDto,
    RunStatusDto, SessionProjectionDto, WorkspaceRootDto,
};

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-domain-m3-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

#[test]
fn pending_turns_keep_their_order_but_require_unique_identities() {
    let session_id = SessionId::new();
    let first = PendingTurnProjectionDto::new(session_id, TurnId::new(), "first")
        .expect("pending turn is valid");
    let second = PendingTurnProjectionDto::new(session_id, TurnId::new(), "second")
        .expect("pending turn is valid");
    let projection = SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
        None,
        None,
        vec![first.clone(), second],
    )
    .expect("ordered pending turns are coherent");
    assert_eq!(projection.pending_turns().len(), 2);
    assert_eq!(projection.pending_turns()[0].content(), "first");
    assert_eq!(projection.pending_turns()[1].content(), "second");
    assert!(
        SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            workspace_root(),
            RunModeDto::Build,
            None,
            None,
            vec![first.clone(), first],
        )
        .is_err()
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
}

#[test]
fn session_projection_rejects_nested_state_from_a_foreign_session() {
    let session_id = SessionId::new();
    let foreign_turn =
        PendingTurnProjectionDto::new(SessionId::new(), TurnId::new(), "other session")
            .expect("pending fixture is valid");
    assert!(
        SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
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
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            workspace_root(),
            RunModeDto::Build,
            None,
            Some(foreign_run),
            Vec::new(),
        )
        .is_err()
    );

    let local_run = RunProjectionDto::new(
        session_id,
        RunId::new(),
        TurnId::new(),
        RunStatusDto::Starting,
        ConfigRevisionId::new(),
    );
    let local_turn = PendingTurnProjectionDto::new(session_id, TurnId::new(), "local")
        .expect("pending fixture is valid");
    assert!(
        SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            workspace_root(),
            RunModeDto::Build,
            None,
            Some(local_run),
            vec![local_turn],
        )
        .is_ok()
    );
}

#[test]
fn projection_accessors_and_deserialization_cover_optional_state() {
    let project_id = ProjectId::new();
    let session_id = SessionId::new();
    let workspace_id = WorkspaceId::new();
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
fn plan_transitions_accept_allowed_edges_and_reject_undeclared_edges() {
    assert!(validate_plan_status_transition(None, PlanStatusDto::Drafting).is_ok());
    for (from, to) in [
        (PlanStatusDto::Drafting, PlanStatusDto::Revising),
        (PlanStatusDto::Drafting, PlanStatusDto::Submitted),
        (PlanStatusDto::Drafting, PlanStatusDto::Abandoned),
        (PlanStatusDto::Revising, PlanStatusDto::Revising),
        (PlanStatusDto::Revising, PlanStatusDto::Submitted),
        (PlanStatusDto::Revising, PlanStatusDto::Abandoned),
        (PlanStatusDto::Submitted, PlanStatusDto::Approved),
        (PlanStatusDto::Submitted, PlanStatusDto::Rejected),
        (PlanStatusDto::Submitted, PlanStatusDto::Abandoned),
        (PlanStatusDto::Rejected, PlanStatusDto::Revising),
        (PlanStatusDto::Rejected, PlanStatusDto::Abandoned),
    ] {
        assert!(validate_plan_status_transition(Some(from), to).is_ok());
    }
    for status in [
        PlanStatusDto::Approved,
        PlanStatusDto::Superseded,
        PlanStatusDto::Abandoned,
    ] {
        assert!(validate_plan_status_transition(Some(status), PlanStatusDto::Drafting).is_err());
    }
    for (from, to) in [
        (None, PlanStatusDto::Submitted),
        (None, PlanStatusDto::Approved),
        (Some(PlanStatusDto::Drafting), PlanStatusDto::Approved),
        (Some(PlanStatusDto::Drafting), PlanStatusDto::Superseded),
        (Some(PlanStatusDto::Submitted), PlanStatusDto::Drafting),
        (Some(PlanStatusDto::Submitted), PlanStatusDto::Superseded),
        (Some(PlanStatusDto::Rejected), PlanStatusDto::Submitted),
        (Some(PlanStatusDto::Superseded), PlanStatusDto::Abandoned),
    ] {
        assert!(validate_plan_status_transition(from, to).is_err());
    }
}

#[test]
fn transcript_rows_validate_their_closed_kind_shape() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();
    let row = |kind: MessageKindDto,
               text: &str,
               reasoning: Option<&str>,
               tool_call_id: Option<ToolCallId>,
               tool_id: Option<&str>| {
        MessageProjectionDto::new(
            session_id,
            Some(run_id),
            kind,
            text,
            reasoning.map(str::to_owned),
            tool_call_id,
            tool_id.map(str::to_owned),
        )
    };

    assert!(row(MessageKindDto::User, "hello", None, None, None).is_ok());
    assert!(row(MessageKindDto::User, " ", None, None, None).is_err());
    assert!(row(MessageKindDto::User, "hello", Some("thought"), None, None).is_err());
    assert!(
        row(
            MessageKindDto::User,
            "hello",
            None,
            Some(call_id),
            Some("read")
        )
        .is_err()
    );

    assert!(
        row(
            MessageKindDto::Assistant,
            "answer",
            Some("reasoning"),
            None,
            None
        )
        .is_ok()
    );
    assert!(row(MessageKindDto::Assistant, "", None, None, None).is_err());
    assert!(
        row(
            MessageKindDto::Assistant,
            "answer",
            None,
            Some(call_id),
            Some("read")
        )
        .is_err()
    );

    assert!(
        row(
            MessageKindDto::ToolCall,
            "{}",
            None,
            Some(call_id),
            Some("read")
        )
        .is_ok()
    );
    assert!(
        row(
            MessageKindDto::ToolCall,
            "{}",
            Some("reasoning"),
            Some(call_id),
            Some("read")
        )
        .is_err()
    );
    assert!(row(MessageKindDto::ToolCall, "{}", None, None, Some("read")).is_err());
    assert!(
        row(
            MessageKindDto::ToolCall,
            "{}",
            None,
            Some(call_id),
            Some(" ")
        )
        .is_err()
    );

    assert!(
        row(
            MessageKindDto::ToolResult,
            "contents",
            None,
            Some(call_id),
            Some("read")
        )
        .is_ok()
    );
    assert!(
        row(
            MessageKindDto::ToolResult,
            " ",
            None,
            Some(call_id),
            Some("read")
        )
        .is_err()
    );
    assert!(row(MessageKindDto::ToolResult, "contents", None, None, None).is_err());

    assert!(row(MessageKindDto::Notice, "notice", None, None, None).is_ok());
    assert!(
        row(
            MessageKindDto::Notice,
            "notice",
            None,
            Some(call_id),
            Some("read")
        )
        .is_err()
    );

    assert!(row(MessageKindDto::User, "bad\0text", None, None, None).is_err());
    assert!(
        row(
            MessageKindDto::Assistant,
            "answer",
            Some("bad\0thought"),
            None,
            None
        )
        .is_err()
    );
    assert!(
        MessageProjectionDto::new(
            session_id,
            None,
            MessageKindDto::Notice,
            "unbound notice",
            None,
            None,
            None,
        )
        .is_ok()
    );
}

#[test]
fn transcript_rows_round_trip_and_reject_invalid_wire_shapes() {
    let session_id = SessionId::new();
    let call_id = ToolCallId::new();
    let row = MessageProjectionDto::new(
        session_id,
        Some(RunId::new()),
        MessageKindDto::ToolCall,
        "{\"path\":\"src/lib.rs\"}",
        None,
        Some(call_id),
        Some("read".to_owned()),
    )
    .expect("tool-call row is valid");
    let decoded: MessageProjectionDto =
        serde_json::from_str(&serde_json::to_string(&row).expect("row serializes"))
            .expect("row decodes");
    assert_eq!(decoded, row);
    assert_eq!(decoded.kind(), MessageKindDto::ToolCall);
    assert_eq!(decoded.tool_call_id(), Some(call_id));
    assert_eq!(decoded.tool_id(), Some("read"));
    assert_eq!(decoded.text(), "{\"path\":\"src/lib.rs\"}");

    let mut additive = serde_json::to_value(&row).expect("row serializes to JSON");
    additive["future_additive_field"] = serde_json::json!(true);
    assert!(serde_json::from_value::<MessageProjectionDto>(additive).is_ok());

    let notice = MessageProjectionDto::new(
        session_id,
        None,
        MessageKindDto::Notice,
        "a notice",
        None,
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
            "session_id": session_id,
            "kind": "user",
            "text": " "
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<MessageProjectionDto>(serde_json::json!({
            "session_id": session_id,
            "kind": "tool_result",
            "text": "contents",
            "tool_call_id": call_id,
            "tool_id": "read"
        }))
        .is_ok()
    );
    assert!(
        serde_json::from_value::<MessageProjectionDto>(serde_json::json!({
            "session_id": session_id,
            "kind": "tool_call",
            "text": "{}",
            "tool_call_id": call_id,
            "tool_id": " "
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<MessageProjectionDto>(serde_json::json!({
            "session_id": session_id,
            "kind": "system",
            "text": "unsupported kind"
        }))
        .is_err()
    );
}
