use super::*;

fn decode_preview_response(response: &[u8]) -> (serde_json::Value, &[u8]) {
    assert_eq!(&response[..4], PREVIEW_RESPONSE_MAGIC);
    assert_eq!(response[4], PREVIEW_RESPONSE_VERSION);
    let metadata_len = u32::from_le_bytes(response[5..9].try_into().unwrap()) as usize;
    let payload_offset = 9 + metadata_len;
    let metadata = serde_json::from_slice(&response[9..payload_offset]).unwrap();
    (metadata, &response[payload_offset..])
}

#[test]
fn preview_response_keeps_text_metadata_and_raw_bytes() {
    let response = encode_preview_response(
        db::PreviewMetadata {
            entry_id: 17,
            kind: db::PreviewKind::Text,
            name: "text.txt".to_string(),
            size_bytes: 13,
            batch: None,
        },
        db::PreviewPayload {
            kind: "text".to_string(),
            name: "text.txt".to_string(),
            size_bytes: 13,
            data: b"preview bytes".to_vec(),
        },
    )
    .unwrap();
    let (metadata, data) = decode_preview_response(&response);

    assert_eq!(metadata["kind"], "text");
    assert_eq!(metadata["entry_id"], 17);
    assert_eq!(metadata["name"], "text.txt");
    assert_eq!(metadata["size_bytes"], 13);
    assert!(metadata["width"].is_null());
    assert!(metadata["height"].is_null());
    assert!(metadata["batch"].is_null());
    assert_eq!(data, b"preview bytes");
}

#[test]
fn preview_response_decodes_images_to_rgba_with_dimensions() {
    let rgba = [1, 2, 3, 4, 5, 6, 7, 8];
    let mut packed = Vec::new();
    packed.extend_from_slice(&2_u32.to_le_bytes());
    packed.extend_from_slice(&1_u32.to_le_bytes());
    packed.extend_from_slice(&rgba);
    let response = encode_preview_response(
        db::PreviewMetadata {
            entry_id: 23,
            kind: db::PreviewKind::Image,
            name: "image".to_string(),
            size_bytes: packed.len() as u64,
            batch: None,
        },
        db::PreviewPayload {
            kind: "image".to_string(),
            name: "image".to_string(),
            size_bytes: packed.len() as u64,
            data: packed,
        },
    )
    .unwrap();
    let (metadata, data) = decode_preview_response(&response);

    assert_eq!(metadata["kind"], "image");
    assert_eq!(metadata["size_bytes"], rgba.len());
    assert_eq!(metadata["width"], 2);
    assert_eq!(metadata["height"], 1);
    assert_eq!(data, rgba);
}

#[test]
fn shortcut_transaction_registers_new_then_persists() {
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let register = |next: &str| {
        calls.borrow_mut().push(format!("register:{next}"));
        Ok(())
    };
    let saved = std::cell::Cell::new(false);
    let save = || {
        saved.set(true);
        Ok(())
    };
    assert!(apply_shortcut_change("old", "new", register, save).is_ok());
    assert!(saved.get());
    assert_eq!(*calls.borrow(), vec!["register:new"]);
}

#[test]
fn shortcut_transaction_register_failure_restores_previous() {
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let register = |next: &str| {
        calls.borrow_mut().push(format!("register:{next}"));
        if next == "taken" {
            Err("shortcut is taken".to_string())
        } else {
            Ok(())
        }
    };
    let save = || panic!("save must not run after register failure");
    let error = apply_shortcut_change("old", "taken", register, save).unwrap_err();
    assert_eq!(error, "shortcut is taken");
    assert_eq!(*calls.borrow(), vec!["register:taken", "register:old"]);
}

#[test]
fn shortcut_transaction_save_failure_restores_previous() {
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let register = |next: &str| {
        calls.borrow_mut().push(format!("register:{next}"));
        Ok(())
    };
    let save = || Err("disk is full".to_string());
    let error = apply_shortcut_change("old", "new", register, save).unwrap_err();
    assert_eq!(error, "disk is full");
    assert_eq!(*calls.borrow(), vec!["register:new", "register:old"]);
}

#[test]
fn shortcut_transaction_restore_failure_mentions_both_errors() {
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let register = |next: &str| {
        calls.borrow_mut().push(format!("register:{next}"));
        if next == "new" {
            Ok(())
        } else {
            Err("old shortcut no longer available".to_string())
        }
    };
    let save = || Err("disk is full".to_string());
    let error = apply_shortcut_change("old", "new", register, save).unwrap_err();
    assert!(error.contains("disk is full"), "got: {error}");
    assert!(
        error.contains("old shortcut no longer available"),
        "got: {error}"
    );
    assert_eq!(*calls.borrow(), vec!["register:new", "register:old"]);
}

#[test]
fn shortcut_transaction_reregisters_unchanged_shortcut_without_saving() {
    let calls = std::cell::RefCell::new(Vec::<String>::new());
    let register = |next: &str| {
        calls.borrow_mut().push(format!("register:{next}"));
        Ok(())
    };
    let save = || {
        panic!("save must not run for an unchanged shortcut");
    };
    assert!(apply_shortcut_change("same", "same", register, save).is_ok());
    assert_eq!(*calls.borrow(), vec!["register:same"]);
}
