//! Shared fixtures for the intention-daemon integration suites.
//!
//! The streaming-foundation target compiles this module and uses the fixtures
//! only under its `test-support` feature, so unused items are expected here.
//! The module is a nested file module, never a Cargo test target of its own.

#![allow(
    dead_code,
    reason = "streaming-foundation fixtures exist only under its test-support feature"
)]
#![allow(
    clippy::expect_used,
    reason = "shared fixtures use expect to provide precise failures"
)]

use std::path::Path;
use std::sync::Arc;

use intention_config::ConfigSnapshotDto;
use intention_daemon::DaemonApplicationFacade;
use intention_proto::{
    CreateSessionCommandDto, ProjectId, ProtocolResultDto, RunModeDto, SessionId, WorkspaceId,
    WorkspaceRootDto,
};
use intention_providers::ModelExecutionDriver;
use intention_test_support::fixture_snapshot;
use tempfile::TempDir;

/// Opens a durable fixture facade at one labelled test-only database path.
pub fn fixture_facade(
    label: &str,
    driver: Arc<dyn ModelExecutionDriver + Send + Sync>,
) -> (TempDir, DaemonApplicationFacade, ConfigSnapshotDto) {
    let directory = TempDir::new().expect("temporary directory exists");
    let snapshot = fixture_snapshot();
    let facade = DaemonApplicationFacade::open_for_test_support_with_driver(
        directory.path().join(format!("{label}.sqlite")),
        snapshot.clone(),
        driver,
    )
    .expect("fixture facade opens");
    (directory, facade, snapshot)
}

/// Creates one fixture session rooted at the supplied workspace directory.
pub fn create_session(facade: &DaemonApplicationFacade, session_id: SessionId, workspace: &Path) {
    let created = facade
        .create_session(CreateSessionCommandDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            WorkspaceRootDto::parse(workspace.to_string_lossy().into_owned())
                .expect("fixture workspace is absolute"),
            RunModeDto::Build,
        ))
        .expect("fixture session creates");
    assert!(matches!(created, ProtocolResultDto::SessionCreated(_)));
}
