//! Schema-4 control-plane storage: the additive migration surface and the
//! DTO-only repository implementations for the provider catalog, configuration
//! reloads, session provider defaults, resolved selections, the
//! unavailable-provider queue, provider usage, the removal lifecycle, held
//! recovered runs, and legacy M4 selection bindings.
//!
//! The migration is purely additive: existing M3/M4 tables and rows are never
//! rewritten. All schema-4 records are credential-free; credentials never
//! enter any schema-4 text column.

use intention_domain::{
    ContextPreservationCapability, CredentialTransportMode, ModelCapabilitySetV1,
    ProviderDriverContractRevisionDto, ProviderKindDescriptorRevisionV1, ProviderKindTombstoneDto,
    ProviderProfileRevisionV1, ProviderProfileTombstoneDto, ProviderSelectionV1,
    ReasoningCapability,
};
use intention_storage::{
    AcceptProviderCatalogInputDto, AcceptProviderCatalogRemovalInputDto,
    AdmitHeldRecoveredRunInputDto, AppendLegacyM4SelectionBindingInputDto,
    AppendProviderKindDescriptorRevisionInputDto, AppendProviderProfileRevisionInputDto,
    CommitConfigurationReloadInputDto, ConfigurationReloadRepositoryDto,
    CreateProviderCatalogRemovalCandidateInputDto, EnqueueUnavailableRunInputDto,
    ExpireProviderCatalogCandidateInputDto, ExpireProviderCatalogRemovalCandidateInputDto,
    HeldRecoveredRunDto, HeldRunAdmissionStateDto, HeldRunRepositoryDto,
    LegacyBindingRepositoryDto, LegacyBindingValidationStatusDto,
    LegacyM4SelectionBindingRecordDto, LoadProviderCatalogPageInputDto,
    LoadUnavailableQueuePageInputDto, MarkRecoveredRunHeldInputDto,
    PersistResolvedRunProviderSelectionInputDto, PromoteUnavailableRunsInputDto,
    PromoteUnavailableRunsOutcomeDto, ProviderCatalogMaterialDto, ProviderCatalogPageDto,
    ProviderCatalogProfileEntryDto, ProviderCatalogRemovalCandidateDto,
    ProviderCatalogRemovalStatusDto, ProviderCatalogRepositoryDto, ProviderCatalogStateDto,
    ProviderCatalogStatusDto, ProviderKindDescriptorCandidateDto, ProviderProfileCandidateDto,
    ProviderReadinessDto, ProviderRemovalRepositoryDto, ProviderSelectionRepositoryDto,
    ProviderUsageAggregateDto, ProviderUsageRepositoryDto, QueueReconciliationMarkerDto,
    ReconcileUnavailableQueueInputDto, ReconcileUnavailableQueueOutcomeDto,
    RecordProviderUsageInputDto, RejectProviderCatalogCandidateInputDto,
    RejectProviderCatalogRemovalInputDto, SessionProviderDefaultDto,
    SessionProviderDefaultsRepositoryDto, SetSessionProviderProfileInputDto,
    SetSessionProviderProfileOutcomeDto, UnavailableQueueRepositoryDto, UnavailableQueueStateDto,
    UnavailableRunQueueEntryDto,
};
use intention_types::{DtoResult, ErrorDto, RunId, SessionId};
use sqlite::OptionalExtension;

use super::{
    FaultPoint, SqliteStorageRepository, codec_error, conflict, not_found, storage_error,
    unavailable,
};

/// The provider catalog removal candidate lifetime in seconds (30 minutes).
const REMOVAL_CANDIDATE_LIFETIME_SECONDS: i64 = 30 * 60;
/// The maximum page size for the provider catalog projection.
const MAX_CATALOG_PAGE_SIZE: u64 = 1024;
/// The maximum page size for the unavailable-provider queue.
const MAX_QUEUE_PAGE_SIZE: u64 = 1024;
/// The maximum promotion batch per terminal transition.
const MAX_PROMOTION_BATCH: u64 = 8;
/// The maximum reconciliation batch per pass.
const MAX_RECONCILIATION_BATCH: u64 = 32;

/// The additive schema-4 migration surface: fourteen new tables, their
/// supporting indexes, and the seeded provider catalog state singleton. No
/// pre-existing table or row is touched.
pub const SCHEMA_M5_SQL: &str = "
CREATE TABLE provider_kind_descriptor_revisions (
  kind_id TEXT NOT NULL,
  descriptor_revision_id TEXT NOT NULL,
  descriptor_json TEXT NOT NULL,
  descriptor_digest TEXT NOT NULL UNIQUE,
  catalog_revision_id INTEGER NOT NULL,
  accepted_at INTEGER NOT NULL CHECK(accepted_at >= 0),
  PRIMARY KEY(kind_id, descriptor_revision_id)
);
CREATE INDEX provider_kind_descriptor_revisions_by_catalog ON provider_kind_descriptor_revisions(catalog_revision_id);
CREATE TABLE provider_profile_revisions (
  profile_id TEXT NOT NULL,
  profile_revision_id TEXT NOT NULL,
  kind_id TEXT NOT NULL,
  kind_descriptor_revision_id TEXT NOT NULL,
  provider_driver_contract_revision TEXT NOT NULL,
  model_id TEXT NOT NULL,
  normalized_effective_endpoint TEXT NOT NULL,
  credential_transport_mode TEXT NOT NULL CHECK(credential_transport_mode IN ('bearer','safe_header')),
  credential_transport_safe_header_name TEXT,
  declared_model_capability_subset_json TEXT NOT NULL,
  resolved_reasoning_policy TEXT NOT NULL,
  effective_execution_policy TEXT NOT NULL,
  effective_loopback_policy_or_not_applicable TEXT NOT NULL,
  profile_revision_json TEXT NOT NULL,
  profile_revision_digest TEXT NOT NULL UNIQUE,
  catalog_revision_id INTEGER NOT NULL,
  accepted_at INTEGER NOT NULL CHECK(accepted_at >= 0),
  PRIMARY KEY(profile_id, profile_revision_id),
  CHECK(
    (credential_transport_mode = 'bearer' AND credential_transport_safe_header_name IS NULL)
    OR (credential_transport_mode = 'safe_header' AND credential_transport_safe_header_name IS NOT NULL)
  )
);
CREATE INDEX provider_profile_revisions_by_catalog ON provider_profile_revisions(catalog_revision_id);
CREATE INDEX provider_profile_revisions_by_kind ON provider_profile_revisions(kind_id, kind_descriptor_revision_id);
CREATE TABLE provider_profile_tombstones (
  profile_id TEXT PRIMARY KEY,
  removed_catalog_revision_id INTEGER NOT NULL,
  removed_at INTEGER NOT NULL CHECK(removed_at >= 0),
  provenance TEXT NOT NULL,
  tombstone_json TEXT NOT NULL,
  tombstone_digest TEXT NOT NULL UNIQUE
);
CREATE TABLE provider_kind_tombstones (
  kind_id TEXT PRIMARY KEY,
  removed_catalog_revision_id INTEGER NOT NULL,
  removed_at INTEGER NOT NULL CHECK(removed_at >= 0),
  provenance TEXT NOT NULL,
  tombstone_json TEXT NOT NULL,
  tombstone_digest TEXT NOT NULL UNIQUE
);
CREATE TABLE provider_catalog_state (
  singleton_id INTEGER PRIMARY KEY CHECK(singleton_id = 1),
  active_catalog_revision_id INTEGER,
  candidate_catalog_revision_id INTEGER,
  status TEXT NOT NULL CHECK(status IN ('preparing','active','pending_removal','activation_recovery_required')),
  active_default_profile_id TEXT,
  candidate_handle TEXT,
  degraded_reason TEXT,
  updated_at INTEGER NOT NULL CHECK(updated_at >= 0)
);
INSERT INTO provider_catalog_state(singleton_id, active_catalog_revision_id, candidate_catalog_revision_id, status, active_default_profile_id, candidate_handle, degraded_reason, updated_at)
  VALUES (1, NULL, NULL, 'preparing', NULL, NULL, NULL, 0);
CREATE TABLE provider_catalog_profile_projection (
  projection_state TEXT NOT NULL CHECK(projection_state IN ('active','candidate')),
  catalog_revision_id INTEGER NOT NULL,
  profile_id TEXT NOT NULL,
  profile_revision_id TEXT NOT NULL,
  kind_id TEXT NOT NULL,
  kind_descriptor_revision_id TEXT NOT NULL,
  display_name TEXT,
  enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
  credential_configured INTEGER NOT NULL CHECK(credential_configured IN (0,1)),
  readiness TEXT NOT NULL CHECK(readiness IN ('ready','disabled','unavailable')),
  safe_projection_json TEXT NOT NULL,
  PRIMARY KEY(projection_state, catalog_revision_id, profile_id)
);
CREATE TABLE configuration_audit (
  audit_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  operation_id TEXT NOT NULL,
  audit_kind TEXT NOT NULL CHECK(audit_kind IN ('ProviderCatalogCandidatePrepared','ProviderCatalogRemovalPending','ProviderCatalogRemovalAccepted','ProviderCatalogCandidateRejected','ProviderCatalogCandidateExpired','ProviderCatalogAccepted','ProviderCatalogActivated','ProviderCatalogActivationRecoveryRequired','ProviderCatalogRecoveryCompleted')),
  catalog_revision_id INTEGER,
  config_revision_id TEXT,
  profile_id TEXT,
  run_id TEXT,
  occurred_at INTEGER NOT NULL CHECK(occurred_at >= 0),
  audit_json TEXT NOT NULL,
  UNIQUE(operation_id, audit_kind)
);
CREATE INDEX configuration_audit_by_catalog ON configuration_audit(catalog_revision_id, audit_sequence);
CREATE INDEX configuration_audit_by_run ON configuration_audit(run_id, audit_sequence);
CREATE TABLE session_provider_defaults (
  session_id TEXT PRIMARY KEY REFERENCES sessions(session_id),
  profile_id TEXT NOT NULL,
  projection_revision INTEGER NOT NULL CHECK(projection_revision >= 0),
  last_operation_id TEXT NOT NULL,
  updated_at INTEGER NOT NULL CHECK(updated_at >= 0)
);
CREATE INDEX session_provider_defaults_by_profile ON session_provider_defaults(profile_id);
CREATE TABLE resolved_run_provider_selections (
  run_id TEXT PRIMARY KEY REFERENCES runs(run_id),
  session_id TEXT NOT NULL REFERENCES sessions(session_id),
  selection_canonicalization_version TEXT NOT NULL,
  profile_id TEXT NOT NULL,
  provider_profile_revision_id TEXT NOT NULL,
  kind_id TEXT NOT NULL,
  kind_descriptor_revision_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  normalized_effective_endpoint TEXT NOT NULL,
  credential_transport_mode TEXT NOT NULL CHECK(credential_transport_mode IN ('bearer','safe_header')),
  credential_transport_safe_header_name TEXT,
  declared_model_capability_subset_json TEXT NOT NULL,
  resolved_reasoning_policy TEXT NOT NULL,
  effective_execution_policy TEXT NOT NULL,
  effective_loopback_policy_or_not_applicable TEXT NOT NULL,
  provider_driver_contract_revision TEXT NOT NULL,
  selection_source TEXT,
  selection_json TEXT NOT NULL,
  selection_digest TEXT NOT NULL UNIQUE,
  CHECK(
    (credential_transport_mode = 'bearer' AND credential_transport_safe_header_name IS NULL)
    OR (credential_transport_mode = 'safe_header' AND credential_transport_safe_header_name IS NOT NULL)
  )
);
CREATE INDEX resolved_run_provider_selections_by_profile ON resolved_run_provider_selections(profile_id, provider_profile_revision_id);
CREATE INDEX resolved_run_provider_selections_by_session ON resolved_run_provider_selections(session_id);
CREATE TABLE unavailable_provider_queue (
  queue_id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL UNIQUE REFERENCES runs(run_id),
  session_id TEXT NOT NULL REFERENCES sessions(session_id),
  profile_id TEXT NOT NULL,
  provider_profile_revision_id TEXT NOT NULL,
  unavailable_reason TEXT NOT NULL,
  first_unavailable_at INTEGER NOT NULL CHECK(first_unavailable_at >= 0),
  promotion_attempts INTEGER NOT NULL DEFAULT 0 CHECK(promotion_attempts >= 0),
  state TEXT NOT NULL CHECK(state IN ('queued','terminalized','promoted')),
  last_operation_id TEXT,
  selection_json TEXT NOT NULL,
  UNIQUE(session_id, run_id)
);
CREATE INDEX unavailable_provider_queue_fifo ON unavailable_provider_queue(state, queue_id);
CREATE INDEX unavailable_provider_queue_by_profile ON unavailable_provider_queue(profile_id, provider_profile_revision_id);
CREATE TABLE unavailable_queue_reconciliation_markers (
  marker_id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  created_at INTEGER NOT NULL CHECK(created_at >= 0),
  reason TEXT NOT NULL,
  next_page_cursor TEXT,
  resolved_at INTEGER,
  UNIQUE(session_id, reason, resolved_at)
);
CREATE TABLE provider_usage_aggregates (
  profile_id TEXT NOT NULL,
  provider_profile_revision_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  usage_period_start INTEGER NOT NULL CHECK(usage_period_start >= 0),
  usage_period_end INTEGER NOT NULL CHECK(usage_period_end >= usage_period_start),
  request_count INTEGER NOT NULL CHECK(request_count >= 0),
  input_units INTEGER NOT NULL CHECK(input_units >= 0),
  output_units INTEGER NOT NULL CHECK(output_units >= 0),
  reasoning_units INTEGER NOT NULL CHECK(reasoning_units >= 0),
  last_run_id TEXT,
  updated_at INTEGER NOT NULL CHECK(updated_at >= 0),
  PRIMARY KEY(profile_id, provider_profile_revision_id, model_id, usage_period_start, usage_period_end)
);
CREATE INDEX provider_usage_aggregates_by_revision ON provider_usage_aggregates(provider_profile_revision_id, model_id, usage_period_start);
CREATE TABLE provider_usage_facts (
  run_id TEXT NOT NULL REFERENCES runs(run_id),
  usage_event_id TEXT NOT NULL,
  profile_id TEXT NOT NULL,
  provider_profile_revision_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  input_units INTEGER NOT NULL CHECK(input_units >= 0),
  output_units INTEGER NOT NULL CHECK(output_units >= 0),
  reasoning_units INTEGER NOT NULL CHECK(reasoning_units >= 0),
  occurred_at INTEGER NOT NULL CHECK(occurred_at >= 0),
  usage_json TEXT NOT NULL,
  PRIMARY KEY(run_id, usage_event_id)
);
CREATE INDEX provider_usage_facts_by_profile ON provider_usage_facts(profile_id, provider_profile_revision_id, model_id);
CREATE TABLE provider_catalog_removal_candidates (
  candidate_handle TEXT PRIMARY KEY,
  candidate_catalog_revision_id INTEGER NOT NULL UNIQUE,
  active_catalog_revision_id INTEGER NOT NULL,
  created_at INTEGER NOT NULL CHECK(created_at >= 0),
  expires_at INTEGER NOT NULL CHECK(expires_at > created_at),
  source_recheck TEXT NOT NULL,
  status TEXT NOT NULL CHECK(status IN ('pending','accepted','rejected','expired')),
  candidate_json TEXT NOT NULL,
  operation_id TEXT,
  completed_at INTEGER
);
CREATE UNIQUE INDEX one_pending_provider_catalog_candidate ON provider_catalog_removal_candidates(status) WHERE status = 'pending';
CREATE TABLE held_recovered_runs (
  run_id TEXT PRIMARY KEY REFERENCES runs(run_id),
  session_id TEXT NOT NULL REFERENCES sessions(session_id),
  held_at INTEGER NOT NULL CHECK(held_at >= 0),
  reason TEXT NOT NULL CHECK(reason = 'recovered_run_requires_explicit_admission'),
  admission_state TEXT NOT NULL CHECK(admission_state IN ('held','admitted','rejected')),
  admission_operation_id TEXT,
  admitted_at INTEGER,
  UNIQUE(session_id, run_id)
);
CREATE INDEX held_recovered_runs_by_admission ON held_recovered_runs(admission_state, held_at);
CREATE TABLE legacy_m4_selection_bindings (
  config_revision_id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL,
  provider_profile_revision_id TEXT NOT NULL,
  kind_id TEXT NOT NULL,
  kind_descriptor_revision_id TEXT NOT NULL,
  provider_driver_contract_revision TEXT NOT NULL,
  binding_digest TEXT NOT NULL UNIQUE,
  snapshot_bytes_digest TEXT NOT NULL,
  validation_status TEXT NOT NULL CHECK(validation_status IN ('validated','corrupt')),
  binding_json TEXT NOT NULL,
  created_at INTEGER NOT NULL CHECK(created_at >= 0)
);
CREATE INDEX legacy_m4_selection_bindings_by_profile ON legacy_m4_selection_bindings(profile_id, provider_profile_revision_id);
";

