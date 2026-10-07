//! The versus sidebars: on the left, score chips sorted by rank (the
//! leader's chip is biggest and wears the crown); on the right, the big
//! tide clock over a running log of the round's notable events.

mod clock;
mod feed;

pub use clock::update_side_clock;
pub use feed::{EventLog, collect_chat, collect_log, update_log};

use crate::app::net::Online;
use crate::app::settings::GameSettings;
use crate::app::teams::TeamMode;
use crate::app::{Controllers, Playback, SeatController, SeatNames, Seats, Sim};
use crate::app::{menu_ui, palette};
use crate::sim::MAX_PLAYERS;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// Width of each sidebar in px; `fit_camera` reserves this much on both
/// sides.
pub const SIDEBAR_W: f32 = 250.0;

/// The sidebar root; despawning it takes everything with it.
#[derive(Component)]
pub struct SidePanelRoot;

/// A per-seat score chip inside the sidebar. Its vertical slot follows the
/// seat's current rank, so overtakes visibly reshuffle the column.
#[derive(Component)]
pub struct SidePanel(pub u8);

/// The score number inside a chip; `bump` animates on change.
#[derive(Component)]
pub struct SideScore {
    pub seat: u8,
    bump: f32,
}

/// One arrow slot on a chip, oldest first: an outline, filled for an
/// arrow standing and drained by how much of its life is gone. One per
/// arrow the rules allow, so the row is the cap, read at a glance.
#[derive(Component)]
pub struct ArrowDot {
    seat: u8,
    index: u8,
}

/// The fill inside an [`ArrowDot`]: its height is the arrow's life left.
#[derive(Component)]
pub struct ArrowFill {
    seat: u8,
    index: u8,
}

/// The most arrow slots a chip draws. The dial stops at six; a level file
/// can ask for more, and past this the row would run into the score.
const MAX_DOTS: u8 = 10;

/// The crown icon shown on the current leader's chip.
#[derive(Component)]
pub struct LeaderCrown(pub u8);

/// The round rank medal on a chip's left edge (colour by rank).
#[derive(Component)]
pub struct RankMedal(pub u8);

/// The digit inside the rank medal.
#[derive(Component)]
pub struct RankDigit(pub u8);

/// Chip geometry by rank: the leader gets the tall card and the huge
/// number; the rest shrink with their standing, with a gap between cards.
/// Six of them still has to fit the column above the fold, so the tail is
/// tighter than the head.
///
/// Each chip holds two lines, the name over the arrows, which sets the
/// smallest: 48 is the two of them and the padding, with nothing spare.
///
/// There was a third, three castle-tier pips, gone because they said
/// again what the number beside them says and the castle on the beach
/// shows, and nobody could tell what they were for.
const CHIP_TOPS: [f32; MAX_PLAYERS] = [10.0, 106.0, 172.0, 234.0, 294.0, 352.0];
const CHIP_HEIGHTS: [f32; MAX_PLAYERS] = [88.0, 58.0, 54.0, 52.0, 50.0, 48.0];
const SCORE_PX: [f32; MAX_PLAYERS] = [46.0, 28.0, 24.0, 21.0, 19.0, 18.0];

/// Rank medal colours: gold, silver, bronze, then driftwood for the rest.
const MEDALS: [Color; MAX_PLAYERS] = [
    palette::MEDAL_GOLD,
    palette::MEDAL_SILVER,
    palette::MEDAL_BRONZE,
    palette::MEDAL_DRIFTWOOD,
    palette::MEDAL_DRIFTWOOD,
    palette::MEDAL_DRIFTWOOD,
];
/// What a rank medal's digit reads: the place, one-based. A table, so the
/// chips repaint without formatting anything.
const PLACES: [&str; MAX_PLAYERS] = ["1", "2", "3", "4", "5", "6"];
const LEADER_BORDER: Color = palette::GOLD;
const LOG_TOP: f32 = 88.0;

/// Seats ranked by score (descending, seat number breaking ties), and each
/// seat's rank.
fn ranks(scores: &[u32; MAX_PLAYERS], seats: u8) -> [usize; MAX_PLAYERS] {
    let count = usize::from(seats.max(2));
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by_key(|&seat| (std::cmp::Reverse(scores[seat]), seat));
    let mut rank = [0usize; MAX_PLAYERS];
    for (place, &seat) in order.iter().enumerate() {
        rank[seat] = place;
    }
    rank
}

