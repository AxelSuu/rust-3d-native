use std::sync::LazyLock;

use bevy::{
    asset::RenderAssetUsages,
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::VertexAttributeValues,
    prelude::*,
    render::render_resource::PrimitiveTopology,
};

use crate::util::{Rng, emit, fbm, gradient, hdr, noise2, smoothstep};

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_terrain, spawn_water, spawn_props))
            .add_systems(Update, (animate_water, sway, fireflies));
    }
}

pub struct Island {
    pub center: Vec2,
    pub radius: f32,
    pub peak: f32,
}

pub const ISLANDS: [Island; 6] = [
    Island { center: Vec2::new(0.0, 0.0), radius: 62.0, peak: 6.5 },
    Island { center: Vec2::new(92.0, 30.0), radius: 42.0, peak: 5.5 },
    Island { center: Vec2::new(-78.0, 62.0), radius: 40.0, peak: 6.0 },
    Island { center: Vec2::new(-56.0, -92.0), radius: 44.0, peak: 7.0 },
    Island { center: Vec2::new(62.0, -88.0), radius: 36.0, peak: 5.0 },
    Island { center: Vec2::new(10.0, 105.0), radius: 24.0, peak: 4.0 },
];

/// Beacons sit on flattened plateaus near the island cores.
pub const BEACON_SITES: [Vec2; 5] = [
    Vec2::new(-6.0, -10.0),
    Vec2::new(94.0, 28.0),
    Vec2::new(-80.0, 60.0),
    Vec2::new(-56.0, -90.0),
    Vec2::new(62.0, -86.0),
];

pub const PLAYER_SPAWN: Vec2 = Vec2::new(4.0, 30.0);
pub const WORLD_RADIUS: f32 = 170.0;
const SEA_FLOOR: f32 = -8.0;
const PLATEAU_INNER: f32 = 6.5;
const PLATEAU_OUTER: f32 = 11.0;

fn raw_height(p: Vec2) -> f32 {
    let warp = Vec2::new(fbm(p * 0.018, 3, 11), fbm(p * 0.018 + Vec2::new(5.2, 1.3), 3, 17)) * 18.0;
    let q = p + warp;
    let mut h = SEA_FLOOR;
    for isl in &ISLANDS {
        let d = q.distance(isl.center) / isl.radius;
        let t = smoothstep(1.15, 0.0, d);
        h = h.max(SEA_FLOOR + (isl.peak - SEA_FLOOR) * t);
    }
    let land = smoothstep(-3.0, 3.0, h);
    h += fbm(p * 0.045, 4, 3) * 2.6 * land;
    let ridge = (1.0 - fbm(p * 0.03, 3, 7).abs() * 2.2).max(0.0);
    h += ridge * ridge * 3.0 * smoothstep(3.0, 6.0, h);
    // Flatten the band just above sea level into beaches.
    if h > 0.0 {
        h *= smoothstep(0.0, 3.5, h).max(0.08);
    }
    h
}

static SITE_HEIGHTS: LazyLock<[f32; 5]> =
    LazyLock::new(|| BEACON_SITES.map(|s| raw_height(s).max(2.5)));

pub fn site_height(i: usize) -> f32 {
    SITE_HEIGHTS[i]
}

pub fn terrain_height(x: f32, z: f32) -> f32 {
    let p = Vec2::new(x, z);
    let mut h = raw_height(p);
    for (i, s) in BEACON_SITES.iter().enumerate() {
        let d = p.distance(*s);
        if d < PLATEAU_OUTER {
            h = SITE_HEIGHTS[i].lerp(h, smoothstep(PLATEAU_INNER, PLATEAU_OUTER, d));
        }
    }
    h
}

pub fn water_height(x: f32, z: f32, t: f32) -> f32 {
    0.22 * (x * 0.09 + t * 1.1).sin()
        + 0.17 * (z * 0.12 - t * 0.9).sin()
        + 0.12 * ((x + z) * 0.21 + t * 1.7).sin()
        + 0.07 * ((x - z) * 0.33 - t * 2.3).sin()
}

pub fn surface_height(x: f32, z: f32, t: f32) -> f32 {
    terrain_height(x, z).max(water_height(x, z, t))
}

