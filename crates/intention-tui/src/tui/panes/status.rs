//! The status pane: the human status row under the input and the detail line.
//!
//! The row names what the user acts on in words: the connection, the session,
//! the session's run mode, the run's own state, and the context-usage
//! placeholder. The old debug vocabulary (`connected`, `run none`,
//! `stream idle`, `stream closed`) described this front end's own plumbing and
//! is gone: the stream is an implementation detail, and a run is described by
//! what it is doing, not by the mirror's status word.
//!
//! The elapsed value is the front end's measurement, reported through
//! `Action::ElapsedReported`; the row only formats the value the state carries,
//! so the view stays a pure function of state.

use intention_proto::RunStatusDto;
use revue::style::Color;
use revue::widget::{RichText, Span, Style, Text};

use crate::app::{AppState, ConnectionStatus, short_identifier};
use crate::tui::palette;

/// The detail line this front end shows while no error or notice claims it.
///
/// The line advertises no history keys: `Up`/`Down` still recall the input
/// history, but the hint names only the two keys a new user needs to send a
/// turn or switch surface.
const DEFAULT_HINT: &str = "enter sends | /new session | /sessions switch";

/// The word for a daemon this front end has not reached yet.
const CONNECTING: &str = "Connecting…";

/// The word for a ready daemon.
const READY: &str = "Ready";

/// The word for a daemon the last attempt could not reach.
///
/// `Offline`, not `disconnected`: the removed debug vocabulary is matched by
/// substring, and `connected` may not reappear inside another word.
const OFFLINE: &str = "Offline";

/// The separator between two status runs.
const SEPARATOR: &str = " · ";

/// The marker of a run that has not finished yet.
const THINKING: &str = "Thinking…";

/// The marker of a completed run.
const ANSWERED: &str = "Answered";

/// The marker of a run daemon recovery ended.
const INTERRUPTED: &str = "Interrupted";

/// The marker of a run that failed.
const FAILED: &str = "Failed";

/// The context-usage placeholder; the wire carries no token usage yet.
// @todo(core): the run projection carries no token usage, so the status row
// shows this placeholder instead of a fabricated count. The core would have to
// carry a `UsageDto` (prompt, completion, and total tokens) on the run
// projection for the row to show a real number.
const CONTEXT_USAGE: &str = "ctx @todo(core)";

/// Returns the status row under the input: the human state of the session.
pub(in crate::tui) fn status_row(state: &AppState) -> RichText {
    let mut text = RichText::new();
    let (connection, ink) = match state.connection() {
        ConnectionStatus::Connecting => (CONNECTING, palette::WARNING),
        ConnectionStatus::Ready(_) => (READY, palette::SUCCESS),
        ConnectionStatus::Failed => (OFFLINE, palette::ERROR),
    };
    text = run(text, connection, ink);
    text = run(text, SEPARATOR, palette::INK_FAINT);
    text = match state.session_id() {
        Some(session_id) => run(
            text,
            &format!("session {}", short_identifier(session_id)),
            palette::INK_MUTED,
        ),
        None => run(text, "no session", palette::INK_FAINT),
    };
    if let Some(mode) = state.session_mode() {
        text = run(text, SEPARATOR, palette::INK_FAINT);
        text = run(text, mode.as_str(), palette::INK_MUTED);
    }
    if let Some(status) = state.run_status() {
        text = run(text, SEPARATOR, palette::INK_FAINT);
        text = run_phrase(text, status, state.elapsed_millis());
    }
    text = run(text, SEPARATOR, palette::INK_FAINT);
    run(text, CONTEXT_USAGE, palette::TODO_INK)
}

/// Returns the detail line: the failure, the notice, or the idle hint.
pub(in crate::tui) fn status_detail(state: &AppState) -> Text {
    match (state.error(), state.notice()) {
        (Some(error), _) => Text::new(error.to_owned()).fg(palette::ERROR).bold(),
        (None, Some(notice)) => Text::new(notice.to_owned()).fg(palette::INK_MUTED),
        (None, None) => Text::new(DEFAULT_HINT).fg(palette::INK_FAINT).dim(),
    }
}

/// Appends one styled run to the status row.
fn run(text: RichText, content: &str, ink: Color) -> RichText {
    text.span(Span::styled(
        content.to_owned(),
        Style::new().fg(ink).bg(palette::PANEL),
    ))
}

/// Appends the phrase one run status reads as, with its measured elapsed value.
///
/// A live run reads `Thinking… 3.2s`, a finished one `Answered in 13s`, a
/// failed one `Failed` (the typed error is the detail line), and an interrupted
/// one `Interrupted`. A run whose elapsed value the front end never reported
/// keeps the word without a number instead of showing a fabricated one.
fn run_phrase(text: RichText, status: RunStatusDto, elapsed: Option<u64>) -> RichText {
    match status {
        RunStatusDto::Starting | RunStatusDto::Running => {
            let mut text = run(text, THINKING, palette::SCARLET);
            if let Some(millis) = elapsed {
                text = run(
                    text,
                    &format!(" {}", live_elapsed(millis)),
                    palette::TIMER_INK,
                );
            }
            text
        }
        RunStatusDto::Completed => {
            let mut text = run(text, ANSWERED, palette::SUCCESS);
            if let Some(millis) = elapsed {
                text = run(text, " in ", palette::SUCCESS);
                text = run(text, &finished_elapsed(millis), palette::TIMER_INK);
            }
            text
        }
        RunStatusDto::Interrupted => run(text, INTERRUPTED, palette::WARNING),
        RunStatusDto::Failed => run(text, FAILED, palette::ERROR),
    }
}

/// Returns the elapsed value a live run shows: tenths of a second.
fn live_elapsed(millis: u64) -> String {
    format!("{:.1}s", millis as f64 / 1000.0)
}

/// Returns the elapsed value a finished run shows: whole seconds.
fn finished_elapsed(millis: u64) -> String {
    format!("{}s", millis / 1000)
}

#[cfg(test)]
mod tests {
    use super::{finished_elapsed, live_elapsed};

    #[test]
    fn a_live_elapsed_value_reads_in_tenths_of_a_second() {
        assert_eq!(live_elapsed(0), "0.0s");
        assert_eq!(live_elapsed(3_200), "3.2s");
        assert_eq!(live_elapsed(12_999), "13.0s");
    }

    #[test]
    fn a_finished_elapsed_value_reads_in_whole_seconds() {
        assert_eq!(finished_elapsed(0), "0s");
        assert_eq!(finished_elapsed(13_000), "13s");
        assert_eq!(finished_elapsed(13_900), "13s");
    }
}
