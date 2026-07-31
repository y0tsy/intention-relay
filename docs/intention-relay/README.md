# Intention Relay reference material

This directory contains the active legacy-derived reference material for rebuilding Intention Relay. It captures product behavior and agent policy that may inform the new implementation, but it does not prescribe the legacy architecture.

## Contents

- [`legacy-baseline/`](legacy-baseline/00-manifest.md): the selected product baseline. Use it to identify user-visible capabilities and known limitations that the rewrite must deliberately accept, change, or reject.
- [`legacy-antibusy-prompts/`](legacy-antibusy-prompts/README.md): a read-only source copy of the static prompts injected into every legacy Antibusy session. Adapt these prompts for Intention Relay, do not consume them as production prompts unchanged.

For the broader, superseded legacy audit, see [`../reference/legacy-antibusy-audit/`](../reference/legacy-antibusy-audit/00-manifest.md). It is retained as research material and must not override this selected baseline without an explicit decision.