pub fn terrain_normal(x: f32, z: f32) -> Vec3 {
    let e = 0.6;
    let dx = terrain_height(x + e, z) - terrain_height(x - e, z);
    let dz = terrain_height(x, z + e) - terrain_height(x, z - e);
    Vec3::new(-dx, 2.0 * e, -dz).normalize()
}

pub struct Obstacle {
    pub center: Vec2,
    pub radius: f32,
    pub top: f32,
}

/// Circular obstacles (tree trunks, rocks, beacons) that block anything below their top.
#[derive(Resource, Default)]
pub struct Obstacles(pub Vec<Obstacle>);

impl Obstacles {
    /// Resolves overlaps and returns the normal of the last obstacle touched.
    pub fn push_out(&self, pos: &mut Vec3, radius: f32) -> Option<Vec2> {
        let mut p = pos.xz();
        let mut normal = None;
        for o in self.0.iter().filter(|o| pos.y - radius < o.top) {
            let (c, r) = (&o.center, o.radius);
            let d = p - *c;
            let min = r + radius;
            let l2 = d.length_squared();
            if l2 < min * min && l2 > 1e-6 {
                let n = d / l2.sqrt();
                p = *c + n * min;
                normal = Some(n);
            }
        }
        pos.x = p.x;
        pos.z = p.y;
        normal
    }
}

pub fn srgb(hex: &str) -> Color {
    Srgba::hex(hex).unwrap().into()
}

fn lin(c: Color) -> [f32; 4] {
    c.to_linear().to_f32_array()
}

fn shade(c: Color, k: f32) -> Color {
    let l = c.to_linear();
    Color::LinearRgba(LinearRgba::new(l.red * k, l.green * k, l.blue * k, l.alpha))
}

fn terrain_color(h: f32, n: Vec3, p: Vec2) -> Color {
    let sand = srgb("d9c08f");
    let wet = srgb("8c7a5c");
    let grass_a = srgb("4f8a4a");
    let grass_b = srgb("6d9b3e");
    let moss = srgb("3c6e4c");
    let rock = srgb("7d7a82");
    let cliff = srgb("5d5866");
    let tint = noise2(p * 0.07, 41) * 0.5 + 0.5;
    let grass = gradient(&[moss, grass_a, grass_b], tint);
    let mut c = if h < -0.4 {
        gradient(&[shade(wet, 0.55), wet], smoothstep(-6.0, -0.4, h))
    } else if h < 1.1 {
        gradient(&[wet, sand], smoothstep(-0.4, 0.3, h))
    } else {
        let g = gradient(&[sand, grass], smoothstep(1.1, 1.8, h));
        gradient(&[g, rock], smoothstep(8.0, 11.0, h))
    };
    let steep = smoothstep(0.82, 0.62, n.y);
    if h > 0.5 {
        c = gradient(&[c, cliff], steep);
    }
    c
}

pub fn flat_mesh(positions: Vec<[f32; 3]>, colors: Vec<[f32; 4]>) -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_computed_flat_normals()
}

fn spawn_terrain(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let extent = 165.0;
    let cells = 180;
    let step = extent * 2.0 / cells as f32;
    let mut rng = Rng::new(7);

    let n = cells + 1;
    let mut heights = vec![0.0; n * n];
    for j in 0..n {
        for i in 0..n {
            let x = -extent + i as f32 * step;
            let z = -extent + j as f32 * step;
            heights[j * n + i] = terrain_height(x, z);
        }
    }
    let vert = |i: usize, j: usize| {
        Vec3::new(-extent + i as f32 * step, heights[j * n + i], -extent + j as f32 * step)
    };

    let mut positions = Vec::with_capacity(cells * cells * 6);
    let mut colors = Vec::with_capacity(cells * cells * 6);
    for j in 0..cells {
        for i in 0..cells {
            let (a, b, c, d) = (vert(i, j), vert(i + 1, j), vert(i, j + 1), vert(i + 1, j + 1));
            // Skip fully submerged deep-ocean quads far from any island.
            if a.y.max(b.y).max(c.y).max(d.y) < SEA_FLOOR + 0.5 && a.xz().length() > 60.0 {
                continue;
            }
            let tris = if (i + j) % 2 == 0 { [[a, c, b], [b, c, d]] } else { [[a, c, d], [a, d, b]] };
            for t in tris {
                let normal = (t[1] - t[0]).cross(t[2] - t[0]).normalize();
                let centroid = (t[0] + t[1] + t[2]) / 3.0;
                let col = terrain_color(centroid.y, normal, centroid.xz());
                let col = lin(shade(col, 1.0 + rng.signed() * 0.06));
                for v in t {
                    positions.push(v.to_array());
                    colors.push(col);
                }
            }
        }
    }

    // A deep seabed disc so the horizon never shows a void.
    commands.spawn((
        Mesh3d(meshes.add(Circle::new(2000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: srgb("1b2a3a"),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, SEA_FLOOR + 0.2, 0.0)
            .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        NotShadowCaster,
    ));

    commands.spawn((
        Name::new("Terrain"),
        Mesh3d(meshes.add(flat_mesh(positions, colors))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.92,
            reflectance: 0.2,
            ..default()
        })),
    ));
}

