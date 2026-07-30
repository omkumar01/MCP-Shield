//! Security guardrails.
//!
//! Provides deterministic symbolic guardrails (ePCA) and egress inspection
//! to prevent prompt injection and other attacks without relying on LLMs.

pub mod ecpa;
pub mod egress;

pub use ecpa::{
    ConstraintResult, EcpaError, EcpaGuardrail, PredicateEvaluation, RuleEcpaGuardrail,
    StubEcpaGuardrail, ToolConstraint,
};
pub use egress::{
    EgressInspector, GuardrailError, InjectionPattern, PatternEgressInspector, PatternType,
};
