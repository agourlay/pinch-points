//! HUD: a header bar with the mode title, per-player colored score chips,
//! and the clock/inventory, plus a contextual prompt along the bottom. The
//! board is still the primary readout (spec §3.4); this is the redundancy.

use crate::app::editor::EditorState;
use crate::app::lobby::LobbyState;
use crate::app::settings::GameSettings;
use crate::app::{Campaign, Phase, Playback, Screen, Sim, VersusPhase};
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

/// The share of the window the prompt's pill may take, as its node says.
const PROMPT_SHARE: f32 = 0.88;
/// The pill's padding either side of the text.
const PROMPT_PAD_X: f32 = 12.0;
/// The smallest the prompt shrinks to on the play screens: the fine print.
const PROMPT_MIN_PX: f32 = menu_ui::type_scale::FINE;

/// The size the prompt is drawn at on a play screen, given how wide it
/// would be at [`HEADER_PX`] and how much room its pill has: its own size
/// if it fits, smaller until it does, and `None` if even
/// [`PROMPT_MIN_PX`] cannot hold it on one line, for the caller to wrap.
fn prompt_fit(natural: f32, room: f32) -> Option<f32> {
    if natural <= room {
        return Some(HEADER_PX);
    }
    let size = HEADER_PX * room / natural;
    (size >= PROMPT_MIN_PX).then_some(size)
}

/// The prompt line as `fit_prompt` sizes it: the words, the size and
/// wrapping it is set in, and how wide it came out last frame.
type PromptText = (
    &'static Text,
    &'static mut TextFont,
    &'static mut TextLayout,
    &'static bevy::text::TextLayoutInfo,
);

/// Keep the prompt on one line on the screens that have the crab legend
/// just above it.
///
/// The prompt sits at the foot and wraps upward when it is long, into a
/// second row the chrome once kept for it. The legend strip took that row,
/// so the two-seat prompt at 720p ran its second line over the legend. On
/// those screens it now stays one line and shrinks until it fits, which
/// text width makes a single step: it scales with the font size. Anywhere
/// else it wraps as it always has, where the screens make room for two
/// rows (the lobby does). A prompt too long even at the fine print's size
/// wraps too, since running off the window is worse than the overlap.
///
/// Reads last frame's layout, so a new prompt is one frame at the old
/// size before it settles.
pub fn fit_prompt(
    viewport: crate::app::menu_ui::Viewport,
    guides: Query<&Node, (With<FieldGuide>, Without<PromptLabel>)>,
    mut prompts: Query<PromptText, With<PromptLabel>>,
    // The line given up on: too long even at the fine print, and wrapped.
    // Held until the words change, or measuring the wrapped pill as if it
    // were the line would unwrap it and give up again, every other frame.
    mut too_long: Local<Option<String>>,
) {
    let legend_up = guides.iter().any(|node| node.display != Display::None);
    let Some(size) = viewport.size() else {
        return;
    };
    let room = size.x * PROMPT_SHARE - 2.0 * PROMPT_PAD_X;
    for (text, mut font, mut layout, info) in &mut prompts {
        let FontSize::Px(now) = font.font_size else {
            continue;
        };
        let given_up = too_long.as_deref() == Some(text.0.as_str());
        let (size, wrap) = if legend_up && given_up {
            (PROMPT_MIN_PX, true)
        } else if legend_up && info.size.x > 0.0 {
            let drawn = info.size.x / info.scale_factor.max(f32::EPSILON);
            // How wide it is at full size: measured on one line, since it
            // only ever is on these screens once this has run.
            let natural = drawn * HEADER_PX / now;
            let one_line = layout.linebreak == LineBreak::NoWrap;
            match prompt_fit(natural, room) {
                Some(size) if one_line => (size, false),
                // Wrapped last frame: its width is the pill's, not the
                // line's. Unwrap at full size and measure next frame.
                Some(_) => (HEADER_PX, false),
                None => {
                    *too_long = Some(text.0.clone());
                    (PROMPT_MIN_PX, true)
                }
            }
        } else {
            (HEADER_PX, true)
        };
        // Half-pixel steps: a sub-pixel change is no change to read, and
        // every write re-shapes the line.
        let size = (size * 2.0).floor() / 2.0;
        if (now - size).abs() > 0.01 {
            font.font_size = FontSize::Px(size);
        }
        let linebreak = if wrap {
            LineBreak::WordBoundary
        } else {
            LineBreak::NoWrap
        };
        if layout.linebreak != linebreak {
            layout.linebreak = linebreak;
        }
    }
}

