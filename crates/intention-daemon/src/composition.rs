//! Durable Slice 2 composition root for the daemon host.
//!
//! Only this module selects SQLite, the private live provider registry, and the
//! configuration file boundary. Raw configuration, credential material,
//! database resources, locations, and every live driver stay private: the host
//! reaches the engine, the repository, and the resolved driver of one exact
//! persisted selection through the crate-private accessors below, and the wire
//! surface is served from this one composition.
//!
//! The catalog runtime owns one process-local state machine: the active safe
//! catalog with its private driver registry, at most one pending removal
//! candidate, the closed activation state, the closed degraded reason, and the
//! retained private credential slot. Every registry swap is serialized by the
//! runtime's own gate; every multi-step durable command is serialized by the
//! composition's command gate. Storage is always the last lock taken.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(any(test, feature = "test-support"))]
use intention_config::ConfigPathDto;
use intention_config::catalog::{
    CatalogCandidate, CatalogCredentialMaterial, CatalogDocumentDto, CatalogReloadImpactDto,
    classify_catalog_reload,
};
use intention_config::{
    ConfigPathResolver, ConfigSnapshotDto, ConfigSourceDto, ContextWindowPolicyDto,
    RawConfigInputDto,
};
use intention_engine::ApplicationService;
#[cfg(test)]
use intention_engine::{ModelRunCommitObserver, ToolInvocationRequestDto};
use intention_proto::DaemonHealthDto;
use intention_proto::provider::{
    AcceptProviderCatalogRemovalCommandDto, ApplyConfigurationDocumentCommandDto,
    ApplyConfigurationEditsCommandDto, CatalogRevisionId, CheckProviderHealthCommandDto,
    ConfigurationEditAcceptedDto, ConfigurationReloadAcceptedDto, CredentialRotationAcceptedDto,
    DiscoverProviderModelsCommandDto, GetSessionProviderProfileQueryDto,
    ListProviderCatalogQueryDto, ProviderCatalogActivationStateDto,
    ProviderCatalogCandidateHandleDto, ProviderCatalogCandidateRejectedDto,
    ProviderCatalogDegradedReasonDto, ProviderCatalogPageDto, ProviderCatalogRemovalAcceptedDto,
    ProviderCatalogStatusDto, ProviderDiscoveryAttemptId, ProviderDriverCapabilitiesDto,
    ProviderProfileEntryDto, ProviderProfileId, ProviderProfilePolicyDto,
    ProviderProfileReadinessDto, ProviderProfileRevisionId, ProviderProfileRevisionV1,
    ProviderSelectionUnavailabilityDto, ReasoningHistoryManifestDto,
    RejectProviderCatalogCandidateCommandDto, ReloadConfigurationCommandDto,
    ResolvedRunProviderSelectionDto, RotateProviderCredentialCommandDto,
    SessionProviderProfileChangedDto, SessionProviderProfileProjectionDto,
    SetSessionProviderProfileAcceptedDto, SetSessionProviderProfileCommandDto,
};
use intention_proto::{
    ConfigRevisionId, CreateSessionAcceptedDto, CreateSessionCommandDto, DtoResult, ErrorDto,
    InterruptRunAcceptedDto, InterruptRunCommandDto, ProtocolResultDto, RemoveTurnAcceptedDto,
    RemoveTurnCommandDto, RunId, RunProjectionDto, SchemaVersionDto, SendUserTurnAcceptedDto,
    SendUserTurnCommandDto, SendUserTurnOutcomeDto, SessionId, SessionProjectionDto,
    SessionSnapshotDto, TimestampDto,
};
#[cfg(test)]
use intention_proto::{ProjectId, RunModeDto, WorkspaceId, WorkspaceRootDto};
use intention_providers::{
    AuthenticationHeaderPolicyV1, GENERIC_CHAT_KIND_ID, GenericChatDriver,
    GenericChatDriverOptions, ModelExecutionDriver, OPENROUTER_KIND_ID, OpenRouterDriver,
    OpenRouterDriverOptions, driver_capabilities, driver_contract,
};
#[cfg(test)]
use intention_storage::ToolResultEvidenceDto;
use intention_storage::{
    AcceptedTurnOutcomeDto, ProviderCatalogRevisionDto, ProviderCatalogStateDto,
    ProviderDiscoveryFailureDto, SqliteDatabaseLocationDto, SqliteStorageRepository,
    StorageRepositoryDto,
};
#[cfg(test)]
use intention_tools::{ToolInput, WorkspaceRoot};
use intention_transport::MAX_TRANSCRIPT_SNAPSHOT_BYTES;

/// The single live configuration snapshot schema (intention-config current schema).
const CONFIG_SCHEMA_VERSION: SchemaVersionDto = SchemaVersionDto::new(1, 0);
const DATABASE_FILENAME: &str = "intention-relay.sqlite";

/// The retained suffix of the atomic configuration-file write.
const CONFIGURATION_WRITE_SUFFIX: &str = ".intention-relay-write";

/// The retained bounded size of a current-state snapshot's recent transcript.
///
/// The row bound is additionally held to `MAX_TRANSCRIPT_SNAPSHOT_BYTES`, the
/// representation budget derived from the single transport envelope cap.
pub const SESSION_SNAPSHOT_MESSAGES: u32 = 256;

/// The one daemon composition root.
///
/// It owns durable storage, the catalog runtime, the current configuration
/// revision, and command-admission serialization. The daemon host reaches the
/// engine, the repository, and the resolved driver of one exact persisted
/// selection through the crate-private accessors instead of a `*_for_daemon*`
/// bridge surface.
#[derive(Clone)]
pub struct DaemonApplicationFacade {
    inner: Arc<FacadeInner>,
}

struct FacadeInner {
    repository: SqliteStorageRepository,
    catalog: CatalogRuntime,
    configuration: Mutex<Option<CurrentConfiguration>>,
    sessions: SessionEvents,
    command_gate: CommandGate,
}

/// The one durable-command admission gate of this composition root.
///
/// A durable command is a multi-step sequence: it reads current durable state
/// and then commits a transition, so two commands that interleave their steps
/// could both decide from the same pre-state and break the single-writer rule
/// the durable run registry depends on. The gate serializes exactly those
/// sequences against each other. A pure read — the session snapshot, the run
/// snapshot a subscription answers with, and the interrupt validation — never
/// takes it. `run` is the gate's only entry point, so a command path cannot run
/// ungated by construction.
///
/// Ordering: the gate is an independent lock, and it is never held together
/// with the daemon host's registry lock. Every gated closure reaches durable
/// storage, which takes the repository's one connection lock, so the only
/// ordering relations are gate → storage, catalog → storage, and registry →
/// storage: storage is always the last lock taken, and no lock is taken while
/// the connection lock is held.
pub struct CommandGate {
    inner: Mutex<()>,
}

impl CommandGate {
    const fn new() -> Self {
        Self {
            inner: Mutex::new(()),
        }
    }

    /// Runs one durable multi-step command sequence as the single writer.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when an earlier command panicked
    /// while holding the gate.
    pub fn run<T>(&self, command: impl FnOnce() -> DtoResult<T>) -> DtoResult<T> {
        let _gate = self.inner.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_command_unavailable",
                "daemon command is unavailable",
            )
        })?;
        command()
    }
}

/// The configuration revision the composition applies to fresh runs.
///
/// The configuration revision carries the immutable global context-window
/// policy a run executes with; the exact provider identity of a run is its own
/// persisted resolved selection. One committed snapshot per exact selected
/// profile revision keeps a repeated acceptance of the same turn replaying the
/// identical configuration revision, which is what the durable turn replay
/// compares.
struct CurrentConfiguration {
    context_window: ContextWindowPolicyDto,
    /// The base revision committed at open and on every configuration change.
    revision_id: ConfigRevisionId,
    /// One committed turn revision per exact selected profile revision.
    turn_revisions: HashMap<ProviderProfileRevisionId, ConfigSnapshotDto>,
}

impl CurrentConfiguration {
    fn new(context_window: ContextWindowPolicyDto, revision_id: ConfigRevisionId) -> Self {
        Self {
            context_window,
            revision_id,
            turn_revisions: HashMap::new(),
        }
    }
}

/// The composition's session-event port.
///
/// The port is the engine's frozen session-event boundary
/// ([`intention_engine::selection::SessionEventSink`]): a committed session
/// default change is published here as one closed typed value, after its durable
/// commit and never as part of it. A production composition keeps no durable
/// copy — durable delivery belongs to a later milestone — while a test-support
/// build records every publication for observation.
struct SessionEvents {
    #[cfg(any(test, feature = "test-support"))]
    recorded: Arc<Mutex<Vec<SessionProviderProfileChangedDto>>>,
}

impl SessionEvents {
    #[cfg(not(any(test, feature = "test-support")))]
    const fn production() -> Self {
        Self {}
    }

    #[cfg(any(test, feature = "test-support"))]
    fn observable() -> Self {
        Self {
            recorded: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    fn recorded(&self) -> Vec<SessionProviderProfileChangedDto> {
        self.recorded
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

impl intention_engine::selection::SessionEventSink for SessionEvents {
    fn observe_session_provider_profile_change(&self, event: &SessionProviderProfileChangedDto) {
        #[cfg(any(test, feature = "test-support"))]
        {
            if let Ok(mut events) = self.recorded.lock() {
                events.push(event.clone());
            }
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            let _ = event;
        }
    }
}

/// The exact private registry identity of one live driver.
///
/// A live entry is keyed by the exact profile revision, the exact kind
/// descriptor revision, and the code-owned driver contract's family and major
/// and minor components, so two profiles that share every textual field stay
/// independent entries and a changed revision never resolves an older client.
type RegistryKey = (
    ProviderProfileRevisionId,
    intention_proto::provider::ProviderKindDescriptorRevisionId,
    String,
    u16,
    u16,
);

/// One live private driver entry.
type LiveDriver = Arc<dyn ModelExecutionDriver + Send + Sync>;

/// The composition's registry-liveness answer for one selection resolution.
///
/// The engine asks only whether an exact selection has a live private entry, so
/// the probe borrows the runtime registry for the duration of one resolution
/// instead of handing any driver or handle across the engine boundary.
struct RegistryProbe<'a> {
    registry: &'a HashMap<RegistryKey, LiveDriver>,
}

impl intention_engine::selection::ProviderDriverRegistry for RegistryProbe<'_> {
    fn has_live_driver(&self, selection: &ResolvedRunProviderSelectionDto) -> bool {
        self.registry
            .contains_key(&registry_key_of_selection(selection))
    }
}

/// The active credential-free catalog plus the current safe document.
#[derive(Clone)]
struct ActiveCatalog {
    revision: ProviderCatalogRevisionDto,
    document: CatalogDocumentDto,
}

impl ActiveCatalog {
    const fn default_profile_id(&self) -> Option<&ProviderProfileId> {
        self.revision.default_profile_id()
    }

    fn profile(&self, profile_id: &ProviderProfileId) -> Option<&ProviderProfileRevisionV1> {
        self.revision
            .profile_revisions()
            .iter()
            .find(|profile| profile.profile_id() == profile_id)
    }

    fn policy(&self, profile_id: &ProviderProfileId) -> Option<&ProviderProfilePolicyDto> {
        self.revision
            .profile_policies()
            .iter()
            .find(|entry| entry.profile_id() == profile_id)
            .map(intention_storage::ProviderProfilePolicyEntryDto::policy)
    }

    /// Returns the declared profiles in stable profile-id order.
    fn ordered_profiles(&self) -> Vec<&ProviderProfileRevisionV1> {
        let mut profiles: Vec<&ProviderProfileRevisionV1> =
            self.revision.profile_revisions().iter().collect();
        profiles.sort_by(|left, right| left.profile_id().as_str().cmp(right.profile_id().as_str()));
        profiles
    }
}

/// One process-local pending removal candidate.
struct PendingCandidate {
    handle: ProviderCatalogCandidateHandleDto,
    preparation: engine_api::PreparedCatalog,
    registry: HashMap<RegistryKey, LiveDriver>,
}

/// One opaque private credential value.
///
/// This type intentionally implements no `Debug`, `Display`, `Clone`, or serde
/// traits and exposes no value accessor: a credential crosses only the driver
/// construction boundary as a borrowed value.
struct PrivateCredential(String);

/// Whether the composition has a file-backed private credential source.
enum CredentialSource {
    /// The real configuration file captured through the private boundary.
    FileBacked(ConfigSourceDto),
    /// A test-support host whose document was supplied in memory.
    #[cfg(any(test, feature = "test-support"))]
    Absent,
}

impl CredentialSource {
    /// Returns the file-backed source, when the composition has one.
    const fn file(&self) -> Option<&ConfigSourceDto> {
        match self {
            Self::FileBacked(source) => Some(source),
            #[cfg(any(test, feature = "test-support"))]
            Self::Absent => None,
        }
    }
}

/// The private live provider registry and catalog state machine.
///
/// One gate serializes every read-modify-swap of the state: durable acceptance
/// is committed before the swap, so a swap never performs durable work while
/// the state gate is held, and a concurrent reader always observes one coherent
/// active catalog with its registry.
struct CatalogRuntime {
    state: Mutex<CatalogRuntimeState>,
    /// The one pending removal candidate, if any.
    ///
    /// It is a separate gate from the running state so an accept or reject can
    /// move the pre-validated candidate through its durable steps without
    /// holding the state gate across storage work.
    pending: Mutex<Option<PendingCandidate>>,
    source: CredentialSource,
    /// The one driver a test or test-support host injects for every profile.
    injected: Option<LiveDriver>,
}

struct CatalogRuntimeState {
    active: Option<ActiveCatalog>,
    registry: HashMap<RegistryKey, LiveDriver>,
    activation: ProviderCatalogActivationStateDto,
    degraded: Option<ProviderCatalogDegradedReasonDto>,
    /// The retained private credential material captured at the last parse.
    credentials: Option<CatalogCredentialMaterial>,
    /// Per-profile replacement credentials of one controlled rotation.
    rotated: HashMap<ProviderProfileId, PrivateCredential>,
}

impl CatalogRuntime {
    fn new(
        source: CredentialSource,
        credentials: CatalogCredentialMaterial,
        injected: Option<LiveDriver>,
    ) -> Self {
        Self {
            state: Mutex::new(CatalogRuntimeState {
                active: None,
                registry: HashMap::new(),
                activation: ProviderCatalogActivationStateDto::Preparing,
                degraded: None,
                credentials: Some(credentials),
                rotated: HashMap::new(),
            }),
            pending: Mutex::new(None),
            source,
            injected,
        }
    }

    /// Locks the running catalog state.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when the state gate is poisoned.
    fn lock(&self) -> DtoResult<std::sync::MutexGuard<'_, CatalogRuntimeState>> {
        self.state.lock().map_err(|_| {
            ErrorDto::unavailable(
                "provider_catalog_runtime_unavailable",
                "the provider catalog runtime is unavailable",
            )
        })
    }

    /// Locks the pending-candidate slot.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when the pending gate is poisoned.
    fn lock_pending(&self) -> DtoResult<std::sync::MutexGuard<'_, Option<PendingCandidate>>> {
        self.pending.lock().map_err(|_| {
            ErrorDto::unavailable(
                "provider_catalog_runtime_unavailable",
                "the provider catalog runtime is unavailable",
            )
        })
    }

    /// Returns the closed activation state and degraded reason.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when the state gate is poisoned.
    fn status(
        &self,
    ) -> DtoResult<(
        ProviderCatalogActivationStateDto,
        Option<ProviderCatalogDegradedReasonDto>,
    )> {
        let status = {
            let state = self.lock()?;
            (state.activation, state.degraded)
        };
        Ok(status)
    }

    /// Rejects provider state changes, admission, and default changes while the
    /// catalog is degraded read-only.
    ///
    /// # Errors
    ///
    /// Returns the closed `execution_not_ready` failure.
    fn ensure_ready(&self) -> DtoResult<()> {
        let state = self.lock()?;
        let degraded = state.degraded.is_some();
        drop(state);
        if degraded {
            return Err(execution_not_ready());
        }
        Ok(())
    }

    /// Returns a copy of the active catalog.
    ///
    /// # Errors
    ///
    /// Returns `execution_not_ready` when no catalog is active yet.
    fn active(&self) -> DtoResult<ActiveCatalog> {
        let state = self.lock()?;
        state.active.clone().ok_or_else(execution_not_ready)
    }

    /// Returns the active credential-free document.
    ///
    /// # Errors
    ///
    /// Returns `execution_not_ready` when no catalog is active yet.
    fn active_document(&self) -> DtoResult<CatalogDocumentDto> {
        let state = self.lock()?;
        state
            .active
            .as_ref()
            .map(|active| active.document.clone())
            .ok_or_else(execution_not_ready)
    }

    /// Returns the active catalog revision identity, when one is active.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when the state gate is poisoned.
    fn active_revision_id(&self) -> DtoResult<Option<CatalogRevisionId>> {
        let state = self.lock()?;
        Ok(state
            .active
            .as_ref()
            .map(|active| active.revision.catalog_revision_id()))
    }

