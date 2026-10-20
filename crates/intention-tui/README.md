# intention-tui

The terminal application over the shared `intention-client`: one binary with
the revue fullscreen UI (`tui`), the interactive `repl`, and the headless
`run <PROMPT>` mode over one render-free state machine and the same client calls.

## Module map

- `src/app/` — `action.rs` (`Action`/`Effect` vocabulary), `state.rs` (state,
  accessors, `update`), `commands.rs` (the one command registry with its
  declared arguments, the command and argument word, and the menu's filter and
  ranking), `theme.rs` (the palette and preference vocabularies and the picker
  transitions),
  `sessions.rs`/`browser.rs`/`run.rs`/
  `input.rs` and the layered `ctrl_c.rs`/`escape.rs` (transitions), `tests.rs`.
- `src/client_task.rs` — the one async `Effect` to client-call mapper.
- `src/tui/` — `mod.rs` (`run_blocking`, event handler, wheel mapping, wiring,
  the turn clock), `theme.rs` (the local theme file, the launch resolution and
  its precedence, and the terminal's background probe), `layout.rs` (the neutral
  row model and the transcript layout
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
lockup over the version the binary was built as - and the interactive
front ends open no session at all at launch, not even the most recent one: a
session appears only for an explicit `--session`/`--continue`, `/new`, a selected
browser row, or the first prompt, which creates the session it needs. In the
chat the command hint band is `Esc`'s innermost layer: while it is open one
press closes it and touches nothing else. The theme picker is the next layer:
while its screen owns the window, `Esc` drops the preview and returns to the
committed theme, so the chat's cancel, clear, and exit arms are unreachable.
With the band closed and no picker open, `Esc` stops a
live run in one press, clears a typed line into the recallable history on a
second consecutive press, and exits when the line is empty; `Ctrl+C` arms the
same stop on its first press and fires it on the second, then falls back to the
recall and exit layers. A run is live only while a live subscription observes
it - a terminal status, a stream the daemon closed, or a connection that failed
all end its liveness - so a daemon that died never keeps the stop, clear, and
exit arms unreachable. A live channel follows the session, not one run: opening a
session opens one `SubscribeSession` stream that carries every committed row of
that session, the active run's projection, and the active run's transient text,
whoever committed them - this window's turn, another window's turn, or the
daemon's own lifecycle - so two terminal windows on the same session both show a
turn sent in either one and either window can stop the run they both observe. A
lost stream asks for exactly one reconnect (bootstrap, open the session,
subscribe again) and a failed reconnect reports its typed error with the exit
arms still reachable. The line front ends read SIGINT the same way: `Esc`'s
and `Ctrl+C`'s stop is one `run.cancel`, the REPL stops the run and comes back
to its prompt (a signal at the prompt leaves the loop like `/quit`), and
`run <PROMPT>` reports the interrupted outcome with status 5.

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
the effective provider model its projection carries, one row per buffer line
with a `█` block cursor, and the run status row under it), and the notice or
error line. The status row reads in words - `Ready · session … · build ·
Working… 6.2s · ctx in 1200 out 300` - and names the run's live phase: `waiting`
from the moment the turn request leaves for the network, `thinking` when the
reasoning stream starts, `answering` when the answer stream starts, and
`working` while the run's committed projection reports a tool call in flight
past half a second (a shorter call leaves the phase it found). A terminal run
reads its committed facts - the duration from the projection's own times
(`Answered in 13s`) and the usage from the terminal outcome
(`ctx in 1200 out 300`, or `ctx not reported`) - and the interrupted and failed
runs keep their words. A live elapsed value reads in tenths of a second and the
committed duration in whole seconds; the live value is measured by this front
end's turn clock and reported to the core as
`Action::ElapsedReported`, so the view stays a pure function of state and the
half-second tool threshold measures reported time instead of a clock in the core.
A `\` immediately before `Enter` inserts a line break instead of submitting, and
the backslash never reaches the buffer; the block grows a row per buffer line up
to the share the panel's body sets - a third of the rows it competes for, and
never fewer than three buffer lines beside its own chrome - and then shows the
window of lines that keeps the caret's line visible, so the transcript gives up
exactly the rows the block takes and no more.

A paste is one edit and never a row per line. The terminal owns the clipboard and
delivers the text (this front end reads none), and the paste's own size decides
what the line shows: up to four hundred characters and eight lines enter the line
exactly as they arrived, and a larger paste is kept outside the buffer behind one
labelled line, `[pasted <head>... <n> lines]`, whose head is the paste's first
fifty characters with its line breaks and tabs read as spaces and the terminal's
framing dropped. `←` and `→` step over a label and `Backspace` takes it whole, so
no press lands inside it; submitting the line expands every label back into the
text it stands for, in the order the line spells them, so the session receives the
paste and the history keeps the line the user saw. A terminal that writes a paste
into the application without the bracketed-paste markers is handled by the front
end's own detector (`src/tui/paste.rs`): two presses within five milliseconds open
a run whose characters keep collecting across the pauses a terminal's chunked
delivery leaves, the `Enter` such a paste carried stays its line break, the `Enter`
the user presses afterwards submits the line, and the run reaches the line as one
paste.

The command surface is one registry (`src/app/commands.rs`) and the hint band it
feeds: `/new` (Session), `/sessions` (Navigation), `/theme` (Appearance),
`/session` (Session), and `/quit` (Session) are the registered commands, each
carrying its name, one-line description, category, action, and declared
arguments, and the registry is the single source for the fullscreen submission
path, the line REPL's parse, the band, and every notice. `/theme` declares one
optional argument, `theme`, whose values are the preference vocabulary itself
(`auto`, `light`, and `dark`), and `/session` declares one required free-form
argument whose word the transition parses as a durable identifier. The parser
reads only the registry: a submitted line
resolves every
word against the declarations, so an unknown value answers
`unknown theme "purple" - expected auto or light or dark`, a word past the last declared
argument answers `unexpected argument "now"`, and a declared value matches
however it is spelled (`DARK` names `dark`). A slash
is a command only as a line's *first* character: a slash anywhere else is
ordinary text, and a line whose first character is a blank is a turn. The line
REPL reads the same registry, so a line parses the same way in both front ends
and an unknown name lists the same set in both; a command that drives a
fullscreen surface (`/sessions`, `/theme`) is known there and answers that it
needs the fullscreen front end. While the
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
`Up`/`Down` move the highlight while the band is open, the caret across the
buffer's lines whenever a line waits that way, and the input history at the
buffer's first and last line. `Tab` commits the highlighted row - a command name or an
argument value plus one trailing space - and `Enter` completes an abbreviated
word the same way. A word that already spells the command (`/sessions`) or one of
the argument's declared values (`/theme light`, in whichever case the submission
path accepts) has nothing left to complete, so `Enter` runs the line on that
press; the empty argument word an omitted optional argument opens is the same
case, so `Enter` runs the command itself (`/theme` opens the picker) while `Tab`
completes the value it highlights. A space after a complete
command name closes the band only for a command that declares no arguments, and
a command with a declared argument reopens the band on its argument word; a
space after a complete value closes it. The band closes when the slash is
removed, on `Esc`, or when nothing matches.

The colour theme belongs to this front end: it is local presentation state, no
wire operation carries it, and the daemon never learns it. The light palette is
the warm off-white surface; the dark palette is a
neutral graphite surface whose only hue is its accent pair. Both are
the same role table resolved by `palette::of(theme)`, which the frame calls once
per state's effective theme and hands to every pane, so a theme switch repaints
the whole window - the transcript layout cache included, because the cache is
keyed by the theme like the pane width.

A launch resolves a *preference* - `auto`, `light`, or `dark` - and paints the
palette it names. The preference is the fullscreen UI's `--theme auto|light|dark`
flag when it was spelled; else the local `tui.toml`'s `[tui] theme` declaration,
the choice the user already made; else `auto`. A spelled `light` or `dark`, by
flag or by file, beats every form of environment detection; `auto` - and an
absent file, section, or key - paints what detection answers: the `COLORFGBG`
variable a terminal exports when it exists, else a foreground-free `OSC 11`
query of the terminal background with a 15 ms deadline, and the zero-configuration
route that needs no platform theme lookup, else the dark palette. The dark
fallback is deliberate: a terminal that reports neither its background nor
`COLORFGBG` is usually a dark one, which is the profile terminal emulators and
terminal applications ship by default. Detection runs once per launch even when a
spelled palette wins, because the picker's `Auto` row previews what returning to
detection would paint, and the probe is skipped when standard input or output is
not a terminal.

The local file is `tui.toml` beside the
configuration directory's `config.toml` (Linux `$XDG_CONFIG_HOME` or
`~/.config`, macOS `~/Library/Application Support`, Windows `%APPDATA%`), it
carries one `[tui] theme` declaration, and an unknown spelling is a typed
validation error rather than a silent fallback. `/theme auto`, `/theme light`,
and `/theme dark` select a preference; the selection commits in memory at once
and asks for one rewrite
of the local file after the frame it produced, so the next launch reads the same
preference and no daemon round trip gates the repaint. A stored `auto` is what
clears a spelled-out palette. A failed rewrite reports its
typed error while the committed selection stands, and a selection that matches
the committed preference writes nothing. A theme change records no configuration
revision and never rewrites the credential-bearing configuration file.

`/theme` with no argument opens the theme picker as `Screen::Theme`: a small
panel in the same one-window layout, drawn in the chat panel's band region above
the input block (the region the command band uses), so the input block and the
detail line keep their rows instead of the panel docking at the window's bottom.
Its rounded frame carries the `Theme` title, one row per preference - `Auto`,
`Light`, `Dark`, the first being the way back to detection without editing the
file by hand - with the preference's name and one-line description, the
highlighted row marked with the sessions
browser's `> ` cursor and selected wash, and its own last inner row carries the
`enter apply | esc revert` legend. `Up`/`Down` preview the candidate they land
on: the whole window, transcript included, repaints through the palette that
preference names (`Auto` through the palette detection answered at this launch)
and nothing is
persisted. `Enter` commits the highlighted preference, writes the local file the
next
launch reads, and closes;
`Esc` drops the preview, restores the committed preference, and closes. The picker's
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
  block plus the markdown answer when the step answered, and a reasoning-only
  row - a step that reasoned and called a tool without answering - is its
  reasoning block alone.
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
  is drawn once, as a framed block whose badge, typed argument facts, typed
  result payload, lifecycle status, and approved metadata use the tool palette
  roles; nothing re-reads the arguments document or the result text, and an
  `execute` result is never rendered in any form.
- A daemon notice renders as its own block on the palette's notice pair, never
  as ordinary prose, and never through the markdown pipeline.
- Reasoning streams and commits as the same block: the wire's transient reasoning
  channel and the committed row that replaces it are laid out by the same block
  functions, so the live segment never looks provisional. A block longer than 15
  display rows shows its first fifteen and a marker naming the hidden ones, and
  each activation of that marker - or of `Ctrl+E` - reveals a hundred more.
- Every surface reads a fact the daemon publishes, and a surface with no fact
  to read renders nothing: the welcome overview lists the version alone, and
  the input block draws no `mode` or `model` badge until a session snapshot
  names them. No pane draws a placeholder for a fact the core does not carry.
  The command hint band and the theme picker are the surfaces with no
  placeholder at all: every band row - name, description, and category, or
  value, description, and argument - is a front-end registry fact, and every
  picker row is a theme fact. The model badge and the usage segment are
  projection facts: the badge names the effective model the session snapshot
  carries, and the segment reads the terminal outcome's usage
  (`ctx in 1200 out 300`, or `ctx not reported`).
- The crate's `@todo(hack)` markers name the work the adapter or the client
  still owes.

## Palette

`src/tui/palette.rs` is the single source of colours: `palette::of(theme)`
resolves one `Palette` value once per frame from the state's effective theme,
and every pane reads the roles of the reference it was handed. The light palette
is the warm off-white surface; the dark palette is a
neutral graphite ramp whose only hue is the accent pair, and it is what a
terminal that reports no background is painted with. No pane constructs a
colour of its own.

| Role | Light | Dark | Used by |
| --- | --- | --- | --- |
| `canvas` | `#F7F2E9` | `#0A0A0C` | the window behind every panel |
| `panel` | `#FFFCF5` | `#101012` | card and panel fills |
| `header_bg` | `#F3E7D5` | `#161618` | the tab row and the table header band |
| `tab_active_bg` | `#FCE2C4` | `#2F2F31` | the active tab's wash |
| `row_selected_bg` | `#FADFC0` | `#333335` | the cursor row's wash |
| `row_alt_bg` | `#FBF5EC` | `#0D0D0F` | every second row's zebra tint |
| `stub_bg` | `#FBE4DF` | `#1C1C1E` | the empty state of a list the core reports no rows for |
| `input_bg` | `#FFF4E2` | `#131315` | the chat input buffer rows |
| `badge_bg` | `#EFE0C8` | `#28282A` | the badge wash of the input header and the welcome version |
| `badge_ink` | `#5C4632` | `#CACACC` | the label ink of a badge |
| `user_surface` | `#EDF1F5` | `#252527` | the wash of a user message card |
| `user_border` | `#A9BCD0` | `#474749` | the frame of a user message card |
| `user_label` | `#2F5D7C` | `#828284` | the `you` label of a user message card |
| `selection_bg` | `#D8E6F8` | `#3A3A3C` | the fill of the transcript's selected display rows |
| `tool_surface` | `#FBEDDE` | `#1F1F21` | the wash of a tool block |
| `tool_border` | `#D8A76F` | `#525254` | the frame of a tool block |
| `tool_label` | `#8F4A16` | `#C2C2C4` | the badge ink of a tool block's header |
| `tool_preview` | `#7A5330` | `#8A8A8C` | the parsed result ink of a tool block |
| `notice_surface` | `#F7E8CE` | `#222224` | the wash of a daemon notice block |
| `notice_ink` | `#8A5A0B` | `#BABABC` | the ink of a daemon notice block |
| `scroll_track` | `#E7DCCB` | `#2B2B2D` | the track of the transcript's native scrollbar |
| `scroll_thumb` | `#B08968` | `#4A4A4C` | the thumb of the transcript's native scrollbar |
| `divider_tint` | `#EFE6D8` | `#19191B` | the divider bands between blocks |
| `border` | `#DCCFBB` | `#414143` | the neutral frames (chat, panes) |
| `accent` | `#F97316` | `#FB923C` | the primary accent: the focused frame, hotkeys |
| `accent_deep` | `#C2410C` | `#FDBA74` | accent text on an accent wash, the welcome lockup's product word |
| `scarlet` | `#DC2626` | `#5E5E60` | the secondary accent: active runs, failures |
| `markdown_heading` | `#9A3412` | `#FAFAFC` | an assistant heading line |
| `markdown_code` | `#7C2D12` | `#B2B2B4` | inline code and code blocks in an answer |
| `markdown_link` | `#1D4ED8` | `#AAAAAC` | link text in an answer |
| `reasoning_body` | `#8A7A66` | `#5A5A5C` | the dimmed chain-of-thought block |
| `reasoning_header` | `#6D5A44` | `#7A7A7C` | the reasoning block's glyph and header |
| `reasoning_marker` | `#8A5A2B` | `#A2A2A4` | the expand affordance of a collapsed reasoning block |
| `timer_ink` | `#A16207` | `#9A9A9C` | the measured elapsed value |
| `ink` | `#2A211A` | `#DADADC` | body text |
| `ink_muted` | `#7A6A58` | `#A6A6A8` | secondary text, hints, and the welcome lockup's trailing word |
| `ink_faint` | `#A8967F` | `#626264` | dividers and separators |
| `success` | `#2F7D4F` | `#6A6A6C` | completed runs |
| `warning` | `#B45309` | `#727274` | queued or interrupted work |
| `error` | `#DC2626` | `#5E5E60` | typed failures (the scarlet ink) |

