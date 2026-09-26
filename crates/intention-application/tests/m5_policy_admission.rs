#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Policy admission fixtures use expect and panic for precise diagnostics."
)]

//! Slice 3 policy admission tests.
//!
//! Owner: architecture 27 with ADR 0044. These tests pin effective-snapshot
//! resolution, the four admission decisions, confirmation and corridor
//! lifecycle, reservation atomicity, and recovery dispositions.
//!
//! Every fixture is hermetic and deterministic: the in-memory repositories
//! below mirror the durable reservation, confirmation, corridor, and counter
//! invariants; time is fixed Unix milliseconds; and no test sleeps, binds a
//! port, or touches the network.

use std::cell::RefCell;
use std::rc::Rc;

use intention_application::programmatic_policy::{
    EXACT_CONFIRMATION_VALIDITY_MS, ProgrammaticAdmissionEvidenceDto,
    ProgrammaticAdmissionOutcomeDto, ProgrammaticAdmissionRequestDto,
    ProgrammaticCalendarWindowDto, ProgrammaticCorridorApprovalInputDto,
    ProgrammaticExactConfirmationRequestDto, ProgrammaticPolicyAdmissionService,
    ProgrammaticReservationRecoveryInputDto,
};
use intention_domain::canonical::Digest256;
use intention_domain::programmatic_policy::{
    DescriptorInputConstraintSelectionV1, ProgrammaticAdmissionCallV1,
    ProgrammaticAdmissionRuleEntryV1, ProgrammaticAdmissionRuleV1, ProgrammaticCalendarLimitV1,
    ProgrammaticCalendarPeriodKindV1, ProgrammaticCallContextV1,
    ProgrammaticCallerApplicablePolicyV1, ProgrammaticCallerPolicyRevisionV1,
    ProgrammaticCallerPolicyScopeV1, ProgrammaticMcpMethodReferenceV1,
    ProgrammaticProvenanceLinksV1, ProgrammaticRecoveryDispositionV1, ProgrammaticRootOriginKindV1,
    ProgrammaticRootOriginRuleV1, ProgrammaticRuleSelectorV1, ProgrammaticRunLimitsV1,
    ProgrammaticToolEffectFlagV1,
};
use intention_domain::slice3_selections::ProgrammaticCallerRootOriginV1;
use intention_storage::programmatic_policy_repo::{
    AppendProgrammaticPolicyRevisionInputDto, CommitProgrammaticReservationStartedInputDto,
    ConsumeProgrammaticCorridorActionInputDto, CreateProgrammaticPolicyInputDto,
    DecideProgrammaticPolicyConfirmationInputDto, ProgrammaticAdmissionRepositoryDto,
    ProgrammaticAuthorizationCorridorRecordDto, ProgrammaticCalendarPeriodKindDto,
    ProgrammaticConfirmationRepositoryDto, ProgrammaticConfirmationStateDto,
    ProgrammaticCorridorStateDto, ProgrammaticPolicyConfirmationRecordDto,
    ProgrammaticPolicyCounterRecordDto, ProgrammaticPolicyDraftRecordDto,
    ProgrammaticPolicyLifecycleStateDto, ProgrammaticPolicyRecordDto,
    ProgrammaticPolicyRepositoryDto, ProgrammaticPolicyReservationRecordDto,
    ProgrammaticPolicyScopeDto, ProgrammaticPolicySnapshotRecordDto,
    ProgrammaticReservationStateDto, RecoverProgrammaticPolicyReservationInputDto,
    ReleaseProgrammaticPolicyReservationInputDto, ReserveProgrammaticPolicyActionInputDto,
    ReserveProgrammaticPolicyActionOutcomeDto, TransitionProgrammaticPolicyLifecycleInputDto,
};
use intention_types::{DtoResult, ErrorDto};

/// Formats one fixture identity as canonical UUID text.
fn identity_text(byte: u8) -> String {
    let hex: String = std::iter::repeat_n(format!("{byte:02x}"), 16).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// A fixed daemon-assigned identity of one repeated byte.
const fn identity(byte: u8) -> [u8; 16] {
    [byte; 16]
}

/// A fixed deterministic digest.
fn digest(seed: u8) -> Digest256 {
    Digest256::sha256(&[seed; 16])
}

/// The canonical digest text of one fixed digest.
fn digest_text(seed: u8) -> String {
    digest(seed).to_string()
}

/// The immutable project scope shared by the fixture policies.
const fn policy_scope() -> ProgrammaticCallerPolicyScopeV1 {
    ProgrammaticCallerPolicyScopeV1::Project {
        project_id: identity(2),
    }
}

/// One bounded typed input constraint of the fixture corridor.
fn corridor_constraint() -> DescriptorInputConstraintSelectionV1 {
    DescriptorInputConstraintSelectionV1::new("paths", 1, vec!["src".to_owned()])
        .expect("the fixture constraint is valid")
}

/// One applicable fixture policy with the selected decision and exact tool.
fn applicable(
    policy_byte: u8,
    decision: ProgrammaticAdmissionRuleV1,
    tool_id: &str,
) -> ProgrammaticCallerApplicablePolicyV1 {
    let policy_id = identity(policy_byte);
    let revision = ProgrammaticCallerPolicyRevisionV1::new(
        policy_id,
        1,
        vec![
            ProgrammaticRootOriginRuleV1::new(
                ProgrammaticRootOriginKindV1::InteractiveUser,
                decision,
            ),
            ProgrammaticRootOriginRuleV1::new(
                ProgrammaticRootOriginKindV1::ContinualHarness,
                decision,
            ),
        ],
        vec![
            ProgrammaticAdmissionRuleEntryV1::new(
                decision,
                vec![
                    ProgrammaticRuleSelectorV1::exact_tool(tool_id, 1)
                        .expect("the fixture selector is valid"),
                ],
                vec![],
            )
            .expect("the fixture rule is valid"),
        ],
        ProgrammaticRunLimitsV1::new(4, 2).expect("the fixture run limits are valid"),
        ProgrammaticCalendarLimitV1::new(ProgrammaticCalendarPeriodKindV1::Day, 8)
            .expect("the fixture calendar limit is valid"),
        vec![],
    )
    .expect("the fixture revision is valid");
    ProgrammaticCallerApplicablePolicyV1::new(policy_id, 1, policy_scope(), revision)
        .expect("the fixture selection is coherent")
}

/// One typed admission call of the fixture root run.
fn call(
    root_origin: ProgrammaticRootOriginKindV1,
    tool_id: &str,
    effect_flags: Vec<ProgrammaticToolEffectFlagV1>,
    typed_input_constraints: Vec<DescriptorInputConstraintSelectionV1>,
) -> ProgrammaticAdmissionCallV1 {
    ProgrammaticAdmissionCallV1::new(
        ProgrammaticCallContextV1 {
            root_origin,
            root_session_id: identity(5),
            root_run_id: identity(6),
            current_session_id: identity(5),
            current_run_id: identity(6),
            tool_call_id: identity(7),
        },
        tool_id,
        1,
        effect_flags,
        None,
        typed_input_constraints,
        digest(11),
    )
    .expect("the fixture call is valid")
}

/// One interactive root origin of the fixture session.
const fn interactive_origin() -> ProgrammaticCallerRootOriginV1 {
    ProgrammaticCallerRootOriginV1::InteractiveUser {
        originating_turn_id: identity(9),
    }
}

/// One harness root origin of the fixture rule revision.
const fn harness_origin() -> ProgrammaticCallerRootOriginV1 {
    ProgrammaticCallerRootOriginV1::ContinualHarness {
        harness_id: identity(3),
        rule_revision: 1,
        trigger_reason_id: identity(4),
    }
}

/// One admission request over the selected applicable policies.
fn request(
    root_origin: ProgrammaticCallerRootOriginV1,
    call: ProgrammaticAdmissionCallV1,
    applicable_policies: Vec<ProgrammaticCallerApplicablePolicyV1>,
) -> ProgrammaticAdmissionRequestDto {
    ProgrammaticAdmissionRequestDto {
        root_origin,
        call,
        links: ProgrammaticProvenanceLinksV1 {
            parent_link_references: vec![],
            bridge_operation_reference: None,
            leading_goal_reference: None,
        },
        snapshot_id: identity_text(8),
        admission_basis_reference: identity_text(10),
        applicable_policies,
        project_id: identity(2),
        session_id: identity(5),
        leading_goal_chain: vec![],
        inherited_from_source_session: false,
        descriptor_constraint_family_declared: true,
        calendar_window: ProgrammaticCalendarWindowDto {
            start_ms: 1_000,
            end_ms: 2_000,
            time_zone: "UTC".to_owned(),
        },
        occurred_at_ms: 1_500,
    }
}

/// One exact interactive admission request of the selected decision.
fn exact_request(decision: ProgrammaticAdmissionRuleV1) -> ProgrammaticAdmissionRequestDto {
    request(
        interactive_origin(),
        call(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            "execute",
            vec![ProgrammaticToolEffectFlagV1::LocalExecute],
            vec![],
        ),
        vec![applicable(1, decision, "execute")],
    )
}

/// One bounded harness request over the fixture corridor selection.
fn bounded_request() -> ProgrammaticAdmissionRequestDto {
    request(
        harness_origin(),
        call(
            ProgrammaticRootOriginKindV1::ContinualHarness,
            "sub_agent",
            vec![ProgrammaticToolEffectFlagV1::ChildDelegation],
            vec![corridor_constraint()],
        ),
        vec![applicable(
            1,
            ProgrammaticAdmissionRuleV1::BoundedConfirmationRequired,
            "sub_agent",
        )],
    )
}

/// The fixture durable counters of the fixture policy.
fn counters() -> ProgrammaticPolicyCounterRecordDto {
    ProgrammaticPolicyCounterRecordDto {
        policy_id: identity_text(1),
        run_started_actions: 0,
        run_reserved_actions: 0,
        run_in_flight_actions: 0,
        calendar_started_actions: 0,
        calendar_reserved_actions: 0,
        calendar_window_start_ms: 1_000,
        calendar_window_end_ms: 2_000,
        calendar_window_time_zone: "UTC".to_owned(),
        updated_at_ms: 1_000,
    }
}

/// One accepted exact confirmation of the fixture root tree.
fn accepted_confirmation(
    tool_call_byte: u8,
    confirmation_byte: u8,
) -> ProgrammaticPolicyConfirmationRecordDto {
    ProgrammaticPolicyConfirmationRecordDto {
        confirmation_id: identity_text(confirmation_byte),
        root_session_id: identity_text(5),
        root_run_id: identity_text(6),
        tool_call_id: identity_text(tool_call_byte),
        tool_id: "sub_agent".to_owned(),
        descriptor_revision: "1".to_owned(),
        mcp_method_reference: None,
        typed_input_digest: digest_text(11),
        policy_snapshot_digest: digest_text(12),
        state: ProgrammaticConfirmationStateDto::Accepted,
        created_at_ms: 1_000,
        decided_at_ms: Some(1_200),
    }
}

/// Extracts the admitted evidence of one outcome.
fn admitted(outcome: ProgrammaticAdmissionOutcomeDto) -> ProgrammaticAdmissionEvidenceDto {
    match outcome {
        ProgrammaticAdmissionOutcomeDto::Admitted(evidence) => *evidence,
        ProgrammaticAdmissionOutcomeDto::ConfirmationRequired(confirmation) => {
            panic!("expected an admitted call, got an awaiting confirmation: {confirmation:?}")
        }
    }
}

/// Extracts the awaiting confirmation of one outcome.
fn awaiting(outcome: ProgrammaticAdmissionOutcomeDto) -> ProgrammaticPolicyConfirmationRecordDto {
    match outcome {
        ProgrammaticAdmissionOutcomeDto::ConfirmationRequired(confirmation) => *confirmation,
        ProgrammaticAdmissionOutcomeDto::Admitted(evidence) => {
            panic!("expected an awaiting confirmation, got admitted evidence: {evidence:?}")
        }
    }
}

/// The in-memory policy repository of the admission runtime tests.
struct FakePolicies {
    log: Rc<RefCell<Vec<String>>>,
    lifecycle: RefCell<ProgrammaticPolicyLifecycleStateDto>,
    snapshots: RefCell<Vec<ProgrammaticPolicySnapshotRecordDto>>,
}

impl FakePolicies {
    const fn new(log: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            log,
            lifecycle: RefCell::new(ProgrammaticPolicyLifecycleStateDto::Active),
            snapshots: RefCell::new(Vec::new()),
        }
    }

    fn record(&self, entry: impl Into<String>) {
        self.log.borrow_mut().push(entry.into());
    }

    fn set_lifecycle(&self, state: ProgrammaticPolicyLifecycleStateDto) {
        *self.lifecycle.borrow_mut() = state;
    }
}

