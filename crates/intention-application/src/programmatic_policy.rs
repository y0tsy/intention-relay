//! Slice 3 policy runtime: admission transaction, confirmations, corridors.
//!
//! Owner: architecture 27 with ADR 0044. This module owns the application
//! transactions that resolve and freeze the effective policy snapshot, decide
//! admission, reserve policy counters atomically before `ToolCallStarted`,
//! release or permanently consume reservations, drive the exact-confirmation
//! and bounded-corridor flows, and apply the recovery dispositions of
//! outstanding reservations.
//!
//! # Boundary
//!
//! Every value that crosses this boundary is a bounded identity, revision,
//! bound, digest, closed decision, or canonical selector label. No credential,
//! filesystem path, provider resource, raw tool input, grant, process handle,
//! or implementation state crosses it: boundary scalars are rejected with
//! `credentials_forbidden` or `programmatic_policy_not_applicable` before any
//! repository call.
//!
//! # Frozen snapshot and idempotent replay
//!
//! The effective snapshot is resolved and frozen once at admission, and the
//! stored record is the immutable execution meaning of that run. Every
//! confirmation identity and reservation reference is deterministic over the
//! root run, the tool call, and the selected policy identity, so an equal
//! repeated operation reads the accepted binding and reserves no second unit,
//! consumes no second corridor action, and creates no second confirmation.
//!
//! # Shared runtime codecs
//!
//! `identity_text`, `identity_bytes`, `digest_text`, and `digest_bytes` are the
//! single identity and digest text codecs of the Slice 3 runtime; the harness
//! runtime in `crate::harness` uses the same functions so durable rows written
//! by either service are readable by the other.

use intention_domain::canonical::{Digest256, contains_control_or_nul, contains_credential_shape};
use intention_domain::programmatic_policy::{
    DIRECT_LOCAL_READ_TOOL_IDS, DescriptorInputConstraintSelectionV1,
    EffectiveProgrammaticCallerPolicySnapshotV1, ProgrammaticAdmissionCallV1,
    ProgrammaticAdmissionRuleV1, ProgrammaticAuthorizationCorridorV1,
    ProgrammaticCalendarPeriodKindV1, ProgrammaticCallerApplicablePolicyV1,
    ProgrammaticCallerPolicyLifecycleStateV1, ProgrammaticCallerProvenanceV1,
    ProgrammaticCorridorRootV1, ProgrammaticCorridorSelectorsV1, ProgrammaticProvenanceLinksV1,
    ProgrammaticRecoveryDispositionV1, ProgrammaticRootOriginKindV1, ProgrammaticRuleSelectorV1,
    ProgrammaticRunLimitsV1, ProgrammaticToolEffectFlagV1, new_provenance,
    recovery_disposition_for, resolve_effective_policy_snapshot,
    validate_bounded_corridor_admission, validate_corridor_active, validate_direct_local_read_tool,
    validate_harness_delegation, validate_interaction_admission, validate_live_policy_state,
    validate_scope_applicability,
};
use intention_domain::slice3_selections::ProgrammaticCallerRootOriginV1;
use intention_storage::programmatic_policy_repo::{
    CommitProgrammaticReservationStartedInputDto, ConsumeProgrammaticCorridorActionInputDto,
    DecideProgrammaticPolicyConfirmationInputDto, MAX_POLICY_SNAPSHOT_REFERENCES,
    ProgrammaticAdmissionDecisionDto, ProgrammaticAdmissionRepositoryDto,
    ProgrammaticAuthorizationCorridorRecordDto, ProgrammaticCalendarCounterReferenceDto,
    ProgrammaticCalendarLimitRecordDto, ProgrammaticCalendarPeriodKindDto,
    ProgrammaticConfirmationRepositoryDto, ProgrammaticConfirmationStateDto,
    ProgrammaticCorridorStateDto, ProgrammaticPolicyConfirmationRecordDto,
    ProgrammaticPolicyCounterRecordDto, ProgrammaticPolicyLifecycleStateDto,
    ProgrammaticPolicyRepositoryDto, ProgrammaticPolicyReservationRecordDto,
    ProgrammaticPolicyRevisionReferenceDto, ProgrammaticPolicySnapshotRecordDto,
    ProgrammaticReservationStateDto, ProgrammaticRootOriginKindDto,
    RecoverProgrammaticPolicyReservationInputDto, ReleaseProgrammaticPolicyReservationInputDto,
    ReserveProgrammaticPolicyActionInputDto, ReserveProgrammaticPolicyActionOutcomeDto,
};
use intention_types::{DtoResult, ErrorDto};

/// The code-owned validity window of one exact confirmation.
///
/// An accepted confirmation authorizes its one bound call only inside this
/// window after the user decision; a later call needs a new decision.
pub const EXACT_CONFIRMATION_VALIDITY_MS: u64 = 15 * 60 * 1_000;

/// The maximum characters of one boundary scalar.
const MAX_BOUNDARY_TEXT_CHARS: usize = 256;

/// The repository-wide safe credential rejection code.
const CREDENTIALS_FORBIDDEN: &str = "credentials_forbidden";

/// Builds one typed pre-effect policy rejection.
fn policy_error(code: &'static str, message: &'static str) -> ErrorDto {
    ErrorDto::validation(code, message)
}

/// Formats one daemon-assigned identity as canonical UUID text.
#[must_use]
pub(crate) fn identity_text(identity: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(36);
    for (index, byte) in identity.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            text.push('-');
        }
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

/// Parses canonical UUID text into one daemon-assigned identity.
///
/// # Errors
///
/// Returns `invalid_runtime_identity` when the text is not thirty-two
/// hexadecimal digits with optional canonical separators.
pub(crate) fn identity_bytes(value: &str) -> DtoResult<[u8; 16]> {
    fn invalid() -> ErrorDto {
        policy_error("invalid_runtime_identity", "identity text is not canonical")
    }
    let digits: Vec<u8> = value.bytes().filter(|byte| *byte != b'-').collect();
    if digits.len() != 32 {
        return Err(invalid());
    }
    let mut identity = [0_u8; 16];
    for (index, chunk) in digits.chunks_exact(2).enumerate() {
        let high = hex_value(chunk[0]).ok_or_else(invalid)?;
        let low = hex_value(chunk[1]).ok_or_else(invalid)?;
        identity[index] = (high << 4) | low;
    }
    Ok(identity)
}

/// Returns one hexadecimal digit value.
const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// Formats one digest as its canonical `sha256:<64 lowercase hex>` text.
#[must_use]
pub(crate) fn digest_text(digest: Digest256) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(71);
    text.push_str("sha256:");
    for byte in digest.bytes() {
        text.push(char::from(HEX[usize::from(byte >> 4)]));
        text.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    text
}

/// Parses one canonical `sha256:<64 lowercase hex>` digest.
///
/// # Errors
///
/// Returns `invalid_runtime_digest` for a value outside the canonical form.
pub(crate) fn digest_bytes(value: &str) -> DtoResult<Digest256> {
    let invalid = || policy_error("invalid_runtime_digest", "digest text is not canonical");
    let hex = value.strip_prefix("sha256:").ok_or_else(invalid)?;
    Digest256::from_str_hex(hex).map_err(|_| invalid())
}

/// Derives one deterministic daemon-assigned identity from a domain digest.
#[must_use]
pub(crate) fn derived_identity(digest: Digest256) -> [u8; 16] {
    let bytes = digest.bytes();
    let mut identity = [0_u8; 16];
    identity.copy_from_slice(&bytes[..16]);
    identity
}

