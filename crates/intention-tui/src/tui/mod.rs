//! The fullscreen terminal front end over the shared application core.
//!
//! [`TerminalView`] is a pure function of the same [`AppState`] the application core
//! publishes, and this front end drives that state through the same [`Action`] and
//! [`crate::app::Effect`] vocabulary, so no session, transcript, input, or status
//! decision lives in the view. One client driver owns the connection: keys become
//! actions for the core, the core returns effects, and the driver reports the
//! actions its client work produced.
//!
//! The front end draws one window: the chat panel on the canvas, with the
//! sessions browser docked as the window's bottom rows while the core's screen
//! carries it. Committed rows lay out at the transcript pane's width,
//! `palette` is the only source of colour, and the event loop leaves every quit
//! key to the shared core's layered action: a live run is stopped on the second
//! consecutive Ctrl+C, a typed line is abandoned into the history, and an empty
//! line arms the exit for a second press. Ctrl+Q stays the immediate exit.
//!
//! # The wheel, the pointer, and copying
//!
//! The transcript is moved by the mouse wheel: the terminal's mouse capture is
//! on so wheel events reach `handle_event`, which maps a notch to
//! [`Action::ScrollTranscript`] - one normal step - and routes the left button
//! through `Pointer`. A press anchors a selection at the display row it hit,
//! a drag extends it, and a release keeps it: the range reaches the core as one
//! [`Action::SelectTranscriptRows`], and the pane paints it. A press and
//! release without a drag is a hit-test only - it leaves the pressed row alone
//! and never moves the window - and a press on a collapsed reasoning block's
//! marker expands that block when the button comes back up.
//!
//! While the left button is held and the pointer slides past the transcript's
//! top or bottom edge, every event scrolls the window one fast step
//! ([`Action::ScrollTranscriptFast`], five wheel notches) and extends the
//! selection to the row the pointer is over, so a selection can reach far
//! beyond the window; a wheel notch that arrives while the button is held adds
//! the same fast step. The selection's leading edge follows the content, so the
//! highlight covers exactly the rows the pointer dragged past.
//!
//! Capture costs the terminal its own drag-selection: while capture is on, a
//! terminal needs Shift held to select text with the mouse instead of feeding
//! the press to this front end. This front end performs no copying of any kind:
//! nothing copies on release, nothing asks the terminal for its selection, and
//! the crate takes no clipboard dependency. Copying is delegated to the
//! terminal emulator, which owns the screen and the user's own drag.
//!
//! # The turn clock
//!
//! The application core is deliberately clock-free, so this front end times the
//! live tail of one turn itself: `TurnTimer` starts when the core returns the
//! `SendTurn` effect, reports each changed tenth of a second while the turn is
//! live, and ends on the terminal run status, each report arriving in the core
//! as [`Action::ElapsedReported`]. The final duration is not this front end's:
//! the committed run projection carries its start and finish times, so the
//! status row renders the committed whole seconds once the run is terminal. The
//! view therefore formats values the state carries instead of reading a clock,
//! and it stays a pure function of state.
//!
//! # Pastes
//!
//! A paste is one edit, never a submission per line. The terminal delivers a
//! bracketed paste as one `Event::Paste`, so its text enters the line whole;
//! a terminal that types a paste into the application instead is recognised by
//! the timing of its presses (`paste`), its `Enter` presses insert the pasted
//! text's own line breaks, and the run's end reports the characters it inserted
//! so the core can keep a large paste outside the line as one label. The
//! terminal owns the clipboard: this front end reads none, and it carries no
//! copy path of its own.

mod driver;
mod keymap;
mod layout;
mod palette;
mod panes;
mod paste;
mod screens;
mod surface;
pub mod testing;
mod theme;

pub use driver::TuiOptions;
pub use screens::TerminalView;

use std::time::Instant;

use intention_client::IntentionClient;
use intention_proto::{
    DtoResult, ErrorDto, SessionFrameDto, SessionId, WorkspaceRootDto, run_status_is_terminal,
};
use intention_transport::LocalEndpoint;
use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui::crossterm::cursor::Show;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;

use crate::app::{
    Action, AppState, BrowserCursorMove, Effect, Screen, TRANSCRIPT_DRAG_ROWS, TranscriptScroll,
};

use driver::Driver;
use keymap::key_action;
use layout::{LaidOutRow, TranscriptLayoutCache, TranscriptWindow};
use paste::Pastes;
use surface::TranscriptSurface;

/// The longest the event loop waits for input before it reports a tick.
///
/// The tick is the clock the front end measures with: the live turn's elapsed
/// tenth of a second and a paste run's own silence both advance on it.
const TICK: std::time::Duration = std::time::Duration::from_millis(100);

