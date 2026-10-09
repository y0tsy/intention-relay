//! Non-production fixtures for durable integration tests.

mod model_fixtures;

use std::path::Path;

use tempfile::TempDir;

pub use crate::model_fixtures::{ScriptedDriver, run_ready};
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_daemon::{DaemonApplicationFacade, TestHostLifecycle};
use intention_proto::{
    ConfigRevisionId, DtoResult, ProjectId, ProtocolResultDto, SchemaVersionDto, SessionId,
    TimestampDto, WorkspaceId,
};
use intention_proto::{CreateSessionCommandDto, RunModeDto, WorkspaceRootDto};
use intention_transport::{AsyncLocalListener, LocalEndpoint};

/// The provider credential every fixture snapshot carries.
///
/// Suites assert against this constant instead of repeating the literal, so a
/// fixture change can never silently drift away from the assertions.
pub const FIXTURE_CREDENTIAL: &str = "fixture-secret";

/// Creates a durable facade with a controlled credential-free fixture snapshot.
///
/// # Errors
///
/// Returns the typed facade startup failure.
fn open_fixture_facade(path: impl AsRef<Path>) -> DtoResult<DaemonApplicationFacade> {
    DaemonApplicationFacade::open_for_test_support(path, fixture_snapshot())
}

/// Returns a credential-free fixture snapshot with a native absolute source path.
#[must_use]
pub fn fixture_snapshot() -> ConfigSnapshotDto {
    fixture_snapshot_with_model("fixture")
}

/// Returns a fixture snapshot that explicitly selects one provider model.
#[must_use]
pub fn fixture_snapshot_with_model(model: &str) -> ConfigSnapshotDto {
    fixture_snapshot_with_context_window(model, None)
}

/// Returns a fixture snapshot with the exact context-window policy the caller requests.
///
/// `None` keeps the resolved configuration's default window policy; `Some`
/// writes the window token count the sliding-window fixtures drive.
#[must_use]
pub fn fixture_snapshot_with_context_window(
    model: &str,
    window_tokens: Option<u64>,
) -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-relay-fixtures.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .unwrap_or_else(|_| unreachable!("fixture configuration source is absolute")),
    );
    let context_window = window_tokens.map_or_else(String::new, |window| {
        format!("context_window_tokens = {window}\n")
    });
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        format!(
            "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"{model}\"\ncredential = \"{FIXTURE_CREDENTIAL}\"\n{context_window}"
        ),
        source,
    ))
    .unwrap_or_else(|_| unreachable!("fixture configuration resolves"));
    ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        TimestampDto::from_unix_seconds(1)
            .unwrap_or_else(|_| unreachable!("fixture timestamp is valid")),
        resolved,
    )
    .unwrap_or_else(|_| unreachable!("fixture snapshot is credential-free"))
}

/// Returns a native absolute workspace root for a controlled fixture label.
#[must_use]
pub fn fixture_workspace_root(label: &str) -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-relay-fixtures")
            .join(label)
            .to_string_lossy()
            .into_owned(),
    )
    .unwrap_or_else(|_| unreachable!("native temporary fixture root is absolute"))
}

/// Returns a controlled session-creation command for the fixture session.
#[must_use]
fn fixture_session_command(session_id: SessionId) -> CreateSessionCommandDto {
    CreateSessionCommandDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        fixture_workspace_root("m3-test-support"),
        RunModeDto::Build,
    )
}

/// Creates a durable fixture session through the public facade.
///
/// # Errors
///
/// Returns the typed facade rejection.
fn create_fixture_session(
    facade: &DaemonApplicationFacade,
    session_id: SessionId,
) -> DtoResult<()> {
    match facade.create_session(fixture_session_command(session_id))? {
        ProtocolResultDto::SessionCreated(_) => Ok(()),
        _ => unreachable!("fixture session creation answers with a session creation"),
    }
}

/// Owns a durable fixture database, one configured session, and its fixture host.
pub struct FixtureHost {
    directory: TempDir,
    lifecycle: TestHostLifecycle,
}

impl FixtureHost {
    /// Opens a durable fixture host with one ready session.
    ///
    /// # Errors
    ///
    /// Returns a typed storage or session-creation failure.
    pub fn open(session_id: SessionId) -> DtoResult<Self> {
        let directory = TempDir::new().map_err(|_| {
            intention_proto::ErrorDto::unavailable(
                "fixture_storage_unavailable",
                "fixture durable storage is unavailable",
            )
        })?;
        let facade = open_fixture_facade(directory.path().join("fixture.sqlite"))?;
        create_fixture_session(&facade, session_id)?;
        let lifecycle = intention_daemon::test_host_lifecycle(facade);
        Ok(Self {
            directory,
            lifecycle,
        })
    }

    /// Serves a bounded fixture connection count through the real daemon dispatch path.
    ///
    /// The database directory stays owned by this call, so a fixture that serves
    /// for the whole scenario keeps its durable storage alive.
    ///
    /// # Errors
    ///
    /// Returns a typed listener failure.
    pub async fn serve(self, endpoint: LocalEndpoint, connection_count: usize) -> DtoResult<()> {
        let listener = AsyncLocalListener::bind(endpoint)?;
        let Self {
            directory,
            lifecycle,
        } = self;
        let _directory = directory;
        lifecycle
            .serve_connections(listener, connection_count)
            .await;
        Ok(())
    }
}
