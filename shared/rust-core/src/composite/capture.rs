//! Ownership handoff from completed history capture to an outgoing event.
use super::{budgets::PipelineLease, PortableBundle};
use crate::protocol::native_clipboard::NativeSnapshot;
use std::sync::Arc;

pub struct CapturedBroadcast {
    revision: i64,
    payload: Arc<[u8]>,
    _memory: Arc<PipelineLease>,
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
    let payload = bundle.encode().map_err(|e| e.to_string())?;
    drop(bundle);
    let payload: Arc<[u8]> = Arc::from(payload.into_boxed_slice());
    // Capture scratch is gone. Keep the source buffer plus one envelope-copy
    // workspace admitted; SharedEvent separately admits its retained wire body.
    let memory = match Arc::try_unwrap(memory) {
        Ok(mut memory) => {
            memory.shrink_to(
                (payload.len() as u64).saturating_mul(2)
                    + crate::protocol::EVENT_ENVELOPE_HEADER_SIZE as u64,
            );
            Arc::new(memory)
        }
        // A cancelled image decoder may still own scratch. Keep the full lease
        // until it exits instead of dropping the copy or undercounting memory.
        Err(memory) => memory,
    };
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
    fn cancelled_image_worker_keeps_its_full_lease_without_dropping_the_fallback_event() {
        let ledger = Arc::new(budgets::BudgetLedger::default());
        let memory = Arc::new(ledger.reserve_pipeline(240 * 1024 * 1024).unwrap());
        let worker_memory = memory.clone();
        let snapshot = NativeSnapshot {
            before: 3,
            after: 3,
            items: 1,
            state: crate::protocol::native_clipboard::SnapshotState::Stable,
            formats: vec![],
        };
        let bundle = PortableBundle {
            representations: vec![Representation {
                mime: Mime::Text,
                bytes: b"saved body".to_vec(),
            }],
            resources: vec![],
            bindings: vec![],
            completeness: Completeness::Complete,
        };
        let captured = prepare_broadcast(snapshot, bundle, memory).unwrap();
        assert_eq!(ledger.snapshot().pipeline_reserved, 240 * 1024 * 1024);
        assert_eq!(
            PortableBundle::decode(captured.payload()).unwrap().text(),
            Some("saved body")
        );
        drop(captured);
        assert_eq!(ledger.snapshot().pipeline_reserved, 240 * 1024 * 1024);
        drop(worker_memory);
        assert_eq!(ledger.snapshot().pipeline_reserved, 0);
    }

    #[test]
    fn review_large_capture_hands_off_scratch_before_shared_event_admission() {
        let _pipeline = crate::composite::budgets::pipeline_budget_test_lock();
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
