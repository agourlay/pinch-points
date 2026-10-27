//! The line-up before a match: who is playing, in seat order and in their
//! colours, team by team under a team mode, and the map, its name and its
//! picture, held up for a few seconds before the count.
//!
//! On the couch it opens every round of a table set up here, for the same
//! [`LINEUP_SECS`], and the count follows: there is no network to wait on.
//! The pause card stops it as it stops the count.
//!
//! Every peer at the lobby card when the host started the match shows it,
//! for the same fixed [`LINEUP_SECS`] from its own `Start`, ahead of the
//! count ([`crate::app::countdown`]), which every peer already runs on its
//! own clock from the same moment. So it adds to how long the beach holds
//! still and to nothing else: the round, the sim and the hash are where
//! they were, every peer starts on the same skew its `Start` arrived with,
//! and the round-holding checks wait for the round as they always have.
//! Every online round played from its first frame opens on it: the map
//! was picked for it, at the start by the lobby's page and between the
//! rounds of a series by the host's ([`crate::app::next_map`]), so every
//! peer is shown what was picked.

use crate::app::cycle::Cycle;
use crate::app::i18n::{Tr, fill};
use crate::app::match_setup::{MapChoice, MapPreviews, Pic, TermsDials, cut_to, line_px};
use crate::app::menu_ui;
use crate::app::net::Online;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::{Controllers, SeatController, SeatNames, Seats};
use crate::sim::MAX_PLAYERS;
use bevy::prelude::*;

/// How long the line-up is up before the count, in seconds: long enough
/// to find one's own name and colour and read the map's, and short enough
/// that nobody waits on it.
pub const LINEUP_SECS: f32 = 4.0;

/// Everything the line-up puts on screen, under one root.
#[derive(Component)]
pub struct LineupUi;

/// The map's picture on the line-up, filled once it is drawn.
#[derive(Component)]
struct LineupPicture;

/// The line-up card's width.
pub(crate) const CARD_W: f32 = 940.0;
/// The map's picture, at three quarters of the size it is drawn at.
pub(crate) const PICTURE: Vec2 = Vec2::new(
    0.75 * crate::app::match_setup::PREVIEW_PX.x as f32,
    0.75 * crate::app::match_setup::PREVIEW_PX.y as f32,
);
/// The players' column, beside the map's.
pub(crate) const COLUMN_W: f32 = CARD_W - 2.0 * menu_ui::CARD_PAD - PICTURE.x - COLUMN_GAP;
/// The air between the map's column and the players'.
const COLUMN_GAP: f32 = 24.0;
/// A player's row: the disc, the name and what plays the seat.
pub(crate) const NAME_W: f32 = 250.0;
pub(crate) const KIND_W: f32 = 170.0;
pub(crate) const NAME_FONT: f32 = 22.0;
pub(crate) const KIND_FONT: f32 = 16.0;
pub(crate) const TITLE_FONT: f32 = 30.0;
pub(crate) const MAP_FONT: f32 = 20.0;
pub(crate) const SUB_FONT: f32 = 15.0;
const DISC: f32 = 24.0;
const ROW_H: f32 = 34.0;
/// The air above every team's heading but the first.
const TEAM_GAP: f32 = 6.0;

/// The map the line-up names, as a peer names the host's: the handmade
/// beach by its own name when the round is played on it, else the map
/// the terms name, and under it what it is when there is more to say.
pub(crate) fn map_words(
    tr: &Tr,
    session: &crate::app::net::OnlineSession,
) -> (String, Option<&'static str>) {
    use crate::app::match_setup::{beach_from, beach_in_play, map_name};
    let map = session.terms.map();
    let name = beach_in_play(map, beach_from(&session.beach), session.seats)
        .map_or_else(|| map_name(tr, map), |level| level.name);
    let sub = (map == MapChoice::Surprise).then_some(tr.setup_surprise_sub);
    (name, sub)
}

/// What plays `seat` beside its name: the AI at its level, a bot, or a
/// person, who needs nothing said.
pub(crate) fn seat_kind(tr: &Tr, controller: SeatController) -> String {
    match controller {
        SeatController::Ai(level) => fill(tr.setup_by_ai, &[("l", tr.bot_levels[level.index()])]),
        SeatController::Bot => tr.setup_by_bot.to_string(),
        SeatController::Local | SeatController::Remote => String::new(),
    }
}