#[derive(Component)]
struct Water {
    /// Per-vertex (x, z) of the undisplaced grid.
    base: Vec<Vec2>,
}

fn spawn_water(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let extent = 170.0;
    let cells = 76;
    let step = extent * 2.0 / cells as f32;
    let deep = srgb("16406a");
    let mid = srgb("1f7a96");
    let shallow = srgb("4fc4c0");
    let foam = srgb("d8f4f0");

    let mut base = Vec::new();
    let mut colors = Vec::new();
    for j in 0..cells {
        for i in 0..cells {
            let x0 = -extent + i as f32 * step;
            let z0 = -extent + j as f32 * step;
            let a = Vec2::new(x0, z0);
            let b = Vec2::new(x0 + step, z0);
            let c = Vec2::new(x0, z0 + step);
            let d = Vec2::new(x0 + step, z0 + step);
            for t in [[a, c, b], [b, c, d]] {
                let centroid = (t[0] + t[1] + t[2]) / 3.0;
                if centroid.length() > extent {
                    continue;
                }
                let depth = -terrain_height(centroid.x, centroid.y);
                let mut col = gradient(&[foam, shallow, mid, deep], smoothstep(-0.2, 9.0, depth).powf(0.6));
                let alpha = 0.55 + 0.4 * smoothstep(0.0, 6.0, depth);
                col.set_alpha(alpha);
                for v in t {
                    base.push(v);
                    colors.push(lin(col));
                }
            }
        }
    }
    let positions: Vec<[f32; 3]> = base.iter().map(|v| [v.x, 0.0, v.y]).collect();
    let mesh = flat_mesh(positions, colors);

    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.12,
        reflectance: 0.7,
        ..default()
    });
    commands.spawn((
        Name::new("Water"),
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material.clone()),
        Water { base },
        NotShadowCaster,
    ));

    // Calm outer ocean ring out to the horizon.
    commands.spawn((
        Mesh3d(meshes.add(Annulus::new(extent - 4.0, 2000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: deep.with_alpha(0.97),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.15,
            reflectance: 0.6,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.05, 0.0)
            .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        NotShadowCaster,
        NotShadowReceiver,
    ));
}

fn animate_water(time: Res<Time>, water: Query<(&Water, &Mesh3d)>, mut meshes: ResMut<Assets<Mesh>>) {
    let t = time.elapsed_secs();
    for (water, handle) in &water {
        let Some(mut mesh) = meshes.get_mut(handle) else { continue };
        let mut positions = Vec::with_capacity(water.base.len());
        let mut normals = Vec::with_capacity(water.base.len());
        for tri in water.base.chunks_exact(3) {
            let v: [Vec3; 3] = std::array::from_fn(|k| {
                let b = tri[k];
                Vec3::new(b.x, water_height(b.x, b.y, t), b.y)
            });
            let n = (v[1] - v[0]).cross(v[2] - v[0]).normalize();
            for p in v {
                positions.push(p.to_array());
                normals.push(n.to_array());
            }
        }
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    }
}

/// Builds a flat-shaded, vertex-colored part ready to be merged into a prop mesh.
pub fn part(mesh: Mesh, color: Color, transform: Transform) -> Mesh {
    let mut mesh = mesh;
    if mesh.indices().is_some() {
        mesh.duplicate_vertices();
    }
    mesh.remove_attribute(Mesh::ATTRIBUTE_UV_0);
    mesh.remove_attribute(Mesh::ATTRIBUTE_NORMAL);
    let count = mesh.count_vertices();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![lin(color); count]);
    mesh.transformed_by(transform).with_computed_flat_normals()
}

