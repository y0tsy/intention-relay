//! Application command/query orchestration over DTO-only current-state storage.
//!
//! This crate maps committed repository outcomes into protocol-ready DTOs. It
//! neither owns database resources nor reimplements repository idempotency.
//! Every tool call commits its `tool_call` row before dispatch and exactly one
//! terminal result row with its answering `tool_result` message afterwards.

mod context_window;
mod runtime;

pub use crate::runtime::*;

use std::collections::{BTreeMap, BTreeSet};

use intention_config::ConfigSnapshotDto;
use intention_proto::{
    CreateSessionCommandDto, CreateSessionFirstTurnDto, MessageKindDto, MessageProjectionDto,
    NewMessageDto, PendingTurnProjectionDto, RemoveTurnCommandDto, RunProjectionDto,
    SendUserTurnCommandDto, SessionProjectionDto, ToolCallRowDto, ToolIdentityDto, ToolInput,
    ToolResult, ToolResultMetadataEntryDto, ToolResultRowDto, ToolResultStatusDto, ToolRowDto,
};
use intention_proto::{DtoResult, ErrorDto, RunId, SessionId, TimestampDto};
use intention_proto::{ToolCallDto, ToolCallId};
use intention_storage::{AcceptedTurnOutcomeDto, StorageRepositoryDto};
use intention_tools::{
    InterruptCause, ToolDispatchOutcome, ToolService, WorkspaceRoot, partial_tool_result_content,
    render_tool_result_content, tool_failure_guidance,
};

/// Complete engine command for one local tool invocation.
#[derive(Debug)]
pub struct ToolInvocationRequestDto {
    workspace: WorkspaceRoot,
    session_id: SessionId,
    run_id: RunId,
    call_id: ToolCallId,
    tool_id: String,
    occurred_at: TimestampDto,
    cancellation: RunCancellation,
    arguments_json: String,
}

impl ToolInvocationRequestDto {
    /// Creates a local tool invocation input.
    ///
    /// The canonical arguments document defaults to an empty JSON object; a
    /// caller that holds the model's original arguments attaches them with
    /// [`Self::with_arguments_json`] so the committed `tool_call` row carries
    /// the exact requested arguments and the service decodes that same
    /// document after the commit.
    #[must_use]
    pub fn new(
        workspace: WorkspaceRoot,
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
        tool_id: impl Into<String>,
        occurred_at: TimestampDto,
    ) -> Self {
        Self {
            workspace,
            session_id,
            run_id,
            call_id,
            tool_id: tool_id.into(),
            occurred_at,
            cancellation: RunCancellation::new(),
            arguments_json: "{}".to_owned(),
        }
    }

    /// Attaches the canonical JSON arguments document the model requested.
    #[must_use]
    pub fn with_arguments_json(mut self, arguments_json: impl Into<String>) -> Self {
        self.arguments_json = arguments_json.into();
        self
    }

    /// Binds this invocation to its run's cancellation handle.
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: RunCancellation) -> Self {
        self.cancellation = cancellation;
        self
    }
}

/// The committed result of creating one durable session and its optional first turn.
///
/// `session` is the created session's committed projection and `first_turn` is
/// the accepted outcome of the command's first turn, or `None` when the command
/// carried none. The session is committed before the turn, so a rejected first
/// turn still leaves the durable session this value names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatedSessionDto {
    session: SessionProjectionDto,
    first_turn: Option<AcceptedTurnOutcomeDto>,
}

impl CreatedSessionDto {
    /// Creates the committed outcome of one combined creation.
    #[must_use]
    pub const fn new(
        session: SessionProjectionDto,
        first_turn: Option<AcceptedTurnOutcomeDto>,
    ) -> Self {
        Self {
            session,
            first_turn,
        }
    }

    /// Returns the created session's committed projection.
    #[must_use]
    pub const fn session(&self) -> &SessionProjectionDto {
        &self.session
    }

    /// Returns the accepted first turn, or `None` when the command carried none.
    #[must_use]
    pub const fn first_turn(&self) -> Option<&AcceptedTurnOutcomeDto> {
        self.first_turn.as_ref()
    }
}

/// DTO-only application facade over one semantic storage repository.
pub struct ApplicationService<'a, Repository> {
    repository: &'a Repository,
}

