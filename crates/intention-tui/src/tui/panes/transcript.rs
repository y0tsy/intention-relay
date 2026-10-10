//! The transcript pane: one padded block per committed row.
//!
//! A committed row becomes one or more blocks, and exactly one blank row
//! ([`crate::tui::layout::append_blocks`]) separates two consecutive blocks:
//!
//! - a committed **user** row is a card: a labelled frame on [`Palette::user_surface`]
//!   whose content is laid out through the same markdown pipeline as an answer,
//!   because a user message is prose too (code fences, bold, and lists render);
//! - a committed **assistant** row is its reasoning block (when the row carries
//!   one) plus the answer block, which opens with its own monochrome marker;
//! - a committed **tool** row is dispatched by its `tool_id` in [`super::tools`],
//!   where the exchange a call and its result form is drawn once, at the call's
//!   row, and its result row contributes no block;
//! - a committed **notice** row stays a plain line;
//! - the live **provisional** tail is the step in flight: its reasoning block
//!   and the answer block after it, each laid out through the very function
//!   the committed assistant row uses (same marker, hanging indent, collapse
//!   rule, inks, and wash), so no live row looks provisional and none changes
//!   when the commit arrives.
//!
//! The gap rule is one constant and one helper, so the user card is padded from
//! the reasoning above the answer, the reasoning is padded from the answer, and
//! an answer always starts after a blank row. The transcript window counts
//! display rows, gaps included: it shows the newest `height - 2` rows and moves
//! `state.scroll()` rows towards the older ones, one native scrollbar column
//! stays reserved beside them, and the selected display rows are painted on
//! [`Palette::selection_bg`].
//!
//! Committed rows are laid out once and kept in the [`TranscriptLayoutCache`]
//! the front end owns; this pane asks that cache for the rows of the current
//! transcript version and materialises `RichText` for the visible window only.
//! The provisional tail is laid out per frame and never enters the cache; that
//! is a caching decision, not a style one, because its rows are identical to
//! the committed row's. Every frame publishes the window it painted
//! ([`TranscriptWindow`], live reasoning marker included) so the next mouse
//! event can hit-test it.

use std::cell::RefCell;
use std::ops::Range;

use intention_proto::{MessageKindDto, MessageProjectionDto};
use revue::style::Color;
use revue::text::{TextWrapper, WrapMode};
use revue::widget::{Border, ScrollView, hstack, vstack};

use crate::app::AppState;
use crate::tui::layout::{
    CONTINUATION_INDENT, LaidOutRow, MARKER_GAP, MessageBlocks, RowBuilder, RowStyleId,
    TranscriptLayoutCache, TranscriptWindow, append_blocks, box_bottom, box_content, box_top,
    display_width, marker_text,
};
use crate::tui::palette::Palette;
use crate::tui::panes::{markdown, tools};

/// The glyph and label a user card's frame carries.
const USER_GLYPH: &str = "❯";

/// The label the user card's marker introduces.
const USER_LABEL: &str = "you";

/// The glyph that opens a committed answer block.
///
/// U+2237 PROPORTION: a member of the reasoning header's own family (U+2234
/// THEREFORE, `∴`) that is visibly distinct from it - two vertically stacked
/// dots against the reasoning glyph's triangle of dots - and rare, monochrome,
/// and free of any emoji presentation, so no terminal paints it in colour and
/// no U+FE0E text-presentation selector is needed. It is one column wide under
/// `char_width`, which is what the block's hanging indent is measured through,
/// and no variant of it appears anywhere else in the interface.
const ANSWER_GLYPH: &str = "∷";

/// The columns a user card's frame and padding take: the two borders and one
/// padding column beside each.
const CARD_CHROME_COLUMNS: usize = 4;

/// The glyph that opens a committed reasoning block.
const REASONING_GLYPH: &str = "∴";

/// The label the reasoning block's marker introduces.
const REASONING_LABEL: &str = "reasoning";

/// The display rows a reasoning block shows before it collapses.
const REASONING_VISIBLE_ROWS: usize = 15;

/// The expand affordance a collapsed reasoning block's marker opens with.
const REASONING_EXPAND: &str = "▸";

/// The glyph that opens a committed notice block.
///
/// U+203B REFERENCE MARK: monochrome, one column wide under `char_width`, free
/// of any emoji presentation, and spent nowhere else in the interface.
const NOTICE_GLYPH: &str = "※";

/// The columns the transcript pane's own frame takes.
const TRANSCRIPT_FRAME_ROWS: u16 = 2;

/// The columns the transcript keeps for its native scrollbar.
///
/// The gutter is always reserved, whether or not the transcript is taller than
/// the window: the wrap width then never changes when the transcript crosses
/// the window's height, and revue's own scrollbar paints the column only while
/// there is content beyond the window.
const SCROLLBAR_COLUMNS: usize = 1;

