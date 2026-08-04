use std::path::PathBuf;
use std::sync::OnceLock;

/// Return the platform application-data directory, creating it when needed.
pub fn get_data_dir() -> PathBuf {
    static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
    DATA_DIR
        .get_or_init(|| {
            let directory = std::env::var_os("TAILSYNC_DATA_DIR")
                .map(PathBuf::from)
                .or_else(|| {
                    directories::ProjectDirs::from("com", "tailsync", "TailSync")
                        .map(|dirs| dirs.data_dir().to_path_buf())
                })
                .unwrap_or_else(|| {
                    let home = std::env::var("HOME")
                        .or_else(|_| std::env::var("USERPROFILE"))
                        .unwrap_or_else(|_| ".".to_string());
                    PathBuf::from(home).join(".tailsync")
                });
            std::fs::create_dir_all(&directory).ok();
            directory
        })
        .clone()
}

pub fn get_file_history_dir() -> PathBuf {
    get_data_dir().join("file-history")
}

pub fn get_image_history_dir() -> PathBuf {
    get_data_dir().join("image-history")
}

pub fn get_incoming_dir() -> PathBuf {
    get_data_dir().join("incoming")
}

pub fn get_clipboard_files_dir() -> PathBuf {
    get_data_dir().join("clipboard-files")
}