// ---------------------------------------------------------------------------
// Typed JSON codecs for safe schema-4 records.
//
// The schema-4 JSON columns are built and read through typed record structs
// instead of untyped JSON values. Every key, value type, and null/omission
// rule below mirrors the exact bytes the earlier Value-based builders
// produced (keys emitted in sorted order, string values escaped by
// serde_json itself), so stored records and their digests remain
// byte-identical. Value spans extracted from persisted records are decoded
// with `serde_json::from_str` against the concrete Rust type of each field.
// ---------------------------------------------------------------------------

/// One field of a persisted JSON object: absent, explicit null, or a
/// non-null raw JSON value span.
enum RawJsonField<'a> {
    /// The key is not present in the object.
    Missing,
    /// The key is present with the JSON `null` value.
    Null,
    /// The key is present with a non-null raw JSON value span.
    Value(&'a str),
}

/// Scans one JSON object record and returns the raw span of the last
/// occurrence of `key`, preserving the distinction between an absent key,
/// an explicit `null`, and a present non-null value. Structural errors in
/// the record are rejected so malformed persisted records fail to decode.
fn json_object_field<'a>(encoded: &'a str, key: &str) -> DtoResult<RawJsonField<'a>> {
    let bytes = encoded.as_bytes();
    let quoted_key = format!("\"{key}\"");
    let mut index = skip_json_whitespace(bytes, 0);
    if index >= bytes.len() || bytes[index] != b'{' {
        return Err(codec_error(
            "persisted record field is missing or malformed",
        ));
    }
    index += 1;
    let mut found = RawJsonField::Missing;
    loop {
        index = skip_json_whitespace(bytes, index);
        if index >= bytes.len() {
            return Err(codec_error(
                "persisted record field is missing or malformed",
            ));
        }
        if bytes[index] == b'}' {
            return Ok(found);
        }
        let (member, after_member) = json_string_span(bytes, index)?;
        index = skip_json_whitespace(bytes, after_member);
        if index >= bytes.len() || bytes[index] != b':' {
            return Err(codec_error(
                "persisted record field is missing or malformed",
            ));
        }
        index = skip_json_whitespace(bytes, index + 1);
        let (value, after_value) = json_value_span(bytes, index)?;
        index = after_value;
        if member == quoted_key {
            found = if value == "null" {
                RawJsonField::Null
            } else {
                RawJsonField::Value(value)
            };
        }
        index = skip_json_whitespace(bytes, index);
        if index >= bytes.len() {
            return Err(codec_error(
                "persisted record field is missing or malformed",
            ));
        }
        match bytes[index] {
            b',' => index += 1,
            b'}' => return Ok(found),
            _ => {
                return Err(codec_error(
                    "persisted record field is missing or malformed",
                ));
            }
        }
    }
}

/// Skips JSON whitespace and returns the first non-whitespace index.
fn skip_json_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t' | b'\r' | b'\n') {
        index += 1;
    }
    index
}

/// Locates one JSON string at `start` (including its quotes) and returns
/// its raw span plus the index just past the closing quote.
fn json_string_span(bytes: &[u8], start: usize) -> DtoResult<(&str, usize)> {
    if start >= bytes.len() || bytes[start] != b'"' {
        return Err(codec_error(
            "persisted record field is missing or malformed",
        ));
    }
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index = index.saturating_add(2),
            b'"' => {
                let span = std::str::from_utf8(&bytes[start..index + 1])
                    .map_err(|_| codec_error("persisted record field is missing or malformed"))?;
                return Ok((span, index + 1));
            }
            _ => index += 1,
        }
    }
    Err(codec_error(
        "persisted record field is missing or malformed",
    ))
}

/// Locates one JSON value at `start` and returns its raw span plus the
/// index just past the value. Nested objects, arrays, and strings are
/// walked so the span ends at the first top-level comma or brace.
fn json_value_span(bytes: &[u8], start: usize) -> DtoResult<(&str, usize)> {
    if start >= bytes.len() {
        return Err(codec_error(
            "persisted record field is missing or malformed",
        ));
    }
    let end = match bytes[start] {
        b'"' => json_string_span(bytes, start)?.1,
        b'{' | b'[' => {
            let mut depth = 1_u32;
            let mut index = start + 1;
            while index < bytes.len() && depth > 0 {
                match bytes[index] {
                    b'"' => index = json_string_span(bytes, index)?.1,
                    b'{' | b'[' => {
                        depth += 1;
                        index += 1;
                    }
                    b'}' | b']' => {
                        depth -= 1;
                        index += 1;
                    }
                    _ => index += 1,
                }
            }
            if depth != 0 {
                return Err(codec_error(
                    "persisted record field is missing or malformed",
                ));
            }
            index
        }
        b't' => {
            if bytes.get(start..start + 4) != Some(b"true") {
                return Err(codec_error(
                    "persisted record field is missing or malformed",
                ));
            }
            start + 4
        }
        b'f' => {
            if bytes.get(start..start + 5) != Some(b"false") {
                return Err(codec_error(
                    "persisted record field is missing or malformed",
                ));
            }
            start + 5
        }
        b'n' => {
            if bytes.get(start..start + 4) != Some(b"null") {
                return Err(codec_error(
                    "persisted record field is missing or malformed",
                ));
            }
            start + 4
        }
        _ => {
            let mut index = start;
            while index < bytes.len() && is_json_number_byte(bytes[index]) {
                index += 1;
            }
            index
        }
    };
    let span = std::str::from_utf8(&bytes[start..end])
        .map_err(|_| codec_error("persisted record field is missing or malformed"))?;
    Ok((span, end))
}

const fn is_json_number_byte(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E')
}

/// Decodes one required string field, rejecting absent and null values.
fn decode_json_string(field: RawJsonField<'_>) -> DtoResult<String> {
    match field {
        RawJsonField::Missing | RawJsonField::Null => Err(codec_error(
            "persisted record field is missing or malformed",
        )),
        RawJsonField::Value(span) => serde_json::from_str(span)
            .map_err(|_| codec_error("persisted record field is missing or malformed")),
    }
}

/// Decodes one optional string field: absent and null both read as `None`.
fn decode_json_optional_string(field: RawJsonField<'_>) -> DtoResult<Option<String>> {
    match field {
        RawJsonField::Missing | RawJsonField::Null => Ok(None),
        RawJsonField::Value(span) => serde_json::from_str(span)
            .map(Some)
            .map_err(|_| codec_error("persisted record field is malformed")),
    }
}

/// Decodes one required list-of-strings field.
fn decode_json_string_list(field: RawJsonField<'_>) -> DtoResult<Vec<String>> {
    match field {
        RawJsonField::Missing | RawJsonField::Null => {
            Err(codec_error("persisted list field is missing or malformed"))
        }
        RawJsonField::Value(span) => serde_json::from_str(span)
            .map_err(|_| codec_error("persisted list field is missing or malformed")),
    }
}

/// Decodes one required boolean field.
fn decode_json_bool(field: RawJsonField<'_>) -> DtoResult<bool> {
    match field {
        RawJsonField::Missing | RawJsonField::Null => Err(codec_error(
            "persisted boolean field is missing or malformed",
        )),
        RawJsonField::Value(span) => serde_json::from_str(span)
            .map_err(|_| codec_error("persisted boolean field is missing or malformed")),
    }
}

/// Decodes one required unsigned integer field, reporting `missing_message`
/// for absent, null, or malformed values.
fn decode_json_u64(field: RawJsonField<'_>, missing_message: &str) -> DtoResult<u64> {
    match field {
        RawJsonField::Missing | RawJsonField::Null => Err(codec_error(missing_message)),
        RawJsonField::Value(span) => {
            serde_json::from_str(span).map_err(|_| codec_error(missing_message))
        }
    }
}

/// Encodes one string with serde_json's exact JSON escaping. Plain string
/// serialization cannot fail, so the fallback is unreachable in practice.
fn encode_json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

/// Encodes one optional string as a JSON string or an explicit `null`.
fn encode_json_optional_string(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), encode_json_string)
}

/// Encodes a list of strings as one JSON array; see [`encode_json_string`].
fn encode_json_string_list(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_owned())
}

fn encode_json_bool(value: bool) -> String {
    if value {
        "true".to_owned()
    } else {
        "false".to_owned()
    }
}

fn encode_json_u64(value: u64) -> String {
    value.to_string()
}

/// Typed encoding of one model capability envelope.
#[derive(Clone, Debug)]
struct CapabilityEnvelopeJson {
    context_preservation: ContextPreservationJson,
    input: String,
    reasoning: String,
    structured_output: String,
    taxonomy_version: String,
    text_streaming: bool,
    tool_exchange: bool,
}

impl CapabilityEnvelopeJson {
    fn to_json(&self) -> String {
        format!(
            "{{\"context_preservation\":{},\"input\":{},\"reasoning\":{},\"structured_output\":{},\"taxonomy_version\":{},\"text_streaming\":{},\"tool_exchange\":{}}}",
            self.context_preservation.to_json(),
            encode_json_string(&self.input),
            encode_json_string(&self.reasoning),
            encode_json_string(&self.structured_output),
            encode_json_string(&self.taxonomy_version),
            encode_json_bool(self.text_streaming),
            encode_json_bool(self.tool_exchange),
        )
    }

    fn from_json(encoded: &str) -> DtoResult<Self> {
        let context_preservation = match json_object_field(encoded, "context_preservation")? {
            RawJsonField::Missing | RawJsonField::Null => {
                return Err(codec_error("capability context preservation missing"));
            }
            RawJsonField::Value(span) => span,
        };
        let local_durable_history_v1 =
            match json_object_field(context_preservation, "local_durable_history_v1")? {
                RawJsonField::Missing | RawJsonField::Null => {
                    return Err(codec_error("capability context preservation missing"));
                }
                RawJsonField::Value(span) => span,
            };
        Ok(Self {
            context_preservation: ContextPreservationJson {
                local_durable_history_v1: LocalDurableHistoryV1Json {
                    reasoning_input_contract: decode_json_string(json_object_field(
                        local_durable_history_v1,
                        "reasoning_input_contract",
                    )?)?,
                },
            },
            input: "text_only".to_owned(),
            reasoning: decode_json_string(json_object_field(encoded, "reasoning")?)?,
            structured_output: "unsupported".to_owned(),
            taxonomy_version: decode_json_string(json_object_field(encoded, "taxonomy_version")?)?,
            text_streaming: decode_json_bool(json_object_field(encoded, "text_streaming")?)?,
            tool_exchange: decode_json_bool(json_object_field(encoded, "tool_exchange")?)?,
        })
    }
}

/// Typed encoding of the context-preservation capability entry.
#[derive(Clone, Debug)]
struct ContextPreservationJson {
    local_durable_history_v1: LocalDurableHistoryV1Json,
}

impl ContextPreservationJson {
    fn to_json(&self) -> String {
        format!(
            "{{\"local_durable_history_v1\":{}}}",
            self.local_durable_history_v1.to_json()
        )
    }
}

/// Typed encoding of the local durable history entry.
#[derive(Clone, Debug)]
struct LocalDurableHistoryV1Json {
    reasoning_input_contract: String,
}

impl LocalDurableHistoryV1Json {
    fn to_json(&self) -> String {
        format!(
            "{{\"reasoning_input_contract\":{}}}",
            encode_json_string(&self.reasoning_input_contract)
        )
    }
}

/// Typed encoding of one kind descriptor revision record.
#[derive(Clone, Debug)]
struct KindDescriptorJson {
    credential_transport_contract: String,
    descriptor_family: String,
    driver_contract_family: String,
    endpoint_policy: String,
    kind_id: String,
    model_capability_envelope: CapabilityEnvelopeJson,
    ordered_protocol_part_revisions: Vec<String>,
}

impl KindDescriptorJson {
    fn to_json(&self) -> String {
        format!(
            "{{\"credential_transport_contract\":{},\"descriptor_family\":{},\"driver_contract_family\":{},\"endpoint_policy\":{},\"kind_id\":{},\"model_capability_envelope\":{},\"ordered_protocol_part_revisions\":{}}}",
            encode_json_string(&self.credential_transport_contract),
            encode_json_string(&self.descriptor_family),
            encode_json_string(&self.driver_contract_family),
            encode_json_string(&self.endpoint_policy),
            encode_json_string(&self.kind_id),
            self.model_capability_envelope.to_json(),
            encode_json_string_list(&self.ordered_protocol_part_revisions),
        )
    }

    fn from_json(encoded: &str) -> DtoResult<Self> {
        let envelope = match json_object_field(encoded, "model_capability_envelope")? {
            RawJsonField::Missing | RawJsonField::Null => {
                return Err(codec_error("kind descriptor envelope missing"));
            }
            RawJsonField::Value(span) => span,
        };
        Ok(Self {
            credential_transport_contract: decode_json_string(json_object_field(
                encoded,
                "credential_transport_contract",
            )?)?,
            descriptor_family: decode_json_string(json_object_field(
                encoded,
                "descriptor_family",
            )?)?,
            driver_contract_family: decode_json_string(json_object_field(
                encoded,
                "driver_contract_family",
            )?)?,
            endpoint_policy: decode_json_string(json_object_field(encoded, "endpoint_policy")?)?,
            kind_id: decode_json_string(json_object_field(encoded, "kind_id")?)?,
            model_capability_envelope: CapabilityEnvelopeJson::from_json(envelope)?,
            ordered_protocol_part_revisions: decode_json_string_list(json_object_field(
                encoded,
                "ordered_protocol_part_revisions",
            )?)?,
        })
    }
}

/// Typed encoding of one profile revision record.
#[derive(Clone, Debug)]
struct ProfileJson {
    capability_taxonomy_revision: String,
    credential_transport_mode: String,
    driver_contract_revision: DriverContractRevisionJson,
    endpoint: String,
    kind_descriptor_revision_id: String,
    model_id: String,
    profile_id: String,
    provider_kind_id: String,
    reasoning_compatibility_id: Option<String>,
    revision_id: String,
    safe_header_name: Option<String>,
}

