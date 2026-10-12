//! Recognising a paste a terminal delivered as typing.
//!
//! Bracketed paste is the path a terminal offers when it can: the pasted text
//! arrives as one [`Event::Paste`], so the front end knows it was a paste
//! without guessing anything, and it inserts the whole text as the one edit it
//! is. A terminal that types a paste into the application instead produces its
//! characters and its line breaks as ordinary key presses, and there the front
//! end has to hold the text until it knows the paste has ended: applying each
//! character as it arrives would paint the rows the paste is about to replace,
//! because the loop redraws after every event it handles.
//!
//! **Size decides, never timing.** A run starts when two presses arrive within
//! [`BURST_GAP`], which no hand can do, so ordinary typing never opens one; from
//! then on the run's own text decides how long it may take. A run the size of the
//! line - up to a few hundred characters and a few lines - is settled after the
//! short gap, because a paste that small is never delivered in pieces; a run
//! larger than that is one label's worth of text and keeps collecting across
//! [`PASTE_GAP`], because a terminal hands a large paste over in chunks whose
//! pauses are tens or hundreds of milliseconds long. The submission key is the
//! exception: it joins a run only within the short gap, so the line break a paste
//! carried while it was being written is part of the paste, and the `Enter` the
//! user presses once the text is in the line always sends it.
//!
//! A tick never ends a run - it is the clock the run's silence is measured
//! against - and neither does the terminal's own work: only the run's own
//! silence, or the user's next input, ends it.
//!
//! This detector serves the chat's line. The sessions browser's filter and the
//! theme picker keep their own key handling, unchanged.
//!
//! [`Event::Paste`]: revue::event::Event::Paste

use std::time::{Duration, Instant};

use crate::app::Action;

/// The longest gap between two presses that still opens a run.
///
/// A hand cannot press two keys within five milliseconds of one another, while a
/// terminal writing a paste into the application does it in one burst. The gap
/// only ever *opens* a run: continuing one is [`PASTE_GAP`]'s business.
const BURST_GAP: Duration = Duration::from_millis(5);

/// The gap a run's characters may arrive within while it is still small, and the
/// silence that ends one.
///
/// A run this size is a line the user could have typed, so it is settled as soon
/// as the presses stop - one frame at most - instead of waiting for the wider gap
/// a paste earns.
const RUN_GAP: Duration = Duration::from_millis(200);

/// The gap a paste's own characters may arrive within, and the silence that ends
/// one.
///
/// A large paste is handed over in chunks - a clipboard transfer gives the
/// terminal a few kilobytes at a time - and the pauses between two chunks are
/// tens or hundreds of milliseconds long. Size, not timing, is what tells the run
/// it is a paste: once its text has grown past the size one label stands for, its
/// characters keep collecting across those pauses, and only this much silence
/// ends it.
const PASTE_GAP: Duration = Duration::from_millis(1_000);

/// The run of presses the front end is holding.
#[derive(Debug)]
pub(super) struct Pastes {
    /// The text the run carries, exactly as its presses spelled it.
    text: String,
    /// The typed action a lone press carries.
    ///
    /// A run of one press is the typed input it was - the character or the
    /// submission the user made - so that press's own action is kept beside the
    /// run and handed back instead of a paste.
    alone: Option<Action>,
    /// Whether the run is a paste: presses that arrived together.
    confirmed: bool,
    /// When the run's last press arrived.
    last: Option<Instant>,
}

impl Pastes {
    /// Creates a detector that is holding nothing.
    pub(super) const fn new() -> Self {
        Self {
            text: String::new(),
            alone: None,
            confirmed: false,
            last: None,
        }
    }

    /// Notes one character-carrying press and returns the action it settled.
    ///
    /// A press that follows the run in flight inside its gap joins it; a press a
    /// hand's own pause separates settles the run first - so the line keeps the
    /// order the user pressed in - and opens a run of its own.
    ///
    /// The `Enter` that closes a paste is the paste's own: a run large enough to
    /// be one label is text the user cannot have read yet, so the press that ended
    /// it - the line break a terminal split off its own chunk, or a reflex press -
    /// leaves the line standing instead of submitting it. Only a press made after
    /// the paste is in the line sends the turn.
    pub(super) fn note_press(&mut self, now: Instant, action: Action) -> Option<Action> {
        if self.continues(now, &action) {
            self.push_character(press_character(&action), now);
            return None;
        }
        let closes_paste = self.confirmed
            && crate::app::paste_is_large(&self.text)
            && matches!(action, Action::InputSubmitted);
        let settled = self.flush();
        if closes_paste {
            return settled;
        }
        self.text.clear();
        self.alone = Some(action.clone());
        self.push_character(press_character(&action), now);
        settled
    }

