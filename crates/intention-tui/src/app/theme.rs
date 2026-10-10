//! The colour theme one terminal session renders its palette from, and the
//! picker transitions that select it.
//!
//! The theme is the terminal's own value: the light theme is the warm
//! off-white surface every session starts in, and the dark theme is the warm
//! charcoal surface for a terminal that paints on a dark background. The two
//! names are the whole vocabulary here; the palette module turns one of them
//! into the roles a frame paints, so no view reads the theme itself.
//!
//! The parser is deliberately total: it reads one name however it is spelled
//! and answers `None` for anything else, so a caller that maps an external
//! spelling onto a theme decides what an unknown name means without this type
//! inventing a fallback of its own.
//!
//! The committed theme is the daemon's value, never the terminal's guess: the
//! settings read and every selection round trip through the daemon, and only
//! the accepted reply moves it. A preview is the one local value - the picker's
//! live candidate - and it is never persisted.

use intention_proto::{ErrorDto, ThemeDto};

use super::{AppState, Effect, Screen};

/// The colour theme a terminal front end renders its palette from.
///
/// The default is [`Theme::Light`], the theme the terminal has always drawn:
/// a session that was never told otherwise keeps its warm off-white surface.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Theme {
    /// The warm off-white theme every session starts in.
    #[default]
    Light,
    /// The warm charcoal theme for a terminal that paints on a dark background.
    Dark,
}

impl Theme {
    /// Every theme, in the order the picker draws its rows.
    pub const ALL: [Self; 2] = [Self::Light, Self::Dark];

    /// Returns the theme's stable lowercase name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// Returns the theme's display name, as the picker's row spells it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    /// Returns the one line the picker's row and the `/theme` value show.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Light => "the warm off-white surface",
            Self::Dark => "the warm charcoal surface",
        }
    }

    /// Returns the theme one picker row above this one, stopping at the first.
    ///
    /// The picker walks its two rows without wrapping, exactly as the command
    /// band's highlight and the sessions browser's cursor walk theirs.
    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Light => Self::Light,
            Self::Dark => Self::Light,
        }
    }

    /// Returns the theme one picker row below this one, stopping at the last.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Dark,
        }
    }

    /// Returns the theme one name selects, however the name is spelled, or
    /// `None` for a name no theme answers to.
    ///
    /// The comparison is case-insensitive and ignores nothing else: the
    /// terminal's two names are the whole vocabulary, and an unknown name is
    /// the caller's to interpret rather than a value this type guesses at.
    #[must_use]
    #[allow(
        clippy::should_implement_trait,
        reason = "The parser is total and answers None for an unknown name, so it \
                  cannot be `std::str::FromStr`, whose error type would add a \
                  failure case the terminal's own vocabulary does not have."
    )]
    pub const fn from_str(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case(Self::Light.as_str()) {
            Some(Self::Light)
        } else if name.eq_ignore_ascii_case(Self::Dark.as_str()) {
            Some(Self::Dark)
        } else {
            None
        }
    }
}

impl From<ThemeDto> for Theme {
    /// Returns the theme one wire value names.
    fn from(theme: ThemeDto) -> Self {
        match theme {
            ThemeDto::Light => Self::Light,
            ThemeDto::Dark => Self::Dark,
        }
    }
}

impl From<Theme> for ThemeDto {
    /// Returns the wire value one theme is spelled as.
    fn from(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self::Light,
            Theme::Dark => Self::Dark,
        }
    }
}

impl AppState {
    /// Opens the theme picker on the committed theme.
    ///
    /// The picker is a screen of the same one window: it takes the region above
    /// the input block out of the transcript, so the input block and the detail
    /// line keep their rows. A preview from an earlier visit is dropped, so the
    /// picker always opens showing what the daemon accepted, and a notice a
    /// chat command left does not outlive the chat screen it belonged to.
    pub(super) fn request_theme_picker(&mut self) -> Vec<Effect> {
        self.screen = Screen::Theme;
        self.theme_preview = None;
        self.notice = None;
        Vec::new()
    }