impl ProfileJson {
    fn to_json(&self) -> String {
        format!(
            "{{\"capability_taxonomy_revision\":{},\"credential_transport_mode\":{},\"driver_contract_revision\":{},\"endpoint\":{},\"kind_descriptor_revision_id\":{},\"model_id\":{},\"profile_id\":{},\"provider_kind_id\":{},\"reasoning_compatibility_id\":{},\"revision_id\":{},\"safe_header_name\":{}}}",
            encode_json_string(&self.capability_taxonomy_revision),
            encode_json_string(&self.credential_transport_mode),
            self.driver_contract_revision.to_json(),
            encode_json_string(&self.endpoint),
            encode_json_string(&self.kind_descriptor_revision_id),
            encode_json_string(&self.model_id),
            encode_json_string(&self.profile_id),
            encode_json_string(&self.provider_kind_id),
            encode_json_optional_string(self.reasoning_compatibility_id.as_deref()),
            encode_json_string(&self.revision_id),
            encode_json_optional_string(self.safe_header_name.as_deref()),
        )
    }

    fn from_json(encoded: &str) -> DtoResult<Self> {
        let contract = match json_object_field(encoded, "driver_contract_revision")? {
            RawJsonField::Missing => return Err(codec_error("profile driver contract missing")),
            RawJsonField::Null => {
                return Err(codec_error(
                    "persisted record field is missing or malformed",
                ));
            }
            RawJsonField::Value(span) => span,
        };
        Ok(Self {
            capability_taxonomy_revision: decode_json_string(json_object_field(
                encoded,
                "capability_taxonomy_revision",
            )?)?,
            credential_transport_mode: decode_json_string(json_object_field(
                encoded,
                "credential_transport_mode",
            )?)?,
            driver_contract_revision: DriverContractRevisionJson {
                driver_family: decode_json_string(json_object_field(contract, "driver_family")?)?,
                major: decode_json_u64(
                    json_object_field(contract, "major")?,
                    "profile driver contract major missing",
                )?,
                minor: decode_json_u64(
                    json_object_field(contract, "minor")?,
                    "profile driver contract minor missing",
                )?,
            },
            endpoint: decode_json_string(json_object_field(encoded, "endpoint")?)?,
            kind_descriptor_revision_id: decode_json_string(json_object_field(
                encoded,
                "kind_descriptor_revision_id",
            )?)?,
            model_id: decode_json_string(json_object_field(encoded, "model_id")?)?,
            profile_id: decode_json_string(json_object_field(encoded, "profile_id")?)?,
            provider_kind_id: decode_json_string(json_object_field(encoded, "provider_kind_id")?)?,
            reasoning_compatibility_id: decode_json_optional_string(json_object_field(
                encoded,
                "reasoning_compatibility_id",
            )?)?,
            revision_id: decode_json_string(json_object_field(encoded, "revision_id")?)?,
            safe_header_name: decode_json_optional_string(json_object_field(
                encoded,
                "safe_header_name",
            )?)?,
        })
    }
}

/// Typed encoding of the nested driver contract revision entry.
#[derive(Clone, Debug)]
struct DriverContractRevisionJson {
    driver_family: String,
    major: u64,
    minor: u64,
}

impl DriverContractRevisionJson {
    fn to_json(&self) -> String {
        format!(
            "{{\"driver_family\":{},\"major\":{},\"minor\":{}}}",
            encode_json_string(&self.driver_family),
            encode_json_u64(self.major),
            encode_json_u64(self.minor),
        )
    }
}

/// Typed encoding of one resolved provider selection record.
#[derive(Clone, Debug)]
struct SelectionJson {
    credential_transport_mode: String,
    credential_transport_safe_header_name: Option<String>,
    declared_model_capability_subset: Vec<String>,
    effective_execution_policy: String,
    effective_loopback_policy_or_not_applicable: String,
    kind_descriptor_revision_id: String,
    kind_id: String,
    model_id: String,
    normalized_effective_endpoint: String,
    profile_id: String,
    provider_driver_contract_revision: String,
    provider_profile_revision_id: String,
    resolved_reasoning_policy: String,
    selection_canonicalization_version: String,
    selection_source: Option<String>,
}

impl SelectionJson {
    fn to_json(&self) -> String {
        format!(
            "{{\"credential_transport_mode\":{},\"credential_transport_safe_header_name\":{},\"declared_model_capability_subset\":{},\"effective_execution_policy\":{},\"effective_loopback_policy_or_not_applicable\":{},\"kind_descriptor_revision_id\":{},\"kind_id\":{},\"model_id\":{},\"normalized_effective_endpoint\":{},\"profile_id\":{},\"provider_driver_contract_revision\":{},\"provider_profile_revision_id\":{},\"resolved_reasoning_policy\":{},\"selection_canonicalization_version\":{},\"selection_source\":{}}}",
            encode_json_string(&self.credential_transport_mode),
            encode_json_optional_string(self.credential_transport_safe_header_name.as_deref()),
            encode_json_string_list(&self.declared_model_capability_subset),
            encode_json_string(&self.effective_execution_policy),
            encode_json_string(&self.effective_loopback_policy_or_not_applicable),
            encode_json_string(&self.kind_descriptor_revision_id),
            encode_json_string(&self.kind_id),
            encode_json_string(&self.model_id),
            encode_json_string(&self.normalized_effective_endpoint),
            encode_json_string(&self.profile_id),
            encode_json_string(&self.provider_driver_contract_revision),
            encode_json_string(&self.provider_profile_revision_id),
            encode_json_string(&self.resolved_reasoning_policy),
            encode_json_string(&self.selection_canonicalization_version),
            encode_json_optional_string(self.selection_source.as_deref()),
        )
    }

    fn from_json(encoded: &str) -> DtoResult<Self> {
        Ok(Self {
            credential_transport_mode: decode_json_string(json_object_field(
                encoded,
                "credential_transport_mode",
            )?)?,
            credential_transport_safe_header_name: decode_json_optional_string(json_object_field(
                encoded,
                "credential_transport_safe_header_name",
            )?)?,
            declared_model_capability_subset: decode_json_string_list(json_object_field(
                encoded,
                "declared_model_capability_subset",
            )?)?,
            effective_execution_policy: decode_json_string(json_object_field(
                encoded,
                "effective_execution_policy",
            )?)?,
            effective_loopback_policy_or_not_applicable: decode_json_string(json_object_field(
                encoded,
                "effective_loopback_policy_or_not_applicable",
            )?)?,
            kind_descriptor_revision_id: decode_json_string(json_object_field(
                encoded,
                "kind_descriptor_revision_id",
            )?)?,
            kind_id: decode_json_string(json_object_field(encoded, "kind_id")?)?,
            model_id: decode_json_string(json_object_field(encoded, "model_id")?)?,
            normalized_effective_endpoint: decode_json_string(json_object_field(
                encoded,
                "normalized_effective_endpoint",
            )?)?,
            profile_id: decode_json_string(json_object_field(encoded, "profile_id")?)?,
            provider_driver_contract_revision: decode_json_string(json_object_field(
                encoded,
                "provider_driver_contract_revision",
            )?)?,
            provider_profile_revision_id: decode_json_string(json_object_field(
                encoded,
                "provider_profile_revision_id",
            )?)?,
            resolved_reasoning_policy: decode_json_string(json_object_field(
                encoded,
                "resolved_reasoning_policy",
            )?)?,
            selection_canonicalization_version: decode_json_string(json_object_field(
                encoded,
                "selection_canonicalization_version",
            )?)?,
            selection_source: decode_json_optional_string(json_object_field(
                encoded,
                "selection_source",
            )?)?,
        })
    }
}

/// Typed encoding of one profile tombstone record.
#[derive(Clone, Debug)]
struct ProfileTombstoneJson {
    digest: String,
    profile_id: String,
    provenance: String,
    removed_catalog_revision: u64,
    removed_time: u64,
}

impl ProfileTombstoneJson {
    fn to_json(&self) -> String {
        format!(
            "{{\"digest\":{},\"profile_id\":{},\"provenance\":{},\"removed_catalog_revision\":{},\"removed_time\":{}}}",
            encode_json_string(&self.digest),
            encode_json_string(&self.profile_id),
            encode_json_string(&self.provenance),
            encode_json_u64(self.removed_catalog_revision),
            encode_json_u64(self.removed_time),
        )
    }
}

/// Typed encoding of one kind tombstone record.
#[derive(Clone, Debug)]
struct KindTombstoneJson {
    digest: String,
    kind_id: String,
    provenance: String,
    removed_catalog_revision: u64,
    removed_time: u64,
}

impl KindTombstoneJson {
    fn to_json(&self) -> String {
        format!(
            "{{\"digest\":{},\"kind_id\":{},\"provenance\":{},\"removed_catalog_revision\":{},\"removed_time\":{}}}",
            encode_json_string(&self.digest),
            encode_json_string(&self.kind_id),
            encode_json_string(&self.provenance),
            encode_json_u64(self.removed_catalog_revision),
            encode_json_u64(self.removed_time),
        )
    }
}

/// Typed encoding of one legacy M4 selection binding record.
#[derive(Clone, Debug)]
struct LegacyBindingJson {
    capability_subset: Vec<String>,
    default_profile_id: String,
    default_profile_revision_id: String,
    driver_contract_revision: String,
    execution_policy: String,
    kind_descriptor_revision_id: String,
    legacy_config_revision_id: String,
    legacy_safe_selection: String,
    legacy_snapshot_schema: String,
}

impl LegacyBindingJson {
    fn to_json(&self) -> String {
        format!(
            "{{\"capability_subset\":{},\"default_profile_id\":{},\"default_profile_revision_id\":{},\"driver_contract_revision\":{},\"execution_policy\":{},\"kind_descriptor_revision_id\":{},\"legacy_config_revision_id\":{},\"legacy_safe_selection\":{},\"legacy_snapshot_schema\":{}}}",
            encode_json_string_list(&self.capability_subset),
            encode_json_string(&self.default_profile_id),
            encode_json_string(&self.default_profile_revision_id),
            encode_json_string(&self.driver_contract_revision),
            encode_json_string(&self.execution_policy),
            encode_json_string(&self.kind_descriptor_revision_id),
            encode_json_string(&self.legacy_config_revision_id),
            encode_json_string(&self.legacy_safe_selection),
            encode_json_string(&self.legacy_snapshot_schema),
        )
    }
}

/// Typed encoding of one corrupt legacy M4 selection binding record.
#[derive(Clone, Debug)]
struct CorruptLegacyBindingJson {
    config_revision_id: String,
    corrupt: bool,
}

impl CorruptLegacyBindingJson {
    fn to_json(&self) -> String {
        format!(
            "{{\"config_revision_id\":{},\"corrupt\":{}}}",
            encode_json_string(&self.config_revision_id),
            encode_json_bool(self.corrupt),
        )
    }
}

/// Typed encoding of one safe provider catalog projection record.
#[derive(Clone, Debug)]
struct SafeProjectionJson {
    credential_transport_mode: String,
    credential_transport_safe_header_name: Option<String>,
    declared_model_capability_subset: Vec<String>,
    effective_execution_policy: String,
    effective_loopback_policy_or_not_applicable: String,
    kind_descriptor_revision_id: String,
    kind_id: String,
    model_id: String,
    normalized_effective_endpoint: String,
    profile_id: String,
    profile_revision_id: String,
    provider_driver_contract_revision: String,
    resolved_reasoning_policy: String,
}

impl SafeProjectionJson {
    fn to_json(&self) -> String {
        format!(
            "{{\"credential_transport_mode\":{},\"credential_transport_safe_header_name\":{},\"declared_model_capability_subset\":{},\"effective_execution_policy\":{},\"effective_loopback_policy_or_not_applicable\":{},\"kind_descriptor_revision_id\":{},\"kind_id\":{},\"model_id\":{},\"normalized_effective_endpoint\":{},\"profile_id\":{},\"profile_revision_id\":{},\"provider_driver_contract_revision\":{},\"resolved_reasoning_policy\":{}}}",
            encode_json_string(&self.credential_transport_mode),
            encode_json_optional_string(self.credential_transport_safe_header_name.as_deref()),
            encode_json_string_list(&self.declared_model_capability_subset),
            encode_json_string(&self.effective_execution_policy),
            encode_json_string(&self.effective_loopback_policy_or_not_applicable),
            encode_json_string(&self.kind_descriptor_revision_id),
            encode_json_string(&self.kind_id),
            encode_json_string(&self.model_id),
            encode_json_string(&self.normalized_effective_endpoint),
            encode_json_string(&self.profile_id),
            encode_json_string(&self.profile_revision_id),
            encode_json_string(&self.provider_driver_contract_revision),
            encode_json_string(&self.resolved_reasoning_policy),
        )
    }
}

/// Typed encoding of one configuration audit payload. The candidate handle
/// and default profile keys are present only for the audits that produced
/// them (the default profile key only on catalog acceptance, the candidate
/// handle key always present but possibly `null` on the candidate audits),
/// and the remaining keys appear only when their constructor set them,
/// matching the exact payload bytes the Value-based audit builder produced.
#[derive(Clone, Debug, Default)]
struct AuditRecordJson {
    candidate_handle: Option<String>,
    default_profile_id: Option<String>,
    include_candidate_handle: bool,
    include_default_profile_id: bool,
    kind_id: Option<String>,
    profile_id: Option<String>,
    reloaded: Option<bool>,
}

impl AuditRecordJson {
    fn kind_descriptor_prepared(kind_id: &str) -> Self {
        Self {
            kind_id: Some(kind_id.to_owned()),
            ..Self::default()
        }
    }

    fn profile_prepared(profile_id: &str) -> Self {
        Self {
            profile_id: Some(profile_id.to_owned()),
            ..Self::default()
        }
    }

    fn candidate_handle(handle: Option<String>) -> Self {
        Self {
            candidate_handle: handle,
            include_candidate_handle: true,
            ..Self::default()
        }
    }

    fn catalog_accepted(candidate_handle: String, default_profile_id: String) -> Self {
        Self {
            candidate_handle: Some(candidate_handle),
            default_profile_id: Some(default_profile_id),
            include_candidate_handle: true,
            include_default_profile_id: true,
            ..Self::default()
        }
    }

    fn reloaded() -> Self {
        Self {
            reloaded: Some(true),
            ..Self::default()
        }
    }

    fn to_json(&self) -> String {
        let mut fields: Vec<(&str, String)> = Vec::new();
        if self.include_candidate_handle {
            fields.push((
                "candidate_handle",
                encode_json_optional_string(self.candidate_handle.as_deref()),
            ));
        }
        if self.include_default_profile_id {
            fields.push((
                "default_profile_id",
                encode_json_optional_string(self.default_profile_id.as_deref()),
            ));
        }
        if let Some(kind_id) = &self.kind_id {
            fields.push(("kind_id", encode_json_string(kind_id)));
        }
        if let Some(profile_id) = &self.profile_id {
            fields.push(("profile_id", encode_json_string(profile_id)));
        }
        if let Some(reloaded) = self.reloaded {
            fields.push(("reloaded", encode_json_bool(reloaded)));
        }
        let mut encoded = String::from("{");
        for (index, (key, value)) in fields.iter().enumerate() {
            if index > 0 {
                encoded.push(',');
            }
            encoded.push('"');
            encoded.push_str(key);
            encoded.push_str("\":");
            encoded.push_str(value);
        }
        encoded.push('}');
        encoded
    }
}

/// Typed encoding of one provider catalog page token.
#[derive(Clone, Debug)]
struct CatalogTokenJson {
    after: Option<String>,
    revision: u64,
}

impl CatalogTokenJson {
    fn to_json(&self) -> String {
        format!(
            "{{\"after\":{},\"revision\":{}}}",
            encode_json_optional_string(self.after.as_deref()),
            encode_json_u64(self.revision),
        )
    }

