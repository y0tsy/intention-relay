# intention-tui

The terminal application over the shared `intention-client`: one binary with
the revue fullscreen UI (`tui`), the interactive `repl`, and the headless
`run <PROMPT>` mode over one render-free state machine and the same client calls.

## Module map

- `src/app/` — `action.rs` (`Action`/`Effect` vocabulary), `state.rs` (state,
  accessors, `update`), `sessions.rs`/`browser.rs`/`run.rs`/`input.rs`
  (transitions), `tests.rs`.
- `src/client_task.rs` — the one async `Effect` to client-call mapper.
- `src/tui/` — `mod.rs` (`run_blocking`, event handler, wheel mapping, wiring,
  the turn clock), `layout.rs` (the neutral row model and the transcript layout
  cache the front end owns), `palette.rs` (the one colour system), `screens/`
  (`RevueView`, one file per surface: `chat.rs` and the `sessions.rs` docked
  browser panel), `keymap.rs` (one mapping per state), `driver.rs` (thread,
  runtime, select loop), and `panes/` (one file per chat pane: `transcript.rs`,
  `input.rs`, `status.rs`, the `markdown.rs` content layout, and the
  `tools.rs` per-tool block dispatch).
- `src/main.rs`, `src/cli.rs`, `src/headless.rs`, `src/repl.rs` — the binary.

## One window

The front end has exactly one window and one base surface: the canvas, the chat
panel on it, and - while the core's screen carries the browser - the browser
docked to the window's bottom rows. `/sessions` opens the browser; `Esc` closes
it; `Enter` opens the session the cursor points at and returns to the same chat,
which keeps its transcript, its input, and its window. No command opens a second
window, and no mode switch replaces the frame: `screen` names which surface owns
the keyboard, not which window is drawn.

Sessions are reachable only through `/sessions`: the chat has no sidebar, and
the docked panel carries the whole browser - the frame with the `Sessions` title,
the tab radio row, the search bar, the table, and the hotkey legend as the
panel's own last inner row. The panel spans the window's width, grows with its
result up to half the window, and keeps its bottom edge at the window's last
row; the chat container above it shrinks to the rows that leave exactly one
blank canvas gap row between the two. The list virtualizes inside the panel.

The chat panel itself is the transcript (user messages as labelled cards,
assistant answers as markdown under their `∷` marker, committed reasoning as a
separate dimmed block, tool exchanges as framed blocks, daemon notices as their
own `※` block), the input block (a badge header with the session's real mode and
a model placeholder, the input line with a `█` block cursor, and the run status
row under it), and the notice or error line. The status row reads
in words - `Ready · session … · build · Thinking… 3.2s · ctx @todo(core)` - and
the elapsed value is measured by this front end's turn clock and reported to the
core as `Action::ElapsedReported`, so the view stays a pure function of state.

The wheel moves both surfaces: in the chat it moves the transcript three display
rows a notch, and in the sessions browser it moves the cursor one row with the
virtualized window following it. The left button selects transcript display
rows - a press anchors, a drag extends, a release keeps, and a drag past an edge
(or a wheel notch while the button is held) scrolls five notches at once - and a
click on a collapsed reasoning block's marker expands it. The transcript
reserves one native scrollbar column, so a scrolled-back window shows that there
is more content above it. Mouse capture is on so these events reach the front
end, which means a terminal's own drag-selection needs Shift held. This front
end performs no copying of any kind: copying is the terminal emulator's
business, and the crate carries no clipboard path at all.

Rules the panes keep:

- User and assistant text both go through revue's `markdown` feature: a user row
  is a framed, padded card labelled `❯ you`, an assistant row is its reasoning
  block plus the markdown answer.
- Every marker in the window keeps the one shared `MARKER_GAP` column between
  its glyph and its text, and an answer's `∷` marker rides the answer's own
  first line with a hanging indent for every row after it.
- Exactly one blank row separates two consecutive transcript blocks - the user
  card, the reasoning block, the answer, and the notice line - and the display
  row window counts those gap rows like any other row.
- Committed rows are laid out once into neutral rows and cached by pane width
  and transcript version: an append lays out only the tail, a resize or a
  replacement (a snapshot, a trim, a session switch) lays the transcript out
  again, and only the visible window becomes widgets. The live provisional tail
  is one or two rows laid out per frame and never cached. A reasoning expansion
  and a newly committed tool result also replay the whole transcript, because
  both change a block the cached prefix holds.
- A tool row is paired with the row that shares its `tool_call_id`: the exchange
  is drawn once, as a framed block whose badge, plates, and structured result
  rows use the tool palette roles; an `execute` result is never rendered in any
  form.
- A daemon notice renders as its own block on the palette's notice pair, never
  as ordinary prose, and never through the markdown pipeline.
- Reasoning renders only when a committed row carries it (the wire streams no
  reasoning frame); a block longer than 15 display rows shows its first fifteen
  and a marker naming the hidden ones, and each activation of that marker - or
  of `Ctrl+E` - reveals a hundred more.