/// Appends one length-framed text field to a deterministic digest input.
pub(crate) fn push_framed(input: &mut Vec<u8>, value: &str) {
    input.extend_from_slice(&u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    input.extend_from_slice(value.as_bytes());
}

/// Validates one bounded credential-free and path-free boundary scalar.
///
/// # Errors
///
/// Returns `programmatic_policy_not_applicable` for a blank, control-bearing,
/// or path-shaped value, `programmatic_policy_limit_exceeded` for an over-long
/// value, and `credentials_forbidden` for a credential-shaped value.
fn validate_boundary_scalar(value: &str, message: &'static str) -> DtoResult<()> {
    if value.trim().is_empty() || contains_control_or_nul(value) {
        return Err(policy_error("programmatic_policy_not_applicable", message));
    }
    if value.chars().count() > MAX_BOUNDARY_TEXT_CHARS {
        return Err(policy_error(
            "programmatic_policy_limit_exceeded",
            "a policy boundary scalar stays inside its closed bound",
        ));
    }
    if contains_credential_shape(value) {
        return Err(policy_error(
            CREDENTIALS_FORBIDDEN,
            "credentials are forbidden",
        ));
    }
    if names_filesystem_path(value) {
        return Err(policy_error(
            "programmatic_policy_not_applicable",
            "a filesystem path never crosses the policy admission boundary",
        ));
    }
    Ok(())
}

/// Whether one boundary scalar names a filesystem path.
///
/// A descriptor-declared typed input constraint is a closed safe value, never
/// an unvalidated path, so path-shaped input is refused at this boundary.
fn names_filesystem_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with('\\')
        || value.contains("..")
        || value.as_bytes().get(1).is_some_and(|byte| *byte == b':')
}

/// Validates every boundary scalar of one admission call.
///
/// # Errors
///
/// Returns the boundary failures of [`validate_boundary_scalar`].
fn validate_call_boundary(call: &ProgrammaticAdmissionCallV1) -> DtoResult<()> {
    validate_boundary_scalar(&call.tool_id, "a tool identity must name a registry tool")?;
    for constraint in &call.typed_input_constraints {
        validate_boundary_scalar(&constraint.family, "a constraint family needs a safe name")?;
        for value in &constraint.typed_values {
            validate_boundary_scalar(value, "a typed input constraint needs a safe value")?;
        }
    }
    if let Some(method) = &call.mcp_method {
        validate_boundary_scalar(&method.method_reference, "an MCP method needs a safe name")?;
        validate_boundary_scalar(
            &method.method_schema_revision,
            "an MCP method needs a safe schema revision",
        )?;
    }
    Ok(())
}

/// Returns the canonical label of one declared effect flag.
const fn effect_flag_label(flag: ProgrammaticToolEffectFlagV1) -> &'static str {
    match flag {
        ProgrammaticToolEffectFlagV1::LocalRead => "local_read",
        ProgrammaticToolEffectFlagV1::LocalWrite => "local_write",
        ProgrammaticToolEffectFlagV1::LocalExecute => "local_execute",
        ProgrammaticToolEffectFlagV1::NetworkAccess => "network_access",
        ProgrammaticToolEffectFlagV1::ChildDelegation => "child_delegation",
        ProgrammaticToolEffectFlagV1::UserInteraction => "user_interaction",
        ProgrammaticToolEffectFlagV1::McpInvocation => "mcp_invocation",
    }
}

/// Returns the canonical label of one rule or corridor exact selector.
fn selector_label(selector: &ProgrammaticRuleSelectorV1) -> String {
    match selector {
        ProgrammaticRuleSelectorV1::EffectProfile {
            required_effect_flags,
        } => {
            let mut labels: Vec<&str> = required_effect_flags
                .iter()
                .copied()
                .map(effect_flag_label)
                .collect();
            labels.sort_unstable();
            format!("effect:{}", labels.join("+"))
        }
        ProgrammaticRuleSelectorV1::ExactTool {
            tool_id,
            descriptor_revision,
        } => format!("tool:{tool_id}@{descriptor_revision}"),
        ProgrammaticRuleSelectorV1::McpMethod { reference } => format!(
            "mcp:{}:{}:{}",
            identity_text(reference.connection_reference),
            reference.method_reference,
            reference.method_schema_revision
        ),
    }
}

/// Returns the canonical label of one typed input constraint selection.
///
/// The bounded typed values collapse into one deterministic digest, so a label
/// stays inside its closed scalar bound regardless of how many values the
/// descriptor-declared family selected.
fn constraint_label(constraint: &DescriptorInputConstraintSelectionV1) -> String {
    let mut input = Vec::new();
    push_framed(&mut input, "programmatic-caller-constraint-v1");
    push_framed(&mut input, &constraint.family);
    push_framed(&mut input, &constraint.revision.to_string());
    let mut values = constraint.typed_values.clone();
    values.sort_unstable();
    values.dedup();
    for value in &values {
        push_framed(&mut input, value);
    }
    format!(
        "constraint:{}@{}#{}",
        constraint.family,
        constraint.revision,
        identity_text(derived_identity(Digest256::sha256(&input)))
    )
}

/// Returns the canonical label of one exact tool call selector.
fn call_tool_label(call: &ProgrammaticAdmissionCallV1) -> String {
    format!("tool:{}@{}", call.tool_id, call.descriptor_revision)
}

/// Returns the canonical label of one call's MCP method selector, if any.
fn call_mcp_label(call: &ProgrammaticAdmissionCallV1) -> Option<String> {
    call.mcp_method.as_ref().map(|reference| {
        format!(
            "mcp:{}:{}:{}",
            identity_text(reference.connection_reference),
            reference.method_reference,
            reference.method_schema_revision
        )
    })
}

/// Converts one durable lifecycle discriminator into its domain state.
const fn lifecycle_from_storage(
    state: ProgrammaticPolicyLifecycleStateDto,
) -> ProgrammaticCallerPolicyLifecycleStateV1 {
    match state {
        ProgrammaticPolicyLifecycleStateDto::Active => {
            ProgrammaticCallerPolicyLifecycleStateV1::Active
        }
        ProgrammaticPolicyLifecycleStateDto::Suspended => {
            ProgrammaticCallerPolicyLifecycleStateV1::Suspended
        }
        ProgrammaticPolicyLifecycleStateDto::Revoked => {
            ProgrammaticCallerPolicyLifecycleStateV1::Revoked
        }
        ProgrammaticPolicyLifecycleStateDto::Archived => {
            ProgrammaticCallerPolicyLifecycleStateV1::Archived
        }
    }
}

/// Converts one domain admission decision into its durable discriminator.
const fn decision_to_storage(
    decision: ProgrammaticAdmissionRuleV1,
) -> ProgrammaticAdmissionDecisionDto {
    match decision {
        ProgrammaticAdmissionRuleV1::Prohibited => ProgrammaticAdmissionDecisionDto::Prohibited,
        ProgrammaticAdmissionRuleV1::DirectLocalRead => {
            ProgrammaticAdmissionDecisionDto::DirectLocalRead
        }
        ProgrammaticAdmissionRuleV1::BoundedConfirmationRequired => {
            ProgrammaticAdmissionDecisionDto::BoundedConfirmationRequired
        }
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired => {
            ProgrammaticAdmissionDecisionDto::ExactConfirmationRequired
        }
    }
}

/// Resolves the effective decision of one call under one frozen snapshot.
///
/// The documented contract is the most restrictive of the code-owned
/// interactive root-run local-read baseline (when the call is eligible), the
/// snapshot's decision ceiling, and every matching rule entry; a call that
/// neither carries the baseline nor matches a selected rule stays
/// `Prohibited`.
///
/// The domain `resolve_snapshot_decision` helper seeds every ineligible call
/// at `Prohibited` and only ever tightens, so a call covered by a
/// confirmation or corridor rule could never resolve to the decision that
/// admits it. The runtime resolves the documented intersection here against
/// the same frozen domain primitives instead; a domain-side fix of that
/// helper can replace this function without touching the service.
fn effective_admission_decision(
    snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
    call: &ProgrammaticAdmissionCallV1,
) -> ProgrammaticAdmissionRuleV1 {
    let baseline_covered = snapshot.baseline.is_some()
        && call.context.root_origin == ProgrammaticRootOriginKindV1::InteractiveUser
        && call.context.is_root_run_call()
        && DIRECT_LOCAL_READ_TOOL_IDS.contains(&call.tool_id.as_str());
    let mut decision = if baseline_covered {
        ProgrammaticAdmissionRuleV1::DirectLocalRead
    } else {
        snapshot.decision_ceiling
    };
    decision = decision.most_restrictive(snapshot.decision_ceiling);
    let mut covered = baseline_covered;
    for rule in &snapshot.ordered_rules {
        if rule.matches(call) {
            decision = decision.most_restrictive(rule.decision_for(call.context.root_origin));
            covered = true;
        }
    }
    if covered {
        decision
    } else {
        ProgrammaticAdmissionRuleV1::Prohibited
    }
}

