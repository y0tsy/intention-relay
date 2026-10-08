//! Shared fixtures for the intention-daemon integration suites.
//!
//! Each integration target compiles this module and uses a different subset of
//! the fixtures, so unused items are expected here. The module is a nested file
//! module, never a Cargo test target of its own.

#![allow(
    dead_code,
    reason = "every integration target compiles the shared module and uses a different subset"
)]
#![allow(
    clippy::expect_used,
    reason = "shared fixtures use expect to provide precise failures"
)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use intention_config::ConfigSnapshotDto;
use intention_daemon::DaemonApplicationFacade;
use intention_engine::{
    ModelRunCommitDto, ModelRunCommitObserver, ModelRunExecutionInputDto, ModelSleepFuture,
    ModelTimePort, RunCancellation,
};
use intention_proto::{
    CreateSessionCommandDto, IdempotencyKey, ProjectId, ProtocolAcceptedResultDto,
    ProtocolCommandDto, ProtocolCommandResultDto, RunId, RunModeDto, RunStatusDto,
    SendUserTurnCommandDto, SendUserTurnOutcomeDto, SessionId, TimestampDto, WorkspaceId,
    WorkspaceRootDto,
};
use intention_providers::{
    ModelExecutionDriver, ModelMessageDto, ModelRequestDto, ModelRoleDto, ModelToolDefinitionDto,
};
use intention_test_support::fixture_snapshot;
use intention_transport::LocalEndpoint;
use tempfile::TempDir;

/// Wall-clock time port backed by the Tokio timer.
pub struct TokioTime;

impl ModelTimePort for TokioTime {
    fn now(&self) -> TimestampDto {
        TimestampDto::from_unix_seconds(2).expect("fixture timestamp is valid")
    }

    fn sleep(&self, duration: Duration) -> ModelSleepFuture<'_> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// Records every committed value the daemon dispatch path publishes.
///
/// The daemon's suites observe committed values only through the facade's
/// execution bridge, so no shared crate fixture can replace this recorder.
#[derive(Default)]
pub struct RecordingObserver {
    commits: Mutex<Vec<ModelRunCommitDto>>,
}

impl RecordingObserver {
    /// Returns every committed transcript row and status, in publication order.
    pub fn commits(&self) -> Vec<ModelRunCommitDto> {
        self.commits
            .lock()
            .expect("observer recorder remains available")
            .clone()
    }

    /// Returns every committed status the runtime published, in order.
    pub fn observed_statuses(&self) -> Vec<RunStatusDto> {
        self.commits
            .lock()
            .expect("observer recorder remains available")
            .iter()
            .filter_map(|committed| match committed {
                ModelRunCommitDto::Status { status, .. } => Some(*status),
                ModelRunCommitDto::Content(_) => None,
            })
            .collect()
    }
}

impl ModelRunCommitObserver for RecordingObserver {
    fn observe_model_run_commit(&self, committed: &ModelRunCommitDto) {
        self.commits
            .lock()
            .expect("observer recorder remains available")
            .push(committed.clone());
    }
}

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
    let create = ProtocolCommandDto::CreateSession(CreateSessionCommandDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        WorkspaceRootDto::parse(workspace.to_string_lossy().into_owned())
            .expect("fixture workspace is absolute"),
        RunModeDto::Build,
    ));
    assert!(matches!(
        facade.command(create),
        ProtocolCommandResultDto::Accepted(_)
    ));
}

/// Starts one fixture turn and returns the run it admitted.
pub fn started_run(facade: &DaemonApplicationFacade, session_id: SessionId) -> RunId {
    let result = facade.command(ProtocolCommandDto::SendUserTurn(
        SendUserTurnCommandDto::new(session_id, IdempotencyKey::new(), "turn")
            .expect("turn is valid"),
    ));
    let ProtocolCommandResultDto::Accepted(accepted) = result else {
        panic!("fixture turn starts")
    };
    let ProtocolAcceptedResultDto::SendUserTurn(turn) = accepted.result() else {
        panic!("fixture result is a turn")
    };
    let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
        panic!("first turn starts")
    };
    run_id
}

