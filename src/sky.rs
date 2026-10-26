use bevy::{
    light::{CascadeShadowConfigBuilder, NotShadowCaster, NotShadowReceiver},
    mesh::VertexAttributeValues,
    prelude::*,
};

use crate::{
    BEACON_COUNT, Progress,
    util::{Rng, damp, gradient, hdr, lerp_color, smoothstep},
    world::flat_mesh,
};

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Daylight>()
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(0.35, 0.38, 0.8),
                brightness: 300.0,
                ..default()
            })
            .add_systems(Startup, spawn_sky)
            .add_systems(Update, (advance_daylight, apply_daylight).chain());
    }
}

/// 0 = deep night, 1 = full dawn. Each kindled beacon pushes the sky toward morning.
#[derive(Resource, Default)]
pub struct Daylight {
    pub value: f32,
    applied: Option<f32>,
}

#[derive(Component)]
struct SkyDome;

#[derive(Component)]
struct Stars {
    phase: f32,
}

#[derive(Component)]
struct Moon;

#[derive(Component)]
struct Sun;

#[derive(Component)]
pub struct Celestial;

const SKY_RADIUS: f32 = 900.0;

fn moon_dir() -> Vec3 {
    Vec3::new(-0.35, 0.8, -0.5).normalize()
}

fn sun_dir(d: f32) -> Vec3 {
    Vec3::new(0.75, -0.08 + 0.3 * d, -0.6).normalize()
}

