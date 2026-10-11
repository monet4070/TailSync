//! R0 production-seam regressions for the composite-clipboard repair.
//!
//! Plan: `docs/COMPOSITE-CLIPBOARD-REPAIR-EXECUTION-PLAN-2026-10-04.md` §4 R0.2.
//! Ledger: `artifacts/composite-repair/2026-10-04-r0/expected-failures.json`.
//!
//! These assert the **desired** behaviour of the defects recorded in
//! `docs/COMPOSITE-CLIPBOARD-ACCEPTANCE-REVIEW-2026-10-04.md`. They are therefore
//! expected to FAIL until the owning stage lands; they are gated behind
//! `repair-regressions` rather than weakened or ignored, so the default gate stays
//! meaningful while the fix is in flight. Each ledger entry is removed as its stage
//! closes, and at R6 this suite joins the standard test entry.
//!
//! Run with (see plan §6):
//! `node scripts/run-isolated-tests.mjs -- cargo test --locked --offline \
//!    -p tailsync-core --features repair-regressions,test-support --test repair_regressions -- --test-threads=1`
#![cfg(feature = "repair-regressions")]

use tailsync_core::composite::origin::{Origin, OriginTracker};
use tailsync_core::composite::{packed_to_png, Completeness, Mime, PortableBundle, Representation};

// ---------------------------------------------------------------------------
// RR-05 / H-20 / T06 — origin fingerprint must not depend on representation order
// ---------------------------------------------------------------------------

/// The local fingerprint used for *origin* decisions.
///
/// R2-03 landed `composite::fingerprint::origin_fingerprint`, which is what
/// `shared/platform-clipboard-composite.rs` now records and classifies with on both
/// the publish and the read-back side. K1 keeps the *wire* digest order-sensitive on
/// purpose; only the origin fingerprint is a projection that survives OS read-back
/// normalisation.
fn origin_fingerprint(bundle: &PortableBundle) -> [u8; 32] {
    tailsync_core::composite::fingerprint::origin_fingerprint(bundle)
        .expect("fixture bundle must be a valid portable bundle")
}

fn two_representation_bundle(order: [Mime; 2]) -> PortableBundle {
    let text = b"<p>order-independent origin fixture</p>".to_vec();
    let png = packed_to_png(1, 1, &[1, 2, 3, 255]).expect("1x1 png");
    let representations = order
        .iter()
        .map(|mime| Representation {
            mime: *mime,
            bytes: match mime {
                Mime::Text => text.clone(),
                Mime::Png => png.clone(),
                other => panic!("fixture only builds Text/Png, got {other:?}"),
            },
        })
        .collect();
    PortableBundle {
        representations,
        resources: Vec::new(),
        bindings: Vec::new(),
        completeness: Completeness::Complete,
    }
}

/// H-20 was reproduced with the real packaged helper: a bundle published as
/// `[Png, Text]` is read back by the OS as `[Text, Png]`; because the recorded
/// fingerprint is order-sensitive the device classifies **its own write** as
/// `External` and re-broadcasts it (echo / duplicate history).
///
/// Desired: the origin fingerprint is a projection of the observable publish set,
/// so a permutation of the same content is still `SelfWrite`.
#[test]
fn rr05_origin_fingerprint_survives_representation_order_change() {
    let published = two_representation_bundle([Mime::Png, Mime::Text]);
    let read_back = two_representation_bundle([Mime::Text, Mime::Png]);

    let revision = 42;
    let token = [7_u8; 16];
    let mut tracker = OriginTracker::default();
    tracker.record(revision, origin_fingerprint(&published), token);

    let classified = tracker.classify(Some(revision), origin_fingerprint(&read_back), Some(token));
    assert_eq!(
        classified,
        Origin::SelfWrite,
        "an order-permuted read-back of our own publish must stay SelfWrite; \
         classifying it External re-broadcasts the device's own clipboard write (H-20)"
    );
}
