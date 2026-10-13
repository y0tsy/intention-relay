//! The tool block dispatch: one renderer per typed tool kind.
//!
//! A durable tool exchange is two committed rows sharing a `tool_call_id`: the
//! `tool_call` row's typed projection carries the call identity, the tool
//! identity, and the decoded typed arguments, and the `tool_result` row's
//! projection carries the same identity, the lifecycle status, the typed
//! payload, and the approved metadata entries. The transcript pairs the two by
//! that id - never by adjacency alone - and draws one block at the call's
//! position; the result row itself renders nothing while its call is
//! committed, so an exchange is shown exactly once. A call whose result row is
//! not committed draws its call alone with a muted note; a result whose call is
//! not committed draws what it carries and says so, never inventing arguments.
//!
//! Every fact the block shows comes from the row's typed projection: the core's
//! declared [`ToolId`] selects the presentation arm, the typed arguments fill
//! the badge and the preview, and the typed payload, status, and metadata fill
//! the result. Nothing here reads a JSON document, re-parses a result's text,
//! or keeps a second copy of the declared tool set: a wire name outside the
//! declared set stays an honest [`ToolIdentityDto::Undeclared`] and the row
//! says so instead of defaulting to an empty identity.
//!
//! Every block is framed through the palette: [`Palette::tool_surface`] is its
//! wash, [`Palette::tool_border`] draws the top border around the badge, the
//! left and right rails, and the bottom border, [`Palette::tool_label`] writes
//! the badge, and [`Palette::tool_preview`] writes the result body - a muted
//! warm ink, readably apart from an answer's body ink, so a tool result never
//! reads as assistant prose.
//!
//! Every list is cut to [`TOOL_PREVIEW_ROWS`] display rows and closed by an
//! explicit `… N more rows` marker, so no block swallows the transcript window.
//!
//! An `execute` result is never printed, in any form, now or later: it is the
//! command's raw output, and showing it would dump the command's bytes into the
//! transcript. That guard is the first check in [`block`], before the per-type
//! dispatch, so no renderer can ever draw it; the typed process status is what
//! the block shows instead.

use intention_proto::{
    GrepMatch, GrepScope, MessageKindDto, MessageProjectionDto, ToolId, ToolInput,
    ToolProcessStatus, ToolResult, ToolResultMetadataEntryDto, ToolResultRowDto,
    ToolResultStatusDto, ToolRowDto,
};

use crate::tui::layout::{
    LaidOutRow, MARKER_GAP, RowBuilder, RowStyleId, box_bottom, box_content, box_top,
    display_width, marker_text,
};
use crate::tui::palette::Palette;
use crate::tui::panes::transcript::{marked_row, plain_rows};

/// The display rows one tool block previews before it bounds its content.
const TOOL_PREVIEW_ROWS: usize = 12;

/// The glyph that opens a tool block's badge.
///
/// U+25B8 BLACK RIGHT-POINTING SMALL TRIANGLE: one column wide under
/// `char_width`, and it has no emoji presentation, so every terminal draws it
/// in [`Palette::tool_label`] instead of a colour. The transcript's whole
/// marker family stays monochrome text glyphs (`❯`, `∴`, `▸`, and the answer's
/// `∷`).
const TOOL_GLYPH: &str = "▸";

/// The rail a structured tool-result row opens with.
///
/// U+2023 TRIANGULAR BULLET: monochrome, one column wide under `char_width`,
/// free of emoji presentation, and spent nowhere else in the interface.
const TOOL_LIST_GLYPH: &str = "‣";

/// The separator between the badge and the facts beside it, and between a hit's
/// location and its fragment.
const TOOL_SEPARATOR: &str = " · ";

/// The columns one tool block's frame takes: the two borders and one padding
/// column beside each.
const TOOL_CHROME_COLUMNS: usize = 4;

/// The columns one tool block's top border spends around its badge: `╭─`, the
/// space before the badge, the space after it, and `╮`.
const TOOL_BADGE_CHROME_COLUMNS: usize = 5;

/// The note a tool row without its typed projection shows.
///
/// The row contract requires typed evidence for every tool row, so this note is
/// the honest degradation of a row that reached a renderer without one.
const NOTE_MALFORMED: &str = "the row carries no typed tool projection";

/// The note a row whose identity is outside the declared tool set shows.
const NOTE_UNDECLARED: &str =
    "the row names a tool outside the declared set, so it has no typed kind";

