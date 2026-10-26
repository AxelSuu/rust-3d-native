use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
};

use crate::{
    GameState, RestartLevel,
    audio::{Sfx, SfxKind},
    camera::CameraRig,
    fx::{Burst, FxAssets, FxRng, Palette, SizeCurve, Trauma, burst},
    util::{damp, hdr},
    world::{Obstacles, PLAYER_SPAWN, WORLD_RADIUS, surface_height, terrain_height},
};

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PlayerHit>()
            .add_systems(Startup, spawn_player)
            .add_systems(
                Update,
                (
                    autopilot.run_if(|| std::env::var("EMBER_AUTOPILOT").is_ok()),
                    movement.run_if(not(in_state(GameState::Title))),
                    on_hit,
                    animate_body,
                    trail,
                    reset_on_restart,
                )
                    .chain(),
            );
    }
}

const HOVER: f32 = 1.1;
const RUN_SPEED: f32 = 10.5;
const DASH_SPEED: f32 = 34.0;
const JUMP_VEL: f32 = 13.0;
const GRAVITY: f32 = 34.0;

/// Dev aid: steers toward embers, then beacons, so the loop can be tested unattended.
#[derive(Resource, Default)]
struct Autopilot {
    dir: Vec3,
    jump: bool,
}

fn autopilot(
    mut commands: Commands,
    carried: Res<crate::ember::Carried>,
    player: Single<(&Transform, &Player)>,
    embers: Query<(&crate::ember::Ember, &Transform), Without<Player>>,
    beacons: Query<&crate::beacon::Beacon>,
    time: Res<Time>,
    mut detour: Local<f32>,
) {
    let (tf, p) = player.into_inner();
    let me = tf.translation;
    let target = if carried.0 >= crate::EMBERS_PER_BEACON {
        beacons.iter().filter(|b| !b.lit).map(|b| b.base).min_by(|a, b| a.distance(me).total_cmp(&b.distance(me)))
    } else {
        embers
            .iter()
            .filter(|(e, _)| matches!(e, crate::ember::Ember::Free { .. }))
            .map(|(_, t)| t.translation)
            .min_by(|a, b| a.distance(me).total_cmp(&b.distance(me)))
    };
    let mut dir = target.map_or(Vec3::ZERO, |t| (t - me).with_y(0.0).normalize_or_zero());
    let stalled = p.grounded && p.vel.with_y(0.0).length() < RUN_SPEED * 0.4;
    if stalled {
        *detour = 1.2;
    }
    if *detour > 0.0 {
        *detour -= time.delta_secs();
        dir = Quat::from_rotation_y(1.2) * dir;
    }
    commands.insert_resource(Autopilot { dir, jump: stalled });
}

#[derive(Message)]
pub struct PlayerHit {
    pub from: Vec3,
}

#[derive(Component)]
pub struct Player {
    pub vel: Vec3,
    pub grounded: bool,
    pub invuln: f32,
    pub dashing: f32,
    dash_cd: f32,
    air_jumps: u8,
    squash: f32,
    squash_vel: f32,
    facing: f32,
    trail_acc: f32,
    blink: f32,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            vel: Vec3::ZERO,
            grounded: true,
            invuln: 0.0,
            dashing: 0.0,
            dash_cd: 0.0,
            air_jumps: 1,
            squash: 0.0,
            squash_vel: 0.0,
            facing: 0.0,
            trail_acc: 0.0,
            blink: 3.0,
        }
    }
}

impl Player {
    fn kick(&mut self, amount: f32) {
        self.squash_vel += amount;
    }
}

#[derive(Component)]
struct Body;

#[derive(Component)]
struct Shell;

#[derive(Component)]
struct Eye;

#[derive(Component)]
struct PlayerLight;

/// Walks inland from the nominal spawn until there is solid ground.
pub fn spawn_point() -> Vec3 {
    let mut p = PLAYER_SPAWN;
    while terrain_height(p.x, p.y) < 1.5 && p.length() > 1.0 {
        p *= 0.97;
    }
    Vec3::new(p.x, terrain_height(p.x, p.y) + HOVER, p.y)
}

