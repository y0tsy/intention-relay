//! Application command/query orchestration over DTO-only current-state storage.
//!
//! This crate maps committed repository outcomes into protocol-ready DTOs. It
//! neither owns database resources nor reimplements repository idempotency.
//! Every tool call commits its `tool_call` row before dispatch and exactly one
//! terminal result row with its answering `tool_result` message afterwards.

mod context_window;
mod runtime;

pub use crate::runtime::*;

use intention_config::ConfigSnapshotDto;
use intention_domain::{ToolResultMetadataEntryDto, ToolResultStatusDto};
use intention_proto::ToolCallId;
use intention_proto::{
    CreateSessionAcceptedDto, InterruptRunAcceptedDto, ProtocolAcceptedResultDto,
    RemoveTurnAcceptedDto, SendUserTurnAcceptedDto, SendUserTurnOutcomeDto,
};
use intention_proto::{
    CreateSessionCommandDto, InterruptRunCommandDto, MessageKindDto, MessageProjectionDto,
    RemoveTurnCommandDto, SendUserTurnCommandDto,
};
use intention_proto::{DtoResult, ErrorDto, RunId, SessionId, TimestampDto};
use intention_storage::{
    AcceptUserTurnInputDto, AcceptedTurnOutcomeDto, AppendMessageInputDto, CreateSessionInputDto,
    RemoveTurnInputDto, StorageRepositoryDto, ToolResultEvidenceDto, WriteToolResultInputDto,
};
use intention_tools::{
    CancellationSignal, InterruptCause, ToolDispatchOutcome, ToolInput, ToolResult, ToolService,
    WorkspaceRoot,
};

/// Synchronous DTO-only boundary that admits accepted work to daemon-owned scheduling.
///
/// Implementations must not invoke a provider. The daemon host owns all
/// asynchronous execution after this bounded post-commit admission.
pub trait ModelRunDispatchPort {
    /// Schedules one fully constructed model run.
    ///
    /// # Errors
    ///
    /// Returns a typed local scheduling error when the daemon cannot accept the work.
    fn dispatch_model_run(&self, input: ModelRunExecutionInputDto) -> DtoResult<()>;
}

/// Terminal application outcome of one explicit local tool invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalToolInvocationOutcomeDto {
    /// The tool completed and produced its final typed result.
    Completed(ToolResult),
    /// The tool stopped before a final result.
    Partial {
        /// Whether the stop was an explicit cancellation.
        stopped: bool,
        /// Output captured before the stop, when the tool produced any.
        result: Option<ToolResult>,
    },
}

impl ModelRunCommitObserver for () {
    fn observe_model_run_commit(&self, _: &ModelRunCommitDto) {}
}

/// Composition-owned boundary that binds the authorized workspace before the
/// invocation executes.
///
/// Canonical paths stay inside the application: the bound root is returned to
/// the application, never to an observation boundary.
pub trait WorkspaceBoundaryPort {
    /// Binds the authorized workspace and returns the root the invocation
    /// addresses.
    ///
    /// # Errors
    ///
    /// Returns a typed workspace-resolution error when the workspace cannot be
    /// authorized or prepared for the invocation.
    fn resolve(&self, workspace: &WorkspaceRoot) -> DtoResult<WorkspaceRoot>;
}

impl WorkspaceBoundaryPort for () {
    fn resolve(&self, workspace: &WorkspaceRoot) -> DtoResult<WorkspaceRoot> {
        Ok(workspace.clone())
    }
}

/// Complete engine command for one local tool invocation.
#[derive(Debug)]
pub struct ToolInvocationRequestDto {
    workspace: WorkspaceRoot,
    session_id: SessionId,
    run_id: RunId,
    call_id: ToolCallId,
    tool_id: String,
    input: ToolInput,
    occurred_at: TimestampDto,
    cancellation: CancellationSignal,
    arguments_json: String,
}

