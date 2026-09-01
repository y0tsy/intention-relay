//! The control-plane gate: serialized catalog state transitions.
//!
//! The gate serializes catalog acceptance, session-default changes, admission,
//! and registry lookups. It never blocks active model tasks: admission
//! clones an opaque handle id and tasks run outside the gate.

use std::sync::Mutex;

use intention_types::{DtoResult, ErrorDto};

/// The closed catalog readiness states.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogReadiness {
    /// No startup attempt has completed.
    Uninitialized,
    /// A startup attempt is in progress.
    Loading,
    /// The active catalog is fully built and serving.
    Ready,
    /// A removal candidate is pending against the active catalog.
    PendingRemoval {
        /// The prepared candidate catalog revision.
        candidate_revision: String,
        /// The candidate expiry time in Unix seconds.
        expires_at: u64,
    },
    /// The active catalog requires explicit activation recovery.
    ActivationRecoveryRequired {
        /// The accepted catalog revision awaiting recovery.
        accepted_revision: String,
    },
    /// The catalog is degraded and read-only for a typed reason.
    Blocked {
        /// The stable degraded reason.
        reason: String,
    },
}

/// The mutable state guarded by the control-plane gate.
pub struct ControlPlaneState {
    /// The current catalog readiness.
    pub(crate) readiness: CatalogReadiness,
    /// The last applied catalog revision, if any.
    pub(crate) applied_revision: Option<u64>,
    /// The active default profile id, if any.
    pub(crate) active_default_profile_id: Option<String>,
    /// The prepared-but-not-accepted candidate revision, if any.
    pub(crate) candidate_catalog_revision_id: Option<u64>,
    /// The safe degraded reason, if any.
    pub(crate) degraded_reason: Option<String>,
    /// The in-memory prepared candidate, if one is pending.
    pub(crate) prepared: Option<crate::provider_catalog::PreparedCandidate>,
}

impl Default for ControlPlaneState {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlPlaneState {
    /// Creates the initial uninitialized control-plane state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            readiness: CatalogReadiness::Uninitialized,
            applied_revision: None,
            active_default_profile_id: None,
            candidate_catalog_revision_id: None,
            degraded_reason: None,
            prepared: None,
        }
    }
}

/// Serializes catalog acceptance, session-default changes, admission, and
/// registry lookups without ever blocking active model tasks.
pub struct ControlPlaneGate {
    state: Mutex<ControlPlaneState>,
}

impl Default for ControlPlaneGate {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlPlaneGate {
    /// Creates a gate in the uninitialized readiness state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(ControlPlaneState::new()),
        }
    }

    /// Runs one exclusive state transition under the gate.
    ///
    /// The closure receives mutable control-plane state and may commit durable
    /// storage changes. Active model tasks never hold this lock: they clone an
    /// opaque handle id and run outside the gate.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the gate lock is poisoned, or the
    /// closure's own typed error.
    pub fn run_exclusive<T>(
        &self,
        f: impl FnOnce(&mut ControlPlaneState) -> DtoResult<T>,
    ) -> DtoResult<T> {
        let mut state = self.state.lock().map_err(|_| {
            ErrorDto::unavailable(
                "catalog_gate_unavailable",
                "the control-plane gate lock is poisoned",
            )
        })?;
        f(&mut state)
    }

    /// Reads the current control-plane state without mutating it.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the gate lock is poisoned.
    pub fn read<T>(&self, f: impl FnOnce(&ControlPlaneState) -> T) -> DtoResult<T> {
        let state = self.state.lock().map_err(|_| {
            ErrorDto::unavailable(
                "catalog_gate_unavailable",
                "the control-plane gate lock is poisoned",
            )
        })?;
        Ok(f(&state))
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit fixtures use expect to provide precise test failure messages."
    )]

    use super::*;

    #[test]
    fn run_exclusive_serializes_mutating_transitions() {
        let gate = ControlPlaneGate::new();
        gate.run_exclusive(|state| {
            state.applied_revision = Some(1);
            Ok(())
        })
        .expect("first transition commits");
        gate.run_exclusive(|state| {
            state.applied_revision = Some(2);
            state.readiness = CatalogReadiness::Ready;
            Ok(())
        })
        .expect("second transition commits");
        let applied = gate
            .read(|state| state.applied_revision)
            .expect("gate read succeeds");
        assert_eq!(applied, Some(2));
        let readiness = gate
            .read(|state| state.readiness.clone())
            .expect("gate read succeeds");
        assert_eq!(readiness, CatalogReadiness::Ready);
    }

    #[test]
    fn read_does_not_mutate_state() {
        let gate = ControlPlaneGate::new();
        gate.run_exclusive(|state| {
            state.applied_revision = Some(7);
            Ok(())
        })
        .expect("transition commits");
        let observed = gate
            .read(|state| {
                let value = state.applied_revision;
                assert_eq!(value, Some(7));
                value
            })
            .expect("gate read succeeds");
        assert_eq!(observed, Some(7));
        let after = gate
            .read(|state| state.applied_revision)
            .expect("gate read succeeds");
        assert_eq!(after, Some(7));
    }

    #[test]
    fn run_exclusive_propagates_typed_errors_without_mutation() {
        let gate = ControlPlaneGate::new();
        let error = gate
            .run_exclusive(|state| -> DtoResult<()> {
                state.applied_revision = Some(9);
                Err(ErrorDto::validation("fixture_error", "fixture failure"))
            })
            .expect_err("closure error propagates");
        assert_eq!(error.code(), "fixture_error");
        let after = gate
            .read(|state| state.applied_revision)
            .expect("gate read succeeds");
        assert_eq!(after, Some(9));
    }
}
