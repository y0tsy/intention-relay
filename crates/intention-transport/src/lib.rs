//! Framed, local-only IPC for Intention Relay.
//!
//! The public surface accepts only `intention-protocol` DTOs. A bounded,
//! length-prefixed JSON codec remains private to this crate, and the underlying
//! Unix-domain socket or Windows named pipe never crosses its crate boundary.

#[cfg(unix)]
use std::fs;
use std::future::Future;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use intention_protocol::{
    ProtocolDaemonFrameDto, ProtocolHelloDto, ProtocolRequestEnvelopeDto,
    ProtocolResponseEnvelopeDto, ProtocolVersionDto, RunSubscriptionRequestEnvelopeDto,
};
use intention_types::{DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto};
use interprocess::ConnectWaitMode;
use interprocess::local_socket::prelude::{LocalSocketListener, LocalSocketStream};
use interprocess::local_socket::tokio::{
    Listener as TokioLocalSocketListener, RecvHalf as TokioRecvHalf, SendHalf as TokioSendHalf,
    Stream as TokioLocalSocketStream,
};
use interprocess::local_socket::traits::Listener as _;
#[cfg(unix)]
use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::traits::tokio::{Listener as _, Stream as _};
use interprocess::local_socket::{ConnectOptions, GenericFilePath, ListenerOptions, PathNameType};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const FRAME_LENGTH_BYTES: usize = 4;
const MAX_FRAME_BYTES: usize = 1_048_576;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const LISTENER_SPIN_TIMEOUT: Duration = Duration::from_millis(500);

/// The upper bound for one synchronous framed read or write.
///
/// A local request/response completes in milliseconds; the bound exists so a
/// peer that accepts a connection and then stops answering fails with a typed
/// unavailable error instead of blocking the caller until a CI step timeout.
const SYNC_IO_TIMEOUT: Duration = Duration::from_secs(10);

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

/// A local framed connection which exchanges only typed protocol DTOs.
pub struct LocalConnection {
    stream: LocalSocketStream,
}

impl LocalConnection {
    /// Connects to the endpoint with a bounded wait.
    ///
    /// The returned connection bounds every synchronous read and write with
    /// the transport I/O timeout, so a peer that accepts the connection and
    /// never answers fails with a typed unavailable error instead of blocking
    /// the caller indefinitely.
    ///
    /// # Errors
    ///
    /// Returns a safe typed unavailable error when the daemon endpoint cannot be
    /// reached or the connection cannot be bounded, rather than exposing an OS
    /// path or error string.
    pub fn connect(endpoint: &LocalEndpoint) -> DtoResult<Self> {
        Self::connect_with_io_timeout(endpoint, SYNC_IO_TIMEOUT)
    }

