//! Slice 3 typed client surface.
//!
//! Owner: ADR 0044. Slice 3 adds no negotiated capability and no new envelope:
//! every Slice 3 selection rides the single live `run-execution-meaning-v4`
//! record and the existing daemon facade. This module is the public client path
//! for the typed Slice 3 records an adapter consumes: harness rules and
//! triggers, the closed programmatic-caller admission decisions and
//! confirmations, Goal identity/readiness/gates, and the delegated verification
//! authority and verdicts.
//!
//! The re-export adds no mapping, validation, bound, or numeric tag of its own:
//! the protocol crate owns the wire surface and `intention-domain` remains the
//! sole owner of the canonical records, validation codes, and bounds, so the
//! shared client's fail-closed credential and path behavior is exactly the
//! behavior of those single owners.

pub use intention_protocol::slice3::*;
