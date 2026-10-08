//! NDJSON-framed, local-only IPC for Intention Relay.
//!
//! The public surface carries serialized JSON-RPC 2.0 envelopes defined by
//! `intention-proto`; one envelope is written per NDJSON line. Framing stays
//! private to this crate, and the underlying Unix-domain socket or Windows
//! named pipe never crosses its crate boundary.

#[cfg(unix)]
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use intention_proto::{DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto};
use intention_proto::{
    JsonRpcRequestDto, JsonRpcResponseDto, ProtocolHelloDto, ProtocolVersionDto,
    decode_hello_request, decode_hello_response, encode_hello_request, encode_hello_response,
};
use interprocess::ConnectWaitMode;
use interprocess::local_socket::tokio::{
    Listener as TokioLocalSocketListener, RecvHalf as TokioRecvHalf, SendHalf as TokioSendHalf,
    Stream as TokioLocalSocketStream,
};
use interprocess::local_socket::traits::tokio::{Listener as _, Stream as _};
use interprocess::local_socket::{ConnectOptions, GenericFilePath, ListenerOptions, PathNameType};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_MESSAGE_BYTES: usize = 1_048_576;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const LISTENER_SPIN_TIMEOUT: Duration = Duration::from_millis(500);

/// The client hello request identity; the daemon reply must echo exactly this.
const HELLO_REQUEST_ID: u64 = 1;

/// The upper bound for the liveness probe that decides whether an endpoint is
/// a stale socket or belongs to a live listener.
///
/// Unix-only: a named pipe on Windows leaves no stale filesystem entry, so the
/// endpoint path is never probed there.
#[cfg(unix)]
const STALE_PROBE_TIMEOUT: Duration = Duration::from_millis(50);

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

/// An asynchronous local listener with the existing endpoint ownership policy.
///
/// It accepts one client at a time into an opaque daemon-side connection. The
/// caller supplies the runtime; this transport type never creates one.
pub struct AsyncLocalListener {
    listener: TokioLocalSocketListener,
}

impl AsyncLocalListener {
    /// Binds a user-private local endpoint for asynchronous connections.
    ///
    /// On Unix, a bind conflict from an unclean previous daemon exit is
    /// recovered: a stale socket whose bounded probe connection is refused is
    /// removed and the bind is retried once. Dropping the listener never
    /// removes the endpoint path.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error when the parent cannot be prepared, another
    /// listener owns the endpoint, or the local IPC implementation is unavailable.
    pub fn bind(endpoint: LocalEndpoint) -> DtoResult<Self> {
        prepare_parent_directory(&endpoint)?;
        let listener = match listener_options(&endpoint)?.create_tokio() {
            Ok(listener) => listener,
            Err(_) if reclaim_stale_socket(&endpoint) => listener_options(&endpoint)?
                .create_tokio()
                .map_err(|_| endpoint_in_use())?,
            Err(_) => return Err(endpoint_in_use()),
        };
        Ok(Self { listener })
    }

    /// Accepts one client into an asynchronous daemon-side connection.
    ///
    /// # Errors
    ///
    /// Returns a safe unavailable error when the listener cannot accept the
    /// next peer connection.
    pub async fn accept(&self) -> DtoResult<AsyncLocalDaemonConnection> {
        let stream = self
            .listener
            .accept()
            .await
            .map_err(|_| unavailable("local_daemon_connection_unavailable"))?;
        Ok(AsyncLocalDaemonConnection { stream })
    }
}

/// An opaque asynchronous client connection before hello negotiation.
pub struct AsyncLocalClientConnection {
    stream: TokioLocalSocketStream,
}

impl AsyncLocalClientConnection {
    /// Connects to the existing local endpoint with the fixed bounded wait.
    ///
    /// # Errors
    ///
    /// Returns a safe unavailable error when no daemon endpoint can be reached.
    pub async fn connect(endpoint: &LocalEndpoint) -> DtoResult<Self> {
        let options = ConnectOptions::new()
            .name(endpoint.socket_name()?)
            .wait_mode(ConnectWaitMode::Timeout(CONNECT_TIMEOUT));
        let stream = timeout_async_connect(CONNECT_TIMEOUT, options.connect_tokio())
            .await
            .map_err(|_| unavailable("local_daemon_unavailable"))?;
        Ok(Self { stream })
    }

