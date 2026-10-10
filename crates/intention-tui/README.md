# intention-tui

The terminal application over the shared `intention-client`: one binary with
the revue fullscreen UI (`tui`), the interactive `repl`, and the headless
`run <PROMPT>` mode over one render-free state machine and the same client calls.

## Module map

- `src/app/` — `action.rs` (`Action`/`Effect` vocabulary), `state.rs` (state,
  accessors, `update`), `commands.rs` (the one command registry with its
  declared arguments, the command and argument word, and the menu's filter and
  ranking), `theme.rs` (the theme vocabulary, the settings round trip, and the
  picker transitions), `sessions.rs`/`browser.rs`/`run.rs`/
  `input.rs` and the layered `ctrl_c.rs`/`escape.rs` (transitions), `tests.rs`.
- `src/client_task.rs` — the one async `Effect` to client-call mapper.
- `src/tui/` — `mod.rs` (`run_blocking`, event handler, wheel mapping, wiring,
  the turn clock), `layout.rs` (the neutral row model and the transcript layout
  cache the front end owns, keyed by pane width, theme, and transcript
  epochs), `palette.rs` (the one
  colour system, resolved per frame from the active theme), `screens/`
  (`RevueView`, one file per surface: `chat.rs`, which also draws the theme
  picker in the band region, and the `sessions.rs` docked
  browser panel), `keymap.rs` (one mapping per state), `driver.rs` (thread,
  runtime, select loop), and `panes/` (one file per chat pane: `transcript.rs`,
  `welcome.rs`, `input.rs`, `status.rs`, the `commands.rs` hint band, the
  `theme.rs` picker panel, the `markdown.rs` content layout, and the `tools.rs`
  per-tool block dispatch).
- `src/main.rs`, `src/cli.rs`, `src/headless.rs`, `src/repl.rs` — the binary.

## One window

The front end has exactly one window and one base surface: the canvas, the chat
panel on it, and - while the core's screen carries the browser - the browser
docked to the window's bottom rows. `/sessions` opens the browser; `Esc` closes
it; `Enter` opens the session the cursor points at and returns to the same chat,
which keeps its transcript, its input, and its window. No command opens a second
window, and no mode switch replaces the frame: `screen` names which surface owns
the keyboard, not which window is drawn.

