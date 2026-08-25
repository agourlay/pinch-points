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
/// their own so this is the only vertical gap that applies to them; when
/// they were direct children of the card they picked up its wider one and
/// the grid came out taller than this number allows for.
const GRID_ROOM: f32 = 430.0;

const GAP: f32 = 6.0;

/// Vertical room the player's-own-levels heading takes out of the grid.
/// Counted out rather than guessed at, because what it under-counts the
/// tiles quietly take back: 6 of margin, the 1-pixel rule, 4 between rule
/// and label, an 18-pixel line of text, and one more [`GAP`] for the row it
/// stands in.
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
pub fn rows(builtins: usize, total: usize) -> Vec<std::ops::Range<usize>> {
    let builtins = builtins.min(total);
    let mut out = Vec::new();
    for (start, end) in [(0, builtins), (builtins, total)] {
        for first in (start..end).step_by(COLS) {
            out.push(first..(first + COLS).min(end));
        }
    }
    out
}

/// Is this one of the player's own levels rather than a shipped stage?
pub fn is_custom(campaign: &Campaign, index: usize) -> bool {
    index >= campaign.builtins
}

/// The number a tile wears. The shipped campaign counts from one, and the
/// player's own levels count from one again: they are a shelf of their own,
/// not stages eighty-three onward.
pub fn tile_number(campaign: &Campaign, index: usize) -> usize {
    if is_custom(campaign, index) {
        index - campaign.builtins + 1
    } else {
        index + 1
    }
}