impl ProgrammaticPolicyRepositoryDto for FakePolicies {
    fn create_programmatic_policy(
        &self,
        _input: CreateProgrammaticPolicyInputDto,
    ) -> DtoResult<ProgrammaticPolicyRecordDto> {
        self.record("policy-create");
        Err(ErrorDto::unavailable(
            "programmatic_policy_storage_unsupported",
            "the admission runtime never creates a policy identity",
        ))
    }

    fn load_programmatic_policy(
        &self,
        policy_id: String,
    ) -> DtoResult<ProgrammaticPolicyRecordDto> {
        self.record("policy-read");
        Ok(ProgrammaticPolicyRecordDto {
            policy_id,
            scope: ProgrammaticPolicyScopeDto::Project {
                project_id: identity_text(2),
            },
            calendar_period_kind: ProgrammaticCalendarPeriodKindDto::Day,
            lifecycle_state: *self.lifecycle.borrow(),
            active_revision: 1,
            canonical_policy_digest: digest_text(3),
            updated_at_ms: 1_000,
        })
    }

    fn load_programmatic_policy_revision(
        &self,
        _policy_id: String,
        _revision: u64,
    ) -> DtoResult<intention_storage::programmatic_policy_repo::ProgrammaticPolicyRevisionRecordDto>
    {
        self.record("policy-revision-read");
        Err(ErrorDto::validation(
            "programmatic_policy_revision_conflict",
            "the exact policy revision is absent",
        ))
    }

    fn append_programmatic_policy_revision(
        &self,
        _input: AppendProgrammaticPolicyRevisionInputDto,
    ) -> DtoResult<ProgrammaticPolicyRecordDto> {
        self.record("policy-revision-append");
        Err(ErrorDto::unavailable(
            "programmatic_policy_storage_unsupported",
            "the admission runtime never appends a policy revision",
        ))
    }

    fn transition_programmatic_policy_lifecycle(
        &self,
        _input: TransitionProgrammaticPolicyLifecycleInputDto,
    ) -> DtoResult<ProgrammaticPolicyRecordDto> {
        self.record("policy-transition");
        Err(ErrorDto::unavailable(
            "programmatic_policy_storage_unsupported",
            "the admission runtime never transitions a policy lifecycle",
        ))
    }

    fn count_programmatic_policies_in_project(&self, _project_id: String) -> DtoResult<u64> {
        self.record("policy-count");
        Ok(1)
    }

    fn count_programmatic_policies_on_goal(&self, _goal_id: String) -> DtoResult<u64> {
        Ok(0)
    }

    fn store_programmatic_policy_snapshot(
        &self,
        input: ProgrammaticPolicySnapshotRecordDto,
    ) -> DtoResult<ProgrammaticPolicySnapshotRecordDto> {
        self.record("snapshot-store");
        input.validate()?;
        let mut snapshots = self.snapshots.borrow_mut();
        if let Some(stored) = snapshots
            .iter()
            .find(|stored| stored.snapshot_id == input.snapshot_id)
        {
            if *stored != input {
                return Err(ErrorDto::validation(
                    "programmatic_policy_snapshot_unavailable",
                    "the snapshot identity is already bound to different content",
                ));
            }
            return Ok(stored.clone());
        }
        snapshots.push(input.clone());
        Ok(input)
    }

    fn load_programmatic_policy_snapshot(
        &self,
        snapshot_id: String,
    ) -> DtoResult<ProgrammaticPolicySnapshotRecordDto> {
        self.record("snapshot-read");
        self.snapshots
            .borrow()
            .iter()
            .find(|snapshot| snapshot.snapshot_id == snapshot_id)
            .cloned()
            .ok_or_else(|| {
                ErrorDto::validation(
                    "programmatic_policy_snapshot_unavailable",
                    "the effective snapshot is absent",
                )
            })
    }
}

/// The in-memory confirmation and corridor repository of the tests.
struct FakeConfirmations {
    log: Rc<RefCell<Vec<String>>>,
    confirmations: RefCell<Vec<ProgrammaticPolicyConfirmationRecordDto>>,
    corridors: RefCell<Vec<ProgrammaticAuthorizationCorridorRecordDto>>,
}

impl FakeConfirmations {
    const fn new(log: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            log,
            confirmations: RefCell::new(Vec::new()),
            corridors: RefCell::new(Vec::new()),
        }
    }

    fn record(&self, entry: impl Into<String>) {
        self.log.borrow_mut().push(entry.into());
    }

    fn store_confirmation(&self, confirmation: ProgrammaticPolicyConfirmationRecordDto) {
        self.confirmations.borrow_mut().push(confirmation);
    }

    fn confirmations(&self) -> Vec<ProgrammaticPolicyConfirmationRecordDto> {
        self.confirmations.borrow().clone()
    }

    fn corridors(&self) -> Vec<ProgrammaticAuthorizationCorridorRecordDto> {
        self.corridors.borrow().clone()
    }

    fn set_corridor_state(&self, state: ProgrammaticCorridorStateDto) {
        for corridor in self.corridors.borrow_mut().iter_mut() {
            corridor.state = state;
        }
    }

    fn set_corridor_consumed(&self, consumed: u64) {
        for corridor in self.corridors.borrow_mut().iter_mut() {
            corridor.consumed_action_count = consumed;
        }
    }
}

impl ProgrammaticConfirmationRepositoryDto for FakeConfirmations {
    fn create_programmatic_policy_confirmation(
        &self,
        input: ProgrammaticPolicyConfirmationRecordDto,
    ) -> DtoResult<ProgrammaticPolicyConfirmationRecordDto> {
        self.record("confirmation-create");
        input.validate()?;
        let mut confirmations = self.confirmations.borrow_mut();
        if let Some(stored) = confirmations
            .iter()
            .find(|stored| stored.confirmation_id == input.confirmation_id)
        {
            if *stored != input {
                return Err(ErrorDto::validation(
                    "programmatic_policy_confirmation_required",
                    "the confirmation identity is already bound to different content",
                ));
            }
            return Ok(stored.clone());
        }
        confirmations.push(input.clone());
        Ok(input)
    }

    fn load_programmatic_policy_confirmation(
        &self,
        tool_call_id: String,
    ) -> DtoResult<Option<ProgrammaticPolicyConfirmationRecordDto>> {
        self.record("confirmation-read");
        Ok(self
            .confirmations
            .borrow()
            .iter()
            .find(|confirmation| confirmation.tool_call_id == tool_call_id)
            .cloned())
    }

    fn decide_programmatic_policy_confirmation(
        &self,
        input: DecideProgrammaticPolicyConfirmationInputDto,
    ) -> DtoResult<ProgrammaticPolicyConfirmationRecordDto> {
        self.record("confirmation-decide");
        let mut confirmations = self.confirmations.borrow_mut();
        let Some(stored) = confirmations.iter_mut().find(|stored| {
            stored.confirmation_id == input.confirmation_id
                && stored.tool_call_id == input.tool_call_id
                && stored.typed_input_digest == input.typed_input_digest
        }) else {
            return Err(ErrorDto::validation(
                "programmatic_policy_confirmation_required",
                "the confirmation binding does not match the decision",
            ));
        };
        if stored.state != ProgrammaticConfirmationStateDto::Awaiting {
            return Err(ErrorDto::validation(
                "programmatic_policy_confirmation_expired",
                "the confirmation is no longer awaiting a decision",
            ));
        }
        stored.state = input.state;
        stored.decided_at_ms = Some(input.decided_at_ms);
        Ok(stored.clone())
    }

    fn create_programmatic_authorization_corridor(
        &self,
        input: ProgrammaticAuthorizationCorridorRecordDto,
    ) -> DtoResult<ProgrammaticAuthorizationCorridorRecordDto> {
        self.record("corridor-create");
        input.validate()?;
        self.corridors.borrow_mut().push(input.clone());
        Ok(input)
    }

    fn load_active_programmatic_corridor(
        &self,
        root_run_id: String,
    ) -> DtoResult<Option<ProgrammaticAuthorizationCorridorRecordDto>> {
        self.record("corridor-read");
        Ok(self
            .corridors
            .borrow()
            .iter()
            .find(|corridor| {
                corridor.root_run_id == root_run_id
                    && corridor.state == ProgrammaticCorridorStateDto::Active
            })
            .cloned())
    }

    fn consume_programmatic_corridor_action(
        &self,
        input: ConsumeProgrammaticCorridorActionInputDto,
    ) -> DtoResult<ProgrammaticAuthorizationCorridorRecordDto> {
        self.record("corridor-consume");
        let mut corridors = self.corridors.borrow_mut();
        let Some(corridor) = corridors.iter_mut().find(|corridor| {
            corridor.corridor_digest == input.corridor_digest
                && corridor.root_run_id == input.root_run_id
        }) else {
            return Err(ErrorDto::validation(
                "programmatic_policy_corridor_unavailable",
                "no active corridor matches the consumption",
            ));
        };
        if corridor.state != ProgrammaticCorridorStateDto::Active {
            return Err(ErrorDto::validation(
                "programmatic_policy_corridor_unavailable",
                "the corridor is not active",
            ));
        }
        if corridor.consumed_action_count >= corridor.maximum_action_count {
            return Err(ErrorDto::validation(
                "programmatic_policy_corridor_exhausted",
                "the shared corridor allocation is exhausted",
            ));
        }
        corridor.consumed_action_count += 1;
        Ok(corridor.clone())
    }