The chat opens on its welcome surface while no session is open - the product
lockup over a version/`AGENTS.md`/`MCPs`/`Skills` overview - and the interactive
front ends open no session at all at launch, not even the most recent one: a
session appears only for an explicit `--session`/`--continue`, `/new`, a selected
browser row, or the first prompt, which creates the session it needs. In the
chat the command hint band is `Esc`'s innermost layer: while it is open one
press closes it and touches nothing else. The theme picker is the next layer:
while its screen owns the window, `Esc` drops the preview and returns to the
committed theme, so the chat's cancel, clear, and exit arms are unreachable.
With the band closed and no picker open, `Esc` cancels a
live run in one press, clears a typed line into the recallable history on a
second consecutive press, and exits when the line is empty; `Ctrl+C` keeps its
layered interrupt, recall, and exit behavior.

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
a model placeholder, one row per buffer line with a `█` block cursor, and the run
status row under it), and the notice or error line. The status row reads
in words - `Ready · session … · build · Thinking… 3.2s · ctx @todo(core)` - and
the elapsed value is measured by this front end's turn clock and reported to the
core as `Action::ElapsedReported`, so the view stays a pure function of state.
A `\` immediately before `Enter` inserts a line break instead of submitting, and
the backslash never reaches the buffer; the block grows a row per buffer line
while the transcript gives up exactly those rows, with no line cap.

The command surface is one registry (`src/app/commands.rs`) and the hint band it
feeds: `/new` (Session), `/sessions` (Navigation), and `/theme` (Appearance) are
the registered commands, each carrying its name, one-line description, category,
action, and declared arguments, and the registry is the single source for the
submission path, the band, and every notice. `/theme` declares one optional
argument, `theme`, whose values are the theme vocabulary itself (`light` and
`dark`), and the parser reads only the registry: a submitted line resolves every
word against the declarations, so an unknown value answers
`unknown theme "purple" - expected light or dark`, a word past the last declared
argument answers `unexpected argument "now"`, and a declared value matches
however it is spelled (`DARK` names `dark`). A slash
is a command only as a line's *first* character: a slash anywhere else is
ordinary text, and a line whose first character is a blank is a turn. While the
input's first character is `/` and the caret sits in or immediately after a
word, the band opens directly above the input block as one more row region of
the one chat panel. It is one surface with two vocabularies: while the caret is
in the command word its rounded frame is titled `commands` with the three
columns `[Command] [Description] [Category]`, and while it is in a declared
argument word the frame is titled `arguments` with the same three cells
relabelled `[Value] [Description] [Argument]`. Its highlighted row is marked
with the sessions browser's own `> ` cursor and selected wash, it shows at most
five rows with an explicit `… and N more commands` (or `values`) row when the
filter matches more, and the first and trailing columns are sized to every row
the word can show so the list never shifts as the filter narrows. The band
spends the transcript's rows and no other's: the input block and the detail line
never move, and a window too short for both the band and the input shows no band
rather than taking the input's rows. The filter ranks a row that starts with the
typed text above one whose characters appear in order (`/ssn` finds
`/sessions`), ordered by rank and then by registry or declaration order so the
list never reorders while typing, and an empty filter matches everything.
`Up`/`Down` move the highlight while the band is open; the input history walks
only once it is closed. `Tab` commits the highlighted row - a command name or an
argument value plus one trailing space - and `Enter` completes it too, except on
the empty argument word an omitted optional argument opens: that band has
nothing to complete, so `Enter` runs the command itself (`/theme` opens the
picker) while `Tab` completes the value it highlights. A space after a complete
command name closes the band only for a command that declares no arguments, and
a command with a declared argument reopens the band on its argument word; a
space after a complete value closes it. The band closes when the slash is
removed, on `Esc`, or when nothing matches.

The colour theme is the terminal's own value and the daemon's. The light palette
is the warm off-white surface every session starts in; the dark palette is the
warm charcoal surface for a terminal that paints on a dark background. Both are
the same role table resolved by `palette::of(theme)`, which the frame calls once
per state's effective theme and hands to every pane, so a theme switch repaints
the whole window - the transcript layout cache included, because the cache is
keyed by the theme like the pane width. The committed theme is never the
terminal's guess: `config.toml` supplies the default through an optional
`[tui] theme = "light" | "dark"` (an absent section or key means light, and an
unknown spelling is a typed validation error), the daemon answers the effective
theme - the single-row `tui_settings` override in the state database, else the
configuration default, else light - and only the accepted reply to a selection
moves the committed value. `/theme light` and `/theme dark` select a theme
through the daemon; the selection previews while the round trip is in flight, a
rejection drops the preview and shows the error line, and a theme set records no
configuration revision and never rewrites the credential-bearing configuration
file.

`/theme` with no argument opens the theme picker as `Screen::Theme`: a small
panel in the same one-window layout, drawn in the chat panel's band region above
the input block (the region the command band uses), so the input block and the
detail line keep their rows instead of the panel docking at the window's bottom.
Its rounded frame carries the `Theme` title, one row per theme with the theme's
name and one-line description, the committed row marked with the sessions
browser's `> ` cursor and selected wash, and its own last inner row carries the
`enter apply | esc revert` legend. `Up`/`Down` preview the candidate they land
on: the whole window, transcript included, repaints through it and nothing is
persisted. `Enter` commits the highlighted theme through the daemon and closes;
`Esc` drops the preview, restores the committed theme, and closes. The picker's
`Esc` is its own layer, so the Esc layering is the open band, then the picker,
then a live run's cancel, then the input-clear arm, then the exit.

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
- Committed rows are laid out once into neutral rows and cached by pane width,
  theme, transcript version, reasoning-expansion epoch, and paired-tool-result
  count: an append lays out only the tail, a resize, a theme change, or a
  replacement (a snapshot, a trim, a session switch) lays the transcript out
  again, and only the visible window becomes widgets. The live tail a step is
  streaming - its reasoning segment and its answer segment - is laid out per
  frame through the same block functions the committed rows use and never
  cached, so nothing looks streaming and nothing changes style at commit time.
  A reasoning expansion and a newly committed tool result also replay the whole
  transcript, because both change a block the cached prefix holds.
- A tool row is paired with the row that shares its `tool_call_id`: the exchange
  is drawn once, as a framed block whose badge, plates, and structured result
  rows use the tool palette roles; an `execute` result is never rendered in any
  form.
- A daemon notice renders as its own block on the palette's notice pair, never
  as ordinary prose, and never through the markdown pipeline.
- Reasoning streams and commits as the same block: the wire's transient reasoning
  channel and the committed row that replaces it are laid out by the same block
  functions, so the live segment never looks provisional. A block longer than 15
  display rows shows its first fifteen and a marker naming the hidden ones, and
  each activation of that marker - or of `Ctrl+E` - reveals a hundred more.
- A value the wire does not carry is an explicit `@todo(core)` placeholder
  (`mode`, `model`, `ctx`, and the welcome overview's `AGENTS.md`, `MCPs`, and
  `Skills` chips), never a fabricated one. The command hint band and the theme
  picker are the surfaces with no placeholder at all: every band row - name,
  description, and category, or value, description, and argument - is a
  front-end registry fact, and every picker row is a theme fact, so nothing in
  either can be a `@todo(core)`.
- The same `@todo(core)` marker also names work the front end still performs
  that belongs to the core rather than a fact it lacks: the tool arguments and
  result payloads `src/tui/panes/tools.rs` parses, the session recency order
  and continue target `src/app/sessions.rs`, `src/app/browser.rs`, and
  `src/headless.rs` derive from raw fields, the transcript rows
  `src/app/run.rs` reconciles by value, the `--mode` spelling `src/cli.rs`
  repeats, and the calendar arithmetic `src/tui/screens/sessions.rs` runs on a
  raw Unix second. Each marker names what the core should own.

## Palette

`src/tui/palette.rs` is the single source of colours: `palette::of(theme)`
resolves one `Palette` value once per frame from the state's effective theme,
and every pane reads the roles of the reference it was handed. The light theme
is the warm off-white surface every session starts in; the dark theme is the
same warm family read on a warm charcoal canvas, keeping every role's hue and
lifting its value far enough to read. No pane constructs a colour of its own.

| Role | Light | Dark | Used by |
| --- | --- | --- | --- |
| `canvas` | `#F7F2E9` | `#16130F` | the window behind every panel |
| `panel` | `#FFFCF5` | `#1E1A15` | card and panel fills |
| `header_bg` | `#F3E7D5` | `#272219` | the tab row and the table header band |
| `tab_active_bg` | `#FCE2C4` | `#3A2A18` | the active tab's wash |
| `row_selected_bg` | `#FADFC0` | `#40291A` | the cursor row's wash |
| `row_alt_bg` | `#FBF5EC` | `#221D17` | every second row's zebra tint |
| `stub_bg` | `#FBE4DF` | `#33211E` | the `@todo(core)` empty state and welcome sub-blocks |
| `input_bg` | `#FFF4E2` | `#241E17` | the chat input buffer rows |
| `badge_bg` | `#EFE0C8` | `#322A20` | the badge wash of the input header and the welcome version |
| `badge_ink` | `#5C4632` | `#E4D3BC` | the label ink of a badge |
| `user_surface` | `#EDF1F5` | `#1C2229` | the wash of a user message card |
| `user_border` | `#A9BCD0` | `#4A5A6B` | the frame of a user message card |
| `user_label` | `#2F5D7C` | `#9CC2E0` | the `you` label of a user message card |
| `selection_bg` | `#D8E6F8` | `#26384C` | the fill of the transcript's selected display rows |
| `tool_surface` | `#FBEDDE` | `#241C15` | the wash of a tool block |
| `tool_border` | `#D8A76F` | `#8A5F32` | the frame of a tool block |
| `tool_label` | `#8F4A16` | `#F0A868` | the badge ink of a tool block's header |
| `tool_preview` | `#7A5330` | `#D3A87C` | the parsed result ink of a tool block |
| `notice_surface` | `#F7E8CE` | `#2A2113` | the wash of a daemon notice block |
| `notice_ink` | `#8A5A0B` | `#E3B15C` | the ink of a daemon notice block |
| `scroll_track` | `#E7DCCB` | `#2A241C` | the track of the transcript's native scrollbar |
| `scroll_thumb` | `#B08968` | `#6B543C` | the thumb of the transcript's native scrollbar |
| `divider_tint` | `#EFE6D8` | `#2A231B` | the divider bands between blocks |
| `border` | `#DCCFBB` | `#3A3229` | the neutral frames (chat, panes) |
| `accent` | `#F97316` | `#FB923C` | the primary accent: the focused frame, hotkeys |
| `accent_deep` | `#C2410C` | `#FDBA74` | accent text on an accent wash, the welcome lockup's product word |
| `scarlet` | `#DC2626` | `#F87171` | the secondary accent: active runs, failures |
| `markdown_heading` | `#9A3412` | `#FDA47A` | an assistant heading line |
| `markdown_code` | `#7C2D12` | `#FBC08A` | inline code and code blocks in an answer |
| `markdown_link` | `#1D4ED8` | `#93B4FB` | link text in an answer |
| `reasoning_body` | `#8A7A66` | `#B7A48C` | the dimmed chain-of-thought block |
| `reasoning_header` | `#6D5A44` | `#CDB79A` | the reasoning block's glyph and header |
| `reasoning_marker` | `#8A5A2B` | `#DEA263` | the expand affordance of a collapsed reasoning block |
| `timer_ink` | `#A16207` | `#E9C46A` | the measured elapsed value |
| `todo_ink` | `#8B7AA8` | `#C4B5E0` | the `@todo(core)` placeholder tone |
| `ink` | `#2A211A` | `#F0E7DA` | body text |
| `ink_muted` | `#7A6A58` | `#B9A992` | secondary text, hints, and the welcome lockup's trailing word |
| `ink_faint` | `#A8967F` | `#8F7E69` | dividers and separators |
| `success` | `#2F7D4F` | `#6EE7A0` | completed runs |
| `warning` | `#B45309` | `#F5B45C` | queued or interrupted work |
| `error` | `#DC2626` | `#F87171` | typed failures (the scarlet ink) |