    /// Ends the run in flight when its own silence has passed, and returns the
    /// action it settles.
    ///
    /// A lone press settles on the first event at all: it is not a paste, so
    /// nothing waits with it. A confirmed run waits out the paste's gap, so the
    /// ticks between a terminal's chunks leave it standing.
    pub(super) fn settle(&mut self, now: Instant) -> Option<Action> {
        let last = self.last?;
        if !self.confirmed {
            return self.flush();
        }
        if now.saturating_duration_since(last) <= PASTE_GAP {
            return None;
        }
        self.flush()
    }

    /// Ends the run in flight now, whatever its gap.
    ///
    /// The user's own next input ends a run: their key or click must reach the
    /// line after the paste it interrupted, never before it.
    pub(super) fn end(&mut self) -> Option<Action> {
        self.flush()
    }

    /// Returns whether a press arriving now continues the run in flight.
    fn continues(&self, now: Instant, action: &Action) -> bool {
        let Some(last) = self.last else {
            return false;
        };
        let gap = now.saturating_duration_since(last);
        gap <= self.window(action)
    }

    /// Returns the gap one press may arrive within.
    ///
    /// The run's first two presses must arrive together. From then on the run is a
    /// paste and keeps collecting across the pauses a terminal's delivery leaves,
    /// while the submission key joins it only within the short gap: the line break
    /// a paste carried arrives inside its own chunk, so a press later than that is
    /// the user's own and must not become one of the paste's line breaks.
    const fn window(&self, action: &Action) -> Duration {
        if !self.confirmed {
            return BURST_GAP;
        }
        if matches!(action, Action::InputSubmitted) {
            return RUN_GAP;
        }
        PASTE_GAP
    }

    /// Adds one press's character to the run in flight.
    fn push_character(&mut self, character: Option<char>, now: Instant) {
        if let Some(character) = character {
            self.text.push(character);
        }
        self.confirm(now);
    }

    /// Confirms the run as a paste once it holds a second press.
    ///
    /// No hand presses two keys within the burst gap, so whatever follows a press
    /// that fast came from the terminal, and from then on the run's own size and
    /// gap decide where it ends.
    fn confirm(&mut self, now: Instant) {
        if self.last.is_some() {
            self.alone = None;
            self.confirmed = true;
        }
        self.last = Some(now);
    }

    /// Ends the run in flight and returns the one action it stands for.
    ///
    /// A run of several presses is the paste the terminal wrote, so its text
    /// enters the line as one edit and the core decides what that text is: a small
    /// one inserts as ordinary characters and a large one becomes one labelled
    /// line whose paste the submission path expands. A run of one press is that
    /// press's own typed action.
    fn flush(&mut self) -> Option<Action> {
        self.last = None;
        self.confirmed = false;
        let text = std::mem::take(&mut self.text);
        match self.alone.take() {
            Some(action) => Some(action),
            None if text.is_empty() => None,
            None => Some(Action::InputPasted(text)),
        }
    }
}

