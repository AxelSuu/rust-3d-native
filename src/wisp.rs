use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
};

use crate::{
    GameState, LevelEntity, RestartLevel,
    audio::{Sfx, SfxKind},
    beacon::{BeaconLit, Beacon},
    fx::{Burst, FxAssets, FxRng, Palette, SizeCurve, burst},
    player::{Player, PlayerHit, spawn_point},
    util::{Rng, damp, hdr},
    world::{Obstacles, random_land_point, surface_height},
};

pub struct WispPlugin;

impl Plugin for WispPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (setup_assets, spawn_wisps).chain())
            .add_systems(OnEnter(GameState::Victory), dissolve_all)
            .add_systems(
                Update,
                (respawn_on_restart, dispel, think.run_if(in_state(GameState::Playing)), animate, dissolve).chain(),
            );
    }
}

const WISP_COUNT: usize = 10;
const SIGHT: f32 = 15.0;
const CHASE_SPEED: f32 = 7.8;
const SAFE_RADIUS: f32 = 24.0;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Wander,
    Chase,
    Retreat,
    Dissolve,
}

#[derive(Component)]
pub struct Wisp {
    home: Vec3,
    target: Vec3,
    vel: Vec3,
    mode: Mode,
    timer: f32,
    phase: f32,
    trail_acc: f32,
}

#[derive(Component)]
struct Shards;

#[derive(Resource)]
struct WispAssets {
    core: Handle<Mesh>,
    core_mat: Handle<StandardMaterial>,
    shard: Handle<Mesh>,
    eye_mat: Handle<StandardMaterial>,
    aura_mat: Handle<StandardMaterial>,
}

fn setup_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    fx: Res<FxAssets>,
) {
    commands.insert_resource(WispAssets {
        core: meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap()),
        core_mat: fx.mat(Palette::Shadow),
        shard: meshes.add(Tetrahedron::default()),
        eye_mat: materials.add(StandardMaterial {
            base_color: hdr(12.0, 1.0, 8.0),
            unlit: true,
            ..default()
        }),
        aura_mat: materials.add(StandardMaterial {
            base_color: hdr(0.03, 0.0, 0.07),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            ..default()
        }),
    });
}

fn spawn_wisp(commands: &mut Commands, a: &WispAssets, pos: Vec3, phase: f32) {
    commands
        .spawn((
            LevelEntity,
            Wisp { home: pos, target: pos, vel: Vec3::ZERO, mode: Mode::Wander, timer: 0.0, phase, trail_acc: 0.0 },
            Transform::from_translation(pos + Vec3::Y * 1.8),
            Visibility::default(),
        ))
        .with_children(|w| {
            w.spawn((
                Mesh3d(a.core.clone()),
                MeshMaterial3d(a.core_mat.clone()),
                Transform::from_scale(Vec3::splat(0.6)),
            ));
            for s in [0.85, 1.15, 1.5] {
                w.spawn((
                    Mesh3d(a.core.clone()),
                    MeshMaterial3d(a.aura_mat.clone()),
                    Transform::from_scale(Vec3::splat(s)),
                    NotShadowCaster,
                    NotShadowReceiver,
                ));
            }
            for x in [-0.2, 0.2] {
                w.spawn((
                    Mesh3d(a.core.clone()),
                    MeshMaterial3d(a.eye_mat.clone()),
                    Transform::from_xyz(x, 0.12, -0.52)
                        .with_scale(Vec3::new(0.09, 0.05, 0.04))
                        .with_rotation(Quat::from_rotation_z(if x < 0.0 { -0.35 } else { 0.35 })),
                    NotShadowCaster,
                ));
            }
            w.spawn((Shards, Transform::default(), Visibility::default())).with_children(|s| {
                for k in 0..6 {
                    let a2 = k as f32 * std::f32::consts::TAU / 6.0;
                    s.spawn((
                        Mesh3d(a.shard.clone()),
                        MeshMaterial3d(a.core_mat.clone()),
                        Transform::from_xyz(a2.cos() * 1.0, (k as f32 * 1.3).sin() * 0.3, a2.sin() * 1.0)
                            .with_scale(Vec3::splat(0.22))
                            .with_rotation(Quat::from_rotation_x(a2)),
                    ));
                }
            });
        });
}

