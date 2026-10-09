#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! DTO contract evidence for the `intention-proto` public API: identity, error, shared
//! model-value wire contracts, the workspace-root and turn-command value contracts, and
//! the run-lifecycle rules.

use intention_proto::{
    CorrelationIdDto, CreateSessionCommandDto, ErrorCategoryDto, ErrorDto, ErrorRetryDto,
    GetSessionSnapshotQueryDto, IdempotencyKey, InterruptRunCommandDto, ProjectId,
    ProviderErrorDto, ProviderProfileId, ProviderProfileOverrideDto, RemoveTurnCommandDto, RunId,
    RunModeDto, RunStatusDto, SendUserTurnCommandDto, SessionId, TimestampDto, ToolCallDto,
    ToolCallId, TurnId, UsageDto, WorkspaceId, WorkspaceRelativePathDto, WorkspaceRootDto,
    run_status_is_terminal,
};

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-proto-contracts-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

#[test]
fn ids_round_trip_as_canonical_uuid_strings() {
    let session_id = SessionId::new();
    let serialized = serde_json::to_string(&session_id).expect("test serialization must succeed");
    let decoded: SessionId =
        serde_json::from_str(&serialized).expect("test deserialization must succeed");

    assert_eq!(decoded, session_id);
    assert_eq!(decoded.to_string(), serialized.trim_matches('"'));
}

#[test]
fn malformed_ids_return_a_typed_validation_error() {
    for invalid in [
        "not-an-id",
        "aaaaaaaaaaaaaaaa4aaa8aaaaaaaaaaaaaaa",
        "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
    ] {
        let error = SessionId::parse(invalid).expect_err("invalid ID must fail");
        assert_eq!(error.category(), ErrorCategoryDto::Validation);
        assert_eq!(error.code(), "invalid_id");
    }
}

#[test]
fn validated_value_dtos_reject_invalid_wire_values() {
    assert!(serde_json::from_str::<TimestampDto>("-1").is_err());
}

#[test]
fn current_shape_minimal_error_decodes_absent_optional_fields_as_none() {
    // The current ErrorDto shape keeps `correlation_id` optional on the wire: a
    // minimal current error without it must decode as None and re-encode
    // without inventing a value.
    let error: ErrorDto = serde_json::from_str(
        r#"{"code":"fixture","category":"not_found","message":"safe","retry":"manual"}"#,
    )
    .expect("minimal current-shape error decodes");
    assert_eq!(error.code(), "fixture");
    assert_eq!(error.category(), ErrorCategoryDto::NotFound);
    assert_eq!(error.retry(), ErrorRetryDto::Manual);
    assert!(
        error.correlation_id().is_none(),
        "absent correlation is None"
    );
    let encoded = serde_json::to_string(&error).expect("test serialization must succeed");
    let decoded: ErrorDto =
        serde_json::from_str(&encoded).expect("round trip must decode identically");
    assert_eq!(decoded, error);
}

#[test]
fn malformed_error_wire_data_is_rejected() {
    for invalid_path in [
        "",
        "/etc/passwd",
        "../escape",
        "src/../escape",
        "src/\u{0000}bad",
    ] {
        assert!(WorkspaceRelativePathDto::parse(invalid_path).is_err());
    }
    let correlation = CorrelationIdDto::new();
    let encoded = serde_json::to_string(&correlation).expect("correlation serializes");
    assert_eq!(
        serde_json::from_str::<CorrelationIdDto>(&encoded).expect("correlation decodes"),
        correlation
    );
    let parsed_path =
        WorkspaceRelativePathDto::parse("src/parsed.rs").expect("fixture path is valid");
    assert_eq!(parsed_path.as_str(), "src/parsed.rs");
    for non_canonical in [
        "aaaaaaaaaaaaaaaa4aaa8aaaaaaaaaaaaaaa",
        "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
    ] {
        assert!(CorrelationIdDto::parse(non_canonical).is_err());
    }

    for wire in [
        r#"{"code":"","category":"not_found","message":"safe","retry":"manual"}"#,
        r#"{"code":"fixture","category":"not_found","message":" ","retry":"manual"}"#,
        r#"{"code":"fixture","category":"not_found","message":"safe","retry":"manual","correlation_id":"not-a-uuid"}"#,
    ] {
        assert!(serde_json::from_str::<ErrorDto>(wire).is_err());
    }
}

