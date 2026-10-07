//! Minimal TUI/REPL proof adapter for the shared local client.
//!
//! This crate owns only terminal-facing presentation mapping. It does not create
//! a daemon implementation, access domain services, or retain business state.

use intention_client::IntentionClient;
use intention_proto::DtoResult;
use intention_proto::{
    DaemonHealthDto, SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
};

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

    /// Subscribes to one session using the shared client protocol mapping.
    ///
    /// The request returns the current session snapshot; there is no cursor and
    /// no resume state.
    ///
    /// # Errors
    ///
    /// Returns a typed client/transport/protocol error. The returned DTO is the
    /// same one available to all other presentation adapters.
    pub async fn subscribe(
        &self,
        subscription: SubscribeSessionCommandDto,
    ) -> DtoResult<SessionSubscriptionResponseDto> {
        self.client.subscribe(subscription).await
    }
}
