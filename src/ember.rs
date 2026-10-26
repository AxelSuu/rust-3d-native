use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
};

use crate::{
    GameState, LevelEntity, Progress, RestartLevel,
    audio::{Sfx, SfxKind},
    fx::{Burst, FxAssets, FxRng, Palette, SizeCurve, burst},
    player::{Player, PlayerHit},
    util::{Rng, damp, hdr},
    world::{BEACON_SITES, ISLANDS, flat_mesh, random_land_point, surface_height, water_height},
};

pub struct EmberPlugin;

impl Plugin for EmberPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Carried>()
            .add_message::<EmberDelivered>()
            .add_systems(Startup, (setup_assets, spawn_embers).chain())
            .add_systems(
                Update,
                (
                    respawn_on_restart,
                    scatter_on_hit,
                    free_embers.run_if(not(in_state(GameState::Title))),
                    carried_embers,
                    offering_embers,
                    scattered_embers,
                    count_carried,
                )
                    .chain(),
            );
    }
}

const EMBER_COUNT: usize = 34;
const HOVER: f32 = 1.3;

#[derive(Component)]
pub enum Ember {
    Free { home: Vec3, phase: f32 },
    Carried { slot: usize },
    Offering { from: Vec3, to: Vec3, t: f32, beacon: Entity },
    Scattered { vel: Vec3, cooldown: f32 },
}

#[derive(Message)]
pub struct EmberDelivered {
    pub beacon: Entity,
}

#[derive(Resource, Default)]
pub struct Carried(pub u32);

#[derive(Resource)]
struct EmberAssets {
    mesh: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
    halo: Handle<StandardMaterial>,
}

fn octahedron() -> Mesh {
    let t = Vec3::Y;
    let b = -Vec3::Y;
    let ring = [Vec3::X, Vec3::Z, -Vec3::X, -Vec3::Z];
    let mut pos = Vec::new();
    for i in 0..4 {
        let a = ring[i];
        let c = ring[(i + 1) % 4];
        pos.extend([t, c, a, b, a, c].map(|v| v.to_array()));
    }
    let n = pos.len();
    flat_mesh(pos, vec![[1.0; 4]; n])
}

fn setup_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(EmberAssets {
        mesh: meshes.add(octahedron()),
        mat: materials.add(StandardMaterial {
            base_color: hdr(9.0, 4.2, 1.0),
            unlit: true,
            ..default()
        }),
        halo: materials.add(StandardMaterial {
            base_color: hdr(0.025, 0.01, 0.002),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            ..default()
        }),
    });
}

fn spawn_ember(commands: &mut Commands, assets: &EmberAssets, fx: &FxAssets, home: Vec3, phase: f32, light: bool) {
    commands
        .spawn((
            LevelEntity,
            Ember::Free { home, phase },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.mat.clone()),
            Transform::from_translation(home + Vec3::Y * HOVER).with_scale(Vec3::new(0.32, 0.5, 0.32)),
            NotShadowCaster,
            NotShadowReceiver,
        ))
        .with_children(|e| {
            for s in [1.6, 2.2, 3.0] {
                e.spawn((
                    Mesh3d(fx.sphere.clone()),
                    MeshMaterial3d(assets.halo.clone()),
                    Transform::from_scale(Vec3::new(s, s * 0.66, s)),
                    NotShadowCaster,
                    NotShadowReceiver,
                ));
            }
            if !light {
                return;
            }
            e.spawn((
                PointLight {
                    color: Color::srgb(1.0, 0.6, 0.25),
                    intensity: 30_000.0,
                    range: 7.0,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::default(),
            ));
        });
}

fn ember_homes() -> Vec<Vec3> {
    let mut rng = Rng::new(2024);
    let mut homes: Vec<Vec3> = Vec::new();
    // A few float over the sea between islands, breadcrumbs pointing the way.
    for isl in &ISLANDS[1..5] {
        for k in [0.42, 0.58] {
            let p = ISLANDS[0].center.lerp(isl.center, k);
            homes.push(Vec3::new(p.x, water_height(p.x, p.y, 0.0).max(0.0) + 0.6, p.y));
        }
    }
    while homes.len() < EMBER_COUNT {
        let Some(p) = random_land_point(&mut rng, 0.8, 14.0, 0.6) else { continue };
        let crowded = homes.iter().any(|h| h.xz().distance(p.xz()) < 14.0);
        let near_beacon = BEACON_SITES.iter().any(|s| s.distance(p.xz()) < 16.0);
        if !crowded && !near_beacon {
            homes.push(p);
        }
    }
    homes
}

fn spawn_embers(mut commands: Commands, assets: Res<EmberAssets>, fx: Res<FxAssets>, quality: Res<crate::Quality>) {
    for (i, home) in ember_homes().into_iter().enumerate() {
        spawn_ember(&mut commands, &assets, &fx, home, i as f32 * 1.7, !quality.low);
    }
}

fn respawn_on_restart(
    mut commands: Commands,
    mut restarts: MessageReader<RestartLevel>,
    embers: Query<Entity, With<Ember>>,
    assets: Res<EmberAssets>,
    fx: Res<FxAssets>,
    quality: Res<crate::Quality>,
) {
    if restarts.read().count() == 0 {
        return;
    }
    for e in &embers {
        commands.entity(e).despawn();
    }
    for (i, home) in ember_homes().into_iter().enumerate() {
        spawn_ember(&mut commands, &assets, &fx, home, i as f32 * 1.7, !quality.low);
    }
}

