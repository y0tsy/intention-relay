//! Minimal TUI/REPL proof adapter for the shared local client.
//!
//! This crate owns only terminal-facing presentation mapping. It does not create
//! a daemon implementation, access domain services, or retain business state.

use intention_client::IntentionClient;
use intention_protocol::{
    DaemonHealthDto, SessionSubscriptionResponseDto, SubscribeSessionCommandDto,
};
use intention_types::DtoResult;

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
    pub fn connect(&self) -> DtoResult<DaemonHealthDto> {
        self.client.connect_or_bootstrap()
    }

    /// Subscribes to one session using the shared client protocol mapping.
    ///
    /// # Errors
    ///
    /// Returns a typed client/transport/protocol error. The returned DTO is the
    /// same one available to all other presentation adapters.
    pub fn subscribe(
        &self,
        subscription: SubscribeSessionCommandDto,
    ) -> DtoResult<SessionSubscriptionResponseDto> {
        self.client.subscribe(subscription)
    }
}
