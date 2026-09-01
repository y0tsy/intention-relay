#![allow(
    clippy::expect_used,
    reason = "Zone 4 control-plane runtime fixtures use expect for precise diagnostics."
)]

//! Zone 4 control-plane runtime integration tests.
//!
//! Every service is exercised through fake ports with the same trait shapes
//! the composition root implements. The fake-secret sweep proves that private
//! credential material never appears in any service DTO `Debug` output or
//! error, and the non-authorizing tests prove health, discovery, and pricing
//! produce projections only: they never create a `RunId`, reason, lifecycle
//! event, scheduler candidate, or selection/routing decision.

use std::cell::RefCell;
use std::sync::Mutex;

use intention_application::{
    ConfigurationReloadService, CredentialRotationService, DiscoveryPort, DiscoveryScopeDto,
    DriverRebuildPort, HealthProbePort, PricingPolicyService, PrivateCredentialMaterial,
    PrivateCredentialPort, ProviderDiscoveryService, ProviderHealthService, ReloadCandidateDto,
    ReloadCommitOutcomeDto, SafeBindingSource, SafeCompositionBindingDto,
};
use intention_config::{
    ConfigPathDto, ConfigSnapshotDto, ConfigSourceDto, RawConfigInputDto, ResolvedConfigDto,
};
use intention_protocol::contract_families::{
    CredentialRotationResultDto, PricingObservationDto, PricingProjectionDto,
    ProviderAvailabilityObservation, ProviderDiscoveryProjectionDto, ProviderHealthProjectionDto,
    ProviderModelDiscoveryRecordDto, RotateProviderCredentialsCommandDto,
};
use intention_storage::{CommitConfigurationReloadInputDto, ConfigurationReloadRepositoryDto};
use intention_types::{ConfigRevisionId, DtoResult, ErrorDto, SchemaVersionDto, TimestampDto};

const FAKE_SECRET: &str = "sk-zone4-runtime-fake-secret";
const ENDPOINT: &str = "https://api.example.invalid/v1";

fn time() -> TimestampDto {
    TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid")
}

