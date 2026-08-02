#![allow(
    clippy::expect_used,
    reason = "M3 contract fixtures use expect for precise test diagnostics."
)]

use intention_types::QueuePositionDto;

#[test]
fn queue_positions_construct_and_decode_as_typed_ordering_values() {
    let position = QueuePositionDto::new(0);
    assert_eq!(position.value(), 0);
    assert_eq!(
        serde_json::from_str::<QueuePositionDto>("7").expect("u64 queue position decodes"),
        QueuePositionDto::new(7)
    );
    assert!(serde_json::from_str::<QueuePositionDto>("-1").is_err());
}
