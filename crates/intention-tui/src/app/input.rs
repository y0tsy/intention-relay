//! The input transitions: the input line, its cursor, its history, the command
//! hint menu, and submission.
//!
//! # Pastes
//!
//! A paste is one edit, never the keystrokes its characters look like: the
//! terminal delivers the text, and a text that carries a line break - or more
//! characters than the inline limit - is kept outside the buffer behind one
//! labelled line, because a line the user cannot read at a glance is not a line
//! they can edit. The label names the paste's ordinal and its own beginning, and
//! the submission path expands it back into the whole text, so the session
//! receives the paste and not its summary.

use super::commands::{self, CommandMenu, MenuRow};
use super::{
    AppState, Effect, InputCursorMove, InputHistoryMove, MenuMove, ThemePreference,
    TranscriptScroll,
};

/// How many transcript display rows one mouse-wheel notch moves.
const TRANSCRIPT_WHEEL_ROWS: u16 = 3;

/// How many transcript display rows one fast drag step moves.
///
/// Five wheel notches: a held pointer that slides past the transcript's edge -
/// and a wheel notch that arrives while the button is held - reaches rows far
/// beyond the window quickly. The step is public because the view computes
/// where the window lands to keep a dragged selection's leading edge on the row
/// the pointer is over.
pub const TRANSCRIPT_DRAG_ROWS: u16 = TRANSCRIPT_WHEEL_ROWS * 5;

/// The characters a paste may carry and still be inserted as ordinary text.
///
/// A longer paste is one labelled line whose text waits outside the buffer. The
/// limit is about what the line can show, never about what a turn may carry: the
/// text itself is unbounded and reaches the session whole. The same limit is what
/// the front end measures a run of keystrokes against, so a terminal without
/// bracketed paste is kept outside the line by the paste's own size.
const PASTE_INLINE_CHARS: usize = 400;

/// The lines a paste may carry and still be inserted as ordinary text.
///
/// Size decides, not timing: a paste of a few lines is a line the user can read
/// in the input, and anything longer is one they cannot, whatever it weighs in
/// characters.
const PASTE_INLINE_LINES: usize = 8;

/// The characters of a paste label's head: the text's own beginning.
const PASTE_HEAD_CHARS: usize = 50;

/// The prefix every paste label carries.
const PASTE_LABEL_PREFIX: &str = "[pasted ";

/// One pasted text the input line stands for with one labelled line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PastedText {
    /// The labelled line that stands for the text inside the buffer.
    label: String,
    /// The pasted text, exactly as the terminal delivered it.
    text: String,
}

impl AppState {
    /// Inserts one typed character at the cursor.
    pub(super) fn apply_input_char(&mut self, character: char) -> Vec<Effect> {
        let offset = self.input_byte_offset(self.cursor);
        self.input.insert(offset, character);
        self.cursor += 1;
        self.refresh_menu();
        Vec::new()
    }

    /// Removes the character before the cursor.
    ///
    /// A paste label is one unit for the delete: removing one of its characters
    /// would leave a label that stands for nothing, so the whole label goes at
    /// once, exactly as it was inserted.
    pub(super) fn apply_input_backspace(&mut self) -> Vec<Effect> {
        if self.cursor == 0 {
            return Vec::new();
        }
        if let Some((start, end)) = self
            .paste_spans()
            .into_iter()
            .find(|(start, end)| *start < self.cursor && self.cursor <= *end)
        {
            let removed = self.input_byte_offset(start)..self.input_byte_offset(end);
            self.input.replace_range(removed, "");
            self.cursor = start;
            self.refresh_menu();
            return Vec::new();
        }
        let removed = self.input_byte_offset(self.cursor - 1)..self.input_byte_offset(self.cursor);
        self.input.replace_range(removed, "");
        self.cursor -= 1;
        self.refresh_menu();
        Vec::new()
    }

