#![allow(
    clippy::expect_used,
    reason = "M3 contract fixtures use expect for precise test diagnostics."
)]

use intention_types::SessionEventSequenceDto;

#[test]
fn session_sequences_construct_and_decode_as_typed_ordering_values() {
    let sequence = SessionEventSequenceDto::new(0);
    assert_eq!(sequence.value(), 0);
    assert_eq!(
        serde_json::from_str::<SessionEventSequenceDto>("7").expect("u64 sequence decodes"),
        SessionEventSequenceDto::new(7)
    );
    assert!(serde_json::from_str::<SessionEventSequenceDto>("-1").is_err());
    assert!(SessionEventSequenceDto::new(1) > SessionEventSequenceDto::new(0));
}
