//! Real-daemon process fixtures for daemon-host integration suites.
//!
//! The fixtures locate the `intention-daemon` binary, spawn it with only
//! explicit per-process environment overrides, wait for its typed readiness,
//! and kill it within a bounded window, so every suite drives the real process
//! boundary without duplicating harness code.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use intention_client::{IntentionClient, ProcessDaemonLauncher};
use intention_transport::LocalEndpoint;

/// The bounded window one killed daemon process has to become reapable.
const KILL_DEADLINE: Duration = Duration::from_secs(5);

/// Returns the executable path of the real daemon binary the fixtures spawn.
///
/// `INTENTION_DAEMON_BIN` overrides the resolution. Cargo builds every target of
/// a profile into that profile's directory and runs a test binary from the
/// profile's `deps` directory, so the daemon binary sits exactly one directory
/// above the running executable's own. The platform executable suffix is
/// applied.
///
/// The crate that owns the daemon binary names it directly instead, through the
/// `CARGO_BIN_EXE_intention-daemon` value Cargo defines for its integration
/// tests; this derivation is what a suite in another crate - which Cargo gives
/// no such value - resolves it with.
///
/// # Panics
///
/// Panics when the running test executable has no resolvable profile directory.
#[must_use]
pub fn daemon_binary_path() -> PathBuf {
    if let Some(program) = std::env::var_os("INTENTION_DAEMON_BIN") {
        return PathBuf::from(program);
    }
    let executable = std::env::current_exe().expect("the running test executable path resolves");
    let profile = executable
        .parent()
        .and_then(Path::parent)
        .expect("the running test executable sits in its profile's deps directory");
    profile.join(daemon_binary_name())
}

/// Returns the platform file name of the daemon executable.
fn daemon_binary_name() -> String {
    format!("intention-daemon{}", std::env::consts::EXE_SUFFIX)
}

/// Returns the platform config path the daemon resolves from its environment.
#[must_use]
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
///
/// # Panics
///
/// Panics when the built instance id is not a valid endpoint identity.
#[must_use]
pub fn unique_endpoint(prefix: &str) -> LocalEndpoint {
    let sequence = NEXT_ENDPOINT.fetch_add(1, Ordering::Relaxed);
    LocalEndpoint::from_instance_id(format!("{prefix}-{}-{}", std::process::id(), sequence))
        .expect("fixture endpoint is valid")
}

/// Replicates the daemon transport's platform endpoint path resolution so the
/// fixture can remove exactly the socket file its own daemon created.
#[cfg(unix)]
#[must_use]
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
///
/// # Panics
///
/// Panics when the config directory or the config file cannot be written.
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
///
/// # Panics
///
/// Panics when the daemon log file cannot be opened or the daemon binary does
/// not spawn.
#[must_use]
pub fn spawn_daemon(
    endpoint: &LocalEndpoint,
    config_home: &Path,
    state_home: &Path,
    log_path: Option<&Path>,
    removed_environment: &[&str],
) -> Child {
    let mut command = Command::new(daemon_binary_path());
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

/// Kills one spawned daemon process within the bounded kill window and reaps it.
///
/// The first signal normally reaps the child on the first poll; when it does
/// not, the hard kill is retried and the fallback reap is bounded by the same
/// deadline, so a process that never becomes reapable cannot hang the suite. A
/// missing child is a no-op, so a Drop path may call this unconditionally.
pub fn kill(daemon: &mut Option<Child>) {
    let Some(mut child) = daemon.take() else {
        return;
    };
    let _ = child.kill();
    let deadline = Instant::now() + KILL_DEADLINE;
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            let _ = child.wait();
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Waits for the spawned daemon to report `Ready` through the shared client.
///
/// `await_ready()` only connects and queries; it never launches the daemon.
/// Its own bounded wait is looped under the fixture deadline, so a daemon that
/// needs longer than one client budget still becomes ready in time.
///
/// # Panics
///
/// Panics when the daemon does not report readiness before `deadline`.
#[must_use]
pub async fn wait_until_ready(endpoint: &LocalEndpoint, deadline: Instant) -> IntentionClient {
    let client = IntentionClient::new(
        endpoint.clone(),
        Box::new(
            ProcessDaemonLauncher::new(daemon_binary_path().to_string_lossy().into_owned())
                .expect("daemon program is valid"),
        ),
    );
    while Instant::now() < deadline {
        if client.await_ready().await.is_ok() {
            return client;
        }
    }
    panic!("daemon becomes ready before the deadline");
}