    /// Inserts one pasted text at the cursor as one edit.
    ///
    /// The terminal delivers the text and this front end reads no clipboard, so
    /// the whole paste enters at once instead of arriving as the keystrokes its
    /// characters look like. The text's own size decides what the line shows: a
    /// paste of a few lines, or of a few hundred characters, is inserted exactly
    /// as it arrived, and a larger one is kept behind one labelled line that the
    /// submission path expands again. Line endings are the buffer's own - a
    /// Windows paste's carriage returns are not content - and nothing else is
    /// rewritten: the text the session receives is the text that was pasted.
    pub(super) fn apply_input_pasted(&mut self, text: String) -> Vec<Effect> {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if text.is_empty() {
            return Vec::new();
        }
        if paste_is_large(&text) {
            self.insert_paste(text);
        } else {
            self.insert_input_text(&text);
        }
        self.refresh_menu();
        Vec::new()
    }

    /// Inserts `text` at the cursor as one edit.
    fn insert_input_text(&mut self, text: &str) {
        let offset = self.input_byte_offset(self.cursor);
        self.input.insert_str(offset, text);
        self.cursor += text.chars().count();
    }

    /// Keeps `text` outside the buffer and inserts the label standing for it.
    fn insert_paste(&mut self, text: String) {
        let label = paste_label(&text);
        self.insert_input_text(&label);
        self.pasted.push(PastedText { label, text });
    }

    /// Returns the character span of every paste label the buffer carries.
    ///
    /// The span is what makes a label one unit for the delete and the horizontal
    /// caret moves, so a paste is never silently reduced to a label's text. A
    /// label the user edited out of shape is not found here and stays the
    /// ordinary characters it now is.
    fn paste_spans(&self) -> Vec<(usize, usize)> {
        let mut spans = Vec::new();
        for pasted in &self.pasted {
            let mut from = 0;
            while let Some(offset) = self.input[from..].find(&pasted.label) {
                let start = self.input[..from + offset].chars().count();
                spans.push((start, start + pasted.label.chars().count()));
                from += offset + pasted.label.len();
            }
        }
        spans
    }

    /// Returns the edge a paste label moves the caret to, when the caret names
    /// one.
    ///
    /// A label is one unit for the horizontal moves: the caret leaves it on the
    /// edge it was heading for - the label's start going left, its end going
    /// right - so no press steps into the middle of a paste's label.
    fn paste_edge(&self, movement: InputCursorMove) -> Option<usize> {
        let cursor = self.cursor;
        self.paste_spans()
            .into_iter()
            .find_map(|(start, end)| match movement {
                InputCursorMove::Left if start < cursor && cursor <= end => Some(start),
                InputCursorMove::Right if start <= cursor && cursor < end => Some(end),
                _ => None,
            })
    }

    /// Returns `content` with every paste label replaced by the text it stands
    /// for.
    ///
    /// A label carries no identity of its own - it is the paste's own beginning
    /// and its line count, and nothing the user has to read past - so the
    /// occurrences a line spells are matched in the order they appear: the first
    /// occurrence of one label claims the first paste carrying it, the second the
    /// next, and so on. Two pastes that happen to share a beginning and a length
    /// therefore still expand into their own texts, and a label the user edited
    /// out of shape matches nothing and stays the ordinary text it now is.
    fn expand_pastes(&self, content: &str) -> String {
        let mut claimed = vec![false; self.pasted.len()];
        let mut expanded = String::with_capacity(content.len());
        let mut rest = content;
        loop {
            let next = self
                .pasted
                .iter()
                .enumerate()
                .filter(|(index, _)| !claimed[*index])
                .filter_map(|(index, pasted)| rest.find(&pasted.label).map(|at| (at, index)))
                .min_by_key(|(at, _)| *at);
            let Some((at, index)) = next else {
                break;
            };
            expanded.push_str(&rest[..at]);
            expanded.push_str(&self.pasted[index].text);
            claimed[index] = true;
            rest = &rest[at + self.pasted[index].label.len()..];
        }
        expanded.push_str(rest);
        expanded
    }

