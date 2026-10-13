//! The terminal front end's own theme: the local settings file and the
//! environment detection a launch falls back on.
//!
//! The theme is a presentation choice of this front end and nothing else: no
//! wire value carries it, the daemon never learns it, and no durable store
//! holds it. One launch resolves the preference in force once, highest
//! precedence first:
//!
//! 1. the explicit `--theme` flag, which is `auto`, `light`, or `dark`;
//! 2. the local file's `[tui] theme` key - the choice the user already saved in
//!    this front end, which beats every form of environment detection;
//! 3. `auto` when the local file declares no theme, or declares `auto`.
//!
//! The preference decides the palette: `light` and `dark` paint themselves
//! whatever the environment says, while `auto` paints what environment
//! detection answers - `COLORFGBG` when the terminal exports it, else the
//! terminal's own answer to the standard `OSC 11` background query, else
//! [`DEFAULT_THEME`].
//!
//! The operating system's own theme is not a source: the terminal probe answers
//! with the background the terminal paints, which is the environment this front
//! end runs in, and the project adds no platform theme dependency for a second
//! answer to the same question.
//!
//! The local file is [`THEME_FILE_NAME`] under the platform configuration
//! directory (`$XDG_CONFIG_HOME/intention-relay` or `$HOME/.config` on Linux,
//! `$HOME/Library/Application Support` on macOS, `%APPDATA%` on Windows). A
//! live selection is applied in memory and then rewritten there in one line, so
//! the next start reads the choice back; a rewrite of `auto` clears the
//! spelled-out palette and leaves the next launch to detection.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use intention_proto::{DtoResult, ErrorDto};
use revue::style::Color;

use crate::app::{Theme, ThemePreference};

/// The local settings file's name under the platform configuration directory.
const THEME_FILE_NAME: &str = "tui.toml";

/// The application's directory under the platform configuration directory.
const CONFIG_DIRECTORY_NAME: &str = "intention-relay";

/// The background query: OSC 11, "report the background colour", closed by the
/// string terminator.
const OSC11_QUERY: &[u8] = b"\x1b]11;?\x1b\\";

/// The bounded window one terminal has to answer the background query.
///
/// A terminal answers a query within a few milliseconds, and the probe runs
/// before the first frame, so the window is short enough that a terminal which
/// never answers costs one launch almost nothing: 15 ms is a liveness
/// safeguard, never a budget a compliant terminal can lose.
const PROBE_DEADLINE: Duration = Duration::from_millis(15);

/// The palette the resolution paints when the environment answers nothing.
///
/// The fallback is the dark palette because a terminal that reports neither
/// `COLORFGBG` nor a background colour is far more often a dark one: the
/// emulators that ship a default profile ship a dark one (macOS Terminal's
/// `Basic` is white on black, Windows Terminal's default `Campbell` paints a
/// black background, and GNOME Terminal and Konsole default to dark schemes),
/// and terminal applications default to their dark themes for the same reason.
/// A user whose terminal is light either exports `COLORFGBG`, answers the
/// background query, or spells the theme out in `tui.toml` or `--theme`.
const DEFAULT_THEME: Theme = Theme::Dark;

/// The largest reply the probe reads before giving up.
///
/// A background reply is far shorter than this; the bound keeps a terminal
/// that streams anything else from filling the probe's buffer.
const MAX_REPLY_BYTES: usize = 64;

/// The perceived brightness at or above which a background is a light one.
///
/// The rule reads the background the terminal reports the way a reader does:
/// the standard 299/587/114 perceived-brightness weights over the eight-bit
/// channels, split at the midpoint of the scale.
const LIGHT_BACKGROUND_MIN_BRIGHTNESS: u32 = 128;