    /// Replaces the current safe document of the active catalog.
    ///
    /// # Errors
    ///
    /// Returns `execution_not_ready` when no catalog is active yet.
    fn refresh_document(&self, document: CatalogDocumentDto) -> DtoResult<()> {
        {
            let mut state = self.lock()?;
            let active = state.active.as_mut().ok_or_else(execution_not_ready)?;
            active.document = document;
            drop(state);
        }
        Ok(())
    }

    /// Returns the exact profile revision of one active profile.
    ///
    /// # Errors
    ///
    /// Returns `provider_profile_runtime_unavailable` when the profile is not a
    /// member of the active catalog.
    fn profile_revision(
        &self,
        profile_id: &ProviderProfileId,
    ) -> DtoResult<ProviderProfileRevisionV1> {
        let state = self.lock()?;
        state
            .active
            .as_ref()
            .and_then(|active| active.profile(profile_id))
            .cloned()
            .ok_or_else(provider_profile_runtime_unavailable)
    }

    /// Returns whether the exact registry identity has a live driver.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when the state gate is poisoned.
    fn registry_has(&self, key: &RegistryKey) -> DtoResult<bool> {
        let state = self.lock()?;
        Ok(state.registry.contains_key(key))
    }

    /// Returns the handle of the pending removal candidate, when one exists.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when the pending gate is poisoned.
    fn pending_handle(&self) -> DtoResult<Option<ProviderCatalogCandidateHandleDto>> {
        let pending = self.lock_pending()?;
        Ok(pending.as_ref().map(|candidate| candidate.handle))
    }

    /// Returns the live driver of one active profile's exact revision.
    ///
    /// # Errors
    ///
    /// Returns `provider_profile_runtime_unavailable` when the profile has no
    /// live registry entry, or the typed unavailable failure when the state gate
    /// is poisoned.
    fn driver_for_profile(&self, profile_id: &ProviderProfileId) -> DtoResult<LiveDriver> {
        let revision = self.profile_revision(profile_id)?;
        let key = registry_key_of_revision(&revision)?;
        let state = self.lock()?;
        state
            .registry
            .get(&key)
            .map(Arc::clone)
            .ok_or_else(provider_profile_runtime_unavailable)
    }

    /// Returns the live driver of one exact persisted selection.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when the state gate is poisoned.
    fn driver_for_selection(
        &self,
        selection: &ResolvedRunProviderSelectionDto,
    ) -> DtoResult<Option<LiveDriver>> {
        let key = registry_key_of_selection(selection);
        let state = self.lock()?;
        Ok(state.registry.get(&key).map(Arc::clone))
    }

    /// Resolves one turn's provider selection against the live registry.
    ///
    /// The active revision, its default profile, and the registry are read under
    /// one state gate, so a resolution can never observe a half-swapped catalog.
    ///
    /// # Errors
    ///
    /// Returns the engine's typed resolution failure, the repository's typed
    /// session read failure, or `execution_not_ready` when no catalog is active.
    fn resolve_turn_selection(
        &self,
        repository: &SqliteStorageRepository,
        command: &SendUserTurnCommandDto,
    ) -> DtoResult<ResolvedRunProviderSelectionDto> {
        let session_default = repository
            .load_session_provider_profile(command.session_id())?
            .provider_profile_id()
            .cloned();
        let state = self.lock()?;
        let selection = {
            let active = state.active.as_ref().ok_or_else(execution_not_ready)?;
            let probe = RegistryProbe {
                registry: &state.registry,
            };
            engine_api::resolve_turn_selection(command, session_default, active, &probe)?
        };
        drop(state);
        Ok(selection)
    }

    /// Returns one credential-free catalog entry per active profile.
    ///
    /// # Errors
    ///
    /// Returns a typed error when an entry cannot be built.
    fn entries(&self) -> DtoResult<Vec<ProviderProfileEntryDto>> {
        let entries = {
            let state = self.lock()?;
            let Some(active) = &state.active else {
                return Err(execution_not_ready());
            };
            let mut entries = Vec::with_capacity(active.revision.profile_revisions().len());
            for profile in active.ordered_profiles() {
                let policy = match active.policy(profile.profile_id()) {
                    Some(policy) => policy.clone(),
                    None => default_profile_policy(profile)?,
                };
                let key = registry_key_of_revision(profile)?;
                let configured = credential_present(&state, profile.profile_id());
                entries.push(profile_entry(
                    profile,
                    &policy,
                    configured,
                    state.registry.contains_key(&key),
                )?);
            }
            entries
        };
        Ok(entries)
    }

    /// Renders the credential-bearing configuration source of one edited document.
    ///
    /// The edited document is validated inside the private credential boundary
    /// with the retained material restored, and the rendered text never leaves
    /// the composition.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error when the document cannot be bound to the
    /// retained material, or `credential_rotation_source_unavailable` when no
    /// private material is retained.
    fn render_source(&self, document: &CatalogDocumentDto) -> DtoResult<String> {
        let state = self.lock()?;
        let rendered = {
            let credentials = state
                .credentials
                .as_ref()
                .ok_or_else(credential_source_unavailable)?;
            let candidate =
                CatalogCandidate::bind_retained_material(document.clone(), credentials)?;
            render_credential_document(&candidate, credentials)?
        };
        drop(state);
        Ok(rendered)
    }

    /// Replaces one profile's private credential and rebuilds its live client.
    ///
    /// # Errors
    ///
    /// Returns the adapter's typed construction failure when the rebuilt client
    /// cannot be created.
    fn rotate_credential(
        &self,
        profile_id: ProviderProfileId,
        credential: PrivateCredential,
        revision: &ProviderProfileRevisionV1,
    ) -> DtoResult<()> {
        let key = registry_key_of_revision(revision)?;
        let driver = self.build_single_driver(revision, &credential)?;
        {
            let mut state = self.lock()?;
            state.rotated.insert(profile_id, credential);
            state.registry.insert(key, driver);
        }
        Ok(())
    }

    /// Builds one driver for one exact revision and credential.
    ///
    /// # Errors
    ///
    /// Returns the adapter's typed construction failure, or
    /// `provider_profile_runtime_unavailable` when the kind has no code-owned
    /// driver.
    fn build_single_driver(
        &self,
        revision: &ProviderProfileRevisionV1,
        credential: &PrivateCredential,
    ) -> DtoResult<LiveDriver> {
        if let Some(injected) = &self.injected {
            return Ok(Arc::clone(injected));
        }
        build_driver(revision, &credential.0)
    }

    /// Builds the replacement registry of one catalog revision.
    ///
    /// A strict build is the all-or-nothing pre-validation of an accepted
    /// catalog: every profile with a code-owned driver must construct its live
    /// client, or the activation fails closed. A lenient build serves only a
    /// degraded read-only state, where a profile without retained material or
    /// without a code-owned driver simply has no live entry.
    ///
    /// # Errors
    ///
    /// Returns the adapter's typed construction failure in a strict build.
    fn build_registry(
        &self,
        revision: &ProviderCatalogRevisionDto,
        strict: bool,
    ) -> DtoResult<HashMap<RegistryKey, LiveDriver>> {
        let mut registry = HashMap::with_capacity(revision.profile_revisions().len());
        for profile in revision.profile_revisions() {
            if driver_contract(profile.kind_id()).is_none() {
                continue;
            }
            let key = registry_key_of_revision(profile)?;
            if let Some(injected) = &self.injected {
                registry.insert(key, Arc::clone(injected));
                continue;
            }
            let credential = {
                let state = self.lock()?;
                credential_value(&state, profile)
            };
            match credential {
                Some(credential) => match build_driver(profile, &credential) {
                    Ok(driver) => {
                        registry.insert(key, driver);
                    }
                    Err(error) if strict => return Err(error),
                    Err(_) => {}
                },
                None if strict => return Err(credential_source_unavailable()),
                None => {}
            }
        }
        Ok(registry)
    }

    /// Installs one active catalog together with its pre-built registry.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when the state gate is poisoned.
    fn install(
        &self,
        active: ActiveCatalog,
        registry: HashMap<RegistryKey, LiveDriver>,
    ) -> DtoResult<()> {
        {
            let mut state = self.lock()?;
            state.active = Some(active);
            state.registry = registry;
            state.activation = ProviderCatalogActivationStateDto::Active;
            state.degraded = None;
        }
        Ok(())
    }

    /// Installs one degraded read-only state with its lenient registry.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when the state gate is poisoned.
    fn install_degraded(
        &self,
        active: ActiveCatalog,
        registry: HashMap<RegistryKey, LiveDriver>,
        activation: ProviderCatalogActivationStateDto,
        reason: ProviderCatalogDegradedReasonDto,
    ) -> DtoResult<()> {
        {
            let mut state = self.lock()?;
            state.active = Some(active);
            state.registry = registry;
            state.activation = activation;
            state.degraded = Some(reason);
        }
        Ok(())
    }

    /// Installs one pre-validated pending removal candidate.
    ///
    /// # Errors
    ///
    /// Returns a typed conflict when a candidate is already pending, or the
    /// typed unavailable failure when a gate is poisoned.
    fn install_pending(
        &self,
        active: ActiveCatalog,
        registry: HashMap<RegistryKey, LiveDriver>,
        candidate: PendingCandidate,
    ) -> DtoResult<()> {
        {
            let mut pending = self.lock_pending()?;
            if pending.is_some() {
                return Err(provider_catalog_changed());
            }
            *pending = Some(candidate);
        }
        self.install_degraded(
            active,
            registry,
            ProviderCatalogActivationStateDto::PendingRemoval,
            ProviderCatalogDegradedReasonDto::RemovalCandidatePending,
        )
    }

    /// Moves the pending candidate out after verifying its exact handle.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_changed` when no candidate, or a different
    /// candidate, is pending.
    fn take_pending(
        &self,
        handle: ProviderCatalogCandidateHandleDto,
    ) -> DtoResult<PendingCandidate> {
        let mut pending = self.lock_pending()?;
        let candidate = pending.take().ok_or_else(provider_catalog_changed)?;
        if candidate.handle != handle {
            *pending = Some(candidate);
            drop(pending);
            return Err(provider_catalog_changed());
        }
        drop(pending);
        Ok(candidate)
    }

    /// Restores one pending candidate after a durable acceptance failure.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when the pending gate is poisoned.
    fn restore_pending(&self, candidate: PendingCandidate) -> DtoResult<()> {
        {
            let mut pending = self.lock_pending()?;
            if pending.is_none() {
                *pending = Some(candidate);
            }
        }
        Ok(())
    }

    /// Promotes the pre-validated pending candidate into the active catalog.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when the state gate is poisoned.
    fn promote_pending(&self, candidate: PendingCandidate) -> DtoResult<()> {
        let document = self.active_document()?;
        let revision = candidate.preparation.revision;
        self.install(ActiveCatalog { revision, document }, candidate.registry)
    }

    /// Leaves the active catalog degraded read-only after a rejection.
    ///
    /// # Errors
    ///
    /// Returns `execution_not_ready` when no catalog is active yet.
    fn mark_candidate_rejected(&self) -> DtoResult<()> {
        {
            let mut state = self.lock()?;
            if state.active.is_none() {
                return Err(execution_not_ready());
            }
            state.activation = ProviderCatalogActivationStateDto::Active;
            state.degraded = Some(ProviderCatalogDegradedReasonDto::RemovalCandidateRejected);
        }
        Ok(())
    }
}

/// Returns whether one profile has retained private credential material.
fn credential_present(state: &CatalogRuntimeState, profile_id: &ProviderProfileId) -> bool {
    state.rotated.contains_key(profile_id)
        || state.credentials.as_ref().is_some_and(|credentials| {
            credentials
                .with_profile_credential(profile_id, |_| ())
                .is_ok()
        })
}

/// Returns one profile's private credential value, when one is retained.
fn credential_value(
    state: &CatalogRuntimeState,
    profile: &ProviderProfileRevisionV1,
) -> Option<String> {
    if let Some(rotated) = state.rotated.get(profile.profile_id()) {
        return Some(rotated.0.clone());
    }
    state.credentials.as_ref().and_then(|credentials| {
        credentials
            .with_profile_credential(profile.profile_id(), str::to_owned)
            .ok()
    })
}

/// The one engine boundary of the composition.
///
/// Every catalog and selection service the composition consumes lives here as a
/// thin wrapper over the frozen Slice 2 engine surface
/// (`intention_engine::catalog::*` and `intention_engine::selection::*`), so a
/// signature reconciliation at integration touches only this module.
mod engine_api {
    use super::{
        ActiveCatalog, ApplicationService, CatalogDocumentDto, CatalogRevisionId,
        ConfigSnapshotDto, DtoResult, ProviderCatalogCandidateHandleDto,
        ProviderCatalogCandidateRejectedDto, ProviderCatalogRevisionDto, ProviderCatalogStateDto,
        ProviderProfileId, ReasoningHistoryManifestDto, ResolvedRunProviderSelectionDto,
        SendUserTurnCommandDto, SessionId, SetSessionProviderProfileAcceptedDto,
        SetSessionProviderProfileCommandDto, SqliteStorageRepository, TimestampDto,
    };
    use intention_engine::catalog::{
        CatalogCandidateInputDto, CatalogChangeKindDto, CatalogPreparationDto,
    };
    use intention_engine::selection::{
        ProviderDriverRegistry, SelectionResolutionInputDto, SessionEventSink, resolve_selection,
    };

    /// One engine-prepared catalog candidate with its durable revision form.
    pub(super) struct PreparedCatalog {
        /// The credential-free revision the candidate prepares as.
        pub(super) revision: ProviderCatalogRevisionDto,
        /// The dedicated engine preparation, kept for accept and recovery.
        pub(super) preparation: CatalogPreparationDto,
        /// How the candidate compares to the accepted revision.
        pub(super) change: CatalogChangeKindDto,
    }

    impl PreparedCatalog {
        /// Returns whether the candidate omits an accepted profile or kind.
        pub(super) const fn has_removals(&self) -> bool {
            self.preparation.is_removal_candidate()
        }

        /// Returns the pending-candidate handle of one removal candidate.
        pub(super) fn candidate_handle(&self) -> Option<ProviderCatalogCandidateHandleDto> {
            self.preparation.candidate_handle()
        }
    }

    /// Prepares one credential-free catalog document against the accepted revision.
    ///
    /// # Errors
    ///
    /// Returns the engine's typed catalog validation failure.
    pub(super) fn prepare_catalog_candidate(
        document: &CatalogDocumentDto,
        active: Option<&ProviderCatalogRevisionDto>,
        occurred_at: TimestampDto,
    ) -> DtoResult<PreparedCatalog> {
        let input = CatalogCandidateInputDto::new(document.clone(), active.cloned());
        let preparation = intention_engine::catalog::prepare_catalog_candidate(&input)?;
        Ok(PreparedCatalog {
            revision: preparation.revision_at(occurred_at)?,
            change: preparation.change(),
            preparation,
        })
    }

    /// Accepts one prepared catalog revision in one durable transaction.
    ///
    /// # Errors
    ///
    /// Returns the engine's typed validation, conflict, or storage failure.
    pub(super) fn accept_catalog_revision(
        repository: &SqliteStorageRepository,
        prepared: &PreparedCatalog,
        occurred_at: TimestampDto,
    ) -> DtoResult<CatalogRevisionId> {
        intention_engine::catalog::accept_catalog_revision(
            repository,
            &prepared.preparation,
            occurred_at,
        )
    }

