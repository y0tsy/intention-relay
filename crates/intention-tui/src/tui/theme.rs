//! The terminal front end's own theme: the local settings file, the
//! environment, and the terminal background probe.
//!
//! The theme is a presentation choice of this front end and nothing else: no
//! wire value carries it, the daemon never learns it, and no durable store
//! holds it. One launch resolves the theme once, highest precedence first:
//!
//! 1. the explicit `--theme` flag;
//! 2. `COLORFGBG`, the terminal background index many emulators export;
//! 3. the local file's `[tui] theme` key;
//! 4. the terminal's answer to the standard `OSC 11` background query;
//! 5. the light theme, when nothing answers.
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
//! the next start reads the choice back.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use intention_proto::{DtoResult, ErrorDto};
use revue::style::Color;

use crate::app::Theme;

/// The local settings file's name under the platform configuration directory.
const THEME_FILE_NAME: &str = "tui.toml";

/// The application's directory under the platform configuration directory.
const CONFIG_DIRECTORY_NAME: &str = "intention-relay";

/// The background query: OSC 11, "report the background colour", closed by the
/// string terminator.
const OSC11_QUERY: &[u8] = b"\x1b]11;?\x1b\\";

/// The bounded window one terminal has to answer the background query.
///
/// A terminal answers a query within a few milliseconds; the bound is a
/// liveness safeguard against one that never answers, never a budget a
/// compliant terminal can lose.
const PROBE_DEADLINE: Duration = Duration::from_millis(100);

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

/// Resolves the theme of one launch.
///
/// `explicit` is the `--theme` value the caller spelled, if any: the flag wins
/// over every other source, and the sources below it are gathered in order, so
/// a higher source is never asked to share its answer with a lower one.
///
/// # Errors
///
/// Returns the typed validation error of a local file whose declared theme is
/// not a declared spelling, and the typed unavailable error of a local file
/// that cannot be read. Nothing silently falls back.
pub(in crate::tui) fn resolve(explicit: Option<Theme>) -> DtoResult<Theme> {
    resolve_with(
        explicit,
        || {
            std::env::var("COLORFGBG")
                .ok()
                .as_deref()
                .and_then(colorfgbg_theme)
        },
        || local_theme_path().map_or(Ok(None), |path| read_local_theme(&path)),
        terminal_probe,
    )
}

