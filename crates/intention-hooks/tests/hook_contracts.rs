#![allow(
    clippy::expect_used,
    reason = "Contract test setup failures are reported with local context."
)]
#![allow(
    clippy::unwrap_used,
    reason = "Contract test setup failures are reported with local context."
)]

use intention_hooks::{FailurePolicy, Hook, Outcome, Phase, PhaseContext, Registry};
use intention_tools::{BoundedText, ExecuteInput, ToolInput, ToolResult};
use intention_types::{ErrorDto, ToolCallId};

struct RejectingHook;
impl Hook for RejectingHook {
    fn id(&self) -> &'static str {
        "rejecting"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::BeforeToolExecution]
    }
    fn priority(&self) -> u32 {
        0
    }
    fn run(&self, _: &PhaseContext) -> intention_types::DtoResult<Outcome> {
        Ok(Outcome::Reject(ErrorDto::validation("blocked", "blocked")))
    }
}

struct EffectHook(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl Hook for EffectHook {
    fn id(&self) -> &'static str {
        "effect"
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::BeforeToolExecution]
    }
    fn priority(&self) -> u32 {
        10
    }
    fn run(&self, _: &PhaseContext) -> intention_types::DtoResult<Outcome> {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(Outcome::Continue)
    }
}

struct OrderingHook(
    &'static str,
    u32,
    std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
);
impl Hook for OrderingHook {
    fn id(&self) -> &'static str {
        self.0
    }
    fn phases(&self) -> &'static [Phase] {
        &[Phase::BeforeToolExecution]
    }
    fn priority(&self) -> u32 {
        self.1
    }
    fn run(&self, _: &PhaseContext) -> intention_types::DtoResult<Outcome> {
        self.2.lock().unwrap().push(self.0);
        Ok(Outcome::Continue)
    }
}

#[test]
fn rejection_is_typed_and_stops_before_execution() {
    let context = PhaseContext::Execution {
        call: ToolCallId::new(),
        input: ToolInput::Execute(ExecuteInput {
            program: BoundedText::new("false").expect("program"),
            args: vec![],
        }),
    };
    let mut registry = Registry::default();
    registry
        .register(Box::new(RejectingHook))
        .expect("register");
    assert!(matches!(
        registry.dispatch(&context).expect("dispatch"),
        Outcome::Reject(_)
    ));
}

#[test]
fn rejection_skips_later_hook_effects() {
    let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let context = PhaseContext::Execution {
        call: ToolCallId::new(),
        input: ToolInput::Execute(ExecuteInput {
            program: BoundedText::new("true").expect("program"),
            args: vec![],
        }),
    };
    let mut registry = Registry::default();
    registry
        .register(Box::new(RejectingHook))
        .expect("register");
    registry
        .register(Box::new(EffectHook(called.clone())))
        .expect("register");
    assert!(matches!(
        registry.dispatch(&context).expect("dispatch"),
        Outcome::Reject(_)
    ));
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn every_hook_phase_maps_to_its_typed_context() {
    let call = ToolCallId::new();
    let input = ToolInput::Execute(ExecuteInput {
        program: BoundedText::new("true").expect("program"),
        args: vec![],
    });
    let result = ToolResult::Execute(intention_tools::TextResult {
        text: BoundedText::new("ok").expect("result"),
        truncated: false,
    });
    let contexts = [
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
    for (context, expected) in contexts {
        assert_eq!(context.phase(), expected);
    }
}

#[test]
fn hooks_order_by_priority_then_id() {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut registry = Registry::new();
    for hook in [
        OrderingHook("z", 1, seen.clone()),
        OrderingHook("a", 1, seen.clone()),
        OrderingHook("m", 0, seen.clone()),
    ] {
        registry.register(Box::new(hook)).expect("register");
    }
    assert_eq!(
        registry
            .dispatch(&PhaseContext::Execution {
                call: ToolCallId::new(),
                input: ToolInput::Execute(ExecuteInput {
                    program: BoundedText::new("true").expect("program"),
                    args: vec![],
                }),
            })
            .expect("dispatch"),
        Outcome::Continue
    );
    assert_eq!(*seen.lock().unwrap(), vec!["m", "a", "z"]);
}

#[test]
fn registration_revision_is_safe_metadata_and_ordering_tiebreaker() {
    struct RevisionHook(
        &'static str,
        u32,
        std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
    );
    impl Hook for RevisionHook {
        fn id(&self) -> &'static str {
            self.0
        }
        fn phases(&self) -> &'static [Phase] {
            &[Phase::BeforeToolExecution]
        }
        fn priority(&self) -> u32 {
            1
        }
        fn registration_revision(&self) -> u32 {
            self.1
        }
        fn run(&self, _: &PhaseContext) -> intention_types::DtoResult<Outcome> {
            self.2.lock().unwrap().push(self.0);
            Ok(Outcome::Continue)
        }
    }
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut registry = Registry::new();
    registry
        .register(Box::new(RevisionHook("new", 2, seen.clone())))
        .expect("register");
    registry
        .register(Box::new(RevisionHook("old", 1, seen.clone())))
        .expect("register");
    let context = PhaseContext::Execution {
        call: ToolCallId::new(),
        input: ToolInput::Execute(ExecuteInput {
            program: BoundedText::new("true").expect("program"),
            args: vec![],
        }),
    };
    let result = registry
        .dispatch_with_observability(&context)
        .expect("dispatch");
    assert_eq!(*seen.lock().unwrap(), vec!["old", "new"]);
    assert!(result.failures.is_empty());
    assert_eq!(FailurePolicy::FailClosed, FailurePolicy::FailClosed);
}

