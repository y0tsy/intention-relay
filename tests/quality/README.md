# M0 quality checks

This directory is reserved for quality-gate integration evidence. M0 keeps its
isolated expected-failure inputs under `quality/fixtures/` and runs them through
`make quality-self-test`, because deliberately invalid source must never be a
normal workspace member.

M1 may add crate-specific integration tests under the applicable crate while
preserving the repository-level quality checks here.
