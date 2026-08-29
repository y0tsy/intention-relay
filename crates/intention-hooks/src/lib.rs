//! Typed, deterministic hook registration and dispatch contracts.
//!
//! The two workspace phases are the mandatory workspace boundary: the
//! `intention-workspace` owner resolves and validates the authorized root
//! between [`Phase::BeforeWorkspaceResolution`] and
//! [`Phase::AfterWorkspaceResolution`]. Phase contexts may identify a
//! workspace only through safe identity — the [`ToolCallId`] plus typed
//! relative input today, and the daemon-owned
//! [`intention_types::WorkspaceId`] once the application wires it into these
//! contexts — never a canonical or absolute workspace path. Hooks remain
//! persistence- and publication-free: they observe typed contexts and return
//! typed outcomes that the caller alone applies.

use intention_tools::{ToolInput, ToolResult};
use intention_types::{DtoResult, ErrorDto, ToolCallId};

/// The eight points in the tool lifecycle.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Phase {
    BeforeToolInvocation,
    BeforeWorkspaceResolution,
    AfterWorkspaceResolution,
    BeforeToolExecution,
    AfterToolExecution,
    BeforeToolResultPersist,
    BeforeToolResultModelContext,
    AfterToolResultPublished,
}

/// Policy applied when a hook returns an operational failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailurePolicy {
    /// Stop dispatch and return the failure.
    FailClosed,
    /// Record the failure through the caller's observability boundary and continue.
    FailOpen,
}

/// Safe metadata identifying one hook dispatch attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HookObservability {
    pub hook_id: &'static str,
    /// Stable contract revision for this registered hook implementation.
    pub registration_revision: u32,
    pub phase: Phase,
    pub failure_policy: FailurePolicy,
}

/// Result of dispatch together with failures tolerated by fail-open policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchResult {
    pub outcome: Outcome,
    pub failures: Vec<HookObservability>,
}

/// Context supplied to a hook, with phase-specific payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PhaseContext {
    Invocation {
        call: ToolCallId,
        input: ToolInput,
    },
    /// Mandatory workspace boundary before root resolution. Carries only the
    /// safe call identity and typed relative input, never a workspace path.
    WorkspaceResolution {
        call: ToolCallId,
        input: ToolInput,
    },
    /// Mandatory workspace boundary after the authorized root resolved. The
    /// same safe-identity rule holds: the root itself never enters the
    /// context, and only the daemon-owned workspace identity may name it.
    WorkspaceResolved {
        call: ToolCallId,
        input: ToolInput,
    },
    Execution {
        call: ToolCallId,
        input: ToolInput,
    },
    Executed {
        call: ToolCallId,
        input: ToolInput,
        result: ToolResult,
    },
    Persist {
        call: ToolCallId,
        result: ToolResult,
    },
    ModelContext {
        call: ToolCallId,
        result: ToolResult,
    },
    Published {
        call: ToolCallId,
        result: ToolResult,
    },
}

impl PhaseContext {
    #[must_use]
    pub const fn phase(&self) -> Phase {
        match self {
            Self::Invocation { .. } => Phase::BeforeToolInvocation,
            Self::WorkspaceResolution { .. } => Phase::BeforeWorkspaceResolution,
            Self::WorkspaceResolved { .. } => Phase::AfterWorkspaceResolution,
            Self::Execution { .. } => Phase::BeforeToolExecution,
            Self::Executed { .. } => Phase::AfterToolExecution,
            Self::Persist { .. } => Phase::BeforeToolResultPersist,
            Self::ModelContext { .. } => Phase::BeforeToolResultModelContext,
            Self::Published { .. } => Phase::AfterToolResultPublished,
        }
    }
}

/// A typed hook decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    Continue,
    Reject(ErrorDto),
    TransformInput(ToolInput),
    TransformResult(ToolResult),
}

