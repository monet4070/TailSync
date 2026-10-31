//! Offline replay of an explicitly supplied WeChat/Qt image capture; no general clipboard writes.
use tailsync_core::{
    composite::{
        self, fingerprint,
        origin::{Origin, OriginTracker},
        Mime,
    },
    protocol::native_archive::NativeArchive,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("capture path required")?;
    let archive = NativeArchive::decode(&std::fs::read(path)?)?;
    let bundle = composite::from_archive(&archive).map_err(|e| format!("{e:?}"))?;
    let png = bundle.representation(Mime::Png).ok_or("PNG missing")?;
    let packed = composite::png_to_packed(png)?;
    let digest = fingerprint::origin_fingerprint_from_capture(&archive.original)?;
    assert_eq!(
        OriginTracker::default().classify(Some(archive.original.after), digest, None),
        Origin::External
    );
    assert!(fingerprint::origin_fingerprint_from_native(&archive.original).is_err());
    let published = composite::to_macos_snapshot(&bundle)?;
    let own = fingerprint::origin_fingerprint_from_native(&published)?;
    let mut tracker = OriginTracker::default();
    tracker.record(1, own, [1; 16]);
    assert_eq!(
        tracker.classify(
            Some(1),
            fingerprint::origin_fingerprint_from_capture(&published)?,
            Some([1; 16])
        ),
        Origin::SelfWrite
    );
    println!("PASS: actual WeChat image converted and decoded; External origin; native replay rejected; own write suppressed; items={}, formats={}, PNG bytes={}, decoded bytes={}",archive.original.items,archive.original.formats.len(),png.len(),packed.len());
    Ok(())
}