- A value the wire does not carry is an explicit `@todo(core)` placeholder
  (`mode`, `model`, `ctx`), never a fabricated one.

## Palette

`src/tui/palette.rs` is the single source of colours: a warm off-white canvas,
a juicy orange primary accent, a scarlet secondary accent, and the washes each
surface is tinted with. No pane constructs a colour of its own.

| Role | Value | Used by |
| --- | --- | --- |
| `CANVAS` | `#F7F2E9` | the window behind every panel |
| `PANEL` | `#FFFCF5` | card and panel fills |
| `HEADER_BG` | `#F3E7D5` | the tab row and the table header band |
| `TAB_ACTIVE_BG` | `#FCE2C4` | the active tab's wash |
| `ROW_SELECTED_BG` | `#FADFC0` | the cursor row's wash |
| `ROW_ALT_BG` | `#FBF5EC` | the zebra tint of every second row |
| `STUB_BG` | `#FBE4DF` | the `@todo(core)` empty state |
| `INPUT_BG` | `#FFF4E2` | the chat input line and the search bar |
| `BADGE_BG` | `#EFE0C8` | the badge wash of the input header |
| `BADGE_INK` | `#5C4632` | the label ink of an input-header badge |
| `USER_SURFACE` | `#EDF1F5` | the wash of a user message card |
| `USER_BORDER` | `#A9BCD0` | the frame of a user message card |
| `USER_LABEL` | `#2F5D7C` | the `you` label of a user message card |
| `SELECTION_BG` | `#D8E6F8` | the fill of the transcript's selected display rows |
| `TOOL_SURFACE` | `#FBEDDE` | the wash of a tool block: a light orange veil |
| `TOOL_BORDER` | `#D8A76F` | the frame of a tool block |
| `TOOL_LABEL` | `#8F4A16` | the badge ink of a tool block's header |
| `TOOL_PREVIEW` | `#7A5330` | the parsed result ink of a tool block |
| `NOTICE_SURFACE` | `#F7E8CE` | the wash of a daemon notice block |
| `NOTICE_INK` | `#8A5A0B` | the ink of a daemon notice block |
| `SCROLL_TRACK` | `#E7DCCB` | the track of the transcript's native scrollbar |
| `SCROLL_THUMB` | `#B08968` | the thumb of the transcript's native scrollbar |
| `DIVIDER_TINT` | `#EFE6D8` | the divider bands between blocks |
| `BORDER` | `#DCCFBB` | the neutral frames |
| `ACCENT` | `#F97316` | the browser frame, hotkeys, focus |
| `ACCENT_DEEP` | `#C2410C` | accent text on an accent wash |
| `SCARLET` | `#DC2626` | a live run's marker and destructive hints |
| `MARKDOWN_HEADING` | `#9A3412` | an assistant heading line |
| `MARKDOWN_CODE` | `#7C2D12` | code in an assistant answer |
| `MARKDOWN_LINK` | `#1D4ED8` | link text in an answer |
| `REASONING_BODY` | `#8A7A66` | the dimmed chain-of-thought block |
| `REASONING_HEADER` | `#6D5A44` | the reasoning block's glyph and header |
| `REASONING_MARKER` | `#8A5A2B` | the expand affordance of a collapsed reasoning block |
| `TIMER_INK` | `#A16207` | the measured elapsed value |
| `TODO_INK` | `#8B7AA8` | the `@todo(core)` placeholder tone |
| `INK` | `#2A211A` | body text |
| `INK_MUTED` | `#7A6A58` | secondary text and hints |
| `INK_FAINT` | `#A8967F` | dividers and separators |
| `SUCCESS` | `#2F7D4F` | completed runs |
| `WARNING` | `#B45309` | queued or interrupted work, stub notices |
| `ERROR` | `#DC2626` | typed failures (the scarlet ink) |

Colours degrade, words do not: a terminal without truecolor maps each role to
its nearest ANSI colour, so no role carries a state alone. Every use pairs its
colour with a glyph, a border, or a modifier - the cursor row carries `> ` and
bold, the active tab carries `⦿` and bold, the hotkey carries bold before its
muted action, a failure carries its status word - and the render tests assert
those characters and modifiers instead of colours.

## Adding a feature

Vocabulary goes in `src/app/action.rs`, transitions in the matching `src/app/`
file, wiring in `src/tui/` or `src/client_task.rs`.

### A new pane

1. Chat pane: create `src/tui/panes/<name>.rs` with a `pub(in crate::tui)`
   builder plus private helpers, declare `pub(super) mod <name>;` in
   `src/tui/panes/mod.rs`, and add it to the pane list in
   `src/tui/screens/chat.rs::chat_screen`, sizing its rows in that panel's
   budget so the transcript takes what is left.
