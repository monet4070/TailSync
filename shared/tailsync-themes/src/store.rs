use super::*;

pub(super) fn root(base: &Path) -> PathBuf {
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
pub(super) fn id_path(id: &str) -> String {
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
    let encoded = serde_json::to_vec(&s)
        .map_err(|error| ThemeError::new("THEME_IO", error.to_string(), ""))?;
    atomic(&settings_path(base), &encoded)
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<(), ThemeError> {
    let parent = path
        .parent()
        .ok_or_else(|| ThemeError::new("THEME_IO", "path has no parent", ""))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| ThemeError::new("THEME_IO", "path has no file name", ""))?;
    fs::create_dir_all(parent).map_err(|e| ThemeError::new("THEME_IO", e.to_string(), ""))?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}-{:x}",
        file_name.to_string_lossy(),
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

pub(super) fn recover_swap(dir: &Path) -> Result<(), ThemeError> {
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
pub(super) fn delete_theme_by_handle_with_remover_at<F>(
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
            if let Ok(encoded) = serde_json::to_vec(&original_settings) {
                let _ = atomic(&settings_path(base), &encoded);
            }
        }
        return Err(ThemeError::new("THEME_IO", error.to_string(), ""));
    }
    Ok(())
}