/// The note a committed call whose arguments document did not decode shows.
const NOTE_UNDECODED_ARGUMENTS: &str =
    "the committed arguments document did not decode as this tool's typed input";

/// The muted note a call without a committed result row shows.
const NOTE_NO_RESULT: &str = "no tool_result row is committed";

/// The muted note a result without a committed call row shows.
const NOTE_NO_CALL: &str =
    "the call row for this result is not committed, so its arguments are unknown";

/// The note a failed result shows.
const NOTE_FAILED: &str = "the tool reported a failure";

/// The note a partial result shows.
const NOTE_PARTIAL: &str = "the tool stopped before a final result; the output above is partial";

/// The note a typed truncated result shows.
const NOTE_TRUNCATED: &str = "the tool marked its result truncated";

/// The note an empty path list shows.
const NOTE_EMPTY_PATHS: &str = "the pattern matched no workspace path";

/// The note an empty match list shows.
const NOTE_EMPTY_MATCHES: &str = "the pattern matched nothing";

/// The note an empty text result shows.
const NOTE_EMPTY_TEXT: &str = "the tool answered with no content";

/// The note an execute result without a terminal classification shows.
const NOTE_NO_PROCESS_STATUS: &str = "the call observed no terminal program status";

/// The durable tool exchanges of one transcript, paired by `tool_call_id`.
pub(in crate::tui) struct Pairing<'a> {
    /// The committed transcript the pairs are read from.
    transcript: &'a [MessageProjectionDto],
}

impl<'a> Pairing<'a> {
    /// Pairs one transcript's tool rows by call identity.
    pub(in crate::tui) const fn of(transcript: &'a [MessageProjectionDto]) -> Self {
        Self { transcript }
    }

    /// Returns the result row committed for the call at `index`, if any.
    ///
    /// The pair is the first `tool_result` row after the call that shares its
    /// `tool_call_id`: pairing is by identity, never by adjacency alone.
    pub(in crate::tui) fn result(&self, index: usize) -> Option<&'a MessageProjectionDto> {
        let call_id = self.transcript.get(index)?.tool_call_id()?;
        self.transcript[index + 1..].iter().find(|row| {
            row.kind() == MessageKindDto::ToolResult && row.tool_call_id() == Some(call_id)
        })
    }

    /// Returns the call row committed before the result at `index`, if any.
    pub(in crate::tui) fn call(&self, index: usize) -> Option<&'a MessageProjectionDto> {
        let call_id = self.transcript.get(index)?.tool_call_id()?;
        self.transcript[..index].iter().rev().find(|row| {
            row.kind() == MessageKindDto::ToolCall && row.tool_call_id() == Some(call_id)
        })
    }

    /// Returns whether the result at `index` answers a committed call.
    pub(in crate::tui) fn answers_call(&self, index: usize) -> bool {
        self.call(index).is_some()
    }
}

/// Returns the one block a committed tool row renders.
///
/// `None` means the row is the result its committed call already draws, or an
/// `execute` result, which is never rendered in any form.
pub(in crate::tui) fn block(
    row: &MessageProjectionDto,
    index: usize,
    pairing: &Pairing<'_>,
    width: usize,
    palette: &'static Palette,
) -> Option<Vec<LaidOutRow>> {
    let inner = width.saturating_sub(TOOL_CHROME_COLUMNS);
    let Some(tool) = row.tool() else {
        // A tool row carries typed evidence by contract; a row that reached a
        // renderer without one still renders its honest note, never a panic,
        // never an invented identity, and never a badge around an empty id.
        return Some(framed(
            None,
            note_rows(NOTE_MALFORMED, inner, palette),
            width,
            palette,
        ));
    };
    // An `execute` result is never rendered, in any form: it is the command's
    // raw output, and this check runs before every per-type renderer so none of
    // them - present or future - can draw it.
    if row.kind() == MessageKindDto::ToolResult
        && tool.tool_id().declared() == Some(ToolId::Execute)
    {
        return None;
    }
    // A result whose call is committed is drawn by that call's block: the
    // exchange appears once, at the row that opened it.
    if row.kind() == MessageKindDto::ToolResult && pairing.answers_call(index) {
        return None;
    }
    let facts = argument_facts(tool);
    let badge = badge_text(tool.tool_id().wire_name(), &facts, width);
    let mut content = Vec::new();
    match row.kind() {
        MessageKindDto::ToolResult => {
            content.extend(note_rows(NOTE_NO_CALL, inner, palette));
            if let Some(result) = tool.result() {
                content.extend(payload_rows(result, inner, palette));
            }
        }
        _ => {
            content.extend(argument_rows(tool, inner, palette));
            match pairing
                .result(index)
                .and_then(MessageProjectionDto::tool)
                .and_then(ToolRowDto::result)
            {
                Some(result) => content.extend(payload_rows(result, inner, palette)),
                None => content.extend(note_rows(NOTE_NO_RESULT, inner, palette)),
            }
        }
    }
    Some(framed(Some(&badge), content, width, palette))
}

