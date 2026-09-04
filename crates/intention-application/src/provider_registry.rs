//! Private provider driver registry for the catalog runtime.
//!
//! The registry holds opaque, credential-free identities and built driver
//! handles. It never holds credentials, SDK/client resources, or raw
//! configuration. Driver wiring happens in the composition root; the catalog
//! only builds and swaps opaque handles.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use intention_domain::{
    ProviderDriverContractRevisionDto, ProviderProfileRevisionV1, ProviderSelectionV1,
    canonical::Digest256,
};
use intention_types::{DtoResult, ErrorDto};

/// Maximum concurrently active private registry entries.
pub const MAX_ACTIVE_PRIVATE_ENTRIES: usize = 128;

/// The credential-free identity of one private registry entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivateRegistryKey {
    pub profile_id: String,
    pub profile_revision_id: String,
    pub kind_descriptor_revision_id: String,
    pub driver_contract: ProviderDriverContractRevisionDto,
}

impl std::hash::Hash for PrivateRegistryKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.profile_id.hash(state);
        self.profile_revision_id.hash(state);
        self.kind_descriptor_revision_id.hash(state);
        self.driver_contract.driver_family.hash(state);
        self.driver_contract.major.hash(state);
        self.driver_contract.minor.hash(state);
    }
}

/// Opaque marker handle for one built private model-run driver.
///
/// The handle is intentionally empty: driver wiring is owned by the
/// composition root in a later slice, and the catalog only carries opaque
/// handles. Tasks hold an [`Arc`] clone of the handle so a registry swap never
/// invalidates an in-flight task's handle.
pub trait ModelRunDriverHandle: Send + Sync {}

/// Private per-profile material consumed by a driver factory.
///
/// This type deliberately implements no `Debug`, `Display`, or serde traits:
/// it is the private side of the registry and never crosses a DTO boundary.
/// `private_credential_reference` is an opaque id into a composition-owned
/// credential store; the credential itself never enters the catalog.
pub struct PrivateProviderProfileMaterial {
    pub profile: ProviderProfileRevisionV1,
    pub selection: ProviderSelectionV1,
    pub endpoint: String,
    pub private_credential_reference: u64,
}

/// Builds opaque private driver handles for one provider kind.
pub trait ProviderDriverFactory: Send + Sync {
    /// Returns the provider kind this factory serves.
    #[must_use]
    fn kind(&self) -> &str;
    /// Returns whether this factory supports the supplied driver contract
    /// (same family, exact major, and an explicitly supported minor).
    #[must_use]
    fn supports_contract(&self, contract: &ProviderDriverContractRevisionDto) -> bool;
    /// Builds one opaque driver handle from private profile material.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the material cannot be materialized into a
    /// driver handle.
    fn build(
        &self,
        profile: PrivateProviderProfileMaterial,
    ) -> DtoResult<Box<dyn ModelRunDriverHandle + Send + Sync>>;
}

/// The private provider driver registry.
///
/// Entries are swapped atomically: the complete replacement map is built
/// first, then installed. Lookups clone an opaque [`Arc`] handle id so active
/// model tasks run outside any registry lock.
pub struct PrivateRegistry {
    entries: Mutex<HashMap<PrivateRegistryKey, Arc<dyn ModelRunDriverHandle + Send + Sync>>>,
}

impl Default for PrivateRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PrivateRegistry {
    /// Creates an empty private registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Builds the complete replacement entry map, all-or-nothing.
    ///
    /// Every material is built before any entry is installed; a single build
    /// failure discards the whole map. Each material must resolve to a factory
    /// whose kind matches the profile kind and whose supported contracts cover
    /// the key's driver contract.
    ///
    /// # Errors
    ///
    /// Returns `provider_driver_unavailable` when no factory serves the
    /// profile kind, `provider_driver_contract_incompatible` when the factory
    /// does not support the driver contract, or the factory's own build error.
    pub fn build_all(
        factories: &[Box<dyn ProviderDriverFactory>],
        materials: Vec<(PrivateRegistryKey, PrivateProviderProfileMaterial)>,
    ) -> DtoResult<HashMap<PrivateRegistryKey, Arc<dyn ModelRunDriverHandle + Send + Sync>>> {
        let mut built = HashMap::with_capacity(materials.len());
        for (key, material) in materials {
            let factory = factories
                .iter()
                .find(|factory| factory.kind() == material.profile.provider_kind_id)
                .ok_or_else(|| {
                    ErrorDto::unavailable(
                        "provider_driver_unavailable",
                        "no registered driver factory serves the provider kind",
                    )
                })?;
            if !factory.supports_contract(&key.driver_contract) {
                return Err(ErrorDto::validation(
                    "provider_driver_contract_incompatible",
                    "the registered driver factory does not support the profile driver contract",
                ));
            }
            let handle = Arc::from(factory.build(material)?);
            built.insert(key, handle);
        }
        Ok(built)
    }

    /// Atomically installs a complete replacement entry map.
    ///
    /// The replacement map must be fully built first; this method only
    /// enforces the active-entry bound and swaps. Handles previously cloned by
    /// active tasks remain valid because they are reference-counted.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the replacement map exceeds
    /// [`MAX_ACTIVE_PRIVATE_ENTRIES`], or an unavailable error when the
    /// registry lock is poisoned.
    pub fn activate(
        &self,
        built: HashMap<PrivateRegistryKey, Arc<dyn ModelRunDriverHandle + Send + Sync>>,
    ) -> DtoResult<()> {
        if built.len() > MAX_ACTIVE_PRIVATE_ENTRIES {
            return Err(ErrorDto::validation(
                "provider_registry_limit_exceeded",
                "the private provider registry exceeds its active entry limit",
            ));
        }
        let mut entries = self.entries.lock().map_err(|_| {
            ErrorDto::unavailable(
                "provider_registry_unavailable",
                "the private provider registry lock is poisoned",
            )
        })?;
        *entries = built;
        drop(entries);
        Ok(())
    }

    /// Clones the opaque handle for one exact registry key, if present.
    #[must_use]
    pub fn lookup(
        &self,
        key: &PrivateRegistryKey,
    ) -> Option<Arc<dyn ModelRunDriverHandle + Send + Sync>> {
        self.entries
            .lock()
            .ok()
            .and_then(|entries| entries.get(key).cloned())
    }

    /// Returns the number of active private registry entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.lock().map_or(0, |entries| entries.len())
    }

    /// Returns whether the private registry holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Derives the stable opaque private credential reference for a profile
/// revision.
///
/// The reference is deterministic per revision identity so the same profile
/// always resolves to the same composition-owned credential slot.
#[must_use]
pub fn private_credential_reference(profile_revision_id: &str) -> u64 {
    let digest = Digest256::sha256(profile_revision_id.as_bytes()).bytes();
    u64::from_be_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ])
}