    fn close_programmatic_corridors_for_root(
        &self,
        root_run_id: String,
        state: ProgrammaticCorridorStateDto,
        closed_at_ms: u64,
    ) -> DtoResult<u64> {
        self.record("corridor-close");
        let mut closed = 0;
        for corridor in self.corridors.borrow_mut().iter_mut() {
            if corridor.root_run_id == root_run_id {
                corridor.state = state;
                corridor.created_at_ms = closed_at_ms;
                closed += 1;
            }
        }
        Ok(closed)
    }

    fn propose_programmatic_policy_draft(
        &self,
        _input: ProgrammaticPolicyDraftRecordDto,
    ) -> DtoResult<ProgrammaticPolicyDraftRecordDto> {
        self.record("draft-propose");
        Err(ErrorDto::unavailable(
            "programmatic_policy_storage_unsupported",
            "the admission runtime never prepares a policy draft",
        ))
    }

    fn load_pending_programmatic_policy_draft(
        &self,
        _scope: ProgrammaticPolicyScopeDto,
        _record_kind: String,
    ) -> DtoResult<Option<ProgrammaticPolicyDraftRecordDto>> {
        Ok(None)
    }

    fn decide_programmatic_policy_draft(
        &self,
        _draft_id: String,
        _state: intention_storage::programmatic_policy_repo::ProgrammaticPolicyDraftStateDto,
        _decided_at_ms: u64,
    ) -> DtoResult<ProgrammaticPolicyDraftRecordDto> {
        Err(ErrorDto::unavailable(
            "programmatic_policy_storage_unsupported",
            "the admission runtime never decides a policy draft",
        ))
    }
}

/// The in-memory counter and reservation repository of the tests.
struct FakeAdmissions {
    log: Rc<RefCell<Vec<String>>>,
    counters: RefCell<Option<ProgrammaticPolicyCounterRecordDto>>,
    reservations: RefCell<Vec<ProgrammaticPolicyReservationRecordDto>>,
}

impl FakeAdmissions {
    fn new(log: Rc<RefCell<Vec<String>>>) -> Self {
        Self {
            log,
            counters: RefCell::new(Some(counters())),
            reservations: RefCell::new(Vec::new()),
        }
    }

    fn record(&self, entry: impl Into<String>) {
        self.log.borrow_mut().push(entry.into());
    }

    fn set_counters(&self, counters: ProgrammaticPolicyCounterRecordDto) {
        *self.counters.borrow_mut() = Some(counters);
    }

    fn clear_counters(&self) {
        *self.counters.borrow_mut() = None;
    }

    fn reservations(&self) -> Vec<ProgrammaticPolicyReservationRecordDto> {
        self.reservations.borrow().clone()
    }

    fn counters_state(&self) -> ProgrammaticPolicyCounterRecordDto {
        self.counters
            .borrow()
            .clone()
            .expect("the fixture counters are installed")
    }

    /// Removes one reservation, leaving a partial admission binding.
    fn remove_reservation(&self) {
        self.reservations.borrow_mut().pop();
    }
}

impl ProgrammaticAdmissionRepositoryDto for FakeAdmissions {
    fn load_programmatic_policy_counters(
        &self,
        _policy_id: String,
    ) -> DtoResult<Option<ProgrammaticPolicyCounterRecordDto>> {
        self.record("counters-read");
        Ok(self.counters.borrow().clone())
    }

    fn reserve_programmatic_policy_action(
        &self,
        input: ReserveProgrammaticPolicyActionInputDto,
    ) -> DtoResult<ReserveProgrammaticPolicyActionOutcomeDto> {
        self.record("reservation-reserve");
        let requested = input.reservation.clone();
        let mut reservations = self.reservations.borrow_mut();
        if let Some(existing) = reservations
            .iter()
            .find(|existing| existing.reservation_reference == requested.reservation_reference)
        {
            if existing.tool_call_id != requested.tool_call_id
                || existing.typed_input_digest != requested.typed_input_digest
            {
                return Err(ErrorDto::validation(
                    "programmatic_policy_reservation_conflict",
                    "the reservation is already bound to a different typed input",
                ));
            }
            return Ok(ReserveProgrammaticPolicyActionOutcomeDto {
                reservation: existing.clone(),
                counters: self.counters_state(),
                replayed: true,
            });
        }
        let mut counters = self.counters.borrow_mut();
        let Some(counter) = counters.as_mut() else {
            return Err(ErrorDto::validation(
                "programmatic_policy_counter_unavailable",
                "an unavailable counter blocks only the dependent action",
            ));
        };
        if counter.run_started_actions + counter.run_reserved_actions >= input.max_actions_per_run {
            return Err(ErrorDto::validation(
                "programmatic_policy_run_limit_exceeded",
                "the per-run action limit is exhausted",
            ));
        }
        if counter.run_in_flight_actions + counter.run_reserved_actions + 1
            > input.max_concurrent_actions_per_run
        {
            return Err(ErrorDto::validation(
                "programmatic_policy_run_limit_exceeded",
                "the per-run concurrency limit is exhausted",
            ));
        }
        if counter.calendar_started_actions + counter.calendar_reserved_actions
            >= input.calendar_max_actions
        {
            return Err(ErrorDto::validation(
                "programmatic_policy_calendar_limit_exceeded",
                "the calendar action limit is exhausted",
            ));
        }
        if input.calendar_window_end_ms <= input.calendar_window_start_ms {
            return Err(ErrorDto::validation(
                "programmatic_policy_counter_unavailable",
                "a calendar window has a positive bounded duration",
            ));
        }
        counter.run_reserved_actions += 1;
        counter.calendar_reserved_actions += 1;
        counter.calendar_window_start_ms = input.calendar_window_start_ms;
        counter.calendar_window_end_ms = input.calendar_window_end_ms;
        counter.calendar_window_time_zone = input.calendar_window_time_zone;
        reservations.push(requested.clone());
        Ok(ReserveProgrammaticPolicyActionOutcomeDto {
            reservation: requested,
            counters: counter.clone(),
            replayed: false,
        })
    }

    fn load_programmatic_policy_reservation(
        &self,
        reservation_reference: String,
    ) -> DtoResult<Option<ProgrammaticPolicyReservationRecordDto>> {
        self.record("reservation-read");
        Ok(self
            .reservations
            .borrow()
            .iter()
            .find(|reservation| reservation.reservation_reference == reservation_reference)
            .cloned())
    }

    fn load_programmatic_policy_reservations_for_run(
        &self,
        root_run_id: String,
    ) -> DtoResult<Vec<ProgrammaticPolicyReservationRecordDto>> {
        self.record("reservations-read");
        Ok(self
            .reservations
            .borrow()
            .iter()
            .filter(|reservation| reservation.root_run_id == root_run_id)
            .cloned()
            .collect())
    }

    fn release_programmatic_policy_reservation(
        &self,
        input: ReleaseProgrammaticPolicyReservationInputDto,
    ) -> DtoResult<ProgrammaticPolicyReservationRecordDto> {
        self.record("reservation-release");
        let mut reservations = self.reservations.borrow_mut();
        let Some(stored) = reservations.iter_mut().find(|stored| {
            stored.reservation_reference == input.reservation_reference
                && stored.tool_call_id == input.tool_call_id
        }) else {
            return Err(ErrorDto::validation(
                "programmatic_policy_reservation_conflict",
                "no outstanding reservation matches the release",
            ));
        };
        if stored.state != ProgrammaticReservationStateDto::Reserved {
            return Err(ErrorDto::validation(
                "programmatic_policy_reservation_conflict",
                "the reservation is not outstanding",
            ));
        }
        stored.state = ProgrammaticReservationStateDto::ReleasedOnKnownPreEffect;
        stored.finished_at_ms = Some(input.released_at_ms);
        let released = stored.clone();
        let mut counters = self.counters.borrow_mut();
        if let Some(counter) = counters.as_mut() {
            counter.run_reserved_actions = counter.run_reserved_actions.saturating_sub(1);
            counter.calendar_reserved_actions = counter.calendar_reserved_actions.saturating_sub(1);
        }
        Ok(released)
    }

    fn commit_programmatic_reservation_started(
        &self,
        input: CommitProgrammaticReservationStartedInputDto,
    ) -> DtoResult<ProgrammaticPolicyReservationRecordDto> {
        self.record("reservation-start");
        let mut reservations = self.reservations.borrow_mut();
        let Some(stored) = reservations.iter_mut().find(|stored| {
            stored.reservation_reference == input.reservation_reference
                && stored.tool_call_id == input.tool_call_id
        }) else {
            return Err(ErrorDto::validation(
                "programmatic_policy_reservation_conflict",
                "no outstanding reservation matches the start",
            ));
        };
        if stored.state != ProgrammaticReservationStateDto::Reserved {
            return Err(ErrorDto::validation(
                "programmatic_policy_reservation_conflict",
                "the reservation is not outstanding",
            ));
        }
        stored.state = ProgrammaticReservationStateDto::PermanentOnStart;
        stored.finished_at_ms = Some(input.started_at_ms);
        let started = stored.clone();
        let mut counters = self.counters.borrow_mut();
        if let Some(counter) = counters.as_mut() {
            counter.run_reserved_actions = counter.run_reserved_actions.saturating_sub(1);
            counter.run_started_actions += 1;
            counter.run_in_flight_actions += 1;
            counter.calendar_reserved_actions = counter.calendar_reserved_actions.saturating_sub(1);
            counter.calendar_started_actions += 1;
        }
        Ok(started)
    }

    fn recover_programmatic_policy_reservation(
        &self,
        input: RecoverProgrammaticPolicyReservationInputDto,
    ) -> DtoResult<ProgrammaticPolicyReservationRecordDto> {
        self.record("reservation-recover");
        let mut reservations = self.reservations.borrow_mut();
        let Some(stored) = reservations
            .iter_mut()
            .find(|stored| stored.reservation_reference == input.reservation_reference)
        else {
            return Err(ErrorDto::validation(
                "programmatic_policy_reservation_conflict",
                "no outstanding reservation matches the recovery",
            ));
        };
        let expected_state = if input.tool_call_started {
            ProgrammaticReservationStateDto::PermanentOnStart
        } else {
            ProgrammaticReservationStateDto::Reserved
        };
        if stored.state != expected_state {
            return Err(ErrorDto::validation(
                "programmatic_policy_reservation_conflict",
                "the reservation state does not match the recovery disposition",
            ));
        }
        if input.tool_call_started {
            stored.state = ProgrammaticReservationStateDto::ExternalEffectUnknown;
        } else {
            stored.state = ProgrammaticReservationStateDto::InterruptedBeforeStart;
            let mut counters = self.counters.borrow_mut();
            if let Some(counter) = counters.as_mut() {
                counter.run_reserved_actions = counter.run_reserved_actions.saturating_sub(1);
                counter.calendar_reserved_actions =
                    counter.calendar_reserved_actions.saturating_sub(1);
                counter.updated_at_ms = input.recovered_at_ms;
            }
        }
        stored.finished_at_ms = Some(input.recovered_at_ms);
        Ok(stored.clone())
    }
}

