//! The revue key vocabulary mapped onto the shared core's actions.
//!
//! One mapping serves every screen: the action a key asks for depends on the
//! screen the shared state reports, so a letter is an input character in the
//! chat, a filter character in the sessions browser, and nothing in the theme
//! picker, and no key means two things at once. No letter carries a command in
//! any screen: a command is a submitted line beginning with `/`, so `n` and `r`
//! are ordinary characters.
//!
//! The transcript is moved by the mouse wheel, not by keys: `PageUp` and
//! `PageDown` map to nothing here because they never reached a real terminal's
//! application, and the wheel is handled in [`super::handle_event`]. `Ctrl+E`
//! is the one chat binding beside the control keys: it expands the newest
//! collapsed reasoning block, exactly as a click on that block's marker does.
//!
//! `Esc` is the chat's cancel/exit key: a live run is interrupted by a single
//! press, a typed line needs two consecutive presses to be abandoned, and an
//! empty line leaves the front end. The two docked surfaces are separate
//! keymaps and keep `Esc` as their own close key - the sessions browser closes,
//! and the theme picker drops its preview - so the chat's meanings are
//! unreachable while either of them is up: the layering is the band, then the
//! picker, then the live run's cancel, then the input-clear arm, then the exit.
//!
//! The input's command hint band is the newest claim on two keys, and both
//! claims are decided here rather than in the core: while the band is open,
//! `Up` and `Down` move its highlight and `Tab` commits it, while a closed band
//! leaves `Up` and `Down` their older meaning, the input line history, and
//! leaves `Tab` bound to nothing. `Enter` keeps its one meaning - submitting the
//! line - because the submission path completes an open band before it runs
//! anything, so no key press ever means two things at once. The theme picker
//! claims `Up`, `Down`, and `Enter` the same way: the two arrows preview the
//! candidate they land on and `Enter` commits it.
//!
//! The transcript's pointer path - selection, drag-scroll, and the marker click
//! - lives in [`super`], because it needs the frame's published geometry.

use revue::event::{Key, KeyEvent};

use crate::app::{
    Action, AppState, BrowserCursorMove, InputCursorMove, InputHistoryMove, MenuMove, Screen,
};

/// Returns the action one revue key press asks for on the current screen.
///
/// The state is what tells the two meanings of the vertical arrows apart: the
/// hint band is open only while the state says so, and only then do the arrows
/// move its highlight.
pub(super) const fn key_action(key: &KeyEvent, state: &AppState) -> Option<Action> {
    match state.screen() {
        Screen::Chat => chat_action(key, state),
        Screen::Sessions => sessions_action(key),
        Screen::Theme => theme_action(key, state),
    }
}

/// Returns the action one key press asks for on the chat screen.
///
/// `Up`, `Down`, and `Tab` are the three keys the hint menu claims; `Enter` is
/// deliberately not one of them, because the submission path completes an open
/// menu before it runs anything, so the one action `Enter` carries keeps one
/// meaning.
const fn chat_action(key: &KeyEvent, state: &AppState) -> Option<Action> {
    match key.key {
        Key::Char('q') if key.ctrl => Some(Action::Quit),
        Key::Char('c') if key.ctrl => Some(Action::CtrlCPressed),
        Key::Char('e') if key.ctrl => Some(Action::ExpandReasoning { row: None }),
        Key::Char(character) if !key.ctrl => Some(Action::InputChar(character)),
        Key::Enter => Some(Action::InputSubmitted),
        Key::Backspace => Some(Action::InputBackspace),
        Key::Left => Some(Action::MoveInputCursor(InputCursorMove::Left)),
        Key::Right => Some(Action::MoveInputCursor(InputCursorMove::Right)),
        Key::Home => Some(Action::MoveInputCursor(InputCursorMove::Home)),
        Key::End => Some(Action::MoveInputCursor(InputCursorMove::End)),
        Key::Up => vertical_action(state, MenuMove::Up),
        Key::Down => vertical_action(state, MenuMove::Down),
        Key::Tab if state.command_menu().is_some() => Some(Action::MenuAccept),
        Key::Escape => Some(Action::EscapePressed),
        _ => None,
    }
}

/// Returns the action one vertical arrow asks for on the chat screen.
///
/// The hint menu is the newer meaning of the two and claims the arrows exactly
/// while it is open: with no menu, `Up` and `Down` walk the input line history
/// precisely as they did before the menu existed.
const fn vertical_action(state: &AppState, direction: MenuMove) -> Option<Action> {
    if state.command_menu().is_some() {
        return Some(Action::MenuMove(direction));
    }
    Some(Action::NavigateInputHistory(match direction {
        MenuMove::Up => InputHistoryMove::Previous,
        MenuMove::Down => InputHistoryMove::Next,
    }))
}

