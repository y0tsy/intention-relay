//! The one command registry: the names, arguments, descriptions, and
//! categories a submitted line and the input's hint band both read.
//!
//! A command exists exactly once, as an entry of [`COMMANDS`]. The submission
//! path resolves a typed line through [`resolve_command`] and runs the
//! transition its entry names, the unknown-command notice lists the same
//! entries through [`known_commands`], and the hint band ranks the same
//! entries - and the values of a declared argument - through [`CommandMenu`],
//! so a command can never exist in one surface and not in another. Both
//! notices a resolved line can answer with are built here too, from the same
//! declarations. Nothing outside this module spells a command name.

use super::Theme;

/// One value one declared command argument accepts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueSpec {
    /// The value, exactly as the completion writes it into the input line.
    ///
    /// Matching ignores case; the spelling here is the canonical one the band
    /// shows and the notices name.
    pub value: &'static str,
    /// The one line the band shows beside the value.
    pub description: &'static str,
}

/// One argument a command declares.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArgumentSpec {
    /// The argument's name, as the band's third column and the notices spell it.
    pub name: &'static str,
    /// Every value the argument accepts, in the order the band lists them.
    pub values: &'static [ValueSpec],
    /// Whether a submitted line must spell one of the values.
    ///
    /// A command that omits an optional argument has something else to do with
    /// the omission - `/theme` with no value opens its picker - where a missing
    /// required one is a notice.
    pub required: bool,
}

impl ArgumentSpec {
    /// Returns the declared value one word names, however the word is spelled.
    fn value_named(self, word: &str) -> Option<&'static ValueSpec> {
        self.values
            .iter()
            .find(|value| value.value.eq_ignore_ascii_case(word))
    }

    /// Returns the accepted values as the notices spell them: `light or dark`.
    fn accepted_values(self) -> String {
        self.values
            .iter()
            .map(|value| value.value)
            .collect::<Vec<_>>()
            .join(" or ")
    }
}

/// One command the chat knows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    /// The command's name, spelled without its leading slash.
    pub name: &'static str,
    /// The one line the hint band shows beside the name.
    pub description: &'static str,
    /// The category the hint band's third column shows.
    pub category: &'static str,
    /// The arguments the command declares, in the order a line fills them.
    ///
    /// An empty slice is a command that takes no argument: a space after its
    /// name closes the hint band instead of opening an argument word. A
    /// declared argument is what makes the band list values.
    pub arguments: &'static [ArgumentSpec],
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
    /// Select the theme one argument names, or open the theme picker when the
    /// line omitted the argument.
    Theme,
}

/// The values `/theme` accepts, in the order the picker draws its rows.
///
/// The spellings and descriptions are the theme's own vocabulary, so the
/// command surface and the picker panel can never disagree about what the two
/// themes are called or which surface each one paints.
// @todo(hack): the values are re-spelled here from `Theme::as_str`, and only a
// `debug_assert!` in `input.rs` keeps the two vocabularies in sync; the
// registry should derive its values from the theme vocabulary itself.
static THEME_VALUES: [ValueSpec; 2] = [
    ValueSpec {
        value: Theme::Light.as_str(),
        description: Theme::Light.description(),
    },
    ValueSpec {
        value: Theme::Dark.as_str(),
        description: Theme::Dark.description(),
    },
];

/// The one argument `/theme` declares.
static THEME_ARGUMENT: [ArgumentSpec; 1] = [ArgumentSpec {
    name: "theme",
    values: &THEME_VALUES,
    required: false,
}];