    /// Connects to the endpoint and applies the requested synchronous I/O bound.
    fn connect_with_io_timeout(endpoint: &LocalEndpoint, io_timeout: Duration) -> DtoResult<Self> {
        let stream = ConnectOptions::new()
            .name(endpoint.socket_name()?)
            .wait_mode(ConnectWaitMode::Timeout(CONNECT_TIMEOUT))
            .connect_sync()
            .map_err(|_| unavailable("local_daemon_unavailable"))?;
        apply_sync_io_timeout(&stream, io_timeout)?;
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
}

impl LocalListener {
    /// Binds a user-private local endpoint.
    ///
    /// On Unix, a bind conflict from an unclean previous daemon exit is
    /// recovered: when the endpoint path is a socket whose bounded probe
    /// connection is refused, the socket is removed and the bind is retried
    /// once. A live listener (including one whose accept backlog saturates the
    /// probe), a replaced socket, and a non-socket path are never removed.
    ///
    /// Dropping a listener never removes the endpoint path; reclaim at the
    /// next bind is the only removal path, so a clean and an unclean exit look
    /// the same on disk.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error when the parent directory cannot be prepared,
    /// the endpoint is already serving another daemon, or platform IPC is
    /// unavailable. It never removes a path that is not a stale socket.
    pub fn bind(endpoint: LocalEndpoint) -> DtoResult<Self> {
        prepare_parent_directory(&endpoint)?;
        let listener = match listener_options(&endpoint)?.create_sync() {
            Ok(listener) => listener,
            Err(_) if reclaim_stale_socket(&endpoint) => listener_options(&endpoint)?
                .create_sync()
                .map_err(|_| endpoint_in_use())?,
            Err(_) => return Err(endpoint_in_use()),
        };
        Ok(Self { listener })
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
        apply_sync_io_timeout(&stream, SYNC_IO_TIMEOUT)?;
        Ok(LocalConnection { stream })
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
    /// recovered exactly like [`LocalListener::bind`]: a stale socket whose
    /// bounded probe connection is refused is removed and the bind is retried
    /// once. Dropping the listener never removes the endpoint path.
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
    /// requests flow to the daemon and responses flow from it.
    ///
    /// # Errors
    ///
    /// Returns a typed incompatibility or safe framing/connection error when the
    /// hello exchange cannot complete.
    pub async fn negotiate(
        mut self,
        local: ProtocolHelloDto,
    ) -> DtoResult<(ProtocolHelloDto, AsyncRequestSender, AsyncResponseReceiver)> {
        write_async_frame(&mut self.stream, &local).await?;
        let remote: ProtocolHelloDto = read_async_frame(&mut self.stream).await?;
        require_exact_protocol_version(local.version(), remote.version())?;
        let (receiver, sender) = self.stream.split();
        Ok((
            remote,
            AsyncRequestSender { sender },
            AsyncResponseReceiver { receiver },
        ))
    }

    /// Exchanges hello and consumes the connection into a daemon-frame receiver.
    ///
    /// # Errors
    ///
    /// Returns a typed incompatibility or safe framing/connection error when the
    /// hello exchange cannot complete.
    pub async fn negotiate_daemon_frames(
        mut self,
        local: ProtocolHelloDto,
    ) -> DtoResult<(
        ProtocolHelloDto,
        AsyncRequestSender,
        AsyncDaemonFrameReceiver,
    )> {
        write_async_frame(&mut self.stream, &local).await?;
        let remote: ProtocolHelloDto = read_async_frame(&mut self.stream).await?;
        require_exact_protocol_version(local.version(), remote.version())?;
        let (receiver, sender) = self.stream.split();
        Ok((
            remote,
            AsyncRequestSender { sender },
            AsyncDaemonFrameReceiver { receiver },
        ))
    }
}

/// An opaque asynchronous daemon connection before hello negotiation.
pub struct AsyncLocalDaemonConnection {
    stream: TokioLocalSocketStream,
}

impl AsyncLocalDaemonConnection {
    /// Negotiates one connection and selects its typed response role from the
    /// peer's declared run-stream capability.
    ///
    /// This keeps ordinary M3 peers on their established response framing while
    /// allowing opt-in run-stream peers to receive daemon frames on the same
    /// endpoint.
    ///
    /// # Errors
    ///
    /// Returns a typed incompatibility or framing error when hello negotiation
    /// cannot complete.
    pub async fn negotiate_by_capability(
        mut self,
        local: ProtocolHelloDto,
    ) -> DtoResult<(ProtocolHelloDto, AsyncDaemonConnectionRoles)> {
        let remote: ProtocolHelloDto = read_async_frame(&mut self.stream).await?;
        require_exact_protocol_version(local.version(), remote.version())?;
        write_async_frame(&mut self.stream, &local).await?;
        let (receiver, sender) = self.stream.split();
        let roles = if remote
            .capabilities()
            .contains(&intention_protocol::ProtocolCapabilityDto::RunStreamSubscriptions)
        {
            AsyncDaemonConnectionRoles::RunStream(
                AsyncRequestReceiver { receiver },
                AsyncDaemonFrameSender { sender },
            )
        } else {
            AsyncDaemonConnectionRoles::Ordinary(
                AsyncRequestReceiver { receiver },
                AsyncResponseSender { sender },
            )
        };
        Ok((remote, roles))
    }

    /// Exchanges the daemon hello and consumes the connection into daemon roles.
    ///
    /// The returned roles retain only the appropriate typed protocol direction:
    /// requests arrive from the client and responses flow back to it.
    ///
    /// # Errors
    ///
    /// Returns a typed incompatibility or safe framing/connection error when the
    /// hello exchange cannot complete.
    pub async fn negotiate(
        mut self,
        local: ProtocolHelloDto,
    ) -> DtoResult<(ProtocolHelloDto, AsyncRequestReceiver, AsyncResponseSender)> {
        let remote: ProtocolHelloDto = read_async_frame(&mut self.stream).await?;
        require_exact_protocol_version(local.version(), remote.version())?;
        write_async_frame(&mut self.stream, &local).await?;
        let (receiver, sender) = self.stream.split();
        Ok((
            remote,
            AsyncRequestReceiver { receiver },
            AsyncResponseSender { sender },
        ))
    }

