#![allow(
    clippy::expect_used,
    reason = "Contract fixtures use expect to provide precise test failure messages."
)]

//! Versioned fixture compatibility and safe-error contract evidence.

use intention_types::{
    CorrelationIdDto, ErrorCategoryDto, ErrorDetailDto, ErrorDto, ErrorRetryDto,
    WorkspaceRelativePathDto,
};

const FAKE_CREDENTIAL: &str = "fixture-credential-not-real-12345";
const WORKSPACE_ROOT: &str = "/workspace/private-project";

#[test]
fn versioned_missing_path_fixture_decodes_and_round_trips_safely() {
    let error: ErrorDto = serde_json::from_str(include_str!(
        "fixtures/error-v1-missing-workspace-path.json"
    ))
    .expect("typed error fixture must decode");

    let encoded = serde_json::to_string(&error).expect("test serialization must succeed");
    assert!(!encoded.contains(FAKE_CREDENTIAL));
    assert!(!encoded.contains(WORKSPACE_ROOT));
    assert!(!error.to_string().contains("src/missing.rs"));
}

#[test]
fn typed_missing_path_detail_round_trips_without_exposing_root_or_secret() {
    let path = WorkspaceRelativePathDto::parse("src/missing.rs").expect("fixture path is valid");
    let correlation = CorrelationIdDto::parse("11111111-1111-4111-8111-111111111111")
        .expect("fixture correlation identifier is valid");
    let error = ErrorDto::with_detail(
        "workspace_path_not_found",
        ErrorCategoryDto::NotFound,
        "the requested workspace path was not found",
        ErrorRetryDto::Manual,
        Some(correlation),
        ErrorDetailDto::MissingWorkspacePath { path },
    )
    .expect("fixture error is valid");

    let encoded = serde_json::to_string(&error).expect("test serialization must succeed");
    let decoded: ErrorDto =
        serde_json::from_str(&encoded).expect("test deserialization must succeed");

    assert_eq!(decoded, error);
    assert!(error.detail().is_some(), "typed detail is retained");
    assert!(error.correlation_id().is_some(), "correlation is retained");
    assert!(!encoded.contains(FAKE_CREDENTIAL));
    assert!(!encoded.contains(WORKSPACE_ROOT));
    assert!(!error.to_string().contains("src/missing.rs"));
    let correlation_text = correlation.as_str();
    assert!(!error.to_string().contains(&correlation_text));
}

#[test]
fn current_shape_minimal_error_decodes_absent_optional_fields_as_none() {
    // The current ErrorDto shape keeps `correlation_id` and `detail` optional
    // on the wire: a minimal current error without either field must decode
    // both as None and re-encode without inventing values.
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
    assert!(error.detail().is_none(), "absent detail is None");
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
    assert!(CorrelationIdDto::parse(&correlation.as_str()).is_ok());
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
        r#"{"code":"fixture","category":"not_found","message":"safe","retry":"manual","detail":{"kind":"missing_workspace_path","path":"/etc/passwd"}}"#,
        r#"{"code":"fixture","category":"not_found","message":"safe","retry":"manual","detail":{"kind":"unknown_detail"}}"#,
    ] {
        assert!(serde_json::from_str::<ErrorDto>(wire).is_err());
    }
}
