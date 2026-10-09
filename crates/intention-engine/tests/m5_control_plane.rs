#![allow(
    clippy::expect_used,
    reason = "Control-plane fixtures use expect to provide precise test failures."
)]

//! Slice 2 engine contracts for catalog preparation, provider selection, and
//! typed cross-turn reasoning history.

mod common;

use std::sync::PoisonError;

use common::{FakeRepository, RecordingSessionSink, ScriptedRegistry, time};
use intention_config::RawConfigInputDto;
use intention_config::catalog::CatalogCandidate;
use intention_engine::catalog::{
    CatalogActivationRecoveryDto, CatalogCandidateInputDto, CatalogChangeKindDto,
    CatalogRemovalEntryDto, accept_catalog_revision, mark_catalog_activated,
    prepare_catalog_candidate, recover_catalog_activation, reject_catalog_candidate,
};
use intention_engine::reasoning::{
    MAX_RUN_REASONING_AGGREGATE_BYTES, RunReasoningAggregate, build_history_manifest,
    history_bound, resolve_reasoning_history, verify_and_build_history,
};
use intention_engine::selection::{
    SelectionResolutionDto, SelectionResolutionInputDto, load_profile_usage, resolve_selection,
    set_session_provider_profile,
};
use intention_proto::provider::{
    ProviderProfileId, ProviderProfileOverrideDto, ProviderProfilePolicyDto,
    ProviderSelectionSourceDto, ProviderSelectionUnavailabilityDto, ReasoningFragmentCategoryDto,
    ReasoningHistoryTransferDto, SetSessionProviderProfileCommandDto,
};
use intention_proto::{DtoResult, ErrorDto, IdempotencyKey, RunId, SessionId};
use intention_storage::{ProviderCatalogRevisionDto, SessionProviderProfileChangeDto};
use intention_test_support::{fixture_reasoning_history, fixture_snapshot};

/// The provider credential every fixture document carries; never a real value.
const FIXTURE_CREDENTIAL: &str = "fixture-credential-not-real-12345";

/// A second distinct fixture credential for the second profile.
const SECOND_FIXTURE_CREDENTIAL: &str = "fixture-credential-not-real-67890";

/// The global catalog section both fixture documents share.
const GLOBAL_SECTION: &str = "schema_version = 1\n\n[provider]\ncontext_window_tokens = 180000\ndefault_profile = \"main\"\n";

/// One declared user kind whose protocol parts the immutability contract freezes.
const USER_KIND_SECTION: &str = "\n[providers.kinds.my-dialect]\nkind_id = \"my-dialect\"\nstream = \"chat_completions_sse\"\nreasoning = \"reasoning_content\"\nactivation = \"thinking_enabled\"\neffort = \"reasoning_effort\"\ncredential_transport = { header = \"X-Api-Key\" }\n";

/// Returns the `main` profile declaration of the fixture catalog.
fn main_profile() -> String {
    format!(
        "\n[providers.profiles.main]\nkind = \"openrouter\"\nmodel = \"gpt-5.6-terra\"\ncredential = \"{FIXTURE_CREDENTIAL}\"\ndisplay_name = \"Main\"\nenabled = true\nreasoning_effort = \"medium\"\n\n[providers.profiles.main.execution]\nattempt_timeout_seconds = 45\nmax_attempts = 2\n\n[providers.profiles.main.capabilities]\ntext_streaming = true\nreasoning = \"textual_reasoning_v1\"\nreasoning_efforts = [\"medium\", \"low\"]\ntool_exchange = true\n"
    )
}

/// Returns the `local` profile declaration of the fixture catalog.
fn local_profile() -> String {
    format!(
        "\n[providers.profiles.local]\nkind = \"generic-chat-completion-api\"\nmodel = \"example-chat-model\"\ncredential = \"{SECOND_FIXTURE_CREDENTIAL}\"\nendpoint = \"http://127.0.0.1:8080/v1/\"\ndisplay_name = \"Local\"\nenabled = true\n\n[providers.profiles.local.capabilities]\ntext_streaming = true\n"
    )
}

