# M1 and M1+ contract tests

The active M1 crates own test-first DTO, validation, migration, redaction, and
protocol compatibility evidence. Versioned fixtures prove supported legacy
error decoding, typed correlation/detail safety, persisted-event and protocol
compatibility, credential-free snapshots, and malformed wire rejection.

M1+ extends `quality/self_test.py` with isolated expected-failure proofs for
policy-aligned workspace cycles, exact Cargo integration test targets,
signature-aware public API resource leaks, and exact-file coverage exclusion
semantics. The public API checker reads pinned-nightly rustdoc JSON; the coverage
checker validates ownership and report membership before changing a denominator.

Every fixture uses only recognizable fake credentials; no test fixture contains
a real credential.