    fn from_json(encoded: &str) -> DtoResult<Self> {
        // The trailing cursor is read leniently, exactly like the previous
        // `as_str` extraction: a non-string value means "no cursor".
        let after = match json_object_field(encoded, "after")? {
            RawJsonField::Missing | RawJsonField::Null => None,
            RawJsonField::Value(span) => serde_json::from_str(span).ok(),
        };
        Ok(Self {
            after,
            revision: decode_json_u64(
                json_object_field(encoded, "revision")?,
                "persisted record field is missing or malformed",
            )?,
        })
    }
}

/// Computes the deterministic record digest over the canonical JSON encoding
/// of one safe schema-4 record.
fn record_digest(encoded: &str) -> String {
    intention_domain::canonical::Digest256::sha256(encoded.as_bytes()).to_string()
}

const fn transport_mode_name(mode: CredentialTransportMode) -> &'static str {
    match mode {
        CredentialTransportMode::Bearer => "bearer",
        CredentialTransportMode::SafeHeader => "safe_header",
    }
}

fn capability_envelope_json(envelope: &ModelCapabilitySetV1) -> CapabilityEnvelopeJson {
    let reasoning = match envelope.reasoning {
        ReasoningCapability::Disabled => "disabled",
        ReasoningCapability::TextualReasoningV1 => "textual_reasoning_v1",
    };
    CapabilityEnvelopeJson {
        context_preservation: match &envelope.context_preservation {
            ContextPreservationCapability::LocalDurableHistoryV1 {
                reasoning_input_contract,
            } => ContextPreservationJson {
                local_durable_history_v1: LocalDurableHistoryV1Json {
                    reasoning_input_contract: reasoning_input_contract.clone(),
                },
            },
        },
        input: "text_only".to_owned(),
        reasoning: reasoning.to_owned(),
        structured_output: "unsupported".to_owned(),
        taxonomy_version: envelope.taxonomy_version.clone(),
        text_streaming: envelope.text_streaming,
        tool_exchange: envelope.tool_exchange,
    }
}

fn kind_descriptor_json(descriptor: &ProviderKindDescriptorRevisionV1) -> KindDescriptorJson {
    KindDescriptorJson {
        credential_transport_contract: descriptor.credential_transport_contract.clone(),
        descriptor_family: descriptor.descriptor_family.clone(),
        driver_contract_family: descriptor.driver_contract_family.clone(),
        endpoint_policy: descriptor.endpoint_policy.clone(),
        kind_id: descriptor.kind_id.clone(),
        model_capability_envelope: capability_envelope_json(&descriptor.model_capability_envelope),
        ordered_protocol_part_revisions: descriptor.ordered_protocol_part_revisions.clone(),
    }
}

fn profile_json(profile: &ProviderProfileRevisionV1) -> ProfileJson {
    ProfileJson {
        capability_taxonomy_revision: profile.capability_taxonomy_revision.clone(),
        credential_transport_mode: transport_mode_name(profile.credential_transport_mode)
            .to_owned(),
        driver_contract_revision: DriverContractRevisionJson {
            driver_family: profile.driver_contract_revision.driver_family.clone(),
            major: profile.driver_contract_revision.major,
            minor: profile.driver_contract_revision.minor,
        },
        endpoint: profile.endpoint.clone(),
        kind_descriptor_revision_id: profile.kind_descriptor_revision_id.clone(),
        model_id: profile.model_id.clone(),
        profile_id: profile.profile_id.clone(),
        provider_kind_id: profile.provider_kind_id.clone(),
        reasoning_compatibility_id: profile.reasoning_compatibility_id.clone(),
        revision_id: profile.revision_id.clone(),
        safe_header_name: profile.safe_header_name.clone(),
    }
}

fn selection_json(selection: &ProviderSelectionV1) -> SelectionJson {
    SelectionJson {
        credential_transport_mode: transport_mode_name(selection.credential_transport_mode)
            .to_owned(),
        credential_transport_safe_header_name: selection
            .credential_transport_safe_header_name
            .clone(),
        declared_model_capability_subset: selection.declared_model_capability_subset.clone(),
        effective_execution_policy: selection.effective_execution_policy.clone(),
        effective_loopback_policy_or_not_applicable: selection
            .effective_loopback_policy_or_not_applicable
            .clone(),
        kind_descriptor_revision_id: selection.kind_descriptor_revision_id.clone(),
        kind_id: selection.kind_id.clone(),
        model_id: selection.model_id.clone(),
        normalized_effective_endpoint: selection.normalized_effective_endpoint.clone(),
        profile_id: selection.profile_id.clone(),
        provider_driver_contract_revision: selection.provider_driver_contract_revision.clone(),
        provider_profile_revision_id: selection.provider_profile_revision_id.clone(),
        resolved_reasoning_policy: selection.resolved_reasoning_policy.clone(),
        selection_canonicalization_version: selection.selection_canonicalization_version.clone(),
        selection_source: selection.selection_source.clone(),
    }
}

fn profile_tombstone_json(tombstone: &ProviderProfileTombstoneDto) -> ProfileTombstoneJson {
    ProfileTombstoneJson {
        digest: tombstone.digest.to_string(),
        profile_id: tombstone.profile_id.clone(),
        provenance: tombstone.provenance.clone(),
        removed_catalog_revision: tombstone.removed_catalog_revision,
        removed_time: tombstone.removed_time,
    }
}

fn kind_tombstone_json(tombstone: &ProviderKindTombstoneDto) -> KindTombstoneJson {
    KindTombstoneJson {
        digest: tombstone.digest.to_string(),
        kind_id: tombstone.kind_id.clone(),
        provenance: tombstone.provenance.clone(),
        removed_catalog_revision: tombstone.removed_catalog_revision,
        removed_time: tombstone.removed_time,
    }
}

fn legacy_binding_json(
    binding: &intention_domain::LegacyM4SelectionBindingDto,
) -> LegacyBindingJson {
    LegacyBindingJson {
        capability_subset: binding.capability_subset.clone(),
        default_profile_id: binding.default_profile_id.clone(),
        default_profile_revision_id: binding.default_profile_revision_id.clone(),
        driver_contract_revision: binding.driver_contract_revision.clone(),
        execution_policy: binding.execution_policy.clone(),
        kind_descriptor_revision_id: binding.kind_descriptor_revision_id.clone(),
        legacy_config_revision_id: binding.legacy_config_revision_id.clone(),
        legacy_safe_selection: binding.legacy_safe_selection.clone(),
        legacy_snapshot_schema: binding.legacy_snapshot_schema.clone(),
    }
}

fn corrupt_legacy_binding_json(config_revision_id: &str) -> CorruptLegacyBindingJson {
    CorruptLegacyBindingJson {
        config_revision_id: config_revision_id.to_owned(),
        corrupt: true,
    }
}

fn safe_projection_json(candidate: &ProviderProfileCandidateDto) -> SafeProjectionJson {
    SafeProjectionJson {
        credential_transport_mode: transport_mode_name(candidate.profile.credential_transport_mode)
            .to_owned(),
        credential_transport_safe_header_name: candidate.profile.safe_header_name.clone(),
        declared_model_capability_subset: candidate.declared_model_capability_subset.clone(),
        effective_execution_policy: candidate.effective_execution_policy.clone(),
        effective_loopback_policy_or_not_applicable: candidate
            .effective_loopback_policy_or_not_applicable
            .clone(),
        kind_descriptor_revision_id: candidate.profile.kind_descriptor_revision_id.clone(),
        kind_id: candidate.profile.provider_kind_id.clone(),
        model_id: candidate.profile.model_id.clone(),
        normalized_effective_endpoint: candidate.profile.endpoint.clone(),
        profile_id: candidate.profile.profile_id.clone(),
        profile_revision_id: candidate.profile.revision_id.clone(),
        provider_driver_contract_revision: driver_contract_name(
            &candidate.profile.driver_contract_revision,
        ),
        resolved_reasoning_policy: candidate.resolved_reasoning_policy.clone(),
    }
}

fn driver_contract_name(contract: &intention_domain::ProviderDriverContractRevisionDto) -> String {
    format!(
        "{}-{}.{}",
        contract.driver_family, contract.major, contract.minor
    )
}

fn parse_catalog_status(value: &str) -> DtoResult<ProviderCatalogStatusDto> {
    match value {
        "preparing" => Ok(ProviderCatalogStatusDto::Preparing),
        "active" => Ok(ProviderCatalogStatusDto::Active),
        "pending_removal" => Ok(ProviderCatalogStatusDto::PendingRemoval),
        "activation_recovery_required" => Ok(ProviderCatalogStatusDto::ActivationRecoveryRequired),
        _ => Err(codec_error("invalid durable catalog status")),
    }
}

fn parse_catalog_readiness(value: &str) -> DtoResult<ProviderReadinessDto> {
    match value {
        "ready" => Ok(ProviderReadinessDto::Ready),
        "disabled" => Ok(ProviderReadinessDto::Disabled),
        "unavailable" => Ok(ProviderReadinessDto::Unavailable),
        _ => Err(codec_error("invalid durable catalog readiness")),
    }
}

fn parse_queue_state(value: &str) -> DtoResult<UnavailableQueueStateDto> {
    match value {
        "queued" => Ok(UnavailableQueueStateDto::Queued),
        "terminalized" => Ok(UnavailableQueueStateDto::Terminalized),
        "promoted" => Ok(UnavailableQueueStateDto::Promoted),
        _ => Err(codec_error("invalid durable unavailable queue state")),
    }
}

fn parse_admission_state(value: &str) -> DtoResult<HeldRunAdmissionStateDto> {
    match value {
        "held" => Ok(HeldRunAdmissionStateDto::Held),
        "admitted" => Ok(HeldRunAdmissionStateDto::Admitted),
        "rejected" => Ok(HeldRunAdmissionStateDto::Rejected),
        _ => Err(codec_error("invalid durable held-run admission state")),
    }
}

fn parse_removal_status(value: &str) -> DtoResult<ProviderCatalogRemovalStatusDto> {
    match value {
        "pending" => Ok(ProviderCatalogRemovalStatusDto::Pending),
        "accepted" => Ok(ProviderCatalogRemovalStatusDto::Accepted),
        "rejected" => Ok(ProviderCatalogRemovalStatusDto::Rejected),
        "expired" => Ok(ProviderCatalogRemovalStatusDto::Expired),
        _ => Err(codec_error("invalid durable removal candidate status")),
    }
}

fn parse_legacy_validation_status(value: &str) -> DtoResult<LegacyBindingValidationStatusDto> {
    match value {
        "validated" => Ok(LegacyBindingValidationStatusDto::Validated),
        "corrupt" => Ok(LegacyBindingValidationStatusDto::Corrupt),
        _ => Err(codec_error("invalid durable legacy binding status")),
    }
}

fn load_catalog_state(connection: &sqlite::Connection) -> DtoResult<ProviderCatalogStateDto> {
    let (active, candidate, status, default_profile, handle, degraded, updated) = connection
        .query_row(
            "SELECT active_catalog_revision_id, candidate_catalog_revision_id, status, active_default_profile_id, candidate_handle, degraded_reason, updated_at FROM provider_catalog_state WHERE singleton_id=1",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )
        .map_err(storage_error)?;
    Ok(ProviderCatalogStateDto {
        active_catalog_revision_id: active
            .map(u64::try_from)
            .transpose()
            .map_err(|_| codec_error("invalid catalog revision"))?,
        candidate_catalog_revision_id: candidate
            .map(u64::try_from)
            .transpose()
            .map_err(|_| codec_error("invalid catalog revision"))?,
        status: parse_catalog_status(&status)?,
        active_default_profile_id: default_profile,
        candidate_handle: handle,
        degraded_reason: degraded,
        updated_at: updated,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "One flat audit insert carries the fixed nine-column configuration audit record."
)]
fn insert_audit(
    tx: &sqlite::Transaction<'_>,
    operation_id: &str,
    audit_kind: &str,
    catalog_revision_id: Option<i64>,
    config_revision_id: Option<String>,
    profile_id: Option<String>,
    run_id: Option<String>,
    occurred_at: i64,
    audit_json: &AuditRecordJson,
) -> DtoResult<()> {
    let encoded_audit = audit_json.to_json();
    tx.execute(
        "INSERT OR IGNORE INTO configuration_audit(operation_id, audit_kind, catalog_revision_id, config_revision_id, profile_id, run_id, occurred_at, audit_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        sqlite::params![
            operation_id,
            audit_kind,
            catalog_revision_id,
            config_revision_id,
            profile_id,
            run_id,
            occurred_at,
            encoded_audit
        ],
    )
    .map_err(storage_error)?;
    Ok(())
}