/// Runs the terminal front end until the user quits.
///
/// Mouse capture is on, so the wheel scrolls the transcript instead of the
/// terminal's own scrollback; see the module documentation for the trade-off.
///
/// # Errors
///
/// Returns the typed validation error of a local settings file that declares
/// an undeclared theme or an unreadable shape, and an unavailable error when
/// the client runtime or the terminal cannot be started. Client, protocol, and
/// run failures reach the status pane instead of ending the session.
pub fn run_blocking(
    options: TuiOptions,
    client: IntentionClient,
    endpoint: LocalEndpoint,
) -> DtoResult<()> {
    // The theme is this front end's own value and is resolved once, before the
    // first frame: the spelled flag, else the local settings file, else the
    // environment detection the file asks for. See `theme`.
    let theme = theme::resolve(options.theme())?;
    // The front end resolves the session it opens once, through the client: an
    // explicit continuation asks the client for the target, a selected session
    // is used as it is, and a launch that asked for neither opens nothing.
    let session = match options.session() {
        Some(session) => Some(session),
        None if options.continue_session() => Some(driver::resolve_continue(&client)?),
        None => None,
    };
    let driver = Driver::spawn(
        client,
        endpoint,
        options.workspace_root().clone(),
        options.mode(),
    )?;
    let mut front_end = FrontEnd::new(session, options.workspace_root().clone(), driver, theme);
    front_end.start();
    let mut session = Session::start()?;
    let mut dirty = true;
    loop {
        if dirty {
            session
                .terminal
                .draw(|frame| front_end.render(frame))
                .map_err(terminal_error)?;
        }
        let event = read_event()?;
        dirty = handle_event(&event, &mut front_end);
        if front_end.state.should_quit() {
            return Ok(());
        }
    }
}

/// Returns the unavailable error one terminal operation reports.
///
/// The terminal is the one thing the user can act on, so every terminal
/// failure - starting it, drawing a frame, reading an event - reports the same
/// stable message instead of an operating-system error the interface cannot
/// render.
fn terminal_error(_cause: std::io::Error) -> ErrorDto {
    ErrorDto::unavailable(
        "terminal_unavailable",
        "the terminal could not be started or drawn on",
    )
}

/// The terminal this front end owns for one run.
///
/// The alternate screen, raw mode, mouse capture, and bracketed paste are
/// entered together and left together: the guard restores the terminal on
/// every exit path, the error ones included, so a failed frame never leaves a
/// shell in raw mode.
struct Session {
    /// The terminal the frames are drawn on.
    terminal: DefaultTerminal,
}

impl Session {
    /// Enters the alternate screen and starts the terminal.
    ///
    /// Mouse capture is on so the wheel reaches this front end instead of the
    /// terminal's own scrollback. Bracketed paste is on so a paste arrives as
    /// one [`Event::Paste`] instead of a run of presses.
    fn start() -> DtoResult<Self> {
        let mut terminal = ratatui::try_init().map_err(terminal_error)?;
        execute!(
            terminal.backend_mut(),
            EnableMouseCapture,
            EnableBracketedPaste
        )
        .map_err(terminal_error)?;
        Ok(Self { terminal })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableBracketedPaste,
            DisableMouseCapture
        );
        ratatui::restore();
        let _ = execute!(std::io::stdout(), Show);
    }
}

/// One event the front end acts on.
///
/// The terminal's own vocabulary is flattened here so the front end's event
/// handling names only what it acts on: a key press, a mouse event, a paste,
/// the terminal's own resize and focus changes, and the tick the event loop
/// adds while no input arrives.
#[derive(Clone, Debug)]
enum Event {
    /// One key press.
    Key(KeyEvent),
    /// One mouse event.
    Mouse(MouseEvent),
    /// One whole bracketed paste.
    Paste(String),
    /// The terminal's size changed.
    Resize,
    /// The terminal window gained focus.
    FocusGained,
    /// The terminal window lost focus.
    FocusLost,
    /// The event loop's own clock.
    Tick,
}

/// Waits for one event and translates it, reporting a tick when none arrives.
///
/// A key release is not an event this front end acts on, so it is dropped
/// rather than reported; a terminal that never sends releases simply never
/// produces one.
fn read_event() -> DtoResult<Event> {
    if !event::poll(TICK).map_err(terminal_error)? {
        return Ok(Event::Tick);
    }
    loop {
        let event = match event::read().map_err(terminal_error)? {
            CrosstermEvent::Key(key)
                if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
            {
                Event::Key(key)
            }
            CrosstermEvent::Key(_) => continue,
            CrosstermEvent::Mouse(mouse) => Event::Mouse(mouse),
            CrosstermEvent::Paste(text) => Event::Paste(text),
            CrosstermEvent::Resize(..) => Event::Resize,
            CrosstermEvent::FocusGained => Event::FocusGained,
            CrosstermEvent::FocusLost => Event::FocusLost,
        };
        return Ok(event);
    }
}

