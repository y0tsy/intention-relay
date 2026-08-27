use crate::{EditInput, ReadInput, ToolResult, WriteInput, bounded_lossy, bounded_text};
use intention_types::DtoResult;
use intention_workspace::WorkspaceRoot;

pub(super) fn read(root: &WorkspaceRoot, input: ReadInput) -> DtoResult<ToolResult> {
    super::read_tool(root, input)
}
pub(super) fn write(root: &WorkspaceRoot, input: WriteInput) -> DtoResult<ToolResult> {
    super::write_tool(root, input)
}
pub(super) fn edit(root: &WorkspaceRoot, input: EditInput) -> DtoResult<ToolResult> {
    super::edit_tool(root, input)
}

#[allow(
    dead_code,
    reason = "keeps helper references available for testable tool wiring"
)]
fn _keep_helpers_visible(_: &[u8]) {
    let _ = (bounded_lossy, bounded_text);
}