/// Inserts one provider kind descriptor revision into append-only history.
/// Idempotent for the identical record; conflicts on digest or identity reuse.
pub fn insert_kind_descriptor(
    tx: &sqlite::Transaction<'_>,
    descriptor_revision_id: &str,
    descriptor: &ProviderKindDescriptorRevisionV1,
    catalog_revision_id: u64,
    accepted_at: i64,
) -> DtoResult<()> {
    let json = kind_descriptor_json(descriptor).to_json();
    let digest = record_digest(&json);
    let existing_by_digest = tx
        .query_row(
            "SELECT kind_id, descriptor_revision_id FROM provider_kind_descriptor_revisions WHERE descriptor_digest=?1",
            [&digest],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some((kind_id, revision_id)) = existing_by_digest {
        if kind_id == descriptor.kind_id && revision_id == descriptor_revision_id {
            return Ok(());
        }
        return Err(conflict(
            "provider_kind_descriptor_digest_conflict",
            "the kind descriptor digest is already bound to a different kind identity",
        ));
    }
    let existing_by_identity = tx
        .query_row(
            "SELECT descriptor_digest FROM provider_kind_descriptor_revisions WHERE kind_id=?1 AND descriptor_revision_id=?2",
            sqlite::params![descriptor.kind_id, descriptor_revision_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(existing_digest) = existing_by_identity {
        if existing_digest == digest {
            return Ok(());
        }
        return Err(conflict(
            "provider_kind_descriptor_revision_conflict",
            "the kind descriptor revision identity is already bound to different bytes",
        ));
    }
    tx.execute(
        "INSERT INTO provider_kind_descriptor_revisions(kind_id, descriptor_revision_id, descriptor_json, descriptor_digest, catalog_revision_id, accepted_at) VALUES (?1,?2,?3,?4,?5,?6)",
        sqlite::params![
            descriptor.kind_id,
            descriptor_revision_id,
            json,
            digest,
            i64::try_from(catalog_revision_id)
                .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
            accepted_at
        ],
    )
    .map_err(storage_error)?;
    Ok(())
}

/// Inserts one provider profile revision into append-only history.
/// Idempotent for the identical record; conflicts on digest or identity reuse.
pub fn insert_profile(
    tx: &sqlite::Transaction<'_>,
    candidate: &ProviderProfileCandidateDto,
    catalog_revision_id: u64,
    accepted_at: i64,
) -> DtoResult<()> {
    let profile = &candidate.profile;
    let json = profile_json(profile).to_json();
    let digest = record_digest(&json);
    let existing_by_digest = tx
        .query_row(
            "SELECT profile_id, profile_revision_id FROM provider_profile_revisions WHERE profile_revision_digest=?1",
            [&digest],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some((profile_id, revision_id)) = existing_by_digest {
        if profile_id == profile.profile_id && revision_id == profile.revision_id {
            return Ok(());
        }
        return Err(conflict(
            "provider_profile_digest_conflict",
            "the profile revision digest is already bound to a different profile identity",
        ));
    }
    let existing_by_identity = tx
        .query_row(
            "SELECT profile_revision_digest FROM provider_profile_revisions WHERE profile_id=?1 AND profile_revision_id=?2",
            sqlite::params![profile.profile_id, profile.revision_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(existing_digest) = existing_by_identity {
        if existing_digest == digest {
            return Ok(());
        }
        return Err(conflict(
            "provider_profile_revision_conflict",
            "the profile revision identity is already bound to different bytes",
        ));
    }
    tx.execute(
        "INSERT INTO provider_profile_revisions(profile_id, profile_revision_id, kind_id, kind_descriptor_revision_id, provider_driver_contract_revision, model_id, normalized_effective_endpoint, credential_transport_mode, credential_transport_safe_header_name, declared_model_capability_subset_json, resolved_reasoning_policy, effective_execution_policy, effective_loopback_policy_or_not_applicable, profile_revision_json, profile_revision_digest, catalog_revision_id, accepted_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        sqlite::params![
            profile.profile_id,
            profile.revision_id,
            profile.provider_kind_id,
            profile.kind_descriptor_revision_id,
            driver_contract_name(&profile.driver_contract_revision),
            profile.model_id,
            profile.endpoint,
            transport_mode_name(profile.credential_transport_mode),
            profile.safe_header_name,
            serde_json::to_string(&candidate.declared_model_capability_subset).map_err(codec_error)?,
            candidate.resolved_reasoning_policy,
            candidate.effective_execution_policy,
            candidate.effective_loopback_policy_or_not_applicable,
            json,
            digest,
            i64::try_from(catalog_revision_id)
                .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
            accepted_at
        ],
    )
    .map_err(storage_error)?;
    Ok(())
}

/// Persists the resolved provider selection of one fresh run in the supplied
/// transaction. Idempotent for the identical record; conflicts on digest reuse.
pub fn insert_selection(
    tx: &sqlite::Transaction<'_>,
    session_id: SessionId,
    run_id: RunId,
    selection: &ProviderSelectionV1,
) -> DtoResult<()> {
    let json = selection_json(selection).to_json();
    let digest = record_digest(&json);
    let existing_by_digest = tx
        .query_row(
            "SELECT run_id FROM resolved_run_provider_selections WHERE selection_digest=?1",
            [&digest],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(existing_run) = existing_by_digest {
        if existing_run == run_id.to_string() {
            return Ok(());
        }
        return Err(conflict(
            "provider_selection_digest_conflict",
            "the provider selection digest is already bound to a different run",
        ));
    }
    tx.execute(
        "INSERT INTO resolved_run_provider_selections(run_id, session_id, selection_canonicalization_version, profile_id, provider_profile_revision_id, kind_id, kind_descriptor_revision_id, model_id, normalized_effective_endpoint, credential_transport_mode, credential_transport_safe_header_name, declared_model_capability_subset_json, resolved_reasoning_policy, effective_execution_policy, effective_loopback_policy_or_not_applicable, provider_driver_contract_revision, selection_source, selection_json, selection_digest) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        sqlite::params![
            run_id.to_string(),
            session_id.to_string(),
            selection.selection_canonicalization_version,
            selection.profile_id,
            selection.provider_profile_revision_id,
            selection.kind_id,
            selection.kind_descriptor_revision_id,
            selection.model_id,
            selection.normalized_effective_endpoint,
            transport_mode_name(selection.credential_transport_mode),
            selection.credential_transport_safe_header_name,
            serde_json::to_string(&selection.declared_model_capability_subset).map_err(codec_error)?,
            selection.resolved_reasoning_policy,
            selection.effective_execution_policy,
            selection.effective_loopback_policy_or_not_applicable,
            selection.provider_driver_contract_revision,
            selection.selection_source,
            json,
            digest
        ],
    )
    .map_err(storage_error)?;
    Ok(())
}

fn load_queue_entry(row: &sqlite::Row<'_>) -> Result<UnavailableRunQueueEntryDto, sqlite::Error> {
    fn to_sql_error(error: ErrorDto) -> sqlite::Error {
        sqlite::Error::ToSqlConversionFailure(Box::new(error))
    }
    Ok(UnavailableRunQueueEntryDto {
        queue_id: row.get(0)?,
        run_id: RunId::parse(&row.get::<_, String>(1)?).map_err(to_sql_error)?,
        session_id: SessionId::parse(&row.get::<_, String>(2)?).map_err(to_sql_error)?,
        profile_id: row.get(3)?,
        provider_profile_revision_id: row.get(4)?,
        unavailable_reason: row.get(5)?,
        first_unavailable_at: row.get(6)?,
        promotion_attempts: u64::try_from(row.get::<_, i64>(7)?).unwrap_or(u64::MAX),
        state: parse_queue_state(&row.get::<_, String>(8)?).map_err(to_sql_error)?,
        last_operation_id: row.get(9)?,
        selection_json: row.get(10)?,
    })
}

impl ProviderCatalogRepositoryDto for SqliteStorageRepository {
    fn append_provider_kind_descriptor_revision(
        &self,
        input: AppendProviderKindDescriptorRevisionInputDto,
    ) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            insert_kind_descriptor(
                &tx,
                &input.descriptor_revision_id,
                &input.descriptor,
                input.catalog_revision_id,
                input.accepted_at,
            )?;
            // Appending is the preparation step: mark the candidate revision
            // and record one Prepared audit per operation.
            tx.execute(
                "UPDATE provider_catalog_state SET candidate_catalog_revision_id=?1, updated_at=?2 WHERE singleton_id=1",
                sqlite::params![
                    i64::try_from(input.catalog_revision_id)
                        .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
                    input.accepted_at
                ],
            )
            .map_err(storage_error)?;
            insert_audit(
                &tx,
                &input.operation_id,
                "ProviderCatalogCandidatePrepared",
                Some(
                    i64::try_from(input.catalog_revision_id)
                        .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
                ),
                None,
                None,
                None,
                input.accepted_at,
                &AuditRecordJson::kind_descriptor_prepared(&input.descriptor.kind_id),
            )?;
            tx.commit().map_err(storage_error)
        })
    }

    fn append_provider_profile_revision(
        &self,
        input: AppendProviderProfileRevisionInputDto,
    ) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            insert_profile(
                &tx,
                &input.profile,
                input.catalog_revision_id,
                input.accepted_at,
            )?;
            tx.execute(
                "UPDATE provider_catalog_state SET candidate_catalog_revision_id=?1, updated_at=?2 WHERE singleton_id=1",
                sqlite::params![
                    i64::try_from(input.catalog_revision_id)
                        .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
                    input.accepted_at
                ],
            )
            .map_err(storage_error)?;
            insert_audit(
                &tx,
                &input.operation_id,
                "ProviderCatalogCandidatePrepared",
                Some(
                    i64::try_from(input.catalog_revision_id)
                        .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
                ),
                None,
                None,
                None,
                input.accepted_at,
                &AuditRecordJson::profile_prepared(&input.profile.profile.profile_id),
            )?;
            tx.commit().map_err(storage_error)
        })
    }

    fn load_provider_catalog_status(&self) -> DtoResult<ProviderCatalogStateDto> {
        let connection = self.connection()?;
        let state = load_catalog_state(&connection)?;
        drop(connection);
        Ok(state)
    }

    fn load_provider_catalog_page(
        &self,
        input: LoadProviderCatalogPageInputDto,
    ) -> DtoResult<ProviderCatalogPageDto> {
        if input.limit == 0 || input.limit > MAX_CATALOG_PAGE_SIZE {
            return Err(ErrorDto::validation(
                "invalid_catalog_page_limit",
                "provider catalog page limit must be between 1 and 1024",
            ));
        }
        let connection = self.connection()?;
        let state = load_catalog_state(&connection)?;
        let active_revision = state.active_catalog_revision_id.ok_or_else(|| {
            not_found(
                "provider_catalog_not_active",
                "no active provider catalog is committed",
            )
        })?;
        let after: Option<String> = if let Some(token) = &input.token {
            let parsed = CatalogTokenJson::from_json(token).map_err(|_| {
                ErrorDto::validation(
                    "invalid_catalog_page_token",
                    "the provider catalog page token is not valid",
                )
            })?;
            if parsed.revision != active_revision {
                return Err(ErrorDto::validation(
                    "catalog_page_token_stale",
                    "the provider catalog changed since the page token was issued",
                ));
            }
            parsed.after
        } else {
            None
        };
        let limit = i64::try_from(input.limit).map_err(|_| {
            ErrorDto::validation(
                "invalid_catalog_page_limit",
                "provider catalog page limit is outside the supported range",
            )
        })?;
        let mut statement = connection
            .prepare(
                "SELECT profile_id, profile_revision_id, kind_id, kind_descriptor_revision_id, display_name, enabled, credential_configured, readiness, safe_projection_json FROM provider_catalog_profile_projection WHERE projection_state='active' AND (?1 IS NULL OR profile_id > ?1) ORDER BY profile_id LIMIT ?2",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map(sqlite::params![after, limit + 1], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                ))
            })
            .map_err(storage_error)?;
        let mut entries = Vec::new();
        for row in rows {
            let (
                profile_id,
                revision_id,
                kind_id,
                kind_revision,
                display,
                enabled,
                configured,
                readiness,
                safe_json,
            ) = row.map_err(storage_error)?;
            entries.push(ProviderCatalogProfileEntryDto {
                profile_id,
                profile_revision_id: revision_id,
                kind_id,
                kind_descriptor_revision_id: kind_revision,
                display_name: display,
                enabled: enabled != 0,
                credential_configured: configured != 0,
                readiness: parse_catalog_readiness(&readiness)?,
                safe_projection_json: safe_json,
            });
        }
        drop(statement);
        drop(connection);
        let has_more = entries.len() as i64 > limit;
        if has_more {
            entries.truncate(limit as usize);
        }
        let next_token = if has_more {
            let last = entries
                .last()
                .ok_or_else(|| codec_error("provider catalog page lost its last entry"))?;
            Some(
                CatalogTokenJson {
                    after: Some(last.profile_id.clone()),
                    revision: active_revision,
                }
                .to_json(),
            )
        } else {
            None
        };
        Ok(ProviderCatalogPageDto {
            entries,
            next_token,
            has_more,
        })
    }

    fn accept_provider_catalog(&self, input: AcceptProviderCatalogInputDto) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            let state = load_catalog_state(&tx)?;
            if state.candidate_catalog_revision_id != Some(input.catalog_revision_id) {
                return Err(conflict(
                    "provider_catalog_revision_conflict",
                    "the accepted catalog revision does not match the prepared candidate",
                ));
            }
            if state
                .candidate_handle
                .as_ref()
                .is_some_and(|handle| handle != &input.candidate_handle)
            {
                return Err(conflict(
                    "provider_catalog_candidate_conflict",
                    "the accepted candidate handle does not match the prepared candidate",
                ));
            }
            self.fault(FaultPoint::CatalogRevision)?;
            for kind in &input.kind_descriptors {
                insert_kind_descriptor(
                    &tx,
                    &kind.descriptor_revision_id,
                    &kind.descriptor,
                    input.catalog_revision_id,
                    input.accepted_at,
                )?;
            }
            for profile in &input.profiles {
                insert_profile(&tx, profile, input.catalog_revision_id, input.accepted_at)?;
            }
            let active_entries = {
                let mut statement = tx
                    .prepare(
                        "SELECT profile_id, kind_id FROM provider_catalog_profile_projection WHERE projection_state='active'",
                    )
                    .map_err(storage_error)?;
                let rows = statement
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(storage_error)?;
                rows.map(|row| row.map_err(storage_error))
                    .collect::<DtoResult<Vec<_>>>()?
            };
            let new_profile_ids = input
                .profiles
                .iter()
                .map(|profile| profile.profile.profile_id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let new_kind_ids = input
                .kind_descriptors
                .iter()
                .map(|kind| kind.descriptor.kind_id.as_str())
                .collect::<std::collections::HashSet<_>>();
            for (profile_id, kind_id) in active_entries {
                if !new_profile_ids.contains(profile_id.as_str()) {
                    let tombstone = ProviderProfileTombstoneDto::new(
                        profile_id,
                        input.catalog_revision_id,
                        u64::try_from(input.accepted_at)
                            .map_err(|_| codec_error("invalid catalog timestamp"))?,
                        "catalog-acceptance",
                    )
                    .map_err(codec_error)?;
                    tx.execute(
                        "INSERT OR IGNORE INTO provider_profile_tombstones(profile_id, removed_catalog_revision_id, removed_at, provenance, tombstone_json, tombstone_digest) VALUES (?1,?2,?3,?4,?5,?6)",
                        sqlite::params![
                            tombstone.profile_id,
                            i64::try_from(tombstone.removed_catalog_revision)
                                .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
                            i64::try_from(tombstone.removed_time)
                                .map_err(|_| codec_error("catalog timestamp outside the SQLite range"))?,
                            tombstone.provenance,
                            profile_tombstone_json(&tombstone).to_json(),
                            tombstone.digest.to_string()
                        ],
                    )
                    .map_err(storage_error)?;
                }
                if !new_kind_ids.contains(kind_id.as_str()) {
                    let tombstone = ProviderKindTombstoneDto::new(
                        kind_id,
                        input.catalog_revision_id,
                        u64::try_from(input.accepted_at)
                            .map_err(|_| codec_error("invalid catalog timestamp"))?,
                        "catalog-acceptance",
                    )
                    .map_err(codec_error)?;
                    tx.execute(
                        "INSERT OR IGNORE INTO provider_kind_tombstones(kind_id, removed_catalog_revision_id, removed_at, provenance, tombstone_json, tombstone_digest) VALUES (?1,?2,?3,?4,?5,?6)",
                        sqlite::params![
                            tombstone.kind_id,
                            i64::try_from(tombstone.removed_catalog_revision)
                                .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
                            i64::try_from(tombstone.removed_time)
                                .map_err(|_| codec_error("catalog timestamp outside the SQLite range"))?,
                            tombstone.provenance,
                            kind_tombstone_json(&tombstone).to_json(),
                            tombstone.digest.to_string()
                        ],
                    )
                    .map_err(storage_error)?;
                }
            }
            self.fault(FaultPoint::CatalogTombstone)?;
            tx.execute(
                "DELETE FROM provider_catalog_profile_projection WHERE projection_state='active'",
                [],
            )
            .map_err(storage_error)?;
            for profile in &input.profiles {
                tx.execute(
                    "INSERT INTO provider_catalog_profile_projection(projection_state, catalog_revision_id, profile_id, profile_revision_id, kind_id, kind_descriptor_revision_id, display_name, enabled, credential_configured, readiness, safe_projection_json) VALUES ('active', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    sqlite::params![
                        i64::try_from(input.catalog_revision_id)
                            .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
                        profile.profile.profile_id,
                        profile.profile.revision_id,
                        profile.profile.provider_kind_id,
                        profile.profile.kind_descriptor_revision_id,
                        profile.display_name,
                        i64::from(profile.enabled),
                        i64::from(profile.credential_configured),
                        readiness_name(profile.readiness),
                        safe_projection_json(profile).to_json()
                    ],
                )
                .map_err(storage_error)?;
            }
            self.fault(FaultPoint::CatalogProjection)?;
            tx.execute(
                "UPDATE provider_catalog_state SET active_catalog_revision_id=?1, candidate_catalog_revision_id=NULL, status='active', active_default_profile_id=?2, candidate_handle=NULL, degraded_reason=NULL, updated_at=?3 WHERE singleton_id=1",
                sqlite::params![
                    i64::try_from(input.catalog_revision_id)
                        .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
                    input.default_profile_id,
                    input.accepted_at
                ],
            )
            .map_err(storage_error)?;
            let revision = i64::try_from(input.catalog_revision_id)
                .map_err(|_| codec_error("catalog revision outside the SQLite range"))?;
            let audit =
                AuditRecordJson::catalog_accepted(input.candidate_handle, input.default_profile_id);
            insert_audit(
                &tx,
                &input.operation_id,
                "ProviderCatalogAccepted",
                Some(revision),
                None,
                None,
                None,
                input.accepted_at,
                &audit,
            )?;
            insert_audit(
                &tx,
                &input.operation_id,
                "ProviderCatalogActivated",
                Some(revision),
                None,
                None,
                None,
                input.accepted_at,
                &audit,
            )?;
            self.fault(FaultPoint::CatalogAudit)?;
            tx.commit().map_err(storage_error)
        })
    }

    fn reject_provider_catalog_candidate(
        &self,
        input: RejectProviderCatalogCandidateInputDto,
    ) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            let state = load_catalog_state(&tx)?;
            if state.candidate_catalog_revision_id != Some(input.catalog_revision_id)
                || state
                    .candidate_handle
                    .as_ref()
                    .is_some_and(|handle| handle != &input.candidate_handle)
            {
                return Err(conflict(
                    "provider_catalog_candidate_conflict",
                    "the rejected candidate does not match the prepared candidate",
                ));
            }
            tx.execute(
                "UPDATE provider_catalog_state SET candidate_catalog_revision_id=NULL, candidate_handle=NULL, status=CASE WHEN active_catalog_revision_id IS NULL THEN 'preparing' ELSE 'active' END, updated_at=?1 WHERE singleton_id=1",
                [input.rejected_at],
            )
            .map_err(storage_error)?;
            let revision = i64::try_from(input.catalog_revision_id)
                .map_err(|_| codec_error("catalog revision outside the SQLite range"))?;
            insert_audit(
                &tx,
                &input.operation_id,
                "ProviderCatalogCandidateRejected",
                Some(revision),
                None,
                None,
                None,
                input.rejected_at,
                &AuditRecordJson::candidate_handle(Some(input.candidate_handle)),
            )?;
            tx.commit().map_err(storage_error)
        })
    }

    fn expire_provider_catalog_candidate(
        &self,
        input: ExpireProviderCatalogCandidateInputDto,
    ) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            let state = load_catalog_state(&tx)?;
            if state.candidate_catalog_revision_id != Some(input.catalog_revision_id) {
                return Err(conflict(
                    "provider_catalog_candidate_conflict",
                    "the expired candidate does not match the prepared candidate",
                ));
            }
            tx.execute(
                "UPDATE provider_catalog_state SET candidate_catalog_revision_id=NULL, candidate_handle=NULL, status=CASE WHEN active_catalog_revision_id IS NULL THEN 'preparing' ELSE 'active' END, updated_at=?1 WHERE singleton_id=1",
                [input.expired_at],
            )
            .map_err(storage_error)?;
            let revision = i64::try_from(input.catalog_revision_id)
                .map_err(|_| codec_error("catalog revision outside the SQLite range"))?;
            insert_audit(
                &tx,
                &input.operation_id,
                "ProviderCatalogCandidateExpired",
                Some(revision),
                None,
                None,
                None,
                input.expired_at,
                &AuditRecordJson::candidate_handle(state.candidate_handle),
            )?;
            tx.commit().map_err(storage_error)
        })
    }

    fn load_provider_catalog_material(&self) -> DtoResult<ProviderCatalogMaterialDto> {
        let connection = self.connection()?;
        let state = load_catalog_state(&connection)?;
        let active_revision = state.active_catalog_revision_id.ok_or_else(|| {
            not_found(
                "provider_catalog_not_active",
                "no active provider catalog is committed",
            )
        })?;
        // Resolve the active material through the projection: the append-only
        // history may carry an active record at an earlier revision when an
        // identical record was re-appended idempotently.
        let mut entries = Vec::new();
        {
            let mut statement = connection
                .prepare(
                    "SELECT profile_id, profile_revision_id, kind_id, kind_descriptor_revision_id FROM provider_catalog_profile_projection WHERE projection_state='active' ORDER BY profile_id",
                )
                .map_err(storage_error)?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .map_err(storage_error)?;
            for row in rows {
                entries.push(row.map_err(storage_error)?);
            }
            drop(statement);
        }
        let mut kind_descriptors = Vec::new();
        let mut profiles = Vec::new();
        for (profile_id, profile_revision_id, _kind_id, kind_descriptor_revision_id) in &entries {
            let profile_row = connection
                .query_row(
                    "SELECT profile_revision_json, declared_model_capability_subset_json, resolved_reasoning_policy, effective_execution_policy, effective_loopback_policy_or_not_applicable FROM provider_profile_revisions WHERE profile_id=?1 AND profile_revision_id=?2",
                    sqlite::params![profile_id, profile_revision_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    },
                )
                .optional()
                .map_err(storage_error)?;
            let Some((encoded, subset, reasoning, execution, loopback)) = profile_row else {
                return Err(codec_error(
                    "active projection references an unknown profile revision",
                ));
            };
            let profile = parse_profile_json(&encoded)?;
            let projection = connection
                .query_row(
                    "SELECT display_name, enabled, credential_configured, readiness FROM provider_catalog_profile_projection WHERE projection_state='active' AND profile_id=?1",
                    [profile_id],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .map_err(storage_error)?;
            let (display_name, enabled, configured, readiness) = projection;
            if !kind_descriptors
                .iter()
                .any(|existing: &ProviderKindDescriptorCandidateDto| {
                    existing.descriptor_revision_id == *kind_descriptor_revision_id
                })
            {
                let kind_row = connection
                    .query_row(
                        "SELECT kind_id, descriptor_json FROM provider_kind_descriptor_revisions WHERE descriptor_revision_id=?1",
                        [kind_descriptor_revision_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()
                    .map_err(storage_error)?;
                let Some((kind_id, encoded_kind)) = kind_row else {
                    return Err(codec_error(
                        "active projection references an unknown kind descriptor revision",
                    ));
                };
                let descriptor = parse_kind_descriptor_json(&encoded_kind)?;
                if descriptor.kind_id != kind_id {
                    return Err(codec_error("kind descriptor identity mismatch"));
                }
                kind_descriptors.push(ProviderKindDescriptorCandidateDto {
                    descriptor_revision_id: kind_descriptor_revision_id.clone(),
                    descriptor,
                });
            }
            profiles.push(ProviderProfileCandidateDto {
                profile,
                declared_model_capability_subset: serde_json::from_str(&subset)
                    .map_err(codec_error)?,
                resolved_reasoning_policy: reasoning,
                effective_execution_policy: execution,
                effective_loopback_policy_or_not_applicable: loopback,
                display_name,
                enabled: enabled != 0,
                credential_configured: configured != 0,
                readiness: parse_catalog_readiness(&readiness)?,
            });
        }
        drop(connection);
        Ok(ProviderCatalogMaterialDto {
            catalog_revision_id: active_revision,
            default_profile_id: state.active_default_profile_id,
            kind_descriptors,
            profiles,
        })
    }
}

/// Parses one persisted kind descriptor record back into its domain record.
fn parse_kind_descriptor_json(encoded: &str) -> DtoResult<ProviderKindDescriptorRevisionV1> {
    let record = KindDescriptorJson::from_json(encoded)?;
    let descriptor = ProviderKindDescriptorRevisionV1 {
        kind_id: record.kind_id,
        descriptor_family: record.descriptor_family,
        ordered_protocol_part_revisions: record.ordered_protocol_part_revisions,
        endpoint_policy: record.endpoint_policy,
        credential_transport_contract: record.credential_transport_contract,
        model_capability_envelope: parse_capability_envelope(record.model_capability_envelope)?,
        driver_contract_family: record.driver_contract_family,
    };
    descriptor.validate().map_err(codec_error)?;
    Ok(descriptor)
}

/// Parses one persisted profile revision record back into its domain record.
fn parse_profile_json(encoded: &str) -> DtoResult<ProviderProfileRevisionV1> {
    let record = ProfileJson::from_json(encoded)?;
    let transport = match record.credential_transport_mode.as_str() {
        "bearer" => CredentialTransportMode::Bearer,
        "safe_header" => CredentialTransportMode::SafeHeader,
        _ => return Err(codec_error("invalid persisted credential transport mode")),
    };
    let profile = ProviderProfileRevisionV1 {
        profile_id: record.profile_id,
        revision_id: record.revision_id,
        provider_kind_id: record.provider_kind_id,
        model_id: record.model_id,
        endpoint: record.endpoint,
        credential_transport_mode: transport,
        safe_header_name: record.safe_header_name,
        capability_taxonomy_revision: record.capability_taxonomy_revision,
        reasoning_compatibility_id: record.reasoning_compatibility_id,
        kind_descriptor_revision_id: record.kind_descriptor_revision_id,
        driver_contract_revision: ProviderDriverContractRevisionDto {
            driver_family: record.driver_contract_revision.driver_family,
            major: record.driver_contract_revision.major,
            minor: record.driver_contract_revision.minor,
        },
    };
    profile.validate().map_err(codec_error)?;
    Ok(profile)
}

/// Parses one persisted capability envelope back into its domain record.
fn parse_capability_envelope(record: CapabilityEnvelopeJson) -> DtoResult<ModelCapabilitySetV1> {
    let reasoning = match record.reasoning.as_str() {
        "disabled" => ReasoningCapability::Disabled,
        "textual_reasoning_v1" => ReasoningCapability::TextualReasoningV1,
        _ => return Err(codec_error("invalid persisted reasoning capability")),
    };
    let envelope = ModelCapabilitySetV1 {
        taxonomy_version: record.taxonomy_version,
        input: intention_domain::ModelInputCapability::TextOnly,
        text_streaming: record.text_streaming,
        structured_output: intention_domain::StructuredOutputCapability::Unsupported,
        reasoning,
        tool_exchange: record.tool_exchange,
        context_preservation:
            intention_domain::ContextPreservationCapability::LocalDurableHistoryV1 {
                reasoning_input_contract: record
                    .context_preservation
                    .local_durable_history_v1
                    .reasoning_input_contract,
            },
    };
    envelope.validate().map_err(codec_error)?;
    Ok(envelope)
}

/// Parses one persisted resolved provider selection record back into its domain record.
fn parse_selection_json(encoded: &str) -> DtoResult<ProviderSelectionV1> {
    let record = SelectionJson::from_json(encoded)?;
    let transport = match record.credential_transport_mode.as_str() {
        "bearer" => CredentialTransportMode::Bearer,
        "safe_header" => CredentialTransportMode::SafeHeader,
        _ => return Err(codec_error("invalid persisted credential transport mode")),
    };
    let selection = ProviderSelectionV1 {
        selection_canonicalization_version: record.selection_canonicalization_version,
        profile_id: record.profile_id,
        provider_profile_revision_id: record.provider_profile_revision_id,
        kind_id: record.kind_id,
        kind_descriptor_revision_id: record.kind_descriptor_revision_id,
        model_id: record.model_id,
        normalized_effective_endpoint: record.normalized_effective_endpoint,
        credential_transport_mode: transport,
        credential_transport_safe_header_name: record.credential_transport_safe_header_name,
        declared_model_capability_subset: record.declared_model_capability_subset,
        resolved_reasoning_policy: record.resolved_reasoning_policy,
        effective_execution_policy: record.effective_execution_policy,
        effective_loopback_policy_or_not_applicable: record
            .effective_loopback_policy_or_not_applicable,
        provider_driver_contract_revision: record.provider_driver_contract_revision,
        selection_source: record.selection_source,
    };
    selection.validate().map_err(codec_error)?;
    Ok(selection)
}

const fn readiness_name(readiness: ProviderReadinessDto) -> &'static str {
    match readiness {
        ProviderReadinessDto::Ready => "ready",
        ProviderReadinessDto::Disabled => "disabled",
        ProviderReadinessDto::Unavailable => "unavailable",
    }
}

impl ConfigurationReloadRepositoryDto for SqliteStorageRepository {
    fn commit_configuration_reload(
        &self,
        input: CommitConfigurationReloadInputDto,
    ) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            Self::store_config(&tx, &input.snapshot)?;
            self.fault(FaultPoint::ReloadSnapshot)?;
            insert_audit(
                &tx,
                &input.operation_id,
                "ProviderCatalogAccepted",
                None,
                Some(input.snapshot.revision_id().to_string()),
                None,
                None,
                input.reloaded_at,
                &AuditRecordJson::reloaded(),
            )?;
            self.fault(FaultPoint::ReloadAudit)?;
            tx.execute(
                "UPDATE provider_catalog_state SET status=CASE WHEN active_catalog_revision_id IS NULL THEN status ELSE 'active' END, updated_at=?1 WHERE singleton_id=1",
                [input.reloaded_at],
            )
            .map_err(storage_error)?;
            tx.commit().map_err(storage_error)
        })
    }
}

