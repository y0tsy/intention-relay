//! Markdown content as neutral transcript display rows.
//!
//! Both committed assistant answers and committed user cards lay their text out
//! here: the source goes through `tui-markdown`, so code fences, bold, lists,
//! and tables render in either place. Every line the parser produces becomes one
//! or more [`LaidOutRow`] values:
//!
//! - a heading takes [`Palette::markdown_heading`] with bold, and the style
//!   sheet emits no `#` marker for it,
//! - `**strong**` keeps the parser's bold modifier,
//! - a table keeps the parser's box-drawing grid, laid out as an aligned block
//!   and wrapped to the pane by [`tui_markdown::Options::table_width`],
//! - inline code and links keep [`Palette::markdown_code`] and
//!   [`Palette::markdown_link`].
//!
//! The style sheet is configured from the resolved palette, so every ink the
//! parser carries is a palette role: the pane can therefore compare a segment's
//! colour against those roles instead of guessing at a widget's own tones. That
//! is what keeps [`RowStyleId`] a closed set with no unresolved colour in it.
//!
//! The syntax highlighter stays off (`tui-markdown`'s `highlight-code` feature
//! is not enabled), so a fence renders in the code ink rather than in a theme's
//! own colours and the palette remains the one colour system.
//!
//! The returned text is laid out at the caller's width: a long paragraph wraps
//! with the pane's continuation indent, while a box-drawing grid line (a table
//! or a bordered code block) stays atomic and is clipped exactly as the parser
//! clipped it. Wrapping the grid would shred the alignment the table exists to
//! show.
//!
//! The wrap walks `&str` slices of the parsed segments, measures them with
//! `char_width` without allocating, breaks at the last space by a byte scan,
//! and assembles every row with `push_str` of those slices. A row is returned
//! as [`LaidOutRow`], so the user card can frame and clip it without losing the
//! parser's per-segment inks and modifiers, and the caller names the surface
//! the rows sit on, because a card refuses to paint the panel's wash inside its
//! own frame.
// @todo(hack): the parser reports styled lines rather than a row kind, so this
// module re-wraps every parsed line at the caller's width and sniffs box-drawing
// grid lines by their first glyph; a parsed line kind would remove both.

use ratatui::style::{Color, Modifier, Style};
use tui_markdown::{AlertKind, Options, StyleSheet, from_str_with_options};

use crate::tui::layout::{LaidOutRow, RowBuilder, RowStyleId, wrap_segments};
use crate::tui::palette::Palette;

/// The box-drawing glyphs that open an atomic grid or rule line.
const GRID_OPENERS: [char; 4] = ['┌', '├', '└', '│'];

/// The style sheet one markdown block renders through.
///
/// Every method answers from the resolved palette, so the parser's output
/// carries palette roles and nothing else.
#[derive(Clone, Copy, Debug)]
struct PaletteSheet {
    palette: &'static Palette,
}

impl StyleSheet for PaletteSheet {
    fn heading(&self, _level: u8) -> Style {
        Style::new()
            .fg(self.palette.markdown_heading)
            .add_modifier(Modifier::BOLD)
    }

    fn heading_marker(&self, _level: u8) -> &str {
        // A heading reads as a heading through its ink and weight, never
        // through a hash prefix, and a marker would spend a display row.
        ""
    }

    fn code(&self) -> Style {
        Style::new().fg(self.palette.markdown_code)
    }

    fn link(&self) -> Style {
        Style::new().fg(self.palette.markdown_link)
    }

    fn blockquote(&self) -> Style {
        Style::new().fg(self.palette.ink_muted)
    }

    fn html(&self) -> Style {
        Style::new().fg(self.palette.ink_muted)
    }

    fn math_inline(&self) -> Style {
        Style::new().fg(self.palette.ink_muted)
    }

    fn math_display(&self) -> Style {
        Style::new().fg(self.palette.ink_muted)
    }

    fn footnote_ref(&self) -> Style {
        Style::new().fg(self.palette.ink_faint)
    }

    fn footnote_def(&self) -> Style {
        Style::new().fg(self.palette.ink_faint)
    }

