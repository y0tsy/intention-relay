#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "Coverage fixtures use infallible setup values; failures indicate broken test setup."
)]

use intention_tools::*;
use intention_types::WorkspaceRelativePathDto;

fn p(s: &str) -> WorkspaceRelativePathDto {
    WorkspaceRelativePathDto::parse(s).unwrap()
}
fn t(s: &str) -> BoundedText {
    BoundedText::new(s).unwrap()
}

#[test]
fn logical_paths_cover_all_inputs() {
    let path = p("x.txt");
    let inputs = [
        ToolInput::Read(ReadInput { path: path.clone() }),
        ToolInput::Write(WriteInput {
            path: path.clone(),
            content: t("x"),
            expected_content: None,
        }),
        ToolInput::Edit(EditInput {
            path: path.clone(),
            old: t("x"),
            new: t("y"),
            expected_content: None,
        }),
        ToolInput::Grep(GrepInput {
            pattern: t("x"),
            path: Some(path.clone()),
            scope: None,
        }),
        ToolInput::Glob(GlobInput { pattern: t("*") }),
        ToolInput::Execute(ExecuteInput {
            program: t("true"),
            args: vec![],
        }),
    ];
    assert_eq!(inputs[0].logical_path(), Some(&path));
    assert_eq!(inputs[1].logical_path(), Some(&path));
    assert_eq!(inputs[2].logical_path(), Some(&path));
    assert_eq!(inputs[3].logical_path(), Some(&path));
    assert!(inputs[4].logical_path().is_none() && inputs[5].logical_path().is_none());
}
