use crate::file_format::{
    IoAction,
    IoResult,
};
use anyhow::anyhow;
use bevy::tasks::IoTaskPool;
use crossbeam_channel::Sender;
use serde::{
    Deserialize,
    Serialize,
};
use simple_crypt::encrypt;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

pub(crate) fn load_from<T>(save_path: PathBuf, sender: Sender<IoResult<T>>, encrypt_key: String)
where
    T: Serialize + for<'de> Deserialize<'de> + Send + 'static,
{
    let file_path_str = save_path.to_str().unwrap_or_default().to_string();
    if cfg!(target_family = "wasm") {
        let _ = sender.send(IoResult::failure(
            IoAction::Load((file_path_str, None)),
            Err(anyhow!("Not support WASM")),
        ));
        return;
    } else {
        IoTaskPool::get()
            .spawn(async move {
                let enc_saved = match fs::read(save_path) {
                    Ok(ret) => ret,
                    Err(e) => {
                        let _ = sender.send(IoResult::failure(
                            IoAction::Load((file_path_str, None)),
                            Err(anyhow!(e)),
                        ));
                        return;
                    }
                };

                let decrypted = if encrypt_key.is_empty() {
                    enc_saved
                } else {
                    match simple_crypt::decrypt(enc_saved.as_slice(), encrypt_key.as_bytes()) {
                        Ok(ret) => ret,
                        Err(e) => {
                            let _ = sender.send(IoResult::failure(
                                IoAction::Load((file_path_str, None)),
                                Err(anyhow!(e)),
                            ));
                            return;
                        }
                    }
                };
                match postcard::from_bytes(decrypted.as_slice()) {
                    Ok(ret) => {
                        let _ = sender.send(IoResult::success(IoAction::Load((file_path_str, ret))));
                    }
                    Err(e) => {
                        let _ = sender.send(IoResult::failure(
                            IoAction::Load((file_path_str, None)),
                            Err(anyhow!(e)),
                        ));
                        return;
                    }
                }
            })
            .detach();
    }
}

pub(crate) fn save_to<T>(data: &T, save_path: PathBuf, encrypt_key: &str, sender: Sender<IoResult<T>>)
where
    T: Serialize + for<'de> Deserialize<'de> + Send + 'static,
{
    let file_path_str = save_path.to_str().unwrap_or_default().to_string();
    if cfg!(target_family = "wasm") {
        let _ = sender.send(IoResult::failure(
            IoAction::Save(file_path_str),
            Err(anyhow!("Not support WASM")),
        ));
        return;
    } else {
        let data = match postcard::to_allocvec(data) {
            Ok(ret) => ret,
            Err(e) => {
                let _ = sender.send(IoResult::failure(IoAction::Save(file_path_str), Err(anyhow!(e))));
                return;
            }
        };
        let encrypt_key = String::from(encrypt_key);

        IoTaskPool::get()
            .spawn(async move {
                let enc_saved = if encrypt_key.is_empty() {
                    data
                } else {
                    match encrypt(data.as_slice(), encrypt_key.as_bytes()) {
                        Ok(ret) => ret,
                        Err(e) => {
                            let _ = sender.send(IoResult::failure(IoAction::Save(file_path_str), Err(anyhow!(e))));
                            return;
                        }
                    }
                };

                if let Some(parent_dir) = save_path.parent()
                    && let Err(e) = fs::create_dir_all(parent_dir)
                {
                    let _ = sender.send(IoResult::failure(IoAction::Save(file_path_str), Err(anyhow!(e))));
                    return;
                }
                if let Err(e) = File::create(save_path).and_then(|mut file| file.write_all(enc_saved.as_slice())) {
                    let _ = sender.send(IoResult::failure(IoAction::Save(file_path_str), Err(anyhow!(e))));
                    return;
                }

                let _ = sender.send(IoResult::success(IoAction::Save(file_path_str)));
            })
            .detach();
    }
}
