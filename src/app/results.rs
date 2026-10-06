//! Round-end overlays: the tide-is-in standings card, the puzzle victory
//! card, and the winner confetti.

use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::net::Online;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::side_panels::leading_seats;
use crate::app::teams::TeamMode;
use crate::app::{Campaign, Sim};
use crate::sim::MAX_PLAYERS;
use bevy::prelude::*;

/// Everything spawned for a results overlay (versus standings, puzzle win).
#[derive(Component)]
pub struct ResultsPanel;

const CARD_TEXT: Color = palette::PARCHMENT;

/// The winner headline and its display colour, derived from the same
/// [`leading_seats`] logic the panels crown with.
fn winner_line(
    settings: &GameSettings,
    names: &crate::app::SeatNames,
    scores: &[u32; MAX_PLAYERS],
    seats: u8,
    mode: TeamMode,
) -> (String, Color) {
    let tr = settings.tr();
    let leaders = leading_seats(scores, seats, mode);
    if mode != TeamMode::Solo {
        let Some(seat) = leaders.iter().position(|&led| led) else {
            return (tr.dead_heat.to_string(), CARD_TEXT);
        };
        let team = mode.team_of(seat as u8);
        return (
            tr.team_wins.replace(
                "{t}",
                &crate::app::teams::label(settings, names, mode, team, seats),
            ),
            palette::player_color(seat as u8),
        );
    }
    match leaders.iter().position(|&led| led) {
        Some(winner) => (
            fill(tr.wins, &[("p", &names.label(tr, winner as u8))]),
            palette::player_color(winner as u8).lighter(0.10),
        ),
        None => (tr.dead_heat.to_string(), CARD_TEXT),
    }
}

/// How many pixels a tile is when the results card draws the winner's
/// castle: bigger than on the board, since it is the one thing on the card
/// that is a picture.
const CASTLE_TILE_PX: f32 = 96.0;

/// The winner's castle on the results card, built from the same pieces as
/// the one on their tile, at the tier they finished on: a round won big
/// shows off a big castle, moat and all.
fn winner_castle(
    card: &mut ChildSpawnerCommands,
    art: &crate::app::art::Art,
    tier: u8,
    color: Color,
) {
    let t = CASTLE_TILE_PX;
    // Room for the widest castle (the moat) and its flag above the keep.
    let side = 1.4 * t;
    card.spawn(Node {
        width: Val::Px(side),
        height: Val::Px(side),
        margin: UiRect::vertical(Val::Px(4.0)),
        ..default()
    })
    .with_children(|stage| {
        // A mound of sand to stand on, or it floats on the dark card.
        let mound = Vec2::new(1.55, 0.6) * t;
        stage.spawn((
            // Stretched: left to keep its square, the mound drew as a round
            // blob under the gate rather than ground under the castle.
            ImageNode::new(art.puddle.clone())
                .with_color(Color::srgba(0.84, 0.77, 0.62, 0.9))
                .with_mode(NodeImageMode::Stretch),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(side / 2.0 - mound.x / 2.0),
                top: Val::Px(side / 2.0 + 0.40 * t - mound.y / 2.0),
                width: Val::Px(mound.x),
                height: Val::Px(mound.y),
                ..default()
            },
        ));
        // The parts come back to front, which is the order UI draws in.
        for part in crate::app::board_render::castle_parts(art, tier, color) {
            let size = part.size * t;
            let node = Node {
                position_type: PositionType::Absolute,
                left: Val::Px(side / 2.0 + part.at.x * t - size.x / 2.0),
                // UI runs down, the castle's y up.
                top: Val::Px(side / 2.0 - part.at.y * t - size.y / 2.0),
                width: Val::Px(size.x),
                height: Val::Px(size.y),
                ..default()
            };
            match part.image {
                Some(image) => {
                    stage.spawn((ImageNode::new(image).with_color(part.tint), node));
                }
                None => {
                    stage.spawn((BackgroundColor(part.tint), node));
                }
            }
        }
    });
}

/// Spawn the wrapper that centres the results card, and return its entity.
fn results_card(commands: &mut Commands) -> Entity {
    commands
        .spawn((
            ResultsPanel,
            // Above the header/prompt bars in the UI stack.
            GlobalZIndex(menu_ui::layer::RESULTS),
            menu_ui::centred_overlay(),
        ))
        .id()
}

