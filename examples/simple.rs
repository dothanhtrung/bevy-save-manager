use std::time::Duration;

use bevy::time::common_conditions::on_timer;
use bevy::{
    color::palettes::tailwind::RED_900,
    prelude::*,
};
use bevy_save_manager::save::{
    EncryptSave,
    GameSavePlugin,
    LoadRecent,
    QuickSave,
};
use serde::{
    Deserialize,
    Serialize,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // Add the plugin
        .add_plugins(GameSavePlugin::<SaveData>::default())
        .add_systems(PreStartup, load_game)
        .add_systems(Startup, setup)
        .add_systems(Update, save.run_if(on_timer(Duration::from_secs(1))))
        .add_systems(Update, game_process.run_if(on_timer(Duration::from_millis(100))))
        .run();
}

#[derive(Resource, Default, Serialize, Deserialize)]
struct SaveData {
    pos: Vec3,
}

impl EncryptSave for SaveData {}

fn load_game(mut load_request: MessageWriter<LoadRecent>) {
    load_request.write(LoadRecent);
}

fn setup(mut commands: Commands, saved_data: Res<SaveData>) {
    commands.spawn((
        Sprite::from_color(RED_900, Vec2::new(64., 64.)),
        Transform::from_translation(saved_data.pos),
    ));
}

fn save(mut save_request: MessageWriter<QuickSave>) {
    save_request.write(QuickSave);
}

fn game_process(mut query: Query<&mut Transform, With<Sprite>>) {
    for mut transform in query.iter_mut() {
        transform.translation.x = ((transform.translation.x as u32 + 10) / 128) as f32;
    }
}