impl ToolInvocationRequestDto {
    /// Creates a local tool invocation input.
    ///
    /// The canonical arguments document defaults to an empty JSON object; a
    /// caller that holds the model's original arguments attaches them with
    /// [`Self::with_arguments_json`] so the committed `tool_call` row carries
    /// the exact requested arguments.
    #[must_use]
    pub fn new(
        workspace: WorkspaceRoot,
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
        tool_id: impl Into<String>,
        input: ToolInput,
        occurred_at: TimestampDto,
    ) -> Self {
        Self {
            workspace,
            session_id,
            run_id,
            call_id,
            tool_id: tool_id.into(),
            input,
            occurred_at,
            cancellation: CancellationSignal::new(),
            arguments_json: "{}".to_owned(),
        }
    }

    /// Attaches the canonical JSON arguments document the model requested.
    #[must_use]
    pub fn with_arguments_json(mut self, arguments_json: impl Into<String>) -> Self {
        self.arguments_json = arguments_json.into();
        self
    }

    /// Requests cancellation of this invocation.
    #[must_use]
    pub fn with_cancellation(mut self, cancellation: CancellationSignal) -> Self {
        self.cancellation = cancellation;
        self
    }
}

/// DTO-only application facade over one semantic storage repository.
pub struct ApplicationService<'a, Repository> {
    repository: &'a Repository,
    workspace_boundary: Box<dyn WorkspaceBoundaryPort + 'a>,
}