/// Creates the policy admission service over the three fake repositories.
const fn service<'a>(
    policies: &'a FakePolicies,
    confirmations: &'a FakeConfirmations,
    admissions: &'a FakeAdmissions,
) -> ProgrammaticPolicyAdmissionService<'a, FakePolicies, FakeConfirmations, FakeAdmissions> {
    ProgrammaticPolicyAdmissionService::new(policies, confirmations, admissions)
}

/// The shared fixture bundle of one admission test.
struct Fixture {
    log: Rc<RefCell<Vec<String>>>,
    policies: FakePolicies,
    confirmations: FakeConfirmations,
    admissions: FakeAdmissions,
}

impl Fixture {
    fn new() -> Self {
        let log = Rc::new(RefCell::new(Vec::new()));
        Self {
            policies: FakePolicies::new(Rc::clone(&log)),
            confirmations: FakeConfirmations::new(Rc::clone(&log)),
            admissions: FakeAdmissions::new(Rc::clone(&log)),
            log,
        }
    }

    const fn service(
        &self,
    ) -> ProgrammaticPolicyAdmissionService<'_, FakePolicies, FakeConfirmations, FakeAdmissions>
    {
        service(&self.policies, &self.confirmations, &self.admissions)
    }

    fn calls(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    fn count(&self, call: &str) -> usize {
        self.calls().iter().filter(|entry| *entry == call).count()
    }
}

#[test]
fn resolve_snapshot_rejects_an_incoherent_applicable_selection() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let mut selection = applicable(
        1,
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
        "execute",
    );
    selection.revision = 2;
    let error = service
        .resolve_snapshot(&request(
            interactive_origin(),
            call(
                ProgrammaticRootOriginKindV1::InteractiveUser,
                "execute",
                vec![],
                vec![],
            ),
            vec![selection],
        ))
        .expect_err("an applicable policy names its exact revision record");
    assert_eq!(error.code(), "programmatic_policy_revision_conflict");
}

#[test]
fn resolve_snapshot_rejects_an_inapplicable_scope() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let mut selection = applicable(
        1,
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
        "execute",
    );
    selection.scope = ProgrammaticCallerPolicyScopeV1::Project {
        project_id: identity(42),
    };
    let error = service
        .resolve_snapshot(&request(
            interactive_origin(),
            call(
                ProgrammaticRootOriginKindV1::InteractiveUser,
                "execute",
                vec![],
                vec![],
            ),
            vec![selection],
        ))
        .expect_err("a policy scope never crosses its project");
    assert_eq!(error.code(), "programmatic_policy_not_applicable");
}

#[test]
fn resolve_snapshot_rejects_an_over_bound_selection() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let selections = (20_u8..85)
        .map(|byte| applicable(byte, ProgrammaticAdmissionRuleV1::Prohibited, "execute"))
        .collect();
    let error = service
        .resolve_snapshot(&request(
            interactive_origin(),
            call(
                ProgrammaticRootOriginKindV1::InteractiveUser,
                "execute",
                vec![],
                vec![],
            ),
            selections,
        ))
        .expect_err("an effective snapshot stays inside its selected-record bound");
    assert_eq!(error.code(), "programmatic_policy_snapshot_too_large");
}

#[test]
fn a_harness_root_without_a_policy_fails_closed() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let error = service
        .resolve_snapshot(&request(
            harness_origin(),
            call(
                ProgrammaticRootOriginKindV1::ContinualHarness,
                "sub_agent",
                vec![],
                vec![],
            ),
            vec![],
        ))
        .expect_err("a harness root needs one separately selected policy");
    assert_eq!(error.code(), "programmatic_policy_snapshot_unavailable");
}

#[test]
fn an_interactive_root_read_stays_on_the_code_owned_baseline() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let read = service
        .admit_before_start(&request(
            interactive_origin(),
            call(
                ProgrammaticRootOriginKindV1::InteractiveUser,
                "read",
                vec![ProgrammaticToolEffectFlagV1::LocalRead],
                vec![],
            ),
            vec![],
        ))
        .expect("the code-owned baseline admits a root-run local read");
    let evidence = admitted(read);
    assert_eq!(
        evidence.decision,
        ProgrammaticAdmissionRuleV1::DirectLocalRead
    );
    assert!(evidence.snapshot_id.is_none());
    assert!(evidence.reservations.is_empty());
    assert!(!evidence.replayed);

    let execute = service
        .admit_before_start(&request(
            interactive_origin(),
            call(
                ProgrammaticRootOriginKindV1::InteractiveUser,
                "execute",
                vec![ProgrammaticToolEffectFlagV1::LocalExecute],
                vec![],
            ),
            vec![],
        ))
        .expect_err("the baseline covers only the closed direct-local-read set");
    assert_eq!(execute.code(), "programmatic_policy_not_applicable");
}

#[test]
fn a_prohibited_decision_rejects_the_call() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let error = service
        .admit_before_start(&exact_request(ProgrammaticAdmissionRuleV1::Prohibited))
        .expect_err("a prohibited decision admits nothing");
    assert_eq!(error.code(), "programmatic_policy_not_applicable");
    assert_eq!(fixture.count("reservation-reserve"), 0);
}

#[test]
fn a_suspended_or_revoked_policy_denies_a_not_yet_started_call() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);

    fixture
        .policies
        .set_lifecycle(ProgrammaticPolicyLifecycleStateDto::Suspended);
    let error = service
        .admit_before_start(&request)
        .expect_err("a suspended policy denies every not-yet-started matching call");
    assert_eq!(error.code(), "programmatic_policy_suspended");

    fixture
        .policies
        .set_lifecycle(ProgrammaticPolicyLifecycleStateDto::Revoked);
    let error = service
        .admit_before_start(&request)
        .expect_err("a revoked policy denies every new admission");
    assert_eq!(error.code(), "programmatic_policy_revoked");

    fixture
        .policies
        .set_lifecycle(ProgrammaticPolicyLifecycleStateDto::Archived);
    let error = service
        .admit_before_start(&request)
        .expect_err("an archived policy grants no authority");
    assert_eq!(error.code(), "programmatic_policy_not_applicable");
    assert_eq!(fixture.count("confirmation-create"), 0);
}

#[test]
fn an_exact_confirmation_is_coalesced_then_admits_after_acceptance() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);

    let first = service
        .admit_before_start(&request)
        .expect("the first attempt awaits its exact confirmation");
    let confirmation = awaiting(first);
    assert_eq!(
        confirmation.state,
        ProgrammaticConfirmationStateDto::Awaiting
    );
    assert_eq!(confirmation.tool_id, "execute");
    assert_eq!(confirmation.typed_input_digest, digest_text(11));
    assert_eq!(confirmation.decided_at_ms, None);
    assert_eq!(fixture.count("confirmation-create"), 1);
    assert_eq!(fixture.count("reservation-reserve"), 0);

    let second = service
        .admit_before_start(&request)
        .expect("an equal repeated attempt reads the same confirmation");
    assert_eq!(
        awaiting(second).confirmation_id,
        confirmation.confirmation_id
    );
    assert_eq!(fixture.count("confirmation-create"), 1);

    let coalesced = service
        .request_exact_confirmation(&ProgrammaticExactConfirmationRequestDto {
            request: request.clone(),
        })
        .expect("an equal repeated request reads the existing confirmation");
    assert_eq!(coalesced.confirmation_id, confirmation.confirmation_id);

    let decision = DecideProgrammaticPolicyConfirmationInputDto {
        confirmation_id: confirmation.confirmation_id.clone(),
        tool_call_id: confirmation.tool_call_id.clone(),
        typed_input_digest: confirmation.typed_input_digest,
        state: ProgrammaticConfirmationStateDto::Accepted,
        decided_at_ms: 1_500,
    };
    let accepted = service
        .decide_exact_confirmation(&decision, ProgrammaticConfirmationStateDto::Accepted)
        .expect("the exact confirmation is accepted");
    assert_eq!(accepted.state, ProgrammaticConfirmationStateDto::Accepted);

    let admitted = admitted(
        service
            .admit_before_start(&request)
            .expect("an accepted exact confirmation admits its one bound call"),
    );
    assert_eq!(
        admitted.decision,
        ProgrammaticAdmissionRuleV1::ExactConfirmationRequired
    );
    assert_eq!(
        admitted.snapshot_id.as_deref(),
        Some(identity_text(8).as_str())
    );
    assert_eq!(admitted.reservations.len(), 1);
    assert!(!admitted.replayed);
    assert_eq!(fixture.policies.snapshots.borrow().len(), 1);
}

#[test]
fn a_rejected_confirmation_never_admits() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    let confirmation = awaiting(
        service
            .admit_before_start(&request)
            .expect("the first attempt awaits its exact confirmation"),
    );
    let decision = DecideProgrammaticPolicyConfirmationInputDto {
        confirmation_id: confirmation.confirmation_id,
        tool_call_id: confirmation.tool_call_id,
        typed_input_digest: confirmation.typed_input_digest,
        state: ProgrammaticConfirmationStateDto::Rejected,
        decided_at_ms: 1_500,
    };
    service
        .decide_exact_confirmation(&decision, ProgrammaticConfirmationStateDto::Rejected)
        .expect("the exact confirmation is rejected");
    let error = service
        .admit_before_start(&request)
        .expect_err("a rejected confirmation never admits its call");
    assert_eq!(error.code(), "programmatic_policy_confirmation_expired");
}

#[test]
fn an_expired_confirmation_window_fails_closed() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    let confirmation = awaiting(
        service
            .admit_before_start(&request)
            .expect("the first attempt awaits its exact confirmation"),
    );
    let decision = DecideProgrammaticPolicyConfirmationInputDto {
        confirmation_id: confirmation.confirmation_id,
        tool_call_id: confirmation.tool_call_id,
        typed_input_digest: confirmation.typed_input_digest,
        state: ProgrammaticConfirmationStateDto::Accepted,
        decided_at_ms: 1_500,
    };
    service
        .decide_exact_confirmation(&decision, ProgrammaticConfirmationStateDto::Accepted)
        .expect("the exact confirmation is accepted");

    let mut expired = request.clone();
    expired.occurred_at_ms = 1_500 + EXACT_CONFIRMATION_VALIDITY_MS + 1;
    let error = service
        .admit_before_start(&expired)
        .expect_err("a decision outside its validity window never admits");
    assert_eq!(error.code(), "programmatic_policy_confirmation_expired");

    let mut boundary = request;
    boundary.occurred_at_ms = 1_500 + EXACT_CONFIRMATION_VALIDITY_MS;
    assert!(
        service.admit_before_start(&boundary).is_ok(),
        "the last millisecond of the window still admits"
    );
}

