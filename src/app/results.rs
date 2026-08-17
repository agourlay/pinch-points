//! Round-end overlays: the tide-is-in standings card, the puzzle victory
//! card, and the winner confetti.

use crate::app::i18n::fill;
use crate::app::menu_ui;
use crate::app::net::Online;
use crate::app::palette;
use crate::app::settings::GameSettings;
use crate::app::side_panels::leading_seats;
use crate::app::teams::TeamMode;
use crate::app::{Bots, Campaign, Playback, Seats, Sim};
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

/// Spawn the wrapper that centres the results card, and return its entity.
fn results_card(commands: &mut Commands) -> Entity {
    commands
        .spawn((
            ResultsPanel,
            // Above the header/prompt bars in the UI stack.
            GlobalZIndex(10),
            menu_ui::centred_overlay(),
        ))
        .id()
}

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
    let mut order: Vec<u8> = (0..seats).collect();
    order.sort_by_key(|&seat| std::cmp::Reverse(scores[seat as usize]));
    order
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

/// The tide-is-in standings card: winner headline, ranked scores in seat
/// colours (with AI/you markers), and the round's total haul.
#[allow(clippy::too_many_arguments)]
pub fn spawn_versus_results(
    mut commands: Commands,
    sim: Res<Sim>,
    seats: Res<Seats>,
    settings: Res<GameSettings>,
    names: Res<crate::app::SeatNames>,
    art: Res<crate::app::art::Art>,
    mut rng: ResMut<crate::app::effects::VisualRng>,
    bots: Res<Bots>,
    online: Res<Online>,
    playback: Res<Playback>,
    daily: Res<crate::app::Daily>,
    highlight: Res<crate::app::Highlight>,
    stats: Res<crate::app::achievements::Stats>,
    tournament: Res<crate::app::tournament::Tournament>,
) {
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

    let local = online.0.as_ref().and_then(|s| s.session.seat());
    let rows = standings_rows(&settings, &names, scores, count, mode, |seat| {
        crate::app::side_panels::seat_tag(tr, &bots, local, playback.0.is_some(), seat)
    });
    let haul = board.crabs_banked();

    let card = results_card(&mut commands);
    commands.entity(card).with_children(|wrap| {
        wrap.spawn(menu_ui::screen_card()).with_children(|card| {
            let title = card_text(20.0, CARD_TEXT.darker(0.1));
            card.spawn((Text::new(tr.tide_is_in), title.0, title.1));
            let head = card_text(30.0, headline_color);
            card.spawn((Text::new(headline), head.0, head.1));
            card.spawn(Node {
                height: Val::Px(6.0),
                ..default()
            });
            for (line, color) in rows {
                let row = card_text(23.0, color);
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
            if tournament.active {
                card.spawn(Node {
                    height: Val::Px(6.0),
                    ..default()
                });
                let round = card_text(17.0, CARD_TEXT.darker(0.15));
                card.spawn((
                    Text::new(fill(tr.tour_round, &[("n", &tournament.round.to_string())])),
                    round.0,
                    round.1,
                ));
                let series: Vec<String> =
                    crate::app::tournament::standings(&settings, &names, &tournament, mode, count)
                        .into_iter()
                        .map(|(line, _)| line)
                        .collect();
                let score = card_text(23.0, palette::GOLD);
                card.spawn((Text::new(series.join("  ·  ")), score.0, score.1));
                if tournament.finished {
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
                    card.spawn((Text::new(tr.tour_next), hint.0, hint.1));
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
) {
    let tr = settings.tr();
    let name = settings
        .language
        .level_name(&campaign.current().name)
        .to_string();
    let last = campaign.index + 1 == campaign.levels.len();
    let card = results_card(&mut commands);
    commands.entity(card).with_children(|wrap| {
        wrap.spawn(menu_ui::screen_card()).with_children(|card| {
            let head = card_text(30.0, palette::GOLD);
            let title = match last {
                true => tr.campaign_done,
                false => tr.all_safe,
            };
            card.spawn((Text::new(title), head.0, head.1));
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
            card.spawn((
                Text::new(if last { tr.last_level } else { tr.prompt_won }),
                foot.0,
                foot.1,
            ));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::EN;

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
