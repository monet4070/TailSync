//! Verify a pre-upgrade rollback package and restore it into a new directory.
//!
//! This is the executable behind `scripts/restore-pre-v12-rollback.sh`; keeping the
//! logic here means the tested Rust path is the one operators actually run.
//!
//! Usage: restore_rollback <storage-dir> <target-dir>

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let storage = args
        .next()
        .ok_or("usage: restore_rollback <storage-dir> <target-dir>")?;
    let target = args
        .next()
        .ok_or("usage: restore_rollback <storage-dir> <target-dir>")?;
    let version = match args.next().as_deref() {
        None => 12,
        Some("--v13") => 13,
        _ => return Err("unsupported rollback option".into()),
    };
    if args.next().is_some() {
        return Err("unexpected rollback argument".into());
    }
    let storage = std::path::PathBuf::from(storage);
    let target = std::path::PathBuf::from(target);

    let manifest = if version == 13 {
        tailsync_core::db::verify_v13_rollback_package(&storage)?
    } else {
        tailsync_core::db::verify_rollback_package(&storage)?
    };
    let restored = if version == 13 {
        tailsync_core::db::restore_v13_rollback_package(&storage, &target)?
    } else {
        tailsync_core::db::restore_rollback_package(&storage, &target)?
    };

    println!(
        "{}",
        serde_json::json!({
            "action": format!("restore-pre-v{version}-rollback"),
            "package": storage.join(format!("pre-v{version}-rollback")).display().to_string(),
            "restored_into": target.display().to_string(),
            "source_schema_version": restored.source_schema_version,
            "payloads": manifest.payloads.len(),
            "total_bytes": manifest.total_bytes,
            "not_rolled_back": restored.excludes,
            "next_steps": [
                "verify the restored directory with the previous release binary before switching paths",
                "archive the post-upgrade composite history separately: an older binary cannot read it",
                "keep identity, trust and effect receipts in place; do not copy them from any snapshot",
                "release the package only once the rollback is no longer needed",
            ],
        })
    );
    Ok(())
}
