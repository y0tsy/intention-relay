# Intention Relay reference material

This directory contains the active legacy-derived reference material for rebuilding Intention Relay. It captures product behavior and agent policy that may inform the new implementation, but it does not prescribe the legacy architecture.

## Contents

- [`architecture/`](architecture/README.md): the approved target architecture, crate boundaries, quality gates, Makefile contract, TDD/verification policy, and implementation roadmap for the new Intention Relay implementation.
- [`m4.md`](m4.md): historical M4 execution charter, including Package 1 baseline, accepted decisions, lane integrations, and retained scope boundaries.
- [`production-ceiling-removal.md`](production-ceiling-removal.md): working scope reference for removing product ceilings from post-M4 production code (PR #15).
- [`closeout/m4-closure-evidence.md`](closeout/m4-closure-evidence.md): immutable M4 implementation baseline, local verification, CI matrix, coverage, acceptance evidence, exceptions, and retained deferrals.
- [`reconciliation/`](reconciliation/README.md): approved documentation-only post-M4 authority, compatibility, ownership, and delivery-boundary reconciliation.
- [`decisions/`](decisions/README.md): accepted cross-document architecture decisions and their provenance.
- [`legacy-baseline/`](legacy-baseline/00-manifest.md): the selected product baseline. Use it to identify user-visible capabilities and known limitations that the rewrite must deliberately accept, change, or reject.
- [`legacy-antibusy-prompts/`](legacy-antibusy-prompts/README.md): a read-only source copy of the static prompts injected into every legacy Antibusy session. Adapt these prompts for Intention Relay, do not consume them as production prompts unchanged.

For the broader, superseded legacy audit, see [`../reference/legacy-antibusy-audit/`](../reference/legacy-antibusy-audit/00-manifest.md). It is retained as research material and must not override this selected baseline without an explicit decision.
