//! The main menu's kit: its type, its colours and its pieces, after Gates' `render/ui.rs`
//! (`AnthonE/Gates`, MIT, MoreRight DAO), whose arrangement the menu takes (a wordmark top left,
//! a nav column that never moves, the identity at the top right, a panel and a pane over a backdrop
//! that drifts under a scrim) in Eloram's own colours: the HUD's navy, bronze and gold (`hud.rs`).
//!
//! The type is Roboto Condensed (Apache-2.0, `crates/el-client/fonts/`), compiled in, in two
//! fixed handles: the menu's alone. The HUD keeps Bevy's default face, which its fixed-width rows
//! and the ASCII rule of its words (`el_client_core::controls`) are made for.

use bevy::asset::{RenderAssetUsages, uuid_handle};
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;

use crate::hud::{BUTTON, BUTTON_GOLD, DIM, FRAME, FRAME_SOFT, GOLD, TEXT, TRIM};

pub const SANS: Handle<Font> = uuid_handle!("2f6e0a51-8c3b-4d6e-9a21-5b7c0e4d1a01");
pub const SANS_BOLD: Handle<Font> = uuid_handle!("2f6e0a51-8c3b-4d6e-9a21-5b7c0e4d1a02");
/// The sigil beside the wordmark: the window's and the page's icon (`icon/eloram-256.png`).
pub const SIGIL: Handle<Image> = uuid_handle!("2f6e0a51-8c3b-4d6e-9a21-5b7c0e4d1a03");

const SANS_TTF: &[u8] = include_bytes!("../../fonts/RobotoCondensed-Regular.ttf");
const SANS_BOLD_TTF: &[u8] = include_bytes!("../../fonts/RobotoCondensed-Bold.ttf");
const SIGIL_PNG: &[u8] = include_bytes!("../../icon/eloram-256.png");

/// The menu's colours, from the HUD's (a handful of its own for the scrim and the hover).
pub const INK: Color = Color::srgb(0.02, 0.025, 0.045);
pub const PANEL_BG: Color = Color::srgba(0.035, 0.045, 0.07, 0.86);
pub const STAGE_BG: Color = Color::srgba(0.035, 0.045, 0.07, 0.62);
pub const RULE: Color = FRAME_SOFT;
pub const NAV_IDLE: Color = Color::srgba(0.70, 0.72, 0.78, 0.62);
pub const TITLE: Color = TEXT;
pub const FAINT: Color = Color::srgba(0.56, 0.59, 0.66, 0.80);
pub const ROW_HOVER: Color = Color::srgb(0.17, 0.18, 0.24);
pub const GO_HOVER: Color = Color::srgb(0.40, 0.32, 0.13);
pub const GO_WAIT: Color = Color::srgb(0.16, 0.14, 0.10);
pub const PICKED: Color = Color::srgba(0.78, 0.64, 0.36, 0.22);
pub const BAD: Color = Color::srgb(0.90, 0.48, 0.42);

/// The layout's measures, Gates' at 1280 wide scaled to the HUD's 1600 by 900 design.
pub const GUT: f32 = 42.0;
pub const NAV_W: f32 = 288.0;
pub const PANEL_W: f32 = 475.0;
pub const BUTTON_H: f32 = 40.0;

/// Puts the faces and the sigil in the assets at plugin-build time: a handle that never resolves
/// draws no text at all, so they are compiled in and there before the first frame.
pub fn build(app: &mut App) {
    {
        let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
        for (h, bytes) in [(&SANS, SANS_TTF), (&SANS_BOLD, SANS_BOLD_TTF)] {
            let _ = fonts.insert(h.id(), Font::from_bytes(bytes.to_vec()));
        }
    }
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

/// The wordmark: the sigil, the name in gold, a line under it.
pub fn wordmark(line: &str) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(16),
            ..default()
        },
        children![
            (ImageNode::new(SIGIL), Node { width: px(54), height: px(54), ..default() }),
            (
                Node { flex_direction: FlexDirection::Column, row_gap: px(6), ..default() },
                children![(strong("ELORAM", 54.0, GOLD), TEXT_SHADOW), strong(line.to_string(), 14.0, FAINT)],
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

/// One entry in the nav column: the type is the button; the picked one has a gold bar in the gutter.
pub fn nav_item(text: &str, picked: bool) -> impl Bundle {
    (
        Button,
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(0), px(8)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        Hover { idle: Color::NONE, over: Color::NONE },
        children![
            (
                Node {
                    width: px(6),
                    height: px(28),
                    margin: UiRect::new(px(-19), px(13), px(0), px(0)),
                    ..default()
                },
                BackgroundColor(if picked { GOLD } else { Color::NONE }),
            ),
            strong(text.to_string(), 32.0, nav_color(picked, false)),
        ],
    )
}

/// The control column.
pub fn panel(width: f32) -> impl Bundle {
    (
        Node {
            width: px(width),
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            padding: UiRect::all(px(25)),
            border: UiRect::all(px(1)),
            ..default()
        },
        BackgroundColor(PANEL_BG),
        BorderColor::all(RULE),
    )
}

/// The wide content column beside it (`left_edge`: drawn alone, with its own left border).
pub fn pane(left_edge: bool) -> impl Bundle {
    (
        Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            padding: UiRect::all(px(25)),
            border: UiRect::new(px(if left_edge { 1 } else { 0 }), px(1), px(1), px(1)),
            overflow: Overflow::clip_y(),
            ..default()
        },
        BackgroundColor(STAGE_BG),
        BorderColor::all(RULE),
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

/// The big gold button (PLAY), its line under the label; dim but still clickable when it can't
/// act, so a click says why.
pub fn go(label: &str, line: &str, ready: bool) -> impl Bundle {
    let idle = if ready { BUTTON_GOLD } else { GO_WAIT };
    (
        Button,
        Node {
            height: px(80),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: px(2),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(4)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(idle),
        BorderColor::all(if ready { TRIM } else { FRAME }),
        Hover { idle, over: if ready { GO_HOVER } else { GO_WAIT } },
        children![
            strong(label.to_string(), 36.0, if ready { GOLD } else { DIM }),
            strong(line.to_uppercase(), 13.0, if ready { TEXT.with_alpha(0.75) } else { FAINT }),
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
            border_radius: BorderRadius::all(px(4)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(idle),
        BorderColor::all(if hot { TRIM } else { FRAME }),
        Hover { idle, over: if hot { GO_HOVER } else { ROW_HOVER } },
        children![strong(text.to_string(), 16.0, if hot { GOLD } else { TEXT })],
    )
}

/// A spacer that takes the room left.
pub fn spacer() -> impl Bundle {
    Node { flex_grow: 1.0, ..default() }
}
