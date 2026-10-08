#![allow(
    clippy::expect_used,
    reason = "M5 tool-result contract fixtures use expect for precise diagnostics."
)]

//! Typed, credential-free durable tool-result contract evidence.

use intention_domain::{ToolResultMetadataEntryDto, ToolResultStatusDto};

#[test]
fn tool_result_status_set_is_closed_to_terminal_outcomes() {
    for status in [
        ToolResultStatusDto::Completed,
        ToolResultStatusDto::Failed,
        ToolResultStatusDto::Partial,
    ] {
        let wire = serde_json::to_string(&status).expect("status serializes");
        let decoded: ToolResultStatusDto = serde_json::from_str(&wire).expect("status decodes");
        assert_eq!(decoded, status);
    }
    assert_eq!(
        serde_json::to_value(ToolResultStatusDto::Completed).expect("status serializes to JSON"),
        serde_json::json!("completed")
    );
    assert_eq!(
        serde_json::to_value(ToolResultStatusDto::Failed).expect("status serializes to JSON"),
        serde_json::json!("failed")
    );
    assert_eq!(
        serde_json::to_value(ToolResultStatusDto::Partial).expect("status serializes to JSON"),
        serde_json::json!("partial")
    );
    for undeclared in ["started", "admitted", "rejected"] {
        assert!(serde_json::from_str::<ToolResultStatusDto>(&format!("\"{undeclared}\"")).is_err());
    }
}

#[test]
fn tool_result_metadata_entries_validate_keys_and_preserve_values() {
    let entry =
        ToolResultMetadataEntryDto::new("bytes", "17").expect("bounded metadata entry is valid");
    assert_eq!(entry.key(), "bytes");
    assert_eq!(entry.value(), "17");
    let decoded: ToolResultMetadataEntryDto =
        serde_json::from_str(&serde_json::to_string(&entry).expect("entry serializes"))
            .expect("entry decodes");
    assert_eq!(decoded, entry);

    assert!(ToolResultMetadataEntryDto::new(" ", "v").is_err());
    assert!(ToolResultMetadataEntryDto::new("bad\0key", "v").is_err());
    assert!(ToolResultMetadataEntryDto::new("k", "bad\0value").is_err());

    let empty_value =
        ToolResultMetadataEntryDto::new("k", "").expect("an empty metadata value is allowed");
    assert_eq!(empty_value.value(), "");
    // Keys and values beyond the former 128-byte and 1 KiB caps are accepted
    // and preserved exactly.
    let long_key =
        ToolResultMetadataEntryDto::new("x".repeat(129), "v").expect("129-byte key is accepted");
    assert_eq!(long_key.key().len(), 129);
    let long_value =
        ToolResultMetadataEntryDto::new("k", "x".repeat(1025)).expect("1 KiB + 1 value");
    assert_eq!(long_value.value().len(), 1025);
}

#[test]
fn tool_result_metadata_wire_shape_is_closed_and_additive_tolerant() {
    let entry = ToolResultMetadataEntryDto::new("bytes", "17").expect("metadata entry is valid");
    let encoded = serde_json::to_value(&entry).expect("entry serializes to JSON");

    // The metadata entry persists exactly the documented typed fields.
    assert_eq!(encoded, serde_json::json!({"key": "bytes", "value": "17"}));
    assert_eq!(
        encoded.as_object().expect("entry is a JSON object").len(),
        2
    );

    let mut additive = encoded;
    additive["future_additive_field"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ToolResultMetadataEntryDto>(additive).is_ok());

    assert!(
        serde_json::from_value::<ToolResultMetadataEntryDto>(serde_json::json!({"value": "17"}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<ToolResultMetadataEntryDto>(serde_json::json!({"key": "bytes"}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<ToolResultMetadataEntryDto>(
            serde_json::json!({"key": " ", "value": "17"})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ToolResultMetadataEntryDto>(
            serde_json::json!({"key": "bytes", "value": "bad\0value"})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ToolResultMetadataEntryDto>(
            serde_json::json!({"key": 7, "value": "17"})
        )
        .is_err()
    );
}
