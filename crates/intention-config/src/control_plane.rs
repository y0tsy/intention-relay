//! Slice 2 controlled-reload candidate layer.
//!
//! This module is the candidate side of the controlled configuration reload
//! contract: it parses and validates a raw TOML candidate through the existing
//! startup [`ResolvedConfigDto::parse_resolve`] path, projects a
//! credential-free candidate snapshot, and exposes safe semantic comparison,
//! change classification, catalog-affecting rejection, and a credential-free
//! digest.
//!
//! Raw TOML content and credential material never appear in DTOs, errors,
//! digests, or serialized output. The raw text exists only transiently inside
//! the parse call; responses and DTOs never echo it.

use crate::{ConfigSnapshotDto, RawConfigInputDto, ResolvedConfigDto};
use intention_types::{
    ConfigRevisionId, DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto, TimestampDto,
};
use serde::{Deserialize, Deserializer, Serialize};

/// Maximum raw candidate TOML bytes accepted by the reload contract.
pub const MAX_CANDIDATE_RAW_BYTES: usize = 512 * 1024;
/// Maximum safe validation issues returned for one candidate.
pub const MAX_CANDIDATE_ISSUES: usize = 32;

/// Identifies how a candidate configuration was produced without disclosing it.
///
/// The `RawToml` variant intentionally carries only the byte size and never the
/// raw content: the content exists only transiently inside the parse call, and
/// responses and DTOs never echo it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigCandidateSourceDto {
    /// A raw TOML document supplied by the caller.
    RawToml {
        /// The byte length of the raw document, without any content.
        size_bytes: u64,
    },
    /// A structured edit operation applied to the active configuration.
    StructuredEdits,
    /// The startup configuration file itself.
    StartupFile,
}

impl ConfigCandidateSourceDto {
    /// Returns the stable safe representation for diagnostics and snapshots.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RawToml { .. } => "raw_toml",
            Self::StructuredEdits => "structured_edits",
            Self::StartupFile => "startup_file",
        }
    }
}

/// One deterministic, safe validation finding for a candidate.
///
/// Messages are static and never contain raw TOML content or credential
/// material; `field` carries only the structural configuration path.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateIssueDto {
    code: String,
    field: Option<String>,
    message: String,
}

impl<'de> Deserialize<'de> for CandidateIssueDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCandidateIssueDto {
            code: String,
            field: Option<String>,
            message: String,
        }

        let raw = RawCandidateIssueDto::deserialize(deserializer)?;
        Ok(Self {
            code: raw.code,
            field: raw.field,
            message: raw.message,
        })
    }
}

impl CandidateIssueDto {
    fn new(code: impl Into<String>, field: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            field: field.map(str::to_owned),
            message: message.into(),
        }
    }

    /// Returns the stable machine-readable issue code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the structural configuration field path, when one applies.
    #[must_use]
    pub fn field(&self) -> Option<&str> {
        self.field.as_deref()
    }

    /// Returns the safe static human-readable issue message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// A bounded, deterministic summary of candidate validation issues.
///
/// Issues are collected in deterministic order and bounded at
/// [`MAX_CANDIDATE_ISSUES`]; overflow is reported through `truncated` and the
/// preserved `total_issue_count`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateValidationSummaryDto {
    issues: Vec<CandidateIssueDto>,
    truncated: bool,
    total_issue_count: u32,
}

impl<'de> Deserialize<'de> for CandidateValidationSummaryDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCandidateValidationSummaryDto {
            issues: Vec<CandidateIssueDto>,
            truncated: bool,
            total_issue_count: u32,
        }

        let raw = RawCandidateValidationSummaryDto::deserialize(deserializer)?;
        Ok(Self {
            issues: raw.issues,
            truncated: raw.truncated,
            total_issue_count: raw.total_issue_count,
        })
    }
}

impl CandidateValidationSummaryDto {
    const fn new(issues: Vec<CandidateIssueDto>, truncated: bool, total_issue_count: u32) -> Self {
        Self {
            issues,
            truncated,
            total_issue_count,
        }
    }

    /// Returns the bounded, deterministic issue list.
    #[must_use]
    pub fn issues(&self) -> &[CandidateIssueDto] {
        &self.issues
    }