    /// Exchanges hello and consumes the connection into a daemon-frame sender.
    ///
    /// # Errors
    ///
    /// Returns a typed incompatibility or safe framing/connection error when the
    /// hello exchange cannot complete.
    pub async fn negotiate_daemon_frames(
        mut self,
        local: ProtocolHelloDto,
    ) -> DtoResult<(
        ProtocolHelloDto,
        AsyncRequestReceiver,
        AsyncDaemonFrameSender,
    )> {
        let remote: ProtocolHelloDto = read_async_frame(&mut self.stream).await?;
        require_exact_protocol_version(local.version(), remote.version())?;
        write_async_frame(&mut self.stream, &local).await?;
        let (receiver, sender) = self.stream.split();
        Ok((
            remote,
            AsyncRequestReceiver { receiver },
            AsyncDaemonFrameSender { sender },
        ))
    }
}

/// Opaque daemon roles selected after the peer's hello capabilities are known.
pub enum AsyncDaemonConnectionRoles {
    /// The retained correlated M3 response roles.
    Ordinary(AsyncRequestReceiver, AsyncResponseSender),
    /// The opt-in correlated-response plus uncorrelated-stream roles.
    RunStream(AsyncRequestReceiver, AsyncDaemonFrameSender),
}

/// The client-to-daemon half of an established asynchronous connection.
pub struct AsyncRequestSender {
    sender: TokioSendHalf,
}

impl AsyncRequestSender {
    /// Sends one bounded correlated protocol request.
    ///
    /// # Errors
    ///
    /// Returns a safe framing or connection error when the request cannot be sent.
    pub async fn send(&mut self, request: &ProtocolRequestEnvelopeDto) -> DtoResult<()> {
        write_async_frame(&mut self.sender, request).await
    }

    /// Sends a correlated run subscription request on this established connection.
    ///
    /// # Errors
    ///
    /// Returns a safe framing or connection error when the request cannot be sent.
    pub async fn send_run_subscription(
        &mut self,
        request: &RunSubscriptionRequestEnvelopeDto,
    ) -> DtoResult<()> {
        write_async_frame(&mut self.sender, request).await
    }
}

/// The daemon-to-client half of an established asynchronous connection.
pub struct AsyncResponseReceiver {
    receiver: TokioRecvHalf,
}

impl AsyncResponseReceiver {
    /// Receives one bounded correlated protocol response.
    ///
    /// # Errors
    ///
    /// Returns a safe framing or connection error when the response cannot be read.
    pub async fn receive(&mut self) -> DtoResult<ProtocolResponseEnvelopeDto> {
        read_async_frame(&mut self.receiver).await
    }
}

/// The client-to-daemon half that receives established requests.
pub struct AsyncRequestReceiver {
    receiver: TokioRecvHalf,
}

impl AsyncRequestReceiver {
    /// Receives one bounded correlated protocol request.
    ///
    /// # Errors
    ///
    /// Returns a safe framing or connection error when the request cannot be read.
    pub async fn receive(&mut self) -> DtoResult<ProtocolRequestEnvelopeDto> {
        read_async_frame(&mut self.receiver).await
    }

