#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first domain DTO and invariant evidence.

use intention_domain::{
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
fn send_turn_requires_a_non_empty_message_and_a_typed_idempotency_key() {
    let session_id = SessionId::new();
    let idempotency_key = IdempotencyKey::new();
    let command = SendUserTurnCommandDto::new(session_id, idempotency_key, "Explain M1")
        .expect("non-empty fixture message is valid");

    assert_eq!(command.session_id(), session_id);
    assert_eq!(command.idempotency_key(), idempotency_key);
    assert_eq!(command.content(), "Explain M1");
    assert_ne!(idempotency_key, IdempotencyKey::new());
    assert!(SendUserTurnCommandDto::new(SessionId::new(), IdempotencyKey::new(), "   ").is_err());
}

#[test]
fn send_turn_wire_values_require_content_and_idempotency_key() {
    let command =
        SendUserTurnCommandDto::new(SessionId::new(), IdempotencyKey::new(), "Explain M1")
            .expect("non-empty fixture message is valid");
    let decoded: SendUserTurnCommandDto =
        serde_json::from_str(&serde_json::to_string(&command).expect("command serializes"))
            .expect("command decodes");
    assert_eq!(decoded, command);

    assert!(
        serde_json::from_value::<SendUserTurnCommandDto>(serde_json::json!({
            "session_id": command.session_id(),
            "idempotency_key": command.idempotency_key(),
            "content": " "
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SendUserTurnCommandDto>(serde_json::json!({
            "session_id": command.session_id(),
            "content": "Explain M1"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SendUserTurnCommandDto>(serde_json::json!({
            "session_id": command.session_id(),
            "idempotency_key": command.idempotency_key(),
            "content": "Explain M1",
            "future_additive_field": true
        }))
        .is_ok()
    );
}

#[test]
fn run_status_terminal_classification_covers_all_statuses() {
    use intention_domain::RunStatusDto as S;
    let all = [
        S::Starting,
        S::Running,
        S::Completed,
        S::Failed,
        S::Interrupted,
    ];
    for status in all {
        assert_eq!(
            status.is_terminal(),
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
fn session_commands_round_trip_with_typed_identity_and_mode() {
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
}

#[test]
fn turn_run_and_snapshot_commands_keep_typed_identity() {
    let session_id = SessionId::new();
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