2. Docked surface: add the variant to `Screen` in `src/app/mod.rs` and the
   transition that selects it, then create `src/tui/screens/<name>.rs` with a
   pure geometry function and a builder; `RevueView::render` shrinks the chat
   into the rows the docked surface does not use and places that surface in the
   window's bottom rows, so the window never changes and no surface paints over
   another. A surface that shows relative times takes the `now` the view
   received instead of reading a clock.
3. Geometry in one place: compute the surface's columns and rows from the window
   once, clamp the frame with `min_size`/`max_size`, and hand every block the
   inner width, so the table and the legend agree on their edges.

### A new browser tab or column

1. Tab: add the variant to `BrowserTab` in `src/app/browser.rs`, its `label`,
   its place in `BrowserTab::ALL`, its glyph in `ACTIVE_TAB`/`INACTIVE_TAB`
   handling, and its neighbours in `next`/`previous`. A tab whose core fact does
   not exist yet returns `Some(reason)` from `stub_reason`; the view renders
   that reason as the tab's empty state.
2. Column: add its share to `MODIFIED_PERCENT` and its minimum beside the
   others in `src/tui/screens/sessions.rs::columns`, then render it in
   `row_text`. Shares are percentages of the flexible width, so the table
   stretches with the card; the title column takes what they leave. A value the
   core does not carry yet is a `MISSING_*` placeholder with a `// @todo(core)`
   comment naming what is missing.
3. Filtering, ordering, and the cursor clamp stay in
   `SessionsBrowserState::refresh`: the view lays rows out and decides nothing.

### A palette role

1. Add the `const Color` to `src/tui/palette.rs` with a one-line doc and the
   module table row that says where it is used, and add it to the `ROLES` list
   in the palette tests so opacity and distinctness keep covering it.
2. Use it through `Text::fg`/`bg`, `Border::fg`/`bg`, or `Card::background`,
   and pair it with a glyph or a modifier whenever it carries a state.
3. Never construct a colour outside the palette: `Color::rgb` is palette-only,
   and the views import roles, not values.

### A new key or wheel binding

1. Add or reuse an `Action` in `src/app/action.rs`.
2. Map the key in `src/tui/keymap.rs`: `key_action` dispatches on the screen the
   state reports, so the same key can mean one thing in the chat and another in
   the sessions browser. A command is not a key: an unmodified letter is an
   input or filter character, and a command is a submitted line beginning with
   `/`.
3. Map a wheel notch in `src/tui/mod.rs::mouse_action`, and route the chat's
   left-button press, drag, and release through `Pointer` (the selection, a
   marker click's expansion, and the edge-drag fast scroll); mouse capture is
   on, and every other mouse event stays ignored.
4. Handle the action in `src/app/state.rs::update` and put its transition body
   in `src/app/sessions.rs`, `browser.rs`, `run.rs`, or `input.rs`.

### A new slash command

1. Add or reuse an `Action` in `src/app/action.rs` and handle it in
   `src/app/state.rs::update`.
2. Name it in the `submit_command` dispatch in `src/app/input.rs` and in the
   `KNOWN_COMMANDS` notice an unknown command shows.
3. Advertise it in `src/tui/panes/status.rs::DEFAULT_HINT`.

### A new client-backed behavior

1. Add the `Effect` (the request) and the `Action` (the answer) in
   `src/app/action.rs`.
2. Add the transition that returns the effect in `src/app/sessions.rs`,
   `run.rs`, or `input.rs`.
3. Add the one `client_task::perform` arm in `src/client_task.rs` that turns
   the effect into its client call and the outcome into its action; a
   subscription-shaped behavior adds an opener beside
   `client_task::open_subscription` instead, and an effect that only moves a
   front-end resource (`Subscribe`, `CloseStream`) returns `None` there while
   each driver handles it.
4. Show the new state with a pane, a status line, or neither.

### A new front end or mode

1. Add the command to `src/main.rs::dispatch` and parse it in `src/cli.rs`.
2. Drive `intention_tui::app::AppState` through `update`, performing request
   effects with `intention_tui::client_task::perform` and `open_subscription`.
   A fullscreen front end follows the wiring in `src/tui/mod.rs`.

### Where the tests go

- `AppState` transitions: `src/app/tests.rs`, without a terminal or daemon.
- Rendering and geometry: `tests/tui_render.rs` over
  `intention_tui::tui::RevueView`. The assertions read characters, cells, and
  rows, never colours; `Pilot::find_text` reports a byte offset, so a test that
  needs a column converts it through the line (see `find_text` there).
- View, keymap, and palette helpers: inline `#[cfg(test)]` modules in the
  screen, keymap, and palette files (`src/tui/screens/sessions.rs`,
  `src/tui/keymap.rs`, `src/tui/palette.rs`).
- Markdown layout, the neutral rows, and the layout cache invariants: inline
  `#[cfg(test)]` modules in `src/tui/panes/markdown.rs` and
  `src/tui/panes/transcript.rs`.
- Process contract: `tests/terminal_e2e.rs` (modes, exit statuses, output).
- Binary helpers: inline `#[cfg(test)]` modules in `cli`, `headless`, `repl`.
