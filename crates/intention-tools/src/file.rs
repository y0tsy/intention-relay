use crate::{CancellationSignal, EditInput, ExecutedOutcome, ReadInput, WriteInput};
use intention_proto::DtoResult;
use intention_workspace::WorkspaceRoot;

pub fn read(
    root: &WorkspaceRoot,
    input: ReadInput,
    cancellation: &CancellationSignal,
) -> DtoResult<ExecutedOutcome> {
    super::read_tool(root, input, cancellation)
}
pub fn write(
    root: &WorkspaceRoot,
    input: WriteInput,
    cancellation: &CancellationSignal,
) -> DtoResult<ExecutedOutcome> {
    super::write_tool(root, input, cancellation)
}
pub fn edit(
    root: &WorkspaceRoot,
    input: EditInput,
    cancellation: &CancellationSignal,
) -> DtoResult<ExecutedOutcome> {
    super::edit_tool(root, input, cancellation)
}
