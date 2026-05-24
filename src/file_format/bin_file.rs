use anyhow::anyhow;
use bevy::tasks::IoTaskPool;
use crossbeam_channel::Sender;
use serde::Serialize;
use simple_crypt::encrypt;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use crate::save::{
    IoAction,
    IoResult,
};

pub(crate) fn load_from(config_path: PathBuf, sender: Sender<IoResult>, save_id: u32, encrypt_key: String) {
    if cfg!(target_family = "wasm") {
        let _ = sender.send(IoResult::failure(
            IoAction::Load((save_id, Vec::new())),
            Err(anyhow!("Not support WASM")),
        ));
    } else {
        IoTaskPool::get()
            .spawn(async move {
                let enc_saved = match fs::read(config_path) {
                    Ok(ret) => ret,
                    Err(e) => {
                        let _ = sender.send(IoResult::failure(
                            IoAction::Load((save_id, Vec::new())),
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
                                IoAction::Load((save_id, Vec::new())),
                                Err(anyhow!(e)),
                            ));
                            return;
                        }
                    }
                };
                let _ = sender.send(IoResult::success(IoAction::Load((save_id, decrypted))));
            })
            .detach();
    }
}

pub(crate) fn save_to<T>(
    data: &T,
    saved_path: PathBuf,
    encrypt_key: &str,
    sender: Sender<IoResult>,
    save_id: u32,
) -> anyhow::Result<()>
where
    T: Serialize,
{
    if cfg!(target_family = "wasm") {
        return Err(anyhow!("Not support WASM"));
    } else {
        let data = postcard::to_allocvec(data)?;
        let encrypt_key = String::from(encrypt_key);
        IoTaskPool::get()
            .spawn(async move {
                let enc_saved = if encrypt_key.is_empty() {
                    data
                } else {
                    match encrypt(data.as_slice(), encrypt_key.as_bytes()) {
                        Ok(ret) => ret,
                        Err(e) => {
                            let _ = sender.send(IoResult::failure(IoAction::Save(save_id), Err(anyhow!(e))));
                            return;
                        }
                    }
                };

                if let Some(parent_dir) = saved_path.parent()
                    && let Err(e) = fs::create_dir_all(parent_dir)
                {
                    let _ = sender.send(IoResult::failure(IoAction::Save(save_id), Err(anyhow!(e))));
                }
                if let Err(e) = File::create(saved_path).and_then(|mut file| file.write_all(enc_saved.as_slice())) {
                    let _ = sender.send(IoResult::failure(IoAction::Save(save_id), Err(anyhow!(e))));
                }

                let _ = sender.send(IoResult::success(IoAction::Save(save_id)));
            })
            .detach();
    }
    Ok(())
}
