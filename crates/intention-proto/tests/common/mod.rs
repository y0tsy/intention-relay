#![allow(
    clippy::expect_used,
    reason = "Shared protocol fixtures use expect for precise test diagnostics."
)]

use intention_proto::{
    MessageId, MessageKindDto, MessageProjectionDto, ProjectId, RunId, RunModeDto, SessionId,
    SessionProjectionDto, WorkspaceId, WorkspaceRootDto,
};

/// Returns one native fixture workspace root under the temporary directory.
pub fn fixture_workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-proto-contracts-workspace")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("fixture workspace root is valid")
}

/// Returns one coherent session projection for `session_id`.
pub fn fixture_projection(session_id: SessionId) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        fixture_workspace_root(),
        RunModeDto::Build,
        None,
        None,
        Vec::new(),
    )
    .expect("fixture projection is valid")
}

/// Returns one transcript notice row owned by `run_id` in `session_id`.
pub fn fixture_message(session_id: SessionId, run_id: RunId) -> MessageProjectionDto {
    MessageProjectionDto::new(
        MessageId::new(1).expect("fixture row identity is valid"),
        session_id,
        Some(run_id),
        MessageKindDto::Notice,
        "fixture notice",
        None,
        None,
        None,
    )
    .expect("fixture message is valid")
}