/// The seats in the order the line-up lists them: seat order, or under a
/// team mode team by team, the first seat of each team with the team it
/// heads.
pub(crate) fn seat_order(mode: crate::app::teams::TeamMode, seats: u8) -> Vec<(u8, Option<u8>)> {
    let teams = mode != crate::app::teams::TeamMode::Solo;
    (0..mode.teams(seats))
        .flat_map(|team| {
            mode.seats_of(team, seats)
                .into_iter()
                .enumerate()
                .map(move |(n, seat)| (seat, (teams && n == 0).then_some(team)))
        })
        .collect()
}

/// A team's heading on the line-up: its seats' names joined, as the
/// header's tally and the results card name a team, in the colour of the
/// seat that stands for it.
///
/// Across the players' column ([`COLUMN_W`]), and when the names joined
/// run past it (three of twelve kana do), each is given the same room,
/// the most that lets them all fit, and cut to it ([`cut_to`]): a short
/// name is whole, and of the long ones none is cut more than another.
pub(crate) fn team_heading(
    tr: &Tr,
    names: &SeatNames,
    mode: crate::app::teams::TeamMode,
    team: u8,
    seats: u8,
) -> (String, Color) {
    use crate::app::teams::team_label;
    let label = |room: f32| {
        team_label(mode, team, seats, |seat| {
            cut_to(&names.label(tr, seat), room, SUB_FONT)
        })
    };
    let fits = |line: &str| line_px(line, SUB_FONT) <= COLUMN_W;
    let mut room = COLUMN_W;
    let mut said = label(room);
    while !fits(&said) && room > 0.0 {
        room -= 1.0;
        said = label(room);
    }
    let face = crate::app::teams::face_of(mode, team, seats);
    (said, palette::seat_text(face))
}

/// The map a couch round is played on, named as its card names it: a
/// beach off the shelf by its own name, else the map's, and under it what
/// it is when there is more to say. The Random card was drawn as the
/// match started, so the map in force is the one played.
pub(crate) fn couch_map_words(
    tr: &Tr,
    config: &crate::app::match_setup::MatchConfig,
    beaches: &crate::app::match_setup::CustomBeaches,
) -> (String, Option<&'static str>) {
    use crate::app::match_setup::map_name;
    let shelf = beaches.fitting(config.seats);
    let beach = shelf
        .get(config.custom)
        .filter(|_| config.map == MapChoice::Custom);
    let name = beach.map_or_else(
        || map_name(tr, config.map),
        |beach| beach.level.name.clone(),
    );
    let sub = (config.map == MapChoice::Surprise).then_some(tr.setup_surprise_sub);
    (name, sub)
}

/// What the line-up is built from.
#[derive(bevy::ecs::system::SystemParam)]
pub struct LineupParts<'w> {
    countdown: Res<'w, crate::app::countdown::Countdown>,
    online: Res<'w, Online>,
    settings: Res<'w, GameSettings>,
    names: Res<'w, SeatNames>,
    controllers: Res<'w, Controllers>,
    seats: Res<'w, Seats>,
    couch: CouchTable<'w>,
}

/// The table a couch round was set up with: its map, and the daily, which
/// is scored every seat for itself ([`crate::app::teams::local`]).
#[derive(bevy::ecs::system::SystemParam)]
pub struct CouchTable<'w> {
    config: Res<'w, crate::app::match_setup::MatchConfig>,
    beaches: Res<'w, crate::app::match_setup::CustomBeaches>,
    daily: Res<'w, crate::app::Daily>,
}

