#![allow(
    clippy::expect_used,
    reason = "Storage contract fixtures use expect for precise test diagnostics."
)]

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    CreateSessionCommandDto, MessageKindDto, MessageProjectionDto, PendingTurnProjectionDto,
    RemoveTurnCommandDto, RunModeDto, RunProjectionDto, RunStatusDto, ToolResultMetadataEntryDto,
    ToolResultStatusDto, WorkspaceRootDto,
};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, AppendMessageInputDto,
    ConsumePendingUserTurnsInputDto, CreateSessionInputDto, FinishRunInputDto,
    RecoverUnfinishedRunsInputDto, RemoveTurnInputDto, StartingRunModelContextDto,
    ToolResultEvidenceDto, TransitionRunInputDto, WriteToolResultInputDto,
};
use intention_types::{
    ConfigRevisionId, ErrorCategoryDto, FinishReasonDto, IdempotencyKey, ProjectId, RunId,
    SessionId, TimestampDto, ToolCallId, TurnId, UsageDto, WorkspaceId,
};

fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture time is valid")
}

fn snapshot() -> ConfigSnapshotDto {
    serde_json::from_str(include_str!(
        "../../intention-config/tests/fixtures/config-snapshot-v1.json"
    ))
    .expect("safe config snapshot decodes")
}

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-storage-contracts-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

fn user_message(session_id: SessionId, run_id: RunId, text: &str) -> MessageProjectionDto {
    MessageProjectionDto::new(
        session_id,
        Some(run_id),
        MessageKindDto::User,
        text,
        None,
        None,
        None,
    )
    .expect("fixture user row is valid")
}

fn evidence(session_id: SessionId, run_id: RunId, call_id: ToolCallId) -> ToolResultEvidenceDto {
    ToolResultEvidenceDto::new(
        session_id,
        run_id,
        call_id,
        "read",
        ToolResultStatusDto::Completed,
        r#"{"result":"read"}"#,
        vec![ToolResultMetadataEntryDto::new("truncated", "false").expect("metadata is valid")],
        time(3),
    )
    .expect("fixture evidence is valid")
}

#[test]
fn create_and_turn_inputs_expose_their_typed_fields() {
    let created_at = time(1);
    let session_id = SessionId::new();
    let workspace_id = WorkspaceId::new();
    let project_id = ProjectId::new();
    let command = CreateSessionCommandDto::new(
        project_id,
        session_id,
        workspace_id,
        workspace_root(),
        RunModeDto::Build,
    );
    assert_eq!(command.project_id(), project_id);
    assert_eq!(command.session_id(), session_id);
    assert_eq!(command.workspace_id(), workspace_id);
    assert_eq!(command.workspace_root(), &workspace_root());
    assert_eq!(command.mode(), RunModeDto::Build);
    let create = CreateSessionInputDto::new(command, created_at);
    assert_eq!(create.command().session_id(), session_id);
    assert_eq!(create.occurred_at(), created_at);

    let snapshot = snapshot();
    let key = IdempotencyKey::new();
    let run_id = RunId::new();
    let turn = AcceptUserTurnInputDto::new(
        session_id,
        key,
        "hello",
        run_id,
        snapshot.clone(),
        created_at,
    )
    .expect("turn input is valid");
    assert_eq!(turn.session_id(), session_id);
    assert_eq!(turn.idempotency_key(), key);
    assert_eq!(turn.content(), "hello");
    assert_eq!(turn.proposed_run_id(), run_id);
    assert_eq!(turn.config_snapshot(), &snapshot);
    assert_eq!(turn.config_revision_id(), snapshot.revision_id());
    assert_eq!(turn.occurred_at(), created_at);
    assert_eq!(
        turn.config_revision_id(),
        ConfigRevisionId::parse("44444444-4444-4444-8444-444444444444")
            .expect("fixture id is valid")
    );
    assert_eq!(
        AcceptUserTurnInputDto::new(session_id, key, " ", run_id, snapshot, created_at,)
            .expect_err("blank turn content rejects")
            .code(),
        "invalid_turn_content"
    );
}

#[test]
fn turn_and_run_inputs_are_typed_and_ordered() {
    let at = time(2);
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let remove = RemoveTurnInputDto::new(RemoveTurnCommandDto::new(session_id, TurnId::new()), at);
    assert_eq!(remove.command().session_id(), session_id);
    assert_eq!(remove.command().turn_id().to_string().len(), 36);
    assert_eq!(remove.occurred_at(), at);

    let consume = ConsumePendingUserTurnsInputDto::new(session_id, run_id, at);
    assert_eq!(consume.session_id(), session_id);
    assert_eq!(consume.run_id(), run_id);
    assert_eq!(consume.occurred_at(), at);

    let transition = TransitionRunInputDto::new(session_id, run_id, RunStatusDto::Running, at);
    assert_eq!(transition.session_id(), session_id);
    assert_eq!(transition.run_id(), run_id);
    assert_eq!(transition.status(), RunStatusDto::Running);
    assert_eq!(transition.occurred_at(), at);

    assert_eq!(RecoverUnfinishedRunsInputDto::new(at).recovered_at(), at);
}