    /// Exchanges the client hello and consumes the connection into client roles.
    ///
    /// The returned roles retain only the appropriate typed protocol direction:
    /// requests flow to the daemon, and responses or notifications flow from it.
    ///
    /// # Errors
    ///
    /// Returns a typed incompatibility or safe framing/connection error when the
    /// hello exchange cannot complete.
    pub async fn negotiate(
        mut self,
        local: ProtocolHelloDto,
    ) -> DtoResult<(ProtocolHelloDto, AsyncRequestSender, AsyncMessageReceiver)> {
        let request = encode_hello_request(HELLO_REQUEST_ID, local);
        write_async_message(&mut self.stream, &request).await?;
        let mut residual = Vec::new();
        let line = read_async_line(&mut self.stream, &mut residual).await?;
        let response: JsonRpcResponseDto<ProtocolHelloDto> =
            JsonRpcResponseDto::parse(&line).map_err(|error| error.to_error())?;
        validate_hello_reply_id(&response)?;
        let remote = decode_hello_response(&response)?;
        let (receiver, sender) = self.stream.split();
        Ok((
            remote,
            AsyncRequestSender { sender },
            AsyncMessageReceiver { receiver, residual },
        ))
    }
}

/// An opaque asynchronous daemon connection before hello negotiation.
pub struct AsyncLocalDaemonConnection {
    stream: TokioLocalSocketStream,
}

impl AsyncLocalDaemonConnection {
    /// Exchanges the daemon hello and consumes the connection into daemon roles.
    ///
    /// The returned roles retain only the appropriate typed protocol direction:
    /// requests arrive from the client, and responses or notifications flow
    /// back to it on the same connection.
    ///
    /// # Errors
    ///
    /// Returns a typed incompatibility or safe framing/connection error when the
    /// hello exchange cannot complete. A malformed hello or a differing version
    /// is answered with a typed JSON-RPC error before the connection closes.
    pub async fn negotiate(
        mut self,
        local: ProtocolHelloDto,
    ) -> DtoResult<(ProtocolHelloDto, AsyncRequestReceiver, AsyncMessageSender)> {
        let mut residual = Vec::new();
        let line = read_async_line(&mut self.stream, &mut residual).await?;
        let request: JsonRpcRequestDto<ProtocolHelloDto> = match JsonRpcRequestDto::parse(&line) {
            Ok(request) => request,
            Err(failure) => {
                let (id, error) = failure.into_parts();
                let reply = JsonRpcResponseDto::<ProtocolHelloDto>::error(id, error.clone());
                let _ = write_async_message(&mut self.stream, &reply).await;
                return Err(error.to_error());
            }
        };
        let remote = match decode_hello_request(&request) {
            Ok(remote) => remote,
            Err(error) => {
                let reply = JsonRpcResponseDto::<ProtocolHelloDto>::error(
                    Some(request.id()),
                    error.clone(),
                );
                let _ = write_async_message(&mut self.stream, &reply).await;
                return Err(error.to_error());
            }
        };
        let response = encode_hello_response(request.id(), local);
        write_async_message(&mut self.stream, &response).await?;
        let (receiver, sender) = self.stream.split();
        Ok((
            remote,
            AsyncRequestReceiver { receiver, residual },
            AsyncMessageSender { sender },
        ))
    }
}

/// The client-to-daemon half of an established asynchronous connection.
pub struct AsyncRequestSender {
    sender: TokioSendHalf,
}

impl AsyncRequestSender {
    /// Sends one bounded JSON message.
    ///
    /// # Errors
    ///
    /// Returns a safe framing or connection error when the message cannot be sent.
    pub async fn send_message<T: serde::Serialize + Sync + ?Sized>(
        &mut self,
        value: &T,
    ) -> DtoResult<()> {
        write_async_message(&mut self.sender, value).await
    }
}

/// The daemon-to-client half that receives responses and notifications.
pub struct AsyncMessageReceiver {
    receiver: TokioRecvHalf,
    residual: Vec<u8>,
}

impl AsyncMessageReceiver {
    /// Receives one bounded NDJSON line carrying one JSON-RPC envelope.
    ///
    /// # Errors
    ///
    /// Returns a safe framing or connection error when the line cannot be read.
    pub async fn receive_line(&mut self) -> DtoResult<String> {
        read_async_line(&mut self.receiver, &mut self.residual).await
    }
}

