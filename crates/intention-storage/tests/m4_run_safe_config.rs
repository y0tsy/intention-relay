#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{create_session, repository, time};

use intention_config::{ConfigSnapshotDto, RawConfigInputDto, ResolvedConfigDto};
use intention_proto::{ErrorCategoryDto, RunId, SessionId};
use intention_storage::StorageRepositoryDto;

#[test]
fn a_matching_run_loads_the_snapshot_its_acceptance_froze() {
    let (_directory, repository) = repository();
    let session_id = create_session(&repository, "matching");
    let snapshot = snapshot("safe-model", Some("https://models.example.test/v1"), 17, 2);
    let run_id = start(&repository, session_id, snapshot.clone());

    let loaded = repository
        .load_run_config_snapshot(session_id, run_id)
        .expect("the run row carries its frozen snapshot");
    assert_eq!(loaded, snapshot);
    let encoded = serde_json::to_string(&loaded).expect("the frozen snapshot serializes");
    assert!(!encoded.contains("recognizable-fixture-credential"));
    assert!(!encoded.contains("safe-config.toml"));
}

#[test]
fn unknown_and_cross_session_run_config_lookups_share_safe_no_leak_error() {
    let (_directory, repository) = repository();
    let session_id = create_session(&repository, "owner");
    let run_id = start(&repository, session_id, snapshot("safe-model", None, 30, 2));
    let other_session_id = create_session(&repository, "other");

    let errors = [
        repository
            .load_run_config_snapshot(SessionId::new(), run_id)
            .expect_err("unknown session is hidden"),
        repository
            .load_run_config_snapshot(session_id, RunId::new())
            .expect_err("unknown run is hidden"),
        repository
            .load_run_config_snapshot(other_session_id, run_id)
            .expect_err("cross-session run is hidden"),
    ];
    for error in errors {
        assert_eq!(error.code(), "storage_record_not_found");
        assert_eq!(error.category(), ErrorCategoryDto::NotFound);
        assert!(!error.retryable());
        let rendered = error.to_string();
        assert!(!rendered.contains("recognizable-fixture-credential"));
        assert!(!rendered.contains("safe-config.toml"));
        assert!(!rendered.contains("sqlite"));
    }
}

#[test]
fn an_undecodable_stored_snapshot_is_unavailable() {
    for stored in ["{not-a-snapshot", r#"{"captured_at":1700000000}"#] {
        let (directory, repository) = repository();
        let session_id = create_session(&repository, "corrupted");
        let run_id = start(&repository, session_id, snapshot("safe-model", None, 30, 2));
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens for corruption");
        connection
            .execute(
                "UPDATE runs SET config_snapshot_json=?2 WHERE id=?1",
                sqlite::params![run_id.to_string(), stored],
            )
            .expect("the persisted snapshot is corrupted");

        let corrupted = repository
            .load_run_config_snapshot(session_id, run_id)
            .expect_err("an undecodable stored snapshot is not served");
        assert_eq!(corrupted.code(), "storage_unavailable");
        assert_eq!(corrupted.category(), ErrorCategoryDto::Unavailable);
        assert!(corrupted.retryable());
        assert!(!corrupted.to_string().contains("not-a-snapshot"));
    }
}

#[test]
fn a_deleted_run_row_is_a_permanent_not_found() {
    let (directory, repository) = repository();
    let session_id = create_session(&repository, "deleted");
    let run_id = start(&repository, session_id, snapshot("safe-model", None, 30, 2));
    let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
        .expect("database reopens for deletion");
    connection
        .execute("DELETE FROM runs WHERE id=?1", [run_id.to_string()])
        .expect("the run row is removed");

    let missing = repository
        .load_run_config_snapshot(session_id, run_id)
        .expect_err("a genuinely absent run row is a missing record");
    assert_eq!(missing.code(), "storage_record_not_found");
    assert_eq!(missing.category(), ErrorCategoryDto::NotFound);
    assert!(!missing.retryable());
}

#[test]
fn backend_failure_on_a_run_config_lookup_is_unavailable_not_not_found() {
    let (directory, repository) = repository();
    let session_id = create_session(&repository, "backend-failure");
    let run_id = start(&repository, session_id, snapshot("safe-model", None, 30, 2));
    let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
        .expect("database reopens for the backend failure");
    connection
        .execute_batch("DROP TABLE runs;")
        .expect("the backend-failure fixture mutation applies");
    drop(connection);

    let error = repository
        .load_run_config_snapshot(session_id, run_id)
        .expect_err("a backend read failure is unavailable");
    assert_eq!(error.code(), "storage_unavailable");
    assert_eq!(error.category(), ErrorCategoryDto::Unavailable);
    assert!(error.retryable());
}

/// Starts one run through a first user turn and returns its committed identity.
fn start(
    repository: &intention_storage::SqliteStorageRepository,
    session_id: SessionId,
    snapshot: ConfigSnapshotDto,
) -> RunId {
    match repository
        .accept_user_turn(session_id, "turn", snapshot, time(2))
        .expect("the turn starts a run")
    {
        intention_storage::AcceptedTurnOutcomeDto::Started { run, .. } => run.run_id(),
        intention_storage::AcceptedTurnOutcomeDto::Pending(_) => {
            unreachable!("the first turn of an idle session starts a run")
        }
    }
}

fn snapshot(
    model: &str,
    endpoint: Option<&str>,
    attempt_timeout_seconds: u8,
    max_attempts: u8,
) -> ConfigSnapshotDto {
    let endpoint = endpoint.map_or_else(String::new, |value| format!("endpoint = \"{value}\"\n"));
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(format!(
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"{model}\"\n{endpoint}credential = \"recognizable-fixture-credential\"\n[provider.execution]\nattempt_timeout_seconds = {attempt_timeout_seconds}\nmax_attempts = {max_attempts}"
    )))
    .expect("safe configuration resolves");
    ConfigSnapshotDto::new(time(1), resolved)
}
