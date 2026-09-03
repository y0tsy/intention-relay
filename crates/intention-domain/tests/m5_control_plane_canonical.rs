#![allow(
    clippy::expect_used,
    reason = "M5+ control-plane canonical fixtures use expect for precise test diagnostics."
)]

//! Slice 2 control-plane domain canonical compatibility: registry, round
//! trips, golden bytes/digests, invariants, limits, digest preservation, and
//! cross-boundary safety.

use intention_domain::canonical::{
    CanonicalError, CanonicalIdentityInput, CanonicalRecordReader, TagRegistry, TagStatus,
    WireType, encode_optional_utf8, encode_utf8, encode_utf8_list,
};
use intention_domain::{
    ContextPreservationCapability, ContextSourceEntryV1, ContextSourceManifestV1,
    CredentialTransportMode, ModelCapabilitySelectionV1, ModelCapabilitySetV1,
    ModelContextProjectionV1, ModelInputCapability, ProviderCatalogLimits,
    ProviderDriverContractRevisionDto, ProviderKindDescriptorRevisionV1, ProviderKindTombstoneDto,
    ProviderProfileRevisionV1, ProviderProfileTombstoneDto, ProviderSelectionV1,
    ReasoningCapability, ReasoningHistoryBound, ReasoningHistoryManifestDto,
    StructuredOutputCapability, context_source_manifest_digest, model_context_projection_digest,
    provider_profile_revision_digest, provider_selection_digest, reasoning_history_manifest_digest,
    validate_endpoint, validate_provider_kind_id, validate_provider_kind_removal,
    validate_provider_kind_revision_immutability, validate_safe_header_name,
};
use sha2::{Digest, Sha256};

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(hex: &str) -> Vec<u8> {
    assert!(
        hex.len().is_multiple_of(2),
        "golden bytes_hex must have an even number of hex digits"
    );
    (0..hex.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&hex[index..index + 2], 16).expect("golden bytes_hex digits are hex")
        })
        .collect()
}

fn digest_hex(digest: intention_domain::canonical::Digest256) -> String {
    hex_encode(&digest.bytes())
}

fn transport_mode_bytes(mode: CredentialTransportMode) -> Vec<u8> {
    match mode {
        CredentialTransportMode::Bearer => vec![0],
        CredentialTransportMode::SafeHeader => vec![1],
    }
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

fn profile_revision() -> ProviderProfileRevisionV1 {
    ProviderProfileRevisionV1 {
        profile_id: "profile-default".to_owned(),
        revision_id: "rev-0001".to_owned(),
        provider_kind_id: "responses".to_owned(),
        model_id: "gpt-4.1".to_owned(),
        endpoint: "https://api.openai.com/v1".to_owned(),
        credential_transport_mode: CredentialTransportMode::Bearer,
        safe_header_name: None,
        capability_taxonomy_revision: "model-capability-taxonomy-v1".to_owned(),
        reasoning_compatibility_id: Some("reasoning-compat-v1".to_owned()),
        kind_descriptor_revision_id: "kind-descriptor-rev-0001".to_owned(),
        driver_contract_revision: ProviderDriverContractRevisionDto {
            driver_family: "responses".to_owned(),
            major: 1,
            minor: 0,
        },
    }
}

fn provider_selection() -> ProviderSelectionV1 {
    ProviderSelectionV1 {
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
            "reasoning".to_owned(),
            "context_preservation".to_owned(),
        ],
        resolved_reasoning_policy: "textual-reasoning-v1".to_owned(),
        effective_execution_policy: "ordinary".to_owned(),
        effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
        provider_driver_contract_revision: "responses-1.0".to_owned(),
        selection_source: Some("catalog-rev-0001".to_owned()),
    }
}

fn reasoning_manifest() -> ReasoningHistoryManifestDto {
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
    manifest
}

fn context_manifest() -> ContextSourceManifestV1 {
    let mut manifest = ContextSourceManifestV1 {
        compatibility_id: "context-source-manifest-v1".to_owned(),
        source_entries: vec![
            ContextSourceEntryV1 {
                source_id: "session-history".to_owned(),
                source_kind: "session".to_owned(),
                revision: "rev-0001".to_owned(),
                safe_label: Some("Session history".to_owned()),
            },
            ContextSourceEntryV1 {
                source_id: "project-facts".to_owned(),
                source_kind: "project".to_owned(),
                revision: "rev-0002".to_owned(),
                safe_label: None,
            },
        ],
        manifest_digest: String::new(),
    };
    let digest = context_source_manifest_digest(&manifest).expect("manifest digests");
    manifest.manifest_digest = digest_hex(digest.digest);
    manifest
}

fn context_projection() -> ModelContextProjectionV1 {
    let mut projection = ModelContextProjectionV1 {
        projection_revision: "1".to_owned(),
        context_schema_version: "1".to_owned(),
        source_manifest_digest: digest_hex(
            context_source_manifest_digest(&context_manifest())
                .expect("manifest digests")
                .digest,
        ),
        ordered_messages: vec![
            "user: inspect the plan".to_owned(),
            "assistant: the plan is ready".to_owned(),
        ],
        model_context_digest: String::new(),
    };
    let digest = model_context_projection_digest(&projection).expect("projection digests");
    projection.model_context_digest = digest_hex(digest.digest);
    projection
}

fn kind_descriptor() -> ProviderKindDescriptorRevisionV1 {
    ProviderKindDescriptorRevisionV1 {
        kind_id: "responses".to_owned(),
        descriptor_family: "responses-descriptor".to_owned(),
        ordered_protocol_part_revisions: vec!["parts-v1".to_owned()],
        endpoint_policy: "https-only".to_owned(),
        credential_transport_contract: "bearer-or-safe-header".to_owned(),
        model_capability_envelope: capability_envelope(),
        driver_contract_family: "responses".to_owned(),
    }
}

/// One parsed golden fixture file.
struct GoldenFixture {
    record: String,
    tag: u32,
    record_version: u32,
    bytes_hex: String,
    sha256: String,
}

/// Parses the line-oriented golden fixture format.
fn parse_golden(text: &str) -> Result<GoldenFixture, String> {
    let mut fixture = GoldenFixture {
        record: String::new(),
        tag: 0,
        record_version: 0,
        bytes_hex: String::new(),
        sha256: String::new(),
    };
    for line in text.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| "golden lines are key=value".to_owned())?;
        match key {
            "format" if value == "typed-tlv-v1" => {}
            "format" => return Err(format!("unexpected golden format: {value}")),
            "record" => fixture.record = value.to_owned(),
            "tag" => {
                fixture.tag = u32::from_str_radix(value.trim_start_matches("0x"), 16)
                    .map_err(|_| format!("golden tag is not hex: {value}"))?
            }
            "record_version" => {
                fixture.record_version = value
                    .parse()
                    .map_err(|_| format!("golden record_version is not a u32: {value}"))?
            }
            "bytes_hex" => fixture.bytes_hex = value.to_owned(),
            "sha256" => fixture.sha256 = value.to_owned(),
            other => return Err(format!("unknown golden key: {other}")),
        }
    }
    Ok(fixture)
}