impl<'a, Repository> ApplicationService<'a, Repository>
where
    Repository: StorageRepositoryDto,
{
    /// Executes one explicit local invocation and durably records its lifecycle.
    ///
    /// # Errors
    ///
    /// Returns the typed validation, storage, or tool execution error.
    pub fn invoke_local_tool(
        &self,
        input: ToolInvocationRequestDto,
    ) -> DtoResult<LocalToolInvocationOutcomeDto> {
        self.invoke_local_tool_with_publication(input, &())
    }

    /// Executes, durably commits, and hands each committed row to the commit
    /// sink.
    ///
    /// # Errors
    ///
    /// Returns the typed validation, storage, workspace, or tool execution
    /// error.
    pub fn invoke_local_tool_with_publication<P: ModelRunCommitObserver>(
        &self,
        input: ToolInvocationRequestDto,
        publisher: &P,
    ) -> DtoResult<LocalToolInvocationOutcomeDto> {
        let ToolInvocationRequestDto {
            workspace,
            session_id,
            run_id,
            call_id,
            tool_id,
            input,
            occurred_at,
            cancellation,
            arguments_json,
        } = input;
        // The input identity is validated before the call row commits, so a
        // mismatched identifier is rejected without a durable trace.
        if tool_id != input.tool_id().as_str() {
            return Err(ErrorDto::validation(
                "tool_id_mismatch",
                "tool identifier does not match typed tool input",
            ));
        }
        // The tool-call row commits before any dispatch: the model's requested
        // call is durable evidence even when execution fails.
        self.append_tool_call(
            session_id,
            run_id,
            call_id,
            &tool_id,
            &arguments_json,
            occurred_at,
            publisher,
        )?;
        // The workspace owner binds the authorized root before execution, and
        // the invocation addresses only that bound root.
        let workspace = self
            .workspace_boundary
            .resolve(&workspace)
            .inspect_err(|error| {
                let _ = self.append_tool_failure(
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    error,
                    occurred_at,
                    publisher,
                );
            })?;
        let service = ToolService::new(workspace);
        let outcome = service.dispatch_with_cancellation(call_id, input, cancellation.clone());
        let result: DtoResult<ToolResult> = match outcome {
            Ok(ToolDispatchOutcome::Completed(value)) => Ok(value),
            Ok(ToolDispatchOutcome::Interrupted { cause, partial }) => {
                // An interrupted dispatch is a durable partial outcome, not a
                // typed failure: the call ends with whatever output was
                // captured and the run continues with the next model step.
                let stopped = cause == InterruptCause::Stopped || cancellation.is_cancelled();
                let content = partial_tool_result_content(stopped, partial.as_ref())?;
                self.commit_tool_result(
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    ToolResultStatusDto::Partial,
                    content,
                    Vec::new(),
                    occurred_at,
                    publisher,
                )?;
                return Ok(LocalToolInvocationOutcomeDto::Partial {
                    stopped,
                    result: partial,
                });
            }
            Err(error) => {
                self.append_tool_failure(
                    session_id,
                    run_id,
                    call_id,
                    &tool_id,
                    &error,
                    occurred_at,
                    publisher,
                )?;
                return Err(error);
            }
        };
        // The terminal result commits once, with its answering transcript row,
        // and the committed row is published immediately after that commit.
        let (status, content, metadata) = match &result {
            Ok(value) => (
                ToolResultStatusDto::Completed,
                render_tool_result_content(value)?,
                tool_result_metadata(value)?,
            ),
            Err(error) => (
                ToolResultStatusDto::Failed,
                error.code().to_owned(),
                Vec::new(),
            ),
        };
        self.commit_tool_result(
            session_id,
            run_id,
            call_id,
            &tool_id,
            status,
            content,
            metadata,
            occurred_at,
            publisher,
        )?;
        result.map(LocalToolInvocationOutcomeDto::Completed)
    }

    /// Commits one tool-call row before the call is dispatched, then hands the
    /// committed row to the commit sink.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or storage error when the row cannot commit.
    #[expect(
        clippy::too_many_arguments,
        reason = "One flat call payload keeps the single call-row commit at one call site."
    )]
    fn append_tool_call<P: ModelRunCommitObserver>(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
        tool_id: &str,
        arguments_json: &str,
        occurred_at: TimestampDto,
        publisher: &P,
    ) -> DtoResult<MessageProjectionDto> {
        let message = MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::ToolCall,
            arguments_json,
            None,
            Some(call_id),
            Some(tool_id.to_owned()),
        )?;
        let committed = self
            .repository
            .append_message(AppendMessageInputDto::new(message, occurred_at))?;
        publisher.observe_model_run_commit(&ModelRunCommitDto::Content(committed.clone()));
        Ok(committed)
    }

    /// Commits one terminal failed result for a call that never produced output.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or storage error when the result cannot commit.
    #[expect(
        clippy::too_many_arguments,
        reason = "One flat failure payload keeps the single failure commit at one call site."
    )]
    fn append_tool_failure<P: ModelRunCommitObserver>(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
        tool_id: &str,
        error: &ErrorDto,
        occurred_at: TimestampDto,
        publisher: &P,
    ) -> DtoResult<()> {
        self.commit_tool_result(
            session_id,
            run_id,
            call_id,
            tool_id,
            ToolResultStatusDto::Failed,
            error.code().to_owned(),
            Vec::new(),
            occurred_at,
            publisher,
        )
        .map(|_| ())
    }

    /// Commits one terminal tool result with its answering transcript row, then
    /// hands that committed row to the commit sink.
    ///
    /// Exactly one transaction writes the `tool_results` row and the
    /// `tool_result` message the model reads.
    ///
    /// # Errors
    ///
    /// Returns a typed validation or storage error when the result cannot commit.
    #[expect(
        clippy::too_many_arguments,
        reason = "One flat terminal-result payload keeps the single transaction at one call site."
    )]
    fn commit_tool_result<P: ModelRunCommitObserver>(
        &self,
        session_id: SessionId,
        run_id: RunId,
        call_id: ToolCallId,
        tool_id: &str,
        status: ToolResultStatusDto,
        content: String,
        metadata: Vec<ToolResultMetadataEntryDto>,
        occurred_at: TimestampDto,
        publisher: &P,
    ) -> DtoResult<ToolResultEvidenceDto> {
        let evidence = ToolResultEvidenceDto::new(
            session_id,
            run_id,
            call_id,
            tool_id.to_owned(),
            status,
            content.clone(),
            metadata,
            occurred_at,
        )?;
        let message = MessageProjectionDto::new(
            session_id,
            Some(run_id),
            MessageKindDto::ToolResult,
            content,
            None,
            Some(call_id),
            Some(tool_id.to_owned()),
        )?;
        let committed = self
            .repository
            .write_tool_result(WriteToolResultInputDto::new(evidence, message.clone())?)?;
        publisher.observe_model_run_commit(&ModelRunCommitDto::Content(message));
        Ok(committed)
    }

    /// Creates an application facade around a DTO-only durable repository.
    #[must_use]
    pub fn new(repository: &'a Repository) -> Self {
        Self {
            repository,
            workspace_boundary: Box::new(()),
        }
    }

    #[must_use]
    pub fn with_workspace_boundary<B: WorkspaceBoundaryPort + 'a>(mut self, boundary: B) -> Self {
        self.workspace_boundary = Box::new(boundary);
        self
    }

    /// Creates a durable session and maps its committed evidence for protocol use.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when durable session creation fails.
    pub fn create_session(
        &self,
        command: CreateSessionCommandDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let projection = self
            .repository
            .create_session(CreateSessionInputDto::new(command, occurred_at))?;
        Ok(ProtocolAcceptedResultDto::CreateSession(
            CreateSessionAcceptedDto::new(
                projection.project_id(),
                projection.workspace_id(),
                projection.session_id(),
            ),
        ))
    }

    /// Accepts a user turn and schedules an exactly-started run only after its initial commit.
    ///
    /// A pending outcome returns immediately. A started outcome reads the model
    /// context of the exact `Starting` run and dispatches it, so an acceptance
    /// whose run has already left `Starting` neither reads context nor
    /// dispatches and a repeated acceptance never schedules one run twice. Any
    /// post-commit context or dispatch failure is durably recorded against the
    /// exact `Starting` run and this method still returns the original
    /// acceptance.
    ///
    /// # Errors
    ///
    /// Returns an admission or malformed durable-acceptance error. Post-commit
    /// scheduling failures deliberately preserve the committed acceptance result.
    pub fn send_user_turn_and_schedule<Dispatch>(
        &self,
        command: SendUserTurnCommandDto,
        proposed_run_id: RunId,
        config_snapshot: ConfigSnapshotDto,
        occurred_at: TimestampDto,
        dispatch: &Dispatch,
    ) -> DtoResult<ProtocolAcceptedResultDto>
    where
        Dispatch: ModelRunDispatchPort,
    {
        let outcome = self
            .repository
            .accept_user_turn(AcceptUserTurnInputDto::new(
                command.session_id(),
                command.idempotency_key(),
                command.content(),
                proposed_run_id,
                config_snapshot,
                occurred_at,
            )?)?;
        let accepted = accepted_user_turn(&command, &outcome)?;
        let ProtocolAcceptedResultDto::SendUserTurn(accepted_turn) = accepted else {
            unreachable!("accepted user turn always returns user-turn acceptance")
        };
        let SendUserTurnOutcomeDto::Started { run_id, .. } = accepted_turn.outcome() else {
            return Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn));
        };
        let session_id = accepted_turn.session_id();
        let schedule = match self
            .repository
            .load_starting_run_model_context(session_id, run_id)
        {
            Ok(context) if context.session_id() == session_id && context.run_id() == run_id => {
                match schedule_from_context(context, run_id, ModelCancellationSignal::new()) {
                    Ok(schedule) => schedule,
                    Err(_) => {
                        preserve_accepted_after_scheduling_failure(
                            self.repository,
                            session_id,
                            run_id,
                            "model_context_unavailable",
                            occurred_at,
                        );
                        return Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn));
                    }
                }
            }
            Ok(_) | Err(_) => {
                preserve_accepted_after_scheduling_failure(
                    self.repository,
                    session_id,
                    run_id,
                    "model_context_unavailable",
                    occurred_at,
                );
                return Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn));
            }
        };
        if dispatch.dispatch_model_run(schedule).is_err() {
            preserve_accepted_after_scheduling_failure(
                self.repository,
                session_id,
                run_id,
                "model_scheduling_unavailable",
                occurred_at,
            );
        }
        Ok(ProtocolAcceptedResultDto::SendUserTurn(accepted_turn))
    }

    /// Removes one not-yet-seen pending turn and maps its committed evidence.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when no pending turn can be removed.
    pub fn remove_turn(
        &self,
        command: RemoveTurnCommandDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let turn = self
            .repository
            .remove_turn(RemoveTurnInputDto::new(command, occurred_at))?;
        Ok(ProtocolAcceptedResultDto::RemoveTurn(
            RemoveTurnAcceptedDto::new(turn.session_id(), turn.turn_id()),
        ))
    }

    /// Accepts an interruption request for one exact active run.
    ///
    /// Interruption is not a durable run state: the run stays active and the
    /// daemon host signals the registered executor after this validation.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error when the exact run is not active, or a
    /// repository error when the session projection cannot be read.
    pub fn interrupt_run(
        &self,
        command: InterruptRunCommandDto,
    ) -> DtoResult<ProtocolAcceptedResultDto> {
        let projection = self
            .repository
            .load_session_projection(command.session_id())?;
        let active = projection
            .active_run()
            .filter(|run| run.run_id() == command.run_id())
            .ok_or_else(|| {
                ErrorDto::validation(
                    "active_run_not_found",
                    "the requested run is not active in the session",
                )
            })?;
        Ok(ProtocolAcceptedResultDto::InterruptRun(
            InterruptRunAcceptedDto::new(command.session_id(), active.run_id()),
        ))
    }

    /// Reconstructs the exact durable context for one current `Starting` run.
    ///
    /// This is the daemon-host admission read. It deliberately does not dispatch
    /// work itself, so composition remains the owner of provider execution, and
    /// the supplied cancellation signal becomes part of the returned execution
    /// input so the host that registers it owns the run's interruption.
    ///
    /// # Errors
    ///
    /// Returns a typed context or scheduling error when the exact durable run is
    /// unavailable or is no longer eligible for execution.
    pub fn schedule_starting_run(
        &self,
        session_id: SessionId,
        run_id: RunId,
        cancellation: ModelCancellationSignal,
    ) -> DtoResult<ModelRunExecutionInputDto> {
        schedule_from_context(
            self.repository
                .load_starting_run_model_context(session_id, run_id)?,
            run_id,
            cancellation,
        )
    }
}

