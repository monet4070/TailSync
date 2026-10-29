//! R3 native-outcome probe: the production sequence against a real helper.
//!
//! It runs exactly the order the platform uses — register the intent **before** the
//! helper starts, run it under one deadline via the shared supervisor, and on a lost
//! receipt establish the resolution revision after the helper has been terminated and
//! reaped — and then asks the production origin tracker what the read-back means.
//!
//! The point is the contract, not the timing:
//!
//! * a stalled helper must produce [`HelperOutcome::TimedOut`], never `SpawnFailed`,
//!   because "we do not know" is not "nothing happened" (K2);
//! * after the stall the device must classify its **own** write as `UnknownMismatch`, not
//!   `External`, or it would re-broadcast it (K4/H-20);
//! * when the clipboard was cleared and left empty the outcome is still `TimedOut`,
//!   and the platform must not report a clean no-effect.
//!
//! Only a `tailsync-probe.*` named pasteboard is touched.
//!
//! Usage: native_outcome_probe <helper-path> <board-name>

use std::path::PathBuf;
use std::time::Duration;

use tailsync_core::composite::fingerprint::origin_fingerprint_from_native;
use tailsync_core::composite::origin::{Origin, OriginTracker};
use tailsync_core::composite::supervisor::{self, HelperOutcome, HelperRequest};
use tailsync_core::protocol::native_clipboard::{
    FormatState, NativeFormat, NativeSnapshot, SnapshotState, NATIVE_LIMIT,
};