/// Returns the framed block one tool exchange renders.
///
/// The badge rides the block's top border, every content row carries the tool
/// frame's left and right rails, and one bottom border closes the block, so a
/// tool exchange is one boxed block a reader can tell from an answer at a
/// glance. A block whose row carries no typed evidence has no badge at all.
fn framed(
    badge: Option<&str>,
    content: Vec<LaidOutRow>,
    width: usize,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    let inner = width.saturating_sub(TOOL_CHROME_COLUMNS);
    let mut rows = vec![box_top(
        badge.map(|badge| (badge, RowStyleId::ToolLabel)),
        width,
        RowStyleId::ToolBorder,
        palette.tool_surface,
    )];
    rows.extend(
        content
            .iter()
            .map(|row| box_content(row, inner, RowStyleId::ToolBorder)),
    );
    rows.push(box_bottom(
        width,
        RowStyleId::ToolBorder,
        palette.tool_surface,
    ));
    rows
}

/// Returns the badge that fits one tool block's top border.
///
/// The badge is the glyph, the shared marker gap, the wire tool id, and the
/// typed facts that fit; the top border's own cells and the spaces around the
/// badge come out of the block's width, so the badge never ends in half a path.
fn badge_text(tool_id: &str, facts: &[String], width: usize) -> String {
    let budget = width.saturating_sub(TOOL_BADGE_CHROME_COLUMNS);
    let mut badge = marker_text(TOOL_GLYPH, tool_id);
    let separator = display_width(TOOL_SEPARATOR);
    for fact in facts {
        if display_width(&badge) + separator + display_width(fact) > budget {
            break;
        }
        badge.push_str(TOOL_SEPARATOR);
        badge.push_str(fact);
    }
    badge
}

/// Returns the typed facts one tool row's arguments carry for its badge.
///
/// The facts are read from the core's typed arguments alone: a call whose
/// document never decoded carries none, and an undeclared identity carries
/// none either.
fn argument_facts(tool: &ToolRowDto) -> Vec<String> {
    let Some(arguments) = tool.call().and_then(|call| call.arguments()) else {
        return Vec::new();
    };
    match arguments {
        ToolInput::Read(input) => vec![input.path.as_str().to_owned()],
        ToolInput::Glob(input) => vec![format!("\"{}\"", input.pattern.as_str())],
        ToolInput::Grep(input) => {
            let mut facts = vec![format!("\"{}\"", input.pattern.as_str())];
            match &input.scope {
                Some(GrepScope::File { path }) => {
                    facts.push("file".to_owned());
                    facts.push(path.as_str().to_owned());
                }
                Some(GrepScope::Directory { path }) => {
                    facts.push("directory".to_owned());
                    facts.push(path.as_str().to_owned());
                }
                Some(GrepScope::Workspace) => facts.push("workspace".to_owned()),
                None => {}
            }
            if let Some(path) = input.path.as_ref() {
                facts.push(path.as_str().to_owned());
            }
            facts
        }
        ToolInput::Write(input) => vec![input.path.as_str().to_owned()],
        ToolInput::Edit(input) => vec![input.path.as_str().to_owned()],
        ToolInput::Execute(input) => {
            let mut facts = vec![input.command.as_str().to_owned()];
            if let Some(cwd) = input.cwd.as_ref() {
                facts.push(cwd.as_str().to_owned());
            }
            facts
        }
    }
}

/// Returns the rows a call's own typed state leaves: the note an undeclared
/// identity or an undecoded arguments document shows, or nothing when the call
/// carries its decoded arguments.
fn argument_rows(tool: &ToolRowDto, width: usize, palette: &'static Palette) -> Vec<LaidOutRow> {
    let Some(call) = tool.call() else {
        return Vec::new();
    };
    if call.tool_id().declared().is_none() {
        return note_rows(NOTE_UNDECLARED, width, palette);
    }
    if call.arguments().is_none() {
        return note_rows(NOTE_UNDECODED_ARGUMENTS, width, palette);
    }
    Vec::new()
}

