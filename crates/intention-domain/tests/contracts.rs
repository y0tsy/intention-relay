#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Test-first domain DTO and invariant evidence.

use intention_domain::{
    DomainEventDto, RunModeDto, SendUserTurnCommandDto, SessionCreatedEventDto, WorkspaceRootDto,
};
use intention_types::{ProjectId, SessionId, TimestampDto, TurnId, WorkspaceId};

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
fn send_turn_requires_a_non_empty_message_and_typed_session_identity() {
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let command = SendUserTurnCommandDto::new(session_id, turn_id, "Explain M1")
        .expect("non-empty fixture message is valid");

    assert_eq!(command.session_id(), session_id);
    assert_eq!(command.turn_id(), turn_id);
    assert_eq!(command.content(), "Explain M1");
    assert!(SendUserTurnCommandDto::new(SessionId::new(), TurnId::new(), "   ").is_err());
}

#[test]
fn workspace_root_and_turn_wire_values_enforce_validation() {
    #[cfg(unix)]
    assert!(WorkspaceRootDto::parse("/workspace/project").is_ok());
    #[cfg(windows)]
    {
        assert!(WorkspaceRootDto::parse(r"C:\workspace\project").is_ok());
        assert!(WorkspaceRootDto::parse(r"\\server\share\project").is_ok());
        assert!(WorkspaceRootDto::parse(r"C:workspace\project").is_err());
    }
    assert!(WorkspaceRootDto::parse("").is_err());
    assert!(WorkspaceRootDto::parse("relative/project").is_err());
    assert!(serde_json::from_str::<WorkspaceRootDto>(r#""relative/project""#).is_err());

    assert!(serde_json::from_str::<SendUserTurnCommandDto>(
        r#"{"session_id":"11111111-1111-4111-8111-111111111111","turn_id":"22222222-2222-4222-8222-222222222222","content":" "}"#
    )
    .is_err());
}

#[test]
fn domain_events_round_trip_with_typed_identity_and_mode() {
    let event = DomainEventDto::SessionCreated(SessionCreatedEventDto::new(
        ProjectId::new(),
        SessionId::new(),
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Plan,
        TimestampDto::from_unix_seconds(1_700_000_000).expect("fixture timestamp is valid"),
    ));

    let encoded = serde_json::to_string(&event).expect("test serialization must succeed");
    let decoded: DomainEventDto =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");

    assert_eq!(decoded, event);
}

#[test]
fn domain_constructors_and_accessors_cover_all_basic_shapes() {
    use intention_domain::{
        CreateSessionCommandDto, PlanStatusDto, QueuedTurnProjectionDto,
        RemoveQueuedTurnCommandDto, RunProjectionDto, RunStatusDto,
    };
    use intention_types::{ConfigRevisionId, QueuePositionDto};
    let project = ProjectId::new();
    let session = SessionId::new();
    let workspace = WorkspaceId::new();
    let turn = TurnId::new();
    let root = workspace_root();
    for mode in [RunModeDto::Plan, RunModeDto::Build] {
        let c = CreateSessionCommandDto::new(project, session, workspace, root.clone(), mode);
        assert_eq!(
            (
                c.project_id(),
                c.session_id(),
                c.workspace_id(),
                c.workspace_root(),
                c.mode()
            ),
            (project, session, workspace, &root, mode)
        );
    }
    let remove = RemoveQueuedTurnCommandDto::new(session, turn);
    assert_eq!((remove.session_id(), remove.turn_id()), (session, turn));
    let queued =
        QueuedTurnProjectionDto::new(session, turn, "hello", QueuePositionDto::new(2)).unwrap();
    assert_eq!(
        (
            queued.session_id(),
            queued.turn_id(),
            queued.content(),
            queued.position().value()
        ),
        (session, turn, "hello", 2)
    );
    assert!(
        serde_json::from_value::<QueuedTurnProjectionDto>(
            serde_json::json!({"session_id":session,"turn_id":turn,"content":" ","position":0})
        )
        .is_err()
    );
    let run = RunProjectionDto::new(
        session,
        intention_types::RunId::new(),
        turn,
        RunStatusDto::Running,
        ConfigRevisionId::new(),
    );
    assert_eq!(
        (run.session_id(), run.turn_id(), run.status()),
        (session, turn, RunStatusDto::Running)
    );
    let _ = [
        PlanStatusDto::Drafting,
        PlanStatusDto::Revising,
        PlanStatusDto::Submitted,
        PlanStatusDto::Approved,
        PlanStatusDto::Rejected,
        PlanStatusDto::Superseded,
        PlanStatusDto::Abandoned,
    ];
}

#[test]
fn run_status_matrix_and_terminal_classification_are_complete() {
    use intention_domain::{RunStatusDto as S, validate_run_status_transition as v};
    let allowed = [
        (S::Queued, S::Starting),
        (S::Queued, S::Cancelled),
        (S::Queued, S::Interrupted),
        (S::Starting, S::Running),
        (S::Starting, S::Cancelling),
        (S::Starting, S::Failed),
        (S::Starting, S::Interrupted),
        (S::Running, S::WaitingInput),
        (S::Running, S::Completing),
        (S::Running, S::Cancelling),
        (S::Running, S::Failed),
        (S::Running, S::Interrupted),
        (S::WaitingInput, S::Running),
        (S::WaitingInput, S::Cancelling),
        (S::WaitingInput, S::Failed),
        (S::WaitingInput, S::Interrupted),
        (S::Completing, S::Completed),
        (S::Completing, S::Failed),
        (S::Completing, S::Interrupted),
        (S::Cancelling, S::Cancelled),
        (S::Cancelling, S::Failed),
        (S::Cancelling, S::Interrupted),
    ];
    for (a, b) in allowed {
        assert!(v(a, b).is_ok());
    }
    let all = [
        S::Queued,
        S::Starting,
        S::Running,
        S::WaitingInput,
        S::Completing,
        S::Cancelling,
        S::Completed,
        S::Cancelled,
        S::Failed,
        S::Interrupted,
    ];
    for a in all {
        for b in all {
            if !allowed.contains(&(a, b)) {
                assert!(v(a, b).is_err());
            }
        }
    }
    for s in all {
        assert_eq!(
            s.is_terminal(),
            matches!(s, S::Completed | S::Cancelled | S::Failed | S::Interrupted)
        );
    }
}
