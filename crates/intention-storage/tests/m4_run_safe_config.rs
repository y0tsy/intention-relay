#![allow(
    clippy::expect_used,
    reason = "M4 SQLite safe configuration fixtures use expect for precise diagnostics."
)]

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{create_session, repository, selection, time};

use intention_config::{ConfigSnapshotDto, ContextWindowPolicyDto};
use intention_proto::{
    ConfigRevisionId, ErrorCategoryDto, ErrorRetryDto, IdempotencyKey, RunId, SessionId,
};
use intention_storage::StorageRepositoryDto;

#[test]
fn matching_run_loads_its_immutable_safe_configuration_selection() {
    let (_directory, repository) = repository();
    let session_id = create_session(&repository, "matching");
    let run_id = RunId::new();
    let snapshot = snapshot();
    repository
        .accept_user_turn(
            session_id,
            IdempotencyKey::new(),
            "turn",
            run_id,
            snapshot.clone(),
            selection("fixture"),
            None,
            time(2),
        )
        .expect("turn persists its safe selection");

    let loaded = repository
        .load_run_config_snapshot(session_id, run_id)
        .expect("matching run configuration loads");
    assert_eq!(loaded, snapshot);
    let encoded = serde_json::to_string(&loaded).expect("safe selection serializes");
    assert!(!encoded.contains("recognizable-fixture-credential"));
    assert!(!encoded.contains("safe-config.toml"));
}

#[test]
fn unknown_and_cross_session_run_config_lookups_share_safe_no_leak_error() {
    let (_directory, repository) = repository();
    let session_id = create_session(&repository, "owner");
    let run_id = RunId::new();
    repository
        .accept_user_turn(
            session_id,
            IdempotencyKey::new(),
            "turn",
            run_id,
            snapshot(),
            selection("fixture"),
            None,
            time(2),
        )
        .expect("turn starts");
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
        assert_eq!(error.code(), "run_configuration_not_found");
        let rendered = error.to_string();
        assert!(!rendered.contains("recognizable-fixture-credential"));
        assert!(!rendered.contains("safe-config.toml"));
        assert!(!rendered.contains("sqlite"));
    }
}

#[test]
fn corrupted_run_configuration_snapshot_is_a_decode_failure_and_a_missing_row_stays_unavailable() {
    let (directory, repository) = repository();
    let session_id = create_session(&repository, "corrupted");
    let run_id = RunId::new();
    let snapshot = snapshot();
    let revision_id = snapshot.revision_id();
    repository
        .accept_user_turn(
            session_id,
            IdempotencyKey::new(),
            "turn",
            run_id,
            snapshot,
            selection("fixture"),
            None,
            time(2),
        )
        .expect("turn starts");
    let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
        .expect("database reopens for corruption");
    connection
        .execute(
            "UPDATE configuration_revisions SET snapshot_json=?2 WHERE id=?1",
            sqlite::params![revision_id.to_string(), "{not-a-snapshot"],
        )
        .expect("persisted snapshot is corrupted");

    let corrupted = repository
        .load_run_config_snapshot(session_id, run_id)
        .expect_err("a malformed persisted snapshot is not decodable");
    assert_eq!(corrupted.code(), "storage_decode_failed");
    assert_eq!(corrupted.category(), ErrorCategoryDto::Internal);
    assert_eq!(corrupted.retry(), ErrorRetryDto::Never);
    assert!(!corrupted.to_string().contains("not-a-snapshot"));

    // A missing row remains transient unavailability, not corruption.
    connection
        .execute(
            "DELETE FROM configuration_revisions WHERE id=?1",
            [revision_id.to_string()],
        )
        .expect("persisted snapshot row is removed");
    drop(connection);
    let missing = repository
        .load_run_config_snapshot(session_id, run_id)
        .expect_err("a missing persisted snapshot is unavailable");
    assert_eq!(missing.code(), "run_configuration_unavailable");
}

#[test]
fn backend_failure_on_a_run_config_lookup_is_unavailable_not_not_found() {
    // Only a genuinely missing row is a permanent not-found; a backend read
    // failure on the run-identity or the persisted-snapshot lookup stays
    // transient `storage_unavailable`.
    for (label, mutation) in [
        ("backend-failure", "DROP TABLE runs;"),
        (
            "snapshot-backend-failure",
            "DROP TABLE configuration_revisions;",
        ),
    ] {
        let (directory, repository) = repository();
        let session_id = create_session(&repository, label);
        let run_id = RunId::new();
        repository
            .accept_user_turn(
                session_id,
                IdempotencyKey::new(),
                "turn",
                run_id,
                snapshot(),
                selection("fixture"),
                None,
                time(2),
            )
            .expect("turn starts");
        let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
            .expect("database reopens for the backend failure");
        connection
            .execute_batch(mutation)
            .expect("the backend-failure fixture mutation applies");
        drop(connection);
        let error = repository
            .load_run_config_snapshot(session_id, run_id)
            .expect_err("a backend read failure is unavailable");
        assert_eq!(error.code(), "storage_unavailable");
        assert_eq!(error.category(), ErrorCategoryDto::Unavailable);
        assert_ne!(error.retry(), ErrorRetryDto::Never);
    }
}

#[test]
fn run_config_read_requires_a_persistable_snapshot() {
    let (directory, repository) = repository();
    let session_id = create_session(&repository, "unpersistable");
    let run_id = RunId::new();
    let snapshot = snapshot();
    let revision_id = snapshot.revision_id();
    repository
        .accept_user_turn(
            session_id,
            IdempotencyKey::new(),
            "turn",
            run_id,
            snapshot.clone(),
            selection("fixture"),
            None,
            time(2),
        )
        .expect("turn starts");

    // The stored selection is rewritten to a foreign schema version: it no
    // longer satisfies the persistence gate, so the read must not hand it out.
    let mut wire: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&snapshot).expect("snapshot serializes"))
            .expect("snapshot is JSON");
    wire["schema_version"]["major"] = serde_json::json!(2);
    let connection = sqlite::Connection::open(directory.path().join("storage.sqlite"))
        .expect("database reopens for the rewrite");
    connection
        .execute(
            "UPDATE configuration_revisions SET snapshot_json=?2 WHERE id=?1",
            sqlite::params![
                revision_id.to_string(),
                serde_json::to_string(&wire).expect("mutated snapshot serializes")
            ],
        )
        .expect("the stored snapshot is replaced");
    drop(connection);

    let error = repository
        .load_run_config_snapshot(session_id, run_id)
        .expect_err("an unpersistable stored snapshot is not served");
    assert_eq!(error.code(), "storage_decode_failed");
    assert_eq!(error.category(), ErrorCategoryDto::Internal);
    assert_eq!(error.retry(), ErrorRetryDto::Never);
    assert!(
        !error
            .to_string()
            .contains("recognizable-fixture-credential")
    );
}

fn snapshot() -> ConfigSnapshotDto {
    ConfigSnapshotDto::new(
        intention_proto::SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        time(1),
        ContextWindowPolicyDto::new(250_000).expect("the fixture window is positive"),
    )
    .expect("safe snapshot is valid")
}
