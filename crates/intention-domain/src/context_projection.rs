//! Context source manifests and the immutable model context projection.
//!
//! A projection references safe source material and carries ordered messages
//! only; it never carries credentials, raw TOML, filesystem paths, client
//! handles, or operational readiness.

use crate::canonical::{
    CanonicalError, CanonicalIdentityInput, CanonicalRecordBuilder, CanonicalRecordReader,
    Digest256, NamespacedDigest, TagRegistry, WireType, contains_control_or_nul,
    contains_credential_shape, decode_optional_utf8, decode_utf8, decode_utf8_list,
    encode_optional_utf8, encode_utf8, encode_utf8_list,
};

/// Maximum source entries in one context source manifest.
pub const MAX_CONTEXT_SOURCE_ENTRIES: usize = 256;
/// Maximum ordered messages in one model context projection.
pub const MAX_PROJECTION_MESSAGES: usize = 1024;
/// Maximum aggregate message bytes in one model context projection.
pub const MAX_PROJECTION_AGGREGATE_BYTES: u64 = 1024 * 1024;
/// Maximum characters of one context source scalar.
pub const MAX_CONTEXT_SOURCE_STRING_CHARS: usize = 256;

/// One safe context source reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextSourceEntryV1 {
    pub source_id: String,
    pub source_kind: String,
    pub revision: String,
    pub safe_label: Option<String>,
}

