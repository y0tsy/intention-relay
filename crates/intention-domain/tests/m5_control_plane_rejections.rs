#![allow(
    clippy::expect_used,
    reason = "M5+ control-plane rejection fixtures use expect for precise test diagnostics."
)]

//! Slice 2 control-plane typed-TLV rejection coverage for every newly wired
//! canonical family: missing, duplicate, descending, and unknown fields;
//! wrong wire types; truncated and trailing bytes; unsupported versions;
//! invalid UTF-8; empty required strings; over-limit scalars and lists;
//! invalid digest shapes; invalid closed enums; invalid nested records; and
//! noncanonical encodings.

use intention_domain::canonical::{
    CanonicalError, TagRegistry, WireType, encode_list_items, encode_optional_utf8, encode_u64,
    encode_utf8, encode_utf8_list,
};
use intention_domain::{
    ContextPreservationCapability, ContextSourceEntryV1, ContextSourceManifestV1,
    LegacyM4SelectionBindingDto, ModelCapabilitySetV1, ModelContextProjectionV1,
    ModelInputCapability, ProviderDriverContractRevisionDto, ProviderKindDescriptorRevisionV1,
    ProviderProfileRevisionV1, ProviderSelectionV1, ReasoningCapability, ReasoningHistoryBound,
    ReasoningHistoryManifestDto, StructuredOutputCapability, context_source_manifest_digest,
    model_context_projection_digest, reasoning_history_manifest_digest,
};

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn digest_hex(digest: intention_domain::canonical::Digest256) -> String {
    hex_encode(&digest.bytes())
}

/// Renders a raw canonical record for negative framing fixtures.
fn raw_record(tag: u32, version: u32, fields: &[(u32, u8, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"IRCR");
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(&tag.to_be_bytes());
    out.extend_from_slice(&version.to_be_bytes());
    for (number, wire_type, value) in fields {
        out.extend_from_slice(&number.to_be_bytes());
        out.push(*wire_type);
        out.extend_from_slice(&(value.len() as u32).to_be_bytes());
        out.extend_from_slice(value);
    }
    out
}

/// Renders one raw field with a declared length that does not match the
/// actual payload, for truncated-field fixtures.
fn raw_field(number: u32, wire_type: u8, declared_len: u32, actual: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&number.to_be_bytes());
    out.push(wire_type);
    out.extend_from_slice(&declared_len.to_be_bytes());
    out.extend_from_slice(actual);
    out
}

