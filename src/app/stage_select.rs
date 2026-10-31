//! The stage list: every puzzle in the current campaign as a numbered tile,
//! cleared ones marked, locked ones greyed out and refused: the original's
//! stage select, re-shelled.

use crate::app::i18n::fill;
use crate::app::progress::Progress;
use crate::app::settings::GameSettings;
use crate::app::{Campaign, PlacementDenied, Screen, i18n, menu_ui, palette};
use bevy::prelude::*;

/// Tiles per row, and it stays ten however long the campaign gets: a row is
/// a decade, so the second row starts at eleven and the seventh at
/// sixty-one. Widening the rows to fit more stages on screen costs that,
/// and it is most of what makes a grid of eighty numbers navigable.
pub const COLS: usize = 10;

/// Height the grid may take inside the card, in pixels: what is left of the
/// gap between the header and the prompt once the count, its bar, the
/// caption and the hint have had theirs. The rows live in a container of
/// their own so this is the only vertical gap that applies to them.
const GRID_ROOM: f32 = 430.0;

const GAP: f32 = 6.0;

/// Vertical room the player's-own-levels heading takes out of the grid,
/// counted out rather than guessed at: 6 of margin, the 1-pixel rule, 4
/// between rule and label, an 18-pixel line of text, and one more [`GAP`]
/// for the row it stands in.
const HEADING_ROOM: f32 = 6.0 + 1.0 + 4.0 + 18.0 + GAP;

/// Which of the two lines under the grid is being built. The loop that
/// makes them is identical either way apart from the marker component, and
/// a bool would not say which was which.
#[derive(Clone, Copy)]
enum Caption {
    Stage,
    Hint,
}

/// The largest tile a grid of `rows` rows may use, once `heading` pixels
/// have been set aside for the player's-own-levels label. Fifty-two is the
/// size the art was drawn for; a long campaign gets whatever fits instead,
/// which is how eighty-two stages stay on one screen without splitting the
/// decades across wider rows.
fn tile(rows: usize, heading: f32) -> f32 {
    let rows = rows.max(1) as f32;
    ((GRID_ROOM - heading - (rows - 1.0) * GAP) / rows).min(52.0)
}

/// The grid row by row: the shipped campaign in decades, then the player's
/// own levels in decades of their own. The two lists never share a row, so
/// the seam is visible without counting, and a fifth custom level cannot
/// end up sitting next to the last shipped one.
#[cfg(test)]
pub fn rows(builtins: usize, total: usize) -> Vec<std::ops::Range<usize>> {
    let builtins = builtins.min(total);
    rows_of(&[0..builtins, builtins..total])
}

/// [`rows`] over any run of sections, each starting a row of its own: the
/// Tide Pool, the co-op stages and the player's own levels.
pub fn rows_of(sections: &[std::ops::Range<usize>]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    for section in sections {
        for first in section.clone().step_by(COLS) {
            out.push(first..(first + COLS).min(section.end));
        }
    }
    out
}

/// The grid's sections in the order drawn, each with the heading over it
/// (none over the Tide Pool, which the header names).
fn sections(campaign: &Campaign) -> [(crate::app::campaign::Section, std::ops::Range<usize>); 3] {
    use crate::app::campaign::Section;
    [Section::TidePool, Section::Coop, Section::Custom].map(|s| (s, campaign.section(s)))
}

/// Is this one of the player's own levels rather than a shipped stage?
pub fn is_custom(campaign: &Campaign, index: usize) -> bool {
    index >= campaign.builtins
}

/// The number a tile wears. The shipped campaign counts from one, and the
/// player's own levels count from one again: they are a shelf of their own,
/// not stages eighty-three onward.
pub fn tile_number(campaign: &Campaign, index: usize) -> usize {
    campaign.place_of(index).at
}

#[derive(Resource, Default)]
pub struct StageList {
    pub selected: usize,
    /// Where a puzzle sending the player back asks the list to open,
    /// rather than on the furthest open stage. Taken on entering.
    pub landing: Option<Landing>,
    /// The grid as it was drawn, one range per row. Kept rather than
    /// recomputed: the tiles are spawned once on entering and nothing
    /// reshuffles them while the screen is up.
    rows: Vec<std::ops::Range<usize>>,
}

/// A stage a puzzle sends the player back to the list on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Landing {
    /// The stage the cursor lands on.
    pub at: usize,
    /// Whether the list says it needs two players: Enter on a won card or
    /// N in setup asked for it with one seated, and was sent here instead.
    pub needs_two: bool,
}

/// The way back to the stage list from a puzzle, landing where asked.
#[derive(bevy::ecs::system::SystemParam)]
pub struct BackToTheList<'w> {
    list: ResMut<'w, StageList>,
    next_screen: ResMut<'w, NextState<Screen>>,
}

impl BackToTheList<'_> {
    /// Back to the list, opening on `landing`, else where it would.
    pub fn go(&mut self, landing: Option<Landing>) {
        self.list.landing = landing;
        self.next_screen.set(Screen::StageSelect);
    }
}

#[derive(Component)]
pub struct StageSelectUi;

/// One tile, tagged with the level index it stands for.
#[derive(Component)]
pub struct StageTile(usize);

/// A tile's number, tagged so the cursor can brighten it.
#[derive(Component)]
pub struct StageNumber(usize);

/// The line under the grid: which stage the cursor is on, and its state.
#[derive(Component)]
pub struct StageCaption;

/// The teaching hint for the stage under the cursor, when it has one.
#[derive(Component)]
pub struct StageHint;

/// The hint line and the pill it rides. A named type because it wants both
/// the text and the node, and a `Without` to stay disjoint from the
/// caption's query over the same two components.
type HintLine<'w, 's> =
    Query<'w, 's, (&'static mut Text, &'static mut Node), (With<StageHint>, Without<StageCaption>)>;

/// How a stage reads on the grid. Locked stages are shown, not hidden:
/// seeing what is still ahead is half the point of a stage list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TileState {
    Cleared,
    Open,
    Locked,
}

/// How hard a stage is, read off the one number the author already had to
/// choose: how many signposts it grants. One is a lesson, four is a proof,
/// five is the top of the campaign. A no-signpost stage is a
/// watch-it-happen stage and gets its own colour rather than being lumped
/// in with the easiest.
///
/// The key under the grid draws one swatch per value in [`KEY_POSTS`];
/// `the_key_reaches_the_hardest_stage` keeps that range and this match in
/// step with the campaign, because the fallback arm is silent: the first
/// five-post levels shipped drawn in the four-post red.
pub fn difficulty_ink(posts: u8) -> Color {
    match posts {
        0 => palette::INK_TIDE,
        1 => palette::INK_ONE_POST,
        2 => palette::GOLD,
        3 => palette::INK_THREE_POSTS,
        4 => palette::INK_RAID,
        _ => palette::INK_LURE,
    }
}

