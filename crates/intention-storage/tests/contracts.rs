#![allow(
    clippy::expect_used,
    reason = "M3 contract fixtures use expect for precise test diagnostics."
)]

use intention_config::ConfigSnapshotDto;
use intention_domain::{CreateSessionCommandDto, RunModeDto, RunStatusDto, WorkspaceRootDto};
use intention_storage::{
    AcceptUserTurnInputDto, CreateSessionInputDto, RecoverUnfinishedRunsInputDto,
    StorageRepositoryDto, TransitionRunInputDto,
};
use intention_types::{
    ConfigRevisionId, ProjectId, RunId, SessionId, TimestampDto, TurnId, WorkspaceId,
};

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-storage-contracts-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

#[test]
fn storage_input_and_commit_evidence_reject_invalid_boundaries() {
    let time = TimestampDto::from_unix_seconds(1).expect("fixture time is valid");
    let session_id = SessionId::new();
    let snapshot: ConfigSnapshotDto = serde_json::from_str(include_str!(
        "../../intention-config/tests/fixtures/config-snapshot-v1.json"
    ))
    .expect("safe config snapshot decodes");
    assert!(
        AcceptUserTurnInputDto::new(session_id, TurnId::new(), " ", RunId::new(), snapshot, time,)
            .is_err()
    );

    let projection = intention_domain::SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
        None,
        None,
        Vec::new(),
        intention_types::SessionEventSequenceDto::new(0),
    )
    .expect("projection is coherent");
    assert!(
        intention_storage::CommittedChangeDto::new(
            projection,
            intention_types::SessionEventSequenceDto::new(0),
            vec![intention_types::EventEnvelopeDto::new(
                intention_types::EventMetadataDto::new(
                    intention_types::SchemaVersionDto::new(1, 0),
                    intention_types::EventId::new(),
                    session_id,
                    None,
                    None,
                    intention_types::SessionEventSequenceDto::new(1),
                    time,
                ),
                intention_domain::DomainEventDto::RunStatusChanged(
                    intention_domain::RunStatusChangedEventDto::new(
                        session_id,
                        RunId::new(),
                        RunStatusDto::Running,
                        time,
                    ),
                ),
            )],
            None,
        )
        .is_err()
    );
}

#[test]
fn repository_contracts_supply_ids_timestamps_and_config_revisions_for_all_mutating_paths() {
    fn accepts_repository(_repository: &dyn StorageRepositoryDto) {}
    let _ = accepts_repository;
    let time = TimestampDto::from_unix_seconds(1).expect("fixture time is valid");
    let session_id = SessionId::new();
    let create = CreateSessionInputDto::new(
        CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            workspace_root(),
            RunModeDto::Build,
        ),
        time,
    );
    assert_eq!(create.occurred_at(), time);
    let snapshot: ConfigSnapshotDto = serde_json::from_str(include_str!(
        "../../intention-config/tests/fixtures/config-snapshot-v1.json"
    ))
    .expect("safe config snapshot decodes");
    let turn = AcceptUserTurnInputDto::new(
        session_id,
        TurnId::new(),
        "hello",
        RunId::new(),
        snapshot,
        time,
    )
    .expect("turn input is valid");
    assert_eq!(turn.occurred_at(), time);
    assert_eq!(
        turn.config_revision_id(),
        ConfigRevisionId::parse("44444444-4444-4444-8444-444444444444")
            .expect("fixture id is valid")
    );
    let transition =
        TransitionRunInputDto::new(session_id, RunId::new(), RunStatusDto::Completed, time);
    assert_eq!(transition.occurred_at(), time);
    assert_eq!(
        RecoverUnfinishedRunsInputDto::new(time).recovered_at(),
        time
    );
}
