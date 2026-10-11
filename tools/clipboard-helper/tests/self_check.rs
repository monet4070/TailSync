#![cfg(windows)]

use std::process::{Command, Stdio};

#[test]
fn startup_probe_succeeds_without_clipboard_input() {
    let output = Command::new(env!("CARGO_BIN_EXE_tailsync-clipboard-helper"))
        .arg("--self-check")
        .stdin(Stdio::null())
        .output()
        .expect("launch the real Windows helper");

    assert!(output.status.success(), "helper rejected its startup probe");
    assert!(output.stderr.is_empty());
    let announcement = String::from_utf8(output.stdout).unwrap();
    let fields: Vec<_> = announcement.split_whitespace().collect();
    assert_eq!(fields.first(), Some(&"CWH1"));
    assert!(fields.contains(&"protocol=2"));
    assert!(fields.contains(&"analyze=1"));
}
