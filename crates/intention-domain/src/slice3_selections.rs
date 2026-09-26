//! Frozen Slice 3 canonical selection records.
//!
//! Owner: architectures 26/27/28 through ADR 0044. These records bind the
//! `run-execution-meaning-v4` selection slots 7-10: the continual-harness
//! selection (`0x0204`), the goal-run selection (`0x0203`), the MCP method
//! catalog selection (`0x0205`), and the programmatic-caller root origin
//! nested in the programmatic-caller policy selection (`0x0201`, field 1).
//!
//! Every record is credential-free, typed, versioned, and immutable. The
//! records carry safe identity, revision, digest, and bound values only:
//! never raw content, credentials, paths, grants, provider resources, process
//! handles, or implementation state. Historical M4 and non-harness runs
//! acquire no synthetic record.

use crate::canonical::{
    CanonicalError, CanonicalRecordReader, Digest256, TagRegistry, WireType,
    contains_control_or_nul, contains_credential_shape, decode_list_items, decode_optional_utf8,
    decode_u64, decode_utf8, decode_utf8_list, decode_uuid_list, encode_bool, encode_list_items,
    encode_optional_utf8, encode_u64, encode_utf8, encode_utf8_list,
};
use crate::run_execution_meaning::{DisabledOr, record, uuid};

/// Maximum characters in one Slice 3 selection scalar text value.
pub const MAX_SLICE3_TEXT_CHARS: usize = 256;

/// Maximum encoded bytes of one goal-run selection record (1 MiB).
pub const MAX_GOAL_RUN_SELECTION_BYTES: usize = 1024 * 1024;

/// Maximum encoded bytes of one continual-harness selection record (512 KiB).
pub const MAX_CONTINUAL_HARNESS_SELECTION_BYTES: usize = 512 * 1024;

/// Maximum encoded bytes of one MCP method catalog selection record (512 KiB).
pub const MAX_MCP_METHOD_CATALOG_SELECTION_BYTES: usize = 512 * 1024;

/// Harness completion-cause chain depth bound.
pub const HARNESS_MAX_CAUSE_DEPTH: u64 = 8;

/// Harness concurrent non-terminal work bound.
pub const HARNESS_MAX_CONCURRENT: u64 = 16;

/// Harness total launch bound from one original cause.
pub const HARNESS_MAX_TOTAL_LAUNCHES: u64 = 256;

/// Harness dossier byte bound.
pub const HARNESS_MAX_DOSSIER_BYTES: u64 = 512 * 1024;

/// Harness verified checkpoint byte bound.
pub const HARNESS_MAX_CHECKPOINT_BYTES: u64 = 512 * 1024;

/// Harness safe conclusion byte bound.
pub const HARNESS_MAX_CONCLUSION_BYTES: u64 = 512 * 1024;

/// Maximum narrowed tool identifiers in one harness class resolution.
pub const HARNESS_MAX_NARROWED_TOOLS: usize = 16;

/// Goal parent revision chain bound.
pub const GOAL_MAX_PARENT_CHAIN: usize = 16;

/// Goal obligatory component reference bound.
pub const GOAL_MAX_COMPONENT_REFERENCES: usize = 32;

/// Goal selected gate revision bound.
pub const GOAL_MAX_GATE_REVISIONS: usize = 32;

/// Goal selected evidence reference bound.
pub const GOAL_MAX_EVIDENCE_REFERENCES: usize = 512;

/// Active memory card bound in one goal-run selection.
pub const GOAL_MAX_MEMORY_CARDS: u64 = 128;

/// Selected skill and role card bound in one goal-run selection.
pub const GOAL_MAX_SKILL_ROLE_CARDS: u64 = 32;

/// Revealed full-record reference bound in one goal-run selection.
pub const GOAL_MAX_REVEALED_RECORDS: usize = 512;

/// Goal canonical target snapshot byte bound (1 MiB).
pub const GOAL_MAX_TARGET_SNAPSHOT_BYTES: u64 = 1024 * 1024;

/// Goal context byte bound in one run (4 MiB).
pub const GOAL_MAX_CONTEXT_BYTES: u64 = 4 * 1024 * 1024;

/// Validates one Slice 3 selection text value.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidField` for a blank, over-long, or
/// control-bearing value and `CanonicalError::CredentialsForbidden` for a
/// credential-shaped value.
fn validate_text(value: &str) -> Result<(), CanonicalError> {
    if value.trim().is_empty()
        || value.chars().count() > MAX_SLICE3_TEXT_CHARS
        || contains_control_or_nul(value)
    {
        return Err(CanonicalError::InvalidField);
    }
    if contains_credential_shape(value) {
        return Err(CanonicalError::CredentialsForbidden);
    }
    Ok(())
}

/// Encodes a strict list of raw UUID values.
fn encode_uuid_list(items: &[[u8; 16]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + items.len() * 16);
    out.extend_from_slice(&(items.len() as u32).to_be_bytes());
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// Encodes an optional UUID as a one-byte presence marker followed by the
/// sixteen raw bytes when present.
fn encode_optional_uuid(value: &Option<[u8; 16]>) -> Vec<u8> {
    value.as_ref().map_or_else(
        || vec![0],
        |bytes| {
            let mut out = vec![1];
            out.extend_from_slice(bytes);
            out
        },
    )
}

/// Decodes an optional UUID from its one-byte presence framing.
///
/// # Errors
///
/// Returns `CanonicalError::InvalidOptional` when the marker is unknown or
/// the payload length is not sixteen bytes for a present value.
fn decode_optional_uuid(bytes: &[u8]) -> Result<Option<[u8; 16]>, CanonicalError> {
    match bytes.split_first() {
        Some((0, [])) => Ok(None),
        Some((1, rest)) if rest.len() == 16 => Ok(Some(uuid(rest)?)),
        _ => Err(CanonicalError::InvalidOptional),
    }
}

/// The closed programmatic-caller root origin of one policy selection.
///
/// Exactly two roots exist in this first scope; no protocol peer, task,
/// child, MCP service, provider, bridge channel, queue item, replay, or
/// recovery becomes an independent root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgrammaticCallerRootOriginV1 {
    /// The root of an ordinary user-admitted run and all of its descendants.
    InteractiveUser {
        /// The originating user turn identity.
        originating_turn_id: [u8; 16],
    },
    /// The root of one separately admitted harness launch and descendants.
    ContinualHarness {
        /// The harness identity.
        harness_id: [u8; 16],
        /// The active immutable rule revision that admitted the launch.
        rule_revision: u64,
        /// The durable trigger reason identity.
        trigger_reason_id: [u8; 16],
    },
}

impl ProgrammaticCallerRootOriginV1 {
    /// Encodes this root origin into its nested anonymous record.
    ///
    /// Version one is the `InteractiveUser` variant and version two the
    /// `ContinualHarness` variant.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::DuplicateOrDescendingField` only if the fixed
    /// field table were noncanonical, and `CanonicalError::OverLimit` only if
    /// a field or the record exceeded the codec size bounds; both are
    /// impossible by construction.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        match self {
            Self::InteractiveUser {
                originating_turn_id,
            } => record(
                0,
                1,
                vec![(1, WireType::Uuid, originating_turn_id.to_vec())],
            ),
            Self::ContinualHarness {
                harness_id,
                rule_revision,
                trigger_reason_id,
            } => record(
                0,
                2,
                vec![
                    (1, WireType::Uuid, harness_id.to_vec()),
                    (2, WireType::U64, encode_u64(*rule_revision)),
                    (3, WireType::Uuid, trigger_reason_id.to_vec()),
                ],
            ),
        }
    }

    /// Decodes this root origin from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown variant version,
    /// `CanonicalError::InvalidField` for an absent or malformed field, and
    /// other `CanonicalError` values for malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 3)?;
        if reader.tag != 0 {
            return Err(CanonicalError::InvalidTag);
        }
        match reader.version {
            1 => Ok(Self::InteractiveUser {
                originating_turn_id: uuid(
                    reader
                        .field(1, WireType::Uuid)?
                        .ok_or(CanonicalError::InvalidField)?,
                )?,
            }),
            2 => Ok(Self::ContinualHarness {
                harness_id: uuid(
                    reader
                        .field(1, WireType::Uuid)?
                        .ok_or(CanonicalError::InvalidField)?,
                )?,
                rule_revision: decode_u64(
                    reader
                        .field(2, WireType::U64)?
                        .ok_or(CanonicalError::InvalidField)?,
                )?,
                trigger_reason_id: uuid(
                    reader
                        .field(3, WireType::Uuid)?
                        .ok_or(CanonicalError::InvalidField)?,
                )?,
            }),
            _ => Err(CanonicalError::InvalidTag),
        }
    }
}

/// The closed harness trigger source kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessSourceKindV1 {
    /// Explicit user launch.
    ExplicitUserLaunch,
    /// Project-time-zone calendar time.
    CalendarTime,
    /// A fixed equal interval.
    FixedInterval,
    /// A selected known terminal outcome of another harness or session.
    TerminalOutcomeLink,
}

impl HarnessSourceKindV1 {
    fn enc(self) -> Vec<u8> {
        vec![match self {
            Self::ExplicitUserLaunch => 0,
            Self::CalendarTime => 1,
            Self::FixedInterval => 2,
            Self::TerminalOutcomeLink => 3,
        }]
    }

    fn dec(bytes: &[u8]) -> Result<Self, CanonicalError> {
        match bytes {
            [0] => Ok(Self::ExplicitUserLaunch),
            [1] => Ok(Self::CalendarTime),
            [2] => Ok(Self::FixedInterval),
            [3] => Ok(Self::TerminalOutcomeLink),
            _ => Err(CanonicalError::InvalidField),
        }
    }
}

/// The closed harness execution classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HarnessExecutionClassV1 {
    /// The light inherited class.
    Light,
    /// The medium inherited class.
    Medium,
    /// The heavy inherited class.
    Heavy,
}

impl HarnessExecutionClassV1 {
    fn enc(self) -> Vec<u8> {
        vec![match self {
            Self::Light => 0,
            Self::Medium => 1,
            Self::Heavy => 2,
        }]
    }

    fn dec(bytes: &[u8]) -> Result<Self, CanonicalError> {
        match bytes {
            [0] => Ok(Self::Light),
            [1] => Ok(Self::Medium),
            [2] => Ok(Self::Heavy),
            _ => Err(CanonicalError::InvalidField),
        }
    }
}

/// One durable harness trigger reason captured before admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessTriggerReasonV1 {
    /// The stable daemon-assigned reason identity.
    pub reason_id: [u8; 16],
    /// The closed trigger source kind.
    pub source_kind: HarnessSourceKindV1,
    /// The first observation time in Unix milliseconds.
    pub first_observed_at_ms: u64,
    /// The latest observation time in Unix milliseconds.
    pub last_observed_at_ms: u64,
    /// The number of coalesced observations in this one reason.
    pub coalesced_count: u64,
}