fn capability_envelope() -> ModelCapabilitySetV1 {
    ModelCapabilitySetV1 {
        taxonomy_version: "model-capability-taxonomy-v1".to_owned(),
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

fn context_preservation_record() -> Vec<u8> {
    capability_envelope()
        .context_preservation
        .encode()
        .expect("context preservation encodes")
}

fn driver_contract_record() -> Vec<u8> {
    ProviderDriverContractRevisionDto {
        driver_family: "responses".to_owned(),
        major: 1,
        minor: 0,
    }
    .encode()
    .expect("driver contract encodes")
}

fn profile_fields() -> Vec<(u32, u8, Vec<u8>)> {
    vec![
        (1, WireType::Utf8 as u8, encode_utf8("profile-default")),
        (2, WireType::Utf8 as u8, encode_utf8("rev-0001")),
        (3, WireType::Utf8 as u8, encode_utf8("responses")),
        (4, WireType::Utf8 as u8, encode_utf8("gpt-4.1")),
        (
            5,
            WireType::Utf8 as u8,
            encode_utf8("https://api.openai.com/v1"),
        ),
        (6, WireType::U64 as u8, vec![0]),
        (7, WireType::Optional as u8, encode_optional_utf8(&None)),
        (
            8,
            WireType::Utf8 as u8,
            encode_utf8("model-capability-taxonomy-v1"),
        ),
        (
            9,
            WireType::Optional as u8,
            encode_optional_utf8(&Some("reasoning-compat-v1".to_owned())),
        ),
        (
            10,
            WireType::Utf8 as u8,
            encode_utf8("kind-descriptor-rev-0001"),
        ),
        (11, WireType::Record as u8, driver_contract_record()),
    ]
}

fn selection_fields() -> Vec<(u32, u8, Vec<u8>)> {
    vec![
        (1, WireType::Utf8 as u8, encode_utf8("1")),
        (2, WireType::Utf8 as u8, encode_utf8("profile-default")),
        (3, WireType::Utf8 as u8, encode_utf8("rev-0001")),
        (4, WireType::Utf8 as u8, encode_utf8("responses")),
        (
            5,
            WireType::Utf8 as u8,
            encode_utf8("kind-descriptor-rev-0001"),
        ),
        (6, WireType::Utf8 as u8, encode_utf8("gpt-4.1")),
        (
            7,
            WireType::Utf8 as u8,
            encode_utf8("https://api.openai.com/v1"),
        ),
        (8, WireType::U64 as u8, vec![0]),
        (9, WireType::Optional as u8, encode_optional_utf8(&None)),
        (
            10,
            WireType::List as u8,
            encode_utf8_list(&["text_input".to_owned(), "text_streaming".to_owned()]),
        ),
        (
            11,
            WireType::Utf8 as u8,
            encode_utf8("textual-reasoning-v1"),
        ),
        (12, WireType::Utf8 as u8, encode_utf8("ordinary")),
        (13, WireType::Utf8 as u8, encode_utf8("not-applicable")),
        (14, WireType::Utf8 as u8, encode_utf8("responses-1.0")),
        (
            15,
            WireType::Optional as u8,
            encode_optional_utf8(&Some("catalog-rev-0001".to_owned())),
        ),
    ]
}

fn manifest_fields() -> Vec<(u32, u8, Vec<u8>)> {
    let mut manifest = ReasoningHistoryManifestDto {
        compatibility_id: "reasoning-compat-v1".to_owned(),
        entries: vec![
            "session-11111111-1111-4111-8111-111111111111-run-0001".to_owned(),
            "session-11111111-1111-4111-8111-111111111111-run-0002".to_owned(),
        ],
        manifest_digest: String::new(),
        transfer_policy: "textual-history-v1".to_owned(),
        history_bound: ReasoningHistoryBound {
            max_entries: 64,
            max_aggregate_bytes: 4 * 1024 * 1024,
        },
    };
    let digest = reasoning_history_manifest_digest(&manifest).expect("manifest digests");
    manifest.manifest_digest = digest_hex(digest.digest);
    vec![
        (
            1,
            WireType::Utf8 as u8,
            encode_utf8(&manifest.compatibility_id),
        ),
        (2, WireType::List as u8, encode_utf8_list(&manifest.entries)),
        (
            3,
            WireType::Utf8 as u8,
            encode_utf8(&manifest.manifest_digest),
        ),
        (
            4,
            WireType::Utf8 as u8,
            encode_utf8(&manifest.transfer_policy),
        ),
        (
            5,
            WireType::Record as u8,
            manifest.history_bound.encode().expect("bound encodes"),
        ),
    ]
}

fn context_manifest_fields() -> Vec<(u32, u8, Vec<u8>)> {
    let mut manifest = ContextSourceManifestV1 {
        compatibility_id: "context-source-manifest-v1".to_owned(),
        source_entries: vec![ContextSourceEntryV1 {
            source_id: "session-history".to_owned(),
            source_kind: "session".to_owned(),
            revision: "rev-0001".to_owned(),
            safe_label: None,
        }],
        manifest_digest: String::new(),
    };
    let digest = context_source_manifest_digest(&manifest).expect("manifest digests");
    manifest.manifest_digest = digest_hex(digest.digest);
    let entries = manifest
        .source_entries
        .iter()
        .map(|entry| entry.encode().expect("entry encodes"))
        .collect::<Vec<_>>();
    vec![
        (
            1,
            WireType::Utf8 as u8,
            encode_utf8(&manifest.compatibility_id),
        ),
        (2, WireType::List as u8, encode_list_items(&entries)),
        (
            3,
            WireType::Utf8 as u8,
            encode_utf8(&manifest.manifest_digest),
        ),
    ]
}

fn projection_fields() -> Vec<(u32, u8, Vec<u8>)> {
    let mut projection = ModelContextProjectionV1 {
        projection_revision: "1".to_owned(),
        context_schema_version: "1".to_owned(),
        source_manifest_digest: "0".repeat(64),
        ordered_messages: vec!["hello".to_owned(), "world".to_owned()],
        model_context_digest: String::new(),
    };
    let digest = model_context_projection_digest(&projection).expect("projection digests");
    projection.model_context_digest = digest_hex(digest.digest);
    vec![
        (
            1,
            WireType::Utf8 as u8,
            encode_utf8(&projection.projection_revision),
        ),
        (
            2,
            WireType::Utf8 as u8,
            encode_utf8(&projection.context_schema_version),
        ),
        (
            3,
            WireType::Utf8 as u8,
            encode_utf8(&projection.source_manifest_digest),
        ),
        (
            4,
            WireType::List as u8,
            encode_utf8_list(&projection.ordered_messages),
        ),
        (
            5,
            WireType::Utf8 as u8,
            encode_utf8(&projection.model_context_digest),
        ),
    ]
}

fn binding_fields() -> Vec<(u32, u8, Vec<u8>)> {
    vec![
        (
            1,
            WireType::Utf8 as u8,
            encode_utf8("11111111-1111-4111-8111-111111111111"),
        ),
        (
            2,
            WireType::Utf8 as u8,
            encode_utf8("m4-config-snapshot-v1"),
        ),
        (
            3,
            WireType::Utf8 as u8,
            encode_utf8("legacy-uuid:22222222-2222-4222-8222-222222222222"),
        ),
        (4, WireType::Utf8 as u8, encode_utf8("default")),
        (5, WireType::Utf8 as u8, encode_utf8("rev-0001")),
        (
            6,
            WireType::Utf8 as u8,
            encode_utf8("kind-descriptor-rev-0001"),
        ),
        (
            7,
            WireType::List as u8,
            encode_utf8_list(&["text_input".to_owned(), "text_streaming".to_owned()]),
        ),
        (8, WireType::Utf8 as u8, encode_utf8("ordinary")),
        (9, WireType::Utf8 as u8, encode_utf8("responses-1.0")),
    ]
}

fn taxonomy_fields() -> Vec<(u32, u8, Vec<u8>)> {
    vec![
        (
            1,
            WireType::Utf8 as u8,
            encode_utf8("model-capability-taxonomy-v1"),
        ),
        (2, WireType::U64 as u8, vec![0]),
        (3, WireType::Bool as u8, vec![1]),
        (4, WireType::U64 as u8, vec![0]),
        (5, WireType::U64 as u8, vec![1]),
        (6, WireType::Bool as u8, vec![0]),
        (7, WireType::Record as u8, context_preservation_record()),
    ]
}

/// Converts owned fields into reference fields for `raw_record`.
fn refs(fields: &[(u32, u8, Vec<u8>)]) -> Vec<(u32, u8, &[u8])> {
    fields
        .iter()
        .map(|(number, wire_type, value)| (*number, *wire_type, value.as_slice()))
        .collect()
}

/// Replaces one field's wire type and value in place, preserving the strictly
/// ascending field order.
fn replace_field(
    fields: &[(u32, u8, Vec<u8>)],
    number: u32,
    wire_type: u8,
    value: Vec<u8>,
) -> Vec<(u32, u8, Vec<u8>)> {
    let mut replaced: Vec<(u32, u8, Vec<u8>)> = fields
        .iter()
        .filter(|(n, ..)| *n != number)
        .cloned()
        .collect();
    replaced.push((number, wire_type, value));
    replaced.sort_by_key(|(n, ..)| *n);
    replaced
}

fn with_invalid_utf8(fields: &[(u32, u8, Vec<u8>)], number: u32) -> Vec<(u32, u8, Vec<u8>)> {
    fields
        .iter()
        .map(|(n, wire_type, value)| {
            if *n == number && *wire_type == WireType::Utf8 as u8 {
                (*n, *wire_type, vec![0xc3, 0x28])
            } else {
                (*n, *wire_type, value.clone())
            }
        })
        .collect()
}

// ---- Model capability taxonomy (0x0206) ----

#[test]
fn taxonomy_rejects_malformed_and_noncanonical_framing() {
    let tag = TagRegistry::MODEL_CAPABILITY_TAXONOMY_V1;
    let fields = taxonomy_fields();

    // Missing required fields 1-7.
    for missing in 1..=7 {
        let partial = fields
            .iter()
            .filter(|(number, ..)| *number != missing)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            ModelCapabilitySetV1::decode(&raw_record(tag, 1, &refs(&partial)))
                .expect_err("missing field is rejected"),
            CanonicalError::InvalidField,
            "missing taxonomy field {missing}"
        );
    }
    // Duplicate field 1.
    let mut duplicate = fields.clone();
    duplicate.push((1, WireType::Utf8 as u8, encode_utf8("dup")));
    assert_eq!(
        ModelCapabilitySetV1::decode(&raw_record(tag, 1, &refs(&duplicate)))
            .expect_err("duplicate field is rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    // Descending tags.
    let descending = raw_record(
        tag,
        1,
        &[
            (2, WireType::U64 as u8, &[0]),
            (1, WireType::Utf8 as u8, b"x"),
        ],
    );
    assert_eq!(
        ModelCapabilitySetV1::decode(&descending).expect_err("descending tags are rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    // Unknown field beyond the fixed table.
    let unknown = raw_record(
        tag,
        1,
        &[
            (1, WireType::Utf8 as u8, b"x"),
            (8, WireType::U64 as u8, &[0]),
        ],
    );
    assert_eq!(
        ModelCapabilitySetV1::decode(&unknown).expect_err("unknown field is rejected"),
        CanonicalError::UnknownField(8)
    );
    // Wrong wire type on field 1.
    let wrong_wire = raw_record(tag, 1, &[(1, WireType::U64 as u8, &[0])]);
    assert_eq!(
        ModelCapabilitySetV1::decode(&wrong_wire).expect_err("wrong wire type is rejected"),
        CanonicalError::InvalidField
    );
    // Truncated field payload.
    let truncated = {
        let mut out = Vec::new();
        out.extend_from_slice(b"IRCR");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&raw_field(1, WireType::Utf8 as u8, 100, b"ab"));
        out
    };
    assert_eq!(
        ModelCapabilitySetV1::decode(&truncated).expect_err("truncated field is rejected"),
        CanonicalError::Truncated
    );
    // Trailing bytes.
    let mut trailing = raw_record(tag, 1, &refs(&fields));
    trailing.extend_from_slice(&[0xDE, 0xAD, 0xBE]);
    assert_eq!(
        ModelCapabilitySetV1::decode(&trailing).expect_err("trailing bytes are rejected"),
        CanonicalError::TrailingBytes
    );
    // Unsupported version.
    assert_eq!(
        ModelCapabilitySetV1::decode(&raw_record(tag, 2, &[])).expect_err("version is rejected"),
        CanonicalError::InvalidTag
    );
    // Invalid UTF-8 in the taxonomy version.
    let bad_utf8 = with_invalid_utf8(&fields, 1);
    assert_eq!(
        ModelCapabilitySetV1::decode(&raw_record(tag, 1, &refs(&bad_utf8)))
            .expect_err("invalid utf8 is rejected"),
        CanonicalError::InvalidUtf8
    );
    // Invalid closed enums.
    for (number, value) in [(2, 9u8), (4, 9u8), (5, 9u8)] {
        let bad_enum = replace_field(&fields, number, WireType::U64 as u8, vec![value]);
        assert_eq!(
            ModelCapabilitySetV1::decode(&raw_record(tag, 1, &refs(&bad_enum)))
                .expect_err("invalid closed enum is rejected"),
            CanonicalError::InvalidField,
            "taxonomy enum field {number}"
        );
    }
    // Invalid bool scalar.
    let bad_bool = replace_field(&fields, 3, WireType::Bool as u8, vec![2]);
    assert_eq!(
        ModelCapabilitySetV1::decode(&raw_record(tag, 1, &refs(&bad_bool)))
            .expect_err("invalid bool scalar is rejected"),
        CanonicalError::InvalidBool
    );
    // Invalid nested context-preservation record.
    let bad_nested = replace_field(
        &fields,
        7,
        WireType::Record as u8,
        raw_record(0x9999, 1, &[]),
    );
    assert_eq!(
        ModelCapabilitySetV1::decode(&raw_record(tag, 1, &refs(&bad_nested)))
            .expect_err("invalid nested record is rejected"),
        CanonicalError::InvalidTag
    );
    // Empty required taxonomy version.
    let empty = replace_field(&fields, 1, WireType::Utf8 as u8, Vec::new());
    assert_eq!(
        ModelCapabilitySetV1::decode(&raw_record(tag, 1, &refs(&empty)))
            .expect_err("empty taxonomy version is rejected")
            .code(),
        "provider_profile_revision_invalid"
    );
    // Noncanonical encoding: an over-long closed-enum scalar.
    let noncanonical = replace_field(&fields, 2, WireType::U64 as u8, vec![0, 0]);
    assert_eq!(
        ModelCapabilitySetV1::decode(&raw_record(tag, 1, &refs(&noncanonical)))
            .expect_err("noncanonical enum encoding is rejected"),
        CanonicalError::InvalidField
    );
}

// ---- Provider profile revision (0x0207) ----

#[test]
fn profile_rejects_malformed_and_noncanonical_framing() {
    let tag = TagRegistry::PROVIDER_PROFILE_REVISION_V1;
    let fields = profile_fields();

    for missing in 1..=11 {
        let partial = fields
            .iter()
            .filter(|(number, ..)| *number != missing)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            ProviderProfileRevisionV1::decode(&raw_record(tag, 1, &refs(&partial)))
                .expect_err("missing field is rejected"),
            CanonicalError::InvalidField,
            "missing profile field {missing}"
        );
    }
    let mut duplicate = fields.clone();
    duplicate.push((1, WireType::Utf8 as u8, encode_utf8("dup")));
    assert_eq!(
        ProviderProfileRevisionV1::decode(&raw_record(tag, 1, &refs(&duplicate)))
            .expect_err("duplicate field is rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    let descending = raw_record(
        tag,
        1,
        &[
            (2, WireType::Utf8 as u8, b"x"),
            (1, WireType::Utf8 as u8, b"y"),
        ],
    );
    assert_eq!(
        ProviderProfileRevisionV1::decode(&descending).expect_err("descending tags are rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    let unknown = raw_record(
        tag,
        1,
        &[
            (1, WireType::Utf8 as u8, b"x"),
            (12, WireType::U64 as u8, &[0]),
        ],
    );
    assert_eq!(
        ProviderProfileRevisionV1::decode(&unknown).expect_err("unknown field is rejected"),
        CanonicalError::UnknownField(12)
    );
    let wrong_wire = raw_record(tag, 1, &[(1, WireType::U64 as u8, &[0])]);
    assert_eq!(
        ProviderProfileRevisionV1::decode(&wrong_wire).expect_err("wrong wire type is rejected"),
        CanonicalError::InvalidField
    );
    let truncated = {
        let mut out = Vec::new();
        out.extend_from_slice(b"IRCR");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&raw_field(1, WireType::Utf8 as u8, 100, b"ab"));
        out
    };
    assert_eq!(
        ProviderProfileRevisionV1::decode(&truncated).expect_err("truncated field is rejected"),
        CanonicalError::Truncated
    );
    let mut trailing = raw_record(tag, 1, &refs(&fields));
    trailing.extend_from_slice(&[0xAA, 0xBB]);
    assert_eq!(
        ProviderProfileRevisionV1::decode(&trailing).expect_err("trailing bytes are rejected"),
        CanonicalError::TrailingBytes
    );
    assert_eq!(
        ProviderProfileRevisionV1::decode(&raw_record(tag, 2, &[]))
            .expect_err("version is rejected"),
        CanonicalError::InvalidTag
    );
    let bad_utf8 = with_invalid_utf8(&fields, 1);
    assert_eq!(
        ProviderProfileRevisionV1::decode(&raw_record(tag, 1, &refs(&bad_utf8)))
            .expect_err("invalid utf8 is rejected"),
        CanonicalError::InvalidUtf8
    );
    // Invalid credential transport enum.
    let bad_enum = replace_field(&fields, 6, WireType::U64 as u8, vec![9]);
    assert_eq!(
        ProviderProfileRevisionV1::decode(&raw_record(tag, 1, &refs(&bad_enum)))
            .expect_err("invalid transport enum is rejected"),
        CanonicalError::InvalidField
    );
    // Invalid optional marker on the safe header name.
    let bad_optional = replace_field(&fields, 7, WireType::Optional as u8, vec![2]);
    assert_eq!(
        ProviderProfileRevisionV1::decode(&raw_record(tag, 1, &refs(&bad_optional)))
            .expect_err("invalid optional marker is rejected"),
        CanonicalError::InvalidOptional
    );
    // Invalid nested driver contract record.
    let bad_nested = replace_field(
        &fields,
        11,
        WireType::Record as u8,
        raw_record(0x9999, 1, &[]),
    );
    assert_eq!(
        ProviderProfileRevisionV1::decode(&raw_record(tag, 1, &refs(&bad_nested)))
            .expect_err("invalid nested record is rejected"),
        CanonicalError::InvalidTag
    );
    // Empty required profile id.
    let empty = replace_field(&fields, 1, WireType::Utf8 as u8, Vec::new());
    assert_eq!(
        ProviderProfileRevisionV1::decode(&raw_record(tag, 1, &refs(&empty)))
            .expect_err("empty profile id is rejected")
            .code(),
        "provider_profile_revision_invalid"
    );
    // Over-limit profile id scalar.
    let over = replace_field(
        &fields,
        1,
        WireType::Utf8 as u8,
        encode_utf8(&"p".repeat(64)),
    );
    assert_eq!(
        ProviderProfileRevisionV1::decode(&raw_record(tag, 1, &refs(&over)))
            .expect_err("over-limit profile id is rejected")
            .code(),
        "provider_profile_revision_invalid"
    );
    // Noncanonical u64 inside the nested driver contract.
    let bad_contract = raw_record(
        0,
        1,
        &[
            (1, WireType::Utf8 as u8, b"responses"),
            (2, WireType::U64 as u8, &[0, 1]),
            (3, WireType::U64 as u8, &[0]),
        ],
    );
    let noncanonical = replace_field(&fields, 11, WireType::Record as u8, bad_contract);
    assert_eq!(
        ProviderProfileRevisionV1::decode(&raw_record(tag, 1, &refs(&noncanonical)))
            .expect_err("noncanonical u64 is rejected"),
        CanonicalError::InvalidField
    );
}

// ---- Provider selection (0x0208) ----

#[test]
fn selection_rejects_malformed_and_noncanonical_framing() {
    let tag = TagRegistry::PROVIDER_SELECTION_V1;
    let fields = selection_fields();

    for missing in 1..=15 {
        let partial = fields
            .iter()
            .filter(|(number, ..)| *number != missing)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            ProviderSelectionV1::decode(&raw_record(tag, 1, &refs(&partial)))
                .expect_err("missing field is rejected"),
            CanonicalError::InvalidField,
            "missing selection field {missing}"
        );
    }
    let mut duplicate = fields.clone();
    duplicate.push((1, WireType::Utf8 as u8, encode_utf8("dup")));
    assert_eq!(
        ProviderSelectionV1::decode(&raw_record(tag, 1, &refs(&duplicate)))
            .expect_err("duplicate field is rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    let descending = raw_record(
        tag,
        1,
        &[
            (2, WireType::Utf8 as u8, b"x"),
            (1, WireType::Utf8 as u8, b"y"),
        ],
    );
    assert_eq!(
        ProviderSelectionV1::decode(&descending).expect_err("descending tags are rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    let unknown = raw_record(
        tag,
        1,
        &[
            (1, WireType::Utf8 as u8, b"x"),
            (16, WireType::U64 as u8, &[0]),
        ],
    );
    assert_eq!(
        ProviderSelectionV1::decode(&unknown).expect_err("unknown field is rejected"),
        CanonicalError::UnknownField(16)
    );
    let wrong_wire = raw_record(tag, 1, &[(1, WireType::U64 as u8, &[0])]);
    assert_eq!(
        ProviderSelectionV1::decode(&wrong_wire).expect_err("wrong wire type is rejected"),
        CanonicalError::InvalidField
    );
    let truncated = {
        let mut out = Vec::new();
        out.extend_from_slice(b"IRCR");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&raw_field(1, WireType::Utf8 as u8, 100, b"ab"));
        out
    };
    assert_eq!(
        ProviderSelectionV1::decode(&truncated).expect_err("truncated field is rejected"),
        CanonicalError::Truncated
    );
    let mut trailing = raw_record(tag, 1, &refs(&fields));
    trailing.extend_from_slice(&[0x01, 0x02]);
    assert_eq!(
        ProviderSelectionV1::decode(&trailing).expect_err("trailing bytes are rejected"),
        CanonicalError::TrailingBytes
    );
    assert_eq!(
        ProviderSelectionV1::decode(&raw_record(tag, 2, &[])).expect_err("version is rejected"),
        CanonicalError::InvalidTag
    );
    let bad_utf8 = with_invalid_utf8(&fields, 2);
    assert_eq!(
        ProviderSelectionV1::decode(&raw_record(tag, 1, &refs(&bad_utf8)))
            .expect_err("invalid utf8 is rejected"),
        CanonicalError::InvalidUtf8
    );
    let bad_enum = replace_field(&fields, 8, WireType::U64 as u8, vec![7]);
    assert_eq!(
        ProviderSelectionV1::decode(&raw_record(tag, 1, &refs(&bad_enum)))
            .expect_err("invalid transport enum is rejected"),
        CanonicalError::InvalidField
    );
    let bad_optional = replace_field(&fields, 15, WireType::Optional as u8, vec![0, 0xAA]);
    assert_eq!(
        ProviderSelectionV1::decode(&raw_record(tag, 1, &refs(&bad_optional)))
            .expect_err("closed optional marker with payload is rejected"),
        CanonicalError::InvalidOptional
    );
    // Empty required string: canonicalization version.
    let empty = replace_field(&fields, 1, WireType::Utf8 as u8, Vec::new());
    assert_eq!(
        ProviderSelectionV1::decode(&raw_record(tag, 1, &refs(&empty)))
            .expect_err("empty canonicalization version is rejected")
            .code(),
        "provider_profile_revision_invalid"
    );
    // Over-limit kind id scalar.
    let over = replace_field(
        &fields,
        4,
        WireType::Utf8 as u8,
        encode_utf8(&"k".repeat(64)),
    );
    assert_eq!(
        ProviderSelectionV1::decode(&raw_record(tag, 1, &refs(&over)))
            .expect_err("over-limit kind id is rejected")
            .code(),
        "invalid_provider_kind"
    );
    // A malformed capability-subset list item is rejected.
    let mut list = (1u32).to_be_bytes().to_vec();
    list.extend_from_slice(&0x00FF_FFFFu32.to_be_bytes());
    list.extend_from_slice(&[0xAA; 8]);
    let bad_list = replace_field(&fields, 10, WireType::List as u8, list);
    assert_eq!(
        ProviderSelectionV1::decode(&raw_record(tag, 1, &refs(&bad_list)))
            .expect_err("truncated list item is rejected"),
        CanonicalError::Truncated
    );
    // Noncanonical closed-enum encoding.
    let noncanonical = replace_field(&fields, 8, WireType::U64 as u8, vec![0, 0]);
    assert_eq!(
        ProviderSelectionV1::decode(&raw_record(tag, 1, &refs(&noncanonical)))
            .expect_err("noncanonical enum encoding is rejected"),
        CanonicalError::InvalidField
    );
}

// ---- Reasoning history manifest (0x0209) ----

#[test]
fn reasoning_manifest_rejects_malformed_and_noncanonical_framing() {
    let tag = TagRegistry::REASONING_HISTORY_MANIFEST_V1;
    let fields = manifest_fields();

    for missing in 1..=5 {
        let partial = fields
            .iter()
            .filter(|(number, ..)| *number != missing)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            ReasoningHistoryManifestDto::decode(&raw_record(tag, 1, &refs(&partial)))
                .expect_err("missing field is rejected"),
            CanonicalError::InvalidField,
            "missing manifest field {missing}"
        );
    }
    let mut duplicate = fields.clone();
    duplicate.push((1, WireType::Utf8 as u8, encode_utf8("dup")));
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&raw_record(tag, 1, &refs(&duplicate)))
            .expect_err("duplicate field is rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    let descending = raw_record(
        tag,
        1,
        &[
            (2, WireType::List as u8, &(0u32).to_be_bytes()),
            (1, WireType::Utf8 as u8, b"x"),
        ],
    );
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&descending).expect_err("descending tags are rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    let unknown = raw_record(
        tag,
        1,
        &[
            (1, WireType::Utf8 as u8, b"x"),
            (6, WireType::U64 as u8, &[0]),
        ],
    );
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&unknown).expect_err("unknown field is rejected"),
        CanonicalError::UnknownField(6)
    );
    let wrong_wire = raw_record(tag, 1, &[(1, WireType::U64 as u8, &[0])]);
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&wrong_wire).expect_err("wrong wire type is rejected"),
        CanonicalError::InvalidField
    );
    let truncated = {
        let mut out = Vec::new();
        out.extend_from_slice(b"IRCR");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&raw_field(1, WireType::Utf8 as u8, 100, b"ab"));
        out
    };
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&truncated).expect_err("truncated field is rejected"),
        CanonicalError::Truncated
    );
    let mut trailing = raw_record(tag, 1, &refs(&fields));
    trailing.extend_from_slice(&[0x99, 0x88]);
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&trailing).expect_err("trailing bytes are rejected"),
        CanonicalError::TrailingBytes
    );
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&raw_record(tag, 2, &[]))
            .expect_err("version is rejected"),
        CanonicalError::InvalidTag
    );
    let bad_utf8 = with_invalid_utf8(&fields, 1);
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&raw_record(tag, 1, &refs(&bad_utf8)))
            .expect_err("invalid utf8 is rejected"),
        CanonicalError::InvalidUtf8
    );
    // Invalid digest shape.
    let bad_digest = replace_field(
        &fields,
        3,
        WireType::Utf8 as u8,
        encode_utf8("not-a-digest"),
    );
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&raw_record(tag, 1, &refs(&bad_digest)))
            .expect_err("invalid digest shape is rejected"),
        CanonicalError::InvalidDigest
    );
    // Invalid nested history-bound record.
    let bad_nested = replace_field(
        &fields,
        5,
        WireType::Record as u8,
        raw_record(0x9999, 1, &[]),
    );
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&raw_record(tag, 1, &refs(&bad_nested)))
            .expect_err("invalid nested record is rejected"),
        CanonicalError::InvalidTag
    );
    // Empty required compatibility id.
    let empty = replace_field(&fields, 1, WireType::Utf8 as u8, Vec::new());
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&raw_record(tag, 1, &refs(&empty)))
            .expect_err("empty compatibility id is rejected")
            .code(),
        "reasoning_history_incompatible"
    );
    // Entries over the declared bound.
    let mut entries = vec!["entry".to_owned(); 65];
    entries.push("extra".to_owned());
    let over = replace_field(&fields, 2, WireType::List as u8, encode_utf8_list(&entries));
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&raw_record(tag, 1, &refs(&over)))
            .expect_err("entries over the bound are rejected")
            .code(),
        "reasoning_history_too_large"
    );
    // Noncanonical u64 inside the nested history bound.
    let bad_bound = raw_record(
        0,
        1,
        &[
            (1, WireType::U64 as u8, &[0, 1]),
            (2, WireType::U64 as u8, &[0]),
        ],
    );
    let noncanonical = replace_field(&fields, 5, WireType::Record as u8, bad_bound);
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&raw_record(tag, 1, &refs(&noncanonical)))
            .expect_err("noncanonical u64 is rejected"),
        CanonicalError::InvalidField
    );
}

