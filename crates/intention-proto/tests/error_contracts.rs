#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Safe-error decode and wire-validation contract evidence.

use intention_proto::{
    CorrelationIdDto, ErrorCategoryDto, ErrorDto, ErrorRetryDto, WorkspaceRelativePathDto,
};

#[test]
fn current_shape_minimal_error_decodes_absent_optional_fields_as_none() {
    // The current ErrorDto shape keeps `correlation_id` optional on the wire: a
    // minimal current error without it must decode as None and re-encode
    // without inventing a value.
    let error: ErrorDto = serde_json::from_str(
        r#"{"code":"fixture","category":"not_found","message":"safe","retry":"manual"}"#,
    )
    .expect("minimal current-shape error decodes");
    assert_eq!(error.code(), "fixture");
    assert_eq!(error.category(), ErrorCategoryDto::NotFound);
    assert_eq!(error.retry(), ErrorRetryDto::Manual);
    assert!(
        error.correlation_id().is_none(),
        "absent correlation is None"
    );
    let encoded = serde_json::to_string(&error).expect("test serialization must succeed");
    let decoded: ErrorDto =
        serde_json::from_str(&encoded).expect("round trip must decode identically");
    assert_eq!(decoded, error);
}

#[test]
fn malformed_error_wire_data_is_rejected() {
    for invalid_path in [
        "",
        "/etc/passwd",
        "../escape",
        "src/../escape",
        "src/\u{0000}bad",
    ] {
        assert!(WorkspaceRelativePathDto::parse(invalid_path).is_err());
    }
    let correlation = CorrelationIdDto::new();
    let encoded = serde_json::to_string(&correlation).expect("correlation serializes");
    assert_eq!(
        serde_json::from_str::<CorrelationIdDto>(&encoded).expect("correlation decodes"),
        correlation
    );
    let parsed_path =
        WorkspaceRelativePathDto::parse("src/parsed.rs").expect("fixture path is valid");
    assert_eq!(parsed_path.as_str(), "src/parsed.rs");
    for non_canonical in [
        "aaaaaaaaaaaaaaaa4aaa8aaaaaaaaaaaaaaa",
        "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
    ] {
        assert!(CorrelationIdDto::parse(non_canonical).is_err());
    }

    for wire in [
        r#"{"code":"","category":"not_found","message":"safe","retry":"manual"}"#,
        r#"{"code":"fixture","category":"not_found","message":" ","retry":"manual"}"#,
        r#"{"code":"fixture","category":"not_found","message":"safe","retry":"manual","correlation_id":"not-a-uuid"}"#,
    ] {
        assert!(serde_json::from_str::<ErrorDto>(wire).is_err());
    }
}
