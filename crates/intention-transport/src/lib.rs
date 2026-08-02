//! Framed, local-only IPC for Intention Relay.
//!
//! The public surface accepts only `intention-protocol` DTOs. A bounded,
//! length-prefixed JSON codec remains private to this crate, and the underlying
//! Unix-domain socket or Windows named pipe never crosses its crate boundary.

#[cfg(unix)]
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use intention_protocol::{
    ProtocolHelloDto, ProtocolRequestEnvelopeDto, ProtocolResponseEnvelopeDto, ProtocolVersionDto,
};
use intention_types::{DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto};
use interprocess::ConnectWaitMode;
use interprocess::local_socket::prelude::{LocalSocketListener, LocalSocketStream};
use interprocess::local_socket::traits::Listener as _;
use interprocess::local_socket::{ConnectOptions, GenericFilePath, ListenerOptions, PathNameType};

const FRAME_LENGTH_BYTES: usize = 4;
const MAX_FRAME_BYTES: usize = 1_048_576;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const LISTENER_SPIN_TIMEOUT: Duration = Duration::from_millis(500);

/// A validated, private-to-the-current-user location for a local daemon endpoint.
///
/// The value is intentionally not serializable and is never included in a public
/// protocol DTO or an error message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalEndpoint {
    instance_id: String,
    path: PathBuf,
}

impl LocalEndpoint {
    /// Creates a local endpoint from a safe logical instance identifier.
    ///
    /// The identifier is not an operating-system path. It contains only ASCII
    /// letters, digits, `_`, and `-`, and resolves below the current user's
    /// platform runtime directory.
    ///
    /// # Errors
    ///
    /// Returns a validation error for an unsafe logical identifier or an
    /// unavailable error if the platform runtime directory cannot be determined.
    pub fn from_instance_id(instance_id: impl Into<String>) -> DtoResult<Self> {
        let instance_id = instance_id.into();
        let valid = !instance_id.is_empty()
            && instance_id.len() <= 100
            && instance_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
        if !valid {
            return Err(ErrorDto::validation(
                "invalid_local_endpoint_instance",
                "local daemon instance identifier must be a safe logical name",
            ));
        }
        Ok(Self {
            path: platform_endpoint_path(&instance_id)?,
            instance_id,
        })
    }

    /// Derives the standard per-user daemon endpoint.
    ///
    /// # Errors
    ///
    /// Returns a safe unavailable error when a usable platform runtime directory
    /// cannot be determined.
    pub fn platform_default() -> DtoResult<Self> {
        Self::from_instance_id("intention-relay")
    }

    /// Returns the safe logical endpoint instance identifier.
    #[must_use]
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    fn socket_name(&self) -> DtoResult<interprocess::local_socket::Name<'_>> {
        GenericFilePath::map(self.path.as_os_str().into())
            .map_err(|_| unavailable("local_endpoint_unavailable"))
    }
}

fn platform_endpoint_path(instance_id: &str) -> DtoResult<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
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
            .ok_or_else(|| unavailable("local_runtime_directory_unavailable"))?;
        Ok(base.join(format!("{instance_id}.sock")))
    }
    #[cfg(target_os = "macos")]
    {
        let base = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|candidate| candidate.is_absolute())
            .map(|candidate| candidate.join("Library/Application Support/intention-relay"))
            .ok_or_else(|| unavailable("local_runtime_directory_unavailable"))?;
        Ok(base.join(format!("{instance_id}.sock")))
    }
    #[cfg(windows)]
    {
        Ok(PathBuf::from(format!(r"\\.\pipe\{instance_id}")))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err(ErrorDto::unavailable(
            "local_transport_unsupported",
            "the current platform does not support the local daemon transport",
        ))
    }
}

/// A local framed connection which exchanges only typed protocol DTOs.
pub struct LocalConnection {
    stream: LocalSocketStream,
}

impl LocalConnection {
    /// Connects to the endpoint with a bounded wait.
    ///
    /// # Errors
    ///
    /// Returns a safe typed unavailable error when the daemon endpoint cannot be
    /// reached, rather than exposing an OS path or error string.
    pub fn connect(endpoint: &LocalEndpoint) -> DtoResult<Self> {
        let stream = ConnectOptions::new()
            .name(endpoint.socket_name()?)
            .wait_mode(ConnectWaitMode::Timeout(CONNECT_TIMEOUT))
            .connect_sync()
            .map_err(|_| unavailable("local_daemon_unavailable"))?;
        Ok(Self { stream })
    }

    /// Writes one bounded request envelope.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable error when the connected peer no longer
    /// accepts data or the DTO cannot be encoded into the fixed protocol codec.
    pub fn send_request(&mut self, request: &ProtocolRequestEnvelopeDto) -> DtoResult<()> {
        write_frame(&mut self.stream, request)
    }

    /// Reads one bounded request envelope.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or unavailable error for malformed, oversized,
    /// incomplete, or disconnected frames.
    pub fn receive_request(&mut self) -> DtoResult<ProtocolRequestEnvelopeDto> {
        read_frame(&mut self.stream)
    }