fn wisp_homes() -> Vec<Vec3> {
    let mut rng = Rng::new(666);
    let spawn = spawn_point();
    let mut homes: Vec<Vec3> = Vec::new();
    while homes.len() < WISP_COUNT {
        let Some(p) = random_land_point(&mut rng, 1.0, 12.0, 0.5) else { continue };
        if p.distance(spawn) < 35.0 || homes.iter().any(|h| h.distance(p) < 30.0) {
            continue;
        }
        homes.push(p);
    }
    homes
}

fn spawn_wisps(mut commands: Commands, assets: Res<WispAssets>) {
    for (i, h) in wisp_homes().into_iter().enumerate() {
        spawn_wisp(&mut commands, &assets, h, i as f32 * 2.3);
    }
}

fn respawn_on_restart(
    mut commands: Commands,
    mut restarts: MessageReader<RestartLevel>,
    wisps: Query<Entity, With<Wisp>>,
    assets: Res<WispAssets>,
) {
    if restarts.read().count() == 0 {
        return;
    }
    for e in &wisps {
        commands.entity(e).despawn();
    }
    for (i, h) in wisp_homes().into_iter().enumerate() {
        spawn_wisp(&mut commands, &assets, h, i as f32 * 2.3);
    }
}

fn think(
    time: Res<Time>,
    obstacles: Res<Obstacles>,
    mut hits: MessageWriter<PlayerHit>,
    mut sfx: MessageWriter<Sfx>,
    player: Single<(&Transform, &Player)>,
    beacons: Query<&Beacon>,
    mut wisps: Query<(&mut Wisp, &mut Transform), Without<Player>>,
) {
    let dt = time.delta_secs().min(0.25);
    let t = time.elapsed_secs();
    let (ptf, p) = player.into_inner();
    let ppos = ptf.translation;
    let player_safe = beacons.iter().any(|b| b.lit && b.base.xz().distance(ppos.xz()) < SAFE_RADIUS);
    let mut hit_sent = false;

    for (mut w, mut tf) in &mut wisps {
        if w.mode == Mode::Dissolve {
            continue;
        }
        w.timer -= dt;
        let pos = tf.translation;
        let to_player = ppos - pos;
        let dist = to_player.length();

        match w.mode {
            Mode::Wander => {
                if w.timer <= 0.0 || pos.xz().distance(w.target.xz()) < 1.5 {
                    let a = t * 1.7 + w.phase * 3.1;
                    let r = 6.0 + (w.phase * 7.7 + t).sin().abs() * 10.0;
                    w.target = w.home + Vec3::new(a.cos() * r, 0.0, a.sin() * r);
                    w.timer = 3.5;
                }
                if dist < SIGHT && p.invuln <= 0.0 && !player_safe {
                    w.mode = Mode::Chase;
                    sfx.write(Sfx::pitched(SfxKind::Growl, 0.9 + (w.phase % 0.3)));
                }
            }
            Mode::Chase => {
                w.target = ppos;
                if dist > SIGHT * 1.8 || pos.distance(w.home) > 50.0 || player_safe || p.invuln > 0.0 {
                    w.mode = Mode::Retreat;
                    w.timer = 2.5;
                }
                if dist < 1.4 && p.invuln <= 0.0 && !hit_sent {
                    hits.write(PlayerHit { from: pos });
                    hit_sent = true;
                    w.mode = Mode::Retreat;
                    w.timer = 4.0;
                }
            }
            Mode::Retreat => {
                w.target = w.home;
                if w.timer <= 0.0 {
                    w.mode = Mode::Wander;
                }
            }
            Mode::Dissolve => {}
        }

        let speed = match w.mode {
            Mode::Chase => CHASE_SPEED,
            Mode::Retreat => 6.0,
            _ => 2.8,
        };
        let mut desired = (w.target - pos).with_y(0.0).normalize_or_zero() * speed;
        // Lit beacons repel the dark.
        for b in &beacons {
            if !b.lit {
                continue;
            }
            let away = (pos - b.base).with_y(0.0);
            let d = away.length();
            if d < SAFE_RADIUS + 4.0 {
                desired += away.normalize_or_zero() * 14.0 * (1.0 - d / (SAFE_RADIUS + 4.0));
            }
        }
        let rate = if w.mode == Mode::Chase { 3.0 } else { 2.0 };
        w.vel = w.vel.lerp(desired, damp(rate, dt));
        let mut next = pos + w.vel * dt;
        let _ = obstacles.push_out(&mut next, 0.7);
        let ground = surface_height(next.x, next.z, t) + 1.7 + (t * 2.0 + w.phase).sin() * 0.3;
        next.y = next.y.lerp(ground, damp(5.0, dt));
        tf.translation = next;

        let look = if w.mode == Mode::Chase { to_player } else { w.vel };
        if look.with_y(0.0).length_squared() > 0.01 {
            let yaw = (-look.x).atan2(-look.z);
            tf.rotation = tf.rotation.slerp(Quat::from_rotation_y(yaw), damp(6.0, dt));
        }
    }
}