/// Converts one domain calendar period kind into its durable discriminator.
const fn period_to_storage(
    period: ProgrammaticCalendarPeriodKindV1,
) -> ProgrammaticCalendarPeriodKindDto {
    match period {
        ProgrammaticCalendarPeriodKindV1::Day => ProgrammaticCalendarPeriodKindDto::Day,
        ProgrammaticCalendarPeriodKindV1::Week => ProgrammaticCalendarPeriodKindDto::Week,
        ProgrammaticCalendarPeriodKindV1::Month => ProgrammaticCalendarPeriodKindDto::Month,
    }
}

/// Converts one domain root-origin kind into its durable discriminator.
const fn origin_to_storage(kind: ProgrammaticRootOriginKindV1) -> ProgrammaticRootOriginKindDto {
    match kind {
        ProgrammaticRootOriginKindV1::InteractiveUser => {
            ProgrammaticRootOriginKindDto::InteractiveUser
        }
        ProgrammaticRootOriginKindV1::ContinualHarness => {
            ProgrammaticRootOriginKindDto::ContinualHarness
        }
    }
}

/// The calendar window one admission reserves against.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticCalendarWindowDto {
    /// The current window start in Unix milliseconds.
    pub start_ms: u64,
    /// The current window end in Unix milliseconds.
    pub end_ms: u64,
    /// The project time zone retained by the current window.
    pub time_zone: String,
}

impl ProgrammaticCalendarWindowDto {
    /// Validates this calendar window.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_counter_unavailable` for an incoherent
    /// window and the boundary failures of the retained time zone.
    pub fn validate(&self) -> DtoResult<()> {
        if self.end_ms <= self.start_ms {
            return Err(policy_error(
                "programmatic_policy_counter_unavailable",
                "a calendar window has a positive bounded duration",
            ));
        }
        validate_boundary_scalar(
            &self.time_zone,
            "a calendar window keeps its project time zone",
        )
    }
}

/// The complete typed input of one pre-start policy admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticAdmissionRequestDto {
    /// The one daemon-assigned root origin of this calling path.
    pub root_origin: ProgrammaticCallerRootOriginV1,
    /// The typed admission call.
    pub call: ProgrammaticAdmissionCallV1,
    /// The immutable parent links of this calling path.
    pub links: ProgrammaticProvenanceLinksV1,
    /// The daemon-assigned identity of the snapshot frozen by this admission.
    pub snapshot_id: String,
    /// The daemon-assigned safe admission basis reference.
    pub admission_basis_reference: String,
    /// The applicable policy selections resolved by the daemon.
    pub applicable_policies: Vec<ProgrammaticCallerApplicablePolicyV1>,
    /// The owning project identity of the calling context.
    pub project_id: [u8; 16],
    /// The current session identity of the calling context.
    pub session_id: [u8; 16],
    /// The frozen leading goal chain of the calling context.
    pub leading_goal_chain: Vec<[u8; 16]>,
    /// Whether the session inherits the policy through explicit fork lineage.
    pub inherited_from_source_session: bool,
    /// Whether the selected descriptor declares a closed typed input family.
    pub descriptor_constraint_family_declared: bool,
    /// The calendar window this admission reserves against.
    pub calendar_window: ProgrammaticCalendarWindowDto,
    /// The admission time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

impl ProgrammaticAdmissionRequestDto {
    /// Validates the daemon-assigned identities and boundary scalars.
    ///
    /// # Errors
    ///
    /// Returns the boundary failures of `validate_boundary_scalar`, the
    /// window failures of [`ProgrammaticCalendarWindowDto::validate`], and the
    /// call boundary failures of `validate_call_boundary`.
    pub fn validate(&self) -> DtoResult<()> {
        validate_boundary_scalar(&self.snapshot_id, "a snapshot needs a safe identity")?;
        validate_boundary_scalar(
            &self.admission_basis_reference,
            "an admission basis needs a safe identity",
        )?;
        validate_call_boundary(&self.call)?;
        self.calendar_window.validate()
    }
}

/// One safe admission evidence record created with the admission outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticAdmissionEvidenceDto {
    /// The resolved root-origin kind.
    pub root_origin_kind: ProgrammaticRootOriginKindV1,
    /// The most restrictive effective decision that admitted the call.
    pub decision: ProgrammaticAdmissionRuleV1,
    /// The frozen effective snapshot identity; `None` for the code-owned
    /// interactive local-read baseline.
    pub snapshot_id: Option<String>,
    /// The frozen effective snapshot digest.
    pub snapshot_digest: Digest256,
    /// The frozen narrowest per-run limits.
    pub run_limits: ProgrammaticRunLimitsV1,
    /// The immutable audit provenance of this action.
    pub provenance: ProgrammaticCallerProvenanceV1,
    /// The policy-counter reservation references consumed by this admission.
    pub reservations: Vec<String>,
    /// The active corridor digest when the admission used a corridor.
    pub corridor_digest: Option<String>,
    /// Whether the equal repeated operation replayed an accepted binding.
    pub replayed: bool,
    /// The admission time in Unix milliseconds.
    pub admitted_at_ms: u64,
}

/// The typed outcome of one pre-start policy admission attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgrammaticAdmissionOutcomeDto {
    /// The call is admitted and its policy counters are reserved.
    Admitted(Box<ProgrammaticAdmissionEvidenceDto>),
    /// The call awaits one exact confirmation; nothing was reserved.
    ConfirmationRequired(Box<ProgrammaticPolicyConfirmationRecordDto>),
}

/// One request creating the exact confirmation of one bound call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticExactConfirmationRequestDto {
    /// The admission request whose exact call is confirmed.
    pub request: ProgrammaticAdmissionRequestDto,
}

/// One request creating a user-approved bounded confirmation corridor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticCorridorApprovalInputDto {
    /// The one active root session identity of the corridor.
    pub root_session_id: [u8; 16],
    /// The one active root run identity of the corridor.
    pub root_run_id: [u8; 16],
    /// The root-origin kind of the active tree.
    pub root_origin_kind: ProgrammaticRootOriginKindV1,
    /// The effective policy snapshot digest selected by the root.
    pub effective_policy_snapshot_digest: Digest256,
    /// The effective policy snapshot identity selected by the root.
    pub effective_policy_snapshot_reference: String,
    /// The required declared effect flags of every covered call.
    pub required_effect_selectors: Vec<ProgrammaticToolEffectFlagV1>,
    /// The exact tool or MCP method selectors of every covered call.
    pub exact_tool_or_mcp_method_selectors: Vec<ProgrammaticRuleSelectorV1>,
    /// The selected typed input constraints of every covered call.
    pub descriptor_input_constraint_selections: Vec<DescriptorInputConstraintSelectionV1>,
    /// The shared maximum action count.
    pub maximum_action_count: u64,
    /// The shared maximum concurrent action count.
    pub maximum_concurrent_actions: u64,
    /// The accepted confirmation identity that approved this corridor.
    pub confirmation_reference: String,
    /// The tool-call identity of that accepted confirmation.
    pub confirmation_tool_call_id: String,
    /// The approval time in Unix milliseconds.
    pub occurred_at_ms: u64,
}

/// The typed recovery input of the outstanding reservations of one root run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticReservationRecoveryInputDto {
    /// The exact root run identity whose reservations are recovered.
    pub root_run_id: String,
    /// The tool-call identities that already reached `ToolCallStarted`.
    pub started_tool_call_ids: Vec<String>,
    /// The recovery time in Unix milliseconds.
    pub recovered_at_ms: u64,
}

