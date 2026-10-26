use bevy::prelude::*;

/// Small deterministic PCG-style RNG so worlds are reproducible without extra crates.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }

    pub fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let x = ((self.0 >> 18) ^ self.0) >> 27;
        let rot = (self.0 >> 59) as u32;
        (x as u32).rotate_right(rot)
    }

    /// Uniform in [0, 1).
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    pub fn signed(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }

    pub fn unit_vec3(&mut self) -> Vec3 {
        loop {
            let v = Vec3::new(self.signed(), self.signed(), self.signed());
            let l = v.length_squared();
            if l > 0.0001 && l <= 1.0 {
                return v / l.sqrt();
            }
        }
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next_u32() as usize) % items.len()]
    }
}

fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6b343)
        ^ (y as u32).wrapping_mul(0xd8163841)
        ^ seed.wrapping_mul(0xcb1ab31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 0x00ff_ffff as f32 * 2.0 - 1.0
}

/// Smooth value noise in [-1, 1].
pub fn noise2(p: Vec2, seed: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let (x, y) = (i.x as i32, i.y as i32);
    let a = hash2(x, y, seed);
    let b = hash2(x + 1, y, seed);
    let c = hash2(x, y + 1, seed);
    let d = hash2(x + 1, y + 1, seed);
    a.lerp(b, u.x).lerp(c.lerp(d, u.x), u.y)
}

pub fn fbm(p: Vec2, octaves: u32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut freq = 1.0;
    let mut norm = 0.0;
    for o in 0..octaves {
        sum += noise2(p * freq, seed.wrapping_add(o * 101)) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / norm
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Frame-rate independent exponential approach.
pub fn damp(rate: f32, dt: f32) -> f32 {
    1.0 - (-rate * dt).exp()
}

pub fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let a = a.to_linear();
    let b = b.to_linear();
    Color::LinearRgba(LinearRgba::new(
        a.red.lerp(b.red, t),
        a.green.lerp(b.green, t),
        a.blue.lerp(b.blue, t),
        a.alpha.lerp(b.alpha, t),
    ))
}

/// Piecewise-linear gradient over evenly spaced keys.
pub fn gradient(keys: &[Color], t: f32) -> Color {
    let t = t.clamp(0.0, 1.0) * (keys.len() - 1) as f32;
    let i = (t.floor() as usize).min(keys.len() - 2);
    lerp_color(keys[i], keys[i + 1], t - i as f32)
}

pub fn ease_out_back(t: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// HDR color for unlit glowing surfaces (values above 1 feed bloom).
pub fn hdr(r: f32, g: f32, b: f32) -> Color {
    Color::LinearRgba(LinearRgba::new(r, g, b, 1.0))
}

/// Emissive that ignores camera exposure (alpha 0 opts out of exposure scaling).
pub fn emit(r: f32, g: f32, b: f32) -> LinearRgba {
    LinearRgba::new(r, g, b, 0.0)
}
