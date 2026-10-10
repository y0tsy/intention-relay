//! The neutral transcript layout and the cache a frame reuses.
//!
//! A transcript block is laid out once, into [`LaidOutRow`] values: text plus
//! the styled runs of that text. Nothing here is a widget, a resolved [`Style`],
//! or a per-character copy, so the whole committed transcript can sit in memory
//! between frames while only the visible window becomes a widget.
//!
//! [`TranscriptLayoutCache`] holds those rows keyed by the width they were laid
//! out at, the transcript version the core reported, the reasoning expansion
//! epoch, and the committed tool-result count:
//!
//! - the width changed: the wrap points moved, so the whole transcript is laid
//!   out again;
//! - the epoch moved and the core confirms only appends happened since the
//!   cached one: only the appended rows are laid out and the cached prefix is
//!   kept;
//! - a reasoning expansion or a newly committed tool result moved: both insert
//!   rows into or change a block the cached prefix holds, so the whole
//!   transcript is laid out again;
//! - anything else - a replacement, a trim's front drain, a session switch, a
//!   future epoch, or no cache yet - is a full replay.
//!
//! The provisional tail is not committed state and never enters the cache: the
//! pane lays it out per frame and appends it through [`append_blocks`], the one
//! place that knows the transcript's gap rule.
//!
//! A run resolves to a [`Style`] once when a visible row is materialised; the
//! per-character `Style` clone the glyph model used is gone.
// @todo(revue): revue offers no clippable container (a nested `Border` cannot be
// windowed mid-block), no fixed-column or table layout, no text-measure or
// truncation helper, and no child geometry, so this module hand-draws block
// frames as flat rows, hand-rolls wrapping and truncation, and computes the
// screen rows a pointer hit-tests from the window the pane published; the
// pane's scroll arithmetic is duplicated here for drag-scrolling. One shared
// container, measurement, and geometry surface in revue would delete this
// whole class.

use std::ops::Range;

use revue::render::Modifier;
use revue::style::Color;
use revue::text::char_width;
use revue::widget::{RichText, Span, Style, theme};

use crate::app::{AppState, Theme, TranscriptScroll};
use crate::tui::palette::Palette;

/// The indent every continuation row of one wrapped logical line carries.
pub(in crate::tui) const CONTINUATION_INDENT: &str = "  ";

/// The one blank column every marker glyph keeps before the text it opens.
///
/// The user row's `❯`, the reasoning header's `∴`, the answer's `∷`, the
/// reasoning expand affordance's `▸`, and a tool badge's `▸` all separate
/// their glyph from the text it precedes by exactly this many columns, so no
/// marker invents spacing of its own.
pub(in crate::tui) const MARKER_GAP: usize = 1;

/// Returns one marker's glyph and the text it opens, separated by the shared
/// marker gap.
pub(in crate::tui) fn marker_text(glyph: &str, text: &str) -> String {
    format!("{glyph}{}{text}", " ".repeat(MARKER_GAP))
}

/// How many blank rows separate two consecutive transcript blocks.
const BLOCK_GAP: usize = 1;

/// The widget's own neutral inks, mapped onto the palette's divider ink.
const WIDGET_NEUTRALS: [Color; 3] = [theme::DISABLED_FG, theme::DARK_GRAY, theme::PLACEHOLDER_FG];

/// One transcript display row in the neutral layout.
///
/// The row owns its characters and the runs that style them, so a caller can
/// clip it, frame it, or materialise it without losing a single style.
pub(in crate::tui) struct LaidOutRow {
    /// The characters the row paints, exactly as they appear on screen.
    pub(in crate::tui) text: String,
    /// The styled slices of `text`, in paint order.
    pub(in crate::tui) runs: Vec<Run>,
    /// The wash the row sits on.
    pub(in crate::tui) surface: Color,
}

/// One styled slice of one laid-out row.
pub(in crate::tui) struct Run {
    /// The byte range of [`LaidOutRow::text`] this run paints.
    pub(in crate::tui) range: Range<usize>,
    /// The ink and modifiers the slice paints with.
    pub(in crate::tui) style: RowStyleId,
}

