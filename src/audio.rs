//! Every sound is synthesized at startup, so the game ships without asset files.

use std::{f32::consts::TAU, sync::Arc};

use bevy::{
    audio::{AudioSource, PlaybackMode, Volume},
    prelude::*,
};

use crate::{GameState, util::Rng};

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Sfx>()
            .add_systems(Startup, (synthesize, start_ambience).chain())
            .add_systems(OnEnter(GameState::Victory), |mut sfx: MessageWriter<Sfx>| {
                sfx.write(Sfx::new(SfxKind::Victory));
            })
            .add_systems(Update, play);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SfxKind {
    Pickup,
    Deposit,
    Ignite,
    Hit,
    Dash,
    Jump,
    Land,
    Growl,
    Victory,
    Start,
}

#[derive(Message)]
pub struct Sfx {
    kind: SfxKind,
    pitch: f32,
}

impl Sfx {
    pub fn new(kind: SfxKind) -> Self {
        Self { kind, pitch: 1.0 }
    }

    pub fn pitched(kind: SfxKind, pitch: f32) -> Self {
        Self { kind, pitch }
    }
}

#[derive(Resource)]
struct Sounds(Vec<(SfxKind, Handle<AudioSource>, f32)>);

const RATE: u32 = 44_100;

fn wav(samples: &[f32]) -> AudioSource {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32000.0) as i16).to_le_bytes());
    }
    AudioSource { bytes: Arc::from(b) }
}

fn render(secs: f32, mut f: impl FnMut(f32) -> f32) -> Vec<f32> {
    let n = (secs * RATE as f32) as usize;
    let mut out: Vec<f32> = (0..n).map(|i| f(i as f32 / RATE as f32)).collect();
    // Short fades kill clicks at both ends.
    let fade = (RATE as f32 * 0.004) as usize;
    for i in 0..fade.min(n) {
        let k = i as f32 / fade as f32;
        out[i] *= k;
        out[n - 1 - i] *= k;
    }
    out
}

fn sine(f: f32, t: f32) -> f32 {
    (TAU * f * t).sin()
}

/// A bell: decaying partials with slightly inharmonic ratios.
fn bell(f: f32, t: f32, decay: f32) -> f32 {
    let e = (-t * decay).exp();
    let attack = (t / 0.004).min(1.0);
    attack
        * e
        * (sine(f, t) + 0.5 * sine(f * 2.0, t) * (-t * decay * 1.5).exp() + 0.25 * sine(f * 2.76, t) * (-t * decay * 2.5).exp()
            + 0.12 * sine(f * 5.4, t) * (-t * decay * 4.0).exp())
}

struct Noise {
    rng: Rng,
    lp: f32,
}

impl Noise {
    fn new(seed: u64) -> Self {
        Self { rng: Rng::new(seed), lp: 0.0 }
    }

    /// White noise through a one-pole lowpass; `k` near 0 is darker.
    fn low(&mut self, k: f32) -> f32 {
        let w = self.rng.signed();
        self.lp += (w - self.lp) * k;
        self.lp
    }
}

