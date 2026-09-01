//! Provider kind descriptors, profile revisions, driver contracts, and
//! tombstones for the domain-owned provider catalog.
//!
//! Catalog records are credential-free safe identities. Credential literals,
//! display names, enabled state, readiness, SDK/client resources, and current
//! configuration never enter canonical bytes or digests.

use crate::canonical::{
    CanonicalError, CanonicalIdentityInput, CanonicalRecordBuilder, CanonicalRecordReader,
    Digest256, NamespacedDigest, TagRegistry, WireType, contains_control_or_nul,
    contains_credential_shape, decode_optional_utf8, decode_utf8, decode_utf8_list,
    encode_optional_utf8, encode_u64, encode_utf8, encode_utf8_list,
};
use crate::provider_selection::{MODEL_CAPABILITY_TAXONOMY_V1, ModelCapabilitySetV1};

/// Maximum characters of a provider profile or user kind identifier.
pub const MAX_PROVIDER_ID_CHARS: usize = 63;
/// Maximum characters of a safe header name.
pub const MAX_SAFE_HEADER_NAME_CHARS: usize = 128;
/// Maximum characters of a generic provider-domain scalar string.
pub const MAX_PROVIDER_STRING_CHARS: usize = 256;
/// Maximum enabled profiles in one accepted catalog.
pub const MAX_PROVIDER_PROFILES: usize = 128;
/// Maximum user-declared kinds in one accepted catalog.
pub const MAX_PROVIDER_KINDS: usize = 32;
/// Maximum raw catalog candidate input bytes.
pub const MAX_PROVIDER_CATALOG_CANDIDATE_BYTES: usize = 512 * 1024;
/// Maximum safe validation issues returned for one candidate.
pub const MAX_CATALOG_ISSUES: usize = 32;

/// The closed credential transport modes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialTransportMode {
    /// Authorization through the standard bearer scheme.
    Bearer,
    /// Authorization through one descriptor-selected safe header name.
    SafeHeader,
}

impl CredentialTransportMode {
    pub(crate) fn enc(self) -> Vec<u8> {
        vec![match self {
            Self::Bearer => 0,
            Self::SafeHeader => 1,
        }]
    }

    pub(crate) fn dec(bytes: &[u8]) -> Result<Self, CanonicalError> {
        match bytes {
            [0] => Ok(Self::Bearer),
            [1] => Ok(Self::SafeHeader),
            _ => Err(CanonicalError::InvalidField),
        }
    }
}

/// The fixed code-owned provider catalog bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderCatalogLimits {
    pub max_profiles: usize,
    pub max_kinds: usize,
    pub max_candidate_bytes: usize,
    pub max_issues: usize,
}

impl ProviderCatalogLimits {
    /// The frozen catalog limit values.
    #[must_use]
    pub const fn frozen() -> Self {
        Self {
            max_profiles: MAX_PROVIDER_PROFILES,
            max_kinds: MAX_PROVIDER_KINDS,
            max_candidate_bytes: MAX_PROVIDER_CATALOG_CANDIDATE_BYTES,
            max_issues: MAX_CATALOG_ISSUES,
        }
    }

    /// Validates that every limit equals its frozen value.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::OverLimit` when any limit differs from the
    /// frozen catalog value.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        if *self != Self::frozen() {
            return Err(CanonicalError::OverLimit);
        }
        Ok(())
    }
}

/// Validates a provider-domain scalar string against a character bound.
///
/// # Errors
///
/// Returns `CanonicalError::ProviderProfileRevisionInvalid` when the value is
/// empty, exceeds `max_chars` characters, or carries control characters.
pub fn validate_provider_string(value: &str, max_chars: usize) -> Result<(), CanonicalError> {
    if value.is_empty() || value.len() > max_chars || contains_control_or_nul(value) {
        return Err(CanonicalError::ProviderProfileRevisionInvalid);
    }
    Ok(())
}

/// Validates a provider kind identifier against the closed kind policy.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidProviderKind` when the kind id is `openai`
/// or is empty, over `MAX_PROVIDER_ID_CHARS` characters, or carries control
/// characters. The `openai` alias never enters a DTO, canonical record,
/// digest, or durable fact.
pub fn validate_provider_kind_id(kind_id: &str) -> Result<(), CanonicalError> {
    if kind_id == "openai"
        || kind_id.is_empty()
        || kind_id.len() > MAX_PROVIDER_ID_CHARS
        || contains_control_or_nul(kind_id)
    {
        return Err(CanonicalError::InvalidProviderKind);
    }
    Ok(())
}

/// Validates a provider profile identifier.
///
/// # Errors
///
/// Returns `CanonicalError::ProviderProfileRevisionInvalid` when the id is
/// empty, exceeds `MAX_PROVIDER_ID_CHARS` characters, or carries control
/// characters.
pub fn validate_profile_id(profile_id: &str) -> Result<(), CanonicalError> {
    validate_provider_string(profile_id, MAX_PROVIDER_ID_CHARS)
}

