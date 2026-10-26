use bevy::{prelude::*, ui::UiTransform};

use crate::{
    BEACON_COUNT, EMBERS_PER_BEACON, GameState, Progress,
    audio::{Sfx, SfxKind},
    beacon::{Beacon, BeaconLit, FEED_RADIUS},
    ember::Carried,
    player::Player,
    util::{damp, ease_out_back},
};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Groups>()
            .add_systems(Startup, (spawn_ui, debug_skip_title))
            .add_systems(Update, (start_game.run_if(in_state(GameState::Title)), set_group_targets))
            .add_systems(
                Update,
                (fade_groups, lift_curtain, title_pulse, hud_text, beacon_pips, hint, toast_on_lit, animate_toast, victory_text),
            );
    }
}

/// Fade groups; every text/background tagged with a group follows its alpha.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Group {
    Title,
    Hud,
    Victory,
    Curtain,
}

#[derive(Resource)]
struct Groups {
    alpha: [f32; 4],
    target: [f32; 4],
}

impl Default for Groups {
    fn default() -> Self {
        Self { alpha: [0.0, 0.0, 0.0, 1.0], target: [1.0, 0.0, 0.0, 0.0] }
    }
}

#[derive(Component)]
struct Fades {
    group: Group,
    base: f32,
}

#[derive(Component)]
struct TitlePrompt;

#[derive(Component)]
struct EmberCount;

#[derive(Component)]
struct EmberIcon;

#[derive(Component)]
struct BeaconCount;

#[derive(Component)]
struct BeaconPip(usize);

#[derive(Component)]
struct Hint;

#[derive(Component)]
struct Toast {
    t: f32,
}

#[derive(Component)]
struct ToastSub;

#[derive(Component)]
struct VictoryStats;

#[derive(Component)]
struct Curtain;

const GOLD: Color = Color::srgb(1.0, 0.78, 0.42);
const PALE: Color = Color::srgb(0.92, 0.9, 1.0);
const DIM: Color = Color::srgb(0.65, 0.66, 0.85);

fn text(s: &str, size: f32, color: Color, group: Group) -> impl Bundle {
    (
        Text::new(s),
        TextFont { font_size: FontSize::Px(size), ..default() },
        TextColor(color.with_alpha(0.0)),
        TextLayout::justify(Justify::Center),
        Fades { group, base: color.alpha() },
    )
}

fn spawn_ui(mut commands: Commands) {
    // Title.
    commands
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(18),
            ..default()
        })
        .with_children(|p| {
            p.spawn((text("EMBER ISLES", 104.0, GOLD, Group::Title), TextShadow::default()));
            p.spawn(text(
                "The isles have forgotten the sun.\nGather embers, kindle the five beacons, and call back the dawn.",
                22.0,
                PALE,
                Group::Title,
            ));
            p.spawn(Node { height: px(40), ..default() });
            p.spawn((text("press ENTER or click to begin", 24.0, GOLD, Group::Title), TitlePrompt));
        });
    commands
        .spawn(Node {
            width: percent(100),
            position_type: PositionType::Absolute,
            bottom: px(36),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_child(text(
            "WASD move  |  SPACE jump, double jump, hold to glide  |  SHIFT dash  |  MOUSE / Q E look  |  SCROLL zoom",
            16.0,
            DIM,
            Group::Title,
        ));

    // HUD: ember counter, top left.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: px(28),
            left: px(36),
            align_items: AlignItems::Center,
            column_gap: px(16),
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                EmberIcon,
                Node {
                    width: px(22),
                    height: px(22),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(GOLD.with_alpha(0.0)),
                BoxShadow::new(GOLD.with_alpha(0.0), px(0), px(0), px(2), px(14)),
                UiTransform { rotation: Rot2::degrees(45.0), ..default() },
                Fades { group: Group::Hud, base: 1.0 },
            ));
            p.spawn((text("0", 40.0, GOLD, Group::Hud), EmberCount));
            p.spawn(text("embers", 18.0, DIM, Group::Hud));
        });

    // HUD: beacons, bottom centre.
    commands
        .spawn(Node {
            width: percent(100),
            position_type: PositionType::Absolute,
            bottom: px(30),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(12),
            ..default()
        })
        .with_children(|p| {
            p.spawn((text("", 18.0, PALE, Group::Hud), Hint));
            p.spawn(Node { column_gap: px(22), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                for i in 0..BEACON_COUNT {
                    row.spawn((
                        BeaconPip(i),
                        Node {
                            width: px(16),
                            height: px(16),
                            border: UiRect::all(px(2)),
                            border_radius: BorderRadius::all(px(3)),
                            ..default()
                        },
                        BorderColor::all(DIM.with_alpha(0.0)),
                        BackgroundColor(Color::NONE),
                        BoxShadow::new(Color::NONE, px(0), px(0), px(1), px(12)),
                        UiTransform { rotation: Rot2::degrees(45.0), ..default() },
                    ));
                }
            });
            p.spawn((text("0 / 5 beacons kindled", 15.0, DIM, Group::Hud), BeaconCount));
        });

    // Toast, upper centre.
    commands
        .spawn(Node {
            width: percent(100),
            position_type: PositionType::Absolute,
            top: percent(16),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(8),
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                TextFont { font_size: FontSize::Px(54.0), ..default() },
                TextColor(GOLD.with_alpha(0.0)),
                TextShadow::default(),
                Toast { t: 99.0 },
                UiTransform::default(),
            ));
            p.spawn((
                Text::new(""),
                TextFont { font_size: FontSize::Px(20.0), ..default() },
                TextColor(PALE.with_alpha(0.0)),
                ToastSub,
            ));
        });

    // Victory.
    commands
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(16),
            ..default()
        })
        .with_children(|p| {
            p.spawn((text("DAWN RETURNS", 92.0, Color::srgb(1.0, 0.92, 0.75), Group::Victory), TextShadow::default()));
            p.spawn((text("", 22.0, Color::srgb(1.0, 0.95, 0.9), Group::Victory), VictoryStats, TextShadow::default()));
            p.spawn(Node { height: px(30), ..default() });
            p.spawn((
                text("R to begin anew  |  or wander the morning isles", 18.0, Color::srgb(1.0, 0.95, 0.9), Group::Victory),
                TextShadow::default(),
            ));
        });

    // Curtain: fades the world in from black.
    commands.spawn((
        Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, ..default() },
        BackgroundColor(Color::BLACK),
        Fades { group: Group::Curtain, base: 1.0 },
        Curtain,
        GlobalZIndex(10),
    ));
}

