//! The versus sidebars: on the left, score chips sorted by rank (the
//! leader's chip is biggest and wears the crown); on the right, the big
//! tide clock over a running log of the round's notable events, with the
//! tide event still running pinned at its top.

mod clock;
mod event_timer;
mod feed;

pub use clock::update_side_clock;
pub use event_timer::update_event_timer;
pub use feed::{EventLog, collect_chat, collect_log, reach_the_board, update_log};

use crate::app::seating::{Seating, beside_the_robot, robot_inline};
use crate::app::settings::GameSettings;
use crate::app::{Seats, Sim};
use crate::app::{menu_ui, palette};
use crate::sim::MAX_PLAYERS;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// Width of each sidebar in px; `fit_camera` reserves this much on both
/// sides.
pub const SIDEBAR_W: f32 = 250.0;

/// How far above the window's foot the sidebars stop, clear of the prompt
/// line.
const SIDEBAR_FOOT: f32 = 44.0;

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

/// One arrow slot on a chip, oldest first: a faint pip while the arrow is
/// in hand, and for an arrow standing an outline, filled and drained by
/// how much of its life is gone. One per arrow the rules allow, so the
/// row is the cap, read at a glance. Not an empty outline in hand: a row
/// of hollow boxes under a name reads as characters the font cannot draw.
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

/// Text written on seat `.0`'s chip, in [`palette::ink_on`] its fill: the
/// score at full strength, the name and its tag (`.1`) a touch under.
#[derive(Component)]
pub struct ChipInk(pub u8, pub bool);

/// A chip's fill: the seat's own colour while it leads, a darker shade
/// while it trails, either one its text reads on.
pub fn chip_fill(seat: u8, leading: bool) -> Color {
    match leading {
        true => palette::leader_chip(seat),
        false => palette::seat_chip(seat),
    }
}

/// An arrow slot's ink on a chip in `fill`: the fill of an arrow standing,
/// and the outline of a free slot, in the chip's own [`palette::ink_on`].
fn pip_on(fill: Color) -> Color {
    palette::ink_on(fill).with_alpha(0.95)
}

fn pip_off(fill: Color) -> Color {
    palette::ink_on(fill).with_alpha(0.25)
}

/// The ink text `named` (a name, else the score) is written in on `fill`.
fn chip_ink(fill: Color, named: bool) -> Color {
    match named {
        true => palette::name_ink_on(fill),
        false => palette::ink_on(fill),
    }
}

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
///
/// Seats on level terms share a size, the smallest of the slots they take
/// between them ([`Chart::size`]), so the column never grows past six of
/// these stacked.
const CHIP_HEIGHTS: [f32; MAX_PLAYERS] = [88.0, 58.0, 54.0, 52.0, 50.0, 48.0];
/// Where the first chip starts, and the gap under each one.
const CHIP_TOP: f32 = 10.0;
const CHIP_GAP: f32 = 8.0;
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
/// Where the log card starts: under the clock card, with the same gap the
/// two have always had.
const LOG_TOP: f32 = clock::CLOCK_TOP + clock::CLOCK_H + 14.0;
/// The hairline round every sidebar card, which Bevy counts inside a
/// card's height.
const CARD_BORDER: f32 = 2.0;

/// How the chips stand: each seat's slot down the column, its place and
/// the size it is drawn at, all indexed by seat.
///
/// The place is the results card's ([`crate::app::teams::placings`]): by
/// the team's total under a team mode, and level totals sharing a place,
/// so a dead heat wears one digit and medal, as the card and the crown
/// call it. Only the order needs a tie broken, and it breaks it by team,
/// then seat, keeping partners together.
struct Chart {
    slot: [usize; MAX_PLAYERS],
    place: [u32; MAX_PLAYERS],
    /// The geometry row a seat is drawn at: the last slot its place takes,
    /// so seats that share a place share a size.
    size: [usize; MAX_PLAYERS],
    /// Whether every seat shares first place, as at the start of a round:
    /// a table with nobody ahead, whose medals are all driftwood rather
    /// than six golds.
    level: bool,
}