    /// Marks the exact accepted catalog revision active.
    ///
    /// # Errors
    ///
    /// Returns the engine's typed conflict or storage failure.
    pub(super) fn mark_catalog_activated(
        repository: &SqliteStorageRepository,
        revision_id: CatalogRevisionId,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProviderCatalogStateDto> {
        intention_engine::catalog::mark_catalog_activated(repository, revision_id, occurred_at)
    }

    /// Activates the exact accepted catalog after a crash, or refuses a changed file.
    ///
    /// # Errors
    ///
    /// Returns the engine's typed conflict or storage failure.
    pub(super) fn recover_catalog_activation(
        repository: &SqliteStorageRepository,
        accepted: &ProviderCatalogRevisionDto,
        prepared: &PreparedCatalog,
        occurred_at: TimestampDto,
    ) -> DtoResult<bool> {
        let outcome = intention_engine::catalog::recover_catalog_activation(
            repository,
            accepted,
            &prepared.preparation,
            occurred_at,
        )?;
        Ok(outcome.recovered_exactly())
    }

    /// Records one rejected catalog candidate as append-only audit evidence.
    ///
    /// # Errors
    ///
    /// Returns the engine's typed storage failure.
    pub(super) fn reject_catalog_candidate(
        repository: &SqliteStorageRepository,
        handle: ProviderCatalogCandidateHandleDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProviderCatalogCandidateRejectedDto> {
        intention_engine::catalog::reject_catalog_candidate(repository, handle, occurred_at)
    }

    /// Resolves one turn's provider selection in the frozen precedence order.
    ///
    /// # Errors
    ///
    /// Returns `provider_profile_revision_mismatch` for a failed
    /// expected-revision check, `provider_profile_runtime_unavailable` for an
    /// effective profile without a live private entry, and
    /// `provider_configuration_unavailable` when no profile is configured.
    pub(super) fn resolve_turn_selection(
        command: &SendUserTurnCommandDto,
        session_default: Option<ProviderProfileId>,
        active: &ActiveCatalog,
        registry: &dyn ProviderDriverRegistry,
    ) -> DtoResult<ResolvedRunProviderSelectionDto> {
        let input = SelectionResolutionInputDto::new(
            command.provider_profile().cloned(),
            session_default,
            active.default_profile_id().cloned(),
            Some(active.revision.clone()),
            registry,
        );
        resolve_selection(input)?.into_accepted_selection()
    }

    /// Commits one session default change and publishes its typed event.
    ///
    /// # Errors
    ///
    /// Returns a typed conflict for a mismatched expected revision, a typed
    /// not-found failure for an unknown session, or a typed storage failure.
    pub(super) fn set_session_provider_profile(
        repository: &SqliteStorageRepository,
        sink: &dyn SessionEventSink,
        command: SetSessionProviderProfileCommandDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<SetSessionProviderProfileAcceptedDto> {
        intention_engine::selection::set_session_provider_profile(
            repository,
            sink,
            &command,
            occurred_at,
        )
    }

    /// Resolves the cross-turn reasoning history manifest of one dependent run.
    ///
    /// # Errors
    ///
    /// Returns the closed history verification or bound failure, or the typed
    /// storage read failure.
    pub(super) fn resolve_reasoning_history(
        repository: &SqliteStorageRepository,
        session_id: SessionId,
        transfer: &intention_proto::provider::ReasoningHistoryTransferDto,
    ) -> DtoResult<Option<ReasoningHistoryManifestDto>> {
        Ok(intention_engine::reasoning::resolve_reasoning_history(
            repository, session_id, transfer,
        )?
        .map(|resolution| resolution.manifest().clone()))
    }

    /// Accepts one user turn with its exact resolved provider selection.
    ///
    /// # Errors
    ///
    /// Returns the engine's typed admission or durable-acceptance failure.
    pub(super) fn send_user_turn(
        repository: &SqliteStorageRepository,
        command: SendUserTurnCommandDto,
        proposed_run_id: intention_proto::RunId,
        config_snapshot: ConfigSnapshotDto,
        selection: ResolvedRunProviderSelectionDto,
        reasoning_history: Option<ReasoningHistoryManifestDto>,
        occurred_at: TimestampDto,
    ) -> DtoResult<intention_storage::AcceptedTurnOutcomeDto> {
        ApplicationService::new(repository).send_user_turn(
            command,
            proposed_run_id,
            config_snapshot,
            selection,
            reasoning_history,
            occurred_at,
        )
    }
}

/// Returns the closed `execution_not_ready` failure of a degraded catalog.
fn execution_not_ready() -> ErrorDto {
    ErrorDto::validation(
        "execution_not_ready",
        "the provider catalog is degraded and read-only",
    )
}

/// Returns the closed failure of an unavailable exact profile runtime entry.
fn provider_profile_runtime_unavailable() -> ErrorDto {
    ErrorDto::validation(
        "provider_profile_runtime_unavailable",
        "the requested provider profile has no live runtime entry",
    )
}

/// Returns the closed failure of a catalog-affecting live change.
fn catalog_change_requires_restart() -> ErrorDto {
    ErrorDto::validation(
        "catalog_change_requires_restart",
        "catalog-affecting configuration changes require a daemon restart",
    )
}

/// Returns the closed failure of a rotation that would change frozen meaning.
fn credential_rotation_frozen_meaning_mismatch() -> ErrorDto {
    ErrorDto::validation(
        "credential_rotation_frozen_meaning_mismatch",
        "credential rotation requires every safe selected field to match its active revision",
    )
}

/// Returns the closed failure of a rotation without a file-backed source.
fn credential_source_unavailable() -> ErrorDto {
    ErrorDto::validation(
        "credential_rotation_source_unavailable",
        "no file-backed private credential source is configured",
    )
}

/// Returns the closed failure of a stale catalog change or candidate handle.
fn provider_catalog_changed() -> ErrorDto {
    ErrorDto::validation(
        "provider_catalog_changed",
        "the addressed provider catalog revision is no longer the active one",
    )
}

/// Returns the closed failure of a malformed catalog page token.
fn provider_catalog_page_token_invalid() -> ErrorDto {
    ErrorDto::validation(
        "provider_catalog_page_token_invalid",
        "the catalog page token is malformed",
    )
}

impl DaemonApplicationFacade {
    /// Returns the one durable repository this composition root owns.
    pub(crate) fn repository(&self) -> &SqliteStorageRepository {
        &self.inner.repository
    }

    /// Returns the durable-command admission gate to the daemon host.
    pub(crate) fn command_gate(&self) -> &CommandGate {
        &self.inner.command_gate
    }

    /// Returns a credential-free ready health projection.
    #[must_use]
    pub const fn health(&self) -> DaemonHealthDto {
        DaemonHealthDto::ready()
    }

    /// Loads one coherent current-state session snapshot.
    ///
    /// This is the single session read: recent transcript rows are bounded by
    /// the retained delivery bound and the byte budget derived from the
    /// transport envelope cap, which charges the encoded projection as well as
    /// the transcript rows. The run-scoped read is the dedicated run
    /// subscription, which the daemon host answers from the same repository.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the session projection or its
    /// transcript rows cannot be read.
    pub fn session_snapshot(&self, session_id: SessionId) -> DtoResult<SessionSnapshotDto> {
        let projection = self.inner.repository.load_session_projection(session_id)?;
        let projection_bytes = session_projection_bytes(&projection)?;
        let messages = bounded_snapshot_messages(
            projection_bytes,
            self.inner
                .repository
                .load_recent_messages(session_id, SESSION_SNAPSHOT_MESSAGES)?,
        )?;
        SessionSnapshotDto::with_projection(session_id, projection, messages)
    }

    /// Loads platform configuration, opens platform state storage, and recovers before ready.
    ///
    /// Raw TOML, credentials, and configuration paths remain inside this
    /// method's private loading boundary and are never included in public values
    /// or errors.
    ///
    /// # Errors
    ///
    /// Returns safe typed failures when platform configuration cannot be
    /// resolved, permission-checked, read, validated, prepared, accepted,
    /// activated, or recovered.
    pub fn open_platform() -> DtoResult<Self> {
        Self::open_with_source(ConfigPathResolver::resolve(None)?)
    }

    fn open_with_source(source: ConfigSourceDto) -> DtoResult<Self> {
        #[cfg(unix)]
        intention_config::ensure_user_only_permissions(source.path())?;
        let text = read_source_text(&source)?;
        let location = platform_database_location()?;
        let repository = open_repository(&location)?;
        Self::recover(&repository)?;
        Self::compose(repository, CredentialSource::FileBacked(source), text, None)
    }

    /// Opens a caller-provided database with an in-memory catalog document.
    ///
    /// This is the deterministic fixture host: the document is supplied
    /// directly, the composition keeps no file-backed private source, and a
    /// rotation or configuration edit therefore fails
    /// `credential_rotation_source_unavailable` instead of writing a file.
    ///
    /// # Errors
    ///
    /// Returns a safe typed storage, validation, or activation error.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn open_for_test_support(
        database_location: impl AsRef<Path>,
        document: &str,
    ) -> DtoResult<Self> {
        Self::open_in_memory_source(database_location.as_ref(), document.to_owned(), None)
    }

    /// Opens an in-memory catalog document host with an injected driver.
    ///
    /// # Errors
    ///
    /// Returns a safe typed storage, validation, or activation error.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn open_for_test_support_with_driver(
        database_location: impl AsRef<Path>,
        document: &str,
        driver: Arc<dyn ModelExecutionDriver + Send + Sync>,
    ) -> DtoResult<Self> {
        Self::open_in_memory_source(
            database_location.as_ref(),
            document.to_owned(),
            Some(driver),
        )
    }

    /// Opens a caller-provided database with a file-backed catalog document.
    ///
    /// The supplied text becomes the private configuration file, so reload,
    /// rotation, and configuration edits operate on a real file source exactly
    /// like a platform host.
    ///
    /// # Errors
    ///
    /// Returns a safe typed storage, validation, permission, or activation error.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn open_for_test_support_with_file(
        database_location: impl AsRef<Path>,
        config_path: impl AsRef<Path>,
        document: &str,
    ) -> DtoResult<Self> {
        Self::open_fixture_file(
            database_location.as_ref(),
            config_path.as_ref(),
            document,
            None,
        )
    }

