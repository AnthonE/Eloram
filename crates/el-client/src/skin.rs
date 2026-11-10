//! What the UI is made of (`docs/DESIGN.md`, "The look of the UI"): slate plates in rims of worked
//! bronze, lit from above, their free corners cut; ember gold for what's ready, picked or to be
//! pressed; a carved face for titles and a condensed one for everything read at a glance.
//!
//! The plates are drawn by a shader ([`Plate`], `shaders/plate.wgsl`): one draw for a panel's
//! slab, rim, grain and glow, where the stock nodes took a shadow, a border and a gradient. The
//! styles are a handful of fixed handles ([`plates`]), put in the assets as the plugin is built, so
//! a bundle names one without a resource in hand. A control whose colours the game's own systems
//! write (a button's tint, a frame in a tier's colour) keeps them and wears a [`finish`] over
//! them: the same shader, clear but for the light it adds. The gauges (health, mana) are
//! [`Gauge`], `shaders/gauge.wgsl`.
//!
//! The faces: Cinzel Bold (OFL) for titles, Roboto Condensed (Apache-2.0) for the rest, compiled
//! in (`crates/el-client/fonts/`).

use bevy::asset::{Asset, uuid_handle};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

/// Roboto Condensed, regular and bold: everything read at a glance.
pub const SANS: Handle<Font> = uuid_handle!("2f6e0a51-8c3b-4d6e-9a21-5b7c0e4d1a01");
pub const SANS_BOLD: Handle<Font> = uuid_handle!("2f6e0a51-8c3b-4d6e-9a21-5b7c0e4d1a02");
/// Cinzel Bold: a title, a name, a result.
pub const DISPLAY: Handle<Font> = uuid_handle!("2f6e0a51-8c3b-4d6e-9a21-5b7c0e4d1a04");

const SANS_TTF: &[u8] = include_bytes!("../fonts/RobotoCondensed-Regular.ttf");
const SANS_BOLD_TTF: &[u8] = include_bytes!("../fonts/RobotoCondensed-Bold.ttf");
const DISPLAY_TTF: &[u8] = include_bytes!("../fonts/Cinzel-Bold.ttf");

/// The palette. Slate: the plates. Bronze: their rims. Ember: what's hot.
pub const SLATE_TOP: Color = Color::srgba(0.075, 0.092, 0.130, 0.96);
pub const SLATE: Color = Color::srgba(0.028, 0.036, 0.058, 0.96);
pub const BRONZE_HI: Color = Color::srgb(0.93, 0.78, 0.47);
pub const BRONZE: Color = Color::srgb(0.58, 0.46, 0.26);
pub const BRONZE_LO: Color = Color::srgb(0.22, 0.165, 0.085);
pub const EMBER: Color = Color::srgb(1.0, 0.62, 0.20);
pub const EMBER_HI: Color = Color::srgb(1.0, 0.86, 0.52);
pub const IRON_HI: Color = Color::srgb(0.46, 0.48, 0.54);
pub const IRON_LO: Color = Color::srgb(0.12, 0.13, 0.16);

/// A plate's uniform (`shaders/plate.wgsl` says what each is).
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct PlateParams {
    pub top: Vec4,
    pub bottom: Vec4,
    pub rim_hi: Vec4,
    pub rim_lo: Vec4,
    pub glow: Vec4,
    pub cuts: Vec4,
    pub shape: Vec4,
    pub live: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct Plate {
    #[uniform(0)]
    pub p: PlateParams,
}

impl UiMaterial for Plate {
    fn fragment_shader() -> ShaderRef {
        "embedded://el_client/shaders/plate.wgsl".into()
    }
}

/// A gauge's uniform (`shaders/gauge.wgsl`).
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct GaugeParams {
    pub lo: Vec4,
    pub hi: Vec4,
    pub lost: Vec4,
    pub back: Vec4,
    /// x: the fill; y: where what was lost ends; z: a tick's share; w: the shield's end.
    pub amount: Vec4,
    /// x: the low beat; y: the light's speed; z: every which tick is heavy.
    pub live: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct Gauge {
    #[uniform(0)]
    pub g: GaugeParams,
}

impl UiMaterial for Gauge {
    fn fragment_shader() -> ShaderRef {
        "embedded://el_client/shaders/gauge.wgsl".into()
    }
}

/// The menu's embers (`shaders/sparks.wgsl`): sparks rising over the whole screen.
#[derive(Clone, Copy, Debug, ShaderType)]
pub struct SparksParams {
    pub color: Vec4,
    /// x: how fast they rise; y: how many (0 to 1); z: how far they sway.
    pub shape: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone)]
pub struct Sparks {
    #[uniform(0)]
    pub s: SparksParams,
}

impl UiMaterial for Sparks {
    fn fragment_shader() -> ShaderRef {
        "embedded://el_client/shaders/sparks.wgsl".into()
    }
}

/// The menu's sparks, one material for every rebuild of it.
pub const SPARKS: Handle<Sparks> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9b01");

/// A sheet of sparks over the whole of its parent, under what's spawned after it.
pub fn sparks() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        MaterialNode(SPARKS),
        ZIndex(-1),
    )
}