    /// Returns whether more issues exist than the bounded list carries.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    /// Returns the complete issue count before bounding.
    #[must_use]
    pub const fn total_issue_count(&self) -> u32 {
        self.total_issue_count
    }
}

/// A credential-free candidate configuration produced by the reload contract.
///
/// The candidate carries a fresh, not-yet-persisted revision identity, its
/// safe source classification, a safe snapshot, and a bounded validation
/// summary. When validation fails, the safe snapshot is the previous
/// (unchanged) snapshot so the daemon never advances on invalid input. The
/// candidate never holds raw TOML or credential material.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConfigCandidateDto {
    candidate_revision_id: String,
    source: ConfigCandidateSourceDto,
    safe_snapshot: ConfigSnapshotDto,
    validation: CandidateValidationSummaryDto,
}

impl<'de> Deserialize<'de> for ConfigCandidateDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawConfigCandidateDto {
            candidate_revision_id: String,
            source: ConfigCandidateSourceDto,
            safe_snapshot: ConfigSnapshotDto,
            validation: CandidateValidationSummaryDto,
        }

        let raw = RawConfigCandidateDto::deserialize(deserializer)?;
        if raw.candidate_revision_id.trim().is_empty() {
            return Err(serde::de::Error::custom(
                "candidate revision id must not be empty",
            ));
        }
        Ok(Self {
            candidate_revision_id: raw.candidate_revision_id,
            source: raw.source,
            safe_snapshot: raw.safe_snapshot,
            validation: raw.validation,
        })
    }
}

impl ConfigCandidateDto {
    /// Returns the fresh candidate revision identity, not yet persisted.
    #[must_use]
    pub fn candidate_revision_id(&self) -> &str {
        &self.candidate_revision_id
    }

    /// Returns the safe candidate source classification.
    #[must_use]
    pub const fn source(&self) -> ConfigCandidateSourceDto {
        self.source
    }

    /// Returns the safe candidate configuration snapshot.
    #[must_use]
    pub const fn safe_snapshot(&self) -> &ConfigSnapshotDto {
        &self.safe_snapshot
    }

    /// Returns the bounded candidate validation summary.
    #[must_use]
    pub const fn validation(&self) -> &CandidateValidationSummaryDto {
        &self.validation
    }
}

/// The acceptance projection of a candidate for the reload orchestration zone.
///
/// `accepted` records whether the candidate may be applied; `changed_semantics`
/// records whether the candidate changes any semantic field relative to the
/// active snapshot; `changed_field_categories` carries the closed category
/// list from [`classify_changed_fields`]; `failure_code` carries the typed
/// rejection code when the candidate was not accepted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CandidateAcceptanceOutcomeDto {
    accepted: bool,
    changed_semantics: bool,
    changed_field_categories: Vec<String>,
    failure_code: Option<String>,
}

impl<'de> Deserialize<'de> for CandidateAcceptanceOutcomeDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawCandidateAcceptanceOutcomeDto {
            accepted: bool,
            changed_semantics: bool,
            changed_field_categories: Vec<String>,
            failure_code: Option<String>,
        }

        let raw = RawCandidateAcceptanceOutcomeDto::deserialize(deserializer)?;
        Ok(Self {
            accepted: raw.accepted,
            changed_semantics: raw.changed_semantics,
            changed_field_categories: raw.changed_field_categories,
            failure_code: raw.failure_code,
        })
    }
}

impl CandidateAcceptanceOutcomeDto {
    /// Creates an acceptance projection from the reload decision inputs.
    #[must_use]
    pub const fn new(
        accepted: bool,
        changed_semantics: bool,
        changed_field_categories: Vec<String>,
        failure_code: Option<String>,
    ) -> Self {
        Self {
            accepted,
            changed_semantics,
            changed_field_categories,
            failure_code,
        }
    }

    /// Returns whether the candidate may be applied.
    #[must_use]
    pub const fn accepted(&self) -> bool {
        self.accepted
    }

    /// Returns whether the candidate changes any semantic field.
    #[must_use]
    pub const fn changed_semantics(&self) -> bool {
        self.changed_semantics
    }

