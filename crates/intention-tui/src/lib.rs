//! Minimal TUI/REPL proof adapter for the shared local client.
//!
//! This crate owns only terminal-facing presentation mapping. It does not create
//! a daemon implementation, access domain services, or retain business state.

use intention_client::IntentionClient;
use intention_proto::{DaemonHealthDto, DtoResult, SessionId, SessionSnapshotDto};

/// A minimal proof adapter that reaches daemon state only through `IntentionClient`.
pub struct TuiProofClient {
    client: IntentionClient,
}

impl TuiProofClient {
    /// Wraps an explicitly configured shared local client.
    #[must_use]
    pub const fn new(client: IntentionClient) -> Self {
        Self { client }
    }

    /// Connects or bootstraps the shared daemon and returns its safe health view.
    ///
    /// # Errors
    ///
    /// Returns a typed client/transport/protocol error without terminal-specific
    /// domain behavior.
    pub async fn connect(&self) -> DtoResult<DaemonHealthDto> {
        self.client.connect_or_bootstrap().await
    }

    /// Reads the current durable session snapshot through the shared client.
    ///
    /// This is the single session read: it returns current durable state, there
    /// is no cursor and no resume state, and a re-read re-reads current state.
    ///
    /// # Errors
    ///
    /// Returns a typed client/transport/protocol error. The returned DTO is the
    /// same one available to all other presentation adapters.
    pub async fn session_snapshot(&self, session_id: SessionId) -> DtoResult<SessionSnapshotDto> {
        self.client.session_snapshot(session_id).await
    }
}
