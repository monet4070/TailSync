#![allow(clippy::result_large_err)]

//! Theme package V2.  This is deliberately data-only: package JSON is parsed
//! into tokens, never evaluated as CSS, Swift, JavaScript, or selectors. The
//! error stays unboxed to preserve its serialized cross-platform contract.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
};
use zip::ZipArchive;

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
const CORE_VERSION: &str = "2.1.0";
// These capabilities describe behavior which this resolver implements today.
// Keep this deliberately small: a package must fail closed when it requires a
// capability that this Core cannot prove it supports.
const SUPPORTED_CAPABILITIES: &[&str] = &["theme-v2", "high-contrast", "platform-overrides"];

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ThemeError {
    pub code: String,
    pub message: String,
    pub json_pointer: String,
    pub platforms: Vec<String>,
    pub severity: String,
    pub recoverable: bool,
    pub fallback_applied: bool,
}
impl ThemeError {
    fn new(code: &str, message: impl Into<String>, pointer: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            json_pointer: pointer.into(),
            platforms: vec!["windows".into(), "macos".into()],
            severity: "error".into(),
            recoverable: true,
            fallback_applied: false,
        }
    }

    fn warning(code: &str, message: impl Into<String>, pointer: impl Into<String>) -> Self {
        let mut diagnostic = Self::new(code, message, pointer);
        diagnostic.severity = "warning".into();
        diagnostic
    }
}
impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for ThemeError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeManifest {
    pub format_version: u32,
    pub id: String,
    pub version: String,
    pub min_core_version: String,
    pub name: BTreeMap<String, String>,
    pub extends: String,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub signature: Option<Value>,
    pub light: Value,
    pub dark: Value,
    #[serde(default)]
    pub high_contrast: Option<ModePair>,
    #[serde(default = "empty_object")]
    pub foundation: Value,
    #[serde(default = "empty_object")]
    pub components: Value,
    #[serde(default)]
    pub platform: PlatformTokens,
    #[serde(default)]
    pub asset_slots: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModePair {
    pub light: Value,
    pub dark: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlatformTokens {
    #[serde(default = "empty_object")]
    pub windows: Value,
    #[serde(default = "empty_object")]
    pub macos: Value,
}
impl Default for PlatformTokens {
    fn default() -> Self {
        Self {
            windows: empty_object(),
            macos: empty_object(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetMetadata {
    pub key: String,
    pub digest: String,
    pub mime_type: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeAssetDescriptor {
    pub slot: String,
    pub key: String,
    pub digest: String,
    pub mime_type: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTheme {
    pub id: String,
    pub digest: String,
    pub mode: String,
    pub high_contrast: bool,
    pub tokens: Value,
    pub provenance: BTreeMap<String, String>,
    pub assets: Vec<AssetMetadata>,
    pub asset_slots: BTreeMap<String, ThemeAssetDescriptor>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeDescriptor {
    pub id: String,
    /// Opaque, storage-scoped handle used for destructive operations.  This
    /// remains usable even when a damaged package no longer has a valid id.
    pub storage_handle: String,
    pub source: String,
    pub format_version: u32,
    pub version: String,
    pub digest: String,
    pub name: BTreeMap<String, String>,
    pub resolved: Option<ResolvedTheme>,
    pub resolved_light: Option<ResolvedTheme>,
    pub resolved_dark: Option<ResolvedTheme>,
    pub diagnostics: Vec<ThemeError>,
    pub status: String,
    pub platforms: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateThemeOptions {
    #[serde(default)]
    pub allow_same_version: bool,
    #[serde(default)]
    pub allow_downgrade: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeValidation {
    pub valid: bool,
    pub digest: Option<String>,
    pub candidate_version: Option<String>,
    pub preview: Option<ResolvedTheme>,
    pub diagnostics: Vec<ThemeError>,
    pub assets: Vec<AssetMetadata>,
    pub compatible: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalThemeSettings {
    pub active_theme_id: String,
    pub appearance: String,
    pub high_contrast: bool,
}

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

fn builtin_ids() -> [&'static str; 5] {
    [CANVAS_ID, FLUX_ID, LEDGER_ID, AURA_ID, MONO_ID]
}

fn builtin_name(id: &str) -> &'static str {
    match id {
        CANVAS_ID => "Canvas",
        FLUX_ID => "Flux",
        LEDGER_ID => "Ledger",
        AURA_ID => "Aura",
        MONO_ID => "Mono",
        _ => "Canvas",
    }
}

fn component_state(
    background: &str,
    foreground: &str,
    secondary_text: &str,
    accent: &str,
) -> Value {
    serde_json::json!({
        "background": background,
        "foreground": foreground,
        "secondaryText": secondary_text,
        "border": "ref:/colors/border/default",
        "focusRing": accent,
        "icon": secondary_text,
        "accent": accent,
        "radius": 9,
        "padding": 10,
        "spacing": 8,
        "typography": {"size": 13, "weight": 400},
        "shadow": {"radius": 8, "y": 3, "opacity": 0.10}
    })
}

fn component_defaults() -> Value {
    let mut components = serde_json::Map::new();
    for component in COMPONENT_NAMES {
        let mut states = serde_json::Map::new();
        states.insert(
            "default".into(),
            component_state(
                if *component == "search" || *component == "input" {
                    "ref:/colors/background/input"
                } else {
                    "ref:/colors/background/surface"
                },
                "ref:/colors/text/primary",
                "ref:/colors/text/secondary",
                "ref:/colors/accent/default",
            ),
        );
        states.insert(
            "hover".into(),
            component_state(
                "ref:/colors/background/hover",
                "ref:/colors/text/primary",
                "ref:/colors/text/secondary",
                "ref:/colors/accent/hover",
            ),
        );
        states.insert(
            "active".into(),
            component_state(
                "ref:/colors/background/active",
                "ref:/colors/text/primary",
                "ref:/colors/text/secondary",
                "ref:/colors/accent/default",
            ),
        );
        states.insert(
            "selected".into(),
            component_state(
                "ref:/colors/accent/soft",
                "ref:/colors/text/primary",
                "ref:/colors/text/secondary",
                "ref:/colors/accent/default",
            ),
        );
        states.insert(
            "disabled".into(),
            component_state(
                "ref:/colors/background/surface",
                "ref:/colors/text/tertiary",
                "ref:/colors/text/tertiary",
                "ref:/colors/text/tertiary",
            ),
        );
        states.insert(
            "focus".into(),
            component_state(
                if *component == "search" || *component == "input" {
                    "ref:/colors/background/input"
                } else {
                    "ref:/colors/background/surface"
                },
                "ref:/colors/text/primary",
                "ref:/colors/text/secondary",
                "ref:/colors/accent/default",
            ),
        );
        components.insert((*component).into(), Value::Object(states));
    }
    Value::Object(components)
}

/// The built-ins are Core data, not renderer CSS.  Keep this table in lockstep
/// with the original five palettes so every client resolves the same 24 colors.
fn builtin_tokens(id: &str, mode: &str) -> Value {
    let dark = mode == "dark";
    let (
        accent,
        hover,
        soft,
        on,
        bg,
        surface,
        input,
        bg_hover,
        active,
        raised,
        toast,
        primary,
        secondary,
        tertiary,
        text_toast,
        border,
        strong,
        divider,
        positive,
        positive_soft,
        warning,
        warning_soft,
        info,
        info_soft,
    ) = match (id, dark) {
        (CANVAS_ID, false) => (
            "#d5684b",
            "#bb553b",
            "rgba(213, 104, 75, 0.11)",
            "#ffffff",
            "#faf9f5",
            "#fffefa",
            "rgba(26, 25, 22, 0.045)",
            "#f0eee7",
            "#e8e4da",
            "#fffefa",
            "#171716",
            "#191918",
            "#68665f",
            "#98958b",
            "#ffffff",
            "#e7e3d9",
            "#d3cec2",
            "#ece8df",
            "#44745a",
            "rgba(68, 116, 90, 0.11)",
            "#b96536",
            "rgba(185, 101, 54, 0.11)",
            "#765b8f",
            "rgba(118, 91, 143, 0.11)",
        ),
        (CANVAS_ID, true) => (
            "#ec8668",
            "#f29b80",
            "rgba(236, 134, 104, 0.14)",
            "#181412",
            "#191918",
            "#232321",
            "rgba(255, 253, 245, 0.055)",
            "#292825",
            "#33312d",
            "#262522",
            "#f8f5ed",
            "#f4f1e9",
            "#aaa69c",
            "#77746c",
            "#171716",
            "#32312d",
            "#48463f",
            "#2c2b28",
            "#75aa86",
            "rgba(117, 170, 134, 0.14)",
            "#dc9163",
            "rgba(220, 145, 99, 0.14)",
            "#ad8cc6",
            "rgba(173, 140, 198, 0.14)",
        ),
        (FLUX_ID, false) => (
            "#147970",
            "#0e635d",
            "rgba(20, 121, 112, 0.1)",
            "#ffffff",
            "#f4f8f6",
            "#fbfdfb",
            "rgba(10, 57, 54, 0.045)",
            "#e8f0ed",
            "#dbe8e3",
            "#ffffff",
            "#0e2422",
            "#102724",
            "#58706c",
            "#879a96",
            "#ffffff",
            "#dce8e4",
            "#bfd2cd",
            "#e3ece9",
            "#147970",
            "rgba(20, 121, 112, 0.1)",
            "#b96536",
            "rgba(185, 101, 54, 0.11)",
            "#765b8f",
            "rgba(118, 91, 143, 0.11)",
        ),
        (FLUX_ID, true) => (
            "#65c8bd",
            "#7bd7ce",
            "rgba(101, 200, 189, 0.13)",
            "#09201e",
            "#111b1a",
            "#182422",
            "rgba(226, 255, 250, 0.055)",
            "#21302e",
            "#293b38",
            "#1c2927",
            "#ecfaf7",
            "#eaf7f4",
            "#9cb7b2",
            "#6d8984",
            "#0b1816",
            "#263633",
            "#3c5450",
            "#21302e",
            "#65c8bd",
            "rgba(101, 200, 189, 0.13)",
            "#dc9163",
            "rgba(220, 145, 99, 0.14)",
            "#ad8cc6",
            "rgba(173, 140, 198, 0.14)",
        ),
        (LEDGER_ID, false) => (
            "#536859",
            "#405246",
            "rgba(83, 104, 89, 0.1)",
            "#ffffff",
            "#f7f7f3",
            "#fdfdf9",
            "rgba(38, 48, 40, 0.045)",
            "#eceee8",
            "#e0e4dc",
            "#fffffb",
            "#1b211c",
            "#202720",
            "#626b63",
            "#929a92",
            "#ffffff",
            "#e0e4dc",
            "#c8d0c7",
            "#e7e9e3",
            "#536859",
            "rgba(83, 104, 89, 0.1)",
            "#b96536",
            "rgba(185, 101, 54, 0.11)",
            "#765b8f",
            "rgba(118, 91, 143, 0.11)",
        ),
        (LEDGER_ID, true) => (
            "#93b09c",
            "#a8c1af",
            "rgba(147, 176, 156, 0.13)",
            "#121814",
            "#161a16",
            "#202520",
            "rgba(241, 252, 243, 0.05)",
            "#282e28",
            "#323a33",
            "#242a24",
            "#f2f6f1",
            "#edf2ec",
            "#a7b2a7",
            "#737d74",
            "#151a16",
            "#2d352e",
            "#465047",
            "#282f29",
            "#93b09c",
            "rgba(147, 176, 156, 0.13)",
            "#dc9163",
            "rgba(220, 145, 99, 0.14)",
            "#ad8cc6",
            "rgba(173, 140, 198, 0.14)",
        ),
        (AURA_ID, false) => (
            "#a34f75",
            "#893e61",
            "rgba(163, 79, 117, 0.1)",
            "#ffffff",
            "#faf7f9",
            "#fffdfd",
            "rgba(70, 34, 55, 0.045)",
            "#f2e9ee",
            "#eadce3",
            "#ffffff",
            "#2c1c25",
            "#2d2229",
            "#76636e",
            "#a18f99",
            "#ffffff",
            "#eadde4",
            "#d5c1cc",
            "#f0e5ea",
            "#536859",
            "rgba(83, 104, 89, 0.1)",
            "#a34f75",
            "rgba(163, 79, 117, 0.1)",
            "#a34f75",
            "rgba(163, 79, 117, 0.1)",
        ),
        (AURA_ID, true) => (
            "#dc8daf",
            "#e6a3bf",
            "rgba(220, 141, 175, 0.13)",
            "#26151d",
            "#1d171b",
            "#281f24",
            "rgba(255, 239, 247, 0.055)",
            "#33272d",
            "#3f3038",
            "#2d2328",
            "#fff5f9",
            "#faeef4",
            "#c0a6b2",
            "#866f7a",
            "#20151a",
            "#382c32",
            "#58434d",
            "#30252a",
            "#93b09c",
            "rgba(147, 176, 156, 0.13)",
            "#dc8daf",
            "rgba(220, 141, 175, 0.13)",
            "#dc8daf",
            "rgba(220, 141, 175, 0.13)",
        ),
        (MONO_ID, false) => (
            "#111111",
            "#000000",
            "rgba(0, 0, 0, 0.08)",
            "#ffffff",
            "#ffffff",
            "#ffffff",
            "#f3f3f3",
            "#eeeeee",
            "#dfdfdf",
            "#ffffff",
            "#000000",
            "#080808",
            "#454545",
            "#6d6d6d",
            "#ffffff",
            "#d5d5d5",
            "#8c8c8c",
            "#dedede",
            "#111111",
            "rgba(0, 0, 0, 0.08)",
            "#3d3d3d",
            "rgba(0, 0, 0, 0.08)",
            "#656565",
            "rgba(0, 0, 0, 0.08)",
        ),
        (MONO_ID, true) => (
            "#f5f5f5",
            "#ffffff",
            "rgba(255, 255, 255, 0.12)",
            "#080808",
            "#080808",
            "#101010",
            "#181818",
            "#1d1d1d",
            "#292929",
            "#121212",
            "#ffffff",
            "#ffffff",
            "#c8c8c8",
            "#8e8e8e",
            "#000000",
            "#2c2c2c",
            "#747474",
            "#252525",
            "#f5f5f5",
            "rgba(255, 255, 255, 0.12)",
            "#c8c8c8",
            "rgba(255, 255, 255, 0.1)",
            "#969696",
            "rgba(255, 255, 255, 0.08)",
        ),
        _ => return builtin_tokens(CANVAS_ID, mode),
    };
    let display = match id {
        CANVAS_ID | LEDGER_ID => vec![
            "STZhongsong",
            "Songti SC",
            "STSong",
            "SimSun",
            "Iowan Old Style",
            "Baskerville",
            "Georgia",
            "serif",
        ],
        FLUX_ID => vec![
            "Bahnschrift",
            "DengXian",
            "Avenir Next",
            "Hiragino Sans GB",
            "Segoe UI Variable Display",
            "sans-serif",
        ],
        AURA_ID => vec![
            "Candara",
            "YouYuan",
            "Hiragino Maru Gothic ProN",
            "PingFang SC",
            "Microsoft YaHei UI",
            "sans-serif",
        ],
        MONO_ID => vec![
            "Cascadia Mono",
            "Cascadia Code",
            "SF Mono",
            "IBM Plex Mono",
            "monospace",
        ],
        _ => vec!["system-ui"],
    };
    serde_json::json!({"colors":{"accent":{"default":accent,"hover":hover,"soft":soft,"onAccent":on},"background":{"canvas":bg,"surface":surface,"input":input,"hover":bg_hover,"active":active,"raised":raised,"toast":toast},"text":{"primary":primary,"secondary":secondary,"tertiary":tertiary,"toast":text_toast},"border":{"default":border,"strong":strong,"divider":divider},"status":{"positive":positive,"positiveSoft":positive_soft,"warning":warning,"warningSoft":warning_soft,"info":info,"infoSoft":info_soft}},"typography":{"ui":{"families":["Segoe UI Variable Text","Segoe UI","Microsoft YaHei UI","PingFang SC","sans-serif"],"size":13,"lineHeight":20,"weight":400},"display":{"families":display},"reading":{"families":["Segoe UI Variable Text","Segoe UI","Microsoft YaHei UI","PingFang SC","sans-serif"]},"search":{"size":if id == FLUX_ID || id == MONO_ID {16}else{18},"useDisplayFont":!(id == FLUX_ID || id == MONO_ID)},"section":{"size":if id == MONO_ID {18}else{23},"uppercase":id == FLUX_ID || id == MONO_ID},"history":{"size":13}},"density":{"control":13,"row":13},"shape":{"controlRadius":if id == MONO_ID {3}else{9},"surfaceRadius":if id == MONO_ID {3}else{10},"windowRadius":if id == MONO_ID {3}else{10}},"effects":{"opacity":1,"shadow":{"radius":if id == MONO_ID {0}else{70},"y":if id == MONO_ID {0}else{24},"opacity":if id == MONO_ID {0.0}else{0.16}},"motion":{"fast":160,"slow":420,"easing":"cubic-bezier(0.22, 1, 0.36, 1)"}},"components":component_defaults()})
}
fn canvas(mode: &str) -> Value {
    builtin_tokens(CANVAS_ID, mode)
}
fn merge(
    base: &mut Value,
    next: &Value,
    provenance: &mut BTreeMap<String, String>,
    origin: &str,
    path: &str,
) {
    match (base, next) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in b {
                let q = if path.is_empty() {
                    format!("/{k}")
                } else {
                    format!("{path}/{k}")
                };
                merge(
                    a.entry(k.clone()).or_insert(Value::Null),
                    v,
                    provenance,
                    origin,
                    &q,
                )
            }
        }
        (a, b) => {
            *a = b.clone();
            provenance.insert(path.into(), origin.into());
        }
    }
}
fn mark_provenance(
    value: &Value,
    provenance: &mut BTreeMap<String, String>,
    origin: &str,
    path: &str,
) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let next = if path.is_empty() {
                    format!("/{key}")
                } else {
                    format!("{path}/{key}")
                };
                mark_provenance(child, provenance, origin, &next);
            }
        }
        _ => {
            provenance.insert(path.into(), origin.into());
        }
    }
}
fn resolve_color(value: &Value, root: &Value, seen: &mut Vec<String>) -> Result<Value, ThemeError> {
    if let Value::Object(map) = value {
        if map.contains_key("systemAccent") {
            return Ok(Value::String("system".into()));
        }
        if let Some(mix) = map.get("mix") {
            let mix = object(mix, "/mix")?;
            let amount = mix.get("amount").and_then(Value::as_f64).ok_or_else(|| {
                ThemeError::new(
                    "THEME_COLOR_EXPRESSION",
                    "mix amount is required",
                    "/mix/amount",
                )
            })?;
            let base = resolve_color(
                mix.get("base").ok_or_else(|| {
                    ThemeError::new(
                        "THEME_COLOR_EXPRESSION",
                        "mix base is required",
                        "/mix/base",
                    )
                })?,
                root,
                seen,
            )?;
            let with = resolve_color(
                mix.get("with").ok_or_else(|| {
                    ThemeError::new(
                        "THEME_COLOR_EXPRESSION",
                        "mix with is required",
                        "/mix/with",
                    )
                })?,
                root,
                seen,
            )?;
            let a = parse_rgb(base.as_str().unwrap_or(""))?;
            let b = parse_rgb(with.as_str().unwrap_or(""))?;
            let rgb = [0, 1, 2]
                .map(|i| (a[i] as f64 * (1.0 - amount) + b[i] as f64 * amount).round() as u8);
            return Ok(Value::String(format!(
                "#{:02x}{:02x}{:02x}",
                rgb[0], rgb[1], rgb[2]
            )));
        }
        if let Some(contrast) = map.get("contrastColor") {
            let contrast = object(contrast, "/contrastColor")?;
            let background = resolve_color(
                contrast.get("background").ok_or_else(|| {
                    ThemeError::new(
                        "THEME_COLOR_EXPRESSION",
                        "contrast background is required",
                        "/contrastColor/background",
                    )
                })?,
                root,
                seen,
            )?;
            let light = resolve_color(
                contrast.get("light").ok_or_else(|| {
                    ThemeError::new(
                        "THEME_COLOR_EXPRESSION",
                        "contrast light is required",
                        "/contrastColor/light",
                    )
                })?,
                root,
                seen,
            )?;
            let dark = resolve_color(
                contrast.get("dark").ok_or_else(|| {
                    ThemeError::new(
                        "THEME_COLOR_EXPRESSION",
                        "contrast dark is required",
                        "/contrastColor/dark",
                    )
                })?,
                root,
                seen,
            )?;
            let bg_luma = relative_luminance(parse_rgb(background.as_str().unwrap_or(""))?);
            let light_luma = relative_luminance(parse_rgb(light.as_str().unwrap_or(""))?);
            let dark_luma = relative_luminance(parse_rgb(dark.as_str().unwrap_or(""))?);
            let light_ratio = contrast_ratio(bg_luma, light_luma);
            let dark_ratio = contrast_ratio(bg_luma, dark_luma);
            let minimum = contrast
                .get("minimumRatio")
                .and_then(Value::as_f64)
                .unwrap_or(4.5);
            if light_ratio >= minimum || light_ratio >= dark_ratio {
                return Ok(light);
            }
            return Ok(dark);
        }
    }
    let Value::String(s) = value else {
        return Ok(value.clone());
    };
    if s == "system" {
        return Ok(value.clone());
    }
    if s.starts_with('#') || s.starts_with("rgba(") {
        parse_rgb(s)?;
        return Ok(value.clone());
    }
    if let Some(path) = s.strip_prefix("ref:") {
        if seen.len() >= 16 {
            return err(
                "THEME_COLOR_DEPTH",
                "color expression nesting is too deep",
                path,
            );
        }
        if seen.iter().any(|x| x == path) {
            return err("THEME_COLOR_CYCLE", "cyclic color reference", path);
        }
        seen.push(path.into());
        let mut cur = root;
        for key in path.trim_start_matches('/').split('/') {
            cur = cur.get(key).ok_or_else(|| {
                ThemeError::new("THEME_COLOR_REFERENCE", "unknown color reference", path)
            })?
        }
        let out = resolve_color(cur, root, seen);
        seen.pop();
        return out;
    }
    if let Some(alpha) = s.strip_prefix("alpha(").and_then(|x| x.strip_suffix(')')) {
        let mut p = alpha.rsplitn(2, ',');
        let a: f64 = match p.next().and_then(|x| x.trim().parse().ok()) {
            Some(v) if (0.0..=1.0).contains(&v) => v,
            _ => return err("THEME_COLOR_EXPRESSION", "alpha must be in [0,1]", ""),
        };
        let c = resolve_color(
            &Value::String(p.next().unwrap_or("").trim().into()),
            root,
            seen,
        )?;
        let rgb = parse_rgb(c.as_str().unwrap_or("#000000"))?;
        return Ok(Value::String(format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            rgb[0],
            rgb[1],
            rgb[2],
            (a * 255.0).round() as u8
        )));
    }
    err("THEME_COLOR_EXPRESSION", "unsupported color value", "")
}

fn parse_rgb(value: &str) -> Result<[u8; 3], ThemeError> {
    // `system` is a platform-bound output token. Expressions that need a
    // concrete color use this deterministic fallback; renderers replace a
    // direct system token with the actual OS accent at the final boundary.
    if value == "system" {
        return Ok([0, 120, 212]);
    }
    if let Some(body) = value
        .strip_prefix("rgba(")
        .and_then(|v| v.strip_suffix(')'))
    {
        let channels = body
            .split(',')
            .map(|channel| channel.trim().parse::<f64>().ok())
            .collect::<Option<Vec<_>>>();
        if let Some(channels) = channels.filter(|channels| {
            channels.len() == 4
                && channels[..3]
                    .iter()
                    .all(|channel| (0.0..=255.0).contains(channel))
                && (0.0..=1.0).contains(&channels[3])
        }) {
            return Ok([
                channels[0].round() as u8,
                channels[1].round() as u8,
                channels[2].round() as u8,
            ]);
        }
    }
    let raw = value.strip_prefix('#').ok_or_else(|| {
        ThemeError::new(
            "THEME_COLOR_EXPRESSION",
            "color expression must resolve to #rrggbb",
            "",
        )
    })?;
    let raw = if raw.len() == 3 {
        format!(
            "{}{}{}{}{}{}",
            &raw[0..1],
            &raw[0..1],
            &raw[1..2],
            &raw[1..2],
            &raw[2..3],
            &raw[2..3]
        )
    } else {
        raw.to_owned()
    };
    if raw.len() != 6 && raw.len() != 8 {
        return err(
            "THEME_COLOR_EXPRESSION",
            "color expression must resolve to #rrggbb",
            "",
        );
    }
    u64::from_str_radix(&raw, 16)
        .map_err(|_| ThemeError::new("THEME_COLOR_EXPRESSION", "invalid hexadecimal color", ""))?;
    let bytes = (0..3)
        .map(|index| {
            u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16).map_err(|_| {
                ThemeError::new("THEME_COLOR_EXPRESSION", "invalid hexadecimal color", "")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok([bytes[0], bytes[1], bytes[2]])
}

fn relative_luminance(rgb: [u8; 3]) -> f64 {
    let channel = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2])
}

fn contrast_ratio(a: f64, b: f64) -> f64 {
    let (high, low) = if a > b { (a, b) } else { (b, a) };
    (high + 0.05) / (low + 0.05)
}

#[derive(Clone, Copy)]
struct ContrastColor {
    rgb: [f64; 3],
    alpha: f64,
}

fn contrast_color(value: &Value) -> Option<ContrastColor> {
    let value = value.as_str()?;
    if value == "system" {
        return Some(ContrastColor {
            rgb: [0.0, 120.0 / 255.0, 212.0 / 255.0],
            alpha: 1.0,
        });
    }
    if let Some(raw) = value.strip_prefix('#') {
        let expanded = if raw.len() == 3 {
            raw.chars()
                .flat_map(|character| [character, character])
                .collect::<String>()
        } else {
            raw.to_owned()
        };
        if expanded.len() != 6 && expanded.len() != 8 {
            return None;
        }
        let byte = |index| u8::from_str_radix(&expanded[index..index + 2], 16).ok();
        return Some(ContrastColor {
            rgb: [
                f64::from(byte(0)?) / 255.0,
                f64::from(byte(2)?) / 255.0,
                f64::from(byte(4)?) / 255.0,
            ],
            alpha: if expanded.len() == 8 {
                f64::from(byte(6)?) / 255.0
            } else {
                1.0
            },
        });
    }
    let body = value.strip_prefix("rgba(")?.strip_suffix(')')?;
    let channels = body
        .split(',')
        .map(|channel| channel.trim().parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;
    (channels.len() == 4
        && channels[..3]
            .iter()
            .all(|channel| (0.0..=255.0).contains(channel))
        && (0.0..=1.0).contains(&channels[3]))
    .then(|| ContrastColor {
        rgb: [
            channels[0] / 255.0,
            channels[1] / 255.0,
            channels[2] / 255.0,
        ],
        alpha: channels[3],
    })
}

fn composite(foreground: ContrastColor, background: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|index| {
        foreground.rgb[index] * foreground.alpha + background[index] * (1.0 - foreground.alpha)
    })
}

fn luminance(rgb: [f64; 3]) -> f64 {
    let channel = |value: f64| {
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2])
}

fn opaque_background(tokens: &Value, path: &str, canvas_path: &str) -> Option<[f64; 3]> {
    let background = contrast_color(tokens.pointer(path)?)?;
    if path == canvas_path {
        return Some(background.rgb);
    }
    let canvas = contrast_color(tokens.pointer(canvas_path)?)?;
    Some(composite(background, composite(canvas, [1.0, 1.0, 1.0])))
}

fn color_contrast(
    tokens: &Value,
    foreground_path: &str,
    background_path: &str,
    canvas_path: &str,
) -> Option<f64> {
    let background = opaque_background(tokens, background_path, canvas_path)?;
    let foreground = contrast_color(tokens.pointer(foreground_path)?)?;
    Some(contrast_ratio(
        luminance(composite(foreground, background)),
        luminance(background),
    ))
}

fn set_policy_value(
    tokens: &mut Value,
    provenance: &mut BTreeMap<String, String>,
    path: &str,
    value: Value,
) {
    let Some(current) = tokens.pointer_mut(path) else {
        return;
    };
    if *current != value {
        *current = value;
        provenance.insert(path.into(), "systemHighContrast".into());
    }
}

fn rgb_hex(rgb: [f64; 3]) -> Value {
    Value::String(format!(
        "#{:02x}{:02x}{:02x}",
        (rgb[0] * 255.0).round() as u8,
        (rgb[1] * 255.0).round() as u8,
        (rgb[2] * 255.0).round() as u8
    ))
}

fn make_opaque(
    tokens: &mut Value,
    provenance: &mut BTreeMap<String, String>,
    path: &str,
    canvas_path: &str,
) {
    let Some(color) = tokens.pointer(path).and_then(contrast_color) else {
        return;
    };
    if color.alpha >= 1.0 {
        return;
    }
    let rgb = if path == canvas_path {
        color.rgb
    } else {
        let canvas = tokens
            .pointer(canvas_path)
            .and_then(contrast_color)
            .map(|canvas| composite(canvas, [1.0, 1.0, 1.0]))
            .unwrap_or([1.0, 1.0, 1.0]);
        composite(color, canvas)
    };
    set_policy_value(tokens, provenance, path, rgb_hex(rgb));
}

fn ensure_contrast(
    tokens: &mut Value,
    provenance: &mut BTreeMap<String, String>,
    foreground_path: &str,
    background_path: &str,
    canvas_path: &str,
    minimum: f64,
) {
    let Some(ratio) = color_contrast(tokens, foreground_path, background_path, canvas_path) else {
        return;
    };
    if ratio >= minimum {
        return;
    }
    let Some(background) = opaque_background(tokens, background_path, canvas_path) else {
        return;
    };
    let black = contrast_ratio(luminance([0.0, 0.0, 0.0]), luminance(background));
    let white = contrast_ratio(luminance([1.0, 1.0, 1.0]), luminance(background));
    set_policy_value(
        tokens,
        provenance,
        foreground_path,
        Value::String(if white > black { "#ffffff" } else { "#000000" }.into()),
    );
}
fn pointer(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

fn object<'a>(
    value: &'a Value,
    path: &str,
) -> Result<&'a serde_json::Map<String, Value>, ThemeError> {
    value
        .as_object()
        .ok_or_else(|| ThemeError::new("THEME_TOKEN_TYPE", "token group must be an object", path))
}

fn only_keys(
    values: &serde_json::Map<String, Value>,
    allowed: &[&str],
    path: &str,
) -> Result<(), ThemeError> {
    if let Some(key) = values.keys().find(|key| !allowed.contains(&key.as_str())) {
        return err(
            "THEME_TOKEN_UNKNOWN",
            format!("unknown token '{key}'"),
            pointer(path, key),
        );
    }
    Ok(())
}

fn string(value: &Value, path: &str, description: &str) -> Result<(), ThemeError> {
    let value = value.as_str().ok_or_else(|| {
        ThemeError::new(
            "THEME_TOKEN_TYPE",
            format!("{description} must be a string"),
            path,
        )
    })?;
    if value.contains("url(")
        || value.contains('<')
        || value.contains(';')
        || value.contains("@import")
    {
        return err(
            "THEME_UNSAFE_VALUE",
            "CSS, URL, and markup are forbidden",
            path,
        );
    }
    Ok(())
}

fn validate_color_string(value: &str, path: &str) -> Result<(), ThemeError> {
    if value == "system"
        || value.starts_with("ref:")
        || value.starts_with("alpha(")
        || value.starts_with('#')
        || value.starts_with("rgba(")
    {
        if value.starts_with('#') || value.starts_with("rgba(") {
            parse_rgb(value).map_err(|mut error| {
                error.json_pointer = path.into();
                error
            })?;
        }
        return Ok(());
    }
    err(
        "THEME_UNSAFE_VALUE",
        "color must be a supported literal or expression",
        path,
    )
}

fn bounded_number(
    value: &Value,
    path: &str,
    min: f64,
    max: f64,
    description: &str,
) -> Result<(), ThemeError> {
    let value = value.as_f64().ok_or_else(|| {
        ThemeError::new(
            "THEME_TOKEN_TYPE",
            format!("{description} must be a number"),
            path,
        )
    })?;
    if !value.is_finite() || !(min..=max).contains(&value) {
        return err(
            "THEME_TOKEN_RANGE",
            format!("{description} must be between {min} and {max}"),
            path,
        );
    }
    Ok(())
}

fn color_token(value: &Value, path: &str) -> Result<(), ThemeError> {
    match value {
        Value::String(value) => {
            string(&Value::String(value.clone()), path, "color token")?;
            validate_color_string(value, path)
        }
        Value::Object(map) => {
            only_keys(map, &["mix", "contrastColor", "systemAccent"], path)?;
            if map.len() != 1 {
                return err(
                    "THEME_COLOR_EXPRESSION",
                    "color expression must contain exactly one operator",
                    path,
                );
            }
            if let Some(mix) = map.get("mix") {
                let p = pointer(path, "mix");
                let mix = object(mix, &p)?;
                only_keys(mix, &["base", "with", "amount"], &p)?;
                for key in ["base", "with"] {
                    let value = mix.get(key).ok_or_else(|| {
                        ThemeError::new(
                            "THEME_COLOR_EXPRESSION",
                            format!("mix.{key} is required"),
                            pointer(&p, key),
                        )
                    })?;
                    color_token(value, &pointer(&p, key))?;
                }
                let amount = mix.get("amount").ok_or_else(|| {
                    ThemeError::new(
                        "THEME_COLOR_EXPRESSION",
                        "mix.amount is required",
                        pointer(&p, "amount"),
                    )
                })?;
                bounded_number(amount, &pointer(&p, "amount"), 0.0, 1.0, "mix amount")?;
            } else if let Some(contrast) = map.get("contrastColor") {
                let p = pointer(path, "contrastColor");
                let contrast = object(contrast, &p)?;
                only_keys(
                    contrast,
                    &["background", "light", "dark", "minimumRatio"],
                    &p,
                )?;
                let background = contrast.get("background").ok_or_else(|| {
                    ThemeError::new(
                        "THEME_COLOR_EXPRESSION",
                        "contrastColor.background is required",
                        pointer(&p, "background"),
                    )
                })?;
                color_token(background, &pointer(&p, "background"))?;
                for key in ["light", "dark"] {
                    let value = contrast.get(key).ok_or_else(|| {
                        ThemeError::new(
                            "THEME_COLOR_EXPRESSION",
                            format!("contrastColor.{key} is required"),
                            pointer(&p, key),
                        )
                    })?;
                    color_token(value, &pointer(&p, key))?;
                }
                if let Some(ratio) = contrast.get("minimumRatio") {
                    bounded_number(
                        ratio,
                        &pointer(&p, "minimumRatio"),
                        1.0,
                        21.0,
                        "minimum ratio",
                    )?;
                }
            } else if let Some(system) = map.get("systemAccent") {
                if system != &Value::Bool(true) {
                    return err(
                        "THEME_COLOR_EXPRESSION",
                        "systemAccent must be true",
                        pointer(path, "systemAccent"),
                    );
                }
            }
            Ok(())
        }
        _ => err(
            "THEME_TOKEN_TYPE",
            "color token must be a string or expression",
            path,
        ),
    }
}

fn optional_object<'a>(
    values: &'a serde_json::Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<&'a serde_json::Map<String, Value>>, ThemeError> {
    values
        .get(key)
        .map(|value| object(value, &pointer(path, key)))
        .transpose()
}

const COMPONENT_NAMES: &[&str] = &[
    "search", "history", "section", "panel", "button", "input", "toast",
];
const COMPONENT_STATES: &[&str] = &[
    "default", "hover", "active", "selected", "disabled", "focus",
];
const COMPONENT_COLOR_FIELDS: &[&str] = &[
    "background",
    "foreground",
    "secondaryText",
    "border",
    "focusRing",
    "icon",
    "accent",
];

/// Component tokens are semantic UI roles, never renderer properties.  Their
/// compact shared shape lets a package override only the states it needs while
/// Canvas supplies the complete fallback tree.
fn validate_component_tokens(value: &Value, path: &str) -> Result<(), ThemeError> {
    let components = object(value, path)?;
    only_keys(components, COMPONENT_NAMES, path)?;
    for (component, states) in components {
        let component_path = pointer(path, component);
        let states = object(states, &component_path)?;
        only_keys(states, COMPONENT_STATES, &component_path)?;
        for (state, fields) in states {
            let state_path = pointer(&component_path, state);
            let fields = object(fields, &state_path)?;
            only_keys(
                fields,
                &[
                    "background",
                    "foreground",
                    "secondaryText",
                    "border",
                    "focusRing",
                    "icon",
                    "accent",
                    "radius",
                    "padding",
                    "spacing",
                    "typography",
                    "shadow",
                ],
                &state_path,
            )?;
            for field in COMPONENT_COLOR_FIELDS {
                if let Some(value) = fields.get(*field) {
                    color_token(value, &pointer(&state_path, field))?;
                }
            }
            for field in ["radius", "padding", "spacing"] {
                if let Some(value) = fields.get(field) {
                    bounded_number(value, &pointer(&state_path, field), 0.0, 128.0, field)?;
                }
            }
            if let Some(typography) = fields.get("typography") {
                let typography_path = pointer(&state_path, "typography");
                let typography = object(typography, &typography_path)?;
                only_keys(typography, &["size", "weight"], &typography_path)?;
                if let Some(value) = typography.get("size") {
                    bounded_number(
                        value,
                        &pointer(&typography_path, "size"),
                        1.0,
                        256.0,
                        "size",
                    )?;
                }
                if let Some(value) = typography.get("weight") {
                    bounded_number(
                        value,
                        &pointer(&typography_path, "weight"),
                        100.0,
                        900.0,
                        "weight",
                    )?;
                }
            }
            if let Some(shadow) = fields.get("shadow") {
                let shadow_path = pointer(&state_path, "shadow");
                let shadow = object(shadow, &shadow_path)?;
                only_keys(shadow, &["radius", "y", "opacity"], &shadow_path)?;
                for field in ["radius", "y"] {
                    if let Some(value) = shadow.get(field) {
                        bounded_number(value, &pointer(&shadow_path, field), 0.0, 128.0, field)?;
                    }
                }
                if let Some(value) = shadow.get("opacity") {
                    bounded_number(
                        value,
                        &pointer(&shadow_path, "opacity"),
                        0.0,
                        1.0,
                        "opacity",
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn validate_tokens(value: &Value, path: &str) -> Result<(), ThemeError> {
    let root = object(value, path)?;
    only_keys(
        root,
        &[
            "colors",
            "typography",
            "density",
            "shape",
            "effects",
            "components",
        ],
        path,
    )?;

    if let Some(components) = root.get("components") {
        validate_component_tokens(components, &pointer(path, "components"))?;
    }

    if let Some(colors) = optional_object(root, "colors", path)? {
        let colors_path = pointer(path, "colors");
        only_keys(
            colors,
            &["background", "text", "accent", "border", "status"],
            &colors_path,
        )?;
        for (group, keys) in [
            (
                "background",
                &[
                    "canvas", "surface", "input", "hover", "active", "raised", "toast",
                ][..],
            ),
            ("text", &["primary", "secondary", "tertiary", "toast"][..]),
            ("accent", &["default", "hover", "soft", "onAccent"][..]),
            ("border", &["default", "strong", "divider"][..]),
            (
                "status",
                &[
                    "positive",
                    "positiveSoft",
                    "warning",
                    "warningSoft",
                    "info",
                    "infoSoft",
                ][..],
            ),
        ] {
            if let Some(group_values) = optional_object(colors, group, &colors_path)? {
                let group_path = pointer(&colors_path, group);
                only_keys(group_values, keys, &group_path)?;
                for key in keys {
                    if let Some(value) = group_values.get(*key) {
                        color_token(value, &pointer(&group_path, key))?;
                    }
                }
            }
        }
    }

    if let Some(typography) = optional_object(root, "typography", path)? {
        let typography_path = pointer(path, "typography");
        only_keys(
            typography,
            &["ui", "display", "reading", "search", "section", "history"],
            &typography_path,
        )?;
        if let Some(ui) = optional_object(typography, "ui", &typography_path)? {
            let ui_path = pointer(&typography_path, "ui");
            only_keys(ui, &["families", "size", "lineHeight", "weight"], &ui_path)?;
            if let Some(families) = ui.get("families") {
                let families = families.as_array().ok_or_else(|| {
                    ThemeError::new(
                        "THEME_TOKEN_TYPE",
                        "font families must be an array",
                        pointer(&ui_path, "families"),
                    )
                })?;
                if families.is_empty() {
                    return err(
                        "THEME_TOKEN_RANGE",
                        "font families must not be empty",
                        pointer(&ui_path, "families"),
                    );
                }
                for (index, family) in families.iter().enumerate() {
                    let family_path = format!("{}/{}", pointer(&ui_path, "families"), index);
                    string(family, &family_path, "font family")?;
                    if family
                        .as_str()
                        .is_some_and(|family| family.trim().is_empty())
                    {
                        return err(
                            "THEME_TOKEN_RANGE",
                            "font family must not be empty",
                            family_path,
                        );
                    }
                }
            }
            for key in ["size", "lineHeight", "weight"] {
                if let Some(value) = ui.get(key) {
                    bounded_number(value, &pointer(&ui_path, key), 1.0, 256.0, key)?;
                }
            }
        }
        for group in ["display", "reading"] {
            if let Some(font) = optional_object(typography, group, &typography_path)? {
                let p = pointer(&typography_path, group);
                only_keys(font, &["families"], &p)?;
                if let Some(f) = font.get("families") {
                    let a = f.as_array().ok_or_else(|| {
                        ThemeError::new(
                            "THEME_TOKEN_TYPE",
                            "font families must be an array",
                            pointer(&p, "families"),
                        )
                    })?;
                    if a.is_empty() {
                        return err(
                            "THEME_TOKEN_RANGE",
                            "font families must not be empty",
                            pointer(&p, "families"),
                        );
                    }
                    for (i, x) in a.iter().enumerate() {
                        string(
                            x,
                            &format!("{}/{}", pointer(&p, "families"), i),
                            "font family",
                        )?;
                    }
                }
            }
        }
        if let Some(search) = optional_object(typography, "search", &typography_path)? {
            let search_path = pointer(&typography_path, "search");
            only_keys(search, &["size", "useDisplayFont"], &search_path)?;
            if let Some(value) = search.get("size") {
                bounded_number(value, &pointer(&search_path, "size"), 1.0, 256.0, "size")?;
            }
            if let Some(value) = search.get("useDisplayFont") {
                if !value.is_boolean() {
                    return err(
                        "THEME_TOKEN_TYPE",
                        "useDisplayFont must be a boolean",
                        pointer(&search_path, "useDisplayFont"),
                    );
                }
            }
        }
        for group in ["section", "history"] {
            if let Some(values) = optional_object(typography, group, &typography_path)? {
                let p = pointer(&typography_path, group);
                only_keys(
                    values,
                    if group == "section" {
                        &["size", "uppercase"]
                    } else {
                        &["size"]
                    },
                    &p,
                )?;
                if let Some(v) = values.get("size") {
                    bounded_number(v, &pointer(&p, "size"), 1.0, 256.0, "size")?;
                }
                if let Some(v) = values.get("uppercase") {
                    if !v.is_boolean() {
                        return err(
                            "THEME_TOKEN_TYPE",
                            "uppercase must be a boolean",
                            pointer(&p, "uppercase"),
                        );
                    }
                }
            }
        }
    }

    for (group, keys, max) in [
        ("density", &["control", "row"][..], 128.0),
        (
            "shape",
            &["controlRadius", "surfaceRadius", "windowRadius"][..],
            128.0,
        ),
    ] {
        if let Some(values) = optional_object(root, group, path)? {
            let group_path = pointer(path, group);
            only_keys(values, keys, &group_path)?;
            for key in keys {
                if let Some(value) = values.get(*key) {
                    bounded_number(value, &pointer(&group_path, key), 0.0, max, key)?;
                }
            }
        }
    }

    if let Some(effects) = optional_object(root, "effects", path)? {
        let effects_path = pointer(path, "effects");
        only_keys(effects, &["opacity", "shadow", "motion"], &effects_path)?;
        if let Some(value) = effects.get("opacity") {
            bounded_number(
                value,
                &pointer(&effects_path, "opacity"),
                0.0,
                1.0,
                "opacity",
            )?;
        }
        if let Some(motion) = optional_object(effects, "motion", &effects_path)? {
            let motion_path = pointer(&effects_path, "motion");
            only_keys(motion, &["fast", "slow", "easing"], &motion_path)?;
            for key in ["fast", "slow"] {
                if let Some(value) = motion.get(key) {
                    bounded_number(
                        value,
                        &pointer(&motion_path, key),
                        0.0,
                        10_000.0,
                        "duration",
                    )?;
                }
            }
            if let Some(value) = motion.get("easing") {
                string(value, &pointer(&motion_path, "easing"), "easing")?;
                if !matches!(
                    value.as_str(),
                    Some("standard" | "linear" | "easeIn" | "easeOut" | "easeInOut")
                ) {
                    return err(
                        "THEME_TOKEN_VALUE",
                        "easing is not supported",
                        pointer(&motion_path, "easing"),
                    );
                }
            }
        }
        if let Some(shadow) = optional_object(effects, "shadow", &effects_path)? {
            let p = pointer(&effects_path, "shadow");
            only_keys(shadow, &["radius", "y", "opacity"], &p)?;
            for key in ["radius", "y"] {
                if let Some(v) = shadow.get(key) {
                    bounded_number(v, &pointer(&p, key), 0.0, 128.0, key)?;
                }
            }
            if let Some(v) = shadow.get("opacity") {
                bounded_number(v, &pointer(&p, "opacity"), 0.0, 1.0, "opacity")?;
            }
        }
    }
    Ok(())
}

fn read_package(bytes: &[u8]) -> Result<(ThemeManifest, Vec<AssetMetadata>), ThemeError> {
    if bytes.len() > MAX_COMPRESSED_BYTES {
        return err(
            "THEME_PACKAGE_TOO_LARGE",
            "compressed package exceeds 25 MiB",
            "",
        );
    };
    let mut z = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| ThemeError::new("THEME_ARCHIVE", "invalid .tailsync-theme archive", ""))?;
    if z.len() > MAX_ASSETS + 1 {
        return err("THEME_TOO_MANY_FILES", "too many package files", "");
    };
    let mut json = None;
    let mut assets = Vec::new();
    let mut total: u64 = 0;
    let mut pixels: u64 = 0;
    for i in 0..z.len() {
        let mut f = z
            .by_index(i)
            .map_err(|_| ThemeError::new("THEME_ARCHIVE", "unreadable archive entry", ""))?;
        let n = f.name().to_owned();
        if !safe_zip_path(&n) {
            return err(
                "THEME_PATH_TRAVERSAL",
                "archive contains unsafe path",
                format!("/{n}"),
            );
        }
        total = total
            .checked_add(f.size())
            .ok_or_else(|| ThemeError::new("THEME_PACKAGE_TOO_LARGE", "size overflow", ""))?;
        if total > MAX_UNCOMPRESSED_BYTES {
            return err(
                "THEME_PACKAGE_TOO_LARGE",
                "uncompressed package exceeds 50 MiB",
                "",
            );
        };
        let mut data = Vec::new();
        f.read_to_end(&mut data).map_err(|_| {
            ThemeError::new(
                "THEME_ARCHIVE",
                "could not read archive entry",
                format!("/{n}"),
            )
        })?;
        if n == "theme.json" {
            json = Some(data)
        } else if n.starts_with("assets/") {
            if f.size() > MAX_ASSET_BYTES {
                return err(
                    "THEME_ASSET_TOO_LARGE",
                    "asset exceeds 10 MiB",
                    format!("/{n}"),
                );
            };
            let mime = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
                "image/png"
            } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
                "image/jpeg"
            } else {
                return err(
                    "THEME_ASSET_MIME",
                    "only PNG/JPEG assets are allowed",
                    format!("/{n}"),
                );
            };
            let (w, h) = image_size(&data, mime).ok_or_else(|| {
                ThemeError::new(
                    "THEME_ASSET_DIMENSIONS",
                    "invalid image dimensions",
                    format!("/{n}"),
                )
            })?;
            if w > 8192 || h > 8192 {
                return err(
                    "THEME_ASSET_DIMENSIONS",
                    "asset dimension exceeds 8192",
                    format!("/{n}"),
                );
            };
            pixels += w as u64 * h as u64;
            if pixels > MAX_DECODED_PIXELS {
                return err(
                    "THEME_ASSET_PIXELS",
                    "total decoded pixels exceeds limit",
                    "",
                );
            };
            assets.push(AssetMetadata {
                key: n,
                digest: blake3::hash(&data).to_hex().to_string(),
                mime_type: mime.into(),
                bytes: data.len() as u64,
                width: w,
                height: h,
            });
        } else {
            return err(
                "THEME_PACKAGE_FILE",
                "only theme.json and assets/ are permitted",
                format!("/{n}"),
            );
        }
    }
    let raw = json.ok_or_else(|| {
        ThemeError::new(
            "THEME_MANIFEST_MISSING",
            "theme.json is required",
            "/theme.json",
        )
    })?;
    let m: ThemeManifest = serde_json::from_slice(&raw)
        .map_err(|e| ThemeError::new("THEME_MANIFEST", e.to_string(), "/theme.json"))?;
    Ok((m, assets))
}
fn validate_asset_slots(
    slots: &BTreeMap<String, String>,
    assets: &[AssetMetadata],
) -> Result<(), ThemeError> {
    let allowed = ["logo", "emptyState", "previewPlaceholder"];
    if let Some(slot) = slots.keys().find(|slot| !allowed.contains(&slot.as_str())) {
        return err(
            "THEME_ASSET_SLOT",
            "unknown semantic asset slot",
            pointer("/assetSlots", slot),
        );
    }
    for (slot, key) in slots {
        let path = pointer("/assetSlots", slot);
        if !key.starts_with("assets/") || !safe_zip_path(key) {
            return err(
                "THEME_ASSET_SLOT",
                "asset slot must reference an asset packaged under assets/",
                path,
            );
        }
        if !assets.iter().any(|asset| asset.key == *key) {
            return err(
                "THEME_ASSET_SLOT",
                "asset slot references an asset that is not in this package",
                path,
            );
        }
    }
    Ok(())
}

fn resolved_asset_slots(
    slots: &BTreeMap<String, String>,
    assets: &[AssetMetadata],
) -> BTreeMap<String, ThemeAssetDescriptor> {
    slots
        .iter()
        .filter_map(|(slot, key)| {
            assets.iter().find(|asset| asset.key == *key).map(|asset| {
                (
                    slot.clone(),
                    ThemeAssetDescriptor {
                        slot: slot.clone(),
                        key: asset.key.clone(),
                        digest: asset.digest.clone(),
                        mime_type: asset.mime_type.clone(),
                        bytes: asset.bytes,
                        width: asset.width,
                        height: asset.height,
                    },
                )
            })
        })
        .collect()
}

fn validate_manifest(
    m: &ThemeManifest,
    digest: &str,
    assets: &[AssetMetadata],
) -> Result<(), ThemeError> {
    if m.format_version != FORMAT_VERSION {
        return err(
            "THEME_FORMAT",
            "only formatVersion 2 is supported",
            "/formatVersion",
        );
    };
    if !valid_custom_id(&m.id) {
        return err(
            "THEME_ID",
            "custom id must be custom:<author>.<name>",
            "/id",
        );
    };
    if semver(&m.version).is_none() {
        return err("THEME_VERSION", "version must be SemVer", "/version");
    };
    let minimum_core = semver(&m.min_core_version).ok_or_else(|| {
        ThemeError::new(
            "THEME_MIN_CORE_VERSION",
            "minCoreVersion must be SemVer",
            "/minCoreVersion",
        )
    })?;
    let core = semver(CORE_VERSION).expect("CORE_VERSION must be valid SemVer");
    if compare_semver(&minimum_core, &core).is_gt() {
        return err(
            "THEME_MIN_CORE_VERSION",
            format!(
                "theme requires Core {}, but this Core is {CORE_VERSION}",
                m.min_core_version
            ),
            "/minCoreVersion",
        );
    }
    for (index, capability) in m.required_capabilities.iter().enumerate() {
        if !SUPPORTED_CAPABILITIES.contains(&capability.as_str()) {
            return err(
                "THEME_CAPABILITY_UNSUPPORTED",
                format!("required capability '{capability}' is not supported"),
                format!("/requiredCapabilities/{index}"),
            );
        }
    }
    if m.extends != CANVAS_ID {
        return err(
            "THEME_EXTENDS",
            "themes may only inherit builtin:canvas@1",
            "/extends",
        );
    };
    if !m.name.contains_key("en") {
        return err("THEME_NAME", "name.en is required", "/name/en");
    };
    if m.digest.as_ref().is_some_and(|v| v != digest) {
        return err(
            "THEME_DIGEST",
            "manifest digest does not match package",
            "/digest",
        );
    };
    validate_asset_slots(&m.asset_slots, assets)?;
    for (p, v) in [
        ("/light", &m.light),
        ("/dark", &m.dark),
        ("/foundation", &m.foundation),
        ("/platform/windows", &m.platform.windows),
        ("/platform/macos", &m.platform.macos),
    ] {
        validate_tokens(v, p)?
    }
    validate_component_tokens(&m.components, "/components")?;
    if let Some(high_contrast) = &m.high_contrast {
        validate_tokens(&high_contrast.light, "/highContrast/light")?;
        validate_tokens(&high_contrast.dark, "/highContrast/dark")?;
    }
    for (p, v) in [("/light", &m.light), ("/dark", &m.dark)] {
        for x in ["colors/background/canvas", "colors/text/primary"] {
            let mut q = v;
            for k in x.split('/') {
                q = q.get(k).ok_or_else(|| {
                    ThemeError::new(
                        "THEME_REQUIRED_TOKEN",
                        format!("{x} is required"),
                        format!("{p}/{x}"),
                    )
                })?
            }
            color_token(q, &format!("{p}/{x}"))?;
        }
    }
    Ok(())
}

fn validate_all_resolved_modes(
    manifest: &ThemeManifest,
    digest: &str,
    assets: &[AssetMetadata],
) -> Result<(), ThemeError> {
    for platform in ["windows", "macos"] {
        for mode in ["light", "dark"] {
            for high_contrast in [false, true] {
                resolve_manifest(
                    manifest,
                    digest,
                    mode,
                    platform,
                    high_contrast,
                    assets.to_vec(),
                )?;
            }
        }
    }
    Ok(())
}

pub fn validate_theme_for_platform(
    bytes: &[u8],
    mode: &str,
    platform: &str,
    high: bool,
) -> ThemeValidation {
    let digest = package_digest(bytes);
    match read_package(bytes).and_then(|(m, a)| {
        validate_manifest(&m, &digest, &a)?;
        let author_preview =
            resolve_manifest_unenforced(&m, &digest, mode, platform, high, a.clone())?;
        let mut preview = author_preview.clone();
        if high {
            enforce_high_contrast(&mut preview.tokens, &mut preview.provenance);
        }
        Ok((author_preview, preview, a, m.version))
    }) {
        Ok((author_preview, preview, assets, candidate_version)) => {
            let mut diagnostics = accessibility_diagnostics(&author_preview, high);
            for diagnostic in &mut diagnostics {
                diagnostic.platforms = vec![platform.into()];
                diagnostic.fallback_applied = high
                    && preview
                        .provenance
                        .get(&diagnostic.json_pointer)
                        .is_some_and(|origin| origin == "systemHighContrast");
            }
            ThemeValidation {
                valid: true,
                digest: Some(digest),
                candidate_version: Some(candidate_version),
                preview: Some(preview),
                diagnostics,
                assets,
                compatible: true,
            }
        }
        Err(mut e) => {
            e.platforms = vec![platform.into()];
            ThemeValidation {
                valid: false,
                digest: None,
                candidate_version: None,
                preview: None,
                diagnostics: vec![e],
                assets: vec![],
                compatible: false,
            }
        }
    }
}

pub fn validate_theme(bytes: &[u8], mode: &str, high: bool) -> ThemeValidation {
    validate_theme_for_platform(bytes, mode, "windows", high)
}

/// Non-blocking WCAG diagnostics for package authors. Resolver output remains
/// usable; clients can present these warnings before installation/activation.
fn accessibility_diagnostics(resolved: &ResolvedTheme, high_contrast: bool) -> Vec<ThemeError> {
    let tokens = &resolved.tokens;
    let canvas = "/colors/background/canvas";
    let surface = "/colors/background/surface";
    let text_minimum = if high_contrast { 7.0 } else { 4.5 };
    let mut pairs = vec![
        (
            "/colors/text/primary".to_string(),
            surface.to_string(),
            text_minimum,
            "primary text".to_string(),
        ),
        (
            "/colors/text/secondary".to_string(),
            surface.to_string(),
            text_minimum,
            "secondary text".to_string(),
        ),
        (
            "/colors/text/tertiary".to_string(),
            surface.to_string(),
            text_minimum,
            "tertiary text".to_string(),
        ),
        (
            "/colors/text/toast".to_string(),
            "/colors/background/toast".to_string(),
            text_minimum,
            "toast text".to_string(),
        ),
        (
            "/colors/accent/onAccent".to_string(),
            "/colors/accent/default".to_string(),
            text_minimum,
            "accent text".to_string(),
        ),
        (
            "/colors/status/positive".to_string(),
            surface.to_string(),
            3.0,
            "positive status".to_string(),
        ),
        (
            "/colors/status/warning".to_string(),
            surface.to_string(),
            3.0,
            "warning status".to_string(),
        ),
        (
            "/colors/status/info".to_string(),
            surface.to_string(),
            3.0,
            "informational status".to_string(),
        ),
    ];
    if let Some(components) = tokens.pointer("/components").and_then(Value::as_object) {
        for (component, states) in components {
            let Some(states) = states.as_object() else {
                continue;
            };
            for (state, fields) in states {
                let Some(fields) = fields.as_object() else {
                    continue;
                };
                let background = format!("/components/{component}/{state}/background");
                for field in ["foreground", "secondaryText"] {
                    if fields.contains_key(field) {
                        pairs.push((
                            format!("/components/{component}/{state}/{field}"),
                            background.clone(),
                            text_minimum,
                            format!("{component}.{state}.{field}"),
                        ));
                    }
                }
                for field in ["border", "focusRing", "icon", "accent"] {
                    if fields.contains_key(field) {
                        pairs.push((
                            format!("/components/{component}/{state}/{field}"),
                            background.clone(),
                            3.0,
                            format!("{component}.{state}.{field}"),
                        ));
                    }
                }
            }
        }
    }

    pairs
        .into_iter()
        .filter(|(foreground, background, _, _)| {
            resolved.provenance.get(foreground).is_some_and(|origin| origin != "canvas")
                || resolved.provenance.get(background).is_some_and(|origin| origin != "canvas")
        })
        .filter_map(|(foreground, background, minimum, label)| {
            let ratio = color_contrast(tokens, &foreground, &background, canvas)?;
            (ratio < minimum).then(|| {
                ThemeError::warning(
                    "THEME_CONTRAST_WARNING",
                    format!(
                        "{label} contrast against {background} is {ratio:.2}:1; this mode requires at least {minimum:.1}:1"
                    ),
                    foreground,
                )
            })
        })
        .collect()
}
fn resolve_manifest_unenforced(
    m: &ThemeManifest,
    digest: &str,
    mode: &str,
    platform: &str,
    high: bool,
    assets: Vec<AssetMetadata>,
) -> Result<ResolvedTheme, ThemeError> {
    if !matches!(mode, "light" | "dark") {
        return err("THEME_MODE", "mode must be light or dark", "/mode");
    };
    let mut tokens = canvas(mode);
    let mut provenance = BTreeMap::new();
    mark_provenance(&tokens, &mut provenance, "canvas", "");
    merge(
        &mut tokens,
        &m.foundation,
        &mut provenance,
        "foundation",
        "",
    );
    // `components` is a sibling of foundation tokens, not a free-form root
    // overlay. Keeping it nested prevents a package from shadowing a core
    // token group and gives every leaf a stable `/components/...` provenance.
    let base_components = tokens
        .get_mut("components")
        .expect("Canvas always defines component defaults");
    merge(
        base_components,
        &m.components,
        &mut provenance,
        "components",
        "/components",
    );
    merge(
        &mut tokens,
        if mode == "light" { &m.light } else { &m.dark },
        &mut provenance,
        mode,
        "",
    );
    let platform_tokens = match platform {
        "windows" => &m.platform.windows,
        "macos" => &m.platform.macos,
        _ => {
            return err(
                "THEME_PLATFORM",
                "platform must be windows or macos",
                "/platform",
            )
        }
    };
    if platform_tokens.is_object() {
        merge(&mut tokens, platform_tokens, &mut provenance, platform, "");
    }
    if high {
        if let Some(h) = &m.high_contrast {
            merge(
                &mut tokens,
                if mode == "light" { &h.light } else { &h.dark },
                &mut provenance,
                "highContrast",
                "",
            )
        }
    }
    let root = tokens.clone();
    resolve_colors(&mut tokens, &root, &mut Vec::new())?;
    Ok(ResolvedTheme {
        id: m.id.clone(),
        digest: digest.into(),
        mode: mode.into(),
        high_contrast: high,
        tokens,
        provenance,
        asset_slots: resolved_asset_slots(&m.asset_slots, &assets),
        assets,
    })
}
fn resolve_manifest(
    m: &ThemeManifest,
    digest: &str,
    mode: &str,
    platform: &str,
    high: bool,
    assets: Vec<AssetMetadata>,
) -> Result<ResolvedTheme, ThemeError> {
    let mut resolved = resolve_manifest_unenforced(m, digest, mode, platform, high, assets)?;
    if high {
        enforce_high_contrast(&mut resolved.tokens, &mut resolved.provenance);
    }
    Ok(resolved)
}
fn resolve_colors(v: &mut Value, root: &Value, seen: &mut Vec<String>) -> Result<(), ThemeError> {
    match v {
        Value::Object(m) => {
            if m.keys()
                .any(|key| matches!(key.as_str(), "mix" | "contrastColor" | "systemAccent"))
            {
                let resolved = resolve_color(&Value::Object(m.clone()), root, seen)?;
                *v = resolved;
                return Ok(());
            }
            for x in m.values_mut() {
                resolve_colors(x, root, seen)?
            }
        }
        Value::Array(a) => {
            for x in a {
                resolve_colors(x, root, seen)?
            }
        }
        Value::String(value) => {
            if value == "system"
                || value.starts_with('#')
                || value.starts_with("rgba(")
                || value.starts_with("alpha(")
                || value.starts_with("ref:")
            {
                let r = resolve_color(v, root, seen)?;
                *v = r
            }
        }
        _ => {}
    }
    Ok(())
}
fn enforce_high_contrast(tokens: &mut Value, provenance: &mut BTreeMap<String, String>) {
    let canvas = "/colors/background/canvas";
    let background_paths = [
        canvas,
        "/colors/background/surface",
        "/colors/background/input",
        "/colors/background/hover",
        "/colors/background/active",
        "/colors/background/raised",
        "/colors/background/toast",
    ];
    for path in background_paths {
        make_opaque(tokens, provenance, path, canvas);
    }

    for (foreground, background, minimum) in [
        ("/colors/text/primary", "/colors/background/surface", 7.0),
        ("/colors/text/secondary", "/colors/background/surface", 7.0),
        ("/colors/text/tertiary", "/colors/background/surface", 7.0),
        ("/colors/text/toast", "/colors/background/toast", 7.0),
        ("/colors/accent/onAccent", "/colors/accent/default", 7.0),
    ] {
        ensure_contrast(tokens, provenance, foreground, background, canvas, minimum);
    }

    let component_paths = tokens
        .pointer("/components")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|components| components.iter())
        .flat_map(|(component, states)| {
            states.as_object().into_iter().flat_map(move |states| {
                states
                    .keys()
                    .map(move |state| (component.clone(), state.clone()))
            })
        })
        .collect::<Vec<_>>();
    for (component, state) in component_paths {
        let base = format!("/components/{component}/{state}");
        let background = format!("{base}/background");
        make_opaque(tokens, provenance, &background, canvas);
        for field in ["foreground", "secondaryText"] {
            ensure_contrast(
                tokens,
                provenance,
                &format!("{base}/{field}"),
                &background,
                canvas,
                7.0,
            );
        }
        for field in ["border", "focusRing", "icon", "accent"] {
            ensure_contrast(
                tokens,
                provenance,
                &format!("{base}/{field}"),
                &background,
                canvas,
                3.0,
            );
        }
        set_policy_value(
            tokens,
            provenance,
            &format!("{base}/shadow/radius"),
            Value::from(0),
        );
        set_policy_value(
            tokens,
            provenance,
            &format!("{base}/shadow/y"),
            Value::from(0),
        );
        set_policy_value(
            tokens,
            provenance,
            &format!("{base}/shadow/opacity"),
            Value::from(0),
        );
    }
    set_policy_value(tokens, provenance, "/effects/opacity", Value::from(1));
    set_policy_value(tokens, provenance, "/effects/shadow/radius", Value::from(0));
    set_policy_value(tokens, provenance, "/effects/shadow/y", Value::from(0));
    set_policy_value(
        tokens,
        provenance,
        "/effects/shadow/opacity",
        Value::from(0),
    );
}

fn root(base: &Path) -> PathBuf {
    base.join("themes-v2")
}
fn ensure_root(base: &Path) -> Result<PathBuf, ThemeError> {
    let directory = root(base);
    fs::create_dir_all(&directory).map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
    }
    Ok(directory)
}
fn lock_root(base: &Path) -> Result<fs::File, ThemeError> {
    use fs2::FileExt;
    let directory = ensure_root(base)?;
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join(".themes-v2.lock"))
        .map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
    file.lock_exclusive()
        .map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
    Ok(file)
}
fn id_path(id: &str) -> String {
    id.strip_prefix("custom:").unwrap_or(id).replace('.', "_")
}
fn settings_path(base: &Path) -> PathBuf {
    root(base).join("local-settings.json")
}
pub fn get_local_theme_settings_at(base: &Path) -> LocalThemeSettings {
    fs::read(settings_path(base))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(LocalThemeSettings {
            active_theme_id: CANVAS_ID.into(),
            appearance: "system".into(),
            high_contrast: false,
        })
}
pub fn set_local_theme_settings_at(base: &Path, s: LocalThemeSettings) -> Result<(), ThemeError> {
    let _lock = lock_root(base)?;
    set_local_theme_settings_at_unlocked(base, s)
}
/// Outcome of attempting to migrate a legacy (pre-V2) theme selection from
/// `config-v2.json` into `themes-v2/local-settings.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyThemeMigration {
    /// V2 local settings were written from the legacy fields just now.
    Migrated,
    /// A readable `local-settings.json` already existed; it was left untouched.
    AlreadyPresent,
    /// `local-settings.json` exists but is unreadable, so the legacy fields
    /// are kept in `config-v2.json` as the only surviving copy of the choice.
    Preserved,
}

/// Mapping from the pre-V2 built-in `color_theme` values to Theme V2 ids.
/// This table is the single source of truth for the upgrade migration.
pub const LEGACY_BUILTIN_THEME_MAPPING: &[(&str, &str)] = &[
    ("tailsync", CANVAS_ID),
    ("ocean", FLUX_ID),
    ("forest", LEDGER_ID),
    ("rose", AURA_ID),
    ("high-contrast", MONO_ID),
];

fn legacy_builtin_id(color_theme: &str) -> Option<&'static str> {
    LEGACY_BUILTIN_THEME_MAPPING
        .iter()
        .find_map(|(legacy, id)| (*legacy == color_theme).then_some(*id))
}

/// Migrate a legacy theme selection (pre-V2 `theme` / `color_theme` settings
/// fields) into `themes-v2/local-settings.json`.
///
/// Rules:
/// - A readable existing `local-settings.json` is never overwritten: the
///   on-disk V2 selection is authoritative and the config file is simply
///   cleaned of the obsolete fields (caller's job).
/// - A `local-settings.json` that exists but cannot be read is left in place
///   and the legacy fields are kept in the config file, so the user's choice
///   is never lost and the migration is retried on the next start.
/// - Legacy built-in themes map onto their V2 equivalents via
///   [`LEGACY_BUILTIN_THEME_MAPPING`]; legacy custom selections
///   (`custom:<id>`) are **not** converted — they fall back to Canvas with a
///   warning and the old theme files stay untouched in the legacy themes
///   directory.
pub fn migrate_legacy_theme_selection_at(
    base: &Path,
    theme: Option<&str>,
    color_theme: Option<&str>,
) -> Result<LegacyThemeMigration, ThemeError> {
    let local = settings_path(base);
    if local.exists() {
        let readable = fs::read(&local)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<LocalThemeSettings>(&bytes).ok());
        return if readable.is_some() {
            Ok(LegacyThemeMigration::AlreadyPresent)
        } else {
            log::warn!(
                "{} exists but could not be read; keeping the legacy theme fields in config-v2.json and retrying on the next start",
                local.display()
            );
            Ok(LegacyThemeMigration::Preserved)
        };
    }
    let appearance = match theme {
        Some(value @ ("system" | "light" | "dark")) => value.to_string(),
        Some(other) => {
            log::warn!("Unknown legacy theme value {other:?}; falling back to \"system\"");
            "system".to_string()
        }
        None => "system".to_string(),
    };
    let active_theme_id = match color_theme.and_then(legacy_builtin_id) {
        Some(id) => id.to_string(),
        None => match color_theme {
            Some(value) if value.starts_with("custom:") => {
                log::warn!(
                    "Legacy custom theme selection {value:?} cannot be converted to Theme V2; \
                     falling back to {CANVAS_ID}. The old theme file stays in the legacy themes \
                     directory and is not converted automatically."
                );
                CANVAS_ID.to_string()
            }
            Some(value) => {
                log::warn!(
                    "Unknown legacy color_theme value {value:?}; falling back to {CANVAS_ID}"
                );
                CANVAS_ID.to_string()
            }
            None => CANVAS_ID.to_string(),
        },
    };
    set_local_theme_settings_at(
        base,
        LocalThemeSettings {
            active_theme_id,
            appearance,
            high_contrast: false,
        },
    )?;
    Ok(LegacyThemeMigration::Migrated)
}

