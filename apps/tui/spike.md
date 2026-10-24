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

## The reactive layer

The renderer is `@opentui/react`'s React 19 reconciler over a store that views
subscribe to with selectors. No dependency was added for it: `useSyncExternalStore`
is part of React, and the selector layer is thirty lines in `src/ui/use-store.ts`.

**Solid was the alternative.** `@opentui/solid` 0.6.8 ships the same hooks, but
its `preload.node.js` throws `@opentui/solid/preload is Bun-only`, and its JSX
runtime applies props once, so every dynamic value would have to be an accessor.
Reaching that runtime under Node and `tsx` means a custom Babel loader, which the
Windows lane would carry as well. `@opentui/vue` 0.1.25 trails the core at 0.6.8
and was never a candidate.

**A streamed answer is parsed in two parts.** `splitBlocks` cuts the source at
the last closed block, so a frame re-parses the paragraph still being written and
leaves the settled prefix to its memoized block. The parse is memoized on the
source string, so a repaint that hands the same text back paints the tokens it
already has.

**Row identity is a revision, not the array.** The client mutates its rows in
place, so a view comparing the array it was handed cannot see an appended row.
`SessionStreamState.revision` counts how many times the committed rows moved,
`AppState.transcript` copies them once per revision, and a transient delta moves
without moving it.

**A scrollbox needs a basis.** In a bordered column, `flexGrow={1}` alone lets
the scrollbox take the rows its content asks for and push the panels below it
out of the frame. `flexBasis={0}` with `flexGrow={1}` gives it the free space and
nothing else.

## The kernel boundary

The front end is one composition of bricks, not a set of components that each
count rows. `src/kit` owns what needs measurement: display-cell text, table
columns, pane chrome, the footer plan, the split-footer surface, and printing.
`src/ui` names content and reads the columns and rows its parent hands down
through React context. `src/kit/boundaries.test.ts` enforces both directions -
no renderer field, display width, `Math`, or deep kernel import in a surface; no
surface import in the kernel - and proves the guard catches a forbidden line.

**A brick provides, it never paints a number.** `Pane` subtracts its border and
padding and provides the content columns; `Hanging` subtracts a marker; the
footer plan provides each slot's rows. A child reads a handle, so a wrapped line
or a hidden column is decided once, where the chrome is known.

**Renderer internals live behind one module.** The split surface needs the
renderer's private offset, its pending native mode, and its external output
queue. Those casts are in `src/kit/screen/surface.ts`; the printer asks
`windowSettled`, `pinSurface`, and `replayWindow`, and no surface sees a private
field. `Screen` keeps the printed-row count, and `FooterFrame` reports the
footer height, so the application has no geometry and no renderer handshake.

**The printer takes blocks, not selectors.** It receives a session key, the
blocks to print, a palette, and a render prop. The app adapter (`src/ui/Printer.tsx`)
maps store blocks to that shape, which is what lets the fold, the chunking, and
the scrollback portal live in the kernel without importing app state.

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
