//! Deterministic, cross-machine content fingerprint.
//!
//! The fingerprint is a versioned, length-prefixed, order-sensitive encoding of
//! the standard representations, image resources, and their bindings. It
//! deliberately excludes the event ID, timestamps, diagnostics, native OS
//! revisions, and self-write source markers, so the same semantic clipboard
//! content fingerprints identically on both peers.
//!
//! Each section starts with its element count and each element is framed by its
//! type byte and byte length, so no two different structures can serialize to
//! the same byte stream (the "type, length, and order boundaries" required by
//! the plan). [`CONTENT_HASH_VERSION`] is part of the hashed input, so a future
//! encoding change produces a different domain.

use blake3::Hasher;

use super::PortableBundle;

/// Version of the fingerprint encoding itself.
pub const CONTENT_HASH_VERSION: u8 = 1;

/// Domain separator so the fingerprint can never collide with another BLAKE3
/// use in the codebase.
const CONTENT_HASH_DOMAIN: &[u8] = b"tailsync.composite.content";

/// Compute the stable content fingerprint of a bundle.
pub fn content_hash(bundle: &PortableBundle) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(CONTENT_HASH_DOMAIN);
    hasher.update(&[CONTENT_HASH_VERSION]);
    hasher.update(&bundle.version.to_be_bytes());

    hasher.update(&(bundle.representations.len() as u64).to_be_bytes());
    for representation in &bundle.representations {
        hasher.update(&[representation.mime.as_u8()]);
        hasher.update(&(representation.bytes.len() as u64).to_be_bytes());
        hasher.update(&representation.bytes);
    }

    hasher.update(&(bundle.resources.len() as u64).to_be_bytes());
    for resource in &bundle.resources {
        hasher.update(&[resource.mime.as_u8()]);
        hasher.update(&(resource.bytes.len() as u64).to_be_bytes());
        hasher.update(&resource.bytes);
    }

    hasher.update(&(bundle.bindings.len() as u64).to_be_bytes());
    for binding in &bundle.bindings {
        hasher.update(&binding.representation.to_be_bytes());
        hasher.update(&binding.resource.to_be_bytes());
        hasher.update(&(binding.target.len() as u64).to_be_bytes());
        hasher.update(binding.target.as_bytes());
    }

    *hasher.finalize().as_bytes()
}
