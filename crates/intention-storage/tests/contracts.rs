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
use intention_proto::{MessageKindDto, MessageProjectionDto, RunId, SessionId, ToolCallId};
use intention_storage::{
    StartingRunModelContextDto, ToolResultEvidenceDto, ToolResultMetadataEntryDto,
    ToolResultStatusDto,
};

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

#[test]
fn tool_result_status_set_is_closed_to_terminal_outcomes() {
    for status in [
        ToolResultStatusDto::Completed,
        ToolResultStatusDto::Failed,
        ToolResultStatusDto::Partial,
    ] {
        let wire = serde_json::to_string(&status).expect("status serializes");
        let decoded: ToolResultStatusDto = serde_json::from_str(&wire).expect("status decodes");
        assert_eq!(decoded, status);
    }
    assert_eq!(
        serde_json::to_value(ToolResultStatusDto::Completed).expect("status serializes to JSON"),
        serde_json::json!("completed")
    );
    assert_eq!(
        serde_json::to_value(ToolResultStatusDto::Failed).expect("status serializes to JSON"),
        serde_json::json!("failed")
    );
    assert_eq!(
        serde_json::to_value(ToolResultStatusDto::Partial).expect("status serializes to JSON"),
        serde_json::json!("partial")
    );
    for undeclared in ["started", "admitted", "rejected"] {
        assert!(serde_json::from_str::<ToolResultStatusDto>(&format!("\"{undeclared}\"")).is_err());
    }
}

#[test]
fn tool_result_metadata_entries_validate_keys_and_keep_a_closed_wire_shape() {
    let entry =
        ToolResultMetadataEntryDto::new("bytes", "17").expect("bounded metadata entry is valid");
    assert_eq!(entry.key(), "bytes");
    assert_eq!(entry.value(), "17");
    let decoded: ToolResultMetadataEntryDto =
        serde_json::from_str(&serde_json::to_string(&entry).expect("entry serializes"))
            .expect("entry decodes");
    assert_eq!(decoded, entry);

    assert!(ToolResultMetadataEntryDto::new(" ", "v").is_err());
    assert!(ToolResultMetadataEntryDto::new("bad\0key", "v").is_err());
    assert!(ToolResultMetadataEntryDto::new("k", "bad\0value").is_err());

    let empty_value =
        ToolResultMetadataEntryDto::new("k", "").expect("an empty metadata value is allowed");
    assert_eq!(empty_value.value(), "");
    // Keys and values beyond the former 128-byte and 1 KiB caps are accepted
    // and preserved exactly.
    let long_key =
        ToolResultMetadataEntryDto::new("x".repeat(129), "v").expect("129-byte key is accepted");
    assert_eq!(long_key.key().len(), 129);
    let long_value =
        ToolResultMetadataEntryDto::new("k", "x".repeat(1025)).expect("1 KiB + 1 value");
    assert_eq!(long_value.value().len(), 1025);

    // The metadata entry persists exactly the documented typed fields.
    let encoded = serde_json::to_value(&entry).expect("entry serializes to JSON");
    assert_eq!(encoded, serde_json::json!({"key": "bytes", "value": "17"}));
    assert_eq!(
        encoded.as_object().expect("entry is a JSON object").len(),
        2
    );

    let mut additive = encoded;
    additive["future_additive_field"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ToolResultMetadataEntryDto>(additive).is_ok());

    assert!(
        serde_json::from_value::<ToolResultMetadataEntryDto>(serde_json::json!({"value": "17"}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<ToolResultMetadataEntryDto>(serde_json::json!({"key": "bytes"}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<ToolResultMetadataEntryDto>(
            serde_json::json!({"key": " ", "value": "17"})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ToolResultMetadataEntryDto>(
            serde_json::json!({"key": "bytes", "value": "bad\0value"})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ToolResultMetadataEntryDto>(
            serde_json::json!({"key": 7, "value": "17"})
        )
        .is_err()
    );
}
