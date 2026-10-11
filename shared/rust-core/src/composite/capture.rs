//! Ownership handoff from completed history capture to an outgoing event.
use super::{budgets::PipelineLease, PortableBundle};
use crate::protocol::native_clipboard::NativeSnapshot;
use std::sync::Arc;

pub struct CapturedBroadcast {
    revision: i64,
    payload: Arc<[u8]>,
    _memory: PipelineLease,
}
impl CapturedBroadcast {
    pub fn revision(&self) -> i64 {
        self.revision
    }
    pub fn payload(&self) -> &Arc<[u8]> {
        &self.payload
    }
}

/// The history worker must have finished and released its admission clone.
/// Consuming the snapshot and projection ends ownership of capture scratch.
pub fn prepare_broadcast(
    snapshot: NativeSnapshot,
    bundle: PortableBundle,
    memory: Arc<PipelineLease>,
) -> Result<CapturedBroadcast, String> {
    let revision = snapshot.after;
    drop(snapshot);
    let mut memory = Arc::try_unwrap(memory)
        .map_err(|_| "Capture worker still owns broadcast admission".to_string())?;
    let payload = bundle.encode().map_err(|e| e.to_string())?;
    drop(bundle);
    let payload: Arc<[u8]> = Arc::from(payload.into_boxed_slice());
    // Capture scratch is gone. Keep the source buffer plus one envelope-copy
    // workspace admitted; SharedEvent separately admits its retained wire body.
    memory.shrink_to(
        (payload.len() as u64).saturating_mul(2)
            + crate::protocol::EVENT_ENVELOPE_HEADER_SIZE as u64,
    );
    Ok(CapturedBroadcast {
        revision,
        payload,
        _memory: memory,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{composite::*, peer::delivery::SharedEvent, protocol::Command};
    use rand::{RngCore, SeedableRng};

    #[test]
    fn review_large_capture_hands_off_scratch_before_shared_event_admission() {
        let ledger = budgets::global();
        let before = ledger.snapshot().pipeline_reserved;
        let memory = Arc::new(ledger.reserve_pipeline(240 * 1024 * 1024).unwrap());
        let mut rgba = vec![0; 2200 * 2100 * 4];
        rand::rngs::StdRng::seed_from_u64(17).fill_bytes(&mut rgba);
        let png = packed_to_png(2200, 2100, &rgba).unwrap();
        drop(rgba);
        assert!(png.len() > 16 * 1024 * 1024);
        let snapshot = NativeSnapshot {
            before: 7,
            after: 7,
            items: 1,
            state: crate::protocol::native_clipboard::SnapshotState::Stable,
            formats: vec![crate::protocol::native_clipboard::NativeFormat {
                item: 0,
                name: "public.png".into(),
                state: crate::protocol::native_clipboard::FormatState::Captured,
                bytes: png.clone(),
            }],
        };
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Png,
                bytes: png,
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        validate_for_publish(&bundle).unwrap();
        let digest = bundle.digest().unwrap();
        let captured = prepare_broadcast(snapshot, bundle, memory).unwrap();
        let event = SharedEvent::encode_shared(Command::CompositePayload, captured.payload.clone())
            .expect("valid large capture must leave admission for its outgoing envelope");
        assert_eq!(captured.revision, 7);
        assert_eq!(
            PortableBundle::decode(&captured.payload)
                .unwrap()
                .digest()
                .unwrap(),
            digest
        );
        assert!(ledger.snapshot().pipeline_reserved < before + budgets::PIPELINE_BUDGET_BYTES);
        drop(event);
        drop(captured);
        assert_eq!(ledger.snapshot().pipeline_reserved, before);
    }
}
