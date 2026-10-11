#![cfg(windows)]

use std::io::Write;
use std::process::{Command, Stdio};
use tailsync_protocol::native_clipboard::{NativeSnapshot, PublishReceipt, PublishState};

fn helper(argument: &str, input: Option<&[u8]>) -> Vec<u8> {
    let executable = std::env::var_os("TAILSYNC_TEST_HELPER_EXE")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_tailsync-clipboard-helper").into());
    let mut process = Command::new(executable)
        .arg(argument)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start helper");
    if let Some(input) = input {
        process.stdin.take().unwrap().write_all(input).unwrap();
    }
    let output = process.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "helper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn read() -> NativeSnapshot {
    NativeSnapshot::decode(&helper("--read-snapshot", None)).unwrap()
}

fn write(snapshot: &NativeSnapshot, token: [u8; 16]) -> PublishReceipt {
    let mut input = token.to_vec();
    input.extend_from_slice(&(-1_i64).to_be_bytes());
    input.extend_from_slice(&snapshot.encode().unwrap());
    PublishReceipt::decode(&helper("--write-snapshot", Some(&input))).unwrap()
}

struct RestoreClipboard {
    snapshot: NativeSnapshot,
    token: [u8; 16],
}

impl Drop for RestoreClipboard {
    fn drop(&mut self) {
        assert_eq!(
            write(&self.snapshot, self.token).state,
            PublishState::Published
        );
    }
}

/// Run only with TailSync stopped: this writes a fixture and restores the prior
/// supported clipboard formats, so no paired device receives the fixture.
#[test]
#[ignore = "writes the Windows clipboard; stop TailSync and run serially"]
fn publish_receipt_revision_matches_stable_read_after_helper_exit() {
    assert_eq!(std::env::var("TAILSYNC_TEST_CLIPBOARD").as_deref(), Ok("1"));
    let mut original = read();
    let original_token = original
        .formats
        .iter()
        .find(|format| format.name == "org.tailsync.clipboard-source")
        .and_then(|format| format.bytes.as_slice().try_into().ok())
        .unwrap_or([93; 16]);
    original
        .formats
        .retain(|format| format.name != "org.tailsync.clipboard-source");
    assert!(
        !original.formats.is_empty(),
        "no clipboard content to restore"
    );
    assert!(
        original.formats.iter().all(|format| matches!(
            format.name.as_str(),
            "CF_UNICODETEXT" | "HTML Format" | "Rich Text Format" | "PNG" | "CF_DIB" | "CF_DIBV5"
        )),
        "clipboard contains a format this helper cannot restore; do not modify it"
    );
    let _restore = RestoreClipboard {
        snapshot: original,
        token: original_token,
    };
    let mut previous_revision = 0;
    for text in [
        "receipt regression 中文\nline 2",
        "receipt regression 中文\nline 2",
        "different user copy 中文\nline 2",
    ] {
        let token = [47; 16];
        let snapshot = NativeSnapshot {
            before: 0,
            after: 0,
            items: 1,
            state: tailsync_protocol::native_clipboard::SnapshotState::Stable,
            formats: vec![tailsync_protocol::native_clipboard::NativeFormat {
                item: 0,
                name: "CF_UNICODETEXT".into(),
                state: tailsync_protocol::native_clipboard::FormatState::Captured,
                bytes: text
                    .encode_utf16()
                    .chain(Some(0))
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            }],
        };
        let receipt = write(&snapshot, token);
        assert_eq!(receipt.state, PublishState::Published);
        assert!(
            receipt.revision > previous_revision,
            "equal-content writes still create new revisions"
        );
        previous_revision = receipt.revision;
        let observed = read();
        assert_eq!(observed.before, observed.after);
        assert_eq!(receipt.revision, observed.after,
            "receipt sampled before clipboard ownership was released; a monitor would treat the write as external");
        assert_eq!(
            observed
                .formats
                .iter()
                .find(|format| format.name == "CF_UNICODETEXT")
                .unwrap()
                .bytes,
            snapshot.formats[0].bytes
        );
    }
}
