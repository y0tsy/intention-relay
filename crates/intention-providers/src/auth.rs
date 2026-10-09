//! The code-owned authentication header policy an adapter applies at construction.
//!
//! The policy carries names only, never values: the credential itself stays in
//! the private construction path of the adapter that applies the policy. A
//! transport the executing adapter cannot apply fails closed where the policy
//! is applied; it is never silently dropped or replaced by another transport.

use intention_proto::DtoResult;
use intention_proto::provider::{CredentialTransportDto, CredentialTransportModeDto};

/// The authentication header policy one adapter applies to its private client.
///
/// The closed declaration vocabulary is the credential transport selected by
/// the exact profile revision: bearer authorization, or one declared safe
/// header whose complete value is the credential.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticationHeaderPolicyV1 {
    transport: CredentialTransportModeDto,
    safe_header_name: Option<String>,
}

impl AuthenticationHeaderPolicyV1 {
    /// Creates a validated authentication header policy.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the safe-header transport carries no
    /// valid header name or the bearer transport carries a header name.
    pub fn new(
        transport: CredentialTransportModeDto,
        safe_header_name: Option<String>,
    ) -> DtoResult<Self> {
        let validated = CredentialTransportDto::new(transport, safe_header_name)?;
        Ok(Self {
            transport: validated.mode(),
            safe_header_name: validated.safe_header_name().map(str::to_owned),
        })
    }

    /// Creates the policy declared by one validated credential transport.
    #[must_use]
    pub fn from_credential_transport(transport: &CredentialTransportDto) -> Self {
        Self {
            transport: transport.mode(),
            safe_header_name: transport.safe_header_name().map(str::to_owned),
        }
    }

    /// Creates the bearer authorization policy.
    #[must_use]
    pub const fn bearer() -> Self {
        Self {
            transport: CredentialTransportModeDto::Bearer,
            safe_header_name: None,
        }
    }

    /// Returns the closed credential transport mode this policy selects.
    #[must_use]
    pub const fn transport(&self) -> CredentialTransportModeDto {
        self.transport
    }

    /// Returns the declared safe header name, when the policy selects one.
    #[must_use]
    pub fn safe_header_name(&self) -> Option<&str> {
        self.safe_header_name.as_deref()
    }

    /// Returns whether the policy selects the safe-header transport.
    #[must_use]
    pub const fn selects_safe_header(&self) -> bool {
        matches!(self.transport, CredentialTransportModeDto::SafeHeader)
    }
}
