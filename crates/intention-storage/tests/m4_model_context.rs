#![allow(
    clippy::expect_used,
    reason = "M4 model-context contract fixtures use expect for precise diagnostics."
)]

use intention_config::ConfigSnapshotDto;
use intention_domain::SessionProjectionDto;
use intention_storage::{
    AcceptUserTurnInputDto, CommittedChangeDto, CreateSessionInputDto, ModelContextMessageDto,
    ModelContextRoleDto, RecoverUnfinishedRunsInputDto, RemoveQueuedTurnInputDto,
    StorageRepositoryDto, TransitionRunInputDto,
};
use intention_types::{ErrorDto, RunId, SessionEventSequenceDto, SessionId};

#[test]
fn model_context_dtos_are_typed_and_default_failure_is_safe() {
    let user = ModelContextMessageDto::new(ModelContextRoleDto::User, "user content")
        .expect("non-blank user content is valid");
    let assistant =
        ModelContextMessageDto::new(ModelContextRoleDto::Assistant, "assistant content")
            .expect("non-blank assistant content is valid");
    assert_eq!(user.role(), ModelContextRoleDto::User);
    assert_eq!(user.content(), "user content");
    assert_eq!(assistant.role(), ModelContextRoleDto::Assistant);
    assert_eq!(assistant.content(), "assistant content");
    assert!(ModelContextMessageDto::new(ModelContextRoleDto::User, " \n\t ").is_err());

    fn accepts_repository(_repository: &dyn StorageRepositoryDto) {}
    let _ = accepts_repository;
    let repository = DefaultRepository;
    let error = repository
        .load_starting_run_model_context(SessionId::new(), RunId::new())
        .expect_err("default model context lookup is unavailable");
    assert_eq!(error.code(), "run_model_context_unavailable");
    let rendered = error.to_string();
    assert!(!rendered.contains("credential"));
    assert!(!rendered.contains("sqlite"));
}

struct DefaultRepository;

impl StorageRepositoryDto for DefaultRepository {
    fn create_session(
        &self,
        _input: CreateSessionInputDto,
    ) -> Result<CommittedChangeDto, ErrorDto> {
        Err(unused())
    }

    fn accept_user_turn(
        &self,
        _input: AcceptUserTurnInputDto,
    ) -> Result<CommittedChangeDto, ErrorDto> {
        Err(unused())
    }

    fn remove_queued_turn(
        &self,
        _input: RemoveQueuedTurnInputDto,
    ) -> Result<CommittedChangeDto, ErrorDto> {
        Err(unused())
    }

    fn transition_run(
        &self,
        _input: TransitionRunInputDto,
    ) -> Result<CommittedChangeDto, ErrorDto> {
        Err(unused())
    }

    fn recover_unfinished_runs(
        &self,
        _input: RecoverUnfinishedRunsInputDto,
    ) -> Result<Vec<CommittedChangeDto>, ErrorDto> {
        Err(unused())
    }

    fn load_session_snapshot(
        &self,
        _session_id: SessionId,
    ) -> Result<SessionProjectionDto, ErrorDto> {
        Err(unused())
    }

    fn load_tail(
        &self,
        _session_id: SessionId,
        _after_sequence: SessionEventSequenceDto,
    ) -> Result<Vec<intention_types::EventEnvelopeDto<intention_domain::DomainEventDto>>, ErrorDto>
    {
        Err(unused())
    }

    fn accept_configuration_revision(&self, _snapshot: ConfigSnapshotDto) -> Result<(), ErrorDto> {
        Err(unused())
    }
}

fn unused() -> ErrorDto {
    ErrorDto::unavailable("fixture", "unused")
}
