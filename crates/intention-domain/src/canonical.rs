//! Strict canonical record codec used by domain-owned historical identities.

use sha2::{Digest as Sha2Digest, Sha256};
use std::{fmt, str::FromStr};

/// Maximum encoded size of one canonical record field value.
pub const MAX_FIELD_BYTES: usize = 16 * 1024 * 1024;

/// Maximum total encoded size of one canonical record.
pub const MAX_RECORD_BYTES: usize = 64 * 1024 * 1024;

/// The wire types of the canonical typed-TLV framing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum WireType {
    U64 = 1,
    Bool = 2,
    Utf8 = 3,
    Uuid = 4,
    Digest = 5,
    Bytes = 6,
    Record = 7,
    List = 8,
    Optional = 9,
}

impl TryFrom<u8> for WireType {
    type Error = CanonicalError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::U64),
            2 => Ok(Self::Bool),
            3 => Ok(Self::Utf8),
            4 => Ok(Self::Uuid),
            5 => Ok(Self::Digest),
            6 => Ok(Self::Bytes),
            7 => Ok(Self::Record),
            8 => Ok(Self::List),
            9 => Ok(Self::Optional),
            _ => Err(CanonicalError::InvalidWireType),
        }
    }
}

/// A strict canonical codec failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CanonicalError {
    Truncated,
    InvalidMagic,
    InvalidVersion,
    InvalidTag,
    InvalidField,
    InvalidWireType,
    InvalidUtf8,
    InvalidBool,
    InvalidOptional,
    DuplicateOrDescendingField,
    UnknownField(u32),
    TrailingBytes,
    OverLimit,
    DigestMismatch,
    InvalidDigest,
    /// A provider kind identifier violates the closed kind policy.
    InvalidProviderKind,
    /// An endpoint violates the closed endpoint policy.
    InvalidEndpoint,
    /// A provider profile, kind descriptor, or capability record is invalid.
    ProviderProfileRevisionInvalid,
    /// A context source manifest violates its closed bounds.
    ContextSourceManifestInvalid,
    /// A model context projection record is semantically invalid.
    ModelContextProjectionInvalid,
    /// A model context projection exceeds its aggregate byte bound.
    ModelContextProjectionTooLarge,
    /// A credential-shaped value reached a field that must never carry secrets.
    CredentialsForbidden,
    /// A provider kind revision changes immutable kind identity or closed parts.
    ProviderKindImmutableMismatch,
    /// A provider kind still has dependent profiles and cannot be removed.
    ProviderKindHasDependents,
    /// Required reasoning history material is missing or corrupt.
    ReasoningHistoryUnavailable,
    /// Required reasoning history is incompatible with the transfer policy.
    ReasoningHistoryIncompatible,
    /// Reasoning history exceeds its aggregate or entry bound.
    ReasoningHistoryTooLarge,
    /// A reasoning fragment would exceed the fixed per-run output bound.
    ReasoningOutputLimitExceeded,
    /// A provider reasoning stream value violates the closed stream rules.
    ProviderReasoningStreamInvalid,
}

impl CanonicalError {
    /// The stable machine-readable snake_case code for this error.
    ///
    /// The code is a fixed string that never embeds input-derived values, so
    /// it is safe for logs and machine consumption.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Truncated => "truncated",
            Self::InvalidMagic => "invalid_magic",
            Self::InvalidVersion => "invalid_version",
            Self::InvalidTag => "invalid_tag",
            Self::InvalidField => "invalid_field",
            Self::InvalidWireType => "invalid_wire_type",
            Self::InvalidUtf8 => "invalid_utf8",
            Self::InvalidBool => "invalid_bool",
            Self::InvalidOptional => "invalid_optional",
            Self::DuplicateOrDescendingField => "duplicate_or_descending_field",
            Self::UnknownField(_) => "unknown_field",
            Self::TrailingBytes => "trailing_bytes",
            Self::OverLimit => "over_limit",
            Self::DigestMismatch => "digest_mismatch",
            Self::InvalidDigest => "invalid_digest",
            Self::InvalidProviderKind => "invalid_provider_kind",
            Self::InvalidEndpoint => "invalid_endpoint",
            Self::ProviderProfileRevisionInvalid => "provider_profile_revision_invalid",
            Self::ContextSourceManifestInvalid => "context_source_manifest_invalid",
            Self::ModelContextProjectionInvalid => "model_context_projection_invalid",
            Self::ModelContextProjectionTooLarge => "model_context_projection_too_large",
            Self::CredentialsForbidden => "credentials_forbidden",
            Self::ProviderKindImmutableMismatch => "provider_kind_immutable_mismatch",
            Self::ProviderKindHasDependents => "provider_kind_has_dependents",
            Self::ReasoningHistoryUnavailable => "reasoning_history_unavailable",
            Self::ReasoningHistoryIncompatible => "reasoning_history_incompatible",
            Self::ReasoningHistoryTooLarge => "reasoning_history_too_large",
            Self::ReasoningOutputLimitExceeded => "reasoning_output_limit_exceeded",
            Self::ProviderReasoningStreamInvalid => "provider_reasoning_stream_invalid",
        }
    }
}