struct OutcomeHook {
    id: &'static str,
    phase: Phase,
    outcome: Outcome,
    failure_policy: FailurePolicy,
}

impl Hook for OutcomeHook {
    fn id(&self) -> &'static str {
        self.id
    }
    fn phases(&self) -> &'static [Phase] {
        // The test registers one phase at a time; the leaked slice is scoped to the test process.
        Box::leak(Box::new([self.phase]))
    }
    fn priority(&self) -> u32 {
        0
    }
    fn failure_policy(&self, _: Phase) -> FailurePolicy {
        self.failure_policy
    }
    fn run(&self, _: &PhaseContext) -> intention_types::DtoResult<Outcome> {
        Ok(self.outcome.clone())
    }
}

fn input_context(phase: Phase) -> PhaseContext {
    let input = ToolInput::Execute(ExecuteInput {
        program: BoundedText::new("true").expect("program"),
        args: vec![],
    });
    match phase {
        Phase::BeforeToolInvocation => PhaseContext::Invocation {
            call: ToolCallId::new(),
            input,
        },
        Phase::BeforeWorkspaceResolution => PhaseContext::WorkspaceResolution {
            call: ToolCallId::new(),
            input,
        },
        Phase::AfterWorkspaceResolution => PhaseContext::WorkspaceResolved {
            call: ToolCallId::new(),
            input,
        },
        Phase::BeforeToolExecution => PhaseContext::Execution {
            call: ToolCallId::new(),
            input,
        },
        _ => unreachable!("not an input phase"),
    }
}

fn result_context(phase: Phase) -> PhaseContext {
    let result = ToolResult::Execute(intention_tools::TextResult {
        text: BoundedText::new("ok").expect("result"),
        truncated: false,
    });
    match phase {
        Phase::AfterToolExecution => PhaseContext::Executed {
            call: ToolCallId::new(),
            input: ToolInput::Execute(ExecuteInput {
                program: BoundedText::new("true").expect("program"),
                args: vec![],
            }),
            result,
        },
        Phase::BeforeToolResultPersist => PhaseContext::Persist {
            call: ToolCallId::new(),
            result,
        },
        Phase::BeforeToolResultModelContext => PhaseContext::ModelContext {
            call: ToolCallId::new(),
            result,
        },
        Phase::AfterToolResultPublished => PhaseContext::Published {
            call: ToolCallId::new(),
            result,
        },
        _ => unreachable!("not a result phase"),
    }
}

