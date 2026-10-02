use intention_tools::TOOL_SCHEMA_VERSION;

#[test]
fn tool_schema_version_is_current() {
    assert_eq!(TOOL_SCHEMA_VERSION, 1);
}