/// Returns the rows of one committed result's typed evidence: its lifecycle
/// status, its typed payload, and its approved metadata entries.
///
/// No row ever prints a tool result's text: the typed payload is the rendering
/// source, and an `execute` payload's text is never rendered at all.
fn payload_rows(
    result: &ToolResultRowDto,
    width: usize,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    let mut rows = Vec::new();
    match result.status() {
        ToolResultStatusDto::Completed => {}
        ToolResultStatusDto::Failed => rows.extend(note_rows(NOTE_FAILED, width, palette)),
        ToolResultStatusDto::Partial => rows.extend(note_rows(NOTE_PARTIAL, width, palette)),
    }
    if let Some(payload) = result.result() {
        rows.extend(payload_content(payload, width, palette));
    }
    rows.extend(
        result
            .metadata()
            .iter()
            .flat_map(|entry| metadata_rows(entry, width, palette)),
    );
    rows
}

/// Returns the rows of one typed result payload.
fn payload_content(
    payload: &ToolResult,
    width: usize,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    match payload {
        ToolResult::Read(value) => {
            let mut rows = if value.text.as_str().trim().is_empty() {
                note_rows(NOTE_EMPTY_TEXT, width, palette)
            } else {
                bounded(
                    content_rows(value.text.as_str(), width, RowStyleId::ToolPreview, palette),
                    width,
                    palette,
                )
            };
            if value.truncated {
                rows.extend(note_rows(NOTE_TRUNCATED, width, palette));
            }
            rows
        }
        ToolResult::Glob(value) => {
            let mut rows = if value.paths.is_empty() {
                note_rows(NOTE_EMPTY_PATHS, width, palette)
            } else {
                bounded(
                    value
                        .paths
                        .iter()
                        .flat_map(|path| {
                            list_rows(path.as_str(), RowStyleId::ToolPreview, width, palette)
                        })
                        .collect(),
                    width,
                    palette,
                )
            };
            if value.truncated {
                rows.extend(note_rows(NOTE_TRUNCATED, width, palette));
            }
            rows
        }
        ToolResult::Grep(value) => {
            let mut rows = if value.matches.is_empty() {
                note_rows(NOTE_EMPTY_MATCHES, width, palette)
            } else {
                bounded(
                    value
                        .matches
                        .iter()
                        .flat_map(|matched| hit_rows(matched, width, palette))
                        .collect(),
                    width,
                    palette,
                )
            };
            if value.truncated {
                rows.extend(note_rows(NOTE_TRUNCATED, width, palette));
            }
            rows
        }
        ToolResult::Write(value) => content_rows(
            &format!("wrote {} bytes", value.bytes),
            width,
            RowStyleId::ToolPreview,
            palette,
        ),
        ToolResult::Edit(value) => content_rows(
            &format!("edited {} bytes", value.bytes),
            width,
            RowStyleId::ToolPreview,
            palette,
        ),
        // An execute result's text is never rendered, in any form: only the
        // typed terminal classification crosses into the transcript.
        ToolResult::Execute(value) => {
            let line = match value.process {
                Some(ToolProcessStatus::Success) => "the program completed successfully".to_owned(),
                Some(ToolProcessStatus::NonZero { code }) => {
                    format!("the program exited with code {code}")
                }
                Some(ToolProcessStatus::Signal { signal }) => {
                    format!("the program was terminated by signal {signal}")
                }
                None => NOTE_NO_PROCESS_STATUS.to_owned(),
            };
            content_rows(&line, width, RowStyleId::ToolPreview, palette)
        }
    }
}

/// Returns the rows of one approved metadata entry, `key: value`.
fn metadata_rows(
    entry: &ToolResultMetadataEntryDto,
    width: usize,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    note_rows(
        &format!("{}: {}", entry.key(), entry.value()),
        width,
        palette,
    )
}

/// Returns the wrapped rows of one content text in `ink` on the tool wash.
fn content_rows(
    text: &str,
    width: usize,
    ink: RowStyleId,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    let mut rows = Vec::new();
    for line in text.lines() {
        rows.extend(plain_rows(line, width, ink, palette));
    }
    rows.into_iter()
        .map(|row| row.on_surface(palette.tool_surface))
        .collect()
}

