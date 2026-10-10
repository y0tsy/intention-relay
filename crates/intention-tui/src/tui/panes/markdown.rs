//! Markdown content as neutral transcript display rows.
//!
//! Both committed assistant answers and committed user cards lay their text out
//! here: the source goes through revue's own `Markdown` parser (the `markdown`
//! feature), so code fences, bold, lists, and tables render in either place.
//! Every line the parser produces becomes one or more [`LaidOutRow`] values:
//!
//! - a heading keeps the parser's `#` marker and takes [`Palette::markdown_heading`],
//! - `**strong**` keeps the parser's bold modifier,
//! - a table keeps the parser's box-drawing grid, laid out as an aligned block,
//! - inline code and links keep [`Palette::markdown_code`] and
//!   [`Palette::markdown_link`].
//!
//! The widget's FIGlet headings stay off on purpose: big art would multiply a
//! heading's display rows and make the transcript's row budget lie. The `#`
//! marker plus the heading ink reads as a heading in one row.
//!
//! The widget itself draws fixed lines and clips anything wider than its area,
//! so this module re-lays the parsed lines out at the caller's width instead: a
//! long paragraph wraps with the pane's continuation indent, while a
//! box-drawing grid line (a table or a bordered code block) stays atomic and is
//! clipped exactly as the widget would clip it. Wrapping the grid would shred
//! the alignment the table exists to show.
//!
//! The wrap walks `&str` slices of the parsed segments, measures them with
//! `char_width` without allocating, breaks at the last space by a byte scan,
//! and assembles every row with `push_str` of those slices. A row is returned
//! as [`LaidOutRow`], so the user card can frame and clip it without losing the
//! parser's per-segment inks and modifiers, and the caller names the surface
//! the rows sit on, because a card refuses to paint the panel's wash inside its
//! own frame.
//!
//! The palette is the one colour system, so the widget's nominal syntax
//! highlighter is off and its neutral grid tone maps onto the pane's divider
//! ink; the colours the parser carries are the ones this module configured,
//! plus the widget's own callout banner.
// @todo(revue): the `Markdown` widget draws fixed lines and clips anything
// wider than its area, so this module re-wraps every parsed line at the
// caller's width and sniffs box-drawing grid lines by their first glyph; a
// width-aware widget that reports a parsed line kind would remove both.

use revue::style::Color;
use revue::text::char_width;
use revue::widget::Markdown;
use revue::widget::markdown::Line;

use crate::tui::layout::{CONTINUATION_INDENT, LaidOutRow, RowBuilder, RowStyleId};
use crate::tui::palette::Palette;

/// The box-drawing glyphs that open an atomic grid or rule line.
const GRID_OPENERS: [char; 4] = ['┌', '├', '└', '│'];

/// Returns the display rows of one markdown block at `width`, painted on
/// `surface`.
///
/// The rows come from the parsed widget's own line inventory (
/// [`Markdown::line_count`] lines, each laid out at `width`), so the transcript
/// window counts the rows the block actually paints. The parser is configured
/// from the resolved `palette`, so the inks it carries are the theme's own.
pub(in crate::tui) fn rows(
    source: &str,
    width: usize,
    surface: Color,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    let markdown = Markdown::new(source)
        .heading_fg(palette.markdown_heading)
        .code_fg(palette.markdown_code)
        .link_fg(palette.markdown_link)
        .syntax_highlight(false);
    let mut rows = Vec::new();
    for line in &markdown.lines {
        line_rows(line, width.max(1), surface, &mut rows, palette);
    }
    rows
}

/// Appends the display rows of one parsed markdown line.
///
/// An empty line stays one blank display row, so the pane's row window keeps
/// matching the widget's own line inventory.
fn line_rows(
    line: &Line,
    width: usize,
    surface: Color,
    rows: &mut Vec<LaidOutRow>,
    palette: &'static Palette,
) {
    if is_empty_line(line) {
        rows.push(LaidOutRow::blank(surface));
        return;
    }
    if is_grid_line(line) {
        rows.push(grid_row(line, surface, palette));
        return;
    }
    rows.extend(wrapped_rows(line, width, surface, palette));
}

/// Returns whether one parsed line paints no character.
fn is_empty_line(line: &Line) -> bool {
    line.segments.iter().all(|segment| segment.text.is_empty())
}

/// Returns whether one line is a box-drawing grid line the pane never wraps.
fn is_grid_line(line: &Line) -> bool {
    line.segments
        .iter()
        .flat_map(|segment| segment.text.chars())
        .next()
        .is_some_and(|character| GRID_OPENERS.contains(&character))
}

/// Returns one atomic grid row: the line laid out whole, never wrapped.
fn grid_row(line: &Line, surface: Color, palette: &'static Palette) -> LaidOutRow {
    let mut row = RowBuilder::new();
    for segment in &line.segments {
        row.push(
            &segment.text,
            RowStyleId::of_parsed(segment.fg, segment.modifier, palette),
        );
    }
    row.finish(surface)
}

