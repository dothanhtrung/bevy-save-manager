use crate::file_format::{
    IoAction,
    IoResult,
};
use bevy::tasks::IoTaskPool;
use crossbeam_channel::Sender;
use ron::ser::{
    PrettyConfig,
    to_string_pretty,
};
use serde::{
    Deserialize,
    Serialize,
};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

pub(crate) fn load_from<T>(save_path: PathBuf, sender: Sender<IoResult<T>>)
where
    T: for<'de> Deserialize<'de> + Serialize + Send + 'static,
{
    let file_path_str = save_path.to_str().unwrap_or_default().to_string();
    if cfg!(target_family = "wasm") {
        let _ = sender.send(IoResult::failure(IoAction::Load(file_path_str), "Not support WASM"));
        return;
    }

    IoTaskPool::get()
        .spawn(async move {
            let file = match File::open(save_path) {
                Ok(ret) => ret,
                Err(e) => {
                    let _ = sender.send(IoResult::failure(IoAction::Load(file_path_str), &e.to_string()));
                    return;
                }
            };
            match ron::de::from_reader(file) {
                Ok(ret) => {
                    let _ = sender.send(IoResult::success(IoAction::Load(file_path_str), Some(ret)));
                }
                Err(e) => {
                    let _ = sender.send(IoResult::failure(IoAction::Load(file_path_str), &e.to_string()));
                    return;
                }
            }
        })
        .detach();
}

pub(crate) fn save_to<T>(data: &T, save_path: PathBuf, sender: Sender<IoResult<T>>)
where
    T: Serialize + for<'de> Deserialize<'de> + Send + 'static,
{
    let file_path_str = save_path.to_str().unwrap_or_default().to_string();
    let pretty = PrettyConfig::default();

    let ron_str = match to_string_pretty(&data, pretty) {
        Ok(ret) => ret,
        Err(e) => {
            let _ = sender.send(IoResult::failure(IoAction::Save(file_path_str), &e.to_string()));
            return;
        }
    };

    #[cfg(not(target_family = "wasm"))]
    IoTaskPool::get()
        .spawn(async move {
            if let Some(parent_dir) = save_path.parent()
                && let Err(e) = std::fs::create_dir_all(parent_dir)
            {
                let _ = sender.send(IoResult::failure(IoAction::Save(file_path_str), &e.to_string()));
                return;
            }

            let mut file = match File::create(save_path) {
                Ok(ret) => ret,
                Err(e) => {
                    let _ = sender.send(IoResult::failure(IoAction::Save(file_path_str), &e.to_string()));
                    return;
                }
            };
            if let Err(e) = file.write_all(ron_str.as_bytes()) {
                let _ = sender.send(IoResult::failure(IoAction::Save(file_path_str), &e.to_string()));
                return;
            }
            let _ = sender.send(IoResult::success(IoAction::Save(file_path_str), None));
        })
        .detach();
}
