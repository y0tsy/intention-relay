//! The tool block dispatch: one renderer per wire tool type.
//!
//! A durable tool exchange is two committed rows sharing a `tool_call_id`: the
//! `tool_call` row's `text` is the arguments document the model asked with, and
//! the `tool_result` row's `text` is the tool's answer. The transcript pairs
//! the two by that id - never by adjacency alone - and draws one block at the
//! call's position; the result row itself renders nothing while its call is
//! committed, so an exchange is shown exactly once. A call whose result row is
//! not committed draws its call alone with a muted note; a result whose call is
//! not committed draws what it carries and says so, never inventing arguments.
//!
//! Every block is framed through the palette: [`Palette::tool_surface`] is its
//! wash, [`Palette::tool_border`] draws the top border around the badge, the
//! left and right rails, and the bottom border, [`Palette::tool_label`] writes
//! the badge, and [`Palette::tool_preview`] writes the result body - a muted
//! warm ink, readably apart from an answer's body ink, so a tool result never
//! reads as assistant prose.
//!
//! The read-only tools - `read`, `glob`, and `grep` - are parsed in full: the
//! facts their arguments carry and their results rendered as structured rows
//! (a path list, a list of located hits, or a file preview). Every list is cut
//! to [`TOOL_PREVIEW_ROWS`] display rows and closed by an explicit
//! `… N more rows` marker, so no block swallows the transcript window.
//!
//! `write`, `edit`, and `execute` are not parsed into a block shape: their
//! typed outcome is not on the durable rows, so each renders its badge plus a
//! visible `@todo(core)` plate naming what the rows do not carry. An `execute`
//! result is never printed, in any form, now or later: it is the command's raw
//! output, and showing it would dump the command's bytes into the transcript.
//! That guard is the first check in [`block`], before the per-type dispatch, so
//! no renderer can ever draw it.
//!
//! An id outside the six wire names is unknown: the wire names tools by string
//! and no closed set is declared, so the block is a badge, the arguments' raw
//! text, and the plate that says exactly that. A document that is not one JSON
//! object, or a field of the wrong type, is never guessed at: the row keeps its
//! raw text on the muted ink and a plate names what could not be read.

use intention_proto::{MessageKindDto, MessageProjectionDto};
use serde_json::{Map, Value};

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

/// The prefix every plate line carries.
const TOOL_PLATE_PREFIX: &str = "@todo(core): ";

/// The plate a document that is not one JSON object shows.
const PLATE_MALFORMED: &str =
    "the arguments document is not one JSON object; the raw text is shown unparsed";

/// The plate an unknown tool id shows.
const PLATE_UNKNOWN: &str = "the wire names tools by string and the closed set is not declared";

/// The plate a `write` call shows.
const PLATE_WRITE: &str = "the wire carries no typed write outcome: the written size is not on \
                           the durable rows, and the result row's text is not printed";

/// The plate an `edit` call shows.
const PLATE_EDIT: &str = "the wire carries no typed edit outcome: the applied patch is not a \
                          typed fact, and the result row's text is not printed";

/// The plate an `execute` call shows.
const PLATE_EXECUTE: &str =
    "the wire carries no typed exit status, and the command's output is never printed";

/// The muted note a call without a committed result row shows.
const NOTE_NO_RESULT: &str = "no tool_result row is committed";

/// The muted note a result without a committed call row shows.
const NOTE_NO_CALL: &str =
    "the call row for this result is not committed, so its arguments are unknown";

/// The one tool type a committed row names by its wire `tool_id`.
// @todo(core): declare the closed set of tool ids in the core and map the wire
// `tool_id` to a typed tool kind; the pane must not string-match tool names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolKind {
    /// The `read` tool: a file's requested range.
    Read,
    /// The `glob` tool: a pattern's matched paths.
    Glob,
    /// The `grep` tool: a pattern's matches inside files.
    Grep,
    /// The `write` tool: a whole file's new content.
    Write,
    /// The `edit` tool: one patch applied to a file.
    Edit,
    /// The `execute` tool: a command and its output.
    Execute,
    /// A wire tool id this front end does not know.
    Other,
}