fn set_local_theme_settings_at_unlocked(
    base: &Path,
    s: LocalThemeSettings,
) -> Result<(), ThemeError> {
    if !valid_id(&s.active_theme_id) {
        return err("THEME_ID", "invalid active theme id", "/activeThemeId");
    };
    if !matches!(s.appearance.as_str(), "light" | "dark" | "system") {
        return err(
            "THEME_APPEARANCE",
            "appearance must be system/light/dark",
            "/appearance",
        );
    };
    if !builtin_ids().contains(&s.active_theme_id.as_str()) {
        // Selection is never a dangling preference: validate the installed
        // package before persisting it, so every process resolves the same
        // local-only active theme or Canvas.
        for platform in ["windows", "macos"] {
            for mode in ["light", "dark"] {
                resolve_theme_at_unlocked(
                    &s.active_theme_id,
                    mode,
                    platform,
                    s.high_contrast,
                    base,
                )?;
            }
        }
    }
    atomic(&settings_path(base), &serde_json::to_vec(&s).unwrap())
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<(), ThemeError> {
    let parent = path
        .parent()
        .ok_or_else(|| ThemeError::new("THEME_IO", "path has no parent", ""))?;
    fs::create_dir_all(parent).map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}-{:x}",
        path.file_name().unwrap().to_string_lossy(),
        std::process::id(),
        rand::random::<u64>()
    ));
    let write = (|| {
        let mut file =
            fs::File::create(&tmp).map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
        file.write_all(bytes)
            .map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
        file.sync_all()
            .map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
        fs::rename(&tmp, path).map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))
    })();
    if write.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    write?;
    Ok(())
}

