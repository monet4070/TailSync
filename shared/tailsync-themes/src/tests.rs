use super::validation::*;
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
    serde_json::json!({"formatVersion":2,"id":"custom:studio.night","version":"1.0.0","minCoreVersion":CORE_VERSION,"name":{"en":"Night"},"extends":"builtin:canvas@1","light":{"colors":{"background":{"canvas":"#ffffff"},"text":{"primary":"#111111"}}},"dark":{"colors":{"background":{"canvas":"#111111"},"text":{"primary":"#ffffff"}}}})
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
    m["foundation"] = serde_json::json!({"effects": {"motion": {"fast": 20, "easing": "spring"}}});
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
    std::io::Write::write_all(&mut archive, serde_json::to_string(&m).unwrap().as_bytes()).unwrap();
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
    m["highContrast"] = serde_json::json!({"light": {"effects": {"opacity": "full"}}, "dark": {}});
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
    let core = semver(CORE_VERSION).unwrap();
    let newer_core = if core.patch < u32::MAX {
        format!("{}.{}.{}", core.major, core.minor, core.patch + 1)
    } else if core.minor < u32::MAX {
        format!("{}.{}.0", core.major, core.minor + 1)
    } else {
        format!("{}.0.0", core.major + 1)
    };
    m["minCoreVersion"] = Value::from(newer_core);
    let error = diagnostic(m);
    assert_eq!(error.code, "THEME_MIN_CORE_VERSION");
    assert_eq!(error.json_pointer, "/minCoreVersion");

    let mut m = manifest();
    let compatible_version = if core.prerelease.is_none() {
        format!("{}.{}.{}-rc.1", core.major, core.minor, core.patch)
    } else {
        CORE_VERSION.to_owned()
    };
    m["minCoreVersion"] = Value::from(compatible_version);
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
        let resolved = resolve_theme_at(id, "light", "windows", true, Path::new("/tmp")).unwrap();
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
