# M1 and M1+ contract tests

The active M1 crates own test-first DTO, validation, migration, redaction, and
protocol compatibility evidence. Versioned fixtures prove supported legacy
error decoding, typed correlation/detail safety, persisted-event and protocol
compatibility, credential-free snapshots, and malformed wire rejection.

M1+ extends `quality/self_test.py` with isolated expected-failure proofs for
policy-aligned workspace cycles, exact Cargo integration test targets,
provider SDK namespace ownership, and exact-file coverage exclusion
semantics. The architecture checker restricts `async_openai::` and
`openrouter_rs::` to their owner crate's private implementation; the coverage
checker validates ownership and report membership before changing a
denominator.

Every fixture uses only recognizable fake credentials; no test fixture contains
a real credential.