impl<'a, Repository> ApplicationService<'a, Repository>
where
    Repository: StorageRepositoryDto,
{
    /// Executes one explicit local invocation, durably records its lifecycle,
    /// and returns its model-visible outcome.
    ///
    /// The call row commits before decoding, so a call whose arguments never
    /// decode still leaves durable evidence; its failure commits one terminal
    /// result carrying bounded model-visible guidance and the run continues.
    ///
    /// # Errors
    ///
    /// Returns the typed storage error when a durable row cannot commit.
    pub fn invoke_local_tool_with_publication<P: SessionCommitObserver>(
        &self,
        input: ToolInvocationRequestDto,
        publisher: &P,
    ) -> DtoResult<ToolResultOutcomeDto> {
        let ToolInvocationRequestDto {
            workspace,
            session_id,
            run_id,
            call_id,
            tool_id,
            occurred_at,
            cancellation,
            arguments_json,
        } = input;
        // One decode: the typed value the dispatch runs is exactly the typed
        // value the committed call row carries, so the durable evidence and the
        // effect cannot disagree and no second decode exists.
        let decoded = ToolInput::from_arguments_json(&tool_id, &arguments_json);
        let projection = match &decoded {
            Ok(decoded) => ToolCallRowDto::new(
                call_id,
                ToolIdentityDto::parse(&tool_id)?,
                Some(decoded.clone()),
            )?,
            Err(_) => ToolCallRowDto::new(call_id, ToolIdentityDto::parse(&tool_id)?, None)?,
        };
        // The tool-call row commits before any effect: the model's requested
        // call is durable evidence even when its arguments never decode.
        self.append_tool_call(
            session_id,
            run_id,
            &arguments_json,
            ToolRowDto::Call(projection),
            occurred_at,
            publisher,
        )?;
        // The invocation addresses the workspace root its caller resolved; the
        // root is an addressing anchor resolved once by the host.
        let service = ToolService::new(workspace);
        // A call whose arguments do not decode is a known, model-visible
        // failure: the durable result answers it with bounded guidance and the
        // run continues, so the model can correct the call.
        let decoded = match decoded {
            Ok(decoded) => decoded,
            Err(error) => {
                let content = tool_failure_guidance(&tool_id, &error);
                self.commit_tool_result(
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    ToolResultStatusDto::Failed,
                    None,
                    content.clone(),
                    Vec::new(),
                    occurred_at,
                    publisher,
                )?;
                return ToolResultOutcomeDto::failed(error, content);
            }
        };
        let value = match service.dispatch_with_cancellation(decoded, cancellation.tool_signal()) {
            Ok(ToolDispatchOutcome::Completed(value)) => value,
            Ok(ToolDispatchOutcome::Interrupted { cause, partial }) => {
                // An interrupted dispatch is a durable partial outcome, not a
                // typed failure: the call ends with whatever output was
                // captured, its cause and declared deadline stay durable
                // evidence, and the run continues with the next model step.
                let content = partial_tool_result_content(cause, partial.as_ref())?;
                self.commit_tool_result(
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    ToolResultStatusDto::Partial,
                    partial,
                    content.clone(),
                    interruption_metadata(cause)?,
                    occurred_at,
                    publisher,
                )?;
                return ToolResultOutcomeDto::partial(content);
            }
            Err(error) => {
                let content = tool_failure_guidance(&tool_id, &error);
                self.commit_tool_result(
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    ToolResultStatusDto::Failed,
                    None,
                    content.clone(),
                    Vec::new(),
                    occurred_at,
                    publisher,
                )?;
                return ToolResultOutcomeDto::failed(error, content);
            }
        };
        // The terminal result commits once, with its answering transcript row,
        // and the committed row is published immediately after that commit.
        let content = render_tool_result_content(&value)?;
        let metadata = tool_result_metadata(&value)?;
        self.commit_tool_result(
            session_id,
            run_id,
            call_id,
            &tool_id,
            ToolResultStatusDto::Completed,
            Some(value),
            content.clone(),
            metadata,
            occurred_at,
            publisher,
        )?;
        ToolResultOutcomeDto::completed(content)
    }

    /// Commits one tool-call row before the call is dispatched, then hands the
    /// committed row to the commit sink.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or storage error when the row cannot commit.
    fn append_tool_call<P: SessionCommitObserver>(
        &self,
        session_id: SessionId,
        run_id: RunId,
        arguments_json: &str,
        tool: ToolRowDto,
        occurred_at: TimestampDto,
        publisher: &P,
    ) -> DtoResult<MessageProjectionDto> {
        let message = NewMessageDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::ToolCall,
            arguments_json,
            None,
            Some(tool),
        )?;
        let committed = self.repository.append_message(message, occurred_at)?;
        publisher.observe_session_commit(&SessionCommitDto::Content(committed.clone()));
        Ok(committed)
    }

    /// Commits one terminal tool result as its answering transcript row, then
    /// hands that committed row to the commit sink.
    ///
    /// Exactly one transaction appends the `tool_result` message the model
    /// reads; the row's own typed projection carries its status, payload, and
    /// metadata, so the message is the result's single durable home.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or storage error when the result cannot commit.
    #[expect(
        clippy::too_many_arguments,
        reason = "One flat terminal-result payload keeps the single transaction at one call site."
    )]
    fn commit_tool_result<P: SessionCommitObserver>(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
        tool_id: &str,
        status: ToolResultStatusDto,
        result: Option<ToolResult>,
        content: String,
        metadata: Vec<ToolResultMetadataEntryDto>,
        occurred_at: TimestampDto,
        publisher: &P,
    ) -> DtoResult<MessageProjectionDto> {
        let projection = ToolResultRowDto::new(
            call_id,
            ToolIdentityDto::parse(tool_id)?,
            status,
            result,
            metadata,
        )?;
        let message = NewMessageDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::ToolResult,
            content,
            None,
            Some(ToolRowDto::Result(projection)),
        )?;
        let committed = self.repository.append_message(message, occurred_at)?;
        publisher.observe_session_commit(&SessionCommitDto::Content(committed.clone()));
        Ok(committed)
    }

    /// Creates an application facade around a DTO-only durable repository.
    #[must_use]
    pub const fn new(repository: &'a Repository) -> Self {
        Self { repository }
    }

    /// Creates a durable session and accepts the first turn it carries.
    ///
    /// The command's declared run policy is resolved before the workspace
    /// binding and the durable write: an absent mode becomes the vocabulary's
    /// `RunModeDto::DEFAULT`, so no front end decides the durable policy of a
    /// created session.
    ///
    /// A workspace root is bound to exactly one project and workspace identity
    /// for the life of the database, so a command proposing another identity
    /// for an already-bound root joins the durable binding instead: the
    /// proposed identities are advisory, and an unbounded number of sessions
    /// can be created under one root. A root with no binding keeps the
    /// command's identities and establishes the association.
    ///
    /// The session commits before the turn, so a rejected first turn leaves the
    /// durable session the client proposed and returns the typed rejection; no
    /// rollback exists and the next command may target that same session.
    /// Repeating an exactly-replayed command is this creation's replay: the
    /// durable `session_already_exists` conflict reads the session back and the
    /// turn path replays its own recorded outcome.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when the binding cannot be read or
    /// when the creation fails, or the typed admission error when the first
    /// turn is rejected.
    pub fn create_session(
        &self,
        command: CreateSessionCommandDto,
        config_snapshot: ConfigSnapshotDto,
        occurred_at: TimestampDto,
        observer: &dyn SessionCommitObserver,
    ) -> DtoResult<CreatedSessionDto> {
        let first_turn = command.first_turn().cloned();
        let resolved = command.resolved_mode();
        let command = command.with_mode(resolved);
        let session_id = command.session_id();
        let command = self.join_workspace_binding(command)?;
        let session = match self.repository.create_session(command, occurred_at) {
            Ok(session) => session,
            Err(error) if error.code() == "session_already_exists" => {
                self.repository.load_session_projection(session_id)?
            }
            Err(error) => return Err(error),
        };
        let first_turn = match first_turn {
            Some(first_turn) => Some(self.accept_first_turn(
                &first_turn,
                session.session_id(),
                config_snapshot,
                occurred_at,
                observer,
            )?),
            None => None,
        };
        Ok(CreatedSessionDto::new(session, first_turn))
    }

    /// Accepts one carried first turn against the session this creation made.
    ///
    /// The proposed run identity is the turn path's own derivation: the same
    /// session and `IdempotencyKey` always name the same run, so a replayed
    /// command replays the recorded acceptance instead of conflicting.
    ///
    /// # Errors
    ///
    /// Returns the typed validation error when the run identity is not
    /// canonical, or the typed admission error when the turn is rejected.
    fn accept_first_turn(
        &self,
        first_turn: &CreateSessionFirstTurnDto,
        session_id: SessionId,
        config_snapshot: ConfigSnapshotDto,
        occurred_at: TimestampDto,
        observer: &dyn SessionCommitObserver,
    ) -> DtoResult<AcceptedTurnOutcomeDto> {
        let proposed_run_id = RunId::parse(&first_turn.idempotency_key().to_string())?;
        let command = SendUserTurnCommandDto::new(
            session_id,
            first_turn.idempotency_key(),
            first_turn.content(),
        )?;
        self.send_user_turn(
            command,
            proposed_run_id,
            config_snapshot,
            occurred_at,
            observer,
        )
    }

    /// Returns `command` carrying the durable identity binding of its root.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when the binding cannot be read.
    fn join_workspace_binding(
        &self,
        command: CreateSessionCommandDto,
    ) -> DtoResult<CreateSessionCommandDto> {
        let Some(binding) = self
            .repository
            .workspace_binding(command.workspace_root())?
        else {
            return Ok(command);
        };
        Ok(CreateSessionCommandDto::new(
            binding.project_id(),
            command.session_id(),
            binding.workspace_id(),
            command.workspace_root().clone(),
            command.mode(),
            command.first_turn().cloned(),
        ))
    }

    /// Accepts a user turn and returns its committed durable outcome.
    ///
    /// A pending outcome returns immediately, and a started outcome returns
    /// once the exact `Starting` run is durably accepted. The daemon host owns
    /// the single scheduling path: it observes the committed acceptance and
    /// reads the exact `Starting` run through [`Self::schedule_starting_run`],
    /// so acceptance never schedules work itself and a post-commit scheduling
    /// failure cannot change the committed acceptance result.
    ///
    /// The committed acceptance reaches `observer` the way every other durable
    /// commit does: a started turn hands over its committed user row and the
    /// run status it opened, and a pending turn hands over nothing, because a
    /// queued turn has no transcript row yet - its row commits when it joins
    /// the run context.
    ///
    /// `proposed_run_id` must be a deterministic function of the command's own
    /// identity: the same session and `IdempotencyKey` must always propose the
    /// same [`RunId`], because the repository compares it when it replays an
    /// accepted turn. A retry that proposes a fresh identity is
    /// indistinguishable from the same key bound to different content and fails
    /// with `turn_idempotency_conflict`, so a client could never learn that its
    /// turn was already accepted.
    ///
    /// # Errors
    ///
    /// Returns an admission or malformed durable-acceptance error.
    pub fn send_user_turn(
        &self,
        command: SendUserTurnCommandDto,
        proposed_run_id: RunId,
        config_snapshot: ConfigSnapshotDto,
        occurred_at: TimestampDto,
        observer: &dyn SessionCommitObserver,
    ) -> DtoResult<AcceptedTurnOutcomeDto> {
        let accepted = self.repository.accept_user_turn(
            command.session_id(),
            command.idempotency_key(),
            command.content(),
            proposed_run_id,
            config_snapshot,
            occurred_at,
        )?;
        if let AcceptedTurnOutcomeDto::Started { run, message } = &accepted {
            // The prompt reaches a session subscriber before the run it opened:
            // a reader that shows the row first never displays an answer before
            // the turn it answers.
            observer.observe_session_commit(&SessionCommitDto::Content(message.clone()));
            observer.observe_session_commit(&SessionCommitDto::Status {
                session_id: run.session_id(),
                run_id: run.run_id(),
                status: run.status(),
            });
        }
        Ok(accepted)
    }

    /// Removes one not-yet-seen pending turn and returns its committed projection.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when no pending turn can be removed.
    pub fn remove_turn(
        &self,
        command: RemoveTurnCommandDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<PendingTurnProjectionDto> {
        self.repository.remove_turn(command, occurred_at)
    }

    /// Returns the exact active run one request addresses.
    ///
    /// The terminal stop validates through this rule: the addressed run must be
    /// the session's own active run.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error when the exact run is not active, or a
    /// repository error when the session projection cannot be read.
    pub fn active_run(&self, session_id: SessionId, run_id: RunId) -> DtoResult<RunProjectionDto> {
        self.repository
            .load_session_projection(session_id)?
            .active_run()
            .filter(|run| run.run_id() == run_id)
            .ok_or_else(|| {
                ErrorDto::validation(
                    "active_run_not_found",
                    "the requested run is not active in the session",
                )
            })
    }

    /// Reconstructs the exact durable context for one current `Starting` run.
    ///
    /// This is the daemon-host admission read. It deliberately does not dispatch
    /// work itself, so composition remains the owner of provider execution, and
    /// the supplied run-scoped cancellation handle becomes part of the returned
    /// execution input so the host that created it owns the run's interruption.
    ///
    /// # Errors
    ///
    /// Returns a typed context or scheduling error when the exact durable run is
    /// unavailable or is no longer eligible for execution.
    pub fn schedule_starting_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        cancellation: RunCancellation,
    ) -> DtoResult<ModelRunExecutionInputDto> {
        schedule_from_context(
            self.repository
                .load_starting_run_model_context(session_id, run_id)?,
            run_id,
            cancellation,
        )
    }
}

