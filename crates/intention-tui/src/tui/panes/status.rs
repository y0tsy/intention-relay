//! The status pane: the human status row under the input and the detail line.
//!
//! The row names what the user acts on in words: the connection, the session,
//! the session's run mode, the run's live phase, and the committed token usage.
//! The old debug vocabulary (`connected`, `run none`, `stream idle`,
//! `stream closed`) described this front end's own plumbing and is gone: the
//! stream is an implementation detail, and a run is described by what it is
//! doing, not by the mirror's status word.
//!
//! The live phase is the shared state machine's own value: `Waiting…` from the
//! moment a turn request leaves for the network, `Thinking…` when the reasoning
//! stream starts, `Answering…` when the answer stream starts, and `Working…`
//! while the run projection reports an open tool call past the half-second
//! threshold - a shorter call leaves the phase the one it found. A completed run
//! reads `Answered in 13s`, the committed run's whole seconds; the interrupted
//! and failed runs keep their words. A live value reads in tenths of a second,
//! because it is the front end's own measurement reported through
//! `Action::ElapsedReported`; the final value is the committed projection's own
//! start and finish times, so the row only formats values the state carries and
//! the view stays a pure function of state.
//!
//! The usage segment reads the committed `UsageDto` the run projection carries:
//! `ctx in 1200 out 300`, or `ctx not reported` when the provider reported no
//! usage. A run that has committed no usage yet carries no usage segment rather
//! than a fabricated number.

use intention_proto::{RunProjectionDto, RunStatusDto, UsageDto};
use revue::style::Color;
use revue::widget::{RichText, Span, Style, Text};

use crate::app::{AppState, ConnectionStatus, RunPhase, short_identifier};
use crate::tui::palette::Palette;

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

/// The marker of a run whose turn request left for the network.
const WAITING: &str = "Waiting…";

/// The marker of a run whose reasoning stream started.
const THINKING: &str = "Thinking…";

/// The marker of a run whose answer stream started.
const ANSWERING: &str = "Answering…";

/// The marker of a run with a tool call in flight past the working threshold.
const WORKING: &str = "Working…";

/// The marker of a completed run.
const ANSWERED: &str = "Answered";

/// The marker of a run daemon recovery ended.
const INTERRUPTED: &str = "Interrupted";

/// The marker of a run that failed.
const FAILED: &str = "Failed";

/// Returns the usage segment one run reads as, when its projection carries a
/// committed usage value.
///
/// The numbers are the committed provider-normalized counts: input tokens are
/// the context the run was sent, output tokens what it produced. A run whose
/// usage was not reported says so instead of showing a zero.
fn usage_segment(run: Option<RunProjectionDto>) -> Option<String> {
    match run?.usage()? {
        UsageDto::NotReported => Some("ctx not reported".to_owned()),
        UsageDto::Reported {
            input_tokens,
            output_tokens,
            ..
        } => Some(format!("ctx in {input_tokens} out {output_tokens}")),
    }
}

/// Returns the committed whole seconds one terminated run took, when its
/// projection carries both times.
fn committed_seconds(run: RunProjectionDto) -> Option<i64> {
    let finished = run.finished_at()?;
    Some(finished.unix_seconds() - run.started_at().unix_seconds())
}

/// Returns the status row under the input: the human state of the session.
pub(in crate::tui) fn status_row(state: &AppState, palette: &'static Palette) -> RichText {
    let mut text = RichText::new();
    let (connection, ink) = match state.connection() {
        ConnectionStatus::Connecting => (CONNECTING, palette.warning),
        ConnectionStatus::Ready => (READY, palette.success),
        ConnectionStatus::Failed => (OFFLINE, palette.error),
    };
    text = run(text, connection, ink, palette);
    text = run(text, SEPARATOR, palette.ink_faint, palette);
    text = match state.session_id() {
        Some(session_id) => run(
            text,
            &format!("session {}", short_identifier(session_id)),
            palette.ink_muted,
            palette,
        ),
        None => run(text, "no session", palette.ink_faint, palette),
    };
    if let Some(mode) = state.session_mode() {
        text = run(text, SEPARATOR, palette.ink_faint, palette);
        text = run(text, mode.as_str(), palette.ink_muted, palette);
    }
    let phase = state.run_phase();
    let status = state.run_status();
    let projection = state.active_run();
    // A live phase names the run even before a refreshed projection arrives;
    // the status word covers every other run, terminal ones included.
    if phase.is_some() || status.is_some() {
        text = run(text, SEPARATOR, palette.ink_faint, palette);
        text = run_phrase(
            text,
            phase,
            status,
            state.elapsed_millis(),
            projection,
            palette,
        );
    }
    // The committed usage is a run fact; a run that has not committed one yet
    // shows no segment at all instead of a placeholder.
    if let Some(usage) = usage_segment(projection) {
        text = run(text, SEPARATOR, palette.ink_faint, palette);
        text = run(text, &usage, palette.ink_muted, palette);
    }
    text
}

