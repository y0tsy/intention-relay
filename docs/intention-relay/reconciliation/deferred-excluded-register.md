# Deferred and Excluded Register

This register is the sole coordination index for claims intentionally outside
current implementation scope. It does not authorize future work. Every item has
a disposition and a condition for reconsideration; “as recorded in concept” is
not sufficient evidence.

| ID | Topic | Disposition | Applicability | Reason | Reconsideration owner/trigger |
| --- | --- | --- | --- | --- | --- |
| EXC-001 | Live provider/profile reload | Adopt for M5+ (future direction) | future | Adopted as accepted post-M5 direction under ADR 0020; requires explicit lifecycle, race and persistence contract | [Architecture 25](../architecture/25-configuration-provider-control-plane.md), M5+ activating specification |
| EXC-002 | Credential rotation during admitted work | Adopt for M5+ (future direction) | future | Adopted as accepted post-M5 direction under ADR 0020; must not alter frozen meaning or private resource binding | [Architecture 25](../architecture/25-configuration-provider-control-plane.md), M5+ activating specification |
| EXC-003 | Provider health-check service | Adopt for M5+ (future direction) | future | Adopted as accepted post-M5 direction under ADR 0020; readiness semantics and worker topology are not activated | [Architecture 25](../architecture/25-configuration-provider-control-plane.md), M5+ activating specification |
| EXC-004 | Pricing/budget policy | Adopt for M5+ (future direction) | future Mandate | Adopted as accepted post-M5 direction under ADR 0020; product ceilings are not part of direct Mandate admission | [Architecture 25](../architecture/25-configuration-provider-control-plane.md), M5+ activating specification |
| EXC-005 | Dynamic ToolId/registry creation | Exclude | future Mandate | Fixed registry and one capability path are required | Architecture decision superseding 15 |
| EXC-006 | Sandbox/container authority | Exclude | all current packages | WorkspaceRoot and hooks are not OS security boundaries | Separate security architecture |
| EXC-007 | Remote continuation/provider state | Exclude | future Mandate | Recovery is local-history-first and never resumes old work | New explicit continuation architecture |
| EXC-008 | Rich MIME/raw kernel output | Defer | future kernel | Safe public projections remain text-only | Kernel owner, projection contract |
| EXC-009 | Physical deletion/GC of historical work | Defer | historical | Retention/deletion policy is outside non-destructive packages | Storage owner, retention decision |
| EXC-010 | Worker/process supervision topology | Defer | future | No production supervisor is activated by documentation | Runtime owner, supervision design |
| EXC-011 | Calendar/interval/time-zone/DST semantics | Defer | future scheduler | Scheduler contract currently covers readiness and fresh admission only | Scheduler owner, calendar package |
| EXC-012 | Autonomous background scheduling by kernel | Exclude | future kernel | Kernel background work cannot create authority or schedule runs | Architecture 20 owner |
| EXC-013 | User-created bounded MCP catalog as Mandate source | Exclude | future Mandate | Dynamic typed acquisition supersedes retained catalog rules | MCP owner, explicit supersession decision |
| EXC-014 | OS push/inbox notification delivery | Exclude | future activity/UI | Notifications are in-app protocol summaries only | UI product decision |
| EXC-015 | Activity numeric product ceilings | Defer | future activity/UI | Values require intrinsic/capacity/ordinary classification | Activity owner, M6 activation |
| EXC-016 | Automatic verifier authority inheritance | Exclude | future VerifierMandate | Authority must be explicit, target-scoped and revisioned | Verifier owner, explicit authority decision |
| EXC-017 | Parent/child indirect lifecycle authority | Exclude | future Mandate | Parentage is not lifecycle or mutation authority | Child/lifecycle owners |
| EXC-018 | Alternate adapter transport or gateway | Exclude | all future adapters | Shared typed client and one daemon ingress are required | Transport owner, new transport decision |
| EXC-019 | Historical synthetic activity/kernel/profile state | Exclude | M3/M4 | Historical bytes and meaning cannot be rewritten or reconstructed | Compatibility owner |
| EXC-020 | Unbounded raw maps/plugins in semantic DTOs | Exclude | future | Closed typed schemas and canonical validation are required | DTO owner, explicit schema decision |
| EXC-021 | Continual-harness model (rules, trigger capture, schedule, dossier, checkpoint, classes, bounds) | Adopt for M5+ (future direction) | future | Adopted as accepted post-M5 direction under ADR 0021; bounds classified intrinsic/capacity/product, never Mandate quotas | [Architecture 26](../architecture/26-continual-harness.md), M5+ activating specification |
| EXC-022 | Programmatic-caller policy and admission (origins, provenance, policies, corridors, counters, drafts) | Adopt for M5+ (future direction) | future | Adopted as accepted post-M5 direction under ADR 0022; historical-only for new Mandate work where conflicting | [Architecture 27](../architecture/27-programmatic-caller-policy-and-admission.md), M5+ activating specification |

All rows remain non-authorizing until a new approved decision updates this
register and the relevant owner architecture, roadmap, policy, and evidence.
Rows EXC-001..004 are adopted as accepted future directions by
[ADR 0020](../decisions/0020-configuration-provider-control-plane-directions.md),
EXC-021 by [ADR 0021](../decisions/0021-continual-harness-directions.md), and
EXC-022 by [ADR 0022](../decisions/0022-programmatic-caller-policy-directions.md);
all remain non-authorizing until a later M5+ activating specification.
