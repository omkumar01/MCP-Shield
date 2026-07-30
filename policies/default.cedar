// MCP-Shield Default Cedar Policy
// ================================
// This is the default Cedar policy set for MCP-Shield.
// Cedar provides deterministic, sub-millisecond Attribute-Based Access Control
// (ABAC) for the MCP-Shield gateway.
//
// Policy Syntax: https://docs.cedarpolicy.com/policies/syntax.html

// ── Entity Declarations ──────────────────────────────────────────────
// These define the entity types used in the policy evaluation.

// The principal is always a Client (the MCP Host / AI application).
// The action is an MCP method (e.g., "tools/call").
// The resource is a Tool, Resource, or Prompt.

// ── Default Permit Policy ────────────────────────────────────────────
// In Phase 1, the default policy permits all authenticated clients to
// perform all actions. Phase 2 will replace this with fine-grained policies.

permit (
    principal == Client::"authenticated",
    action == Action::"tools/list",
    resource
);

permit (
    principal == Client::"authenticated",
    action == Action::"tools/call",
    resource == Tool::"com.echo.echo"
)
when {
    principal has scopes &&
    principal.scopes.contains("mcp:tools:call")
};

permit (
    principal == Client::"authenticated",
    action == Action::"tools/call",
    resource == Tool::"com.echo.add"
)
when {
    principal has scopes &&
    principal.scopes.contains("mcp:tools:call")
};

permit (
    principal == Client::"authenticated",
    action == Action::"resources/list",
    resource
);

permit (
    principal == Client::"authenticated",
    action == Action::"prompts/list",
    resource
);

permit (
    principal == Client::"authenticated",
    action == Action::"ping",
    resource
);

// ── Deny Policy Example ──────────────────────────────────────────────
// Deny access to admin operations for non-admin principals.

forbid (
    principal,
    action == Action::"shutdown",
    resource
)
unless {
    principal has scopes &&
    principal.scopes.contains("mcp:admin")
};