/// Applies one event to the front end and reports whether a redraw is needed.
///
/// Nothing here decides what a session, transcript, input, or status line
/// holds: a key or mouse event becomes one or more [`Action`] values of the
/// shared core, the shared state machine turns them into [`crate::app::Effect`]
/// values, and the client task reports the [`Action`] values its work produced.
/// A tick only reports the turn's elapsed time, so a live status row repaints
/// while the run works.
///
/// # Pastes
///
/// A character the user types into the chat's line meets the paste detector
/// first, because a paste must reach the core as the one edit it is: presses
/// that follow one another faster than a hand can move collect into one run and
/// enter the line as one [`Action::InputPasted`], and a press a hand's own pause
/// separates enters it as the press it was. Nothing of a run reaches the line
/// while it is in flight, so a paste never paints the rows it is about to
/// replace. A bracketed paste - the terminal delivering the text whole - is one
/// edit too, and a large one waits for the rest of itself: a terminal hands a
/// paste over in chunks, and a multiplexer wraps every chunk in its own markers,
/// so the chunks of one paste join one run. Only the run's own silence, or the
/// user's next input, ends it - a tick never does, or a paste arriving in chunks
/// would become a paste per chunk. See the `paste` detector.
fn handle_event(event: &Event, front_end: &mut FrontEnd) -> bool {
    let now = Instant::now();
    let chat = front_end.state.screen() == Screen::Chat;
    let mut changed;
    // The chat's line is the one surface a paste arrives on: as typing, or as the
    // terminal's own event.
    let in_line = match event {
        Event::Key(key) if chat && carries_characters(key) => {
            let settled = key_action(key, &front_end.state)
                .and_then(|action| front_end.pastes.note_press(now, action));
            changed = front_end.settle_paste(settled);
            true
        }
        Event::Paste(text) if chat => {
            // One bracketed paste is one whole paste: the terminal held it until
            // its closing marker, so nothing waits for it and nothing merges it.
            // The run in flight is the user's own typing and enters the line
            // first.
            let settled = front_end.pastes.end();
            front_end.settle_paste(settled);
            front_end.apply(Action::InputPasted(text.clone()));
            changed = true;
            true
        }
        // The tick is the clock a run's silence is measured against, and a
        // resize or a focus change is the terminal's own work: none of them ends
        // a run a terminal is still writing.
        Event::Tick | Event::Resize | Event::FocusGained | Event::FocusLost => {
            let settled = front_end.pastes.settle(now);
            changed = front_end.settle_paste(settled);
            false
        }
        // Everything else is the user's own: the run in flight ends at once, so
        // the press or click that interrupted a paste reaches the line after it.
        _ => {
            let settled = front_end.pastes.end();
            changed = front_end.settle_paste(settled);
            false
        }
    };
    if !in_line {
        changed |= match event {
            Event::Key(key) => {
                let action = key_action(key, &front_end.state);
                let pressed = action.is_some();
                if let Some(action) = action {
                    front_end.apply(action);
                }
                pressed
            }
            Event::Mouse(mouse) => front_end.apply_mouse(mouse),
            // The line belongs to the chat: a paste while the browser or the
            // picker owns the window is not a line's text.
            Event::Paste(_) => false,
            // The terminal's own events repaint the frame: the one window is a
            // pure function of state and size, so a resize is a redraw and a
            // focus change leaves the window as it was.
            Event::Resize => true,
            Event::FocusGained | Event::FocusLost | Event::Tick => false,
        };
    }
    changed |= front_end.drain_actions();
    if matches!(event, Event::Tick) {
        changed |= front_end.tick();
    }
    changed
}

/// Returns whether one key press carries characters into the line before it.
///
/// These are the presses a paste delivered as typing is made of: its characters
/// and its line breaks. Every other key asks for something else, so it ends the
/// run in flight instead of continuing it.
fn carries_characters(key: &KeyEvent) -> bool {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    (matches!(key.code, KeyCode::Char(_)) && !control) || key.code == KeyCode::Enter
}

/// Returns the action one wheel event asks for on one screen, or `None` to
/// ignore it.
///
/// Only the wheel moves anything here: in the chat a notch moves the
/// transcript window one normal step, and in the sessions browser it moves the
/// cursor one row, exactly as the arrow keys do, so the browser's virtualized
/// window follows the cursor. Clicks, drags, and moves belong to the chat
/// pointer ([`Pointer`]), and the sessions browser carries no selection
/// behaviour.
const fn mouse_action(mouse: &MouseEvent, screen: Screen) -> Option<Action> {
    match (screen, mouse.kind) {
        (Screen::Chat, MouseEventKind::ScrollUp) => {
            Some(Action::ScrollTranscript(TranscriptScroll::Older))
        }
        (Screen::Chat, MouseEventKind::ScrollDown) => {
            Some(Action::ScrollTranscript(TranscriptScroll::Newer))
        }
        (Screen::Sessions, MouseEventKind::ScrollUp) => {
            Some(Action::BrowserCursorMove(BrowserCursorMove::Up))
        }
        (Screen::Sessions, MouseEventKind::ScrollDown) => {
            Some(Action::BrowserCursorMove(BrowserCursorMove::Down))
        }
        _ => None,
    }
}

/// Returns the terminal row one mouse event points at.
const fn mouse_row(mouse: &MouseEvent) -> u16 {
    mouse.row
}

/// The chat's left-button pointer, carried between mouse events.
///
/// The pointer owns no selection: it remembers the drag in progress - the
/// display row the press anchored, the row it last reached, whether the pointer
/// moved, and the collapsed reasoning marker the press hit - and reports the
/// actions one event asks for. The core owns the selection those actions leave,
/// and the pane paints it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Pointer {
    /// The drag in progress, while the left button is held.
    drag: Option<Drag>,
}

/// One left-button drag over the transcript.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Drag {
    /// The display row the press anchored the selection at.
    anchor: u16,
    /// The display row the pointer last reached.
    extent: u16,
    /// The collapsed reasoning block the press hit, when it hit a marker.
    marker: Option<usize>,
    /// Whether the pointer moved since the press.
    moved: bool,
}

impl Pointer {
    /// Returns the actions one mouse event asks for.
    ///
    /// `window` is the geometry the last frame published and `marker` is the
    /// collapsed reasoning block the pressed display row holds, resolved by the
    /// caller from the same frame's layout: only a press needs it.
    fn actions(
        &mut self,
        mouse: &MouseEvent,
        state: &AppState,
        window: TranscriptWindow,
        marker: Option<usize>,
    ) -> Vec<Action> {
        let screen = state.screen();
        if screen != Screen::Chat {
            // The browser owns every mouse event while it is up, and a drag
            // cannot outlive the screen it started on.
            self.drag = None;
            return mouse_action(mouse, screen).into_iter().collect();
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => self.press(mouse, window, marker),
            MouseEventKind::Drag(MouseButton::Left) => self.hold(mouse, window),
            MouseEventKind::Up(MouseButton::Left) => self.release(),
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if self.drag.is_some() => {
                self.hold(mouse, window)
            }
            _ => mouse_action(mouse, screen).into_iter().collect(),
        }
    }