fn free_embers(
    mut commands: Commands,
    time: Res<Time>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut progress: ResMut<Progress>,
    carried: Res<Carried>,
    mut sfx: MessageWriter<Sfx>,
    player: Single<&Transform, With<Player>>,
    mut embers: Query<(&mut Ember, &mut Transform), Without<Player>>,
) {
    let t = time.elapsed_secs();
    let mut picked = carried.0 as usize;
    for (mut ember, mut tf) in &mut embers {
        let Ember::Free { home, phase } = *ember else { continue };
        tf.translation = home + Vec3::Y * (HOVER + (t * 2.0 + phase).sin() * 0.25);
        tf.rotation = Quat::from_rotation_y(t * 1.6 + phase);
        if tf.translation.distance(player.translation) < 2.0 {
            *ember = Ember::Carried { slot: picked };
            picked += 1;
            progress.embers_gathered += 1;
            // Climbing pentatonic run as the collection grows.
            const STEPS: [f32; 5] = [1.0, 1.125, 1.26, 1.5, 1.68];
            let pitch = STEPS[(picked - 1) % 5] * if picked > 5 { 2.0 } else { 1.0 };
            sfx.write(Sfx::pitched(SfxKind::Pickup, pitch));
            burst(
                &mut commands,
                &fx,
                &mut rng.0,
                Burst {
                    pos: tf.translation,
                    count: 22,
                    palette: Palette::Ember,
                    speed: (2.0, 7.0),
                    size: (0.05, 0.13),
                    curve: SizeCurve::Pop,
                    ..default()
                },
            );
        }
    }
}

fn carried_embers(
    time: Res<Time>,
    player: Single<&Transform, With<Player>>,
    mut embers: Query<(&mut Ember, &mut Transform), Without<Player>>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    let mut slots: Vec<_> = embers
        .iter_mut()
        .filter(|(e, _)| matches!(**e, Ember::Carried { .. }))
        .collect();
    slots.sort_by_key(|(e, _)| match **e {
        Ember::Carried { slot } => slot,
        _ => 0,
    });
    let n = slots.len();
    for (i, (mut ember, mut tf)) in slots.into_iter().enumerate() {
        *ember = Ember::Carried { slot: i };
        let a = t * 2.2 + i as f32 * std::f32::consts::TAU / n as f32;
        let r = 1.25 + n as f32 * 0.06;
        let target = player.translation
            + Vec3::new(a.cos() * r, 0.35 + (t * 3.0 + i as f32).sin() * 0.18, a.sin() * r);
        tf.translation = tf.translation.lerp(target, damp(12.0, dt));
        tf.rotation = Quat::from_rotation_y(t * 4.0 + i as f32);
        tf.scale = tf.scale.lerp(Vec3::new(0.2, 0.32, 0.2), damp(8.0, dt));
    }
}

fn offering_embers(
    mut commands: Commands,
    time: Res<Time>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut delivered: MessageWriter<EmberDelivered>,
    mut embers: Query<(Entity, &mut Ember, &mut Transform)>,
) {
    for (e, mut ember, mut tf) in &mut embers {
        let Ember::Offering { from, to, t, beacon } = &mut *ember else { continue };
        *t += time.delta_secs() * 1.5;
        let k = t.min(1.0);
        let ctrl = from.lerp(*to, 0.5) + Vec3::Y * 5.0;
        let a = from.lerp(ctrl, k);
        let b = ctrl.lerp(*to, k);
        tf.translation = a.lerp(b, k);
        tf.rotation = Quat::from_rotation_y(k * 12.0);
        tf.scale = Vec3::new(0.3, 0.45, 0.3) * (1.0 - k * 0.5);
        if *t >= 1.0 {
            delivered.write(EmberDelivered { beacon: *beacon });
            burst(
                &mut commands,
                &fx,
                &mut rng.0,
                Burst { pos: *to, count: 14, palette: Palette::Flame, speed: (1.5, 4.0), up_bias: 0.8, ..default() },
            );
            commands.entity(e).despawn();
        }
    }
}

fn scatter_on_hit(
    mut hits: MessageReader<PlayerHit>,
    mut rng: ResMut<FxRng>,
    mut embers: Query<&mut Ember>,
) {
    if hits.read().count() == 0 {
        return;
    }
    for mut ember in &mut embers {
        if matches!(*ember, Ember::Carried { .. }) {
            let dir = Vec3::new(rng.0.signed(), 0.0, rng.0.signed()).normalize_or(Vec3::X);
            *ember = Ember::Scattered {
                vel: dir * rng.0.range(5.0, 10.0) + Vec3::Y * rng.0.range(8.0, 12.0),
                cooldown: 1.0,
            };
        }
    }
}

fn scattered_embers(time: Res<Time>, mut embers: Query<(&mut Ember, &mut Transform)>) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (mut ember, mut tf) in &mut embers {
        let Ember::Scattered { vel, cooldown } = &mut *ember else { continue };
        vel.y -= 22.0 * dt;
        tf.translation += *vel * dt;
        tf.rotation *= Quat::from_rotation_y(dt * 10.0);
        tf.scale = tf.scale.lerp(Vec3::new(0.32, 0.5, 0.32), damp(6.0, dt));
        let ground = surface_height(tf.translation.x, tf.translation.z, t).max(0.3);
        if tf.translation.y < ground + HOVER {
            tf.translation.y = ground + HOVER;
            vel.x *= 0.5;
            vel.z *= 0.5;
            vel.y = vel.y.abs() * 0.3;
        }
        *cooldown -= dt;
        if *cooldown <= 0.0 && vel.length() < 2.0 {
            let home = tf.translation - Vec3::Y * HOVER;
            *ember = Ember::Free { home, phase: t };
        }
    }
}

fn count_carried(embers: Query<&Ember>, mut carried: ResMut<Carried>) {
    carried.0 = embers.iter().filter(|e| matches!(e, Ember::Carried { .. })).count() as u32;
}