impl HarnessTriggerReasonV1 {
    /// Encodes this trigger reason into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` when the observation window is
    /// reversed or the coalesced count is zero, and other `CanonicalError`
    /// values only for impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        if self.last_observed_at_ms < self.first_observed_at_ms || self.coalesced_count == 0 {
            return Err(CanonicalError::InvalidField);
        }
        record(
            0,
            1,
            vec![
                (1, WireType::Uuid, self.reason_id.to_vec()),
                (2, WireType::U64, self.source_kind.enc()),
                (3, WireType::U64, encode_u64(self.first_observed_at_ms)),
                (4, WireType::U64, encode_u64(self.last_observed_at_ms)),
                (5, WireType::U64, encode_u64(self.coalesced_count)),
            ],
        )
    }

    /// Decodes this trigger reason from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown version,
    /// `CanonicalError::InvalidField` for an absent or malformed field, and
    /// other `CanonicalError` values for malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 5)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let reason = Self {
            reason_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            source_kind: HarnessSourceKindV1::dec(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            first_observed_at_ms: decode_u64(
                reader
                    .field(3, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            last_observed_at_ms: decode_u64(
                reader
                    .field(4, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            coalesced_count: decode_u64(
                reader
                    .field(5, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        if reason.last_observed_at_ms < reason.first_observed_at_ms || reason.coalesced_count == 0 {
            return Err(CanonicalError::InvalidField);
        }
        Ok(reason)
    }
}

/// The resolved harness execution class and narrowed tool subset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessClassResolutionV1 {
    /// The closed inherited class selection.
    pub class: HarnessExecutionClassV1,
    /// The registered tool identifiers the class permits, in registry order.
    pub narrowed_tool_ids: Vec<String>,
}

impl HarnessClassResolutionV1 {
    /// Encodes this class resolution into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` when a tool identifier is
    /// invalid and `CanonicalError::CredentialsForbidden` for a
    /// credential-shaped value; an over-limit list is rejected with
    /// `CanonicalError::OverLimit`.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        if self.narrowed_tool_ids.len() > HARNESS_MAX_NARROWED_TOOLS {
            return Err(CanonicalError::OverLimit);
        }
        for tool_id in &self.narrowed_tool_ids {
            validate_text(tool_id)?;
        }
        record(
            0,
            1,
            vec![
                (1, WireType::U64, self.class.enc()),
                (2, WireType::List, encode_utf8_list(&self.narrowed_tool_ids)),
            ],
        )
    }

    /// Decodes this class resolution from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown version,
    /// `CanonicalError::InvalidField` for an absent or malformed field,
    /// `CanonicalError::OverLimit` for an over-limit tool list, and other
    /// `CanonicalError` values for malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 2)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let class = HarnessExecutionClassV1::dec(
            reader
                .field(1, WireType::U64)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        let narrowed_tool_ids = decode_utf8_list(
            reader
                .field(2, WireType::List)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        if narrowed_tool_ids.len() > HARNESS_MAX_NARROWED_TOOLS {
            return Err(CanonicalError::OverLimit);
        }
        for tool_id in &narrowed_tool_ids {
            validate_text(tool_id)?;
        }
        Ok(Self {
            class,
            narrowed_tool_ids,
        })
    }
}

/// The immutable harness bounds recorded in one selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HarnessSelectionBoundsV1 {
    /// The maximum completion-cause chain depth.
    pub max_cause_depth: u64,
    /// The maximum concurrent non-terminal launches in the subtree.
    pub max_concurrent: u64,
    /// The maximum total launches from one original cause.
    pub max_total_launches: u64,
    /// The dossier byte bound.
    pub dossier_bytes: u64,
    /// The verified checkpoint byte bound.
    pub checkpoint_bytes: u64,
    /// The safe conclusion byte bound.
    pub conclusion_bytes: u64,
}

impl HarnessSelectionBoundsV1 {
    /// Encodes these bounds into their nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::OverLimit` when a bound is zero or exceeds
    /// the code-owned harness maximum and other `CanonicalError` values only
    /// for impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        if self.max_cause_depth == 0
            || self.max_cause_depth > HARNESS_MAX_CAUSE_DEPTH
            || self.max_concurrent == 0
            || self.max_concurrent > HARNESS_MAX_CONCURRENT
            || self.max_total_launches == 0
            || self.max_total_launches > HARNESS_MAX_TOTAL_LAUNCHES
            || self.dossier_bytes == 0
            || self.dossier_bytes > HARNESS_MAX_DOSSIER_BYTES
            || self.checkpoint_bytes == 0
            || self.checkpoint_bytes > HARNESS_MAX_CHECKPOINT_BYTES
            || self.conclusion_bytes == 0
            || self.conclusion_bytes > HARNESS_MAX_CONCLUSION_BYTES
        {
            return Err(CanonicalError::OverLimit);
        }
        record(
            0,
            1,
            (1..=6)
                .zip([
                    self.max_cause_depth,
                    self.max_concurrent,
                    self.max_total_launches,
                    self.dossier_bytes,
                    self.checkpoint_bytes,
                    self.conclusion_bytes,
                ])
                .map(|(number, value)| (number, WireType::U64, encode_u64(value)))
                .collect(),
        )
    }

    /// Decodes these bounds from their nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown version,
    /// `CanonicalError::InvalidField` for an absent or malformed field,
    /// `CanonicalError::OverLimit` for an out-of-range bound, and other
    /// `CanonicalError` values for malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 6)?;
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
        let bounds = Self {
            max_cause_depth: values[0],
            max_concurrent: values[1],
            max_total_launches: values[2],
            dossier_bytes: values[3],
            checkpoint_bytes: values[4],
            conclusion_bytes: values[5],
        };
        if bounds.max_cause_depth == 0
            || bounds.max_cause_depth > HARNESS_MAX_CAUSE_DEPTH
            || bounds.max_concurrent == 0
            || bounds.max_concurrent > HARNESS_MAX_CONCURRENT
            || bounds.max_total_launches == 0
            || bounds.max_total_launches > HARNESS_MAX_TOTAL_LAUNCHES
            || bounds.dossier_bytes == 0
            || bounds.dossier_bytes > HARNESS_MAX_DOSSIER_BYTES
            || bounds.checkpoint_bytes == 0
            || bounds.checkpoint_bytes > HARNESS_MAX_CHECKPOINT_BYTES
            || bounds.conclusion_bytes == 0
            || bounds.conclusion_bytes > HARNESS_MAX_CONCLUSION_BYTES
        {
            return Err(CanonicalError::OverLimit);
        }
        Ok(bounds)
    }
}

/// The continual-harness selection nested in execution meaning slot 7.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinualHarnessSelectionV1 {
    /// The harness identity.
    pub harness_id: [u8; 16],
    /// The active immutable rule revision that admitted the launch.
    pub rule_revision: u64,
    /// The durable trigger reason.
    pub trigger_reason: HarnessTriggerReasonV1,
    /// The resolved class and narrowed tool subset.
    pub class_resolution: HarnessClassResolutionV1,
    /// The dossier digest built at admission.
    pub dossier_digest: Digest256,
    /// The verified checkpoint reference when one is present.
    pub checkpoint_reference: Option<[u8; 16]>,
    /// The applied project time zone.
    pub time_zone_application: String,
    /// The immutable harness bounds.
    pub bounds: HarnessSelectionBoundsV1,
}

impl ContinualHarnessSelectionV1 {
    /// Encodes this selection into its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a zero rule revision or an
    /// invalid time zone, `CanonicalError::CredentialsForbidden` for a
    /// credential-shaped value, `CanonicalError::OverLimit` when the record
    /// exceeds its byte bound, and other `CanonicalError` values for the
    /// nested component failures.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        if self.rule_revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        validate_text(&self.time_zone_application)?;
        let bytes = record(
            TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
            1,
            vec![
                (1, WireType::Uuid, self.harness_id.to_vec()),
                (2, WireType::U64, encode_u64(self.rule_revision)),
                (3, WireType::Record, self.trigger_reason.encode()?),
                (4, WireType::Record, self.class_resolution.encode()?),
                (5, WireType::Digest, self.dossier_digest.bytes().to_vec()),
                (
                    6,
                    WireType::Optional,
                    encode_optional_uuid(&self.checkpoint_reference),
                ),
                (7, WireType::Utf8, encode_utf8(&self.time_zone_application)),
                (8, WireType::Record, self.bounds.encode()?),
            ],
        )?;
        if bytes.len() > MAX_CONTINUAL_HARNESS_SELECTION_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        Ok(bytes)
    }

    /// Decodes this selection from its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the tag or version is not
    /// the continual-harness selection v1 table, `CanonicalError::InvalidField`
    /// for an absent or malformed field, and other `CanonicalError` values
    /// for malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        if bytes.len() > MAX_CONTINUAL_HARNESS_SELECTION_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        let reader = CanonicalRecordReader::new(bytes, 8)?;
        if reader.tag != TagRegistry::CONTINUAL_HARNESS_SELECTION_V1 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let time_zone_application = decode_utf8(
            reader
                .field(7, WireType::Utf8)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .to_owned();
        validate_text(&time_zone_application)?;
        let selection = Self {
            harness_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            rule_revision: decode_u64(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            trigger_reason: HarnessTriggerReasonV1::decode(
                reader
                    .field(3, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            class_resolution: HarnessClassResolutionV1::decode(
                reader
                    .field(4, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            dossier_digest: Digest256::from_bytes(
                reader
                    .field(5, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            checkpoint_reference: decode_optional_uuid(
                reader
                    .field(6, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            time_zone_application,
            bounds: HarnessSelectionBoundsV1::decode(
                reader
                    .field(8, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        if selection.rule_revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        Ok(selection)
    }
}

/// The Goal-run kind recorded in one goal-run selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalRunKindV1 {
    /// An ordinary run with exactly one leading Goal.
    GoalDirectedOrdinary,
    /// A verification-only run for one Goal.
    VerificationOnly,
}

impl GoalRunKindV1 {
    fn enc(self) -> Vec<u8> {
        vec![match self {
            Self::GoalDirectedOrdinary => 0,
            Self::VerificationOnly => 1,
        }]
    }

    fn dec(bytes: &[u8]) -> Result<Self, CanonicalError> {
        match bytes {
            [0] => Ok(Self::GoalDirectedOrdinary),
            [1] => Ok(Self::VerificationOnly),
            _ => Err(CanonicalError::InvalidField),
        }
    }
}

/// The Goal scope and explicit session-link provenance of one selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalScopeLinkProvenanceV1 {
    /// The project identity.
    pub project_id: [u8; 16],
    /// The owner session identity for a session Goal or explicit link.
    pub session_id: Option<[u8; 16]>,
    /// The explicit durable goal-to-session link identity when present.
    pub link_id: Option<[u8; 16]>,
}

impl GoalScopeLinkProvenanceV1 {
    /// Encodes this provenance into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` when a link identity is present
    /// without an owner session identity, and other `CanonicalError` values
    /// only for impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        if self.link_id.is_some() && self.session_id.is_none() {
            return Err(CanonicalError::InvalidField);
        }
        record(
            0,
            1,
            vec![
                (1, WireType::Uuid, self.project_id.to_vec()),
                (
                    2,
                    WireType::Optional,
                    encode_optional_uuid(&self.session_id),
                ),
                (3, WireType::Optional, encode_optional_uuid(&self.link_id)),
            ],
        )
    }

    /// Decodes this provenance from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown version,
    /// `CanonicalError::InvalidField` for an absent or malformed field or a
    /// link without an owner session, and other `CanonicalError` values for
    /// malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 3)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let provenance = Self {
            project_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            session_id: decode_optional_uuid(
                reader
                    .field(2, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            link_id: decode_optional_uuid(
                reader
                    .field(3, WireType::Optional)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        if provenance.link_id.is_some() && provenance.session_id.is_none() {
            return Err(CanonicalError::InvalidField);
        }
        Ok(provenance)
    }
}

/// One ordered parent Goal revision reference in the chain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalRevisionReferenceV1 {
    /// The Goal identity.
    pub goal_id: [u8; 16],
    /// The exact immutable revision.
    pub revision: u64,
}

impl GoalRevisionReferenceV1 {
    /// Encodes this reference into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a zero revision and other
    /// `CanonicalError` values only for impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        if self.revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        record(
            0,
            1,
            vec![
                (1, WireType::Uuid, self.goal_id.to_vec()),
                (2, WireType::U64, encode_u64(self.revision)),
            ],
        )
    }

    /// Decodes this reference from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown version and
    /// `CanonicalError::InvalidField` for an absent, malformed, or zero
    /// revision field.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 2)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let reference = Self {
            goal_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            revision: decode_u64(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        if reference.revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        Ok(reference)
    }
}

/// One selected gate or template revision reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalGateRevisionReferenceV1 {
    /// The gate or template identity.
    pub gate_reference: [u8; 16],
    /// The exact immutable revision.
    pub revision: u64,
}

impl GoalGateRevisionReferenceV1 {
    /// Encodes this reference into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a zero revision and other
    /// `CanonicalError` values only for impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        if self.revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        record(
            0,
            1,
            vec![
                (1, WireType::Uuid, self.gate_reference.to_vec()),
                (2, WireType::U64, encode_u64(self.revision)),
            ],
        )
    }

    /// Decodes this reference from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown version and
    /// `CanonicalError::InvalidField` for an absent, malformed, or zero
    /// revision field.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 2)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let reference = Self {
            gate_reference: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            revision: decode_u64(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        if reference.revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        Ok(reference)
    }
}

/// One selected memory, skill, or role card reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalCardReferenceV1 {
    /// The card record identity.
    pub card_reference: [u8; 16],
    /// The exact immutable revision.
    pub revision: u64,
}

impl GoalCardReferenceV1 {
    /// Encodes this reference into its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a zero revision and other
    /// `CanonicalError` values only for impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        if self.revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        record(
            0,
            1,
            vec![
                (1, WireType::Uuid, self.card_reference.to_vec()),
                (2, WireType::U64, encode_u64(self.revision)),
            ],
        )
    }

    /// Decodes this reference from its nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown version and
    /// `CanonicalError::InvalidField` for an absent, malformed, or zero
    /// revision field.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 2)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let reference = Self {
            card_reference: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            revision: decode_u64(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
        };
        if reference.revision == 0 {
            return Err(CanonicalError::InvalidField);
        }
        Ok(reference)
    }
}