/// Builds the approved credential-free metadata entries of one tool result.
///
/// # Errors
///
/// Returns a validation error only when the static `truncated` entry is
/// rejected, which the metadata constructor cannot do.
fn tool_result_metadata(result: &ToolResult) -> DtoResult<Vec<ToolResultMetadataEntryDto>> {
    let truncated = match result {
        ToolResult::Read(value) => value.truncated,
        ToolResult::Execute(value) => value.text.truncated,
        ToolResult::Glob(value) => value.truncated,
        ToolResult::Grep(value) => value.truncated,
        ToolResult::Write(_) | ToolResult::Edit(_) => false,
    };
    if truncated {
        Ok(vec![ToolResultMetadataEntryDto::new("truncated", "true")?])
    } else {
        Ok(Vec::new())
    }
}

/// Builds the durable interruption evidence of one partial tool result.
///
/// The cause records its own closed code, and a call cut by its declared
/// deadline also records the deadline it hit, so the durable row names exactly
/// what stopped the call without carrying any captured content.
///
/// # Errors
///
/// Returns a validation error only when the static code entry or the declared
/// deadline is rejected, which the metadata constructor cannot do.
fn interruption_metadata(cause: InterruptCause) -> DtoResult<Vec<ToolResultMetadataEntryDto>> {
    let mut entries = vec![ToolResultMetadataEntryDto::new(
        "interruption",
        cause.code(),
    )?];
    if let InterruptCause::TimedOut(declared) = cause {
        entries.push(ToolResultMetadataEntryDto::new(
            "declared_timeout_seconds",
            declared.as_secs().to_string(),
        )?);
    }
    Ok(entries)
}

