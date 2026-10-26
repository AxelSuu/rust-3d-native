use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
};

use crate::{
    BEACON_COUNT, EMBERS_PER_BEACON, GameState, LevelEntity, Progress, RestartLevel,
    audio::{Sfx, SfxKind},
    ember::{Ember, EmberDelivered},
    fx::{Burst, FxAssets, FxRng, Palette, SizeCurve, Trauma, burst, spawn_ring},
    player::Player,
    util::{Rng, damp, ease_out_back, hdr, lerp_color, smoothstep},
    world::{BEACON_SITES, combine, ico, jitter, part, site_height, srgb},
};

pub struct BeaconPlugin;

impl Plugin for BeaconPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<BeaconLit>()
            .add_systems(Startup, (setup_assets, spawn_beacons).chain())
            .add_systems(
                Update,
                (
                    respawn_on_restart,
                    feed.run_if(not(in_state(GameState::Title))),
                    receive,
                    animate,
                    flames,
                    check_victory,
                )
                    .chain(),
            );
    }
}

pub const FEED_RADIUS: f32 = 7.5;
const BRAZIER_Y: f32 = 4.6;

#[derive(Message)]
pub struct BeaconLit {
    pub pos: Vec3,
}

#[derive(Component)]
pub struct Beacon {
    pub fuel: u32,
    pub incoming: u32,
    pub lit: bool,
    lit_t: f32,
    feed_timer: f32,
    pub base: Vec3,
    flame_acc: f32,
}

impl Beacon {
    pub fn brazier(&self) -> Vec3 {
        self.base + Vec3::Y * BRAZIER_Y
    }
}

#[derive(Component)]
struct Pip {
    n: u32,
}

#[derive(Component)]
struct Beam {
    mat: Handle<StandardMaterial>,
}

#[derive(Component)]
struct Rune {
    mat: Handle<StandardMaterial>,
}

#[derive(Component)]
struct BeaconLight;

#[derive(Resource)]
struct BeaconAssets {
    stone: Handle<Mesh>,
    stone_mat: Handle<StandardMaterial>,
    pip: Handle<Mesh>,
    pip_off: Handle<StandardMaterial>,
    pip_on: Handle<StandardMaterial>,
    beam: Handle<Mesh>,
    rune: Handle<Mesh>,
}

fn shrine_mesh() -> Mesh {
    let mut rng = Rng::new(5);
    let stone = srgb("8b8796");
    let dark = srgb("5f5b6b");
    let moss = srgb("4d7a55");
    let mut parts = vec![
        part(Cylinder::new(4.4, 0.6).mesh().resolution(8).build(), dark, Transform::from_xyz(0.0, 0.1, 0.0)),
        part(Cylinder::new(3.6, 0.5).mesh().resolution(8).build(), stone, Transform::from_xyz(0.0, 0.6, 0.0)),
        part(
            Cylinder::new(0.55, 3.2).mesh().resolution(6).build(),
            stone,
            Transform::from_xyz(0.0, 2.4, 0.0),
        ),
        part(
            Cone::new(1.4, 1.0).mesh().resolution(8).build(),
            dark,
            Transform::from_xyz(0.0, BRAZIER_Y - 0.4, 0.0).with_rotation(Quat::from_rotation_x(std::f32::consts::PI)),
        ),
        part(
            Torus::new(1.2, 1.5).mesh().minor_resolution(4).major_resolution(8).build(),
            stone,
            Transform::from_xyz(0.0, BRAZIER_Y + 0.05, 0.0),
        ),
    ];
    for i in 0..4 {
        let a = std::f32::consts::FRAC_PI_4 + i as f32 * std::f32::consts::FRAC_PI_2;
        let p = Vec3::new(a.cos(), 0.0, a.sin()) * 2.9;
        let h = 3.4 + rng.f32() * 1.2;
        let tilt = Quat::from_rotation_z(rng.signed() * 0.06) * Quat::from_rotation_x(rng.signed() * 0.06);
        parts.push(part(
            Cuboid::new(0.7, h, 0.7).mesh().build(),
            stone,
            Transform::from_translation(p + Vec3::Y * (0.6 + h * 0.5)).with_rotation(Quat::from_rotation_y(a) * tilt),
        ));
        parts.push(part(
            Cuboid::new(0.95, 0.3, 0.95).mesh().build(),
            dark,
            Transform::from_translation(p + Vec3::Y * (0.75 + h)).with_rotation(Quat::from_rotation_y(a) * tilt),
        ));
        parts.push(part(
            jitter(ico(0), &mut rng, 0.3),
            moss,
            Transform::from_translation(p + Vec3::new(0.3, 0.75, 0.2)).with_scale(Vec3::new(0.6, 0.3, 0.6)),
        ));
    }
    combine(parts)
}