impl SessionProviderDefaultsRepositoryDto for SqliteStorageRepository {
    fn get_session_provider_profile(
        &self,
        session_id: SessionId,
    ) -> DtoResult<Option<SessionProviderDefaultDto>> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT profile_id, projection_revision, last_operation_id, updated_at FROM session_provider_defaults WHERE session_id=?1",
                [session_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?;
        drop(connection);
        let Some((profile_id, revision, operation_id, updated_at)) = row else {
            return Ok(None);
        };
        Ok(Some(SessionProviderDefaultDto {
            session_id,
            profile_id,
            projection_revision: u64::try_from(revision)
                .map_err(|_| codec_error("invalid session default revision"))?,
            last_operation_id: operation_id,
            updated_at,
        }))
    }

    fn set_session_provider_profile(
        &self,
        input: SetSessionProviderProfileInputDto,
    ) -> DtoResult<SetSessionProviderProfileOutcomeDto> {
        immediate_transaction!(self, |tx| {
            let existing = tx
                .query_row(
                    "SELECT profile_id, projection_revision, last_operation_id FROM session_provider_defaults WHERE session_id=?1",
                    [input.session_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(storage_error)?;
            let Some((profile_id, revision, last_operation_id)) = existing else {
                tx.execute(
                    "INSERT INTO session_provider_defaults(session_id, profile_id, projection_revision, last_operation_id, updated_at) VALUES (?1,?2,0,?3,?4)",
                    sqlite::params![
                        input.session_id.to_string(),
                        input.profile_id,
                        input.operation_id,
                        input.updated_at
                    ],
                )
                .map_err(storage_error)?;
                tx.commit().map_err(storage_error)?;
                return Ok(SetSessionProviderProfileOutcomeDto {
                    changed: true,
                    projection_revision: 0,
                });
            };
            if last_operation_id == input.operation_id {
                if profile_id == input.profile_id {
                    tx.commit().map_err(storage_error)?;
                    return Ok(SetSessionProviderProfileOutcomeDto {
                        changed: false,
                        projection_revision: u64::try_from(revision)
                            .map_err(|_| codec_error("invalid session default revision"))?,
                    });
                }
                return Err(conflict(
                    "session_provider_default_conflict",
                    "the operation already bound a different provider profile",
                ));
            }
            let revision_value = u64::try_from(revision)
                .map_err(|_| codec_error("invalid session default revision"))?;
            if revision_value != input.expected_projection_revision {
                return Err(conflict(
                    "session_provider_default_stale",
                    "the session provider default changed concurrently",
                ));
            }
            if profile_id == input.profile_id {
                tx.execute(
                    "UPDATE session_provider_defaults SET last_operation_id=?2, updated_at=?3 WHERE session_id=?1",
                    sqlite::params![input.session_id.to_string(), input.operation_id, input.updated_at],
                )
                .map_err(storage_error)?;
                tx.commit().map_err(storage_error)?;
                return Ok(SetSessionProviderProfileOutcomeDto {
                    changed: false,
                    projection_revision: revision_value,
                });
            }
            tx.execute(
                "UPDATE session_provider_defaults SET profile_id=?2, projection_revision=projection_revision+1, last_operation_id=?3, updated_at=?4 WHERE session_id=?1",
                sqlite::params![
                    input.session_id.to_string(),
                    input.profile_id,
                    input.operation_id,
                    input.updated_at
                ],
            )
            .map_err(storage_error)?;
            tx.commit().map_err(storage_error)?;
            Ok(SetSessionProviderProfileOutcomeDto {
                changed: true,
                projection_revision: revision_value + 1,
            })
        })
    }
}

impl ProviderSelectionRepositoryDto for SqliteStorageRepository {
    fn persist_resolved_run_provider_selection(
        &self,
        input: PersistResolvedRunProviderSelectionInputDto,
    ) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            insert_selection(&tx, input.session_id, input.run_id, &input.selection)?;
            self.fault(FaultPoint::ProviderSelection)?;
            tx.commit().map_err(storage_error)
        })
    }

    fn load_resolved_run_provider_selection(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<Option<ProviderSelectionV1>> {
        let connection = self.connection()?;
        let encoded = connection
            .query_row(
                "SELECT selection_json FROM resolved_run_provider_selections WHERE run_id=?1 AND session_id=?2",
                sqlite::params![run_id.to_string(), session_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage_error)?;
        drop(connection);
        let Some(encoded) = encoded else {
            return Ok(None);
        };
        let selection = parse_selection_json(&encoded)?;
        Ok(Some(selection))
    }
}

impl UnavailableQueueRepositoryDto for SqliteStorageRepository {
    fn enqueue_unavailable_run(&self, input: EnqueueUnavailableRunInputDto) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            tx.execute(
                "INSERT OR IGNORE INTO unavailable_provider_queue(run_id, session_id, profile_id, provider_profile_revision_id, unavailable_reason, first_unavailable_at, promotion_attempts, state, last_operation_id, selection_json) VALUES (?1,?2,?3,?4,?5,?6,0,'queued',?7,?8)",
                sqlite::params![
                    input.run_id.to_string(),
                    input.session_id.to_string(),
                    input.profile_id,
                    input.provider_profile_revision_id,
                    input.unavailable_reason,
                    input.first_unavailable_at,
                    input.operation_id,
                    input.selection_json
                ],
            )
            .map_err(storage_error)?;
            tx.commit().map_err(storage_error)
        })
    }

    fn load_unavailable_queue_page(
        &self,
        input: LoadUnavailableQueuePageInputDto,
    ) -> DtoResult<Vec<UnavailableRunQueueEntryDto>> {
        if input.limit == 0 || input.limit > MAX_QUEUE_PAGE_SIZE {
            return Err(ErrorDto::validation(
                "invalid_unavailable_queue_page_limit",
                "unavailable queue page limit must be between 1 and 1024",
            ));
        }
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT queue_id, run_id, session_id, profile_id, provider_profile_revision_id, unavailable_reason, first_unavailable_at, promotion_attempts, state, last_operation_id, selection_json FROM unavailable_provider_queue WHERE (?1 IS NULL OR queue_id > ?1) ORDER BY queue_id LIMIT ?2",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map(
                sqlite::params![
                    input.after_queue_id,
                    i64::try_from(input.limit).map_err(|_| ErrorDto::validation(
                        "invalid_unavailable_queue_page_limit",
                        "unavailable queue page limit is outside the supported range"
                    ))?
                ],
                load_queue_entry,
            )
            .map_err(storage_error)?;
        let entries = rows
            .map(|row| row.map_err(storage_error))
            .collect::<DtoResult<Vec<_>>>()?;
        drop(statement);
        drop(connection);
        Ok(entries)
    }

    fn promote_unavailable_runs(
        &self,
        input: PromoteUnavailableRunsInputDto,
    ) -> DtoResult<PromoteUnavailableRunsOutcomeDto> {
        if input.max == 0 || input.max > MAX_PROMOTION_BATCH {
            return Err(ErrorDto::validation(
                "invalid_unavailable_queue_promotion",
                "unavailable queue promotion batch must be between 1 and 8",
            ));
        }
        immediate_transaction!(self, |tx| {
            let limit = i64::try_from(input.max).map_err(|_| {
                ErrorDto::validation(
                    "invalid_unavailable_queue_promotion",
                    "unavailable queue promotion batch is outside the supported range",
                )
            })?;
            let mut statement = tx
                .prepare(
                    "SELECT queue_id, run_id, session_id, profile_id, provider_profile_revision_id, unavailable_reason, first_unavailable_at, promotion_attempts, state, last_operation_id, selection_json FROM unavailable_provider_queue WHERE state='queued' ORDER BY queue_id LIMIT ?1",
                )
                .map_err(storage_error)?;
            let rows = statement
                .query_map([limit], load_queue_entry)
                .map_err(storage_error)?;
            let mut promoted = Vec::new();
            for row in rows {
                let mut entry = row.map_err(storage_error)?;
                tx.execute(
                    "UPDATE unavailable_provider_queue SET state='promoted', promotion_attempts=promotion_attempts+1, last_operation_id=?2 WHERE queue_id=?1",
                    sqlite::params![entry.queue_id, input.operation_id],
                )
                .map_err(storage_error)?;
                entry.state = UnavailableQueueStateDto::Promoted;
                entry.promotion_attempts = entry.promotion_attempts.saturating_add(1);
                entry.last_operation_id = Some(input.operation_id.clone());
                promoted.push(entry);
            }
            let remaining: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM unavailable_provider_queue WHERE state='queued'",
                    [],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            let mut reconciliation_marker_created = false;
            if remaining == 0 {
                let mut sessions = promoted
                    .iter()
                    .map(|entry| entry.session_id)
                    .collect::<Vec<_>>();
                sessions.sort_unstable();
                sessions.dedup();
                for session_id in sessions {
                    let unresolved = tx
                        .query_row(
                            "SELECT marker_id FROM unavailable_queue_reconciliation_markers WHERE session_id=?1 AND reason='promotion_exhausted' AND resolved_at IS NULL",
                            [session_id.to_string()],
                            |row| row.get::<_, i64>(0),
                        )
                        .optional()
                        .map_err(storage_error)?;
                    if unresolved.is_none() {
                        tx.execute(
                            "INSERT INTO unavailable_queue_reconciliation_markers(session_id, created_at, reason, next_page_cursor, resolved_at) VALUES (?1,?2,'promotion_exhausted',NULL,NULL)",
                            sqlite::params![session_id.to_string(), input.now],
                        )
                        .map_err(storage_error)?;
                        reconciliation_marker_created = true;
                    }
                }
            }
            drop(statement);
            self.fault(FaultPoint::UnavailableQueue)?;
            tx.commit().map_err(storage_error)?;
            Ok(PromoteUnavailableRunsOutcomeDto {
                promoted,
                reconciliation_marker_created,
            })
        })
    }

    fn reconcile_unavailable_queue(
        &self,
        input: ReconcileUnavailableQueueInputDto,
    ) -> DtoResult<ReconcileUnavailableQueueOutcomeDto> {
        if input.max == 0 || input.max > MAX_RECONCILIATION_BATCH {
            return Err(ErrorDto::validation(
                "invalid_unavailable_queue_reconciliation",
                "unavailable queue reconciliation batch must be between 1 and 32",
            ));
        }
        immediate_transaction!(self, |tx| {
            let limit = i64::try_from(input.max).map_err(|_| {
                ErrorDto::validation(
                    "invalid_unavailable_queue_reconciliation",
                    "unavailable queue reconciliation batch is outside the supported range",
                )
            })?;
            let mut statement = tx
                .prepare(
                    "SELECT queue_id, run_id, session_id, profile_id, provider_profile_revision_id, unavailable_reason, first_unavailable_at, promotion_attempts, state, last_operation_id, selection_json FROM unavailable_provider_queue WHERE state='queued' ORDER BY queue_id LIMIT ?1",
                )
                .map_err(storage_error)?;
            let rows = statement
                .query_map([limit], load_queue_entry)
                .map_err(storage_error)?;
            let mut processed = Vec::new();
            let mut terminalized = Vec::new();
            for row in rows {
                let mut entry = row.map_err(storage_error)?;
                let terminal = tx
                    .query_row(
                        &format!(
                            "SELECT 1 FROM runs WHERE run_id=?1 AND status IN ({})",
                            super::TERMINAL_STATUSES
                        ),
                        [entry.run_id.to_string()],
                        |_| Ok(()),
                    )
                    .optional()
                    .map_err(storage_error)?
                    .is_some();
                processed.push(entry.clone());
                if terminal {
                    tx.execute(
                        "UPDATE unavailable_provider_queue SET state='terminalized', last_operation_id=?2 WHERE queue_id=?1",
                        sqlite::params![entry.queue_id, input.operation_id],
                    )
                    .map_err(storage_error)?;
                    entry.state = UnavailableQueueStateDto::Terminalized;
                    entry.last_operation_id = Some(input.operation_id.clone());
                    terminalized.push(entry);
                }
            }
            drop(statement);
            self.fault(FaultPoint::UnavailableQueue)?;
            tx.commit().map_err(storage_error)?;
            Ok(ReconcileUnavailableQueueOutcomeDto {
                processed,
                terminalized,
            })
        })
    }

    fn load_queue_reconciliation_marker(
        &self,
        session_id: SessionId,
    ) -> DtoResult<Option<QueueReconciliationMarkerDto>> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT marker_id, session_id, created_at, reason, next_page_cursor, resolved_at FROM unavailable_queue_reconciliation_markers WHERE session_id=?1 AND resolved_at IS NULL ORDER BY marker_id DESC LIMIT 1",
                [session_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?;
        drop(connection);
        let Some((marker_id, encoded_session, created_at, reason, cursor, resolved_at)) = row
        else {
            return Ok(None);
        };
        Ok(Some(QueueReconciliationMarkerDto {
            marker_id,
            session_id: SessionId::parse(&encoded_session).map_err(codec_error)?,
            created_at,
            reason,
            next_page_cursor: cursor,
            resolved_at,
        }))
    }
}

