//! Slice 2 provider-catalog document parsing, validation, comparison, and editing.
//!
//! The catalog document is the one live TOML shape, evolved in place under
//! `schema_version = 1`: a global `[provider]` policy, `[providers.profiles.<id>]`
//! declarations, and optional `[providers.kinds.<id>]` user-kind declarations.
//! One parse validates the whole document once and yields a credential-free
//! [`CatalogDocumentDto`] plus opaque private credential material held by
//! [`CatalogCredentialMaterial`].
//!
//! Layering note (recorded deviation from the lane brief, confirmed against
//! architecture 22): this crate owns the validated credential-free
//! *declarations* (kind id, model id, normalized endpoint, credential transport,
//! declared capability subset, reasoning-effort selection, execution and loopback
//! policy). It does not assemble `ProviderProfileRevisionV1` /
//! `ProviderKindDescriptorRevisionV1` values: first-party descriptor content,
//! descriptor revision identities, and the code-owned capability-compatibility
//! matrix belong to `intention-providers` and the catalog builder, and this crate
//! cannot depend on those crates. `intention-daemon` later consumes the language
//! boundary, and `intention-engine`'s catalog path turns declarations into the
//! shared proto revisions.
//!
//! Endpoint law: an endpoint is accepted only as an absolute URL with a non-root
//! API base path. `https` accepts a validated lowercase DNS name or IPv4 literal;
//! `http` is accepted only for exactly `localhost`, `127.0.0.1`, or `[::1]` and
//! yields the explicit loopback policy from the explicit `http://` spelling,
//! because the frozen v1 profile shape has no separate loopback flag. Userinfo,
//! query, fragment, controls, backslashes, malformed percent escapes, non-canonical
//! hosts (uppercase, aliases, leading-zero or out-of-range IPv4, other IPv6
//! literals, and inet_aton-style numeric tails such as `127.1` or `0x7f.0.0.1`),
//! root paths, and `.`/`..`/empty path segments fail closed.
//!
//! Credential transport has one closed document spelling everywhere:
//! `credential_transport = "bearer"` or
//! `credential_transport = { header = "<token>" }`, defaulting to bearer when the
//! key is omitted (profile and kind sections alike). Because kind sections mirror
//! `UserKindCompositionDto`'s closed parts, the shared
//! `{ mode = "...", safe_header_name = "..." }` shape is accepted there as well and
//! normalized to the same validated declaration; every other spelling, unknown key,
//! and inconsistent pairing (a bearer carrying a header name) fails closed. The
//! parse always normalizes into the shared `CredentialTransportDto` wire shape, and
//! rendering emits the document spelling, so round-trips are exact and the shared
//! DTO tags never leak into a rendered document.
//!
//! Closed document codes introduced here (validation category): `invalid_provider_kind`,
//! `invalid_provider_profile_id`, `invalid_provider_default_profile`,
//! `missing_provider_profile`, `invalid_provider_display_name`,
//! `invalid_provider_credential_transport`, `invalid_provider_reasoning_effort`,
//! `invalid_provider_capabilities`, `invalid_configuration_edit`, plus the frozen
//! codes `configuration_edit_contains_credential` and
//! `catalog_change_requires_restart`. Existing codes (`invalid_config_toml`,
//! `invalid_config_schema`, `unsupported_config_schema_version`,
//! `missing_provider_credential`, `invalid_provider_model`,
//! `invalid_provider_endpoint`, `invalid_provider_attempt_timeout_seconds`,
//! `invalid_provider_max_attempts`, `invalid_provider_context_window_tokens`,
//! `invalid_provider_pricing_policy`) keep their meaning.
//!
//! Reload classification: profile-revision meaning (kind, model, endpoint,
//! credential transport, capability subset, reasoning/execution/loopback policy)
//! and the kind/profile sets are catalog-affecting and reject a live reload with
//! `catalog_change_requires_restart`; display name, enabled state, pricing, the
//! default profile, and the context window affect fresh runs only. Credential
//! material never participates in comparison.

use intention_proto::{
    ConfigurationEditDto, CredentialTransportModeDto, DtoResult, ErrorDto, LoopbackPolicyDto,
    ProviderKindId, ProviderPricingPolicyDto, ProviderProfileId, ReasoningEffortLevelDto,
    UserKindCompositionDto,
};
use serde::{Deserialize, Serialize};

use crate::{
    CURRENT_SCHEMA_MAJOR, ContextWindowPolicyDto, ProviderExecutionPolicyDto, RawConfigInputDto,
    RawProviderExecutionPolicyDto,
};

/// Header names a safe-header credential transport must never take over.
///
/// The framing headers would corrupt the request envelope, and `authorization`
/// is owned by the dedicated bearer mode.
const FORBIDDEN_SAFE_HEADER_NAMES: [&str; 7] = [
    "authorization",
    "proxy-authorization",
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "upgrade",
];

/// The closed field set of one `[providers.kinds.<id>]` declaration.
///
/// The shared `UserKindCompositionDto` serde shape is the source of truth; this
/// list exists because a document-level parser must reject unknown keys, and the
/// composition's own decoder accepts them.
const USER_KIND_COMPOSITION_KEYS: [&str; 6] = [
    "kind_id",
    "stream",
    "reasoning",
    "activation",
    "effort",
    "credential_transport",
];

/// A validated, credential-free provider-catalog document.
///
/// The document is produced only by [`CatalogDocumentDto::parse_credential_free_edit`]
/// or by [`CatalogCandidate::parse`]; a decoded document is never trusted from a
/// foreign source. Profile and user-kind lists are ordered by their stable ids,
/// so two semantically equal documents are equal values.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CatalogDocumentDto {
    context_window: ContextWindowPolicyDto,
    default_profile: Option<ProviderProfileId>,
    user_kinds: Vec<CatalogUserKindDto>,
    profiles: Vec<CatalogProfileDto>,
}

/// One declared user provider kind and its closed protocol-part composition.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CatalogUserKindDto {
    kind_id: ProviderKindId,
    composition: UserKindCompositionDto,
}

impl CatalogUserKindDto {
    /// Returns the declared user-kind identity.
    #[must_use]
    pub const fn kind_id(&self) -> &ProviderKindId {
        &self.kind_id
    }

    /// Returns the closed protocol-part composition admitted for this kind.
    #[must_use]
    pub const fn composition(&self) -> &UserKindCompositionDto {
        &self.composition
    }
}