impl ToolKind {
    /// Returns the tool type one wire `tool_id` names.
    fn of(tool_id: &str) -> Self {
        match tool_id {
            "read" => Self::Read,
            "glob" => Self::Glob,
            "grep" => Self::Grep,
            "write" => Self::Write,
            "edit" => Self::Edit,
            "execute" => Self::Execute,
            _ => Self::Other,
        }
    }
}

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
    // @todo(hack): the wire's missing `tool_id` silently becomes an empty
    // string here and in `Exchange::of`; the absent case should be handled
    // explicitly instead of defaulting to an id no tool carries.
    let kind = ToolKind::of(row.tool_id().unwrap_or_default());
    // An `execute` result is never rendered, in any form: it is the command's
    // raw output, and this check runs before every per-type renderer so none of
    // them - present or future - can draw it.
    if row.kind() == MessageKindDto::ToolResult && kind == ToolKind::Execute {
        return None;
    }
    // A result whose call is committed is drawn by that call's block: the
    // exchange appears once, at the row that opened it.
    if row.kind() == MessageKindDto::ToolResult && pairing.answers_call(index) {
        return None;
    }
    let exchange = Exchange::of(row, index, pairing);
    let (facts, missing) = arguments_facts(exchange.tool_id, &exchange.arguments);
    let badge = badge_text(exchange.tool_id, &facts, width);
    let inner = width.saturating_sub(TOOL_CHROME_COLUMNS);
    let content = match kind {
        ToolKind::Read | ToolKind::Glob | ToolKind::Grep => {
            read_only_content(&exchange, kind, missing, inner, palette)
        }
        ToolKind::Write => plate_content(&exchange, missing, PLATE_WRITE, inner, palette),
        ToolKind::Edit => plate_content(&exchange, missing, PLATE_EDIT, inner, palette),
        ToolKind::Execute => plate_content(&exchange, missing, PLATE_EXECUTE, inner, palette),
        ToolKind::Other => other_content(&exchange, missing, inner, palette),
    };
    Some(framed(&badge, content, width, palette))
}

/// Returns the framed block one tool exchange renders.
///
/// The badge rides the block's top border, every content row carries the tool
/// frame's left and right rails, and one bottom border closes the block, so a
/// tool exchange is one boxed block a reader can tell from an answer at a
/// glance.
fn framed(
    badge: &str,
    content: Vec<LaidOutRow>,
    width: usize,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    let inner = width.saturating_sub(TOOL_CHROME_COLUMNS);
    let mut rows = vec![box_top(
        Some((badge, RowStyleId::ToolLabel)),
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
/// facts that fit; the top border's own cells and the spaces around the badge
/// come out of the block's width, so the badge never ends in half a path.
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

/// One tool exchange as the block that draws it reads it.
struct Exchange<'a> {
    /// The wire tool id the row names.
    tool_id: &'a str,
    /// The call's arguments document; absent when no call row is committed.
    arguments: Arguments,
    /// The result row's content, when a result row is committed.
    result: Option<&'a str>,
    /// Whether the call row itself is committed.
    call_committed: bool,
}

impl<'a> Exchange<'a> {
    /// Reads one tool row and its committed counterpart.
    fn of(row: &'a MessageProjectionDto, index: usize, pairing: &'a Pairing<'a>) -> Self {
        let tool_id = row.tool_id().unwrap_or_default();
        if row.kind() == MessageKindDto::ToolCall {
            Self {
                tool_id,
                arguments: Arguments::of(row.text()),
                result: pairing.result(index).map(MessageProjectionDto::text),
                call_committed: true,
            }
        } else {
            // Only an orphan result reaches here: a result whose call is
            // committed is refused before any block is built.
            Self {
                tool_id,
                arguments: Arguments::Missing,
                result: Some(row.text()),
                call_committed: false,
            }
        }
    }
}

/// One call's arguments document, read without trusting its shape.
// @todo(core): decode the tool-arguments document into typed core argument
// structs; the pane must not run `serde_json` or probe JSON field types.
enum Arguments {
    /// A JSON object document, field by field.
    Object(Map<String, Value>),
    /// A document that is not one JSON object: its raw text is kept.
    Raw(String),
    /// No document at all.
    Missing,
}

impl Arguments {
    /// Decodes one arguments document defensively.
    ///
    /// Nothing here can fail: a document that is not one JSON object keeps its
    /// raw text instead of dropping the row, and a field of the wrong type is
    /// simply not returned as the fact it was meant to be.
    fn of(text: &str) -> Self {
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(fields)) => Self::Object(fields),
            Ok(_) => Self::Raw(text.to_owned()),
            Err(_) if text.trim().is_empty() => Self::Missing,
            Err(_) => Self::Raw(text.to_owned()),
        }
    }

    /// Returns one field, only when the document carries a string under it.
    fn text(&self, field: &str) -> Option<&str> {
        match self {
            Self::Object(fields) => fields.get(field).and_then(Value::as_str),
            Self::Raw(_) | Self::Missing => None,
        }
    }

    /// Returns one field, only when the document carries a whole number under it.
    fn number(&self, field: &str) -> Option<u64> {
        match self {
            Self::Object(fields) => fields.get(field).and_then(Value::as_u64),
            Self::Raw(_) | Self::Missing => None,
        }
    }

    /// Returns one field, only when the document carries an object under it.
    fn object(&self, field: &str) -> Option<&Map<String, Value>> {
        match self {
            Self::Object(fields) => fields.get(field).and_then(Value::as_object),
            Self::Raw(_) | Self::Missing => None,
        }
    }

    /// Returns one field, only when every element under it is a string.
    fn texts(&self, field: &str) -> Option<Vec<&str>> {
        match self {
            Self::Object(fields) => fields
                .get(field)?
                .as_array()?
                .iter()
                .map(Value::as_str)
                .collect(),
            Self::Raw(_) | Self::Missing => None,
        }
    }

    /// Returns the raw text of a document that is not one JSON object.
    fn raw(&self) -> Option<&str> {
        match self {
            Self::Raw(text) => Some(text),
            Self::Object(_) | Self::Missing => None,
        }
    }
}

