#![allow(
    clippy::expect_used,
    reason = "Scheduling failure-helper fixtures use expect for precise diagnostics."
)]

use std::cell::RefCell;

use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{RunProjectionDto, RunStatusDto, SessionProjectionDto};
use intention_proto::{
    ConfigRevisionId, DtoResult, ErrorDto, RunId, SchemaVersionDto, SessionId, TimestampDto, TurnId,
};
use intention_runtime::fail_starting_run;
use intention_storage::{FinishRunInputDto, StorageRepositoryDto};

fn time() -> TimestampDto {
    TimestampDto::from_unix_seconds(9).expect("fixture timestamp is valid")
}

fn snapshot() -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-runtime-failure-helper.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture path is absolute"),
    );
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-secret\"",
        source,
    ))
    .expect("fixture config resolves");
    ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        time(),
        resolved,
    )
    .expect("fixture snapshot is valid")
}

/// Records every terminal commit so the helper's exact write count is visible.
struct FakeRepository {
    session_id: SessionId,
    run_id: RunId,
    status: RefCell<RunStatusDto>,
    finishes: RefCell<Vec<FinishRunInputDto>>,
}

impl FakeRepository {
    fn new(status: RunStatusDto) -> Self {
        Self {
            session_id: SessionId::new(),
            run_id: RunId::new(),
            status: RefCell::new(status),
            finishes: RefCell::new(Vec::new()),
        }
    }

    fn projection(&self) -> RunProjectionDto {
        RunProjectionDto::new(
            self.session_id,
            self.run_id,
            TurnId::new(),
            *self.status.borrow(),
            snapshot().revision_id(),
        )
    }
}

impl StorageRepositoryDto for FakeRepository {
    fn create_session(
        &self,
        _input: intention_storage::CreateSessionInputDto,
    ) -> DtoResult<SessionProjectionDto> {
        unused()
    }

    fn accept_user_turn(
        &self,
        _input: intention_storage::AcceptUserTurnInputDto,
    ) -> DtoResult<intention_storage::AcceptedTurnOutcomeDto> {
        unused()
    }

    fn remove_turn(
        &self,
        _input: intention_storage::RemoveTurnInputDto,
    ) -> DtoResult<intention_domain::PendingTurnProjectionDto> {
        unused()
    }

    fn consume_pending_user_turns(
        &self,
        _input: intention_storage::ConsumePendingUserTurnsInputDto,
    ) -> DtoResult<Vec<intention_domain::MessageProjectionDto>> {
        unused()
    }

    fn transition_run(
        &self,
        _input: intention_storage::TransitionRunInputDto,
    ) -> DtoResult<RunProjectionDto> {
        unused()
    }

    fn finish_run(&self, input: FinishRunInputDto) -> DtoResult<RunProjectionDto> {
        *self.status.borrow_mut() = input.status();
        self.finishes.borrow_mut().push(input);
        Ok(self.projection())
    }

    fn append_message(
        &self,
        _input: intention_storage::AppendMessageInputDto,
    ) -> DtoResult<intention_domain::MessageProjectionDto> {
        unused()
    }

    fn write_tool_result(
        &self,
        _input: intention_storage::WriteToolResultInputDto,
    ) -> DtoResult<intention_storage::ToolResultEvidenceDto> {
        unused()
    }

    fn load_tool_result(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _call_id: intention_proto::ToolCallId,
    ) -> DtoResult<intention_storage::ToolResultEvidenceDto> {
        unused()
    }

    fn load_run_config_snapshot(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<ConfigSnapshotDto> {
        unused()
    }

    fn load_starting_run_model_context(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<intention_storage::StartingRunModelContextDto> {
        unused()
    }

    fn load_run_projection(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<RunProjectionDto> {
        Ok(self.projection())
    }

    fn load_session_projection(
        &self,
        _session_id: SessionId,
    ) -> DtoResult<intention_domain::SessionProjectionDto> {
        unused()
    }

    fn load_recent_messages(
        &self,
        _session_id: SessionId,
        _limit: u32,
    ) -> DtoResult<Vec<intention_domain::MessageProjectionDto>> {
        unused()
    }

    fn load_run_messages(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _limit: u32,
    ) -> DtoResult<Vec<intention_domain::MessageProjectionDto>> {
        unused()
    }

    fn recover_unfinished_runs(
        &self,
        _input: intention_storage::RecoverUnfinishedRunsInputDto,
    ) -> DtoResult<Vec<RunProjectionDto>> {
        unused()
    }

    fn accept_configuration_revision(&self, _snapshot: ConfigSnapshotDto) -> DtoResult<()> {
        unused()
    }
}

fn unused<T>() -> DtoResult<T> {
    Err(ErrorDto::unavailable("fixture_unused", "unused"))
}

#[test]
fn failure_helper_commits_one_terminal_failure_only_for_the_exact_starting_run() {
    let repository = FakeRepository::new(RunStatusDto::Starting);
    let outcome = fail_starting_run(
        &repository,
        repository.session_id,
        repository.run_id,
        "model_scheduling_unavailable",
        time(),
    )
    .expect("starting run can fail atomically");

    assert_eq!(outcome.status(), RunStatusDto::Failed);
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].status(), RunStatusDto::Failed);
    assert_eq!(
        finishes[0].error_code(),
        Some("model_scheduling_unavailable")
    );
    assert_eq!(finishes[0].occurred_at(), time());

    let wrong_state = FakeRepository::new(RunStatusDto::Running);
    let error = fail_starting_run(
        &wrong_state,
        wrong_state.session_id,
        wrong_state.run_id,
        "model_context_unavailable",
        time(),
    )
    .expect_err("running run must not be failed by scheduling recovery");
    assert_eq!(error.code(), "invalid_starting_run_failure_state");
    assert!(wrong_state.finishes.borrow().is_empty());
}