    /// Moves the cursor one character, one line boundary, or one line.
    ///
    /// The cursor counts characters of the whole buffer, so `Left` and `Right`
    /// step across a line break like any other character, except over a paste
    /// label, which they treat as the one unit it is. `Home` and `End` stay
    /// inside the line the cursor is on: `Home` reaches its first character and
    /// `End` its last, whether the line ends at a break or at the buffer's end.
    /// `Up` and `Down` reach the line above or below and keep the cursor's
    /// column where that line is long enough, ending the line where it is not;
    /// at the buffer's first or last line they move nothing, because the keymap
    /// gives those presses their other meaning instead.
    pub(super) fn apply_input_cursor_move(&mut self, movement: InputCursorMove) {
        let characters: Vec<char> = self.input.chars().collect();
        self.cursor = self.cursor.min(characters.len());
        self.cursor = match movement {
            InputCursorMove::Left => self
                .paste_edge(InputCursorMove::Left)
                .unwrap_or_else(|| self.cursor.saturating_sub(1)),
            InputCursorMove::Right => self
                .paste_edge(InputCursorMove::Right)
                .unwrap_or_else(|| self.cursor.saturating_add(1).min(characters.len())),
            InputCursorMove::Home => line_start(&characters, self.cursor),
            InputCursorMove::End => line_end(&characters, self.cursor),
            InputCursorMove::Up => line_step(&characters, self.cursor, true),
            InputCursorMove::Down => line_step(&characters, self.cursor, false),
        };
        self.refresh_menu();
    }

    /// Recomputes the input's command hint band from the line and the caret.
    ///
    /// The open rule is one function of the two: the line's first character is
    /// `/`, the caret sits in or immediately after a word of that line, and the
    /// word's filter selects at least one row - the registry's commands for the
    /// command word, and a declared argument's values for an argument word.
    /// Anything else - the slash removed, the caret past the word, no match
    /// left - closes the band, and the same refresh reopens it as soon as the
    /// line matches again. A band whose word did not change keeps the row the
    /// user highlighted, so moving the caret inside the word it is already on
    /// never moves the highlight; another character starts the highlight at the
    /// best match.
    pub(super) fn refresh_menu(&mut self) {
        let opened = commands::command_word(&self.input, self.cursor)
            .and_then(|word| CommandMenu::opened(&word));
        self.command_menu = match (self.command_menu.take(), opened) {
            (Some(previous), Some(next)) if previous.ranks_the_same_word(&next) => Some(previous),
            (_, opened) => opened,
        };
    }

    /// Moves the hint menu's highlight one row towards `direction`.
    pub(super) fn move_menu(&mut self, direction: MenuMove) {
        if let Some(menu) = self.command_menu.as_mut() {
            menu.move_highlight(direction);
        }
    }

    /// Commits the band's highlighted row into the input line.
    ///
    /// The word the band was opened for - the command word's leading slash
    /// through its last character, or an argument word's first through its
    /// last - is replaced by the row's completion, which is its label and one
    /// trailing space, so the caret lands ready for the argument after a
    /// command or for the submission after a value. The band then follows the
    /// same open rule: a command that declares an argument reopens it on the
    /// argument word, and a complete command or value closes it.
    pub(super) fn accept_menu(&mut self) {
        let Some(completed) = self
            .command_menu
            .as_ref()
            .and_then(CommandMenu::highlighted)
            .map(MenuRow::completion)
        else {
            return;
        };
        let Some(word) = commands::command_word(&self.input, self.cursor) else {
            self.command_menu = None;
            return;
        };
        let before = self.input[..word.start].chars().count();
        self.input.replace_range(word.start..word.end, &completed);
        self.cursor = before + completed.chars().count();
        self.refresh_menu();
    }

    /// Walks the history of sent and abandoned lines, newest entry first.
    pub(super) fn apply_input_history_move(&mut self, movement: InputHistoryMove) {
        match movement {
            InputHistoryMove::Previous => self.recall_older_line(),
            InputHistoryMove::Next => self.recall_newer_line(),
        }
    }

    /// Replaces the input line with the next older history entry, if one exists.
    ///
    /// The first step saves the line the user was typing as the draft; walking
    /// to the oldest entry stops there instead of leaving the history.
    fn recall_older_line(&mut self) {
        let Some(newest) = self.history.len().checked_sub(1) else {
            return;
        };
        let position = match self.history_position {
            Some(position) => position.saturating_sub(1),
            None => {
                self.history_draft = Some(self.input.clone());
                newest
            }
        };
        self.history_position = Some(position);
        self.set_input(self.history[position].clone());
    }

