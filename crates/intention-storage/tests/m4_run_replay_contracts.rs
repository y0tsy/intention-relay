#![allow(
    clippy::expect_used,
    reason = "M4 storage contract fixtures use expect for precise diagnostics."
)]

use intention_domain::{
    RemoveQueuedTurnCommandDto, RunModeDto, SessionProjectionDto, WorkspaceRootDto,
};
use intention_storage::{CommittedChangeDto, RemoveQueuedTurnInputDto};
use intention_types::{
    ProjectId, SessionEventSequenceDto, SessionId, TimestampDto, TurnId, WorkspaceId,
};

#[test]
fn storage_commit_and_remove_dtos_expose_their_safe_fields() {
    let time = time();
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let projection = SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        WorkspaceRootDto::parse(
            std::env::temp_dir()
                .join("intention-storage-contracts-workspace")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("workspace root is valid"),
        RunModeDto::Build,
        None,
        None,
        Vec::new(),
        SessionEventSequenceDto::new(0),
    )
    .expect("projection is valid");
    let committed = CommittedChangeDto::new(
        projection,
        SessionEventSequenceDto::new(0),
        Vec::new(),
        None,
    )
    .expect("empty commit evidence is valid");
    assert_eq!(committed.position().value(), 0);
    assert!(committed.events().is_empty());
    assert!(committed.turn_outcome().is_none());

    let remove =
        RemoveQueuedTurnInputDto::new(RemoveQueuedTurnCommandDto::new(session_id, turn_id), time);
    assert_eq!(remove.command().session_id(), session_id);
    assert_eq!(remove.command().turn_id(), turn_id);
}

fn time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1).expect("fixture time is valid")
}