#[derive(Resource, Default)]
pub struct StageList {
    pub selected: usize,
    /// The grid as it was drawn, one range per row. Kept rather than
    /// recomputed: the tiles are spawned once on entering and nothing
    /// reshuffles them while the screen is up, so rebuilding the layout
    /// sixty times a second would be sixty answers to a settled question.
    rows: Vec<std::ops::Range<usize>>,
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
/// five-post levels shipped drawn in the four-post red, and nobody could
/// have told from the grid.
pub fn difficulty_ink(posts: u8) -> Color {
    match posts {
        0 => palette::INK_TIDE,
        1 => Color::srgb(0.52, 0.82, 0.55),
        2 => palette::GOLD,
        3 => Color::srgb(0.96, 0.62, 0.30),
        4 => palette::INK_RAID,
        _ => palette::INK_LURE,
    }
}

/// The signpost counts the key under the grid explains, lowest first.
pub const KEY_POSTS: std::ops::RangeInclusive<u8> = 1..=5;

impl TileState {
    fn of(progress: &Progress, campaign: &Campaign, index: usize) -> TileState {
        if progress.is_cleared(campaign.kind, &campaign.levels[index].name) {
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
            TileState::Cleared => (palette::GOLD.with_alpha(0.28), Color::srgb(1.0, 0.96, 0.86)),
            TileState::Open => (Color::srgba(0.95, 0.93, 0.84, 0.12), palette::IDLE_ROW),
            TileState::Locked => (
                Color::srgba(0.06, 0.08, 0.11, 0.45),
                Color::srgba(0.95, 0.93, 0.84, 0.22),
            ),
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
        TileState::Open => fill(tr.stage_open, &[("n", &level.posts.to_string())]),
        TileState::Locked => fill(tr.stage_locked, &[("n", &index.to_string())]),
    };
    // Both shelves count from one, so the caption says which shelf it is
    // reading: the heading that says so on the grid is too far from this
    // line to carry it.
    let shelf = if is_custom(campaign, index) {
        format!("{} ", tr.stage_custom)
    } else {
        String::new()
    };
    format!(
        "{shelf}{}. {} - {tail}",
        tile_number(campaign, index),
        lang.level_name(&level.name)
    )
}

/// The company this card keeps. The grid is the widest card in the game
/// at a full ten columns of full-size tiles, so the flock hangs right out
/// at the margins: gulls in the sky above, crabs on the sand below.
const FLOCK: [crate::app::company::Perch; 4] = [
    crate::app::company::Perch::gull(0.04, 0.07, 62.0),
    crate::app::company::Perch::gull(0.87, 0.09, 56.0),
    crate::app::company::Perch::crab(0.03, 0.74, 66.0),
    crate::app::company::Perch::crab(0.88, 0.79, 58.0),
];

/// The widest the grid can be: ten columns of the biggest tile the art was
/// drawn for, the gaps between them, and the card's padding. A short
/// campaign gets exactly this; a long one shrinks its tiles and gets less.
/// The bound the flock is hung against, since it has to clear the card on
/// every machine rather than on this one's level count.
#[cfg(test)]
const WIDEST_CARD: f32 = COLS as f32 * 52.0 + (COLS - 1) as f32 * GAP + 2.0 * 28.0;

pub fn enter_stage_select(
    mut commands: Commands,
    campaign: Res<Campaign>,
    progress: Res<Progress>,
    settings: Res<GameSettings>,
    art: Res<crate::app::art::Art>,
    mut list: ResMut<StageList>,
) {
    // Land the cursor where the player left off, not back at stage one.
    list.selected = progress.furthest_open(&campaign);
    list.rows = rows(campaign.builtins, campaign.levels.len());
    let tr = settings.tr();
    commands
        .spawn((
            StageSelectUi,
            // Fills the space between the header and the prompt line, and
            // centres the grid in it: the list is the whole screen here.
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(52.0),
                bottom: Val::Px(52.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(12.0),
                ..default()
            },
        ))
        .with_children(|wrap| {
            // The flock first, so it sits behind the card rather than on it.
            crate::app::company::flock(wrap, &art, &FLOCK);
            // The grid between the crab and the gull, so it stays centred
            // on its own tiles rather than on the crab.
            wrap.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(crate::app::company::CRITTER_GAP),
                ..default()
            })
            .with_children(|line| {
                crate::app::company::shoulder(line, &art, crate::app::company::Company::Crab, 0.0);
                line.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(12.0),
                        padding: UiRect::axes(Val::Px(28.0), Val::Px(20.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(16.0)),
                        ..default()
                    },
                    BackgroundColor(palette::CARD_FILL),
                    BorderColor::all(palette::CARD_EDGE),
                ))
                .with_children(|column| {
                    // The count and its bar are the shipped campaign's, which is
                    // the thing that can be finished. Levels the player keeps
                    // adding would make the denominator a moving target, and
                    // "82 of 93" would read as an unfinished campaign forever.
                    let shipped = 0..campaign.builtins;
                    let total = shipped.len();
                    let done = progress.cleared_in(&campaign, shipped);
                    column.spawn((
                        Text::new(fill(
                            tr.stage_progress,
                            &[("a", &done.to_string()), ("b", &total.to_string())],
                        )),
                        TextFont {
                            font_size: FontSize::Px(18.0),
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
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
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
                    // The player's own levels are a shelf under the campaign,
                    // with a label of their own: they are not stage eighty-three.
                    let has_custom = campaign.builtins < campaign.levels.len();
                    let heading = if has_custom { HEADING_ROOM } else { 0.0 };
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
                                if has_custom && row.start == campaign.builtins {
                                    spawn_shelf_heading(grid, tr, &campaign, &progress);
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
                                    font_size: FontSize::Px(13.0),
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
                                            font_size: FontSize::Px(13.0),
                                            ..default()
                                        },
                                        TextColor(palette::PARCHMENT.with_alpha(0.60)),
                                    ));
                                });
                            }
                        });
                });
                crate::app::company::shoulder(line, &art, crate::app::company::Company::Gull, 1.7);
            });
            // Both lines sit under the card, not in it. They change with
            // every cursor move, and inside the card they would size it:
            // the box would breathe as you walked across the grid. Out here
            // they get the whole screen's width, which the longest German
            // caption needs, and their own fixed rows, so nothing moves.
            wrap.spawn(Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|lines| {
                for (height, font, color, tag) in [
                    (26.0, 20.0, palette::SELECTED_ROW, Caption::Stage),
                    (
                        22.0,
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
                            let mut text = line.spawn((
                                Text::new(String::new()),
                                TextFont {
                                    font_size: FontSize::Px(font),
                                    ..default()
                                },
                                TextLayout::no_wrap(),
                                TextColor(color),
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

/// The label between the two shelves: a rule, the name of the shelf, and
/// how much of it is cleared. Without it the custom levels read as more
/// campaign - which is what the numbering said before they got their own.
fn spawn_shelf_heading(
    grid: &mut ChildSpawnerCommands,
    tr: &i18n::Tr,
    campaign: &Campaign,
    progress: &Progress,
) {
    let mine = campaign.builtins..campaign.levels.len();
    let count = fill(
        tr.stage_progress,
        &[
            (
                "a",
                &progress.cleared_in(campaign, mine.clone()).to_string(),
            ),
            ("b", &mine.len().to_string()),
        ],
    );
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
                    Text::new(tr.stage_custom.to_string()),
                    TextFont {
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(palette::PARCHMENT.with_alpha(0.60)),
                ));
                line.spawn((
                    Text::new(count),
                    TextFont {
                        font_size: FontSize::Px(13.0),
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
    line.spawn((
        StageTile(index),
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            border: UiRect::all(Val::Px(2.0)),
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
                // The number has to stay legible in whatever the tile
                // shrank to, so it shrinks with it rather than by itself.
                font_size: FontSize::Px((size * 0.4).min(20.0)),
                ..default()
            },
            TextColor(ink),
        ));
    });
}

/// Paint the selection ring and the caption. The tile states themselves are
/// fixed while the screen is up: nothing gets cleared from in here.
#[allow(clippy::too_many_arguments)]
pub fn update_stage_tiles(
    list: Res<StageList>,
    campaign: Res<Campaign>,
    progress: Res<Progress>,
    settings: Res<GameSettings>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    mut tiles: Query<(&StageTile, &mut BorderColor, &mut BackgroundColor)>,
    mut numbers: Query<(&StageNumber, &mut TextColor)>,
    mut captions: Query<&mut Text, With<StageCaption>>,
    mut hints: Query<&mut Text, (With<StageHint>, Without<StageCaption>)>,
) {
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
        // The cursor also lifts the tile it stands on. A ring alone reads as
        // one more colour in a grid that already has three of them, and the
        // stage you are about to press Enter on should not be something you
        // have to hunt for.
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
    if let Ok(mut text) = captions.single_mut() {
        let line = caption(
            settings.tr(),
            settings.language,
            &campaign,
            state,
            list.selected,
        );
        menu_ui::set_text(&mut text, &line);
    }
    // A locked stage keeps its lesson to itself; the caption already says
    // what to do about it.
    if let Ok(mut text) = hints.single_mut() {
        let hint = match state {
            TileState::Locked => String::new(),
            TileState::Cleared | TileState::Open => caps.legend(
                settings
                    .language
                    .level_hint(&campaign.levels[list.selected].name)
                    .unwrap_or(""),
            ),
        };
        menu_ui::set_text(&mut text, &hint);
    }
}

/// Arrows move, Enter plays an open stage, a locked one just says no.
pub fn stage_select_input(
    keys: Res<ButtonInput<KeyCode>>,
    progress: Res<Progress>,
    mut list: ResMut<StageList>,
    mut campaign: ResMut<Campaign>,
    mut denied: MessageWriter<PlacementDenied>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    // Read the layout, then move: the rows are settled and the cursor is
    // the only thing this system changes.
    let at = step(list.selected, &list.rows, &keys);
    list.selected = at;
    if keys.just_pressed(KeyCode::Escape) {
        next_screen.set(Screen::Menu);
        return;
    }
    if !menu_ui::enter(&keys) {
        return;
    }
    if progress.unlocked(&campaign, list.selected) {
        campaign.index = list.selected;
        next_screen.set(Screen::Puzzle);
    } else {
        denied.write(PlacementDenied {
            player: 0,
            out_of_signposts: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::CampaignKind;
    use crate::app::i18n::EN;
    use crate::sim::campaign_levels;

    fn campaign() -> Campaign {
        let levels = campaign_levels();
        let builtins = levels.len();
        Campaign {
            kind: CampaignKind::TidePool,
            levels,
            index: 0,
            builtins,
        }
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

        progress.mark(campaign.kind, &campaign.levels[0].name);
        let state = TileState::of(&progress, &campaign, 0);
        assert_eq!(state, TileState::Cleared);
        assert_eq!(TileState::of(&progress, &campaign, 1), TileState::Open);
        assert_eq!(TileState::of(&progress, &campaign, 2), TileState::Locked);
        let cleared = caption(&EN, i18n::Lang::En, &campaign, state, 0);
        assert!(cleared.ends_with(EN.stage_cleared), "{cleared}");
    }

    /// The flock is hung on the frame by hand, and a hand can hang one
    /// over the grid, where the card's near-solid fill shows it through as
    /// a smudge under a stage number. Checked against the *widest* the
    /// grid can be, not this machine's level count: a player with fewer
    /// levels gets bigger tiles and a wider card.
    #[test]
    fn the_flock_leaves_the_grid_alone() {
        use crate::app::company;
        company::flock_is_hung_clear(&FLOCK, company::keep_clear(WIDEST_CARD), (0.0, 0.94));
    }

    /// The key under the grid must explain every edge the grid can draw.
    /// `difficulty_ink` falls through to one colour for anything above its
    /// last named arm, so a harder tier than the key knows about ships in
    /// the colour of the tier below it, silently: that is how the first
    /// five-signpost levels appeared tagged as four.
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