impl fmt::Display for CanonicalError {
    /// Renders the stable [`CanonicalError::code`]; attacker-controlled raw
    /// values such as the field number in `UnknownField` are never printed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for CanonicalError {}

/// The SHA-256 digest of exact canonical bytes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Digest256([u8; 32]);

impl Digest256 {
    /// Creates a digest from exactly thirty-two raw bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidDigest` when `bytes` is not exactly
    /// thirty-two bytes long.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CanonicalError> {
        bytes
            .try_into()
            .map(Self)
            .map_err(|_| CanonicalError::InvalidDigest)
    }

    /// Computes the SHA-256 digest of `bytes`.
    #[must_use]
    pub fn sha256(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// Parses exactly sixty-four lowercase hex digits into a digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidDigest` when `value` is not exactly
    /// sixty-four lowercase hex digits.
    pub fn from_str_hex(value: &str) -> Result<Self, CanonicalError> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(CanonicalError::InvalidDigest);
        }
        let mut bytes = [0; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| CanonicalError::InvalidDigest)?;
        }
        Ok(Self(bytes))
    }

    /// Returns the thirty-two raw digest bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }

    /// Binds the SHA-256 digest of `bytes` to one validated namespace.
    ///
    /// The namespace must be non-empty ASCII letters, digits, `-`, `_`, or
    /// `.`.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidDigest` when `namespace` is empty or
    /// contains a character outside the allowed set.
    pub fn for_namespace(
        namespace: &str,
        bytes: &[u8],
    ) -> Result<NamespacedDigest, CanonicalError> {
        if namespace.is_empty()
            || !namespace
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(CanonicalError::InvalidDigest);
        }
        Ok(NamespacedDigest {
            namespace: namespace.to_owned(),
            digest: Self::sha256(bytes),
        })
    }
}

impl fmt::Display for Digest256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sha256:{}", hex(&self.0))
    }
}

impl FromStr for Digest256 {
    type Err = CanonicalError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let digits = value
            .strip_prefix("sha256:")
            .ok_or(CanonicalError::InvalidDigest)?;
        if digits.len() != 64
            || !digits
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(CanonicalError::InvalidDigest);
        }
        let mut bytes = [0; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&digits[index * 2..index * 2 + 2], 16)
                .map_err(|_| CanonicalError::InvalidDigest)?;
        }
        Ok(Self(bytes))
    }
}

/// A SHA-256 digest bound to one namespace.
///
/// The text form is `<namespace>:sha256:<64 lowercase hex>`; the namespace
/// grammar is the same as [`Digest256::for_namespace`]: non-empty ASCII
/// letters, digits, `-`, `_`, or `.`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NamespacedDigest {
    /// The digest namespace.
    pub namespace: String,
    /// The SHA-256 digest bytes.
    pub digest: Digest256,
}

impl fmt::Display for NamespacedDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.digest)
    }
}

impl FromStr for NamespacedDigest {
    type Err = CanonicalError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (namespace, digits) = value
            .split_once(":sha256:")
            .ok_or(CanonicalError::InvalidDigest)?;
        if namespace.is_empty()
            || !namespace
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(CanonicalError::InvalidDigest);
        }
        let digest = Digest256::from_str(&format!("sha256:{digits}"))?;
        Ok(Self {
            namespace: namespace.to_owned(),
            digest,
        })
    }
}

