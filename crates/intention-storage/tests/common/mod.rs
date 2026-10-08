#![allow(
    clippy::expect_used,
    reason = "Shared storage fixtures use expect for precise test diagnostics."
)]

use intention_proto::{
    CreateSessionCommandDto, ProjectId, RunModeDto, SessionId, TimestampDto, WorkspaceId,
    WorkspaceRootDto,
};
use intention_storage::{
    CreateSessionInputDto, SqliteDatabaseLocationDto, SqliteStorageRepository, StorageRepositoryDto,
};
use tempfile::TempDir;

/// Returns one fixture timestamp in whole unix seconds.
pub fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture time is valid")
}

/// Returns one native fixture workspace root labeled `label`.
pub fn workspace_root(label: &str) -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-storage-tests")
            .join(label)
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

/// Opens the suite's SQLite database inside one temporary directory.
pub fn open(directory: &TempDir) -> SqliteStorageRepository {
    SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(
            directory
                .path()
                .join("storage.sqlite")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("temp location is absolute"),
    )
    .expect("database opens")
}

/// Reopens the suite's SQLite database after the repository was dropped.
pub fn reopen(directory: &TempDir) -> SqliteStorageRepository {
    open(directory)
}

/// Creates one temporary directory together with its open repository.
pub fn repository() -> (TempDir, SqliteStorageRepository) {
    let directory = TempDir::new().expect("temporary directory exists");
    let store = open(&directory);
    (directory, store)
}

/// Creates one durable session whose workspace root is labeled `label`.
pub fn create_session(repository: &SqliteStorageRepository, label: &str) -> SessionId {
    let session_id = SessionId::new();
    repository
        .create_session(CreateSessionInputDto::new(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                workspace_root(label),
                RunModeDto::Build,
            ),
            time(1),
        ))
        .expect("session creates");
    session_id
}