fn explicit_source() -> ConfigSourceDto {
    ConfigSourceDto::Explicit(
        ConfigPathDto::parse(
            std::env::temp_dir()
                .join("intention-application-zone4-runtime.toml")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("fixture path is absolute"),
    )
}

fn raw_config(kind: &str, model: &str) -> String {
    format!(
        "schema_version = 1\n[provider]\nkind = \"{kind}\"\nmodel = \"{model}\"\ncredential = \"fake-plain\"\nendpoint = \"{ENDPOINT}\"\n"
    )
}

fn snapshot(kind: &str, model: &str) -> ConfigSnapshotDto {
    let resolved = ResolvedConfigDto::parse_resolve(RawConfigInputDto::new(
        raw_config(kind, model),
        explicit_source(),
    ))
    .expect("fixture config resolves");
    ConfigSnapshotDto::new(
        SchemaVersionDto::new(1, 0),
        ConfigRevisionId::new(),
        time(),
        resolved,
    )
    .expect("fixture snapshot is valid")
}

fn binding_snapshot() -> SafeCompositionBindingDto {
    SafeCompositionBindingDto {
        profile_id: "default".to_owned(),
        provider_profile_revision_id: "profile-rev-1".to_owned(),
        safe_composition_revision: "composition-rev-1".to_owned(),
        kind_id: "openrouter".to_owned(),
        kind_descriptor_revision_id: "kind-rev-1".to_owned(),
        model_id: "model-a".to_owned(),
        endpoint: Some(ENDPOINT.to_owned()),
        declared_model_capability_subset: vec!["text_input".to_owned()],
        effective_execution_policy: "execution-timeout-30-attempts-2".to_owned(),
        effective_loopback_policy_or_not_applicable: "not-applicable".to_owned(),
        provider_driver_contract_revision: "driver-1.0".to_owned(),
    }
}

struct FakeReloadRepository {
    committed: RefCell<Vec<CommitConfigurationReloadInputDto>>,
    fail: bool,
}

impl FakeReloadRepository {
    const fn new() -> Self {
        Self {
            committed: RefCell::new(Vec::new()),
            fail: false,
        }
    }
}

impl ConfigurationReloadRepositoryDto for FakeReloadRepository {
    fn commit_configuration_reload(
        &self,
        input: CommitConfigurationReloadInputDto,
    ) -> DtoResult<()> {
        if self.fail {
            return Err(ErrorDto::unavailable(
                "reload_storage_unavailable",
                "fixture storage failure",
            ));
        }
        self.committed.borrow_mut().push(input);
        Ok(())
    }
}

struct FakeBindingSource {
    revision: String,
    snapshot: ConfigSnapshotDto,
    binding: SafeCompositionBindingDto,
}

impl FakeBindingSource {
    fn new(revision: &str, snapshot: ConfigSnapshotDto) -> Self {
        Self {
            revision: revision.to_owned(),
            snapshot,
            binding: binding_snapshot(),
        }
    }
}

impl SafeBindingSource for FakeBindingSource {
    fn active_revision(&self) -> DtoResult<String> {
        Ok(self.revision.clone())
    }

    fn active_snapshot(&self) -> DtoResult<ConfigSnapshotDto> {
        Ok(self.snapshot.clone())
    }

    fn binding(&self, profile_id: &str) -> DtoResult<SafeCompositionBindingDto> {
        if profile_id == self.binding.profile_id {
            Ok(self.binding.clone())
        } else {
            Err(ErrorDto::unavailable(
                "provider_profile_unavailable",
                "fixture profile is unavailable",
            ))
        }
    }
}

struct FakeCredentialPort {
    material: Option<Vec<u8>>,
    calls: Mutex<Vec<String>>,
}

impl FakeCredentialPort {
    const fn new(material: Option<Vec<u8>>) -> Self {
        Self {
            material,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .expect("fixture lock is not poisoned")
            .clone()
    }
}

impl PrivateCredentialPort for FakeCredentialPort {
    fn obtain_replacement(&self, profile_id: &str) -> DtoResult<PrivateCredentialMaterial> {
        self.calls
            .lock()
            .expect("fixture lock is not poisoned")
            .push(profile_id.to_owned());
        self.material
            .clone()
            .map(PrivateCredentialMaterial::from_private_bytes)
            .ok_or_else(|| {
                ErrorDto::unavailable(
                    "credential_rotation_source_unavailable",
                    "fixture credential source is unavailable",
                )
            })
    }
}

struct FakeRebuildPort {
    rebuilt: Mutex<Vec<String>>,
}

impl FakeRebuildPort {
    const fn new() -> Self {
        Self {
            rebuilt: Mutex::new(Vec::new()),
        }
    }

    fn rebuilt(&self) -> Vec<String> {
        self.rebuilt
            .lock()
            .expect("fixture lock is not poisoned")
            .clone()
    }
}

impl DriverRebuildPort for FakeRebuildPort {
    fn rebuild(&self, profile_id: &str, _material: PrivateCredentialMaterial) -> DtoResult<()> {
        self.rebuilt
            .lock()
            .expect("fixture lock is not poisoned")
            .push(profile_id.to_owned());
        Ok(())
    }
}

struct FakeHealthProbe {
    outcome: DtoResult<ProviderAvailabilityObservation>,
    calls: Mutex<Vec<String>>,
}

impl FakeHealthProbe {
    const fn new(outcome: DtoResult<ProviderAvailabilityObservation>) -> Self {
        Self {
            outcome,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .expect("fixture lock is not poisoned")
            .clone()
    }
}

impl HealthProbePort for FakeHealthProbe {
    fn probe(&self, provider_id: &str) -> DtoResult<ProviderAvailabilityObservation> {
        self.calls
            .lock()
            .expect("fixture lock is not poisoned")
            .push(provider_id.to_owned());
        self.outcome.clone()
    }
}

struct FakeDiscoveryPort {
    outcome: DtoResult<Vec<ProviderModelDiscoveryRecordDto>>,
    calls: Mutex<Vec<String>>,
}

impl FakeDiscoveryPort {
    const fn new(outcome: DtoResult<Vec<ProviderModelDiscoveryRecordDto>>) -> Self {
        Self {
            outcome,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .expect("fixture lock is not poisoned")
            .clone()
    }
}

impl DiscoveryPort for FakeDiscoveryPort {
    fn discover(
        &self,
        scope: &DiscoveryScopeDto,
    ) -> DtoResult<Vec<ProviderModelDiscoveryRecordDto>> {
        self.calls
            .lock()
            .expect("fixture lock is not poisoned")
            .push(scope.as_str());
        self.outcome.clone()
    }
}

fn discovery_record(model_id: &str) -> ProviderModelDiscoveryRecordDto {
    ProviderModelDiscoveryRecordDto {
        discovery_scope: "all".to_owned(),
        model_id: model_id.to_owned(),
        capability_records: vec!["text_input".to_owned()],
        source_attempt_id: "attempt-1".to_owned(),
        discovered_at: 1,
    }
}

#[test]
fn reload_prepare_commit_round_trip_persists_the_safe_snapshot() {
    let previous = snapshot("openrouter", "model-a");
    let repository = FakeReloadRepository::new();
    let binding = FakeBindingSource::new("revision-1", previous.clone());
    let service = ConfigurationReloadService::new(&repository, &binding);

    let candidate: ReloadCandidateDto = service
        .prepare(
            RawConfigInputDto::new(raw_config("openrouter", "model-b"), explicit_source()),
            &previous,
            "operation-1".to_owned(),
        )
        .expect("candidate prepares");
    assert!(candidate.accepted);
    assert!(candidate.changed_semantics);
    assert!(candidate.failure_code.is_none());

    let outcome: ReloadCommitOutcomeDto = service
        .commit(
            candidate.candidate().clone(),
            Some("revision-1".to_owned()),
            "operation-1".to_owned(),
            100,
        )
        .expect("commit succeeds");
    assert_eq!(outcome.transaction_id, "operation-1");
    assert_eq!(outcome.previous_revision, "revision-1");
    assert!(outcome.fresh_runs_only);
    let committed = repository.committed.borrow();
    assert_eq!(committed.len(), 1);
    assert_eq!(
        committed[0].snapshot.resolved().provider().model(),
        "model-b"
    );
    assert!(!format!("{outcome:?}").contains(FAKE_SECRET));
    assert!(!format!("{candidate:?}").contains(FAKE_SECRET));
}

#[test]
fn reload_stale_revision_and_storage_failure_leave_the_recorded_snapshot() {
    let previous = snapshot("openrouter", "model-a");
    let repository = FakeReloadRepository::new();
    let binding = FakeBindingSource::new("revision-1", previous.clone());
    let service = ConfigurationReloadService::new(&repository, &binding);
    let candidate = service
        .prepare(
            RawConfigInputDto::new(raw_config("openrouter", "model-b"), explicit_source()),
            &previous,
            "operation-1".to_owned(),
        )
        .expect("candidate prepares");

    let stale = service
        .commit(
            candidate.candidate().clone(),
            Some("revision-stale".to_owned()),
            "operation-1".to_owned(),
            100,
        )
        .expect_err("stale expected revision is rejected");
    assert_eq!(stale.code(), "config_revision_mismatch");
    assert!(repository.committed.borrow().is_empty());

    let failing = FakeReloadRepository {
        fail: true,
        ..FakeReloadRepository::new()
    };
    let binding = FakeBindingSource::new("revision-1", previous);
    let service = ConfigurationReloadService::new(&failing, &binding);
    let error = service
        .commit(
            candidate.candidate().clone(),
            Some("revision-1".to_owned()),
            "operation-1".to_owned(),
            100,
        )
        .expect_err("storage failure propagates");
    assert_eq!(error.code(), "reload_storage_unavailable");
    assert!(failing.committed.borrow().is_empty());
}

#[test]
fn rotation_rejects_mismatch_before_replacement_and_never_touches_the_snapshot() {
    let previous = snapshot("openrouter", "model-a");
    let binding = FakeBindingSource::new("revision-1", previous);
    let rebuild = FakeRebuildPort::new();
    let service = CredentialRotationService::new(&binding, &rebuild);
    let port = FakeCredentialPort::new(Some(FAKE_SECRET.as_bytes().to_vec()));

    let mismatch = RotateProviderCredentialsCommandDto {
        profile_id: "default".to_owned(),
        provider_profile_revision_id: "profile-rev-1".to_owned(),
        expected_credential_composition_revision: "composition-stale".to_owned(),
        operation_id: "operation-1".to_owned(),
    };
    let error = service
        .rotate(mismatch, &port, 100)
        .expect_err("stale composition is rejected");
    assert_eq!(error.code(), "credential_rotation_frozen_meaning_mismatch");
    assert!(rebuild.rebuilt().is_empty());
    assert!(port.calls().is_empty());
    assert!(!error.to_string().contains(FAKE_SECRET));
    // The safe snapshot and binding are untouched: the binding source still
    // reports the identical revision and composition.
    assert_eq!(
        binding.active_revision().expect("active revision reads"),
        "revision-1"
    );

    let rotated: CredentialRotationResultDto = service
        .rotate(
            RotateProviderCredentialsCommandDto {
                profile_id: "default".to_owned(),
                provider_profile_revision_id: "profile-rev-1".to_owned(),
                expected_credential_composition_revision: "composition-rev-1".to_owned(),
                operation_id: "operation-2".to_owned(),
            },
            &port,
            100,
        )
        .expect("rotation succeeds");
    assert!(rotated.rotated);
    assert_eq!(rotated.profile_id, "default");
    assert_eq!(rebuild.rebuilt(), ["default"]);
    assert!(!format!("{rotated:?}").contains(FAKE_SECRET));
    assert!(!format!("{:?}", port.calls()).contains(FAKE_SECRET));
}

#[test]
fn rotation_without_a_credential_source_fails_closed() {
    let previous = snapshot("openrouter", "model-a");
    let binding = FakeBindingSource::new("revision-1", previous);
    let rebuild = FakeRebuildPort::new();
    let service = CredentialRotationService::new(&binding, &rebuild);
    let port = FakeCredentialPort::new(None);
    let error = service
        .rotate(
            RotateProviderCredentialsCommandDto {
                profile_id: "default".to_owned(),
                provider_profile_revision_id: "profile-rev-1".to_owned(),
                expected_credential_composition_revision: "composition-rev-1".to_owned(),
                operation_id: "operation-1".to_owned(),
            },
            &port,
            100,
        )
        .expect_err("missing credential source fails closed");
    assert_eq!(error.code(), "credential_rotation_source_unavailable");
    assert!(rebuild.rebuilt().is_empty());
}

#[test]
fn health_check_projects_evidence_without_touching_any_run_or_selection_state() {
    let probe = FakeHealthProbe::new(Ok(ProviderAvailabilityObservation::Available));
    let projection: ProviderHealthProjectionDto = ProviderHealthService
        .check("default".to_owned(), &probe, 100)
        .expect("health check projects");
    assert_eq!(probe.calls(), ["default"]);
    assert_eq!(projection.provider_id, "default");
    assert_eq!(projection.observations.len(), 1);
    projection.validate().expect("projection is valid");
    // The projection is closed and non-authorizing: it carries no run,
    // reason, or selection identity, and the service invoked only the probe
    // port (never any storage, runtime, or catalog API).
    let debug = format!("{projection:?}");
    for forbidden in ["run_id", "selection", "mandate", "scheduler", "lifecycle"] {
        assert!(
            !debug.contains(forbidden),
            "health projection must not reference {forbidden}"
        );
    }
    assert!(!debug.contains(FAKE_SECRET));
}

#[test]
fn discovery_records_are_additive_and_never_route() {
    let port = FakeDiscoveryPort::new(Ok(vec![
        discovery_record("gpt-4o"),
        discovery_record("o3"),
        discovery_record("codex-1"),
    ]));
    let projection: ProviderDiscoveryProjectionDto = ProviderDiscoveryService
        .start(DiscoveryScopeDto::AllModels, &port, 100)
        .expect("discovery starts");
    assert_eq!(
        projection.phase,
        Some(intention_protocol::contract_families::ProviderDiscoveryPhase::Terminal)
    );
    assert_eq!(projection.records.len(), 3);
    let debug = format!("{projection:?}");
    // Model identities are data: the service returns the records only and
    // never changes any selection or routing state.
    assert!(debug.contains("gpt-4o"));
    assert!(!debug.contains("selection"));
    assert!(!debug.contains("run_id"));
    projection.validate().expect("projection is valid");

    let status = ProviderDiscoveryService
        .status(
            projection.attempt_id.expect("attempt id is present"),
            &port,
            200,
        )
        .expect("status projects");
    assert!(status.records.is_empty());
    assert_eq!(
        status.safe_status.as_deref(),
        Some("attempt_state_unavailable")
    );
    assert_eq!(
        port.calls().len(),
        1,
        "status must never re-run or continue an attempt"
    );
}

#[test]
fn pricing_projection_never_gates_admission_or_eligibility() {
    let service = PricingPolicyService;
    let projection: PricingProjectionDto = service.project(vec![
        PricingObservationDto {
            provider_kind_id: "openrouter".to_owned(),
            model_id: "model-a".to_owned(),
            bounded_numeric_value: 0,
            classification:
                intention_protocol::contract_families::PricingClassification::ProductPolicy,
            observed_at: 1,
        },
        PricingObservationDto {
            provider_kind_id: "openrouter".to_owned(),
            model_id: "model-b".to_owned(),
            bounded_numeric_value: 42,
            classification:
                intention_protocol::contract_families::PricingClassification::ProductPolicy,
            observed_at: 1,
        },
    ]);
    assert_eq!(projection.observations.len(), 2);
    assert_eq!(
        projection.observations[0].classification,
        intention_protocol::contract_families::PricingClassification::IntrinsicRepresentationBound
    );
    assert_eq!(
        projection.observations[1].classification,
        intention_protocol::contract_families::PricingClassification::CapacityObservation
    );
    assert!(projection.disclaimer.is_some());
    projection.validate().expect("projection is valid");
    let debug = format!("{projection:?}");
    for forbidden in [
        "run_id",
        "mandate",
        "selection",
        "ceiling",
        "eligibility",
        "scheduler",
    ] {
        assert!(
            !debug.contains(forbidden),
            "pricing projection must not carry {forbidden}"
        );
    }
}

#[test]
fn fake_secret_never_appears_in_any_service_dto_or_error() {
    let previous = snapshot("openrouter", "model-a");
    let repository = FakeReloadRepository::new();
    let binding = FakeBindingSource::new("revision-1", previous.clone());
    let reload = ConfigurationReloadService::new(&repository, &binding);
    let candidate = reload
        .prepare(
            RawConfigInputDto::new(raw_config("openrouter", "model-b"), explicit_source()),
            &previous,
            "operation-1".to_owned(),
        )
        .expect("candidate prepares");
    let outcome = reload
        .commit(
            candidate.candidate().clone(),
            Some("revision-1".to_owned()),
            "operation-1".to_owned(),
            100,
        )
        .expect("commit succeeds");

    let rebuild = FakeRebuildPort::new();
    let rotation = CredentialRotationService::new(&binding, &rebuild);
    let rotated = rotation
        .rotate(
            RotateProviderCredentialsCommandDto {
                profile_id: "default".to_owned(),
                provider_profile_revision_id: "profile-rev-1".to_owned(),
                expected_credential_composition_revision: "composition-rev-1".to_owned(),
                operation_id: "operation-1".to_owned(),
            },
            &FakeCredentialPort::new(Some(FAKE_SECRET.as_bytes().to_vec())),
            100,
        )
        .expect("rotation succeeds");

    let health = ProviderHealthService
        .check(
            "default".to_owned(),
            &FakeHealthProbe::new(Ok(ProviderAvailabilityObservation::Available)),
            100,
        )
        .expect("health projects");
    let discovery = ProviderDiscoveryService
        .start(
            DiscoveryScopeDto::AllModels,
            &FakeDiscoveryPort::new(Ok(vec![discovery_record("gpt-4o")])),
            100,
        )
        .expect("discovery starts");
    let pricing = PricingPolicyService.project(vec![PricingObservationDto {
        provider_kind_id: "openrouter".to_owned(),
        model_id: "model-a".to_owned(),
        bounded_numeric_value: 1,
        classification:
            intention_protocol::contract_families::PricingClassification::CapacityObservation,
        observed_at: 1,
    }]);

    for value in [
        format!("{candidate:?}"),
        format!("{outcome:?}"),
        format!("{rotated:?}"),
        format!("{health:?}"),
        format!("{discovery:?}"),
        format!("{pricing:?}"),
    ] {
        assert!(
            !value.contains(FAKE_SECRET),
            "service DTO Debug output must never expose the fake secret"
        );
    }
}
