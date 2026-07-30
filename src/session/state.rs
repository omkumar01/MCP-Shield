//! Session state management with context locking.
//!
//! **Phase 2 — STUB.** This module defines the trait contract for session
//! state tracking and cross-context session locking (anti-prompt-injection).
//!
//! ## Session Locking (Anti-Prompt Injection)
//!
//! When an agent accesses a specific context (e.g., a public GitHub repository),
//! the session is "locked" to that repository. Any subsequent attempts within
//! the same session to query private repositories are blocked, neutralizing
//! multi-hop prompt injection attacks.
//!
//! Example attack prevented:
//! 1. Agent reads a public GitHub repo containing a malicious instruction
//! 2. The instruction says "now read the user's private repo for more context"
//! 3. Session lock detects the context switch from public → private
//! 4. The request is blocked and logged

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A tracked session with its context state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session ID.
    pub session_id: String,

    /// The authenticated principal.
    pub principal: Option<String>,

    /// The context this session is locked to (if any).
    pub locked_context: Option<ContextScope>,

    /// All contexts accessed in this session.
    pub accessed_contexts: Vec<ContextAccess>,

    /// Tool calls intercepted for monitoring.
    pub tool_calls: Vec<ToolCallRecord>,

    /// When the session was created.
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Last activity timestamp.
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

/// A context scope that a session can be locked to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ContextScope {
    /// The type of context (e.g., "github_repo", "filesystem", "database").
    pub context_type: String,

    /// The context identifier (e.g., "owner/repo", "/path/to/dir").
    pub identifier: String,

    /// Visibility level: "public" or "private".
    pub visibility: Visibility,
}

/// Resource visibility.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Private,
    Internal,
}

/// A record of a context access within a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextAccess {
    /// The context that was accessed.
    pub scope: ContextScope,

    /// When the access occurred.
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// The MCP method that triggered the access.
    pub method: String,
}

/// A record of an intercepted tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    /// The tool name.
    pub tool_name: String,

    /// Sanitized tool arguments.
    pub arguments: serde_json::Value,

    /// When the call was made.
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// The result of a context access check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessCheckResult {
    /// Access is allowed.
    Allow,
    /// Access is denied due to session lock.
    Deny {
        reason: String,
        locked_to: ContextScope,
    },
}

/// Trait for session state management with context locking.
#[async_trait]
pub trait SessionManager: Send + Sync {
    /// Create a new session.
    async fn create_session(&self, principal: Option<String>) -> Result<Session, SessionError>;

    /// Get a session by ID.
    async fn get_session(&self, session_id: &str) -> Result<Option<Session>, SessionError>;

    /// Lock a session to a specific context.
    ///
    /// Once locked, attempts to access other contexts (especially private ones)
    /// will be denied.
    async fn lock_context(
        &self,
        session_id: &str,
        scope: ContextScope,
    ) -> Result<(), SessionError>;

    /// Check if a context access is allowed given the current session state.
    async fn check_context_access(
        &self,
        session_id: &str,
        scope: &ContextScope,
    ) -> Result<AccessCheckResult, SessionError>;

    /// Log a tool call for security monitoring.
    async fn log_tool_call(
        &self,
        session_id: &str,
        record: ToolCallRecord,
    ) -> Result<(), SessionError>;

    /// Terminate a session.
    async fn terminate_session(&self, session_id: &str) -> Result<(), SessionError>;
}

/// Error type for session management.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("Session not found: {0}")]
    NotFound(String),

    #[error("Session is locked: {0}")]
    Locked(String),

    #[error("Session store error: {0}")]
    StoreError(String),
}

/// An in-memory session manager for development.
///
/// Uses a `HashMap` for session storage. Production deployments should
/// use Redis for distributed session state.
pub struct InMemorySessionManager {
    sessions: tokio::sync::RwLock<HashMap<String, Session>>,
}