    /// Returns the closed categories of fields the candidate changes.
    #[must_use]
    pub fn changed_field_categories(&self) -> &[String] {
        &self.changed_field_categories
    }

    /// Returns the typed rejection code, when the candidate was not accepted.
    #[must_use]
    pub fn failure_code(&self) -> Option<&str> {
        self.failure_code.as_deref()
    }
}

/// Parses and validates a raw TOML reload candidate.
///
/// The candidate is validated through the existing startup
/// [`ResolvedConfigDto::parse_resolve`] path. Raw content larger than
/// [`MAX_CANDIDATE_RAW_BYTES`] and raw content carrying credential-shaped
/// values are rejected without echoing any content. Validation findings are
/// collected deterministically and bounded at [`MAX_CANDIDATE_ISSUES`]. A
/// valid candidate carries a new snapshot with a fresh revision id; an invalid
/// candidate carries the unchanged `previous` snapshot so the active
/// configuration never advances on invalid input.
///
/// # Errors
///
/// Returns `candidate_too_large` when the raw content exceeds 512 KiB, and
/// `credentials_forbidden` when the raw content carries a credential-shaped
/// value outside the legitimate credential key. Both errors are static and
/// never contain raw content.
pub fn parse_candidate(
    raw: RawConfigInputDto,
    previous: &ConfigSnapshotDto,
) -> DtoResult<ConfigCandidateDto> {
    let size_bytes = raw.text.len() as u64;
    if raw.text.len() > MAX_CANDIDATE_RAW_BYTES {
        return Err(ErrorDto::validation(
            "candidate_too_large",
            "candidate raw configuration exceeds the 512 KiB reload limit",
        ));
    }
    let document = toml::from_str::<toml::Value>(&raw.text).ok();
    if document
        .as_ref()
        .is_some_and(has_forbidden_credential_shaped_content)
    {
        return Err(ErrorDto::new(
            "credentials_forbidden",
            ErrorCategoryDto::Policy,
            "credential-shaped values are not permitted in a reload candidate",
            ErrorRetryDto::Manual,
            None,
        )?);
    }
    let issues = document.map_or_else(
        || {
            vec![CandidateIssueDto::new(
                "invalid_config_toml",
                None,
                "configuration TOML could not be parsed",
            )]
        },
        |parsed| collect_validation_issues(&parsed),
    );
    let validation = bounded_validation_summary(issues);
    let candidate_revision_id = ConfigRevisionId::new();
    let safe_snapshot = match ResolvedConfigDto::parse_resolve(raw) {
        Ok(resolved) => {
            let captured_at = now_timestamp()?;
            ConfigSnapshotDto::new(
                resolved.schema_version(),
                candidate_revision_id,
                captured_at,
                resolved,
            )?
        }
        Err(_) => previous.clone(),
    };
    Ok(ConfigCandidateDto {
        candidate_revision_id: candidate_revision_id.to_string(),
        source: ConfigCandidateSourceDto::RawToml { size_bytes },
        safe_snapshot,
        validation,
    })
}

/// Compares the semantic fields of two snapshots without credential material.
///
/// Participating resolved fields: schema version, provider kind, model,
/// endpoint, execution policy (attempt timeout and max attempts), and source
/// kind. Excluded: revision identity, capture timestamp, snapshot contract
/// version, and the credential-derived `credential_configured` flag, so a
/// credential-only replacement compares equal while any changed
/// model/endpoint/kind/capability/policy compares different. TOML whitespace
/// and key order never participate because both inputs are already resolved.
#[must_use]
pub fn semantic_equivalence(left: &ConfigSnapshotDto, right: &ConfigSnapshotDto) -> bool {
    left.resolved().schema_version() == right.resolved().schema_version()
        && left.resolved().provider().kind() == right.resolved().provider().kind()
        && left.resolved().provider().model() == right.resolved().provider().model()
        && left.resolved().provider().endpoint() == right.resolved().provider().endpoint()
        && left.resolved().provider_execution() == right.resolved().provider_execution()
        && left.resolved().source_kind() == right.resolved().source_kind()
}

