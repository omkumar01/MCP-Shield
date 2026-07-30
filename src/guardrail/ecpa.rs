//! ePCA (executable Proof-Constrained Actions) symbolic guardrails.
//!
//! **Phase 3 — STUB.** This module defines the trait contract for the ePCA
//! framework, which forces the AI agent's intended operations into first-order
//! logical mathematical constraints before execution.
//!
//! ## Why Not LLMs for Security?
//!
//! Per the MCP-Shield specification: "Do not integrate LLMs for real-time
//! security decision-making." LLMs are probabilistic and can be manipulated
//! via prompt injection. Instead, ePCA provides mathematically verifiable
//! security with a zero attack success rate for predefined vulnerabilities,
//! without sacrificing agent utility or introducing LLM latency.
//!
//! ## How ePCA Works
//!
//! 1. Each critical tool (filesystem, shell, network) has a set of formal
//!    constraints expressed in first-order logic (FOL).
//! 2. Before a tool executes, its arguments are translated into logical
//!    predicates.
//! 3. A deterministic solver checks if the predicates satisfy all constraints.
//! 4. If any constraint is violated, execution is blocked with a precise
//!    mathematical proof of why.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// A constraint definition for a critical tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConstraint {
    /// The tool name these constraints apply to.
    pub tool_name: String,

    /// Human-readable description of the constraint set.
    pub description: String,

    /// The formal constraints as predicates.
    pub predicates: Vec<Predicate>,

    /// Whether violations block execution entirely.
    pub block_on_violation: bool,
}

/// A single predicate in a constraint set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Predicate {
    /// The predicate name (e.g., "path_within_root", "is_safe_command").
    pub name: String,

    /// The argument fields this predicate applies to.
    pub fields: Vec<String>,

    /// The constraint expression (e.g., "starts_with('/allowed/')").
    pub expression: String,

    /// Whether this predicate must be satisfied (required) or is a warning.
    pub required: bool,
}

/// The result of an ePCA constraint evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintResult {
    /// Whether all constraints were satisfied.
    pub satisfied: bool,

    /// The specific predicates that were evaluated.
    pub evaluations: Vec<PredicateEvaluation>,

    /// A human-readable summary.
    pub summary: String,
}

/// The evaluation of a single predicate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredicateEvaluation {
    /// The predicate name.
    pub name: String,

    /// Whether the predicate was satisfied.
    pub satisfied: bool,

    /// The evaluated value.
    pub value: String,

    /// Explanation of the result.
    pub explanation: String,
}

/// Trait for ePCA symbolic guardrail evaluation.
#[async_trait]
pub trait EcpaGuardrail: Send + Sync {
    /// Evaluate the constraints for a tool call.
    ///
    /// Returns `Ok(ConstraintResult)` with `satisfied=true` if all constraints
    /// are met, or `satisfied=false` if any constraint is violated.
    async fn evaluate_constraints(
        &self,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<ConstraintResult, EcpaError>;

    /// Register a constraint set for a tool.
    async fn register_constraints(&self, constraints: ToolConstraint) -> Result<(), EcpaError>;

    /// List all registered constraint sets.
    async fn list_constraints(&self) -> Vec<String>;
}

/// Error type for ePCA evaluation.
#[derive(Debug, thiserror::Error)]
pub enum EcpaError {
    #[error("Constraint evaluation error: {0}")]
    EvaluationError(String),

    #[error("Unknown tool: {0}")]
    UnknownTool(String),

    #[error("Predicate parse error: {0}")]
    ParseError(String),
}

/// A stub ePCA guardrail for Phase 1.
///
/// All tool calls are allowed — real constraint evaluation is deferred to Phase 3.
pub struct StubEcpaGuardrail;

#[async_trait]
impl EcpaGuardrail for StubEcpaGuardrail {
    async fn evaluate_constraints(
        &self,
        tool_name: &str,
        _arguments: &Value,
    ) -> Result<ConstraintResult, EcpaError> {
        Ok(ConstraintResult {
            satisfied: true,
            evaluations: vec![PredicateEvaluation {
                name: "stub".to_string(),
                satisfied: true,
                value: "always_true".to_string(),
                explanation: format!(
                    "Phase 1 stub: tool '{}' allowed without constraint evaluation",
                    tool_name
                ),
            }],
            summary: "All constraints satisfied (stub)".to_string(),
        })
    }

