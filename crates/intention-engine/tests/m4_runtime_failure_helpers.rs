#![allow(
    clippy::expect_used,
    reason = "Scheduling failure-helper fixtures use expect for precise diagnostics."
)]

mod common;

use common::{FakeRepository, time};
use intention_engine::fail_starting_run;
use intention_proto::{RunStatusDto, SessionId};

#[test]
fn failure_helper_commits_one_terminal_failure_only_for_the_exact_starting_run() {
    let repository = FakeRepository::with_status(SessionId::new(), RunStatusDto::Starting);
    let outcome = fail_starting_run(
        &repository,
        repository.session_id,
        repository.run_id,
        "model_scheduling_unavailable",
        time(9),
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
    assert_eq!(finishes[0].occurred_at(), time(9));

    let wrong_state = FakeRepository::with_status(SessionId::new(), RunStatusDto::Running);
    let error = fail_starting_run(
        &wrong_state,
        wrong_state.session_id,
        wrong_state.run_id,
        "model_context_unavailable",
        time(9),
    )
    .expect_err("running run must not be failed by scheduling recovery");
    assert_eq!(error.code(), "invalid_starting_run_failure_state");
    assert!(wrong_state.finishes.borrow().is_empty());
}