/// Builds one scheduled model-run input over the shared fixture request.
///
/// `tools` carries the model-visible definitions for suites that exercise the
/// tool loop; `None` keeps the provider request tool-free.
pub fn schedule(
    session_id: SessionId,
    run_id: RunId,
    snapshot: ConfigSnapshotDto,
    tools: Option<Vec<ModelToolDefinitionDto>>,
) -> ModelRunExecutionInputDto {
    let request = ModelRequestDto::new(
        run_id,
        "fixture",
        vec![ModelMessageDto::new(ModelRoleDto::User, "turn").expect("message is valid")],
        None,
    )
    .expect("request is valid");
    let request = match tools {
        Some(tools) => request
            .with_tools(tools)
            .expect("tool advertisement is valid"),
        None => request,
    };
    ModelRunExecutionInputDto::new(
        session_id,
        run_id,
        request,
        snapshot,
        RunCancellation::new(),
    )
}

/// Returns the platform config path the daemon resolves from its environment.
pub fn config_path(config_home: &Path) -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        config_home.join("intention-relay").join("config.toml")
    }
    #[cfg(target_os = "macos")]
    {
        config_home
            .join("Library/Application Support/intention-relay")
            .join("config.toml")
    }
    #[cfg(windows)]
    {
        config_home.join("intention-relay").join("config.toml")
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        config_home.join("intention-relay").join("config.toml")
    }
}

static NEXT_ENDPOINT: AtomicUsize = AtomicUsize::new(0);

/// Builds a unique safe endpoint instance id for one fixture prefix.
pub fn unique_endpoint(prefix: &str) -> LocalEndpoint {
    let sequence = NEXT_ENDPOINT.fetch_add(1, Ordering::Relaxed);
    LocalEndpoint::from_instance_id(format!("{prefix}-{}-{}", std::process::id(), sequence))
        .expect("fixture endpoint is valid")
}

/// Replicates the daemon transport's platform endpoint path resolution so the
/// fixture can remove exactly the socket file its own daemon created.
#[cfg(unix)]
pub fn endpoint_socket_path(endpoint: &LocalEndpoint) -> Option<PathBuf> {
    let base = {
        #[cfg(target_os = "linux")]
        {
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(PathBuf::from)
                .filter(|candidate| candidate.is_absolute())
                .or_else(|| {
                    std::env::var_os("XDG_CONFIG_HOME")
                        .map(PathBuf::from)
                        .filter(|candidate| candidate.is_absolute())
                        .map(|candidate| candidate.join("intention-relay"))
                })
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .filter(|candidate| candidate.is_absolute())
                        .map(|candidate| candidate.join(".config/intention-relay"))
                })
        }
        #[cfg(target_os = "macos")]
        {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|candidate| candidate.is_absolute())
                .map(|candidate| candidate.join("Library/Application Support/intention-relay"))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            None
        }
    }?;
    Some(base.join(format!("{}.sock", endpoint.instance_id())))
}

/// Writes one daemon configuration document to the platform config path with
/// owner-only permissions on Unix.
pub fn write_config_document(config_home: &Path, document: &str) {
    let config_path = config_path(config_home);
    let parent = config_path.parent().expect("config path has a parent");
    std::fs::create_dir_all(parent).expect("config directory is created");
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut options = std::fs::OpenOptions::new();
        options.create(true).write(true).truncate(true).mode(0o600);
        let mut file = options.open(&config_path).expect("config file opens");
        file.write_all(document.as_bytes())
            .expect("config file writes");
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&config_path, document).expect("config file writes");
    }
}

/// Spawns the real daemon binary with only per-process environment overrides.
///
/// A supplied `log_path` redirects both output streams into that file; without
/// one the daemon inherits them. `removed_environment` names variables the
/// child must never inherit.
pub fn spawn_daemon(
    endpoint: &LocalEndpoint,
    config_home: &Path,
    state_home: &Path,
    log_path: Option<&Path>,
    removed_environment: &[&str],
) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_intention-daemon"));
    command.arg(endpoint.instance_id());
    if let Some(log_path) = log_path {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .expect("daemon log file opens");
        let log_errors = log.try_clone().expect("daemon log file clones");
        command.stdout(Stdio::from(log));
        command.stderr(Stdio::from(log_errors));
    }
    for variable in removed_environment {
        command.env_remove(*variable);
    }
    #[cfg(target_os = "linux")]
    {
        command.env("XDG_CONFIG_HOME", config_home);
        command.env("XDG_STATE_HOME", state_home);
    }
    #[cfg(target_os = "macos")]
    {
        // Both the configuration and the state directory derive from HOME.
        command.env("HOME", config_home);
    }
    #[cfg(windows)]
    {
        command.env("APPDATA", config_home);
        command.env("LOCALAPPDATA", state_home);
    }
    command.spawn().expect("daemon binary spawns")
}