/// Classifies the changed fields between two snapshots into closed categories.
///
/// The closed category set is `provider_kind`, `model`, `endpoint`,
/// `capability_subset`, `execution_policy`, `credential_transport`,
/// `reasoning_policy`, `display`, and `other`, in that fixed order. Only
/// categories that actually changed are returned. In this slice the resolved
/// projection has no capability, credential-transport, or reasoning-policy
/// fields, so those categories cannot fire yet and are reserved for later
/// slices. `display` covers the capture timestamp; `other` covers the source
/// kind and resolved schema version. Revision identity is deliberately
/// unclassified because it is neither semantic nor display metadata.
#[must_use]
pub fn classify_changed_fields(left: &ConfigSnapshotDto, right: &ConfigSnapshotDto) -> Vec<String> {
    let mut changed = Vec::new();
    if left.resolved().provider().kind() != right.resolved().provider().kind() {
        changed.push("provider_kind".to_owned());
    }
    if left.resolved().provider().model() != right.resolved().provider().model() {
        changed.push("model".to_owned());
    }
    if left.resolved().provider().endpoint() != right.resolved().provider().endpoint() {
        changed.push("endpoint".to_owned());
    }
    if left.resolved().provider_execution() != right.resolved().provider_execution() {
        changed.push("execution_policy".to_owned());
    }
    if left.captured_at() != right.captured_at() {
        changed.push("display".to_owned());
    }
    if left.resolved().schema_version() != right.resolved().schema_version()
        || left.resolved().source_kind() != right.resolved().source_kind()
    {
        changed.push("other".to_owned());
    }
    changed
}

/// Rejects candidate changes that would alter provider-kind/catalog semantics.
///
/// The provider catalog is startup-only per the configuration/provider
/// control-plane architecture (architecture 25, owned by architectures 22 and
/// 29): Slice 2 reload must not silently change catalog or profile semantics.
/// A candidate that changes the provider kind (the catalog-bound selection in
/// this slice) is rejected with `catalog_change_requires_restart`; later
/// slices add explicit catalog activation through the same reload contract.
/// Non-catalog fields (model, endpoint, execution policy) pass.
///
/// # Errors
///
/// Returns `catalog_change_requires_restart` when the candidate changes the
/// provider kind relative to the previous snapshot.
pub fn reject_catalog_affecting_edits(
    candidate: &ConfigCandidateDto,
    previous: &ConfigSnapshotDto,
) -> DtoResult<()> {
    let catalog_affected = classify_changed_fields(candidate.safe_snapshot(), previous)
        .iter()
        .any(|category| category.as_str() == "provider_kind");
    if catalog_affected {
        Err(ErrorDto::new(
            "catalog_change_requires_restart",
            ErrorCategoryDto::Policy,
            "catalog-affecting configuration changes require a daemon restart",
            ErrorRetryDto::Manual,
            None,
        )?)
    } else {
        Ok(())
    }
}

/// Computes the deterministic credential-free digest of a candidate.
///
/// The digest is SHA-256 over the candidate's safe semantic fields (schema
/// version, provider kind, model, endpoint, execution policy, and source
/// kind). Credential material, revision identity, capture time, and validation
/// issues never participate, so credential-only differences produce identical
/// digests and any semantic difference produces a different digest. The
/// returned value is sixty-four lowercase hexadecimal characters.
#[must_use]
pub fn redacted_safe_digest(candidate: &ConfigCandidateDto) -> String {
    let snapshot = candidate.safe_snapshot();
    let canonical = format!(
        "ir-config-candidate-v1|schema={}.{}|kind={}|model={}|endpoint={}|attempt_timeout_seconds={}|max_attempts={}|source_kind={}",
        snapshot.resolved().schema_version().major(),
        snapshot.resolved().schema_version().minor(),
        snapshot.resolved().provider().kind().as_str(),
        snapshot.resolved().provider().model(),
        snapshot.resolved().provider().endpoint().unwrap_or(""),
        snapshot
            .resolved()
            .provider_execution()
            .attempt_timeout_seconds(),
        snapshot.resolved().provider_execution().max_attempts(),
        snapshot.resolved().source_kind().as_str(),
    );
    sha256::hex(&sha256::digest(canonical.as_bytes()))
}

