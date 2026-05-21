use crate::file_format::ron_file;
use crate::{
    FileFormat,
    get_relative_path,
};
use bevy::app::App;
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
            .add_systems(Startup, load_config::<T>)
            .add_systems(Update, save_config::<T>.run_if(on_message::<GameSettingChanged>));
    }
}

#[derive(Resource, Default, Deref, DerefMut)]
pub struct SettingFileFormat(pub FileFormat);

#[derive(Message)]
pub struct GameSettingChanged;

#[derive(Message)]
pub struct GameSettingLoaded;

fn load_config<T>(
    mut config: ResMut<T>,
    mut event: MessageWriter<GameSettingLoaded>,
    file_format: Res<SettingFileFormat>,
) where
    T: Resource + GameSetting,
{
    if let Err(_e) = config.load(&file_format.0) {
        #[cfg(feature = "log")]
        warn!(
            "Failed to load game config {} : {}",
            T::config_path().as_path().to_str().unwrap_or_default(),
            _e
        );
    } else {
        event.write(GameSettingLoaded);
    }
}

fn save_config<T>(config: Res<T>, file_format: Res<SettingFileFormat>)
where
    T: Resource + GameSetting,
{
    if let Err(_e) = config.save(&file_format.0) {
        #[cfg(feature = "log")]
        warn!(
            "Failed to save game config {}: {}",
            T::config_path().as_path().to_str().unwrap_or_default(),
            _e
        );
    }
}

// TODO: Return Load/Save Result
pub trait GameSetting: Serialize + for<'de> Deserialize<'de> {
    const DEFAULT_CONF: &'static str = "game_setting.conf";

    fn config_path() -> PathBuf {
        let mut ret = PathBuf::from(Self::DEFAULT_CONF);
        if !ret.is_absolute() {
            ret = get_relative_path().join(ret);
        }
        ret
    }

    fn load(&mut self, file_format: &FileFormat) -> anyhow::Result<()> {
        match *file_format {
            FileFormat::Ron => {
                *self = ron_file::load_from(&Self::config_path())?;
            }
            FileFormat::Bin => {}
        }
        Ok(())
    }

    fn save(&self, file_format: &FileFormat) -> anyhow::Result<()> {
        match *file_format {
            FileFormat::Ron => {
                ron_file::save_to(self, Self::config_path())?;
            }
            FileFormat::Bin => {}
        }
        Ok(())
    }
}