/// Every command the chat knows, in the order a menu lists them.
pub const COMMANDS: [CommandSpec; 3] = [
    CommandSpec {
        name: "new",
        description: "create a new session in this workspace",
        category: "Session",
        arguments: &[],
        action: CommandAction::NewSession,
    },
    CommandSpec {
        name: "sessions",
        description: "browse, filter, and switch sessions",
        category: "Navigation",
        arguments: &[],
        action: CommandAction::SessionsBrowser,
    },
    CommandSpec {
        name: "theme",
        description: "pick the colour theme the terminal paints with",
        category: "Appearance",
        arguments: &THEME_ARGUMENT,
        action: CommandAction::Theme,
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

/// One word of the input line: the range it occupies and the filter it ranks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandWord {
    /// The byte offset the word starts at: the line's leading slash for the
    /// command word, the first character of the argument word otherwise.
    pub(crate) start: usize,
    /// The byte offset just past the word's last character.
    pub(crate) end: usize,
    /// The text the word ranks with.
    pub(crate) filter: String,
    /// The argument this word fills; `None` for the command word.
    pub(crate) argument: Option<ArgumentSpec>,
}

/// Returns the command or argument word the caret sits in or immediately after.
///
/// A slash is a command only as a submitted line's *first* character, so only
/// the input's first line can carry a word, and only while that line starts
/// with `/`: a slash anywhere else is ordinary text that opens no band. The
/// command word ends at the first blank of that line, so the caret is inside
/// or immediately after it exactly while it lies between the slash and that
/// end; the filter is what the user typed after the slash.
///
/// Past the command word, the caret sits in one of the command's argument
/// words: only a command that declares the argument the caret's word fills
/// answers with a word, which is what makes a space after a complete command
/// name reopen the band for `/theme ` and close it for `/new `. The filter is
/// the whole word, so a caret moved inside the word it is already on ranks the
/// same list.
#[must_use]
pub fn command_word(input: &str, cursor: usize) -> Option<CommandWord> {
    if !input.starts_with('/') {
        return None;
    }
    let line_end = input.find('\n').unwrap_or(input.len());
    let line = &input[..line_end];
    let caret = byte_offset(input, cursor);
    if caret > line_end {
        return None;
    }
    let name_end = line.find(char::is_whitespace).unwrap_or(line.len());
    if caret <= name_end {
        return Some(CommandWord {
            start: 0,
            end: name_end,
            filter: line[1..name_end].to_owned(),
            argument: None,
        });
    }
    let command = command_named(&line[1..name_end])?;
    let (index, (start, end)) = argument_word(&line[name_end..], caret - name_end)?;
    let argument = *command.arguments.get(index)?;
    Some(CommandWord {
        start: name_end + start,
        end: name_end + end,
        filter: line[name_end + start..name_end + end].to_owned(),
        argument: Some(argument),
    })
}

/// Returns the argument word one caret sits in, as its index and its byte
/// range inside `rest`.
///
/// The words are the maximal non-blank runs after the command word, so the
/// first one is the first declared argument's. The caret belongs to the word
/// it lies in; a caret at the line's end belongs to the empty word following
/// the words it is past, which is what opens the band while the user has typed
/// the blank but no value yet. A caret between two words, or in a word past
/// the last word, belongs to no argument.
fn argument_word(rest: &str, caret: usize) -> Option<(usize, (usize, usize))> {
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (offset, character) in rest.char_indices() {
        if character.is_whitespace() {
            if let Some(word_start) = start.take() {
                words.push((word_start, offset));
            }
        } else {
            start.get_or_insert(offset);
        }
    }
    if let Some(start) = start {
        words.push((start, rest.len()));
    }
    for (index, (start, end)) in words.iter().enumerate() {
        if *start <= caret && caret <= *end {
            return Some((index, (*start, *end)));
        }
    }
    (caret == rest.len()).then_some((words.len(), (rest.len(), rest.len())))
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
    ranked(filter, COMMANDS.iter().map(|command| command.name))
}

/// Returns the value rows one filter selects, best match first.
///
/// The rule and the ordering are the command rows' own: a prefix beats an
/// ordered subsequence, ties keep the declaration order, and an empty filter
/// lists every value.
fn value_suggestions(filter: &str, values: &'static [ValueSpec]) -> Vec<usize> {
    ranked(filter, values.iter().map(|value| value.value))
}

/// Returns the indices of the names one filter selects, best match first.
fn ranked<'a>(filter: &str, names: impl Iterator<Item = &'a str>) -> Vec<usize> {
    let mut ranked = names
        .enumerate()
        .filter_map(|(index, name)| match_rank(filter, name).map(|rank| (rank, index)))
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

/// What one hint band ranks.
///
/// The band is one surface with two vocabularies: the command word ranks the
/// registry, and an argument word ranks that argument's declared values in the
/// same three-column shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuKind {
    /// The command word: the rows are registered commands.
    Command,
    /// An argument word: the rows are that argument's declared values.
    Argument(ArgumentSpec),
}

