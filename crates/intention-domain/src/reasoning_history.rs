//! Normalized reasoning deltas, summaries, and the typed cross-turn reasoning
//! history manifest.
//!
//! Reasoning manifests are immutable references to completed compatible source
//! responses; they never carry reasoning text, credentials, or current state.

use crate::canonical::{
    CanonicalError, CanonicalIdentityInput, CanonicalRecordBuilder, CanonicalRecordReader,
    Digest256, NamespacedDigest, TagRegistry, WireType, contains_control_or_nul, decode_utf8,
    decode_utf8_list, encode_u64, encode_utf8, encode_utf8_list,
};

/// The fixed combined canonical reasoning output/history bound of one run.
pub const MAX_REASONING_AGGREGATE_BYTES: u64 = 4 * 1024 * 1024;

/// The closed textual reasoning dialect values a descriptor may declare.
pub const REASONING_DIALECT_VALUES: [&str; 8] = [
    "reasoning_content",
    "reasoning",
    "reasoning_details[].text",
    "reasoning_details[].message.thinking",
    "thinking",
    "reasoning_effort",
    "thinking_budget",
    "thinking_token_budget",
];

/// The closed reasoning fragment category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReasoningDeltaCategory {
    /// The main textual reasoning representation.
    Primary,
    /// A separate detailed reasoning representation.
    Detail,
}

/// One normalized reasoning fragment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReasoningDeltaDto {
    pub category: ReasoningDeltaCategory,
    pub content: String,
}

impl ReasoningDeltaDto {
    /// Validates one normalized reasoning fragment.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ProviderReasoningStreamInvalid` when the
    /// content is blank or carries control characters.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        validate_reasoning_content(&self.content)
    }
}

/// One normalized reasoning summary, distinct from reasoning and never raw
/// chain-of-thought.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReasoningSummaryDeltaDto {
    pub content: String,
}

impl ReasoningSummaryDeltaDto {
    /// Validates one normalized reasoning summary.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ProviderReasoningStreamInvalid` when the
    /// content is blank or carries control characters.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        validate_reasoning_content(&self.content)
    }
}

/// The closed per-run reasoning history bounds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReasoningHistoryBound {
    pub max_entries: u32,
    pub max_aggregate_bytes: u64,
}

impl ReasoningHistoryBound {
    /// Validates that the bound is usable.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ReasoningHistoryTooLarge` when either bound
    /// is zero.
    pub const fn validate(&self) -> Result<(), CanonicalError> {
        if self.max_entries == 0 || self.max_aggregate_bytes == 0 {
            return Err(CanonicalError::ReasoningHistoryTooLarge);
        }
        Ok(())
    }

    /// Encodes this bound into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ReasoningHistoryTooLarge` when either bound
    /// is zero, and `CanonicalError::DuplicateOrDescendingField` or
    /// `CanonicalError::OverLimit` only if the fixed field table were
    /// noncanonical or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        record(
            0,
            1,
            vec![
                (1, WireType::U64, encode_u64(self.max_entries as u64)),
                (2, WireType::U64, encode_u64(self.max_aggregate_bytes)),
            ],
        )
    }

    /// Decodes this bound from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the nested record is not the
    /// anonymous tag-zero version-one frame, `CanonicalError::InvalidField`
    /// when either field is absent or malformed,
    /// `CanonicalError::ReasoningHistoryTooLarge` when either bound is zero,
    /// and other `CanonicalError` values for malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 2)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let bound = Self {
            max_entries: u32::try_from(decode_u64_field(&reader, 1)?)
                .map_err(|_| CanonicalError::InvalidField)?,
            max_aggregate_bytes: decode_u64_field(&reader, 2)?,
        };
        bound.validate()?;
        Ok(bound)
    }
}

/// The immutable reasoning history manifest of one dependent run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReasoningHistoryManifestDto {
    pub compatibility_id: String,
    pub entries: Vec<String>,
    pub manifest_digest: String,
    pub transfer_policy: String,
    pub history_bound: ReasoningHistoryBound,
}