Three rules hold per theme and one holds across them: every role is fully
opaque, because a terminal cannot show a transparent literal; within one theme
no two roles share a value, the one documented alias being `error`, which
spells `scarlet` exactly; within one theme every role carries its own value, so
a surface is never mistaken for its neighbour; and every role differs between
the two themes, so a frame is wholly one theme and never a half-switched
surface. The dark theme is additionally monochrome: every role but `accent` and
`accent_deep` is a neutral grey, so a state there reaches the reader through its
word and glyph, never through a colour of its own. The transcript layout cache
is keyed by the theme like the pane width,
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
   handling, and its neighbours in `next`/`previous`. A tab selects rows by a
   fact the session summary carries, so a tab whose fact the wire does not
   publish does not exist.
2. Column: add its share to `MODIFIED_PERCENT` and its minimum beside the
   others in `src/tui/screens/sessions.rs::columns`, then render it in
   `row_text`. Shares are percentages of the flexible width, so the table
   stretches with the card; the title column takes what they leave. A column
   renders a fact the session summary carries, never a fabricated value.
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
   key's arm lives in `src/app/arm.rs`, beside the key layers `ctrl_c.rs` and
   `escape.rs`.

### A new slash command

1. Add or reuse an `Action` in `src/app/action.rs` and handle it in
   `src/app/state.rs::update`.
