#![allow(
    clippy::expect_used,
    reason = "M4 SQLite safe configuration fixtures use expect for precise diagnostics."
)]

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{create_session, repository, time};

use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_proto::{
    ConfigRevisionId, ErrorCategoryDto, ErrorRetryDto, IdempotencyKey, RunId, SessionId,
};
use intention_storage::{AcceptUserTurnInputDto, StorageRepositoryDto};

#[test]
fn matching_run_loads_its_immutable_safe_configuration_selection() {
    let (_directory, repository) = repository();
    let session_id = create_session(&repository, "matching");
    let run_id = RunId::new();
    let snapshot = snapshot("safe-model", Some("https://models.example.test/v1"), 17, 2);
    repository
        .accept_user_turn(
            AcceptUserTurnInputDto::new(
                session_id,
                IdempotencyKey::new(),
                "turn",
                run_id,
                snapshot.clone(),
                time(2),
            )
            .expect("turn starts"),
        )
        .expect("turn persists its safe selection");

    let loaded = repository
        .load_run_config_snapshot(session_id, run_id)
        .expect("matching run configuration loads");
    assert_eq!(loaded, snapshot);
    assert_eq!(loaded.resolved().provider().model(), "safe-model");
    assert_eq!(
        loaded.resolved().provider().endpoint(),
        Some("https://models.example.test/v1")
    );
    assert_eq!(
        loaded
            .resolved()
            .provider_execution()
            .attempt_timeout_seconds(),
        17
    );
    assert_eq!(loaded.resolved().provider_execution().max_attempts(), 2);
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
            AcceptUserTurnInputDto::new(
                session_id,
                IdempotencyKey::new(),
                "turn",
                run_id,
                snapshot("safe-model", None, 30, 2),
                time(2),
            )
            .expect("turn input is valid"),
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
    let snapshot = snapshot("safe-model", None, 30, 2);
    let revision_id = snapshot.revision_id();
    repository
        .accept_user_turn(
            AcceptUserTurnInputDto::new(
                session_id,
                IdempotencyKey::new(),
                "turn",
                run_id,
                snapshot,
                time(2),
            )
            .expect("turn input is valid"),
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
        (
            "backend-failure",
            "PRAGMA foreign_keys = OFF;\nDROP TABLE runs;",
        ),
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
                AcceptUserTurnInputDto::new(
                    session_id,
                    IdempotencyKey::new(),
                    "turn",
                    run_id,
                    snapshot("safe-model", None, 30, 2),
                    time(2),
                )
                .expect("turn input is valid"),
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

fn snapshot(
    model: &str,
    endpoint: Option<&str>,
    attempt_timeout_seconds: u8,
    max_attempts: u8,
) -> ConfigSnapshotDto {
    let endpoint = endpoint.map_or_else(String::new, |value| format!("endpoint = \"{value}\"\n"));
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"{model}\"\n{endpoint}credential = \"recognizable-fixture-credential\"\n[provider.execution]\nattempt_timeout_seconds = {attempt_timeout_seconds}\nmax_attempts = {max_attempts}"
        ),
        ConfigSourceDto::Explicit(
            ConfigPathDto::parse(
                std::env::temp_dir()
                    .join("safe-config.toml")
                    .to_string_lossy()
                    .into_owned(),
            )
            .expect("configuration path is absolute"),
        ),
    ))
    .expect("safe configuration resolves");
    ConfigSnapshotDto::new(
        intention_proto::SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        time(1),
        resolved,
    )
    .expect("safe snapshot is valid")
}