fn spawn_player(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let sphere = meshes.add(Sphere::new(1.0).mesh().ico(3).unwrap());
    let core = materials.add(StandardMaterial {
        base_color: hdr(5.0, 3.4, 1.4),
        unlit: true,
        ..default()
    });
    let shell = materials.add(StandardMaterial {
        base_color: hdr(0.04, 0.022, 0.006),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        ..default()
    });
    let eye = materials.add(StandardMaterial {
        base_color: Color::srgb(0.06, 0.03, 0.08),
        unlit: true,
        ..default()
    });

    commands
        .spawn((
            Name::new("Player"),
            Player::default(),
            Transform::from_translation(spawn_point()),
            Visibility::default(),
        ))
        .with_children(|p| {
            p.spawn((
                PlayerLight,
                PointLight {
                    color: Color::srgb(1.0, 0.75, 0.45),
                    intensity: 250_000.0,
                    range: 22.0,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.6, 0.0),
            ));
            p.spawn((Body, Transform::default(), Visibility::default())).with_children(|b| {
                b.spawn((
                    Mesh3d(sphere.clone()),
                    MeshMaterial3d(core),
                    Transform::from_scale(Vec3::splat(0.45)),
                    NotShadowCaster,
                ));
                b.spawn((Shell, Transform::default(), Visibility::default())).with_children(|s| {
                    for k in [0.62, 0.8, 1.05] {
                        s.spawn((
                            Mesh3d(sphere.clone()),
                            MeshMaterial3d(shell.clone()),
                            Transform::from_scale(Vec3::splat(k)),
                            NotShadowCaster,
                            NotShadowReceiver,
                        ));
                    }
                });
                for x in [-0.16, 0.16] {
                    b.spawn((
                        Eye,
                        Mesh3d(sphere.clone()),
                        MeshMaterial3d(eye.clone()),
                        Transform::from_xyz(x, 0.08, -0.41).with_scale(Vec3::new(0.065, 0.1, 0.05)),
                        NotShadowCaster,
                    ));
                }
            });
        });
}