fn read_snapshot(helper: &PathBuf, board: &str) -> Result<NativeSnapshot, String> {
    let outcome = supervisor::run(HelperRequest {
        program: helper,
        args: &["--pasteboard", board, "--read-snapshot"],
        stdin: None,
        deadline: Duration::from_secs(5),
        stdout_limit: NATIVE_LIMIT,
    });
    match outcome {
        HelperOutcome::Exited {
            success: true,
            stdout,
            ..
        } => NativeSnapshot::decode(&stdout).map_err(|error| error.to_string()),
        other => Err(format!("read failed: {other:?}")),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let helper = PathBuf::from(
        args.next()
            .ok_or("usage: native_outcome_probe <helper> <board>")?,
    );
    let board = args
        .next()
        .ok_or("usage: native_outcome_probe <helper> <board>")?;

    // The payload we intend to publish: text plus a small PNG.
    let text = b"r3 outcome probe text".to_vec();
    let png = one_pixel_png();
    let intended = NativeSnapshot {
        before: 0,
        after: 0,
        items: 1,
        state: SnapshotState::Stable,
        formats: vec![
            NativeFormat {
                item: 0,
                name: "public.utf8-plain-text".into(),
                state: FormatState::Captured,
                bytes: text.clone(),
            },
            NativeFormat {
                item: 0,
                name: "public.png".into(),
                state: FormatState::Captured,
                bytes: png.clone(),
            },
        ],
    };
    let target = origin_fingerprint_from_native(&intended)?;

    let token: [u8; 16] = [0x3c; 16];
    let mut tracker = OriginTracker::default();
    // R3-03: the intent goes in BEFORE the helper can touch anything.
    let registered = tracker.record_intent(token, target);

    let mut input = token.to_vec();
    input.extend_from_slice(&(-1_i64).to_be_bytes());
    input.extend_from_slice(&intended.encode()?);

    let ledger = tailsync_core::composite::budgets::global();
    let budget_before = ledger.snapshot().pipeline_reserved;
    let lease = ledger.reserve_pipeline(input.len() as u64)?;
    let spawned = std::cell::Cell::new(None);
    let on_spawn = |pid| {
        spawned.set(Some(pid));
        Ok(())
    };
    // A deliberately short deadline so the injected stall is what the probe observes.
    let outcome = supervisor::run_registered(
        HelperRequest {
            program: &helper,
            args: &["--pasteboard", &board, "--write-snapshot"],
            stdin: Some(input),
            deadline: Duration::from_millis(1000),
            stdout_limit: 4096,
        },
        None,
        Some(&on_spawn),
    );
    drop(lease);
    let budget_after = ledger.snapshot().pipeline_reserved;
    let child_reaped = spawned.get().map(|pid: u32| {
        !std::process::Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .output()
            .is_ok_and(|result| result.status.success())
    });

    // Establish the resolution boundary exactly as the platform does: the helper has
    // been terminated and reaped, so the current revision is now attributable.
    let after_termination = read_snapshot(&helper, &board);
    let resolution_revision = after_termination
        .as_ref()
        .ok()
        .filter(|snapshot| snapshot.before == snapshot.after)
        .map(|snapshot| snapshot.after);
    if let Some(revision) = resolution_revision {
        tracker.resolve_intent(token, revision);
    }

    let observed = after_termination
        .as_ref()
        .ok()
        .and_then(|snapshot| origin_fingerprint_from_native(snapshot).ok());
    let classification = observed.map(|digest| {
        tracker.classify(
            resolution_revision,
            digest,
            after_termination.as_ref().ok().and_then(|snapshot| {
                snapshot
                    .formats
                    .iter()
                    .find(|format| format.name == "org.tailsync.clipboard-source")
                    .and_then(|format| format.bytes.as_slice().try_into().ok())
            }),
        )
    });

    let (outcome_kind, timed_out, spawn_failed) = match &outcome {
        HelperOutcome::Exited { .. } => ("exited", false, false),
        HelperOutcome::TimedOut { .. } => ("timed_out", true, false),
        HelperOutcome::OutputExceeded { .. } => ("output_exceeded", false, false),
        HelperOutcome::SpawnFailed { .. } => ("spawn_failed", false, true),
    };
    let (exit_success, stdout_bytes, stderr_bytes, elapsed) = match &outcome {
        HelperOutcome::Exited {
            success,
            stdout,
            stderr_bytes,
            elapsed,
        } => (
            Some(*success),
            Some(stdout.len()),
            Some(*stderr_bytes),
            *elapsed,
        ),
        HelperOutcome::TimedOut {
            stderr_bytes,
            elapsed,
        }
        | HelperOutcome::OutputExceeded {
            stderr_bytes,
            elapsed,
        } => (None, None, Some(*stderr_bytes), *elapsed),
        HelperOutcome::SpawnFailed { elapsed, .. } => (None, None, None, *elapsed),
    };

    println!(
        "{}",
        serde_json::json!({
            "probe": "r3-native-outcome",
            "native_attempts": usize::from(spawned.get().is_some()),
            "child_reaped": child_reaped,
            "history_count": serde_json::Value::Null,
            "history_scope": "supervisor/origin seam; receiver history measured separately",
            "budget_before": budget_before,
            "budget_after": budget_after,
            "budget_released": budget_before == budget_after,
            "board": board,
            "intent_registered": registered,
            "outcome": outcome_kind,
            "helper_exit_success": exit_success,
            "helper_stdout_bytes": stdout_bytes,
            "helper_stderr_bytes": stderr_bytes,
            "helper_elapsed_ms": elapsed.as_millis(),
            "timed_out": timed_out,
            "spawn_failed": spawn_failed,
            "may_have_had_an_effect": !spawn_failed,
            "resolution_revision": resolution_revision,
            "clipboard_state": after_termination.as_ref().ok().map(|s| s.state as u8),
            "clipboard_formats": after_termination.as_ref().ok().map(|s| s.formats.len()),
            "clipboard_empty": after_termination
                .as_ref()
                .ok()
                .map(|s| s.state == SnapshotState::Empty || s.items == 0),
            "classification": classification.map(|origin| match origin {
                Origin::SelfWrite => "self_write",
                Origin::External => "external",
                Origin::UnknownMismatch => "unknown_mismatch",
                Origin::Unknown => "unknown",
            }),
            "intended_fingerprint_matches_readback": observed == Some(target),
        })
    );
    Ok(())
}

/// Minimal valid 1x1 RGBA PNG (stored deflate), so the probe needs no image crate.
fn one_pixel_png() -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffff_u32;
        for byte in data {
            crc ^= *byte as u32;
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        !crc
    }
    fn adler32(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1_u32, 0_u32);
        for byte in data {
            a = (a + *byte as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }
    fn chunk(bytes: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
        bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
        bytes.extend_from_slice(kind);
        bytes.extend_from_slice(body);
        let mut crc_input = kind.to_vec();
        crc_input.extend_from_slice(body);
        bytes.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    }
    chunk(
        &mut bytes,
        b"IHDR",
        &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0],
    );
    let raw = [0_u8, 10, 20, 30, 255];
    let mut idat = vec![0x78, 0x01, 1];
    idat.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    idat.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    idat.extend_from_slice(&raw);
    idat.extend_from_slice(&adler32(&raw).to_be_bytes());
    chunk(&mut bytes, b"IDAT", &idat);
    chunk(&mut bytes, b"IEND", &[]);
    bytes
}
