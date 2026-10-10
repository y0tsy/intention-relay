//! The blocking client task one revue front end drives.
//!
//! The task owns the connection: it performs the request effects the core
//! returns through the shared [`client_task`], and it owns the one live run
//! subscription a front end keeps. Every client call itself lives in
//! [`client_task`], so this module holds the wiring only.

use std::thread;

use intention_client::{IntentionClient, RunStreamSubscription};
use intention_proto::{
    DtoResult, ErrorDto, RunModeDto, RunStreamFrameDto, SessionId, WorkspaceRootDto,
};
use intention_transport::LocalEndpoint;
use tokio::sync::mpsc;

use crate::app::{Action, Effect};
use crate::client_task;

/// The startup choices the command line hands to the fullscreen front end.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TuiOptions {
    workspace_root: WorkspaceRootDto,
    mode: RunModeDto,
    session: Option<SessionId>,
    continue_session: bool,
}

impl TuiOptions {
    /// Creates the front-end options for one workspace, run mode, and session.
    ///
    /// `session` is the session the caller selected; `None` opens nothing, so
    /// the launch shows the welcome state until the user asks for a session. A
    /// caller that wants the newest session opened asks for it explicitly with
    /// [`TuiOptions::continuing`].
    #[must_use]
    pub const fn new(
        workspace_root: WorkspaceRootDto,
        mode: RunModeDto,
        session: Option<SessionId>,
    ) -> Self {
        Self {
            workspace_root,
            mode,
            session,
            continue_session: false,
        }
    }

    /// Returns these options asking to continue the newest session, or not.
    ///
    /// The request is explicit - the command line's `--continue` - and it is
    /// the only launch request that opens a session the caller did not name.
    #[must_use]
    pub const fn continuing(mut self, continue_session: bool) -> Self {
        self.continue_session = continue_session;
        self
    }

    /// Returns the workspace root a new session is created with.
    #[must_use]
    pub const fn workspace_root(&self) -> &WorkspaceRootDto {
        &self.workspace_root
    }

    /// Returns the run mode a new session is created with.
    #[must_use]
    pub const fn mode(&self) -> RunModeDto {
        self.mode
    }

    /// Returns the session the caller selected, if any.
    #[must_use]
    pub const fn session(&self) -> Option<SessionId> {
        self.session
    }

    /// Returns whether the caller asked to continue the most recent session.
    #[must_use]
    pub const fn continue_session(&self) -> bool {
        self.continue_session
    }
}

/// One step of the client task: an effect to perform, or a live frame.
enum Step {
    /// The next effect a front end asked for, or `None` once it left.
    Effect(Option<Effect>),
    /// The next committed frame, or the end of the live subscription.
    Frame(DtoResult<Option<RunStreamFrameDto>>),
}

/// The client task one revue front end drives through [`Effect`] values.
pub(super) struct Driver {
    effects: mpsc::UnboundedSender<Effect>,
    actions: mpsc::UnboundedReceiver<Action>,
}

impl Driver {
    /// Starts the client task that owns the connection for one front end.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the client runtime cannot be created.
    pub(super) fn spawn(
        client: IntentionClient,
        endpoint: LocalEndpoint,
        workspace_root: WorkspaceRootDto,
        mode: RunModeDto,
    ) -> DtoResult<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| {
                ErrorDto::unavailable(
                    "terminal_runtime_unavailable",
                    "the terminal runtime could not be created",
                )
            })?;
        let (effects, effect_requests) = mpsc::unbounded_channel();
        let (actions, action_events) = mpsc::unbounded_channel();
        thread::spawn(move || {
            runtime.block_on(drive(
                client,
                endpoint,
                workspace_root,
                mode,
                effect_requests,
                actions,
            ));
        });
        Ok(Self {
            effects,
            actions: action_events,
        })
    }

    /// Queues one effect for the client task.
    ///
    /// A closed queue means the front end is leaving, so the effect is dropped
    /// instead of reported.
    pub(super) fn dispatch(&self, effect: Effect) {
        // @todo(hack): a closed queue silently drops the effect because the
        // driver carries no closed signal; a typed driver failure would let the
        // front end stop instead of losing the request.
        let _ = self.effects.send(effect);
    }

    /// Takes the next action the client task reported, if one is waiting.
    pub(super) fn take_action(&mut self) -> Option<Action> {
        self.actions.try_recv().ok()
    }
}

/// Performs the effects one front end asks for and forwards the live frames.
///
/// One task owns the connection: it performs a request effect when one arrives
/// and otherwise waits on the live subscription, so the front end keeps a
/// single ordered path from a key press to a client call.
async fn drive(
    client: IntentionClient,
    endpoint: LocalEndpoint,
    workspace_root: WorkspaceRootDto,
    mode: RunModeDto,
    mut effects: mpsc::UnboundedReceiver<Effect>,
    actions: mpsc::UnboundedSender<Action>,
) {
    let mut live: Option<RunStreamSubscription> = None;
    loop {
        let step = match live.as_mut() {
            Some(subscription) => tokio::select! {
                effect = effects.recv() => Step::Effect(effect),
                received = subscription.receive() => Step::Frame(received),
            },
            None => Step::Effect(effects.recv().await),
        };
        let action = match step {
            Step::Effect(None) => return,
            Step::Effect(Some(Effect::CloseStream)) => {
                // The task owns the one live subscription, so the front end
                // unsubscribes by asking for the drop instead of holding a
                // stream the core would only discard frames from.
                // @todo(hack): the effect vocabulary carries no unsubscribe, so
                // the driver models one by dropping the subscription it owns;
                // give the core a typed unsubscribe instead.
                live = None;
                None
            }
            Step::Effect(Some(Effect::Subscribe { session_id, run_id })) => {
                live = None;
                match client_task::open_subscription(&endpoint, session_id, run_id).await {
                    Ok(subscription) => {
                        if actions
                            .send(Action::RunStreamOpened(subscription.state().clone()))
                            .is_err()
                        {
                            return;
                        }
                        live = Some(subscription);
                        None
                    }
                    Err(error) => Some(Action::RunStreamFailed(error)),
                }
            }
            Step::Effect(Some(effect)) => {
                client_task::perform(&client, &workspace_root, mode, effect)
                    .await
                    .map(|outcome| outcome.unwrap_or_else(|(action, _failure)| action))
            }
            Step::Frame(Ok(Some(frame))) => Some(Action::FrameReceived(frame)),
            Step::Frame(Ok(None)) => {
                live = None;
                Some(Action::RunStreamEnded)
            }
            Step::Frame(Err(error)) => {
                live = None;
                Some(Action::RunStreamFailed(error))
            }
        };
        if let Some(action) = action
            && actions.send(action).is_err()
        {
            return;
        }
    }
}
