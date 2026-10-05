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

/// Test adapter: unwraps one completed dispatch and fails loudly on any
/// interruption, so fixtures that expect a final typed result stay direct.
trait DispatchCompleted {
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

#[test]
fn exercises_plain_grep_and_envelope_fallback() {
    let (dir, service) = service();
    std::fs::write(dir.path().join("x.txt"), "é needle\nnope\n").unwrap();
    let result = service.dispatch_completed(
        ToolCallId::new(),
        ToolInput::Grep(GrepInput {
            pattern: text("needle"),
            scope: None,
            path: Some(path("x.txt")),
        }),
        CancellationSignal::new(),
    );
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
        result: ToolResult::Read(TextResult {
            text: text("ok"),
            truncated: false,
        }),
        observability: ToolObservability {
            outcome: ToolOutcome::Succeeded,
            policy: ToolPolicy::Denied,
            elapsed_ms: 7,
        },
        execution: None,
    };
    assert_eq!(envelope.projection().execution.policy, ToolPolicy::Denied);
}
