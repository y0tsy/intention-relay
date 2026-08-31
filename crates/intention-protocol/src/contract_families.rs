//! Additive protocol contract-family DTOs.
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
pub struct AgentActivityTreeId(pub String);
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
        round_trip(&AgentActivityJournalRecordDto {
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
        });
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
        round_trip(&LegacyM4SelectionBindingDto {
            legacy_config_revision_id: "config-1".to_owned(),
            legacy_snapshot_schema: "schema-1".to_owned(),
            legacy_safe_selection: "selection".to_owned(),
            default_profile_id: "profile-1".to_owned(),
            default_profile_revision_id: "rev-1".to_owned(),
            kind_descriptor_revision_id: "kind-rev-1".to_owned(),
            capability_subset: vec!["text".to_owned()],
            execution_policy: "execution-policy".to_owned(),
            driver_contract_revision: "driver-1.0".to_owned(),
        });
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
}