    /// Writes one bounded response envelope.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable error when the connected peer no longer
    /// accepts data or the DTO cannot be encoded into the fixed protocol codec.
    pub fn send_response(&mut self, response: &ProtocolResponseEnvelopeDto) -> DtoResult<()> {
        write_frame(&mut self.stream, response)
    }

    /// Reads one bounded response envelope.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or unavailable error for malformed, oversized,
    /// incomplete, or disconnected frames.
    pub fn receive_response(&mut self) -> DtoResult<ProtocolResponseEnvelopeDto> {
        read_frame(&mut self.stream)
    }

    /// Sends a typed protocol hello during connection negotiation.
    ///
    /// # Errors
    ///
    /// Returns a typed unavailable error when the connection cannot send a
    /// complete frame.
    pub fn send_hello(&mut self, hello: &ProtocolHelloDto) -> DtoResult<()> {
        write_frame(&mut self.stream, hello)
    }

    /// Receives a typed protocol hello during connection negotiation.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or unavailable error for malformed, oversized,
    /// incomplete, or disconnected frames.
    pub fn receive_hello(&mut self) -> DtoResult<ProtocolHelloDto> {
        read_frame(&mut self.stream)
    }
}

/// A local listener that accepts framed, typed client connections.
pub struct LocalListener {
    listener: LocalSocketListener,
    #[cfg(unix)]
    endpoint: LocalEndpoint,
}

impl LocalListener {
    /// Binds a user-private local endpoint.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error when the parent directory cannot be prepared,
    /// the endpoint is already serving another daemon, or platform IPC is
    /// unavailable. It never removes a path that was not created by this host.
    pub fn bind(endpoint: LocalEndpoint) -> DtoResult<Self> {
        prepare_parent_directory(&endpoint)?;
        let listener = listener_options(&endpoint)?.create_sync().map_err(|_| {
            ErrorDto::new(
                "local_daemon_endpoint_in_use",
                ErrorCategoryDto::Conflict,
                "the local daemon endpoint is already in use",
                ErrorRetryDto::Immediate,
                None,
            )
            .unwrap_or_else(|_| unavailable("local_daemon_endpoint_in_use"))
        })?;
        Ok(Self {
            listener,
            #[cfg(unix)]
            endpoint,
        })
    }

    /// Accepts one client connection.
    ///
    /// # Errors
    ///
    /// Returns a safe typed unavailable error if the listener cannot accept the
    /// next peer connection.
    pub fn accept(&self) -> DtoResult<LocalConnection> {
        let stream = self
            .listener
            .accept()
            .map_err(|_| unavailable("local_daemon_connection_unavailable"))?;
        Ok(LocalConnection { stream })
    }
}

impl Drop for LocalListener {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = fs::remove_file(&self.endpoint.path);
        }
    }
}

/// Performs the mandatory client/daemon hello exchange.
///
/// # Errors
///
/// Returns the typed protocol mismatch error when the peer major version differs,
/// or a typed transport error when the handshake cannot complete.
pub fn negotiate_client(
    connection: &mut LocalConnection,
    local: ProtocolHelloDto,
) -> DtoResult<ProtocolHelloDto> {
    connection.send_hello(&local)?;
    let remote = connection.receive_hello()?;
    local.version().ensure_compatible_with(remote.version())?;
    Ok(remote)
}

/// Performs the daemon side of the mandatory hello exchange.
///
/// # Errors
///
/// Returns the typed protocol mismatch error when the peer major version differs,
/// or a typed transport error when the handshake cannot complete.
pub fn negotiate_daemon(
    connection: &mut LocalConnection,
    local: ProtocolHelloDto,
) -> DtoResult<ProtocolHelloDto> {
    let remote = connection.receive_hello()?;
    local.version().ensure_compatible_with(remote.version())?;
    connection.send_hello(&local)?;
    Ok(remote)
}

/// Returns the currently implemented local protocol version.
#[must_use]
pub const fn local_protocol_version() -> ProtocolVersionDto {
    ProtocolVersionDto::new(1, 0)
}

fn listener_options(endpoint: &LocalEndpoint) -> DtoResult<ListenerOptions<'_>> {
    let options = ListenerOptions::new()
        .name(endpoint.socket_name()?)
        .reclaim_name(false)
        .try_overwrite(false)
        .max_spin_time(LISTENER_SPIN_TIMEOUT);
    #[cfg(unix)]
    {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;

        Ok(options.mode(0o600))
    }
    #[cfg(not(unix))]
    {
        Ok(options)
    }
}

#[cfg(unix)]
fn prepare_parent_directory(endpoint: &LocalEndpoint) -> DtoResult<()> {
    let parent = endpoint.path.parent().ok_or_else(|| {
        ErrorDto::validation(
            "invalid_local_endpoint",
            "local daemon endpoint must have a parent directory",
        )
    })?;
    fs::create_dir_all(parent).map_err(|_| unavailable("local_runtime_directory_unavailable"))?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
        .map_err(|_| unavailable("local_runtime_directory_unavailable"))?;
    Ok(())
}