/// The header strip; its dark backdrop hides on the menu so the sky
/// reaches the top of the window.
#[derive(Component)]
pub struct HeaderBar;

/// The header bar's lines: the title on the left, the tally on the right,
/// in the display face.
const HEADER_PX: f32 = 22.0;

pub fn spawn_hud(
    mut commands: Commands,
    settings: Res<GameSettings>,
    art: Res<crate::app::art::Art>,
) {
    let font = menu_ui::display_font(HEADER_PX);
    // Header bar: title | score chips | clock-or-inventory.
    commands
        .spawn((
            HeaderBar,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Px(menu_ui::HEADER_H),
                padding: UiRect::horizontal(Val::Px(12.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                column_gap: Val::Px(18.0),
                ..default()
            },
            BackgroundColor(palette::HEADER_FILL),
        ))
        .with_children(|bar| {
            // The title in a box of its own, because a node does not clip
            // its own text: the clip has to come from a parent. How wide
            // the box is `update_hud_text`'s business, one screen at a
            // time. No wrapping inside it either, since the bar is one
            // line tall and a second row would be drawn over the board.
            bar.spawn((
                TitleBox,
                Node {
                    overflow: Overflow::clip_x(),
                    flex_shrink: 0.0,
                    ..default()
                },
            ))
            .with_children(|box_| {
                box_.spawn((
                    LevelLabel,
                    Text::new(""),
                    font.clone(),
                    TextColor(palette::HUD_INK),
                    TextLayout::no_wrap(),
                ));
            });
            bar.spawn((
                PostsLabel,
                Text::new(""),
                font.clone(),
                TextColor(palette::HUD_INK),
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
            top: Val::Px(menu_ui::UNDER_HEADER),
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
                menu_ui::display_font(menu_ui::type_scale::DISPLAY),
                TextColor(CLOCK_CALM),
            ));
        });
    spawn_field_guide(&mut commands, settings.tr(), &art);
}

/// The box the title is written in, which is what holds it clear of the
/// clock: a node does not clip its own text, so the width lives out here
/// rather than on the label itself.
#[derive(Component)]
pub struct TitleBox;

/// How much of the header bar the title may take, on a screen that has a
/// clock centred in that same bar.
///
/// The clock is absolutely positioned in a full-width centring wrapper, so
/// flexbox cannot hold the two apart: a long level name simply ran under
/// the digits. At the editor's own 28-character limit a name overflowed
/// the room before the clock in every one of the eight languages, by 28px
/// in Japanese and 178px in Spanish, and the two painted over each other.
///
/// 44% of the bar leaves the clock its middle with room to spare at the
/// design width and more at any larger one, since both are measured from
/// the same centre.
const TITLE_SHARE_WITH_CLOCK: f32 = 44.0;

/// How wide the title may be on `screen`: held clear of the clock where
/// there is one, and otherwise left alone, since the editor's header is
/// the longest in the game and has the whole bar to itself.
fn title_width(screen: Screen) -> Val {
    match screen {
        Screen::Puzzle => Val::Percent(TITLE_SHARE_WITH_CLOCK),
        Screen::Menu
        | Screen::Versus
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
        | Screen::NewVersion => Val::Auto,
    }
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

/// What the player has run into on this level: how stuck they are, and
/// the last placement refused.
#[derive(bevy::ecs::system::SystemParam)]
pub struct HintNotes<'w> {
    stuck: Res<'w, crate::app::hint::Hints>,
    denied: Res<'w, crate::app::hint::DeniedNote>,
}

/// Show a level's teaching hint while its signposts are being placed.
pub fn update_hint(
    campaign: Res<Campaign>,
    screen: Res<State<Screen>>,
    phase: Res<State<Phase>>,
    settings: Res<GameSettings>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    notes: HintNotes,
    mut hints: Query<&mut Text, With<HintLabel>>,
) {
    let HintNotes { stuck, denied } = notes;
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
        // going down: Dry Feet is decided in eight seconds, with the player
        // choosing where to spend their one post. Only levels that carry a
        // round, since the campaign tick limit is a backstop rather than a
        // deadline.
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

/// Where every screen is at: the states, and the menus whose row or card
/// changes what the prompt may promise.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Modes<'w> {
    screen: Res<'w, State<Screen>>,
    phase: Res<'w, State<Phase>>,
    vphase: Res<'w, State<VersusPhase>>,
    editor: Res<'w, EditorState>,
    lobby: Res<'w, LobbyState>,
    match_menu: Res<'w, crate::app::match_setup::MatchMenu>,
    pause_menu: Res<'w, crate::app::pause::PauseMenu>,
    coop: Res<'w, crate::app::Coop>,
}

/// Everything the header, the status slot and the prompt are written from:
/// the sources of a [`text::Readout`], read together every frame.
#[derive(bevy::ecs::system::SystemParam)]
pub struct HudSources<'w> {
    modes: Modes<'w>,
    seating: crate::app::side_panels::Seating<'w>,
    sim: Res<'w, Sim>,
    campaign: Res<'w, Campaign>,
    settings: Res<'w, GameSettings>,
    keycaps: Res<'w, crate::app::keycaps::KeyCaps>,
    library: Res<'w, crate::app::replays::Library>,
    notice: Res<'w, crate::app::RoundNotice>,
    speed: Res<'w, crate::app::replays::PlaybackSpeed>,
    tournament: Res<'w, crate::app::tournament::Tournament>,
    spectators: Res<'w, crate::app::spectators::SpectatorChat>,
}

