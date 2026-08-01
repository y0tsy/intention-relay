# M1 contract tests

The active M1 crates own test-first DTO, validation, migration, redaction, and
protocol compatibility evidence. Versioned fixtures prove supported legacy
error decoding, typed correlation/detail safety, persisted-event and protocol
compatibility, credential-free snapshots, and malformed wire rejection. The
quality self-test additionally proves adapter/protocol isolation,
composition-only implementation selection, and provider-SDK leakage gates.

Every fixture uses only recognizable fake credentials; no test fixture contains
a real credential.
