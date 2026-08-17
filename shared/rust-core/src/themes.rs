//! Custom runtime themes: JSON file format, validation, and the token
//! contract shared by the Windows and macOS renderers.
//!
//! The file format mirrors the design-system contract in `docs/THEMING.md`
//! §2.2 (the CSS colour-token table) plus the Swift-side dimensions
//! (`metrics` / `typography` / `fonts`) from
//! `macos/swift-ui/Sources/TailSync/Models/Theme.swift`.
//!
//! Security model: every value is validated before it can leave this module;
//! colour tokens are hex strings (with an optional opacity), font names are
//! restricted to a printable ASCII subset, and no field is ever interpreted
//! as HTML, CSS text, or a shell command.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use base64::Engine;

use crate::db::get_data_dir;

/// Maximum size of a single theme file (64 KiB).
pub const MAX_THEME_FILE_SIZE: usize = 64 * 1024;

/// Maximum size of a theme file that embeds background images (4 MiB).
pub const MAX_THEME_FILE_SIZE_WITH_IMAGE: usize = 4 * 1024 * 1024;

/// Maximum number of custom theme files in the themes directory (over-limit
/// files are ignored and reported).
pub const MAX_THEMES_PER_DIR: usize = 32;

/// Name of the advisory lock file kept inside the themes directory. It is
/// shared by every entry point that mutates the directory (import, delete)
/// so count/conflict decisions serialize; it never ends in `.json`, so it
/// is invisible to listings.
pub const THEMES_LOCK_FILE: &str = ".themes.lock";

/// Reserved IDs of the five built-in themes. A custom theme file may not
/// reuse one of these (conflict error at discovery/import time).
pub const BUILTIN_THEME_IDS: [&str; 5] = ["tailsync", "ocean", "forest", "rose", "high-contrast"];

/// Namespace prefix for custom-theme preferences stored in settings and
/// localStorage (the theme selection value, e.g. `custom:studio`).
pub const CUSTOM_THEME_PREFIX: &str = "custom:";

/// Maximum length of a localised theme name value, counted in Unicode
/// scalar values (characters), not bytes.
pub const MAX_NAME_LENGTH: usize = 64;

/// True when `value` is an acceptable localised theme name: non-empty and at
/// most [`MAX_NAME_LENGTH`] characters. Characters are counted as Unicode
/// scalar values (`chars().count()`), so multi-byte scripts (Chinese, emoji,
/// etc.) are treated the same as ASCII. This is deliberately separate from
/// the byte-based file-size limit ([`MAX_THEME_FILE_SIZE`]), which is
/// unchanged.
pub fn is_valid_name_value(value: &str) -> bool {
    !value.is_empty() && value.chars().count() <= MAX_NAME_LENGTH
}

/// Maximum length of a font name.
pub const MAX_FONT_NAME_LENGTH: usize = 64;

/// Returns true when `id` collides with a built-in theme's reserved ID.
pub fn is_builtin_id(id: &str) -> bool {
    BUILTIN_THEME_IDS.contains(&id)
}

/// Theme-id rule from the specification: `^[a-z0-9][a-z0-9-]{0,31}$`.
pub fn is_valid_theme_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    let mut rest = 0;
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return false;
        }
        rest += 1;
    }
    rest <= 31
}

/// True when the value is a custom-theme preference of the form
/// `custom:{id}` with a valid theme id. Used by the Settings validation so
/// the theme selection can sync across devices even when the theme file
/// itself is local.
pub fn is_custom_theme_preference(value: &str) -> bool {
    match value.strip_prefix(CUSTOM_THEME_PREFIX) {
        Some(id) => is_valid_theme_id(id),
        None => false,
    }
}

/// Language-key rule: `en` / `zh-CN` or any BCP-47-like form
/// `^[a-zA-Z-]{2,10}$`.
pub fn is_valid_lang_key(key: &str) -> bool {
    let len = key.len();
    if !(2..=10).contains(&len) {
        return false;
    }
    key.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
}

/// Hex-colour rule: `^#[0-9a-fA-F]{6}$`.
pub fn is_valid_hex(hex: &str) -> bool {
    let body = match hex.strip_prefix('#') {
        Some(body) => body,
        None => return false,
    };
    body.len() == 6 && body.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Font-name rule: `^[A-Za-z0-9 .,'-]+$` with length 1..=64.
pub fn is_valid_font_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_FONT_NAME_LENGTH {
        return false;
    }
    name.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'.' | b',' | b'\'' | b'-'))
}

/// A validated colour token: a `#rrggbb` hex colour plus an optional opacity
/// in `[0, 1]`. In JSON this may be written either as a bare string
/// (`"#d5684b"`) or as an object (`{ "hex": "#d5684b", "opacity": 0.11 }`).
#[derive(Debug, Clone, PartialEq)]
pub struct ColorSpec {
    pub hex: String,
    pub opacity: Option<f64>,
}

impl ColorSpec {
    /// CSS value in the THEMING.md §2.2 style: the bare hex when no opacity is
    /// present, otherwise an `rgba(r, g, b, opacity)` string. Never panics and
    /// never echoes unvalidated input: if the hex is malformed the raw value
    /// is returned unchanged (callers only reach this on validated themes).
    pub fn css_value(&self) -> String {
        if let Some(opacity) = self.opacity {
            if let Some((r, g, b)) = parse_hex_rgb(&self.hex) {
                return format!("rgba({r}, {g}, {b}, {opacity})");
            }
        }
        self.hex.clone()
    }
}

fn parse_hex_rgb(hex: &str) -> Option<(u32, u32, u32)> {
    let body = hex.strip_prefix('#')?;
    if body.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(body, 16).ok()?;
    Some(((value >> 16) & 0xFF, (value >> 8) & 0xFF, value & 0xFF))
}

impl Serialize for ColorSpec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ColorSpec", 2)?;
        state.serialize_field("hex", &self.hex)?;
        if let Some(opacity) = self.opacity {
            state.serialize_field("opacity", &opacity)?;
        }
        state.end()
    }
}

impl<'de> Deserialize<'de> for ColorSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Plain(String),
            WithOpacity {
                hex: String,
                #[serde(default)]
                opacity: Option<f64>,
            },
        }
        match Raw::deserialize(deserializer)? {
            Raw::Plain(hex) => Ok(ColorSpec { hex, opacity: None }),
            Raw::WithOpacity { hex, opacity } => Ok(ColorSpec { hex, opacity }),
        }
    }
}

/// One light or dark palette: the 24 CSS colour tokens of THEMING.md §2.2,
/// one field per token (CSS name without the leading `--`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PaletteSpec {
    pub brand: ColorSpec,
    pub brand_hover: ColorSpec,
    pub brand_soft: ColorSpec,
    pub brand_text: ColorSpec,
    pub bg_window: ColorSpec,
    pub bg_card: ColorSpec,
    pub bg_input: ColorSpec,
    pub bg_hover: ColorSpec,
    pub bg_active: ColorSpec,
    pub bg_raised: ColorSpec,
    pub bg_toast: ColorSpec,
    pub text_primary: ColorSpec,
    pub text_secondary: ColorSpec,
    pub text_tertiary: ColorSpec,
    pub text_toast: ColorSpec,
    pub border: ColorSpec,
    pub border_strong: ColorSpec,
    pub divider: ColorSpec,
    pub green: ColorSpec,
    pub green_soft: ColorSpec,
    pub orange: ColorSpec,
    pub orange_soft: ColorSpec,
    pub purple: ColorSpec,
    pub purple_soft: ColorSpec,
}

impl PaletteSpec {
    /// The 24 CSS colour tokens of THEMING.md §2.2, in the order they appear
    /// in `shared/art-direction.css` theme blocks: (`--name`, spec) pairs.
    pub fn color_tokens(&self) -> Vec<(&'static str, &ColorSpec)> {
        vec![
            ("--brand", &self.brand),
            ("--brand-hover", &self.brand_hover),
            ("--brand-soft", &self.brand_soft),
            ("--brand-text", &self.brand_text),
            ("--bg-window", &self.bg_window),
            ("--bg-card", &self.bg_card),
            ("--bg-input", &self.bg_input),
            ("--bg-hover", &self.bg_hover),
            ("--bg-active", &self.bg_active),
            ("--bg-raised", &self.bg_raised),
            ("--bg-toast", &self.bg_toast),
            ("--text-primary", &self.text_primary),
            ("--text-secondary", &self.text_secondary),
            ("--text-tertiary", &self.text_tertiary),
            ("--text-toast", &self.text_toast),
            ("--border", &self.border),
            ("--border-strong", &self.border_strong),
            ("--divider", &self.divider),
            ("--green", &self.green),
            ("--green-soft", &self.green_soft),
            ("--orange", &self.orange),
            ("--orange-soft", &self.orange_soft),
            ("--purple", &self.purple),
            ("--purple-soft", &self.purple_soft),
        ]
    }

    /// CSS custom-property pairs (`--name`, value) in contract order.
    pub fn css_variables(&self) -> Vec<(&'static str, String)> {
        self.color_tokens()
            .into_iter()
            .map(|(name, color)| (name, color.css_value()))
            .collect()
    }
}

/// Shape and density dimensions (Swift `TailSyncThemeMetrics` mirror).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeMetrics {
    pub card_radius: f64,
    pub control_radius: f64,
    pub row_padding: f64,
    pub shadow_radius: f64,
}

/// Typography dimensions (Swift `TailSyncThemeTypography` mirror).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeTypography {
    pub section_title_size: f64,
    pub uppercases_section_titles: bool,
    pub search_size: f64,
    pub search_uses_display_font: bool,
    pub history_content_size: f64,
}

/// Font names for the display/reading faces; `null` (or absent) means the
/// platform system font.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeFonts {
    pub display: Option<String>,
    pub reading: Option<String>,
}

/// Structural overrides (V1 supports `borderRadius` and `shadow: false`).
/// Unknown keys are kept in `ignored` so the apply step can warn about them
/// instead of rejecting the whole file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeStructural {
    pub border_radius: Option<f64>,
    pub shadow: Option<bool>,
    #[serde(flatten)]
    pub ignored: BTreeMap<String, serde_json::Value>,
}

/// An embedded background image: the declared MIME type plus the payload as
/// strict standard base64. Only PNG and JPEG are accepted; the payload is
/// validated against the declared type and the header limits before it can
/// reach any renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageSpec {
    pub mime_type: ImageMime,
    pub data_b64: String,
}

/// One light/dark background mode: an optional image plus its scrim (required
/// whenever the image is present). `scrim` alone is rejected at validation —
/// the renderer only supports the "image + scrim" pairing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundMode {
    #[serde(default)]
    pub image: Option<ImageSpec>,
    #[serde(default)]
    pub scrim: Option<ColorSpec>,
}

/// Optional per-mode background definitions. Each mode may be absent (no
/// background on that side), and the two sides are independent.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeBackground {
    #[serde(default)]
    pub light: Option<BackgroundMode>,
    #[serde(default)]
    pub dark: Option<BackgroundMode>,
}

/// Light and dark palettes; both are required (a theme must define both
/// modes, per the design rules in THEMING.md §4.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemePalettePair {
    pub light: PaletteSpec,
    pub dark: PaletteSpec,
}

/// The validated theme-file shape. Unknown top-level fields are rejected so
/// authors get a precise error instead of a silently ignored typo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    pub format: u32,
    pub id: String,
    pub name: BTreeMap<String, String>,
    pub palette: ThemePalettePair,
    pub metrics: ThemeMetrics,
    pub typography: ThemeTypography,
    pub fonts: ThemeFonts,
    #[serde(default)]
    pub structural: Option<ThemeStructural>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<ThemeBackground>,
}

/// Validation failure for one theme file. Never a panic: every rule failure
/// is reported as a `ThemeLoadError { file, reason }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThemeLoadError {
    pub file: String,
    pub reason: String,
}

impl ThemeLoadError {
    pub fn new(file: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for ThemeLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.file, self.reason)
    }
}

impl std::error::Error for ThemeLoadError {}

/// Parse and validate a theme file's bytes. `file` is a display name for
/// error reporting (file name or path); it is never used for anything else.
/// The size cap is conditional: themes with embedded background images may
/// be up to [`MAX_THEME_FILE_SIZE_WITH_IMAGE`], everything else stays at
/// [`MAX_THEME_FILE_SIZE`].
pub fn validate_theme_bytes(bytes: &[u8], file: &str) -> Result<ThemeFile, ThemeLoadError> {
    if bytes.len() > MAX_THEME_FILE_SIZE_WITH_IMAGE {
        return Err(ThemeLoadError::new(
            file,
            format!("Theme file exceeds the {MAX_THEME_FILE_SIZE_WITH_IMAGE} byte size limit"),
        ));
    }
    let theme: ThemeFile = serde_json::from_slice(bytes)
        .map_err(|error| ThemeLoadError::new(file, format!("Invalid theme JSON: {error}")))?;
    theme
        .validate()
        .map_err(|reason| ThemeLoadError::new(file, reason))?;
    if !theme.has_background_image() && bytes.len() > MAX_THEME_FILE_SIZE {
        return Err(ThemeLoadError::new(
            file,
            format!("Theme file exceeds the {MAX_THEME_FILE_SIZE} byte size limit"),
        ));
    }
    Ok(theme)
}

