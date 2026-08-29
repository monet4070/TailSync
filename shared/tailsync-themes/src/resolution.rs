use super::*;

pub(super) fn canvas(mode: &str) -> Value {
    builtin_tokens(CANVAS_ID, mode)
}
pub(super) fn merge(
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
pub(super) fn mark_provenance(
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
pub(super) fn resolve_color(
    value: &Value,
    root: &Value,
    seen: &mut Vec<String>,
) -> Result<Value, ThemeError> {
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

pub(super) fn parse_rgb(value: &str) -> Result<[u8; 3], ThemeError> {
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

pub(super) fn relative_luminance(rgb: [u8; 3]) -> f64 {
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

pub(super) fn contrast_ratio(a: f64, b: f64) -> f64 {
    let (high, low) = if a > b { (a, b) } else { (b, a) };
    (high + 0.05) / (low + 0.05)
}

#[derive(Clone, Copy)]
pub(super) struct ContrastColor {
    rgb: [f64; 3],
    alpha: f64,
}

pub(super) fn contrast_color(value: &Value) -> Option<ContrastColor> {
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

pub(super) fn composite(foreground: ContrastColor, background: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|index| {
        foreground.rgb[index] * foreground.alpha + background[index] * (1.0 - foreground.alpha)
    })
}

pub(super) fn luminance(rgb: [f64; 3]) -> f64 {
    let channel = |value: f64| {
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2])
}

pub(super) fn opaque_background(tokens: &Value, path: &str, canvas_path: &str) -> Option<[f64; 3]> {
    let background = contrast_color(tokens.pointer(path)?)?;
    if path == canvas_path {
        return Some(background.rgb);
    }
    let canvas = contrast_color(tokens.pointer(canvas_path)?)?;
    Some(composite(background, composite(canvas, [1.0, 1.0, 1.0])))
}

pub(super) fn color_contrast(
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

pub(super) fn set_policy_value(
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

pub(super) fn rgb_hex(rgb: [f64; 3]) -> Value {
    Value::String(format!(
        "#{:02x}{:02x}{:02x}",
        (rgb[0] * 255.0).round() as u8,
        (rgb[1] * 255.0).round() as u8,
        (rgb[2] * 255.0).round() as u8
    ))
}

pub(super) fn make_opaque(
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

pub(super) fn ensure_contrast(
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
pub(super) fn accessibility_diagnostics(
    resolved: &ResolvedTheme,
    high_contrast: bool,
) -> Vec<ThemeError> {
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
pub(super) fn resolve_manifest_unenforced(
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
pub(super) fn resolve_manifest(
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
pub(super) fn resolve_colors(
    v: &mut Value,
    root: &Value,
    seen: &mut Vec<String>,
) -> Result<(), ThemeError> {
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
pub(super) fn enforce_high_contrast(tokens: &mut Value, provenance: &mut BTreeMap<String, String>) {
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
