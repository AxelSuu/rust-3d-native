mod audio;
mod beacon;
mod camera;
mod ember;
mod fx;
mod player;
mod sky;
mod ui;
mod util;
mod wisp;
mod world;

use bevy::{
    prelude::*,
    render::{
        renderer::RenderAdapterInfo,
        view::screenshot::{Screenshot, save_to_disk},
    },
    window::PresentMode,
};

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameState {
    #[default]
    Title,
    Playing,
    Victory,
}

pub const BEACON_COUNT: usize = 5;
pub const EMBERS_PER_BEACON: u32 = 5;

#[derive(Resource, Default)]
pub struct Progress {
    pub lit: usize,
    pub elapsed: f32,
    pub embers_gathered: u32,
    pub times_caught: u32,
}

/// Marks everything that is torn down and rebuilt on restart.
#[derive(Component)]
pub struct LevelEntity;

#[derive(Message)]
pub struct RestartLevel;

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.05, 0.05, 0.12)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Ember Isles".into(),
                resolution: (1600, 900).into(),
                present_mode: PresentMode::AutoVsync,
                ..default()
            }),
            ..default()
        }))
        .init_state::<GameState>()
        .init_resource::<Progress>()
        .add_message::<RestartLevel>()
        .add_plugins((
            fx::FxPlugin,
            world::WorldPlugin,
            sky::SkyPlugin,
            camera::CameraPlugin,
            player::PlayerPlugin,
            ember::EmberPlugin,
            beacon::BeaconPlugin,
            wisp::WispPlugin,
            audio::SoundPlugin,
            ui::UiPlugin,
        ))
        .add_systems(Update, (tick_progress, restart).chain())
        .add_systems(PreStartup, detect_quality)
        .add_systems(Update, auto_screenshot)
        .run();
}

fn tick_progress(time: Res<Time>, state: Res<State<GameState>>, mut progress: ResMut<Progress>) {
    if *state.get() == GameState::Playing {
        progress.elapsed += time.delta_secs();
    }
}

fn restart(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    mut progress: ResMut<Progress>,
    mut writer: MessageWriter<RestartLevel>,
) {
    if *state.get() == GameState::Victory && keys.just_pressed(KeyCode::KeyR) {
        *progress = Progress::default();
        writer.write(RestartLevel);
        next.set(GameState::Playing);
    }
}

/// Dev aid: `EMBER_SHOT=out.png EMBER_SHOT_AT=6 EMBER_STATE=play cargo run`
/// renders, saves a screenshot and quits.
fn auto_screenshot(
    mut commands: Commands,
    time: Res<Time>,
    mut done: Local<u8>,
    mut frames: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(path) = std::env::var("EMBER_SHOT") else { return };
    let at: f32 = std::env::var("EMBER_SHOT_AT").ok().and_then(|s| s.parse().ok()).unwrap_or(5.0);
    let t = time.elapsed_secs();
    if t > at - 4.0 && *done == 0 {
        *frames += 1;
    }
    if *done == 0 && t > at {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        *done = 1;
    } else if *done == 1 && t > at + 1.5 {
        info!("fps {:.1}", *frames as f32 / 4.0);
        exit.write(AppExit::Success);
        *done = 2;
    }
}

/// Lighter rendering (no shadows, MSAA or ember lights, reduced render scale).
/// On by default for software rasterizers; `EMBER_LOW=0|1` overrides.
#[derive(Resource)]
pub struct Quality {
    pub low: bool,
}

fn detect_quality(mut commands: Commands, adapter: Option<Res<RenderAdapterInfo>>) {
    let software = adapter.is_some_and(|a| a.name.contains("llvmpipe") || a.name.contains("SwiftShader"));
    let low = match std::env::var("EMBER_LOW").as_deref() {
        Ok("0") => false,
        Ok(_) => true,
        Err(_) => software,
    };
    if low {
        info!("using low quality rendering");
    }
    commands.insert_resource(Quality { low });
}

pub fn debug_state() -> Option<String> {
    std::env::var("EMBER_STATE").ok()
}
