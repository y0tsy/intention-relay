//! The welcome pane: the product lockup and the environment overview the chat
//! shows while the core has no open session.
//!
//! The pane replaces the transcript's content in the same region - the rows
//! above the input block - so the input block, the detail line, and the panel
//! geometry never move: the lockup sits at the top of the region and the
//! overview block is pinned to the region's last rows, directly above the
//! input block.
//!
//! The overview is one horizontal block of up to four sub-blocks. The product
//! version is a compile-time fact of this binary, so it is the one real
//! sub-block; the AGENTS.md, MCPs, and Skills sub-blocks are `@todo(core)`
//! plates, because the client has no instruction-source, MCP, or skills
//! support to report yet. A plate is deliberate: it carries the stub wash and
//! the placeholder ink, and never a fabricated count or a fake tick.
//!
//! revue has no font sizes, so the lockup approximates one: the product word
//! is written bold in an accent ink and the trailing word is tracked out one
//! column per letter in the muted ink, which is the classic small-caps
//! treatment a terminal carries. The alternatives were Unicode small caps or
//! modifier letters (a different glyph repertoire whose width no terminal
//! measures the same way) and weight alone (which leaves the two words reading
//! as one uniform line), so the tracked muted form is the one that keeps every
//! character monochrome and every column measured through `char_width`.

use revue::style::Color;
use revue::widget::{Border, RichText, Span, Stack, Style, vstack};

use crate::app::AppState;
use crate::tui::layout::display_width;
use crate::tui::palette::Palette;

/// The product word of the lockup.
const PRODUCT: &str = "INTENTION";

/// The trailing word of the lockup.
const TRAILING: &str = "RELAY";

/// The columns the lockup keeps between its two words.
///
/// The trailing word starts this many columns after the product word ends, so
/// the pair reads as one lockup rather than two labels.
const LOGO_GAP: usize = 2;

/// The blank rows the pane keeps above the lockup.
const LOGO_LEAD_ROWS: u16 = 1;

/// The rows the lockup itself occupies.
const LOGO_ROWS: u16 = 1;

/// The rows the lockup area occupies when the region has room for it.
const LOCKUP_ROWS: u16 = LOGO_LEAD_ROWS + LOGO_ROWS;

/// The rows the framed overview occupies: its top edge, its sub-block row, and
/// its bottom edge.
const OVERVIEW_ROWS: u16 = 3;

/// The columns the overview's own frame takes.
const OVERVIEW_FRAME: usize = 2;

/// The label of the product-version sub-block.
const VERSION_LABEL: &str = "Version";

/// The label of the project-instruction sub-block.
const AGENTS_LABEL: &str = "AGENTS.md";

/// The label of the MCP-capability sub-block.
const MCPS_LABEL: &str = "MCPs";

/// The label of the skills sub-block.
const SKILLS_LABEL: &str = "Skills";

/// The version this binary was built as.
///
/// The workspace publishes no separate product version, so the lockup names
/// the version of the binary that draws it.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The visible marker of every sub-block the client cannot fill yet.
// @todo(core): the instruction sources (AGENTS.md loading), MCP capabilities,
// and skills have no client support and no wire projection, so those
// sub-blocks show this deliberate plate instead of a fabricated count or tick.
const STUB: &str = "@todo(core)";

/// Returns whether the chat shows the welcome surface.
///
/// The welcome is the chat's empty state: no session is open and no committed
/// row exists. An open session without rows still shows the transcript pane,
/// and a row without an open session still shows its content, so the welcome
/// can never cover work the core already reports.
pub(in crate::tui) fn is_welcome(state: &AppState) -> bool {
    state.session_id().is_none() && state.transcript().is_empty()
}

/// The rows the welcome region spends, top to bottom.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WelcomeLayout {
    /// The blank rows above the lockup.
    lead: u16,
    /// The rows the lockup occupies.
    logo: u16,
    /// The blank rows between the lockup and the overview.
    filler: u16,
    /// The rows the overview occupies.
    overview: u16,
}

