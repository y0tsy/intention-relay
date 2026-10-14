//! The colour themes one terminal session renders its palette from, the
//! preference that selects one, and the picker transitions that move it.
//!
//! Two vocabularies describe the terminal's colour here. [`Theme`] is the
//! palette a frame paints: the light theme is the warm off-white surface, and
//! the dark theme is the neutral graphite surface for a terminal that paints on
//! a dark background. [`ThemePreference`] is what the user asked for - one of
//! those palettes, or [`ThemePreference::Auto`], which leaves the palette to
//! the launch's own environment detection.
//!
//! [`ThemePreference::ALL`] is the whole preference vocabulary: the registry
//! derives `/theme`'s declared values from it and the picker draws one row per
//! entry, so a name this type does not carry is not a name either surface can
//! declare, and a declared value always applies its preference.
//! [`ThemePreference::parse`] reads the durable `auto`/`light`/`dark` spelling
//! the front end's local settings file stores.
//!
//! The committed preference is this front end's value: a selection applies it
//! in memory at once and asks for its local rewrite, and only the picker's live
//! preview sits beside it, never persisted.

use intention_proto::{DtoResult, ErrorDto};

use super::{AppState, Effect, Screen};

/// The colour palette a terminal front end renders with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Theme {
    /// The warm off-white palette.
    Light,
    /// The neutral graphite palette.
    Dark,
}

impl Theme {
    /// Every palette, in the order the vocabulary is declared.
    pub const ALL: [Self; 2] = [Self::Light, Self::Dark];
}

/// The theme a user asked for: one palette, or detection.
///
/// The default is [`ThemePreference::Auto`]: a launch that stores no preference
/// keeps asking the environment, so a terminal that changes with the desktop
/// carries the front end with it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ThemePreference {
    /// Leave the palette to the launch's own environment detection.
    #[default]
    Auto,
    /// Paint the light palette whatever the environment answers.
    Light,
    /// Paint the dark palette whatever the environment answers.
    Dark,
}

impl ThemePreference {
    /// Every preference, in the order the picker draws its rows.
    pub const ALL: [Self; 3] = [Self::Auto, Self::Light, Self::Dark];

    /// Returns the durable spelling of this preference.
    ///
    /// The spelling is what the local settings file stores and what the
    /// `/theme` command declares, so the file, the command surface, and the
    /// picker can never disagree about what the three preferences are called.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// Parses the durable spelling of one preference.
    ///
    /// # Errors
    ///
    /// Returns a validation error when `value` is not a declared spelling.
    pub fn parse(value: &str) -> DtoResult<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => Err(ErrorDto::validation(
                "invalid_tui_theme",
                "tui theme must be \"auto\", \"light\", or \"dark\"",
            )),
        }
    }

    /// Returns the preference's display name, as the picker's row spells it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    /// Returns the one line the picker's row and the `/theme` value show.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Auto => "follow the terminal's own background",
            Self::Light => "the warm off-white surface",
            Self::Dark => "the neutral graphite surface",
        }
    }

    /// Returns the preference one picker row above this one, stopping at the
    /// first.
    ///
    /// The picker walks its three rows without wrapping, exactly as the command
    /// band's highlight and the sessions browser's cursor walk theirs.
    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::Auto => Self::Auto,
            Self::Light => Self::Auto,
            Self::Dark => Self::Light,
        }
    }

    /// Returns the preference one picker row below this one, stopping at the
    /// last.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Auto => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::Dark,
        }
    }

    /// Returns the palette this preference paints.
    ///
    /// `detected` is what the launch's environment detection answered; it
    /// decides for [`ThemePreference::Auto`] and is ignored by a preference
    /// that names a palette itself.
    #[must_use]
    pub const fn theme(self, detected: Theme) -> Theme {
        match self {
            Self::Auto => detected,
            Self::Light => Theme::Light,
            Self::Dark => Theme::Dark,
        }
    }
}

impl AppState {
    /// Opens the theme picker on the committed preference.
    ///
    /// The picker is a screen of the same one window: it takes the region above
    /// the input block out of the transcript, so the input block and the detail
    /// line keep their rows. A preview from an earlier visit is dropped, so the
    /// picker always opens showing the committed preference, and a notice a
    /// chat command left does not outlive the chat screen it belonged to.
    pub(super) fn request_theme_picker(&mut self) -> Vec<Effect> {
        self.screen = Screen::Theme;
        self.theme_preview = None;
        self.notice = None;
        Vec::new()
    }

