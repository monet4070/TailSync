//! Composite clipboard rules, conversion results, stable fingerprint, and the
//! supported portable format scope.
//!
//! The wire-level value types and the bounded binary codec live in
//! [`tailsync_protocol::composite`]; this module re-exports them so the shared
//! Core exposes one canonical `PortableBundle` contract, and adds the pieces
//! the implementation plan assigns to Core:
//!
//! * the deterministic, cross-machine content fingerprint
//!   ([`fingerprint::content_hash`]);
//! * the supported format scope and native-format mapping
//!   ([`support`]);
//! * the native→portable conversion result ([`ConversionResult`]).
//!
//! Platform capture/playback and any network sending stay outside this module
//! until the later milestones; M1 only freezes the contract and pure rules.

pub mod fingerprint;
pub mod support;

pub use fingerprint::{content_hash, CONTENT_HASH_VERSION};
pub use support::{portable_mime_for_native, support_level, SupportLevel};

pub use tailsync_protocol::composite::{
    Completeness, PortableBundle, PortableMime, Representation, Resource, ResourceBinding,
    COMPOSITE_BUNDLE_VERSION,
};

/// Result of converting one captured native snapshot into a portable bundle.
///
/// A `Partial` result keeps the usable bundle and records which native formats
/// were dropped; it is never treated as a complete cross-platform success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionResult {
    /// Every captured format mapped to a supported portable representation.
    Complete(PortableBundle),
    /// The bundle is usable, but some captured formats were dropped.
    Partial {
        bundle: PortableBundle,
        dropped: Vec<String>,
    },
    /// No portable representation could be produced.
    Unsupported { reason: String },
}

impl ConversionResult {
    /// The portable bundle, if any was produced.
    pub fn bundle(&self) -> Option<&PortableBundle> {
        match self {
            Self::Complete(bundle) => Some(bundle),
            Self::Partial { bundle, .. } => Some(bundle),
            Self::Unsupported { .. } => None,
        }
    }

    /// Whether every captured format was carried over.
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete(_))
    }

    /// Native format names that could not be represented.
    pub fn dropped(&self) -> &[String] {
        match self {
            Self::Partial { dropped, .. } => dropped,
            _ => &[],
        }
    }
}

#[cfg(test)]
mod tests;
