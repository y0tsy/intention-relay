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