fn animate(
    mut commands: Commands,
    time: Res<Time>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut wisps: Query<(&mut Wisp, &Transform, &Children)>,
    mut shards: Query<&mut Transform, (With<Shards>, Without<Wisp>)>,
) {
    let t = time.elapsed_secs();
    for (mut w, tf, children) in &mut wisps {
        for c in children {
            if let Ok(mut s) = shards.get_mut(*c) {
                let spin = if w.mode == Mode::Chase { 5.0 } else { 1.5 };
                s.rotation = Quat::from_rotation_y(t * spin + w.phase) * Quat::from_rotation_x((t + w.phase).sin() * 0.4);
                s.scale = Vec3::splat(1.0 + 0.1 * (t * 6.0 + w.phase).sin());
            }
        }
        w.trail_acc += time.delta_secs() * if w.mode == Mode::Chase { 30.0 } else { 10.0 };
        while w.trail_acc >= 1.0 {
            w.trail_acc -= 1.0;
            let life = rng.0.range(0.6, 1.2);
            let size = rng.0.range(0.1, 0.22);
            commands.spawn((
                Mesh3d(fx.sphere.clone()),
                MeshMaterial3d(fx.mat(if rng.0.f32() < 0.5 { Palette::Shadow } else { Palette::Violet })),
                Transform::from_translation(tf.translation + rng.0.unit_vec3() * 0.5).with_scale(Vec3::splat(size)),
                crate::fx::Particle {
                    vel: rng.0.unit_vec3() * 0.3 + Vec3::Y * 0.6,
                    life,
                    max_life: life,
                    size,
                    gravity: -0.5,
                    drag: 1.0,
                    curve: SizeCurve::Shrink,
                },
                NotShadowCaster,
                NotShadowReceiver,
            ));
        }
    }
}

fn dispel(mut lit: MessageReader<BeaconLit>, mut wisps: Query<(&mut Wisp, &Transform)>) {
    for ev in lit.read() {
        for (mut w, tf) in &mut wisps {
            if tf.translation.distance(ev.pos) < 40.0 {
                w.mode = Mode::Dissolve;
                w.timer = 0.0;
            }
        }
    }
}

fn dissolve_all(mut wisps: Query<&mut Wisp>) {
    for mut w in &mut wisps {
        w.mode = Mode::Dissolve;
        w.timer = 0.0;
    }
}

fn dissolve(
    mut commands: Commands,
    time: Res<Time>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut wisps: Query<(Entity, &mut Wisp, &mut Transform)>,
) {
    for (e, mut w, mut tf) in &mut wisps {
        if w.mode != Mode::Dissolve {
            continue;
        }
        w.timer += time.delta_secs();
        let k = w.timer / 0.8;
        tf.scale = Vec3::splat((1.0 - k).max(0.0)) * (1.0 + k * 0.5);
        tf.translation.y += time.delta_secs() * 2.0;
        if k >= 1.0 {
            burst(
                &mut commands,
                &fx,
                &mut rng.0,
                Burst {
                    pos: tf.translation,
                    count: 40,
                    palette: Palette::Violet,
                    speed: (2.0, 8.0),
                    size: (0.08, 0.2),
                    life: (0.6, 1.2),
                    gravity: -1.0,
                    ..default()
                },
            );
            commands.entity(e).despawn();
        }
    }
}