fn setup_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(BeaconAssets {
        stone: meshes.add(shrine_mesh()),
        stone_mat: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.9,
            ..default()
        }),
        pip: meshes.add(Sphere::new(1.0).mesh().ico(0).unwrap()),
        pip_off: materials.add(StandardMaterial {
            base_color: hdr(0.25, 0.45, 0.9),
            unlit: true,
            ..default()
        }),
        pip_on: materials.add(StandardMaterial {
            base_color: hdr(14.0, 6.0, 1.2),
            unlit: true,
            ..default()
        }),
        beam: meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(16).segments(1).build()),
        rune: meshes.add(Torus::new(5.6, 5.9).mesh().minor_resolution(4).major_resolution(64)),
    });
}

fn cold_beam() -> Color {
    hdr(0.05, 0.12, 0.3)
}

fn warm_beam() -> Color {
    hdr(1.6, 0.7, 0.18)
}

fn spawn_beacon(commands: &mut Commands, a: &BeaconAssets, materials: &mut Assets<StandardMaterial>, i: usize) {
    let site = BEACON_SITES[i];
    let base = Vec3::new(site.x, site_height(i), site.y);
    let beam_mat = materials.add(StandardMaterial {
        base_color: cold_beam(),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        fog_enabled: false,
        cull_mode: None,
        ..default()
    });
    let rune_mat = materials.add(StandardMaterial {
        base_color: hdr(0.1, 0.25, 0.6),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        ..default()
    });
    commands
        .spawn((
            LevelEntity,
            Beacon { fuel: 0, incoming: 0, lit: false, lit_t: 0.0, feed_timer: 0.0, base, flame_acc: 0.0 },
            Mesh3d(a.stone.clone()),
            MeshMaterial3d(a.stone_mat.clone()),
            Transform::from_translation(base - Vec3::Y * 0.3).with_rotation(Quat::from_rotation_y(i as f32 * 0.7)),
        ))
        .with_children(|b| {
            for n in 0..EMBERS_PER_BEACON {
                b.spawn((
                    Pip { n },
                    Mesh3d(a.pip.clone()),
                    MeshMaterial3d(a.pip_off.clone()),
                    Transform::from_xyz(0.0, BRAZIER_Y + 0.9, 0.0).with_scale(Vec3::splat(0.16)),
                    NotShadowCaster,
                ));
            }
            b.spawn((
                Beam { mat: beam_mat.clone() },
                Mesh3d(a.beam.clone()),
                MeshMaterial3d(beam_mat),
                Transform::from_xyz(0.0, BRAZIER_Y + 150.0, 0.0).with_scale(Vec3::new(0.3, 300.0, 0.3)),
                NotShadowCaster,
                NotShadowReceiver,
            ));
            b.spawn((
                Rune { mat: rune_mat.clone() },
                Mesh3d(a.rune.clone()),
                MeshMaterial3d(rune_mat),
                Transform::from_xyz(0.0, 0.75, 0.0).with_scale(Vec3::new(1.0, 0.2, 1.0)),
                NotShadowCaster,
                NotShadowReceiver,
            ));
            b.spawn((
                BeaconLight,
                PointLight {
                    color: Color::srgb(0.5, 0.65, 1.0),
                    intensity: 60_000.0,
                    range: 18.0,
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_xyz(0.0, BRAZIER_Y + 1.0, 0.0),
            ));
        });
}

fn spawn_beacons(
    mut commands: Commands,
    assets: Res<BeaconAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut progress: ResMut<Progress>,
) {
    let prelit: usize = std::env::var("EMBER_LIT").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    for i in 0..BEACON_COUNT {
        spawn_beacon(&mut commands, &assets, &mut materials, i);
    }
    progress.lit = 0;
    if prelit > 0 {
        commands.insert_resource(Prelit(prelit.min(BEACON_COUNT)));
    }
}

/// Dev aid: beacons to light immediately (`EMBER_LIT=n`).
#[derive(Resource)]
struct Prelit(usize);

fn respawn_on_restart(
    mut commands: Commands,
    mut restarts: MessageReader<RestartLevel>,
    beacons: Query<(Entity, &Children), With<Beacon>>,
    beams: Query<&Beam>,
    runes: Query<&Rune>,
    assets: Res<BeaconAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if restarts.read().count() == 0 {
        return;
    }
    for (e, children) in &beacons {
        for c in children {
            if let Ok(b) = beams.get(*c) {
                materials.remove(&b.mat);
            }
            if let Ok(r) = runes.get(*c) {
                materials.remove(&r.mat);
            }
        }
        commands.entity(e).despawn();
    }
    for i in 0..BEACON_COUNT {
        spawn_beacon(&mut commands, &assets, &mut materials, i);
    }
}

fn feed(
    time: Res<Time>,
    player: Single<&Transform, With<Player>>,
    mut beacons: Query<(Entity, &mut Beacon)>,
    mut embers: Query<(&mut Ember, &Transform), Without<Player>>,
) {
    let dt = time.delta_secs();
    for (entity, mut b) in &mut beacons {
        b.feed_timer -= dt;
        if b.lit || b.fuel + b.incoming >= EMBERS_PER_BEACON || b.feed_timer > 0.0 {
            continue;
        }
        if player.translation.xz().distance(b.base.xz()) > FEED_RADIUS {
            continue;
        }
        let highest = embers
            .iter_mut()
            .filter_map(|(e, tf)| match *e {
                Ember::Carried { slot } => Some((slot, e, tf)),
                _ => None,
            })
            .max_by_key(|(slot, _, _)| *slot);
        if let Some((_, mut ember, tf)) = highest {
            *ember = Ember::Offering { from: tf.translation, to: b.brazier(), t: 0.0, beacon: entity };
            b.incoming += 1;
            b.feed_timer = 0.2;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn receive(
    mut commands: Commands,
    mut delivered: MessageReader<EmberDelivered>,
    mut lit_writer: MessageWriter<BeaconLit>,
    mut sfx: MessageWriter<Sfx>,
    mut beacons: Query<(Entity, &mut Beacon, &Children)>,
    mut pips: Query<(&Pip, &mut MeshMaterial3d<StandardMaterial>)>,
    assets: Res<BeaconAssets>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut trauma: ResMut<Trauma>,
    mut progress: ResMut<Progress>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    prelit: Option<Res<Prelit>>,
) {
    let mut events: Vec<Entity> = delivered.read().map(|d| d.beacon).collect();
    if let Some(p) = prelit {
        // Dev shortcut: fill the first n beacons instantly.
        for (e, _, _) in beacons.iter().take(p.0) {
            events.extend(std::iter::repeat_n(e, EMBERS_PER_BEACON as usize));
        }
        commands.remove_resource::<Prelit>();
        for (_, mut b, _) in beacons.iter_mut().take(p.0) {
            b.incoming = EMBERS_PER_BEACON;
        }
    }
    for target in events {
        let Ok((_, mut b, children)) = beacons.get_mut(target) else { continue };
        b.incoming = b.incoming.saturating_sub(1);
        b.fuel += 1;
        for c in children {
            if let Ok((pip, mut mat)) = pips.get_mut(*c)
                && pip.n < b.fuel
            {
                mat.0 = assets.pip_on.clone();
            }
        }
        sfx.write(Sfx::pitched(SfxKind::Deposit, 1.0 + b.fuel as f32 * 0.12));
        if b.fuel >= EMBERS_PER_BEACON && !b.lit {
            b.lit = true;
            b.lit_t = 0.0;
            progress.lit += 1;
            let pos = b.brazier();
            lit_writer.write(BeaconLit { pos });
            trauma.add(0.6);
            sfx.write(Sfx::new(SfxKind::Ignite));
            burst(
                &mut commands,
                &fx,
                &mut rng.0,
                Burst {
                    pos,
                    count: 90,
                    palette: Palette::Ember,
                    speed: (6.0, 22.0),
                    size: (0.1, 0.3),
                    life: (0.8, 1.8),
                    gravity: 6.0,
                    drag: 1.2,
                    up_bias: 0.7,
                    curve: SizeCurve::Pop,
                },
            );
            burst(
                &mut commands,
                &fx,
                &mut rng.0,
                Burst { pos, count: 40, palette: Palette::White, speed: (4.0, 12.0), life: (0.4, 0.9), ..default() },
            );
            spawn_ring(&mut commands, &fx, &mut materials, b.base + Vec3::Y * 0.8, 40.0, 1.6, LinearRgba::rgb(3.0, 1.3, 0.3));
            spawn_ring(&mut commands, &fx, &mut materials, pos, 14.0, 0.9, LinearRgba::rgb(4.0, 3.0, 1.5));
        }
    }
}

#[allow(clippy::type_complexity)]
fn animate(
    time: Res<Time>,
    mut beacons: Query<(&mut Beacon, &Children)>,
    mut pips: Query<(&Pip, &mut Transform), (Without<Beam>, Without<Rune>)>,
    mut beams: Query<(&Beam, &mut Transform), (Without<Pip>, Without<Rune>)>,
    mut runes: Query<(&Rune, &mut Transform), (Without<Pip>, Without<Beam>)>,
    mut lights: Query<&mut PointLight, With<BeaconLight>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (mut b, children) in &mut beacons {
        if b.lit {
            b.lit_t += dt;
        }
        let ignite = smoothstep(0.0, 1.2, b.lit_t) * if b.lit { 1.0 } else { 0.0 };
        let fill = b.fuel as f32 / EMBERS_PER_BEACON as f32;
        for c in children {
            if let Ok((pip, mut tf)) = pips.get_mut(*c) {
                let a = t * 0.8 + pip.n as f32 * std::f32::consts::TAU / EMBERS_PER_BEACON as f32;
                let r = 1.7 + ignite * 0.6;
                tf.translation = Vec3::new(a.cos() * r, BRAZIER_Y + 1.0 + (t * 2.0 + pip.n as f32).sin() * 0.15, a.sin() * r);
                tf.rotation = Quat::from_rotation_y(t * 2.0);
                let on = pip.n < b.fuel;
                let target = if on { 0.24 } else { 0.14 };
                tf.scale = tf.scale.lerp(Vec3::new(target, target * 1.5, target), damp(8.0, dt));
            }
            if let Ok((beam, mut tf)) = beams.get_mut(*c) {
                let width = if b.lit {
                    let pop = ease_out_back((b.lit_t / 0.9).min(1.0));
                    0.3 + pop * 1.1 + (t * 3.0).sin() * 0.08
                } else {
                    0.3 + 0.04 * (t * 1.5).sin()
                };
                tf.scale.x = width;
                tf.scale.z = width;
                if let Some(mut m) = materials.get_mut(&beam.mat) {
                    let pulse = 0.85 + 0.15 * (t * 2.0 + b.base.x).sin();
                    let base = lerp_color(cold_beam(), warm_beam(), ignite).to_linear() * pulse;
                    // Brief white flash on ignition.
                    let flash = (1.0 - b.lit_t / 0.6).max(0.0) * if b.lit { 6.0 } else { 0.0 };
                    m.base_color = Color::LinearRgba(base + LinearRgba::rgb(flash, flash * 0.8, flash * 0.5));
                }
            }
            if let Ok((rune, mut tf)) = runes.get_mut(*c) {
                tf.rotation = Quat::from_rotation_y(t * (0.1 + ignite * 0.4));
                if let Some(mut m) = materials.get_mut(&rune.mat) {
                    let idle = 0.5 + 0.5 * (t * 1.2 + b.base.z).sin();
                    let cold = hdr(0.05 + fill * 0.6, 0.15 + fill * 0.3, 0.45 * idle + 0.1);
                    m.base_color = lerp_color(cold, hdr(3.0, 1.4, 0.35), ignite);
                }
            }
            if let Ok(mut l) = lights.get_mut(*c) {
                let flicker = 1.0 + 0.12 * (t * 13.0 + b.base.x).sin() + 0.08 * (t * 29.0).sin();
                l.color = lerp_color(Color::srgb(0.5, 0.65, 1.0), Color::srgb(1.0, 0.6, 0.25), ignite);
                l.intensity = (60_000.0 + fill * 100_000.0).lerp(1_600_000.0 * flicker, ignite);
                l.range = 18.0 + ignite * 22.0;
            }
        }
    }
}

fn flames(
    mut commands: Commands,
    time: Res<Time>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut beacons: Query<&mut Beacon>,
) {
    for mut b in &mut beacons {
        if !b.lit {
            continue;
        }
        b.flame_acc += time.delta_secs() * 45.0;
        let origin = b.brazier();
        while b.flame_acc >= 1.0 {
            b.flame_acc -= 1.0;
            let off = Vec3::new(rng.0.signed(), 0.0, rng.0.signed()) * 0.7;
            let life = rng.0.range(0.5, 1.1);
            let size = rng.0.range(0.15, 0.4);
            let palette = if rng.0.f32() < 0.6 { Palette::Flame } else { Palette::Gold };
            commands.spawn((
                Mesh3d(fx.sphere.clone()),
                MeshMaterial3d(fx.mat(palette)),
                Transform::from_translation(origin + off).with_scale(Vec3::splat(size)),
                crate::fx::Particle {
                    vel: Vec3::new(-off.x * 0.8, rng.0.range(2.5, 5.0), -off.z * 0.8),
                    life,
                    max_life: life,
                    size,
                    gravity: -1.5,
                    drag: 1.0,
                    curve: SizeCurve::Pop,
                },
                NotShadowCaster,
                NotShadowReceiver,
            ));
        }
    }
}

fn check_victory(
    time: Res<Time>,
    progress: Res<Progress>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    mut delay: Local<f32>,
) {
    if progress.lit >= BEACON_COUNT && *state.get() == GameState::Playing {
        *delay += time.delta_secs();
        if *delay > 3.0 {
            next.set(GameState::Victory);
            *delay = 0.0;
        }
    } else {
        *delay = 0.0;
    }
}
