use std::{fs, path::Path};

/// Maximum size accepted when a user selects a package from the filesystem.
/// Archive validation has tighter compressed and expanded limits; this bound
/// prevents the platform command boundary from reading an obviously oversized
/// file before the archive validator gets a chance to inspect it.
pub const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;

/// Read a user-selected theme package using the same filesystem policy on all
/// platforms. The caller remains responsible for mapping the structured error
/// into its transport-specific response shape.
pub fn read_theme_package_file(path: &Path) -> Result<Vec<u8>, crate::ThemeError> {
    if !path.to_string_lossy().ends_with(".tailsync-theme") {
        return Err(crate::ThemeError::new(
            "THEME_EXTENSION",
            "theme package must end in .tailsync-theme",
            "/path",
        ));
    }

    let metadata = fs::symlink_metadata(path)
        .map_err(|error| crate::ThemeError::new("THEME_IO", error.to_string(), "/path"))?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
        return Err(crate::ThemeError::new(
            "THEME_PATH",
            "theme package must be a regular file, not a symbolic link",
            "/path",
        ));
    }
    if metadata.len() > MAX_IMPORT_BYTES {
        return Err(crate::ThemeError::new(
            "THEME_TOO_LARGE",
            "theme package exceeds the 64 MiB import limit",
            "/path",
        ));
    }

    let canonical = path
        .canonicalize()
        .map_err(|error| crate::ThemeError::new("THEME_IO", error.to_string(), "/path"))?;
    fs::read(canonical)
        .map_err(|error| crate::ThemeError::new("THEME_IO", error.to_string(), "/path"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tailsync-theme-package-{name}-{}",
            rand::random::<u64>()
        ))
    }

    #[test]
    fn reads_regular_theme_package() {
        let path = temp_path("regular").with_extension("tailsync-theme");
        fs::write(&path, b"theme").unwrap();
        assert_eq!(read_theme_package_file(&path).unwrap(), b"theme");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_wrong_extension() {
        let error = read_theme_package_file(Path::new("theme.zip")).unwrap_err();
        assert_eq!(error.code, "THEME_EXTENSION");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symbolic_links() {
        let target = temp_path("target").with_extension("tailsync-theme");
        let link = temp_path("link").with_extension("tailsync-theme");
        fs::write(&target, b"theme").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let error = read_theme_package_file(&link).unwrap_err();
        assert_eq!(error.code, "THEME_PATH");
        let _ = fs::remove_file(link);
        let _ = fs::remove_file(target);
    }
}