/// One catalog profile: revision-affecting declaration plus presentation policy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CatalogProfileDto {
    declaration: CatalogProfileDeclarationDto,
    display_name: String,
    enabled: bool,
    pricing: Option<ProviderPricingPolicyDto>,
}

impl CatalogProfileDto {
    /// Returns the stable profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.declaration.profile_id
    }

    /// Returns the revision-affecting declaration.
    #[must_use]
    pub const fn declaration(&self) -> &CatalogProfileDeclarationDto {
        &self.declaration
    }

    /// Returns the safe presentation name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns whether the profile is enabled for fresh selection.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Returns the optional product pricing policy.
    #[must_use]
    pub const fn pricing(&self) -> Option<&ProviderPricingPolicyDto> {
        self.pricing.as_ref()
    }

    /// Returns whether both profiles describe the same revision-affecting meaning.
    ///
    /// Display name, enabled state, and pricing are excluded, exactly like
    /// architecture 22's profile-revision law, so credential-only replacement and
    /// presentation edits never look like a revision change.
    fn same_revision_meaning(&self, other: &Self) -> bool {
        self.declaration == other.declaration
    }
}

/// The credential-free, revision-affecting declaration of one profile.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CatalogProfileDeclarationDto {
    profile_id: ProviderProfileId,
    kind_id: ProviderKindId,
    model_id: String,
    normalized_effective_endpoint: Option<String>,
    credential_transport_mode: CredentialTransportModeDto,
    credential_transport_safe_header_name: Option<String>,
    reasoning_effort: Option<ReasoningEffortLevelDto>,
    effective_execution_policy: ProviderExecutionPolicyDto,
    declared_capabilities: CatalogDeclaredCapabilitySubsetDto,
    effective_loopback_policy: LoopbackPolicyDto,
}

impl CatalogProfileDeclarationDto {
    /// Returns the stable profile identity.
    #[must_use]
    pub const fn profile_id(&self) -> &ProviderProfileId {
        &self.profile_id
    }

    /// Returns the declared provider kind.
    #[must_use]
    pub const fn kind_id(&self) -> &ProviderKindId {
        &self.kind_id
    }

    /// Returns the exact configured model identifier.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Returns the normalized credential-free endpoint when one is configured.
    #[must_use]
    pub fn normalized_effective_endpoint(&self) -> Option<&str> {
        self.normalized_effective_endpoint.as_deref()
    }

    /// Returns the validated credential transport mode.
    #[must_use]
    pub const fn credential_transport_mode(&self) -> CredentialTransportModeDto {
        self.credential_transport_mode
    }

    /// Returns the validated safe header name for safe-header transport.
    #[must_use]
    pub fn credential_transport_safe_header_name(&self) -> Option<&str> {
        self.credential_transport_safe_header_name.as_deref()
    }

    /// Returns the selected reasoning-effort value when one is declared.
    #[must_use]
    pub const fn reasoning_effort(&self) -> Option<ReasoningEffortLevelDto> {
        self.reasoning_effort
    }

    /// Returns the effective provider execution policy.
    #[must_use]
    pub const fn effective_execution_policy(&self) -> &ProviderExecutionPolicyDto {
        &self.effective_execution_policy
    }

    /// Returns the declared exact-model capability subset.
    #[must_use]
    pub const fn declared_capabilities(&self) -> &CatalogDeclaredCapabilitySubsetDto {
        &self.declared_capabilities
    }

    /// Returns the applicable loopback policy.
    #[must_use]
    pub const fn effective_loopback_policy(&self) -> LoopbackPolicyDto {
        self.effective_loopback_policy
    }
}

/// The exact-model capability subset one profile explicitly declares.
///
/// This is the closed declarable slice of the model-capability taxonomy v1.
/// `input`, `structured_output`, and `context_preservation` are fixed by the
/// taxonomy and code-owned descriptor/compatibility metadata, so only the
/// text-streaming, textual-reasoning, and custom-function-call dimensions are
/// declarable. The descriptor-envelope intersection itself belongs to
/// `intention-providers` and the catalog builder.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CatalogDeclaredCapabilitySubsetDto {
    text_streaming: bool,
    reasoning: CatalogDeclaredReasoningCapabilityDto,
    tool_exchange: bool,
}

impl CatalogDeclaredCapabilitySubsetDto {
    /// Returns whether text streaming is declared available.
    #[must_use]
    pub const fn text_streaming(&self) -> bool {
        self.text_streaming
    }

    /// Returns the declared textual-reasoning capability.
    #[must_use]
    pub const fn reasoning(&self) -> &CatalogDeclaredReasoningCapabilityDto {
        &self.reasoning
    }

    /// Returns whether custom function-call admission is declared available.
    #[must_use]
    pub const fn tool_exchange(&self) -> bool {
        self.tool_exchange
    }
}

/// The declared textual-reasoning capability of one exact model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum CatalogDeclaredReasoningCapabilityDto {
    /// The exact model declares no textual reasoning.
    Disabled,
    /// The exact model declares textual reasoning with a closed effort set.
    TextualReasoningV1 {
        /// The canonical, de-duplicated closed effort values the model accepts.
        supported_efforts: Vec<ReasoningEffortLevelDto>,
        /// Whether the model can emit provider reasoning summaries.
        summary_support: bool,
    },
}

/// How a reload candidate compares to the active document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogReloadImpactDto {
    /// The candidate is semantically equal to the active document.
    Unchanged,
    /// The candidate changes only fresh-run behavior.
    FreshRunsOnly,
}

/// Opaque per-profile private credential material.
///
/// This type intentionally implements no `Debug`, `Display`, `Clone`, or serde
/// traits, provides no credential accessor, and never compares credentials.
/// Composition may move it through [`CatalogCandidate::into_parts_for_catalog`]
/// and read one profile's credential only through the controlled
/// [`CatalogCredentialMaterial::with_profile_credential`] boundary.
pub struct CatalogCredentialMaterial {
    credentials: Vec<(ProviderProfileId, String)>,
}

impl CatalogCredentialMaterial {
    /// Applies one profile's private credential inside a controlled boundary.
    ///
    /// # Errors
    ///
    /// Returns a safe validation error when the profile has no credential
    /// material, without disclosing any credential value.
    pub fn with_profile_credential<T>(
        &self,
        profile_id: &ProviderProfileId,
        consumer: impl FnOnce(&str) -> T,
    ) -> DtoResult<T> {
        self.credentials
            .iter()
            .find(|(candidate, _)| candidate.as_str() == profile_id.as_str())
            .map(|(_, credential)| consumer(credential))
            .ok_or_else(missing_provider_credential)
    }
}