    /// Opens a file-backed catalog document host with an injected driver.
    ///
    /// # Errors
    ///
    /// Returns a safe typed storage, validation, permission, or activation error.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn open_for_test_support_with_file_and_driver(
        database_location: impl AsRef<Path>,
        config_path: impl AsRef<Path>,
        document: &str,
        driver: Arc<dyn ModelExecutionDriver + Send + Sync>,
    ) -> DtoResult<Self> {
        Self::open_fixture_file(
            database_location.as_ref(),
            config_path.as_ref(),
            document,
            Some(driver),
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    fn open_in_memory_source(
        database_location: &Path,
        document: String,
        injected: Option<LiveDriver>,
    ) -> DtoResult<Self> {
        let repository = open_repository(database_location)?;
        Self::recover(&repository)?;
        Self::compose(repository, CredentialSource::Absent, document, injected)
    }

    #[cfg(any(test, feature = "test-support"))]
    fn open_fixture_file(
        database_location: &Path,
        config_path: &Path,
        document: &str,
        injected: Option<LiveDriver>,
    ) -> DtoResult<Self> {
        let source = ConfigSourceDto::Explicit(ConfigPathDto::parse(
            config_path.to_string_lossy().into_owned(),
        )?);
        write_source_text(&source, document)?;
        #[cfg(unix)]
        intention_config::ensure_user_only_permissions(source.path())?;
        let repository = open_repository(database_location)?;
        Self::recover(&repository)?;
        Self::compose(
            repository,
            CredentialSource::FileBacked(source),
            document.to_owned(),
            injected,
        )
    }

    /// Recovers durable state before the host serves anything.
    ///
    /// # Errors
    ///
    /// Returns the typed storage failure when recovery cannot be committed.
    fn recover(repository: &SqliteStorageRepository) -> DtoResult<()> {
        let occurred_at = now()?;
        let _interrupted = repository.recover_unfinished_runs(occurred_at)?;
        // An attempt that never reached a terminal state becomes an
        // interruption record; nothing resumes and nothing is retried.
        let _interrupted_attempts =
            repository.recover_unfinished_discovery_attempts(occurred_at)?;
        Ok(())
    }

    fn compose(
        repository: SqliteStorageRepository,
        source: CredentialSource,
        text: String,
        injected: Option<LiveDriver>,
    ) -> DtoResult<Self> {
        let candidate = CatalogCandidate::parse(RawConfigInputDto::new(text))?;
        let (document, credentials) =
            candidate.into_parts_for_catalog(|document, credentials| (document, credentials));
        let facade = Self {
            inner: Arc::new(FacadeInner {
                repository,
                catalog: CatalogRuntime::new(source, credentials, injected),
                configuration: Mutex::new(None),
                sessions: session_events(),
                command_gate: CommandGate::new(),
            }),
        };
        facade.start_catalog(document)?;
        Ok(facade)
    }

    /// Prepares, accepts, and activates the startup catalog before readiness.
    ///
    /// # Errors
    ///
    /// Returns the typed engine, adapter, or storage failure when the catalog
    /// cannot be prepared or activated, and `execution_not_ready` when the
    /// durable pointers are incoherent.
    fn start_catalog(&self, document: CatalogDocumentDto) -> DtoResult<()> {
        let durable = self.inner.repository.load_catalog_state()?;
        let accepted = durable.accepted_catalog_revision_id();
        let previous = match accepted {
            Some(revision_id) => Some(self.inner.repository.load_catalog_revision(revision_id)?),
            None => None,
        };
        let prepared = engine_api::prepare_catalog_candidate(&document, previous.as_ref(), now()?)?;
        if durable.requires_activation_recovery() {
            return self.recover_catalog(document, prepared, accepted);
        }
        let context_window = *document.context_window();
        if prepared.has_removals() {
            // An omitted profile or kind is a process-local pending removal: the
            // previous accepted catalog stays active and degraded read-only
            // until an explicit accept or reject.
            let revision = previous.ok_or_else(execution_not_ready)?;
            let active_registry = self.inner.catalog.build_registry(&revision, false)?;
            let candidate_registry = self
                .inner
                .catalog
                .build_registry(&prepared.revision, false)?;
            let handle = prepared
                .candidate_handle()
                .ok_or_else(provider_catalog_changed)?;
            self.inner.catalog.install_pending(
                ActiveCatalog { revision, document },
                active_registry,
                PendingCandidate {
                    handle,
                    preparation: prepared,
                    registry: candidate_registry,
                },
            )?;
            return self.install_configuration(context_window);
        }
        // All-or-nothing: the replacement registry is built and pre-validated
        // before any durable acceptance.
        let registry = self
            .inner
            .catalog
            .build_registry(&prepared.revision, true)?;
        let revision_id = match prepared.change {
            intention_engine::catalog::CatalogChangeKindDto::Unchanged => {
                // A semantic-equal catalog writes no new revision, while the
                // current display, enabled, and pricing policies still follow
                // the startup document.
                self.refresh_policies(&prepared, previous.as_ref())?;
                accepted.ok_or_else(execution_not_ready)?
            }
            intention_engine::catalog::CatalogChangeKindDto::FreshRunsOnly
            | intention_engine::catalog::CatalogChangeKindDto::CatalogAffecting => {
                engine_api::accept_catalog_revision(&self.inner.repository, &prepared, now()?)?
            }
        };
        engine_api::mark_catalog_activated(&self.inner.repository, revision_id, now()?)?;
        self.inner.catalog.install(
            ActiveCatalog {
                revision: prepared.revision,
                document,
            },
            registry,
        )?;
        self.install_configuration(context_window)
    }

    /// Recovers one accepted-but-not-activated catalog exactly, or stays degraded.
    ///
    /// # Errors
    ///
    /// Returns the typed engine, adapter, or storage failure when the exact
    /// accepted catalog cannot be rebuilt.
    fn recover_catalog(
        &self,
        document: CatalogDocumentDto,
        prepared: engine_api::PreparedCatalog,
        accepted: Option<CatalogRevisionId>,
    ) -> DtoResult<()> {
        let Some(accepted) = accepted else {
            return Err(execution_not_ready());
        };
        let accepted_revision = self.inner.repository.load_catalog_revision(accepted)?;
        if prepared.preparation.matches_revision(&accepted_revision) {
            let registry = self
                .inner
                .catalog
                .build_registry(&accepted_revision, true)?;
            if engine_api::recover_catalog_activation(
                &self.inner.repository,
                &accepted_revision,
                &prepared,
                now()?,
            )? {
                self.inner.catalog.install(
                    ActiveCatalog {
                        revision: accepted_revision,
                        document: document.clone(),
                    },
                    registry,
                )?;
                // The exact accepted revision became active; the recovery
                // taxonomy is written by the storage activation boundary.
                let context_window = *document.context_window();
                return self.install_configuration(context_window);
            }
        }
        // A changed file is never adopted: the exact accepted revision stays
        // accepted and unactivated, and the daemon serves degraded read-only.
        let registry = self
            .inner
            .catalog
            .build_registry(&accepted_revision, false)?;
        self.inner.catalog.install_degraded(
            ActiveCatalog {
                revision: accepted_revision,
                document: document.clone(),
            },
            registry,
            ProviderCatalogActivationStateDto::ActivationRecoveryRequired,
            ProviderCatalogDegradedReasonDto::ActivationRecoveryRequired,
        )?;
        let context_window = *document.context_window();
        self.install_configuration(context_window)
    }

    /// Refreshes non-revision-affecting profile policies of an equal catalog.
    ///
    /// # Errors
    ///
    /// Returns the typed storage failure when a policy cannot be stored.
    fn refresh_policies(
        &self,
        prepared: &engine_api::PreparedCatalog,
        previous: Option<&ProviderCatalogRevisionDto>,
    ) -> DtoResult<()> {
        let Some(previous) = previous else {
            return Ok(());
        };
        let occurred_at = now()?;
        for entry in prepared.revision.profile_policies() {
            let unchanged = previous
                .profile_policies()
                .iter()
                .find(|existing| existing.profile_id() == entry.profile_id())
                .is_some_and(|existing| existing.policy() == entry.policy());
            if !unchanged {
                self.inner.repository.store_profile_policy(
                    entry.profile_id().clone(),
                    entry.policy().clone(),
                    occurred_at,
                )?;
            }
        }
        Ok(())
    }

    /// Commits and installs the base configuration revision of this composition.
    ///
    /// # Errors
    ///
    /// Returns the typed snapshot validation or storage failure.
    fn install_configuration(&self, context_window: ContextWindowPolicyDto) -> DtoResult<()> {
        let revision_id = ConfigRevisionId::new();
        let snapshot = config_snapshot(revision_id, now()?, context_window)?;
        self.inner
            .repository
            .accept_configuration_revision(snapshot)?;
        {
            let mut configuration = self.configuration_lock()?;
            *configuration = Some(CurrentConfiguration::new(context_window, revision_id));
        }
        Ok(())
    }

    /// Commits one fresh-run configuration revision and installs it.
    ///
    /// # Errors
    ///
    /// Returns the typed snapshot validation or storage failure.
    fn commit_configuration(
        &self,
        context_window: ContextWindowPolicyDto,
    ) -> DtoResult<ConfigRevisionId> {
        let revision_id = ConfigRevisionId::new();
        let snapshot = config_snapshot(revision_id, now()?, context_window)?;
        self.inner
            .repository
            .accept_configuration_revision(snapshot)?;
        {
            let mut configuration = self.configuration_lock()?;
            *configuration = Some(CurrentConfiguration::new(context_window, revision_id));
        }
        Ok(revision_id)
    }

    /// Returns the configuration revision of one accepted turn.
    ///
    /// One committed snapshot per exact selected profile revision keeps a
    /// repeated acceptance replaying the identical configuration revision,
    /// which is the contract the durable turn replay compares.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when the configuration gate is
    /// poisoned, or `execution_not_ready` when the composition holds no
    /// configuration revision.
    fn turn_config_snapshot(
        &self,
        selection: &ResolvedRunProviderSelectionDto,
    ) -> DtoResult<ConfigSnapshotDto> {
        let mut guard = self.configuration_lock()?;
        let configuration = guard.as_mut().ok_or_else(execution_not_ready)?;
        if let Some(existing) = configuration
            .turn_revisions
            .get(&selection.provider_profile_revision_id())
        {
            let existing = existing.clone();
            drop(guard);
            return Ok(existing);
        }
        let snapshot = config_snapshot(
            ConfigRevisionId::new(),
            now()?,
            configuration.context_window,
        )?;
        configuration
            .turn_revisions
            .insert(selection.provider_profile_revision_id(), snapshot.clone());
        drop(guard);
        Ok(snapshot)
    }

    /// Locks the current configuration slot.
    ///
    /// # Errors
    ///
    /// Returns the typed unavailable failure when the configuration gate is poisoned.
    fn configuration_lock(
        &self,
    ) -> DtoResult<std::sync::MutexGuard<'_, Option<CurrentConfiguration>>> {
        self.inner.configuration.lock().map_err(|_| {
            ErrorDto::unavailable(
                "daemon_configuration_unavailable",
                "the daemon configuration state is unavailable",
            )
        })
    }

    /// Creates one durable session and assembles its typed reply evidence.
    ///
    /// # Errors
    ///
    /// Returns the typed durable failure when session creation is rejected.
    pub fn create_session(&self, command: CreateSessionCommandDto) -> DtoResult<ProtocolResultDto> {
        self.inner.command_gate.run(|| {
            let projection =
                ApplicationService::new(&self.inner.repository).create_session(command, now()?)?;
            Ok(ProtocolResultDto::SessionCreated(
                CreateSessionAcceptedDto::new(
                    projection.project_id(),
                    projection.workspace_id(),
                    projection.session_id(),
                ),
            ))
        })
    }

    /// Accepts one user turn with its exact resolved provider selection.
    ///
    /// The proposed run identity is derived from the caller's idempotency key,
    /// so a repeated command proposes the identity the durable turn already
    /// recorded and the repository replays its outcome instead of reporting a
    /// conflict. The selection resolves in the frozen order — turn override with
    /// its expected-revision check, session default, global default — and a
    /// resolved profile without a live private entry is rejected before commit.
    /// The run's cross-turn reasoning history is resolved from its own selection
    /// and commits in the same transaction as the turn.
    ///
    /// # Errors
    ///
    /// Returns the typed admission failure when the turn is rejected, or
    /// `execution_not_ready` while the catalog is degraded read-only.
    pub fn send_user_turn(&self, command: SendUserTurnCommandDto) -> DtoResult<ProtocolResultDto> {
        self.inner.command_gate.run(|| {
            self.inner.catalog.ensure_ready()?;
            let proposed_run_id = RunId::parse(&command.idempotency_key().to_string())?;
            let selection = self
                .inner
                .catalog
                .resolve_turn_selection(&self.inner.repository, &command)?;
            let config_snapshot = self.turn_config_snapshot(&selection)?;
            let reasoning_history = engine_api::resolve_reasoning_history(
                &self.inner.repository,
                command.session_id(),
                selection
                    .declared_model_capability_subset()
                    .context_preservation()
                    .reasoning_input_contract(),
            )?;
            let outcome = engine_api::send_user_turn(
                &self.inner.repository,
                command,
                proposed_run_id,
                config_snapshot,
                selection,
                reasoning_history,
                now()?,
            )?;
            Ok(turn_accepted(&outcome))
        })
    }

    /// Removes one not-yet-seen pending user turn and assembles its typed reply evidence.
    ///
    /// # Errors
    ///
    /// Returns the typed durable failure when no pending turn can be removed.
    pub fn remove_turn(&self, command: RemoveTurnCommandDto) -> DtoResult<ProtocolResultDto> {
        self.inner.command_gate.run(|| {
            let turn =
                ApplicationService::new(&self.inner.repository).remove_turn(command, now()?)?;
            Ok(ProtocolResultDto::TurnRemoved(RemoveTurnAcceptedDto::new(
                turn.session_id(),
                turn.turn_id(),
            )))
        })
    }

    /// Validates one interruption request and assembles its typed reply evidence.
    ///
    /// Interruption is not a durable run state: the validation commits nothing,
    /// so it is a pure read and never enters the command gate. The daemon host
    /// signals the registered execution afterwards, and the host registry lock
    /// is what orders that signal against admission.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error when the exact run is not active.
    pub fn interrupt_run(&self, command: InterruptRunCommandDto) -> DtoResult<ProtocolResultDto> {
        let run = ApplicationService::new(&self.inner.repository).interrupt_run(command)?;
        Ok(ProtocolResultDto::RunInterrupted(
            InterruptRunAcceptedDto::new(run.session_id(), run.run_id()),
        ))
    }

    /// Lists one bounded page of the active credential-free catalog.
    ///
    /// Entries are ordered by stable profile identity; the continuation token is
    /// opaque and names the active catalog revision it was issued for, so a
    /// token from another revision fails `provider_catalog_changed`.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_page_token_invalid` for a malformed or foreign
    /// token, `provider_catalog_changed` for a token of another revision, or
    /// `execution_not_ready` when no catalog is active yet.
    pub fn list_provider_catalog(
        &self,
        query: ListProviderCatalogQueryDto,
    ) -> DtoResult<ProtocolResultDto> {
        let active = self.inner.catalog.active()?;
        let revision_id = active.revision.catalog_revision_id();
        let default_profile_id = active.default_profile_id().cloned();
        let entries = self.inner.catalog.entries()?;
        let start = match query.page_token() {
            None => 0,
            Some(token) => {
                let (token_revision, profile_id) =
                    decode_page_token(token).ok_or_else(provider_catalog_page_token_invalid)?;
                if token_revision != revision_id {
                    return Err(provider_catalog_changed());
                }
                entries
                    .iter()
                    .position(|entry| entry.profile_id() == &profile_id)
                    .map(|index| index + 1)
                    .ok_or_else(provider_catalog_page_token_invalid)?
            }
        };
        Ok(ProtocolResultDto::ProviderCatalogPage(catalog_page(
            revision_id,
            default_profile_id,
            entries,
            start,
        )?))
    }

    /// Returns the current safe provider catalog status.
    ///
    /// # Errors
    ///
    /// Returns `execution_not_ready` when no catalog is active yet, or a typed
    /// runtime failure.
    pub fn provider_catalog_status(&self) -> DtoResult<ProtocolResultDto> {
        let (activation, degraded) = self.inner.catalog.status()?;
        let active = self.inner.catalog.active()?;
        let candidate = self.inner.catalog.pending_handle()?;
        Ok(ProtocolResultDto::ProviderCatalogStatus(
            ProviderCatalogStatusDto::new(
                activation,
                degraded,
                Some(active.revision.catalog_revision_id()),
                candidate,
                active.default_profile_id().cloned(),
                Vec::new(),
            )?,
        ))
    }

    /// Changes the durable session default provider profile.
    ///
    /// The command is optimistic and takes the session, a known and enabled
    /// profile with a live private entry, and the expected session projection
    /// revision. A real change publishes the typed
    /// `SessionProviderProfileChanged` value to the composition's session-event
    /// port; setting the already-durable profile is a successful no-change
    /// outcome that publishes nothing.
    ///
    /// # Errors
    ///
    /// Returns `execution_not_ready` while the catalog is degraded read-only,
    /// `provider_profile_runtime_unavailable` for an unavailable target, a typed
    /// conflict for a mismatched expected revision, or the typed storage failure.
    pub fn set_session_provider_profile(
        &self,
        command: SetSessionProviderProfileCommandDto,
    ) -> DtoResult<ProtocolResultDto> {
        self.inner.command_gate.run(|| {
            self.inner.catalog.ensure_ready()?;
            let active = self.inner.catalog.active()?;
            if active.profile(command.profile_id()).is_none()
                || active
                    .policy(command.profile_id())
                    .is_none_or(|policy| !policy.enabled())
            {
                return Err(provider_profile_runtime_unavailable());
            }
            let revision = self.inner.catalog.profile_revision(command.profile_id())?;
            let key = registry_key_of_revision(&revision)?;
            if !self.inner.catalog.registry_has(&key)? {
                return Err(provider_profile_runtime_unavailable());
            }
            let accepted = engine_api::set_session_provider_profile(
                &self.inner.repository,
                &self.inner.sessions,
                command,
                now()?,
            )?;
            Ok(ProtocolResultDto::SessionProviderProfileSet(accepted))
        })
    }

    /// Returns one session's durable provider default with its safe resolution.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an unknown session or the typed storage
    /// failure when the durable read fails.
    pub fn session_provider_profile(
        &self,
        query: GetSessionProviderProfileQueryDto,
    ) -> DtoResult<ProtocolResultDto> {
        let durable = self
            .inner
            .repository
            .load_session_provider_profile(query.session_id())?;
        let active = self.inner.catalog.active()?;
        let entries = self.inner.catalog.entries()?;
        let durable_profile_id = durable.provider_profile_id().cloned();
        let resolved_entry = durable_profile_id.as_ref().and_then(|profile_id| {
            entries
                .into_iter()
                .find(|entry| entry.profile_id() == profile_id)
        });
        let unavailability = match (&durable_profile_id, &resolved_entry) {
            (None, _) | (Some(_), None) => Some(ProviderSelectionUnavailabilityDto::Missing),
            (Some(_), Some(_)) => None,
        };
        Ok(ProtocolResultDto::SessionProviderProfile(
            SessionProviderProfileProjectionDto::new(
                query.session_id(),
                durable_profile_id,
                resolved_entry,
                unavailability,
                durable.session_projection_revision(),
                active.default_profile_id().cloned(),
            )?,
        ))
    }

    /// Accepts the one pending catalog removal.
    ///
    /// The exact candidate handle is verified first; acceptance commits the
    /// catalog state and only then swaps the pre-validated private registry. No
    /// external action happens inside the durable transaction.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_changed` for a stale or unknown handle, or the
    /// typed engine or storage failure.
    pub fn accept_provider_catalog_removal(
        &self,
        command: AcceptProviderCatalogRemovalCommandDto,
    ) -> DtoResult<ProtocolResultDto> {
        self.inner.command_gate.run(|| {
            let candidate = self.inner.catalog.take_pending(command.candidate())?;
            let accepted = engine_api::accept_catalog_revision(
                &self.inner.repository,
                &candidate.preparation,
                now()?,
            )
            .and_then(|revision_id| {
                engine_api::mark_catalog_activated(&self.inner.repository, revision_id, now()?)?;
                Ok(revision_id)
            });
            let revision_id = match accepted {
                Ok(revision_id) => revision_id,
                Err(error) => {
                    // The candidate is process-local, so a failed durable step
                    // must leave it in place for an explicit retry.
                    let _restored = self.inner.catalog.restore_pending(candidate);
                    return Err(error);
                }
            };
            self.inner.catalog.promote_pending(candidate)?;
            Ok(ProtocolResultDto::ProviderCatalogRemovalAccepted(
                ProviderCatalogRemovalAcceptedDto::new(revision_id),
            ))
        })
    }

    /// Rejects the one pending catalog candidate and stays degraded read-only.
    ///
    /// # Errors
    ///
    /// Returns `provider_catalog_changed` for a stale or unknown handle, or the
    /// typed engine or storage failure.
    pub fn reject_provider_catalog_candidate(
        &self,
        command: RejectProviderCatalogCandidateCommandDto,
    ) -> DtoResult<ProtocolResultDto> {
        self.inner.command_gate.run(|| {
            let candidate = self.inner.catalog.take_pending(command.candidate())?;
            let rejected = engine_api::reject_catalog_candidate(
                &self.inner.repository,
                candidate.handle,
                now()?,
            )?;
            drop(candidate);
            self.inner.catalog.mark_candidate_rejected()?;
            Ok(ProtocolResultDto::ProviderCatalogCandidateRejected(
                rejected,
            ))
        })
    }

    /// Reloads the configuration file through the private loading boundary.
    ///
    /// A semantically equal candidate is a no-op success; a candidate that only
    /// changes the global context window commits a new configuration revision
    /// that fresh runs apply; a catalog-affecting candidate rejects with
    /// `catalog_change_requires_restart` and commits nothing.
    ///
    /// # Errors
    ///
    /// Returns `execution_not_ready` while the catalog is degraded read-only,
    /// `credential_rotation_source_unavailable` without a file-backed source,
    /// `catalog_change_requires_restart` for a catalog-affecting change, or the
    /// typed read, validation, or storage failure.
    pub fn reload_configuration(
        &self,
        _command: ReloadConfigurationCommandDto,
    ) -> DtoResult<ProtocolResultDto> {
        self.inner.command_gate.run(|| {
            self.inner.catalog.ensure_ready()?;
            let source = self
                .inner
                .catalog
                .source
                .file()
                .ok_or_else(credential_source_unavailable)?;
            let text = read_source_text(source)?;
            let candidate = CatalogCandidate::parse(RawConfigInputDto::new(text))?;
            let active_document = self.inner.catalog.active_document()?;
            let impact = reload_impact(&active_document, candidate.safe_document())?;
            let catalog_revision_id = self.inner.catalog.active_revision_id()?;
            match impact {
                CatalogReloadImpactDto::Unchanged => {
                    let revision_id = {
                        let configuration = self.configuration_lock()?;
                        configuration
                            .as_ref()
                            .ok_or_else(execution_not_ready)?
                            .revision_id
                    };
                    Ok(ProtocolResultDto::ConfigurationReloaded(
                        ConfigurationReloadAcceptedDto::new(revision_id, catalog_revision_id),
                    ))
                }
                CatalogReloadImpactDto::FreshRunsOnly => {
                    let context_window = *candidate.safe_document().context_window();
                    let revision_id = self.commit_configuration(context_window)?;
                    self.inner
                        .catalog
                        .refresh_document(candidate.safe_document().clone())?;
                    Ok(ProtocolResultDto::ConfigurationReloaded(
                        ConfigurationReloadAcceptedDto::new(revision_id, catalog_revision_id),
                    ))
                }
            }
        })
    }

    /// Rotates one profile's private credential without changing frozen meaning.
    ///
    /// # Errors
    ///
    /// Returns `credential_rotation_source_unavailable` without a file-backed
    /// source, `credential_rotation_frozen_meaning_mismatch` when a safe selected
    /// field changed, or the typed read, validation, or adapter failure.
    pub fn rotate_provider_credential(
        &self,
        command: RotateProviderCredentialCommandDto,
    ) -> DtoResult<ProtocolResultDto> {
        self.inner.command_gate.run(|| {
            self.inner.catalog.ensure_ready()?;
            let source = self
                .inner
                .catalog
                .source
                .file()
                .ok_or_else(credential_source_unavailable)?;
            let text = read_source_text(source)?;
            let candidate = CatalogCandidate::parse(RawConfigInputDto::new(text))?;
            let profile_id = command.profile_id().clone();
            let document = candidate.safe_document().clone();
            let active = self.inner.catalog.active()?;
            if !frozen_meaning_matches(&active.document, &document) {
                return Err(credential_rotation_frozen_meaning_mismatch());
            }
            let credential = candidate.into_parts_for_catalog(|_document, material| {
                material.with_profile_credential(&profile_id, str::to_owned)
            })?;
            let revision = self.inner.catalog.profile_revision(&profile_id)?;
            self.inner.catalog.rotate_credential(
                profile_id.clone(),
                PrivateCredential(credential),
                &revision,
            )?;
            Ok(ProtocolResultDto::ProviderCredentialRotated(
                CredentialRotationAcceptedDto::new(profile_id),
            ))
        })
    }

    /// Probes one profile's live provider for non-authorizing health evidence.
    ///
    /// A probe is one request with no retry, creates no run, retry counter, or
    /// quota, and is served even while the catalog is degraded: evidence that
    /// changes no provider state stays available in the read-only surface.
    ///
    /// # Errors
    ///
    /// Returns `provider_profile_runtime_unavailable` when the profile has no
    /// live entry, or the adapter's typed probe failure.
    pub async fn check_provider_health(
        &self,
        command: CheckProviderHealthCommandDto,
    ) -> DtoResult<ProtocolResultDto> {
        let driver = self
            .inner
            .catalog
            .driver_for_profile(command.profile_id())?;
        let evidence = driver.health_probe().await?;
        Ok(ProtocolResultDto::ProviderHealth(evidence))
    }

    /// Runs one provider/model discovery attempt with durable attempt evidence.
    ///
    /// The attempt is durably admitted before dispatch, marked started before
    /// the outbound boundary, and terminalized with its records or its safe
    /// failure. Nothing resumes or retries it.
    ///
    /// # Errors
    ///
    /// Returns `provider_profile_runtime_unavailable` when the profile has no
    /// live entry, the adapter's typed listing failure, or the typed storage
    /// failure.
    pub async fn discover_provider_models(
        &self,
        command: DiscoverProviderModelsCommandDto,
    ) -> DtoResult<ProtocolResultDto> {
        let driver = self
            .inner
            .catalog
            .driver_for_profile(command.profile_id())?;
        let attempt_id = ProviderDiscoveryAttemptId::new();
        let profile_id = command.profile_id().clone();
        let _attempted =
            self.inner
                .repository
                .begin_discovery_attempt(attempt_id, profile_id, now()?)?;
        let _started = self
            .inner
            .repository
            .mark_discovery_started(attempt_id, now()?)?;
        match driver.list_models().await {
            Ok(records) => {
                let result = self.inner.repository.complete_discovery_attempt(
                    attempt_id,
                    records,
                    now()?,
                )?;
                Ok(ProtocolResultDto::ProviderModelsDiscovered(result))
            }
            Err(error) => {
                let failure = ProviderDiscoveryFailureDto::new(error.code(), error.message())?;
                // The terminal evidence is best effort: an unfinished attempt is
                // terminalized as interrupted when the daemon next opens, and
                // the provider's own failure stays the command outcome.
                let _terminal =
                    self.inner
                        .repository
                        .fail_discovery_attempt(attempt_id, failure, now()?);
                Err(error)
            }
        }
    }

    /// Applies one credential-free configuration document to the file.
    ///
    /// # Errors
    ///
    /// Returns `configuration_edit_contains_credential` for a credential-bearing
    /// candidate, `credential_rotation_source_unavailable` without a file-backed
    /// source, or the typed validation, write, or storage failure.
    pub fn apply_configuration_document(
        &self,
        command: ApplyConfigurationDocumentCommandDto,
    ) -> DtoResult<ProtocolResultDto> {
        self.inner.command_gate.run(|| {
            let document = CatalogDocumentDto::parse_credential_free_edit(command.document())?;
            self.apply_edited_document(document)
                .map(ProtocolResultDto::ConfigurationDocumentApplied)
        })
    }

    /// Applies closed typed configuration edits to the file.
    ///
    /// # Errors
    ///
    /// Returns `invalid_configuration_edit` for an inapplicable edit,
    /// `credential_rotation_source_unavailable` without a file-backed source, or
    /// the typed validation, write, or storage failure.
    pub fn apply_configuration_edits(
        &self,
        command: ApplyConfigurationEditsCommandDto,
    ) -> DtoResult<ProtocolResultDto> {
        self.inner.command_gate.run(|| {
            let active_document = self.inner.catalog.active_document()?;
            let document = active_document.apply_edits(command.edits())?;
            self.apply_edited_document(document)
                .map(ProtocolResultDto::ConfigurationEditsApplied)
        })
    }

    /// Validates, writes, and commits one edited catalog document.
    ///
    /// The edited document is credential-free; the retained private material is
    /// restored inside the private boundary, the file is replaced atomically
    /// with its mode preserved, and the returned acceptance names the committed
    /// configuration revision and whether the edit can only take effect at the
    /// next daemon restart. The response never carries a credential.
    ///
    /// # Errors
    ///
    /// Returns `credential_rotation_source_unavailable` without a file-backed
    /// source, or the typed validation, write, or storage failure.
    fn apply_edited_document(
        &self,
        document: CatalogDocumentDto,
    ) -> DtoResult<ConfigurationEditAcceptedDto> {
        let rendered = self.inner.catalog.render_source(&document)?;
        let active_document = self.inner.catalog.active_document()?;
        let requires_restart = catalog_affecting(&active_document, &document);
        let source = self
            .inner
            .catalog
            .source
            .file()
            .ok_or_else(credential_source_unavailable)?;
        write_source_text(source, &rendered)?;
        let context_window = *document.context_window();
        let revision_id = self.commit_configuration(context_window)?;
        if !requires_restart {
            // Only the global context window can change without a restart; the
            // refreshed document keeps the reload comparison exact.
            self.inner.catalog.refresh_document(document)?;
        }
        Ok(ConfigurationEditAcceptedDto::new(
            revision_id,
            requires_restart,
        ))
    }

    /// Returns the live driver of one exact persisted run selection.
    ///
    /// A missing or unreadable runtime gate returns `None` so the host fails the
    /// run closed instead of calling any provider.
    #[must_use]
    pub(crate) fn resolve_driver_for_run(
        &self,
        selection: &ResolvedRunProviderSelectionDto,
    ) -> Option<LiveDriver> {
        self.inner
            .catalog
            .driver_for_selection(selection)
            .ok()
            .flatten()
    }

    /// Returns every session-event publication a fixture host observed.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    #[must_use]
    pub fn recorded_session_events(&self) -> Vec<SessionProviderProfileChangedDto> {
        self.inner.sessions.recorded()
    }
}

