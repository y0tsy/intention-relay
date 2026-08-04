#![allow(
    clippy::expect_used,
    reason = "M4 shared model-value fixtures use expect for precise diagnostics."
)]

use intention_types::{
    CorrelationIdDto, ErrorRetryDto, ProviderErrorDto, ToolCallDto, ToolCallId, UsageDto,
};

#[test]
fn model_values_are_owned_by_types_and_preserve_the_contract() {
    let call = ToolCallDto::new(ToolCallId::new(), "inspect", r#"{"path":"src"}"#)
        .expect("object arguments are valid");
    assert_eq!(call.name(), "inspect");
    assert!(ToolCallDto::new(ToolCallId::new(), "inspect", "[]").is_err());

    let usage = UsageDto::reported(2, 3, 5).expect("consistent usage is valid");
    assert_eq!(
        serde_json::to_string(&usage).expect("usage serializes"),
        r#"{"state":"reported","input_tokens":2,"output_tokens":3,"total_tokens":5}"#
    );
    assert!(UsageDto::reported(u64::MAX, 1, 0).is_err());
    assert!(UsageDto::reported(u64::MAX, 1, u64::MAX).is_err());

    let correlation = CorrelationIdDto::new();
    let error = ProviderErrorDto::unavailable("provider_unavailable", true, Some(correlation))
        .expect("safe provider failure is valid");
    assert_eq!(error.retry(), ErrorRetryDto::Delayed);
    assert_eq!(error.correlation_id(), Some(correlation));
    assert_eq!(error.to_string(), "provider_unavailable");
}