/// Returns one fixture catalog text with exactly the requested members.
fn catalog_text(kinds: &str, profiles: &str) -> String {
    format!("{GLOBAL_SECTION}{kinds}{profiles}")
}

/// Returns one fixture catalog text that declares no global default profile.
fn catalog_text_without_default(kinds: &str, profiles: &str) -> String {
    format!("schema_version = 1\n\n[provider]\ncontext_window_tokens = 180000\n{kinds}{profiles}")
}

/// Parses one credential-free fixture document from catalog text.
fn document(text: &str) -> intention_config::catalog::CatalogDocumentDto {
    CatalogCandidate::parse(RawConfigInputDto::new(text))
        .expect("the fixture catalog document resolves")
        .safe_document()
        .clone()
}

/// Prepares one fixture document against the supplied accepted revision.
fn prepare(
    text: &str,
    active: Option<&ProviderCatalogRevisionDto>,
) -> DtoResult<intention_engine::catalog::CatalogPreparationDto> {
    prepare_catalog_candidate(&CatalogCandidateInputDto::new(
        document(text),
        active.cloned(),
    ))
}

/// Returns the fixture profile identity of one label.
fn profile(label: &str) -> ProviderProfileId {
    ProviderProfileId::parse(label).expect("the fixture profile identity is valid")
}

/// Returns one fixture transfer contract over the fixture compatibility identity.
fn transfer() -> ReasoningHistoryTransferDto {
    ReasoningHistoryTransferDto::textual_history_v1("fixture-compatibility-v1")
        .expect("the fixture transfer contract is valid")
}

#[test]
fn catalog_preparation_is_a_catalog_affecting_candidate_on_a_fresh_accept() {
    let preparation =
        prepare(&catalog_text("", &main_profile()), None).expect("the fixture candidate prepares");

    assert_eq!(
        preparation.change(),
        CatalogChangeKindDto::CatalogAffecting,
        "a candidate without an accepted revision is catalog affecting"
    );
    assert!(preparation.expected_active_revision_id().is_none());
    assert!(preparation.removals().is_empty());
    assert!(!preparation.is_removal_candidate());
    assert!(preparation.candidate_handle().is_none());
    assert_eq!(
        preparation
            .default_profile_id()
            .map(ProviderProfileId::as_str),
        Some("main")
    );
    assert_eq!(preparation.profile_revisions().len(), 1);
    assert_eq!(preparation.profile_policies().len(), 1);
    // Both code-owned first-party descriptors are always present, so a catalog
    // never becomes a removal candidate for a kind the tree can drive.
    assert_eq!(preparation.kind_descriptors().len(), 2);
}

#[test]
fn catalog_preparation_derives_one_stable_revision_identity_per_meaning() {
    let first =
        prepare(&catalog_text("", &main_profile()), None).expect("the fixture candidate prepares");
    let second = prepare(&catalog_text("", &main_profile()), None)
        .expect("the equal fixture candidate prepares");

    assert_eq!(
        first.candidate_revision_id(),
        second.candidate_revision_id(),
        "a semantically equal catalog derives the same revision identity"
    );

    let changed = prepare(
        &catalog_text("", &main_profile().replace("gpt-5.6-terra", "gpt-5.6-luna")),
        None,
    )
    .expect("the changed fixture candidate prepares");
    assert_ne!(
        first.candidate_revision_id(),
        changed.candidate_revision_id(),
        "a different model identity derives a different revision identity"
    );
}