/// The signpost counts the key under the grid explains, lowest first.
pub const KEY_POSTS: std::ops::RangeInclusive<u8> = 1..=5;

impl TileState {
    fn of(progress: &Progress, campaign: &Campaign, index: usize) -> TileState {
        if progress.level_cleared(&campaign.levels[index]) {
            TileState::Cleared
        } else if progress.unlocked(campaign, index) {
            TileState::Open
        } else {
            TileState::Locked
        }
    }

    /// Tile fill and number ink. The border is not here: it carries how
    /// hard the stage is rather than how far you have got, and the two are
    /// independent. State is the fill, difficulty is the edge.
    fn colors(self) -> (Color, Color) {
        match self {
            TileState::Cleared => (palette::GOLD.with_alpha(0.28), palette::TILE_CLEARED_INK),
            TileState::Open => (palette::TILE_OPEN_FILL, palette::IDLE_ROW),
            TileState::Locked => (palette::TILE_LOCKED_FILL, palette::TILE_LOCKED_INK),
        }
    }

    /// How strongly the difficulty edge shows: full on a stage you can
    /// play, a hint of it on one still shut.
    fn edge_alpha(self) -> f32 {
        match self {
            TileState::Cleared | TileState::Open => 0.95,
            TileState::Locked => 0.30,
        }
    }
}

/// Grid movement, clamped rather than wrapping: a stage list is a map, and
/// walking off the end of it should not teleport you to the far corner.
///
/// Up and down walk the rows the grid actually drew rather than jumping ten
/// indices: the player's own levels start a row of their own wherever the
/// shipped campaign happens to end, so a fixed stride would land the cursor
/// mid-row - or, from the last shipped row, skip the custom shelf entirely.
pub fn step(
    selected: usize,
    rows: &[std::ops::Range<usize>],
    keys: &ButtonInput<KeyCode>,
) -> usize {
    // No rows means no grid has been drawn to walk, so the cursor stays
    // where it was put. Returning zero would quietly re-home it, and the
    // player would press Enter on a stage they are not looking at.
    let Some(last) = rows.last().map(|row| row.end - 1) else {
        return selected;
    };
    let pressed = |a: KeyCode, b: KeyCode| keys.just_pressed(a) || keys.just_pressed(b);
    let mut at = selected.min(last);
    if pressed(KeyCode::KeyA, KeyCode::ArrowLeft) {
        at = at.saturating_sub(1);
    }
    if pressed(KeyCode::KeyD, KeyCode::ArrowRight) {
        at = (at + 1).min(last);
    }
    let row = rows.iter().position(|row| row.contains(&at)).unwrap_or(0);
    let column = at - rows[row].start;
    // A ragged row is shorter than the one above it: fall to its end rather
    // than past it, keeping the column where the row is wide enough.
    let mut land = |row: usize| {
        let row = &rows[row];
        at = (row.start + column).min(row.end - 1);
    };
    if pressed(KeyCode::KeyW, KeyCode::ArrowUp) && row > 0 {
        land(row - 1);
    }
    if pressed(KeyCode::KeyS, KeyCode::ArrowDown) && row + 1 < rows.len() {
        land(row + 1);
    }
    at
}

/// The caption for the stage under the cursor: its number, its name, and
/// what it wants, or what still stands in the way.
pub fn caption(
    tr: &i18n::Tr,
    lang: i18n::Lang,
    campaign: &Campaign,
    state: TileState,
    index: usize,
) -> String {
    let level = &campaign.levels[index];
    let tail = match state {
        TileState::Cleared => tr.stage_cleared.to_string(),
        // A no-signpost stage is a watch-it-happen stage; saying
        // "signposts: 0" would read as a missing number.
        TileState::Open if level.posts == 0 => tr.stage_free.to_string(),
        // A co-op stage says it is for two, and what each hand holds.
        TileState::Open if let Some([left, right]) = level.hands() => fill(
            &crate::app::name_the_pair(tr, tr.stage_open_hands),
            &[("a", &left.to_string()), ("b", &right.to_string())],
        ),
        TileState::Open => fill(tr.stage_open, &[("n", &level.posts.to_string())]),
        TileState::Locked => locked_tail(tr, campaign, index),
    };
    // Both shelves count from one, so the caption says which shelf it is
    // reading: the heading that says so on the grid is too far from this
    // line to carry it.
    let shelf = match campaign.section_of(index) {
        crate::app::campaign::Section::Custom => format!("{} ", tr.stage_custom),
        crate::app::campaign::Section::Coop => format!("{} ", tr.stage_coop),
        crate::app::campaign::Section::TidePool => String::new(),
    };
    format!(
        "{shelf}{}. {} - {tail}",
        tile_number(campaign, index),
        campaign.shown_name(index, lang)
    )
}

/// What a locked stage says stands in its way: the stage before it on its
/// own shelf ([`Progress::unlocked`] opens a stage on the clear of the one
/// at `index - 1`), by the number that shelf gives it, and on the co-op
/// shelf the shelf too, as the stage list's tiles number it.
fn locked_tail(tr: &i18n::Tr, campaign: &Campaign, index: usize) -> String {
    use crate::app::campaign::Section;
    let before = campaign.place_of(index.saturating_sub(1));
    let template = match before.section {
        Section::Coop => tr.stage_locked_coop,
        Section::TidePool | Section::Custom => tr.stage_locked,
    };
    fill(template, &[("n", &before.at.to_string())])
}