/// The sixteen ANSI palette colours `COLORFGBG` names by index.
///
/// The xterm defaults: the first eight are the normal colours and the second
/// eight their bright variants.
static ANSI_16: [Color; 16] = [
    Color::rgb(0x00, 0x00, 0x00),
    Color::rgb(0xCD, 0x00, 0x00),
    Color::rgb(0x00, 0xCD, 0x00),
    Color::rgb(0xCD, 0xCD, 0x00),
    Color::rgb(0x00, 0x00, 0xEE),
    Color::rgb(0xCD, 0x00, 0xCD),
    Color::rgb(0x00, 0xCD, 0xCD),
    Color::rgb(0xE5, 0xE5, 0xE5),
    Color::rgb(0x7F, 0x7F, 0x7F),
    Color::rgb(0xFF, 0x00, 0x00),
    Color::rgb(0x00, 0xFF, 0x00),
    Color::rgb(0xFF, 0xFF, 0x00),
    Color::rgb(0x5C, 0x5C, 0xFF),
    Color::rgb(0xFF, 0x00, 0xFF),
    Color::rgb(0x00, 0xFF, 0xFF),
    Color::rgb(0xFF, 0xFF, 0xFF),
];

/// The theme one launch resolved: the preference in force and the palette the
/// environment detection answered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::tui) struct ResolvedTheme {
    preference: ThemePreference,
    detected: Theme,
}

impl ResolvedTheme {
    /// Returns the preference this launch resolved.
    ///
    /// The preference is what the front end stores: the one the flag spelled,
    /// or the one the local file declared, or `auto` when neither said
    /// anything.
    #[must_use]
    pub(in crate::tui) const fn preference(&self) -> ThemePreference {
        self.preference
    }

    /// Returns the palette the environment detection answered.
    ///
    /// The answer decides the palette for `auto` and is carried alongside every
    /// other preference, so the picker's `Auto` row previews what returning to
    /// detection would paint.
    #[must_use]
    pub(in crate::tui) const fn detected(&self) -> Theme {
        self.detected
    }
}

/// Resolves the theme of one launch.
///
/// `explicit` is the `--theme` value the caller spelled, if any: the flag wins
/// over every other source - including the local file, so `--theme auto` runs
/// detection for one launch without touching what the file stores - and the
/// sources below it are gathered in order, so a higher source is never asked to
/// share its answer with a lower one. Detection is read once per launch even
/// when the preference names a palette itself, because the picker's `Auto` row
/// previews what detection would paint.
///
/// # Errors
///
/// Returns the typed validation error of a local file whose declared theme is
/// not a declared preference, and the typed unavailable error of a local file
/// that cannot be read. Nothing silently falls back.
pub(in crate::tui) fn resolve(explicit: Option<ThemePreference>) -> DtoResult<ResolvedTheme> {
    resolve_with(
        explicit,
        || local_theme_path().map_or(Ok(None), |path| read_local_theme(&path)),
        detect,
    )
}

/// Resolves one launch's theme from already-gathered evidence.
///
/// The evidence is gathered lazily by the closure the caller supplies for each
/// source, and this function is the one place the precedence order lives: the
/// flag, else the local file, else `auto`.
///
/// # Errors
///
/// Returns the local file's typed error unchanged.
fn resolve_with(
    explicit: Option<ThemePreference>,
    local_file: impl FnOnce() -> DtoResult<Option<ThemePreference>>,
    detection: impl FnOnce() -> Theme,
) -> DtoResult<ResolvedTheme> {
    let declared = match explicit {
        Some(preference) => Some(preference),
        None => local_file()?,
    };
    Ok(ResolvedTheme {
        // A file that declares no theme asks for detection, exactly as a file
        // that spells `auto` does.
        preference: declared.unwrap_or_default(),
        detected: detection(),
    })
}