/// Returns the content rows of one read-only exchange.
///
/// The content is the arguments' raw text when they cannot be read, the plate
/// each unreadable or missing field leaves, and the structured rows of the
/// committed result - or the muted note that no result row is committed.
fn read_only_content(
    exchange: &Exchange<'_>,
    kind: ToolKind,
    missing: Option<String>,
    inner: usize,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    let mut content = Vec::new();
    content.extend(raw_rows(&exchange.arguments, inner, palette));
    if exchange.call_committed {
        content.extend(
            missing
                .into_iter()
                .flat_map(|fact| plate_rows(&fact, inner, palette)),
        );
    } else {
        content.extend(note_rows(NOTE_NO_CALL, inner, palette));
    }
    match exchange.result {
        Some(result) => content.extend(result_rows(kind, result, inner, palette)),
        None => content.extend(note_rows(NOTE_NO_RESULT, inner, palette)),
    }
    content
}

/// Returns the structured rows of one read-only result.
///
/// The content is parsed by the tool that produced it: a `glob` result is a
/// path list, a `grep` result is a list of located hits, and any other
/// read-only result is a file preview. A line the parser cannot read is kept
/// whole in the result ink rather than dropped, and the whole list is bounded
/// with a `… N more rows` marker.
// @todo(core): the durable rows carry no typed result: parsing a `glob` path
// list, a `grep` hit's `path:line:column` location, or a read preview out of
// the result text belongs to the core, as does the metadata a preview cannot
// invent (a read result's `truncated` flag, for one), so a preview shows the
// content's own `[truncated]` line where the tool wrote one and never invents
// the flag.
fn result_rows(
    kind: ToolKind,
    content: &str,
    inner: usize,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    let rows = match kind {
        ToolKind::Glob => content
            .lines()
            .flat_map(|path| list_rows(path, RowStyleId::ToolPreview, inner, palette))
            .collect(),
        ToolKind::Grep => content
            .lines()
            .flat_map(|hit| hit_rows(hit, inner, palette))
            .collect(),
        _ => content_rows(content, inner, RowStyleId::ToolPreview, palette),
    };
    bounded(rows, inner, palette)
}

/// Returns the content rows of one `write`, `edit`, or `execute` exchange.
///
/// None of the three prints its result row's text: for `write` and `edit` it is
/// a written byte count rather than a typed outcome, and an `execute` result is
/// refused in [`block`] before this content is built.
fn plate_content(
    exchange: &Exchange<'_>,
    missing: Option<String>,
    plate: &str,
    inner: usize,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    let mut content = Vec::new();
    content.extend(raw_rows(&exchange.arguments, inner, palette));
    if exchange.call_committed {
        content.extend(
            missing
                .into_iter()
                .flat_map(|fact| plate_rows(&fact, inner, palette)),
        );
        if exchange.result.is_none() {
            content.extend(note_rows(NOTE_NO_RESULT, inner, palette));
        }
    } else {
        content.extend(note_rows(NOTE_NO_CALL, inner, palette));
    }
    content.extend(plate_rows(plate, inner, palette));
    content
}