fn spawn_sky(
    mut commands: Commands,
    quality: Res<crate::Quality>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dome = Sphere::new(SKY_RADIUS).mesh().uv(48, 32);
    let count = dome.count_vertices();
    let dome = dome.with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32, 0.0, 0.0, 1.0]; count]);
    let sky = commands
        .spawn((
            SkyDome,
            Mesh3d(meshes.add(dome)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::WHITE,
                unlit: true,
                cull_mode: None,
                fog_enabled: false,
                ..default()
            })),
            Transform::default(),
            NotShadowCaster,
            NotShadowReceiver,
        ))
        .id();

    // Three star layers twinkle out of phase with one another.
    let mut rng = Rng::new(77);
    for layer in 0..3 {
        let mut positions = Vec::new();
        let mut colors = Vec::new();
        for _ in 0..260 {
            let mut d = rng.unit_vec3();
            d.y = d.y.abs() * 0.95 + 0.05;
            let d = d.normalize();
            let c = d * (SKY_RADIUS * 0.95);
            let size = rng.range(0.8, 2.4);
            let right = d.cross(Vec3::Y).normalize_or(Vec3::X) * size;
            let up = right.cross(d).normalize() * size;
            let tint = rng.f32();
            let col = hdr(2.0 + tint, 2.0 + tint * 0.5, 2.4 - tint * 0.6).to_linear().to_f32_array();
            for v in [c - right, c + up, c + right, c - right, c + right, c - up] {
                positions.push(v.to_array());
                colors.push(col);
            }
        }
        let mat = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            cull_mode: None,
            fog_enabled: false,
            alpha_mode: AlphaMode::Add,
            ..default()
        });
        let star = commands
            .spawn((
                Stars { phase: layer as f32 * 2.1 },
                Mesh3d(meshes.add(flat_mesh(positions, colors))),
                MeshMaterial3d(mat),
                NotShadowCaster,
                NotShadowReceiver,
            ))
            .id();
        commands.entity(sky).add_child(star);
    }

    let disc = |m: &mut Assets<StandardMaterial>, c: Color| {
        m.add(StandardMaterial {
            base_color: c,
            unlit: true,
            fog_enabled: false,
            alpha_mode: AlphaMode::Add,
            ..default()
        })
    };
    let ball = meshes.add(Sphere::new(1.0).mesh().uv(32, 16));
    let moon = commands
        .spawn((
            Moon,
            Mesh3d(ball.clone()),
            MeshMaterial3d(disc(&mut materials, hdr(3.0, 3.2, 3.8))),
            Transform::from_translation(moon_dir() * SKY_RADIUS * 0.9).with_scale(Vec3::splat(26.0)),
            NotShadowCaster,
            NotShadowReceiver,
        ))
        .with_child((
            Mesh3d(ball.clone()),
            MeshMaterial3d(disc(&mut materials, hdr(0.05, 0.06, 0.12))),
            Transform::from_scale(Vec3::splat(2.6)),
            NotShadowCaster,
            NotShadowReceiver,
        ))
        .id();
    let sun = commands
        .spawn((
            Sun,
            Mesh3d(ball.clone()),
            MeshMaterial3d(disc(&mut materials, hdr(30.0, 16.0, 6.0))),
            Transform::from_translation(sun_dir(0.0) * SKY_RADIUS * 0.9).with_scale(Vec3::splat(34.0)),
            NotShadowCaster,
            NotShadowReceiver,
        ))
        .with_child((
            Mesh3d(ball),
            MeshMaterial3d(disc(&mut materials, hdr(0.25, 0.1, 0.04))),
            Transform::from_scale(Vec3::splat(3.2)),
            NotShadowCaster,
            NotShadowReceiver,
        ))
        .id();
    commands.entity(sky).add_children(&[moon, sun]);

    commands.spawn((
        Celestial,
        DirectionalLight {
            illuminance: 7000.0,
            shadow_maps_enabled: !quality.low,
            color: Color::srgb(0.6, 0.7, 1.0),
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            first_cascade_far_bound: 25.0,
            maximum_distance: 160.0,
            ..default()
        }
        .build(),
        Transform::from_translation(moon_dir()).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn advance_daylight(time: Res<Time>, progress: Res<Progress>, mut day: ResMut<Daylight>) {
    let target = progress.lit as f32 / BEACON_COUNT as f32;
    // Slow sweep, but always moves at least a little so the change is felt right away.
    let step = (target - day.value) * damp(0.5, time.delta_secs());
    let min = 0.03 * time.delta_secs();
    day.value = if (target - day.value).abs() < min {
        target
    } else {
        day.value + step.abs().max(min) * (target - day.value).signum()
    };
}

#[derive(Clone, Copy)]
struct Palette {
    zenith: Color,
    horizon: Color,
    glow: Color,
}

fn palette(d: f32) -> Palette {
    let srgb = |h: &str| -> Color { Srgba::hex(h).unwrap().into() };
    Palette {
        zenith: gradient(&[srgb("05061a"), srgb("12153d"), srgb("2a3d7a"), srgb("4f86c6")], d),
        horizon: gradient(&[srgb("1f1c4a"), srgb("4a2f6b"), srgb("b5577a"), srgb("ffb38a")], d),
        glow: gradient(&[srgb("2a2a6a"), srgb("8a3f6e"), srgb("ff7a4a"), srgb("ffd59a")], d),
    }
}

pub fn fog_color(d: f32) -> Color {
    let p = palette(d);
    lerp_color(p.horizon, p.zenith, 0.25)
}

fn apply_daylight(
    time: Res<Time>,
    mut day: ResMut<Daylight>,
    camera: Single<(&Transform, &mut DistanceFog), With<Camera3d>>,
    mut sky: Query<(&mut Transform, &Mesh3d), (With<SkyDome>, Without<Camera3d>)>,
    stars: Query<(&Stars, &MeshMaterial3d<StandardMaterial>)>,
    mut sun: Query<&mut Transform, (With<Sun>, Without<SkyDome>, Without<Camera3d>)>,
    mut moon: Query<&mut Transform, (With<Moon>, Without<Sun>, Without<SkyDome>, Without<Camera3d>)>,
    mut light: Query<(&mut DirectionalLight, &mut Transform), (With<Celestial>, Without<Moon>, Without<Sun>, Without<SkyDome>, Without<Camera3d>)>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let d = day.value;
    let (cam_tf, mut fog) = camera.into_inner();
    let t = time.elapsed_secs();

    for (mut tf, mesh) in &mut sky {
        tf.translation = cam_tf.translation;
        if day.applied.is_none_or(|a| (d - a).abs() > 0.002) {
            let p = palette(d);
            let sun_d = sun_dir(d);
            let moon_d = moon_dir();
            if let Some(mut m) = meshes.get_mut(&mesh.0)
                && let Some(VertexAttributeValues::Float32x3(pos)) = m.attribute(Mesh::ATTRIBUTE_POSITION).cloned()
            {
                let colors: Vec<[f32; 4]> = pos
                    .iter()
                    .map(|v| {
                        let dir = Vec3::from(*v).normalize();
                        let h = dir.y.max(0.0);
                        let mut c = lerp_color(p.horizon, p.zenith, smoothstep(0.0, 0.55, h).powf(0.8));
                        let below = smoothstep(0.0, -0.25, dir.y);
                        c = lerp_color(c, p.horizon, below * 0.5);
                        let sun_amt = dir.dot(sun_d).max(0.0).powf(6.0) * smoothstep(0.1, 0.6, d);
                        c = lerp_color(c, p.glow, sun_amt);
                        let moon_amt = dir.dot(moon_d).max(0.0).powf(24.0) * (1.0 - d) * 0.35;
                        c = lerp_color(c, Color::srgb(0.5, 0.55, 0.9), moon_amt);
                        c.to_linear().to_f32_array()
                    })
                    .collect();
                m.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
            }
            day.applied = Some(d);
        }
    }

    for (s, mat) in &stars {
        if let Some(mut m) = materials.get_mut(&mat.0) {
            let tw = 0.6 + 0.4 * (t * 1.7 + s.phase).sin();
            let k = (1.0 - smoothstep(0.35, 0.85, d)) * tw;
            m.base_color = Color::LinearRgba(LinearRgba::rgb(k, k, k));
        }
    }

    for mut tf in &mut sun {
        tf.translation = sun_dir(d) * SKY_RADIUS * 0.9;
    }
    for mut tf in &mut moon {
        let sink = smoothstep(0.4, 1.0, d);
        let dir = (moon_dir() - Vec3::Y * sink * 0.8).normalize();
        tf.translation = dir * SKY_RADIUS * 0.9;
    }

    let night_color = Color::srgb(0.55, 0.65, 1.0);
    let dawn_color = Color::srgb(1.0, 0.72, 0.5);
    for (mut l, mut tf) in &mut light {
        let dir = moon_dir().lerp(sun_dir(d.max(0.6)), smoothstep(0.3, 0.8, d)).normalize();
        *tf = Transform::from_translation(dir).looking_at(Vec3::ZERO, Vec3::Y);
        l.color = lerp_color(night_color, dawn_color, smoothstep(0.2, 1.0, d));
        l.illuminance = 7000.0 + 5000.0 * smoothstep(0.3, 1.0, d);
    }

    ambient.color = lerp_color(Color::srgb(0.35, 0.38, 0.8), Color::srgb(1.0, 0.8, 0.75), d);
    ambient.brightness = 1100.0 + 500.0 * d;

    let p = palette(d);
    fog.color = fog_color(d);
    fog.directional_light_color = p.glow.with_alpha(0.5 * smoothstep(0.3, 0.9, d));
    fog.directional_light_exponent = 12.0;
}