/// Bounds a collected issue list at [`MAX_CANDIDATE_ISSUES`].
///
/// The first issues are preserved in collection order; the complete count and
/// a truncation flag are carried alongside.
#[must_use]
fn bounded_validation_summary(issues: Vec<CandidateIssueDto>) -> CandidateValidationSummaryDto {
    let total_issue_count = issues.len() as u32;
    let truncated = issues.len() > MAX_CANDIDATE_ISSUES;
    let mut bounded = issues;
    bounded.truncate(MAX_CANDIDATE_ISSUES);
    CandidateValidationSummaryDto::new(bounded, truncated, total_issue_count)
}

/// Collects one deterministic issue per field-level problem in a TOML document.
///
/// Checks mirror the startup [`ResolvedConfigDto::parse_resolve`] validation
/// rules at field granularity, so a valid configuration collects no issues.
#[must_use]
fn collect_validation_issues(document: &toml::Value) -> Vec<CandidateIssueDto> {
    let mut issues = Vec::new();
    let Some(table) = document.as_table() else {
        issues.push(CandidateIssueDto::new(
            "invalid_config_toml",
            None,
            "configuration TOML could not be parsed",
        ));
        return issues;
    };
    match table.get("schema_version") {
        Some(toml::Value::Integer(major)) if *major == 1 => {
            collect_v1_issues(table, &mut issues);
        }
        Some(toml::Value::Integer(_)) => issues.push(CandidateIssueDto::new(
            "unsupported_config_schema_version",
            Some("schema_version"),
            "configuration schema version is not supported",
        )),
        Some(_) => issues.push(CandidateIssueDto::new(
            "invalid_config_schema_version",
            Some("schema_version"),
            "configuration schema version must be an integer",
        )),
        None => issues.push(CandidateIssueDto::new(
            "invalid_config_schema",
            Some("schema_version"),
            "configuration does not include a schema version",
        )),
    }
    issues
}

/// Collects v1 field-level issues in deterministic order.
fn collect_v1_issues(table: &toml::Table, issues: &mut Vec<CandidateIssueDto>) {
    for key in table.keys() {
        if key != "schema_version" && key != "provider" {
            issues.push(unknown_field_issue(key));
        }
    }
    let Some(provider) = table.get("provider") else {
        issues.push(CandidateIssueDto::new(
            "invalid_config_schema",
            Some("provider"),
            "configuration does not include a provider table",
        ));
        return;
    };
    let Some(provider_table) = provider.as_table() else {
        issues.push(CandidateIssueDto::new(
            "invalid_config_schema",
            Some("provider"),
            "provider must be a configuration table",
        ));
        return;
    };
    validate_provider_kind(provider_table.get("kind"), "provider.kind", issues);
    validate_provider_model(provider_table.get("model"), "provider.model", issues);
    validate_provider_credential(
        provider_table.get("credential"),
        "provider.credential",
        issues,
    );
    validate_provider_endpoint(provider_table.get("endpoint"), "provider.endpoint", issues);
    validate_provider_execution(
        provider_table.get("execution"),
        "provider.execution",
        issues,
    );
    for key in provider_table.keys() {
        if !matches!(
            key.as_str(),
            "kind" | "model" | "credential" | "endpoint" | "execution"
        ) {
            issues.push(unknown_field_issue(&format!("provider.{key}")));
        }
    }
}

/// Validates a provider kind value against the closed kind set.
fn validate_provider_kind(
    value: Option<&toml::Value>,
    field: &str,
    issues: &mut Vec<CandidateIssueDto>,
) {
    let supported = matches!(
        value.and_then(toml::Value::as_str),
        Some("openrouter") | Some("generic-chat-completion-api")
    );
    if !supported {
        issues.push(CandidateIssueDto::new(
            "invalid_provider_kind",
            Some(field),
            "provider kind is not supported",
        ));
    }
}

/// Validates a provider model value as non-empty.
fn validate_provider_model(
    value: Option<&toml::Value>,
    field: &str,
    issues: &mut Vec<CandidateIssueDto>,
) {
    if value
        .and_then(toml::Value::as_str)
        .is_none_or(|model| model.trim().is_empty())
    {
        issues.push(CandidateIssueDto::new(
            "invalid_provider_model",
            Some(field),
            "provider model must not be empty",
        ));
    }
}