/// Returns the palette the environment answers.
///
/// The cheap read comes first: `COLORFGBG` is a variable the terminal already
/// exported, so a terminal that carries one costs no query at all. Only a
/// terminal that carries none is asked for its background colour through the
/// bounded probe, and a terminal that answers neither leaves [`DEFAULT_THEME`].
fn detect() -> Theme {
    detect_with(
        || {
            std::env::var("COLORFGBG")
                .ok()
                .as_deref()
                .and_then(colorfgbg_theme)
        },
        terminal_probe,
    )
}

/// Returns the palette one detection answers, variable first and probe second.
fn detect_with(
    variable: impl FnOnce() -> Option<Theme>,
    probe: impl FnOnce() -> Option<Theme>,
) -> Theme {
    variable().or_else(probe).unwrap_or(DEFAULT_THEME)
}

/// Returns the local theme file of this platform, when it has one.
#[must_use]
pub(in crate::tui) fn local_theme_path() -> Option<PathBuf> {
    platform_config_directory()
        .map(|directory| directory.join(CONFIG_DIRECTORY_NAME).join(THEME_FILE_NAME))
}

/// Returns the platform configuration directory this application uses.
///
/// The directories are the ones `intention-config` resolves the daemon's
/// `config.toml` under, so the TUI's own file lives beside it.
fn platform_config_directory() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let home = std::env::var_os("HOME").map(|home| Path::new(&home).join(".config"));
        xdg.filter(|directory| directory.is_absolute())
            .or_else(|| home.filter(|directory| directory.is_absolute()))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(|home| Path::new(&home).join("Library").join("Application Support"))
            .filter(|directory| directory.is_absolute())
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|directory| directory.is_absolute())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Reads the theme preference one local file declares.
///
/// # Errors
///
/// Returns an unavailable error when the file exists but cannot be read, and
/// the parse error of [`parse_local_theme`] when its content does not declare a
/// coherent preference.
fn read_local_theme(path: &Path) -> DtoResult<Option<ThemePreference>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(ErrorDto::unavailable(
                "tui_config_unreadable",
                "the local terminal settings file could not be read",
            ));
        }
    };
    parse_local_theme(&text)
}

/// Parses the local settings document into the preference it declares.
///
/// `None` means the document declares no preference: an empty file, an absent
/// `[tui]` table, or an absent `theme` key all ask for environment detection,
/// exactly as a declared `auto` does. Any other shape is rejected rather than
/// ignored, so a typo never silently selects another theme.
///
/// # Errors
///
/// Returns a validation error when the document is not one `[tui]` table with
/// one string `theme` key, and the preference vocabulary's own validation error
/// when the declared spelling is not a declared preference.
fn parse_local_theme(text: &str) -> DtoResult<Option<ThemePreference>> {
    let document: toml::Value = toml::from_str(text).map_err(|_| {
        ErrorDto::validation(
            "invalid_tui_config",
            "the local terminal settings TOML could not be parsed",
        )
    })?;
    let Some(table) = document.as_table() else {
        return Err(ErrorDto::validation(
            "invalid_tui_config",
            "the local terminal settings must be one [tui] table",
        ));
    };
    if table.keys().any(|key| key != "tui") {
        return Err(ErrorDto::validation(
            "invalid_tui_config",
            "the local terminal settings must be one [tui] table",
        ));
    }
    let Some(tui) = table.get("tui") else {
        return Ok(None);
    };
    let Some(tui) = tui.as_table() else {
        return Err(ErrorDto::validation(
            "invalid_tui_config",
            "the local terminal settings must be one [tui] table",
        ));
    };
    if tui.keys().any(|key| key != "theme") {
        return Err(ErrorDto::validation(
            "invalid_tui_config",
            "the [tui] table declares only the theme key",
        ));
    }
    let Some(theme) = tui.get("theme") else {
        return Ok(None);
    };
    let Some(theme) = theme.as_str() else {
        return Err(ErrorDto::validation(
            "invalid_tui_config",
            "the tui theme must be a string",
        ));
    };
    ThemePreference::parse(theme).map(Some)
}

