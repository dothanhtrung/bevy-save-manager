//! ### Plugin
//!

use std::env;
use std::path::PathBuf;

mod file_format;
pub mod save;
pub mod setting;

#[derive(Default, PartialEq, Eq, Clone)]
pub enum FileFormat {
    #[default]
    Ron,
    Bin,
}

fn get_relative_path() -> PathBuf {
    if cfg!(target_os = "linux")
        && let Ok(appimage_path) = env::var("APPIMAGE")
    {
        let path = PathBuf::from(appimage_path);
        if let Some(parent_dir) = path.parent() {
            return parent_dir.to_path_buf();
        }
    }
    PathBuf::from(".")
}