/// Asserts one golden fixture matches the encoder output byte-for-byte and
/// that the fixture SHA-256 is the digest of exactly those bytes.
fn assert_golden_fixture(
    text: &str,
    expected_record: &str,
    expected_tag: u32,
    expected_version: u32,
    encoded: &[u8],
) {
    let fixture = parse_golden(text).expect("golden fixture parses");
    assert_eq!(fixture.record, expected_record);
    assert_eq!(fixture.tag, expected_tag);
    assert_eq!(fixture.record_version, expected_version);
    let golden_bytes = hex_decode(&fixture.bytes_hex);
    assert_eq!(
        encoded, golden_bytes,
        "encoded bytes must equal the golden bytes_hex"
    );
    let digest: [u8; 32] = Sha256::digest(&golden_bytes).into();
    assert_eq!(
        hex_encode(&digest),
        fixture.sha256,
        "sha256 of the golden bytes must equal the golden sha256"
    );
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

fn all_family_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in [
        capability_envelope().encode().expect("taxonomy encodes"),
        kind_descriptor().encode().expect("kind descriptor encodes"),
        profile_revision().encode().expect("profile encodes"),
        provider_selection().encode().expect("selection encodes"),
        reasoning_manifest().encode().expect("manifest encodes"),
        context_manifest().encode().expect("manifest encodes"),
        context_projection().encode().expect("projection encodes"),
    ] {
        bytes.extend_from_slice(&record);
    }
    bytes
}

// ---- Registry ----

#[test]
fn slice2_registry_has_exactly_nine_wired_tags() {
    let wired: Vec<&intention_domain::canonical::LedgerTag> = TagRegistry::LEDGER
        .iter()
        .filter(|entry| entry.status == TagStatus::Wired)
        .collect();
    assert_eq!(wired.len(), 9, "exactly nine families are wired in Slice 2");
    let names: Vec<&str> = wired.iter().map(|entry| entry.name).collect();
    assert_eq!(
        names,
        vec![
            "run-execution-meaning",
            "programmatic-caller-policy-selection-v1",
            "agent-activity-selection-v1",
            "model-capability-taxonomy-v1",
            "provider-profile-revision-v1",
            "provider-selection-v1",
            "reasoning-history-manifest-v1",
            "context-source-manifest-v1",
            "model-context-projection-v1",
        ]
    );
    let values: Vec<u32> = wired.iter().map(|entry| entry.value).collect();
    assert_eq!(
        values,
        vec![
            0x0101, 0x0201, 0x0202, 0x0206, 0x0207, 0x0208, 0x0209, 0x020A, 0x020B
        ]
    );
}

#[test]
fn slice2_registry_classifies_future_tags_by_slice() {
    let status_of = |tag: u32| -> Option<TagStatus> {
        TagRegistry::LEDGER
            .iter()
            .find(|entry| entry.value == tag)
            .map(|entry| entry.status)
    };
    for tag in [0x0203, 0x0204, 0x0205, 0x0301, 0x0302, 0x0303, 0x0304] {
        assert_eq!(
            status_of(tag),
            Some(TagStatus::ReservedForSlice3),
            "tag 0x{tag:04x} must be reserved for Slice 3"
        );
    }
    for tag in [
        0x0401, 0x0402, 0x0403, 0x0501, 0x0502, 0x0503, 0x0504, 0x0505,
    ] {
        assert_eq!(
            status_of(tag),
            Some(TagStatus::ReservedForSlice4),
            "tag 0x{tag:04x} must be reserved for Slice 4"
        );
    }
    for tag in [
        0x0101, 0x0201, 0x0202, 0x0206, 0x0207, 0x0208, 0x0209, 0x020A, 0x020B,
    ] {
        assert_eq!(status_of(tag), Some(TagStatus::Wired), "tag 0x{tag:04x}");
    }
}

#[test]
fn registry_constants_appear_once() {
    let mut values = Vec::new();
    for entry in TagRegistry::LEDGER {
        assert!(
            !values.contains(&entry.value),
            "ledger tag value 0x{:04x} appears more than once",
            entry.value
        );
        values.push(entry.value);
    }
    let constants: [u32; 24] = [
        TagRegistry::RUN_EXECUTION_MEANING,
        TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
        TagRegistry::AGENT_ACTIVITY_SELECTION_V1,
        TagRegistry::GOAL_RUN_SELECTION_V1,
        TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
        TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1,
        TagRegistry::MODEL_CAPABILITY_TAXONOMY_V1,
        TagRegistry::PROVIDER_PROFILE_REVISION_V1,
        TagRegistry::PROVIDER_SELECTION_V1,
        TagRegistry::REASONING_HISTORY_MANIFEST_V1,
        TagRegistry::CONTEXT_SOURCE_MANIFEST_V1,
        TagRegistry::MODEL_CONTEXT_PROJECTION_V1,
        TagRegistry::TOOL_DESCRIPTOR_REVISION,
        TagRegistry::TOOL_REGISTRY_REVISION,
        TagRegistry::MODEL_TOOL_LOOP_V1,
        TagRegistry::BRIDGE_INVOCATION_V1,
        TagRegistry::FORK_BASE_SNAPSHOT_V1,
        TagRegistry::FORK_PREVIEW_V1,
        TagRegistry::FORK_COMMAND_V1,
        TagRegistry::AGENT_ACTIVITY_TREE_V1,
        TagRegistry::AGENT_ACTIVITY_PAIR_V1,
        TagRegistry::AGENT_MESSAGE_V1,
        TagRegistry::AGENT_ACTIVITY_JOURNAL_RECORD_V1,
        TagRegistry::AGENT_NOTIFICATION_RECORD_V1,
    ];
    assert_eq!(TagRegistry::LEDGER.len(), constants.len());
    for constant in constants {
        assert!(
            TagRegistry::LEDGER
                .iter()
                .any(|entry| entry.value == constant),
            "registry constant 0x{constant:04x} is missing from the ledger table"
        );
    }
}