/// The sha256-v1 identity of canonical identity-bearing bytes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IdentityV1([u8; 32]);

impl IdentityV1 {
    /// Computes the sha256-v1 identity of `bytes`.
    #[must_use]
    pub fn sha256(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// Returns the thirty-two raw identity bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
    }
}

impl fmt::Display for IdentityV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sha256-v1:{}", hex(&self.0))
    }
}

impl FromStr for IdentityV1 {
    type Err = CanonicalError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let digits = value
            .strip_prefix("sha256-v1:")
            .ok_or(CanonicalError::InvalidDigest)?;
        let digest = Digest256::from_str(&format!("sha256:{digits}"))?;
        Ok(Self(digest.bytes()))
    }
}

/// Builds the canonical identity input over only identity-bearing fields.
///
/// The identity is computed over exactly the fields added through
/// [`Self::field`]. The digest field itself is excluded by construction:
/// [`Self::digest`] always recomputes the identity from the encoded input and
/// never accepts a stored digest. Credentials, filesystem paths,
/// display/presentation data, readiness, and current state are not
/// identity-bearing: values supplied through the `with_*` setters are retained
/// for provenance only and are never encoded or digested.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CanonicalIdentityInput {
    fields: Vec<(u32, WireType, Vec<u8>)>,
    excluded_credentials: Option<Vec<u8>>,
    excluded_filesystem_path: Option<String>,
    excluded_display_data: Option<String>,
    excluded_readiness: Option<bool>,
    excluded_current_state: Option<Vec<u8>>,
}

impl CanonicalIdentityInput {
    /// Creates an empty identity input.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            fields: Vec::new(),
            excluded_credentials: None,
            excluded_filesystem_path: None,
            excluded_display_data: None,
            excluded_readiness: None,
            excluded_current_state: None,
        }
    }

    /// Adds one identity-bearing field with a positive, strictly increasing
    /// field number.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` when the field
    /// number is zero or does not strictly increase.
    pub fn field(
        mut self,
        number: u32,
        wire_type: WireType,
        value: Vec<u8>,
    ) -> Result<Self, CanonicalError> {
        if number == 0
            || self
                .fields
                .last()
                .is_some_and(|(last, _, _)| number <= *last)
        {
            return Err(CanonicalError::DuplicateOrDescendingField);
        }
        self.fields.push((number, wire_type, value));
        Ok(self)
    }

    /// Retains a credential value for provenance only; never encoded or
    /// digested.
    #[must_use]
    pub fn with_credentials(mut self, value: Vec<u8>) -> Self {
        self.excluded_credentials = Some(value);
        self
    }

    /// Retains a filesystem path for provenance only; never encoded or
    /// digested.
    #[must_use]
    pub fn with_filesystem_path(mut self, value: impl Into<String>) -> Self {
        self.excluded_filesystem_path = Some(value.into());
        self
    }

    /// Retains display/presentation data for provenance only; never encoded
    /// or digested.
    #[must_use]
    pub fn with_display_data(mut self, value: impl Into<String>) -> Self {
        self.excluded_display_data = Some(value.into());
        self
    }

    /// Retains a readiness value for provenance only; never encoded or
    /// digested.
    #[must_use]
    pub const fn with_readiness(mut self, value: bool) -> Self {
        self.excluded_readiness = Some(value);
        self
    }

    /// Retains current state for provenance only; never encoded or digested.
    #[must_use]
    pub fn with_current_state(mut self, value: Vec<u8>) -> Self {
        self.excluded_current_state = Some(value);
        self
    }

    /// Encodes the identity-bearing fields into their canonical record bytes.
    ///
    /// The input is framed as an anonymous record (tag zero, version one) so
    /// the identity is a digest of the same `IRCR`/`typed-tlv-v1` bytes used
    /// by every other canonical record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` or
    /// `CanonicalError::OverLimit` only if the field stream were noncanonical
    /// or a field or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        let mut builder = CanonicalRecordBuilder::new(0, 1);
        for (number, wire_type, value) in &self.fields {
            builder = builder.field(*number, *wire_type, value.clone())?;
        }
        builder.finish()
    }

    /// Computes the sha256-v1 identity of the encoded identity-bearing fields.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` or
    /// `CanonicalError::OverLimit` only if the field stream were noncanonical
    /// or a field or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn digest(&self) -> Result<IdentityV1, CanonicalError> {
        Ok(IdentityV1::sha256(&self.encode()?))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Builds a canonical record with strictly increasing field numbers.
