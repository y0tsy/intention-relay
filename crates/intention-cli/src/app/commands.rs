//! The one command registry: the names, arguments, and transitions the line
//! front ends read.
//!
//! A command exists exactly once, as an entry of [`COMMANDS`]. A typed line
//! resolves through [`resolve_command`] and runs the transition its entry
//! names, and the unknown-command notice lists the same entries through
//! [`known_commands`], so a command can never exist in one front end and not in
//! another. Nothing outside this module spells a command name.
//!
//! The fullscreen surface keeps its own registry: it owns surfaces this grammar
//! does not drive, so a command that exists only there is not a command this
//! binary pretends to know.

/// One argument a command declares.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArgumentSpec {
    /// The argument's name, as the notices spell it.
    pub name: &'static str,
    /// Whether a submitted line must spell the argument.
    pub required: bool,
}

/// One command a line front end knows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    /// The command's name, spelled without its leading slash.
    pub name: &'static str,
    /// The arguments the command declares, in the order a line fills them.
    ///
    /// An empty slice is a command that takes no argument.
    pub arguments: &'static [ArgumentSpec],
    /// The transition a submitted command of this name runs.
    pub(crate) action: CommandAction,
}

/// The transition one registered command runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandAction {
    /// Create a session in the workspace and open it.
    NewSession,
    /// Open the session the command's one argument names.
    OpenSession,
    /// Leave the front end.
    Quit,
}

/// The one argument `/session` declares: the identifier of the session to open.
///
/// The identifier is a durable value no declaration can list, so the word
/// passes through to the transition that opens the session.
static SESSION_ARGUMENT: [ArgumentSpec; 1] = [ArgumentSpec {
    name: "session",
    required: true,
}];

/// Every command a line front end knows, in the order a notice lists them.
pub const COMMANDS: [CommandSpec; 3] = [
    CommandSpec {
        name: "new",
        arguments: &[],
        action: CommandAction::NewSession,
    },
    CommandSpec {
        name: "session",
        arguments: &SESSION_ARGUMENT,
        action: CommandAction::OpenSession,
    },
    CommandSpec {
        name: "quit",
        arguments: &[],
        action: CommandAction::Quit,
    },
];

impl CommandSpec {
    /// Returns the name as a user writes it, with its leading slash.
    #[must_use]
    pub fn typed_name(self) -> String {
        format!("/{}", self.name)
    }

    /// Returns the transition a submitted command of this name runs.
    #[must_use]
    pub const fn action(self) -> CommandAction {
        self.action
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

/// One resolved command line: the registered command and its argument words.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandInvocation {
    /// The command the line named.
    spec: CommandSpec,
    /// The word the line gave each declared argument, in declaration order.
    words: Vec<String>,
}

impl CommandInvocation {
    /// Returns the registered command the line named.
    #[must_use]
    pub const fn spec(&self) -> CommandSpec {
        self.spec
    }