/// Writes one theme preference into the local settings file.
///
/// The whole file is one `[tui]` table with one line, so a rewrite replaces the
/// previous choice exactly; a written `auto` is what clears the spelled-out
/// palette and asks the next launch for detection. The parent directory is
/// created when it does not exist yet.
///
/// # Errors
///
/// Returns an unavailable error when the directory or the file cannot be
/// written.
fn write_local_theme(path: &Path, preference: ThemePreference) -> DtoResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| write_failure())?;
    }
    std::fs::write(
        path,
        format!("[tui]\ntheme = \"{}\"\n", preference.as_str()),
    )
    .map_err(|_| write_failure())
}

/// Returns the typed failure of one local settings write.
fn write_failure() -> ErrorDto {
    ErrorDto::unavailable(
        "tui_config_write_failed",
        "the local terminal settings file could not be written",
    )
}

/// Rewrites the local settings file for one selection made in this front end.
///
/// # Errors
///
/// Returns an unavailable error when the platform reports no configuration
/// directory or the file cannot be written.
pub(in crate::tui) fn rewrite(preference: ThemePreference) -> DtoResult<()> {
    let path = local_theme_path().ok_or_else(|| {
        ErrorDto::unavailable(
            "tui_config_directory_unavailable",
            "a platform configuration directory could not be determined",
        )
    })?;
    write_local_theme(&path, preference)
}

/// Returns the theme one `COLORFGBG` value names.
///
/// The variable's last field is the background colour index - the earlier
/// fields are the foreground and, in some emulators, a literal `default` - so
/// the last field is what this reads. An absent, malformed, or out-of-range
/// value is no evidence, and the resolution continues below the variable.
fn colorfgbg_theme(value: &str) -> Option<Theme> {
    let background = value.trim().rsplit(';').next()?;
    let index = background.trim().parse::<usize>().ok()?;
    let color = ANSI_16.get(index)?;
    Some(theme_of_background(*color))
}

/// Returns the theme one background colour selects.
///
/// The rule is the one the terminal probe and `COLORFGBG` share: a background
/// at or above the perceived-brightness midpoint is a light one.
fn theme_of_background(color: Color) -> Theme {
    let brightness =
        (u32::from(color.r) * 299 + u32::from(color.g) * 587 + u32::from(color.b) * 114) / 1000;
    if brightness >= LIGHT_BACKGROUND_MIN_BRIGHTNESS {
        Theme::Light
    } else {
        Theme::Dark
    }
}

/// Probes the terminal for its background colour.
///
/// The probe is the zero-configuration mechanism: this front end asks the
/// terminal it runs in, and the terminal's own answer decides. It runs before
/// the fullscreen loop takes the terminal, and it reports nothing when standard
/// input or output is not a terminal or the terminal never answers.
#[cfg(unix)]
fn terminal_probe() -> Option<Theme> {
    use std::io::IsTerminal;

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return None;
    }
    if crossterm::terminal::enable_raw_mode().is_err() {
        return None;
    }
    let reply = probe_reply();
    // The fullscreen loop enables raw mode for itself; the probe leaves the
    // terminal as it found it.
    let _ = crossterm::terminal::disable_raw_mode();
    reply
        .as_deref()
        .and_then(parse_osc11_background)
        .map(theme_of_background)
}