/// One row of the hint band: the three cells the band draws.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuRow {
    /// One registered command.
    Command(CommandSpec),
    /// One value of a declared argument, with the argument it fills.
    Value {
        /// The argument the value belongs to.
        argument: ArgumentSpec,
        /// The value itself.
        value: ValueSpec,
    },
}

impl MenuRow {
    /// Returns the first cell, exactly as the band paints it: a command's typed
    /// name or one argument value.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Command(command) => command.typed_name(),
            Self::Value { value, .. } => value.value.to_owned(),
        }
    }

    /// Returns the second cell: the row's one-line description.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Command(command) => command.description,
            Self::Value { value, .. } => value.description,
        }
    }

    /// Returns the third cell: the command's category or the argument's name.
    #[must_use]
    pub const fn trailing(self) -> &'static str {
        match self {
            Self::Command(command) => command.category,
            Self::Value { argument, .. } => argument.name,
        }
    }

    /// Returns the text this row completes its word with: the label and one
    /// trailing space, ready for the argument - or the submission - after it.
    #[must_use]
    pub fn completion(self) -> String {
        match self {
            Self::Command(command) => format!("{} ", command.typed_name()),
            Self::Value { value, .. } => format!("{} ", value.value),
        }
    }
}

/// The input's hint band: the rows the current word selects and the row the
/// user highlighted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandMenu {
    /// The filter the matches were computed for.
    filter: String,
    /// What the word ranks: commands or one argument's values.
    kind: MenuKind,
    /// The matched rows, best match first.
    matches: Vec<usize>,
    /// The highlighted row of `matches`.
    highlight: usize,
}

impl CommandMenu {
    /// Returns the band one word's filter opens, or `None` for a filter that
    /// selects no row.
    pub(crate) fn opened(word: &CommandWord) -> Option<Self> {
        let (kind, matches) = word.argument.map_or_else(
            || (MenuKind::Command, suggestions(&word.filter)),
            |argument| {
                (
                    MenuKind::Argument(argument),
                    value_suggestions(&word.filter, argument.values),
                )
            },
        );
        (!matches.is_empty()).then(|| Self {
            filter: word.filter.clone(),
            kind,
            matches,
            highlight: 0,
        })
    }

    /// Returns what this menu ranks.
    pub(crate) const fn kind(&self) -> MenuKind {
        self.kind
    }

    /// Returns how many rows the menu offers.
    pub(crate) const fn len(&self) -> usize {
        self.matches.len()
    }

    /// Returns the highlighted row.
    pub(crate) const fn highlight(&self) -> usize {
        self.highlight
    }

    /// Returns every row the menu offers, best match first.
    pub(crate) fn rows(&self) -> impl Iterator<Item = MenuRow> + '_ {
        self.matches.iter().map(|index| self.row(*index))
    }

    /// Returns the highlighted row.
    pub(crate) fn highlighted(&self) -> Option<MenuRow> {
        self.matches
            .get(self.highlight)
            .map(|index| self.row(*index))
    }

    /// Returns the row one match index names.
    fn row(&self, index: usize) -> MenuRow {
        match self.kind {
            MenuKind::Command => MenuRow::Command(COMMANDS[index]),
            MenuKind::Argument(argument) => MenuRow::Value {
                argument,
                value: argument.values[index],
            },
        }
    }

    /// Returns whether two menus rank the same word.
    ///
    /// The same filter for the same argument means the user's highlight still
    /// points at a row of the same list, so moving the caret inside the word it
    /// is already on never moves the highlight; anything else starts at the
    /// best match.
    pub(crate) fn ranks_the_same_word(&self, other: &Self) -> bool {
        self.filter == other.filter && self.kind == other.kind
    }

    /// Returns whether this band has no word to complete.
    ///
    /// The empty argument word is the one band with nothing to complete: the
    /// command is already complete and none of its argument is typed, so Enter
    /// runs the command - `/theme` opens the picker - while Tab still completes
    /// the value the band highlights.
    pub(crate) const fn completes_nothing(&self) -> bool {
        matches!(self.kind, MenuKind::Argument(_)) && self.filter.is_empty()
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

/// One step through the input's command hint band.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuMove {
    /// Move the highlight one row up.
    Up,
    /// Move the highlight one row down.
    Down,
}

