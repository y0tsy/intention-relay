//! Provider capability taxonomy and provider selection canonical records.
//!
//! The model capability taxonomy is the closed v1 capability vocabulary; the
//! provider selection is the credential-free immutable evidence bound at fresh
//! admission. Selections never carry credential literals, display names,
//! enabled state, readiness, SDK/client resources, or current configuration.

use crate::canonical::{
    CanonicalError, CanonicalIdentityInput, CanonicalRecordBuilder, CanonicalRecordReader,
    Digest256, NamespacedDigest, TagRegistry, WireType, contains_control_or_nul,
    decode_optional_utf8, decode_utf8, decode_utf8_list, encode_optional_utf8, encode_utf8,
    encode_utf8_list,
};
use crate::provider_catalog::{
    CredentialTransportMode, MAX_PROVIDER_ID_CHARS, validate_credential_transport,
    validate_endpoint, validate_provider_kind_id, validate_provider_string,
};

/// The closed model capability taxonomy version.
pub const MODEL_CAPABILITY_TAXONOMY_V1: &str = "model-capability-taxonomy-v1";
/// The single canonicalization version written by every provider-selection
/// producer (catalog and run-admission paths) so semantically identical
/// selections share identical canonical bytes and digests.
pub const PROVIDER_SELECTION_CANONICALIZATION_VERSION: &str = "1";

/// The closed text-only model input capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelInputCapability {
    TextOnly,
}

impl ModelInputCapability {
    fn enc(self) -> Vec<u8> {
        vec![match self {
            Self::TextOnly => 0,
        }]
    }

    fn dec(bytes: &[u8]) -> Result<Self, CanonicalError> {
        match bytes {
            [0] => Ok(Self::TextOnly),
            _ => Err(CanonicalError::InvalidField),
        }
    }
}

/// The closed structured-output capability of the v1 taxonomy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StructuredOutputCapability {
    Unsupported,
}

impl StructuredOutputCapability {
    fn enc(self) -> Vec<u8> {
        vec![match self {
            Self::Unsupported => 0,
        }]
    }

    fn dec(bytes: &[u8]) -> Result<Self, CanonicalError> {
        match bytes {
            [0] => Ok(Self::Unsupported),
            _ => Err(CanonicalError::InvalidField),
        }
    }
}

/// The closed reasoning capability of the v1 taxonomy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReasoningCapability {
    Disabled,
    TextualReasoningV1,
}

impl ReasoningCapability {
    fn enc(self) -> Vec<u8> {
        vec![match self {
            Self::Disabled => 0,
            Self::TextualReasoningV1 => 1,
        }]
    }

    fn dec(bytes: &[u8]) -> Result<Self, CanonicalError> {
        match bytes {
            [0] => Ok(Self::Disabled),
            [1] => Ok(Self::TextualReasoningV1),
            _ => Err(CanonicalError::InvalidField),
        }
    }
}

/// The closed context-preservation capability of the v1 taxonomy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextPreservationCapability {
    LocalDurableHistoryV1 { reasoning_input_contract: String },
}

impl ContextPreservationCapability {
    /// Validates the closed capability value.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ProviderProfileRevisionInvalid` when the
    /// reasoning input contract is blank, over the scalar bound, or carries
    /// control characters.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        match self {
            Self::LocalDurableHistoryV1 {
                reasoning_input_contract,
            } => validate_provider_string(reasoning_input_contract, 256),
        }
    }

    /// Encodes this capability into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ProviderProfileRevisionInvalid` when the
    /// reasoning input contract is invalid, and
    /// `CanonicalError::DuplicateOrDescendingField` or
    /// `CanonicalError::OverLimit` only if the fixed field table were
    /// noncanonical or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        match self {
            Self::LocalDurableHistoryV1 {
                reasoning_input_contract,
            } => record(
                0,
                1,
                vec![(1, WireType::Utf8, encode_utf8(reasoning_input_contract))],
            ),
        }
    }

    /// Decodes this capability from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the nested record is not the
    /// anonymous tag-zero version-one frame,
    /// `CanonicalError::InvalidField` when the contract field is absent or
    /// malformed, `CanonicalError::ProviderProfileRevisionInvalid` when the
    /// contract is invalid, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 1)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let reasoning_input_contract = decode_utf8(
            reader
                .field(1, WireType::Utf8)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .to_owned();
        let capability = Self::LocalDurableHistoryV1 {
            reasoning_input_contract,
        };
        capability.validate()?;
        Ok(capability)
    }
}