/// One applied recovery disposition of an outstanding reservation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticReservationRecoveryDto {
    /// The recovered reservation reference.
    pub reservation_reference: String,
    /// The applied disposition.
    pub disposition: ProgrammaticRecoveryDispositionV1,
    /// Whether external work of this action may be resumed. Always false.
    pub external_work_resumes: bool,
}

/// The typed outcome of one known pre-effect reservation release.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticReservationReleaseDto {
    /// The released reservation record.
    pub reservation: ProgrammaticPolicyReservationRecordDto,
    /// Whether the release applied to an outstanding reservation.
    pub released_before_start: bool,
}

/// The typed outcome of one `ToolCallStarted` permanence commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgrammaticReservationStartDto {
    /// The permanently consumed reservation record.
    pub reservation: ProgrammaticPolicyReservationRecordDto,
    /// Whether the consumption stays permanent through cancellation or loss.
    pub permanent: bool,
}

/// The policy admission service over DTO-only durable repository contracts.
pub struct ProgrammaticPolicyAdmissionService<'a, Policies, Confirmations, Admissions>
where
    Policies: ProgrammaticPolicyRepositoryDto,
    Confirmations: ProgrammaticConfirmationRepositoryDto,
    Admissions: ProgrammaticAdmissionRepositoryDto,
{
    policies: &'a Policies,
    confirmations: &'a Confirmations,
    admissions: &'a Admissions,
}

impl<'a, Policies, Confirmations, Admissions>
    ProgrammaticPolicyAdmissionService<'a, Policies, Confirmations, Admissions>
