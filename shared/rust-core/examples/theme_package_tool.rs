//! Validate and package `.tailsync-theme` theme packages from a `theme.json`
//! manifest.  Runs the same validation the daemon applies on import
//! (`validate_theme_for_platform`) across both platforms, both appearance
//! modes, and both contrast settings, then optionally writes the installable
//! zip archive.
//!
//! Usage:
//!   cargo run --example theme_package_tool -- <theme.json> [output.tailsync-theme]

use std::{
    env, fs,
    io::{Cursor, Write},
    process::ExitCode,
};

use tailsync_core::themes_v2::validate_theme_for_platform;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args.len() > 3 {
        eprintln!("usage: theme_package_tool <theme.json> [output.tailsync-theme]");
        return ExitCode::from(2);
    }

    let manifest = match fs::read(&args[1]) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("failed to read {}: {error}", args[1]);
            return ExitCode::FAILURE;
        }
    };

    let cursor = Cursor::new(Vec::new());
    let mut archive = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default();
    if let Err(error) = archive.start_file("theme.json", options) {
        eprintln!("failed to start zip entry: {error}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = archive.write_all(&manifest) {
        eprintln!("failed to write zip entry: {error}");
        return ExitCode::FAILURE;
    }
    let package = match archive.finish() {
        Ok(cursor) => cursor.into_inner(),
        Err(error) => {
            eprintln!("failed to finish zip archive: {error}");
            return ExitCode::FAILURE;
        }
    };

    let mut failed = false;
    for platform in ["windows", "macos"] {
        for mode in ["light", "dark"] {
            for high_contrast in [false, true] {
                let validation =
                    validate_theme_for_platform(&package, mode, platform, high_contrast);
                let label = format!(
                    "{platform}/{mode}{}",
                    if high_contrast { "/high-contrast" } else { "" }
                );
                if !validation.valid {
                    failed = true;
                    for diagnostic in &validation.diagnostics {
                        eprintln!(
                            "[{label}] {} {} at {}",
                            diagnostic.severity, diagnostic.code, diagnostic.json_pointer
                        );
                        eprintln!("  {}", diagnostic.message);
                    }
                } else {
                    let warnings: Vec<_> = validation
                        .diagnostics
                        .iter()
                        .filter(|diagnostic| diagnostic.severity == "warning")
                        .collect();
                    if warnings.is_empty() {
                        println!(
                            "[{label}] ok (digest {})",
                            validation.digest.unwrap_or_default()
                        );
                    } else {
                        for warning in warnings {
                            println!(
                                "[{label}] warning {} at {}: {}",
                                warning.code, warning.json_pointer, warning.message
                            );
                        }
                    }
                }
            }
        }
    }

    if failed {
        eprintln!("{}: validation failed", args[1]);
        return ExitCode::FAILURE;
    }

    if let Some(output) = args.get(2) {
        if let Err(error) = fs::write(output, &package) {
            eprintln!("failed to write {output}: {error}");
            return ExitCode::FAILURE;
        }
        println!("wrote {output} ({} bytes)", package.len());
    }

    ExitCode::SUCCESS
}