#[test]
fn accepting_an_unchanged_candidate_writes_no_new_revision() {
    let prepared =
        prepare(&catalog_text("", &main_profile()), None).expect("the fixture candidate prepares");
    let accepted = prepared
        .revision_at(time(10))
        .expect("the revision is representable");
    let repository = FakeRepository::new(SessionId::new(), RunId::new(), fixture_snapshot());

    let preparation = prepare(&catalog_text("", &main_profile()), Some(&accepted))
        .expect("the equal candidate prepares");
    assert_eq!(preparation.change(), CatalogChangeKindDto::Unchanged);
    assert_eq!(
        preparation.expected_active_revision_id(),
        Some(accepted.catalog_revision_id())
    );

    let accepted_id = accept_catalog_revision(&repository, &preparation, time(11))
        .expect("an unchanged acceptance commits");
    assert_eq!(accepted_id, accepted.catalog_revision_id());
    assert!(
        repository
            .accepted_catalog_revisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty(),
        "an unchanged candidate commits no new revision"
    );
}

#[test]
fn accepting_and_activating_a_candidate_commits_the_prepared_identity() {
    let repository = FakeRepository::new(SessionId::new(), RunId::new(), fixture_snapshot());
    let preparation =
        prepare(&catalog_text("", &main_profile()), None).expect("the fixture candidate prepares");
    let expected = preparation
        .revision_at(time(20))
        .expect("the revision is representable");

    let accepted_id = accept_catalog_revision(&repository, &preparation, time(20))
        .expect("the candidate is accepted");
    assert_eq!(accepted_id, preparation.candidate_revision_id());
    let committed = repository
        .accepted_catalog_revisions
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(committed, vec![expected]);
    drop(committed);

    let state = mark_catalog_activated(&repository, accepted_id, time(21))
        .expect("the accepted revision activates");
    assert_eq!(state.accepted_catalog_revision_id(), Some(accepted_id));
    assert_eq!(state.activated_catalog_revision_id(), Some(accepted_id));
    assert!(!state.requires_activation_recovery());
}

#[test]
fn policy_only_edits_classify_as_fresh_runs_only() {
    let prepared =
        prepare(&catalog_text("", &main_profile()), None).expect("the fixture candidate prepares");
    let accepted = prepared
        .revision_at(time(30))
        .expect("the revision is representable");

    // The display name and enabled state are policy, not revision-affecting
    // meaning, so the candidate stays on the same revision identity.
    let edited = prepared
        .profile_policies()
        .iter()
        .map(|entry| {
            intention_storage::ProviderProfilePolicyEntryDto::new(
                entry.profile_id().clone(),
                ProviderProfilePolicyDto::new("Renamed", false, None)
                    .expect("the fixture policy is valid"),
            )
        })
        .collect::<Vec<_>>();
    let policy_only = ProviderCatalogRevisionDto::new(
        accepted.catalog_revision_id(),
        accepted.default_profile_id().cloned(),
        time(30),
        accepted.kind_descriptors().to_vec(),
        accepted.profile_revisions().to_vec(),
        edited,
    )
    .expect("the policy-edited revision is coherent");

    let preparation = prepare(&catalog_text("", &main_profile()), Some(&policy_only))
        .expect("the policy-edited candidate prepares");
    assert_eq!(preparation.change(), CatalogChangeKindDto::FreshRunsOnly);
    assert_eq!(
        preparation.candidate_revision_id(),
        accepted.catalog_revision_id(),
        "the presentation edit never moves the revision identity"
    );
}

#[test]
fn a_removal_candidate_exposes_a_pending_handle_and_rejection_records_audit() {
    let full = catalog_text("", &format!("{}{}", main_profile(), local_profile()));
    let prepared = prepare(&full, None).expect("the two-profile candidate prepares");
    let accepted = prepared
        .revision_at(time(40))
        .expect("the revision is representable");

    let reduced = prepare(&catalog_text("", &main_profile()), Some(&accepted))
        .expect("the reduced candidate prepares");
    assert!(reduced.is_removal_candidate());
    assert_eq!(
        reduced.removals(),
        &[CatalogRemovalEntryDto::Profile {
            profile_id: profile("local")
        }]
    );
    let handle = reduced
        .candidate_handle()
        .expect("a removal candidate carries a pending handle");
    assert_eq!(
        handle.candidate_revision_id(),
        reduced.candidate_revision_id()
    );
    assert_eq!(
        handle.expected_active_revision_id(),
        accepted.catalog_revision_id()
    );

    let repository = FakeRepository::new(SessionId::new(), RunId::new(), fixture_snapshot());
    let rejected = reject_catalog_candidate(&repository, handle, time(41))
        .expect("the removal candidate is rejected");
    assert_eq!(
        rejected.active_catalog_revision_id(),
        Some(accepted.catalog_revision_id())
    );
    assert_eq!(
        repository
            .rejected_candidates
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
        vec![reduced.candidate_revision_id()],
        "rejection records the pending candidate and emits no acceptance"
    );
    assert!(
        repository
            .accepted_catalog_revisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    );
}

