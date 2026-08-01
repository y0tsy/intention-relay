//! Versioned public local-protocol DTOs for Intention Relay.
//!
//! This crate contains no socket framing, client bootstrap, daemon lifecycle, or
//! presentation logic. Those implementation boundaries start in M2.

use intention_domain::{
    GetSessionSnapshotQueryDto, RunModeDto, SendUserTurnCommandDto, StopRunCommandDto,
};
use intention_types::{DtoResult, ErrorDto, SchemaVersionDto, SessionEventSequenceDto, SessionId};
use serde::{Deserialize, Deserializer, Serialize, de};

/// The protocol version negotiated before a client uses local transport.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolVersionDto {
    major: u16,
    minor: u16,
}

impl ProtocolVersionDto {
    /// Creates an explicit local protocol version.
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Returns the incompatible-on-change protocol component.
    #[must_use]
    pub const fn major(self) -> u16 {
        self.major
    }

    /// Returns the additive-compatible protocol component.
    #[must_use]
    pub const fn minor(self) -> u16 {
        self.minor
    }

    /// Rejects a protocol version with a different major component.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the remote major version differs.
    pub fn ensure_compatible_with(self, remote: Self) -> DtoResult<()> {
        if self.major == remote.major {
            Ok(())
        } else {
            Err(ErrorDto::unavailable(
                "incompatible_protocol_version",
                "protocol major versions are incompatible",
            ))
        }
    }
}

/// A feature the local adapter and daemon both understand.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolCapabilityDto {
    /// The peer can subscribe to ordered session snapshots and event tails.
    SessionSubscriptions,
}

/// A safe metadata handshake exchanged before any protocol command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProtocolHelloDto {
    version: ProtocolVersionDto,
    capabilities: Vec<ProtocolCapabilityDto>,
    adapter_name: String,
}

impl<'de> Deserialize<'de> for ProtocolHelloDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProtocolHelloDto {
            version: ProtocolVersionDto,
            capabilities: Vec<ProtocolCapabilityDto>,
            adapter_name: String,
        }

        let raw = RawProtocolHelloDto::deserialize(deserializer)?;
        Self::new(raw.version, raw.capabilities, raw.adapter_name).map_err(de::Error::custom)
    }
}

impl ProtocolHelloDto {
    /// Creates a handshake with a non-empty adapter metadata name.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the adapter name is blank.
    pub fn new(
        version: ProtocolVersionDto,
        capabilities: Vec<ProtocolCapabilityDto>,
        adapter_name: impl Into<String>,
    ) -> DtoResult<Self> {
        let adapter_name = adapter_name.into();
        if adapter_name.trim().is_empty() {
            Err(ErrorDto::validation(
                "invalid_adapter_name",
                "adapter name must not be empty",
            ))
        } else {
            Ok(Self {
                version,
                capabilities,
                adapter_name,
            })
        }
    }

    /// Returns the peer protocol version.
    #[must_use]
    pub const fn version(&self) -> ProtocolVersionDto {
        self.version
    }

    /// Returns the peer's explicitly declared capabilities.
    #[must_use]
    pub fn capabilities(&self) -> &[ProtocolCapabilityDto] {
        &self.capabilities
    }

    /// Returns the safe local adapter metadata name.
    #[must_use]
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }
}

/// A subscription request scoped to one durable session.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubscribeSessionCommandDto {
    schema_version: SchemaVersionDto,
    session_id: SessionId,
    after_sequence: Option<SessionEventSequenceDto>,
    requested_mode: RunModeDto,
}

impl SubscribeSessionCommandDto {
    /// Creates a typed subscription request.
    #[must_use]
    pub const fn new(
        schema_version: SchemaVersionDto,
        session_id: SessionId,
        after_sequence: Option<SessionEventSequenceDto>,
        requested_mode: RunModeDto,
    ) -> Self {
        Self {
            schema_version,
            session_id,
            after_sequence,
            requested_mode,
        }
    }

    /// Returns the request schema version.
    #[must_use]
    pub const fn schema_version(self) -> SchemaVersionDto {
        self.schema_version
    }

    /// Returns the subscribed session identity.
    #[must_use]
    pub const fn session_id(self) -> SessionId {
        self.session_id
    }

    /// Returns the last durable sequence already observed, if any.
    #[must_use]
    pub const fn after_sequence(self) -> Option<SessionEventSequenceDto> {
        self.after_sequence
    }

    /// Returns the requesting adapter's current mode projection.
    #[must_use]
    pub const fn requested_mode(self) -> RunModeDto {
        self.requested_mode
    }
}

/// A typed protocol command wrapper with no transport-specific resources.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProtocolCommandDto {
    /// Sends an accepted user turn to the daemon authority.
    SendUserTurn(SendUserTurnCommandDto),
    /// Requests cancellation of an active daemon-owned run.
    StopRun(StopRunCommandDto),
    /// Begins a typed session event subscription.
    SubscribeSession(SubscribeSessionCommandDto),
}