impl Chart {
    fn of(scores: &[u32; MAX_PLAYERS], seats: u8, mode: crate::app::teams::TeamMode) -> Chart {
        let seats = seats.clamp(2, MAX_PLAYERS as u8);
        let count = usize::from(seats);
        let placings = crate::app::teams::placings(scores, seats, mode);
        let mut place = [1u32; MAX_PLAYERS];
        place[..count].copy_from_slice(&placings);
        let mut order: Vec<usize> = (0..count).collect();
        order.sort_by_key(|&seat| (place[seat], mode.team_of(seat as u8), seat));
        // Seats past the table keep the slots past it, so the slots stay
        // one per seat and `frames` can walk them in order.
        let mut slot: [usize; MAX_PLAYERS] = std::array::from_fn(|seat| seat);
        for (at, &seat) in order.iter().enumerate() {
            slot[seat] = at;
        }
        let size = std::array::from_fn(|seat| {
            let within = place[..count].iter().filter(|&&p| p <= place[seat]).count();
            within.saturating_sub(1).min(MAX_PLAYERS - 1)
        });
        let level = place[..count].iter().all(|&p| p == 1);
        Chart {
            slot,
            place,
            size,
            level,
        }
    }

    /// The medal disc seat `seat` wears.
    fn medal(&self, seat: usize) -> Color {
        match self.level {
            true => palette::MEDAL_DRIFTWOOD,
            false => MEDALS[(self.place[seat] as usize).clamp(1, MAX_PLAYERS) - 1],
        }
    }

