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
use intention_proto::ToolCallId;
use intention_proto::{
    CreateSessionCommandDto, InterruptRunCommandDto, MessageKindDto, MessageProjectionDto,
    PendingTurnProjectionDto, RemoveTurnCommandDto, RunProjectionDto, SendUserTurnCommandDto,
    SessionProjectionDto,
};
use intention_proto::{DtoResult, ErrorDto, RunId, SessionId, TimestampDto};
use intention_storage::{
    AcceptedTurnOutcomeDto, StorageRepositoryDto, ToolResultEvidenceDto,
    ToolResultMetadataEntryDto, ToolResultStatusDto,
};
use intention_tools::{
    InterruptCause, ToolDispatchOutcome, ToolInput, ToolResult, ToolService, WorkspaceRoot,
    partial_tool_result_content, render_tool_result_content,
};

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
    cancellation: RunCancellation,
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
    /// # Errors
    ///
    /// Returns the typed validation, storage, or tool execution error when no
    /// terminal outcome could be produced.
    pub fn invoke_local_tool_with_publication<P: ModelRunCommitObserver>(
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
        // The invocation addresses the workspace root its caller resolved; the
        // root is an addressing anchor resolved once by the host.
        let service = ToolService::new(workspace);
        let outcome = service.dispatch_with_cancellation(input, cancellation.tool_signal());
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
                    content.clone(),
                    Vec::new(),
                    occurred_at,
                    publisher,
                )?;
                return ToolResultOutcomeDto::partial(content);
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
                return Ok(ToolResultOutcomeDto::failed(error));
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
            content.clone(),
            metadata,
            occurred_at,
            publisher,
        )?;
        match result {
            Ok(_) => ToolResultOutcomeDto::completed(content),
            Err(error) => Ok(ToolResultOutcomeDto::failed(error)),
        }
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
        let committed = self.repository.append_message(message, occurred_at)?;
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
            .write_tool_result(evidence, message.clone())?;
        publisher.observe_model_run_commit(&ModelRunCommitDto::Content(message));
        Ok(committed)
    }

    /// Creates an application facade around a DTO-only durable repository.
    #[must_use]
    pub const fn new(repository: &'a Repository) -> Self {
        Self { repository }
    }

    /// Creates a durable session and returns its committed projection.
    ///
    /// # Errors
    ///
    /// Returns the typed repository error when durable session creation fails.
    pub fn create_session(
        &self,
        command: CreateSessionCommandDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<SessionProjectionDto> {
        self.repository.create_session(command, occurred_at)
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
    /// # Errors
    ///
    /// Returns an admission or malformed durable-acceptance error.
    pub fn send_user_turn(
        &self,
        command: SendUserTurnCommandDto,
        proposed_run_id: RunId,
        config_snapshot: ConfigSnapshotDto,
        occurred_at: TimestampDto,
    ) -> DtoResult<AcceptedTurnOutcomeDto> {
        self.repository.accept_user_turn(
            command.session_id(),
            command.idempotency_key(),
            command.content(),
            proposed_run_id,
            config_snapshot,
            occurred_at,
        )
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

    /// Accepts an interruption request for one exact active run.
    ///
    /// Interruption is not a durable run state: the run stays active and the
    /// daemon host signals the registered executor after this validation.
    ///
    /// # Errors
    ///
    /// Returns a typed validation error when the exact run is not active, or a
    /// repository error when the session projection cannot be read.
    pub fn interrupt_run(&self, command: InterruptRunCommandDto) -> DtoResult<RunProjectionDto> {
        self.repository
            .load_session_projection(command.session_id())?
            .active_run()
            .filter(|run| run.run_id() == command.run_id())
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

fn schedule_from_context(
    context: intention_storage::StartingRunModelContextDto,
    run_id: RunId,
    cancellation: RunCancellation,
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
    #![allow(
        clippy::expect_used,
        reason = "Metadata fixtures use expect to provide precise failures."
    )]

    use super::tool_result_metadata;
    use intention_tools::ToolResult;

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
}
