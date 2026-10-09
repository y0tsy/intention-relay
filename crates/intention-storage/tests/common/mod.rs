#![allow(
    clippy::expect_used,
    reason = "Shared storage fixtures use expect for precise test diagnostics."
)]

use intention_proto::provider::{
    CatalogRevisionId, ContextPreservationCapabilityDto, CredentialTransportContractDto,
    CredentialTransportDto, CredentialTransportModeDto, LoopbackPolicyDto, ModelCapabilitySetV1,
    ModelCapabilityTaxonomyVersionDto, ModelInputKindDto, ProviderCapabilityAvailabilityDto,
    ProviderDriverContractRevisionDto, ProviderEndpointPolicyDto, ProviderExecutionPolicyDto,
    ProviderKindDescriptorRevisionId, ProviderKindDescriptorRevisionV1, ProviderKindId,
    ProviderProfileId, ProviderProfilePolicyDto, ProviderProfileRevisionId,
    ProviderProfileRevisionV1, ProviderSelectionSourceDto, ReasoningCapabilityDto,
    ReasoningEffortLevelDto, ReasoningFragmentCategoryDto, ReasoningHistoryTransferDto,
    ResolvedReasoningPolicyDto, ResolvedRunProviderSelectionDto, ToolExchangeCapabilityDto,
};
use intention_proto::{
    CreateSessionCommandDto, ProjectId, RunModeDto, SessionId, TimestampDto, WorkspaceId,
    WorkspaceRootDto,
};
use intention_storage::{
    ProviderCatalogRevisionDto, ProviderProfilePolicyEntryDto, SqliteDatabaseLocationDto,
    SqliteStorageRepository, StorageRepositoryDto,
};
use tempfile::TempDir;

/// Returns one fixture timestamp in whole unix seconds.
pub fn time(value: i64) -> TimestampDto {
    TimestampDto::from_unix_seconds(value).expect("fixture time is valid")
}

/// Returns one native fixture workspace root labeled `label`.
pub fn workspace_root(label: &str) -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-storage-tests")
            .join(label)
            .to_string_lossy()
            .into_owned(),
    )
    .expect("native fixture workspace is valid")
}

/// Opens the suite's SQLite database inside one temporary directory.
pub fn open(directory: &TempDir) -> SqliteStorageRepository {
    SqliteStorageRepository::open(
        SqliteDatabaseLocationDto::new(
            directory
                .path()
                .join("storage.sqlite")
                .to_string_lossy()
                .into_owned(),
        )
        .expect("temp location is absolute"),
    )
    .expect("database opens")
}

/// Reopens the suite's SQLite database after the repository was dropped.
pub fn reopen(directory: &TempDir) -> SqliteStorageRepository {
    open(directory)
}

/// Creates one temporary directory together with its open repository.
pub fn repository() -> (TempDir, SqliteStorageRepository) {
    let directory = TempDir::new().expect("temporary directory exists");
    let store = open(&directory);
    (directory, store)
}

/// Creates one durable session whose workspace root is labeled `label`.
pub fn create_session(repository: &SqliteStorageRepository, label: &str) -> SessionId {
    let session_id = SessionId::new();
    repository
        .create_session(
            CreateSessionCommandDto::new(
                ProjectId::new(),
                session_id,
                WorkspaceId::new(),
                workspace_root(label),
                RunModeDto::Build,
            ),
            time(1),
        )
        .expect("session creates");
    session_id
}

/// Returns the suite's fixture profile identity.
pub fn profile_id(label: &str) -> ProviderProfileId {
    ProviderProfileId::parse(label).expect("fixture profile identity is valid")
}

/// Returns the suite's fixture provider kind identity.
pub fn kind_id() -> ProviderKindId {
    ProviderKindId::parse("openrouter").expect("fixture kind identity is valid")
}

/// Returns the suite's fixture cross-turn transfer contract.
pub fn transfer() -> ReasoningHistoryTransferDto {
    ReasoningHistoryTransferDto::textual_history_v1("fixture-compatibility-v1")
        .expect("fixture transfer contract is valid")
}

/// Returns the suite's fixture declared model capability subset.
pub fn capability_subset() -> ModelCapabilitySetV1 {
    ModelCapabilitySetV1::new(
        ModelCapabilityTaxonomyVersionDto::current(),
        ModelInputKindDto::TextOnly,
        ProviderCapabilityAvailabilityDto::Enabled,
        ProviderCapabilityAvailabilityDto::Disabled,
        ReasoningCapabilityDto::textual_reasoning_v1(vec![ReasoningEffortLevelDto::Medium], true)
            .expect("fixture reasoning capability is valid"),
        ToolExchangeCapabilityDto::model_tool_loop_v1("fixture-tool-loop-v1")
            .expect("fixture tool loop is valid"),
        ContextPreservationCapabilityDto::local_durable_history_v1(transfer()),
    )
    .expect("fixture capability subset is valid")
}