    /// Steps towards the newest history entry, then back to the preserved draft.
    fn recall_newer_line(&mut self) {
        let Some(position) = self.history_position else {
            return;
        };
        if position + 1 < self.history.len() {
            self.history_position = Some(position + 1);
            self.set_input(self.history[position + 1].clone());
            return;
        }
        // Walking past the newest entry leaves the history for the draft.
        self.history_position = None;
        let draft = self.history_draft.take().unwrap_or_default();
        self.set_input(draft);
    }

    /// Sends the input buffer as a user turn, or runs the slash command it names.
    ///
    /// A backslash immediately before the cursor escapes the Enter press: this
    /// action removes it and inserts a line break in its place, so a multi-line
    /// prompt is typed without being submitted and the backslash itself never
    /// reaches the submitted buffer.
    ///
    /// An open band claims Enter only while its word is incomplete: the press
    /// then completes the word the user is typing, so the caret lands past the
    /// command with one space ready for its arguments. A word that already
    /// spells the command (`/sessions`) or a declared value (`/theme light`)
    /// has nothing left to complete, so Enter runs the line - and the empty
    /// argument word `/theme` opens is the same case from the other side.
    ///
    /// A prompt with no open session starts the session it needs instead of
    /// being rejected: the one create command carries the prompt as the created
    /// session's first turn, so nothing is parked for a later snapshot to send.
    pub(super) fn submit_input(&mut self) -> Vec<Effect> {
        if self.insert_line_break() {
            return Vec::new();
        }
        // The band owns Enter while its word is incomplete: the press
        // completes the word the user is typing instead of running it, so an
        // abbreviated line grows to its command or value first. A word that
        // already spells the command or a declared value has nothing left to
        // complete, so Enter runs the line - one press for `/sessions`, not
        // two - while Tab still completes the row the band highlights.
        if let Some(menu) = self.command_menu.as_ref()
            && !menu.completes_nothing()
        {
            self.accept_menu();
            return Vec::new();
        }
        let content = self.input.trim().to_owned();
        if content.is_empty() {
            self.notice = Some("type a message before sending it".to_owned());
            return Vec::new();
        }
        if let Some(command) = self.input.strip_prefix('/') {
            // One rule decides what a command is, for the menu and for
            // submission alike: the line's first character is the slash, so a
            // line whose first character is anything else - a blank included -
            // is a turn.
            let command = command.trim().to_owned();
            self.clear_input();
            return self.submit_command(&command);
        }
        // The line the interactive path submitted is the input line's own: it
        // is recalled through the history and cleared here, while the turn the
        // core sends is data. Whether or not a session is open, the prompt is
        // data to the core, which creates the session it needs. A paste label
        // stands for text this front end keeps outside the buffer, so the turn
        // carries the whole paste while the history keeps the line as the user
        // saw it.
        self.history.push(content.clone());
        self.clear_input();
        self.submit_turn(self.expand_pastes(&content))
    }

    /// Sends one prompt as one user turn, or starts the session it needs.
    ///
    /// The prompt arrives as data: the input line, its history, the hint menu,
    /// and the slash grammar belong to the interactive path and never decide
    /// what a prompt means here, so a `/…` prompt is a turn and a trailing
    /// backslash is not a line break. A prompt with no open session creates the
    /// session it needs and commits the prompt as that session's first turn in
    /// one command.
    pub(super) fn submit_turn(&mut self, content: String) -> Vec<Effect> {
        let Some(session_id) = self.session_id else {
            self.error = None;
            // A new turn starts a new measurement: the previous turn's elapsed
            // time never survives into the next run's status line.
            self.elapsed_millis = None;
            self.note("creating a session".to_owned());
            return vec![Effect::CreateSession {
                first_turn: Some(content),
            }];
        };
        self.error = None;
        // A new turn starts a new measurement: the previous turn's elapsed time
        // never survives into the next run's status line.
        self.elapsed_millis = None;
        self.note("sending turn".to_owned());
        vec![Effect::SendTurn {
            session_id,
            content,
        }]
    }

