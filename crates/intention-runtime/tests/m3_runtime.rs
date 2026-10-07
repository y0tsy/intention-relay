#![allow(
    clippy::expect_used,
    reason = "Focused runtime fixtures use expect to provide precise test failures."
)]

use std::cell::RefCell;

use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{
    RunModeDto, RunProjectionDto, RunStatusDto, SessionProjectionDto, WorkspaceRootDto,
};
use intention_proto::{
    ConfigRevisionId, DtoResult, ErrorDto, ProjectId, RunId, SchemaVersionDto, SessionId,
    TimestampDto, TurnId, WorkspaceId,
};
use intention_runtime::fail_starting_run;
use intention_storage::{CreateSessionInputDto, FinishRunInputDto, StorageRepositoryDto};

fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture timestamp is valid")
}

fn snapshot() -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-runtime-test.toml")
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
        time(1),
        resolved,
    )
    .expect("fixture snapshot is valid")
}

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-runtime-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

/// Records the exact terminal run outcome committed by the runtime helper.
struct FakeRepository {
    session_id: SessionId,
    run: RefCell<RunProjectionDto>,
    finishes: RefCell<Vec<FinishRunInputDto>>,
}

impl FakeRepository {
    fn with_status(session_id: SessionId, status: RunStatusDto) -> Self {
        Self {
            session_id,
            run: RefCell::new(RunProjectionDto::new(
                session_id,
                RunId::new(),
                TurnId::new(),
                status,
                snapshot().revision_id(),
            )),
            finishes: RefCell::new(Vec::new()),
        }
    }

    fn run_id(&self) -> RunId {
        self.run.borrow().run_id()
    }
}

impl StorageRepositoryDto for FakeRepository {
    fn create_session(&self, _input: CreateSessionInputDto) -> DtoResult<SessionProjectionDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "create is not used by this fixture",
        ))
    }

    fn accept_user_turn(
        &self,
        _input: intention_storage::AcceptUserTurnInputDto,
    ) -> DtoResult<intention_storage::AcceptedTurnOutcomeDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "accept is not used by this fixture",
        ))
    }

    fn remove_turn(
        &self,
        _input: intention_storage::RemoveTurnInputDto,
    ) -> DtoResult<intention_domain::PendingTurnProjectionDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "remove is not used by this fixture",
        ))
    }

    fn consume_pending_user_turns(
        &self,
        _input: intention_storage::ConsumePendingUserTurnsInputDto,
    ) -> DtoResult<Vec<intention_domain::MessageProjectionDto>> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "consume is not used by this fixture",
        ))
    }

    fn transition_run(
        &self,
        _input: intention_storage::TransitionRunInputDto,
    ) -> DtoResult<RunProjectionDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "transition is not used by this fixture",
        ))
    }

    fn finish_run(&self, input: FinishRunInputDto) -> DtoResult<RunProjectionDto> {
        assert_eq!(input.session_id(), self.session_id);
        assert_eq!(input.run_id(), self.run_id());
        let current = *self.run.borrow();
        let finished = RunProjectionDto::new(
            current.session_id(),
            current.run_id(),
            current.turn_id(),
            input.status(),
            current.config_revision_id(),
        );
        *self.run.borrow_mut() = finished;
        self.finishes.borrow_mut().push(input);
        Ok(finished)
    }

    fn append_message(
        &self,
        _input: intention_storage::AppendMessageInputDto,
    ) -> DtoResult<intention_domain::MessageProjectionDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "append is not used by this fixture",
        ))
    }

    fn write_tool_result(
        &self,
        _input: intention_storage::WriteToolResultInputDto,
    ) -> DtoResult<intention_storage::ToolResultEvidenceDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "tool result is not used by this fixture",
        ))
    }

    fn load_tool_result(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _call_id: intention_proto::ToolCallId,
    ) -> DtoResult<intention_storage::ToolResultEvidenceDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "tool result is not used by this fixture",
        ))
    }

    fn load_run_config_snapshot(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<ConfigSnapshotDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "configuration is not used by this fixture",
        ))
    }

    fn load_starting_run_model_context(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
    ) -> DtoResult<intention_storage::StartingRunModelContextDto> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "context is not used by this fixture",
        ))
    }

    fn load_run_projection(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunProjectionDto> {
        if session_id != self.session_id || run_id != self.run_id() {
            return Err(ErrorDto::validation(
                "run_not_found",
                "the requested durable run does not exist",
            ));
        }
        Ok(*self.run.borrow())
    }

    fn load_session_projection(&self, session_id: SessionId) -> DtoResult<SessionProjectionDto> {
        assert_eq!(session_id, self.session_id);
        SessionProjectionDto::new(
            ProjectId::new(),
            self.session_id,
            WorkspaceId::new(),
            workspace_root(),
            RunModeDto::Build,
            Some(self.run.borrow().config_revision_id()),
            Some(*self.run.borrow()),
            Vec::new(),
        )
    }

    fn load_recent_messages(
        &self,
        _session_id: SessionId,
        _limit: u32,
    ) -> DtoResult<Vec<intention_domain::MessageProjectionDto>> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "messages are not used by this fixture",
        ))
    }

    fn load_run_messages(
        &self,
        _session_id: SessionId,
        _run_id: RunId,
        _limit: u32,
    ) -> DtoResult<Vec<intention_domain::MessageProjectionDto>> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "messages are not used by this fixture",
        ))
    }

    fn recover_unfinished_runs(
        &self,
        _input: intention_storage::RecoverUnfinishedRunsInputDto,
    ) -> DtoResult<Vec<RunProjectionDto>> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "recovery is not used by this fixture",
        ))
    }

    fn accept_configuration_revision(&self, _snapshot: ConfigSnapshotDto) -> DtoResult<()> {
        Err(ErrorDto::unavailable(
            "fixture_unused",
            "configuration is not used by this fixture",
        ))
    }
}