/// Returns the rows of one structured list entry.
///
/// The rail glyph rides the entry's first display row, the shared marker gap
/// separates it from the text, and the text keeps the same column on every
/// wrapped row through the same hanging indent an answer uses.
fn list_rows(
    text: &str,
    ink: RowStyleId,
    inner: usize,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    let indent = display_width(TOOL_LIST_GLYPH) + MARKER_GAP;
    let width = inner.saturating_sub(indent);
    plain_rows(text, width, ink, palette)
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            marked_row(
                &row,
                index == 0,
                TOOL_LIST_GLYPH,
                indent,
                width,
                RowStyleId::ToolBorder,
                palette.tool_surface,
            )
        })
        .collect()
}

/// Returns the rows of one typed `grep` match.
///
/// The location is written in the muted ink and the matched fragment in the
/// result ink, so a hit reads as one structured row rather than as a line of
/// prose: the location is the typed match's own path, line, and column.
fn hit_rows(matched: &GrepMatch, inner: usize, palette: &'static Palette) -> Vec<LaidOutRow> {
    let location = format!(
        "{}:{}:{}",
        matched.path.as_str(),
        matched.line,
        matched.column
    );
    let indent = display_width(TOOL_LIST_GLYPH) + MARKER_GAP;
    let prefix = display_width(&location) + display_width(TOOL_SEPARATOR);
    let width = inner.saturating_sub(indent + prefix);
    let mut rows = Vec::new();
    for (index, row) in plain_rows(
        matched.fragment.as_str(),
        width,
        RowStyleId::ToolPreview,
        palette,
    )
    .into_iter()
    .enumerate()
    {
        let mut builder = RowBuilder::new();
        if index == 0 {
            builder.push(TOOL_LIST_GLYPH, RowStyleId::ToolBorder);
            builder.push(&" ".repeat(MARKER_GAP), RowStyleId::ToolBorder);
            builder.push(&location, RowStyleId::Muted);
            builder.push(TOOL_SEPARATOR, RowStyleId::ToolBorder);
        } else {
            builder.push(&" ".repeat(indent + prefix), RowStyleId::ToolBorder);
        }
        builder.push_clipped(&row, width);
        rows.push(builder.finish(palette.tool_surface));
    }
    rows
}

/// Returns the bounded rows of one structured result.
///
/// A list longer than [`TOOL_PREVIEW_ROWS`] display rows keeps its first rows
/// and is closed by a muted `… N more rows` marker naming what the bound left
/// out.
fn bounded(mut rows: Vec<LaidOutRow>, inner: usize, palette: &'static Palette) -> Vec<LaidOutRow> {
    let hidden = rows.len().saturating_sub(TOOL_PREVIEW_ROWS);
    if hidden > 0 {
        rows.truncate(TOOL_PREVIEW_ROWS);
        rows.extend(content_rows(
            &format!("… {hidden} more rows"),
            inner,
            RowStyleId::ToolNote,
            palette,
        ));
    }
    rows
}

/// Returns the muted rows of one note about a typed fact the row shows as a
/// note rather than as a structured entry.
fn note_rows(note: &str, width: usize, palette: &'static Palette) -> Vec<LaidOutRow> {
    content_rows(note, width, RowStyleId::ToolNote, palette)
}

#[cfg(test)]
mod tests {

    use std::sync::atomic::{AtomicI64, Ordering};

    use intention_proto::{
        BoundedText, ExecuteResult, GrepMatch, GrepResult, MessageId, MessageKindDto,
        MessageProjectionDto, PathsResult, RunId, SessionId, TextResult, ToolCallId,
        ToolCallRowDto, ToolId, ToolIdentityDto, ToolInput, ToolProcessStatus, ToolResult,
        ToolResultRowDto, ToolResultStatusDto, ToolRowDto, WorkspaceRelativePathDto, WriteResult,
    };

    use crate::tui::layout::MARKER_GAP;
    use crate::tui::palette::Palette;

    use super::{
        NOTE_EMPTY_PATHS, NOTE_MALFORMED, NOTE_NO_RESULT, NOTE_TRUNCATED, NOTE_UNDECLARED, Pairing,
        TOOL_GLYPH, block,
    };

    /// The light palette every fixture renders with.
    const LIGHT: &Palette = &crate::tui::palette::LIGHT;

    /// Returns one fresh durable row identity for a committed fixture row.
    fn row_id() -> MessageId {
        static NEXT: AtomicI64 = AtomicI64::new(0);
        MessageId::new(NEXT.fetch_add(1, Ordering::Relaxed) + 1)
            .expect("the fixture row identity is positive")
    }

    /// Returns one bounded fixture text.
    fn bounded(text: &str) -> BoundedText {
        BoundedText::new(text).expect("the fixture text is bounded")
    }

