//! The legacy M4 selection binding bridge.
//!
//! A binding references original legacy bytes and IDs unchanged; it never
//! rewrites legacy snapshot bytes and never reconstructs them from current
//! configuration.

use crate::canonical::{
    CanonicalError, CanonicalRecordBuilder, CanonicalRecordReader, TagRegistry, WireType,
    contains_control_or_nul, decode_utf8, decode_utf8_list, encode_utf8, encode_utf8_list,
};

/// The closed legacy safe selection prefix.
pub const LEGACY_SELECTION_PREFIX: &str = "legacy-uuid:";
/// Maximum characters of one legacy binding scalar.
pub const MAX_LEGACY_BINDING_STRING_CHARS: usize = 256;

/// The immutable legacy M4 selection binding record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacyM4SelectionBindingDto {
    pub legacy_config_revision_id: String,
    pub legacy_snapshot_schema: String,
    pub legacy_safe_selection: String,
    pub default_profile_id: String,
    pub default_profile_revision_id: String,
    pub kind_descriptor_revision_id: String,
    pub capability_subset: Vec<String>,
    pub execution_policy: String,
    pub driver_contract_revision: String,
}

impl LegacyM4SelectionBindingDto {
    /// Validates the binding's closed values.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::LegacySelectionReferenceInvalid` when any
    /// scalar is blank, over the bound, or carries control characters, a
    /// capability entry is blank, or the legacy safe selection is not a
    /// canonical `legacy-uuid:<UUID>` reference.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        validate_binding_string(&self.legacy_config_revision_id)?;
        validate_binding_string(&self.legacy_snapshot_schema)?;
        validate_legacy_safe_selection(&self.legacy_safe_selection)?;
        validate_binding_string(&self.default_profile_id)?;
        validate_binding_string(&self.default_profile_revision_id)?;
        validate_binding_string(&self.kind_descriptor_revision_id)?;
        for capability in &self.capability_subset {
            validate_binding_string(capability)?;
        }
        validate_binding_string(&self.execution_policy)?;
        validate_binding_string(&self.driver_contract_revision)
    }

    /// Encodes this binding into its canonical record bytes.
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
            TagRegistry::LEGACY_M4_SELECTION_BINDING,
            1,
            vec![
                (
                    1,
                    WireType::Utf8,
                    encode_utf8(&self.legacy_config_revision_id),
                ),
                (2, WireType::Utf8, encode_utf8(&self.legacy_snapshot_schema)),
                (3, WireType::Utf8, encode_utf8(&self.legacy_safe_selection)),
                (4, WireType::Utf8, encode_utf8(&self.default_profile_id)),
                (
                    5,
                    WireType::Utf8,
                    encode_utf8(&self.default_profile_revision_id),
                ),
                (
                    6,
                    WireType::Utf8,
                    encode_utf8(&self.kind_descriptor_revision_id),
                ),
                (7, WireType::List, encode_utf8_list(&self.capability_subset)),
                (8, WireType::Utf8, encode_utf8(&self.execution_policy)),
                (
                    9,
                    WireType::Utf8,
                    encode_utf8(&self.driver_contract_revision),
                ),
            ],
        )
    }

    /// Decodes this binding from its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the record is not the
    /// legacy-m4-selection-binding table, `CanonicalError::InvalidField` when
    /// any of the nine fields is absent or malformed, the validation errors
    /// of [`Self::validate`], and other `CanonicalError` values for malformed
    /// or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 9)?;
        if reader.tag != TagRegistry::LEGACY_M4_SELECTION_BINDING || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let binding = Self {
            legacy_config_revision_id: decode_utf8(
                reader
                    .field(1, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            legacy_snapshot_schema: decode_utf8(
                reader
                    .field(2, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            legacy_safe_selection: decode_utf8(
                reader
                    .field(3, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            default_profile_id: decode_utf8(
                reader
                    .field(4, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            default_profile_revision_id: decode_utf8(
                reader
                    .field(5, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            kind_descriptor_revision_id: decode_utf8(
                reader
                    .field(6, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            capability_subset: decode_utf8_list(
                reader
                    .field(7, WireType::List)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            execution_policy: decode_utf8(
                reader
                    .field(8, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            driver_contract_revision: decode_utf8(
                reader
                    .field(9, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
        };
        binding.validate()?;
        Ok(binding)
    }
}

/// Validates one legacy safe selection reference.
///
/// The reference must be exactly `legacy-uuid:` followed by one canonical
/// lowercase hyphenated UUID. Legacy bytes are never rewritten.
///
/// # Errors
///
/// Returns `CanonicalError::LegacySelectionReferenceInvalid` when the value
/// is not a canonical legacy UUID reference.
pub fn validate_legacy_safe_selection(value: &str) -> Result<(), CanonicalError> {
    let Some(uuid) = value.strip_prefix(LEGACY_SELECTION_PREFIX) else {
        return Err(CanonicalError::LegacySelectionReferenceInvalid);
    };
    if is_canonical_uuid(uuid) {
        Ok(())
    } else {
        Err(CanonicalError::LegacySelectionReferenceInvalid)
    }
}

/// Whether `value` is one canonical lowercase hyphenated UUID.
#[must_use]
fn is_canonical_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        match index {
            8 | 13 | 18 | 23 => {
                if *byte != b'-' {
                    return false;
                }
            }
            _ => {
                if !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase() {
                    return false;
                }
            }
        }
    }
    true
}

/// Validates one legacy binding scalar string.
fn validate_binding_string(value: &str) -> Result<(), CanonicalError> {
    if value.is_empty()
        || value.len() > MAX_LEGACY_BINDING_STRING_CHARS
        || contains_control_or_nul(value)
    {
        return Err(CanonicalError::LegacySelectionReferenceInvalid);
    }
    Ok(())
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

    fn fixture_binding() -> LegacyM4SelectionBindingDto {
        LegacyM4SelectionBindingDto {
            legacy_config_revision_id: "11111111-1111-4111-8111-111111111111".to_owned(),
            legacy_snapshot_schema: "m4-config-snapshot-v1".to_owned(),
            legacy_safe_selection: "legacy-uuid:22222222-2222-4222-8222-222222222222".to_owned(),
            default_profile_id: "default".to_owned(),
            default_profile_revision_id: "rev-0001".to_owned(),
            kind_descriptor_revision_id: "kind-descriptor-rev-0001".to_owned(),
            capability_subset: vec!["text_input".to_owned(), "text_streaming".to_owned()],
            execution_policy: "ordinary".to_owned(),
            driver_contract_revision: "responses-1.0".to_owned(),
        }
    }

    #[test]
    fn legacy_binding_round_trips_and_preserves_the_legacy_reference_exactly() {
        let binding = fixture_binding();
        let bytes = binding.encode().expect("binding encodes");
        assert_eq!(
            LegacyM4SelectionBindingDto::decode(&bytes).expect("binding decodes"),
            binding
        );
        assert_eq!(
            LegacyM4SelectionBindingDto::decode(&bytes)
                .expect("binding decodes")
                .encode()
                .expect("binding re-encodes"),
            bytes
        );
        assert!(validate_legacy_safe_selection(&binding.legacy_safe_selection).is_ok());
    }

    #[test]
    fn legacy_binding_requires_the_legacy_uuid_prefix_and_canonical_uuid() {
        for invalid in [
            "22222222-2222-4222-8222-222222222222",
            "legacy-uuid:not-a-uuid",
            "legacy-uuid:22222222-2222-4222-8222-22222222222Z",
            "legacy-uuid:22222222222242228222222222222222",
            "legacy-uuid:22222222-2222-4222-8222-222222222222-extra",
            "other:22222222-2222-4222-8222-222222222222",
        ] {
            assert_eq!(
                validate_legacy_safe_selection(invalid)
                    .expect_err("invalid legacy reference is rejected")
                    .code(),
                "legacy_selection_reference_invalid",
                "reference {invalid}"
            );
        }
        let mut binding = fixture_binding();
        binding.legacy_safe_selection =
            "legacy-uuid:22222222-2222-4222-8222-222222222222".to_owned();
        assert!(binding.encode().is_ok());
        binding.legacy_safe_selection = "22222222-2222-4222-8222-222222222222".to_owned();
        assert_eq!(
            binding
                .encode()
                .expect_err("missing prefix is rejected")
                .code(),
            "legacy_selection_reference_invalid"
        );
    }
}
