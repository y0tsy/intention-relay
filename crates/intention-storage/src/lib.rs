//! DTO-only semantic storage contracts for durable sessions and transcript state.
//!
//! The SQLite backend lives behind this same boundary as the private `sqlite`
//! module, surfaced only through the DTO-only repository contract below.
//!
//! Implementations own transactions and backend resources. This crate exposes no
//! connection, filesystem, SQL, path, or closure-based API across its boundary.
//!
//! Every state-changing method commits its change in exactly one transaction or
//! changes nothing, and returns the committed values. There is no event log, no
//! snapshot, no cursor, and no replay: a later read observes only committed
//! state, and a caller publishes from the values a commit returned.
//!
//! Every field is a typed domain or storage value. No opaque JSON string,
//! untyped map, or caller-supplied encoding crosses this boundary: a backend
//! that persists a record as text owns both directions of its canonical codec
//! behind the typed field, and only the typed value is visible to callers.

use intention_config::ConfigSnapshotDto;
use intention_proto::provider::{
    CatalogRevisionId, ProviderDiscoveryAttemptId, ProviderKindDescriptorRevisionV1,
    ProviderModelRecordDto, ProviderProfileId, ProviderProfilePolicyDto, ProviderProfileRevisionId,
    ProviderProfileRevisionV1, ReasoningHistoryManifestDto, ResolvedRunProviderSelectionDto,
};
use intention_proto::{
    CreateSessionCommandDto, MessageKindDto, MessageProjectionDto, PendingTurnProjectionDto,
    RemoveTurnCommandDto, RunProjectionDto, RunStatusDto, SessionProjectionDto, SessionSnapshotDto,
};
use intention_proto::{
    DtoResult, ErrorCategoryDto, ErrorDto, ErrorRetryDto, FinishReasonDto, IdempotencyKey, RunId,
    SessionId, TimestampDto, ToolCallId, UsageDto, run_status_is_terminal,
};
use serde::{Deserialize, Deserializer, Serialize, de};

mod sqlite;

pub use sqlite::{SqliteDatabaseLocationDto, SqliteStorageRepository};

/// The maximum durable tool result content size in bytes.
const MAX_TOOL_RESULT_CONTENT_BYTES: usize = 512 * 1024;

/// The maximum durable user turn content size in bytes.
///
/// A turn is admitted with a bound so the session projection that carries its
/// pending rows can never be permanently larger than one transport envelope.
const MAX_TURN_CONTENT_BYTES: usize = 512 * 1024;

/// The maximum encoded pending-turn projection content kept in one session
/// projection, in bytes.
///
/// It equals the admission bound, so the newest queued turn always fits and a
/// projection always reports at least one pending turn; older turns beyond it
/// are reported through the projection's omitted count instead of being
/// materialised.
const MAX_PENDING_TURN_PROJECTION_BYTES: usize = MAX_TURN_CONTENT_BYTES;

/// The maximum aggregate reasoning material one cross-turn history may carry,
/// in bytes.
///
/// It is the fixed combined reasoning bound of the provider reasoning contract
/// (4 MiB). A history whose aggregate exceeds it is rejected whole before
/// provider work; no fragment is truncated and no partial manifest is
/// committed.
const MAX_REASONING_HISTORY_AGGREGATE_BYTES: u64 = 4 * 1024 * 1024;

/// The maximum aggregate discovery result content one attempt may record, in
/// bytes.
///
/// The bound is one transport envelope (512 KiB): a record set whose model
/// identities and display names together exceed one envelope could never be
/// delivered as a single typed result, so the attempt rejects the whole set
/// instead of truncating it.
const MAX_DISCOVERY_RESULT_BYTES: usize = 512 * 1024;

/// The terminal outcome recorded for one local tool result.
///
/// The taxonomy is deliberately closed to terminal outcomes: a call records
/// exactly one result row, and admission, rejection, and start evidence are
/// not persisted.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatusDto {
    /// The tool completed and produced a normalized safe result.
    Completed,
    /// The tool reported a safe failure outcome.
    Failed,
    /// The tool stopped before a final outcome; its captured output is partial.
    Partial,
}

impl ToolResultStatusDto {
    /// Returns the canonical durable string representation of this tool result status.
    ///
    /// The representation is persisted verbatim, so it must stay byte-identical
    /// across releases.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Partial => "partial",
        }
    }

    /// Parses the canonical durable string representation of a tool result status.
    ///
    /// # Errors
    ///
    /// Returns a safe internal error when `value` is not a declared durable tool result status.
    pub fn parse(value: &str) -> DtoResult<Self> {
        match value {
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "partial" => Ok(Self::Partial),
            _ => Err(ErrorDto::new(
                "invalid_tool_result_status",
                ErrorCategoryDto::Internal,
                "the durable tool result status is not declared",
                ErrorRetryDto::Never,
                None,
            )?),
        }
    }
}

/// One credential-free structured metadata entry of a tool result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolResultMetadataEntryDto {
    key: String,
    value: String,
}

impl<'de> Deserialize<'de> for ToolResultMetadataEntryDto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawToolResultMetadataEntryDto {
            key: String,
            value: String,
        }

        let raw = RawToolResultMetadataEntryDto::deserialize(deserializer)?;
        Self::new(raw.key, raw.value).map_err(de::Error::custom)
    }
}

impl ToolResultMetadataEntryDto {
    /// Creates one metadata entry with a non-blank key.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the key is blank or either field
    /// contains a NUL character.
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> DtoResult<Self> {
        let key = key.into();
        let value = value.into();
        if key.trim().is_empty() || key.contains('\0') || value.contains('\0') {
            return Err(ErrorDto::validation(
                "invalid_tool_result_metadata",
                "tool result metadata must have a non-blank key",
            ));
        }
        Ok(Self { key, value })
    }

    /// Returns the stable metadata key.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns the metadata value.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Typed durable evidence of one committed local tool result.
///
/// `content` carries the bounded canonical projection of the typed result as
/// selected by the caller; it never carries credentials, absolute workspace
/// roots, or backend resources. `metadata` carries approved credential-free
/// structured entries such as truncation or process-exit evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolResultEvidenceDto {
    session_id: SessionId,
    run_id: RunId,
    call_id: ToolCallId,
    tool_id: String,
    status: ToolResultStatusDto,
    content: String,
    metadata: Vec<ToolResultMetadataEntryDto>,
    occurred_at: TimestampDto,
}

