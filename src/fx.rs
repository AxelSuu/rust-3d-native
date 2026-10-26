use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
};

use crate::util::{Rng, emit};

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Trauma>()
            .insert_resource(FxRng(Rng::new(99)))
            .add_systems(PreStartup, setup_fx)
            .add_systems(Update, (tick_particles, tick_rings, decay_trauma));
    }
}

#[derive(Resource)]
pub struct FxRng(pub Rng);

/// Camera shake budget; shake magnitude is trauma squared.
#[derive(Resource, Default)]
pub struct Trauma(pub f32);

impl Trauma {
    pub fn add(&mut self, v: f32) {
        self.0 = (self.0 + v).min(1.0);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Palette {
    Gold,
    White,
    Cyan,
    Violet,
    Flame,
    Ember,
    Dust,
    Shadow,
}

#[derive(Resource)]
pub struct FxAssets {
    pub sphere: Handle<Mesh>,
    pub diamond: Handle<Mesh>,
    pub ring: Handle<Mesh>,
    mats: Vec<(Palette, Handle<StandardMaterial>)>,
}

impl FxAssets {
    pub fn mat(&self, p: Palette) -> Handle<StandardMaterial> {
        self.mats.iter().find(|(k, _)| *k == p).unwrap().1.clone()
    }
}

fn setup_fx(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let glow = |m: &mut Assets<StandardMaterial>, e: LinearRgba| {
        m.add(StandardMaterial {
            base_color: Color::LinearRgba(e),
            unlit: true,
            ..default()
        })
    };
    let mats = vec![
        (Palette::Gold, glow(&mut materials, LinearRgba::rgb(18.0, 9.0, 2.0))),
        (Palette::White, glow(&mut materials, LinearRgba::rgb(14.0, 13.0, 11.0))),
        (Palette::Cyan, glow(&mut materials, LinearRgba::rgb(2.0, 10.0, 14.0))),
        (Palette::Violet, glow(&mut materials, LinearRgba::rgb(6.0, 1.0, 12.0))),
        (Palette::Flame, glow(&mut materials, LinearRgba::rgb(30.0, 8.0, 1.0))),
        (Palette::Ember, glow(&mut materials, LinearRgba::rgb(40.0, 18.0, 4.0))),
        (
            Palette::Dust,
            materials.add(StandardMaterial {
                base_color: Srgba::hex("c9b8a0").unwrap().into(),
                perceptual_roughness: 1.0,
                ..default()
            }),
        ),
        (
            Palette::Shadow,
            materials.add(StandardMaterial {
                base_color: Srgba::hex("120a1c").unwrap().into(),
                emissive: emit(0.6, 0.05, 1.2),
                perceptual_roughness: 1.0,
                ..default()
            }),
        ),
    ];
    commands.insert_resource(FxAssets {
        sphere: meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap()),
        diamond: meshes.add(Sphere::new(1.0).mesh().ico(0).unwrap()),
        ring: meshes.add(Torus::new(0.94, 1.0).mesh().minor_resolution(6).major_resolution(48)),
        mats,
    });
}

#[derive(Clone, Copy)]
pub enum SizeCurve {
    /// Starts at full size, shrinks to nothing.
    Shrink,
    /// Grows quickly, then shrinks.
    Pop,
}

#[derive(Component)]
pub struct Particle {
    pub vel: Vec3,
    pub life: f32,
    pub max_life: f32,
    pub size: f32,
    pub gravity: f32,
    pub drag: f32,
    pub curve: SizeCurve,
}

pub struct Burst {
    pub pos: Vec3,
    pub count: usize,
    pub palette: Palette,
    pub speed: (f32, f32),
    pub size: (f32, f32),
    pub life: (f32, f32),
    pub gravity: f32,
    pub drag: f32,
    pub up_bias: f32,
    pub curve: SizeCurve,
}

impl Default for Burst {
    fn default() -> Self {
        Self {
            pos: Vec3::ZERO,
            count: 12,
            palette: Palette::Gold,
            speed: (2.0, 6.0),
            size: (0.06, 0.14),
            life: (0.5, 1.0),
            gravity: 0.0,
            drag: 2.0,
            up_bias: 0.0,
            curve: SizeCurve::Shrink,
        }
    }
}

pub fn burst(commands: &mut Commands, fx: &FxAssets, rng: &mut Rng, b: Burst) {
    let mat = fx.mat(b.palette);
    let mesh = if b.palette == Palette::Dust { fx.diamond.clone() } else { fx.sphere.clone() };
    for _ in 0..b.count {
        let dir = (rng.unit_vec3() + Vec3::Y * b.up_bias).normalize_or_zero();
        let size = rng.range(b.size.0, b.size.1);
        let life = rng.range(b.life.0, b.life.1);
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(b.pos).with_scale(Vec3::splat(size)),
            Particle {
                vel: dir * rng.range(b.speed.0, b.speed.1),
                life,
                max_life: life,
                size,
                gravity: b.gravity,
                drag: b.drag,
                curve: b.curve,
            },
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
}

fn tick_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transform, &mut Particle)>,
) {
    let dt = time.delta_secs();
    for (e, mut tf, mut p) in &mut q {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        p.vel.y -= p.gravity * dt;
        let drag = (-p.drag * dt).exp();
        p.vel *= drag;
        tf.translation += p.vel * dt;
        let t = 1.0 - p.life / p.max_life;
        let k = match p.curve {
            SizeCurve::Shrink => 1.0 - t,
            SizeCurve::Pop => (t * 6.0).min(1.0) * (1.0 - t),
        };
        tf.scale = Vec3::splat(p.size * k.max(0.0));
    }
}

/// An expanding, fading torus with its own material so alpha can animate.
#[derive(Component)]
pub struct Ring {
    pub t: f32,
    pub duration: f32,
    pub radius: f32,
    pub emissive: LinearRgba,
}

pub fn spawn_ring(
    commands: &mut Commands,
    fx: &FxAssets,
    materials: &mut Assets<StandardMaterial>,
    pos: Vec3,
    radius: f32,
    duration: f32,
    emissive: LinearRgba,
) {
    let mat = materials.add(StandardMaterial {
        base_color: Color::LinearRgba(emissive),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        ..default()
    });
    commands.spawn((
        Mesh3d(fx.ring.clone()),
        MeshMaterial3d(mat),
        Transform::from_translation(pos).with_scale(Vec3::ZERO),
        Ring { t: 0.0, duration, radius, emissive },
        NotShadowCaster,
        NotShadowReceiver,
    ));
}

fn tick_rings(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(Entity, &mut Transform, &mut Ring, &MeshMaterial3d<StandardMaterial>)>,
) {
    for (e, mut tf, mut ring, mat) in &mut q {
        ring.t += time.delta_secs();
        let k = (ring.t / ring.duration).min(1.0);
        if k >= 1.0 {
            materials.remove(&mat.0);
            commands.entity(e).despawn();
            continue;
        }
        let r = ring.radius * crate::util::ease_out_cubic(k);
        tf.scale = Vec3::new(r, (1.0 - k) * 3.0 + 0.3, r);
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.base_color = Color::LinearRgba(ring.emissive * (1.0 - k).powi(2));
        }
    }
}

fn decay_trauma(time: Res<Time>, mut trauma: ResMut<Trauma>) {
    trauma.0 = (trauma.0 - time.delta_secs() * 0.9).max(0.0);
}