/// One resolved command line: the registered command and its argument values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandInvocation {
    /// The command the line named.
    spec: CommandSpec,
    /// The resolved value of each declared argument, in declaration order;
    /// `None` for an optional argument the line omitted.
    values: Vec<Option<&'static ValueSpec>>,
}

impl CommandInvocation {
    /// Returns the registered command the line named.
    #[must_use]
    pub const fn spec(&self) -> CommandSpec {
        self.spec
    }

    /// Returns the value the declared argument at `index` was given, if any.
    ///
    /// The index is the argument's position in the command's declaration,
    /// which is the order the words on the line fill.
    #[must_use]
    pub fn value(&self, index: usize) -> Option<&'static str> {
        self.values
            .get(index)
            .copied()
            .flatten()
            .map(|value| value.value)
    }
}

/// The outcome of resolving one submitted line through the one registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommandResolution {
    /// The line names a registered command and satisfies its arguments.
    Invocation(CommandInvocation),
    /// The line names no registered command.
    UnknownName {
        /// The name the line spelled, without its leading slash.
        name: String,
    },
    /// A word names no value the argument it fills declares.
    UnknownValue {
        /// The argument the word was matched against.
        argument: ArgumentSpec,
        /// The word the line spelled.
        value: String,
    },
    /// A required argument was omitted.
    MissingArgument {
        /// The argument the line omitted.
        argument: ArgumentSpec,
    },
    /// A word follows the command's last declared argument.
    UnexpectedArgument {
        /// The word the line spelled.
        value: String,
    },
}

impl CommandResolution {
    /// Returns the one-line notice this resolution answers with, or `None` when
    /// the line resolved to a command.
    ///
    /// Every notice is built from the same declaration the line was resolved
    /// against: the unknown name lists the registry, and a bad argument names
    /// the argument and the values it declares.
    #[must_use]
    pub fn notice(&self) -> Option<String> {
        match self {
            Self::Invocation(_) => None,
            Self::UnknownName { name } => Some(format!(
                "unknown command /{name}; known commands: {}",
                known_commands()
            )),
            Self::UnknownValue { argument, value } => Some(format!(
                "unknown {} \"{value}\" - expected {}",
                argument.name,
                argument.accepted_values()
            )),
            Self::MissingArgument { argument } => Some(format!(
                "missing argument \"{}\" - expected {}",
                argument.name,
                argument.accepted_values()
            )),
            Self::UnexpectedArgument { value } => Some(format!("unexpected argument \"{value}\"")),
        }
    }

    /// Returns the invocation this resolution carries.
    ///
    /// # Errors
    ///
    /// Returns the one-line notice the line answers with when it spells no
    /// command.
    pub fn into_invocation(self) -> Result<CommandInvocation, String> {
        match self {
            Self::Invocation(invocation) => Ok(invocation),
            resolution => Err(resolution.notice().unwrap_or_default()),
        }
    }
}