#[test]
fn reserved_tags_are_not_active_capabilities() {
    let reserved: Vec<u32> = TagRegistry::LEDGER
        .iter()
        .filter(|entry| entry.status != TagStatus::Wired)
        .map(|entry| entry.value)
        .collect();
    assert_eq!(reserved.len(), 15);
    type Decoder = fn(&[u8]) -> Result<(), CanonicalError>;
    let decoders: [(&str, Decoder); 6] = [
        ("taxonomy", |bytes| {
            ModelCapabilitySetV1::decode(bytes).map(|_| ())
        }),
        ("profile", |bytes| {
            ProviderProfileRevisionV1::decode(bytes).map(|_| ())
        }),
        ("selection", |bytes| {
            ProviderSelectionV1::decode(bytes).map(|_| ())
        }),
        ("reasoning manifest", |bytes| {
            ReasoningHistoryManifestDto::decode(bytes).map(|_| ())
        }),
        ("context manifest", |bytes| {
            ContextSourceManifestV1::decode(bytes).map(|_| ())
        }),
        ("context projection", |bytes| {
            ModelContextProjectionV1::decode(bytes).map(|_| ())
        }),
    ];
    for tag in reserved {
        let bytes = raw_record(tag, 1, &[]);
        for (name, decoder) in decoders {
            assert_eq!(
                decoder(&bytes).expect_err("reserved tag is not decodable"),
                CanonicalError::InvalidTag,
                "{name} must reject reserved tag 0x{tag:04x}"
            );
        }
    }
}

// ---- Round trips ----

#[test]
fn model_capability_taxonomy_round_trips_exactly() {
    let record = capability_envelope();
    let bytes = record.encode().expect("taxonomy encodes");
    let decoded = ModelCapabilitySetV1::decode(&bytes).expect("taxonomy decodes");
    assert_eq!(decoded, record);
    assert_eq!(
        decoded.encode().expect("taxonomy re-encodes"),
        bytes,
        "decode and re-encode must reproduce the exact input bytes"
    );
}

#[test]
fn provider_kind_descriptor_round_trips_exactly() {
    let record = kind_descriptor();
    let bytes = record.encode().expect("kind descriptor encodes");
    let decoded = ProviderKindDescriptorRevisionV1::decode(&bytes).expect("descriptor decodes");
    assert_eq!(decoded, record);
    assert_eq!(
        decoded.encode().expect("descriptor re-encodes"),
        bytes,
        "decode and re-encode must reproduce the exact input bytes"
    );
}

#[test]
fn provider_profile_revision_round_trips_exactly() {
    let record = profile_revision();
    let bytes = record.encode().expect("profile encodes");
    let decoded = ProviderProfileRevisionV1::decode(&bytes).expect("profile decodes");
    assert_eq!(decoded, record);
    assert_eq!(
        decoded.encode().expect("profile re-encodes"),
        bytes,
        "decode and re-encode must reproduce the exact input bytes"
    );
}

#[test]
fn provider_selection_round_trips_exactly() {
    let record = provider_selection();
    let bytes = record.encode().expect("selection encodes");
    let decoded = ProviderSelectionV1::decode(&bytes).expect("selection decodes");
    assert_eq!(decoded, record);
    assert_eq!(
        decoded.encode().expect("selection re-encodes"),
        bytes,
        "decode and re-encode must reproduce the exact input bytes"
    );
}

#[test]
fn reasoning_history_manifest_round_trips_exactly() {
    let record = reasoning_manifest();
    let bytes = record.encode().expect("manifest encodes");
    let decoded = ReasoningHistoryManifestDto::decode(&bytes).expect("manifest decodes");
    assert_eq!(decoded, record);
    assert_eq!(
        decoded.encode().expect("manifest re-encodes"),
        bytes,
        "decode and re-encode must reproduce the exact input bytes"
    );
}

#[test]
fn context_source_manifest_round_trips_exactly() {
    let record = context_manifest();
    let bytes = record.encode().expect("manifest encodes");
    let decoded = ContextSourceManifestV1::decode(&bytes).expect("manifest decodes");
    assert_eq!(decoded, record);
    assert_eq!(
        decoded.encode().expect("manifest re-encodes"),
        bytes,
        "decode and re-encode must reproduce the exact input bytes"
    );
}

#[test]
fn model_context_projection_round_trips_exactly() {
    let record = context_projection();
    let bytes = record.encode().expect("projection encodes");
    let decoded = ModelContextProjectionV1::decode(&bytes).expect("projection decodes");
    assert_eq!(decoded, record);
    assert_eq!(
        decoded.encode().expect("projection re-encodes"),
        bytes,
        "decode and re-encode must reproduce the exact input bytes"
    );
}

// ---- Golden compatibility ----

#[test]
fn golden_model_capability_taxonomy_matches_the_encoder_and_round_trips() {
    let record = capability_envelope();
    let encoded = record.encode().expect("taxonomy encodes");
    assert_golden_fixture(
        include_str!("fixtures/goldens/model-capability-taxonomy-v1.txt"),
        "model-capability-taxonomy-v1",
        0x0206,
        1,
        &encoded,
    );
    assert_eq!(
        ModelCapabilitySetV1::decode(&encoded).expect("golden taxonomy decodes"),
        record
    );
}

#[test]
fn golden_provider_profile_revision_matches_the_encoder_and_round_trips() {
    let record = profile_revision();
    let encoded = record.encode().expect("profile encodes");
    assert_golden_fixture(
        include_str!("fixtures/goldens/provider-profile-revision-v1.txt"),
        "provider-profile-revision-v1",
        0x0207,
        1,
        &encoded,
    );
    assert_eq!(
        ProviderProfileRevisionV1::decode(&encoded).expect("golden profile decodes"),
        record
    );
}

#[test]
fn golden_provider_selection_matches_the_encoder_and_round_trips() {
    let record = provider_selection();
    let encoded = record.encode().expect("selection encodes");
    assert_golden_fixture(
        include_str!("fixtures/goldens/provider-selection-v1.txt"),
        "provider-selection-v1",
        0x0208,
        1,
        &encoded,
    );
    assert_eq!(
        ProviderSelectionV1::decode(&encoded).expect("golden selection decodes"),
        record
    );
}

#[test]
fn golden_reasoning_history_manifest_matches_the_encoder_and_round_trips() {
    let record = reasoning_manifest();
    let encoded = record.encode().expect("manifest encodes");
    assert_golden_fixture(
        include_str!("fixtures/goldens/reasoning-history-manifest-v1.txt"),
        "reasoning-history-manifest-v1",
        0x0209,
        1,
        &encoded,
    );
    assert_eq!(
        ReasoningHistoryManifestDto::decode(&encoded).expect("golden manifest decodes"),
        record
    );
}