fn schedule_from_context(
    context: intention_storage::StartingRunModelContextDto,
    run_id: RunId,
    cancellation: RunCancellation,
) -> DtoResult<ModelRunExecutionInputDto> {
    let messages = model_context_messages(context.messages())?;
    let request = ModelRequestDto::new(
        context.run_id(),
        context.safe_config().resolved().provider().model(),
        messages,
        None,
    )?
    .with_tools(advertised_tool_definitions()?)?;
    // The constructed request must agree with the requested run identity and
    // the durable starting-run selection before it becomes executable work.
    if request.run_id() != run_id
        || request.model() != context.safe_config().resolved().provider().model()
    {
        return Err(ErrorDto::validation(
            "invalid_model_run_schedule",
            "model scheduling request must match the durable starting run selection",
        ));
    }
    Ok(ModelRunExecutionInputDto::new(
        context.session_id(),
        context.run_id(),
        request,
        context.safe_config().clone(),
        cancellation,
    ))
}

/// Rebuilds the ordered model messages of one starting run from its committed
/// transcript rows.
///
/// A `tool_call` row replays as the assistant tool-call message the in-run tool
/// loop emits: that one call, with identity and wire name from the row and the
/// committed arguments document. Its `tool_result` row replays as the tool-role
/// message answering that call.
///
/// An assistant row without answer text carries only the reasoning of a step
/// that issued calls without answering; reasoning is never replayed, so the row
/// contributes no model message.
///
/// A call whose result never committed cannot replay: an assistant tool-call
/// message whose answer is missing is not a valid model context, so the call is
/// omitted. A result whose call was omitted for that reason, or that never had
/// a committed call row, is omitted with it. An omitted row stays durable
/// transcript evidence; the rebuild invents no content in its place.
///
/// # Errors
///
/// Returns a typed validation error when a tool row does not carry its call
/// identity and wire tool name, or when a committed call's arguments document
/// cannot form a model tool call.
fn model_context_messages(rows: &[MessageProjectionDto]) -> DtoResult<Vec<ModelMessageDto>> {
    // Only a call whose committed result follows it can replay, so no emitted
    // assistant tool-call message stays unanswered.
    let mut first_results: BTreeMap<ToolCallId, usize> = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        if row.kind() == MessageKindDto::ToolResult
            && let Some(call_id) = row.tool_call_id()
        {
            first_results.entry(call_id).or_insert(index);
        }
    }
    let mut awaiting_result: BTreeSet<ToolCallId> = BTreeSet::new();
    let mut messages = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        match row.kind() {
            MessageKindDto::User => {
                messages.push(ModelMessageDto::new(ModelRoleDto::User, row.text())?);
            }
            MessageKindDto::Assistant => {
                // A reasoning-only step row is durable transcript evidence,
                // not model context: reasoning is never replayed, so a row
                // without answer text contributes no message.
                if !row.text().trim().is_empty() {
                    messages.push(ModelMessageDto::new(ModelRoleDto::Assistant, row.text())?);
                }
            }
            MessageKindDto::Notice => {
                messages.push(ModelMessageDto::new(ModelRoleDto::Notice, row.text())?);
            }
            MessageKindDto::ToolCall => {
                let call_id = row.tool_call_id().ok_or_else(incomplete_tool_row)?;
                if first_results
                    .get(&call_id)
                    .is_some_and(|result| *result > index)
                {
                    let call = ToolCallDto::new(
                        call_id,
                        row.tool_id().ok_or_else(incomplete_tool_row)?.wire_name(),
                        row.text(),
                    )?;
                    messages.push(ModelMessageDto::assistant_tool_calls(None, vec![call])?);
                    awaiting_result.insert(call_id);
                }
            }
            MessageKindDto::ToolResult => {
                let call_id = row.tool_call_id().ok_or_else(incomplete_tool_row)?;
                // Exactly one tool-role message answers each replayed call.
                if awaiting_result.remove(&call_id) {
                    messages.push(ModelMessageDto::tool_result(call_id, row.text())?);
                }
            }
        }
    }
    Ok(messages)
}