/// Validates a provider credential value as non-empty.
fn validate_provider_credential(
    value: Option<&toml::Value>,
    field: &str,
    issues: &mut Vec<CandidateIssueDto>,
) {
    if value
        .and_then(toml::Value::as_str)
        .is_none_or(|credential| credential.trim().is_empty())
    {
        issues.push(CandidateIssueDto::new(
            "missing_provider_credential",
            Some(field),
            "provider credential must not be empty",
        ));
    }
}

/// Validates a configured provider endpoint as non-empty.
fn validate_provider_endpoint(
    value: Option<&toml::Value>,
    field: &str,
    issues: &mut Vec<CandidateIssueDto>,
) {
    if let Some(endpoint) = value
        && endpoint
            .as_str()
            .is_none_or(|configured| configured.trim().is_empty())
    {
        issues.push(CandidateIssueDto::new(
            "invalid_provider_endpoint",
            Some(field),
            "provider endpoint must not be empty when configured",
        ));
    }
}

/// Validates the provider execution policy table and its bounded values.
fn validate_provider_execution(
    value: Option<&toml::Value>,
    field: &str,
    issues: &mut Vec<CandidateIssueDto>,
) {
    let Some(execution) = value else {
        return;
    };
    let Some(execution_table) = execution.as_table() else {
        issues.push(CandidateIssueDto::new(
            "invalid_config_schema",
            Some(field),
            "provider execution policy must be a table",
        ));
        return;
    };
    if let Some(timeout) = execution_table.get("attempt_timeout_seconds") {
        let in_range =
            matches!(timeout, toml::Value::Integer(seconds) if (1..=60).contains(seconds));
        if !in_range {
            issues.push(CandidateIssueDto::new(
                "invalid_provider_attempt_timeout_seconds",
                Some("provider.execution.attempt_timeout_seconds"),
                "provider attempt timeout seconds must be between 1 and 60",
            ));
        }
    }
    if let Some(attempts) = execution_table.get("max_attempts") {
        let in_range = matches!(attempts, toml::Value::Integer(count) if (1..=2).contains(count));
        if !in_range {
            issues.push(CandidateIssueDto::new(
                "invalid_provider_max_attempts",
                Some("provider.execution.max_attempts"),
                "provider max attempts must be between 1 and 2",
            ));
        }
    }
    for key in execution_table.keys() {
        if !matches!(key.as_str(), "attempt_timeout_seconds" | "max_attempts") {
            issues.push(unknown_field_issue(&format!("{field}.{key}")));
        }
    }
}

/// Builds an unrecognized-field issue with its structural path.
#[must_use]
fn unknown_field_issue(path: &str) -> CandidateIssueDto {
    CandidateIssueDto::new(
        "unknown_config_field",
        Some(path),
        "unrecognized configuration field",
    )
}

/// Whether the parsed document carries credential-shaped content outside the
/// legitimate credential key.
///
/// The legitimate credential key is `provider.credential`; its value is
/// allowed through and redacted by the startup projection. Any other
/// credential-shaped key or value, including `model.api_key` in an unversioned
/// document, is forbidden.
#[must_use]
fn has_forbidden_credential_shaped_content(document: &toml::Value) -> bool {
    contains_forbidden_credential(document, "", "provider.credential")
}

/// Walks a TOML value tree for forbidden credential-shaped keys and values.
fn contains_forbidden_credential(value: &toml::Value, path: &str, legitimate: &str) -> bool {
    match value {
        toml::Value::Table(table) => table.iter().any(|(key, child)| {
            let child_path = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            (is_credential_shaped_key(key) && child_path != legitimate)
                || contains_forbidden_credential(child, &child_path, legitimate)
        }),
        toml::Value::String(text) => path != legitimate && contains_credential_shape(text),
        toml::Value::Array(items) => items
            .iter()
            .any(|item| contains_forbidden_credential(item, path, legitimate)),
        _ => false,
    }
}

/// Whether a configuration key name is credential-shaped.
#[must_use]
fn is_credential_shaped_key(key: &str) -> bool {
    contains_credential_shape(key)
        || matches!(
            key.to_ascii_lowercase().as_str(),
            "token" | "bearer" | "auth" | "authorization" | "access_token" | "auth_token"
        )
}