/// The immutable Goal-run bounds recorded in one selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoalRunSelectionBoundsV1 {
    /// The active memory card bound.
    pub max_memory_cards: u64,
    /// The selected skill and role card bound.
    pub max_skill_role_cards: u64,
    /// The canonical target snapshot byte bound.
    pub target_snapshot_bytes: u64,
    /// The Goal context byte bound in one run.
    pub context_bytes: u64,
}

impl GoalRunSelectionBoundsV1 {
    /// Encodes these bounds into their nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::OverLimit` when a bound is zero or exceeds
    /// the code-owned Goal maximum and other `CanonicalError` values only for
    /// impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        if self.max_memory_cards == 0
            || self.max_memory_cards > GOAL_MAX_MEMORY_CARDS
            || self.max_skill_role_cards == 0
            || self.max_skill_role_cards > GOAL_MAX_SKILL_ROLE_CARDS
            || self.target_snapshot_bytes == 0
            || self.target_snapshot_bytes > GOAL_MAX_TARGET_SNAPSHOT_BYTES
            || self.context_bytes == 0
            || self.context_bytes > GOAL_MAX_CONTEXT_BYTES
        {
            return Err(CanonicalError::OverLimit);
        }
        record(
            0,
            1,
            (1..=4)
                .zip([
                    self.max_memory_cards,
                    self.max_skill_role_cards,
                    self.target_snapshot_bytes,
                    self.context_bytes,
                ])
                .map(|(number, value)| (number, WireType::U64, encode_u64(value)))
                .collect(),
        )
    }

    /// Decodes these bounds from their nested anonymous record.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` for an unknown version,
    /// `CanonicalError::InvalidField` for an absent or malformed field,
    /// `CanonicalError::OverLimit` for an out-of-range bound, and other
    /// `CanonicalError` values for malformed framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        let reader = CanonicalRecordReader::new(bytes, 4)?;
        if reader.tag != 0 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let values = (1..=4)
            .map(|number| {
                decode_u64(
                    reader
                        .field(number, WireType::U64)?
                        .ok_or(CanonicalError::InvalidField)?,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let bounds = Self {
            max_memory_cards: values[0],
            max_skill_role_cards: values[1],
            target_snapshot_bytes: values[2],
            context_bytes: values[3],
        };
        if bounds.max_memory_cards == 0
            || bounds.max_memory_cards > GOAL_MAX_MEMORY_CARDS
            || bounds.max_skill_role_cards == 0
            || bounds.max_skill_role_cards > GOAL_MAX_SKILL_ROLE_CARDS
            || bounds.target_snapshot_bytes == 0
            || bounds.target_snapshot_bytes > GOAL_MAX_TARGET_SNAPSHOT_BYTES
            || bounds.context_bytes == 0
            || bounds.context_bytes > GOAL_MAX_CONTEXT_BYTES
        {
            return Err(CanonicalError::OverLimit);
        }
        Ok(bounds)
    }
}

/// The goal-run selection nested in execution meaning slot 8.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalRunSelectionV1 {
    /// The leading Goal identity.
    pub leading_goal_id: [u8; 16],
    /// The exact immutable Goal revision.
    pub goal_revision: u64,
    /// The scope and explicit session-link provenance.
    pub scope_link_provenance: GoalScopeLinkProvenanceV1,
    /// The ordered parent revision chain.
    pub parent_revision_chain: Vec<GoalRevisionReferenceV1>,
    /// The effective obligatory-component references.
    pub obligatory_component_references: Vec<[u8; 16]>,
    /// The selected gate and template revisions.
    pub selected_gate_revisions: Vec<GoalGateRevisionReferenceV1>,
    /// The valid gate evidence references.
    pub valid_evidence_references: Vec<[u8; 16]>,
    /// The selected memory cards.
    pub selected_memory_cards: Vec<GoalCardReferenceV1>,
    /// The selected Skill cards.
    pub selected_skill_cards: Vec<GoalCardReferenceV1>,
    /// The selected role cards.
    pub selected_role_cards: Vec<GoalCardReferenceV1>,
    /// The already-revealed full-record references.
    pub revealed_full_record_references: Vec<[u8; 16]>,
    /// The selected effective programmatic-caller policy snapshot reference.
    pub policy_snapshot_reference: [u8; 16],
    /// The selected agent-activity selection reference.
    pub activity_selection_reference: [u8; 16],
    /// The closed run kind.
    pub run_kind: GoalRunKindV1,
    /// The canonical target-snapshot digest.
    pub target_snapshot_digest: Digest256,
    /// The immutable Goal-run bounds.
    pub bounds: GoalRunSelectionBoundsV1,
}