    /// Applies one left-button press: it anchors the drag at the row it hits.
    ///
    /// A press on a displayed row anchors a one-row selection, which is the
    /// hit-test result of a click. A press on a collapsed reasoning block's
    /// marker selects nothing: releasing without a drag expands that block
    /// instead.
    fn press(
        &mut self,
        mouse: &MouseEvent,
        window: TranscriptWindow,
        marker: Option<usize>,
    ) -> Vec<Action> {
        let Some(row) = window.row_at(mouse_row(mouse)) else {
            self.drag = None;
            return Vec::new();
        };
        self.drag = Some(Drag {
            anchor: row,
            extent: row,
            marker,
            moved: false,
        });
        if marker.is_some() {
            return Vec::new();
        }
        vec![Action::SelectTranscriptRows {
            anchor: row,
            extent: row,
        }]
    }

    /// Applies one held-button event: a fast scroll and a selection extension.
    ///
    /// A wheel notch scrolls the window in its own direction, and a drag past
    /// an edge scrolls it towards that edge; both move it one fast step. The
    /// pointer itself is clamped to the window's nearest row, and the selection
    /// keeps its anchor, so dragging past an edge extends the selection to the
    /// rows the window scrolled past.
    fn hold(&mut self, mouse: &MouseEvent, window: TranscriptWindow) -> Vec<Action> {
        let Some(drag) = self.drag.as_mut() else {
            return Vec::new();
        };
        drag.moved = true;
        drag.marker = None;
        let mut actions = Vec::new();
        let window = fast_scroll(mouse, window).map_or(window, |direction| {
            actions.push(Action::ScrollTranscriptFast(direction));
            window.scrolled(direction, TRANSCRIPT_DRAG_ROWS)
        });
        drag.extent = window.clamped_row(mouse_row(mouse));
        actions.push(Action::SelectTranscriptRows {
            anchor: drag.anchor,
            extent: drag.extent,
        });
        actions
    }

    /// Applies one left-button release: a click on a marker expands its block.
    ///
    /// The selection a drag reported stays where the drag left it; a press and
    /// release without a drag leaves exactly the pressed row selected.
    fn release(&mut self) -> Vec<Action> {
        let Some(drag) = self.drag.take() else {
            return Vec::new();
        };
        if drag.moved {
            return Vec::new();
        }
        drag.marker.map_or_else(Vec::new, |message| {
            vec![Action::ExpandReasoning { row: Some(message) }]
        })
    }
}

/// Returns the fast scroll direction one held-button event asks for.
///
/// A wheel notch scrolls in its own direction; a drag scrolls towards the edge
/// the pointer is past, and sliding neither way asks for no scroll.
fn fast_scroll(mouse: &MouseEvent, window: TranscriptWindow) -> Option<TranscriptScroll> {
    let row = mouse_row(mouse);
    match mouse.kind {
        MouseEventKind::ScrollUp => Some(TranscriptScroll::Older),
        MouseEventKind::ScrollDown => Some(TranscriptScroll::Newer),
        _ if window.above(row) => Some(TranscriptScroll::Older),
        _ if window.below(row) => Some(TranscriptScroll::Newer),
        _ => None,
    }
}

/// The terminal front end: the shared state, its view, its client task, its one
/// turn clock, the chat's pointer, the paste detector, and the transcript's
/// layout cache and scroll surface.
struct FrontEnd {
    state: AppState,
    driver: Driver,
    timer: TurnTimer,
    /// The run of chat keystrokes that arrived too fast to have been typed.
    ///
    /// The detector holds the run's text until one event tells it whether the
    /// presses were a paste or a hand. See the `paste` detector.
    pastes: Pastes,
    /// The chat's left-button pointer, kept between mouse events.
    pointer: Pointer,
    /// The committed transcript's laid-out rows, kept between frames.
    cache: TranscriptLayoutCache,
    /// The scroll surface the transcript rows are painted into.
    surface: TranscriptSurface,
}

impl FrontEnd {
    /// Creates the front end of one session selection over one client task.
    ///
    /// `session` is the session this launch opens: the one the caller selected,
    /// or the one the client resolved for an explicit continuation. A launch
    /// that named neither opens none, and the user can still ask for a session
    /// later with `/new` or a row of the sessions browser. `theme` is the
    /// preference and detection answer the launch resolved, so the first frame
    /// already paints the palette they name.
    fn new(
        session: Option<SessionId>,
        workspace_root: WorkspaceRootDto,
        driver: Driver,
        theme: theme::ResolvedTheme,
    ) -> Self {
        Self {
            state: AppState::new(session)
                .with_theme(theme.preference(), theme.detected())
                .with_workspace_root(workspace_root),
            driver,
            timer: TurnTimer::new(),
            pastes: Pastes::new(),
            pointer: Pointer { drag: None },
            cache: TranscriptLayoutCache::new(),
            surface: TranscriptSurface::new(),
        }
    }

    /// Queues the effects that connect a freshly created front end.
    fn start(&self) {
        let startup = self.state.start();
        for effect in startup {
            self.driver.dispatch(effect);
        }
    }