// ---- Context source manifest (0x020A) ----

#[test]
fn context_manifest_rejects_malformed_and_noncanonical_framing() {
    let tag = TagRegistry::CONTEXT_SOURCE_MANIFEST_V1;
    let fields = context_manifest_fields();

    for missing in 1..=3 {
        let partial = fields
            .iter()
            .filter(|(number, ..)| *number != missing)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            ContextSourceManifestV1::decode(&raw_record(tag, 1, &refs(&partial)))
                .expect_err("missing field is rejected"),
            CanonicalError::InvalidField,
            "missing manifest field {missing}"
        );
    }
    let mut duplicate = fields.clone();
    duplicate.push((1, WireType::Utf8 as u8, encode_utf8("dup")));
    assert_eq!(
        ContextSourceManifestV1::decode(&raw_record(tag, 1, &refs(&duplicate)))
            .expect_err("duplicate field is rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    let descending = raw_record(
        tag,
        1,
        &[
            (2, WireType::List as u8, &(0u32).to_be_bytes()),
            (1, WireType::Utf8 as u8, b"x"),
        ],
    );
    assert_eq!(
        ContextSourceManifestV1::decode(&descending).expect_err("descending tags are rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    let unknown = raw_record(
        tag,
        1,
        &[
            (1, WireType::Utf8 as u8, b"x"),
            (4, WireType::U64 as u8, &[0]),
        ],
    );
    assert_eq!(
        ContextSourceManifestV1::decode(&unknown).expect_err("unknown field is rejected"),
        CanonicalError::UnknownField(4)
    );
    let wrong_wire = raw_record(tag, 1, &[(1, WireType::U64 as u8, &[0])]);
    assert_eq!(
        ContextSourceManifestV1::decode(&wrong_wire).expect_err("wrong wire type is rejected"),
        CanonicalError::InvalidField
    );
    let truncated = {
        let mut out = Vec::new();
        out.extend_from_slice(b"IRCR");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&raw_field(1, WireType::Utf8 as u8, 100, b"ab"));
        out
    };
    assert_eq!(
        ContextSourceManifestV1::decode(&truncated).expect_err("truncated field is rejected"),
        CanonicalError::Truncated
    );
    let mut trailing = raw_record(tag, 1, &refs(&fields));
    trailing.extend_from_slice(&[0x11, 0x22]);
    assert_eq!(
        ContextSourceManifestV1::decode(&trailing).expect_err("trailing bytes are rejected"),
        CanonicalError::TrailingBytes
    );
    assert_eq!(
        ContextSourceManifestV1::decode(&raw_record(tag, 2, &[])).expect_err("version is rejected"),
        CanonicalError::InvalidTag
    );
    let bad_utf8 = with_invalid_utf8(&fields, 1);
    assert_eq!(
        ContextSourceManifestV1::decode(&raw_record(tag, 1, &refs(&bad_utf8)))
            .expect_err("invalid utf8 is rejected"),
        CanonicalError::InvalidUtf8
    );
    // Invalid digest shape.
    let bad_digest = replace_field(&fields, 3, WireType::Utf8 as u8, encode_utf8("ABCD"));
    assert_eq!(
        ContextSourceManifestV1::decode(&raw_record(tag, 1, &refs(&bad_digest)))
            .expect_err("invalid digest shape is rejected"),
        CanonicalError::InvalidDigest
    );
    // Malformed nested source entry.
    let bad_entry = replace_field(
        &fields,
        2,
        WireType::List as u8,
        encode_list_items(&[vec![0xDE, 0xAD]]),
    );
    assert_eq!(
        ContextSourceManifestV1::decode(&raw_record(tag, 1, &refs(&bad_entry)))
            .expect_err("malformed nested entry is rejected"),
        CanonicalError::Truncated
    );
    // Empty required compatibility id.
    let empty = replace_field(&fields, 1, WireType::Utf8 as u8, Vec::new());
    assert_eq!(
        ContextSourceManifestV1::decode(&raw_record(tag, 1, &refs(&empty)))
            .expect_err("empty compatibility id is rejected")
            .code(),
        "context_source_manifest_invalid"
    );
    // Zero entries.
    let zero = replace_field(
        &fields,
        2,
        WireType::List as u8,
        (0u32).to_be_bytes().to_vec(),
    );
    assert_eq!(
        ContextSourceManifestV1::decode(&raw_record(tag, 1, &refs(&zero)))
            .expect_err("zero entries are rejected")
            .code(),
        "context_source_manifest_invalid"
    );
    // A noncanonical entries list: items after the declared count.
    let entry = fields[1].2.clone();
    let mut list = (0u32).to_be_bytes().to_vec();
    list.extend_from_slice(&entry);
    let trailing_items = replace_field(&fields, 2, WireType::List as u8, list);
    assert_eq!(
        ContextSourceManifestV1::decode(&raw_record(tag, 1, &refs(&trailing_items)))
            .expect_err("trailing list items are rejected"),
        CanonicalError::TrailingBytes
    );
}