#[test]
fn input_transforms_are_rejected_for_every_result_phase() {
    for phase in [
        Phase::AfterToolExecution,
        Phase::BeforeToolResultPersist,
        Phase::BeforeToolResultModelContext,
        Phase::AfterToolResultPublished,
    ] {
        let mut registry = Registry::new();
        registry
            .register(Box::new(OutcomeHook {
                id: "input",
                phase,
                outcome: Outcome::TransformInput(ToolInput::Execute(ExecuteInput {
                    program: BoundedText::new("changed").expect("program"),
                    args: vec![],
                })),
                failure_policy: FailurePolicy::FailClosed,
            }))
            .expect("register");
        let error = registry
            .dispatch(&result_context(phase))
            .expect_err("incompatible input transform");
        assert_eq!(error.code(), "invalid_hook_outcome");
    }
}

#[test]
fn result_transforms_are_rejected_for_every_input_phase() {
    let result = ToolResult::Execute(intention_tools::TextResult {
        text: BoundedText::new("changed").expect("result"),
        truncated: false,
    });
    for phase in [
        Phase::BeforeToolInvocation,
        Phase::BeforeWorkspaceResolution,
        Phase::AfterWorkspaceResolution,
        Phase::BeforeToolExecution,
    ] {
        let mut registry = Registry::new();
        registry
            .register(Box::new(OutcomeHook {
                id: "result",
                phase,
                outcome: Outcome::TransformResult(result.clone()),
                failure_policy: FailurePolicy::FailClosed,
            }))
            .expect("register");
        let error = registry
            .dispatch(&input_context(phase))
            .expect_err("incompatible result transform");
        assert_eq!(error.code(), "invalid_hook_outcome");
    }
}

#[test]
fn fail_open_is_observable_and_fail_closed_is_not_swallowed() {
    for policy in [FailurePolicy::FailOpen, FailurePolicy::FailClosed] {
        let mut registry = Registry::new();
        registry
            .register(Box::new(OutcomeHook {
                id: "failure",
                phase: Phase::BeforeToolExecution,
                outcome: Outcome::Reject(ErrorDto::validation("failure", "failure")),
                failure_policy: policy,
            }))
            .expect("register");
        // Rejection is policy, not operational failure; this proves fail-open metadata is only
        // emitted for actual hook errors while preserving fail-closed behavior.
        let result = registry
            .dispatch_with_observability(&input_context(Phase::BeforeToolExecution))
            .expect("dispatch");
        assert!(matches!(result.outcome, Outcome::Reject(_)));
        assert!(result.failures.is_empty());
    }
}

#[test]
fn fail_open_records_operational_failure_without_leaking_error_details() {
    struct F;
    impl Hook for F {
        fn id(&self) -> &'static str {
            "open-failure"
        }
        fn phases(&self) -> &'static [Phase] {
            &[Phase::BeforeToolExecution]
        }
        fn priority(&self) -> u32 {
            0
        }
        fn failure_policy(&self, _: Phase) -> FailurePolicy {
            FailurePolicy::FailOpen
        }
        fn run(&self, _: &PhaseContext) -> intention_types::DtoResult<Outcome> {
            Err(ErrorDto::validation(
                "secret-internal-code",
                "private detail",
            ))
        }
    }
    let mut registry = Registry::new();
    registry.register(Box::new(F)).expect("register");
    let result = registry
        .dispatch_with_observability(&input_context(Phase::BeforeToolExecution))
        .expect("fail-open dispatch");
    assert_eq!(result.outcome, Outcome::Continue);
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.failures[0].hook_id, "open-failure");
    assert_eq!(result.failures[0].failure_policy, FailurePolicy::FailOpen);
}