/// One validated catalog document plus its private credential material.
///
/// The safe document may be inspected and rendered; the private material crosses
/// only [`CatalogCandidate::into_parts_for_catalog`] or the credential-material
/// rotation boundary. The candidate intentionally implements no `Debug`,
/// `Display`, or serde traits.
pub struct CatalogCandidate {
    document: CatalogDocumentDto,
    material: CatalogCredentialMaterial,
}

impl CatalogCandidate {
    /// Parses, validates, and resolves one complete catalog document.
    ///
    /// Every declared profile must carry a non-blank credential; the credential
    /// value is retained only as opaque private material and never enters the
    /// returned credential-free document, an error, or a projection.
    ///
    /// # Errors
    ///
    /// Returns only safe typed validation errors; the input document and any
    /// credential it contains are deliberately omitted from errors.
    pub fn parse(input: RawConfigInputDto) -> DtoResult<Self> {
        let (document, credentials) =
            parse_catalog_document(&input.text, CatalogCredentialPolicy::Required)?;
        Ok(Self {
            document,
            material: CatalogCredentialMaterial { credentials },
        })
    }

    /// Returns the credential-free validated document.
    #[must_use]
    pub const fn safe_document(&self) -> &CatalogDocumentDto {
        &self.document
    }

    /// Moves this candidate into the supplied catalog-composition consumer.
    ///
    /// This is the only consuming boundary for private credential material.
    pub fn into_parts_for_catalog<T>(
        self,
        consumer: impl FnOnce(CatalogDocumentDto, CatalogCredentialMaterial) -> T,
    ) -> T {
        consumer(self.document, self.material)
    }

    /// Binds a credential-free edited document to retained private material.
    ///
    /// This is the edit-surface boundary: the document is restored server-side
    /// inside the private loading boundary, every profile must have retained
    /// material, and the returned candidate carries exactly the document's
    /// profiles. Unused retained material is dropped.
    ///
    /// # Errors
    ///
    /// Returns a safe validation error when a declared profile has no retained
    /// credential material.
    pub fn bind_retained_material(
        document: CatalogDocumentDto,
        material: &CatalogCredentialMaterial,
    ) -> DtoResult<Self> {
        let mut credentials = Vec::with_capacity(document.profiles.len());
        for profile in &document.profiles {
            let credential = material
                .credentials
                .iter()
                .find(|(candidate, _)| candidate.as_str() == profile.profile_id().as_str())
                .map(|(_, credential)| credential.clone())
                .ok_or_else(missing_provider_credential)?;
            credentials.push((profile.profile_id().clone(), credential));
        }
        Ok(Self {
            document,
            material: CatalogCredentialMaterial { credentials },
        })
    }

    /// Classifies this candidate against the active document for a live reload.
    ///
    /// # Errors
    ///
    /// Returns `catalog_change_requires_restart` when the candidate changes
    /// catalog-affecting configuration.
    pub fn classify_reload_against(
        &self,
        active: &CatalogDocumentDto,
    ) -> DtoResult<CatalogReloadImpactDto> {
        classify_catalog_reload(active, &self.document)
    }

    /// Renders a credential-free document with typed edits applied.
    ///
    /// # Errors
    ///
    /// Returns a safe typed validation error when an edit names an unknown
    /// profile or an invalid display name.
    pub fn render_edited_document(&self, edits: &[ConfigurationEditDto]) -> DtoResult<String> {
        self.document.render_with_edits(edits)
    }
}

impl CatalogDocumentDto {
    /// Parses and validates one credential-free configuration-edit document.
    ///
    /// # Errors
    ///
    /// Returns only safe typed validation errors. Any `credential` key anywhere
    /// in the document fails `configuration_edit_contains_credential`.
    pub fn parse_credential_free_edit(text: &str) -> DtoResult<Self> {
        parse_catalog_document(text, CatalogCredentialPolicy::Forbidden)
            .map(|(document, _material)| document)
    }

    /// Returns the effective global context-window policy.
    #[must_use]
    pub const fn context_window(&self) -> &ContextWindowPolicyDto {
        &self.context_window
    }

    /// Returns the optional global default profile.
    #[must_use]
    pub const fn default_profile(&self) -> Option<&ProviderProfileId> {
        self.default_profile.as_ref()
    }

    /// Returns declared user kinds ordered by stable kind id.
    #[must_use]
    pub fn user_kinds(&self) -> &[CatalogUserKindDto] {
        &self.user_kinds
    }

    /// Returns profiles ordered by stable profile id.
    #[must_use]
    pub fn profiles(&self) -> &[CatalogProfileDto] {
        &self.profiles
    }

    /// Returns one profile by identity.
    #[must_use]
    pub fn profile(&self, profile_id: &ProviderProfileId) -> Option<&CatalogProfileDto> {
        self.profiles
            .iter()
            .find(|profile| profile.profile_id().as_str() == profile_id.as_str())
    }

    /// Applies typed edits to a copy of this document.
    ///
    /// Edits apply in order; a later edit for the same profile field wins.
    ///
    /// # Errors
    ///
    /// Returns a safe typed validation error when an edit names an unknown
    /// profile or an invalid display name.
    pub fn apply_edits(&self, edits: &[ConfigurationEditDto]) -> DtoResult<Self> {
        let mut edited = self.clone();
        for edit in edits {
            match edit {
                ConfigurationEditDto::SetDefaultProfile { profile_id } => {
                    if edited.profile(profile_id).is_none() {
                        return Err(invalid_configuration_edit());
                    }
                    edited.default_profile = Some(profile_id.clone());
                }
                ConfigurationEditDto::SetProfileEnabled {
                    profile_id,
                    enabled,
                } => {
                    let index = edited
                        .position(profile_id)
                        .ok_or_else(invalid_configuration_edit)?;
                    edited.profiles[index].enabled = *enabled;
                }
                ConfigurationEditDto::SetProfileDisplayName {
                    profile_id,
                    display_name,
                } => {
                    if display_name.trim().is_empty() || display_name.chars().any(char::is_control)
                    {
                        return Err(invalid_configuration_edit());
                    }
                    let index = edited
                        .position(profile_id)
                        .ok_or_else(invalid_configuration_edit)?;
                    edited.profiles[index].display_name = display_name.clone();
                }
            }
        }
        Ok(edited)
    }

