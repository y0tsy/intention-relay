//! Domain value rules and invariants for Intention Relay.
//!
//! Shared value and wire types live in `intention-proto`; this crate keeps the
//! durable domain lifecycle rules and the remaining domain records.

use intention_proto::{DtoResult, ErrorDto, RunStatusDto};
use serde::{Deserialize, Deserializer, Serialize, de};

/// The durable lifecycle status for a physical plan.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatusDto {
    /// The plan exists and can be edited.
    Drafting,
    /// The plan body is being revised.
    Revising,
    /// The plan awaits a user decision.
    Submitted,
    /// The user accepted the plan.
    Approved,
    /// The user rejected the plan and may provide feedback.
    Rejected,
    /// A later plan superseded this plan.
    Superseded,
    /// The plan was explicitly abandoned.
    Abandoned,
}

/// Returns whether no future run status transition is valid from `status`.
#[must_use]
pub const fn run_status_is_terminal(status: RunStatusDto) -> bool {
    matches!(
        status,
        RunStatusDto::Completed | RunStatusDto::Failed | RunStatusDto::Interrupted
    )
}

/// Validates one durable run lifecycle transition without accessing runtime or storage state.
///
/// # Errors
///
/// Returns a conflict error when `to` is not a declared successor of `from`.
pub fn validate_run_status_transition(from: RunStatusDto, to: RunStatusDto) -> DtoResult<()> {
    let allowed = matches!(
        (from, to),
        (RunStatusDto::Starting, RunStatusDto::Running)
            | (RunStatusDto::Starting, RunStatusDto::Failed)
            | (RunStatusDto::Starting, RunStatusDto::Interrupted)
            | (RunStatusDto::Running, RunStatusDto::Completed)
            | (RunStatusDto::Running, RunStatusDto::Failed)
            | (RunStatusDto::Running, RunStatusDto::Interrupted)
    );
    if allowed {
        Ok(())
    } else {
        Err(ErrorDto::new(
            "invalid_run_status_transition",
            intention_proto::ErrorCategoryDto::Conflict,
            "run status transition is not permitted",
            intention_proto::ErrorRetryDto::Never,
            None,
        )?)
    }
}

/// Validates a plan lifecycle transition.
///
/// # Errors
///
/// Returns a conflict error when the requested transition is not permitted.
pub fn validate_plan_status_transition(
    from: Option<PlanStatusDto>,
    to: PlanStatusDto,
) -> DtoResult<()> {
    let allowed = matches!(
        (from, to),
        (None, PlanStatusDto::Drafting)
            | (
                Some(PlanStatusDto::Drafting),
                PlanStatusDto::Revising | PlanStatusDto::Submitted | PlanStatusDto::Abandoned
            )
            | (
                Some(PlanStatusDto::Revising),
                PlanStatusDto::Revising | PlanStatusDto::Submitted | PlanStatusDto::Abandoned
            )
            | (
                Some(PlanStatusDto::Submitted),
                PlanStatusDto::Approved | PlanStatusDto::Rejected | PlanStatusDto::Abandoned
            )
            | (
                Some(PlanStatusDto::Rejected),
                PlanStatusDto::Revising | PlanStatusDto::Abandoned
            )
    );
    if allowed {
        Ok(())
    } else {
        Err(ErrorDto::new(
            "invalid_plan_status_transition",
            intention_proto::ErrorCategoryDto::Conflict,
            "plan status transition is not permitted",
            intention_proto::ErrorRetryDto::Never,
            None,
        )?)
    }
}

/// The terminal outcome recorded for one local tool result.
///
/// The taxonomy is deliberately closed to terminal outcomes: a call records
/// exactly one result row, and admission, rejection, and start evidence are
/// not persisted.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatusDto {
    /// The tool completed and produced a normalized safe result.
    Completed,
    /// The tool reported a safe failure outcome.
    Failed,
    /// The tool stopped because its run was cancelled.
    Cancelled,
    /// The tool stopped before a final outcome; its captured output is partial.
    Partial,
}

/// One credential-free structured metadata entry of a tool result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolResultMetadataEntryDto {
    key: String,
    value: String,
}

impl<'de> Deserialize<'de> for ToolResultMetadataEntryDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawToolResultMetadataEntryDto {
            key: String,
            value: String,
        }

        let raw = RawToolResultMetadataEntryDto::deserialize(deserializer)?;
        Self::new(raw.key, raw.value).map_err(de::Error::custom)
    }
}

impl ToolResultMetadataEntryDto {
    /// Creates one metadata entry with a non-blank key.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the key is blank or either field
    /// contains a NUL character.
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> DtoResult<Self> {
        let key = key.into();
        let value = value.into();
        if key.trim().is_empty() || key.contains('\0') || value.contains('\0') {
            return Err(ErrorDto::validation(
                "invalid_tool_result_metadata",
                "tool result metadata must have a non-blank key",
            ));
        }
        Ok(Self { key, value })
    }

    /// Returns the stable metadata key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns the metadata value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;
    use intention_proto::{
        ConfigRevisionId, GetSessionSnapshotQueryDto, InterruptRunCommandDto, MessageKindDto,
        MessageProjectionDto, PendingTurnProjectionDto, ProjectId, RunId, RunModeDto,
        RunProjectionDto, SessionId, SessionProjectionDto, ToolCallId, TurnId, WorkspaceId,
        WorkspaceRootDto,
    };