#[test]
fn a_confirmation_binding_mismatch_fails_closed() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    let confirmation = awaiting(
        service
            .admit_before_start(&request)
            .expect("the first attempt awaits its exact confirmation"),
    );
    service
        .decide_exact_confirmation(
            &DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: confirmation.confirmation_id,
                tool_call_id: confirmation.tool_call_id,
                typed_input_digest: confirmation.typed_input_digest,
                state: ProgrammaticConfirmationStateDto::Accepted,
                decided_at_ms: 1_500,
            },
            ProgrammaticConfirmationStateDto::Accepted,
        )
        .expect("the exact confirmation is accepted");

    let mut rebound = request;
    rebound.call.typed_input_digest = digest(44);
    let error = service
        .admit_before_start(&rebound)
        .expect_err("an accepted confirmation binds only its one exact call");
    assert_eq!(error.code(), "programmatic_policy_confirmation_required");
}

#[test]
fn an_exact_confirmation_decision_accepts_only_accept_or_reject() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let error = service
        .decide_exact_confirmation(
            &DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: identity_text(30),
                tool_call_id: identity_text(7),
                typed_input_digest: digest_text(11),
                state: ProgrammaticConfirmationStateDto::Awaiting,
                decided_at_ms: 1_500,
            },
            ProgrammaticConfirmationStateDto::Awaiting,
        )
        .expect_err("a decision is an accept or a reject");
    assert_eq!(error.code(), "programmatic_policy_confirmation_required");
}

#[test]
fn a_replayed_admission_reserves_no_second_unit() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    let confirmation = awaiting(
        service
            .admit_before_start(&request)
            .expect("the first attempt awaits its exact confirmation"),
    );
    service
        .decide_exact_confirmation(
            &DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: confirmation.confirmation_id,
                tool_call_id: confirmation.tool_call_id,
                typed_input_digest: confirmation.typed_input_digest,
                state: ProgrammaticConfirmationStateDto::Accepted,
                decided_at_ms: 1_500,
            },
            ProgrammaticConfirmationStateDto::Accepted,
        )
        .expect("the exact confirmation is accepted");
    let first = admitted(
        service
            .admit_before_start(&request)
            .expect("the accepted confirmation admits the call"),
    );
    assert!(!first.replayed);
    assert_eq!(fixture.count("reservation-reserve"), 1);

    let replay = admitted(
        service
            .admit_before_start(&request)
            .expect("an equal repeated attempt replays the accepted binding"),
    );
    assert!(replay.replayed);
    assert_eq!(replay.reservations, first.reservations);
    assert_eq!(fixture.admissions.reservations().len(), 1);
    assert_eq!(
        fixture.admissions.counters_state().run_reserved_actions,
        1,
        "a replay never reserves a second unit"
    );
}

#[test]
fn a_repurposed_request_fails_closed() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    let confirmation = awaiting(
        service
            .admit_before_start(&request)
            .expect("the first attempt awaits its exact confirmation"),
    );
    service
        .decide_exact_confirmation(
            &DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: confirmation.confirmation_id,
                tool_call_id: confirmation.tool_call_id,
                typed_input_digest: confirmation.typed_input_digest,
                state: ProgrammaticConfirmationStateDto::Accepted,
                decided_at_ms: 1_500,
            },
            ProgrammaticConfirmationStateDto::Accepted,
        )
        .expect("the exact confirmation is accepted");
    service
        .admit_before_start(&request)
        .expect("the accepted confirmation admits the call");

    let mut retyped = request;
    retyped.call.typed_input_digest = digest(45);
    let error = service
        .admit_before_start(&retyped)
        .expect_err("one tool call never re-binds to a different typed input");
    assert_eq!(error.code(), "programmatic_policy_reservation_conflict");
}

#[test]
fn a_partial_admission_binding_fails_closed() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = request(
        interactive_origin(),
        call(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            "execute",
            vec![ProgrammaticToolEffectFlagV1::LocalExecute],
            vec![],
        ),
        vec![
            applicable(
                1,
                ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
                "execute",
            ),
            applicable(
                31,
                ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
                "execute",
            ),
        ],
    );
    service
        .admit_before_start(&request)
        .expect("the first attempt awaits its exact confirmation");
    let confirmation = fixture.confirmations.confirmations();
    assert_eq!(confirmation.len(), 1);
    service
        .decide_exact_confirmation(
            &DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: confirmation[0].confirmation_id.clone(),
                tool_call_id: confirmation[0].tool_call_id.clone(),
                typed_input_digest: confirmation[0].typed_input_digest.clone(),
                state: ProgrammaticConfirmationStateDto::Accepted,
                decided_at_ms: 1_500,
            },
            ProgrammaticConfirmationStateDto::Accepted,
        )
        .expect("the exact confirmation is accepted");
    service
        .admit_before_start(&request)
        .expect("both selected policies reserve their one unit");
    assert_eq!(fixture.admissions.reservations().len(), 2);

    fixture.admissions.remove_reservation();
    let error = service
        .admit_before_start(&request)
        .expect_err("a partial admission binding is never completed silently");
    assert_eq!(error.code(), "programmatic_policy_reservation_conflict");
}

#[test]
fn a_terminal_reservation_outcome_fails_closed() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    let confirmation = awaiting(
        service
            .admit_before_start(&request)
            .expect("the first attempt awaits its exact confirmation"),
    );
    service
        .decide_exact_confirmation(
            &DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: confirmation.confirmation_id,
                tool_call_id: confirmation.tool_call_id,
                typed_input_digest: confirmation.typed_input_digest,
                state: ProgrammaticConfirmationStateDto::Accepted,
                decided_at_ms: 1_500,
            },
            ProgrammaticConfirmationStateDto::Accepted,
        )
        .expect("the exact confirmation is accepted");
    let admitted = admitted(
        service
            .admit_before_start(&request)
            .expect("the accepted confirmation admits the call"),
    );
    service
        .release_before_start(&ReleaseProgrammaticPolicyReservationInputDto {
            reservation_reference: admitted.reservations[0].clone(),
            tool_call_id: identity_text(7),
            released_at_ms: 1_600,
        })
        .expect("the known pre-effect outcome releases the reservation");

    let error = service
        .admit_before_start(&request)
        .expect_err("a terminal reservation outcome never re-reserves");
    assert_eq!(error.code(), "programmatic_policy_reservation_conflict");
}

#[test]
fn the_run_and_calendar_limits_deny_the_admission() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    let confirmation = awaiting(
        service
            .admit_before_start(&request)
            .expect("the first attempt awaits its exact confirmation"),
    );
    service
        .decide_exact_confirmation(
            &DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: confirmation.confirmation_id,
                tool_call_id: confirmation.tool_call_id,
                typed_input_digest: confirmation.typed_input_digest,
                state: ProgrammaticConfirmationStateDto::Accepted,
                decided_at_ms: 1_500,
            },
            ProgrammaticConfirmationStateDto::Accepted,
        )
        .expect("the exact confirmation is accepted");

    fixture
        .admissions
        .set_counters(ProgrammaticPolicyCounterRecordDto {
            run_started_actions: 4,
            ..counters()
        });
    let error = service
        .admit_before_start(&request)
        .expect_err("the per-run action limit is exhausted");
    assert_eq!(error.code(), "programmatic_policy_run_limit_exceeded");

    fixture
        .admissions
        .set_counters(ProgrammaticPolicyCounterRecordDto {
            calendar_started_actions: 8,
            ..counters()
        });
    let error = service
        .admit_before_start(&request)
        .expect_err("the calendar action limit is exhausted");
    assert_eq!(error.code(), "programmatic_policy_calendar_limit_exceeded");

    fixture.admissions.clear_counters();
    let error = service
        .admit_before_start(&request)
        .expect_err("an unavailable counter blocks only the dependent action");
    assert_eq!(error.code(), "programmatic_policy_counter_unavailable");
    assert_eq!(
        fixture.admissions.reservations().len(),
        0,
        "a denied limit reserves no unit"
    );
}

#[test]
fn a_corridor_needs_an_accepted_confirmation() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let error = service
        .approve_corridor(&ProgrammaticCorridorApprovalInputDto {
            root_session_id: identity(5),
            root_run_id: identity(6),
            root_origin_kind: ProgrammaticRootOriginKindV1::ContinualHarness,
            effective_policy_snapshot_digest: digest(13),
            effective_policy_snapshot_reference: identity_text(8),
            required_effect_selectors: vec![ProgrammaticToolEffectFlagV1::ChildDelegation],
            exact_tool_or_mcp_method_selectors: vec![
                ProgrammaticRuleSelectorV1::exact_tool("sub_agent", 1)
                    .expect("the fixture selector is valid"),
            ],
            descriptor_input_constraint_selections: vec![corridor_constraint()],
            maximum_action_count: 8,
            maximum_concurrent_actions: 2,
            confirmation_reference: identity_text(30),
            confirmation_tool_call_id: identity_text(31),
            occurred_at_ms: 1_300,
        })
        .expect_err("a corridor needs its accepted approval confirmation");
    assert_eq!(error.code(), "programmatic_policy_confirmation_required");
    assert_eq!(fixture.count("corridor-create"), 0);
}

#[test]
fn a_bounded_confirmation_admits_through_its_corridor() {
    let fixture = Fixture::new();
    fixture
        .confirmations
        .store_confirmation(accepted_confirmation(31, 30));
    let service = fixture.service();
    let corridor = service
        .approve_corridor(&ProgrammaticCorridorApprovalInputDto {
            root_session_id: identity(5),
            root_run_id: identity(6),
            root_origin_kind: ProgrammaticRootOriginKindV1::ContinualHarness,
            effective_policy_snapshot_digest: digest(13),
            effective_policy_snapshot_reference: identity_text(8),
            required_effect_selectors: vec![ProgrammaticToolEffectFlagV1::ChildDelegation],
            exact_tool_or_mcp_method_selectors: vec![
                ProgrammaticRuleSelectorV1::exact_tool("sub_agent", 1)
                    .expect("the fixture selector is valid"),
            ],
            descriptor_input_constraint_selections: vec![corridor_constraint()],
            maximum_action_count: 8,
            maximum_concurrent_actions: 2,
            confirmation_reference: identity_text(30),
            confirmation_tool_call_id: identity_text(31),
            occurred_at_ms: 1_300,
        })
        .expect("the user-approved corridor is created");
    assert_eq!(corridor.state, ProgrammaticCorridorStateDto::Active);
    assert_eq!(corridor.consumed_action_count, 0);
    assert_eq!(corridor.corridor_digest.len(), 71);

    let admitted = admitted(
        service
            .admit_before_start(&bounded_request())
            .expect("the bounded corridor admits the harness delegation"),
    );
    assert_eq!(
        admitted.decision,
        ProgrammaticAdmissionRuleV1::BoundedConfirmationRequired
    );
    assert_eq!(
        admitted.corridor_digest.as_deref(),
        Some(corridor.corridor_digest.as_str())
    );
    assert_eq!(admitted.reservations.len(), 1);
    assert_eq!(
        fixture.confirmations.corridors()[0].consumed_action_count,
        1
    );
}

#[test]
fn a_harness_delegation_without_a_corridor_fails_closed() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let error = service
        .admit_before_start(&bounded_request())
        .expect_err("harness child admission must fit the user-approved corridor");
    assert_eq!(
        error.code(),
        "programmatic_policy_harness_delegation_forbidden"
    );
}