fn recover_swap(dir: &Path) -> Result<(), ThemeError> {
    let current = dir.join("current.tailsync-theme");
    let rollback = dir.join("rollback.tailsync-theme");
    let old_current = dir.join(".swap-current-backup");
    let old_rollback = dir.join(".swap-rollback-backup");
    // An interrupted promotion either has no current yet (restore it), or a
    // new current and its old bytes safely staged (finish the promotion).
    if old_current.exists() {
        if !current.exists() {
            fs::rename(&old_current, &current)
                .map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
        } else {
            if rollback.exists() && !old_rollback.exists() {
                fs::rename(&rollback, &old_rollback)
                    .map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
            }
            fs::rename(&old_current, &rollback)
                .map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
        }
    }
    if old_rollback.exists() {
        if rollback.exists() {
            fs::remove_file(old_rollback)
                .map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
        } else {
            fs::rename(old_rollback, rollback)
                .map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
        }
    }
    Ok(())
}
fn swap_packages(dir: &Path, candidate: &[u8]) -> Result<(), ThemeError> {
    recover_swap(dir)?;
    let current = dir.join("current.tailsync-theme");
    let rollback = dir.join("rollback.tailsync-theme");
    let old_current = dir.join(".swap-current-backup");
    let old_rollback = dir.join(".swap-rollback-backup");
    let staged = dir.join(".swap-candidate");
    atomic(&staged, candidate)?;
    fs::rename(&current, &old_current)
        .map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
    if rollback.exists() {
        if let Err(error) = fs::rename(&rollback, &old_rollback) {
            // No candidate has been promoted yet; restore current before
            // returning so this failure cannot leave it absent.
            if let Err(restore) = fs::rename(&old_current, &current) {
                return Err(ThemeError::new(
                    "THEME_IO",
                    format!("{error}; additionally could not restore current: {restore}"),
                    "",
                ));
            }
            return Err(ThemeError::new("THEME_IO", error.to_string(), ""));
        }
    }
    if let Err(error) = fs::rename(&staged, &current) {
        // Candidate was never installed; restore the old current immediately.
        let _ = fs::rename(&old_current, &current);
        let _ = fs::rename(&old_rollback, &rollback);
        return Err(ThemeError::new("THEME_IO", error.to_string(), ""));
    }
    // From here recovery can always finish using the two backups.
    recover_swap(dir)
}
pub fn install_theme_at(
    bytes: &[u8],
    expected_digest: &str,
    base: &Path,
) -> Result<ThemeDescriptor, ThemeError> {
    let digest = package_digest(bytes);
    if digest != expected_digest {
        return err(
            "THEME_DIGEST",
            "expected digest does not match package",
            "/expectedDigest",
        );
    };
    let (m, assets) = read_package(bytes)?;
    validate_manifest(&m, &digest, &assets)?;
    validate_all_resolved_modes(&m, &digest, &assets)?;
    let _lock = lock_root(base)?;
    let dir = root(base).join(id_path(&m.id));
    recover_swap(&dir)?;
    let current = dir.join("current.tailsync-theme");
    if current.exists() {
        return err(
            "THEME_ALREADY_INSTALLED",
            "theme is already installed; use update",
            "/id",
        );
    } else {
        fs::create_dir_all(&dir).map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?
    }
    atomic(&current, bytes)?;
    Ok(descriptor(
        &m,
        &digest,
        assets,
        "custom",
        "valid",
        id_path(&m.id),
    ))
}
pub fn update_theme_at(
    bytes: &[u8],
    expected_digest: &str,
    options: UpdateThemeOptions,
    base: &Path,
) -> Result<ThemeDescriptor, ThemeError> {
    let digest = package_digest(bytes);
    if digest != expected_digest {
        return err(
            "THEME_DIGEST",
            "expected digest does not match package",
            "/expectedDigest",
        );
    }
    let (manifest, assets) = read_package(bytes)?;
    validate_manifest(&manifest, &digest, &assets)?;
    validate_all_resolved_modes(&manifest, &digest, &assets)?;
    let _lock = lock_root(base)?;
    let dir = root(base).join(id_path(&manifest.id));
    recover_swap(&dir)?;
    let old = fs::read(dir.join("current.tailsync-theme")).map_err(|_| {
        ThemeError::new(
            "THEME_NOT_FOUND",
            "theme is not installed; use install",
            "/id",
        )
    })?;
    let old_digest = package_digest(&old);
    let (old_manifest, old_assets) = read_package(&old)?;
    validate_manifest(&old_manifest, &old_digest, &old_assets)?;
    if old_manifest.id != manifest.id {
        return err(
            "THEME_ID",
            "updated package id does not match the installed theme",
            "/id",
        );
    }
    let new_version = semver(&manifest.version).expect("validated above");
    let old_version = semver(&old_manifest.version).expect("validated installed package");
    let ordering = compare_semver(&new_version, &old_version);
    if ordering.is_eq() && !options.allow_same_version {
        return err(
            "THEME_VERSION",
            "same-version update requires allowSameVersion",
            "/options/allowSameVersion",
        );
    }
    if ordering.is_lt() && !options.allow_downgrade {
        return err(
            "THEME_VERSION",
            "downgrade requires allowDowngrade",
            "/options/allowDowngrade",
        );
    }
    swap_packages(&dir, bytes)?;
    Ok(descriptor(
        &manifest,
        &digest,
        assets,
        "custom",
        "valid",
        id_path(&manifest.id),
    ))
}
fn descriptor(
    m: &ThemeManifest,
    d: &str,
    a: Vec<AssetMetadata>,
    source: &str,
    status: &str,
    storage_handle: String,
) -> ThemeDescriptor {
    ThemeDescriptor {
        id: m.id.clone(),
        storage_handle,
        source: source.into(),
        format_version: m.format_version,
        version: m.version.clone(),
        digest: d.into(),
        name: m.name.clone(),
        resolved: resolve_manifest(m, d, "light", "windows", false, a.clone()).ok(),
        resolved_light: resolve_manifest(m, d, "light", "windows", false, a.clone()).ok(),
        resolved_dark: resolve_manifest(m, d, "dark", "windows", false, a).ok(),
        diagnostics: vec![],
        status: status.into(),
        platforms: vec!["windows".into(), "macos".into()],
    }
}
pub fn list_themes_v2_at(base: &Path) -> Vec<ThemeDescriptor> {
    let mut out = builtin_ids().into_iter().map(builtin_descriptor).collect();
    let Ok(_lock) = lock_root(base) else {
        return out;
    };
    let Ok(rd) = fs::read_dir(root(base)) else {
        return out;
    };
    for d in rd.flatten() {
        if recover_swap(&d.path()).is_err() {
            continue;
        }
        let p = d.path().join("current.tailsync-theme");
        if !p.is_file() {
            continue;
        }
        let storage_handle = d.file_name().to_string_lossy().into_owned();
        match fs::read(&p).ok().and_then(|b| {
            let dg = package_digest(&b);
            read_package(&b).ok().and_then(|(m, a)| {
                validate_manifest(&m, &dg, &a)
                    .and_then(|_| validate_all_resolved_modes(&m, &dg, &a))
                    .ok()
                    .map(|_| descriptor(&m, &dg, a, "custom", "valid", storage_handle.clone()))
            })
        }) {
            Some(x) => out.push(x),
            None => out.push(ThemeDescriptor {
                id: invalid_descriptor_id(&p, &storage_handle),
                storage_handle,
                source: "custom".into(),
                format_version: FORMAT_VERSION,
                version: "0.0.0".into(),
                digest: String::new(),
                name: BTreeMap::new(),
                resolved: None,
                resolved_light: None,
                resolved_dark: None,
                diagnostics: vec![ThemeError::new(
                    "THEME_INVALID",
                    "installed package cannot be loaded",
                    "",
                )],
                status: "invalid".into(),
                platforms: vec!["windows".into(), "macos".into()],
            }),
        }
    }
    out
}
fn builtin_resolved(id: &str, mode: &str) -> ResolvedTheme {
    let mut tokens = builtin_tokens(id, mode);
    let mut provenance = BTreeMap::new();
    mark_provenance(&tokens, &mut provenance, id, "");
    let root = tokens.clone();
    resolve_colors(&mut tokens, &root, &mut Vec::new())
        .expect("built-in theme color references must resolve");
    ResolvedTheme {
        id: id.into(),
        digest: id.to_string(),
        mode: mode.into(),
        high_contrast: false,
        tokens,
        provenance,
        assets: vec![],
        asset_slots: BTreeMap::new(),
    }
}
fn builtin_descriptor(id: &str) -> ThemeDescriptor {
    ThemeDescriptor {
        id: id.into(),
        storage_handle: id.into(),
        source: "builtin".into(),
        format_version: FORMAT_VERSION,
        version: "1.0.0".into(),
        digest: id.into(),
        name: BTreeMap::from([("en".into(), builtin_name(id).into())]),
        resolved: Some(builtin_resolved(id, "light")),
        resolved_light: Some(builtin_resolved(id, "light")),
        resolved_dark: Some(builtin_resolved(id, "dark")),
        diagnostics: vec![],
        status: "valid".into(),
        platforms: vec!["windows".into(), "macos".into()],
    }
}
fn invalid_descriptor_id(path: &Path, storage_handle: &str) -> String {
    fs::read(path)
        .ok()
        .and_then(|bytes| {
            let mut z = ZipArchive::new(Cursor::new(bytes)).ok()?;
            let mut f = z.by_name("theme.json").ok()?;
            let mut json = Vec::new();
            f.read_to_end(&mut json).ok()?;
            let id = serde_json::from_slice::<Value>(&json)
                .ok()?
                .get("id")?
                .as_str()
                .map(str::to_owned)?;
            (valid_id(&id)
                && !builtin_ids().contains(&id.as_str())
                && id_path(&id) == storage_handle)
                .then_some(id)
        })
        .unwrap_or_else(|| format!("invalid:{storage_handle}"))
}
fn resolve_theme_at_unlocked(
    id: &str,
    mode: &str,
    platform: &str,
    high: bool,
    base: &Path,
) -> Result<ResolvedTheme, ThemeError> {
    if !valid_id(id) {
        return err("THEME_ID", "invalid theme id", "/id");
    }
    if !matches!(mode, "light" | "dark") {
        return err("THEME_MODE", "mode must be light or dark", "/mode");
    }
    if !matches!(platform, "windows" | "macos") {
        return err(
            "THEME_PLATFORM",
            "platform must be windows or macos",
            "/platform",
        );
    }
    if builtin_ids().contains(&id) {
        let mut resolved = builtin_resolved(id, mode);
        if high {
            enforce_high_contrast(&mut resolved.tokens, &mut resolved.provenance);
        }
        resolved.high_contrast = high;
        return Ok(resolved);
    }
    let dir = root(base).join(id_path(id));
    recover_swap(&dir)?;
    let b = fs::read(dir.join("current.tailsync-theme"))
        .map_err(|_| ThemeError::new("THEME_NOT_FOUND", "theme is not installed", "/id"))?;
    let dg = package_digest(&b);
    let (m, a) = read_package(&b)?;
    if m.id != id {
        return err(
            "THEME_ID",
            "installed theme id does not match requested id",
            "/id",
        );
    }
    validate_manifest(&m, &dg, &a)?;
    resolve_manifest(&m, &dg, mode, platform, high, a)
}
pub fn resolve_theme_at(
    id: &str,
    mode: &str,
    platform: &str,
    high: bool,
    base: &Path,
) -> Result<ResolvedTheme, ThemeError> {
    if builtin_ids().contains(&id) {
        return resolve_theme_at_unlocked(id, mode, platform, high, base);
    }
    let _lock = lock_root(base)?;
    resolve_theme_at_unlocked(id, mode, platform, high, base)
}
pub fn get_theme_asset_at(
    id: &str,
    digest: &str,
    key: &str,
    base: &Path,
) -> Result<(String, Vec<u8>), ThemeError> {
    if !valid_id(id) {
        return err("THEME_ID", "invalid theme id", "/id");
    }
    if !key.starts_with("assets/") || !safe_zip_path(key) {
        return err("THEME_ASSET_KEY", "invalid asset key", "/assetKey");
    };
    let dir = root(base).join(id_path(id));
    {
        let _lock = lock_root(base)?;
        recover_swap(&dir)?;
    }
    let b = fs::read(dir.join("current.tailsync-theme"))
        .map_err(|_| ThemeError::new("THEME_NOT_FOUND", "theme is not installed", "/id"))?;
    if package_digest(&b) != digest {
        return err("THEME_DIGEST", "asset digest is stale", "/digest");
    };
    // Re-run the complete package validation on every read.  Listing is only
    // a snapshot; a package may have been replaced after it was installed.
    let (manifest, metadata) = read_package(&b)?;
    if manifest.id != id {
        return err(
            "THEME_ID",
            "installed theme id does not match requested id",
            "/id",
        );
    }
    validate_manifest(&manifest, digest, &metadata)?;
    let expected = metadata
        .iter()
        .find(|asset| asset.key == key)
        .ok_or_else(|| ThemeError::new("THEME_ASSET_NOT_FOUND", "asset not found", "/assetKey"))?;
    let mut z = ZipArchive::new(Cursor::new(b))
        .map_err(|_| ThemeError::new("THEME_ARCHIVE", "invalid package", ""))?;
    let mut f = z
        .by_name(key)
        .map_err(|_| ThemeError::new("THEME_ASSET_NOT_FOUND", "asset not found", "/assetKey"))?;
    let mut v = Vec::new();
    f.read_to_end(&mut v)
        .map_err(|_| ThemeError::new("THEME_IO", "could not read asset", ""))?;
    let mime = if v.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if v.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else {
        return err("THEME_ASSET_MIME", "asset MIME mismatch", "");
    };
    let (width, height) = image_size(&v, mime).ok_or_else(|| {
        ThemeError::new(
            "THEME_ASSET_DIMENSIONS",
            "invalid image dimensions",
            "/assetKey",
        )
    })?;
    if mime != expected.mime_type
        || v.len() as u64 != expected.bytes
        || width != expected.width
        || height != expected.height
    {
        return err(
            "THEME_ASSET_CHANGED",
            "asset does not match validated package metadata",
            "/assetKey",
        );
    }
    Ok((mime.into(), v))
}

