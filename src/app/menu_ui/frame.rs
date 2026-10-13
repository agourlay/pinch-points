//! How a card is dressed: the driftwood frame round it and the tide
//! lapping at its foot, given to every new card the frame it appears.

use super::{CARD_PAD, Viewport};
use crate::app::palette;
use bevy::color::Mix;
use bevy::prelude::*;
use bevy::ui::widget::NodeImageMode;

/// A card the tide dresses: [`dress_cards`] gives every one of these a
/// foam line at its foot the frame it appears, so a screen cannot forget
/// the dressing and no screen has to carry the art handle for it.
#[derive(Component)]
pub struct ShoreCard;

/// The driftwood frame round a card (`card_frame.png`): nine-sliced, so its
/// corners draw as they are and its sides stretch along the card.
#[derive(Component)]
pub struct CardFrame;

/// A card that wears the driftwood frame but not the tide line: the menu's
/// own sign and list, and the lobby's panels, which draw their foam
/// themselves or have none. Every [`ShoreCard`] is framed too.
#[derive(Component)]
pub struct Framed;

/// The corner a framed card is rounded to: the frame's own, so no corner of
/// the card's fill pokes out past the wood.
const FRAMED_RADIUS: f32 = 16.0;

/// How wide the frame's band of wood is, in the sprite's own pixels
/// (tools/gen_sprites.py draws it 32 of 192).
const FRAME_BAND: f32 = 32.0;

/// Where the sprite is cut, in its own pixels: past the band, far enough
/// in that each corner slice holds its whole rounded corner. Cut at the
/// band, the curves ran over into the side slices and were stretched along
/// them, as dark smears into the card.
const FRAME_SLICE: f32 = 56.0;
const _: () = assert!(
    FRAME_SLICE > FRAME_BAND,
    "the cut must hold each corner whole"
);

/// How thick the frame is drawn, in interface pixels.
pub(super) const FRAME_PX: f32 = 13.0;

/// The slice's corner scale for a frame [`FRAME_PX`] thick at this scale.
///
/// Bevy draws a slice's border at the sprite's own pixels times this cap,
/// in *physical* pixels, whatever the interface's scale: left fixed, a
/// frame drawn 10 px thick at 720p came out 7 at 1080p, as the cards grew
/// round it.
fn frame_corner_scale(ui_scale: f32, scale_factor: f32) -> f32 {
    FRAME_PX / FRAME_BAND * ui_scale * scale_factor
}

/// A card's frame: driftwood, or driftwood stained with the card's own
/// edge colour where a screen gave it one (the winner's, a loss's red),
/// so the frame carries what the hairline under it used to.
fn frame_bundle(art: &crate::app::art::Art, edge: Option<Color>, corner: f32) -> impl Bundle {
    let tint = frame_tint(edge);
    (
        CardFrame,
        ImageNode {
            image: art.card_frame.clone(),
            color: tint,
            image_mode: NodeImageMode::Sliced(TextureSlicer {
                border: BorderRect::all(FRAME_SLICE),
                max_corner_scale: corner,
                ..default()
            }),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            // Over the card's own edge, a pixel out, so none of the
            // hairline shows round it.
            left: Val::Px(-1.0),
            right: Val::Px(-1.0),
            top: Val::Px(-1.0),
            bottom: Val::Px(-1.0),
            ..default()
        },
        Pickable::IGNORE,
    )
}

/// The tint a card's frame takes from the card's edge: bare wood for the
/// plain gold edge every card is born with, or none; the wood stained
/// toward any other, at full strength whatever the edge's own alpha.
fn frame_tint(edge: Option<Color>) -> Color {
    match edge {
        Some(color) if color != palette::CARD_EDGE => {
            Color::WHITE.mix(&color.with_alpha(1.0), 0.55)
        }
        Some(_) | None => Color::WHITE,
    }
}

/// Keep every frame [`FRAME_PX`] thick in interface pixels as the
/// interface's scale moves (see [`frame_corner_scale`]).
pub fn scale_card_frames(
    viewport: Viewport,
    mut frames: Query<&mut ImageNode, With<CardFrame>>,
    added: Query<(), Added<CardFrame>>,
) {
    if !viewport.ui_scale_changed() && added.is_empty() {
        return;
    }
    let corner = frame_corner_scale(viewport.ui_scale(), viewport.scale_factor());
    for mut frame in &mut frames {
        if let NodeImageMode::Sliced(slicer) = &mut frame.image_mode
            && (slicer.max_corner_scale - corner).abs() > f32::EPSILON
        {
            slicer.max_corner_scale = corner;
        }
    }
}

/// A card just made: what dressing it needs to know.
type NewCard = (Entity, &'static mut Node, Option<&'static BorderColor>);