const fn outcome_is_compatible(phase: Phase, outcome: &Outcome) -> bool {
    match outcome {
        Outcome::TransformInput(_) => matches!(
            phase,
            Phase::BeforeToolInvocation
                | Phase::BeforeWorkspaceResolution
                | Phase::AfterWorkspaceResolution
                | Phase::BeforeToolExecution
        ),
        Outcome::TransformResult(_) => matches!(
            phase,
            Phase::AfterToolExecution
                | Phase::BeforeToolResultPersist
                | Phase::BeforeToolResultModelContext
                | Phase::AfterToolResultPublished
        ),
        Outcome::Continue | Outcome::Reject(_) => true,
    }
}

impl Outcome {
    /// Applies this decision to the current result, preserving transform chaining.
    fn apply_result(self, current: &mut Option<ToolResult>) -> DtoResult<()> {
        match self {
            Self::Continue => Ok(()),
            Self::Reject(error) => Err(error),
            Self::TransformResult(result) => {
                *current = Some(result);
                Ok(())
            }
            Self::TransformInput(_) => Err(ErrorDto::validation(
                "invalid_hook_outcome",
                "input transformation is only valid before execution",
            )),
        }
    }
}

fn replace_input(context: PhaseContext, input: ToolInput) -> DtoResult<PhaseContext> {
    let call = match &context {
        PhaseContext::Invocation { call, .. }
        | PhaseContext::WorkspaceResolution { call, .. }
        | PhaseContext::WorkspaceResolved { call, .. }
        | PhaseContext::Execution { call, .. } => *call,
        _ => {
            return Err(ErrorDto::validation(
                "invalid_hook_outcome",
                "input transformation is only valid before execution",
            ));
        }
    };
    Ok(match context {
        PhaseContext::Invocation { .. } => PhaseContext::Invocation { call, input },
        PhaseContext::WorkspaceResolution { .. } => {
            PhaseContext::WorkspaceResolution { call, input }
        }
        PhaseContext::WorkspaceResolved { .. } => PhaseContext::WorkspaceResolved { call, input },
        PhaseContext::Execution { .. } => PhaseContext::Execution { call, input },
        _ => unreachable!(),
    })
}

/// A hook implementation. Hooks do not own persistence or publication.
pub trait Hook: Send + Sync {
    fn id(&self) -> &'static str;
    fn phases(&self) -> &'static [Phase];
    fn priority(&self) -> u32;
    /// Stable revision of this hook's registration contract.
    fn registration_revision(&self) -> u32 {
        1
    }
    /// Returns the failure policy for each declared phase.
    fn failure_policy(&self, _: Phase) -> FailurePolicy {
        FailurePolicy::FailClosed
    }
    /// Returns safe metadata for diagnostics without exposing hook internals.
    fn observability(&self, phase: Phase) -> HookObservability {
        HookObservability {
            hook_id: self.id(),
            registration_revision: self.registration_revision(),
            phase,
            failure_policy: self.failure_policy(phase),
        }
    }
    /// # Errors
    ///
    /// Returns a typed hook failure or policy rejection.
    fn run(&self, context: &PhaseContext) -> DtoResult<Outcome>;
}

/// Deterministically ordered hook registry.
#[derive(Default)]
pub struct Registry {
    hooks: Vec<Box<dyn Hook>>,
}
impl Registry {
    /// Creates an empty hook registry.
    #[must_use]
    pub const fn new() -> Self {
        Self { hooks: Vec::new() }
    }
    /// Registers a unique hook id.
    ///
    /// # Errors
    ///
    /// Returns an error when the hook identifier is already registered.
    pub fn register(&mut self, hook: Box<dyn Hook>) -> DtoResult<()> {
        if hook.id().is_empty() {
            return Err(ErrorDto::validation(
                "invalid_hook_id",
                "hook identifier must not be empty",
            ));
        }
        if self.hooks.iter().any(|h| h.id() == hook.id()) {
            return Err(ErrorDto::validation(
                "duplicate_hook",
                "hook identifier is already registered",
            ));
        }
        let phases = hook.phases();
        if phases
            .iter()
            .enumerate()
            .any(|(index, phase)| phases[..index].contains(phase))
        {
            return Err(ErrorDto::validation(
                "duplicate_hook_phase",
                "hook declares a phase more than once",
            ));
        }
        self.hooks.push(hook);
        self.hooks
            .sort_by_key(|h| (h.priority(), h.registration_revision(), h.id()));
        Ok(())
    }
    /// Runs hooks for a phase in deterministic order.
    ///
    /// # Errors
    ///
    /// Returns a typed hook failure from the selected hook.
    pub fn dispatch(&self, context: &PhaseContext) -> DtoResult<Outcome> {
        Ok(self.dispatch_with_observability(context)?.outcome)
    }

