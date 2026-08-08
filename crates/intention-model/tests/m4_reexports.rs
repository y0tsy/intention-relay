#![allow(
    clippy::expect_used,
    reason = "M4 model compatibility fixtures use expect for precise diagnostics."
)]

use intention_model::{FinishReasonDto, ProviderErrorDto, ToolCallDto, UsageDto};
use intention_types::{CorrelationIdDto, ErrorRetryDto, ToolCallId};

#[test]
fn types_owned_model_values_remain_available_through_model_reexports() {
    let call = ToolCallDto::new(ToolCallId::new(), "inspect", r#"{"path":"src"}"#)
        .expect("tool call remains valid");
    assert_eq!(call.name(), "inspect");
    assert_eq!(
        UsageDto::reported(3, 4, 7).expect("usage remains valid"),
        intention_types::UsageDto::reported(3, 4, 7).expect("types usage is valid")
    );
    assert_eq!(
        FinishReasonDto::Stop,
        intention_types::FinishReasonDto::Stop
    );
    let error =
        ProviderErrorDto::unavailable("provider_unavailable", false, Some(CorrelationIdDto::new()))
            .expect("provider failure remains valid");
    assert_eq!(error.retry(), ErrorRetryDto::Never);
}
