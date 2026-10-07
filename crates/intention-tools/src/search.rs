use crate::{CancellationSignal, ExecutedOutcome, GlobInput, GrepInput};
use intention_proto::DtoResult;
use intention_workspace::WorkspaceRoot;

pub fn glob(
    root: &WorkspaceRoot,
    input: GlobInput,
    cancellation: &CancellationSignal,
) -> DtoResult<ExecutedOutcome> {
    super::glob_tool(root, input, cancellation)
}
pub fn grep(
    root: &WorkspaceRoot,
    input: GrepInput,
    cancellation: &CancellationSignal,
) -> DtoResult<ExecutedOutcome> {
    super::grep_tool(root, input, cancellation)
}
