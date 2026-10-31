//! Installation-scoped preferences; clipboard history and settings are untouched.
use std::path::{Path, PathBuf};

pub(super) struct TourMarkers {
    root: PathBuf,
    installation: String,
}

impl TourMarkers {
    pub(super) fn new(root: PathBuf, installation: String) -> Self {
        Self { root, installation }
    }

    fn path(&self, suppressed: bool) -> PathBuf {
        self.root.join(if suppressed {
            "onboarding-v3.suppressed"
        } else {
            "onboarding-v2.dismissed"
        })
    }

    fn matches(&self, suppressed: bool) -> bool {
        let path = self.path(suppressed);
        match std::fs::read_to_string(&path) {
            Ok(value) if value.trim() == self.installation => true,
            // Adopt old unscoped preferences on the first upgrade.
            Ok(value) if matches!(value.trim(), "2" | "3") => self.write(suppressed).is_ok(),
            _ => false,
        }
    }

    fn write(&self, suppressed: bool) -> Result<(), String> {
        std::fs::create_dir_all(&self.root).map_err(|error| error.to_string())?;
        std::fs::write(self.path(suppressed), &self.installation).map_err(|error| error.to_string())
    }

    pub(super) fn suppressed(&self) -> bool {
        self.matches(true)
    }

    pub(super) fn should_show(&self) -> bool {
        !self.suppressed() && !self.matches(false)
    }

    pub(super) fn dismiss(&self) -> Result<(), String> {
        self.write(false)
    }

    pub(super) fn suppress(&self) -> Result<(), String> {
        self.write(true)?;
        self.dismiss()
    }
}

pub(super) fn installation_identity(executable: &Path) -> String {
    let directory = executable.parent().unwrap_or(executable);
    // The NSIS hook creates a fresh ID on manual installation, preserves it
    // for /UPDATE and removes it on uninstall.
    if let Ok(id) = std::fs::read_to_string(directory.join("tailsync-install-id")) {
        let id = id.trim();
        if !id.is_empty() {
            return format!("nsis:{id}");
        }
    }
    // Portable/dev builds have no installer. Replacing the executable in the
    // same directory is an update; recreating that directory is a reinstall.
    let created = std::fs::metadata(directory)
        .and_then(|value| value.created())
        .ok();
    format!("portable:{}:{created:?}", directory.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self(
                std::env::temp_dir().join(format!("tailsync-tour-{}-{unique}", std::process::id())),
            )
        }
        fn markers(&self, id: &str) -> TourMarkers {
            TourMarkers::new(self.0.clone(), id.to_owned())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn dismissal_survives_relaunch_and_reinstall_resets_it() {
        let fixture = Fixture::new();
        assert!(fixture.markers("install-A").should_show());
        fixture.markers("install-A").suppress().unwrap();
        assert!(fixture.markers("install-A").suppressed());
        assert!(!fixture.markers("install-A").should_show());
        assert!(!fixture.markers("install-B").suppressed());
        assert!(fixture.markers("install-B").should_show());
        fixture.markers("install-B").dismiss().unwrap();
        assert!(!fixture.markers("install-B").should_show());
        assert!(!fixture.markers("install-B").suppressed());
    }

    #[test]
    fn legacy_dismissal_is_adopted_once() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(&fixture.0).unwrap();
        std::fs::write(fixture.0.join("onboarding-v3.suppressed"), "3\n").unwrap();
        assert!(fixture.markers("install-A").suppressed());
        assert!(!fixture.markers("install-B").suppressed());
    }

    #[test]
    fn installer_identity_is_stable_across_binary_updates_and_changes_on_reinstall() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(&fixture.0).unwrap();
        let exe = fixture.0.join("tailsync.exe");
        let id = fixture.0.join("tailsync-install-id");
        std::fs::write(&id, "{install-A}\r\n").unwrap();
        let original = installation_identity(&exe);
        std::fs::write(&exe, "new-binary").unwrap();
        assert_eq!(original, installation_identity(&exe));
        std::fs::write(&id, "{install-B}\r\n").unwrap();
        assert_ne!(original, installation_identity(&exe));
    }
}