#[test]
fn omitting_a_user_kind_and_its_profile_reports_both_removals() {
    // A document can never keep a profile whose kind it omits: the engine's own
    // `provider_kind_has_dependents` guarantee is also enforced earlier by the
    // catalog document parser. What remains expressible is removing the kind
    // together with the profile that declared it.
    let declared = format!(
        "\n[providers.profiles.dialect]\nkind = \"my-dialect\"\nmodel = \"dialect-model\"\ncredential = \"{FIXTURE_CREDENTIAL}\"\nendpoint = \"https://dialect.example.invalid/v1/\"\ndisplay_name = \"Dialect\"\nenabled = true\ncredential_transport = {{ header = \"X-Api-Key\" }}\n\n[providers.profiles.dialect.capabilities]\ntext_streaming = true\n"
    );
    let with_kind = catalog_text(
        USER_KIND_SECTION,
        &format!("{}{}", main_profile(), declared),
    );
    let accepted = prepare(&with_kind, None)
        .expect("the user-kind candidate prepares")
        .revision_at(time(45))
        .expect("the revision is representable");
    assert_eq!(accepted.kind_descriptors().len(), 3);

    let preparation = prepare(&catalog_text("", &main_profile()), Some(&accepted))
        .expect("the reduced candidate prepares");
    assert_eq!(
        preparation.removals(),
        &[
            CatalogRemovalEntryDto::Profile {
                profile_id: profile("dialect")
            },
            CatalogRemovalEntryDto::Kind {
                kind_id: intention_proto::provider::ProviderKindId::parse("my-dialect")
                    .expect("the fixture kind identity is valid")
            },
        ],
        "a removed kind and its declaration profile are both tombstoned"
    );
    assert!(preparation.is_removal_candidate());
    assert!(preparation.candidate_handle().is_some());
}

#[test]
fn a_changed_kind_declaration_is_immutably_rejected() {
    let kinds = USER_KIND_SECTION;
    let declared = format!(
        "\n[providers.profiles.dialect]\nkind = \"my-dialect\"\nmodel = \"dialect-model\"\ncredential = \"{FIXTURE_CREDENTIAL}\"\nendpoint = \"https://dialect.example.invalid/v1/\"\ndisplay_name = \"Dialect\"\nenabled = true\ncredential_transport = {{ header = \"X-Api-Key\" }}\n\n[providers.profiles.dialect.capabilities]\ntext_streaming = true\n"
    );
    let accepted = prepare(&catalog_text_without_default(kinds, &declared), None)
        .expect("the user-kind candidate prepares")
        .revision_at(time(50))
        .expect("the revision is representable");

    let changed_kinds = kinds.replace(
        "effort = \"reasoning_effort\"",
        "effort = \"thinking_budget\"",
    );
    let error = prepare(
        &catalog_text_without_default(&changed_kinds, &declared),
        Some(&accepted),
    )
    .expect_err("a changed kind declaration is rejected");
    assert_eq!(error.code(), "provider_kind_immutable_mismatch");
}

