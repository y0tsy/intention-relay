//! The legacy M4 selection binding bridge.
//!
//! The bridge materializes one deterministic first-party `default` profile
//! binding for every persisted legacy M4 configuration revision. It reads the
//! original persisted snapshot bytes unchanged and never rewrites them, and it
//! never synthesizes provider selections for historical runs.

use intention_config::ConfigSnapshotDto;
use intention_domain::{LegacyM4SelectionBindingDto, legacy_bridge::LEGACY_SELECTION_PREFIX};
use intention_storage::{
    AppendLegacyM4SelectionBindingInputDto, LegacyBindingRepositoryDto,
    LegacyBindingValidationStatusDto, StorageRepositoryDto,
};
use intention_types::{DtoResult, ErrorDto};

/// The deterministic legacy snapshot schema name.
const LEGACY_SNAPSHOT_SCHEMA: &str = "m4-config-snapshot-v1";
/// The deterministic first-party default profile id for legacy bindings.
const LEGACY_DEFAULT_PROFILE_ID: &str = "default";
/// The deterministic legacy driver contract revision.
const LEGACY_DRIVER_CONTRACT_REVISION: &str = "legacy-m4-driver-1.0";
/// The deterministic legacy capability subset.
const LEGACY_CAPABILITY_SUBSET: [&str; 2] = ["text_input", "text_streaming"];

/// The legacy M4 selection binding bridge.
pub struct LegacyM4Bridge;

impl LegacyM4Bridge {
    /// Materializes one deterministic legacy selection binding for every
    /// persisted legacy M4 configuration revision and returns the number of
    /// revisions processed.
    ///
    /// The original persisted snapshot bytes are read unchanged and never
    /// rewritten. Each revision maps deterministically to a first-party
    /// `default` profile binding with a `legacy-uuid:<UUID>` safe selection.
    /// Unsupported or malformed snapshots are recorded as corrupt bindings so
    /// historical replay remains readable. No provider selection is ever
    /// synthesized for historical runs.
    ///
    /// # Errors
    ///
    /// Returns a storage error when the persisted revisions cannot be read or
    /// a binding cannot be appended.
    pub fn materialize<Config, Bindings>(
        &self,
        config: &Config,
        bindings: &Bindings,
        now: u64,
    ) -> DtoResult<u64>
    where
        Config: StorageRepositoryDto,
        Bindings: LegacyBindingRepositoryDto,
    {
        let records = config.load_config_revision_records()?;
        for record in &records {
            let binding = record
                .snapshot
                .as_ref()
                .and_then(|snapshot| legacy_binding(snapshot, &record.snapshot_bytes_digest));
            let validation_status = if binding.is_some() {
                LegacyBindingValidationStatusDto::Validated
            } else {
                LegacyBindingValidationStatusDto::Corrupt
            };
            bindings.append_legacy_m4_selection_binding(
                AppendLegacyM4SelectionBindingInputDto {
                    config_revision_id: record.revision_id.clone(),
                    binding,
                    snapshot_bytes_digest: record.snapshot_bytes_digest.clone(),
                    validation_status,
                    created_at: i64_time(now),
                },
            )?;
        }
        u64::try_from(records.len()).map_err(|_| {
            ErrorDto::unavailable(
                "legacy_bridge_count_overflow",
                "the legacy binding count exceeds the supported range",
            )
        })
    }
}

/// Builds the deterministic legacy binding for one snapshot, or `None` when
/// the snapshot cannot be represented.
fn legacy_binding(
    snapshot: &ConfigSnapshotDto,
    snapshot_bytes_digest: &str,
) -> Option<LegacyM4SelectionBindingDto> {
    let binding = LegacyM4SelectionBindingDto {
        legacy_config_revision_id: snapshot.revision_id().to_string(),
        legacy_snapshot_schema: LEGACY_SNAPSHOT_SCHEMA.to_owned(),
        legacy_safe_selection: legacy_safe_selection(snapshot_bytes_digest),
        default_profile_id: LEGACY_DEFAULT_PROFILE_ID.to_owned(),
        default_profile_revision_id: format!("default-{}", &snapshot_bytes_digest[..8]),
        kind_descriptor_revision_id: format!("legacy-kind-{}", &snapshot_bytes_digest[..16]),
        capability_subset: LEGACY_CAPABILITY_SUBSET
            .iter()
            .map(|capability| (*capability).to_owned())
            .collect(),
        execution_policy: legacy_execution_policy(snapshot),
        driver_contract_revision: LEGACY_DRIVER_CONTRACT_REVISION.to_owned(),
    };
    binding.validate().ok()?;
    Some(binding)
}

/// Derives the deterministic canonical `legacy-uuid:<UUID>` safe selection
/// from the original snapshot bytes digest.
fn legacy_safe_selection(snapshot_bytes_digest: &str) -> String {
    let mut bytes = [0u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&snapshot_bytes_digest[index * 2..index * 2 + 2], 16).unwrap_or(0);
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{}{}{}{}{}-{}{}-{}{}-{}{}-{}{}{}{}{}{}",
        LEGACY_SELECTION_PREFIX,
        hex_byte(bytes[0]),
        hex_byte(bytes[1]),
        hex_byte(bytes[2]),
        hex_byte(bytes[3]),
        hex_byte(bytes[4]),
        hex_byte(bytes[5]),
        hex_byte(bytes[6]),
        hex_byte(bytes[7]),
        hex_byte(bytes[8]),
        hex_byte(bytes[9]),
        hex_byte(bytes[10]),
        hex_byte(bytes[11]),
        hex_byte(bytes[12]),
        hex_byte(bytes[13]),
        hex_byte(bytes[14]),
        hex_byte(bytes[15]),
    )
}

/// Formats one byte as two lowercase hexadecimal characters.
fn hex_byte(byte: u8) -> String {
    format!("{byte:02x}")
}

/// The deterministic legacy execution policy for one snapshot.
fn legacy_execution_policy(snapshot: &ConfigSnapshotDto) -> String {
    let execution = snapshot.resolved().provider_execution();
    format!(
        "execution-timeout-{}-attempts-{}",
        execution.attempt_timeout_seconds(),
        execution.max_attempts(),
    )
}

/// Converts one whole-second Unix time to the storage `i64` representation.
fn i64_time(now: u64) -> i64 {
    i64::try_from(now).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;
    use intention_domain::validate_legacy_safe_selection;

    #[test]
    fn legacy_safe_selection_is_a_canonical_uuid_reference() {
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let selection = legacy_safe_selection(digest);
        assert!(validate_legacy_safe_selection(&selection).is_ok());
        assert_eq!(selection, legacy_safe_selection(digest));
        assert!(selection.starts_with(LEGACY_SELECTION_PREFIX));
        assert_eq!(selection.len(), LEGACY_SELECTION_PREFIX.len() + 36);
    }
}