pub fn enter_stage_select(
    mut commands: Commands,
    mut campaign: ResMut<Campaign>,
    mut progress: ResMut<Progress>,
    settings: Res<GameSettings>,
    mut list: ResMut<StageList>,
    mut notice: ResMut<crate::app::RoundNotice>,
) {
    // Whatever the menu last said is about the menu.
    notice.0.clear();
    let landing = list.landing.take();
    // A stage played from a pasted code was put on the list for its one
    // go, and is no stage of the list's: the list is the shelf as it is
    // filed. Left on, it was offered as the player's own, with an Edit and
    // a Copy that found no file.
    let landing = match campaign.holds_a_code() {
        true => {
            campaign.reset(crate::app::campaign::stage_list());
            // The list just changed under it.
            None
        }
        false => landing.filter(|landing| landing.at < campaign.levels.len()),
    };
    // A tick an older build filed under a level's name is filed under its
    // file before the list is drawn, and kept there: a later file of the
    // same name would leave the name meaning neither.
    if progress.adopt_legacy(&campaign) {
        crate::app::progress::save(&mut progress);
    }
    let tr = settings.tr();
    // Land the cursor where the player left off, not back at stage one:
    // on the stage a puzzle sent them back on, saying why when it was
    // refused them, else on the furthest open stage of the ladder.
    list.selected = match landing {
        Some(Landing { at, needs_two }) => {
            if needs_two {
                notice.0 = crate::app::name_the_pair(tr, tr.stage_needs_two);
            }
            at
        }
        None => progress.furthest_open(&campaign),
    };
    list.rows = rows_of(&sections(&campaign).map(|(_, range)| range));
    commands
        .spawn((
            StageSelectUi,
            // Fills the space between the header and the prompt line, and
            // centres the grid in it: the list is the whole screen here.
            Node {
                // Wider than the shared frame's, and said out loud: the
                // grid and its key want air between them.
                row_gap: Val::Px(12.0),
                ..menu_ui::between_bars()
            },
        ))
        .with_children(|wrap| {
            wrap.spawn(Node {
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|line| {
                // The shared card, holding a grid rather than a list, so
                // it wants more air around it than a run of rows does.
                // `bg` and not `fill`: this file's `fill` is the one that
                // puts words into a translated string.
                let (mark, mut node, bg, edge, shadow) = menu_ui::screen_card();
                node.row_gap = Val::Px(12.0);
                node.padding = UiRect::axes(Val::Px(28.0), Val::Px(20.0));
                line.spawn((mark, node, bg, edge, shadow))
                    .with_children(|column| {
                        // The count and its bar are the shipped campaign's, which is
                        // the thing that can be finished. Levels the player keeps
                        // adding would make the denominator a moving target, and
                        // "82 of 93" would read as an unfinished campaign forever.
                        let shipped = campaign.section(crate::app::campaign::Section::TidePool);
                        let total = shipped.len();
                        let done = progress.cleared_in(&campaign, shipped);
                        column.spawn((
                            Text::new(fill(
                                tr.stage_progress,
                                &[("a", &done.to_string()), ("b", &total.to_string())],
                            )),
                            TextFont {
                                font_size: FontSize::Px(menu_ui::type_scale::ROW),
                                ..default()
                            },
                            TextColor(palette::PARCHMENT.with_alpha(0.75)),
                        ));
                        // The same count as a bar. Eighty-two gold tiles scattered
                        // through a grid do not add up to a feeling of progress on
                        // their own; one line across the top does.
                        let filled = if total == 0 {
                            0.0
                        } else {
                            done as f32 / total as f32 * 100.0
                        };
                        column.spawn((
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(4.0),
                                border_radius: BorderRadius::all(Val::Px(2.0)),
                                ..default()
                            },
                            BackgroundColor(palette::BAR_TRACK),
                            children![(
                                Node {
                                    width: Val::Percent(filled),
                                    height: Val::Percent(100.0),
                                    border_radius: BorderRadius::all(Val::Px(2.0)),
                                    ..default()
                                },
                                BackgroundColor(palette::GOLD),
                            )],
                        ));
                        // The same rows the cursor will walk, drawn from the one
                        // layout the screen agreed on above.
                        let layout = &list.rows;
                        // The co-op stages and the player's own levels are
                        // shelves under the campaign, each with a label of its
                        // own: they are not stage a hundred and one.
                        let shelves: Vec<_> = sections(&campaign)
                            .into_iter()
                            .filter(|(section, range)| {
                                *section != crate::app::campaign::Section::TidePool
                                    && !range.is_empty()
                            })
                            .collect();
                        let heading = HEADING_ROOM * shelves.len() as f32;
                        let edge = tile(layout.len(), heading);
                        column
                            .spawn(Node {
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                row_gap: Val::Px(GAP),
                                ..default()
                            })
                            .with_children(|grid| {
                                for row in layout {
                                    if let Some((section, range)) =
                                        shelves.iter().find(|(_, range)| range.start == row.start)
                                    {
                                        let label = match section {
                                            crate::app::campaign::Section::Coop => tr.stage_coop,
                                            crate::app::campaign::Section::Custom
                                            | crate::app::campaign::Section::TidePool => {
                                                tr.stage_custom
                                            }
                                        };
                                        let count = fill(
                                            tr.stage_progress,
                                            &[
                                                (
                                                    "a",
                                                    &progress
                                                        .cleared_in(&campaign, range.clone())
                                                        .to_string(),
                                                ),
                                                ("b", &range.len().to_string()),
                                            ],
                                        );
                                        spawn_shelf_heading(grid, label, count);
                                    }
                                    grid.spawn(Node {
                                        column_gap: Val::Px(GAP),
                                        ..default()
                                    })
                                    .with_children(|line| {
                                        for index in row.clone() {
                                            spawn_tile(line, &campaign, &progress, index, edge);
                                        }
                                    });
                                }
                            });
                        // The key to the edges. Colours nobody can decode are just
                        // noise, and the number under each swatch is the whole
                        // rule: that many signposts, no more.
                        column
                            .spawn(Node {
                                column_gap: Val::Px(14.0),
                                align_items: AlignItems::Center,
                                margin: UiRect::top(Val::Px(4.0)),
                                ..default()
                            })
                            .with_children(|key| {
                                key.spawn((
                                    Text::new(tr.stage_key.to_string()),
                                    TextFont {
                                        font_size: FontSize::Px(menu_ui::type_scale::FINE),
                                        ..default()
                                    },
                                    TextColor(palette::PARCHMENT.with_alpha(0.45)),
                                ));
                                for posts in KEY_POSTS {
                                    key.spawn(Node {
                                        column_gap: Val::Px(5.0),
                                        align_items: AlignItems::Center,
                                        ..default()
                                    })
                                    .with_children(|item| {
                                        item.spawn((
                                            Node {
                                                width: Val::Px(10.0),
                                                height: Val::Px(10.0),
                                                border: UiRect::all(Val::Px(2.0)),
                                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                                ..default()
                                            },
                                            BackgroundColor(Color::NONE),
                                            BorderColor::all(difficulty_ink(posts)),
                                        ));
                                        item.spawn((
                                            Text::new(posts.to_string()),
                                            TextFont {
                                                font_size: FontSize::Px(menu_ui::type_scale::FINE),
                                                ..default()
                                            },
                                            TextColor(palette::PARCHMENT.with_alpha(0.60)),
                                        ));
                                    });
                                }
                            });
                    });
            });
            // Both lines sit under the card, not in it: inside they would
            // size it, and the box would breathe as you walked across the
            // grid. Out here they get the whole screen's width, which the
            // longest German caption needs, and their own fixed rows.
            wrap.spawn(Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|lines| {
                // Four pixels taller apiece than the line they hold, which
                // is the room the pill's own padding takes: the row clips,
                // and a pill that does not fit is a pill with its ends cut
                // off.
                for (height, font, color, tag) in [
                    (30.0, 20.0, palette::SELECTED_ROW, Caption::Stage),
                    (
                        26.0,
                        17.0,
                        palette::SELECTED_ROW.with_alpha(0.8),
                        Caption::Hint,
                    ),
                ] {
                    lines
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(height),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            overflow: Overflow::clip(),
                            ..default()
                        })
                        .with_children(|line| {
                            // Both lines ride a pill, for the reason the
                            // prompt's own does: out here they are over the
                            // open beach, and pale text on bright sand is
                            // the one place this interface goes illegible.
                            let mut text = line.spawn((
                                Text::new(String::new()),
                                TextFont {
                                    font_size: FontSize::Px(font),
                                    ..default()
                                },
                                TextLayout::no_wrap(),
                                TextColor(color),
                                Node {
                                    padding: UiRect::axes(Val::Px(12.0), Val::Px(3.0)),
                                    border_radius: BorderRadius::all(Val::Px(11.0)),
                                    ..default()
                                },
                                BackgroundColor(palette::PILL_FILL),
                            ));
                            match tag {
                                Caption::Stage => text.insert(StageCaption),
                                Caption::Hint => text.insert(StageHint),
                            };
                        });
                }
            });
        });
}

