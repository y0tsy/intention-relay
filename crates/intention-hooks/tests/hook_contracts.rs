#![allow(
    clippy::expect_used,
    reason = "Contract test setup failures are reported with local context."
)]
#![allow(
    clippy::unwrap_used,
    reason = "Contract test setup failures are reported with local context."
)]

use intention_hooks::{Hook, Outcome, Phase, PhaseContext, Registry};
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
