//! Round-end overlays: the tide-is-in standings card, the puzzle victory
//! card, and the winner confetti.

use crate::app::campaign::WonDoor;
use crate::app::gamepad::{Face, LastDevice, PadFamily};
use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::net::Online;
use crate::app::palette;
use crate::app::seating::leading_seats;
use crate::app::settings::GameSettings;
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
            palette::seat_text(seat as u8),
        );
    }
    match leaders.iter().position(|&led| led) {
        Some(winner) => (
            fill(tr.wins, &[("p", &names.label(tr, winner as u8))]),
            palette::seat_text(winner as u8),
        ),
        None => (tr.dead_heat.to_string(), CARD_TEXT),
    }
}

/// How many pixels a tile is when the results card draws the winner's
/// castle: bigger than on the board, since it is the one thing on the card
/// that is a picture.
const CASTLE_TILE_PX: f32 = 96.0;
/// The castle's tile on a crowded card ([`castle_tile_px`]).
const CASTLE_TILE_PX_CROWDED: f32 = 60.0;
/// The most lines of standings, awards and calls a versus card holds
/// under a full-size castle at the 720-pixel window the interface is drawn
/// for. A table of six with five awards and a series under it ran the
/// card 35 pixels past the top and foot of the window.
const LINES_UNDER_A_FULL_CASTLE: usize = 9;

/// The castle's tile for a versus card with `lines` lines of standings,
/// awards and calls: full size while they leave room, smaller past that,
/// so the card stays inside the window rather than the picture pushing
/// the table off it.
fn castle_tile_px(lines: usize) -> f32 {
    match lines <= LINES_UNDER_A_FULL_CASTLE {
        true => CASTLE_TILE_PX,
        false => CASTLE_TILE_PX_CROWDED,
    }
}

/// The winner's castle on the results card, built from the same pieces as
/// the one on their tile, at the tier they finished on: a round won big
/// shows off a big castle, moat and all.
fn winner_castle(
    card: &mut ChildSpawnerCommands,
    art: &crate::app::art::Art,
    tier: u8,
    color: Color,
    t: f32,
) {
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

/// One line of a card: `text` at `size`, in `color`.
/// A card's line of keys, in all its spellings: the keyboard's, and a
/// pad's player's, from the table of the face the card stands for, with
/// each family's button names ([`PadFamily::ALL`]).
/// [`swap_card_keys`] shows whichever the hands last used want.
#[derive(Component)]
pub struct CardKeys {
    keys: String,
    pads: Vec<(PadFamily, String)>,
}

impl CardKeys {
    /// The keyboard's words, and in front of the face's controls the pad's
    /// `lead`, if the line has one.
    fn new(keys: &str, lead: Option<&str>, face: &Face, tr: &crate::app::i18n::Tr) -> CardKeys {
        let pads = PadFamily::ALL.map(|family| {
            let controls = crate::app::gamepad::scheme::legend(tr, family, face);
            let line = match lead {
                Some(lead) => format!("{lead} | {controls}"),
                None => controls,
            };
            (family, line)
        });
        CardKeys {
            keys: keys.to_string(),
            pads: pads.to_vec(),
        }
    }

    fn line(&self, device: LastDevice) -> &str {
        match device {
            LastDevice::Keyboard => &self.keys,
            LastDevice::Pad(family) => self
                .pads
                .iter()
                .find(|(spelled, _)| *spelled == family)
                .map_or(&self.keys, |(_, line)| line),
        }
    }
}

/// A card's line of keys, as the hands last used read it.
fn keys_line(keys: CardKeys, device: LastDevice, size: f32, color: Color) -> impl Bundle {
    let line = card_line(keys.line(device).to_string(), size, color);
    (line, keys)
}

/// Say a card's keys in the hands last used: the keyboard's, or the pad's
/// buttons, as the prompt line along the bottom does.
pub fn swap_card_keys(device: Res<LastDevice>, mut lines: Query<(&CardKeys, &mut Text)>) {
    for (keys, mut text) in &mut lines {
        menu_ui::set_text(&mut text, keys.line(*device));
    }
}

fn card_line(text: impl Into<String>, size: f32, color: Color) -> (Text, TextFont, TextColor) {
    let (font, ink) = card_text(size, color);
    (Text::new(text), font, ink)
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
        let totals: Vec<u32> = standing.iter().map(|&(_, total, _)| total).collect();
        return standing
            .iter()
            .zip(crate::bots::seat::ranks(&totals))
            .map(|((name, total, seat), place)| {
                (
                    format!("{place}  {name:<13} {total:>4}"),
                    palette::seat_text(*seat),
                )
            })
            .collect();
    }
    // Every row as wide as the widest, name and tag together, so the
    // scores stand in one column: each row is centred on its own, and a
    // long name ("Longnamebot99") pushed its row half a name off the rest.
    let who = |seat: u8| format!("{}{}", names.label(settings.tr(), seat), tag(seat));
    let width = (0..seats)
        .map(|seat| who(seat).chars().count())
        .max()
        .unwrap_or(0)
        .max(MIN_WHO);
    let order = standing_order(scores, seats);
    let sorted: Vec<u32> = order.iter().map(|&seat| scores[seat as usize]).collect();
    order
        .iter()
        .zip(crate::bots::seat::ranks(&sorted))
        .map(|(&seat, place)| {
            (
                format!(
                    "{place}  {:<width$} {:>4}",
                    who(seat),
                    scores[seat as usize]
                ),
                palette::seat_text(seat),
            )
        })
        .collect()
}