#[test]
fn activation_recovery_requires_exactly_the_accepted_revision() {
    let repository = FakeRepository::new(SessionId::new(), RunId::new(), fixture_snapshot());
    let accepted = prepare(&catalog_text("", &main_profile()), None)
        .expect("the fixture candidate prepares")
        .revision_at(time(60))
        .expect("the revision is representable");

    let changed = prepare(
        &catalog_text("", &main_profile().replace("gpt-5.6-terra", "gpt-5.6-luna")),
        None,
    )
    .expect("the changed candidate prepares");
    let outcome = recover_catalog_activation(&repository, &accepted, &changed, time(61))
        .expect("a mismatch is a closed outcome, not a failure");
    assert_eq!(
        outcome,
        CatalogActivationRecoveryDto::Mismatch {
            accepted_catalog_revision_id: accepted.catalog_revision_id()
        }
    );
    assert!(!outcome.recovered_exactly());
    assert!(
        repository
            .activated_catalog_revisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty(),
        "a mismatch activates nothing"
    );

    let exact =
        prepare(&catalog_text("", &main_profile()), None).expect("the equal candidate prepares");
    let outcome = recover_catalog_activation(&repository, &accepted, &exact, time(62))
        .expect("the exact candidate recovers the activation");
    assert!(outcome.recovered_exactly());
    assert_eq!(
        repository
            .activated_catalog_revisions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
        vec![accepted.catalog_revision_id()]
    );
}

/// Returns one active fixture catalog revision with both fixture profiles.
fn active_catalog() -> ProviderCatalogRevisionDto {
    let full = catalog_text("", &format!("{}{}", main_profile(), local_profile()));
    prepare(&full, None)
        .expect("the two-profile candidate prepares")
        .revision_at(time(70))
        .expect("the revision is representable")
}

#[test]
fn selection_precedence_is_turn_then_session_then_global() {
    let catalog = active_catalog();
    let registry = ScriptedRegistry::new(true);

    let global = resolve_selection(SelectionResolutionInputDto::new(
        None,
        None,
        Some(profile("main")),
        Some(catalog.clone()),
        &registry,
    ))
    .expect("the global default resolves");
    let SelectionResolutionDto::Selected(selection) = &global else {
        unreachable!("the global default resolves to an exact selection");
    };
    assert_eq!(selection.profile_id(), &profile("main"));
    assert_eq!(
        selection.selection_source(),
        ProviderSelectionSourceDto::GlobalDefault
    );

    let session = resolve_selection(SelectionResolutionInputDto::new(
        None,
        Some(profile("local")),
        Some(profile("main")),
        Some(catalog.clone()),
        &registry,
    ))
    .expect("the session default resolves");
    let SelectionResolutionDto::Selected(selection) = &session else {
        unreachable!("the session default resolves to an exact selection");
    };
    assert_eq!(selection.profile_id(), &profile("local"));
    assert_eq!(
        selection.selection_source(),
        ProviderSelectionSourceDto::SessionDefault
    );

    let overridden = resolve_selection(SelectionResolutionInputDto::new(
        Some(ProviderProfileOverrideDto::new(profile("main"), None)),
        Some(profile("local")),
        Some(profile("main")),
        Some(catalog),
        &registry,
    ))
    .expect("the turn override resolves");
    let SelectionResolutionDto::Selected(selection) = &overridden else {
        unreachable!("the turn override resolves to an exact selection");
    };
    assert_eq!(selection.profile_id(), &profile("main"));
    assert_eq!(
        selection.selection_source(),
        ProviderSelectionSourceDto::TurnOverride
    );
    assert_eq!(overridden.selection(), Some(selection));
}

