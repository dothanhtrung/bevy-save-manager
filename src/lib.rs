//! ### Plugin
//!

use bevy::prelude::{
    Commands,
    Resource,
};
use crossbeam_channel::{
    Receiver,
    Sender,
};
use std::env;
use std::path::PathBuf;

pub mod encrypt_save;
pub mod raw_save;

pub enum IoAction {
    Save,
    Load,
}

pub struct IoResult {
    action: IoAction,
    result: anyhow::Result<()>,
}

impl IoResult {
    pub fn success(action: IoAction) -> Self {
        Self { action, result: Ok(()) }
    }

    pub fn failure(action: IoAction, result: anyhow::Result<()>) -> Self {
        Self { action, result }
    }
}

#[derive(Resource)]
pub struct IoChannel {
    sender: Sender<IoResult>,
    receiver: Receiver<IoResult>,
}

fn setup_channel(mut commands: Commands) {
    let (sender, receiver) = crossbeam_channel::unbounded();
    commands.insert_resource(IoChannel { sender, receiver });
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