/// The closed set of inks, and parser modifiers, one run can paint with.
///
/// An id names a role and the modifier bits the markdown parser carried for the
/// run's segment; [`RowStyleId::style`] resolves the pair plus the surface the
/// row sits on into one [`Style`]. A visible row therefore builds one `Style`
/// per run, never one per character, and a colour is never invented here: every
/// ink is a [`crate::tui::palette`] role or the parser's own callout banner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) enum RowStyleId {
    /// Ordinary body text in the palette's body ink.
    Body(Modifier),
    /// A heading line: the heading ink.
    Heading(Modifier),
    /// Inline code and code-fence text: the code ink.
    Code(Modifier),
    /// Link text: the link ink.
    Link(Modifier),
    /// The parser's neutral grid, rule, and quote tone: the divider ink.
    Divider(Modifier),
    /// A parsed segment colour this module does not configure: revue's callout
    /// banner, kept exactly as the parser carries it.
    // @todo(revue): the markdown parser hands this module raw colours instead of
    // role ids, so one segment cannot be expressed as a palette role and this
    // variant is the only colour the view carries unresolved; a parser that
    // reports roles would keep the palette the single source of colour.
    Callout(Color, Modifier),
    /// A committed reasoning block's body row.
    Reasoning,
    /// The reasoning block's glyph and header text.
    ReasoningHeader,
    /// The expand affordance of a collapsed reasoning block.
    ReasoningMarker,
    /// A quiet bare mark in the muted ink: the glyph that opens an answer.
    Muted,
    /// A daemon notice block's marker and text.
    Notice,
    /// The accent ink: the bar that closes a reasoning block.
    Accent,
    /// The frame of a user message's card.
    UserBorder,
    /// The `you` label of a user message's card.
    UserLabel,
    /// The glyph and separators of one tool block.
    ToolBorder,
    /// The badge of one tool block's header.
    ToolLabel,
    /// The parsed preview content of one read-only tool block.
    ToolPreview,
    /// A muted note inside one tool block: a fact the durable rows do not carry.
    ToolNote,
    /// An `@todo(core)` plate row a tool block cannot fill from the wire.
    Todo,
}

impl RowStyleId {
    /// Returns the id of one parsed markdown segment.
    ///
    /// A segment with no colour is body text. The widget's own neutrals (its
    /// grid and rule tone and the placeholder tone of a heading marker and a
    /// quote) become the palette's divider ink; every other colour the parser
    /// carries is compared against the resolved palette, because the parser was
    /// configured from those very roles.
    pub(in crate::tui) fn of_parsed(
        fg: Option<Color>,
        modifier: Modifier,
        palette: &'static Palette,
    ) -> Self {
        let Some(fg) = fg else {
            return Self::Body(modifier);
        };
        if WIDGET_NEUTRALS.contains(&fg) {
            return Self::Divider(modifier);
        }
        if fg == palette.markdown_heading {
            Self::Heading(modifier)
        } else if fg == palette.markdown_code {
            Self::Code(modifier)
        } else if fg == palette.markdown_link {
            Self::Link(modifier)
        } else {
            Self::Callout(fg, modifier)
        }
    }

