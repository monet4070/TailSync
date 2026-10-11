//! Synthetic named-board snapshot only; validates the real helper projection.
use tailsync_core::{
    composite::{from_native, Mime},
    protocol::native_clipboard::NativeSnapshot,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("snapshot path required")?;
    let snapshot = NativeSnapshot::decode(&std::fs::read(path)?)?;
    let bundle = from_native(&snapshot).map_err(|error| format!("capture: {error:?}"))?;
    assert_eq!(bundle.resources.len(), 1);
    assert_eq!(bundle.bindings.len(), 1);
    assert!(std::str::from_utf8(bundle.representation(Mime::Text).unwrap())?.contains("RTFD 中文"));
    assert!(snapshot
        .formats
        .iter()
        .any(|f| f.name == "com.apple.flat-rtfd"));
    bundle.validate()?;
    let helper = std::env::args().nth(2);
    if let Some(helper) = helper {
        use tailsync_core::composite::{
            fingerprint::origin_fingerprint_from_native,
            origin::{Origin, OriginTracker},
            supervisor::{self, HelperOutcome, HelperRequest},
        };
        use tailsync_core::protocol::native_clipboard::{
            PublishReceipt, PublishState, NATIVE_LIMIT,
        };
        let board = std::env::args().nth(3).ok_or("private board required")?;
        assert!(board.starts_with("tailsync-probe."));
        let target = origin_fingerprint_from_native(&snapshot)?;
        let token = [0x41; 16];
        let mut tracker = OriginTracker::default();
        tracker.record_intent(token, target);
        let mut publish = snapshot.clone();
        publish.formats.retain(|f| {
            !matches!(
                f.name.as_str(),
                "org.tailsync.clipboard-source" | "org.tailsync.rtfd-converted"
            )
        });
        let mut input = token.to_vec();
        input.extend_from_slice(&(-1i64).to_be_bytes());
        input.extend(publish.encode()?);
        let path = std::path::Path::new(&helper);
        let write = supervisor::run(HelperRequest {
            program: path,
            args: &["--pasteboard", &board, "--write-snapshot"],
            stdin: Some(input),
            deadline: std::time::Duration::from_secs(5),
            stdout_limit: NATIVE_LIMIT,
        });
        let HelperOutcome::Exited {
            success: true,
            stdout,
            ..
        } = write
        else {
            return Err(format!("write {write:?}").into());
        };
        let receipt = PublishReceipt::decode(&stdout)?;
        assert_eq!(receipt.state, PublishState::Published);
        tracker.record(receipt.revision, target, receipt.token);
        let read = supervisor::run(HelperRequest {
            program: path,
            args: &["--pasteboard", &board, "--read-snapshot"],
            stdin: None,
            deadline: std::time::Duration::from_secs(5),
            stdout_limit: NATIVE_LIMIT,
        });
        let HelperOutcome::Exited {
            success: true,
            stdout,
            ..
        } = read
        else {
            return Err(format!("read {read:?}").into());
        };
        let restored = NativeSnapshot::decode(&stdout)?;
        assert_eq!(origin_fingerprint_from_native(&restored)?, target);
        assert_eq!(
            tracker.classify(Some(restored.after), target, Some(token)),
            Origin::SelfWrite
        );
        assert_eq!(from_native(&restored).unwrap().resources.len(), 1);
        println!("RTFD native restore: Published receipt, stable readback and matching SelfWrite; original attachment retained");
    }
    println!(
        "RTFD: original native retained, text+HTML+1 embedded PNG validated; stable={}",
        snapshot.before == snapshot.after
    );
    Ok(())
}
