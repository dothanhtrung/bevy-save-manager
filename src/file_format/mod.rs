use bevy::ecs::resource::Resource;
use crossbeam_channel::{
    Receiver,
    Sender,
};
use serde::{
    Deserialize,
    Serialize,
};

pub(crate) mod bin_file;
pub(crate) mod ron_file;

pub enum IoAction<T>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    Save(String),
    Load((String, Option<T>)), // TODO: Return data should be in IoResult
}

pub struct IoResult<T>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    pub action: IoAction<T>,
    pub result: anyhow::Result<()>,
}

impl<T> IoResult<T>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    pub fn success(action: IoAction<T>) -> Self {
        Self { action, result: Ok(()) }
    }

    pub fn failure(action: IoAction<T>, result: anyhow::Result<()>) -> Self {
        Self { action, result }
    }
}

#[derive(Resource)]
pub(crate) struct IoChannel<T>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    pub(crate) sender: Sender<IoResult<T>>,
    pub(crate) receiver: Receiver<IoResult<T>>,
}