fn movement(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    rig: Res<CameraRig>,
    obstacles: Res<Obstacles>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut trauma: ResMut<Trauma>,
    mut sfx: MessageWriter<Sfx>,
    player: Single<(&mut Transform, &mut Player)>,
    auto: Option<Res<Autopilot>>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    let (mut tf, mut p) = player.into_inner();

    let mut input = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        input.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        input.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        input.x += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        input.x -= 1.0;
    }
    let fwd = rig.forward();
    let right = Vec3::new(-fwd.z, 0.0, fwd.x);
    let mut wish = (fwd * input.y + right * input.x).normalize_or_zero();
    let mut auto_jump = false;
    let ghost = auto.is_some();
    if let Some(a) = auto {
        wish = a.dir;
        auto_jump = a.jump;
    }

    p.invuln = (p.invuln - dt).max(0.0);
    p.dash_cd = (p.dash_cd - dt).max(0.0);

    let dash_pressed = keys.just_pressed(KeyCode::ShiftLeft) || keys.just_pressed(KeyCode::ShiftRight);
    if dash_pressed && p.dash_cd <= 0.0 {
        let dir = if wish != Vec3::ZERO {
            wish
        } else {
            Vec3::new(-p.facing.sin(), 0.0, -p.facing.cos())
        };
        p.vel = dir * DASH_SPEED + Vec3::Y * p.vel.y.max(1.0);
        p.dashing = 0.2;
        p.dash_cd = 0.75;
        p.kick(-3.0);
        trauma.add(0.12);
        sfx.write(Sfx::new(SfxKind::Dash));
        burst(
            &mut commands,
            &fx,
            &mut rng.0,
            Burst { pos: tf.translation, count: 18, palette: Palette::Cyan, speed: (3.0, 7.0), ..default() },
        );
    }

    if keys.just_pressed(KeyCode::Space) || auto_jump {
        if p.grounded {
            p.vel.y = JUMP_VEL;
            p.grounded = false;
            p.kick(4.0);
            sfx.write(Sfx::new(SfxKind::Jump));
        } else if p.air_jumps > 0 {
            p.air_jumps -= 1;
            p.vel.y = JUMP_VEL * 0.85;
            p.kick(4.0);
            sfx.write(Sfx::pitched(SfxKind::Jump, 1.35));
            burst(
                &mut commands,
                &fx,
                &mut rng.0,
                Burst {
                    pos: tf.translation - Vec3::Y * 0.4,
                    count: 14,
                    palette: Palette::White,
                    speed: (2.0, 4.0),
                    ..default()
                },
            );
        }
    }

    // Fixed substeps keep jumps and landings stable at low frame rates.
    let steps = (time.delta_secs() / (1.0 / 60.0)).ceil().clamp(1.0, 8.0);
    let h = time.delta_secs().min(0.25) / steps;
    let mut pos = tf.translation;
    for _ in 0..steps as u32 {
        let mut horiz = Vec3::new(p.vel.x, 0.0, p.vel.z);
        if p.dashing > 0.0 {
            p.dashing -= h;
            p.vel.y = p.vel.y.max(0.0);
        } else {
            let rate = if p.grounded { 11.0 } else { 4.0 };
            horiz = horiz.lerp(wish * RUN_SPEED, damp(rate, h));
        }
        if !p.grounded {
            p.vel.y -= GRAVITY * h;
            if keys.pressed(KeyCode::Space) && p.vel.y < 0.0 {
                p.vel.y = p.vel.y.max(-3.0);
            }
        }
        p.vel.x = horiz.x;
        p.vel.z = horiz.z;
        pos += p.vel * h;

        if !ghost && let Some(n) = obstacles.push_out(&mut pos, 0.5) {
            let into = p.vel.xz().dot(n).min(0.0);
            p.vel.x -= n.x * into;
            p.vel.z -= n.y * into;
        }
        let r = pos.xz().length();
        if r > WORLD_RADIUS {
            let back = pos.xz() / r * WORLD_RADIUS;
            pos.x = back.x;
            pos.z = back.y;
        }

        let ground = surface_height(pos.x, pos.z, t) + HOVER;
        if pos.y <= ground + 0.3 && p.vel.y <= 0.0 {
            if !p.grounded && p.vel.y < -7.0 {
                let impact = p.vel.y * 0.35;
                p.kick(impact);
                burst(
                    &mut commands,
                    &fx,
                    &mut rng.0,
                    Burst {
                        pos: Vec3::new(pos.x, ground - HOVER + 0.1, pos.z),
                        count: 14,
                        palette: Palette::Dust,
                        speed: (2.0, 5.0),
                        size: (0.06, 0.12),
                        life: (0.4, 0.7),
                        gravity: 6.0,
                        up_bias: 0.6,
                        ..default()
                    },
                );
                sfx.write(Sfx::pitched(SfxKind::Land, 1.0));
            }
            p.grounded = true;
            p.air_jumps = 1;
            p.vel.y = 0.0;
            pos.y = pos.y.lerp(ground, damp(18.0, h)).max(ground - 0.4);
        } else if pos.y > ground + 0.3 {
            p.grounded = false;
        }
    }
    tf.translation = pos;
}

fn on_hit(
    mut commands: Commands,
    mut hits: MessageReader<PlayerHit>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut trauma: ResMut<Trauma>,
    mut sfx: MessageWriter<Sfx>,
    mut progress: ResMut<crate::Progress>,
    player: Single<(&Transform, &mut Player)>,
) {
    let (tf, mut p) = player.into_inner();
    for hit in hits.read() {
        if p.invuln > 0.0 {
            continue;
        }
        let away = (tf.translation - hit.from).with_y(0.0).normalize_or(Vec3::X);
        p.vel = away * 16.0 + Vec3::Y * 11.0;
        p.grounded = false;
        p.invuln = 2.0;
        p.kick(-5.0);
        progress.times_caught += 1;
        trauma.add(0.55);
        sfx.write(Sfx::new(SfxKind::Hit));
        burst(
            &mut commands,
            &fx,
            &mut rng.0,
            Burst { pos: tf.translation, count: 30, palette: Palette::Violet, speed: (3.0, 9.0), ..default() },
        );
    }
}