/// Returns the content rows of an unknown wire tool id.
///
/// The wire names tools by string and no closed set is declared, so the content
/// is the arguments' raw text when the row carries one and the plate that says
/// exactly that - never a guess at a shape.
fn other_content(
    exchange: &Exchange<'_>,
    missing: Option<String>,
    inner: usize,
    palette: &'static Palette,
) -> Vec<LaidOutRow> {
    let mut content = Vec::new();
    content.extend(raw_rows(&exchange.arguments, inner, palette));
    if exchange.call_committed {
        content.extend(
            missing
                .into_iter()
                .flat_map(|fact| plate_rows(&fact, inner, palette)),
        );
    } else {
        content.extend(note_rows(NOTE_NO_CALL, inner, palette));
    }
    content.extend(plate_rows(PLATE_UNKNOWN, inner, palette));
    content
}

/// Returns the facts one call's arguments carry, and the plate its missing or
/// unreadable document leaves.
///
/// A document that is not one JSON object is never parsed into facts: the
/// caller keeps its raw text muted and shows the same plate every such
/// document shows.
// @todo(core): derive the argument facts - `path`, `offset`, `limit`,
// `pattern`, `scope`, `program`, `args`, and the joined command line - from
// typed core argument structs; the pane must not read the document's keys.
fn arguments_facts(tool_id: &str, arguments: &Arguments) -> (Vec<String>, Option<String>) {
    if arguments.raw().is_some() {
        return (Vec::new(), Some(PLATE_MALFORMED.to_owned()));
    }
    match ToolKind::of(tool_id) {
        ToolKind::Read => read_facts(arguments),
        ToolKind::Glob => glob_facts(arguments),
        ToolKind::Grep => grep_facts(arguments),
        ToolKind::Write | ToolKind::Edit => path_facts(tool_id, arguments),
        ToolKind::Execute => execute_facts(arguments),
        ToolKind::Other => (Vec::new(), None),
    }
}

/// Returns the `read` facts: the path, and the offset and limit when present.
fn read_facts(arguments: &Arguments) -> (Vec<String>, Option<String>) {
    let Some(path) = arguments.text("path") else {
        return (
            Vec::new(),
            Some("the read arguments carry no path".to_owned()),
        );
    };
    let mut facts = vec![path.to_owned()];
    if let Some(offset) = arguments.number("offset") {
        facts.push(format!("offset {offset}"));
    }
    if let Some(limit) = arguments.number("limit") {
        facts.push(format!("limit {limit}"));
    }
    (facts, None)
}

/// Returns the `glob` facts: the quoted pattern.
fn glob_facts(arguments: &Arguments) -> (Vec<String>, Option<String>) {
    arguments.text("pattern").map_or_else(
        || {
            (
                Vec::new(),
                Some("the glob arguments carry no pattern".to_owned()),
            )
        },
        |pattern| (vec![format!("\"{pattern}\"")], None),
    )
}

/// Returns the `grep` facts: the quoted pattern and its search scope.
fn grep_facts(arguments: &Arguments) -> (Vec<String>, Option<String>) {
    let Some(pattern) = arguments.text("pattern") else {
        return (
            Vec::new(),
            Some("the grep arguments carry no pattern".to_owned()),
        );
    };
    let mut facts = vec![format!("\"{pattern}\"")];
    if let Some(scope) = arguments.object("scope") {
        if let Some(kind) = scope.get("kind").and_then(Value::as_str) {
            facts.push(kind.to_owned());
        }
        if let Some(path) = scope.get("path").and_then(Value::as_str) {
            facts.push(path.to_owned());
        }
    }
    if let Some(path) = arguments.text("path") {
        facts.push(path.to_owned());
    }
    (facts, None)
}

/// Returns the facts a mutating call carries: its target path.
fn path_facts(tool_id: &str, arguments: &Arguments) -> (Vec<String>, Option<String>) {
    arguments.text("path").map_or_else(
        || {
            (
                Vec::new(),
                Some(format!("the {tool_id} arguments carry no path")),
            )
        },
        |path| (vec![path.to_owned()], None),
    )
}

