use bevy::{
    camera::RenderTarget,
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    post_process::bloom::Bloom,
    prelude::*,
    render::render_resource::TextureFormat,
    ui::IsDefaultUiCamera,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};

use crate::{
    GameState,
    fx::Trauma,
    player::Player,
    sky::fog_color,
    util::{damp, noise2},
    world::surface_height,
};

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraRig>()
            .add_systems(Startup, (spawn_camera, render_scale).chain())
            .add_systems(OnEnter(GameState::Playing), grab_cursor)
            .add_systems(Update, (cursor_toggle, orbit_input, follow).chain());
    }
}

#[derive(Resource)]
pub struct CameraRig {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    focus: Vec3,
    eye: Vec3,
    look: Vec3,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.5,
            distance: 15.0,
            focus: Vec3::ZERO,
            eye: Vec3::new(0.0, 80.0, 200.0),
            look: Vec3::ZERO,
        }
    }
}

impl CameraRig {
    /// Horizontal forward direction the player's input is relative to.
    pub fn forward(&self) -> Vec3 {
        Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos())
    }
}

fn spawn_camera(mut commands: Commands, mut rig: ResMut<CameraRig>, quality: Res<crate::Quality>) {
    if let Some(yaw) = std::env::var("EMBER_YAW").ok().and_then(|s| s.parse().ok()) {
        rig.yaw = yaw;
    }
    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(fog_color(0.0)),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            fov: 55f32.to_radians(),
            far: 2000.0,
            ..default()
        }),
        Tonemapping::TonyMcMapface,
        Bloom {
            intensity: 0.32,
            ..Bloom::NATURAL
        },
        DistanceFog {
            color: fog_color(0.0),
            falloff: FogFalloff::Linear { start: 70.0, end: 340.0 },
            ..default()
        },
        if quality.low { Msaa::Off } else { Msaa::Sample4 },
        Transform::from_xyz(0.0, 80.0, 200.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Renders the world into a smaller texture in low quality (or with
/// `EMBER_RENDER_SCALE`), keeping the UI crisp at native resolution.
fn render_scale(
    mut commands: Commands,
    quality: Res<crate::Quality>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(Entity, &mut Camera), With<Camera3d>>,
    mut images: ResMut<Assets<Image>>,
) {
    let scale = std::env::var("EMBER_RENDER_SCALE")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(if quality.low { 0.4 } else { 1.0 });
    if scale >= 1.0 {
        return;
    }
    info!("rendering world at {:.0}% resolution", scale * 100.0);
    let w = ((window.physical_width() as f32 * scale) as u32).max(64);
    let h = ((window.physical_height() as f32 * scale) as u32).max(36);
    let image = images.add(Image::new_target_texture(w, h, TextureFormat::Rgba8Unorm, Some(TextureFormat::Rgba8UnormSrgb)));
    let (entity, mut cam) = camera.into_inner();
    cam.order = -1;
    commands.entity(entity).insert(RenderTarget::Image(image.clone().into()));
    commands.spawn((Camera2d, Camera { order: 0, ..default() }, IsDefaultUiCamera));
    commands.spawn((
        ImageNode::new(image),
        Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, ..default() },
        GlobalZIndex(-1),
    ));
}

fn grab_cursor(mut cursor: Single<&mut CursorOptions>) {
    cursor.visible = false;
    cursor.grab_mode = CursorGrabMode::Locked;
}

fn cursor_toggle(
    mut cursor: Single<&mut CursorOptions>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        cursor.visible = true;
        cursor.grab_mode = CursorGrabMode::None;
    }
    if mouse.just_pressed(MouseButton::Left) && *state.get() != GameState::Title {
        cursor.visible = false;
        cursor.grab_mode = CursorGrabMode::Locked;
    }
}

fn orbit_input(
    mut rig: ResMut<CameraRig>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Single<&CursorOptions>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    if cursor.grab_mode != CursorGrabMode::None || mouse.pressed(MouseButton::Right) {
        rig.yaw -= motion.delta.x * 0.0035;
        rig.pitch += motion.delta.y * 0.0028;
    }
    let mut key_yaw = 0.0;
    if keys.pressed(KeyCode::KeyQ) || keys.pressed(KeyCode::ArrowLeft) {
        key_yaw += 1.0;
    }
    if keys.pressed(KeyCode::KeyE) || keys.pressed(KeyCode::ArrowRight) {
        key_yaw -= 1.0;
    }
    rig.yaw += key_yaw * 2.2 * dt;
    if keys.pressed(KeyCode::ArrowUp) {
        rig.pitch -= 1.2 * dt;
    }
    if keys.pressed(KeyCode::ArrowDown) {
        rig.pitch += 1.2 * dt;
    }
    rig.pitch = rig.pitch.clamp(-0.15, 1.25);
    rig.distance = (rig.distance - scroll.delta.y * 1.2).clamp(6.0, 26.0);
}

fn follow(
    time: Res<Time>,
    mut rig: ResMut<CameraRig>,
    trauma: Res<Trauma>,
    state: Res<State<GameState>>,
    player: Single<(&Transform, &Player), Without<Camera3d>>,
    mut cam: Single<&mut Transform, With<Camera3d>>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    let (ptf, p) = player.into_inner();

    let (eye, look, rate) = match state.get() {
        GameState::Title => {
            // Slow cinematic drift around the archipelago.
            let a = t * 0.05 + 0.6;
            let eye = Vec3::new(a.sin() * 150.0, 55.0 + (t * 0.1).sin() * 8.0, a.cos() * 150.0);
            (eye, Vec3::new(0.0, 4.0, 0.0), 2.0)
        }
        _ => {
            let lead = Vec3::new(p.vel.x, 0.0, p.vel.z) * 0.18;
            rig.focus = rig.focus.lerp(ptf.translation + lead, damp(7.0, dt));
            let dir = Vec3::new(
                rig.yaw.sin() * rig.pitch.cos(),
                rig.pitch.sin(),
                rig.yaw.cos() * rig.pitch.cos(),
            );
            let anchor = rig.focus + Vec3::Y * 1.4;
            let mut eye = anchor + dir * rig.distance;
            // Lift the camera until no terrain sits between it and the player.
            for k in 1..=8 {
                let f = k as f32 / 8.0;
                let p = anchor.lerp(eye, f);
                let need = surface_height(p.x, p.z, t) + 1.0;
                if p.y < need {
                    eye.y += (need - p.y) / f;
                }
            }
            let rate = if *state.get() == GameState::Victory { 2.0 } else { 9.0 };
            (eye, rig.focus + Vec3::Y * 1.2, rate)
        }
    };

    let eye_now = rig.eye.lerp(eye, damp(rate, dt));
    let look_now = rig.look.lerp(look, damp(rate * 1.3, dt));
    rig.eye = eye_now;
    rig.look = look_now;

    let shake = trauma.0 * trauma.0;
    let offset = Vec3::new(
        noise2(Vec2::new(t * 18.0, 0.0), 5),
        noise2(Vec2::new(t * 18.0, 3.0), 6),
        noise2(Vec2::new(t * 18.0, 7.0), 7),
    ) * shake
        * 0.9;
    **cam = Transform::from_translation(eye_now + offset).looking_at(look_now, Vec3::Y);
}
