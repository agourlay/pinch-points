//! The crab field guide along the foot of the play screens: each kind
//! drawn as it walks the board, with what it banks on this board.

use crate::app::settings::GameSettings;
use crate::app::{Playback, Screen, menu_ui, palette};
use bevy::prelude::*;

/// The crab field guide: each kind in its colour with what it banks.
///
/// At the foot of the play screens, opposite the prompt, where the thing it
/// explains is on the board in front of you, rather than on the landing
/// menu, which is the one screen with no crab on it.
#[derive(Component)]
pub struct FieldGuide;

/// One crab's note in the guide: `KINDS[i]`, so a language change can
/// find and reword it.
#[derive(Component)]
pub struct FieldGuideNote(usize);

/// The golden crab's coin and note, folded away on a round that never
/// has one: the strip quoted a jackpot a one-minute round cannot call.
#[derive(Component)]
pub struct GoldenEntry;

/// A crab's size in the guide. It was 20, and a crab drawn with its legs
/// out to the corners of its square was a speck at that size.
const LEGEND_CRAB_PX: f32 = 26.0;

/// The sand a legend crab stands on: the board's own, a touch dimmed so the
/// coins do not outshine the notes beside them.
const LEGEND_SAND: Color = Color::srgb(0.84, 0.77, 0.62);

/// The kinds the guide lists, in the order shown.
const KINDS: [crate::sim::CrabKind; 6] = [
    crate::sim::CrabKind::Common,
    crate::sim::CrabKind::Juvenile,
    crate::sim::CrabKind::Giant,
    crate::sim::CrabKind::Molting,
    crate::sim::CrabKind::Golden,
    crate::sim::CrabKind::Sparkling,
];

/// What the guide says of `KINDS[i]`: its worth on this board, and its
/// trait in the player's language. The name is the one part left off: on
/// the board a crab is a colour, and the colour is right there.
///
/// The board's worth and not the kind's, because they differ: a versus
/// round's called golden crab is worth less than a puzzle's
/// (`Board::crab_value`, `GOLDEN_CALLED_VALUE`), and a guide quoting the
/// kind would promise the puzzle's price for a crab that banks the other.
///
/// The sparkling crab's "surprise" is the event wheel it spins, and on a
/// beach whose wheel is off (`Board::events_enabled`, the same switch the
/// spin reads) it is a crab worth its points and nothing more.
fn field_guide_note(tr: &crate::app::i18n::Tr, i: usize, board: &crate::sim::Board) -> String {
    let worth = board.crab_value(KINDS[i]);
    let wheel_off = KINDS[i] == crate::sim::CrabKind::Sparkling && !board.events_enabled();
    let note = if wheel_off { "" } else { tr.crab_notes[i] };
    if note.is_empty() {
        worth.to_string()
    } else {
        format!("{worth} {note}")
    }
}

/// The notes are baked from the language at start-up; a language picked
/// later rewords them here.
pub fn update_field_guide(
    settings: Res<GameSettings>,
    sim: Res<crate::app::Sim>,
    mut notes: Query<(&FieldGuideNote, &mut Text)>,
    mut golden_entries: Query<&mut Node, With<GoldenEntry>>,
    mut board_shown: Local<Option<(u32, bool, bool)>>,
) {
    // The notes move only with the board's rules, which change when a
    // round of another kind is loaded: the golden crab's worth is the one
    // that differs between them, and whether it comes at all, and the
    // sparkling crab's surprise whether the wheel turns.
    let read = (
        sim.0.crab_value(crate::sim::CrabKind::Golden),
        sim.0.golden_crab_can_come(),
        sim.0.events_enabled(),
    );
    if !settings.is_changed() && Some(read) == *board_shown {
        return;
    }
    *board_shown = Some(read);
    for mut node in &mut golden_entries {
        menu_ui::set_shown(&mut node, read.1);
    }
    let tr = settings.tr();
    for (note, mut text) in &mut notes {
        menu_ui::set_text(&mut text, &field_guide_note(tr, note.0, &sim.0));
    }
}

