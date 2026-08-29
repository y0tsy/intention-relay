#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Coverage fixtures use infallible setup values; failures indicate broken test setup."
)]

use intention_domain::WorkspaceRootDto;
use intention_tools::*;
use intention_types::{ToolCallId, WorkspaceRelativePathDto};
use tempfile::TempDir;

fn service() -> (TempDir, ToolService) {
    let dir = tempfile::tempdir().unwrap();
    let dto = WorkspaceRootDto::parse(dir.path().to_string_lossy().into_owned()).unwrap();
    let root = intention_workspace::WorkspaceRoot::resolve(&dto).unwrap();
    (dir, ToolService::new(root))
}
fn path(s: &str) -> WorkspaceRelativePathDto {
    WorkspaceRelativePathDto::parse(s).unwrap()
}
fn text(s: &str) -> BoundedText {
    BoundedText::new(s).unwrap()
}

#[test]
fn exercises_plain_grep_and_envelope_fallback() {
    let (dir, service) = service();
    std::fs::write(dir.path().join("x.txt"), "é needle\nnope\n").unwrap();
    let result = service
        .dispatch(
            ToolCallId::new(),
            ToolInput::Grep(GrepInput {
                pattern: text("needle"),
                scope: None,
                path: Some(path("x.txt")),
            }),
        )
        .unwrap();
    let ToolResult::Grep(result) = result else {
        return;
    };
    assert_eq!(result.matches[0].column, 3);
    let envelope = ToolResultEnvelope {
        schema_version: TOOL_SCHEMA_VERSION,
        context: ToolContext {
            session_id: intention_types::SessionId::new(),
            run_id: intention_types::RunId::new(),
            call_id: ToolCallId::new(),
        },
        result: ToolResult::Read(TextResult { text: text("ok") }),
        observability: ToolObservability {
            outcome: ToolOutcome::Succeeded,
            policy: ToolPolicy::Denied,
            elapsed_ms: 7,
        },
        execution: None,
    };
    assert_eq!(envelope.projection().execution.policy, ToolPolicy::Denied);
}

#[test]
fn exercises_cancelled_and_schema_rejected_invocations() {
    let (_dir, service) = service();
    let ctx = ToolContext {
        session_id: intention_types::SessionId::new(),
        run_id: intention_types::RunId::new(),
        call_id: ToolCallId::new(),
    };
    let cancelled = service
        .dispatch_with_cancellation(
            ToolCallId::new(),
            ToolInput::Glob(GlobInput {
                pattern: text("*.txt"),
            }),
            CancellationSignal::cancelled(),
        )
        .unwrap_err();
    assert_eq!(cancelled.code(), "tool_cancelled");
    let bad = service
        .invoke_enveloped(ToolInvocation {
            schema_version: 99,
            context: ctx,
            input: ToolInput::Glob(GlobInput {
                pattern: text("*.txt"),
            }),
        })
        .unwrap_err();
    assert_eq!(bad.code(), "tool_schema_mismatch");
}

#[test]
fn exercises_projection_variants_and_bounded_text_validation() {
    assert!(BoundedText::new("a\0b").is_err());
    let p = ToolResult::Glob(PathsResult {
        paths: vec![path("a")],
    })
    .projection();
    assert!(matches!(p.content, ToolProjectedContent::Paths { .. }));
    let p = ToolResult::Grep(GrepResult { matches: vec![] }).projection();
    assert!(matches!(p.content, ToolProjectedContent::Matches { .. }));
    let p = ToolResult::Write(WriteResult { bytes: 3 }).projection();
    assert!(matches!(
        p.content,
        ToolProjectedContent::Mutation { bytes: 3 }
    ));
}