pub struct CanonicalRecordBuilder {
    tag: u32,
    version: u32,
    fields: Vec<(u32, WireType, Vec<u8>)>,
    encoded_len: usize,
}

impl CanonicalRecordBuilder {
    /// Creates a builder for one record tag and version.
    #[must_use]
    pub const fn new(tag: u32, version: u32) -> Self {
        Self {
            tag,
            version,
            fields: Vec::new(),
            // Four magic bytes plus three four-byte header words.
            encoded_len: 16,
        }
    }

    /// Adds one field with a positive, strictly increasing field number.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` when the field
    /// number is zero or does not strictly increase, and
    /// `CanonicalError::OverLimit` when the field value exceeds
    /// `MAX_FIELD_BYTES` or the accumulated record size would exceed
    /// `MAX_RECORD_BYTES`.
    pub fn field(
        mut self,
        number: u32,
        wire_type: WireType,
        value: Vec<u8>,
    ) -> Result<Self, CanonicalError> {
        if number == 0
            || self
                .fields
                .last()
                .is_some_and(|(last, _, _)| number <= *last)
        {
            return Err(CanonicalError::DuplicateOrDescendingField);
        }
        if value.len() > MAX_FIELD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        let value_len = value.len();
        if self.encoded_len + 9 + value_len > MAX_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        self.encoded_len += 9 + value_len;
        self.fields.push((number, wire_type, value));
        Ok(self)
    }

    /// Finishes the record into its canonical framed bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::OverLimit` only if a field length could not
    /// fit the wire `u32`; that is impossible by construction because
    /// [`Self::field`] rejects values over `MAX_FIELD_BYTES`, far below
    /// `u32::MAX`.
    pub fn finish(self) -> Result<Vec<u8>, CanonicalError> {
        let mut out = Vec::new();
        out.extend_from_slice(b"IRCR");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&self.tag.to_be_bytes());
        out.extend_from_slice(&self.version.to_be_bytes());
        for (number, wire_type, value) in self.fields {
            out.extend_from_slice(&number.to_be_bytes());
            out.push(wire_type as u8);
            let length = u32::try_from(value.len()).map_err(|_| CanonicalError::OverLimit)?;
            out.extend_from_slice(&length.to_be_bytes());
            out.extend(value);
        }
        Ok(out)
    }
}

/// Parses and validates one canonical record frame.
pub struct CanonicalRecordReader<'a> {
    /// The record family tag.
    pub tag: u32,
    /// The field-table version.
    pub version: u32,
    fields: Vec<(u32, WireType, &'a [u8])>,
}

impl<'a> CanonicalRecordReader<'a> {
    /// Parses and validates one canonical record frame.
    ///
    /// `max_field` is the largest field number of the record's fixed field
    /// table; any field number beyond it is rejected as unknown. Scalar forms
    /// are validated by wire type: booleans must be exactly one `0x00`/`0x01`
    /// byte, optional values must carry a one-byte presence marker, and UTF-8
    /// values must be valid UTF-8.
    ///
    /// # Errors
    ///
    /// Returns a `CanonicalError` for malformed or noncanonical framing:
    /// truncated, over-limit, or trailing bytes; bad magic or version; unknown
    /// wire types; zero, duplicate, descending, or unknown field numbers; and
    /// invalid bool, optional-marker, or UTF-8 scalar forms.
    pub fn new(bytes: &'a [u8], max_field: u32) -> Result<Self, CanonicalError> {
        if bytes.len() < 16 {
            return Err(CanonicalError::Truncated);
        }
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        if &bytes[..4] != b"IRCR" {
            return Err(CanonicalError::InvalidMagic);
        }
        if read_u32(bytes, 4) != 1 {
            return Err(CanonicalError::InvalidVersion);
        }
        let tag = read_u32(bytes, 8);
        let version = read_u32(bytes, 12);
        let mut position = 16;
        let mut last = 0;
        let mut fields = Vec::new();
        while position < bytes.len() {
            if bytes.len() - position < 9 {
                return Err(CanonicalError::TrailingBytes);
            }
            let number = read_u32(bytes, position);
            if number == 0 || number <= last {
                return Err(CanonicalError::DuplicateOrDescendingField);
            }
            if number > max_field {
                return Err(CanonicalError::UnknownField(number));
            }
            let wire_type = WireType::try_from(bytes[position + 4])?;
            let length = read_u32(bytes, position + 5) as usize;
            position += 9;
            if length > MAX_FIELD_BYTES {
                return Err(CanonicalError::OverLimit);
            }
            if length > bytes.len() - position {
                return Err(CanonicalError::Truncated);
            }
            let value = &bytes[position..position + length];
            validate_scalar_form(wire_type, value)?;
            fields.push((number, wire_type, value));
            position += length;
            last = number;
        }
        Ok(Self {
            tag,
            version,
            fields,
        })
    }