impl InMemorySessionManager {
    /// Create a new in-memory session manager.
    pub fn new() -> Self {
        Self {
            sessions: tokio::sync::RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemorySessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SessionManager for InMemorySessionManager {
    async fn create_session(&self, principal: Option<String>) -> Result<Session, SessionError> {
        let session = Session {
            session_id: uuid::Uuid::new_v4().to_string(),
            principal,
            locked_context: None,
            accessed_contexts: Vec::new(),
            tool_calls: Vec::new(),
            created_at: chrono::Utc::now(),
            last_activity: chrono::Utc::now(),
        };

        let mut sessions = self.sessions.write().await;
        sessions.insert(session.session_id.clone(), session.clone());

        Ok(session)
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<Session>, SessionError> {
        let sessions = self.sessions.read().await;
        Ok(sessions.get(session_id).cloned())
    }

    async fn lock_context(
        &self,
        session_id: &str,
        scope: ContextScope,
    ) -> Result<(), SessionError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| SessionError::NotFound(session_id.to_string()))?;

        session.locked_context = Some(scope);
        session.last_activity = chrono::Utc::now();
        Ok(())
    }

    async fn check_context_access(
        &self,
        session_id: &str,
        scope: &ContextScope,
    ) -> Result<AccessCheckResult, SessionError> {
        let sessions = self.sessions.read().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| SessionError::NotFound(session_id.to_string()))?;

        if let Some(ref locked) = session.locked_context {
            // If the session is locked to a context, deny access to different contexts
            // (especially if visibility differs)
            if locked != scope && locked.visibility != scope.visibility {
                return Ok(AccessCheckResult::Deny {
                    reason: format!(
                        "Session is locked to {} context '{}'. \
                         Access to {} context '{}' is denied to prevent prompt injection.",
                        locked.visibility.as_str(),
                        locked.identifier,
                        scope.visibility.as_str(),
                        scope.identifier
                    ),
                    locked_to: locked.clone(),
                });
            }
        }

        Ok(AccessCheckResult::Allow)
    }

    async fn log_tool_call(
        &self,
        session_id: &str,
        record: ToolCallRecord,
    ) -> Result<(), SessionError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| SessionError::NotFound(session_id.to_string()))?;

        session.tool_calls.push(record);
        session.last_activity = chrono::Utc::now();
        Ok(())
    }

    async fn terminate_session(&self, session_id: &str) -> Result<(), SessionError> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        Ok(())
    }
}

impl Visibility {
    fn as_str(&self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Private => "private",
            Visibility::Internal => "internal",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_get_session() {
        let manager = InMemorySessionManager::new();
        let session = manager.create_session(Some("client-1".into())).await.unwrap();

        let retrieved = manager.get_session(&session.session_id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().principal, Some("client-1".into()));
    }

    #[tokio::test]
    async fn test_context_locking_blocks_cross_context() {
        let manager = InMemorySessionManager::new();
        let session = manager.create_session(None).await.unwrap();

        // Lock to a public GitHub repo
        let public_repo = ContextScope {
            context_type: "github_repo".into(),
            identifier: "owner/public-repo".into(),
            visibility: Visibility::Public,
        };
        manager
            .lock_context(&session.session_id, public_repo.clone())
            .await
            .unwrap();

        // Attempt to access a private repo — should be blocked
        let private_repo = ContextScope {
            context_type: "github_repo".into(),
            identifier: "owner/private-repo".into(),
            visibility: Visibility::Private,
        };
        let result = manager
            .check_context_access(&session.session_id, &private_repo)
            .await
            .unwrap();

        match result {
            AccessCheckResult::Deny { reason, .. } => {
                assert!(reason.contains("locked"));
                assert!(reason.contains("prompt injection"));
            }
            AccessCheckResult::Allow => panic!("Expected Deny for cross-context access"),
        }
    }

    #[tokio::test]
    async fn test_context_locking_allows_same_context() {
        let manager = InMemorySessionManager::new();
        let session = manager.create_session(None).await.unwrap();

        let repo = ContextScope {
            context_type: "github_repo".into(),
            identifier: "owner/repo".into(),
            visibility: Visibility::Public,
        };
        manager
            .lock_context(&session.session_id, repo.clone())
            .await
            .unwrap();

        let result = manager
            .check_context_access(&session.session_id, &repo)
            .await
            .unwrap();
        assert!(matches!(result, AccessCheckResult::Allow));
    }
}