/// The narrowest the name column of the standings is drawn: a seat label
/// and its tag, "P2 (AI)", which every row was padded to before rows took
/// the widest name's width.
const MIN_WHO: usize = 9;

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
                palette::seat_text(award.who[0]),
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
    if session.stands.winner.picks.any() {
        let list = session
            .stands
            .winner
            .picks
            .line(|seat| names.label(tr, seat));
        rows.push((
            fill(tr.crowd_picked, &[("l", &list)]),
            CARD_TEXT.darker(0.15),
        ));
    }
    if let Some((seat, right)) = session.stands.winner.last_call {
        let calls = &session.stands.winner.calls;
        let (a, b) = (calls.right.to_string(), calls.made.to_string());
        rows.push(match right {
            true => (fill(tr.call_right, &[("a", &a), ("b", &b)]), palette::GOLD),
            false => (
                fill(
                    tr.call_wrong,
                    &[("p", &names.label(tr, seat)), ("a", &a), ("b", &b)],
                ),
                palette::seat_text(seat),
            ),
        });
    }
    rows
}

/// What a finished round fed into besides its own scores: the day's best,
/// the reel being cut from it, the series it belongs to, the awards, and
/// its recording waiting to be kept.
#[derive(bevy::ecs::system::SystemParam)]
pub struct RoundExtras<'w> {
    highlight: Res<'w, crate::app::Highlight>,
    stats: Res<'w, crate::app::achievements::Stats>,
    tournament: Res<'w, crate::app::tournament::Tournament>,
    tally: Res<'w, crate::app::awards::RoundTally>,
    kinds: Res<'w, crate::app::SeatKinds>,
    /// The hands the card's line of keys is said for.
    device: Res<'w, LastDevice>,
    /// Whether the table can be played again from the card.
    table: Res<'w, crate::app::rematch::LocalTable>,
    /// Whether the round can be kept on the replay shelf.
    keep: Res<'w, crate::app::replays::RoundToKeep>,
}

