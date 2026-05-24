use crate::file_format::{
    IoAction,
    IoChannel,
    IoResult,
    bin_file,
};
use crate::setting::{
    GameSetting,
    GameSettingChanged,
    GameSettingPlugin,
};
use crate::{
    FileFormat,
    get_relative_path,
};
use anyhow::anyhow;
use bevy::app::{
    App,
    Startup,
};
use bevy::platform::collections::HashMap;
#[cfg(feature = "log")]
use bevy::prelude::error;
use bevy::prelude::{
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
    on_message,
};
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
use std::fs;
use std::path::{
    Path,
    PathBuf,
};
use std::time::SystemTime;

#[derive(Default)]
pub struct GameSavePlugin<T>
where
    T: Resource + Default + EncryptSave,
{
    _config: Option<T>,
}

impl<T> Plugin for GameSavePlugin<T>
where
    T: Resource + Default + EncryptSave,
{
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<EntropyPlugin<WyRand>>() {
            app.add_plugins(EntropyPlugin::<WyRand>::default());
        }

        app.add_plugins(GameSettingPlugin::<SaveConfig>::default())
            .insert_resource(T::default())
            .insert_resource(SaveFileFormat(FileFormat::Bin))
            .insert_resource(CurrentSave::default())
            .insert_resource(PlayTimeTrack(0))
            .add_message::<NewSave>()
            .add_message::<QuickSave>()
            .add_message::<SaveGame>()
            .add_message::<DeleteSave>()
            .add_message::<LoadGame>()
            .add_message::<LoadRecent>()
            .add_message::<LoadGameSaveFinished>()
            .add_message::<SaveGameFinished>()
            .add_systems(Startup, setup_channel::<T>)
            .add_systems(Update, listen_channel::<T>)
            .add_systems(Update, on_load::<T>.run_if(on_message::<LoadGame>))
            .add_systems(Update, on_load_recent.run_if(on_message::<LoadRecent>))
            .add_systems(Update, on_save::<T>.run_if(on_message::<SaveGame>))
            .add_systems(Update, on_new_save::<T>.run_if(on_message::<NewSave>))
            .add_systems(Update, on_quick_save.run_if(on_message::<QuickSave>))
            .add_systems(Update, on_delete.run_if(on_message::<DeleteSave>));
    }
}

#[derive(Resource, Deref, DerefMut)]
pub struct SaveFileFormat(pub FileFormat);

/// Save to current save file.
#[derive(Message)]
pub struct QuickSave;

/// Create new save
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
pub struct LoadGameSaveFinished(pub anyhow::Result<()>);

/// Fired when saving to file finished
#[derive(Message, Deref, DerefMut)]
pub struct SaveGameFinished(pub anyhow::Result<()>);

#[derive(Resource, Default)]
pub struct CurrentSave {
    pub save_id: u32,
    pub duration: u64,
}

impl CurrentSave {
    fn reset(&mut self) {
        self.save_id = 0;
        self.duration = 0;
    }
}

#[derive(Resource, Deref, DerefMut)]
struct PlayTimeTrack(u64);