    /// Applies one action and asks the client task for the effects it returns.
    ///
    /// The timer's two ends live here: the `SendTurn` effect the core returns
    /// starts it, and a terminal run status frame ends it. The final duration
    /// is the committed projection's own times, so the ended measurement is
    /// dropped instead of fed back into the core.
    fn apply(&mut self, action: Action) {
        let terminal = matches!(
            &action,
            Action::FrameReceived {
                frame: SessionFrameDto::Status(run),
                ..
            } if run_status_is_terminal(run.status())
        );
        let effects = self.state.update(action);
        if terminal {
            self.timer.stop();
        }
        for effect in effects {
            if matches!(&effect, Effect::SendTurn { .. }) {
                self.timer.start(Instant::now());
            }
            self.driver.dispatch(effect);
        }
    }

    /// Applies the action a paste run settled, and reports whether one did.
    ///
    /// A run settles as one paste, or as the single press a run of one press was,
    /// and both reach the core through the same path every other action takes.
    fn settle_paste(&mut self, settled: Option<Action>) -> bool {
        let Some(action) = settled else {
            return false;
        };
        self.apply(action);
        true
    }

    /// Applies every action the client task reported and reports whether any did.
    fn drain_actions(&mut self) -> bool {
        let mut changed = false;
        while let Some(action) = self.driver.take_action() {
            self.apply(action);
            changed = true;
        }
        changed
    }

    /// Applies one mouse event and reports whether a redraw is needed.
    ///
    /// The event hit-tests the window the last frame published, and the marker
    /// the pressed display row holds is resolved from that same frame's layout;
    /// everything else is the pointer's own arithmetic.
    fn apply_mouse(&mut self, mouse: &MouseEvent) -> bool {
        let window = self.cache.window();
        let marker = match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => window
                .row_at(mouse_row(mouse))
                .and_then(|row| self.marker_at(row)),
            _ => None,
        };
        let actions = self.pointer.actions(mouse, &self.state, window, marker);
        let changed = !actions.is_empty();
        for action in actions {
            self.apply(action);
        }
        changed
    }

    /// Returns the collapsed reasoning block one display row's marker expands.
    ///
    /// A marker row inside the committed rows names that row; the live tail's
    /// marker is not cached, so the row the pane published for it names the row
    /// the live segment will become.
    fn marker_at(&self, row: u16) -> Option<usize> {
        marker_target(
            &self.cache,
            self.state.live_reasoning_row(),
            usize::from(row),
        )
    }

    /// Reports the live turn's elapsed time when a tenth of a second changed.
    fn tick(&mut self) -> bool {
        let Some(millis) = self.timer.due(Instant::now()) else {
            return false;
        };
        self.apply(Action::ElapsedReported { millis });
        true
    }

    /// Paints one frame of the one window.
    fn render(&mut self, frame: &mut Frame) {
        TerminalView::new(&self.state, &mut self.cache, &mut self.surface).render(frame);
    }
}

/// Returns the reasoning block one display row's expand marker belongs to.
///
/// A marker row inside the committed rows names the committed row that owns it.
/// The live tail is laid out per frame and never enters the cache, so a row the
/// frame published as the live marker names `live_row` instead: the row the
/// live segment will become, which is the anchor its expansion state uses.
fn marker_target(cache: &TranscriptLayoutCache, live_row: usize, row: usize) -> Option<usize> {
    if cache.window().live_reasoning_marker.map(usize::from) == Some(row) {
        return Some(live_row);
    }
    if !cache
        .laid_out()
        .get(row)
        .is_some_and(LaidOutRow::is_reasoning_marker)
    {
        return None;
    }
    cache.message_at(row)
}

/// The fraction of a second one live timer report covers.
const MILLIS_PER_TENTH: u64 = 100;

/// The front end's clock for one dispatched turn.
///
/// It measures from the instant the front end dispatches `SendTurn` to the
/// instant the run reports a terminal status, and reports changed tenths while
/// the turn is live - the one granularity a running status row shows. The final
/// value is not reported: the committed run projection carries the run's own
/// start and finish times, and the status row reads those in whole seconds.
struct TurnTimer {
    /// The instant the current turn was dispatched, while one is in flight.
    started: Option<Instant>,
    /// The last elapsed value reported, so a tick reports only changes.
    reported_millis: Option<u64>,
}

impl TurnTimer {
    /// Creates a timer with no turn in flight.
    const fn new() -> Self {
        Self {
            started: None,
            reported_millis: None,
        }
    }

    /// Starts a measurement for the turn just dispatched.
    const fn start(&mut self, now: Instant) {
        self.started = Some(now);
        self.reported_millis = None;
    }

    /// Returns the elapsed value to report at `now`, if the displayed tenth of
    /// a second changed since the last report.
    fn due(&mut self, now: Instant) -> Option<u64> {
        let millis = elapsed_millis(self.started?, now)?;
        if self
            .reported_millis
            .is_some_and(|reported| reported / MILLIS_PER_TENTH == millis / MILLIS_PER_TENTH)
        {
            return None;
        }
        self.reported_millis = Some(millis);
        Some(millis)
    }

    /// Ends the measurement of a terminated turn, keeping no value.
    ///
    /// The committed run's start and finish times are the final duration the
    /// status row shows, so the front end's own final measurement is dropped
    /// and a later tick reports nothing.
    const fn stop(&mut self) {
        self.started = None;
        self.reported_millis = None;
    }
}

/// Returns the whole milliseconds between two instants.
fn elapsed_millis(started: Instant, now: Instant) -> Option<u64> {
    u64::try_from(now.saturating_duration_since(started).as_millis()).ok()
}

#[cfg(test)]
mod tests {

    use std::time::{Duration, Instant};