/// The client-to-daemon half that receives established requests.
pub struct AsyncRequestReceiver {
    receiver: TokioRecvHalf,
    residual: Vec<u8>,
}

impl AsyncRequestReceiver {
    /// Receives one bounded NDJSON line carrying one JSON-RPC request.
    ///
    /// # Errors
    ///
    /// Returns a safe framing or connection error when the line cannot be read.
    pub async fn receive_line(&mut self) -> DtoResult<String> {
        read_async_line(&mut self.receiver, &mut self.residual).await
    }
}

/// The daemon-to-client half that sends responses and notifications.
pub struct AsyncMessageSender {
    sender: TokioSendHalf,
}

impl AsyncMessageSender {
    /// Sends one bounded JSON message.
    ///
    /// # Errors
    ///
    /// Returns a safe framing or connection error when the message cannot be sent.
    pub async fn send_message<T: serde::Serialize + Sync + ?Sized>(
        &mut self,
        value: &T,
    ) -> DtoResult<()> {
        write_async_message(&mut self.sender, value).await
    }
}

/// Returns the currently implemented local protocol version.
#[must_use]
pub const fn local_protocol_version() -> ProtocolVersionDto {
    intention_proto::CURRENT_PROTOCOL_VERSION
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

async fn write_async_message<T: serde::Serialize + Sync + ?Sized>(
    stream: &mut (impl AsyncWrite + Send + Unpin),
    value: &T,
) -> DtoResult<()> {
    let payload = encode_message(value)?;
    stream
        .write_all(&payload)
        .await
        .map_err(|_| unavailable("local_daemon_connection_unavailable"))?;
    stream
        .flush()
        .await
        .map_err(|_| unavailable("local_daemon_connection_unavailable"))
}

/// Reads the next complete NDJSON line with the asynchronous transport read.
///
/// The framing rules live in [`take_line`] and [`append_chunk`].
async fn read_async_line(
    stream: &mut (impl AsyncRead + Send + Unpin),
    residual: &mut Vec<u8>,
) -> DtoResult<String> {
    loop {
        if let Some(line) = take_line(residual) {
            return line;
        }
        let mut chunk = [0_u8; 8192];
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|_| unavailable("local_daemon_connection_unavailable"))?;
        if read == 0 {
            return Err(unavailable("local_daemon_connection_unavailable"));
        }
        append_chunk(residual, &chunk[..read])?;
    }
}

/// Extracts the next complete message line from the buffered residual bytes.
///
/// Returns `None` while no newline has arrived. A line is decoded after a
/// single trailing carriage return is removed, so CRLF framing carries the
/// same message as LF framing. A blank line is framing rather than a message
/// and is skipped, so it is never answered with a parse error.
fn take_line(residual: &mut Vec<u8>) -> Option<DtoResult<String>> {
    loop {
        let position = residual.iter().position(|byte| *byte == b'\n')?;
        let line: Vec<u8> = residual.drain(..position).collect();
        residual.drain(..1);
        if line.is_empty() || line == b"\r" {
            continue;
        }
        return Some(decode_line(line));
    }
}

/// Buffers one freshly read chunk and enforces the transport message cap.
fn append_chunk(residual: &mut Vec<u8>, chunk: &[u8]) -> DtoResult<()> {
    residual.extend_from_slice(chunk);
    if residual.len() > MAX_MESSAGE_BYTES {
        return Err(oversized_message());
    }
    Ok(())
}

/// Encodes one typed message into its bounded, newline-terminated NDJSON line.
fn encode_message<T: serde::Serialize + ?Sized>(value: &T) -> DtoResult<Vec<u8>> {
    let mut payload = serde_json::to_vec(value).map_err(|_| {
        ErrorDto::validation(
            "local_protocol_encode_failed",
            "a typed local protocol message could not be encoded",
        )
    })?;
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err(oversized_message());
    }
    payload.push(b'\n');
    Ok(payload)
}

fn decode_line(mut line: Vec<u8>) -> DtoResult<String> {
    if line.len() > MAX_MESSAGE_BYTES {
        return Err(oversized_message());
    }
    // One trailing carriage return is CRLF framing, not message content. A
    // `\r` byte is never a UTF-8 continuation byte, so trimming it here is
    // equivalent to trimming it after decoding.
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    String::from_utf8(line).map_err(|_| {
        ErrorDto::validation(
            "invalid_local_protocol_message",
            "a local protocol message was invalid",
        )
    })
}