impl ThemeFile {
    /// Apply every semantic rule from the specification. Returns the first
    /// failure reason; never panics.
    pub fn validate(&self) -> Result<(), String> {
        if self.format != 1 {
            return Err(format!(
                "Unsupported theme format {} (only format 1 is supported)",
                self.format
            ));
        }
        if !is_valid_theme_id(&self.id) {
            return Err(format!(
                "Invalid theme id {:?} (must match ^[a-z0-9][a-z0-9-]{{0,31}}$)",
                self.id
            ));
        }
        if !self.name.contains_key("en") {
            return Err("Theme name must include an \"en\" entry".to_string());
        }
        for (key, value) in &self.name {
            if !is_valid_lang_key(key) {
                return Err(format!(
                    "Invalid theme name key {key:?} (must match ^[a-zA-Z-]{{2,10}}$)"
                ));
            }
            if !is_valid_name_value(value) {
                return Err(format!(
                    "Theme name {value:?} exceeds the {MAX_NAME_LENGTH} character limit"
                ));
            }
        }
        validate_palette(&self.palette.light, "light")?;
        validate_palette(&self.palette.dark, "dark")?;
        validate_metrics(&self.metrics)?;
        validate_typography(&self.typography)?;
        validate_fonts(&self.fonts)?;
        validate_structural(self.structural.as_ref())?;
        validate_background(self.background.as_ref())?;
        Ok(())
    }

    /// True when any background mode carries an embedded image.
    pub fn has_background_image(&self) -> bool {
        self.background
            .as_ref()
            .map(|background| {
                [&background.light, &background.dark]
                    .iter()
                    .any(|mode| mode.as_ref().is_some_and(|mode| mode.image.is_some()))
            })
            .unwrap_or(false)
    }

    /// Best localised display name for `preferred` locale (falls back to
    /// "en", then to any entry, then to the id).
    pub fn localized_name(&self, preferred: &str) -> String {
        self.name
            .get(preferred)
            .or_else(|| self.name.get("en"))
            .or_else(|| self.name.values().next())
            .cloned()
            .unwrap_or_else(|| self.id.clone())
    }

    pub fn palette_light(&self) -> &PaletteSpec {
        &self.palette.light
    }

    pub fn palette_dark(&self) -> &PaletteSpec {
        &self.palette.dark
    }
}

fn validate_palette(palette: &PaletteSpec, mode: &str) -> Result<(), String> {
    for (token, color) in palette.color_tokens() {
        if !is_valid_hex(&color.hex) {
            return Err(format!(
                "Palette {mode} token {token} has invalid colour {color:?} (must match ^#[0-9a-fA-F]{{6}}$)"
            ));
        }
        if let Some(opacity) = color.opacity {
            if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
                return Err(format!(
                    "Palette {mode} token {token} has opacity {opacity} outside [0, 1]"
                ));
            }
        }
    }
    Ok(())
}

fn validate_metrics(metrics: &ThemeMetrics) -> Result<(), String> {
    check_range("metrics.cardRadius", metrics.card_radius, 0.0, 24.0)?;
    check_range("metrics.controlRadius", metrics.control_radius, 0.0, 24.0)?;
    check_range("metrics.rowPadding", metrics.row_padding, 4.0, 32.0)?;
    check_range("metrics.shadowRadius", metrics.shadow_radius, 0.0, 32.0)
}

fn validate_typography(typography: &ThemeTypography) -> Result<(), String> {
    check_range(
        "typography.sectionTitleSize",
        typography.section_title_size,
        9.0,
        32.0,
    )?;
    check_range("typography.searchSize", typography.search_size, 9.0, 32.0)?;
    check_range(
        "typography.historyContentSize",
        typography.history_content_size,
        9.0,
        32.0,
    )
}

fn validate_fonts(fonts: &ThemeFonts) -> Result<(), String> {
    for (field, name) in [("display", &fonts.display), ("reading", &fonts.reading)] {
        if let Some(name) = name {
            if !is_valid_font_name(name) {
                return Err(format!(
                    "fonts.{field} has invalid font name {name:?} (allowed: letters, digits, spaces, . , ' - ; at most {MAX_FONT_NAME_LENGTH} characters)"
                ));
            }
        }
    }
    Ok(())
}

fn validate_structural(structural: Option<&ThemeStructural>) -> Result<(), String> {
    if let Some(structural) = structural {
        if let Some(border_radius) = structural.border_radius {
            check_range("structural.borderRadius", border_radius, 0.0, 64.0)?;
        }
    }
    Ok(())
}

/// Scrim opacity must stay strong enough for text contrast (readability is
/// not negotiable) — values outside [0.5, 0.95] are rejected.
const SCRIM_OPACITY_MIN: f64 = 0.5;
const SCRIM_OPACITY_MAX: f64 = 0.95;

/// Validate both background modes: image/scrim pairing, scrim hex + opacity
/// range, and the embedded payload (strict base64, byte cap, magic +
/// dimensions via the header parser).
fn validate_background(background: Option<&ThemeBackground>) -> Result<(), String> {
    let Some(background) = background else {
        return Ok(());
    };
    validate_background_mode(background.light.as_ref(), "background.light")?;
    validate_background_mode(background.dark.as_ref(), "background.dark")
}

fn validate_background_mode(mode: Option<&BackgroundMode>, path: &str) -> Result<(), String> {
    let Some(mode) = mode else {
        return Ok(());
    };
    match (&mode.image, &mode.scrim) {
        (Some(image), Some(scrim)) => {
            if !is_valid_hex(&scrim.hex) {
                return Err(format!(
                    "{path}: scrim has invalid colour {:?} (must match ^#[0-9a-fA-F]{{6}}$)",
                    scrim.hex
                ));
            }
            let opacity = scrim.opacity.ok_or_else(|| {
                format!("{path}: scrim opacity is required in [{SCRIM_OPACITY_MIN}, {SCRIM_OPACITY_MAX}]")
            })?;
            if !opacity.is_finite() || !(SCRIM_OPACITY_MIN..=SCRIM_OPACITY_MAX).contains(&opacity) {
                return Err(format!(
                    "{path}: scrim opacity {opacity} is outside [{SCRIM_OPACITY_MIN}, {SCRIM_OPACITY_MAX}]"
                ));
            }
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(&image.data_b64)
                .map_err(|_| format!("{path}: image dataB64 is not valid base64"))?;
            if decoded.len() > MAX_IMAGE_BYTES {
                return Err(format!(
                    "{path}: image exceeds the {MAX_IMAGE_BYTES} byte limit"
                ));
            }
            validate_image_payload(&decoded, image.mime_type)
                .map_err(|reason| format!("{path}: {reason}"))?;
            Ok(())
        }
        (Some(_), None) => Err(format!(
            "{path}: scrim is required when an image is present (readability is not negotiable)"
        )),
        (None, Some(_)) => Err(format!(
            "{path}: scrim requires an image (the renderer only supports the image + scrim pairing)"
        )),
        (None, None) => Ok(()),
    }
}

fn check_range(field: &str, value: f64, min: f64, max: f64) -> Result<(), String> {
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "{field} value {value} is outside the allowed range [{min}, {max}]"
        ))
    }
}

// ─── Discovery and listing ──────────────────────────────────────────────

/// Name of the custom-themes directory inside the platform data directory.
pub const THEMES_DIRECTORY_NAME: &str = "themes";

/// One validated custom theme, as handed to the UI layers. `file` is the
/// theme file's base name inside the themes directory. Never carries image
/// bytes: background info is metadata only (presence, scrim, MIME type); the
/// payload is fetched on demand via `theme_background`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeEntry {
    pub id: String,
    pub name: BTreeMap<String, String>,
    pub file: String,
    pub palette: ThemePalettePair,
    pub metrics: ThemeMetrics,
    pub typography: ThemeTypography,
    pub fonts: ThemeFonts,
    #[serde(default)]
    pub structural: Option<ThemeStructural>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<ThemeEntryBackground>,
}

/// Background metadata for one entry: per-mode presence/scrim/MIME only —
/// explicitly no payload bytes, so listing volume stays the same order of
/// magnitude as before this feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeEntryBackground {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<ThemeBackgroundMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<ThemeBackgroundMeta>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeBackgroundMeta {
    pub has_image: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrim: Option<ColorSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<ImageMime>,
}

impl ThemeEntry {
    pub fn from_theme(theme: ThemeFile, file: String) -> Self {
        let background = theme
            .background
            .as_ref()
            .map(|background| ThemeEntryBackground {
                light: background
                    .light
                    .as_ref()
                    .map(ThemeBackgroundMeta::from_mode),
                dark: background.dark.as_ref().map(ThemeBackgroundMeta::from_mode),
            });
        Self {
            id: theme.id,
            name: theme.name,
            file,
            palette: theme.palette,
            metrics: theme.metrics,
            typography: theme.typography,
            fonts: theme.fonts,
            structural: theme.structural,
            background,
        }
    }
}

impl ThemeBackgroundMeta {
    fn from_mode(mode: &BackgroundMode) -> Self {
        let image = mode.image.as_ref();
        ThemeBackgroundMeta {
            has_image: image.is_some(),
            scrim: mode.scrim.clone(),
            mime_type: image.map(|image| image.mime_type),
        }
    }
}

/// Result of scanning the themes directory: every valid theme plus an error
/// marker per skipped file (bad JSON, reserved id, duplicate id, over-limit,
/// unreadable).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ThemeListing {
    pub entries: Vec<ThemeEntry>,
    pub errors: Vec<ThemeLoadError>,
}

/// Path of the themes directory under the platform data directory
/// (`{数据目录}/themes`), created with owner-only permissions (0700 on Unix,
/// protected DACL on Windows) when needed.
pub fn themes_dir() -> PathBuf {
    ensure_themes_dir(&get_data_dir())
        .unwrap_or_else(|_| get_data_dir().join(THEMES_DIRECTORY_NAME))
}

/// Path of the themes directory under an explicit base directory.
pub fn themes_dir_at(base: &Path) -> PathBuf {
    base.join(THEMES_DIRECTORY_NAME)
}

/// Create the themes directory below `base` with owner-only permissions.
/// Idempotent; safe to call on every list/import.
pub fn ensure_themes_dir(base: &Path) -> Result<PathBuf, String> {
    let directory = themes_dir_at(base);
    std::fs::create_dir_all(&directory).map_err(|error| {
        format!(
            "Could not create themes directory {}: {error}",
            directory.display()
        )
    })?;
    crate::identity::restrict_private_directory(&directory).map_err(|error| {
        format!(
            "Could not restrict themes directory {}: {error}",
            directory.display()
        )
    })?;
    Ok(directory)
}

/// Scan `{数据目录}/themes/*.json` and return every valid custom theme plus
/// error markers for skipped files. Only `.json` files are read;
/// subdirectories and other extensions are ignored.
pub fn list_themes() -> ThemeListing {
    list_themes_at(&get_data_dir())
}

/// Sorted list of `(base name, path)` for every `.json` file directly inside
/// `directory`. Subdirectories and other extensions are ignored.
fn scan_json_files(directory: &Path) -> Vec<(String, PathBuf)> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let Ok(read) = std::fs::read_dir(directory) else {
        return files;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue; // subdirectories and non-files are ignored
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".json") {
            continue; // only .json files are read
        }
        files.push((name.to_string(), path));
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

/// Scan the themes directory below an explicit base (testable variant of
/// [`list_themes`]).
pub fn list_themes_at(base: &Path) -> ThemeListing {
    let directory = match ensure_themes_dir(base) {
        Ok(directory) => directory,
        Err(reason) => {
            return ThemeListing {
                entries: Vec::new(),
                errors: vec![ThemeLoadError::new("<themes dir>", reason)],
            }
        }
    };
    let files = scan_json_files(&directory);

    let mut listing = ThemeListing::default();
    let mut seen_ids = std::collections::HashSet::new();
    for (index, (name, path)) in files.into_iter().enumerate() {
        if index >= MAX_THEMES_PER_DIR {
            listing.errors.push(ThemeLoadError::new(
                name,
                format!(
                    "Themes directory exceeds the {MAX_THEMES_PER_DIR} theme limit; extra files are ignored"
                ),
            ));
            continue;
        }
        match load_theme_file(&path, &name) {
            Ok(theme) => {
                if is_builtin_id(&theme.id) {
                    listing.errors.push(ThemeLoadError::new(
                        name,
                        format!("Theme id {:?} is reserved for a built-in theme", theme.id),
                    ));
                    continue;
                }
                if !seen_ids.insert(theme.id.clone()) {
                    listing.errors.push(ThemeLoadError::new(
                        name,
                        format!("Duplicate theme id {:?}; ignoring this file", theme.id),
                    ));
                    continue;
                }
                listing.entries.push(ThemeEntry::from_theme(theme, name));
            }
            Err(error) => listing.errors.push(error),
        }
    }
    listing
}