/// Validates an endpoint as credential-free execution metadata.
///
/// The endpoint must be absolute HTTPS or HTTP with no userinfo, query,
/// fragment, control characters, whitespace, or malformed percent escapes.
/// Raw or secret-bearing URL input is never public or durable identity.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidEndpoint` when the endpoint is invalid or
/// `CanonicalError::CredentialsForbidden` when it carries a credential-shaped
/// token.
pub fn validate_endpoint(endpoint: &str) -> Result<(), CanonicalError> {
    let lower = endpoint.to_ascii_lowercase();
    let scheme_len = if let Some(rest) = lower.strip_prefix("https://") {
        if rest.is_empty() {
            return Err(CanonicalError::InvalidEndpoint);
        }
        "https://".len()
    } else if let Some(rest) = lower.strip_prefix("http://") {
        if rest.is_empty() {
            return Err(CanonicalError::InvalidEndpoint);
        }
        "http://".len()
    } else {
        return Err(CanonicalError::InvalidEndpoint);
    };
    if endpoint.contains('@')
        || endpoint.contains('?')
        || endpoint.contains('#')
        || contains_control_or_nul(endpoint)
        || endpoint.bytes().any(|byte| byte.is_ascii_whitespace())
        || !valid_percent_escapes(endpoint)
    {
        return Err(CanonicalError::InvalidEndpoint);
    }
    if contains_credential_shape(&endpoint[scheme_len..]) {
        return Err(CanonicalError::CredentialsForbidden);
    }
    Ok(())
}

/// Whether every `%` in `value` begins a valid two-digit hex escape.
#[must_use]
fn valid_percent_escapes(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

/// Validates one safe header name: a name only, never a value.
///
/// The name must be a non-empty HTTP token of at most
/// `MAX_SAFE_HEADER_NAME_CHARS` characters.
///
/// # Errors
///
/// Returns `CanonicalError::ProviderProfileRevisionInvalid` when the name is
/// empty, over the character bound, or carries a character outside the HTTP
/// token set, and `CanonicalError::CredentialsForbidden` when the name carries
/// a credential-shaped token.
pub fn validate_safe_header_name(name: &str) -> Result<(), CanonicalError> {
    if contains_credential_shape(name) {
        return Err(CanonicalError::CredentialsForbidden);
    }
    if name.is_empty()
        || name.len() > MAX_SAFE_HEADER_NAME_CHARS
        || !name.bytes().all(is_header_token_byte)
    {
        return Err(CanonicalError::ProviderProfileRevisionInvalid);
    }
    Ok(())
}

/// Whether `byte` is one HTTP token character (`tchar`).
const fn is_header_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// Validates the closed credential transport consistency.
///
/// # Errors
///
/// Returns `CanonicalError::ProviderProfileRevisionInvalid` when `Bearer`
/// carries a header name or `SafeHeader` carries no header name, and the
/// errors of [`validate_safe_header_name`] for the header name itself.
pub fn validate_credential_transport(
    mode: CredentialTransportMode,
    safe_header_name: &Option<String>,
) -> Result<(), CanonicalError> {
    match mode {
        CredentialTransportMode::Bearer => {
            if safe_header_name.is_some() {
                return Err(CanonicalError::ProviderProfileRevisionInvalid);
            }
            Ok(())
        }
        CredentialTransportMode::SafeHeader => safe_header_name.as_ref().map_or(
            Err(CanonicalError::ProviderProfileRevisionInvalid),
            |name| validate_safe_header_name(name),
        ),
    }
}

/// The code-owned driver contract identity of one provider driver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderDriverContractRevisionDto {
    pub driver_family: String,
    pub major: u64,
    pub minor: u64,
}

impl ProviderDriverContractRevisionDto {
    /// Validates the driver contract revision.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ProviderProfileRevisionInvalid` when the
    /// driver family is blank, over the scalar bound, or carries control
    /// characters.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        validate_provider_string(&self.driver_family, MAX_PROVIDER_STRING_CHARS)
    }

    /// Encodes this driver contract into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ProviderProfileRevisionInvalid` when the
    /// driver family is invalid, and
    /// `CanonicalError::DuplicateOrDescendingField` or
    /// `CanonicalError::OverLimit` only if the fixed field table were
    /// noncanonical or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        record(
            0,
            1,
            vec![
                (1, WireType::Utf8, encode_utf8(&self.driver_family)),
                (2, WireType::U64, encode_u64(self.major)),
                (3, WireType::U64, encode_u64(self.minor)),
            ],
        )
    }

    /// Decodes this driver contract from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the nested record is not the
    /// anonymous tag-zero version-one frame, `CanonicalError::InvalidField`
    /// when any of the three fields is absent or malformed,
    /// `CanonicalError::ProviderProfileRevisionInvalid` when the driver
    /// family is invalid, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 3)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let contract = Self {
            driver_family: decode_utf8(
                reader
                    .field(1, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            major: decode_u64_field(&reader, 2)?,
            minor: decode_u64_field(&reader, 3)?,
        };
        contract.validate()?;
        Ok(contract)
    }
}