// ---- Model context projection (0x020B) ----

#[test]
fn projection_rejects_malformed_and_noncanonical_framing() {
    let tag = TagRegistry::MODEL_CONTEXT_PROJECTION_V1;
    let fields = projection_fields();

    for missing in 1..=5 {
        let partial = fields
            .iter()
            .filter(|(number, ..)| *number != missing)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            ModelContextProjectionV1::decode(&raw_record(tag, 1, &refs(&partial)))
                .expect_err("missing field is rejected"),
            CanonicalError::InvalidField,
            "missing projection field {missing}"
        );
    }
    let mut duplicate = fields.clone();
    duplicate.push((1, WireType::Utf8 as u8, encode_utf8("dup")));
    assert_eq!(
        ModelContextProjectionV1::decode(&raw_record(tag, 1, &refs(&duplicate)))
            .expect_err("duplicate field is rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    let descending = raw_record(
        tag,
        1,
        &[
            (2, WireType::Utf8 as u8, b"x"),
            (1, WireType::Utf8 as u8, b"y"),
        ],
    );
    assert_eq!(
        ModelContextProjectionV1::decode(&descending).expect_err("descending tags are rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    let unknown = raw_record(
        tag,
        1,
        &[
            (1, WireType::Utf8 as u8, b"x"),
            (6, WireType::U64 as u8, &[0]),
        ],
    );
    assert_eq!(
        ModelContextProjectionV1::decode(&unknown).expect_err("unknown field is rejected"),
        CanonicalError::UnknownField(6)
    );
    let wrong_wire = raw_record(tag, 1, &[(1, WireType::U64 as u8, &[0])]);
    assert_eq!(
        ModelContextProjectionV1::decode(&wrong_wire).expect_err("wrong wire type is rejected"),
        CanonicalError::InvalidField
    );
    let truncated = {
        let mut out = Vec::new();
        out.extend_from_slice(b"IRCR");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&raw_field(1, WireType::Utf8 as u8, 100, b"ab"));
        out
    };
    assert_eq!(
        ModelContextProjectionV1::decode(&truncated).expect_err("truncated field is rejected"),
        CanonicalError::Truncated
    );
    let mut trailing = raw_record(tag, 1, &refs(&fields));
    trailing.extend_from_slice(&[0x77, 0x66]);
    assert_eq!(
        ModelContextProjectionV1::decode(&trailing).expect_err("trailing bytes are rejected"),
        CanonicalError::TrailingBytes
    );
    assert_eq!(
        ModelContextProjectionV1::decode(&raw_record(tag, 2, &[]))
            .expect_err("version is rejected"),
        CanonicalError::InvalidTag
    );
    let bad_utf8 = with_invalid_utf8(&fields, 1);
    assert_eq!(
        ModelContextProjectionV1::decode(&raw_record(tag, 1, &refs(&bad_utf8)))
            .expect_err("invalid utf8 is rejected"),
        CanonicalError::InvalidUtf8
    );
    // Invalid digest shapes on both digest fields.
    for number in [3, 5] {
        let bad_digest = replace_field(&fields, number, WireType::Utf8 as u8, encode_utf8("xyz"));
        assert_eq!(
            ModelContextProjectionV1::decode(&raw_record(tag, 1, &refs(&bad_digest)))
                .expect_err("invalid digest shape is rejected"),
            CanonicalError::InvalidDigest,
            "projection digest field {number}"
        );
    }
    // Empty required projection revision.
    let empty = replace_field(&fields, 1, WireType::Utf8 as u8, Vec::new());
    assert_eq!(
        ModelContextProjectionV1::decode(&raw_record(tag, 1, &refs(&empty)))
            .expect_err("empty projection revision is rejected")
            .code(),
        "model_context_projection_invalid"
    );
    // Blank message entry.
    let blank = replace_field(
        &fields,
        4,
        WireType::List as u8,
        encode_utf8_list(&["   ".to_owned()]),
    );
    assert_eq!(
        ModelContextProjectionV1::decode(&raw_record(tag, 1, &refs(&blank)))
            .expect_err("blank message is rejected")
            .code(),
        "model_context_projection_invalid"
    );
    // Over-limit message count.
    let over = replace_field(
        &fields,
        4,
        WireType::List as u8,
        encode_utf8_list(&vec!["x".to_owned(); 1025]),
    );
    assert_eq!(
        ModelContextProjectionV1::decode(&raw_record(tag, 1, &refs(&over)))
            .expect_err("over-limit message count is rejected")
            .code(),
        "model_context_projection_invalid"
    );
    // Over-limit aggregate message bytes.
    let over_bytes = replace_field(
        &fields,
        4,
        WireType::List as u8,
        encode_utf8_list(&["x".repeat(1024 * 1024 + 1)]),
    );
    assert_eq!(
        ModelContextProjectionV1::decode(&raw_record(tag, 1, &refs(&over_bytes)))
            .expect_err("over-limit aggregate bytes are rejected")
            .code(),
        "model_context_projection_too_large"
    );
    // Noncanonical list framing: a trailing partial item.
    let mut list = (2u32).to_be_bytes().to_vec();
    list.extend_from_slice(&(5u32).to_be_bytes());
    list.extend_from_slice(b"hello");
    let noncanonical = replace_field(&fields, 4, WireType::List as u8, list);
    assert_eq!(
        ModelContextProjectionV1::decode(&raw_record(tag, 1, &refs(&noncanonical)))
            .expect_err("truncated list item is rejected"),
        CanonicalError::Truncated
    );
}