fn synthesize(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut sounds = Vec::new();
    let mut add = |kind, samples: Vec<f32>, vol: f32| {
        sounds.push((kind, sources.add(wav(&samples)), vol));
    };

    add(SfxKind::Pickup, render(0.9, |t| 0.4 * bell(1046.5, t, 5.0) + 0.2 * bell(1568.0, t, 7.0)), 0.5);

    add(SfxKind::Deposit, render(1.4, |t| 0.5 * bell(523.25, t, 3.0) + 0.15 * bell(784.0, t, 4.0)), 0.5);

    let mut n = Noise::new(3);
    add(
        SfxKind::Ignite,
        render(4.5, |t| {
            let boom_f = 40.0 + 60.0 * (-t * 4.0).exp();
            let boom = (TAU * boom_f * t).sin() * (-t * 2.5).exp() * 0.9;
            let crackle = n.low(0.25) * (-t * 5.0).exp() * 0.6;
            let swell = (t / 0.35).min(1.0) * (-((t - 0.35).max(0.0)) * 0.9).exp();
            let vib = 1.0 + 0.003 * (TAU * 5.0 * t).sin();
            let chord = [261.63, 329.63, 392.0, 523.25, 659.25]
                .iter()
                .enumerate()
                .map(|(i, f)| sine(f * vib, t) / (1.0 + i as f32 * 0.4))
                .sum::<f32>()
                * swell
                * 0.22;
            boom + crackle + chord
        }),
        0.8,
    );

    let mut n = Noise::new(4);
    add(
        SfxKind::Hit,
        render(0.8, |t| {
            let saw = |f: f32| 2.0 * ((f * t).fract()) - 1.0;
            let e = (-t * 5.0).exp();
            (saw(98.0) + saw(103.8) + 0.6 * saw(146.0)) * 0.25 * e + n.low(0.3) * e * 0.4
        }),
        0.55,
    );

    let mut n = Noise::new(5);
    add(
        SfxKind::Dash,
        render(0.5, |t| {
            let env = (t / 0.04).min(1.0) * (-t * 7.0).exp();
            let k = 0.05 + 0.5 * (-t * 6.0).exp();
            n.low(k) * env * 1.6
        }),
        0.45,
    );

    add(
        SfxKind::Jump,
        render(0.25, |t| {
            let f = 320.0 + 900.0 * t;
            (TAU * f * t).sin() * (-t * 14.0).exp() * 0.5
        }),
        0.3,
    );

    let mut n = Noise::new(6);
    add(SfxKind::Land, render(0.25, |t| n.low(0.08) * (-t * 18.0).exp() * 1.5), 0.35);

    let mut n = Noise::new(7);
    add(
        SfxKind::Growl,
        render(1.0, |t| {
            let env = (t / 0.2).min(1.0) * (-t * 2.5).exp();
            let wob = 1.0 + 0.04 * (TAU * 7.0 * t).sin();
            (sine(73.0 * wob, t) + 0.5 * sine(110.0 * wob, t) + 0.3 * sine(155.6, t)) * env * 0.35 + n.low(0.05) * env * 0.5
        }),
        0.35,
    );

    add(
        SfxKind::Victory,
        render(6.0, |t| {
            let notes = [523.25, 659.25, 783.99, 1046.5, 1318.5, 1568.0];
            notes
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let start = i as f32 * 0.28;
                    if t < start { 0.0 } else { bell(*f, t - start, 1.2) * 0.22 }
                })
                .sum::<f32>()
                + [130.81, 196.0, 261.63]
                    .iter()
                    .map(|f| sine(*f, t) * 0.12 * (t / 1.5).min(1.0) * (-(t - 3.5).max(0.0)).exp())
                    .sum::<f32>()
        }),
        0.6,
    );

    add(SfxKind::Start, render(2.0, |t| 0.3 * bell(392.0, t, 2.0) + 0.3 * bell(587.33, (t - 0.15).max(0.0), 2.0)), 0.5);

    commands.insert_resource(Sounds(sounds));
}

#[derive(Component)]
struct Ambience;

fn start_ambience(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    // 16 s loop; every periodic component completes whole cycles so the seam is silent.
    let len = 16.0;
    let mut n = Noise::new(11);
    let mut surf = Noise::new(12);
    let pad = [110.0, 165.0, 220.0, 275.0, 330.0];
    let raw = render(len + 1.0, |t| {
        let swell = 0.55 + 0.45 * (TAU * t / 8.0).sin();
        let ocean = n.low(0.02) * 1.8 * swell + surf.low(0.15) * 0.12 * swell * swell;
        let drone = pad
            .iter()
            .enumerate()
            .map(|(i, f)| sine(*f, t) * (0.6 + 0.4 * (TAU * t * (i as f32 + 1.0) / len).sin()) / (1.0 + i as f32))
            .sum::<f32>()
            * 0.05;
        ocean * 0.5 + drone
    });
    // Crossfade the extra second back over the start for a seamless loop.
    let n_loop = (len * RATE as f32) as usize;
    let tail = raw.len() - n_loop;
    let mut out = raw[..n_loop].to_vec();
    for i in 0..tail {
        let k = i as f32 / tail as f32;
        out[i] = out[i] * k + raw[n_loop + i] * (1.0 - k);
    }
    commands.spawn((
        Ambience,
        AudioPlayer(sources.add(wav(&out))),
        PlaybackSettings { mode: PlaybackMode::Loop, volume: Volume::Linear(0.5), ..default() },
    ));
}

fn play(mut commands: Commands, mut events: MessageReader<Sfx>, sounds: Res<Sounds>) {
    for ev in events.read() {
        if let Some((_, handle, vol)) = sounds.0.iter().find(|(k, _, _)| *k == ev.kind) {
            commands.spawn((
                AudioPlayer(handle.clone()),
                PlaybackSettings::DESPAWN.with_speed(ev.pitch).with_volume(Volume::Linear(*vol)),
            ));
        }
    }
}
