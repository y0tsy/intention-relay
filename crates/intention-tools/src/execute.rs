use crate::{CancellationSignal, ExecuteInput, ExecutedOutcome};
use intention_types::DtoResult;
use intention_workspace::WorkspaceRoot;

pub fn run(
    root: &WorkspaceRoot,
    input: ExecuteInput,
    cancellation: CancellationSignal,
) -> DtoResult<ExecutedOutcome> {
    super::execute_tool(root, input, cancellation)
}
