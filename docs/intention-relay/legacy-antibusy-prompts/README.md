# Legacy Antibusy static session prompts

This directory is a read-only reference copy of the prompt text that legacy Antibusy injects into every newly created session, regardless of project, session mode, model, or runtime configuration.

## Provenance

- **Source repository:** `/home/data/antibusy`
- **Source revision:** `8604fde0566d4dfadf8124e0724c5a82df3b89de`
- **Assembly site:** `crates/tauri-app/src/state.rs`, `build_system_prompt()`
- **Static-source binding:** `crates/prompt/src/sources/static_source.rs`
- **Copied on:** 2026-07-31

`build_system_prompt()` unconditionally adds these sources:

| Priority | Prompt source | Legacy source file | Reference copy |
| ---: | --- | --- | --- |
| 100 | `IdentitySource` | `crates/prompt/src/sources/prompts/identity.md` | `static-session-prompts/identity.md` |
| 95 | `GuidelinesSource` | `crates/prompt/src/sources/prompts/guidelines.md` | `static-session-prompts/guidelines.md` |
| 85 | `ToolUsageSource` | `crates/prompt/src/sources/prompts/tool_usage.md` | `static-session-prompts/tool_usage.md` |
| 65 | `CodingConventionsSource` | `crates/prompt/src/sources/prompts/coding_conventions.md` | `static-session-prompts/coding_conventions.md` |

The legacy `PromptBuilder` sorts sources by descending priority and joins them with `\n\n---\n\n`. The total assembled system prompt is limited to 100,000 characters.

## Not copied

The following prompt contributions are not static text used universally by every session, so they are intentionally excluded from this reference:

- project-specific `AGENTS.md` and `.antibusy/memory.md` content;
- session/project/date/Git-derived sources;
- Build and Plan mode templates, which vary by session mode;
- VFR instructions, which depend on configuration;
- few-shot examples, which depend on configuration/freshness;
- MCP prompts and active-skill additions, which depend on runtime configuration and state.

The exact source files above were unchanged in the legacy working tree when this copy was made.

## Adaptation map

The four static sources are adapted, never consumed unchanged
([architecture 30](../architecture/30-instruction-sources-and-system-context.md),
[ADR 0043](../decisions/0043-instruction-sources-and-system-context.md)):

| Legacy source | Instruction kind in architecture 30 | What changes |
| --- | --- | --- |
| `IdentitySource` (`identity.md`) | `Identity` | The product framing becomes Intention Relay: a local-first single-user daemon with typed client adapters, not the legacy Tauri-only application. |
| `GuidelinesSource` (`guidelines.md`) | `Guidelines` | Behavioural guidance keeps its advisory meaning; it cannot create authority, and repository content, tool output, and fetched material stay untrusted data. |
| `ToolUsageSource` (`tool_usage.md`) | `ToolUsage` | Tool names and semantics follow the frozen Intention Relay registry (`read`, `write`, `edit`, `execute`, `glob`, `grep`) and the `WorkspaceRoot` rules, not the legacy tool set. |
| `CodingConventionsSource` (`coding_conventions.md`) | `CodingConventions` | Conventions stay project-facing guidance and cannot grant permissions; the repository's Makefile quality pipeline stays the only quality authority. |

The legacy priority order becomes the fixed canonical assembly order of
architecture 30's profile and projection (identity, guidelines, tool usage,
coding conventions), and the legacy 100,000-character cap becomes the total
projection bound. The contributions named under "Not copied" keep separate
owners: workspace `AGENTS.md` is the `ProjectInstructions` source of architecture
30, memory-derived material stays with architecture 21, mode templates with
architecture 07, VFR instructions with architecture 06, and few-shot examples,
date/Git-derived context, and MCP-provided prompts are recorded in the deferred
and excluded register (`EXC-059..065`).
