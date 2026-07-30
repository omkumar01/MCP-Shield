//! Session state management.
//!
//! Provides dynamic state tracking with cross-context session locking
//! to prevent prompt injection attacks.

pub mod state;

pub use state::{
    AccessCheckResult, ContextAccess, ContextScope, InMemorySessionManager, Session,
    SessionError, SessionManager, ToolCallRecord, Visibility,
};