impl ToolResultEvidenceDto {
    /// Creates typed tool result evidence with bounded canonical content.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the tool identity is blank, the content
    /// is blank, oversized, or contains interior NUL bytes, or the metadata keys
    /// are duplicated.
    #[expect(
        clippy::too_many_arguments,
        reason = "The flat durable tool-result payload keeps one validating constructor."
    )]
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
        tool_id: impl Into<String>,
        status: ToolResultStatusDto,
        content: impl Into<String>,
        metadata: Vec<ToolResultMetadataEntryDto>,
        occurred_at: TimestampDto,
    ) -> DtoResult<Self> {
        let tool_id = tool_id.into();
        let content = content.into();
        if tool_id.trim().is_empty() || content.trim().is_empty() || content.contains('\0') {
            return Err(ErrorDto::validation(
                "invalid_tool_result",
                "tool result needs a tool identity and non-empty safe content",
            ));
        }
        if content.len() > MAX_TOOL_RESULT_CONTENT_BYTES {
            return Err(ErrorDto::validation(
                "invalid_tool_result",
                "tool result content exceeds the durable canonical size limit",
            ));
        }
        let mut seen_keys = std::collections::HashSet::with_capacity(metadata.len());
        if metadata.iter().any(|entry| !seen_keys.insert(entry.key())) {
            return Err(ErrorDto::validation(
                "invalid_tool_result_metadata",
                "tool result metadata keys must be unique",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            call_id,
            tool_id,
            status,
            content,
            metadata,
            occurred_at,
        })
    }
    /// Returns the owning durable session.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }
    /// Returns the run that executed the tool call.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }
    /// Returns the identified tool invocation.
    #[must_use]
    pub const fn call_id(&self) -> ToolCallId {
        self.call_id
    }
    /// Returns the wire tool identity that produced the result.
    #[must_use]
    pub fn tool_id(&self) -> &str {
        &self.tool_id
    }
    /// Returns the terminal outcome recorded for the result.
    #[must_use]
    pub const fn status(&self) -> ToolResultStatusDto {
        self.status
    }
    /// Returns the bounded canonical durable result content.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
    /// Returns the approved structured metadata entries.
    #[must_use]
    pub fn metadata(&self) -> &[ToolResultMetadataEntryDto] {
        &self.metadata
    }
    /// Returns the durable evidence event time.
    #[must_use]
    pub const fn occurred_at(&self) -> TimestampDto {
        self.occurred_at
    }
}

/// Immutable outcome for an accepted user turn after committing it durably.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcceptedTurnOutcomeDto {
    /// The turn began a newly created run and committed its user message.
    Started {
        /// The committed run projection.
        run: RunProjectionDto,
        /// The committed user message that starts the run context.
        message: MessageProjectionDto,
    },
    /// The turn is a pending message that joins the active run context later.
    Pending(PendingTurnProjectionDto),
}

/// One validated terminal run outcome, committed by a single durable transaction.
///
/// The status is always terminal. `usage` and `finish_reason` may be absent
/// when the provider reported neither, and the optional error pair carries the
/// safe code and message of a failed run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunOutcomeDto {
    status: RunStatusDto,
    usage: Option<UsageDto>,
    finish_reason: Option<FinishReasonDto>,
    error_code: Option<String>,
    error_message: Option<String>,
}

impl RunOutcomeDto {
    /// Creates one validated terminal run outcome.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `status` is not terminal, or when the
    /// error pair is incomplete, blank, or contains a NUL byte.
    pub fn new(
        status: RunStatusDto,
        usage: Option<UsageDto>,
        finish_reason: Option<FinishReasonDto>,
        error_code: Option<String>,
        error_message: Option<String>,
    ) -> DtoResult<Self> {
        if !run_status_is_terminal(status) {
            return Err(ErrorDto::validation(
                "invalid_run_outcome",
                "a terminal run outcome needs a terminal status",
            ));
        }
        let error_valid = match (&error_code, &error_message) {
            (None, None) => true,
            (Some(code), Some(message)) => {
                !code.trim().is_empty()
                    && !message.trim().is_empty()
                    && !code.contains('\0')
                    && !message.contains('\0')
            }
            _ => false,
        };
        if !error_valid {
            return Err(ErrorDto::validation(
                "invalid_run_outcome",
                "a terminal run outcome carries either both error fields or neither",
            ));
        }
        Ok(Self {
            status,
            usage,
            finish_reason,
            error_code,
            error_message,
        })
    }

    /// Returns the terminal run status.
    #[must_use]
    pub const fn status(&self) -> RunStatusDto {
        self.status
    }

    /// Returns the reported model usage, when the provider reported one.
    #[must_use]
    pub const fn usage(&self) -> Option<&UsageDto> {
        self.usage.as_ref()
    }

    /// Returns the provider finish reason, when the provider reported one.
    #[must_use]
    pub const fn finish_reason(&self) -> Option<FinishReasonDto> {
        self.finish_reason
    }

    /// Returns the safe error code of a failed run, when one was recorded.
    #[must_use]
    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }

    /// Returns the safe error message of a failed run, when one was recorded.
    #[must_use]
    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }
}

/// DTO-only full session model context for one current starting run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartingRunModelContextDto {
    session_id: SessionId,
    run_id: RunId,
    safe_config: ConfigSnapshotDto,
    messages: Vec<MessageProjectionDto>,
}

impl StartingRunModelContextDto {
    /// Creates context whose final message is the current starting user turn.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the configuration is not persistable,
    /// the context is empty, a message belongs to another session, or the final
    /// message is not the starting run's user turn.
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        safe_config: ConfigSnapshotDto,
        messages: Vec<MessageProjectionDto>,
    ) -> DtoResult<Self> {
        safe_config.validate_for_persistence()?;
        let ends_with_user = messages.last().is_some_and(|message| {
            message.kind() == MessageKindDto::User && message.run_id() == Some(run_id)
        });
        if messages
            .iter()
            .any(|message| message.session_id() != session_id)
            || !ends_with_user
        {
            return Err(ErrorDto::validation(
                "invalid_model_context",
                "starting run model context must end with its session's starting user turn",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            safe_config,
            messages,
        })
    }

    /// Returns the session that owns this model context.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the current starting run that owns this context.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the run's immutable credential-free configuration selection.
    #[must_use]
    pub const fn safe_config(&self) -> &ConfigSnapshotDto {
        &self.safe_config
    }

    /// Returns ordered committed transcript rows in insertion order.
    #[must_use]
    pub fn messages(&self) -> &[MessageProjectionDto] {
        &self.messages
    }
}

