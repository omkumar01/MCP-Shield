//! Control plane for tenant and policy configuration.
//!
//! **Phase 4 — PRODUCTION.** Provides distributed configuration management
//! backed by PostgreSQL (feature-gated) with an in-memory fallback.

pub mod db;

pub use db::{
    ControlPlane, ControlPlaneError, InMemoryControlPlane, PolicyEntry, RateLimitRule, Tenant,
    UpstreamEntry, create_control_plane,
};

#[cfg(feature = "postgres")]
pub use db::PostgresControlPlane;
