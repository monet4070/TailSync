// Run with the helper from the same checkout. Only a private named pasteboard
// is touched; the classification and format conversion are production Core.
use std::process::Command;
use tailsync_core::composite::{
    from_native,
    origin::{Origin, OriginTracker},
};
use tailsync_core::protocol::native_clipboard::{NativeSnapshot, PublishReceipt, PublishState};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let helper = std::env::args()
        .nth(1)
        .ok_or("usage: composite_origin_probe <helper> [iterations]")?;
    let iterations: usize = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "10000".into())
        .parse()?;
    if iterations == 0 || iterations > 10000 {
        return Err("iteration limit".into());
    }
    let name = format!(
        "tailsync-probe.origin-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    );
    let start = std::time::Instant::now();
    let output = Command::new(helper)
        .args([
            "--pasteboard",
            &name,
            "--probe-origin-loop",
            &iterations.to_string(),
        ])
        .output()?;
    if !output.status.success() || output.stdout.len() > 40 * 1024 * 1024 {
        return Err("native probe failed".into());
    }
    let mut bytes = output.stdout.as_slice();
    let mut tracker = OriginTracker::default();
    let mut self_writes = 0;
    let mut external = 0;
    let mut expected = None;
    while !bytes.is_empty() {
        if bytes.len() < 4 {
            return Err("truncated native probe".into());
        }
        let length = u32::from_be_bytes(bytes[..4].try_into()?) as usize;
        bytes = &bytes[4..];
        if length < 65 || length > bytes.len() {
            return Err("invalid native probe frame".into());
        }
        let frame = &bytes[..length];
        bytes = &bytes[length..];
        let receipt = PublishReceipt::decode(&frame[1..33])?;
        let snapshot = NativeSnapshot::decode(&frame[33..])?;
        if receipt.state != PublishState::Published
            || snapshot.before != snapshot.after
            || snapshot.after != receipt.revision
        {
            return Err("unstable native roundtrip".into());
        }
        let bundle = from_native(&snapshot).map_err(|_| "invalid native formats")?;
        if bundle.representations.len() != 3
            || !bundle.text().is_some_and(|s| s.contains("中文 🙂"))
        {
            return Err("native formats lost".into());
        }
        let digest = bundle.digest()?;
        if let Some(old) = expected {
            if old != digest {
                return Err("native image or format fingerprint drift".into());
            }
        } else {
            expected = Some(digest);
        }
        if frame[0] == 0 {
            tracker.record(receipt.revision, digest, receipt.token);
        }
        let token = snapshot
            .formats
            .iter()
            .find(|f| f.name == "org.tailsync.clipboard-source")
            .and_then(|f| f.bytes.as_slice().try_into().ok());
        let origin = tracker.classify(Some(snapshot.after), digest, token);
        match (frame[0], origin) {
            (0, Origin::SelfWrite) => self_writes += 1,
            (1, Origin::External) => external += 1,
            _ => return Err("origin causality regression".into()),
        }
    }
    if self_writes != iterations || external != iterations {
        return Err("missing native probe samples".into());
    }
    println!("{{\"self_writes\":{self_writes},\"unexpected_echoes\":0,\"equal_user_copies\":{external},\"elapsed_ms\":{},\"general_clipboard_modified\":false,\"target_app_acceptance\":\"UNKNOWN\"}}",start.elapsed().as_millis());
    Ok(())
}