/// The label over a shelf: a rule, the name of the shelf, and how much of
/// it is cleared. Without it the co-op stages and the custom levels read as
/// more campaign - which is what the numbering said before they got their
/// own.
fn spawn_shelf_heading(grid: &mut ChildSpawnerCommands, label: &str, count: String) {
    grid.spawn(Node {
        width: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: Val::Px(4.0),
        margin: UiRect::top(Val::Px(6.0)),
        ..default()
    })
    .with_children(|shelf| {
        shelf.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(1.0),
                ..default()
            },
            BackgroundColor(palette::PARCHMENT.with_alpha(0.14)),
        ));
        shelf
            .spawn(Node {
                column_gap: Val::Px(8.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|line| {
                line.spawn((
                    Text::new(label.to_string()),
                    TextFont {
                        font_size: FontSize::Px(menu_ui::type_scale::FINE),
                        ..default()
                    },
                    TextColor(palette::PARCHMENT.with_alpha(0.60)),
                ));
                line.spawn((
                    Text::new(count),
                    TextFont {
                        font_size: FontSize::Px(menu_ui::type_scale::FINE),
                        ..default()
                    },
                    TextColor(palette::PARCHMENT.with_alpha(0.40)),
                ));
            });
    });
}

fn spawn_tile(
    line: &mut ChildSpawnerCommands,
    campaign: &Campaign,
    progress: &Progress,
    index: usize,
    size: f32,
) {
    let state = TileState::of(progress, campaign, index);
    let (fill, ink) = state.colors();
    let border = difficulty_ink(campaign.levels[index].posts).with_alpha(state.edge_alpha());
    let mark = campaign.levels[index].is_coop().then(|| CoopMark::on(size));
    line.spawn((
        StageTile(index),
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            border: UiRect::all(Val::Px(TILE_BORDER)),
            // The badge's corner is kept clear of the number: on a small
            // tile a two-digit number reaches the corner, so the number is
            // centred in what is left under the badge.
            padding: UiRect::top(Val::Px(mark.map_or(0.0, CoopMark::room))),
            border_radius: BorderRadius::all(Val::Px(size * 0.17)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(fill),
        BorderColor::all(border),
    ))
    .with_children(|tile| {
        tile.spawn((
            StageNumber(index),
            Text::new(tile_number(campaign, index).to_string()),
            TextFont {
                font_size: FontSize::Px(number_px(size)),
                ..default()
            },
            TextColor(ink),
        ));
        // A stage for two wears it at the top, wherever it is shelved:
        // a co-op stage of the player's own sits among solo ones.
        let paint = palette::seat_paint(1).with_alpha(0.95);
        match mark {
            Some(CoopMark::Badge(px)) => {
                tile.spawn((
                    Text::new(COOP_BADGE),
                    TextFont {
                        font_size: FontSize::Px(px),
                        ..default()
                    },
                    TextColor(paint),
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(0.0),
                        right: Val::Px(2.0),
                        ..default()
                    },
                ));
            }
            Some(CoopMark::Strip(height)) => {
                tile.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(0.0),
                        left: Val::Percent(20.0),
                        width: Val::Percent(60.0),
                        height: Val::Px(height),
                        ..default()
                    },
                    BackgroundColor(paint),
                ));
            }
            None => {}
        }
    });
}

/// The border every stage tile is drawn with, inside its edge.
const TILE_BORDER: f32 = 2.0;

/// How much of a font size a line of text stands in: Bevy's default line
/// height.
const LINE: f32 = 1.2;

/// The smallest the 2P badge is drawn: below this it is not read as text.
const BADGE_MIN_PX: f32 = 8.0;

/// A tile too small for the badge wears a bar this tall instead, or as
/// tall as the gap over its number allows, but never less than
/// [`STRIP_MIN_PX`].
const STRIP_PX: f32 = 2.0;

/// The least the bar is drawn, at any size: a co-op tile with no mark
/// reads as a solo one. A tile this small (a shelf of some 140 levels of
/// the player's own) has no gap over its number left, and its number is
/// under four pixels tall, so the bar keeps its pixel over it.
const STRIP_MIN_PX: f32 = 1.0;

/// The size a tile's number is drawn at. It has to stay legible in
/// whatever the tile shrank to, so it shrinks with it rather than by
/// itself.
fn number_px(size: f32) -> f32 {
    (size * 0.4).min(20.0)
}

/// What marks a co-op stage's tile at a given size: the 2P badge while the
/// badge and the number both fit, one over the other, inside the border;
/// on a tile too small for that (a long shelf of the player's own levels
/// shrinks every tile), a bar in P2's colour along the top edge, which
/// still says "two players" and never touches the number.
#[derive(Clone, Copy, Debug, PartialEq)]
enum CoopMark {
    Badge(f32),
    Strip(f32),
}

impl CoopMark {
    fn on(size: f32) -> CoopMark {
        let badge = (size * 0.24).clamp(BADGE_MIN_PX, 12.0);
        let inner = size - 2.0 * TILE_BORDER;
        let number = LINE * number_px(size);
        match LINE * badge + number <= inner {
            true => CoopMark::Badge(badge),
            false => CoopMark::Strip(((inner - number) / 2.0).clamp(STRIP_MIN_PX, STRIP_PX)),
        }
    }

