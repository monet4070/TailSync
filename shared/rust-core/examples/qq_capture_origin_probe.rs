//! Offline replay of an explicitly supplied local capture; prints no clipboard text.
use tailsync_core::{
    composite::{
        self, fingerprint,
        origin::{Origin, OriginTracker},
    },
    protocol::native_archive::NativeArchive,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("capture path required")?;
    let archive = NativeArchive::decode(&std::fs::read(path)?)?;
    let bundle = composite::from_archive(&archive).map_err(|e| format!("{e:?}"))?;
    let digest = fingerprint::origin_fingerprint_from_capture(&archive.original)?;
    let origin = OriginTracker::default().classify(Some(archive.original.after), digest, None);
    assert_eq!(origin, Origin::External);
    assert!(fingerprint::origin_fingerprint_from_native(&archive.original).is_err());
    println!("PASS: real capture converted, origin External, native replay rejected; items={}, formats={}, resources={}", archive.original.items, archive.original.formats.len(), bundle.resources.len());
    Ok(())
}