/// Resolve a semantic asset slot only after revalidating the installed package.
/// Callers never provide archive paths, so package authors cannot turn an image
/// slot into an arbitrary file read.
pub fn get_theme_asset_slot_at(
    id: &str,
    digest: &str,
    slot: &str,
    base: &Path,
) -> Result<(ThemeAssetDescriptor, Vec<u8>), ThemeError> {
    if !valid_id(id) {
        return err("THEME_ID", "invalid theme id", "/id");
    }
    let dir = root(base).join(id_path(id));
    {
        let _lock = lock_root(base)?;
        recover_swap(&dir)?;
    }
    let b = fs::read(dir.join("current.tailsync-theme"))
        .map_err(|_| ThemeError::new("THEME_NOT_FOUND", "theme is not installed", "/id"))?;
    if package_digest(&b) != digest {
        return err("THEME_DIGEST", "asset digest is stale", "/digest");
    }
    let (manifest, assets) = read_package(&b)?;
    if manifest.id != id {
        return err(
            "THEME_ID",
            "installed theme id does not match requested id",
            "/id",
        );
    }
    validate_manifest(&manifest, digest, &assets)?;
    let key = manifest.asset_slots.get(slot).ok_or_else(|| {
        ThemeError::new(
            "THEME_ASSET_SLOT",
            "asset slot is not declared",
            "/assetSlot",
        )
    })?;
    let descriptor = resolved_asset_slots(&manifest.asset_slots, &assets)
        .remove(slot)
        .ok_or_else(|| {
            ThemeError::new("THEME_ASSET_SLOT", "asset slot is invalid", "/assetSlot")
        })?;
    let (_mime, bytes) = get_theme_asset_at(id, digest, key, base)?;
    Ok((descriptor, bytes))
}

