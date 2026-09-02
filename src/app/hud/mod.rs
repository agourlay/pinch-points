//! HUD: a header bar with the mode title, per-player colored score chips,
//! and the clock/inventory, plus a contextual prompt along the bottom. The
//! board is still the primary readout (spec §3.4); this is the redundancy.

use crate::app::editor::EditorState;
use crate::app::lobby::LobbyState;
use crate::app::net::Online;
use crate::app::settings::GameSettings;
use crate::app::{Bots, Campaign, Phase, Playback, Screen, Seats, Sim, VersusPhase};
use crate::app::{menu_ui, palette};

use bevy::prelude::*;

mod text;

use crate::app::palette::CLOCK_CALM;
pub(crate) use text::{clock_color, clock_into, event_name, urgency_band};

#[derive(Component)]
pub struct LevelLabel;

#[derive(Component)]
pub struct PostsLabel;

#[derive(Component)]
pub struct PromptLabel;

/// The header strip; its dark backdrop hides on the menu so the sky
/// reaches the top of the window.
#[derive(Component)]
pub struct HeaderBar;

pub fn spawn_hud(
    mut commands: Commands,
    settings: Res<crate::app::settings::GameSettings>,
    art: Res<crate::app::art::Art>,
) {
    let font = TextFont {
        font_size: FontSize::Px(22.0),
        ..default()
    };
    // Header bar: title | score chips | clock-or-inventory.
    commands
        .spawn((
            HeaderBar,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Px(42.0),
                padding: UiRect::horizontal(Val::Px(12.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                column_gap: Val::Px(18.0),
                ..default()
            },
            BackgroundColor(palette::HEADER_FILL),
        ))
        .with_children(|bar| {
            bar.spawn((
                LevelLabel,
                Text::new(""),
                font.clone(),
                TextColor(Color::WHITE),
            ));
            bar.spawn((
                PostsLabel,
                Text::new(""),
                font.clone(),
                TextColor(Color::WHITE),
            ));
        });
    // The prompt rides its own dark pill. On the play screens that is
    // nearly invisible against the backdrop; on the menu it is the only
    // thing keeping pale text legible over bright sand.
    commands.spawn((
        PromptLabel,
        Text::new(""),
        font,
        TextColor(palette::SELECTED_ROW),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(8.0),
            left: Val::Px(12.0),
            // Never the whole width: the menu's version pill sits at the
            // other end of this strip, and a long prompt (French, on a
            // big UI scale) wraps to its second row, which the chrome
            // already reserves, rather than running under it.
            max_width: Val::Percent(88.0),
            padding: UiRect::axes(Val::Px(12.0), Val::Px(5.0)),
            border_radius: BorderRadius::all(Val::Px(11.0)),
            ..default()
        },
        BackgroundColor(palette::PILL_FILL),
    ));
    // Teaching hint: a soft line under the header while placing signposts
    // on levels that carry one.
    commands
        .spawn((Node {
            position_type: PositionType::Absolute,
            top: Val::Px(46.0),
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },))
        .with_children(|wrap| {
            wrap.spawn((
                HintLabel,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(menu_ui::type_scale::ROW),
                    ..default()
                },
                TextColor(palette::SELECTED_ROW.with_alpha(0.85)),
            ));
        });

    // The tide clock: big, top-centre, red for the final scramble. A
    // full-width centring wrapper keeps the digits centred at any width.
    // `ZIndex(1)` lifts it above the header bar it overlaps: the bar is
    // spawned first and opaque, and without this the digits paint behind it.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(2.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            ZIndex(1),
        ))
        .with_children(|wrap| {
            wrap.spawn((
                TideClock,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(menu_ui::type_scale::DISPLAY),
                    ..default()
                },
                TextColor(CLOCK_CALM),
            ));
        });
    spawn_field_guide(&mut commands, settings.tr(), &art);
}

/// Teaching-hint line under the header (puzzle setup only).
#[derive(Component)]
pub struct HintLabel;

/// The big versus tide clock (top-centre).
#[derive(Component)]
pub struct TideClock;

/// The one shipped level whose lesson is the keys themselves (see
/// `LEVEL_HINTS`); a test checks the table still says so.
pub(crate) const KEY_LESSON_LEVEL: &str = "Welcome Ashore";