    /// The room kept at the top of the tile for the mark, which the number
    /// is centred below.
    fn room(self) -> f32 {
        match self {
            CoopMark::Badge(px) => LINE * px,
            CoopMark::Strip(_) => 0.0,
        }
    }
}

/// The corner mark on a co-op stage's tile: two players, in the words every
/// language's couch games already use.
pub const COOP_BADGE: &str = "2P";

/// What the stage list draws: the tiles and their numbers, the caption
/// under the grid, and the hint line.
#[derive(bevy::ecs::system::SystemParam)]
pub struct StageUi<'w, 's> {
    tiles: Query<
        'w,
        's,
        (
            &'static StageTile,
            &'static mut BorderColor,
            &'static mut BackgroundColor,
        ),
    >,
    numbers: Query<'w, 's, (&'static StageNumber, &'static mut TextColor)>,
    captions: Query<'w, 's, (&'static mut Text, &'static mut Node), With<StageCaption>>,
    hints: HintLine<'w, 's>,
}

/// Paint the selection ring and the caption. The tile states themselves are
/// fixed while the screen is up: nothing gets cleared from in here.
pub fn update_stage_tiles(
    list: Res<StageList>,
    campaign: Res<Campaign>,
    progress: Res<Progress>,
    settings: Res<GameSettings>,
    coop: Res<crate::app::Coop>,
    (caps, device): (
        Res<crate::app::keycaps::KeyCaps>,
        Res<crate::app::gamepad::LastDevice>,
    ),
    ui: StageUi,
) {
    let StageUi {
        mut tiles,
        mut numbers,
        mut captions,
        mut hints,
    } = ui;
    for (tile, mut border, mut fill) in &mut tiles {
        let state = TileState::of(&progress, &campaign, tile.0);
        let picked = tile.0 == list.selected;
        let target = if picked {
            palette::SELECTED_ROW
        } else {
            difficulty_ink(campaign.levels[tile.0].posts).with_alpha(state.edge_alpha())
        };
        if border.top != target {
            *border = BorderColor::all(target);
        }
        // The cursor also lifts the tile it stands on: a ring alone reads as
        // one more colour in a grid that already has three.
        let ground = if picked {
            palette::SELECTED_ROW.with_alpha(0.55)
        } else {
            state.colors().0
        };
        menu_ui::set_bg(&mut fill, ground);
    }
    // The cursor lifts its number out of the row, locked or not: a greyed
    // tile you are standing on still has to be readable. Ink dark, because
    // the tile under it is now the brightest thing on the screen.
    for (number, mut color) in &mut numbers {
        let target = if number.0 == list.selected {
            palette::CARD_FILL.with_alpha(1.0)
        } else {
            TileState::of(&progress, &campaign, number.0).colors().1
        };
        menu_ui::set_color(&mut color, target);
    }
    let state = TileState::of(&progress, &campaign, list.selected);
    if let Ok((mut text, mut node)) = captions.single_mut() {
        let line = caption(
            settings.tr(),
            settings.language,
            &campaign,
            state,
            list.selected,
        );
        menu_ui::set_text(&mut text, &line);
        menu_ui::set_shown(&mut node, !line.is_empty());
    }
    // A locked stage keeps its lesson to itself; the caption already says
    // what to do about it. Two thirds of the campaign has no hint either,
    // so the line is empty more often than not - and the pill goes with
    // it, rather than sitting on the sand as an empty lozenge.
    if let Ok((mut text, mut node)) = hints.single_mut() {
        let hint = match state {
            TileState::Locked => String::new(),
            // Asked as the puzzle's own line asks it: the key lesson is
            // kept back under keys it does not describe.
            TileState::Cleared | TileState::Open => caps.legend(
                &campaign
                    .shown_hint(list.selected, &settings, *coop, *device)
                    .unwrap_or_default(),
            ),
        };
        menu_ui::set_text(&mut text, &hint);
        menu_ui::set_shown(&mut node, !hint.is_empty());
    }
}

/// Where a stage of the player's own goes from the stage list: into the
/// editor, or onto the clipboard as a code. Said in the header's status
/// slot (`RoundNotice`).
#[derive(bevy::ecs::system::SystemParam)]
pub struct OwnStage<'w> {
    opening: ResMut<'w, crate::app::editor::EditorOpening>,
    clipboard: ResMut<'w, Clipboard>,
    notice: ResMut<'w, crate::app::RoundNotice>,
    settings: Res<'w, GameSettings>,
    shared: MessageWriter<'w, crate::app::CodeShared>,
}

/// Leaving the list takes what it said with it: the notice is the list's
/// status line there and the menu's after it, so "a co-op stage needs two"
/// otherwise greeted the player back on the menu after Esc.
pub fn exit_stage_select(mut notice: ResMut<crate::app::RoundNotice>) {
    notice.0.clear();
}

/// Arrows move, Enter plays an open stage, a locked one just says no, and
/// C turns co-op on and off. On a stage of the player's own, E opens it in
/// the editor and F3 copies its code, as F3 does there. A pad's buttons
/// come through the bridge as these keys (`gamepad::scheme`).
pub fn stage_select_input(
    keyboard: crate::app::keycaps::Keyboard,
    mut ladder: crate::app::progress::Ladder,
    mut coop: ResMut<crate::app::Coop>,
    mut list: ResMut<StageList>,
    mut denied: MessageWriter<PlacementDenied>,
    mut next_screen: ResMut<NextState<Screen>>,
    mut own: OwnStage,
) {
    let keys = &keyboard.keys;
    // Read the layout, then move: the rows are settled and the cursor is
    // the only thing this system changes.
    let at = step(list.selected, &list.rows, keys);
    if at != list.selected {
        // What was said was about the stage left.
        own.notice.0.clear();
    }
    list.selected = at;
    if keyboard.global('C') {
        coop.0 = !coop.0;
        // Whatever was said, "a co-op stage needs two" above all, was said
        // to the table as it was seated.
        own.notice.0.clear();
    }
    if keys.just_pressed(KeyCode::Escape) {
        next_screen.set(Screen::Menu);
        return;
    }
    let edit = keyboard.just('E');
    let copy = keys.just_pressed(KeyCode::F3);
    if (edit || copy) && is_custom(&ladder.campaign, at) {
        own_stage(
            &ladder.campaign.levels[at],
            (edit, copy),
            own,
            &mut next_screen,
        );
        return;
    }
    if !menu_ui::enter(keys) {
        return;
    }
    if ladder.campaign.needs_partner(list.selected, *coop) {
        let tr = own.settings.tr();
        own.notice.0 = crate::app::name_the_pair(tr, tr.stage_needs_two);
        denied.write(PlacementDenied {
            player: 0,
            out_of_signposts: false,
        });
        return;
    }
    if ladder.climb_to(list.selected) {
        next_screen.set(Screen::Puzzle);
    } else {
        denied.write(PlacementDenied {
            player: 0,
            out_of_signposts: false,
        });
    }
}