/// One sidebar's frame: full height, pinned to one edge, under the header
/// and above the prompt line.
fn sidebar(left: bool) -> (SidePanelRoot, Node) {
    (
        SidePanelRoot,
        Node {
            position_type: PositionType::Absolute,
            left: if left { Val::Px(0.0) } else { Val::Auto },
            right: if left { Val::Auto } else { Val::Px(0.0) },
            top: Val::Px(menu_ui::UNDER_HEADER),
            bottom: Val::Px(44.0),
            width: Val::Px(SIDEBAR_W),
            ..default()
        },
    )
}

/// The card language both sidebars share: the menus' gold hairline and
/// drop shadow over the in-round panel's opaque fill, so the chrome of a
/// round is the same set of cards as the screens around it, standing on
/// the same beach. The fill stays opaque: the scores are read at a glance.
fn card(top: f32, height: Option<f32>) -> (Node, BorderColor, BackgroundColor, BoxShadow) {
    (
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(top),
            bottom: if height.is_some() {
                Val::Auto
            } else {
                Val::Px(0.0)
            },
            left: Val::Px(10.0),
            right: Val::Px(10.0),
            height: height.map_or(Val::Auto, Val::Px),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(12.0)),
            ..default()
        },
        BorderColor::all(palette::CARD_EDGE),
        BackgroundColor(palette::CARD_BG),
        menu_ui::card_shadow(),
    )
}

/// One seat's score chip: rank medal, name over arrow dots, the big number,
/// and a crown that only the leader shows.
fn spawn_score_chip(
    root: &mut ChildSpawnerCommands,
    art: &crate::app::art::Art,
    seat: u8,
    (name, tag): (String, &'static str),
    arrows: u8,
    bot: bool,
) {
    root.spawn((
        SidePanel(seat),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(CHIP_TOPS[seat as usize]),
            left: Val::Px(10.0),
            right: Val::Px(10.0),
            height: Val::Px(CHIP_HEIGHTS[seat as usize]),
            align_items: AlignItems::Center,
            column_gap: Val::Px(10.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(12.0)),
            ..default()
        },
        BorderColor::all(palette::player_color(seat).lighter(0.08)),
        BackgroundColor(palette::player_color(seat).darker(0.22)),
    ))
    .with_children(|chip| {
        // Rank medal on the left edge.
        chip.spawn((
            RankMedal(seat),
            Node {
                width: Val::Px(26.0),
                height: Val::Px(26.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(13.0)),
                ..default()
            },
            BackgroundColor(MEDALS[seat as usize]),
        ))
        .with_children(|medal| {
            medal.spawn((
                RankDigit(seat),
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(menu_ui::type_scale::BODY),
                    ..default()
                },
                TextColor(palette::MEDAL_DIGIT),
            ));
        });
        // Name, and the arrows under it.
        chip.spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            // Free to be narrower than the name, which clips: wrapped, a
            // long name and its tag took two lines and put the arrows on
            // the chip's edge.
            min_width: Val::Px(0.0),
            row_gap: Val::Px(3.0),
            ..default()
        })
        .with_children(|mid| {
            mid.spawn(Node {
                column_gap: Val::Px(5.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|line| {
                let font = TextFont {
                    font_size: FontSize::Px(menu_ui::type_scale::BODY),
                    ..default()
                };
                line.spawn(Node {
                    min_width: Val::Px(0.0),
                    overflow: Overflow::clip_x(),
                    ..default()
                })
                .with_children(|clip| {
                    // The robot rides in the line with the name, so a name
                    // too long for the chip clips at its end and the robot
                    // at its start stays where it is read first.
                    clip.spawn((
                        Text::new(""),
                        font.clone(),
                        TextLayout::no_wrap(),
                        TextColor(palette::CHIP_NAME),
                    ))
                    .with_children(|text| {
                        if bot {
                            text.spawn(robot_inline(art, seat, 18.0));
                        }
                        text.spawn((
                            TextSpan::new(beside_the_robot(bot, &name)),
                            font.clone(),
                            TextColor(palette::CHIP_NAME),
                        ));
                    });
                });
                if !tag.is_empty() {
                    line.spawn((
                        Text::new(tag.trim_start()),
                        font,
                        TextLayout::no_wrap(),
                        TextColor(palette::CHIP_NAME),
                        Node {
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                }
            });
            mid.spawn(Node {
                column_gap: Val::Px(3.0),
                ..default()
            })
            .with_children(|dots| {
                for index in 0..arrows.min(MAX_DOTS) {
                    dots.spawn((
                        ArrowDot { seat, index },
                        Node {
                            width: Val::Px(6.0),
                            height: Val::Px(9.0),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(2.0)),
                            flex_direction: FlexDirection::Column,
                            justify_content: JustifyContent::FlexEnd,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BorderColor::all(palette::PIP_OFF),
                        BackgroundColor(Color::NONE),
                    ))
                    .with_children(|dot| {
                        dot.spawn((
                            ArrowFill { seat, index },
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Percent(0.0),
                                ..default()
                            },
                            BackgroundColor(palette::PIP_ON),
                        ));
                    });
                }
            });
        });
        // The big number, right-aligned.
        chip.spawn((
            SideScore { seat, bump: 0.0 },
            Text::new("0"),
            menu_ui::display_font(SCORE_PX[seat as usize]),
            TextColor(palette::HUD_INK),
        ));
        // The crown perches on the card's top-right corner.
        chip.spawn((
            LeaderCrown(seat),
            Visibility::Hidden,
            ImageNode::new(art.crown.clone()),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(-13.0),
                right: Val::Px(6.0),
                width: Val::Px(24.0),
                height: Val::Px(24.0),
                ..default()
            },
        ));
    });
}