#[test]
fn an_exhausted_or_revoked_corridor_fails_closed() {
    let fixture = Fixture::new();
    fixture
        .confirmations
        .store_confirmation(accepted_confirmation(31, 30));
    let service = fixture.service();
    service
        .approve_corridor(&ProgrammaticCorridorApprovalInputDto {
            root_session_id: identity(5),
            root_run_id: identity(6),
            root_origin_kind: ProgrammaticRootOriginKindV1::ContinualHarness,
            effective_policy_snapshot_digest: digest(13),
            effective_policy_snapshot_reference: identity_text(8),
            required_effect_selectors: vec![ProgrammaticToolEffectFlagV1::ChildDelegation],
            exact_tool_or_mcp_method_selectors: vec![
                ProgrammaticRuleSelectorV1::exact_tool("sub_agent", 1)
                    .expect("the fixture selector is valid"),
            ],
            descriptor_input_constraint_selections: vec![corridor_constraint()],
            maximum_action_count: 8,
            maximum_concurrent_actions: 2,
            confirmation_reference: identity_text(30),
            confirmation_tool_call_id: identity_text(31),
            occurred_at_ms: 1_300,
        })
        .expect("the user-approved corridor is created");

    fixture.confirmations.set_corridor_consumed(8);
    let error = service
        .admit_before_start(&bounded_request())
        .expect_err("an exhausted corridor admits nothing more");
    assert_eq!(error.code(), "programmatic_policy_corridor_exhausted");

    fixture
        .confirmations
        .set_corridor_state(ProgrammaticCorridorStateDto::Revoked);
    let error = service
        .admit_before_start(&bounded_request())
        .expect_err("a revoked corridor never reopens");
    assert_eq!(
        error.code(),
        "programmatic_policy_harness_delegation_forbidden",
        "a revoked corridor no longer covers the harness delegation"
    );
}

#[test]
fn a_corridor_coverage_mismatch_fails_closed() {
    let fixture = Fixture::new();
    fixture
        .confirmations
        .store_confirmation(accepted_confirmation(31, 30));
    let service = fixture.service();
    service
        .approve_corridor(&ProgrammaticCorridorApprovalInputDto {
            root_session_id: identity(5),
            root_run_id: identity(6),
            root_origin_kind: ProgrammaticRootOriginKindV1::ContinualHarness,
            effective_policy_snapshot_digest: digest(13),
            effective_policy_snapshot_reference: identity_text(8),
            required_effect_selectors: vec![ProgrammaticToolEffectFlagV1::LocalWrite],
            exact_tool_or_mcp_method_selectors: vec![
                ProgrammaticRuleSelectorV1::exact_tool("sub_agent", 1)
                    .expect("the fixture selector is valid"),
            ],
            descriptor_input_constraint_selections: vec![corridor_constraint()],
            maximum_action_count: 8,
            maximum_concurrent_actions: 2,
            confirmation_reference: identity_text(30),
            confirmation_tool_call_id: identity_text(31),
            occurred_at_ms: 1_300,
        })
        .expect("the user-approved corridor is created");
    let error = service
        .admit_before_start(&bounded_request())
        .expect_err("the call does not satisfy the corridor effect selector");
    assert_eq!(error.code(), "programmatic_policy_corridor_unavailable");

    fixture.confirmations.set_corridor_consumed(0);
    fixture.confirmations.corridors.borrow_mut()[0].required_effect_selectors =
        vec!["child_delegation".to_owned()];
    let mut mismatching_request = bounded_request();
    mismatching_request.call.typed_input_constraints = vec![
        DescriptorInputConstraintSelectionV1::new("paths", 1, vec!["tests".to_owned()])
            .expect("the mismatching constraint is valid"),
    ];
    let error = service
        .admit_before_start(&mismatching_request)
        .expect_err("the call does not satisfy a selected typed input constraint");
    assert_eq!(
        error.code(),
        "programmatic_policy_input_constraint_mismatch"
    );
}

#[test]
fn a_bounded_corridor_denial_releases_the_new_reservation() {
    let fixture = Fixture::new();
    fixture
        .confirmations
        .store_confirmation(accepted_confirmation(31, 30));
    let service = fixture.service();
    service
        .approve_corridor(&ProgrammaticCorridorApprovalInputDto {
            root_session_id: identity(5),
            root_run_id: identity(6),
            root_origin_kind: ProgrammaticRootOriginKindV1::ContinualHarness,
            effective_policy_snapshot_digest: digest(13),
            effective_policy_snapshot_reference: identity_text(8),
            required_effect_selectors: vec![ProgrammaticToolEffectFlagV1::ChildDelegation],
            exact_tool_or_mcp_method_selectors: vec![
                ProgrammaticRuleSelectorV1::exact_tool("sub_agent", 1)
                    .expect("the fixture selector is valid"),
            ],
            descriptor_input_constraint_selections: vec![corridor_constraint()],
            maximum_action_count: 8,
            maximum_concurrent_actions: 2,
            confirmation_reference: identity_text(30),
            confirmation_tool_call_id: identity_text(31),
            occurred_at_ms: 1_300,
        })
        .expect("the user-approved corridor is created");

    fixture.confirmations.set_corridor_consumed(8);
    let error = service
        .admit_before_start(&bounded_request())
        .expect_err("the exhausted corridor denies the delegation");
    assert_eq!(error.code(), "programmatic_policy_corridor_exhausted");
    let reservations = fixture.admissions.reservations();
    assert_eq!(reservations.len(), 1);
    assert_eq!(
        reservations[0].state,
        ProgrammaticReservationStateDto::ReleasedOnKnownPreEffect,
        "a corridor denial releases the reservation it just created"
    );
    assert_eq!(fixture.admissions.counters_state().run_reserved_actions, 0);
}

#[test]
fn an_over_bound_corridor_is_rejected() {
    let fixture = Fixture::new();
    fixture
        .confirmations
        .store_confirmation(accepted_confirmation(31, 30));
    let service = fixture.service();
    let error = service
        .approve_corridor(&ProgrammaticCorridorApprovalInputDto {
            root_session_id: identity(5),
            root_run_id: identity(6),
            root_origin_kind: ProgrammaticRootOriginKindV1::ContinualHarness,
            effective_policy_snapshot_digest: digest(13),
            effective_policy_snapshot_reference: identity_text(8),
            required_effect_selectors: vec![ProgrammaticToolEffectFlagV1::ChildDelegation],
            exact_tool_or_mcp_method_selectors: vec![
                ProgrammaticRuleSelectorV1::exact_tool("sub_agent", 1)
                    .expect("the fixture selector is valid"),
            ],
            descriptor_input_constraint_selections: vec![corridor_constraint()],
            maximum_action_count: 257,
            maximum_concurrent_actions: 2,
            confirmation_reference: identity_text(30),
            confirmation_tool_call_id: identity_text(31),
            occurred_at_ms: 1_300,
        })
        .expect_err("a corridor stays inside the closed 256/16 action bound");
    assert_eq!(error.code(), "programmatic_policy_limit_exceeded");
    assert_eq!(fixture.count("corridor-create"), 0);
}

#[test]
fn reservation_permanence_survives_cancellation_and_recovery() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    let confirmation = awaiting(
        service
            .admit_before_start(&request)
            .expect("the first attempt awaits its exact confirmation"),
    );
    service
        .decide_exact_confirmation(
            &DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: confirmation.confirmation_id,
                tool_call_id: confirmation.tool_call_id,
                typed_input_digest: confirmation.typed_input_digest,
                state: ProgrammaticConfirmationStateDto::Accepted,
                decided_at_ms: 1_500,
            },
            ProgrammaticConfirmationStateDto::Accepted,
        )
        .expect("the exact confirmation is accepted");
    let admitted = admitted(
        service
            .admit_before_start(&request)
            .expect("the accepted confirmation admits the call"),
    );
    let started = service
        .commit_tool_call_started(&CommitProgrammaticReservationStartedInputDto {
            reservation_reference: admitted.reservations[0].clone(),
            tool_call_id: identity_text(7),
            started_at_ms: 1_700,
        })
        .expect("the call reached ToolCallStarted");
    assert!(started.permanent);
    assert_eq!(
        started.reservation.state,
        ProgrammaticReservationStateDto::PermanentOnStart
    );

    let recovered = service
        .recover_outstanding(&ProgrammaticReservationRecoveryInputDto {
            root_run_id: identity_text(6),
            started_tool_call_ids: vec![identity_text(7)],
            recovered_at_ms: 1_800,
        })
        .expect("the outstanding reservation is recovered");
    assert_eq!(recovered.len(), 1);
    assert_eq!(
        recovered[0].disposition,
        ProgrammaticRecoveryDispositionV1::ExternalEffectUnknown
    );
    assert!(!recovered[0].external_work_resumes);
    assert_eq!(
        fixture.admissions.reservations()[0].state,
        ProgrammaticReservationStateDto::ExternalEffectUnknown,
        "a started ambiguous action is never retried or re-reserved"
    );
    assert_eq!(
        fixture.admissions.counters_state().run_started_actions,
        1,
        "the permanent consumption survives the recovery"
    );
}

#[test]
fn an_unstarted_reservation_is_released_by_recovery() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    let confirmation = awaiting(
        service
            .admit_before_start(&request)
            .expect("the first attempt awaits its exact confirmation"),
    );
    service
        .decide_exact_confirmation(
            &DecideProgrammaticPolicyConfirmationInputDto {
                confirmation_id: confirmation.confirmation_id,
                tool_call_id: confirmation.tool_call_id,
                typed_input_digest: confirmation.typed_input_digest,
                state: ProgrammaticConfirmationStateDto::Accepted,
                decided_at_ms: 1_500,
            },
            ProgrammaticConfirmationStateDto::Accepted,
        )
        .expect("the exact confirmation is accepted");
    service
        .admit_before_start(&request)
        .expect("the accepted confirmation admits the call");

    let recovered = service
        .recover_outstanding(&ProgrammaticReservationRecoveryInputDto {
            root_run_id: identity_text(6),
            started_tool_call_ids: vec![],
            recovered_at_ms: 1_800,
        })
        .expect("the unstarted reservation is released atomically");
    assert_eq!(recovered.len(), 1);
    assert_eq!(
        recovered[0].disposition,
        ProgrammaticRecoveryDispositionV1::InterruptedBeforeStart
    );
    assert!(!recovered[0].external_work_resumes);
    assert_eq!(fixture.admissions.counters_state().run_reserved_actions, 0);
}