/// Whether a value carries a credential-shaped pattern.
///
/// Mirrors the repository-wide credential-shape convention (see the domain
/// canonical layer): `sk-` prefixes, bearer tokens, `api_key`/`apikey`,
/// `secret`, `password`, and `token=`/`key=`/`auth=` assignment patterns.
#[must_use]
fn contains_credential_shape(value: &str) -> bool {
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

/// Returns the current whole-second Unix timestamp.
///
/// # Errors
///
/// Returns `candidate_timestamp_unavailable` when the system clock precedes
/// the Unix epoch.
fn now_timestamp() -> DtoResult<TimestampDto> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| {
            ErrorDto::unavailable(
                "candidate_timestamp_unavailable",
                "a reliable configuration timestamp could not be obtained",
            )
        })?
        .as_secs() as i64;
    TimestampDto::from_unix_seconds(seconds)
}

/// Dependency-free SHA-256 used exclusively for the credential-free candidate
/// digest. Verified against published known-answer vectors in the module tests.
mod sha256 {
    /// The sixty-four SHA-256 round constants: the first thirty-two bits of
    /// the fractional parts of the cube roots of the first sixty-four primes.
    const ROUND_CONSTANTS: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    /// Computes the SHA-256 digest of `data` as thirty-two big-endian bytes.
    #[must_use]
    pub fn digest(data: &[u8]) -> [u8; 32] {
        let bit_length = (data.len() as u64) << 3;
        let pad_length = 55usize.wrapping_sub(data.len()) & 63;
        let mut padded = Vec::with_capacity(data.len() + 1 + pad_length + 8);
        padded.extend_from_slice(data);
        padded.push(0x80);
        padded.resize(padded.len() + pad_length, 0);
        padded.extend_from_slice(&bit_length.to_be_bytes());

        let mut state = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];
        for block in padded.chunks_exact(64) {
            compress(&mut state, block);
        }
        let mut out = [0u8; 32];
        for (index, word) in state.iter().enumerate() {
            out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    /// Encodes `bytes` as sixty-four lowercase hexadecimal characters.
    #[must_use]
    pub fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// Compresses one sixty-four-byte block into the running state.
    fn compress(state: &mut [u32; 8], block: &[u8]) {
        let mut schedule = [0u32; 64];
        for (index, word) in schedule.iter_mut().enumerate().take(16) {
            *word = read_u32_be(block, index * 4);
        }
        for index in 16..64 {
            let sigma0 = schedule[index - 15].rotate_right(7)
                ^ schedule[index - 15].rotate_right(18)
                ^ (schedule[index - 15] >> 3);
            let sigma1 = schedule[index - 2].rotate_right(17)
                ^ schedule[index - 2].rotate_right(19)
                ^ (schedule[index - 2] >> 10);
            schedule[index] = sigma1
                .wrapping_add(schedule[index - 7])
                .wrapping_add(sigma0)
                .wrapping_add(schedule[index - 16]);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for index in 0..64 {
            let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(sum1)
                .wrapping_add(choice)
                .wrapping_add(ROUND_CONSTANTS[index])
                .wrapping_add(schedule[index]);
            let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sum0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    /// Reads one big-endian thirty-two-bit word at `offset` within a block.
    #[must_use]
    fn read_u32_be(block: &[u8], offset: usize) -> u32 {
        u32::from_be_bytes([
            block[offset],
            block[offset + 1],
            block[offset + 2],
            block[offset + 3],
        ])
    }

    #[cfg(test)]
    mod tests {
        #![allow(
            clippy::expect_used,
            reason = "Known-answer fixtures use expect to provide precise test failure messages."
        )]

        use super::{digest, hex};

        /// Matches SHA-256 against the published FIPS 180-4 known-answer vectors.
        #[test]
        fn sha256_matches_published_known_answer_vectors() {
            assert_eq!(
                hex(&digest(b"")),
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "empty vector"
            );
            assert_eq!(
                hex(&digest(b"abc")),
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                "single-block vector"
            );
            assert_eq!(
                hex(&digest(
                    b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
                )),
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
                "multi-block vector"
            );
            assert_eq!(
                hex(&digest(&[0x61; 1_000_000])),
                "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
                "million-a vector"
            );
        }
    }
}