fn load_theme_file(path: &Path, name: &str) -> Result<ThemeFile, ThemeLoadError> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        ThemeLoadError::new(name, format!("Could not read theme file: {error}"))
    })?;
    if metadata.len() > MAX_THEME_FILE_SIZE_WITH_IMAGE as u64 {
        return Err(ThemeLoadError::new(
            name,
            format!("Theme file exceeds the {MAX_THEME_FILE_SIZE_WITH_IMAGE} byte size limit"),
        ));
    }
    let bytes = std::fs::read(path).map_err(|error| {
        ThemeLoadError::new(name, format!("Could not read theme file: {error}"))
    })?;
    validate_theme_bytes(&bytes, name)
}

// ─── Import and delete ─────────────────────────────────────────────────

/// Acquire the advisory exclusive lock over the themes directory. The lock
/// file lives inside the directory and the OS releases the lock
/// automatically when the process exits, so a crashed import can never
/// leave the directory permanently locked. All TailSync entry points that
/// mutate the directory (import, delete) take this same lock, which makes
/// count and conflict decisions (the 32-theme limit, duplicate ids) atomic
/// even under concurrent calls from different threads or processes. The
/// lock is advisory: it coordinates TailSync entry points, not unrelated
/// processes that ignore it. Dropping the returned file releases the lock.
fn lock_themes_dir(directory: &Path) -> Result<std::fs::File, String> {
    use fs2::FileExt;
    let lock_path = directory.join(THEMES_LOCK_FILE);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| format!("Could not open {}: {error}", lock_path.display()))?;
    file.lock_exclusive()
        .map_err(|error| format!("Could not lock {}: {error}", lock_path.display()))?;
    Ok(file)
}

/// Import a theme file into `{数据目录}/themes/{id}.json`. The source is
/// read and fully validated before anything is written; the stored copy is
/// an independent copy, not a reference. Conflicts (an existing file with
/// the same id, a reserved built-in id, or a full themes directory) are
/// reported as clear errors. Only `.json` sources are accepted.
///
/// The install is atomic and concurrency-safe: bytes are written to a
/// unique temporary file inside the themes directory (whose name never ends
/// in `.json`, so listings can never observe it), flushed, and then moved
/// into place with `rename` while holding the directory lock. A concurrent
/// list either sees the complete target or no target — never a half-written
/// file — and concurrent imports of the same id serialize so exactly one
/// succeeds. Temporary files are removed on every failure path.
pub fn import_theme_file(src: &Path) -> Result<ThemeEntry, String> {
    import_theme_file_at(src, &get_data_dir())
}

/// Testable variant of [`import_theme_file`] with an explicit base directory.
pub fn import_theme_file_at(src: &Path, base: &Path) -> Result<ThemeEntry, String> {
    let source_name = src.display().to_string();
    if !source_name.ends_with(".json") {
        return Err(format!(
            "{source_name}: only .json theme files can be imported"
        ));
    }
    let bytes =
        std::fs::read(src).map_err(|error| format!("Could not read {source_name}: {error}"))?;
    let theme = validate_theme_bytes(&bytes, &source_name).map_err(|error| error.to_string())?;
    if is_builtin_id(&theme.id) {
        return Err(format!(
            "Theme id {:?} is reserved for a built-in theme",
            theme.id
        ));
    }
    let directory = ensure_themes_dir(base)?;
    // Everything below must serialize with other import/delete calls.
    let _lock = lock_themes_dir(&directory)?;
    let target_name = format!("{}.json", theme.id);
    let target = directory.join(&target_name);
    if target.is_file() {
        return Err(format!(
            "A theme with id {:?} already exists ({}); delete it first or choose a different file",
            theme.id,
            target.display()
        ));
    }
    let mut json_count = 0usize;
    for (name, path) in scan_json_files(&directory) {
        if name == target_name {
            continue;
        }
        json_count += 1;
        if let Ok(existing) = load_theme_file(&path, &name) {
            if existing.id == theme.id {
                return Err(format!(
                    "A theme with id {:?} already exists in {}",
                    theme.id, name
                ));
            }
        }
    }
    if json_count >= MAX_THEMES_PER_DIR {
        return Err(format!(
            "Themes directory already contains the maximum of {MAX_THEMES_PER_DIR} themes"
        ));
    }
    // Write to a unique temporary file first, then atomically install it as
    // `{id}.json`. The temporary name is a dotfile that never ends in
    // `.json`, so concurrent listings never see a half-written target: the
    // target only appears via the atomic rename, fully flushed.
    let temp = directory.join(format!(
        ".{target_name}.tmp-{:08x}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let install = (|| -> Result<(), String> {
        let mut file = std::fs::File::create(&temp)
            .map_err(|error| format!("Could not create {}: {error}", temp.display()))?;
        file.write_all(&bytes)
            .map_err(|error| format!("Could not write {}: {error}", temp.display()))?;
        file.sync_all()
            .map_err(|error| format!("Could not flush {}: {error}", temp.display()))?;
        std::fs::rename(&temp, &target)
            .map_err(|error| format!("Could not install {}: {error}", target.display()))?;
        Ok(())
    })();
    if let Err(error) = install {
        // Never leave the temporary file behind on a failed install.
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    Ok(ThemeEntry::from_theme(theme, target_name))
}

/// Delete the custom theme stored as `{id}.json` inside the themes
/// directory. Built-in ids and ids that do not match the theme-id rule are
/// rejected; the target path is always `{themes dir}/{id}.json`, so only
/// files inside the themes directory can ever be removed.
pub fn delete_theme(id: &str) -> Result<(), String> {
    delete_theme_at(id, &get_data_dir())
}

/// Testable variant of [`delete_theme`] with an explicit base directory.
pub fn delete_theme_at(id: &str, base: &Path) -> Result<(), String> {
    if !is_valid_theme_id(id) {
        return Err(format!("Invalid theme id {id:?}"));
    }
    if is_builtin_id(id) {
        return Err(format!(
            "Theme id {id:?} is reserved for a built-in theme and cannot be deleted"
        ));
    }
    let directory = ensure_themes_dir(base)?;
    // Serialize with imports: a concurrent import of the same id either
    // runs before us (we report "not found") or after us (it reports
    // "already exists") — never both succeeding.
    let _lock = lock_themes_dir(&directory)?;
    let target = directory.join(format!("{id}.json"));
    if !target.is_file() {
        return Err(format!("Theme {id:?} not found"));
    }
    std::fs::remove_file(&target)
        .map_err(|error| format!("Could not delete {}: {error}", target.display()))?;
    Ok(())
}

/// Decoded background image of one custom theme mode.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundImage {
    pub mime_type: ImageMime,
    pub data: Vec<u8>,
}

/// Fetch the decoded background image of `{id}.json` for one mode. Returns
/// `Ok(None)` when the theme (or that mode) has no image; `Err` for invalid
/// or reserved ids, unreadable files, or payloads that fail the R001/R002
/// re-validation on read. The image is never part of the listing — callers
/// ask for it on demand.
pub fn theme_background(id: &str, light: bool) -> Result<Option<BackgroundImage>, String> {
    theme_background_at(id, light, &get_data_dir())
}

/// Testable variant of [`theme_background`] with an explicit base directory.
pub fn theme_background_at(
    id: &str,
    light: bool,
    base: &Path,
) -> Result<Option<BackgroundImage>, String> {
    if !is_valid_theme_id(id) {
        return Err(format!("Invalid theme id {id:?}"));
    }
    if is_builtin_id(id) {
        return Err(format!("Theme id {id:?} is reserved for a built-in theme"));
    }
    let directory = ensure_themes_dir(base)?;
    let path = directory.join(format!("{id}.json"));
    if !path.is_file() {
        return Ok(None);
    }
    let theme = load_theme_file(&path, &format!("{id}.json")).map_err(|error| error.to_string())?;
    let mode = if light {
        theme
            .background
            .as_ref()
            .and_then(|background| background.light.as_ref())
    } else {
        theme
            .background
            .as_ref()
            .and_then(|background| background.dark.as_ref())
    };
    let Some(mode) = mode else {
        return Ok(None);
    };
    let Some(image) = mode.image.as_ref() else {
        return Ok(None);
    };
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&image.data_b64)
        .map_err(|_| format!("Theme {id:?} contains an image that is not valid base64"))?;
    // Re-validate the payload on every read (the file could have been
    // tampered with after listing).
    validate_image_payload(&decoded, image.mime_type)
        .map_err(|reason| format!("Theme {id:?} background image failed validation: {reason}"))?;
    Ok(Some(BackgroundImage {
        mime_type: image.mime_type,
        data: decoded,
    }))
}

// ─── Embedded background-image payload validation ───────────────────────

/// MIME types accepted for theme background images. SVG (scripting surface)
/// and GIF (animation decoding surface) are never accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMime {
    Png,
    Jpeg,
}

impl ImageMime {
    pub fn mime_type(&self) -> &'static str {
        match self {
            ImageMime::Png => "image/png",
            ImageMime::Jpeg => "image/jpeg",
        }
    }

    pub fn parse(value: &str) -> Option<ImageMime> {
        match value {
            "image/png" => Some(ImageMime::Png),
            "image/jpeg" => Some(ImageMime::Jpeg),
            _ => None,
        }
    }
}

impl Serialize for ImageMime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.mime_type())
    }
}

impl<'de> Deserialize<'de> for ImageMime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        ImageMime::parse(&value).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "Unsupported image MIME type {value:?} (only image/png and image/jpeg are accepted)"
            ))
        })
    }
}

/// Header-declared dimensions of an embedded image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageDimensions {
    pub width: u32,
    pub height: u32,
}

/// Per-side dimension limit for embedded images.
pub const MAX_IMAGE_DIMENSION: u32 = 6000;

/// Total pixel limit for embedded images (6000 × 4000).
pub const MAX_IMAGE_PIXELS: u64 = 24_000_000;

/// Max decoded size of an embedded image payload.
pub const MAX_IMAGE_BYTES: usize = 3 * 1024 * 1024;

const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Validate an image payload against the declared MIME type by reading only
/// the container header (never decoding pixels). Returns the declared
/// dimensions after enforcing the dimension and pixel-count limits. This is
/// the bomb guard: a "small file, huge dimensions" payload is rejected here
/// without ever allocating pixel buffers.
pub fn validate_image_payload(bytes: &[u8], mime: ImageMime) -> Result<ImageDimensions, String> {
    let dimensions = match mime {
        ImageMime::Png => parse_png_dimensions(bytes)?,
        ImageMime::Jpeg => parse_jpeg_dimensions(bytes)?,
    };
    if dimensions.width == 0 || dimensions.height == 0 {
        return Err(format!(
            "{} image declares zero dimension ({}x{})",
            mime.mime_type(),
            dimensions.width,
            dimensions.height
        ));
    }
    if dimensions.width > MAX_IMAGE_DIMENSION || dimensions.height > MAX_IMAGE_DIMENSION {
        return Err(format!(
            "{} image dimensions {}x{} exceed the {MAX_IMAGE_DIMENSION}px per-side limit",
            mime.mime_type(),
            dimensions.width,
            dimensions.height
        ));
    }
    let pixels = u64::from(dimensions.width) * u64::from(dimensions.height);
    if pixels > MAX_IMAGE_PIXELS {
        return Err(format!(
            "{} image {}x{} exceeds the {} megapixel limit",
            mime.mime_type(),
            dimensions.width,
            dimensions.height,
            MAX_IMAGE_PIXELS / 1_000_000
        ));
    }
    Ok(dimensions)
}