#[test]
fn selection_reports_each_unavailable_reason_without_a_fallback() {
    let catalog = active_catalog();
    let live = ScriptedRegistry::new(true);
    let dead = ScriptedRegistry::new(false);

    let missing = resolve_selection(SelectionResolutionInputDto::new(
        Some(ProviderProfileOverrideDto::new(profile("absent"), None)),
        Some(profile("main")),
        None,
        Some(catalog.clone()),
        &live,
    ))
    .expect("a missing profile is a closed outcome");
    assert_eq!(
        missing,
        SelectionResolutionDto::Unavailable {
            profile_id: profile("absent"),
            source: ProviderSelectionSourceDto::TurnOverride,
            reason: ProviderSelectionUnavailabilityDto::Missing,
        },
        "there is no fallback to a profile that still exists"
    );

    let unrunnable = resolve_selection(SelectionResolutionInputDto::new(
        None,
        None,
        Some(profile("main")),
        Some(catalog.clone()),
        &dead,
    ))
    .expect("a dead registry entry is a closed outcome");
    let SelectionResolutionDto::Unavailable { reason, .. } = &unrunnable else {
        unreachable!("a profile without a live entry is unavailable");
    };
    assert_eq!(
        *reason,
        ProviderSelectionUnavailabilityDto::RuntimeUnavailable
    );
    assert_eq!(
        unrunnable
            .into_accepted_selection()
            .expect_err("an unavailable selection never becomes accepted")
            .code(),
        "provider_profile_runtime_unavailable"
    );

    let unconfigured = resolve_selection(SelectionResolutionInputDto::new(
        None,
        None,
        None,
        Some(catalog),
        &live,
    ))
    .expect("no declared profile is a closed outcome");
    assert_eq!(unconfigured, SelectionResolutionDto::Unconfigured);
    assert_eq!(
        unconfigured
            .into_accepted_selection()
            .expect_err("an unconfigured turn never becomes accepted")
            .code(),
        "provider_configuration_unavailable"
    );
}

#[test]
fn selection_rejects_a_mismatched_expected_profile_revision() {
    let catalog = active_catalog();
    let revision = catalog
        .profile_revisions()
        .iter()
        .find(|revision| revision.profile_id() == &profile("main"))
        .expect("the fixture profile is a catalog member");
    let other = prepare(
        &catalog_text("", &main_profile().replace("gpt-5.6-terra", "gpt-5.6-luna")),
        None,
    )
    .expect("the changed candidate prepares")
    .profile_revisions()[0]
        .revision_id();

    let registry = ScriptedRegistry::new(true);
    let error = resolve_selection(SelectionResolutionInputDto::new(
        Some(ProviderProfileOverrideDto::new(
            profile("main"),
            Some(other),
        )),
        None,
        None,
        Some(catalog.clone()),
        &registry,
    ))
    .expect_err("a pinned stale revision is rejected");
    assert_eq!(error.code(), "provider_profile_revision_mismatch");

    let resolved = resolve_selection(SelectionResolutionInputDto::new(
        Some(ProviderProfileOverrideDto::new(
            profile("main"),
            Some(revision.revision_id()),
        )),
        None,
        None,
        Some(catalog),
        &registry,
    ))
    .expect("the pinned current revision resolves");
    assert!(matches!(resolved, SelectionResolutionDto::Selected(_)));
}

#[test]
fn session_default_change_publishes_exactly_one_committed_event() {
    let repository = FakeRepository::new(SessionId::new(), RunId::new(), fixture_snapshot());
    let sink = RecordingSessionSink::new();
    let command = SetSessionProviderProfileCommandDto::new(
        repository.session_id,
        profile("local"),
        4,
        IdempotencyKey::new(),
    );
    *repository
        .session_change
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(SessionProviderProfileChangeDto::new(
        repository.session_id,
        true,
        5,
    ));

    let accepted = set_session_provider_profile(&repository, &sink, &command, time(80))
        .expect("the committed change is accepted");
    assert_eq!(accepted.session_id(), repository.session_id);
    assert!(accepted.changed());
    assert_eq!(accepted.session_projection_revision(), 5);
    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id(), repository.session_id);
    assert_eq!(events[0].profile_id(), &profile("local"));
    assert_eq!(events[0].session_projection_revision(), 5);

    // Setting the already-durable profile is a successful no-change outcome
    // that publishes nothing.
    *repository
        .session_change
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(SessionProviderProfileChangeDto::new(
        repository.session_id,
        false,
        5,
    ));
    let unchanged = set_session_provider_profile(&repository, &sink, &command, time(81))
        .expect("the no-change outcome stays accepted");
    assert!(!unchanged.changed());
    assert_eq!(unchanged.session_projection_revision(), 5);
    assert_eq!(sink.events().len(), 1, "no-change publishes no event");
}