    /// Resolves one run into the style it paints with on `surface`.
    ///
    /// Every ink comes from the resolved `palette`, and the `bg` is the wash
    /// the row named; the parser's modifier bits are carried over so bold
    /// really is bold.
    pub(in crate::tui) const fn style(self, surface: Color, palette: &'static Palette) -> Style {
        match self {
            Self::Body(modifier) => ink(palette.ink, surface, modifier),
            Self::Heading(modifier) => ink(palette.markdown_heading, surface, modifier),
            Self::Code(modifier) => ink(palette.markdown_code, surface, modifier),
            Self::Link(modifier) => ink(palette.markdown_link, surface, modifier),
            Self::Divider(modifier) => ink(palette.ink_faint, surface, modifier),
            Self::Callout(callout, modifier) => ink(callout, surface, modifier),
            Self::Reasoning => ink(palette.reasoning_body, surface, Modifier::empty()),
            Self::ReasoningHeader => ink(palette.reasoning_header, surface, Modifier::empty()),
            Self::ReasoningMarker => ink(palette.reasoning_marker, surface, Modifier::BOLD),
            Self::Muted => ink(palette.ink_muted, surface, Modifier::empty()),
            Self::Notice => ink(palette.notice_ink, surface, Modifier::empty()),
            Self::Accent => ink(palette.accent, surface, Modifier::empty()),
            Self::UserBorder => ink(palette.user_border, surface, Modifier::empty()),
            Self::UserLabel => ink(palette.user_label, surface, Modifier::BOLD),
            Self::ToolBorder => ink(palette.tool_border, surface, Modifier::empty()),
            Self::ToolLabel => ink(palette.tool_label, surface, Modifier::BOLD),
            Self::ToolPreview => ink(palette.tool_preview, surface, Modifier::empty()),
            Self::ToolNote => ink(palette.ink_muted, surface, Modifier::DIM),
            Self::Todo => ink(palette.todo_ink, surface, Modifier::empty()),
        }
    }
}

/// Returns one ink on one wash with the parser's modifier bits.
const fn ink(fg: Color, bg: Color, modifier: Modifier) -> Style {
    Style {
        fg: Some(fg),
        bg: Some(bg),
        bold: modifier.contains(Modifier::BOLD),
        italic: modifier.contains(Modifier::ITALIC),
        underline: modifier.contains(Modifier::UNDERLINE),
        dim: modifier.contains(Modifier::DIM),
        strikethrough: modifier.contains(Modifier::CROSSED_OUT),
        reverse: false,
    }
}

impl LaidOutRow {
    /// Creates one row from its text, runs, and surface.
    pub(in crate::tui) const fn new(text: String, runs: Vec<Run>, surface: Color) -> Self {
        Self {
            text,
            runs,
            surface,
        }
    }

    /// Creates one blank row; it paints nothing, so the surface shows through.
    pub(in crate::tui) const fn blank(surface: Color) -> Self {
        Self {
            text: String::new(),
            runs: Vec::new(),
            surface,
        }
    }

    /// Returns whether the row paints no characters.
    #[cfg(test)]
    pub(in crate::tui) const fn is_blank(&self) -> bool {
        self.text.is_empty()
    }

    /// Returns the row painted on `surface` instead of its own wash.
    ///
    /// The runs resolve their ink against the surface when the row is
    /// materialised, so repainting a row - the transcript's selection fill -
    /// needs no per-run work.
    #[must_use]
    pub(in crate::tui) const fn on_surface(mut self, surface: Color) -> Self {
        self.surface = surface;
        self
    }

    /// Returns whether the row is a collapsed reasoning block's expand marker.
    pub(in crate::tui) fn is_reasoning_marker(&self) -> bool {
        self.runs
            .iter()
            .any(|run| run.style == RowStyleId::ReasoningMarker)
    }

    /// Returns the row's display width in columns.
    #[cfg(test)]
    pub(in crate::tui) fn width(&self) -> usize {
        display_width(&self.text)
    }

    /// Returns the row's run styles, in paint order.
    #[cfg(test)]
    pub(in crate::tui) fn styles(&self) -> Vec<RowStyleId> {
        self.runs.iter().map(|run| run.style).collect()
    }

    /// Materialises the row as one widget, resolving one style per run.
    pub(in crate::tui) fn rich_text(&self, palette: &'static Palette) -> RichText {
        self.rich_text_on(self.surface, palette)
    }

    /// Materialises the row on `surface` instead of its own wash.
    ///
    /// The transcript paints its selected display rows this way: the runs
    /// resolve their ink against the surface the row is painted on, so a
    /// selection repaints a row without touching a single run.
    pub(in crate::tui) fn rich_text_on(
        &self,
        surface: Color,
        palette: &'static Palette,
    ) -> RichText {
        if self.runs.is_empty() {
            return RichText::plain(self.text.clone()).default_style(Style::new().bg(surface));
        }
        let mut text = RichText::new();
        for run in &self.runs {
            let slice = self.text[run.range.clone()].to_owned();
            text = text.span(Span::styled(slice, run.style.style(surface, palette)));
        }
        text
    }
}

