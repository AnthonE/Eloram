//! The main menu's kit: its type, its colours and its pieces, after Gates' `render/ui.rs`
//! (`AnthonE/Gates`, MIT, MoreRight DAO), whose arrangement the menu takes (a wordmark top left,
//! a nav column that never moves, the identity at the top right, a panel and a pane over a backdrop
//! that drifts under a scrim) in Eloram's own colours: the HUD's navy, bronze and gold (`hud.rs`).
//!
//! The type and the plates are the UI's own (`skin.rs`): Cinzel for the wordmark, the nav and a
//! page's title, Roboto Condensed for the rest.

use bevy::asset::{RenderAssetUsages, uuid_handle};
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;

use crate::hud::{BUTTON, BUTTON_GOLD, DIM, FRAME, FRAME_SOFT, GOLD, TEXT, TRIM};
use crate::skin::{self, DISPLAY, SANS, SANS_BOLD, plates};

/// The sigil beside the wordmark: the window's and the page's icon (`icon/eloram-256.png`).
pub const SIGIL: Handle<Image> = uuid_handle!("2f6e0a51-8c3b-4d6e-9a21-5b7c0e4d1a03");

const SIGIL_PNG: &[u8] = include_bytes!("../../icon/eloram-256.png");

/// The menu's colours, from the HUD's (a handful of its own for the scrim and the hover).
pub const INK: Color = Color::srgb(0.02, 0.025, 0.045);
pub const RULE: Color = FRAME_SOFT;
pub const NAV_IDLE: Color = Color::srgba(0.70, 0.72, 0.78, 0.62);
pub const TITLE: Color = TEXT;
pub const FAINT: Color = Color::srgba(0.56, 0.59, 0.66, 0.80);
pub const ROW_HOVER: Color = Color::srgb(0.17, 0.18, 0.24);
/// A row that can be picked, at rest: a shade darker than its panel.
pub const ROW_IDLE: Color = Color::srgba(0.0, 0.0, 0.0, 0.28);
pub const GO_HOVER: Color = Color::srgb(0.40, 0.32, 0.13);
pub const PICKED: Color = Color::srgba(0.86, 0.56, 0.20, 0.16);
pub const BAD: Color = Color::srgb(0.90, 0.48, 0.42);

/// The layout's measures, Gates' at 1280 wide scaled to the HUD's 1600 by 900 design.
pub const GUT: f32 = 42.0;
pub const NAV_W: f32 = 288.0;
pub const PANEL_W: f32 = 475.0;
pub const BUTTON_H: f32 = 40.0;

/// Puts the sigil in the assets at plugin-build time: a handle that never resolves draws nothing,
/// so it is compiled in and there before the first frame.
pub fn build(app: &mut App) {
    if let Ok(img) = Image::from_buffer(
        SIGIL_PNG,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::linear(),
        RenderAssetUsages::RENDER_WORLD,
    ) {
        let _ = app.world_mut().resource_mut::<Assets<Image>>().insert(SIGIL.id(), img);
    }
}

