use super::*;

pub(super) fn pointer(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

pub(super) fn object<'a>(
    value: &'a Value,
    path: &str,
) -> Result<&'a serde_json::Map<String, Value>, ThemeError> {
    value
        .as_object()
        .ok_or_else(|| ThemeError::new("THEME_TOKEN_TYPE", "token group must be an object", path))
}

pub(super) fn only_keys(
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

pub(super) fn string(value: &Value, path: &str, description: &str) -> Result<(), ThemeError> {
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

pub(super) fn validate_color_string(value: &str, path: &str) -> Result<(), ThemeError> {
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

pub(super) fn bounded_number(
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

pub(super) fn color_token(value: &Value, path: &str) -> Result<(), ThemeError> {
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

pub(super) fn optional_object<'a>(
    values: &'a serde_json::Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<Option<&'a serde_json::Map<String, Value>>, ThemeError> {
    values
        .get(key)
        .map(|value| object(value, &pointer(path, key)))
        .transpose()
}

pub(super) const COMPONENT_NAMES: &[&str] = &[
    "search", "history", "section", "panel", "button", "input", "toast",
];
pub(super) const COMPONENT_STATES: &[&str] = &[
    "default", "hover", "active", "selected", "disabled", "focus",
];
pub(super) const COMPONENT_COLOR_FIELDS: &[&str] = &[
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
pub(super) fn validate_component_tokens(value: &Value, path: &str) -> Result<(), ThemeError> {
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

pub(super) fn validate_tokens(value: &Value, path: &str) -> Result<(), ThemeError> {
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

pub(super) fn read_package(
    bytes: &[u8],
) -> Result<(ThemeManifest, Vec<AssetMetadata>), ThemeError> {
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
pub(super) fn validate_asset_slots(
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

pub(super) fn resolved_asset_slots(
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

pub(super) fn validate_manifest(
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

pub(super) fn validate_all_resolved_modes(
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