// ---- Legacy M4 selection binding (0x020C) ----

#[test]
fn legacy_binding_rejects_malformed_and_noncanonical_framing() {
    let tag = TagRegistry::LEGACY_M4_SELECTION_BINDING;
    let fields = binding_fields();

    for missing in 1..=9 {
        let partial = fields
            .iter()
            .filter(|(number, ..)| *number != missing)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            LegacyM4SelectionBindingDto::decode(&raw_record(tag, 1, &refs(&partial)))
                .expect_err("missing field is rejected"),
            CanonicalError::InvalidField,
            "missing binding field {missing}"
        );
    }
    let mut duplicate = fields.clone();
    duplicate.push((1, WireType::Utf8 as u8, encode_utf8("dup")));
    assert_eq!(
        LegacyM4SelectionBindingDto::decode(&raw_record(tag, 1, &refs(&duplicate)))
            .expect_err("duplicate field is rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    let descending = raw_record(
        tag,
        1,
        &[
            (2, WireType::Utf8 as u8, b"x"),
            (1, WireType::Utf8 as u8, b"y"),
        ],
    );
    assert_eq!(
        LegacyM4SelectionBindingDto::decode(&descending).expect_err("descending tags are rejected"),
        CanonicalError::DuplicateOrDescendingField
    );
    let unknown = raw_record(
        tag,
        1,
        &[
            (1, WireType::Utf8 as u8, b"x"),
            (10, WireType::U64 as u8, &[0]),
        ],
    );
    assert_eq!(
        LegacyM4SelectionBindingDto::decode(&unknown).expect_err("unknown field is rejected"),
        CanonicalError::UnknownField(10)
    );
    let wrong_wire = raw_record(tag, 1, &[(1, WireType::U64 as u8, &[0])]);
    assert_eq!(
        LegacyM4SelectionBindingDto::decode(&wrong_wire).expect_err("wrong wire type is rejected"),
        CanonicalError::InvalidField
    );
    let truncated = {
        let mut out = Vec::new();
        out.extend_from_slice(b"IRCR");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&raw_field(1, WireType::Utf8 as u8, 100, b"ab"));
        out
    };
    assert_eq!(
        LegacyM4SelectionBindingDto::decode(&truncated).expect_err("truncated field is rejected"),
        CanonicalError::Truncated
    );
    let mut trailing = raw_record(tag, 1, &refs(&fields));
    trailing.extend_from_slice(&[0x55, 0x44]);
    assert_eq!(
        LegacyM4SelectionBindingDto::decode(&trailing).expect_err("trailing bytes are rejected"),
        CanonicalError::TrailingBytes
    );
    assert_eq!(
        LegacyM4SelectionBindingDto::decode(&raw_record(tag, 2, &[]))
            .expect_err("version is rejected"),
        CanonicalError::InvalidTag
    );
    let bad_utf8 = with_invalid_utf8(&fields, 1);
    assert_eq!(
        LegacyM4SelectionBindingDto::decode(&raw_record(tag, 1, &refs(&bad_utf8)))
            .expect_err("invalid utf8 is rejected"),
        CanonicalError::InvalidUtf8
    );
    // An invalid legacy safe selection reference.
    let bad_reference = replace_field(
        &fields,
        3,
        WireType::Utf8 as u8,
        encode_utf8("22222222-2222-4222-8222-222222222222"),
    );
    assert_eq!(
        LegacyM4SelectionBindingDto::decode(&raw_record(tag, 1, &refs(&bad_reference)))
            .expect_err("invalid legacy reference is rejected")
            .code(),
        "legacy_selection_reference_invalid"
    );
    // Empty required legacy config revision id.
    let empty = replace_field(&fields, 1, WireType::Utf8 as u8, Vec::new());
    assert_eq!(
        LegacyM4SelectionBindingDto::decode(&raw_record(tag, 1, &refs(&empty)))
            .expect_err("empty legacy config revision id is rejected")
            .code(),
        "legacy_selection_reference_invalid"
    );
    // Over-limit scalar.
    let over = replace_field(
        &fields,
        4,
        WireType::Utf8 as u8,
        encode_utf8(&"p".repeat(257)),
    );
    assert_eq!(
        LegacyM4SelectionBindingDto::decode(&raw_record(tag, 1, &refs(&over)))
            .expect_err("over-limit scalar is rejected")
            .code(),
        "legacy_selection_reference_invalid"
    );
    // A malformed capability-subset list.
    let bad_list = replace_field(&fields, 7, WireType::List as u8, vec![0xAA, 0xBB]);
    assert_eq!(
        LegacyM4SelectionBindingDto::decode(&raw_record(tag, 1, &refs(&bad_list)))
            .expect_err("truncated capability list is rejected"),
        CanonicalError::Truncated
    );
}

