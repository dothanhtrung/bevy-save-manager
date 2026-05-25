use crate::file_format::{
    IoAction,
    IoChannel,
    IoResult,
    bin_file,
    ron_file,
};
use crate::{
    FileFormat,
    get_relative_path,
};
use bevy::app::App;
use bevy::ecs::system::Commands;
#[cfg(feature = "log")]
use bevy::prelude::warn;
use bevy::prelude::{
    Deref,
    DerefMut,
    IntoScheduleConfigs,
    Message,
    MessageWriter,
    Plugin,
    Res,
    ResMut,
    Resource,
    Startup,
    Update,
    on_message,
};
use crossbeam_channel::Sender;
use serde::{
    Deserialize,
    Serialize,
};
use std::path::PathBuf;

#[derive(Default)]
pub struct GameSettingPlugin<T>
where
    T: Resource + Default + GameSetting,
{
    _config: Option<T>,
}

impl<T> Plugin for GameSettingPlugin<T>
where
    T: Resource + Default + GameSetting,
{
    fn build(&self, app: &mut App) {
        app.insert_resource(T::default())
            .insert_resource(SettingFileFormat::default())
            .add_message::<GameSettingChanged>()
            .add_message::<GameSettingLoaded>()
            .add_systems(Startup, (setup_channel::<T>, load_config::<T>).chain())
            .add_systems(Update, save_config::<T>.run_if(on_message::<GameSettingChanged>))
            .add_systems(Update, listen_channel::<T>);
    }
}

#[derive(Resource, Default, Deref, DerefMut)]
pub struct SettingFileFormat(pub FileFormat);

#[derive(Message)]
pub struct GameSettingChanged;

#[derive(Message)]
pub struct GameSettingLoaded(pub Result<(), String>);

#[derive(Message)]
pub struct GameSettingSaved(pub Result<(), String>);

fn setup_channel<T>(mut commands: Commands)
where
    T: Serialize + for<'de> Deserialize<'de> + Send + 'static,
{
    let (sender, receiver) = crossbeam_channel::unbounded::<IoResult<T>>();
    commands.insert_resource(IoChannel { sender, receiver });
}

fn listen_channel<T>(
    channel: Res<IoChannel<T>>,
    mut setting: ResMut<T>,
    mut save_message: MessageWriter<GameSettingSaved>,
    mut load_message: MessageWriter<GameSettingLoaded>,
) where
    T: Resource + GameSetting,
{
    for msg in channel.receiver.try_iter() {
        match msg.action {
            IoAction::Save(_) => match msg.result {
                Ok(_) => {
                    save_message.write(GameSettingSaved(Ok(())));
                }
                Err(e) => {
                    save_message.write(GameSettingSaved(Err(e.to_string())));
                    continue;
                }
            },
            IoAction::Load(_) => match msg.result {
                Ok(Some(data)) => {
                    *setting = data;
                    load_message.write(GameSettingLoaded(Ok(())));
                }
                Ok(None) => {
                    load_message.write(GameSettingLoaded(Err("Data is empty".to_string())));
                    continue;
                }
                Err(e) => {
                    load_message.write(GameSettingLoaded(Err(e)));
                    continue;
                }
            },
        }
    }
}

fn load_config<T>(mut config: ResMut<T>, file_format: Res<SettingFileFormat>, channel: Res<IoChannel<T>>)
where
    T: Resource + GameSetting,
{
    config.load(&channel.sender, &file_format.0);
}

fn save_config<T>(config: Res<T>, file_format: Res<SettingFileFormat>, channel: Res<IoChannel<T>>)
where
    T: Resource + GameSetting,
{
    config.save(&channel.sender, &file_format.0);
}

pub trait GameSetting: Serialize + for<'de> Deserialize<'de> + Send + 'static {
    const DEFAULT_CONF: &'static str = "game_setting.conf";

    fn config_path() -> PathBuf {
        let mut ret = PathBuf::from(Self::DEFAULT_CONF);
        if !ret.is_absolute() {
            ret = get_relative_path().join(ret);
        }
        ret
    }

    fn load(&mut self, sender: &Sender<IoResult<Self>>, file_format: &FileFormat) {
        match *file_format {
            FileFormat::Ron => {
                ron_file::load_from(Self::config_path(), sender.clone());
            }
            FileFormat::Bin => {
                bin_file::load_from(Self::config_path(), sender.clone(), String::new());
            }
        }
    }

    fn save(&self, sender: &Sender<IoResult<Self>>, file_format: &FileFormat) {
        match *file_format {
            FileFormat::Ron => {
                ron_file::save_to(self, Self::config_path(), sender.clone());
            }
            FileFormat::Bin => {
                ron_file::save_to(self, Self::config_path(), sender.clone());
            }
        }
    }
}