/// Resolves one theme from already-gathered evidence, in precedence order.
///
/// The evidence is gathered lazily by the closure the caller supplies for each
/// level, so this function is the one place the precedence order lives.
///
/// # Errors
///
/// Returns the local file's typed error unchanged.
fn resolve_with(
    explicit: Option<Theme>,
    terminal_variable: impl FnOnce() -> Option<Theme>,
    local_file: impl FnOnce() -> DtoResult<Option<Theme>>,
    terminal_probe: impl FnOnce() -> Option<Theme>,
) -> DtoResult<Theme> {
    if let Some(theme) = explicit {
        return Ok(theme);
    }
    if let Some(theme) = terminal_variable() {
        return Ok(theme);
    }
    if let Some(theme) = local_file()? {
        return Ok(theme);
    }
    Ok(terminal_probe().unwrap_or(Theme::Light))
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

/// Reads the theme one local file declares.
///
/// # Errors
///
/// Returns an unavailable error when the file exists but cannot be read, and
/// the parse error of [`parse_local_theme`] when its content does not declare a
/// coherent theme.
fn read_local_theme(path: &Path) -> DtoResult<Option<Theme>> {
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

/// Parses the local settings document into the theme it declares.
///
/// `None` means the document declares no theme: an empty file, an absent
/// `[tui]` table, or an absent `theme` key leave the resolution to the sources
/// below the file. Any other shape is rejected rather than ignored, so a typo
/// never silently selects another theme.
///
/// # Errors
///
/// Returns a validation error when the document is not one `[tui]` table with
/// one string `theme` key, and the theme vocabulary's own validation error when
/// the declared spelling is not a declared theme.
fn parse_local_theme(text: &str) -> DtoResult<Option<Theme>> {
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
    Theme::parse(theme).map(Some)
}

/// Writes one theme into the local settings file.
///
/// The whole file is one `[tui]` table with one line, so a rewrite replaces the
/// previous choice exactly. The parent directory is created when it does not
/// exist yet.
///
/// # Errors
///
/// Returns an unavailable error when the directory or the file cannot be
/// written.
fn write_local_theme(path: &Path, theme: Theme) -> DtoResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| write_failure())?;
    }
    std::fs::write(path, format!("[tui]\ntheme = \"{}\"\n", theme.as_str()))
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
pub(in crate::tui) fn rewrite(theme: Theme) -> DtoResult<()> {
    let path = local_theme_path().ok_or_else(|| {
        ErrorDto::unavailable(
            "tui_config_directory_unavailable",
            "a platform configuration directory could not be determined",
        )
    })?;
    write_local_theme(&path, theme)
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

    use revue::style::Color;
    use tempfile::TempDir;

    use crate::app::Theme;

    use super::{
        MAX_REPLY_BYTES, THEME_FILE_NAME, colorfgbg_theme, local_theme_path, parse_local_theme,
        parse_osc11_background, read_local_theme, read_osc11_reply, resolve_with,
        theme_of_background, write_local_theme,
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
                "{text:?} declares no theme"
            );
        }
    }

    #[test]
    fn one_tui_table_declares_its_theme() {
        assert_eq!(
            parse_local_theme("[tui]\ntheme = \"dark\"\n").expect("the document parses"),
            Some(Theme::Dark)
        );
        assert_eq!(
            parse_local_theme("[tui]\ntheme = \"light\"\n").expect("the document parses"),
            Some(Theme::Light)
        );
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
        for theme in Theme::ALL {
            write_local_theme(&path, theme).expect("the selection writes");
            assert_eq!(
                read_local_theme(&path).expect("the written file reads"),
                Some(theme),
                "the written selection reads back"
            );
        }
        let rewritten = std::fs::read_to_string(&path).expect("the file is readable");
        assert_eq!(
            rewritten, "[tui]\ntheme = \"dark\"\n",
            "the file holds one table and one line, so a rewrite replaces the choice"
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
    fn the_precedence_runs_from_the_flag_to_the_default() {
        let theme = |value: Theme| {
            move || {
                let _ = value;
                Some(value)
            }
        };
        let file = |value: Option<Theme>| move || Ok(value);
        let terminal = || Some(Theme::Dark);
        assert_eq!(
            resolve_with(
                Some(Theme::Dark),
                || panic!("no variable"),
                || panic!("no file"),
                || panic!("no probe")
            )
            .expect("the flag decides"),
            Theme::Dark,
            "the flag is never asked to share its answer"
        );
        assert_eq!(
            resolve_with(
                None,
                theme(Theme::Light),
                || panic!("no file"),
                || panic!("no probe")
            )
            .expect("the variable decides"),
            Theme::Light,
            "COLORFGBG beats the file and the probe"
        );
        assert_eq!(
            resolve_with(
                None,
                || None,
                file(Some(Theme::Dark)),
                || panic!("no probe")
            )
            .expect("the file decides"),
            Theme::Dark,
            "the file beats the probe"
        );
        assert_eq!(
            resolve_with(None, || None, file(None), terminal).expect("the probe decides"),
            Theme::Dark,
            "the probe decides when the terminal answers"
        );
        assert_eq!(
            resolve_with(None, || None, file(None), || None).expect("the default decides"),
            Theme::Light,
            "the light theme is the default"
        );
    }

    #[test]
    fn a_local_file_of_an_unknown_spelling_fails_the_resolution() {
        let error = resolve_with(
            None,
            || None,
            || parse_local_theme("[tui]\ntheme = \"midnight\"\n"),
            || panic!("no probe"),
        )
        .expect_err("the local file's failure reaches the caller");
        assert_eq!(error.code(), "invalid_tui_theme");
    }
}