    fn definition_term(&self) -> Style {
        Style::new()
            .fg(self.palette.markdown_heading)
            .add_modifier(Modifier::BOLD)
    }

    fn definition_description(&self) -> Style {
        Style::new().fg(self.palette.ink_muted)
    }

    fn alert(&self, _kind: AlertKind) -> Style {
        Style::new().fg(self.palette.notice_ink)
    }

    fn table_header(&self) -> Style {
        Style::new()
            .fg(self.palette.markdown_heading)
            .add_modifier(Modifier::BOLD)
    }

    fn table_border(&self) -> Style {
        Style::new().fg(self.palette.ink_faint)
    }

    fn image_alt(&self) -> Style {
        Style::new()
            .fg(self.palette.ink_muted)
            .add_modifier(Modifier::DIM | Modifier::ITALIC)
    }
}

/// Returns the display rows of one markdown block at `width`, painted on
/// `surface`.
///
/// The rows come from the parsed text's own line inventory, each laid out at
/// `width`, so the transcript window counts the rows the block actually paints.
/// The parser is configured from the resolved `palette`, so the inks it carries
/// are the theme's own, and its tables are wrapped to the same width.
pub(in crate::tui) fn rows(
    source: &str,
    width: usize,
    surface: Color,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    let options = Options::new(PaletteSheet { palette }).table_width(table_width(width));
    let parsed = from_str_with_options(source, &options);
    let mut rows = Vec::new();
    for line in &parsed.lines {
        // The parser carries a block's own style - a heading's ink and weight -
        // on the line and its inline styles on the spans, so the two are
        // merged before the ink is named: a heading's text would otherwise read
        // as body text.
        let segments: Vec<(String, RowStyleId)> = line
            .spans
            .iter()
            .map(|span| {
                let style = line.style.patch(span.style);
                let named = RowStyleId::of_parsed(style.fg, style.add_modifier, palette);
                (span.content.to_string(), named)
            })
            .collect();
        line_rows(&segments, width.max(1), surface, &mut rows);
    }
    rows
}

/// Returns one width as the parser's table budget.
///
/// A budget the parser cannot satisfy still renders: it falls back to the
/// narrowest grid it can draw rather than truncating a cell.
fn table_width(width: usize) -> u16 {
    u16::try_from(width.max(1)).unwrap_or(u16::MAX)
}

/// Appends the display rows of one parsed markdown line.
///
/// An empty line stays one blank display row, so the pane's row window keeps
/// matching the parser's own line inventory.
fn line_rows(
    segments: &[(String, RowStyleId)],
    width: usize,
    surface: Color,
    rows: &mut Vec<LaidOutRow>,
) {
    if segments.iter().all(|(text, _)| text.is_empty()) {
        rows.push(LaidOutRow::blank(surface));
        return;
    }
    if is_grid_line(segments) {
        rows.push(grid_row(segments, surface));
        return;
    }
    rows.extend(wrapped_rows(segments, width, surface));
}

/// Returns whether one line is a box-drawing grid line the pane never wraps.
fn is_grid_line(segments: &[(String, RowStyleId)]) -> bool {
    segments
        .iter()
        .flat_map(|(text, _)| text.chars())
        .next()
        .is_some_and(|character| GRID_OPENERS.contains(&character))
}

/// Returns one atomic grid row: the line laid out whole, never wrapped.
fn grid_row(segments: &[(String, RowStyleId)], surface: Color) -> LaidOutRow {
    let mut row = RowBuilder::new();
    for (text, style) in segments {
        row.push(text, *style);
    }
    row.finish(surface)
}

/// Returns the display rows of one wrapped line, with the continuation indent
/// on every row after the first.
fn wrapped_rows(
    segments: &[(String, RowStyleId)],
    width: usize,
    surface: Color,
) -> Vec<LaidOutRow> {
    let borrowed: Vec<(&str, RowStyleId)> = segments
        .iter()
        .map(|(text, style)| (text.as_str(), *style))
        .collect();
    wrap_segments(&borrowed, width, surface)
}

#[cfg(test)]
mod tests {

    use ratatui::style::Modifier;

    use super::{GRID_OPENERS, rows};
    use crate::tui::layout::{LaidOutRow, RowStyleId};
    use crate::tui::palette::Palette;

