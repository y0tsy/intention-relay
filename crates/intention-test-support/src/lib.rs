//! Non-production fixtures for durable M3 integration tests.

use std::path::Path;
use std::thread;

use tempfile::TempDir;

use intention::DaemonApplicationFacade;
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_daemon::serve_test_connection;
use intention_domain::{CreateSessionCommandDto, RunModeDto, WorkspaceRootDto};
use intention_protocol::{
    DaemonReadinessDto, ProtocolCommandDto, ProtocolCommandResultDto, ProtocolQueryDto,
    ProtocolQueryResultDto,
};
use intention_transport::{LocalEndpoint, LocalListener};
use intention_types::{
    ConfigRevisionId, DtoResult, ProjectId, SchemaVersionDto, SessionId, TimestampDto, WorkspaceId,
};

/// Opens a durable facade at an explicit test-only database path.
///
/// # Errors
///
/// Returns the typed facade startup failure.
pub fn open_facade(
    path: impl AsRef<Path>,
    snapshot: ConfigSnapshotDto,
) -> DtoResult<DaemonApplicationFacade> {
    DaemonApplicationFacade::open_for_test_support(path, snapshot)
}

/// Creates a durable facade with a controlled credential-free fixture snapshot.
///
/// # Errors
///
/// Returns the typed facade startup failure.
pub fn open_fixture_facade(path: impl AsRef<Path>) -> DtoResult<DaemonApplicationFacade> {
    open_facade(path, fixture_snapshot())
}

/// Returns a credential-free fixture snapshot with a native absolute source path.
#[must_use]
pub fn fixture_snapshot() -> ConfigSnapshotDto {
    let source = ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-relay-fixtures.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .unwrap_or_else(|_| unreachable!("fixture configuration source is absolute")),
    );
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        "schema_version = 1\n[provider]\nkind = \"openrouter\"\nmodel = \"fixture\"\ncredential = \"fixture-credential\"",
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

/// Returns a controlled session-creation command for integration scenarios.
#[must_use]
pub fn fixture_session_command(session_id: SessionId) -> CreateSessionCommandDto {
    CreateSessionCommandDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        fixture_workspace_root("m3-test-support"),
        RunModeDto::Build,
    )
}

/// Creates a durable fixture session through the public protocol facade.
///
/// # Errors
///
/// Returns the typed protocol rejection.
pub fn create_fixture_session(
    facade: &DaemonApplicationFacade,
    session_id: SessionId,
) -> DtoResult<()> {
    match facade.command(ProtocolCommandDto::CreateSession(fixture_session_command(
        session_id,
    ))) {
        ProtocolCommandResultDto::Accepted(_) => Ok(()),
        ProtocolCommandResultDto::Rejected(error) => Err(error),
    }
}

/// Returns whether the fixture facade reports ready through the protocol boundary.
#[must_use]
pub fn fixture_ready(facade: &DaemonApplicationFacade) -> bool {
    matches!(
        facade.query(ProtocolQueryDto::GetDaemonHealth),
        ProtocolQueryResultDto::DaemonHealth(health) if health.readiness() == DaemonReadinessDto::Ready
    )
}

/// Loads the fixture session's durable event history through the test-only facade seam.
///
/// # Errors
///
/// Returns the typed durable-history failure.
pub fn durable_events(
    facade: &DaemonApplicationFacade,
    session_id: SessionId,
) -> DtoResult<Vec<intention_types::EventEnvelopeDto<intention_domain::DomainEventDto>>> {
    facade.durable_events_for_test_support(session_id)
}

/// Owns a durable fixture database and its configured session lifetime.
pub struct FixtureHost {
    directory: TempDir,
    facade: DaemonApplicationFacade,
}

impl FixtureHost {
    /// Opens a durable fixture host with one ready session.
    ///
    /// # Errors
    ///
    /// Returns a typed storage or session-creation failure.
    pub fn open(session_id: SessionId) -> DtoResult<Self> {
        let directory = TempDir::new().map_err(|_| {
            intention_types::ErrorDto::unavailable(
                "fixture_storage_unavailable",
                "fixture durable storage is unavailable",
            )
        })?;
        let facade = open_fixture_facade(directory.path().join("fixture.sqlite"))?;
        create_fixture_session(&facade, session_id)?;
        Ok(Self { directory, facade })
    }

    /// Starts a bounded local fixture host and transfers database ownership to the serving thread.
    #[must_use]
    pub fn spawn(
        self,
        endpoint: LocalEndpoint,
        connection_count: usize,
    ) -> thread::JoinHandle<DtoResult<()>> {
        thread::spawn(move || {
            let Self { directory, facade } = self;
            let _directory = directory;
            serve_fixture_connections(endpoint, facade, connection_count)
        })
    }
}

/// Serves a bounded fixture connection count through the real daemon dispatch path.
///
/// # Errors
///
/// Returns a typed listener or connection failure.
fn serve_fixture_connections(
    endpoint: LocalEndpoint,
    facade: DaemonApplicationFacade,
    connection_count: usize,
) -> DtoResult<()> {
    let listener = LocalListener::bind(endpoint)?;
    for _ in 0..connection_count {
        serve_test_connection(listener.accept()?, facade.clone());
    }
    Ok(())
}