/// Returns the transcript pane: the committed blocks, then the provisional tail.
///
/// Every block is laid out at the content width the pane actually has, and the
/// window counts those display rows rather than logical blocks: it shows the
/// newest `height - 2` rows and moves `state.scroll()` rows towards the older
/// ones. `top` is the screen row the window's first display row sits on, which
/// the pane publishes with the rest of its geometry. The committed rows come
/// from `cache`; only the window's rows become widgets. `palette` is the
/// resolved palette of the frame, and every row - committed or provisional -
/// paints its roles.
pub(in crate::tui) fn transcript_pane(
    state: &AppState,
    top: u16,
    height: u16,
    width: u16,
    cache: &RefCell<TranscriptLayoutCache>,
    palette: &'static Palette,
) -> Border {
    let content_width = usize::from(width.saturating_sub(2));
    let gutter = content_width > SCROLLBAR_COLUMNS;
    let text_width = content_width.saturating_sub(usize::from(gutter));
    let provisional = provisional_rows(state, text_width, palette);
    let mut cache = cache.borrow_mut();
    let committed_len = cache
        .rows(state, text_width, palette, |range, width| {
            committed_blocks(state, range, width, palette)
        })
        .len();
    // The provisional tail is one more block: the gap rule puts one blank row
    // between it and the committed rows, exactly as it would between two
    // committed blocks.
    let tail_offset = committed_len + usize::from(committed_len > 0 && !provisional.is_empty());
    let total = tail_offset + provisional.len();
    let visible = usize::from(height.saturating_sub(TRANSCRIPT_FRAME_ROWS));
    let newest = total.saturating_sub(visible);
    let start = newest.saturating_sub(usize::from(state.scroll()));
    // The live tail's expand marker is not in the cache, so the pane publishes
    // the display row it painted it on: that is what turns a click there into
    // an expansion of the live segment.
    let live_reasoning_marker = provisional
        .iter()
        .position(LaidOutRow::is_reasoning_marker)
        .map(|offset| as_row(tail_offset + offset));
    cache.publish_window(TranscriptWindow {
        top,
        visible: as_row(visible),
        start: as_row(start),
        total: as_row(total),
        live_reasoning_marker,
    });
    let committed = cache.laid_out();
    let selection = state.transcript_selection();
    let mut rows = vstack();
    for index in start..total.min(start.saturating_add(visible)) {
        let selected = selection.is_some_and(|range| range.contains(as_row(index)));
        let surface = if selected {
            palette.selection_bg
        } else {
            palette.panel
        };
        let row = window_row(committed, &provisional, tail_offset, index);
        rows = rows.child(row.map_or_else(
            || LaidOutRow::blank(surface).rich_text(palette),
            |row| row.rich_text_on(surface, palette),
        ));
    }
    // The scrollbar is revue's own widget and carries no state: the core owns
    // the window's offset, so every frame hands it the transcript's height and
    // the first visible display row.
    let body = if gutter {
        hstack()
            .child_sized(rows, u16::try_from(text_width).unwrap_or(u16::MAX))
            .child_sized(
                scrollbar(total, start, palette),
                u16::try_from(SCROLLBAR_COLUMNS).unwrap_or(u16::MAX),
            )
    } else {
        rows
    };
    Border::rounded()
        .title("transcript")
        .fg(palette.border)
        .bg(palette.panel)
        .child(body)
}

/// Returns revue's native scrollbar for one transcript window.
///
/// The widget paints its track and thumb only while the content is taller than
/// the window, which is exactly while there is content above or below the
/// visible rows; a transcript that fits shows the gutter's blank column.
fn scrollbar(total: usize, start: usize, palette: &'static Palette) -> ScrollView {
    ScrollView::new()
        .content_height(as_row(total))
        .scroll_offset(as_row(start))
        .scrollbar_style(palette.scroll_thumb, palette.scroll_track)
}

/// Returns one display-row count as the terminal's row type.
fn as_row(rows: usize) -> u16 {
    u16::try_from(rows).unwrap_or(u16::MAX)
}

/// Returns the row one window index shows, or `None` for the tail's gap row.
fn window_row<'a>(
    committed: &'a [LaidOutRow],
    provisional: &'a [LaidOutRow],
    tail_offset: usize,
    index: usize,
) -> Option<&'a LaidOutRow> {
    if index < committed.len() {
        return committed.get(index);
    }
    if index >= tail_offset {
        return provisional.get(index - tail_offset);
    }
    None
}

/// Returns one entry per committed transcript row in `range`, each holding the
/// blocks that row renders.
///
/// A row that renders nothing - a tool result the call's block already draws -
/// still takes its entry, so the cache keeps one display-row span per message.
fn committed_blocks(
    state: &AppState,
    range: Range<usize>,
    width: usize,
    palette: &'static Palette,
) -> Vec<MessageBlocks> {
    let tools = tools::Pairing::of(state.transcript());
    let start = range.start;
    state.transcript()[range]
        .iter()
        .enumerate()
        .map(|(offset, message)| {
            message_blocks(state, message, start + offset, &tools, width, palette)
        })
        .collect()
}

/// Returns the rows of the live provisional tail, laid out for this frame.
///
/// The tail is the step's two blocks before they commit - its reasoning, then
/// the answer it precedes - laid out through the very functions their committed
/// assistant row will use: the same markdown pipeline, the same markers, the
/// same hanging indents, the same collapse rule, the same inks and wash, and
/// the same one-row block gap between them. Nothing here knows it is streaming,
/// and no row of it changes when the commit arrives.
fn provisional_rows(state: &AppState, width: usize, palette: &'static Palette) -> Vec<LaidOutRow> {
    let mut rows = Vec::new();
    append_blocks(
        &mut rows,
        provisional_blocks(state, width, palette),
        palette,
    );
    rows
}

/// Returns the blocks the live provisional tail renders, in stream order.
///
/// A step streams its reasoning before the answer it informs, and the
/// committed assistant row carries both, so the live tail is exactly the
/// block list that row will produce - with the empty block a channel that has
/// not streamed yet simply contributing no row.
fn provisional_blocks(state: &AppState, width: usize, palette: &'static Palette) -> MessageBlocks {
    let mut blocks = Vec::new();
    let reasoning = state.provisional_reasoning();
    if !reasoning.is_empty() {
        // The live segment reads the expansion state of the row it will
        // become, so its expand affordance reveals the same rows there.
        blocks.push(reasoning_block(
            reasoning,
            width,
            state.reasoning_expansion(state.live_reasoning_row()),
            palette,
        ));
    }
    let answer = state.provisional_text();
    if !answer.is_empty() {
        blocks.push(answer_block(answer, width, palette));
    }
    blocks
}

/// Returns the blocks one committed row renders, one entry per block.
fn message_blocks(
    state: &AppState,
    message: &MessageProjectionDto,
    index: usize,
    tools: &tools::Pairing<'_>,
    width: usize,
    palette: &'static Palette,
) -> MessageBlocks {
    match message.kind() {
        MessageKindDto::User => vec![user_card(message.text(), width, palette)],
        MessageKindDto::Assistant => assistant_blocks(state, message, index, width, palette),
        // A tool row is dispatched by its `tool_id`, paired with the row that
        // shares its call identity: the exchange is drawn once, at the call.
        MessageKindDto::ToolCall | MessageKindDto::ToolResult => {
            tools::block(message, index, tools, width, palette)
                .into_iter()
                .collect()
        }
        // A notice is daemon-authored content bound to the run: it renders as
        // its own block on the palette's notice pair, never as ordinary prose.
        MessageKindDto::Notice => vec![notice_block(message.text(), width, palette)],
    }
}

