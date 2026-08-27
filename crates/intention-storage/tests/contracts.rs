#![allow(
    clippy::expect_used,
    reason = "M3 contract fixtures use expect for precise test diagnostics."
)]

use intention_config::ConfigSnapshotDto;
use intention_domain::{
    CreateSessionCommandDto, RunModeDto, RunStatusDto, ToolLifecycleEventDto,
    ToolLifecycleStatusDto, WorkspaceRootDto,
};
use intention_storage::{
    AcceptUserTurnInputDto, AppendModelRunFactsInputDto, AppendModelRunFactsOutcomeDto,
    AppendToolLifecycleEventInputDto, CommittedChangeDto, CreateSessionInputDto,
    ModelContextMessageDto, ModelContextRoleDto, RecoverUnfinishedRunsInputDto,
    RemoveQueuedTurnInputDto, StartingRunModelContextDto, StorageRepositoryDto,
    TransitionRunInputDto,
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
    let event = ToolLifecycleEventDto::new(
        session_id,
        RunId::new(),
        intention_types::ToolCallId::new(),
        "read",
        ToolLifecycleStatusDto::Rejected,
        "policy",
        time,
    )
    .expect("tool event is valid");
    let run_id = event.run_id();
    assert_eq!(
        AppendToolLifecycleEventInputDto::new(event)
            .event()
            .run_id(),
        run_id
    );
}

#[test]
fn storage_dtos_expose_all_fields_and_default_repository_failures_safely() {
    let time = TimestampDto::from_unix_seconds(1).expect("fixture time is valid");
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let snapshot: ConfigSnapshotDto = serde_json::from_str(include_str!(
        "../../intention-config/tests/fixtures/config-snapshot-v1.json"
    ))
    .expect("safe config snapshot decodes");
    let remove = RemoveQueuedTurnInputDto::new(
        intention_domain::RemoveQueuedTurnCommandDto::new(session_id, TurnId::new()),
        time,
    );
    assert_eq!(remove.occurred_at(), time);
    assert_eq!(remove.command().session_id(), session_id);

    let facts = vec![
        intention_domain::ModelRunFactInputDto::AssistantContentAppended {
            assistant_turn_id: intention_types::AssistantTurnId::new(),
            content: "hello".into(),
        },
    ];
    let append = AppendModelRunFactsInputDto::new(
        session_id,
        run_id,
        intention_domain::RunEventCursorDto::new(0),
        facts,
        Some(RunStatusDto::Running),
        time,
    )
    .expect("fact append is valid");
    assert_eq!(append.session_id(), session_id);
    assert_eq!(append.run_id(), run_id);
    assert_eq!(append.expected_cursor().value(), 0);
    assert_eq!(append.facts().len(), 1);
    assert_eq!(append.status(), Some(RunStatusDto::Running));
    assert_eq!(append.occurred_at(), time);
    let append_snapshot = intention_domain::RunSnapshotDto::new(
        session_id,
        run_id,
        intention_types::SessionEventSequenceDto::new(1),
        intention_domain::ModelRunProjectionDto::new(
            intention_domain::RunProjectionDto::new(
                session_id,
                run_id,
                TurnId::new(),
                RunStatusDto::Running,
                ConfigRevisionId::parse("44444444-4444-4444-8444-444444444444")
                    .expect("revision is valid"),
            ),
            intention_domain::RunEventCursorDto::new(1),
            None,
            "",
            None,
            None,
            None,
        )
        .expect("model projection is valid"),
    )
    .expect("snapshot is valid");
    let fact = intention_domain::ModelRunFactDto::new(
        intention_domain::RunEventCursorDto::new(1),
        intention_domain::ModelRunFactInputDto::provider_attempt_started(1).expect("fact is valid"),
    )
    .expect("fact is valid");
    let outcome = AppendModelRunFactsOutcomeDto::new(
        intention_domain::RunEventCursorDto::new(1),
        append_snapshot,
        vec![fact],
    )
    .expect("outcome is valid");
    assert_eq!(outcome.cursor().value(), 1);
    assert_eq!(outcome.snapshot().run_id(), run_id);
    assert_eq!(outcome.facts().len(), 1);
    assert!(
        AppendModelRunFactsInputDto::new(
            session_id,
            run_id,
            intention_domain::RunEventCursorDto::new(0),
            vec![],
            None,
            time
        )
        .is_err()
    );

    let message =
        ModelContextMessageDto::new(ModelContextRoleDto::User, "hello").expect("message is valid");
    assert_eq!(message.role(), ModelContextRoleDto::User);
    assert_eq!(message.content(), "hello");
    assert!(ModelContextMessageDto::new(ModelContextRoleDto::Assistant, " ").is_err());
    let context = StartingRunModelContextDto::new(session_id, run_id, snapshot, vec![message])
        .expect("context is valid");
    assert_eq!(context.session_id(), session_id);
    assert_eq!(context.run_id(), run_id);
    assert_eq!(context.messages().len(), 1);
    assert!(
        StartingRunModelContextDto::new(session_id, run_id, context.safe_config().clone(), vec![],)
            .is_err()
    );

    struct Empty;
    impl StorageRepositoryDto for Empty {
        fn create_session(
            &self,
            _: CreateSessionInputDto,
        ) -> intention_types::DtoResult<CommittedChangeDto> {
            panic!()
        }
        fn accept_user_turn(
            &self,
            _: AcceptUserTurnInputDto,
        ) -> intention_types::DtoResult<CommittedChangeDto> {
            panic!()
        }
        fn remove_queued_turn(
            &self,
            _: RemoveQueuedTurnInputDto,
        ) -> intention_types::DtoResult<CommittedChangeDto> {
            panic!()
        }
        fn transition_run(
            &self,
            _: TransitionRunInputDto,
        ) -> intention_types::DtoResult<CommittedChangeDto> {
            panic!()
        }
        fn recover_unfinished_runs(
            &self,
            _: RecoverUnfinishedRunsInputDto,
        ) -> intention_types::DtoResult<Vec<CommittedChangeDto>> {
            panic!()
        }
        fn load_session_snapshot(
            &self,
            _: SessionId,
        ) -> intention_types::DtoResult<intention_domain::SessionProjectionDto> {
            panic!()
        }
        fn load_tail(
            &self,
            _: SessionId,
            _: intention_types::SessionEventSequenceDto,
        ) -> intention_types::DtoResult<
            Vec<intention_types::EventEnvelopeDto<intention_domain::DomainEventDto>>,
        > {
            panic!()
        }
        fn accept_configuration_revision(
            &self,
            _: ConfigSnapshotDto,
        ) -> intention_types::DtoResult<()> {
            panic!()
        }
    }
    let _ = AppendModelRunFactsOutcomeDto::new;
    let repository = Empty;
    let event = ToolLifecycleEventDto::new(
        session_id,
        run_id,
        intention_types::ToolCallId::new(),
        "read",
        ToolLifecycleStatusDto::Rejected,
        "policy",
        time,
    )
    .expect("tool event is valid");
    assert_eq!(
        repository
            .append_tool_lifecycle_event(AppendToolLifecycleEventInputDto::new(event))
            .unwrap_err()
            .code(),
        "tool_lifecycle_unavailable"
    );
    assert_eq!(
        repository
            .append_model_run_facts(append)
            .unwrap_err()
            .code(),
        "run_history_unavailable"
    );
    assert_eq!(
        repository
            .load_run_config_snapshot(session_id, run_id)
            .unwrap_err()
            .code(),
        "run_configuration_unavailable"
    );
    assert_eq!(
        repository
            .load_starting_run_model_context(session_id, run_id)
            .unwrap_err()
            .code(),
        "run_model_context_unavailable"
    );
    assert_eq!(
        repository
            .load_current_run_replay(session_id, run_id)
            .unwrap_err()
            .code(),
        "run_history_unavailable"
    );
    assert_eq!(
        repository
            .load_run_tail(
                session_id,
                run_id,
                intention_domain::RunEventCursorDto::new(0)
            )
            .unwrap_err()
            .code(),
        "run_history_unavailable"
    );
}