/// Give a newborn card its tide line and its driftwood frame.
pub fn dress_cards(
    mut commands: Commands,
    art: Res<crate::app::art::Art>,
    viewport: Viewport,
    mut cards: Query<NewCard, Added<ShoreCard>>,
    mut framed: Query<NewCard, (Added<Framed>, Without<ShoreCard>)>,
) {
    let corner = frame_corner_scale(viewport.ui_scale(), viewport.scale_factor());
    for (card, mut node, edge) in &mut framed {
        let frame = frame_card(&mut commands, &art, &mut node, edge, corner);
        commands.entity(card).insert_children(0, &[frame]);
    }
    for (card, mut node, edge) in &mut cards {
        // Spawned before the tide, inserted after it below.
        let frame = frame_card(&mut commands, &art, &mut node, edge, corner);
        // The last row keeps its feet dry whatever padding the card chose.
        let dry = Val::Px(FOAM_DEPTH + 4.0);
        if px_of(node.padding.bottom).unwrap_or(0.0) < FOAM_DEPTH {
            node.padding.bottom = dry;
        }
        // Prepended, not appended: UI siblings stack in the order they are
        // added, and the foam belongs behind the rows, not over the last
        // one, which is the promise tide_line's doc makes.
        let tide = commands.spawn(tide_bundle(&art.foam)).id();
        // The frame over the foam, so the tide breaks inside the wood
        // rather than across it, and under the rows.
        commands.entity(card).insert_children(0, &[tide, frame]);
    }
}

/// Round a card to the frame's corner, hold its contents clear of the
/// wood, and spawn the frame for it, stained with the card's edge, for the
/// caller to put in place among the card's children.
fn frame_card(
    commands: &mut Commands,
    art: &crate::app::art::Art,
    node: &mut Node,
    edge: Option<&BorderColor>,
    corner: f32,
) -> Entity {
    node.border_radius = BorderRadius::all(Val::Px(FRAMED_RADIUS));
    clear_the_wood(node);
    commands
        .spawn(frame_bundle(art, edge.map(|edge| edge.top), corner))
        .id()
}

/// Hold a framed card's contents [`CARD_PAD`] in from its edge at the top
/// and the sides, whatever padding the card chose: the frame is drawn over
/// the card, so anything nearer runs up against the wood. The foot is the
/// tide's to set.
fn clear_the_wood(node: &mut Node) {
    for side in [
        &mut node.padding.top,
        &mut node.padding.left,
        &mut node.padding.right,
    ] {
        if px_of(*side).is_some_and(|px| px < CARD_PAD) {
            *side = Val::Px(CARD_PAD);
        }
    }
}

fn px_of(v: Val) -> Option<f32> {
    match v {
        Val::Px(px) => Some(px),
        // Font-relative lengths (new in Bevy 0.20) are resolved by the
        // layout pass, not known here; no card pads in them.
        Val::Auto
        | Val::Percent(_)
        | Val::Vw(_)
        | Val::Vh(_)
        | Val::VMin(_)
        | Val::VMax(_)
        | Val::Em(_)
        | Val::Rem(_) => None,
    }
}

/// How deep the tide laps into a card's foot.
pub const FOAM_DEPTH: f32 = 20.0;

/// The tide at the foot of a card: the foam strip, drawn across it and
/// turned over so it breaks upward into the panel. It makes a card a
/// stretch of beach rather than a box; spawn it before the rows so UI
/// sibling order keeps it behind them, and leave the card room at the
/// foot ([`FOAM_DEPTH`]) so the last row stays dry.
pub fn tide_line(
    panel: &mut bevy::ecs::relationship::RelatedSpawnerCommands<ChildOf>,
    foam: &Handle<Image>,
) {
    panel.spawn(tide_bundle(foam));
}

/// The pieces of one tide line, for a spawner that is not a child scope.
fn tide_bundle(foam: &Handle<Image>) -> (ImageNode, Node) {
    (
        ImageNode {
            image: foam.clone(),
            color: palette::FOAM_LINE,
            flip_y: true,
            // Stretched, not tiled: tiling scales the sprite down to the
            // strip's height first, and six scallops inside twenty pixels
            // come out as a dotted line.
            image_mode: NodeImageMode::Stretch,
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            bottom: Val::Px(0.0),
            height: Val::Px(FOAM_DEPTH),
            // Kept off the card's rounded corners, which a square strip
            // would otherwise fill back in.
            border_radius: BorderRadius::bottom(Val::Px(10.0)),
            ..default()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain card's frame is bare wood; a card a screen gave its own edge
    /// (the winner's colour, a loss's red) stains its wood with it, at full
    /// strength however faint that edge was drawn.
    #[test]
    fn a_frame_takes_the_cards_own_colour_and_only_that() {
        assert_eq!(frame_tint(None), Color::WHITE);
        assert_eq!(frame_tint(Some(palette::CARD_EDGE)), Color::WHITE);
        let red = frame_tint(Some(palette::INK_RAID.with_alpha(0.6)));
        assert_ne!(red, Color::WHITE, "stained");
        assert_eq!(red.alpha(), 1.0, "at full strength");
        assert_eq!(
            red,
            frame_tint(Some(palette::INK_RAID)),
            "whatever the edge's alpha"
        );
    }

    /// The frame is the same thickness in interface pixels at every scale:
    /// its slice cap follows the interface and the display, so a card grown
    /// to 1080p keeps its frame in proportion, and the default is the
    /// frame's own width over the band's.
    #[test]
    fn a_frame_keeps_its_thickness_at_every_scale() {
        let at = |ui: f32, factor: f32| FRAME_BAND * frame_corner_scale(ui, factor) / (ui * factor);
        for (ui, factor) in [(1.0, 1.0), (1.5, 1.0), (0.8, 1.0), (1.5, 2.0)] {
            assert!((at(ui, factor) - FRAME_PX).abs() < 1e-4, "{ui} x {factor}");
        }
    }
}