#[derive(Deserialize, Serialize, Clone)]
pub struct SaveInfo {
    pub name: String,
    /// Path to save file. Alternative to `save_dir`.
    pub path: PathBuf,
    /// Play duration in UNIX epoch
    pub duration: u64,
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

pub fn setup_channel<T>(mut commands: Commands)
where
    T: Serialize + for<'de> Deserialize<'de> + Send + 'static,
{
    let (sender, receiver) = crossbeam_channel::unbounded::<IoResult<T>>();
    commands.insert_resource(IoChannel { sender, receiver });
}

fn on_load<T>(
    mut load_message: MessageReader<LoadGame>,
    save_config: Res<SaveConfig>,
    mut load_finished: MessageWriter<LoadGameSaveFinished>,
    channel: Res<IoChannel<T>>,
    file_format: Res<SaveFileFormat>,
) where
    T: Resource + EncryptSave,
{
    for id in load_message.read() {
        if let Some(info) = save_config.saves.get(&id.0) {
            let saved_path = save_config.save_dir.join(&info.path);
            T::load_from(&saved_path, &channel.sender, &file_format);
        } else {
            load_finished.write(LoadGameSaveFinished(Err(anyhow!("Save file does not exist"))));
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
    mut play_time_track: ResMut<PlayTimeTrack>,
    channel: Res<IoChannel<T>>,
    data: Res<T>,
    file_format: Res<SaveFileFormat>,
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
        data.save_to(saved_path.clone(), &channel, &file_format.0);

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        save_config.saves.insert(
            save_id,
            SaveInfo {
                name: msg.0.clone(),
                path: PathBuf::from(file_name),
                duration: 0,
                modified_at: now,
            },
        );
        setting_changed.write(GameSettingChanged);
        **play_time_track = now;
    }
}

fn on_save<T>(
    save_data: Res<T>,
    mut save_message: MessageReader<SaveGame>,
    save_config: Res<SaveConfig>,
    channel: Res<IoChannel<T>>,
    mut new_save: MessageWriter<NewSave>,
    file_format: Res<SaveFileFormat>,
) where
    T: Resource + EncryptSave,
{
    for msg in save_message.read() {
        let save_id = **msg;
        if let Some(info) = save_config.saves.get(&save_id) {
            let saved_path = save_config.save_dir.join(&info.path);
            save_data.save_to(saved_path.clone(), &channel, &file_format.0);
        } else {
            // save_id does not exist. Create a new save.
            new_save.write(NewSave(String::new()));
        }
    }
}

fn listen_channel<T>(
    channel: Res<IoChannel<T>>,
    mut save_data: ResMut<T>,
    mut save_message: MessageWriter<SaveGameFinished>,
    mut load_message: MessageWriter<LoadGameSaveFinished>,
    mut current_save: ResMut<CurrentSave>,
    mut save_config: ResMut<SaveConfig>,
    mut setting_changed: MessageWriter<GameSettingChanged>,
    mut play_time_track: ResMut<PlayTimeTrack>,
) where
    T: Resource + EncryptSave,
{
    for msg in channel.receiver.try_iter() {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        match msg.action {
            IoAction::Save(file_path) => {
                let save_id = find_save_id(&save_config, &file_path);
                if save_id == 0 {
                    continue;
                }
                save_config.last_saved = save_id;
                current_save.save_id = save_id;

                save_message.write(SaveGameFinished(msg.result));

                if let Some(info) = save_config.saves.get_mut(&save_id) {
                    current_save.duration += now - **play_time_track;
                    info.duration = current_save.duration;
                    **play_time_track = now;
                    setting_changed.write(GameSettingChanged);
                }
            }
            IoAction::Load((file_path, data)) => {
                let save_id = find_save_id(&save_config, &file_path);
                if save_id == 0 {
                    continue;
                }
                match msg.result {
                    Ok(data)
                    current_save.save_id = save_id;
                    if let Some(info) = save_config.saves.get(&save_id) {
                        current_save.duration = info.duration;
                    }
                    *save_data = data;
                    // match postcard::from_bytes(data.as_slice()) {
                    //     Ok(ret) => *save_data = ret,
                    //     Err(e) => {
                    //         load_message.write(LoadGameSaveFinished(Err(anyhow!(e))));
                    //         continue;
                    //     }
                    // }
                    load_message.write(LoadGameSaveFinished(msg.result));
                    **play_time_track = now;
                }
            }
        }
    }
}

fn on_quick_save(current_save: Res<CurrentSave>, mut save_message: MessageWriter<SaveGame>) {
    let save_id = current_save.save_id;
    save_message.write(SaveGame(save_id));
}

fn on_delete(
    mut current_save: ResMut<CurrentSave>,
    mut delete_event: MessageReader<DeleteSave>,
    mut save_config: ResMut<SaveConfig>,
) {
    for saved_id in delete_event.read() {
        if let Some(info) = save_config.saves.get(&saved_id.0) {
            if let Err(_e) = fs::remove_file(&info.path) {
                #[cfg(feature = "log")]
                error!("Failed to delete save data {}: {}", info.path.display(), _e);
            } else {
                save_config.saves.remove(&saved_id.0);
                current_save.reset();
                if save_config.last_saved == **saved_id {
                    save_config.last_saved = 0;
                }
            }
        }
    }
}

pub trait EncryptSave: Serialize + for<'de> Deserialize<'de> {
    const ENCR_KEY: &'static str = "";

    fn load_from(config_path: &Path, sender: &Sender<IoResult<Self>>, file_format: &FileFormat) {
        match *file_format {
            FileFormat::Ron => {}
            FileFormat::Bin => {
                bin_file::load_from(PathBuf::from(config_path), sender.clone(), String::from(Self::ENCR_KEY));
            }
        }
    }

    fn save_to(&self, saved_path: PathBuf, channel: &IoChannel<Self>, file_format: &FileFormat) {
        let sender = channel.sender.clone();
        match *file_format {
            FileFormat::Ron => {}
            FileFormat::Bin => {
                bin_file::save_to(self, saved_path, Self::ENCR_KEY, sender.clone());
            }
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

fn find_save_id(save_config: &SaveConfig, file_path: &str) -> u32 {
    if file_path.is_empty() {
        return 0;
    }
    for (id, info) in save_config.saves.iter() {
        if file_path == save_config.save_dir.join(&info.path).to_str().unwrap_or_default() {
            return *id;
        }
    }
    0
}
