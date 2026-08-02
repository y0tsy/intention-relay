#![allow(
    clippy::expect_used,
    reason = "M3 contract fixtures use expect for precise test diagnostics."
)]

use intention_config::{ConfigSnapshotDto, ResolvedConfigDto};

#[test]
fn snapshots_expose_only_safe_persistence_validation() {
    let snapshot: ConfigSnapshotDto =
        serde_json::from_str(include_str!("fixtures/config-snapshot-v1.json"))
            .expect("safe fixture decodes");
    assert!(snapshot.validate_for_persistence().is_ok());
    let encoded = serde_json::to_string(&snapshot).expect("safe snapshot serializes");
    assert!(!encoded.contains("fixture-credential-not-real-12345"));
    let _resolved: &ResolvedConfigDto = snapshot.resolved();
}