// ---- Shared scalar and list codec discipline ----

#[test]
fn utf8_list_rejects_over_limit_counts_and_trailing_bytes() {
    let mut over = (intention_domain::canonical::MAX_LIST_ITEMS as u32 + 1)
        .to_be_bytes()
        .to_vec();
    assert_eq!(
        intention_domain::canonical::decode_list_items(&over)
            .expect_err("over-limit list count is rejected"),
        CanonicalError::OverLimit
    );
    over = (0u32).to_be_bytes().to_vec();
    over.extend_from_slice(&[0x01]);
    assert_eq!(
        intention_domain::canonical::decode_list_items(&over)
            .expect_err("bytes after the declared items are rejected"),
        CanonicalError::TrailingBytes
    );
    let mut truncated = (1u32).to_be_bytes().to_vec();
    truncated.extend_from_slice(&(10u32).to_be_bytes());
    truncated.extend_from_slice(&[0x61]);
    assert_eq!(
        intention_domain::canonical::decode_list_items(&truncated)
            .expect_err("truncated list item is rejected"),
        CanonicalError::Truncated
    );
}

#[test]
fn optional_string_rejects_bad_markers_and_payloads() {
    assert_eq!(
        intention_domain::canonical::decode_optional_utf8(&[])
            .expect_err("missing marker is rejected"),
        CanonicalError::InvalidOptional
    );
    assert_eq!(
        intention_domain::canonical::decode_optional_utf8(&[0, 0xAA])
            .expect_err("closed marker with payload is rejected"),
        CanonicalError::InvalidOptional
    );
    assert_eq!(
        intention_domain::canonical::decode_optional_utf8(&[1, 0xc3, 0x28])
            .expect_err("invalid utf8 payload is rejected"),
        CanonicalError::InvalidUtf8
    );
    assert_eq!(
        intention_domain::canonical::decode_optional_utf8(&[2])
            .expect_err("unknown marker is rejected"),
        CanonicalError::InvalidOptional
    );
    assert_eq!(
        intention_domain::canonical::decode_optional_utf8(&[1])
            .expect("open marker with empty payload decodes"),
        Some(String::new())
    );
    assert_eq!(
        intention_domain::canonical::decode_optional_utf8(&[0]).expect("closed marker decodes"),
        None
    );
}

