use crate::db;
use crate::network;
use crate::AppState;
use log::info;
use tauri::{command, AppHandle, Manager, State};

pub use db::HistoryQueryPage as HistoryPage;

mod error;
mod history;
mod peers;
mod platform;
mod settings;
mod storage;
mod themes;

use platform::rgba_to_dib;
#[cfg(target_os = "windows")]
use platform::set_clipboard_dib;

pub use error::CommandError;
pub use history::*;
pub use peers::*;
pub use platform::*;
pub use settings::*;
pub use storage::*;
pub use themes::*;
