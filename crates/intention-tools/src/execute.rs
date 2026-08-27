use crate::{CancellationSignal, ExecuteInput, ExecutedTool};
use intention_types::DtoResult;
use intention_workspace::WorkspaceRoot;

pub fn run(
    root: &WorkspaceRoot,
    input: ExecuteInput,
    cancellation: CancellationSignal,
) -> DtoResult<ExecutedTool> {
    super::execute_tool(root, input, cancellation)
}
