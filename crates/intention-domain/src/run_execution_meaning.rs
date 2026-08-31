//! Canonical run-execution meaning records and their historical compatibility.

use crate::canonical::{
    CanonicalError, CanonicalRecordBuilder, CanonicalRecordReader, Digest256, TagRegistry,
    WireType, decode_u64, decode_uuid_list, encode_bool, encode_u64,
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

/// Maximum messages per activity exchange, per the frozen ledger.
pub const MAX_ACTIVITY_MESSAGES: u64 = 1024;
/// Maximum aggregate activity bytes per root tree, per the frozen ledger.
pub const MAX_ACTIVITY_AGGREGATE_BYTES: u64 = 4 * 1024 * 1024;
/// Maximum journal records per activity tree, per the frozen ledger.
pub const MAX_ACTIVITY_JOURNAL_RECORDS: u64 = 4096;
/// Maximum bytes per activity record, per the frozen ledger.
pub const MAX_ACTIVITY_RECORD_BYTES: u64 = 64 * 1024;
/// Maximum records per activity page, per the frozen ledger.
pub const MAX_ACTIVITY_PAGE_RECORDS: u64 = 256;
/// Maximum bytes per activity page, per the frozen ledger.
pub const MAX_ACTIVITY_PAGE_BYTES: u64 = 512 * 1024;
/// Maximum typed references per activity record, per the frozen ledger.
pub const MAX_ACTIVITY_TYPED_REFERENCES: u64 = 16;
/// Maximum clarification wait seconds, per the frozen ledger.
pub const MAX_ACTIVITY_CLARIFICATION_WAIT_SECONDS: u64 = 60 * 60;

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
    pub max_clarification_wait_seconds: u64,
}

impl FixedActivityLimits {
    /// The frozen ledger values for every activity limit.
    #[must_use]
    pub const fn frozen() -> Self {
        Self {
            max_messages: MAX_ACTIVITY_MESSAGES,
            max_aggregate_bytes: MAX_ACTIVITY_AGGREGATE_BYTES,
            max_journal_records: MAX_ACTIVITY_JOURNAL_RECORDS,
            max_record_bytes: MAX_ACTIVITY_RECORD_BYTES,
            max_page_records: MAX_ACTIVITY_PAGE_RECORDS,
            max_page_bytes: MAX_ACTIVITY_PAGE_BYTES,
            max_typed_references: MAX_ACTIVITY_TYPED_REFERENCES,
            max_clarification_wait_seconds: MAX_ACTIVITY_CLARIFICATION_WAIT_SECONDS,
        }
    }

    /// Validates that every limit equals its frozen ledger value.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::OverLimit` when any limit differs from the
    /// frozen ledger value.
    pub fn validate(&self) -> Result<(), CanonicalError> {
        if *self != Self::frozen() {
            return Err(CanonicalError::OverLimit);
        }
        Ok(())
    }

    /// Encodes these activity limits into their nested limits record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::OverLimit` when any limit differs from the
    /// frozen ledger value or a field or the record exceeds the codec size
    /// bounds, and `CanonicalError::DuplicateOrDescendingField` only if the
    /// fixed field table were noncanonical; it is canonical by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        self.validate()?;
        record(
            0,
            1,
            (1..=8)
                .zip([
                    self.max_messages,
                    self.max_aggregate_bytes,
                    self.max_journal_records,
                    self.max_record_bytes,
                    self.max_page_records,
                    self.max_page_bytes,
                    self.max_typed_references,
                    self.max_clarification_wait_seconds,
                ])
                .map(|(number, value)| (number, WireType::U64, encode_u64(value)))
                .collect(),
        )
    }

    /// Decodes these activity limits from their nested limits record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the nested record is not the
    /// anonymous tag-zero version-one limits frame,
    /// `CanonicalError::InvalidField` when any of the eight limit fields is
    /// absent or malformed, `CanonicalError::OverLimit` when any decoded
    /// limit differs from the frozen ledger value, and other
    /// `CanonicalError` values for malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 8)?;
        // The encoder frames the nested limits as an anonymous record: tag
        // zero, version one.
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let values = (1..=8)
            .map(|number| {
                decode_u64(
                    reader
                        .field(number, WireType::U64)?
                        .ok_or(CanonicalError::InvalidField)?,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let limits = Self {
            max_messages: values[0],
            max_aggregate_bytes: values[1],
            max_journal_records: values[2],
            max_record_bytes: values[3],
            max_page_records: values[4],
            max_page_bytes: values[5],
            max_typed_references: values[6],
            max_clarification_wait_seconds: values[7],
        };
        limits.validate()?;
        Ok(limits)
    }
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
///
/// # Errors
///
/// Returns `CanonicalError::DuplicateOrDescendingField` or
/// `CanonicalError::OverLimit` only if the field stream were noncanonical or
/// a field or the record exceeded the codec size bounds.
fn record(
    tag: u32,
    version: u32,
    fields: Vec<(u32, WireType, Vec<u8>)>,
) -> Result<Vec<u8>, CanonicalError> {
    let mut builder = CanonicalRecordBuilder::new(tag, version);
    for (number, wire_type, value) in fields {
        builder = builder.field(number, wire_type, value)?;
    }
    builder.finish()
}

fn uuid(bytes: &[u8]) -> Result<[u8; 16], CanonicalError> {
    bytes.try_into().map_err(|_| CanonicalError::InvalidField)
}

impl FixedRunLimits {
    /// Encodes these run limits into their nested limits record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` only if the fixed
    /// field table were noncanonical, and `CanonicalError::OverLimit` only if
    /// a field or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        record(
            0,
            1,
            (1..=6)
                .zip([
                    self.max_attempts,
                    self.max_total_seconds,
                    self.max_actions,
                    self.max_concurrent_actions,
                    self.max_retained_bytes,
                    self.max_clarification_seconds,
                ])
                .map(|(number, value)| (number, WireType::U64, encode_u64(value)))
                .collect(),
        )
    }

    /// Decodes these run limits from their nested limits record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the nested record is not the
    /// anonymous tag-zero version-one limits frame,
    /// `CanonicalError::InvalidField` when any of the six limit fields is
    /// absent or malformed, and other `CanonicalError` values for malformed
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 6)?;
        // The encoder frames the nested limits as an anonymous record: tag
        // zero, version one.
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let values = (1..=6)
            .map(|number| {
                decode_u64(
                    reader
                        .field(number, WireType::U64)?
                        .ok_or(CanonicalError::InvalidField)?,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            max_attempts: values[0],
            max_total_seconds: values[1],
            max_actions: values[2],
            max_concurrent_actions: values[3],
            max_retained_bytes: values[4],
            max_clarification_seconds: values[5],
        })
    }
}

