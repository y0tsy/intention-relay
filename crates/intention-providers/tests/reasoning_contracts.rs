#![allow(
    clippy::expect_used,
    reason = "Reasoning contract fixtures use expect to provide precise test failure messages."
)]

use intention_proto::ReasoningFragmentCategoryDto as ProtoCategory;
use intention_providers::{
    AssistantReasoningHistoryDto, FinishReasonDto, MAX_MODEL_REASONING_FRAGMENT_BYTES,
    ModelEventDto, ModelMessageDto, ModelRoleDto, ModelStreamLifecycleDto,
    ReasoningFragmentCategoryDto as ProviderCategory, UsageDto,
};

fn fragment(category: ProviderCategory, content: &str) -> ModelEventDto {
    ModelEventDto::reasoning_delta(category, content).expect("fixture fragment is valid")
}

fn summary(content: &str) -> ModelEventDto {
    ModelEventDto::reasoning_summary_delta(content).expect("fixture summary is valid")
}

#[test]
fn reasoning_categories_are_the_closed_proto_values() {
    // The re-exported category is the proto type itself, not a local copy.
    let categories: [ProviderCategory; 2] = [ProtoCategory::Primary, ProtoCategory::Detail];
    assert_eq!(categories, [ProtoCategory::Primary, ProtoCategory::Detail]);
    assert_eq!(
        serde_json::to_value(ProtoCategory::Primary).expect("category serializes"),
        serde_json::json!("primary")
    );
    assert_eq!(
        serde_json::to_value(ProtoCategory::Detail).expect("category serializes"),
        serde_json::json!("detail")
    );
}

#[test]
fn reasoning_fragments_require_content_and_keep_their_category() {
    let primary = fragment(ProviderCategory::Primary, "weighing options");
    assert_eq!(
        primary,
        ModelEventDto::ReasoningDelta {
            category: ProviderCategory::Primary,
            content: "weighing options".to_owned(),
        }
    );
    let detail = fragment(ProviderCategory::Detail, "weighing options");
    assert_ne!(
        primary, detail,
        "equal text in different closed categories stays distinct"
    );

    assert_eq!(
        ModelEventDto::reasoning_delta(ProviderCategory::Primary, "")
            .expect_err("an empty fragment is not a value")
            .code(),
        "invalid_model_reasoning_delta"
    );
}

#[test]
fn reasoning_summaries_require_content_and_stay_distinct_from_fragments() {
    let summary = summary("condensed answer");
    assert_eq!(
        summary,
        ModelEventDto::ReasoningSummaryDelta {
            content: "condensed answer".to_owned(),
        }
    );
    assert_ne!(
        summary,
        fragment(ProviderCategory::Primary, "condensed answer"),
        "a summary is never a reasoning fragment"
    );
    assert_eq!(
        ModelEventDto::reasoning_summary_delta("")
            .expect_err("an empty summary is not a value")
            .code(),
        "invalid_model_reasoning_summary_delta"
    );
}

#[test]
fn reasoning_fragments_and_summaries_are_bounded_per_fragment_without_truncation() {
    let at_bound = "a".repeat(MAX_MODEL_REASONING_FRAGMENT_BYTES);
    let accepted = ModelEventDto::reasoning_delta(ProviderCategory::Detail, at_bound)
        .expect("a fragment at the bound is accepted");
    assert!(
        matches!(
            &accepted,
            ModelEventDto::ReasoningDelta { category, content }
                if *category == ProviderCategory::Detail
                    && content.len() == MAX_MODEL_REASONING_FRAGMENT_BYTES
        ),
        "a fragment at the bound stays whole and keeps its category"
    );

    let oversized = "a".repeat(MAX_MODEL_REASONING_FRAGMENT_BYTES + 1);
    assert_eq!(
        ModelEventDto::reasoning_delta(ProviderCategory::Primary, oversized.clone())
            .expect_err("an over-bound fragment is rejected")
            .code(),
        "provider_reasoning_fragment_too_large"
    );
    assert_eq!(
        ModelEventDto::reasoning_summary_delta(oversized)
            .expect_err("an over-bound summary is rejected")
            .code(),
        "provider_reasoning_fragment_too_large"
    );

    // The bound counts bytes: a multi-byte fragment over the byte bound is
    // rejected whole, never truncated to a shorter value.
    let multibyte = "é".repeat(MAX_MODEL_REASONING_FRAGMENT_BYTES / 2 + 1);
    assert_eq!(
        ModelEventDto::reasoning_delta(ProviderCategory::Primary, multibyte)
            .expect_err("the bound counts bytes, not characters")
            .code(),
        "provider_reasoning_fragment_too_large"
    );
}

