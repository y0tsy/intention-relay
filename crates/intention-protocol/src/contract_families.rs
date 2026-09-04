//! Additive protocol contract-family DTOs.
use intention_domain::canonical::TagRegistry;
use intention_types::{DtoResult, ErrorDto};
use serde::{Deserialize, Serialize};

fn valid_text(value: &str, max: usize, code: &'static str) -> DtoResult<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max || value.chars().any(char::is_control) {
        Err(ErrorDto::validation(code, "text value is invalid"))
    } else {
        Ok(value.to_owned())
    }
}
fn digest(value: &str) -> DtoResult<String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        Ok(value.to_owned())
    } else {
        Err(ErrorDto::validation(
            "invalid_digest",
            "digest must be 64 lowercase hexadecimal characters",
        ))
    }
}
fn bounded<T>(items: Vec<T>, max: usize, code: &'static str) -> DtoResult<Vec<T>> {
    if items.len() <= max {
        Ok(items)
    } else {
        Err(ErrorDto::validation(code, "collection limit exceeded"))
    }
}
/// Whether `value` looks like a credential.
///
/// Identifier role of the shared credential-shape policy owned by
/// `intention-domain::canonical` (PR24-035): a value is credential-shaped
/// when it carries any control character anywhere, contains the
/// case-insensitive `key` or `token` substring, starts an `sk-` secret
/// anywhere in the string, or holds a case-insensitive `bearer` token
/// followed by a non-empty token. Trimmed and whitespace-padded variants are
/// detected too. Detection is intentionally over-inclusive: it is used to
/// fail closed on provider-adjacent fields.
#[must_use]
fn credential_shaped(value: &str) -> bool {
    intention_domain::canonical::credential_shaped_identifier(value)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialTransportMode {
    Bearer,
    SafeHeader,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ResolvedRunProviderSelectionDto {
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
impl ResolvedRunProviderSelectionDto {
    /// Validates that the selection is credential-free and carries a
    /// normalized endpoint and provider kind.
    ///
    /// # Errors
    ///
    /// Returns `credentials_forbidden` when any field looks like a key, token,
    /// `sk-` secret, `Bearer` credential, or carries a control character
    /// anywhere; `invalid_endpoint` for endpoint forms carrying userinfo,
    /// query, fragment, or control characters; and `invalid_provider_kind`
    /// for the unnormalized `openai` kind.
    pub fn validate(&self) -> DtoResult<()> {
        if self.normalized_effective_endpoint.contains(['?', '#', '@'])
            || self
                .normalized_effective_endpoint
                .chars()
                .any(char::is_control)
        {
            return Err(ErrorDto::validation(
                "invalid_endpoint",
                "endpoint is invalid",
            ));
        }
        let mut fields: Vec<&str> = vec![
            &self.selection_canonicalization_version,
            &self.profile_id,
            &self.provider_profile_revision_id,
            &self.kind_id,
            &self.kind_descriptor_revision_id,
            &self.model_id,
            &self.normalized_effective_endpoint,
            &self.resolved_reasoning_policy,
            &self.effective_execution_policy,
            &self.effective_loopback_policy_or_not_applicable,
            &self.provider_driver_contract_revision,
        ];
        if let Some(header) = &self.credential_transport_safe_header_name {
            fields.push(header);
        }
        if let Some(source) = &self.selection_source {
            fields.push(source);
        }
        fields.extend(
            self.declared_model_capability_subset
                .iter()
                .map(String::as_str),
        );
        if fields.iter().any(|value| credential_shaped(value)) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        if self.kind_id == "openai" {
            return Err(ErrorDto::validation(
                "invalid_provider_kind",
                "openai must be normalized to responses",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderProfileRevisionV1 {
    pub profile_id: String,
    pub revision_id: String,
    pub provider_kind_id: String,
    pub model_id: String,
    pub endpoint: String,
    pub credential_transport_mode: CredentialTransportMode,
    pub safe_header_name: Option<String>,
    pub capability_taxonomy_revision: String,
    pub reasoning_compatibility_id: Option<String>,
}
impl ProviderProfileRevisionV1 {
    /// Validates the credential-free profile revision fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_profile_revision_invalid` for blank, over-long, or
    /// control-bearing text fields or an invalid safe header name;
    /// `invalid_endpoint` for endpoint forms carrying userinfo, query, or
    /// fragment characters; and `credentials_forbidden` for credential-shaped
    /// values.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.profile_id,
            &self.revision_id,
            &self.provider_kind_id,
            &self.model_id,
            &self.endpoint,
            &self.capability_taxonomy_revision,
        ] {
            valid_text(field, 256, "provider_profile_revision_invalid")?;
        }
        if let Some(compatibility) = &self.reasoning_compatibility_id {
            valid_text(compatibility, 256, "provider_profile_revision_invalid")?;
        }
        if self.endpoint.contains(['?', '#', '@']) {
            return Err(ErrorDto::validation(
                "invalid_endpoint",
                "endpoint is invalid",
            ));
        }
        if let Some(header) = &self.safe_header_name {
            let header = header.trim();
            if header.is_empty()
                || header.chars().count() > 128
                || header.chars().any(char::is_control)
            {
                return Err(ErrorDto::validation(
                    "provider_profile_revision_invalid",
                    "safe header name is invalid",
                ));
            }
        }
        if [
            &self.profile_id,
            &self.revision_id,
            &self.provider_kind_id,
            &self.model_id,
            &self.endpoint,
            &self.capability_taxonomy_revision,
        ]
        .iter()
        .any(|value| credential_shaped(value))
            || self
                .reasoning_compatibility_id
                .as_deref()
                .is_some_and(credential_shaped)
            || self
                .safe_header_name
                .as_deref()
                .is_some_and(credential_shaped)
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ForkSessionCommandDto {
    pub source_session_id: String,
    pub boundary: ForkBoundary,
    pub expected_source_sequence: u64,
    pub expected_preview_digest: String,
    pub title_present: bool,
    pub requested_title: Option<String>,
    pub future_profile_override_present: bool,
    pub future_profile_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_profile_revision: Option<String>,
}
impl ForkSessionCommandDto {
    /// Validates the fork command's preview digest, optional title, and
    /// optional future provider override.
    ///
    /// # Errors
    ///
    /// Returns `invalid_digest` for a malformed preview digest,
    /// `invalid_title` for a blank, over-long, or control-bearing title, and
    /// `provider_profile_override_invalid` when the override flag is set
    /// without an override, an expected profile revision is supplied without
    /// an override, or an override value is blank, over-long, or
    /// control-bearing; `credentials_forbidden` for a credential-shaped
    /// override value.
    pub fn validate(&self) -> DtoResult<()> {
        digest(&self.expected_preview_digest)?;
        if let Some(title) = &self.requested_title {
            valid_text(title, 128, "invalid_title")?;
        }
        validate_profile_override_pair(
            self.future_profile_override_present,
            self.future_profile_override.as_deref(),
            self.expected_profile_revision.as_deref(),
        )
    }

    /// Binds the optional future provider override of the forked session.
    ///
    /// The builder keeps the existing all-fields constructor working while
    /// validating the override pair.
    ///
    /// # Errors
    ///
    /// Returns `provider_profile_override_invalid` when an expected profile
    /// revision is supplied without an override, or an override value is
    /// blank, over-long, or control-bearing, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn with_profile_override(
        mut self,
        profile_id: Option<String>,
        expected_profile_revision: Option<String>,
    ) -> DtoResult<Self> {
        validate_profile_override_pair(
            profile_id.is_some(),
            profile_id.as_deref(),
            expected_profile_revision.as_deref(),
        )?;
        self.future_profile_override_present = profile_id.is_some();
        self.future_profile_override = profile_id;
        self.expected_profile_revision = expected_profile_revision;
        Ok(self)
    }
}

/// Validates one optional future/expected provider override pair.
///
/// # Errors
///
/// Returns `provider_profile_override_invalid` when the presence flag is set
/// without an override value, an expected revision is supplied without an
/// override, or a value is blank, over-long, or control-bearing, and
/// `credentials_forbidden` for a credential-shaped value.
fn validate_profile_override_pair(
    present: bool,
    profile_id: Option<&str>,
    expected_profile_revision: Option<&str>,
) -> DtoResult<()> {
    if present && profile_id.is_none() {
        return Err(ErrorDto::validation(
            "provider_profile_override_invalid",
            "a provider override presence flag requires an override value",
        ));
    }
    if !present && profile_id.is_some() {
        return Err(ErrorDto::validation(
            "provider_profile_override_invalid",
            "a provider override value requires its presence flag",
        ));
    }
    if expected_profile_revision.is_some() && profile_id.is_none() {
        return Err(ErrorDto::validation(
            "provider_profile_override_invalid",
            "an expected profile revision requires a provider override",
        ));
    }
    for value in [profile_id, expected_profile_revision]
        .into_iter()
        .flatten()
    {
        if value.is_empty() || value.len() > 63 || value.chars().any(char::is_control) {
            return Err(ErrorDto::validation(
                "provider_profile_override_invalid",
                "provider profile override values are invalid",
            ));
        }
        if credential_shaped(value) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
    }
    Ok(())
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ForkSessionResultDto {
    pub session_id: String,
    pub preview_digest: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GetForkPreviewQueryDto {
    pub source_session_id: String,
    pub boundary: ForkBoundary,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ForkPreviewDto {
    pub preview_digest: String,
    pub page_count: u32,
    pub snapshot_size_bytes: u64,
}
impl ForkPreviewDto {
    /// Validates the frozen preview bounds: a 1 MiB snapshot and 64 pages.
    ///
    /// # Errors
    ///
    /// Returns `fork_snapshot_too_large` when the snapshot exceeds 1 MiB or
    /// the preview exceeds 64 pages.
    pub fn validate(&self) -> DtoResult<()> {
        if self.snapshot_size_bytes > 1024 * 1024 || self.page_count > 64 {
            return Err(ErrorDto::validation(
                "fork_snapshot_too_large",
                "fork preview exceeds its snapshot or page bound",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StartForkRunCommandDto {
    pub session_id: String,
    pub profile_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_profile_revision: Option<String>,
}
impl StartForkRunCommandDto {
    /// Validates the start-fork-run provider override pair.
    ///
    /// # Errors
    ///
    /// Returns `provider_profile_override_invalid` when an expected profile
    /// revision is supplied without an override, or an override value is
    /// blank, over-long, or control-bearing, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        validate_profile_override_pair(
            self.profile_override.is_some(),
            self.profile_override.as_deref(),
            self.expected_profile_revision.as_deref(),
        )
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GetConversationTreeQueryDto {
    pub session_id: String,
    pub page: u32,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConversationTreePageDto {
    pub children: Vec<ConversationBranchSummaryDto>,
    pub next_page: Option<u32>,
}
impl ConversationTreePageDto {
    /// Validates the bounded conversation tree page.
    ///
    /// # Errors
    ///
    /// Returns `invalid_conversation_tree_page` when the page exceeds
    /// sixty-four summaries.
    pub fn validate(&self) -> DtoResult<()> {
        bounded(self.children.clone(), 64, "invalid_conversation_tree_page").map(|_| ())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConversationBranchSummaryDto {
    pub session_id: String,
    pub title: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RenameSessionCommandDto {
    pub session_id: String,
    pub title: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArchiveSessionCommandDto {
    pub session_id: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RestoreSessionCommandDto {
    pub session_id: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForkBoundary {
    CommittedUserTurn {
        source_turn_id: String,
        accepted_sequence: u64,
    },
    CompletedAssistantTurn {
        source_run_id: String,
        final_assistant_turn_id: Option<String>,
        completed_sequence: u64,
        final_run_cursor: u64,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ForkBaseSnapshotV1 {
    pub schema_version: String,
    pub context_schema_version: String,
    pub source_session_id: String,
    pub conversation_tree_id: String,
    pub boundary: ForkBoundary,
    pub source_boundary_sequence: u64,
    pub source_run_cursors: Vec<u64>,
    pub effective_instruction_projection: String,
    pub materialized_model_messages: Vec<String>,
    pub inherited_future_defaults: Vec<String>,
    pub historical_config_policy_references: Vec<String>,
    pub safe_usage_provenance: Vec<String>,
    pub terminal_tool_result_references: Vec<String>,
    pub policy_decision_references: Vec<String>,
    pub terminal_child_result_references: Vec<String>,
    pub workspace_state: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ForkBaseSnapshotV2 {
    pub schema_version: String,
    pub context_schema_version: String,
    pub source_session_id: String,
    pub conversation_tree_id: String,
    pub boundary: ForkBoundary,
    pub source_boundary_sequence: u64,
    pub source_run_cursors: Vec<u64>,
    pub effective_instruction_projection: String,
    pub materialized_model_messages: Vec<String>,
    pub inherited_future_defaults: Vec<String>,
    pub historical_config_policy_references: Vec<String>,
    pub inherited_reasoning_history_references: Vec<String>,
    pub safe_usage_provenance: Vec<String>,
    pub terminal_tool_result_references: Vec<String>,
    pub policy_decision_references: Vec<String>,
    pub terminal_child_result_references: Vec<String>,
    pub workspace_state: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ForkPreviewV1 {
    pub preview_digest: String,
    pub source_head_sequence: u64,
    pub page_count: u32,
    pub snapshot_size_bytes: u64,
    pub workspace_state: String,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ForkPreviewV2 {
    pub preview_digest: String,
    pub source_head_sequence: u64,
    pub page_count: u32,
    pub snapshot_size_bytes: u64,
    pub inherited_reasoning_history_references: Vec<String>,
    pub workspace_state: String,
}

fn validate_fork_base_snapshot_common(
    materialized_model_messages: &[String],
    workspace_state: &str,
) -> DtoResult<()> {
    if materialized_model_messages.len() > 1024
        || materialized_model_messages
            .iter()
            .map(String::len)
            .sum::<usize>()
            > 1024 * 1024
    {
        return Err(ErrorDto::validation(
            "fork_snapshot_too_large",
            "fork base snapshot exceeds its 1 MiB bound",
        ));
    }
    if workspace_state != "unverified" {
        return Err(ErrorDto::validation(
            "fork_snapshot_unsupported",
            "workspace state must be unverified",
        ));
    }
    Ok(())
}
fn validate_fork_preview_common(
    preview_digest: &str,
    page_count: u32,
    snapshot_size_bytes: u64,
    workspace_state: &str,
) -> DtoResult<()> {
    digest(preview_digest)?;
    if page_count > 64 || snapshot_size_bytes > 1024 * 1024 {
        return Err(ErrorDto::validation(
            "fork_snapshot_too_large",
            "fork preview exceeds its snapshot or page bound",
        ));
    }
    if workspace_state != "unverified" {
        return Err(ErrorDto::validation(
            "fork_snapshot_unsupported",
            "workspace state must be unverified",
        ));
    }
    Ok(())
}
fn validate_reasoning_references(references: &[String]) -> DtoResult<()> {
    if references.len() > 4096 {
        return Err(ErrorDto::validation(
            "fork_snapshot_too_large",
            "inherited reasoning references exceed the 1 MiB snapshot bound",
        ));
    }
    for reference in references {
        if reference.trim().is_empty() || reference.chars().any(char::is_control) {
            return Err(ErrorDto::validation(
                "fork_reference_unavailable",
                "inherited reasoning reference is malformed",
            ));
        }
    }
    if references.iter().map(String::len).sum::<usize>() > 1024 * 1024 {
        return Err(ErrorDto::validation(
            "fork_snapshot_too_large",
            "inherited reasoning references exceed the 1 MiB snapshot bound",
        ));
    }
    Ok(())
}

impl ForkBaseSnapshotV1 {
    /// Validates the frozen v1 base-snapshot bounds.
    ///
    /// # Errors
    ///
    /// Returns `fork_snapshot_too_large` when the materialized messages
    /// exceed 1,024 entries or 1 MiB aggregate and
    /// `fork_snapshot_unsupported` when the workspace state is not the closed
    /// `unverified` value.
    pub fn validate(&self) -> DtoResult<()> {
        validate_fork_base_snapshot_common(&self.materialized_model_messages, &self.workspace_state)
    }
}
impl ForkBaseSnapshotV2 {
    /// Validates the frozen v2 base-snapshot bounds, including the inherited
    /// reasoning references.
    ///
    /// # Errors
    ///
    /// Returns `fork_snapshot_too_large` for message or reference overflow,
    /// `fork_snapshot_unsupported` for a non-`unverified` workspace state, and
    /// `fork_reference_unavailable` for a malformed reasoning reference.
    pub fn validate(&self) -> DtoResult<()> {
        validate_fork_base_snapshot_common(
            &self.materialized_model_messages,
            &self.workspace_state,
        )?;
        validate_reasoning_references(&self.inherited_reasoning_history_references)
    }
}
impl ForkPreviewV1 {
    /// Validates the frozen v1 preview bounds.
    ///
    /// # Errors
    ///
    /// Returns `invalid_digest` for a malformed preview digest,
    /// `fork_snapshot_too_large` for a snapshot above 1 MiB or a preview
    /// above 64 pages, and `fork_snapshot_unsupported` for a non-`unverified`
    /// workspace state.
    pub fn validate(&self) -> DtoResult<()> {
        validate_fork_preview_common(
            &self.preview_digest,
            self.page_count,
            self.snapshot_size_bytes,
            &self.workspace_state,
        )
    }
}
impl ForkPreviewV2 {
    /// Validates the frozen v2 preview bounds, including the inherited
    /// reasoning references.
    ///
    /// # Errors
    ///
    /// Returns `invalid_digest` for a malformed preview digest,
    /// `fork_snapshot_too_large` for snapshot, page, or reference overflow,
    /// `fork_snapshot_unsupported` for a non-`unverified` workspace state, and
    /// `fork_reference_unavailable` for a malformed reasoning reference.
    pub fn validate(&self) -> DtoResult<()> {
        validate_fork_preview_common(
            &self.preview_digest,
            self.page_count,
            self.snapshot_size_bytes,
            &self.workspace_state,
        )?;
        validate_reasoning_references(&self.inherited_reasoning_history_references)
    }
}

impl TryFrom<ForkPreviewV1> for ForkPreviewDto {
    type Error = ErrorDto;

    /// Bridges a frozen v1 preview to the legacy summary DTO without loss.
    ///
    /// The bridge succeeds only when every dropped field is at its default or
    /// absent value: a zero `source_head_sequence` and the closed
    /// `unverified` workspace state.
    ///
    /// # Errors
    ///
    /// Returns `fork_preview_conversion_lossy` when the source carries a
    /// non-default dropped field.
    fn try_from(value: ForkPreviewV1) -> Result<Self, Self::Error> {
        if value.workspace_state != "unverified" || value.source_head_sequence != 0 {
            return Err(ErrorDto::validation(
                "fork_preview_conversion_lossy",
                "fork preview conversion would drop source fields",
            ));
        }
        Ok(Self {
            preview_digest: value.preview_digest,
            page_count: value.page_count,
            snapshot_size_bytes: value.snapshot_size_bytes,
        })
    }
}
impl TryFrom<ForkPreviewV2> for ForkPreviewDto {
    type Error = ErrorDto;

    /// Bridges a frozen v2 preview to the legacy summary DTO without loss.
    ///
    /// The bridge succeeds only when every dropped field is at its default or
    /// absent value: a zero `source_head_sequence`, the closed `unverified`
    /// workspace state, and no inherited reasoning history references.
    ///
    /// # Errors
    ///
    /// Returns `fork_preview_conversion_lossy` when the source carries a
    /// non-default dropped field.
    fn try_from(value: ForkPreviewV2) -> Result<Self, Self::Error> {
        if value.workspace_state != "unverified"
            || value.source_head_sequence != 0
            || !value.inherited_reasoning_history_references.is_empty()
        {
            return Err(ErrorDto::validation(
                "fork_preview_conversion_lossy",
                "fork preview conversion would drop source fields",
            ));
        }
        Ok(Self {
            preview_digest: value.preview_digest,
            page_count: value.page_count,
            snapshot_size_bytes: value.snapshot_size_bytes,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningFactCategory {
    Primary,
    Detail,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunReasoningHistoryPageDto {
    pub session_id: String,
    pub run_id: String,
    pub captured_upper_cursor: u64,
    pub facts: Vec<String>,
}
impl RunReasoningHistoryPageDto {
    /// Validates the bounded reasoning history page.
    ///
    /// # Errors
    ///
    /// Returns `provider_reasoning_stream_invalid` when the page is empty or
    /// exceeds two hundred fifty-six facts.
    pub fn validate(&self) -> DtoResult<()> {
        if self.facts.is_empty() || self.facts.len() > 256 {
            Err(ErrorDto::validation(
                "provider_reasoning_stream_invalid",
                "invalid reasoning page",
            ))
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunReasoningHistoryCompletedDto {
    pub run_id: String,
    pub final_cursor: u64,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReasoningHistoryManifestDto {
    pub compatibility_id: String,
    pub entries: Vec<String>,
    pub manifest_digest: String,
}
impl ReasoningHistoryManifestDto {
    /// Validates the bounded reasoning-history manifest.
    ///
    /// # Errors
    ///
    /// Returns `reasoning_history_manifest_invalid` for a blank, over-long, or
    /// control-bearing compatibility ID, a manifest with more than 256
    /// entries, or a blank or control-bearing entry, and `invalid_digest` for
    /// a malformed manifest digest.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(
            &self.compatibility_id,
            256,
            "reasoning_history_manifest_invalid",
        )?;
        if self.entries.len() > 256 {
            return Err(ErrorDto::validation(
                "reasoning_history_manifest_invalid",
                "reasoning history manifest exceeds its 256-entry bound",
            ));
        }
        for entry in &self.entries {
            if entry.trim().is_empty() || entry.chars().any(char::is_control) {
                return Err(ErrorDto::validation(
                    "reasoning_history_manifest_invalid",
                    "reasoning history manifest entry is invalid",
                ));
            }
        }
        digest(&self.manifest_digest)?;
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningHistoryTransferDto {
    Disabled,
    TextualHistoryV1 { compatibility_id: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContextSourceEntryV1 {
    pub source_id: String,
    pub source_kind: String,
    pub revision: String,
    pub safe_label: Option<String>,
}
impl ContextSourceEntryV1 {
    /// Validates the bounded context-source entry fields.
    ///
    /// # Errors
    ///
    /// Returns `context_source_manifest_invalid` for a blank, over-long, or
    /// control-bearing entry field or safe label.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [&self.source_id, &self.source_kind, &self.revision] {
            valid_text(field, 256, "context_source_manifest_invalid")?;
        }
        if let Some(label) = &self.safe_label {
            valid_text(label, 256, "context_source_manifest_invalid")?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContextSourceManifestV1 {
    pub compatibility_id: String,
    pub source_entries: Vec<ContextSourceEntryV1>,
    pub manifest_digest: String,
}
impl ContextSourceManifestV1 {
    /// Validates the bounded context-source manifest.
    ///
    /// # Errors
    ///
    /// Returns `context_source_manifest_invalid` when the manifest carries no
    /// entries or more than 256 entries, or when the compatibility ID or any
    /// entry field is blank, over-long, or control-bearing, and
    /// `invalid_digest` for a malformed manifest digest.
    pub fn validate(&self) -> DtoResult<()> {
        if self.source_entries.is_empty() || self.source_entries.len() > 256 {
            return Err(ErrorDto::validation(
                "context_source_manifest_invalid",
                "context source manifest must carry between 1 and 256 entries",
            ));
        }
        valid_text(
            &self.compatibility_id,
            256,
            "context_source_manifest_invalid",
        )?;
        for entry in &self.source_entries {
            entry.validate()?;
        }
        digest(&self.manifest_digest)?;
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelContextProjectionV1 {
    pub projection_revision: String,
    pub context_schema_version: String,
    pub source_manifest_digest: String,
    pub ordered_messages: Vec<String>,
    pub model_context_digest: String,
}
impl ModelContextProjectionV1 {
    /// Validates the bounded model-context projection.
    ///
    /// # Errors
    ///
    /// Returns `model_context_projection_invalid` when the ordered messages
    /// are empty, contain a blank entry, or exceed 1,024 entries;
    /// `model_context_projection_too_large` when they exceed 1 MiB aggregate;
    /// and `invalid_digest` for a malformed source-manifest or model-context
    /// digest.
    pub fn validate(&self) -> DtoResult<()> {
        if self.ordered_messages.is_empty()
            || self.ordered_messages.len() > 1024
            || self
                .ordered_messages
                .iter()
                .any(|message| message.trim().is_empty())
        {
            return Err(ErrorDto::validation(
                "model_context_projection_invalid",
                "ordered messages must be nonblank and between 1 and 1,024 entries",
            ));
        }
        if self.ordered_messages.iter().map(String::len).sum::<usize>() > 1024 * 1024 {
            return Err(ErrorDto::validation(
                "model_context_projection_too_large",
                "model context projection exceeds its 1 MiB bound",
            ));
        }
        digest(&self.source_manifest_digest)?;
        digest(&self.model_context_digest)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentActivityTreeId(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentActivityLimitsV1 {
    pub max_messages: u32,
    pub max_aggregate_bytes: u64,
    pub max_journal_records: u32,
    pub max_record_bytes: u32,
    pub max_page_records: u32,
    pub max_page_bytes: u64,
    pub max_references: u32,
}
impl AgentActivityLimitsV1 {
    /// Validates that the limits match the frozen ledger values.
    ///
    /// # Errors
    ///
    /// Returns `agent_activity_limits_invalid` unless every limit is exactly
    /// the frozen value: 1,024 messages, 4 MiB aggregate, 4,096 journal
    /// records, 64 KiB per record, 256 records and 512 KiB per page, and 16
    /// references.
    pub fn validate(&self) -> DtoResult<()> {
        if self.max_messages != 1024
            || self.max_aggregate_bytes != 4 * 1024 * 1024
            || self.max_journal_records != 4096
            || self.max_record_bytes != 64 * 1024
            || self.max_page_records != 256
            || self.max_page_bytes != 512 * 1024
            || self.max_references != 16
        {
            return Err(ErrorDto::validation(
                "agent_activity_limits_invalid",
                "activity limits must match the frozen ledger values",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentActivityTreeV1 {
    pub activity_tree_id: AgentActivityTreeId,
    pub root_run_reference: String,
    pub activity_exchange_revision: String,
    pub activity_journal_revision: String,
    pub user_projection_revision: String,
    pub fixed_limits: AgentActivityLimitsV1,
}
impl AgentActivityTreeV1 {
    /// Validates the tree's safe text fields and frozen limits.
    ///
    /// # Errors
    ///
    /// Returns `agent_activity_tree_invalid` for blank, over-long, or
    /// control-bearing text fields and `agent_activity_limits_invalid` when
    /// the fixed limits depart from the frozen ledger values.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.activity_tree_id.0,
            &self.root_run_reference,
            &self.activity_exchange_revision,
            &self.activity_journal_revision,
            &self.user_projection_revision,
        ] {
            valid_text(field, 256, "agent_activity_tree_invalid")?;
        }
        self.fixed_limits.validate()
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentActivityPairV1 {
    pub pair_id: String,
    pub activity_tree_id: AgentActivityTreeId,
    pub parent_run_reference: String,
    pub child_run_reference: String,
    pub activity_exchange_revision: String,
    pub activity_journal_revision: String,
    pub user_projection_revision: String,
    pub fixed_limits: AgentActivityLimitsV1,
}
impl AgentActivityPairV1 {
    /// Validates the pair's distinct run references, safe text fields, and
    /// frozen limits.
    ///
    /// # Errors
    ///
    /// Returns `agent_activity_pair_invalid` when the parent and child run
    /// references are equal or a text field is blank, over-long, or
    /// control-bearing, and `agent_activity_limits_invalid` when the fixed
    /// limits depart from the frozen ledger values.
    pub fn validate(&self) -> DtoResult<()> {
        if self.parent_run_reference == self.child_run_reference {
            return Err(ErrorDto::validation(
                "agent_activity_pair_invalid",
                "parent and child run references must differ",
            ));
        }
        for field in [
            &self.pair_id,
            &self.activity_tree_id.0,
            &self.parent_run_reference,
            &self.child_run_reference,
            &self.activity_exchange_revision,
            &self.activity_journal_revision,
            &self.user_projection_revision,
        ] {
            valid_text(field, 256, "agent_activity_pair_invalid")?;
        }
        self.fixed_limits.validate()
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMessageDirection {
    ParentToChild,
    ChildToParent,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMessageKind {
    Instruction,
    Report,
    ClarificationRequest,
    ClarificationReply,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentMessageDto {
    pub message_id: String,
    pub activity_tree_id: AgentActivityTreeId,
    pub pair_id: String,
    pub pair_order: u32,
    pub direction: AgentMessageDirection,
    pub kind: AgentMessageKind,
    pub sender_run_reference: String,
    pub recipient_run_reference: String,
    pub source_model_step_reference: String,
    pub safe_text: Option<String>,
    pub typed_references: Vec<String>,
    pub delivery_state: String,
    pub canonical_message_digest: String,
}
impl AgentMessageDto {
    /// Validates the bounded message fields and digest.
    ///
    /// # Errors
    ///
    /// Returns `agent_message_invalid` when a required text field or the
    /// optional safe text is blank, over-long, or control-bearing,
    /// `agent_message_reference_invalid` when the message carries more than
    /// sixteen typed references, and `invalid_digest` for a malformed
    /// canonical message digest.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.message_id,
            &self.activity_tree_id.0,
            &self.pair_id,
            &self.sender_run_reference,
            &self.recipient_run_reference,
            &self.source_model_step_reference,
            &self.delivery_state,
        ] {
            valid_text(field, 256, "agent_message_invalid")?;
        }
        if let Some(safe_text) = &self.safe_text {
            valid_text(safe_text, 64 * 1024, "agent_message_invalid")?;
        }
        bounded(
            self.typed_references.clone(),
            16,
            "agent_message_reference_invalid",
        )?;
        digest(&self.canonical_message_digest)?;
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentActivityJournalRecordDto {
    pub activity_tree_id: AgentActivityTreeId,
    pub record_id: String,
    pub sequence: u64,
    pub occurred_at: u64,
    pub root_run_reference: String,
    pub direct_pair_reference_when_present: Option<String>,
    pub record_kind: String,
    pub safe_user_projection: String,
    pub typed_references: Vec<String>,
    pub canonical_record_digest: String,
}
impl AgentActivityJournalRecordDto {
    /// Validates the bounded activity journal record.
    ///
    /// The accounting unit is the UTF-8 byte length of every payload-bearing
    /// field, including the typed-reference contents.
    ///
    /// # Errors
    ///
    /// Returns `agent_activity_record_too_large` when the string payload,
    /// including the typed-reference contents, exceeds the 64 KiB record
    /// bound, `agent_message_reference_invalid` when the record carries more
    /// than sixteen typed references, and
    /// `agent_activity_journal_limit_exceeded` when the zero-based sequence
    /// is at or beyond the 4,096-record journal bound.
    pub fn validate(&self) -> DtoResult<()> {
        let payload_bytes = self.activity_tree_id.0.len()
            + self.record_id.len()
            + self.root_run_reference.len()
            + self
                .direct_pair_reference_when_present
                .as_deref()
                .map_or(0, str::len)
            + self.record_kind.len()
            + self.safe_user_projection.len()
            + self.canonical_record_digest.len()
            + self.typed_references.iter().map(String::len).sum::<usize>();
        if payload_bytes > 64 * 1024 {
            return Err(ErrorDto::validation(
                "agent_activity_record_too_large",
                "activity journal record exceeds the 64 KiB record bound",
            ));
        }
        bounded(
            self.typed_references.clone(),
            16,
            "agent_message_reference_invalid",
        )?;
        if self.sequence >= 4096 {
            return Err(ErrorDto::validation(
                "agent_activity_journal_limit_exceeded",
                "activity journal exceeds its 4,096-record bound",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentNotificationCursorDto(pub u64);
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentNotificationSummaryDto {
    pub activity_tree_id: AgentActivityTreeId,
    pub safe_summary: String,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationLevel {
    Urgent,
    Ordinary,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentNotificationRecordDto {
    pub notification_cursor: AgentNotificationCursorDto,
    pub activity_tree_id: AgentActivityTreeId,
    pub activity_record_reference: String,
    pub level: NotificationLevel,
    pub reason: String,
    pub safe_counts_and_states: String,
    pub occurred_at: u64,
}
impl AgentNotificationRecordDto {
    /// Validates the frozen notification aggregate byte budget.
    ///
    /// # Errors
    ///
    /// Returns `agent_notification_record_invalid` for a blank, over-long, or
    /// control-bearing text field, and
    /// `agent_notification_summary_too_large` when the record's text payload
    /// exceeds the 64 KiB per-page bound.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [&self.activity_tree_id.0, &self.activity_record_reference] {
            valid_text(field, 256, "agent_notification_record_invalid")?;
        }
        for field in [&self.reason, &self.safe_counts_and_states] {
            valid_text(field, 64 * 1024, "agent_notification_record_invalid")?;
        }
        let payload_bytes = self.activity_record_reference.len()
            + self.reason.len()
            + self.safe_counts_and_states.len();
        if payload_bytes > 64 * 1024 {
            return Err(ErrorDto::validation(
                "agent_notification_summary_too_large",
                "notification summary exceeds the 64 KiB page bound",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BridgeRunGrantDto {
    pub opaque_grant_identity: String,
    pub issued_protocol_revision: String,
}
impl BridgeRunGrantDto {
    /// Validates the bounded, credential-free grant fields.
    ///
    /// # Errors
    ///
    /// Returns `bridge_invocation_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [&self.opaque_grant_identity, &self.issued_protocol_revision] {
            valid_text(field, 256, "bridge_invocation_invalid")?;
        }
        if [&self.opaque_grant_identity, &self.issued_protocol_revision]
            .iter()
            .any(|value| credential_shaped(value))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BridgeAttachmentResponseDto {
    pub bridge_run_grant: BridgeRunGrantDto,
    pub negotiated_capabilities: Vec<crate::ProtocolCapabilityDto>,
    pub initial_run_cursor: u64,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BridgeInvocationCommandDto {
    pub bridge_run_grant: BridgeRunGrantDto,
    pub bridge_operation_id: String,
    pub typed_tool_invocation: String,
}
impl BridgeInvocationCommandDto {
    /// Validates the bounded invocation command.
    ///
    /// # Errors
    ///
    /// Returns `bridge_invocation_invalid` for a blank, over-long, or
    /// control-bearing command field or grant field and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        self.bridge_run_grant.validate()?;
        for field in [&self.bridge_operation_id, &self.typed_tool_invocation] {
            valid_text(field, 256, "bridge_invocation_invalid")?;
        }
        if [&self.bridge_operation_id, &self.typed_tool_invocation]
            .iter()
            .any(|value| credential_shaped(value))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BridgeInvocationAcceptedDto {
    pub bridge_operation_id: String,
    pub tool_call_id: String,
    pub admission_state: String,
}
impl BridgeInvocationAcceptedDto {
    /// Validates the bounded acceptance fields.
    ///
    /// # Errors
    ///
    /// Returns `bridge_invocation_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.bridge_operation_id,
            &self.tool_call_id,
            &self.admission_state,
        ] {
            valid_text(field, 256, "bridge_invocation_invalid")?;
        }
        if [
            &self.bridge_operation_id,
            &self.tool_call_id,
            &self.admission_state,
        ]
        .iter()
        .any(|value| credential_shaped(value))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BridgeOperationV1 {
    pub bridge_operation_id: String,
    pub run_id: String,
    pub mandate_id: String,
    pub mandate_revision: String,
    pub model_step_id: String,
    pub tool_id: String,
    pub descriptor_revision: String,
    pub typed_input_digest: String,
    pub tool_call_id: String,
    pub admission_outcome: String,
    pub attempt_reference: Option<String>,
}
impl BridgeOperationV1 {
    /// Validates the bounded bridge operation.
    ///
    /// # Errors
    ///
    /// Returns `bridge_invocation_invalid` for a blank, over-long, or
    /// control-bearing field, `invalid_digest` for a malformed typed-input
    /// digest, and `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        digest(&self.typed_input_digest)?;
        for field in [
            &self.bridge_operation_id,
            &self.run_id,
            &self.mandate_id,
            &self.mandate_revision,
            &self.model_step_id,
            &self.tool_id,
            &self.descriptor_revision,
            &self.tool_call_id,
            &self.admission_outcome,
        ] {
            valid_text(field, 256, "bridge_invocation_invalid")?;
        }
        if let Some(attempt) = &self.attempt_reference {
            valid_text(attempt, 256, "bridge_invocation_invalid")?;
        }
        let mut fields = vec![
            &self.bridge_operation_id,
            &self.run_id,
            &self.mandate_id,
            &self.mandate_revision,
            &self.model_step_id,
            &self.tool_id,
            &self.descriptor_revision,
            &self.typed_input_digest,
            &self.tool_call_id,
            &self.admission_outcome,
        ];
        if let Some(attempt) = &self.attempt_reference {
            fields.push(attempt);
        }
        if fields.iter().any(|value| credential_shaped(value)) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunToolHistoryPageDto {
    pub session_id: String,
    pub run_id: String,
    pub captured_upper_cursor: u64,
    pub facts: Vec<String>,
}
impl RunToolHistoryPageDto {
    /// Validates the bounded tool history page.
    ///
    /// # Errors
    ///
    /// Returns `tool_result_stream_invalid` when the page is empty or exceeds
    /// two hundred fifty-six facts.
    pub fn validate(&self) -> DtoResult<()> {
        if self.facts.is_empty() || self.facts.len() > 256 {
            Err(ErrorDto::validation(
                "tool_result_stream_invalid",
                "invalid tool history page",
            ))
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunToolHistoryCompletedDto {
    pub run_id: String,
    pub final_cursor: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelToolLoopV1 {
    pub tool_registry_revision_id: String,
    pub admission_engine_revision: String,
    pub hook_pipeline_revision: String,
    pub active_descriptors: Vec<ActiveToolDescriptorSelectionV1>,
    pub model_tool_loop_required: bool,
    pub translation_revision: String,
    pub stream_shape: String,
}
impl ModelToolLoopV1 {
    /// Validates the bounded model-tool-loop shape.
    ///
    /// The loop may name at most sixteen active descriptors, matching the
    /// closed sixteen-call-per-group rule, and each descriptor field is
    /// bounded like a tool-descriptor revision.
    ///
    /// # Errors
    ///
    /// Returns `model_tool_loop_invalid` for a blank, over-long, or
    /// control-bearing loop field, more than sixteen active descriptors, or
    /// an invalid active descriptor, and `credentials_forbidden` for a
    /// credential-shaped loop field.
    pub fn validate(&self) -> DtoResult<()> {
        let fields = [
            &self.tool_registry_revision_id,
            &self.admission_engine_revision,
            &self.hook_pipeline_revision,
            &self.translation_revision,
            &self.stream_shape,
        ];
        for field in fields {
            valid_text(field, 256, "model_tool_loop_invalid")?;
        }
        if fields.iter().any(|value| credential_shaped(value)) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        if self.active_descriptors.len() > 16 {
            return Err(ErrorDto::validation(
                "model_tool_loop_invalid",
                "model tool loop exceeds its 16-active-descriptor bound",
            ));
        }
        for descriptor in &self.active_descriptors {
            descriptor.validate()?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ActiveToolDescriptorSelectionV1 {
    pub tool_id: String,
    pub intended_owner: String,
    pub descriptor_revision: String,
    pub input_schema_reference: String,
    pub result_schema_reference: String,
    pub required_capability_binding: String,
    pub mode_relation: String,
    pub model_function_schema_revision: String,
    pub safe_result_projection_revision: String,
    pub observation_contract_revision: String,
    pub stream_shape: String,
}
impl ActiveToolDescriptorSelectionV1 {
    /// Validates the credential-free active-descriptor fields.
    ///
    /// # Errors
    ///
    /// Returns `model_tool_loop_invalid` for blank, over-long, or
    /// control-bearing fields and `credentials_forbidden` for
    /// credential-shaped values.
    pub fn validate(&self) -> DtoResult<()> {
        let fields = [
            &self.tool_id,
            &self.intended_owner,
            &self.descriptor_revision,
            &self.input_schema_reference,
            &self.result_schema_reference,
            &self.required_capability_binding,
            &self.mode_relation,
            &self.model_function_schema_revision,
            &self.safe_result_projection_revision,
            &self.observation_contract_revision,
            &self.stream_shape,
        ];
        for field in fields {
            valid_text(field, 256, "model_tool_loop_invalid")?;
        }
        if fields.iter().any(|value| credential_shaped(value)) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolDescriptorRevision {
    pub tool_id: String,
    pub descriptor_revision: String,
    pub intended_owner: String,
    pub input_schema_reference: String,
    pub result_schema_reference: String,
    pub required_capability_binding: String,
    pub mode_relation: String,
    pub model_function_schema_revision: String,
    pub safe_result_projection_revision: String,
    pub observation_contract_revision: String,
    pub stream_shape: String,
}
impl ToolDescriptorRevision {
    /// Validates the credential-free descriptor fields.
    ///
    /// # Errors
    ///
    /// Returns `tool_descriptor_revision_invalid` for blank, over-long, or
    /// control-bearing fields and `credentials_forbidden` for
    /// credential-shaped values.
    pub fn validate(&self) -> DtoResult<()> {
        let fields = [
            &self.tool_id,
            &self.descriptor_revision,
            &self.intended_owner,
            &self.input_schema_reference,
            &self.result_schema_reference,
            &self.required_capability_binding,
            &self.mode_relation,
            &self.model_function_schema_revision,
            &self.safe_result_projection_revision,
            &self.observation_contract_revision,
            &self.stream_shape,
        ];
        for field in fields {
            valid_text(field, 256, "tool_descriptor_revision_invalid")?;
        }
        if fields.iter().any(|value| credential_shaped(value)) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolRegistryRevision {
    pub registry_revision_id: String,
    pub descriptors: Vec<ToolDescriptorRevision>,
    pub admission_engine_revision: String,
    pub hook_pipeline_revision: String,
}
impl ToolRegistryRevision {
    /// Validates the bounded registry revision.
    ///
    /// # Errors
    ///
    /// Returns `tool_registry_revision_invalid` for a blank, over-long, or
    /// control-bearing registry field and `credentials_forbidden` for a
    /// credential-shaped value; `tool_registry_revision_too_large` when the
    /// revision carries more than 256 descriptors or the descriptor fields
    /// exceed 512 KiB aggregate; `duplicate_tool_descriptor` for a repeated
    /// tool ID; and the per-descriptor failures for any invalid descriptor.
    pub fn validate(&self) -> DtoResult<()> {
        let fields = [
            &self.registry_revision_id,
            &self.admission_engine_revision,
            &self.hook_pipeline_revision,
        ];
        for field in fields {
            valid_text(field, 256, "tool_registry_revision_invalid")?;
        }
        if fields.iter().any(|value| credential_shaped(value)) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        if self.descriptors.len() > 256 {
            return Err(ErrorDto::validation(
                "tool_registry_revision_too_large",
                "tool registry revision exceeds its 256-descriptor bound",
            ));
        }
        let mut seen_tool_ids: Vec<&str> = Vec::new();
        let mut aggregate_bytes = 0usize;
        for descriptor in &self.descriptors {
            descriptor.validate()?;
            if seen_tool_ids.contains(&descriptor.tool_id.as_str()) {
                return Err(ErrorDto::validation(
                    "duplicate_tool_descriptor",
                    "duplicate tool descriptor",
                ));
            }
            seen_tool_ids.push(&descriptor.tool_id);
            aggregate_bytes += descriptor.tool_id.len()
                + descriptor.descriptor_revision.len()
                + descriptor.intended_owner.len()
                + descriptor.input_schema_reference.len()
                + descriptor.result_schema_reference.len()
                + descriptor.required_capability_binding.len()
                + descriptor.mode_relation.len()
                + descriptor.model_function_schema_revision.len()
                + descriptor.safe_result_projection_revision.len()
                + descriptor.observation_contract_revision.len()
                + descriptor.stream_shape.len();
        }
        if aggregate_bytes > 512 * 1024 {
            return Err(ErrorDto::validation(
                "tool_registry_revision_too_large",
                "tool registry revision exceeds its 512 KiB bound",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelToolExchangeDto {
    pub assistant_ordered_calls: Vec<String>,
    pub canonical_call_identities: Vec<String>,
    pub completed_result_records: Vec<String>,
    pub safe_model_visible_projections: Vec<String>,
}
impl ModelToolExchangeDto {
    /// Validates the bounded tool exchange.
    ///
    /// # Errors
    ///
    /// Returns `provider_tool_group_invalid` when the exchange orders more
    /// than sixteen assistant calls.
    pub fn validate(&self) -> DtoResult<()> {
        bounded(
            self.assistant_ordered_calls.clone(),
            16,
            "provider_tool_group_invalid",
        )
        .map(|_| ())
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTerminalOutcome {
    Succeeded,
    DeniedBeforeExecution,
    FailedBeforeExternalEffect,
    CancelledBeforeStart,
    InterruptedBeforeStart,
    OutputLimitExceeded,
    ExecutionUnavailable,
    ExternalEffectUnknown,
}

// ---------------------------------------------------------------------------
// Slice 2 control-plane protocol surface.
//
// Every type in this section is additive and gated behind the negotiated
// `provider_profiles_v1` capability: no existing M3/M4/M5 DTO or wire shape
// changes, and no field here carries credential material.
// ---------------------------------------------------------------------------

/// The closed readiness state of one provider catalog entry.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderReadinessDto {
    /// The provider is ready to serve requests.
    Ready,
    /// The provider is disabled and must not be selected.
    Disabled,
    /// The provider is currently unavailable.
    Unavailable,
}

/// A credential-free, pageable provider catalog query.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GetProviderCatalogQueryDto {
    pub schema_version: String,
    pub page_token: Option<String>,
    pub expected_catalog_revision_id: Option<String>,
}
impl GetProviderCatalogQueryDto {
    /// Validates the bounded, credential-free catalog query fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_invalid` for a blank, over-long, or
    /// control-bearing schema version or catalog revision reference,
    /// `invalid_page_token` for a blank, over-long, or control-bearing page
    /// token, and `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(&self.schema_version, 64, "provider_catalog_invalid")?;
        if let Some(revision) = &self.expected_catalog_revision_id {
            valid_text(revision, 256, "provider_catalog_invalid")?;
        }
        if let Some(token) = &self.page_token {
            valid_text(token, 1024, "invalid_page_token")?;
        }
        if credential_shaped(&self.schema_version)
            || self
                .expected_catalog_revision_id
                .as_deref()
                .is_some_and(credential_shaped)
            || self.page_token.as_deref().is_some_and(credential_shaped)
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// One credential-free provider catalog entry.
///
/// The entry names where credentials are transported and whether they are
/// configured; it never carries credential material, raw payloads, or paths.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogEntryDto {
    pub profile_id: String,
    pub profile_revision_id: String,
    pub display_name: String,
    pub enabled: bool,
    pub provider_kind_id: String,
    pub kind_descriptor_revision_id: String,
    pub model_id: String,
    pub normalized_endpoint: Option<String>,
    pub effective_execution_policy: String,
    pub capability_subset: Vec<String>,
    pub credential_transport_mode: CredentialTransportMode,
    pub credential_transport_safe_header_name: Option<String>,
    pub credential_configured: bool,
    pub driver_declared_capabilities: Vec<String>,
    pub readiness: ProviderReadinessDto,
}
impl ProviderCatalogEntryDto {
    /// Validates the bounded, credential-free catalog entry fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_entry_invalid` for a blank, over-long, or
    /// control-bearing text field or capability entry, or a page exceeding
    /// its 256-capability bound, `invalid_endpoint` for an endpoint carrying
    /// userinfo, query, fragment, or control characters, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.profile_id,
            &self.profile_revision_id,
            &self.display_name,
            &self.provider_kind_id,
            &self.kind_descriptor_revision_id,
            &self.model_id,
            &self.effective_execution_policy,
        ] {
            valid_text(field, 256, "provider_catalog_entry_invalid")?;
        }
        if let Some(endpoint) = &self.normalized_endpoint
            && (endpoint.contains(['?', '#', '@']) || endpoint.chars().any(char::is_control))
        {
            return Err(ErrorDto::validation(
                "invalid_endpoint",
                "endpoint is invalid",
            ));
        }
        if let Some(header) = &self.credential_transport_safe_header_name {
            let header = header.trim();
            if header.is_empty()
                || header.chars().count() > 128
                || header.chars().any(char::is_control)
            {
                return Err(ErrorDto::validation(
                    "provider_catalog_entry_invalid",
                    "safe header name is invalid",
                ));
            }
        }
        for list in [&self.capability_subset, &self.driver_declared_capabilities] {
            if list.len() > 256 {
                return Err(ErrorDto::validation(
                    "provider_catalog_entry_invalid",
                    "provider catalog entry exceeds its 256-capability bound",
                ));
            }
            for capability in list {
                valid_text(capability, 256, "provider_catalog_entry_invalid")?;
            }
        }
        let mut fields: Vec<&str> = vec![
            &self.profile_id,
            &self.profile_revision_id,
            &self.display_name,
            &self.provider_kind_id,
            &self.kind_descriptor_revision_id,
            &self.model_id,
            &self.effective_execution_policy,
        ];
        if let Some(endpoint) = &self.normalized_endpoint {
            fields.push(endpoint);
        }
        if let Some(header) = &self.credential_transport_safe_header_name {
            fields.push(header);
        }
        fields.extend(self.capability_subset.iter().map(String::as_str));
        fields.extend(self.driver_declared_capabilities.iter().map(String::as_str));
        if fields.iter().any(|value| credential_shaped(value)) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A paged, profile-id-sorted provider catalog projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogPageDto {
    pub schema_version: String,
    pub catalog_revision_id: String,
    pub entries: Vec<ProviderCatalogEntryDto>,
    pub next_page_token: Option<String>,
    pub has_more: bool,
}
impl ProviderCatalogPageDto {
    /// Validates the sorted, bounded, credential-free catalog page.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_invalid` for a blank, over-long, or
    /// control-bearing text field, a page exceeding its 256-entry bound, or
    /// an inconsistent `has_more`/next-token pair,
    /// `provider_catalog_unsorted` when entries are not strictly sorted by
    /// profile id (or repeat a profile id), `invalid_page_token` for a
    /// malformed continuation token, and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(&self.schema_version, 64, "provider_catalog_invalid")?;
        valid_text(&self.catalog_revision_id, 256, "provider_catalog_invalid")?;
        if self.entries.len() > 256 {
            return Err(ErrorDto::validation(
                "provider_catalog_invalid",
                "provider catalog page exceeds its 256-entry bound",
            ));
        }
        for pair in self.entries.windows(2) {
            if pair[0].profile_id >= pair[1].profile_id {
                return Err(ErrorDto::validation(
                    "provider_catalog_unsorted",
                    "provider catalog entries must be strictly sorted by profile id",
                ));
            }
        }
        for entry in &self.entries {
            entry.validate()?;
        }
        match (&self.next_page_token, self.has_more) {
            (Some(token), true) => {
                valid_text(token, 1024, "invalid_page_token")?;
            }
            (None, false) => {}
            _ => {
                return Err(ErrorDto::validation(
                    "provider_catalog_invalid",
                    "has_more requires a next page token and vice versa",
                ));
            }
        }
        if credential_shaped(&self.schema_version)
            || credential_shaped(&self.catalog_revision_id)
            || self
                .next_page_token
                .as_deref()
                .is_some_and(credential_shaped)
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A credential-free provider catalog status query.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GetProviderCatalogStatusQueryDto {
    pub schema_version: String,
}
impl GetProviderCatalogStatusQueryDto {
    /// Validates the bounded status query schema version.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_status_invalid` for a blank, over-long, or
    /// control-bearing schema version and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(&self.schema_version, 64, "provider_catalog_status_invalid")?;
        if credential_shaped(&self.schema_version) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// The closed activation state of the daemon provider catalog.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCatalogActivationState {
    /// The catalog is preparing its first revision.
    Preparing,
    /// A catalog revision is active and serving.
    Active,
    /// A removal candidate is pending removal.
    PendingRemoval,
    /// Activation recovery is required before the catalog can serve.
    ActivationRecoveryRequired,
}

/// The closed degraded-reason set of a non-active catalog.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCatalogDegradedReason {
    /// A removal candidate is pending a removal decision.
    RemovalCandidatePending,
    /// A removal candidate was rejected after review.
    RemovalCandidateRejected,
    /// A removal candidate expired before acceptance.
    RemovalCandidateExpired,
    /// Activation recovery is required before the catalog can serve.
    ActivationRecoveryRequired,
}

/// The safe removal impact of a pending catalog candidate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogRemovalImpactDto {
    pub affected_profile_ids: Vec<String>,
    pub safe_impact_summary: String,
}
impl ProviderCatalogRemovalImpactDto {
    /// Validates the bounded, credential-free removal impact.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_removal_impact_invalid` for a blank,
    /// over-long, or control-bearing field, more than 256 affected profiles,
    /// or a blank or control-bearing profile id, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(
            &self.safe_impact_summary,
            4096,
            "provider_catalog_removal_impact_invalid",
        )?;
        if self.affected_profile_ids.len() > 256 {
            return Err(ErrorDto::validation(
                "provider_catalog_removal_impact_invalid",
                "removal impact exceeds its 256-profile bound",
            ));
        }
        for profile in &self.affected_profile_ids {
            valid_text(profile, 256, "provider_catalog_removal_impact_invalid")?;
        }
        if credential_shaped(&self.safe_impact_summary)
            || self
                .affected_profile_ids
                .iter()
                .any(|profile| credential_shaped(profile))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A credential-free provider catalog activation and degradation projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogStatusDto {
    pub schema_version: String,
    pub activation_state: ProviderCatalogActivationState,
    pub degraded_reason: Option<ProviderCatalogDegradedReason>,
    pub active_catalog_revision_id: Option<String>,
    pub candidate_catalog_revision_id: Option<String>,
    pub active_default_profile_id: Option<String>,
    pub removal_impact: Option<ProviderCatalogRemovalImpactDto>,
    pub provider_profiles_negotiated: bool,
}
impl ProviderCatalogStatusDto {
    /// Validates the bounded status fields and the closed
    /// activation/degradation combinations.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_status_invalid` for a blank, over-long, or
    /// control-bearing text field, or an inconsistent activation state and
    /// degraded reason, and `credentials_forbidden` for a credential-shaped
    /// value.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(&self.schema_version, 64, "provider_catalog_status_invalid")?;
        for value in [
            &self.active_catalog_revision_id,
            &self.candidate_catalog_revision_id,
            &self.active_default_profile_id,
        ]
        .into_iter()
        .flatten()
        {
            valid_text(value, 256, "provider_catalog_status_invalid")?;
        }
        let combination_valid = match self.activation_state {
            ProviderCatalogActivationState::Active | ProviderCatalogActivationState::Preparing => {
                self.degraded_reason.is_none()
            }
            ProviderCatalogActivationState::PendingRemoval => {
                self.candidate_catalog_revision_id.is_some()
                    && matches!(
                        self.degraded_reason,
                        Some(
                            ProviderCatalogDegradedReason::RemovalCandidatePending
                                | ProviderCatalogDegradedReason::RemovalCandidateRejected
                                | ProviderCatalogDegradedReason::RemovalCandidateExpired
                        )
                    )
            }
            ProviderCatalogActivationState::ActivationRecoveryRequired => {
                self.degraded_reason
                    == Some(ProviderCatalogDegradedReason::ActivationRecoveryRequired)
            }
        };
        if !combination_valid {
            return Err(ErrorDto::validation(
                "provider_catalog_status_invalid",
                "activation state and degraded reason are inconsistent",
            ));
        }
        if let Some(impact) = &self.removal_impact {
            impact.validate()?;
        }
        let mut fields: Vec<&str> = vec![&self.schema_version];
        fields.extend(
            [
                &self.active_catalog_revision_id,
                &self.candidate_catalog_revision_id,
                &self.active_default_profile_id,
            ]
            .into_iter()
            .flatten()
            .map(String::as_str),
        );
        if fields.iter().any(|value| credential_shaped(value)) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// The closed reason a session provider profile could not be resolved.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProfileUnavailableReason {
    /// No catalog entry exists for the requested profile.
    ProfileNotFound,
    /// The requested profile exists but is disabled.
    ProfileDisabled,
    /// The requested provider is currently unavailable.
    ProviderUnavailable,
    /// The provider catalog is not active yet.
    CatalogNotActive,
}

/// The resolved disposition of a session provider profile reference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum ResolvedProviderProfileDto {
    /// The profile resolved to a concrete catalog revision.
    Resolved {
        profile_id: String,
        profile_revision_id: String,
    },
    /// The profile could not be resolved, with a closed reason.
    Unavailable(ProviderProfileUnavailableReason),
}
impl ResolvedProviderProfileDto {
    /// Validates the resolved profile reference.
    ///
    /// # Errors
    ///
    /// Returns `session_provider_profile_invalid` for a blank, over-long, or
    /// control-bearing profile id or revision and `credentials_forbidden`
    /// for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        match self {
            Self::Resolved {
                profile_id,
                profile_revision_id,
            } => {
                for field in [profile_id, profile_revision_id] {
                    valid_text(field, 256, "session_provider_profile_invalid")?;
                }
                if credential_shaped(profile_id) || credential_shaped(profile_revision_id) {
                    return Err(ErrorDto::validation(
                        "credentials_forbidden",
                        "credentials are forbidden",
                    ));
                }
                Ok(())
            }
            Self::Unavailable(_) => Ok(()),
        }
    }
}

/// A command binding a session's durable provider profile intent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SetSessionProviderProfileCommandDto {
    pub schema_version: String,
    pub session_id: String,
    pub profile_id: String,
    pub expected_session_projection_revision: u64,
    pub operation_id: String,
}
impl SetSessionProviderProfileCommandDto {
    /// Validates the bounded, credential-free set command fields.
    ///
    /// # Errors
    ///
    /// Returns `set_session_provider_profile_invalid` for a blank, over-long,
    /// or control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.schema_version,
            &self.session_id,
            &self.profile_id,
            &self.operation_id,
        ] {
            valid_text(field, 256, "set_session_provider_profile_invalid")?;
        }
        if [
            &self.schema_version,
            &self.session_id,
            &self.profile_id,
            &self.operation_id,
        ]
        .iter()
        .any(|value| credential_shaped(value))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// Acceptance evidence for a session provider profile set operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SetSessionProviderProfileAcceptedDto {
    pub session_id: String,
    pub changed: bool,
    pub resulting_projection_revision: u64,
    pub resolved: ResolvedProviderProfileDto,
}
impl SetSessionProviderProfileAcceptedDto {
    /// Validates the acceptance fields and resolved profile reference.
    ///
    /// # Errors
    ///
    /// Returns `session_provider_profile_invalid` for a blank, over-long, or
    /// control-bearing session id or resolved reference and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(&self.session_id, 256, "session_provider_profile_invalid")?;
        if credential_shaped(&self.session_id) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        self.resolved.validate()
    }
}

/// A query for one session's durable provider profile projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GetSessionProviderProfileQueryDto {
    pub schema_version: String,
    pub session_id: String,
}
impl GetSessionProviderProfileQueryDto {
    /// Validates the bounded, credential-free query fields.
    ///
    /// # Errors
    ///
    /// Returns `session_provider_profile_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [&self.schema_version, &self.session_id] {
            valid_text(field, 256, "session_provider_profile_invalid")?;
        }
        if credential_shaped(&self.schema_version) || credential_shaped(&self.session_id) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// The durable provider profile projection of one session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionProviderProfileDto {
    pub session_id: String,
    pub profile_id: String,
    pub resolved: ResolvedProviderProfileDto,
    pub session_projection_revision: u64,
    pub global_default_profile_id: String,
}
impl SessionProviderProfileDto {
    /// Validates the durable projection fields and resolved reference.
    ///
    /// # Errors
    ///
    /// Returns `session_provider_profile_invalid` for a blank, over-long, or
    /// control-bearing field or resolved reference and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.session_id,
            &self.profile_id,
            &self.global_default_profile_id,
        ] {
            valid_text(field, 256, "session_provider_profile_invalid")?;
        }
        if [
            &self.session_id,
            &self.profile_id,
            &self.global_default_profile_id,
        ]
        .iter()
        .any(|value| credential_shaped(value))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        self.resolved.validate()
    }
}

/// A command accepting the removal of a prepared catalog candidate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AcceptProviderCatalogRemovalCommandDto {
    pub candidate_handle: String,
    pub expected_active_catalog_revision_id: String,
    pub expected_candidate_catalog_revision_id: String,
    pub operation_id: String,
    pub source_recheck: bool,
}
impl AcceptProviderCatalogRemovalCommandDto {
    /// Validates the bounded, credential-free removal command fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_removal_invalid` for a blank, over-long, or
    /// control-bearing field, or when the expected active and candidate
    /// revisions are equal, and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        if self.expected_active_catalog_revision_id == self.expected_candidate_catalog_revision_id {
            return Err(ErrorDto::validation(
                "provider_catalog_removal_invalid",
                "expected active and candidate revisions must differ",
            ));
        }
        for field in [
            &self.candidate_handle,
            &self.expected_active_catalog_revision_id,
            &self.expected_candidate_catalog_revision_id,
            &self.operation_id,
        ] {
            valid_text(field, 256, "provider_catalog_removal_invalid")?;
        }
        if [
            &self.candidate_handle,
            &self.expected_active_catalog_revision_id,
            &self.expected_candidate_catalog_revision_id,
            &self.operation_id,
        ]
        .iter()
        .any(|value| credential_shaped(value))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// Acceptance evidence for an accepted catalog removal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AcceptProviderCatalogRemovalAcceptedDto {
    pub candidate_handle: String,
    pub active_catalog_revision_id: String,
}
impl AcceptProviderCatalogRemovalAcceptedDto {
    /// Validates the bounded, credential-free acceptance fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_removal_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [&self.candidate_handle, &self.active_catalog_revision_id] {
            valid_text(field, 256, "provider_catalog_removal_invalid")?;
        }
        if credential_shaped(&self.candidate_handle)
            || credential_shaped(&self.active_catalog_revision_id)
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A command rejecting a catalog removal candidate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RejectProviderCatalogCandidateCommandDto {
    pub candidate_handle: String,
    pub expected_active_catalog_revision_id: String,
    pub operation_id: String,
}
impl RejectProviderCatalogCandidateCommandDto {
    /// Validates the bounded, credential-free rejection command fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_removal_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.candidate_handle,
            &self.expected_active_catalog_revision_id,
            &self.operation_id,
        ] {
            valid_text(field, 256, "provider_catalog_removal_invalid")?;
        }
        if [
            &self.candidate_handle,
            &self.expected_active_catalog_revision_id,
            &self.operation_id,
        ]
        .iter()
        .any(|value| credential_shaped(value))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// Acceptance evidence for a rejected catalog candidate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RejectProviderCatalogCandidateAcceptedDto {
    pub candidate_handle: String,
}
impl RejectProviderCatalogCandidateAcceptedDto {
    /// Validates the bounded, credential-free acceptance field.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_removal_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(
            &self.candidate_handle,
            256,
            "provider_catalog_removal_invalid",
        )?;
        if credential_shaped(&self.candidate_handle) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// The closed maximum number of runs promoted in one reconciliation page.
pub const MAX_UNAVAILABLE_QUEUE_PROMOTIONS: u64 = 8;

/// A command reconciling a session's unavailable-run queue in bounded pages.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReconcileUnavailableQueueCommandDto {
    pub session_id: String,
    pub operation_id: String,
    pub page_cursor: Option<String>,
}
impl ReconcileUnavailableQueueCommandDto {
    /// Validates the bounded, credential-free reconciliation command fields.
    ///
    /// # Errors
    ///
    /// Returns `unavailable_queue_invalid` for a blank, over-long, or
    /// control-bearing field, `invalid_page_token` for a malformed page
    /// cursor, and `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [&self.session_id, &self.operation_id] {
            valid_text(field, 256, "unavailable_queue_invalid")?;
        }
        if let Some(cursor) = &self.page_cursor {
            valid_text(cursor, 1024, "invalid_page_token")?;
        }
        if credential_shaped(&self.session_id)
            || credential_shaped(&self.operation_id)
            || self.page_cursor.as_deref().is_some_and(credential_shaped)
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// Acceptance evidence for one unavailable-run queue reconciliation page.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReconcileUnavailableQueueAcceptedDto {
    pub session_id: String,
    pub page_cursor: Option<String>,
    pub promoted_count: u64,
}
impl ReconcileUnavailableQueueAcceptedDto {
    /// Validates the bounded, credential-free reconciliation page.
    ///
    /// # Errors
    ///
    /// Returns `unavailable_queue_invalid` for a blank, over-long, or
    /// control-bearing field or a promotion batch beyond the closed
    /// eight-run bound, `invalid_page_token` for a malformed page cursor,
    /// and `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(&self.session_id, 256, "unavailable_queue_invalid")?;
        if credential_shaped(&self.session_id) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        if let Some(cursor) = &self.page_cursor {
            valid_text(cursor, 1024, "invalid_page_token")?;
            if credential_shaped(cursor) {
                return Err(ErrorDto::validation(
                    "credentials_forbidden",
                    "credentials are forbidden",
                ));
            }
        }
        if self.promoted_count > MAX_UNAVAILABLE_QUEUE_PROMOTIONS {
            return Err(ErrorDto::validation(
                "unavailable_queue_invalid",
                "reconciliation page exceeds its 8-promotion bound",
            ));
        }
        Ok(())
    }
}

/// A command admitting a recovered run back into its session.
///
/// Admission restores the run to the session queue; it never reroutes the run
/// to another session or provider.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdmitRecoveredRunCommandDto {
    pub session_id: String,
    pub run_id: String,
    pub operation_id: String,
}
impl AdmitRecoveredRunCommandDto {
    /// Validates the bounded, credential-free admission command fields.
    ///
    /// # Errors
    ///
    /// Returns `recovered_run_admission_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [&self.session_id, &self.run_id, &self.operation_id] {
            valid_text(field, 256, "recovered_run_admission_invalid")?;
        }
        if [&self.session_id, &self.run_id, &self.operation_id]
            .iter()
            .any(|value| credential_shaped(value))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// Acceptance evidence for an admitted recovered run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AdmitRecoveredRunAcceptedDto {
    pub session_id: String,
    pub run_id: String,
}
impl AdmitRecoveredRunAcceptedDto {
    /// Validates the bounded, credential-free acceptance fields.
    ///
    /// # Errors
    ///
    /// Returns `recovered_run_admission_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [&self.session_id, &self.run_id] {
            valid_text(field, 256, "recovered_run_admission_invalid")?;
        }
        if credential_shaped(&self.session_id) || credential_shaped(&self.run_id) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A query for one provider's usage aggregation over a period.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GetProviderUsageQueryDto {
    pub schema_version: String,
    pub profile_id: String,
    pub usage_period_start: u64,
    pub usage_period_end: u64,
}
impl GetProviderUsageQueryDto {
    /// Validates the bounded, credential-free usage query fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_usage_invalid` for a blank, over-long, or
    /// control-bearing field, or a period ending before its start, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        if self.usage_period_end < self.usage_period_start {
            return Err(ErrorDto::validation(
                "provider_usage_invalid",
                "usage period must end at or after its start",
            ));
        }
        for field in [&self.schema_version, &self.profile_id] {
            valid_text(field, 256, "provider_usage_invalid")?;
        }
        if credential_shaped(&self.schema_version) || credential_shaped(&self.profile_id) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// One credential-free provider usage aggregation.
///
/// The aggregation carries units only: request counts and input, output, and
/// reasoning units. It never carries price, currency, or cost values.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UsageAggregationDto {
    pub profile_id: String,
    pub provider_profile_revision_id: String,
    pub model_id: String,
    pub request_count: u64,
    pub input_units: u64,
    pub output_units: u64,
    pub reasoning_units: u64,
    pub usage_period_start: u64,
    pub usage_period_end: u64,
}
impl UsageAggregationDto {
    /// Validates the bounded, credential-free usage aggregation.
    ///
    /// # Errors
    ///
    /// Returns `provider_usage_invalid` for a blank, over-long, or
    /// control-bearing field, or a period ending before its start, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        if self.usage_period_end < self.usage_period_start {
            return Err(ErrorDto::validation(
                "provider_usage_invalid",
                "usage period must end at or after its start",
            ));
        }
        for field in [
            &self.profile_id,
            &self.provider_profile_revision_id,
            &self.model_id,
        ] {
            valid_text(field, 256, "provider_usage_invalid")?;
        }
        if [
            &self.profile_id,
            &self.provider_profile_revision_id,
            &self.model_id,
        ]
        .iter()
        .any(|value| credential_shaped(value))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// Optional reasoning token usage for one provider observation.
///
/// An absent token count means the provider did not report that count; it
/// must never be interpreted as a zero count.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReasoningUsageDto {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

fn validate_safe_event_fields(fields: &[&str], code: &'static str) -> DtoResult<()> {
    for field in fields {
        valid_text(field, 256, code)?;
    }
    if fields.iter().any(|value| credential_shaped(value)) {
        return Err(ErrorDto::validation(
            "credentials_forbidden",
            "credentials are forbidden",
        ));
    }
    Ok(())
}

/// A provider catalog candidate was prepared for activation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogCandidatePreparedEventDto {
    pub candidate_handle: String,
    pub candidate_catalog_revision_id: String,
    pub occurred_at: u64,
}
impl ProviderCatalogCandidatePreparedEventDto {
    /// Validates the bounded, credential-free event fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_event_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_event_fields(
            &[&self.candidate_handle, &self.candidate_catalog_revision_id],
            "provider_catalog_event_invalid",
        )
    }
}

/// A provider catalog removal became pending.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogRemovalPendingEventDto {
    pub candidate_handle: String,
    pub removal_revision_id: String,
    pub occurred_at: u64,
}
impl ProviderCatalogRemovalPendingEventDto {
    /// Validates the bounded, credential-free event fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_event_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_event_fields(
            &[&self.candidate_handle, &self.removal_revision_id],
            "provider_catalog_event_invalid",
        )
    }
}

/// A provider catalog removal candidate was rejected.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogCandidateRejectedEventDto {
    pub candidate_handle: String,
    pub safe_rejection_reason: String,
    pub occurred_at: u64,
}
impl ProviderCatalogCandidateRejectedEventDto {
    /// Validates the bounded, credential-free event fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_event_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_event_fields(
            &[&self.candidate_handle, &self.safe_rejection_reason],
            "provider_catalog_event_invalid",
        )
    }
}

/// A provider catalog removal candidate expired.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogCandidateExpiredEventDto {
    pub candidate_handle: String,
    pub occurred_at: u64,
}
impl ProviderCatalogCandidateExpiredEventDto {
    /// Validates the bounded, credential-free event field.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_event_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_event_fields(&[&self.candidate_handle], "provider_catalog_event_invalid")
    }
}

/// Activation recovery became required for the provider catalog.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogActivationRecoveryRequiredEventDto {
    pub candidate_handle: String,
    pub safe_recovery_reason: String,
    pub occurred_at: u64,
}
impl ProviderCatalogActivationRecoveryRequiredEventDto {
    /// Validates the bounded, credential-free event fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_event_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_event_fields(
            &[&self.candidate_handle, &self.safe_recovery_reason],
            "provider_catalog_event_invalid",
        )
    }
}

/// Provider catalog activation recovery completed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderCatalogRecoveryCompletedEventDto {
    pub active_catalog_revision_id: String,
    pub occurred_at: u64,
}
impl ProviderCatalogRecoveryCompletedEventDto {
    /// Validates the bounded, credential-free event field.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_event_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_event_fields(
            &[&self.active_catalog_revision_id],
            "provider_catalog_event_invalid",
        )
    }
}

/// A session's durable provider profile changed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionProviderProfileChangedEventDto {
    pub session_id: String,
    pub previous_profile_id: String,
    pub profile_id: String,
    pub session_projection_revision: u64,
    pub occurred_at: u64,
}
impl SessionProviderProfileChangedEventDto {
    /// Validates the bounded, credential-free event fields.
    ///
    /// # Errors
    ///
    /// Returns `session_provider_profile_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_event_fields(
            &[
                &self.session_id,
                &self.previous_profile_id,
                &self.profile_id,
            ],
            "session_provider_profile_invalid",
        )
    }
}

/// The explicit origin of a configuration reload request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationOriginDto {
    /// An interactive user requested the reload.
    User,
    /// An administrator requested the reload.
    Admin,
}

/// A command reloading daemon configuration from a candidate reference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReloadConfigurationCommandDto {
    pub candidate_snapshot_reference: Option<String>,
    pub candidate_edit_reference: Option<String>,
    pub expected_active_config_revision: String,
    pub operation_id: String,
    pub origin: ConfigurationOriginDto,
}
impl ReloadConfigurationCommandDto {
    /// Validates the bounded, credential-free reload command fields.
    ///
    /// # Errors
    ///
    /// Returns `configuration_reload_invalid` for a blank, over-long, or
    /// control-bearing field, or when neither candidate reference is
    /// present, and `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        if self.candidate_snapshot_reference.is_none() && self.candidate_edit_reference.is_none() {
            return Err(ErrorDto::validation(
                "configuration_reload_invalid",
                "a reload must name a candidate snapshot or edit reference",
            ));
        }
        for field in [&self.expected_active_config_revision, &self.operation_id] {
            valid_text(field, 256, "configuration_reload_invalid")?;
        }
        if let Some(reference) = &self.candidate_snapshot_reference {
            valid_text(reference, 256, "configuration_reload_invalid")?;
        }
        if let Some(reference) = &self.candidate_edit_reference {
            valid_text(reference, 256, "configuration_reload_invalid")?;
        }
        let mut fields: Vec<&str> = vec![&self.expected_active_config_revision, &self.operation_id];
        if let Some(reference) = &self.candidate_snapshot_reference {
            fields.push(reference);
        }
        if let Some(reference) = &self.candidate_edit_reference {
            fields.push(reference);
        }
        if fields.iter().any(|value| credential_shaped(value)) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// The closed validation outcome of a configuration reload.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationValidationOutcomeDto {
    Valid,
    Invalid,
}

/// The closed atomic commit outcome of a configuration reload.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationCommitOutcomeDto {
    Committed,
    Rejected,
}

/// The durable outcome of one configuration reload transaction.
///
/// Configuration has no migration path under ADR 0038; the former constant
/// `migration_result` wire field was removed with the migration wording.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReloadTransactionDto {
    pub transaction_id: String,
    pub previous_config_revision: String,
    pub candidate_config_revision: String,
    pub validation_result: ConfigurationValidationOutcomeDto,
    pub commit_outcome: ConfigurationCommitOutcomeDto,
    pub safe_failure_code: Option<String>,
    pub safe_failure_detail: Option<String>,
}
impl ReloadTransactionDto {
    /// Validates the bounded, credential-free transaction fields.
    ///
    /// # Errors
    ///
    /// Returns `configuration_reload_invalid` for a blank, over-long, or
    /// control-bearing field, when a failed reload carries no safe failure
    /// code, or when a successful reload carries failure detail, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.transaction_id,
            &self.previous_config_revision,
            &self.candidate_config_revision,
        ] {
            valid_text(field, 256, "configuration_reload_invalid")?;
        }
        let failed = self.validation_result == ConfigurationValidationOutcomeDto::Invalid
            || self.commit_outcome == ConfigurationCommitOutcomeDto::Rejected;
        if failed && self.safe_failure_code.is_none() {
            return Err(ErrorDto::validation(
                "configuration_reload_invalid",
                "failed reloads must carry a safe failure code",
            ));
        }
        if !failed && (self.safe_failure_code.is_some() || self.safe_failure_detail.is_some()) {
            return Err(ErrorDto::validation(
                "configuration_reload_invalid",
                "successful reloads must not carry failure detail",
            ));
        }
        if let Some(code) = &self.safe_failure_code {
            valid_text(code, 128, "configuration_reload_invalid")?;
        }
        if let Some(detail) = &self.safe_failure_detail {
            valid_text(detail, 4096, "configuration_reload_invalid")?;
        }
        let mut fields: Vec<&str> = vec![
            &self.transaction_id,
            &self.previous_config_revision,
            &self.candidate_config_revision,
        ];
        if let Some(code) = &self.safe_failure_code {
            fields.push(code);
        }
        if let Some(detail) = &self.safe_failure_detail {
            fields.push(detail);
        }
        if fields.iter().any(|value| credential_shaped(value)) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A configuration reload was committed and became active.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfigurationReloadedEventDto {
    pub transaction_id: String,
    pub config_revision: String,
    pub occurred_at: u64,
}
impl ConfigurationReloadedEventDto {
    /// Validates the bounded, credential-free event fields.
    ///
    /// # Errors
    ///
    /// Returns `configuration_reload_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        validate_safe_event_fields(
            &[&self.transaction_id, &self.config_revision],
            "configuration_reload_invalid",
        )
    }
}

/// A configuration reload was safely rejected.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfigurationReloadRejectedEventDto {
    pub transaction_id: String,
    pub safe_failure_code: String,
    pub safe_failure_detail: Option<String>,
    pub occurred_at: u64,
}
impl ConfigurationReloadRejectedEventDto {
    /// Validates the bounded, credential-free event fields.
    ///
    /// # Errors
    ///
    /// Returns `configuration_reload_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        let mut fields: Vec<&str> = vec![&self.transaction_id, &self.safe_failure_code];
        if let Some(detail) = &self.safe_failure_detail {
            fields.push(detail);
        }
        validate_safe_event_fields(&fields, "configuration_reload_invalid")
    }
}

/// A command rotating a provider's credentials.
///
/// This command names the affected provider/profile identity and the expected
/// safe composition revision only; the credential material itself is supplied
/// out-of-band through a private channel and never appears in a DTO.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RotateProviderCredentialsCommandDto {
    pub profile_id: String,
    pub provider_profile_revision_id: String,
    pub expected_credential_composition_revision: String,
    pub operation_id: String,
}
impl RotateProviderCredentialsCommandDto {
    /// Validates the bounded, credential-free rotation command fields.
    ///
    /// # Errors
    ///
    /// Returns `credential_rotation_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.profile_id,
            &self.provider_profile_revision_id,
            &self.expected_credential_composition_revision,
            &self.operation_id,
        ] {
            valid_text(field, 256, "credential_rotation_invalid")?;
        }
        if [
            &self.profile_id,
            &self.provider_profile_revision_id,
            &self.expected_credential_composition_revision,
            &self.operation_id,
        ]
        .iter()
        .any(|value| credential_shaped(value))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// The durable result of one provider credential rotation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CredentialRotationResultDto {
    pub operation_id: String,
    pub profile_id: String,
    pub safe_credential_composition_revision: String,
    pub rotated: bool,
}
impl CredentialRotationResultDto {
    /// Validates the bounded, credential-free rotation result fields.
    ///
    /// # Errors
    ///
    /// Returns `credential_rotation_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.operation_id,
            &self.profile_id,
            &self.safe_credential_composition_revision,
        ] {
            valid_text(field, 256, "credential_rotation_invalid")?;
        }
        if [
            &self.operation_id,
            &self.profile_id,
            &self.safe_credential_composition_revision,
        ]
        .iter()
        .any(|value| credential_shaped(value))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// The closed availability observation of one provider health check.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAvailabilityObservation {
    Available,
    Unavailable,
    Unknown,
}

/// The closed failure category of one unavailable provider health check.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHealthFailureCategory {
    ConnectionFailed,
    AuthenticationRejected,
    RequestTimeout,
    RateLimited,
    ServiceUnavailable,
}

/// One non-authorizing provider health evidence observation.
///
/// The evidence records what a check observed; it never authorizes routing
/// or admission decisions by itself.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderHealthEvidenceDto {
    pub profile_id: String,
    pub provider_profile_revision_id: String,
    pub health_attempt_id: String,
    pub check_contract_revision: String,
    pub observed_availability: ProviderAvailabilityObservation,
    pub observed_at: u64,
    pub failure_category: Option<ProviderHealthFailureCategory>,
    pub safe_diagnostic_code: Option<String>,
}
impl ProviderHealthEvidenceDto {
    /// Validates the bounded, credential-free health evidence fields and the
    /// closed availability/failure combinations.
    ///
    /// # Errors
    ///
    /// Returns `provider_health_evidence_invalid` for a blank, over-long, or
    /// control-bearing field, a failure category or diagnostic code on an
    /// `Available` observation, or a missing failure category on an
    /// `Unavailable` observation, and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        let availability_valid = match self.observed_availability {
            ProviderAvailabilityObservation::Available => {
                self.failure_category.is_none() && self.safe_diagnostic_code.is_none()
            }
            ProviderAvailabilityObservation::Unavailable => self.failure_category.is_some(),
            ProviderAvailabilityObservation::Unknown => true,
        };
        if !availability_valid {
            return Err(ErrorDto::validation(
                "provider_health_evidence_invalid",
                "availability observation and failure detail are inconsistent",
            ));
        }
        for field in [
            &self.profile_id,
            &self.provider_profile_revision_id,
            &self.health_attempt_id,
            &self.check_contract_revision,
        ] {
            valid_text(field, 256, "provider_health_evidence_invalid")?;
        }
        if let Some(code) = &self.safe_diagnostic_code {
            valid_text(code, 128, "provider_health_evidence_invalid")?;
        }
        let mut fields: Vec<&str> = vec![
            &self.profile_id,
            &self.provider_profile_revision_id,
            &self.health_attempt_id,
            &self.check_contract_revision,
        ];
        if let Some(code) = &self.safe_diagnostic_code {
            fields.push(code);
        }
        if fields.iter().any(|value| credential_shaped(value)) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// The closed phase of one provider discovery attempt.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDiscoveryPhase {
    BeforeStart,
    Started,
    Terminal,
}

/// One safe provider discovery attempt record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderDiscoveryAttemptDto {
    pub attempt_id: String,
    pub discovery_scope: String,
    pub phase: ProviderDiscoveryPhase,
    pub started_at: u64,
    pub safe_status: String,
}
impl ProviderDiscoveryAttemptDto {
    /// Validates the bounded, credential-free attempt fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_discovery_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [&self.attempt_id, &self.discovery_scope, &self.safe_status] {
            valid_text(field, 256, "provider_discovery_invalid")?;
        }
        if [&self.attempt_id, &self.discovery_scope, &self.safe_status]
            .iter()
            .any(|value| credential_shaped(value))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// One additive provider model discovery record.
///
/// Discovery records are additive observations about a model; they never make
/// routing decisions by themselves.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderModelDiscoveryRecordDto {
    pub discovery_scope: String,
    pub model_id: String,
    pub capability_records: Vec<String>,
    pub source_attempt_id: String,
    pub discovered_at: u64,
}
impl ProviderModelDiscoveryRecordDto {
    /// Validates the bounded, credential-free discovery record fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_discovery_invalid` for a blank, over-long, or
    /// control-bearing field or capability record, or a record with more
    /// than 256 capability records, and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.discovery_scope,
            &self.model_id,
            &self.source_attempt_id,
        ] {
            valid_text(field, 256, "provider_discovery_invalid")?;
        }
        if self.capability_records.len() > 256 {
            return Err(ErrorDto::validation(
                "provider_discovery_invalid",
                "discovery record exceeds its 256-capability bound",
            ));
        }
        for record in &self.capability_records {
            valid_text(record, 256, "provider_discovery_invalid")?;
        }
        let mut fields: Vec<&str> = vec![
            &self.discovery_scope,
            &self.model_id,
            &self.source_attempt_id,
        ];
        fields.extend(self.capability_records.iter().map(String::as_str));
        if fields.iter().any(|value| credential_shaped(value)) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// The closed classification of one pricing observation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingClassification {
    /// The value is bounded by the provider's intrinsic representation.
    IntrinsicRepresentationBound,
    /// The value was observed from provider capacity behavior.
    CapacityObservation,
    /// The value follows the provider's published product policy.
    ProductPolicy,
}

/// One safe, non-authorizing pricing observation.
///
/// The observation records a bounded numeric value for one provider kind and
/// model; it is never an admission ceiling on its own.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PricingObservationDto {
    pub provider_kind_id: String,
    pub model_id: String,
    pub bounded_numeric_value: u64,
    pub classification: PricingClassification,
    pub observed_at: u64,
}
impl PricingObservationDto {
    /// Validates the bounded, credential-free observation fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_pricing_observation_invalid` for a blank, over-long,
    /// or control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [&self.provider_kind_id, &self.model_id] {
            valid_text(field, 256, "provider_pricing_observation_invalid")?;
        }
        if credential_shaped(&self.provider_kind_id) || credential_shaped(&self.model_id) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A query requesting non-authorizing provider health evidence.
///
/// The query names one provider; the returned evidence records what a check
/// observed and never authorizes routing or admission by itself.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GetProviderHealthEvidenceQueryDto {
    pub schema_version: String,
    pub provider_id: String,
}
impl GetProviderHealthEvidenceQueryDto {
    /// Validates the bounded, credential-free health query fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_health_invalid` for a blank, over-long, or
    /// control-bearing schema version or provider id (the provider id is
    /// bounded at 63 characters, matching the profile id bound), and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(&self.schema_version, 64, "provider_health_invalid")?;
        valid_text(&self.provider_id, 63, "provider_health_invalid")?;
        if credential_shaped(&self.schema_version) || credential_shaped(&self.provider_id) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A query requesting the status of one provider discovery attempt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GetProviderDiscoveryStatusQueryDto {
    pub schema_version: String,
    pub attempt_id: Option<String>,
}
impl GetProviderDiscoveryStatusQueryDto {
    /// Validates the bounded, credential-free discovery status query fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_discovery_invalid` for a blank, over-long, or
    /// control-bearing schema version or attempt reference, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(&self.schema_version, 64, "provider_discovery_invalid")?;
        if let Some(attempt_id) = &self.attempt_id {
            valid_text(attempt_id, 256, "provider_discovery_invalid")?;
        }
        if credential_shaped(&self.schema_version)
            || self.attempt_id.as_deref().is_some_and(credential_shaped)
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A query requesting the safe pricing policy projection.
///
/// The projection is never an admission ceiling, quota, or reservation: it
/// records bounded observations and their code-owned classification only.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GetPricingPolicyQueryDto {
    pub schema_version: String,
    pub model_id: Option<String>,
}
impl GetPricingPolicyQueryDto {
    /// Validates the bounded, credential-free pricing query fields.
    ///
    /// # Errors
    ///
    /// Returns `provider_pricing_query_invalid` for a blank, over-long, or
    /// control-bearing schema version or model reference, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(&self.schema_version, 64, "provider_pricing_query_invalid")?;
        if let Some(model_id) = &self.model_id {
            valid_text(model_id, 63, "provider_pricing_query_invalid")?;
        }
        if credential_shaped(&self.schema_version)
            || self.model_id.as_deref().is_some_and(credential_shaped)
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A credential-free projection of one provider's health evidence.
///
/// The projection is closed and non-authorizing: it records what checks
/// observed and creates no RunId, reason, or selection. Restoration of a
/// provider therefore only permits reevaluation; it never routes or admits by
/// itself.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderHealthProjectionDto {
    pub provider_id: String,
    pub observations: Vec<ProviderHealthEvidenceDto>,
    pub safe_reason_code: Option<String>,
    pub observed_at: u64,
}
impl ProviderHealthProjectionDto {
    /// Validates the bounded, credential-free health projection.
    ///
    /// # Errors
    ///
    /// Returns `provider_health_invalid` for a blank, over-long, or
    /// control-bearing provider id or safe reason code, a projection with
    /// more than 64 observations, or an invalid observation, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(&self.provider_id, 63, "provider_health_invalid")?;
        if let Some(code) = &self.safe_reason_code {
            valid_text(code, 128, "provider_health_invalid")?;
        }
        bounded(self.observations.clone(), 64, "provider_health_invalid")?;
        for observation in &self.observations {
            observation.validate()?;
        }
        if credential_shaped(&self.provider_id)
            || self
                .safe_reason_code
                .as_deref()
                .is_some_and(credential_shaped)
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A credential-free projection of one provider discovery attempt.
///
/// The projection is additive only: records are observations about model
/// identities and never route traffic. Attempt status is reported through the
/// closed [`ProviderDiscoveryPhase`]; a terminal phase means the discovery
/// port returned or errored. No automatic continuation is ever implied.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderDiscoveryProjectionDto {
    pub attempt_id: Option<String>,
    pub phase: Option<ProviderDiscoveryPhase>,
    pub records: Vec<ProviderModelDiscoveryRecordDto>,
    pub safe_status: Option<String>,
}
impl ProviderDiscoveryProjectionDto {
    /// Validates the bounded, credential-free discovery projection.
    ///
    /// # Errors
    ///
    /// Returns `provider_discovery_invalid` for a blank, over-long, or
    /// control-bearing attempt reference or safe status, a projection with
    /// more than 256 records or an invalid record, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        if let Some(attempt_id) = &self.attempt_id {
            valid_text(attempt_id, 256, "provider_discovery_invalid")?;
        }
        if let Some(status) = &self.safe_status {
            valid_text(status, 256, "provider_discovery_invalid")?;
        }
        bounded(self.records.clone(), 256, "provider_discovery_invalid")?;
        for record in &self.records {
            record.validate()?;
        }
        if self.attempt_id.as_deref().is_some_and(credential_shaped)
            || self.safe_status.as_deref().is_some_and(credential_shaped)
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A credential-free, non-authorizing pricing policy projection.
///
/// The projection carries bounded observations and one code-owned policy
/// classification. It is never an admission ceiling, quota, or reservation for
/// Mandate admission, tool admission, or scheduler eligibility.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PricingProjectionDto {
    pub observations: Vec<PricingObservationDto>,
    pub policy_classification: Option<PricingClassification>,
    pub disclaimer: Option<String>,
}
impl PricingProjectionDto {
    /// Validates the bounded, credential-free pricing projection.
    ///
    /// # Errors
    ///
    /// Returns `provider_pricing_projection_invalid` for a projection with
    /// more than 256 observations or an invalid observation, a blank,
    /// over-long, or control-bearing disclaimer, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        bounded(
            self.observations.clone(),
            256,
            "provider_pricing_projection_invalid",
        )?;
        for observation in &self.observations {
            observation.validate()?;
        }
        if let Some(disclaimer) = &self.disclaimer {
            valid_text(disclaimer, 1024, "provider_pricing_projection_invalid")?;
        }
        if self.disclaimer.as_deref().is_some_and(credential_shaped) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A query requesting the safe configuration projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GetConfigurationProjectionQueryDto {
    pub schema_version: String,
}
impl GetConfigurationProjectionQueryDto {
    /// Validates the bounded, credential-free configuration projection query.
    ///
    /// # Errors
    ///
    /// Returns `configuration_projection_invalid` for a blank, over-long, or
    /// control-bearing schema version and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        valid_text(&self.schema_version, 64, "configuration_projection_invalid")?;
        if credential_shaped(&self.schema_version) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A safe projection of the applied daemon configuration.
///
/// The projection carries the applied config revision, the resolved provider
/// kind and model, whether a credential is configured (never the credential
/// itself), the provider execution policy, and the closed reload status. It
/// never carries raw TOML, credentials, private endpoints, or paths.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfigurationProjectionDto {
    pub schema_version: String,
    pub applied_config_revision_id: String,
    pub provider_kind: String,
    pub model_id: String,
    pub credential_configured: bool,
    pub provider_execution_policy: String,
    pub reload_status: String,
}
impl ConfigurationProjectionDto {
    /// Validates the bounded, credential-free configuration projection.
    ///
    /// # Errors
    ///
    /// Returns `configuration_projection_invalid` for a blank, over-long, or
    /// control-bearing field and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.schema_version,
            &self.applied_config_revision_id,
            &self.provider_kind,
            &self.model_id,
            &self.provider_execution_policy,
            &self.reload_status,
        ] {
            valid_text(field, 256, "configuration_projection_invalid")?;
        }
        if [
            &self.schema_version,
            &self.applied_config_revision_id,
            &self.provider_kind,
            &self.model_id,
            &self.provider_execution_policy,
            &self.reload_status,
        ]
        .iter()
        .any(|value| credential_shaped(value))
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// A bounded, credential-free raw TOML configuration edit.
///
/// The candidate content is bounded and validated free of credentials and
/// NUL characters. Responses to this command never echo the raw candidate
/// content back to any peer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RawTomlEditCommandDto {
    pub operation_id: String,
    pub expected_config_revision: String,
    pub candidate_content: String,
}
impl RawTomlEditCommandDto {
    /// Validates the bounded, credential-free raw edit fields.
    ///
    /// # Errors
    ///
    /// Returns `raw_toml_edit_invalid` for a blank, over-long, or
    /// control-bearing command field, an empty or over-long candidate
    /// content, or a candidate carrying characters outside newline,
    /// carriage-return, tab, and printable text, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [&self.operation_id, &self.expected_config_revision] {
            valid_text(field, 256, "raw_toml_edit_invalid")?;
        }
        if self.candidate_content.trim().is_empty() || self.candidate_content.len() > 64 * 1024 {
            return Err(ErrorDto::validation(
                "raw_toml_edit_invalid",
                "candidate content is invalid",
            ));
        }
        if self
            .candidate_content
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        {
            return Err(ErrorDto::validation(
                "raw_toml_edit_invalid",
                "candidate content carries invalid control characters",
            ));
        }
        if intention_domain::canonical::credential_shaped_raw_content(&self.candidate_content)
            || credential_shaped(&self.operation_id)
            || credential_shaped(&self.expected_config_revision)
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// One typed, credential-free configuration edit operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigurationEditOperationDto {
    /// Sets one configuration key path to a bounded safe value.
    Set {
        key_path: String,
        safe_value: String,
    },
    /// Removes one configuration key path.
    Remove { key_path: String },
}
impl ConfigurationEditOperationDto {
    /// Validates the bounded, credential-free operation fields.
    ///
    /// # Errors
    ///
    /// Returns `configuration_edit_invalid` for a blank, over-long, or
    /// control-bearing key path or safe value and `credentials_forbidden`
    /// for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        match self {
            Self::Set {
                key_path,
                safe_value,
            } => {
                valid_text(key_path, 256, "configuration_edit_invalid")?;
                valid_text(safe_value, 1024, "configuration_edit_invalid")?;
                if credential_shaped(key_path) || credential_shaped(safe_value) {
                    return Err(ErrorDto::validation(
                        "credentials_forbidden",
                        "credentials are forbidden",
                    ));
                }
                Ok(())
            }
            Self::Remove { key_path } => {
                valid_text(key_path, 256, "configuration_edit_invalid")?;
                if credential_shaped(key_path) {
                    return Err(ErrorDto::validation(
                        "credentials_forbidden",
                        "credentials are forbidden",
                    ));
                }
                Ok(())
            }
        }
    }
}

/// A command applying typed, credential-free configuration edits.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfigurationEditCommandDto {
    pub operation_id: String,
    pub expected_config_revision: String,
    pub operations: Vec<ConfigurationEditOperationDto>,
}
impl ConfigurationEditCommandDto {
    /// Validates the bounded, credential-free typed edit command.
    ///
    /// # Errors
    ///
    /// Returns `configuration_edit_invalid` for a blank, over-long, or
    /// control-bearing command field, an empty operation list or one beyond
    /// its 16-operation bound, or an invalid operation, and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [&self.operation_id, &self.expected_config_revision] {
            valid_text(field, 256, "configuration_edit_invalid")?;
        }
        if credential_shaped(&self.operation_id)
            || credential_shaped(&self.expected_config_revision)
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        if self.operations.is_empty() || self.operations.len() > 16 {
            return Err(ErrorDto::validation(
                "configuration_edit_invalid",
                "typed edits must carry between 1 and 16 operations",
            ));
        }
        for operation in &self.operations {
            operation.validate()?;
        }
        Ok(())
    }
}

/// A closed code-owned policy for arbitrary provider request headers.
///
/// The policy names allowed header names only, bound to one kind descriptor
/// revision; it never carries header values.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArbitraryHeaderPolicyDto {
    pub policy_revision: String,
    pub kind_descriptor_revision_id: String,
    pub allowed_header_names: Vec<String>,
}
impl ArbitraryHeaderPolicyDto {
    /// Validates the bounded, credential-free header policy fields.
    ///
    /// # Errors
    ///
    /// Returns `arbitrary_header_policy_invalid` for a blank, over-long, or
    /// control-bearing revision or header name, a policy with no names or
    /// more than 64 names, and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [&self.policy_revision, &self.kind_descriptor_revision_id] {
            valid_text(field, 256, "arbitrary_header_policy_invalid")?;
        }
        if self.allowed_header_names.is_empty() || self.allowed_header_names.len() > 64 {
            return Err(ErrorDto::validation(
                "arbitrary_header_policy_invalid",
                "header policy must carry between 1 and 64 safe header names",
            ));
        }
        for header in &self.allowed_header_names {
            let header = header.trim();
            if header.is_empty()
                || header.chars().count() > 128
                || header.chars().any(char::is_control)
            {
                return Err(ErrorDto::validation(
                    "arbitrary_header_policy_invalid",
                    "allowed header name is invalid",
                ));
            }
        }
        let mut fields: Vec<&str> = vec![&self.policy_revision, &self.kind_descriptor_revision_id];
        fields.extend(self.allowed_header_names.iter().map(String::as_str));
        if fields.iter().any(|value| credential_shaped(value)) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        Ok(())
    }
}

/// Local-history-first provider reasoning preservation controls.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderPreservationControlsDto {
    /// Preserve provider reasoning in the local history.
    pub preserve_thinking: bool,
    /// Keep the thinking field in the local history when preserved.
    pub thinking_keep: bool,
}

/// A closed code-owned server-side parser configuration.
///
/// The configuration names a parser and its bounded limits only; it never
/// carries raw JSON templates.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerSideParserConfigDto {
    None,
    Vllm {
        parser_id: String,
        bounded_limits: String,
    },
    Sglang {
        parser_id: String,
        bounded_limits: String,
    },
}
impl ServerSideParserConfigDto {
    /// Validates the bounded, credential-free parser configuration fields.
    ///
    /// # Errors
    ///
    /// Returns `server_side_parser_invalid` for a blank, over-long, or
    /// control-bearing parser id or bounded limits and
    /// `credentials_forbidden` for a credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        match self {
            Self::None => Ok(()),
            Self::Vllm {
                parser_id,
                bounded_limits,
            }
            | Self::Sglang {
                parser_id,
                bounded_limits,
            } => {
                valid_text(parser_id, 256, "server_side_parser_invalid")?;
                valid_text(bounded_limits, 1024, "server_side_parser_invalid")?;
                if credential_shaped(parser_id) || credential_shaped(bounded_limits) {
                    return Err(ErrorDto::validation(
                        "credentials_forbidden",
                        "credentials are forbidden",
                    ));
                }
                Ok(())
            }
        }
    }
}

/// The closed reasoning effort levels recognized by the catalog projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffortLevel {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

/// The closed Responses reasoning modes recognized by the catalog projection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsesReasoningMode {
    Standard,
    Pro,
}

/// A credential-free provider reasoning catalog projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderReasoningCatalogProjectionDto {
    pub provider_kind_id: String,
    pub model_id: String,
    pub supported_effort_levels: Vec<ReasoningEffortLevel>,
    pub responses_reasoning_modes: Vec<ResponsesReasoningMode>,
    pub projection_revision: String,
}
impl ProviderReasoningCatalogProjectionDto {
    /// Validates the bounded, duplicate-free, credential-free projection.
    ///
    /// # Errors
    ///
    /// Returns `provider_reasoning_catalog_invalid` for a blank, over-long,
    /// or control-bearing text field, a closed set with duplicates or
    /// repeated entries, and `credentials_forbidden` for a
    /// credential-shaped value.
    pub fn validate(&self) -> DtoResult<()> {
        for field in [
            &self.provider_kind_id,
            &self.model_id,
            &self.projection_revision,
        ] {
            valid_text(field, 256, "provider_reasoning_catalog_invalid")?;
        }
        if credential_shaped(&self.provider_kind_id)
            || credential_shaped(&self.model_id)
            || credential_shaped(&self.projection_revision)
        {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
        for (index, level) in self.supported_effort_levels.iter().enumerate() {
            if self.supported_effort_levels[..index].contains(level) {
                return Err(ErrorDto::validation(
                    "provider_reasoning_catalog_invalid",
                    "supported effort levels must not repeat",
                ));
            }
        }
        for (index, mode) in self.responses_reasoning_modes.iter().enumerate() {
            if self.responses_reasoning_modes[..index].contains(mode) {
                return Err(ErrorDto::validation(
                    "provider_reasoning_catalog_invalid",
                    "responses reasoning modes must not repeat",
                ));
            }
        }
        Ok(())
    }
}

/// Metadata linking one public wire contract family to its domain-owned tag.
///
/// This descriptor is nonserialized metadata: it never appears in DTO JSON.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContractFamilyDescriptor {
    /// The stable ledger family name.
    pub name: &'static str,
    /// The family version (v1 or v2 for the fork snapshot and preview
    /// families, which share one ledger tag).
    pub version: u16,
    /// The domain-owned canonical tag from [`TagRegistry`].
    pub tag: u32,
}

pub const PROGRAMMATIC_CALLER_POLICY_SELECTION_V1: ContractFamilyDescriptor =
    ContractFamilyDescriptor {
        name: "programmatic-caller-policy-selection-v1",
        version: 1,
        tag: TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
    };
pub const AGENT_ACTIVITY_SELECTION_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "agent-activity-selection-v1",
    version: 1,
    tag: TagRegistry::AGENT_ACTIVITY_SELECTION_V1,
};
pub const GOAL_RUN_SELECTION_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "goal-run-selection-v1",
    version: 1,
    tag: TagRegistry::GOAL_RUN_SELECTION_V1,
};
pub const CONTINUAL_HARNESS_SELECTION_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "continual-harness-selection-v1",
    version: 1,
    tag: TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
};
pub const MCP_METHOD_CATALOG_SELECTION_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "mcp-method-catalog-selection-v1",
    version: 1,
    tag: TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1,
};
pub const MODEL_CAPABILITY_TAXONOMY_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "model-capability-taxonomy-v1",
    version: 1,
    tag: TagRegistry::MODEL_CAPABILITY_TAXONOMY_V1,
};
pub const PROVIDER_PROFILE_REVISION_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "provider-profile-revision-v1",
    version: 1,
    tag: TagRegistry::PROVIDER_PROFILE_REVISION_V1,
};
pub const PROVIDER_SELECTION_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "provider-selection-v1",
    version: 1,
    tag: TagRegistry::PROVIDER_SELECTION_V1,
};
pub const REASONING_HISTORY_MANIFEST_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "reasoning-history-manifest-v1",
    version: 1,
    tag: TagRegistry::REASONING_HISTORY_MANIFEST_V1,
};
pub const CONTEXT_SOURCE_MANIFEST_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "context-source-manifest-v1",
    version: 1,
    tag: TagRegistry::CONTEXT_SOURCE_MANIFEST_V1,
};
pub const MODEL_CONTEXT_PROJECTION_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "model-context-projection-v1",
    version: 1,
    tag: TagRegistry::MODEL_CONTEXT_PROJECTION_V1,
};
pub const TOOL_DESCRIPTOR_REVISION: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "tool-descriptor-revision",
    version: 1,
    tag: TagRegistry::TOOL_DESCRIPTOR_REVISION,
};
pub const TOOL_REGISTRY_REVISION: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "tool-registry-revision",
    version: 1,
    tag: TagRegistry::TOOL_REGISTRY_REVISION,
};
pub const MODEL_TOOL_LOOP_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "model-tool-loop-v1",
    version: 1,
    tag: TagRegistry::MODEL_TOOL_LOOP_V1,
};
pub const BRIDGE_INVOCATION_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "bridge-invocation-v1",
    version: 1,
    tag: TagRegistry::BRIDGE_INVOCATION_V1,
};
pub const FORK_BASE_SNAPSHOT_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "fork-base-snapshot-v1",
    version: 1,
    tag: TagRegistry::FORK_BASE_SNAPSHOT_V1,
};
pub const FORK_BASE_SNAPSHOT_V2: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "fork-base-snapshot-v2",
    version: 2,
    tag: TagRegistry::FORK_BASE_SNAPSHOT_V1,
};
pub const FORK_PREVIEW_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "fork-preview-v1",
    version: 1,
    tag: TagRegistry::FORK_PREVIEW_V1,
};
pub const FORK_PREVIEW_V2: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "fork-preview-v2",
    version: 2,
    tag: TagRegistry::FORK_PREVIEW_V1,
};
pub const FORK_COMMAND_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "fork-command-v1",
    version: 1,
    tag: TagRegistry::FORK_COMMAND_V1,
};
pub const AGENT_ACTIVITY_TREE_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "agent-activity-tree-v1",
    version: 1,
    tag: TagRegistry::AGENT_ACTIVITY_TREE_V1,
};
pub const AGENT_ACTIVITY_PAIR_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "agent-activity-pair-v1",
    version: 1,
    tag: TagRegistry::AGENT_ACTIVITY_PAIR_V1,
};
pub const AGENT_MESSAGE_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "agent-message-v1",
    version: 1,
    tag: TagRegistry::AGENT_MESSAGE_V1,
};
pub const AGENT_ACTIVITY_JOURNAL_RECORD_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "agent-activity-journal-record-v1",
    version: 1,
    tag: TagRegistry::AGENT_ACTIVITY_JOURNAL_RECORD_V1,
};
pub const AGENT_NOTIFICATION_RECORD_V1: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "agent-notification-record-v1",
    version: 1,
    tag: TagRegistry::AGENT_NOTIFICATION_RECORD_V1,
};