/// The tide-is-in standings card: winner headline, ranked scores in seat
/// colours (with AI/you markers), the round's awards, and its total haul.
pub fn spawn_versus_results(
    mut commands: Commands,
    sim: Res<Sim>,
    scoring: crate::app::teams::Scoring,
    art: Res<crate::app::art::Art>,
    mut rng: ResMut<crate::app::effects::VisualRng>,
    seating: crate::app::seating::Seating,
    extras: RoundExtras,
) {
    let viewer = seating.viewer();
    let crate::app::seating::Seating {
        seats,
        controllers,
        names,
        online,
        playback: _,
    } = seating;
    let RoundExtras {
        highlight,
        stats,
        tournament,
        tally,
        kinds,
        device,
        table,
        keep,
    } = extras;
    let board = &sim.0;
    let scores = board.scores();
    let count = seats.0.max(2);
    let mode = scoring.mode(count);
    let crate::app::teams::LocalScoring { settings, daily } = &scoring.local;
    let tr = settings.tr();
    let (headline, headline_color) = winner_line(settings, &names, scores, count, mode);
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

    let rows = standings_rows(settings, &names, scores, count, mode, |seat| {
        crate::app::seating::seat_tag(tr, &controllers, viewer, seat)
    });
    let haul = board.crabs_banked();
    let awards = award_rows(settings, &names, &tally, board, seats.0, mode);
    let crowd = crowd_rows(settings, &names, &online);

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
                let lines = rows.len() + awards.len() + crowd.len();
                winner_castle(
                    card,
                    &art,
                    tier,
                    palette::player_color(seat),
                    castle_tile_px(lines),
                );
            }
            card.spawn((
                Text::new(headline),
                menu_ui::display_font(34.0),
                TextColor(headline_color),
            ));
            card.spawn(menu_ui::spacer(6.0));
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
                // At a table with a bot every row leads with a robot's
                // room, and only the bots' draw one: the columns of the
                // table stay lined up whoever holds which seat.
                let bot = seat.filter(|&seat| kinds.bot(seat));
                let room = match bot {
                    Some(seat) => crate::app::seating::robot_inline(&art, seat, 22.0),
                    // The robot itself, unseen: a box only takes its room
                    // once its image has loaded, and Bevy's own blank one
                    // never does, so it took none and these rows sat a
                    // robot's width off the bots'.
                    None => InlineImage {
                        color: Color::NONE,
                        ..crate::app::seating::robot_inline(&art, 0, 22.0)
                    },
                };
                let (font, ink) = (row.0.clone(), row.1);
                card.spawn((Text::new(""), row.0, row.1))
                    .with_children(|text| {
                        text.spawn(room);
                        text.spawn((
                            TextSpan::new(crate::app::seating::beside_the_robot(true, &line)),
                            font,
                            ink,
                        ));
                    });
            }
            if !awards.is_empty() {
                card.spawn(menu_ui::spacer(6.0));
            }
            for (line, color) in awards {
                card.spawn(card_line(line, 17.0, color));
            }
            for (line, color) in crowd {
                card.spawn(card_line(line, 17.0, color));
            }
            card.spawn(menu_ui::spacer(6.0));
            card.spawn(card_line(
                match haul {
                    1 => tr.haul_one.to_string(),
                    _ => fill(tr.haul, &[("n", &haul.to_string())]),
                },
                17.0,
                CARD_TEXT.darker(0.15),
            ));
            // The reel is written off-thread and is usually not there yet
            // when the card goes up: the line is spawned hidden and shown
            // by `update_highlight_line` once the save has happened.
            card.spawn((
                highlight_line_node(highlight.0.is_some()),
                card_line(
                    highlight_line_text(tr, &highlight),
                    15.0,
                    CARD_TEXT.darker(0.3),
                ),
                HighlightLine,
            ));
            if daily.active {
                card.spawn(card_line(
                    fill(tr.daily_best, &[("n", &stats.daily_best.to_string())]),
                    17.0,
                    palette::GOLD,
                ));
            }
            // The keys, in the words the prompt line uses, from the same
            // answers the keys read: where Enter goes, what R plays again
            // at a table set up here, and the offer to keep the round.
            let rematch = crate::app::rematch::rematch(&table, &online, &tournament);
            let offer = keep.offer();
            let door = (
                keys_line(
                    over_keys(tr, &online, &tournament, rematch, offer),
                    *device,
                    17.0,
                    CARD_TEXT.darker(0.15),
                ),
                OverKeys,
            );
            if tournament.in_series() {
                series_rows(card, settings, &names, &tournament, door, mode, count);
            } else if rematch.is_some() || offer != crate::app::replays::KeepOffer::Nothing {
                card.spawn(menu_ui::spacer(6.0));
                card.spawn(door);
            }
        });
    });
}

/// The versus results card's line of keys, which changes while the card
/// is up: the offer to keep the round turns into its confirmation.
#[derive(Component)]
pub struct OverKeys;

/// The versus results card's keys in both spellings: the keyboard's line
/// as the prompt line has it ([`crate::app::rematch::over_prompt`]), and a
/// pad's from the results table, led by the word that the round is kept.
fn over_keys(
    tr: &crate::app::i18n::Tr,
    online: &Online,
    tournament: &crate::app::tournament::Tournament,
    rematch: Option<crate::app::rematch::Rematch>,
    keep: crate::app::replays::KeepOffer,
) -> CardKeys {
    let series_on = tournament.is_running();
    let face = crate::app::gamepad::face::results_face(online, series_on, rematch, keep);
    let keys = crate::app::rematch::over_prompt(tr, online, series_on, rematch, keep);
    CardKeys::new(&keys, crate::app::replays::kept_lead(tr, keep), &face, tr)
}

