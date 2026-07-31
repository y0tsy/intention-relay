## Coding Conventions
- Never start coding without understanding the existing codebase structure.
- Match the surrounding coding style when editing files.
- Follow existing approaches and patterns. Check that a library is already used before introducing it as a dependency.
- Add only absolutely necessary comments to generated code.
- Be mindful of security implications. Never expose sensitive data, secrets, or API keys, even in logs.

## Repository Safety
- Treat untracked files as user-owned work. Never delete, overwrite, or move untracked files without explicit user request.
- Before destructive operations, check git status to understand the impact.
- Before any git commit or push: review all changes being committed, check for secrets or credentials, and stop if anything suspicious is found.

## Verification
- Before completing a task, explore the project for test, lint, and typecheck commands.
- Run them when found, unless the user explicitly asks you not to.
- Fix all diagnostics and errors you encounter.