/// Sends the background query and reads the terminal's answer in raw mode.
///
/// Raw mode is what makes the answer readable at all: a terminal's reply
/// carries no line break, so the cooked-mode line discipline would keep it
/// buffered. The wait is bounded by [`PROBE_DEADLINE`], and the probe reads no
/// further than [`MAX_REPLY_BYTES`].
#[cfg(unix)]
fn probe_reply() -> Option<Vec<u8>> {
    use std::io::Write;
    use std::os::fd::AsRawFd;

    let mut stdout = std::io::stdout();
    stdout.write_all(OSC11_QUERY).ok()?;
    stdout.flush().ok()?;
    let mut stdin = std::io::stdin().lock();
    let descriptor = stdin.as_raw_fd();
    let mut poll = mio::Poll::new().ok()?;
    let mut events = mio::Events::with_capacity(1);
    poll.registry()
        .register(
            &mut mio::unix::SourceFd(&descriptor),
            mio::Token(0),
            mio::Interest::READABLE,
        )
        .ok()?;
    let deadline = Instant::now() + PROBE_DEADLINE;
    let mut remaining = PROBE_DEADLINE;
    read_osc11_reply(&mut stdin, || {
        remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        poll.poll(&mut events, Some(remaining)).is_ok() && !events.is_empty()
    })
}

/// Probes the terminal for its background colour on a platform without the
/// query.
///
/// The Windows console delivers input through its own record API rather than a
/// byte stream a reply could be read from, so no probe runs there and the
/// resolution reaches its file or its default.
#[cfg(not(unix))]
fn terminal_probe() -> Option<Theme> {
    None
}

/// Reads one background reply with a caller-supplied readiness check.
///
/// `ready` reports whether one more read may proceed: on a terminal it is the
/// bounded readiness wait, and a reader that hands out its bytes at once needs
/// no wait at all. A response closed by the string terminator or by `BEL` is
/// complete; a read that ends, or a reply that grows past [`MAX_REPLY_BYTES`],
/// is no reply.
fn read_osc11_reply(mut reader: impl Read, mut ready: impl FnMut() -> bool) -> Option<Vec<u8>> {
    let mut reply = Vec::new();
    while reply.len() < MAX_REPLY_BYTES {
        if !ready() {
            return None;
        }
        let mut chunk = [0_u8; 32];
        let read = reader.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        reply.extend_from_slice(&chunk[..read]);
        if reply_is_complete(&reply) {
            return Some(reply);
        }
    }
    None
}

/// Returns whether one reply already carries its terminator.
fn reply_is_complete(reply: &[u8]) -> bool {
    reply.ends_with(b"\x1b\\") || reply.ends_with(b"\x07")
}

/// Parses the background colour out of one `OSC 11` reply.
///
/// The reply is `OSC 11 ; <colour>` where the colour is either `rgb:`
/// components or a `#rrggbb` literal; everything around it - the introducer and
/// the terminator - is framing. A reply of another query, or a malformed
/// colour, is no evidence.
fn parse_osc11_background(reply: &[u8]) -> Option<Color> {
    let text = std::str::from_utf8(reply).ok()?;
    let start = text.find("]11;")? + "]11;".len();
    let rest = &text[start..];
    let end = rest.find(['\u{7}', '\u{1b}']).unwrap_or(rest.len());
    parse_color(&rest[..end])
}

/// Parses one terminal colour literal into its RGB value.
///
/// Both spellings a terminal answers with are accepted: `#rrggbb`, and the
/// `rgb:` form whose three components carry one to four hexadecimal digits
/// each.
fn parse_color(spec: &str) -> Option<Color> {
    if let Some(hex) = spec.strip_prefix('#') {
        if hex.len() != 6 {
            return None;
        }
        return Some(Color::rgb(
            component(hex.get(0..2)?)?,
            component(hex.get(2..4)?)?,
            component(hex.get(4..6)?)?,
        ));
    }
    let components: Vec<&str> = spec.strip_prefix("rgb:")?.split('/').collect();
    if components.len() != 3 {
        return None;
    }
    Some(Color::rgb(
        component(components[0])?,
        component(components[1])?,
        component(components[2])?,
    ))
}