/// Every public wire contract family in the ADR 0036 ledger, in ledger order.
///
/// The fork snapshot and preview families carry one descriptor per version
/// (v1 and v2) sharing their single ledger tag; every other family appears
/// exactly once.
pub const PUBLIC_WIRE_CONTRACT_FAMILIES: &[ContractFamilyDescriptor] = &[
    PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
    AGENT_ACTIVITY_SELECTION_V1,
    GOAL_RUN_SELECTION_V1,
    CONTINUAL_HARNESS_SELECTION_V1,
    MCP_METHOD_CATALOG_SELECTION_V1,
    MODEL_CAPABILITY_TAXONOMY_V1,
    PROVIDER_PROFILE_REVISION_V1,
    PROVIDER_SELECTION_V1,
    REASONING_HISTORY_MANIFEST_V1,
    CONTEXT_SOURCE_MANIFEST_V1,
    MODEL_CONTEXT_PROJECTION_V1,
    TOOL_DESCRIPTOR_REVISION,
    TOOL_REGISTRY_REVISION,
    MODEL_TOOL_LOOP_V1,
    BRIDGE_INVOCATION_V1,
    FORK_BASE_SNAPSHOT_V1,
    FORK_BASE_SNAPSHOT_V2,
    FORK_PREVIEW_V1,
    FORK_PREVIEW_V2,
    FORK_COMMAND_V1,
    AGENT_ACTIVITY_TREE_V1,
    AGENT_ACTIVITY_PAIR_V1,
    AGENT_MESSAGE_V1,
    AGENT_ACTIVITY_JOURNAL_RECORD_V1,
    AGENT_NOTIFICATION_RECORD_V1,
];

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;

    fn round_trip<T>(value: &T)
    where
        T: Clone + Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let wire = serde_json::to_vec(value).expect("DTO encodes");
        assert_eq!(
            serde_json::from_slice::<T>(&wire).expect("DTO decodes"),
            value.clone()
        );
    }

    fn provider_selection() -> ResolvedRunProviderSelectionDto {
        ResolvedRunProviderSelectionDto {
            selection_canonicalization_version: "1".to_owned(),
            profile_id: "profile-1".to_owned(),
            provider_profile_revision_id: "rev-1".to_owned(),
            kind_id: "responses".to_owned(),
            kind_descriptor_revision_id: "kind-rev-1".to_owned(),
            model_id: "model-1".to_owned(),
            normalized_effective_endpoint: "https://provider.example".to_owned(),
            credential_transport_mode: CredentialTransportMode::SafeHeader,
            credential_transport_safe_header_name: Some("x-safe-header".to_owned()),
            declared_model_capability_subset: vec!["text".to_owned()],
            resolved_reasoning_policy: "reasoning-policy".to_owned(),
            effective_execution_policy: "execution-policy".to_owned(),
            effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
            provider_driver_contract_revision: "driver-1.0".to_owned(),
            selection_source: Some("provider-profiles".to_owned()),
        }
    }

    fn fork_command() -> ForkSessionCommandDto {
        ForkSessionCommandDto {
            source_session_id: "source".to_owned(),
            boundary: ForkBoundary::CommittedUserTurn {
                source_turn_id: "turn-1".to_owned(),
                accepted_sequence: 7,
            },
            expected_source_sequence: 7,
            expected_preview_digest: "a".repeat(64),
            title_present: true,
            requested_title: Some("forked session".to_owned()),
            future_profile_override_present: true,
            future_profile_override: Some("profile-2".to_owned()),
            expected_profile_revision: None,
        }
    }

    fn fork_command_with_title(title: String) -> ForkSessionCommandDto {
        let mut command = fork_command();
        command.requested_title = Some(title);
        command
    }

    fn agent_message() -> AgentMessageDto {
        AgentMessageDto {
            message_id: "message-1".to_owned(),
            activity_tree_id: AgentActivityTreeId("tree-1".to_owned()),
            pair_id: "pair-1".to_owned(),
            pair_order: 1,
            direction: AgentMessageDirection::ParentToChild,
            kind: AgentMessageKind::Instruction,
            sender_run_reference: "run-1".to_owned(),
            recipient_run_reference: "run-2".to_owned(),
            source_model_step_reference: "step-1".to_owned(),
            safe_text: Some("safe text".to_owned()),
            typed_references: Vec::new(),
            delivery_state: "delivered".to_owned(),
            canonical_message_digest: "a".repeat(64),
        }
    }

    fn tree_page() -> ConversationTreePageDto {
        ConversationTreePageDto {
            children: vec![ConversationBranchSummaryDto {
                session_id: "session-1".to_owned(),
                title: Some("branch".to_owned()),
            }],
            next_page: None,
        }
    }

    #[test]
    fn provider_selection_and_credential_transport_families_round_trip() {
        round_trip(&provider_selection());
        for mode in [
            CredentialTransportMode::Bearer,
            CredentialTransportMode::SafeHeader,
        ] {
            round_trip(&mode);
        }
    }

    #[test]
    fn fork_family_round_trips() {
        round_trip(&fork_command());
        round_trip(&ForkSessionResultDto {
            session_id: "session-1".to_owned(),
            preview_digest: "a".repeat(64),
        });
        round_trip(&GetForkPreviewQueryDto {
            source_session_id: "source".to_owned(),
            boundary: ForkBoundary::CompletedAssistantTurn {
                source_run_id: "run-1".to_owned(),
                final_assistant_turn_id: Some("turn-1".to_owned()),
                completed_sequence: 9,
                final_run_cursor: 12,
            },
        });
        round_trip(&ForkPreviewDto {
            preview_digest: "a".repeat(64),
            page_count: 3,
            snapshot_size_bytes: 4096,
        });
        round_trip(&StartForkRunCommandDto {
            session_id: "session-1".to_owned(),
            profile_override: Some("profile-2".to_owned()),
            expected_profile_revision: None,
        });
        round_trip(&GetConversationTreeQueryDto {
            session_id: "session-1".to_owned(),
            page: 0,
        });
        round_trip(&tree_page());
        round_trip(&ConversationBranchSummaryDto {
            session_id: "session-1".to_owned(),
            title: None,
        });
        round_trip(&RenameSessionCommandDto {
            session_id: "session-1".to_owned(),
            title: "renamed".to_owned(),
        });
        round_trip(&ArchiveSessionCommandDto {
            session_id: "session-1".to_owned(),
        });
        round_trip(&RestoreSessionCommandDto {
            session_id: "session-1".to_owned(),
        });
        for boundary in [
            ForkBoundary::CommittedUserTurn {
                source_turn_id: "turn-1".to_owned(),
                accepted_sequence: 7,
            },
            ForkBoundary::CompletedAssistantTurn {
                source_run_id: "run-1".to_owned(),
                final_assistant_turn_id: None,
                completed_sequence: 9,
                final_run_cursor: 12,
            },
        ] {
            round_trip(&boundary);
        }
    }

    #[test]
    fn reasoning_history_family_round_trips() {
        for category in [
            ReasoningFactCategory::Primary,
            ReasoningFactCategory::Detail,
        ] {
            round_trip(&category);
        }
        round_trip(&RunReasoningHistoryPageDto {
            session_id: "session-1".to_owned(),
            run_id: "run-1".to_owned(),
            captured_upper_cursor: 5,
            facts: vec!["fact".to_owned()],
        });
        round_trip(&RunReasoningHistoryCompletedDto {
            run_id: "run-1".to_owned(),
            final_cursor: 9,
        });
        let manifest = ReasoningHistoryManifestDto {
            compatibility_id: "compat-1".to_owned(),
            entries: vec!["entry".to_owned()],
            manifest_digest: "a".repeat(64),
        };
        assert!(manifest.validate().is_ok());
        round_trip(&manifest);
        for transfer in [
            ReasoningHistoryTransferDto::Disabled,
            ReasoningHistoryTransferDto::TextualHistoryV1 {
                compatibility_id: "compat-1".to_owned(),
            },
        ] {
            round_trip(&transfer);
        }
    }

    #[test]
    fn agent_activity_family_round_trips() {
        round_trip(&AgentActivityTreeId("tree-1".to_owned()));
        for direction in [
            AgentMessageDirection::ParentToChild,
            AgentMessageDirection::ChildToParent,
        ] {
            round_trip(&direction);
        }
        for kind in [
            AgentMessageKind::Instruction,
            AgentMessageKind::Report,
            AgentMessageKind::ClarificationRequest,
            AgentMessageKind::ClarificationReply,
        ] {
            round_trip(&kind);
        }
        assert!(agent_message().validate().is_ok());
        round_trip(&agent_message());
        let journal_record = AgentActivityJournalRecordDto {
            activity_tree_id: AgentActivityTreeId("tree-1".to_owned()),
            record_id: "record-1".to_owned(),
            sequence: 3,
            occurred_at: 100,
            root_run_reference: "run-1".to_owned(),
            direct_pair_reference_when_present: Some("pair-1".to_owned()),
            record_kind: "message".to_owned(),
            safe_user_projection: "safe".to_owned(),
            typed_references: Vec::new(),
            canonical_record_digest: "a".repeat(64),
        };
        assert!(journal_record.validate().is_ok());
        round_trip(&journal_record);
    }

    #[test]
    fn notification_family_round_trips() {
        round_trip(&AgentNotificationCursorDto(4));
        round_trip(&AgentNotificationSummaryDto {
            activity_tree_id: AgentActivityTreeId("tree-1".to_owned()),
            safe_summary: "summary".to_owned(),
        });
        for level in [NotificationLevel::Urgent, NotificationLevel::Ordinary] {
            round_trip(&level);
        }
        let notification_record = AgentNotificationRecordDto {
            notification_cursor: AgentNotificationCursorDto(4),
            activity_tree_id: AgentActivityTreeId("tree-1".to_owned()),
            activity_record_reference: "record-1".to_owned(),
            level: NotificationLevel::Ordinary,
            reason: "reason".to_owned(),
            safe_counts_and_states: "counts".to_owned(),
            occurred_at: 100,
        };
        assert!(notification_record.validate().is_ok());
        round_trip(&notification_record);
    }

    #[test]
    fn gateway_family_round_trips() {
        let grant = BridgeRunGrantDto {
            opaque_grant_identity: "grant-1".to_owned(),
            issued_protocol_revision: "rev-1".to_owned(),
        };
        assert!(grant.validate().is_ok());
        round_trip(&grant);
        round_trip(&BridgeAttachmentResponseDto {
            bridge_run_grant: grant.clone(),
            negotiated_capabilities: vec![
                crate::ProtocolCapabilityDto::DaemonToolGatewayV1,
                crate::ProtocolCapabilityDto::ModelToolLoopV1,
            ],
            initial_run_cursor: 3,
        });
        let command = BridgeInvocationCommandDto {
            bridge_run_grant: grant,
            bridge_operation_id: "operation-1".to_owned(),
            typed_tool_invocation: "invocation".to_owned(),
        };
        assert!(command.validate().is_ok());
        round_trip(&command);
        let accepted = BridgeInvocationAcceptedDto {
            bridge_operation_id: "operation-1".to_owned(),
            tool_call_id: "call-1".to_owned(),
            admission_state: "admitted".to_owned(),
        };
        assert!(accepted.validate().is_ok());
        round_trip(&accepted);
        let operation = BridgeOperationV1 {
            bridge_operation_id: "operation-1".to_owned(),
            run_id: "run-1".to_owned(),
            mandate_id: "mandate-1".to_owned(),
            mandate_revision: "rev-1".to_owned(),
            model_step_id: "step-1".to_owned(),
            tool_id: "tool-1".to_owned(),
            descriptor_revision: "descriptor-1".to_owned(),
            typed_input_digest: "a".repeat(64),
            tool_call_id: "call-1".to_owned(),
            admission_outcome: "admitted".to_owned(),
            attempt_reference: Some("attempt-1".to_owned()),
        };
        assert!(operation.validate().is_ok());
        round_trip(&operation);
        round_trip(&RunToolHistoryPageDto {
            session_id: "session-1".to_owned(),
            run_id: "run-1".to_owned(),
            captured_upper_cursor: 5,
            facts: vec!["fact".to_owned()],
        });
        round_trip(&RunToolHistoryCompletedDto {
            run_id: "run-1".to_owned(),
            final_cursor: 9,
        });
    }

    #[test]
    fn tool_loop_family_round_trips() {
        let tool_loop = ModelToolLoopV1 {
            tool_registry_revision_id: "registry-1".to_owned(),
            admission_engine_revision: "admission-1".to_owned(),
            hook_pipeline_revision: "hooks-1".to_owned(),
            active_descriptors: vec![ActiveToolDescriptorSelectionV1 {
                tool_id: "tool-1".to_owned(),
                intended_owner: "mandate".to_owned(),
                descriptor_revision: "descriptor-1".to_owned(),
                input_schema_reference: "input-1".to_owned(),
                result_schema_reference: "result-1".to_owned(),
                required_capability_binding: "provider_tool_group".to_owned(),
                mode_relation: "build".to_owned(),
                model_function_schema_revision: "schema-1".to_owned(),
                safe_result_projection_revision: "projection-1".to_owned(),
                observation_contract_revision: "observation-1".to_owned(),
                stream_shape: "stream".to_owned(),
            }],
            model_tool_loop_required: true,
            translation_revision: "translation-1".to_owned(),
            stream_shape: "stream".to_owned(),
        };
        assert!(tool_loop.validate().is_ok());
        round_trip(&tool_loop);
        round_trip(&ModelToolExchangeDto {
            assistant_ordered_calls: vec!["call-1".to_owned()],
            canonical_call_identities: vec!["identity-1".to_owned()],
            completed_result_records: vec!["record-1".to_owned()],
            safe_model_visible_projections: vec!["projection-1".to_owned()],
        });
        for outcome in [
            ToolTerminalOutcome::Succeeded,
            ToolTerminalOutcome::DeniedBeforeExecution,
            ToolTerminalOutcome::FailedBeforeExternalEffect,
            ToolTerminalOutcome::CancelledBeforeStart,
            ToolTerminalOutcome::InterruptedBeforeStart,
            ToolTerminalOutcome::OutputLimitExceeded,
            ToolTerminalOutcome::ExecutionUnavailable,
            ToolTerminalOutcome::ExternalEffectUnknown,
        ] {
            round_trip(&outcome);
        }
    }

    #[test]
    fn agent_activity_journal_record_limits_accept_at_limit_and_reject_one_over() {
        let record =
            |sequence: u64, safe_user_projection: String, typed_references: Vec<String>| {
                AgentActivityJournalRecordDto {
                    activity_tree_id: AgentActivityTreeId("tree-1".to_owned()),
                    record_id: "record-1".to_owned(),
                    sequence,
                    occurred_at: 100,
                    root_run_reference: "run-1".to_owned(),
                    direct_pair_reference_when_present: Some("pair-1".to_owned()),
                    record_kind: "message".to_owned(),
                    safe_user_projection,
                    typed_references,
                    canonical_record_digest: "a".repeat(64),
                }
            };
        // 64 KiB string-payload bound.
        let base_payload = "tree-1".len()
            + "record-1".len()
            + "run-1".len()
            + "pair-1".len()
            + "message".len()
            + 64;
        let at_limit = 64 * 1024 - base_payload;
        assert!(
            record(0, "a".repeat(at_limit), Vec::new())
                .validate()
                .is_ok()
        );
        assert_eq!(
            record(0, "a".repeat(at_limit + 1), Vec::new())
                .validate()
                .expect_err("oversized record is rejected")
                .code(),
            "agent_activity_record_too_large"
        );

        // The 64 KiB bound counts the UTF-8 bytes of every payload-bearing
        // field, including the typed-reference contents.
        let reference_payload_base = base_payload + "safe".len();
        assert!(
            record(
                0,
                "safe".to_owned(),
                vec!["a".repeat(64 * 1024 - reference_payload_base)],
            )
            .validate()
            .is_ok()
        );
        assert_eq!(
            record(
                0,
                "safe".to_owned(),
                vec!["a".repeat(64 * 1024 - reference_payload_base + 1)],
            )
            .validate()
            .expect_err("oversized reference payload is rejected")
            .code(),
            "agent_activity_record_too_large"
        );
        // Reference bytes are counted alongside the scalar fields: sixteen
        // 1 KiB references leave exactly the remaining budget for the
        // projection.
        let references = vec!["a".repeat(1024); 16];
        let remaining = 64 * 1024 - reference_payload_base - 16 * 1024;
        assert!(
            record(0, "a".repeat(remaining), references)
                .validate()
                .is_ok()
        );

        // 16 typed references.
        let references = |count: usize| (0..count).map(|i| format!("ref-{i}")).collect::<Vec<_>>();
        assert!(
            record(0, "safe".to_owned(), references(16))
                .validate()
                .is_ok()
        );
        assert_eq!(
            record(0, "safe".to_owned(), references(17))
                .validate()
                .expect_err("17 references are rejected")
                .code(),
            "agent_message_reference_invalid"
        );

        // Zero-based sequence below the 4,096-record journal bound.
        assert!(
            record(4095, "safe".to_owned(), Vec::new())
                .validate()
                .is_ok()
        );
        assert_eq!(
            record(4096, "safe".to_owned(), Vec::new())
                .validate()
                .expect_err("sequence 4096 is rejected")
                .code(),
            "agent_activity_journal_limit_exceeded"
        );
    }

    #[test]
    fn closed_capability_enums_serialize_to_stable_names() {
        for (capability, expected) in [
            (
                crate::ProtocolCapabilityDto::ProviderProfilesV1,
                "provider_profiles_v1",
            ),
            (
                crate::ProtocolCapabilityDto::SessionForkV1,
                "session_fork_v1",
            ),
            (
                crate::ProtocolCapabilityDto::NormalizedReasoningStreamV1,
                "normalized_reasoning_stream_v1",
            ),
            (
                crate::ProtocolCapabilityDto::AgentActivityV1,
                "agent_activity_v1",
            ),
            (
                crate::ProtocolCapabilityDto::UserNotificationsV1,
                "user_notifications_v1",
            ),
            (
                crate::ProtocolCapabilityDto::DaemonToolGatewayV1,
                "daemon_tool_gateway_v1",
            ),
            (
                crate::ProtocolCapabilityDto::ModelToolLoopV1,
                "model_tool_loop_v1",
            ),
        ] {
            assert_eq!(
                serde_json::to_string(&capability).expect("capability serializes"),
                format!("\"{expected}\"")
            );
        }
    }

    #[test]
    fn limit_boundaries_accept_at_limit_and_reject_one_over() {
        // Session title: 128 Unicode scalar values.
        assert!(fork_command_with_title("a".repeat(128)).validate().is_ok());
        assert_eq!(
            fork_command_with_title("a".repeat(129))
                .validate()
                .expect_err("129-scalar title is rejected")
                .code(),
            "invalid_title"
        );
        // Counting is by Unicode scalar, not byte.
        assert!(fork_command_with_title("é".repeat(128)).validate().is_ok());
        assert_eq!(
            fork_command_with_title("é".repeat(129))
                .validate()
                .expect_err("129-scalar title is rejected")
                .code(),
            "invalid_title"
        );

        // 16 tool calls per group.
        let calls = |count: usize| (0..count).map(|i| format!("call-{i}")).collect::<Vec<_>>();
        let exchange = |count: usize| ModelToolExchangeDto {
            assistant_ordered_calls: calls(count),
            canonical_call_identities: Vec::new(),
            completed_result_records: Vec::new(),
            safe_model_visible_projections: Vec::new(),
        };
        assert!(exchange(16).validate().is_ok());
        assert_eq!(
            exchange(17)
                .validate()
                .expect_err("17 calls are rejected")
                .code(),
            "provider_tool_group_invalid"
        );

        // 256 facts per history page (reasoning and tool history).
        let facts = |count: usize| (0..count).map(|i| format!("fact-{i}")).collect::<Vec<_>>();
        let reasoning_page = |count: usize| RunReasoningHistoryPageDto {
            session_id: "session-1".to_owned(),
            run_id: "run-1".to_owned(),
            captured_upper_cursor: 5,
            facts: facts(count),
        };
        assert!(reasoning_page(256).validate().is_ok());
        assert_eq!(
            reasoning_page(257)
                .validate()
                .expect_err("257 facts are rejected")
                .code(),
            "provider_reasoning_stream_invalid"
        );
        let tool_page = |count: usize| RunToolHistoryPageDto {
            session_id: "session-1".to_owned(),
            run_id: "run-1".to_owned(),
            captured_upper_cursor: 5,
            facts: facts(count),
        };
        assert!(tool_page(256).validate().is_ok());
        assert_eq!(
            tool_page(257)
                .validate()
                .expect_err("257 facts are rejected")
                .code(),
            "tool_result_stream_invalid"
        );

        // 16 typed references per agent message.
        let mut message = agent_message();
        message.typed_references = (0..16).map(|i| format!("ref-{i}")).collect();
        assert!(message.validate().is_ok());
        message.typed_references = (0..17).map(|i| format!("ref-{i}")).collect();
        assert_eq!(
            message
                .validate()
                .expect_err("17 references are rejected")
                .code(),
            "agent_message_reference_invalid"
        );

        // 64 summaries per conversation tree page.
        let mut page = tree_page();
        page.children = (0..64)
            .map(|i| ConversationBranchSummaryDto {
                session_id: format!("session-{i}"),
                title: None,
            })
            .collect();
        assert!(page.validate().is_ok());
        page.children = (0..65)
            .map(|i| ConversationBranchSummaryDto {
                session_id: format!("session-{i}"),
                title: None,
            })
            .collect();
        assert_eq!(
            page.validate()
                .expect_err("65 summaries are rejected")
                .code(),
            "invalid_conversation_tree_page"
        );
    }

    #[test]
    fn validation_rejects_credentials_endpoints_digests_titles_and_empty_pages() {
        // Credential-shaped values in provider selection fields.
        let mut credential = provider_selection();
        credential.model_id = "sk-key-123".to_owned();
        assert_eq!(
            credential
                .validate()
                .expect_err("key-shaped credential is rejected")
                .code(),
            "credentials_forbidden"
        );
        let mut token = provider_selection();
        token.profile_id = "Bearer Token".to_owned();
        assert_eq!(
            token
                .validate()
                .expect_err("token-shaped credential is rejected")
                .code(),
            "credentials_forbidden"
        );
        let mut newline = provider_selection();
        newline.kind_descriptor_revision_id = "rev\npayload".to_owned();
        assert_eq!(
            newline
                .validate()
                .expect_err("newline-shaped credential is rejected")
                .code(),
            "credentials_forbidden"
        );

        // Invalid endpoints: userinfo, query, fragment, and control characters.
        for endpoint in [
            "https://user:pass@provider.example",
            "https://provider.example?q=1",
            "https://provider.example#frag",
            "https://provider.example\tpath",
        ] {
            let mut selection = provider_selection();
            selection.normalized_effective_endpoint = endpoint.to_owned();
            assert_eq!(
                selection
                    .validate()
                    .expect_err("invalid endpoint is rejected")
                    .code(),
                "invalid_endpoint"
            );
        }

        // The openai kind must be normalized to responses.
        let mut openai = provider_selection();
        openai.kind_id = "openai".to_owned();
        assert_eq!(
            openai
                .validate()
                .expect_err("openai kind is rejected")
                .code(),
            "invalid_provider_kind"
        );

        // Malformed expected_preview_digest values.
        for digest in [
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            "z".repeat(64),
        ] {
            let mut command = fork_command();
            command.expected_preview_digest = digest;
            assert_eq!(
                command
                    .validate()
                    .expect_err("malformed digest is rejected")
                    .code(),
                "invalid_digest"
            );
        }

        // Blank and control-only titles.
        for title in ["   ", "title\u{0000}with-control"] {
            assert_eq!(
                fork_command_with_title(title.to_owned())
                    .validate()
                    .expect_err("invalid title is rejected")
                    .code(),
                "invalid_title"
            );
        }

        // Empty history pages.
        assert_eq!(
            RunReasoningHistoryPageDto {
                session_id: "session-1".to_owned(),
                run_id: "run-1".to_owned(),
                captured_upper_cursor: 5,
                facts: Vec::new(),
            }
            .validate()
            .expect_err("empty reasoning page is rejected")
            .code(),
            "provider_reasoning_stream_invalid"
        );
        assert_eq!(
            RunToolHistoryPageDto {
                session_id: "session-1".to_owned(),
                run_id: "run-1".to_owned(),
                captured_upper_cursor: 5,
                facts: Vec::new(),
            }
            .validate()
            .expect_err("empty tool history page is rejected")
            .code(),
            "tool_result_stream_invalid"
        );
    }

    #[test]
    fn provider_selection_rejects_sk_prefixed_and_bearer_credentials() {
        for value in [
            "sk-123",
            "sk-proj-abc123def456",
            "Bearer abc123",
            "SK-123",
            "bearer abc123",
        ] {
            let mut selection = provider_selection();
            selection.model_id = value.to_owned();
            assert_eq!(
                selection
                    .validate()
                    .expect_err("credential-shaped value is rejected")
                    .code(),
                "credentials_forbidden"
            );
        }
    }

    #[test]
    fn credential_detector_rejects_controls_and_credentials_anywhere() {
        for value in [
            "sk-123",
            "SK-123",
            "model=sk-secret",
            "prefix sk-abc",
            "  sk-abc  ",
            "Bearer secret",
            "bearer\tsecret",
            "BEARER secret",
            "\tBearer secret",
            "Bearer secret trailing",
            "token-value",
            "api-key",
            "line\nbreak",
            "tab\tbreak",
            "carriage\rreturn",
            "\u{0000}control",
        ] {
            assert!(
                credential_shaped(value),
                "{value:?} must be credential-shaped"
            );
        }
        for value in [
            "provider-profiles",
            "model-1",
            "x-safe-header",
            "bearer",
            "bearer ",
            "responses",
        ] {
            assert!(
                !credential_shaped(value),
                "{value:?} must not be credential-shaped"
            );
        }
    }

    #[test]
    fn provider_adjacent_dtos_reject_credentials_in_every_string_field() {
        let mut header = provider_selection();
        header.credential_transport_safe_header_name = Some("sk-header".to_owned());
        assert_eq!(
            header
                .validate()
                .expect_err("credential-shaped header is rejected")
                .code(),
            "credentials_forbidden"
        );
        let mut source = provider_selection();
        source.selection_source = Some("Bearer grant".to_owned());
        assert_eq!(
            source
                .validate()
                .expect_err("credential-shaped selection source is rejected")
                .code(),
            "credentials_forbidden"
        );
        let mut subset = provider_selection();
        subset.declared_model_capability_subset = vec!["sk-capability".to_owned()];
        assert_eq!(
            subset
                .validate()
                .expect_err("credential-shaped capability is rejected")
                .code(),
            "credentials_forbidden"
        );
        let mut profile = profile_revision();
        profile.safe_header_name = Some("token-header".to_owned());
        assert_eq!(
            profile
                .validate()
                .expect_err("credential-shaped profile header is rejected")
                .code(),
            "credentials_forbidden"
        );
    }

    #[test]
    fn reasoning_manifest_and_context_source_entries_validate() {
        let manifest = ReasoningHistoryManifestDto {
            compatibility_id: "compat-1".to_owned(),
            entries: (0..256).map(|i| format!("entry-{i}")).collect(),
            manifest_digest: "a".repeat(64),
        };
        assert!(manifest.validate().is_ok());
        let mut blank_compatibility = manifest.clone();
        blank_compatibility.compatibility_id = "   ".to_owned();
        assert_eq!(
            blank_compatibility
                .validate()
                .expect_err("blank compatibility id is rejected")
                .code(),
            "reasoning_history_manifest_invalid"
        );
        let mut too_many = manifest.clone();
        too_many.entries.push("entry-256".to_owned());
        assert_eq!(
            too_many
                .validate()
                .expect_err("257 entries are rejected")
                .code(),
            "reasoning_history_manifest_invalid"
        );
        let mut blank_entry = manifest.clone();
        blank_entry.entries = vec!["   ".to_owned()];
        assert_eq!(
            blank_entry
                .validate()
                .expect_err("blank entry is rejected")
                .code(),
            "reasoning_history_manifest_invalid"
        );
        let mut bad_digest = manifest;
        bad_digest.manifest_digest = "z".repeat(64);
        assert_eq!(
            bad_digest
                .validate()
                .expect_err("malformed digest is rejected")
                .code(),
            "invalid_digest"
        );

        let entry = |source_id: String, safe_label: Option<String>| ContextSourceEntryV1 {
            source_id,
            source_kind: "goal".to_owned(),
            revision: "rev-1".to_owned(),
            safe_label,
        };
        let context_manifest =
            |source_entries: Vec<ContextSourceEntryV1>| ContextSourceManifestV1 {
                compatibility_id: "compat-1".to_owned(),
                source_entries,
                manifest_digest: "a".repeat(64),
            };
        assert!(
            context_manifest(vec![entry("source-1".to_owned(), None)])
                .validate()
                .is_ok()
        );
        assert_eq!(
            context_manifest(vec![entry("   ".to_owned(), None)])
                .validate()
                .expect_err("blank entry field is rejected")
                .code(),
            "context_source_manifest_invalid"
        );
        assert_eq!(
            context_manifest(vec![entry("source-1".to_owned(), Some("   ".to_owned()))])
                .validate()
                .expect_err("blank safe label is rejected")
                .code(),
            "context_source_manifest_invalid"
        );
        assert_eq!(
            context_manifest(vec![entry("source\u{0000}-1".to_owned(), None)])
                .validate()
                .expect_err("control-bearing entry field is rejected")
                .code(),
            "context_source_manifest_invalid"
        );
    }

    #[test]
    fn agent_message_and_notification_records_validate_text_fields() {
        let mut message = agent_message();
        assert!(message.validate().is_ok());
        message.delivery_state = "   ".to_owned();
        assert_eq!(
            message
                .validate()
                .expect_err("blank message field is rejected")
                .code(),
            "agent_message_invalid"
        );
        let mut message = agent_message();
        message.safe_text = Some("text\u{0000}control".to_owned());
        assert_eq!(
            message
                .validate()
                .expect_err("control-bearing safe text is rejected")
                .code(),
            "agent_message_invalid"
        );
        let mut message = agent_message();
        message.canonical_message_digest = "A".repeat(64);
        assert_eq!(
            message
                .validate()
                .expect_err("malformed digest is rejected")
                .code(),
            "invalid_digest"
        );

        let notification =
            |reason: String, safe_counts_and_states: String| AgentNotificationRecordDto {
                notification_cursor: AgentNotificationCursorDto(4),
                activity_tree_id: AgentActivityTreeId("tree-1".to_owned()),
                activity_record_reference: "record-1".to_owned(),
                level: NotificationLevel::Ordinary,
                reason,
                safe_counts_and_states,
                occurred_at: 100,
            };
        assert!(
            notification("reason".to_owned(), "counts".to_owned())
                .validate()
                .is_ok()
        );
        assert_eq!(
            notification("  ".to_owned(), "counts".to_owned())
                .validate()
                .expect_err("blank reason is rejected")
                .code(),
            "agent_notification_record_invalid"
        );
        let mut blank_tree = notification("reason".to_owned(), "counts".to_owned());
        blank_tree.activity_tree_id = AgentActivityTreeId("   ".to_owned());
        assert_eq!(
            blank_tree
                .validate()
                .expect_err("blank tree id is rejected")
                .code(),
            "agent_notification_record_invalid"
        );
    }

    #[test]
    fn bridge_dtos_validate_text_digests_and_references() {
        let grant = BridgeRunGrantDto {
            opaque_grant_identity: "grant-1".to_owned(),
            issued_protocol_revision: "rev-1".to_owned(),
        };
        assert!(grant.validate().is_ok());
        let command = BridgeInvocationCommandDto {
            bridge_run_grant: grant.clone(),
            bridge_operation_id: "operation-1".to_owned(),
            typed_tool_invocation: "invocation".to_owned(),
        };
        assert!(command.validate().is_ok());
        let mut credential_grant = grant;
        credential_grant.opaque_grant_identity = "sk-grant".to_owned();
        assert_eq!(
            BridgeInvocationCommandDto {
                bridge_run_grant: credential_grant,
                bridge_operation_id: "operation-1".to_owned(),
                typed_tool_invocation: "invocation".to_owned(),
            }
            .validate()
            .expect_err("credential-shaped grant is rejected")
            .code(),
            "credentials_forbidden"
        );
        let mut blank_command = command;
        blank_command.typed_tool_invocation = "   ".to_owned();
        assert_eq!(
            blank_command
                .validate()
                .expect_err("blank command field is rejected")
                .code(),
            "bridge_invocation_invalid"
        );

        let accepted = BridgeInvocationAcceptedDto {
            bridge_operation_id: "operation-1".to_owned(),
            tool_call_id: "call-1".to_owned(),
            admission_state: "admitted".to_owned(),
        };
        assert!(accepted.validate().is_ok());
        let mut blank_accepted = accepted;
        blank_accepted.admission_state = "admitted\u{0000}".to_owned();
        assert_eq!(
            blank_accepted
                .validate()
                .expect_err("control-bearing acceptance field is rejected")
                .code(),
            "bridge_invocation_invalid"
        );

        let operation = BridgeOperationV1 {
            bridge_operation_id: "operation-1".to_owned(),
            run_id: "run-1".to_owned(),
            mandate_id: "mandate-1".to_owned(),
            mandate_revision: "rev-1".to_owned(),
            model_step_id: "step-1".to_owned(),
            tool_id: "tool-1".to_owned(),
            descriptor_revision: "descriptor-1".to_owned(),
            typed_input_digest: "a".repeat(64),
            tool_call_id: "call-1".to_owned(),
            admission_outcome: "admitted".to_owned(),
            attempt_reference: Some("attempt-1".to_owned()),
        };
        assert!(operation.validate().is_ok());
        let mut bad_digest = operation.clone();
        bad_digest.typed_input_digest = "b".repeat(63);
        assert_eq!(
            bad_digest
                .validate()
                .expect_err("malformed typed input digest is rejected")
                .code(),
            "invalid_digest"
        );
        let mut credential_attempt = operation;
        credential_attempt.attempt_reference = Some("Bearer attempt".to_owned());
        assert_eq!(
            credential_attempt
                .validate()
                .expect_err("credential-shaped attempt reference is rejected")
                .code(),
            "credentials_forbidden"
        );
    }

    #[test]
    fn model_tool_loop_and_active_descriptors_validate() {
        let active_descriptor = |index: usize| ActiveToolDescriptorSelectionV1 {
            tool_id: format!("tool-{index}"),
            intended_owner: "mandate".to_owned(),
            descriptor_revision: "descriptor-1".to_owned(),
            input_schema_reference: "input-1".to_owned(),
            result_schema_reference: "result-1".to_owned(),
            required_capability_binding: "provider_tool_group".to_owned(),
            mode_relation: "build".to_owned(),
            model_function_schema_revision: "schema-1".to_owned(),
            safe_result_projection_revision: "projection-1".to_owned(),
            observation_contract_revision: "observation-1".to_owned(),
            stream_shape: "stream".to_owned(),
        };
        let tool_loop =
            |active_descriptors: Vec<ActiveToolDescriptorSelectionV1>| ModelToolLoopV1 {
                tool_registry_revision_id: "registry-1".to_owned(),
                admission_engine_revision: "admission-1".to_owned(),
                hook_pipeline_revision: "hooks-1".to_owned(),
                active_descriptors,
                model_tool_loop_required: true,
                translation_revision: "translation-1".to_owned(),
                stream_shape: "stream".to_owned(),
            };
        assert!(
            tool_loop((0..16).map(active_descriptor).collect())
                .validate()
                .is_ok()
        );
        assert_eq!(
            tool_loop((0..17).map(active_descriptor).collect())
                .validate()
                .expect_err("17 active descriptors are rejected")
                .code(),
            "model_tool_loop_invalid"
        );
        let mut blank = tool_loop(Vec::new());
        blank.translation_revision = "   ".to_owned();
        assert_eq!(
            blank
                .validate()
                .expect_err("blank loop field is rejected")
                .code(),
            "model_tool_loop_invalid"
        );
        let mut credential = tool_loop(Vec::new());
        credential.hook_pipeline_revision = "sk-hooks".to_owned();
        assert_eq!(
            credential
                .validate()
                .expect_err("credential-shaped loop field is rejected")
                .code(),
            "credentials_forbidden"
        );
        let mut descriptor_credential = active_descriptor(0);
        descriptor_credential.tool_id = "Bearer tool".to_owned();
        assert_eq!(
            descriptor_credential
                .validate()
                .expect_err("credential-shaped descriptor field is rejected")
                .code(),
            "credentials_forbidden"
        );
        let mut descriptor_blank = active_descriptor(0);
        descriptor_blank.mode_relation = "build\u{0001}".to_owned();
        assert_eq!(
            descriptor_blank
                .validate()
                .expect_err("control-bearing descriptor field is rejected")
                .code(),
            "model_tool_loop_invalid"
        );
    }

    #[test]
    fn fork_preview_and_notification_limits_accept_at_limit_and_reject_one_over() {
        // Fork preview: 1 MiB snapshot bound and 64-page preview bound.
        let preview = |snapshot_size_bytes: u64, page_count: u32| ForkPreviewDto {
            preview_digest: "a".repeat(64),
            page_count,
            snapshot_size_bytes,
        };
        assert!(preview(1024 * 1024, 64).validate().is_ok());
        assert_eq!(
            preview(1024 * 1024 + 1, 1)
                .validate()
                .expect_err("oversized snapshot is rejected")
                .code(),
            "fork_snapshot_too_large"
        );
        assert_eq!(
            preview(1, 65)
                .validate()
                .expect_err("over-page preview is rejected")
                .code(),
            "fork_snapshot_too_large"
        );

        // Notification aggregate: 64 KiB per-page byte budget.
        let notification = |payload: String| AgentNotificationRecordDto {
            notification_cursor: AgentNotificationCursorDto(4),
            activity_tree_id: AgentActivityTreeId("tree-1".to_owned()),
            activity_record_reference: "record-1".to_owned(),
            level: NotificationLevel::Ordinary,
            reason: "reason".to_owned(),
            safe_counts_and_states: payload,
            occurred_at: 100,
        };
        let at_limit = 64 * 1024 - ("record-1".len() + "reason".len());
        assert!(notification("a".repeat(at_limit)).validate().is_ok());
        assert_eq!(
            notification("a".repeat(at_limit + 1))
                .validate()
                .expect_err("oversized notification is rejected")
                .code(),
            "agent_notification_summary_too_large"
        );
    }

    #[test]
    fn fake_secret_never_enters_serialized_provider_selection_or_committed_fixtures() {
        const FAKE_SECRET: &str = "fixture-fake-secret-w2-001";

        // Provider-selection test input: the caller supplies a recognizable
        // fake credential value, while the credential-free selection DTO only
        // names a transport header. The serialized JSON of the DTO must never
        // contain the credential itself.
        let caller_credential_input = FAKE_SECRET;
        let selection = provider_selection();
        assert_eq!(
            selection.credential_transport_safe_header_name.as_deref(),
            Some("x-safe-header")
        );
        let serialized = serde_json::to_string(&selection).expect("provider selection serializes");
        assert!(
            !serialized.contains(caller_credential_input),
            "serialized provider selection must not contain the fake secret"
        );
        let decoded: ResolvedRunProviderSelectionDto =
            serde_json::from_str(&serialized).expect("provider selection wire decodes");
        assert_eq!(decoded, selection);
        assert!(decoded.validate().is_ok());

        // No committed golden fixture may embed the fake secret.
        for fixture in [
            include_str!("../tests/fixtures/goldens/hello-current-version-v1.json"),
            include_str!("../tests/fixtures/goldens/hello-incompatible-major-v2.json"),
            include_str!("../tests/fixtures/goldens/hello-unnegotiated-capability-v1.json"),
        ] {
            assert!(
                !fixture.contains(FAKE_SECRET),
                "committed fixture must not contain the fake secret"
            );
        }
    }

    fn profile_revision() -> ProviderProfileRevisionV1 {
        ProviderProfileRevisionV1 {
            profile_id: "profile-1".to_owned(),
            revision_id: "rev-1".to_owned(),
            provider_kind_id: "responses".to_owned(),
            model_id: "model-1".to_owned(),
            endpoint: "https://provider.example".to_owned(),
            credential_transport_mode: CredentialTransportMode::SafeHeader,
            safe_header_name: Some("x-safe-header".to_owned()),
            capability_taxonomy_revision: "model-capability-taxonomy-v1".to_owned(),
            reasoning_compatibility_id: Some("compat-1".to_owned()),
        }
    }

    fn base_snapshot_v1() -> ForkBaseSnapshotV1 {
        ForkBaseSnapshotV1 {
            schema_version: "1".to_owned(),
            context_schema_version: "1".to_owned(),
            source_session_id: "source".to_owned(),
            conversation_tree_id: "tree-1".to_owned(),
            boundary: ForkBoundary::CommittedUserTurn {
                source_turn_id: "turn-1".to_owned(),
                accepted_sequence: 7,
            },
            source_boundary_sequence: 7,
            source_run_cursors: vec![3, 5],
            effective_instruction_projection: "instruction".to_owned(),
            materialized_model_messages: vec!["message-1".to_owned()],
            inherited_future_defaults: vec!["default-1".to_owned()],
            historical_config_policy_references: vec!["policy-1".to_owned()],
            safe_usage_provenance: vec!["usage-1".to_owned()],
            terminal_tool_result_references: vec!["tool-1".to_owned()],
            policy_decision_references: vec!["decision-1".to_owned()],
            terminal_child_result_references: vec!["child-1".to_owned()],
            workspace_state: "unverified".to_owned(),
        }
    }

    fn base_snapshot_v2() -> ForkBaseSnapshotV2 {
        ForkBaseSnapshotV2 {
            schema_version: "2".to_owned(),
            context_schema_version: "1".to_owned(),
            source_session_id: "source".to_owned(),
            conversation_tree_id: "tree-1".to_owned(),
            boundary: ForkBoundary::CommittedUserTurn {
                source_turn_id: "turn-1".to_owned(),
                accepted_sequence: 7,
            },
            source_boundary_sequence: 7,
            source_run_cursors: vec![3, 5],
            effective_instruction_projection: "instruction".to_owned(),
            materialized_model_messages: vec!["message-1".to_owned()],
            inherited_future_defaults: vec!["default-1".to_owned()],
            historical_config_policy_references: vec!["policy-1".to_owned()],
            inherited_reasoning_history_references: vec!["reasoning-1".to_owned()],
            safe_usage_provenance: vec!["usage-1".to_owned()],
            terminal_tool_result_references: vec!["tool-1".to_owned()],
            policy_decision_references: vec!["decision-1".to_owned()],
            terminal_child_result_references: vec!["child-1".to_owned()],
            workspace_state: "unverified".to_owned(),
        }
    }

    fn preview_v1() -> ForkPreviewV1 {
        ForkPreviewV1 {
            preview_digest: "a".repeat(64),
            source_head_sequence: 7,
            page_count: 3,
            snapshot_size_bytes: 4096,
            workspace_state: "unverified".to_owned(),
        }
    }

    fn preview_v2() -> ForkPreviewV2 {
        ForkPreviewV2 {
            preview_digest: "a".repeat(64),
            source_head_sequence: 7,
            page_count: 3,
            snapshot_size_bytes: 4096,
            inherited_reasoning_history_references: vec!["reasoning-1".to_owned()],
            workspace_state: "unverified".to_owned(),
        }
    }

    fn frozen_limits() -> AgentActivityLimitsV1 {
        AgentActivityLimitsV1 {
            max_messages: 1024,
            max_aggregate_bytes: 4 * 1024 * 1024,
            max_journal_records: 4096,
            max_record_bytes: 64 * 1024,
            max_page_records: 256,
            max_page_bytes: 512 * 1024,
            max_references: 16,
        }
    }

    fn tool_descriptor(index: usize) -> ToolDescriptorRevision {
        ToolDescriptorRevision {
            tool_id: format!("tool-{index}"),
            descriptor_revision: "descriptor-1".to_owned(),
            intended_owner: "mandate".to_owned(),
            input_schema_reference: "input-1".to_owned(),
            result_schema_reference: "result-1".to_owned(),
            required_capability_binding: "provider_tool_group".to_owned(),
            mode_relation: "build".to_owned(),
            model_function_schema_revision: "schema-1".to_owned(),
            safe_result_projection_revision: "projection-1".to_owned(),
            observation_contract_revision: "observation-1".to_owned(),
            stream_shape: "stream".to_owned(),
        }
    }

    #[test]
    fn provider_profile_revision_family_round_trips_and_validates() {
        let revision = profile_revision();
        assert!(revision.validate().is_ok());
        round_trip(&revision);
        let mut bearer = revision;
        bearer.credential_transport_mode = CredentialTransportMode::Bearer;
        round_trip(&bearer);
    }

    #[test]
    fn provider_profile_revision_validation_rejects_invalid_fields() {
        let too_long = "a".repeat(257);
        let control = "a\u{0000}b";
        for field in ["   ", too_long.as_str(), control] {
            let mut revision = profile_revision();
            revision.profile_id = field.to_owned();
            assert_eq!(
                revision
                    .validate()
                    .expect_err("invalid profile field is rejected")
                    .code(),
                "provider_profile_revision_invalid"
            );
        }
        for endpoint in [
            "https://provider.example?q=1",
            "https://provider.example#frag",
            "https://user@provider.example",
        ] {
            let mut revision = profile_revision();
            revision.endpoint = endpoint.to_owned();
            assert_eq!(
                revision
                    .validate()
                    .expect_err("invalid endpoint is rejected")
                    .code(),
                "invalid_endpoint"
            );
        }
        let long_header = "a".repeat(129);
        for header in ["   ", long_header.as_str(), "header\u{0000}name"] {
            let mut revision = profile_revision();
            revision.safe_header_name = Some(header.to_owned());
            assert_eq!(
                revision
                    .validate()
                    .expect_err("invalid header name is rejected")
                    .code(),
                "provider_profile_revision_invalid"
            );
        }
        for value in ["sk-123", "Bearer abc123", "token-value"] {
            let mut revision = profile_revision();
            revision.model_id = value.to_owned();
            assert_eq!(
                revision
                    .validate()
                    .expect_err("credential-shaped value is rejected")
                    .code(),
                "credentials_forbidden"
            );
        }
    }

    #[test]
    fn fork_profile_override_pairs_validate_and_round_trip() {
        // Presence flag without a value is invalid.
        let mut command = fork_command();
        command.future_profile_override_present = true;
        command.future_profile_override = None;
        command.expected_profile_revision = None;
        assert_eq!(
            command
                .validate()
                .expect_err("flag without value is rejected")
                .code(),
            "provider_profile_override_invalid"
        );
        // Expected revision without an override is invalid.
        let mut command = fork_command();
        command.future_profile_override_present = true;
        command.future_profile_override = Some("profile-2".to_owned());
        command.expected_profile_revision = Some("rev-1".to_owned());
        assert!(command.validate().is_ok());
        let mut command = fork_command();
        command.future_profile_override_present = false;
        command.future_profile_override = None;
        command.expected_profile_revision = Some("rev-1".to_owned());
        assert_eq!(
            command
                .validate()
                .expect_err("expected revision without override is rejected")
                .code(),
            "provider_profile_override_invalid"
        );
        // Overlong and credential-shaped values are rejected.
        let mut command = fork_command();
        command.expected_profile_revision = Some("p".repeat(64));
        assert_eq!(
            command
                .validate()
                .expect_err("overlong revision is rejected")
                .code(),
            "provider_profile_override_invalid"
        );
        let mut command = fork_command();
        command.expected_profile_revision = Some("api-key-rev".to_owned());
        assert_eq!(
            command
                .validate()
                .expect_err("credential-shaped revision is rejected")
                .code(),
            "credentials_forbidden"
        );
        // The builder keeps the override pair coherent and round-trips.
        let built = fork_command()
            .with_profile_override(Some("profile-2".to_owned()), Some("rev-1".to_owned()))
            .expect("builder override pair is valid");
        assert!(built.future_profile_override_present);
        assert_eq!(built.future_profile_override.as_deref(), Some("profile-2"));
        assert_eq!(built.expected_profile_revision.as_deref(), Some("rev-1"));
        round_trip(&built);
        let start = StartForkRunCommandDto {
            session_id: "session-1".to_owned(),
            profile_override: Some("profile-2".to_owned()),
            expected_profile_revision: None,
        };
        assert!(start.validate().is_ok());
        let mut start = start;
        start.expected_profile_revision = Some("rev-1".to_owned());
        assert!(start.validate().is_ok());
        start.profile_override = None;
        assert_eq!(
            start
                .validate()
                .expect_err("revision without override is rejected")
                .code(),
            "provider_profile_override_invalid"
        );
    }
    #[test]
    fn fork_base_snapshot_families_round_trip_and_validate() {
        let v1 = base_snapshot_v1();
        assert!(v1.validate().is_ok());
        round_trip(&v1);
        let v2 = base_snapshot_v2();
        assert!(v2.validate().is_ok());
        round_trip(&v2);
    }

    #[test]
    fn fork_base_snapshot_limits_accept_at_limit_and_reject_one_over() {
        let mut snapshot = base_snapshot_v1();
        let entry = "a".repeat(1024);
        snapshot.materialized_model_messages = (0..1024).map(|_| entry.clone()).collect();
        assert!(snapshot.validate().is_ok());
        snapshot.materialized_model_messages.push("a".to_owned());
        assert_eq!(
            snapshot
                .validate()
                .expect_err("1,025 messages are rejected")
                .code(),
            "fork_snapshot_too_large"
        );
        let mut over_aggregate = base_snapshot_v1();
        over_aggregate.materialized_model_messages = (0..1024).map(|_| entry.clone()).collect();
        over_aggregate.materialized_model_messages[0] = "a".repeat(1025);
        assert_eq!(
            over_aggregate
                .validate()
                .expect_err("aggregate over 1 MiB is rejected")
                .code(),
            "fork_snapshot_too_large"
        );

        let mut v2 = base_snapshot_v2();
        v2.inherited_reasoning_history_references =
            (0..4096).map(|_| "reasoning".to_owned()).collect();
        assert!(v2.validate().is_ok());
        v2.inherited_reasoning_history_references
            .push("reasoning".to_owned());
        assert_eq!(
            v2.validate()
                .expect_err("4,097 references are rejected")
                .code(),
            "fork_snapshot_too_large"
        );
        let mut over_references = base_snapshot_v2();
        over_references.inherited_reasoning_history_references =
            (0..4096).map(|_| "a".repeat(256)).collect();
        assert!(over_references.validate().is_ok());
        over_references.inherited_reasoning_history_references[0] = "a".repeat(257);
        assert_eq!(
            over_references
                .validate()
                .expect_err("reference aggregate over 1 MiB is rejected")
                .code(),
            "fork_snapshot_too_large"
        );
        let mut malformed = base_snapshot_v2();
        malformed.inherited_reasoning_history_references = vec!["   ".to_owned()];
        assert_eq!(
            malformed
                .validate()
                .expect_err("blank reference is rejected")
                .code(),
            "fork_reference_unavailable"
        );
        let mut unsupported = base_snapshot_v1();
        unsupported.workspace_state = "verified".to_owned();
        assert_eq!(
            unsupported
                .validate()
                .expect_err("non-unverified workspace state is rejected")
                .code(),
            "fork_snapshot_unsupported"
        );
    }

    #[test]
    fn fork_preview_families_round_trip_validate_and_bridge_to_fork_preview_dto() {
        let v1 = preview_v1();
        assert!(v1.validate().is_ok());
        round_trip(&v1);
        let v2 = preview_v2();
        assert!(v2.validate().is_ok());
        round_trip(&v2);

        // Lossless bridging: every dropped field is at its default or absent
        // value, so no preview data is lost.
        let mut lossless_v1 = v1.clone();
        lossless_v1.source_head_sequence = 0;
        let bridged_v1: ForkPreviewDto = lossless_v1
            .try_into()
            .expect("lossless v1 preview conversion");
        assert_eq!(bridged_v1.preview_digest, "a".repeat(64));
        assert_eq!(bridged_v1.page_count, 3);
        assert_eq!(bridged_v1.snapshot_size_bytes, 4096);
        let mut lossless_v2 = v2.clone();
        lossless_v2.source_head_sequence = 0;
        lossless_v2.inherited_reasoning_history_references = Vec::new();
        let bridged_v2: ForkPreviewDto = lossless_v2
            .try_into()
            .expect("lossless v2 preview conversion");
        assert_eq!(bridged_v2, bridged_v1);

        // Lossy conversions fail closed with a stable code.
        assert_eq!(
            ForkPreviewDto::try_from(v1)
                .expect_err("nonzero head sequence is lossy")
                .code(),
            "fork_preview_conversion_lossy"
        );
        assert_eq!(
            ForkPreviewDto::try_from(v2)
                .expect_err("inherited references are lossy")
                .code(),
            "fork_preview_conversion_lossy"
        );
        let mut non_unverified = preview_v1();
        non_unverified.workspace_state = "verified".to_owned();
        assert_eq!(
            ForkPreviewDto::try_from(non_unverified)
                .expect_err("non-default workspace state is lossy")
                .code(),
            "fork_preview_conversion_lossy"
        );
    }

    #[test]
    fn fork_preview_validation_rejects_invalid_digests_bounds_and_workspace_state() {
        for digest in ["a".repeat(63), "A".repeat(64)] {
            let mut preview = preview_v1();
            preview.preview_digest = digest;
            assert_eq!(
                preview
                    .validate()
                    .expect_err("malformed digest is rejected")
                    .code(),
                "invalid_digest"
            );
        }
        let mut large = preview_v1();
        large.snapshot_size_bytes = 1024 * 1024 + 1;
        assert_eq!(
            large
                .validate()
                .expect_err("oversized snapshot is rejected")
                .code(),
            "fork_snapshot_too_large"
        );
        let mut many_pages = preview_v1();
        many_pages.page_count = 65;
        assert_eq!(
            many_pages
                .validate()
                .expect_err("over-page preview is rejected")
                .code(),
            "fork_snapshot_too_large"
        );
        let mut unsupported = preview_v2();
        unsupported.workspace_state = "verified".to_owned();
        assert_eq!(
            unsupported
                .validate()
                .expect_err("non-unverified workspace state is rejected")
                .code(),
            "fork_snapshot_unsupported"
        );
        let mut malformed = preview_v2();
        malformed.inherited_reasoning_history_references = vec!["".to_owned()];
        assert_eq!(
            malformed
                .validate()
                .expect_err("blank reference is rejected")
                .code(),
            "fork_reference_unavailable"
        );
    }

    #[test]
    fn context_source_manifest_family_round_trips_and_validates() {
        let manifest = ContextSourceManifestV1 {
            compatibility_id: "compat-1".to_owned(),
            source_entries: vec![ContextSourceEntryV1 {
                source_id: "source-1".to_owned(),
                source_kind: "goal".to_owned(),
                revision: "rev-1".to_owned(),
                safe_label: Some("label".to_owned()),
            }],
            manifest_digest: "a".repeat(64),
        };
        assert!(manifest.validate().is_ok());
        round_trip(&manifest);
        round_trip(&manifest.source_entries[0]);
        for entries in [
            Vec::new(),
            (0..257)
                .map(|i| ContextSourceEntryV1 {
                    source_id: format!("source-{i}"),
                    source_kind: "goal".to_owned(),
                    revision: "rev-1".to_owned(),
                    safe_label: None,
                })
                .collect(),
        ] {
            let mut invalid = manifest.clone();
            invalid.source_entries = entries;
            assert_eq!(
                invalid
                    .validate()
                    .expect_err("out-of-bounds entries are rejected")
                    .code(),
                "context_source_manifest_invalid"
            );
        }
        let mut bad_digest = manifest;
        bad_digest.manifest_digest = "A".repeat(64);
        assert_eq!(
            bad_digest
                .validate()
                .expect_err("malformed digest is rejected")
                .code(),
            "invalid_digest"
        );
    }

    #[test]
    fn model_context_projection_round_trips_and_validates() {
        let projection = ModelContextProjectionV1 {
            projection_revision: "projection-1".to_owned(),
            context_schema_version: "1".to_owned(),
            source_manifest_digest: "a".repeat(64),
            ordered_messages: vec!["message-1".to_owned(), "message-2".to_owned()],
            model_context_digest: "a".repeat(64),
        };
        assert!(projection.validate().is_ok());
        round_trip(&projection);
    }

    #[test]
    fn model_context_projection_validation_rejects_invalid_bounds_and_digests() {
        let projection = |ordered_messages: Vec<String>| ModelContextProjectionV1 {
            projection_revision: "projection-1".to_owned(),
            context_schema_version: "1".to_owned(),
            source_manifest_digest: "a".repeat(64),
            ordered_messages,
            model_context_digest: "a".repeat(64),
        };
        assert_eq!(
            projection(Vec::new())
                .validate()
                .expect_err("empty projection is rejected")
                .code(),
            "model_context_projection_invalid"
        );
        assert_eq!(
            projection((0..1025).map(|i| format!("message-{i}")).collect())
                .validate()
                .expect_err("1,025 messages are rejected")
                .code(),
            "model_context_projection_invalid"
        );
        assert_eq!(
            projection(vec!["   ".to_owned()])
                .validate()
                .expect_err("blank message is rejected")
                .code(),
            "model_context_projection_invalid"
        );
        assert_eq!(
            projection(vec!["a".repeat(1024 * 1024 + 1)])
                .validate()
                .expect_err("aggregate over 1 MiB is rejected")
                .code(),
            "model_context_projection_too_large"
        );
        let mut bad_source_digest = projection(vec!["message".to_owned()]);
        bad_source_digest.source_manifest_digest = "b".repeat(63);
        assert_eq!(
            bad_source_digest
                .validate()
                .expect_err("malformed source digest is rejected")
                .code(),
            "invalid_digest"
        );
        let mut bad_digest = projection(vec!["message".to_owned()]);
        bad_digest.model_context_digest = "b".repeat(65);
        assert_eq!(
            bad_digest
                .validate()
                .expect_err("malformed digest is rejected")
                .code(),
            "invalid_digest"
        );
    }

    #[test]
    fn agent_activity_limits_require_the_frozen_ledger_values() {
        let limits = frozen_limits();
        assert!(limits.validate().is_ok());
        round_trip(&limits);

        let mut changed = limits;
        changed.max_messages = 1023;
        assert_eq!(
            changed
                .validate()
                .expect_err("altered message limit is rejected")
                .code(),
            "agent_activity_limits_invalid"
        );
        let mut changed = limits;
        changed.max_aggregate_bytes = 4 * 1024 * 1024 - 1;
        assert_eq!(
            changed
                .validate()
                .expect_err("altered aggregate limit is rejected")
                .code(),
            "agent_activity_limits_invalid"
        );
        let mut changed = limits;
        changed.max_journal_records = 4097;
        assert_eq!(
            changed
                .validate()
                .expect_err("altered journal limit is rejected")
                .code(),
            "agent_activity_limits_invalid"
        );
        let mut changed = limits;
        changed.max_record_bytes = 64 * 1024 + 1;
        assert_eq!(
            changed
                .validate()
                .expect_err("altered record limit is rejected")
                .code(),
            "agent_activity_limits_invalid"
        );
        let mut changed = limits;
        changed.max_page_records = 255;
        assert_eq!(
            changed
                .validate()
                .expect_err("altered page record limit is rejected")
                .code(),
            "agent_activity_limits_invalid"
        );
        let mut changed = limits;
        changed.max_page_bytes = 512 * 1024 - 1;
        assert_eq!(
            changed
                .validate()
                .expect_err("altered page byte limit is rejected")
                .code(),
            "agent_activity_limits_invalid"
        );
        let mut changed = limits;
        changed.max_references = 15;
        assert_eq!(
            changed
                .validate()
                .expect_err("altered reference limit is rejected")
                .code(),
            "agent_activity_limits_invalid"
        );
    }

    #[test]
    fn agent_activity_tree_and_pair_round_trip_and_validate() {
        let tree = AgentActivityTreeV1 {
            activity_tree_id: AgentActivityTreeId("tree-1".to_owned()),
            root_run_reference: "run-1".to_owned(),
            activity_exchange_revision: "exchange-1".to_owned(),
            activity_journal_revision: "journal-1".to_owned(),
            user_projection_revision: "projection-1".to_owned(),
            fixed_limits: frozen_limits(),
        };
        assert!(tree.validate().is_ok());
        round_trip(&tree);

        let pair = AgentActivityPairV1 {
            pair_id: "pair-1".to_owned(),
            activity_tree_id: AgentActivityTreeId("tree-1".to_owned()),
            parent_run_reference: "run-1".to_owned(),
            child_run_reference: "run-2".to_owned(),
            activity_exchange_revision: "exchange-1".to_owned(),
            activity_journal_revision: "journal-1".to_owned(),
            user_projection_revision: "projection-1".to_owned(),
            fixed_limits: frozen_limits(),
        };
        assert!(pair.validate().is_ok());
        round_trip(&pair);

        let mut same_runs = pair;
        same_runs.child_run_reference = "run-1".to_owned();
        assert_eq!(
            same_runs
                .validate()
                .expect_err("equal run references are rejected")
                .code(),
            "agent_activity_pair_invalid"
        );

        let mut blank_tree = tree.clone();
        blank_tree.root_run_reference = "   ".to_owned();
        assert_eq!(
            blank_tree
                .validate()
                .expect_err("blank tree field is rejected")
                .code(),
            "agent_activity_tree_invalid"
        );

        let mut bad_limits_tree = tree;
        bad_limits_tree.fixed_limits.max_references = 15;
        assert_eq!(
            bad_limits_tree
                .validate()
                .expect_err("altered limits are rejected")
                .code(),
            "agent_activity_limits_invalid"
        );
    }

    #[test]
    fn tool_descriptor_and_registry_revisions_round_trip_and_validate() {
        let descriptor = tool_descriptor(0);
        assert!(descriptor.validate().is_ok());
        round_trip(&descriptor);

        let registry = ToolRegistryRevision {
            registry_revision_id: "registry-1".to_owned(),
            descriptors: vec![tool_descriptor(0), tool_descriptor(1)],
            admission_engine_revision: "admission-1".to_owned(),
            hook_pipeline_revision: "hooks-1".to_owned(),
        };
        assert!(registry.validate().is_ok());
        round_trip(&registry);

        let mut blank = descriptor.clone();
        blank.mode_relation = "   ".to_owned();
        assert_eq!(
            blank
                .validate()
                .expect_err("blank descriptor field is rejected")
                .code(),
            "tool_descriptor_revision_invalid"
        );
        let mut credential = descriptor;
        credential.tool_id = "sk-key-123".to_owned();
        assert_eq!(
            credential
                .validate()
                .expect_err("credential-shaped value is rejected")
                .code(),
            "credentials_forbidden"
        );
        let mut duplicate = registry;
        duplicate.descriptors = vec![tool_descriptor(0), tool_descriptor(0)];
        assert_eq!(
            duplicate
                .validate()
                .expect_err("duplicate tool id is rejected")
                .code(),
            "duplicate_tool_descriptor"
        );
    }

    #[test]
    fn tool_registry_revision_bounds_accept_at_limit_and_reject_one_over() {
        let small_registry = |count: usize| ToolRegistryRevision {
            registry_revision_id: "registry-1".to_owned(),
            descriptors: (0..count).map(tool_descriptor).collect(),
            admission_engine_revision: "admission-1".to_owned(),
            hook_pipeline_revision: "hooks-1".to_owned(),
        };
        assert!(small_registry(256).validate().is_ok());
        assert_eq!(
            small_registry(257)
                .validate()
                .expect_err("257 descriptors are rejected")
                .code(),
            "tool_registry_revision_too_large"
        );

        // Max-size descriptors: each carries 256 chars in ten fields plus an
        // eight-char tool ID, so 204 fit within 512 KiB and 205 do not.
        let max_size_descriptor = |index: usize| ToolDescriptorRevision {
            tool_id: format!("tool-{index:0>4}"),
            descriptor_revision: "a".repeat(256),
            intended_owner: "a".repeat(256),
            input_schema_reference: "a".repeat(256),
            result_schema_reference: "a".repeat(256),
            required_capability_binding: "a".repeat(256),
            mode_relation: "a".repeat(256),
            model_function_schema_revision: "a".repeat(256),
            safe_result_projection_revision: "a".repeat(256),
            observation_contract_revision: "a".repeat(256),
            stream_shape: "a".repeat(256),
        };
        let full_registry = |count: usize| ToolRegistryRevision {
            registry_revision_id: "registry-1".to_owned(),
            descriptors: (0..count).map(max_size_descriptor).collect(),
            admission_engine_revision: "admission-1".to_owned(),
            hook_pipeline_revision: "hooks-1".to_owned(),
        };
        assert!(full_registry(204).validate().is_ok());
        assert_eq!(
            full_registry(205)
                .validate()
                .expect_err("205 max-size descriptors exceed 512 KiB")
                .code(),
            "tool_registry_revision_too_large"
        );
    }

    #[test]
    fn public_wire_families_cover_exactly_the_ledger_tags() {
        // The expected tag set is rebuilt solely from the domain-owned
        // registry constants: there is no protocol-side numeric mirror.
        const LEDGER_TAGS: [u32; 23] = [
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
        let mut tags: Vec<u32> = PUBLIC_WIRE_CONTRACT_FAMILIES
            .iter()
            .map(|descriptor| descriptor.tag)
            .collect();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(
            tags, LEDGER_TAGS,
            "wire families must cover exactly the 23 ADR 0036 ledger tags"
        );
    }

    #[test]
    fn public_wire_families_duplicate_only_for_versioned_fork_families() {
        for descriptor in PUBLIC_WIRE_CONTRACT_FAMILIES {
            let count = PUBLIC_WIRE_CONTRACT_FAMILIES
                .iter()
                .filter(|other| other.tag == descriptor.tag)
                .count();
            let expected = if descriptor.tag == TagRegistry::FORK_BASE_SNAPSHOT_V1
                || descriptor.tag == TagRegistry::FORK_PREVIEW_V1
            {
                2
            } else {
                1
            };
            assert_eq!(
                count, expected,
                "{} must be the only duplicate tag, found {count} descriptors",
                descriptor.name
            );
        }
    }

    #[test]
    fn public_wire_family_tags_match_the_domain_tag_registry() {
        let registry: [(&str, u32); 25] = [
            (
                "programmatic-caller-policy-selection-v1",
                TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
            ),
            (
                "agent-activity-selection-v1",
                TagRegistry::AGENT_ACTIVITY_SELECTION_V1,
            ),
            ("goal-run-selection-v1", TagRegistry::GOAL_RUN_SELECTION_V1),
            (
                "continual-harness-selection-v1",
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
            ),
            (
                "mcp-method-catalog-selection-v1",
                TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1,
            ),
            (
                "model-capability-taxonomy-v1",
                TagRegistry::MODEL_CAPABILITY_TAXONOMY_V1,
            ),
            (
                "provider-profile-revision-v1",
                TagRegistry::PROVIDER_PROFILE_REVISION_V1,
            ),
            ("provider-selection-v1", TagRegistry::PROVIDER_SELECTION_V1),
            (
                "reasoning-history-manifest-v1",
                TagRegistry::REASONING_HISTORY_MANIFEST_V1,
            ),
            (
                "context-source-manifest-v1",
                TagRegistry::CONTEXT_SOURCE_MANIFEST_V1,
            ),
            (
                "model-context-projection-v1",
                TagRegistry::MODEL_CONTEXT_PROJECTION_V1,
            ),
            (
                "tool-descriptor-revision",
                TagRegistry::TOOL_DESCRIPTOR_REVISION,
            ),
            (
                "tool-registry-revision",
                TagRegistry::TOOL_REGISTRY_REVISION,
            ),
            ("model-tool-loop-v1", TagRegistry::MODEL_TOOL_LOOP_V1),
            ("bridge-invocation-v1", TagRegistry::BRIDGE_INVOCATION_V1),
            ("fork-base-snapshot-v1", TagRegistry::FORK_BASE_SNAPSHOT_V1),
            ("fork-base-snapshot-v2", TagRegistry::FORK_BASE_SNAPSHOT_V1),
            ("fork-preview-v1", TagRegistry::FORK_PREVIEW_V1),
            ("fork-preview-v2", TagRegistry::FORK_PREVIEW_V1),
            ("fork-command-v1", TagRegistry::FORK_COMMAND_V1),
            (
                "agent-activity-tree-v1",
                TagRegistry::AGENT_ACTIVITY_TREE_V1,
            ),
            (
                "agent-activity-pair-v1",
                TagRegistry::AGENT_ACTIVITY_PAIR_V1,
            ),
            ("agent-message-v1", TagRegistry::AGENT_MESSAGE_V1),
            (
                "agent-activity-journal-record-v1",
                TagRegistry::AGENT_ACTIVITY_JOURNAL_RECORD_V1,
            ),
            (
                "agent-notification-record-v1",
                TagRegistry::AGENT_NOTIFICATION_RECORD_V1,
            ),
        ];
        assert_eq!(registry.len(), PUBLIC_WIRE_CONTRACT_FAMILIES.len());
        for descriptor in PUBLIC_WIRE_CONTRACT_FAMILIES {
            let expected = registry
                .iter()
                .find(|(name, _)| *name == descriptor.name)
                .expect("every family has a domain registry entry")
                .1;
            assert_eq!(
                descriptor.tag, expected,
                "{} must carry the domain registry tag",
                descriptor.name
            );
        }
    }

    #[test]
    fn current_protocol_and_schema_versions_are_1_1() {
        assert_eq!(
            crate::CURRENT_PROTOCOL_VERSION,
            crate::ProtocolVersionDto::new(1, 1)
        );
        assert_eq!(
            crate::CURRENT_DTO_SCHEMA_VERSION,
            intention_types::SchemaVersionDto::new(1, 1)
        );
    }

    fn catalog_query() -> GetProviderCatalogQueryDto {
        GetProviderCatalogQueryDto {
            schema_version: "1.1".to_owned(),
            page_token: None,
            expected_catalog_revision_id: Some("catalog-rev-1".to_owned()),
        }
    }

    fn catalog_entry() -> ProviderCatalogEntryDto {
        ProviderCatalogEntryDto {
            profile_id: "profile-1".to_owned(),
            profile_revision_id: "rev-1".to_owned(),
            display_name: "Provider One".to_owned(),
            enabled: true,
            provider_kind_id: "responses".to_owned(),
            kind_descriptor_revision_id: "kind-rev-1".to_owned(),
            model_id: "model-1".to_owned(),
            normalized_endpoint: Some("https://provider.example".to_owned()),
            effective_execution_policy: "execution-policy".to_owned(),
            capability_subset: vec!["text".to_owned()],
            credential_transport_mode: CredentialTransportMode::SafeHeader,
            credential_transport_safe_header_name: Some("x-safe-header".to_owned()),
            credential_configured: true,
            driver_declared_capabilities: vec!["text".to_owned()],
            readiness: ProviderReadinessDto::Ready,
        }
    }

    fn catalog_page() -> ProviderCatalogPageDto {
        ProviderCatalogPageDto {
            schema_version: "1.1".to_owned(),
            catalog_revision_id: "catalog-rev-1".to_owned(),
            entries: vec![catalog_entry()],
            next_page_token: None,
            has_more: false,
        }
    }

    fn catalog_status_query() -> GetProviderCatalogStatusQueryDto {
        GetProviderCatalogStatusQueryDto {
            schema_version: "1.1".to_owned(),
        }
    }

    fn catalog_status() -> ProviderCatalogStatusDto {
        ProviderCatalogStatusDto {
            schema_version: "1.1".to_owned(),
            activation_state: ProviderCatalogActivationState::Active,
            degraded_reason: None,
            active_catalog_revision_id: Some("catalog-rev-1".to_owned()),
            candidate_catalog_revision_id: None,
            active_default_profile_id: Some("profile-1".to_owned()),
            removal_impact: None,
            provider_profiles_negotiated: true,
        }
    }

    fn set_profile_command() -> SetSessionProviderProfileCommandDto {
        SetSessionProviderProfileCommandDto {
            schema_version: "1.1".to_owned(),
            session_id: "session-1".to_owned(),
            profile_id: "profile-1".to_owned(),
            expected_session_projection_revision: 7,
            operation_id: "operation-1".to_owned(),
        }
    }

    fn set_profile_accepted(changed: bool) -> SetSessionProviderProfileAcceptedDto {
        SetSessionProviderProfileAcceptedDto {
            session_id: "session-1".to_owned(),
            changed,
            resulting_projection_revision: 8,
            resolved: ResolvedProviderProfileDto::Resolved {
                profile_id: "profile-1".to_owned(),
                profile_revision_id: "rev-1".to_owned(),
            },
        }
    }

    fn session_profile_query() -> GetSessionProviderProfileQueryDto {
        GetSessionProviderProfileQueryDto {
            schema_version: "1.1".to_owned(),
            session_id: "session-1".to_owned(),
        }
    }

    fn session_profile() -> SessionProviderProfileDto {
        SessionProviderProfileDto {
            session_id: "session-1".to_owned(),
            profile_id: "profile-1".to_owned(),
            resolved: ResolvedProviderProfileDto::Resolved {
                profile_id: "profile-1".to_owned(),
                profile_revision_id: "rev-1".to_owned(),
            },
            session_projection_revision: 8,
            global_default_profile_id: "profile-default".to_owned(),
        }
    }

    fn accept_removal_command() -> AcceptProviderCatalogRemovalCommandDto {
        AcceptProviderCatalogRemovalCommandDto {
            candidate_handle: "candidate-1".to_owned(),
            expected_active_catalog_revision_id: "catalog-rev-1".to_owned(),
            expected_candidate_catalog_revision_id: "catalog-rev-2".to_owned(),
            operation_id: "operation-1".to_owned(),
            source_recheck: true,
        }
    }

    fn reject_candidate_command() -> RejectProviderCatalogCandidateCommandDto {
        RejectProviderCatalogCandidateCommandDto {
            candidate_handle: "candidate-1".to_owned(),
            expected_active_catalog_revision_id: "catalog-rev-1".to_owned(),
            operation_id: "operation-1".to_owned(),
        }
    }

    fn reconcile_queue_command() -> ReconcileUnavailableQueueCommandDto {
        ReconcileUnavailableQueueCommandDto {
            session_id: "session-1".to_owned(),
            operation_id: "operation-1".to_owned(),
            page_cursor: Some("opaque-page-cursor-01".to_owned()),
        }
    }

    fn reconcile_queue_accepted() -> ReconcileUnavailableQueueAcceptedDto {
        ReconcileUnavailableQueueAcceptedDto {
            session_id: "session-1".to_owned(),
            page_cursor: Some("opaque-page-cursor-01".to_owned()),
            promoted_count: 8,
        }
    }

    fn admit_recovered_command() -> AdmitRecoveredRunCommandDto {
        AdmitRecoveredRunCommandDto {
            session_id: "session-1".to_owned(),
            run_id: "run-1".to_owned(),
            operation_id: "operation-1".to_owned(),
        }
    }

    fn admit_recovered_accepted() -> AdmitRecoveredRunAcceptedDto {
        AdmitRecoveredRunAcceptedDto {
            session_id: "session-1".to_owned(),
            run_id: "run-1".to_owned(),
        }
    }

    fn usage_query() -> GetProviderUsageQueryDto {
        GetProviderUsageQueryDto {
            schema_version: "1.1".to_owned(),
            profile_id: "profile-1".to_owned(),
            usage_period_start: 100,
            usage_period_end: 200,
        }
    }

    fn usage_aggregation() -> UsageAggregationDto {
        UsageAggregationDto {
            profile_id: "profile-1".to_owned(),
            provider_profile_revision_id: "rev-1".to_owned(),
            model_id: "model-1".to_owned(),
            request_count: 12,
            input_units: 1000,
            output_units: 500,
            reasoning_units: 250,
            usage_period_start: 100,
            usage_period_end: 200,
        }
    }

    fn candidate_prepared_event() -> ProviderCatalogCandidatePreparedEventDto {
        ProviderCatalogCandidatePreparedEventDto {
            candidate_handle: "candidate-1".to_owned(),
            candidate_catalog_revision_id: "catalog-rev-2".to_owned(),
            occurred_at: 100,
        }
    }

    fn reload_command() -> ReloadConfigurationCommandDto {
        ReloadConfigurationCommandDto {
            candidate_snapshot_reference: Some("snapshot-1".to_owned()),
            candidate_edit_reference: None,
            expected_active_config_revision: "config-rev-1".to_owned(),
            operation_id: "operation-1".to_owned(),
            origin: ConfigurationOriginDto::Admin,
        }
    }

    fn reload_transaction(committed: bool) -> ReloadTransactionDto {
        ReloadTransactionDto {
            transaction_id: "transaction-1".to_owned(),
            previous_config_revision: "config-rev-1".to_owned(),
            candidate_config_revision: "config-rev-2".to_owned(),
            validation_result: ConfigurationValidationOutcomeDto::Valid,
            commit_outcome: if committed {
                ConfigurationCommitOutcomeDto::Committed
            } else {
                ConfigurationCommitOutcomeDto::Rejected
            },
            safe_failure_code: if committed {
                None
            } else {
                Some("reload_rejected".to_owned())
            },
            safe_failure_detail: if committed {
                None
            } else {
                Some("safe rejection detail".to_owned())
            },
        }
    }

    fn rotate_credentials_command() -> RotateProviderCredentialsCommandDto {
        RotateProviderCredentialsCommandDto {
            profile_id: "profile-1".to_owned(),
            provider_profile_revision_id: "rev-1".to_owned(),
            expected_credential_composition_revision: "composition-1".to_owned(),
            operation_id: "operation-1".to_owned(),
        }
    }

    fn rotation_result() -> CredentialRotationResultDto {
        CredentialRotationResultDto {
            operation_id: "operation-1".to_owned(),
            profile_id: "profile-1".to_owned(),
            safe_credential_composition_revision: "composition-2".to_owned(),
            rotated: true,
        }
    }

    fn health_evidence() -> ProviderHealthEvidenceDto {
        ProviderHealthEvidenceDto {
            profile_id: "profile-1".to_owned(),
            provider_profile_revision_id: "rev-1".to_owned(),
            health_attempt_id: "attempt-1".to_owned(),
            check_contract_revision: "check-1".to_owned(),
            observed_availability: ProviderAvailabilityObservation::Available,
            observed_at: 100,
            failure_category: None,
            safe_diagnostic_code: None,
        }
    }

    fn discovery_attempt() -> ProviderDiscoveryAttemptDto {
        ProviderDiscoveryAttemptDto {
            attempt_id: "attempt-1".to_owned(),
            discovery_scope: "responses".to_owned(),
            phase: ProviderDiscoveryPhase::Started,
            started_at: 100,
            safe_status: "running".to_owned(),
        }
    }

    fn discovery_record() -> ProviderModelDiscoveryRecordDto {
        ProviderModelDiscoveryRecordDto {
            discovery_scope: "responses".to_owned(),
            model_id: "model-1".to_owned(),
            capability_records: vec!["text".to_owned()],
            source_attempt_id: "attempt-1".to_owned(),
            discovered_at: 100,
        }
    }

    fn pricing_observation() -> PricingObservationDto {
        PricingObservationDto {
            provider_kind_id: "responses".to_owned(),
            model_id: "model-1".to_owned(),
            bounded_numeric_value: 1000,
            classification: PricingClassification::CapacityObservation,
            observed_at: 100,
        }
    }

    fn raw_toml_edit() -> RawTomlEditCommandDto {
        RawTomlEditCommandDto {
            operation_id: "operation-1".to_owned(),
            expected_config_revision: "config-rev-1".to_owned(),
            candidate_content: "[daemon]\nmax_parallel_runs = 2\n".to_owned(),
        }
    }

    fn typed_config_edit() -> ConfigurationEditCommandDto {
        ConfigurationEditCommandDto {
            operation_id: "operation-1".to_owned(),
            expected_config_revision: "config-rev-1".to_owned(),
            operations: vec![ConfigurationEditOperationDto::Set {
                key_path: "daemon.max_parallel_runs".to_owned(),
                safe_value: "2".to_owned(),
            }],
        }
    }

    fn header_policy() -> ArbitraryHeaderPolicyDto {
        ArbitraryHeaderPolicyDto {
            policy_revision: "policy-1".to_owned(),
            kind_descriptor_revision_id: "kind-rev-1".to_owned(),
            allowed_header_names: vec!["x-provider-trace".to_owned()],
        }
    }

    fn reasoning_catalog_projection() -> ProviderReasoningCatalogProjectionDto {
        ProviderReasoningCatalogProjectionDto {
            provider_kind_id: "responses".to_owned(),
            model_id: "model-1".to_owned(),
            supported_effort_levels: vec![ReasoningEffortLevel::Low, ReasoningEffortLevel::High],
            responses_reasoning_modes: vec![ResponsesReasoningMode::Standard],
            projection_revision: "projection-1".to_owned(),
        }
    }

    #[test]
    fn zone2_provider_catalog_family_round_trips_and_validates() {
        let mut query = catalog_query();
        assert!(query.validate().is_ok());
        round_trip(&query);
        query.page_token = Some("opaque-page-cursor-01".to_owned());
        assert!(query.validate().is_ok());
        round_trip(&query);

        let entry = catalog_entry();
        assert!(entry.validate().is_ok());
        round_trip(&entry);
        for readiness in [
            ProviderReadinessDto::Ready,
            ProviderReadinessDto::Disabled,
            ProviderReadinessDto::Unavailable,
        ] {
            round_trip(&readiness);
        }
        for mode in [
            CredentialTransportMode::Bearer,
            CredentialTransportMode::SafeHeader,
        ] {
            let mut entry = catalog_entry();
            entry.credential_transport_mode = mode;
            assert!(entry.validate().is_ok());
            round_trip(&entry);
        }

        let page = catalog_page();
        assert!(page.validate().is_ok());
        round_trip(&page);
        let mut paged = catalog_page();
        paged.entries.push(ProviderCatalogEntryDto {
            profile_id: "profile-2".to_owned(),
            ..catalog_entry()
        });
        paged.next_page_token = Some("opaque-page-cursor-02".to_owned());
        paged.has_more = true;
        assert!(paged.validate().is_ok());
        round_trip(&paged);
        let empty = ProviderCatalogPageDto {
            entries: Vec::new(),
            ..catalog_page()
        };
        assert!(empty.validate().is_ok());
        round_trip(&empty);

        let status_query = catalog_status_query();
        assert!(status_query.validate().is_ok());
        round_trip(&status_query);

        let status = catalog_status();
        assert!(status.validate().is_ok());
        round_trip(&status);
        let impact = ProviderCatalogRemovalImpactDto {
            affected_profile_ids: vec!["profile-1".to_owned()],
            safe_impact_summary: "one profile affected".to_owned(),
        };
        assert!(impact.validate().is_ok());
        round_trip(&impact);
    }

    #[test]
    fn zone2_provider_catalog_validation_rejects_bad_inputs() {
        let too_long = "a".repeat(257);
        for field in ["   ", too_long.as_str(), "a\u{0000}b"] {
            let mut query = catalog_query();
            query.schema_version = field.to_owned();
            assert_eq!(
                query
                    .validate()
                    .expect_err("invalid catalog query is rejected")
                    .code(),
                "provider_catalog_invalid"
            );
            let mut entry = catalog_entry();
            entry.profile_id = field.to_owned();
            assert_eq!(
                entry
                    .validate()
                    .expect_err("invalid catalog entry is rejected")
                    .code(),
                "provider_catalog_entry_invalid"
            );
        }

        // Endpoint forms carrying userinfo, query, fragment, or control
        // characters are rejected.
        for endpoint in [
            "https://user@provider.example",
            "https://provider.example?q=1",
            "https://provider.example#frag",
            "https://provider.example\tpath",
        ] {
            let mut entry = catalog_entry();
            entry.normalized_endpoint = Some(endpoint.to_owned());
            assert_eq!(
                entry
                    .validate()
                    .expect_err("invalid endpoint is rejected")
                    .code(),
                "invalid_endpoint"
            );
        }

        // Credential-shaped values are rejected in entry fields.
        for value in ["sk-123", "Bearer secret", "api-key-1"] {
            let mut entry = catalog_entry();
            entry.model_id = value.to_owned();
            assert_eq!(
                entry
                    .validate()
                    .expect_err("credential-shaped value is rejected")
                    .code(),
                "credentials_forbidden"
            );
            let mut header = catalog_entry();
            header.credential_transport_safe_header_name = Some(value.to_owned());
            assert_eq!(
                header
                    .validate()
                    .expect_err("credential-shaped header is rejected")
                    .code(),
                "credentials_forbidden"
            );
        }

        // Page token bounds and credential shapes.
        let mut query = catalog_query();
        query.page_token = Some("   ".to_owned());
        assert_eq!(
            query
                .validate()
                .expect_err("blank page token is rejected")
                .code(),
            "invalid_page_token"
        );
        let mut query = catalog_query();
        query.page_token = Some("a".repeat(1025));
        assert_eq!(
            query
                .validate()
                .expect_err("over-long page token is rejected")
                .code(),
            "invalid_page_token"
        );
        let mut query = catalog_query();
        query.page_token = Some("opaque-token-01".to_owned());
        assert_eq!(
            query
                .validate()
                .expect_err("credential-shaped page token is rejected")
                .code(),
            "credentials_forbidden"
        );
        let mut query = catalog_query();
        query.page_token = Some("opaque-page-cursor\u{0000}-01".to_owned());
        assert_eq!(
            query
                .validate()
                .expect_err("control-bearing page token is rejected")
                .code(),
            "invalid_page_token"
        );

        // Catalog page: unsorted or repeated profile ids are rejected.
        let mut unsorted = catalog_page();
        unsorted.entries = vec![
            ProviderCatalogEntryDto {
                profile_id: "profile-2".to_owned(),
                ..catalog_entry()
            },
            ProviderCatalogEntryDto {
                profile_id: "profile-1".to_owned(),
                ..catalog_entry()
            },
        ];
        assert_eq!(
            unsorted
                .validate()
                .expect_err("unsorted catalog page is rejected")
                .code(),
            "provider_catalog_unsorted"
        );
        let mut repeated = catalog_page();
        repeated.entries = vec![catalog_entry(), catalog_entry()];
        assert_eq!(
            repeated
                .validate()
                .expect_err("repeated profile id is rejected")
                .code(),
            "provider_catalog_unsorted"
        );

        // Catalog page: has_more must agree with the next page token.
        let mut missing_token = catalog_page();
        missing_token.has_more = true;
        assert_eq!(
            missing_token
                .validate()
                .expect_err("has_more without a token is rejected")
                .code(),
            "provider_catalog_invalid"
        );
        let mut stale_token = catalog_page();
        stale_token.next_page_token = Some("opaque-page-cursor-01".to_owned());
        assert_eq!(
            stale_token
                .validate()
                .expect_err("a token without has_more is rejected")
                .code(),
            "provider_catalog_invalid"
        );
        let mut oversized = catalog_page();
        oversized.entries = (0..257)
            .map(|index| ProviderCatalogEntryDto {
                profile_id: format!("profile-{index}"),
                ..catalog_entry()
            })
            .collect();
        assert_eq!(
            oversized
                .validate()
                .expect_err("a 257-entry page is rejected")
                .code(),
            "provider_catalog_invalid"
        );
    }

    #[test]
    fn zone2_catalog_status_covers_all_activation_and_degraded_states() {
        let mut preparing = catalog_status();
        preparing.activation_state = ProviderCatalogActivationState::Preparing;
        preparing.active_catalog_revision_id = None;
        assert!(preparing.validate().is_ok());
        round_trip(&preparing);

        for reason in [
            ProviderCatalogDegradedReason::RemovalCandidatePending,
            ProviderCatalogDegradedReason::RemovalCandidateRejected,
            ProviderCatalogDegradedReason::RemovalCandidateExpired,
        ] {
            let mut pending = catalog_status();
            pending.activation_state = ProviderCatalogActivationState::PendingRemoval;
            pending.degraded_reason = Some(reason);
            pending.candidate_catalog_revision_id = Some("catalog-rev-2".to_owned());
            pending.removal_impact = Some(ProviderCatalogRemovalImpactDto {
                affected_profile_ids: vec!["profile-1".to_owned()],
                safe_impact_summary: "one profile affected".to_owned(),
            });
            assert!(pending.validate().is_ok());
            round_trip(&pending);
        }

        let mut recovery = catalog_status();
        recovery.activation_state = ProviderCatalogActivationState::ActivationRecoveryRequired;
        recovery.degraded_reason = Some(ProviderCatalogDegradedReason::ActivationRecoveryRequired);
        assert!(recovery.validate().is_ok());
        round_trip(&recovery);

        // Invalid activation/degradation combinations fail closed.
        let mut active_degraded = catalog_status();
        active_degraded.degraded_reason =
            Some(ProviderCatalogDegradedReason::RemovalCandidatePending);
        assert_eq!(
            active_degraded
                .validate()
                .expect_err("active with a degraded reason is rejected")
                .code(),
            "provider_catalog_status_invalid"
        );
        let mut preparing_degraded = catalog_status();
        preparing_degraded.activation_state = ProviderCatalogActivationState::Preparing;
        preparing_degraded.degraded_reason =
            Some(ProviderCatalogDegradedReason::RemovalCandidateExpired);
        assert_eq!(
            preparing_degraded
                .validate()
                .expect_err("preparing with a degraded reason is rejected")
                .code(),
            "provider_catalog_status_invalid"
        );
        let mut pending_without_candidate = catalog_status();
        pending_without_candidate.activation_state = ProviderCatalogActivationState::PendingRemoval;
        pending_without_candidate.degraded_reason =
            Some(ProviderCatalogDegradedReason::RemovalCandidatePending);
        assert_eq!(
            pending_without_candidate
                .validate()
                .expect_err("pending removal without a candidate revision is rejected")
                .code(),
            "provider_catalog_status_invalid"
        );
        let mut pending_wrong_reason = catalog_status();
        pending_wrong_reason.activation_state = ProviderCatalogActivationState::PendingRemoval;
        pending_wrong_reason.degraded_reason =
            Some(ProviderCatalogDegradedReason::ActivationRecoveryRequired);
        pending_wrong_reason.candidate_catalog_revision_id = Some("catalog-rev-2".to_owned());
        assert_eq!(
            pending_wrong_reason
                .validate()
                .expect_err("pending removal with the wrong reason is rejected")
                .code(),
            "provider_catalog_status_invalid"
        );
        let mut recovery_without_reason = catalog_status();
        recovery_without_reason.activation_state =
            ProviderCatalogActivationState::ActivationRecoveryRequired;
        assert_eq!(
            recovery_without_reason
                .validate()
                .expect_err("recovery without its reason is rejected")
                .code(),
            "provider_catalog_status_invalid"
        );
    }

    #[test]
    fn zone2_session_provider_profile_family_round_trips_and_validates() {
        let command = set_profile_command();
        assert!(command.validate().is_ok());
        round_trip(&command);

        // Changed, idempotent no-op, and unavailable acceptance evidence.
        let changed = set_profile_accepted(true);
        assert!(changed.validate().is_ok());
        round_trip(&changed);
        let idempotent = set_profile_accepted(false);
        assert!(idempotent.validate().is_ok());
        round_trip(&idempotent);
        for reason in [
            ProviderProfileUnavailableReason::ProfileNotFound,
            ProviderProfileUnavailableReason::ProfileDisabled,
            ProviderProfileUnavailableReason::ProviderUnavailable,
            ProviderProfileUnavailableReason::CatalogNotActive,
        ] {
            let mut unavailable = set_profile_accepted(true);
            unavailable.resolved = ResolvedProviderProfileDto::Unavailable(reason);
            assert!(unavailable.validate().is_ok());
            round_trip(&unavailable);
        }

        let query = session_profile_query();
        assert!(query.validate().is_ok());
        round_trip(&query);

        let profile = session_profile();
        assert!(profile.validate().is_ok());
        round_trip(&profile);

        // Blank and credential-shaped session profile fields fail closed.
        let mut blank = set_profile_command();
        blank.session_id = "   ".to_owned();
        assert_eq!(
            blank
                .validate()
                .expect_err("blank session id is rejected")
                .code(),
            "set_session_provider_profile_invalid"
        );
        let mut credential = set_profile_command();
        credential.profile_id = "sk-profile".to_owned();
        assert_eq!(
            credential
                .validate()
                .expect_err("credential-shaped profile id is rejected")
                .code(),
            "credentials_forbidden"
        );
        let mut credential_resolved = session_profile();
        credential_resolved.resolved = ResolvedProviderProfileDto::Resolved {
            profile_id: "Bearer profile".to_owned(),
            profile_revision_id: "rev-1".to_owned(),
        };
        assert_eq!(
            credential_resolved
                .validate()
                .expect_err("credential-shaped resolved profile is rejected")
                .code(),
            "credentials_forbidden"
        );
    }

    #[test]
    fn zone2_removal_queue_recovery_family_round_trips_and_validates() {
        let accept = accept_removal_command();
        assert!(accept.validate().is_ok());
        round_trip(&accept);
        let accepted = AcceptProviderCatalogRemovalAcceptedDto {
            candidate_handle: "candidate-1".to_owned(),
            active_catalog_revision_id: "catalog-rev-1".to_owned(),
        };
        assert!(accepted.validate().is_ok());
        round_trip(&accepted);

        let reject = reject_candidate_command();
        assert!(reject.validate().is_ok());
        round_trip(&reject);
        let rejected = RejectProviderCatalogCandidateAcceptedDto {
            candidate_handle: "candidate-1".to_owned(),
        };
        assert!(rejected.validate().is_ok());
        round_trip(&rejected);

        // Mismatched expected revisions: the active and candidate revisions
        // must differ.
        let mut same_revisions = accept.clone();
        same_revisions.expected_candidate_catalog_revision_id = "catalog-rev-1".to_owned();
        assert_eq!(
            same_revisions
                .validate()
                .expect_err("equal expected revisions are rejected")
                .code(),
            "provider_catalog_removal_invalid"
        );

        let reconcile = reconcile_queue_command();
        assert!(reconcile.validate().is_ok());
        round_trip(&reconcile);
        let mut reconcile_no_cursor = reconcile;
        reconcile_no_cursor.page_cursor = None;
        assert!(reconcile_no_cursor.validate().is_ok());
        round_trip(&reconcile_no_cursor);

        // The 8-promotion boundary is enforced on reconciliation pages.
        assert!(reconcile_queue_accepted().validate().is_ok());
        round_trip(&reconcile_queue_accepted());
        let mut over_bound = reconcile_queue_accepted();
        over_bound.promoted_count = 9;
        assert_eq!(
            over_bound
                .validate()
                .expect_err("a 9-promotion page is rejected")
                .code(),
            "unavailable_queue_invalid"
        );

        let admit = admit_recovered_command();
        assert!(admit.validate().is_ok());
        round_trip(&admit);
        let admitted = admit_recovered_accepted();
        assert!(admitted.validate().is_ok());
        round_trip(&admitted);

        // Blank and credential-shaped removal fields fail closed.
        let mut blank_accept = accept;
        blank_accept.candidate_handle = "   ".to_owned();
        assert_eq!(
            blank_accept
                .validate()
                .expect_err("blank candidate handle is rejected")
                .code(),
            "provider_catalog_removal_invalid"
        );
        let mut credential_admit = admit;
        credential_admit.operation_id = "Bearer operation".to_owned();
        assert_eq!(
            credential_admit
                .validate()
                .expect_err("credential-shaped operation id is rejected")
                .code(),
            "credentials_forbidden"
        );
    }

    #[test]
    fn zone2_usage_family_round_trips_and_validates() {
        let query = usage_query();
        assert!(query.validate().is_ok());
        round_trip(&query);
        let aggregation = usage_aggregation();
        assert!(aggregation.validate().is_ok());
        round_trip(&aggregation);

        // Absent reasoning token counts never mean zero.
        let absent = ReasoningUsageDto {
            input_tokens: None,
            output_tokens: None,
        };
        round_trip(&absent);
        let partial = ReasoningUsageDto {
            input_tokens: Some(0),
            output_tokens: Some(3),
        };
        round_trip(&partial);

        // Periods ending before their start are rejected.
        let mut reversed = usage_query();
        reversed.usage_period_start = 200;
        reversed.usage_period_end = 100;
        assert_eq!(
            reversed
                .validate()
                .expect_err("reversed usage period is rejected")
                .code(),
            "provider_usage_invalid"
        );
        let mut reversed_aggregation = aggregation;
        reversed_aggregation.usage_period_end = 99;
        assert_eq!(
            reversed_aggregation
                .validate()
                .expect_err("reversed aggregation period is rejected")
                .code(),
            "provider_usage_invalid"
        );
        let mut credential = usage_aggregation();
        credential.model_id = "sk-model".to_owned();
        assert_eq!(
            credential
                .validate()
                .expect_err("credential-shaped model id is rejected")
                .code(),
            "credentials_forbidden"
        );
    }

    #[test]
    fn zone2_catalog_events_round_trip_and_validate() {
        let prepared = candidate_prepared_event();
        assert!(prepared.validate().is_ok());
        round_trip(&prepared);
        let pending = ProviderCatalogRemovalPendingEventDto {
            candidate_handle: "candidate-1".to_owned(),
            removal_revision_id: "catalog-rev-2".to_owned(),
            occurred_at: 100,
        };
        assert!(pending.validate().is_ok());
        round_trip(&pending);
        let rejected = ProviderCatalogCandidateRejectedEventDto {
            candidate_handle: "candidate-1".to_owned(),
            safe_rejection_reason: "reviewer rejected".to_owned(),
            occurred_at: 100,
        };
        assert!(rejected.validate().is_ok());
        round_trip(&rejected);
        let expired = ProviderCatalogCandidateExpiredEventDto {
            candidate_handle: "candidate-1".to_owned(),
            occurred_at: 100,
        };
        assert!(expired.validate().is_ok());
        round_trip(&expired);
        let recovery_required = ProviderCatalogActivationRecoveryRequiredEventDto {
            candidate_handle: "candidate-1".to_owned(),
            safe_recovery_reason: "activation failed".to_owned(),
            occurred_at: 100,
        };
        assert!(recovery_required.validate().is_ok());
        round_trip(&recovery_required);
        let recovery_completed = ProviderCatalogRecoveryCompletedEventDto {
            active_catalog_revision_id: "catalog-rev-1".to_owned(),
            occurred_at: 100,
        };
        assert!(recovery_completed.validate().is_ok());
        round_trip(&recovery_completed);
        let changed = SessionProviderProfileChangedEventDto {
            session_id: "session-1".to_owned(),
            previous_profile_id: "profile-default".to_owned(),
            profile_id: "profile-1".to_owned(),
            session_projection_revision: 8,
            occurred_at: 100,
        };
        assert!(changed.validate().is_ok());
        round_trip(&changed);

        let mut credential = prepared;
        credential.candidate_handle = "sk-candidate".to_owned();
        assert_eq!(
            credential
                .validate()
                .expect_err("credential-shaped event field is rejected")
                .code(),
            "credentials_forbidden"
        );
    }

    #[test]
    fn zone2_configuration_reload_family_round_trips_and_validates() {
        for origin in [ConfigurationOriginDto::User, ConfigurationOriginDto::Admin] {
            let mut command = reload_command();
            command.origin = origin;
            assert!(command.validate().is_ok());
            round_trip(&command);
        }
        let mut edit_reference = reload_command();
        edit_reference.candidate_snapshot_reference = None;
        edit_reference.candidate_edit_reference = Some("edit-1".to_owned());
        assert!(edit_reference.validate().is_ok());
        round_trip(&edit_reference);

        let committed = reload_transaction(true);
        assert!(committed.validate().is_ok());
        round_trip(&committed);
        let rejected = reload_transaction(false);
        assert!(rejected.validate().is_ok());
        round_trip(&rejected);
        let mut invalid = reload_transaction(true);
        invalid.validation_result = ConfigurationValidationOutcomeDto::Invalid;
        invalid.safe_failure_code = Some("validation_failed".to_owned());
        assert!(invalid.validate().is_ok());
        round_trip(&invalid);

        let reloaded = ConfigurationReloadedEventDto {
            transaction_id: "transaction-1".to_owned(),
            config_revision: "config-rev-2".to_owned(),
            occurred_at: 100,
        };
        assert!(reloaded.validate().is_ok());
        round_trip(&reloaded);
        let reload_rejected = ConfigurationReloadRejectedEventDto {
            transaction_id: "transaction-1".to_owned(),
            safe_failure_code: "validation_failed".to_owned(),
            safe_failure_detail: Some("safe detail".to_owned()),
            occurred_at: 100,
        };
        assert!(reload_rejected.validate().is_ok());
        round_trip(&reload_rejected);

        // Neither candidate reference present is rejected.
        let mut no_reference = reload_command();
        no_reference.candidate_snapshot_reference = None;
        no_reference.candidate_edit_reference = None;
        assert_eq!(
            no_reference
                .validate()
                .expect_err("a reload without candidate references is rejected")
                .code(),
            "configuration_reload_invalid"
        );
        // Failed reloads must carry a safe failure code.
        let mut missing_code = reload_transaction(false);
        missing_code.safe_failure_code = None;
        assert_eq!(
            missing_code
                .validate()
                .expect_err("a failed reload without a failure code is rejected")
                .code(),
            "configuration_reload_invalid"
        );
        // Successful reloads must not carry failure detail.
        let mut stale_detail = reload_transaction(true);
        stale_detail.safe_failure_detail = Some("stale".to_owned());
        assert_eq!(
            stale_detail
                .validate()
                .expect_err("a successful reload with failure detail is rejected")
                .code(),
            "configuration_reload_invalid"
        );
        let mut credential = reload_command();
        credential.operation_id = "Bearer operation".to_owned();
        assert_eq!(
            credential
                .validate()
                .expect_err("credential-shaped reload field is rejected")
                .code(),
            "credentials_forbidden"
        );
    }

    #[test]
    fn zone2_rotation_and_health_evidence_round_trip_and_validate() {
        let rotate = rotate_credentials_command();
        assert!(rotate.validate().is_ok());
        round_trip(&rotate);
        let result = rotation_result();
        assert!(result.validate().is_ok());
        round_trip(&result);

        let evidence = health_evidence();
        assert!(evidence.validate().is_ok());
        round_trip(&evidence);
        for category in [
            ProviderHealthFailureCategory::ConnectionFailed,
            ProviderHealthFailureCategory::AuthenticationRejected,
            ProviderHealthFailureCategory::RequestTimeout,
            ProviderHealthFailureCategory::RateLimited,
            ProviderHealthFailureCategory::ServiceUnavailable,
        ] {
            let mut unavailable = health_evidence();
            unavailable.observed_availability = ProviderAvailabilityObservation::Unavailable;
            unavailable.failure_category = Some(category);
            unavailable.safe_diagnostic_code = Some("diag-1".to_owned());
            assert!(unavailable.validate().is_ok());
            round_trip(&unavailable);
        }
        for availability in [
            ProviderAvailabilityObservation::Available,
            ProviderAvailabilityObservation::Unavailable,
            ProviderAvailabilityObservation::Unknown,
        ] {
            round_trip(&availability);
        }

        // An Available observation must not carry failure detail.
        let mut contradictory = health_evidence();
        contradictory.failure_category = Some(ProviderHealthFailureCategory::RequestTimeout);
        assert_eq!(
            contradictory
                .validate()
                .expect_err("available with a failure category is rejected")
                .code(),
            "provider_health_evidence_invalid"
        );
        // An Unavailable observation must carry a failure category.
        let mut missing_category = health_evidence();
        missing_category.observed_availability = ProviderAvailabilityObservation::Unavailable;
        assert_eq!(
            missing_category
                .validate()
                .expect_err("unavailable without a failure category is rejected")
                .code(),
            "provider_health_evidence_invalid"
        );
        let mut credential = rotate;
        credential.profile_id = "sk-profile".to_owned();
        assert_eq!(
            credential
                .validate()
                .expect_err("credential-shaped rotation field is rejected")
                .code(),
            "credentials_forbidden"
        );
    }

    #[test]
    fn zone2_discovery_and_pricing_family_round_trips_and_validates() {
        for phase in [
            ProviderDiscoveryPhase::BeforeStart,
            ProviderDiscoveryPhase::Started,
            ProviderDiscoveryPhase::Terminal,
        ] {
            let mut attempt = discovery_attempt();
            attempt.phase = phase;
            assert!(attempt.validate().is_ok());
            round_trip(&attempt);
        }
        let record = discovery_record();
        assert!(record.validate().is_ok());
        round_trip(&record);
        for classification in [
            PricingClassification::IntrinsicRepresentationBound,
            PricingClassification::CapacityObservation,
            PricingClassification::ProductPolicy,
        ] {
            let mut observation = pricing_observation();
            observation.classification = classification;
            assert!(observation.validate().is_ok());
            round_trip(&observation);
        }

        let mut oversized = discovery_record();
        oversized.capability_records = (0..257).map(|i| format!("capability-{i}")).collect();
        assert_eq!(
            oversized
                .validate()
                .expect_err("a 257-capability record is rejected")
                .code(),
            "provider_discovery_invalid"
        );
        let mut credential = pricing_observation();
        credential.model_id = "Bearer model".to_owned();
        assert_eq!(
            credential
                .validate()
                .expect_err("credential-shaped pricing field is rejected")
                .code(),
            "credentials_forbidden"
        );
    }

    #[test]
    fn zone2_edit_policy_parser_and_reasoning_family_round_trips_and_validates() {
        let raw_edit = raw_toml_edit();
        assert!(raw_edit.validate().is_ok());
        round_trip(&raw_edit);
        let mut single_line = raw_edit.clone();
        single_line.candidate_content = "max_parallel_runs = 2".to_owned();
        assert!(single_line.validate().is_ok());

        // Credential-shaped TOML content is rejected (redacted edits only).
        for content in [
            "api_key = \"sk-secret\"\n".to_owned(),
            "token = \"secret\"\n".to_owned(),
            "bearer = \"credential\"\n".to_owned(),
        ] {
            let mut credential = raw_edit.clone();
            credential.candidate_content = content;
            assert_eq!(
                credential
                    .validate()
                    .expect_err("credential-bearing TOML content is rejected")
                    .code(),
                "credentials_forbidden"
            );
        }
        let mut nul_content = raw_edit.clone();
        nul_content.candidate_content = "[daemon]\nvalue\u{0000}=1\n".to_owned();
        assert_eq!(
            nul_content
                .validate()
                .expect_err("NUL-bearing TOML content is rejected")
                .code(),
            "raw_toml_edit_invalid"
        );
        let mut oversized = raw_edit;
        oversized.candidate_content = "a".repeat(64 * 1024 + 1);
        assert_eq!(
            oversized
                .validate()
                .expect_err("over-long TOML content is rejected")
                .code(),
            "raw_toml_edit_invalid"
        );

        let typed_edit = typed_config_edit();
        assert!(typed_edit.validate().is_ok());
        round_trip(&typed_edit);
        let remove_edit = ConfigurationEditCommandDto {
            operations: vec![ConfigurationEditOperationDto::Remove {
                key_path: "daemon.max_parallel_runs".to_owned(),
            }],
            ..typed_config_edit()
        };
        assert!(remove_edit.validate().is_ok());
        round_trip(&remove_edit);
        let mut empty_operations = typed_config_edit();
        empty_operations.operations = Vec::new();
        assert_eq!(
            empty_operations
                .validate()
                .expect_err("an empty typed edit is rejected")
                .code(),
            "configuration_edit_invalid"
        );
        let mut credential_operation = typed_config_edit();
        credential_operation.operations = vec![ConfigurationEditOperationDto::Set {
            key_path: "daemon.max_parallel_runs".to_owned(),
            safe_value: "Bearer value".to_owned(),
        }];
        assert_eq!(
            credential_operation
                .validate()
                .expect_err("credential-shaped safe value is rejected")
                .code(),
            "credentials_forbidden"
        );

        let policy = header_policy();
        assert!(policy.validate().is_ok());
        round_trip(&policy);
        let mut empty_policy = header_policy();
        empty_policy.allowed_header_names = Vec::new();
        assert_eq!(
            empty_policy
                .validate()
                .expect_err("an empty header policy is rejected")
                .code(),
            "arbitrary_header_policy_invalid"
        );
        let mut credential_policy = header_policy();
        credential_policy.allowed_header_names = vec!["sk-header".to_owned()];
        assert_eq!(
            credential_policy
                .validate()
                .expect_err("credential-shaped header name is rejected")
                .code(),
            "credentials_forbidden"
        );

        round_trip(&ProviderPreservationControlsDto {
            preserve_thinking: true,
            thinking_keep: false,
        });
        for parser in [
            ServerSideParserConfigDto::None,
            ServerSideParserConfigDto::Vllm {
                parser_id: "parser-1".to_owned(),
                bounded_limits: "max-context=8192".to_owned(),
            },
            ServerSideParserConfigDto::Sglang {
                parser_id: "parser-2".to_owned(),
                bounded_limits: "max-context=4096".to_owned(),
            },
        ] {
            assert!(parser.validate().is_ok());
            round_trip(&parser);
        }
        let credential_parser = ServerSideParserConfigDto::Vllm {
            parser_id: "parser-1".to_owned(),
            bounded_limits: "Bearer limits".to_owned(),
        };
        assert_eq!(
            credential_parser
                .validate()
                .expect_err("credential-shaped parser limits are rejected")
                .code(),
            "credentials_forbidden"
        );

        let projection = reasoning_catalog_projection();
        assert!(projection.validate().is_ok());
        round_trip(&projection);
        for effort in [
            ReasoningEffortLevel::None,
            ReasoningEffortLevel::Minimal,
            ReasoningEffortLevel::Low,
            ReasoningEffortLevel::Medium,
            ReasoningEffortLevel::High,
            ReasoningEffortLevel::Xhigh,
            ReasoningEffortLevel::Max,
        ] {
            round_trip(&effort);
        }
        for mode in [
            ResponsesReasoningMode::Standard,
            ResponsesReasoningMode::Pro,
        ] {
            round_trip(&mode);
        }
        let mut all_levels = reasoning_catalog_projection();
        all_levels.supported_effort_levels = vec![
            ReasoningEffortLevel::None,
            ReasoningEffortLevel::Minimal,
            ReasoningEffortLevel::Low,
            ReasoningEffortLevel::Medium,
            ReasoningEffortLevel::High,
            ReasoningEffortLevel::Xhigh,
            ReasoningEffortLevel::Max,
        ];
        assert!(all_levels.validate().is_ok());
        round_trip(&all_levels);
        let mut both_modes = reasoning_catalog_projection();
        both_modes.responses_reasoning_modes = vec![
            ResponsesReasoningMode::Standard,
            ResponsesReasoningMode::Pro,
        ];
        assert!(both_modes.validate().is_ok());
        round_trip(&both_modes);
        let mut duplicated = reasoning_catalog_projection();
        duplicated.supported_effort_levels =
            vec![ReasoningEffortLevel::High, ReasoningEffortLevel::High];
        assert_eq!(
            duplicated
                .validate()
                .expect_err("duplicate effort levels are rejected")
                .code(),
            "provider_reasoning_catalog_invalid"
        );
        let mut credential_projection = reasoning_catalog_projection();
        credential_projection.model_id = "sk-model".to_owned();
        assert_eq!(
            credential_projection
                .validate()
                .expect_err("credential-shaped projection field is rejected")
                .code(),
            "credentials_forbidden"
        );
    }

    #[test]
    fn zone2_dtos_and_events_never_serialize_fake_credentials() {
        const FAKE_SECRETS: [&str; 3] = ["sk-test", "Bearer secret", "api_key"];
        let payloads: Vec<String> = vec![
            serde_json::to_string(&catalog_query()).expect("catalog query serializes"),
            serde_json::to_string(&catalog_entry()).expect("catalog entry serializes"),
            serde_json::to_string(&catalog_page()).expect("catalog page serializes"),
            serde_json::to_string(&catalog_status_query()).expect("status query serializes"),
            serde_json::to_string(&catalog_status()).expect("catalog status serializes"),
            serde_json::to_string(&ProviderCatalogRemovalImpactDto {
                affected_profile_ids: vec!["profile-1".to_owned()],
                safe_impact_summary: "one profile affected".to_owned(),
            })
            .expect("removal impact serializes"),
            serde_json::to_string(&set_profile_command()).expect("set command serializes"),
            serde_json::to_string(&set_profile_accepted(true)).expect("accepted serializes"),
            serde_json::to_string(&session_profile_query()).expect("session query serializes"),
            serde_json::to_string(&session_profile()).expect("session profile serializes"),
            serde_json::to_string(&accept_removal_command()).expect("removal command serializes"),
            serde_json::to_string(&reject_candidate_command()).expect("reject command serializes"),
            serde_json::to_string(&reconcile_queue_command())
                .expect("reconcile command serializes"),
            serde_json::to_string(&reconcile_queue_accepted())
                .expect("reconcile accepted serializes"),
            serde_json::to_string(&admit_recovered_command()).expect("admit command serializes"),
            serde_json::to_string(&admit_recovered_accepted()).expect("admit accepted serializes"),
            serde_json::to_string(&usage_query()).expect("usage query serializes"),
            serde_json::to_string(&usage_aggregation()).expect("usage aggregation serializes"),
            serde_json::to_string(&ReasoningUsageDto {
                input_tokens: Some(1),
                output_tokens: None,
            })
            .expect("reasoning usage serializes"),
            serde_json::to_string(&candidate_prepared_event()).expect("prepared event serializes"),
            serde_json::to_string(&ProviderCatalogRemovalPendingEventDto {
                candidate_handle: "candidate-1".to_owned(),
                removal_revision_id: "catalog-rev-2".to_owned(),
                occurred_at: 100,
            })
            .expect("pending event serializes"),
            serde_json::to_string(&ProviderCatalogCandidateRejectedEventDto {
                candidate_handle: "candidate-1".to_owned(),
                safe_rejection_reason: "reviewer rejected".to_owned(),
                occurred_at: 100,
            })
            .expect("rejected event serializes"),
            serde_json::to_string(&ProviderCatalogCandidateExpiredEventDto {
                candidate_handle: "candidate-1".to_owned(),
                occurred_at: 100,
            })
            .expect("expired event serializes"),
            serde_json::to_string(&ProviderCatalogActivationRecoveryRequiredEventDto {
                candidate_handle: "candidate-1".to_owned(),
                safe_recovery_reason: "activation failed".to_owned(),
                occurred_at: 100,
            })
            .expect("recovery required event serializes"),
            serde_json::to_string(&ProviderCatalogRecoveryCompletedEventDto {
                active_catalog_revision_id: "catalog-rev-1".to_owned(),
                occurred_at: 100,
            })
            .expect("recovery completed event serializes"),
            serde_json::to_string(&SessionProviderProfileChangedEventDto {
                session_id: "session-1".to_owned(),
                previous_profile_id: "profile-default".to_owned(),
                profile_id: "profile-1".to_owned(),
                session_projection_revision: 8,
                occurred_at: 100,
            })
            .expect("session event serializes"),
            serde_json::to_string(&reload_command()).expect("reload command serializes"),
            serde_json::to_string(&reload_transaction(true))
                .expect("reload transaction serializes"),
            serde_json::to_string(&ConfigurationReloadedEventDto {
                transaction_id: "transaction-1".to_owned(),
                config_revision: "config-rev-2".to_owned(),
                occurred_at: 100,
            })
            .expect("reloaded event serializes"),
            serde_json::to_string(&ConfigurationReloadRejectedEventDto {
                transaction_id: "transaction-1".to_owned(),
                safe_failure_code: "validation_failed".to_owned(),
                safe_failure_detail: None,
                occurred_at: 100,
            })
            .expect("reload rejected event serializes"),
            serde_json::to_string(&rotate_credentials_command())
                .expect("rotation command serializes"),
            serde_json::to_string(&rotation_result()).expect("rotation result serializes"),
            serde_json::to_string(&health_evidence()).expect("health evidence serializes"),
            serde_json::to_string(&discovery_attempt()).expect("discovery attempt serializes"),
            serde_json::to_string(&discovery_record()).expect("discovery record serializes"),
            serde_json::to_string(&pricing_observation()).expect("pricing observation serializes"),
            serde_json::to_string(&raw_toml_edit()).expect("raw toml edit serializes"),
            serde_json::to_string(&typed_config_edit()).expect("typed edit serializes"),
            serde_json::to_string(&header_policy()).expect("header policy serializes"),
            serde_json::to_string(&ProviderPreservationControlsDto {
                preserve_thinking: true,
                thinking_keep: false,
            })
            .expect("preservation controls serialize"),
            serde_json::to_string(&ServerSideParserConfigDto::Vllm {
                parser_id: "parser-1".to_owned(),
                bounded_limits: "max-context=8192".to_owned(),
            })
            .expect("parser config serializes"),
            serde_json::to_string(&reasoning_catalog_projection())
                .expect("reasoning projection serializes"),
        ];
        for payload in payloads {
            let json = payload;
            for secret in FAKE_SECRETS {
                assert!(
                    !json.contains(secret),
                    "serialized control-plane DTO must not contain {secret:?}"
                );
            }
        }
    }

    #[test]
    fn zone2_credential_shaped_control_plane_fields_are_all_rejected() {
        let cases: Vec<Box<dyn Fn() -> DtoResult<()>>> = vec![
            Box::new(|| {
                let mut value = catalog_query();
                value.expected_catalog_revision_id = Some("sk-test".to_owned());
                value.validate()
            }),
            Box::new(|| {
                let mut value = catalog_entry();
                value.driver_declared_capabilities = vec!["Bearer secret".to_owned()];
                value.validate()
            }),
            Box::new(|| {
                let mut value = catalog_page();
                value.catalog_revision_id = "sk-test".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = catalog_status();
                value.active_default_profile_id = Some("api_key".to_owned());
                value.validate()
            }),
            Box::new(|| {
                let mut value = set_profile_accepted(true);
                value.session_id = "sk-test".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = session_profile();
                value.global_default_profile_id = "Bearer secret".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = reject_candidate_command();
                value.operation_id = "api_key".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = reconcile_queue_accepted();
                value.page_cursor = Some("sk-test".to_owned());
                value.validate()
            }),
            Box::new(|| {
                let mut value = admit_recovered_accepted();
                value.run_id = "Bearer secret".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = usage_query();
                value.profile_id = "sk-test".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = candidate_prepared_event();
                value.candidate_handle = "Bearer secret".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = reload_transaction(true);
                value.transaction_id = "sk-test".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = rotation_result();
                value.profile_id = "api_key".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = health_evidence();
                value.observed_availability = ProviderAvailabilityObservation::Unavailable;
                value.failure_category = Some(ProviderHealthFailureCategory::ServiceUnavailable);
                value.safe_diagnostic_code = Some("Bearer secret".to_owned());
                value.validate()
            }),
            Box::new(|| {
                let mut value = discovery_attempt();
                value.safe_status = "sk-test".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = discovery_record();
                value.model_id = "api_key".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = pricing_observation();
                value.provider_kind_id = "Bearer secret".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = typed_config_edit();
                value.expected_config_revision = "sk-test".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = header_policy();
                value.kind_descriptor_revision_id = "api_key".to_owned();
                value.validate()
            }),
            Box::new(|| {
                let mut value = reasoning_catalog_projection();
                value.projection_revision = "Bearer secret".to_owned();
                value.validate()
            }),
        ];
        for (index, case) in cases.into_iter().enumerate() {
            assert_eq!(
                case()
                    .expect_err("credential-shaped value is rejected")
                    .code(),
                "credentials_forbidden",
                "credential case {index} must fail closed"
            );
        }
    }

    #[test]
    fn zone2_closed_enums_reject_unknown_wire_values() {
        // "unexpected" matches no variant of any closed enum, unlike
        // "unknown", which is a valid ProviderAvailabilityObservation.
        for wire in ["\"unexpected\"", "\"READY\"", "\"ready \"", "42", "null"] {
            assert!(serde_json::from_str::<ProviderReadinessDto>(wire).is_err());
            assert!(serde_json::from_str::<ProviderCatalogActivationState>(wire).is_err());
            assert!(serde_json::from_str::<ProviderCatalogDegradedReason>(wire).is_err());
            assert!(serde_json::from_str::<ProviderProfileUnavailableReason>(wire).is_err());
            assert!(serde_json::from_str::<PricingClassification>(wire).is_err());
            assert!(serde_json::from_str::<ProviderDiscoveryPhase>(wire).is_err());
            assert!(serde_json::from_str::<ProviderAvailabilityObservation>(wire).is_err());
            assert!(serde_json::from_str::<ProviderHealthFailureCategory>(wire).is_err());
            assert!(serde_json::from_str::<ReasoningEffortLevel>(wire).is_err());
            assert!(serde_json::from_str::<ResponsesReasoningMode>(wire).is_err());
            assert!(serde_json::from_str::<ConfigurationOriginDto>(wire).is_err());
            assert!(serde_json::from_str::<ConfigurationValidationOutcomeDto>(wire).is_err());
            assert!(serde_json::from_str::<ConfigurationCommitOutcomeDto>(wire).is_err());
            assert!(serde_json::from_str::<ServerSideParserConfigDto>(wire).is_err());
            assert!(serde_json::from_str::<CredentialTransportMode>(wire).is_err());
        }
        // Tagged enums reject unknown tags and unknown variants.
        assert!(
            serde_json::from_str::<ResolvedProviderProfileDto>(r#"{"kind":"unknown","data":{}}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<ConfigurationEditOperationDto>(
                r#"{"kind":"unknown","data":{}}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<ConfigurationEditOperationDto>(
                r#"{"kind":"set","data":{"key_path":"a","safe_value":"b"}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn wired_domain_ledger_tags_have_matching_public_wire_families() {
        use intention_domain::canonical::{TagRegistry, TagStatus};

        // The domain-internal run-execution-meaning family owns the nested
        // M3/M4 selection records and is opaque to the protocol codec: it is
        // the one Wired ledger tag with no public wire family.
        const DOMAIN_INTERNAL_FAMILY_TAG: u32 = 0x0101;

        let mut wired: Vec<&intention_domain::canonical::LedgerTag> = TagRegistry::LEDGER
            .iter()
            .filter(|entry| entry.status == TagStatus::Wired)
            .collect();
        wired.sort_by_key(|entry| entry.value);

        // Every Wired ledger tag except the domain-internal execution-meaning
        // family must be covered by exactly the expected number of public
        // wire descriptors; the fork aliases share one tag for two versioned
        // descriptors.
        for entry in wired {
            if entry.value == DOMAIN_INTERNAL_FAMILY_TAG {
                assert!(
                    PUBLIC_WIRE_CONTRACT_FAMILIES
                        .iter()
                        .all(|descriptor| descriptor.tag != entry.value),
                    "{} must not claim the domain-internal ledger tag",
                    entry.name
                );
                continue;
            }
            let count = PUBLIC_WIRE_CONTRACT_FAMILIES
                .iter()
                .filter(|descriptor| descriptor.tag == entry.value)
                .count();
            let expected = if entry.value == TagRegistry::FORK_BASE_SNAPSHOT_V1
                || entry.value == TagRegistry::FORK_PREVIEW_V1
            {
                2
            } else {
                1
            };
            assert_eq!(
                count, expected,
                "Wired ledger tag {} (0x{:04X}) must be covered exactly {expected} time(s) by PUBLIC_WIRE_CONTRACT_FAMILIES",
                entry.name, entry.value
            );
        }

        // Conversely, every descriptor's tag must exist in the domain ledger.
        // Reserved tags are permitted today and become Wired as their codecs
        // land, so no assertion is made on their status yet.
        for descriptor in PUBLIC_WIRE_CONTRACT_FAMILIES {
            assert!(
                TagRegistry::LEDGER
                    .iter()
                    .any(|entry| entry.value == descriptor.tag),
                "{} must exist in the domain ledger",
                descriptor.name
            );
        }
    }
}
