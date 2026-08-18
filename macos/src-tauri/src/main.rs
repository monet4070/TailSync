// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Propagate the daemon's exit code: a failed startup must be observable
    // by the caller (scripts, launchers, upgrade installers), not masked by
    // a normal zero exit.
    std::process::exit(tailsync_lib::run());
}