#[test]
fn closing_corridors_requires_a_terminal_state() {
    let fixture = Fixture::new();
    fixture
        .confirmations
        .store_confirmation(accepted_confirmation(31, 30));
    let service = fixture.service();
    service
        .approve_corridor(&ProgrammaticCorridorApprovalInputDto {
            root_session_id: identity(5),
            root_run_id: identity(6),
            root_origin_kind: ProgrammaticRootOriginKindV1::ContinualHarness,
            effective_policy_snapshot_digest: digest(13),
            effective_policy_snapshot_reference: identity_text(8),
            required_effect_selectors: vec![ProgrammaticToolEffectFlagV1::ChildDelegation],
            exact_tool_or_mcp_method_selectors: vec![
                ProgrammaticRuleSelectorV1::exact_tool("sub_agent", 1)
                    .expect("the fixture selector is valid"),
            ],
            descriptor_input_constraint_selections: vec![corridor_constraint()],
            maximum_action_count: 8,
            maximum_concurrent_actions: 2,
            confirmation_reference: identity_text(30),
            confirmation_tool_call_id: identity_text(31),
            occurred_at_ms: 1_300,
        })
        .expect("the user-approved corridor is created");

    let error = service
        .close_corridors(
            &identity_text(6),
            ProgrammaticCorridorStateDto::Active,
            1_900,
        )
        .expect_err("a terminalized root tree closes its corridors as expired or revoked");
    assert_eq!(error.code(), "programmatic_policy_corridor_unavailable");

    let closed = service
        .close_corridors(
            &identity_text(6),
            ProgrammaticCorridorStateDto::Expired,
            1_900,
        )
        .expect("the terminal root tree closes its corridor");
    assert_eq!(closed, 1);
    assert_eq!(
        fixture.confirmations.corridors()[0].state,
        ProgrammaticCorridorStateDto::Expired
    );
}

#[test]
fn boundary_scalars_reject_paths_and_credentials() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let path_shaped = request(
        interactive_origin(),
        call(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            "read",
            vec![ProgrammaticToolEffectFlagV1::LocalRead],
            vec![],
        ),
        vec![],
    );
    let mut path_shaped = path_shaped;
    path_shaped.call.tool_id = "/tmp/project/data".to_owned();
    let error = service
        .admit_before_start(&path_shaped)
        .expect_err("a filesystem path never crosses the policy boundary");
    assert_eq!(error.code(), "programmatic_policy_not_applicable");

    let credential = DescriptorInputConstraintSelectionV1::new(
        "env",
        1,
        vec!["sk-live-fixture-secret".to_owned()],
    )
    .expect_err("a credential-shaped constraint value is refused at the domain boundary");
    assert_eq!(credential.code(), "credentials_forbidden");
}

/// One exact MCP method reference of the fixture connection.
fn mcp_method() -> ProgrammaticMcpMethodReferenceV1 {
    ProgrammaticMcpMethodReferenceV1::new(identity(14), "tools/call", "1")
        .expect("the fixture MCP method reference is valid")
}

/// One typed admission call of the fixture MCP tool.
fn mcp_call(
    root_origin: ProgrammaticRootOriginKindV1,
    effect_flags: Vec<ProgrammaticToolEffectFlagV1>,
) -> ProgrammaticAdmissionCallV1 {
    ProgrammaticAdmissionCallV1::new(
        ProgrammaticCallContextV1 {
            root_origin,
            root_session_id: identity(5),
            root_run_id: identity(6),
            current_session_id: identity(5),
            current_run_id: identity(6),
            tool_call_id: identity(7),
        },
        "mcp",
        1,
        effect_flags,
        Some(mcp_method()),
        vec![corridor_constraint()],
        digest(11),
    )
    .expect("the fixture MCP call is valid")
}

/// One bounded admission request of the fixture MCP tool.
fn mcp_request(
    root_origin: ProgrammaticRootOriginKindV1,
    effect_flags: Vec<ProgrammaticToolEffectFlagV1>,
) -> ProgrammaticAdmissionRequestDto {
    let origin = match root_origin {
        ProgrammaticRootOriginKindV1::InteractiveUser => interactive_origin(),
        ProgrammaticRootOriginKindV1::ContinualHarness => harness_origin(),
    };
    request(
        origin,
        mcp_call(root_origin, effect_flags),
        vec![applicable(
            1,
            ProgrammaticAdmissionRuleV1::BoundedConfirmationRequired,
            "mcp",
        )],
    )
}

/// One policy-backed interactive direct-local-read request.
fn direct_local_read_request() -> ProgrammaticAdmissionRequestDto {
    request(
        interactive_origin(),
        call(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            "read",
            vec![ProgrammaticToolEffectFlagV1::LocalRead],
            vec![],
        ),
        vec![applicable(
            1,
            ProgrammaticAdmissionRuleV1::DirectLocalRead,
            "read",
        )],
    )
}

/// One exact tool selector of the fixture corridor.
fn exact_selector(tool_id: &str) -> ProgrammaticRuleSelectorV1 {
    ProgrammaticRuleSelectorV1::exact_tool(tool_id, 1).expect("the fixture selector is valid")
}

/// One corridor approval input over the fixture root tree.
fn corridor_approval(
    required_effect_selectors: Vec<ProgrammaticToolEffectFlagV1>,
    exact_tool_or_mcp_method_selectors: Vec<ProgrammaticRuleSelectorV1>,
) -> ProgrammaticCorridorApprovalInputDto {
    ProgrammaticCorridorApprovalInputDto {
        root_session_id: identity(5),
        root_run_id: identity(6),
        root_origin_kind: ProgrammaticRootOriginKindV1::ContinualHarness,
        effective_policy_snapshot_digest: digest(13),
        effective_policy_snapshot_reference: identity_text(8),
        required_effect_selectors,
        exact_tool_or_mcp_method_selectors,
        descriptor_input_constraint_selections: vec![corridor_constraint()],
        maximum_action_count: 8,
        maximum_concurrent_actions: 2,
        confirmation_reference: identity_text(30),
        confirmation_tool_call_id: identity_text(31),
        occurred_at_ms: 1_300,
    }
}

#[test]
fn admission_boundary_scalars_reject_blank_over_bound_and_unusable_input() {
    let fixture = Fixture::new();
    let service = fixture.service();

    let mut blank_snapshot = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    blank_snapshot.snapshot_id = "   ".to_owned();
    let error = service
        .admit_before_start(&blank_snapshot)
        .expect_err("a blank snapshot identity never crosses the boundary");
    assert_eq!(error.code(), "programmatic_policy_not_applicable");

    let mut oversized_snapshot =
        exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    oversized_snapshot.snapshot_id = "a".repeat(257);
    let error = service
        .admit_before_start(&oversized_snapshot)
        .expect_err("an over-long boundary scalar is refused");
    assert_eq!(error.code(), "programmatic_policy_limit_exceeded");

    let mut credential_basis =
        exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    credential_basis.admission_basis_reference = "api_key=fixture-value".to_owned();
    let error = service
        .admit_before_start(&credential_basis)
        .expect_err("a credential-shaped basis reference is refused");
    assert_eq!(error.code(), "credentials_forbidden");

    let mut blank_basis = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    blank_basis.admission_basis_reference = String::new();
    let error = service
        .admit_before_start(&blank_basis)
        .expect_err("an admission basis reference is never blank");
    assert_eq!(error.code(), "programmatic_policy_not_applicable");

    let mut blank_tool = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    blank_tool.call.tool_id = "\t\n".to_owned();
    let error = service
        .admit_before_start(&blank_tool)
        .expect_err("a blank tool identity never names a registry tool");
    assert_eq!(error.code(), "programmatic_policy_not_applicable");

    let mut incoherent_window =
        exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    incoherent_window.calendar_window.end_ms = 1_000;
    let error = service
        .admit_before_start(&incoherent_window)
        .expect_err("a calendar window has a positive bounded duration");
    assert_eq!(error.code(), "programmatic_policy_counter_unavailable");

    let mut blank_zone = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    blank_zone.calendar_window.time_zone = String::new();
    let error = service
        .admit_before_start(&blank_zone)
        .expect_err("a calendar window keeps its project time zone");
    assert_eq!(error.code(), "programmatic_policy_not_applicable");
}

#[test]
fn an_mcp_call_with_an_unusable_method_reference_fails_closed() {
    let fixture = Fixture::new();
    let service = fixture.service();

    let mut blank_method = mcp_request(
        ProgrammaticRootOriginKindV1::ContinualHarness,
        vec![ProgrammaticToolEffectFlagV1::McpInvocation],
    );
    blank_method
        .call
        .mcp_method
        .as_mut()
        .expect("the fixture call carries its MCP method")
        .method_reference = String::new();
    let error = service
        .admit_before_start(&blank_method)
        .expect_err("an MCP method needs a safe name");
    assert_eq!(error.code(), "programmatic_policy_not_applicable");

    let mut blank_revision = mcp_request(
        ProgrammaticRootOriginKindV1::ContinualHarness,
        vec![ProgrammaticToolEffectFlagV1::McpInvocation],
    );
    blank_revision
        .call
        .mcp_method
        .as_mut()
        .expect("the fixture call carries its MCP method")
        .method_schema_revision = String::new();
    let error = service
        .admit_before_start(&blank_revision)
        .expect_err("an MCP method needs a safe schema revision");
    assert_eq!(error.code(), "programmatic_policy_not_applicable");
}

#[test]
fn a_direct_local_read_policy_admits_replays_and_rejects_an_unreadable_basis() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = direct_local_read_request();

    let first = admitted(
        service
            .admit_before_start(&request)
            .expect("the selected direct-local-read policy admits the read"),
    );
    assert_eq!(first.decision, ProgrammaticAdmissionRuleV1::DirectLocalRead);
    assert_eq!(
        first.snapshot_id.as_deref(),
        Some(identity_text(8).as_str())
    );
    assert_eq!(first.reservations.len(), 1);
    assert!(!first.replayed);

    let replay = admitted(
        service
            .admit_before_start(&request)
            .expect("an equal repeated attempt replays the accepted binding"),
    );
    assert!(replay.replayed);
    assert_eq!(replay.reservations, first.reservations);
    assert_eq!(fixture.count("reservation-reserve"), 1);

    let mut unreadable = request;
    unreadable.admission_basis_reference = "g".repeat(32);
    let error = service
        .admit_before_start(&unreadable)
        .expect_err("an admission basis reference is canonical identity text");
    assert_eq!(error.code(), "invalid_runtime_identity");
}

#[test]
fn weekly_and_monthly_calendar_limits_reserve_their_own_counters() {
    for period in [
        ProgrammaticCalendarPeriodKindV1::Week,
        ProgrammaticCalendarPeriodKindV1::Month,
    ] {
        let fixture = Fixture::new();
        let service = fixture.service();
        let mut selection = applicable(1, ProgrammaticAdmissionRuleV1::DirectLocalRead, "read");
        selection.revision_record.calendar_limit = ProgrammaticCalendarLimitV1::new(period, 8)
            .expect("the fixture calendar limit is valid");
        let request = request(
            interactive_origin(),
            call(
                ProgrammaticRootOriginKindV1::InteractiveUser,
                "read",
                vec![ProgrammaticToolEffectFlagV1::LocalRead],
                vec![],
            ),
            vec![selection],
        );
        let evidence = admitted(
            service
                .admit_before_start(&request)
                .expect("a policy-backed read is admitted"),
        );
        assert_eq!(evidence.reservations.len(), 1);
        assert_eq!(
            fixture
                .admissions
                .counters_state()
                .calendar_reserved_actions,
            1
        );
    }
}