/// The bot tag before a name: the robot's head in the seat's colour, `px`
/// square, drawn in the line of text as one of its spans. The game sets it
/// from what holds the seat, never from the name, so a bot that calls
/// itself after a friend still wears it.
///
/// In the line rather than a node beside it (Bevy 0.20's `InlineImage`):
/// it sits on the text's own line, clips with it, and needs no row of
/// nodes around the text to hold it.
pub fn robot_inline(art: &crate::app::art::Art, seat: u8, px: f32) -> InlineImage {
    InlineImage {
        image: art.robot.clone(),
        color: palette::player_color(seat).lighter(0.3),
        width: Some(px),
        height: Some(px),
        ..default()
    }
}

/// The text that follows the robot, or stands alone without one: a space
/// apart from it when there is one.
pub fn beside_the_robot(bot: bool, text: &str) -> String {
    match bot {
        true => format!(" {text}"),
        false => text.to_string(),
    }
}

/// The right sidebar: the big tide clock over the event feed.
fn spawn_clock_and_feed(commands: &mut Commands) {
    commands.spawn(sidebar(false)).with_children(|root| {
        clock::spawn_clock(root);
        feed::spawn_feed(root);
    });
}

/// Spawn the sidebars after `load_versus` has set the seat count.
pub fn spawn_side_panels(
    mut commands: Commands,
    seating: Seating,
    kinds: Res<crate::app::SeatKinds>,
    settings: Res<GameSettings>,
    art: Res<crate::app::art::Art>,
    sim: Res<Sim>,
    mut log: ResMut<EventLog>,
) {
    log.0.clear();
    let tr = settings.tr();
    let labels: Vec<(String, &'static str)> = (0..seating.seats.0.max(2))
        .map(|seat| seating.name_and_tag(tr, seat))
        .collect();
    // The cap is the board's, set before the round and never during it.
    let arrows = sim.0.signpost_rule().0;
    commands.spawn(sidebar(true)).with_children(|root| {
        for (seat, label) in labels.into_iter().enumerate() {
            let seat = seat as u8;
            spawn_score_chip(root, &art, seat, label, arrows, kinds.bot(seat));
        }
    });
    spawn_clock_and_feed(&mut commands);
}

/// Index of the strictly-largest nonzero entry, if it is unique. The
/// shared core of "who is winning" for rounds, series, and panels.
pub fn unique_max<T: Copy + Ord + Default>(values: &[T]) -> Option<usize> {
    let best = values.iter().max().copied()?;
    if best == T::default() || values.iter().filter(|&&v| v == best).count() != 1 {
        return None;
    }
    values.iter().position(|&v| v == best)
}

/// Which seats currently lead: the unique top scorer (or, in 2v2, both
/// members of the strictly leading team). Nobody leads at zero or in a tie.
pub fn leading_seats(
    scores: &[u32; MAX_PLAYERS],
    seats: u8,
    mode: TeamMode,
) -> [bool; MAX_PLAYERS] {
    let mut leaders = [false; MAX_PLAYERS];
    if mode == TeamMode::Solo {
        if let Some(winner) = unique_max(&scores[..usize::from(seats.max(2))]) {
            leaders[winner] = true;
        }
        return leaders;
    }
    let totals = crate::app::teams::team_scores(scores, seats, mode);
    if let Some(team) = unique_max(&totals) {
        for seat in mode.seats_of(team as u8, seats) {
            leaders[usize::from(seat)] = true;
        }
    }
    leaders
}

/// Who sits where: the table, which chairs the AI holds and what everyone
/// is called, and whether this screen is a peer's, a recording's or the
/// keyboard's. Read together wherever a seat is labelled or a mark is
/// shown only to the people it is for.
#[derive(SystemParam)]
pub struct Seating<'w> {
    pub seats: Res<'w, Seats>,
    pub controllers: Res<'w, Controllers>,
    pub names: Res<'w, SeatNames>,
    pub online: Res<'w, Online>,
    pub playback: Res<'w, Playback>,
}