    /// Receives a correlated run subscription request on this established connection.
    ///
    /// # Errors
    ///
    /// Returns a safe framing or connection error when the request cannot be read.
    pub async fn receive_run_subscription(
        &mut self,
    ) -> DtoResult<RunSubscriptionRequestEnvelopeDto> {
        read_async_frame(&mut self.receiver).await
    }
}

/// The daemon-to-client half that sends established responses.
pub struct AsyncResponseSender {
    sender: TokioSendHalf,
}

impl AsyncResponseSender {
    /// Sends one bounded correlated protocol response.
    ///
    /// # Errors
    ///
    /// Returns a safe framing or connection error when the response cannot be sent.
    pub async fn send(&mut self, response: &ProtocolResponseEnvelopeDto) -> DtoResult<()> {
        write_async_frame(&mut self.sender, response).await
    }
}

/// The client-side receive role for correlated responses and uncorrelated stream frames.
pub struct AsyncDaemonFrameReceiver {
    receiver: TokioRecvHalf,
}

impl AsyncDaemonFrameReceiver {
    /// Receives one bounded daemon-originated frame.
    ///
    /// # Errors
    ///
    /// Returns a safe framing or connection error when the frame cannot be read.
    pub async fn receive(&mut self) -> DtoResult<ProtocolDaemonFrameDto> {
        read_async_frame(&mut self.receiver).await
    }
}

/// The daemon-side send role for correlated responses and uncorrelated stream frames.
pub struct AsyncDaemonFrameSender {
    sender: TokioSendHalf,
}

impl AsyncDaemonFrameSender {
    /// Sends one bounded daemon-originated frame.
    ///
    /// # Errors
    ///
    /// Returns a safe framing or connection error when the frame cannot be sent.
    pub async fn send(&mut self, frame: &ProtocolDaemonFrameDto) -> DtoResult<()> {
        write_async_frame(&mut self.sender, frame).await
    }
}

/// Performs the mandatory client/daemon hello exchange.
///
/// # Errors
///
/// Returns the typed protocol mismatch error when the peer protocol version
/// differs from the current version, or a typed transport error when the
/// handshake cannot complete.
pub fn negotiate_client(
    connection: &mut LocalConnection,
    local: ProtocolHelloDto,
) -> DtoResult<ProtocolHelloDto> {
    connection.send_hello(&local)?;
    let remote = connection.receive_hello()?;
    require_exact_protocol_version(local.version(), remote.version())?;
    Ok(remote)
}

/// Performs the daemon side of the mandatory hello exchange.
///
/// # Errors
///
/// Returns the typed protocol mismatch error when the peer protocol version
/// differs from the current version, or a typed transport error when the
/// handshake cannot complete.
pub fn negotiate_daemon(
    connection: &mut LocalConnection,
    local: ProtocolHelloDto,
) -> DtoResult<ProtocolHelloDto> {
    let remote = connection.receive_hello()?;
    require_exact_protocol_version(local.version(), remote.version())?;
    connection.send_hello(&local)?;
    Ok(remote)
}

/// Returns the currently implemented local protocol version.
#[must_use]
pub const fn local_protocol_version() -> ProtocolVersionDto {
    intention_protocol::CURRENT_PROTOCOL_VERSION
}

/// Requires the peer hello to carry the exact current protocol version.
///
/// # Errors
///
/// Returns an unavailable error when either peer version differs from
/// [`intention_protocol::CURRENT_PROTOCOL_VERSION`].
fn require_exact_protocol_version(
    local: ProtocolVersionDto,
    remote: ProtocolVersionDto,
) -> DtoResult<()> {
    if local != remote
        || local != intention_protocol::CURRENT_PROTOCOL_VERSION
        || remote != intention_protocol::CURRENT_PROTOCOL_VERSION
    {
        return Err(ErrorDto::unavailable(
            "incompatible_protocol_version",
            "protocol version must equal the current version",
        ));
    }
    Ok(())
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

async fn timeout_async_connect<T>(
    timeout: Duration,
    connect: impl Future<Output = std::io::Result<T>>,
) -> std::io::Result<T> {
    tokio::time::timeout(timeout, connect)
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "local connect timed out"))?
}

async fn write_async_frame<T: serde::Serialize + Sync>(
    stream: &mut (impl AsyncWrite + Send + Unpin),
    value: &T,
) -> DtoResult<()> {
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
        .await
        .map_err(|_| unavailable("local_daemon_connection_unavailable"))?;
    stream
        .write_all(&payload)
        .await
        .map_err(|_| unavailable("local_daemon_connection_unavailable"))?;
    stream
        .flush()
        .await
        .map_err(|_| unavailable("local_daemon_connection_unavailable"))
}

async fn read_async_frame<T: serde::de::DeserializeOwned>(
    stream: &mut (impl AsyncRead + Send + Unpin),
) -> DtoResult<T> {
    let mut header = [0_u8; FRAME_LENGTH_BYTES];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|_| unavailable("local_daemon_connection_unavailable"))?;
    let length = usize::try_from(u32::from_be_bytes(header)).map_err(|_| oversized_frame())?;
    if length > MAX_FRAME_BYTES {
        return Err(oversized_frame());
    }
    let mut payload = vec![0_u8; length];
    stream
        .read_exact(&mut payload)
        .await
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

/// Applies the bounded synchronous I/O timeout to one connected stream.
///
/// Unix-domain sockets carry per-call read and write timeouts. The non-Unix
/// stub below is a recorded limitation rather than a second bound: the locked
/// `interprocess` 2.4.4 named-pipe stream returns an unsupported error from
/// `set_recv_timeout` and `set_send_timeout`, so on those targets only the
/// bounded connect wait (`CONNECT_TIMEOUT`) applies and a peer that stops
/// answering mid-frame cannot be interrupted per call. The limitation is
/// anchored at
/// `docs/intention-relay/architecture/03-daemon-transport-and-adapters.md`
/// ("M2 serving and backpressure") and at repair R28 in `pr24-review-1.md`
/// Appendix I.3; a platform with a real per-call bound replaces the stub.
#[cfg(unix)]
fn apply_sync_io_timeout(stream: &LocalSocketStream, timeout: Duration) -> DtoResult<()> {
    stream
        .set_recv_timeout(Some(timeout))
        .and_then(|()| stream.set_send_timeout(Some(timeout)))
        .map_err(|_| unavailable("local_daemon_connection_unavailable"))
}

/// Keeps the documented non-Unix blocking behavior: the named-pipe transport
/// cannot express a per-call read or write deadline, so this stub applies no
/// bound (see `apply_sync_io_timeout` and architecture 03 for the recorded
/// limitation, R28).
#[cfg(not(unix))]
const fn apply_sync_io_timeout(_stream: &LocalSocketStream, _timeout: Duration) -> DtoResult<()> {
    Ok(())
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
    fn listener_enforces_private_permissions_and_leaves_its_socket_for_reclaim() {
        use std::os::unix::fs::PermissionsExt;

        let endpoint = endpoint();
        let socket_path = endpoint.path.clone();
        let parent = socket_path
            .parent()
            .expect("socket has a parent")
            .to_owned();
        let listener = LocalListener::bind(endpoint.clone()).expect("listener binds");
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
            "dropping a listener leaves its endpoint for reclaim at the next bind"
        );
        // Reclaim at the next bind is the only removal path.
        let reclaimed = LocalListener::bind(endpoint).expect("the left endpoint is reclaimed");
        drop(reclaimed);
    }