    /// The digit on that medal.
    fn digit(&self, seat: usize) -> &'static str {
        PLACES[(self.place[seat] as usize).clamp(1, MAX_PLAYERS) - 1]
    }

    /// Each seat's chip: its top and its height.
    fn frames(&self) -> [(f32, f32); MAX_PLAYERS] {
        let mut by_slot = [0usize; MAX_PLAYERS];
        for seat in 0..MAX_PLAYERS {
            by_slot[self.slot[seat]] = seat;
        }
        let mut tops = [0.0f32; MAX_PLAYERS];
        let mut top = CHIP_TOP;
        for slot in 0..MAX_PLAYERS {
            tops[slot] = top;
            top += CHIP_HEIGHTS[self.size[by_slot[slot]]] + CHIP_GAP;
        }
        std::array::from_fn(|seat| (tops[self.slot[seat]], CHIP_HEIGHTS[self.size[seat]]))
    }
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
            bottom: Val::Px(SIDEBAR_FOOT),
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
            border: UiRect::all(Val::Px(CARD_BORDER)),
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
            top: Val::Px(CHIP_TOP + f32::from(seat) * (CHIP_HEIGHTS[5] + CHIP_GAP)),
            left: Val::Px(10.0),
            right: Val::Px(10.0),
            height: Val::Px(CHIP_HEIGHTS[5]),
            align_items: AlignItems::Center,
            column_gap: Val::Px(10.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(12.0)),
            ..default()
        },
        BorderColor::all(palette::seat_paint(seat)),
        BackgroundColor(chip_fill(seat, false)),
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
            BackgroundColor(palette::MEDAL_DRIFTWOOD),
        ))
        .with_children(|medal| {
            medal.spawn((
                RankDigit(seat),
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(menu_ui::type_scale::BODY),
                    ..default()
                },
                TextColor(palette::ink_on(palette::MEDAL_DRIFTWOOD)),
            ));
        });
        let fill = chip_fill(seat, false);
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
                        ChipInk(seat, true),
                        TextColor(chip_ink(fill, true)),
                    ))
                    .with_children(|text| {
                        if bot {
                            text.spawn(robot_inline(art, seat, 18.0));
                        }
                        text.spawn((
                            TextSpan::new(beside_the_robot(bot, &name)),
                            font.clone(),
                            ChipInk(seat, true),
                            TextColor(chip_ink(fill, true)),
                        ));
                    });
                });
                if !tag.is_empty() {
                    line.spawn((
                        Text::new(tag.trim_start()),
                        font,
                        TextLayout::no_wrap(),
                        ChipInk(seat, true),
                        TextColor(chip_ink(fill, true)),
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
                        BorderColor::all(Color::NONE),
                        BackgroundColor(pip_off(fill)),
                    ))
                    .with_children(|dot| {
                        dot.spawn((
                            ArrowFill { seat, index },
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Percent(0.0),
                                ..default()
                            },
                            BackgroundColor(pip_on(fill)),
                        ));
                    });
                }
            });
        });
        // The big number, right-aligned.
        chip.spawn((
            SideScore { seat, bump: 0.0 },
            Text::new("0"),
            menu_ui::display_font(SCORE_PX[5]),
            ChipInk(seat, false),
            TextColor(chip_ink(fill, false)),
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

/// How strongly the next-to-go marks show, `secs` into the round: the
/// pulse the arrow on the beach and its dot on the chip share, so the eye
/// ties the two together. Steady at full under reduced motion.
pub fn next_to_go_pulse(secs: f32, reduced_motion: bool) -> f32 {
    if reduced_motion {
        return 1.0;
    }
    0.55 + 0.45 * (secs * std::f32::consts::TAU * 1.2).sin().abs()
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

/// A chip's rank digit and its ink.
type ChipDigit = (
    &'static RankDigit,
    &'static mut Text,
    &'static mut TextColor,
);

/// Every part of the score chips that follows the standings.
#[derive(SystemParam)]
pub struct Chips<'w, 's> {
    frames: Query<'w, 's, ChipFrame>,
    scores: Query<'w, 's, ChipScore>,
    crowns: Query<'w, 's, (&'static LeaderCrown, &'static mut Visibility)>,
    medals: Query<'w, 's, (&'static RankMedal, &'static mut BackgroundColor), Without<SidePanel>>,
    digits: Query<'w, 's, ChipDigit, (Without<SideScore>, Without<ChipInk>)>,
    inks: Query<'w, 's, (&'static ChipInk, &'static mut TextColor)>,
}

/// Keep the chips sorted and sized by rank, the numbers current, and the
/// crown on the leader (all writes guarded).
pub fn update_side_panels(
    scoring: crate::app::teams::Scoring,
    round: crate::app::round_state::RoundState,
    sim: Res<Sim>,
    seats: Res<Seats>,
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
        inks,
    } = &mut chips;
    let board_scores = sim.0.scores();
    let leaders = scoring.leaders(board_scores, seats.0);
    let chart = Chart::of(board_scores, seats.0, scoring.mode(seats.0));
    let frames = chart.frames();
    for (panel, mut node, mut bg, mut border) in panels {
        let seat = panel.0 as usize;
        let (top, height) = (Val::Px(frames[seat].0), Val::Px(frames[seat].1));
        if node.top != top {
            node.top = top;
        }
        if node.height != height {
            node.height = height;
        }
        menu_ui::set_bg(&mut bg, chip_fill(panel.0, leaders[seat]));
        let edge = if leaders[seat] {
            LEADER_BORDER
        } else {
            palette::seat_paint(panel.0)
        };
        let edge = BorderColor::all(edge);
        border.set_if_neq(edge);
    }
    for (medal, mut bg) in medals {
        menu_ui::set_bg(&mut bg, chart.medal(medal.0 as usize));
    }
    for (digit, mut text, mut ink) in digits {
        let seat = digit.0 as usize;
        menu_ui::set_text(&mut text, chart.digit(seat));
        menu_ui::set_color(&mut ink, palette::ink_on(chart.medal(seat)));
    }
    for (chip, mut ink) in inks {
        let fill = chip_fill(chip.0, leaders[usize::from(chip.0)]);
        menu_ui::set_color(&mut ink, chip_ink(fill, chip.1));
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
            chip.bump = if scoring.local.settings.reduced_motion {
                0.0
            } else {
                1.0
            };
        }
        // Held mid-pop under a pause card, with the beach.
        if !round.held() {
            chip.bump = (chip.bump - time.delta_secs() * 4.0).max(0.0);
        }
        // The size follows the seat's standing, and only that: six fixed
        // values, each rasterized into the glyph atlas once and reused
        // forever.
        let base = FontSize::Px(SCORE_PX[chart.size[chip.seat as usize]]);
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
        visibility.set_if_neq(target);
    }
}

/// The arrow slots on the chips, and the chips' fills they are inked for.
#[derive(SystemParam)]
pub struct ArrowSlots<'w, 's> {
    chips: Query<'w, 's, ChipFill, (Without<ArrowFill>, Without<ArrowDot>)>,
    dots: Query<'w, 's, SlotDot, (Without<ArrowFill>, Without<SidePanel>)>,
    fills: Query<'w, 's, SlotFill, (Without<SidePanel>, Without<ArrowDot>)>,
}

/// A chip and its fill, which its slots are inked for.
type ChipFill = (&'static SidePanel, &'static BackgroundColor);

/// An arrow slot: its outline and its in-hand pip.
type SlotDot = (
    &'static ArrowDot,
    &'static mut BorderColor,
    &'static mut BackgroundColor,
);

/// An arrow slot's fill: how full it is, and its ink.
type SlotFill = (
    &'static ArrowFill,
    &'static mut Node,
    &'static mut BackgroundColor,
);

/// Keep every chip's arrow row current: a slot filled per arrow standing,
/// oldest on the left and drained by its age, and the oldest picked out
/// in gold, pulsing with its post on the beach, while planting another
/// would cost it. That last only for the seats played at this screen.
pub fn update_arrow_dots(
    sim: Res<Sim>,
    seating: Seating,
    settings: Res<GameSettings>,
    clock: Res<crate::app::round_state::RoundClock>,
    slots: ArrowSlots,
    mut lives: Local<[Vec<f32>; MAX_PLAYERS]>,
) {
    let ArrowSlots {
        chips,
        mut dots,
        mut fills,
    } = slots;
    let board = &sim.0;
    // Each chip's fill as it is painted, which its slots are inked for.
    let mut chip = [Color::WHITE; MAX_PLAYERS];
    for (panel, fill) in &chips {
        chip[usize::from(panel.0)] = fill.0;
    }
    let mut marked = [false; MAX_PLAYERS];
    for (seat, out) in lives.iter_mut().enumerate() {
        board.signpost_lives(seat as u8, out);
        marked[seat] = seating.played_here(seat as u8) && board.next_to_go(seat as u8).is_some();
    }
    // On the clock its post on the beach pulses to (`dress_signposts`).
    let pulse = next_to_go_pulse(clock.0, settings.reduced_motion);
    // The next to go is the oldest by definition, so the first slot.
    let is_next = |seat: u8, index: u8| index == 0 && marked[usize::from(seat)];
    for (dot, mut border, mut pip) in &mut dots {
        // The outline says an arrow stands there and the fill how long it
        // has left; an arrow in hand is a faint pip with no outline. Told
        // by the fill alone, an arrow about to run out was an empty slot
        // to look at, and three standing read as one.
        let ink = chip[usize::from(dot.seat)];
        let standing = usize::from(dot.index) < lives[usize::from(dot.seat)].len();
        let edge = BorderColor::all(if is_next(dot.seat, dot.index) {
            palette::GOLD
        } else if standing {
            pip_on(ink)
        } else {
            Color::NONE
        });
        border.set_if_neq(edge);
        menu_ui::set_bg(
            &mut pip,
            match standing {
                true => Color::NONE,
                false => pip_off(ink),
            },
        );
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
            false => pip_on(chip[usize::from(fill.seat)]),
        };
        menu_ui::set_bg(&mut bg, ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything a score chip writes is drawn by the fonts it is written
    /// in, in every language: the tag after a name, the medal's digit and
    /// the score. The rows' face draws what DejaVu carries and the
    /// Japanese subset the rest; the score is the display face. Its arrow
    /// slots are drawn, not written, so no glyph there can go missing.
    #[test]
    fn every_character_a_chip_writes_is_in_its_font() {
        use crate::app::i18n::metrics::{characters_in_font, display_covers};
        let mut rows =
            characters_in_font(include_bytes!("../../../assets/fonts/DejaVuSansMono.ttf"));
        rows.extend(characters_in_font(include_bytes!(
            "../../../assets/fonts/NotoSansMonoCJKjp-Subset.otf"
        )));
        let display = display_covers();
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            let mut written: String = [tr.tag_you, tr.tag_ai].concat();
            written.push_str(&crate::app::seat_label(tr, 5));
            written.push_str("0123456789");
            for ch in written.chars().filter(|ch| !ch.is_whitespace()) {
                assert!(rows.contains(&ch), "{lang:?}: {ch:?} has no glyph");
            }
        }
        for ch in "0123456789".chars() {
            assert!(display.contains(&ch), "{ch:?}");
        }
    }

    /// Every rank's medal digit reads on its disc, the driftwood of the
    /// lower places as well as the gold.
    #[test]
    fn every_medal_digit_reads() {
        for (rank, disc) in MEDALS.iter().enumerate() {
            let seen = palette::contrast(palette::ink_on(*disc), *disc);
            assert!(seen >= palette::TEXT_CONTRAST, "rank {rank}: {seen:.2}");
        }
    }

    /// The score chip pops by *scale*, never by font size.
    ///
    /// Animating the size instead re-rasterizes the digits every frame of
    /// the bump. The size here follows rank alone, six fixed values.
    #[test]
    fn the_score_pop_is_scale_and_never_a_new_font_size() {
        let mut app = crate::app::round_state::held_test_app();
        app.init_resource::<crate::app::Playback>();
        app.init_resource::<Seats>();
        app.insert_resource(GameSettings::default());
        app.init_resource::<crate::app::Daily>();
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

        // Held mid-pop under the pause card, with the beach.
        let before = app.world().get::<SideScore>(chip).expect("chip").bump;
        app.insert_resource(crate::app::pause::PauseMenu::raised());
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(100));
        app.update();
        let held = app.world().get::<SideScore>(chip).expect("chip").bump;
        assert_eq!(held, before, "the pop held under the card");
        app.insert_resource(crate::app::pause::PauseMenu::default());

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
    fn level_scores_share_a_place_and_a_size() {
        use crate::app::teams::TeamMode;
        // P3 leads, P1 and P2 tie for second (seat order only orders
        // them), P4 trails in fourth, as the results card numbers them.
        let chart = Chart::of(&[4, 4, 9, 1, 0, 0], 4, TeamMode::Solo);
        assert_eq!(&chart.slot[..4], &[1, 2, 0, 3]);
        assert_eq!(&chart.place[..4], &[2, 2, 1, 4]);
        assert_eq!(&chart.size[..4], &[2, 2, 0, 3], "the pair shares a size");
        assert_eq!((chart.digit(0), chart.digit(1)), ("2", "2"));
        assert_eq!(chart.medal(0), chart.medal(1));
        assert!(!chart.level);
        // A full table of six ranks all of them.
        let chart = Chart::of(&[1, 6, 2, 5, 3, 4], 6, TeamMode::Solo);
        assert_eq!(chart.slot, [5, 0, 4, 1, 3, 2]);
        assert_eq!(chart.size, [5, 0, 4, 1, 3, 2]);
        // A 30-30 finish is a dead heat on the card: no gold "1" and a
        // silver "2".
        let chart = Chart::of(&[30, 30, 0, 0, 0, 0], 2, TeamMode::Solo);
        assert_eq!(&chart.place[..2], &[1, 1]);
        assert_eq!(chart.medal(0), chart.medal(1));
        // At the start every seat is level: one place, one calm medal, the
        // smallest chip, and the column no taller than a ranked one.
        let chart = Chart::of(&[0; MAX_PLAYERS], 6, TeamMode::Solo);
        assert!(chart.level);
        assert_eq!(chart.size, [5; MAX_PLAYERS]);
        assert!((0..6).all(|seat| chart.digit(seat) == "1"));
        assert!((0..6).all(|seat| chart.medal(seat) == palette::MEDAL_DRIFTWOOD));
        let (top, height) = chart.frames()[chart.slot.iter().position(|&s| s == 5).unwrap()];
        let ranked = Chart::of(&[1, 6, 2, 5, 3, 4], 6, TeamMode::Solo).frames();
        let (last_top, last_height) = ranked[0];
        assert!(top + height <= last_top + last_height);
    }

    /// Under a team mode a seat wears its team's place, as the crown and
    /// the results card rank it: the top single score on the losing team
    /// is no gold "1".
    #[test]
    fn a_team_mode_ranks_seats_by_their_team() {
        use crate::app::teams::TeamMode;
        // Pairs at four: {0,1} bank 40, {2,3} bank 50.
        let chart = Chart::of(&[40, 0, 25, 25, 0, 0], 4, TeamMode::Pairs);
        assert_eq!(&chart.place[..4], &[2, 2, 1, 1]);
        assert_eq!(&chart.slot[..4], &[2, 3, 0, 1], "partners side by side");
        assert_eq!(chart.digit(0), "2");
        assert_eq!(chart.medal(2), MEDALS[0]);
        assert_eq!(chart.size[0], chart.size[1]);
    }
}