pub fn lin(c: Color) -> Vec4 {
    c.to_linear().to_vec4()
}

/// A plate's making, in the words a style is thought in.
#[derive(Clone, Copy)]
pub struct Make {
    pub top: Color,
    pub bottom: Color,
    pub rim_hi: Color,
    pub rim_lo: Color,
    /// The rim's width (px): the node's border should be as wide.
    pub rim: f32,
    /// The corners' cuts (px): top left, top right, bottom right, bottom left.
    pub cuts: [f32; 4],
    pub grain: f32,
    pub shade: f32,
    pub lip: f32,
    pub glow: Color,
    pub glow_reach: f32,
    pub breath: f32,
    pub sheen: f32,
    pub sheen_secs: f32,
}

impl Make {
    /// A slate plate in a bronze rim, uncut: what the styles start from.
    pub const SLATE: Make = Make {
        top: SLATE_TOP,
        bottom: SLATE,
        rim_hi: BRONZE_HI,
        rim_lo: BRONZE_LO,
        rim: 3.0,
        cuts: [0.0; 4],
        grain: 0.10,
        shade: 0.30,
        lip: 0.10,
        glow: Color::NONE,
        glow_reach: 10.0,
        breath: 0.0,
        sheen: 0.0,
        sheen_secs: 4.0,
    };

    pub const fn cut(mut self, tl: f32, tr: f32, br: f32, bl: f32) -> Self {
        self.cuts = [tl, tr, br, bl];
        self
    }

    pub fn plate(self) -> Plate {
        Plate {
            p: PlateParams {
                top: lin(self.top),
                bottom: lin(self.bottom),
                rim_hi: lin(self.rim_hi),
                rim_lo: lin(self.rim_lo),
                glow: lin(self.glow),
                cuts: Vec4::from_array(self.cuts),
                shape: Vec4::new(self.rim, self.grain, self.shade, self.glow_reach),
                live: Vec4::new(self.sheen, self.sheen_secs, self.breath, self.lip),
            },
        }
    }
}

/// The styles, each a fixed handle.
pub mod plates {
    use super::{Handle, Plate, uuid_handle};

    /// A panel that floats: the shop, the scoreboard, a card in the middle. Every corner cut.
    pub const PANEL: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a01");
    /// Docked along the bottom (its top corners cut), the top (its bottom ones), and in the
    /// bottom right corner (its top left).
    pub const DOCK_BOTTOM: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a02");
    pub const DOCK_TOP: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a03");
    pub const DOCK_CORNER: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a04");
    /// What floats over the map and shouldn't hide it: a thin rim, the slab half clear.
    pub const SOFT: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a05");
    /// A tooltip: dense, a thin bright rim.
    pub const TIP: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a06");
    /// An ability's frame: ready (ember, breathing), cooling down (iron), short of mana (blue),
    /// not learned (dark).
    pub const SLOT_READY: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a08");
    pub const SLOT_COOLING: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a09");
    pub const SLOT_SHORT: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a0a");
    pub const SLOT_DARK: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a0b");
    /// A picture's frame with no state to say: bronze.
    pub const SLOT_BRONZE: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a1a");
    /// The button that starts things (PLAY, READY): ember under a gold rim, a sheen crossing it;
    /// the same under the pointer, pressed, and while it can't act.
    pub const GO: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a0c");
    pub const GO_OVER: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a0d");
    pub const GO_DOWN: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a0e");
    pub const GO_WAIT: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a0f");
    /// A finish: clear but for the light it adds, over a control whose own colours say its state.
    /// A thin rim, a wide one, and one for a picture (no gloss over it, a shade round its edge).
    pub const FINISH: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a10");
    pub const FINISH_WIDE: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a11");
    /// The menu's panel and its wider pane: slate that lets the backdrop through.
    pub const MENU_PANEL: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a13");
    pub const MENU_PANE: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a14");
    /// The pane with no panel beside it: every corner cut.
    pub const MENU_ALONE: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a16");
    /// An ally's row, against the screen's left: its right corners cut.
    pub const ALLY: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a18");
    /// An augment's card: a tall control's light, a sheen crossing it now and then.
    pub const FINISH_CARD: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a19");
    /// A finish for the one button to press now: lit from inside, a sheen crossing it.
    pub const FINISH_HOT: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a1b");
    /// The screen's edge while the champion's health is low: red, beating, clear in the middle.
    pub const DANGER: Handle<Plate> = uuid_handle!("6a1f0c22-51d7-4a3e-8f10-0c5e7d2b9a1c");
}