/// The closed v1 model capability set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCapabilitySetV1 {
    pub taxonomy_version: String,
    pub input: ModelInputCapability,
    pub text_streaming: bool,
    pub structured_output: StructuredOutputCapability,
    pub reasoning: ReasoningCapability,
    pub tool_exchange: bool,
    pub context_preservation: ContextPreservationCapability,
}

impl ModelCapabilitySetV1 {
    /// Validates the closed capability set.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ProviderProfileRevisionInvalid` when the
    /// taxonomy version is not the closed `model-capability-taxonomy-v1`
    /// value or a nested capability is invalid.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        if self.taxonomy_version != MODEL_CAPABILITY_TAXONOMY_V1 {
            return Err(CanonicalError::ProviderProfileRevisionInvalid);
        }
        self.context_preservation.validate()
    }

    /// Returns whether the closed taxonomy declares `capability` as supported.
    ///
    /// The closed v1 taxonomy declares structured output unsupported, so that
    /// capability can never be selected under this version. A capability is
    /// never inferred from a model ID, endpoint, or availability observation.
    #[must_use]
    pub fn supports(&self, capability: &str) -> bool {
        match capability {
            "text_input" => true,
            "text_streaming" => self.text_streaming,
            "structured_output" => false,
            "reasoning" => !matches!(self.reasoning, ReasoningCapability::Disabled),
            "tool_exchange" => self.tool_exchange,
            "context_preservation" => true,
            _ => false,
        }
    }

    /// Encodes this capability set into its canonical taxonomy record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ProviderProfileRevisionInvalid` when the
    /// taxonomy version is not the closed value or a nested capability is
    /// invalid, and `CanonicalError::DuplicateOrDescendingField` or
    /// `CanonicalError::OverLimit` only if the fixed field table were
    /// noncanonical or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        record(
            TagRegistry::MODEL_CAPABILITY_TAXONOMY_V1,
            1,
            vec![
                (1, WireType::Utf8, encode_utf8(&self.taxonomy_version)),
                (2, WireType::U64, self.input.enc()),
                (3, WireType::Bool, vec![self.text_streaming as u8]),
                (4, WireType::U64, self.structured_output.enc()),
                (5, WireType::U64, self.reasoning.enc()),
                (6, WireType::Bool, vec![self.tool_exchange as u8]),
                (7, WireType::Record, self.context_preservation.encode()?),
            ],
        )
    }

    /// Decodes this capability set from its canonical taxonomy record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the record is not the
    /// model-capability-taxonomy-v1 table, `CanonicalError::InvalidField`
    /// when any of the seven fields is absent or malformed,
    /// `CanonicalError::ProviderProfileRevisionInvalid` when the decoded
    /// values are not the closed taxonomy, and other `CanonicalError` values
    /// for malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 7)?;
        if reader.tag != TagRegistry::MODEL_CAPABILITY_TAXONOMY_V1 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let capability = Self {
            taxonomy_version: decode_utf8(
                reader
                    .field(1, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            input: ModelInputCapability::dec(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            text_streaming: decode_bool_field(&reader, 3)?,
            structured_output: StructuredOutputCapability::dec(
                reader
                    .field(4, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            reasoning: ReasoningCapability::dec(
                reader
                    .field(5, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            tool_exchange: decode_bool_field(&reader, 6)?,
            context_preservation: ContextPreservationCapability::decode(
                reader
                    .field(7, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        capability.validate()?;
        Ok(capability)
    }
}

/// One explicit capability selection against a descriptor envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCapabilitySelectionV1 {
    pub taxonomy_version: String,
    pub descriptor_capability_envelope: ModelCapabilitySetV1,
    pub selected_capabilities: Vec<String>,
}

impl ModelCapabilitySelectionV1 {
    /// Validates the selection against the closed taxonomy.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ProviderProfileRevisionInvalid` when the
    /// taxonomy version is not the closed value, a selected capability is
    /// blank, or a selected capability is not supported by the descriptor
    /// envelope. A capability is never selected solely because health or
    /// discovery reports it available.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        if self.taxonomy_version != MODEL_CAPABILITY_TAXONOMY_V1 {
            return Err(CanonicalError::ProviderProfileRevisionInvalid);
        }
        self.descriptor_capability_envelope.validate()?;
        for capability in &self.selected_capabilities {
            if capability.is_empty() || contains_control_or_nul(capability) {
                return Err(CanonicalError::ProviderProfileRevisionInvalid);
            }
            if !self.descriptor_capability_envelope.supports(capability) {
                return Err(CanonicalError::ProviderProfileRevisionInvalid);
            }
        }
        Ok(())
    }

    /// Encodes this selection into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::ProviderProfileRevisionInvalid` when the
    /// selection is not a subset of the descriptor envelope, and other
    /// `CanonicalError` values if the envelope cannot encode or the fixed
    /// field table were noncanonical.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        record(
            0,
            1,
            vec![
                (1, WireType::Utf8, encode_utf8(&self.taxonomy_version)),
                (
                    2,
                    WireType::Record,
                    self.descriptor_capability_envelope.encode()?,
                ),
                (
                    3,
                    WireType::List,
                    encode_utf8_list(&self.selected_capabilities),
                ),
            ],
        )
    }

    /// Decodes this selection from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the nested record is not the
    /// anonymous tag-zero version-one frame, `CanonicalError::InvalidField`
    /// when any of the three fields is absent or malformed,
    /// `CanonicalError::ProviderProfileRevisionInvalid` when the selection is
    /// not a subset of the envelope, and other `CanonicalError` values for
    /// malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 3)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let selection = Self {
            taxonomy_version: decode_utf8(
                reader
                    .field(1, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            descriptor_capability_envelope: ModelCapabilitySetV1::decode(
                reader
                    .field(2, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            selected_capabilities: decode_utf8_list(
                reader
                    .field(3, WireType::List)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        selection.validate()?;
        Ok(selection)
    }
}

/// The credential-free immutable provider selection record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSelectionV1 {
    pub selection_canonicalization_version: String,
    pub profile_id: String,
    pub provider_profile_revision_id: String,
    pub kind_id: String,
    pub kind_descriptor_revision_id: String,
    pub model_id: String,
    pub normalized_effective_endpoint: String,
    pub credential_transport_mode: CredentialTransportMode,
    pub credential_transport_safe_header_name: Option<String>,
    pub declared_model_capability_subset: Vec<String>,
    pub resolved_reasoning_policy: String,
    pub effective_execution_policy: String,
    pub effective_loopback_policy_or_not_applicable: String,
    pub provider_driver_contract_revision: String,
    pub selection_source: Option<String>,
}

impl ProviderSelectionV1 {
    /// Validates the selection's closed values.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidProviderKind` when the kind id is
    /// `openai` or otherwise invalid, `CanonicalError::InvalidEndpoint` when
    /// the endpoint carries userinfo, query, fragment, or control characters,
    /// `CanonicalError::CredentialsForbidden` when a provenance or header
    /// field carries a credential-shaped value, and
    /// `CanonicalError::ProviderProfileRevisionInvalid` for a canonicalization
    /// version other than [`PROVIDER_SELECTION_CANONICALIZATION_VERSION`] and
    /// for every other invalid selection value.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        if self.selection_canonicalization_version != PROVIDER_SELECTION_CANONICALIZATION_VERSION {
            return Err(CanonicalError::ProviderProfileRevisionInvalid);
        }
        validate_provider_string(&self.profile_id, MAX_PROVIDER_ID_CHARS)?;
        validate_provider_string(&self.provider_profile_revision_id, MAX_PROVIDER_ID_CHARS)?;
        validate_provider_kind_id(&self.kind_id)?;
        validate_provider_string(&self.kind_descriptor_revision_id, MAX_PROVIDER_ID_CHARS)?;
        validate_provider_string(&self.model_id, MAX_PROVIDER_ID_CHARS)?;
        validate_endpoint(&self.normalized_effective_endpoint)?;
        validate_credential_transport(
            self.credential_transport_mode,
            &self.credential_transport_safe_header_name,
        )?;
        for capability in &self.declared_model_capability_subset {
            validate_provider_string(capability, 256)?;
        }
        validate_provider_string(&self.resolved_reasoning_policy, 256)?;
        validate_provider_string(&self.effective_execution_policy, 256)?;
        validate_provider_string(&self.effective_loopback_policy_or_not_applicable, 256)?;
        validate_provider_string(&self.provider_driver_contract_revision, 256)?;
        if let Some(source) = &self.selection_source {
            validate_provider_string(source, 256)?;
            if crate::canonical::contains_credential_shape(source) {
                return Err(CanonicalError::CredentialsForbidden);
            }
        }
        Ok(())
    }

    /// Encodes this selection into its canonical record bytes.
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
        let mut fields = self.identity_fields();
        fields.push((
            15,
            WireType::Optional,
            encode_optional_utf8(&self.selection_source),
        ));
        record(TagRegistry::PROVIDER_SELECTION_V1, 1, fields)
    }

    /// Computes this selection's canonical identity digest.
    ///
    /// The digest is computed over the same field table [`Self::encode`] uses,
    /// restricted to the identity-bearing fields 1 to 14. `selection_source`
    /// (field 15) is audit provenance, is not identity-bearing, and never
    /// reaches the digest, so the digest bytes and the encodable bytes cannot
    /// disagree.
    ///
    /// # Errors
    ///
    /// Returns the validation errors of [`Self::validate`], and
    /// `CanonicalError::DuplicateOrDescendingField` or
    /// `CanonicalError::OverLimit` only if the fixed field table were
    /// noncanonical or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn identity_digest(&self) -> Result<NamespacedDigest, CanonicalError> {
        self.validate()?;
        let mut input = CanonicalIdentityInput::new();
        for (number, wire_type, value) in self.identity_fields() {
            input = input.field(number, wire_type, value)?;
        }
        provider_selection_digest(input)
    }

    /// Returns the identity-bearing canonical field table shared by
    /// [`Self::encode`] and [`Self::identity_digest`].
    ///
    /// The table is defined once so the encoded record and its identity digest
    /// can never disagree about field numbers, wire types, order, or values.
    fn identity_fields(&self) -> Vec<(u32, WireType, Vec<u8>)> {
        vec![
            (
                1,
                WireType::Utf8,
                encode_utf8(&self.selection_canonicalization_version),
            ),
            (2, WireType::Utf8, encode_utf8(&self.profile_id)),
            (
                3,
                WireType::Utf8,
                encode_utf8(&self.provider_profile_revision_id),
            ),
            (4, WireType::Utf8, encode_utf8(&self.kind_id)),
            (
                5,
                WireType::Utf8,
                encode_utf8(&self.kind_descriptor_revision_id),
            ),
            (6, WireType::Utf8, encode_utf8(&self.model_id)),
            (
                7,
                WireType::Utf8,
                encode_utf8(&self.normalized_effective_endpoint),
            ),
            (8, WireType::U64, self.credential_transport_mode.enc()),
            (
                9,
                WireType::Optional,
                encode_optional_utf8(&self.credential_transport_safe_header_name),
            ),
            (
                10,
                WireType::List,
                encode_utf8_list(&self.declared_model_capability_subset),
            ),
            (
                11,
                WireType::Utf8,
                encode_utf8(&self.resolved_reasoning_policy),
            ),
            (
                12,
                WireType::Utf8,
                encode_utf8(&self.effective_execution_policy),
            ),
            (
                13,
                WireType::Utf8,
                encode_utf8(&self.effective_loopback_policy_or_not_applicable),
            ),
            (
                14,
                WireType::Utf8,
                encode_utf8(&self.provider_driver_contract_revision),
            ),
        ]
    }

    /// Decodes this selection from its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the record is not the
    /// provider-selection-v1 table, `CanonicalError::InvalidField` when any
    /// of the fifteen fields is absent or malformed, the validation errors of
    /// [`Self::validate`], and other `CanonicalError` values for malformed or
    /// noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 15)?;
        if reader.tag != TagRegistry::PROVIDER_SELECTION_V1 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let selection = Self {
            selection_canonicalization_version: decode_utf8(
                reader
                    .field(1, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            profile_id: decode_utf8(
                reader
                    .field(2, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            provider_profile_revision_id: decode_utf8(
                reader
                    .field(3, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            kind_id: decode_utf8(
                reader
                    .field(4, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            kind_descriptor_revision_id: decode_utf8(
                reader
                    .field(5, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            model_id: decode_utf8(
                reader
                    .field(6, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            normalized_effective_endpoint: decode_utf8(
                reader
                    .field(7, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            credential_transport_mode: CredentialTransportMode::dec(
                reader
                    .field(8, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            credential_transport_safe_header_name: decode_optional_utf8(
                reader
                    .field(9, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            declared_model_capability_subset: decode_utf8_list(
                reader
                    .field(10, WireType::List)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            resolved_reasoning_policy: decode_utf8(
                reader
                    .field(11, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            effective_execution_policy: decode_utf8(
                reader
                    .field(12, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            effective_loopback_policy_or_not_applicable: decode_utf8(
                reader
                    .field(13, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            provider_driver_contract_revision: decode_utf8(
                reader
                    .field(14, WireType::Utf8)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?
            .to_owned(),
            selection_source: decode_optional_utf8(
                reader
                    .field(15, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        selection.validate()?;
        Ok(selection)
    }
}

/// Computes the namespaced provider-selection digest over identity-bearing
/// fields only.
///
/// This is the single digest implementation behind
/// [`ProviderSelectionV1::identity_digest`]; callers obtain the input from the
/// record rather than rebuilding the field table, so one logical selection has
/// exactly one identity.
///
/// The input is a [`CanonicalIdentityInput`] so that credentials, filesystem
/// paths, display data, readiness, and current state supplied through the
/// `with_*` setters are retained for provenance only and never reach the
/// digest. The selection source is audit provenance and is not identity-bearing.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidDigest` when the namespace is invalid, and
/// the identity input's own encoding errors, which are impossible for a
/// canonical field stream.
fn provider_selection_digest(
    input: CanonicalIdentityInput,
) -> Result<NamespacedDigest, CanonicalError> {
    Digest256::for_namespace("provider-selection", &input.encode()?)
}

/// Decodes a boolean field value.
fn decode_bool_field(
    reader: &CanonicalRecordReader<'_>,
    number: u32,
) -> Result<bool, CanonicalError> {
    match reader
        .field(number, WireType::Bool)?
        .ok_or(CanonicalError::InvalidField)?
    {
        [0] => Ok(false),
        [1] => Ok(true),
        _ => Err(CanonicalError::InvalidBool),
    }
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

    fn fixture_capability_set() -> ModelCapabilitySetV1 {
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

    #[test]
    fn capability_set_round_trips_and_is_closed() {
        let capability = fixture_capability_set();
        let bytes = capability.encode().expect("capability set encodes");
        assert_eq!(
            ModelCapabilitySetV1::decode(&bytes).expect("capability set decodes"),
            capability
        );
        assert_eq!(
            ModelCapabilitySetV1::decode(&bytes)
                .expect("capability set decodes")
                .encode()
                .expect("capability set re-encodes"),
            bytes
        );
        let mut unknown = capability.clone();
        unknown.taxonomy_version = "model-capability-taxonomy-v2".to_owned();
        assert_eq!(
            unknown
                .encode()
                .expect_err("unknown taxonomy version is rejected")
                .code(),
            "provider_profile_revision_invalid"
        );
        assert!(capability.supports("text_streaming"));
        assert!(!capability.supports("structured_output"));
        assert!(!capability.supports("unknown-capability"));
    }

    #[test]
    fn capability_selection_must_be_a_subset_of_the_envelope() {
        let envelope = fixture_capability_set();
        let selection = ModelCapabilitySelectionV1 {
            taxonomy_version: MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
            descriptor_capability_envelope: envelope.clone(),
            selected_capabilities: vec!["text_streaming".to_owned(), "reasoning".to_owned()],
        };
        let bytes = selection.encode().expect("subset selection encodes");
        assert_eq!(
            ModelCapabilitySelectionV1::decode(&bytes).expect("selection decodes"),
            selection
        );
        let unsupported = ModelCapabilitySelectionV1 {
            taxonomy_version: MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
            descriptor_capability_envelope: envelope.clone(),
            selected_capabilities: vec!["structured_output".to_owned()],
        };
        assert_eq!(
            unsupported
                .encode()
                .expect_err("unsupported capability is rejected")
                .code(),
            "provider_profile_revision_invalid"
        );
        let not_declared = ModelCapabilitySelectionV1 {
            taxonomy_version: MODEL_CAPABILITY_TAXONOMY_V1.to_owned(),
            descriptor_capability_envelope: envelope,
            selected_capabilities: vec!["tool_exchange".to_owned()],
        };
        assert_eq!(
            not_declared
                .encode()
                .expect_err("undeclared capability is rejected")
                .code(),
            "provider_profile_revision_invalid"
        );
    }

    #[test]
    fn provider_selection_round_trips_and_rejects_openai_and_bad_endpoints() {
        let selection = ProviderSelectionV1 {
            selection_canonicalization_version: "1".to_owned(),
            profile_id: "profile-default".to_owned(),
            provider_profile_revision_id: "rev-0001".to_owned(),
            kind_id: "responses".to_owned(),
            kind_descriptor_revision_id: "kind-descriptor-rev-0001".to_owned(),
            model_id: "gpt-4.1".to_owned(),
            normalized_effective_endpoint: "https://api.openai.com/v1".to_owned(),
            credential_transport_mode: CredentialTransportMode::Bearer,
            credential_transport_safe_header_name: None,
            declared_model_capability_subset: vec![
                "text_input".to_owned(),
                "text_streaming".to_owned(),
            ],
            resolved_reasoning_policy: "textual-reasoning-v1".to_owned(),
            effective_execution_policy: "ordinary".to_owned(),
            effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
            provider_driver_contract_revision: "responses-1.0".to_owned(),
            selection_source: Some("catalog-rev-0001".to_owned()),
        };
        let bytes = selection.encode().expect("selection encodes");
        assert_eq!(
            ProviderSelectionV1::decode(&bytes).expect("selection decodes"),
            selection
        );
        let mut openai = selection.clone();
        openai.kind_id = "openai".to_owned();
        assert_eq!(
            openai
                .encode()
                .expect_err("openai kind id is rejected")
                .code(),
            "invalid_provider_kind"
        );
        let mut userinfo = selection.clone();
        userinfo.normalized_effective_endpoint = "https://user:pass@api.example.com/v1".to_owned();
        assert_eq!(
            userinfo
                .encode()
                .expect_err("userinfo endpoint is rejected")
                .code(),
            "invalid_endpoint"
        );
        let mut query = selection;
        query.normalized_effective_endpoint = "https://api.example.com/v1?key=value".to_owned();
        assert_eq!(
            query
                .encode()
                .expect_err("query endpoint is rejected")
                .code(),
            "invalid_endpoint"
        );
    }

    #[test]
    fn provider_selection_identity_digest_is_record_owned_and_excludes_provenance() {
        let selection = ProviderSelectionV1 {
            selection_canonicalization_version: "1".to_owned(),
            profile_id: "profile-default".to_owned(),
            provider_profile_revision_id: "rev-0001".to_owned(),
            kind_id: "responses".to_owned(),
            kind_descriptor_revision_id: "kind-descriptor-rev-0001".to_owned(),
            model_id: "gpt-4.1".to_owned(),
            normalized_effective_endpoint: "https://api.openai.com/v1".to_owned(),
            credential_transport_mode: CredentialTransportMode::Bearer,
            credential_transport_safe_header_name: None,
            declared_model_capability_subset: vec!["text_streaming".to_owned()],
            resolved_reasoning_policy: "textual-reasoning-v1".to_owned(),
            effective_execution_policy: "ordinary".to_owned(),
            effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
            provider_driver_contract_revision: "responses-1.0".to_owned(),
            selection_source: Some("catalog-rev-0001".to_owned()),
        };
        let baseline = selection.identity_digest().expect("selection digests");
        assert_eq!(baseline.namespace, "provider-selection");
        // The selection source is audit provenance and never identity-bearing.
        let mut other = selection.clone();
        other.selection_source = Some("other-catalog-rev".to_owned());
        assert_eq!(
            other.identity_digest().expect("selection digests"),
            baseline
        );
        // An identity-bearing field does move the digest.
        let mut changed = selection.clone();
        changed.model_id = "gpt-4.1-mini".to_owned();
        assert_ne!(
            changed.identity_digest().expect("selection digests"),
            baseline
        );
        // The digest is derived from the record's own field table, so the
        // canonical round trip preserves identity exactly.
        let bytes = selection.encode().expect("selection encodes");
        let decoded = ProviderSelectionV1::decode(&bytes).expect("selection decodes");
        assert_eq!(
            decoded.identity_digest().expect("selection digests"),
            baseline
        );
    }

    #[test]
    fn provider_selection_rejects_a_non_current_canonicalization_version() {
        let mut selection = ProviderSelectionV1 {
            selection_canonicalization_version: "2".to_owned(),
            profile_id: "profile-default".to_owned(),
            provider_profile_revision_id: "rev-0001".to_owned(),
            kind_id: "responses".to_owned(),
            kind_descriptor_revision_id: "kind-descriptor-rev-0001".to_owned(),
            model_id: "gpt-4.1".to_owned(),
            normalized_effective_endpoint: "https://api.openai.com/v1".to_owned(),
            credential_transport_mode: CredentialTransportMode::Bearer,
            credential_transport_safe_header_name: None,
            declared_model_capability_subset: vec!["text_streaming".to_owned()],
            resolved_reasoning_policy: "textual-reasoning-v1".to_owned(),
            effective_execution_policy: "ordinary".to_owned(),
            effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
            provider_driver_contract_revision: "responses-1.0".to_owned(),
            selection_source: None,
        };
        assert_eq!(
            selection
                .encode()
                .expect_err("a non-current canonicalization version is rejected")
                .code(),
            "provider_profile_revision_invalid"
        );
        selection.selection_canonicalization_version =
            PROVIDER_SELECTION_CANONICALIZATION_VERSION.to_owned();
        assert!(selection.encode().is_ok());
    }
}
