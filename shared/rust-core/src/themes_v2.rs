#![allow(clippy::result_large_err)]

//! Theme package V2 — platform-facing shim over the standalone `tailsync-themes` crate.
//!
//! The theme logic (parsing, validation, storage, resolution) now lives in the
//! location-agnostic `tailsync-themes` crate, where every entry point takes an
//! explicit `base` directory. This module re-exports that crate wholesale and
//! keeps the thin `get_data_dir()`-based convenience wrappers here — the data
//! directory belongs to `crate::db`, not to the theme logic — so existing
//! `tailsync_core::themes_v2::*` call sites stay byte-for-byte unchanged.

use crate::db::get_data_dir;
pub use tailsync_themes::*;

pub fn install_theme(bytes: &[u8], expected: &str) -> Result<ThemeDescriptor, ThemeError> {
    install_theme_at(bytes, expected, &get_data_dir())
}
pub fn update_theme(
    bytes: &[u8],
    expected: &str,
    options: UpdateThemeOptions,
) -> Result<ThemeDescriptor, ThemeError> {
    update_theme_at(bytes, expected, options, &get_data_dir())
}
pub fn list_themes_v2() -> Vec<ThemeDescriptor> {
    list_themes_v2_at(&get_data_dir())
}
pub fn resolve_theme(
    id: &str,
    mode: &str,
    platform: &str,
    high: bool,
) -> Result<ResolvedTheme, ThemeError> {
    resolve_theme_at(id, mode, platform, high, &get_data_dir())
}
pub fn rollback_theme(id: &str) -> Result<ThemeDescriptor, ThemeError> {
    rollback_theme_at(id, &get_data_dir())
}
pub fn delete_theme(id: &str) -> Result<(), ThemeError> {
    delete_theme_at(id, &get_data_dir())
}
pub fn delete_theme_by_handle(handle: &str) -> Result<(), ThemeError> {
    delete_theme_by_handle_at(handle, &get_data_dir(), Some(&format!("invalid:{handle}")))
}
pub fn delete_theme_by_handle_for_theme(handle: &str, id: &str) -> Result<(), ThemeError> {
    delete_theme_by_handle_at(handle, &get_data_dir(), Some(id))
}
pub fn get_local_theme_settings() -> LocalThemeSettings {
    get_local_theme_settings_at(&get_data_dir())
}
pub fn set_local_theme_settings(settings: LocalThemeSettings) -> Result<(), ThemeError> {
    set_local_theme_settings_at(&get_data_dir(), settings)
}
pub fn get_theme_asset(id: &str, digest: &str, key: &str) -> Result<(String, Vec<u8>), ThemeError> {
    get_theme_asset_at(id, digest, key, &get_data_dir())
}
pub fn get_theme_asset_slot(
    id: &str,
    digest: &str,
    slot: &str,
) -> Result<(ThemeAssetDescriptor, Vec<u8>), ThemeError> {
    get_theme_asset_slot_at(id, digest, slot, &get_data_dir())
}