/// The growable row one layout pass builds.
///
/// A builder tracks its own display width, so a wrap decision never re-measures
/// the characters it has already assembled.
pub(in crate::tui) struct RowBuilder {
    /// The row's text assembled so far.
    text: String,
    /// The styled slices of `text`, in paint order.
    runs: Vec<Run>,
    /// The display width of `text`, in columns.
    width: usize,
}

impl RowBuilder {
    /// Creates an empty row.
    pub(in crate::tui) const fn new() -> Self {
        Self {
            text: String::new(),
            runs: Vec::new(),
            width: 0,
        }
    }

    /// Appends one slice with `style`.
    ///
    /// A slice that continues the previous run's slice and style extends that
    /// run instead of adding one, so a row carries one run per style change.
    pub(in crate::tui) fn push(&mut self, slice: &str, style: RowStyleId) {
        if slice.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(slice);
        self.width += display_width(slice);
        if let Some(last) = self.runs.last_mut()
            && last.style == style
            && last.range.end == start
        {
            last.range.end = start + slice.len();
            return;
        }
        self.runs.push(Run {
            range: start..start + slice.len(),
            style,
        });
    }

    /// Appends `row`'s runs, clipped to `budget` columns, and returns the
    /// columns actually written.
    ///
    /// A run is cut mid-way when its tail would cross the budget, so a card
    /// frame never swallows the text after an over-long line.
    pub(in crate::tui) fn push_clipped(&mut self, row: &LaidOutRow, budget: usize) -> usize {
        let mut used = 0;
        for run in &row.runs {
            if used >= budget {
                break;
            }
            let content = &row.text[run.range.clone()];
            let mut clipped = content.len();
            let mut clipped_width = 0;
            let mut cut = false;
            for (offset, character) in content.char_indices() {
                let character_width = usize::from(char_width(character));
                if clipped_width + character_width > budget - used {
                    clipped = offset;
                    cut = true;
                    break;
                }
                clipped_width += character_width;
            }
            if clipped > 0 {
                self.push(&content[..clipped], run.style);
                used += clipped_width;
            }
            if cut {
                break;
            }
        }
        used
    }

    /// Returns the display width of the row so far.
    pub(in crate::tui) const fn width(&self) -> usize {
        self.width
    }

    /// Returns whether the row paints nothing yet.
    pub(in crate::tui) const fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Splits the row at its last space, keeping the head and returning the
    /// part after the space as a new row.
    ///
    /// The space itself is dropped, which is exactly what a word break does. A
    /// row with no space is one word that overflowed: it keeps everything and
    /// the returned row is empty.
    pub(in crate::tui) fn split_at_last_space(&mut self) -> Self {
        let Some(space) = self.text.rfind(' ') else {
            return Self::new();
        };
        let dropped = display_width(&self.text[space..]);
        let tail = self.text.split_off(space + 1);
        let tail_width = self.width.saturating_sub(display_width(&self.text));
        self.text.truncate(space);
        self.width -= dropped;
        let (head_runs, tail_runs) = split_runs(std::mem::take(&mut self.runs), space);
        self.runs = head_runs;
        Self {
            text: tail,
            runs: tail_runs,
            width: tail_width,
        }
    }

    /// Prepends the continuation indent, styled like the row's first run.
    pub(in crate::tui) fn prepend_indent(&mut self) {
        let style = self
            .runs
            .first()
            .map_or(RowStyleId::Body(Modifier::empty()), |run| run.style);
        let mut runs = vec![Run {
            range: 0..CONTINUATION_INDENT.len(),
            style,
        }];
        let shift = CONTINUATION_INDENT.len();
        for run in std::mem::take(&mut self.runs) {
            let range = (run.range.start + shift)..(run.range.end + shift);
            if let Some(last) = runs.last_mut()
                && last.style == run.style
                && last.range.end == range.start
            {
                last.range.end = range.end;
                continue;
            }
            runs.push(Run {
                range,
                style: run.style,
            });
        }
        let mut text = String::with_capacity(CONTINUATION_INDENT.len() + self.text.len());
        text.push_str(CONTINUATION_INDENT);
        text.push_str(&self.text);
        self.text = text;
        self.width += CONTINUATION_INDENT.len();
        self.runs = runs;
    }