    /// Looks up one field by its table number and expected wire type.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` when the field exists with a
    /// different wire type than `wire_type`.
    pub fn field(
        &self,
        number: u32,
        wire_type: WireType,
    ) -> Result<Option<&'a [u8]>, CanonicalError> {
        match self
            .fields
            .iter()
            .find(|(existing, _, _)| *existing == number)
        {
            None => Ok(None),
            Some((_, existing_type, value)) => {
                if *existing_type == wire_type {
                    Ok(Some(value))
                } else {
                    Err(CanonicalError::InvalidField)
                }
            }
        }
    }
}

/// Validates the scalar form of one field value by its wire type.
fn validate_scalar_form(wire_type: WireType, value: &[u8]) -> Result<(), CanonicalError> {
    match wire_type {
        WireType::Bool => match value {
            [0] | [1] => Ok(()),
            _ => Err(CanonicalError::InvalidBool),
        },
        WireType::Optional => match value.first() {
            Some(0) | Some(1) => Ok(()),
            _ => Err(CanonicalError::InvalidOptional),
        },
        WireType::Utf8 => std::str::from_utf8(value)
            .map(|_| ())
            .map_err(|_| CanonicalError::InvalidUtf8),
        _ => Ok(()),
    }
}

/// Reads a big-endian `u32` at `offset`; callers must verify the bounds first.
fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// Encodes an unsigned value in minimal big-endian bytes.
#[must_use]
pub fn encode_u64(value: u64) -> Vec<u8> {
    let mut bytes = value.to_be_bytes().to_vec();
    while bytes.len() > 1 && bytes[0] == 0 {
        bytes.remove(0);
    }
    bytes
}

/// Decodes minimal big-endian unsigned value bytes.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidField` for empty, over-long, or
/// non-minimal encodings.
pub fn decode_u64(bytes: &[u8]) -> Result<u64, CanonicalError> {
    if bytes.is_empty() || bytes.len() > 8 || (bytes.len() > 1 && bytes[0] == 0) {
        return Err(CanonicalError::InvalidField);
    }
    let mut value = [0; 8];
    value[8 - bytes.len()..].copy_from_slice(bytes);
    Ok(u64::from_be_bytes(value))
}

/// Encodes a boolean as one `0x00`/`0x01` byte.
#[must_use]
pub fn encode_bool(value: bool) -> Vec<u8> {
    vec![value as u8]
}

/// Decodes a one-byte boolean value.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidBool` unless `bytes` is exactly one
/// `0x00` or `0x01` byte.
pub fn decode_bool(bytes: &[u8]) -> Result<bool, CanonicalError> {
    match bytes {
        [0] => Ok(false),
        [1] => Ok(true),
        _ => Err(CanonicalError::InvalidBool),
    }
}

/// Encodes text as UTF-8 bytes.
#[must_use]
pub fn encode_utf8(value: &str) -> Vec<u8> {
    value.as_bytes().to_vec()
}

/// Decodes UTF-8 text bytes without trimming or normalization.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidUtf8` when `bytes` is not valid UTF-8.
pub fn decode_utf8(bytes: &[u8]) -> Result<&str, CanonicalError> {
    std::str::from_utf8(bytes).map_err(|_| CanonicalError::InvalidUtf8)
}