/// Returns how the welcome region spends `rows` rows.
///
/// The overview is pinned to the region's last rows and the lockup is dropped
/// before the overview is, so every row count leaves a coherent surface: a
/// region with room for nothing draws nothing, and one too short for the
/// lockup still draws the overview.
const fn layout(rows: u16) -> WelcomeLayout {
    let overview = if rows >= OVERVIEW_ROWS {
        OVERVIEW_ROWS
    } else {
        rows
    };
    let above = rows - overview;
    let lockup = if above >= LOCKUP_ROWS {
        LOCKUP_ROWS
    } else {
        above
    };
    let lead = if lockup >= LOCKUP_ROWS {
        LOGO_LEAD_ROWS
    } else {
        0
    };
    WelcomeLayout {
        lead,
        logo: lockup - lead,
        filler: above - lockup,
        overview,
    }
}

/// Returns the welcome pane: the lockup and the overview, filling exactly the
/// rows and columns of the transcript region it replaces.
pub(in crate::tui) fn welcome_pane(rows: u16, columns: u16, palette: &'static Palette) -> Stack {
    let layout = layout(rows);
    let width = usize::from(columns);
    let mut body = vstack();
    if layout.lead > 0 {
        body = body.child_sized(blank_row(width, palette), layout.lead);
    }
    if layout.logo > 0 {
        body = body.child_sized(logo_row(width, palette), layout.logo);
    }
    if layout.filler > 0 {
        body = body.child_sized(blank_row(width, palette), layout.filler);
    }
    if layout.overview > 0 {
        body = body.child_sized(overview(layout.overview, width, palette), layout.overview);
    }
    body
}

/// Returns the lockup: the product word, the documented gap, and the tracked
/// trailing word, centred in `width` columns.
///
/// Every column is measured through `char_width`, so the lockup and its pads
/// fill exactly `width` columns however a terminal renders the letters.
fn logo_row(width: usize, palette: &'static Palette) -> RichText {
    let trailing = tracked(TRAILING);
    let lockup = display_width(PRODUCT) + LOGO_GAP + display_width(&trailing);
    let left = width.saturating_sub(lockup) / 2;
    let right = width.saturating_sub(lockup + left);
    let mut text = RichText::new();
    if left > 0 {
        text = text.span(padding(left, palette));
    }
    text = text.span(Span::styled(
        PRODUCT,
        Style::new()
            .fg(palette.accent_deep)
            .bg(palette.panel)
            .bold(),
    ));
    text = text.span(padding(LOGO_GAP, palette));
    text = text.span(Span::styled(
        trailing,
        Style::new().fg(palette.ink_muted).bg(palette.panel),
    ));
    if right > 0 {
        text = text.span(padding(right, palette));
    }
    text
}