/// The closed append-only catalog audit taxonomy.
///
/// The storage path writes exactly these records around catalog preparation,
/// acceptance, activation, rejection, and activation recovery. No wire DTO
/// carries them; they are durable evidence, not a protocol surface.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCatalogAuditRecordKindDto {
    /// A candidate catalog revision was prepared for acceptance.
    ProviderCatalogCandidatePrepared,
    /// A candidate removes at least one accepted profile or kind.
    ProviderCatalogRemovalPending,
    /// The removal candidate was accepted.
    ProviderCatalogRemovalAccepted,
    /// The removal candidate was rejected and dropped.
    ProviderCatalogCandidateRejected,
    /// The catalog revision was durably accepted.
    ProviderCatalogAccepted,
    /// The accepted catalog revision became the active one.
    ProviderCatalogActivated,
    /// An accepted revision was found unactivated after a crash.
    ProviderCatalogActivationRecoveryRequired,
    /// The exact accepted revision became active after recovery.
    ProviderCatalogRecoveryCompleted,
}

impl ProviderCatalogAuditRecordKindDto {
    /// Returns the canonical durable string representation of this audit kind.
    ///
    /// The representation is persisted verbatim, so it must stay byte-identical
    /// across releases.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProviderCatalogCandidatePrepared => "provider_catalog_candidate_prepared",
            Self::ProviderCatalogRemovalPending => "provider_catalog_removal_pending",
            Self::ProviderCatalogRemovalAccepted => "provider_catalog_removal_accepted",
            Self::ProviderCatalogCandidateRejected => "provider_catalog_candidate_rejected",
            Self::ProviderCatalogAccepted => "provider_catalog_accepted",
            Self::ProviderCatalogActivated => "provider_catalog_activated",
            Self::ProviderCatalogActivationRecoveryRequired => {
                "provider_catalog_activation_recovery_required"
            }
            Self::ProviderCatalogRecoveryCompleted => "provider_catalog_recovery_completed",
        }
    }

    /// Parses the canonical durable string representation of a catalog audit kind.
    ///
    /// # Errors
    ///
    /// Returns a safe internal error when `value` is not a declared audit kind.
    pub fn parse(value: &str) -> DtoResult<Self> {
        match value {
            "provider_catalog_candidate_prepared" => Ok(Self::ProviderCatalogCandidatePrepared),
            "provider_catalog_removal_pending" => Ok(Self::ProviderCatalogRemovalPending),
            "provider_catalog_removal_accepted" => Ok(Self::ProviderCatalogRemovalAccepted),
            "provider_catalog_candidate_rejected" => Ok(Self::ProviderCatalogCandidateRejected),
            "provider_catalog_accepted" => Ok(Self::ProviderCatalogAccepted),
            "provider_catalog_activated" => Ok(Self::ProviderCatalogActivated),
            "provider_catalog_activation_recovery_required" => {
                Ok(Self::ProviderCatalogActivationRecoveryRequired)
            }
            "provider_catalog_recovery_completed" => Ok(Self::ProviderCatalogRecoveryCompleted),
            _ => Err(ErrorDto::new(
                "invalid_catalog_audit_kind",
                ErrorCategoryDto::Internal,
                "the durable catalog audit kind is not declared",
                ErrorRetryDto::Never,
                None,
            )?),
        }
    }
}

/// The current accepted and activated catalog pointers.
///
/// An accepted revision that is not the activated one is exactly the
/// activation-recovery state: the process crashed after acceptance and before
/// the exact accepted catalog became active.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderCatalogStateDto {
    accepted_catalog_revision_id: Option<CatalogRevisionId>,
    activated_catalog_revision_id: Option<CatalogRevisionId>,
}

impl ProviderCatalogStateDto {
    /// Creates one coherent accepted/activated pointer pair.
    ///
    /// # Errors
    ///
    /// Returns a validation error when an activated pointer exists without any
    /// accepted revision. The two pointers differ exactly while the accepted
    /// catalog revision has not become active yet.
    pub fn new(
        accepted_catalog_revision_id: Option<CatalogRevisionId>,
        activated_catalog_revision_id: Option<CatalogRevisionId>,
    ) -> DtoResult<Self> {
        if activated_catalog_revision_id.is_some() && accepted_catalog_revision_id.is_none() {
            return Err(ErrorDto::validation(
                "invalid_catalog_state",
                "an activated catalog revision requires an accepted catalog revision",
            ));
        }
        Ok(Self {
            accepted_catalog_revision_id,
            activated_catalog_revision_id,
        })
    }

    /// Returns the accepted catalog revision, when one was accepted.
    #[must_use]
    pub const fn accepted_catalog_revision_id(&self) -> Option<CatalogRevisionId> {
        self.accepted_catalog_revision_id
    }

    /// Returns the activated catalog revision, when one became active.
    #[must_use]
    pub const fn activated_catalog_revision_id(&self) -> Option<CatalogRevisionId> {
        self.activated_catalog_revision_id
    }

    /// Returns whether an accepted catalog revision is not the activated one.
    ///
    /// A true value means the exact accepted revision must be rebuilt and
    /// activated before the daemon can report ready.
    #[must_use]
    pub fn requires_activation_recovery(&self) -> bool {
        self.accepted_catalog_revision_id
            .is_some_and(|accepted| self.activated_catalog_revision_id != Some(accepted))
    }
}

/// One profile's current display, enabled, and pricing policy carried by a
/// catalog revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderProfilePolicyEntryDto {
    profile_id: ProviderProfileId,
    policy: ProviderProfilePolicyDto,
}

impl ProviderProfilePolicyEntryDto {
    /// Creates one profile policy entry.
    #[must_use]
    pub const fn new(profile_id: ProviderProfileId, policy: ProviderProfilePolicyDto) -> Self {
        Self { profile_id, policy }
    }

    /// Returns the profile the policy belongs to.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }

    /// Returns the current policy of the profile.
    #[must_use]
    pub const fn policy(&self) -> &ProviderProfilePolicyDto {
        &self.policy
    }
}

/// Returns the validation error for an incoherent catalog revision.
fn invalid_catalog_revision() -> ErrorDto {
    ErrorDto::validation(
        "invalid_catalog_revision",
        "a catalog revision must declare coherent kinds, profiles, and policies",
    )
}