/// Every style and its making.
fn styles() -> Vec<(Handle<Plate>, Make)> {
    use plates::*;
    let slate = Make::SLATE;
    let clear = Color::NONE;
    let slot = |hi: Color, lo: Color, glow: Color, breath: f32| Make {
        top: clear,
        bottom: clear,
        rim_hi: hi,
        rim_lo: lo,
        rim: 3.0,
        grain: 0.05,
        shade: 0.0,
        lip: 0.0,
        glow,
        glow_reach: 5.0,
        breath,
        ..slate
    };
    let go = |top: Color, bottom: Color, glow: f32, sheen: f32| {
        Make {
            top,
            bottom,
            rim_hi: EMBER_HI,
            rim_lo: Color::srgb(0.42, 0.24, 0.06),
            rim: 3.0,
            grain: 0.08,
            shade: 0.18,
            lip: 0.30,
            glow: EMBER.with_alpha(glow),
            glow_reach: 14.0,
            breath: 0.5,
            sheen,
            sheen_secs: 3.6,
            ..slate
        }
        .cut(9.0, 9.0, 9.0, 9.0)
    };
    // A finish's rim is light and dark laid over the control's own border, not a colour.
    // Its gloss is blended in linear light: a hundredth of white over near-black is a tenth to the eye.
    let finish = |rim: f32, gloss: f32, shade: f32| Make {
        top: Color::srgba(1.0, 1.0, 1.0, gloss),
        bottom: Color::srgba(0.0, 0.0, 0.0, gloss * 1.6),
        rim_hi: Color::srgba(1.0, 0.96, 0.86, 0.42),
        rim_lo: Color::srgba(0.0, 0.0, 0.0, 0.50),
        rim,
        grain: 0.0,
        shade,
        lip: 0.0,
        ..slate
    };
    vec![
        (PANEL, slate.cut(12.0, 12.0, 12.0, 12.0)),
        (DOCK_BOTTOM, slate.cut(14.0, 14.0, 0.0, 0.0)),
        (DOCK_TOP, slate.cut(0.0, 0.0, 12.0, 12.0)),
        (DOCK_CORNER, slate.cut(14.0, 0.0, 0.0, 0.0)),
        (
            SOFT,
            Make {
                top: Color::srgba(0.05, 0.062, 0.09, 0.80),
                bottom: Color::srgba(0.02, 0.028, 0.045, 0.66),
                rim_hi: BRONZE.with_alpha(0.85),
                rim_lo: BRONZE_LO.with_alpha(0.85),
                rim: 1.0,
                shade: 0.15,
                lip: 0.05,
                ..slate
            }
            .cut(6.0, 0.0, 6.0, 0.0),
        ),
        (
            TIP,
            Make {
                top: Color::srgba(0.085, 0.10, 0.14, 0.985),
                bottom: Color::srgba(0.03, 0.038, 0.06, 0.985),
                rim: 2.0,
                shade: 0.2,
                ..slate
            }
            .cut(7.0, 0.0, 7.0, 0.0),
        ),
        (
            ALLY,
            Make {
                top: Color::srgba(0.05, 0.062, 0.09, 0.86),
                bottom: Color::srgba(0.02, 0.028, 0.045, 0.74),
                rim_hi: BRONZE.with_alpha(0.9),
                rim_lo: BRONZE_LO.with_alpha(0.9),
                rim: 1.0,
                shade: 0.15,
                lip: 0.05,
                ..slate
            }
            .cut(0.0, 8.0, 8.0, 0.0),
        ),
        (
            FINISH_CARD,
            Make {
                top: Color::srgba(1.0, 1.0, 1.0, 0.015),
                bottom: Color::srgba(0.0, 0.0, 0.0, 0.35),
                sheen: 0.05,
                sheen_secs: 5.0,
                shade: 0.0,
                ..finish(2.0, 0.015, 0.0)
            },
        ),
        (SLOT_READY, slot(EMBER_HI, Color::srgb(0.45, 0.27, 0.08), EMBER.with_alpha(0.0), 0.0)),
        (SLOT_COOLING, slot(IRON_HI, IRON_LO, clear, 0.0)),
        (
            SLOT_SHORT,
            slot(
                Color::srgb(0.50, 0.72, 1.0),
                Color::srgb(0.08, 0.18, 0.42),
                Color::srgba(0.2, 0.45, 1.0, 0.0),
                0.0,
            ),
        ),
        (SLOT_BRONZE, slot(BRONZE_HI, BRONZE_LO, clear, 0.0)),
        (SLOT_DARK, slot(Color::srgb(0.20, 0.21, 0.25), Color::srgb(0.05, 0.055, 0.07), clear, 0.0)),
        (GO, go(Color::srgb(0.46, 0.30, 0.09), Color::srgb(0.20, 0.115, 0.03), 0.20, 0.34)),
        (GO_OVER, go(Color::srgb(0.62, 0.42, 0.13), Color::srgb(0.30, 0.18, 0.05), 0.42, 0.34)),
        (GO_DOWN, go(Color::srgb(0.36, 0.23, 0.07), Color::srgb(0.16, 0.09, 0.025), 0.30, 0.0)),
        (
            GO_WAIT,
            Make {
                top: Color::srgb(0.13, 0.115, 0.09),
                bottom: Color::srgb(0.07, 0.062, 0.05),
                rim_hi: BRONZE,
                rim_lo: BRONZE_LO,
                rim: 3.0,
                shade: 0.2,
                ..slate
            }
            .cut(9.0, 9.0, 9.0, 9.0),
        ),
        (
            DANGER,
            Make {
                top: clear,
                bottom: clear,
                rim: 0.0,
                grain: 0.0,
                shade: 0.0,
                lip: 0.0,
                glow: Color::srgba(0.85, 0.06, 0.03, 0.42),
                glow_reach: 110.0,
                breath: 0.7,
                ..slate
            },
        ),
        (FINISH, finish(1.0, 0.012, 0.0)),
        (FINISH_WIDE, finish(2.0, 0.012, 0.0)),
        (
            FINISH_HOT,
            Make {
                glow: EMBER.with_alpha(0.30),
                glow_reach: 12.0,
                breath: 0.5,
                sheen: 0.10,
                sheen_secs: 3.2,
                ..finish(2.0, 0.02, 0.0)
            },
        ),
        (
            MENU_PANEL,
            Make {
                top: Color::srgba(0.06, 0.075, 0.11, 0.90),
                bottom: Color::srgba(0.022, 0.03, 0.05, 0.92),
                rim: 2.0,
                shade: 0.22,
                ..slate
            }
            .cut(14.0, 0.0, 0.0, 14.0),
        ),
        (
            MENU_PANE,
            Make {
                top: Color::srgba(0.045, 0.056, 0.085, 0.70),
                bottom: Color::srgba(0.02, 0.027, 0.045, 0.78),
                rim_hi: BRONZE.with_alpha(0.8),
                rim_lo: BRONZE_LO.with_alpha(0.8),
                rim: 1.0,
                shade: 0.16,
                lip: 0.04,
                ..slate
            }
            .cut(0.0, 14.0, 14.0, 0.0),
        ),
        (
            MENU_ALONE,
            Make {
                top: Color::srgba(0.045, 0.056, 0.085, 0.70),
                bottom: Color::srgba(0.02, 0.027, 0.045, 0.78),
                rim_hi: BRONZE.with_alpha(0.8),
                rim_lo: BRONZE_LO.with_alpha(0.8),
                rim: 1.0,
                shade: 0.16,
                lip: 0.04,
                ..slate
            }
            .cut(14.0, 14.0, 14.0, 14.0),
        ),
    ]
}

