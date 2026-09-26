# Intention Relay reference material

This directory is the authoritative source for product, architecture, quality, and
roadmap context. It also keeps the selected legacy-derived baseline and prompt copies
that the rewrite must deliberately accept, change, or reject; the approved
architecture, not the legacy material, prescribes the new implementation.

## Contents

- [`architecture/`](architecture/README.md): the approved target architecture, crate boundaries, quality gates, Makefile contract, TDD/verification policy, and implementation roadmap for the new Intention Relay implementation.
- [`m4.md`](m4.md): historical M4 execution charter, including Package 1 baseline, accepted decisions, lane integrations, and retained scope boundaries.
- [`m4plus_concept.md`](m4plus_concept.md): the retained research concept and the primary provenance for the post-M4 authority; it is research material, not an implementation acceptance target ([supersession index](reconciliation/concept-supersession-index.md)).
- [`production-ceiling-removal.md`](production-ceiling-removal.md): working scope reference for removing product ceilings from post-M4 production code (PR #15).
- [`closeout/`](closeout/m0-m1-closure-evidence.md): the M0/M1, M1+, M2, M3, M4, and M5 closure evidence, each recorded at its own baseline; current delivery evidence lives in the [evidence register](reconciliation/evidence-register.md).
- [`reconciliation/`](reconciliation/README.md): approved documentation-only post-M4 authority, compatibility, ownership, and delivery-boundary reconciliation.
- [`decisions/`](decisions/README.md): accepted cross-document architecture decisions and their provenance.
- [`legacy-baseline/`](legacy-baseline/00-manifest.md): the selected product baseline. Use it to identify user-visible capabilities and known limitations that the rewrite must deliberately accept, change, or reject.
- [`legacy-antibusy-prompts/`](legacy-antibusy-prompts/README.md): a read-only source copy of the static prompts injected into every legacy Antibusy session. Adapt these prompts for Intention Relay, do not consume them as production prompts unchanged.

For the broader, superseded legacy audit, see [`../reference/archive/legacy-antibusy-audit/`](../reference/archive/legacy-antibusy-audit/00-manifest.md). It is archived and retained as research material and must not override this selected baseline without an explicit decision.
