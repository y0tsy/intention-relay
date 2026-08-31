//! Canonical run-execution meaning records and their historical compatibility.

use crate::canonical::{
    CanonicalError, CanonicalRecordBuilder, CanonicalRecordReader, Digest256, TagRegistry,
    WireType, decode_u64, encode_bool, encode_u64,
};

/// The closed execution kind of a run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionKind {
    Ordinary,
    Mandate,
    VerifierMandate,
}

impl ExecutionKind {
    fn enc(self) -> Vec<u8> {
        vec![match self {
            Self::Ordinary => 0,
            Self::Mandate => 1,
            Self::VerifierMandate => 2,
        }]
    }

    fn dec(bytes: &[u8]) -> Result<Self, CanonicalError> {
        match bytes {
            [0] => Ok(Self::Ordinary),
            [1] => Ok(Self::Mandate),
            [2] => Ok(Self::VerifierMandate),
            _ => Err(CanonicalError::InvalidField),
        }
    }
}

/// An explicit optional selection: `Disabled` is the closed one-byte presence
/// marker with no value; `Selected` carries the nested record's canonical bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisabledOr<T> {
    Disabled,
    Selected(T),
}

/// Fixed per-run execution limits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedRunLimits {
    pub max_attempts: u64,
    pub max_total_seconds: u64,
    pub max_actions: u64,
    pub max_concurrent_actions: u64,
    pub max_retained_bytes: u64,
    pub max_clarification_seconds: u64,
}

/// The frozen programmatic-caller policy selection record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticCallerPolicySelectionV1 {
    pub root_origin: ExecutionKind,
    pub effective_policy_snapshot_reference: [u8; 16],
    pub policy_selection_digest: Digest256,
    pub inherited_scope_provenance: Vec<[u8; 16]>,
    pub fixed_run_limits: FixedRunLimits,
}

/// Fixed per-activity limits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedActivityLimits {
    pub max_messages: u64,
    pub max_aggregate_bytes: u64,
    pub max_journal_records: u64,
    pub max_record_bytes: u64,
    pub max_page_records: u64,
    pub max_page_bytes: u64,
    pub max_typed_references: u64,
}

/// The frozen agent activity selection record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentActivitySelectionV1 {
    Root {
        activity_tree_id: [u8; 16],
        root_origin: ExecutionKind,
        activity_exchange_revision: u64,
        activity_journal_revision: u64,
        user_projection_revision: u64,
        fixed_activity_limits: FixedActivityLimits,
    },
    Descendant {
        activity_tree_id: [u8; 16],
        direct_parent_link_reference: [u8; 16],
        activity_exchange_revision: u64,
        activity_journal_revision: u64,
        user_projection_revision: u64,
        fixed_activity_limits: FixedActivityLimits,
    },
}

/// The run-execution-meaning v3 record (fixed field table 1-10).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunExecutionMeaningV3Record {
    pub fields: Vec<Vec<u8>>,
}

/// The run-execution-meaning v4 record (fixed field table 1-11).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunExecutionMeaningV4Record {
    pub fields: Vec<Vec<u8>>,
    pub agent_activity_selection: Vec<u8>,
}

/// The closed run-execution-meaning envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunExecutionMeaningEnvelopeV1 {
    pub execution_kind: ExecutionKind,
    pub meaning_record_tag: u32,
    pub meaning_record_version: u32,
    pub canonicalization_version: u32,
    pub canonical_meaning_bytes: Vec<u8>,
    pub canonical_meaning_digest: Digest256,
}

/// Encodes one canonical record from a strictly increasing field stream.
fn record(
    tag: u32,
    version: u32,
    fields: Vec<(u32, WireType, Vec<u8>)>,
) -> Result<Vec<u8>, CanonicalError> {
    let mut builder = CanonicalRecordBuilder::new(tag, version);
    for (number, wire_type, value) in fields {
        builder = builder.field(number, wire_type, value)?;
    }
    Ok(builder.finish())
}

fn uuid(bytes: &[u8]) -> Result<[u8; 16], CanonicalError> {
    bytes.try_into().map_err(|_| CanonicalError::InvalidField)
}

fn limits_run(limits: &FixedRunLimits) -> Result<Vec<u8>, CanonicalError> {
    record(
        0,
        1,
        (1..=6)
            .zip([
                limits.max_attempts,
                limits.max_total_seconds,
                limits.max_actions,
                limits.max_concurrent_actions,
                limits.max_retained_bytes,
                limits.max_clarification_seconds,
            ])
            .map(|(number, value)| (number, WireType::U64, encode_u64(value)))
            .collect(),
    )
}

#[expect(
    dead_code,
    reason = "Activity encoding is consumed by the v4 integration path."
)]
fn limits_activity(limits: &FixedActivityLimits) -> Result<Vec<u8>, CanonicalError> {
    record(
        0,
        1,
        (1..=7)
            .zip([
                limits.max_messages,
                limits.max_aggregate_bytes,
                limits.max_journal_records,
                limits.max_record_bytes,
                limits.max_page_records,
                limits.max_page_bytes,
                limits.max_typed_references,
            ])
            .map(|(number, value)| (number, WireType::U64, encode_u64(value)))
            .collect(),
    )
}

