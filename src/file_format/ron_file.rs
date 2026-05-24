use bevy::tasks::IoTaskPool;
use ron::ser::{
    PrettyConfig,
    to_string_pretty,
};
use serde::{
    Deserialize,
    Serialize,
};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

pub(crate) fn load_from<T>(config_path: &PathBuf) -> anyhow::Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let file = File::open(config_path)?;
    let ret = ron::de::from_reader(file)?;
    Ok(ret)
}

pub(crate) fn save_to<T>(data: &T, config_path: PathBuf) -> anyhow::Result<()>
where
    T: Serialize,
{
    let pretty = PrettyConfig::default();
    let ron_str = to_string_pretty(&data, pretty)?;

    #[cfg(not(target_family = "wasm"))]
    IoTaskPool::get()
        .spawn(async move {
            if let Some(parent_dir) = config_path.parent() {
                std::fs::create_dir_all(parent_dir)?;
            }
            let mut file = File::create(config_path)?;
            file.write_all(ron_str.as_bytes()).map_err(|e| anyhow::anyhow!(e))
        })
        .detach();

    Ok(())
}
