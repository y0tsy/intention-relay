#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Coverage fixtures use infallible setup values; failures indicate broken test setup."
)]

use intention_domain::WorkspaceRootDto;
use intention_tools::{
    BoundedText, EditInput, GrepInput, GrepScope, ReadInput, TOOL_SCHEMA_VERSION, ToolContext,
    ToolExecutionMetadata, ToolInput, ToolInvocation, ToolOutcome, ToolPolicy, ToolProcessStatus,
    ToolResult, ToolResultEnvelope, ToolService,
};
use intention_types::{RunId, SessionId, ToolCallId, WorkspaceRelativePathDto};

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
fn invocation_constructor_and_context_path_metadata_are_checked() {
    let (_dir, service) = service();
    let call = ToolCallId::new();
    let path = WorkspaceRelativePathDto::parse("file.txt").unwrap();
    let invocation = ToolInvocation::new(
        TOOL_SCHEMA_VERSION,
        context(call),
        ToolInput::Read(ReadInput { path: path.clone() }),
        call,
    )
    .unwrap();
    assert_eq!(invocation.context.call_id, call);
    assert!(
        ToolInvocation::new(
            TOOL_SCHEMA_VERSION,
            context(call),
            ToolInput::Read(ReadInput { path: path.clone() }),
            ToolCallId::new(),
        )
        .is_err()
    );

    let error = service
        .invoke_enveloped(ToolInvocation {
            schema_version: TOOL_SCHEMA_VERSION + 1,
            context: context(call),
            input: ToolInput::Read(ReadInput { path }),
        })
        .unwrap_err();
    assert_eq!(error.code(), "tool_schema_mismatch");

    let error = service
        .invoke_with_context(
            context(ToolCallId::new()),
            ToolInput::Read(ReadInput {
                path: WorkspaceRelativePathDto::parse("missing").unwrap(),
            }),
        )
        .unwrap_err();
    assert_eq!(error.code(), "workspace_path_unavailable");
}

#[test]
fn metadata_builders_and_projection_cover_all_result_shapes() {
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
    let call = ToolCallId::new();
    let envelope = service
        .invoke_enveloped(ToolInvocation {
            schema_version: TOOL_SCHEMA_VERSION,
            context: context(call),
            input: ToolInput::Grep(GrepInput {
                pattern: BoundedText::new("x").unwrap(),
                path: Some(WorkspaceRelativePathDto::parse("missing").unwrap()),
                scope: Some(GrepScope::File {
                    path: WorkspaceRelativePathDto::parse("missing").unwrap(),
                }),
            }),
        })
        .unwrap_err();
    assert_eq!(envelope.code(), "workspace_path_unavailable");

    let bare = ToolResult::Edit(intention_tools::WriteResult { bytes: 4 });
    assert_eq!(bare.tool_id().as_str(), "edit");
    let projection = bare.projection();
    assert!(matches!(
        projection.content,
        intention_tools::ToolProjectedContent::Mutation { bytes: 4 }
    ));

    let envelope = ToolResultEnvelope {
        schema_version: TOOL_SCHEMA_VERSION,
        context: context(call),
        result: ToolResult::Read(intention_tools::TextResult {
            text: BoundedText::new("ok").unwrap(),
        }),
        observability: intention_tools::ToolObservability {
            outcome: ToolOutcome::Failed,
            policy: ToolPolicy::Denied,
            elapsed_ms: 9,
        },
        execution: None,
    };
    assert_eq!(envelope.projection().execution.policy, ToolPolicy::Denied);
}

#[test]
fn read_and_edit_failures_are_typed_through_context_invocation() {
    let (dir, service) = service();
    std::fs::create_dir(dir.path().join("directory")).unwrap();
    let path = WorkspaceRelativePathDto::parse("directory").unwrap();
    for input in [
        ToolInput::Read(ReadInput { path: path.clone() }),
        ToolInput::Edit(EditInput {
            path,
            old: BoundedText::new("x").unwrap(),
            new: BoundedText::new("y").unwrap(),
            expected_content: None,
        }),
    ] {
        let error = service
            .invoke_with_context(context(ToolCallId::new()), input)
            .unwrap_err();
        assert!(matches!(
            error.code(),
            "tool_read_failed" | "tool_edit_conflict"
        ));
    }
}