#[test]
fn golden_context_source_manifest_matches_the_encoder_and_round_trips() {
    let record = context_manifest();
    let encoded = record.encode().expect("manifest encodes");
    assert_golden_fixture(
        include_str!("fixtures/goldens/context-source-manifest-v1.txt"),
        "context-source-manifest-v1",
        0x020A,
        1,
        &encoded,
    );
    assert_eq!(
        ContextSourceManifestV1::decode(&encoded).expect("golden manifest decodes"),
        record
    );
}

#[test]
fn golden_model_context_projection_matches_the_encoder_and_round_trips() {
    let record = context_projection();
    let encoded = record.encode().expect("projection encodes");
    assert_golden_fixture(
        include_str!("fixtures/goldens/model-context-projection-v1.txt"),
        "model-context-projection-v1",
        0x020B,
        1,
        &encoded,
    );
    assert_eq!(
        ModelContextProjectionV1::decode(&encoded).expect("golden projection decodes"),
        record
    );
}

// ---- Invariants ----

#[test]
fn capability_taxonomy_version_is_closed() {
    let mut unknown = capability_envelope();
    unknown.taxonomy_version = "model-capability-taxonomy-v2".to_owned();
    assert_eq!(
        unknown
            .encode()
            .expect_err("unknown taxonomy version is rejected")
            .code(),
        "provider_profile_revision_invalid"
    );
    let selection = ModelCapabilitySelectionV1 {
        taxonomy_version: "model-capability-taxonomy-v2".to_owned(),
        descriptor_capability_envelope: capability_envelope(),
        selected_capabilities: Vec::new(),
    };
    assert_eq!(
        selection
            .encode()
            .expect_err("unknown selection taxonomy is rejected")
            .code(),
        "provider_profile_revision_invalid"
    );
    let mut profile = profile_revision();
    profile.capability_taxonomy_revision = "other-taxonomy-v1".to_owned();
    assert_eq!(
        profile
            .encode()
            .expect_err("unknown profile taxonomy revision is rejected")
            .code(),
        "provider_profile_revision_invalid"
    );
}

#[test]
fn selection_capabilities_must_be_subset_of_envelope() {
    let envelope = capability_envelope();
    let selection = ModelCapabilitySelectionV1 {
        taxonomy_version: "model-capability-taxonomy-v1".to_owned(),
        descriptor_capability_envelope: envelope.clone(),
        selected_capabilities: vec!["text_streaming".to_owned(), "reasoning".to_owned()],
    };
    let bytes = selection.encode().expect("subset selection encodes");
    assert_eq!(
        ModelCapabilitySelectionV1::decode(&bytes).expect("subset selection decodes"),
        selection
    );
    for capability in [
        "structured_output",
        "tool_exchange",
        "not-a-capability",
        "reasoning_details",
    ] {
        let invalid = ModelCapabilitySelectionV1 {
            taxonomy_version: "model-capability-taxonomy-v1".to_owned(),
            descriptor_capability_envelope: envelope.clone(),
            selected_capabilities: vec![capability.to_owned()],
        };
        assert_eq!(
            invalid
                .encode()
                .expect_err("capability outside the envelope is rejected")
                .code(),
            "provider_profile_revision_invalid",
            "capability {capability}"
        );
    }
}

#[test]
fn provider_selection_rejects_openai_kind_id() {
    let mut selection = provider_selection();
    selection.kind_id = "openai".to_owned();
    assert_eq!(
        selection
            .encode()
            .expect_err("openai kind id is rejected")
            .code(),
        "invalid_provider_kind"
    );
    assert_eq!(
        validate_provider_kind_id("openai")
            .expect_err("openai is rejected")
            .code(),
        "invalid_provider_kind"
    );
}

#[test]
fn provider_selection_rejects_invalid_endpoint() {
    let mut selection = provider_selection();
    selection.normalized_effective_endpoint = "https://user:pass@api.example.com/v1".to_owned();
    assert_eq!(
        selection
            .encode()
            .expect_err("userinfo endpoint is rejected")
            .code(),
        "invalid_endpoint"
    );
    selection.normalized_effective_endpoint = "https://api.example.com/v1?x=1".to_owned();
    assert_eq!(
        selection
            .encode()
            .expect_err("query endpoint is rejected")
            .code(),
        "invalid_endpoint"
    );
    selection.normalized_effective_endpoint = "https://api.example.com/v1#x".to_owned();
    assert_eq!(
        selection
            .encode()
            .expect_err("fragment endpoint is rejected")
            .code(),
        "invalid_endpoint"
    );
}

#[test]
fn provider_profile_rejects_endpoint_userinfo_query_fragment() {
    for endpoint in [
        "https://user:pass@api.example.com/v1",
        "https://api.example.com/v1?x=1",
        "https://api.example.com/v1#x",
        "https://api.example.com/\x01",
    ] {
        let mut profile = profile_revision();
        profile.endpoint = endpoint.to_owned();
        assert_eq!(
            profile
                .encode()
                .expect_err("invalid profile endpoint is rejected")
                .code(),
            "invalid_endpoint",
            "endpoint {endpoint}"
        );
    }
    assert!(validate_endpoint("https://api.openai.com/v1").is_ok());
}

#[test]
fn safe_header_name_is_name_only() {
    assert!(validate_safe_header_name("X-Custom-Auth").is_ok());
    for invalid in [
        "Authorization: Bearer x",
        "name=value",
        "has space",
        "semi;colon",
        "quote\"value",
        "",
    ] {
        assert!(
            validate_safe_header_name(invalid).is_err(),
            "header name {invalid:?} must be rejected as not a name"
        );
    }
    assert_eq!(
        validate_safe_header_name("Bearer secret")
            .expect_err("a credential-shaped header name is rejected")
            .code(),
        "credentials_forbidden"
    );
    let mut profile = profile_revision();
    profile.credential_transport_mode = CredentialTransportMode::SafeHeader;
    profile.safe_header_name = Some("X-Custom-Auth".to_owned());
    let bytes = profile.encode().expect("safe header profile encodes");
    assert!(
        String::from_utf8_lossy(&bytes).contains("X-Custom-Auth"),
        "the header name is present in the record"
    );
    assert!(
        !String::from_utf8_lossy(&bytes).contains("secret"),
        "no secret value is present in the record"
    );
}

#[test]
fn credential_transport_never_encodes_secret_value() {
    let mut profile = profile_revision();
    profile.credential_transport_mode = CredentialTransportMode::SafeHeader;
    profile.safe_header_name = Some("X-Api-Key".to_owned());
    let bytes = profile.encode().expect("profile encodes");
    let decoded = ProviderProfileRevisionV1::decode(&bytes).expect("profile decodes");
    assert_eq!(
        decoded.credential_transport_mode,
        CredentialTransportMode::SafeHeader
    );
    assert_eq!(decoded.safe_header_name.as_deref(), Some("X-Api-Key"));
    // The record carries the header name but never a secret value.
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("X-Api-Key"));
    for secret in ["sk-", "Bearer ", "api_key", "secret", "token="] {
        assert!(
            !text.contains(secret),
            "secret marker {secret:?} must not appear"
        );
    }
}

