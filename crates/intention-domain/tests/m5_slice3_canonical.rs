#![allow(
    clippy::expect_used,
    reason = "Slice 3 canonical fixtures use expect for precise test diagnostics."
)]

//! Slice 3 activated canonical contract tests.
//!
//! Owner: ADR 0044 with ADR 0036's tag registry. These tests pin the seven
//! Slice 3 activated tag families (`0x0201`, `0x0203`-`0x0205`,
//! `0x0301`-`0x0304`), the v4 selection slot map, digest goldens, and
//! credential/bound rejections.

use intention_domain::canonical::{CanonicalError, Digest256, TagRegistry, WireType};
use intention_domain::run_execution_meaning::{
    AgentActivitySelectionV1, DisabledOr, FixedActivityLimits, FixedRunLimits,
    ProgrammaticCallerPolicySelectionV1, RunExecutionMeaningV4Record,
};
use intention_domain::slice3_selections::{
    ContinualHarnessSelectionV1, GOAL_MAX_CONTEXT_BYTES, GOAL_MAX_MEMORY_CARDS,
    GOAL_MAX_SKILL_ROLE_CARDS, GOAL_MAX_TARGET_SNAPSHOT_BYTES, GoalCardReferenceV1,
    GoalGateRevisionReferenceV1, GoalRevisionReferenceV1, GoalRunKindV1, GoalRunSelectionBoundsV1,
    GoalRunSelectionV1, GoalScopeLinkProvenanceV1, HARNESS_MAX_CAUSE_DEPTH, HARNESS_MAX_CONCURRENT,
    HARNESS_MAX_DOSSIER_BYTES, HARNESS_MAX_TOTAL_LAUNCHES, HarnessClassResolutionV1,
    HarnessExecutionClassV1, HarnessSelectionBoundsV1, HarnessSourceKindV1, HarnessTriggerReasonV1,
    MAX_SLICE3_TEXT_CHARS, McpMethodCatalogSelectionV1, ProgrammaticCallerRootOriginV1,
};
use sha2::{Digest, Sha256};

/// One parsed golden fixture file.
struct GoldenFixture {
    record: String,
    tag: u32,
    record_version: u32,
    bytes_hex: String,
    sha256: String,
}

fn parse_golden(text: &str) -> Result<GoldenFixture, String> {
    let mut fixture = GoldenFixture {
        record: String::new(),
        tag: 0,
        record_version: 0,
        bytes_hex: String::new(),
        sha256: String::new(),
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
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
            _ => {}
        }
    }
    Ok(fixture)
}

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

fn hex16(spec: &str) -> [u8; 16] {
    let digits: String = spec.chars().filter(|character| *character != '-').collect();
    let mut bytes = [0u8; 16];
    for (index, chunk) in digits.as_bytes().chunks(2).enumerate() {
        let pair = std::str::from_utf8(chunk).expect("fixture uuid digits are ascii");
        bytes[index] = u8::from_str_radix(pair, 16).expect("fixture uuid digits are hex");
    }
    bytes
}

const fn harness_bounds() -> HarnessSelectionBoundsV1 {
    HarnessSelectionBoundsV1 {
        max_cause_depth: HARNESS_MAX_CAUSE_DEPTH,
        max_concurrent: HARNESS_MAX_CONCURRENT,
        max_total_launches: HARNESS_MAX_TOTAL_LAUNCHES,
        dossier_bytes: HARNESS_MAX_DOSSIER_BYTES,
        checkpoint_bytes: 256 * 1024,
        conclusion_bytes: 128 * 1024,
    }
}

const fn goal_bounds() -> GoalRunSelectionBoundsV1 {
    GoalRunSelectionBoundsV1 {
        max_memory_cards: GOAL_MAX_MEMORY_CARDS,
        max_skill_role_cards: GOAL_MAX_SKILL_ROLE_CARDS,
        target_snapshot_bytes: GOAL_MAX_TARGET_SNAPSHOT_BYTES,
        context_bytes: GOAL_MAX_CONTEXT_BYTES,
    }
}