    /// Replaces the backslash before the cursor with a line break, if one is
    /// there.
    ///
    /// Returns whether the Enter press inserted a line break instead of
    /// submitting the buffer. The backslash is the escape for that one press:
    /// the same action removes it and inserts the break, so no backslash ever
    /// reaches the submitted prompt, while a backslash not followed by Enter
    /// stays literal. The replacement keeps the character count, so the cursor
    /// stays after the break it just inserted.
    fn insert_line_break(&mut self) -> bool {
        let Some(before) = self.cursor.checked_sub(1) else {
            return false;
        };
        let offset = self.input_byte_offset(before);
        if !self.input[offset..].starts_with('\\') {
            return false;
        }
        self.input.replace_range(offset..offset + 1, "\n");
        self.refresh_menu();
        true
    }

    /// Runs one slash command, spelled without its leading `/`.
    ///
    /// The name is resolved through the one registry, and the transition it
    /// runs is the one its entry names, so the submission path and the hint
    /// menu can never disagree about which commands exist. An unregistered name
    /// answers with the registry's own list.
    ///
    /// The line is resolved through the one registry, so the words after the
    /// name are matched against the values the command declares and a line that
    /// does not satisfy them answers the notice the declaration builds. The
    /// transition a resolved line runs is the one its entry names.
    fn submit_command(&mut self, command: &str) -> Vec<Effect> {
        let invocation = match commands::resolve_command(command).into_invocation() {
            Ok(invocation) => invocation,
            Err(notice) => {
                self.note(notice);
                return Vec::new();
            }
        };
        match invocation.spec().action {
            commands::CommandAction::NewSession => self.request_new_session(),
            commands::CommandAction::SessionsBrowser => self.request_sessions_browser(),
            commands::CommandAction::Theme => {
                // `/theme`'s one argument is the first the command declares: a
                // declared value selects its preference - `auto` included, which
                // stores the request to follow detection - and only the
                // argument's absence opens the picker. The declaration is
                // derived from `ThemePreference::ALL`, so the value's position
                // in it is its preference's.
                match invocation.value_index(0) {
                    Some(position) => self.apply_theme_selected(ThemePreference::ALL[position]),
                    None => self.request_theme_picker(),
                }
            }
        }
    }

    /// Clears the input line, keeping its text recallable through the history.
    pub(super) fn clear_input_into_history(&mut self) -> Vec<Effect> {
        let content = self.input.trim().to_owned();
        if !content.is_empty() {
            self.history.push(content);
        }
        self.clear_input();
        Vec::new()
    }

    /// Clears the input line and leaves the history at the live line.
    fn clear_input(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.history_position = None;
        self.history_draft = None;
        self.refresh_menu();
    }

    /// Replaces the input line with `content` and puts the cursor at its end.
    fn set_input(&mut self, content: String) {
        self.cursor = content.chars().count();
        self.input = content;
        self.refresh_menu();
    }

    /// Returns the byte offset of character index `index` in the input line.
    ///
    /// The cursor is a character index, so every insertion and removal turns it
    /// into the byte offset `String` operates on; an index at or past the end
    /// maps to the line's end.
    fn input_byte_offset(&self, index: usize) -> usize {
        self.input
            .char_indices()
            .nth(index)
            .map_or(self.input.len(), |(offset, _)| offset)
    }

    /// Moves the transcript window by one wheel notch of display rows.
    pub(super) const fn apply_scroll(&mut self, direction: TranscriptScroll) {
        self.scroll_by(direction, TRANSCRIPT_WHEEL_ROWS);
    }

    /// Moves the transcript window by one fast drag step of display rows.
    pub(super) const fn apply_scroll_fast(&mut self, direction: TranscriptScroll) {
        self.scroll_by(direction, TRANSCRIPT_DRAG_ROWS);
    }

