//! Control plane for tenant and policy configuration.
//!
//! **Phase 4 — STUB.** Provides distributed configuration management
//! backed by PostgreSQL.

pub mod db;

pub use db::{
    ControlPlane, ControlPlaneError, PolicyEntry, PostgresControlPlane, StubControlPlane,
    Tenant, UpstreamEntry,
};