    /// Runs hooks and returns the outcome plus safe metadata for tolerated failures.
    ///
    /// # Errors
    ///
    /// Returns a typed hook failure from a fail-closed hook or an invalid outcome.
    pub fn dispatch_with_observability(&self, context: &PhaseContext) -> DtoResult<DispatchResult> {
        let mut current = context.clone();
        let mut current_result = None;
        let mut failures = Vec::new();
        for hook in &self.hooks {
            if hook.phases().contains(&current.phase()) {
                // A hook error is a failed dispatch, while an explicit rejection is
                // an ordinary policy outcome. Neither permits later hooks to run.
                let outcome = match hook.run(&current) {
                    Ok(outcome) => outcome,
                    Err(_error)
                        if hook.failure_policy(current.phase()) == FailurePolicy::FailOpen =>
                    {
                        // Failure-open is intentionally an internal seam: dispatch has no
                        // publication or storage ownership, so callers may observe metadata.
                        failures.push(hook.observability(current.phase()));
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                match outcome {
                    Outcome::Reject(error) => {
                        return Ok(DispatchResult {
                            outcome: Outcome::Reject(error),
                            failures,
                        });
                    }
                    Outcome::TransformInput(input) => {
                        if !outcome_is_compatible(
                            current.phase(),
                            &Outcome::TransformInput(input.clone()),
                        ) {
                            return Err(ErrorDto::validation(
                                "invalid_hook_outcome",
                                "input transformation is incompatible with its phase",
                            ));
                        }
                        current = replace_input(current, input)?;
                    }
                    outcome => {
                        if !outcome_is_compatible(current.phase(), &outcome) {
                            return Err(ErrorDto::validation(
                                "invalid_hook_outcome",
                                "hook outcome is incompatible with its phase",
                            ));
                        }
                        outcome.apply_result(&mut current_result)?;
                    }
                }
            }
        }
        let outcome = current_result.map_or_else(
            || match current {
                PhaseContext::Invocation { input, .. }
                | PhaseContext::WorkspaceResolution { input, .. }
                | PhaseContext::WorkspaceResolved { input, .. }
                | PhaseContext::Execution { input, .. } => {
                    let original = match context {
                        PhaseContext::Invocation { input, .. }
                        | PhaseContext::WorkspaceResolution { input, .. }
                        | PhaseContext::WorkspaceResolved { input, .. }
                        | PhaseContext::Execution { input, .. } => input,
                        _ => unreachable!(),
                    };
                    if input != *original {
                        Ok(Outcome::TransformInput(input))
                    } else {
                        Ok(Outcome::Continue)
                    }
                }
                _ => Ok(Outcome::Continue),
            },
            |result| Ok(Outcome::TransformResult(result)),
        )?;
        Ok(DispatchResult { outcome, failures })
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "Test fixtures use unwrap for fixed valid values."
)]
mod tests {
    use super::*;
    use intention_tools::{BoundedText, ExecuteInput};
    struct H {
        id: &'static str,
        priority: u32,
        outcome: Outcome,
        phases: &'static [Phase],
    }
    impl Hook for H {
        fn id(&self) -> &'static str {
            self.id
        }
        fn phases(&self) -> &'static [Phase] {
            self.phases
        }
        fn priority(&self) -> u32 {
            self.priority
        }
        fn run(&self, _: &PhaseContext) -> DtoResult<Outcome> {
            Ok(self.outcome.clone())
        }
    }
    fn ctx() -> PhaseContext {
        PhaseContext::Executed {
            call: ToolCallId::new(),
            input: ToolInput::Execute(ExecuteInput {
                program: BoundedText::new("x").unwrap(),
                args: vec![],
            }),
            result: ToolResult::Execute(intention_tools::TextResult {
                text: BoundedText::new("x").unwrap(),
            }),
        }
    }
    #[test]
    fn orders_duplicates_rejects_and_chains_transforms() {
        static P: [Phase; 1] = [Phase::AfterToolExecution];
        let mut r = Registry::default();
        r.register(Box::new(H {
            id: "b",
            priority: 2,
            outcome: Outcome::TransformResult(ToolResult::Execute(intention_tools::TextResult {
                text: BoundedText::new("b").unwrap(),
            })),
            phases: &P,
        }))
        .unwrap();
        r.register(Box::new(H {
            id: "a",
            priority: 1,
            outcome: Outcome::TransformResult(ToolResult::Execute(intention_tools::TextResult {
                text: BoundedText::new("a").unwrap(),
            })),
            phases: &P,
        }))
        .unwrap();
        assert!(
            r.register(Box::new(H {
                id: "a",
                priority: 3,
                outcome: Outcome::Continue,
                phases: &P
            }))
            .is_err()
        );
        assert!(matches!(
            r.dispatch(&ctx()).unwrap(),
            Outcome::TransformResult(_)
        ));
    }

