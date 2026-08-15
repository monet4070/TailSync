//! Peer Directory and Peer Delivery domain.
//!
//! This module owns the cross-platform peer model: discovery candidates,
//! health status, delivery receipts, and resolved routes. Platform crates
//! re-export these types from their `network` modules so existing call sites
//! stay unchanged; state derivation and delivery policy land here in later
//! steps of the maintainability refactor.

pub mod admission;
pub mod connection_limiter;
pub mod delivery;
pub mod directory;
pub mod event_receiver;
pub mod health;
pub mod inbound_source;
pub mod rate_limit;
pub mod types;