    /// Renders this document, with typed edits applied, as deterministic
    /// credential-free TOML.
    ///
    /// # Errors
    ///
    /// Returns a safe typed validation error when an edit names an unknown
    /// profile or an invalid display name.
    pub fn render_with_edits(&self, edits: &[ConfigurationEditDto]) -> DtoResult<String> {
        render_catalog_document(&self.apply_edits(edits)?)
    }

    fn position(&self, profile_id: &ProviderProfileId) -> Option<usize> {
        self.profiles
            .iter()
            .position(|profile| profile.profile_id().as_str() == profile_id.as_str())
    }
}

/// Classifies a reload candidate against the active document.
///
/// # Errors
///
/// Returns `catalog_change_requires_restart` when the candidate changes
/// catalog-affecting configuration (the profile/kind set, a profile's
/// revision-affecting declaration, or a user-kind composition).
pub fn classify_catalog_reload(
    active: &CatalogDocumentDto,
    candidate: &CatalogDocumentDto,
) -> DtoResult<CatalogReloadImpactDto> {
    if catalog_affecting_change(active, candidate) {
        return Err(ErrorDto::validation(
            "catalog_change_requires_restart",
            "catalog-affecting configuration changes require a daemon restart",
        ));
    }
    if active == candidate {
        Ok(CatalogReloadImpactDto::Unchanged)
    } else {
        Ok(CatalogReloadImpactDto::FreshRunsOnly)
    }
}

/// Validates and normalizes one provider endpoint into a credential-free string.
///
/// # Errors
///
/// Returns a safe typed validation error for every endpoint that is not an
/// absolute canonical URL with a non-root API base path; the rejected input is
/// never echoed in the error.
pub fn normalize_provider_endpoint(endpoint: &str) -> DtoResult<String> {
    analyze_provider_endpoint(endpoint).map(|(normalized, _policy)| normalized)
}

/// The credential requirement of one catalog parse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CatalogCredentialPolicy {
    /// Every profile must carry a non-blank credential.
    Required,
    /// No profile may carry a credential.
    Forbidden,
}

fn parse_catalog_document(
    text: &str,
    credential_policy: CatalogCredentialPolicy,
) -> DtoResult<(CatalogDocumentDto, Vec<(ProviderProfileId, String)>)> {
    let value: toml::Value = toml::from_str(text).map_err(|_| invalid_config_toml())?;
    require_catalog_schema_version(&value)?;
    if credential_policy == CatalogCredentialPolicy::Forbidden
        && contains_named_key(&value, "credential")
    {
        return Err(ErrorDto::validation(
            "configuration_edit_contains_credential",
            "configuration edit documents must be credential-free",
        ));
    }
    let raw: RawCatalogDocumentDto = value.try_into().map_err(|_| invalid_config_schema())?;
    let RawCatalogDocumentDto {
        provider,
        providers,
        ..
    } = raw;
    let (context_window_tokens, default_profile_raw) = provider.map_or((None, None), |global| {
        (global.context_window_tokens, global.default_profile)
    });
    let context_window = ContextWindowPolicyDto::from_raw(context_window_tokens)?;
    let RawProvidersDto { profiles, kinds } = providers.unwrap_or_default();
    let user_kinds = resolve_user_kinds(kinds)?;
    let Some(profiles) = profiles else {
        return Err(missing_provider_profile());
    };
    if profiles.is_empty() {
        return Err(missing_provider_profile());
    }
    let mut resolved_profiles = Vec::with_capacity(profiles.len());
    let mut credentials = Vec::new();
    for (raw_id, raw_profile) in profiles {
        let profile_id =
            ProviderProfileId::parse(&raw_id).map_err(|_| invalid_provider_profile_id())?;
        let raw_profile: RawCatalogProfileDto = raw_profile
            .try_into()
            .map_err(|_| invalid_config_schema())?;
        let (profile, credential) =
            resolve_profile(profile_id, raw_profile, &user_kinds, credential_policy)?;
        if let Some(credential) = credential {
            credentials.push((profile.profile_id().clone(), credential));
        }
        resolved_profiles.push(profile);
    }
    resolved_profiles
        .sort_by(|left, right| left.profile_id().as_str().cmp(right.profile_id().as_str()));
    for pair in resolved_profiles.windows(2) {
        if pair[0].profile_id().as_str() == pair[1].profile_id().as_str() {
            return Err(invalid_provider_profile_id());
        }
    }
    let default_profile = resolve_default_profile(default_profile_raw, &resolved_profiles)?;
    let document = CatalogDocumentDto {
        context_window,
        default_profile,
        user_kinds,
        profiles: resolved_profiles,
    };
    Ok((document, credentials))
}

fn resolve_default_profile(
    raw: Option<String>,
    profiles: &[CatalogProfileDto],
) -> DtoResult<Option<ProviderProfileId>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let profile_id =
        ProviderProfileId::parse(&raw).map_err(|_| invalid_provider_default_profile())?;
    if profiles
        .iter()
        .any(|profile| profile.profile_id().as_str() == profile_id.as_str())
    {
        Ok(Some(profile_id))
    } else {
        Err(invalid_provider_default_profile())
    }
}

fn resolve_user_kinds(kinds: Option<toml::Table>) -> DtoResult<Vec<CatalogUserKindDto>> {
    let Some(kinds) = kinds else {
        return Ok(Vec::new());
    };
    let mut resolved = Vec::with_capacity(kinds.len());
    for (raw_id, raw_composition) in kinds {
        let kind_id = ProviderKindId::parse(&raw_id).map_err(|_| invalid_provider_kind())?;
        if kind_id.is_first_party() {
            return Err(invalid_provider_kind());
        }
        let mut table = raw_composition
            .as_table()
            .cloned()
            .ok_or_else(invalid_config_schema)?;
        if table
            .keys()
            .any(|key| !USER_KIND_COMPOSITION_KEYS.contains(&key.as_str()))
        {
            return Err(invalid_config_schema());
        }
        let credential_transport =
            resolve_credential_transport(table.remove("credential_transport"))?;
        table.insert(
            "credential_transport".to_owned(),
            credential_transport.shared_dto_value(),
        );
        let composition: UserKindCompositionDto = toml::Value::Table(table)
            .try_into()
            .map_err(|_| invalid_config_schema())?;
        if composition.kind_id().as_str() != kind_id.as_str() {
            return Err(invalid_provider_kind());
        }
        resolved.push(CatalogUserKindDto {
            kind_id,
            composition,
        });
    }
    resolved.sort_by(|left, right| left.kind_id.as_str().cmp(right.kind_id.as_str()));
    Ok(resolved)
}

