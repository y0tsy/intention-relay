//! The one async client-call mapper every terminal front end shares.
//!
//! Every terminal mode turns the same [`Effect`] values into the same shared
//! client calls, and every failure becomes the same core [`Action`] value. This
//! module is the single place a new client-backed behavior is added: a request
//! effect gains exactly one arm in [`perform`], and a subscription-shaped
//! behavior gains an opener beside [`open_session_subscription`]. A live subscription is
//! a resource of the front end that keeps it, so the opener returns it to its
//! caller instead of hiding it here.

use intention_client::{IntentionClient, SessionStreamClient, SessionStreamSubscription};
use intention_proto::{
    CreateSessionCommandDto, CreateSessionFirstTurnDto, ErrorDto, ProjectId, RunModeDto, SessionId,
    SubscribeSessionCommandDto, WorkspaceId, WorkspaceRootDto,
};
use intention_transport::LocalEndpoint;

use crate::app::{Action, Effect};

/// Performs one request effect over the shared client.
///
/// Returns `None` for the effects that carry no client call: a live session
/// subscription is a front-end resource, so a subscriber opens it with
/// [`open_session_subscription`] and keeps it until it replaces it.
///
/// The `Err` arm carries the core's failure action together with the typed
/// error, so a front end that reports a process status can keep the error while
/// every front end feeds the action back into the core.
#[must_use]
pub async fn perform(
    client: &IntentionClient,
    workspace_root: &WorkspaceRootDto,
    mode: Option<RunModeDto>,
    effect: Effect,
) -> Option<Result<Action, (Action, ErrorDto)>> {
    let outcome = match effect {
        Effect::Connect => client
            .connect_or_bootstrap()
            .await
            .map(Action::Bootstrapped)
            .map_err(|error| (Action::BootstrapFailed(error.clone()), error)),
        Effect::ListSessions => client
            .list_sessions()
            .await
            .map(Action::SessionsListed)
            .map_err(|error| (Action::SessionsListFailed(error.clone()), error)),
        Effect::OpenSession(session_id) => client
            .session_snapshot(session_id)
            .await
            .map(Action::SessionSnapshotLoaded)
            .map_err(|error| (Action::SessionSnapshotFailed(error.clone()), error)),
        Effect::PersistTheme(_) => {
            // The local settings file is the fullscreen front end's own work:
            // no client call carries a theme, and no other terminal mode has a
            // theme to write.
            return None;
        }
        Effect::CreateSession { first_turn } => {
            let carried_first_turn = first_turn.is_some();
            let first_turn = match first_turn {
                Some(content) => {
                    let turn = match CreateSessionFirstTurnDto::new(content) {
                        Ok(turn) => turn,
                        Err(error) => {
                            return Some(Err((
                                Action::SessionCreateFailed {
                                    carried_first_turn,
                                    error: error.clone(),
                                },
                                error,
                            )));
                        }
                    };
                    Some(turn)
                }
                None => None,
            };
            let command = CreateSessionCommandDto::new(
                ProjectId::new(),
                SessionId::new(),
                WorkspaceId::new(),
                workspace_root.clone(),
                mode,
                first_turn,
            );
            client
                .create_session(command)
                .await
                .map(|created| Action::SessionCreated(created.session_id()))
                .map_err(|error| {
                    (
                        Action::SessionCreateFailed {
                            carried_first_turn,
                            error: error.clone(),
                        },
                        error,
                    )
                })
        }
        Effect::SendTurn {
            session_id,
            content,
        } => client
            .send_user_turn(session_id, content)
            .await
            .map(Action::TurnAccepted)
            .map_err(|error| (Action::TurnFailed(error.clone()), error)),
        Effect::CancelRun { session_id, run_id } => client
            .cancel_run(session_id, run_id)
            .await
            .map(|_accepted| Action::RunCancelAccepted)
            .map_err(|error| (Action::RunCancelFailed(error.clone()), error)),
        Effect::Subscribe { .. } => return None,
    };
    Some(outcome)
}

/// Opens one live session subscription for the front end that keeps it.
///
/// # Errors
///
/// Returns the typed failure when the subscription cannot be opened.
pub async fn open_session_subscription(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
) -> Result<SessionStreamSubscription, ErrorDto> {
    SessionStreamClient::new(endpoint.clone())
        .subscribe(SubscribeSessionCommandDto::new(session_id))
        .await
}