/// Returns the facts an `execute` call carries: its command line.
fn execute_facts(arguments: &Arguments) -> (Vec<String>, Option<String>) {
    let Some(program) = arguments.text("program") else {
        return (
            Vec::new(),
            Some("the execute arguments carry no program".to_owned()),
        );
    };
    let Some(args) = arguments.texts("args") else {
        return (
            Vec::new(),
            Some("the execute arguments carry no args list".to_owned()),
        );
    };
    let mut command = program.to_owned();
    for arg in args {
        command.push(' ');
        command.push_str(arg);
    }
    (vec![command], None)
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

/// Returns the rows of one `grep` hit.
///
/// The canonical hit line is `path:line:column: fragment`: the location is
/// written in the muted ink and the matched fragment in the result ink, so a
/// hit reads as one structured row rather than as a line of prose. A line that
/// does not name a location is kept whole in the result ink, never guessed at.
fn hit_rows(hit: &str, inner: usize, palette: &'static Palette) -> Vec<LaidOutRow> {
    let Some((location, fragment)) = hit.split_once(": ") else {
        return list_rows(hit, RowStyleId::ToolPreview, inner, palette);
    };
    let indent = display_width(TOOL_LIST_GLYPH) + MARKER_GAP;
    let prefix = display_width(location) + display_width(TOOL_SEPARATOR);
    let width = inner.saturating_sub(indent + prefix);
    let mut rows = Vec::new();
    for (index, row) in plain_rows(fragment, width, RowStyleId::ToolPreview, palette)
        .into_iter()
        .enumerate()
    {
        let mut builder = RowBuilder::new();
        if index == 0 {
            builder.push(TOOL_LIST_GLYPH, RowStyleId::ToolBorder);
            builder.push(&" ".repeat(MARKER_GAP), RowStyleId::ToolBorder);
            builder.push(location, RowStyleId::Muted);
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

/// Returns the muted rows of one note about a fact the durable rows do not carry.
fn note_rows(note: &str, width: usize, palette: &'static Palette) -> Vec<LaidOutRow> {
    content_rows(note, width, RowStyleId::ToolNote, palette)
}

/// Returns the muted rows of a raw arguments document, when there is one.
fn raw_rows(arguments: &Arguments, width: usize, palette: &'static Palette) -> Vec<LaidOutRow> {
    arguments.raw().map_or_else(Vec::new, |raw| {
        content_rows(raw, width, RowStyleId::ToolNote, palette)
    })
}

/// Returns the plate rows of one fact the wire cannot fill: `@todo(core): …`
/// on the todo wash and ink.
fn plate_rows(fact: &str, width: usize, palette: &'static Palette) -> Vec<LaidOutRow> {
    plain_rows(
        &format!("{TOOL_PLATE_PREFIX}{fact}"),
        width,
        RowStyleId::Todo,
        palette,
    )
    .into_iter()
    .map(|row| row.on_surface(palette.stub_bg))
    .collect()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Unit tests build typed fixture DTOs directly and assert them for diagnostics."
    )]

    use intention_proto::{MessageKindDto, MessageProjectionDto, RunId, SessionId, ToolCallId};

    use crate::tui::layout::MARKER_GAP;
    use crate::tui::palette::Palette;

    use super::{Pairing, TOOL_GLYPH, ToolKind, block};

    /// The light palette every fixture renders with.
    const LIGHT: &Palette = &crate::tui::palette::LIGHT;

    /// Returns one committed tool row naming `tool_id` with `text`.
    fn tool_row(
        kind: MessageKindDto,
        tool_id: &str,
        text: &str,
        call_id: ToolCallId,
    ) -> MessageProjectionDto {
        MessageProjectionDto::new(
            SessionId::new(),
            Some(RunId::new()),
            kind,
            text,
            None,
            Some(call_id),
            Some(tool_id.to_owned()),
        )
        .expect("the fixture tool row is coherent")
    }

    /// Returns one tool exchange's transcript and the call's index.
    fn exchange(
        tool_id: &str,
        arguments: &str,
        result: Option<&str>,
    ) -> (Vec<MessageProjectionDto>, usize) {
        let call_id = ToolCallId::new();
        let mut rows = vec![tool_row(
            MessageKindDto::ToolCall,
            tool_id,
            arguments,
            call_id,
        )];
        if let Some(content) = result {
            rows.push(tool_row(
                MessageKindDto::ToolResult,
                tool_id,
                content,
                call_id,
            ));
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
    fn every_wire_tool_id_has_its_own_dispatch_arm() {
        for (tool_id, expected) in [
            ("read", ToolKind::Read),
            ("glob", ToolKind::Glob),
            ("grep", ToolKind::Grep),
            ("write", ToolKind::Write),
            ("edit", ToolKind::Edit),
            ("execute", ToolKind::Execute),
            ("mystery", ToolKind::Other),
        ] {
            assert_eq!(ToolKind::of(tool_id), expected, "{tool_id}");
        }
    }

    #[test]
    fn a_read_only_block_renders_its_arguments_and_bounds_its_preview() {
        let content = (0..20)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (rows, index) = exchange(
            "read",
            r#"{"path":"src/lib.rs","limit":10}"#,
            Some(&content),
        );
        let text = rendered(&rows, index, 40);
        assert!(text.contains("╭─ ▸ read · src/lib.rs · limit 10"), "{text}");
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
            Some("src/lib.rs\nsrc/main.rs"),
        );
        let glob = rendered(&glob_rows, glob_index, 40);
        assert!(glob.contains("╭─ ▸ glob · \"**/*.rs\""), "{glob}");
        assert!(glob.contains("│ ‣ src/lib.rs"), "{glob}");
        assert!(glob.contains("│ ‣ src/main.rs"), "{glob}");

        let (grep_rows, grep_index) = exchange(
            "grep",
            r#"{"pattern":"needle","scope":{"kind":"directory","path":"src"}}"#,
            Some("src/lib.rs:3:5: needle"),
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
    fn write_edit_and_execute_render_a_badge_and_plate_without_their_result_text() {
        for (tool_id, arguments) in [
            ("write", r#"{"path":"src/lib.rs","content":"new"}"#),
            ("edit", r#"{"path":"src/lib.rs","old":"a","new":"b"}"#),
            ("execute", r#"{"program":"cargo","args":["test"]}"#),
        ] {
            let (rows, index) = exchange(tool_id, arguments, Some("SECRET RESULT TEXT"));
            let text = rendered(&rows, index, 60);
            assert!(text.contains(&format!("▸ {tool_id}")), "{text}");
            assert!(text.contains("@todo(core): "), "{text}");
            assert!(
                !text.contains("SECRET RESULT TEXT"),
                "{tool_id} never prints its result text: {text}"
            );
        }
    }

    #[test]
    fn an_execute_result_is_never_rendered_in_any_form() {
        let (rows, _) = exchange(
            "execute",
            r#"{"program":"cargo","args":[]}"#,
            Some("raw output"),
        );
        let pairing = Pairing::of(&rows);
        assert!(
            block(&rows[1], 1, &pairing, 60, LIGHT).is_none(),
            "an execute result row renders nothing"
        );
    }

    #[test]
    fn a_result_whose_call_is_committed_is_drawn_by_that_call_alone() {
        let (rows, _) = exchange("read", r#"{"path":"src/lib.rs"}"#, Some("the content"));
        let pairing = Pairing::of(&rows);
        assert!(
            block(&rows[1], 1, &pairing, 60, LIGHT).is_none(),
            "the exchange is drawn once, at the call"
        );
        assert!(rendered(&rows, 0, 60).contains("the content"));
    }

    #[test]
    fn a_malformed_argument_document_still_renders_a_block() {
        for tool_id in [
            "read", "glob", "grep", "write", "edit", "execute", "mystery",
        ] {
            let (rows, index) = exchange(tool_id, "not json at all", Some("content"));
            let text = rendered(&rows, index, 60);
            assert!(
                text.contains("▸") && text.contains("not json at all"),
                "{tool_id} keeps its raw arguments: {text}"
            );
            assert!(
                text.contains("@todo(core): "),
                "{tool_id} shows a plate: {text}"
            );
        }
    }

    #[test]
    fn a_call_without_a_result_row_renders_its_call_alone() {
        let (rows, index) = exchange("read", r#"{"path":"src/lib.rs"}"#, None);
        let text = rendered(&rows, index, 60);
        assert!(text.contains("▸ read · src/lib.rs"), "{text}");
        assert!(text.contains("no tool_result row is committed"), "{text}");
    }

    #[test]
    fn an_orphan_result_never_invents_the_arguments_it_lacks() {
        let rows = vec![tool_row(
            MessageKindDto::ToolResult,
            "glob",
            "src/lib.rs",
            ToolCallId::new(),
        )];
        let text = rendered(&rows, 0, 60);
        assert!(text.contains("╭─ ▸ glob"), "{text}");
        assert!(
            text.contains("the call row for this result is not committed"),
            "{text}"
        );
        assert!(text.contains("│ ‣ src/lib.rs"), "{text}");
    }

    #[test]
    fn an_unknown_tool_id_names_the_closed_set_it_cannot_know() {
        let (rows, index) = exchange("mystery", r#"{"shape":"unknown"}"#, Some("output"));
        let text = rendered(&rows, index, 120);
        assert!(text.contains("▸ mystery"), "{text}");
        assert!(
            text.contains("the wire names tools by string and the closed set is not declared"),
            "{text}"
        );
    }
}