/// One credential-free provider catalog revision offered for durable acceptance.
///
/// The revision carries every kind descriptor and profile revision it declares,
/// the current display/enabled/pricing policy of each member profile, and the
/// global default profile. Credentials, raw TOML, configuration paths, and
/// private driver resources never appear here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCatalogRevisionDto {
    catalog_revision_id: CatalogRevisionId,
    default_profile_id: Option<ProviderProfileId>,
    captured_at: TimestampDto,
    kind_descriptors: Vec<ProviderKindDescriptorRevisionV1>,
    profile_revisions: Vec<ProviderProfileRevisionV1>,
    profile_policies: Vec<ProviderProfilePolicyEntryDto>,
}

impl ProviderCatalogRevisionDto {
    /// Creates one coherent credential-free catalog revision.
    ///
    /// # Errors
    ///
    /// Returns a validation error when kind or profile identities are
    /// duplicated, when a profile references a kind the revision does not
    /// declare, when a profile's declared kind descriptor revision is not the
    /// revision declared for its kind, or when the policy entries are not
    /// exactly one per member profile.
    pub fn new(
        catalog_revision_id: CatalogRevisionId,
        default_profile_id: Option<ProviderProfileId>,
        captured_at: TimestampDto,
        kind_descriptors: Vec<ProviderKindDescriptorRevisionV1>,
        profile_revisions: Vec<ProviderProfileRevisionV1>,
        profile_policies: Vec<ProviderProfilePolicyEntryDto>,
    ) -> DtoResult<Self> {
        let mut kind_ids = std::collections::BTreeSet::new();
        let mut descriptor_ids = std::collections::BTreeSet::new();
        for descriptor in &kind_descriptors {
            if !kind_ids.insert(descriptor.kind_id())
                || !descriptor_ids.insert(descriptor.descriptor_revision_id())
            {
                return Err(invalid_catalog_revision());
            }
        }
        let mut profile_ids = std::collections::BTreeSet::new();
        for profile in &profile_revisions {
            if !profile_ids.insert(profile.profile_id()) {
                return Err(invalid_catalog_revision());
            }
            let declared = kind_descriptors
                .iter()
                .find(|descriptor| descriptor.kind_id() == profile.kind_id());
            let Some(declared) = declared else {
                return Err(invalid_catalog_revision());
            };
            if declared.descriptor_revision_id() != profile.kind_descriptor_revision_id() {
                return Err(invalid_catalog_revision());
            }
        }
        let mut policy_profile_ids = std::collections::BTreeSet::new();
        for entry in &profile_policies {
            if !policy_profile_ids.insert(entry.profile_id()) {
                return Err(invalid_catalog_revision());
            }
        }
        if policy_profile_ids != profile_ids {
            return Err(invalid_catalog_revision());
        }
        Ok(Self {
            catalog_revision_id,
            default_profile_id,
            captured_at,
            kind_descriptors,
            profile_revisions,
            profile_policies,
        })
    }

    /// Returns the catalog revision identity.
    #[must_use]
    pub const fn catalog_revision_id(&self) -> CatalogRevisionId {
        self.catalog_revision_id
    }

    /// Returns the global default profile, when the revision declares one.
    #[must_use]
    pub const fn default_profile_id(&self) -> Option<&ProviderProfileId> {
        self.default_profile_id.as_ref()
    }

    /// Returns the capture time the caller assigned to this revision.
    #[must_use]
    pub const fn captured_at(&self) -> TimestampDto {
        self.captured_at
    }

    /// Returns the kind descriptor revisions this catalog revision declares.
    #[must_use]
    pub fn kind_descriptors(&self) -> &[ProviderKindDescriptorRevisionV1] {
        &self.kind_descriptors
    }

    /// Returns the profile revisions this catalog revision declares.
    #[must_use]
    pub fn profile_revisions(&self) -> &[ProviderProfileRevisionV1] {
        &self.profile_revisions
    }

    /// Returns the current policy of every member profile.
    #[must_use]
    pub fn profile_policies(&self) -> &[ProviderProfilePolicyEntryDto] {
        &self.profile_policies
    }
}

/// The committed outcome of one optimistic session provider-profile change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionProviderProfileChangeDto {
    session_id: SessionId,
    changed: bool,
    session_projection_revision: u64,
}

impl SessionProviderProfileChangeDto {
    /// Creates one committed session provider-profile change outcome.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        changed: bool,
        session_projection_revision: u64,
    ) -> Self {
        Self {
            session_id,
            changed,
            session_projection_revision,
        }
    }

    /// Returns the session whose durable default was addressed.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns whether the durable default changed.
    #[must_use]
    pub const fn changed(&self) -> bool {
        self.changed
    }

    /// Returns the session projection revision after the commit.
    #[must_use]
    pub const fn session_projection_revision(&self) -> u64 {
        self.session_projection_revision
    }
}

/// One session's durable provider default and projection revision.
///
/// Availability, the resolved catalog entry, and the global default are
/// daemon-computed read projections and are deliberately absent here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionProviderProfileDto {
    session_id: SessionId,
    provider_profile_id: Option<ProviderProfileId>,
    session_projection_revision: u64,
}

impl SessionProviderProfileDto {
    /// Creates one durable session provider-profile read.
    #[must_use]
    pub const fn new(
        session_id: SessionId,
        provider_profile_id: Option<ProviderProfileId>,
        session_projection_revision: u64,
    ) -> Self {
        Self {
            session_id,
            provider_profile_id,
            session_projection_revision,
        }
    }

    /// Returns the addressed session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the durable session default, when one was set.
    #[must_use]
    pub const fn provider_profile_id(&self) -> Option<&ProviderProfileId> {
        self.provider_profile_id.as_ref()
    }

    /// Returns the durable session projection revision.
    #[must_use]
    pub const fn session_projection_revision(&self) -> u64 {
        self.session_projection_revision
    }
}

/// The closed lifecycle state of one provider discovery attempt.
///
/// `Prepared` is the before-start state committed before any outbound
/// boundary, `Started` is committed before the outbound call, and the three
/// terminal states record a known result, a known failure, or a recovery-time
/// interruption without terminal proof.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderDiscoveryAttemptStateDto {
    /// The attempt is durably admitted but no outbound work started.
    Prepared,
    /// The attempt crossed its outbound boundary.
    Started,
    /// The attempt recorded a known terminal result.
    Completed,
    /// The attempt recorded a known failure.
    Failed,
    /// Recovery terminalized an attempt without terminal proof.
    Interrupted,
}

