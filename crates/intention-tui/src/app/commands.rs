//! The one command registry: the names, descriptions, and categories a
//! submitted line and the input's hint menu both read.
//!
//! A command exists exactly once, as an entry of [`COMMANDS`]. The submission
//! path resolves a typed name through [`command_named`] and runs the transition
//! its entry names, the unknown-command notice lists the same entries through
//! [`known_commands`], and the hint menu filters the same entries through
//! [`suggestions`], so a command can never exist in one surface and not in
//! another. Nothing outside this module spells a command name.

/// One command the chat knows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    /// The command's name, spelled without its leading slash.
    pub name: &'static str,
    /// The one line the hint menu shows beside the name.
    pub description: &'static str,
    /// The category the hint menu's third column shows.
    pub category: &'static str,
    /// The transition a submitted command of this name runs.
    pub(crate) action: CommandAction,
}

/// The transition one registered command runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandAction {
    /// Create a session in the workspace and open it.
    NewSession,
    /// Open the sessions browser and refresh the list it shows.
    SessionsBrowser,
}

/// Every command the chat knows, in the order a menu lists them.
pub const COMMANDS: [CommandSpec; 2] = [
    CommandSpec {
        name: "new",
        description: "create a new session in this workspace",
        category: "Session",
        action: CommandAction::NewSession,
    },
    CommandSpec {
        name: "sessions",
        description: "browse, filter, and switch sessions",
        category: "Navigation",
        action: CommandAction::SessionsBrowser,
    },
];

impl CommandSpec {
    /// Returns the name as a user writes it, with its leading slash.
    #[must_use]
    pub fn typed_name(self) -> String {
        format!("/{}", self.name)
    }
}

/// Returns the registered command one typed name selects, if any.
#[must_use]
pub fn command_named(name: &str) -> Option<CommandSpec> {
    COMMANDS
        .iter()
        .copied()
        .find(|command| command.name == name)
}

/// Returns every registered command name as one line, in registry order.
#[must_use]
pub fn known_commands() -> String {
    COMMANDS
        .iter()
        .map(|command| command.typed_name())
        .collect::<Vec<_>>()
        .join(" ")
}

/// One command word of the input line: the range it occupies and its filter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandWord {
    /// The byte offset of the word's leading slash.
    pub(crate) start: usize,
    /// The byte offset just past the word's last character.
    pub(crate) end: usize,
    /// The text between the slash and the word's end.
    pub(crate) filter: String,
}

/// Returns the command word the caret sits in or immediately after.
///
/// A slash is a command only as a submitted line's *first* character, so only
/// the input's first line can carry a word, and only while that line starts
/// with `/`: a slash anywhere else is ordinary text that opens no menu. The
/// word ends at the first blank of that line, so the caret is inside or
/// immediately after it exactly while it lies between the slash and that end.
/// The filter is what the user typed after the slash, which is what the hint
/// menu ranks.
#[must_use]
pub fn command_word(input: &str, cursor: usize) -> Option<CommandWord> {
    if !input.starts_with('/') {
        return None;
    }
    let line_end = input.find('\n').unwrap_or(input.len());
    let word_end = input[..line_end]
        .find(char::is_whitespace)
        .unwrap_or(line_end);
    if byte_offset(input, cursor) > word_end {
        return None;
    }
    Some(CommandWord {
        start: 0,
        end: word_end,
        filter: input[1..word_end].to_owned(),
    })
}

/// Returns the byte offset of character index `index` in `input`.
///
/// The caret is a character index, so every menu decision turns it into the
/// byte offset `String` operates on; an index at or past the end maps to the
/// line's end.
fn byte_offset(input: &str, index: usize) -> usize {
    input
        .char_indices()
        .nth(index)
        .map_or(input.len(), |(offset, _)| offset)
}

/// Returns the registry rows one filter selects, best match first.
///
/// The order is the match rank first and the registry order second, so two
/// equally good matches keep the order the registry declares and the list never
/// reorders itself while the user types.
#[must_use]
pub fn suggestions(filter: &str) -> Vec<usize> {
    let mut ranked = COMMANDS
        .iter()
        .enumerate()
        .filter_map(|(index, command)| match_rank(filter, command.name).map(|rank| (rank, index)))
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(rank, index)| (*rank, *index));
    ranked.into_iter().map(|(_, index)| index).collect()
}

/// Returns the match rank of one name for one filter, if it matches at all.
///
/// The rule is deterministic and total, over lower-cased text: rank `0` when
/// the name starts with the filter, rank `1` when the filter's characters
/// appear in the name in order, so `/ssn` finds `/sessions`, and `None`
/// otherwise. An empty filter matches every name at rank `0`, which is the
/// state a bare `/` opens the menu in.
fn match_rank(filter: &str, name: &str) -> Option<u8> {
    let filter = filter.to_lowercase();
    let name = name.to_lowercase();
    if name.starts_with(&filter) {
        return Some(0);
    }
    let mut characters = name.chars();
    filter
        .chars()
        .all(|wanted| characters.any(|candidate| candidate == wanted))
        .then_some(1)
}

/// The input's command hint menu: the commands the current word selects and the
/// row the user highlighted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandMenu {
    /// The filter the matches were computed for.
    filter: String,
    /// The registry rows the filter selects, best match first.
    matches: Vec<usize>,
    /// The highlighted row of `matches`.
    highlight: usize,
}

impl CommandMenu {
    /// Returns the menu one word's filter opens, or `None` for a filter no
    /// command matches.
    pub(crate) fn opened(filter: &str) -> Option<Self> {
        let matches = suggestions(filter);
        (!matches.is_empty()).then(|| Self {
            filter: filter.to_owned(),
            matches,
            highlight: 0,
        })
    }