where
    Policies: ProgrammaticPolicyRepositoryDto,
    Confirmations: ProgrammaticConfirmationRepositoryDto,
    Admissions: ProgrammaticAdmissionRepositoryDto,
{
    /// Creates the policy admission service over its durable repositories.
    #[must_use]
    pub const fn new(
        policies: &'a Policies,
        confirmations: &'a Confirmations,
        admissions: &'a Admissions,
    ) -> Self {
        Self {
            policies,
            confirmations,
            admissions,
        }
    }

    /// Resolves the applicable immutable snapshot for one admission request.
    ///
    /// Only policies whose scope covers the calling context participate; a
    /// cross-project, uncovered-goal, or non-inherited session policy is
    /// rejected with `programmatic_policy_not_applicable`.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_revision_conflict` for an incoherent
    /// applicable selection, `programmatic_policy_not_applicable` for an
    /// inapplicable scope, `programmatic_policy_snapshot_too_large` beyond the
    /// closed snapshot reference bound, and
    /// `programmatic_policy_snapshot_unavailable` when no selected policy
    /// covers a harness root origin.
    pub fn resolve_snapshot(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
    ) -> DtoResult<EffectiveProgrammaticCallerPolicySnapshotV1> {
        let mut applicable = Vec::new();
        for policy in &request.applicable_policies {
            if policy.policy_id != policy.revision_record.policy_id
                || policy.revision != policy.revision_record.revision
            {
                return Err(policy_error(
                    "programmatic_policy_revision_conflict",
                    "an applicable policy must name its exact revision record",
                ));
            }
            validate_scope_applicability(
                &policy.scope,
                request.project_id,
                request.session_id,
                &request.leading_goal_chain,
                request.inherited_from_source_session,
            )?;
            applicable.push(policy.clone());
        }
        if applicable.len() > MAX_POLICY_SNAPSHOT_REFERENCES {
            return Err(policy_error(
                "programmatic_policy_snapshot_too_large",
                "an effective snapshot stays inside its selected-record bounds",
            ));
        }
        resolve_effective_policy_snapshot(
            ProgrammaticRootOriginKindV1::of(&request.root_origin),
            &applicable,
        )
    }

    /// Atomically decides and reserves one programmatic action before
    /// `ToolCallStarted`.
    ///
    /// The frozen snapshot, the live policy state, the closed decision, the
    /// selected confirmation or corridor, every applicable run and calendar
    /// limit, and the policy-counter reservations are verified in this order;
    /// a failure before `ToolCallStarted` reserves nothing, and an equal
    /// repeated operation returns the accepted binding as a replay.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_not_applicable` for a prohibited decision
    /// or an inapplicable scope, `programmatic_policy_origin_invalid` for a
    /// provenance or root-origin mismatch, `programmatic_policy_suspended` and
    /// `programmatic_policy_revoked` for a live denial,
    /// `programmatic_policy_root_only_interaction` and
    /// `programmatic_policy_harness_delegation_forbidden` for an unauthorized
    /// interaction or harness delegation, the
    /// `programmatic_policy_confirmation_*` and
    /// `programmatic_policy_corridor_*` denials, the
    /// `programmatic_policy_counter_unavailable`,
    /// `programmatic_policy_run_limit_exceeded`,
    /// `programmatic_policy_calendar_limit_exceeded`, and
    /// `programmatic_policy_reservation_conflict` counter denials, and the
    /// typed boundary, snapshot, or repository failures.
    pub fn admit_before_start(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
    ) -> DtoResult<ProgrammaticAdmissionOutcomeDto> {
        request.validate()?;
        let snapshot = self.resolve_snapshot(request)?;
        self.validate_live_policies(&snapshot, false)?;
        let decision = effective_admission_decision(&snapshot, &request.call);
        if decision == ProgrammaticAdmissionRuleV1::Prohibited {
            return Err(policy_error(
                "programmatic_policy_not_applicable",
                "no applicable policy rule admits this call",
            ));
        }
        self.validate_interaction_rules(request)?;
        match decision {
            ProgrammaticAdmissionRuleV1::Prohibited => Err(policy_error(
                "programmatic_policy_not_applicable",
                "no applicable policy rule admits this call",
            )),
            ProgrammaticAdmissionRuleV1::DirectLocalRead => {
                validate_direct_local_read_tool(snapshot.root_origin_kind, &request.call.tool_id)?;
                if let Some(replayed) = self.replay_binding(request, &snapshot)? {
                    return Ok(ProgrammaticAdmissionOutcomeDto::Admitted(Box::new(
                        replayed,
                    )));
                }
                Ok(ProgrammaticAdmissionOutcomeDto::Admitted(Box::new(
                    self.admit_direct(request, &snapshot)?,
                )))
            }
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired => {
                if let Some(replayed) = self.replay_binding(request, &snapshot)? {
                    return Ok(ProgrammaticAdmissionOutcomeDto::Admitted(Box::new(
                        replayed,
                    )));
                }
                match self.confirmation_state(request, &snapshot)? {
                    Some(confirmation) => {
                        Ok(ProgrammaticAdmissionOutcomeDto::ConfirmationRequired(
                            Box::new(confirmation),
                        ))
                    }
                    None => Ok(ProgrammaticAdmissionOutcomeDto::Admitted(Box::new(
                        self.admit_direct(request, &snapshot)?,
                    ))),
                }
            }
            ProgrammaticAdmissionRuleV1::BoundedConfirmationRequired => {
                if let Some(replayed) = self.replay_binding(request, &snapshot)? {
                    return Ok(ProgrammaticAdmissionOutcomeDto::Admitted(Box::new(
                        replayed,
                    )));
                }
                Ok(ProgrammaticAdmissionOutcomeDto::Admitted(Box::new(
                    self.admit_through_corridor(request, &snapshot)?,
                )))
            }
        }
    }

    /// Creates or coalesces the exact confirmation of one bound call.
    ///
    /// An equal repeated request reads and returns the one existing
    /// confirmation; a different identity for the same call fails closed
    /// rather than binding a second exact confirmation.
    ///
    /// # Errors
    ///
    /// Returns the typed boundary, snapshot, or repository failures.
    pub fn request_exact_confirmation(
        &self,
        input: &ProgrammaticExactConfirmationRequestDto,
    ) -> DtoResult<ProgrammaticPolicyConfirmationRecordDto> {
        input.request.validate()?;
        let snapshot = self.resolve_snapshot(&input.request)?;
        let existing = self
            .confirmations
            .load_programmatic_policy_confirmation(identity_text(
                input.request.call.context.tool_call_id,
            ))?;
        if let Some(existing) = existing {
            return Ok(existing);
        }
        self.create_awaiting_confirmation(&input.request, &snapshot)
    }

    /// Decides the one exact confirmation of a bound call.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_confirmation_required` for a decision that
    /// is not an accept or reject, and the typed repository failures of the
    /// exact binding check.
    pub fn decide_exact_confirmation(
        &self,
        input: &DecideProgrammaticPolicyConfirmationInputDto,
        state: ProgrammaticConfirmationStateDto,
    ) -> DtoResult<ProgrammaticPolicyConfirmationRecordDto> {
        match state {
            ProgrammaticConfirmationStateDto::Accepted
            | ProgrammaticConfirmationStateDto::Rejected => {}
            ProgrammaticConfirmationStateDto::Awaiting
            | ProgrammaticConfirmationStateDto::Expired
            | ProgrammaticConfirmationStateDto::Cancelled => {
                return Err(policy_error(
                    "programmatic_policy_confirmation_required",
                    "a confirmation decision is an accept or a reject",
                ));
            }
        }
        let decision = DecideProgrammaticPolicyConfirmationInputDto {
            state,
            ..input.clone()
        };
        self.confirmations
            .decide_programmatic_policy_confirmation(decision)
    }

    /// Creates one user-approved bounded confirmation corridor.
    ///
    /// The corridor is bounded by the closed 256/16 action bound, belongs to
    /// exactly one active root tree, and requires an accepted exact
    /// confirmation of the corridor's own approval call.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_confirmation_required` when the approving
    /// confirmation is absent, not accepted, or bound to a different
    /// identity, `programmatic_policy_limit_exceeded` for out-of-bound action
    /// counts or selector sets, and the typed boundary or repository failures.
    pub fn approve_corridor(
        &self,
        input: &ProgrammaticCorridorApprovalInputDto,
    ) -> DtoResult<ProgrammaticAuthorizationCorridorRecordDto> {
        validate_boundary_scalar(
            &input.effective_policy_snapshot_reference,
            "a corridor needs a safe snapshot reference",
        )?;
        validate_boundary_scalar(
            &input.confirmation_reference,
            "a corridor needs a safe confirmation reference",
        )?;
        self.require_accepted_confirmation(
            &input.confirmation_tool_call_id,
            &input.confirmation_reference,
        )?;
        let confirmation_reference = identity_bytes(&input.confirmation_reference)?;
        let corridor = ProgrammaticAuthorizationCorridorV1::new(
            ProgrammaticCorridorRootV1 {
                root_session_id: input.root_session_id,
                root_run_id: input.root_run_id,
                root_origin_kind: input.root_origin_kind,
                effective_policy_snapshot_digest: input.effective_policy_snapshot_digest,
            },
            ProgrammaticCorridorSelectorsV1::new(
                input.required_effect_selectors.clone(),
                input.exact_tool_or_mcp_method_selectors.clone(),
                input.descriptor_input_constraint_selections.clone(),
            )?,
            input.maximum_action_count,
            input.maximum_concurrent_actions,
            confirmation_reference,
        )?;
        let mut required_effect_selectors: Vec<String> = input
            .required_effect_selectors
            .iter()
            .copied()
            .map(effect_flag_label)
            .map(str::to_owned)
            .collect();
        required_effect_selectors.sort_unstable();
        let record = ProgrammaticAuthorizationCorridorRecordDto {
            corridor_digest: digest_text(corridor.canonical_corridor_digest),
            root_session_id: identity_text(input.root_session_id),
            root_run_id: identity_text(input.root_run_id),
            root_origin_kind: origin_to_storage(input.root_origin_kind),
            policy_snapshot_reference: input.effective_policy_snapshot_reference.clone(),
            required_effect_selectors,
            exact_tool_or_method_selectors: input
                .exact_tool_or_mcp_method_selectors
                .iter()
                .map(selector_label)
                .collect(),
            descriptor_input_constraint_selections: input
                .descriptor_input_constraint_selections
                .iter()
                .map(constraint_label)
                .collect(),
            maximum_action_count: corridor.maximum_action_count,
            maximum_concurrent_actions: corridor.maximum_concurrent_actions,
            confirmation_reference: input.confirmation_reference.clone(),
            state: ProgrammaticCorridorStateDto::Active,
            consumed_action_count: 0,
            created_at_ms: input.occurred_at_ms,
        };
        record.validate()?;
        self.confirmations
            .create_programmatic_authorization_corridor(record)
    }

    /// Releases one outstanding reservation on a known pre-effect outcome.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_reservation_conflict` when the named call
    /// does not match the reservation binding, and the typed repository
    /// failures of the release.
    pub fn release_before_start(
        &self,
        input: &ReleaseProgrammaticPolicyReservationInputDto,
    ) -> DtoResult<ProgrammaticReservationReleaseDto> {
        let reservation = self
            .admissions
            .release_programmatic_policy_reservation(input.clone())?;
        Ok(ProgrammaticReservationReleaseDto {
            reservation,
            released_before_start: true,
        })
    }

    /// Makes one reservation permanently consumed on `ToolCallStarted`.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_reservation_conflict` when the named call
    /// does not match the reservation binding, and the typed repository
    /// failures of the permanence commit.
    pub fn commit_tool_call_started(
        &self,
        input: &CommitProgrammaticReservationStartedInputDto,
    ) -> DtoResult<ProgrammaticReservationStartDto> {
        let reservation = self
            .admissions
            .commit_programmatic_reservation_started(input.clone())?;
        Ok(ProgrammaticReservationStartDto {
            reservation,
            permanent: true,
        })
    }

    /// Applies the recovery disposition of every recoverable reservation.
    ///
    /// A `Reserved` reservation is released atomically by recovery; a
    /// `PermanentOnStart` ambiguous action keeps its permanent consumption,
    /// becomes `ExternalEffectUnknown`, and is never retried or re-reserved.
    /// Every terminal reservation is skipped, so a repeated recovery changes
    /// nothing.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_reservation_conflict` when a listed
    /// reservation is not recoverable, and the typed repository failures of
    /// the recovery.
    pub fn recover_outstanding(
        &self,
        input: &ProgrammaticReservationRecoveryInputDto,
    ) -> DtoResult<Vec<ProgrammaticReservationRecoveryDto>> {
        let reservations = self
            .admissions
            .load_programmatic_policy_reservations_for_run(input.root_run_id.clone())?;
        let mut recovered = Vec::new();
        for reservation in reservations {
            let recoverable = matches!(
                reservation.state,
                ProgrammaticReservationStateDto::Reserved
                    | ProgrammaticReservationStateDto::PermanentOnStart
            );
            if !recoverable {
                continue;
            }
            let tool_call_started = input
                .started_tool_call_ids
                .contains(&reservation.tool_call_id);
            let disposition = recovery_disposition_for(tool_call_started);
            self.admissions.recover_programmatic_policy_reservation(
                RecoverProgrammaticPolicyReservationInputDto {
                    reservation_reference: reservation.reservation_reference.clone(),
                    tool_call_started,
                    recovered_at_ms: input.recovered_at_ms,
                },
            )?;
            recovered.push(ProgrammaticReservationRecoveryDto {
                reservation_reference: reservation.reservation_reference,
                disposition,
                external_work_resumes: false,
            });
        }
        Ok(recovered)
    }

    /// Closes every corridor of one terminalized root tree.
    ///
    /// # Errors
    ///
    /// Returns `programmatic_policy_corridor_unavailable` for a state that is
    /// not a terminal close and the typed repository failures of the atomic
    /// close.
    pub fn close_corridors(
        &self,
        root_run_id: &str,
        state: ProgrammaticCorridorStateDto,
        closed_at_ms: u64,
    ) -> DtoResult<u64> {
        match state {
            ProgrammaticCorridorStateDto::Expired | ProgrammaticCorridorStateDto::Revoked => {}
            ProgrammaticCorridorStateDto::Active | ProgrammaticCorridorStateDto::Exhausted => {
                return Err(policy_error(
                    "programmatic_policy_corridor_unavailable",
                    "a terminalized root tree closes its corridors as expired or revoked",
                ));
            }
        }
        self.confirmations.close_programmatic_corridors_for_root(
            root_run_id.to_owned(),
            state,
            closed_at_ms,
        )
    }

    /// Freezes one effective snapshot into its durable immutable record.
    fn freeze_snapshot(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
        snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
    ) -> DtoResult<()> {
        if snapshot.policy_references.is_empty() {
            return Ok(());
        }
        let record = ProgrammaticPolicySnapshotRecordDto {
            snapshot_id: request.snapshot_id.clone(),
            root_origin_kind: origin_to_storage(snapshot.root_origin_kind),
            policy_references: snapshot
                .policy_references
                .iter()
                .map(|reference| ProgrammaticPolicyRevisionReferenceDto {
                    policy_id: identity_text(reference.policy_id),
                    revision: reference.revision,
                })
                .collect(),
            decision_ceiling: decision_to_storage(snapshot.decision_ceiling),
            max_actions_per_run: snapshot.run_limits.max_actions,
            max_concurrent_actions_per_run: snapshot.run_limits.max_concurrent_actions,
            calendar_limits: snapshot
                .calendar_limits
                .iter()
                .map(|limit| ProgrammaticCalendarLimitRecordDto {
                    period_kind: period_to_storage(limit.period_kind),
                    max_actions: limit.max_actions,
                })
                .collect(),
            calendar_counter_references: snapshot
                .calendar_counter_references
                .iter()
                .map(|counter| ProgrammaticCalendarCounterReferenceDto {
                    policy_id: identity_text(counter.policy_id),
                    period_kind: period_to_storage(counter.period_kind),
                })
                .collect(),
            baseline_max_actions: snapshot
                .baseline
                .map(|baseline| baseline.maximum_action_count),
            baseline_max_concurrent_actions: snapshot
                .baseline
                .map(|baseline| baseline.maximum_concurrent_actions),
            snapshot_digest: digest_text(snapshot.snapshot_digest),
            created_at_ms: request.occurred_at_ms,
        };
        record.validate()?;
        self.policies.store_programmatic_policy_snapshot(record)?;
        Ok(())
    }

    /// Validates the live lifecycle state of every selected policy identity.
    fn validate_live_policies(
        &self,
        snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
        tool_call_started: bool,
    ) -> DtoResult<()> {
        for reference in &snapshot.policy_references {
            let policy = self
                .policies
                .load_programmatic_policy(identity_text(reference.policy_id))?;
            validate_live_policy_state(
                lifecycle_from_storage(policy.lifecycle_state),
                tool_call_started,
            )?;
        }
        Ok(())
    }

    /// Validates the root-only interaction and harness-delegation rules.
    fn validate_interaction_rules(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
    ) -> DtoResult<()> {
        let kind = ProgrammaticRootOriginKindV1::of(&request.root_origin);
        validate_interaction_admission(
            kind,
            &request.call.tool_id,
            request.call.is_root_run_call(),
        )?;
        let corridor_approved = self.has_active_corridor_selector(&request.call)?;
        validate_harness_delegation(kind, &request.call.tool_id, corridor_approved)
    }

    /// Whether an active corridor of the root tree covers the call's tool.
    fn has_active_corridor_selector(&self, call: &ProgrammaticAdmissionCallV1) -> DtoResult<bool> {
        let root_run_id = identity_text(call.context.root_run_id);
        let Some(corridor) = self
            .confirmations
            .load_active_programmatic_corridor(root_run_id)?
        else {
            return Ok(false);
        };
        if corridor.state != ProgrammaticCorridorStateDto::Active {
            return Ok(false);
        }
        let tool_label = call_tool_label(call);
        let mcp_label = call_mcp_label(call);
        Ok(corridor.exact_tool_or_method_selectors.iter().any(|label| {
            *label == tool_label || mcp_label.as_ref().is_some_and(|method| label == method)
        }))
    }

    /// Reads the accepted idempotent binding of an equal repeated operation.
    ///
    /// Returns `None` when the operation is fresh, the frozen evidence when
    /// every selected policy already bound this exact call, and
    /// `programmatic_policy_reservation_conflict` for a partial or rebinding
    /// state.
    fn replay_binding(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
        snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
    ) -> DtoResult<Option<ProgrammaticAdmissionEvidenceDto>> {
        if snapshot.policy_references.is_empty() {
            return Ok(None);
        }
        let mut accepted = Vec::new();
        for reference in &snapshot.policy_references {
            let reservation_reference =
                identity_text(self.reservation_identity(request, reference.policy_id));
            let Some(reservation) = self
                .admissions
                .load_programmatic_policy_reservation(reservation_reference.clone())?
            else {
                if accepted.is_empty() {
                    return Ok(None);
                }
                return Err(policy_error(
                    "programmatic_policy_reservation_conflict",
                    "a partial admission binding is never completed silently",
                ));
            };
            if !self.reservation_matches(&reservation, request, reference.policy_id) {
                return Err(policy_error(
                    "programmatic_policy_reservation_conflict",
                    "one tool call never re-binds to a different typed input",
                ));
            }
            match reservation.state {
                ProgrammaticReservationStateDto::Reserved
                | ProgrammaticReservationStateDto::PermanentOnStart => {}
                ProgrammaticReservationStateDto::ReleasedOnKnownPreEffect
                | ProgrammaticReservationStateDto::InterruptedBeforeStart
                | ProgrammaticReservationStateDto::ExternalEffectUnknown => {
                    return Err(policy_error(
                        "programmatic_policy_reservation_conflict",
                        "the call already reached a terminal reservation outcome",
                    ));
                }
            }
            accepted.push(reservation_reference);
        }
        let decision = effective_admission_decision(snapshot, &request.call);
        let provenance = self.provenance(request, snapshot, &accepted)?;
        Ok(Some(ProgrammaticAdmissionEvidenceDto {
            root_origin_kind: snapshot.root_origin_kind,
            decision,
            snapshot_id: Some(request.snapshot_id.clone()),
            snapshot_digest: snapshot.snapshot_digest,
            run_limits: snapshot.run_limits,
            provenance,
            reservations: accepted,
            corridor_digest: None,
            replayed: true,
            admitted_at_ms: request.occurred_at_ms,
        }))
    }

    /// Whether one reservation is exactly bound to this call.
    fn reservation_matches(
        &self,
        reservation: &ProgrammaticPolicyReservationRecordDto,
        request: &ProgrammaticAdmissionRequestDto,
        policy_id: [u8; 16],
    ) -> bool {
        reservation.tool_call_id == identity_text(request.call.context.tool_call_id)
            && reservation.typed_input_digest == digest_text(request.call.typed_input_digest)
            && reservation.policy_id == identity_text(policy_id)
            && reservation.root_run_id == identity_text(request.call.context.root_run_id)
    }

    /// Derives the deterministic reservation identity of one call and policy.
    fn reservation_identity(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
        policy_id: [u8; 16],
    ) -> [u8; 16] {
        let mut input = Vec::new();
        push_framed(&mut input, "programmatic-caller-reservation-v1");
        push_framed(&mut input, &identity_text(request.call.context.root_run_id));
        push_framed(
            &mut input,
            &identity_text(request.call.context.tool_call_id),
        );
        push_framed(&mut input, &identity_text(policy_id));
        derived_identity(Digest256::sha256(&input))
    }

    /// Derives the deterministic confirmation identity of one call.
    fn confirmation_identity(&self, call: &ProgrammaticAdmissionCallV1) -> [u8; 16] {
        let mut input = Vec::new();
        push_framed(&mut input, "programmatic-caller-confirmation-v1");
        push_framed(&mut input, &identity_text(call.context.root_session_id));
        push_framed(&mut input, &identity_text(call.context.root_run_id));
        push_framed(&mut input, &identity_text(call.context.tool_call_id));
        derived_identity(Digest256::sha256(&input))
    }

    /// Derives the deterministic calendar counter identity of one policy.
    fn calendar_counter_identity(
        &self,
        policy_id: [u8; 16],
        period: ProgrammaticCalendarPeriodKindV1,
    ) -> [u8; 16] {
        let mut input = Vec::new();
        push_framed(&mut input, "programmatic-caller-calendar-counter-v1");
        push_framed(&mut input, &identity_text(policy_id));
        push_framed(&mut input, period_to_storage(period).name());
        derived_identity(Digest256::sha256(&input))
    }

    /// Builds the immutable provenance of one action.
    fn provenance(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
        snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
        reservations: &[String],
    ) -> DtoResult<ProgrammaticCallerProvenanceV1> {
        new_provenance(
            request.root_origin,
            &request.call,
            request.links.clone(),
            derived_identity(snapshot.snapshot_digest),
            identity_bytes(&request.admission_basis_reference)?,
            reservations
                .iter()
                .map(|reference| identity_bytes(reference))
                .collect::<DtoResult<Vec<[u8; 16]>>>()?,
        )
    }

    /// Admits one call directly and reserves its policy counters.
    fn admit_direct(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
        snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
    ) -> DtoResult<ProgrammaticAdmissionEvidenceDto> {
        self.freeze_snapshot(request, snapshot)?;
        let (reservations, replayed) = self.reserve_counters(request, snapshot)?;
        let decision = effective_admission_decision(snapshot, &request.call);
        let provenance = self.provenance(request, snapshot, &reservations)?;
        Ok(ProgrammaticAdmissionEvidenceDto {
            root_origin_kind: snapshot.root_origin_kind,
            decision,
            snapshot_id: self.snapshot_reference(request, snapshot),
            snapshot_digest: snapshot.snapshot_digest,
            run_limits: snapshot.run_limits,
            provenance,
            reservations,
            corridor_digest: None,
            replayed,
            admitted_at_ms: request.occurred_at_ms,
        })
    }

    /// Admits one call through its one active user-approved corridor.
    fn admit_through_corridor(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
        snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
    ) -> DtoResult<ProgrammaticAdmissionEvidenceDto> {
        self.validate_interaction_rules(request)?;
        let root_run_id = identity_text(request.call.context.root_run_id);
        let corridor = self
            .confirmations
            .load_active_programmatic_corridor(root_run_id.clone())?
            .ok_or_else(|| {
                policy_error(
                    "programmatic_policy_corridor_unavailable",
                    "the call needs the user-approved corridor of its root tree",
                )
            })?;
        match corridor.state {
            ProgrammaticCorridorStateDto::Active => {}
            ProgrammaticCorridorStateDto::Exhausted => {
                return Err(policy_error(
                    "programmatic_policy_corridor_exhausted",
                    "the shared corridor allocation is exhausted",
                ));
            }
            ProgrammaticCorridorStateDto::Expired | ProgrammaticCorridorStateDto::Revoked => {
                return Err(policy_error(
                    "programmatic_policy_corridor_unavailable",
                    "the corridor expired or was revoked",
                ));
            }
        }
        validate_corridor_active(true)?;
        validate_bounded_corridor_admission(
            &request.call.tool_id,
            request.descriptor_constraint_family_declared,
        )?;
        self.validate_corridor_coverage(&corridor, request)?;
        self.freeze_snapshot(request, snapshot)?;
        let (reservations, replayed) = self.reserve_counters(request, snapshot)?;
        let consumed = self.confirmations.consume_programmatic_corridor_action(
            ConsumeProgrammaticCorridorActionInputDto {
                corridor_digest: corridor.corridor_digest.clone(),
                root_run_id,
                occurred_at_ms: request.occurred_at_ms,
            },
        );
        if let Err(error) = consumed {
            self.release_new_reservations(request, &reservations);
            return Err(error);
        }
        let decision = effective_admission_decision(snapshot, &request.call);
        let provenance = self.provenance(request, snapshot, &reservations)?;
        Ok(ProgrammaticAdmissionEvidenceDto {
            root_origin_kind: snapshot.root_origin_kind,
            decision,
            snapshot_id: self.snapshot_reference(request, snapshot),
            snapshot_digest: snapshot.snapshot_digest,
            run_limits: snapshot.run_limits,
            provenance,
            reservations,
            corridor_digest: Some(corridor.corridor_digest),
            replayed,
            admitted_at_ms: request.occurred_at_ms,
        })
    }

    /// Returns the frozen snapshot identity of one snapshot, if any.
    fn snapshot_reference(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
        snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
    ) -> Option<String> {
        if snapshot.policy_references.is_empty() {
            None
        } else {
            Some(request.snapshot_id.clone())
        }
    }

    /// Validates that one call stays inside one corridor's bounded selectors.
    fn validate_corridor_coverage(
        &self,
        corridor: &ProgrammaticAuthorizationCorridorRecordDto,
        request: &ProgrammaticAdmissionRequestDto,
    ) -> DtoResult<()> {
        for label in &corridor.required_effect_selectors {
            if !request
                .call
                .effect_flags
                .iter()
                .any(|flag| effect_flag_label(*flag) == label)
            {
                return Err(policy_error(
                    "programmatic_policy_corridor_unavailable",
                    "the call does not satisfy the corridor effect selector",
                ));
            }
        }
        if !corridor.exact_tool_or_method_selectors.is_empty() {
            let tool_label = call_tool_label(&request.call);
            let mcp_label = call_mcp_label(&request.call);
            let covered = corridor.exact_tool_or_method_selectors.iter().any(|label| {
                *label == tool_label || mcp_label.as_ref().is_some_and(|method| label == method)
            });
            if !covered {
                return Err(policy_error(
                    "programmatic_policy_corridor_unavailable",
                    "the call does not satisfy any corridor exact selector",
                ));
            }
        }
        for label in &corridor.descriptor_input_constraint_selections {
            if !request
                .call
                .typed_input_constraints
                .iter()
                .any(|constraint| &constraint_label(constraint) == label)
            {
                return Err(policy_error(
                    "programmatic_policy_input_constraint_mismatch",
                    "the call does not satisfy a selected typed input constraint",
                ));
            }
        }
        Ok(())
    }

    /// Verifies every applicable run and calendar limit and reserves one unit.
    ///
    /// The returned references are the accepted reservation identities, and
    /// the flag reports whether the storage replayed an equal accepted binding
    /// instead of reserving a second unit.
    fn reserve_counters(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
        snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
    ) -> DtoResult<(Vec<String>, bool)> {
        let mut references = Vec::new();
        let mut replayed = false;
        for (index, reference) in snapshot.policy_references.iter().enumerate() {
            let policy_id = identity_text(reference.policy_id);
            let counter = self
                .admissions
                .load_programmatic_policy_counters(policy_id.clone())?;
            let Some(counter) = counter else {
                return Err(policy_error(
                    "programmatic_policy_counter_unavailable",
                    "an unavailable counter blocks only the dependent action",
                ));
            };
            self.validate_counter_limits(&counter, snapshot, index)?;
            let Some(calendar_limit) = snapshot.calendar_limits.get(index) else {
                return Err(policy_error(
                    "programmatic_policy_counter_unavailable",
                    "an unavailable counter blocks only the dependent action",
                ));
            };
            let reservation_reference = self.reservation_identity(request, reference.policy_id);
            let calendar_reference =
                self.calendar_counter_identity(reference.policy_id, calendar_limit.period_kind);
            let outcome: ReserveProgrammaticPolicyActionOutcomeDto = self
                .admissions
                .reserve_programmatic_policy_action(ReserveProgrammaticPolicyActionInputDto {
                    reservation: ProgrammaticPolicyReservationRecordDto {
                        reservation_reference: identity_text(reservation_reference),
                        policy_id,
                        policy_revision: reference.revision,
                        root_run_id: identity_text(request.call.context.root_run_id),
                        tool_call_id: identity_text(request.call.context.tool_call_id),
                        typed_input_digest: digest_text(request.call.typed_input_digest),
                        calendar_counter_reference: identity_text(calendar_reference),
                        reserved_at_ms: request.occurred_at_ms,
                        state: ProgrammaticReservationStateDto::Reserved,
                        finished_at_ms: None,
                    },
                    max_actions_per_run: snapshot.run_limits.max_actions,
                    max_concurrent_actions_per_run: snapshot.run_limits.max_concurrent_actions,
                    calendar_max_actions: calendar_limit.max_actions,
                    calendar_window_start_ms: request.calendar_window.start_ms,
                    calendar_window_end_ms: request.calendar_window.end_ms,
                    calendar_window_time_zone: request.calendar_window.time_zone.clone(),
                })?;
            replayed |= outcome.replayed;
            references.push(outcome.reservation.reservation_reference);
        }
        Ok((references, replayed))
    }

    /// Verifies the run and calendar bounds of one policy counter.
    fn validate_counter_limits(
        &self,
        counter: &ProgrammaticPolicyCounterRecordDto,
        snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
        index: usize,
    ) -> DtoResult<()> {
        let run_used = counter
            .run_started_actions
            .saturating_add(counter.run_reserved_actions);
        if run_used >= snapshot.run_limits.max_actions {
            return Err(policy_error(
                "programmatic_policy_run_limit_exceeded",
                "the per-run action limit is exhausted",
            ));
        }
        let concurrency_used = counter
            .run_in_flight_actions
            .saturating_add(counter.run_reserved_actions)
            .saturating_add(1);
        if concurrency_used > snapshot.run_limits.max_concurrent_actions {
            return Err(policy_error(
                "programmatic_policy_run_limit_exceeded",
                "the per-run concurrency limit is exhausted",
            ));
        }
        let calendar_used = counter
            .calendar_started_actions
            .saturating_add(counter.calendar_reserved_actions);
        if let Some(limit) = snapshot.calendar_limits.get(index)
            && calendar_used >= limit.max_actions
        {
            return Err(policy_error(
                "programmatic_policy_calendar_limit_exceeded",
                "the calendar action limit is exhausted",
            ));
        }
        Ok(())
    }

    /// Releases the reservations this attempt newly created (compensation).
    ///
    /// The frozen storage contract consumes a corridor action in its own
    /// transaction, so a corridor denial after a successful reservation
    /// releases the unstarted units instead of leaving a half-committed
    /// admission.
    fn release_new_reservations(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
        reservations: &[String],
    ) {
        for reference in reservations {
            let _ = self.admissions.release_programmatic_policy_reservation(
                ReleaseProgrammaticPolicyReservationInputDto {
                    reservation_reference: reference.clone(),
                    tool_call_id: identity_text(request.call.context.tool_call_id),
                    released_at_ms: request.occurred_at_ms,
                },
            );
        }
    }

    /// Reads the exact confirmation state of one bound call.
    ///
    /// Returns `Some` when the confirmation still awaits its user decision and
    /// `None` when an accepted confirmation authorizes this exact call.
    fn confirmation_state(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
        snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
    ) -> DtoResult<Option<ProgrammaticPolicyConfirmationRecordDto>> {
        let tool_call_id = identity_text(request.call.context.tool_call_id);
        let confirmation = self
            .confirmations
            .load_programmatic_policy_confirmation(tool_call_id)?;
        let Some(confirmation) = confirmation else {
            return Ok(Some(self.create_awaiting_confirmation(request, snapshot)?));
        };
        match confirmation.state {
            ProgrammaticConfirmationStateDto::Awaiting => Ok(Some(confirmation)),
            ProgrammaticConfirmationStateDto::Accepted => {
                self.validate_confirmation_binding(&confirmation, request, snapshot)?;
                Ok(None)
            }
            ProgrammaticConfirmationStateDto::Rejected
            | ProgrammaticConfirmationStateDto::Expired
            | ProgrammaticConfirmationStateDto::Cancelled => Err(policy_error(
                "programmatic_policy_confirmation_expired",
                "the exact confirmation is no longer valid",
            )),
        }
    }

    /// Creates the one awaiting exact confirmation of a bound call.
    fn create_awaiting_confirmation(
        &self,
        request: &ProgrammaticAdmissionRequestDto,
        snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
    ) -> DtoResult<ProgrammaticPolicyConfirmationRecordDto> {
        let call = &request.call;
        let record = ProgrammaticPolicyConfirmationRecordDto {
            confirmation_id: identity_text(self.confirmation_identity(call)),
            root_session_id: identity_text(call.context.root_session_id),
            root_run_id: identity_text(call.context.root_run_id),
            tool_call_id: identity_text(call.context.tool_call_id),
            tool_id: call.tool_id.clone(),
            descriptor_revision: call.descriptor_revision.to_string(),
            mcp_method_reference: call
                .mcp_method
                .as_ref()
                .map(|method| method.method_reference.clone()),
            typed_input_digest: digest_text(call.typed_input_digest),
            policy_snapshot_digest: digest_text(snapshot.snapshot_digest),
            state: ProgrammaticConfirmationStateDto::Awaiting,
            created_at_ms: request.occurred_at_ms,
            decided_at_ms: None,
        };
        record.validate()?;
        self.confirmations
            .create_programmatic_policy_confirmation(record)
    }

    /// Validates one accepted exact confirmation against its one bound call.
    fn validate_confirmation_binding(
        &self,
        confirmation: &ProgrammaticPolicyConfirmationRecordDto,
        request: &ProgrammaticAdmissionRequestDto,
        snapshot: &EffectiveProgrammaticCallerPolicySnapshotV1,
    ) -> DtoResult<()> {
        let call = &request.call;
        let binding_matches = confirmation.tool_call_id == identity_text(call.context.tool_call_id)
            && confirmation.root_run_id == identity_text(call.context.root_run_id)
            && confirmation.root_session_id == identity_text(call.context.root_session_id)
            && confirmation.tool_id == call.tool_id
            && confirmation.descriptor_revision == call.descriptor_revision.to_string()
            && confirmation.typed_input_digest == digest_text(call.typed_input_digest)
            && digest_bytes(&confirmation.policy_snapshot_digest)? == snapshot.snapshot_digest
            && confirmation.mcp_method_reference
                == call
                    .mcp_method
                    .as_ref()
                    .map(|method| method.method_reference.clone());
        if !binding_matches {
            return Err(policy_error(
                "programmatic_policy_confirmation_required",
                "the exact confirmation binds only its one exact call",
            ));
        }
        let decided_at = confirmation.decided_at_ms.ok_or_else(|| {
            policy_error(
                "programmatic_policy_confirmation_expired",
                "an accepted confirmation carries its decision time",
            )
        })?;
        let expires_at = decided_at.saturating_add(EXACT_CONFIRMATION_VALIDITY_MS);
        if request.occurred_at_ms < confirmation.created_at_ms
            || request.occurred_at_ms > expires_at
        {
            return Err(policy_error(
                "programmatic_policy_confirmation_expired",
                "the exact confirmation is outside its validity window",
            ));
        }
        Ok(())
    }

    /// Requires the accepted confirmation that approves one corridor.
    fn require_accepted_confirmation(
        &self,
        tool_call_id: &str,
        confirmation_reference: &str,
    ) -> DtoResult<()> {
        let confirmation = self
            .confirmations
            .load_programmatic_policy_confirmation(tool_call_id.to_owned())?
            .ok_or_else(|| {
                policy_error(
                    "programmatic_policy_confirmation_required",
                    "a corridor needs its accepted approval confirmation",
                )
            })?;
        if confirmation.state != ProgrammaticConfirmationStateDto::Accepted
            || confirmation.confirmation_id != confirmation_reference
        {
            return Err(policy_error(
                "programmatic_policy_confirmation_required",
                "a corridor needs its accepted approval confirmation",
            ));
        }
        Ok(())
    }
}