/// Maximum items in one decoded UUID list.
///
/// The cap is `MAX_FIELD_BYTES / 16`, the largest number of sixteen-byte
/// items that can fit inside one field value, so no valid encoding is ever
/// rejected. It bounds the allocation a hostile declared count could drive.
pub const MAX_UUID_LIST_ITEMS: usize = MAX_FIELD_BYTES / 16;

/// Decodes a strict list of sixteen-byte UUIDs.
///
/// A list is a big-endian `u32` item count followed by exactly that many
/// sixteen-byte UUIDs; the count must fit the remaining bytes exactly.
///
/// # Errors
///
/// Returns `CanonicalError::OverLimit` when the declared count exceeds
/// `MAX_UUID_LIST_ITEMS`, `CanonicalError::Truncated` when the count or an
/// item does not fit the remaining bytes, and
/// `CanonicalError::TrailingBytes` when bytes remain after the declared
/// items.
pub fn decode_uuid_list(bytes: &[u8]) -> Result<Vec<[u8; 16]>, CanonicalError> {
    if bytes.len() < 4 {
        return Err(CanonicalError::Truncated);
    }
    let count = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if count > MAX_UUID_LIST_ITEMS {
        return Err(CanonicalError::OverLimit);
    }
    let expected = 4u64 + (count as u64) * 16;
    if expected > bytes.len() as u64 {
        return Err(CanonicalError::Truncated);
    }
    if expected < bytes.len() as u64 {
        return Err(CanonicalError::TrailingBytes);
    }
    let mut items = Vec::with_capacity(count);
    for index in 0..count {
        let start = 4 + index * 16;
        items.push(
            bytes[start..start + 16]
                .try_into()
                .map_err(|_| CanonicalError::Truncated)?,
        );
    }
    Ok(items)
}

/// Maximum items in one decoded length-prefixed list.
///
/// Every item carries at least a four-byte length prefix, so the cap is
/// `MAX_FIELD_BYTES / 4`, the largest number of items that can fit inside one
/// field value. It bounds the allocation a hostile declared count could drive.
pub const MAX_LIST_ITEMS: usize = MAX_FIELD_BYTES / 4;

/// Encodes items into a strict length-prefixed list.
///
/// A list is a big-endian `u32` item count followed by exactly that many
/// length-prefixed items; the count is the item count, never a byte total.
#[must_use]
pub fn encode_list_items(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(items.len() as u32).to_be_bytes());
    for item in items {
        out.extend_from_slice(&(item.len() as u32).to_be_bytes());
        out.extend_from_slice(item);
    }
    out
}

/// Decodes a strict length-prefixed list of opaque items.
///
/// # Errors
///
/// Returns `CanonicalError::OverLimit` when the declared count exceeds
/// `MAX_LIST_ITEMS`, `CanonicalError::Truncated` when the count or an item
/// does not fit the remaining bytes, and `CanonicalError::TrailingBytes`
/// when bytes remain after the declared items.
pub fn decode_list_items(bytes: &[u8]) -> Result<Vec<Vec<u8>>, CanonicalError> {
    if bytes.len() < 4 {
        return Err(CanonicalError::Truncated);
    }
    let count = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if count > MAX_LIST_ITEMS {
        return Err(CanonicalError::OverLimit);
    }
    let mut position = 4usize;
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.len() - position < 4 {
            return Err(CanonicalError::Truncated);
        }
        let length = u32::from_be_bytes([
            bytes[position],
            bytes[position + 1],
            bytes[position + 2],
            bytes[position + 3],
        ]) as usize;
        position += 4;
        if length > bytes.len() - position {
            return Err(CanonicalError::Truncated);
        }
        items.push(bytes[position..position + length].to_vec());
        position += length;
    }
    if position != bytes.len() {
        return Err(CanonicalError::TrailingBytes);
    }
    Ok(items)
}

/// Encodes a strict list of UTF-8 strings.
#[must_use]
pub fn encode_utf8_list(items: &[String]) -> Vec<u8> {
    encode_list_items(
        &items
            .iter()
            .map(|item| item.as_bytes().to_vec())
            .collect::<Vec<_>>(),
    )
}