fn resolve_profile(
    profile_id: ProviderProfileId,
    raw: RawCatalogProfileDto,
    declared_kinds: &[CatalogUserKindDto],
    credential_policy: CatalogCredentialPolicy,
) -> DtoResult<(CatalogProfileDto, Option<String>)> {
    let kind_id = ProviderKindId::parse(&raw.kind).map_err(|_| invalid_provider_kind())?;
    let kind_declared = kind_id.is_first_party()
        || declared_kinds
            .iter()
            .any(|declared| declared.kind_id.as_str() == kind_id.as_str());
    if !kind_declared {
        return Err(invalid_provider_kind());
    }
    if raw.model.trim().is_empty() {
        return Err(ErrorDto::validation(
            "invalid_provider_model",
            "provider model must not be empty",
        ));
    }
    let (normalized_effective_endpoint, effective_loopback_policy) = match raw.endpoint.as_deref() {
        Some(endpoint) => {
            let (normalized, policy) = analyze_provider_endpoint(endpoint)?;
            (Some(normalized), policy)
        }
        None => (None, LoopbackPolicyDto::NotApplicable),
    };
    let credential_transport = resolve_credential_transport(raw.credential_transport)?;
    let reasoning_effort = raw
        .reasoning_effort
        .as_deref()
        .map(parse_reasoning_effort)
        .transpose()?;
    let declared_capabilities = resolve_capabilities(raw.capabilities)?;
    if let Some(effort) = &reasoning_effort {
        let declared = match declared_capabilities.reasoning() {
            CatalogDeclaredReasoningCapabilityDto::TextualReasoningV1 {
                supported_efforts, ..
            } => supported_efforts.contains(effort),
            CatalogDeclaredReasoningCapabilityDto::Disabled => false,
        };
        if !declared {
            return Err(invalid_provider_reasoning_effort());
        }
    }
    let effective_execution_policy = crate::resolve_provider_execution_policy(raw.execution)?;
    let pricing = resolve_pricing(raw.pricing)?;
    let display_name = raw
        .display_name
        .unwrap_or_else(|| profile_id.as_str().to_owned());
    validate_display_name(&display_name)?;
    let enabled = raw.enabled.unwrap_or(true);
    let credential = match raw.credential {
        Some(value) if value.trim().is_empty() => return Err(missing_provider_credential()),
        Some(value) => Some(value),
        None if credential_policy == CatalogCredentialPolicy::Required => {
            return Err(missing_provider_credential());
        }
        None => None,
    };
    let declaration = CatalogProfileDeclarationDto {
        profile_id,
        kind_id,
        model_id: raw.model,
        normalized_effective_endpoint,
        credential_transport_mode: credential_transport.mode,
        credential_transport_safe_header_name: credential_transport.safe_header_name,
        reasoning_effort,
        effective_execution_policy,
        declared_capabilities,
        effective_loopback_policy,
    };
    let profile = CatalogProfileDto {
        declaration,
        display_name,
        enabled,
        pricing,
    };
    Ok((profile, credential))
}

/// One validated credential transport in the shared document spelling.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedCredentialTransport {
    mode: CredentialTransportModeDto,
    safe_header_name: Option<String>,
}

impl ResolvedCredentialTransport {
    /// Returns the value in the shared `CredentialTransportDto` serde shape.
    fn shared_dto_value(&self) -> toml::Value {
        let mut table = toml::Table::new();
        table.insert(
            "mode".to_owned(),
            toml::Value::String(self.mode.as_str().to_owned()),
        );
        if let Some(name) = &self.safe_header_name {
            table.insert(
                "safe_header_name".to_owned(),
                toml::Value::String(name.clone()),
            );
        }
        toml::Value::Table(table)
    }

    /// Returns the value in this document's closed credential-transport spelling.
    fn document_value(&self) -> toml::Value {
        self.safe_header_name.as_ref().map_or_else(
            || toml::Value::String("bearer".to_owned()),
            |name| {
                let mut table = toml::Table::new();
                table.insert("header".to_owned(), toml::Value::String(name.clone()));
                toml::Value::Table(table)
            },
        )
    }
}

fn resolve_credential_transport(
    raw: Option<toml::Value>,
) -> DtoResult<ResolvedCredentialTransport> {
    match raw {
        None => Ok(ResolvedCredentialTransport {
            mode: CredentialTransportModeDto::Bearer,
            safe_header_name: None,
        }),
        Some(toml::Value::String(mode)) if mode == "bearer" => Ok(ResolvedCredentialTransport {
            mode: CredentialTransportModeDto::Bearer,
            safe_header_name: None,
        }),
        Some(toml::Value::String(_)) => Err(invalid_provider_credential_transport()),
        Some(toml::Value::Table(table)) => {
            if let Some(header) = table.get("header") {
                if table.len() != 1 {
                    return Err(invalid_provider_credential_transport());
                }
                let Some(header) = header.as_str() else {
                    return Err(invalid_provider_credential_transport());
                };
                return Ok(ResolvedCredentialTransport {
                    mode: CredentialTransportModeDto::SafeHeader,
                    safe_header_name: Some(normalize_safe_header_name(header)?),
                });
            }
            // Kind sections mirror `UserKindCompositionDto`'s closed parts, so the
            // shared transport shape is accepted there and normalized the same way.
            if table
                .keys()
                .any(|key| !matches!(key.as_str(), "mode" | "safe_header_name"))
            {
                return Err(invalid_provider_credential_transport());
            }
            match table.get("mode").and_then(toml::Value::as_str) {
                Some("bearer") if !table.contains_key("safe_header_name") => {
                    Ok(ResolvedCredentialTransport {
                        mode: CredentialTransportModeDto::Bearer,
                        safe_header_name: None,
                    })
                }
                Some("safe_header") => {
                    let Some(header) = table.get("safe_header_name").and_then(toml::Value::as_str)
                    else {
                        return Err(invalid_provider_credential_transport());
                    };
                    Ok(ResolvedCredentialTransport {
                        mode: CredentialTransportModeDto::SafeHeader,
                        safe_header_name: Some(normalize_safe_header_name(header)?),
                    })
                }
                _ => Err(invalid_provider_credential_transport()),
            }
        }
        Some(_) => Err(invalid_provider_credential_transport()),
    }
}