/// Say on the card that the round is kept, the moment it is, and what R
/// does the moment that changes.
pub fn refresh_over_keys(
    keep: Res<crate::app::replays::RoundToKeep>,
    settings: Res<GameSettings>,
    online: Res<Online>,
    tournament: Res<crate::app::tournament::Tournament>,
    table: Res<crate::app::rematch::LocalTable>,
    mut lines: Query<&mut CardKeys, With<OverKeys>>,
) {
    // The table changes when R asks before restarting a series, and when
    // the question is taken back.
    if !keep.is_changed() && !table.is_changed() {
        return;
    }
    let rematch = crate::app::rematch::rematch(&table, &online, &tournament);
    for mut keys in &mut lines {
        *keys = over_keys(settings.tr(), &online, &tournament, rematch, keep.offer());
    }
}

/// A series' part of the round's card: which round this was, each seat's
/// wins, the champion or a drawn series once it is decided, and the line
/// of keys: the door to the next round, or the way on after the last.
fn series_rows(
    card: &mut ChildSpawnerCommands,
    settings: &GameSettings,
    names: &crate::app::SeatNames,
    tournament: &crate::app::tournament::Tournament,
    door: impl Bundle,
    mode: TeamMode,
    count: u8,
) {
    let tr = settings.tr();
    card.spawn(menu_ui::spacer(6.0));
    card.spawn(card_line(
        crate::app::tournament::round_line(tr, tournament),
        17.0,
        CARD_TEXT.darker(0.15),
    ));
    // Each seat's wins in its own colour, as many to a line as fit. One
    // gold line joined with dots ran a table of six past the card's frame,
    // and six full names past the window.
    card.spawn(Node {
        flex_wrap: FlexWrap::Wrap,
        justify_content: JustifyContent::Center,
        column_gap: Val::Px(SERIES_GAP),
        max_width: Val::Px(SERIES_W),
        ..default()
    })
    .with_children(|series| {
        for standing in crate::app::tournament::standings(settings, names, tournament, mode, count)
        {
            series.spawn((
                card_line(standing.line(), SERIES_FONT, standing.color),
                TextLayout::no_wrap(),
            ));
        }
    });
    if tournament.is_decided() {
        if let Some(champ) = tournament.winner(mode, count) {
            let (who, seat) =
                crate::app::tournament::champion_name(settings, names, mode, champ, count);
            card.spawn(card_line(
                fill(tr.tour_champion, &[("p", &who)]),
                26.0,
                palette::seat_text(seat),
            ));
        } else {
            // Over with nobody ahead. The card used to say nothing at all,
            // neither a champion nor a next round, as though the series had
            // simply stopped.
            card.spawn(card_line(tr.tour_drawn, 26.0, CARD_TEXT));
        }
    }
    // Under a decided series the line is still the way on: Enter to the
    // menu and, at a couch table, R for the series again.
    card.spawn(door);
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
    coop: Res<crate::app::Coop>,
    (settings, device): (Res<GameSettings>, Res<LastDevice>),
    sim: Res<Sim>,
    art: Res<crate::app::art::Art>,
) {
    let tr = settings.tr();
    let name = campaign
        .shown_name(campaign.index, settings.language)
        .to_string();
    // The shipped campaign ends with its last shipped stage; the player's
    // own levels behind it are a shelf, not stage eighty-three, and the
    // last of those ends only the list. So the card counts this level
    // within whichever of the two it belongs to, and only the shipped
    // section's end is the campaign's end.
    let door = campaign.won_door(*coop);
    let board = &sim.0;
    // The castle the crabs were routed to: the first on the beach, which
    // is the only one in all but a handful of stages.
    let castle = board.castle_owners().next();
    let card = results_card(&mut commands);
    commands.entity(card).with_children(|wrap| {
        let mut frame = wrap.spawn(menu_ui::screen_card());
        frame.insert(BackgroundColor(palette::CARD_BG));
        frame.with_children(|card| {
            let saved_all = campaign.current().goal.saves_every_crab();
            let title = won_title(tr, campaign.place(), saved_all);
            card.spawn((
                Text::new(title),
                menu_ui::display_font(34.0),
                TextColor(palette::GOLD),
            ));
            if let Some(owner) = castle {
                let tier = crate::sim::castle_tier(board.scores()[usize::from(owner)]);
                winner_castle(
                    card,
                    &art,
                    tier,
                    palette::player_color(owner),
                    CASTLE_TILE_PX,
                );
            }
            saved_row(card, &art, &campaign, board);
            card.spawn(card_line(level_line(tr, &campaign, &name), 21.0, CARD_TEXT));
            let keys = won_keys(tr, door, saved_all);
            let lead = match (door, saved_all) {
                (WonDoor::Menu, _) => tr.pad.last_level,
                (WonDoor::Next | WonDoor::Stages, true) => tr.pad.won,
                (WonDoor::Next | WonDoor::Stages, false) => tr.pad.won_goal,
            };
            let keys = CardKeys::new(keys, Some(lead), &Face::PuzzleWon { door }, tr);
            card.spawn(keys_line(keys, *device, 17.0, CARD_TEXT.darker(0.15)));
        });
    });
}