    /// Moves the transcript window `rows` display rows towards `direction`.
    const fn scroll_by(&mut self, direction: TranscriptScroll, rows: u16) {
        match direction {
            TranscriptScroll::Older => {
                self.scroll = self.scroll.saturating_add(rows);
            }
            TranscriptScroll::Newer => {
                self.scroll = self.scroll.saturating_sub(rows);
            }
        }
    }
}

/// Returns the index of the first character of the line holding `cursor`.
fn line_start(characters: &[char], cursor: usize) -> usize {
    characters[..cursor]
        .iter()
        .rposition(|character| *character == '\n')
        .map_or(0, |index| index + 1)
}

/// Returns the index of the line break ending the line holding `cursor`.
///
/// A cursor on the buffer's last line reaches the buffer's end instead, which
/// is that line's end.
fn line_end(characters: &[char], cursor: usize) -> usize {
    characters[cursor..]
        .iter()
        .position(|character| *character == '\n')
        .map_or(characters.len(), |index| cursor + index)
}

/// Returns the position one line above or below `cursor` reaches.
///
/// The move keeps the cursor's column and ends the target line where that line
/// is shorter, so the cursor always rides a character of the line it names. At
/// the buffer's first line an upward move and at its last line a downward move
/// return the position they were given: the keymap gives those presses their
/// other meaning instead of moving the cursor nowhere.
fn line_step(characters: &[char], cursor: usize, up: bool) -> usize {
    let column = cursor - line_start(characters, cursor);
    if up {
        let start = line_start(characters, cursor);
        if start == 0 {
            return cursor;
        }
        let previous = line_start(characters, start - 1);
        previous + column.min(start - 1 - previous)
    } else {
        let end = line_end(characters, cursor);
        if end == characters.len() {
            return cursor;
        }
        let next = end + 1;
        next + column.min(line_end(characters, next) - next)
    }
}

/// Returns whether a paste is too large to read in the line.
///
/// Size decides, never timing: a paste longer than a few hundred characters, or
/// taller than a few lines, is one the user cannot read at a glance in the line,
/// so its text waits outside the buffer and one label stands for it. The front
/// end asks the same question about a run of keystrokes, because that run is a
/// paste whenever its own text has grown past this size.
#[must_use]
pub fn paste_is_large(text: &str) -> bool {
    text.chars().count() > PASTE_INLINE_CHARS || text.split('\n').count() > PASTE_INLINE_LINES
}

/// Returns the label one stored paste is shown as.
///
/// The label names the paste's own beginning and how many lines it carries -
/// `[pasted <head>... <n> lines]` - so the line reads as the paste it stands for
/// instead of as an anonymous token. The head is the text's first characters with
/// its line breaks and tabs read as spaces, and with the control characters a
/// terminal may have carried dropped: what is left is the paste's own words, and
/// a paste whose first characters are all control characters shows no head at all
/// rather than an empty label.
fn paste_label(text: &str) -> String {
    let head = paste_head(text);
    let lines = text.split('\n').count().max(1);
    let unit = if lines == 1 { "line" } else { "lines" };
    format!("{PASTE_LABEL_PREFIX}{head}... {lines} {unit}]")
}

/// Returns the first characters of one paste, read as one line of prose.
///
/// The characters an input line cannot show are dropped, the line breaks and
/// tabs inside the head read as the spaces between words, and the result is
/// trimmed, so a paste that starts with an indented line still shows its own
/// words in the label.
fn paste_head(text: &str) -> String {
    let shown = text
        .chars()
        .filter(|character| !is_unshowable_control(*character))
        .take(PASTE_HEAD_CHARS);
    let mut head = String::with_capacity(PASTE_HEAD_CHARS);
    for character in shown {
        match character {
            '\n' | '\t' => head.push(' '),
            other => head.push(other),
        }
    }
    head.trim().to_owned()
}

/// Returns whether one control character is one a terminal may have carried
/// without the user typing it.
///
/// The line break and the tab are text; every other C0 control, and the delete
/// character, are the terminal's own framing.
const fn is_unshowable_control(character: char) -> bool {
    let code = character as u32;
    code <= 8 || code == 127 || (code >= 14 && code <= 31)
}
