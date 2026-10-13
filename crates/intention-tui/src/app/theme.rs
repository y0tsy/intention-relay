//! The colour theme one terminal session renders its palette from, and the
//! picker transitions that select it.
//!
//! The theme is the terminal's own value: the light theme is the warm
//! off-white surface every session starts in, and the dark theme is the
//! neutral graphite surface for a terminal that paints on a dark background.
//! The two names are the whole vocabulary here; the palette module turns one
//! of them into the roles a frame paints, so no view reads the theme itself.
//!
//! [`Theme::ALL`] is the whole vocabulary: the registry derives `/theme`'s
//! declared values from it, so a name this type does not carry is not a name
//! the command surface can declare, and a declared value always applies its
//! theme. [`Theme::parse`] reads the durable `light`/`dark` spelling of the
//! front end's local settings file.
//!
//! The committed theme is this front end's value: a selection applies it in
//! memory at once and asks for its local rewrite, and only the picker's live
//! preview sits beside it, never persisted.

use intention_proto::{DtoResult, ErrorDto};

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
    /// The neutral graphite theme for a terminal that paints on a dark
    /// background.
    Dark,
}

impl Theme {
    /// Every theme, in the order the picker draws its rows.
    pub const ALL: [Self; 2] = [Self::Light, Self::Dark];

    /// Returns the durable spelling of this theme.
    ///
    /// The spelling is what the local settings file stores and what the
    /// `/theme` command declares, so the file, the command surface, and the
    /// picker can never disagree about what the two themes are called.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// Parses the durable spelling of one theme.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `value` is not a declared spelling.
    pub fn parse(value: &str) -> DtoResult<Self> {
        match value {
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => Err(ErrorDto::validation(
                "invalid_tui_theme",
                "tui theme must be \"light\" or \"dark\"",
            )),
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
            Self::Dark => "the neutral graphite surface",
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
}

impl AppState {
    /// Opens the theme picker on the committed theme.
    ///
    /// The picker is a screen of the same one window: it takes the region above
    /// the input block out of the transcript, so the input block and the detail
    /// line keep their rows. A preview from an earlier visit is dropped, so the
    /// picker always opens showing the committed theme, and a notice a chat
    /// command left does not outlive the chat screen it belonged to.
    pub(super) fn request_theme_picker(&mut self) -> Vec<Effect> {
        self.screen = Screen::Theme;
        self.theme_preview = None;
        self.notice = None;
        Vec::new()
    }

    /// Reports a local rewrite of a selected theme that failed.
    ///
    /// The selection itself already moved the committed theme - the local file
    /// is what carries it to the next start - so the failure becomes the error
    /// line and never a rollback of the theme on screen.
    pub(super) fn apply_theme_persist_failed(&mut self, error: &ErrorDto) -> Vec<Effect> {
        self.apply_failure(error)
    }

    /// Selects one theme: it applies at once and its local file is rewritten.
    ///
    /// The theme is this front end's own value, so the committed theme moves
    /// with the selection instead of waiting for a round trip; the effect asks
    /// the front end to rewrite the local settings file, so the choice survives
    /// the next start. The picker closes on its own Enter: the selection is
    /// that screen's commit. Selecting the theme already committed asks for
    /// nothing, because there is nothing to write.
    pub(super) fn apply_theme_selected(&mut self, theme: Theme) -> Vec<Effect> {
        self.theme_preview = None;
        if self.screen == Screen::Theme {
            self.screen = Screen::Chat;
        }
        if self.theme == theme {
            return Vec::new();
        }
        self.theme = theme;
        vec![Effect::PersistTheme(theme)]
    }

    /// Previews one theme while the picker's highlight sits on it.
    ///
    /// The preview is local and never persisted: it is what makes the whole
    /// window repaint through a candidate before the picker's Enter commits it.
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

    #[test]
    fn every_theme_parses_from_its_own_durable_spelling() {
        for theme in Theme::ALL {
            assert_eq!(
                Theme::parse(theme.as_str()).expect("a declared spelling parses"),
                theme,
                "the durable spelling names its theme"
            );
        }
        for value in ["", "Light", "midnight", " dark"] {
            let error = Theme::parse(value).expect_err("an undeclared spelling fails");
            assert_eq!(error.code(), "invalid_tui_theme", "{value:?} is rejected");
        }
    }
}