Three rules hold per theme and one holds across them: every role is fully
opaque, because a terminal cannot show a transparent literal; within one theme
no two roles share a value, the one documented alias being `error`, which
spells `scarlet` exactly; within one theme every role carries its own value, so
a surface is never mistaken for its neighbour; and every role differs between
the two themes, so a frame is wholly one theme and never a half-switched
surface. The transcript layout cache is keyed by the theme like the pane width,
so a theme change replays the whole transcript exactly as a resize does. Colours
degrade, words do not: a terminal without truecolor maps each role of the active
theme to its nearest ANSI colour, so no role carries a state alone. Every use
pairs its colour with a glyph, a border, or a modifier - the cursor row carries
`> ` and bold, the active tab carries `⦿` and bold, the hotkey carries bold
before its muted action, a failure carries its status word - and the render
tests assert those characters and modifiers instead of colours.

## Adding a feature

Vocabulary goes in `src/app/action.rs`, transitions in the matching `src/app/`
file, wiring in `src/tui/` or `src/client_task.rs`.

### A new pane

1. Chat pane: create `src/tui/panes/<name>.rs` with a `pub(in crate::tui)`
   builder plus private helpers, declare `pub(super) mod <name>;` in
   `src/tui/panes/mod.rs`, and add it to the pane list in
   `src/tui/screens/chat.rs::chat_screen`, sizing its rows in that panel's
   budget so the transcript takes what is left.