fn load_usage_aggregate_row(
    row: &sqlite::Row<'_>,
) -> Result<ProviderUsageAggregateDto, sqlite::Error> {
    fn to_sql_error(error: ErrorDto) -> sqlite::Error {
        sqlite::Error::ToSqlConversionFailure(Box::new(error))
    }
    Ok(ProviderUsageAggregateDto {
        profile_id: row.get(0)?,
        provider_profile_revision_id: row.get(1)?,
        model_id: row.get(2)?,
        usage_period_start: row.get(3)?,
        usage_period_end: row.get(4)?,
        request_count: u64::try_from(row.get::<_, i64>(5)?).unwrap_or(u64::MAX),
        input_units: u64::try_from(row.get::<_, i64>(6)?).unwrap_or(u64::MAX),
        output_units: u64::try_from(row.get::<_, i64>(7)?).unwrap_or(u64::MAX),
        reasoning_units: u64::try_from(row.get::<_, i64>(8)?).unwrap_or(u64::MAX),
        last_run_id: row
            .get::<_, Option<String>>(9)?
            .map(|encoded| RunId::parse(&encoded))
            .transpose()
            .map_err(to_sql_error)?,
        updated_at: row.get(10)?,
    })
}

impl ProviderUsageRepositoryDto for SqliteStorageRepository {
    fn record_provider_usage(&self, input: RecordProviderUsageInputDto) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            for event in &input.events {
                let input_units = i64::try_from(event.input_units)
                    .map_err(|_| codec_error("usage units outside the SQLite range"))?;
                let output_units = i64::try_from(event.output_units)
                    .map_err(|_| codec_error("usage units outside the SQLite range"))?;
                let reasoning_units = i64::try_from(event.reasoning_units)
                    .map_err(|_| codec_error("usage units outside the SQLite range"))?;
                let inserted = tx
                    .execute(
                        "INSERT OR IGNORE INTO provider_usage_facts(run_id, usage_event_id, profile_id, provider_profile_revision_id, model_id, input_units, output_units, reasoning_units, occurred_at, usage_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                        sqlite::params![
                            event.run_id.to_string(),
                            event.usage_event_id,
                            event.profile_id,
                            event.provider_profile_revision_id,
                            event.model_id,
                            input_units,
                            output_units,
                            reasoning_units,
                            event.occurred_at,
                            event.usage_json
                        ],
                    )
                    .map_err(storage_error)?;
                if inserted == 1 {
                    tx.execute(
                        "INSERT INTO provider_usage_aggregates(profile_id, provider_profile_revision_id, model_id, usage_period_start, usage_period_end, request_count, input_units, output_units, reasoning_units, last_run_id, updated_at) VALUES (?1,?2,?3,?4,?5,1,?6,?7,?8,?9,?10) ON CONFLICT(profile_id, provider_profile_revision_id, model_id, usage_period_start, usage_period_end) DO UPDATE SET request_count=request_count+1, input_units=input_units+excluded.input_units, output_units=output_units+excluded.output_units, reasoning_units=reasoning_units+excluded.reasoning_units, last_run_id=excluded.last_run_id, updated_at=excluded.updated_at",
                        sqlite::params![
                            event.profile_id,
                            event.provider_profile_revision_id,
                            event.model_id,
                            input.usage_period_start,
                            input.usage_period_end,
                            input_units,
                            output_units,
                            reasoning_units,
                            event.run_id.to_string(),
                            input.recorded_at
                        ],
                    )
                    .map_err(storage_error)?;
                }
            }
            self.fault(FaultPoint::UsageAggregate)?;
            tx.commit().map_err(storage_error)
        })
    }

    fn load_provider_usage_by_profile(
        &self,
        profile_id: String,
    ) -> DtoResult<Vec<ProviderUsageAggregateDto>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT profile_id, provider_profile_revision_id, model_id, usage_period_start, usage_period_end, request_count, input_units, output_units, reasoning_units, last_run_id, updated_at FROM provider_usage_aggregates WHERE profile_id=?1 ORDER BY provider_profile_revision_id, model_id, usage_period_start",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([profile_id], load_usage_aggregate_row)
            .map_err(storage_error)?;
        let aggregates = rows
            .map(|row| row.map_err(storage_error))
            .collect::<DtoResult<Vec<_>>>()?;
        drop(statement);
        drop(connection);
        Ok(aggregates)
    }

    fn load_provider_usage_by_revision_and_model(
        &self,
        provider_profile_revision_id: String,
        model_id: String,
    ) -> DtoResult<Vec<ProviderUsageAggregateDto>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT profile_id, provider_profile_revision_id, model_id, usage_period_start, usage_period_end, request_count, input_units, output_units, reasoning_units, last_run_id, updated_at FROM provider_usage_aggregates WHERE provider_profile_revision_id=?1 AND model_id=?2 ORDER BY profile_id, usage_period_start",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map(
                sqlite::params![provider_profile_revision_id, model_id],
                load_usage_aggregate_row,
            )
            .map_err(storage_error)?;
        let aggregates = rows
            .map(|row| row.map_err(storage_error))
            .collect::<DtoResult<Vec<_>>>()?;
        drop(statement);
        drop(connection);
        Ok(aggregates)
    }
}