    /// Returns one workspace-relative fixture path.
    fn relative(path: &str) -> WorkspaceRelativePathDto {
        WorkspaceRelativePathDto::parse(path).expect("the fixture path is workspace-relative")
    }

    /// Returns one parsed wire identity for a fixture row.
    fn identity(tool_id: &str) -> ToolIdentityDto {
        ToolIdentityDto::parse(tool_id).expect("the fixture tool identity is well-formed")
    }

    /// Returns one committed row of the fixture session carrying `tool`.
    fn tool_row(kind: MessageKindDto, tool: ToolRowDto) -> MessageProjectionDto {
        MessageProjectionDto::new(
            row_id(),
            SessionId::new(),
            Some(RunId::new()),
            kind,
            "committed content",
            None,
            Some(tool),
        )
        .expect("the fixture tool row is coherent")
    }

    /// Returns one typed `read` payload carrying `text`.
    fn read_payload(text: &str, truncated: bool) -> ToolResult {
        ToolResult::Read(TextResult {
            text: bounded(text),
            truncated,
        })
    }

    /// Returns one tool exchange's transcript and the call's index.
    fn exchange(
        tool_id: &str,
        arguments: &str,
        result: Option<(ToolResultStatusDto, Option<ToolResult>)>,
    ) -> (Vec<MessageProjectionDto>, usize) {
        let call_id = ToolCallId::new();
        let call = ToolRowDto::Call(
            ToolCallRowDto::new(
                call_id,
                identity(tool_id),
                ToolInput::from_arguments_json(tool_id, arguments).ok(),
            )
            .expect("the fixture call row is coherent"),
        );
        let mut rows = vec![tool_row(MessageKindDto::ToolCall, call)];
        if let Some((status, payload)) = result {
            let evidence = ToolRowDto::Result(
                ToolResultRowDto::new(call_id, identity(tool_id), status, payload, Vec::new())
                    .expect("the fixture result row is coherent"),
            );
            rows.push(tool_row(MessageKindDto::ToolResult, evidence));
        }
        (rows, 0)
    }

