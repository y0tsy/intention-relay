use crate::{CancellationSignal, ExecuteInput, ToolResult};
use intention_types::DtoResult;
use intention_workspace::WorkspaceRoot;

pub(super) fn run(
    root: &WorkspaceRoot,
    input: ExecuteInput,
    cancellation: CancellationSignal,
) -> DtoResult<ToolResult> {
    super::execute_tool(root, input, cancellation)
}