    /// Reports a local rewrite of a selected theme that failed.
    ///
    /// The selection itself already moved the committed preference - the local
    /// file is what carries it to the next start - so the failure becomes the
    /// error line and never a rollback of the theme on screen.
    pub(super) fn apply_theme_persist_failed(&mut self, error: &ErrorDto) -> Vec<Effect> {
        self.apply_failure(error)
    }

    /// Selects one theme preference: it applies at once and its local file is
    /// rewritten.
    ///
    /// The preference is this front end's own value, so the committed theme
    /// moves with the selection instead of waiting for a round trip; the effect
    /// asks the front end to rewrite the local settings file, so the choice
    /// survives the next start. Selecting [`ThemePreference::Auto`] stores the
    /// request to follow detection and clears any spelled-out palette. The
    /// picker closes on its own Enter: the selection is that screen's commit.
    /// Selecting the preference already committed asks for nothing, because
    /// there is nothing to write.
    pub(super) fn apply_theme_selected(&mut self, preference: ThemePreference) -> Vec<Effect> {
        self.theme_preview = None;
        if self.screen == Screen::Theme {
            self.screen = Screen::Chat;
        }
        if self.theme_preference == preference {
            return Vec::new();
        }
        self.theme_preference = preference;
        self.theme = preference.theme(self.detected_theme);
        vec![Effect::PersistTheme(preference)]
    }

    /// Previews one preference while the picker's highlight sits on it.
    ///
    /// The preview is local and never persisted: it is what makes the whole
    /// window repaint through a candidate before the picker's Enter commits it.
    /// [`ThemePreference::Auto`] previews as the palette detection answered, so
    /// the whole-window repaint shows what returning to detection would paint.
    pub(super) const fn apply_theme_previewed(
        &mut self,
        preference: ThemePreference,
    ) -> Vec<Effect> {
        self.theme_preview = Some(preference);
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
    use super::{Theme, ThemePreference};

    #[test]
    fn the_picker_rows_walk_without_wrapping() {
        assert_eq!(
            ThemePreference::ALL,
            [
                ThemePreference::Auto,
                ThemePreference::Light,
                ThemePreference::Dark
            ],
            "the rows are the three preferences in picker order"
        );
        assert_eq!(
            ThemePreference::Auto.previous(),
            ThemePreference::Auto,
            "the first row is the top"
        );
        assert_eq!(ThemePreference::Auto.next(), ThemePreference::Light);
        assert_eq!(ThemePreference::Light.previous(), ThemePreference::Auto);
        assert_eq!(ThemePreference::Light.next(), ThemePreference::Dark);
        assert_eq!(ThemePreference::Dark.previous(), ThemePreference::Light);
        assert_eq!(
            ThemePreference::Dark.next(),
            ThemePreference::Dark,
            "the last row is the bottom"
        );
    }

    #[test]
    fn every_preference_carries_its_own_row_vocabulary() {
        for preference in ThemePreference::ALL {
            assert!(!preference.label().trim().is_empty());
            assert!(
                !preference.label().chars().any(char::is_whitespace),
                "a label stays one word: {preference:?}"
            );
            assert!(
                !preference.description().trim().is_empty(),
                "a row describes the preference it offers: {preference:?}"
            );
        }
        assert_ne!(
            ThemePreference::Light.description(),
            ThemePreference::Dark.description(),
            "the two palette rows do not describe the same surface"
        );
        assert_ne!(
            ThemePreference::Auto.description(),
            ThemePreference::Light.description(),
            "the detection row does not describe a palette of its own"
        );
    }

    #[test]
    fn every_preference_parses_from_its_own_durable_spelling() {
        for preference in ThemePreference::ALL {
            assert_eq!(
                ThemePreference::parse(preference.as_str()).expect("a declared spelling parses"),
                preference,
                "the durable spelling names its preference"
            );
        }
        for value in ["", "Light", "midnight", " dark"] {
            let error = ThemePreference::parse(value).expect_err("an undeclared spelling fails");
            assert_eq!(error.code(), "invalid_tui_theme", "{value:?} is rejected");
        }
    }

    #[test]
    fn auto_paints_what_detection_answered_and_a_spelled_palette_paints_its_own() {
        assert_eq!(
            ThemePreference::Auto.theme(Theme::Light),
            Theme::Light,
            "detection decides for auto"
        );
        assert_eq!(
            ThemePreference::Auto.theme(Theme::Dark),
            Theme::Dark,
            "detection decides for auto whatever it answers"
        );
        for detected in Theme::ALL {
            assert_eq!(
                ThemePreference::Light.theme(detected),
                Theme::Light,
                "a spelled light palette never follows detection"
            );
            assert_eq!(
                ThemePreference::Dark.theme(detected),
                Theme::Dark,
                "a spelled dark palette never follows detection"
            );
        }
    }
}