#[test]
fn storage_constructors_cover_successful_accessors_and_validation_paths() {
    let time = TimestampDto::from_unix_seconds(2).expect("fixture time is valid");
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let snapshot: ConfigSnapshotDto = serde_json::from_str(include_str!(
        "../../intention-config/tests/fixtures/config-snapshot-v1.json"
    ))
    .expect("safe config snapshot decodes");
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
    assert_eq!(create.command().session_id(), session_id);
    assert_eq!(create.occurred_at(), time);

    let turn = AcceptUserTurnInputDto::new(
        session_id,
        TurnId::new(),
        "content",
        run_id,
        snapshot.clone(),
        time,
    )
    .expect("turn is valid");
    assert_eq!(turn.session_id(), session_id);
    assert_eq!(turn.turn_id().to_string().len(), 36);
    assert_eq!(turn.content(), "content");
    assert_eq!(turn.proposed_run_id(), run_id);
    assert_eq!(turn.config_snapshot(), &snapshot);

    let transition = TransitionRunInputDto::new(session_id, run_id, RunStatusDto::Running, time);
    assert_eq!(transition.session_id(), session_id);
    assert_eq!(transition.run_id(), run_id);
    assert_eq!(transition.status(), RunStatusDto::Running);
    let remove = RemoveQueuedTurnInputDto::new(
        intention_domain::RemoveQueuedTurnCommandDto::new(session_id, TurnId::new()),
        time,
    );
    assert_eq!(remove.command().turn_id().to_string().len(), 36);

    let facts = vec![
        intention_domain::ModelRunFactInputDto::provider_attempt_started(1).expect("fact is valid"),
    ];
    assert!(
        AppendModelRunFactsInputDto::new(
            session_id,
            run_id,
            intention_domain::RunEventCursorDto::new(0),
            facts,
            None,
            time,
        )
        .is_ok()
    );
    let finished = intention_domain::ModelRunFactInputDto::Finished {
        reason: intention_types::FinishReasonDto::Stop,
    };
    assert!(
        AppendModelRunFactsInputDto::new(
            session_id,
            run_id,
            intention_domain::RunEventCursorDto::new(0),
            vec![
                finished.clone(),
                intention_domain::ModelRunFactInputDto::provider_attempt_started(1)
                    .expect("fact is valid")
            ],
            None,
            time,
        )
        .is_err()
    );
    let message = ModelContextMessageDto::new(ModelContextRoleDto::Assistant, "done")
        .expect("message is valid");
    assert_eq!(message.content(), "done");
    assert_eq!(
        RecoverUnfinishedRunsInputDto::new(time).recovered_at(),
        time
    );
}