    #[cfg(unix)]
    #[test]
    fn stale_socket_is_reclaimed_but_live_endpoints_and_files_are_never_removed() {
        // An abandoned socket file (unclean daemon exit) must be reclaimed by
        // a later bind (PR24-010).
        let stale = endpoint();
        let stale_path = stale.path.clone();
        let abandoned =
            std::os::unix::net::UnixListener::bind(&stale_path).expect("stale socket seeds");
        drop(abandoned); // UnixListener drop does not unlink the path
        assert!(
            stale_path.exists(),
            "the abandoned socket file must survive its listener"
        );
        let listener = LocalListener::bind(stale).expect("stale socket is reclaimed");
        drop(listener);
        assert!(
            stale_path.exists(),
            "dropping a listener leaves its endpoint for the next reclaim"
        );

        // A live listener on the same path must never be unlinked.
        let live = endpoint();
        let live_path = live.path.clone();
        let _listener = LocalListener::bind(live.clone()).expect("first listener binds");
        let error = match LocalListener::bind(live) {
            Err(error) => error,
            Ok(_) => panic!("second bind conflicts"),
        };
        assert_eq!(error.code(), "local_daemon_endpoint_in_use");
        assert!(
            live_path.exists(),
            "a live listener's socket is never removed"
        );

        // A non-socket path at the endpoint must never be removed.
        let regular = endpoint();
        let regular_path = regular.path.clone();
        fs::write(&regular_path, b"not a socket").expect("regular file seeds");
        let error = match LocalListener::bind(regular) {
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
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn async_listener_enforces_private_permissions_and_removes_owned_socket() {
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
            // with a timeout instead of a refusal (P2-14).
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

    #[cfg(unix)]
    #[test]
    fn dropping_a_listener_never_removes_a_socket_owned_by_another_host() {
        let owned = endpoint();
        let owned_path = owned.path.clone();
        let listener = LocalListener::bind(owned).expect("fixture listener binds");

        // Another host takes over the endpoint path after the socket file was
        // unlinked, exactly as in the saturated-backlog hazard (P2-14).
        let replacement = endpoint();
        let replacement_path = replacement.path;
        let replacement_listener = std::os::unix::net::UnixListener::bind(&replacement_path)
            .expect("replacement socket seeds");
        fs::remove_file(&owned_path).expect("the old socket unlinks");
        fs::rename(&replacement_path, &owned_path).expect("the replacement owns the endpoint");

        drop(listener);
        assert!(
            owned_path.exists(),
            "a dropped listener never unlinks the path it no longer owns"
        );
        std::os::unix::net::UnixStream::connect(&owned_path)
            .expect("the replacement listener stays reachable");
        drop(replacement_listener);
    }

    #[cfg(unix)]
    #[test]
    fn a_silent_peer_fails_closed_within_the_bounded_sync_read_timeout() {
        let endpoint = endpoint();
        let listener = LocalListener::bind(endpoint.clone()).expect("listener binds");
        let (release, held) = std::sync::mpsc::channel::<()>();
        let server = thread::spawn(move || {
            let connection = listener.accept().expect("server accepts");
            let _ = held.recv_timeout(Duration::from_secs(30));
            drop(connection);
        });
        let mut client =
            LocalConnection::connect_with_io_timeout(&endpoint, Duration::from_millis(100))
                .expect("client connects");
        let started = std::time::Instant::now();
        let error = client
            .receive_hello()
            .expect_err("a silent peer must fail closed");
        assert_eq!(error.code(), "local_daemon_connection_unavailable");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the read is bounded by the transport timeout"
        );
        release.send(()).expect("the server releases");
        server.join().expect("the server thread completes");
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

    #[tokio::test]
    async fn async_negotiation_rejects_oversized_malformed_and_truncated_frames() {
        for (header, payload, expected_code) in [
            (
                u32::try_from(MAX_FRAME_BYTES + 1)
                    .expect("frame length fits")
                    .to_be_bytes()
                    .to_vec(),
                Vec::new(),
                "local_protocol_frame_too_large",
            ),
            (
                3_u32.to_be_bytes().to_vec(),
                b"bad".to_vec(),
                "invalid_local_protocol_frame",
            ),
            (
                8_u32.to_be_bytes().to_vec(),
                b"{}".to_vec(),
                "local_daemon_connection_unavailable",
            ),
            (
                vec![0_u8, 0_u8],
                Vec::new(),
                "local_daemon_connection_unavailable",
            ),
        ] {
            let endpoint = endpoint();
            let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
            let server = tokio::spawn(async move {
                let connection = listener.accept().await.expect("server accepts");
                match connection
                    .negotiate(
                        ProtocolHelloDto::new(local_protocol_version(), Vec::new(), "async-server")
                            .expect("fixture hello is valid"),
                    )
                    .await
                {
                    Ok(_) => panic!("invalid hello frame is rejected"),
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
            client.write_all(&header).await.expect("raw header writes");
            client
                .write_all(&payload)
                .await
                .expect("raw payload writes");
            drop(client);
            let error = server.await.expect("server completes");
            assert_eq!(error.code(), expected_code);
        }
    }

    #[tokio::test]
    async fn async_negotiation_rejects_an_oversized_outbound_hello_before_framing() {
        let endpoint = endpoint();
        let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
        let server = tokio::spawn(async move {
            let connection = listener.accept().await.expect("server accepts");
            match connection
                .negotiate(
                    ProtocolHelloDto::new(local_protocol_version(), Vec::new(), "async-server")
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
        let oversized = ProtocolHelloDto::new(
            local_protocol_version(),
            Vec::new(),
            "x".repeat(MAX_FRAME_BYTES + 1),
        )
        .expect("non-empty fixture hello is valid");
        let error = match connection.negotiate(oversized).await {
            Ok(_) => panic!("oversized outbound hello is rejected"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "local_protocol_frame_too_large");
        assert_eq!(
            server.await.expect("server completes").code(),
            "local_daemon_connection_unavailable"
        );
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