/// Put the line-up up for a round that opens on one, and take it down
/// once its time is up.
pub fn show_lineup(
    mut commands: Commands,
    parts: LineupParts,
    built: Query<Entity, With<LineupUi>>,
) {
    let LineupParts {
        countdown,
        online,
        settings,
        names,
        controllers,
        seats,
        couch,
    } = parts;
    let up = countdown.lineup_left() > 0.0;
    let seats = seats.0.min(MAX_PLAYERS as u8);
    // As the round scores it: online the terms' mode, on the couch this
    // machine's, where the table fits it.
    let local = crate::app::teams::local(&settings, &couch.daily);
    let mode = crate::app::teams::in_play(local, &online, None, seats);
    if !up {
        for root in &built {
            commands.entity(root).despawn();
        }
        return;
    }
    if !built.is_empty() {
        return;
    }
    let tr = settings.tr();
    // Online this peer's own seat is marked, and a watcher told it is
    // one; on the couch every seat is somebody here.
    let (map, sub, watching, mine) = match online.0.as_ref() {
        Some(session) => {
            let (map, sub) = map_words(tr, session);
            (map, sub, session.watching(), session.session.seat())
        }
        None => {
            let (map, sub) = couch_map_words(tr, &couch.config, &couch.beaches);
            (map, sub, false, None)
        }
    };
    commands
        .spawn((
            LineupUi,
            GlobalZIndex(menu_ui::layer::BANNER),
            Node {
                flex_direction: FlexDirection::Column,
                ..menu_ui::centred_overlay()
            },
        ))
        .with_children(|veil| {
            let (mark, mut node, fill, edge, shadow) = menu_ui::screen_card();
            node.width = Val::Px(CARD_W);
            node.align_items = AlignItems::Stretch;
            veil.spawn((mark, node, fill, edge, shadow))
                .with_children(|card| {
                    card.spawn((
                        Text::new(tr.lineup_title),
                        menu_ui::display_font(TITLE_FONT),
                        TextColor(palette::GOLD),
                        Node {
                            align_self: AlignSelf::Center,
                            margin: UiRect::bottom(Val::Px(12.0)),
                            ..default()
                        },
                    ));
                    card.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(COLUMN_GAP),
                        ..default()
                    })
                    .with_children(|row| {
                        map_column(row, &map, sub);
                        let table = Table {
                            names: &names,
                            controllers: &controllers,
                            mode,
                            seats,
                            mine,
                        };
                        players_column(row, tr, &table);
                    });
                    if watching {
                        card.spawn((
                            Text::new(tr.lineup_watching),
                            TextFont {
                                font_size: FontSize::Px(KIND_FONT),
                                ..default()
                            },
                            TextColor(palette::PARCHMENT.with_alpha(0.7)),
                            Node {
                                align_self: AlignSelf::Center,
                                margin: UiRect::top(Val::Px(10.0)),
                                ..default()
                            },
                        ));
                    }
                });
        });
}

/// The map's column: its picture, its name, and what it is.
fn map_column(row: &mut ChildSpawnerCommands, map: &str, sub: Option<&str>) {
    row.spawn(Node {
        width: Val::Px(PICTURE.x),
        flex_shrink: 0.0,
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(4.0),
        ..default()
    })
    .with_children(|column| {
        column.spawn((
            LineupPicture,
            ImageNode::default().with_color(crate::app::materials::SAND),
            Node {
                width: Val::Px(PICTURE.x),
                height: Val::Px(PICTURE.y),
                border_radius: BorderRadius::all(Val::Px(8.0)),
                ..default()
            },
        ));
        column
            .spawn(Node {
                width: Val::Px(PICTURE.x),
                overflow: Overflow::clip_x(),
                margin: UiRect::top(Val::Px(4.0)),
                ..default()
            })
            .with_children(|cell| {
                cell.spawn((
                    Text::new(map),
                    menu_ui::display_font(MAP_FONT),
                    TextLayout::no_wrap(),
                    TextColor(palette::PARCHMENT),
                ));
            });
        if let Some(sub) = sub {
            column.spawn((
                Text::new(sub),
                TextFont {
                    font_size: FontSize::Px(SUB_FONT),
                    ..default()
                },
                TextColor(palette::PARCHMENT.with_alpha(0.6)),
            ));
        }
    });
}

/// The table the line-up lists: who sits where, what plays each seat,
/// how it is scored, and this peer's own seat, if it has one.
struct Table<'a> {
    names: &'a SeatNames,
    controllers: &'a Controllers,
    mode: crate::app::teams::TeamMode,
    seats: u8,
    mine: Option<u8>,
}

/// A seat's name on the line-up, this peer's own marked as it is on the
/// scoreboard. Cut to its cell ([`NAME_W`]) where it runs past, the mark
/// kept whole and the name cut: every name has the same room, and twelve
/// kana with the mark run past it in every language.
pub(crate) fn seat_name(tr: &Tr, names: &SeatNames, seat: u8, mine: Option<u8>) -> String {
    let tag = match mine == Some(seat) {
        true => tr.tag_you,
        false => "",
    };
    let room = NAME_W - line_px(tag, NAME_FONT);
    format!("{}{tag}", cut_to(&names.label(tr, seat), room, NAME_FONT))
}

