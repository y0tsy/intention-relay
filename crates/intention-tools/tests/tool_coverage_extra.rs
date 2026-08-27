use intention_tools::{BoundedText, TOOL_SCHEMA_VERSION, ToolContext, ToolInput, ToolInvocation};
use intention_types::{RunId, SessionId, ToolCallId};

#[test]
fn bounded_text_rejects_nul_and_oversized_values() {
    assert!(BoundedText::new("a\0b").is_err());
    assert!(BoundedText::new("x".repeat(1_048_577)).is_err());
}

#[test]
fn invocation_validates_schema_and_call_identity() {
    let context = ToolContext {
        session_id: SessionId::new(),
        run_id: RunId::new(),
        call_id: ToolCallId::new(),
    };
    let input = ToolInput::Execute(intention_tools::ExecuteInput {
        program: BoundedText::new("true")
            .unwrap_or_else(|_| unreachable!("static program fixture")),
        args: vec![],
    });
    let invocation = ToolInvocation {
        schema_version: 99,
        context,
        input,
    };
    assert!(invocation.validate_schema_version().is_err());
    assert!(invocation.validate_call_id(ToolCallId::new()).is_err());
    assert_eq!(TOOL_SCHEMA_VERSION, 1);
}
