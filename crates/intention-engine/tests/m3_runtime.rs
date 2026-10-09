#![allow(
    clippy::expect_used,
    reason = "Focused runtime fixtures use expect to provide precise test failures."
)]

mod common;

use std::sync::PoisonError;

use common::{FakeRepository, time};
use intention_engine::fail_starting_run;
use intention_proto::{RunId, RunStatusDto, SessionId};

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
    let finishes = repository
        .finishes
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
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
    assert!(
        repository
            .finishes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    );
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
    assert!(
        repository
            .finishes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    );
}