    fn fixture_workspace_root() -> WorkspaceRootDto {
        WorkspaceRootDto::parse(
            std::env::temp_dir()
                .join("intention-domain-unit-workspace")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("native fixture workspace is absolute")
    }

    #[test]
    fn run_and_plan_statuses_round_trip_through_wire_values() {
        for mode in [RunModeDto::Plan, RunModeDto::Build] {
            let encoded = serde_json::to_string(&mode).expect("mode serialization succeeds");
            let decoded: RunModeDto =
                serde_json::from_str(&encoded).expect("mode parsing succeeds");
            assert_eq!(decoded, mode);
        }
        for status in [
            RunStatusDto::Starting,
            RunStatusDto::Running,
            RunStatusDto::Completed,
            RunStatusDto::Failed,
            RunStatusDto::Interrupted,
        ] {
            let encoded = serde_json::to_string(&status).expect("status serialization succeeds");
            let decoded: RunStatusDto =
                serde_json::from_str(&encoded).expect("status parsing succeeds");
            assert_eq!(decoded, status);
        }
        for status in [PlanStatusDto::Drafting, PlanStatusDto::Approved] {
            let encoded = serde_json::to_string(&status).expect("status serialization succeeds");
            let decoded: PlanStatusDto =
                serde_json::from_str(&encoded).expect("status parsing succeeds");
            assert_eq!(decoded, status);
        }

        let session_id = SessionId::new();
        let run_id = RunId::new();
        let interrupt = InterruptRunCommandDto::new(session_id, run_id);
        assert_eq!(interrupt.session_id(), session_id);
        assert_eq!(interrupt.run_id(), run_id);
        let query = GetSessionSnapshotQueryDto::new(session_id);
        assert_eq!(query.session_id(), session_id);
    }

    #[test]
    fn session_projection_rejects_a_pending_turn_from_another_session() {
        let session_id = SessionId::new();
        let projection = SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            fixture_workspace_root(),
            RunModeDto::Build,
            None,
            None,
            vec![
                PendingTurnProjectionDto::new(SessionId::new(), TurnId::new(), "hello")
                    .expect("pending turn is non-empty"),
            ],
        );
        assert!(projection.is_err());

        let projection = SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            fixture_workspace_root(),
            RunModeDto::Build,
            None,
            Some(RunProjectionDto::new(
                session_id,
                RunId::new(),
                TurnId::new(),
                RunStatusDto::Running,
                ConfigRevisionId::new(),
            )),
            Vec::new(),
        )
        .expect("coherent session projection is valid");
        assert_eq!(projection.session_id(), session_id);
        assert_eq!(
            projection.active_run().map(RunProjectionDto::status),
            Some(RunStatusDto::Running)
        );
    }

    #[test]
    fn run_status_transitions_accept_only_declared_edges() {
        let statuses = [
            RunStatusDto::Starting,
            RunStatusDto::Running,
            RunStatusDto::Completed,
            RunStatusDto::Failed,
            RunStatusDto::Interrupted,
        ];
        for from in statuses {
            for to in statuses {
                let allowed = matches!(
                    (from, to),
                    (RunStatusDto::Starting, RunStatusDto::Running)
                        | (RunStatusDto::Starting, RunStatusDto::Failed)
                        | (RunStatusDto::Starting, RunStatusDto::Interrupted)
                        | (RunStatusDto::Running, RunStatusDto::Completed)
                        | (RunStatusDto::Running, RunStatusDto::Failed)
                        | (RunStatusDto::Running, RunStatusDto::Interrupted)
                );
                assert_eq!(
                    validate_run_status_transition(from, to).is_ok(),
                    allowed,
                    "unexpected run transition: {from:?} -> {to:?}"
                );
            }
        }
    }

    #[test]
    fn transcript_rows_validate_their_closed_shapes() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let call_id = ToolCallId::new();
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::User,
                "hello",
                None,
                None,
                None,
            )
            .is_ok()
        );
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::Assistant,
                "answer",
                Some("reasoning".to_owned()),
                None,
                None,
            )
            .is_ok()
        );
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::ToolCall,
                "{\"path\":\"src/lib.rs\"}",
                None,
                Some(call_id),
                Some("read".to_owned()),
            )
            .is_ok()
        );
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::ToolResult,
                "contents",
                Some("reasoning".to_owned()),
                Some(call_id),
                Some("read".to_owned()),
            )
            .is_err()
        );
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::ToolResult,
                " ",
                None,
                Some(call_id),
                Some("read".to_owned()),
            )
            .is_err()
        );
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::Notice,
                "notice",
                None,
                Some(call_id),
                Some("read".to_owned()),
            )
            .is_err()
        );
        assert!(
            MessageProjectionDto::new(
                session_id,
                Some(run_id),
                MessageKindDto::ToolCall,
                "{}",
                None,
                None,
                Some("read".to_owned()),
            )
            .is_err()
        );
        let decoded: MessageProjectionDto = serde_json::from_str(
            &serde_json::to_string(
                &MessageProjectionDto::new(
                    session_id,
                    None,
                    MessageKindDto::Notice,
                    "a notice",
                    None,
                    None,
                    None,
                )
                .expect("notice row is valid"),
            )
            .expect("notice row serializes"),
        )
        .expect("notice row deserializes");
        assert_eq!(decoded.kind(), MessageKindDto::Notice);
        assert_eq!(decoded.run_id(), None);
    }
}