/// Assembles the wire acceptance evidence of one committed user-turn outcome.
///
/// The engine returns the committed durable outcome; the boundary that owns the
/// typed wire vocabulary converts it once here.
const fn turn_accepted(outcome: &AcceptedTurnOutcomeDto) -> ProtocolResultDto {
    match outcome {
        AcceptedTurnOutcomeDto::Started { run, .. } => {
            ProtocolResultDto::TurnAccepted(SendUserTurnAcceptedDto::new(
                run.session_id(),
                run.turn_id(),
                SendUserTurnOutcomeDto::Started {
                    run_id: run.run_id(),
                    config_revision_id: run.config_revision_id(),
                },
            ))
        }
        AcceptedTurnOutcomeDto::Pending(turn) => {
            ProtocolResultDto::TurnAccepted(SendUserTurnAcceptedDto::new(
                turn.session_id(),
                turn.turn_id(),
                SendUserTurnOutcomeDto::Pending,
            ))
        }
    }
}

/// Builds one validated configuration snapshot from its safe parts.
///
/// The committed revision carries the window policy and its own revision
/// identity; provider identity lives in the run's persisted resolved selection.
///
/// # Errors
///
/// Returns the typed snapshot validation failure.
fn config_snapshot(
    revision_id: ConfigRevisionId,
    captured_at: TimestampDto,
    context_window: ContextWindowPolicyDto,
) -> DtoResult<ConfigSnapshotDto> {
    ConfigSnapshotDto::new(
        CONFIG_SCHEMA_VERSION,
        revision_id,
        captured_at,
        context_window,
    )
}

/// Returns whether one candidate changes catalog-affecting configuration.
///
/// The kind and profile sets, the default profile, every revision-affecting
/// declaration, and the non-revision-affecting display, enabled, and pricing
/// policies are startup-applied catalog state: a live change to any of them
/// requires a restart. Only the global context window changes fresh-run
/// behavior without one.
fn catalog_affecting(active: &CatalogDocumentDto, candidate: &CatalogDocumentDto) -> bool {
    active.user_kinds() != candidate.user_kinds()
        || active.default_profile() != candidate.default_profile()
        || active.profiles() != candidate.profiles()
}

/// Classifies one reload candidate against the active document.
///
/// # Errors
///
/// Returns `catalog_change_requires_restart` for every catalog-affecting change.
fn reload_impact(
    active: &CatalogDocumentDto,
    candidate: &CatalogDocumentDto,
) -> DtoResult<CatalogReloadImpactDto> {
    let impact = classify_catalog_reload(active, candidate)?;
    if impact == CatalogReloadImpactDto::FreshRunsOnly && catalog_affecting(active, candidate) {
        return Err(catalog_change_requires_restart());
    }
    Ok(impact)
}

/// Returns whether two documents share every frozen revision-affecting meaning.
///
/// Only the addressed profile's private credential may differ; every declared
/// kind, the default profile, and every revision-affecting profile declaration
/// must stay exactly the revision the active catalog was built from.
fn frozen_meaning_matches(active: &CatalogDocumentDto, candidate: &CatalogDocumentDto) -> bool {
    active.user_kinds() == candidate.user_kinds()
        && active.default_profile() == candidate.default_profile()
        && active.profiles().len() == candidate.profiles().len()
        && active.profiles().iter().all(|profile| {
            candidate
                .profile(profile.profile_id())
                .is_some_and(|other| profile.declaration() == other.declaration())
        })
}

/// Returns the registry identity of one exact profile revision.
///
/// # Errors
///
/// Returns `provider_profile_runtime_unavailable` when the kind has no
/// code-owned driver contract.
fn registry_key_of_revision(revision: &ProviderProfileRevisionV1) -> DtoResult<RegistryKey> {
    let contract =
        driver_contract(revision.kind_id()).ok_or_else(provider_profile_runtime_unavailable)?;
    Ok((
        revision.revision_id(),
        revision.kind_descriptor_revision_id(),
        contract.driver_family().to_owned(),
        contract.major(),
        contract.minor(),
    ))
}

/// Returns the registry identity of one exact persisted selection.
fn registry_key_of_selection(selection: &ResolvedRunProviderSelectionDto) -> RegistryKey {
    let contract = selection.provider_driver_contract_revision();
    (
        selection.provider_profile_revision_id(),
        selection.kind_descriptor_revision_id(),
        contract.driver_family().to_owned(),
        contract.major(),
        contract.minor(),
    )
}

/// Builds one live driver from an exact revision and its private credential.
///
/// The adapter's declared options are derived from the same revision, so the
/// authentication header policy the adapter applies is exactly the declared
/// credential transport; a transport the adapter cannot apply fails closed
/// inside the adapter instead of being silently dropped.
///
/// # Errors
///
/// Returns the adapter's typed construction failure, or
/// `provider_profile_runtime_unavailable` when the kind has no code-owned driver.
fn build_driver(revision: &ProviderProfileRevisionV1, credential: &str) -> DtoResult<LiveDriver> {
    let policy =
        AuthenticationHeaderPolicyV1::from_credential_transport(revision.credential_transport());
    let kind = revision.kind_id().as_str();
    let driver: LiveDriver = if kind == OPENROUTER_KIND_ID {
        let options = OpenRouterDriverOptions::from_profile_revision(revision);
        if options.credential_transport().mode() != policy.transport() {
            return Err(provider_profile_runtime_unavailable());
        }
        Arc::new(OpenRouterDriver::from_profile_revision_with_options(
            revision,
            credential.to_owned(),
            options,
        )?)
    } else if kind == GENERIC_CHAT_KIND_ID {
        let options = GenericChatDriverOptions::from_profile_revision(revision);
        if options.credential_transport().mode() != policy.transport() {
            return Err(provider_profile_runtime_unavailable());
        }
        Arc::new(GenericChatDriver::from_profile_revision_with_options(
            revision,
            credential.to_owned(),
            options,
        )?)
    } else {
        return Err(provider_profile_runtime_unavailable());
    };
    Ok(driver)
}

/// Builds one credential-free catalog entry.
///
/// # Errors
///
/// Returns a typed validation error when the entry cannot be built.
fn profile_entry(
    profile: &ProviderProfileRevisionV1,
    policy: &ProviderProfilePolicyDto,
    credential_configured: bool,
    live: bool,
) -> DtoResult<ProviderProfileEntryDto> {
    let capabilities = driver_capabilities(profile.kind_id());
    let driver_capabilities = ProviderDriverCapabilitiesDto::new(
        capabilities.supports_text(),
        capabilities.supports_reasoning(),
        capabilities.supports_tool_calls(),
    );
    let readiness = if policy.enabled() {
        if live {
            ProviderProfileReadinessDto::Ready
        } else {
            ProviderProfileReadinessDto::Unavailable
        }
    } else {
        ProviderProfileReadinessDto::Disabled
    };
    ProviderProfileEntryDto::new(
        profile.profile_id().clone(),
        policy.display_name(),
        policy.enabled(),
        profile.kind_id().clone(),
        profile.kind_descriptor_revision_id(),
        profile.model_id(),
        profile.normalized_effective_endpoint().map(str::to_owned),
        profile.effective_execution_policy(),
        profile.declared_model_capability_subset().clone(),
        profile.credential_transport().clone(),
        credential_configured,
        driver_capabilities,
        readiness,
        policy.pricing().cloned(),
    )
}

/// Returns the fallback policy of one catalog revision member.
///
/// # Errors
///
/// Returns a typed validation error when the profile identity cannot name a
/// policy, which a validated catalog revision cannot produce.
fn default_profile_policy(
    profile: &ProviderProfileRevisionV1,
) -> DtoResult<ProviderProfilePolicyDto> {
    ProviderProfilePolicyDto::new(profile.profile_id().as_str(), true, None)
}

/// Encodes one opaque page token naming the catalog revision and last entry.
fn encode_page_token(revision_id: CatalogRevisionId, profile_id: &ProviderProfileId) -> String {
    format!("{revision_id}:{}", profile_id.as_str())
}

/// Decodes one opaque catalog page token.
fn decode_page_token(token: &str) -> Option<(CatalogRevisionId, ProviderProfileId)> {
    let (revision, profile) = token.split_once(':')?;
    Some((
        CatalogRevisionId::parse(revision).ok()?,
        ProviderProfileId::parse(profile).ok()?,
    ))
}

/// Builds one page of catalog entries under the transport envelope budget.
///
/// # Errors
///
/// Returns a typed encode failure or the page validation failure.
fn catalog_page(
    revision_id: CatalogRevisionId,
    default_profile_id: Option<ProviderProfileId>,
    entries: Vec<ProviderProfileEntryDto>,
    start: usize,
) -> DtoResult<ProviderCatalogPageDto> {
    let total = entries.len();
    let skeleton = ProviderCatalogPageDto::new(
        Some(revision_id),
        default_profile_id.clone(),
        Vec::new(),
        None,
        false,
    )?;
    let mut counter = CountingWriter(0);
    serde_json::to_writer(&mut counter, &skeleton).map_err(|_| encode_failure())?;
    let mut budget = MAX_TRANSCRIPT_SNAPSHOT_BYTES.saturating_sub(counter.0);
    let mut page = Vec::new();
    for entry in entries.into_iter().skip(start) {
        let mut counter = CountingWriter(0);
        serde_json::to_writer(&mut counter, &entry).map_err(|_| encode_failure())?;
        let size = counter.0.saturating_add(1);
        if size > budget && !page.is_empty() {
            break;
        }
        budget = budget.saturating_sub(size);
        page.push(entry);
    }
    let next_start = start.saturating_add(page.len());
    let has_more = next_start < total;
    let next_page_token = if has_more {
        page.last()
            .map(|entry| encode_page_token(revision_id, entry.profile_id()))
    } else {
        None
    };
    ProviderCatalogPageDto::new(
        Some(revision_id),
        default_profile_id,
        page,
        next_page_token,
        has_more,
    )
}

/// Renders one credential-bearing configuration document inside the private boundary.
///
/// The configuration lane renders the credential-free document deterministically;
/// this boundary restores each profile's retained private credential into its
/// own table. A rendered profile table the composition cannot address would
/// silently lose that profile's credential, so the write is refused instead of
/// committed.
///
/// # Errors
///
/// Returns a typed validation error when the document cannot be rendered, or
/// `credential_rotation_source_unavailable` when a profile's retained private
/// credential cannot be restored.
fn render_credential_document(
    candidate: &CatalogCandidate,
    credentials: &CatalogCredentialMaterial,
) -> DtoResult<String> {
    let rendered = candidate.render_edited_document(&[])?;
    let document = candidate.safe_document();
    let mut output = String::with_capacity(rendered.len());
    let mut restored = 0_usize;
    for line in rendered.lines() {
        output.push_str(line);
        output.push('\n');
        let Some(profile_id) = document
            .profiles()
            .iter()
            .find(|profile| profile_table_header(profile.profile_id()) == line)
            .map(|profile| profile.profile_id())
        else {
            continue;
        };
        let credential = credentials.with_profile_credential(profile_id, toml_basic_string)?;
        output.push_str("credential = ");
        output.push_str(&credential);
        output.push('\n');
        restored = restored.saturating_add(1);
    }
    if restored != document.profiles().len() {
        return Err(credential_source_unavailable());
    }
    Ok(output)
}