    use intention_proto::{
        DaemonHealthDto, ProjectId, RunModeDto, SessionId, SessionSummariesDto, SessionSummaryDto,
        TimestampDto, WorkspaceId, WorkspaceRootDto,
    };
    use ratatui::crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    use crate::app::{Action, AppState, Screen, TRANSCRIPT_DRAG_ROWS, TranscriptScroll};
    use crate::tui::layout::{TranscriptLayoutCache, TranscriptWindow};
    use crate::tui::testing::Screen as TestScreen;

    use super::{Pointer, TurnTimer, marker_target, mouse_action};

    /// Returns the transcript window the pointer tests hit-test.
    ///
    /// The window shows ten rows starting at screen row four, with `start` as
    /// its first display row out of `total`.
    const fn window(start: u16, total: u16) -> TranscriptWindow {
        TranscriptWindow {
            top: 4,
            visible: 10,
            start,
            total,
            live_reasoning_marker: None,
        }
    }

    /// Returns one mouse event of `kind` at one screen row.
    const fn mouse(kind: MouseEventKind, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: 1,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    /// Applies one wheel notch to `state` the way the event loop does.
    ///
    /// Returns whether the event asked for an action at all, so a test can tell
    /// a notch that moved something from one the screen ignores.
    fn wheel(state: &mut AppState, kind: MouseEventKind) -> bool {
        let action = mouse_action(&mouse(kind, 1), state.screen());
        let Some(action) = action else {
            return false;
        };
        state.update(action);
        true
    }

    #[test]
    fn the_published_live_marker_names_the_row_the_live_segment_becomes() {
        let mut cache = TranscriptLayoutCache::new();
        assert_eq!(
            marker_target(&cache, 7, 0),
            None,
            "no painted frame publishes a live marker"
        );

        cache.publish_window(TranscriptWindow {
            top: 4,
            visible: 10,
            start: 0,
            total: 3,
            live_reasoning_marker: Some(1),
        });
        assert_eq!(
            marker_target(&cache, 7, 1),
            Some(7),
            "the row the pane published as the live marker expands the live segment"
        );
        assert_eq!(
            marker_target(&cache, 7, 0),
            None,
            "a row with no marker expands nothing"
        );
    }

    /// Returns one session summary with a deterministic id and timestamp.
    fn summary(index: usize) -> SessionSummaryDto {
        let literal = format!("{index:08}-1111-4111-8111-111111111111");
        let updated_at = TimestampDto::from_unix_seconds(1_759_000_000 - index as i64 * 60)
            .expect("the fixture timestamp is valid");
        SessionSummaryDto::new(
            SessionId::parse(&literal).expect("the fixture session id is canonical"),
            ProjectId::new(),
            WorkspaceId::new(),
            RunModeDto::Build,
            updated_at,
            updated_at,
            WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
                .expect("the fixture workspace root is absolute"),
            0,
        )
    }

    /// Returns a state with the sessions browser open over `count` sessions.
    fn browser_state(count: usize) -> AppState {
        let mut state = AppState::new(None);
        state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
        let sessions = (0..count).map(summary).collect();
        state.update(Action::SessionsListed(
            SessionSummariesDto::new(sessions, 0).expect("the fixture session list is coherent"),
        ));
        state.update(Action::SessionsBrowserRequested);
        state
    }

    #[test]
    fn the_turn_timer_reports_each_changed_tenth_of_a_second() {
        let start = Instant::now();
        let mut timer = TurnTimer::new();
        assert_eq!(timer.due(start), None, "no turn is in flight");
        timer.start(start);
        assert_eq!(timer.due(start), Some(0));
        assert_eq!(
            timer.due(start + Duration::from_millis(50)),
            None,
            "the same tenth of a second is not reported twice"
        );
        assert_eq!(timer.due(start + Duration::from_millis(125)), Some(125));
        assert_eq!(timer.due(start + Duration::from_millis(199)), None);
        assert_eq!(timer.due(start + Duration::from_millis(200)), Some(200));
    }

    #[test]
    fn the_turn_timer_ends_with_the_runs_terminal_status() {
        let start = Instant::now();
        let mut timer = TurnTimer::new();
        timer.start(start);
        timer.stop();
        assert_eq!(
            timer.due(start + Duration::from_millis(13_000)),
            None,
            "a finished turn reports no further tenths"
        );
    }

    #[test]
    fn wheel_events_scroll_the_transcript_and_other_mouse_events_do_not() {
        let mut state = AppState::new(None);
        assert!(wheel(&mut state, MouseEventKind::ScrollUp));
        assert_eq!(state.scroll(), 3, "one notch moves three rows");
        wheel(&mut state, MouseEventKind::ScrollDown);
        assert_eq!(state.scroll(), 0, "a notch back reaches the newest rows");
        assert!(
            !wheel(&mut state, MouseEventKind::Down(MouseButton::Left)),
            "a click asks for no action of its own"
        );
        assert_eq!(state.scroll(), 0, "a click moves nothing");
    }

    #[test]
    fn a_wheel_notch_moves_the_browser_cursor_and_its_window() {
        let mut state = browser_state(12);
        assert!(
            TestScreen::view(&state, 100, 30)
                .screen_text()
                .contains("1-5 of 12"),
            "the window starts at the newest sessions"
        );
        wheel(&mut state, MouseEventKind::ScrollDown);
        assert_eq!(
            state.browser_cursor(),
            1,
            "a wheel notch moves the cursor one row"
        );
        for _ in 0..5 {
            wheel(&mut state, MouseEventKind::ScrollDown);
        }
        assert_eq!(state.browser_cursor(), 6);
        assert_eq!(state.screen(), Screen::Sessions);
        assert!(
            TestScreen::view(&state, 100, 30)
                .screen_text()
                .contains("3-7 of 12"),
            "the virtualized window follows the wheeled cursor"
        );
        wheel(&mut state, MouseEventKind::ScrollUp);
        assert_eq!(state.browser_cursor(), 5);
        for _ in 0..10 {
            wheel(&mut state, MouseEventKind::ScrollUp);
        }
        assert_eq!(
            state.browser_cursor(),
            0,
            "the cursor stops at the newest session"
        );
    }

    /// Every crate source that could carry a copy path, read at compile time.
    const CLIPBOARD_FREE_SOURCES: [(&str, &str); 33] = [
        ("Cargo.toml", include_str!("../../Cargo.toml")),
        ("src/lib.rs", include_str!("../lib.rs")),
        ("src/main.rs", include_str!("../main.rs")),
        ("src/cli.rs", include_str!("../cli.rs")),
        ("src/headless.rs", include_str!("../headless.rs")),
        ("src/repl.rs", include_str!("../repl.rs")),
        ("src/client_task.rs", include_str!("../client_task.rs")),
        ("src/app/action.rs", include_str!("../app/action.rs")),
        ("src/app/browser.rs", include_str!("../app/browser.rs")),
        ("src/app/ctrl_c.rs", include_str!("../app/ctrl_c.rs")),
        ("src/app/input.rs", include_str!("../app/input.rs")),
        ("src/app/mod.rs", include_str!("../app/mod.rs")),
        ("src/app/run.rs", include_str!("../app/run.rs")),
        ("src/app/sessions.rs", include_str!("../app/sessions.rs")),
        ("src/app/state.rs", include_str!("../app/state.rs")),
        ("src/app/tests.rs", include_str!("../app/tests.rs")),
        (
            "src/app/transcript.rs",
            include_str!("../app/transcript.rs"),
        ),
        ("src/tui/driver.rs", include_str!("driver.rs")),
        ("src/tui/keymap.rs", include_str!("keymap.rs")),
        ("src/tui/layout.rs", include_str!("layout.rs")),
        ("src/tui/mod.rs", include_str!("mod.rs")),
        ("src/tui/palette.rs", include_str!("palette.rs")),
        ("src/tui/panes/input.rs", include_str!("panes/input.rs")),
        (
            "src/tui/panes/markdown.rs",
            include_str!("panes/markdown.rs"),
        ),
        ("src/tui/panes/mod.rs", include_str!("panes/mod.rs")),
        ("src/tui/panes/status.rs", include_str!("panes/status.rs")),
        ("src/tui/panes/tools.rs", include_str!("panes/tools.rs")),
        (
            "src/tui/panes/transcript.rs",
            include_str!("panes/transcript.rs"),
        ),
        ("src/tui/screens/chat.rs", include_str!("screens/chat.rs")),
        ("src/tui/screens/mod.rs", include_str!("screens/mod.rs")),
        ("src/tui/surface.rs", include_str!("surface.rs")),
        ("src/tui/testing.rs", include_str!("testing.rs")),
        (
            "src/tui/screens/sessions.rs",
            include_str!("screens/sessions.rs"),
        ),
    ];

    #[test]
    fn the_front_end_owns_no_clipboard_path() {
        // The audited names are assembled from pieces so this audit's own
        // source cannot trip it: only a real dependency, call, or escape
        // sequence in an audited source can. The sources may name copying in
        // their own documentation - they must never carry a way to do it.
        let banned = [
            concat!("ar", "board"),
            concat!("copy", "pasta"),
            concat!("clipboard", "::"),
            concat!("set_", "clip", "board"),
            concat!("osc", "52"),
            concat!("OSC", "52"),
            concat!("]", "52"),
            concat!("xc", "lip"),
            concat!("wl-", "copy"),
        ];
        for (file, source) in CLIPBOARD_FREE_SOURCES {
            for token in banned {
                assert!(
                    !source.contains(token),
                    "{token} must not appear in {file}: copying is the terminal emulator's business"
                );
            }
        }
    }

    #[test]
    fn a_press_drag_and_release_select_the_display_rows_the_pointer_reaches() {
        let mut pointer = Pointer::default();
        let state = AppState::new(None);
        let window = window(20, 100);
        assert_eq!(
            pointer.actions(
                &mouse(MouseEventKind::Down(MouseButton::Left), 6),
                &state,
                window,
                None
            ),
            vec![Action::SelectTranscriptRows {
                anchor: 22,
                extent: 22
            }],
            "a press anchors a one-row selection at the row it hits"
        );
        assert_eq!(
            pointer.actions(
                &mouse(MouseEventKind::Drag(MouseButton::Left), 11),
                &state,
                window,
                None
            ),
            vec![Action::SelectTranscriptRows {
                anchor: 22,
                extent: 27
            }],
            "a drag extends the selection to the row it reaches"
        );
        assert!(
            pointer
                .actions(
                    &mouse(MouseEventKind::Up(MouseButton::Left), 11),
                    &state,
                    window,
                    None
                )
                .is_empty(),
            "a release keeps the selection the drag reported"
        );
    }

    #[test]
    fn a_click_away_from_a_marker_only_hit_tests_the_pressed_row() {
        let mut pointer = Pointer::default();
        let state = AppState::new(None);
        let window = window(20, 100);
        assert_eq!(
            pointer.actions(
                &mouse(MouseEventKind::Down(MouseButton::Left), 6),
                &state,
                window,
                None
            ),
            vec![Action::SelectTranscriptRows {
                anchor: 22,
                extent: 22
            }],
            "a click selects exactly the pressed row, never a whole block"
        );
        assert!(
            pointer
                .actions(
                    &mouse(MouseEventKind::Up(MouseButton::Left), 6),
                    &state,
                    window,
                    None
                )
                .is_empty(),
            "and the release expands nothing"
        );
    }

    #[test]
    fn a_press_outside_the_window_anchors_nothing() {
        let mut pointer = Pointer::default();
        let state = AppState::new(None);
        let window = window(20, 100);
        assert!(
            pointer
                .actions(
                    &mouse(MouseEventKind::Down(MouseButton::Left), 2),
                    &state,
                    window,
                    None
                )
                .is_empty(),
            "a press above the window hits no display row"
        );
        assert!(
            pointer
                .actions(
                    &mouse(MouseEventKind::Drag(MouseButton::Left), 8),
                    &state,
                    window,
                    None
                )
                .is_empty(),
            "a drag without a press moves nothing"
        );
    }

    #[test]
    fn a_press_and_release_without_a_drag_expands_the_marker_it_hits() {
        let mut pointer = Pointer::default();
        let state = AppState::new(None);
        let window = window(20, 100);
        assert!(
            pointer
                .actions(
                    &mouse(MouseEventKind::Down(MouseButton::Left), 6),
                    &state,
                    window,
                    Some(3)
                )
                .is_empty(),
            "a press on a marker selects nothing"
        );
        assert_eq!(
            pointer.actions(
                &mouse(MouseEventKind::Up(MouseButton::Left), 6),
                &state,
                window,
                None
            ),
            vec![Action::ExpandReasoning { row: Some(3) }],
            "a click on a marker expands that block"
        );
    }

    #[test]
    fn a_marker_press_that_drags_selects_instead_of_expanding() {
        let mut pointer = Pointer::default();
        let state = AppState::new(None);
        let window = window(20, 100);
        pointer.actions(
            &mouse(MouseEventKind::Down(MouseButton::Left), 6),
            &state,
            window,
            Some(3),
        );
        assert_eq!(
            pointer.actions(
                &mouse(MouseEventKind::Drag(MouseButton::Left), 9),
                &state,
                window,
                None
            ),
            vec![Action::SelectTranscriptRows {
                anchor: 22,
                extent: 25
            }],
            "a drag from a marker selects the rows it reaches"
        );
        assert!(
            pointer
                .actions(
                    &mouse(MouseEventKind::Up(MouseButton::Left), 9),
                    &state,
                    window,
                    None
                )
                .is_empty(),
            "the pointer moved, so the release does not expand"
        );
    }

    #[test]
    fn a_drag_past_the_top_edge_scrolls_one_fast_step_and_extends_the_selection() {
        let mut pointer = Pointer::default();
        let state = AppState::new(None);
        let window = window(20, 100);
        pointer.actions(
            &mouse(MouseEventKind::Down(MouseButton::Left), 6),
            &state,
            window,
            None,
        );
        assert_eq!(
            pointer.actions(
                &mouse(MouseEventKind::Drag(MouseButton::Left), 1),
                &state,
                window,
                None
            ),
            vec![
                Action::ScrollTranscriptFast(TranscriptScroll::Older),
                Action::SelectTranscriptRows {
                    anchor: 22,
                    extent: 5
                },
            ],
            "the window moves one fast step and the selection follows it"
        );
        assert_eq!(
            TRANSCRIPT_DRAG_ROWS, 15,
            "the fast step is five wheel notches of three rows"
        );
    }

    #[test]
    fn a_wheel_notch_while_dragging_uses_the_fast_step_and_a_plain_one_keeps_the_normal_step() {
        let mut pointer = Pointer::default();
        let state = AppState::new(None);
        let window = window(20, 100);
        assert_eq!(
            pointer.actions(&mouse(MouseEventKind::ScrollUp, 6), &state, window, None),
            vec![Action::ScrollTranscript(TranscriptScroll::Older)],
            "a plain wheel notch keeps the normal step"
        );
        pointer.actions(
            &mouse(MouseEventKind::Down(MouseButton::Left), 6),
            &state,
            window,
            None,
        );
        assert_eq!(
            pointer.actions(&mouse(MouseEventKind::ScrollUp, 6), &state, window, None),
            vec![
                Action::ScrollTranscriptFast(TranscriptScroll::Older),
                Action::SelectTranscriptRows {
                    anchor: 22,
                    extent: 7
                },
            ],
            "a notch while the button is held adds the fast step"
        );
    }

    #[test]
    fn the_keys_of_one_flow_drive_the_hint_menu_from_a_slash_to_the_command() {
        use crate::app::Effect;
        use crate::tui::key_action;
        use crate::tui::panes::commands;

        let mut state = AppState::new(None);
        let mut rows = Vec::new();
        let mut effects = Vec::new();
        for key in [
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        ] {
            let action = key_action(&key, &state).expect("each key of the flow asks for an action");
            effects = state.update(action);
            rows.push(commands::menu_rows(&state));
        }
        assert_eq!(
            rows,
            vec![7, 5, 0, 0],
            "the slash opens the band, the narrowed list keeps it, and the commit closes it"
        );
        assert_eq!(
            effects,
            vec![Effect::CreateSession { first_turn: None }],
            "the second Enter of the flow runs the command the menu completed"
        );
        assert_eq!(state.input(), "", "the command left the input line empty");
        assert_eq!(state.notice(), Some("creating a session"));
    }
}