/// Returns one committed assistant row's blocks: its reasoning, then the answer.
///
/// The reasoning here is the durable attachment the committed row carries; the
/// live chain of thought of a step in flight is not a committed row, and the
/// pane draws it as the provisional tail's own reasoning segment.
fn assistant_blocks(
    state: &AppState,
    message: &MessageProjectionDto,
    index: usize,
    width: usize,
    palette: &'static Palette,
) -> MessageBlocks {
    let mut blocks = Vec::new();
    if let Some(reasoning) = message.reasoning() {
        blocks.push(reasoning_block(
            reasoning,
            width,
            state.reasoning_expansion(index),
            palette,
        ));
    }
    blocks.push(answer_block(message.text(), width, palette));
    blocks
}

/// Returns the display rows of one answer: its marker riding the first row.
///
/// The marker glyph sits at the start of the answer's first display row,
/// immediately before the text and separated from it by the shared marker gap.
/// Every row after it is held at the same text column by a hanging indent, so a
/// wrapped answer lines up under its first line rather than under the glyph.
/// The markdown is laid out that indent short of the block's width, so a
/// prefixed row is exactly as wide as the block and never clipped.
fn answer_block(text: &str, width: usize, palette: &'static Palette) -> Vec<LaidOutRow> {
    let indent = answer_indent();
    let inner = width.saturating_sub(indent);
    markdown::rows(text, inner, palette.panel, palette)
        .into_iter()
        .enumerate()
        .map(|(index, row)| answer_row(&row, index == 0, indent, inner, palette))
        .collect()
}

/// Returns the columns the answer marker's glyph and shared gap take.
///
/// The glyph's own width comes from `char_width`, so the hanging indent is
/// exactly as wide as the marker paints.
fn answer_indent() -> usize {
    display_width(ANSWER_GLYPH) + MARKER_GAP
}

/// Returns one answer row: the marker on the first row, the hanging indent on
/// every row after it, then the row's own runs.
fn answer_row(
    row: &LaidOutRow,
    marked: bool,
    indent: usize,
    inner: usize,
    palette: &'static Palette,
) -> LaidOutRow {
    let mut builder = RowBuilder::new();
    if marked {
        builder.push(ANSWER_GLYPH, RowStyleId::Muted);
        builder.push(&" ".repeat(MARKER_GAP), RowStyleId::Muted);
    } else {
        builder.push(&" ".repeat(indent), RowStyleId::Muted);
    }
    builder.push_clipped(row, inner);
    builder.finish(palette.panel)
}

/// Returns the display rows of one user card.
///
/// The card is a framed, padded block rather than a raw `user> …` line: it is
/// drawn as flat display rows (not as a nested `Border` widget) so the
/// transcript's row window can start and end anywhere inside a card and every
/// row the card paints is counted exactly once. Its content goes through the
/// markdown layout at the card's inner width, so a user message renders its
/// code fences, bold, and lists exactly like an answer does; a window too
/// narrow for a frame falls back to the bare rows.
fn user_card(text: &str, width: usize, palette: &'static Palette) -> Vec<LaidOutRow> {
    if width <= CARD_CHROME_COLUMNS {
        return markdown::rows(text, width.max(1), palette.user_surface, palette);
    }
    let inner = width - CARD_CHROME_COLUMNS;
    let label = marker_text(USER_GLYPH, USER_LABEL);
    let mut rows = vec![box_top(
        Some((&label, RowStyleId::UserLabel)),
        width,
        RowStyleId::UserBorder,
        palette.user_surface,
    )];
    for row in markdown::rows(text, inner, palette.user_surface, palette) {
        rows.push(box_content(&row, inner, RowStyleId::UserBorder));
    }
    rows.push(box_bottom(
        width,
        RowStyleId::UserBorder,
        palette.user_surface,
    ));
    rows
}

/// Returns the display rows of one daemon notice block.
///
/// A notice is daemon-authored content bound to the run, not model prose: its
/// text is never run through the markdown pipeline, and its marker rides the
/// notice's own first line with the same shared gap and hanging indent an
/// answer uses. Its wash and ink are the palette's notice pair, so a notice
/// never reads as an answer or as a tool result.
// @todo(core): a `notice` row carries only free text - no notice code, no
// severity, and no run binding beyond the row's own run - so the block renders
// the text exactly as the daemon wrote it and never invents a classification.
fn notice_block(text: &str, width: usize, palette: &'static Palette) -> Vec<LaidOutRow> {
    let indent = display_width(NOTICE_GLYPH) + MARKER_GAP;
    let inner = width.saturating_sub(indent);
    plain_rows(text, inner, RowStyleId::Notice, palette)
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            marked_row(
                &row,
                index == 0,
                NOTICE_GLYPH,
                indent,
                inner,
                RowStyleId::Notice,
                palette.notice_surface,
            )
        })
        .collect()
}

/// Returns one row of a block whose marker rides its first display row.
///
/// The glyph opens the first row, the shared [`MARKER_GAP`] separates it from
/// the text it precedes, and every row after it keeps the same text column
/// through a hanging indent, so a wrapped block lines up under its first line
/// rather than under the glyph. The indent is measured through `char_width`,
/// and the caller lays the row out `inner` columns short of the block's width,
/// so a marked row is never wider than the block.
pub(in crate::tui) fn marked_row(
    row: &LaidOutRow,
    marked: bool,
    glyph: &str,
    indent: usize,
    inner: usize,
    ink: RowStyleId,
    surface: Color,
) -> LaidOutRow {
    let mut builder = RowBuilder::new();
    if marked {
        builder.push(glyph, ink);
        builder.push(&" ".repeat(MARKER_GAP), ink);
    } else {
        builder.push(&" ".repeat(indent), ink);
    }
    builder.push_clipped(row, inner);
    builder.finish(surface)
}

