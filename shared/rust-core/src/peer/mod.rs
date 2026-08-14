//! Peer Directory and Peer Delivery domain.
//!
//! This module owns the cross-platform peer model: discovery candidates,
//! health status, delivery receipts, and resolved routes. Platform crates
//! re-export these types from their `network` modules so existing call sites
//! stay unchanged; state derivation and delivery policy land here in later
//! steps of the maintainability refactor.

pub mod delivery;
pub mod directory;
pub mod health;
pub mod types;