    /// Returns the word the declared argument at `index` was given, if any.
    #[must_use]
    pub fn word(&self, index: usize) -> Option<&str> {
        self.words.get(index).map(String::as_str)
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
    /// against: the unknown name lists the registry, and a missing argument
    /// names the argument.
    #[must_use]
    pub fn notice(&self) -> Option<String> {
        match self {
            Self::Invocation(_) => None,
            Self::UnknownName { name } => Some(format!(
                "unknown command /{name}; known commands: {}",
                known_commands()
            )),
            Self::MissingArgument { argument } => {
                Some(format!("missing argument \"{}\"", argument.name))
            }
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
/// declaration order. The parser reads only the registry, so a declaration
/// added there is parsed and explained here without another spelling of it
/// anywhere.
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
    let mut given = Vec::with_capacity(spec.arguments.len());
    for (index, argument) in spec.arguments.iter().enumerate() {
        match words.get(index) {
            None => {
                if argument.required {
                    return CommandResolution::MissingArgument {
                        argument: *argument,
                    };
                }
            }
            Some(word) => given.push((*word).to_owned()),
        }
    }
    if let Some(unexpected) = words.get(spec.arguments.len()) {
        return CommandResolution::UnexpectedArgument {
            value: (*unexpected).to_owned(),
        };
    }
    CommandResolution::Invocation(CommandInvocation { spec, words: given })
}

#[cfg(test)]
mod tests {

    use super::{
        ArgumentSpec, COMMANDS, CommandAction, CommandResolution, command_named, known_commands,
        resolve_arguments, resolve_command,
    };

    #[test]
    fn the_registry_names_every_command_once_with_a_named_argument() {
        assert_eq!(COMMANDS.len(), 3, "the line front ends know three commands");
        for command in COMMANDS {
            assert!(
                !command.name.starts_with('/'),
                "the registry stores the name"
            );
            for argument in command.arguments {
                assert!(
                    !argument.name.trim().is_empty(),
                    "/{} declares a named argument",
                    command.name
                );
            }
        }
        let mut names = COMMANDS.map(|command| command.name).to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COMMANDS.len(), "a name is registered once");
        assert_eq!(command_named("new").expect("new is registered").name, "new");
        assert_eq!(command_named("connectors"), None);
        assert_eq!(
            known_commands(),
            "/new /session /quit",
            "the one set both line front ends list"
        );
    }

    #[test]
    fn the_session_command_declares_one_required_argument() {
        let session = command_named("session").expect("session is registered");
        assert_eq!(session.action(), CommandAction::OpenSession);
        assert_eq!(session.arguments.len(), 1);
        let argument = session.arguments[0];
        assert_eq!(argument.name, "session");
        assert!(
            argument.required,
            "a line with no identifier names no session"
        );
    }

    #[test]
    fn the_commands_that_take_no_argument_declare_none() {
        for name in ["new", "quit"] {
            let command = command_named(name).expect("the command is registered");
            assert!(command.arguments.is_empty(), "/{name} takes no argument");
        }
        assert_eq!(
            command_named("quit").expect("quit is registered").action(),
            CommandAction::Quit
        );
        assert_eq!(
            command_named("new").expect("new is registered").action(),
            CommandAction::NewSession
        );
    }

    #[test]
    fn a_complete_line_resolves_to_its_invocation() {
        let invocation = resolve_command("session 4f0a2c11-0000-4000-8000-000000000000")
            .into_invocation()
            .expect("the line spells a command");
        assert_eq!(invocation.spec().name, "session");
        assert_eq!(
            invocation.word(0),
            Some("4f0a2c11-0000-4000-8000-000000000000"),
            "the free-form word reaches the transition unchanged"
        );
        assert_eq!(invocation.word(1), None, "the command declares one word");
        let empty = resolve_command("new")
            .into_invocation()
            .expect("new takes no argument");
        assert_eq!(empty.spec().name, "new");
        assert_eq!(empty.word(0), None);
    }

    #[test]
    fn a_line_naming_no_command_lists_the_registry() {
        let notice = resolve_command("watch")
            .notice()
            .expect("an unknown name answers with a notice");
        assert!(notice.contains("/watch"), "the notice names what was typed");
        assert!(
            notice.contains(&known_commands()),
            "the notice lists the registry it resolved against"
        );
    }

    #[test]
    fn a_missing_required_argument_names_the_argument() {
        let resolution = resolve_command("session");
        assert_eq!(
            resolution,
            CommandResolution::MissingArgument {
                argument: ArgumentSpec {
                    name: "session",
                    required: true,
                }
            }
        );
        assert_eq!(
            resolution.notice().as_deref(),
            Some("missing argument \"session\""),
        );
    }

    #[test]
    fn a_word_past_the_last_argument_is_rejected() {
        let resolution = resolve_arguments(COMMANDS[0], &["extra"]);
        assert_eq!(
            resolution,
            CommandResolution::UnexpectedArgument {
                value: "extra".to_owned()
            }
        );
        assert_eq!(
            resolution.notice().as_deref(),
            Some("unexpected argument \"extra\""),
        );
    }
}