fn normalize_safe_header_name(value: &str) -> DtoResult<String> {
    if value.is_empty() || !value.bytes().all(is_header_token_byte) {
        return Err(invalid_provider_credential_transport());
    }
    let normalized = value.to_ascii_lowercase();
    if FORBIDDEN_SAFE_HEADER_NAMES.contains(&normalized.as_str()) {
        return Err(invalid_provider_credential_transport());
    }
    Ok(normalized)
}

const fn is_header_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn parse_reasoning_effort(value: &str) -> DtoResult<ReasoningEffortLevelDto> {
    ReasoningEffortLevelDto::parse(value).map_err(|_| invalid_provider_reasoning_effort())
}

fn resolve_capabilities(
    raw: Option<RawCapabilitiesDto>,
) -> DtoResult<CatalogDeclaredCapabilitySubsetDto> {
    let raw = raw.unwrap_or_default();
    let text_streaming = raw.text_streaming.unwrap_or(false);
    let tool_exchange = raw.tool_exchange.unwrap_or(false);
    let has_reasoning_details = raw.reasoning_efforts.is_some() || raw.reasoning_summary.is_some();
    let reasoning = match raw.reasoning.as_deref().unwrap_or("disabled") {
        "disabled" => {
            if has_reasoning_details {
                return Err(invalid_provider_capabilities());
            }
            CatalogDeclaredReasoningCapabilityDto::Disabled
        }
        "textual_reasoning_v1" => {
            let declared = raw.reasoning_efforts.unwrap_or_default();
            let mut supported_efforts = Vec::new();
            for value in declared {
                supported_efforts.push(parse_reasoning_effort(&value)?);
            }
            supported_efforts.sort();
            supported_efforts.dedup();
            CatalogDeclaredReasoningCapabilityDto::TextualReasoningV1 {
                supported_efforts,
                summary_support: raw.reasoning_summary.unwrap_or(false),
            }
        }
        _ => return Err(invalid_provider_capabilities()),
    };
    Ok(CatalogDeclaredCapabilitySubsetDto {
        text_streaming,
        reasoning,
        tool_exchange,
    })
}

fn resolve_pricing(raw: Option<RawPricingDto>) -> DtoResult<Option<ProviderPricingPolicyDto>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    if raw.input_per_million_tokens.is_none() && raw.output_per_million_tokens.is_none() {
        return Ok(None);
    }
    ProviderPricingPolicyDto::new(raw.input_per_million_tokens, raw.output_per_million_tokens)
        .map(Some)
}

fn validate_display_name(value: &str) -> DtoResult<()> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        Err(invalid_provider_display_name())
    } else {
        Ok(())
    }
}

fn catalog_affecting_change(active: &CatalogDocumentDto, candidate: &CatalogDocumentDto) -> bool {
    if active.user_kinds != candidate.user_kinds {
        return true;
    }
    if active.profiles.len() != candidate.profiles.len() {
        return true;
    }
    active.profiles.iter().any(|current| {
        candidate
            .profile(current.profile_id())
            .is_none_or(|next| !current.same_revision_meaning(next))
    })
}

fn analyze_provider_endpoint(endpoint: &str) -> DtoResult<(String, LoopbackPolicyDto)> {
    if endpoint.is_empty() || endpoint.bytes().any(|byte| !(0x21..=0x7E).contains(&byte)) {
        return Err(invalid_provider_endpoint());
    }
    if endpoint.contains('#') || endpoint.contains('?') || endpoint.contains('\\') {
        return Err(invalid_provider_endpoint());
    }
    let Some((scheme, remainder)) = endpoint.split_once("://") else {
        return Err(invalid_provider_endpoint());
    };
    let uses_loopback_http = match scheme {
        "https" => false,
        "http" => true,
        _ => return Err(invalid_provider_endpoint()),
    };
    let Some((authority, path)) = remainder.split_once('/') else {
        return Err(invalid_provider_endpoint());
    };
    if authority.is_empty() || authority.contains('@') || authority.contains('%') {
        return Err(invalid_provider_endpoint());
    }
    let mut path = path;
    while path.ends_with('/') {
        path = &path[..path.len() - 1];
    }
    if path.is_empty() {
        return Err(invalid_provider_endpoint());
    }
    let (host, port) = split_authority(authority)?;
    validate_host(host)?;
    let loopback_host = is_loopback_host(host);
    if uses_loopback_http && !loopback_host {
        return Err(invalid_provider_endpoint());
    }
    validate_percent_escapes(endpoint)?;
    let raw_path = format!("/{path}");
    for segment in raw_path.split('/').skip(1) {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(invalid_provider_endpoint());
        }
    }
    let port_suffix = match (scheme, port) {
        (_, None) => String::new(),
        ("https", Some(443)) | ("http", Some(80)) => String::new(),
        (_, Some(port)) => format!(":{port}"),
    };
    let normalized = format!(
        "{scheme}://{host}{port_suffix}{}",
        uppercase_percent_escapes(&raw_path)
    );
    let policy = if uses_loopback_http {
        LoopbackPolicyDto::ExplicitLoopback
    } else {
        LoopbackPolicyDto::NotApplicable
    };
    Ok((normalized, policy))
}

fn split_authority(authority: &str) -> DtoResult<(&str, Option<u16>)> {
    if authority.starts_with('[') {
        let close = authority.find(']').ok_or_else(invalid_provider_endpoint)?;
        let host = &authority[..=close];
        if host != "[::1]" {
            return Err(invalid_provider_endpoint());
        }
        let remainder = &authority[close + 1..];
        if remainder.is_empty() {
            return Ok((host, None));
        }
        let Some(port) = remainder.strip_prefix(':') else {
            return Err(invalid_provider_endpoint());
        };
        return Ok((host, Some(parse_port(port)?)));
    }
    if authority.contains(['[', ']']) {
        return Err(invalid_provider_endpoint());
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() || host.contains(':') {
            return Err(invalid_provider_endpoint());
        }
        return Ok((host, Some(parse_port(port)?)));
    }
    if authority.is_empty() {
        return Err(invalid_provider_endpoint());
    }
    Ok((authority, None))
}

fn parse_port(port: &str) -> DtoResult<u16> {
    if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_provider_endpoint());
    }
    if port.len() > 1 && port.starts_with('0') {
        return Err(invalid_provider_endpoint());
    }
    match port.parse::<u16>() {
        Ok(0) | Err(_) => Err(invalid_provider_endpoint()),
        Ok(value) => Ok(value),
    }
}

