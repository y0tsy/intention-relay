use crate::{GlobInput, GrepInput, ToolResult};
use intention_types::DtoResult;
use intention_workspace::WorkspaceRoot;

pub(super) fn glob(root: &WorkspaceRoot, input: GlobInput) -> DtoResult<ToolResult> {
    super::glob_tool(root, input)
}
pub(super) fn grep(root: &WorkspaceRoot, input: GrepInput) -> DtoResult<ToolResult> {
    super::grep_tool(root, input)
}