#[test]
fn u64_encoding_is_minimal_and_strict() {
    assert_eq!(encode_u64(0), vec![0]);
    assert_eq!(encode_u64(1), vec![1]);
    assert_eq!(encode_u64(300), vec![1, 44]);
    assert_eq!(
        intention_domain::canonical::decode_u64(&[]).expect_err("empty u64 is rejected"),
        CanonicalError::InvalidField
    );
    assert_eq!(
        intention_domain::canonical::decode_u64(&[0, 1]).expect_err("non-minimal u64 is rejected"),
        CanonicalError::InvalidField
    );
    assert_eq!(
        intention_domain::canonical::decode_u64(&[1, 2, 3, 4, 5, 6, 7, 8, 9])
            .expect_err("over-long u64 is rejected"),
        CanonicalError::InvalidField
    );
    assert_eq!(
        intention_domain::canonical::decode_u64(&[0x01, 0x2c]).expect("u64 decodes"),
        300
    );
}

#[test]
fn new_error_codes_are_stable_and_never_leak_values() {
    for (error, expected) in [
        (CanonicalError::InvalidProviderKind, "invalid_provider_kind"),
        (CanonicalError::InvalidEndpoint, "invalid_endpoint"),
        (
            CanonicalError::ProviderProfileRevisionInvalid,
            "provider_profile_revision_invalid",
        ),
        (
            CanonicalError::ContextSourceManifestInvalid,
            "context_source_manifest_invalid",
        ),
        (
            CanonicalError::ModelContextProjectionInvalid,
            "model_context_projection_invalid",
        ),
        (
            CanonicalError::ModelContextProjectionTooLarge,
            "model_context_projection_too_large",
        ),
        (
            CanonicalError::LegacySelectionReferenceInvalid,
            "legacy_selection_reference_invalid",
        ),
        (
            CanonicalError::CredentialsForbidden,
            "credentials_forbidden",
        ),
        (
            CanonicalError::ProviderKindImmutableMismatch,
            "provider_kind_immutable_mismatch",
        ),
        (
            CanonicalError::ProviderKindHasDependents,
            "provider_kind_has_dependents",
        ),
        (
            CanonicalError::ReasoningHistoryUnavailable,
            "reasoning_history_unavailable",
        ),
        (
            CanonicalError::ReasoningHistoryIncompatible,
            "reasoning_history_incompatible",
        ),
        (
            CanonicalError::ReasoningHistoryTooLarge,
            "reasoning_history_too_large",
        ),
        (
            CanonicalError::ReasoningOutputLimitExceeded,
            "reasoning_output_limit_exceeded",
        ),
        (
            CanonicalError::ProviderReasoningStreamInvalid,
            "provider_reasoning_stream_invalid",
        ),
        (CanonicalError::InvalidDigest, "invalid_digest"),
    ] {
        assert_eq!(error.code(), expected);
        assert_eq!(error.to_string(), expected, "display never leaks values");
    }
}