fn oversized_message() -> ErrorDto {
    ErrorDto::validation(
        "local_protocol_message_too_large",
        "a local protocol message exceeded the configured limit",
    )
}

async fn timeout_async_connect<T>(
    timeout: Duration,
    connect: impl Future<Output = std::io::Result<T>>,
) -> std::io::Result<T> {
    tokio::time::timeout(timeout, connect)
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "local connect timed out"))?
}

fn unavailable(code: &'static str) -> ErrorDto {
    ErrorDto::unavailable(code, "the local daemon connection is unavailable")
}

/// Validates that one hello reply echoes the hello request identity.
///
/// This mirrors the correlation every ordinary reply is held to: a reply for
/// another identity is never accepted as the handshake answer.
fn validate_hello_reply_id(response: &JsonRpcResponseDto<ProtocolHelloDto>) -> DtoResult<()> {
    if response.id() == Some(HELLO_REQUEST_ID) {
        Ok(())
    } else {
        Err(ErrorDto::validation(
            "invalid_local_protocol_response",
            "the local daemon returned an uncorrelated hello reply",
        ))
    }
}

fn endpoint_in_use() -> ErrorDto {
    ErrorDto::new(
        "local_daemon_endpoint_in_use",
        ErrorCategoryDto::Conflict,
        "the local daemon endpoint is already in use",
        ErrorRetryDto::Immediate,
        None,
    )
    .unwrap_or_else(|_| unavailable("local_daemon_endpoint_in_use"))
}

/// The filesystem identity of one Unix socket file.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SocketIdentity {
    device: u64,
    inode: u64,
}