#[test]
fn session_default_change_propagates_a_stale_projection_conflict() {
    let repository = FakeRepository::new(SessionId::new(), RunId::new(), fixture_snapshot());
    let sink = RecordingSessionSink::new();
    *repository
        .session_change_error
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(ErrorDto::validation(
        "session_projection_revision_conflict",
        "the expected session projection revision is stale",
    ));
    let command = SetSessionProviderProfileCommandDto::new(
        repository.session_id,
        profile("local"),
        3,
        IdempotencyKey::new(),
    );

    let error = set_session_provider_profile(&repository, &sink, &command, time(90))
        .expect_err("a stale expectation is a typed conflict");
    assert_eq!(error.code(), "session_projection_revision_conflict");
    assert!(
        sink.events().is_empty(),
        "a refused change publishes nothing"
    );
}

#[test]
fn profile_usage_read_stays_keyed_by_the_exact_profile_and_reports_no_price() {
    let repository = FakeRepository::new(SessionId::new(), RunId::new(), fixture_snapshot());
    let catalog = active_catalog();
    let revision = catalog
        .profile_revisions()
        .iter()
        .find(|revision| revision.profile_id() == &profile("main"))
        .expect("the fixture profile is a catalog member");
    let aggregate = intention_storage::ProfileUsageAggregateDto::new(
        profile("main"),
        revision.revision_id(),
        "gpt-5.6-terra",
        2,
        1,
        30,
        12,
        42,
    )
    .expect("the fixture usage aggregate is valid");
    *repository
        .profile_usage
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = vec![aggregate.clone()];

    let usage = load_profile_usage(&repository, &profile("main")).expect("usage loads");
    assert_eq!(usage, vec![aggregate]);
    assert_eq!(usage[0].profile_id(), &profile("main"));
    assert_eq!(
        usage[0].provider_profile_revision_id(),
        revision.revision_id()
    );
    assert_eq!(usage[0].model_id(), "gpt-5.6-terra");
    assert_eq!(usage[0].reported_runs(), 2);
    assert_eq!(usage[0].unreported_runs(), 1);
    assert_eq!(usage[0].input_tokens(), 30);
    assert_eq!(usage[0].output_tokens(), 12);
    assert_eq!(usage[0].total_tokens(), 42);
}

#[test]
fn reasoning_history_manifest_keeps_typed_empty_records_and_transfers_only_text() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let repository = FakeRepository::new(session_id, RunId::new(), fixture_snapshot());
    let mut sources = vec![
        intention_storage::ReasoningHistorySourceStepDto::new(
            session_id,
            run_id,
            7,
            Some("why".to_owned()),
        )
        .expect("the fixture source step is valid"),
        intention_storage::ReasoningHistorySourceStepDto::new(session_id, run_id, 8, None)
            .expect("a textless step is valid"),
    ];
    sources.push(
        intention_storage::ReasoningHistorySourceStepDto::new(session_id, run_id, 9, None)
            .expect("a textless step is valid"),
    );
    *repository
        .reasoning_source
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = sources.clone();

    let resolution = resolve_reasoning_history(&repository, session_id, &transfer())
        .expect("the fixture history resolves")
        .expect("a textual transfer carries a resolution");
    let manifest = resolution.manifest();
    assert_eq!(
        manifest.compatibility_id(),
        Some("fixture-compatibility-v1")
    );
    assert_eq!(manifest.sources().len(), 3);
    assert_eq!(manifest.aggregate_size_bytes(), 3);
    assert_eq!(manifest.sources()[0].records().len(), 1);
    assert_eq!(manifest.sources()[0].records()[0].size_bytes(), 3);
    assert!(
        manifest.sources()[1].records().is_empty() && manifest.sources()[2].records().is_empty(),
        "a completed response without reasoning keeps a typed empty reference"
    );

    let typed = resolution.typed_transfer();
    assert_eq!(typed.compatibility_id(), "fixture-compatibility-v1");
    assert_eq!(typed.sources().len(), 3);
    let expected = fixture_reasoning_history(
        "fixture-compatibility-v1",
        &[(ReasoningFragmentCategoryDto::Primary, "why")],
        &[],
    )
    .expect("the fixture history is valid");
    assert_eq!(
        typed.sources()[0].history(),
        Some(&expected),
        "the verified transfer material is exactly the durable reasoning text"
    );
    assert!(typed.sources()[1].history().is_none());
    assert_eq!(typed.history_for_run(run_id), typed.sources()[0].history());

    let bound = history_bound(manifest).expect("the audit bound is representable");
    assert_eq!(bound.source_entry_count(), 3);
    assert_eq!(bound.aggregate_size_bytes(), 3);
}

