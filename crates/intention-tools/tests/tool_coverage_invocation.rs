#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Coverage fixtures use infallible setup values; failures indicate broken test setup."
)]

use intention_domain::WorkspaceRootDto;
use intention_proto::{RunId, SessionId, ToolCallId, WorkspaceRelativePathDto};
use intention_tools::{
    BoundedText, GlobInput, TOOL_SCHEMA_VERSION, ToolContext, ToolExecutionMetadata, ToolInput,
    ToolInvocation, ToolPolicy, ToolProcessStatus, ToolService,
};

fn service() -> (tempfile::TempDir, ToolService) {
    let dir = tempfile::tempdir().unwrap();
    let root = intention_workspace::WorkspaceRoot::resolve(
        &WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap(),
    )
    .unwrap();
    (dir, ToolService::new(root))
}

fn context(call_id: ToolCallId) -> ToolContext {
    ToolContext {
        session_id: SessionId::parse("00000000-0000-4000-8000-000000000001").unwrap(),
        run_id: RunId::parse("00000000-0000-4000-8000-000000000002").unwrap(),
        call_id,
    }
}

#[test]
fn metadata_builders_and_schema_validation_are_checked() {
    let path = WorkspaceRelativePathDto::parse("file.txt").unwrap();
    let metadata = ToolExecutionMetadata::for_workspace(ToolPolicy::Denied, 7)
        .with_path(Some(path.clone()))
        .with_process_status(Some(ToolProcessStatus::NonZero { code: 3 }));
    assert_eq!(metadata.path, Some(path));
    assert_eq!(
        metadata.process_status,
        Some(ToolProcessStatus::NonZero { code: 3 })
    );

    let (_dir, service) = service();
    let error = service
        .invoke_enveloped(ToolInvocation {
            schema_version: TOOL_SCHEMA_VERSION + 1,
            context: context(ToolCallId::new()),
            input: ToolInput::Glob(GlobInput {
                pattern: BoundedText::new("*").unwrap(),
            }),
        })
        .unwrap_err();
    assert_eq!(error.code(), "tool_schema_mismatch");
}