// ---- Kind descriptor (nested) framing ----

#[test]
fn kind_descriptor_rejects_malformed_framing() {
    let fields: Vec<(u32, u8, Vec<u8>)> = vec![
        (1, WireType::Utf8 as u8, encode_utf8("responses")),
        (2, WireType::Utf8 as u8, encode_utf8("responses-descriptor")),
        (
            3,
            WireType::List as u8,
            encode_utf8_list(&["parts-v1".to_owned()]),
        ),
        (4, WireType::Utf8 as u8, encode_utf8("https-only")),
        (
            5,
            WireType::Utf8 as u8,
            encode_utf8("bearer-or-safe-header"),
        ),
        (
            6,
            WireType::Record as u8,
            capability_envelope().encode().expect("taxonomy encodes"),
        ),
        (7, WireType::Utf8 as u8, encode_utf8("responses")),
    ];
    for missing in 1..=7 {
        let partial = fields
            .iter()
            .filter(|(number, ..)| *number != missing)
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            ProviderKindDescriptorRevisionV1::decode(&raw_record(0, 1, &refs(&partial)))
                .expect_err("missing field is rejected"),
            CanonicalError::InvalidField,
            "missing descriptor field {missing}"
        );
    }
    let unknown = raw_record(
        0,
        1,
        &[
            (1, WireType::Utf8 as u8, b"x"),
            (8, WireType::U64 as u8, &[0]),
        ],
    );
    assert_eq!(
        ProviderKindDescriptorRevisionV1::decode(&unknown).expect_err("unknown field is rejected"),
        CanonicalError::UnknownField(8)
    );
    // A kind descriptor is only valid as the anonymous tag-zero frame.
    let wrong_tag = raw_record(0x9999, 1, &refs(&fields));
    assert_eq!(
        ProviderKindDescriptorRevisionV1::decode(&wrong_tag)
            .expect_err("wrong nested tag is rejected"),
        CanonicalError::InvalidTag
    );
    // An empty kind id is rejected with the kind policy code.
    let empty = replace_field(&fields, 1, WireType::Utf8 as u8, Vec::new());
    assert_eq!(
        ProviderKindDescriptorRevisionV1::decode(&raw_record(0, 1, &refs(&empty)))
            .expect_err("empty kind id is rejected")
            .code(),
        "invalid_provider_kind"
    );
    // A nested capability envelope with the wrong tag is rejected.
    let bad_envelope = replace_field(
        &fields,
        6,
        WireType::Record as u8,
        raw_record(0x9999, 1, &[]),
    );
    assert_eq!(
        ProviderKindDescriptorRevisionV1::decode(&raw_record(0, 1, &refs(&bad_envelope)))
            .expect_err("invalid nested envelope is rejected"),
        CanonicalError::InvalidTag
    );
}