impl Seating<'_> {
    /// The seat at this keyboard: see [`local_seat`].
    pub fn local(&self) -> Option<u8> {
        local_seat(&self.online, self.playback.0.is_some())
    }

    /// Whether `seat` is played at this screen: see [`played_here`].
    pub fn played_here(&self, seat: u8) -> bool {
        played_here(
            self.local(),
            self.online.0.is_some(),
            &self.controllers,
            seat,
        )
    }

    /// A seat's name and its "(you)" or "(AI)" tag, apart: a chip clips a
    /// long name and never its tag.
    pub fn name_and_tag(&self, tr: &crate::app::i18n::Tr, seat: u8) -> (String, &'static str) {
        let tag = seat_tag(tr, &self.controllers, self.local(), seat);
        (self.names.label(tr, seat), tag)
    }
}

/// The seat the person at this keyboard is playing, and `None` when
/// nobody is: a spectator at somebody else's beach, or a recording.
///
/// Offline the answer is seat 0, because the person at the keyboard is
/// always P1 there. [`seat_tag`] used to make that guess itself, from a
/// local seat of `None`, and a spectator has one of those too: every
/// watcher on a beach was shown the host's chair marked as its own, on
/// the panels and on the results card both.
pub fn local_seat(online: &Online, playback_active: bool) -> Option<u8> {
    match &online.0 {
        Some(session) => session.session.seat(),
        // A recording is watched, not played, so nobody at this keyboard
        // holds a chair in it either.
        None if playback_active => None,
        None => Some(0),
    }
}

/// Whether `seat` is played at this screen: offline every seat the AI
/// does not hold, since everyone sharing the keyboard and the pads is here;
/// online only the chair this peer was dealt; and nobody's while watching,
/// a recording or somebody else's beach. `local` is [`local_seat`]'s.
///
/// What the next-to-go mark is shown for. A table of AI is always at its
/// cap, and marking their posts set half the beach wobbling for nobody.
pub fn played_here(local: Option<u8>, online: bool, controllers: &Controllers, seat: u8) -> bool {
    match local {
        None => false,
        Some(mine) if online => mine == seat,
        Some(_) => controllers.0.get(usize::from(seat)) == Some(&SeatController::Local),
    }
}

/// How strongly the next-to-go marks show, `secs` into the round: the
/// pulse the arrow on the beach and its dot on the chip share, so the eye
/// ties the two together. Steady at full under reduced motion.
pub fn next_to_go_pulse(secs: f32, reduced_motion: bool) -> f32 {
    if reduced_motion {
        return 1.0;
    }
    0.55 + 0.45 * (secs * std::f32::consts::TAU * 1.2).sin().abs()
}

