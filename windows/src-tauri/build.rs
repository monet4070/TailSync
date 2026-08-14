fn main() {
    println!("cargo:rerun-if-env-changed=TAILSYNC_PUBLISHED_RELEASE");
    println!("cargo:rerun-if-env-changed=TAILSYNC_UPDATER_PUBLIC_KEY");
    if std::env::var("TAILSYNC_PUBLISHED_RELEASE").as_deref() == Ok("1")
        && std::env::var("TAILSYNC_UPDATER_PUBLIC_KEY")
            .map(|value| value.trim().is_empty())
            .unwrap_or(true)
    {
        panic!("TAILSYNC_UPDATER_PUBLIC_KEY is required for a published release");
    }
    #[cfg(target_os = "windows")]
    println!(
        "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
    );
    tauri_build::build()
}