/// Returns the display rows of one reasoning block: a header glyph, the
/// visible body rows, the marker a collapsed block shows, and the accent bar
/// that closes the block.
///
/// Collapse rule: a block longer than [`REASONING_VISIBLE_ROWS`] display rows
/// shows its first fifteen and one marker naming how many rows stay hidden.
/// The marker is the block's expand affordance: each activation reveals one
/// more chunk of display rows, which is what `expansion` counts. The state
/// lives in the core, keyed by the block's committed row, so it survives
/// appends and clears when the transcript is replaced or front-trimmed.
fn reasoning_block(
    reasoning: &str,
    width: usize,
    expansion: usize,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    let mut rows = vec![text_row(
        &marker_text(REASONING_GLYPH, REASONING_LABEL),
        RowStyleId::ReasoningHeader,
        palette,
    )];
    let body = plain_rows(reasoning, width, RowStyleId::Reasoning, palette);
    let shown = REASONING_VISIBLE_ROWS
        .saturating_add(expansion)
        .min(body.len());
    let hidden = body.len() - shown;
    rows.extend(body.into_iter().take(shown));
    if hidden > 0 {
        rows.push(text_row(
            &marker_text(
                REASONING_EXPAND,
                &format!("… {hidden} more reasoning lines hidden — click or Ctrl+E to reveal more"),
            ),
            RowStyleId::ReasoningMarker,
            palette,
        ));
    }
    rows.push(text_row(
        &"─".repeat(width.max(1)),
        RowStyleId::Accent,
        palette,
    ));
    rows
}

/// Returns the display rows of one plain logical line, word-wrapped with the
/// continuation indent.
///
/// The tool blocks lay their preview content out through the same wrapper, so
/// one wrap rule covers the whole transcript.
pub(in crate::tui) fn plain_rows(
    line: &str,
    width: usize,
    ink: RowStyleId,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    TextWrapper::new(width.max(1))
        .mode(WrapMode::Word)
        .subsequent_indent(CONTINUATION_INDENT)
        .wrap(line)
        .into_iter()
        .map(|row| text_row(&row, ink, palette))
        .collect()
}