#[test]
fn fail_starting_run_records_a_manual_failure_for_the_exact_starting_run() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_status(session_id, RunStatusDto::Starting);
    let run_id = repository.run_id();

    let outcome = fail_starting_run(
        &repository,
        session_id,
        run_id,
        "model_context_unavailable",
        time(72),
    )
    .expect("starting mutation failure commits");

    assert_eq!(outcome.run_id(), run_id);
    assert_eq!(outcome.status(), RunStatusDto::Failed);
    let finishes = repository.finishes.borrow();
    assert_eq!(finishes.len(), 1);
    assert_eq!(finishes[0].session_id(), session_id);
    assert_eq!(finishes[0].run_id(), run_id);
    assert_eq!(finishes[0].status(), RunStatusDto::Failed);
    assert_eq!(finishes[0].error_code(), Some("model_context_unavailable"));
    assert!(finishes[0].error_message().is_some());
    assert_eq!(finishes[0].usage(), None);
    assert_eq!(finishes[0].finish_reason(), None);
    assert_eq!(finishes[0].occurred_at(), time(72));
}

#[test]
fn fail_starting_run_rejects_a_run_that_is_no_longer_starting() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_status(session_id, RunStatusDto::Running);
    let run_id = repository.run_id();

    assert_eq!(
        fail_starting_run(
            &repository,
            session_id,
            run_id,
            "model_context_unavailable",
            time(72),
        )
        .expect_err("a running run is not a scheduling failure")
        .code(),
        "invalid_starting_run_failure_state"
    );
    assert!(repository.finishes.borrow().is_empty());
}

#[test]
fn fail_starting_run_propagates_an_absent_run_without_writing() {
    let session_id = SessionId::new();
    let repository = FakeRepository::with_status(session_id, RunStatusDto::Starting);
    assert_eq!(
        fail_starting_run(
            &repository,
            session_id,
            RunId::new(),
            "model_context_unavailable",
            time(72),
        )
        .expect_err("an unknown run rejects")
        .code(),
        "run_not_found"
    );
    assert!(repository.finishes.borrow().is_empty());
}
