use crate::get_relative_path;
use crate::setting::{
    GameSetting,
    GameSettingChanged,
    GameSettingPlugin,
};
use anyhow::anyhow;
use bevy::app::{
    App,
    Startup,
};
#[cfg(feature = "log")]
use bevy::prelude::error;
use bevy::prelude::{
    on_message,
    Commands,
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
use crossbeam_channel::{
    Receiver,
    Sender,
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
use std::io::Write;
use std::path::{
    Path,
    PathBuf,
};
use std::time::SystemTime;

#[derive(Default)]
pub struct GameSavePlugin<T>
where
    T: Resource + Default + EncryptSave + Clone,
{
    _config: Option<T>,
}

impl<T> Plugin for GameSavePlugin<T>
where
    T: Resource + Default + EncryptSave + Clone,
{
    fn build(&self, app: &mut App) {
        app.add_plugins(EntropyPlugin::<WyRand>::default())
            .add_plugins(GameSettingPlugin::<SaveConfig>::default())
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
            .add_systems(Update, listen_channel::<T>)
            .add_systems(Update, on_load::<T>.run_if(on_message::<LoadGame>))
            .add_systems(Update, on_load_recent.run_if(on_message::<LoadRecent>))
            .add_systems(Update, on_save::<T>.run_if(on_message::<SaveGame>))
            .add_systems(Update, on_new_save::<T>.run_if(on_message::<NewSave>))
            .add_systems(Update, on_quick_save.run_if(on_message::<QuickSave>))
            .add_systems(Update, on_delete.run_if(on_message::<DeleteSave>));
    }
}

#[derive(Message)]
pub struct QuickSave;

#[derive(Message, Deref, DerefMut)]
pub struct NewSave(pub String);

/// Tell system to save data to file by save id.
#[derive(Message, Deref, DerefMut)]
pub struct SaveGame(pub u32);

#[derive(Message, Deref, DerefMut)]
pub struct DeleteSave(pub u32);

#[derive(Message, Deref, DerefMut)]
pub struct LoadGame(pub u32);

#[derive(Message)]
pub struct LoadRecent;

/// Fired when loading from file finished
#[derive(Message, Deref, DerefMut)]
pub struct LoadFinished(anyhow::Result<()>);

/// Fired when saving to file finished
#[derive(Message, Deref, DerefMut)]
pub struct SaveFinished(anyhow::Result<()>);

#[derive(Resource, Deref, DerefMut)]
pub struct CurrentSave(pub u32);

#[derive(Deserialize, Serialize, Clone)]
pub struct SaveInfo {
    pub name: String,
    pub path: PathBuf,
    // TODO: Play time in seconds
    // pub duration: u64,
    /// Modified time in UNIX epoch
    pub modified_at: u64,
}

#[derive(Resource, Deserialize, Serialize, Clone)]
pub struct SaveConfig {
    /// Valid save id start from 1
    pub saves: HashMap<u32, SaveInfo>,
    pub save_dir: PathBuf,
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

pub enum IoAction {
    Save(u32),
    Load((u32, Vec<u8>)),
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

pub fn setup_channel(mut commands: Commands) {
    let (sender, receiver) = crossbeam_channel::unbounded();
    commands.insert_resource(IoChannel { sender, receiver });
}

fn on_load<T>(
    mut load_message: MessageReader<LoadGame>,
    save_config: Res<SaveConfig>,
    mut load_finished: MessageWriter<LoadFinished>,
    channel: Res<IoChannel>,
) where
    T: Resource + EncryptSave,
{
    for id in load_message.read() {
        if let Some(info) = save_config.saves.get(&id.0) {
            let saved_path = save_config.save_dir.join(&info.path);
            let sender = channel.sender.clone();
            let save_id = id.0;
            IoTaskPool::get()
                .spawn(async move {
                    T::load_from(&saved_path, sender, save_id);
                })
                .detach();
        } else {
            load_finished.write(LoadFinished(Err(anyhow!("Save file does not exist"))));
        }
    }
}

fn on_load_recent(save_config: Res<SaveConfig>, mut load_message: MessageWriter<LoadGame>) {
    load_message.write(LoadGame(save_config.last_saved));
}

fn on_new_save<T>(
    mut new_save_msg: MessageReader<NewSave>,
    mut rng: Single<&mut WyRand, With<GlobalRng>>,
    mut save_config: ResMut<SaveConfig>,
    mut setting_changed: MessageWriter<GameSettingChanged>,
    channel: Res<IoChannel>,
    data: Res<T>,
) where
    T: Resource + EncryptSave,
{
    for msg in new_save_msg.read() {
        let file_name = format!("{}.dat", random_string(&mut rng));
        let mut saved_path = save_config.save_dir.join(file_name.as_str());
        if !saved_path.is_absolute() {
            saved_path = get_relative_path().join(saved_path);
        }

        // TODO: Handle max_key == max of u32
        let save_id = if let Some(max_key) = save_config.saves.keys().max() { max_key + 1 } else { 1 };
        data.save_to(saved_path.clone(), &channel, save_id);

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        save_config.saves.insert(
            save_id,
            SaveInfo {
                name: msg.0.clone(),
                path: PathBuf::from(file_name),
                modified_at: now,
            },
        );
        setting_changed.write(GameSettingChanged);
    }
}

fn on_save<T>(
    data: Res<T>,
    mut save_message: MessageReader<SaveGame>,
    mut save_config: ResMut<SaveConfig>,
    channel: Res<IoChannel>,
) where
    T: Resource + EncryptSave,
{
    for msg in save_message.read() {
        let save_id = **msg;
        if let Some(info) = save_config.saves.get(&save_id) {
            let saved_path = save_config.save_dir.join(&info.path);
            data.save_to(saved_path.clone(), &channel, save_id);
        }
    }
}

fn listen_channel<T>(
    channel: Res<IoChannel>,
    mut save_data: ResMut<T>,
    mut save_message: MessageWriter<SaveFinished>,
    mut load_message: MessageWriter<LoadFinished>,
    mut current_save: ResMut<CurrentSave>,
    mut save_config: ResMut<SaveConfig>,
) where
    T: Resource + EncryptSave,
{
    for msg in channel.receiver.try_iter() {
        match msg.action {
            IoAction::Save(save_id) => {
                save_config.last_saved = save_id;
                current_save.0 = save_id;

                save_message.write(SaveFinished(msg.result));
            }
            IoAction::Load((save_id, data)) => {
                if msg.result.is_ok() {
                    current_save.0 = save_id;
                    match postcard::from_bytes(data.as_slice()) {
                        Ok(ret) => *save_data = ret,
                        Err(e) => {
                            load_message.write(LoadFinished(Err(anyhow!(e))));
                        }
                    }
                    load_message.write(LoadFinished(msg.result));
                }
            }
        }
    }
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
        if let Some(info) = save_config.saves.get(saved_id) {
            if let Err(_e) = fs::remove_file(&info.path) {
                #[cfg(feature = "log")]
                error!("Failed to delete save data {}: {}", info.path.display(), _e);
            } else {
                save_config.saves.remove(saved_id);
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

    fn load_from(config_path: &Path, sender: Sender<IoResult>, save_id: u32) {
        if cfg!(target_family = "wasm") {
            let _ = sender.send(IoResult::failure(
                IoAction::Load((save_id, Vec::new())),
                Err(anyhow!("Not support WASM")),
            ));
            return;
        } else {
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

            let decrypted = if Self::ENCR_KEY.is_empty() {
                enc_saved
            } else {
                match decrypt(enc_saved.as_slice(), Self::ENCR_KEY.as_bytes()) {
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
        }
    }

    fn save_to(&self, saved_path: PathBuf, channel: &IoChannel, save_id: u32) {
        let sender = channel.sender.clone();
        if cfg!(target_family = "wasm") {
            let _ = sender.send(IoResult::failure(
                IoAction::Save(save_id),
                Err(anyhow!("Not support WASM")),
            ));
            return;
        } else {
            let data = match postcard::to_allocvec(self) {
                Ok(ret) => ret,
                Err(e) => {
                    let _ = sender.send(IoResult::failure(IoAction::Save(save_id), Err(anyhow!(e))));
                    return;
                }
            };

            IoTaskPool::get()
                .spawn(async move {
                    let enc_saved = if Self::ENCR_KEY.is_empty() {
                        data
                    } else {
                        match encrypt(data.as_slice(), Self::ENCR_KEY.as_bytes()) {
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
