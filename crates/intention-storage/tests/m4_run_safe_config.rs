#![allow(
    clippy::expect_used,
    reason = "M4 safe configuration contract fixtures use expect for precise diagnostics."
)]

use intention_config::ConfigSnapshotDto;
use intention_domain::SessionProjectionDto;
use intention_storage::{
    AcceptUserTurnInputDto, CommittedChangeDto, CreateSessionInputDto,
    RecoverUnfinishedRunsInputDto, RemoveQueuedTurnInputDto, StorageRepositoryDto,
    TransitionRunInputDto,
};
use intention_types::{ErrorDto, RunId, SessionEventSequenceDto, SessionId};

#[test]
fn safe_run_config_lookup_is_dto_only_and_default_failure_is_safe() {
    fn accepts_repository(_repository: &dyn StorageRepositoryDto) {}
    let _ = accepts_repository;
    let repository = DefaultRepository;
    let error = repository
        .load_run_config_snapshot(SessionId::new(), RunId::new())
        .expect_err("default safe configuration lookup is unavailable");
    assert_eq!(error.code(), "run_configuration_unavailable");
    assert!(!error.to_string().contains("credential"));
    assert!(!error.to_string().contains("sqlite"));
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