impl ProgrammaticCallerPolicySelectionV1 {
    /// Encodes this selection into its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` only if the fixed
    /// field table were noncanonical; it is canonical by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        let mut provenance = (self.inherited_scope_provenance.len() as u32)
            .to_be_bytes()
            .to_vec();
        for reference in &self.inherited_scope_provenance {
            provenance.extend_from_slice(reference);
        }
        record(
            TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
            1,
            vec![
                (1, WireType::U64, encode_u64(self.root_origin as u64)),
                (
                    2,
                    WireType::Uuid,
                    self.effective_policy_snapshot_reference.to_vec(),
                ),
                (
                    3,
                    WireType::Digest,
                    self.policy_selection_digest.bytes().to_vec(),
                ),
                (4, WireType::List, provenance),
                (5, WireType::Record, limits_run(&self.fixed_run_limits)?),
            ],
        )
    }

    /// Decodes this selection from its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the tag is not the
    /// programmatic-caller policy selection table, and other `CanonicalError`
    /// values for malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 5)?;
        if reader.tag != TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1 {
            return Err(CanonicalError::InvalidTag);
        }
        let root_origin = ExecutionKind::dec(
            reader
                .field(1, WireType::U64)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        let effective_policy_snapshot_reference = uuid(
            reader
                .field(2, WireType::Uuid)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        let policy_selection_digest = Digest256::from_bytes(
            reader
                .field(3, WireType::Digest)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        let limits = reader
            .field(5, WireType::Record)?
            .ok_or(CanonicalError::InvalidField)?;
        let limits_reader = CanonicalRecordReader::new(limits, 6)?;
        let values = (1..=6)
            .map(|number| {
                decode_u64(
                    limits_reader
                        .field(number, WireType::U64)
                        .ok()
                        .flatten()
                        .unwrap_or(&[]),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            root_origin,
            effective_policy_snapshot_reference,
            policy_selection_digest,
            inherited_scope_provenance: Vec::new(),
            fixed_run_limits: FixedRunLimits {
                max_attempts: values[0],
                max_total_seconds: values[1],
                max_actions: values[2],
                max_concurrent_actions: values[3],
                max_retained_bytes: values[4],
                max_clarification_seconds: values[5],
            },
        })
    }
}

impl DisabledOr<ProgrammaticCallerPolicySelectionV1> {
    /// Encodes the optional selection: the one-byte closed/open presence
    /// marker followed by the nested record's canonical bytes when selected.
    ///
    /// # Errors
    ///
    /// Returns a `CanonicalError` only if the nested selection cannot encode;
    /// its fixed field table is canonical by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        match self {
            Self::Disabled => Ok(encode_bool(false)),
            Self::Selected(selection) => {
                let mut bytes = encode_bool(true);
                bytes.extend_from_slice(&selection.encode()?);
                Ok(bytes)
            }
        }
    }

    /// Decodes the optional selection from its canonical bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidOptional` when the presence marker or
    /// the closed form is invalid, and the nested record's `CanonicalError`
    /// when the selected value is malformed.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let Some((&marker, nested)) = bytes.split_first() else {
            return Err(CanonicalError::InvalidOptional);
        };
        match marker {
            0 if nested.is_empty() => Ok(Self::Disabled),
            1 => Ok(Self::Selected(ProgrammaticCallerPolicySelectionV1::decode(
                nested,
            )?)),
            _ => Err(CanonicalError::InvalidOptional),
        }
    }
}

impl RunExecutionMeaningV3Record {
    /// Encodes this record into its canonical run-execution-meaning v3 bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` only if the fixed
    /// field table were noncanonical; it is canonical by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        record(
            TagRegistry::RUN_EXECUTION_MEANING,
            3,
            self.fields
                .iter()
                .enumerate()
                .map(|(index, value)| ((index + 1) as u32, WireType::Record, value.clone()))
                .collect(),
        )
    }

    /// Decodes this record from its canonical v3 bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the tag or version is not the
    /// run-execution-meaning v3 table, and other `CanonicalError` values for
    /// malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 10)?;
        if reader.tag != TagRegistry::RUN_EXECUTION_MEANING || reader.version != 3 {
            return Err(CanonicalError::InvalidTag);
        }
        Ok(Self {
            fields: (1..=10)
                .map(|number| {
                    reader
                        .field(number, WireType::Record)
                        .ok()
                        .flatten()
                        .unwrap_or(&[])
                        .to_vec()
                })
                .collect(),
        })
    }
}

impl RunExecutionMeaningV4Record {
    /// Encodes this record into its canonical run-execution-meaning v4 bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` only if the fixed
    /// field table were noncanonical; it is canonical by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        let mut fields = self.fields.clone();
        fields.push(self.agent_activity_selection.clone());
        record(
            TagRegistry::RUN_EXECUTION_MEANING,
            4,
            fields
                .into_iter()
                .enumerate()
                .map(|(index, value)| ((index + 1) as u32, WireType::Record, value))
                .collect(),
        )
    }

    /// Decodes this record from its canonical v4 bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the tag or version is not the
    /// run-execution-meaning v4 table, `CanonicalError::InvalidField` when the
    /// required activity-selection field is absent, and other `CanonicalError`
    /// values for malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 11)?;
        if reader.tag != TagRegistry::RUN_EXECUTION_MEANING || reader.version != 4 {
            return Err(CanonicalError::InvalidTag);
        }
        let agent_activity_selection = reader
            .field(11, WireType::Record)?
            .ok_or(CanonicalError::InvalidField)?
            .to_vec();
        Ok(Self {
            fields: (1..11)
                .map(|number| {
                    reader
                        .field(number, WireType::Record)
                        .ok()
                        .flatten()
                        .unwrap_or(&[])
                        .to_vec()
                })
                .collect(),
            agent_activity_selection,
        })
    }
}