pub(super) fn spawn_field_guide(
    commands: &mut Commands,
    tr: &crate::app::i18n::Tr,
    art: &crate::app::art::Art,
    board: &crate::sim::Board,
) {
    // Its own row, in the band between the foot of the board and the
    // prompt. It cannot share the prompt's line: with the traits on it the
    // two come to more than the window is wide in every language.
    commands
        .spawn((
            FieldGuide,
            Node {
                position_type: PositionType::Absolute,
                // Clear of both its neighbours: the board stops above it
                // because the chrome reserves this band, and the prompt
                // starts below it with the same air in between.
                bottom: Val::Px(60.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    column_gap: Val::Px(10.0),
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(4.0)),
                    border_radius: BorderRadius::all(Val::Px(11.0)),
                    ..default()
                },
                BackgroundColor(palette::PILL_FILL),
            ))
            .with_children(|strip| {
                for (i, kind) in KINDS.iter().enumerate() {
                    // The crab as it walks the board, big claw and all,
                    // rather than a coloured dot: its body and claw in one
                    // square, the claw in the body's colour, since the
                    // guide is about kinds and not about hands.
                    let color = crate::app::creatures::body_color(*kind);
                    let layer = |image: &Handle<Image>| {
                        (
                            ImageNode::new(image.clone()).with_color(color),
                            Node {
                                position_type: PositionType::Absolute,
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                        )
                    };
                    // Each on a coin of sand, the ground it is drawn for:
                    // its legs and outline are dark, and straight on the
                    // dark pill they vanished and left a blob with a claw.
                    let mut coin = strip.spawn((
                        Node {
                            width: Val::Px(LEGEND_CRAB_PX),
                            height: Val::Px(LEGEND_CRAB_PX),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(LEGEND_SAND),
                    ));
                    coin.with_children(|icon| {
                        icon.spawn(layer(&art.crab));
                        icon.spawn(layer(&art.claw));
                    });
                    let golden = *kind == crate::sim::CrabKind::Golden;
                    if golden {
                        coin.insert(GoldenEntry);
                    }
                    let mut note = strip.spawn((
                        FieldGuideNote(i),
                        Text::new(field_guide_note(tr, i, board)),
                        TextFont {
                            font_size: FontSize::Px(menu_ui::type_scale::FINE),
                            ..default()
                        },
                        TextLayout::no_wrap(),
                        TextColor(palette::PARCHMENT.with_alpha(0.85)),
                    ));
                    if golden {
                        note.insert((GoldenEntry, Node::default()));
                    }
                }
            });
        });
}

/// The guide belongs on the screens with crabs on them.
pub fn field_guide_visibility(
    screen: Res<State<Screen>>,
    playback: Res<Playback>,
    mut guides: Query<&mut Node, With<FieldGuide>>,
) {
    // An exhaustive match rather than a `matches!`, so a new screen has to
    // say whether crabs are on it.
    let wanted = match screen.get() {
        Screen::Versus | Screen::Puzzle => true,
        Screen::Menu
        | Screen::Editor
        | Screen::Lobby
        | Screen::Settings
        | Screen::Controls
        | Screen::MatchSetup
        | Screen::Achievements
        | Screen::StageSelect
        | Screen::Replays
        | Screen::Interlude
        | Screen::Language
        | Screen::NewVersion => false,
    };
    // The band at the foot of the board holds one row, and while a
    // recording is playing the transport belongs in it: what a crab is
    // worth is a thing to know while routing them, and nobody watching a
    // replay is routing anything.
    let wanted = wanted && playback.0.is_none();
    for mut node in &mut guides {
        menu_ui::set_shown(&mut node, wanted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::Lang;

    /// The guide quotes what a crab is worth on the board in play: a
    /// versus round's called golden crab, not the puzzle's fifty.
    #[test]
    fn the_field_guide_quotes_the_boards_golden_worth() {
        let golden = KINDS
            .iter()
            .position(|&k| k == crate::sim::CrabKind::Golden)
            .expect("the golden crab is in the guide");
        let tr = Lang::En.tr();
        let mut board = crate::sim::Board::new(9, 7, 1);
        assert!(field_guide_note(tr, golden, &board).starts_with("50 "));
        crate::sim::set_versus_rules(&mut board, 240);
        let note = field_guide_note(tr, golden, &board);
        assert!(
            note.starts_with(&format!("{} ", crate::sim::GOLDEN_CALLED_VALUE)),
            "{note}"
        );
    }

    /// The sparkling crab promises a surprise only where its wheel turns.
    #[test]
    fn the_sparkling_crab_promises_nothing_where_the_wheel_is_off() {
        let sparkling = KINDS
            .iter()
            .position(|&k| k == crate::sim::CrabKind::Sparkling)
            .expect("the sparkling crab is in the guide");
        let tr = Lang::En.tr();
        let mut board = crate::sim::Board::new(9, 7, 1);
        board.set_events_enabled(false);
        let worth = board.crab_value(crate::sim::CrabKind::Sparkling);
        assert_eq!(field_guide_note(tr, sparkling, &board), worth.to_string());
        board.set_events_enabled(true);
        let note = field_guide_note(tr, sparkling, &board);
        assert!(note.ends_with(tr.crab_notes[sparkling]), "{note}");
    }
}