pub fn combine(parts: Vec<Mesh>) -> Mesh {
    let mut iter = parts.into_iter();
    let mut base = iter.next().unwrap();
    for p in iter {
        base.merge(&p).unwrap();
    }
    base
}

pub fn jitter(mut mesh: Mesh, rng: &mut Rng, amount: f32) -> Mesh {
    if let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
        // Displace by position-hash so shared vertices stay welded.
        let seed = rng.next_u32();
        for p in pos.iter_mut() {
            let v = Vec3::from(*p);
            let k = noise2(Vec2::new(v.x * 3.1 + v.y * 1.7, v.z * 2.9 - v.y * 1.3), seed);
            *p = (v * (1.0 + k * amount)).to_array();
        }
    }
    mesh
}

pub fn ico(sub: u32) -> Mesh {
    Sphere::new(1.0).mesh().ico(sub).unwrap()
}

fn pine(rng: &mut Rng) -> Mesh {
    let trunk = srgb("5a3d2b");
    let greens = [srgb("1f5e4a"), srgb("24674f"), srgb("2d7556"), srgb("1b4f45")];
    let mut parts = vec![part(
        Cylinder::new(0.22, 1.8).mesh().resolution(6).build(),
        trunk,
        Transform::from_xyz(0.0, 0.9, 0.0),
    )];
    let tiers = 3 + (rng.f32() * 2.0) as usize;
    let mut y = 1.2;
    let mut r = 1.7;
    for _ in 0..tiers {
        let h = r * 1.5;
        parts.push(part(
            Cone::new(r, h).mesh().resolution(7).build(),
            *rng.pick(&greens),
            Transform::from_xyz(0.0, y + h * 0.5, 0.0)
                .with_rotation(Quat::from_rotation_y(rng.range(0.0, 6.28))),
        ));
        y += h * 0.45;
        r *= 0.74;
    }
    combine(parts)
}

fn round_tree(rng: &mut Rng) -> Mesh {
    let trunk = srgb("6b4a35");
    let palettes = [
        [srgb("3f8f4f"), srgb("4f9e52")],
        [srgb("2f7a5a"), srgb("3a8a64")],
        [srgb("c76b8a"), srgb("d985a0")],
        [srgb("d9a03f"), srgb("e3b452")],
    ];
    let pal = rng.pick(&palettes);
    let mut parts = vec![part(
        Cylinder::new(0.2, 2.4).mesh().resolution(6).build(),
        trunk,
        Transform::from_xyz(0.0, 1.2, 0.0),
    )];
    let blobs = 2 + (rng.f32() * 2.0) as usize;
    for k in 0..blobs {
        let r = rng.range(0.9, 1.5);
        let offset = Vec3::new(rng.signed() * 0.8, 2.6 + k as f32 * 0.5 + rng.f32() * 0.4, rng.signed() * 0.8);
        parts.push(part(
            jitter(ico(1), rng, 0.25),
            pal[k % 2],
            Transform::from_translation(offset).with_scale(Vec3::splat(r)),
        ));
    }
    combine(parts)
}

fn rock(rng: &mut Rng) -> Mesh {
    let greys = [srgb("85818c"), srgb("6f6b78"), srgb("9a958f")];
    part(
        jitter(ico(1), rng, 0.45),
        *rng.pick(&greys),
        Transform::from_scale(Vec3::new(rng.range(0.8, 1.4), rng.range(0.5, 0.9), rng.range(0.8, 1.4))),
    )
}

fn bush(rng: &mut Rng) -> Mesh {
    let greens = [srgb("3d7d45"), srgb("4a8c3f"), srgb("356e50")];
    let c = *rng.pick(&greens);
    combine(vec![
        part(jitter(ico(1), rng, 0.3), c, Transform::from_scale(Vec3::new(1.0, 0.7, 1.0))),
        part(
            jitter(ico(1), rng, 0.3),
            shade(c, 1.15),
            Transform::from_xyz(0.6, 0.1, 0.3).with_scale(Vec3::splat(0.6)),
        ),
    ])
}