/// Show a level's teaching hint while its signposts are being placed.
#[allow(clippy::too_many_arguments)]
pub fn update_hint(
    campaign: Res<Campaign>,
    screen: Res<State<Screen>>,
    phase: Res<State<Phase>>,
    settings: Res<GameSettings>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    stuck: Res<crate::app::hint::Hints>,
    denied: Res<crate::app::hint::DeniedNote>,
    mut hints: Query<&mut Text, With<HintLabel>>,
) {
    let line = if *screen.get() != Screen::Puzzle {
        String::new()
    } else {
        // A stuck player is told about the hint instead of being told the
        // lesson again; the lesson is what they have already tried.
        crate::app::hint::hint_line(settings.tr(), &stuck, &denied, phase.get()).unwrap_or_else(
            || {
                let name = &campaign.current().name;
                // The first level's lesson names the stock keys; with the keys
                // rebound or the one-hand preset on it would teach the wrong
                // ones, and the prompt line already points at Settings.
                let honest = name != KEY_LESSON_LEVEL || settings.stock_legend();
                if *phase.get() == Phase::Setup && honest {
                    caps.legend(settings.language.level_hint(name).unwrap_or(""))
                } else {
                    String::new()
                }
            },
        )
    };
    for mut text in &mut hints {
        menu_ui::set_text(&mut text, &line);
    }
}

