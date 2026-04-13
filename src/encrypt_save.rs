use crate::raw_save::{
    GameSetting,
    GameSettingChanged,
    RawSavePlugin,
};
use crate::{
    get_relative_path,
    setup_channel,
    IoAction,
    IoChannel,
    IoResult,
};
use bevy::app::{
    App,
    Startup,
};
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
    Single,
    Update,
    With,
};
use bevy::tasks::IoTaskPool;
use bevy_rand::prelude::{
    EntropyPlugin,
    GlobalRng,
    WyRand,
};
use rand::RngExt;
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
use std::io::{
    Error,
    Write,
};
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
        app.add_plugins(EntropyPlugin::<WyRand>::default())
            .add_plugins(RawSavePlugin::<SaveConfig>::default())
            .insert_resource(T::default())
            .insert_resource(CurrentSave(0))
            .add_message::<QuickSave>()
            .add_message::<SaveGame>()
            .add_message::<DeleteSave>()
            .add_message::<LoadGame>()
            .add_message::<LoadRecent>()
            .add_message::<LoadFinished>()
            .add_message::<SaveFinished>()
            .add_systems(Startup, setup_channel)
            .add_systems(Update, on_load::<T>.run_if(on_message::<LoadGame>))
            .add_systems(Update, on_load_recent.run_if(on_message::<LoadRecent>))
            .add_systems(Update, on_save::<T>.run_if(on_message::<SaveGame>))
            .add_systems(Update, on_new_save.run_if(on_message::<NewSave>))
            .add_systems(Update, on_quick_save.run_if(on_message::<QuickSave>))
            .add_systems(Update, on_delete.run_if(on_message::<DeleteSave>));
    }
}

#[derive(Message)]
pub struct QuickSave;

#[derive(Message)]
pub struct NewSave;

/// Tell system to save data to file by save id.
/// If save id is 0, new save will be created.
#[derive(Message, Deref, DerefMut)]
pub struct SaveGame(pub u32);

#[derive(Message, Deref, DerefMut)]
pub struct DeleteSave(pub u32);

#[derive(Message, Deref, DerefMut)]
pub struct LoadGame(pub u32);

#[derive(Message)]
pub struct LoadRecent;

/// true: Load succeeded
/// false: Load failed
#[derive(Message, Deref, DerefMut, Default)]
pub struct LoadFinished(bool);

/// Not actual sent when saving is finished writing to disk
/// true: Save succeeded
/// false: Save failed
#[derive(Message, Deref, DerefMut, Default)]
pub struct SaveFinished(bool);

#[derive(Resource, Deref, DerefMut)]
pub struct CurrentSave(pub u32);

#[derive(Resource, Deserialize, Serialize, Clone)]
pub struct SaveConfig {
    /// Valid save id start from 1
    saves: HashMap<u32, PathBuf>,
    save_dir: PathBuf,
    last_saved: u32,
}

impl Default for SaveConfig {
    fn default() -> Self {
        Self {
            saves: HashMap::new(),
            save_dir: PathBuf::from("saves"),
            last_saved: 0,
        }
    }
}

impl GameSetting for SaveConfig {
    const DEFAULT_CONF: &'static str = "saves/save_setting.conf";
}

fn on_load<T>(
    mut data: ResMut<T>,
    mut load_message: MessageReader<LoadGame>,
    mut current_save: ResMut<CurrentSave>,
    save_config: Res<SaveConfig>,
    mut load_finished: MessageWriter<LoadFinished>,
    channel: Res<IoChannel>,
) where
    T: Resource + EncryptSave,
{
    for id in load_message.read() {
        if let Some(saved_path) = save_config.saves.get(&id.0) {
            let saved_path = save_config.save_dir.join(saved_path);
            if let Err(_e) = data.load_from(&saved_path, &channel) {
                load_finished.write(LoadFinished(false));
                #[cfg(feature = "log")]
                warn!("Failed to load save data {}: {}", saved_path.display(), _e);
            } else {
                current_save.0 = id.0;
                load_finished.write(LoadFinished(true));
            }
        } else {
            load_finished.write(LoadFinished(false));
        }
    }
}

fn on_load_recent(save_config: Res<SaveConfig>, mut load_message: MessageWriter<LoadGame>) {
    load_message.write(LoadGame(save_config.last_saved));
}