#[test]
fn finish_run_input_requires_a_terminal_safe_outcome() {
    let at = time(3);
    let session_id = SessionId::new();
    let run_id = RunId::new();
    for status in [RunStatusDto::Starting, RunStatusDto::Running] {
        assert_eq!(
            FinishRunInputDto::new(session_id, run_id, status, None, None, None, None, at)
                .expect_err("a non-terminal outcome rejects")
                .code(),
            "invalid_run_outcome"
        );
    }
    assert_eq!(
        FinishRunInputDto::new(
            session_id,
            run_id,
            RunStatusDto::Failed,
            None,
            None,
            Some("provider_failed".to_owned()),
            None,
            at,
        )
        .expect_err("an incomplete error pair rejects")
        .code(),
        "invalid_run_outcome"
    );
    assert_eq!(
        FinishRunInputDto::new(
            session_id,
            run_id,
            RunStatusDto::Failed,
            None,
            None,
            Some("provider_failed".to_owned()),
            Some("unsafe\0message".to_owned()),
            at,
        )
        .expect_err("unsafe error text rejects")
        .code(),
        "invalid_run_outcome"
    );
    let usage = UsageDto::reported(2, 3, 5).expect("fixture usage is consistent");
    let finish = FinishRunInputDto::new(
        session_id,
        run_id,
        RunStatusDto::Completed,
        Some(usage),
        Some(FinishReasonDto::Stop),
        None,
        None,
        at,
    )
    .expect("terminal outcome is valid");
    assert_eq!(finish.session_id(), session_id);
    assert_eq!(finish.run_id(), run_id);
    assert_eq!(finish.status(), RunStatusDto::Completed);
    assert_eq!(finish.usage(), Some(&usage));
    assert_eq!(finish.finish_reason(), Some(FinishReasonDto::Stop));
    assert_eq!(finish.error_code(), None);
    assert_eq!(finish.error_message(), None);
    assert_eq!(finish.occurred_at(), at);
}

#[test]
fn append_and_tool_result_inputs_keep_closed_shapes() {
    let at = time(4);
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();
    let message = user_message(session_id, run_id, "appended");
    let append = AppendMessageInputDto::new(message.clone(), at);
    assert_eq!(append.message(), &message);
    assert_eq!(append.occurred_at(), at);

    let result = evidence(session_id, run_id, call_id);
    let answering = MessageProjectionDto::new(
        session_id,
        Some(run_id),
        MessageKindDto::ToolResult,
        result.content(),
        None,
        Some(call_id),
        Some("read".to_owned()),
    )
    .expect("answering row is valid");
    let write = WriteToolResultInputDto::new(result.clone(), answering.clone())
        .expect("matching evidence and answer commit together");
    assert_eq!(write.evidence(), &result);
    assert_eq!(write.message(), &answering);

    for mismatched in [
        MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            "not a result",
            None,
            None,
            None,
        )
        .expect("assistant row is valid"),
        MessageProjectionDto::new(
            SessionId::new(),
            Some(run_id),
            MessageKindDto::ToolResult,
            result.content(),
            None,
            Some(call_id),
            Some("read".to_owned()),
        )
        .expect("cross-session row is structurally valid"),
        MessageProjectionDto::new(
            session_id,
            Some(RunId::new()),
            MessageKindDto::ToolResult,
            result.content(),
            None,
            Some(call_id),
            Some("read".to_owned()),
        )
        .expect("cross-run row is structurally valid"),
        MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::ToolResult,
            result.content(),
            None,
            Some(ToolCallId::new()),
            Some("read".to_owned()),
        )
        .expect("cross-call row is structurally valid"),
        MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::ToolResult,
            result.content(),
            None,
            Some(call_id),
            Some("glob".to_owned()),
        )
        .expect("cross-tool row is structurally valid"),
    ] {
        assert_eq!(
            WriteToolResultInputDto::new(result.clone(), mismatched)
                .expect_err("a tool result commits only with its own answering row")
                .code(),
            "invalid_tool_result"
        );
    }
}