2. Band-region panel: a panel that owns a screen but must not move the input
   block - the theme picker's shape - also lives in `src/tui/panes/<name>.rs`
   and is claimed by `src/tui/screens/chat.rs::hint_rows` and `hint_block` for
   its screen, with its own `Screen` variant and keymap arm. It takes the
   region above the input block the command band otherwise uses, so the input
   block and the detail line keep their rows.
3. Docked surface: add the variant to `Screen` in `src/app/mod.rs` and the
   transition that selects it, then create `src/tui/screens/<name>.rs` with a
   pure geometry function and a builder; `RevueView::render` shrinks the chat
   into the rows the docked surface does not use and places that surface in the
   window's bottom rows, so the window never changes and no surface paints over
   another. A surface that shows relative times takes the `now` the view
   received instead of reading a clock.
4. Geometry in one place: compute the surface's columns and rows from the window
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

1. Add the field to `Palette` in `src/tui/palette.rs` with a one-line doc, set
   its value in both `LIGHT` and `DARK`, and add the module table row that says
   where it is used; add it to the `roles` list in the palette tests so
   opacity, distinctness, the `error`/`scarlet` alias, and the cross-theme rule
   keep covering it.
2. Use it through `Text::fg`/`bg`, `Border::fg`/`bg`, or `Card::background`,
   reading it from the resolved `&'static Palette` the frame handed down, and
   pair it with a glyph or a modifier whenever it carries a state.