impl AgentActivitySelectionV1 {
    /// Encodes this selection into its canonical record bytes.
    ///
    /// The Root variant encodes as record version 1 and the Descendant variant
    /// as record version 2; the version discriminates the variant under the
    /// shared activity-selection tag.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` only if the fixed
    /// field table were noncanonical, and `CanonicalError::OverLimit` only if
    /// a field or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        let (version, field_two) = match self {
            Self::Root { root_origin, .. } => {
                (1, (2, WireType::U64, encode_u64(*root_origin as u64)))
            }
            Self::Descendant {
                direct_parent_link_reference,
                ..
            } => (
                2,
                (2, WireType::Uuid, direct_parent_link_reference.to_vec()),
            ),
        };
        let (
            activity_tree_id,
            activity_exchange_revision,
            activity_journal_revision,
            user_projection_revision,
            fixed_activity_limits,
        ) = match self {
            Self::Root {
                activity_tree_id,
                activity_exchange_revision,
                activity_journal_revision,
                user_projection_revision,
                fixed_activity_limits,
                ..
            }
            | Self::Descendant {
                activity_tree_id,
                activity_exchange_revision,
                activity_journal_revision,
                user_projection_revision,
                fixed_activity_limits,
                ..
            } => (
                activity_tree_id,
                activity_exchange_revision,
                activity_journal_revision,
                user_projection_revision,
                fixed_activity_limits,
            ),
        };
        record(
            TagRegistry::AGENT_ACTIVITY_SELECTION_V1,
            version,
            vec![
                (1, WireType::Uuid, activity_tree_id.to_vec()),
                field_two,
                (3, WireType::U64, encode_u64(*activity_exchange_revision)),
                (4, WireType::U64, encode_u64(*activity_journal_revision)),
                (5, WireType::U64, encode_u64(*user_projection_revision)),
                (6, WireType::Record, fixed_activity_limits.encode()?),
            ],
        )
    }

    /// Decodes this selection from its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the tag is not the activity
    /// selection table or the version is not a Root (1) or Descendant (2)
    /// variant, `CanonicalError::InvalidField` when any of the six fields is
    /// absent or carries the wrong wire type, and other `CanonicalError`
    /// values for malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 6)?;
        if reader.tag != TagRegistry::AGENT_ACTIVITY_SELECTION_V1
            || !matches!(reader.version, 1 | 2)
        {
            return Err(CanonicalError::InvalidTag);
        }
        let activity_tree_id = uuid(
            reader
                .field(1, WireType::Uuid)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        let fixed_activity_limits = FixedActivityLimits::decode(
            reader
                .field(6, WireType::Record)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        let activity_exchange_revision = decode_u64(
            reader
                .field(3, WireType::U64)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        let activity_journal_revision = decode_u64(
            reader
                .field(4, WireType::U64)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        let user_projection_revision = decode_u64(
            reader
                .field(5, WireType::U64)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        match reader.version {
            1 => Ok(Self::Root {
                activity_tree_id,
                root_origin: ExecutionKind::dec(
                    reader
                        .field(2, WireType::U64)?
                        .ok_or(CanonicalError::InvalidField)?,
                )?,
                activity_exchange_revision,
                activity_journal_revision,
                user_projection_revision,
                fixed_activity_limits,
            }),
            2 => Ok(Self::Descendant {
                activity_tree_id,
                direct_parent_link_reference: uuid(
                    reader
                        .field(2, WireType::Uuid)?
                        .ok_or(CanonicalError::InvalidField)?,
                )?,
                activity_exchange_revision,
                activity_journal_revision,
                user_projection_revision,
                fixed_activity_limits,
            }),
            _ => Err(CanonicalError::InvalidTag),
        }
    }
}

impl ProgrammaticCallerPolicySelectionV1 {
    /// Encodes this selection into its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` only if the fixed
    /// field table were noncanonical, and `CanonicalError::OverLimit` only if
    /// a field or the record exceeded the codec size bounds; both are
    /// impossible by construction.
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
                (5, WireType::Record, self.fixed_run_limits.encode()?),
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
        let inherited_scope_provenance = decode_uuid_list(
            reader
                .field(4, WireType::List)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        let fixed_run_limits = FixedRunLimits::decode(
            reader
                .field(5, WireType::Record)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        Ok(Self {
            root_origin,
            effective_policy_snapshot_reference,
            policy_selection_digest,
            inherited_scope_provenance,
            fixed_run_limits,
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
    /// field table were noncanonical, and `CanonicalError::OverLimit` only if
    /// a field or the record exceeded the codec size bounds; both are
    /// impossible by construction.
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
    /// run-execution-meaning v3 table, `CanonicalError::InvalidField` when any
    /// of the ten mandatory fields is absent or carries the wrong wire type,
    /// and other `CanonicalError` values for malformed or noncanonical
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 10)?;
        if reader.tag != TagRegistry::RUN_EXECUTION_MEANING || reader.version != 3 {
            return Err(CanonicalError::InvalidTag);
        }
        let fields = (1..=10)
            .map(|number| {
                reader
                    .field(number, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)
                    .map(|value| value.to_vec())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { fields })
    }
}

impl RunExecutionMeaningV4Record {
    /// Encodes this record into its canonical run-execution-meaning v4 bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` only if the fixed
    /// field table were noncanonical, and `CanonicalError::OverLimit` only if
    /// a field or the record exceeded the codec size bounds; both are
    /// impossible by construction.
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
    /// run-execution-meaning v4 table, `CanonicalError::InvalidField` when any
    /// of the eleven mandatory fields is absent or carries the wrong wire
    /// type, and other `CanonicalError` values for malformed or noncanonical
    /// framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 11)?;
        if reader.tag != TagRegistry::RUN_EXECUTION_MEANING || reader.version != 4 {
            return Err(CanonicalError::InvalidTag);
        }
        let agent_activity_selection = reader
            .field(11, WireType::Record)?
            .ok_or(CanonicalError::InvalidField)?
            .to_vec();
        let fields = (1..11)
            .map(|number| {
                reader
                    .field(number, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)
                    .map(|value| value.to_vec())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            fields,
            agent_activity_selection,
        })
    }

    /// Decodes the v4 activity-selection field into its semantic record.
    ///
    /// # Errors
    ///
    /// Returns the `CanonicalError` of [`AgentActivitySelectionV1::decode`]
    /// when the field bytes are not a valid activity selection.
    pub fn decode_agent_activity_selection(
        &self,
    ) -> Result<AgentActivitySelectionV1, CanonicalError> {
        AgentActivitySelectionV1::decode(&self.agent_activity_selection)
    }
}

impl RunExecutionMeaningEnvelopeV1 {
    /// Encodes this envelope into its canonical bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` only if the fixed
    /// field table were noncanonical, and `CanonicalError::OverLimit` only if
    /// a field or the record exceeded the codec size bounds; both are
    /// impossible by construction.
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
    /// Returns `CanonicalError::InvalidTag` when the record is not the
    /// run-execution-meaning envelope frame (tag `0x0102`, version 1),
    /// `CanonicalError::DigestMismatch` when the stored digest does not match
    /// the canonical meaning bytes, and other `CanonicalError` values for
    /// malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 6)?;
        // The envelope is a closed fixed frame: tag 0x0102, version 1, per
        // the ADR 0036 execution-meaning table.
        if reader.tag != 0x0102 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
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
        CanonicalIdentityInput, IdentityV1, MAX_FIELD_BYTES, MAX_RECORD_BYTES, MAX_UUID_LIST_ITEMS,
        NamespacedDigest, TagStatus, decode_bool, decode_utf8, decode_uuid_list, encode_utf8,
    };
    use sha2::{Digest, Sha256};

    fn fixture_v3_record() -> RunExecutionMeaningV3Record {
        RunExecutionMeaningV3Record {
            fields: (0..10).map(|i| vec![i as u8; 4]).collect(),
        }
    }

    fn fixture_activity_selection() -> AgentActivitySelectionV1 {
        AgentActivitySelectionV1::Root {
            activity_tree_id: [7u8; 16],
            root_origin: ExecutionKind::VerifierMandate,
            activity_exchange_revision: 1,
            activity_journal_revision: 1,
            user_projection_revision: 1,
            fixed_activity_limits: golden_activity_limits(),
        }
    }

    fn fixture_v4_record() -> RunExecutionMeaningV4Record {
        RunExecutionMeaningV4Record {
            fields: (0..10).map(|i| vec![i as u8; 3]).collect(),
            agent_activity_selection: fixture_activity_selection()
                .encode()
                .expect("fixture activity selection encodes"),
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
            decoded
                .decode_agent_activity_selection()
                .expect("activity selection decodes"),
            fixture_activity_selection()
        );
    }

    #[test]
    fn activity_selection_round_trips_for_root_and_descendant_variants() {
        let root = AgentActivitySelectionV1::Root {
            activity_tree_id: [7u8; 16],
            root_origin: ExecutionKind::Mandate,
            activity_exchange_revision: 5,
            activity_journal_revision: 6,
            user_projection_revision: 7,
            fixed_activity_limits: golden_activity_limits(),
        };
        let root_bytes = root.encode().expect("root selection encodes");
        assert_eq!(
            AgentActivitySelectionV1::decode(&root_bytes).expect("root selection decodes"),
            root
        );
        let descendant = AgentActivitySelectionV1::Descendant {
            activity_tree_id: [8u8; 16],
            direct_parent_link_reference: [9u8; 16],
            activity_exchange_revision: 1,
            activity_journal_revision: 2,
            user_projection_revision: 3,
            fixed_activity_limits: golden_activity_limits(),
        };
        let descendant_bytes = descendant.encode().expect("descendant selection encodes");
        assert_eq!(
            AgentActivitySelectionV1::decode(&descendant_bytes)
                .expect("descendant selection decodes"),
            descendant
        );
        // The Root variant is record version 1 and the Descendant variant is
        // record version 2 under the shared activity-selection tag.
        assert_eq!(
            CanonicalRecordReader::new(&root_bytes, 6)
                .expect("root selection parses")
                .version,
            1
        );
        assert_eq!(
            CanonicalRecordReader::new(&descendant_bytes, 6)
                .expect("descendant selection parses")
                .version,
            2
        );
    }

    #[test]
    fn activity_selection_fields_carry_the_frozen_tags_and_wire_types() {
        let root = fixture_activity_selection()
            .encode()
            .expect("root selection encodes");
        let reader = CanonicalRecordReader::new(&root, 6).expect("root selection parses");
        for (number, wire_type) in [
            (1, WireType::Uuid),
            (2, WireType::U64),
            (3, WireType::U64),
            (4, WireType::U64),
            (5, WireType::U64),
            (6, WireType::Record),
        ] {
            assert!(
                reader
                    .field(number, wire_type)
                    .expect("field lookup succeeds")
                    .is_some(),
                "field {number} must be {wire_type:?}"
            );
        }
        // A field looked up with the wrong wire type is rejected.
        assert_eq!(
            reader
                .field(1, WireType::U64)
                .expect_err("uuid field is not u64"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            reader
                .field(6, WireType::List)
                .expect_err("limits field is not a list"),
            CanonicalError::InvalidField
        );
        // The Descendant variant carries the parent link as a Uuid at field 2.
        let descendant = AgentActivitySelectionV1::Descendant {
            activity_tree_id: [8u8; 16],
            direct_parent_link_reference: [9u8; 16],
            activity_exchange_revision: 1,
            activity_journal_revision: 1,
            user_projection_revision: 1,
            fixed_activity_limits: golden_activity_limits(),
        }
        .encode()
        .expect("descendant selection encodes");
        let reader = CanonicalRecordReader::new(&descendant, 6).expect("descendant parses");
        assert!(
            reader
                .field(2, WireType::Uuid)
                .expect("field lookup succeeds")
                .is_some()
        );
        assert_eq!(
            reader
                .field(2, WireType::U64)
                .expect_err("descendant field two is a uuid"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn activity_selection_rejects_wrong_variant_field_types() {
        let limits = golden_activity_limits_record();
        // A Root selection must carry its origin as a U64; a Uuid at field 2
        // is rejected.
        let root_with_uuid = raw_record(
            TagRegistry::AGENT_ACTIVITY_SELECTION_V1,
            1,
            &[
                (1, WireType::Uuid as u8, &[7u8; 16]),
                (2, WireType::Uuid as u8, &[8u8; 16]),
                (3, WireType::U64 as u8, &[1]),
                (4, WireType::U64 as u8, &[1]),
                (5, WireType::U64 as u8, &[1]),
                (6, WireType::Record as u8, &limits),
            ],
        );
        assert_eq!(
            AgentActivitySelectionV1::decode(&root_with_uuid)
                .expect_err("root field two must be a U64 origin"),
            CanonicalError::InvalidField
        );
        // A Descendant selection must carry its parent link as a Uuid; a U64
        // at field 2 is rejected.
        let descendant_with_u64 = raw_record(
            TagRegistry::AGENT_ACTIVITY_SELECTION_V1,
            2,
            &[
                (1, WireType::Uuid as u8, &[7u8; 16]),
                (2, WireType::U64 as u8, &[0]),
                (3, WireType::U64 as u8, &[1]),
                (4, WireType::U64 as u8, &[1]),
                (5, WireType::U64 as u8, &[1]),
                (6, WireType::Record as u8, &limits),
            ],
        );
        assert_eq!(
            AgentActivitySelectionV1::decode(&descendant_with_u64)
                .expect_err("descendant field two must be a Uuid"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn activity_selection_requires_all_six_fields() {
        let tree = [7u8; 16];
        let limits = golden_activity_limits_record();
        let fields: Vec<(u32, u8, Vec<u8>)> = vec![
            (1, WireType::Uuid as u8, tree.to_vec()),
            (2, WireType::U64 as u8, vec![1]),
            (3, WireType::U64 as u8, vec![1]),
            (4, WireType::U64 as u8, vec![1]),
            (5, WireType::U64 as u8, vec![1]),
            (6, WireType::Record as u8, limits),
        ];
        for missing in 1..=6 {
            let partial: Vec<(u32, u8, &[u8])> = fields
                .iter()
                .filter(|(number, ..)| *number != missing)
                .map(|(number, wire_type, value)| (*number, *wire_type, value.as_slice()))
                .collect();
            let bytes = raw_record(TagRegistry::AGENT_ACTIVITY_SELECTION_V1, 1, &partial);
            assert_eq!(
                AgentActivitySelectionV1::decode(&bytes).expect_err("missing field is rejected"),
                CanonicalError::InvalidField,
                "missing field {missing}"
            );
        }
    }

    #[test]
    fn activity_selection_rejects_unknown_versions_and_other_tags() {
        let unknown_version = raw_record(TagRegistry::AGENT_ACTIVITY_SELECTION_V1, 3, &[]);
        assert_eq!(
            AgentActivitySelectionV1::decode(&unknown_version)
                .expect_err("unknown version is rejected"),
            CanonicalError::InvalidTag
        );
        let unknown_tag = raw_record(0x0A0A, 1, &[]);
        assert_eq!(
            AgentActivitySelectionV1::decode(&unknown_tag)
                .expect_err("unknown record tag is rejected"),
            CanonicalError::InvalidTag
        );
    }

    #[test]
    fn v4_records_expose_the_semantic_activity_selection() {
        let v4 = fixture_v4_record();
        let bytes = v4.encode().expect("v4 record encodes");
        let decoded = RunExecutionMeaningV4Record::decode(&bytes).expect("v4 record decodes");
        assert_eq!(
            decoded.agent_activity_selection,
            fixture_activity_selection()
                .encode()
                .expect("fixture activity selection encodes")
        );
        assert_eq!(
            decoded
                .decode_agent_activity_selection()
                .expect("semantic activity selection decodes"),
            fixture_activity_selection()
        );
        // Corrupting the tag-11 bytes surfaces the semantic decode error.
        let mut malformed = decoded;
        malformed.agent_activity_selection =
            raw_record(TagRegistry::AGENT_ACTIVITY_SELECTION_V1, 3, &[]);
        assert_eq!(
            malformed
                .decode_agent_activity_selection()
                .expect_err("malformed activity selection is rejected"),
            CanonicalError::InvalidTag
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
    fn meaning_records_require_every_mandatory_field() {
        let fields = golden_meaning_fields();
        let activity = golden_v4_record().agent_activity_selection;
        let full: Vec<(u32, u8, &[u8])> = fields
            .iter()
            .enumerate()
            .map(|(index, value)| ((index + 1) as u32, WireType::Record as u8, value.as_slice()))
            .collect();
        for missing in 0..10 {
            let mut partial = full.clone();
            partial.remove(missing);
            let v3 = raw_record(TagRegistry::RUN_EXECUTION_MEANING, 3, &partial);
            assert_eq!(
                RunExecutionMeaningV3Record::decode(&v3).expect_err("missing v3 field is rejected"),
                CanonicalError::InvalidField,
                "missing v3 field {}",
                missing + 1
            );
            let mut v4 = partial;
            v4.push((11, WireType::Record as u8, &activity));
            let v4_bytes = raw_record(TagRegistry::RUN_EXECUTION_MEANING, 4, &v4);
            assert_eq!(
                RunExecutionMeaningV4Record::decode(&v4_bytes)
                    .expect_err("missing v4 field is rejected"),
                CanonicalError::InvalidField,
                "missing v4 field {}",
                missing + 1
            );
        }
        // Field 11 is mandatory for v4 in addition to fields 1-10.
        let v4_without_activity = raw_record(TagRegistry::RUN_EXECUTION_MEANING, 4, &full);
        assert_eq!(
            RunExecutionMeaningV4Record::decode(&v4_without_activity)
                .expect_err("v4 requires field 11"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn meaning_records_reject_wrong_field_wire_types() {
        // A v3 field 1 encoded as a U64 instead of a Record is rejected.
        let v3_wrong = raw_record(
            TagRegistry::RUN_EXECUTION_MEANING,
            3,
            &[(1, WireType::U64 as u8, &[0])],
        );
        assert_eq!(
            RunExecutionMeaningV3Record::decode(&v3_wrong)
                .expect_err("wrong v3 wire type is rejected"),
            CanonicalError::InvalidField
        );
        // A v4 field 1 encoded as a U64 instead of a Record is rejected even
        // with a valid field 11 present.
        let activity = golden_v4_record().agent_activity_selection;
        let meaning_fields = golden_meaning_fields();
        let mut v4_wrong: Vec<(u32, u8, &[u8])> = meaning_fields
            .iter()
            .enumerate()
            .map(|(index, value)| ((index + 1) as u32, WireType::Record as u8, value.as_slice()))
            .collect();
        v4_wrong[0] = (1, WireType::U64 as u8, &[0]);
        v4_wrong.push((11, WireType::Record as u8, &activity));
        assert_eq!(
            RunExecutionMeaningV4Record::decode(&raw_record(
                TagRegistry::RUN_EXECUTION_MEANING,
                4,
                &v4_wrong,
            ))
            .expect_err("wrong v4 wire type is rejected"),
            CanonicalError::InvalidField
        );
        // A v4 field 11 encoded as a U64 instead of a Record is rejected.
        let mut v4_wrong_activity: Vec<(u32, u8, &[u8])> = meaning_fields
            .iter()
            .enumerate()
            .map(|(index, value)| ((index + 1) as u32, WireType::Record as u8, value.as_slice()))
            .collect();
        v4_wrong_activity.push((11, WireType::U64 as u8, &[0]));
        assert_eq!(
            RunExecutionMeaningV4Record::decode(&raw_record(
                TagRegistry::RUN_EXECUTION_MEANING,
                4,
                &v4_wrong_activity,
            ))
            .expect_err("wrong v4 activity wire type is rejected"),
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
    fn envelope_rejects_wrong_tag_version_and_unknown_execution_kind() {
        // The envelope frame is closed to tag 0x0102, version 1.
        let wrong_tag = raw_record(0x0A0A, 1, &[]);
        assert_eq!(
            RunExecutionMeaningEnvelopeV1::decode(&wrong_tag)
                .expect_err("unknown envelope tag is rejected"),
            CanonicalError::InvalidTag
        );
        let wrong_version = raw_record(0x0102, 2, &[]);
        assert_eq!(
            RunExecutionMeaningEnvelopeV1::decode(&wrong_version)
                .expect_err("unknown envelope version is rejected"),
            CanonicalError::InvalidTag
        );

        // An unknown execution-kind byte is rejected directly and through the
        // envelope decoder.
        assert!(ExecutionKind::dec(&[3]).is_err());
        assert!(ExecutionKind::dec(&[0, 0]).is_err());
        let meaning = golden_v4_record()
            .encode()
            .expect("golden v4 meaning encodes");
        let unknown_kind = raw_record(
            0x0102,
            1,
            &[
                (1, WireType::U64 as u8, &[3]),
                (
                    2,
                    WireType::U64 as u8,
                    &encode_u64(TagRegistry::RUN_EXECUTION_MEANING as u64),
                ),
                (3, WireType::U64 as u8, &encode_u64(4)),
                (4, WireType::U64 as u8, &encode_u64(1)),
                (5, WireType::Bytes as u8, &meaning),
                (
                    6,
                    WireType::Digest as u8,
                    &Digest256::sha256(&meaning).bytes(),
                ),
            ],
        );
        assert_eq!(
            RunExecutionMeaningEnvelopeV1::decode(&unknown_kind)
                .expect_err("unknown execution kind is rejected"),
            CanonicalError::InvalidField
        );
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
    fn canonical_error_codes_are_stable_and_display_never_leaks_values() {
        assert_eq!(CanonicalError::Truncated.code(), "truncated");
        assert_eq!(CanonicalError::InvalidMagic.code(), "invalid_magic");
        assert_eq!(CanonicalError::InvalidVersion.code(), "invalid_version");
        assert_eq!(CanonicalError::InvalidTag.code(), "invalid_tag");
        assert_eq!(CanonicalError::InvalidField.code(), "invalid_field");
        assert_eq!(CanonicalError::InvalidWireType.code(), "invalid_wire_type");
        assert_eq!(CanonicalError::InvalidUtf8.code(), "invalid_utf8");
        assert_eq!(CanonicalError::InvalidBool.code(), "invalid_bool");
        assert_eq!(CanonicalError::InvalidOptional.code(), "invalid_optional");
        assert_eq!(
            CanonicalError::DuplicateOrDescendingField.code(),
            "duplicate_or_descending_field"
        );
        assert_eq!(
            CanonicalError::UnknownField(u32::MAX).code(),
            "unknown_field"
        );
        assert_eq!(CanonicalError::TrailingBytes.code(), "trailing_bytes");
        assert_eq!(CanonicalError::OverLimit.code(), "over_limit");
        assert_eq!(CanonicalError::DigestMismatch.code(), "digest_mismatch");
        assert_eq!(CanonicalError::InvalidDigest.code(), "invalid_digest");

        // Display renders the stable code and never prints the raw field
        // number or any other input-derived value.
        assert_eq!(
            CanonicalError::UnknownField(u32::MAX).to_string(),
            "unknown_field"
        );
        assert_eq!(CanonicalError::OverLimit.to_string(), "over_limit");
        assert_eq!(CanonicalError::InvalidDigest.to_string(), "invalid_digest");
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
    fn builder_enforces_field_and_record_size_limits() {
        // A single field value over the per-field bound is rejected.
        assert_eq!(
            CanonicalRecordBuilder::new(TagRegistry::RUN_EXECUTION_MEANING, 3)
                .field(1, WireType::Bytes, vec![0u8; MAX_FIELD_BYTES + 1])
                .err()
                .expect("over-limit field value is rejected"),
            CanonicalError::OverLimit
        );

        // A boundary-sized single field is accepted, encodes, and parses.
        let boundary_field = CanonicalRecordBuilder::new(TagRegistry::RUN_EXECUTION_MEANING, 3)
            .field(1, WireType::Bytes, vec![0u8; MAX_FIELD_BYTES])
            .expect("boundary-sized field is accepted")
            .finish()
            .expect("boundary-sized field finishes");
        assert_eq!(boundary_field.len(), 16 + 9 + MAX_FIELD_BYTES);
        assert_eq!(
            CanonicalRecordReader::new(&boundary_field, 1)
                .expect("boundary record parses")
                .field(1, WireType::Bytes)
                .expect("field lookup succeeds")
                .expect("field is present")
                .len(),
            MAX_FIELD_BYTES
        );

        // Accumulated fields that would exceed the record bound are rejected
        // even though every individual field fits the per-field bound.
        let builder = (1..=3)
            .try_fold(
                CanonicalRecordBuilder::new(TagRegistry::RUN_EXECUTION_MEANING, 3),
                |builder, number| {
                    builder.field(number, WireType::Bytes, vec![0u8; MAX_FIELD_BYTES])
                },
            )
            .expect("three boundary-sized fields are accepted");
        assert_eq!(
            builder
                .field(4, WireType::Bytes, vec![0u8; MAX_FIELD_BYTES])
                .err()
                .expect("accumulated record size is rejected"),
            CanonicalError::OverLimit
        );

        // A record at exactly the record bound still encodes and parses.
        let remainder = MAX_RECORD_BYTES - 16 - 4 * 9 - 3 * MAX_FIELD_BYTES;
        let builder = (1..=3)
            .try_fold(
                CanonicalRecordBuilder::new(TagRegistry::RUN_EXECUTION_MEANING, 3),
                |builder, number| {
                    builder.field(number, WireType::Bytes, vec![0u8; MAX_FIELD_BYTES])
                },
            )
            .expect("three boundary-sized fields are accepted");
        let builder = builder
            .field(4, WireType::Bytes, vec![0u8; remainder])
            .expect("fourth field fits the record bound exactly");
        let exact = builder.finish().expect("exact-boundary record finishes");
        assert_eq!(exact.len(), MAX_RECORD_BYTES);
        assert!(CanonicalRecordReader::new(&exact, 4).is_ok());
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
    fn programmatic_provenance_round_trips_and_is_strict() {
        let selection = ProgrammaticCallerPolicySelectionV1 {
            root_origin: ExecutionKind::Ordinary,
            effective_policy_snapshot_reference: [3u8; 16],
            policy_selection_digest: Digest256::from_bytes(&[9u8; 32])
                .expect("fixture digest is valid"),
            inherited_scope_provenance: vec![[1u8; 16], [2u8; 16]],
            fixed_run_limits: FixedRunLimits {
                max_attempts: 1,
                max_total_seconds: 2,
                max_actions: 3,
                max_concurrent_actions: 4,
                max_retained_bytes: 5,
                max_clarification_seconds: 6,
            },
        };
        let bytes = selection.encode().expect("selection encodes");
        assert_eq!(
            ProgrammaticCallerPolicySelectionV1::decode(&bytes).expect("selection decodes"),
            selection
        );

        // An empty provenance round-trips byte-identically to the historical
        // encoding: a zero count and no items.
        let empty = golden_selection();
        let empty_bytes = empty.encode().expect("empty selection encodes");
        assert_eq!(
            ProgrammaticCallerPolicySelectionV1::decode(&empty_bytes)
                .expect("empty selection decodes")
                .encode()
                .expect("empty selection re-encodes"),
            empty_bytes
        );

        // A declared count that does not fit the remaining bytes is truncated.
        let mut truncated = (2u32).to_be_bytes().to_vec();
        truncated.extend_from_slice(&[1u8; 16]);
        let truncated_record = raw_record(
            TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
            1,
            &[
                (1, WireType::U64 as u8, &[0]),
                (2, WireType::Uuid as u8, &[3u8; 16]),
                (3, WireType::Digest as u8, &[9u8; 32]),
                (4, WireType::List as u8, &truncated),
            ],
        );
        assert_eq!(
            ProgrammaticCallerPolicySelectionV1::decode(&truncated_record)
                .expect_err("truncated provenance item is rejected"),
            CanonicalError::Truncated
        );

        // Bytes after the declared items are trailing.
        let mut trailing = (1u32).to_be_bytes().to_vec();
        trailing.extend_from_slice(&[1u8; 16]);
        trailing.extend_from_slice(&[2u8; 16]);
        let trailing_record = raw_record(
            TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
            1,
            &[
                (1, WireType::U64 as u8, &[0]),
                (2, WireType::Uuid as u8, &[3u8; 16]),
                (3, WireType::Digest as u8, &[9u8; 32]),
                (4, WireType::List as u8, &trailing),
            ],
        );
        assert_eq!(
            ProgrammaticCallerPolicySelectionV1::decode(&trailing_record)
                .expect_err("trailing provenance bytes are rejected"),
            CanonicalError::TrailingBytes
        );

        // A missing provenance field is rejected.
        let limits = golden_selection()
            .fixed_run_limits
            .encode()
            .expect("fixture run limits encode");
        let missing_provenance = raw_record(
            TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
            1,
            &[
                (1, WireType::U64 as u8, &[0]),
                (2, WireType::Uuid as u8, &[3u8; 16]),
                (3, WireType::Digest as u8, &[9u8; 32]),
                (5, WireType::Record as u8, &limits),
            ],
        );
        assert_eq!(
            ProgrammaticCallerPolicySelectionV1::decode(&missing_provenance)
                .expect_err("missing provenance field is rejected"),
            CanonicalError::InvalidField
        );

        // A nested limits record missing one of its six fields is rejected
        // instead of silently filling an empty value.
        let incomplete_limits = raw_record(0, 1, &[(1, WireType::U64 as u8, &[1])]);
        let missing_limits_field = raw_record(
            TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
            1,
            &[
                (1, WireType::U64 as u8, &[0]),
                (2, WireType::Uuid as u8, &[3u8; 16]),
                (3, WireType::Digest as u8, &[9u8; 32]),
                (4, WireType::List as u8, &(0u32).to_be_bytes()),
                (5, WireType::Record as u8, &incomplete_limits),
            ],
        );
        assert_eq!(
            ProgrammaticCallerPolicySelectionV1::decode(&missing_limits_field)
                .expect_err("missing run-limit field is rejected"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn uuid_list_count_cap_is_enforced_before_allocation() {
        // A declared count over the cap is rejected even with no item bytes.
        let over_cap = (MAX_UUID_LIST_ITEMS as u32 + 1).to_be_bytes().to_vec();
        assert_eq!(
            decode_uuid_list(&over_cap).expect_err("count over the cap is rejected"),
            CanonicalError::OverLimit
        );

        // A count at the cap with an exact-size payload decodes fully.
        let exact = {
            let mut bytes = (MAX_UUID_LIST_ITEMS as u32).to_be_bytes().to_vec();
            bytes.resize(4 + MAX_UUID_LIST_ITEMS * 16, 0);
            bytes
        };
        assert_eq!(
            decode_uuid_list(&exact)
                .expect("cap-sized list decodes")
                .len(),
            MAX_UUID_LIST_ITEMS
        );

        // A count at the cap with a truncated payload is truncated, not
        // over-limit: the cap check never misfires on valid counts.
        let truncated = (MAX_UUID_LIST_ITEMS as u32).to_be_bytes().to_vec();
        assert_eq!(
            decode_uuid_list(&truncated).expect_err("truncated cap-sized list is truncated"),
            CanonicalError::Truncated
        );
    }

    #[test]
    fn fixed_run_limits_round_trip_and_require_all_six_fields() {
        let limits = FixedRunLimits {
            max_attempts: 1,
            max_total_seconds: 3600,
            max_actions: 1024,
            max_concurrent_actions: 4,
            max_retained_bytes: 1048576,
            max_clarification_seconds: 3600,
        };
        let bytes = limits.encode().expect("run limits encode");
        assert_eq!(
            FixedRunLimits::decode(&bytes).expect("run limits decode"),
            limits
        );

        // A nested record missing any limit field is rejected.
        let incomplete = raw_record(
            0,
            1,
            &[
                (1, WireType::U64 as u8, &[1]),
                (2, WireType::U64 as u8, &[2]),
                (3, WireType::U64 as u8, &[3]),
                (4, WireType::U64 as u8, &[4]),
                (5, WireType::U64 as u8, &[5]),
            ],
        );
        assert_eq!(
            FixedRunLimits::decode(&incomplete).expect_err("missing run-limit field is rejected"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn nested_limits_reject_wrong_record_framing() {
        // The nested limits records are anonymous: tag zero, version one.
        let wrong_tag = raw_record(TagRegistry::RUN_EXECUTION_MEANING, 1, &[]);
        assert_eq!(
            FixedRunLimits::decode(&wrong_tag).expect_err("wrong nested tag is rejected"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            FixedActivityLimits::decode(&wrong_tag)
                .expect_err("wrong nested activity tag is rejected"),
            CanonicalError::InvalidTag
        );
        let wrong_version = raw_record(0, 2, &[]);
        assert_eq!(
            FixedRunLimits::decode(&wrong_version).expect_err("wrong nested version is rejected"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            FixedActivityLimits::decode(&wrong_version)
                .expect_err("wrong nested activity version is rejected"),
            CanonicalError::InvalidTag
        );

        // The framing check propagates through the programmatic selection
        // decoder's nested limits field, even when all six limit values are
        // present under the wrong frame.
        let mis_framed_limits = raw_record(
            TagRegistry::AGENT_ACTIVITY_SELECTION_V1,
            1,
            &[
                (1, WireType::U64 as u8, &[1]),
                (2, WireType::U64 as u8, &[2]),
                (3, WireType::U64 as u8, &[3]),
                (4, WireType::U64 as u8, &[4]),
                (5, WireType::U64 as u8, &[5]),
                (6, WireType::U64 as u8, &[6]),
            ],
        );
        let selection = raw_record(
            TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
            1,
            &[
                (1, WireType::U64 as u8, &[0]),
                (2, WireType::Uuid as u8, &[3u8; 16]),
                (3, WireType::Digest as u8, &[9u8; 32]),
                (4, WireType::List as u8, &(0u32).to_be_bytes()),
                (5, WireType::Record as u8, &mis_framed_limits),
            ],
        );
        assert_eq!(
            ProgrammaticCallerPolicySelectionV1::decode(&selection)
                .expect_err("wrong nested limits framing is rejected"),
            CanonicalError::InvalidTag
        );
    }

    #[test]
    fn activity_limit_constants_match_the_ledger() {
        assert_eq!(MAX_ACTIVITY_MESSAGES, 1024);
        assert_eq!(MAX_ACTIVITY_AGGREGATE_BYTES, 4 * 1024 * 1024);
        assert_eq!(MAX_ACTIVITY_JOURNAL_RECORDS, 4096);
        assert_eq!(MAX_ACTIVITY_RECORD_BYTES, 64 * 1024);
        assert_eq!(MAX_ACTIVITY_PAGE_RECORDS, 256);
        assert_eq!(MAX_ACTIVITY_PAGE_BYTES, 512 * 1024);
        assert_eq!(MAX_ACTIVITY_TYPED_REFERENCES, 16);
        assert_eq!(MAX_ACTIVITY_CLARIFICATION_WAIT_SECONDS, 60 * 60);
        let frozen = FixedActivityLimits::frozen();
        assert_eq!(frozen.max_messages, MAX_ACTIVITY_MESSAGES);
        assert_eq!(frozen.max_aggregate_bytes, MAX_ACTIVITY_AGGREGATE_BYTES);
        assert_eq!(frozen.max_journal_records, MAX_ACTIVITY_JOURNAL_RECORDS);
        assert_eq!(frozen.max_record_bytes, MAX_ACTIVITY_RECORD_BYTES);
        assert_eq!(frozen.max_page_records, MAX_ACTIVITY_PAGE_RECORDS);
        assert_eq!(frozen.max_page_bytes, MAX_ACTIVITY_PAGE_BYTES);
        assert_eq!(frozen.max_typed_references, MAX_ACTIVITY_TYPED_REFERENCES);
        assert_eq!(
            frozen.max_clarification_wait_seconds,
            MAX_ACTIVITY_CLARIFICATION_WAIT_SECONDS
        );
    }

    /// Returns one activity limit value by its nested field number.
    fn activity_limit_value(limits: &FixedActivityLimits, number: u32) -> u64 {
        match number {
            1 => limits.max_messages,
            2 => limits.max_aggregate_bytes,
            3 => limits.max_journal_records,
            4 => limits.max_record_bytes,
            5 => limits.max_page_records,
            6 => limits.max_page_bytes,
            7 => limits.max_typed_references,
            8 => limits.max_clarification_wait_seconds,
            _ => 0,
        }
    }

    /// Builds one activity limits value with a single field replaced.
    fn activity_limits_with(
        limits: &FixedActivityLimits,
        number: u32,
        value: u64,
    ) -> FixedActivityLimits {
        let mut replaced = limits.clone();
        match number {
            1 => replaced.max_messages = value,
            2 => replaced.max_aggregate_bytes = value,
            3 => replaced.max_journal_records = value,
            4 => replaced.max_record_bytes = value,
            5 => replaced.max_page_records = value,
            6 => replaced.max_page_bytes = value,
            7 => replaced.max_typed_references = value,
            8 => replaced.max_clarification_wait_seconds = value,
            _ => {}
        }
        replaced
    }

    /// Encodes an activity limits record without the production validation.
    fn raw_activity_limits_record(limits: &FixedActivityLimits) -> Vec<u8> {
        record(
            0,
            1,
            (1..=8)
                .map(|number| {
                    (
                        number,
                        WireType::U64,
                        encode_u64(activity_limit_value(limits, number)),
                    )
                })
                .collect(),
        )
        .expect("raw activity limits record encodes")
    }

    #[test]
    fn activity_limit_boundaries_are_enforced_on_encode_and_decode() {
        let frozen = FixedActivityLimits::frozen();
        assert_eq!(frozen.validate(), Ok(()));
        let bytes = frozen.encode().expect("frozen limits encode");
        assert_eq!(
            FixedActivityLimits::decode(&bytes).expect("frozen limits decode"),
            frozen
        );
        // The frozen record embeds all eight ledger fields.
        let reader = CanonicalRecordReader::new(&bytes, 8).expect("limits record parses");
        for number in 1..=8 {
            assert!(
                reader
                    .field(number, WireType::U64)
                    .expect("field lookup succeeds")
                    .is_some(),
                "field {number}"
            );
        }
        for number in 1..=8 {
            let value = activity_limit_value(&frozen, number);
            // One below the frozen value is over-limit on validate, encode,
            // and decode.
            let below = activity_limits_with(&frozen, number, value - 1);
            assert_eq!(
                below.validate(),
                Err(CanonicalError::OverLimit),
                "below field {number}"
            );
            assert!(below.encode().is_err(), "below field {number}");
            assert_eq!(
                FixedActivityLimits::decode(&raw_activity_limits_record(&below))
                    .expect_err("below field is over-limit"),
                CanonicalError::OverLimit
            );
            // One above the frozen value is over-limit on validate, encode,
            // and decode.
            let above = activity_limits_with(&frozen, number, value + 1);
            assert_eq!(
                above.validate(),
                Err(CanonicalError::OverLimit),
                "above field {number}"
            );
            assert!(above.encode().is_err(), "above field {number}");
            assert_eq!(
                FixedActivityLimits::decode(&raw_activity_limits_record(&above))
                    .expect_err("above field is over-limit"),
                CanonicalError::OverLimit
            );
        }
    }

    #[test]
    fn activity_selection_round_trips_frozen_limits_inside_both_variants() {
        let root = AgentActivitySelectionV1::Root {
            activity_tree_id: [7u8; 16],
            root_origin: ExecutionKind::Ordinary,
            activity_exchange_revision: 1,
            activity_journal_revision: 2,
            user_projection_revision: 3,
            fixed_activity_limits: FixedActivityLimits::frozen(),
        };
        assert_eq!(
            AgentActivitySelectionV1::decode(&root.encode().expect("root selection encodes"))
                .expect("root selection decodes"),
            root
        );
        let descendant = AgentActivitySelectionV1::Descendant {
            activity_tree_id: [8u8; 16],
            direct_parent_link_reference: [9u8; 16],
            activity_exchange_revision: 1,
            activity_journal_revision: 2,
            user_projection_revision: 3,
            fixed_activity_limits: FixedActivityLimits::frozen(),
        };
        assert_eq!(
            AgentActivitySelectionV1::decode(
                &descendant.encode().expect("descendant selection encodes"),
            )
            .expect("descendant selection decodes"),
            descendant
        );
    }

    #[test]
    fn identity_v1_formats_and_parses_strictly() {
        let identity = IdentityV1::sha256(b"intention-relay");
        let text = identity.to_string();
        assert!(text.starts_with("sha256-v1:"));
        assert_eq!(text.len(), "sha256-v1:".len() + 64);
        assert_eq!(
            text.parse::<IdentityV1>().expect("identity parses"),
            identity
        );
        assert_eq!(
            identity.bytes(),
            Digest256::sha256(b"intention-relay").bytes()
        );
        // The text form is lowercase-only and fixed-width.
        assert!(text.to_uppercase().parse::<IdentityV1>().is_err());
        assert!("sha256-v1:".parse::<IdentityV1>().is_err());
        assert!(
            format!("sha256-v1:{}", "a".repeat(63))
                .parse::<IdentityV1>()
                .is_err()
        );
        assert!("sha256-v1:".to_owned().parse::<IdentityV1>().is_err());
        assert!(
            text.replacen("sha256-v1:", "sha256:", 1)
                .parse::<IdentityV1>()
                .is_err()
        );
        assert_eq!(
            IdentityV1::sha256(b"a").to_string(),
            format!(
                "sha256-v1:{}",
                hex_encode(&IdentityV1::sha256(b"a").bytes())
            )
        );
    }

    #[test]
    fn namespaced_digest_formats_parses_and_validates_namespaces() {
        let bytes = b"canonical payload";
        let digest = Digest256::for_namespace("run", bytes).expect("namespace is valid");
        assert_eq!(
            digest,
            NamespacedDigest {
                namespace: "run".to_owned(),
                digest: Digest256::sha256(bytes),
            }
        );
        let text = digest.to_string();
        assert_eq!(
            text,
            format!(
                "run:sha256:{}",
                hex_encode(&Digest256::sha256(bytes).bytes())
            )
        );
        assert_eq!(
            text.parse::<NamespacedDigest>()
                .expect("namespaced digest parses"),
            digest
        );
        // Strict parsing rejections: empty namespace, uppercase digits, wrong
        // widths, a second separator, and a missing separator.
        let digits64 = "a".repeat(64);
        assert!(
            format!(":sha256:{digits64}")
                .parse::<NamespacedDigest>()
                .is_err()
        );
        assert!(
            format!("run:sha256:{}", "A".repeat(64))
                .parse::<NamespacedDigest>()
                .is_err()
        );
        assert!(
            format!("run:sha256:{}", "a".repeat(63))
                .parse::<NamespacedDigest>()
                .is_err()
        );
        assert!(
            format!("run:sha256:{digits64}:sha256:{digits64}")
                .parse::<NamespacedDigest>()
                .is_err()
        );
        assert!("run-sha256-abcdef".parse::<NamespacedDigest>().is_err());
        // for_namespace validates the namespace character set.
        assert!(Digest256::for_namespace("", bytes).is_err());
        assert!(Digest256::for_namespace("bad space", bytes).is_err());
        assert!(Digest256::for_namespace("bad/name", bytes).is_err());
        assert!(Digest256::for_namespace("bad:name", bytes).is_err());
        for namespace in ["a", "A0", "run-2", "under_score", "dot.name"] {
            assert!(Digest256::for_namespace(namespace, bytes).is_ok());
        }
        // NamespacedDigest::from_str enforces the same namespace grammar:
        // non-empty ASCII letters, digits, `-`, `_`, and `.`.
        for bad in ["bad space", "bad:name", "bad/name"] {
            assert!(
                format!("{bad}:sha256:{digits64}")
                    .parse::<NamespacedDigest>()
                    .is_err(),
                "namespace {bad:?} must be rejected"
            );
        }
        for namespace in ["a", "A0", "run-2", "under_score", "dot.name"] {
            assert!(
                format!("{namespace}:sha256:{digits64}")
                    .parse::<NamespacedDigest>()
                    .is_ok(),
                "namespace {namespace:?} must parse"
            );
        }
    }

    /// Builds the identity input over one run-execution-meaning record.
    fn identity_input_for(meaning: &[u8], meaning_version: u32) -> CanonicalIdentityInput {
        CanonicalIdentityInput::new()
            .field(1, WireType::U64, ExecutionKind::Ordinary.enc())
            .expect("execution kind field accepts")
            .field(
                2,
                WireType::U64,
                encode_u64(TagRegistry::RUN_EXECUTION_MEANING as u64),
            )
            .expect("meaning tag field accepts")
            .field(3, WireType::U64, encode_u64(meaning_version as u64))
            .expect("meaning version field accepts")
            .field(4, WireType::U64, encode_u64(1))
            .expect("canonicalization version field accepts")
            .field(5, WireType::Bytes, meaning.to_vec())
            .expect("meaning bytes field accepts")
    }

    #[test]
    fn identity_input_excludes_non_identity_fields_with_sentinels() {
        let v3 = golden_v3_record().encode().expect("v3 meaning encodes");
        let plain = identity_input_for(&v3, 3);
        let encoded = plain.encode().expect("identity input encodes");
        let baseline = plain.digest().expect("identity digests");

        // The digest field is excluded by construction: the identity is always
        // recomputed from the encoded input and never accepts a stored digest.
        assert_eq!(baseline, IdentityV1::sha256(&encoded));

        // Credentials, filesystem paths, display/presentation data, readiness,
        // and current state are excluded: sentinel values never change the
        // digest, individually or together.
        let sentinels = plain
            .with_credentials(vec![0xDE, 0xAD, 0xBE, 0xEF])
            .with_filesystem_path(
                std::env::temp_dir()
                    .join("intention-relay-credential-path")
                    .to_string_lossy()
                    .into_owned(),
            )
            .with_display_data("display name".to_owned())
            .with_readiness(true)
            .with_current_state(vec![0x01, 0x02, 0x03]);
        assert_eq!(
            sentinels
                .digest()
                .expect("excluded values keep the identity"),
            baseline
        );
        for changed in [
            sentinels
                .clone()
                .with_credentials(vec![0x00; 16])
                .digest()
                .expect("credentials stay excluded"),
            sentinels
                .clone()
                .with_filesystem_path(
                    std::env::temp_dir()
                        .join("other-credential-path")
                        .to_string_lossy()
                        .into_owned(),
                )
                .digest()
                .expect("paths stay excluded"),
            sentinels
                .clone()
                .with_display_data("other display".to_owned())
                .digest()
                .expect("display data stays excluded"),
            sentinels
                .clone()
                .with_readiness(false)
                .digest()
                .expect("readiness stays excluded"),
            sentinels
                .clone()
                .with_current_state(vec![0xFF; 16])
                .digest()
                .expect("current state stays excluded"),
        ] {
            assert_eq!(changed, baseline);
        }
        // Excluded values never reach the encoded bytes.
        assert_eq!(sentinels.encode().expect("identity input encodes"), encoded);
    }

    #[test]
    fn identity_input_builder_enforces_strictly_increasing_fields() {
        assert_eq!(
            CanonicalIdentityInput::new()
                .field(1, WireType::U64, vec![0])
                .expect("first field accepts")
                .field(1, WireType::U64, vec![1])
                .expect_err("duplicate field is rejected"),
            CanonicalError::DuplicateOrDescendingField
        );
        assert_eq!(
            CanonicalIdentityInput::new()
                .field(2, WireType::U64, vec![0])
                .expect("higher field accepts")
                .field(1, WireType::U64, vec![1])
                .expect_err("descending field is rejected"),
            CanonicalError::DuplicateOrDescendingField
        );
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
        FixedActivityLimits::frozen()
    }

    /// Encodes the fixed activity limits as their nested limits record, which
    /// mirrors the production `FixedActivityLimits::encode` encoding
    /// byte-for-byte.
    fn golden_activity_limits_record() -> Vec<u8> {
        golden_activity_limits()
            .encode()
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
            agent_activity_selection: AgentActivitySelectionV1::Root {
                activity_tree_id: golden_uuid("11111111-1111-4111-8111-111111111111"),
                root_origin: ExecutionKind::Ordinary,
                activity_exchange_revision: 1,
                activity_journal_revision: 1,
                user_projection_revision: 1,
                fixed_activity_limits: golden_activity_limits(),
            }
            .encode()
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
        namespace: String,
    }

    /// Parses the line-oriented golden fixture format.
    fn parse_golden(text: &str) -> Result<GoldenFixture, String> {
        let mut fixture = GoldenFixture {
            record: String::new(),
            tag: 0,
            record_version: 0,
            bytes_hex: String::new(),
            sha256: String::new(),
            namespace: String::new(),
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
                "namespace" => fixture.namespace = value.to_owned(),
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

    fn golden_activity_selection_root() -> AgentActivitySelectionV1 {
        AgentActivitySelectionV1::Root {
            activity_tree_id: golden_uuid("11111111-1111-4111-8111-111111111111"),
            root_origin: ExecutionKind::Ordinary,
            activity_exchange_revision: 1,
            activity_journal_revision: 1,
            user_projection_revision: 1,
            fixed_activity_limits: golden_activity_limits(),
        }
    }

    fn golden_activity_selection_descendant() -> AgentActivitySelectionV1 {
        AgentActivitySelectionV1::Descendant {
            activity_tree_id: golden_uuid("11111111-1111-4111-8111-111111111111"),
            direct_parent_link_reference: golden_uuid("44444444-4444-4444-8444-444444444444"),
            activity_exchange_revision: 1,
            activity_journal_revision: 1,
            user_projection_revision: 1,
            fixed_activity_limits: golden_activity_limits(),
        }
    }

    fn golden_selection_with_provenance() -> ProgrammaticCallerPolicySelectionV1 {
        ProgrammaticCallerPolicySelectionV1 {
            root_origin: ExecutionKind::Ordinary,
            effective_policy_snapshot_reference: golden_uuid(
                "22222222-2222-4222-8222-222222222222",
            ),
            policy_selection_digest: Digest256::from_bytes(&[0x11; 32])
                .expect("golden digest is valid"),
            inherited_scope_provenance: vec![
                golden_uuid("33333333-3333-4333-8333-333333333333"),
                golden_uuid("44444444-4444-4444-8444-444444444444"),
            ],
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

    fn golden_identity_input_v4() -> CanonicalIdentityInput {
        let meaning = golden_v4_record()
            .encode()
            .expect("golden v4 meaning encodes");
        identity_input_for(&meaning, 4)
    }

    #[test]
    fn golden_agent_activity_selection_root_fixture_matches_the_encoder_and_round_trips() {
        let selection = golden_activity_selection_root();
        let encoded = selection.encode().expect("root selection encodes");
        assert_golden_fixture(
            include_str!("../tests/fixtures/goldens/agent-activity-selection-root-v1.txt"),
            "agent-activity-selection-root-v1",
            0x0202,
            1,
            &encoded,
        );
        assert_eq!(
            AgentActivitySelectionV1::decode(&encoded).expect("golden root selection decodes"),
            selection
        );
    }

    #[test]
    fn golden_agent_activity_selection_descendant_fixture_matches_the_encoder_and_round_trips() {
        let selection = golden_activity_selection_descendant();
        let encoded = selection.encode().expect("descendant selection encodes");
        assert_golden_fixture(
            include_str!("../tests/fixtures/goldens/agent-activity-selection-descendant-v1.txt"),
            "agent-activity-selection-descendant-v1",
            0x0202,
            2,
            &encoded,
        );
        assert_eq!(
            AgentActivitySelectionV1::decode(&encoded)
                .expect("golden descendant selection decodes"),
            selection
        );
    }

    #[test]
    fn golden_programmatic_policy_provenance_fixture_matches_the_encoder_and_round_trips() {
        let selection = golden_selection_with_provenance();
        let encoded = selection.encode().expect("selection encodes");
        assert_golden_fixture(
            include_str!(
                "../tests/fixtures/goldens/programmatic-policy-selection-provenance-v1.txt"
            ),
            "programmatic-policy-selection-provenance-v1",
            0x0201,
            1,
            &encoded,
        );
        assert_eq!(
            ProgrammaticCallerPolicySelectionV1::decode(&encoded)
                .expect("golden selection decodes"),
            selection
        );
    }

    #[test]
    fn golden_identity_v1_fixture_matches_the_encoder_and_round_trips() {
        let input = golden_identity_input_v4();
        let encoded = input.encode().expect("identity input encodes");
        assert_golden_fixture(
            include_str!("../tests/fixtures/goldens/identity-v1.txt"),
            "identity-v1",
            0x0000,
            1,
            &encoded,
        );
        // The golden sha256 is exactly the identity's 64 lowercase hex digits.
        let identity = input.digest().expect("identity digests");
        assert_eq!(identity, IdentityV1::sha256(&encoded));
        let fixture = parse_golden(include_str!("../tests/fixtures/goldens/identity-v1.txt"))
            .expect("identity fixture parses");
        assert_eq!(hex_encode(&identity.bytes()), fixture.sha256);
        assert_eq!(
            identity
                .to_string()
                .parse::<IdentityV1>()
                .expect("identity parses"),
            identity
        );
    }

    #[test]
    fn golden_namespaced_digest_fixture_matches_the_encoder_and_round_trips() {
        let v3 = golden_v3_record()
            .encode()
            .expect("golden v3 meaning encodes");
        assert_golden_fixture(
            include_str!("../tests/fixtures/goldens/namespaced-digest-v1.txt"),
            "namespaced-digest-v1",
            0x0101,
            3,
            &v3,
        );
        let fixture = parse_golden(include_str!(
            "../tests/fixtures/goldens/namespaced-digest-v1.txt"
        ))
        .expect("namespaced digest fixture parses");
        let digest =
            Digest256::for_namespace(&fixture.namespace, &v3).expect("golden namespace is valid");
        assert_eq!(hex_encode(&digest.digest.bytes()), fixture.sha256);
        assert_eq!(
            digest.to_string(),
            format!("{}:sha256:{}", fixture.namespace, fixture.sha256)
        );
        assert_eq!(
            digest
                .to_string()
                .parse::<NamespacedDigest>()
                .expect("namespaced digest parses"),
            digest
        );
    }

    #[test]
    fn golden_identity_exclusion_fixture_matches_the_encoder_and_round_trips() {
        let v3 = golden_v3_record()
            .encode()
            .expect("golden v3 meaning encodes");
        let input = identity_input_for(&v3, 3)
            .with_credentials(vec![0xDE, 0xAD, 0xBE, 0xEF])
            .with_filesystem_path(
                std::env::temp_dir()
                    .join("intention-relay-excluded-path")
                    .to_string_lossy()
                    .into_owned(),
            )
            .with_display_data("excluded display".to_owned())
            .with_readiness(true)
            .with_current_state(vec![0x01, 0x02, 0x03]);
        let encoded = input.encode().expect("identity input encodes");
        assert_golden_fixture(
            include_str!("../tests/fixtures/goldens/identity-exclusion-v1.txt"),
            "identity-exclusion-v1",
            0x0000,
            1,
            &encoded,
        );
        // The golden identity equals the identity of the same input without
        // any excluded values.
        let baseline = identity_input_for(&v3, 3)
            .digest()
            .expect("baseline identity digests");
        assert_eq!(input.digest().expect("identity digests"), baseline);
        let fixture = parse_golden(include_str!(
            "../tests/fixtures/goldens/identity-exclusion-v1.txt"
        ))
        .expect("identity exclusion fixture parses");
        assert_eq!(hex_encode(&baseline.bytes()), fixture.sha256);
    }

    #[test]
    fn tag_registry_parity_with_the_adr_0036_ledger() {
        // Every ledger tag maps to exactly one registry constant with the
        // matching numeric value.
        let registry_value = |name: &str| -> Option<u32> {
            match name {
                "run-execution-meaning" => Some(TagRegistry::RUN_EXECUTION_MEANING),
                "programmatic-caller-policy-selection-v1" => {
                    Some(TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1)
                }
                "agent-activity-selection-v1" => Some(TagRegistry::AGENT_ACTIVITY_SELECTION_V1),
                "goal-run-selection-v1" => Some(TagRegistry::GOAL_RUN_SELECTION_V1),
                "continual-harness-selection-v1" => {
                    Some(TagRegistry::CONTINUAL_HARNESS_SELECTION_V1)
                }
                "mcp-method-catalog-selection-v1" => {
                    Some(TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1)
                }
                "model-capability-taxonomy-v1" => Some(TagRegistry::MODEL_CAPABILITY_TAXONOMY_V1),
                "provider-profile-revision-v1" => Some(TagRegistry::PROVIDER_PROFILE_REVISION_V1),
                "provider-selection-v1" => Some(TagRegistry::PROVIDER_SELECTION_V1),
                "reasoning-history-manifest-v1" => Some(TagRegistry::REASONING_HISTORY_MANIFEST_V1),
                "context-source-manifest-v1" => Some(TagRegistry::CONTEXT_SOURCE_MANIFEST_V1),
                "model-context-projection-v1" => Some(TagRegistry::MODEL_CONTEXT_PROJECTION_V1),
                "legacy-m4-selection-binding" => Some(TagRegistry::LEGACY_M4_SELECTION_BINDING),
                "tool-descriptor-revision" => Some(TagRegistry::TOOL_DESCRIPTOR_REVISION),
                "tool-registry-revision" => Some(TagRegistry::TOOL_REGISTRY_REVISION),
                "model-tool-loop-v1" => Some(TagRegistry::MODEL_TOOL_LOOP_V1),
                "bridge-invocation-v1" => Some(TagRegistry::BRIDGE_INVOCATION_V1),
                "fork-base-snapshot-v1/v2" => Some(TagRegistry::FORK_BASE_SNAPSHOT_V1),
                "fork-preview-v1/v2" => Some(TagRegistry::FORK_PREVIEW_V1),
                "fork-command-v1" => Some(TagRegistry::FORK_COMMAND_V1),
                "agent-activity-tree-v1" => Some(TagRegistry::AGENT_ACTIVITY_TREE_V1),
                "agent-activity-pair-v1" => Some(TagRegistry::AGENT_ACTIVITY_PAIR_V1),
                "agent-message-v1" => Some(TagRegistry::AGENT_MESSAGE_V1),
                "agent-activity-journal-record-v1" => {
                    Some(TagRegistry::AGENT_ACTIVITY_JOURNAL_RECORD_V1)
                }
                "agent-notification-record-v1" => Some(TagRegistry::AGENT_NOTIFICATION_RECORD_V1),
                _ => None,
            }
        };
        let mut values = Vec::new();
        for entry in TagRegistry::LEDGER {
            assert_eq!(
                registry_value(entry.name).expect("ledger tag has a registry constant"),
                entry.value,
                "ledger tag {} must match its registry constant",
                entry.name
            );
            assert!(
                !values.contains(&entry.value),
                "ledger tag value 0x{:04x} appears more than once",
                entry.value
            );
            values.push(entry.value);
            let wired = matches!(
                entry.name,
                "run-execution-meaning"
                    | "programmatic-caller-policy-selection-v1"
                    | "agent-activity-selection-v1"
            );
            assert_eq!(
                entry.status,
                if wired {
                    TagStatus::Wired
                } else {
                    TagStatus::ReservedForSlice2
                },
                "unexpected status for ledger tag {}",
                entry.name
            );
        }
        // The fork aliases are single registry entries covering both versions.
        assert_eq!(TagRegistry::FORK_BASE_SNAPSHOT_V1, 0x0401);
        assert_eq!(TagRegistry::FORK_PREVIEW_V1, 0x0402);
        assert_eq!(
            TagRegistry::LEDGER
                .iter()
                .find(|entry| entry.name == "fork-base-snapshot-v1/v2")
                .expect("fork base alias exists")
                .value,
            0x0401
        );
        assert_eq!(
            TagRegistry::LEDGER
                .iter()
                .find(|entry| entry.name == "fork-preview-v1/v2")
                .expect("fork preview alias exists")
                .value,
            0x0402
        );
        // Every registry constant appears in the ledger table exactly once.
        let registry_entries: [(u32, &str); 25] = [
            (TagRegistry::RUN_EXECUTION_MEANING, "run-execution-meaning"),
            (
                TagRegistry::PROGRAMMATIC_CALLER_POLICY_SELECTION_V1,
                "programmatic-caller-policy-selection-v1",
            ),
            (
                TagRegistry::AGENT_ACTIVITY_SELECTION_V1,
                "agent-activity-selection-v1",
            ),
            (TagRegistry::GOAL_RUN_SELECTION_V1, "goal-run-selection-v1"),
            (
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
                "continual-harness-selection-v1",
            ),
            (
                TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1,
                "mcp-method-catalog-selection-v1",
            ),
            (
                TagRegistry::MODEL_CAPABILITY_TAXONOMY_V1,
                "model-capability-taxonomy-v1",
            ),
            (
                TagRegistry::PROVIDER_PROFILE_REVISION_V1,
                "provider-profile-revision-v1",
            ),
            (TagRegistry::PROVIDER_SELECTION_V1, "provider-selection-v1"),
            (
                TagRegistry::REASONING_HISTORY_MANIFEST_V1,
                "reasoning-history-manifest-v1",
            ),
            (
                TagRegistry::CONTEXT_SOURCE_MANIFEST_V1,
                "context-source-manifest-v1",
            ),
            (
                TagRegistry::MODEL_CONTEXT_PROJECTION_V1,
                "model-context-projection-v1",
            ),
            (
                TagRegistry::LEGACY_M4_SELECTION_BINDING,
                "legacy-m4-selection-binding",
            ),
            (
                TagRegistry::TOOL_DESCRIPTOR_REVISION,
                "tool-descriptor-revision",
            ),
            (
                TagRegistry::TOOL_REGISTRY_REVISION,
                "tool-registry-revision",
            ),
            (TagRegistry::MODEL_TOOL_LOOP_V1, "model-tool-loop-v1"),
            (TagRegistry::BRIDGE_INVOCATION_V1, "bridge-invocation-v1"),
            (
                TagRegistry::FORK_BASE_SNAPSHOT_V1,
                "fork-base-snapshot-v1/v2",
            ),
            (TagRegistry::FORK_PREVIEW_V1, "fork-preview-v1/v2"),
            (TagRegistry::FORK_COMMAND_V1, "fork-command-v1"),
            (
                TagRegistry::AGENT_ACTIVITY_TREE_V1,
                "agent-activity-tree-v1",
            ),
            (
                TagRegistry::AGENT_ACTIVITY_PAIR_V1,
                "agent-activity-pair-v1",
            ),
            (TagRegistry::AGENT_MESSAGE_V1, "agent-message-v1"),
            (
                TagRegistry::AGENT_ACTIVITY_JOURNAL_RECORD_V1,
                "agent-activity-journal-record-v1",
            ),
            (
                TagRegistry::AGENT_NOTIFICATION_RECORD_V1,
                "agent-notification-record-v1",
            ),
        ];
        assert_eq!(TagRegistry::LEDGER.len(), registry_entries.len());
        for (value, name) in registry_entries {
            assert!(
                TagRegistry::LEDGER
                    .iter()
                    .any(|entry| entry.value == value && entry.name == name),
                "registry constant {name} (0x{value:04x}) is missing from the ledger table"
            );
        }
    }
}
