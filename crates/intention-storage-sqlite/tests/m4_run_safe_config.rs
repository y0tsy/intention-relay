#![allow(
    clippy::expect_used,
    reason = "M4 SQLite safe configuration fixtures use expect for precise diagnostics."
)]

use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_domain::{CreateSessionCommandDto, RunModeDto, WorkspaceRootDto};
use intention_storage::{AcceptUserTurnInputDto, CreateSessionInputDto, StorageRepositoryDto};
use intention_storage_sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};
use intention_types::{
    ConfigRevisionId, ProjectId, RunId, SessionId, TimestampDto, TurnId, WorkspaceId,
};
use tempfile::TempDir;

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
                TurnId::new(),
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
                TurnId::new(),
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

fn repository() -> (TempDir, SqliteStorageRepository) {
    let directory = TempDir::new().expect("temporary directory exists");
    let repository = SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(
            directory
                .path()
                .join("storage.sqlite")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("database path is absolute"),
    )
    .expect("database opens");
    (directory, repository)
}

fn create_session(repository: &SqliteStorageRepository, label: &str) -> SessionId {
    let session_id = SessionId::new();
    repository
        .create_session(CreateSessionInputDto::new(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                WorkspaceRootDto::parse(
                    std::env::temp_dir()
                        .join("intention-m4-run-safe-config")
                        .join(label)
                        .to_string_lossy()
                        .into_owned(),
                )
                .expect("workspace root is absolute"),
                RunModeDto::Build,
            ),
            time(1),
        ))
        .expect("session creates");
    session_id
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
        intention_types::SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        time(1),
        resolved,
    )
    .expect("safe snapshot is valid")
}

fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture time is valid")
}