fn on_save<T>(
    data: Res<T>,
    mut save_message: MessageReader<SaveGame>,
    mut current_save: ResMut<CurrentSave>,
    mut save_config: ResMut<SaveConfig>,
    mut setting_changed: MessageWriter<GameSettingChanged>,
    mut save_finished: MessageWriter<SaveFinished>,
    mut rng: Single<&mut WyRand, With<GlobalRng>>,
    channel: Res<IoChannel>,
) where
    T: Resource + EncryptSave,
{
    for msg in save_message.read() {
        let save_id = **msg;
        if save_id == 0 {
            let file_name = format!("{}.dat", random_string(&mut rng));
            let mut saved_path = save_config.save_dir.join(file_name.as_str());
            if !saved_path.is_absolute() {
                saved_path = get_relative_path().join(saved_path);
            }
            if let Err(_e) = data.save_to(saved_path.clone(), &channel) {
                #[cfg(feature = "log")]
                error!("Failed to save data {}: {}", saved_path.display(), _e);
                save_finished.write(SaveFinished(false));
            } else {
                // TODO: Handle max_key == max of u32
                let new_key = if let Some(max_key) = save_config.saves.keys().max() { max_key + 1 } else { 1 };
                save_config.saves.insert(new_key, PathBuf::from(file_name));
                save_config.last_saved = new_key;
                current_save.0 = new_key;
                setting_changed.write(GameSettingChanged);
                save_finished.write(SaveFinished(true));
            }
        } else {
            if let Some(saved_path) = save_config.saves.get(&save_id) {
                let saved_path = save_config.save_dir.join(saved_path);
                if let Err(_e) = data.save_to(saved_path.clone(), &channel) {
                    #[cfg(feature = "log")]
                    error!("Failed to save data {}: {}", saved_path.display(), _e);
                    save_finished.write(SaveFinished(false));
                } else {
                    save_config.last_saved = save_id;
                    current_save.0 = save_id;
                    save_finished.write(SaveFinished(true));
                }
            }
        }
    }
}

fn on_new_save(mut save_message: MessageWriter<SaveGame>) {
    save_message.write(SaveGame(0));
}

fn on_quick_save(current_save: Res<CurrentSave>, mut save_message: MessageWriter<SaveGame>) {
    let save_id = **current_save;
    save_message.write(SaveGame(save_id));
}

fn on_delete(
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
    const ENCR_KEY: &'static str = "";

    fn load_from(&mut self, config_path: &Path, channel: &IoChannel) -> anyhow::Result<()> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let sender = channel.sender.clone();
            IoTaskPool::get()
                .spawn(async move {
                    let enc_saved = fs::read(config_path)?;
                    let decrypted = if Self::ENCR_KEY.is_empty() {
                        enc_saved
                    } else {
                        decrypt(enc_saved.as_slice(), Self::ENCR_KEY.as_bytes())?
                    };
                    *self = postcard::from_bytes(decrypted.as_slice())?;
                    sender.send(IoResult::success(IoAction::Load))
                })
                .detach();
        }
        Ok(())
    }

    fn save_to(&self, saved_path: PathBuf, channel: &IoChannel) -> anyhow::Result<()> {
        let data = postcard::to_allocvec(self)?;
        let enc_saved =
            if Self::ENCR_KEY.is_empty() { data } else { encrypt(data.as_slice(), Self::ENCR_KEY.as_bytes())? };

        #[cfg(not(target_arch = "wasm32"))]
        {
            let sender = channel.sender.clone();
            IoTaskPool::get()
                .spawn(async move {
                    if let Some(parent_dir) = saved_path.parent()
                        && let Err(e) = fs::create_dir_all(parent_dir)
                    {
                        sender.send(IoResult::failure(IoAction::Save, Err(anyhow::anyhow!(e))))?;
                    }
                    match File::create(saved_path).and_then(|mut file| file.write_all(enc_saved.as_slice())) {
                        Ok(_) => sender.send(IoResult::success(IoAction::Save)),
                        Err(e) => sender.send(IoResult::failure(IoAction::Save, Err(anyhow::anyhow!(e)))),
                    }
                })
                .detach();
        }
        Ok(())
    }
}

fn random_string(rng: &mut WyRand) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    const LEN: usize = 12;

    (0..LEN)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}