/// The series under the standings: each seat's wins at this size, this
/// far apart, wrapping to lines no wider than this. Wide enough for the
/// widest entry there is, a team of three twelve-letter names with three
/// wins (see `the_series_fits_the_card`), and two seats to a line when
/// every name is that long.
const SERIES_FONT: f32 = 23.0;
const SERIES_GAP: f32 = 24.0;
const SERIES_W: f32 = 600.0;

fn card_text(size: f32, color: Color) -> (TextFont, TextColor) {
    (
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

/// The standings, best first: place, who, and what they banked, each in its
/// seat's colour. In 2v2 the two teams' totals stand in for the four seats.
///
/// Pure, so the ordering and the team arithmetic can be checked without
/// building a card. `tag` supplies the "(you)" / "(AI)" marker.
fn standings_rows(
    settings: &GameSettings,
    names: &crate::app::SeatNames,
    scores: &[u32; MAX_PLAYERS],
    seats: u8,
    mode: TeamMode,
    tag: impl Fn(u8) -> &'static str,
) -> Vec<(String, Color)> {
    if mode != TeamMode::Solo {
        let totals = crate::app::teams::team_scores(scores, seats, mode);
        let mut standing: Vec<(String, u32, u8)> = totals
            .iter()
            .enumerate()
            .map(|(team, &total)| {
                let team = team as u8;
                let lead_seat = crate::app::teams::face_of(mode, team, seats);
                (
                    crate::app::teams::label(settings, names, mode, team, seats),
                    total,
                    lead_seat,
                )
            })
            .collect();
        standing.sort_by_key(|&(_, total, seat)| (std::cmp::Reverse(total), seat));
        return standing
            .iter()
            .map(|(name, total, seat)| (name.clone(), *total, *seat))
            .enumerate()
            .map(|(place, (name, total, seat))| {
                (
                    format!("{}  {name:<13} {total:>4}", place + 1),
                    palette::player_color(seat),
                )
            })
            .collect();
    }
    standing_order(scores, seats)
        .iter()
        .enumerate()
        .map(|(place, &seat)| {
            let label = names.label(settings.tr(), seat);
            (
                format!(
                    "{}  {label}{:<7} {:>4}",
                    place + 1,
                    tag(seat),
                    scores[seat as usize]
                ),
                palette::player_color(seat),
            )
        })
        .collect()
}

/// The seats best first, the order a free-for-all's standings are written
/// in: by score, and by seat between equals.
fn standing_order(scores: &[u32; MAX_PLAYERS], seats: u8) -> Vec<u8> {
    let mut order: Vec<u8> = (0..seats).collect();
    order.sort_by_key(|&seat| std::cmp::Reverse(scores[seat as usize]));
    order
}

/// The round's awards as card lines, each in the colour of the first seat
/// it names. Nothing for a solo table, where every award goes to the one
/// player there, or for a round the tally did not see whole.
fn award_rows(
    settings: &GameSettings,
    names: &crate::app::SeatNames,
    tally: &crate::app::awards::RoundTally,
    board: &crate::sim::Board,
    seats: u8,
    mode: TeamMode,
) -> Vec<(String, Color)> {
    if seats < 2 || !tally.covers(board) {
        return Vec::new();
    }
    let tr = settings.tr();
    let winners = leading_seats(board.scores(), seats, mode);
    tally
        .awards(seats, mode, &winners)
        .iter()
        .map(|award| {
            (
                award.line(tr, |seat| names.label(tr, seat)),
                palette::player_color(award.who[0]),
            )
        })
        .collect()
}

/// What the crowd called, for everyone at an online table, and for a
/// spectator how their own call went. Nothing when nobody called anyone.
fn crowd_rows(
    settings: &GameSettings,
    names: &crate::app::SeatNames,
    online: &Online,
) -> Vec<(String, Color)> {
    let Some(session) = &online.0 else {
        return Vec::new();
    };
    let tr = settings.tr();
    let mut rows = Vec::new();
    if session.stands.picks.any() {
        let list = session.stands.picks.line(|seat| names.label(tr, seat));
        rows.push((
            fill(tr.crowd_picked, &[("l", &list)]),
            CARD_TEXT.darker(0.15),
        ));
    }
    if let Some((seat, right)) = session.stands.last_call {
        let calls = &session.stands.calls;
        let (a, b) = (calls.right.to_string(), calls.made.to_string());
        rows.push(match right {
            true => (fill(tr.call_right, &[("a", &a), ("b", &b)]), palette::GOLD),
            false => (
                fill(
                    tr.call_wrong,
                    &[("p", &names.label(tr, seat)), ("a", &a), ("b", &b)],
                ),
                palette::player_color(seat),
            ),
        });
    }
    rows
}

/// What a finished round fed into besides its own scores: the day's best,
/// the reel being cut from it, the series it belongs to, and the awards.
#[derive(bevy::ecs::system::SystemParam)]
pub struct RoundExtras<'w> {
    daily: Res<'w, crate::app::Daily>,
    highlight: Res<'w, crate::app::Highlight>,
    stats: Res<'w, crate::app::achievements::Stats>,
    tournament: Res<'w, crate::app::tournament::Tournament>,
    tally: Res<'w, crate::app::awards::RoundTally>,
    kinds: Res<'w, crate::app::SeatKinds>,
}

/// The tide-is-in standings card: winner headline, ranked scores in seat
/// colours (with AI/you markers), the round's awards, and its total haul.
pub fn spawn_versus_results(
    mut commands: Commands,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    art: Res<crate::app::art::Art>,
    mut rng: ResMut<crate::app::effects::VisualRng>,
    seating: crate::app::side_panels::Seating,
    extras: RoundExtras,
) {
    let local = seating.local();
    let crate::app::side_panels::Seating {
        seats,
        controllers,
        names,
        online,
        ..
    } = seating;
    let RoundExtras {
        daily,
        highlight,
        stats,
        tournament,
        tally,
        kinds,
    } = extras;
    let board = &sim.0;
    let scores = board.scores();
    let count = seats.0.max(2);
    let mode = crate::app::teams::in_play(&settings, &online, count);
    let tr = settings.tr();
    let (headline, headline_color) = winner_line(&settings, &names, scores, count, mode);
    // Confetti for a decided round, in the winners' colours, unless the
    // player asked for less motion, in which case the card speaks for itself.
    let winners = leading_seats(scores, count, mode);
    for seat in 0..MAX_PLAYERS as u8 {
        if winners[seat as usize] && !settings.reduced_motion {
            crate::app::effects::confetti(
                &mut commands,
                &mut rng,
                &art,
                palette::player_color(seat),
            );
        }
    }

    let rows = standings_rows(&settings, &names, scores, count, mode, |seat| {
        crate::app::side_panels::seat_tag(tr, &controllers, local, seat)
    });
    let haul = board.crabs_banked();
    let awards = award_rows(&settings, &names, &tally, board, seats.0, mode);

    // Whose castle the card shows off: the winner's, at the tier the round
    // left it on. A dead heat has no one to crown, and keeps the card's gold.
    let crowned = winners.iter().position(|&won| won).map(|seat| seat as u8);
    let card = results_card(&mut commands);
    commands.entity(card).with_children(|wrap| {
        // Opaque, as the round's other cards are: it stands on the board,
        // and the browsing cards' 0.95, blended in linear light, let the
        // crabs and fences show through the standings.
        let mut frame = wrap.spawn(menu_ui::screen_card());
        frame.insert(BackgroundColor(palette::CARD_BG));
        if crowned.is_some() {
            frame.insert(BorderColor::all(headline_color));
        }
        frame.with_children(|card| {
            card.spawn((
                Text::new(tr.tide_is_in),
                menu_ui::display_font(20.0),
                TextColor(CARD_TEXT.darker(0.1)),
            ));
            if let Some(seat) = crowned {
                let tier = crate::sim::castle_tier(scores[usize::from(seat)]);
                winner_castle(card, &art, tier, palette::player_color(seat));
            }
            card.spawn((
                Text::new(headline),
                menu_ui::display_font(34.0),
                TextColor(headline_color),
            ));
            card.spawn(Node {
                height: Val::Px(6.0),
                ..default()
            });
            // A bot wears the robot beside its line. The rows keep one
            // left edge between them, so a slot is held on every row of a
            // table with any bot at it.
            let seat_rows: Vec<Option<u8>> = match mode {
                TeamMode::Solo => standing_order(scores, count)
                    .into_iter()
                    .map(Some)
                    .collect(),
                TeamMode::Pairs | TeamMode::Trios => vec![None; rows.len()],
            };
            let any_bot = seat_rows.iter().flatten().any(|&seat| kinds.bot(seat));
            for ((line, color), seat) in rows.into_iter().zip(seat_rows) {
                let row = card_text(23.0, color);
                if !any_bot {
                    card.spawn((Text::new(line), row.0, row.1));
                    continue;
                }
                card.spawn(Node {
                    column_gap: Val::Px(6.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|line_row| {
                    match seat.filter(|&seat| kinds.bot(seat)) {
                        Some(seat) => {
                            line_row.spawn(crate::app::side_panels::robot_icon(&art, seat, 22.0));
                        }
                        None => {
                            line_row.spawn(Node {
                                width: Val::Px(22.0),
                                ..default()
                            });
                        }
                    }
                    line_row.spawn((Text::new(line), row.0, row.1));
                });
            }
            if !awards.is_empty() {
                card.spawn(Node {
                    height: Val::Px(6.0),
                    ..default()
                });
            }
            for (line, color) in awards {
                let row = card_text(17.0, color);
                card.spawn((Text::new(line), row.0, row.1));
            }
            for (line, color) in crowd_rows(&settings, &names, &online) {
                let row = card_text(17.0, color);
                card.spawn((Text::new(line), row.0, row.1));
            }
            card.spawn(Node {
                height: Val::Px(6.0),
                ..default()
            });
            let foot = card_text(17.0, CARD_TEXT.darker(0.15));
            card.spawn((
                Text::new(fill(tr.haul, &[("n", &haul.to_string())])),
                foot.0,
                foot.1,
            ));
            // The reel is written off-thread and is usually not there yet
            // when the card goes up: the line is spawned hidden and shown
            // by `update_highlight_line` once the save has happened.
            let line = card_text(15.0, CARD_TEXT.darker(0.3));
            card.spawn((
                highlight_line_node(highlight.0.is_some()),
                Text::new(highlight_line_text(tr, &highlight)),
                line.0,
                line.1,
                HighlightLine,
            ));
            if daily.active {
                let best = card_text(17.0, palette::GOLD);
                card.spawn((
                    Text::new(fill(tr.daily_best, &[("n", &stats.daily_best.to_string())])),
                    best.0,
                    best.1,
                ));
            }
            if tournament.in_series() {
                card.spawn(Node {
                    height: Val::Px(6.0),
                    ..default()
                });
                let round = card_text(17.0, CARD_TEXT.darker(0.15));
                card.spawn((
                    Text::new(crate::app::tournament::round_line(tr, &tournament)),
                    round.0,
                    round.1,
                ));
                // Each seat's wins in its own colour, as many to a line as
                // fit. One gold line joined with dots ran a table of six
                // past the card's frame, and six full names past the window.
                card.spawn(Node {
                    flex_wrap: FlexWrap::Wrap,
                    justify_content: JustifyContent::Center,
                    column_gap: Val::Px(SERIES_GAP),
                    max_width: Val::Px(SERIES_W),
                    ..default()
                })
                .with_children(|series| {
                    for standing in crate::app::tournament::standings(
                        &settings,
                        &names,
                        &tournament,
                        mode,
                        count,
                    ) {
                        let entry = card_text(SERIES_FONT, standing.color);
                        series.spawn((
                            Text::new(standing.line()),
                            entry.0,
                            entry.1,
                            TextLayout::no_wrap(),
                        ));
                    }
                });
                if tournament.is_decided() {
                    if let Some(champ) = tournament.winner(mode, count) {
                        let (who, seat) = crate::app::tournament::champion_name(
                            &settings, &names, mode, champ, count,
                        );
                        let line = card_text(26.0, palette::player_color(seat).lighter(0.1));
                        card.spawn((
                            Text::new(fill(tr.tour_champion, &[("p", &who)])),
                            line.0,
                            line.1,
                        ));
                    }
                } else {
                    let hint = card_text(17.0, CARD_TEXT.darker(0.15));
                    let door =
                        crate::app::play_input::enter_door(tr, &online, tournament.is_running());
                    card.spawn((Text::new(door), hint.0, hint.1));
                }
            }
        });
    });
}

/// The results card's "highlight reel: …" line, shown once the reel is
/// on disk and not before.
#[derive(Component)]
pub struct HighlightLine;

/// The line's node: laid out only when there is a path to say.
fn highlight_line_node(shown: bool) -> Node {
    Node {
        display: if shown { Display::Flex } else { Display::None },
        ..default()
    }
}

/// What the line says: the reel's path, or nothing yet.
fn highlight_line_text(tr: &crate::app::i18n::Tr, highlight: &crate::app::Highlight) -> String {
    highlight
        .0
        .as_ref()
        .map(|path| fill(tr.highlight_saved, &[("path", path)]))
        .unwrap_or_default()
}

/// Show the reel line when the reel thread reports the GIF written.
pub fn update_highlight_line(
    highlight: Res<crate::app::Highlight>,
    settings: Res<GameSettings>,
    mut lines: Query<(&mut Text, &mut Node), With<HighlightLine>>,
) {
    if !highlight.is_changed() {
        return;
    }
    let tr = settings.tr();
    for (mut text, mut node) in &mut lines {
        text.0 = highlight_line_text(tr, &highlight);
        *node = highlight_line_node(highlight.0.is_some());
    }
}

/// The puzzle victory card: level cleared, name, and what comes next.
pub fn spawn_puzzle_won(
    mut commands: Commands,
    campaign: Res<Campaign>,
    settings: Res<GameSettings>,
    sim: Res<Sim>,
    art: Res<crate::app::art::Art>,
) {
    let tr = settings.tr();
    let name = settings
        .language
        .level_name(&campaign.current().name)
        .to_string();
    // The shipped campaign ends with its last shipped stage; the player's
    // own levels behind it are a shelf, not stage eighty-three, and the
    // last of those ends only the list. So the card counts this level
    // within whichever of the two it belongs to, and only the shipped
    // section's end is the campaign's end.
    let (place, of, custom) = campaign.place();
    let last = campaign.is_last();
    let last_shipped = last && !custom;
    let board = &sim.0;
    // The castle the crabs were routed to: the first on the beach, which
    // is the only one in all but a handful of stages.
    let castle = board.castle_owners().next();
    let card = results_card(&mut commands);
    commands.entity(card).with_children(|wrap| {
        let mut frame = wrap.spawn(menu_ui::screen_card());
        frame.insert(BackgroundColor(palette::CARD_BG));
        frame.with_children(|card| {
            let title = match last_shipped {
                true => tr.campaign_done,
                false => tr.all_safe,
            };
            card.spawn((
                Text::new(title),
                menu_ui::display_font(34.0),
                TextColor(palette::GOLD),
            ));
            if let Some(owner) = castle {
                let tier = crate::sim::castle_tier(board.scores()[usize::from(owner)]);
                winner_castle(card, &art, tier, palette::player_color(owner));
            }
            saved_row(card, &art, &campaign, board);
            let sub = card_text(21.0, CARD_TEXT);
            let shelf = match custom {
                true => format!("{}  ", tr.stage_custom),
                false => String::new(),
            };
            card.spawn((
                Text::new(format!("{shelf}{place} / {of}  -  {name}")),
                sub.0,
                sub.1,
            ));
            let foot = card_text(17.0, CARD_TEXT.darker(0.15));
            card.spawn((
                Text::new(if last { tr.last_level } else { tr.prompt_won }),
                foot.0,
                foot.1,
            ));
        });
    });
}

/// How big a crab is in the row of them on a puzzle's card.
const ROW_CRAB_PX: f32 = 34.0;
/// Most crabs the row shows: a stage with spawners can save dozens, and a
/// row of forty is a smear. Past this the row is as many as fit.
const ROW_CRABS_MAX: usize = 12;

/// The level's crabs in a row on a strip of sand, the ones saved bright and
/// the rest faded, so a win shows off who made it home and a loss shows
/// how near it came.
///
/// Drawn as the level's own crabs, kind and claw and all, in the order the
/// level lists them: a crab from a spawner, which the level cannot name,
/// is a common one. Which crabs were the lost ones is not something the
/// board keeps once they are gone, so a loss dims the tail of the row: the
/// count is exact, the faces are the level's.
fn saved_row(
    card: &mut ChildSpawnerCommands,
    art: &crate::app::art::Art,
    campaign: &Campaign,
    board: &crate::sim::Board,
) {
    use crate::app::creatures::{body_color, claw_color, mirrored};
    use crate::sim::{CrabKind, Handedness};
    let level = campaign.current();
    let of = board.crabs_spawned().max(level.crab_count()) as usize;
    let saved = board.crabs_banked() as usize;
    let shown = of.min(ROW_CRABS_MAX);
    let start = level.board();
    let faces: Vec<(CrabKind, Handedness)> = start
        .crabs()
        .iter()
        .map(|crab| (crab.kind, crab.handed))
        .chain(std::iter::repeat((CrabKind::Common, Handedness::Right)))
        .take(shown)
        .collect();
    card.spawn((
        Node {
            column_gap: Val::Px(2.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(4.0)),
            margin: UiRect::vertical(Val::Px(4.0)),
            border_radius: BorderRadius::all(Val::Px(14.0)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.84, 0.77, 0.62)),
    ))
    .with_children(|row| {
        for (i, (kind, handed)) in faces.into_iter().enumerate() {
            let home = i < saved;
            let fade = if home { 1.0 } else { 0.28 };
            let layer = |image: &Handle<Image>, color: Color| {
                let mut node = ImageNode::new(image.clone()).with_color(color.with_alpha(fade));
                node.flip_y = mirrored(handed);
                (
                    node,
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                )
            };
            row.spawn(Node {
                width: Val::Px(ROW_CRAB_PX),
                height: Val::Px(ROW_CRAB_PX),
                ..default()
            })
            .with_children(|icon| {
                // Facing along the row, the way they walked home.
                icon.spawn(layer(&art.crab, body_color(kind)));
                icon.spawn(layer(&art.claw, claw_color(handed)));
            });
        }
    });
}

/// The puzzle loss card: which level, and Enter to try again.
///
/// A loss stops the sim on the tick the crab was lost, and the board holds
/// still under whatever it was doing, a gull mid-tile with its meal. With
/// no card that reads as the game hanging, the prompt pill along the bottom
/// being the only word of it.
pub fn spawn_puzzle_lost(
    mut commands: Commands,
    campaign: Res<Campaign>,
    settings: Res<GameSettings>,
    // Optional, so the card goes up whatever else is loaded: the row of
    // crabs is a picture on it, and the words are the news.
    sim: Option<Res<Sim>>,
    art: Option<Res<crate::app::art::Art>>,
) {
    let tr = settings.tr();
    let name = settings
        .language
        .level_name(&campaign.current().name)
        .to_string();
    let card = results_card(&mut commands);
    commands.entity(card).with_children(|wrap| {
        let mut frame = wrap.spawn(menu_ui::screen_card());
        frame.insert((
            BackgroundColor(palette::CARD_BG),
            BorderColor::all(palette::INK_RAID.with_alpha(0.6)),
        ));
        frame.with_children(|card| {
            card.spawn((
                Text::new(tr.crabs_lost),
                menu_ui::display_font(34.0),
                TextColor(palette::INK_RAID),
            ));
            if let (Some(sim), Some(art)) = (&sim, &art) {
                saved_row(card, art, &campaign, &sim.0);
            }
            let sub = card_text(21.0, CARD_TEXT);
            card.spawn((
                Text::new(format!(
                    "{} / {}  -  {name}",
                    campaign.index + 1,
                    campaign.levels.len()
                )),
                sub.0,
                sub.1,
            ));
            let foot = card_text(17.0, CARD_TEXT.darker(0.15));
            card.spawn((Text::new(tr.prompt_lost), foot.0, foot.1));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::EN;

    /// Losing a puzzle raises a card over the held board, and leaving the
    /// phase takes it down again: the stop is said, not left to be guessed.
    #[test]
    fn a_lost_puzzle_says_so_on_a_card() {
        use crate::app::{CampaignKind, Phase, Screen};
        use crate::sim::campaign_levels;
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        let levels = campaign_levels();
        let builtins = levels.len();
        app.insert_resource(Campaign {
            kind: CampaignKind::TidePool,
            levels,
            index: 3,
            builtins,
        });
        app.insert_resource(GameSettings::default());
        app.insert_state(Screen::Puzzle);
        app.init_state::<Phase>();
        app.add_systems(
            OnEnter(Phase::Lost),
            spawn_puzzle_lost.run_if(in_state(Screen::Puzzle)),
        );
        app.add_systems(OnExit(Phase::Lost), menu_ui::despawn_marked::<ResultsPanel>);
        let cards = |app: &mut App| {
            app.world_mut()
                .query_filtered::<Entity, With<ResultsPanel>>()
                .iter(app.world())
                .count()
        };
        app.update();
        assert_eq!(cards(&mut app), 0, "no card while the round runs");
        app.world_mut()
            .resource_mut::<NextState<Phase>>()
            .set(Phase::Lost);
        app.update();
        assert_eq!(cards(&mut app), 1, "one card on the loss");
        let said: Vec<String> = app
            .world_mut()
            .query::<&Text>()
            .iter(app.world())
            .map(|t| t.0.clone())
            .collect();
        assert!(said.iter().any(|t| t == EN.crabs_lost), "{said:?}");
        assert!(said.iter().any(|t| t == EN.prompt_lost), "{said:?}");
        assert!(said.iter().any(|t| t.starts_with("4 / ")), "{said:?}");
        app.world_mut()
            .resource_mut::<NextState<Phase>>()
            .set(Phase::Setup);
        app.update();
        assert_eq!(cards(&mut app), 0, "and gone again on retry");
    }

    /// The series line fits the card at its widest. A team of three with
    /// the longest names a seat can carry and the most wins a series can
    /// give is one entry, and has to fit a line on its own; the card around
    /// the lines has to fit the window the interface is drawn for.
    #[test]
    fn the_series_fits_the_card() {
        use crate::app::i18n::metrics::text_px;
        use crate::app::settings::{DESIGN_W, NAME_MAX};
        let name = "M".repeat(NAME_MAX);
        let trio = [name.as_str(); 3].join("+");
        let widest = format!("{trio}  ***");
        let px = text_px(&widest, SERIES_FONT);
        assert!(px <= SERIES_W, "{widest:?} is {px}px of {SERIES_W}");
        // Two solo seats of the longest name share a line.
        let solo = text_px(&format!("{name}  ***"), SERIES_FONT);
        assert!(2.0 * solo + SERIES_GAP <= SERIES_W, "{solo}px a seat");
        let card = SERIES_W + 2.0 * 22.0;
        assert!(card <= DESIGN_W, "a {card}px card");
    }

    /// The standings run best-first, carry each seat's marker, and in 2v2
    /// collapse to the two team totals.
    #[test]
    fn standings_rank_by_score() {
        let rows = standings_rows(
            &GameSettings::default(),
            &crate::app::SeatNames::default(),
            &[3, 9, 5, 0, 0, 0],
            4,
            TeamMode::Solo,
            |seat| {
                if seat == 1 { EN.tag_you } else { EN.tag_ai }
            },
        );
        let lines: Vec<&str> = rows.iter().map(|(line, _)| line.as_str()).collect();
        assert!(
            lines[0].starts_with("1  P2"),
            "the leader is first: {lines:?}"
        );
        assert!(lines[0].contains("(you)"), "and carries its marker");
        assert!(lines[1].starts_with("2  P3"));
        assert!(lines[2].starts_with("3  P1"));
        assert!(lines[3].starts_with("4  P4"));
        assert!(lines[3].ends_with('0'), "the score is on the line");
        // Each row wears its own seat colour, not its placing colour.
        assert_eq!(rows[0].1, palette::player_color(1));
        assert_eq!(rows[2].1, palette::player_color(0));
    }

    /// Team standings collapse to one row per team, named by the seats on
    /// it, best first.
    #[test]
    fn standings_collapse_to_one_row_a_team() {
        let rows = standings_rows(
            &GameSettings::default(),
            &crate::app::SeatNames::default(),
            &[1, 2, 10, 0, 0, 0],
            4,
            TeamMode::Pairs,
            |_| "",
        );
        assert_eq!(rows.len(), 2, "one row per team");
        assert!(
            rows[0].0.contains("P3+P4"),
            "the leading team leads: {}",
            rows[0].0
        );
        assert!(rows[0].0.trim_end().ends_with("10"), "{}", rows[0].0);
        assert!(rows[1].0.contains("P1+P2"), "{}", rows[1].0);
        assert!(rows[1].0.trim_end().ends_with('3'), "{}", rows[1].0);

        // Six seats in trios: two rows of three, and the mirror-image split
        // means the label lists every member.
        let rows = standings_rows(
            &GameSettings::default(),
            &crate::app::SeatNames::default(),
            &[1, 9, 1, 9, 1, 9],
            6,
            TeamMode::Trios,
            |_| "",
        );
        assert_eq!(rows.len(), 2);
        assert!(rows[0].0.contains("P2+P4+P6"), "{}", rows[0].0);
        assert!(rows[0].0.trim_end().ends_with("27"), "{}", rows[0].0);

        // And six seats in pairs is three rows.
        let rows = standings_rows(
            &GameSettings::default(),
            &crate::app::SeatNames::default(),
            &[1, 1, 5, 5, 3, 3],
            6,
            TeamMode::Pairs,
            |_| "",
        );
        assert_eq!(rows.len(), 3, "2v2v2");
        assert!(rows[0].0.contains("P3+P4"), "{}", rows[0].0);
    }

    #[test]
    fn winner_line_names_the_unique_top_scorer() {
        let (text, _) = winner_line(
            &GameSettings::default(),
            &crate::app::SeatNames::default(),
            &[3, 9, 0, 0, 0, 0],
            2,
            TeamMode::Solo,
        );
        assert_eq!(text, "P2 wins!");
        let (text, _) = winner_line(
            &GameSettings::default(),
            &crate::app::SeatNames::default(),
            &[4, 4, 0, 0, 0, 0],
            2,
            TeamMode::Solo,
        );
        assert_eq!(text, EN.dead_heat);
        // Seats outside the match never win, whatever their array slots say.
        let (text, _) = winner_line(
            &GameSettings::default(),
            &crate::app::SeatNames::default(),
            &[1, 2, 99, 0, 0, 0],
            2,
            TeamMode::Solo,
        );
        assert_eq!(text, "P2 wins!");
    }

    /// A named seat wins under its own name, in the standings and in the
    /// headline both.
    #[test]
    fn a_named_seat_wins_under_its_name() {
        let settings = GameSettings {
            names: std::array::from_fn(|seat| match seat {
                0 => "Anna".to_string(),
                1 => "Bo".to_string(),
                _ => String::new(),
            }),
            ..GameSettings::default()
        };
        let names = crate::app::SeatNames(settings.names.clone());
        let (text, _) = winner_line(&settings, &names, &[3, 9, 0, 0, 0, 0], 2, TeamMode::Solo);
        assert_eq!(text, "Bo wins!");
        let rows = standings_rows(
            &settings,
            &names,
            &[3, 9, 0, 0, 0, 0],
            2,
            TeamMode::Solo,
            |_| "",
        );
        assert!(rows[0].0.contains("Bo"), "{}", rows[0].0);
        assert!(rows[1].0.contains("Anna"), "{}", rows[1].0);
    }

    #[test]
    fn winner_line_sums_teams() {
        let pairs = |scores: &[u32; MAX_PLAYERS], seats| {
            winner_line(
                &GameSettings::default(),
                &crate::app::SeatNames::default(),
                scores,
                seats,
                TeamMode::Pairs,
            )
            .0
        };
        assert_eq!(pairs(&[5, 5, 4, 5, 0, 0], 4), "P1+P2 win!");
        assert_eq!(pairs(&[2, 2, 2, 2, 0, 0], 4), EN.dead_heat);
        // Three pairs on six seats: the winner is named by its members.
        assert_eq!(pairs(&[1, 1, 2, 2, 9, 9], 6), "P5+P6 win!");
        let (text, _) = winner_line(
            &GameSettings::default(),
            &crate::app::SeatNames::default(),
            &[9, 1, 9, 1, 9, 1],
            6,
            TeamMode::Trios,
        );
        assert_eq!(text, "P1+P3+P5 win!");
    }
}
