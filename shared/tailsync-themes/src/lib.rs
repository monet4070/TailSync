#![allow(clippy::result_large_err)]

//! Theme package V2.  This is deliberately data-only: package JSON is parsed
//! into tokens, never evaluated as CSS, Swift, JavaScript, or selectors. The
//! error stays unboxed to preserve its serialized cross-platform contract.
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
};
use zip::ZipArchive;

mod model;
mod package_io;

pub use model::{
    AssetMetadata, LocalThemeSettings, ModePair, PlatformTokens, ResolvedTheme,
    ThemeAssetDescriptor, ThemeDescriptor, ThemeError, ThemeManifest, ThemeValidation,
    UpdateThemeOptions,
};
pub use package_io::{read_theme_package_file, MAX_IMPORT_BYTES};

pub const FORMAT_VERSION: u32 = 2;
pub const CANVAS_ID: &str = "builtin:canvas@1";
pub const FLUX_ID: &str = "builtin:flux@1";
pub const LEDGER_ID: &str = "builtin:ledger@1";
pub const AURA_ID: &str = "builtin:aura@1";
pub const MONO_ID: &str = "builtin:mono@1";
pub const MAX_COMPRESSED_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_UNCOMPRESSED_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_ASSET_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_ASSETS: usize = 64;
pub const MAX_DECODED_PIXELS: u64 = 80_000_000;
const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
// These capabilities describe behavior which this resolver implements today.
// Keep this deliberately small: a package must fail closed when it requires a
// capability that this Core cannot prove it supports.
const SUPPORTED_CAPABILITIES: &[&str] = &["theme-v2", "high-contrast", "platform-overrides"];

fn err<T>(code: &str, msg: impl Into<String>, p: impl Into<String>) -> Result<T, ThemeError> {
    Err(ThemeError::new(code, msg, p))
}
fn valid_id(id: &str) -> bool {
    if id.starts_with("builtin:") {
        return builtin_ids().contains(&id);
    }
    let Some(rest) = id.strip_prefix("custom:") else {
        return false;
    };
    let mut s = rest.split('.');
    matches!((s.next(),s.next(),s.next()),(Some(a),Some(b),None) if !a.is_empty()&&!b.is_empty()&&a.len()<=32&&b.len()<=32&&a.bytes().all(|c|c.is_ascii_lowercase()||c.is_ascii_digit()||c==b'-')&&b.bytes().all(|c|c.is_ascii_lowercase()||c.is_ascii_digit()||c==b'-'))
}

fn valid_custom_id(id: &str) -> bool {
    id.starts_with("custom:") && valid_id(id)
}
#[derive(Debug, Clone, Eq, PartialEq)]
struct Semver {
    major: u32,
    minor: u32,
    patch: u32,
    prerelease: Option<Vec<String>>,
}

fn semver_identifier(value: &str, numeric: bool) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && (!numeric || value.len() == 1 || !value.starts_with('0'))
}

fn semver(v: &str) -> Option<Semver> {
    let (without_build, build) = match v.split_once('+') {
        Some((version, build)) => (version, Some(build)),
        None => (v, None),
    };
    if build.is_some_and(|build| {
        build
            .split('.')
            .any(|identifier| !semver_identifier(identifier, false))
    }) {
        return None;
    }
    let (core, prerelease) = match without_build.split_once('-') {
        Some((core, prerelease)) => (core, Some(prerelease)),
        None => (without_build, None),
    };
    let mut numbers = core.split('.');
    let parse_number = |number: &str| {
        (number == "0" || (!number.starts_with('0') && !number.is_empty()))
            .then(|| number.parse::<u32>().ok())
            .flatten()
    };
    let version = Semver {
        major: parse_number(numbers.next()?)?,
        minor: parse_number(numbers.next()?)?,
        patch: parse_number(numbers.next()?)?,
        prerelease: prerelease.map(|value| value.split('.').map(str::to_owned).collect()),
    };
    if numbers.next().is_some()
        || version.prerelease.as_ref().is_some_and(|identifiers| {
            identifiers.iter().any(|identifier| {
                !semver_identifier(identifier, identifier.bytes().all(|b| b.is_ascii_digit()))
            })
        })
    {
        None
    } else {
        Some(version)
    }
}

fn compare_semver(left: &Semver, right: &Semver) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    for (left, right) in [
        (left.major, right.major),
        (left.minor, right.minor),
        (left.patch, right.patch),
    ] {
        match left.cmp(&right) {
            Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    match (&left.prerelease, &right.prerelease) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(left), Some(right)) => {
            for (left, right) in left.iter().zip(right) {
                let left_numeric = left.bytes().all(|byte| byte.is_ascii_digit());
                let right_numeric = right.bytes().all(|byte| byte.is_ascii_digit());
                let ordering = match (left_numeric, right_numeric) {
                    // Numeric prerelease identifiers can exceed Rust's
                    // integer range. SemVer forbids leading zeroes, so their
                    // length and lexical order are their numeric order.
                    (true, true) => left.len().cmp(&right.len()).then_with(|| left.cmp(right)),
                    (true, false) => Ordering::Less,
                    (false, true) => Ordering::Greater,
                    (false, false) => left.cmp(right),
                };
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            left.len().cmp(&right.len())
        }
    }
}
fn package_digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
fn safe_zip_path(name: &str) -> bool {
    let p = Path::new(name);
    !p.is_absolute()
        && !p.components().any(|x| {
            matches!(
                x,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}
fn image_size(bytes: &[u8], mime: &str) -> Option<(u32, u32)> {
    if mime == "image/png" && bytes.len() >= 24 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" {
        Some((
            u32::from_be_bytes(bytes[16..20].try_into().ok()?),
            u32::from_be_bytes(bytes[20..24].try_into().ok()?),
        ))
    } else if mime == "image/jpeg" {
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xff {
                i += 1;
                continue;
            }
            let m = bytes[i + 1];
            if (0xc0..=0xc3).contains(&m)
                || (0xc5..=0xc7).contains(&m)
                || (0xc9..=0xcb).contains(&m)
                || (0xcd..=0xcf).contains(&m)
            {
                return Some((
                    u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32,
                    u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32,
                ));
            }
            if i + 4 > bytes.len() {
                break;
            }
            i += 2 + u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        }
        None
    } else {
        None
    }
}

mod builtins;
use builtins::*;
mod resolution;
#[cfg(test)]
use resolution::color_contrast;
use resolution::{
    accessibility_diagnostics, enforce_high_contrast, mark_provenance, parse_rgb, resolve_colors,
    resolve_manifest, resolve_manifest_unenforced,
};
mod validation;
use validation::{
    object, read_package, resolved_asset_slots, validate_all_resolved_modes, validate_manifest,
    COMPONENT_NAMES,
};
pub use validation::{validate_theme, validate_theme_for_platform};

/// Non-blocking WCAG diagnostics for package authors. Resolver output remains
/// usable; clients can present these warnings before installation/activation.
mod store;

pub use store::{
    delete_theme_at, delete_theme_by_handle_at, get_local_theme_settings_at, get_theme_asset_at,
    get_theme_asset_slot_at, get_theme_asset_slot_from_package, install_theme_at,
    list_themes_v2_at, migrate_legacy_theme_selection_at, resolve_theme_at, rollback_theme_at,
    set_local_theme_settings_at, update_theme_at, LegacyThemeMigration,
    LEGACY_BUILTIN_THEME_MAPPING,
};

#[cfg(test)]
use store::{delete_theme_by_handle_with_remover_at, id_path, recover_swap, root};
#[cfg(test)]
mod tests;
