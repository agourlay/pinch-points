//! The versus sidebars: on the left, score chips sorted by rank (the
//! leader's chip is biggest and wears the crown); on the right, the big
//! tide clock over a running log of the round's notable events.

mod clock;
mod feed;

pub use clock::update_side_clock;
pub use feed::{EventLog, collect_log, update_log};

use crate::app::net::Online;
use crate::app::settings::GameSettings;
use crate::app::teams::TeamMode;
use crate::app::{Bots, Playback, Seats, Sim};
use crate::app::{menu_ui, palette};
use crate::sim::MAX_PLAYERS;
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

/// One castle-tier pip under a chip's score (filled up to the tier).
#[derive(Component)]
pub struct TierPip {
    seat: u8,
    index: u8,
}

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
const CHIP_TOPS: [f32; MAX_PLAYERS] = [10.0, 108.0, 172.0, 230.0, 284.0, 334.0];
const CHIP_HEIGHTS: [f32; MAX_PLAYERS] = [88.0, 56.0, 50.0, 46.0, 42.0, 40.0];
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
            top: Val::Px(46.0),
            bottom: Val::Px(44.0),
            width: Val::Px(SIDEBAR_W),
            ..default()
        },
    )
}

/// The card language both sidebars share: a hairline edge over the dark
/// panel fill.
fn card(top: f32, height: Option<f32>) -> (Node, BorderColor, BackgroundColor) {
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
        BorderColor::all(palette::HAIRLINE),
        BackgroundColor(palette::CARD_BG),
    )
}

/// One seat's score chip: rank medal, name over tier pips, the big number,
/// and a crown that only the leader shows.
fn spawn_score_chip(
    root: &mut ChildSpawnerCommands,
    art: &crate::app::art::Art,
    seat: u8,
    label: String,
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
        // Name and castle-tier pips.
        chip.spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|mid| {
            mid.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(menu_ui::type_scale::BODY),
                    ..default()
                },
                TextColor(palette::CHIP_NAME),
            ));
            mid.spawn(Node {
                column_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|pips| {
                for index in 0..3u8 {
                    pips.spawn((
                        TierPip { seat, index },
                        Node {
                            width: Val::Px(11.0),
                            height: Val::Px(5.0),
                            border_radius: BorderRadius::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(palette::PIP_OFF),
                    ));
                }
            });
        });
        // The big number, right-aligned.
        chip.spawn((
            SideScore { seat, bump: 0.0 },
            Text::new("0"),
            TextFont {
                font_size: FontSize::Px(SCORE_PX[seat as usize]),
                ..default()
            },
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

/// The right sidebar: the big tide clock over the event feed.
fn spawn_clock_and_feed(commands: &mut Commands) {
    commands.spawn(sidebar(false)).with_children(|root| {
        clock::spawn_clock(root);
        feed::spawn_feed(root);
    });
}

/// Spawn the sidebars after `load_versus` has set the seat count.
#[allow(clippy::too_many_arguments)]
pub fn spawn_side_panels(
    mut commands: Commands,
    seats: Res<Seats>,
    settings: Res<GameSettings>,
    names: Res<crate::app::SeatNames>,
    art: Res<crate::app::art::Art>,
    bots: Res<Bots>,
    online: Res<Online>,
    playback: Res<Playback>,
    mut log: ResMut<EventLog>,
) {
    log.0.clear();
    let tr = settings.tr();
    let local = online.0.as_ref().and_then(|s| s.session.seat());
    let labels: Vec<String> = (0..seats.0.max(2))
        .map(|seat| {
            let tag = seat_tag(tr, &bots, local, playback.0.is_some(), seat);
            format!("{}{tag}", names.label(tr, seat))
        })
        .collect();
    commands.spawn(sidebar(true)).with_children(|root| {
        for (seat, label) in labels.into_iter().enumerate() {
            spawn_score_chip(root, &art, seat as u8, label);
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

/// The "(you)" / "(AI)" tag for a seat, shared by the panels and the
/// results card.
pub fn seat_tag(
    tr: &crate::app::i18n::Tr,
    bots: &Bots,
    local: Option<u8>,
    playback_active: bool,
    seat: u8,
) -> &'static str {
    if bots.0[seat as usize].is_some() {
        tr.tag_ai
    } else if local == Some(seat) || (local.is_none() && !playback_active && seat == 0) {
        tr.tag_you
    } else {
        ""
    }
}

/// Keep the chips sorted and sized by rank, the numbers current, and the
/// crown on the leader (all writes guarded).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_side_panels(
    online: Res<Online>,
    sim: Res<Sim>,
    seats: Res<Seats>,
    settings: Res<GameSettings>,
    time: Res<Time>,
    mut panels: Query<(
        &SidePanel,
        &mut Node,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut scores: Query<(&mut SideScore, &mut Text, &mut TextFont, &mut UiTransform)>,
    mut crowns: Query<(&LeaderCrown, &mut Visibility)>,
    mut pips: Query<(&TierPip, &mut BackgroundColor), Without<SidePanel>>,
    mut medals: Query<(&RankMedal, &mut BackgroundColor), (Without<SidePanel>, Without<TierPip>)>,
    mut digits: Query<(&RankDigit, &mut Text), Without<SideScore>>,
    mut value: Local<String>,
) {
    let board_scores = sim.0.scores();
    let mode = crate::app::teams::in_play(&settings, &online, seats.0);
    let leaders = leading_seats(board_scores, seats.0, mode);
    let rank = ranks(board_scores, seats.0);
    for (panel, mut node, mut bg, mut border) in &mut panels {
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
    for (medal, mut bg) in &mut medals {
        menu_ui::set_bg(&mut bg, MEDALS[rank[medal.0 as usize]]);
    }
    for (digit, mut text) in &mut digits {
        menu_ui::set_text(&mut text, PLACES[rank[digit.0 as usize]]);
    }
    for (mut chip, mut text, mut font, mut transform) in &mut scores {
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
        // The pop is scale, not size. Bevy keys its glyph atlas by font
        // size, so animating the size allocates a fresh atlas and
        // re-rasterizes the digits on every frame of the bump: 4.4% of the
        // game's whole CPU, for something the GPU does for nothing.
        let pop = Vec2::splat(1.0 + 0.22 * chip.bump);
        if transform.scale != pop {
            transform.scale = pop;
        }
    }
    for (crown, mut visibility) in &mut crowns {
        let target = if leaders[crown.0 as usize] {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != target {
            *visibility = target;
        }
    }
    for (pip, mut bg) in &mut pips {
        let tier = crate::sim::castle_tier(board_scores[pip.seat as usize]);
        let filled = pip.index < tier;
        let target = if filled {
            palette::PIP_ON
        } else {
            palette::PIP_OFF
        };
        menu_ui::set_bg(&mut bg, target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The score chip pops by *scale*, never by font size.
    ///
    /// Animating the size instead re-rasterizes the digits every frame of
    /// the bump, at the cost the code beside it records. The size here
    /// follows rank alone, which takes six fixed values.
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