/// The players' column: each seat, in its colour, with what plays it,
/// team by team under a team mode, which its heading names.
fn players_column(row: &mut ChildSpawnerCommands, tr: &Tr, table: &Table) {
    let Table {
        names,
        controllers,
        mode,
        seats,
        mine,
    } = *table;
    row.spawn(Node {
        flex_grow: 1.0,
        flex_direction: FlexDirection::Column,
        ..default()
    })
    .with_children(|column| {
        if mode != crate::app::teams::TeamMode::Solo {
            column.spawn((
                Text::new(tr.team_mode_names[mode.index()]),
                menu_ui::display_font(SUB_FONT),
                TextColor(palette::GOLD.with_alpha(0.75)),
                Node {
                    margin: UiRect::bottom(Val::Px(4.0)),
                    ..default()
                },
            ));
        }
        for (seat, heads) in seat_order(mode, seats) {
            if let Some(team) = heads {
                let (label, ink) = team_heading(tr, names, mode, team, seats);
                column
                    .spawn(Node {
                        overflow: Overflow::clip_x(),
                        margin: UiRect::top(Val::Px(if team > 0 { TEAM_GAP } else { 0.0 })),
                        ..default()
                    })
                    .with_children(|cell| {
                        // In the rows' face, as the names under it are.
                        cell.spawn((
                            Text::new(label),
                            TextFont {
                                font_size: FontSize::Px(SUB_FONT),
                                ..default()
                            },
                            TextLayout::no_wrap(),
                            TextColor(ink),
                        ));
                    });
            }
            let kind = seat_kind(tr, controllers.0[usize::from(seat)]);
            column
                .spawn(Node {
                    height: Val::Px(ROW_H),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|line| {
                    let disc = palette::leader_chip(seat);
                    line.spawn((
                        Node {
                            width: Val::Px(DISC),
                            height: Val::Px(DISC),
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(Val::Px(2.0)),
                            border_radius: BorderRadius::all(Val::Px(DISC / 2.0)),
                            ..default()
                        },
                        BackgroundColor(disc),
                        BorderColor::all(palette::seat_paint(seat)),
                    ))
                    .with_children(|chip| {
                        chip.spawn((
                            Text::new((seat + 1).to_string()),
                            menu_ui::display_font(DISC * 0.6),
                            TextColor(palette::ink_on(disc)),
                        ));
                    });
                    line.spawn(Node {
                        width: Val::Px(NAME_W),
                        flex_shrink: 0.0,
                        overflow: Overflow::clip_x(),
                        ..default()
                    })
                    .with_children(|cell| {
                        cell.spawn((
                            Text::new(seat_name(tr, names, seat, mine)),
                            TextFont {
                                font_size: FontSize::Px(NAME_FONT),
                                ..default()
                            },
                            TextLayout::no_wrap(),
                            TextColor(palette::seat_text(seat)),
                        ));
                    });
                    line.spawn((
                        Text::new(kind),
                        TextFont {
                            font_size: FontSize::Px(KIND_FONT),
                            ..default()
                        },
                        TextLayout::no_wrap(),
                        TextColor(palette::PARCHMENT.with_alpha(0.6)),
                        Node {
                            width: Val::Px(KIND_W),
                            overflow: Overflow::clip_x(),
                            ..default()
                        },
                    ));
                });
        }
    });
}

/// The line-up's picture of the map: drawn from the round's own board the
/// first frame the line-up is up, by the map cards' renderer, put on the
/// card once it is drawn, and let go of once the line-up is down.
pub fn lineup_picture(world: &mut World) {
    let up = world
        .query_filtered::<(), With<LineupUi>>()
        .iter(world)
        .next()
        .is_some();
    let held = world.resource::<MapPreviews>().image(Pic::Played);
    match (up, held) {
        (true, None) => {
            let board = world.resource::<crate::app::Sim>().0.clone();
            crate::app::match_setup::draw_board_picture(world, Pic::Played, &board);
        }
        (true, Some(drawn)) => {
            let mut pictures = world.query_filtered::<&mut ImageNode, With<LineupPicture>>();
            for mut image in pictures.iter_mut(world) {
                if image.image != drawn {
                    image.image = drawn.clone();
                    image.color = Color::WHITE;
                }
            }
        }
        (false, Some(_)) => crate::app::match_setup::release_previews(world),
        (false, None) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::metrics::{display_px, text_px};
    use crate::app::teams::TeamMode;

    /// The line-up goes up for a session that opens on it, names every
    /// seat and the map the terms name, takes the session's word so the
    /// next round opens on the count alone, and comes down once its time
    /// is up.
    /// What every line-up app needs besides the table: the couch's config
    /// and shelf, and the daily.
    fn lineup_app(settings: GameSettings) -> App {
        let mut app = App::new();
        app.insert_resource(settings);
        app.init_resource::<crate::app::match_setup::MatchConfig>();
        app.init_resource::<crate::app::match_setup::CustomBeaches>();
        app.init_resource::<crate::app::Daily>();
        app.init_resource::<Online>();
        app
    }

    /// Every text on screen.
    fn said(app: &mut App) -> Vec<String> {
        let world = app.world_mut();
        world
            .query::<&Text>()
            .iter(world)
            .map(|text| text.0.clone())
            .collect()
    }

    /// A couch round in pairs opens on the line-up too: the map in force,
    /// every seat, team by team under its heading, and no seat marked as
    /// this peer's, the whole table being here.
    #[test]
    fn a_couch_round_lines_up_its_teams() {
        let settings = GameSettings {
            team_mode: TeamMode::Pairs,
            ..GameSettings::default()
        };
        let mut app = lineup_app(settings);
        app.world_mut()
            .resource_mut::<crate::app::match_setup::MatchConfig>()
            .map = MapChoice::LogJam;
        app.init_resource::<crate::app::countdown::Countdown>();
        app.insert_resource(Seats(4));
        let mut names = SeatNames::default();
        for (seat, name) in ["Anna", "Bo", "Cleo", "Dan"].iter().enumerate() {
            names.0[seat] = (*name).to_string();
        }
        app.insert_resource(names);
        let mut controllers = Controllers::default();
        controllers.0[3] = SeatController::Ai(crate::sim::BotLevel::Easy);
        app.insert_resource(controllers);
        app.world_mut()
            .resource_mut::<crate::app::countdown::Countdown>()
            .set_lineup_for_tests(LINEUP_SECS);
        app.add_systems(Update, show_lineup);
        app.update();
        let said = said(&mut app);
        let tr = &crate::app::i18n::EN;
        for want in [
            "Anna+Bo",
            "Cleo+Dan",
            "Anna",
            "Dan",
            tr.team_mode_names[TeamMode::Pairs.index()],
            &crate::app::match_setup::map_name(tr, MapChoice::LogJam),
            &fill(tr.setup_by_ai, &[("l", tr.bot_levels[0])]),
        ] {
            assert!(said.iter().any(|line| line == want), "{want:?} in {said:?}");
        }
        assert!(
            !said.iter().any(|line| line.contains(tr.tag_you)),
            "{said:?}"
        );
        assert!(!said.iter().any(|line| line == tr.lineup_watching));
    }

    #[test]
    fn the_lineup_names_the_table_and_the_map() {
        use crate::transport::{MatchTerms, UdpTransport};
        let mut app = lineup_app(GameSettings::default());
        app.init_resource::<crate::app::countdown::Countdown>();
        app.insert_resource(Seats(3));
        let mut names = SeatNames::default();
        names.0[0] = "Anna".into();
        names.0[1] = "Bo".into();
        app.insert_resource(names);
        let mut controllers = Controllers::default();
        controllers.0[2] = SeatController::Ai(crate::sim::BotLevel::Hard);
        app.insert_resource(controllers);
        let session = crate::app::net::OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            crate::sim::Lockstep::new(0, vec![0, 1], crate::sim::DEFAULT_DELAY),
            3,
            MatchTerms {
                map: MapChoice::TheMoat.index() as u8,
                ..MatchTerms::default()
            },
        );
        app.insert_resource(Online(Some(session)));
        app.world_mut()
            .resource_mut::<crate::app::countdown::Countdown>()
            .set_lineup_for_tests(LINEUP_SECS);
        app.add_systems(Update, show_lineup);
        app.update();
        let said: Vec<String> = {
            let world = app.world_mut();
            world
                .query::<&Text>()
                .iter(world)
                .map(|text| text.0.clone())
                .collect()
        };
        let tr = &crate::app::i18n::EN;
        for want in [
            format!("Anna{}", tr.tag_you),
            "Bo".to_string(),
            crate::app::seat_label(tr, 2),
            fill(tr.setup_by_ai, &[("l", tr.bot_levels[2])]),
            crate::app::match_setup::map_name(tr, MapChoice::TheMoat),
            tr.lineup_title.to_string(),
        ] {
            assert!(said.contains(&want), "{want:?} in {said:?}");
        }
        app.world_mut()
            .resource_mut::<crate::app::countdown::Countdown>()
            .set_lineup_for_tests(0.0);
        app.update();
        let world = app.world_mut();
        assert_eq!(world.query::<&LineupUi>().iter(world).count(), 0, "down");
    }

    /// The widest name there is: twelve kana, each a full em, where a
    /// Latin twelve is three fifths of that.
    fn kana() -> String {
        "あいうえおかきくけこさし".to_string()
    }

    /// A name of twelve kana fits its cell on the line-up in every
    /// language, with this peer's own mark after it, measured in the
    /// shipped faces: cut short with "…", the mark whole. A name that fits
    /// is whole.
    #[test]
    fn a_name_of_twelve_kana_fits_its_cell() {
        assert_eq!(kana().chars().count(), crate::app::settings::NAME_MAX);
        let mut names = SeatNames::default();
        names.0[0] = kana();
        names.0[1] = "W".repeat(crate::app::settings::NAME_MAX);
        names.0[2] = "Anna".into();
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            for seat in [0, 1] {
                for mine in [None, Some(seat)] {
                    let said = seat_name(tr, &names, seat, mine);
                    let wide = text_px(&said, NAME_FONT);
                    assert!(wide <= NAME_W, "{lang:?}: {said} is {wide:.0}px");
                    if mine.is_some() {
                        assert!(said.ends_with(tr.tag_you), "{lang:?}: {said}");
                    }
                }
            }
            assert_eq!(
                seat_name(tr, &names, 2, Some(2)),
                format!("Anna{}", tr.tag_you),
                "{lang:?}: whole"
            );
        }
        let cut = seat_name(&crate::app::i18n::JA, &names, 0, Some(0));
        assert!(cut.starts_with("あいうえ") && cut.contains('…'), "{cut}");
    }

    /// A team of three twelve-kana names, the widest a team's names come,
    /// is headed across the players' column in every language, each name
    /// cut alike, where joined whole they ran past it. Names that fit are
    /// left whole.
    #[test]
    fn a_team_of_three_kana_names_is_headed_fairly() {
        let mode = TeamMode::Trios;
        let team = mode.seats_of(0, 6);
        let mut names = SeatNames::default();
        for name in &mut names.0 {
            *name = kana();
        }
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            let (label, _) = team_heading(tr, &names, mode, 0, 6);
            let wide = text_px(&label, SUB_FONT);
            assert!(wide <= COLUMN_W, "{lang:?}: {label} is {wide:.0}px");
            let parts: Vec<&str> = label.split('+').collect();
            assert_eq!(parts.len(), 3, "{label}");
            assert!(parts.iter().all(|part| *part == parts[0]), "{label}");
            assert!(parts[0].starts_with("あいうえ"), "{label}");
        }
        // A short name and a long Latin one beside a kana one fit whole.
        names.0[usize::from(team[1])] = "Bo".into();
        names.0[usize::from(team[2])] = "W".repeat(crate::app::settings::NAME_MAX);
        let tr = &crate::app::i18n::EN;
        let (label, _) = team_heading(tr, &names, mode, 0, 6);
        let whole = crate::app::teams::team_label(mode, 0, 6, |seat| names.label(tr, seat));
        assert_eq!(label, whole);
    }

    /// Seat order alone, and team by team under a team mode, every seat
    /// once.
    #[test]
    fn the_seats_are_listed_team_by_team() {
        let solo: Vec<u8> = seat_order(TeamMode::Solo, 4).iter().map(|s| s.0).collect();
        assert_eq!(solo, [0, 1, 2, 3]);
        assert!(seat_order(TeamMode::Solo, 4).iter().all(|s| s.1.is_none()));
        let pairs = seat_order(TeamMode::Pairs, 4);
        let mut every: Vec<u8> = pairs.iter().map(|s| s.0).collect();
        let heads: Vec<u8> = pairs.iter().filter_map(|s| s.1).collect();
        assert_eq!(heads, [0, 1], "a heading a team");
        let mut names = SeatNames::default();
        names.0[0] = "Anna".into();
        names.0[1] = "Bo".into();
        let tr = &crate::app::i18n::EN;
        let (label, _) = team_heading(tr, &names, TeamMode::Pairs, 0, 4);
        assert_eq!(label, "Anna+Bo", "named as the header names it");
        every.sort_unstable();
        assert_eq!(every, [0, 1, 2, 3]);
        for (seat, _) in &pairs[..2] {
            assert_eq!(TeamMode::Pairs.team_of(*seat), 0);
        }
    }

    /// Every line of the line-up fits its place in every language, at the
    /// longest a name, a map name and what plays a seat come: six rows and
    /// a watcher's line inside a 1280x720 window.
    #[test]
    fn every_line_fits_the_lineup_in_every_language() {
        let mut names = SeatNames::default();
        names.0[0] = "W".repeat(crate::app::settings::NAME_MAX);
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            let name = seat_name(tr, &names, 0, Some(0));
            let wide = text_px(&name, NAME_FONT);
            assert!(wide <= NAME_W, "{lang:?}: {name} is {wide:.0}px");
            assert!(
                display_px(tr.lineup_title, TITLE_FONT) <= CARD_W,
                "{lang:?}"
            );
            assert!(text_px(tr.lineup_watching, KIND_FONT) <= CARD_W, "{lang:?}");
            assert!(
                text_px(tr.setup_surprise_sub, SUB_FONT) <= PICTURE.x,
                "{lang:?}"
            );
            for kind in [
                SeatController::Ai(crate::sim::BotLevel::Easy),
                SeatController::Ai(crate::sim::BotLevel::Normal),
                SeatController::Ai(crate::sim::BotLevel::Hard),
                SeatController::Bot,
            ] {
                let said = seat_kind(tr, kind);
                let wide = text_px(&said, KIND_FONT);
                assert!(wide <= KIND_W, "{lang:?}: {said} is {wide:.0}px");
            }
            for map in MapChoice::ALL {
                let said = crate::app::match_setup::map_name(tr, map);
                let wide = display_px(&said, MAP_FONT);
                assert!(wide <= PICTURE.x, "{lang:?}: {said} is {wide:.0}px");
            }
            for mode in [TeamMode::Pairs, TeamMode::Trios] {
                let said = tr.team_mode_names[mode.index()];
                assert!(display_px(said, SUB_FONT) <= NAME_W, "{lang:?}: {said}");
            }
        }
        // A trio of the longest names, named as a team, across the column.
        let mut long = SeatNames::default();
        for name in &mut long.0 {
            *name = "W".repeat(crate::app::settings::NAME_MAX);
        }
        let tr = &crate::app::i18n::EN;
        let (label, _) = team_heading(tr, &long, TeamMode::Trios, 0, 6);
        let column = COLUMN_W;
        let wide = text_px(&label, SUB_FONT);
        assert!(wide <= column, "{label} is {wide:.0}px of {column:.0}");
        // The row: a disc, the name and what plays it, beside the picture.
        let row = DISC + NAME_W + KIND_W + 2.0 * 10.0;
        assert!(PICTURE.x + 24.0 + row <= CARD_W - 2.0 * menu_ui::CARD_PAD);
        // Six rows, a team break or two and the heading, beside the picture
        // and its two lines, under the title, with the watcher's line: the
        // window between the bars at 720 holds it.
        // Under Pairs at six: the mode, and three teams' headings, two of
        // them with the air above.
        let heading = SUB_FONT * menu_ui::LINE_HEIGHT;
        let players = 6.0 * ROW_H + 4.0 * heading + 4.0 + 2.0 * TEAM_GAP;
        let map = PICTURE.y + 4.0 + (MAP_FONT + SUB_FONT) * menu_ui::LINE_HEIGHT;
        let card = 2.0 * menu_ui::CARD_PAD
            + TITLE_FONT * menu_ui::LINE_HEIGHT
            + 12.0
            + players.max(map)
            + 10.0
            + KIND_FONT * menu_ui::LINE_HEIGHT;
        assert!(card <= 720.0 - 2.0 * 60.0, "{card:.0}px");
    }
}