    /// The light palette every fixture renders with.
    const LIGHT: &Palette = &crate::tui::palette::LIGHT;

    /// Returns one row's text and run styles, for row-for-row comparison.
    fn roles(rows: &[LaidOutRow]) -> Vec<(String, Vec<RowStyleId>)> {
        rows.iter()
            .map(|row| (row.text.clone(), row.styles()))
            .collect()
    }

    /// Returns the plain text of every row, joined by `\n`.
    fn text(rows: &[LaidOutRow]) -> String {
        rows.iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_long_paragraph_wraps_inside_the_pane_width() {
        let source = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
        let wrapped = rows(source, 20, LIGHT.panel, LIGHT);
        assert!(
            wrapped.len() > 1,
            "a paragraph wider than the pane becomes several rows"
        );
        for row in &wrapped {
            assert!(
                row.width() <= 20,
                "every wrapped row fits the pane: {} columns",
                row.width()
            );
        }
    }

    #[test]
    fn a_table_grid_stays_atomic_instead_of_wrapping() {
        let source = "| a | longer heading |\n|---|---|\n| b | c |";
        let laid_out = rows(source, 4, LIGHT.panel, LIGHT);
        assert!(
            laid_out.iter().any(|row| row.width() > 4),
            "a grid line is laid out whole and clipped, never wrapped"
        );
    }

    #[test]
    fn a_short_answer_stays_one_row() {
        assert_eq!(rows("just one line", 40, LIGHT.panel, LIGHT).len(), 1);
    }

    #[test]
    fn the_character_that_breaks_a_row_opens_the_next_row_unconditionally() {
        // Width three leaves one column beside the continuation indent, so
        // every row after the first holds one character - except the row the
        // break itself opens, which carries the character that broke the row
        // before it.
        let laid_out = rows("Heading", 3, LIGHT.panel, LIGHT);
        assert_eq!(text(&laid_out), "Hea\n  d\n  i\n  n\n  g");
    }

    #[test]
    fn a_wrapped_paragraph_indents_every_row_after_the_first() {
        let laid_out = rows(
            "alpha beta gamma delta epsilon zeta",
            14,
            LIGHT.panel,
            LIGHT,
        );
        assert!(laid_out.len() > 1);
        assert!(
            !laid_out[0].text.starts_with("  "),
            "the first row carries no indent: {:?}",
            laid_out[0].text
        );
        for row in &laid_out[1..] {
            assert!(
                row.text.starts_with("  "),
                "a continuation row is indented: {:?}",
                row.text
            );
            assert!(
                row.width() <= 14,
                "an indented continuation row still fits: {} columns",
                row.width()
            );
        }
    }

    #[test]
    fn the_wrap_keeps_every_character_of_the_source_and_drops_only_break_spaces() {
        let source = "alpha beta gamma delta epsilon zeta eta theta";
        let laid_out = rows(source, 12, LIGHT.panel, LIGHT);
        let wrapped: String = laid_out
            .iter()
            .map(|row| row.text.replace(' ', ""))
            .collect::<Vec<_>>()
            .join("");
        let source: String = source.replace(' ', "");
        assert_eq!(
            wrapped, source,
            "a wrap only ever drops the space it breaks at"
        );
    }

    #[test]
    fn a_heading_drops_its_marker_and_reads_as_a_heading() {
        let laid_out = rows("# A heading", 40, LIGHT.panel, LIGHT);
        assert_eq!(text(&laid_out), "A heading");
        assert_eq!(
            roles(&laid_out),
            vec![(
                "A heading".to_owned(),
                vec![RowStyleId::Heading(Modifier::BOLD)],
            )]
        );
    }

    #[test]
    fn every_heading_level_drops_its_marker_and_keeps_its_text() {
        for level in 1..=6 {
            let source = format!("{} Heading {level}", "#".repeat(level));
            let laid_out = rows(&source, 40, LIGHT.panel, LIGHT);
            assert_eq!(
                text(&laid_out),
                format!("Heading {level}"),
                "the level {level} marker is never painted"
            );
            assert_eq!(
                roles(&laid_out),
                vec![(
                    format!("Heading {level}"),
                    vec![RowStyleId::Heading(Modifier::BOLD)],
                )]
            );
        }
    }

    #[test]
    fn a_hash_the_parser_read_as_text_stays_on_screen() {
        let source = "#Heading without the space";
        let laid_out = rows(source, 40, LIGHT.panel, LIGHT);
        assert_eq!(
            text(&laid_out),
            "#Heading without the space",
            "a hash CommonMark reads as text is content and stays visible"
        );
    }

    #[test]
    fn a_bold_segment_keeps_the_parsers_bold_modifier() {
        let laid_out = rows("plain **strong** plain", 40, LIGHT.panel, LIGHT);
        assert_eq!(text(&laid_out), "plain strong plain");
        assert_eq!(
            roles(&laid_out),
            vec![(
                "plain strong plain".to_owned(),
                vec![
                    RowStyleId::Body(Modifier::empty()),
                    RowStyleId::Body(Modifier::BOLD),
                    RowStyleId::Body(Modifier::empty()),
                ],
            )]
        );
    }

    #[test]
    fn a_list_item_keeps_its_bullet_and_inline_code_keeps_the_code_ink() {
        let laid_out = rows("- one with `code`", 40, LIGHT.panel, LIGHT);
        assert!(
            text(&laid_out).contains("one with code"),
            "the item's text is painted: {:?}",
            text(&laid_out)
        );
        assert!(
            laid_out
                .iter()
                .flat_map(LaidOutRow::styles)
                .any(|style| style == RowStyleId::Code(Modifier::empty())),
            "an inline code span keeps the code ink"
        );
    }

    #[test]
    fn a_code_fence_keeps_its_text_in_the_code_ink() {
        let laid_out = rows("```\nlet x = 1;\n```", 20, LIGHT.panel, LIGHT);
        assert!(
            laid_out.iter().any(|row| row.text.contains("let x = 1;")),
            "the fence's code line keeps its text"
        );
        assert!(
            laid_out
                .iter()
                .flat_map(LaidOutRow::styles)
                .any(|style| style == RowStyleId::Code(Modifier::empty())),
            "the fenced code paints in the code ink"
        );
    }

    #[test]
    fn a_table_keeps_its_aligned_grid_and_its_header_ink() {
        let source = "| a | longer heading |\n|---|---|\n| b | c |";
        let laid_out = rows(source, 60, LIGHT.panel, LIGHT);
        assert!(
            laid_out.len() >= 4,
            "the grid keeps its border, header, separator, and body rows"
        );
        assert!(
            laid_out.iter().all(|row| GRID_OPENERS
                .iter()
                .any(|opener| row.text.starts_with(*opener))),
            "every table row is a box-drawing grid line"
        );
        assert!(
            laid_out[1].text.contains("longer heading"),
            "the header keeps its cells aligned: {:?}",
            laid_out[1].text
        );
        assert!(
            laid_out[1]
                .styles()
                .contains(&RowStyleId::Heading(Modifier::BOLD)),
            "a table header cell keeps the heading ink"
        );
        assert!(
            laid_out
                .iter()
                .flat_map(LaidOutRow::styles)
                .any(|style| style == RowStyleId::Divider(Modifier::empty())),
            "the grid paints in the divider ink"
        );
    }

    #[test]
    fn a_table_is_wrapped_to_the_pane_it_is_laid_out_in() {
        let source = "| heading | another heading |\n|---|---|\n| a long body cell | c |";
        let narrow = rows(source, 24, LIGHT.panel, LIGHT);
        assert!(
            narrow.iter().all(|row| row.width() <= 24),
            "the parser wraps a table to the pane's width"
        );
    }

    #[test]
    fn a_rule_stays_one_blank_row() {
        let laid_out = rows("before\n\n---\n\nafter", 40, LIGHT.panel, LIGHT);
        assert!(
            laid_out.iter().any(LaidOutRow::is_blank),
            "a horizontal rule paints one blank row"
        );
        assert!(
            text(&laid_out).starts_with("before"),
            "the rule keeps its place between the paragraphs"
        );
        assert!(
            text(&laid_out).ends_with("after"),
            "the text after the rule keeps its own row"
        );
    }
}