/// Prose: a blurb, a status line.
pub fn label(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
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

/// Everything else: an entry, a heading, a button, a number.
pub fn strong(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
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

/// The carved face: the wordmark, the nav, a page's title.
pub fn title(s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
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

pub const TEXT_SHADOW: TextShadow =
    TextShadow { offset: Vec2::splat(1.0), color: Color::srgba(0.0, 0.0, 0.0, 0.85) };

/// What a button's ground is at rest and under the pointer.
#[derive(Component, Clone, Copy)]
pub struct Hover {
    pub idle: Color,
    pub over: Color,
}

pub fn pressed(c: Color) -> Color {
    let s = c.to_srgba();
    Color::srgba(s.red * 0.8, s.green * 0.8, s.blue * 0.8, s.alpha)
}

/// One hover for every button of the menu: lit under the pointer, a shade down and sunk a hair
/// when pressed.
pub fn hover(
    mut q: Query<(&Interaction, &Hover, &mut BackgroundColor, &mut UiTransform), Changed<Interaction>>,
) {
    for (interaction, h, mut bg, mut xf) in &mut q {
        let (want, scale) = match interaction {
            Interaction::None => (h.idle, 1.0),
            Interaction::Hovered => (h.over, 1.0),
            Interaction::Pressed => (pressed(h.over), 0.96),
        };
        bg.set_if_neq(BackgroundColor(want));
        if xf.scale != Vec2::splat(scale) {
            xf.scale = Vec2::splat(scale);
        }
    }
}

/// The Causeway behind the menu (`web/assets/menu/causeway.jpg`, made by the art suite): absent is
/// a supported state, the ink showing through.
pub const BACKDROP_PATH: &str = "menu/causeway.jpg";

#[derive(Component)]
pub struct Drifts;

pub fn backdrop(image: Handle<Image>) -> impl Bundle {
    (
        ImageNode::new(image),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        ZIndex(-2),
        Drifts,
    )
}

/// The wash over the backdrop: heavy at the left where the nav and the panel are, and at the foot,
/// light on the right where the lane is.
pub fn scrim() -> impl Bundle {
    let ink = |a: f32| Color::srgba(0.02, 0.025, 0.045, a);
    let stops =
        |s: &[(f32, f32)]| s.iter().map(|&(a, at)| ColorStop::new(ink(a), percent(at))).collect::<Vec<_>>();
    (
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundGradient(vec![
            LinearGradient::to_right(stops(&[(0.90, 0.0), (0.66, 24.0), (0.22, 56.0), (0.06, 100.0)])).into(),
            LinearGradient::to_bottom(stops(&[(0.45, 0.0), (0.0, 18.0), (0.0, 52.0), (0.86, 100.0)])).into(),
            RadialGradient::new(
                UiPosition::TOP_LEFT.at_percent(64.0, 40.0),
                RadialGradientShape::FarthestCorner,
                vec![
                    ColorStop::new(Color::NONE, percent(45)),
                    ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.45), percent(100)),
                ],
            )
            .into(),
        ]),
        ZIndex(-1),
    )
}

/// The backdrop's slow push-in: 70 s one way, eased, then back (a still that moves reads as
/// footage). Read off the app's clock, so a rebuild doesn't restart it.
pub fn drift(t: f32) -> UiTransform {
    let leg = 70.0;
    let u = (t % (2.0 * leg)) / leg;
    let u = if u > 1.0 { 2.0 - u } else { u };
    let e = u * u * (3.0 - 2.0 * u);
    UiTransform {
        translation: Val2::percent(-1.4 * e, -0.8 * e),
        scale: Vec2::splat(1.0 + 0.07 * e),
        ..default()
    }
}

/// The wordmark: the sigil, the name carved in gold with a light behind it, a rule and a line
/// under it.
pub fn wordmark(line: &str) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(18),
            ..default()
        },
        children![
            (ImageNode::new(SIGIL), Node { width: px(62), height: px(62), ..default() }),
            (
                Node { flex_direction: FlexDirection::Column, row_gap: px(4), ..default() },
                children![
                    (
                        title("ELORAM", 56.0, GOLD),
                        TextShadow { offset: Vec2::new(0.0, 3.0), color: Color::srgba(0.0, 0.0, 0.0, 0.9) },
                    ),
                    (
                        // The rule under the name: bright at its start, gone by its end.
                        Node { width: px(300), height: px(2), ..default() },
                        BackgroundGradient(vec![
                            LinearGradient::to_right(vec![
                                ColorStop::new(skin::EMBER_HI, percent(0)),
                                ColorStop::new(skin::BRONZE.with_alpha(0.7), percent(45)),
                                ColorStop::new(Color::NONE, percent(100)),
                            ])
                            .into(),
                        ]),
                    ),
                    strong(line.to_string(), 14.0, FAINT),
                ],
            ),
        ],
    )
}

/// A nav entry's colour: the picked one brightest, the pointer lifts the rest.
pub fn nav_color(picked: bool, over: bool) -> Color {
    match (picked, over) {
        (true, _) => TITLE,
        (false, true) => TEXT,
        (false, false) => NAV_IDLE,
    }
}

