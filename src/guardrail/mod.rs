//! Security guardrails.
//!
//! Provides deterministic symbolic guardrails (ePCA) and egress inspection
//! to prevent prompt injection and other attacks without relying on LLMs.

pub mod ecpa;
pub mod egress;

pub use ecpa::{
    ConstraintBuilder, ConstraintLoader, ConstraintResult, EcpaError, EcpaGuardrail,
    PredicateEvaluation, RuleEcpaGuardrail, StubEcpaGuardrail, ToolConstraint,
};
pub use egress::{
    EgressInspector, GuardrailError, InjectionPattern, InspectableResult, InspectionResult,
    PatternEgressInspector, PatternType, StubEgressInspector,
};