#[cfg(not(unix))]
const fn prepare_parent_directory(_endpoint: &LocalEndpoint) -> DtoResult<()> {
    Ok(())
}

fn write_frame<T: serde::Serialize>(stream: &mut LocalSocketStream, value: &T) -> DtoResult<()> {
    let payload = serde_json::to_vec(value).map_err(|_| {
        ErrorDto::validation(
            "local_protocol_encode_failed",
            "a typed local protocol message could not be encoded",
        )
    })?;
    let length = u32::try_from(payload.len()).map_err(|_| oversized_frame())?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(oversized_frame());
    }
    stream
        .write_all(&length.to_be_bytes())
        .and_then(|_| stream.write_all(&payload))
        .and_then(|_| stream.flush())
        .map_err(|_| unavailable("local_daemon_connection_unavailable"))
}

fn read_frame<T: serde::de::DeserializeOwned>(stream: &mut LocalSocketStream) -> DtoResult<T> {
    let mut header = [0_u8; FRAME_LENGTH_BYTES];
    stream
        .read_exact(&mut header)
        .map_err(|_| unavailable("local_daemon_connection_unavailable"))?;
    let length = usize::try_from(u32::from_be_bytes(header)).map_err(|_| oversized_frame())?;
    if length > MAX_FRAME_BYTES {
        return Err(oversized_frame());
    }
    let mut payload = vec![0_u8; length];
    stream
        .read_exact(&mut payload)
        .map_err(|_| unavailable("local_daemon_connection_unavailable"))?;
    serde_json::from_slice(&payload).map_err(|_| {
        ErrorDto::validation(
            "invalid_local_protocol_frame",
            "a local protocol frame was invalid",
        )
    })
}

fn oversized_frame() -> ErrorDto {
    ErrorDto::validation(
        "local_protocol_frame_too_large",
        "a local protocol frame exceeded the configured limit",
    )
}

fn unavailable(code: &'static str) -> ErrorDto {
    ErrorDto::unavailable(code, "the local daemon connection is unavailable")
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "Transport unit fixtures use direct assertions for diagnostics."
    )]

    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);

    fn endpoint() -> LocalEndpoint {
        let sequence = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after Unix epoch")
            .as_nanos();
        LocalEndpoint::from_instance_id(format!("transport-unit-{nanos}-{sequence}"))
            .expect("fixture endpoint is valid")
    }

    #[test]
    fn platform_default_uses_a_safe_logical_instance_identifier() {
        let endpoint = LocalEndpoint::platform_default().expect("platform default is available");
        assert_eq!(endpoint.instance_id(), "intention-relay");
        assert!(endpoint.path.is_absolute());
    }

    #[cfg(unix)]
    #[test]
    fn listener_enforces_private_permissions_and_removes_owned_socket() {
        use std::os::unix::fs::PermissionsExt;

        let endpoint = endpoint();
        let socket_path = endpoint.path.clone();
        let parent = socket_path
            .parent()
            .expect("socket has a parent")
            .to_owned();
        let listener = LocalListener::bind(endpoint).expect("listener binds");
        assert_eq!(
            fs::metadata(parent)
                .expect("parent metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&socket_path)
                .expect("socket metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        drop(listener);
        assert!(
            !socket_path.exists(),
            "listener removes only its owned socket"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_endpoints_resolve_to_local_named_pipes() {
        let endpoint = endpoint();
        assert_eq!(
            endpoint.path,
            PathBuf::from(format!(r"\\.\pipe\{}", endpoint.instance_id()))
        );
    }

    #[test]
    fn receive_request_rejects_oversized_malformed_and_incomplete_frames() {
        for frame in [
            (
                u32::try_from(MAX_FRAME_BYTES + 1).expect("frame length fits"),
                Vec::new(),
            ),
            (3, b"{".to_vec()),
            (8, b"{}".to_vec()),
        ] {
            let endpoint = endpoint();
            let listener = LocalListener::bind(endpoint.clone()).expect("listener binds");
            let server = thread::spawn(move || {
                let mut connection = listener.accept().expect("server accepts");
                connection
                    .receive_request()
                    .expect_err("invalid frame is rejected")
            });
            let mut client = LocalConnection::connect(&endpoint).expect("client connects");
            client
                .stream
                .write_all(&frame.0.to_be_bytes())
                .and_then(|_| client.stream.write_all(&frame.1))
                .expect("raw fixture frame writes");
            drop(client);
            let error = server.join().expect("server completes");
            assert!(matches!(
                error.code(),
                "local_protocol_frame_too_large"
                    | "invalid_local_protocol_frame"
                    | "local_daemon_connection_unavailable"
            ));
        }
    }

    #[test]
    fn receive_hello_reports_a_typed_error_when_peer_disconnects() {
        let endpoint = endpoint();
        let listener = LocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = thread::spawn(move || {
            let connection = listener.accept().expect("server accepts");
            drop(connection);
        });
        let mut client = LocalConnection::connect(&endpoint).expect("client connects");
        server.join().expect("server completes");
        let error = client.receive_hello().expect_err("closed peer is typed");
        assert_eq!(error.code(), "local_daemon_connection_unavailable");
    }
}