/// Returns the suite's fixture resolved reasoning policy.
pub fn reasoning_policy() -> ResolvedReasoningPolicyDto {
    ResolvedReasoningPolicyDto::new(
        Some(ReasoningEffortLevelDto::Medium),
        true,
        transfer(),
        vec![ReasoningFragmentCategoryDto::Primary],
    )
    .expect("fixture reasoning policy is valid")
}

/// Returns the suite's fixture execution policy.
pub fn execution_policy() -> ProviderExecutionPolicyDto {
    ProviderExecutionPolicyDto::new(30, 2).expect("fixture execution policy is valid")
}

/// Returns one fixture profile revision labeled `label`.
pub fn profile_revision(label: &str) -> ProviderProfileRevisionV1 {
    ProviderProfileRevisionV1::new(
        profile_id(label),
        ProviderProfileRevisionId::new(),
        kind_id(),
        intention_proto::provider::ProviderKindDescriptorRevisionId::new(),
        format!("fixture-model-{label}"),
        None,
        CredentialTransportDto::bearer(),
        capability_subset(),
        reasoning_policy(),
        execution_policy(),
        LoopbackPolicyDto::NotApplicable,
    )
    .expect("fixture profile revision is valid")
}

/// Returns the suite's fixture driver contract revision.
pub fn driver_contract() -> ProviderDriverContractRevisionDto {
    ProviderDriverContractRevisionDto::new("fixture-driver", 1, 0)
        .expect("fixture driver contract is valid")
}

/// Returns one valid resolved provider selection labeled `label`.
pub fn selection(label: &str) -> ResolvedRunProviderSelectionDto {
    ResolvedRunProviderSelectionDto::from_profile_revision(
        &profile_revision(label),
        driver_contract(),
        ProviderSelectionSourceDto::GlobalDefault,
    )
}

/// Returns the suite's fixture endpoint policy.
pub const fn endpoint_policy() -> ProviderEndpointPolicyDto {
    ProviderEndpointPolicyDto::new(false, true, false)
}

/// Returns the suite's fixture credential transport contract.
pub fn credential_transport_contract() -> CredentialTransportContractDto {
    CredentialTransportContractDto::new(vec![CredentialTransportModeDto::Bearer], None)
        .expect("fixture credential transport contract is valid")
}

/// Returns one fixture kind descriptor revision with the supplied identity.
pub fn kind_descriptor(
    descriptor_revision_id: ProviderKindDescriptorRevisionId,
    descriptor_family: &str,
) -> ProviderKindDescriptorRevisionV1 {
    ProviderKindDescriptorRevisionV1::new(
        kind_id(),
        descriptor_revision_id,
        descriptor_family.to_owned(),
        vec!["fixture-part-v1".to_owned()],
        endpoint_policy(),
        credential_transport_contract(),
        capability_subset(),
        "fixture-driver",
    )
    .expect("fixture kind descriptor revision is valid")
}

/// Returns one fixture profile revision with the supplied descriptor identity.
pub fn profile_revision_with_descriptor(
    label: &str,
    descriptor_revision_id: ProviderKindDescriptorRevisionId,
) -> ProviderProfileRevisionV1 {
    ProviderProfileRevisionV1::new(
        profile_id(label),
        ProviderProfileRevisionId::new(),
        kind_id(),
        descriptor_revision_id,
        format!("fixture-model-{label}"),
        None,
        CredentialTransportDto::bearer(),
        capability_subset(),
        reasoning_policy(),
        execution_policy(),
        LoopbackPolicyDto::NotApplicable,
    )
    .expect("fixture profile revision is valid")
}

/// Returns the suite's fixture profile policy.
pub fn profile_policy(display_name: &str, enabled: bool) -> ProviderProfilePolicyDto {
    ProviderProfilePolicyDto::new(display_name, enabled, None)
        .expect("fixture profile policy is valid")
}

/// Returns one coherent fixture catalog revision with one kind and one profile.
pub fn catalog_revision(
    label: &str,
    catalog_revision_id: CatalogRevisionId,
) -> ProviderCatalogRevisionDto {
    let descriptor_revision_id = ProviderKindDescriptorRevisionId::new();
    ProviderCatalogRevisionDto::new(
        catalog_revision_id,
        Some(profile_id(label)),
        time(2),
        vec![kind_descriptor(descriptor_revision_id, "fixture-family")],
        vec![profile_revision_with_descriptor(
            label,
            descriptor_revision_id,
        )],
        vec![ProviderProfilePolicyEntryDto::new(
            profile_id(label),
            profile_policy("Fixture Profile", true),
        )],
    )
    .expect("fixture catalog revision is valid")
}

/// Returns one fixture catalog revision that declares no members.
pub fn empty_catalog_revision(
    catalog_revision_id: CatalogRevisionId,
) -> ProviderCatalogRevisionDto {
    ProviderCatalogRevisionDto::new(
        catalog_revision_id,
        None,
        time(3),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect("empty fixture catalog revision is valid")
}