/// The "(you)" / "(AI)" tag for a seat, shared by the panels and the
/// results card.
pub fn seat_tag(
    tr: &crate::app::i18n::Tr,
    controllers: &Controllers,
    local: Option<u8>,
    seat: u8,
) -> &'static str {
    if controllers.ai(usize::from(seat)).is_some() {
        tr.tag_ai
    } else if local == Some(seat) {
        tr.tag_you
    } else {
        ""
    }
}

/// A chip's card: where it sits in the column and how it is coloured.
type ChipFrame = (
    &'static SidePanel,
    &'static mut Node,
    &'static mut BackgroundColor,
    &'static mut BorderColor,
);

/// A chip's score number and how big it is drawn.
type ChipScore = (
    &'static mut SideScore,
    &'static mut Text,
    &'static mut TextFont,
    &'static mut UiTransform,
);

/// Every part of the score chips that follows the standings.
#[derive(SystemParam)]
pub struct Chips<'w, 's> {
    frames: Query<'w, 's, ChipFrame>,
    scores: Query<'w, 's, ChipScore>,
    crowns: Query<'w, 's, (&'static LeaderCrown, &'static mut Visibility)>,
    medals: Query<'w, 's, (&'static RankMedal, &'static mut BackgroundColor), Without<SidePanel>>,
    digits: Query<'w, 's, (&'static RankDigit, &'static mut Text), Without<SideScore>>,
}