/// Returns the display rows of one wrapped line, with the continuation indent
/// on every row after the first.
fn wrapped_rows(
    line: &Line,
    width: usize,
    surface: Color,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    let mut wrapped: Vec<RowBuilder> = Vec::new();
    let mut row = RowBuilder::new();
    for segment in &line.segments {
        let style = RowStyleId::of_parsed(segment.fg, segment.modifier, palette);
        let mut rest = segment.text.as_str();
        while !rest.is_empty() {
            if row.is_empty() {
                // A space never starts a row; runs of spaces collapse at a break.
                rest = rest.trim_start_matches(' ');
                if rest.is_empty() {
                    break;
                }
            }
            let limit = if wrapped.is_empty() {
                width
            } else {
                width.saturating_sub(CONTINUATION_INDENT.len()).max(1)
            };
            let taken = fitting_prefix(&row, rest, limit);
            let (accepted, remainder) = rest.split_at(taken);
            row.push(accepted, style);
            rest = remainder;
            let Some(breaking) = rest.chars().next() else {
                break;
            };
            // The next character is the one that breaks the row: the row splits
            // at its last space, the part after that space opens the next row,
            // and the breaking character joins that row unconditionally. A row
            // therefore overflows only by the character that broke it, and a
            // word wider than the pane keeps a row of its own.
            let tail = row.split_at_last_space();
            let head = std::mem::replace(&mut row, tail);
            wrapped.push(head);
            row.push(&rest[..breaking.len_utf8()], style);
            rest = &rest[breaking.len_utf8()..];
        }
    }
    if !row.is_empty() {
        wrapped.push(row);
    }
    wrapped
        .into_iter()
        .enumerate()
        .map(|(index, mut row)| {
            if index > 0 {
                row.prepend_indent();
            }
            row.finish(surface)
        })
        .collect()
}

/// Returns the byte length of the longest prefix of `rest` the row can take.
///
/// The first character of an empty row always fits, so a word wider than the
/// pane keeps its own row instead of wrapping forever; a space fits wherever it
/// follows a character, so a break only ever drops the space it breaks at; and
/// every other character must fit the `limit` the row has left.
fn fitting_prefix(row: &RowBuilder, rest: &str, limit: usize) -> usize {
    let mut width = row.width();
    let mut first = row.is_empty();
    let mut taken = 0;
    for (offset, character) in rest.char_indices() {
        let character_width = usize::from(char_width(character));
        if !first && character != ' ' && width + character_width > limit {
            break;
        }
        width += character_width;
        taken = offset + character.len_utf8();
        first = false;
    }
    taken
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit tests assert typed fixture rows for diagnostics."
    )]

    use revue::render::Modifier;

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
        // before it. The deleted glyph pipeline did exactly this, so the span
        // walk does too.
        let laid_out = rows("# Heading", 3, LIGHT.panel, LIGHT);
        assert_eq!(text(&laid_out), "#\n  He\n  a\n  d\n  i\n  n\n  g");
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
    fn a_heading_keeps_its_marker_and_the_heading_ink() {
        let laid_out = rows("# A heading", 40, LIGHT.panel, LIGHT);
        assert_eq!(text(&laid_out), "# A heading");
        assert_eq!(
            roles(&laid_out),
            vec![(
                "# A heading".to_owned(),
                vec![
                    RowStyleId::Divider(Modifier::empty()),
                    RowStyleId::Heading(Modifier::BOLD),
                ],
            )]
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
        assert_eq!(text(&laid_out), "• one with code");
        assert_eq!(
            roles(&laid_out),
            vec![(
                "• one with code".to_owned(),
                vec![
                    RowStyleId::Body(Modifier::empty()),
                    RowStyleId::Code(Modifier::empty()),
                ],
            )]
        );
    }

    #[test]
    fn a_code_fence_stays_a_grid_of_atomic_rows_in_the_code_ink() {
        let laid_out = rows("```\nlet x = 1;\n```", 8, LIGHT.panel, LIGHT);
        assert!(
            laid_out.iter().all(|row| row.width() > 8),
            "the code frame stays as wide as its content and is never wrapped"
        );
        assert!(
            laid_out.iter().any(|row| row.text.contains("let x = 1;")),
            "the fence's code line keeps its text"
        );
        assert!(
            laid_out
                .iter()
                .flat_map(LaidOutRow::styles)
                .any(|style| style == RowStyleId::Divider(Modifier::empty())),
            "the frame and padding paint in the divider ink"
        );
    }

    #[test]
    fn a_table_keeps_its_aligned_grid_and_its_header_ink() {
        let source = "| a | longer heading |\n|---|---|\n| b | c |";
        let laid_out = rows(source, 60, LIGHT.panel, LIGHT);
        assert_eq!(
            laid_out.len(),
            5,
            "top border, header, separator, body row, bottom border"
        );
        assert!(
            laid_out.iter().all(|row| GRID_OPENERS
                .iter()
                .any(|opener| row.text.starts_with(*opener))),
            "every table row is a box-drawing grid line"
        );
        assert_eq!(
            laid_out[1].text, "│ a │ longer heading │",
            "the header keeps its cells aligned"
        );
        assert!(
            laid_out[1]
                .styles()
                .contains(&RowStyleId::Heading(Modifier::BOLD)),
            "a table header cell keeps the heading ink"
        );
        assert_eq!(
            laid_out[3].text,
            format!("│ b │ c{}│", " ".repeat("longer heading".len())),
            "the body row keeps every cell padded to its column"
        );
        assert_eq!(
            laid_out[3].styles(),
            vec![
                RowStyleId::Divider(Modifier::empty()),
                RowStyleId::Body(Modifier::empty()),
                RowStyleId::Divider(Modifier::empty()),
                RowStyleId::Body(Modifier::empty()),
                RowStyleId::Divider(Modifier::empty()),
            ],
            "the grid keeps the divider ink and a plain cell the body ink"
        );
    }

    #[test]
    fn a_rule_stays_one_blank_row() {
        let laid_out = rows("before\n\n---\n\nafter", 40, LIGHT.panel, LIGHT);
        assert!(
            laid_out.iter().any(LaidOutRow::is_blank),
            "a horizontal rule paints one blank row"
        );
        assert_eq!(
            text(&laid_out),
            "before\n\nafter",
            "the parser's own line inventory is kept"
        );
    }
}