pub struct SkinPlugin;

impl Plugin for SkinPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/plate.wgsl");
        bevy::asset::embedded_asset!(app, "shaders/gauge.wgsl");
        bevy::asset::embedded_asset!(app, "shaders/sparks.wgsl");
        app.add_plugins((
            UiMaterialPlugin::<Plate>::default(),
            UiMaterialPlugin::<Gauge>::default(),
            UiMaterialPlugin::<Sparks>::default(),
        ));
        let _ = app.world_mut().resource_mut::<Assets<Sparks>>().insert(
            SPARKS.id(),
            Sparks {
                s: SparksParams { color: lin(EMBER).with_w(0.85), shape: Vec4::new(1.0, 0.16, 0.35, 0.0) },
            },
        );
        // The faces and the styles are in before the first frame: a handle that never resolves
        // draws nothing at all.
        {
            let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
            for (h, bytes) in [(&SANS, SANS_TTF), (&SANS_BOLD, SANS_BOLD_TTF), (&DISPLAY, DISPLAY_TTF)] {
                let _ = fonts.insert(h.id(), Font::from_bytes(bytes.to_vec()));
            }
        }
        {
            let mut plates = app.world_mut().resource_mut::<Assets<Plate>>();
            for (h, make) in styles() {
                let _ = plates.insert(h.id(), make.plate());
            }
        }
        app.add_systems(Startup, warm).add_systems(Update, presses);
    }
}

