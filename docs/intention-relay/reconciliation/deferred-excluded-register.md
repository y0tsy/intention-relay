# Deferred and Excluded Register

This register is the sole coordination index for claims intentionally outside
current implementation scope. It does not authorize future work. Every item has
a disposition and a condition for reconsideration; “as recorded in concept” is
not sufficient evidence.

| ID | Topic | Disposition | Applicability | Reason | Reconsideration owner/trigger |
| --- | --- | --- | --- | --- | --- |
| EXC-001 | Live provider/profile reload | Defer | future | Requires explicit lifecycle, race and persistence contract | Provider owner, approved reload proposal |
| EXC-002 | Credential rotation during admitted work | Defer | future | Must not alter frozen meaning or private resource binding | Provider/security owner, rotation contract |
| EXC-003 | Provider health-check service | Defer | future | Readiness semantics and worker topology are not activated | Scheduler/provider owner, readiness design |
| EXC-004 | Pricing/budget policy | Exclude | future Mandate | Product ceilings are not part of direct Mandate admission | New product decision only |
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

All rows remain non-authorizing until a new approved decision updates this
register and the relevant owner architecture, roadmap, policy, and evidence.