2. Add one `CommandSpec` entry to `COMMANDS` in `src/app/commands.rs` with its
   name, description, category, action, and declared arguments. An argument is
   an `ArgumentSpec` naming itself, its `ValueSpec` values with their one-line
   descriptions, and whether it is required; an argument that declares no value
   is free-form and its word is the transition's to parse. The registry is the
   single source for the submission path, the REPL's parse, the band, the
   argument completions, and every notice, so nothing else spells a command
   name or value. Both front ends dispatch `CommandAction` in an exhaustive
   match, so a new action reaches both.
3. Advertise it in `src/tui/panes/status.rs::DEFAULT_HINT`.

### A new client-backed behavior

1. Add the `Effect` (the request) and the `Action` (the answer) in
   `src/app/action.rs`.
2. Add the transition that returns the effect in `src/app/sessions.rs`,
   `run.rs`, or `input.rs`.
3. Add the one `client_task::perform` arm in `src/client_task.rs` that turns
   the effect into its client call and the outcome into its action; a
   subscription-shaped behavior adds an opener beside
   `client_task::open_session_subscription` instead, and an effect that only
   moves a front-end resource (`Subscribe`) returns `None` there while each
   driver handles it.
4. Show the new state with a pane, a status line, or neither.

### A new front end or mode

1. Add the command to `src/main.rs::dispatch` and parse it in `src/cli.rs`.
2. Drive `intention_tui::app::AppState` through `update`, performing request
   effects with `intention_tui::client_task::perform` and
   `open_session_subscription`. A fullscreen front end follows the wiring in
   `src/tui/mod.rs`.

### Where the tests go

- `AppState` transitions: `src/app/tests.rs`, without a terminal or daemon.
- Theme vocabulary, the picker transitions, and the app's selection transitions:
  inline
  `#[cfg(test)]` modules in `src/app/theme.rs`, plus the app transition tests in
  `src/app/tests.rs`.
- The local theme file, the launch precedence, and the terminal probe: inline
  `#[cfg(test)]` modules in `src/tui/theme.rs`.
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