#[test]
fn a_non_root_interactive_ask_user_call_fails_closed() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let mut request = request(
        interactive_origin(),
        call(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            "ask_user",
            vec![ProgrammaticToolEffectFlagV1::UserInteraction],
            vec![],
        ),
        vec![applicable(
            1,
            ProgrammaticAdmissionRuleV1::ExactConfirmationRequired,
            "ask_user",
        )],
    );
    request.call.context.current_run_id = identity(13);
    let error = service
        .admit_before_start(&request)
        .expect_err("only the root run starts the user interaction");
    assert_eq!(error.code(), "programmatic_policy_root_only_interaction");
}

#[test]
fn a_bounded_call_without_its_corridor_fails_closed() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = request(
        interactive_origin(),
        call(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            "read",
            vec![ProgrammaticToolEffectFlagV1::LocalRead],
            vec![],
        ),
        vec![applicable(
            1,
            ProgrammaticAdmissionRuleV1::BoundedConfirmationRequired,
            "read",
        )],
    );
    let error = service
        .admit_before_start(&request)
        .expect_err("a bounded decision needs the user-approved corridor of its root tree");
    assert_eq!(error.code(), "programmatic_policy_corridor_unavailable");
}

#[test]
fn an_undeclared_input_family_fails_closed() {
    let fixture = Fixture::new();
    fixture
        .confirmations
        .store_confirmation(accepted_confirmation(31, 30));
    let service = fixture.service();
    service
        .approve_corridor(&corridor_approval(
            vec![ProgrammaticToolEffectFlagV1::ChildDelegation],
            vec![exact_selector("sub_agent")],
        ))
        .expect("the user-approved corridor is created");

    let mut request = bounded_request();
    request.descriptor_constraint_family_declared = false;
    let error = service
        .admit_before_start(&request)
        .expect_err("a bounded corridor needs a descriptor-declared typed input family");
    assert_eq!(
        error.code(),
        "programmatic_policy_input_constraint_mismatch"
    );
}

#[test]
fn an_exact_selector_mismatch_fails_closed() {
    let fixture = Fixture::new();
    fixture
        .confirmations
        .store_confirmation(accepted_confirmation(31, 30));
    let service = fixture.service();
    service
        .approve_corridor(&corridor_approval(
            vec![ProgrammaticToolEffectFlagV1::LocalRead],
            vec![exact_selector("other_tool")],
        ))
        .expect("the user-approved corridor is created");
    let request = request(
        interactive_origin(),
        call(
            ProgrammaticRootOriginKindV1::InteractiveUser,
            "read",
            vec![ProgrammaticToolEffectFlagV1::LocalRead],
            vec![corridor_constraint()],
        ),
        vec![applicable(
            1,
            ProgrammaticAdmissionRuleV1::BoundedConfirmationRequired,
            "read",
        )],
    );
    let error = service
        .admit_before_start(&request)
        .expect_err("the call does not satisfy any corridor exact selector");
    assert_eq!(error.code(), "programmatic_policy_corridor_unavailable");
}

#[test]
fn an_mcp_corridor_binds_every_method_and_effect_label() {
    let fixture = Fixture::new();
    fixture
        .confirmations
        .store_confirmation(accepted_confirmation(31, 30));
    let service = fixture.service();
    let corridor = service
        .approve_corridor(&corridor_approval(
            vec![
                ProgrammaticToolEffectFlagV1::LocalRead,
                ProgrammaticToolEffectFlagV1::LocalWrite,
                ProgrammaticToolEffectFlagV1::LocalExecute,
                ProgrammaticToolEffectFlagV1::NetworkAccess,
                ProgrammaticToolEffectFlagV1::ChildDelegation,
                ProgrammaticToolEffectFlagV1::UserInteraction,
                ProgrammaticToolEffectFlagV1::McpInvocation,
            ],
            vec![
                ProgrammaticRuleSelectorV1::effect_profile(vec![
                    ProgrammaticToolEffectFlagV1::LocalRead,
                    ProgrammaticToolEffectFlagV1::LocalWrite,
                ])
                .expect("the fixture effect selector is valid"),
                ProgrammaticRuleSelectorV1::mcp_method(identity(14), "tools/call", "1")
                    .expect("the fixture method selector is valid"),
            ],
        ))
        .expect("the user-approved MCP corridor is created");

    let request = mcp_request(
        ProgrammaticRootOriginKindV1::ContinualHarness,
        vec![
            ProgrammaticToolEffectFlagV1::LocalRead,
            ProgrammaticToolEffectFlagV1::LocalWrite,
            ProgrammaticToolEffectFlagV1::LocalExecute,
            ProgrammaticToolEffectFlagV1::NetworkAccess,
            ProgrammaticToolEffectFlagV1::ChildDelegation,
            ProgrammaticToolEffectFlagV1::UserInteraction,
            ProgrammaticToolEffectFlagV1::McpInvocation,
        ],
    );
    let first = admitted(
        service
            .admit_before_start(&request)
            .expect("the bounded corridor admits the exact MCP call"),
    );
    assert_eq!(
        first.corridor_digest.as_deref(),
        Some(corridor.corridor_digest.as_str())
    );
    assert_eq!(
        fixture.confirmations.corridors()[0].consumed_action_count,
        1
    );

    let replay = admitted(
        service
            .admit_before_start(&request)
            .expect("an equal repeated attempt replays the corridor admission"),
    );
    assert!(replay.replayed);
    assert_eq!(replay.reservations, first.reservations);
    assert_eq!(
        fixture.confirmations.corridors()[0].consumed_action_count,
        1,
        "a replay never consumes a second corridor action"
    );
}

#[test]
fn the_run_concurrency_limit_denies_a_second_admission() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = direct_local_read_request();
    service
        .admit_before_start(&request)
        .expect("the first policy-backed read is admitted");

    fixture
        .admissions
        .set_counters(ProgrammaticPolicyCounterRecordDto {
            run_in_flight_actions: 2,
            ..counters()
        });
    let mut concurrent = request;
    concurrent.call.context.tool_call_id = identity(13);
    let error = service
        .admit_before_start(&concurrent)
        .expect_err("the per-run concurrency limit is exhausted");
    assert_eq!(error.code(), "programmatic_policy_run_limit_exceeded");
}

#[test]
fn recovery_skips_a_released_reservation_and_rejects_a_missing_start() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = direct_local_read_request();
    let admitted = admitted(
        service
            .admit_before_start(&request)
            .expect("the policy-backed read is admitted"),
    );
    service
        .release_before_start(&ReleaseProgrammaticPolicyReservationInputDto {
            reservation_reference: admitted.reservations[0].clone(),
            tool_call_id: identity_text(7),
            released_at_ms: 1_600,
        })
        .expect("a known pre-effect outcome releases the reservation");
    let skipped = service
        .recover_outstanding(&ProgrammaticReservationRecoveryInputDto {
            root_run_id: identity_text(6),
            started_tool_call_ids: vec![],
            recovered_at_ms: 1_800,
        })
        .expect("a terminal reservation is skipped by recovery");
    assert!(skipped.is_empty());

    let fixture = Fixture::new();
    let service = fixture.service();
    service
        .admit_before_start(&direct_local_read_request())
        .expect("the policy-backed read is admitted");
    let error = service
        .recover_outstanding(&ProgrammaticReservationRecoveryInputDto {
            root_run_id: identity_text(6),
            started_tool_call_ids: vec![identity_text(7)],
            recovered_at_ms: 1_800,
        })
        .expect_err("a reserved-but-unstarted action never claims an external effect");
    assert_eq!(error.code(), "programmatic_policy_reservation_conflict");
}

#[test]
fn the_exact_confirmation_is_created_then_read_back() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    let created = service
        .request_exact_confirmation(&ProgrammaticExactConfirmationRequestDto {
            request: request.clone(),
        })
        .expect("the exact confirmation request creates the one awaiting record");
    assert_eq!(created.state, ProgrammaticConfirmationStateDto::Awaiting);
    assert_eq!(fixture.count("confirmation-create"), 1);

    let read_back = service
        .request_exact_confirmation(&ProgrammaticExactConfirmationRequestDto { request })
        .expect("an equal repeated request reads the existing confirmation");
    assert_eq!(read_back, created);
    assert_eq!(fixture.count("confirmation-create"), 1);
}

#[test]
fn an_accepted_confirmation_without_a_decision_time_fails_closed() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let request = exact_request(ProgrammaticAdmissionRuleV1::ExactConfirmationRequired);
    service
        .admit_before_start(&request)
        .expect("the first attempt awaits its exact confirmation");
    {
        let mut confirmations = fixture.confirmations.confirmations.borrow_mut();
        let stored = confirmations
            .first_mut()
            .expect("the awaiting confirmation is stored");
        stored.state = ProgrammaticConfirmationStateDto::Accepted;
        stored.decided_at_ms = None;
    }
    let error = service
        .admit_before_start(&request)
        .expect_err("an accepted confirmation carries its decision time");
    assert_eq!(error.code(), "programmatic_policy_confirmation_expired");
}

#[test]
fn a_corridor_approval_is_bound_to_its_exact_accepted_confirmation() {
    let fixture = Fixture::new();
    fixture
        .confirmations
        .store_confirmation(accepted_confirmation(31, 30));
    let service = fixture.service();

    let mut blank_snapshot = corridor_approval(
        vec![ProgrammaticToolEffectFlagV1::ChildDelegation],
        vec![exact_selector("sub_agent")],
    );
    blank_snapshot.effective_policy_snapshot_reference = String::new();
    let error = service
        .approve_corridor(&blank_snapshot)
        .expect_err("a corridor needs a safe snapshot reference");
    assert_eq!(error.code(), "programmatic_policy_not_applicable");

    let mut blank_confirmation = corridor_approval(
        vec![ProgrammaticToolEffectFlagV1::ChildDelegation],
        vec![exact_selector("sub_agent")],
    );
    blank_confirmation.confirmation_reference = "  ".to_owned();
    let error = service
        .approve_corridor(&blank_confirmation)
        .expect_err("a corridor needs a safe confirmation reference");
    assert_eq!(error.code(), "programmatic_policy_not_applicable");

    let mut unbound = corridor_approval(
        vec![ProgrammaticToolEffectFlagV1::ChildDelegation],
        vec![exact_selector("sub_agent")],
    );
    unbound.confirmation_reference = identity_text(32);
    let error = service
        .approve_corridor(&unbound)
        .expect_err("a corridor needs its accepted approval confirmation");
    assert_eq!(error.code(), "programmatic_policy_confirmation_required");

    let empty = corridor_approval(vec![], vec![]);
    let error = service
        .approve_corridor(&empty)
        .expect_err("a corridor selects at least one effect or exact tool");
    assert_eq!(error.code(), "programmatic_policy_corridor_unavailable");

    let corridor = service
        .approve_corridor(&corridor_approval(
            vec![ProgrammaticToolEffectFlagV1::ChildDelegation],
            vec![exact_selector("sub_agent")],
        ))
        .expect("the user-approved corridor is created");
    assert_eq!(corridor.state, ProgrammaticCorridorStateDto::Active);
}