    /// Returns the filter this menu ranks for.
    pub(crate) fn filter(&self) -> &str {
        &self.filter
    }

    /// Returns how many commands the menu offers.
    pub(crate) const fn len(&self) -> usize {
        self.matches.len()
    }

    /// Returns the highlighted row.
    pub(crate) const fn highlight(&self) -> usize {
        self.highlight
    }

    /// Returns every command the menu offers, best match first.
    pub(crate) fn commands(&self) -> impl Iterator<Item = CommandSpec> + '_ {
        self.matches.iter().map(|index| COMMANDS[*index])
    }

    /// Returns the highlighted command.
    pub(crate) fn highlighted(&self) -> Option<CommandSpec> {
        self.matches
            .get(self.highlight)
            .map(|index| COMMANDS[*index])
    }

    /// Moves the highlight one row towards `direction`, bounded by the list.
    ///
    /// The highlight stops at the first and last row instead of wrapping, which
    /// is the same bounded walk the sessions browser's cursor makes.
    pub(crate) fn move_highlight(&mut self, direction: MenuMove) {
        let last = self.matches.len().saturating_sub(1);
        self.highlight = match direction {
            MenuMove::Up => self.highlight.saturating_sub(1),
            MenuMove::Down => self.highlight.saturating_add(1).min(last),
        };
    }
}

/// One step through the input's command hint menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuMove {
    /// Move the highlight one row up.
    Up,
    /// Move the highlight one row down.
    Down,
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Registry tests assert the declared commands and their ranking directly."
    )]

    use super::{
        COMMANDS, CommandMenu, MenuMove, command_named, command_word, known_commands, match_rank,
        suggestions,
    };

    #[test]
    fn the_registry_names_every_command_once_with_a_description_and_category() {
        assert_eq!(COMMANDS.len(), 2, "the chat knows two commands today");
        for command in COMMANDS {
            assert!(
                !command.name.starts_with('/'),
                "the registry stores the name"
            );
            assert!(
                !command.description.trim().is_empty() && !command.category.trim().is_empty(),
                "/{command:?} carries a real description and category"
            );
        }
        let mut names = COMMANDS.map(|command| command.name).to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COMMANDS.len(), "a name is registered once");
        assert_eq!(command_named("new").expect("new is registered").name, "new");
        assert_eq!(command_named("connectors"), None);
        assert_eq!(known_commands(), "/new /sessions");
    }

    #[test]
    fn a_command_word_starts_the_first_line_and_ends_at_its_first_blank() {
        let word = command_word("/sessions", 3).expect("a caret inside the word is in it");
        assert_eq!((word.start, word.end), (0, 9));
        assert_eq!(word.filter, "sessions");
        assert!(
            command_word("/sessions", 9).is_some(),
            "the caret immediately after the word is in it"
        );
        assert!(
            command_word("/new argument", 4).is_some(),
            "the caret inside the word is in it"
        );
        assert!(
            command_word("/new argument", 5).is_none(),
            "the caret after the word's blank has left the word"
        );
        assert!(
            command_word("read /new", 9).is_none(),
            "a slash that is not the first character is ordinary text"
        );
        assert!(
            command_word("sessions", 8).is_none(),
            "a line that does not start with a slash carries no command word"
        );
        assert!(
            command_word("/new\nsecond", 9).is_none(),
            "only the first line can carry a command word"
        );
    }

    #[test]
    fn the_ranking_prefers_a_prefix_over_an_ordered_subsequence() {
        assert_eq!(match_rank("", "connectors"), Some(0));
        assert_eq!(match_rank("con", "connectors"), Some(0));
        assert_eq!(match_rank("cnt", "connectors"), Some(1));
        assert_eq!(match_rank("NEC", "connectors"), Some(1));
        assert_eq!(match_rank("zzz", "connectors"), None);
        assert_eq!(
            suggestions(""),
            vec![0, 1],
            "an empty filter keeps the registry order"
        );
        assert_eq!(suggestions("s"), vec![1], "only /sessions starts with s");
        assert_eq!(
            suggestions("n"),
            vec![0, 1],
            "/new starts with n, and n is also an ordered subsequence of sessions"
        );
        assert!(suggestions("zzz").is_empty());
    }

    #[test]
    fn a_menu_opens_only_for_a_filter_that_selects_a_command() {
        assert!(CommandMenu::opened("zzz").is_none(), "no match, no menu");

        let menu = CommandMenu::opened("se").expect("se selects sessions");
        assert_eq!(menu.filter(), "se");
        assert_eq!(menu.len(), 1);
        assert_eq!(menu.highlight(), 0, "the best match starts highlighted");
        assert_eq!(
            menu.commands()
                .map(|command| command.name)
                .collect::<Vec<_>>(),
            vec!["sessions"]
        );
        assert_eq!(
            menu.highlighted().expect("a row is highlighted").name,
            "sessions"
        );
    }

    #[test]
    fn the_highlight_walks_the_list_without_wrapping() {
        let mut menu = CommandMenu::opened("").expect("a bare slash opens the menu");
        assert_eq!(menu.highlight(), 0);
        menu.move_highlight(MenuMove::Up);
        assert_eq!(menu.highlight(), 0, "the first row is the top");
        menu.move_highlight(MenuMove::Down);
        assert_eq!(menu.highlight(), 1);
        menu.move_highlight(MenuMove::Down);
        assert_eq!(menu.highlight(), 1, "the last row is the bottom");
    }
}
