#![allow(
    clippy::expect_used,
    reason = "Reasoning surface fixtures use expect for precise test failure messages."
)]

use intention_model::{
    AuthenticationHeaderPolicyV1, CredentialTransportMode, FinishReasonDto, ModelEventDto,
    ModelStreamLifecycleDto, ReasoningEffortLevel, ReasoningFragmentCategoryDto,
};

#[test]
fn reasoning_deltas_round_trip_with_categorized_wire_shapes() {
    let categorized: ModelEventDto = serde_json::from_str(
        r#"{"kind":"reasoning_delta","category":"detail","content":"detail"}"#,
    )
    .expect("categorized reasoning delta decodes");
    assert_eq!(
        categorized,
        ModelEventDto::ReasoningDelta {
            category: ReasoningFragmentCategoryDto::Detail,
            content: "detail".to_owned(),
        }
    );

    let defaulted = ModelEventDto::reasoning_delta("primary").expect("primary delta is valid");
    assert!(matches!(
        defaulted,
        ModelEventDto::ReasoningDelta {
            category: ReasoningFragmentCategoryDto::Primary,
            ..
        }
    ));
    let explicit =
        ModelEventDto::reasoning_delta_categorized(ReasoningFragmentCategoryDto::Detail, "detail")
            .expect("detail delta is valid");
    let decoded: ModelEventDto =
        serde_json::from_str(&serde_json::to_string(&explicit).expect("event serializes"))
            .expect("event deserializes");
    assert_eq!(decoded, explicit);

    let summary = ModelEventDto::reasoning_summary_delta("summary").expect("summary is valid");
    assert_eq!(
        summary,
        ModelEventDto::ReasoningSummaryDelta {
            content: "summary".to_owned(),
        }
    );
    let decoded: ModelEventDto =
        serde_json::from_str(&serde_json::to_string(&summary).expect("event serializes"))
            .expect("event deserializes");
    assert_eq!(decoded, summary);
    let decoded: ModelEventDto =
        serde_json::from_str(r#"{"kind":"reasoning_summary_delta","content":"summary"}"#)
            .expect("reasoning summary wire decodes");
    assert_eq!(decoded, summary);

    assert!(ModelEventDto::reasoning_delta("").is_err());
    assert!(
        ModelEventDto::reasoning_delta_categorized(ReasoningFragmentCategoryDto::Primary, "")
            .is_err()
    );
    assert!(ModelEventDto::reasoning_summary_delta("").is_err());
    assert!(
        serde_json::from_str::<ModelEventDto>(
            r#"{"kind":"reasoning_delta","category":"bogus","content":"x"}"#
        )
        .is_err()
    );
}

#[test]
fn stream_lifecycle_accepts_reasoning_summary_deltas_like_reasoning_deltas() {
    let mut lifecycle = ModelStreamLifecycleDto::new();
    lifecycle
        .accept(&ModelEventDto::started())
        .expect("start is valid");
    lifecycle
        .accept(&ModelEventDto::reasoning_summary_delta("summary").expect("summary is valid"))
        .expect("summary before finish is valid");
    lifecycle
        .accept(&ModelEventDto::finished(FinishReasonDto::Stop))
        .expect("finish is valid");
    assert!(lifecycle.is_terminal());

    let mut before_start = ModelStreamLifecycleDto::new();
    assert_eq!(
        before_start
            .accept(&ModelEventDto::reasoning_summary_delta("summary").expect("summary is valid"))
            .expect_err("summary before start must fail")
            .code(),
        "invalid_model_stream_order"
    );
}

#[test]
fn reasoning_effort_and_mode_are_closed_and_snake_case() {
    for (level, wire) in [
        (ReasoningEffortLevel::None, "none"),
        (ReasoningEffortLevel::Minimal, "minimal"),
        (ReasoningEffortLevel::Low, "low"),
        (ReasoningEffortLevel::Medium, "medium"),
        (ReasoningEffortLevel::High, "high"),
        (ReasoningEffortLevel::Xhigh, "xhigh"),
        (ReasoningEffortLevel::Max, "max"),
    ] {
        let encoded = serde_json::to_string(&level).expect("effort serializes");
        assert_eq!(encoded, format!("\"{wire}\""));
        let decoded: ReasoningEffortLevel =
            serde_json::from_str(&encoded).expect("effort deserializes");
        assert_eq!(decoded, level);
    }
    assert!(
        serde_json::from_str::<ReasoningEffortLevel>("\"extreme\"")
            .expect_err("unknown effort is rejected")
            .to_string()
            .contains("unknown variant")
    );
}

#[test]
fn header_policy_validates_names_and_transport_consistency() {
    let bearer = AuthenticationHeaderPolicyV1::new(Vec::new(), CredentialTransportMode::Bearer)
        .expect("bearer policy is valid");
    assert!(bearer.allowed_header_names().is_empty());
    assert_eq!(bearer.selected_transport(), CredentialTransportMode::Bearer);

    let safe = AuthenticationHeaderPolicyV1::new(
        vec!["X-Custom-Auth".to_owned()],
        CredentialTransportMode::SafeHeader,
    )
    .expect("safe-header policy is valid");
    assert_eq!(safe.allowed_header_names(), &["X-Custom-Auth".to_owned()]);
    assert_eq!(
        safe.selected_transport(),
        CredentialTransportMode::SafeHeader
    );

    for invalid in [
        Vec::new(),
        vec!["X-Space Header".to_owned()],
        vec!["X:Colon".to_owned()],
        vec!["".to_owned()],
        vec!["X-Duplicate".to_owned(), "X-Duplicate".to_owned()],
        vec!["x".repeat(129)],
        vec!["X-Control\u{0}".to_owned()],
    ] {
        assert!(
            AuthenticationHeaderPolicyV1::new(invalid, CredentialTransportMode::SafeHeader)
                .is_err(),
            "invalid safe-header names must be rejected"
        );
    }
    assert!(
        AuthenticationHeaderPolicyV1::new(
            vec!["X-Custom-Auth".to_owned()],
            CredentialTransportMode::Bearer,
        )
        .is_err()
    );
    assert!(
        AuthenticationHeaderPolicyV1::new(Vec::new(), CredentialTransportMode::SafeHeader).is_err()
    );

    let decoded: AuthenticationHeaderPolicyV1 =
        serde_json::from_str(&serde_json::to_string(&safe).expect("policy serializes"))
            .expect("policy deserializes");
    assert_eq!(decoded, safe);
    assert!(
        serde_json::from_str::<AuthenticationHeaderPolicyV1>(
            r#"{"allowed_header_names":["X-Custom-Auth"],"selected_transport":"safe_header","extra":true}"#
        )
        .is_err()
    );
}