impl HudSources<'_> {
    fn readout(&self) -> text::Readout<'_> {
        let Self {
            modes,
            seating,
            sim,
            campaign,
            settings,
            keycaps,
            library,
            notice,
            speed,
            tournament,
            spectators,
        } = self;
        text::Readout {
            tr: settings.tr(),
            lang: settings.language,
            sim,
            campaign,
            coop: modes.coop.in_play(campaign),
            phase: &modes.phase,
            vphase: &modes.vphase,
            editor: &modes.editor,
            online: &seating.online,
            playback: &seating.playback,
            lobby: &modes.lobby,
            tournament,
            seats: &seating.seats,
            settings,
            keycaps,
            names: &seating.names,
            controllers: &seating.controllers,
            library,
            notice,
            match_menu: &modes.match_menu,
            paused: modes.pause_menu.open,
            spectator_typing: spectators.0.as_deref(),
            crowd: seating
                .online
                .0
                .as_ref()
                .map_or_else(Default::default, |session| session.stands.crowd),
            speed: speed.0,
        }
    }
}

/// The header's four parts, written in turn.
type HudLabels<'w, 's> = ParamSet<
    'w,
    's,
    (
        Query<'static, 'static, &'static mut Text, With<LevelLabel>>,
        Query<'static, 'static, &'static mut Text, With<PostsLabel>>,
        Query<'static, 'static, (&'static mut Text, &'static mut Node), With<PromptLabel>>,
        Query<'static, 'static, &'static mut Node, With<TitleBox>>,
    ),
>;