// ---- Limits ----

#[test]
fn provider_ids_are_limited_to_63_characters() {
    let mut profile = profile_revision();
    profile.profile_id = "p".repeat(64);
    assert_eq!(
        profile
            .encode()
            .expect_err("over-limit profile id is rejected")
            .code(),
        "provider_profile_revision_invalid"
    );
    profile.profile_id = "p".repeat(63);
    assert!(profile.encode().is_ok());
    let mut selection = provider_selection();
    selection.profile_id = "p".repeat(64);
    assert_eq!(
        selection
            .encode()
            .expect_err("over-limit selection id is rejected")
            .code(),
        "provider_profile_revision_invalid"
    );
    let mut kind = kind_descriptor();
    kind.kind_id = "k".repeat(64);
    assert_eq!(
        kind.encode()
            .expect_err("over-limit kind id is rejected")
            .code(),
        "invalid_provider_kind"
    );
}

#[test]
fn provider_profile_count_limit_is_128() {
    let frozen = ProviderCatalogLimits::frozen();
    assert_eq!(frozen.max_profiles, 128);
    assert_eq!(frozen.validate(), Ok(()));
    assert_eq!(
        ProviderCatalogLimits {
            max_profiles: 129,
            ..frozen
        }
        .validate()
        .expect_err("129 profiles are over the limit"),
        CanonicalError::OverLimit
    );
}

#[test]
fn provider_kind_count_limit_is_32() {
    let frozen = ProviderCatalogLimits::frozen();
    assert_eq!(frozen.max_kinds, 32);
    assert_eq!(
        ProviderCatalogLimits {
            max_kinds: 33,
            ..frozen
        }
        .validate()
        .expect_err("33 kinds are over the limit"),
        CanonicalError::OverLimit
    );
}

#[test]
fn provider_catalog_candidate_limit_is_512_kib() {
    let frozen = ProviderCatalogLimits::frozen();
    assert_eq!(frozen.max_candidate_bytes, 512 * 1024);
    assert_eq!(
        ProviderCatalogLimits {
            max_candidate_bytes: 512 * 1024 + 1,
            ..frozen
        }
        .validate()
        .expect_err("an over-limit candidate is rejected"),
        CanonicalError::OverLimit
    );
}

#[test]
fn catalog_issue_limit_is_32() {
    let frozen = ProviderCatalogLimits::frozen();
    assert_eq!(frozen.max_issues, 32);
    assert_eq!(
        ProviderCatalogLimits {
            max_issues: 33,
            ..frozen
        }
        .validate()
        .expect_err("33 issues are over the limit"),
        CanonicalError::OverLimit
    );
}

#[test]
fn context_source_manifest_accepts_1_to_256_entries_only() {
    let mut empty = context_manifest();
    empty.source_entries.clear();
    assert_eq!(
        empty
            .encode()
            .expect_err("empty manifest is rejected")
            .code(),
        "context_source_manifest_invalid"
    );
    let mut many = context_manifest();
    many.source_entries = (0..256)
        .map(|index| ContextSourceEntryV1 {
            source_id: format!("source-{index}"),
            source_kind: "session".to_owned(),
            revision: "rev-0001".to_owned(),
            safe_label: None,
        })
        .collect();
    let digest = context_source_manifest_digest(&many).expect("manifest digests");
    many.manifest_digest = digest_hex(digest.digest);
    assert!(many.encode().is_ok(), "256 entries are accepted");
    many.source_entries.push(ContextSourceEntryV1 {
        source_id: "source-256".to_owned(),
        source_kind: "session".to_owned(),
        revision: "rev-0001".to_owned(),
        safe_label: None,
    });
    let digest = context_source_manifest_digest(&many).expect("manifest digests");
    many.manifest_digest = digest_hex(digest.digest);
    assert_eq!(
        many.encode().expect_err("257 entries are rejected").code(),
        "context_source_manifest_invalid"
    );
}

#[test]
fn model_context_projection_limits_messages_to_1024() {
    let mut projection = context_projection();
    projection.ordered_messages = vec!["x".to_owned(); 1024];
    let digest = model_context_projection_digest(&projection).expect("projection digests");
    projection.model_context_digest = digest_hex(digest.digest);
    assert!(projection.encode().is_ok(), "1024 messages are accepted");
    projection.ordered_messages.push("y".to_owned());
    let digest = model_context_projection_digest(&projection).expect("projection digests");
    projection.model_context_digest = digest_hex(digest.digest);
    assert_eq!(
        projection
            .encode()
            .expect_err("1025 messages are rejected")
            .code(),
        "model_context_projection_invalid"
    );
    projection.ordered_messages.clear();
    assert_eq!(
        projection
            .encode()
            .expect_err("zero messages are rejected")
            .code(),
        "model_context_projection_invalid"
    );
}

#[test]
fn model_context_projection_limits_aggregate_to_1_mib() {
    let mut projection = context_projection();
    projection.ordered_messages = vec!["x".repeat(1024 * 1024 + 1)];
    assert_eq!(
        projection
            .encode()
            .expect_err("an over-limit aggregate is rejected")
            .code(),
        "model_context_projection_too_large"
    );
    projection.ordered_messages = vec!["x".repeat(1024 * 1024 - 1)];
    let digest = model_context_projection_digest(&projection).expect("projection digests");
    projection.model_context_digest = digest_hex(digest.digest);
    assert!(
        projection.encode().is_ok(),
        "aggregate at the bound is accepted"
    );
}

#[test]
fn reasoning_history_bounds_are_enforced() {
    let mut manifest = reasoning_manifest();
    manifest.history_bound.max_entries = 1;
    assert_eq!(
        manifest
            .encode()
            .expect_err("entry count over the bound is rejected")
            .code(),
        "reasoning_history_too_large"
    );
    let mut manifest = reasoning_manifest();
    manifest.history_bound.max_aggregate_bytes = 1;
    assert_eq!(
        manifest
            .encode()
            .expect_err("aggregate bytes over the bound are rejected")
            .code(),
        "reasoning_history_too_large"
    );
}

