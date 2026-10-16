//! F8 shared document corpus: the real `serde` + `DisplayDocument::validate`
//! path must agree with `shared/schema/fixtures/document-corpus.json`.
//!
//! The corpus also drives the generated JavaScript/TypeScript decoder and the
//! Swift `JSONDecoder`. Small cases carry a literal `value`, raw-JSON parser
//! cases carry `json`, and large boundaries carry a compact `expand` descriptor
//! (an input builder only). `valid` is the shared expectation and `expect`
//! records a per-language difference.

use serde_json::{json, Value};
use std::path::PathBuf;
use tailsync_core::composite::display_document::{DisplayDocument, DocumentError};
use tailsync_core::composite::document_build::asset_digest;

fn corpus() -> Vec<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../shared/schema/fixtures/document-corpus.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    serde_json::from_str::<Value>(&text)
        .expect("document corpus is valid JSON")
        .as_array()
        .expect("document corpus is a JSON array")
        .clone()
}

fn run(text: &str, link: Option<&str>) -> Value {
    let mut value = json!({
        "text": text,
        "style": {
            "bold": false,
            "italic": false,
            "underline": false,
            "strikethrough": false,
            "subscript": false,
            "superscript": false,
        },
        "unsupported": false,
    });
    if let Some(link) = link {
        value["link"] = json!(link);
    }
    value
}

fn paragraph(runs: Vec<Value>) -> Value {
    json!({ "kind": "paragraph", "paragraph": { "runs": runs } })
}

fn document(blocks: Vec<Value>) -> Value {
    json!({
        "version": 1,
        "blocks": blocks,
        "assets": [],
        "partial": [],
        "styles_partial": false,
    })
}

/// Input builder for the compact boundary descriptors. It never validates.
fn expand(expand: &Value) -> Value {
    let count = |key: &str| expand[key].as_u64().expect(key) as usize;
    let groups = expand.get("groups").and_then(Value::as_u64).unwrap_or(1) as usize;
    let counts = || {
        (0..groups)
            .map(|index| count("count") / groups + usize::from(index < count("count") % groups))
    };
    match expand["kind"].as_str().expect("expand kind") {
        "paragraphs" => document(
            (0..count("count"))
                .map(|_| paragraph(vec![run("x", None)]))
                .collect(),
        ),
        "runs" => document(
            counts()
                .map(|count| paragraph((0..count).map(|_| run("x", None)).collect()))
                .collect(),
        ),
        "cells" => {
            let rows = counts()
                .map(|count| {
                    json!({ "cells": (0..count)
                .map(|_| json!({ "header": false, "runs": [run("x", None)] }))
                .collect::<Vec<_>>() })
                })
                .collect::<Vec<_>>();
            document(vec![json!({
                "kind": "table",
                "table": { "rows": rows, "truncated": false },
            })])
        }
        "text" => {
            let character = expand
                .get("character")
                .and_then(Value::as_str)
                .unwrap_or("x");
            let link = expand.get("linkBytes").map(|link| {
                format!(
                    "https://example.com/{}",
                    "a".repeat(link.as_u64().expect("linkBytes") as usize - 20)
                )
            });
            let runs = (0..groups)
                .map(|index| {
                    let bytes =
                        count("bytes") / groups + usize::from(index < count("bytes") % groups);
                    let text = character.repeat(bytes / character.len())
                        + &"x".repeat(bytes % character.len());
                    run(&text, if index == 0 { link.as_deref() } else { None })
                })
                .collect();
            document(vec![paragraph(runs)])
        }
        "assets" => {
            let total = count("count");
            let length = expand["length"].as_u64().expect("length");
            let width = expand["width"].as_u64().expect("width");
            let height = expand["height"].as_u64().expect("height");
            let blocks = (0..total)
                .map(|index| {
                    json!({
                        "kind": "image",
                        "image": { "asset": index, "availability": "available", "ordinal": index },
                    })
                })
                .collect::<Vec<_>>();
            let assets = (0..total)
                .map(|index| {
                    json!({
                        "index": index,
                        "offset": index as u64 * length,
                        "length": length,
                        "width": width,
                        "height": height,
                        "sha256": "0a".repeat(32),
                    })
                })
                .collect::<Vec<_>>();
            let mut value = document(blocks);
            value["assets"] = json!(assets);
            value["first_image"] = json!(0);
            value
        }
        "partials" => {
            let detail = "d".repeat(count("detailBytes"));
            let mut value = document(vec![paragraph(vec![run("x", None)])]);
            value["partial"] = json!((0..count("count"))
                .map(|_| json!({ "code": "other_native_items", "detail": detail }))
                .collect::<Vec<_>>());
            value
        }
        other => panic!("unknown expand kind {other}"),
    }
}

fn evaluate(case: &Value) -> bool {
    let document = match case.get("json") {
        Some(json) => serde_json::from_str::<DisplayDocument>(json.as_str().expect("json string")),
        None => {
            let value = case
                .get("value")
                .cloned()
                .unwrap_or_else(|| expand(case.get("expand").expect("value, json or expand")));
            serde_json::from_value::<DisplayDocument>(value)
        }
    };
    document.is_ok_and(|document| document.validate().is_ok())
}

fn expected(case: &Value, language: &str) -> bool {
    case.get("expect")
        .and_then(|expect| expect.get(language))
        .and_then(Value::as_bool)
        .unwrap_or_else(|| case["valid"].as_bool().expect("valid"))
}

#[test]
fn document_corpus_matches_the_rust_model() {
    let cases = corpus();
    assert!(
        cases.len() >= 100,
        "corpus unexpectedly small: {}",
        cases.len()
    );
    let names = cases
        .iter()
        .map(|case| case["name"].as_str().expect("case name"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(names.len(), cases.len(), "corpus names must be unique");
    let accepted = cases.iter().filter(|case| expected(case, "rust")).count();
    assert!(
        (30..=cases.len() - 30).contains(&accepted),
        "corpus expectation balance is implausible: {accepted} accepted"
    );
    let mismatches = cases
        .iter()
        .filter(|case| evaluate(case) != expected(case, "rust"))
        .map(|case| case["name"].as_str().unwrap_or("<unnamed>").to_string())
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "rust corpus mismatches: {mismatches:?}"
    );
}

#[test]
fn uppercase_metadata_digest_cannot_bypass_asset_payload_validation() {
    let mut png = Vec::from(&b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"[..]);
    png.extend_from_slice(&1u32.to_be_bytes());
    png.extend_from_slice(&1u32.to_be_bytes());
    let digest = asset_digest(&png);
    assert_eq!(digest, digest.to_lowercase());
    let mut document: DisplayDocument = serde_json::from_value(json!({
        "version": 1,
        "blocks": [{ "kind": "image", "image": { "asset": 0, "availability": "available", "ordinal": 0 } }],
        "assets": [{
            "index": 0,
            "offset": 0,
            "length": png.len(),
            "width": 1,
            "height": 1,
            "sha256": digest.to_uppercase(),
        }],
        "first_image": 0,
        "partial": [],
        "styles_partial": false,
    }))
    .expect("uppercase metadata digest is a valid document model");
    assert_eq!(document.validate(), Ok(()));
    assert_eq!(document.validate_assets(&png), Err(DocumentError::Invalid));
    document.assets[0].sha256 = digest;
    assert_eq!(document.validate_assets(&png), Ok(()));
}
