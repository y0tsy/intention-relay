#![allow(
    clippy::expect_used,
    reason = "M4 storage contract fixtures use expect for precise diagnostics."
)]

use intention_domain::{
    CreateSessionCommandDto, ModelRunFactInputDto, RemoveQueuedTurnCommandDto, RunEventCursorDto,
    RunEventTailPageDto, RunModeDto, SessionProjectionDto, WorkspaceRootDto,
};
use intention_storage::{
    AppendModelRunFactsInputDto, CommittedChangeDto, CreateSessionInputDto,
    RecoverUnfinishedRunsInputDto, RemoveQueuedTurnInputDto, StorageRepositoryDto,
    TransitionRunInputDto,
};
use intention_types::{
    ErrorRetryDto, FinishReasonDto, ProjectId, RunId, SessionEventSequenceDto, SessionId,
    TimestampDto, TurnId, WorkspaceId,
};

#[test]
fn storage_dtos_expose_their_safe_fields_and_default_history_errors() {
    let time = time();
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let run_id = RunId::new();
    let workspace_root = WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-storage-contracts-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("workspace root is valid");
    let command = CreateSessionCommandDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root,
        RunModeDto::Build,
    );
    let create = CreateSessionInputDto::new(command, time);
    assert_eq!(create.command().session_id(), session_id);
    assert_eq!(create.occurred_at(), time);

    let remove =
        RemoveQueuedTurnInputDto::new(RemoveQueuedTurnCommandDto::new(session_id, turn_id), time);
    assert_eq!(remove.command().session_id(), session_id);
    assert_eq!(remove.command().turn_id(), turn_id);
    assert_eq!(remove.occurred_at(), time);

    let facts = vec![ModelRunFactInputDto::provider_attempt_started(1).expect("attempt is valid")];
    let input = AppendModelRunFactsInputDto::new(
        session_id,
        run_id,
        RunEventCursorDto::new(0),
        facts,
        None,
        time,
    )
    .expect("append input is valid");
    assert_eq!(input.session_id(), session_id);
    assert_eq!(input.run_id(), run_id);
    assert_eq!(input.facts().len(), 1);
    assert_eq!(input.status(), None);
    assert_eq!(input.occurred_at(), time);

    let transition = TransitionRunInputDto::new(
        session_id,
        run_id,
        intention_domain::RunStatusDto::Completed,
        time,
    );
    assert_eq!(transition.session_id(), session_id);
    assert_eq!(transition.run_id(), run_id);
    assert_eq!(
        transition.status(),
        intention_domain::RunStatusDto::Completed
    );
    assert_eq!(transition.occurred_at(), time);
    assert_eq!(
        RecoverUnfinishedRunsInputDto::new(time).recovered_at(),
        time
    );

    let projection = SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        WorkspaceRootDto::parse(
            std::env::temp_dir()
                .join("intention-storage-contracts-workspace-2")
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

    struct DefaultRepository;
    impl StorageRepositoryDto for DefaultRepository {
        fn create_session(
            &self,
            _input: CreateSessionInputDto,
        ) -> intention_types::DtoResult<CommittedChangeDto> {
            Err(intention_types::ErrorDto::unavailable("fixture", "unused"))
        }

        fn accept_user_turn(
            &self,
            _input: intention_storage::AcceptUserTurnInputDto,
        ) -> intention_types::DtoResult<CommittedChangeDto> {
            Err(intention_types::ErrorDto::unavailable("fixture", "unused"))
        }

        fn remove_queued_turn(
            &self,
            _input: RemoveQueuedTurnInputDto,
        ) -> intention_types::DtoResult<CommittedChangeDto> {
            Err(intention_types::ErrorDto::unavailable("fixture", "unused"))
        }

        fn transition_run(
            &self,
            _input: TransitionRunInputDto,
        ) -> intention_types::DtoResult<CommittedChangeDto> {
            Err(intention_types::ErrorDto::unavailable("fixture", "unused"))
        }

        fn recover_unfinished_runs(
            &self,
            _input: RecoverUnfinishedRunsInputDto,
        ) -> intention_types::DtoResult<Vec<CommittedChangeDto>> {
            Err(intention_types::ErrorDto::unavailable("fixture", "unused"))
        }

        fn load_session_snapshot(
            &self,
            _session_id: SessionId,
        ) -> intention_types::DtoResult<SessionProjectionDto> {
            Err(intention_types::ErrorDto::unavailable("fixture", "unused"))
        }

        fn load_tail(
            &self,
            _session_id: SessionId,
            _after_sequence: SessionEventSequenceDto,
        ) -> intention_types::DtoResult<
            Vec<intention_types::EventEnvelopeDto<intention_domain::DomainEventDto>>,
        > {
            Err(intention_types::ErrorDto::unavailable("fixture", "unused"))
        }

        fn accept_configuration_revision(
            &self,
            _snapshot: intention_config::ConfigSnapshotDto,
        ) -> intention_types::DtoResult<()> {
            Err(intention_types::ErrorDto::unavailable("fixture", "unused"))
        }
    }

    let repository = DefaultRepository;
    assert_eq!(
        repository
            .append_model_run_facts(input)
            .expect_err("default append is unavailable")
            .code(),
        "run_history_unavailable"
    );
    assert_eq!(
        repository
            .load_current_run_replay(session_id, run_id)
            .expect_err("default replay is unavailable")
            .code(),
        "run_history_unavailable"
    );
    assert_eq!(
        repository
            .load_run_tail(session_id, run_id, RunEventCursorDto::new(0))
            .expect_err("default tail is unavailable")
            .code(),
        "run_history_unavailable"
    );
}

fn time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1).expect("fixture time is valid")
}

#[test]
fn model_fact_storage_contract_is_dto_only_and_bounded() {
    fn accepts_repository(_repository: &dyn StorageRepositoryDto) {}
    let _ = accepts_repository;
    let input = AppendModelRunFactsInputDto::new(
        SessionId::new(),
        RunId::new(),
        RunEventCursorDto::new(0),
        vec![ModelRunFactInputDto::provider_attempt_started(1).expect("attempt is valid")],
        None,
        time(),
    )
    .expect("non-empty batch is valid");
    assert_eq!(input.expected_cursor().value(), 0);
    assert!(
        AppendModelRunFactsInputDto::new(
            input.session_id(),
            input.run_id(),
            input.expected_cursor(),
            Vec::new(),
            None,
            time(),
        )
        .is_err()
    );
    for terminal_fact in [
        ModelRunFactInputDto::finished(FinishReasonDto::Stop),
        ModelRunFactInputDto::failed(
            intention_domain::RunFailureDto::new("provider_failed", ErrorRetryDto::Never, None)
                .expect("safe failure is valid"),
        ),
    ] {
        assert_eq!(
            AppendModelRunFactsInputDto::new(
                input.session_id(),
                input.run_id(),
                input.expected_cursor(),
                vec![
                    terminal_fact,
                    ModelRunFactInputDto::provider_attempt_started(1).expect("attempt is valid"),
                ],
                None,
                time(),
            )
            .expect_err("facts after a terminal model fact reject")
            .code(),
            "invalid_run_event_cursor"
        );
    }

    let page = RunEventTailPageDto::empty(
        input.session_id(),
        input.run_id(),
        RunEventCursorDto::new(0),
    );
    assert!(!page.has_more());
}