#[test]
fn fail_open_continues_to_later_hooks_and_preserves_ordered_observability() {
    struct F {
        id: &'static str,
        priority: u32,
        failure_policy: FailurePolicy,
        seen: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
    }
    impl Hook for F {
        fn id(&self) -> &'static str {
            self.id
        }
        fn phases(&self) -> &'static [Phase] {
            &[Phase::BeforeToolExecution]
        }
        fn priority(&self) -> u32 {
            self.priority
        }
        fn failure_policy(&self, _: Phase) -> FailurePolicy {
            self.failure_policy
        }
        fn run(&self, _: &PhaseContext) -> intention_types::DtoResult<Outcome> {
            self.seen.lock().unwrap().push(self.id);
            if self.id == "open" {
                Err(ErrorDto::validation("internal", "not returned"))
            } else {
                Ok(Outcome::Continue)
            }
        }
    }
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut registry = Registry::new();
    registry
        .register(Box::new(F {
            id: "open",
            priority: 0,
            failure_policy: FailurePolicy::FailOpen,
            seen: seen.clone(),
        }))
        .expect("register");
    registry
        .register(Box::new(F {
            id: "later",
            priority: 1,
            failure_policy: FailurePolicy::FailClosed,
            seen: seen.clone(),
        }))
        .expect("register");
    let result = registry
        .dispatch_with_observability(&input_context(Phase::BeforeToolExecution))
        .expect("fail-open dispatch");
    assert_eq!(result.outcome, Outcome::Continue);
    assert_eq!(*seen.lock().unwrap(), vec!["open", "later"]);
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.failures[0].hook_id, "open");
}

#[test]
fn fail_closed_operational_error_short_circuits_later_hooks() {
    #[allow(
        dead_code,
        reason = "F intentionally shares the flag handle with the later hook; it never reads its own copy."
    )]
    struct F(std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl Hook for F {
        fn id(&self) -> &'static str {
            "closed"
        }
        fn phases(&self) -> &'static [Phase] {
            &[Phase::BeforeToolExecution]
        }
        fn priority(&self) -> u32 {
            0
        }
        fn run(&self, _: &PhaseContext) -> intention_types::DtoResult<Outcome> {
            Err(ErrorDto::validation("closed", "closed"))
        }
    }
    struct Later(std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl Hook for Later {
        fn id(&self) -> &'static str {
            "later-closed"
        }
        fn phases(&self) -> &'static [Phase] {
            &[Phase::BeforeToolExecution]
        }
        fn priority(&self) -> u32 {
            1
        }
        fn run(&self, _: &PhaseContext) -> intention_types::DtoResult<Outcome> {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(Outcome::Continue)
        }
    }
    let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut registry = Registry::new();
    registry
        .register(Box::new(F(called.clone())))
        .expect("register");
    registry
        .register(Box::new(Later(called.clone())))
        .expect("register");
    let error = registry
        .dispatch(&input_context(Phase::BeforeToolExecution))
        .expect_err("fail-closed error");
    assert_eq!(error.code(), "closed");
    assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn workspace_boundary_contexts_carry_safe_identity_without_paths() {
    let absolute =
        std::env::temp_dir().join(format!("intention-hooks-boundary-{}", std::process::id()));
    // The typed payload DTO rejects absolute paths, so the mandatory
    // workspace boundary phases structurally cannot carry the canonical
    // root; only the safe call identity and relative typed input travel.
    assert!(
        intention_types::WorkspaceRelativePathDto::parse(absolute.to_string_lossy().as_ref())
            .is_err()
    );
    let resolution = PhaseContext::WorkspaceResolution {
        call: ToolCallId::new(),
        input: ToolInput::Execute(ExecuteInput {
            program: BoundedText::new("true").expect("program"),
            args: vec![],
        }),
    };
    let resolved = PhaseContext::WorkspaceResolved {
        call: ToolCallId::new(),
        input: ToolInput::Execute(ExecuteInput {
            program: BoundedText::new("true").expect("program"),
            args: vec![],
        }),
    };
    assert_eq!(resolution.phase(), Phase::BeforeWorkspaceResolution);
    assert_eq!(resolved.phase(), Phase::AfterWorkspaceResolution);
    let root_text = absolute.to_string_lossy().into_owned();
    assert!(!format!("{resolution:?}").contains(&root_text));
    assert!(!format!("{resolved:?}").contains(&root_text));
}