#[test]
fn tool_result_evidence_is_bounded_and_typed() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();
    assert_eq!(
        ToolResultEvidenceDto::new(
            session_id,
            run_id,
            call_id,
            " ",
            ToolResultStatusDto::Completed,
            "content",
            Vec::new(),
            time(3),
        )
        .expect_err("a blank tool identity rejects")
        .code(),
        "invalid_tool_result"
    );
    assert_eq!(
        ToolResultEvidenceDto::new(
            session_id,
            run_id,
            call_id,
            "read",
            ToolResultStatusDto::Partial,
            " ",
            Vec::new(),
            time(3),
        )
        .expect_err("blank content rejects")
        .code(),
        "invalid_tool_result"
    );
    assert_eq!(
        ToolResultEvidenceDto::new(
            session_id,
            run_id,
            call_id,
            "read",
            ToolResultStatusDto::Failed,
            "nul\0byte",
            Vec::new(),
            time(3),
        )
        .expect_err("unsafe content rejects")
        .code(),
        "invalid_tool_result"
    );
    assert_eq!(
        ToolResultEvidenceDto::new(
            session_id,
            run_id,
            call_id,
            "read",
            ToolResultStatusDto::Failed,
            "x".repeat(512 * 1024 + 1),
            Vec::new(),
            time(3),
        )
        .expect_err("oversized content rejects")
        .code(),
        "invalid_tool_result"
    );
    assert_eq!(
        ToolResultEvidenceDto::new(
            session_id,
            run_id,
            call_id,
            "read",
            ToolResultStatusDto::Cancelled,
            "content",
            vec![
                ToolResultMetadataEntryDto::new("truncated", "false").expect("metadata is valid"),
                ToolResultMetadataEntryDto::new("truncated", "true").expect("metadata is valid"),
            ],
            time(3),
        )
        .expect_err("duplicate metadata keys reject")
        .code(),
        "invalid_tool_result_metadata"
    );
    let result = evidence(session_id, run_id, call_id);
    assert_eq!(result.session_id(), session_id);
    assert_eq!(result.run_id(), run_id);
    assert_eq!(result.call_id(), call_id);
    assert_eq!(result.tool_id(), "read");
    assert_eq!(result.status(), ToolResultStatusDto::Completed);
    assert_eq!(result.content(), r#"{"result":"read"}"#);
    assert_eq!(result.metadata().len(), 1);
    assert_eq!(result.metadata()[0].key(), "truncated");
    assert_eq!(result.metadata()[0].value(), "false");
    assert_eq!(result.occurred_at(), time(3));
}

#[test]
fn accepted_turn_outcome_and_model_context_expose_committed_values() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let turn_id = TurnId::new();
    let run = RunProjectionDto::new(
        session_id,
        run_id,
        turn_id,
        RunStatusDto::Starting,
        ConfigRevisionId::new(),
    );
    let message = user_message(session_id, run_id, "started");
    let started = AcceptedTurnOutcomeDto::Started {
        run,
        message: message.clone(),
    };
    assert_eq!(started.started_run(), Some(run));
    assert_eq!(started.started_message(), Some(&message));
    assert_eq!(started.pending_turn(), None);

    let pending_turn = PendingTurnProjectionDto::new(session_id, turn_id, "queued")
        .expect("pending turn is valid");
    let queued = AcceptedTurnOutcomeDto::Pending(pending_turn.clone());
    assert_eq!(queued.started_run(), None);
    assert_eq!(queued.started_message(), None);
    assert_eq!(queued.pending_turn(), Some(&pending_turn));

    let context = StartingRunModelContextDto::new(session_id, run_id, snapshot(), vec![message])
        .expect("starting context is valid");
    assert_eq!(context.session_id(), session_id);
    assert_eq!(context.run_id(), run_id);
    assert_eq!(
        context.messages().last().map(MessageProjectionDto::text),
        Some("started")
    );
    assert_eq!(
        context.safe_config().revision_id(),
        snapshot().revision_id()
    );
    assert_eq!(
        StartingRunModelContextDto::new(session_id, run_id, snapshot(), Vec::new())
            .expect_err("an empty context rejects")
            .code(),
        "invalid_model_context"
    );
    assert_eq!(
        StartingRunModelContextDto::new(
            session_id,
            run_id,
            snapshot(),
            vec![user_message(SessionId::new(), run_id, "foreign")],
        )
        .expect_err("a cross-session message rejects")
        .code(),
        "invalid_model_context"
    );
    assert_eq!(
        StartingRunModelContextDto::new(
            session_id,
            run_id,
            snapshot(),
            vec![
                MessageProjectionDto::new(
                    session_id,
                    Some(run_id),
                    MessageKindDto::Notice,
                    "notice",
                    None,
                    None,
                    None,
                )
                .expect("notice row is valid"),
            ],
        )
        .expect_err("a context that does not end with the starting user turn rejects")
        .code(),
        "invalid_model_context"
    );
    assert_eq!(
        StartingRunModelContextDto::new(
            session_id,
            run_id,
            snapshot(),
            vec![
                MessageProjectionDto::new(
                    session_id,
                    None,
                    MessageKindDto::User,
                    "session-level",
                    None,
                    None,
                    None,
                )
                .expect("session row is valid"),
            ],
        )
        .expect_err("the final message must belong to the starting run")
        .code(),
        "invalid_model_context"
    );
    assert_eq!(
        ErrorCategoryDto::Validation.as_str(),
        "validation",
        "the fixture also pins the typed error category vocabulary"
    );
}