/// Returns `word` with one blank column between its letters.
///
/// Tracking is the size cue this front end has: every letter is still one
/// cell wide, and the spaced word reads as the smaller half of the lockup.
// @todo(revue): revue has no font sizes, so the lockup fakes a type scale with
// letter tracking, and the welcome region budgets and fits its sub-blocks by
// hand; a size or scale surface would let the lockup and the region be
// expressed directly.
fn tracked(word: &str) -> String {
    word.chars()
        .map(|character| character.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Returns the overview: the sub-block row, framed while the region has room
/// for the frame.
fn overview(rows: u16, width: usize, palette: &'static Palette) -> Stack {
    let framed = rows >= OVERVIEW_ROWS;
    let content = width.saturating_sub(if framed { OVERVIEW_FRAME } else { 0 });
    let chips = chips_row(content, palette);
    if framed {
        vstack().child_sized(
            Border::rounded()
                .min_size(as_width(width), rows)
                .max_size(as_width(width), rows)
                .fg(palette.border)
                .bg(palette.panel)
                .child(chips),
            rows,
        )
    } else {
        vstack().child_sized(chips, rows)
    }
}

/// Returns the sub-block row: every sub-block that fits, in order, on its own
/// wash, then the panel fill.
///
/// The row is laid out left to right and a sub-block that does not fit ends the
/// row whole, so a narrow welcome shows fewer sub-blocks instead of half of
/// one. That is "up to four": the overview is one block, never a wrapped one.
fn chips_row(width: usize, palette: &'static Palette) -> RichText {
    let mut text = RichText::new();
    let mut used = 0;
    for sub_block in fitted(width, palette) {
        let chip = sub_block.chip();
        let chip_width = display_width(&chip);
        if used > 0 {
            text = text.span(padding(1, palette));
            used += 1;
        }
        text = text.span(Span::styled(
            chip,
            Style::new().fg(sub_block.ink).bg(sub_block.wash),
        ));
        used += chip_width;
    }
    if used < width {
        text = text.span(padding(width - used, palette));
    }
    text
}

/// Returns the sub-blocks of the overview, in display order.
const fn sub_blocks(palette: &'static Palette) -> [SubBlock; 4] {
    [
        SubBlock {
            label: VERSION_LABEL,
            state: VERSION,
            ink: palette.badge_ink,
            wash: palette.badge_bg,
        },
        SubBlock {
            label: AGENTS_LABEL,
            state: STUB,
            ink: palette.todo_ink,
            wash: palette.stub_bg,
        },
        SubBlock {
            label: MCPS_LABEL,
            state: STUB,
            ink: palette.todo_ink,
            wash: palette.stub_bg,
        },
        SubBlock {
            label: SKILLS_LABEL,
            state: STUB,
            ink: palette.todo_ink,
            wash: palette.stub_bg,
        },
    ]
}

/// Returns the leading sub-blocks that fit in `width` columns, in order.
// @todo(revue): revue has no wrap-and-clip container, so the overview's chips
// are measured one by one and the first that does not fit ends the row - a
// hand-written `flex-wrap: nowrap; overflow: hidden`, as in the sessions
// legend.
fn fitted(width: usize, palette: &'static Palette) -> Vec<SubBlock> {
    let mut fitted = Vec::new();
    let mut used = 0;
    for (index, sub_block) in sub_blocks(palette).into_iter().enumerate() {
        let chip_width = display_width(&sub_block.chip());
        let gap = usize::from(index > 0);
        if used + gap + chip_width > width {
            break;
        }
        used += gap + chip_width;
        fitted.push(sub_block);
    }
    fitted
}

/// One sub-block of the overview.
#[derive(Clone, Copy, Debug)]
struct SubBlock {
    /// The short label of the state the sub-block reports.
    label: &'static str,
    /// The value or placeholder beside the label.
    state: &'static str,
    /// The ink of the sub-block's text.
    ink: Color,
    /// The wash behind the sub-block.
    wash: Color,
}

impl SubBlock {
    /// Returns the sub-block's one-line text.
    fn chip(self) -> String {
        format!("{} {}", self.label, self.state)
    }
}

/// Returns a blank run of `width` columns on the panel wash.
fn padding(width: usize, palette: &'static Palette) -> Span {
    Span::styled(" ".repeat(width), Style::new().bg(palette.panel))
}

/// Returns one blank row of `width` columns on the panel wash.
fn blank_row(width: usize, palette: &'static Palette) -> RichText {
    RichText::new().span(padding(width, palette))
}

/// Returns one column count as the terminal's column type.
fn as_width(columns: usize) -> u16 {
    u16::try_from(columns).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "The pane unit tests build typed fixtures and assert rendered cells directly."
    )]

    use revue::testing::TestApp;

    use super::{
        LOGO_GAP, OVERVIEW_ROWS, PRODUCT, TRAILING, WelcomeLayout, chips_row, display_width,
        fitted, layout, sub_blocks, tracked,
    };
    use crate::tui::palette::Palette;

    /// The light palette every fixture renders with.
    const LIGHT: &Palette = &crate::tui::palette::LIGHT;

    /// Returns the rows one layout spends.
    const fn spent(layout: &WelcomeLayout) -> u16 {
        layout.lead + layout.logo + layout.filler + layout.overview
    }

    #[test]
    fn the_welcome_layout_spends_every_row_it_is_given() {
        for rows in 0..=12 {
            let layout = layout(rows);
            assert_eq!(spent(&layout), rows, "a {rows}-row region spends its rows");
        }
    }

    #[test]
    fn the_welcome_keeps_the_overview_when_the_lockup_no_longer_fits() {
        let roomy = layout(8);
        assert_eq!(roomy.overview, OVERVIEW_ROWS);
        assert_eq!(roomy.lead, 1, "the lockup keeps its lead row");
        assert_eq!(roomy.logo, 1);
        let cramped = layout(OVERVIEW_ROWS);
        assert_eq!(cramped.overview, OVERVIEW_ROWS);
        assert_eq!(cramped.lead, 0, "the lockup gives way before the overview");
        assert_eq!(cramped.logo, 0);
        let tiny = layout(1);
        assert_eq!(
            tiny.overview, 1,
            "a one-row region still shows the overview"
        );
        assert_eq!(layout(0).overview, 0);
    }

    #[test]
    fn the_trailing_word_is_tracked_out_one_column_per_letter() {
        assert_eq!(tracked(TRAILING), "R E L A Y");
        assert_eq!(
            display_width(&tracked(TRAILING)),
            2 * display_width(TRAILING) - 1,
            "every letter but the last carries its own tracking column"
        );
    }

    #[test]
    fn the_lockup_keeps_its_documented_gap() {
        assert_eq!(PRODUCT, "INTENTION");
        assert_eq!(LOGO_GAP, 2, "the trailing word starts two columns along");
    }

    #[test]
    fn the_overview_shows_the_sub_blocks_in_order_and_drops_whole_ones() {
        let blocks = sub_blocks(LIGHT);
        let all = fitted(usize::MAX, LIGHT);
        assert_eq!(
            all.iter().map(|block| block.label).collect::<Vec<_>>(),
            vec!["Version", "AGENTS.md", "MCPs", "Skills"],
            "the overview lists the version, the instructions, the MCPs, and the skills"
        );
        assert_eq!(
            all.iter().map(|block| block.state).collect::<Vec<_>>(),
            vec![
                env!("CARGO_PKG_VERSION"),
                "@todo(core)",
                "@todo(core)",
                "@todo(core)"
            ],
            "only the version is a fact; the rest are deliberate plates"
        );
        let chips: Vec<usize> = blocks
            .iter()
            .map(|block| display_width(&block.chip()))
            .collect();
        let total = chips.iter().sum::<usize>() + blocks.len() - 1;
        assert_eq!(fitted(total, LIGHT).len(), 4, "the full row holds all four");
        assert_eq!(
            fitted(total - 1, LIGHT).len(),
            3,
            "one column short drops the last sub-block whole"
        );
        assert_eq!(
            fitted(chips[0], LIGHT).len(),
            1,
            "the first sub-block alone fits"
        );
        assert_eq!(
            fitted(chips[0] - 1, LIGHT).len(),
            0,
            "nothing fits below it"
        );
    }

    #[test]
    fn the_version_carries_the_badge_surface_and_a_stub_carries_the_stub_plate() {
        let app = TestApp::with_size(chips_row(80, LIGHT), 80, 1);
        let version = app.buffer().get(0, 0).expect("the version chip paints");
        assert_eq!(version.fg, Some(LIGHT.badge_ink));
        assert_eq!(version.bg, Some(LIGHT.badge_bg));
        let (stub_x, stub_row) = app
            .find_text("Skills @todo(core)")
            .expect("the skills stub renders");
        let stub = app
            .buffer()
            .get(stub_x, stub_row)
            .expect("the stub chip paints");
        assert_eq!(stub.fg, Some(LIGHT.todo_ink));
        assert_eq!(
            stub.bg,
            Some(LIGHT.stub_bg),
            "a stub carries the deliberate @todo plate, not a status wash"
        );
    }
}