/// E or F3 on a stage of the player's own: the editor opened on its file,
/// read afresh (the list may show it renamed beside a namesake), or its
/// code copied.
fn own_stage(
    level: &crate::sim::Level,
    (edit, copy): (bool, bool),
    own: OwnStage,
    next_screen: &mut NextState<Screen>,
) {
    let OwnStage {
        mut opening,
        mut clipboard,
        mut notice,
        settings,
        mut shared,
    } = own;
    let tr = settings.tr();
    let opened = crate::app::campaign::filed_path(level)
        .and_then(|path| crate::app::editor::opening_from_file(&path));
    let Some(opened) = opened else {
        notice.0 = fill(tr.code_level_bad, &[("e", &level.name)]);
        return;
    };
    if edit {
        opening.0 = Some(opened);
        next_screen.set(Screen::Editor);
    } else if copy {
        let (said, took) = crate::app::codes::copy_counted(
            &mut clipboard,
            tr,
            crate::share::Kind::Level,
            opened.level.to_text().as_bytes(),
            tr.code_copied,
        );
        notice.0 = said;
        if took {
            shared.write(crate::app::CodeShared);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::EN;

    fn campaign() -> Campaign {
        Campaign::tide_pool()
    }

    fn press(key: KeyCode) -> ButtonInput<KeyCode> {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(key);
        keys
    }

    /// The grid clamps at every edge, and a down-press from the ragged last
    /// row lands on the final stage instead of running off the end.
    #[test]
    fn the_grid_clamps_at_its_edges() {
        let len = 25; // two full rows of ten and a short third
        let grid = rows(len, len);
        assert_eq!(step(0, &grid, &press(KeyCode::ArrowLeft)), 0);
        assert_eq!(step(0, &grid, &press(KeyCode::ArrowUp)), 0);
        assert_eq!(step(0, &grid, &press(KeyCode::ArrowRight)), 1);
        assert_eq!(step(0, &grid, &press(KeyCode::ArrowDown)), COLS);
        assert_eq!(step(24, &grid, &press(KeyCode::ArrowRight)), 24);
        assert_eq!(step(24, &grid, &press(KeyCode::ArrowDown)), 24);
        assert_eq!(
            step(18, &grid, &press(KeyCode::ArrowDown)),
            24,
            "ragged row"
        );
        assert_eq!(step(24, &grid, &press(KeyCode::ArrowUp)), 14);
        assert_eq!(
            step(99, &grid, &press(KeyCode::ArrowRight)),
            24,
            "stale index"
        );
        // Nothing drawn yet: the cursor is left exactly where it was,
        // rather than being sent home to a stage nobody chose.
        assert_eq!(
            step(3, &rows(0, 0), &press(KeyCode::ArrowRight)),
            3,
            "no grid to walk"
        );
    }

    /// The player's own levels start a row of their own, wherever the
    /// shipped campaign happens to end.
    #[test]
    fn the_two_shelves_never_share_a_row() {
        assert_eq!(rows(25, 25), vec![0..10, 10..20, 20..25]);
        // Twelve shipped, three custom: the shipped list keeps its ragged
        // row and the three start fresh below it.
        assert_eq!(rows(12, 15), vec![0..10, 10..12, 12..15]);
        assert_eq!(rows(0, 3), vec![0..3], "nothing shipped, all yours");
        assert_eq!(rows(5, 5), vec![0..5], "nothing of yours yet");
        assert_eq!(rows(9, 4), vec![0..4], "a builtins count past the end");
    }

    /// Walking down off the last shipped row lands on the custom shelf
    /// rather than skipping it, and walking back up returns to the campaign.
    #[test]
    fn the_cursor_walks_between_the_shelves() {
        // Twelve shipped (a row of ten and a row of two), three of yours.
        let grid = rows(12, 15);
        assert_eq!(step(1, &grid, &press(KeyCode::ArrowDown)), 11, "into row 2");
        assert_eq!(
            step(11, &grid, &press(KeyCode::ArrowDown)),
            13,
            "onto the custom shelf, column kept"
        );
        assert_eq!(
            step(5, &grid, &press(KeyCode::ArrowDown)),
            11,
            "ragged row: falls to its end"
        );
        assert_eq!(
            step(14, &grid, &press(KeyCode::ArrowUp)),
            11,
            "back up into the campaign"
        );
        assert_eq!(step(13, &grid, &press(KeyCode::ArrowDown)), 13, "last row");
        assert_eq!(
            step(11, &grid, &press(KeyCode::ArrowRight)),
            12,
            "right walks on into the shelf below"
        );
    }

    /// Both shelves count from one, and the caption says which shelf it is
    /// reading so the two ones cannot be confused.
    #[test]
    fn the_custom_shelf_numbers_itself() {
        let mut campaign = campaign();
        let builtins = campaign.builtins;
        // A level of the player's own, which is any level sitting past the
        // shipped ones: where it came from is not what makes it theirs.
        let mut mine = campaign.levels[0].clone();
        mine.name = "Driftwood Arena".into();
        campaign.levels.push(mine);

        assert!(!is_custom(&campaign, builtins - 1));
        assert!(is_custom(&campaign, builtins));
        assert_eq!(tile_number(&campaign, 0), 1);
        assert_eq!(tile_number(&campaign, builtins - 1), builtins);
        assert_eq!(tile_number(&campaign, builtins), 1, "yours count from one");

        let line = caption(&EN, i18n::Lang::En, &campaign, TileState::Open, builtins);
        assert!(line.starts_with(EN.stage_custom), "{line}");
        assert!(line.contains("1. Driftwood Arena"), "{line}");
        // A shipped stage says nothing about the shelf.
        let shipped = caption(&EN, i18n::Lang::En, &campaign, TileState::Open, 0);
        assert!(shipped.starts_with("1. Welcome Ashore"), "{shipped}");
    }

    /// A stage played from a code pasted on the menu is gone from the list
    /// when the list comes back up: it is on no shelf, and offered as the
    /// player's own it had an Edit and a Copy with no file behind them.
    #[test]
    fn a_stage_from_a_code_leaves_the_list() {
        use bevy::ecs::system::RunSystemOnce;
        let mut campaign = campaign();
        let mut pasted = campaign.levels[0].clone();
        pasted.name = "Pasted Probe".into();
        pasted.filing = None;
        campaign.levels.push(pasted);
        campaign.index = campaign.levels.len() - 1;
        assert!(campaign.holds_a_code());
        let mut app = App::new();
        app.insert_resource(campaign);
        app.init_resource::<Progress>();
        app.init_resource::<StageList>();
        app.init_resource::<GameSettings>();
        app.init_resource::<crate::app::RoundNotice>();
        let _ = app.world_mut().run_system_once(enter_stage_select);
        let campaign = app.world().resource::<Campaign>();
        assert!(!campaign.holds_a_code());
        assert!(
            campaign
                .levels
                .iter()
                .all(|level| level.name != "Pasted Probe")
        );
    }

    /// A puzzle sending the player back on a stage brings the list up on
    /// it, saying it needs two when that is why; the landing is spent, and
    /// the next way in finds the furthest open stage again.
    #[test]
    fn the_list_lands_where_a_puzzle_sent_the_player() {
        use bevy::ecs::system::RunSystemOnce;
        let campaign = campaign();
        let at = campaign.levels.len() - 1;
        let mut app = App::new();
        app.insert_resource(campaign);
        app.init_resource::<Progress>();
        app.insert_resource(StageList {
            landing: Some(Landing {
                at,
                needs_two: true,
            }),
            ..StageList::default()
        });
        app.init_resource::<GameSettings>();
        app.init_resource::<crate::app::RoundNotice>();
        let _ = app.world_mut().run_system_once(enter_stage_select);
        let list = app.world().resource::<StageList>();
        assert_eq!((list.selected, list.landing), (at, None));
        assert_eq!(
            app.world().resource::<crate::app::RoundNotice>().0,
            crate::app::name_the_pair(&EN, EN.stage_needs_two)
        );
        let _ = app.world_mut().run_system_once(enter_stage_select);
        assert_eq!(app.world().resource::<StageList>().selected, 0);
        assert!(
            app.world()
                .resource::<crate::app::RoundNotice>()
                .0
                .is_empty()
        );
    }

    /// What a stage of the player's own goes to: the editor, the
    /// clipboard and the line that says how that went.
    fn own_stage_resources(app: &mut App) {
        app.init_resource::<crate::app::editor::EditorOpening>();
        app.init_resource::<Clipboard>();
        app.init_resource::<crate::app::RoundNotice>();
        app.init_resource::<GameSettings>();
        app.add_message::<crate::app::CodeShared>();
    }

    /// E on a stage of the player's own opens its file in the editor; on a
    /// shipped stage it does nothing.
    #[test]
    fn e_edits_a_stage_of_the_players_own() {
        let path = crate::app::editor::custom_dir().join("stage-edit-probe.txt");
        let mut board = crate::sim::Board::new(6, 5, 2);
        board.set_tile(4, 2, crate::sim::TileKind::Castle(0));
        board.spawn_crab(
            1,
            2,
            crate::sim::Direction::Right,
            crate::sim::Handedness::Left,
            crate::sim::CrabKind::Common,
        );
        let mut level = crate::sim::Level::from_board("Stage Edit Probe", 1, board);
        crate::app::paths::write_atomic(&path, level.to_text()).expect("wrote");
        level.name = "Stage Edit Probe (2)".into();
        level.filing = Some(crate::sim::Filing {
            key: "file:custom/stage-edit-probe.txt".into(),
            legacy: None,
        });
        let mut campaign = campaign();
        campaign.levels.truncate(3);
        campaign.builtins = 3;
        campaign.levels.push(level);
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(campaign);
        app.init_resource::<Progress>();
        app.init_resource::<StageList>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::cursor::LiveSeatKeys>();
        app.init_resource::<crate::app::Coop>();
        own_stage_resources(&mut app);
        app.add_message::<PlacementDenied>();
        app.add_systems(Update, stage_select_input);
        let press_e = |app: &mut App, at: usize| {
            app.world_mut().resource_mut::<StageList>().selected = at;
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::KeyE);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.update();
        };
        press_e(&mut app, 0);
        let screen = |app: &App| *app.world().resource::<State<Screen>>().get();
        assert_eq!(screen(&app), Screen::Menu, "a shipped stage is not edited");
        press_e(&mut app, 3);
        assert_eq!(screen(&app), Screen::Editor);
        let opening = app.world().resource::<crate::app::editor::EditorOpening>();
        let opened = opening.0.as_ref().expect("the editor opens on it");
        assert_eq!(
            opened.level.name, "Stage Edit Probe",
            "as its file names it"
        );
        assert_eq!(opened.path.as_deref(), Some(path.as_path()));
        let _ = std::fs::remove_file(&path);
    }

    /// C on the stage list turns co-op on, and the stage list's prompt
    /// says so.
    #[test]
    fn c_toggles_coop() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(campaign());
        app.init_resource::<Progress>();
        app.init_resource::<StageList>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::cursor::LiveSeatKeys>();
        app.init_resource::<crate::app::Coop>();
        own_stage_resources(&mut app);
        app.add_message::<PlacementDenied>();
        app.add_systems(Update, stage_select_input);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyC);
        app.update();
        assert!(app.world().resource::<crate::app::Coop>().0);
        assert!(EN.prompt_stages.contains("C: co-op"));
    }

    /// "A co-op stage needs two" is about the table as it was seated: C
    /// seating P2 takes it down, and so does leaving the list, whose notice
    /// the menu's status line would otherwise go on showing.
    #[test]
    fn the_needs_two_notice_goes_when_p2_sits_down_or_the_list_is_left() {
        let mut campaign = campaign();
        let first = campaign.levels.len();
        let coop = crate::sim::coop_levels();
        campaign.coop = coop.len();
        campaign.levels.extend(coop);
        campaign.builtins = campaign.levels.len();
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(Screen::StageSelect);
        app.insert_resource(campaign);
        app.init_resource::<Progress>();
        app.init_resource::<StageList>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::cursor::LiveSeatKeys>();
        app.init_resource::<crate::app::Coop>();
        own_stage_resources(&mut app);
        app.add_message::<PlacementDenied>();
        app.add_systems(Update, stage_select_input);
        app.add_systems(OnExit(Screen::StageSelect), exit_stage_select);
        app.world_mut().resource_mut::<StageList>().selected = first;
        let tap = |app: &mut App, key: KeyCode| {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.update();
        };
        let notice = |app: &App| app.world().resource::<crate::app::RoundNotice>().0.clone();
        tap(&mut app, KeyCode::Enter);
        assert!(!notice(&app).is_empty(), "the stage needs two");
        tap(&mut app, KeyCode::KeyC);
        assert!(app.world().resource::<crate::app::Coop>().0);
        assert_eq!(notice(&app), "", "P2 is seated now");
        tap(&mut app, KeyCode::KeyC);
        tap(&mut app, KeyCode::Enter);
        assert!(!notice(&app).is_empty(), "alone again");
        tap(&mut app, KeyCode::Escape);
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
        assert_eq!(notice(&app), "", "not carried onto the menu");
    }

    /// The wiring, end to end in a headless App: Enter on a locked stage is
    /// refused (and says so), Enter on an open one loads that stage.
    #[test]
    fn enter_plays_an_open_stage_and_refuses_a_locked_one() {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.add_message::<PlacementDenied>();
        app.insert_resource(campaign());
        app.init_resource::<Progress>();
        app.init_resource::<StageList>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::cursor::LiveSeatKeys>();
        app.init_resource::<crate::app::Coop>();
        own_stage_resources(&mut app);
        app.add_systems(Update, stage_select_input);

        let enter = |app: &mut App, at: usize| {
            app.world_mut().resource_mut::<StageList>().selected = at;
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            // reset_all, not clear: a key still held is not pressed *again*.
            keys.reset_all();
            keys.press(KeyCode::Enter);
            app.update();
            // No InputPlugin here to age the press, so retire it by hand
            // before the frame that applies the state transition.
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.update();
        };

        // Stage 3 is locked on a fresh save: refused, and the refusal is
        // reported so the sound and the flash can pick it up.
        enter(&mut app, 2);
        assert_eq!(*app.world().resource::<State<Screen>>().get(), Screen::Menu);
        let denials = app
            .world_mut()
            .resource_mut::<Messages<PlacementDenied>>()
            .drain()
            .count();
        assert_eq!(denials, 1, "a locked stage says no out loud");

        enter(&mut app, 0);
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Puzzle
        );
        assert_eq!(app.world().resource::<Campaign>().index, 0);
    }

    /// A locked stage names the stage that would open it; a cleared one says
    /// so; an open one advertises its signpost inventory.
    #[test]
    fn the_caption_reads_the_state_of_the_stage() {
        let campaign = campaign();
        let mut progress = Progress::default();
        let open = caption(&EN, i18n::Lang::En, &campaign, TileState::Open, 0);
        assert!(open.starts_with("1. Welcome Ashore"), "{open}");
        assert!(open.contains('1'), "its one signpost: {open}");

        let locked = caption(&EN, i18n::Lang::En, &campaign, TileState::Locked, 4);
        assert!(locked.contains("clear stage 4"), "{locked}");

        // A watch-it-happen stage says so instead of "signposts: 0".
        let watch = caption(&EN, i18n::Lang::En, &campaign, TileState::Open, 4);
        assert_eq!(campaign.levels[4].posts, 0, "stage 5 is the watch level");
        assert!(watch.ends_with(EN.stage_free), "{watch}");

        progress.mark(&campaign.levels[0].name);
        let state = TileState::of(&progress, &campaign, 0);
        assert_eq!(state, TileState::Cleared);
        assert_eq!(TileState::of(&progress, &campaign, 1), TileState::Open);
        assert_eq!(TileState::of(&progress, &campaign, 2), TileState::Locked);
        let cleared = caption(&EN, i18n::Lang::En, &campaign, state, 0);
        assert!(cleared.ends_with(EN.stage_cleared), "{cleared}");
    }

    /// A locked co-op stage names the co-op stage before it, by the number
    /// its own shelf gives it, not by its place in the whole list.
    #[test]
    fn a_locked_coop_stage_names_the_coop_stage_before_it() {
        let mut campaign = campaign();
        let first = campaign.levels.len();
        let coop = crate::sim::coop_levels();
        campaign.coop = coop.len();
        campaign.levels.extend(coop);
        campaign.builtins = campaign.levels.len();
        let locked = caption(&EN, i18n::Lang::En, &campaign, TileState::Locked, first + 1);
        assert!(locked.starts_with("Co-op 2. "), "{locked}");
        assert!(
            locked.ends_with("locked - clear co-op stage 1 first"),
            "{locked}"
        );
    }

    /// Every tile the grid can draw a co-op stage on wears a mark, and the
    /// mark never touches the tile's number while there is room over it.
    /// A property over sizes rather than a worst case: nothing caps the
    /// player's own levels, so a tile can shrink to a pixel. Down there
    /// the bar keeps [`STRIP_MIN_PX`] (an unmarked co-op tile reads as a
    /// solo one); wherever the gap over the number holds that much, the
    /// bar fits inside the gap. The shipped shelf keeps the 2P badge.
    #[test]
    fn the_coop_mark_shows_and_clears_the_number_at_every_tile_size() {
        let mut size = 1.0;
        while size <= 52.0 {
            let inner = size - 2.0 * TILE_BORDER;
            let number = LINE * number_px(size);
            match CoopMark::on(size) {
                CoopMark::Badge(px) => {
                    assert!(px >= BADGE_MIN_PX);
                    assert!(LINE * px + number <= inner, "badge on number at {size}");
                }
                CoopMark::Strip(height) => {
                    assert!(height >= STRIP_MIN_PX, "no mark at {size}");
                    if (inner - number) / 2.0 >= STRIP_MIN_PX {
                        assert!(height <= (inner - number) / 2.0, "bar on number at {size}");
                    }
                }
            }
            size += 0.25;
        }
        // Down to a pixel is far enough: some sixty rows, six hundred
        // levels of the player's own, get there.
        assert!(tile(60, 2.0 * HEADING_ROOM) < 1.0);
        // The shipped list, two co-op rows and no levels of the player's
        // own, still reads 2P.
        let shipped = rows_of(&[0..100, 100..120]).len();
        let shelf = tile(shipped, HEADING_ROOM);
        assert!(matches!(CoopMark::on(shelf), CoopMark::Badge(_)), "{shelf}");
    }

    /// The key under the grid must explain every edge the grid can draw.
    /// `difficulty_ink` falls through to one colour for anything above its
    /// last named arm, so a harder tier than the key knows about ships in
    /// the colour of the tier below: the first five-signpost levels
    /// appeared tagged as four.
    #[test]
    fn the_key_reaches_the_hardest_stage() {
        let hardest = campaign().levels.iter().map(|l| l.posts).max().unwrap();
        assert!(
            KEY_POSTS.contains(&hardest),
            "the campaign grants {hardest} signposts but the key stops at {}",
            KEY_POSTS.end()
        );
        // Every step of the key is its own colour, or two rows of the key
        // are the same swatch with different numbers under it.
        let inks: Vec<_> = KEY_POSTS.map(|p| difficulty_ink(p).to_srgba()).collect();
        for (i, a) in inks.iter().enumerate() {
            for b in &inks[i + 1..] {
                assert_ne!(a, b, "two signpost counts share a colour");
            }
        }
    }
}
