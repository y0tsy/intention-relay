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
}

impl fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
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

    /// Returns the thirty-two raw digest bytes.
    #[must_use]
    pub const fn bytes(self) -> [u8; 32] {
        self.0
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Builds a canonical record with strictly increasing field numbers.
pub struct CanonicalRecordBuilder {
    tag: u32,
    version: u32,
    fields: Vec<(u32, WireType, Vec<u8>)>,
}

impl CanonicalRecordBuilder {
    /// Creates a builder for one record tag and version.
    #[must_use]
    pub const fn new(tag: u32, version: u32) -> Self {
        Self {
            tag,
            version,
            fields: Vec::new(),
        }
    }

    /// Adds one field with a positive, strictly increasing field number.
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

    /// Finishes the record into its canonical framed bytes.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"IRCR");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&self.tag.to_be_bytes());
        out.extend_from_slice(&self.version.to_be_bytes());
        for (number, wire_type, value) in self.fields {
            out.extend_from_slice(&number.to_be_bytes());
            out.push(wire_type as u8);
            out.extend_from_slice(&(value.len() as u32).to_be_bytes());
            out.extend(value);
        }
        out
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
    pub const LEGACY_M4_SELECTION_BINDING: u32 = 0x020C;
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
}