3. Never construct a colour outside the palette: `Color::rgb` is palette-only,
   and the views read roles, not values.

### A new key or wheel binding

1. Add or reuse an `Action` in `src/app/action.rs`.
2. Map the key in `src/tui/keymap.rs`: `key_action` dispatches on the screen the
   state reports, so the same key can mean one thing in the chat, another in
   the sessions browser, and another in the theme picker. A command is not a
   key: an unmodified letter is an input or filter character, and a command is
   a submitted line beginning with `/`.
3. Map a wheel notch in `src/tui/mod.rs::mouse_action`, and route the chat's
   left-button press, drag, and release through `Pointer` (the selection, a
   marker click's expansion, and the edge-drag fast scroll); mouse capture is
   on, and every other mouse event stays ignored.
4. Handle the action in `src/app/state.rs::update` and put its transition body
   in `src/app/sessions.rs`, `browser.rs`, `run.rs`, or `input.rs`; a layered
   key's arm lives in its own file (`ctrl_c.rs`, `escape.rs`).

### A new slash command

1. Add or reuse an `Action` in `src/app/action.rs` and handle it in
   `src/app/state.rs::update`.
2. Add one `CommandSpec` entry to `COMMANDS` in `src/app/commands.rs` with its
   name, description, category, action, and declared arguments. An argument is
   an `ArgumentSpec` naming itself, its `ValueSpec` values with their one-line
   descriptions, and whether it is required; the registry is the single source
   for the submission path, the band, the argument completions, and every
   notice, so nothing else spells a command name or value.
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
- Theme vocabulary, the settings round trip, and the picker transitions: inline
  `#[cfg(test)]` modules in `src/app/theme.rs`, plus the app transition tests in
  `src/app/tests.rs`.
- Rendering and geometry: `tests/tui_render.rs` over
  `intention_tui::tui::RevueView`. The assertions read characters, cells, and
  rows, never colours; `Pilot::find_text` reports a byte offset, so a test that
  needs a column converts it through the line (see `find_text` there).
- View, keymap, palette, and picker helpers: inline `#[cfg(test)]` modules in
  the screen, keymap, palette, and picker files
  (`src/tui/screens/sessions.rs`, `src/tui/keymap.rs`, `src/tui/palette.rs`,
  `src/tui/panes/theme.rs`).
- Markdown layout, the neutral rows, and the layout cache invariants: inline
  `#[cfg(test)]` modules in `src/tui/panes/markdown.rs` and
  `src/tui/panes/transcript.rs`.
- Process contract: `tests/terminal_e2e.rs` (modes, exit statuses, output).
- Binary helpers: inline `#[cfg(test)]` modules in `cli`, `headless`, `repl`.