/// Returns the rendered TOML table header of one profile.
fn profile_table_header(profile_id: &ProviderProfileId) -> String {
    format!("[providers.profiles.{}]", toml_key(profile_id.as_str()))
}

/// Returns one TOML basic string with every control and escape represented.
fn toml_basic_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\u{8}' => output.push_str("\\b"),
            '\u{c}' => output.push_str("\\f"),
            character if character.is_control() => {
                let code = u32::from(character);
                output.push_str(&format!("\\u{code:04X}"));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

/// Returns one TOML key, quoted when it is not a bare key.
fn toml_key(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return value.to_owned();
    }
    toml_basic_string(value)
}

/// Reads the private configuration text of one file-backed source.
///
/// # Errors
///
/// Returns the typed read failure, which never discloses the local path.
fn read_source_text(source: &ConfigSourceDto) -> DtoResult<String> {
    fs::read_to_string(source.path().as_str()).map_err(|_| {
        ErrorDto::unavailable(
            "daemon_configuration_read_unavailable",
            "daemon configuration could not be read",
        )
    })
}

/// Replaces one configuration file atomically, preserving its permissions.
///
/// # Errors
///
/// Returns the typed write failure, which never discloses the local path.
fn write_source_text(source: &ConfigSourceDto, text: &str) -> DtoResult<()> {
    let path = Path::new(source.path().as_str());
    let permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    // A freshly created configuration file has no previous mode to preserve.
    let created_fresh = permissions.is_none();
    let temporary = PathBuf::from(format!(
        "{}{CONFIGURATION_WRITE_SUFFIX}",
        source.path().as_str()
    ));
    let mut file = fs::File::create(&temporary).map_err(|_| configuration_write_unavailable())?;
    let written = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|_| configuration_write_unavailable());
    drop(file);
    if written.is_err() {
        let _removed = fs::remove_file(&temporary);
        return written;
    }
    if let Some(permissions) = permissions {
        fs::set_permissions(&temporary, permissions)
            .map_err(|_| configuration_write_unavailable())?;
    }
    #[cfg(unix)]
    {
        // A freshly created configuration file must stay user-only even where
        // the platform umask would widen it.
        if created_fresh {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))
                .map_err(|_| configuration_write_unavailable())?;
        }
    }
    #[cfg(windows)]
    {
        // A rename cannot replace an existing file on Windows, so the previous
        // file is removed first; the written candidate is already complete and
        // synced before that point.
        if path.exists() {
            fs::remove_file(path).map_err(|_| configuration_write_unavailable())?;
        }
    }
    fs::rename(&temporary, path).map_err(|_| {
        let _removed = fs::remove_file(&temporary);
        configuration_write_unavailable()
    })
}

/// Returns the closed failure of a configuration write.
fn configuration_write_unavailable() -> ErrorDto {
    ErrorDto::unavailable(
        "daemon_configuration_write_unavailable",
        "daemon configuration could not be written",
    )
}

/// Opens one durable SQLite repository at a caller-provided location.
///
/// # Errors
///
/// Returns a safe typed storage failure; the local path never enters a public
/// DTO or error.
fn open_repository(location: &Path) -> DtoResult<SqliteStorageRepository> {
    SqliteStorageRepository::open(SqliteDatabaseLocationDto::new(
        location.to_string_lossy().into_owned(),
    )?)
}

/// The one session-event slot of one host build.
#[cfg(not(any(test, feature = "test-support")))]
const fn session_events() -> SessionEvents {
    SessionEvents::production()
}

/// The fixture session-event slot: publications stay observable.
#[cfg(any(test, feature = "test-support"))]
fn session_events() -> SessionEvents {
    SessionEvents::observable()
}

/// Counts the bytes one typed message encodes to without buffering them.
///
/// The snapshot budget needs the encoded length only, so encoding writes into
/// this counter instead of one intermediate `Vec<u8>` per committed row.
struct CountingWriter(usize);

impl std::io::Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(buffer.len());
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Trims one snapshot transcript to the representation budget derived from the
/// single transport envelope cap, keeping the newest committed rows that fit.
///
/// A transcript snapshot is the only legitimate response large enough to
/// approach the envelope cap, so the read path owns the byte budget and the
/// transport owns the cap: no legitimate snapshot can be dropped silently at
/// the connection level. The whole message is charged, so the encoded
/// projection that travels beside the transcript is subtracted before the rows
/// are measured, and the newest row is kept unconditionally: a row that alone
/// exceeds the budget must reach the transport, which answers with its typed
/// over-size failure instead of serving an empty transcript.
///
/// # Errors
///
/// Returns the typed local encode failure when a transcript row cannot be
/// encoded.
pub fn bounded_snapshot_messages(
    projection_bytes: usize,
    mut messages: Vec<intention_proto::MessageProjectionDto>,
) -> DtoResult<Vec<intention_proto::MessageProjectionDto>> {
    let mut budget = MAX_TRANSCRIPT_SNAPSHOT_BYTES.saturating_sub(projection_bytes);
    let newest = messages.len().saturating_sub(1);
    let mut keep_from = messages.len();
    for (index, message) in messages.iter().enumerate().rev() {
        let size = encoded_message_bytes(message)?.saturating_add(1);
        if size > budget && index != newest {
            break;
        }
        budget = budget.saturating_sub(size);
        keep_from = index;
    }
    messages.drain(..keep_from);
    Ok(messages)
}

/// Trims one run subscription transcript to the budget its projection leaves.
///
/// The run snapshot carries the same whole-message contract as the session
/// snapshot, so its committed projection is charged before the transcript rows
/// are measured.
///
/// # Errors
///
/// Returns the typed local encode failure when the projection or a transcript
/// row cannot be encoded.
pub fn bounded_run_snapshot_messages(
    run: &RunProjectionDto,
    messages: Vec<intention_proto::MessageProjectionDto>,
) -> DtoResult<Vec<intention_proto::MessageProjectionDto>> {
    let mut counter = CountingWriter(0);
    serde_json::to_writer(&mut counter, run).map_err(|_| encode_failure())?;
    bounded_snapshot_messages(counter.0, messages)
}

/// Counts the encoded bytes one session projection adds to its snapshot.
///
/// # Errors
///
/// Returns the typed local encode failure when the projection cannot be encoded.
fn session_projection_bytes(projection: &SessionProjectionDto) -> DtoResult<usize> {
    let mut counter = CountingWriter(0);
    serde_json::to_writer(&mut counter, projection).map_err(|_| encode_failure())?;
    Ok(counter.0)
}

/// Counts the encoded bytes one transcript row adds to its snapshot.
///
/// # Errors
///
/// Returns the typed local encode failure when the row cannot be encoded.
fn encoded_message_bytes(message: &intention_proto::MessageProjectionDto) -> DtoResult<usize> {
    let mut counter = CountingWriter(0);
    serde_json::to_writer(&mut counter, message).map_err(|_| encode_failure())?;
    Ok(counter.0)
}

/// The one typed failure of a local protocol value that cannot be encoded.
fn encode_failure() -> ErrorDto {
    ErrorDto::validation(
        "local_protocol_encode_failed",
        "a typed local protocol message could not be encoded",
    )
}

pub fn now() -> DtoResult<TimestampDto> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| {
            ErrorDto::unavailable("daemon_clock_unavailable", "daemon clock is unavailable")
        })?
        .as_secs();
    TimestampDto::from_unix_seconds(i64::try_from(seconds).map_err(|_| {
        ErrorDto::unavailable("daemon_clock_unavailable", "daemon clock is unavailable")
    })?)
}

fn platform_database_location() -> DtoResult<PathBuf> {
    let base = platform_state_directory()?;
    fs::create_dir_all(&base).map_err(|_| unavailable_storage())?;
    Ok(base.join(DATABASE_FILENAME))
}

fn platform_state_directory() -> DtoResult<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|path| path.join(".local/state"))
            })
            .map(|path| path.join("intention-relay"))
            .ok_or_else(unavailable_storage)
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|path| path.join("Library/Application Support/intention-relay"))
            .ok_or_else(unavailable_storage)
    }
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(|path| path.join("intention-relay"))
            .ok_or_else(unavailable_storage)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err(unavailable_storage())
    }
}