/// Decodes a strict list of UTF-8 strings.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidUtf8` when any item is not valid UTF-8,
/// and the list framing errors of [`decode_list_items`] otherwise.
pub fn decode_utf8_list(bytes: &[u8]) -> Result<Vec<String>, CanonicalError> {
    decode_list_items(bytes)?
        .into_iter()
        .map(|item| Ok(decode_utf8(&item)?.to_owned()))
        .collect()
}

/// Encodes an optional string as a one-byte presence marker followed by the
/// UTF-8 value when present.
#[must_use]
pub fn encode_optional_utf8(value: &Option<String>) -> Vec<u8> {
    value.as_ref().map_or_else(
        || vec![0],
        |text| {
            let mut bytes = vec![1];
            bytes.extend_from_slice(text.as_bytes());
            bytes
        },
    )
}

/// Decodes an optional string from its presence-marker framing.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidOptional` when the presence marker is
/// missing or a closed marker carries payload bytes, and
/// `CanonicalError::InvalidUtf8` when an open value is not valid UTF-8.
pub fn decode_optional_utf8(bytes: &[u8]) -> Result<Option<String>, CanonicalError> {
    let Some((&marker, rest)) = bytes.split_first() else {
        return Err(CanonicalError::InvalidOptional);
    };
    match marker {
        0 if rest.is_empty() => Ok(None),
        1 => Ok(Some(decode_utf8(rest)?.to_owned())),
        _ => Err(CanonicalError::InvalidOptional),
    }
}

/// Whether `value` carries a credential-shaped token that must never enter a
/// canonical record, digest, or durable identity.
#[must_use]
pub fn contains_credential_shape(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("sk-")
        || lower.starts_with("bearer ")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("token=")
        || lower.contains("key=")
        || lower.contains("auth=")
}

/// Whether `value` contains a control character or a NUL byte.
///
/// Control characters and NUL bytes are rejected before canonical encoding so
/// they can never reach record bytes, digests, or logs.
#[must_use]
pub fn contains_control_or_nul(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| byte == 0 || byte.is_ascii_control())
}

/// Whether a ledger tag has a production codec in this slice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TagStatus {
    /// The tag has a production codec in this PR.
    Wired,
    /// The tag is reserved for Slice 3 with no production codec yet.
    ReservedForSlice3,
    /// The tag is reserved for Slice 4 with no production codec yet.
    ReservedForSlice4,
}

/// One ADR 0036 ledger registry entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LedgerTag {
    /// The ledger name, including version aliases where applicable.
    pub name: &'static str,
    /// The frozen numeric tag value.
    pub value: u32,
    /// Whether a production codec exists in this slice.
    pub status: TagStatus,
}

/// The domain-owned numeric tag registry.
pub struct TagRegistry;

impl TagRegistry {
    pub const RUN_EXECUTION_MEANING: u32 = 0x0101;
    pub const PROGRAMMATIC_CALLER_POLICY_SELECTION_V1: u32 = 0x0201;
    pub const AGENT_ACTIVITY_SELECTION_V1: u32 = 0x0202;
    pub const GOAL_RUN_SELECTION_V1: u32 = 0x0203;
    pub const CONTINUAL_HARNESS_SELECTION_V1: u32 = 0x0204;
    pub const MCP_METHOD_CATALOG_SELECTION_V1: u32 = 0x0205;
    pub const MODEL_CAPABILITY_TAXONOMY_V1: u32 = 0x0206;
    pub const PROVIDER_PROFILE_REVISION_V1: u32 = 0x0207;
    pub const PROVIDER_SELECTION_V1: u32 = 0x0208;
    pub const REASONING_HISTORY_MANIFEST_V1: u32 = 0x0209;
    pub const CONTEXT_SOURCE_MANIFEST_V1: u32 = 0x020A;
    pub const MODEL_CONTEXT_PROJECTION_V1: u32 = 0x020B;
    pub const TOOL_DESCRIPTOR_REVISION: u32 = 0x0301;
    pub const TOOL_REGISTRY_REVISION: u32 = 0x0302;
    pub const MODEL_TOOL_LOOP_V1: u32 = 0x0303;
    pub const BRIDGE_INVOCATION_V1: u32 = 0x0304;
    pub const FORK_BASE_SNAPSHOT_V1: u32 = 0x0401;
    pub const FORK_PREVIEW_V1: u32 = 0x0402;
    pub const FORK_COMMAND_V1: u32 = 0x0403;
    pub const AGENT_ACTIVITY_TREE_V1: u32 = 0x0501;
    pub const AGENT_ACTIVITY_PAIR_V1: u32 = 0x0502;
    pub const AGENT_MESSAGE_V1: u32 = 0x0503;
    pub const AGENT_ACTIVITY_JOURNAL_RECORD_V1: u32 = 0x0504;
    pub const AGENT_NOTIFICATION_RECORD_V1: u32 = 0x0505;