#[test]
fn provider_profile_tombstones_are_append_only_removal_history() {
    let tombstone = ProviderProfileTombstoneDto::new("profile-default", 2, 100, "removal-accepted")
        .expect("tombstone is valid");
    let bytes = tombstone.encode().expect("tombstone encodes");
    assert_eq!(
        ProviderProfileTombstoneDto::decode(&bytes).expect("tombstone decodes"),
        tombstone
    );
    // PR24-017: tombstones are removal-history events keyed by the removed
    // catalog revision, not a permanent admission veto. An identifier removed,
    // reintroduced, and removed again records a fresh event with its own
    // revision, and the encode/decode identity differs per removal revision.
    let reintroduced =
        ProviderProfileTombstoneDto::new("profile-default", 4, 200, "removal-accepted")
            .expect("second removal event is valid");
    assert_ne!(tombstone, reintroduced);
    assert_eq!(
        reintroduced.removed_catalog_revision, 4,
        "reintroduction then removal records the new removal revision"
    );
    let kind_tombstone =
        ProviderKindTombstoneDto::new("generic-chat-completion-api", 2, 101, "removal-accepted")
            .expect("kind tombstone is valid");
    let bytes = kind_tombstone.encode().expect("kind tombstone encodes");
    assert_eq!(
        ProviderKindTombstoneDto::decode(&bytes).expect("kind tombstone decodes"),
        kind_tombstone
    );
    // A kind with dependents cannot be removed; immutable parts cannot change.
    let profile = profile_revision();
    assert_eq!(
        validate_provider_kind_removal("responses", std::slice::from_ref(&profile))
            .expect_err("removal with dependents is rejected")
            .code(),
        "provider_kind_has_dependents"
    );
    let descriptor = kind_descriptor();
    let mut changed = descriptor.clone();
    changed.credential_transport_contract = "bearer-only".to_owned();
    assert_eq!(
        validate_provider_kind_revision_immutability(&descriptor, &changed)
            .expect_err("immutable part change is rejected")
            .code(),
        "provider_kind_immutable_mismatch"
    );
}

// ---- Digest and preservation ----

fn profile_identity_input(profile: &ProviderProfileRevisionV1) -> CanonicalIdentityInput {
    CanonicalIdentityInput::new()
        .field(1, WireType::Utf8, encode_utf8(&profile.profile_id))
        .expect("identity field accepts")
        .field(2, WireType::Utf8, encode_utf8(&profile.revision_id))
        .expect("identity field accepts")
        .field(3, WireType::Utf8, encode_utf8(&profile.provider_kind_id))
        .expect("identity field accepts")
        .field(4, WireType::Utf8, encode_utf8(&profile.model_id))
        .expect("identity field accepts")
        .field(5, WireType::Utf8, encode_utf8(&profile.endpoint))
        .expect("identity field accepts")
        .field(
            6,
            WireType::U64,
            transport_mode_bytes(profile.credential_transport_mode),
        )
        .expect("identity field accepts")
        .field(
            7,
            WireType::Optional,
            encode_optional_utf8(&profile.safe_header_name),
        )
        .expect("identity field accepts")
        .field(
            8,
            WireType::Utf8,
            encode_utf8(&profile.capability_taxonomy_revision),
        )
        .expect("identity field accepts")
        .field(
            9,
            WireType::Optional,
            encode_optional_utf8(&profile.reasoning_compatibility_id),
        )
        .expect("identity field accepts")
        .field(
            10,
            WireType::Utf8,
            encode_utf8(&profile.kind_descriptor_revision_id),
        )
        .expect("identity field accepts")
        .field(
            11,
            WireType::Record,
            profile
                .driver_contract_revision
                .encode()
                .expect("driver contract encodes"),
        )
        .expect("identity field accepts")
}

fn selection_identity_input(selection: &ProviderSelectionV1) -> CanonicalIdentityInput {
    CanonicalIdentityInput::new()
        .field(
            1,
            WireType::Utf8,
            encode_utf8(&selection.selection_canonicalization_version),
        )
        .expect("identity field accepts")
        .field(2, WireType::Utf8, encode_utf8(&selection.profile_id))
        .expect("identity field accepts")
        .field(
            3,
            WireType::Utf8,
            encode_utf8(&selection.provider_profile_revision_id),
        )
        .expect("identity field accepts")
        .field(4, WireType::Utf8, encode_utf8(&selection.kind_id))
        .expect("identity field accepts")
        .field(
            5,
            WireType::Utf8,
            encode_utf8(&selection.kind_descriptor_revision_id),
        )
        .expect("identity field accepts")
        .field(6, WireType::Utf8, encode_utf8(&selection.model_id))
        .expect("identity field accepts")
        .field(
            7,
            WireType::Utf8,
            encode_utf8(&selection.normalized_effective_endpoint),
        )
        .expect("identity field accepts")
        .field(
            8,
            WireType::U64,
            transport_mode_bytes(selection.credential_transport_mode),
        )
        .expect("identity field accepts")
        .field(
            9,
            WireType::Optional,
            encode_optional_utf8(&selection.credential_transport_safe_header_name),
        )
        .expect("identity field accepts")
        .field(
            10,
            WireType::List,
            encode_utf8_list(&selection.declared_model_capability_subset),
        )
        .expect("identity field accepts")
        .field(
            11,
            WireType::Utf8,
            encode_utf8(&selection.resolved_reasoning_policy),
        )
        .expect("identity field accepts")
        .field(
            12,
            WireType::Utf8,
            encode_utf8(&selection.effective_execution_policy),
        )
        .expect("identity field accepts")
        .field(
            13,
            WireType::Utf8,
            encode_utf8(&selection.effective_loopback_policy_or_not_applicable),
        )
        .expect("identity field accepts")
        .field(
            14,
            WireType::Utf8,
            encode_utf8(&selection.provider_driver_contract_revision),
        )
        .expect("identity field accepts")
}

#[test]
fn provider_profile_digest_excludes_credentials() {
    let profile = profile_revision();
    let baseline = provider_profile_revision_digest(profile_identity_input(&profile))
        .expect("profile digests");
    assert_eq!(baseline.namespace, "provider-profile-revision");
    let with_secret = provider_profile_revision_digest(
        profile_identity_input(&profile).with_credentials(b"sk-test-Bearer-secret".to_vec()),
    )
    .expect("credentials stay excluded from the digest");
    assert_eq!(with_secret, baseline);
    let encoded = profile_identity_input(&profile)
        .with_credentials(b"sk-test-Bearer-secret".to_vec())
        .encode()
        .expect("identity input encodes");
    assert!(
        !encoded.windows(7).any(|window| window == b"sk-test"),
        "credential bytes never reach identity bytes"
    );
}