    /// Returns the rendered text of the block at `index`, or the empty string
    /// when the row renders nothing.
    fn rendered(rows: &[MessageProjectionDto], index: usize, width: usize) -> String {
        let pairing = Pairing::of(rows);
        block(&rows[index], index, &pairing, width, LIGHT)
            .unwrap_or_default()
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_badge_keeps_the_shared_marker_gap_after_its_glyph() {
        let (rows, index) = exchange("read", r#"{"path":"src/lib.rs"}"#, None);
        let rendered = rendered(&rows, index, 60);
        assert!(
            rendered.starts_with(&format!(
                "╭─ {TOOL_GLYPH}{}read · src/lib.rs",
                " ".repeat(MARKER_GAP)
            )),
            "{rendered}"
        );
        assert_eq!(MARKER_GAP, 1, "the one shared gap is one blank column");
    }

    #[test]
    fn a_read_only_block_renders_its_arguments_and_bounds_its_preview() {
        let content = (0..20)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (rows, index) = exchange(
            "read",
            r#"{"path":"src/lib.rs"}"#,
            Some((
                ToolResultStatusDto::Completed,
                Some(read_payload(&content, false)),
            )),
        );
        let text = rendered(&rows, index, 40);
        assert!(text.contains("╭─ ▸ read · src/lib.rs"), "{text}");
        assert!(text.contains("│ line 0"), "{text}");
        assert!(text.contains("… 8 more rows"), "{text}");
        assert!(text.contains('╰'), "the block closes its own frame: {text}");
        assert!(
            !text.contains("line 19"),
            "the preview is bounded to 12 display rows: {text}"
        );
    }

    #[test]
    fn a_glob_block_lists_the_matched_paths_and_a_grep_block_lists_its_hits() {
        let (glob_rows, glob_index) = exchange(
            "glob",
            r#"{"pattern":"**/*.rs"}"#,
            Some((
                ToolResultStatusDto::Completed,
                Some(ToolResult::Glob(PathsResult {
                    paths: vec![relative("src/lib.rs"), relative("src/main.rs")],
                    truncated: false,
                })),
            )),
        );
        let glob = rendered(&glob_rows, glob_index, 40);
        assert!(glob.contains("╭─ ▸ glob · \"**/*.rs\""), "{glob}");
        assert!(glob.contains("│ ‣ src/lib.rs"), "{glob}");
        assert!(glob.contains("│ ‣ src/main.rs"), "{glob}");

        let (grep_rows, grep_index) = exchange(
            "grep",
            r#"{"pattern":"needle","scope":{"kind":"directory","path":"src"}}"#,
            Some((
                ToolResultStatusDto::Completed,
                Some(ToolResult::Grep(GrepResult {
                    matches: vec![GrepMatch {
                        path: relative("src/lib.rs"),
                        line: 3,
                        column: 5,
                        fragment: bounded("needle"),
                    }],
                    truncated: false,
                })),
            )),
        );
        let grep = rendered(&grep_rows, grep_index, 60);
        assert!(
            grep.contains("╭─ ▸ grep · \"needle\" · directory · src"),
            "{grep}"
        );
        assert!(
            grep.contains("│ ‣ src/lib.rs:3:5 · needle"),
            "a hit keeps its location and its fragment apart: {grep}"
        );
    }

    #[test]
    fn every_typed_tool_kind_has_its_own_presentation_arm() {
        let arguments = |tool_id: ToolId| match tool_id {
            ToolId::Read => r#"{"path":"src/lib.rs"}"#,
            ToolId::Glob => r#"{"pattern":"**/*.rs"}"#,
            ToolId::Grep => r#"{"pattern":"needle"}"#,
            ToolId::Write => r#"{"path":"src/lib.rs","content":"new"}"#,
            ToolId::Edit => r#"{"path":"src/lib.rs","old":"a","new":"b"}"#,
            ToolId::Execute => r#"{"command":"cargo test"}"#,
        };
        let payload = |tool_id: ToolId| match tool_id {
            ToolId::Read => read_payload("contents", false),
            ToolId::Glob => ToolResult::Glob(PathsResult {
                paths: vec![relative("src/lib.rs")],
                truncated: false,
            }),
            ToolId::Grep => ToolResult::Grep(GrepResult {
                matches: vec![GrepMatch {
                    path: relative("src/lib.rs"),
                    line: 3,
                    column: 5,
                    fragment: bounded("needle"),
                }],
                truncated: false,
            }),
            ToolId::Write => ToolResult::Write(WriteResult { bytes: 3 }),
            ToolId::Edit => ToolResult::Edit(WriteResult { bytes: 1 }),
            ToolId::Execute => ToolResult::Execute(ExecuteResult {
                text: TextResult {
                    text: bounded("raw output"),
                    truncated: false,
                },
                process: Some(ToolProcessStatus::Success),
            }),
        };
        let expected: [(ToolId, &str, &str); 6] = [
            (ToolId::Read, "▸ read · src/lib.rs", "│ contents"),
            (ToolId::Glob, "▸ glob · \"**/*.rs\"", "│ ‣ src/lib.rs"),
            (
                ToolId::Grep,
                "▸ grep · \"needle\"",
                "│ ‣ src/lib.rs:3:5 · needle",
            ),
            (ToolId::Write, "▸ write · src/lib.rs", "│ wrote 3 bytes"),
            (ToolId::Edit, "▸ edit · src/lib.rs", "│ edited 1 byte"),
            (
                ToolId::Execute,
                "▸ execute · cargo test",
                "│ the program completed successfully",
            ),
        ];
        for (tool_id, badge, outcome) in expected {
            let (rows, index) = exchange(
                tool_id.as_str(),
                arguments(tool_id),
                Some((ToolResultStatusDto::Completed, Some(payload(tool_id)))),
            );
            let text = rendered(&rows, index, 80);
            assert!(
                text.starts_with(&format!("╭─ {badge}")),
                "{tool_id}: {text}"
            );
            assert!(
                text.contains(outcome),
                "{tool_id} owns its own outcome: {text}"
            );
            assert!(text.contains('╰'), "{tool_id}: {text}");
        }

        // A wire name outside the declared set is a typed undeclared identity,
        // never a seventh dispatch arm and never an empty-id badge.
        let (rows, index) = exchange("mystery", r#"{"shape":1}"#, None);
        let text = rendered(&rows, index, 120);
        assert!(text.contains("▸ mystery"), "{text}");
        assert!(text.contains(NOTE_UNDECLARED), "{text}");
    }

    #[test]
    fn a_result_sentinel_is_never_rendered_as_an_entry() {
        // An empty typed path list is a note; `[no paths]` is not an entry.
        let (rows, index) = exchange(
            "glob",
            r#"{"pattern":"**/*.rs"}"#,
            Some((
                ToolResultStatusDto::Completed,
                Some(ToolResult::Glob(PathsResult {
                    paths: Vec::new(),
                    truncated: false,
                })),
            )),
        );
        let empty = rendered(&rows, index, 60);
        assert!(empty.contains(NOTE_EMPTY_PATHS), "{empty}");
        assert!(!empty.contains("[no paths]"), "{empty}");
        assert!(
            !empty.contains("‣"),
            "an empty list draws no entry: {empty}"
        );

        // A truncated typed read states its flag in a note; the flat `[truncated]`
        // line the result text may carry is never an entry.
        let (rows, index) = exchange(
            "read",
            r#"{"path":"src/lib.rs"}"#,
            Some((
                ToolResultStatusDto::Completed,
                Some(read_payload("contents", true)),
            )),
        );
        let truncated = rendered(&rows, index, 60);
        assert!(truncated.contains(NOTE_TRUNCATED), "{truncated}");
        assert!(!truncated.contains("[truncated]"), "{truncated}");
        assert!(!truncated.contains("‣"), "{truncated}");
    }

    #[test]
    fn a_row_without_a_typed_kind_renders_the_malformed_note() {
        // The renderer serves any committed row the dispatcher sends; a row that
        // carries no typed tool evidence still renders its honest note, with no
        // badge invented around an absent identity.
        let row = MessageProjectionDto::new(
            row_id(),
            SessionId::new(),
            None,
            MessageKindDto::Notice,
            "a notice",
            None,
            None,
        )
        .expect("the fixture notice row is coherent");
        let pairing = Pairing::of(std::slice::from_ref(&row));
        let rendered = block(&row, 0, &pairing, 60, LIGHT)
            .expect("a row without a typed kind still renders its note")
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains(NOTE_MALFORMED), "{rendered}");
        assert!(
            !rendered.contains(TOOL_GLYPH),
            "no badge is invented for a row without a typed kind: {rendered}"
        );
    }

    #[test]
    fn an_execute_result_is_never_rendered_in_any_form() {
        let (rows, _) = exchange(
            "execute",
            r#"{"command":"cargo test"}"#,
            Some((
                ToolResultStatusDto::Completed,
                Some(ToolResult::Execute(ExecuteResult {
                    text: TextResult {
                        text: bounded("raw output"),
                        truncated: false,
                    },
                    process: Some(ToolProcessStatus::Success),
                })),
            )),
        );
        let pairing = Pairing::of(&rows);
        assert!(
            block(&rows[1], 1, &pairing, 60, LIGHT).is_none(),
            "an execute result row renders nothing"
        );
        let call = rendered(&rows, 0, 60);
        assert!(!call.contains("raw output"), "{call}");
    }

    #[test]
    fn a_result_whose_call_is_committed_is_drawn_by_that_call_alone() {
        let (rows, _) = exchange(
            "read",
            r#"{"path":"src/lib.rs"}"#,
            Some((
                ToolResultStatusDto::Completed,
                Some(read_payload("the content", false)),
            )),
        );
        let pairing = Pairing::of(&rows);
        assert!(
            block(&rows[1], 1, &pairing, 60, LIGHT).is_none(),
            "the exchange is drawn once, at the call"
        );
        assert!(rendered(&rows, 0, 60).contains("the content"));
    }

    #[test]
    fn a_call_without_a_result_row_renders_its_call_alone() {
        let (rows, index) = exchange("read", r#"{"path":"src/lib.rs"}"#, None);
        let text = rendered(&rows, index, 60);
        assert!(text.contains("▸ read · src/lib.rs"), "{text}");
        assert!(text.contains(NOTE_NO_RESULT), "{text}");
    }

    #[test]
    fn an_orphan_result_never_invents_the_arguments_it_lacks() {
        let call_id = ToolCallId::new();
        let rows = vec![tool_row(
            MessageKindDto::ToolResult,
            ToolRowDto::Result(
                ToolResultRowDto::new(
                    call_id,
                    identity("glob"),
                    ToolResultStatusDto::Completed,
                    Some(ToolResult::Glob(PathsResult {
                        paths: vec![relative("src/lib.rs")],
                        truncated: false,
                    })),
                    Vec::new(),
                )
                .expect("the fixture result row is coherent"),
            ),
        )];
        let text = rendered(&rows, 0, 60);
        assert!(text.contains("╭─ ▸ glob"), "{text}");
        assert!(
            text.contains("the call row for this result is not committed"),
            "{text}"
        );
        assert!(text.contains("│ ‣ src/lib.rs"), "{text}");
    }
}
