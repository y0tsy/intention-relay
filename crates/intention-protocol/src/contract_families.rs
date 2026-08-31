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
#[must_use]
fn credential_shaped(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("key")
        || lower.contains("token")
        || value.contains('\n')
        || lower.starts_with("sk-")
        || lower.starts_with("bearer ")
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
    /// `sk-` prefixed secret, `Bearer` credential, or contains a newline;
    /// `invalid_endpoint` for endpoint forms carrying userinfo, query,
    /// fragment, or control characters; and `invalid_provider_kind` for the
    /// unnormalized `openai` kind.
    pub fn validate(&self) -> DtoResult<()> {
        let fields = [
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
        if fields.iter().any(|value| {
            let lower = value.to_ascii_lowercase();
            lower.contains("key")
                || lower.contains("token")
                || value.contains('\n')
                || lower.starts_with("sk-")
                || lower.starts_with("bearer ")
        }) {
            return Err(ErrorDto::validation(
                "credentials_forbidden",
                "credentials are forbidden",
            ));
        }
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
}
impl ForkSessionCommandDto {
    /// Validates the fork command's preview digest and optional title.
    ///
    /// # Errors
    ///
    /// Returns `invalid_digest` for a malformed preview digest and
    /// `invalid_title` for a blank, over-long, or control-bearing title.
    pub fn validate(&self) -> DtoResult<()> {
        digest(&self.expected_preview_digest)?;
        if let Some(title) = &self.requested_title {
            valid_text(title, 128, "invalid_title")?;
        }
        Ok(())
    }
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

impl From<ForkPreviewV1> for ForkPreviewDto {
    fn from(value: ForkPreviewV1) -> Self {
        Self {
            preview_digest: value.preview_digest,
            page_count: value.page_count,
            snapshot_size_bytes: value.snapshot_size_bytes,
        }
    }
}
impl From<ForkPreviewV2> for ForkPreviewDto {
    fn from(value: ForkPreviewV2) -> Self {
        Self {
            preview_digest: value.preview_digest,
            page_count: value.page_count,
            snapshot_size_bytes: value.snapshot_size_bytes,
        }
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
    /// entries or more than 256 entries, and `invalid_digest` for a malformed
    /// manifest digest.
    pub fn validate(&self) -> DtoResult<()> {
        if self.source_entries.is_empty() || self.source_entries.len() > 256 {
            return Err(ErrorDto::validation(
                "context_source_manifest_invalid",
                "context source manifest must carry between 1 and 256 entries",
            ));
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
    /// Validates the bounded message references.
    ///
    /// # Errors
    ///
    /// Returns `agent_message_reference_invalid` when the message carries more
    /// than sixteen typed references.
    pub fn validate(&self) -> DtoResult<()> {
        bounded(
            self.typed_references.clone(),
            16,
            "agent_message_reference_invalid",
        )
        .map(|_| ())
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
    /// # Errors
    ///
    /// Returns `agent_activity_record_too_large` when the string payload
    /// exceeds the 64 KiB record bound, `agent_message_reference_invalid`
    /// when the record carries more than sixteen typed references, and
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
            + self.canonical_record_digest.len();
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
    /// Returns `agent_notification_summary_too_large` when the record's text
    /// payload exceeds the 64 KiB per-page bound.
    pub fn validate(&self) -> DtoResult<()> {
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
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BridgeInvocationAcceptedDto {
    pub bridge_operation_id: String,
    pub tool_call_id: String,
    pub admission_state: String,
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
    /// Returns `tool_registry_revision_too_large` when the revision carries
    /// more than 256 descriptors or the descriptor fields exceed 512 KiB
    /// aggregate, `duplicate_tool_descriptor` for a repeated tool ID, and the
    /// per-descriptor failures for any invalid descriptor.
    pub fn validate(&self) -> DtoResult<()> {
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
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    /// Validates that the binding references legacy bytes by canonical
    /// `legacy-uuid:<canonical UUID>` reference.
    ///
    /// # Errors
    ///
    /// Returns `legacy_selection_reference_invalid` when the safe selection
    /// is not a lowercase canonical `legacy-uuid:` reference with a valid
    /// UUID variant and version.
    pub fn validate(&self) -> DtoResult<()> {
        if !is_canonical_legacy_uuid_reference(&self.legacy_safe_selection) {
            return Err(ErrorDto::validation(
                "legacy_selection_reference_invalid",
                "legacy selection must be a canonical legacy-uuid reference",
            ));
        }
        Ok(())
    }
}

/// Returns whether `value` is exactly `legacy-uuid:<canonical UUID>` where the
/// UUID is the lowercase hyphenated canonical form with a valid RFC 4122
/// variant and version.
#[must_use]
pub fn is_canonical_legacy_uuid_reference(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("legacy-uuid:") else {
        return false;
    };
    let bytes = rest.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    let is_lower_hex = |byte: u8| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase();
    for (index, &byte) in bytes.iter().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return false;
            }
            continue;
        }
        if !is_lower_hex(byte) {
            return false;
        }
    }
    // The third group's first digit is the RFC 4122 version.
    if !matches!(bytes[14], b'1'..=b'8') {
        return false;
    }
    // The fourth group's first digit is the RFC 4122 variant.
    if !matches!(bytes[19], b'8' | b'9' | b'a' | b'b') {
        return false;
    }
    true
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
pub const LEGACY_M4_SELECTION_BINDING: ContractFamilyDescriptor = ContractFamilyDescriptor {
    name: "legacy-m4-selection-binding",
    version: 1,
    tag: TagRegistry::LEGACY_M4_SELECTION_BINDING,
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
    LEGACY_M4_SELECTION_BINDING,
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
        round_trip(&ReasoningHistoryManifestDto {
            compatibility_id: "compat-1".to_owned(),
            entries: vec!["entry".to_owned()],
            manifest_digest: "a".repeat(64),
        });
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
        round_trip(&AgentNotificationRecordDto {
            notification_cursor: AgentNotificationCursorDto(4),
            activity_tree_id: AgentActivityTreeId("tree-1".to_owned()),
            activity_record_reference: "record-1".to_owned(),
            level: NotificationLevel::Ordinary,
            reason: "reason".to_owned(),
            safe_counts_and_states: "counts".to_owned(),
            occurred_at: 100,
        });
    }

    #[test]
    fn gateway_family_round_trips() {
        let grant = BridgeRunGrantDto {
            opaque_grant_identity: "grant-1".to_owned(),
            issued_protocol_revision: "rev-1".to_owned(),
        };
        round_trip(&grant);
        round_trip(&BridgeAttachmentResponseDto {
            bridge_run_grant: grant.clone(),
            negotiated_capabilities: vec![
                crate::ProtocolCapabilityDto::DaemonToolGatewayV1,
                crate::ProtocolCapabilityDto::ModelToolLoopV1,
            ],
            initial_run_cursor: 3,
        });
        round_trip(&BridgeInvocationCommandDto {
            bridge_run_grant: grant,
            bridge_operation_id: "operation-1".to_owned(),
            typed_tool_invocation: "invocation".to_owned(),
        });
        round_trip(&BridgeInvocationAcceptedDto {
            bridge_operation_id: "operation-1".to_owned(),
            tool_call_id: "call-1".to_owned(),
            admission_state: "admitted".to_owned(),
        });
        round_trip(&BridgeOperationV1 {
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
        });
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
        round_trip(&ModelToolLoopV1 {
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
        });
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
    fn legacy_m4_selection_binding_round_trips() {
        let binding = LegacyM4SelectionBindingDto {
            legacy_config_revision_id: "config-1".to_owned(),
            legacy_snapshot_schema: "schema-1".to_owned(),
            legacy_safe_selection: "legacy-uuid:11111111-1111-4111-8111-111111111111".to_owned(),
            default_profile_id: "profile-1".to_owned(),
            default_profile_revision_id: "rev-1".to_owned(),
            kind_descriptor_revision_id: "kind-rev-1".to_owned(),
            capability_subset: vec!["text".to_owned()],
            execution_policy: "execution-policy".to_owned(),
            driver_contract_revision: "driver-1.0".to_owned(),
        };
        assert!(binding.validate().is_ok());
        round_trip(&binding);
    }

    #[test]
    fn legacy_selection_reference_is_preserved_byte_for_byte() {
        let reference = "legacy-uuid:11111111-1111-4111-8111-111111111111";
        let binding = LegacyM4SelectionBindingDto {
            legacy_config_revision_id: "config-1".to_owned(),
            legacy_snapshot_schema: "schema-1".to_owned(),
            legacy_safe_selection: reference.to_owned(),
            default_profile_id: "profile-1".to_owned(),
            default_profile_revision_id: "rev-1".to_owned(),
            kind_descriptor_revision_id: "kind-rev-1".to_owned(),
            capability_subset: vec!["text".to_owned()],
            execution_policy: "execution-policy".to_owned(),
            driver_contract_revision: "driver-1.0".to_owned(),
        };
        let wire = serde_json::to_vec(&binding).expect("binding encodes");
        assert!(String::from_utf8_lossy(&wire).contains(reference));
        let decoded: LegacyM4SelectionBindingDto =
            serde_json::from_slice(&wire).expect("binding decodes");
        assert_eq!(decoded.legacy_safe_selection, reference);
        assert_eq!(decoded, binding);
    }

    #[test]
    fn legacy_selection_reference_validation_rejects_non_canonical_forms() {
        for value in [
            "selection",
            "legacy-uuid:",
            "legacy-uuid:11111111-1111-4111-8111-11111111111",
            "LEGACY-UUID:11111111-1111-4111-8111-111111111111",
            "legacy-uuid:11111111-1111-4111-8111-1111111111111",
            "legacy-uuid:11111111-1111-4111-8111-11111111111G",
            "legacy-uuid:11111111-1111-4111-8111-111111111111 ",
            "legacy-uuid:11111111-1111-4111-8111-111111111111\n",
            "legacy-uuid:/tmp/11111111-1111-4111-8111-111111111111",
            "legacy-uuid:11111111-1111-0111-8111-111111111111",
            "legacy-uuid:11111111-1111-4111-7111-111111111111",
        ] {
            assert!(
                !is_canonical_legacy_uuid_reference(value),
                "{value:?} must not be canonical"
            );
            let binding = LegacyM4SelectionBindingDto {
                legacy_config_revision_id: "config-1".to_owned(),
                legacy_snapshot_schema: "schema-1".to_owned(),
                legacy_safe_selection: value.to_owned(),
                default_profile_id: "profile-1".to_owned(),
                default_profile_revision_id: "rev-1".to_owned(),
                kind_descriptor_revision_id: "kind-rev-1".to_owned(),
                capability_subset: vec!["text".to_owned()],
                execution_policy: "execution-policy".to_owned(),
                driver_contract_revision: "driver-1.0".to_owned(),
            };
            assert_eq!(
                binding
                    .validate()
                    .expect_err("non-canonical reference is rejected")
                    .code(),
                "legacy_selection_reference_invalid"
            );
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
            include_str!("../tests/fixtures/goldens/hello-compatible-minor-v1.json"),
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

        let bridged_v1: ForkPreviewDto = v1.into();
        assert_eq!(bridged_v1.preview_digest, "a".repeat(64));
        assert_eq!(bridged_v1.page_count, 3);
        assert_eq!(bridged_v1.snapshot_size_bytes, 4096);
        let bridged_v2: ForkPreviewDto = v2.into();
        assert_eq!(bridged_v2, bridged_v1);
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
        const LEDGER_TAGS: [u32; 24] = [
            0x0201, 0x0202, 0x0203, 0x0204, 0x0205, 0x0206, 0x0207, 0x0208, 0x0209, 0x020A, 0x020B,
            0x020C, 0x0301, 0x0302, 0x0303, 0x0304, 0x0401, 0x0402, 0x0403, 0x0501, 0x0502, 0x0503,
            0x0504, 0x0505,
        ];
        let mut tags: Vec<u32> = PUBLIC_WIRE_CONTRACT_FAMILIES
            .iter()
            .map(|descriptor| descriptor.tag)
            .collect();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(
            tags, LEDGER_TAGS,
            "wire families must cover exactly the 24 ADR 0036 ledger tags"
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
        let registry: [(&str, u32); 26] = [
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
                "legacy-m4-selection-binding",
                TagRegistry::LEGACY_M4_SELECTION_BINDING,
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
}