#[test]
fn model_values_are_owned_by_types_and_preserve_the_contract() {
    let call = ToolCallDto::new(ToolCallId::new(), "inspect", r#"{"path":"src"}"#)
        .expect("object arguments are valid");
    assert_eq!(call.name(), "inspect");
    assert!(ToolCallDto::new(ToolCallId::new(), "inspect", "[]").is_err());

    let usage = UsageDto::reported(2, 3, 5).expect("consistent usage is valid");
    assert_eq!(
        serde_json::to_string(&usage).expect("usage serializes"),
        r#"{"state":"reported","input_tokens":2,"output_tokens":3,"total_tokens":5}"#
    );
    assert!(UsageDto::reported(u64::MAX, 1, 0).is_err());
    assert!(UsageDto::reported(u64::MAX, 1, u64::MAX).is_err());

    let correlation = CorrelationIdDto::new();
    let error = ProviderErrorDto::unavailable("provider_unavailable", true, Some(correlation))
        .expect("safe provider failure is valid");
    assert_eq!(error.retry(), ErrorRetryDto::Delayed);
    assert_eq!(error.correlation_id(), Some(correlation));
    assert_eq!(error.to_string(), "provider_unavailable");
}

#[test]
fn model_values_reject_invalid_and_decode_valid_wire_forms() {
    assert!(ToolCallDto::new(ToolCallId::new(), " ", "{}").is_err());
    assert!(ToolCallDto::new(ToolCallId::new(), "inspect", "not-json").is_err());
    let call = ToolCallDto::new(ToolCallId::new(), "inspect", "{}").expect("valid call");
    let decoded: ToolCallDto =
        serde_json::from_str(&serde_json::to_string(&call).expect("call serializes"))
            .expect("call decodes");
    assert_eq!(decoded.arguments_json(), "{}");
    assert!(
        serde_json::from_str::<ToolCallDto>(
            r#"{"call_id":"bad","name":"x","arguments_json":"{}"}"#
        )
        .is_err()
    );

    assert_eq!(
        serde_json::from_str::<UsageDto>(r#"{"state":"not_reported"}"#).expect("state decodes"),
        UsageDto::NotReported
    );
    assert!(
        serde_json::from_str::<UsageDto>(
            r#"{"state":"reported","input_tokens":2,"output_tokens":3,"total_tokens":4}"#
        )
        .is_err()
    );

    let error = ProviderErrorDto::unavailable("failed", false, None).expect("valid error");
    assert_eq!(error.retry(), ErrorRetryDto::Never);
    assert!(serde_json::from_str::<ProviderErrorDto>(r#"{"code":"","retry":"never"}"#).is_err());
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
fn send_turn_carries_an_optional_provider_profile_override() {
    let session_id = SessionId::new();
    let command = SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "Explain M1")
        .expect("fixture turn is valid");
    assert!(
        command.provider_profile().is_none(),
        "a turn without an override carries none"
    );
    let override_dto = ProviderProfileOverrideDto::new(
        ProviderProfileId::parse("main").expect("fixture profile identity is valid"),
        None,
    );
    assert_eq!(override_dto.profile_id().as_str(), "main");
    assert_eq!(override_dto.expected_profile_revision_id(), None);

    let overridden = command.with_provider_profile(Some(override_dto.clone()));
    assert_eq!(overridden.provider_profile(), Some(&override_dto));
    let decoded: SendUserTurnCommandDto =
        serde_json::from_str(&serde_json::to_string(&overridden).expect("command serializes"))
            .expect("command decodes");
    assert_eq!(decoded, overridden);

    let defaulted: SendUserTurnCommandDto = serde_json::from_value(serde_json::json!({
        "session_id": session_id,
        "idempotency_key": IdempotencyKey::new(),
        "content": "Explain M1"
    }))
    .expect("a turn without the override field decodes");
    assert!(defaulted.provider_profile().is_none());

    assert!(
        serde_json::from_value::<SendUserTurnCommandDto>(serde_json::json!({
            "session_id": session_id,
            "idempotency_key": IdempotencyKey::new(),
            "content": "Explain M1",
            "provider_profile": {"profile_id": "bad profile id"}
        }))
        .is_err(),
        "a malformed override fails closed"
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
