use crate::setting::{
    GameSetting,
    GameSettingChanged,
    GameSettingSupportPlugin,
};
use bevy::app::App;
#[cfg(feature = "log")]
use bevy::prelude::{
    error,
    warn,
};
use bevy::prelude::{
    on_message,
    Deref,
    DerefMut,
    IntoScheduleConfigs,
    Message,
    MessageReader,
    MessageWriter,
    Plugin,
    Res,
    ResMut,
    Resource,
    Update,
};
use bevy::tasks::IoTaskPool;
use serde::{
    Deserialize,
    Serialize,
};
use simple_crypt::{
    decrypt,
    encrypt,
};
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::{
    Path,
    PathBuf,
};

#[derive(Default)]
pub struct EncryptSavePlugin<T>
where
    T: Resource + Default + EncryptSave + Clone,
{
    _config: Option<T>,
}

impl<T> Plugin for EncryptSavePlugin<T>
where
    T: Resource + Default + EncryptSave + Clone,
{
    fn build(&self, app: &mut App) {
        app.add_plugins(GameSettingSupportPlugin::<SaveConfig>::default())
            .insert_resource(T::default())
            .insert_resource(CurrentSave(0))
            .add_message::<SaveGame>()
            .add_message::<DeleteSave>()
            .add_message::<LoadGame>()
            .add_message::<LoadRecent>()
            .add_systems(Update, load::<T>.run_if(on_message::<LoadGame>))
            .add_systems(Update, load_recent::<T>.run_if(on_message::<LoadRecent>))
            .add_systems(Update, save::<T>.run_if(on_message::<SaveGame>))
            .add_systems(Update, delete.run_if(on_message::<DeleteSave>));
    }
}

#[derive(Message, Deref, DerefMut)]
pub struct SaveGame(pub u32);

#[derive(Message, Deref, DerefMut)]
pub struct DeleteSave(pub u32);

#[derive(Message, Deref, DerefMut)]
pub struct LoadGame(pub u32);

#[derive(Message)]
pub struct LoadRecent;

#[derive(Resource, Deref, DerefMut)]
pub struct CurrentSave(pub u32);

#[derive(Resource, Deserialize, Serialize, Clone, Default)]
pub struct SaveConfig {
    /// Valid save id start from 1
    saves: HashMap<u32, PathBuf>,
    save_dir: PathBuf,
    last_saved: u32,
}

impl GameSetting for SaveConfig {
    const DEFAULT_CONF: &'static str = "save_setting.conf";
}

fn load<T>(
    mut data: ResMut<T>,
    mut load_message: MessageReader<LoadGame>,
    mut current_save: ResMut<CurrentSave>,
    save_config: Res<SaveConfig>,
) where
    T: Resource + EncryptSave,
{
    for id in load_message.read() {
        if let Some(saved_path) = save_config.saves.get(&id.0) {
            let saved_path = save_config.save_dir.join(saved_path);
            if let Err(_e) = data.load_from(&saved_path) {
                #[cfg(feature = "log")]
                warn!("Failed to load save data {}: {}", saved_path.display(), _e);
            } else {
                current_save.0 = id.0;
            }
        }
    }
}

fn load_recent<T>(mut data: ResMut<T>, mut current_save: ResMut<CurrentSave>, save_config: Res<SaveConfig>)
where
    T: Resource + EncryptSave,
{
    if let Some(saved_path) = save_config.saves.get(&save_config.last_saved) {
        let saved_path = save_config.save_dir.join(saved_path);
        if let Err(_e) = data.load_from(&saved_path) {
            #[cfg(feature = "log")]
            warn!("Failed to load save data {}: {}", saved_path.display(), _e);
        } else {
            current_save.0 = save_config.last_saved;
        }
    }
}

fn save<T>(
    data: Res<T>,
    mut save_message: MessageReader<SaveGame>,
    mut current_save: ResMut<CurrentSave>,
    mut save_config: ResMut<SaveConfig>,
    mut setting_changed: MessageWriter<GameSettingChanged>,
) where
    T: Resource + EncryptSave,
{
    for save in save_message.read() {
        let save_id = **save;
        if save_id == 0 {
            let file_name = format!("{}.dat", random_string());
            let saved_path = save_config.save_dir.join(file_name.as_str());
            if let Err(_e) = data.save_to(saved_path.clone()) {
                #[cfg(feature = "log")]
                error!("Failed to save data {}: {}", saved_path.display(), _e);
            } else {
                // TODO: Handle max_key == max of u32
                let new_key = if let Some(max_key) = save_config.saves.keys().max() { max_key + 1 } else { 1 };
                save_config.saves.insert(new_key, PathBuf::from(file_name));
                save_config.last_saved = new_key;
                current_save.0 = new_key;
                setting_changed.write(GameSettingChanged);
            }
        } else {
            if let Some(saved_path) = save_config.saves.get(&save_id) {
                let saved_path = save_config.save_dir.join(saved_path);
                if let Err(_e) = data.save_to(saved_path.clone()) {
                    #[cfg(feature = "log")]
                    error!("Failed to save data {}: {}", saved_path.display(), _e);
                } else {
                    save_config.last_saved = save_id;
                    current_save.0 = save_id;
                }
            }
        }
    }
}

fn delete(
    mut current_save: ResMut<CurrentSave>,
    mut delete_event: MessageReader<DeleteSave>,
    mut save_config: ResMut<SaveConfig>,
) {
    for saved_id in delete_event.read() {
        if let Some(saved_path) = save_config.saves.get(&saved_id) {
            if let Err(_e) = fs::remove_file(saved_path) {
                #[cfg(feature = "log")]
                error!("Failed to delete save data {}: {}", saved_path.display(), _e);
            } else {
                save_config.saves.remove(&saved_id);
                current_save.0 = 0;
                if save_config.last_saved == **saved_id {
                    save_config.last_saved = 0;
                }
            }
        }
    }
}

pub trait EncryptSave: Serialize + for<'de> Deserialize<'de> {
    const ENCR_KEY: &'static str = "0123456789abcdef";

    fn load_from(&mut self, config_path: &Path) -> anyhow::Result<()> {
        let enc_saved = std::fs::read(config_path)?;
        let decrypted = decrypt(enc_saved.as_slice(), Self::ENCR_KEY.as_bytes())?;
        (*self, _) = bincode::serde::decode_from_slice(decrypted.as_slice(), bincode::config::legacy())?;
        Ok(())
    }

    fn save_to(&self, saved_path: PathBuf) -> anyhow::Result<()> {
        let data = bincode::serde::encode_to_vec(self, bincode::config::legacy())?;
        let enc_saved = encrypt(data.as_slice(), Self::ENCR_KEY.as_bytes())?;

        #[cfg(not(target_arch = "wasm32"))]
        IoTaskPool::get()
            .spawn(async move {
                if let Some(parent_dir) = saved_path.parent() {
                    fs::create_dir_all(parent_dir)?;
                }
                File::create(saved_path).and_then(|mut file| file.write_all(enc_saved.as_slice()))
            })
            .detach();

        Ok(())
    }
}

fn random_string() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    const LEN: usize = 12;

    (0..LEN)
        .map(|_| {
            let idx = fastrand::usize(..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}