impl ProviderDiscoveryAttemptStateDto {
    /// Returns the canonical durable string representation of this attempt state.
    ///
    /// The representation is persisted verbatim, so it must stay byte-identical
    /// across releases.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Started => "started",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    /// Parses the canonical durable string representation of a discovery attempt state.
    ///
    /// # Errors
    ///
    /// Returns a safe internal error when `value` is not a declared attempt state.
    pub fn parse(value: &str) -> DtoResult<Self> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "started" => Ok(Self::Started),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(ErrorDto::new(
                "invalid_discovery_attempt_state",
                ErrorCategoryDto::Internal,
                "the durable discovery attempt state is not declared",
                ErrorRetryDto::Never,
                None,
            )?),
        }
    }

    /// Returns whether this state accepts no further transition.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Interrupted)
    }
}

/// Safe failure evidence recorded for one discovery attempt.
///
/// The pair mirrors the terminal run outcome convention: the code is a closed
/// safe classification chosen by the caller and the message is safe text, never
/// native provider output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderDiscoveryFailureDto {
    code: String,
    message: String,
}

impl ProviderDiscoveryFailureDto {
    /// Creates one safe discovery failure.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the code or message is blank or contains
    /// a NUL byte.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> DtoResult<Self> {
        let code = code.into();
        let message = message.into();
        if code.trim().is_empty()
            || message.trim().is_empty()
            || code.contains('\0')
            || message.contains('\0')
        {
            return Err(ErrorDto::validation(
                "invalid_discovery_failure",
                "a discovery failure needs a non-blank safe code and message",
            ));
        }
        Ok(Self { code, message })
    }

    /// Returns the safe failure code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the safe failure message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// One durable provider discovery attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderDiscoveryAttemptDto {
    attempt_id: ProviderDiscoveryAttemptId,
    profile_id: ProviderProfileId,
    state: ProviderDiscoveryAttemptStateDto,
    prepared_at: TimestampDto,
    started_at: Option<TimestampDto>,
    terminated_at: Option<TimestampDto>,
    failure: Option<ProviderDiscoveryFailureDto>,
}

impl ProviderDiscoveryAttemptDto {
    /// Creates one coherent durable discovery attempt.
    ///
    /// # Errors
    ///
    /// Returns a validation error when a started or terminated time is
    /// inconsistent with the attempt state or when failure evidence is not a
    /// failed attempt's evidence.
    pub fn new(
        attempt_id: ProviderDiscoveryAttemptId,
        profile_id: ProviderProfileId,
        state: ProviderDiscoveryAttemptStateDto,
        prepared_at: TimestampDto,
        started_at: Option<TimestampDto>,
        terminated_at: Option<TimestampDto>,
        failure: Option<ProviderDiscoveryFailureDto>,
    ) -> DtoResult<Self> {
        let started_consistent = started_at.is_none_or(|started| started >= prepared_at);
        // Only a dispatched attempt has a start time: a completed attempt always
        // has one, while a failure or a recovery interruption is also
        // representable before dispatch, where the durable before-start record
        // never reached the provider.
        let started_state = match state {
            ProviderDiscoveryAttemptStateDto::Prepared => started_at.is_none(),
            ProviderDiscoveryAttemptStateDto::Started
            | ProviderDiscoveryAttemptStateDto::Completed => started_at.is_some(),
            ProviderDiscoveryAttemptStateDto::Failed
            | ProviderDiscoveryAttemptStateDto::Interrupted => true,
        };
        let terminal_shape = match state {
            ProviderDiscoveryAttemptStateDto::Completed
            | ProviderDiscoveryAttemptStateDto::Interrupted => {
                terminated_at.is_some() && failure.is_none()
            }
            ProviderDiscoveryAttemptStateDto::Failed => {
                terminated_at.is_some() && failure.is_some()
            }
            ProviderDiscoveryAttemptStateDto::Prepared
            | ProviderDiscoveryAttemptStateDto::Started => {
                terminated_at.is_none() && failure.is_none()
            }
        };
        if !started_consistent || !started_state || !terminal_shape {
            return Err(ErrorDto::validation(
                "invalid_discovery_attempt",
                "a discovery attempt state must match its recorded times and failure evidence",
            ));
        }
        Ok(Self {
            attempt_id,
            profile_id,
            state,
            prepared_at,
            started_at,
            terminated_at,
            failure,
        })
    }

    /// Returns the discovery attempt identity.
    #[must_use]
    pub const fn attempt_id(&self) -> ProviderDiscoveryAttemptId {
        self.attempt_id
    }

    /// Returns the profile the attempt discovers models for.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }

    /// Returns the durable attempt state.
    #[must_use]
    pub const fn state(&self) -> ProviderDiscoveryAttemptStateDto {
        self.state
    }

    /// Returns the before-start evidence time.
    #[must_use]
    pub const fn prepared_at(&self) -> TimestampDto {
        self.prepared_at
    }

    /// Returns the outbound-boundary evidence time, when the attempt started.
    #[must_use]
    pub const fn started_at(&self) -> Option<TimestampDto> {
        self.started_at
    }

    /// Returns the terminal evidence time, when the attempt terminated.
    #[must_use]
    pub const fn terminated_at(&self) -> Option<TimestampDto> {
        self.terminated_at
    }

    /// Returns the recorded safe failure, when the attempt failed.
    #[must_use]
    pub const fn failure(&self) -> Option<&ProviderDiscoveryFailureDto> {
        self.failure.as_ref()
    }
}

/// One bounded usage aggregate per exact profile revision and model identity.
///
/// Aggregation is keyed by the run's committed provider selection, so different
/// profiles that share every safe field stay independent usage groups. No
/// price, currency, or estimated cost is recorded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileUsageAggregateDto {
    profile_id: ProviderProfileId,
    provider_profile_revision_id: ProviderProfileRevisionId,
    model_id: String,
    reported_runs: u64,
    unreported_runs: u64,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
}