/// Parses one colour component of one to four hexadecimal digits.
///
/// A component is scaled by the width the terminal sent, so a four-digit
/// component's high byte and a two-digit component's byte both land on the
/// eight-bit scale the palette uses.
fn component(text: &str) -> Option<u8> {
    let digits = text.len();
    if digits == 0 || digits > 4 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(text, 16).ok()?;
    let maximum = (1_u32 << (4 * u32::try_from(digits).ok()?)) - 1;
    u8::try_from(value * 255 / maximum).ok()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::path::Path;
    use std::time::Duration;

    use revue::style::Color;
    use tempfile::TempDir;

    use crate::app::{Theme, ThemePreference};

    use super::{
        DEFAULT_THEME, MAX_REPLY_BYTES, PROBE_DEADLINE, THEME_FILE_NAME, colorfgbg_theme,
        detect_with, local_theme_path, parse_local_theme, parse_osc11_background, read_local_theme,
        read_osc11_reply, resolve_with, theme_of_background, write_local_theme,
    };

    #[test]
    fn the_local_file_path_names_the_theme_file_beside_the_configuration() {
        let path = local_theme_path().expect("the platform reports a configuration directory");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some(THEME_FILE_NAME),
            "the local settings live in one named file"
        );
        assert!(
            path.parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "intention-relay"),
            "the local settings sit in the application's configuration directory"
        );
        assert!(path.is_absolute(), "the local path is absolute");
    }

    #[test]
    fn an_absent_or_theme_less_document_declares_nothing() {
        for text in ["", "\n", "[tui]\n", "# a comment\n", "[tui]\n# a comment\n"] {
            assert_eq!(
                parse_local_theme(text).expect("a document with no theme parses"),
                None,
                "{text:?} declares no preference"
            );
        }
    }

    #[test]
    fn one_tui_table_declares_its_preference() {
        for (document, expected) in [
            ("[tui]\ntheme = \"auto\"\n", ThemePreference::Auto),
            ("[tui]\ntheme = \"light\"\n", ThemePreference::Light),
            ("[tui]\ntheme = \"dark\"\n", ThemePreference::Dark),
        ] {
            assert_eq!(
                parse_local_theme(document).expect("the document parses"),
                Some(expected),
                "the file stores every declared preference"
            );
        }
    }

    #[test]
    fn a_document_of_another_shape_or_spelling_is_rejected() {
        for text in [
            "theme = \"dark\"\n",
            "[tui]\nmode = \"plain\"\n",
            "[other]\ntheme = \"dark\"\n",
            "[tui]\ntheme = 3\n",
            "not a toml document [",
            "[tui]\ntheme = \"midnight\"\n",
        ] {
            let error = parse_local_theme(text).expect_err("the document is rejected");
            let expected = if text.contains("midnight") {
                "invalid_tui_theme"
            } else {
                "invalid_tui_config"
            };
            assert_eq!(error.code(), expected, "{text:?} is rejected as {expected}");
        }
    }

    #[test]
    fn the_local_file_round_trips_one_selection_and_an_absent_file_is_none() {
        let directory = TempDir::new().expect("temporary directory exists");
        let path = directory.path().join(THEME_FILE_NAME);
        assert_eq!(
            read_local_theme(&path).expect("an absent file reads"),
            None,
            "a file that does not exist declares nothing"
        );
        for preference in ThemePreference::ALL {
            write_local_theme(&path, preference).expect("the selection writes");
            assert_eq!(
                read_local_theme(&path).expect("the written file reads"),
                Some(preference),
                "the written selection reads back"
            );
        }
        write_local_theme(&path, ThemePreference::Auto).expect("the selection writes");
        let rewritten = std::fs::read_to_string(&path).expect("the file is readable");
        assert_eq!(
            rewritten, "[tui]\ntheme = \"auto\"\n",
            "the file holds one table and one line, and a stored auto clears the spelled palette"
        );
    }

    #[test]
    fn colorfgbg_reads_its_last_field_as_the_background() {
        assert_eq!(colorfgbg_theme("15;0"), Some(Theme::Dark));
        assert_eq!(colorfgbg_theme("0;15"), Some(Theme::Light));
        assert_eq!(colorfgbg_theme("0;default;7"), Some(Theme::Light));
        assert_eq!(colorfgbg_theme("15;default;0"), Some(Theme::Dark));
        assert_eq!(colorfgbg_theme("15;8"), Some(Theme::Dark));
        assert_eq!(
            colorfgbg_theme("0"),
            Some(Theme::Dark),
            "a single field is the background it names"
        );
        for value in ["", "default", "0;", "0;16", "0;purple"] {
            assert_eq!(colorfgbg_theme(value), None, "{value:?} is no evidence");
        }
    }

    #[test]
    fn the_backgrounds_brightness_selects_the_theme() {
        assert_eq!(
            theme_of_background(Color::rgb(0x0A, 0x0A, 0x0C)),
            Theme::Dark
        );
        assert_eq!(
            theme_of_background(Color::rgb(0xFF, 0xFF, 0xFF)),
            Theme::Light
        );
        assert_eq!(
            theme_of_background(Color::rgb(0xF7, 0xF2, 0xE9)),
            Theme::Light
        );
        assert_eq!(
            theme_of_background(Color::rgb(0x7F, 0x7F, 0x7F)),
            Theme::Dark
        );
        assert_eq!(
            theme_of_background(Color::rgb(0x80, 0x80, 0x80)),
            Theme::Light
        );
    }

    #[test]
    fn one_background_reply_parses_in_every_spelling_terminals_send() {
        for (reply, expected) in [
            (
                b"\x1b]11;rgb:0000/0000/0000\x1b\\".as_slice(),
                Color::rgb(0x00, 0x00, 0x00),
            ),
            (
                b"\x1b]11;rgb:ffff/ffff/ffff\x07".as_slice(),
                Color::rgb(0xFF, 0xFF, 0xFF),
            ),
            (
                b"\x1b]11;#1e1e1e\x1b\\".as_slice(),
                Color::rgb(0x1E, 0x1E, 0x1E),
            ),
            (
                b"\x1b]11;rgb:0a/0a/0c\x1b\\".as_slice(),
                Color::rgb(0x0A, 0x0A, 0x0C),
            ),
        ] {
            assert_eq!(
                parse_osc11_background(reply),
                Some(expected),
                "{reply:?} names its background"
            );
        }
    }

    #[test]
    fn another_query_and_a_malformed_colour_are_no_evidence() {
        for reply in [
            b"\x1b]10;rgb:0000/0000/0000\x1b\\".as_slice(),
            b"\x1b]11;rgb:0000/0000\x1b\\".as_slice(),
            b"\x1b]11;#12345\x1b\\".as_slice(),
            b"\x1b]11;rgb:zz/00/00\x1b\\".as_slice(),
            b"\x1b]11;\x1b\\".as_slice(),
            b"no reply at all".as_slice(),
            b"\xff\xfe".as_slice(),
        ] {
            assert_eq!(
                parse_osc11_background(reply),
                None,
                "{reply:?} answers nothing"
            );
        }
    }

    #[test]
    fn a_reader_delivers_the_reply_up_to_its_terminator() {
        let reply = b"\x1b]11;rgb:0000/0000/0000\x1b\\";
        let read = read_osc11_reply(Cursor::new(reply), || true).expect("the reply reads");
        assert_eq!(read, reply, "the whole reply reaches the parser");
    }

    #[test]
    fn a_reader_with_nothing_to_deliver_reads_no_reply() {
        assert_eq!(
            read_osc11_reply(Cursor::new(Vec::<u8>::new()), || true),
            None,
            "an ended stream is no reply"
        );
        assert_eq!(
            read_osc11_reply(Cursor::new(vec![b'x'; MAX_REPLY_BYTES + 1]), || true),
            None,
            "a stream that never terminates is abandoned at the bound"
        );
        assert_eq!(
            read_osc11_reply(Cursor::new(b"\x1b]11;#000000\x1b\\".to_vec()), || false),
            None,
            "a reader that is never ready reads nothing"
        );
    }

    #[test]
    fn the_probe_waits_fifteen_milliseconds() {
        assert_eq!(
            PROBE_DEADLINE,
            Duration::from_millis(15),
            "the probe's window is short enough to sit in the launch path"
        );
    }

    #[test]
    fn detection_reads_the_variable_first_then_the_probe_then_the_dark_fallback() {
        assert_eq!(
            detect_with(|| Some(Theme::Light), || panic!("the probe is not asked")),
            Theme::Light,
            "COLORFGBG costs no query"
        );
        assert_eq!(
            detect_with(|| None, || Some(Theme::Light)),
            Theme::Light,
            "the terminal's own answer decides when no variable exists"
        );
        assert_eq!(
            detect_with(|| None, || None),
            Theme::Dark,
            "a terminal that answers nothing leaves the dark default"
        );
        assert_eq!(DEFAULT_THEME, Theme::Dark);
    }

    #[test]
    fn the_flag_outranks_the_file_and_the_file_outranks_detection() {
        let file = |declared: Option<ThemePreference>| move || Ok(declared);
        let detection = |theme: Theme| move || theme;

        // The flag wins, and the local file is never read for it.
        let resolved = resolve_with(
            Some(ThemePreference::Light),
            || panic!("the flag does not read the file"),
            detection(Theme::Dark),
        )
        .expect("the flag decides");
        assert_eq!(resolved.preference(), ThemePreference::Light);
        assert_eq!(resolved.detected(), Theme::Dark);
        assert_eq!(
            resolved.preference().theme(resolved.detected()),
            Theme::Light,
            "the spelled flag paints its own palette whatever the environment answered"
        );

        // `--theme auto` runs detection for one launch and ignores the file.
        let resolved = resolve_with(
            Some(ThemePreference::Auto),
            || panic!("the flag does not read the file"),
            detection(Theme::Light),
        )
        .expect("the flag decides");
        assert_eq!(resolved.preference(), ThemePreference::Auto);
        assert_eq!(
            resolved.preference().theme(resolved.detected()),
            Theme::Light,
            "the flag asks detection for this launch"
        );

        // The file's explicit choice beats the environment answer.
        let resolved = resolve_with(
            None,
            file(Some(ThemePreference::Dark)),
            detection(Theme::Light),
        )
        .expect("the file decides");
        assert_eq!(resolved.preference(), ThemePreference::Dark);
        assert_eq!(
            resolved.detected(),
            Theme::Light,
            "the detection answer is still gathered for the picker's auto row"
        );
        assert_eq!(
            resolved.preference().theme(resolved.detected()),
            Theme::Dark,
            "the file's dark palette beats the light the environment answered"
        );
    }

    #[test]
    fn an_auto_or_absent_file_paints_what_detection_answered() {
        let file = |declared: Option<ThemePreference>| move || Ok(declared);
        for declared in [None, Some(ThemePreference::Auto)] {
            for detected in Theme::ALL {
                let resolved = resolve_with(None, file(declared), move || detected)
                    .expect("a file that declares no palette asks for detection");
                assert_eq!(
                    resolved.preference(),
                    ThemePreference::Auto,
                    "{declared:?} asks for detection"
                );
                assert_eq!(
                    resolved.preference().theme(resolved.detected()),
                    detected,
                    "detection paints {detected:?} for {declared:?}"
                );
            }
        }
    }

    #[test]
    fn a_local_file_of_an_unknown_spelling_fails_the_resolution() {
        let error = resolve_with(
            None,
            || parse_local_theme("[tui]\ntheme = \"midnight\"\n"),
            || panic!("the file fails before detection is read"),
        )
        .expect_err("the local file's failure reaches the caller");
        assert_eq!(error.code(), "invalid_tui_theme");
    }
}
