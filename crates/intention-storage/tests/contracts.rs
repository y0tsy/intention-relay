#![allow(
    clippy::expect_used,
    reason = "Storage contract fixtures use expect for precise test diagnostics."
)]

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::time;

use intention_config::ConfigSnapshotDto;
use intention_domain::{ToolResultMetadataEntryDto, ToolResultStatusDto};
use intention_proto::{MessageKindDto, MessageProjectionDto, RunId, SessionId, ToolCallId};
use intention_storage::{StartingRunModelContextDto, ToolResultEvidenceDto};

fn snapshot() -> ConfigSnapshotDto {
    serde_json::from_str(include_str!(
        "../../intention-config/tests/fixtures/config-snapshot-v1.json"
    ))
    .expect("safe config snapshot decodes")
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
            ToolResultStatusDto::Completed,
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
}

#[test]
fn starting_run_model_context_validates_its_committed_values() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let message = user_message(session_id, run_id, "started");
    let context = StartingRunModelContextDto::new(session_id, run_id, snapshot(), vec![message])
        .expect("starting context is valid");
    assert_eq!(context.session_id(), session_id);
    assert_eq!(context.run_id(), run_id);
    assert_eq!(
        context.messages().last().map(MessageProjectionDto::text),
        Some("started")
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
}