    #[test]
    fn rejects_non_adjacent_duplicate_phases() {
        static P: [Phase; 3] = [
            Phase::BeforeToolExecution,
            Phase::AfterToolExecution,
            Phase::BeforeToolExecution,
        ];
        let mut registry = Registry::new();
        let error_code = registry
            .register(Box::new(H {
                id: "duplicate-phase",
                priority: 0,
                outcome: Outcome::Continue,
                phases: &P,
            }))
            .err()
            .map(|error| error.code().to_string());
        assert_eq!(error_code.as_deref(), Some("duplicate_hook_phase"));
    }
    #[test]
    fn empty_pipeline_continues_and_rejection_stops() {
        let mut r = Registry::default();
        assert_eq!(r.dispatch(&ctx()).unwrap(), Outcome::Continue);
        static P: [Phase; 1] = [Phase::AfterToolExecution];
        r.register(Box::new(H {
            id: "reject",
            priority: 0,
            outcome: Outcome::Reject(ErrorDto::validation("no", "no")),
            phases: &P,
        }))
        .unwrap();
        assert!(matches!(r.dispatch(&ctx()).unwrap(), Outcome::Reject(_)));
    }

    #[test]
    fn transform_is_input_to_the_next_hook_and_rejection_short_circuits() {
        use std::sync::{Arc, Mutex};

        struct Recorder {
            id: &'static str,
            priority: u32,
            seen: Arc<Mutex<Vec<String>>>,
            result: Option<ToolResult>,
            reject: bool,
        }
        impl Hook for Recorder {
            fn id(&self) -> &'static str {
                self.id
            }
            fn phases(&self) -> &'static [Phase] {
                static P: [Phase; 1] = [Phase::AfterToolExecution];
                &P
            }
            fn priority(&self) -> u32 {
                self.priority
            }
            fn run(&self, context: &PhaseContext) -> DtoResult<Outcome> {
                if let PhaseContext::Executed { input, .. } = context {
                    self.seen
                        .lock()
                        .unwrap()
                        .push(format!("{}:{input:?}", self.id));
                }
                if self.reject {
                    Ok(Outcome::Reject(ErrorDto::validation("blocked", "blocked")))
                } else {
                    Ok(self
                        .result
                        .clone()
                        .map_or(Outcome::Continue, Outcome::TransformResult))
                }
            }
        }
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut registry = Registry::default();
        registry
            .register(Box::new(Recorder {
                id: "first",
                priority: 1,
                seen: seen.clone(),
                result: Some(ToolResult::Execute(intention_tools::TextResult {
                    text: BoundedText::new("changed").unwrap(),
                })),
                reject: false,
            }))
            .unwrap();
        registry
            .register(Box::new(Recorder {
                id: "second",
                priority: 2,
                seen: seen.clone(),
                result: None,
                reject: true,
            }))
            .unwrap();
        registry
            .register(Box::new(Recorder {
                id: "third",
                priority: 3,
                seen: seen.clone(),
                result: None,
                reject: false,
            }))
            .unwrap();
        assert!(matches!(
            registry.dispatch(&ctx()).unwrap(),
            Outcome::Reject(_)
        ));
        let log = seen.lock().unwrap();
        assert_eq!(log.len(), 2);
        assert!(log[0].starts_with("first:"));
        assert!(log[1].starts_with("second:"));
        drop(log);
    }

    #[test]
    fn every_context_reports_its_phase() {
        let call = ToolCallId::new();
        let input = ctx_input();
        let result = ToolResult::Execute(intention_tools::TextResult {
            text: BoundedText::new("ok").unwrap(),
        });
        let cases = [
            (
                PhaseContext::Invocation {
                    call,
                    input: input.clone(),
                },
                Phase::BeforeToolInvocation,
            ),
            (
                PhaseContext::WorkspaceResolution {
                    call,
                    input: input.clone(),
                },
                Phase::BeforeWorkspaceResolution,
            ),
            (
                PhaseContext::WorkspaceResolved {
                    call,
                    input: input.clone(),
                },
                Phase::AfterWorkspaceResolution,
            ),
            (
                PhaseContext::Execution {
                    call,
                    input: input.clone(),
                },
                Phase::BeforeToolExecution,
            ),
            (
                PhaseContext::Executed {
                    call,
                    input,
                    result: result.clone(),
                },
                Phase::AfterToolExecution,
            ),
            (
                PhaseContext::Persist {
                    call,
                    result: result.clone(),
                },
                Phase::BeforeToolResultPersist,
            ),
            (
                PhaseContext::ModelContext {
                    call,
                    result: result.clone(),
                },
                Phase::BeforeToolResultModelContext,
            ),
            (
                PhaseContext::Published { call, result },
                Phase::AfterToolResultPublished,
            ),
        ];
        for (context, expected) in cases {
            assert_eq!(context.phase(), expected);
        }
    }

    fn ctx_input() -> ToolInput {
        ToolInput::Execute(ExecuteInput {
            program: BoundedText::new("x").unwrap(),
            args: vec![],
        })
    }

    #[test]
    fn skips_hooks_for_other_phases_and_supports_all_input_phases() {
        static ALL: [Phase; 4] = [
            Phase::BeforeToolInvocation,
            Phase::BeforeWorkspaceResolution,
            Phase::AfterWorkspaceResolution,
            Phase::BeforeToolExecution,
        ];
        let changed = ToolInput::Execute(ExecuteInput {
            program: BoundedText::new("changed").unwrap(),
            args: vec![],
        });
        let mut registry = Registry::new();
        registry
            .register(Box::new(H {
                id: "input",
                priority: 0,
                outcome: Outcome::TransformInput(changed.clone()),
                phases: &ALL,
            }))
            .unwrap();
        for context in [
            PhaseContext::Invocation {
                call: ToolCallId::new(),
                input: ctx_input(),
            },
            PhaseContext::WorkspaceResolution {
                call: ToolCallId::new(),
                input: ctx_input(),
            },
            PhaseContext::WorkspaceResolved {
                call: ToolCallId::new(),
                input: ctx_input(),
            },
            PhaseContext::Execution {
                call: ToolCallId::new(),
                input: ctx_input(),
            },
        ] {
            assert_eq!(
                registry.dispatch(&context).unwrap(),
                Outcome::TransformInput(changed.clone())
            );
        }
        let published = PhaseContext::Published {
            call: ToolCallId::new(),
            result: ToolResult::Execute(intention_tools::TextResult {
                text: BoundedText::new("ok").unwrap(),
            }),
        };
        assert_eq!(registry.dispatch(&published).unwrap(), Outcome::Continue);
    }

    #[test]
    fn invalid_input_transform_and_hook_errors_are_reported() {
        static P: [Phase; 1] = [Phase::AfterToolExecution];
        let mut registry = Registry::new();
        registry
            .register(Box::new(H {
                id: "bad",
                priority: 0,
                outcome: Outcome::TransformInput(ctx_input()),
                phases: &P,
            }))
            .unwrap();
        let context = ctx();
        assert!(registry.dispatch(&context).is_err());

        struct F;
        impl Hook for F {
            fn id(&self) -> &'static str {
                "failure"
            }
            fn phases(&self) -> &'static [Phase] {
                static P: [Phase; 1] = [Phase::AfterToolExecution];
                &P
            }
            fn priority(&self) -> u32 {
                0
            }
            fn run(&self, _: &PhaseContext) -> DtoResult<Outcome> {
                Err(ErrorDto::validation("failed", "failed"))
            }
        }
        let mut errors = Registry::new();
        errors.register(Box::new(F)).unwrap();
        assert!(errors.dispatch(&ctx()).is_err());
    }

    #[test]
    fn default_policy_is_fail_closed_and_metadata_is_safe() {
        static P: [Phase; 1] = [Phase::BeforeToolExecution];
        let hook = H {
            id: "observe",
            priority: 1,
            phases: &P,
            outcome: Outcome::Continue,
        };
        assert_eq!(
            hook.failure_policy(Phase::BeforeToolExecution),
            FailurePolicy::FailClosed
        );
        assert_eq!(
            hook.observability(Phase::BeforeToolExecution),
            HookObservability {
                hook_id: "observe",
                registration_revision: 1,
                phase: Phase::BeforeToolExecution,
                failure_policy: FailurePolicy::FailClosed,
            }
        );
    }

    #[test]
    fn input_transform_is_invalid_after_execution() {
        static P: [Phase; 1] = [Phase::AfterToolExecution];
        let mut registry = Registry::new();
        registry
            .register(Box::new(H {
                id: "bad-input",
                priority: 0,
                outcome: Outcome::TransformInput(ctx_input()),
                phases: &P,
            }))
            .unwrap();
        let context = PhaseContext::Executed {
            call: ToolCallId::new(),
            input: ctx_input(),
            result: ToolResult::Execute(intention_tools::TextResult {
                text: BoundedText::new("result").unwrap(),
            }),
        };
        assert!(registry.dispatch(&context).is_err());
    }

    #[test]
    fn result_transform_applies_on_result_phases() {
        static P: [Phase; 3] = [
            Phase::AfterToolExecution,
            Phase::BeforeToolResultPersist,
            Phase::BeforeToolResultModelContext,
        ];
        let result = ToolResult::Execute(intention_tools::TextResult {
            text: BoundedText::new("changed").unwrap(),
        });
        let mut registry = Registry::new();
        registry
            .register(Box::new(H {
                id: "result",
                priority: 0,
                outcome: Outcome::TransformResult(result.clone()),
                phases: &P,
            }))
            .unwrap();
        for context in [
            PhaseContext::Executed {
                call: ToolCallId::new(),
                input: ctx_input(),
                result: result.clone(),
            },
            PhaseContext::Persist {
                call: ToolCallId::new(),
                result: result.clone(),
            },
            PhaseContext::ModelContext {
                call: ToolCallId::new(),
                result: result.clone(),
            },
        ] {
            assert_eq!(
                registry.dispatch(&context).unwrap(),
                Outcome::TransformResult(result.clone())
            );
        }
    }

    #[test]
    fn fail_open_continues_and_reports_metadata() {
        struct F;
        impl Hook for F {
            fn id(&self) -> &'static str {
                "open"
            }
            fn phases(&self) -> &'static [Phase] {
                static P: [Phase; 1] = [Phase::AfterToolExecution];
                &P
            }
            fn priority(&self) -> u32 {
                0
            }
            fn failure_policy(&self, _: Phase) -> FailurePolicy {
                FailurePolicy::FailOpen
            }
            fn run(&self, _: &PhaseContext) -> DtoResult<Outcome> {
                Err(ErrorDto::validation("failed", "failed"))
            }
        }
        let mut registry = Registry::new();
        registry.register(Box::new(F)).unwrap();
        let result = registry.dispatch_with_observability(&ctx()).unwrap();
        assert_eq!(result.outcome, Outcome::Continue);
        assert_eq!(
            result.failures,
            vec![HookObservability {
                hook_id: "open",
                registration_revision: 1,
                phase: Phase::AfterToolExecution,
                failure_policy: FailurePolicy::FailOpen,
            }]
        );
    }

    #[test]
    fn fail_open_then_fail_closed_and_result_reject_paths() {
        struct F {
            id: &'static str,
            policy: FailurePolicy,
            outcome: DtoResult<Outcome>,
        }
        impl Hook for F {
            fn id(&self) -> &'static str {
                self.id
            }
            fn phases(&self) -> &'static [Phase] {
                static P: [Phase; 1] = [Phase::AfterToolExecution];
                &P
            }
            fn priority(&self) -> u32 {
                0
            }
            fn failure_policy(&self, _: Phase) -> FailurePolicy {
                self.policy
            }
            fn run(&self, _: &PhaseContext) -> DtoResult<Outcome> {
                self.outcome.clone()
            }
        }
        let mut open = Registry::new();
        open.register(Box::new(F {
            id: "open",
            policy: FailurePolicy::FailOpen,
            outcome: Err(ErrorDto::validation("x", "x")),
        }))
        .unwrap();
        open.register(Box::new(F {
            id: "closed",
            policy: FailurePolicy::FailClosed,
            outcome: Err(ErrorDto::validation("y", "y")),
        }))
        .unwrap();
        assert!(open.dispatch_with_observability(&ctx()).is_err());

        let result = ToolResult::Execute(intention_tools::TextResult {
            text: BoundedText::new("final").unwrap(),
        });
        let mut reject = Registry::new();
        reject
            .register(Box::new(F {
                id: "reject",
                policy: FailurePolicy::FailClosed,
                outcome: Ok(Outcome::Reject(ErrorDto::validation("blocked", "blocked"))),
            }))
            .unwrap();
        let executed = PhaseContext::Executed {
            call: ToolCallId::new(),
            input: ctx_input(),
            result: result.clone(),
        };
        assert_eq!(
            reject
                .dispatch_with_observability(&executed)
                .unwrap()
                .failures,
            vec![]
        );
        let mut transform = Registry::new();
        transform
            .register(Box::new(F {
                id: "result",
                policy: FailurePolicy::FailClosed,
                outcome: Ok(Outcome::TransformResult(result.clone())),
            }))
            .unwrap();
        assert_eq!(
            transform.dispatch(&executed).unwrap(),
            Outcome::TransformResult(result)
        );
    }

    #[test]
    fn reject_outcome_in_result_dispatch_is_returned_as_error_when_applied_directly() {
        let mut current = None;
        let error = ErrorDto::validation("blocked", "blocked");
        assert_eq!(
            Outcome::Reject(error.clone()).apply_result(&mut current),
            Err(error)
        );
        assert!(current.is_none());
    }

    #[test]
    fn input_transform_on_result_outcome_is_rejected_by_result_application() {
        let mut current = None;
        assert!(
            Outcome::TransformInput(ctx_input())
                .apply_result(&mut current)
                .is_err()
        );
        assert!(current.is_none());
    }

    #[test]
    fn unchanged_and_changed_input_outcomes_are_distinguished() {
        static P: [Phase; 1] = [Phase::BeforeToolExecution];
        let mut same = Registry::new();
        same.register(Box::new(H {
            id: "same",
            priority: 0,
            phases: &P,
            outcome: Outcome::TransformInput(ctx_input()),
        }))
        .unwrap();
        assert_eq!(
            same.dispatch(&PhaseContext::Execution {
                call: ToolCallId::new(),
                input: ctx_input()
            })
            .unwrap(),
            Outcome::Continue
        );
        let changed = ToolInput::Execute(ExecuteInput {
            program: BoundedText::new("different").unwrap(),
            args: vec![],
        });
        let mut different = Registry::new();
        different
            .register(Box::new(H {
                id: "different",
                priority: 0,
                phases: &P,
                outcome: Outcome::TransformInput(changed.clone()),
            }))
            .unwrap();
        assert_eq!(
            different
                .dispatch(&PhaseContext::Execution {
                    call: ToolCallId::new(),
                    input: ctx_input()
                })
                .unwrap(),
            Outcome::TransformInput(changed)
        );
    }
}
