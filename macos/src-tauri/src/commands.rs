use crate::db;
use crate::network;
use crate::AppState;
use log::info;
use tauri::{command, AppHandle, Manager, State};

#[derive(serde::Serialize)]
pub struct HistoryPage {
    pub entries: Vec<db::HistoryEntry>,
    pub total: Option<usize>,
    pub has_more: bool,
}

mod history;
mod peers;
mod platform;
mod settings;
mod storage;
mod themes;

use platform::rgba_to_dib;
#[cfg(target_os = "windows")]
use platform::set_clipboard_dib;

pub use history::*;
pub use peers::*;
pub use platform::*;
pub use settings::*;
pub use storage::*;
pub use themes::*;