fn debug_skip_title(mut next: ResMut<NextState<GameState>>) {
    if matches!(crate::debug_state().as_deref(), Some("play")) {
        next.set(GameState::Playing);
    }
}

fn start_game(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut next: ResMut<NextState<GameState>>,
    mut sfx: MessageWriter<Sfx>,
) {
    if time.elapsed_secs() < 1.0 {
        return;
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || mouse.just_pressed(MouseButton::Left) {
        sfx.write(Sfx::new(SfxKind::Start));
        next.set(GameState::Playing);
    }
}

fn set_group_targets(state: Res<State<GameState>>, mut groups: ResMut<Groups>) {
    let s = *state.get();
    groups.target[Group::Title as usize] = (s == GameState::Title) as u8 as f32;
    groups.target[Group::Hud as usize] = (s == GameState::Playing) as u8 as f32;
    groups.target[Group::Victory as usize] = (s == GameState::Victory) as u8 as f32;
    groups.target[Group::Curtain as usize] = 0.0;
}

fn fade_groups(
    time: Res<Time>,
    mut groups: ResMut<Groups>,
    mut texts: Query<(&Fades, &mut TextColor)>,
    mut bgs: Query<(&Fades, &mut BackgroundColor), Without<TextColor>>,
    mut shadows: Query<(&Fades, &mut BoxShadow)>,
    mut text_shadows: Query<(&Fades, &mut TextShadow)>,
) {
    let dt = time.delta_secs();
    for i in 0..4 {
        let rate = if i == Group::Curtain as usize { 0.9 } else { 3.0 };
        let (a, tgt) = (groups.alpha[i], groups.target[i]);
        groups.alpha[i] = a + (tgt - a) * damp(rate, dt);
    }
    for (f, mut c) in &mut texts {
        c.0.set_alpha(f.base * groups.alpha[f.group as usize]);
    }
    for (f, mut c) in &mut bgs {
        c.0.set_alpha(f.base * groups.alpha[f.group as usize]);
    }
    for (f, mut s) in &mut text_shadows {
        s.color.set_alpha(0.75 * groups.alpha[f.group as usize]);
    }
    for (f, mut s) in &mut shadows {
        for st in s.0.iter_mut() {
            st.color.set_alpha(0.9 * groups.alpha[f.group as usize]);
        }
    }
}

/// A transparent full-screen node still costs a blend per pixel; drop it once faded.
fn lift_curtain(mut commands: Commands, groups: Res<Groups>, curtain: Query<Entity, With<Curtain>>) {
    if groups.alpha[Group::Curtain as usize] < 0.005 {
        for e in &curtain {
            commands.entity(e).despawn();
        }
    }
}

fn title_pulse(time: Res<Time>, groups: Res<Groups>, mut q: Query<&mut TextColor, With<TitlePrompt>>) {
    let k = 0.45 + 0.55 * (time.elapsed_secs() * 2.4).sin().abs();
    for mut c in &mut q {
        c.0.set_alpha(k * groups.alpha[Group::Title as usize]);
    }
}

fn hud_text(
    time: Res<Time>,
    carried: Res<Carried>,
    progress: Res<Progress>,
    mut count: Single<&mut Text, (With<EmberCount>, Without<BeaconCount>)>,
    mut beacons: Single<&mut Text, (With<BeaconCount>, Without<EmberCount>)>,
    mut icon: Single<&mut UiTransform, With<EmberIcon>>,
    mut last: Local<u32>,
    mut pop: Local<f32>,
) {
    if carried.0 != *last {
        if carried.0 > *last {
            *pop = 0.0;
        }
        *last = carried.0;
    }
    *pop += time.delta_secs();
    count.0 = carried.0.to_string();
    beacons.0 = format!("{} / {} beacons kindled", progress.lit, BEACON_COUNT);
    let s = 1.0 + 0.6 * (1.0 - ease_out_back((*pop / 0.35).min(1.0)));
    icon.scale = Vec2::splat(s);
    icon.rotation = Rot2::degrees(45.0 + time.elapsed_secs() * 20.0);
}

fn beacon_pips(
    time: Res<Time>,
    groups: Res<Groups>,
    progress: Res<Progress>,
    mut pips: Query<(&BeaconPip, &mut BackgroundColor, &mut BorderColor, &mut BoxShadow, &mut UiTransform)>,
) {
    let a = groups.alpha[Group::Hud as usize];
    let t = time.elapsed_secs();
    for (pip, mut bg, mut border, mut shadow, mut tf) in &mut pips {
        let lit = pip.0 < progress.lit;
        if lit {
            bg.0 = GOLD.with_alpha(a);
            *border = BorderColor::all(GOLD.with_alpha(a));
            for s in shadow.0.iter_mut() {
                s.color = GOLD.with_alpha(0.8 * a);
            }
            tf.scale = Vec2::splat(1.0 + 0.08 * (t * 3.0 + pip.0 as f32).sin());
        } else {
            bg.0 = Color::NONE;
            *border = BorderColor::all(DIM.with_alpha(0.7 * a));
            for s in shadow.0.iter_mut() {
                s.color = Color::NONE;
            }
            tf.scale = Vec2::ONE;
        }
    }
}

fn hint(
    carried: Res<Carried>,
    progress: Res<Progress>,
    player: Single<&Transform, With<Player>>,
    beacons: Query<&Beacon>,
    mut q: Single<&mut Text, With<Hint>>,
) {
    let near = beacons
        .iter()
        .filter(|b| !b.lit)
        .find(|b| b.base.xz().distance(player.translation.xz()) < FEED_RADIUS + 6.0);
    let msg = if let Some(b) = near {
        let need = EMBERS_PER_BEACON - b.fuel - b.incoming;
        if need == 0 {
            "The flame takes hold...".to_string()
        } else if carried.0 == 0 {
            format!("This beacon hungers for {need} more ember{}", if need == 1 { "" } else { "s" })
        } else {
            "Offering embers...".to_string()
        }
    } else if progress.embers_gathered == 0 {
        "Seek the golden glimmers scattered across the isles".to_string()
    } else if carried.0 >= EMBERS_PER_BEACON {
        "Carry your embers to a pale blue beacon".to_string()
    } else if progress.times_caught > 0 && progress.lit == 0 && carried.0 == 0 {
        "Beware the shadow wisps -- dash to slip past them".to_string()
    } else {
        String::new()
    };
    if q.0 != msg {
        q.0 = msg;
    }
}

fn toast_on_lit(
    mut lit: MessageReader<BeaconLit>,
    progress: Res<Progress>,
    mut toast: Single<(&mut Text, &mut Toast), Without<ToastSub>>,
    mut sub: Single<&mut Text, With<ToastSub>>,
) {
    for _ in lit.read() {
        let (text, t) = &mut *toast;
        text.0 = "BEACON KINDLED".into();
        t.t = 0.0;
        let left = BEACON_COUNT - progress.lit;
        sub.0 = match left {
            0 => "The night breaks.".into(),
            1 => "One beacon remains in darkness.".into(),
            n => format!("{} beacons remain in darkness.", ["", "", "Two", "Three", "Four"][n]),
        };
    }
}

fn animate_toast(
    time: Res<Time>,
    mut toast: Single<(&mut Toast, &mut TextColor, &mut TextShadow, &mut UiTransform), Without<ToastSub>>,
    mut sub: Single<&mut TextColor, With<ToastSub>>,
) {
    let (t, color, shadow, tf) = &mut *toast;
    t.t += time.delta_secs();
    let k = t.t;
    let alpha = if k < 0.3 { k / 0.3 } else { (1.0 - (k - 2.6) / 0.8).clamp(0.0, 1.0) };
    color.0.set_alpha(alpha);
    shadow.color.set_alpha(alpha * 0.75);
    sub.0.set_alpha(alpha * 0.9);
    tf.scale = Vec2::splat(0.6 + 0.4 * ease_out_back((k / 0.5).min(1.0)));
}

fn victory_text(progress: Res<Progress>, mut q: Single<&mut Text, With<VictoryStats>>) {
    let secs = progress.elapsed as u32;
    q.0 = format!(
        "Kindled in {}:{:02}   |   {} embers gathered   |   caught by shadows {} time{}",
        secs / 60,
        secs % 60,
        progress.embers_gathered,
        progress.times_caught,
        if progress.times_caught == 1 { "" } else { "s" }
    );
}
