//! The revue key vocabulary mapped onto the shared core's actions.
//!
//! One mapping serves every screen: the action a key asks for depends on the
//! screen the shared state reports, so a letter is an input character in the
//! chat and a filter character in the sessions browser, and no key means two
//! things at once. No letter carries a command in either screen: a command is a
//! submitted line beginning with `/`, so `n` and `r` are ordinary characters.
//!
//! The transcript is moved by the mouse wheel, not by keys: `PageUp` and
//! `PageDown` map to nothing here because they never reached a real terminal's
//! application, and the wheel is handled in [`super::handle_event`]. `Ctrl+E`
//! is the one chat binding beside the control keys: it expands the newest
//! collapsed reasoning block, exactly as a click on that block's marker does.
//!
//! `Esc` is the chat's cancel/exit key: a live run is interrupted by a single
//! press, a typed line needs two consecutive presses to be abandoned, and an
//! empty line leaves the front end. The sessions browser is a separate keymap
//! and keeps `Esc` as its close key, so the two meanings never meet.
//!
//! The transcript's pointer path - selection, drag-scroll, and the marker click
//! - lives in [`super`], because it needs the frame's published geometry.

use revue::event::{Key, KeyEvent};

use crate::app::{Action, BrowserCursorMove, InputCursorMove, InputHistoryMove, Screen};

/// Returns the action one revue key press asks for on the current screen.
pub(super) const fn key_action(key: &KeyEvent, screen: Screen) -> Option<Action> {
    match screen {
        Screen::Chat => chat_action(key),
        Screen::Sessions => sessions_action(key),
    }
}

/// Returns the action one key press asks for on the chat screen.
const fn chat_action(key: &KeyEvent) -> Option<Action> {
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
        Key::Up => Some(Action::NavigateInputHistory(InputHistoryMove::Previous)),
        Key::Down => Some(Action::NavigateInputHistory(InputHistoryMove::Next)),
        Key::Escape => Some(Action::EscapePressed),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use revue::event::{Key, KeyEvent};

    use crate::app::{Action, BrowserCursorMove, InputCursorMove, InputHistoryMove, Screen};

    use super::key_action;

    #[test]
    fn the_chat_screen_keeps_the_keys_it_always_had() {
        assert_eq!(
            key_action(&KeyEvent::new(Key::Char('r')), Screen::Chat),
            Some(Action::InputChar('r')),
            "a letter is input, never a command"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Enter), Screen::Chat),
            Some(Action::InputSubmitted)
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Left), Screen::Chat),
            Some(Action::MoveInputCursor(InputCursorMove::Left))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Up), Screen::Chat),
            Some(Action::NavigateInputHistory(InputHistoryMove::Previous))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::PageUp), Screen::Chat),
            None,
            "the transcript moves on the mouse wheel, not on a page key"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::PageDown), Screen::Chat),
            None,
            "the transcript moves on the mouse wheel, not on a page key"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Home), Screen::Chat),
            Some(Action::MoveInputCursor(InputCursorMove::Home))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::End), Screen::Chat),
            Some(Action::MoveInputCursor(InputCursorMove::End))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Escape), Screen::Chat),
            Some(Action::EscapePressed),
            "the chat's Esc is the cancel, clear, and exit key"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Escape), Screen::Sessions),
            Some(Action::SessionsBrowserClosed),
            "the browser keeps Esc as its own close key"
        );
    }

    #[test]
    fn the_session_browser_maps_its_own_keys() {
        assert_eq!(
            key_action(&KeyEvent::new(Key::Char('a')), Screen::Sessions),
            Some(Action::BrowserFilterChar('a'))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Char(' ')), Screen::Sessions),
            Some(Action::BrowserFilterChar(' ')),
            "a space filters, it does not scroll"
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Backspace), Screen::Sessions),
            Some(Action::BrowserFilterBackspace)
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Up), Screen::Sessions),
            Some(Action::BrowserCursorMove(BrowserCursorMove::Up))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Down), Screen::Sessions),
            Some(Action::BrowserCursorMove(BrowserCursorMove::Down))
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Tab), Screen::Sessions),
            Some(Action::BrowserTabNext)
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::BackTab), Screen::Sessions),
            Some(Action::BrowserTabPrevious)
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Enter), Screen::Sessions),
            Some(Action::BrowserSelectionRequested)
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('r')), Screen::Sessions),
            Some(Action::BrowserRenameRequested)
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('x')), Screen::Sessions),
            Some(Action::BrowserArchiveRequested)
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('f')), Screen::Sessions),
            Some(Action::BrowserTreeRequested)
        );
        assert_eq!(
            key_action(&KeyEvent::new(Key::Escape), Screen::Sessions),
            Some(Action::SessionsBrowserClosed)
        );
    }

    #[test]
    fn ctrl_c_and_ctrl_q_mean_the_same_on_every_screen() {
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('c')), Screen::Chat),
            Some(Action::CtrlCPressed)
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('c')), Screen::Sessions),
            Some(Action::CtrlCPressed)
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('q')), Screen::Chat),
            Some(Action::Quit)
        );
        assert_eq!(
            key_action(&KeyEvent::ctrl(Key::Char('q')), Screen::Sessions),
            Some(Action::Quit)
        );
    }
}