fn harness_selection() -> ContinualHarnessSelectionV1 {
    ContinualHarnessSelectionV1 {
        harness_id: hex16("11111111-1111-4111-8111-111111111111"),
        rule_revision: 3,
        trigger_reason: HarnessTriggerReasonV1 {
            reason_id: hex16("22222222-2222-4222-8222-222222222222"),
            source_kind: HarnessSourceKindV1::CalendarTime,
            first_observed_at_ms: 1_700_000_000_000,
            last_observed_at_ms: 1_700_000_060_000,
            coalesced_count: 2,
        },
        class_resolution: HarnessClassResolutionV1 {
            class: HarnessExecutionClassV1::Medium,
            narrowed_tool_ids: vec!["read_file".to_owned(), "search_workspace".to_owned()],
        },
        dossier_digest: Digest256::sha256(b"slice3-harness-dossier"),
        checkpoint_reference: Some(hex16("33333333-3333-4333-8333-333333333333")),
        time_zone_application: "Europe/Berlin".to_owned(),
        bounds: harness_bounds(),
    }
}

fn goal_selection() -> GoalRunSelectionV1 {
    GoalRunSelectionV1 {
        leading_goal_id: hex16("44444444-4444-4444-8444-444444444444"),
        goal_revision: 5,
        scope_link_provenance: GoalScopeLinkProvenanceV1 {
            project_id: hex16("55555555-5555-4555-8555-555555555555"),
            session_id: Some(hex16("66666666-6666-4666-8666-666666666666")),
            link_id: Some(hex16("77777777-7777-4777-8777-777777777777")),
        },
        parent_revision_chain: vec![GoalRevisionReferenceV1 {
            goal_id: hex16("88888888-8888-4888-8888-888888888888"),
            revision: 2,
        }],
        obligatory_component_references: vec![hex16("99999999-9999-4999-8999-999999999999")],
        selected_gate_revisions: vec![GoalGateRevisionReferenceV1 {
            gate_reference: hex16("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
            revision: 4,
        }],
        valid_evidence_references: vec![hex16("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb")],
        selected_memory_cards: vec![GoalCardReferenceV1 {
            card_reference: hex16("cccccccc-cccc-4ccc-8ccc-cccccccccccc"),
            revision: 1,
        }],
        selected_skill_cards: vec![GoalCardReferenceV1 {
            card_reference: hex16("dddddddd-dddd-4ddd-8ddd-dddddddddddd"),
            revision: 7,
        }],
        selected_role_cards: Vec::new(),
        revealed_full_record_references: vec![hex16("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee")],
        policy_snapshot_reference: hex16("ffffffff-ffff-4fff-8fff-ffffffffffff"),
        activity_selection_reference: hex16("11111111-2222-4333-8444-555555555555"),
        run_kind: GoalRunKindV1::GoalDirectedOrdinary,
        target_snapshot_digest: Digest256::sha256(b"slice3-goal-target-snapshot"),
        bounds: goal_bounds(),
    }
}

fn mcp_selection() -> McpMethodCatalogSelectionV1 {
    McpMethodCatalogSelectionV1 {
        method_catalog_revision: "mcp-catalog-1".to_owned(),
        connection_reference: hex16("12121212-1212-4212-8212-121212121212"),
        server_revision_digest: Digest256::sha256(b"slice3-mcp-server-revision"),
        method_reference: "tools/list".to_owned(),
        method_schema_revision: "mcp-schema-1".to_owned(),
        typed_input_constraint_family: Some("json-schema-2020-12".to_owned()),
    }
}

fn policy_selection() -> ProgrammaticCallerPolicySelectionV1 {
    ProgrammaticCallerPolicySelectionV1 {
        root_origin: ProgrammaticCallerRootOriginV1::InteractiveUser {
            originating_turn_id: hex16("13131313-1313-4313-8313-131313131313"),
        },
        effective_policy_snapshot_reference: hex16("14141414-1414-4414-8414-141414141414"),
        policy_selection_digest: Digest256::sha256(b"slice3-policy-snapshot"),
        inherited_scope_provenance: vec![hex16("15151515-1515-4515-8515-151515151515")],
        fixed_run_limits: FixedRunLimits {
            max_attempts: 1,
            max_total_seconds: 3600,
            max_actions: 1024,
            max_concurrent_actions: 4,
            max_retained_bytes: 1_048_576,
            max_clarification_seconds: 3600,
        },
    }
}

fn activity_selection() -> AgentActivitySelectionV1 {
    AgentActivitySelectionV1::Root {
        activity_tree_id: hex16("16161616-1616-4616-8616-161616161616"),
        root_origin: intention_domain::run_execution_meaning::ExecutionKind::Ordinary,
        activity_exchange_revision: 1,
        activity_journal_revision: 1,
        user_projection_revision: 1,
        fixed_activity_limits: FixedActivityLimits::frozen(),
    }
}

fn v4_record() -> RunExecutionMeaningV4Record {
    RunExecutionMeaningV4Record {
        fields: (0..6).map(|index| vec![index as u8; 3]).collect(),
        harness_selection: DisabledOr::Selected(harness_selection()),
        goal_selection: DisabledOr::Selected(goal_selection()),
        mcp_method_catalog_selection: DisabledOr::Selected(mcp_selection()),
        programmatic_caller_policy_selection: DisabledOr::Selected(policy_selection()),
        agent_activity_selection: activity_selection()
            .encode()
            .expect("activity selection encodes"),
    }
}

#[test]
fn goal_run_selection_golden_matches_the_encoder_and_round_trips() {
    let selection = goal_selection();
    let encoded = selection.encode().expect("goal selection encodes");
    assert_golden_fixture(
        include_str!("fixtures/goldens/goal-run-selection-v1.txt"),
        "goal-run-selection-v1",
        0x0203,
        1,
        &encoded,
    );
    assert_eq!(
        GoalRunSelectionV1::decode(&encoded).expect("goal selection decodes"),
        selection
    );
}

#[test]
fn continual_harness_selection_golden_matches_the_encoder_and_round_trips() {
    let selection = harness_selection();
    let encoded = selection.encode().expect("harness selection encodes");
    assert_golden_fixture(
        include_str!("fixtures/goldens/continual-harness-selection-v1.txt"),
        "continual-harness-selection-v1",
        0x0204,
        1,
        &encoded,
    );
    assert_eq!(
        ContinualHarnessSelectionV1::decode(&encoded).expect("harness selection decodes"),
        selection
    );
}

#[test]
fn mcp_method_catalog_selection_golden_matches_the_encoder_and_round_trips() {
    let selection = mcp_selection();
    let encoded = selection.encode().expect("mcp selection encodes");
    assert_golden_fixture(
        include_str!("fixtures/goldens/mcp-method-catalog-selection-v1.txt"),
        "mcp-method-catalog-selection-v1",
        0x0205,
        1,
        &encoded,
    );
    assert_eq!(
        McpMethodCatalogSelectionV1::decode(&encoded).expect("mcp selection decodes"),
        selection
    );
}

#[test]
fn programmatic_policy_field_one_is_the_typed_closed_root_origin() {
    // Field 1 is a nested record, not the historical `ExecutionKind` byte.
    let selection = policy_selection();
    let encoded = selection.encode().expect("policy selection encodes");
    assert_eq!(
        ProgrammaticCallerPolicySelectionV1::decode(&encoded)
            .expect("policy selection decodes")
            .root_origin,
        ProgrammaticCallerRootOriginV1::InteractiveUser {
            originating_turn_id: hex16("13131313-1313-4313-8313-131313131313"),
        }
    );
    let harness_root = ProgrammaticCallerPolicySelectionV1 {
        root_origin: ProgrammaticCallerRootOriginV1::ContinualHarness {
            harness_id: hex16("17171717-1717-4717-8717-171717171717"),
            rule_revision: 9,
            trigger_reason_id: hex16("18181818-1818-4818-8818-181818181818"),
        },
        ..selection
    };
    let harness_encoded = harness_root
        .encode()
        .expect("harness-root selection encodes");
    assert_eq!(
        ProgrammaticCallerPolicySelectionV1::decode(&harness_encoded)
            .expect("harness-root selection decodes")
            .root_origin,
        harness_root.root_origin
    );
    // A historical `ExecutionKind` byte in field 1 is no longer accepted.
    let historical = raw_record(
        TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
        1,
        &[(1, WireType::U64, &[0, 0, 0, 0, 0, 0, 0, 0])],
    );
    assert_eq!(
        ProgrammaticCallerPolicySelectionV1::decode(&historical)
            .expect_err("historical execution-kind byte is rejected"),
        CanonicalError::InvalidField
    );
    // An unknown root-origin variant version is rejected.
    let unknown_root = raw_record(
        TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
        1,
        &[(1, WireType::Record, &raw_record(0, 9, &[]))],
    );
    assert_eq!(
        ProgrammaticCallerPolicySelectionV1::decode(&unknown_root)
            .expect_err("unknown root-origin variant is rejected"),
        CanonicalError::InvalidTag
    );
}

#[test]
fn v4_selection_slots_round_trip_typed_values_and_closed_markers() {
    let record = v4_record();
    let encoded = record.encode().expect("v4 record encodes");
    let decoded = RunExecutionMeaningV4Record::decode(&encoded).expect("v4 record decodes");
    assert_eq!(decoded, record);
    assert_eq!(
        decoded.harness_selection,
        DisabledOr::Selected(harness_selection())
    );
    assert_eq!(
        decoded.goal_selection,
        DisabledOr::Selected(goal_selection())
    );
    assert_eq!(
        decoded.mcp_method_catalog_selection,
        DisabledOr::Selected(mcp_selection())
    );
    assert_eq!(
        decoded.programmatic_caller_policy_selection,
        DisabledOr::Selected(policy_selection())
    );
    // Disabled slots carry the closed one-byte marker and stay distinguishable
    // from a selected empty record.
    let mut disabled = record;
    disabled.harness_selection = DisabledOr::Disabled;
    disabled.goal_selection = DisabledOr::Disabled;
    disabled.mcp_method_catalog_selection = DisabledOr::Disabled;
    disabled.programmatic_caller_policy_selection = DisabledOr::Disabled;
    let disabled_bytes = disabled.encode().expect("disabled slots encode");
    let decoded =
        RunExecutionMeaningV4Record::decode(&disabled_bytes).expect("disabled slot record decodes");
    assert_eq!(decoded.harness_selection, DisabledOr::Disabled);
    assert_eq!(decoded.goal_selection, DisabledOr::Disabled);
    assert_eq!(decoded.mcp_method_catalog_selection, DisabledOr::Disabled);
    assert_eq!(
        decoded.programmatic_caller_policy_selection,
        DisabledOr::Disabled
    );
}

#[test]
fn selection_decoders_reject_unknown_tags_versions_and_trailing_bytes() {
    assert_eq!(
        ContinualHarnessSelectionV1::decode(&raw_record(0x0D04, 1, &[]))
            .expect_err("unknown harness tag is rejected"),
        CanonicalError::InvalidTag
    );
    assert_eq!(
        GoalRunSelectionV1::decode(&raw_record(0x0D03, 1, &[]))
            .expect_err("unknown goal tag is rejected"),
        CanonicalError::InvalidTag
    );
    assert_eq!(
        McpMethodCatalogSelectionV1::decode(&raw_record(0x0D05, 1, &[]))
            .expect_err("unknown mcp tag is rejected"),
        CanonicalError::InvalidTag
    );
    assert_eq!(
        ContinualHarnessSelectionV1::decode(&raw_record(
            TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
            2,
            &[],
        ))
        .expect_err("unknown harness version is rejected"),
        CanonicalError::InvalidTag
    );
    assert_eq!(
        GoalRunSelectionV1::decode(&raw_record(TagRegistry::GOAL_RUN_SELECTION_V1, 2, &[]))
            .expect_err("unknown goal version is rejected"),
        CanonicalError::InvalidTag
    );
    assert_eq!(
        McpMethodCatalogSelectionV1::decode(&raw_record(
            TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1,
            2,
            &[],
        ))
        .expect_err("unknown mcp version is rejected"),
        CanonicalError::InvalidTag
    );
    let mut trailing = harness_selection()
        .encode()
        .expect("harness selection encodes");
    trailing.extend_from_slice(&[0x00]);
    assert_eq!(
        ContinualHarnessSelectionV1::decode(&trailing).expect_err("trailing bytes are rejected"),
        CanonicalError::TrailingBytes
    );
    let mut goal_trailing = goal_selection().encode().expect("goal selection encodes");
    goal_trailing.extend_from_slice(&[0x00]);
    assert_eq!(
        GoalRunSelectionV1::decode(&goal_trailing).expect_err("trailing bytes are rejected"),
        CanonicalError::TrailingBytes
    );
    let mut mcp_trailing = mcp_selection().encode().expect("mcp selection encodes");
    mcp_trailing.extend_from_slice(&[0x00]);
    assert_eq!(
        McpMethodCatalogSelectionV1::decode(&mcp_trailing)
            .expect_err("trailing bytes are rejected"),
        CanonicalError::TrailingBytes
    );
}

#[test]
fn disabled_markers_accept_only_the_two_closed_values() {
    for malformed in [Vec::<u8>::new(), vec![0x02], vec![0x00, 0xAA], vec![0x01]] {
        assert!(
            DisabledOr::<GoalRunSelectionV1>::decode(&malformed).is_err(),
            "malformed disabled marker must be rejected"
        );
        assert!(
            DisabledOr::<ContinualHarnessSelectionV1>::decode(&malformed).is_err(),
            "malformed disabled marker must be rejected"
        );
        assert!(
            DisabledOr::<McpMethodCatalogSelectionV1>::decode(&malformed).is_err(),
            "malformed disabled marker must be rejected"
        );
        assert!(
            DisabledOr::<ProgrammaticCallerPolicySelectionV1>::decode(&malformed).is_err(),
            "malformed disabled marker must be rejected"
        );
    }
}

#[test]
fn slice3_selections_reject_over_limit_bounds_and_credential_shaped_text() {
    let mut harness_over = harness_selection();
    harness_over.bounds.max_cause_depth = HARNESS_MAX_CAUSE_DEPTH + 1;
    assert_eq!(
        harness_over
            .encode()
            .expect_err("over-limit harness bound is rejected"),
        CanonicalError::OverLimit
    );
    let mut harness_secret = harness_selection();
    harness_secret.time_zone_application = "api_key=secret-value".to_owned();
    assert_eq!(
        harness_secret
            .encode()
            .expect_err("credential-shaped time zone is rejected"),
        CanonicalError::CredentialsForbidden
    );
    let mut goal_over = goal_selection();
    goal_over.bounds.context_bytes = GOAL_MAX_CONTEXT_BYTES + 1;
    assert_eq!(
        goal_over
            .encode()
            .expect_err("over-limit goal bound is rejected"),
        CanonicalError::OverLimit
    );
    let mut goal_unlinked = goal_selection();
    goal_unlinked.scope_link_provenance.session_id = None;
    assert_eq!(
        goal_unlinked
            .encode()
            .expect_err("link without session is rejected"),
        CanonicalError::InvalidField
    );
    let mut mcp_secret = mcp_selection();
    mcp_secret.method_catalog_revision = "token=secret".to_owned();
    assert_eq!(
        mcp_secret
            .encode()
            .expect_err("credential-shaped catalog revision is rejected"),
        CanonicalError::CredentialsForbidden
    );
    let mut mcp_long = mcp_selection();
    mcp_long.method_reference = "m".repeat(MAX_SLICE3_TEXT_CHARS + 1);
    assert_eq!(
        mcp_long
            .encode()
            .expect_err("over-long method reference is rejected"),
        CanonicalError::InvalidField
    );
}

#[test]
fn slice3_canonical_bytes_contain_no_credentials_or_paths() {
    let mut bytes = Vec::new();
    for encoded in [
        goal_selection().encode().expect("goal selection encodes"),
        harness_selection()
            .encode()
            .expect("harness selection encodes"),
        mcp_selection().encode().expect("mcp selection encodes"),
        policy_selection()
            .encode()
            .expect("policy selection encodes"),
        v4_record().encode().expect("v4 record encodes"),
    ] {
        bytes.extend_from_slice(&encoded);
    }
    let text = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
    for forbidden in [
        "api_key", "apikey", "secret", "password", "bearer ", "token=", "key=", "auth=", "sk-",
    ] {
        assert!(
            !text.contains(forbidden),
            "Slice 3 canonical bytes must not carry {forbidden}"
        );
    }
    for forbidden in ["/home/", "/tmp/", "c:\\", "\\\\?\\", "/workspace/"] {
        assert!(
            !text.contains(forbidden),
            "Slice 3 canonical bytes must not carry the path marker {forbidden}"
        );
    }
}

/// Builds a raw framed record from explicit field triples.
fn raw_record(tag: u32, version: u32, fields: &[(u32, WireType, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"IRCR");
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(&tag.to_be_bytes());
    out.extend_from_slice(&version.to_be_bytes());
    for (number, wire_type, value) in fields {
        out.extend_from_slice(&number.to_be_bytes());
        out.push(*wire_type as u8);
        out.extend_from_slice(&(value.len() as u32).to_be_bytes());
        out.extend_from_slice(value);
    }
    out
}