/// The won card's line of keys, and the header's prompt under it: where
/// Enter goes ([`Campaign::won_door`]), after whether every crab came home
/// or the goal was met. On the last stage the card says the run is over
/// and Enter goes home; a line still offering "next level" under it, or
/// before a co-op stage only one is seated for, is the game arguing with
/// itself.
pub fn won_keys(tr: &crate::app::i18n::Tr, door: WonDoor, saved_all: bool) -> &'static str {
    match (door, saved_all) {
        (WonDoor::Menu, _) => tr.last_level,
        (WonDoor::Next, true) => tr.prompt_won,
        (WonDoor::Next, false) => tr.prompt_won_goal,
        (WonDoor::Stages, true) => tr.prompt_won_stages,
        (WonDoor::Stages, false) => tr.prompt_won_goal_stages,
    }
}

/// The won card's heading. The campaign's own last stage ends it, and
/// only that one: the player's own levels behind it are a shelf, not stage
/// a hundred and one, and the co-op shelf is a ladder of its own for two,
/// whose last stage is not where the beach is made safe. Every other clear
/// says whether every crab came home or the goal was met.
fn won_title(
    tr: &crate::app::i18n::Tr,
    place: crate::app::campaign::Place,
    saved_all: bool,
) -> &'static str {
    use crate::app::campaign::Section;
    let campaign_end = place.at == place.of && place.section == Section::TidePool;
    match (campaign_end, saved_all) {
        (true, _) => tr.campaign_done,
        (false, true) => tr.all_safe,
        (false, false) => tr.goal_met,
    }
}

/// Which level a puzzle card is about: its place in its own section and
/// its name. Both the Won and the Lost card say it in these words, so the
/// player's own levels read as a shelf of their own on either, the way the
/// header bar counts them.
fn level_line(tr: &crate::app::i18n::Tr, campaign: &Campaign, name: &str) -> String {
    let crate::app::campaign::Place { at, of, section } = campaign.place();
    let shelf = match section {
        crate::app::campaign::Section::Custom => format!("{}  ", tr.stage_custom),
        crate::app::campaign::Section::Coop => format!("{}  ", tr.stage_coop),
        crate::app::campaign::Section::TidePool => String::new(),
    };
    format!("{shelf}{at} / {of}  -  {name}")
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
                icon.spawn(layer(&art.claw, claw_color(handed, level.is_coop())));
            });
        }
    });
}