/// Drive the big clock: mm:ss during a versus round, red inside the last
/// 30 seconds, pulsing for the final 10. Colour-only pulsing: it never
/// re-shapes the text.
pub fn update_tide_clock(
    sim: Res<Sim>,
    screen: Res<State<Screen>>,
    phase: Res<State<Phase>>,
    time: Res<Time>,
    settings: Res<GameSettings>,
    mut clocks: Query<(&mut Text, &mut TextColor), With<TideClock>>,
    mut line: Local<String>,
) {
    let remaining = match screen.get() {
        // Versus shows its clock in the right sidebar instead.
        Screen::Versus => None,
        // Every running puzzle counts down: its own round if it has one,
        // otherwise the campaign-wide tick limit that would otherwise fail
        // the level invisibly.
        Screen::Puzzle if *phase.get() == Phase::Running => sim.0.remaining_ticks().or(Some(
            crate::sim::PUZZLE_TICK_LIMIT.saturating_sub(sim.0.ticks()),
        )),
        // A timed level shows its deadline while the signposts are still
        // going down. Dry Feet is decided in eight seconds and the player
        // was choosing where to spend their one post with nothing on screen
        // saying so. Only levels that carry a round: the campaign tick
        // limit is a backstop, not a deadline, and counting it down over an
        // untimed puzzle would invent a pressure that is not there.
        Screen::Puzzle if *phase.get() == Phase::Setup => sim.0.remaining_ticks(),
        Screen::Puzzle
        | Screen::Menu
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
        | Screen::NewVersion => None,
    };
    for (mut text, mut color) in &mut clocks {
        let Some(ticks) = remaining else {
            if !text.0.is_empty() {
                text.0.clear();
            }
            continue;
        };
        clock_into(&mut line, ticks);
        menu_ui::set_text(&mut text, &line);
        let target = clock_color(
            ticks,
            sim.0.round_length(),
            time.elapsed_secs(),
            !settings.reduced_motion,
        );
        menu_ui::set_color(&mut color, target);
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update_hud(
    sim: Res<Sim>,
    campaign: Res<Campaign>,
    screen: Res<State<Screen>>,
    phase: Res<State<Phase>>,
    vphase: Res<State<VersusPhase>>,
    editor: Res<EditorState>,
    online: Res<Online>,
    playback: Res<Playback>,
    lobby: Res<LobbyState>,
    seats: Res<Seats>,
    // Tupled: a system takes at most sixteen parameters, and this one was
    // already there before the seat names arrived.
    (settings, keycaps, names): (
        Res<GameSettings>,
        Res<crate::app::keycaps::KeyCaps>,
        Res<crate::app::SeatNames>,
    ),
    bots: Res<Bots>,
    library: Res<crate::app::replays::Library>,
    // Tupled for the same reason as the pair above: sixteen is the limit.
    (notice, match_menu): (
        Res<crate::app::RoundNotice>,
        Res<crate::app::match_setup::MatchMenu>,
    ),
    // Tupled for the same reason again: sixteen is the limit.
    (speed, tournament): (
        Res<crate::app::replays::PlaybackSpeed>,
        Res<crate::app::tournament::Tournament>,
    ),
    mut labels: ParamSet<(
        Query<&mut Text, With<LevelLabel>>,
        Query<&mut Text, With<PostsLabel>>,
        Query<(&mut Text, &mut Node), With<PromptLabel>>,
    )>,
) {
    let tr = settings.tr();
    let lang = settings.language;
    let said = text::screen_text(
        *screen.get(),
        &text::Readout {
            tr,
            lang,
            sim: &sim,
            campaign: &campaign,
            phase: &phase,
            vphase: &vphase,
            editor: &editor,
            online: &online,
            playback: &playback,
            lobby: &lobby,
            tournament: &tournament,
            seats: &seats,
            settings: &settings,
            keycaps: &keycaps,
            names: &names,
            bots: &bots,
            library: &library,
            notice: &notice,
            match_menu: &match_menu,
            speed: speed.0,
        },
    );
    if let Ok(mut text) = labels.p0().single_mut() {
        menu_ui::set_text(&mut text, &said.title);
    }
    if let Ok(mut text) = labels.p1().single_mut() {
        menu_ui::set_text(&mut text, &said.status);
    }
    if let Ok((mut text, mut node)) = labels.p2().single_mut() {
        menu_ui::set_text(&mut text, &said.prompt);
        // An empty prompt keeps its pill off the sand: a bare dark lozenge
        // in the corner reads as a broken widget, not as quiet.
        menu_ui::set_shown(&mut node, !said.prompt.is_empty());
    }
}

/// The menu is a full-bleed postcard: the header backdrop gets out of
/// the way there and returns on every other screen.
/// The crab field guide: each kind in its colour with what it banks.
///
/// It used to live on the landing menu, which is the one screen where you
/// are not looking at a crab. It sits at the foot of the play screens
/// instead, opposite the prompt, where the thing it explains is on the
/// board in front of you.
#[derive(Component)]
pub struct FieldGuide;

/// One crab's note in the guide: `KINDS[i]`, so a language change can
/// find and reword it.
#[derive(Component)]
pub struct FieldGuideNote(usize);

/// The kinds the guide lists, in the order shown.
const KINDS: [crate::sim::CrabKind; 6] = [
    crate::sim::CrabKind::Common,
    crate::sim::CrabKind::Juvenile,
    crate::sim::CrabKind::Giant,
    crate::sim::CrabKind::Molting,
    crate::sim::CrabKind::Golden,
    crate::sim::CrabKind::Sparkling,
];

/// What the guide says of `KINDS[i]`: its worth, and its trait in the
/// player's language. The name is the one part left off: on the board a
/// crab is a colour, and the colour is right there.
fn field_guide_note(tr: &crate::app::i18n::Tr, i: usize) -> String {
    let kind = KINDS[i];
    let note = tr.crab_notes[i];
    if note.is_empty() {
        kind.value().to_string()
    } else {
        format!("{} {note}", kind.value())
    }
}

/// The notes are baked from the language at start-up; a language picked
/// later rewords them here.
pub fn update_field_guide(
    settings: Res<GameSettings>,
    mut notes: Query<(&FieldGuideNote, &mut Text)>,
) {
    if !settings.is_changed() {
        return;
    }
    let tr = settings.tr();
    for (note, mut text) in &mut notes {
        menu_ui::set_text(&mut text, &field_guide_note(tr, note.0));
    }
}

fn spawn_field_guide(
    commands: &mut Commands,
    tr: &crate::app::i18n::Tr,
    art: &crate::app::art::Art,
) {
    // Its own row, in the band between the foot of the board and the
    // prompt. It cannot share the prompt's line: with the traits on it the
    // two come to more than the window is wide in every language, and most
    // of a crab is what it does, not what it banks.
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
                    strip.spawn((
                        ImageNode::new(art.crab.clone())
                            .with_color(crate::app::creatures::body_color(*kind)),
                        Node {
                            width: Val::Px(20.0),
                            height: Val::Px(20.0),
                            ..default()
                        },
                    ));
                    strip.spawn((
                        FieldGuideNote(i),
                        Text::new(field_guide_note(tr, i)),
                        TextFont {
                            font_size: FontSize::Px(menu_ui::type_scale::FINE),
                            ..default()
                        },
                        TextLayout::no_wrap(),
                        TextColor(palette::PARCHMENT.with_alpha(0.85)),
                    ));
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

pub fn header_backdrop(
    screen: Res<State<Screen>>,
    mut bars: Query<&mut BackgroundColor, With<HeaderBar>>,
) {
    let target = if *screen.get() == Screen::Menu {
        Color::NONE
    } else {
        palette::HEADER_FILL
    };
    for mut bg in &mut bars {
        menu_ui::set_bg(&mut bg, target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::i18n::Lang;

    /// The level whose hint is withheld under rebound keys is the one that
    /// names keys, and the only one: the others teach the beach, and stay
    /// up whatever the keyboard says.
    #[test]
    fn only_the_key_lesson_names_keys() {
        // "arrow key", not "arrow": the signposts are called arrows now, so
        // the bare word is the *object* and appears in several hints. Only
        // the keys are withheld under a rebound keyboard.
        let names_keys = |hint: &str| hint.contains("WASD") || hint.contains("arrow key");
        assert!(names_keys(Lang::En.level_hint(KEY_LESSON_LEVEL).unwrap()));
        for level in crate::sim::campaign_levels() {
            if level.name != KEY_LESSON_LEVEL
                && let Some(hint) = Lang::En.level_hint(&level.name)
            {
                assert!(!names_keys(hint), "{:?} names keys too", level.name);
            }
        }
    }
}
