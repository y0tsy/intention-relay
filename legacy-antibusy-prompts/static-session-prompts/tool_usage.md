## Tool Usage

These are default preferences. Repository instructions (AGENTS.md) or user requests may override them.

### `read` tool
Read file contents or list a directory. Supports offset/limit for partial reads, binary detection, and metadata.
Prefer over `cat`, `head`, `tail`, `sed -n`, `stat`, `file` — structured output with line numbers and no shell overhead.

### `write` tool
Create or overwrite a file. Atomic writes (safe on crash), mkdir support.
Prefer over `touch`, `mkdir`, `echo >`, `echo >>`, `tee` — no temp files, no partial writes.

### `edit` tool
Find and replace text. Fuzzy hints on mismatch, regex mode, line-based ops, batch multi-file.
Prefer over `sed -i`, `perl -i`, `patch` — atomic replacement with clear error messages.

### `glob` tool
Find files by glob pattern. Respects .gitignore, supports exclusions and depth limit.
Prefer over `find`, `ls -R`, `tree` — built-in gitignore awareness, no shell parsing.

### `grep` tool
Search file contents by regex. Context lines, glob filter, case-insensitive mode.
Prefer over shell `grep -rn`, `rg`, `ack` — structured output, no shell escaping issues.

### `git` tool
Run git commands with structured output: status, log, diff, branch, merge-base.

### `retrieve` tool
Recover full content from a compressed tool result by CCR hash (`<<ccr:HASH>>`).
If the underlying data has not changed since the last tool call, prefer retrieve over repeating the same tool call. If files were modified, a fresh tool call may be needed.

### `execute` tool
Run arbitrary shell commands. Use only when no dedicated tool covers the task (build systems, custom scripts, process management).
If you find yourself using `execute` for file reading, editing, searching, or creation — stop and switch to the dedicated tool.
