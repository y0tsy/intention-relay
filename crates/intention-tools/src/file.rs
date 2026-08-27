use crate::{EditInput, ReadInput, ToolResult, WriteInput};
use intention_types::DtoResult;
use intention_workspace::WorkspaceRoot;

pub fn read(root: &WorkspaceRoot, input: ReadInput) -> DtoResult<ToolResult> {
    super::read_tool(root, input)
}
pub fn write(root: &WorkspaceRoot, input: WriteInput) -> DtoResult<ToolResult> {
    super::write_tool(root, input)
}
pub fn edit(root: &WorkspaceRoot, input: EditInput) -> DtoResult<ToolResult> {
    super::edit_tool(root, input)
}