/// Returns the action one key press asks for on the sessions browser screen.
///
/// Every printable key goes to the filter and the arrows move the cursor, so
/// the browser needs no navigation letters. Ctrl+C and Ctrl+Q keep the meanings
/// the chat screen gives them.
const fn sessions_action(key: &KeyEvent) -> Option<Action> {
    match key.key {
        Key::Char('q') if key.ctrl => Some(Action::Quit),
        Key::Char('c') if key.ctrl => Some(Action::CtrlCPressed),
        Key::Char('r') if key.ctrl => Some(Action::BrowserRenameRequested),
        Key::Char('x') if key.ctrl => Some(Action::BrowserArchiveRequested),
        Key::Char('f') if key.ctrl => Some(Action::BrowserTreeRequested),
        Key::Char(character) if !key.ctrl => Some(Action::BrowserFilterChar(character)),
        Key::Backspace => Some(Action::BrowserFilterBackspace),
        Key::Up => Some(Action::BrowserCursorMove(BrowserCursorMove::Up)),
        Key::Down => Some(Action::BrowserCursorMove(BrowserCursorMove::Down)),
        Key::Tab => Some(Action::BrowserTabNext),
        Key::BackTab => Some(Action::BrowserTabPrevious),
        Key::Enter => Some(Action::BrowserSelectionRequested),
        Key::Escape => Some(Action::SessionsBrowserClosed),
        _ => None,
    }
}