/// Returns one text row in one ink id, on the panel's wash.
fn text_row(text: &str, ink: RowStyleId, palette: &'static Palette) -> LaidOutRow {
    let mut row = RowBuilder::new();
    row.push(text, ink);
    row.finish(palette.panel)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit tests build typed fixture DTOs directly and assert them for diagnostics."
    )]

    use std::cell::RefCell;
    use std::ops::Range;

    use intention_client::{RETAINED_TRANSCRIPT_MESSAGES, RunStreamState};
    use intention_proto::{
        ConfigRevisionId, MessageKindDto, MessageProjectionDto, ProjectId, RunId, RunModeDto,
        RunProjectionDto, RunStatusDto, RunStreamFrameDto, RunSubscriptionSnapshotDto, SessionId,
        SessionProjectionDto, SessionSnapshotDto, TextDeltaChannelDto, TextDeltaFrameDto, TurnId,
        WorkspaceId, WorkspaceRootDto,
    };
    use revue::render::Modifier;

    use crate::app::{Action, AppState, Theme};
    use crate::tui::layout::{
        LaidOutRow, MARKER_GAP, RowStyleId, TranscriptLayoutCache, append_blocks, append_messages,
    };
    use crate::tui::palette::{self, Palette};

    /// The light palette every fixture renders with.
    const LIGHT: &Palette = &palette::LIGHT;

    /// The dark palette the theme-switch fixture renders with.
    const DARK: &Palette = &palette::DARK;

    use super::{
        ANSWER_GLYPH, REASONING_GLYPH, USER_GLYPH, answer_block, assistant_blocks,
        committed_blocks, provisional_rows, reasoning_block, text_row, transcript_pane, user_card,
        window_row,
    };

    /// A content width wide enough for the whole fixture set to lay out.
    const WIDTH: usize = 48;

    /// Returns one committed user row carrying `text`.
    fn user_row(session_id: SessionId, text: &str) -> MessageProjectionDto {
        MessageProjectionDto::new(
            session_id,
            None,
            MessageKindDto::User,
            text,
            None,
            None,
            None,
        )
        .expect("the fixture user row is coherent")
    }

    /// Returns one committed assistant row carrying `text` and `reasoning`.
    fn assistant_row(
        session_id: SessionId,
        run_id: RunId,
        text: &str,
        reasoning: Option<&str>,
    ) -> MessageProjectionDto {
        MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            text,
            reasoning.map(str::to_owned),
            None,
            None,
        )
        .expect("the fixture assistant row is coherent")
    }

    /// Returns one run projection of `session_id` in `status`.
    fn run(session_id: SessionId, run_id: RunId, status: RunStatusDto) -> RunProjectionDto {
        RunProjectionDto::new(
            session_id,
            run_id,
            TurnId::new(),
            status,
            ConfigRevisionId::new(),
        )
    }

    /// Returns one live run-stream state over `messages`.
    fn stream_state(
        session_id: SessionId,
        run_id: RunId,
        messages: Vec<MessageProjectionDto>,
    ) -> RunStreamState {
        let snapshot = RunSubscriptionSnapshotDto::new(
            run(session_id, run_id, RunStatusDto::Running),
            messages,
        )
        .expect("the fixture run snapshot is coherent");
        let mut state = RunStreamState::new(session_id, run_id);
        state
            .apply_initial(snapshot)
            .expect("the fixture run snapshot applies to its own scope");
        state
    }

    /// Returns one session snapshot carrying `messages`.
    fn snapshot(session_id: SessionId, messages: Vec<MessageProjectionDto>) -> SessionSnapshotDto {
        let root = WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
            .expect("the process temporary directory is an absolute workspace root");
        let projection = SessionProjectionDto::new(
            ProjectId::new(),
            session_id,
            WorkspaceId::new(),
            root,
            RunModeDto::Build,
            None,
            None,
            Vec::new(),
        )
        .expect("the fixture session projection is coherent");
        SessionSnapshotDto::with_projection(session_id, projection, messages)
            .expect("the fixture session snapshot is coherent")
    }

    /// Returns the transcript the layout fixtures compare: a user card, an
    /// answer carrying every block kind, and a reasoning block.
    fn fixture_messages(session_id: SessionId, run_id: RunId) -> Vec<MessageProjectionDto> {
        let answer = "# Heading\n\nplain **strong** text with `code` and a \
                      [link](https://example.test).\n\n- one\n- two\n\n\
                      | a | longer heading |\n|---|---|\n| b | c |\n\n\
                      ```rust\nlet answer = 42;\n```\n\n\
                      alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi \
                      omicron pi rho sigma tau";
        vec![
            user_row(session_id, "# user card\n\n- one\n- two"),
            assistant_row(
                session_id,
                run_id,
                answer,
                Some("first thought\nsecond thought"),
            ),
        ]
    }

    /// Opens `session_id` over the layout fixture set.
    fn fixture_state(session_id: SessionId, run_id: RunId) -> AppState {
        let mut state = AppState::new(None);
        state.update(Action::SessionSnapshotLoaded(snapshot(
            session_id,
            fixture_messages(session_id, run_id),
        )));
        state
    }

    /// Opens the fixture transcript's run stream, so deltas can reach it.
    fn opened_stream(session_id: SessionId, run_id: RunId) -> AppState {
        let mut state = fixture_state(session_id, run_id);
        let rows = state.transcript().to_vec();
        state.update(Action::RunStreamOpened(stream_state(
            session_id, run_id, rows,
        )));
        state
    }

    /// Streams one answer chunk of the fixture stream's first model step.
    fn stream_answer(state: &mut AppState, session_id: SessionId, run_id: RunId, text: &str) {
        state.update(Action::FrameReceived(RunStreamFrameDto::TextDelta(
            TextDeltaFrameDto::new(session_id, run_id, 0, TextDeltaChannelDto::Answer, text)
                .expect("the fixture answer delta is valid"),
        )));
    }

    /// Streams one reasoning chunk of the fixture stream's first model step.
    fn stream_reasoning(state: &mut AppState, session_id: SessionId, run_id: RunId, text: &str) {
        state.update(Action::FrameReceived(RunStreamFrameDto::TextDelta(
            TextDeltaFrameDto::new(session_id, run_id, 0, TextDeltaChannelDto::Reasoning, text)
                .expect("the fixture reasoning delta is valid"),
        )));
    }

    /// Returns a reasoning fixture longer than the collapsed block shows.
    fn long_reasoning() -> String {
        (0..super::REASONING_VISIBLE_ROWS + 10)
            .map(|index| format!("step {index}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Appends one committed assistant row through the live stream path.
    ///
    /// The subscription snapshot carries the row the session read missed, which
    /// is exactly the append path the core reports as append-only.
    fn append_row(state: &mut AppState, session_id: SessionId, run_id: RunId, text: &str) {
        let mut rows = state.transcript().to_vec();
        rows.push(assistant_row(session_id, run_id, text, None));
        state.update(Action::RunStreamOpened(stream_state(
            session_id, run_id, rows,
        )));
    }

    /// Returns every row's text and run styles, for row-for-row comparison.
    fn roles(rows: &[LaidOutRow]) -> Vec<(String, Vec<RowStyleId>)> {
        rows.iter()
            .map(|row| (row.text.clone(), row.styles()))
            .collect()
    }

    /// Returns what one direct layout of the whole transcript produces.
    fn direct_rows(state: &AppState, width: usize, palette: &'static Palette) -> Vec<LaidOutRow> {
        let mut rows = Vec::new();
        append_messages(
            &mut rows,
            committed_blocks(state, 0..state.transcript().len(), width, palette),
            palette,
        );
        rows
    }

    /// Returns one plain fixture row carrying `text`.
    fn plain(text: &str) -> LaidOutRow {
        text_row(text, RowStyleId::Body(Modifier::empty()), LIGHT)
    }

    #[test]
    fn a_cache_hit_lays_out_nothing_while_the_input_line_is_typed_into() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let mut state = fixture_state(session_id, run_id);
        let mut cache = TranscriptLayoutCache::new();
        let laid_out = cache
            .rows(&state, WIDTH, LIGHT, |range, width| {
                committed_blocks(&state, range, width, LIGHT)
            })
            .len();
        assert!(laid_out > 0, "the fixture set lays out display rows");
        assert_eq!(cache.layout_count(), 1);

        state.update(Action::InputChar('x'));
        state.update(Action::InputChar('y'));
        let again = cache
            .rows(&state, WIDTH, LIGHT, |range, width| {
                committed_blocks(&state, range, width, LIGHT)
            })
            .len();
        assert_eq!(
            cache.layout_count(),
            1,
            "typing into the input line never reaches the transcript layout"
        );
        assert_eq!(again, laid_out, "the same rows stay cached");
    }

    #[test]
    fn an_appended_row_lays_out_only_the_tail() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let mut state = fixture_state(session_id, run_id);
        let seeded = state.transcript().len();
        let mut cache = TranscriptLayoutCache::new();
        let mut ranges: Vec<Range<usize>> = Vec::new();
        let first = cache
            .rows(&state, WIDTH, LIGHT, |range, width| {
                ranges.push(range.clone());
                committed_blocks(&state, range, width, LIGHT)
            })
            .len();
        assert_eq!(ranges, vec![0..seeded], "a cold cache lays out everything");
        assert_eq!(cache.layout_count(), 1);

        ranges.clear();
        append_row(&mut state, session_id, run_id, "a later answer");
        let second = cache
            .rows(&state, WIDTH, LIGHT, |range, width| {
                ranges.push(range.clone());
                committed_blocks(&state, range, width, LIGHT)
            })
            .len();
        assert_eq!(
            ranges,
            vec![seeded..seeded + 1],
            "an append lays out the appended row only"
        );
        assert_eq!(cache.layout_count(), 2, "one tail layout, not a replay");
        assert!(second > first, "the appended block adds its own rows");
        assert_eq!(
            roles(&direct_rows(&state, WIDTH, LIGHT)),
            {
                let cached = cache.rows(&state, WIDTH, LIGHT, |range, width| {
                    committed_blocks(&state, range, width, LIGHT)
                });
                roles(cached)
            },
            "the tail layout ends up exactly where a full replay would"
        );
    }

    #[test]
    fn a_width_change_lays_the_whole_transcript_out_again() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let state = fixture_state(session_id, run_id);
        let mut cache = TranscriptLayoutCache::new();
        let mut ranges: Vec<Range<usize>> = Vec::new();
        cache.rows(&state, WIDTH, LIGHT, |range, width| {
            ranges.push(range.clone());
            committed_blocks(&state, range, width, LIGHT)
        });
        ranges.clear();

        let replayed = cache
            .rows(&state, WIDTH / 2, LIGHT, |range, width| {
                ranges.push(range.clone());
                committed_blocks(&state, range, width, LIGHT)
            })
            .len();
        assert_eq!(
            ranges,
            vec![0..state.transcript().len()],
            "a resize moves every wrap point, so the whole transcript replays"
        );
        assert_eq!(cache.layout_count(), 2);
        assert_eq!(
            replayed,
            direct_rows(&state, WIDTH / 2, LIGHT).len(),
            "the replayed rows are the rows a direct pass lays out"
        );
    }

    #[test]
    fn a_theme_change_lays_the_whole_transcript_out_again() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let mut state = fixture_state(session_id, run_id);
        let mut cache = TranscriptLayoutCache::new();
        let light: Vec<_> = cache
            .rows(&state, WIDTH, LIGHT, |range, width| {
                committed_blocks(&state, range, width, LIGHT)
            })
            .iter()
            .map(|row| row.surface)
            .collect();
        assert_eq!(
            light[0], LIGHT.user_surface,
            "the fixture opens on a user card"
        );

        state = state.with_theme(Theme::Dark);
        let dark: Vec<_> = cache
            .rows(&state, WIDTH, DARK, |range, width| {
                committed_blocks(&state, range, width, DARK)
            })
            .iter()
            .map(|row| row.surface)
            .collect();
        assert_eq!(
            cache.layout_count(),
            2,
            "a theme change replays: cached surfaces carry the theme's own values"
        );
        assert_eq!(
            dark[0], DARK.user_surface,
            "the replay paints the dark card"
        );
        assert_ne!(
            light[0], dark[0],
            "the two themes paint the same row on different surfaces"
        );
    }

    #[test]
    fn a_session_snapshot_lays_the_whole_transcript_out_again() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let mut state = fixture_state(session_id, run_id);
        let mut cache = TranscriptLayoutCache::new();
        let mut ranges: Vec<Range<usize>> = Vec::new();
        cache.rows(&state, WIDTH, LIGHT, |range, width| {
            ranges.push(range.clone());
            committed_blocks(&state, range, width, LIGHT)
        });
        ranges.clear();

        state.update(Action::SessionSnapshotLoaded(snapshot(
            session_id,
            vec![user_row(session_id, "a whole new session")],
        )));
        let replayed = cache
            .rows(&state, WIDTH, LIGHT, |range, width| {
                ranges.push(range.clone());
                committed_blocks(&state, range, width, LIGHT)
            })
            .len();
        assert_eq!(
            ranges,
            vec![0..1],
            "a snapshot replaces the transcript, so nothing cached survives it"
        );
        assert_eq!(cache.layout_count(), 2);
        assert_eq!(
            replayed,
            direct_rows(&state, WIDTH, LIGHT).len(),
            "the replaced transcript lays out as a direct pass would"
        );
    }

    #[test]
    fn a_front_trim_lays_the_whole_transcript_out_again() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let rows: Vec<_> = (0..RETAINED_TRANSCRIPT_MESSAGES)
            .map(|index| assistant_row(session_id, run_id, &format!("row {index}"), None))
            .collect();
        let mut state = AppState::new(None);
        state.update(Action::SessionSnapshotLoaded(snapshot(
            session_id,
            rows.clone(),
        )));
        state.update(Action::RunStreamOpened(stream_state(
            session_id, run_id, rows,
        )));
        let mut cache = TranscriptLayoutCache::new();
        let mut ranges: Vec<Range<usize>> = Vec::new();
        cache.rows(&state, WIDTH, LIGHT, |range, width| {
            ranges.push(range.clone());
            committed_blocks(&state, range, width, LIGHT)
        });
        ranges.clear();

        state.update(Action::FrameReceived(RunStreamFrameDto::Content(
            assistant_row(session_id, run_id, "the newest row", None),
        )));
        assert_eq!(state.transcript().len(), RETAINED_TRANSCRIPT_MESSAGES);
        cache.rows(&state, WIDTH, LIGHT, |range, width| {
            ranges.push(range.clone());
            committed_blocks(&state, range, width, LIGHT)
        });
        assert_eq!(
            ranges,
            vec![0..RETAINED_TRANSCRIPT_MESSAGES],
            "a front drain moves every row, so the whole transcript replays"
        );
        assert_eq!(cache.layout_count(), 2);
    }

    #[test]
    fn the_cached_rows_are_the_rows_of_a_direct_layout() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let mut state = fixture_state(session_id, run_id);
        let mut cache = TranscriptLayoutCache::new();
        let first = {
            let cached = cache.rows(&state, WIDTH, LIGHT, |range, width| {
                committed_blocks(&state, range, width, LIGHT)
            });
            assert_eq!(
                roles(cached),
                roles(&direct_rows(&state, WIDTH, LIGHT)),
                "a cold cache lays out what a direct pass lays out"
            );
            cached.len()
        };

        append_row(&mut state, session_id, run_id, "a later answer");
        let second = {
            let cached = cache.rows(&state, WIDTH, LIGHT, |range, width| {
                committed_blocks(&state, range, width, LIGHT)
            });
            assert_eq!(
                roles(cached),
                roles(&direct_rows(&state, WIDTH, LIGHT)),
                "the tail layout ends up exactly where a full replay would"
            );
            cached.len()
        };
        assert!(
            second > first,
            "the appended block's rows and its gap row joined the cached rows"
        );
    }

    #[test]
    fn the_provisional_tail_is_laid_out_per_frame_and_never_cached() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let mut state = opened_stream(session_id, run_id);
        let mut cache = TranscriptLayoutCache::new();
        cache.rows(&state, WIDTH, LIGHT, |range, width| {
            committed_blocks(&state, range, width, LIGHT)
        });

        stream_answer(&mut state, session_id, run_id, "partial");
        let tail = provisional_rows(&state, WIDTH, LIGHT);
        assert_eq!(tail.len(), 1, "the tail is one wrapped line here");
        assert_eq!(tail[0].text, format!("{ANSWER_GLYPH} partial"));
        assert_eq!(
            tail[0].styles(),
            vec![RowStyleId::Muted, RowStyleId::Body(Modifier::empty())],
            "the tail carries the answer marker and the body ink, nothing of its own"
        );
        assert_eq!(
            cache.layout_count(),
            1,
            "a transient delta is no transcript mutation: nothing replays"
        );
    }

    #[test]
    fn the_streaming_tail_lays_out_as_the_committed_answer_row_it_becomes() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let mut state = opened_stream(session_id, run_id);
        let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda";
        stream_answer(&mut state, session_id, run_id, text);

        let tail = provisional_rows(&state, WIDTH, LIGHT);
        assert!(tail.len() > 1, "the fixture tail wraps");
        assert!(
            tail[0].text.starts_with(&format!("{ANSWER_GLYPH} ")),
            "the live tail opens with the answer's own marker: {:?}",
            tail[0].text
        );
        assert!(
            tail.iter().all(|row| row.surface == LIGHT.panel),
            "the live tail sits on the panel wash, like a committed answer"
        );

        let committed = assistant_blocks(
            &state,
            &assistant_row(session_id, run_id, text, None),
            0,
            WIDTH,
            LIGHT,
        );
        assert_eq!(
            committed.len(),
            1,
            "an answer without reasoning is one block"
        );
        assert_eq!(
            roles(&tail),
            roles(&committed[0]),
            "the live tail lays out exactly as the committed answer row it becomes"
        );
    }

    #[test]
    fn the_streaming_tail_lays_out_as_the_committed_reasoning_block_it_becomes() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let mut state = opened_stream(session_id, run_id);
        let reasoning = "weighing the options carefully";
        let answer = "the answer it informs";
        stream_reasoning(&mut state, session_id, run_id, reasoning);
        stream_answer(&mut state, session_id, run_id, answer);

        let tail = provisional_rows(&state, WIDTH, LIGHT);
        assert!(
            tail[0].text.starts_with(&format!("{REASONING_GLYPH} ")),
            "the live reasoning segment opens with the committed marker: {:?}",
            tail[0].text
        );

        // The committed assistant row this step becomes owns both blocks, with
        // the one-row gap the transcript puts between them.
        let committed = assistant_blocks(
            &AppState::new(None),
            &assistant_row(session_id, run_id, answer, Some(reasoning)),
            0,
            WIDTH,
            LIGHT,
        );
        assert_eq!(committed.len(), 2, "reasoning and answer are two blocks");
        let mut expected = Vec::new();
        append_blocks(&mut expected, committed, LIGHT);
        assert!(
            expected.iter().any(LaidOutRow::is_blank),
            "the fixture keeps the gap row between the two blocks"
        );
        assert_eq!(
            roles(&tail),
            roles(&expected),
            "the live tail is the committed row's own blocks, row for row"
        );
    }

    #[test]
    fn the_live_reasoning_segment_collapses_like_the_committed_block_it_becomes() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let reasoning = long_reasoning();
        let mut state = opened_stream(session_id, run_id);
        stream_reasoning(&mut state, session_id, run_id, &reasoning);
        stream_answer(&mut state, session_id, run_id, "the answer");

        let tail = provisional_rows(&state, WIDTH, LIGHT);
        let committed = reasoning_block(&reasoning, WIDTH, 0, LIGHT);
        assert!(
            committed.iter().any(LaidOutRow::is_reasoning_marker),
            "the committed block of this reasoning collapses"
        );
        assert_eq!(
            roles(&tail[..committed.len()]),
            roles(&committed),
            "the live segment shows the committed block's rows, marker included"
        );
    }

    #[test]
    fn an_expansion_of_the_live_reasoning_segment_survives_the_commit() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let reasoning = long_reasoning();
        let answer = "the answer";
        let mut state = opened_stream(session_id, run_id);
        stream_reasoning(&mut state, session_id, run_id, &reasoning);
        stream_answer(&mut state, session_id, run_id, answer);
        let live_row = state.live_reasoning_row();
        let collapsed = provisional_rows(&state, WIDTH, LIGHT).len();

        state.update(Action::ExpandReasoning {
            row: Some(live_row),
        });
        let expansion = state.reasoning_expansion(live_row);
        assert!(expansion > 0, "the live segment expands by one chunk");
        let expanded_tail = provisional_rows(&state, WIDTH, LIGHT);
        assert!(
            expanded_tail.len() > collapsed,
            "the activation reveals rows the collapsed live segment hid"
        );
        assert!(
            !expanded_tail.iter().any(LaidOutRow::is_reasoning_marker),
            "a fully revealed live segment stops advertising its affordance"
        );

        // The commit lands the assistant row at exactly the row the live
        // segment was anchored at, carrying the same reasoning.
        state.update(Action::FrameReceived(RunStreamFrameDto::Content(
            assistant_row(session_id, run_id, answer, Some(&reasoning)),
        )));
        assert_eq!(state.transcript().len(), live_row + 1);
        assert_eq!(
            state.provisional_reasoning(),
            "",
            "the committed row supersedes the live segment"
        );
        assert_eq!(
            state.reasoning_expansion(live_row),
            expansion,
            "the row the live segment became keeps the expansion it had"
        );
        let committed = assistant_blocks(
            &state,
            &assistant_row(session_id, run_id, answer, Some(&reasoning)),
            live_row,
            WIDTH,
            LIGHT,
        );
        let mut expected = Vec::new();
        append_blocks(&mut expected, committed, LIGHT);
        assert_eq!(
            roles(&expanded_tail),
            roles(&expected),
            "the expanded live segment is the committed block, row for row"
        );
    }

    #[test]
    fn the_pane_publishes_the_live_reasoning_marker_it_painted() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let mut state = opened_stream(session_id, run_id);
        stream_reasoning(&mut state, session_id, run_id, &long_reasoning());
        let cache = RefCell::new(TranscriptLayoutCache::new());
        let _pane = transcript_pane(&state, 0, 40, 50, &cache, LIGHT);
        let window = cache.borrow().window();
        let committed_len = cache.borrow().laid_out().len();
        let marker = window
            .live_reasoning_marker
            .map(usize::from)
            .expect("the collapsed live segment publishes its expand marker row");
        assert!(
            marker >= committed_len,
            "the published marker belongs to the live tail, not the committed rows"
        );
        assert!(
            marker < usize::from(window.total),
            "the published marker is a row the frame painted"
        );

        // A live segment that fits publishes no marker: the row is cleared
        // every frame, so a stale one can never expand a block that is gone.
        let mut short = opened_stream(session_id, run_id);
        stream_reasoning(&mut short, session_id, run_id, "a thought");
        let cache = RefCell::new(TranscriptLayoutCache::new());
        let _pane = transcript_pane(&short, 0, 40, 50, &cache, LIGHT);
        assert_eq!(cache.borrow().window().live_reasoning_marker, None);
    }

    #[test]
    fn the_window_counts_the_tail_gap_row_between_the_committed_rows_and_the_tail() {
        let committed = vec![plain("first"), plain("second")];
        let provisional = vec![plain("∷ partial")];
        let tail_offset =
            committed.len() + usize::from(!committed.is_empty() && !provisional.is_empty());
        assert_eq!(tail_offset, 3, "one gap row sits before the tail");

        assert_eq!(
            window_row(&committed, &provisional, tail_offset, 0).map(|row| row.text.as_str()),
            Some("first")
        );
        assert!(
            window_row(&committed, &provisional, tail_offset, 2).is_none(),
            "the gap row paints the panel's wash and nothing else"
        );
        assert_eq!(
            window_row(&committed, &provisional, tail_offset, 3).map(|row| row.text.as_str()),
            Some("∷ partial")
        );
        assert!(window_row(&committed, &provisional, tail_offset, 4).is_none());
    }

    #[test]
    fn the_pane_lays_the_committed_rows_out_through_the_cache_it_is_given() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let state = fixture_state(session_id, run_id);
        let cache = RefCell::new(TranscriptLayoutCache::new());
        let pane = transcript_pane(&state, 3, 20, 50, &cache, LIGHT);
        assert!(
            matches!(pane, revue::widget::Border { .. }),
            "the pane stays the framed transcript"
        );
        assert_eq!(
            cache.borrow().layout_count(),
            1,
            "the pane lays the committed rows out through the cache it is given"
        );
    }

    #[test]
    fn every_marker_keeps_the_shared_gap_after_its_glyph() {
        let gap = " ".repeat(MARKER_GAP);
        assert_eq!(MARKER_GAP, 1, "the one shared gap is one blank column");

        let answer = answer_block("the answer", 40, LIGHT);
        assert_eq!(
            answer[0].text,
            format!("{ANSWER_GLYPH}{gap}the answer"),
            "the answer's marker rides its own first line"
        );

        let reasoning = reasoning_block("a thought", 40, 0, LIGHT);
        assert_eq!(
            reasoning[0].text,
            format!("{REASONING_GLYPH}{gap}reasoning"),
            "the reasoning header uses the same gap"
        );

        let card = user_card("a question", 40, LIGHT);
        assert!(
            card[0]
                .text
                .starts_with(&format!("╭─ {USER_GLYPH}{gap}you ")),
            "the user card's label uses the same gap: {}",
            card[0].text
        );
    }

    #[test]
    fn a_wrapped_answer_holds_its_text_column() {
        let rows = answer_block(
            "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda",
            24,
            LIGHT,
        );
        assert!(rows.len() > 1, "the fixture answer wraps");
        let indent = " ".repeat(super::answer_indent());
        assert!(
            rows[0].text.starts_with(&format!("{ANSWER_GLYPH} ")),
            "the marker opens the first row: {:?}",
            rows[0].text
        );
        assert!(
            !rows[0].text.starts_with(&format!("{ANSWER_GLYPH}  ")),
            "exactly the shared gap follows the glyph: {:?}",
            rows[0].text
        );
        for row in &rows[1..] {
            assert!(
                row.text.starts_with(&indent),
                "every row after the first keeps the text column: {:?}",
                row.text
            );
        }
    }

    #[test]
    fn the_selected_display_rows_are_painted_on_the_selection_fill() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let rows: Vec<_> = (0..3)
            .map(|index| assistant_row(session_id, run_id, &format!("row {index}"), None))
            .collect();
        let mut state = AppState::new(None);
        state.update(Action::SessionSnapshotLoaded(snapshot(session_id, rows)));
        // The transcript's display rows are `row 0`, a gap, `row 1`, a gap,
        // `row 2`; selecting rows 2 and 3 covers the second answer and its
        // following gap.
        state.update(Action::SelectTranscriptRows {
            anchor: 2,
            extent: 3,
        });
        let cache = RefCell::new(TranscriptLayoutCache::new());
        let pane = transcript_pane(&state, 0, 12, 48, &cache, LIGHT);
        let app = revue::testing::TestApp::with_size(pane, 48, 12);
        let selected = app
            .buffer()
            .get(1, 3)
            .expect("the selected row paints a cell");
        assert_eq!(
            selected.bg,
            Some(LIGHT.selection_bg),
            "the selected display row is painted on the selection fill"
        );
        let plain = app
            .buffer()
            .get(1, 5)
            .expect("an unselected row paints a cell");
        assert_eq!(
            plain.bg,
            Some(LIGHT.panel),
            "a row outside the selection keeps the panel wash"
        );
    }

    #[test]
    fn the_window_publishes_the_geometry_it_painted() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let state = fixture_state(session_id, run_id);
        let cache = RefCell::new(TranscriptLayoutCache::new());
        transcript_pane(&state, 7, 20, 50, &cache, LIGHT);
        let window = cache.borrow().window();
        assert_eq!(window.top, 7, "the window publishes its first screen row");
        assert_eq!(window.visible, 18, "the pane's own frame takes two rows");
        assert!(
            window.total > window.visible,
            "the fixture set is taller than the window"
        );
        assert_eq!(
            window.start,
            window.total - window.visible,
            "an unscrolled window starts at the newest display rows"
        );
    }

    #[test]
    fn the_layout_reports_the_committed_row_a_reasoning_marker_belongs_to() {
        let session_id = SessionId::new();
        let run_id = RunId::new();
        let reasoning = (0..40)
            .map(|index| format!("step {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut state = AppState::new(None);
        state.update(Action::SessionSnapshotLoaded(snapshot(
            session_id,
            vec![assistant_row(
                session_id,
                run_id,
                "the answer",
                Some(&reasoning),
            )],
        )));
        let mut cache = TranscriptLayoutCache::new();
        let marker = {
            let rows = cache.rows(&state, WIDTH, LIGHT, |range, width| {
                committed_blocks(&state, range, width, LIGHT)
            });
            rows.iter()
                .position(|row| row.is_reasoning_marker())
                .expect("the collapsed block shows its expand marker")
        };
        assert_eq!(
            cache.message_at(marker),
            Some(0),
            "the marker row belongs to the committed row that produced it"
        );
        assert!(
            cache.message_at(usize::MAX).is_none(),
            "a display row the layout never produced belongs to no message"
        );
    }
}
