//! The colour theme one terminal session renders its palette from.
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
    /// Returns the theme's stable lowercase name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
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
}