/// Read a semantic asset from an uninstalled package for isolated preview.
/// The archive is fully validated and digest-bound; no package bytes are
/// persisted and callers can only address declared semantic slots.
pub fn get_theme_asset_slot_from_package(
    bytes: &[u8],
    digest: &str,
    slot: &str,
) -> Result<(ThemeAssetDescriptor, Vec<u8>), ThemeError> {
    if package_digest(bytes) != digest {
        return err("THEME_DIGEST", "asset digest is stale", "/digest");
    }
    let (manifest, assets) = read_package(bytes)?;
    validate_manifest(&manifest, digest, &assets)?;
    let key = manifest.asset_slots.get(slot).ok_or_else(|| {
        ThemeError::new(
            "THEME_ASSET_SLOT",
            "asset slot is not declared",
            "/assetSlot",
        )
    })?;
    let descriptor = resolved_asset_slots(&manifest.asset_slots, &assets)
        .remove(slot)
        .ok_or_else(|| {
            ThemeError::new("THEME_ASSET_SLOT", "asset slot is invalid", "/assetSlot")
        })?;
    let mut archive = ZipArchive::new(Cursor::new(bytes.to_vec()))
        .map_err(|_| ThemeError::new("THEME_ARCHIVE", "invalid package", ""))?;
    let mut file = archive
        .by_name(key)
        .map_err(|_| ThemeError::new("THEME_ASSET_NOT_FOUND", "asset not found", "/assetSlot"))?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)
        .map_err(|e| ThemeError::new("THEME_IO", e.to_string(), "/assetSlot"))?;
    if data.len() as u64 != descriptor.bytes
        || blake3::hash(&data).to_hex().to_string() != descriptor.digest
    {
        return err(
            "THEME_ASSET_CHANGED",
            "asset does not match validated package metadata",
            "/assetSlot",
        );
    }
    Ok((descriptor, data))
}