fn load_removal_candidate(
    row: &sqlite::Row<'_>,
) -> Result<ProviderCatalogRemovalCandidateDto, sqlite::Error> {
    fn to_sql_error(error: ErrorDto) -> sqlite::Error {
        sqlite::Error::ToSqlConversionFailure(Box::new(error))
    }
    Ok(ProviderCatalogRemovalCandidateDto {
        candidate_handle: row.get(0)?,
        candidate_catalog_revision_id: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(u64::MAX),
        active_catalog_revision_id: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(u64::MAX),
        created_at: row.get(3)?,
        expires_at: row.get(4)?,
        source_recheck: row.get(5)?,
        status: parse_removal_status(&row.get::<_, String>(6)?).map_err(to_sql_error)?,
        candidate_json: row.get(7)?,
        operation_id: row.get(8)?,
        completed_at: row.get(9)?,
    })
}

impl ProviderRemovalRepositoryDto for SqliteStorageRepository {
    fn create_provider_catalog_removal_candidate(
        &self,
        input: CreateProviderCatalogRemovalCandidateInputDto,
    ) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            let expires_at = input
                .created_at
                .checked_add(REMOVAL_CANDIDATE_LIFETIME_SECONDS)
                .ok_or_else(|| codec_error("removal candidate expiry overflow"))?;
            let inserted = tx
                .execute(
                    "INSERT INTO provider_catalog_removal_candidates(candidate_handle, candidate_catalog_revision_id, active_catalog_revision_id, created_at, expires_at, source_recheck, status, candidate_json, operation_id, completed_at) VALUES (?1,?2,?3,?4,?5,?6,'pending',?7,?8,NULL)",
                    sqlite::params![
                        input.candidate_handle,
                        i64::try_from(input.candidate_catalog_revision_id)
                            .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
                        i64::try_from(input.active_catalog_revision_id)
                            .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
                        input.created_at,
                        expires_at,
                        input.source_recheck,
                        input.candidate_json,
                        input.operation_id
                    ],
                )
                .map_err(|error| {
                    if error
                        .to_string()
                        .contains("one_pending_provider_catalog_candidate")
                    {
                        conflict(
                            "provider_catalog_removal_pending_exists",
                            "a pending provider catalog removal candidate already exists",
                        )
                    } else {
                        storage_error(error)
                    }
                })?;
            if inserted != 1 {
                return Err(conflict(
                    "provider_catalog_removal_candidate_conflict",
                    "the provider catalog removal candidate already exists",
                ));
            }
            insert_audit(
                &tx,
                &input.operation_id,
                "ProviderCatalogRemovalPending",
                Some(
                    i64::try_from(input.candidate_catalog_revision_id)
                        .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
                ),
                None,
                None,
                None,
                input.created_at,
                &AuditRecordJson::candidate_handle(Some(input.candidate_handle)),
            )?;
            tx.execute(
                "UPDATE provider_catalog_state SET status='pending_removal', updated_at=?1 WHERE singleton_id=1",
                [input.created_at],
            )
            .map_err(storage_error)?;
            tx.commit().map_err(storage_error)
        })
    }

    fn accept_provider_catalog_removal(
        &self,
        input: AcceptProviderCatalogRemovalInputDto,
    ) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            let candidate = tx
                .query_row(
                    "SELECT candidate_handle, candidate_catalog_revision_id, active_catalog_revision_id, created_at, expires_at, source_recheck, status, candidate_json, operation_id, completed_at FROM provider_catalog_removal_candidates WHERE candidate_handle=?1",
                    [&input.candidate_handle],
                    load_removal_candidate,
                )
                .optional()
                .map_err(storage_error)?;
            let Some(candidate) = candidate else {
                return Err(not_found(
                    "provider_catalog_removal_not_found",
                    "the requested provider catalog removal candidate does not exist",
                ));
            };
            if candidate.status != ProviderCatalogRemovalStatusDto::Pending {
                return Err(conflict(
                    "provider_catalog_removal_not_pending",
                    "the provider catalog removal candidate is no longer pending",
                ));
            }
            tx.execute(
                "UPDATE provider_catalog_removal_candidates SET status='accepted', operation_id=?2, completed_at=?3 WHERE candidate_handle=?1",
                sqlite::params![input.candidate_handle, input.operation_id, input.accepted_at],
            )
            .map_err(storage_error)?;
            insert_audit(
                &tx,
                &input.operation_id,
                "ProviderCatalogRemovalAccepted",
                Some(
                    i64::try_from(candidate.candidate_catalog_revision_id)
                        .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
                ),
                None,
                None,
                None,
                input.accepted_at,
                &AuditRecordJson::candidate_handle(Some(input.candidate_handle)),
            )?;
            tx.commit().map_err(storage_error)
        })
    }

    fn reject_provider_catalog_removal(
        &self,
        input: RejectProviderCatalogRemovalInputDto,
    ) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            let candidate = tx
                .query_row(
                    "SELECT candidate_handle, candidate_catalog_revision_id, active_catalog_revision_id, created_at, expires_at, source_recheck, status, candidate_json, operation_id, completed_at FROM provider_catalog_removal_candidates WHERE candidate_handle=?1",
                    [&input.candidate_handle],
                    load_removal_candidate,
                )
                .optional()
                .map_err(storage_error)?;
            let Some(candidate) = candidate else {
                return Err(not_found(
                    "provider_catalog_removal_not_found",
                    "the requested provider catalog removal candidate does not exist",
                ));
            };
            if candidate.status != ProviderCatalogRemovalStatusDto::Pending {
                return Err(conflict(
                    "provider_catalog_removal_not_pending",
                    "the provider catalog removal candidate is no longer pending",
                ));
            }
            tx.execute(
                "UPDATE provider_catalog_removal_candidates SET status='rejected', operation_id=?2, completed_at=?3 WHERE candidate_handle=?1",
                sqlite::params![input.candidate_handle, input.operation_id, input.rejected_at],
            )
            .map_err(storage_error)?;
            insert_audit(
                &tx,
                &input.operation_id,
                "ProviderCatalogCandidateRejected",
                Some(
                    i64::try_from(candidate.candidate_catalog_revision_id)
                        .map_err(|_| codec_error("catalog revision outside the SQLite range"))?,
                ),
                None,
                None,
                None,
                input.rejected_at,
                &AuditRecordJson::candidate_handle(Some(input.candidate_handle)),
            )?;
            tx.execute(
                "UPDATE provider_catalog_state SET status='active', updated_at=?1 WHERE singleton_id=1",
                [input.rejected_at],
            )
            .map_err(storage_error)?;
            tx.commit().map_err(storage_error)
        })
    }

    fn expire_provider_catalog_removal_candidate(
        &self,
        input: ExpireProviderCatalogRemovalCandidateInputDto,
    ) -> DtoResult<u64> {
        immediate_transaction!(self, |tx| {
            let mut statement = tx
                .prepare(
                    "SELECT candidate_handle, candidate_catalog_revision_id, active_catalog_revision_id, created_at, expires_at, source_recheck, status, candidate_json, operation_id, completed_at FROM provider_catalog_removal_candidates WHERE status='pending' AND expires_at <= ?1",
                )
                .map_err(storage_error)?;
            let rows = statement
                .query_map([input.now], load_removal_candidate)
                .map_err(storage_error)?;
            let mut expired = 0_u64;
            for row in rows {
                let candidate = row.map_err(storage_error)?;
                tx.execute(
                    "UPDATE provider_catalog_removal_candidates SET status='expired', operation_id=?2, completed_at=?3 WHERE candidate_handle=?1",
                    sqlite::params![candidate.candidate_handle, input.operation_id, input.now],
                )
                .map_err(storage_error)?;
                insert_audit(
                    &tx,
                    &input.operation_id,
                    "ProviderCatalogCandidateExpired",
                    Some(
                        i64::try_from(candidate.candidate_catalog_revision_id).map_err(|_| {
                            codec_error("catalog revision outside the SQLite range")
                        })?,
                    ),
                    None,
                    None,
                    None,
                    input.now,
                    &AuditRecordJson::candidate_handle(Some(candidate.candidate_handle)),
                )?;
                expired = expired.saturating_add(1);
            }
            drop(statement);
            if expired > 0 {
                tx.execute(
                    "UPDATE provider_catalog_state SET status='active', updated_at=?1 WHERE singleton_id=1",
                    [input.now],
                )
                .map_err(storage_error)?;
            }
            tx.commit().map_err(storage_error)?;
            Ok(expired)
        })
    }
}

impl HeldRunRepositoryDto for SqliteStorageRepository {
    fn mark_recovered_run_held(&self, input: MarkRecoveredRunHeldInputDto) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            tx.execute(
                "INSERT OR IGNORE INTO held_recovered_runs(run_id, session_id, held_at, reason, admission_state, admission_operation_id, admitted_at) VALUES (?1,?2,?3,'recovered_run_requires_explicit_admission','held',?4,NULL)",
                sqlite::params![
                    input.run_id.to_string(),
                    input.session_id.to_string(),
                    input.held_at,
                    input.operation_id
                ],
            )
            .map_err(storage_error)?;
            self.fault(FaultPoint::HeldRun)?;
            tx.commit().map_err(storage_error)
        })
    }

    fn admit_held_recovered_run(&self, input: AdmitHeldRecoveredRunInputDto) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            let state = tx
                .query_row(
                    "SELECT admission_state FROM held_recovered_runs WHERE run_id=?1 AND session_id=?2",
                    sqlite::params![input.run_id.to_string(), input.session_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(storage_error)?;
            match state.as_deref() {
                None => {
                    return Err(not_found(
                        "held_recovered_run_not_found",
                        "the requested held recovered run does not exist",
                    ));
                }
                Some("admitted") => {
                    // Idempotent admission never creates a second task.
                    self.fault(FaultPoint::HeldRun)?;
                    tx.commit().map_err(storage_error)?;
                    return Ok(());
                }
                Some("rejected") => {
                    return Err(conflict(
                        "held_recovered_run_rejected",
                        "the held recovered run was already rejected",
                    ));
                }
                Some("held") => {
                    tx.execute(
                        "UPDATE held_recovered_runs SET admission_state='admitted', admission_operation_id=?2, admitted_at=?3 WHERE run_id=?1 AND session_id=?4",
                        sqlite::params![
                            input.run_id.to_string(),
                            input.operation_id,
                            input.admitted_at,
                            input.session_id.to_string()
                        ],
                    )
                    .map_err(storage_error)?;
                    self.fault(FaultPoint::HeldRun)?;
                    tx.commit().map_err(storage_error)?;
                    return Ok(());
                }
                Some(_) => Err(codec_error("invalid durable held-run admission state")),
            }
        })
    }

    fn load_held_recovered_run(&self, run_id: RunId) -> DtoResult<Option<HeldRecoveredRunDto>> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT run_id, session_id, held_at, reason, admission_state, admission_operation_id, admitted_at FROM held_recovered_runs WHERE run_id=?1",
                [run_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?;
        drop(connection);
        let Some((encoded_run, encoded_session, held_at, reason, state, operation_id, admitted_at)) =
            row
        else {
            return Ok(None);
        };
        Ok(Some(HeldRecoveredRunDto {
            run_id: RunId::parse(&encoded_run).map_err(codec_error)?,
            session_id: SessionId::parse(&encoded_session).map_err(codec_error)?,
            held_at,
            reason,
            admission_state: parse_admission_state(&state)?,
            admission_operation_id: operation_id,
            admitted_at,
        }))
    }
}

impl LegacyBindingRepositoryDto for SqliteStorageRepository {
    fn append_legacy_m4_selection_binding(
        &self,
        input: AppendLegacyM4SelectionBindingInputDto,
    ) -> DtoResult<()> {
        immediate_transaction!(self, |tx| {
            let (binding_json, binding_digest) = match &input.binding {
                Some(binding) => {
                    let json = legacy_binding_json(binding).to_json();
                    let digest = record_digest(&json);
                    (json, digest)
                }
                None => {
                    let json = corrupt_legacy_binding_json(&input.config_revision_id).to_json();
                    let digest = record_digest(&json);
                    (json, digest)
                }
            };
            let existing = tx
                .query_row(
                    "SELECT binding_digest FROM legacy_m4_selection_bindings WHERE config_revision_id=?1",
                    [&input.config_revision_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(storage_error)?;
            if let Some(existing_digest) = existing {
                if existing_digest != binding_digest {
                    return Err(conflict(
                        "legacy_binding_conflict",
                        "the configuration revision already bound a different legacy selection",
                    ));
                }
                return Ok(());
            }
            let (profile_id, revision_id, kind_id, kind_revision, driver_contract) =
                input.binding.as_ref().map_or_else(
                    || {
                        (
                            String::new(),
                            String::new(),
                            String::new(),
                            String::new(),
                            String::new(),
                        )
                    },
                    |binding| {
                        (
                            binding.default_profile_id.clone(),
                            binding.default_profile_revision_id.clone(),
                            String::new(),
                            binding.kind_descriptor_revision_id.clone(),
                            binding.driver_contract_revision.clone(),
                        )
                    },
                );
            let validation_status = match input.validation_status {
                LegacyBindingValidationStatusDto::Validated => "validated",
                LegacyBindingValidationStatusDto::Corrupt => "corrupt",
            };
            tx.execute(
                "INSERT INTO legacy_m4_selection_bindings(config_revision_id, profile_id, provider_profile_revision_id, kind_id, kind_descriptor_revision_id, provider_driver_contract_revision, binding_digest, snapshot_bytes_digest, validation_status, binding_json, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                sqlite::params![
                    input.config_revision_id,
                    profile_id,
                    revision_id,
                    kind_id,
                    kind_revision,
                    driver_contract,
                    binding_digest,
                    input.snapshot_bytes_digest,
                    validation_status,
                    binding_json,
                    input.created_at
                ],
            )
            .map_err(storage_error)?;
            tx.commit().map_err(storage_error)
        })
    }

    fn load_legacy_m4_selection_binding(
        &self,
        config_revision_id: String,
    ) -> DtoResult<Option<LegacyM4SelectionBindingRecordDto>> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT config_revision_id, profile_id, provider_profile_revision_id, kind_id, kind_descriptor_revision_id, provider_driver_contract_revision, binding_digest, snapshot_bytes_digest, validation_status, binding_json, created_at FROM legacy_m4_selection_bindings WHERE config_revision_id=?1",
                [&config_revision_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, i64>(10)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?;
        drop(connection);
        let Some((
            revision,
            profile_id,
            revision_id,
            kind_id,
            kind_revision,
            driver_contract,
            binding_digest,
            snapshot_bytes_digest,
            validation_status,
            binding_json,
            created_at,
        )) = row
        else {
            return Ok(None);
        };
        Ok(Some(LegacyM4SelectionBindingRecordDto {
            config_revision_id: revision,
            profile_id,
            provider_profile_revision_id: revision_id,
            kind_id,
            kind_descriptor_revision_id: kind_revision,
            provider_driver_contract_revision: driver_contract,
            binding_digest,
            snapshot_bytes_digest,
            validation_status: parse_legacy_validation_status(&validation_status)?,
            binding_json,
            created_at,
        }))
    }
}

/// Ensures the schema-4 seed row exists after opening (defensive; the migration
/// normally creates it). Never touches pre-existing rows.
pub fn ensure_catalog_state_seed(connection: &sqlite::Connection) -> DtoResult<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO provider_catalog_state(singleton_id, active_catalog_revision_id, candidate_catalog_revision_id, status, active_default_profile_id, candidate_handle, degraded_reason, updated_at) VALUES (1, NULL, NULL, 'preparing', NULL, NULL, NULL, 0)",
            [],
        )
        .map_err(|_| unavailable())?;
    Ok(())
}