impl ReasoningHistoryManifestDto {
    /// Validates the manifest's closed values.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ReasoningHistoryIncompatible` when the
    /// compatibility id or transfer policy is blank,
    /// `CanonicalError::ReasoningHistoryUnavailable` when an entry reference
    /// is blank, `CanonicalError::ReasoningHistoryTooLarge` when the entries
    /// exceed the declared bound or the bound is zero, and
    /// `CanonicalError::InvalidDigest` when the manifest digest is not exactly
    /// sixty-four lowercase hex digits.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        if self.compatibility_id.is_empty() || contains_control_or_nul(&self.compatibility_id) {
            return Err(CanonicalError::ReasoningHistoryIncompatible);
        }
        if self.transfer_policy.is_empty() || contains_control_or_nul(&self.transfer_policy) {
            return Err(CanonicalError::ReasoningHistoryIncompatible);
        }
        self.history_bound.validate()?;
        validate_manifest_entries(&self.entries, &self.history_bound)?;
        validate_digest_hex(&self.manifest_digest)
    }

    /// Encodes this manifest into its canonical record bytes.
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
            TagRegistry::REASONING_HISTORY_MANIFEST_V1,
            1,
            vec![
                (1, WireType::Utf8, encode_utf8(&self.compatibility_id)),
                (2, WireType::List, encode_utf8_list(&self.entries)),
                (3, WireType::Utf8, encode_utf8(&self.manifest_digest)),
                (4, WireType::Utf8, encode_utf8(&self.transfer_policy)),
                (5, WireType::Record, self.history_bound.encode()?),
            ],
        )
    }

    /// Decodes this manifest from its canonical record bytes and verifies its
    /// manifest digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the record is not the
    /// reasoning-history-manifest-v1 table, `CanonicalError::InvalidField`
    /// when any of the five fields is absent or malformed, the validation
    /// errors of [`Self::validate`], `CanonicalError::DigestMismatch` when the
    /// stored digest does not match the identity bytes, and other
    /// `CanonicalError` values for malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 5)?;
        if reader.tag != TagRegistry::REASONING_HISTORY_MANIFEST_V1 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let manifest = Self {
            compatibility_id: decode_utf8(
                reader
                    .field(1, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            entries: decode_utf8_list(
                reader
                    .field(2, WireType::List)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            manifest_digest: decode_utf8(
                reader
                    .field(3, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            transfer_policy: decode_utf8(
                reader
                    .field(4, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            history_bound: ReasoningHistoryBound::decode(
                reader
                    .field(5, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        manifest.validate()?;
        verify_manifest_digest(&manifest)?;
        Ok(manifest)
    }
}

/// Validates one closed reasoning dialect value.
///
/// # Errors
///
/// Returns `CanonicalError::ProviderReasoningStreamInvalid` when `value` is
/// not one of the closed dialect values.
pub fn validate_reasoning_dialect(value: &str) -> Result<(), CanonicalError> {
    if REASONING_DIALECT_VALUES.contains(&value) {
        Ok(())
    } else {
        Err(CanonicalError::ProviderReasoningStreamInvalid)
    }
}

/// Validates that every required reasoning history reference is available.
///
/// # Errors
///
/// Returns `CanonicalError::ReasoningHistoryUnavailable` when a required
/// entry is missing from the available material.
pub fn validate_reasoning_history_available(
    required_entries: &[String],
    available_entries: &[String],
) -> Result<(), CanonicalError> {
    if required_entries
        .iter()
        .any(|required| !available_entries.contains(required))
    {
        return Err(CanonicalError::ReasoningHistoryUnavailable);
    }
    Ok(())
}

/// Validates that required and available reasoning history are compatible.
///
/// # Errors
///
/// Returns `CanonicalError::ReasoningHistoryIncompatible` when the required
/// and available compatibility identities differ.
pub fn validate_reasoning_history_compatibility(
    required: &str,
    available: &str,
) -> Result<(), CanonicalError> {
    if required != available {
        return Err(CanonicalError::ReasoningHistoryIncompatible);
    }
    Ok(())
}

/// Validates that appending one reasoning fragment stays within the fixed
/// per-run output bound.
///
/// # Errors
///
/// Returns `CanonicalError::ReasoningOutputLimitExceeded` when the fragment
/// would push the aggregate over `MAX_REASONING_AGGREGATE_BYTES` or when the
/// aggregate addition itself overflows `u64`.
pub const fn validate_reasoning_output_bound(
    current_aggregate_bytes: u64,
    fragment_len: u64,
) -> Result<(), CanonicalError> {
    match current_aggregate_bytes.checked_add(fragment_len) {
        Some(total) if total <= MAX_REASONING_AGGREGATE_BYTES => Ok(()),
        _ => Err(CanonicalError::ReasoningOutputLimitExceeded),
    }
}

/// Computes the namespaced reasoning-history-manifest digest over the
/// identity-bearing fields only; the manifest digest field is excluded by
/// construction.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidDigest` when the namespace is invalid, and
/// the identity input's own encoding errors, which are impossible for a
/// canonical field stream.
pub fn reasoning_history_manifest_digest(
    manifest: &ReasoningHistoryManifestDto,
) -> Result<NamespacedDigest, CanonicalError> {
    let input = CanonicalIdentityInput::new()
        .field(1, WireType::Utf8, encode_utf8(&manifest.compatibility_id))?
        .field(2, WireType::List, encode_utf8_list(&manifest.entries))?
        .field(4, WireType::Utf8, encode_utf8(&manifest.transfer_policy))?
        .field(5, WireType::Record, manifest.history_bound.encode()?)?;
    Digest256::for_namespace("reasoning-history-manifest", &input.encode()?)
}

/// Validates one reasoning content value.
fn validate_reasoning_content(content: &str) -> Result<(), CanonicalError> {
    if content.trim().is_empty() || contains_control_or_nul(content) {
        return Err(CanonicalError::ProviderReasoningStreamInvalid);
    }
    Ok(())
}

/// Validates manifest entries against the declared bound.
fn validate_manifest_entries(
    entries: &[String],
    bound: &ReasoningHistoryBound,
) -> Result<(), CanonicalError> {
    if entries.len() as u64 > bound.max_entries as u64 {
        return Err(CanonicalError::ReasoningHistoryTooLarge);
    }
    let aggregate = entries
        .iter()
        .try_fold(0u64, |total, entry| total.checked_add(entry.len() as u64))
        .ok_or(CanonicalError::ReasoningHistoryTooLarge)?;
    if aggregate > bound.max_aggregate_bytes {
        return Err(CanonicalError::ReasoningHistoryTooLarge);
    }
    if entries.iter().any(|entry| entry.is_empty()) {
        return Err(CanonicalError::ReasoningHistoryUnavailable);
    }
    Ok(())
}

/// Verifies that the stored manifest digest matches the identity bytes.
fn verify_manifest_digest(manifest: &ReasoningHistoryManifestDto) -> Result<(), CanonicalError> {
    let expected = reasoning_history_manifest_digest(manifest)?;
    let stored = Digest256::from_str_hex(&manifest.manifest_digest)?;
    if stored != expected.digest {
        return Err(CanonicalError::DigestMismatch);
    }
    Ok(())
}

/// Validates that `value` is exactly sixty-four lowercase hex digits.
fn validate_digest_hex(value: &str) -> Result<(), CanonicalError> {
    Digest256::from_str_hex(value).map(|_| ())
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

    fn fixture_manifest() -> ReasoningHistoryManifestDto {
        ReasoningHistoryManifestDto {
            compatibility_id: "reasoning-compat-v1".to_owned(),
            entries: vec![
                "session-11111111-1111-4111-8111-111111111111-run-0001".to_owned(),
                "session-11111111-1111-4111-8111-111111111111-run-0002".to_owned(),
            ],
            manifest_digest: String::new(),
            transfer_policy: "textual-history-v1".to_owned(),
            history_bound: ReasoningHistoryBound {
                max_entries: 64,
                max_aggregate_bytes: MAX_REASONING_AGGREGATE_BYTES,
            },
        }
    }

    fn with_digest(mut manifest: ReasoningHistoryManifestDto) -> ReasoningHistoryManifestDto {
        let digest = reasoning_history_manifest_digest(&manifest).expect("manifest digests");
        manifest.manifest_digest = digest
            .digest
            .to_string()
            .strip_prefix("sha256:")
            .expect("digest text carries the sha256 prefix")
            .to_owned();
        manifest
    }

    #[test]
    fn reasoning_history_manifest_round_trips_and_verifies_its_digest() {
        let manifest = with_digest(fixture_manifest());
        let bytes = manifest.encode().expect("manifest encodes");
        assert_eq!(
            ReasoningHistoryManifestDto::decode(&bytes).expect("manifest decodes"),
            manifest
        );
        assert_eq!(
            ReasoningHistoryManifestDto::decode(&bytes)
                .expect("manifest decodes")
                .encode()
                .expect("manifest re-encodes"),
            bytes
        );
        let mut tampered = manifest;
        tampered.entries.push("extra-entry".to_owned());
        assert_eq!(
            ReasoningHistoryManifestDto::decode(
                &tampered.encode().expect("tampered manifest encodes")
            )
            .expect_err("stale digest is rejected"),
            CanonicalError::DigestMismatch
        );
    }

    #[test]
    fn reasoning_history_bounds_are_enforced() {
        let manifest = with_digest(fixture_manifest());
        let mut over_entries = manifest.clone();
        over_entries.history_bound.max_entries = 1;
        assert_eq!(
            over_entries
                .encode()
                .expect_err("over-limit entry count is rejected")
                .code(),
            "reasoning_history_too_large"
        );
        let mut over_bytes = manifest.clone();
        over_bytes.history_bound.max_aggregate_bytes = 1;
        assert_eq!(
            over_bytes
                .encode()
                .expect_err("over-limit aggregate bytes are rejected")
                .code(),
            "reasoning_history_too_large"
        );
        let mut blank = manifest.clone();
        blank.entries.push(String::new());
        assert_eq!(
            blank.encode().expect_err("blank entry is rejected").code(),
            "reasoning_history_unavailable"
        );
        let mut zero_bound = manifest;
        zero_bound.history_bound.max_entries = 0;
        assert_eq!(
            zero_bound
                .encode()
                .expect_err("zero bound is rejected")
                .code(),
            "reasoning_history_too_large"
        );
    }

    #[test]
    fn reasoning_dialect_is_closed_and_history_rules_are_enforced() {
        for value in REASONING_DIALECT_VALUES {
            assert!(validate_reasoning_dialect(value).is_ok(), "dialect {value}");
        }
        assert_eq!(
            validate_reasoning_dialect("raw_thoughts")
                .expect_err("unknown dialect is rejected")
                .code(),
            "provider_reasoning_stream_invalid"
        );
        assert_eq!(
            validate_reasoning_history_compatibility("a", "b")
                .expect_err("compatibility mismatch is rejected")
                .code(),
            "reasoning_history_incompatible"
        );
        assert!(validate_reasoning_history_compatibility("a", "a").is_ok());
        assert_eq!(
            validate_reasoning_history_available(
                &["required-ref".to_owned()],
                &["other-ref".to_owned()],
            )
            .expect_err("missing material is unavailable")
            .code(),
            "reasoning_history_unavailable"
        );
        assert!(
            validate_reasoning_history_available(
                &["required-ref".to_owned()],
                &["required-ref".to_owned()],
            )
            .is_ok()
        );
        assert_eq!(
            validate_reasoning_output_bound(MAX_REASONING_AGGREGATE_BYTES, 1)
                .expect_err("over-limit fragment is rejected")
                .code(),
            "reasoning_output_limit_exceeded"
        );
        assert_eq!(
            validate_reasoning_output_bound(u64::MAX, 1)
                .expect_err("overflowing aggregate is rejected")
                .code(),
            "reasoning_output_limit_exceeded"
        );
        assert!(validate_reasoning_output_bound(0, 1).is_ok());
        assert!(
            validate_reasoning_output_bound(MAX_REASONING_AGGREGATE_BYTES - 1, 1).is_ok(),
            "an exact-bound aggregate must be accepted"
        );
    }

    #[test]
    fn reasoning_bound_decode_rejects_entries_outside_u32() {
        // A raw `max_entries` value above `u32::MAX` must fail closed instead
        // of silently truncating to a different bound on decode.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"IRCR");
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        for (number, value) in [
            (1u32, encode_u64(u64::from(u32::MAX) + 1)),
            (2u32, encode_u64(1)),
        ] {
            bytes.extend_from_slice(&number.to_be_bytes());
            bytes.push(WireType::U64 as u8);
            bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
            bytes.extend_from_slice(&value);
        }
        assert_eq!(
            ReasoningHistoryBound::decode(&bytes).expect_err("out-of-range bound is rejected"),
            CanonicalError::InvalidField
        );
        // The maximum representable value still decodes.
        let mut maximum = Vec::new();
        maximum.extend_from_slice(b"IRCR");
        maximum.extend_from_slice(&1u32.to_be_bytes());
        maximum.extend_from_slice(&0u32.to_be_bytes());
        maximum.extend_from_slice(&1u32.to_be_bytes());
        for (number, value) in [
            (1u32, encode_u64(u64::from(u32::MAX))),
            (2u32, encode_u64(1)),
        ] {
            maximum.extend_from_slice(&number.to_be_bytes());
            maximum.push(WireType::U64 as u8);
            maximum.extend_from_slice(&(value.len() as u32).to_be_bytes());
            maximum.extend_from_slice(&value);
        }
        assert_eq!(
            ReasoningHistoryBound::decode(&maximum).expect("boundary bound decodes"),
            ReasoningHistoryBound {
                max_entries: u32::MAX,
                max_aggregate_bytes: 1,
            }
        );
    }

    #[test]
    fn reasoning_deltas_and_summaries_validate_closed_content() {
        let delta = ReasoningDeltaDto {
            category: ReasoningDeltaCategory::Primary,
            content: "thinking".to_owned(),
        };
        assert!(delta.validate().is_ok());
        let blank = ReasoningDeltaDto {
            category: ReasoningDeltaCategory::Detail,
            content: "  ".to_owned(),
        };
        assert_eq!(
            blank
                .validate()
                .expect_err("blank reasoning is rejected")
                .code(),
            "provider_reasoning_stream_invalid"
        );
        let summary = ReasoningSummaryDeltaDto {
            content: "summary".to_owned(),
        };
        assert!(summary.validate().is_ok());
    }
}