    /// Finishes the row on `surface`.
    pub(in crate::tui) fn finish(self, surface: Color) -> LaidOutRow {
        LaidOutRow::new(self.text, self.runs, surface)
    }
}

/// Splits one run list at the byte `space` a wrap drops.
///
/// Returns the runs before the dropped byte and the runs after it, the latter
/// re-based so they index the text that moves to the next row.
// @todo(revue): revue carries styled runs but no width-aware text engine, so
// the wrap splits a run list at the dropped space and re-bases every tail
// run's byte range here; a wrapping text engine would own this.
fn split_runs(runs: Vec<Run>, space: usize) -> (Vec<Run>, Vec<Run>) {
    let mut head = Vec::new();
    let mut tail = Vec::new();
    for run in runs {
        if run.range.end <= space {
            head.push(run);
            continue;
        }
        if run.range.start > space {
            tail.push(Run {
                range: (run.range.start - space - 1)..(run.range.end - space - 1),
                style: run.style,
            });
            continue;
        }
        // The run holds the dropped space: its head stays, its tail moves.
        if run.range.start < space {
            head.push(Run {
                range: run.range.start..space,
                style: run.style,
            });
        }
        if run.range.end > space + 1 {
            tail.push(Run {
                range: 0..(run.range.end - space - 1),
                style: run.style,
            });
        }
    }
    (head, tail)
}

/// One committed message's blocks, in paint order.
///
/// Most messages lay out one block; an assistant row lays out its reasoning
/// block, when it carries one, plus its answer block, and a message that
/// renders nothing - a tool result its committed call already draws - lays out
/// none. [`append_messages`] is the one place that knows the gap rule and
/// which display rows each message ends up owning.
pub(in crate::tui) type MessageBlocks = Vec<Vec<LaidOutRow>>;

/// The display rows one committed message owns inside the cached layout.
#[derive(Clone, Debug, Eq, PartialEq)]
struct BlockSpan {
    /// The committed transcript row the display rows belong to.
    message: usize,
    /// The message's own display rows; empty for a message that lays out none.
    rows: Range<usize>,
}

/// Appends one block per entry, [`BLOCK_GAP`] blank rows between two blocks,
/// and returns the display rows each block owns.
///
/// This is the transcript's one gap rule: the cache uses it for the committed
/// blocks it lays out, and the pane uses it for the provisional tail it lays
/// out every frame. A block that lays out no row adds no row and no gap, and
/// an empty result never opens with a gap.
pub(in crate::tui) fn append_blocks(
    rows: &mut Vec<LaidOutRow>,
    blocks: Vec<Vec<LaidOutRow>>,
    palette: &'static Palette,
) -> Vec<Range<usize>> {
    let mut spans = Vec::with_capacity(blocks.len());
    for block in blocks {
        if block.is_empty() {
            spans.push(rows.len()..rows.len());
            continue;
        }
        if !rows.is_empty() {
            for _ in 0..BLOCK_GAP {
                rows.push(LaidOutRow::blank(palette.panel));
            }
        }
        let start = rows.len();
        rows.extend(block);
        spans.push(start..rows.len());
    }
    spans
}

/// Appends one entry per committed message, one display-row range per message
/// in message order.
///
/// The ranges are what turns a hit display row back into the committed row
/// that produced it; a message that lays out nothing owns an empty range.
pub(in crate::tui) fn append_messages(
    rows: &mut Vec<LaidOutRow>,
    messages: Vec<MessageBlocks>,
    palette: &'static Palette,
) -> Vec<Range<usize>> {
    messages
        .into_iter()
        .map(|blocks| {
            let ranges = append_blocks(rows, blocks, palette);
            match (ranges.first(), ranges.last()) {
                (Some(first), Some(last)) => first.start..last.end,
                _ => rows.len()..rows.len(),
            }
        })
        .collect()
}