fn animate_body(
    time: Res<Time>,
    state: Res<State<GameState>>,
    mut player: Single<&mut Player>,
    mut body: Single<(&mut Transform, &mut Visibility), (With<Body>, Without<Shell>, Without<Eye>)>,
    mut shell: Single<&mut Transform, (With<Shell>, Without<Body>, Without<Eye>)>,
    mut eyes: Query<&mut Transform, (With<Eye>, Without<Body>, Without<Shell>)>,
    mut light: Single<&mut PointLight, With<PlayerLight>>,
) {
    let dt = time.delta_secs().min(0.25);
    let t = time.elapsed_secs();
    let p = &mut **player;

    let steps = (dt * 120.0).ceil().max(1.0);
    for _ in 0..steps as u32 {
        let h = dt / steps;
        let accel = -p.squash * 240.0 - p.squash_vel * 13.0;
        p.squash_vel += accel * h;
        p.squash += p.squash_vel * h;
    }

    let horiz = Vec2::new(p.vel.x, p.vel.z);
    let speed = horiz.length();
    if speed > 0.6 {
        let target = (-p.vel.x).atan2(-p.vel.z);
        let mut diff = (target - p.facing + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        if diff.abs() < 1e-4 {
            diff = 0.0;
        }
        p.facing += diff * damp(12.0, dt);
    } else if *state.get() == GameState::Title {
        p.facing = (t * 0.4).sin() * 0.6;
    }

    let stretch = (p.squash + (p.vel.y * 0.012).clamp(-0.15, 0.2)).clamp(-0.45, 0.6);
    let s = 1.0 + stretch;
    let lean = -(speed / RUN_SPEED).min(1.6) * 0.22;
    let bob = (t * 3.2).sin() * 0.12 * if p.grounded { 1.0 } else { 0.3 };

    let (btf, vis) = &mut *body;
    btf.translation = Vec3::Y * bob;
    btf.rotation = Quat::from_rotation_y(p.facing) * Quat::from_rotation_x(lean);
    btf.scale = Vec3::new(1.0 / s.sqrt(), s, 1.0 / s.sqrt());
    **vis = if p.invuln > 0.0 && (t * 18.0).sin() > 0.0 { Visibility::Hidden } else { Visibility::Inherited };

    shell.scale = Vec3::splat(1.0 + 0.07 * (t * 4.0).sin() + if p.dashing > 0.0 { 0.25 } else { 0.0 });

    p.blink -= dt;
    if p.blink < -0.12 {
        p.blink = 2.0 + (t * 7.3).sin().abs() * 3.0;
    }
    let lid = if p.blink < 0.0 { 0.12 } else { 1.0 };
    for mut e in &mut eyes {
        e.scale.y = e.scale.y.lerp(0.1 * lid, damp(40.0, dt));
    }

    light.intensity = 250_000.0 * (1.0 + 0.08 * (t * 9.0).sin() + 0.05 * (t * 23.0).sin());
}

fn trail(
    mut commands: Commands,
    time: Res<Time>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    player: Single<(&Transform, &mut Player)>,
) {
    let (tf, mut p) = player.into_inner();
    let speed = p.vel.length();
    let rate = if p.dashing > 0.0 { 140.0 } else { 14.0 + speed * 3.5 };
    p.trail_acc += time.delta_secs() * rate;
    let palette = if p.dashing > 0.0 { Palette::Cyan } else { Palette::Gold };
    while p.trail_acc >= 1.0 {
        p.trail_acc -= 1.0;
        let jitter = rng.0.unit_vec3() * 0.25;
        let life = rng.0.range(0.45, 0.9);
        let size = rng.0.range(0.06, 0.14);
        let vel = -p.vel * 0.08 + rng.0.unit_vec3() * 0.4 + Vec3::Y * 0.5;
        commands.spawn((
            Mesh3d(fx.sphere.clone()),
            MeshMaterial3d(fx.mat(palette)),
            Transform::from_translation(tf.translation + jitter).with_scale(Vec3::splat(size)),
            crate::fx::Particle { vel, life, max_life: life, size, gravity: -0.6, drag: 1.5, curve: SizeCurve::Shrink },
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
}

fn reset_on_restart(mut restarts: MessageReader<RestartLevel>, player: Single<(&mut Transform, &mut Player)>) {
    if restarts.read().count() > 0 {
        let (mut tf, mut p) = player.into_inner();
        tf.translation = spawn_point();
        *p = Player::default();
    }
}