    /// The complete ADR 0036 ledger tag table, one entry per ledger tag.
    ///
    /// The fork aliases `fork-base-snapshot-v1/v2` and `fork-preview-v1/v2`
    /// each occupy a single entry that covers both versions.
    pub const LEDGER: &'static [LedgerTag] = &[
        LedgerTag {
            name: "run-execution-meaning",
            value: 0x0101,
            status: TagStatus::Wired,
        },
        LedgerTag {
            name: "programmatic-caller-policy-selection-v1",
            value: 0x0201,
            status: TagStatus::Wired,
        },
        LedgerTag {
            name: "agent-activity-selection-v1",
            value: 0x0202,
            status: TagStatus::Wired,
        },
        LedgerTag {
            name: "goal-run-selection-v1",
            value: 0x0203,
            status: TagStatus::ReservedForSlice3,
        },
        LedgerTag {
            name: "continual-harness-selection-v1",
            value: 0x0204,
            status: TagStatus::ReservedForSlice3,
        },
        LedgerTag {
            name: "mcp-method-catalog-selection-v1",
            value: 0x0205,
            status: TagStatus::ReservedForSlice3,
        },
        LedgerTag {
            name: "model-capability-taxonomy-v1",
            value: 0x0206,
            status: TagStatus::Wired,
        },
        LedgerTag {
            name: "provider-profile-revision-v1",
            value: 0x0207,
            status: TagStatus::Wired,
        },
        LedgerTag {
            name: "provider-selection-v1",
            value: 0x0208,
            status: TagStatus::Wired,
        },
        LedgerTag {
            name: "reasoning-history-manifest-v1",
            value: 0x0209,
            status: TagStatus::Wired,
        },
        LedgerTag {
            name: "context-source-manifest-v1",
            value: 0x020A,
            status: TagStatus::Wired,
        },
        LedgerTag {
            name: "model-context-projection-v1",
            value: 0x020B,
            status: TagStatus::Wired,
        },
        LedgerTag {
            name: "tool-descriptor-revision",
            value: 0x0301,
            status: TagStatus::ReservedForSlice3,
        },
        LedgerTag {
            name: "tool-registry-revision",
            value: 0x0302,
            status: TagStatus::ReservedForSlice3,
        },
        LedgerTag {
            name: "model-tool-loop-v1",
            value: 0x0303,
            status: TagStatus::ReservedForSlice3,
        },
        LedgerTag {
            name: "bridge-invocation-v1",
            value: 0x0304,
            status: TagStatus::ReservedForSlice3,
        },
        LedgerTag {
            name: "fork-base-snapshot-v1/v2",
            value: 0x0401,
            status: TagStatus::ReservedForSlice4,
        },
        LedgerTag {
            name: "fork-preview-v1/v2",
            value: 0x0402,
            status: TagStatus::ReservedForSlice4,
        },
        LedgerTag {
            name: "fork-command-v1",
            value: 0x0403,
            status: TagStatus::ReservedForSlice4,
        },
        LedgerTag {
            name: "agent-activity-tree-v1",
            value: 0x0501,
            status: TagStatus::ReservedForSlice4,
        },
        LedgerTag {
            name: "agent-activity-pair-v1",
            value: 0x0502,
            status: TagStatus::ReservedForSlice4,
        },
        LedgerTag {
            name: "agent-message-v1",
            value: 0x0503,
            status: TagStatus::ReservedForSlice4,
        },
        LedgerTag {
            name: "agent-activity-journal-record-v1",
            value: 0x0504,
            status: TagStatus::ReservedForSlice4,
        },
        LedgerTag {
            name: "agent-notification-record-v1",
            value: 0x0505,
            status: TagStatus::ReservedForSlice4,
        },
    ];
}