fn validate_host(host: &str) -> DtoResult<()> {
    if is_loopback_host(host) {
        return Ok(());
    }
    if host.contains(['[', ']']) {
        return Err(invalid_provider_endpoint());
    }
    if host
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return validate_ipv4_literal(host);
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return Err(invalid_provider_endpoint());
    }
    if labels
        .last()
        .is_some_and(|label| is_numeric_host_part(label))
    {
        // inet_aton-style spellings such as `127.1` or `0x7f.0.0.1` are address
        // aliases, not named hosts, and fail closed.
        return Err(invalid_provider_endpoint());
    }
    for label in labels {
        if label.is_empty() || label.starts_with('-') || label.ends_with('-') {
            return Err(invalid_provider_endpoint());
        }
        if !label
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(invalid_provider_endpoint());
        }
    }
    Ok(())
}

fn is_numeric_host_part(label: &str) -> bool {
    if label.is_empty() {
        return false;
    }
    if label.bytes().all(|byte| byte.is_ascii_digit()) {
        return true;
    }
    let Some(hex) = label
        .strip_prefix("0x")
        .or_else(|| label.strip_prefix("0X"))
    else {
        return false;
    };
    !hex.is_empty() && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_ipv4_literal(host: &str) -> DtoResult<()> {
    let octets: Vec<&str> = host.split('.').collect();
    if octets.len() != 4 {
        return Err(invalid_provider_endpoint());
    }
    for octet in octets {
        if octet.is_empty() || octet.len() > 3 || (octet.len() > 1 && octet.starts_with('0')) {
            return Err(invalid_provider_endpoint());
        }
        match octet.parse::<u16>() {
            Ok(value) if value <= 255 => {}
            _ => return Err(invalid_provider_endpoint()),
        }
    }
    Ok(())
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

fn validate_percent_escapes(value: &str) -> DtoResult<()> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        let (Some(high), Some(low)) = (
            bytes.get(index + 1).and_then(|byte| hex_value(*byte)),
            bytes.get(index + 2).and_then(|byte| hex_value(*byte)),
        ) else {
            return Err(invalid_provider_endpoint());
        };
        let decoded = (high << 4) | low;
        if decoded < 0x21 || decoded == 0x7F || matches!(decoded, b'/' | b'\\' | b'.' | b'#' | b'?')
        {
            return Err(invalid_provider_endpoint());
        }
        index += 3;
    }
    Ok(())
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn uppercase_percent_escapes(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        normalized.push(char::from(byte));
        if byte == b'%' {
            normalized.push(char::from(bytes[index + 1].to_ascii_uppercase()));
            normalized.push(char::from(bytes[index + 2].to_ascii_uppercase()));
            index += 3;
        } else {
            index += 1;
        }
    }
    normalized
}

fn contains_named_key(value: &toml::Value, key: &str) -> bool {
    match value {
        toml::Value::Table(table) => table
            .iter()
            .any(|(name, nested)| name == key || contains_named_key(nested, key)),
        toml::Value::Array(items) => items.iter().any(|item| contains_named_key(item, key)),
        _ => false,
    }
}

fn require_catalog_schema_version(value: &toml::Value) -> DtoResult<()> {
    match value.get("schema_version") {
        Some(toml::Value::Integer(major)) if *major == i64::from(CURRENT_SCHEMA_MAJOR) => Ok(()),
        Some(toml::Value::Integer(_)) => Err(ErrorDto::validation(
            "unsupported_config_schema_version",
            "configuration schema version is not supported",
        )),
        Some(_) => Err(ErrorDto::validation(
            "invalid_config_schema_version",
            "configuration schema version must be an integer",
        )),
        None => Err(ErrorDto::validation(
            "invalid_config_schema",
            "configuration does not include a schema version",
        )),
    }
}

fn render_catalog_document(document: &CatalogDocumentDto) -> DtoResult<String> {
    let mut output = String::new();
    output.push_str("schema_version = 1\n\n[provider]\n");
    output.push_str(&format!(
        "context_window_tokens = {}\n",
        document.context_window.window_tokens()
    ));
    if let Some(default_profile) = &document.default_profile {
        output.push_str(&format!(
            "default_profile = {}\n",
            toml_string(default_profile.as_str())
        ));
    }
    for kind in &document.user_kinds {
        let key = toml_key(kind.kind_id.as_str());
        let prefix = format!("providers.kinds.{key}");
        output.push_str(&format!("\n[{prefix}]\n"));
        render_toml_table(&mut output, &prefix, &composition_table(kind)?);
    }
    for profile in &document.profiles {
        render_profile(&mut output, profile)?;
    }
    Ok(output)
}

fn render_profile(output: &mut String, profile: &CatalogProfileDto) -> DtoResult<()> {
    let declaration = &profile.declaration;
    let key = toml_key(declaration.profile_id.as_str());
    let prefix = format!("providers.profiles.{key}");
    output.push_str(&format!("\n[{prefix}]\n"));
    output.push_str(&format!(
        "kind = {}\n",
        toml_string(declaration.kind_id.as_str())
    ));
    output.push_str(&format!("model = {}\n", toml_string(&declaration.model_id)));
    if let Some(endpoint) = &declaration.normalized_effective_endpoint {
        output.push_str(&format!("endpoint = {}\n", toml_string(endpoint)));
    }
    output.push_str(&format!("enabled = {}\n", profile.enabled));
    output.push_str(&format!(
        "display_name = {}\n",
        toml_string(&profile.display_name)
    ));
    if let Some(header) = &declaration.credential_transport_safe_header_name {
        output.push_str(&format!(
            "credential_transport = {{ header = {} }}\n",
            toml_string(header)
        ));
    }
    if let Some(effort) = declaration.reasoning_effort {
        output.push_str(&format!(
            "reasoning_effort = {}\n",
            toml_string(effort.as_str())
        ));
    }
    output.push_str(&format!("\n[{prefix}.execution]\n"));
    output.push_str(&format!(
        "attempt_timeout_seconds = {}\n",
        declaration
            .effective_execution_policy
            .attempt_timeout_seconds()
    ));
    output.push_str(&format!(
        "max_attempts = {}\n",
        declaration.effective_execution_policy.max_attempts()
    ));
    if let Some(pricing) = &profile.pricing {
        output.push_str(&format!("\n[{prefix}.pricing]\n"));
        if let Some(value) = pricing.input_per_million_tokens() {
            output.push_str(&format!(
                "input_per_million_tokens = {}\n",
                toml::Value::Float(value)
            ));
        }
        if let Some(value) = pricing.output_per_million_tokens() {
            output.push_str(&format!(
                "output_per_million_tokens = {}\n",
                toml::Value::Float(value)
            ));
        }
    }
    output.push_str(&format!("\n[{prefix}.capabilities]\n"));
    output.push_str(&format!(
        "text_streaming = {}\n",
        declaration.declared_capabilities.text_streaming
    ));
    match &declaration.declared_capabilities.reasoning {
        CatalogDeclaredReasoningCapabilityDto::Disabled => {
            output.push_str("reasoning = \"disabled\"\n");
        }
        CatalogDeclaredReasoningCapabilityDto::TextualReasoningV1 {
            supported_efforts,
            summary_support,
        } => {
            output.push_str("reasoning = \"textual_reasoning_v1\"\n");
            output.push_str(&format!(
                "reasoning_efforts = {}\n",
                toml::Value::Array(
                    supported_efforts
                        .iter()
                        .map(|effort| toml::Value::String(effort.as_str().to_owned()))
                        .collect()
                )
            ));
            output.push_str(&format!("reasoning_summary = {summary_support}\n"));
        }
    }
    output.push_str(&format!(
        "tool_exchange = {}\n",
        declaration.declared_capabilities.tool_exchange
    ));
    Ok(())
}