/// Reads the identity of one socket file, or `None` when the path is absent
/// or is not a socket.
#[cfg(unix)]
fn socket_identity(path: &std::path::Path) -> Option<SocketIdentity> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let metadata = fs::symlink_metadata(path).ok()?;
    metadata.file_type().is_socket().then(|| SocketIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

/// Reports whether a failed probe proves that nothing is listening.
///
/// Only a refused connection does. A timeout (for example a live listener
/// whose accept backlog is saturated) and every other failure are treated as
/// "in use", so the endpoint path is left in place.
#[cfg(unix)]
const fn probe_failure_reports_stale(kind: std::io::ErrorKind) -> bool {
    matches!(kind, std::io::ErrorKind::ConnectionRefused)
}

/// Probes the endpoint with a bounded connect and reports whether the
/// connection was refused.
#[cfg(unix)]
fn probe_reports_stale_socket(endpoint: &LocalEndpoint) -> bool {
    let Ok(name) = endpoint.socket_name() else {
        return false;
    };
    ConnectOptions::new()
        .name(name)
        .wait_mode(ConnectWaitMode::Timeout(STALE_PROBE_TIMEOUT))
        .connect_sync()
        .err()
        .is_some_and(|error| probe_failure_reports_stale(error.kind()))
}

/// Removes one socket file only when it still resolves to the identity that
/// was captured before the probe.
#[cfg(unix)]
fn remove_socket_if_identity_unchanged(path: &std::path::Path, identity: SocketIdentity) -> bool {
    socket_identity(path) == Some(identity) && fs::remove_file(path).is_ok()
}

/// Reclaims a stale Unix endpoint left behind by an unclean daemon exit.
///
/// A bind failure means the endpoint is in use unless the path is a socket
/// whose bounded probe connection is refused. A probe that fails for any
/// other reason is treated as "in use", never as stale. The socket is removed
/// only when it still has the identity captured before the probe, so a path
/// replaced while the probe ran is never removed. Reclaim at the next bind is
/// the only removal path: no listener ever unlinks its endpoint on drop.
#[cfg(unix)]
fn reclaim_stale_socket(endpoint: &LocalEndpoint) -> bool {
    let Some(identity) = socket_identity(&endpoint.path) else {
        return false;
    };
    if !probe_reports_stale_socket(endpoint) {
        return false;
    }
    remove_socket_if_identity_unchanged(&endpoint.path, identity)
}

/// On platforms without Unix sockets (notably Windows named pipes), a bind
/// conflict always means the endpoint is in use: there is no stale filesystem
/// artifact to reclaim, so the reclaim retry never applies.
#[cfg(not(unix))]
const fn reclaim_stale_socket(_endpoint: &LocalEndpoint) -> bool {
    false
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
    #[tokio::test]
    async fn async_listener_reclaims_a_stale_socket_and_keeps_live_endpoints() {
        let stale = endpoint();
        let stale_path = stale.path.clone();
        let abandoned =
            std::os::unix::net::UnixListener::bind(&stale_path).expect("stale socket seeds");
        drop(abandoned);
        let listener = AsyncLocalListener::bind(stale).expect("stale socket is reclaimed");
        drop(listener);

        let live = endpoint();
        let live_path = live.path.clone();
        let _listener = AsyncLocalListener::bind(live.clone()).expect("first listener binds");
        let error = match AsyncLocalListener::bind(live) {
            Err(error) => error,
            Ok(_) => panic!("second async bind conflicts"),
        };
        assert_eq!(error.code(), "local_daemon_endpoint_in_use");
        assert!(live_path.exists());

        // A non-socket path at the endpoint must never be removed.
        let regular = endpoint();
        let regular_path = regular.path.clone();
        fs::write(&regular_path, b"not a socket").expect("regular file seeds");
        let error = match AsyncLocalListener::bind(regular) {
            Err(error) => error,
            Ok(_) => panic!("non-socket path conflicts"),
        };
        assert_eq!(error.code(), "local_daemon_endpoint_in_use");
        assert_eq!(
            fs::read(&regular_path).expect("regular file remains"),
            b"not a socket"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn async_listener_enforces_private_permissions_and_leaves_its_socket_for_reclaim() {
        use std::os::unix::fs::PermissionsExt;

        let endpoint = endpoint();
        let socket_path = endpoint.path.clone();
        let parent = socket_path
            .parent()
            .expect("socket has a parent")
            .to_owned();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
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
            socket_path.exists(),
            "dropping an async listener leaves its endpoint for reclaim"
        );
        let reclaimed = AsyncLocalListener::bind(endpoint).expect("the left endpoint is reclaimed");
        drop(reclaimed);
    }

    #[cfg(unix)]
    #[test]
    fn only_a_refused_probe_reports_a_stale_endpoint() {
        assert!(probe_failure_reports_stale(
            std::io::ErrorKind::ConnectionRefused
        ));
        for kind in [
            // A saturated accept backlog on a live listener fails the probe
            // with a timeout instead of a refusal.
            std::io::ErrorKind::TimedOut,
            std::io::ErrorKind::WouldBlock,
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::Other,
        ] {
            assert!(
                !probe_failure_reports_stale(kind),
                "a {kind:?} probe failure must never reclaim the endpoint"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn probe_reports_only_a_refused_endpoint_as_stale() {
        let live = endpoint();
        let live_path = live.path.clone();
        let listener =
            std::os::unix::net::UnixListener::bind(&live_path).expect("live socket seeds");
        assert!(
            !probe_reports_stale_socket(&live),
            "a live listener is never stale"
        );
        drop(listener);
        assert!(
            probe_reports_stale_socket(&live),
            "an abandoned socket file is stale"
        );
    }

    #[cfg(unix)]
    #[test]
    fn reclaim_refuses_a_socket_identity_that_changed_after_the_probe() {
        let first = endpoint();
        let first_path = first.path;
        let abandoned =
            std::os::unix::net::UnixListener::bind(&first_path).expect("first socket seeds");
        let inspected = socket_identity(&first_path).expect("the inspected identity reads");
        drop(abandoned);

        // Another owner replaces the path while the reclaim holds the
        // inspected identity, so removal must refuse the new socket.
        let replacement = endpoint();
        let replacement_path = replacement.path;
        let replacement_listener = std::os::unix::net::UnixListener::bind(&replacement_path)
            .expect("replacement socket seeds");
        fs::rename(&replacement_path, &first_path).expect("the replacement replaces the path");
        let replacement_identity =
            socket_identity(&first_path).expect("the replacement identity reads");
        assert_ne!(
            inspected, replacement_identity,
            "the fixture sockets have different identities"
        );

        assert!(
            !remove_socket_if_identity_unchanged(&first_path, inspected),
            "a socket whose identity changed is never removed"
        );
        assert!(
            first_path.exists(),
            "the replacement socket survives the identity check"
        );
        assert!(
            remove_socket_if_identity_unchanged(&first_path, replacement_identity),
            "the unchanged identity is removable"
        );
        assert!(
            !first_path.exists(),
            "the identity check removes only the inspected socket"
        );
        drop(replacement_listener);
    }

    #[tokio::test]
    async fn async_connect_to_an_absent_endpoint_is_a_typed_error() {
        let error = match AsyncLocalClientConnection::connect(&endpoint()).await {
            Ok(_) => panic!("absent endpoint must not connect"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "local_daemon_unavailable");
    }

    #[tokio::test(start_paused = true)]
    async fn async_connect_timeout_bounds_a_pending_connection() {
        let connect = timeout_async_connect(
            CONNECT_TIMEOUT,
            std::future::pending::<std::io::Result<()>>(),
        );
        tokio::pin!(connect);
        tokio::select! {
            result = &mut connect => panic!("pending connection completed: {result:?}"),
            () = tokio::task::yield_now() => {}
        }
        tokio::time::advance(CONNECT_TIMEOUT).await;
        assert_eq!(
            connect
                .await
                .expect_err("pending connection times out")
                .kind(),
            std::io::ErrorKind::TimedOut
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

    #[tokio::test]
    async fn daemon_negotiation_rejects_oversized_malformed_and_truncated_lines() {
        for (line, expected_code) in [
            (
                vec![b'x'; MAX_MESSAGE_BYTES + 1],
                "local_protocol_message_too_large",
            ),
            (b"not json\n".to_vec(), "jsonrpc_parse_error"),
            (b"{}\n".to_vec(), "jsonrpc_invalid_request"),
        ] {
            let endpoint = endpoint();
            let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
            let server = tokio::spawn(async move {
                let connection = listener.accept().await.expect("server accepts");
                match connection
                    .negotiate(
                        ProtocolHelloDto::new(local_protocol_version(), "async-server")
                            .expect("fixture hello is valid"),
                    )
                    .await
                {
                    Ok(_) => panic!("an invalid hello line is rejected"),
                    Err(error) => error,
                }
            });
            let mut client = ConnectOptions::new()
                .name(
                    endpoint
                        .socket_name()
                        .expect("fixture socket name is valid"),
                )
                .wait_mode(ConnectWaitMode::Timeout(CONNECT_TIMEOUT))
                .connect_tokio()
                .await
                .expect("raw client connects");
            client
                .write_all(&line)
                .await
                .expect("raw fixture line writes");
            drop(client);
            let error = server.await.expect("server completes");
            assert_eq!(error.code(), expected_code);
        }

        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = tokio::spawn(async move {
            let connection = listener.accept().await.expect("server accepts");
            match connection
                .negotiate(
                    ProtocolHelloDto::new(local_protocol_version(), "async-server")
                        .expect("fixture hello is valid"),
                )
                .await
            {
                Ok(_) => panic!("a truncated hello line is rejected"),
                Err(error) => error,
            }
        });
        let mut client = ConnectOptions::new()
            .name(
                endpoint
                    .socket_name()
                    .expect("fixture socket name is valid"),
            )
            .wait_mode(ConnectWaitMode::Timeout(CONNECT_TIMEOUT))
            .connect_tokio()
            .await
            .expect("raw client connects");
        client
            .write_all(b"{\"jsonrpc\":\"2.0\"")
            .await
            .expect("raw partial line writes");
        drop(client);
        let error = server.await.expect("server completes");
        assert_eq!(error.code(), "local_daemon_connection_unavailable");
    }

    #[tokio::test]
    async fn daemon_hello_answers_a_version_mismatch_with_a_typed_error() {
        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = tokio::spawn(async move {
            let connection = listener.accept().await.expect("server accepts");
            match connection
                .negotiate(
                    ProtocolHelloDto::new(local_protocol_version(), "async-server")
                        .expect("fixture hello is valid"),
                )
                .await
            {
                Ok(_) => panic!("a future-version peer is rejected"),
                Err(error) => error,
            }
        });
        let mut client = ConnectOptions::new()
            .name(
                endpoint
                    .socket_name()
                    .expect("fixture socket name is valid"),
            )
            .wait_mode(ConnectWaitMode::Timeout(CONNECT_TIMEOUT))
            .connect_tokio()
            .await
            .expect("raw client connects");
        client
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"hello\",\"params\":{\"version\":{\"major\":1,\"minor\":1},\"adapter_name\":\"future\"}}\n",
            )
            .await
            .expect("raw future hello writes");
        let mut reply = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            client.read_exact(&mut byte).await.expect("reply reads");
            if byte[0] == b'\n' {
                break;
            }
            reply.push(byte[0]);
        }
        let line = String::from_utf8(reply).expect("reply is UTF-8");
        let response: JsonRpcResponseDto<ProtocolHelloDto> =
            JsonRpcResponseDto::parse(&line).expect("error response parses");
        let error = response.error_value().expect("an error object is present");
        assert_eq!(error.code(), intention_proto::JSONRPC_VERSION_MISMATCH);
        assert_eq!(
            error.data().map(intention_proto::ErrorDto::code),
            Some("incompatible_protocol_version")
        );
        assert_eq!(
            server.await.expect("server completes").code(),
            "incompatible_protocol_version"
        );
    }

    #[tokio::test]
    async fn daemon_hello_replies_to_malformed_lines_with_standard_error_codes() {
        for (line, expected_code, expected_data_code) in [
            (
                b"{\n".to_vec(),
                intention_proto::JSONRPC_PARSE_ERROR,
                "jsonrpc_parse_error",
            ),
            (
                b"{}\n".to_vec(),
                intention_proto::JSONRPC_INVALID_REQUEST,
                "jsonrpc_invalid_request",
            ),
        ] {
            let endpoint = endpoint();
            let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
            let server = tokio::spawn(async move {
                let connection = listener.accept().await.expect("server accepts");
                match connection
                    .negotiate(
                        ProtocolHelloDto::new(local_protocol_version(), "async-server")
                            .expect("fixture hello is valid"),
                    )
                    .await
                {
                    Ok(_) => panic!("a malformed first line never negotiates"),
                    Err(error) => error,
                }
            });
            let mut client = ConnectOptions::new()
                .name(
                    endpoint
                        .socket_name()
                        .expect("fixture socket name is valid"),
                )
                .wait_mode(ConnectWaitMode::Timeout(CONNECT_TIMEOUT))
                .connect_tokio()
                .await
                .expect("raw client connects");
            client
                .write_all(&line)
                .await
                .expect("raw malformed line writes");
            let mut reply = Vec::new();
            let mut byte = [0_u8; 1];
            loop {
                client.read_exact(&mut byte).await.expect("reply reads");
                if byte[0] == b'\n' {
                    break;
                }
                reply.push(byte[0]);
            }
            let reply = String::from_utf8(reply).expect("reply is UTF-8");
            let response: JsonRpcResponseDto<ProtocolHelloDto> =
                JsonRpcResponseDto::parse(&reply).expect("the error reply parses");
            assert!(response.result_value().is_none());
            assert_eq!(response.id(), None, "an unparseable line has no identity");
            let error = response.error_value().expect("an error object is present");
            assert_eq!(error.code(), expected_code);
            assert_eq!(
                error.data().map(intention_proto::ErrorDto::code),
                Some(expected_data_code)
            );
            assert_eq!(
                server.await.expect("server completes").code(),
                expected_data_code
            );
        }
    }

    #[tokio::test]
    async fn daemon_hello_answers_a_non_hello_first_request_with_the_invalid_request_code() {
        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = tokio::spawn(async move {
            let connection = listener.accept().await.expect("server accepts");
            match connection
                .negotiate(
                    ProtocolHelloDto::new(local_protocol_version(), "async-server")
                        .expect("fixture hello is valid"),
                )
                .await
            {
                Ok(_) => panic!("a first request that is not hello never negotiates"),
                Err(error) => error,
            }
        });
        let mut client = ConnectOptions::new()
            .name(
                endpoint
                    .socket_name()
                    .expect("fixture socket name is valid"),
            )
            .wait_mode(ConnectWaitMode::Timeout(CONNECT_TIMEOUT))
            .connect_tokio()
            .await
            .expect("raw client connects");
        let request = JsonRpcRequestDto::new(
            1,
            intention_proto::ProtocolMethodDto::DaemonHealth.as_str(),
            ProtocolHelloDto::new(local_protocol_version(), "fixture-client")
                .expect("fixture hello is valid"),
        );
        let mut payload = serde_json::to_vec(&request).expect("non-hello request serializes");
        payload.push(b'\n');
        client
            .write_all(&payload)
            .await
            .expect("raw non-hello request writes");
        let mut reply = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            client.read_exact(&mut byte).await.expect("reply reads");
            if byte[0] == b'\n' {
                break;
            }
            reply.push(byte[0]);
        }
        let line = String::from_utf8(reply).expect("reply is UTF-8");
        let response: JsonRpcResponseDto<ProtocolHelloDto> =
            JsonRpcResponseDto::parse(&line).expect("the error response parses");
        assert!(response.result_value().is_none());
        assert_eq!(response.id(), Some(1), "the recovered identity is echoed");
        let error = response.error_value().expect("an error object is present");
        assert_eq!(error.code(), intention_proto::JSONRPC_INVALID_REQUEST);
        assert_eq!(
            error.data().map(intention_proto::ErrorDto::code),
            Some("jsonrpc_hello_required")
        );
        assert_eq!(
            server.await.expect("server completes").code(),
            "jsonrpc_hello_required"
        );
    }

    #[tokio::test]
    async fn async_client_negotiation_rejects_an_uncorrelated_hello_reply() {
        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = tokio::spawn(async move {
            let mut connection = listener.accept().await.expect("server accepts");
            let line = read_async_line(&mut connection.stream, &mut Vec::new())
                .await
                .expect("scripted server receives hello");
            let request: JsonRpcRequestDto<ProtocolHelloDto> =
                JsonRpcRequestDto::parse(&line).expect("scripted hello parses");
            let reply = encode_hello_response(
                request.id() + 1,
                ProtocolHelloDto::new(local_protocol_version(), "uncorrelated-async-daemon")
                    .expect("fixture hello is valid"),
            );
            write_async_message(&mut connection.stream, &reply)
                .await
                .expect("uncorrelated async hello reply sends");
        });
        let connection = AsyncLocalClientConnection::connect(&endpoint)
            .await
            .expect("client connects");
        let error = match connection
            .negotiate(
                ProtocolHelloDto::new(local_protocol_version(), "async-fixture-client")
                    .expect("fixture hello is valid"),
            )
            .await
        {
            Ok(_) => panic!("an uncorrelated hello reply is rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "invalid_local_protocol_response");
        server.await.expect("scripted server completes");
    }

    #[tokio::test]
    async fn async_negotiation_accepts_crlf_framing_after_blank_lines() {
        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = tokio::spawn(async move {
            let connection = listener.accept().await.expect("server accepts");
            connection
                .negotiate(
                    ProtocolHelloDto::new(local_protocol_version(), "crlf-async-server")
                        .expect("fixture hello is valid"),
                )
                .await
                .expect("a CRLF hello after blank lines negotiates")
        });
        let mut client = ConnectOptions::new()
            .name(
                endpoint
                    .socket_name()
                    .expect("fixture socket name is valid"),
            )
            .wait_mode(ConnectWaitMode::Timeout(CONNECT_TIMEOUT))
            .connect_tokio()
            .await
            .expect("raw client connects");
        let hello = encode_hello_request(
            HELLO_REQUEST_ID,
            ProtocolHelloDto::new(local_protocol_version(), "crlf-async-client")
                .expect("fixture hello is valid"),
        );
        let mut framed = b"\n\r\n".to_vec();
        framed.extend_from_slice(&serde_json::to_vec(&hello).expect("hello serializes"));
        framed.extend_from_slice(b"\r\n");
        client
            .write_all(&framed)
            .await
            .expect("raw CRLF hello writes");
        let (remote, _requests, _messages) = server.await.expect("server completes");
        assert_eq!(remote.adapter_name(), "crlf-async-client");
    }

    #[tokio::test]
    async fn async_negotiation_rejects_an_oversized_outbound_hello_before_framing() {
        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = tokio::spawn(async move {
            let connection = listener.accept().await.expect("server accepts");
            match connection
                .negotiate(
                    ProtocolHelloDto::new(local_protocol_version(), "async-server")
                        .expect("fixture hello is valid"),
                )
                .await
            {
                Ok(_) => panic!("closed peer is typed"),
                Err(error) => error,
            }
        });
        let connection = AsyncLocalClientConnection::connect(&endpoint)
            .await
            .expect("client connects");
        let oversized =
            ProtocolHelloDto::new(local_protocol_version(), "x".repeat(MAX_MESSAGE_BYTES + 1))
                .expect("non-empty fixture hello is valid");
        let error = match connection.negotiate(oversized).await {
            Ok(_) => panic!("oversized outbound hello is rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "local_protocol_message_too_large");
        assert_eq!(
            server.await.expect("server completes").code(),
            "local_daemon_connection_unavailable"
        );
    }
}