/// Returns the action one key press asks for on the theme picker screen.
///
/// The picker owns the same two rows the core does: `Up` and `Down` walk them
/// without wrapping and preview the candidate they land on, `Enter` commits
/// the highlighted theme through the daemon, and `Esc` drops the preview and
/// returns to the chat. The picker's `Esc` is its own close key, exactly as the
/// browser's is: while it is open, the chat's cancel, clear, and exit arms are
/// unreachable. Ctrl+C and Ctrl+Q keep the meanings every screen gives them.
const fn theme_action(key: &KeyEvent, state: &AppState) -> Option<Action> {
    match key.key {
        Key::Char('q') if key.ctrl => Some(Action::Quit),
        Key::Char('c') if key.ctrl => Some(Action::CtrlCPressed),
        Key::Up => Some(Action::ThemePreviewed(state.effective_theme().previous())),
        Key::Down => Some(Action::ThemePreviewed(state.effective_theme().next())),
        Key::Enter => Some(Action::ThemeSelected(state.effective_theme())),
        Key::Escape => Some(Action::ThemePreviewCleared),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use revue::event::{Key, KeyEvent};

    use crate::app::{
        Action, AppState, BrowserCursorMove, InputCursorMove, InputHistoryMove, MenuMove, Screen,
        Theme,
    };

    use super::key_action;

    /// Returns a chat-screen state with the hint menu closed.
    fn chat_state() -> AppState {
        AppState::new(None)
    }

    /// Returns a state whose screen is the sessions browser.
    fn sessions_state() -> AppState {
        let mut state = AppState::new(None);
        state.update(Action::SessionsBrowserRequested);
        assert_eq!(state.screen(), Screen::Sessions);
        state
    }

    /// Returns a state whose screen is the theme picker.
    ///
    /// The picker is opened the way the keyboard opens it: `/theme` completes
    /// into the command with its argument word still empty, and the next Enter
    /// runs the command the empty word has nothing to complete for.
    fn picker_state() -> AppState {
        let mut state = AppState::new(None);
        for character in "/theme".chars() {
            state.update(Action::InputChar(character));
        }
        state.update(Action::InputSubmitted);
        state.update(Action::InputSubmitted);
        assert_eq!(state.screen(), Screen::Theme);
        state
    }

    /// Returns a chat-screen state whose input's hint menu is open.
    fn menu_state() -> AppState {
        let mut state = AppState::new(None);
        state.update(Action::InputChar('/'));
        assert!(
            state.command_menu().is_some(),
            "a bare slash opens the menu"
        );
        state
    }

    #[test]
    fn the_chat_screen_keeps_the_keys_it_always_had() {
        assert_eq!(
            key_action(&KeyEvent::new(Key::Char('r')), &chat_state()),
            Some(Action::InputChar('r')),
            "a letter is input, never a command"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Enter), &chat_state()),
            Some(Action::InputSubmitted)
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Left), &chat_state()),
            Some(Action::MoveInputCursor(InputCursorMove::Left))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Up), &chat_state()),
            Some(Action::NavigateInputHistory(InputHistoryMove::Previous))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::PageUp), &chat_state()),
            None,
            "the transcript moves on the mouse wheel, not on a page key"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::PageDown), &chat_state()),
            None,
            "the transcript moves on the mouse wheel, not on a page key"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Home), &chat_state()),
            Some(Action::MoveInputCursor(InputCursorMove::Home))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::End), &chat_state()),
            Some(Action::MoveInputCursor(InputCursorMove::End))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Escape), &chat_state()),
            Some(Action::EscapePressed),
            "the chat's Esc is the cancel, clear, and exit key"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Escape), &sessions_state()),
            Some(Action::SessionsBrowserClosed),
            "the browser keeps Esc as its own close key"
        );
    }

    #[test]
    fn the_session_browser_maps_its_own_keys() {
        assert_eq!(
            key_action(&KeyEvent::new(Key::Char('a')), &sessions_state()),
            Some(Action::BrowserFilterChar('a'))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Char(' ')), &sessions_state()),
            Some(Action::BrowserFilterChar(' ')),
            "a space filters, it does not scroll"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Backspace), &sessions_state()),
            Some(Action::BrowserFilterBackspace)
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Up), &sessions_state()),
            Some(Action::BrowserCursorMove(BrowserCursorMove::Up))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Down), &sessions_state()),
            Some(Action::BrowserCursorMove(BrowserCursorMove::Down))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Tab), &sessions_state()),
            Some(Action::BrowserTabNext)
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::BackTab), &sessions_state()),
            Some(Action::BrowserTabPrevious)
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Enter), &sessions_state()),
            Some(Action::BrowserSelectionRequested)
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('r')), &sessions_state()),
            Some(Action::BrowserRenameRequested)
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('x')), &sessions_state()),
            Some(Action::BrowserArchiveRequested)
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('f')), &sessions_state()),
            Some(Action::BrowserTreeRequested)
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Escape), &sessions_state()),
            Some(Action::SessionsBrowserClosed)
        );
    }

    #[test]
    fn ctrl_c_and_ctrl_q_mean_the_same_on_every_screen() {
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('c')), &chat_state()),
            Some(Action::CtrlCPressed)
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('c')), &sessions_state()),
            Some(Action::CtrlCPressed)
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('q')), &chat_state()),
            Some(Action::Quit)
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('q')), &sessions_state()),
            Some(Action::Quit)
        );
    }

    #[test]
    fn the_hint_menu_owns_the_vertical_arrows_and_tab_while_it_is_open() {
        let open = menu_state();
        let closed = chat_state();
        assert_eq!(
            key_action(&KeyEvent::new(Key::Up), &open),
            Some(Action::MenuMove(MenuMove::Up)),
            "an open menu takes Up from the input history"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Down), &open),
            Some(Action::MenuMove(MenuMove::Down))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Tab), &open),
            Some(Action::MenuAccept),
            "Tab commits the highlighted command"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Up), &closed),
            Some(Action::NavigateInputHistory(InputHistoryMove::Previous)),
            "a closed menu leaves Up the history walk it always was"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Down), &closed),
            Some(Action::NavigateInputHistory(InputHistoryMove::Next))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Tab), &closed),
            None,
            "a closed menu leaves Tab bound to nothing"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Enter), &open),
            Some(Action::InputSubmitted),
            "Enter keeps its one meaning: the submission path completes the menu first"
        );
    }

    #[test]
    fn the_theme_picker_owns_the_arrows_enter_and_escape() {
        let mut state = picker_state();
        assert_eq!(
            key_action(&KeyEvent::new(Key::Down), &state),
            Some(Action::ThemePreviewed(Theme::Dark)),
            "Down previews the row below the committed theme"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Up), &state),
            Some(Action::ThemePreviewed(Theme::Light)),
            "Light is the first row: Up stays"
        );

        state.update(Action::ThemePreviewed(Theme::Dark));
        assert_eq!(
            key_action(&KeyEvent::new(Key::Down), &state),
            Some(Action::ThemePreviewed(Theme::Dark)),
            "Dark is the last row: Down stays"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Up), &state),
            Some(Action::ThemePreviewed(Theme::Light)),
            "Up walks back from the previewed row"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Enter), &state),
            Some(Action::ThemeSelected(Theme::Dark)),
            "Enter commits the row the preview is on"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Escape), &state),
            Some(Action::ThemePreviewCleared),
            "the picker's Esc is its own close key"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Char('l')), &state),
            None,
            "a letter means nothing while the picker owns the keyboard"
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('q')), &state),
            Some(Action::Quit),
            "Ctrl+Q stays the immediate exit on every screen"
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('c')), &state),
            Some(Action::CtrlCPressed),
            "Ctrl+C keeps the layered meaning every screen gives it"
        );
    }
}