impl ProfileUsageAggregateDto {
    /// Creates one coherent usage aggregate.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the model identity is blank or the
    /// aggregate token counts are inconsistent with the reported run count.
    #[expect(
        clippy::too_many_arguments,
        reason = "The flat aggregate keeps one validating constructor per identity."
    )]
    pub fn new(
        profile_id: ProviderProfileId,
        provider_profile_revision_id: ProviderProfileRevisionId,
        model_id: impl Into<String>,
        reported_runs: u64,
        unreported_runs: u64,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
    ) -> DtoResult<Self> {
        let model_id = model_id.into();
        let consistent = input_tokens.checked_add(output_tokens) == Some(total_tokens)
            && (reported_runs > 0
                || (input_tokens == 0 && output_tokens == 0 && total_tokens == 0));
        if model_id.trim().is_empty() || !consistent {
            return Err(ErrorDto::validation(
                "invalid_profile_usage",
                "a usage aggregate needs a model identity and coherent token counts",
            ));
        }
        Ok(Self {
            profile_id,
            provider_profile_revision_id,
            model_id,
            reported_runs,
            unreported_runs,
            input_tokens,
            output_tokens,
            total_tokens,
        })
    }

    /// Returns the exact profile identity of the aggregate.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }

    /// Returns the exact profile revision identity of the aggregate.
    #[must_use]
    pub const fn provider_profile_revision_id(&self) -> ProviderProfileRevisionId {
        self.provider_profile_revision_id
    }

    /// Returns the exact model identity of the aggregate.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns how many terminal runs reported usage.
    #[must_use]
    pub const fn reported_runs(&self) -> u64 {
        self.reported_runs
    }

    /// Returns how many terminal runs reported no usage.
    #[must_use]
    pub const fn unreported_runs(&self) -> u64 {
        self.unreported_runs
    }

    /// Returns the summed reported input tokens.
    #[must_use]
    pub const fn input_tokens(&self) -> u64 {
        self.input_tokens
    }

    /// Returns the summed reported output tokens.
    #[must_use]
    pub const fn output_tokens(&self) -> u64 {
        self.output_tokens
    }

    /// Returns the summed reported total tokens.
    #[must_use]
    pub const fn total_tokens(&self) -> u64 {
        self.total_tokens
    }
}

/// One committed completed reasoning source step of one session.
///
/// The read is the durable source from which a dependent run builds its typed
/// cross-turn reasoning history; it carries the committed assistant step
/// identity and the whole reasoning text of that step, never a fragment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReasoningHistorySourceStepDto {
    session_id: SessionId,
    run_id: RunId,
    message_id: i64,
    reasoning: Option<String>,
}

impl ReasoningHistorySourceStepDto {
    /// Creates one durable reasoning source step.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the message identity is negative or the
    /// reasoning text carries a NUL byte.
    pub fn new(
        session_id: SessionId,
        run_id: RunId,
        message_id: i64,
        reasoning: Option<String>,
    ) -> DtoResult<Self> {
        if message_id < 0 || reasoning.as_deref().is_some_and(|text| text.contains('\0')) {
            return Err(ErrorDto::validation(
                "invalid_reasoning_history_source",
                "a reasoning source step needs a committed message identity and safe reasoning text",
            ));
        }
        Ok(Self {
            session_id,
            run_id,
            message_id,
            reasoning,
        })
    }

    /// Returns the owning session identity.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Returns the completed run that produced the step.
    #[must_use]
    pub const fn run_id(&self) -> RunId {
        self.run_id
    }

    /// Returns the committed transcript row identity of the step.
    #[must_use]
    pub const fn message_id(&self) -> i64 {
        self.message_id
    }

    /// Returns the whole committed reasoning text of the step, when it recorded one.
    #[must_use]
    pub fn reasoning(&self) -> Option<&str> {
        self.reasoning.as_deref()
    }
}