/// Returns the detail line: the failure, the notice, or the idle hint.
pub(in crate::tui) fn status_detail(state: &AppState, palette: &'static Palette) -> Text {
    match (state.error(), state.notice()) {
        (Some(error), _) => Text::new(error.to_owned()).fg(palette.error).bold(),
        (None, Some(notice)) => Text::new(notice.to_owned()).fg(palette.ink_muted),
        (None, None) => Text::new(DEFAULT_HINT).fg(palette.ink_faint).dim(),
    }
}

/// Appends one styled run to the status row.
fn run(text: RichText, content: &str, ink: Color, palette: &'static Palette) -> RichText {
    text.span(Span::styled(
        content.to_owned(),
        Style::new().fg(ink).bg(palette.panel),
    ))
}

/// Appends the phrase one run reads as, with its live elapsed value or its
/// committed duration.
///
/// A live phase reads `Waiting… 0.4s`, `Thinking… 3.2s`, `Answering… 4.1s`, or
/// `Working… 6.2s`; a completed run reads `Answered in 13s` from the committed
/// projection's own times; a failed one `Failed` (the typed error is the detail
/// line), and an interrupted one `Interrupted`. A live value reads in tenths of
/// a second, and a run whose live value the front end never reported keeps the
/// word without a number instead of showing a fabricated one.
fn run_phrase(
    text: RichText,
    phase: Option<RunPhase>,
    status: Option<RunStatusDto>,
    elapsed: Option<u64>,
    projection: Option<RunProjectionDto>,
    palette: &'static Palette,
) -> RichText {
    if let Some(phase) = phase {
        let mut text = run(text, phase_word(phase), palette.scarlet, palette);
        if let Some(millis) = elapsed {
            text = run(
                text,
                &format!(" {}", elapsed_tenths(millis)),
                palette.timer_ink,
                palette,
            );
        }
        return text;
    }
    match status {
        Some(RunStatusDto::Completed) => {
            let mut text = run(text, ANSWERED, palette.success, palette);
            if let Some(seconds) = projection.and_then(committed_seconds) {
                text = run(text, " in ", palette.success, palette);
                text = run(text, &format!("{seconds}s"), palette.timer_ink, palette);
            }
            text
        }
        Some(RunStatusDto::Interrupted) => run(text, INTERRUPTED, palette.warning, palette),
        Some(RunStatusDto::Failed) => run(text, FAILED, palette.error, palette),
        // A live run the state carries no phase for is one this front end is
        // waiting on its first token; the state names a phase for every run it
        // mirrors, so this arm only keeps the view total.
        Some(RunStatusDto::Starting | RunStatusDto::Running) => {
            run(text, WAITING, palette.scarlet, palette)
        }
        None => text,
    }
}

/// Returns the word one live phase reads as.
const fn phase_word(phase: RunPhase) -> &'static str {
    match phase {
        RunPhase::Waiting => WAITING,
        RunPhase::Thinking => THINKING,
        RunPhase::Answering => ANSWERING,
        RunPhase::Working => WORKING,
    }
}

/// Returns how one live measured elapsed value reads: tenths of a second.
///
/// Only live values read this way; the committed final value reads in whole
/// seconds from the run projection's own start and finish times.
fn elapsed_tenths(millis: u64) -> String {
    format!("{:.1}s", millis as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use crate::app::RunPhase;

    use super::{elapsed_tenths, phase_word};

    #[test]
    fn every_elapsed_value_reads_in_tenths_of_a_second() {
        assert_eq!(elapsed_tenths(0), "0.0s");
        assert_eq!(elapsed_tenths(3_200), "3.2s");
        assert_eq!(elapsed_tenths(12_999), "13.0s");
        assert_eq!(elapsed_tenths(13_000), "13.0s");
        assert_eq!(elapsed_tenths(13_400), "13.4s");
    }

    #[test]
    fn every_live_phase_pairs_its_word() {
        for (phase, word) in [
            (RunPhase::Waiting, "Waiting…"),
            (RunPhase::Thinking, "Thinking…"),
            (RunPhase::Answering, "Answering…"),
            (RunPhase::Working, "Working…"),
        ] {
            assert_eq!(phase_word(phase), word);
        }
    }
}
