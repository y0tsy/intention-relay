use crate::WorkspaceRoot;
use crate::{CancellationSignal, ExecuteInput, ExecutedOutcome};
use intention_proto::DtoResult;

pub fn run(
    root: &WorkspaceRoot,
    input: ExecuteInput,
    cancellation: CancellationSignal,
) -> DtoResult<ExecutedOutcome> {
    super::execute_tool(root, input, cancellation)
}