/// The DTO-only current-state storage contract implemented by durable backends.
pub trait StorageRepositoryDto {
    /// Creates a session and returns its committed current-state projection.
    ///
    /// # Errors
    ///
    /// Returns a validation or conflict error when the supplied creation
    /// command cannot be committed, or an unavailable error when durable
    /// storage fails.
    fn create_session(
        &self,
        command: CreateSessionCommandDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<SessionProjectionDto>;

    /// Accepts a turn and atomically records whether it starts a run or becomes a pending message.
    ///
    /// `proposed_run_id` and `config_snapshot` are committed if this turn starts
    /// immediately, or retained as the immutable future-run selection if queued.
    /// `selection` is the exact resolved provider selection of this turn: it is
    /// persisted credential-free in the same transaction, keyed by the turn's
    /// proposed run identity, so the run that starts for this turn carries
    /// exactly one committed selection whether it starts now or is promoted
    /// later. `reasoning_history`, when present, commits its manifest, entries,
    /// and bound audit record in that same transaction. Repeating an accepted
    /// `idempotency_key` replays the current durable outcome instead of
    /// recording a second turn.
    ///
    /// # Errors
    ///
    /// Returns a validation error when content is blank or exceeds the durable
    /// turn-content bound (512 KiB) or when the reasoning history exceeds the
    /// fixed 4 MiB aggregate bound, a validation, not-found, or conflict error
    /// when the turn cannot be accepted for its session, or an unavailable error
    /// when storage fails.
    #[expect(
        clippy::too_many_arguments,
        reason = "One validating admission keeps the turn, its immutable run selection, and its history together."
    )]
    fn accept_user_turn(
        &self,
        session_id: SessionId,
        idempotency_key: IdempotencyKey,
        content: &str,
        proposed_run_id: RunId,
        config_snapshot: ConfigSnapshotDto,
        selection: ResolvedRunProviderSelectionDto,
        reasoning_history: Option<ReasoningHistoryManifestDto>,
        occurred_at: TimestampDto,
    ) -> DtoResult<AcceptedTurnOutcomeDto>;

    /// Removes a not-yet-seen pending turn and returns its committed projection.
    ///
    /// `occurred_at` is intentionally unrecorded: a removed pending turn keeps
    /// no durable removal time, and the caller-supplied value never enters a
    /// stored column.
    ///
    /// # Errors
    ///
    /// Returns a not-found or conflict error when the pending turn cannot be
    /// removed, or an unavailable error when durable storage fails.
    fn remove_turn(
        &self,
        command: RemoveTurnCommandDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<PendingTurnProjectionDto>;

    /// Appends every pending user turn to one active run context in insertion order.
    ///
    /// Every consumed turn becomes `appended` in the same transaction that
    /// commits its user message, so a turn joins exactly one run context.
    ///
    /// # Errors
    ///
    /// Returns a conflict, not-found, or unavailable error when the scoped run
    /// cannot join its pending messages atomically.
    fn consume_pending_user_turns(
        &self,
        session_id: SessionId,
        run_id: RunId,
        occurred_at: TimestampDto,
    ) -> DtoResult<Vec<MessageProjectionDto>>;

    /// Transitions a run to one declared successor status.
    ///
    /// # Errors
    ///
    /// Returns a validation, not-found, or conflict error when the transition
    /// is invalid, or an unavailable error when storage fails.
    fn transition_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        status: RunStatusDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<RunProjectionDto>;

    /// Commits one validated terminal run outcome with its usage, finish, and
    /// error evidence.
    ///
    /// The outcome is written once: repeating the exact committed outcome is
    /// idempotent, while a different terminal outcome for the same run is a
    /// typed conflict instead of a silent overwrite of the recorded evidence.
    ///
    /// # Errors
    ///
    /// Returns a not-found, conflict, or classified storage error when the
    /// outcome cannot be committed for the run. The outcome's terminal shape
    /// is validated by [`RunOutcomeDto::new`] before this call.
    fn finish_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        outcome: RunOutcomeDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<RunProjectionDto>;

    /// Appends one transcript row and returns it as committed.
    ///
    /// # Errors
    ///
    /// Returns a validation, not-found, or conflict error when the row cannot be
    /// committed for its session and run, or an unavailable error when storage
    /// fails.
    fn append_message(
        &self,
        message: MessageProjectionDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<MessageProjectionDto>;

    /// Commits one tool result with its answering transcript row in one transaction.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the answering row is not a `tool_result`
    /// row for the same session, run, call, and tool identity, a not-found or
    /// conflict error when the scoped call cannot record its result, or an
    /// unavailable error when storage fails.
    fn write_tool_result(
        &self,
        evidence: ToolResultEvidenceDto,
        message: MessageProjectionDto,
    ) -> DtoResult<ToolResultEvidenceDto>;

    /// Loads typed evidence durably recorded for one tool call.
    ///
    /// # Errors
    ///
    /// Returns `tool_result_not_found` when no durable evidence exists for the
    /// supplied identity, or `tool_result_unavailable` when durable evidence
    /// cannot be read. No partial evidence, credentials, or backend resources
    /// cross this DTO-only boundary on failure.
    fn load_tool_result(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
    ) -> DtoResult<ToolResultEvidenceDto>;

    /// Loads the persisted credential-free configuration selected for a matching run.
    ///
    /// # Errors
    ///
    /// Returns `run_configuration_not_found` only when the durable run row is
    /// genuinely absent for the supplied identity, `storage_unavailable` when the
    /// durable backend cannot be read at all, `run_configuration_unavailable`
    /// when the persisted safe selection row is absent, and
    /// `storage_decode_failed` when the selection is present but cannot be
    /// decoded. Credentials, raw TOML, configuration paths, and backend
    /// resources never cross this boundary.
    fn load_run_config_snapshot(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<ConfigSnapshotDto>;

    /// Loads the full ordered model context for one current starting run.
    ///
    /// The returned safe immutable configuration belongs only to the target run.
    /// Messages are the committed transcript rows of the session in insertion
    /// order and end with the target run's user turn.
    ///
    /// # Errors
    ///
    /// Returns `run_model_context_unavailable` when the run is unknown,
    /// cross-session, no longer starting, or durable context cannot be read.
    /// No partial context, credentials, raw TOML, configuration paths, or backend
    /// resources cross this DTO-only boundary on failure.
    fn load_starting_run_model_context(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<StartingRunModelContextDto>;

    /// Loads the current durable projection of one run.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an unknown or cross-session run, or an
    /// unavailable error when storage cannot be read.
    fn load_run_projection(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<RunProjectionDto>;

    /// Loads the current durable projection of one session.
    ///
    /// # Errors
    ///
    /// Returns a not-found error when the session has no durable projection, or
    /// an unavailable error when storage cannot be read.
    fn load_session_projection(&self, session_id: SessionId) -> DtoResult<SessionProjectionDto>;

    /// Loads the most recent committed transcript rows of one session.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an unknown session, or an unavailable error
    /// when storage cannot be read.
    fn load_recent_messages(
        &self,
        session_id: SessionId,
        limit: u32,
    ) -> DtoResult<Vec<MessageProjectionDto>>;

    /// Loads the committed transcript rows of one run in insertion order.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an unknown or cross-session run, or an
    /// unavailable error when storage cannot be read.
    fn load_run_messages(
        &self,
        session_id: SessionId,
        run_id: RunId,
        limit: u32,
    ) -> DtoResult<Vec<MessageProjectionDto>>;

    /// Marks persisted unfinished runs interrupted at the supplied time.
    ///
    /// Every unfinished run is marked in one transaction, and the supplied time
    /// becomes each recovered run's finish time.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when recovery changes cannot be durably
    /// committed.
    fn recover_unfinished_runs(
        &self,
        recovered_at: TimestampDto,
    ) -> DtoResult<Vec<RunProjectionDto>>;

    /// Loads the current durable projection of one session together with its
    /// newest committed transcript rows in one read.
    ///
    /// The default body composes [`Self::load_session_projection`] and
    /// [`Self::load_recent_messages`]; a backend that can serve both from one
    /// committed read overrides it. Either way the returned projection and
    /// transcript rows are the committed state observed by that read.
    ///
    /// # Errors
    ///
    /// Returns the not-found, validation, or unavailable error of the
    /// underlying projection and transcript reads.
    fn load_session_snapshot(
        &self,
        session_id: SessionId,
        message_limit: u32,
    ) -> DtoResult<SessionSnapshotDto> {
        let projection = self.load_session_projection(session_id)?;
        let messages = self.load_recent_messages(session_id, message_limit)?;
        SessionSnapshotDto::with_projection(session_id, projection, messages)
    }

    /// Records an already credential-free configuration revision snapshot.
    ///
    /// # Errors
    ///
    /// Returns a validation or conflict error when the revision cannot be
    /// recorded, or an unavailable error when durable storage fails.
    fn accept_configuration_revision(&self, snapshot: ConfigSnapshotDto) -> DtoResult<()>;

    /// Accepts one validated credential-free provider catalog revision.
    ///
    /// Acceptance commits the revision's kind descriptors, profile revisions,
    /// current profile policies, and membership in one transaction, then
    /// advances the accepted pointer and appends the preparation, optional
    /// removal-acceptance, and acceptance audit records. Removal tombstones are
    /// appended for members the revision omits; a removed kind that the revision
    /// still references is rejected. Activation is a separate commit: an
    /// accepted revision that is not activated is the activation-recovery state.
    ///
    /// # Errors
    ///
    /// Returns a validation or conflict error when the revision cannot be
    /// accepted under the single-version catalog law, or an unavailable error
    /// when durable storage fails.
    fn accept_catalog_revision(
        &self,
        revision: ProviderCatalogRevisionDto,
    ) -> DtoResult<ProviderCatalogStateDto>;

    /// Marks the exact accepted catalog revision as the active one.
    ///
    /// Repeating the activation of the already-active revision changes nothing.
    /// When a different revision was active, the call records the
    /// activation-recovery taxonomy in order: the required-recovery record, the
    /// activation record, and the recovery-completed record.
    ///
    /// # Errors
    ///
    /// Returns a conflict error when the addressed revision is not the accepted
    /// one, or an unavailable error when durable storage fails.
    fn mark_catalog_activated(
        &self,
        catalog_revision_id: CatalogRevisionId,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProviderCatalogStateDto>;

    /// Records one rejected removal candidate as append-only audit evidence.
    ///
    /// A rejection never emits acceptance or activation; the recorded evidence
    /// is the pending-removal record followed by the rejection record.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when durable storage fails.
    fn record_catalog_candidate_rejected(
        &self,
        candidate_revision_id: CatalogRevisionId,
        occurred_at: TimestampDto,
    ) -> DtoResult<()>;

    /// Loads the current accepted and activated catalog pointers.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when durable storage cannot be read.
    fn load_catalog_state(&self) -> DtoResult<ProviderCatalogStateDto>;

    /// Loads one committed catalog revision with its current profile policies.
    ///
    /// # Errors
    ///
    /// Returns a not-found error when the revision is unknown, a decode failure
    /// when a stored revision cannot be decoded, or an unavailable error when
    /// durable storage cannot be read.
    fn load_catalog_revision(
        &self,
        catalog_revision_id: CatalogRevisionId,
    ) -> DtoResult<ProviderCatalogRevisionDto>;

    /// Stores the current display, enabled, and pricing policy of one member profile.
    ///
    /// Display name, enabled state, and pricing are not revision-affecting, so
    /// this call changes no catalog or profile revision.
    ///
    /// # Errors
    ///
    /// Returns a not-found error when the profile is not a member of the
    /// accepted catalog revision, or an unavailable error when durable storage
    /// fails.
    fn store_profile_policy(
        &self,
        profile_id: ProviderProfileId,
        policy: ProviderProfilePolicyDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProviderProfilePolicyDto>;

    /// Sets the durable session provider profile against its expected projection revision.
    ///
    /// The change is optimistic: a mismatched expected revision is a typed
    /// conflict and commits nothing. Setting the already-durable profile is a
    /// successful no-change outcome that leaves the revision untouched.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an unknown session, a conflict error when
    /// the expected revision does not match, or an unavailable error when
    /// durable storage fails.
    fn set_session_provider_profile(
        &self,
        session_id: SessionId,
        profile_id: ProviderProfileId,
        expected_session_projection_revision: u64,
        occurred_at: TimestampDto,
    ) -> DtoResult<SessionProviderProfileChangeDto>;

    /// Loads one session's durable provider default and projection revision.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an unknown session, or an unavailable error
    /// when durable storage cannot be read.
    fn load_session_provider_profile(
        &self,
        session_id: SessionId,
    ) -> DtoResult<SessionProviderProfileDto>;

    /// Loads one run's committed resolved provider selection.
    ///
    /// # Errors
    ///
    /// Returns a not-found error when the run or its committed selection does
    /// not exist, a decode failure when the stored selection cannot be decoded,
    /// or an unavailable error when durable storage cannot be read.
    fn load_run_provider_selection(
        &self,
        session_id: SessionId,
        run_id: RunId,
    ) -> DtoResult<ResolvedRunProviderSelectionDto>;

    /// Begins one discovery attempt before any external work.
    ///
    /// # Errors
    ///
    /// Returns a conflict error when the attempt identity is already durable, or
    /// an unavailable error when durable storage fails.
    fn begin_discovery_attempt(
        &self,
        attempt_id: ProviderDiscoveryAttemptId,
        profile_id: ProviderProfileId,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProviderDiscoveryAttemptDto>;

    /// Marks one prepared discovery attempt started before its outbound boundary.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an unknown attempt, a conflict error when
    /// the attempt accepts no start transition, or an unavailable error when
    /// durable storage fails.
    fn mark_discovery_started(
        &self,
        attempt_id: ProviderDiscoveryAttemptId,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProviderDiscoveryAttemptDto>;

    /// Completes one started discovery attempt with its bounded result records.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an unknown attempt, a conflict error when
    /// the attempt accepts no completion transition, a validation error when the
    /// record set exceeds the durable result bound, or an unavailable error when
    /// durable storage fails.
    fn complete_discovery_attempt(
        &self,
        attempt_id: ProviderDiscoveryAttemptId,
        records: Vec<ProviderModelRecordDto>,
        occurred_at: TimestampDto,
    ) -> DtoResult<intention_proto::provider::ProviderDiscoveryResultDto>;

    /// Fails one unfinished discovery attempt with its safe failure evidence.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an unknown attempt, a conflict error when
    /// the attempt is already terminal, or an unavailable error when durable
    /// storage fails.
    fn fail_discovery_attempt(
        &self,
        attempt_id: ProviderDiscoveryAttemptId,
        failure: ProviderDiscoveryFailureDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProviderDiscoveryAttemptDto>;

    /// Terminalizes every unfinished discovery attempt as interrupted.
    ///
    /// Every attempt that is not terminal becomes `Interrupted` with the
    /// supplied time inside one transaction, so a restart cannot leave a
    /// half-applied recovery. No attempt resumes or continues automatically.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when recovery changes cannot be durably
    /// committed.
    fn recover_unfinished_discovery_attempts(
        &self,
        recovered_at: TimestampDto,
    ) -> DtoResult<Vec<ProviderDiscoveryAttemptDto>>;

    /// Loads one bounded usage aggregate per (profile revision, model) identity.
    ///
    /// Aggregation is computed from terminal runs and their committed provider
    /// selections; no price, currency, or estimated cost is produced.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when durable storage cannot be read.
    fn load_profile_usage(
        &self,
        profile_id: ProviderProfileId,
    ) -> DtoResult<Vec<ProfileUsageAggregateDto>>;

    /// Loads the committed completed reasoning source steps of one session.
    ///
    /// Steps are assistant steps of completed runs in durable transcript order,
    /// so a dependent run can build its typed cross-turn history from committed
    /// source material instead of rescanning live state.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an unknown session, or an unavailable error
    /// when durable storage cannot be read.
    fn load_reasoning_history_source(
        &self,
        session_id: SessionId,
    ) -> DtoResult<Vec<ReasoningHistorySourceStepDto>>;
}