/// A node drawn as plate `h` (give the node a border as wide as the style's rim).
pub fn plate(h: Handle<Plate>) -> MaterialNode<Plate> {
    MaterialNode(h)
}

/// The light a control wears over its own colours: a thin rim's.
pub fn finish() -> MaterialNode<Plate> {
    MaterialNode(plates::FINISH)
}

/// A soft shadow under a plate, and a tighter one for what sits on a plate.
pub fn drop_shadow() -> BoxShadow {
    BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.60), px(0), px(5), px(0), px(18))
}

/// A glow round something hot.
pub fn halo(c: Color, alpha: f32, blur: f32) -> BoxShadow {
    BoxShadow::new(c.with_alpha(alpha), px(0), px(0), px(1), px(blur))
}

/// A control drawn as one plate at rest, another under the pointer and a third pressed.
#[derive(Component, Clone)]
pub struct Press {
    pub idle: Handle<Plate>,
    pub over: Handle<Plate>,
    pub down: Handle<Plate>,
}

impl Press {
    pub const GO: Press = Press { idle: plates::GO, over: plates::GO_OVER, down: plates::GO_DOWN };
    pub const WAIT: Press = Press { idle: plates::GO_WAIT, over: plates::GO_WAIT, down: plates::GO_WAIT };
}

type Pressed = (Changed<Interaction>, With<Press>);

fn presses(mut q: Query<(&Interaction, &Press, &mut MaterialNode<Plate>), Pressed>) {
    for (interaction, press, mut node) in &mut q {
        let want = match interaction {
            Interaction::None => &press.idle,
            Interaction::Hovered => &press.over,
            Interaction::Pressed => &press.down,
        };
        if node.0 != *want {
            node.0 = want.clone();
        }
    }
}

/// Words in the condensed face.
pub fn sans(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font: SANS.into(),
            font_size: FontSize::Px(size),
            weight: FontWeight::NORMAL,
            ..default()
        },
        TextColor(color),
    )
}

/// The condensed face, bold: a number, a label, a button.
pub fn bold(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font: SANS_BOLD.into(),
            font_size: FontSize::Px(size),
            weight: FontWeight::BOLD,
            ..default()
        },
        TextColor(color),
    )
}

/// The carved face: a title, a champion's name, a result.
pub fn display(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font: DISPLAY.into(),
            font_size: FontSize::Px(size),
            weight: FontWeight::BOLD,
            ..default()
        },
        TextColor(color),
    )
}

/// The plates' and the gauges' pipelines, built before a match is in view (`warm.rs`,
/// `frame.rs`): a speck of each, clear, in the screen's corner from the first frame. The gauges
/// come up only with the match, and `?queue=1` skips the menu's plates.
fn warm(mut commands: Commands, mut plates: ResMut<Assets<Plate>>, mut gauges: ResMut<Assets<Gauge>>) {
    let speck = || {
        (
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: px(1),
                height: px(1),
                ..default()
            },
            GlobalZIndex(-100),
        )
    };
    let clear = Make { top: Color::NONE, bottom: Color::NONE, rim: 0.0, ..Make::SLATE }.plate();
    commands.spawn((speck(), MaterialNode(plates.add(clear))));
    let empty = GaugeParams {
        lo: Vec4::ZERO,
        hi: Vec4::ZERO,
        lost: Vec4::ZERO,
        back: Vec4::ZERO,
        amount: Vec4::ZERO,
        live: Vec4::ZERO,
    };
    commands.spawn((speck(), MaterialNode(gauges.add(Gauge { g: empty }))));
}