/// What a lost stage is called, as the card's heading and the prompt line
/// under it. A stage that saves every crab is lost to a crab that was not,
/// but one asking for a number of crabs or the golden one is lost only on
/// the clock (`Level::outcome`), maybe with every crab still safe.
pub(crate) fn loss_words(
    tr: &crate::app::i18n::Tr,
    goal: crate::sim::Goal,
) -> (&'static str, &'static str) {
    match goal.saves_every_crab() {
        true => (tr.crabs_lost, tr.prompt_lost),
        false => (tr.goal_missed, tr.prompt_lost_goal),
    }
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
    device: Res<LastDevice>,
    // Optional, so the card goes up whatever else is loaded: the row of
    // crabs is a picture on it, and the words are the news.
    sim: Option<Res<Sim>>,
    art: Option<Res<crate::app::art::Art>>,
) {
    let tr = settings.tr();
    let name = campaign
        .shown_name(campaign.index, settings.language)
        .to_string();
    let (heading, prompt) = loss_words(tr, campaign.current().goal);
    let card = results_card(&mut commands);
    commands.entity(card).with_children(|wrap| {
        let mut frame = wrap.spawn(menu_ui::screen_card());
        frame.insert((
            BackgroundColor(palette::CARD_BG),
            BorderColor::all(palette::INK_RAID.with_alpha(0.6)),
        ));
        frame.with_children(|card| {
            card.spawn((
                Text::new(heading),
                menu_ui::display_font(34.0),
                TextColor(palette::INK_RAID),
            ));
            if let (Some(sim), Some(art)) = (&sim, &art) {
                saved_row(card, art, &campaign, &sim.0);
            }
            card.spawn(card_line(level_line(tr, &campaign, &name), 21.0, CARD_TEXT));
            // The hint has a line of its own under the header, so the
            // card names the ways on.
            let lead = match campaign.current().goal.saves_every_crab() {
                true => tr.pad.lost,
                false => tr.pad.lost_goal,
            };
            let keys = CardKeys::new(prompt, Some(lead), &Face::PuzzleLost { hint: false }, tr);
            card.spawn(keys_line(keys, *device, 17.0, CARD_TEXT.darker(0.15)));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::EN;

    /// The crowd's lines say what the host counted: the calls listed most
    /// called first, and a spectator's own call, right in gold with its
    /// running tally or wrong in the colour of the seat it called. An
    /// offline round has no crowd.
    #[test]
    fn the_crowds_lines_say_the_calls_and_how_ones_own_went() {
        use crate::app::spectators::{Calls, Picks};
        let settings = GameSettings::default();
        let names = crate::app::SeatNames::default();
        assert!(crowd_rows(&settings, &names, &Online::default()).is_empty());
        let mut session = crate::app::net::OnlineSession::new(
            crate::transport::UdpTransport::host(0).expect("socket"),
            crate::sim::Lockstep::observer(vec![0, 1, 2, 3], crate::sim::DEFAULT_DELAY),
            4,
            crate::transport::MatchTerms::default(),
        );
        session.stands.winner.picks = Picks {
            counts: [0, 2, 0, 1, 0, 0],
            open: 0,
        };
        session.stands.winner.calls = Calls { right: 1, made: 3 };
        session.stands.winner.last_call = Some((1, false));
        let mut online = Online(Some(session));
        let rows = crowd_rows(&settings, &names, &online);
        let texts: Vec<&str> = rows.iter().map(|(text, _)| text.as_str()).collect();
        assert_eq!(
            texts,
            [
                fill(EN.crowd_picked, &[("l", "P2 ×2 · P4 ×1")]),
                fill(EN.call_wrong, &[("p", "P2"), ("a", "1"), ("b", "3")]),
            ]
        );
        assert_eq!(rows[1].1, palette::seat_text(1));

        let session = online.0.as_mut().expect("a session");
        session.stands.winner.picks = Picks::default();
        session.stands.winner.last_call = Some((1, true));
        let rows = crowd_rows(&settings, &names, &online);
        assert_eq!(
            rows,
            [(
                fill(EN.call_right, &[("a", "1"), ("b", "3")]),
                palette::GOLD
            )]
        );
    }

    /// The awards are said for a round the tally saw whole at a table of
    /// two or more, and for nothing else: one picked up part way would
    /// credit only the part it saw.
    #[test]
    fn awards_are_said_only_for_a_round_seen_whole() {
        use crate::app::awards::{Reading, RoundTally};
        use crate::sim::{BotLevel, PlayerAction, bot_action, classic_arena};
        let settings = GameSettings::default();
        let names = crate::app::SeatNames::default();
        let play = |from: u64| {
            let mut board = classic_arena(false, 2);
            let mut tally = RoundTally::default();
            for _ in 0..600 {
                let actions: [PlayerAction; MAX_PLAYERS] =
                    std::array::from_fn(|seat| bot_action(&board, seat as u8, BotLevel::Hard));
                let before = Reading::of(&board);
                board.tick(&actions);
                if board.ticks() > from {
                    tally.observe(before, &board, &actions);
                }
            }
            (board, tally)
        };
        let (board, tally) = play(0);
        let said = award_rows(&settings, &names, &tally, &board, 2, TeamMode::Solo);
        assert!(!said.is_empty(), "a round seen whole earns something");
        assert!(award_rows(&settings, &names, &tally, &board, 1, TeamMode::Solo).is_empty());
        let (board, tally) = play(1);
        assert!(
            award_rows(&settings, &names, &tally, &board, 2, TeamMode::Solo).is_empty(),
            "a round seen from its second tick"
        );
    }

    /// A series champion in teams is named as its team and drawn in its
    /// lowest seat's colour; one alone is its own seat.
    #[test]
    fn a_champion_team_is_named_as_the_team() {
        use crate::app::tournament::{Champion, champion_name};
        let settings = GameSettings::default();
        let names = crate::app::SeatNames::default();
        let named = |mode, champion, seats| champion_name(&settings, &names, mode, champion, seats);
        assert_eq!(
            named(TeamMode::Pairs, Champion::Team(1), 4),
            ("P3+P4".to_string(), 2)
        );
        assert_eq!(
            named(TeamMode::Trios, Champion::Team(1), 6),
            ("P2+P4+P6".to_string(), 1)
        );
        assert_eq!(
            named(TeamMode::Solo, Champion::Seat(3), 4),
            ("P4".to_string(), 3)
        );
    }

    /// The beach is made safe on the Tide Pool's last stage, not on the
    /// co-op shelf's last, which is a ladder of its own.
    #[test]
    fn only_the_campaigns_last_stage_makes_the_beach_safe() {
        let mut campaign = Campaign::tide_pool();
        let tide_end = campaign.levels.len() - 1;
        let coop = crate::sim::coop_levels();
        campaign.coop = coop.len();
        campaign.levels.extend(coop);
        campaign.builtins = campaign.levels.len();
        let title = |at: usize| won_title(&EN, campaign.place_of(at), true);
        assert_eq!(title(tide_end), EN.campaign_done);
        assert_eq!(title(campaign.levels.len() - 1), EN.all_safe);
        assert_eq!(title(0), EN.all_safe);
    }

    /// Losing a puzzle raises a card over the held board, and leaving the
    /// phase takes it down again: the stop is said, not left to be guessed.
    #[test]
    fn a_lost_puzzle_says_so_on_a_card() {
        use crate::app::{Phase, Screen};

        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_resource(Campaign {
            index: 3,
            ..Campaign::tide_pool()
        });
        app.insert_resource(GameSettings::default());
        app.init_resource::<LastDevice>();
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

    /// A stage asking for a number of crabs or the golden one is lost on
    /// the clock, so its card says that rather than blaming crabs that may
    /// all be safe.
    #[test]
    fn a_goal_lost_on_the_clock_is_not_blamed_on_the_crabs() {
        use crate::sim::Goal;
        assert_eq!(loss_words(&EN, Goal::AllCrabs).0, EN.crabs_lost);
        assert_eq!(loss_words(&EN, Goal::Survive).1, EN.prompt_lost);
        assert_eq!(loss_words(&EN, Goal::Golden).0, EN.goal_missed);
        assert_eq!(loss_words(&EN, Goal::Bank(5)).1, EN.prompt_lost_goal);
    }

    /// A player's own level is counted on its own shelf on the Lost card
    /// just as on the Won card and the header: driftwood one, not stage a
    /// hundred and one.
    #[test]
    fn a_lost_custom_level_counts_on_its_own_shelf() {
        use crate::sim::campaign_levels;
        let mut levels = campaign_levels();
        let builtins = levels.len();
        levels.push(levels[0].clone());
        let campaign = Campaign {
            index: builtins,
            builtins,
            ..Campaign::tide_pool_of(levels)
        };
        let line = level_line(&EN, &campaign, "Mine");
        assert_eq!(line, format!("{}  1 / 1  -  Mine", EN.stage_custom));
        let shipped = Campaign {
            index: 3,
            ..campaign
        };
        assert_eq!(
            level_line(&EN, &shipped, "Four"),
            format!("4 / {builtins}  -  Four")
        );
    }

    /// The card's line of keys offers the round and then says it is kept,
    /// in the keyboard's words and in a pad's.
    #[test]
    fn the_card_line_turns_the_offer_into_its_confirmation() {
        use crate::app::replays::KeepOffer;
        let (online, tournament) = (
            Online::default(),
            crate::app::tournament::Tournament::default(),
        );
        let tr = &EN;
        let offered = over_keys(tr, &online, &tournament, None, KeepOffer::Offered);
        assert_eq!(offered.keys, "Enter: menu | G: keep replay");
        assert!(
            offered.line(LastDevice::PAD).contains("X: keep replay"),
            "{}",
            offered.line(LastDevice::PAD)
        );
        let kept = over_keys(tr, &online, &tournament, None, KeepOffer::Kept);
        assert_eq!(kept.keys, "Enter: menu | replay kept");
        assert!(
            kept.line(LastDevice::PAD).starts_with("replay kept | "),
            "{}",
            kept.line(LastDevice::PAD)
        );
        assert!(
            !kept.line(LastDevice::PAD).contains("X:"),
            "{}",
            kept.line(LastDevice::PAD)
        );
    }

    /// A spectator's card names Y for talking to the table, as the bridge
    /// presses it; a player's does not.
    #[test]
    fn a_spectators_card_says_y_talks() {
        use crate::app::replays::KeepOffer;
        use crate::sim::{DEFAULT_DELAY, Lockstep};
        use crate::transport::{MatchTerms, UdpTransport};
        let at_table = |step| {
            Online(Some(crate::app::net::OnlineSession::new(
                UdpTransport::host(0).expect("socket"),
                step,
                2,
                MatchTerms::default(),
            )))
        };
        let tournament = crate::app::tournament::Tournament::default();
        let tr = &EN;
        let watching = at_table(Lockstep::observer(vec![0, 1], DEFAULT_DELAY));
        let card = over_keys(tr, &watching, &tournament, None, KeepOffer::Offered);
        assert!(
            card.line(LastDevice::PAD).contains("Y: talk"),
            "{}",
            card.line(LastDevice::PAD)
        );
        let seated = at_table(Lockstep::new(1, vec![0, 1], DEFAULT_DELAY));
        let card = over_keys(tr, &seated, &tournament, None, KeepOffer::Offered);
        assert!(
            !card.line(LastDevice::PAD).contains("Y: talk"),
            "{}",
            card.line(LastDevice::PAD)
        );
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
        assert_eq!(rows[0].1, palette::seat_text(1));
        assert_eq!(rows[2].1, palette::seat_text(0));
    }

    /// Equal scores share a place, and the next row down skips past them:
    /// a dead heat is not drawn as a first and a second.
    #[test]
    fn tied_scores_share_a_place() {
        assert_eq!(crate::bots::seat::ranks(&[9, 5, 5, 0]), vec![1, 2, 2, 4]);
        assert_eq!(crate::bots::seat::ranks(&[4, 4]), vec![1, 1]);
        let rows = standings_rows(
            &GameSettings::default(),
            &crate::app::SeatNames::default(),
            &[7, 7, 2, 0, 0, 0],
            3,
            TeamMode::Solo,
            |_| "",
        );
        let lines: Vec<&str> = rows.iter().map(|(line, _)| line.as_str()).collect();
        assert!(lines[0].starts_with("1  P1"), "{lines:?}");
        assert!(lines[1].starts_with("1  P2"), "{lines:?}");
        assert!(lines[2].starts_with("3  P3"), "{lines:?}");
        let teams = standings_rows(
            &GameSettings::default(),
            &crate::app::SeatNames::default(),
            &[3, 3, 3, 3, 0, 0],
            4,
            TeamMode::Pairs,
            |_| "",
        );
        assert!(teams[0].0.starts_with("1 "), "{teams:?}");
        assert!(teams[1].0.starts_with("1 "), "{teams:?}");
    }

    /// The first row's number is [`teams::top_total`], the one the replay
    /// shelf shows a kept round won with: a seat's score, a team's total,
    /// or the score a dead heat tied on.
    ///
    /// [`teams::top_total`]: crate::app::teams::top_total
    #[test]
    fn the_first_row_shows_the_top_total() {
        let cases: [([u32; MAX_PLAYERS], u8, TeamMode); 5] = [
            ([3, 9, 5, 0, 0, 0], 4, TeamMode::Solo),
            ([7, 7, 2, 0, 0, 0], 3, TeamMode::Solo),
            ([1, 2, 10, 0, 0, 0], 4, TeamMode::Pairs),
            ([1, 9, 1, 9, 1, 9], 6, TeamMode::Trios),
            ([3, 3, 3, 3, 0, 0], 4, TeamMode::Pairs),
        ];
        for (scores, seats, mode) in cases {
            let rows = standings_rows(
                &GameSettings::default(),
                &crate::app::SeatNames::default(),
                &scores,
                seats,
                mode,
                |_| "",
            );
            let shown = rows[0].0.split_whitespace().last().expect("a number");
            assert_eq!(
                shown,
                crate::app::teams::top_total(&scores, seats, mode).to_string(),
                "{scores:?} {mode:?}"
            );
        }
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