    /// Applies the daemon's effective terminal settings.
    ///
    /// The daemon is the authority for the committed theme: whether this
    /// answers the startup read or a selection, the accepted value becomes the
    /// committed theme, and a fresh answer settles any local preview.
    pub(super) fn apply_settings_received(&mut self, theme: ThemeDto) -> Vec<Effect> {
        self.theme = Theme::from(theme);
        self.theme_preview = None;
        self.error = None;
        Vec::new()
    }

    /// Reports a settings read or theme selection the daemon rejected.
    ///
    /// A rejected selection never reaches the committed theme, and the
    /// candidate it previewed is dropped with the failure, so the screen
    /// returns to the theme the daemon still carries while the error line
    /// names why.
    pub(super) fn apply_settings_failed(&mut self, error: &ErrorDto) -> Vec<Effect> {
        self.theme_preview = None;
        self.apply_failure(error)
    }

    /// Selects one theme: the daemon is asked to persist it.
    ///
    /// The committed theme never moves optimistically - it changes only when
    /// the accepted reply arrives as [`super::Action::SettingsReceived`] - and
    /// the selection previews while the round trip is in flight, so the screen
    /// already shows the candidate. The picker closes on its own Enter: the
    /// selection is that screen's commit.
    pub(super) fn apply_theme_selected(&mut self, theme: Theme) -> Vec<Effect> {
        self.theme_preview = Some(theme);
        if self.screen == Screen::Theme {
            self.screen = Screen::Chat;
        }
        vec![Effect::PersistTheme(theme)]
    }

    /// Previews one theme while the picker's highlight sits on it.
    ///
    /// The preview is local and never persisted: it is what makes the whole
    /// window repaint through the candidate before the daemon has been asked.
    pub(super) const fn apply_theme_previewed(&mut self, theme: Theme) -> Vec<Effect> {
        self.theme_preview = Some(theme);
        Vec::new()
    }

    /// Drops the preview and leaves the picker.
    ///
    /// This is the picker's Esc: the committed theme is what the window
    /// repaints through once the preview is gone.
    pub(super) fn apply_theme_preview_cleared(&mut self) -> Vec<Effect> {
        self.theme_preview = None;
        if self.screen == Screen::Theme {
            self.screen = Screen::Chat;
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Theme;

    #[test]
    fn the_theme_parser_reads_both_names_however_they_are_spelled() {
        assert_eq!(Theme::from_str("light"), Some(Theme::Light));
        assert_eq!(Theme::from_str("Light"), Some(Theme::Light));
        assert_eq!(Theme::from_str("LIGHT"), Some(Theme::Light));
        assert_eq!(Theme::from_str("dark"), Some(Theme::Dark));
        assert_eq!(Theme::from_str("DaRk"), Some(Theme::Dark));
        assert_eq!(
            Theme::from_str("light "),
            None,
            "a name the vocabulary does not carry is not trimmed into one"
        );
        assert_eq!(Theme::from_str("dunkel"), None);
        assert_eq!(Theme::from_str(""), None);
    }

    #[test]
    fn every_theme_round_trips_through_its_own_name() {
        for theme in [Theme::Light, Theme::Dark] {
            assert_eq!(Theme::from_str(theme.as_str()), Some(theme));
        }
        assert_eq!(Theme::default(), Theme::Light, "the terminal starts light");
    }

    #[test]
    fn the_picker_rows_walk_without_wrapping() {
        assert_eq!(
            Theme::ALL,
            [Theme::Light, Theme::Dark],
            "the rows are the two themes in picker order"
        );
        assert_eq!(
            Theme::Light.previous(),
            Theme::Light,
            "the first row is the top"
        );
        assert_eq!(Theme::Light.next(), Theme::Dark);
        assert_eq!(Theme::Dark.previous(), Theme::Light);
        assert_eq!(
            Theme::Dark.next(),
            Theme::Dark,
            "the last row is the bottom"
        );
    }

    #[test]
    fn every_theme_carries_its_own_row_vocabulary() {
        for theme in Theme::ALL {
            assert!(!theme.label().trim().is_empty());
            assert!(
                !theme.label().chars().any(char::is_whitespace),
                "a label stays one word: {theme:?}"
            );
            assert!(
                !theme.description().trim().is_empty(),
                "a row describes the theme it offers: {theme:?}"
            );
        }
        assert_ne!(
            Theme::Light.description(),
            Theme::Dark.description(),
            "the two rows do not describe the same surface"
        );
    }
}