/// PNG: signature + the IHDR chunk's width/height fields. Requires the full
/// IHDR chunk (29 bytes) so the header is structurally plausible.
fn parse_png_dimensions(bytes: &[u8]) -> Result<ImageDimensions, String> {
    if bytes.len() < 8 || bytes[..8] != PNG_SIGNATURE {
        return Err("Image bytes do not carry a PNG signature".to_string());
    }
    if bytes.len() < 29 {
        return Err("PNG header is truncated before the IHDR chunk".to_string());
    }
    if &bytes[12..16] != b"IHDR" {
        return Err("First PNG chunk is not IHDR".to_string());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("slice length checked"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("slice length checked"));
    Ok(ImageDimensions { width, height })
}

/// JPEG: SOI marker, then scan segments until an SOF0/SOF1/SOF2 frame marker
/// yields the height/width. Standalone markers (TEM, RSTn) and segment
/// lengths are skipped; EOI/SOS before SOF means no frame was found.
fn parse_jpeg_dimensions(bytes: &[u8]) -> Result<ImageDimensions, String> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Err("Image bytes do not carry a JPEG SOI marker".to_string());
    }
    let mut pos = 2usize;
    while pos < bytes.len() {
        if bytes[pos] != 0xFF {
            return Err("Invalid JPEG marker stream".to_string());
        }
        let mut marker_at = pos;
        while marker_at < bytes.len() && bytes[marker_at] == 0xFF {
            marker_at += 1;
        }
        if marker_at >= bytes.len() {
            return Err("Truncated JPEG marker".to_string());
        }
        let marker = bytes[marker_at];
        let segment = marker_at + 1;
        match marker {
            0xD8 => pos = segment,               // stray SOI: continue
            0x01 | 0xD0..=0xD7 => pos = segment, // TEM / RSTn: no length
            0xD9 | 0xDA => break,                // EOI / SOS before any SOF
            0xC0..=0xC2 => {
                if segment + 7 > bytes.len() {
                    return Err("Truncated JPEG SOF segment".to_string());
                }
                let height = u16::from_be_bytes([bytes[segment + 3], bytes[segment + 4]]) as u32;
                let width = u16::from_be_bytes([bytes[segment + 5], bytes[segment + 6]]) as u32;
                return Ok(ImageDimensions { width, height });
            }
            _ => {
                if segment + 2 > bytes.len() {
                    return Err("Truncated JPEG segment length".to_string());
                }
                let length = u16::from_be_bytes([bytes[segment], bytes[segment + 1]]) as usize;
                if length < 2 {
                    return Err("Invalid JPEG segment length".to_string());
                }
                pos = segment + length;
            }
        }
    }
    Err("JPEG stream has no SOF frame marker".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_theme_json() -> serde_json::Value {
        serde_json::json!({
            "format": 1,
            "id": "studio",
            "name": { "en": "Studio", "zh-CN": "工作室" },
            "palette": {
                "light": {
                    "brand": "#d5684b",
                    "brandHover": "#bb553b",
                    "brandSoft": { "hex": "#d5684b", "opacity": 0.11 },
                    "brandText": "#ffffff",
                    "bgWindow": "#faf9f5",
                    "bgCard": "#fffefa",
                    "bgInput": { "hex": "#1a1916", "opacity": 0.045 },
                    "bgHover": "#f0eee7",
                    "bgActive": "#e8e4da",
                    "bgRaised": "#fffefa",
                    "bgToast": "#171716",
                    "textPrimary": "#191918",
                    "textSecondary": "#68665f",
                    "textTertiary": "#98958b",
                    "textToast": "#ffffff",
                    "border": "#e7e3d9",
                    "borderStrong": "#d3cec2",
                    "divider": "#ece8df",
                    "green": "#44745a",
                    "greenSoft": { "hex": "#44745a", "opacity": 0.11 },
                    "orange": "#b96536",
                    "orangeSoft": { "hex": "#b96536", "opacity": 0.11 },
                    "purple": "#765b8f",
                    "purpleSoft": { "hex": "#765b8f", "opacity": 0.11 }
                },
                "dark": {
                    "brand": "#ec8668",
                    "brandHover": "#f29b80",
                    "brandSoft": { "hex": "#ec8668", "opacity": 0.14 },
                    "brandText": "#181412",
                    "bgWindow": "#191918",
                    "bgCard": "#232321",
                    "bgInput": { "hex": "#fffdf5", "opacity": 0.055 },
                    "bgHover": "#292825",
                    "bgActive": "#33312d",
                    "bgRaised": "#262522",
                    "bgToast": "#f8f5ed",
                    "textPrimary": "#f4f1e9",
                    "textSecondary": "#aaa69c",
                    "textTertiary": "#77746c",
                    "textToast": "#171716",
                    "border": "#32312d",
                    "borderStrong": "#48463f",
                    "divider": "#2c2b28",
                    "green": "#75aa86",
                    "greenSoft": { "hex": "#75aa86", "opacity": 0.14 },
                    "orange": "#dc9163",
                    "orangeSoft": { "hex": "#dc9163", "opacity": 0.14 },
                    "purple": "#ad8cc6",
                    "purpleSoft": { "hex": "#ad8cc6", "opacity": 0.14 }
                }
            },
            "metrics": { "cardRadius": 10, "controlRadius": 9, "rowPadding": 13, "shadowRadius": 8 },
            "typography": {
                "sectionTitleSize": 25,
                "uppercasesSectionTitles": false,
                "searchSize": 18,
                "searchUsesDisplayFont": true,
                "historyContentSize": 15
            },
            "fonts": { "display": "Songti SC", "reading": null },
            "structural": { "borderRadius": 10, "shadow": false }
        })
    }

    fn parse(json: &serde_json::Value) -> Result<ThemeFile, ThemeLoadError> {
        let bytes = serde_json::to_vec(json).unwrap();
        validate_theme_bytes(&bytes, "test.json")
    }

    #[test]
    fn valid_theme_parses_and_exposes_palette() {
        let theme = parse(&sample_theme_json()).expect("sample theme must be valid");
        assert_eq!(theme.id, "studio");
        assert_eq!(theme.format, 1);
        assert_eq!(theme.localized_name("zh-CN"), "工作室");
        assert_eq!(theme.localized_name("fr"), "Studio");
        assert_eq!(theme.palette_light().brand.hex, "#d5684b");
        assert_eq!(theme.palette_dark().brand.hex, "#ec8668");
        assert_eq!(theme.metrics.card_radius, 10.0);
        assert_eq!(theme.typography.section_title_size, 25.0);
        assert!(theme.structural.is_some());
    }

    #[test]
    fn color_shorthand_and_object_form_are_equivalent() {
        let mut json = sample_theme_json();
        json["palette"]["light"]["brand"] = serde_json::json!({ "hex": "#d5684b" });
        let theme = parse(&json).expect("object form without opacity must parse");
        assert_eq!(
            theme.palette_light().brand,
            ColorSpec {
                hex: "#d5684b".into(),
                opacity: None
            }
        );
    }

    #[test]
    fn format_must_be_one() {
        for bad in [0, 2, 3] {
            let mut json = sample_theme_json();
            json["format"] = serde_json::json!(bad);
            let error = parse(&json).expect_err("wrong format must fail");
            assert!(
                error.reason.contains("format"),
                "unexpected reason: {}",
                error.reason
            );
        }
        let mut json = sample_theme_json();
        json.as_object_mut().unwrap().remove("format");
        let error = parse(&json).expect_err("missing format must fail");
        assert!(error.reason.contains("format"));
    }

    #[test]
    fn id_rule_rejects_and_accepts() {
        for bad in [
            "MyTheme",
            "my_theme",
            "my.theme",
            "-theme",
            "my theme",
            "a".repeat(33).as_str(),
        ] {
            let mut json = sample_theme_json();
            json["id"] = serde_json::json!(bad);
            let error = parse(&json).expect_err("bad id must fail");
            assert!(
                error.reason.contains("id"),
                "unexpected reason: {}",
                error.reason
            );
        }
        for good in ["a", "0abc", "my-theme-2", "studio"] {
            let mut json = sample_theme_json();
            json["id"] = serde_json::json!(good);
            assert!(parse(&json).is_ok(), "id {good} must be accepted");
        }
    }

    #[test]
    fn name_requires_en_entry() {
        let mut json = sample_theme_json();
        json["name"] = serde_json::json!({ "zh-CN": "工作室" });
        let error = parse(&json).expect_err("missing en must fail");
        assert!(error.reason.contains("en"));
        json["name"] = serde_json::json!({});
        let error = parse(&json).expect_err("empty name must fail");
        assert!(error.reason.contains("en"));
    }

    #[test]
    fn name_key_pattern_and_value_length() {
        for bad_key in ["e", "zh_CN", "en-US-extra-long-key", "中文"] {
            let mut json = sample_theme_json();
            json["name"] = serde_json::json!({ "en": "Studio", bad_key: "Value" });
            let error = parse(&json).expect_err("bad name key must fail");
            assert!(
                error.reason.contains("name key"),
                "unexpected reason: {}",
                error.reason
            );
        }
        let mut json = sample_theme_json();
        json["name"] = serde_json::json!({ "en": "x".repeat(65) });
        let error = parse(&json).expect_err("65-char name must fail");
        assert!(error.reason.contains("character limit"));
        json["name"] = serde_json::json!({ "en": "x".repeat(64) });
        assert!(parse(&json).is_ok(), "64-char name must be accepted");
    }

    #[test]
    fn name_length_is_counted_in_characters_not_bytes() {
        // 64 ASCII characters pass, 65 fail.
        assert!(is_valid_name_value(&"a".repeat(64)));
        assert!(!is_valid_name_value(&"a".repeat(65)));
        // 64 Chinese characters pass, 65 fail (each is 3 UTF-8 bytes, so the
        // byte count alone would wrongly reject 22+ characters).
        let chinese = "编".repeat(64);
        assert_eq!(chinese.len(), 64 * 3);
        assert!(is_valid_name_value(&chinese));
        assert!(!is_valid_name_value(&"编".repeat(65)));
        // Emoji boundary: a single-scalar emoji is one character.
        assert!(is_valid_name_value(&"😀".repeat(64)));
        assert!(!is_valid_name_value(&"😀".repeat(65)));
        // A ZWJ-sequence emoji counts by its Unicode scalar values (7 here),
        // matching the documented "characters = Unicode scalars" semantics.
        assert_eq!("👨‍👩‍👧‍👦".chars().count(), 7);
        assert!(is_valid_name_value(&"👨‍👩‍👧‍👦".repeat(9))); // 63 scalars
        assert!(!is_valid_name_value(&"👨‍👩‍👧‍👦".repeat(10))); // 70 scalars
                                                         // Mixed scripts: 63 Chinese + 1 ASCII = 64 characters, passes even
                                                         // though it is 190 bytes.
        assert!(is_valid_name_value(&format!("{}a", "编".repeat(63))));
        // Empty values are invalid regardless of the byte limit.
        assert!(!is_valid_name_value(""));
    }

    #[test]
    fn name_validation_runs_inside_validate_theme_bytes() {
        // End-to-end: a 64-char Chinese name survives validate_theme_bytes,
        // a 65-char one is rejected with the character-limit reason.
        let mut json = sample_theme_json();
        json["name"] = serde_json::json!({ "en": "编".repeat(64) });
        let bytes = serde_json::to_vec(&json).unwrap();
        validate_theme_bytes(&bytes, "unicode-name.json").expect("64-char Chinese name must pass");
        json["name"] = serde_json::json!({ "en": "编".repeat(65) });
        let bytes = serde_json::to_vec(&json).unwrap();
        let error = validate_theme_bytes(&bytes, "unicode-name.json")
            .expect_err("65-char Chinese name must fail");
        assert!(error.reason.contains("character limit"));
    }

    #[test]
    fn hex_colours_are_validated() {
        for bad in ["red", "#fff", "#GGGGGG", "d5684b", "#d5684", "#d5684bff"] {
            let mut json = sample_theme_json();
            json["palette"]["light"]["brand"] = serde_json::json!(bad);
            let error = parse(&json).expect_err("bad hex must fail");
            assert!(
                error.reason.contains("--brand"),
                "unexpected reason: {}",
                error.reason
            );
        }
        let mut json = sample_theme_json();
        json["palette"]["light"]["brand"] = serde_json::json!("#D5684B");
        assert!(parse(&json).is_ok(), "uppercase hex must be accepted");
    }

    #[test]
    fn opacity_must_be_in_unit_range() {
        for bad in [1.5, -0.1, 2.0] {
            let mut json = sample_theme_json();
            json["palette"]["light"]["brandSoft"] =
                serde_json::json!({ "hex": "#d5684b", "opacity": bad });
            let error = parse(&json).expect_err("opacity out of range must fail");
            assert!(
                error.reason.contains("opacity"),
                "unexpected reason: {}",
                error.reason
            );
        }
        for good in [0.0, 0.045, 1.0] {
            let mut json = sample_theme_json();
            json["palette"]["light"]["brandSoft"] =
                serde_json::json!({ "hex": "#d5684b", "opacity": good });
            assert!(parse(&json).is_ok(), "opacity {good} must be accepted");
        }
    }

    #[test]
    fn metrics_ranges_are_enforced() {
        for (field, bad) in [
            ("cardRadius", 24.5),
            ("cardRadius", -1.0),
            ("controlRadius", 25.0),
            ("rowPadding", 3.0),
            ("rowPadding", 33.0),
            ("shadowRadius", 33.0),
            ("shadowRadius", -1.0),
        ] {
            let mut json = sample_theme_json();
            json["metrics"][field] = serde_json::json!(bad);
            let error = parse(&json).expect_err("bad metric must fail");
            assert!(
                error.reason.contains("metrics"),
                "unexpected reason: {}",
                error.reason
            );
        }
        let mut json = sample_theme_json();
        json["metrics"] = serde_json::json!({
            "cardRadius": 0, "controlRadius": 24, "rowPadding": 4, "shadowRadius": 0
        });
        assert!(parse(&json).is_ok(), "boundary metrics must be accepted");
        let mut json = sample_theme_json();
        json["metrics"] = serde_json::json!({
            "cardRadius": 24, "controlRadius": 0, "rowPadding": 32, "shadowRadius": 32
        });
        assert!(
            parse(&json).is_ok(),
            "upper-boundary metrics must be accepted"
        );
    }

    #[test]
    fn typography_sizes_are_enforced() {
        for (field, bad) in [
            ("sectionTitleSize", 8.0),
            ("sectionTitleSize", 33.0),
            ("searchSize", 8.0),
            ("searchSize", 33.0),
            ("historyContentSize", 8.0),
            ("historyContentSize", 33.0),
        ] {
            let mut json = sample_theme_json();
            json["typography"][field] = serde_json::json!(bad);
            let error = parse(&json).expect_err("bad type size must fail");
            assert!(
                error.reason.contains("typography"),
                "unexpected reason: {}",
                error.reason
            );
        }
        let mut json = sample_theme_json();
        json["typography"] = serde_json::json!({
            "sectionTitleSize": 9, "uppercasesSectionTitles": true,
            "searchSize": 32, "searchUsesDisplayFont": false, "historyContentSize": 9
        });
        assert!(
            parse(&json).is_ok(),
            "boundary sizes and booleans must be accepted"
        );
    }

    #[test]
    fn font_names_are_validated() {
        for bad in [
            "Bad;Font",
            "Bad\"Font",
            "Font\\Face",
            "x".repeat(65).as_str(),
        ] {
            let mut json = sample_theme_json();
            json["fonts"]["display"] = serde_json::json!(bad);
            let error = parse(&json).expect_err("bad font must fail");
            assert!(
                error.reason.contains("font"),
                "unexpected reason: {}",
                error.reason
            );
        }
        for good in [
            "Arial",
            "Songti SC",
            "Cascadia Code",
            "x".repeat(64).as_str(),
        ] {
            let mut json = sample_theme_json();
            json["fonts"]["display"] = serde_json::json!(good);
            assert!(parse(&json).is_ok(), "font {good} must be accepted");
        }
        let mut json = sample_theme_json();
        json["fonts"] = serde_json::json!({ "display": null, "reading": null });
        assert!(
            parse(&json).is_ok(),
            "null fonts must be accepted (system fallback)"
        );
        let mut json = sample_theme_json();
        json["fonts"]["display"] = serde_json::json!("");
        let error = parse(&json).expect_err("empty font name must fail");
        assert!(error.reason.contains("font"));
    }

    #[test]
    fn structural_is_optional_and_lenient() {
        let mut json = sample_theme_json();
        json.as_object_mut().unwrap().remove("structural");
        assert!(parse(&json).is_ok(), "structural must be optional");
        let mut json = sample_theme_json();
        json["structural"] = serde_json::json!({ "borderRadius": 0, "shadow": false, "glow": 2 });
        let theme = parse(&json).expect("supported keys plus unknown key must parse");
        let structural = theme.structural.as_ref().unwrap();
        assert_eq!(structural.border_radius, Some(0.0));
        assert_eq!(structural.shadow, Some(false));
        assert!(
            structural.ignored.contains_key("glow"),
            "unknown key must be captured for warning"
        );
        let mut json = sample_theme_json();
        json["structural"] = serde_json::json!({ "shadow": true });
        assert!(
            parse(&json).is_ok(),
            "shadow:true parses (ignored at apply time)"
        );
        let mut json = sample_theme_json();
        json["structural"] = serde_json::json!({ "borderRadius": -1 });
        let error = parse(&json).expect_err("negative border radius must fail");
        assert!(error.reason.contains("borderRadius"));
        let mut json = sample_theme_json();
        json["structural"] = serde_json::json!({ "borderRadius": 65 });
        let error = parse(&json).expect_err("oversized border radius must fail");
        assert!(error.reason.contains("borderRadius"));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let mut json = sample_theme_json();
        json["version"] = serde_json::json!("2.0");
        let error = parse(&json).expect_err("unknown top-level field must fail");
        assert!(
            error.reason.contains("version"),
            "unexpected reason: {}",
            error.reason
        );
        let mut json = sample_theme_json();
        json["palette"]["light"]["accent"] = serde_json::json!("#ffffff");
        let error =
            parse(&json).expect_err("Swift-side field name must fail (CSS token names only)");
        assert!(
            error.reason.contains("accent"),
            "unexpected reason: {}",
            error.reason
        );
    }

    #[test]
    fn both_palette_modes_are_required() {
        let mut json = sample_theme_json();
        json["palette"].as_object_mut().unwrap().remove("dark");
        let error = parse(&json).expect_err("missing dark palette must fail");
        assert!(error.reason.contains("dark"));
        let mut json = sample_theme_json();
        json["palette"].as_object_mut().unwrap().remove("light");
        let error = parse(&json).expect_err("missing light palette must fail");
        assert!(error.reason.contains("light"));
    }

    #[test]
    fn file_size_limit_is_enforced() {
        let mut bytes = serde_json::to_vec(&sample_theme_json()).unwrap();
        let padding = MAX_THEME_FILE_SIZE - bytes.len();
        bytes.extend(std::iter::repeat_n(b' ', padding));
        assert!(
            validate_theme_bytes(&bytes, "t.json").is_ok(),
            "exactly 64 KiB must pass"
        );
        bytes.push(b' ');
        let error = validate_theme_bytes(&bytes, "t.json").expect_err("over 64 KiB must fail");
        assert!(error.reason.contains("size limit"));
        assert_eq!(
            error.file, "t.json",
            "file name must be carried into the error"
        );
    }

    #[test]
    fn css_value_synthesis_matches_theming_doc_style() {
        let theme = parse(&sample_theme_json()).unwrap();
        let light = theme.palette_light();
        assert_eq!(light.brand.css_value(), "#d5684b");
        assert_eq!(light.brand_soft.css_value(), "rgba(213, 104, 75, 0.11)");
        assert_eq!(light.bg_input.css_value(), "rgba(26, 25, 22, 0.045)");
        assert_eq!(light.text_toast.css_value(), "#ffffff");
        // Malformed hex with opacity falls back to the raw value, never panics.
        let broken = ColorSpec {
            hex: "nope".into(),
            opacity: Some(0.5),
        };
        assert_eq!(broken.css_value(), "nope");
    }

    #[test]
    fn css_variables_cover_all_24_tokens_in_contract_order() {
        let theme = parse(&sample_theme_json()).unwrap();
        let pairs = theme.palette_light().css_variables();
        assert_eq!(
            pairs.len(),
            24,
            "THEMING.md §2.2 lists 24 CSS colour tokens"
        );
        assert_eq!(pairs[0].0, "--brand");
        assert_eq!(pairs[1].0, "--brand-hover");
        assert_eq!(pairs[10].0, "--bg-toast");
        assert_eq!(pairs[23].0, "--purple-soft");
        let names: Vec<_> = pairs.iter().map(|(name, _)| *name).collect();
        assert!(names.contains(&"--text-primary"));
        assert!(names.contains(&"--border-strong"));
        assert!(names.contains(&"--divider"));
        assert!(names.contains(&"--orange-soft"));
    }

    #[test]
    fn validated_theme_round_trips_through_json() {
        let theme = parse(&sample_theme_json()).unwrap();
        let encoded = serde_json::to_vec(&theme).unwrap();
        let decoded: ThemeFile = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, theme);
        // The serialized form keeps the object shape for colours with opacity.
        let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(value["palette"]["light"]["brandSoft"]["hex"], "#d5684b");
        assert_eq!(value["palette"]["light"]["brandSoft"]["opacity"], 0.11);
    }

    #[test]
    fn builtin_ids_are_reserved_and_constants_are_sane() {
        assert_eq!(
            BUILTIN_THEME_IDS,
            ["tailsync", "ocean", "forest", "rose", "high-contrast"]
        );
        for id in BUILTIN_THEME_IDS {
            assert!(is_builtin_id(id));
        }
        assert!(!is_builtin_id("studio"));
        assert_eq!(MAX_THEME_FILE_SIZE, 64 * 1024);
        assert_eq!(MAX_THEMES_PER_DIR, 32);
        assert_eq!(MAX_NAME_LENGTH, 64);
        assert_eq!(MAX_FONT_NAME_LENGTH, 64);
    }

    #[test]
    fn validation_never_panics_on_adversarial_input() {
        for input in [
            b"".as_slice(),
            b"{".as_slice(),
            b"null".as_slice(),
            b"[]".as_slice(),
            b"{\"format\":1}".as_slice(),
            b"\x00\x01\x02".as_slice(),
        ] {
            let result = validate_theme_bytes(input, "adversarial.json");
            assert!(result.is_err(), "input {:?} must fail cleanly", input);
        }
    }

    // ─── Discovery and listing ─────────────────────────────────────────

    fn temp_base() -> PathBuf {
        std::env::temp_dir().join(format!(
            "tailsync-themes-test-{:016x}",
            rand::random::<u64>()
        ))
    }

    fn write_theme_file(directory: &Path, file_name: &str, id: &str) {
        let mut json = sample_theme_json();
        json["id"] = serde_json::json!(id);
        json["name"] = serde_json::json!({ "en": id });
        let bytes = serde_json::to_vec(&json).unwrap();
        std::fs::write(directory.join(file_name), bytes).unwrap();
    }

    #[test]
    fn list_empty_directory_yields_no_entries_and_creates_dir() {
        let base = temp_base();
        let listing = list_themes_at(&base);
        assert!(listing.entries.is_empty());
        assert!(listing.errors.is_empty());
        assert!(themes_dir_at(&base).is_dir(), "themes dir must be created");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn list_valid_themes_sorted_and_round_trips() {
        let base = temp_base();
        let directory = ensure_themes_dir(&base).unwrap();
        write_theme_file(&directory, "b.json", "beta");
        write_theme_file(&directory, "a.json", "alpha");
        let listing = list_themes_at(&base);
        assert!(
            listing.errors.is_empty(),
            "unexpected errors: {:?}",
            listing.errors
        );
        let ids: Vec<&str> = listing
            .entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect();
        assert_eq!(
            ids,
            ["alpha", "beta"],
            "entries must be sorted by file name"
        );
        let entry = &listing.entries[0];
        assert_eq!(entry.file, "a.json");
        assert_eq!(entry.name.get("en").map(String::as_str), Some("alpha"));
        assert_eq!(entry.metrics.card_radius, 10.0);
        assert_eq!(entry.palette.light.brand.hex, "#d5684b");
        // The daemon serializes entries; the shape must survive a round trip.
        let encoded = serde_json::to_string(entry).unwrap();
        let decoded: ThemeEntry = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, *entry);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn list_skips_bad_json_but_reports_it() {
        let base = temp_base();
        let directory = ensure_themes_dir(&base).unwrap();
        write_theme_file(&directory, "good.json", "good");
        std::fs::write(directory.join("bad.json"), b"not json at all").unwrap();
        let listing = list_themes_at(&base);
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].id, "good");
        assert_eq!(listing.errors.len(), 1);
        assert_eq!(listing.errors[0].file, "bad.json");
        assert!(listing.errors[0].reason.contains("Invalid theme JSON"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn list_ignores_subdirectories_and_non_json_files() {
        let base = temp_base();
        let directory = ensure_themes_dir(&base).unwrap();
        write_theme_file(&directory, "theme.json", "ok");
        std::fs::write(directory.join("readme.txt"), b"hello").unwrap();
        let sub = directory.join("nested");
        std::fs::create_dir_all(&sub).unwrap();
        write_theme_file(&sub, "hidden.json", "hidden");
        let listing = list_themes_at(&base);
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].id, "ok");
        assert!(listing.errors.is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn list_rejects_builtin_id_conflict() {
        let base = temp_base();
        let directory = ensure_themes_dir(&base).unwrap();
        write_theme_file(&directory, "clash.json", "tailsync");
        let listing = list_themes_at(&base);
        assert!(listing.entries.is_empty());
        assert_eq!(listing.errors.len(), 1);
        assert_eq!(listing.errors[0].file, "clash.json");
        assert!(listing.errors[0].reason.contains("reserved"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn list_rejects_duplicate_ids_across_files() {
        let base = temp_base();
        let directory = ensure_themes_dir(&base).unwrap();
        write_theme_file(&directory, "dup-a.json", "same");
        write_theme_file(&directory, "dup-b.json", "same");
        let listing = list_themes_at(&base);
        assert_eq!(listing.entries.len(), 1, "first (sorted) file wins");
        assert_eq!(listing.entries[0].file, "dup-a.json");
        assert_eq!(listing.errors.len(), 1);
        assert!(listing.errors[0].reason.contains("Duplicate theme id"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn list_enforces_directory_count_limit_and_skips_extra_files() {
        let base = temp_base();
        let directory = ensure_themes_dir(&base).unwrap();
        for index in 0..MAX_THEMES_PER_DIR {
            write_theme_file(
                &directory,
                &format!("t{index:02}.json"),
                &format!("t{index:02}"),
            );
        }
        // The 33rd file (sorted last) is garbage: it must be skipped with the
        // limit error, not parsed.
        std::fs::write(directory.join("zz.json"), b"garbage").unwrap();
        let listing = list_themes_at(&base);
        assert_eq!(listing.entries.len(), MAX_THEMES_PER_DIR);
        assert_eq!(listing.errors.len(), 1);
        assert_eq!(listing.errors[0].file, "zz.json");
        assert!(
            listing.errors[0].reason.contains("limit"),
            "unexpected reason: {}",
            listing.errors[0].reason
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn list_rejects_oversized_file_without_reading_it() {
        let base = temp_base();
        let directory = ensure_themes_dir(&base).unwrap();
        // Past the absolute (image-capable) ceiling: rejected at the metadata
        // pre-check before any read.
        let oversized = vec![b' '; MAX_THEME_FILE_SIZE_WITH_IMAGE + 1];
        std::fs::write(directory.join("big.json"), oversized).unwrap();
        let listing = list_themes_at(&base);
        assert!(listing.entries.is_empty());
        assert_eq!(listing.errors.len(), 1);
        assert!(listing.errors[0].reason.contains("size limit"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn themes_dir_is_created_with_0700_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let base = temp_base();
        let directory = ensure_themes_dir(&base).unwrap();
        let mode = std::fs::metadata(&directory).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "themes dir must be owner-only");
        // Idempotent: a second ensure keeps the permissions intact.
        ensure_themes_dir(&base).unwrap();
        let mode = std::fs::metadata(&directory).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn themes_dir_name_is_stable() {
        let base = PathBuf::from("/tmp/example-base");
        assert_eq!(themes_dir_at(&base), base.join("themes"));
        assert_eq!(THEMES_DIRECTORY_NAME, "themes");
    }

    // ─── Import and delete ─────────────────────────────────────────────

    fn theme_bytes_with_id(id: &str) -> Vec<u8> {
        let mut json = sample_theme_json();
        json["id"] = serde_json::json!(id);
        json["name"] = serde_json::json!({ "en": id });
        serde_json::to_vec(&json).unwrap()
    }

    fn write_valid_theme_src(directory: &Path, name: &str, id: &str) -> PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, theme_bytes_with_id(id)).unwrap();
        path
    }

    #[test]
    fn import_copies_theme_independently_of_source() {
        let base = temp_base();
        let src_dir = base.join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = write_valid_theme_src(&src_dir, "studio.json", "studio");
        let entry = import_theme_file_at(&src, &base).unwrap();
        assert_eq!(entry.id, "studio");
        assert_eq!(entry.file, "studio.json");
        assert!(themes_dir_at(&base).join("studio.json").is_file());
        // The stored copy is independent: deleting the source does not
        // affect the imported theme.
        std::fs::remove_file(&src).unwrap();
        let listing = list_themes_at(&base);
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].id, "studio");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn import_rejects_duplicate_ids() {
        let base = temp_base();
        let src_dir = base.join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();
        let first = write_valid_theme_src(&src_dir, "one.json", "studio");
        import_theme_file_at(&first, &base).unwrap();
        // Same id, different source file name.
        let second = write_valid_theme_src(&src_dir, "two.json", "studio");
        let error = import_theme_file_at(&second, &base).unwrap_err();
        assert!(error.contains("already exists"), "unexpected: {error}");
        // Same id hand-placed under a different file name.
        let direct = themes_dir_at(&base).join("custom.json");
        std::fs::write(&direct, theme_bytes_with_id("handy")).unwrap();
        let third = write_valid_theme_src(&src_dir, "three.json", "handy");
        let error = import_theme_file_at(&third, &base).unwrap_err();
        assert!(error.contains("already exists"), "unexpected: {error}");
        // Target file name collision even with an unparseable file.
        std::fs::write(themes_dir_at(&base).join("other.json"), b"garbage").unwrap();
        let fourth = write_valid_theme_src(&src_dir, "four.json", "other");
        let error = import_theme_file_at(&fourth, &base).unwrap_err();
        assert!(error.contains("already exists"), "unexpected: {error}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn import_rejects_builtin_ids_and_bad_sources() {
        let base = temp_base();
        let src_dir = base.join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();
        let builtin = write_valid_theme_src(&src_dir, "builtin.json", "tailsync");
        let error = import_theme_file_at(&builtin, &base).unwrap_err();
        assert!(error.contains("reserved"), "unexpected: {error}");
        std::fs::write(src_dir.join("bad.json"), b"garbage").unwrap();
        let error = import_theme_file_at(&src_dir.join("bad.json"), &base).unwrap_err();
        assert!(error.contains("Invalid theme JSON"), "unexpected: {error}");
        let error = import_theme_file_at(&src_dir.join("missing.json"), &base).unwrap_err();
        assert!(error.contains("Could not read"), "unexpected: {error}");
        let txt = write_valid_theme_src(&src_dir, "notes.txt", "txttheme");
        let error = import_theme_file_at(&txt, &base).unwrap_err();
        assert!(error.contains("only .json"), "unexpected: {error}");
        // Rejected imports never create or touch the themes directory.
        assert!(!themes_dir_at(&base).exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn import_writes_only_inside_themes_dir() {
        let base = temp_base();
        let src_dir = base.join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = write_valid_theme_src(&src_dir, "escape.json", "escape");
        import_theme_file_at(&src, &base).unwrap();
        // The copy lives exactly at {base}/themes/escape.json.
        assert!(themes_dir_at(&base).join("escape.json").is_file());
        // Nothing new appeared next to the source...
        let siblings: Vec<_> = std::fs::read_dir(&src_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(siblings.len(), 1);
        assert_eq!(siblings[0].to_str().unwrap(), "escape.json");
        // ...and the base dir holds only "sources" and "themes".
        let base_entries: Vec<_> = std::fs::read_dir(&base)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(base_entries.len(), 2);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn import_enforces_directory_limit() {
        let base = temp_base();
        let directory = ensure_themes_dir(&base).unwrap();
        for index in 0..MAX_THEMES_PER_DIR {
            std::fs::write(
                directory.join(format!("t{index:02}.json")),
                theme_bytes_with_id(&format!("t{index:02}")),
            )
            .unwrap();
        }
        let src_dir = base.join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = write_valid_theme_src(&src_dir, "extra.json", "extra");
        let error = import_theme_file_at(&src, &base).unwrap_err();
        assert!(error.contains("maximum"), "unexpected: {error}");
        assert!(!themes_dir_at(&base).join("extra.json").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn delete_removes_theme_and_reports_missing() {
        let base = temp_base();
        let src_dir = base.join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = write_valid_theme_src(&src_dir, "gone.json", "gone");
        import_theme_file_at(&src, &base).unwrap();
        delete_theme_at("gone", &base).unwrap();
        assert!(!themes_dir_at(&base).join("gone.json").exists());
        assert!(list_themes_at(&base).entries.is_empty());
        let error = delete_theme_at("gone", &base).unwrap_err();
        assert!(error.contains("not found"), "unexpected: {error}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn delete_rejects_builtin_invalid_and_traversal_ids() {
        let base = temp_base();
        std::fs::create_dir_all(&base).unwrap();
        for bad in BUILTIN_THEME_IDS {
            let error = delete_theme_at(bad, &base).unwrap_err();
            assert!(error.contains("reserved"), "unexpected: {error}");
        }
        for bad in ["../evil", "a/b", "BAD", "..", "..\\evil", "evil.json"] {
            let error = delete_theme_at(bad, &base).unwrap_err();
            assert!(error.contains("Invalid theme id"), "unexpected: {error}");
        }
        // Traversal attempts never touch files outside the themes directory.
        let outside = base.join("evil.json");
        std::fs::write(&outside, b"keep me").unwrap();
        delete_theme_at("../evil", &base).unwrap_err();
        assert!(
            outside.is_file(),
            "file outside themes dir must be untouched"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn concurrent_import_of_same_id_yields_exactly_one_success() {
        let base = temp_base();
        let src_dir = base.join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();
        const THREADS: usize = 8;
        // Every source declares the same id but carries a distinct name
        // value, so the winner is identifiable by the stored bytes.
        let mut source_bytes: Vec<Vec<u8>> = Vec::new();
        let mut sources: Vec<PathBuf> = Vec::new();
        for index in 0..THREADS {
            let mut json = sample_theme_json();
            json["id"] = serde_json::json!("duel");
            json["name"] = serde_json::json!({ "en": format!("duel-{index}") });
            let bytes = serde_json::to_vec(&json).unwrap();
            let path = src_dir.join(format!("duel-{index}.json"));
            std::fs::write(&path, &bytes).unwrap();
            source_bytes.push(bytes);
            sources.push(path);
        }
        let mut handles = Vec::new();
        for src in sources {
            let base = base.clone();
            handles.push(std::thread::spawn(move || {
                import_theme_file_at(&src, &base)
            }));
        }
        let results: Vec<Result<ThemeEntry, String>> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        let successes = results.iter().filter(|result| result.is_ok()).count();
        let errors = results.iter().filter(|result| result.is_err()).count();
        assert_eq!(successes, 1, "exactly one concurrent import must win");
        assert_eq!(errors, THREADS - 1);
        for result in &results {
            if let Err(error) = result {
                assert!(error.contains("already exists"), "unexpected: {error}");
            }
        }
        // The installed file is byte-identical to the winning source and
        // fully parseable — never a half-written mixture.
        let installed = std::fs::read(themes_dir_at(&base).join("duel.json")).unwrap();
        assert!(
            source_bytes.iter().any(|bytes| bytes == &installed),
            "installed file must equal exactly one source"
        );
        let listing = list_themes_at(&base);
        assert_eq!(listing.entries.len(), 1);
        assert!(listing.entries[0].id == "duel");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn concurrent_import_of_distinct_ids_respects_directory_limit() {
        let base = temp_base();
        let src_dir = base.join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();
        let mut sources = Vec::new();
        for index in 0..MAX_THEMES_PER_DIR + 8 {
            let id = format!("c{index:02}");
            let path = src_dir.join(format!("{id}.json"));
            std::fs::write(&path, theme_bytes_with_id(&id)).unwrap();
            sources.push(path);
        }
        let mut handles = Vec::new();
        for src in sources {
            let base = base.clone();
            handles.push(std::thread::spawn(move || {
                import_theme_file_at(&src, &base)
            }));
        }
        let results: Vec<Result<ThemeEntry, String>> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        let successes = results.iter().filter(|result| result.is_ok()).count();
        let errors = results.iter().filter(|result| result.is_err()).count();
        assert_eq!(
            successes, MAX_THEMES_PER_DIR,
            "serialized count check must cap installs"
        );
        assert_eq!(errors, 8);
        for result in &results {
            if let Err(error) = result {
                assert!(error.contains("maximum"), "unexpected: {error}");
            }
        }
        let listing = list_themes_at(&base);
        assert_eq!(listing.entries.len(), MAX_THEMES_PER_DIR);
        // Every installed file is complete and valid — concurrent imports
        // never expose half-written targets to the listing.
        for entry in &listing.entries {
            let loaded = load_theme_file(&themes_dir_at(&base).join(&entry.file), &entry.file)
                .expect("listed entry must be fully readable");
            assert_eq!(loaded.id, entry.id);
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn concurrent_import_and_delete_of_same_id_never_torn() {
        let base = temp_base();
        let src_dir = base.join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();
        let original = write_valid_theme_src(&src_dir, "tug.json", "tug");
        import_theme_file_at(&original, &base).unwrap();
        let replacement = write_valid_theme_src(&src_dir, "tug2.json", "tug");
        let replacement_for_thread = replacement.clone();
        let base_a = base.clone();
        let base_b = base.clone();
        let import_handle =
            std::thread::spawn(move || import_theme_file_at(&replacement_for_thread, &base_a));
        let delete_handle = std::thread::spawn(move || delete_theme_at("tug", &base_b));
        let import_result = import_handle.join().unwrap();
        let delete_result = delete_handle.join().unwrap();
        // The lock serializes the two operations into one of two atomic
        // orders, each of which leaves a consistent state:
        //   delete first  → delete ok, then import installs a fresh copy;
        //   import first  → import reports "already exists", delete removes.
        // The observable contract: delete always succeeds (the file exists
        // at the start), and the final file exists exactly when the import
        // won — never a torn/half-written target.
        assert!(delete_result.is_ok(), "delete must win: {delete_result:?}");
        let import_ok = import_result.is_ok();
        let target = themes_dir_at(&base).join("tug.json");
        assert_eq!(target.exists(), import_ok);
        if import_ok {
            // A fresh, complete copy was installed (this source's bytes).
            assert_eq!(
                std::fs::read(&target).unwrap(),
                std::fs::read(&replacement).unwrap()
            );
            // And it is fully parseable by the loader.
            load_theme_file(&target, "tug.json").expect("installed copy must parse");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn import_leaves_no_temp_files_and_temps_are_invisible_to_listings() {
        let base = temp_base();
        let directory = ensure_themes_dir(&base).unwrap();
        // Leftover temporary files (from an imagined crash) and the lock
        // file must never surface in listings.
        std::fs::write(directory.join(".studio.json.tmp-deadbeef"), b"half-written").unwrap();
        std::fs::write(directory.join("a.json"), theme_bytes_with_id("alpha")).unwrap();
        let names: Vec<String> = list_themes_at(&base)
            .entries
            .iter()
            .map(|entry| entry.file.clone())
            .collect();
        assert_eq!(
            names,
            vec!["a.json"],
            "temp and lock files must be invisible"
        );
        // Success path: no temporary files remain after an import.
        let src_dir = base.join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = write_valid_theme_src(&src_dir, "fresh.json", "fresh");
        import_theme_file_at(&src, &base).unwrap();
        // Failure path: a rejected (conflicting) import also leaves no new
        // temp files behind; the pre-existing foreign leftover stays
        // untouched (import only ever removes its own temporary file).
        let dup = write_valid_theme_src(&src_dir, "dup.json", "fresh");
        let error = import_theme_file_at(&dup, &base).unwrap_err();
        assert!(error.contains("already exists"), "unexpected: {error}");
        let leftovers: Vec<String> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp") && name != ".studio.json.tmp-deadbeef")
            .collect();
        assert!(
            leftovers.is_empty(),
            "import temps must be cleaned up: {leftovers:?}"
        );
        assert!(directory.join(".studio.json.tmp-deadbeef").is_file());
        assert!(directory.join(THEMES_LOCK_FILE).is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn import_never_overwrites_an_existing_target() {
        let base = temp_base();
        let src_dir = base.join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();
        let original = write_valid_theme_src(&src_dir, "keep.json", "keep");
        import_theme_file_at(&original, &base).unwrap();
        let original_bytes = std::fs::read(themes_dir_at(&base).join("keep.json")).unwrap();
        // A different source with the same id must be rejected and must not
        // touch the stored copy.
        let mut json = sample_theme_json();
        json["id"] = serde_json::json!("keep");
        json["name"] = serde_json::json!({ "en": "Keep (replacement)" });
        let replacement_path = src_dir.join("keep2.json");
        std::fs::write(&replacement_path, serde_json::to_vec(&json).unwrap()).unwrap();
        let error = import_theme_file_at(&replacement_path, &base).unwrap_err();
        assert!(error.contains("already exists"), "unexpected: {error}");
        assert_eq!(
            std::fs::read(themes_dir_at(&base).join("keep.json")).unwrap(),
            original_bytes,
            "existing target must be byte-identical after a rejected import"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    // ─── Image header parsing (R002) ───────────────────────────────────

    /// The well-known 1×1 transparent PNG (67 bytes), embedded as base64 so
    /// the byte sequence cannot drift from a real file.
    const ONE_BY_ONE_PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNgAAH//wMAAf8CAqU9iVkAAAAASUVORK5CYII=";

    /// Build a structurally valid PNG header (signature + IHDR) declaring the
    /// given dimensions. Only the header is present — enough for the parser.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(&13u32.to_be_bytes()); // IHDR chunk length
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]); // bit depth, colour type, etc.
        bytes.extend_from_slice(&[0, 0, 0, 0]); // CRC (unused by the parser)
        bytes
    }

    /// Build a structurally valid JPEG header: SOI + APP0 (JFIF) + SOF0 with
    /// the given dimensions + EOI. No pixel data — header parse only.
    fn jpeg_header(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8]; // SOI
                                          // APP0 "JFIF\0" segment
        bytes.extend_from_slice(&[0xFF, 0xE0]);
        bytes.extend_from_slice(&16u16.to_be_bytes());
        bytes.extend_from_slice(b"JFIF\0");
        bytes.extend_from_slice(&[1, 1, 0, 0, 1, 0, 1, 0, 0]);
        // SOF0 segment: len=11, precision 8, height, width, 1 component
        bytes.extend_from_slice(&[0xFF, 0xC0]);
        bytes.extend_from_slice(&11u16.to_be_bytes());
        bytes.push(8);
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&[0, 1, 0x11, 0]);
        bytes.extend_from_slice(&[0xFF, 0xD9]); // EOI
        bytes
    }

    #[test]
    fn image_header_accepts_a_real_png() {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(ONE_BY_ONE_PNG_B64)
            .expect("embedded base64 must decode");
        let dims =
            validate_image_payload(&bytes, ImageMime::Png).expect("real 1x1 PNG header must pass");
        assert_eq!(
            dims,
            ImageDimensions {
                width: 1,
                height: 1
            }
        );
    }

    #[test]
    fn image_header_accepts_a_synthetic_jpeg() {
        let dims = validate_image_payload(&jpeg_header(640, 480), ImageMime::Jpeg)
            .expect("synthetic JPEG header must pass");
        assert_eq!(
            dims,
            ImageDimensions {
                width: 640,
                height: 480
            }
        );
    }

    #[test]
    fn image_header_rejects_magic_mime_mismatch() {
        let png = png_header(10, 10);
        let jpeg = jpeg_header(10, 10);
        let error = validate_image_payload(&png, ImageMime::Jpeg).unwrap_err();
        assert!(error.contains("SOI"), "unexpected: {error}");
        let error = validate_image_payload(&jpeg, ImageMime::Png).unwrap_err();
        assert!(error.contains("PNG"), "unexpected: {error}");
        for mime in [ImageMime::Png, ImageMime::Jpeg] {
            let error = validate_image_payload(b"not an image at all", mime).unwrap_err();
            assert!(!error.is_empty());
        }
    }

    #[test]
    fn image_header_rejects_small_file_huge_dimension_bombs() {
        // Per-side bomb: 6001px on one side.
        let error = validate_image_payload(&png_header(6001, 1), ImageMime::Png).unwrap_err();
        assert!(error.contains("per-side"), "unexpected: {error}");
        let error = validate_image_payload(&jpeg_header(1, 6001), ImageMime::Jpeg).unwrap_err();
        assert!(error.contains("per-side"), "unexpected: {error}");
        // Pixel-count bomb: 6000 x 6000 = 36 MP, both sides within the
        // per-side limit — only the megapixel cap rejects it.
        let error = validate_image_payload(&png_header(6000, 6000), ImageMime::Png).unwrap_err();
        assert!(error.contains("megapixel"), "unexpected: {error}");
        let error = validate_image_payload(&jpeg_header(6000, 6000), ImageMime::Jpeg).unwrap_err();
        assert!(error.contains("megapixel"), "unexpected: {error}");
        // Zero dimension.
        let error = validate_image_payload(&png_header(0, 100), ImageMime::Png).unwrap_err();
        assert!(error.contains("zero dimension"), "unexpected: {error}");
        let error = validate_image_payload(&jpeg_header(100, 0), ImageMime::Jpeg).unwrap_err();
        assert!(error.contains("zero dimension"), "unexpected: {error}");
    }

    #[test]
    fn image_header_accepts_boundary_dimensions() {
        // Exactly 6000 x 4000 = 24 MP — the acceptance boundary.
        let dims = validate_image_payload(&png_header(6000, 4000), ImageMime::Png)
            .expect("boundary PNG must pass");
        assert_eq!(
            dims,
            ImageDimensions {
                width: 6000,
                height: 4000
            }
        );
        let dims = validate_image_payload(&jpeg_header(6000, 4000), ImageMime::Jpeg)
            .expect("boundary JPEG must pass");
        assert_eq!(
            dims,
            ImageDimensions {
                width: 6000,
                height: 4000
            }
        );
        // 6000 x 4001 = 24,006,000 — just over.
        let error = validate_image_payload(&png_header(6000, 4001), ImageMime::Png).unwrap_err();
        assert!(error.contains("megapixel"), "unexpected: {error}");
    }

    #[test]
    fn image_header_rejects_truncated_and_malformed_streams() {
        // PNG: signature only; signature + partial IHDR; wrong first chunk.
        assert!(validate_image_payload(&PNG_SIGNATURE, ImageMime::Png).is_err());
        let mut short = PNG_SIGNATURE.to_vec();
        short.extend_from_slice(&13u32.to_be_bytes());
        short.extend_from_slice(b"IHDR");
        short.extend_from_slice(&1u32.to_be_bytes()); // only half the IHDR data
        assert!(validate_image_payload(&short, ImageMime::Png).is_err());
        let mut wrong_chunk = PNG_SIGNATURE.to_vec();
        wrong_chunk.extend_from_slice(&13u32.to_be_bytes());
        wrong_chunk.extend_from_slice(b"PLTE");
        wrong_chunk.extend_from_slice(&[0; 13]);
        let error = validate_image_payload(&wrong_chunk, ImageMime::Png).unwrap_err();
        assert!(error.contains("IHDR"), "unexpected: {error}");
        // JPEG: SOI + EOI without any SOF; SOS before SOF; zero-length
        // segment (must error, never loop); garbage after SOI.
        assert!(validate_image_payload(&[0xFF, 0xD8, 0xFF, 0xD9], ImageMime::Jpeg).is_err());
        let sos_before_sof = [0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02];
        let error = validate_image_payload(&sos_before_sof, ImageMime::Jpeg).unwrap_err();
        assert!(error.contains("no SOF"), "unexpected: {error}");
        let zero_length = [0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x00, 0xFF, 0xC0];
        let error = validate_image_payload(&zero_length, ImageMime::Jpeg).unwrap_err();
        assert!(error.contains("segment length"), "unexpected: {error}");
        assert!(validate_image_payload(&[0xFF, 0xD8, 0x12, 0x34], ImageMime::Jpeg).is_err());
        assert!(validate_image_payload(&[0xFF], ImageMime::Jpeg).is_err());
        assert!(validate_image_payload(&[], ImageMime::Jpeg).is_err());
    }

    #[test]
    fn image_mime_parse_round_trips_and_rejects_unsafe_types() {
        assert_eq!(ImageMime::parse("image/png"), Some(ImageMime::Png));
        assert_eq!(ImageMime::parse("image/jpeg"), Some(ImageMime::Jpeg));
        assert_eq!(ImageMime::Png.mime_type(), "image/png");
        assert_eq!(ImageMime::Jpeg.mime_type(), "image/jpeg");
        for unsafe_type in ["image/svg+xml", "image/gif", "image/webp", "text/html", ""] {
            assert_eq!(
                ImageMime::parse(unsafe_type),
                None,
                "{unsafe_type} must be rejected"
            );
        }
    }

    // ─── Background spec validation (R001) ─────────────────────────────────

    /// Build the `background` JSON for a theme with an optional light-mode image
    /// (base64-encoded PNG/JPEG header bytes). Used by the R001 acceptance tests.
    fn background_json(
        light_image_b64: Option<&str>,
        light_mime: &str,
        light_scrim_opacity: Option<f64>,
        dark_image_b64: Option<&str>,
        dark_mime: &str,
    ) -> serde_json::Value {
        let mut light = serde_json::Map::new();
        if let Some(b64) = light_image_b64 {
            light.insert(
                "image".into(),
                serde_json::json!({ "mimeType": light_mime, "dataB64": b64 }),
            );
        }
        if let Some(opacity) = light_scrim_opacity {
            light.insert(
                "scrim".into(),
                serde_json::json!({ "hex": "#0f1526", "opacity": opacity }),
            );
        }
        let mut dark = serde_json::Map::new();
        if let Some(b64) = dark_image_b64 {
            dark.insert(
                "image".into(),
                serde_json::json!({ "mimeType": dark_mime, "dataB64": b64 }),
            );
            dark.insert(
                "scrim".into(),
                serde_json::json!({ "hex": "#0f1526", "opacity": 0.82 }),
            );
        }
        let mut background = serde_json::Map::new();
        background.insert("light".into(), serde_json::Value::Object(light));
        if !dark.is_empty() {
            background.insert("dark".into(), serde_json::Value::Object(dark));
        }
        serde_json::Value::Object(background)
    }

    fn theme_with_background(bg: serde_json::Value) -> serde_json::Value {
        let mut json = sample_theme_json();
        json["background"] = bg;
        json
    }

    #[test]
    fn background_with_valid_png_passes() {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(png_header(800, 600));
        let json = theme_with_background(background_json(
            Some(&b64),
            "image/png",
            Some(0.82),
            None,
            "",
        ));
        let theme = parse(&json).expect("PNG background theme must pass");
        let background = theme.background.as_ref().expect("background present");
        let light = background.light.as_ref().expect("light mode present");
        assert_eq!(
            light.image.as_ref().expect("image").mime_type,
            ImageMime::Png
        );
        assert_eq!(light.scrim.as_ref().expect("scrim").hex, "#0f1526");
        assert_eq!(light.scrim.as_ref().unwrap().opacity, Some(0.82));
        assert!(theme.has_background_image());
        assert!(background.dark.is_none(), "dark side may be absent");
    }

    #[test]
    fn background_with_valid_jpeg_passes() {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(jpeg_header(640, 480));
        let json = theme_with_background(background_json(
            Some(&b64),
            "image/jpeg",
            Some(0.5),
            None,
            "",
        ));
        assert!(parse(&json).is_ok(), "JPEG background theme must pass");
    }

    #[test]
    fn background_rejects_missing_scrim() {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(png_header(10, 10));
        let json = theme_with_background(background_json(Some(&b64), "image/png", None, None, ""));
        let error = parse(&json).expect_err("image without scrim must fail");
        assert!(
            error.reason.contains("scrim is required"),
            "unexpected: {}",
            error.reason
        );
    }

    #[test]
    fn background_rejects_scrim_without_image() {
        let json = theme_with_background(background_json(None, "", Some(0.82), None, ""));
        let error = parse(&json).expect_err("scrim without image must fail");
        assert!(
            error.reason.contains("scrim requires an image"),
            "unexpected: {}",
            error.reason
        );
    }

    #[test]
    fn background_scrim_opacity_must_be_in_range() {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(png_header(10, 10));
        for bad in [0.49, 0.96, 1.5, -0.1] {
            let json = theme_with_background(background_json(
                Some(&b64),
                "image/png",
                Some(bad),
                None,
                "",
            ));
            let error = parse(&json).expect_err("out-of-range scrim opacity must fail");
            assert!(
                error.reason.contains("scrim opacity"),
                "unexpected: {}",
                error.reason
            );
        }
        for good in [0.5, 0.82, 0.95] {
            let json = theme_with_background(background_json(
                Some(&b64),
                "image/png",
                Some(good),
                None,
                "",
            ));
            assert!(parse(&json).is_ok(), "scrim opacity {good} must pass");
        }
        // Missing opacity entirely.
        let mut json =
            theme_with_background(background_json(Some(&b64), "image/png", None, None, ""));
        json["background"]["light"]["scrim"] = serde_json::json!({ "hex": "#0f1526" });
        let error = parse(&json).expect_err("missing scrim opacity must fail");
        assert!(
            error.reason.contains("scrim opacity"),
            "unexpected: {}",
            error.reason
        );
    }

    #[test]
    fn background_rejects_invalid_base64() {
        let json = theme_with_background(background_json(
            Some("!!!not-base64!!!"),
            "image/png",
            Some(0.82),
            None,
            "",
        ));
        let error = parse(&json).expect_err("invalid base64 must fail");
        assert!(
            error.reason.contains("not valid base64"),
            "unexpected: {}",
            error.reason
        );
    }

    #[test]
    fn background_rejects_oversized_image_payload() {
        use base64::Engine;
        // A file carrying a >3 MiB image always trips the 4 MiB file cap first
        // (base64 expands 4/3), so the image byte cap is exercised directly
        // against the validation function.
        let mut payload = png_header(100, 100);
        payload.extend(std::iter::repeat_n(
            0u8,
            MAX_IMAGE_BYTES - payload.len() + 1,
        ));
        let mode = BackgroundMode {
            image: Some(ImageSpec {
                mime_type: ImageMime::Png,
                data_b64: base64::engine::general_purpose::STANDARD.encode(&payload),
            }),
            scrim: Some(ColorSpec {
                hex: "#0f1526".into(),
                opacity: Some(0.82),
            }),
        };
        let error = validate_background(Some(&ThemeBackground {
            light: Some(mode),
            dark: None,
        }))
        .expect_err(">3MiB image must fail");
        assert!(error.contains("byte limit"), "unexpected: {error}");
    }

    #[test]
    fn background_rejects_oversized_theme_file() {
        // A theme with an image whose base64 alone pushes the file past the
        // 4 MiB absolute cap: rejected at the pre-parse size check.
        let mut json = theme_with_background(background_json(
            Some("AAAA"),
            "image/png",
            Some(0.82),
            None,
            "",
        ));
        json["name"]["en"] = serde_json::json!("x".repeat(MAX_THEME_FILE_SIZE_WITH_IMAGE));
        let bytes = serde_json::to_vec(&json).unwrap();
        assert!(bytes.len() > MAX_THEME_FILE_SIZE_WITH_IMAGE);
        let error = validate_theme_bytes(&bytes, "big.json").expect_err(">4MiB file must fail");
        assert!(
            error.reason.contains("size limit"),
            "unexpected: {}",
            error.reason
        );
    }

    #[test]
    fn background_keeps_the_64k_cap_for_imageless_themes() {
        // Regression: an imageless theme larger than 64 KiB stays rejected even
        // though the absolute cap grew to 4 MiB.
        let mut bytes = serde_json::to_vec(&sample_theme_json()).unwrap();
        let padding = MAX_THEME_FILE_SIZE - bytes.len() + 1;
        bytes.extend(std::iter::repeat_n(b' ', padding));
        let error =
            validate_theme_bytes(&bytes, "big-plain.json").expect_err("imageless >64KiB must fail");
        assert!(
            error.reason.contains("size limit"),
            "unexpected: {}",
            error.reason
        );
    }

    #[test]
    fn background_unknown_fields_are_rejected() {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(png_header(10, 10));
        let mut json = theme_with_background(background_json(
            Some(&b64),
            "image/png",
            Some(0.82),
            None,
            "",
        ));
        json["background"]["light"]["repeat"] = serde_json::json!("tile");
        let error = parse(&json).expect_err("unknown background key must fail");
        assert!(
            error.reason.contains("repeat"),
            "unexpected: {}",
            error.reason
        );
    }

    #[test]
    fn background_rejects_unsafe_mime_types() {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(png_header(10, 10));
        for unsafe_type in ["image/svg+xml", "image/gif", "image/webp"] {
            let json = theme_with_background(background_json(
                Some(&b64),
                unsafe_type,
                Some(0.82),
                None,
                "",
            ));
            let error = parse(&json).expect_err("{unsafe_type} must fail");
            assert!(
                error.reason.contains("MIME"),
                "unexpected: {}",
                error.reason
            );
        }
    }

    #[test]
    fn background_dark_side_independent_and_round_trips() {
        use base64::Engine;
        let light_b64 = base64::engine::general_purpose::STANDARD.encode(png_header(320, 240));
        let dark_b64 = base64::engine::general_purpose::STANDARD.encode(jpeg_header(320, 240));
        let json = theme_with_background(background_json(
            Some(&light_b64),
            "image/png",
            Some(0.7),
            Some(&dark_b64),
            "image/jpeg",
        ));
        let theme = parse(&json).expect("both-mode background must pass");
        let background = theme.background.as_ref().unwrap();
        assert_eq!(
            background
                .light
                .as_ref()
                .unwrap()
                .image
                .as_ref()
                .unwrap()
                .mime_type,
            ImageMime::Png
        );
        assert_eq!(
            background
                .dark
                .as_ref()
                .unwrap()
                .image
                .as_ref()
                .unwrap()
                .mime_type,
            ImageMime::Jpeg
        );
        // Serialized round trip preserves the structure.
        let encoded = serde_json::to_vec(&theme).unwrap();
        let decoded: ThemeFile = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.background, theme.background);
        // And the mimeType serializes as the wire string.
        let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            value["background"]["light"]["image"]["mimeType"],
            "image/png"
        );
    }

    #[test]
    fn background_absent_in_legacy_themes_stays_invisible() {
        // Old-format themes carry no background field and behave exactly as
        // before (regression guard for the whole prior feature batch).
        let theme = parse(&sample_theme_json()).expect("legacy theme must pass");
        assert!(theme.background.is_none());
        assert!(!theme.has_background_image());
        let encoded = serde_json::to_string(&theme).unwrap();
        assert!(
            !encoded.contains("background"),
            "legacy round trip must not gain a background"
        );
    }
    // ─── Listing slimness and on-demand image fetch (R003) ─────────────

    fn import_background_theme(
        base: &Path,
        id: &str,
        light_b64: &str,
        light_mime: &str,
        dark_b64: Option<&str>,
        dark_mime: &str,
    ) {
        let src_dir = base.join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();
        let mut json = theme_with_background(background_json(
            Some(light_b64),
            light_mime,
            Some(0.82),
            dark_b64,
            dark_mime,
        ));
        json["id"] = serde_json::json!(id);
        json["name"] = serde_json::json!({ "en": id });
        let src = src_dir.join(format!("{id}.json"));
        std::fs::write(&src, serde_json::to_vec(&json).unwrap()).unwrap();
        import_theme_file_at(&src, base).expect("import must succeed");
    }

    fn padded_png_b64(payload_size: usize) -> String {
        use base64::Engine;
        let mut payload = png_header(64, 64);
        if payload.len() < payload_size {
            payload.extend(std::iter::repeat_n(0u8, payload_size - payload.len()));
        }
        base64::engine::general_purpose::STANDARD.encode(&payload)
    }

    #[test]
    fn listing_serialization_carries_no_image_bytes() {
        let base = temp_base();
        let b64 = padded_png_b64(12 * 1024); // ~16 KiB of base64 in the file
        import_background_theme(&base, "studio", &b64, "image/png", None, "");
        let listing = list_themes_at(&base);
        assert!(
            listing.errors.is_empty(),
            "unexpected errors: {:?}",
            listing.errors
        );
        assert_eq!(listing.entries.len(), 1);
        let entry = &listing.entries[0];
        let serialized = serde_json::to_string(entry).unwrap();
        // No payload bytes leak into the listing.
        assert!(
            !serialized.contains("dataB64"),
            "listing must not carry dataB64"
        );
        assert!(
            !serialized.contains(&b64[..64]),
            "listing must not carry base64 chunks"
        );
        // Metadata is present instead.
        let background = entry
            .background
            .as_ref()
            .expect("background metadata present");
        let light = background.light.as_ref().expect("light meta present");
        assert!(light.has_image);
        assert_eq!(light.mime_type, Some(ImageMime::Png));
        assert_eq!(light.scrim.as_ref().expect("scrim meta").hex, "#0f1526");
        assert!(background.dark.is_none());
        // Volume stays the same order of magnitude as before the feature:
        // the theme file is >16 KiB while the listing entry stays tiny.
        let file_len = std::fs::metadata(themes_dir_at(&base).join("studio.json"))
            .unwrap()
            .len();
        assert!(file_len > 16 * 1024);
        assert!(
            serialized.len() < 8 * 1024,
            "listing entry too large: {}",
            serialized.len()
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn theme_background_returns_decoded_bytes_on_demand() {
        let base = temp_base();
        let b64 = padded_png_b64(4096);
        import_background_theme(&base, "studio", &b64, "image/png", None, "");
        let fetched = theme_background_at("studio", true, &base)
            .expect("fetch must succeed")
            .expect("image theme must return bytes");
        assert_eq!(fetched.mime_type, ImageMime::Png);
        // The returned bytes are exactly the stored payload.
        let mut expected = png_header(64, 64);
        expected.extend(std::iter::repeat_n(0u8, 4096 - expected.len()));
        assert_eq!(fetched.data, expected);
        // Dark mode has no image on this theme.
        assert!(theme_background_at("studio", false, &base)
            .unwrap()
            .is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn theme_background_reads_both_modes() {
        let base = temp_base();
        use base64::Engine;
        let light_b64 = base64::engine::general_purpose::STANDARD.encode(png_header(320, 240));
        let dark_b64 = base64::engine::general_purpose::STANDARD.encode(jpeg_header(320, 240));
        import_background_theme(
            &base,
            "duo",
            &light_b64,
            "image/png",
            Some(&dark_b64),
            "image/jpeg",
        );
        let light = theme_background_at("duo", true, &base).unwrap().unwrap();
        let dark = theme_background_at("duo", false, &base).unwrap().unwrap();
        assert_eq!(light.mime_type, ImageMime::Png);
        assert_eq!(dark.mime_type, ImageMime::Jpeg);
        assert_eq!(
            light.data,
            base64::engine::general_purpose::STANDARD
                .decode(&light_b64)
                .unwrap()
        );
        assert_eq!(
            dark.data,
            base64::engine::general_purpose::STANDARD
                .decode(&dark_b64)
                .unwrap()
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn theme_background_returns_none_for_missing_or_imageless() {
        let base = temp_base();
        // Imageless legacy theme.
        let src_dir = base.join("sources");
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = write_valid_theme_src(&src_dir, "plain.json", "plain");
        import_theme_file_at(&src, &base).unwrap();
        assert!(theme_background_at("plain", true, &base).unwrap().is_none());
        assert!(theme_background_at("plain", false, &base)
            .unwrap()
            .is_none());
        // No such file.
        assert!(theme_background_at("ghost", true, &base).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn theme_background_rejects_invalid_and_reserved_ids() {
        let base = temp_base();
        assert!(theme_background_at("../evil", true, &base).is_err());
        assert!(theme_background_at("BAD_ID", true, &base).is_err());
        assert!(theme_background_at("tailsync", true, &base).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn theme_background_revalidates_payload_on_read() {
        use base64::Engine;
        let base = temp_base();
        let b64 = padded_png_b64(2048);
        import_background_theme(&base, "studio", &b64, "image/png", None, "");
        assert!(theme_background_at("studio", true, &base)
            .unwrap()
            .is_some());
        // Tamper with the stored file: replace the image with garbage bytes
        // (valid base64, wrong magic). The on-read validation must reject it.
        let mut json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(themes_dir_at(&base).join("studio.json")).unwrap(),
        )
        .unwrap();
        json["background"]["light"]["image"]["dataB64"] = serde_json::json!(
            base64::engine::general_purpose::STANDARD.encode(b"definitely not an image")
        );
        std::fs::write(
            themes_dir_at(&base).join("studio.json"),
            serde_json::to_vec(&json).unwrap(),
        )
        .unwrap();
        let error =
            theme_background_at("studio", true, &base).expect_err("tampered image must fail");
        assert!(
            error.contains("validation") || error.contains("PNG"),
            "unexpected: {error}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    // ─── Example theme (R010) ─────────────────────────────────────────

    #[test]
    fn example_spider_man_city_theme_passes_validation() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/examples/spider-man-city.json");
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("example theme missing at {}: {error}", path.display()));
        let theme = validate_theme_bytes(&bytes, "spider-man-city.json")
            .expect("example theme must pass validation");
        assert_eq!(theme.id, "spider-man-city");
        assert!(theme.has_background_image());
        let background = theme.background.as_ref().expect("background present");
        let light = background.light.as_ref().expect("light mode");
        assert_eq!(
            light.image.as_ref().expect("image").mime_type,
            ImageMime::Png
        );
        // The embedded image itself passes the payload validation (magic +
        // dimensions) — proven implicitly by theme validation.
        assert_eq!(light.scrim.as_ref().unwrap().opacity, Some(0.85));
    }
}