#[test]
fn verification_requires_every_referenced_source_step() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let sources = vec![
        intention_storage::ReasoningHistorySourceStepDto::new(
            session_id,
            run_id,
            11,
            Some("why".to_owned()),
        )
        .expect("the fixture source step is valid"),
        intention_storage::ReasoningHistorySourceStepDto::new(
            session_id,
            run_id,
            12,
            Some("more".to_owned()),
        )
        .expect("the fixture source step is valid"),
    ];
    let manifest = build_history_manifest(
        &sources,
        &transfer(),
        Some("fixture-compatibility-v1".to_owned()),
    )
    .expect("the fixture manifest builds");
    assert_eq!(manifest.aggregate_size_bytes(), 7);
    verify_and_build_history(&manifest, &sources).expect("the exact reads verify");

    let error = verify_and_build_history(&manifest, &sources[..1])
        .expect_err("a missing referenced response blocks the transfer");
    assert_eq!(error.code(), "reasoning_history_unavailable");

    let truncated = vec![
        sources[0].clone(),
        intention_storage::ReasoningHistorySourceStepDto::new(session_id, run_id, 12, None)
            .expect("a textless step is valid"),
    ];
    let error = verify_and_build_history(&manifest, &truncated)
        .expect_err("a changed durable size blocks the transfer");
    assert_eq!(error.code(), "reasoning_history_unavailable");
}

#[test]
fn a_history_over_the_fixed_bound_fails_typed() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let oversized = "a".repeat(
        usize::try_from(intention_engine::reasoning::MAX_REASONING_HISTORY_AGGREGATE_BYTES)
            .expect("the bound fits a usize")
            + 1,
    );
    let sources = vec![
        intention_storage::ReasoningHistorySourceStepDto::new(
            session_id,
            run_id,
            21,
            Some(oversized),
        )
        .expect("the oversized history step is a valid source"),
    ];

    let error = build_history_manifest(&sources, &transfer(), Some("fixture".to_owned()))
        .expect_err("an over-bound history never commits a partial manifest");
    assert_eq!(error.code(), "reasoning_history_too_large");
}

#[test]
fn a_disabled_transfer_carries_no_history() {
    let repository = FakeRepository::new(SessionId::new(), RunId::new(), fixture_snapshot());
    let resolved = resolve_reasoning_history(
        &repository,
        repository.session_id,
        &ReasoningHistoryTransferDto::Disabled,
    )
    .expect("a disabled transfer is not a failure");
    assert!(resolved.is_none());
}

#[test]
fn the_run_reasoning_aggregate_fails_over_the_fixed_run_bound() {
    let mut aggregate = RunReasoningAggregate::new();
    aggregate
        .observe(&"a".repeat(MAX_RUN_REASONING_AGGREGATE_BYTES))
        .expect("a step at the bound stays admitted");
    assert_eq!(aggregate.bytes(), MAX_RUN_REASONING_AGGREGATE_BYTES);

    let error = aggregate
        .observe("b")
        .expect_err("one byte over the run bound fails the run");
    assert_eq!(error.code(), "reasoning_output_limit_exceeded");
}