/// Keep the chips sorted and sized by rank, the numbers current, and the
/// crown on the leader (all writes guarded).
pub fn update_side_panels(
    online: Res<Online>,
    sim: Res<Sim>,
    seats: Res<Seats>,
    settings: Res<GameSettings>,
    time: Res<Time>,
    mut chips: Chips,
    mut value: Local<String>,
) {
    let Chips {
        frames: panels,
        scores,
        crowns,
        medals,
        digits,
    } = &mut chips;
    let board_scores = sim.0.scores();
    let mode = crate::app::teams::in_play(&settings, &online, seats.0);
    let leaders = leading_seats(board_scores, seats.0, mode);
    let rank = ranks(board_scores, seats.0);
    for (panel, mut node, mut bg, mut border) in panels {
        let seat = panel.0 as usize;
        let r = rank[seat];
        let (top, height) = (Val::Px(CHIP_TOPS[r]), Val::Px(CHIP_HEIGHTS[r]));
        if node.top != top {
            node.top = top;
        }
        if node.height != height {
            node.height = height;
        }
        let color = if leaders[seat] {
            palette::player_color(panel.0)
        } else {
            palette::player_color(panel.0).darker(0.22)
        };
        menu_ui::set_bg(&mut bg, color);
        let edge = if leaders[seat] {
            LEADER_BORDER
        } else {
            palette::player_color(panel.0).lighter(0.08)
        };
        let edge = BorderColor::all(edge);
        if *border != edge {
            *border = edge;
        }
    }
    for (medal, mut bg) in medals {
        menu_ui::set_bg(&mut bg, MEDALS[rank[medal.0 as usize]]);
    }
    for (digit, mut text) in digits {
        menu_ui::set_text(&mut text, PLACES[rank[digit.0 as usize]]);
    }
    for (mut chip, mut text, mut font, mut transform) in scores {
        use std::fmt::Write;
        value.clear();
        let _ = write!(&mut *value, "{}", board_scores[chip.seat as usize]);
        if text.0 != *value {
            text.0.clear();
            text.0.push_str(&value);
            // Pop on change, unless the player asked for less motion, in
            // which case the number simply changes.
            chip.bump = if settings.reduced_motion { 0.0 } else { 1.0 };
        }
        chip.bump = (chip.bump - time.delta_secs() * 4.0).max(0.0);
        // The size follows the seat's rank, and only that: six fixed values,
        // each rasterized into the glyph atlas once and reused forever.
        let base = FontSize::Px(SCORE_PX[rank[chip.seat as usize]]);
        if font.font_size != base {
            font.font_size = base;
        }
        // The pop is scale, not size: Bevy keys its glyph atlas by font
        // size, so animating the size allocates a fresh atlas and
        // re-rasterizes the digits on every frame of the bump, which came
        // to 4.4% of the game's whole CPU.
        let pop = Vec2::splat(1.0 + 0.22 * chip.bump);
        if transform.scale != pop {
            transform.scale = pop;
        }
    }
    for (crown, mut visibility) in crowns {
        let target = if leaders[crown.0 as usize] {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != target {
            *visibility = target;
        }
    }
}

/// Keep every chip's arrow row current: a slot filled per arrow standing,
/// oldest on the left and drained by its age, and the oldest picked out
/// in gold, pulsing with its post on the beach, while planting another
/// would cost it. That last only for the seats played at this screen.
pub fn update_arrow_dots(
    sim: Res<Sim>,
    seating: Seating,
    settings: Res<GameSettings>,
    time: Res<Time>,
    mut dots: Query<(&ArrowDot, &mut BorderColor)>,
    mut fills: Query<(&ArrowFill, &mut Node, &mut BackgroundColor)>,
    mut lives: Local<[Vec<f32>; MAX_PLAYERS]>,
) {
    let board = &sim.0;
    let mut marked = [false; MAX_PLAYERS];
    for (seat, out) in lives.iter_mut().enumerate() {
        board.signpost_lives(seat as u8, out);
        marked[seat] = seating.played_here(seat as u8) && board.next_to_go(seat as u8).is_some();
    }
    let pulse = next_to_go_pulse(time.elapsed_secs(), settings.reduced_motion);
    // The next to go is the oldest by definition, so the first slot.
    let is_next = |seat: u8, index: u8| index == 0 && marked[usize::from(seat)];
    for (dot, mut border) in &mut dots {
        // The outline says whether an arrow stands there and the fill how
        // long it has left. Told by the fill alone, an arrow about to run
        // out was an empty slot to look at, and three standing read as one.
        let standing = usize::from(dot.index) < lives[usize::from(dot.seat)].len();
        let edge = BorderColor::all(if is_next(dot.seat, dot.index) {
            palette::GOLD
        } else if standing {
            palette::PIP_ON
        } else {
            palette::PIP_OFF
        });
        if *border != edge {
            *border = edge;
        }
    }
    for (fill, mut node, mut bg) in &mut fills {
        let life = lives[usize::from(fill.seat)]
            .get(usize::from(fill.index))
            .copied()
            .unwrap_or(0.0);
        let height = Val::Percent(100.0 * life);
        if node.height != height {
            node.height = height;
        }
        let ink = match is_next(fill.seat, fill.index) {
            true => palette::GOLD.with_alpha(pulse),
            false => palette::PIP_ON,
        };
        menu_ui::set_bg(&mut bg, ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The score chip pops by *scale*, never by font size.
    ///
    /// Animating the size instead re-rasterizes the digits every frame of
    /// the bump. The size here follows rank alone, six fixed values.
    #[test]
    fn the_score_pop_is_scale_and_never_a_new_font_size() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<Online>();
        app.init_resource::<Seats>();
        app.insert_resource(GameSettings::default());
        app.insert_resource(Sim(crate::sim::Board::new(4, 4, 0)));
        app.world_mut().resource_mut::<Seats>().0 = 2;
        app.add_systems(Update, update_side_panels);
        let chip = app
            .world_mut()
            .spawn((
                SideScore { seat: 0, bump: 0.0 },
                Text::new("0"),
                TextFont {
                    font_size: FontSize::Px(SCORE_PX[0]),
                    ..default()
                },
                UiTransform::IDENTITY,
            ))
            .id();

        // A bank puts the chip mid-pop.
        app.world_mut().resource_mut::<Sim>().0.set_score(0, 7);
        app.update();
        let scale_at_peak = app.world().get::<UiTransform>(chip).expect("chip").scale;
        let size_at_peak = app.world().get::<TextFont>(chip).expect("chip").font_size;
        assert!(
            scale_at_peak.x > 1.0,
            "the pop should be scale, got {scale_at_peak:?}"
        );
        assert!(
            app.world().get::<SideScore>(chip).expect("chip").bump > 0.0,
            "the bump should be running"
        );

        // Several frames later it is settling, and the font size has not
        // moved once: one atlas entry for the whole animation.
        for _ in 0..5 {
            app.update();
        }
        let scale_later = app.world().get::<UiTransform>(chip).expect("chip").scale;
        let size_later = app.world().get::<TextFont>(chip).expect("chip").font_size;
        assert!(
            scale_later.x <= scale_at_peak.x,
            "the pop should decay: {scale_at_peak:?} then {scale_later:?}"
        );
        assert_eq!(size_at_peak, FontSize::Px(SCORE_PX[0]));
        assert_eq!(size_later, FontSize::Px(SCORE_PX[0]), "size never animates");
    }

    #[test]
    fn leaders_need_a_unique_nonzero_top() {
        assert_eq!(
            leading_seats(&[0, 0, 0, 0, 0, 0], 4, TeamMode::Solo),
            [false; MAX_PLAYERS]
        );
        assert_eq!(
            leading_seats(&[1, 5, 2, 0, 0, 0], 4, TeamMode::Solo),
            [false, true, false, false, false, false]
        );
        assert_eq!(
            leading_seats(&[5, 5, 0, 0, 0, 0], 2, TeamMode::Solo),
            [false; MAX_PLAYERS]
        );
        // Teams: both members of the strictly leading team are crowned.
        assert_eq!(
            leading_seats(&[1, 2, 4, 0, 0, 0], 4, TeamMode::Pairs),
            [false, false, true, true, false, false]
        );
        // Six seats in trios: the mirror-image half that leads is crowned.
        assert_eq!(
            leading_seats(&[9, 1, 9, 1, 9, 1], 6, TeamMode::Trios),
            [true, false, true, false, true, false]
        );
    }

    #[test]
    fn ranks_sort_by_score_then_seat() {
        // P3 leads, P1 and P2 tie (seat order breaks it), P4 trails.
        assert_eq!(ranks(&[4, 4, 9, 1, 0, 0], 4), [1, 2, 0, 3, 0, 0]);
        // Two seats only: the trailing slots keep rank 0 but are unused.
        assert_eq!(ranks(&[0, 7, 0, 0, 0, 0], 2), [1, 0, 0, 0, 0, 0]);
        // A full table of six ranks all of them.
        assert_eq!(ranks(&[1, 6, 2, 5, 3, 4], 6), [5, 0, 4, 1, 3, 2]);
    }
}
#[cfg(test)]
mod seat_tag_tests {
    use super::*;
    use crate::app::i18n::EN;
    use crate::sim::{DEFAULT_DELAY, Lockstep};
    use crate::transport::{MatchTerms, UdpTransport};

    fn session(local: Option<u8>) -> crate::app::net::OnlineSession {
        let step = match local {
            Some(seat) => Lockstep::new(seat, vec![0, 1], DEFAULT_DELAY),
            None => Lockstep::observer(vec![0, 1], DEFAULT_DELAY),
        };
        crate::app::net::OnlineSession::new(
            UdpTransport::host(0).expect("socket"),
            step,
            2,
            MatchTerms::default(),
        )
    }

    /// Who "(you)" belongs to, which is not the same question as "is
    /// there a local seat".
    ///
    /// Offline the person at the keyboard is always P1, and that guess
    /// used to be made from a local seat of `None`. A spectator has one of
    /// those too, so every watcher on a beach was shown the host's chair
    /// marked as its own, on the panels and the results card both.
    #[test]
    fn a_spectator_is_nobody_at_the_table_rather_than_seat_zero() {
        assert_eq!(
            local_seat(&Online::default(), false),
            Some(0),
            "offline, the keyboard is P1"
        );
        assert_eq!(
            local_seat(&Online::default(), true),
            None,
            "a recording is watched, not played"
        );
        assert_eq!(
            local_seat(&Online(Some(session(Some(1)))), false),
            Some(1),
            "online, whatever chair the host dealt"
        );
        assert_eq!(
            local_seat(&Online(Some(session(None))), false),
            None,
            "and a spectator holds none"
        );

        let controllers = Controllers::default();
        let tag = |local, seat| seat_tag(&EN, &controllers, local, seat);
        assert_eq!(tag(Some(0), 0), EN.tag_you);
        assert_eq!(tag(Some(0), 1), "");
        assert_eq!(tag(None, 0), "", "nobody's chair is the spectator's");
        assert_eq!(tag(None, 1), "");
    }
}