/// Promote the retained rollback package after validating it again.  This is
/// intentionally not an activation operation: local selection stays put.
pub fn rollback_theme_at(id: &str, base: &Path) -> Result<ThemeDescriptor, ThemeError> {
    if !valid_custom_id(id) {
        return err(
            "THEME_ID",
            "only installed custom themes can roll back",
            "/id",
        );
    }
    let _lock = lock_root(base)?;
    let dir = root(base).join(id_path(id));
    recover_swap(&dir)?;
    let current = dir.join("current.tailsync-theme");
    let rollback = dir.join("rollback.tailsync-theme");
    if !current.is_file() {
        return err("THEME_NOT_FOUND", "theme is not installed", "/id");
    }
    let candidate = fs::read(&rollback).map_err(|_| {
        ThemeError::new(
            "THEME_NO_ROLLBACK",
            "no rollback version is retained",
            "/id",
        )
    })?;
    let digest = package_digest(&candidate);
    let (manifest, assets) = read_package(&candidate)?;
    validate_manifest(&manifest, &digest, &assets)?;
    validate_all_resolved_modes(&manifest, &digest, &assets)?;
    if manifest.id != id {
        return err(
            "THEME_ID",
            "rollback package id does not match the installed theme",
            "/id",
        );
    }
    swap_packages(&dir, &candidate)?;
    Ok(descriptor(
        &manifest,
        &digest,
        assets,
        "custom",
        "valid",
        id_path(id),
    ))
}

