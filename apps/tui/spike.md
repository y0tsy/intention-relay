# Stack spike

Every claim below was executed before any product code was written. The spike
lived in a scratch directory outside the repository; this file records what it
proved and what it cost.

## Versions

| Component        | Version | Note                                                      |
| ---------------- | ------- | --------------------------------------------------------- |
| Node             | 26.12.1 | Active LTS from 2026-10-28; `engines` requires `>=26.4.0` |
| `@opentui/core`  | 0.6.8   | Zig core with prebuilt natives for eight targets          |
| `@opentui/react` | 0.6.8   | React reconciler, peer `react >= 19.2.0`                  |
| React            | 19.3.1  |                                                           |
| TypeScript       | 7.0.2   | Go-native compiler; no programmatic API before 7.1        |
| `tsx`            | 4.23.15 | Loader for `.tsx` under Node                              |
| `@biomejs/biome` | 2.6.2   | Formatter and linter                                      |
| `@types/node`    | 26.6.5  |                                                           |

## What ran

**Native load under Node.** `node --experimental-ffi` imports `@opentui/core`,
exposes `createCliRenderer` and `Yoga`. The FFI bootstrap inside the package
falls back between `bun:ffi` and `node:ffi`; under Node it requires the flag and
emits `ExperimentalWarning`, which `--disable-warning=ExperimentalWarning`
suppresses.

**Rendering in a real PTY.** An 80×24 frame was captured through the `pyte`-based
harness: border, `<scrollbox>` with a visible thumb, a row counter, and typed
input echoed back through `useKeyboard`.

**Testing.** `testRender(<App/>, { width, height })` from
`@opentui/react/test-utils` returns `renderOnce` and `captureCharFrame`, and the
assertion ran under `node --test` with `--import tsx`. Coverage via
`--experimental-test-coverage --test-coverage-lines=60` reports against the
source `.tsx`, not transpiled output.

**Type checking.** `tsc --noEmit` accepts the same files once
`jsxImportSource: "@opentui/react"` is set. Without it, `tsc` does not know
`box`, `text` or `scrollbox` and reports them as missing intrinsic elements.

## What this means for the build

- The runtime is Node, so the app starts through
  `node --experimental-ffi --import tsx`. The FFI flag is not optional.
- The JSX pragma belongs in `tsconfig.json`, not in per-file comments.
- Tests use the Node test runner and `node:assert`, not a third-party runner.
- A native package must resolve on every platform in the CI matrix. Windows
  resolves `@opentui/core-win32-x64`; the `tui` CI job runs there for exactly
  that reason.

## Test run time

`node --test` runs one process per file, so the suite is bounded by its slowest
file rather than by the sum of its tests. Measured on twelve cores, 204 tests:

| Configuration                                 | Wall time |
| --------------------------------------------- | --------- |
| `--test-isolation=none` (one process, serial) | 24.9 s    |
| default isolation, `--test-concurrency=6`     | 3.4 s     |

`--test-timeout=30000` fails a wedged test instead of wedging the run, and
`--test-force-exit` ends a run whose renderer outlives its test.

## Rejected alternative

Ink 8.0.0 renders correctly on both Node and Bun, and needs no native artifact,
but it provides no scrollbox, no pointer selection and no paste-aware input.
Ink 8 additionally stopped delivering unrecognized control sequences to
`useInput`, so wheel and selection input would have to be parsed from
`process.stdin` by hand. OpenTUI supplies those surfaces directly.