/// Returns the top row of one framed transcript block.
///
/// The frame is drawn as flat display rows, not as a nested `Border` widget, so
/// the transcript's window can start and end anywhere inside a block and every
/// row the frame paints is counted exactly once. `label` is embedded in the top
/// border between its own two border cells; the caller bounds it to the width,
/// because this row never clips a label mid-word.
pub(in crate::tui) fn box_top(
    label: Option<(&str, RowStyleId)>,
    width: usize,
    border: RowStyleId,
    surface: Color,
) -> LaidOutRow {
    let mut row = RowBuilder::new();
    row.push("╭─", border);
    if let Some((label, ink)) = label {
        row.push(" ", border);
        row.push(label, ink);
        row.push(" ", border);
    }
    let fill = width.saturating_sub(row.width() + 1);
    row.push(&"─".repeat(fill), border);
    row.push("╮", border);
    row.finish(surface)
}

/// Returns one framed content row: the borders and one padding column on either
/// side of `row`, clipped to `inner` columns.
///
/// The padding takes the row's own wash, so a framed row of one block never
/// shows a seam of another block's surface.
pub(in crate::tui) fn box_content(
    row: &LaidOutRow,
    inner: usize,
    border: RowStyleId,
) -> LaidOutRow {
    let mut content = RowBuilder::new();
    content.push("│ ", border);
    let used = content.push_clipped(row, inner);
    if used < inner {
        content.push(&" ".repeat(inner - used), border);
    }
    content.push(" │", border);
    content.finish(row.surface)
}

/// Returns the bottom row of one framed transcript block.
pub(in crate::tui) fn box_bottom(width: usize, border: RowStyleId, surface: Color) -> LaidOutRow {
    let mut row = RowBuilder::new();
    row.push("╰", border);
    row.push(&"─".repeat(width.saturating_sub(2)), border);
    row.push("╯", border);
    row.finish(surface)
}

/// The transcript window one frame painted, published for the next event.
///
/// The pane owns the frame's geometry, and the next mouse event hit-tests
/// against what it published: which screen rows and columns the window
/// occupies, which display row it starts at, how many display rows the whole
/// transcript has, and where the live reasoning segment's expand marker was
/// painted. The core never sees these values; the front end reads them back to
/// turn a cell into a display row.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::tui) struct TranscriptWindow {
    /// The screen row of the window's first display row.
    pub(in crate::tui) top: u16,
    /// The display rows the window shows.
    pub(in crate::tui) visible: u16,
    /// The display row the window starts at, oldest first.
    pub(in crate::tui) start: u16,
    /// How many display rows the whole transcript has.
    pub(in crate::tui) total: u16,
    /// The display row of the live reasoning segment's expand marker, when the
    /// live tail collapsed one.
    ///
    /// The live tail is laid out per frame and never enters the cache, so a
    /// marker click can only recognize it through the row the pane published
    /// here; `None` means no live marker was painted.
    pub(in crate::tui) live_reasoning_marker: Option<u16>,
}

impl TranscriptWindow {
    /// Returns the display row one screen row hits, if the window shows one.
    ///
    /// A screen row outside the window, or inside it past the last display
    /// row, hits nothing: a press there anchors no selection.
    pub(in crate::tui) fn row_at(&self, screen_row: u16) -> Option<u16> {
        let offset = screen_row.checked_sub(self.top)?;
        if offset >= self.visible {
            return None;
        }
        let row = self.start.checked_add(offset)?;
        (row < self.total).then_some(row)
    }

    /// Returns whether one screen row lies above the window's rows.
    pub(in crate::tui) const fn above(&self, screen_row: u16) -> bool {
        screen_row < self.top
    }

    /// Returns whether one screen row lies below the window's rows.
    pub(in crate::tui) const fn below(&self, screen_row: u16) -> bool {
        screen_row >= self.top.saturating_add(self.visible)
    }