#[test]
fn provider_profile_digest_excludes_paths() {
    let profile = profile_revision();
    let baseline = provider_profile_revision_digest(profile_identity_input(&profile))
        .expect("profile digests");
    let path = std::env::temp_dir()
        .join("intention-relay-excluded-profile-path")
        .to_string_lossy()
        .into_owned();
    let with_path = provider_profile_revision_digest(
        profile_identity_input(&profile).with_filesystem_path(path),
    )
    .expect("paths stay excluded from the digest");
    assert_eq!(with_path, baseline);
}

#[test]
fn provider_profile_digest_excludes_display_data() {
    let profile = profile_revision();
    let baseline = provider_profile_revision_digest(profile_identity_input(&profile))
        .expect("profile digests");
    let with_display = provider_profile_revision_digest(
        profile_identity_input(&profile).with_display_data("My display name"),
    )
    .expect("display data stays excluded from the digest");
    assert_eq!(with_display, baseline);
}

#[test]
fn provider_profile_digest_excludes_readiness() {
    let profile = profile_revision();
    let baseline = provider_profile_revision_digest(profile_identity_input(&profile))
        .expect("profile digests");
    let with_readiness =
        provider_profile_revision_digest(profile_identity_input(&profile).with_readiness(true))
            .expect("readiness stays excluded from the digest");
    assert_eq!(with_readiness, baseline);
}

#[test]
fn provider_selection_digest_excludes_current_state() {
    let selection = provider_selection();
    let baseline =
        provider_selection_digest(selection_identity_input(&selection)).expect("selection digests");
    assert_eq!(baseline.namespace, "provider-selection");
    let with_state = provider_selection_digest(
        selection_identity_input(&selection).with_current_state(vec![0x01, 0x02, 0x03]),
    )
    .expect("current state stays excluded from the digest");
    assert_eq!(with_state, baseline);
    // The selection source is provenance only and never part of the identity
    // input; changing it must not change the digest.
    let mut other = selection;
    other.selection_source = Some("other-catalog-rev".to_owned());
    assert_eq!(
        provider_selection_digest(selection_identity_input(&other)).expect("selection digests"),
        baseline
    );
}

#[test]
fn context_manifest_digest_excludes_safe_label() {
    let manifest = context_manifest();
    let baseline = context_source_manifest_digest(&manifest).expect("manifest digests");
    let mut relabeled = manifest.clone();
    relabeled.source_entries[0].safe_label = Some("A completely different label".to_owned());
    assert_eq!(
        context_source_manifest_digest(&relabeled).expect("relabeled manifest digests"),
        baseline,
        "safe labels are display data and never enter the identity digest"
    );
    // The manifest bytes do include the label; only the identity excludes it.
    let bytes = relabeled.encode().expect("relabeled manifest encodes");
    assert!(String::from_utf8_lossy(&bytes).contains("A completely different label"));
    // A label that carries a credential shape is rejected outright.
    let mut secret = manifest;
    secret.source_entries[0].safe_label = Some("Bearer secret".to_owned());
    assert_eq!(
        secret
            .encode()
            .expect_err("credential-shaped label is rejected")
            .code(),
        "credentials_forbidden"
    );
}

#[test]
fn no_synthetic_post_m5_records_are_created() {
    for (text, record) in [
        (
            include_str!("fixtures/goldens/execution-meaning-v4.txt"),
            "execution-meaning-v4",
        ),
        (
            include_str!("fixtures/goldens/envelope-ordinary.txt"),
            "envelope-ordinary",
        ),
    ] {
        let fixture = parse_golden(text).expect("golden fixture parses");
        assert_eq!(fixture.record, record);
        let bytes = hex_decode(&fixture.bytes_hex);
        let decoded = match record {
            "execution-meaning-v4" => {
                intention_domain::run_execution_meaning::RunExecutionMeaningV4Record::decode(&bytes)
                    .expect("v4 golden decodes")
                    .encode()
                    .expect("v4 golden re-encodes")
            }
            _ => intention_domain::run_execution_meaning::RunExecutionMeaningEnvelopeV1::decode(
                &bytes,
            )
            .expect("envelope golden decodes")
            .encode()
            .expect("envelope golden re-encodes"),
        };
        assert_eq!(
            decoded, bytes,
            "decoding must never synthesize post-M5 records in {record}"
        );
    }
}

#[test]
fn current_execution_meaning_bytes_remain_byte_stable() {
    for (text, record) in [
        (
            include_str!("fixtures/goldens/execution-meaning-v4.txt"),
            "execution-meaning-v4",
        ),
        (
            include_str!("fixtures/goldens/envelope-ordinary.txt"),
            "envelope-ordinary",
        ),
        (
            include_str!("fixtures/goldens/envelope-mandate.txt"),
            "envelope-mandate",
        ),
        (
            include_str!("fixtures/goldens/envelope-verifier-mandate.txt"),
            "envelope-verifier-mandate",
        ),
        (
            include_str!("fixtures/goldens/agent-activity-selection-root-v1.txt"),
            "agent-activity-selection-root-v1",
        ),
        (
            include_str!("fixtures/goldens/agent-activity-selection-descendant-v1.txt"),
            "agent-activity-selection-descendant-v1",
        ),
        (
            include_str!("fixtures/goldens/programmatic-policy-selection-provenance-v1.txt"),
            "programmatic-policy-selection-provenance-v1",
        ),
    ] {
        let fixture = parse_golden(text).expect("golden fixture parses");
        assert_eq!(fixture.record, record);
        let bytes = hex_decode(&fixture.bytes_hex);
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        assert_eq!(
            hex_encode(&digest),
            fixture.sha256,
            "sha256 of the golden bytes must remain stable for {record}"
        );
        // Every current golden must still decode and re-encode to the exact
        // pinned bytes, proving no byte output changed.
        let re_encoded = match record {
            "execution-meaning-v4" => {
                intention_domain::run_execution_meaning::RunExecutionMeaningV4Record::decode(&bytes)
                    .expect("v4 golden decodes")
                    .encode()
                    .expect("v4 golden re-encodes")
            }
            "envelope-ordinary" | "envelope-mandate" | "envelope-verifier-mandate" => {
                intention_domain::run_execution_meaning::RunExecutionMeaningEnvelopeV1::decode(
                    &bytes,
                )
                .expect("envelope golden decodes")
                .encode()
                .expect("envelope golden re-encodes")
            }
            "agent-activity-selection-root-v1" => {
                intention_domain::run_execution_meaning::AgentActivitySelectionV1::decode(&bytes)
                    .expect("root selection golden decodes")
                    .encode()
                    .expect("root selection golden re-encodes")
            }
            "agent-activity-selection-descendant-v1" => {
                intention_domain::run_execution_meaning::AgentActivitySelectionV1::decode(&bytes)
                    .expect("descendant selection golden decodes")
                    .encode()
                    .expect("descendant selection golden re-encodes")
            }
            _ => {
                intention_domain::run_execution_meaning::ProgrammaticCallerPolicySelectionV1::decode(
                    &bytes,
                )
                .expect("policy selection golden decodes")
                .encode()
                .expect("policy selection golden re-encodes")
            }
        };
        assert_eq!(
            re_encoded, bytes,
            "current golden {record} must remain byte-identical"
        );
    }
}

