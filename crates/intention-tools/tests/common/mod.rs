//! Shared fixtures for the intention-tools integration suites.

#![allow(
    dead_code,
    reason = "Each integration target compiles this module and uses a different subset of its fixtures."
)]
#![allow(
    clippy::expect_used,
    reason = "Shared fixtures fail loudly with local context when deterministic setup breaks."
)]
#![allow(
    clippy::must_use_candidate,
    reason = "Shared fixtures are called for their effect on the fixture, not for the returned value alone."
)]

use intention_proto::{RunId, SessionId, ToolCallId, WorkspaceRelativePathDto, WorkspaceRootDto};
use intention_tools::{
    BoundedText, CancellationSignal, ToolContext, ToolDispatchOutcome, ToolInput, ToolResult,
    ToolService,
};
use std::path::Path;
use tempfile::TempDir;

/// Creates a uniquely prefixed temporary workspace directory.
pub fn fixture_dir(label: &str) -> TempDir {
    tempfile::Builder::new()
        .prefix(&format!("intention-tools-{label}-"))
        .tempdir()
        .expect("temporary workspace")
}

/// Resolves a fixture directory into a workspace root.
pub fn workspace(root: &Path) -> intention_tools::WorkspaceRoot {
    intention_tools::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(root.to_string_lossy().into_owned()).expect("root dto"),
    )
    .expect("workspace")
}

/// Builds a tool service over the fixture directory.
pub fn service(root: &TempDir) -> ToolService {
    ToolService::new(workspace(root.path()))
}

/// Creates a temporary workspace directory with a tool service bound to it.
pub fn fixture_service() -> (TempDir, ToolService) {
    let dir = fixture_dir("service");
    let service = service(&dir);
    (dir, service)
}

/// Parses a workspace-relative fixture path.
pub fn relative(path: &str) -> WorkspaceRelativePathDto {
    WorkspaceRelativePathDto::parse(path).expect("relative path")
}

/// Builds bounded fixture text.
pub fn text(value: &str) -> BoundedText {
    BoundedText::new(value).expect("bounded text")
}

/// Builds the shared fixture context that identifies one tool call.
pub fn fixture_context(call_id: ToolCallId) -> ToolContext {
    ToolContext {
        session_id: SessionId::parse("00000000-0000-4000-8000-000000000001").expect("session id"),
        run_id: RunId::parse("00000000-0000-4000-8000-000000000002").expect("run id"),
        call_id,
    }
}

/// Test adapter: unwraps one completed dispatch and fails loudly on any
/// interruption, so fixtures that expect a final typed result stay direct.
pub trait DispatchCompleted {
    fn dispatch_completed(
        &self,
        call: ToolCallId,
        input: ToolInput,
        cancellation: CancellationSignal,
    ) -> ToolResult;
}

impl DispatchCompleted for ToolService {
    fn dispatch_completed(
        &self,
        call: ToolCallId,
        input: ToolInput,
        cancellation: CancellationSignal,
    ) -> ToolResult {
        match self
            .dispatch_with_cancellation(call, input, cancellation)
            .expect("completed dispatch succeeds")
        {
            ToolDispatchOutcome::Completed(result) => result,
            ToolDispatchOutcome::Interrupted { cause, partial } => {
                unreachable!("unexpected interrupted dispatch: {cause:?} {partial:?}")
            }
        }
    }
}
