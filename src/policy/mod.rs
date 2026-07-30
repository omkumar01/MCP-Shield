//! Authorization policy layer.
//!
//! Embeds the Amazon Cedar policy engine for deterministic, sub-millisecond
//! Attribute-Based Access Control (ABAC).

pub mod cedar;

pub use cedar::{
    AuthorizationRequest, AuthorizationResponse, CedarAuthorizer, CedarError,
    CedarPolicyAuthorizer, Decision, StubAuthorizer,
};