const fn accepted_user_turn(
    command: &SendUserTurnCommandDto,
    outcome: &AcceptedTurnOutcomeDto,
) -> DtoResult<ProtocolAcceptedResultDto> {
    let (turn_id, outcome) = match outcome {
        AcceptedTurnOutcomeDto::Started { run, .. } => (
            run.turn_id(),
            SendUserTurnOutcomeDto::Started {
                run_id: run.run_id(),
                config_revision_id: run.config_revision_id(),
            },
        ),
        AcceptedTurnOutcomeDto::Pending(turn) => (turn.turn_id(), SendUserTurnOutcomeDto::Pending),
    };
    Ok(ProtocolAcceptedResultDto::SendUserTurn(
        SendUserTurnAcceptedDto::new(command.session_id(), turn_id, outcome),
    ))
}

/// Renders one typed tool result into its bounded model-visible content.
///
/// The typed result is redacted and workspace-relative by construction: text
/// and search payloads keep their own bounds, truncated content keeps its
/// explicit marker, and mutations report their byte count.
///
/// # Errors
///
/// Returns a validation error when the rendered content is blank, because a
/// tool result row must always answer its call with readable content.
fn render_tool_result_content(result: &ToolResult) -> DtoResult<String> {
    let content = match result {
        ToolResult::Read(value) | ToolResult::Execute(value) => {
            if value.truncated {
                format!("{}\n[truncated]", value.text.as_str())
            } else {
                value.text.as_str().to_owned()
            }
        }
        ToolResult::Glob(value) => {
            let mut content = value
                .paths
                .iter()
                .map(intention_proto::WorkspaceRelativePathDto::as_str)
                .collect::<Vec<_>>()
                .join("\n");
            append_truncation_marker(&mut content, value.truncated);
            content
        }
        ToolResult::Grep(value) => {
            let mut content = value
                .matches
                .iter()
                .map(|matched| {
                    format!(
                        "{}:{}:{}: {}",
                        matched.path.as_str(),
                        matched.line,
                        matched.column,
                        matched.fragment.as_str()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            append_truncation_marker(&mut content, value.truncated);
            content
        }
        ToolResult::Write(value) | ToolResult::Edit(value) => format!("{} bytes", value.bytes),
    };
    if content.trim().is_empty() {
        return Err(ErrorDto::validation(
            "invalid_tool_result_content",
            "tool result content must not be empty",
        ));
    }
    Ok(content)
}

/// Appends the honest truncation marker to one bounded list projection.
fn append_truncation_marker(content: &mut String, truncated: bool) {
    if truncated {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str("[truncated]");
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
        ToolResult::Read(value) | ToolResult::Execute(value) => value.truncated,
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

/// Renders the durable partial result of one interrupted dispatch.
///
/// The captured output precedes the exact interruption notice selected by doc
/// 15 for the stopped/lost and captured/uncaptured cases.
///
/// # Errors
///
/// Returns a validation error when the captured output cannot render.
fn partial_tool_result_content(stopped: bool, result: Option<&ToolResult>) -> DtoResult<String> {
    let notice = match (stopped, result.is_some()) {
        (true, true) => {
            "[The tool call was stopped before a final result; the output above is partial.]"
        }
        (false, true) => {
            "[The tool call did not receive a final result; the output above is partial.]"
        }
        (true, false) => "[The tool call was stopped before a final result.]",
        (false, false) => "[The tool call did not receive a final result.]",
    };
    match result {
        Some(result) => Ok(format!("{}\n{notice}", render_tool_result_content(result)?)),
        None => Ok(notice.to_owned()),
    }
}

fn preserve_accepted_after_scheduling_failure<Repository>(
    repository: &Repository,
    session_id: SessionId,
    run_id: RunId,
    failure_code: &'static str,
    occurred_at: TimestampDto,
) where
    Repository: StorageRepositoryDto,
{
    if fail_starting_run(repository, session_id, run_id, failure_code, occurred_at).is_err() {
        // The durable acceptance is already the externally documented result;
        // a secondary failure write cannot replace it with a scheduling error.
    }
}

fn schedule_from_context(
    context: intention_storage::StartingRunModelContextDto,
    run_id: RunId,
    cancellation: ModelCancellationSignal,
) -> DtoResult<ModelRunExecutionInputDto> {
    let messages = context
        .messages()
        .iter()
        .map(|message| {
            ModelMessageDto::new(
                match message.kind() {
                    intention_proto::MessageKindDto::User => ModelRoleDto::User,
                    intention_proto::MessageKindDto::Assistant => ModelRoleDto::Assistant,
                    intention_proto::MessageKindDto::Notice => ModelRoleDto::Notice,
                    intention_proto::MessageKindDto::ToolCall
                    | intention_proto::MessageKindDto::ToolResult => {
                        return Err(ErrorDto::validation(
                            "invalid_model_context",
                            "a starting run context carries no tool exchange rows",
                        ));
                    }
                },
                message.text(),
            )
        })
        .collect::<DtoResult<Vec<_>>>()?;
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
            let parameters_json = descriptor.input_schema().ok_or_else(|| {
                ErrorDto::validation(
                    "model_tool_schema_unavailable",
                    "a model-visible tool must advertise an input schema",
                )
            })?;
            ModelToolDefinitionDto::new(
                descriptor.id().as_str(),
                descriptor.description(),
                parameters_json,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "Rendering fixtures use expect to provide precise failures."
    )]

    use super::{partial_tool_result_content, render_tool_result_content, tool_result_metadata};
    use intention_proto::WorkspaceRelativePathDto;
    use intention_tools::{BoundedText, TextResult, ToolResult};

    fn bounded(value: &str) -> BoundedText {
        BoundedText::new(value).unwrap_or_else(|_| unreachable!("fixture tool text is bounded"))
    }

    fn relative(value: &str) -> WorkspaceRelativePathDto {
        WorkspaceRelativePathDto::parse(value)
            .unwrap_or_else(|_| unreachable!("fixture relative path is valid"))
    }

    #[test]
    fn tool_result_content_covers_each_typed_result_family() {
        let read = ToolResult::Read(TextResult {
            text: bounded("hello"),
            truncated: false,
        });
        assert_eq!(
            render_tool_result_content(&read).expect("read content renders"),
            "hello"
        );
        let truncated = ToolResult::Execute(TextResult {
            text: bounded("done"),
            truncated: true,
        });
        assert_eq!(
            render_tool_result_content(&truncated).expect("execute content renders"),
            "done\n[truncated]"
        );
        let glob = ToolResult::Glob(intention_tools::PathsResult {
            paths: vec![relative("src/a.rs"), relative("src/b.rs")],
            truncated: true,
        });
        assert_eq!(
            render_tool_result_content(&glob).expect("glob content renders"),
            "src/a.rs\nsrc/b.rs\n[truncated]"
        );
        let grep = ToolResult::Grep(intention_tools::GrepResult {
            matches: vec![intention_tools::GrepMatch {
                path: relative("src/a.rs"),
                line: 3,
                column: 5,
                fragment: bounded("needle"),
            }],
            truncated: false,
        });
        assert_eq!(
            render_tool_result_content(&grep).expect("grep content renders"),
            "src/a.rs:3:5: needle"
        );
        let write = ToolResult::Write(intention_tools::WriteResult { bytes: 17 });
        assert_eq!(
            render_tool_result_content(&write).expect("write content renders"),
            "17 bytes"
        );
        let edit = ToolResult::Edit(intention_tools::WriteResult { bytes: 2 });
        assert_eq!(
            render_tool_result_content(&edit).expect("edit content renders"),
            "2 bytes"
        );
    }

    #[test]
    fn blank_tool_result_content_is_rejected() {
        let read = ToolResult::Read(TextResult {
            text: bounded(""),
            truncated: false,
        });
        let error = render_tool_result_content(&read).expect_err("blank content is rejected");
        assert_eq!(error.code(), "invalid_tool_result_content");
    }

    #[test]
    fn truncation_is_reported_as_approved_metadata() {
        let truncated = ToolResult::Glob(intention_tools::PathsResult {
            paths: Vec::new(),
            truncated: true,
        });
        let metadata = tool_result_metadata(&truncated).expect("truncation metadata is valid");
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].key(), "truncated");
        assert_eq!(metadata[0].value(), "true");
        let complete = ToolResult::Write(intention_tools::WriteResult { bytes: 1 });
        assert!(
            tool_result_metadata(&complete)
                .expect("complete metadata is valid")
                .is_empty()
        );
    }

    #[test]
    fn partial_content_carries_the_exact_interruption_notice() {
        let captured = ToolResult::Read(TextResult {
            text: bounded("half a line"),
            truncated: false,
        });
        assert_eq!(
            partial_tool_result_content(true, Some(&captured)).expect("partial content renders"),
            "half a line\n[The tool call was stopped before a final result; the output above is partial.]"
        );
        assert_eq!(
            partial_tool_result_content(false, Some(&captured)).expect("partial content renders"),
            "half a line\n[The tool call did not receive a final result; the output above is partial.]"
        );
        assert_eq!(
            partial_tool_result_content(true, None).expect("partial content renders"),
            "[The tool call was stopped before a final result.]"
        );
        assert_eq!(
            partial_tool_result_content(false, None).expect("partial content renders"),
            "[The tool call did not receive a final result.]"
        );
    }
}