#[test]
fn ordinary_run_meaning_does_not_gain_reasoning_records() {
    let fixture = parse_golden(include_str!("fixtures/goldens/execution-meaning-v4.txt"))
        .expect("v4 golden parses");
    let bytes = hex_decode(&fixture.bytes_hex);
    let decoded =
        intention_domain::run_execution_meaning::RunExecutionMeaningV4Record::decode(&bytes)
            .expect("v4 golden decodes");
    // Field 11 remains the agent activity selection, never a reasoning record.
    let selection = decoded
        .decode_agent_activity_selection()
        .expect("field 11 is the agent activity selection");
    assert!(matches!(
        selection,
        intention_domain::run_execution_meaning::AgentActivitySelectionV1::Root { .. }
    ));
    assert!(
        ReasoningHistoryManifestDto::decode(&decoded.agent_activity_selection).is_err(),
        "field 11 must not decode as a reasoning history manifest"
    );
    // Re-encoding the ordinary meaning reproduces the exact historical bytes.
    assert_eq!(
        decoded.encode().expect("v4 golden re-encodes"),
        bytes,
        "ordinary run meaning must not gain reasoning records"
    );
}

// ---- Cross-boundary ----

#[test]
fn canonical_bytes_contain_no_raw_toml() {
    let bytes = all_family_bytes();
    let text = String::from_utf8_lossy(&bytes);
    for marker in [
        "[profiles",
        "[[provider",
        "provider =",
        "toml",
        "TOML",
        "config.toml",
    ] {
        assert!(
            !text.contains(marker),
            "raw TOML marker {marker:?} must never appear"
        );
    }
}

#[test]
fn canonical_bytes_contain_no_credentials() {
    let bytes = all_family_bytes();
    let text = String::from_utf8_lossy(&bytes);
    for marker in [
        "sk-",
        "Bearer secret",
        "api_key",
        "apikey",
        "token=",
        "password",
    ] {
        assert!(
            !text.contains(marker),
            "credential marker {marker:?} must never appear"
        );
    }
}

#[test]
fn canonical_bytes_contain_no_client_handles() {
    let bytes = all_family_bytes();
    let text = String::from_utf8_lossy(&bytes);
    for marker in [
        "client_handle",
        "client-handle",
        "client handle",
        "sdk",
        "SDK",
    ] {
        assert!(
            !text.contains(marker),
            "client-handle marker {marker:?} must never appear"
        );
    }
}

#[test]
fn canonical_bytes_contain_no_filesystem_paths() {
    let bytes = all_family_bytes();
    let text = String::from_utf8_lossy(&bytes);
    let path = std::env::temp_dir()
        .join("intention-relay-boundary")
        .to_string_lossy()
        .into_owned();
    assert!(
        !text.contains(&path),
        "platform-native filesystem paths must never appear in canonical bytes"
    );
}

#[test]
fn safe_projection_contains_no_operational_readiness() {
    let bytes = all_family_bytes();
    let text = String::from_utf8_lossy(&bytes);
    for marker in ["readiness", "is_ready", "health", "enabled", "discovery"] {
        assert!(
            !text.contains(marker),
            "readiness marker {marker:?} must never appear"
        );
    }
    let projection_bytes = context_projection().encode().expect("projection encodes");
    let reader = CanonicalRecordReader::new(&projection_bytes, 5).expect("projection parses");
    assert_eq!(reader.tag, TagRegistry::MODEL_CONTEXT_PROJECTION_V1);
    assert!(
        reader
            .field(5, WireType::Utf8)
            .expect("field lookup succeeds")
            .is_some(),
        "the projection carries only its typed digest, never readiness"
    );
}

// ---- Fake-secret regression ----

#[test]
fn fake_secret_values_are_rejected_before_encoding() {
    let mut profile = profile_revision();
    profile.credential_transport_mode = CredentialTransportMode::SafeHeader;
    profile.safe_header_name = Some("Bearer secret".to_owned());
    assert_eq!(
        profile
            .encode()
            .expect_err("secret header name is rejected")
            .code(),
        "credentials_forbidden"
    );
    let mut selection = provider_selection();
    selection.selection_source = Some("catalog-rev-sk-test".to_owned());
    assert_eq!(
        selection
            .encode()
            .expect_err("secret-shaped provenance is rejected")
            .code(),
        "credentials_forbidden"
    );
    let mut selection = provider_selection();
    selection.normalized_effective_endpoint = "https://sk-test@api.example.com/v1".to_owned();
    assert_eq!(
        selection
            .encode()
            .expect_err("secret-bearing endpoint is rejected")
            .code(),
        "invalid_endpoint"
    );
    let mut manifest = context_manifest();
    manifest.source_entries[0].safe_label = Some("api_key_value".to_owned());
    assert_eq!(
        manifest
            .encode()
            .expect_err("secret-shaped label is rejected")
            .code(),
        "credentials_forbidden"
    );
}

#[test]
fn fake_secret_values_never_reach_canonical_bytes_or_digests() {
    let profile = profile_revision();
    let baseline = provider_profile_revision_digest(profile_identity_input(&profile))
        .expect("profile digests");
    let encoded = profile_identity_input(&profile)
        .with_credentials(b"sk-test".to_vec())
        .with_current_state(b"Bearer secret".to_vec())
        .with_filesystem_path(
            std::env::temp_dir()
                .join("intention-relay-secret-path")
                .to_string_lossy()
                .into_owned(),
        )
        .encode()
        .expect("identity input encodes");
    let text = String::from_utf8_lossy(&encoded);
    for marker in ["sk-test", "Bearer secret"] {
        assert!(
            !text.contains(marker),
            "secret marker {marker:?} must never be encoded"
        );
    }
    assert_eq!(
        provider_profile_revision_digest(
            profile_identity_input(&profile)
                .with_credentials(b"sk-test".to_vec())
                .with_current_state(b"Bearer secret".to_vec()),
        )
        .expect("secrets stay excluded from the digest"),
        baseline
    );
    // The profile record itself carries no secret-shaped field value.
    let bytes = profile.encode().expect("profile encodes");
    let text = String::from_utf8_lossy(&bytes);
    for marker in ["sk-test", "Bearer secret", "api_key"] {
        assert!(
            !text.contains(marker),
            "secret marker {marker:?} must never appear"
        );
    }
}