    async fn register_constraints(&self, _constraints: ToolConstraint) -> Result<(), EcpaError> {
        tracing::warn!("StubEcpaGuardrail::register_constraints() is a no-op (Phase 3)");
        Ok(())
    }

    async fn list_constraints(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Production ePCA guardrail with real constraint evaluation.
///
/// **TODO (Phase 3):** Implement using a deterministic solver.
/// The implementation will:
/// 1. Maintain a registry of `ToolConstraint` sets
/// 2. Translate tool arguments into logical predicates
/// 3. Evaluate each predicate deterministically
/// 4. Return a precise proof of any violation
pub struct RuleEcpaGuardrail {
    constraints: tokio::sync::RwLock<HashMap<String, ToolConstraint>>,
}

impl RuleEcpaGuardrail {
    /// Create a new rule-based ePCA guardrail.
    pub fn new() -> Self {
        Self {
            constraints: tokio::sync::RwLock::new(HashMap::new()),
        }
    }
}

impl Default for RuleEcpaGuardrail {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EcpaGuardrail for RuleEcpaGuardrail {
    async fn evaluate_constraints(
        &self,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<ConstraintResult, EcpaError> {
        let constraints = self.constraints.read().await;
        let constraint = constraints
            .get(tool_name)
            .ok_or_else(|| EcpaError::UnknownTool(tool_name.to_string()))?;

        let mut evaluations = Vec::new();
        let mut all_satisfied = true;

        for predicate in &constraint.predicates {
            let satisfied = self.evaluate_predicate(predicate, arguments);
            if !satisfied && predicate.required {
                all_satisfied = false;
            }
            evaluations.push(PredicateEvaluation {
                name: predicate.name.clone(),
                satisfied,
                value: self.extract_field_value(predicate, arguments),
                explanation: if satisfied {
                    format!("Predicate '{}' satisfied", predicate.name)
                } else {
                    format!(
                        "Predicate '{}' violated: {}",
                        predicate.name, predicate.expression
                    )
                },
            });
        }

        Ok(ConstraintResult {
            satisfied: all_satisfied,
            evaluations,
            summary: if all_satisfied {
                "All constraints satisfied".to_string()
            } else {
                "One or more required constraints violated".to_string()
            },
        })
    }

    async fn register_constraints(&self, constraints: ToolConstraint) -> Result<(), EcpaError> {
        let mut store = self.constraints.write().await;
        store.insert(constraints.tool_name.clone(), constraints);
        Ok(())
    }

    async fn list_constraints(&self) -> Vec<String> {
        let constraints = self.constraints.read().await;
        constraints.keys().cloned().collect()
    }
}

impl RuleEcpaGuardrail {
    /// Evaluate a single predicate against the arguments.
    fn evaluate_predicate(&self, predicate: &Predicate, arguments: &Value) -> bool {
        // Simplified predicate evaluation for the stub.
        // Full implementation will parse the expression and evaluate it.
        let expr = &predicate.expression;
        for field in &predicate.fields {
            let value = arguments.get(field).and_then(|v| v.as_str()).unwrap_or("");
            if expr.contains("starts_with(") {
                let prefix = expr
                    .trim_start_matches("starts_with(")
                    .trim_end_matches(")")
                    .trim_matches(|c| c == '\'' || c == '"');
                if !value.starts_with(prefix) {
                    return false;
                }
            }
        }
        true
    }

    /// Extract a field value for reporting.
    fn extract_field_value(&self, predicate: &Predicate, arguments: &Value) -> String {
        predicate
            .fields
            .iter()
            .filter_map(|f| arguments.get(f).and_then(|v| v.as_str()).map(String::from))
            .collect::<Vec<_>>()
            .join(", ")
    }
}
