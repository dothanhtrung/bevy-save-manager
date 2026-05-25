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

pub enum IoAction {
    Save(String),
    Load(String),
}

pub struct IoResult<T>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    pub action: IoAction,
    pub result: Result<Option<T>, String>,
}

impl<T> IoResult<T>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    pub fn success(action: IoAction, data: Option<T>) -> Self {
        Self {
            action,
            result: Ok(data),
        }
    }

    pub fn failure(action: IoAction, err: &str) -> Self {
        Self {
            action,
            result: Err(err.to_string()),
        }
    }
}

#[derive(Resource)]
pub struct IoChannel<T>
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    pub(crate) sender: Sender<IoResult<T>>,
    pub(crate) receiver: Receiver<IoResult<T>>,
}