#[test]
fn the_lifecycle_accepts_fragments_and_summaries_before_the_terminal_fact() {
    let mut lifecycle = ModelStreamLifecycleDto::new();
    let ordered = [
        ModelEventDto::started(),
        fragment(ProviderCategory::Primary, "first"),
        ModelEventDto::reasoning_presence(),
        fragment(ProviderCategory::Detail, "second"),
        summary("condensed"),
        ModelEventDto::text_delta("answer").expect("fixture text is valid"),
        ModelEventDto::usage(UsageDto::reported(2, 3, 5).expect("fixture usage is valid")),
        ModelEventDto::finished(FinishReasonDto::Stop),
    ];
    for event in &ordered {
        lifecycle
            .accept(event)
            .expect("ordered reasoning events are valid");
    }
}

#[test]
fn reasoning_values_outside_the_live_window_fail_with_the_closed_failure() {
    let mut before_start = ModelStreamLifecycleDto::new();
    for event in [
        fragment(ProviderCategory::Primary, "early"),
        summary("early"),
    ] {
        assert_eq!(
            before_start
                .accept(&event)
                .expect_err("reasoning before start must fail")
                .code(),
            "provider_reasoning_stream_invalid"
        );
    }

    let mut after_terminal = ModelStreamLifecycleDto::new();
    after_terminal
        .accept(&ModelEventDto::started())
        .expect("start is valid");
    after_terminal
        .accept(&ModelEventDto::finished(FinishReasonDto::Stop))
        .expect("finish is valid");
    for event in [fragment(ProviderCategory::Detail, "late"), summary("late")] {
        assert_eq!(
            after_terminal
                .accept(&event)
                .expect_err("reasoning after the terminal fact must fail")
                .code(),
            "provider_reasoning_stream_invalid"
        );
    }
    assert_eq!(
        after_terminal
            .accept(&ModelEventDto::text_delta("late").expect("fixture text is valid"))
            .expect_err("no ordinary fact follows the terminal finish")
            .code(),
        "invalid_model_stream_order"
    );
    assert_eq!(
        after_terminal
            .accept(&ModelEventDto::started())
            .expect_err("a second start is invalid")
            .code(),
        "invalid_model_stream_order"
    );
}

#[test]
fn normalized_reasoning_events_serialize_with_a_required_category() {
    assert_eq!(
        serde_json::to_value(fragment(ProviderCategory::Detail, "detail"))
            .expect("fragment serializes"),
        serde_json::json!({
            "kind": "reasoning_delta",
            "category": "detail",
            "content": "detail",
        })
    );
    assert_eq!(
        serde_json::to_value(summary("condensed")).expect("summary serializes"),
        serde_json::json!({
            "kind": "reasoning_summary_delta",
            "content": "condensed",
        })
    );
    assert_eq!(
        serde_json::to_value(ModelEventDto::reasoning_presence()).expect("presence serializes"),
        serde_json::json!({
            "kind": "reasoning_delta",
            "category": "primary",
            "content": "",
        })
    );
}

#[test]
fn reasoning_presence_is_the_empty_primary_fragment() {
    let presence = ModelEventDto::reasoning_presence();
    assert_eq!(
        presence,
        ModelEventDto::ReasoningDelta {
            category: ProviderCategory::Primary,
            content: String::new(),
        }
    );
    let mut lifecycle = ModelStreamLifecycleDto::new();
    lifecycle
        .accept(&ModelEventDto::started())
        .expect("start is valid");
    lifecycle
        .accept(&presence)
        .expect("presence is a live reasoning fact");
    assert_eq!(
        ModelEventDto::reasoning_delta(ProviderCategory::Primary, "")
            .expect_err("empty text is not a reasoning value")
            .code(),
        "invalid_model_reasoning_delta"
    );
}

fn history(
    compatibility_id: &str,
    fragments: Vec<(ProviderCategory, String)>,
    summaries: Vec<String>,
) -> AssistantReasoningHistoryDto {
    AssistantReasoningHistoryDto::new(compatibility_id, fragments, summaries)
        .expect("fixture history is valid")
}