fn unavailable_storage() -> ErrorDto {
    ErrorDto::unavailable(
        "daemon_storage_unavailable",
        "daemon durable storage is unavailable",
    )
}
#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Composition internals use controlled durable fixtures."
    )]

    use super::*;

    use intention_engine::{ModelRunCommitDto, RunCancellation};
    use intention_proto::{MessageKindDto, MessageProjectionDto, SendUserTurnCommandDto};
    use intention_providers::{ModelCancellationSignal, ModelCapabilitiesDto, ModelEventStream};
    use intention_storage::ToolResultStatusDto;
    use tempfile::TempDir;

    fn test_facade() -> (TempDir, DaemonApplicationFacade) {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test_support(
            directory.path().join("facade.sqlite"),
            fixture_document(),
        )
        .expect("durable facade opens");
        (directory, facade)
    }

    fn fixture_workspace_root() -> WorkspaceRootDto {
        WorkspaceRootDto::parse(
            std::env::temp_dir()
                .join("intention-composition-workspace")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("native fixture workspace is absolute")
    }

    /// The one catalog-document fixture every composition fixture opens from.
    ///
    /// It declares one executable OpenRouter profile with the exact openrouter
    /// kind, a bearer credential, and a declared exact-model capability subset
    /// inside the code-owned descriptor envelope.
    const FIXTURE_DOCUMENT: &str = "schema_version = 1\n\
        \n[provider]\n\
        context_window_tokens = 180000\n\
        default_profile = \"main\"\n\
        \n[providers.profiles.main]\n\
        kind = \"openrouter\"\n\
        model = \"fixture\"\n\
        credential = \"fixture-credential\"\n\
        display_name = \"Main\"\n\
        enabled = true\n\
        \n[providers.profiles.main.capabilities]\n\
        text_streaming = true\n\
        reasoning = \"disabled\"\n\
        tool_exchange = true\n";

    fn fixture_document() -> &'static str {
        FIXTURE_DOCUMENT
    }

    fn create(facade: &DaemonApplicationFacade, session_id: SessionId) {
        let accepted = facade
            .create_session(CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                fixture_workspace_root(),
                RunModeDto::Build,
            ))
            .expect("fixture session creates");
        assert!(matches!(accepted, ProtocolResultDto::SessionCreated(_)));
    }

    /// Creates a workspace directory containing one named file.
    fn workspace_fixture(name: &str, content: &str) -> (TempDir, WorkspaceRoot) {
        let directory = TempDir::new().expect("temporary directory exists");
        fs::write(directory.path().join(name), content).expect("workspace fixture writes");
        let root = WorkspaceRoot::resolve(
            &WorkspaceRootDto::parse(directory.path().to_string_lossy().into_owned())
                .expect("fixture workspace dto is absolute"),
        )
        .expect("fixture workspace resolves");
        (directory, root)
    }

    /// Builds the read input for the shared `hello.txt` workspace fixture.
    fn read_hello_input() -> ToolInput {
        ToolInput::Read(intention_tools::ReadInput {
            path: intention_proto::WorkspaceRelativePathDto::parse("hello.txt")
                .expect("fixture path is valid"),
        })
    }

    /// Starts one durable run through a direct user turn and returns its identity.
    fn started_run(facade: &DaemonApplicationFacade, session_id: SessionId, label: &str) -> RunId {
        let accepted = send_user_turn(facade, session_id, label).expect("fixture turn is accepted");
        let ProtocolResultDto::TurnAccepted(turn) = accepted else {
            unreachable!("fixture turn has user-turn evidence")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
            unreachable!("first fixture turn starts")
        };
        run_id
    }

    fn send_user_turn(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        content: &str,
    ) -> DtoResult<ProtocolResultDto> {
        facade.send_user_turn(
            SendUserTurnCommandDto::new(
                session_id,
                intention_proto::IdempotencyKey::new(),
                content,
            )
            .expect("fixture user turn is valid"),
        )
    }

    /// Reads the committed transcript rows of one run from the same repository
    /// read the run subscription snapshot uses.
    fn run_messages(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        run_id: RunId,
    ) -> Vec<intention_proto::MessageProjectionDto> {
        facade
            .inner
            .repository
            .load_run_messages(session_id, run_id, SESSION_SNAPSHOT_MESSAGES)
            .expect("committed run transcript reads")
    }

    /// Reads the durable tool-result evidence of one exact invocation.
    fn tool_evidence(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        run_id: RunId,
        call_id: intention_proto::ToolCallId,
    ) -> ToolResultEvidenceDto {
        facade
            .inner
            .repository
            .load_tool_result(session_id, run_id, call_id)
            .expect("committed tool evidence reads")
    }

    #[test]
    fn facade_replay_of_the_same_user_turn_returns_the_recorded_outcome() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test_support(
            directory.path().join("idempotent-turn.sqlite"),
            fixture_document(),
        )
        .expect("durable facade opens");
        let session_id = SessionId::new();
        create(&facade, session_id);
        let key = intention_proto::IdempotencyKey::new();
        let command = SendUserTurnCommandDto::new(session_id, key, "idempotent turn")
            .expect("fixture user turn is valid");

        let initial = facade
            .send_user_turn(command.clone())
            .expect("the first user turn is accepted");
        let ProtocolResultDto::TurnAccepted(initial_turn) = initial else {
            unreachable!("the first user turn returns user-turn evidence")
        };
        let SendUserTurnOutcomeDto::Started {
            run_id,
            config_revision_id,
        } = initial_turn.outcome()
        else {
            unreachable!("the first user turn starts a run")
        };
        let committed = run_messages(&facade, session_id, run_id);

        // The proposed run identity is derived from the caller's idempotency
        // key, so a repeated command proposes the identity the durable turn
        // already recorded and replays its outcome instead of conflicting.
        let replayed = facade
            .send_user_turn(command)
            .expect("a repeated command replays its recorded outcome");
        let ProtocolResultDto::TurnAccepted(replayed_turn) = replayed else {
            unreachable!("the replay returns user-turn evidence")
        };
        assert_eq!(
            replayed_turn.outcome(),
            SendUserTurnOutcomeDto::Started {
                run_id,
                config_revision_id,
            }
        );
        assert_eq!(
            run_messages(&facade, session_id, run_id),
            committed,
            "the replayed command duplicates no transcript row"
        );

        // The same key bound to different content stays a durable conflict.
        let conflicting = SendUserTurnCommandDto::new(session_id, key, "different turn")
            .expect("fixture user turn is valid");
        let error = facade
            .send_user_turn(conflicting)
            .expect_err("one key bound to different content conflicts");
        assert_eq!(error.code(), "turn_idempotency_conflict");
    }

    #[test]
    fn an_over_budget_newest_row_is_kept_and_the_projection_is_charged() {
        let row = |text: String| {
            MessageProjectionDto::new(
                SessionId::new(),
                None,
                MessageKindDto::User,
                text,
                None,
                None,
                None,
            )
            .expect("fixture transcript row is valid")
        };
        // A single row larger than the whole budget is kept: the transport owns
        // the envelope cap and answers with its typed over-size failure instead
        // of the peer receiving a valid but empty transcript.
        let oversized = row("x".repeat(MAX_TRANSCRIPT_SNAPSHOT_BYTES));
        let kept = bounded_snapshot_messages(0, vec![oversized.clone()])
            .expect("the over-budget snapshot bounds");
        assert_eq!(kept, vec![oversized]);

        // The projection travelling beside the transcript is charged against
        // the same budget, so it displaces the oldest rows first.
        let rows: Vec<MessageProjectionDto> = (0..4)
            .map(|index| row(format!("row {index} {}", "y".repeat(200 * 1024))))
            .collect();
        let unbounded =
            bounded_snapshot_messages(0, rows.clone()).expect("the uncharged read bounds");
        let charged =
            bounded_snapshot_messages(400 * 1024, rows.clone()).expect("the charged read bounds");
        assert!(charged.len() < unbounded.len());
        assert_eq!(
            charged.last().map(|message| message.text().to_owned()),
            rows.last().map(|message| message.text().to_owned()),
            "the newest row is always kept"
        );
    }

    #[test]
    fn provider_catalog_rejects_invalid_documents_without_secret_disclosure() {
        const SECRET: &str = "selected-provider-secret";
        let directory = TempDir::new().expect("temporary directory exists");
        let config_path = directory.path().join("catalog.toml");
        let document = FIXTURE_DOCUMENT.replace("fixture-credential", SECRET);
        let facade = DaemonApplicationFacade::open_for_test_support_with_file(
            directory.path().join("provider.sqlite"),
            &config_path,
            &document,
        )
        .expect("valid catalog composes");

        let status = facade
            .provider_catalog_status()
            .expect("catalog status reads");
        let encoded = serde_json::to_string(&status).expect("safe status serializes");
        assert!(!encoded.contains(SECRET));
        let page = facade
            .list_provider_catalog(ListProviderCatalogQueryDto::new(None).expect("empty query"))
            .expect("catalog page reads");
        let encoded = serde_json::to_string(&page).expect("safe page serializes");
        assert!(!encoded.contains(SECRET));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&config_path)
                .expect("fixture metadata reads")
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o077,
                0,
                "the private configuration file stays user-only"
            );
        }

        let invalid = directory.path().join("invalid.toml");
        let invalid_document = FIXTURE_DOCUMENT
            .replace("kind = \"openrouter\"", "kind = \"not-a-provider\"")
            .replace("fixture-credential", "invalid-provider-secret");
        let error = DaemonApplicationFacade::open_for_test_support_with_file(
            directory.path().join("invalid.sqlite"),
            &invalid,
            &invalid_document,
        )
        .map(|_facade| ())
        .expect_err("an unknown provider kind fails safely");
        assert_eq!(error.code(), "invalid_provider_kind");
        assert!(!error.to_string().contains("invalid-provider-secret"));
    }

    #[test]
    fn daemon_interrupt_ends_the_in_flight_execute_with_a_partial_result() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "in flight interrupt");
        let (workspace_directory, workspace) = workspace_fixture("keep.txt", "kept");
        let sentinel = workspace_directory.path().join("sentinel.txt");
        let call_id = intention_proto::ToolCallId::new();
        // The one per-run cancellation handle the daemon host registers is the
        // same handle the in-flight tool invocation observes.
        let cancellation = RunCancellation::new();
        let worker_cancellation = cancellation.clone();
        let worker_facade = facade.clone();
        let worker_session = session_id;
        let worker_run = run_id;

        let worker = std::thread::spawn(move || {
            ApplicationService::new(worker_facade.repository()).invoke_local_tool_with_publication(
                ToolInvocationRequestDto::new(
                    workspace,
                    worker_session,
                    worker_run,
                    call_id,
                    "execute",
                    ToolInput::Execute(intention_tools::ExecuteInput {
                        program: intention_tools::BoundedText::new(if cfg!(windows) {
                            "cmd"
                        } else {
                            "sh"
                        })
                        .expect("fixture program"),
                        args: if cfg!(windows) {
                            vec![
                                intention_tools::BoundedText::new("/C").expect("arg"),
                                intention_tools::BoundedText::new(
                                    "echo started> sentinel.txt & ping -n 2 127.0.0.1",
                                )
                                .expect("arg"),
                            ]
                        } else {
                            vec![
                                intention_tools::BoundedText::new("-c").expect("arg"),
                                intention_tools::BoundedText::new(
                                    "printf x > sentinel.txt; sleep 2",
                                )
                                .expect("arg"),
                            ]
                        },
                    }),
                    now().expect("fixture clock reads"),
                )
                .with_arguments_json("{}")
                .with_cancellation(worker_cancellation),
                &RecordingPublisher::new(),
            )
        });

        // The sentinel proves the child was spawned and running, so the
        // interrupt can only land while execution is in flight.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !sentinel.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "execute child never produced its start sentinel"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        cancellation.cancel();

        worker
            .join()
            .expect("worker completes")
            .expect("in-flight execution observes the interrupt as a partial outcome");
        let evidence = tool_evidence(&facade, session_id, run_id, call_id);
        assert_eq!(
            evidence.status(),
            ToolResultStatusDto::Partial,
            "the stopped execute commits its partial result"
        );
        assert_eq!(
            facade
                .session_snapshot(session_id)
                .expect("interrupted run reads")
                .projection()
                .active_run()
                .map(|run| run.run_id()),
            Some(run_id),
            "the interrupted run stays active for the continuing model loop"
        );
    }

    #[test]
    fn committed_tool_results_survive_a_late_interrupt_and_the_run_stays_active() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "late interrupt");
        let (_workspace_directory, workspace) = workspace_fixture("hello.txt", "hello");
        let cancellation = RunCancellation::new();

        let call_ids = [
            intention_proto::ToolCallId::new(),
            intention_proto::ToolCallId::new(),
        ];
        for call_id in call_ids {
            ApplicationService::new(facade.repository())
                .invoke_local_tool_with_publication(
                    ToolInvocationRequestDto::new(
                        workspace.clone(),
                        session_id,
                        run_id,
                        call_id,
                        "read",
                        read_hello_input(),
                        now().expect("fixture clock reads"),
                    )
                    .with_arguments_json("{}")
                    .with_cancellation(cancellation.clone()),
                    &RecordingPublisher::new(),
                )
                .expect("reads complete before any interrupt");
        }

        // The interrupt arrives after the effects committed: it never rewrites
        // completed evidence or terminalizes the run.
        cancellation.cancel();
        assert_eq!(
            facade
                .session_snapshot(session_id)
                .expect("the interrupted run stays readable")
                .projection()
                .active_run()
                .map(|run| run.run_id()),
            Some(run_id),
            "a late interrupt never rewrites completed evidence or terminalizes the run"
        );
        for call_id in call_ids {
            let evidence = tool_evidence(&facade, session_id, run_id, call_id);
            assert_eq!(evidence.status(), ToolResultStatusDto::Completed);
            assert_eq!(evidence.tool_id(), "read");
            assert_eq!(evidence.content(), "hello");
        }
    }

    #[test]
    fn committed_tool_rows_are_published_in_commit_order() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "publication path");
        let (_workspace_directory, workspace) = workspace_fixture("hello.txt", "hello");

        let call_id = intention_proto::ToolCallId::new();
        let publisher = RecordingPublisher::new();
        ApplicationService::new(facade.repository())
            .invoke_local_tool_with_publication(
                ToolInvocationRequestDto::new(
                    workspace,
                    session_id,
                    run_id,
                    call_id,
                    "read",
                    read_hello_input(),
                    now().expect("fixture clock reads"),
                )
                .with_arguments_json("{}"),
                &publisher,
            )
            .expect("read completes");

        // Every committed tool row reached the boundary in commit order, so a
        // live frame always carries the committed value of its own transaction
        // without any durable re-read.
        let published = publisher.published();
        assert_eq!(published.len(), 2);
        assert_eq!(published[0].kind(), MessageKindDto::ToolCall);
        assert_eq!(published[0].tool_call_id(), Some(call_id));
        assert_eq!(published[1].kind(), MessageKindDto::ToolResult);
        assert_eq!(published[1].tool_call_id(), Some(call_id));
        assert_eq!(published[1].text(), "hello");
        let committed: Vec<_> = run_messages(&facade, session_id, run_id)
            .into_iter()
            .filter(|message| message.tool_call_id() == Some(call_id))
            .collect();
        assert_eq!(
            published, committed,
            "published rows are the committed rows"
        );
    }

    /// Commits one fixture transcript row bound to the exact run.
    fn append_transcript_row(
        facade: &DaemonApplicationFacade,
        session_id: SessionId,
        run_id: RunId,
        text: &str,
    ) {
        let message = MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            text,
            None,
            None,
            None,
        )
        .expect("fixture transcript row is valid");
        facade
            .inner
            .repository
            .append_message(
                message,
                TimestampDto::from_unix_seconds(2).expect("fixture timestamp is valid"),
            )
            .expect("fixture transcript row commits");
    }

    #[test]
    fn snapshot_reads_stay_below_the_single_transport_envelope_cap() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let run_id = started_run(&facade, session_id, "snapshot bound");

        // A legitimate 256-row snapshot stays servable in full while it fits
        // the representation budget derived from the envelope cap.
        for index in 0..SESSION_SNAPSHOT_MESSAGES {
            append_transcript_row(
                &facade,
                session_id,
                run_id,
                &format!("row {index} {}", "s".repeat(1024)),
            );
        }
        let servable = facade
            .session_snapshot(session_id)
            .expect("the bounded snapshot reads");
        assert_eq!(
            servable.messages().len(),
            usize::try_from(SESSION_SNAPSHOT_MESSAGES).expect("the row bound fits a usize")
        );
        assert!(
            serde_json::to_vec(&servable)
                .expect("the bounded snapshot encodes")
                .len()
                <= intention_transport::MAX_MESSAGE_BYTES
        );

        // Rows whose combined encoding exceeds the budget keep the newest rows
        // that fit instead of producing an unservable snapshot.
        for index in 0..8 {
            append_transcript_row(
                &facade,
                session_id,
                run_id,
                &format!("big {index} {}", "x".repeat(256 * 1024)),
            );
        }
        let bounded = facade
            .session_snapshot(session_id)
            .expect("the byte-bounded snapshot reads");
        assert!(
            serde_json::to_vec(&bounded)
                .expect("the byte-bounded snapshot encodes")
                .len()
                <= intention_transport::MAX_MESSAGE_BYTES
        );
        assert!(bounded.messages().len() < 256);
        assert_eq!(
            bounded
                .messages()
                .last()
                .expect("the newest row stays")
                .text(),
            format!("big 7 {}", "x".repeat(256 * 1024))
        );
        assert!(
            bounded
                .messages()
                .iter()
                .all(|message| !message.text().starts_with("row 0 ")),
            "the oldest rows are dropped first"
        );
    }

    /// Records every committed transcript row handed to the commit sink.
    struct RecordingPublisher {
        publications: Mutex<Vec<MessageProjectionDto>>,
    }

    impl RecordingPublisher {
        fn new() -> Self {
            Self {
                publications: Mutex::new(Vec::new()),
            }
        }

        fn published(&self) -> Vec<MessageProjectionDto> {
            self.publications
                .lock()
                .expect("publication lock is available")
                .clone()
        }
    }

    impl ModelRunCommitObserver for RecordingPublisher {
        fn observe_model_run_commit(&self, committed: &ModelRunCommitDto) {
            if let ModelRunCommitDto::Content(message) = committed {
                self.publications
                    .lock()
                    .expect("publication lock is available")
                    .push(message.clone());
            }
        }
    }

    /// The one catalog document fixture that omits the accepted profile.
    const REMOVAL_DOCUMENT: &str = "schema_version = 1\n\
        \n[provider]\n\
        context_window_tokens = 180000\n\
        default_profile = \"extra\"\n\
        \n[providers.profiles.extra]\n\
        kind = \"openrouter\"\n\
        model = \"extra-model\"\n\
        credential = \"extra-credential\"\n\
        display_name = \"Extra\"\n\
        enabled = true\n\
        \n[providers.profiles.extra.capabilities]\n\
        text_streaming = true\n\
        reasoning = \"disabled\"\n\
        tool_exchange = true\n";

    fn fixture_profile_id() -> ProviderProfileId {
        ProviderProfileId::parse("main").expect("fixture profile id is valid")
    }

    fn fixture_now() -> TimestampDto {
        TimestampDto::from_unix_seconds(1).expect("fixture timestamp is valid")
    }

    fn fixture_operation_id() -> intention_proto::IdempotencyKey {
        intention_proto::IdempotencyKey::new()
    }

    /// Parses one credential-bearing catalog document into its safe document.
    fn parsed_document(text: &str) -> CatalogDocumentDto {
        CatalogCandidate::parse(RawConfigInputDto::new(text))
            .expect("fixture document parses")
            .safe_document()
            .clone()
    }

    /// Reads the current safe catalog status of one composition.
    fn catalog_status(facade: &DaemonApplicationFacade) -> ProviderCatalogStatusDto {
        let ProtocolResultDto::ProviderCatalogStatus(status) = facade
            .provider_catalog_status()
            .expect("catalog status reads")
        else {
            unreachable!("catalog status answers with its own result")
        };
        status
    }

    /// Opens one file-backed catalog fixture and returns its owned directory.
    fn file_facade(
        label: &str,
    ) -> (
        TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        DaemonApplicationFacade,
    ) {
        let directory = TempDir::new().expect("temporary directory exists");
        let database = directory.path().join(format!("{label}.sqlite"));
        let config_path = directory.path().join(format!("{label}.toml"));
        let facade = DaemonApplicationFacade::open_for_test_support_with_file(
            &database,
            &config_path,
            FIXTURE_DOCUMENT,
        )
        .expect("file-backed catalog opens");
        (directory, database, config_path, facade)
    }

    #[test]
    fn catalog_list_orders_entries_and_rejects_foreign_tokens() {
        let (_directory, facade) = test_facade();
        let ProtocolResultDto::ProviderCatalogPage(page) = facade
            .list_provider_catalog(ListProviderCatalogQueryDto::new(None).expect("empty query"))
            .expect("catalog page reads")
        else {
            unreachable!("catalog list answers with a page")
        };
        assert_eq!(page.entries().len(), 1);
        assert_eq!(page.entries()[0].profile_id(), &fixture_profile_id());
        assert!(page.entries()[0].credential_configured());
        assert_eq!(
            page.entries()[0].readiness(),
            ProviderProfileReadinessDto::Ready
        );
        assert!(!page.has_more());
        assert!(page.next_page_token().is_none());
        assert_eq!(
            page.catalog_revision_id(),
            catalog_status(&facade).active_catalog_revision_id()
        );

        let malformed = facade
            .list_provider_catalog(
                ListProviderCatalogQueryDto::new(Some("not-a-uuid:main".to_owned()))
                    .expect("token shape is safe"),
            )
            .expect_err("a malformed token fails");
        assert_eq!(malformed.code(), "provider_catalog_page_token_invalid");

        let foreign = format!("{}:main", CatalogRevisionId::new());
        let error = facade
            .list_provider_catalog(
                ListProviderCatalogQueryDto::new(Some(foreign)).expect("token shape is safe"),
            )
            .expect_err("a foreign revision token fails");
        assert_eq!(error.code(), "provider_catalog_changed");
    }

    #[test]
    fn a_pending_removal_degrades_read_only_until_it_is_accepted() {
        let (_directory, database, config_path, facade) = file_facade("removal-accept");
        let session_id = SessionId::new();
        create(&facade, session_id);
        drop(facade);

        let facade = DaemonApplicationFacade::open_for_test_support_with_file(
            &database,
            &config_path,
            REMOVAL_DOCUMENT,
        )
        .expect("the removal candidate opens degraded");
        let status = catalog_status(&facade);
        assert_eq!(
            status.activation_state(),
            ProviderCatalogActivationStateDto::PendingRemoval
        );
        assert_eq!(
            status.degraded_reason(),
            Some(ProviderCatalogDegradedReasonDto::RemovalCandidatePending)
        );
        let handle = status
            .candidate()
            .expect("a pending removal names its handle");

        let refused = facade
            .send_user_turn(
                SendUserTurnCommandDto::new(session_id, fixture_operation_id(), "degraded turn")
                    .expect("turn shape is valid"),
            )
            .expect_err("admission is refused while degraded");
        assert_eq!(refused.code(), "execution_not_ready");

        let accepted = facade
            .accept_provider_catalog_removal(AcceptProviderCatalogRemovalCommandDto::new(
                handle,
                fixture_operation_id(),
            ))
            .expect("the pending removal accepts");
        assert!(matches!(
            accepted,
            ProtocolResultDto::ProviderCatalogRemovalAccepted(_)
        ));
        let status = catalog_status(&facade);
        assert_eq!(
            status.activation_state(),
            ProviderCatalogActivationStateDto::Active
        );
        assert_eq!(status.degraded_reason(), None);
        let accepted = facade
            .send_user_turn(
                SendUserTurnCommandDto::new(session_id, fixture_operation_id(), "fresh turn")
                    .expect("turn shape is valid"),
            )
            .expect("admission resumes after the removal is accepted");
        assert!(matches!(accepted, ProtocolResultDto::TurnAccepted(_)));
    }

    #[test]
    fn a_rejected_removal_candidate_leaves_the_daemon_degraded() {
        let (_directory, database, config_path, facade) = file_facade("removal-reject");
        let session_id = SessionId::new();
        create(&facade, session_id);
        drop(facade);

        let facade = DaemonApplicationFacade::open_for_test_support_with_file(
            &database,
            &config_path,
            REMOVAL_DOCUMENT,
        )
        .expect("the removal candidate opens degraded");
        let handle = catalog_status(&facade)
            .candidate()
            .expect("a pending removal names its handle");
        let rejected = facade
            .reject_provider_catalog_candidate(RejectProviderCatalogCandidateCommandDto::new(
                handle,
                fixture_operation_id(),
            ))
            .expect("the pending candidate rejects");
        assert!(matches!(
            rejected,
            ProtocolResultDto::ProviderCatalogCandidateRejected(_)
        ));
        let status = catalog_status(&facade);
        assert_eq!(
            status.activation_state(),
            ProviderCatalogActivationStateDto::Active
        );
        assert_eq!(
            status.degraded_reason(),
            Some(ProviderCatalogDegradedReasonDto::RemovalCandidateRejected)
        );
        let refused = facade
            .send_user_turn(
                SendUserTurnCommandDto::new(session_id, fixture_operation_id(), "rejected turn")
                    .expect("turn shape is valid"),
            )
            .expect_err("a rejected candidate keeps the daemon read-only");
        assert_eq!(refused.code(), "execution_not_ready");
    }

    #[test]
    fn crash_after_acceptance_requires_the_exact_accepted_catalog() {
        let (_directory, database, config_path, facade) = file_facade("activation-recovery");
        let session_id = SessionId::new();
        create(&facade, session_id);
        // Simulate the crash window: a different revision is durably accepted
        // and never activated, so the accepted pointer differs from the active one.
        let changed = FIXTURE_DOCUMENT.replace("model = \"fixture\"", "model = \"changed\"");
        let prepared =
            engine_api::prepare_catalog_candidate(&parsed_document(&changed), None, fixture_now())
                .expect("the changed catalog prepares");
        facade
            .repository()
            .accept_catalog_revision(prepared.revision)
            .expect("acceptance commits without activation");
        drop(facade);

        // The exact accepted document recovers the exact revision.
        let facade = DaemonApplicationFacade::open_for_test_support_with_file(
            &database,
            &config_path,
            &changed,
        )
        .expect("the exact accepted catalog recovers");
        let status = catalog_status(&facade);
        assert_eq!(
            status.activation_state(),
            ProviderCatalogActivationStateDto::Active
        );
        assert_eq!(status.degraded_reason(), None);

        // A second crash window stays open while a changed file is refused.
        let third = changed.replace("model = \"changed\"", "model = \"third\"");
        let prepared =
            engine_api::prepare_catalog_candidate(&parsed_document(&third), None, fixture_now())
                .expect("the third catalog prepares");
        facade
            .repository()
            .accept_catalog_revision(prepared.revision)
            .expect("the second acceptance commits without activation");
        drop(facade);

        let other = third.replace("model = \"third\"", "model = \"other\"");
        let facade = DaemonApplicationFacade::open_for_test_support_with_file(
            &database,
            &config_path,
            &other,
        )
        .expect("a changed file opens degraded read-only");
        let status = catalog_status(&facade);
        assert_eq!(
            status.activation_state(),
            ProviderCatalogActivationStateDto::ActivationRecoveryRequired
        );
        assert_eq!(
            status.degraded_reason(),
            Some(ProviderCatalogDegradedReasonDto::ActivationRecoveryRequired)
        );
        let refused = facade
            .send_user_turn(
                SendUserTurnCommandDto::new(session_id, fixture_operation_id(), "recovering")
                    .expect("turn shape is valid"),
            )
            .expect_err("admission is refused while recovery is required");
        assert_eq!(refused.code(), "execution_not_ready");
        drop(facade);

        // The exact accepted document still recovers after a second crash.
        let facade = DaemonApplicationFacade::open_for_test_support_with_file(
            &database,
            &config_path,
            &third,
        )
        .expect("the exact accepted catalog recovers again");
        let status = catalog_status(&facade);
        assert_eq!(
            status.activation_state(),
            ProviderCatalogActivationStateDto::Active
        );
        assert_eq!(status.degraded_reason(), None);
        let accepted = facade
            .send_user_turn(
                SendUserTurnCommandDto::new(session_id, fixture_operation_id(), "recovered")
                    .expect("turn shape is valid"),
            )
            .expect("admission resumes after exact recovery");
        assert!(matches!(accepted, ProtocolResultDto::TurnAccepted(_)));
    }

    #[test]
    fn reload_commits_a_context_window_change_and_rejects_catalog_changes() {
        let (_directory, _database, config_path, facade) = file_facade("reload");
        let ProtocolResultDto::ConfigurationReloaded(equal) = facade
            .reload_configuration(ReloadConfigurationCommandDto::new(fixture_operation_id()))
            .expect("an equal reload succeeds")
        else {
            unreachable!("reload answers with its acceptance")
        };
        let first_revision = equal.config_revision_id();

        let widened = FIXTURE_DOCUMENT.replace("180000", "240000");
        fs::write(&config_path, &widened).expect("fixture file writes");
        let ProtocolResultDto::ConfigurationReloaded(changed) = facade
            .reload_configuration(ReloadConfigurationCommandDto::new(fixture_operation_id()))
            .expect("a context-window reload succeeds")
        else {
            unreachable!("reload answers with its acceptance")
        };
        assert_ne!(changed.config_revision_id(), first_revision);
        assert_eq!(
            changed.catalog_revision_id(),
            catalog_status(&facade).active_catalog_revision_id()
        );

        let catalog_changed = widened.replace("model = \"fixture\"", "model = \"changed\"");
        fs::write(&config_path, &catalog_changed).expect("fixture file writes");
        let error = facade
            .reload_configuration(ReloadConfigurationCommandDto::new(fixture_operation_id()))
            .expect_err("a catalog-affecting reload is refused");
        assert_eq!(error.code(), "catalog_change_requires_restart");
    }

    #[test]
    fn rotation_replaces_the_private_credential_and_requires_frozen_meaning() {
        let (_directory, _database, config_path, facade) = file_facade("rotation");
        fs::write(
            &config_path,
            FIXTURE_DOCUMENT.replace("fixture-credential", "rotated-credential"),
        )
        .expect("fixture file writes");
        let accepted = facade
            .rotate_provider_credential(RotateProviderCredentialCommandDto::new(
                fixture_profile_id(),
                fixture_operation_id(),
            ))
            .expect("a credential-only rotation succeeds");
        assert!(matches!(
            accepted,
            ProtocolResultDto::ProviderCredentialRotated(_)
        ));

        fs::write(
            &config_path,
            FIXTURE_DOCUMENT
                .replace("fixture-credential", "rotated-credential")
                .replace("model = \"fixture\"", "model = \"changed\""),
        )
        .expect("fixture file writes");
        let error = facade
            .rotate_provider_credential(RotateProviderCredentialCommandDto::new(
                fixture_profile_id(),
                fixture_operation_id(),
            ))
            .expect_err("a changed frozen meaning refuses rotation");
        assert_eq!(error.code(), "credential_rotation_frozen_meaning_mismatch");

        let (_memory_directory, memory_facade) = test_facade();
        let error = memory_facade
            .rotate_provider_credential(RotateProviderCredentialCommandDto::new(
                fixture_profile_id(),
                fixture_operation_id(),
            ))
            .expect_err("an in-memory host has no file-backed credential source");
        assert_eq!(error.code(), "credential_rotation_source_unavailable");
    }

    #[test]
    fn configuration_edits_write_the_file_and_never_echo_the_credential() {
        let (_directory, _database, config_path, facade) = file_facade("edits");
        let edits = ApplyConfigurationEditsCommandDto::new(
            vec![
                intention_proto::provider::ConfigurationEditDto::set_profile_display_name(
                    fixture_profile_id(),
                    "Renamed",
                )
                .expect("the edit is valid"),
            ],
            fixture_operation_id(),
        )
        .expect("the edit command is valid");
        let accepted = facade
            .apply_configuration_edits(edits)
            .expect("the typed edit applies");
        assert!(matches!(
            accepted,
            ProtocolResultDto::ConfigurationEditsApplied(_)
        ));
        assert!(
            !serde_json::to_string(&accepted)
                .expect("safe acceptance serializes")
                .contains("fixture-credential")
        );
        let written = fs::read_to_string(&config_path).expect("the configuration file reads");
        assert!(written.contains("display_name = \"Renamed\""));
        assert!(
            written.contains("fixture-credential"),
            "the retained private credential is restored inside the private boundary"
        );

        let error = facade
            .apply_configuration_document(
                ApplyConfigurationDocumentCommandDto::new(FIXTURE_DOCUMENT, fixture_operation_id())
                    .expect("the command shape is valid"),
            )
            .expect_err("a credential-bearing document is refused");
        assert_eq!(error.code(), "configuration_edit_contains_credential");

        let credential_free = FIXTURE_DOCUMENT.replace("credential = \"fixture-credential\"\n", "");
        let accepted = facade
            .apply_configuration_document(
                ApplyConfigurationDocumentCommandDto::new(credential_free, fixture_operation_id())
                    .expect("the command shape is valid"),
            )
            .expect("a credential-free document applies");
        assert!(matches!(
            accepted,
            ProtocolResultDto::ConfigurationDocumentApplied(_)
        ));
    }

    #[test]
    fn a_session_default_change_publishes_one_typed_event() {
        let (_directory, facade) = test_facade();
        let session_id = SessionId::new();
        create(&facade, session_id);
        let revision = facade
            .repository()
            .load_session_projection(session_id)
            .expect("the session projection reads")
            .session_projection_revision();
        let command = SetSessionProviderProfileCommandDto::new(
            session_id,
            fixture_profile_id(),
            revision,
            fixture_operation_id(),
        );
        let accepted = facade
            .set_session_provider_profile(command)
            .expect("the session default changes");
        let ProtocolResultDto::SessionProviderProfileSet(accepted) = accepted else {
            unreachable!("the session default answers with its acceptance")
        };
        assert!(accepted.changed());
        let events = facade.recorded_session_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].session_id(), session_id);
        assert_eq!(events[0].profile_id(), &fixture_profile_id());

        // Setting the same profile again is a no-change success with no event.
        let revision = facade
            .repository()
            .load_session_projection(session_id)
            .expect("the session projection reads")
            .session_projection_revision();
        let command = SetSessionProviderProfileCommandDto::new(
            session_id,
            fixture_profile_id(),
            revision,
            fixture_operation_id(),
        );
        let ProtocolResultDto::SessionProviderProfileSet(accepted) = facade
            .set_session_provider_profile(command)
            .expect("the repeated session default succeeds")
        else {
            unreachable!("the session default answers with its acceptance")
        };
        assert!(!accepted.changed());
        assert_eq!(facade.recorded_session_events().len(), 1);
    }

    #[test]
    fn a_persisted_selection_never_resolves_a_driver_after_its_revision_is_gone() {
        let (_directory, database, config_path, facade) = file_facade("stale-selection");
        let session_id = SessionId::new();
        create(&facade, session_id);
        let accepted = facade
            .send_user_turn(
                SendUserTurnCommandDto::new(session_id, fixture_operation_id(), "fixture turn")
                    .expect("turn shape is valid"),
            )
            .expect("the fixture turn is accepted");
        let ProtocolResultDto::TurnAccepted(turn) = accepted else {
            unreachable!("an accepted turn answers with its acceptance")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = turn.outcome() else {
            unreachable!("the first fixture turn starts a run")
        };
        let selection = facade
            .repository()
            .load_run_provider_selection(session_id, run_id)
            .expect("the run records its exact selection");
        assert!(
            facade.resolve_driver_for_run(&selection).is_some(),
            "an admitted selection has a live private entry"
        );
        drop(facade);

        // A changed model gives the profile a new revision identity, so the run's
        // own persisted selection never resolves another profile's live driver.
        let changed = FIXTURE_DOCUMENT.replace("model = \"fixture\"", "model = \"changed\"");
        let facade = DaemonApplicationFacade::open_for_test_support_with_file(
            &database,
            &config_path,
            &changed,
        )
        .expect("the changed catalog activates");
        let persisted = facade
            .repository()
            .load_run_provider_selection(session_id, run_id)
            .expect("the run keeps its persisted selection");
        assert_eq!(persisted, selection);
        assert!(
            facade.resolve_driver_for_run(&persisted).is_none(),
            "a stale selection never resolves a live driver"
        );
        // The exact starting run was recovered as interrupted by the next open,
        // so execution admission can never reach a provider for it.
        assert_eq!(
            facade
                .repository()
                .load_run_projection(session_id, run_id)
                .expect("the recovered run reads back")
                .status(),
            intention_proto::RunStatusDto::Interrupted
        );
    }

    #[test]
    fn an_edited_document_restores_the_credential_of_a_quoted_profile_key() {
        let (_directory, database, config_path, facade) = file_facade("quoted-profile");
        let quoted = FIXTURE_DOCUMENT
            .replace(
                "[providers.profiles.main]",
                "[providers.profiles.\"main.v2\"]",
            )
            .replace(
                "[providers.profiles.main.capabilities]",
                "[providers.profiles.\"main.v2\".capabilities]",
            )
            .replace(
                "default_profile = \"main\"",
                "default_profile = \"main.v2\"",
            );
        drop(facade);

        // A profile identity that needs a quoted TOML key keeps its credential
        // through the same credential-free render and private restore path.
        let facade = DaemonApplicationFacade::open_for_test_support_with_file(
            &database,
            &config_path,
            &quoted,
        )
        .expect("the quoted-key catalog opens");
        let edits = ApplyConfigurationEditsCommandDto::new(
            vec![
                intention_proto::provider::ConfigurationEditDto::set_profile_display_name(
                    ProviderProfileId::parse("main.v2").expect("fixture profile id is valid"),
                    "Renamed",
                )
                .expect("the edit is valid"),
            ],
            fixture_operation_id(),
        )
        .expect("the edit command is valid");
        let accepted = facade
            .apply_configuration_edits(edits)
            .expect("the quoted-key edit applies");
        assert!(matches!(
            accepted,
            ProtocolResultDto::ConfigurationEditsApplied(_)
        ));
        let written = fs::read_to_string(&config_path).expect("the configuration file reads");
        assert!(written.contains("[providers.profiles.\"main.v2\"]"));
        assert!(written.contains("display_name = \"Renamed\""));
        assert!(
            written.contains("credential = \"fixture-credential\""),
            "the quoted-key profile keeps its private credential"
        );
    }

    /// The composed probe driver of the health and discovery fixtures.
    struct ProbeDriver;

    fn probe_health() -> DtoResult<intention_proto::provider::ProviderHealthEvidenceDto> {
        intention_proto::provider::ProviderHealthEvidenceDto::new(
            ProviderProfileId::parse("main").expect("fixture profile id is valid"),
            intention_proto::provider::ProviderHealthStateDto::Available,
            None,
        )
    }

    fn probe_models() -> DtoResult<Vec<intention_proto::provider::ProviderModelRecordDto>> {
        Ok(vec![
            intention_proto::provider::ProviderModelRecordDto::new(
                "probe-model",
                Some("Probe Model".to_owned()),
            )?,
        ])
    }

    impl ModelExecutionDriver for ProbeDriver {
        fn capabilities(&self) -> ModelCapabilitiesDto {
            ModelCapabilitiesDto::new(true, true, true, false, false, true)
        }

        fn execute(
            &self,
            _request: intention_providers::ModelRequestDto,
            _cancellation: ModelCancellationSignal,
        ) -> ModelEventStream {
            Box::pin(futures_util::stream::empty())
        }

        fn health_probe(
            &self,
        ) -> intention_providers::ProviderProbeFuture<
            intention_proto::provider::ProviderHealthEvidenceDto,
        > {
            Box::pin(async { probe_health() })
        }

        fn list_models(
            &self,
        ) -> intention_providers::ProviderProbeFuture<
            Vec<intention_proto::provider::ProviderModelRecordDto>,
        > {
            Box::pin(async { probe_models() })
        }
    }

    #[tokio::test]
    async fn health_and_discovery_report_typed_non_authorizing_evidence() {
        let directory = TempDir::new().expect("temporary directory exists");
        let facade = DaemonApplicationFacade::open_for_test_support_with_driver(
            directory.path().join("probe.sqlite"),
            FIXTURE_DOCUMENT,
            Arc::new(ProbeDriver),
        )
        .expect("the probe host opens");

        let evidence = facade
            .check_provider_health(CheckProviderHealthCommandDto::new(fixture_profile_id()))
            .await
            .expect("the health probe answers");
        let ProtocolResultDto::ProviderHealth(evidence) = evidence else {
            unreachable!("a health probe answers with evidence")
        };
        assert_eq!(evidence.provider_id(), &fixture_profile_id());
        assert_eq!(
            evidence.state(),
            intention_proto::provider::ProviderHealthStateDto::Available
        );
        assert_eq!(evidence.reason(), None);

        let discovered = facade
            .discover_provider_models(DiscoverProviderModelsCommandDto::new(fixture_profile_id()))
            .await
            .expect("the discovery attempt answers");
        let ProtocolResultDto::ProviderModelsDiscovered(discovered) = discovered else {
            unreachable!("a discovery attempt answers with its result")
        };
        assert_eq!(discovered.records().len(), 1);
        assert_eq!(discovered.records()[0].model_id(), "probe-model");

        // Neither probe is authoritative: no run, selection, or session state
        // changes, and discovery never continues on its own.
        let session_id = SessionId::new();
        create(&facade, session_id);
        assert!(
            facade
                .repository()
                .load_session_projection(session_id)
                .expect("the session projection reads")
                .active_run()
                .is_none()
        );
    }
}