/// Deleting an active custom theme removes its bytes before committing the
/// Canvas fallback, so a failed delete cannot change the active preference.
pub fn delete_theme_at(id: &str, base: &Path) -> Result<(), ThemeError> {
    if !valid_id(id) || builtin_ids().contains(&id) {
        return err(
            "THEME_ID",
            "only installed custom themes can be deleted",
            "/id",
        );
    }
    delete_theme_by_handle_at(&id_path(id), base, Some(id))
}
/// Delete only a directory named by a descriptor's storage handle.  Handles
/// are constrained to a direct child of themes-v2, never caller paths.
pub fn delete_theme_by_handle_at(
    handle: &str,
    base: &Path,
    expected_id: Option<&str>,
) -> Result<(), ThemeError> {
    delete_theme_by_handle_with_remover_at(handle, base, expected_id, |path| {
        fs::remove_dir_all(path)
    })
}
fn delete_theme_by_handle_with_remover_at<F>(
    handle: &str,
    base: &Path,
    expected_id: Option<&str>,
    remover: F,
) -> Result<(), ThemeError>
where
    F: FnOnce(&Path) -> std::io::Result<()>,
{
    if handle.is_empty()
        || handle.len() > 128
        || !handle
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return err("THEME_ID", "invalid theme storage handle", "/storageHandle");
    }
    let expected_custom_id = match expected_id {
        Some(id) if valid_id(id) && !builtin_ids().contains(&id) && id_path(id) == handle => {
            Some(id)
        }
        Some(id) if id == format!("invalid:{handle}") => None,
        Some(_) => return err("THEME_ID", "theme id does not match storage handle", "/id"),
        None => return err("THEME_ID", "theme id is required for deletion", "/id"),
    };
    let _lock = lock_root(base)?;
    let dir = root(base).join(handle);
    if !dir.is_dir() {
        return err("THEME_NOT_FOUND", "theme is not installed", "/id");
    }
    let original_settings = get_local_theme_settings_at(base);
    if expected_custom_id.is_none() {
        if let Ok(package) = fs::read(dir.join("current.tailsync-theme")) {
            let digest = package_digest(&package);
            if let Ok((manifest, assets)) = read_package(&package) {
                if id_path(&manifest.id) == handle
                    && validate_manifest(&manifest, &digest, &assets).is_ok()
                    && validate_all_resolved_modes(&manifest, &digest, &assets).is_ok()
                {
                    return err(
                        "THEME_ID",
                        "valid theme must be deleted using its manifest id",
                        "/id",
                    );
                }
            }
        }
    }
    if let Some(id) = expected_custom_id {
        // Invalid packages intentionally remain removable. Bind an intact
        // manifest to its handle when possible, but never require a damaged
        // package to pass validation before its storage can be discarded.
        if let Ok(package) = fs::read(dir.join("current.tailsync-theme")) {
            if let Ok((manifest, _)) = read_package(&package) {
                if manifest.id != id {
                    return err("THEME_ID", "theme id does not match storage handle", "/id");
                }
            }
        }
    }
    let active = expected_custom_id.is_some_and(|id| original_settings.active_theme_id == id)
        || (expected_custom_id.is_none()
            && valid_id(&original_settings.active_theme_id)
            && !builtin_ids().contains(&original_settings.active_theme_id.as_str())
            && id_path(&original_settings.active_theme_id) == handle);
    if active {
        let mut canvas = original_settings.clone();
        canvas.active_theme_id = CANVAS_ID.into();
        // Commit the fallback before deletion. If deletion fails below, the
        // original selection is restored while the package still exists.
        set_local_theme_settings_at_unlocked(base, canvas)?;
    }
    if let Err(error) = remover(&dir) {
        if active {
            let _ = atomic(
                &settings_path(base),
                &serde_json::to_vec(&original_settings).unwrap(),
            );
        }
        return Err(ThemeError::new("THEME_IO", error.to_string(), ""));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn package(manifest: Value) -> Vec<u8> {
        let c = Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(c);
        use zip::write::SimpleFileOptions;
        z.start_file("theme.json", SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut z, serde_json::to_string(&manifest).unwrap().as_bytes())
            .unwrap();
        z.finish().unwrap().into_inner()
    }
    fn package_with_asset(manifest: Value, key: &str, bytes: &[u8]) -> Vec<u8> {
        let c = Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(c);
        use zip::write::SimpleFileOptions;
        z.start_file("theme.json", SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut z, serde_json::to_string(&manifest).unwrap().as_bytes())
            .unwrap();
        z.start_file(key, SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut z, bytes).unwrap();
        z.finish().unwrap().into_inner()
    }
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }
    fn manifest() -> Value {
        serde_json::json!({"formatVersion":2,"id":"custom:studio.night","version":"1.0.0","minCoreVersion":"2.1.0","name":{"en":"Night"},"extends":"builtin:canvas@1","light":{"colors":{"background":{"canvas":"#ffffff"},"text":{"primary":"#111111"}}},"dark":{"colors":{"background":{"canvas":"#111111"},"text":{"primary":"#ffffff"}}}})
    }
    fn diagnostic(manifest: Value) -> ThemeError {
        validate_theme(&package(manifest), "light", false)
            .diagnostics
            .into_iter()
            .next()
            .expect("invalid manifest must have a diagnostic")
    }
    #[test]
    fn validates_and_resolves() {
        let b = package(manifest());
        let v = validate_theme(&b, "light", false);
        assert!(v.valid);
        assert_eq!(v.candidate_version.as_deref(), Some("1.0.0"));
        assert_eq!(
            v.preview
                .unwrap()
                .tokens
                .pointer("/colors/background/canvas")
                .unwrap(),
            "#ffffff"
        );
    }

    #[test]
    fn custom_packages_cannot_claim_any_builtin_id() {
        for id in builtin_ids() {
            let mut candidate = manifest();
            candidate["id"] = Value::from(id);
            let error = diagnostic(candidate);
            assert_eq!(error.code, "THEME_ID", "reserved id {id} was accepted");
            assert_eq!(error.json_pointer, "/id");
        }

        let mut candidate = manifest();
        candidate["id"] = Value::from("studio.night");
        assert_eq!(diagnostic(candidate).code, "THEME_ID");
    }

    #[test]
    fn rollback_rejects_packages_claiming_every_builtin_id() {
        let base = std::env::temp_dir().join(format!("themes-v2-{}", rand::random::<u64>()));
        let current = package(manifest());
        install_theme_at(&current, &package_digest(&current), &base).unwrap();
        let dir = root(&base).join(id_path("custom:studio.night"));

        for id in builtin_ids() {
            let error = rollback_theme_at(id, &base).unwrap_err();
            assert_eq!(error.code, "THEME_ID", "rollback accepted reserved id {id}");

            let mut candidate = manifest();
            candidate["id"] = Value::from(id);
            fs::write(dir.join("rollback.tailsync-theme"), package(candidate)).unwrap();
            let error = rollback_theme_at("custom:studio.night", &base).unwrap_err();
            assert_eq!(error.code, "THEME_ID", "rollback accepted reserved id {id}");
            assert_eq!(
                fs::read(dir.join("current.tailsync-theme")).unwrap(),
                current
            );
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn validation_reports_low_contrast_without_rejecting_the_package() {
        let mut m = manifest();
        m["light"]["colors"]["background"] =
            serde_json::json!({"canvas": "#ffffff", "surface": "#ffffff"});
        m["light"]["colors"]["text"] = serde_json::json!({"primary": "rgba(0, 0, 0, 0.1)"});
        m["light"]["colors"]["accent"] =
            serde_json::json!({"default": "#ffffff", "onAccent": "#eeeeee"});
        m["light"]["components"] = serde_json::json!({
            "search": {"focus": {
                "background": "#ffffff",
                "foreground": "#eeeeee",
                "focusRing": "#eeeeee"
            }}
        });
        let validation = validate_theme(&package(m), "light", false);
        assert!(validation.valid, "{:?}", validation.diagnostics);
        for pointer in [
            "/colors/text/primary",
            "/colors/accent/onAccent",
            "/components/search/focus/foreground",
            "/components/search/focus/focusRing",
        ] {
            assert!(validation.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "THEME_CONTRAST_WARNING"
                    && diagnostic.severity == "warning"
                    && diagnostic.json_pointer == pointer
            }));
        }
    }

    #[test]
    fn high_contrast_validation_uses_the_stricter_ratio() {
        let mut m = manifest();
        m["light"]["colors"]["background"] =
            serde_json::json!({"canvas": "#ffffff", "surface": "#ffffff"});
        m["light"]["colors"]["text"] = serde_json::json!({"primary": "#666666"});
        let standard = validate_theme(&package(m.clone()), "light", false);
        assert!(!standard
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.json_pointer == "/colors/text/primary"));
        let high = validate_theme(&package(m), "light", true);
        assert!(high.valid);
        let diagnostic = high
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.json_pointer == "/colors/text/primary")
            .expect("author contrast warning must survive system correction");
        assert_eq!(diagnostic.severity, "warning");
        assert!(diagnostic.fallback_applied);
        assert_eq!(diagnostic.platforms, ["windows"]);
        let tokens = &high.preview.unwrap().tokens;
        assert_eq!(
            tokens.pointer("/colors/background/surface").unwrap(),
            "#ffffff"
        );
        assert_eq!(tokens.pointer("/colors/text/primary").unwrap(), "#000000");
    }
    #[test]
    fn rejects_v1_and_unsafe() {
        let mut m = manifest();
        m["formatVersion"] = Value::from(1);
        assert!(!validate_theme(&package(m), "light", false).valid);
        let mut m = manifest();
        m["light"]["colors"]["background"]["canvas"] = Value::from("url(https://bad)");
        assert!(!validate_theme(&package(m), "light", false).valid);
    }
    #[test]
    fn install_requires_digest_and_retains_rollback() {
        let base = std::env::temp_dir().join(format!("themes-v2-{}", rand::random::<u64>()));
        let b = package(manifest());
        let d = package_digest(&b);
        install_theme_at(&b, &d, &base).unwrap();
        let mut m = manifest();
        m["version"] = Value::from("1.1.0");
        let b2 = package(m);
        update_theme_at(
            &b2,
            &package_digest(&b2),
            UpdateThemeOptions::default(),
            &base,
        )
        .unwrap();
        assert!(root(&base)
            .join(id_path("custom:studio.night"))
            .join("rollback.tailsync-theme")
            .exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn install_is_first_install_and_update_requires_explicit_version_confirmation() {
        let base = std::env::temp_dir().join(format!("themes-v2-{}", rand::random::<u64>()));
        let one = package(manifest());
        install_theme_at(&one, &package_digest(&one), &base).unwrap();
        assert_eq!(
            install_theme_at(&one, &package_digest(&one), &base)
                .unwrap_err()
                .code,
            "THEME_ALREADY_INSTALLED"
        );
        assert_eq!(
            update_theme_at(
                &one,
                &package_digest(&one),
                UpdateThemeOptions::default(),
                &base
            )
            .unwrap_err()
            .code,
            "THEME_VERSION"
        );
        update_theme_at(
            &one,
            &package_digest(&one),
            UpdateThemeOptions {
                allow_same_version: true,
                allow_downgrade: false,
            },
            &base,
        )
        .unwrap();
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn rollback_rejects_invalid_candidate_and_missing_rollback_without_losing_current() {
        let base = std::env::temp_dir().join(format!("themes-v2-{}", rand::random::<u64>()));
        let one = package(manifest());
        install_theme_at(&one, &package_digest(&one), &base).unwrap();
        assert_eq!(
            rollback_theme_at("custom:studio.night", &base)
                .unwrap_err()
                .code,
            "THEME_NO_ROLLBACK"
        );
        let dir = root(&base).join(id_path("custom:studio.night"));
        fs::write(dir.join("rollback.tailsync-theme"), b"not a package").unwrap();
        assert_eq!(
            rollback_theme_at("custom:studio.night", &base)
                .unwrap_err()
                .code,
            "THEME_ARCHIVE"
        );
        assert_eq!(fs::read(dir.join("current.tailsync-theme")).unwrap(), one);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn rollback_swaps_versions_and_interrupted_swap_recovers() {
        let base = std::env::temp_dir().join(format!("themes-v2-{}", rand::random::<u64>()));
        let one = package(manifest());
        install_theme_at(&one, &package_digest(&one), &base).unwrap();
        let mut newer_manifest = manifest();
        newer_manifest["version"] = Value::from("1.1.0");
        let two = package(newer_manifest);
        update_theme_at(
            &two,
            &package_digest(&two),
            UpdateThemeOptions::default(),
            &base,
        )
        .unwrap();
        rollback_theme_at("custom:studio.night", &base).unwrap();
        let dir = root(&base).join(id_path("custom:studio.night"));
        assert_eq!(fs::read(dir.join("current.tailsync-theme")).unwrap(), one);
        assert_eq!(fs::read(dir.join("rollback.tailsync-theme")).unwrap(), two);
        // Simulate an interruption after candidate promotion but before its
        // former current was committed as rollback.
        fs::rename(
            dir.join("current.tailsync-theme"),
            dir.join(".swap-current-backup"),
        )
        .unwrap();
        fs::write(dir.join("current.tailsync-theme"), &two).unwrap();
        recover_swap(&dir).unwrap();
        assert_eq!(fs::read(dir.join("current.tailsync-theme")).unwrap(), two);
        assert_eq!(fs::read(dir.join("rollback.tailsync-theme")).unwrap(), one);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn list_and_resolve_recover_an_interrupted_swap_without_losing_rollback() {
        let base = std::env::temp_dir().join(format!("themes-v2-{}", rand::random::<u64>()));
        let one = package(manifest());
        install_theme_at(&one, &package_digest(&one), &base).unwrap();
        let mut newer_manifest = manifest();
        newer_manifest["version"] = Value::from("1.1.0");
        let two = package(newer_manifest);
        update_theme_at(
            &two,
            &package_digest(&two),
            UpdateThemeOptions::default(),
            &base,
        )
        .unwrap();
        let dir = root(&base).join("studio_night");

        fs::rename(
            dir.join("current.tailsync-theme"),
            dir.join(".swap-current-backup"),
        )
        .unwrap();
        fs::rename(
            dir.join("rollback.tailsync-theme"),
            dir.join(".swap-rollback-backup"),
        )
        .unwrap();
        let listed = list_themes_v2_at(&base);
        assert!(listed.iter().any(|theme| theme.id == "custom:studio.night"));
        assert_eq!(fs::read(dir.join("current.tailsync-theme")).unwrap(), two);
        assert_eq!(fs::read(dir.join("rollback.tailsync-theme")).unwrap(), one);

        fs::rename(
            dir.join("current.tailsync-theme"),
            dir.join(".swap-current-backup"),
        )
        .unwrap();
        fs::rename(
            dir.join("rollback.tailsync-theme"),
            dir.join(".swap-rollback-backup"),
        )
        .unwrap();
        let resolved =
            resolve_theme_at("custom:studio.night", "light", "windows", false, &base).unwrap();
        assert_eq!(resolved.id, "custom:studio.night");
        assert_eq!(fs::read(dir.join("current.tailsync-theme")).unwrap(), two);
        assert_eq!(fs::read(dir.join("rollback.tailsync-theme")).unwrap(), one);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn invalid_listing_keeps_storage_handle_and_manifest_id() {
        let base = std::env::temp_dir().join(format!("themes-v2-{}", rand::random::<u64>()));
        let dir = root(&base).join("studio_night");
        fs::create_dir_all(&dir).unwrap();
        let mut bad = manifest();
        bad["version"] = Value::from("bad");
        fs::write(dir.join("current.tailsync-theme"), package(bad)).unwrap();
        let listed = list_themes_v2_at(&base);
        let broken = listed
            .iter()
            .find(|theme| theme.status == "invalid")
            .unwrap();
        assert_eq!(broken.id, "custom:studio.night");
        assert_eq!(broken.storage_handle, "studio_night");
        delete_theme_by_handle_at(&broken.storage_handle, &base, Some(&broken.id)).unwrap();
        assert!(!dir.exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn failed_active_delete_keeps_the_original_setting_and_files() {
        let base = std::env::temp_dir().join(format!("themes-v2-{}", rand::random::<u64>()));
        let bytes = package(manifest());
        install_theme_at(&bytes, &package_digest(&bytes), &base).unwrap();
        set_local_theme_settings_at(
            &base,
            LocalThemeSettings {
                active_theme_id: "custom:studio.night".into(),
                appearance: "system".into(),
                high_contrast: false,
            },
        )
        .unwrap();
        let error = delete_theme_by_handle_with_remover_at(
            "studio_night",
            &base,
            Some("custom:studio.night"),
            |_| Err(std::io::Error::other("simulated delete failure")),
        )
        .unwrap_err();
        assert_eq!(error.code, "THEME_IO");
        assert_eq!(
            get_local_theme_settings_at(&base).active_theme_id,
            "custom:studio.night"
        );
        assert!(root(&base)
            .join("studio_night/current.tailsync-theme")
            .is_file());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn active_delete_by_handle_commits_canvas_atomically() {
        let base = std::env::temp_dir().join(format!("themes-v2-{}", rand::random::<u64>()));
        let bytes = package(manifest());
        install_theme_at(&bytes, &package_digest(&bytes), &base).unwrap();
        set_local_theme_settings_at(
            &base,
            LocalThemeSettings {
                active_theme_id: "custom:studio.night".into(),
                appearance: "dark".into(),
                high_contrast: true,
            },
        )
        .unwrap();
        delete_theme_by_handle_at("studio_night", &base, Some("custom:studio.night")).unwrap();
        let settings = get_local_theme_settings_at(&base);
        assert_eq!(settings.active_theme_id, CANVAS_ID);
        assert_eq!(settings.appearance, "dark");
        assert!(settings.high_contrast);
        assert!(!root(&base).join("studio_night").exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn delete_binds_custom_and_invalid_ids_to_the_storage_handle() {
        let base = std::env::temp_dir().join(format!("themes-v2-{}", rand::random::<u64>()));
        let studio = package(manifest());
        install_theme_at(&studio, &package_digest(&studio), &base).unwrap();
        assert_eq!(
            delete_theme_by_handle_at("studio_night", &base, Some("invalid:studio_night"),)
                .unwrap_err()
                .code,
            "THEME_ID"
        );
        assert!(root(&base)
            .join("studio_night/current.tailsync-theme")
            .is_file());

        let other_dir = root(&base).join("other_theme");
        fs::create_dir_all(&other_dir).unwrap();
        fs::write(other_dir.join("current.tailsync-theme"), b"damaged package").unwrap();

        assert_eq!(
            delete_theme_by_handle_at("other_theme", &base, Some("custom:studio.night"))
                .unwrap_err()
                .code,
            "THEME_ID"
        );
        assert!(other_dir.is_dir());
        assert_eq!(
            delete_theme_at("other_theme", &base).unwrap_err().code,
            "THEME_ID"
        );

        let broken = list_themes_v2_at(&base)
            .into_iter()
            .find(|theme| theme.storage_handle == "other_theme")
            .unwrap();
        assert_eq!(broken.id, "invalid:other_theme");
        delete_theme_by_handle_at(&broken.storage_handle, &base, Some(&broken.id)).unwrap();
        assert!(!other_dir.exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn install_and_update_require_every_platform_mode_to_resolve() {
        let base = std::env::temp_dir().join(format!("themes-v2-{}", rand::random::<u64>()));
        let mut broken_install = manifest();
        broken_install["platform"] = serde_json::json!({
            "macos": {"colors": {"accent": {"default": "ref:/missing/macos/color"}}}
        });
        let bytes = package(broken_install);
        assert_eq!(
            install_theme_at(&bytes, &package_digest(&bytes), &base)
                .unwrap_err()
                .code,
            "THEME_COLOR_REFERENCE"
        );
        assert!(!root(&base).join("studio_night").exists());

        let current = package(manifest());
        install_theme_at(&current, &package_digest(&current), &base).unwrap();
        let mut broken_update = manifest();
        broken_update["version"] = Value::from("1.1.0");
        broken_update["highContrast"] = serde_json::json!({
            "light": {},
            "dark": {"colors": {"text": {"primary": "ref:/missing/high/dark"}}}
        });
        let update = package(broken_update);
        assert_eq!(
            update_theme_at(
                &update,
                &package_digest(&update),
                UpdateThemeOptions::default(),
                &base
            )
            .unwrap_err()
            .code,
            "THEME_COLOR_REFERENCE"
        );
        assert_eq!(
            fs::read(root(&base).join("studio_night/current.tailsync-theme")).unwrap(),
            current
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn rollback_rejects_a_valid_package_for_another_theme_id() {
        let base = std::env::temp_dir().join(format!("themes-v2-{}", rand::random::<u64>()));
        let current = package(manifest());
        install_theme_at(&current, &package_digest(&current), &base).unwrap();
        let dir = root(&base).join("studio_night");
        let mut other = manifest();
        other["id"] = Value::from("custom:other.theme");
        fs::write(dir.join("rollback.tailsync-theme"), package(other)).unwrap();

        assert_eq!(
            rollback_theme_at("custom:studio.night", &base)
                .unwrap_err()
                .code,
            "THEME_ID"
        );
        assert_eq!(
            fs::read(dir.join("current.tailsync-theme")).unwrap(),
            current
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn platform_tokens_apply_after_mode_tokens() {
        let mut m = manifest();
        m["light"]["shape"] = serde_json::json!({ "controlRadius": 3 });
        m["platform"] = serde_json::json!({ "windows": { "shape": { "controlRadius": 17 } }, "macos": { "shape": { "controlRadius": 5 } } });
        let b = package(m);
        let digest = package_digest(&b);
        let (manifest, assets) = read_package(&b).unwrap();
        let windows = resolve_manifest(
            &manifest,
            &digest,
            "light",
            "windows",
            false,
            assets.clone(),
        )
        .unwrap();
        let macos = resolve_manifest(&manifest, &digest, "light", "macos", false, assets).unwrap();
        assert_eq!(windows.tokens.pointer("/shape/controlRadius").unwrap(), 17);
        assert_eq!(macos.tokens.pointer("/shape/controlRadius").unwrap(), 5);
    }

    #[test]
    fn accepts_every_supported_token_field() {
        let mut m = manifest();
        m["foundation"] = serde_json::json!({
            "colors": {"background": {"surface": "#202124"}, "text": {"secondary": "#a1a1aa"}, "accent": {"default": "#3b82f6", "soft": "alpha(#3b82f6, 0.2)"}},
            "typography": {"ui": {"families": ["system-ui", "sans-serif"], "size": 14, "lineHeight": 20}, "search": {"size": 14}},
            "density": {"control": 8, "row": 12},
            "shape": {"controlRadius": 8, "surfaceRadius": 10},
            "effects": {"opacity": 0.75, "motion": {"fast": 160, "slow": 240, "easing": "standard"}}
        });
        assert!(validate_theme(&package(m), "dark", false).valid);
    }

    #[test]
    fn rejects_unknown_token_with_escaped_pointer() {
        let mut m = manifest();
        m["light"]["colors"]["unknown/token"] = Value::from("#fff");
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_TOKEN_UNKNOWN");
        assert_eq!(error.json_pointer, "/light/colors/unknown~1token");
    }

    #[test]
    fn rejects_token_wrong_type_at_exact_pointer() {
        let mut m = manifest();
        m["light"]["shape"] = serde_json::json!({"controlRadius": "8"});
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_TOKEN_TYPE");
        assert_eq!(error.json_pointer, "/light/shape/controlRadius");
    }

    #[test]
    fn rejects_empty_or_non_string_font_families() {
        let mut m = manifest();
        m["dark"]["typography"] = serde_json::json!({"ui": {"families": []}});
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_TOKEN_RANGE");
        assert_eq!(error.json_pointer, "/dark/typography/ui/families");

        let mut m = manifest();
        m["dark"]["typography"] = serde_json::json!({"ui": {"families": [5]}});
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_TOKEN_TYPE");
        assert_eq!(error.json_pointer, "/dark/typography/ui/families/0");
    }

    #[test]
    fn rejects_out_of_range_numeric_tokens() {
        let mut m = manifest();
        m["light"]["typography"] = serde_json::json!({"search": {"size": 257}});
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_TOKEN_RANGE");
        assert_eq!(error.json_pointer, "/light/typography/search/size");

        let mut m = manifest();
        m["dark"]["effects"] = serde_json::json!({"opacity": 1.1});
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_TOKEN_RANGE");
        assert_eq!(error.json_pointer, "/dark/effects/opacity");
    }

    #[test]
    fn rejects_unsupported_easing() {
        let mut m = manifest();
        m["foundation"] =
            serde_json::json!({"effects": {"motion": {"fast": 20, "easing": "spring"}}});
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_TOKEN_VALUE");
        assert_eq!(error.json_pointer, "/foundation/effects/motion/easing");
    }

    #[test]
    fn components_merge_from_canvas_and_preserve_provenance() {
        let mut m = manifest();
        m["components"] = serde_json::json!({
            "search": {"focus": {"background": "#123456", "padding": 14}},
            "history": {"selected": {"background": "#234567"}},
            "button": {"default": {"accent": "#345678"}}
        });
        m["platform"] = serde_json::json!({
            "windows": {"components": {"button": {"hover": {"radius": 4}}}},
            "macos": {"components": {"search": {"focus": {"padding": 16}}}}
        });
        m["highContrast"] = serde_json::json!({
            "light": {"components": {"history": {"selected": {"foreground": "#ffffff"}}}},
            "dark": {}
        });
        let package = package(m);
        let resolved = validate_theme(&package, "light", true).preview.unwrap();
        assert_eq!(
            resolved
                .tokens
                .pointer("/components/search/focus/background")
                .unwrap(),
            "#123456"
        );
        assert_eq!(
            resolved
                .tokens
                .pointer("/components/history/selected/foreground")
                .unwrap(),
            "#ffffff"
        );
        assert_eq!(
            resolved
                .tokens
                .pointer("/components/button/hover/radius")
                .unwrap(),
            4
        );
        assert_eq!(
            resolved.provenance["/components/search/focus/background"],
            "components"
        );
        assert_eq!(
            resolved.provenance["/components/history/selected/foreground"],
            "highContrast"
        );
        assert_eq!(
            resolved.provenance["/components/button/hover/radius"],
            "windows"
        );
        assert_eq!(
            resolved
                .tokens
                .pointer("/components/panel/default/background")
                .unwrap(),
            resolved
                .tokens
                .pointer("/colors/background/surface")
                .unwrap()
        );
    }

    #[test]
    fn components_reject_unknown_fields_states_and_invalid_values() {
        let mut m = manifest();
        m["components"] = serde_json::json!({"unknown": {}});
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_TOKEN_UNKNOWN");
        assert_eq!(error.json_pointer, "/components/unknown");

        let mut m = manifest();
        m["components"] = serde_json::json!({"button": {"pressed": {}}});
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_TOKEN_UNKNOWN");
        assert_eq!(error.json_pointer, "/components/button/pressed");

        let mut m = manifest();
        m["components"] = serde_json::json!({"button": {"hover": {"css": "color:red"}}});
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_TOKEN_UNKNOWN");
        assert_eq!(error.json_pointer, "/components/button/hover/css");

        let mut m = manifest();
        m["components"] = serde_json::json!({"button": {"hover": {"padding": 129}}});
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_TOKEN_RANGE");
        assert_eq!(error.json_pointer, "/components/button/hover/padding");
    }

    #[test]
    fn structured_color_expressions_are_deterministic_and_checked() {
        let mut m = manifest();
        m["light"]["colors"]["accent"]["default"] = serde_json::json!({
            "mix": {"base": "#000000", "with": "#ffffff", "amount": 0.25}
        });
        m["light"]["colors"]["accent"]["onAccent"] = serde_json::json!({
            "contrastColor": {"background": "ref:/colors/accent/default", "light": "#ffffff", "dark": "#000000"}
        });
        m["dark"]["colors"]["accent"]["default"] = serde_json::json!({"systemAccent": true});
        let value = validate_theme(&package(m), "light", false).preview.unwrap();
        assert_eq!(
            value.tokens.pointer("/colors/accent/default").unwrap(),
            "#404040"
        );
        assert_eq!(
            value.tokens.pointer("/colors/accent/onAccent").unwrap(),
            "#ffffff"
        );

        let mut m = manifest();
        m["light"]["colors"]["accent"]["default"] = serde_json::json!({
            "mix": {"base": "ref:/colors/accent/default", "with": "#fff", "amount": 0.5}
        });
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_COLOR_CYCLE");
    }

    #[test]
    fn semantic_asset_slots_are_package_scoped_and_digest_bound() {
        let mut m = manifest();
        m["assetSlots"] =
            serde_json::json!({"logo": "assets/logo.png", "emptyState": "assets/empty.png"});
        let logo = png(16, 16);
        let empty = png(32, 20);
        let package = package_with_asset(m.clone(), "assets/logo.png", &logo);
        let error = validate_theme(&package, "light", false)
            .diagnostics
            .remove(0);
        assert_eq!(error.code, "THEME_ASSET_SLOT");
        assert_eq!(error.json_pointer, "/assetSlots/emptyState");

        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        use zip::write::SimpleFileOptions;
        archive
            .start_file("theme.json", SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut archive, serde_json::to_string(&m).unwrap().as_bytes())
            .unwrap();
        archive
            .start_file("assets/logo.png", SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut archive, &logo).unwrap();
        archive
            .start_file("assets/empty.png", SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut archive, &empty).unwrap();
        let package = archive.finish().unwrap().into_inner();
        let validation = validate_theme(&package, "light", false);
        assert!(validation.valid);
        let resolved = validation.preview.unwrap();
        let descriptor = &resolved.asset_slots["logo"];
        assert_eq!(descriptor.key, "assets/logo.png");
        assert_eq!(descriptor.width, 16);
        assert_eq!(descriptor.digest, blake3::hash(&logo).to_hex().to_string());
        let (preview_asset, preview_bytes) =
            get_theme_asset_slot_from_package(&package, &package_digest(&package), "logo").unwrap();
        assert_eq!(preview_asset.key, "assets/logo.png");
        assert_eq!(preview_bytes, logo);
        assert_eq!(
            get_theme_asset_slot_from_package(&package, "stale", "logo")
                .unwrap_err()
                .code,
            "THEME_DIGEST"
        );

        let base = std::env::temp_dir().join(format!("theme-assets-{}", rand::random::<u64>()));
        install_theme_at(&package, &package_digest(&package), &base).unwrap();
        let (asset, bytes) = get_theme_asset_slot_at(
            "custom:studio.night",
            &package_digest(&package),
            "logo",
            &base,
        )
        .unwrap();
        assert_eq!(asset.key, "assets/logo.png");
        assert_eq!(bytes, logo);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn platform_and_high_contrast_use_the_same_schema() {
        let mut m = manifest();
        m["platform"] = serde_json::json!({"windows": {"density": {"row": 18}}, "macos": {"shape": {"surfaceRadius": 9}}});
        m["highContrast"] = serde_json::json!({"light": {"effects": {"opacity": 1}}, "dark": {"colors": {"accent": {"soft": "#ffffff"}}}});
        assert!(validate_theme(&package(m), "light", true).valid);

        let mut m = manifest();
        m["platform"] = serde_json::json!({"windows": {"bogus": 1}});
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_TOKEN_UNKNOWN");
        assert_eq!(error.json_pointer, "/platform/windows/bogus");

        let mut m = manifest();
        m["highContrast"] =
            serde_json::json!({"light": {"effects": {"opacity": "full"}}, "dark": {}});
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_TOKEN_TYPE");
        assert_eq!(error.json_pointer, "/highContrast/light/effects/opacity");
    }

    #[test]
    fn strict_semver_rejects_extra_segments_and_invalid_identifiers() {
        for version in ["1.0", "1.0.0.1", "1.0.0-01", "1.0.0+", "v1.0.0"] {
            let mut m = manifest();
            m["version"] = Value::from(version);
            let error = diagnostic(m);
            assert_eq!(error.code, "THEME_VERSION", "{version}");
            assert_eq!(error.json_pointer, "/version");
        }
        let mut m = manifest();
        m["version"] = Value::from("1.0.0-beta.1+build.5");
        assert!(validate_theme(&package(m), "light", false).valid);

        let huge = semver("1.0.0-999999999999999999999999999999999999").unwrap();
        let small = semver("1.0.0-2").unwrap();
        assert!(compare_semver(&huge, &small).is_gt());
    }

    #[test]
    fn eight_digit_hex_validates_every_channel() {
        let mut invalid = manifest();
        invalid["light"]["colors"]["background"]["canvas"] = Value::from("#112233zz");
        let validation = validate_theme(&package(invalid), "light", false);
        assert!(!validation.valid);
        assert_eq!(validation.diagnostics[0].code, "THEME_COLOR_EXPRESSION");

        let mut valid = manifest();
        valid["light"]["colors"]["background"]["canvas"] = Value::from("#11223380");
        assert!(validate_theme(&package(valid), "light", false).valid);
    }

    #[test]
    fn min_core_version_must_be_compatible() {
        let mut m = manifest();
        m["minCoreVersion"] = Value::from("2.1.1");
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_MIN_CORE_VERSION");
        assert_eq!(error.json_pointer, "/minCoreVersion");

        let mut m = manifest();
        m["minCoreVersion"] = Value::from("2.1.0-rc.1");
        assert!(validate_theme(&package(m), "light", false).valid);
    }

    #[test]
    fn capabilities_fail_closed_but_accept_the_supported_whitelist() {
        let mut m = manifest();
        m["requiredCapabilities"] = serde_json::json!(["not-implemented"]);
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_CAPABILITY_UNSUPPORTED");
        assert_eq!(error.json_pointer, "/requiredCapabilities/0");

        let mut m = manifest();
        m["requiredCapabilities"] =
            serde_json::json!(["theme-v2", "platform-overrides", "high-contrast"]);
        assert!(validate_theme(&package(m), "light", false).valid);
    }

    #[test]
    fn builtins_are_stable_complete_and_resolvable() {
        let base = std::env::temp_dir().join(format!("themes-v2-{}", rand::random::<u64>()));
        let listed = list_themes_v2_at(&base);
        assert_eq!(
            listed
                .iter()
                .take(5)
                .map(|x| x.id.as_str())
                .collect::<Vec<_>>(),
            builtin_ids()
        );
        for id in builtin_ids() {
            for mode in ["light", "dark"] {
                let resolved = resolve_theme_at(id, mode, "windows", false, &base).unwrap();
                assert_eq!(
                    resolved.provenance.get("/colors/accent/default"),
                    Some(&id.to_string())
                );
                let colors = resolved
                    .tokens
                    .pointer("/colors")
                    .unwrap()
                    .as_object()
                    .unwrap();
                assert_eq!(
                    colors["accent"].as_object().unwrap().len()
                        + colors["background"].as_object().unwrap().len()
                        + colors["text"].as_object().unwrap().len()
                        + colors["border"].as_object().unwrap().len()
                        + colors["status"].as_object().unwrap().len(),
                    24
                );
            }
            set_local_theme_settings_at(
                &base,
                LocalThemeSettings {
                    active_theme_id: id.into(),
                    appearance: "system".into(),
                    high_contrast: false,
                },
            )
            .unwrap();
            assert!(delete_theme_at(id, &base).is_err());
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn custom_themes_only_extend_canvas() {
        let mut m = manifest();
        m["extends"] = Value::from(FLUX_ID);
        let error = diagnostic(m);
        assert_eq!(error.code, "THEME_EXTENDS");
    }

    #[test]
    fn rejects_css_functions_in_color_tokens() {
        for value in [
            "linear-gradient(#000000, #ffffff)",
            "image-set(\"https://example.invalid/theme.png\" 1x)",
            "var(--host-color)",
            "hsl(0, 0%, 0%)",
        ] {
            let mut m = manifest();
            m["light"]["colors"]["background"]["canvas"] = Value::from(value);
            let validation = validate_theme(&package(m), "light", false);
            assert!(!validation.valid, "{value}");
            assert_eq!(validation.diagnostics[0].code, "THEME_UNSAFE_VALUE");
        }
    }

    #[test]
    fn rgba_requires_exactly_three_channels_and_a_bounded_alpha() {
        for value in [
            "rgba(1, 2, 3)",
            "rgba(1, 2, 3, 1.1)",
            "rgba(1, 2, 3, 0.5, 9)",
        ] {
            let mut m = manifest();
            m["light"]["colors"]["background"]["canvas"] = Value::from(value);
            let validation = validate_theme(&package(m), "light", false);
            assert!(!validation.valid, "{value}");
            assert_eq!(validation.diagnostics[0].code, "THEME_COLOR_EXPRESSION");
        }
    }

    #[test]
    fn resolve_and_asset_reads_reject_traversal_ids() {
        let base = std::env::temp_dir().join(format!("theme-traversal-{}", rand::random::<u64>()));
        assert_eq!(
            resolve_theme_at("custom:../../escape", "light", "windows", false, &base)
                .unwrap_err()
                .code,
            "THEME_ID"
        );
        assert_eq!(
            get_theme_asset_at("custom:../../escape", "digest", "assets/logo.png", &base)
                .unwrap_err()
                .code,
            "THEME_ID"
        );
        assert_eq!(
            get_theme_asset_slot_at("custom:../../escape", "digest", "logo", &base)
                .unwrap_err()
                .code,
            "THEME_ID"
        );
    }

    #[test]
    fn builtin_high_contrast_overrides_visual_tokens() {
        for id in builtin_ids() {
            let resolved =
                resolve_theme_at(id, "light", "windows", true, Path::new("/tmp")).unwrap();
            assert!(
                color_contrast(
                    &resolved.tokens,
                    "/colors/text/primary",
                    "/colors/background/surface",
                    "/colors/background/canvas"
                )
                .unwrap()
                    >= 7.0
            );
            assert!(
                color_contrast(
                    &resolved.tokens,
                    "/components/search/focus/focusRing",
                    "/components/search/focus/background",
                    "/colors/background/canvas"
                )
                .unwrap()
                    >= 3.0
            );
            assert_eq!(resolved.tokens.pointer("/effects/opacity").unwrap(), 1);
            assert_eq!(
                resolved.tokens.pointer("/effects/shadow/opacity").unwrap(),
                0
            );
        }
    }

    #[test]
    fn high_contrast_preserves_compliant_author_colors_and_marks_policy_fallbacks() {
        let mut m = manifest();
        m["highContrast"] = serde_json::json!({
            "light": {
                "colors": {
                    "background": {"canvas": "#112233", "surface": "#112233"},
                    "text": {"primary": "#ffffff"},
                    "accent": {"default": "#ffff00", "onAccent": "#000000"}
                },
                "components": {
                    "search": {"focus": {
                        "background": "#112233",
                        "foreground": "#ffffff",
                        "focusRing": "#ffff00"
                    }}
                },
                "effects": {"opacity": 0.5, "shadow": {"radius": 8, "y": 2, "opacity": 0.4}}
            },
            "dark": {}
        });
        let resolved = validate_theme(&package(m), "light", true).preview.unwrap();
        assert_eq!(
            resolved
                .tokens
                .pointer("/colors/background/canvas")
                .unwrap(),
            "#112233"
        );
        assert_eq!(
            resolved.tokens.pointer("/colors/text/primary").unwrap(),
            "#ffffff"
        );
        assert_eq!(
            resolved
                .tokens
                .pointer("/components/search/focus/background")
                .unwrap(),
            "#112233"
        );
        assert_eq!(
            resolved.provenance["/colors/background/canvas"],
            "highContrast"
        );
        assert_eq!(resolved.tokens.pointer("/effects/opacity").unwrap(), 1);
        assert_eq!(
            resolved.tokens.pointer("/effects/shadow/opacity").unwrap(),
            0
        );
        assert_eq!(
            resolved.provenance["/effects/opacity"],
            "systemHighContrast"
        );
    }
}
