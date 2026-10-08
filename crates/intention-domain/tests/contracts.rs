#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first domain DTO and invariant evidence.

use intention_domain::{ToolResultMetadataEntryDto, ToolResultStatusDto, run_status_is_terminal};
use intention_proto::{
    CreateSessionCommandDto, GetSessionSnapshotQueryDto, InterruptRunCommandDto,
    RemoveTurnCommandDto, RunModeDto, RunStatusDto, SendUserTurnCommandDto, WorkspaceRootDto,
};
use intention_proto::{IdempotencyKey, ProjectId, RunId, SessionId, TurnId, WorkspaceId};

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-domain-contracts-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

#[test]
fn workspace_root_accepts_only_absolute_non_empty_native_paths() {
    let root = workspace_root();
    assert!(std::path::Path::new(root.as_str()).is_absolute());
    assert_eq!(root.as_str(), root.to_string());
    let decoded: WorkspaceRootDto =
        serde_json::from_str(&serde_json::to_string(&root).expect("root serializes"))
            .expect("root decodes");
    assert_eq!(decoded, root);

    #[cfg(unix)]
    assert!(WorkspaceRootDto::parse("/workspace/project").is_ok());
    #[cfg(windows)]
    {
        assert!(WorkspaceRootDto::parse(r"C:\workspace\project").is_ok());
        assert!(WorkspaceRootDto::parse(r"\\server\share\project").is_ok());
        assert!(WorkspaceRootDto::parse(r"C:workspace\project").is_err());
    }
    assert!(WorkspaceRootDto::parse("").is_err());
    assert!(WorkspaceRootDto::parse("   ").is_err());
    assert!(WorkspaceRootDto::parse("relative/project").is_err());
    assert!(serde_json::from_str::<WorkspaceRootDto>(r#""relative/project""#).is_err());
    assert!(serde_json::from_str::<WorkspaceRootDto>(r#""""#).is_err());
}

#[test]
fn send_turn_requires_content_and_typed_identity() {
    let session_id = SessionId::new();
    let idempotency_key = IdempotencyKey::new();
    let command = SendUserTurnCommandDto::new(session_id, idempotency_key, "Explain M1")
        .expect("non-empty fixture message is valid");

    assert_eq!(command.session_id(), session_id);
    assert_eq!(command.idempotency_key(), idempotency_key);
    assert_eq!(command.content(), "Explain M1");
    assert_ne!(idempotency_key, IdempotencyKey::new());
    assert!(SendUserTurnCommandDto::new(SessionId::new(), IdempotencyKey::new(), "   ").is_err());

    let decoded: SendUserTurnCommandDto =
        serde_json::from_str(&serde_json::to_string(&command).expect("command serializes"))
            .expect("command decodes");
    assert_eq!(decoded, command);

    assert!(
        serde_json::from_value::<SendUserTurnCommandDto>(serde_json::json!({
            "session_id": session_id,
            "idempotency_key": idempotency_key,
            "content": " "
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SendUserTurnCommandDto>(serde_json::json!({
            "session_id": session_id,
            "content": "Explain M1"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SendUserTurnCommandDto>(serde_json::json!({
            "session_id": session_id,
            "idempotency_key": idempotency_key,
            "content": "Explain M1",
            "future_additive_field": true
        }))
        .is_ok()
    );
}

#[test]
fn run_status_terminal_classification_covers_all_statuses() {
    use intention_proto::RunStatusDto as S;
    let all = [
        S::Starting,
        S::Running,
        S::Completed,
        S::Failed,
        S::Interrupted,
    ];
    for status in all {
        assert_eq!(
            run_status_is_terminal(status),
            matches!(status, S::Completed | S::Failed | S::Interrupted)
        );
        let wire = serde_json::to_string(&status).expect("status serializes");
        let decoded: RunStatusDto = serde_json::from_str(&wire).expect("status decodes");
        assert_eq!(decoded, status);
    }
    for retired in ["queued", "completing", "waiting_input"] {
        assert!(serde_json::from_str::<RunStatusDto>(&format!("\"{retired}\"")).is_err());
    }
}

#[test]
fn commands_round_trip_with_typed_identity_and_mode() {
    let project_id = ProjectId::new();
    let session_id = SessionId::new();
    let workspace_id = WorkspaceId::new();
    let command = CreateSessionCommandDto::new(
        project_id,
        session_id,
        workspace_id,
        workspace_root(),
        RunModeDto::Plan,
    );

    assert_eq!(command.project_id(), project_id);
    assert_eq!(command.session_id(), session_id);
    assert_eq!(command.workspace_id(), workspace_id);
    assert_eq!(command.workspace_root(), &workspace_root());
    assert_eq!(command.mode(), RunModeDto::Plan);

    let decoded: CreateSessionCommandDto =
        serde_json::from_str(&serde_json::to_string(&command).expect("command serializes"))
            .expect("command decodes");
    assert_eq!(decoded, command);
    assert_eq!(
        serde_json::to_value(command.mode()).expect("mode serializes"),
        serde_json::json!("plan")
    );

    let turn_id = TurnId::new();
    let run_id = RunId::new();

    let removal = RemoveTurnCommandDto::new(session_id, turn_id);
    assert_eq!(removal.session_id(), session_id);
    assert_eq!(removal.turn_id(), turn_id);
    let decoded: RemoveTurnCommandDto =
        serde_json::from_str(&serde_json::to_string(&removal).expect("removal serializes"))
            .expect("removal decodes");
    assert_eq!(decoded, removal);

    let interrupt = InterruptRunCommandDto::new(session_id, run_id);
    assert_eq!(interrupt.session_id(), session_id);
    assert_eq!(interrupt.run_id(), run_id);
    let decoded: InterruptRunCommandDto =
        serde_json::from_str(&serde_json::to_string(&interrupt).expect("interrupt serializes"))
            .expect("interrupt decodes");
    assert_eq!(decoded, interrupt);

    let query = GetSessionSnapshotQueryDto::new(session_id);
    assert_eq!(query.session_id(), session_id);
    let decoded: GetSessionSnapshotQueryDto =
        serde_json::from_str(&serde_json::to_string(&query).expect("query serializes"))
            .expect("query decodes");
    assert_eq!(decoded, query);
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