/// A typed protocol query wrapper with no transport-specific resources.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ProtocolQueryDto {
    /// Obtains the latest durable session projection.
    GetSessionSnapshot(GetSessionSnapshotQueryDto),
}

/// A typed command result independent of a transport codec.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
pub enum ProtocolCommandResultDto {
    /// The daemon accepted the command and will publish resulting state separately.
    Accepted(ProtocolAcceptedDto),
    /// The command was safely rejected before execution.
    Rejected(ErrorDto),
}

/// The immutable correlation data returned for an accepted command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProtocolAcceptedDto {
    correlation_id: String,
}

impl<'de> Deserialize<'de> for ProtocolAcceptedDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawProtocolAcceptedDto {
            correlation_id: String,
        }

        let raw = RawProtocolAcceptedDto::deserialize(deserializer)?;
        Self::new(raw.correlation_id).map_err(de::Error::custom)
    }
}

impl ProtocolAcceptedDto {
    /// Creates a non-empty opaque command correlation reference.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the reference is blank.
    pub fn new(correlation_id: impl Into<String>) -> DtoResult<Self> {
        let correlation_id = correlation_id.into();
        if correlation_id.trim().is_empty() {
            Err(ErrorDto::validation(
                "invalid_correlation_id",
                "correlation identifier must not be empty",
            ))
        } else {
            Ok(Self { correlation_id })
        }
    }

    /// Returns the opaque correlation reference.
    #[must_use]
    pub fn correlation_id(&self) -> &str {
        &self.correlation_id
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;
    use intention_types::TurnId;

    #[test]
    fn protocol_versions_and_hello_validate_all_paths() {
        let version = ProtocolVersionDto::new(1, 2);
        assert_eq!(version.major(), 1);
        assert_eq!(version.minor(), 2);
        assert!(
            version
                .ensure_compatible_with(ProtocolVersionDto::new(1, 3))
                .is_ok()
        );
        assert_eq!(
            version
                .ensure_compatible_with(ProtocolVersionDto::new(2, 0))
                .expect_err("major mismatch must fail")
                .category(),
            intention_types::ErrorCategoryDto::Unavailable
        );
        let hello = ProtocolHelloDto::new(
            version,
            vec![ProtocolCapabilityDto::SessionSubscriptions],
            "fixture",
        )
        .expect("fixture hello is valid");
        assert_eq!(hello.version(), version);
        assert_eq!(
            hello.capabilities(),
            &[ProtocolCapabilityDto::SessionSubscriptions]
        );
        assert_eq!(hello.adapter_name(), "fixture");
        assert_eq!(
            ProtocolHelloDto::new(version, Vec::new(), " ")
                .expect_err("blank adapter must fail")
                .code(),
            "invalid_adapter_name"
        );
    }

    #[test]
    fn protocol_wrappers_and_results_preserve_domain_dtos() {
        let session_id = SessionId::new();
        let subscription = SubscribeSessionCommandDto::new(
            SchemaVersionDto::new(1, 0),
            session_id,
            Some(SessionEventSequenceDto::new(4)),
            RunModeDto::Plan,
        );
        assert_eq!(subscription.schema_version(), SchemaVersionDto::new(1, 0));
        assert_eq!(subscription.session_id(), session_id);
        assert_eq!(
            subscription.after_sequence(),
            Some(SessionEventSequenceDto::new(4))
        );
        assert_eq!(subscription.requested_mode(), RunModeDto::Plan);

        let commands = [
            ProtocolCommandDto::SendUserTurn(
                SendUserTurnCommandDto::new(session_id, TurnId::new(), "hello")
                    .expect("fixture turn is valid"),
            ),
            ProtocolCommandDto::StopRun(StopRunCommandDto::new(
                session_id,
                intention_types::RunId::new(),
            )),
            ProtocolCommandDto::SubscribeSession(subscription),
        ];
        for command in commands {
            let encoded = serde_json::to_string(&command).expect("command serialization succeeds");
            let _: ProtocolCommandDto =
                serde_json::from_str(&encoded).expect("command parsing succeeds");
        }
        let query =
            ProtocolQueryDto::GetSessionSnapshot(GetSessionSnapshotQueryDto::new(session_id));
        let query_encoded = serde_json::to_string(&query).expect("query serialization succeeds");
        let _: ProtocolQueryDto =
            serde_json::from_str(&query_encoded).expect("query parsing succeeds");

        let accepted =
            ProtocolAcceptedDto::new("correlation").expect("non-empty correlation is valid");
        assert_eq!(accepted.correlation_id(), "correlation");
        assert_eq!(
            ProtocolAcceptedDto::new(" ")
                .expect_err("blank correlation must fail")
                .code(),
            "invalid_correlation_id"
        );
        let results = [
            ProtocolCommandResultDto::Accepted(accepted),
            ProtocolCommandResultDto::Rejected(ErrorDto::validation("fixture", "message")),
        ];
        for result in results {
            let encoded = serde_json::to_string(&result).expect("result serialization succeeds");
            let _: ProtocolCommandResultDto =
                serde_json::from_str(&encoded).expect("result parsing succeeds");
        }
    }
}