impl ContextSourceEntryV1 {
    /// Validates the source entry.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ContextSourceManifestInvalid` when a scalar is
    /// blank, over the bound, or carries control characters, and
    /// `CanonicalError::CredentialsForbidden` when the safe label carries a
    /// credential-shaped value.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        validate_source_string(&self.source_id)?;
        validate_source_string(&self.source_kind)?;
        validate_source_string(&self.revision)?;
        if let Some(label) = &self.safe_label {
            validate_source_string(label)?;
            if contains_credential_shape(label) {
                return Err(CanonicalError::CredentialsForbidden);
            }
        }
        Ok(())
    }

    /// Encodes this entry into its nested anonymous record.
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
        self.encode_fields(true)
    }

    /// Encodes the identity-bearing fields only; the safe label is excluded
    /// from the identity digest.
    fn encode_identity(&self) -> Result<Vec<u8>, CanonicalError> {
        self.encode_fields(false)
    }

    fn encode_fields(&self, include_safe_label: bool) -> Result<Vec<u8>, CanonicalError> {
        let mut fields = vec![
            (1, WireType::Utf8, encode_utf8(&self.source_id)),
            (2, WireType::Utf8, encode_utf8(&self.source_kind)),
            (3, WireType::Utf8, encode_utf8(&self.revision)),
        ];
        if include_safe_label {
            fields.push((
                4,
                WireType::Optional,
                encode_optional_utf8(&self.safe_label),
            ));
        }
        record(0, 1, fields)
    }

    /// Decodes this entry from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the nested record is not the
    /// anonymous tag-zero version-one frame, `CanonicalError::InvalidField`
    /// when any of the four fields is absent or malformed, the validation
    /// errors of [`Self::validate`], and other `CanonicalError` values for
    /// malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 4)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let entry = Self {
            source_id: decode_utf8(
                reader
                    .field(1, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            source_kind: decode_utf8(
                reader
                    .field(2, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            revision: decode_utf8(
                reader
                    .field(3, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            safe_label: decode_optional_utf8(
                reader
                    .field(4, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        entry.validate()?;
        Ok(entry)
    }
}

/// The immutable context source manifest of one model context projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextSourceManifestV1 {
    pub compatibility_id: String,
    pub source_entries: Vec<ContextSourceEntryV1>,
    pub manifest_digest: String,
}

impl ContextSourceManifestV1 {
    /// Validates the manifest's closed values.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ContextSourceManifestInvalid` when the
    /// compatibility id is blank or the source entries are not between one and
    /// `MAX_CONTEXT_SOURCE_ENTRIES`, the entry validation errors of
    /// [`ContextSourceEntryV1::validate`], and `CanonicalError::InvalidDigest`
    /// when the manifest digest is not exactly sixty-four lowercase hex
    /// digits.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        if self.compatibility_id.is_empty() || contains_control_or_nul(&self.compatibility_id) {
            return Err(CanonicalError::ContextSourceManifestInvalid);
        }
        if self.source_entries.is_empty() || self.source_entries.len() > MAX_CONTEXT_SOURCE_ENTRIES
        {
            return Err(CanonicalError::ContextSourceManifestInvalid);
        }
        for entry in &self.source_entries {
            entry.validate()?;
        }
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
            TagRegistry::CONTEXT_SOURCE_MANIFEST_V1,
            1,
            vec![
                (1, WireType::Utf8, encode_utf8(&self.compatibility_id)),
                (
                    2,
                    WireType::List,
                    encode_entries(&self.source_entries, true)?,
                ),
                (3, WireType::Utf8, encode_utf8(&self.manifest_digest)),
            ],
        )
    }

    /// Decodes this manifest from its canonical record bytes and verifies its
    /// manifest digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the record is not the
    /// context-source-manifest-v1 table, `CanonicalError::InvalidField` when
    /// any of the three fields is absent or malformed, the validation errors
    /// of [`Self::validate`], `CanonicalError::DigestMismatch` when the stored
    /// digest does not match the identity bytes, and other `CanonicalError`
    /// values for malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 3)?;
        if reader.tag != TagRegistry::CONTEXT_SOURCE_MANIFEST_V1 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let manifest = Self {
            compatibility_id: decode_utf8(
                reader
                    .field(1, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            source_entries: decode_entries(
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
        };
        manifest.validate()?;
        verify_manifest_digest(&manifest)?;
        Ok(manifest)
    }
}

/// The immutable model context projection of one run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelContextProjectionV1 {
    pub projection_revision: String,
    pub context_schema_version: String,
    pub source_manifest_digest: String,
    pub ordered_messages: Vec<String>,
    pub model_context_digest: String,
}

impl ModelContextProjectionV1 {
    /// Validates the projection's closed values.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ModelContextProjectionInvalid` when a scalar
    /// is blank, the ordered messages are not between one and
    /// `MAX_PROJECTION_MESSAGES` nonblank entries, or a digest is not exactly
    /// sixty-four lowercase hex digits, and
    /// `CanonicalError::ModelContextProjectionTooLarge` when the aggregate
    /// message bytes exceed `MAX_PROJECTION_AGGREGATE_BYTES`.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        validate_projection_string(&self.projection_revision)?;
        validate_projection_string(&self.context_schema_version)?;
        validate_digest_hex(&self.source_manifest_digest)?;
        if self.ordered_messages.is_empty() || self.ordered_messages.len() > MAX_PROJECTION_MESSAGES
        {
            return Err(CanonicalError::ModelContextProjectionInvalid);
        }
        let mut aggregate = 0u64;
        for message in &self.ordered_messages {
            if message.trim().is_empty() || contains_control_or_nul(message) {
                return Err(CanonicalError::ModelContextProjectionInvalid);
            }
            aggregate = aggregate
                .checked_add(message.len() as u64)
                .ok_or(CanonicalError::ModelContextProjectionTooLarge)?;
        }
        if aggregate > MAX_PROJECTION_AGGREGATE_BYTES {
            return Err(CanonicalError::ModelContextProjectionTooLarge);
        }
        validate_digest_hex(&self.model_context_digest)
    }

    /// Encodes this projection into its canonical record bytes.
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
            TagRegistry::MODEL_CONTEXT_PROJECTION_V1,
            1,
            vec![
                (1, WireType::Utf8, encode_utf8(&self.projection_revision)),
                (2, WireType::Utf8, encode_utf8(&self.context_schema_version)),
                (3, WireType::Utf8, encode_utf8(&self.source_manifest_digest)),
                (4, WireType::List, encode_utf8_list(&self.ordered_messages)),
                (5, WireType::Utf8, encode_utf8(&self.model_context_digest)),
            ],
        )
    }

    /// Decodes this projection from its canonical record bytes and verifies
    /// its model context digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the record is not the
    /// model-context-projection-v1 table, `CanonicalError::InvalidField` when
    /// any of the five fields is absent or malformed, the validation errors
    /// of [`Self::validate`], `CanonicalError::DigestMismatch` when the stored
    /// digest does not match the identity bytes, and other `CanonicalError`
    /// values for malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 5)?;
        if reader.tag != TagRegistry::MODEL_CONTEXT_PROJECTION_V1 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let projection = Self {
            projection_revision: decode_utf8(
                reader
                    .field(1, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            context_schema_version: decode_utf8(
                reader
                    .field(2, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            source_manifest_digest: decode_utf8(
                reader
                    .field(3, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            ordered_messages: decode_utf8_list(
                reader
                    .field(4, WireType::List)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            model_context_digest: decode_utf8(
                reader
                    .field(5, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
        };
        projection.validate()?;
        verify_projection_digest(&projection)?;
        Ok(projection)
    }
}

/// Computes the namespaced context-source-manifest digest over the
/// identity-bearing fields only.
///
/// The safe label of every source entry and the manifest digest field itself
/// are excluded from the identity by construction.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidDigest` when the namespace is invalid, and
/// the identity input's own encoding errors, which are impossible for a
/// canonical field stream.
pub fn context_source_manifest_digest(
    manifest: &ContextSourceManifestV1,
) -> Result<NamespacedDigest, CanonicalError> {
    let input = CanonicalIdentityInput::new()
        .field(1, WireType::Utf8, encode_utf8(&manifest.compatibility_id))?
        .field(
            2,
            WireType::List,
            encode_entries(&manifest.source_entries, false)?,
        )?;
    Digest256::for_namespace("context-source-manifest", &input.encode()?)
}

/// Computes the namespaced model-context-projection digest over the
/// identity-bearing fields only; the model context digest field is excluded
/// by construction.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidDigest` when the namespace is invalid, and
/// the identity input's own encoding errors, which are impossible for a
/// canonical field stream.
pub fn model_context_projection_digest(
    projection: &ModelContextProjectionV1,
) -> Result<NamespacedDigest, CanonicalError> {
    let input = CanonicalIdentityInput::new()
        .field(
            1,
            WireType::Utf8,
            encode_utf8(&projection.projection_revision),
        )?
        .field(
            2,
            WireType::Utf8,
            encode_utf8(&projection.context_schema_version),
        )?
        .field(
            3,
            WireType::Utf8,
            encode_utf8(&projection.source_manifest_digest),
        )?
        .field(
            4,
            WireType::List,
            encode_utf8_list(&projection.ordered_messages),
        )?;
    Digest256::for_namespace("model-context-projection", &input.encode()?)
}

/// Encodes a list of source entries.
fn encode_entries(
    entries: &[ContextSourceEntryV1],
    include_safe_label: bool,
) -> Result<Vec<u8>, CanonicalError> {
    let mut items = Vec::with_capacity(entries.len());
    for entry in entries {
        items.push(if include_safe_label {
            entry.encode()?
        } else {
            entry.encode_identity()?
        });
    }
    Ok(crate::canonical::encode_list_items(&items))
}

/// Decodes a list of source entries.
fn decode_entries(bytes: &[u8]) -> Result<Vec<ContextSourceEntryV1>, CanonicalError> {
    crate::canonical::decode_list_items(bytes)?
        .into_iter()
        .map(|item| ContextSourceEntryV1::decode(&item))
        .collect()
}

/// Verifies that the stored manifest digest matches the identity bytes.
fn verify_manifest_digest(manifest: &ContextSourceManifestV1) -> Result<(), CanonicalError> {
    let expected = context_source_manifest_digest(manifest)?;
    let stored = Digest256::from_str_hex(&manifest.manifest_digest)?;
    if stored != expected.digest {
        return Err(CanonicalError::DigestMismatch);
    }
    Ok(())
}

/// Verifies that the stored model context digest matches the identity bytes.
fn verify_projection_digest(projection: &ModelContextProjectionV1) -> Result<(), CanonicalError> {
    let expected = model_context_projection_digest(projection)?;
    let stored = Digest256::from_str_hex(&projection.model_context_digest)?;
    if stored != expected.digest {
        return Err(CanonicalError::DigestMismatch);
    }
    Ok(())
}

/// Validates that `value` is exactly sixty-four lowercase hex digits.
fn validate_digest_hex(value: &str) -> Result<(), CanonicalError> {
    Digest256::from_str_hex(value).map(|_| ())
}

/// Validates one context source scalar string.
fn validate_source_string(value: &str) -> Result<(), CanonicalError> {
    if value.is_empty()
        || value.len() > MAX_CONTEXT_SOURCE_STRING_CHARS
        || contains_control_or_nul(value)
    {
        return Err(CanonicalError::ContextSourceManifestInvalid);
    }
    Ok(())
}

/// Validates one projection scalar string.
fn validate_projection_string(value: &str) -> Result<(), CanonicalError> {
    if value.is_empty() || contains_control_or_nul(value) {
        return Err(CanonicalError::ModelContextProjectionInvalid);
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

    fn fixture_entry() -> ContextSourceEntryV1 {
        ContextSourceEntryV1 {
            source_id: "session-history".to_owned(),
            source_kind: "session".to_owned(),
            revision: "rev-0001".to_owned(),
            safe_label: Some("Session history".to_owned()),
        }
    }

    fn fixture_manifest() -> ContextSourceManifestV1 {
        ContextSourceManifestV1 {
            compatibility_id: "context-source-manifest-v1".to_owned(),
            source_entries: vec![fixture_entry()],
            manifest_digest: String::new(),
        }
    }

    fn with_digest(mut manifest: ContextSourceManifestV1) -> ContextSourceManifestV1 {
        let digest = context_source_manifest_digest(&manifest).expect("manifest digests");
        manifest.manifest_digest = digest
            .digest
            .to_string()
            .strip_prefix("sha256:")
            .expect("digest text carries the sha256 prefix")
            .to_owned();
        manifest
    }

    fn fixture_projection() -> ModelContextProjectionV1 {
        ModelContextProjectionV1 {
            projection_revision: "1".to_owned(),
            context_schema_version: "1".to_owned(),
            source_manifest_digest: "0".repeat(64),
            ordered_messages: vec!["hello".to_owned(), "world".to_owned()],
            model_context_digest: String::new(),
        }
    }

    fn with_projection_digest(
        mut projection: ModelContextProjectionV1,
    ) -> ModelContextProjectionV1 {
        let digest = model_context_projection_digest(&projection).expect("projection digests");
        projection.model_context_digest = digest
            .digest
            .to_string()
            .strip_prefix("sha256:")
            .expect("digest text carries the sha256 prefix")
            .to_owned();
        projection
    }

    #[test]
    fn context_source_manifest_round_trips_and_excludes_safe_labels_from_digest() {
        let manifest = with_digest(fixture_manifest());
        let bytes = manifest.encode().expect("manifest encodes");
        assert_eq!(
            ContextSourceManifestV1::decode(&bytes).expect("manifest decodes"),
            manifest
        );
        assert_eq!(
            ContextSourceManifestV1::decode(&bytes)
                .expect("manifest decodes")
                .encode()
                .expect("manifest re-encodes"),
            bytes
        );
        let mut relabeled = manifest.clone();
        relabeled.source_entries[0].safe_label = Some("Different label".to_owned());
        relabeled.manifest_digest = with_digest(relabeled.clone()).manifest_digest;
        assert_eq!(
            context_source_manifest_digest(&manifest).expect("manifest digests"),
            context_source_manifest_digest(&relabeled).expect("relabeled manifest digests")
        );
        assert_eq!(
            ContextSourceManifestV1::decode(
                &relabeled.encode().expect("relabeled manifest encodes")
            )
            .expect("relabeled manifest decodes"),
            relabeled
        );
    }

    #[test]
    fn context_source_manifest_requires_one_to_256_entries() {
        let manifest = with_digest(fixture_manifest());
        let mut empty = manifest;
        empty.source_entries.clear();
        assert_eq!(
            empty
                .encode()
                .expect_err("empty manifest is rejected")
                .code(),
            "context_source_manifest_invalid"
        );
        let many = with_digest(ContextSourceManifestV1 {
            compatibility_id: "context-source-manifest-v1".to_owned(),
            source_entries: (0..MAX_CONTEXT_SOURCE_ENTRIES)
                .map(|index| ContextSourceEntryV1 {
                    source_id: format!("source-{index}"),
                    source_kind: "session".to_owned(),
                    revision: "rev-0001".to_owned(),
                    safe_label: None,
                })
                .collect(),
            manifest_digest: String::new(),
        });
        assert!((many.encode()).is_ok());
        let mut over = many;
        over.source_entries.push(fixture_entry());
        over.manifest_digest = with_digest(over.clone()).manifest_digest;
        assert_eq!(
            over.encode()
                .expect_err("over-limit manifest is rejected")
                .code(),
            "context_source_manifest_invalid"
        );
    }

    #[test]
    fn model_context_projection_round_trips_and_enforces_bounds() {
        let projection = with_projection_digest(fixture_projection());
        let bytes = projection.encode().expect("projection encodes");
        assert_eq!(
            ModelContextProjectionV1::decode(&bytes).expect("projection decodes"),
            projection
        );
        assert_eq!(
            ModelContextProjectionV1::decode(&bytes)
                .expect("projection decodes")
                .encode()
                .expect("projection re-encodes"),
            bytes
        );
        let mut blank = projection.clone();
        blank.ordered_messages.push("  ".to_owned());
        assert_eq!(
            blank
                .encode()
                .expect_err("blank message is rejected")
                .code(),
            "model_context_projection_invalid"
        );
        let mut over_messages = projection.clone();
        over_messages.ordered_messages = vec!["x".to_owned(); MAX_PROJECTION_MESSAGES + 1];
        assert_eq!(
            over_messages
                .encode()
                .expect_err("over-limit message count is rejected")
                .code(),
            "model_context_projection_invalid"
        );
        let mut over_bytes = projection;
        over_bytes.ordered_messages =
            vec!["x".repeat((MAX_PROJECTION_AGGREGATE_BYTES as usize) + 1)];
        assert_eq!(
            over_bytes
                .encode()
                .expect_err("over-limit aggregate bytes are rejected")
                .code(),
            "model_context_projection_too_large"
        );
    }
}