pub fn update_hud(sources: HudSources, mut labels: HudLabels) {
    let screen = sources.modes.screen.get();
    let said = text::screen_text(*screen, &sources.readout());
    if let Ok(mut text) = labels.p0().single_mut() {
        menu_ui::set_text(&mut text, &said.title);
    }
    if let Ok(mut node) = labels.p3().single_mut() {
        // A definite width rather than a maximum: the box is sized by the
        // text inside it, which a maximum does not reach.
        let wanted = title_width(*screen);
        if node.width != wanted {
            node.width = wanted;
        }
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

/// The crab field guide: each kind in its colour with what it banks.
///
/// At the foot of the play screens, opposite the prompt, where the thing it
/// explains is on the board in front of you, rather than on the landing
/// menu, which is the one screen with no crab on it.
#[derive(Component)]
pub struct FieldGuide;

/// One crab's note in the guide: `KINDS[i]`, so a language change can
/// find and reword it.
#[derive(Component)]
pub struct FieldGuideNote(usize);

/// A crab's size in the guide. It was 20, and a crab drawn with its legs
/// out to the corners of its square was a speck at that size.
const LEGEND_CRAB_PX: f32 = 26.0;

/// The sand a legend crab stands on: the board's own, a touch dimmed so the
/// coins do not outshine the notes beside them.
const LEGEND_SAND: Color = Color::srgb(0.84, 0.77, 0.62);

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
    // two come to more than the window is wide in every language.
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
                    // The crab as it walks the board, big claw and all,
                    // rather than a coloured dot: its body and claw in one
                    // square, the claw in the body's colour, since the
                    // guide is about kinds and not about hands.
                    let color = crate::app::creatures::body_color(*kind);
                    let layer = |image: &Handle<Image>| {
                        (
                            ImageNode::new(image.clone()).with_color(color),
                            Node {
                                position_type: PositionType::Absolute,
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                        )
                    };
                    // Each on a coin of sand, the ground it is drawn for:
                    // its legs and outline are dark, and straight on the
                    // dark pill they vanished and left a blob with a claw.
                    strip
                        .spawn((
                            Node {
                                width: Val::Px(LEGEND_CRAB_PX),
                                height: Val::Px(LEGEND_CRAB_PX),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(LEGEND_SAND),
                        ))
                        .with_children(|icon| {
                            icon.spawn(layer(&art.crab));
                            icon.spawn(layer(&art.claw));
                        });
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

/// The menu is a full-bleed postcard: the header backdrop gets out of the
/// way there and returns on every other screen.
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

    /// A prompt that fits keeps its size; one that does not shrinks by
    /// exactly as much as it is too wide, since width scales with size; and
    /// one that would need to go below the fine print is left to wrap.
    #[test]
    fn a_prompt_shrinks_to_its_room_and_no_further() {
        assert_eq!(prompt_fit(900.0, 1000.0), Some(HEADER_PX));
        let size = prompt_fit(1100.0, 1000.0).expect("a tenth too wide fits smaller");
        assert!((size - HEADER_PX * 1000.0 / 1100.0).abs() < 0.01, "{size}");
        assert_eq!(
            prompt_fit(2000.0, 1000.0),
            None,
            "under the fine print: wrap"
        );
    }

    /// Every prompt in every language, with the mute key the play screens
    /// add to it, fits one line of a 720p window without going under the
    /// fine print. The play screens keep their prompt on one line so it
    /// stays off the crab legend, and a prompt this cannot hold would wrap
    /// back over it.
    #[test]
    fn every_prompt_fits_one_line_at_720p() {
        use crate::app::i18n::ALL_LANGS;
        use crate::app::i18n::metrics::display_px;
        let room = 1280.0 * PROMPT_SHARE - 2.0 * PROMPT_PAD_X;
        for lang in ALL_LANGS {
            let tr = lang.tr();
            for (field, line) in tr.strings() {
                if !field.starts_with("prompt_") {
                    continue;
                }
                let prompt = format!("{line} | {}", tr.prompt_mute);
                let natural = display_px(&prompt, HEADER_PX);
                assert!(
                    prompt_fit(natural, room).is_some(),
                    "{lang:?} {field} is {natural:.0}px at full size, too long for one line: {prompt}"
                );
            }
        }
    }

    /// The title stops before the clock, on the one screen that puts a
    /// clock in the same bar.
    ///
    /// The clock floats over the bar rather than sitting in it, so nothing
    /// but this number keeps them apart. A level name may be 28 characters
    /// (`editor::NAME_MAX`), and at that length the header overran the room
    /// before the clock in all eight languages and the two painted over
    /// each other.
    #[test]
    fn a_long_level_name_stops_before_the_clock() {
        use crate::app::i18n::metrics::display_px;
        use crate::app::settings::DESIGN_W;

        // Where the clock's left edge falls at the design width: it is
        // centred, so half of its widest reading sits left of the middle.
        // "10:00" rather than "0:59", since a long round counts in tens.
        let clock = display_px("10:00", menu_ui::type_scale::DISPLAY);
        let clock_left = DESIGN_W / 2.0 - clock / 2.0;

        let Val::Percent(share) = title_width(Screen::Puzzle) else {
            panic!("the puzzle title is held to a share of the bar");
        };
        let title_right = DESIGN_W * share / 100.0;
        assert!(
            title_right < clock_left,
            "the title reaches {title_right:.0}px and the clock starts at {clock_left:.0}px"
        );

        // And the longest header the game can produce does overrun that,
        // which is what the limit is for: without it this is what ran under
        // the digits.
        let name = "W".repeat(28);
        let worst = crate::app::i18n::ALL_LANGS
            .into_iter()
            .map(|lang| {
                let tr = lang.tr();
                [tr.title_tide_pool, tr.title_beach_day, tr.stage_custom]
                    .into_iter()
                    .map(|section| display_px(&format!("{section} 100/100 - {name}"), HEADER_PX))
                    .fold(0.0f32, f32::max)
            })
            .fold(0.0f32, f32::max);
        assert!(
            worst > title_right,
            "nothing to clip: the longest header is {worst:.0}px into {title_right:.0}px"
        );

        // Every other screen keeps the whole bar: the editor's header is
        // the longest in the game and has no clock to share it with.
        assert_eq!(title_width(Screen::Editor), Val::Auto);
        assert_eq!(title_width(Screen::Versus), Val::Auto);
    }

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