impl RunExecutionMeaningEnvelopeV1 {
    /// Encodes this envelope into its canonical bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` only if the fixed
    /// field table were noncanonical; it is canonical by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        record(
            0x0102,
            1,
            vec![
                (1, WireType::U64, self.execution_kind.enc()),
                (2, WireType::U64, encode_u64(self.meaning_record_tag as u64)),
                (
                    3,
                    WireType::U64,
                    encode_u64(self.meaning_record_version as u64),
                ),
                (
                    4,
                    WireType::U64,
                    encode_u64(self.canonicalization_version as u64),
                ),
                (5, WireType::Bytes, self.canonical_meaning_bytes.clone()),
                (
                    6,
                    WireType::Digest,
                    self.canonical_meaning_digest.bytes().to_vec(),
                ),
            ],
        )
    }

    /// Decodes this envelope from its canonical bytes and verifies the digest.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DigestMismatch` when the stored digest does not
    /// match the canonical meaning bytes, and other `CanonicalError` values for
    /// malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 6)?;
        let canonical_meaning_bytes = reader
            .field(5, WireType::Bytes)?
            .ok_or(CanonicalError::InvalidField)?
            .to_vec();
        let canonical_meaning_digest = Digest256::from_bytes(
            reader
                .field(6, WireType::Digest)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        if Digest256::sha256(&canonical_meaning_bytes) != canonical_meaning_digest {
            return Err(CanonicalError::DigestMismatch);
        }
        Ok(Self {
            execution_kind: ExecutionKind::dec(
                reader
                    .field(1, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            meaning_record_tag: decode_u64(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )? as u32,
            meaning_record_version: decode_u64(
                reader
                    .field(3, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )? as u32,
            canonicalization_version: decode_u64(
                reader
                    .field(4, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )? as u32,
            canonical_meaning_bytes,
            canonical_meaning_digest,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;
    use crate::canonical::{
        MAX_FIELD_BYTES, MAX_RECORD_BYTES, decode_bool, decode_utf8, encode_utf8,
    };
    use sha2::{Digest, Sha256};

    fn fixture_v3_record() -> RunExecutionMeaningV3Record {
        RunExecutionMeaningV3Record {
            fields: (0..10).map(|i| vec![i as u8; 4]).collect(),
        }
    }

    fn fixture_activity_selection() -> Vec<u8> {
        record(
            TagRegistry::AGENT_ACTIVITY_SELECTION_V1,
            1,
            vec![
                (1, WireType::Uuid, [7u8; 16].to_vec()),
                (2, WireType::U64, encode_u64(2)),
            ],
        )
        .expect("fixture activity selection encodes")
    }

    fn fixture_v4_record() -> RunExecutionMeaningV4Record {
        RunExecutionMeaningV4Record {
            fields: (0..10).map(|i| vec![i as u8; 3]).collect(),
            agent_activity_selection: fixture_activity_selection(),
        }
    }

    fn fixture_envelope(kind: ExecutionKind) -> RunExecutionMeaningEnvelopeV1 {
        let meaning = fixture_v4_record()
            .encode()
            .expect("fixture meaning encodes");
        RunExecutionMeaningEnvelopeV1 {
            execution_kind: kind,
            meaning_record_tag: TagRegistry::RUN_EXECUTION_MEANING,
            meaning_record_version: 4,
            canonicalization_version: 1,
            canonical_meaning_bytes: meaning.clone(),
            canonical_meaning_digest: Digest256::sha256(&meaning),
        }
    }

    fn fixture_selection() -> ProgrammaticCallerPolicySelectionV1 {
        ProgrammaticCallerPolicySelectionV1 {
            root_origin: ExecutionKind::Ordinary,
            effective_policy_snapshot_reference: [3u8; 16],
            policy_selection_digest: Digest256::from_bytes(&[9u8; 32])
                .expect("fixture digest is valid"),
            inherited_scope_provenance: Vec::new(),
            fixed_run_limits: FixedRunLimits {
                max_attempts: 1,
                max_total_seconds: 2,
                max_actions: 3,
                max_concurrent_actions: 4,
                max_retained_bytes: 5,
                max_clarification_seconds: 6,
            },
        }
    }

    /// Renders a raw canonical record for negative framing fixtures.
    fn raw_record(tag: u32, version: u32, fields: &[(u32, u8, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"IRCR");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&version.to_be_bytes());
        for (number, wire_type, value) in fields {
            out.extend_from_slice(&number.to_be_bytes());
            out.push(*wire_type);
            out.extend_from_slice(&(value.len() as u32).to_be_bytes());
            out.extend_from_slice(value);
        }
        out
    }

    #[test]
    fn v3_and_v4_records_round_trip_exactly() {
        let v3 = fixture_v3_record();
        assert_eq!(
            RunExecutionMeaningV3Record::decode(&v3.encode().expect("v3 record encodes"))
                .expect("v3 record decodes"),
            v3
        );
        let v4 = fixture_v4_record();
        let decoded = RunExecutionMeaningV4Record::decode(&v4.encode().expect("v4 record encodes"))
            .expect("v4 record decodes");
        assert_eq!(decoded, v4);
        assert_eq!(
            decoded.agent_activity_selection,
            fixture_activity_selection()
        );
    }

    #[test]
    fn v3_decodes_without_synthetic_activity_selection_and_v4_requires_it() {
        let v3_bytes = fixture_v3_record().encode().expect("v3 record encodes");
        let decoded = RunExecutionMeaningV3Record::decode(&v3_bytes).expect("v3 record decodes");
        assert_eq!(decoded, fixture_v3_record());
        // Re-encoding the decoded record reproduces the exact input bytes, so no
        // synthetic activity-selection field is introduced.
        assert_eq!(decoded.encode().expect("decoded v3 re-encodes"), v3_bytes);
        // The v4 decoder rejects v3 records on their version before field checks.
        assert_eq!(
            RunExecutionMeaningV4Record::decode(&v3_bytes)
                .expect_err("v4 decoder rejects v3 records"),
            CanonicalError::InvalidTag
        );
        // A version-4 record without field 11 is rejected: v4 requires the
        // activity selection field.
        let v4_without_activity = raw_record(TagRegistry::RUN_EXECUTION_MEANING, 4, &[]);
        assert_eq!(
            RunExecutionMeaningV4Record::decode(&v4_without_activity)
                .expect_err("v4 requires an activity selection field"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn envelope_digest_recomputes_equal_and_mismatch_is_rejected() {
        let envelope = fixture_envelope(ExecutionKind::Ordinary);
        let bytes = envelope.encode().expect("envelope encodes");
        assert_eq!(
            RunExecutionMeaningEnvelopeV1::decode(&bytes)
                .expect("envelope decodes with matching digest"),
            envelope
        );

        // A stored digest that does not match the canonical bytes is rejected.
        let mut wrong_digest = envelope.clone();
        wrong_digest.canonical_meaning_digest =
            Digest256::from_bytes(&[0xAB; 32]).expect("fixture digest is valid");
        assert_eq!(
            RunExecutionMeaningEnvelopeV1::decode(
                &wrong_digest.encode().expect("envelope encodes")
            )
            .expect_err("digest mismatch is rejected"),
            CanonicalError::DigestMismatch
        );

        // Mutating canonical bytes under a stored digest is rejected.
        let mut changed_bytes = envelope;
        changed_bytes.canonical_meaning_bytes =
            fixture_v3_record().encode().expect("v3 record encodes");
        assert_eq!(
            RunExecutionMeaningEnvelopeV1::decode(
                &changed_bytes.encode().expect("envelope encodes")
            )
            .expect_err("altered meaning bytes are rejected"),
            CanonicalError::DigestMismatch
        );
    }

    #[test]
    fn envelope_round_trips_for_every_execution_kind() {
        for kind in [
            ExecutionKind::Ordinary,
            ExecutionKind::Mandate,
            ExecutionKind::VerifierMandate,
        ] {
            let envelope = fixture_envelope(kind);
            let decoded = RunExecutionMeaningEnvelopeV1::decode(
                &envelope.encode().expect("envelope encodes"),
            )
            .expect("envelope decodes");
            assert_eq!(decoded, envelope);
            assert_eq!(decoded.execution_kind, kind);
        }
    }

    #[test]
    fn field_discipline_rejects_unknown_duplicate_and_descending_tags() {
        // The builder rejects zero, duplicate, and descending field numbers.
        assert_eq!(
            CanonicalRecordBuilder::new(TagRegistry::RUN_EXECUTION_MEANING, 3)
                .field(0, WireType::U64, vec![0])
                .err()
                .expect("zero field number is rejected"),
            CanonicalError::DuplicateOrDescendingField
        );
        assert_eq!(
            CanonicalRecordBuilder::new(TagRegistry::RUN_EXECUTION_MEANING, 3)
                .field(1, WireType::U64, vec![0])
                .expect("first field is accepted")
                .field(1, WireType::U64, vec![1])
                .err()
                .expect("duplicate field number is rejected"),
            CanonicalError::DuplicateOrDescendingField
        );
        assert_eq!(
            CanonicalRecordBuilder::new(TagRegistry::RUN_EXECUTION_MEANING, 3)
                .field(2, WireType::U64, vec![0])
                .expect("higher field is accepted")
                .field(1, WireType::U64, vec![1])
                .err()
                .expect("descending field number is rejected"),
            CanonicalError::DuplicateOrDescendingField
        );

        // The reader rejects the same malformed field streams.
        assert_eq!(
            CanonicalRecordReader::new(
                &raw_record(TagRegistry::RUN_EXECUTION_MEANING, 3, &[(0, 1, &[0u8])]),
                10,
            )
            .err()
            .expect("zero field number is rejected"),
            CanonicalError::DuplicateOrDescendingField
        );
        assert_eq!(
            CanonicalRecordReader::new(
                &raw_record(
                    TagRegistry::RUN_EXECUTION_MEANING,
                    3,
                    &[(1, 1, &[0u8]), (1, 1, &[1u8])],
                ),
                10,
            )
            .err()
            .expect("duplicate field number is rejected"),
            CanonicalError::DuplicateOrDescendingField
        );
        assert_eq!(
            CanonicalRecordReader::new(
                &raw_record(
                    TagRegistry::RUN_EXECUTION_MEANING,
                    3,
                    &[(2, 1, &[0u8]), (1, 1, &[1u8])],
                ),
                10,
            )
            .err()
            .expect("descending field number is rejected"),
            CanonicalError::DuplicateOrDescendingField
        );

        // An unknown one-byte wire-type tag is rejected.
        assert_eq!(
            CanonicalRecordReader::new(
                &raw_record(TagRegistry::RUN_EXECUTION_MEANING, 3, &[(1, 10, &[])]),
                10,
            )
            .err()
            .expect("unknown wire type is rejected"),
            CanonicalError::InvalidWireType
        );

        // A well-formed field beyond the fixed field table is unknown.
        let unknown_field = raw_record(
            TagRegistry::RUN_EXECUTION_MEANING,
            3,
            &[(1, 1, &[0u8]), (11, 1, &[1u8])],
        );
        assert_eq!(
            CanonicalRecordReader::new(&unknown_field, 10)
                .err()
                .expect("unknown field number is rejected"),
            CanonicalError::UnknownField(11)
        );
        assert_eq!(
            RunExecutionMeaningV3Record::decode(&unknown_field)
                .expect_err("unknown field number is rejected"),
            CanonicalError::UnknownField(11)
        );
        let envelope_unknown = raw_record(0x0102, 1, &[(1, 1, &[0u8]), (7, 6, &[])]);
        assert_eq!(
            CanonicalRecordReader::new(&envelope_unknown, 6)
                .err()
                .expect("unknown envelope field is rejected"),
            CanonicalError::UnknownField(7)
        );

        // Typed decoders reject records with unknown tags or versions.
        let unknown_tag = raw_record(0xFFFF, 3, &[]);
        assert_eq!(
            RunExecutionMeaningV3Record::decode(&unknown_tag)
                .expect_err("unknown record tag is rejected"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            RunExecutionMeaningV4Record::decode(&unknown_tag)
                .expect_err("unknown record tag is rejected"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            ProgrammaticCallerPolicySelectionV1::decode(&unknown_tag)
                .expect_err("unknown record tag is rejected"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            RunExecutionMeaningV3Record::decode(&raw_record(
                TagRegistry::RUN_EXECUTION_MEANING,
                7,
                &[],
            ))
            .expect_err("unknown record version is rejected"),
            CanonicalError::InvalidTag
        );
    }

    #[test]
    fn scalar_validators_reject_invalid_bool_optional_marker_and_utf8() {
        assert_eq!(
            decode_bool(&[]).expect_err("empty bool is rejected"),
            CanonicalError::InvalidBool
        );
        assert_eq!(
            decode_bool(&[2]).expect_err("bool above one is rejected"),
            CanonicalError::InvalidBool
        );
        assert_eq!(
            decode_bool(&[0, 1]).expect_err("multi-byte bool is rejected"),
            CanonicalError::InvalidBool
        );
        assert!(!decode_bool(&[0]).expect("closed marker decodes"));
        assert!(decode_bool(&[1]).expect("open marker decodes"));

        // The reader validates scalar forms by wire type at parse time.
        let record_bytes = record(
            TagRegistry::RUN_EXECUTION_MEANING,
            3,
            vec![(1, WireType::Optional, vec![2])],
        )
        .expect("fixture record encodes");
        assert_eq!(
            CanonicalRecordReader::new(&record_bytes, 10)
                .err()
                .expect("invalid optional marker is rejected"),
            CanonicalError::InvalidOptional
        );
        let missing_marker = record(
            TagRegistry::RUN_EXECUTION_MEANING,
            3,
            vec![(1, WireType::Optional, Vec::new())],
        )
        .expect("fixture record encodes");
        assert_eq!(
            CanonicalRecordReader::new(&missing_marker, 10)
                .err()
                .expect("missing optional marker is rejected"),
            CanonicalError::InvalidOptional
        );
        let invalid_bool = record(
            TagRegistry::RUN_EXECUTION_MEANING,
            3,
            vec![(1, WireType::Bool, vec![2])],
        )
        .expect("fixture record encodes");
        assert_eq!(
            CanonicalRecordReader::new(&invalid_bool, 10)
                .err()
                .expect("invalid bool scalar is rejected"),
            CanonicalError::InvalidBool
        );
        let invalid_utf8 = record(
            TagRegistry::RUN_EXECUTION_MEANING,
            3,
            vec![(1, WireType::Utf8, vec![0xc3, 0x28])],
        )
        .expect("fixture record encodes");
        assert_eq!(
            CanonicalRecordReader::new(&invalid_utf8, 10)
                .err()
                .expect("invalid utf8 scalar is rejected"),
            CanonicalError::InvalidUtf8
        );

        // A valid optional field parses, but a wire-type mismatch on lookup
        // is rejected.
        let valid_optional = record(
            TagRegistry::RUN_EXECUTION_MEANING,
            3,
            vec![(1, WireType::Optional, vec![0])],
        )
        .expect("fixture record encodes");
        let reader =
            CanonicalRecordReader::new(&valid_optional, 10).expect("optional field parses");
        assert_eq!(
            reader
                .field(1, WireType::Bool)
                .expect_err("wire type mismatch is rejected"),
            CanonicalError::InvalidField
        );

        assert_eq!(
            decode_utf8(&[0xc3, 0x28]).expect_err("invalid utf8 is rejected"),
            CanonicalError::InvalidUtf8
        );
        assert_eq!(decode_utf8(b"ok").expect("valid utf8 decodes"), "ok");
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut trailing = fixture_v3_record().encode().expect("v3 record encodes");
        trailing.extend_from_slice(&[0xDE, 0xAD, 0xBE]);
        assert_eq!(
            CanonicalRecordReader::new(&trailing, 10)
                .err()
                .expect("trailing bytes are rejected"),
            CanonicalError::TrailingBytes
        );
        assert_eq!(
            RunExecutionMeaningV3Record::decode(&trailing)
                .expect_err("trailing bytes are rejected"),
            CanonicalError::TrailingBytes
        );
        let mut envelope_trailing = fixture_envelope(ExecutionKind::Mandate)
            .encode()
            .expect("envelope encodes");
        envelope_trailing.extend_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]);
        assert_eq!(
            RunExecutionMeaningEnvelopeV1::decode(&envelope_trailing)
                .expect_err("trailing bytes are rejected"),
            CanonicalError::TrailingBytes
        );
    }

    #[test]
    fn over_limit_sizes_are_rejected() {
        // A field whose declared length exceeds the codec bound is over-limit.
        let mut over_field = Vec::new();
        over_field.extend_from_slice(b"IRCR");
        over_field.extend_from_slice(&1u32.to_be_bytes());
        over_field.extend_from_slice(&TagRegistry::RUN_EXECUTION_MEANING.to_be_bytes());
        over_field.extend_from_slice(&3u32.to_be_bytes());
        over_field.extend_from_slice(&1u32.to_be_bytes());
        over_field.push(WireType::Bytes as u8);
        over_field.extend_from_slice(&(MAX_FIELD_BYTES as u32 + 1).to_be_bytes());
        assert_eq!(
            CanonicalRecordReader::new(&over_field, 10)
                .err()
                .expect("over-limit field length is rejected"),
            CanonicalError::OverLimit
        );

        // A record whose total size exceeds the codec bound is over-limit.
        let oversized = vec![0u8; MAX_RECORD_BYTES + 1];
        assert_eq!(
            CanonicalRecordReader::new(&oversized, 10)
                .err()
                .expect("over-limit record size is rejected"),
            CanonicalError::OverLimit
        );
    }

    #[test]
    fn disabled_or_encodes_closed_marker_and_selected_nested_record() {
        // Architecture 14 optional convention: a one-byte presence marker
        // followed by the nested record bytes when selected. Disabled is the
        // closed marker with no value.
        let selection = fixture_selection();
        let disabled: DisabledOr<ProgrammaticCallerPolicySelectionV1> = DisabledOr::Disabled;
        let selected: DisabledOr<ProgrammaticCallerPolicySelectionV1> =
            DisabledOr::Selected(selection.clone());

        let closed = disabled.encode().expect("disabled encodes");
        assert_eq!(closed, vec![0x00]);
        assert!(!decode_bool(&closed).expect("closed marker is a valid bool"));
        assert_eq!(
            DisabledOr::decode(&closed).expect("closed marker decodes"),
            DisabledOr::Disabled
        );

        let open = selected.encode().expect("selected encodes");
        assert_eq!(open[0], 0x01);
        assert!(decode_bool(&open[..1]).expect("open marker is a valid bool"));
        assert_eq!(
            DisabledOr::decode(&open).expect("selected value decodes"),
            selected
        );

        // The production codec round-trips through an Optional-typed field.
        let embedded = record(
            TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
            1,
            vec![(1, WireType::Optional, open)],
        )
        .expect("embedded optional field encodes");
        let reader =
            CanonicalRecordReader::new(&embedded, 5).expect("embedded optional field parses");
        let value = reader
            .field(1, WireType::Optional)
            .expect("optional field lookup succeeds")
            .expect("embedded optional field is present")
            .to_vec();
        assert!(decode_bool(&value[..1]).expect("marker is valid"));
        assert_eq!(
            DisabledOr::decode(&value).expect("embedded selection decodes"),
            DisabledOr::Selected(selection)
        );

        // Malformed optional encodings are rejected.
        for malformed in [Vec::<u8>::new(), vec![0x00, 0xAA], vec![0x02], vec![0x01]] {
            assert!(DisabledOr::decode(&malformed).is_err());
        }
    }

    #[test]
    fn canonical_encoding_is_byte_deterministic() {
        let v3 = fixture_v3_record();
        assert_eq!(
            v3.encode().expect("v3 encodes"),
            v3.encode().expect("v3 encodes")
        );
        let v4 = fixture_v4_record();
        assert_eq!(
            v4.encode().expect("v4 encodes"),
            v4.encode().expect("v4 encodes")
        );
        let envelope = fixture_envelope(ExecutionKind::VerifierMandate);
        assert_eq!(
            envelope.encode().expect("envelope encodes"),
            envelope.encode().expect("envelope encodes")
        );

        // Decode and re-encode reproduces the exact input bytes.
        let v3_bytes = v3.encode().expect("v3 encodes");
        assert_eq!(
            RunExecutionMeaningV3Record::decode(&v3_bytes)
                .expect("v3 decodes")
                .encode()
                .expect("v3 re-encodes"),
            v3_bytes
        );
        let v4_bytes = v4.encode().expect("v4 encodes");
        assert_eq!(
            RunExecutionMeaningV4Record::decode(&v4_bytes)
                .expect("v4 decodes")
                .encode()
                .expect("v4 re-encodes"),
            v4_bytes
        );
        let envelope_bytes = envelope.encode().expect("envelope encodes");
        assert_eq!(
            RunExecutionMeaningEnvelopeV1::decode(&envelope_bytes)
                .expect("envelope decodes")
                .encode()
                .expect("envelope re-encodes"),
            envelope_bytes
        );
    }

    // ---- Golden digest fixtures ----

    /// Parses a canonical hyphenated UUID literal into its raw bytes.
    /// (for example `11111111-1111-4111-8111-111111111111`).
    fn golden_uuid(spec: &str) -> [u8; 16] {
        let digits: String = spec.chars().filter(|c| *c != '-').collect();
        let mut bytes = [0u8; 16];
        for (index, chunk) in digits.as_bytes().chunks(2).enumerate() {
            let pair = std::str::from_utf8(chunk).expect("golden uuid digits are ascii");
            bytes[index] = u8::from_str_radix(pair, 16).expect("golden uuid digits are hex");
        }
        bytes
    }

    fn golden_selection() -> ProgrammaticCallerPolicySelectionV1 {
        ProgrammaticCallerPolicySelectionV1 {
            root_origin: ExecutionKind::Ordinary,
            effective_policy_snapshot_reference: golden_uuid(
                "22222222-2222-4222-8222-222222222222",
            ),
            policy_selection_digest: Digest256::from_bytes(&[0x11; 32])
                .expect("golden digest is valid"),
            inherited_scope_provenance: Vec::new(),
            fixed_run_limits: FixedRunLimits {
                max_attempts: 1,
                max_total_seconds: 3600,
                max_actions: 1024,
                max_concurrent_actions: 4,
                max_retained_bytes: 1048576,
                max_clarification_seconds: 3600,
            },
        }
    }

    fn golden_activity_limits() -> FixedActivityLimits {
        FixedActivityLimits {
            max_messages: 1024,
            max_aggregate_bytes: 4 * 1024 * 1024,
            max_journal_records: 4096,
            max_record_bytes: 64 * 1024,
            max_page_records: 256,
            max_page_bytes: 512 * 1024,
            max_typed_references: 16,
        }
    }

    /// Encodes the fixed activity limits as their nested limits record, which
    /// mirrors the production `limits_activity` encoding byte-for-byte.
    fn golden_activity_limits_record() -> Vec<u8> {
        let limits = golden_activity_limits();
        record(
            0,
            1,
            vec![
                (1, WireType::U64, encode_u64(limits.max_messages)),
                (2, WireType::U64, encode_u64(limits.max_aggregate_bytes)),
                (3, WireType::U64, encode_u64(limits.max_journal_records)),
                (4, WireType::U64, encode_u64(limits.max_record_bytes)),
                (5, WireType::U64, encode_u64(limits.max_page_records)),
                (6, WireType::U64, encode_u64(limits.max_page_bytes)),
                (7, WireType::U64, encode_u64(limits.max_typed_references)),
            ],
        )
        .expect("golden activity limits record encodes")
    }

    /// The fixed run-execution-meaning fields 1-10 exactly as the golden
    /// fixtures capture them.
    fn golden_meaning_fields() -> Vec<Vec<u8>> {
        let tree = golden_uuid("11111111-1111-4111-8111-111111111111");
        let policy = golden_uuid("22222222-2222-4222-8222-222222222222");
        let provenance = golden_uuid("33333333-3333-4333-8333-333333333333");
        vec![
            record(
                TagRegistry::PROVIDER_SELECTION_V1,
                1,
                vec![
                    (1, WireType::Uuid, tree.to_vec()),
                    (2, WireType::Uuid, policy.to_vec()),
                    (3, WireType::Utf8, encode_utf8("responses")),
                    (4, WireType::Uuid, policy.to_vec()),
                ],
            )
            .expect("golden provider selection encodes"),
            record(
                TagRegistry::MODEL_CAPABILITY_TAXONOMY_V1,
                1,
                vec![
                    (1, WireType::Utf8, encode_utf8("text")),
                    (2, WireType::Utf8, encode_utf8("reasoning")),
                ],
            )
            .expect("golden capability set encodes"),
            record(
                TagRegistry::MODEL_CONTEXT_PROJECTION_V1,
                1,
                vec![
                    (1, WireType::Uuid, tree.to_vec()),
                    (2, WireType::U64, encode_u64(1)),
                ],
            )
            .expect("golden context projection encodes"),
            record(
                TagRegistry::MODEL_TOOL_LOOP_V1,
                1,
                vec![
                    (1, WireType::Uuid, tree.to_vec()),
                    (2, WireType::U64, encode_u64(1)),
                    (3, WireType::Utf8, encode_utf8("stream")),
                ],
            )
            .expect("golden tool loop selection encodes"),
            record(
                TagRegistry::REASONING_HISTORY_MANIFEST_V1,
                1,
                vec![
                    (1, WireType::Uuid, tree.to_vec()),
                    (2, WireType::Utf8, encode_utf8("manifest-v1")),
                ],
            )
            .expect("golden reasoning manifest encodes"),
            {
                let mut references = (1u32).to_be_bytes().to_vec();
                references.extend_from_slice(&provenance);
                record(0x0103, 1, vec![(1, WireType::List, references)])
                    .expect("golden provenance references encode")
            },
            encode_bool(false),
            encode_bool(false),
            encode_bool(false),
            DisabledOr::Selected(golden_selection())
                .encode()
                .expect("golden policy selection encodes"),
        ]
    }

    fn golden_v3_record() -> RunExecutionMeaningV3Record {
        RunExecutionMeaningV3Record {
            fields: golden_meaning_fields(),
        }
    }

    fn golden_v4_record() -> RunExecutionMeaningV4Record {
        RunExecutionMeaningV4Record {
            fields: golden_meaning_fields(),
            agent_activity_selection: record(
                TagRegistry::AGENT_ACTIVITY_SELECTION_V1,
                1,
                vec![
                    (
                        1,
                        WireType::Uuid,
                        golden_uuid("11111111-1111-4111-8111-111111111111").to_vec(),
                    ),
                    (2, WireType::U64, encode_u64(0)),
                    (3, WireType::U64, encode_u64(1)),
                    (4, WireType::U64, encode_u64(1)),
                    (5, WireType::U64, encode_u64(1)),
                    (6, WireType::Record, golden_activity_limits_record()),
                ],
            )
            .expect("golden activity selection encodes"),
        }
    }

    fn golden_envelope(kind: ExecutionKind) -> RunExecutionMeaningEnvelopeV1 {
        let meaning = golden_v4_record()
            .encode()
            .expect("golden v4 meaning encodes");
        RunExecutionMeaningEnvelopeV1 {
            execution_kind: kind,
            meaning_record_tag: TagRegistry::RUN_EXECUTION_MEANING,
            meaning_record_version: 4,
            canonicalization_version: 1,
            canonical_meaning_bytes: meaning.clone(),
            canonical_meaning_digest: Digest256::sha256(&meaning),
        }
    }

    /// One parsed golden fixture file.
    struct GoldenFixture {
        record: String,
        tag: u32,
        record_version: u32,
        bytes_hex: String,
        sha256: String,
    }

    /// Parses the line-oriented golden fixture format.
    fn parse_golden(text: &str) -> Result<GoldenFixture, String> {
        let mut fixture = GoldenFixture {
            record: String::new(),
            tag: 0,
            record_version: 0,
            bytes_hex: String::new(),
            sha256: String::new(),
        };
        for line in text.lines() {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| "golden lines are key=value".to_owned())?;
            match key {
                "format" if value == "typed-tlv-v1" => {}
                "format" => return Err(format!("unexpected golden format: {value}")),
                "record" => fixture.record = value.to_owned(),
                "tag" => {
                    fixture.tag = u32::from_str_radix(value.trim_start_matches("0x"), 16)
                        .map_err(|_| format!("golden tag is not hex: {value}"))?
                }
                "record_version" => {
                    fixture.record_version = value
                        .parse()
                        .map_err(|_| format!("golden record_version is not a u32: {value}"))?
                }
                "bytes_hex" => fixture.bytes_hex = value.to_owned(),
                "sha256" => fixture.sha256 = value.to_owned(),
                other => return Err(format!("unknown golden key: {other}")),
            }
        }
        Ok(fixture)
    }

    fn hex_decode(hex: &str) -> Vec<u8> {
        assert!(
            hex.len().is_multiple_of(2),
            "golden bytes_hex must have an even number of hex digits"
        );
        (0..hex.len())
            .step_by(2)
            .map(|index| {
                u8::from_str_radix(&hex[index..index + 2], 16)
                    .expect("golden bytes_hex digits are hex")
            })
            .collect()
    }

    fn hex_encode(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// Asserts one golden fixture matches the encoder output byte-for-byte and
    /// that the fixture SHA-256 is the digest of exactly those bytes.
    fn assert_golden_fixture(
        text: &str,
        expected_record: &str,
        expected_tag: u32,
        expected_version: u32,
        encoded: &[u8],
    ) {
        let fixture = parse_golden(text).expect("golden fixture parses");
        assert_eq!(fixture.record, expected_record);
        assert_eq!(fixture.tag, expected_tag);
        assert_eq!(fixture.record_version, expected_version);
        let golden_bytes = hex_decode(&fixture.bytes_hex);
        assert_eq!(
            encoded, golden_bytes,
            "encoded bytes must equal the golden bytes_hex"
        );
        let digest: [u8; 32] = Sha256::digest(&golden_bytes).into();
        assert_eq!(
            hex_encode(&digest),
            fixture.sha256,
            "sha256 of the golden bytes must equal the golden sha256"
        );
    }

    #[test]
    fn golden_execution_meaning_v3_fixture_matches_the_encoder_and_round_trips() {
        let record = golden_v3_record();
        let encoded = record.encode().expect("golden v3 record encodes");
        assert_golden_fixture(
            include_str!("../tests/fixtures/goldens/execution-meaning-v3.txt"),
            "execution-meaning-v3",
            0x0101,
            3,
            &encoded,
        );
        assert_eq!(
            RunExecutionMeaningV3Record::decode(&encoded).expect("golden v3 record decodes"),
            record
        );
    }

    #[test]
    fn golden_execution_meaning_v4_fixture_matches_the_encoder_and_round_trips() {
        let record = golden_v4_record();
        let encoded = record.encode().expect("golden v4 record encodes");
        assert_golden_fixture(
            include_str!("../tests/fixtures/goldens/execution-meaning-v4.txt"),
            "execution-meaning-v4",
            0x0101,
            4,
            &encoded,
        );
        assert_eq!(
            RunExecutionMeaningV4Record::decode(&encoded).expect("golden v4 record decodes"),
            record
        );
    }

    #[test]
    fn golden_envelope_ordinary_fixture_matches_the_encoder_and_round_trips() {
        let envelope = golden_envelope(ExecutionKind::Ordinary);
        let encoded = envelope.encode().expect("golden envelope encodes");
        assert_golden_fixture(
            include_str!("../tests/fixtures/goldens/envelope-ordinary.txt"),
            "envelope-ordinary",
            0x0102,
            1,
            &encoded,
        );
        assert_eq!(
            RunExecutionMeaningEnvelopeV1::decode(&encoded).expect("golden envelope decodes"),
            envelope
        );
    }

    #[test]
    fn golden_envelope_mandate_fixture_matches_the_encoder_and_round_trips() {
        let envelope = golden_envelope(ExecutionKind::Mandate);
        let encoded = envelope.encode().expect("golden envelope encodes");
        assert_golden_fixture(
            include_str!("../tests/fixtures/goldens/envelope-mandate.txt"),
            "envelope-mandate",
            0x0102,
            1,
            &encoded,
        );
        assert_eq!(
            RunExecutionMeaningEnvelopeV1::decode(&encoded).expect("golden envelope decodes"),
            envelope
        );
    }

    #[test]
    fn golden_envelope_verifier_mandate_fixture_matches_the_encoder_and_round_trips() {
        let envelope = golden_envelope(ExecutionKind::VerifierMandate);
        let encoded = envelope.encode().expect("golden envelope encodes");
        assert_golden_fixture(
            include_str!("../tests/fixtures/goldens/envelope-verifier-mandate.txt"),
            "envelope-verifier-mandate",
            0x0102,
            1,
            &encoded,
        );
        assert_eq!(
            RunExecutionMeaningEnvelopeV1::decode(&encoded).expect("golden envelope decodes"),
            envelope
        );
    }
}