/// Returns the invocation one submitted command line spells, or the reason it
/// spells none.
///
/// The line is the text after its leading slash, trimmed. The first word names
/// the command; every word after it fills one declared argument, in
/// declaration order, and must name one of that argument's values. The parser
/// reads only the registry, so a declaration added there is parsed, ranked, and
/// explained here without another spelling of it anywhere.
#[must_use]
pub fn resolve_command(line: &str) -> CommandResolution {
    let mut words = line.split_whitespace();
    let name = words.next().unwrap_or_default();
    let Some(spec) = command_named(name) else {
        return CommandResolution::UnknownName {
            name: name.to_owned(),
        };
    };
    resolve_arguments(spec, &words.collect::<Vec<_>>())
}

/// Returns the resolution of one command's argument words.
fn resolve_arguments(spec: CommandSpec, words: &[&str]) -> CommandResolution {
    let mut values = Vec::with_capacity(spec.arguments.len());
    for (index, argument) in spec.arguments.iter().enumerate() {
        match words.get(index) {
            None => {
                if argument.required {
                    return CommandResolution::MissingArgument {
                        argument: *argument,
                    };
                }
                values.push(None);
            }
            Some(word) => match argument.value_named(word) {
                Some(value) => values.push(Some(value)),
                None => {
                    return CommandResolution::UnknownValue {
                        argument: *argument,
                        value: (*word).to_owned(),
                    };
                }
            },
        }
    }
    if let Some(unexpected) = words.get(spec.arguments.len()) {
        return CommandResolution::UnexpectedArgument {
            value: (*unexpected).to_owned(),
        };
    }
    CommandResolution::Invocation(CommandInvocation { spec, values })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Registry tests assert the declared commands and their ranking directly."
    )]

    use super::{
        ArgumentSpec, COMMANDS, CommandAction, CommandMenu, CommandResolution, CommandSpec,
        MenuKind, MenuMove, ValueSpec, command_named, command_word, known_commands, match_rank,
        resolve_arguments, resolve_command, suggestions,
    };

    /// A synthetic required argument, so the resolver's missing-word arm can be
    /// exercised by a declaration the registry does not carry.
    static REQUIRED: [ArgumentSpec; 1] = [ArgumentSpec {
        name: "shape",
        values: &[
            ValueSpec {
                value: "round",
                description: "the round fixture shape",
            },
            ValueSpec {
                value: "square",
                description: "the square fixture shape",
            },
        ],
        required: true,
    }];

    /// Returns the menu one input line opens with its caret at the line's end.
    fn menu_of(input: &str) -> Option<CommandMenu> {
        let word = command_word(input, input.chars().count())?;
        CommandMenu::opened(&word)
    }

    /// Returns the labels of every row one menu offers.
    fn labels(menu: &CommandMenu) -> Vec<String> {
        menu.rows().map(|row| row.label()).collect()
    }

    #[test]
    fn the_registry_names_every_command_once_with_a_description_and_category() {
        assert_eq!(COMMANDS.len(), 3, "the chat knows three commands today");
        for command in COMMANDS {
            assert!(
                !command.name.starts_with('/'),
                "the registry stores the name"
            );
            assert!(
                !command.description.trim().is_empty() && !command.category.trim().is_empty(),
                "/{command:?} carries a real description and category"
            );
            for argument in command.arguments {
                assert!(
                    !argument.name.trim().is_empty(),
                    "/{} declares a named argument",
                    command.name
                );
                assert!(
                    !argument.values.is_empty(),
                    "the {} argument declares values",
                    argument.name
                );
                for value in argument.values {
                    assert!(
                        !value.value.trim().is_empty() && !value.description.trim().is_empty(),
                        "{} carries a value and a description",
                        argument.name
                    );
                }
            }
        }
        let mut names = COMMANDS.map(|command| command.name).to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COMMANDS.len(), "a name is registered once");
        assert_eq!(command_named("new").expect("new is registered").name, "new");
        assert_eq!(command_named("connectors"), None);
        assert_eq!(known_commands(), "/new /sessions /theme");
    }

    #[test]
    fn the_theme_command_declares_one_optional_argument_with_the_two_themes() {
        let theme = command_named("theme").expect("theme is registered");
        assert_eq!(theme.category, "Appearance");
        assert_eq!(theme.action, CommandAction::Theme);
        assert_eq!(theme.arguments.len(), 1);
        let argument = theme.arguments[0];
        assert_eq!(argument.name, "theme");
        assert!(!argument.required, "the picker needs no argument");
        assert_eq!(
            argument
                .values
                .iter()
                .map(|value| value.value)
                .collect::<Vec<_>>(),
            vec!["light", "dark"],
            "the declared values are the theme vocabulary in picker order"
        );
        assert_eq!(
            argument.values[0].description,
            crate::app::Theme::Light.description(),
            "the command surface reads the theme's own row vocabulary"
        );
    }

    #[test]
    fn a_submitted_line_resolves_its_name_and_its_argument_values() {
        let invocation = resolve_command("theme dark")
            .into_invocation()
            .expect("theme dark resolves to its command");
        assert_eq!(invocation.spec(), command_named("theme").expect("theme"));
        assert_eq!(invocation.value(0), Some("dark"));
        assert_eq!(invocation.value(1), None, "no second argument is declared");

        let invocation = resolve_command("theme DARK")
            .into_invocation()
            .expect("a value matches however it is spelled");
        assert_eq!(
            invocation.value(0),
            Some("dark"),
            "the canonical spelling is what a caller sees"
        );

        let invocation = resolve_command("theme")
            .into_invocation()
            .expect("the argument is optional");
        assert_eq!(invocation.value(0), None);
    }

    #[test]
    fn a_value_no_declaration_answers_to_names_the_accepted_values() {
        assert_eq!(
            resolve_command("theme purple").notice().as_deref(),
            Some("unknown theme \"purple\" - expected light or dark")
        );
        assert_eq!(
            resolve_command("new purple").into_invocation(),
            Err("unexpected argument \"purple\"".to_owned()),
            "a command that declares no argument treats the word as extra"
        );
    }

    #[test]
    fn a_word_past_the_last_declared_argument_is_unexpected() {
        assert_eq!(
            resolve_command("theme dark now").notice().as_deref(),
            Some("unexpected argument \"now\"")
        );
        assert_eq!(
            resolve_command("theme dark Dark").notice().as_deref(),
            Some("unexpected argument \"Dark\""),
            "an extra word is extra even when it names a declared value"
        );
    }

    #[test]
    fn an_omitted_required_argument_names_itself_and_its_values() {
        let spec = CommandSpec {
            name: "synthetic",
            description: "a synthetic fixture command",
            category: "Fixture",
            arguments: &REQUIRED,
            action: CommandAction::NewSession,
        };
        let resolution = resolve_arguments(spec, &[]);
        assert_eq!(
            resolution,
            CommandResolution::MissingArgument {
                argument: REQUIRED[0]
            }
        );
        assert_eq!(
            resolution.notice().as_deref(),
            Some("missing argument \"shape\" - expected round or square")
        );
    }

    #[test]
    fn a_command_word_starts_the_first_line_and_ends_at_its_first_blank() {
        let word = command_word("/sessions", 3).expect("a caret inside the word is in it");
        assert_eq!((word.start, word.end), (0, 9));
        assert_eq!(word.filter, "sessions");
        assert_eq!(word.argument, None, "the command word fills no argument");
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
            "a command that declares no argument has no word for the caret"
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
    fn an_argument_word_opens_for_the_declared_argument_only() {
        let word =
            command_word("/theme ", 7).expect("the blank after a declared argument opens it");
        assert_eq!((word.start, word.end), (7, 7), "the word is empty so far");
        assert_eq!(word.filter, "");
        assert_eq!(
            word.argument
                .expect("the word fills the theme argument")
                .name,
            "theme"
        );

        let word = command_word("/theme d", 8).expect("the value word is open");
        assert_eq!((word.start, word.end), (7, 8));
        assert_eq!(word.filter, "d", "the filter is the whole word");
        assert!(
            command_word("/theme dark", 11).is_some(),
            "the caret immediately after a complete value is still in its word"
        );
        assert!(
            command_word("/theme dark ", 12).is_none(),
            "the blank after a complete value closes the band"
        );
        let word = command_word("/theme  ", 8).expect("a second blank keeps the empty first word");
        assert_eq!(
            (word.start, word.end, word.filter.as_str()),
            (8, 8, ""),
            "the caret past every blank is the empty word it would fill"
        );
        assert!(
            command_word("/theme dark now", 15).is_none(),
            "a word past the last declared argument fills none"
        );
        assert!(
            command_word("/sessions extra", 14).is_none(),
            "a command that declares no argument has no word past its name"
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
            vec![0, 1, 2],
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
        assert!(menu_of("/zzz").is_none(), "no match, no menu");

        let menu = menu_of("/se").expect("se selects sessions");
        assert_eq!(menu.kind(), MenuKind::Command);
        assert_eq!(menu.len(), 1);
        assert_eq!(menu.highlight(), 0, "the best match starts highlighted");
        assert_eq!(labels(&menu), vec!["/sessions"]);
        assert_eq!(
            menu.highlighted().expect("a row is highlighted").label(),
            "/sessions"
        );
    }

    #[test]
    fn an_argument_menu_ranks_the_declared_values_the_commands_own_way() {
        let menu = menu_of("/theme ").expect("a declared argument opens the band");
        assert_eq!(
            menu.kind(),
            MenuKind::Argument(command_named("theme").expect("theme").arguments[0])
        );
        assert_eq!(
            labels(&menu),
            vec!["light", "dark"],
            "an empty filter lists every value in declaration order"
        );

        assert_eq!(
            labels(&menu_of("/theme d").expect("d selects dark")),
            vec!["dark"]
        );
        assert_eq!(
            labels(&menu_of("/theme rk").expect("rk is an ordered subsequence of dark")),
            vec!["dark"],
            "the value ranking is the name ranking"
        );
        assert!(
            menu_of("/theme zz").is_none(),
            "a value filter that selects nothing closes the band"
        );
    }

    #[test]
    fn a_menu_row_completes_with_one_trailing_space() {
        let menu = menu_of("/theme d").expect("d selects dark");
        let row = menu.highlighted().expect("a row is highlighted");
        assert_eq!(row.completion(), "dark ");
        assert_eq!(row.description(), "the warm charcoal surface");
        assert_eq!(row.trailing(), "theme", "the third cell names the argument");

        let menu = menu_of("/se").expect("se selects sessions");
        let row = menu.highlighted().expect("a row is highlighted");
        assert_eq!(row.completion(), "/sessions ");
        assert_eq!(row.trailing(), "Navigation");
    }

    #[test]
    fn the_highlight_walks_the_list_without_wrapping() {
        let mut menu = menu_of("/").expect("a bare slash opens the band");
        assert_eq!(menu.highlight(), 0);
        menu.move_highlight(MenuMove::Up);
        assert_eq!(menu.highlight(), 0, "the first row is the top");
        menu.move_highlight(MenuMove::Down);
        assert_eq!(menu.highlight(), 1);
        menu.move_highlight(MenuMove::Down);
        assert_eq!(menu.highlight(), 2);
        menu.move_highlight(MenuMove::Down);
        assert_eq!(menu.highlight(), 2, "the last row is the bottom");
    }

    #[test]
    fn the_highlight_survives_a_caret_move_inside_the_word_it_is_on() {
        let menu = menu_of("/theme ").expect("the empty word opens the band");
        let mut moved = menu_of("/theme ").expect("the same word opens the same band");
        moved.move_highlight(MenuMove::Down);
        assert!(
            menu.ranks_the_same_word(&moved),
            "the same filter for the same argument keeps the user's highlight"
        );

        let other = menu_of("/theme d").expect("d selects dark");
        assert!(
            !menu.ranks_the_same_word(&other),
            "another word starts the highlight at the best match"
        );
        let command = menu_of("/th").expect("th selects theme");
        assert!(
            !command.ranks_the_same_word(&menu),
            "the command list and an argument list are never the same word"
        );
    }
}