#[derive(Component)]
pub struct Sway {
    phase: f32,
    strength: f32,
    base: Quat,
}

#[derive(Component)]
struct Firefly {
    anchor: Vec3,
    phase: f32,
    speed: f32,
}

/// Picks a random land point within the given height band, away from beacons and spawn.
pub fn random_land_point(rng: &mut Rng, min_h: f32, max_h: f32, max_slope: f32) -> Option<Vec3> {
    for _ in 0..60 {
        let x = rng.range(-150.0, 150.0);
        let z = rng.range(-150.0, 150.0);
        let h = terrain_height(x, z);
        if h < min_h || h > max_h {
            continue;
        }
        if terrain_normal(x, z).y < max_slope {
            continue;
        }
        let p = Vec2::new(x, z);
        if BEACON_SITES.iter().any(|s| s.distance(p) < PLATEAU_OUTER + 1.0) || p.distance(PLAYER_SPAWN) < 6.0 {
            continue;
        }
        return Some(Vec3::new(x, h, z));
    }
    None
}

fn spawn_props(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut rng = Rng::new(1234);
    let mut obstacles = Obstacles(
        BEACON_SITES
            .iter()
            .enumerate()
            .map(|(i, s)| Obstacle { center: *s, radius: 3.4, top: site_height(i) + 5.0 })
            .collect(),
    );
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.85,
        ..default()
    });

    let pines: Vec<Handle<Mesh>> = (0..6).map(|_| meshes.add(pine(&mut rng))).collect();
    let rounds: Vec<Handle<Mesh>> = (0..8).map(|_| meshes.add(round_tree(&mut rng))).collect();
    let rocks: Vec<Handle<Mesh>> = (0..6).map(|_| meshes.add(rock(&mut rng))).collect();
    let bushes: Vec<Handle<Mesh>> = (0..5).map(|_| meshes.add(bush(&mut rng))).collect();

    // Trees cluster in groves driven by a low-frequency noise mask.
    let mut placed = 0;
    while placed < 300 {
        let Some(p) = random_land_point(&mut rng, 1.4, 12.0, 0.8) else { continue };
        let grove = fbm(p.xz() * 0.025, 2, 91);
        if grove < 0.05 && rng.f32() > 0.1 {
            continue;
        }
        placed += 1;
        let is_pine = p.y > 5.0 || rng.f32() < 0.45;
        let mesh = if is_pine { rng.pick(&pines) } else { rng.pick(&rounds) };
        let scale = rng.range(0.8, 1.35);
        let rot = Quat::from_rotation_y(rng.range(0.0, 6.28));
        obstacles.0.push(Obstacle { center: p.xz(), radius: 0.45 * scale, top: p.y + 5.0 * scale });
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(p - Vec3::Y * 0.2).with_rotation(rot).with_scale(Vec3::splat(scale)),
            Sway { phase: rng.range(0.0, 6.28), strength: 0.025, base: rot },
        ));
    }

    for _ in 0..140 {
        let Some(p) = random_land_point(&mut rng, -1.5, 14.0, 0.5) else { continue };
        let scale = rng.range(0.5, 2.2);
        if scale > 1.2 {
            obstacles.0.push(Obstacle { center: p.xz(), radius: scale * 0.9, top: p.y + scale * 0.8 });
        }
        commands.spawn((
            Mesh3d(rng.pick(&rocks).clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(p).with_scale(Vec3::splat(scale))
                .with_rotation(Quat::from_rotation_y(rng.range(0.0, 6.28))),
        ));
    }

    for _ in 0..220 {
        let Some(p) = random_land_point(&mut rng, 1.3, 11.0, 0.75) else { continue };
        let rot = Quat::from_rotation_y(rng.range(0.0, 6.28));
        commands.spawn((
            Mesh3d(rng.pick(&bushes).clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(p).with_rotation(rot).with_scale(Vec3::splat(rng.range(0.5, 1.0))),
            Sway { phase: rng.range(0.0, 6.28), strength: 0.04, base: rot },
        ));
    }

    // Bioluminescent mushrooms: the night's own little lanterns.
    let stem = meshes.add(part(
        Cylinder::new(0.06, 0.4).mesh().resolution(5).build(),
        srgb("e8e0d0"),
        Transform::from_xyz(0.0, 0.2, 0.0),
    ));
    let cap = meshes.add(
        Sphere::new(0.22).mesh().uv(8, 4).with_duplicated_vertices().with_computed_flat_normals(),
    );
    let glow_mats: Vec<Handle<StandardMaterial>> = [
        LinearRgba::rgb(0.2, 3.0, 2.6),
        LinearRgba::rgb(2.6, 0.5, 3.0),
        LinearRgba::rgb(0.6, 1.4, 4.0),
    ]
    .into_iter()
    .map(|e| {
        materials.add(StandardMaterial {
            base_color: Color::LinearRgba(e * 0.25),
            emissive: emit(e.red, e.green, e.blue),
            ..default()
        })
    })
    .collect();
    for _ in 0..90 {
        let Some(center) = random_land_point(&mut rng, 1.5, 10.0, 0.75) else { continue };
        let glow = rng.pick(&glow_mats).clone();
        for _ in 0..(3 + rng.next_u32() % 4) {
            let off = Vec2::new(rng.signed(), rng.signed()) * 1.2;
            let x = center.x + off.x;
            let z = center.z + off.y;
            let y = terrain_height(x, z);
            let s = rng.range(0.6, 1.5);
            commands
                .spawn((
                    Mesh3d(stem.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform::from_xyz(x, y - 0.05, z).with_scale(Vec3::splat(s)),
                    NotShadowCaster,
                ))
                .with_child((
                    Mesh3d(cap.clone()),
                    MeshMaterial3d(glow.clone()),
                    Transform::from_xyz(0.0, 0.42, 0.0).with_scale(Vec3::new(1.0, 0.55, 1.0)),
                    NotShadowCaster,
                ));
        }
    }

    let firefly_mesh = meshes.add(Sphere::new(0.06).mesh().ico(0).unwrap());
    let firefly_mat = materials.add(StandardMaterial {
        base_color: hdr(14.0, 12.0, 3.0),
        unlit: true,
        ..default()
    });
    for _ in 0..260 {
        let Some(p) = random_land_point(&mut rng, 0.5, 12.0, 0.0) else { continue };
        commands.spawn((
            Mesh3d(firefly_mesh.clone()),
            MeshMaterial3d(firefly_mat.clone()),
            Transform::from_translation(p + Vec3::Y * 1.5),
            Firefly { anchor: p + Vec3::Y * rng.range(0.8, 3.5), phase: rng.range(0.0, 100.0), speed: rng.range(0.3, 0.8) },
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }

    commands.insert_resource(obstacles);
}

fn sway(time: Res<Time>, mut q: Query<(&mut Transform, &Sway)>) {
    let t = time.elapsed_secs();
    for (mut tf, s) in &mut q {
        let gust = 0.6 + 0.4 * (t * 0.3 + tf.translation.x * 0.02).sin();
        let ax = (t * 1.3 + s.phase).sin() * s.strength * gust;
        let az = (t * 1.7 + s.phase * 1.3).sin() * s.strength * 0.7 * gust;
        tf.rotation = Quat::from_rotation_x(ax) * Quat::from_rotation_z(az) * s.base;
    }
}

fn fireflies(time: Res<Time>, mut q: Query<(&mut Transform, &Firefly)>) {
    let t = time.elapsed_secs();
    for (mut tf, f) in &mut q {
        let s = t * f.speed + f.phase;
        let target = f.anchor
            + Vec3::new(
                noise2(Vec2::new(s, 0.0), 1) * 3.0,
                noise2(Vec2::new(s, 5.0), 2) * 1.2,
                noise2(Vec2::new(s, 9.0), 3) * 3.0,
            );
        tf.translation = target;
        let blink = ((t * 2.0 + f.phase).sin() * 0.5 + 0.5).powf(3.0);
        tf.scale = Vec3::splat(0.3 + blink * 1.2);
    }
}