impl GoalRunSelectionV1 {
    /// Encodes this selection into its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a zero revision or an
    /// invalid nested reference, `CanonicalError::OverLimit` when a list or a
    /// bound exceeds its code-owned limit or the record exceeds its byte
    /// bound, and other `CanonicalError` values for the nested component
    /// failures.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        if self.goal_revision == 0
            || self.parent_revision_chain.len() > GOAL_MAX_PARENT_CHAIN
            || self.obligatory_component_references.len() > GOAL_MAX_COMPONENT_REFERENCES
            || self.selected_gate_revisions.len() > GOAL_MAX_GATE_REVISIONS
            || self.valid_evidence_references.len() > GOAL_MAX_EVIDENCE_REFERENCES
            || self.revealed_full_record_references.len() > GOAL_MAX_REVEALED_RECORDS
            || u64::try_from(self.selected_memory_cards.len()).unwrap_or(u64::MAX)
                > self.bounds.max_memory_cards
            || u64::try_from(self.selected_skill_cards.len()).unwrap_or(u64::MAX)
                > self.bounds.max_skill_role_cards
            || u64::try_from(self.selected_role_cards.len()).unwrap_or(u64::MAX)
                > self.bounds.max_skill_role_cards
        {
            return Err(CanonicalError::OverLimit);
        }
        let parent_revision_chain = encode_list_items(
            &self
                .parent_revision_chain
                .iter()
                .map(GoalRevisionReferenceV1::encode)
                .collect::<Result<Vec<_>, _>>()?,
        );
        let selected_gate_revisions = encode_list_items(
            &self
                .selected_gate_revisions
                .iter()
                .map(GoalGateRevisionReferenceV1::encode)
                .collect::<Result<Vec<_>, _>>()?,
        );
        let selected_memory_cards = encode_list_items(
            &self
                .selected_memory_cards
                .iter()
                .map(GoalCardReferenceV1::encode)
                .collect::<Result<Vec<_>, _>>()?,
        );
        let selected_skill_cards = encode_list_items(
            &self
                .selected_skill_cards
                .iter()
                .map(GoalCardReferenceV1::encode)
                .collect::<Result<Vec<_>, _>>()?,
        );
        let selected_role_cards = encode_list_items(
            &self
                .selected_role_cards
                .iter()
                .map(GoalCardReferenceV1::encode)
                .collect::<Result<Vec<_>, _>>()?,
        );
        let bytes = record(
            TagRegistry::GOAL_RUN_SELECTION_V1,
            1,
            vec![
                (1, WireType::Uuid, self.leading_goal_id.to_vec()),
                (2, WireType::U64, encode_u64(self.goal_revision)),
                (3, WireType::Record, self.scope_link_provenance.encode()?),
                (4, WireType::List, parent_revision_chain),
                (
                    5,
                    WireType::List,
                    encode_uuid_list(&self.obligatory_component_references),
                ),
                (6, WireType::List, selected_gate_revisions),
                (
                    7,
                    WireType::List,
                    encode_uuid_list(&self.valid_evidence_references),
                ),
                (8, WireType::List, selected_memory_cards),
                (9, WireType::List, selected_skill_cards),
                (10, WireType::List, selected_role_cards),
                (
                    11,
                    WireType::List,
                    encode_uuid_list(&self.revealed_full_record_references),
                ),
                (12, WireType::Uuid, self.policy_snapshot_reference.to_vec()),
                (
                    13,
                    WireType::Uuid,
                    self.activity_selection_reference.to_vec(),
                ),
                (14, WireType::U64, self.run_kind.enc()),
                (
                    15,
                    WireType::Digest,
                    self.target_snapshot_digest.bytes().to_vec(),
                ),
                (16, WireType::Record, self.bounds.encode()?),
            ],
        )?;
        if bytes.len() > MAX_GOAL_RUN_SELECTION_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        Ok(bytes)
    }

    /// Decodes this selection from its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the tag or version is not
    /// the goal-run selection v1 table, `CanonicalError::InvalidField` for an
    /// absent or malformed field, `CanonicalError::OverLimit` for an
    /// over-limit list or bound, and other `CanonicalError` values for
    /// malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        if bytes.len() > MAX_GOAL_RUN_SELECTION_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        let reader = CanonicalRecordReader::new(bytes, 16)?;
        if reader.tag != TagRegistry::GOAL_RUN_SELECTION_V1 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let bounds = GoalRunSelectionBoundsV1::decode(
            reader
                .field(16, WireType::Record)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        let parent_revision_chain = decode_list_items(
            reader
                .field(4, WireType::List)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .into_iter()
        .map(|item| GoalRevisionReferenceV1::decode(&item))
        .collect::<Result<Vec<_>, _>>()?;
        let selected_gate_revisions = decode_list_items(
            reader
                .field(6, WireType::List)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .into_iter()
        .map(|item| GoalGateRevisionReferenceV1::decode(&item))
        .collect::<Result<Vec<_>, _>>()?;
        let selected_memory_cards = decode_list_items(
            reader
                .field(8, WireType::List)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .into_iter()
        .map(|item| GoalCardReferenceV1::decode(&item))
        .collect::<Result<Vec<_>, _>>()?;
        let selected_skill_cards = decode_list_items(
            reader
                .field(9, WireType::List)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .into_iter()
        .map(|item| GoalCardReferenceV1::decode(&item))
        .collect::<Result<Vec<_>, _>>()?;
        let selected_role_cards = decode_list_items(
            reader
                .field(10, WireType::List)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .into_iter()
        .map(|item| GoalCardReferenceV1::decode(&item))
        .collect::<Result<Vec<_>, _>>()?;
        let selection = Self {
            leading_goal_id: uuid(
                reader
                    .field(1, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            goal_revision: decode_u64(
                reader
                    .field(2, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            scope_link_provenance: GoalScopeLinkProvenanceV1::decode(
                reader
                    .field(3, WireType::Record)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            parent_revision_chain,
            obligatory_component_references: decode_uuid_list(
                reader
                    .field(5, WireType::List)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            selected_gate_revisions,
            valid_evidence_references: decode_uuid_list(
                reader
                    .field(7, WireType::List)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            selected_memory_cards,
            selected_skill_cards,
            selected_role_cards,
            revealed_full_record_references: decode_uuid_list(
                reader
                    .field(11, WireType::List)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            policy_snapshot_reference: uuid(
                reader
                    .field(12, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            activity_selection_reference: uuid(
                reader
                    .field(13, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            run_kind: GoalRunKindV1::dec(
                reader
                    .field(14, WireType::U64)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            target_snapshot_digest: Digest256::from_bytes(
                reader
                    .field(15, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            bounds,
        };
        if selection.goal_revision == 0
            || selection.parent_revision_chain.len() > GOAL_MAX_PARENT_CHAIN
            || selection.obligatory_component_references.len() > GOAL_MAX_COMPONENT_REFERENCES
            || selection.selected_gate_revisions.len() > GOAL_MAX_GATE_REVISIONS
            || selection.valid_evidence_references.len() > GOAL_MAX_EVIDENCE_REFERENCES
            || selection.revealed_full_record_references.len() > GOAL_MAX_REVEALED_RECORDS
            || u64::try_from(selection.selected_memory_cards.len()).unwrap_or(u64::MAX)
                > selection.bounds.max_memory_cards
            || u64::try_from(selection.selected_skill_cards.len()).unwrap_or(u64::MAX)
                > selection.bounds.max_skill_role_cards
            || u64::try_from(selection.selected_role_cards.len()).unwrap_or(u64::MAX)
                > selection.bounds.max_skill_role_cards
        {
            return Err(CanonicalError::OverLimit);
        }
        Ok(selection)
    }
}

/// The MCP method catalog selection nested in execution meaning slot 9.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpMethodCatalogSelectionV1 {
    /// The method catalog revision identity.
    pub method_catalog_revision: String,
    /// The selected MCP connection reference.
    pub connection_reference: [u8; 16],
    /// The discovered server revision digest.
    pub server_revision_digest: Digest256,
    /// The exact selected method reference.
    pub method_reference: String,
    /// The method schema revision identity.
    pub method_schema_revision: String,
    /// The supported typed input constraint family when one is selected.
    pub typed_input_constraint_family: Option<String>,
}

impl McpMethodCatalogSelectionV1 {
    /// Encodes this selection into its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidField` for a blank, over-long, or
    /// control-bearing value, `CanonicalError::CredentialsForbidden` for a
    /// credential-shaped value, `CanonicalError::OverLimit` when the record
    /// exceeds its byte bound, and other `CanonicalError` values only for
    /// impossible fixed-table framing faults.
    pub fn encode(&self) -> Result<Vec<u8>, CanonicalError> {
        validate_text(&self.method_catalog_revision)?;
        validate_text(&self.method_reference)?;
        validate_text(&self.method_schema_revision)?;
        if let Some(family) = &self.typed_input_constraint_family {
            validate_text(family)?;
        }
        let bytes = record(
            TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1,
            1,
            vec![
                (
                    1,
                    WireType::Utf8,
                    encode_utf8(&self.method_catalog_revision),
                ),
                (2, WireType::Uuid, self.connection_reference.to_vec()),
                (
                    3,
                    WireType::Digest,
                    self.server_revision_digest.bytes().to_vec(),
                ),
                (4, WireType::Utf8, encode_utf8(&self.method_reference)),
                (5, WireType::Utf8, encode_utf8(&self.method_schema_revision)),
                (
                    6,
                    WireType::Optional,
                    encode_optional_utf8(&self.typed_input_constraint_family),
                ),
            ],
        )?;
        if bytes.len() > MAX_MCP_METHOD_CATALOG_SELECTION_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        Ok(bytes)
    }

    /// Decodes this selection from its canonical record bytes.
    ///
    /// # Errors
    ///
    /// Returns `CanonicalError::InvalidTag` when the tag or version is not
    /// the MCP method catalog selection v1 table, `CanonicalError::InvalidField`
    /// for an absent or malformed field, and other `CanonicalError` values
    /// for malformed or noncanonical framing.
    pub fn decode(bytes: &[u8]) -> Result<Self, CanonicalError> {
        if bytes.len() > MAX_MCP_METHOD_CATALOG_SELECTION_BYTES {
            return Err(CanonicalError::OverLimit);
        }
        let reader = CanonicalRecordReader::new(bytes, 6)?;
        if reader.tag != TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1 || reader.version != 1 {
            return Err(CanonicalError::InvalidTag);
        }
        let method_catalog_revision = decode_utf8(
            reader
                .field(1, WireType::Utf8)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .to_owned();
        let method_reference = decode_utf8(
            reader
                .field(4, WireType::Utf8)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .to_owned();
        let method_schema_revision = decode_utf8(
            reader
                .field(5, WireType::Utf8)?
                .ok_or(CanonicalError::InvalidField)?,
        )?
        .to_owned();
        let typed_input_constraint_family = decode_optional_utf8(
            reader
                .field(6, WireType::Optional)?
                .ok_or(CanonicalError::InvalidField)?,
        )?;
        validate_text(&method_catalog_revision)?;
        validate_text(&method_reference)?;
        validate_text(&method_schema_revision)?;
        if let Some(family) = &typed_input_constraint_family {
            validate_text(family)?;
        }
        Ok(Self {
            method_catalog_revision,
            connection_reference: uuid(
                reader
                    .field(2, WireType::Uuid)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            server_revision_digest: Digest256::from_bytes(
                reader
                    .field(3, WireType::Digest)?
                    .ok_or(CanonicalError::InvalidField)?,
            )?,
            method_reference,
            method_schema_revision,
            typed_input_constraint_family,
        })
    }
}

impl DisabledOr<ContinualHarnessSelectionV1> {
    /// Encodes the optional selection: the one-byte closed/open presence
    /// marker followed by the nested record's canonical bytes when selected.
    ///
    /// # Errors
    ///
    /// Returns a `CanonicalError` only if the nested selection cannot encode.
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
            1 => Ok(Self::Selected(ContinualHarnessSelectionV1::decode(nested)?)),
            _ => Err(CanonicalError::InvalidOptional),
        }
    }
}

impl DisabledOr<GoalRunSelectionV1> {
    /// Encodes the optional selection: the one-byte closed/open presence
    /// marker followed by the nested record's canonical bytes when selected.
    ///
    /// # Errors
    ///
    /// Returns a `CanonicalError` only if the nested selection cannot encode.
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
            1 => Ok(Self::Selected(GoalRunSelectionV1::decode(nested)?)),
            _ => Err(CanonicalError::InvalidOptional),
        }
    }
}

impl DisabledOr<McpMethodCatalogSelectionV1> {
    /// Encodes the optional selection: the one-byte closed/open presence
    /// marker followed by the nested record's canonical bytes when selected.
    ///
    /// # Errors
    ///
    /// Returns a `CanonicalError` only if the nested selection cannot encode.
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
            1 => Ok(Self::Selected(McpMethodCatalogSelectionV1::decode(nested)?)),
            _ => Err(CanonicalError::InvalidOptional),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;

    fn harness_selection() -> ContinualHarnessSelectionV1 {
        ContinualHarnessSelectionV1 {
            harness_id: [1; 16],
            rule_revision: 3,
            trigger_reason: HarnessTriggerReasonV1 {
                reason_id: [2; 16],
                source_kind: HarnessSourceKindV1::FixedInterval,
                first_observed_at_ms: 1_000,
                last_observed_at_ms: 2_000,
                coalesced_count: 2,
            },
            class_resolution: HarnessClassResolutionV1 {
                class: HarnessExecutionClassV1::Medium,
                narrowed_tool_ids: vec!["read".to_owned(), "glob".to_owned()],
            },
            dossier_digest: Digest256::sha256(b"dossier"),
            checkpoint_reference: Some([3; 16]),
            time_zone_application: "Europe/Berlin".to_owned(),
            bounds: HarnessSelectionBoundsV1 {
                max_cause_depth: HARNESS_MAX_CAUSE_DEPTH,
                max_concurrent: HARNESS_MAX_CONCURRENT,
                max_total_launches: HARNESS_MAX_TOTAL_LAUNCHES,
                dossier_bytes: HARNESS_MAX_DOSSIER_BYTES,
                checkpoint_bytes: HARNESS_MAX_CHECKPOINT_BYTES,
                conclusion_bytes: HARNESS_MAX_CONCLUSION_BYTES,
            },
        }
    }

    fn goal_selection() -> GoalRunSelectionV1 {
        GoalRunSelectionV1 {
            leading_goal_id: [4; 16],
            goal_revision: 2,
            scope_link_provenance: GoalScopeLinkProvenanceV1 {
                project_id: [5; 16],
                session_id: Some([6; 16]),
                link_id: Some([7; 16]),
            },
            parent_revision_chain: vec![GoalRevisionReferenceV1 {
                goal_id: [8; 16],
                revision: 1,
            }],
            obligatory_component_references: vec![[9; 16]],
            selected_gate_revisions: vec![GoalGateRevisionReferenceV1 {
                gate_reference: [10; 16],
                revision: 1,
            }],
            valid_evidence_references: vec![[11; 16]],
            selected_memory_cards: vec![GoalCardReferenceV1 {
                card_reference: [12; 16],
                revision: 1,
            }],
            selected_skill_cards: Vec::new(),
            selected_role_cards: Vec::new(),
            revealed_full_record_references: vec![[13; 16]],
            policy_snapshot_reference: [14; 16],
            activity_selection_reference: [15; 16],
            run_kind: GoalRunKindV1::GoalDirectedOrdinary,
            target_snapshot_digest: Digest256::sha256(b"target"),
            bounds: GoalRunSelectionBoundsV1 {
                max_memory_cards: GOAL_MAX_MEMORY_CARDS,
                max_skill_role_cards: GOAL_MAX_SKILL_ROLE_CARDS,
                target_snapshot_bytes: GOAL_MAX_TARGET_SNAPSHOT_BYTES,
                context_bytes: GOAL_MAX_CONTEXT_BYTES,
            },
        }
    }

    fn mcp_selection() -> McpMethodCatalogSelectionV1 {
        McpMethodCatalogSelectionV1 {
            method_catalog_revision: "mcp-catalog-rev-1".to_owned(),
            connection_reference: [16; 16],
            server_revision_digest: Digest256::sha256(b"server"),
            method_reference: "tools/search".to_owned(),
            method_schema_revision: "schema-rev-1".to_owned(),
            typed_input_constraint_family: Some("closed_query_v1".to_owned()),
        }
    }

    #[test]
    fn root_origin_round_trips_both_closed_roots() {
        for origin in [
            ProgrammaticCallerRootOriginV1::InteractiveUser {
                originating_turn_id: [1; 16],
            },
            ProgrammaticCallerRootOriginV1::ContinualHarness {
                harness_id: [2; 16],
                rule_revision: 7,
                trigger_reason_id: [3; 16],
            },
        ] {
            let encoded = origin.encode().expect("root origin encodes");
            assert_eq!(
                ProgrammaticCallerRootOriginV1::decode(&encoded).expect("root origin decodes"),
                origin
            );
        }
    }

    #[test]
    fn harness_selection_round_trips_and_rejects_credentials() {
        let selection = harness_selection();
        let encoded = selection.encode().expect("harness selection encodes");
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&encoded).expect("harness selection decodes"),
            selection
        );
        let mut invalid = harness_selection();
        invalid.class_resolution.narrowed_tool_ids = vec!["api_key=secret-value".to_owned()];
        assert_eq!(
            invalid
                .encode()
                .expect_err("credential-shaped tool id rejects"),
            CanonicalError::CredentialsForbidden
        );
        let mut oversized = harness_selection();
        oversized.bounds.max_total_launches = HARNESS_MAX_TOTAL_LAUNCHES + 1;
        assert_eq!(
            oversized.encode().expect_err("over-limit bound rejects"),
            CanonicalError::OverLimit
        );
    }

    #[test]
    fn goal_selection_round_trips_and_enforces_bounds() {
        let selection = goal_selection();
        let encoded = selection.encode().expect("goal selection encodes");
        assert_eq!(
            GoalRunSelectionV1::decode(&encoded).expect("goal selection decodes"),
            selection
        );
        let mut over_limit = goal_selection();
        over_limit.parent_revision_chain = (0..=GOAL_MAX_PARENT_CHAIN as u8)
            .map(|index| GoalRevisionReferenceV1 {
                goal_id: [index; 16],
                revision: 1,
            })
            .collect();
        assert_eq!(
            over_limit
                .encode()
                .expect_err("over-limit parent chain rejects"),
            CanonicalError::OverLimit
        );
        let mut link_without_session = goal_selection();
        link_without_session.scope_link_provenance.session_id = None;
        assert_eq!(
            link_without_session
                .encode()
                .expect_err("link without session rejects"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn mcp_selection_round_trips_and_rejects_blank_methods() {
        let selection = mcp_selection();
        let encoded = selection.encode().expect("mcp selection encodes");
        assert_eq!(
            McpMethodCatalogSelectionV1::decode(&encoded).expect("mcp selection decodes"),
            selection
        );
        let mut blank = mcp_selection();
        blank.method_reference = "  ".to_owned();
        assert_eq!(
            blank.encode().expect_err("blank method reference rejects"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn disabled_or_marks_each_selection_slot() {
        let disabled = DisabledOr::<GoalRunSelectionV1>::Disabled;
        let encoded = disabled.encode().expect("disabled marker encodes");
        assert_eq!(encoded, vec![0]);
        assert_eq!(
            DisabledOr::<GoalRunSelectionV1>::decode(&encoded).expect("disabled marker decodes"),
            DisabledOr::Disabled
        );
        let selected = DisabledOr::Selected(goal_selection());
        let encoded = selected.encode().expect("selected selection encodes");
        assert_eq!(
            DisabledOr::<GoalRunSelectionV1>::decode(&encoded).expect("selected selection decodes"),
            selected
        );
    }

    /// Renders a raw canonical record for negative framing fixtures.
    fn raw_record(tag: u32, version: u32, fields: &[(u32, u8, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"IRCR");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&version.to_be_bytes());
        for (number, wire_type, value) in fields {
            out.extend_from_slice(&number.to_be_bytes());
            out.push(*wire_type);
            out.extend_from_slice(
                &u32::try_from(value.len())
                    .expect("the fixture field length fits a u32")
                    .to_be_bytes(),
            );
            out.extend_from_slice(value);
        }
        out
    }

    fn raw_field(number: u32, wire_type: WireType, value: &[u8]) -> (u32, u8, Vec<u8>) {
        (number, wire_type as u8, value.to_vec())
    }

    fn set_raw_field(fields: &mut [(u32, u8, Vec<u8>)], number: u32, value: Vec<u8>) {
        let field = fields
            .iter_mut()
            .find(|field| field.0 == number)
            .expect("the fixture field exists");
        field.2 = value;
    }

    fn trigger_reason_fields(reason: &HarnessTriggerReasonV1) -> Vec<(u32, u8, Vec<u8>)> {
        vec![
            raw_field(1, WireType::Uuid, &reason.reason_id),
            raw_field(2, WireType::U64, &reason.source_kind.enc()),
            raw_field(3, WireType::U64, &encode_u64(reason.first_observed_at_ms)),
            raw_field(4, WireType::U64, &encode_u64(reason.last_observed_at_ms)),
            raw_field(5, WireType::U64, &encode_u64(reason.coalesced_count)),
        ]
    }

    fn harness_bounds_fields(bounds: &HarnessSelectionBoundsV1) -> Vec<(u32, u8, Vec<u8>)> {
        vec![
            raw_field(1, WireType::U64, &encode_u64(bounds.max_cause_depth)),
            raw_field(2, WireType::U64, &encode_u64(bounds.max_concurrent)),
            raw_field(3, WireType::U64, &encode_u64(bounds.max_total_launches)),
            raw_field(4, WireType::U64, &encode_u64(bounds.dossier_bytes)),
            raw_field(5, WireType::U64, &encode_u64(bounds.checkpoint_bytes)),
            raw_field(6, WireType::U64, &encode_u64(bounds.conclusion_bytes)),
        ]
    }

    fn harness_selection_fields(
        selection: &ContinualHarnessSelectionV1,
    ) -> Vec<(u32, u8, Vec<u8>)> {
        vec![
            raw_field(1, WireType::Uuid, &selection.harness_id),
            raw_field(2, WireType::U64, &encode_u64(selection.rule_revision)),
            raw_field(
                3,
                WireType::Record,
                &selection
                    .trigger_reason
                    .encode()
                    .expect("the nested trigger reason encodes"),
            ),
            raw_field(
                4,
                WireType::Record,
                &selection
                    .class_resolution
                    .encode()
                    .expect("the nested class resolution encodes"),
            ),
            raw_field(5, WireType::Digest, &selection.dossier_digest.bytes()),
            raw_field(
                6,
                WireType::Optional,
                &encode_optional_uuid(&selection.checkpoint_reference),
            ),
            raw_field(
                7,
                WireType::Utf8,
                &encode_utf8(&selection.time_zone_application),
            ),
            raw_field(
                8,
                WireType::Record,
                &selection.bounds.encode().expect("the nested bounds encode"),
            ),
        ]
    }

    fn goal_bounds_fields(bounds: &GoalRunSelectionBoundsV1) -> Vec<(u32, u8, Vec<u8>)> {
        vec![
            raw_field(1, WireType::U64, &encode_u64(bounds.max_memory_cards)),
            raw_field(2, WireType::U64, &encode_u64(bounds.max_skill_role_cards)),
            raw_field(3, WireType::U64, &encode_u64(bounds.target_snapshot_bytes)),
            raw_field(4, WireType::U64, &encode_u64(bounds.context_bytes)),
        ]
    }

    fn goal_selection_fields(selection: &GoalRunSelectionV1) -> Vec<(u32, u8, Vec<u8>)> {
        vec![
            raw_field(1, WireType::Uuid, &selection.leading_goal_id),
            raw_field(2, WireType::U64, &encode_u64(selection.goal_revision)),
            raw_field(
                3,
                WireType::Record,
                &selection
                    .scope_link_provenance
                    .encode()
                    .expect("the nested provenance encodes"),
            ),
            raw_field(
                4,
                WireType::List,
                &encode_list_items(
                    &selection
                        .parent_revision_chain
                        .iter()
                        .map(|reference| {
                            reference
                                .encode()
                                .expect("the nested chain reference encodes")
                        })
                        .collect::<Vec<_>>(),
                ),
            ),
            raw_field(
                5,
                WireType::List,
                &encode_uuid_list(&selection.obligatory_component_references),
            ),
            raw_field(
                6,
                WireType::List,
                &encode_list_items(
                    &selection
                        .selected_gate_revisions
                        .iter()
                        .map(|reference| {
                            reference
                                .encode()
                                .expect("the nested gate reference encodes")
                        })
                        .collect::<Vec<_>>(),
                ),
            ),
            raw_field(
                7,
                WireType::List,
                &encode_uuid_list(&selection.valid_evidence_references),
            ),
            raw_field(
                8,
                WireType::List,
                &encode_list_items(
                    &selection
                        .selected_memory_cards
                        .iter()
                        .map(|reference| {
                            reference.encode().expect("the nested memory card encodes")
                        })
                        .collect::<Vec<_>>(),
                ),
            ),
            raw_field(
                9,
                WireType::List,
                &encode_list_items(
                    &selection
                        .selected_skill_cards
                        .iter()
                        .map(|reference| reference.encode().expect("the nested Skill card encodes"))
                        .collect::<Vec<_>>(),
                ),
            ),
            raw_field(
                10,
                WireType::List,
                &encode_list_items(
                    &selection
                        .selected_role_cards
                        .iter()
                        .map(|reference| reference.encode().expect("the nested role card encodes"))
                        .collect::<Vec<_>>(),
                ),
            ),
            raw_field(
                11,
                WireType::List,
                &encode_uuid_list(&selection.revealed_full_record_references),
            ),
            raw_field(12, WireType::Uuid, &selection.policy_snapshot_reference),
            raw_field(13, WireType::Uuid, &selection.activity_selection_reference),
            raw_field(14, WireType::U64, &selection.run_kind.enc()),
            raw_field(
                15,
                WireType::Digest,
                &selection.target_snapshot_digest.bytes(),
            ),
            raw_field(
                16,
                WireType::Record,
                &selection.bounds.encode().expect("the nested bounds encode"),
            ),
        ]
    }

    #[test]
    fn optional_uuid_framing_rejects_unknown_markers_and_wrong_lengths() {
        assert_eq!(
            decode_optional_uuid(&[]),
            Err(CanonicalError::InvalidOptional)
        );
        assert_eq!(decode_optional_uuid(&[0]), Ok(None));
        assert_eq!(
            decode_optional_uuid(&[0, 0]),
            Err(CanonicalError::InvalidOptional)
        );
        assert_eq!(
            decode_optional_uuid(&[1]),
            Err(CanonicalError::InvalidOptional)
        );
        assert_eq!(
            decode_optional_uuid(&[1, 0, 1]),
            Err(CanonicalError::InvalidOptional)
        );
        assert_eq!(
            decode_optional_uuid(&[2]),
            Err(CanonicalError::InvalidOptional)
        );
        assert_eq!(decode_optional_uuid(&[1; 17]), Ok(Some([1; 16])));
        assert_eq!(encode_optional_uuid(&None), vec![0]);
        assert_eq!(
            encode_optional_uuid(&Some([2; 16])),
            [vec![1], vec![2; 16]].concat()
        );
    }

    #[test]
    fn root_origin_decode_rejects_unknown_tags_and_malformed_fields() {
        assert_eq!(
            ProgrammaticCallerRootOriginV1::decode(&raw_record(
                1,
                1,
                &[raw_field(1, WireType::Uuid, &[1; 16])]
            ))
            .expect_err("unknown root origin tag"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            ProgrammaticCallerRootOriginV1::decode(&raw_record(0, 3, &[]))
                .expect_err("unknown root origin version"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            ProgrammaticCallerRootOriginV1::decode(&raw_record(
                0,
                1,
                &[raw_field(1, WireType::Uuid, &[1; 8])]
            ))
            .expect_err("malformed interactive root turn identity"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            ProgrammaticCallerRootOriginV1::decode(&raw_record(0, 1, &[]))
                .expect_err("absent interactive root turn identity"),
            CanonicalError::InvalidField
        );
        let harness_fields = |harness: Vec<u8>, revision: Vec<u8>, reason: Vec<u8>| {
            vec![
                raw_field(1, WireType::Uuid, &harness),
                raw_field(2, WireType::U64, &revision),
                raw_field(3, WireType::Uuid, &reason),
            ]
        };
        assert_eq!(
            ProgrammaticCallerRootOriginV1::decode(&raw_record(
                0,
                2,
                &harness_fields(vec![1; 8], vec![1], vec![3; 16])
            ))
            .expect_err("malformed harness identity"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            ProgrammaticCallerRootOriginV1::decode(&raw_record(
                0,
                2,
                &harness_fields(vec![2; 16], vec![0, 1], vec![3; 16])
            ))
            .expect_err("non-minimal harness rule revision"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            ProgrammaticCallerRootOriginV1::decode(&raw_record(
                0,
                2,
                &harness_fields(vec![2; 16], vec![1], vec![3; 8])
            ))
            .expect_err("malformed harness trigger reason identity"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            ProgrammaticCallerRootOriginV1::decode(&raw_record(
                0,
                2,
                &harness_fields(vec![2; 16], vec![1], vec![3; 16])
            ))
            .expect("the harness root origin decodes"),
            ProgrammaticCallerRootOriginV1::ContinualHarness {
                harness_id: [2; 16],
                rule_revision: 1,
                trigger_reason_id: [3; 16],
            }
        );
    }

    #[test]
    fn harness_source_and_execution_class_codecs_cover_every_variant() {
        for (kind, first) in [
            (HarnessSourceKindV1::ExplicitUserLaunch, 0u8),
            (HarnessSourceKindV1::CalendarTime, 1),
            (HarnessSourceKindV1::FixedInterval, 2),
            (HarnessSourceKindV1::TerminalOutcomeLink, 3),
        ] {
            assert_eq!(kind.enc(), vec![first]);
            assert_eq!(HarnessSourceKindV1::dec(&[first]), Ok(kind));
        }
        assert_eq!(
            HarnessSourceKindV1::dec(&[4]),
            Err(CanonicalError::InvalidField)
        );
        assert_eq!(
            HarnessSourceKindV1::dec(&[]),
            Err(CanonicalError::InvalidField)
        );
        for (class, first) in [
            (HarnessExecutionClassV1::Light, 0u8),
            (HarnessExecutionClassV1::Medium, 1),
            (HarnessExecutionClassV1::Heavy, 2),
        ] {
            assert_eq!(class.enc(), vec![first]);
            assert_eq!(HarnessExecutionClassV1::dec(&[first]), Ok(class));
        }
        assert_eq!(
            HarnessExecutionClassV1::dec(&[3]),
            Err(CanonicalError::InvalidField)
        );
    }

    #[test]
    fn trigger_reason_codec_rejects_invalid_windows_and_wrong_fields() {
        let reason = harness_selection().trigger_reason;
        let encoded = reason.encode().expect("the trigger reason fixture encodes");
        assert_eq!(
            HarnessTriggerReasonV1::decode(&encoded).expect("the trigger reason fixture decodes"),
            reason
        );
        assert_eq!(
            HarnessTriggerReasonV1 {
                last_observed_at_ms: 999,
                ..reason
            }
            .encode()
            .expect_err("reversed observation window"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            HarnessTriggerReasonV1 {
                coalesced_count: 0,
                ..reason
            }
            .encode()
            .expect_err("zero coalesced count"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            HarnessTriggerReasonV1::decode(&raw_record(1, 1, &[]))
                .expect_err("unknown trigger reason tag"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            HarnessTriggerReasonV1::decode(&raw_record(0, 2, &[]))
                .expect_err("unknown trigger reason version"),
            CanonicalError::InvalidTag
        );
        let mut fields = trigger_reason_fields(&reason);
        set_raw_field(&mut fields, 1, vec![1; 8]);
        assert_eq!(
            HarnessTriggerReasonV1::decode(&raw_record(0, 1, &fields))
                .expect_err("malformed trigger reason identity"),
            CanonicalError::InvalidField
        );
        let mut fields = trigger_reason_fields(&reason);
        set_raw_field(&mut fields, 2, vec![7]);
        assert_eq!(
            HarnessTriggerReasonV1::decode(&raw_record(0, 1, &fields))
                .expect_err("unknown trigger source kind"),
            CanonicalError::InvalidField
        );
        let mut fields = trigger_reason_fields(&reason);
        set_raw_field(&mut fields, 3, vec![0, 1]);
        assert_eq!(
            HarnessTriggerReasonV1::decode(&raw_record(0, 1, &fields))
                .expect_err("non-minimal first observation"),
            CanonicalError::InvalidField
        );
        let mut fields = trigger_reason_fields(&reason);
        set_raw_field(&mut fields, 4, vec![0, 1]);
        assert_eq!(
            HarnessTriggerReasonV1::decode(&raw_record(0, 1, &fields))
                .expect_err("non-minimal last observation"),
            CanonicalError::InvalidField
        );
        let mut fields = trigger_reason_fields(&reason);
        set_raw_field(&mut fields, 5, vec![0, 1]);
        assert_eq!(
            HarnessTriggerReasonV1::decode(&raw_record(0, 1, &fields))
                .expect_err("non-minimal coalesced count"),
            CanonicalError::InvalidField
        );
        let reversed = HarnessTriggerReasonV1 {
            first_observed_at_ms: 5_000,
            last_observed_at_ms: 4_000,
            ..reason
        };
        assert_eq!(
            HarnessTriggerReasonV1::decode(&raw_record(0, 1, &trigger_reason_fields(&reversed)))
                .expect_err("decoded reversed observation window"),
            CanonicalError::InvalidField
        );
        let zero_count = HarnessTriggerReasonV1 {
            coalesced_count: 0,
            ..reason
        };
        assert_eq!(
            HarnessTriggerReasonV1::decode(&raw_record(0, 1, &trigger_reason_fields(&zero_count)))
                .expect_err("decoded zero coalesced count"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            HarnessTriggerReasonV1::decode(&raw_record(
                0,
                1,
                &[raw_field(1, WireType::Uuid, &[2; 16])]
            ))
            .expect_err("absent trigger reason fields"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn class_resolution_codec_enforces_the_tool_bound_and_field_types() {
        let resolution = harness_selection().class_resolution;
        let encoded = resolution
            .encode()
            .expect("the class resolution fixture encodes");
        assert_eq!(
            HarnessClassResolutionV1::decode(&encoded)
                .expect("the class resolution fixture decodes"),
            resolution
        );
        assert_eq!(
            HarnessClassResolutionV1 {
                class: HarnessExecutionClassV1::Heavy,
                narrowed_tool_ids: vec!["read".to_owned(); HARNESS_MAX_NARROWED_TOOLS + 1],
            }
            .encode()
            .expect_err("over-bound encoded tool list"),
            CanonicalError::OverLimit
        );
        assert_eq!(
            HarnessClassResolutionV1::decode(&raw_record(1, 1, &[]))
                .expect_err("unknown class resolution tag"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            HarnessClassResolutionV1::decode(&raw_record(0, 2, &[]))
                .expect_err("unknown class resolution version"),
            CanonicalError::InvalidTag
        );
        let class_fields = |class: u8, tools: Vec<u8>| {
            vec![
                raw_field(1, WireType::U64, &[class]),
                raw_field(2, WireType::List, &tools),
            ]
        };
        assert_eq!(
            HarnessClassResolutionV1::decode(&raw_record(
                0,
                1,
                &class_fields(9, encode_utf8_list(&[]))
            ))
            .expect_err("unknown execution class"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            HarnessClassResolutionV1::decode(&raw_record(
                0,
                1,
                &class_fields(0, vec![0xFF, 0xFF, 0xFF, 0xFF])
            ))
            .expect_err("over-limit declared tool count"),
            CanonicalError::OverLimit
        );
        assert_eq!(
            HarnessClassResolutionV1::decode(&raw_record(
                0,
                1,
                &class_fields(
                    0,
                    encode_utf8_list(&vec!["read".to_owned(); HARNESS_MAX_NARROWED_TOOLS + 1])
                )
            ))
            .expect_err("over-bound decoded tool list"),
            CanonicalError::OverLimit
        );
        assert_eq!(
            HarnessClassResolutionV1::decode(&raw_record(
                0,
                1,
                &class_fields(0, encode_utf8_list(&["  ".to_owned()]))
            ))
            .expect_err("blank decoded tool identifier"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn harness_bounds_codec_rejects_zero_and_over_limit_bounds() {
        let bounds = harness_selection().bounds;
        let encoded = bounds.encode().expect("the harness bounds fixture encodes");
        assert_eq!(
            HarnessSelectionBoundsV1::decode(&encoded).expect("the harness bounds fixture decodes"),
            bounds
        );
        for invalid in [
            HarnessSelectionBoundsV1 {
                max_cause_depth: 0,
                ..bounds
            },
            HarnessSelectionBoundsV1 {
                max_cause_depth: HARNESS_MAX_CAUSE_DEPTH + 1,
                ..bounds
            },
            HarnessSelectionBoundsV1 {
                max_concurrent: 0,
                ..bounds
            },
            HarnessSelectionBoundsV1 {
                max_concurrent: HARNESS_MAX_CONCURRENT + 1,
                ..bounds
            },
            HarnessSelectionBoundsV1 {
                max_total_launches: 0,
                ..bounds
            },
            HarnessSelectionBoundsV1 {
                max_total_launches: HARNESS_MAX_TOTAL_LAUNCHES + 1,
                ..bounds
            },
            HarnessSelectionBoundsV1 {
                dossier_bytes: 0,
                ..bounds
            },
            HarnessSelectionBoundsV1 {
                dossier_bytes: HARNESS_MAX_DOSSIER_BYTES + 1,
                ..bounds
            },
            HarnessSelectionBoundsV1 {
                checkpoint_bytes: 0,
                ..bounds
            },
            HarnessSelectionBoundsV1 {
                checkpoint_bytes: HARNESS_MAX_CHECKPOINT_BYTES + 1,
                ..bounds
            },
            HarnessSelectionBoundsV1 {
                conclusion_bytes: 0,
                ..bounds
            },
            HarnessSelectionBoundsV1 {
                conclusion_bytes: HARNESS_MAX_CONCLUSION_BYTES + 1,
                ..bounds
            },
        ] {
            assert_eq!(
                invalid.encode().expect_err("out-of-range harness bounds"),
                CanonicalError::OverLimit
            );
            assert_eq!(
                HarnessSelectionBoundsV1::decode(&raw_record(
                    0,
                    1,
                    &harness_bounds_fields(&invalid)
                ))
                .expect_err("out-of-range decoded harness bounds"),
                CanonicalError::OverLimit
            );
        }
        assert_eq!(
            HarnessSelectionBoundsV1::decode(&raw_record(1, 1, &[]))
                .expect_err("unknown harness bounds tag"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            HarnessSelectionBoundsV1::decode(&raw_record(0, 2, &[]))
                .expect_err("unknown harness bounds version"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            HarnessSelectionBoundsV1::decode(&raw_record(
                0,
                1,
                &[raw_field(1, WireType::U64, &[1])]
            ))
            .expect_err("absent harness bounds fields"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn continual_harness_selection_encode_rejects_zero_revision_and_invalid_time_zone() {
        assert_eq!(
            ContinualHarnessSelectionV1 {
                rule_revision: 0,
                ..harness_selection()
            }
            .encode()
            .expect_err("zero harness rule revision"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            ContinualHarnessSelectionV1 {
                time_zone_application: "  ".to_owned(),
                ..harness_selection()
            }
            .encode()
            .expect_err("blank time zone application"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            ContinualHarnessSelectionV1 {
                time_zone_application: "token=abc".to_owned(),
                ..harness_selection()
            }
            .encode()
            .expect_err("credential-shaped time zone application"),
            CanonicalError::CredentialsForbidden
        );
        assert_eq!(
            ContinualHarnessSelectionV1 {
                time_zone_application: "x".repeat(MAX_SLICE3_TEXT_CHARS + 1),
                ..harness_selection()
            }
            .encode()
            .expect_err("over-long time zone application"),
            CanonicalError::InvalidField
        );
        assert!(
            ContinualHarnessSelectionV1 {
                time_zone_application: "x".repeat(MAX_SLICE3_TEXT_CHARS),
                ..harness_selection()
            }
            .encode()
            .is_ok()
        );
    }

    #[test]
    fn continual_harness_selection_decode_rejects_oversized_tag_and_malformed_fields() {
        let oversized = vec![0u8; MAX_CONTINUAL_HARNESS_SELECTION_BYTES + 1];
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&oversized)
                .expect_err("oversized harness selection"),
            CanonicalError::OverLimit
        );
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&raw_record(1, 1, &[]))
                .expect_err("unknown harness selection tag"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&raw_record(
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
                2,
                &[]
            ))
            .expect_err("unknown harness selection version"),
            CanonicalError::InvalidTag
        );
        let fixture = harness_selection();
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&raw_record(
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
                1,
                &harness_selection_fields(&fixture)
            ))
            .expect("the raw harness selection decodes"),
            fixture
        );
        let mut fields = harness_selection_fields(&fixture);
        set_raw_field(&mut fields, 7, b"  ".to_vec());
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&raw_record(
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
                1,
                &fields
            ))
            .expect_err("blank decoded time zone application"),
            CanonicalError::InvalidField
        );
        let mut fields = harness_selection_fields(&fixture);
        set_raw_field(&mut fields, 7, vec![0xFF, 0xFF]);
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&raw_record(
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
                1,
                &fields
            ))
            .expect_err("invalid UTF-8 time zone application"),
            CanonicalError::InvalidUtf8
        );
        let mut fields = harness_selection_fields(&fixture);
        set_raw_field(&mut fields, 1, vec![1; 4]);
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&raw_record(
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
                1,
                &fields
            ))
            .expect_err("malformed harness identity"),
            CanonicalError::InvalidField
        );
        let mut fields = harness_selection_fields(&fixture);
        set_raw_field(&mut fields, 2, vec![0, 1]);
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&raw_record(
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
                1,
                &fields
            ))
            .expect_err("non-minimal harness rule revision"),
            CanonicalError::InvalidField
        );
        let mut fields = harness_selection_fields(&fixture);
        set_raw_field(&mut fields, 3, raw_record(1, 1, &[]));
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&raw_record(
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
                1,
                &fields
            ))
            .expect_err("malformed nested trigger reason"),
            CanonicalError::InvalidTag
        );
        let mut fields = harness_selection_fields(&fixture);
        set_raw_field(&mut fields, 4, raw_record(1, 1, &[]));
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&raw_record(
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
                1,
                &fields
            ))
            .expect_err("malformed nested class resolution"),
            CanonicalError::InvalidTag
        );
        let mut fields = harness_selection_fields(&fixture);
        set_raw_field(&mut fields, 5, vec![1; 8]);
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&raw_record(
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
                1,
                &fields
            ))
            .expect_err("malformed dossier digest"),
            CanonicalError::InvalidDigest
        );
        let mut fields = harness_selection_fields(&fixture);
        set_raw_field(&mut fields, 6, vec![1, 1, 2, 3]);
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&raw_record(
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
                1,
                &fields
            ))
            .expect_err("malformed checkpoint optional"),
            CanonicalError::InvalidOptional
        );
        let mut fields = harness_selection_fields(&fixture);
        set_raw_field(&mut fields, 8, raw_record(0, 2, &[]));
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&raw_record(
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
                1,
                &fields
            ))
            .expect_err("malformed nested harness bounds"),
            CanonicalError::InvalidTag
        );
        let mut fields = harness_selection_fields(&fixture);
        set_raw_field(&mut fields, 2, vec![0]);
        assert_eq!(
            ContinualHarnessSelectionV1::decode(&raw_record(
                TagRegistry::CONTINUAL_HARNESS_SELECTION_V1,
                1,
                &fields
            ))
            .expect_err("zero decoded harness rule revision"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn goal_run_kind_codec_covers_both_kinds() {
        for (kind, value) in [
            (GoalRunKindV1::GoalDirectedOrdinary, 0u8),
            (GoalRunKindV1::VerificationOnly, 1),
        ] {
            assert_eq!(kind.enc(), vec![value]);
            assert_eq!(GoalRunKindV1::dec(&[value]), Ok(kind));
        }
        assert_eq!(GoalRunKindV1::dec(&[2]), Err(CanonicalError::InvalidField));
        assert_eq!(
            GoalRunKindV1::dec(&[0, 1]),
            Err(CanonicalError::InvalidField)
        );
    }

    #[test]
    fn scope_link_provenance_decode_rejects_malformed_fields_and_link_without_session() {
        let provenance = GoalScopeLinkProvenanceV1 {
            project_id: [5; 16],
            session_id: Some([6; 16]),
            link_id: Some([7; 16]),
        };
        let encoded = provenance.encode().expect("the provenance fixture encodes");
        assert_eq!(
            GoalScopeLinkProvenanceV1::decode(&encoded).expect("the provenance fixture decodes"),
            provenance
        );
        assert_eq!(
            GoalScopeLinkProvenanceV1 {
                project_id: [5; 16],
                session_id: None,
                link_id: Some([7; 16]),
            }
            .encode()
            .expect_err("link without an owner session"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            GoalScopeLinkProvenanceV1::decode(&raw_record(1, 1, &[]))
                .expect_err("unknown provenance tag"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            GoalScopeLinkProvenanceV1::decode(&raw_record(0, 2, &[]))
                .expect_err("unknown provenance version"),
            CanonicalError::InvalidTag
        );
        let provenance_fields = |project: Vec<u8>, session: Vec<u8>, link: Vec<u8>| {
            vec![
                raw_field(1, WireType::Uuid, &project),
                raw_field(2, WireType::Optional, &session),
                raw_field(3, WireType::Optional, &link),
            ]
        };
        assert_eq!(
            GoalScopeLinkProvenanceV1::decode(&raw_record(
                0,
                1,
                &provenance_fields(vec![1; 4], vec![0], vec![0])
            ))
            .expect_err("malformed project identity"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            GoalScopeLinkProvenanceV1::decode(&raw_record(
                0,
                1,
                &provenance_fields(vec![1; 16], vec![1, 0, 1], vec![0])
            ))
            .expect_err("malformed session optional"),
            CanonicalError::InvalidOptional
        );
        assert_eq!(
            GoalScopeLinkProvenanceV1::decode(&raw_record(
                0,
                1,
                &provenance_fields(vec![1; 16], vec![0], vec![1, 0, 1])
            ))
            .expect_err("malformed link optional"),
            CanonicalError::InvalidOptional
        );
        assert_eq!(
            GoalScopeLinkProvenanceV1::decode(&raw_record(
                0,
                1,
                &provenance_fields(vec![1; 16], vec![0], vec![1; 17])
            ))
            .expect_err("decoded link without an owner session"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            GoalScopeLinkProvenanceV1::decode(&raw_record(
                0,
                1,
                &provenance_fields(vec![1; 16], vec![0], vec![0])
            ))
            .expect("the session-less provenance decodes"),
            GoalScopeLinkProvenanceV1 {
                project_id: [1; 16],
                session_id: None,
                link_id: None,
            }
        );
    }

    #[test]
    fn goal_revision_reference_codec_enforces_exact_revisions() {
        let reference = GoalRevisionReferenceV1 {
            goal_id: [8; 16],
            revision: 1,
        };
        let encoded = reference.encode().expect("the chain reference encodes");
        assert_eq!(
            GoalRevisionReferenceV1::decode(&encoded).expect("the chain reference decodes"),
            reference
        );
        assert_eq!(
            GoalRevisionReferenceV1 {
                revision: 0,
                ..reference
            }
            .encode()
            .expect_err("zero chain revision"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            GoalRevisionReferenceV1::decode(&raw_record(1, 1, &[]))
                .expect_err("unknown chain reference tag"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            GoalRevisionReferenceV1::decode(&raw_record(0, 2, &[]))
                .expect_err("unknown chain reference version"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            GoalRevisionReferenceV1::decode(&raw_record(
                0,
                1,
                &[
                    raw_field(1, WireType::Uuid, &[1; 4]),
                    raw_field(2, WireType::U64, &encode_u64(1)),
                ]
            ))
            .expect_err("malformed chain Goal identity"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            GoalRevisionReferenceV1::decode(&raw_record(
                0,
                1,
                &[
                    raw_field(1, WireType::Uuid, &[8; 16]),
                    raw_field(2, WireType::U64, &[0, 1]),
                ]
            ))
            .expect_err("non-minimal chain revision"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            GoalRevisionReferenceV1::decode(&raw_record(
                0,
                1,
                &[
                    raw_field(1, WireType::Uuid, &[8; 16]),
                    raw_field(2, WireType::U64, &[0]),
                ]
            ))
            .expect_err("zero decoded chain revision"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn gate_and_card_reference_codecs_enforce_exact_revisions() {
        let gate = GoalGateRevisionReferenceV1 {
            gate_reference: [10; 16],
            revision: 1,
        };
        let encoded = gate.encode().expect("the gate reference encodes");
        assert_eq!(
            GoalGateRevisionReferenceV1::decode(&encoded).expect("the gate reference decodes"),
            gate
        );
        assert_eq!(
            GoalGateRevisionReferenceV1 {
                revision: 0,
                ..gate
            }
            .encode()
            .expect_err("zero gate reference revision"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            GoalGateRevisionReferenceV1::decode(&raw_record(1, 1, &[]))
                .expect_err("unknown gate reference tag"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            GoalGateRevisionReferenceV1::decode(&raw_record(0, 2, &[]))
                .expect_err("unknown gate reference version"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            GoalGateRevisionReferenceV1::decode(&raw_record(
                0,
                1,
                &[
                    raw_field(1, WireType::Uuid, &[1; 4]),
                    raw_field(2, WireType::U64, &encode_u64(1)),
                ]
            ))
            .expect_err("malformed gate reference identity"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            GoalGateRevisionReferenceV1::decode(&raw_record(
                0,
                1,
                &[
                    raw_field(1, WireType::Uuid, &[10; 16]),
                    raw_field(2, WireType::U64, &[0, 1]),
                ]
            ))
            .expect_err("non-minimal gate reference revision"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            GoalGateRevisionReferenceV1::decode(&raw_record(
                0,
                1,
                &[
                    raw_field(1, WireType::Uuid, &[10; 16]),
                    raw_field(2, WireType::U64, &[0]),
                ]
            ))
            .expect_err("zero decoded gate reference revision"),
            CanonicalError::InvalidField
        );
        let card = GoalCardReferenceV1 {
            card_reference: [12; 16],
            revision: 1,
        };
        let encoded = card.encode().expect("the card reference encodes");
        assert_eq!(
            GoalCardReferenceV1::decode(&encoded).expect("the card reference decodes"),
            card
        );
        assert_eq!(
            GoalCardReferenceV1 {
                revision: 0,
                ..card
            }
            .encode()
            .expect_err("zero card reference revision"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            GoalCardReferenceV1::decode(&raw_record(1, 1, &[]))
                .expect_err("unknown card reference tag"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            GoalCardReferenceV1::decode(&raw_record(0, 2, &[]))
                .expect_err("unknown card reference version"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            GoalCardReferenceV1::decode(&raw_record(
                0,
                1,
                &[
                    raw_field(1, WireType::Uuid, &[1; 4]),
                    raw_field(2, WireType::U64, &encode_u64(1)),
                ]
            ))
            .expect_err("malformed card reference identity"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            GoalCardReferenceV1::decode(&raw_record(
                0,
                1,
                &[
                    raw_field(1, WireType::Uuid, &[12; 16]),
                    raw_field(2, WireType::U64, &[0, 1]),
                ]
            ))
            .expect_err("non-minimal card reference revision"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            GoalCardReferenceV1::decode(&raw_record(
                0,
                1,
                &[
                    raw_field(1, WireType::Uuid, &[12; 16]),
                    raw_field(2, WireType::U64, &[0]),
                ]
            ))
            .expect_err("zero decoded card reference revision"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn goal_run_selection_bounds_codec_enforces_code_owned_limits() {
        let bounds = goal_selection().bounds;
        let encoded = bounds.encode().expect("the goal-run bounds encode");
        assert_eq!(
            GoalRunSelectionBoundsV1::decode(&encoded).expect("the goal-run bounds decode"),
            bounds
        );
        for invalid in [
            GoalRunSelectionBoundsV1 {
                max_memory_cards: 0,
                ..bounds
            },
            GoalRunSelectionBoundsV1 {
                max_memory_cards: GOAL_MAX_MEMORY_CARDS + 1,
                ..bounds
            },
            GoalRunSelectionBoundsV1 {
                max_skill_role_cards: 0,
                ..bounds
            },
            GoalRunSelectionBoundsV1 {
                max_skill_role_cards: GOAL_MAX_SKILL_ROLE_CARDS + 1,
                ..bounds
            },
            GoalRunSelectionBoundsV1 {
                target_snapshot_bytes: 0,
                ..bounds
            },
            GoalRunSelectionBoundsV1 {
                target_snapshot_bytes: GOAL_MAX_TARGET_SNAPSHOT_BYTES + 1,
                ..bounds
            },
            GoalRunSelectionBoundsV1 {
                context_bytes: 0,
                ..bounds
            },
            GoalRunSelectionBoundsV1 {
                context_bytes: GOAL_MAX_CONTEXT_BYTES + 1,
                ..bounds
            },
        ] {
            assert_eq!(
                invalid.encode().expect_err("out-of-range goal-run bounds"),
                CanonicalError::OverLimit
            );
            assert_eq!(
                GoalRunSelectionBoundsV1::decode(&raw_record(0, 1, &goal_bounds_fields(&invalid)))
                    .expect_err("out-of-range decoded goal-run bounds"),
                CanonicalError::OverLimit
            );
        }
        assert_eq!(
            GoalRunSelectionBoundsV1::decode(&raw_record(1, 1, &[]))
                .expect_err("unknown goal-run bounds tag"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            GoalRunSelectionBoundsV1::decode(&raw_record(0, 2, &[]))
                .expect_err("unknown goal-run bounds version"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            GoalRunSelectionBoundsV1::decode(&raw_record(
                0,
                1,
                &[raw_field(
                    1,
                    WireType::U64,
                    &encode_u64(GOAL_MAX_MEMORY_CARDS)
                )]
            ))
            .expect_err("absent goal-run bounds fields"),
            CanonicalError::InvalidField
        );
    }

    #[test]
    fn goal_run_selection_decode_rejects_oversized_malformed_and_inconsistent_records() {
        let oversized = vec![0u8; MAX_GOAL_RUN_SELECTION_BYTES + 1];
        assert_eq!(
            GoalRunSelectionV1::decode(&oversized).expect_err("oversized goal-run selection"),
            CanonicalError::OverLimit
        );
        assert_eq!(
            GoalRunSelectionV1::decode(&raw_record(1, 1, &[]))
                .expect_err("unknown goal-run selection tag"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            GoalRunSelectionV1::decode(&raw_record(TagRegistry::GOAL_RUN_SELECTION_V1, 2, &[]))
                .expect_err("unknown goal-run selection version"),
            CanonicalError::InvalidTag
        );
        let fixture = goal_selection();
        assert_eq!(
            GoalRunSelectionV1::decode(&raw_record(
                TagRegistry::GOAL_RUN_SELECTION_V1,
                1,
                &goal_selection_fields(&fixture)
            ))
            .expect("the raw goal-run selection decodes"),
            fixture
        );
        for (field, message) in [
            (4u32, "over-limit declared chain count"),
            (6, "over-limit declared gate count"),
            (8, "over-limit declared memory card count"),
            (9, "over-limit declared Skill card count"),
            (10, "over-limit declared role card count"),
        ] {
            let mut fields = goal_selection_fields(&fixture);
            set_raw_field(&mut fields, field, vec![0xFF, 0xFF, 0xFF, 0xFF]);
            assert_eq!(
                GoalRunSelectionV1::decode(&raw_record(
                    TagRegistry::GOAL_RUN_SELECTION_V1,
                    1,
                    &fields
                ))
                .expect_err(message),
                CanonicalError::OverLimit
            );
        }
        let mut fields = goal_selection_fields(&fixture);
        set_raw_field(&mut fields, 1, vec![1; 4]);
        assert_eq!(
            GoalRunSelectionV1::decode(&raw_record(TagRegistry::GOAL_RUN_SELECTION_V1, 1, &fields))
                .expect_err("malformed leading Goal identity"),
            CanonicalError::InvalidField
        );
        let mut fields = goal_selection_fields(&fixture);
        set_raw_field(&mut fields, 2, vec![0, 1]);
        assert_eq!(
            GoalRunSelectionV1::decode(&raw_record(TagRegistry::GOAL_RUN_SELECTION_V1, 1, &fields))
                .expect_err("non-minimal Goal revision"),
            CanonicalError::InvalidField
        );
        let mut fields = goal_selection_fields(&fixture);
        set_raw_field(&mut fields, 3, raw_record(1, 1, &[]));
        assert_eq!(
            GoalRunSelectionV1::decode(&raw_record(TagRegistry::GOAL_RUN_SELECTION_V1, 1, &fields))
                .expect_err("malformed nested provenance"),
            CanonicalError::InvalidTag
        );
        for (field, message) in [
            (5u32, "malformed obligatory component list"),
            (7, "malformed valid evidence list"),
            (11, "malformed revealed record list"),
        ] {
            let mut fields = goal_selection_fields(&fixture);
            set_raw_field(&mut fields, field, vec![0, 0, 0, 3]);
            assert_eq!(
                GoalRunSelectionV1::decode(&raw_record(
                    TagRegistry::GOAL_RUN_SELECTION_V1,
                    1,
                    &fields
                ))
                .expect_err(message),
                CanonicalError::Truncated
            );
        }
        for (field, message) in [
            (4u32, "malformed parent revision chain list"),
            (6, "malformed selected gate revision list"),
            (8, "malformed selected memory card list"),
            (9, "malformed selected Skill card list"),
            (10, "malformed selected role card list"),
        ] {
            let mut fields = goal_selection_fields(&fixture);
            set_raw_field(&mut fields, field, vec![0, 0, 0, 3]);
            assert_eq!(
                GoalRunSelectionV1::decode(&raw_record(
                    TagRegistry::GOAL_RUN_SELECTION_V1,
                    1,
                    &fields
                ))
                .expect_err(message),
                CanonicalError::Truncated
            );
        }
        let mut fields = goal_selection_fields(&fixture);
        set_raw_field(&mut fields, 12, vec![1; 4]);
        assert_eq!(
            GoalRunSelectionV1::decode(&raw_record(TagRegistry::GOAL_RUN_SELECTION_V1, 1, &fields))
                .expect_err("malformed policy snapshot reference"),
            CanonicalError::InvalidField
        );
        let mut fields = goal_selection_fields(&fixture);
        set_raw_field(&mut fields, 13, vec![1; 4]);
        assert_eq!(
            GoalRunSelectionV1::decode(&raw_record(TagRegistry::GOAL_RUN_SELECTION_V1, 1, &fields))
                .expect_err("malformed activity selection reference"),
            CanonicalError::InvalidField
        );
        let mut fields = goal_selection_fields(&fixture);
        set_raw_field(&mut fields, 14, vec![9]);
        assert_eq!(
            GoalRunSelectionV1::decode(&raw_record(TagRegistry::GOAL_RUN_SELECTION_V1, 1, &fields))
                .expect_err("unknown run kind"),
            CanonicalError::InvalidField
        );
        let mut fields = goal_selection_fields(&fixture);
        set_raw_field(&mut fields, 15, vec![1; 8]);
        assert_eq!(
            GoalRunSelectionV1::decode(&raw_record(TagRegistry::GOAL_RUN_SELECTION_V1, 1, &fields))
                .expect_err("malformed target snapshot digest"),
            CanonicalError::InvalidDigest
        );
        let mut fields = goal_selection_fields(&fixture);
        set_raw_field(&mut fields, 16, raw_record(0, 2, &[]));
        assert_eq!(
            GoalRunSelectionV1::decode(&raw_record(TagRegistry::GOAL_RUN_SELECTION_V1, 1, &fields))
                .expect_err("malformed nested goal-run bounds"),
            CanonicalError::InvalidTag
        );
        let mut fields = goal_selection_fields(&fixture);
        set_raw_field(&mut fields, 2, vec![0]);
        assert_eq!(
            GoalRunSelectionV1::decode(&raw_record(TagRegistry::GOAL_RUN_SELECTION_V1, 1, &fields))
                .expect_err("zero decoded Goal revision"),
            CanonicalError::OverLimit
        );
        let mut fields = goal_selection_fields(&fixture);
        set_raw_field(
            &mut fields,
            8,
            encode_list_items(&[
                GoalCardReferenceV1 {
                    card_reference: [12; 16],
                    revision: 1,
                }
                .encode()
                .expect("the memory card reference encodes"),
                GoalCardReferenceV1 {
                    card_reference: [13; 16],
                    revision: 1,
                }
                .encode()
                .expect("the memory card reference encodes"),
            ]),
        );
        set_raw_field(
            &mut fields,
            16,
            GoalRunSelectionBoundsV1 {
                max_memory_cards: 1,
                ..fixture.bounds
            }
            .encode()
            .expect("the reduced goal-run bounds encode"),
        );
        assert_eq!(
            GoalRunSelectionV1::decode(&raw_record(TagRegistry::GOAL_RUN_SELECTION_V1, 1, &fields))
                .expect_err("memory cards over the decoded bound"),
            CanonicalError::OverLimit
        );
    }

    #[test]
    fn mcp_method_catalog_selection_validates_text_and_optional_family() {
        let without_family = McpMethodCatalogSelectionV1 {
            typed_input_constraint_family: None,
            ..mcp_selection()
        };
        let encoded = without_family
            .encode()
            .expect("the family-less MCP selection encodes");
        assert_eq!(
            McpMethodCatalogSelectionV1::decode(&encoded)
                .expect("the family-less MCP selection decodes"),
            without_family
        );
        assert_eq!(
            McpMethodCatalogSelectionV1 {
                method_catalog_revision: "  ".to_owned(),
                ..mcp_selection()
            }
            .encode()
            .expect_err("blank method catalog revision"),
            CanonicalError::InvalidField
        );
        assert_eq!(
            McpMethodCatalogSelectionV1 {
                typed_input_constraint_family: Some("token=abc".to_owned()),
                ..mcp_selection()
            }
            .encode()
            .expect_err("credential-shaped typed input family"),
            CanonicalError::CredentialsForbidden
        );
        assert_eq!(
            McpMethodCatalogSelectionV1 {
                method_reference: "x".repeat(MAX_SLICE3_TEXT_CHARS + 1),
                ..mcp_selection()
            }
            .encode()
            .expect_err("over-long method reference"),
            CanonicalError::InvalidField
        );
        let oversized = vec![0u8; MAX_MCP_METHOD_CATALOG_SELECTION_BYTES + 1];
        assert_eq!(
            McpMethodCatalogSelectionV1::decode(&oversized)
                .expect_err("oversized MCP method catalog selection"),
            CanonicalError::OverLimit
        );
        assert_eq!(
            McpMethodCatalogSelectionV1::decode(&raw_record(1, 1, &[]))
                .expect_err("unknown MCP selection tag"),
            CanonicalError::InvalidTag
        );
        assert_eq!(
            McpMethodCatalogSelectionV1::decode(&raw_record(
                TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1,
                2,
                &[]
            ))
            .expect_err("unknown MCP selection version"),
            CanonicalError::InvalidTag
        );
        let mcp_fields = |family: Vec<u8>| {
            vec![
                raw_field(1, WireType::Utf8, &encode_utf8("mcp-catalog-rev-1")),
                raw_field(2, WireType::Uuid, &[16; 16]),
                raw_field(3, WireType::Digest, &[1; 32]),
                raw_field(4, WireType::Utf8, &encode_utf8("tools/search")),
                raw_field(5, WireType::Utf8, &encode_utf8("schema-rev-1")),
                raw_field(6, WireType::Optional, &family),
            ]
        };
        assert_eq!(
            McpMethodCatalogSelectionV1::decode(&raw_record(
                TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1,
                1,
                &mcp_fields(vec![0])
            ))
            .expect("the raw MCP selection decodes")
            .typed_input_constraint_family,
            None
        );
        assert_eq!(
            McpMethodCatalogSelectionV1::decode(&raw_record(
                TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1,
                1,
                &mcp_fields(vec![1, 0xFF])
            ))
            .expect_err("invalid UTF-8 typed input family"),
            CanonicalError::InvalidUtf8
        );
        assert_eq!(
            McpMethodCatalogSelectionV1::decode(&raw_record(
                TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1,
                1,
                &mcp_fields(vec![2])
            ))
            .expect_err("unknown typed input family marker"),
            CanonicalError::InvalidOptional
        );
        let mut fields = mcp_fields(vec![0]);
        set_raw_field(&mut fields, 2, vec![1; 4]);
        assert_eq!(
            McpMethodCatalogSelectionV1::decode(&raw_record(
                TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1,
                1,
                &fields
            ))
            .expect_err("malformed MCP connection reference"),
            CanonicalError::InvalidField
        );
        let mut fields = mcp_fields(vec![0]);
        set_raw_field(&mut fields, 3, vec![1; 8]);
        assert_eq!(
            McpMethodCatalogSelectionV1::decode(&raw_record(
                TagRegistry::MCP_METHOD_CATALOG_SELECTION_V1,
                1,
                &fields
            ))
            .expect_err("malformed MCP server revision digest"),
            CanonicalError::InvalidDigest
        );
    }
}
