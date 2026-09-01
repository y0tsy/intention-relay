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
use interprocess::local_socket::traits::tokio::{Listener as _, Stream as _};
use interprocess::local_socket::{ConnectOptions, GenericFilePath, ListenerOptions, PathNameType};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

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

/// An asynchronous local listener with the existing endpoint ownership policy.
///
/// It accepts one client at a time into an opaque daemon-side connection. The
/// caller supplies the runtime; this transport type never creates one.
pub struct AsyncLocalListener {
    listener: TokioLocalSocketListener,
    #[cfg(unix)]
    endpoint: LocalEndpoint,
}

impl AsyncLocalListener {
    /// Binds a user-private local endpoint for asynchronous connections.
    ///
    /// # Errors
    ///
    /// Returns a safe typed error when the parent cannot be prepared, another
    /// listener owns the endpoint, or the local IPC implementation is unavailable.
    pub fn bind(endpoint: LocalEndpoint) -> DtoResult<Self> {
        prepare_parent_directory(&endpoint)?;
        let listener = listener_options(&endpoint)?.create_tokio().map_err(|_| {
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

impl Drop for AsyncLocalListener {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = fs::remove_file(&self.endpoint.path);
        }
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
        let listener = AsyncLocalListener::bind(endpoint).expect("listener binds");
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