/// Renders one TOML table as deterministic text.
///
/// Scalar keys are emitted before nested tables so that a subtable header never
/// captures the scalars that follow it in iteration order.
fn render_toml_table(output: &mut String, prefix: &str, table: &toml::Table) {
    for (name, value) in table {
        if !matches!(value, toml::Value::Table(_)) {
            output.push_str(&format!("{} = {value}\n", toml_key(name)));
        }
    }
    for (name, value) in table {
        if let toml::Value::Table(nested) = value {
            let nested_key = toml_key(name);
            let nested_prefix = format!("{prefix}.{nested_key}");
            output.push_str(&format!("\n[{nested_prefix}]\n"));
            render_toml_table(output, &nested_prefix, nested);
        }
    }
}

fn composition_table(kind: &CatalogUserKindDto) -> DtoResult<toml::Table> {
    let mut table = toml::Value::try_from(&kind.composition)
        .ok()
        .and_then(|value| value.as_table().cloned())
        .ok_or_else(invalid_config_schema)?;
    let transport = kind.composition.credential_transport();
    let document_transport = ResolvedCredentialTransport {
        mode: transport.mode(),
        safe_header_name: transport.safe_header_name().map(str::to_owned),
    };
    table.insert(
        "credential_transport".to_owned(),
        document_transport.document_value(),
    );
    Ok(table)
}

fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

fn toml_key(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return value.to_owned();
    }
    toml_string(value)
}

fn invalid_config_schema() -> ErrorDto {
    ErrorDto::validation(
        "invalid_config_schema",
        "configuration does not match the supported catalog schema",
    )
}

fn invalid_config_toml() -> ErrorDto {
    ErrorDto::validation(
        "invalid_config_toml",
        "configuration TOML could not be parsed",
    )
}

fn missing_provider_profile() -> ErrorDto {
    ErrorDto::validation(
        "missing_provider_profile",
        "configuration must declare at least one provider profile",
    )
}

fn missing_provider_credential() -> ErrorDto {
    ErrorDto::validation(
        "missing_provider_credential",
        "provider credential must not be empty",
    )
}

fn invalid_provider_kind() -> ErrorDto {
    ErrorDto::validation(
        "invalid_provider_kind",
        "provider kind must be a first-party kind or a declared user kind",
    )
}

fn invalid_provider_profile_id() -> ErrorDto {
    ErrorDto::validation(
        "invalid_provider_profile_id",
        "provider profile id must be a non-blank printable token",
    )
}

fn invalid_provider_default_profile() -> ErrorDto {
    ErrorDto::validation(
        "invalid_provider_default_profile",
        "the default profile must reference a declared provider profile",
    )
}

fn invalid_provider_endpoint() -> ErrorDto {
    ErrorDto::validation(
        "invalid_provider_endpoint",
        "provider endpoint must be an absolute canonical API base URL",
    )
}

fn invalid_provider_credential_transport() -> ErrorDto {
    ErrorDto::validation(
        "invalid_provider_credential_transport",
        "credential transport must be bearer or one safe header name",
    )
}

fn invalid_provider_reasoning_effort() -> ErrorDto {
    ErrorDto::validation(
        "invalid_provider_reasoning_effort",
        "reasoning effort must be a declared closed value of the profile capability subset",
    )
}

fn invalid_provider_capabilities() -> ErrorDto {
    ErrorDto::validation(
        "invalid_provider_capabilities",
        "declared capabilities must use closed values and consistent fields",
    )
}

fn invalid_provider_display_name() -> ErrorDto {
    ErrorDto::validation(
        "invalid_provider_display_name",
        "provider display name must be non-blank and free of control characters",
    )
}

fn invalid_configuration_edit() -> ErrorDto {
    ErrorDto::validation(
        "invalid_configuration_edit",
        "configuration edit must reference a declared profile",
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalogDocumentDto {
    #[serde(rename = "schema_version")]
    _schema_version: u16,
    provider: Option<RawCatalogGlobalDto>,
    providers: Option<RawProvidersDto>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalogGlobalDto {
    context_window_tokens: Option<u64>,
    default_profile: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvidersDto {
    profiles: Option<toml::Table>,
    kinds: Option<toml::Table>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalogProfileDto {
    kind: String,
    model: String,
    credential: Option<String>,
    endpoint: Option<String>,
    enabled: Option<bool>,
    display_name: Option<String>,
    credential_transport: Option<toml::Value>,
    reasoning_effort: Option<String>,
    execution: Option<RawProviderExecutionPolicyDto>,
    pricing: Option<RawPricingDto>,
    capabilities: Option<RawCapabilitiesDto>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCapabilitiesDto {
    text_streaming: Option<bool>,
    reasoning: Option<String>,
    reasoning_efforts: Option<Vec<String>>,
    reasoning_summary: Option<bool>,
    tool_exchange: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPricingDto {
    input_per_million_tokens: Option<f64>,
    output_per_million_tokens: Option<f64>,
}