    /// Returns the display row nearest one screen row, clamped to the window.
    ///
    /// A drag past the top edge keeps extending at the window's first row and
    /// a drag past the bottom edge at its last, so a selection can always
    /// reach the nearest visible row.
    pub(in crate::tui) fn clamped_row(&self, screen_row: u16) -> u16 {
        let offset = screen_row
            .saturating_sub(self.top)
            .min(self.visible.saturating_sub(1));
        self.start
            .saturating_add(offset)
            .min(self.total.saturating_sub(1))
    }

    /// Returns the window one `step`-row scroll in `direction` leaves.
    ///
    /// The transcript's offset counts display rows back from its newest row, so
    /// scrolling towards the older rows lowers the window's first row by `step`
    /// and scrolling towards the newer rows raises it, bounded by the
    /// transcript's own first and last rows. This is the arithmetic the pane
    /// performs from the core's offset, applied here so a dragged selection can
    /// follow the content the scroll moves under the pointer.
    pub(in crate::tui) fn scrolled(self, direction: TranscriptScroll, step: u16) -> Self {
        let start = match direction {
            TranscriptScroll::Older => self.start.saturating_sub(step),
            TranscriptScroll::Newer => self
                .start
                .saturating_add(step)
                .min(self.total.saturating_sub(self.visible)),
        };
        Self { start, ..self }
    }
}

/// Returns the display width of one piece of text, in columns.
pub(in crate::tui) fn display_width(text: &str) -> usize {
    text.chars()
        .map(|character| usize::from(char_width(character)))
        .sum()
}

/// The committed transcript's rows, keyed by width and transcript version.
///
/// The cache is what keeps a frame cheap: typing into the input line and the
/// turn timer's ticks never touch the transcript, so the key still matches and
/// a frame materialises the visible window without laying anything out.
// @todo(revue): revue has no dirty tracking or virtualised rendering of its
// own, so the invalidation is built here: the key carries the width, the
// theme, the transcript epoch, the reasoning-expansion epoch, and the paired
// tool-result count, and an append fast path keeps the cached prefix.
pub(in crate::tui) struct TranscriptLayoutCache {
    /// The content width the cached rows were laid out at.
    width: usize,
    /// The colour theme the cached rows were laid out for.
    ///
    /// A laid-out row carries resolved surfaces, so a theme change makes every
    /// cached row a value of the other theme: a moved theme - a picker preview
    /// included, which is why the cache reads the effective theme - replays the
    /// whole transcript, exactly as a resize does.
    theme: Theme,
    /// The transcript epoch the cached rows reflect.
    epoch: u64,
    /// The reasoning expansion epoch the cached rows reflect.
    ///
    /// An expansion inserts display rows into the middle of the transcript, so
    /// the rows a cache holds stop being a prefix of the next layout: a moved
    /// epoch replays the whole transcript.
    reasoning_epoch: u64,
    /// How many committed `tool_result` rows the cached rows pair.
    ///
    /// A tool block pairs a call row with the result that answers it, so a
    /// committed result changes the block of an earlier call the cached prefix
    /// holds; a moved count replays the whole transcript.
    tool_results: usize,
    /// How many committed transcript rows the cached rows cover.
    len: usize,
    /// The laid-out rows, oldest first.
    rows: Vec<LaidOutRow>,
    /// The display rows each committed message owns, in message order.
    spans: Vec<BlockSpan>,
    /// The transcript window the last frame painted.
    window: TranscriptWindow,
    /// How many layouts the cache performed; a hit leaves it where it was.
    #[cfg(test)]
    layouts: u64,
}

impl TranscriptLayoutCache {
    /// Creates a cache that holds no rows.
    #[must_use]
    pub(in crate::tui) const fn new() -> Self {
        Self {
            width: 0,
            theme: Theme::Light,
            epoch: 0,
            reasoning_epoch: 0,
            tool_results: 0,
            len: 0,
            rows: Vec::new(),
            spans: Vec::new(),
            window: TranscriptWindow {
                top: 0,
                visible: 0,
                start: 0,
                total: 0,
                live_reasoning_marker: None,
            },
            #[cfg(test)]
            layouts: 0,
        }
    }