/// One entry in the nav column: the type is the button; the picked one has an ember bar in the
/// gutter, lit.
pub fn nav_item(text: &str, picked: bool) -> impl Bundle {
    (
        Button,
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(0), px(9)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        Hover { idle: Color::NONE, over: Color::NONE },
        children![
            (
                Node {
                    width: px(5),
                    height: px(26),
                    margin: UiRect::new(px(-19), px(14), px(0), px(0)),
                    ..default()
                },
                BackgroundColor(if picked { skin::EMBER_HI } else { Color::NONE }),
                if picked { skin::halo(skin::EMBER, 0.75, 14.0) } else { skin::halo(Color::NONE, 0.0, 0.0) },
            ),
            (title(text.to_string(), 27.0, nav_color(picked, false)), TEXT_SHADOW),
        ],
    )
}

/// The control column: a plate, its outer corners cut.
pub fn panel(width: f32) -> impl Bundle {
    (
        Node {
            width: px(width),
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            padding: UiRect::all(px(25)),
            border: UiRect::all(px(2)),
            ..default()
        },
        skin::plate(plates::MENU_PANEL),
    )
}

/// The wide content column beside it (`left_edge`: drawn alone, every corner its own).
pub fn pane(left_edge: bool) -> impl Bundle {
    (
        Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            padding: UiRect::all(px(25)),
            border: UiRect::all(px(1)),
            overflow: Overflow::clip_y(),
            ..default()
        },
        skin::plate(if left_edge { plates::MENU_ALONE } else { plates::MENU_PANE }),
    )
}

/// A group's heading with a rule running out to its right.
pub fn group_rule(text: &str) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(10),
            margin: UiRect::top(px(12)),
            flex_shrink: 0.0,
            ..default()
        },
        children![
            strong(text.to_string(), 15.0, FAINT),
            (Node { flex_grow: 1.0, height: px(1), ..default() }, BackgroundColor(RULE)),
        ],
    )
}

/// The big button (PLAY): ember under a gold rim, a sheen crossing it, its line under the label;
/// dull but still clickable when it can't act, so a click says why.
pub fn go(label: &str, line: &str, ready: bool) -> impl Bundle {
    (
        Button,
        Node {
            height: px(84),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(1),
            border: UiRect::all(px(3)),
            flex_shrink: 0.0,
            ..default()
        },
        skin::plate(if ready { plates::GO } else { plates::GO_WAIT }),
        if ready { skin::Press::GO } else { skin::Press::WAIT },
        if ready { skin::halo(skin::EMBER, 0.32, 26.0) } else { skin::halo(Color::NONE, 0.0, 0.0) },
        // The hover's own: the button sinks a hair when pressed.
        BackgroundColor(Color::NONE),
        Hover { idle: Color::NONE, over: Color::NONE },
        children![
            (
                title(label.to_string(), 34.0, if ready { Color::srgb(1.0, 0.95, 0.80) } else { DIM }),
                TextShadow { offset: Vec2::new(0.0, 2.0), color: Color::srgba(0.0, 0.0, 0.0, 0.8) },
            ),
            strong(
                line.to_uppercase(),
                13.0,
                if ready { Color::srgba(1.0, 0.93, 0.78, 0.85) } else { FAINT }
            ),
        ],
    )
}

/// A plain button, `width` wide (0: as wide as its words), lit gold when `hot`.
pub fn button(text: &str, width: f32, hot: bool) -> impl Bundle {
    let idle = if hot { BUTTON_GOLD } else { BUTTON };
    (
        Button,
        Node {
            width: if width > 0.0 { px(width) } else { Val::Auto },
            height: px(BUTTON_H),
            padding: UiRect::axes(px(16), px(0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(idle),
        BorderColor::all(if hot { TRIM } else { FRAME }),
        skin::finish(),
        Hover { idle, over: if hot { GO_HOVER } else { ROW_HOVER } },
        children![strong(text.to_uppercase(), 15.0, if hot { GOLD } else { TEXT })],
    )
}

/// A spacer that takes the room left.
pub fn spacer() -> impl Bundle {
    Node { flex_grow: 1.0, ..default() }
}