#[test]
fn attached_reasoning_history_keeps_its_parts_and_stays_out_of_message_text() {
    let attached = history(
        "generic-chat-reasoning-content-v1",
        vec![
            (ProviderCategory::Primary, "first ".to_owned()),
            (ProviderCategory::Detail, "second".to_owned()),
        ],
        vec!["condensed".to_owned()],
    );
    assert_eq!(
        attached.compatibility_id(),
        "generic-chat-reasoning-content-v1"
    );
    assert_eq!(
        attached.fragments(),
        &[
            (ProviderCategory::Primary, "first ".to_owned()),
            (ProviderCategory::Detail, "second".to_owned()),
        ],
        "the recorded fragment order and categories survive the attachment"
    );
    assert_eq!(attached.summaries(), ["condensed".to_owned()]);

    let message = ModelMessageDto::assistant_with_reasoning_history("the answer", attached.clone())
        .expect("assistant history message is valid");
    assert_eq!(message.role(), ModelRoleDto::Assistant);
    assert_eq!(
        message.content(),
        "the answer",
        "prior reasoning never enters the ordinary message text"
    );
    assert_eq!(message.reasoning_history(), Some(&attached));
    assert!(
        message.tool_calls().is_none() && message.tool_call_id().is_none(),
        "an assistant history message is a plain assistant response"
    );

    let serialized = serde_json::to_value(&message).expect("message serializes");
    assert_eq!(serialized["content"], "the answer");
    assert_eq!(serialized["reasoning_history"], {
        serde_json::json!({
            "compatibility_id": "generic-chat-reasoning-content-v1",
            "fragments": [["primary", "first "], ["detail", "second"]],
            "summaries": ["condensed"],
        })
    });
    assert!(
        !serialized["content"]
            .as_str()
            .is_some_and(|content| content.contains("first")
                || content.contains("second")
                || content.contains("condensed")),
        "the serialized text never absorbs the attached reasoning"
    );

    // A plain assistant or tool message carries no history key at all.
    let plain = ModelMessageDto::new(ModelRoleDto::Assistant, "answer").expect("message is valid");
    assert!(plain.reasoning_history().is_none());
    assert!(
        serde_json::to_value(&plain)
            .expect("message serializes")
            .get("reasoning_history")
            .is_none()
    );
}

#[test]
fn attached_reasoning_history_rejects_unrepresentable_shapes() {
    // The compatibility identity is the frozen transfer contract's own token.
    assert_eq!(
        AssistantReasoningHistoryDto::new("not a token", vec![], vec!["summary".to_owned()])
            .expect_err("an invalid compatibility identity is rejected")
            .code(),
        "invalid_reasoning_history_transfer"
    );
    assert_eq!(
        AssistantReasoningHistoryDto::new("generic-chat-reasoning-content-v1", vec![], vec![])
            .expect_err("an empty attachment is not a history")
            .code(),
        "invalid_model_assistant_reasoning_history"
    );
    assert_eq!(
        AssistantReasoningHistoryDto::new(
            "generic-chat-reasoning-content-v1",
            vec![(
                ProviderCategory::Primary,
                "a".repeat(MAX_MODEL_REASONING_FRAGMENT_BYTES + 1)
            )],
            vec![],
        )
        .expect_err("an over-bound fragment is rejected whole")
        .code(),
        "invalid_model_assistant_reasoning_history_text"
    );
    assert_eq!(
        AssistantReasoningHistoryDto::new(
            "generic-chat-reasoning-content-v1",
            vec![],
            vec!["not\u{7}transportable".to_owned()],
        )
        .expect_err("a control character never re-enters a provider request")
        .code(),
        "invalid_model_assistant_reasoning_history_text"
    );
    // The aggregate is the frozen 4 MiB history bound: nine in-bound fragments
    // cross it, and the attachment is rejected instead of truncated.
    let per_fragment = "a".repeat(MAX_MODEL_REASONING_FRAGMENT_BYTES);
    assert_eq!(
        AssistantReasoningHistoryDto::new(
            "generic-chat-reasoning-content-v1",
            (0..9)
                .map(|_| (ProviderCategory::Primary, per_fragment.clone()))
                .collect(),
            vec![],
        )
        .expect_err("the history aggregate bound rejects the whole attachment")
        .code(),
        "invalid_model_assistant_reasoning_history_size"
    );
    assert!(
        AssistantReasoningHistoryDto::new(
            "generic-chat-reasoning-content-v1",
            (0..8)
                .map(|_| (ProviderCategory::Primary, per_fragment.clone()))
                .collect(),
            vec![],
        )
        .is_ok(),
        "an attachment at the aggregate bound stays representable"
    );

    // Only assistant messages carry history, and an empty presence fragment is
    // still a representable reason to attach one.
    let presence_only = history(
        "generic-chat-reasoning-content-v1",
        vec![(ProviderCategory::Primary, String::new())],
        vec![],
    );
    let mut user = ModelMessageDto::new(ModelRoleDto::User, "hello").expect("message is valid");
    assert_eq!(
        user.attach_reasoning_history(presence_only)
            .expect_err("only assistant messages carry reasoning history")
            .code(),
        "invalid_model_message_role"
    );
    assert!(user.reasoning_history().is_none());
}