/// One immutable provider kind descriptor revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderKindDescriptorRevisionV1 {
    pub kind_id: String,
    pub descriptor_family: String,
    pub ordered_protocol_part_revisions: Vec<String>,
    pub endpoint_policy: String,
    pub credential_transport_contract: String,
    pub model_capability_envelope: ModelCapabilitySetV1,
    pub driver_contract_family: String,
}

impl ProviderKindDescriptorRevisionV1 {
    /// Validates the kind descriptor revision.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidProviderKind` when the kind id or any
    /// descriptor scalar is invalid, and the model capability envelope's
    /// validation errors.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        validate_provider_kind_id(&self.kind_id)?;
        validate_kind_descriptor_string(&self.descriptor_family)?;
        for part in &self.ordered_protocol_part_revisions {
            validate_kind_descriptor_string(part)?;
        }
        validate_kind_descriptor_string(&self.endpoint_policy)?;
        validate_kind_descriptor_string(&self.credential_transport_contract)?;
        self.model_capability_envelope.validate()?;
        validate_kind_descriptor_string(&self.driver_contract_family)
    }

    /// Encodes this descriptor into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`], and
    /// `CanonicalError::DuplicateOrDescendingField` or
    /// `CanonicalError::OverLimit` only if the fixed field table were
    /// noncanonical or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        record(
            0,
            1,
            vec![
                (1, WireType::Utf8, encode_utf8(&self.kind_id)),
                (2, WireType::Utf8, encode_utf8(&self.descriptor_family)),
                (
                    3,
                    WireType::List,
                    encode_utf8_list(&self.ordered_protocol_part_revisions),
                ),
                (4, WireType::Utf8, encode_utf8(&self.endpoint_policy)),
                (
                    5,
                    WireType::Utf8,
                    encode_utf8(&self.credential_transport_contract),
                ),
                (
                    6,
                    WireType::Record,
                    self.model_capability_envelope.encode()?,
                ),
                (7, WireType::Utf8, encode_utf8(&self.driver_contract_family)),
            ],
        )
    }

    /// Decodes this descriptor from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the nested record is not the
    /// anonymous tag-zero version-one frame, `CanonicalError::InvalidField`
    /// when any of the seven fields is absent or malformed, the validation
    /// errors of [`Self::validate`], and other `CanonicalError` values for
    /// malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 7)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let descriptor = Self {
            kind_id: decode_utf8(
                reader
                    .field(1, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            descriptor_family: decode_utf8(
                reader
                    .field(2, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            ordered_protocol_part_revisions: decode_utf8_list(
                reader
                    .field(3, WireType::List)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            endpoint_policy: decode_utf8(
                reader
                    .field(4, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            credential_transport_contract: decode_utf8(
                reader
                    .field(5, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            model_capability_envelope: ModelCapabilitySetV1::decode(
                reader
                    .field(6, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            driver_contract_family: decode_utf8(
                reader
                    .field(7, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
        };
        descriptor.validate()?;
        Ok(descriptor)
    }
}

/// One append-only provider profile revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderProfileRevisionV1 {
    pub profile_id: String,
    pub revision_id: String,
    pub provider_kind_id: String,
    pub model_id: String,
    pub endpoint: String,
    pub credential_transport_mode: CredentialTransportMode,
    pub safe_header_name: Option<String>,
    pub capability_taxonomy_revision: String,
    pub reasoning_compatibility_id: Option<String>,
    pub kind_descriptor_revision_id: String,
    pub driver_contract_revision: ProviderDriverContractRevisionDto,
}

impl ProviderProfileRevisionV1 {
    /// Validates the profile revision's closed values.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidProviderKind` when the provider kind
    /// id is invalid, `CanonicalError::InvalidEndpoint` when the endpoint is
    /// invalid, `CanonicalError::CredentialsForbidden` when the safe header
    /// name carries a credential-shaped value, and
    /// `CanonicalError::ProviderProfileRevisionInvalid` for every other
    /// invalid profile value.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        validate_profile_id(&self.profile_id)?;
        validate_provider_string(&self.revision_id, MAX_PROVIDER_ID_CHARS)?;
        validate_provider_kind_id(&self.provider_kind_id)?;
        validate_provider_string(&self.model_id, MAX_PROVIDER_ID_CHARS)?;
        validate_endpoint(&self.endpoint)?;
        validate_credential_transport(self.credential_transport_mode, &self.safe_header_name)?;
        if self.capability_taxonomy_revision != MODEL_CAPABILITY_TAXONOMY_V1 {
            return Err(CanonicalError::ProviderProfileRevisionInvalid);
        }
        if let Some(compatibility_id) = &self.reasoning_compatibility_id {
            validate_provider_string(compatibility_id, MAX_PROVIDER_STRING_CHARS)?;
        }
        validate_provider_string(&self.kind_descriptor_revision_id, MAX_PROVIDER_ID_CHARS)?;
        self.driver_contract_revision.validate()
    }

    /// Encodes this profile revision into its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`], and
    /// `CanonicalError::DuplicateOrDescendingField` or
    /// `CanonicalError::OverLimit` only if the fixed field table were
    /// noncanonical or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        record(
            TagRegistry::PROVIDER_PROFILE_REVISION_V1,
            1,
            vec![
                (1, WireType::Utf8, encode_utf8(&self.profile_id)),
                (2, WireType::Utf8, encode_utf8(&self.revision_id)),
                (3, WireType::Utf8, encode_utf8(&self.provider_kind_id)),
                (4, WireType::Utf8, encode_utf8(&self.model_id)),
                (5, WireType::Utf8, encode_utf8(&self.endpoint)),
                (6, WireType::U64, self.credential_transport_mode.enc()),
                (
                    7,
                    WireType::Optional,
                    encode_optional_utf8(&self.safe_header_name),
                ),
                (
                    8,
                    WireType::Utf8,
                    encode_utf8(&self.capability_taxonomy_revision),
                ),
                (
                    9,
                    WireType::Optional,
                    encode_optional_utf8(&self.reasoning_compatibility_id),
                ),
                (
                    10,
                    WireType::Utf8,
                    encode_utf8(&self.kind_descriptor_revision_id),
                ),
                (
                    11,
                    WireType::Record,
                    self.driver_contract_revision.encode()?,
                ),
            ],
        )
    }

    /// Decodes this profile revision from its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the record is not the
    /// provider-profile-revision-v1 table, `CanonicalError::InvalidField`
    /// when any of the eleven fields is absent or malformed, the validation
    /// errors of [`Self::validate`], and other `CanonicalError` values for
    /// malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 11)?;
        if reader.tag != TagRegistry::PROVIDER_PROFILE_REVISION_V1 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let profile = Self {
            profile_id: decode_utf8(
                reader
                    .field(1, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            revision_id: decode_utf8(
                reader
                    .field(2, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            provider_kind_id: decode_utf8(
                reader
                    .field(3, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            model_id: decode_utf8(
                reader
                    .field(4, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            endpoint: decode_utf8(
                reader
                    .field(5, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            credential_transport_mode: CredentialTransportMode::dec(
                reader
                    .field(6, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            safe_header_name: decode_optional_utf8(
                reader
                    .field(7, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            capability_taxonomy_revision: decode_utf8(
                reader
                    .field(8, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            reasoning_compatibility_id: decode_optional_utf8(
                reader
                    .field(9, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            kind_descriptor_revision_id: decode_utf8(
                reader
                    .field(10, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            driver_contract_revision: ProviderDriverContractRevisionDto::decode(
                reader
                    .field(11, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        profile.validate()?;
        Ok(profile)
    }
}

/// A permanent safe identity record for one removed provider profile.
///
/// A tombstoned profile id can never be reintroduced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderProfileTombstoneDto {
    pub profile_id: String,
    pub removed_catalog_revision: u64,
    pub removed_time: u64,
    pub provenance: String,
    pub digest: Digest256,
}

impl ProviderProfileTombstoneDto {
    /// Creates a permanent profile tombstone with its identity digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ProviderProfileRevisionInvalid` when the
    /// profile id is invalid or the provenance is invalid.
    pub fn new(
        profile_id: impl Into<String>,
        removed_catalog_revision: u64,
        removed_time: u64,
        provenance: impl Into<String>,
    ) -> Result<Self, CanonicalError> {
        let profile_id = profile_id.into();
        let provenance = provenance.into();
        validate_profile_id(&profile_id)?;
        validate_provider_string(&provenance, MAX_PROVIDER_STRING_CHARS)?;
        let identity = tombstone_identity(&[
            (1, WireType::Utf8, encode_utf8(&profile_id)),
            (2, WireType::U64, encode_u64(removed_catalog_revision)),
            (3, WireType::U64, encode_u64(removed_time)),
            (4, WireType::Utf8, encode_utf8(&provenance)),
        ]);
        Ok(Self {
            profile_id,
            removed_catalog_revision,
            removed_time,
            provenance,
            digest: Digest256::sha256(&identity),
        })
    }

    /// Encodes this tombstone into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ProviderProfileRevisionInvalid` when the
    /// tombstone identity is invalid, and
    /// `CanonicalError::DuplicateOrDescendingField` or
    /// `CanonicalError::OverLimit` only if the fixed field table were
    /// noncanonical or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        validate_profile_id(&self.profile_id)?;
        validate_provider_string(&self.provenance, MAX_PROVIDER_STRING_CHARS)?;
        record(
            0,
            1,
            vec![
                (1, WireType::Utf8, encode_utf8(&self.profile_id)),
                (2, WireType::U64, encode_u64(self.removed_catalog_revision)),
                (3, WireType::U64, encode_u64(self.removed_time)),
                (4, WireType::Utf8, encode_utf8(&self.provenance)),
                (5, WireType::Digest, self.digest.bytes().to_vec()),
            ],
        )
    }

    /// Decodes this tombstone from its nested anonymous record and verifies
    /// its identity digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the nested record is not the
    /// anonymous tag-zero version-one frame, `CanonicalError::InvalidField`
    /// when any of the five fields is absent or malformed,
    /// `CanonicalError::DigestMismatch` when the stored digest does not match
    /// the identity bytes, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 5)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let profile_id = decode_utf8(
            reader
                .field(1, WireType::Utf8)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .to_owned();
        let removed_catalog_revision = decode_u64_field(&reader, 2)?;
        let removed_time = decode_u64_field(&reader, 3)?;
        let provenance = decode_utf8(
            reader
                .field(4, WireType::Utf8)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .to_owned();
        let digest = Digest256::from_bytes(
            reader
                .field(5, WireType::Digest)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        let identity = tombstone_identity(&[
            (1, WireType::Utf8, encode_utf8(&profile_id)),
            (2, WireType::U64, encode_u64(removed_catalog_revision)),
            (3, WireType::U64, encode_u64(removed_time)),
            (4, WireType::Utf8, encode_utf8(&provenance)),
        ]);
        if Digest256::sha256(&identity) != digest {
            return Err(CanonicalError::DigestMismatch);
        }
        let tombstone = Self {
            profile_id,
            removed_catalog_revision,
            removed_time,
            provenance,
            digest,
        };
        tombstone.validate()?;
        Ok(tombstone)
    }

    fn validate(&self) -> Result<(), CanonicalError> {
        validate_profile_id(&self.profile_id)?;
        validate_provider_string(&self.provenance, MAX_PROVIDER_STRING_CHARS)
    }
}

/// A permanent safe identity record for one removed provider kind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderKindTombstoneDto {
    pub kind_id: String,
    pub removed_catalog_revision: u64,
    pub removed_time: u64,
    pub provenance: String,
    pub digest: Digest256,
}

impl ProviderKindTombstoneDto {
    /// Creates a permanent kind tombstone with its identity digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidProviderKind` when the kind id is
    /// invalid or the provenance is invalid.
    pub fn new(
        kind_id: impl Into<String>,
        removed_catalog_revision: u64,
        removed_time: u64,
        provenance: impl Into<String>,
    ) -> Result<Self, CanonicalError> {
        let kind_id = kind_id.into();
        let provenance = provenance.into();
        validate_provider_kind_id(&kind_id)?;
        validate_kind_descriptor_string(&provenance)?;
        let identity = tombstone_identity(&[
            (1, WireType::Utf8, encode_utf8(&kind_id)),
            (2, WireType::U64, encode_u64(removed_catalog_revision)),
            (3, WireType::U64, encode_u64(removed_time)),
            (4, WireType::Utf8, encode_utf8(&provenance)),
        ]);
        Ok(Self {
            kind_id,
            removed_catalog_revision,
            removed_time,
            provenance,
            digest: Digest256::sha256(&identity),
        })
    }

    /// Encodes this tombstone into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidProviderKind` when the tombstone
    /// identity is invalid, and `CanonicalError::DuplicateOrDescendingField`
    /// or `CanonicalError::OverLimit` only if the fixed field table were
    /// noncanonical or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        validate_provider_kind_id(&self.kind_id)?;
        validate_kind_descriptor_string(&self.provenance)?;
        record(
            0,
            1,
            vec![
                (1, WireType::Utf8, encode_utf8(&self.kind_id)),
                (2, WireType::U64, encode_u64(self.removed_catalog_revision)),
                (3, WireType::U64, encode_u64(self.removed_time)),
                (4, WireType::Utf8, encode_utf8(&self.provenance)),
                (5, WireType::Digest, self.digest.bytes().to_vec()),
            ],
        )
    }

    /// Decodes this tombstone from its nested anonymous record and verifies
    /// its identity digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the nested record is not the
    /// anonymous tag-zero version-one frame, `CanonicalError::InvalidField`
    /// when any of the five fields is absent or malformed,
    /// `CanonicalError::DigestMismatch` when the stored digest does not match
    /// the identity bytes, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 5)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let kind_id = decode_utf8(
            reader
                .field(1, WireType::Utf8)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .to_owned();
        let removed_catalog_revision = decode_u64_field(&reader, 2)?;
        let removed_time = decode_u64_field(&reader, 3)?;
        let provenance = decode_utf8(
            reader
                .field(4, WireType::Utf8)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .to_owned();
        let digest = Digest256::from_bytes(
            reader
                .field(5, WireType::Digest)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        let identity = tombstone_identity(&[
            (1, WireType::Utf8, encode_utf8(&kind_id)),
            (2, WireType::U64, encode_u64(removed_catalog_revision)),
            (3, WireType::U64, encode_u64(removed_time)),
            (4, WireType::Utf8, encode_utf8(&provenance)),
        ]);
        if Digest256::sha256(&identity) != digest {
            return Err(CanonicalError::DigestMismatch);
        }
        let tombstone = Self {
            kind_id,
            removed_catalog_revision,
            removed_time,
            provenance,
            digest,
        };
        tombstone.validate()?;
        Ok(tombstone)
    }

    fn validate(&self) -> Result<(), CanonicalError> {
        validate_provider_kind_id(&self.kind_id)?;
        validate_kind_descriptor_string(&self.provenance)
    }
}

/// Validates that a kind revision preserves the immutable kind identity and
/// closed parts.
///
/// # Errors
///
/// Returns `CanonicalError::ProviderKindImmutableMismatch` when the kind id,
/// descriptor family, endpoint policy, credential transport contract, or
/// driver contract family changes between revisions. The valid path for such
/// a change is a new kind id plus reassignment.
pub fn validate_provider_kind_revision_immutability(
    previous: &ProviderKindDescriptorRevisionV1,
    next: &ProviderKindDescriptorRevisionV1,
) -> Result<(), CanonicalError> {
    if previous.kind_id != next.kind_id
        || previous.descriptor_family != next.descriptor_family
        || previous.endpoint_policy != next.endpoint_policy
        || previous.credential_transport_contract != next.credential_transport_contract
        || previous.driver_contract_family != next.driver_contract_family
    {
        return Err(CanonicalError::ProviderKindImmutableMismatch);
    }
    Ok(())
}

/// Validates that a kind removal has no remaining dependent profiles.
///
/// # Errors
///
/// Returns `CanonicalError::ProviderKindHasDependents` when any profile still
/// references `kind_id`.
pub fn validate_provider_kind_removal(
    kind_id: &str,
    dependent_profiles: &[ProviderProfileRevisionV1],
) -> Result<(), CanonicalError> {
    if dependent_profiles
        .iter()
        .any(|profile| profile.provider_kind_id == kind_id)
    {
        return Err(CanonicalError::ProviderKindHasDependents);
    }
    Ok(())
}

/// Validates that a tombstoned profile id is never reintroduced.
///
/// # Errors
///
/// Returns `CanonicalError::ProviderProfileRevisionInvalid` when `profile_id`
/// appears in `tombstones`.
pub fn validate_profile_id_not_tombstoned(
    profile_id: &str,
    tombstones: &[ProviderProfileTombstoneDto],
) -> Result<(), CanonicalError> {
    if tombstones
        .iter()
        .any(|tombstone| tombstone.profile_id == profile_id)
    {
        return Err(CanonicalError::ProviderProfileRevisionInvalid);
    }
    Ok(())
}

/// Computes the namespaced provider-profile-revision digest over
/// identity-bearing fields only.
///
/// The input is a [`CanonicalIdentityInput`] so that credentials, filesystem
/// paths, display data, readiness, and current state supplied through the
/// `with_*` setters are retained for provenance only and never reach the
/// digest.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidDigest` when the namespace is invalid, and
/// the identity input's own encoding errors, which are impossible for a
/// canonical field stream.
pub fn provider_profile_revision_digest(
    input: CanonicalIdentityInput,
) -> Result<NamespacedDigest, CanonicalError> {
    Digest256::for_namespace("provider-profile-revision", &input.encode()?)
}

/// Validates one kind descriptor scalar string.
fn validate_kind_descriptor_string(value: &str) -> Result<(), CanonicalError> {
    if value.is_empty() || value.len() > MAX_PROVIDER_STRING_CHARS || contains_control_or_nul(value)
    {
        return Err(CanonicalError::InvalidProviderKind);
    }
    Ok(())
}

/// Decodes a `u64` field value.
fn decode_u64_field(
    reader: &CanonicalRecordReader<'_>,
    number: u32,
) -> Result<u64, CanonicalError> {
    crate::canonical::decode_u64(
        reader
            .field(number, WireType::U64)?
            .ok_or(CanonicalError::InvalidField)?,
    )
}

/// Builds the canonical identity bytes of one tombstone from its first four
/// fields; the digest field is excluded by construction.
#[must_use]
fn tombstone_identity(fields: &[(u32, WireType, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"IRCR");
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&1u32.to_be_bytes());
    for (number, wire_type, value) in fields {
        out.extend_from_slice(&number.to_be_bytes());
        out.push(*wire_type as u8);
        out.extend_from_slice(&(value.len() as u32).to_be_bytes());
        out.extend_from_slice(value);
    }
    out
}

/// Encodes one canonical record from a strictly increasing field stream.
///
/// # Errors
///
/// Returns `CanonicalError::DuplicateOrDescendingField` or
/// `CanonicalError::OverLimit` only if the field stream were noncanonical or
/// a field or the record exceeded the codec size bounds.
fn record(
    tag: u32,
    version: u32,
    fields: Vec<(u32, WireType, Vec<u8>)>,
) -> Result<Vec<u8>, CanonicalError> {
    let mut builder = CanonicalRecordBuilder::new(tag, version);
    for (number, wire_type, value) in fields {
        builder = builder.field(number, wire_type, value)?;
    }
    builder.finish()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;
    use crate::provider_selection::{
        ContextPreservationCapability, MODEL_CAPABILITY_TAXONOMY_V1, ModelCapabilitySetV1,
        ModelInputCapability, ReasoningCapability, StructuredOutputCapability,
    };

    fn fixture_capability_envelope() -> ModelCapabilitySetV1 {
        ModelCapabilitySetV1 {
            taxonomy_version: MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
            input: ModelInputCapability::TextOnly,
            text_streaming: true,
            structured_output: StructuredOutputCapability::Unsupported,
            reasoning: ReasoningCapability::TextualReasoningV1,
            tool_exchange: false,
            context_preservation: ContextPreservationCapability::LocalDurableHistoryV1 {
                reasoning_input_contract: "reasoning-history-transfer-v1".to_owned(),
            },
        }
    }

    fn fixture_profile() -> ProviderProfileRevisionV1 {
        ProviderProfileRevisionV1 {
            profile_id: "profile-default".to_owned(),
            revision_id: "rev-0001".to_owned(),
            provider_kind_id: "responses".to_owned(),
            model_id: "gpt-4.1".to_owned(),
            endpoint: "https://api.openai.com/v1".to_owned(),
            credential_transport_mode: CredentialTransportMode::Bearer,
            safe_header_name: None,
            capability_taxonomy_revision: MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
            reasoning_compatibility_id: Some("reasoning-compat-v1".to_owned()),
            kind_descriptor_revision_id: "kind-descriptor-rev-0001".to_owned(),
            driver_contract_revision: ProviderDriverContractRevisionDto {
                driver_family: "responses".to_owned(),
                major: 1,
                minor: 0,
            },
        }
    }

    #[test]
    fn provider_profile_revision_round_trips_and_validates_closed_values() {
        let profile = fixture_profile();
        let bytes = profile.encode().expect("profile revision encodes");
        assert_eq!(
            ProviderProfileRevisionV1::decode(&bytes).expect("profile revision decodes"),
            profile
        );
        assert_eq!(
            ProviderProfileRevisionV1::decode(&bytes)
                .expect("profile revision decodes")
                .encode()
                .expect("profile revision re-encodes"),
            bytes
        );
        let mut bad_taxonomy = profile.clone();
        bad_taxonomy.capability_taxonomy_revision = "other-taxonomy-v1".to_owned();
        assert_eq!(
            bad_taxonomy
                .encode()
                .expect_err("unknown taxonomy revision is rejected")
                .code(),
            "provider_profile_revision_invalid"
        );
        let mut openai = profile.clone();
        openai.provider_kind_id = "openai".to_owned();
        assert_eq!(
            openai
                .encode()
                .expect_err("openai kind id is rejected")
                .code(),
            "invalid_provider_kind"
        );
        let mut fragment = profile;
        fragment.endpoint = "https://api.example.com/v1#fragment".to_owned();
        assert_eq!(
            fragment
                .encode()
                .expect_err("fragment endpoint is rejected")
                .code(),
            "invalid_endpoint"
        );
    }

    #[test]
    fn safe_header_name_is_a_name_only_and_transport_is_consistent() {
        let mut profile = fixture_profile();
        profile.credential_transport_mode = CredentialTransportMode::SafeHeader;
        assert_eq!(
            profile
                .encode()
                .expect_err("safe header transport requires a header name")
                .code(),
            "provider_profile_revision_invalid"
        );
        profile.safe_header_name = Some("X-Custom-Auth".to_owned());
        assert!(profile.encode().is_ok());
        profile.credential_transport_mode = CredentialTransportMode::Bearer;
        assert_eq!(
            profile
                .encode()
                .expect_err("bearer transport rejects a header name")
                .code(),
            "provider_profile_revision_invalid"
        );
        profile.credential_transport_mode = CredentialTransportMode::SafeHeader;
        profile.safe_header_name = Some("Authorization: Bearer sk-test".to_owned());
        assert_eq!(
            profile
                .encode()
                .expect_err("a credential-shaped header value is rejected")
                .code(),
            "credentials_forbidden"
        );
    }

    #[test]
    fn driver_contract_and_kind_descriptor_round_trip_and_are_strict() {
        let contract = ProviderDriverContractRevisionDto {
            driver_family: "responses".to_owned(),
            major: 1,
            minor: 0,
        };
        let bytes = contract.encode().expect("driver contract encodes");
        assert_eq!(
            ProviderDriverContractRevisionDto::decode(&bytes).expect("driver contract decodes"),
            contract
        );
        let descriptor = ProviderKindDescriptorRevisionV1 {
            kind_id: "responses".to_owned(),
            descriptor_family: "responses-descriptor".to_owned(),
            ordered_protocol_part_revisions: vec!["parts-v1".to_owned()],
            endpoint_policy: "https-only".to_owned(),
            credential_transport_contract: "bearer-or-safe-header".to_owned(),
            model_capability_envelope: fixture_capability_envelope(),
            driver_contract_family: "responses".to_owned(),
        };
        let bytes = descriptor.encode().expect("kind descriptor encodes");
        assert_eq!(
            ProviderKindDescriptorRevisionV1::decode(&bytes).expect("kind descriptor decodes"),
            descriptor
        );
        assert_eq!(
            validate_provider_kind_revision_immutability(&descriptor, &descriptor),
            Ok(())
        );
        let mut changed = descriptor.clone();
        changed.credential_transport_contract = "bearer-only".to_owned();
        assert_eq!(
            validate_provider_kind_revision_immutability(&descriptor, &changed)
                .expect_err("closed transport change is immutable")
                .code(),
            "provider_kind_immutable_mismatch"
        );
        let mut openai = descriptor;
        openai.kind_id = "openai".to_owned();
        assert_eq!(
            openai
                .encode()
                .expect_err("openai kind id is rejected")
                .code(),
            "invalid_provider_kind"
        );
    }

    #[test]
    fn kind_removal_and_tombstone_rules_are_enforced() {
        let profile = fixture_profile();
        assert_eq!(
            validate_provider_kind_removal("responses", std::slice::from_ref(&profile))
                .expect_err("removal with dependents is rejected")
                .code(),
            "provider_kind_has_dependents"
        );
        assert!(validate_provider_kind_removal("responses", &[]).is_ok());
        let tombstone =
            ProviderProfileTombstoneDto::new("profile-default", 2, 100, "removal-accepted")
                .expect("tombstone is valid");
        let bytes = tombstone.encode().expect("tombstone encodes");
        assert_eq!(
            ProviderProfileTombstoneDto::decode(&bytes).expect("tombstone decodes"),
            tombstone
        );
        assert_eq!(
            validate_profile_id_not_tombstoned("profile-default", std::slice::from_ref(&tombstone))
                .expect_err("tombstoned id cannot be reintroduced")
                .code(),
            "provider_profile_revision_invalid"
        );
        assert!(
            validate_profile_id_not_tombstoned("profile-other", std::slice::from_ref(&tombstone))
                .is_ok()
        );
        let kind_tombstone = ProviderKindTombstoneDto::new(
            "generic-chat-completion-api",
            2,
            101,
            "removal-accepted",
        )
        .expect("kind tombstone is valid");
        let bytes = kind_tombstone.encode().expect("kind tombstone encodes");
        assert_eq!(
            ProviderKindTombstoneDto::decode(&bytes).expect("kind tombstone decodes"),
            kind_tombstone
        );
    }

    #[test]
    fn catalog_limits_are_frozen_and_validated() {
        let frozen = ProviderCatalogLimits::frozen();
        assert_eq!(frozen.validate(), Ok(()));
        assert_eq!(frozen.max_profiles, 128);
        assert_eq!(frozen.max_kinds, 32);
        assert_eq!(frozen.max_candidate_bytes, 512 * 1024);
        assert_eq!(frozen.max_issues, 32);
        assert_eq!(
            ProviderCatalogLimits {
                max_profiles: 129,
                ..frozen
            }
            .validate()
            .expect_err("over-limit profile count is rejected"),
            CanonicalError::OverLimit
        );
        assert_eq!(
            ProviderCatalogLimits {
                max_candidate_bytes: 512 * 1024 + 1,
                ..frozen
            }
            .validate()
            .expect_err("over-limit candidate size is rejected"),
            CanonicalError::OverLimit
        );
    }

    #[test]
    fn endpoint_validation_rejects_userinfo_query_fragment_and_controls() {
        for (endpoint, expected) in [
            ("https://user:pass@api.example.com/v1", "invalid_endpoint"),
            ("https://api.example.com/v1?key=value", "invalid_endpoint"),
            ("https://api.example.com/v1#frag", "invalid_endpoint"),
            ("https://api.example.com/v1 ", "invalid_endpoint"),
            ("ftp://api.example.com/v1", "invalid_endpoint"),
            ("https://api.example.com/%zz", "invalid_endpoint"),
        ] {
            assert_eq!(
                validate_endpoint(endpoint)
                    .expect_err("invalid endpoint is rejected")
                    .code(),
                expected,
                "endpoint {endpoint}"
            );
        }
        assert!(validate_endpoint("https://api.example.com/v1").is_ok());
        assert!(validate_endpoint("http://127.0.0.1:8080/v1").is_ok());
        assert_eq!(
            validate_endpoint("https://sk-test@api.example.com/v1")
                .expect_err("credential-bearing endpoint is rejected")
                .code(),
            "invalid_endpoint"
        );
    }

    #[test]
    fn profile_and_kind_ids_are_limited_to_63_characters() {
        let profile = fixture_profile();
        let mut long = profile.clone();
        long.profile_id = "p".repeat(64);
        assert_eq!(
            long.encode()
                .expect_err("over-limit profile id is rejected")
                .code(),
            "provider_profile_revision_invalid"
        );
        let ok = profile.clone();
        let mut at_boundary = ok;
        at_boundary.profile_id = "p".repeat(63);
        assert!(at_boundary.encode().is_ok());
        let mut long_kind = profile;
        long_kind.provider_kind_id = "k".repeat(64);
        assert_eq!(
            long_kind
                .encode()
                .expect_err("over-limit kind id is rejected")
                .code(),
            "invalid_provider_kind"
        );
    }
}
