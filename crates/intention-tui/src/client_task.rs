//! The one async client-call mapper every terminal front end shares.
//!
//! Every terminal mode turns the same [`Effect`] values into the same shared
//! client calls, and every failure becomes the same core [`Action`] value. This
//! module is the single place a new client-backed behavior is added: a request
//! effect gains exactly one arm in [`perform`], and a subscription-shaped
//! behavior gains an opener beside [`open_subscription`]. A live subscription is
//! a resource of the front end that keeps it, so the opener returns it to its
//! caller instead of hiding it here.

use intention_client::{IntentionClient, RunStreamClient, RunStreamSubscription};
use intention_proto::{
    CreateSessionCommandDto, ErrorDto, IdempotencyKey, ProjectId, RunId, RunModeDto, SessionId,
    SubscribeRunCommandDto, WorkspaceId, WorkspaceRootDto,
};
use intention_transport::LocalEndpoint;

use crate::app::{Action, Effect};

/// Performs one request effect over the shared client.
///
/// Returns `None` for the effects that carry no client call: a live run
/// subscription is a front-end resource, so a subscriber opens it with
/// [`open_subscription`] and keeps it, and [`Effect::CloseStream`] only drops
/// the subscription the front end already holds.
///
/// The `Err` arm carries the core's failure action together with the typed
/// error, so a front end that reports a process status can keep the error while
/// every front end feeds the action back into the core.
#[must_use]
pub async fn perform(
    client: &IntentionClient,
    workspace_root: &WorkspaceRootDto,
    mode: RunModeDto,
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
        Effect::CreateSession => {
            let command = CreateSessionCommandDto::new(
                ProjectId::new(),
                SessionId::new(),
                WorkspaceId::new(),
                workspace_root.clone(),
                mode,
            );
            client
                .create_session(command)
                .await
                .map(|created| Action::SessionCreated(created.session_id()))
                .map_err(|error| (Action::SessionCreateFailed(error.clone()), error))
        }
        Effect::SendTurn {
            session_id,
            content,
        } => client
            .send_user_turn(session_id, IdempotencyKey::new(), content)
            .await
            .map(Action::TurnAccepted)
            .map_err(|error| (Action::TurnFailed(error.clone()), error)),
        Effect::Interrupt { session_id, run_id } => client
            .interrupt_run(session_id, run_id)
            .await
            .map(|_accepted| Action::InterruptAccepted)
            .map_err(|error| (Action::InterruptFailed(error.clone()), error)),
        Effect::Subscribe { .. } | Effect::CloseStream => return None,
    };
    Some(outcome)
}

/// Opens one live run subscription for the front end that keeps it.
///
/// # Errors
///
/// Returns the typed failure when the subscription cannot be opened.
pub async fn open_subscription(
    endpoint: &LocalEndpoint,
    session_id: SessionId,
    run_id: RunId,
) -> Result<RunStreamSubscription, ErrorDto> {
    RunStreamClient::new(endpoint.clone())
        .subscribe(SubscribeRunCommandDto::new(session_id, run_id))
        .await
}