/// The typed rejection of a committed tool row without its call identity or
/// wire tool name.
fn incomplete_tool_row() -> ErrorDto {
    ErrorDto::validation(
        "invalid_model_context",
        "a committed tool row carries its call identity and wire tool name",
    )
}

/// Builds the model-visible tool definitions advertised with every scheduled run.
///
/// The model-visible tools surface in advertisement order.
///
/// # Errors
///
/// Returns a typed validation error when a model-visible tool cannot be mapped
/// into a provider-neutral tool definition.
fn advertised_tool_definitions() -> DtoResult<Vec<ModelToolDefinitionDto>> {
    intention_tools::model_visible_descriptors()
        .iter()
        .map(|descriptor| {
            ModelToolDefinitionDto::new(
                descriptor.id().as_str(),
                descriptor.description(),
                descriptor.input_schema(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {

    use super::tool_result_metadata;
    use intention_proto::{
        BoundedText, ExecuteResult, GrepResult, PathsResult, TextResult, ToolProcessStatus,
        ToolResult, WriteResult,
    };

    #[test]
    fn truncation_is_reported_as_approved_metadata() {
        let truncated = ToolResult::Glob(PathsResult {
            paths: Vec::new(),
            truncated: true,
        });
        let metadata = tool_result_metadata(&truncated).expect("truncation metadata is valid");
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].key(), "truncated");
        assert_eq!(metadata[0].value(), "true");
        let complete = ToolResult::Write(WriteResult { bytes: 1 });
        assert!(
            tool_result_metadata(&complete)
                .expect("complete metadata is valid")
                .is_empty()
        );
    }

    #[test]
    fn execute_and_grep_truncation_report_approved_metadata() {
        let executed = ToolResult::Execute(ExecuteResult {
            text: TextResult {
                text: BoundedText::new("cut output").expect("fixture output is valid"),
                truncated: true,
            },
            process: Some(ToolProcessStatus::Success),
        });
        let searched = ToolResult::Grep(GrepResult {
            matches: Vec::new(),
            truncated: true,
        });
        for result in [executed, searched] {
            let metadata = tool_result_metadata(&result).expect("truncation metadata is valid");
            assert_eq!(
                metadata
                    .iter()
                    .map(|entry| (entry.key(), entry.value()))
                    .collect::<Vec<_>>(),
                vec![("truncated", "true")],
                "a result cut by its own bound carries the approved entry"
            );
        }
    }
}