    /// Returns the committed transcript's rows at `width`, laying out only what
    /// the cache does not already hold.
    ///
    /// `blocks` lays out one committed-row range as its messages' blocks, each
    /// block a run of display rows: the whole transcript on a miss, and only
    /// the appended tail when the transcript grew by appends since the cached
    /// epoch. A hit calls it not at all. `palette` is the resolved palette of
    /// the frame, and the cache reads its theme from the same state, so the
    /// gap rows and the blocks paint one theme.
    ///
    /// The cached rows are current only while the width, the theme, the
    /// transcript epoch, the reasoning expansion epoch, and the committed tool
    /// results all match; anything else - a replacement, a trim's front drain,
    /// a session switch, a resize, a theme change, an expansion, a result that
    /// answers an earlier call, or no cache yet - is a full replay.
    pub(in crate::tui) fn rows(
        &mut self,
        state: &AppState,
        width: usize,
        palette: &'static Palette,
        blocks: impl FnOnce(Range<usize>, usize) -> Vec<MessageBlocks>,
    ) -> &[LaidOutRow] {
        let width = width.max(1);
        let theme = state.effective_theme();
        let epoch = state.transcript_epoch();
        let reasoning_epoch = state.reasoning_epoch();
        let tool_results = state.tool_result_count();
        let appended = if width == self.width
            && theme == self.theme
            && reasoning_epoch == self.reasoning_epoch
            && tool_results == self.tool_results
        {
            state.transcript_appended_since(self.epoch)
        } else {
            None
        };
        match appended {
            // The rows the cache holds are current: lay out nothing.
            Some(0) => {}
            // Only appends: lay out the tail and keep the cached prefix.
            Some(appended) => {
                let from = self.len;
                debug_assert_eq!(
                    from + appended,
                    state.transcript().len(),
                    "an append-only answer covers exactly the rows the transcript gained"
                );
                let laid_out = blocks(from..from + appended, width);
                self.epoch = epoch;
                self.len = from + appended;
                self.append(laid_out, from, palette);
                #[cfg(test)]
                self.count_layout();
            }
            // A replacement, a trim, a resize, a theme change, an expansion, a
            // paired result, or a first layout: lay out all.
            None => {
                let laid_out = blocks(0..state.transcript().len(), width);
                self.rows.clear();
                self.spans.clear();
                self.epoch = epoch;
                self.theme = theme;
                self.reasoning_epoch = reasoning_epoch;
                self.tool_results = tool_results;
                self.len = state.transcript().len();
                self.width = width;
                self.append(laid_out, 0, palette);
                #[cfg(test)]
                self.count_layout();
            }
        }
        &self.rows
    }

    /// Appends one range of laid-out messages, recording the display rows each
    /// message owns from `from`.
    fn append(&mut self, messages: Vec<MessageBlocks>, from: usize, palette: &'static Palette) {
        for (offset, rows) in append_messages(&mut self.rows, messages, palette)
            .into_iter()
            .enumerate()
        {
            self.spans.push(BlockSpan {
                message: from + offset,
                rows,
            });
        }
    }

    /// Returns the laid-out rows the last layout produced, oldest first.
    pub(in crate::tui) fn laid_out(&self) -> &[LaidOutRow] {
        &self.rows
    }

    /// Returns the committed message whose block holds one display row.
    pub(in crate::tui) fn message_at(&self, row: usize) -> Option<usize> {
        self.spans
            .iter()
            .find(|span| span.rows.contains(&row))
            .map(|span| span.message)
    }

    /// Returns the transcript window the last frame published.
    pub(in crate::tui) const fn window(&self) -> TranscriptWindow {
        self.window
    }

    /// Records the transcript window one frame painted.
    pub(in crate::tui) const fn publish_window(&mut self, window: TranscriptWindow) {
        self.window = window;
    }

    /// Records one layout pass.
    #[cfg(test)]
    const fn count_layout(&mut self) {
        self.layouts += 1;
    }

    /// Returns how many layouts the cache performed.
    #[cfg(test)]
    pub(in crate::tui) const fn layout_count(&self) -> u64 {
        self.layouts
    }
}
