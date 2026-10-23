# The terminal app

The terminal front end of the daemon: one session's committed transcript, the
model step in flight, and the input line that sends the next turn.

## Running

```sh
npm install
npm start
```

`npm start` runs `src/main.tsx` under Node with the FFI flag the OpenTUI core
requires. The app resolves the daemon program from `INTENTION_DAEMON_PROGRAM`,
and otherwise from `target/debug/intention-daemon` beside the workspace. The
session it opens runs in `INTENTION_WORKSPACE`, or in the directory the app was
started from. The theme preference is a platform config file (`tui.toml`) that
the picker writes.

## Sources

| Path            | What lives there                                                                                                           |
| --------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `src/main.tsx`  | the composition root: one renderer, one store, one application                                                             |
| `src/app/`      | the state machine: `state.ts`, `store.ts`, `action.ts`, `effect.ts`, `select.ts`, `commands.ts`, `history.ts`, `editor.ts` |
| `src/client/`   | the daemon client: transport, protocol, endpoint, and `stream.ts`, the committed state of one subscription                 |
| `src/ui/`       | the surfaces, the keymap, and the two subscription hooks                                                                   |
| `src/theme/`    | the palette and the config file it is read from                                                                            |
| `src/bindings/` | generated DTOs; `make bindings` rewrites them                                                                              |
| `src/testing/`  | fixtures, the daemon stand-in, and the store and render helpers                                                            |

## Rendering rules

The app is React 19 through `@opentui/react`, and every surface subscribes to
the value it paints:
- A selector returns a primitive or an identity the state already owns, never a
  fresh array or object. `src/ui/use-store.ts` compares what a selector hands
  back, so a new array on every read repaints forever.
- Committed rows are keyed on `SessionStreamState.revision`. The client mutates
  its rows in place, so the revision is what tells a view they moved, and a
  transient delta moves without moving it.
- A streamed answer is parsed in two parts. `splitBlocks` cuts the source at the
  last closed block and each parse is memoized on its source, so a frame
  re-parses the paragraph still being written and nothing before it.
- The composer, the status row, and the hint band hold their own subscriptions,
  so a keystroke or a clock tick repaints the row it belongs to.
- Every gesture is routed through `src/ui/keys.ts`: a surface decides what a
  key means in it, and no component reads the keyboard on its own.

`src/ui/reactivity.test.tsx` measures these rules: a parse spy records which
source each frame parsed, and the painted frame records what was laid out.

## Checks

```sh
npm test          # node --test, one process per file
npm run coverage  # the same run with the package's line floor
npm run typecheck # tsc --noEmit
npm run fmt       # biome format --write .
npm run lint      # biome lint .
```

The suite runs one process per file with `--test-concurrency=6`, so the wall
time is bounded by the slowest file rather than by the sum of the tests.
`--test-timeout` fails a wedged test instead of wedging the run, and
`--test-force-exit` ends a run whose renderer outlives its test.

`spike.md` records the stack decisions this package rests on.