/// Returns the character one chat key press spells, or `None` when the press
/// carries no text.
const fn press_character(action: &Action) -> Option<char> {
    match action {
        Action::InputChar(character) => Some(*character),
        // The submission key spells the break a paste's own newline is: inside a
        // run whose presses arrived together the key is never the submission,
        // because the run's text enters the line as data.
        Action::InputSubmitted => Some('\n'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::app::Action;

    use super::Pastes;

    /// Returns one instant `millis` after a fixed start.
    fn at(millis: u64) -> Instant {
        Instant::now() + Duration::from_millis(millis)
    }

    #[test]
    fn a_run_of_presses_enters_the_line_as_one_paste() {
        let mut pastes = Pastes::new();
        assert_eq!(pastes.note_press(at(0), Action::InputChar('a')), None);
        assert_eq!(pastes.note_press(at(1), Action::InputChar('b')), None);
        assert_eq!(
            pastes.note_press(at(2), Action::InputSubmitted),
            None,
            "the run is still in flight"
        );
        assert_eq!(
            pastes.settle(at(3_000)),
            Some(Action::InputPasted("ab\n".to_owned())),
            "the run's presses become one edit, line break included"
        );
        assert_eq!(pastes.settle(at(4_000)), None, "a run ends exactly once");
    }

    #[test]
    fn a_press_a_hand_s_pause_separates_is_the_typed_input_it_was() {
        let mut pastes = Pastes::new();
        assert_eq!(pastes.note_press(at(0), Action::InputChar('h')), None);
        assert_eq!(
            pastes.settle(at(20)),
            Some(Action::InputChar('h')),
            "a lone press is not a paste: the next event settles it"
        );
        assert_eq!(pastes.note_press(at(400), Action::InputSubmitted), None);
        assert_eq!(
            pastes.settle(at(420)),
            Some(Action::InputSubmitted),
            "a lone Enter is still the submission"
        );
    }

    #[test]
    fn a_typing_press_settles_the_press_before_it() {
        let mut pastes = Pastes::new();
        assert_eq!(pastes.note_press(at(0), Action::InputChar('a')), None);
        assert_eq!(
            pastes.note_press(at(100), Action::InputChar('b')),
            Some(Action::InputChar('a')),
            "the oldest press is applied first, so the line keeps its order"
        );
        assert_eq!(
            pastes.settle(at(120)),
            Some(Action::InputChar('b')),
            "the press in hand is a lone press of its own"
        );
    }

    #[test]
    fn the_chunks_of_one_paste_settle_as_one_paste() {
        let mut pastes = Pastes::new();
        // A paste handed over in three chunks with the pauses a clipboard transfer
        // leaves between them, and a tick inside every pause: neither may split it.
        let mut at_millis = 0;
        for chunk in 0..3 {
            assert_eq!(
                pastes.note_press(at(at_millis), Action::InputChar('x')),
                None
            );
            assert_eq!(
                pastes.note_press(at(at_millis + 1), Action::InputSubmitted),
                None,
                "a line break inside a chunk is the paste's own"
            );
            assert_eq!(
                pastes.settle(at(at_millis + 200)),
                None,
                "a tick that lands in the pause between two chunks"
            );
            at_millis += 400;
            let _ = chunk;
        }
        assert_eq!(
            pastes.settle(at(3_000)),
            Some(Action::InputPasted("x\nx\nx\n".to_owned())),
            "the chunks are one paste"
        );
    }

    #[test]
    fn a_paste_the_line_could_hold_waits_the_paste_gap_too() {
        let mut pastes = Pastes::new();
        assert_eq!(pastes.note_press(at(0), Action::InputChar('a')), None);
        assert_eq!(pastes.note_press(at(1), Action::InputChar('b')), None);
        assert_eq!(
            pastes.settle(at(300)),
            None,
            "a run the terminal wrote is a paste however short it is"
        );
        assert_eq!(
            pastes.settle(at(1_200)),
            Some(Action::InputPasted("ab".to_owned())),
            "and settles once the paste's gap has passed"
        );
    }

    #[test]
    fn the_enter_that_closes_a_paste_does_not_submit_it() {
        let mut pastes = Pastes::new();
        // A paste past the size one line can show: nine lines of text, written the
        // way a terminal writes one.
        for line in 0..9 {
            let at_millis = line * 2;
            assert_eq!(
                pastes.note_press(at(at_millis), Action::InputChar('a')),
                None
            );
            assert_eq!(
                pastes.note_press(at(at_millis + 1), Action::InputSubmitted),
                None
            );
        }
        let text = "a\n".repeat(9);
        assert_eq!(
            pastes.note_press(at(300), Action::InputSubmitted),
            Some(Action::InputPasted(text)),
            "the press that ended the paste leaves the line standing"
        );
        assert_eq!(
            pastes.note_press(at(400), Action::InputSubmitted),
            None,
            "the user's own Enter after the paste opens a press of its own"
        );
        assert_eq!(
            pastes.settle(at(420)),
            Some(Action::InputSubmitted),
            "and is then the submission it meant"
        );
    }

    #[test]
    fn the_user_s_own_event_ends_a_run_at_once() {
        let mut pastes = Pastes::new();
        assert_eq!(pastes.note_press(at(0), Action::InputChar('a')), None);
        assert_eq!(pastes.note_press(at(1), Action::InputChar('b')), None);
        assert_eq!(
            pastes.end(),
            Some(Action::InputPasted("ab".to_owned())),
            "the key that follows a run is handled after the paste it ended"
        );
    }
}
