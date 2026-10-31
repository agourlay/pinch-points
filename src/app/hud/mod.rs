//! HUD: a header bar with the mode title, per-player colored score chips,
//! and the clock/inventory, plus a contextual prompt along the bottom. The
//! board is still the primary readout (spec §3.4); this is the redundancy.

use crate::app::editor::EditorState;
use crate::app::lobby::LobbyState;
use crate::app::settings::GameSettings;
use crate::app::{Campaign, Phase, Screen, Sim, VersusPhase};
use crate::app::{menu_ui, palette};

use bevy::prelude::*;

mod clock;
mod field_guide;
mod text;

use crate::app::palette::CLOCK_CALM;
pub(crate) use clock::{Blink, clock_color, clock_into, urgency_band};
pub use clock::{TideClock, update_tide_clock};
use field_guide::spawn_field_guide;
pub use field_guide::{field_guide_visibility, update_field_guide};
pub(crate) use text::{event_name, puzzle_status};

#[derive(Component)]
pub struct LevelLabel;

#[derive(Component)]
pub struct PostsLabel;

#[derive(Component)]
pub struct PromptLabel;

/// The share of the window the prompt's pill may take, padding and all.
///
/// Set on the pill as a share of the viewport (`Val::Vw`) rather than of
/// its parent (`Val::Percent`): Bevy's text measure resolves a percentage
/// maximum against the room the layout hands the text, which is already
/// this share less the padding, so a percentage was taken twice and the
/// pill wrapped at about three quarters of the window, not this.
const PROMPT_SHARE: f32 = 0.88;
/// The pill's padding either side of the text.
const PROMPT_PAD_X: f32 = 12.0;
/// The smallest the prompt shrinks to on the play screens: the fine print.
const PROMPT_MIN_PX: f32 = menu_ui::type_scale::FINE;

/// How wide a line of the prompt may run before it wraps, in a window
/// `width` interface pixels wide: the pill's share of it, less the padding.
fn prompt_room(width: f32) -> f32 {
    width * PROMPT_SHARE - 2.0 * PROMPT_PAD_X
}

/// How the prompt line is set on a screen, which is what the room above
/// the foot of that screen allows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PromptBand {
    /// The crab legend sits just above it: one line, shrunk as far as the
    /// fine print to stay one ([`prompt_fit`]), so it never wraps up over
    /// the legend.
    Shrinks,
    /// One line at full size. The cards stop [`menu_ui::BAR_H`] above the
    /// foot, which clears one row of the pill and not two, and a replay's
    /// transport bar stands where a second row would.
    OneRow,
    /// Up to two lines: the screen keeps a second row clear for the
    /// wordier languages. The lobby stops its chat line and panels above
    /// two rows (`lobby::ui`), and the editor's board stops where every
    /// board screen's does, above two rows (`Screen::chrome`).
    TwoRows,
}

/// How the prompt is set on `screen`, `replaying` whether a recording is
/// playing on it. One answer for the system that sets the line and the
/// tests that measure every line against it.
pub(crate) fn prompt_band(screen: Screen, replaying: bool) -> PromptBand {
    if field_guide::field_guide_shown(screen, replaying) {
        return PromptBand::Shrinks;
    }
    match screen {
        Screen::Lobby | Screen::Editor => PromptBand::TwoRows,
        Screen::Menu
        | Screen::Puzzle
        | Screen::Versus
        | Screen::Settings
        | Screen::Controls
        | Screen::MatchSetup
        | Screen::Achievements
        | Screen::StageSelect
        | Screen::Replays
        | Screen::Interlude
        | Screen::Language
        | Screen::NewVersion => PromptBand::OneRow,
    }
}

/// The prompt as `band` sets it. On a [`PromptBand::TwoRows`] screen the
/// line may break only between two keys: after each ` | `, and between
/// the entries of a list of picks, which are set two spaces apart. Every
/// other space is made non-breaking, so a key is never parted from what
/// it does ("| K" over "Möwen") and no row starts with the separator. The
/// shaper also breaks after a `/` or a `-` before a letter ("LB/" over
/// "RB: Seite"), so a word joiner, which draws as nothing, holds each to
/// what follows. Every other band sets the line on one row, as it is.
pub(crate) fn banded(band: PromptBand, line: &str) -> String {
    use crate::app::i18n::WORD_JOINER;
    const GLUE: &str = "\u{a0}";
    let glued = |words: &str| {
        words
            .replace(' ', GLUE)
            .replace('/', &format!("/{WORD_JOINER}"))
            .replace('-', &format!("-{WORD_JOINER}"))
    };
    match band {
        PromptBand::TwoRows => line
            .split(" | ")
            .map(|part| part.split("  ").map(glued).collect::<Vec<_>>().join("  "))
            .collect::<Vec<_>>()
            .join(&format!("{GLUE}| ")),
        PromptBand::Shrinks | PromptBand::OneRow => line.to_string(),
    }
}

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

/// The rows `line` is set in, in a window `width` interface pixels wide,
/// at [`HEADER_PX`] in the display face, or `None` when a piece of it is
/// wider than a row on its own and runs on past the pill.
///
/// Broken where the shaper breaks a line, a piece at a time, as the pill
/// wraps it: at a space, and after a `/` or a `-` with a letter after it.
/// A non-breaking space and a word joiner are no break, so a line
/// [`banded`] for two rows is broken only between its keys. Not between
/// two kana, where the shaper could also break, so a Japanese line is
/// never counted shorter than it is.
#[cfg(test)]
fn prompt_lines(line: &str, width: f32) -> Option<Vec<String>> {
    use crate::app::i18n::metrics::display_px;
    let room = prompt_room(width);
    // Each piece, and what joins it to the one before: a space, or
    // nothing after a `/` or a `-`.
    let mut pieces: Vec<(&str, String)> = Vec::new();
    let (mut piece, mut joiner) = (String::new(), "");
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            ' ' => {
                pieces.push((joiner, std::mem::take(&mut piece)));
                joiner = " ";
            }
            '/' | '-' if chars.peek().is_some_and(|next| next.is_alphabetic()) => {
                piece.push(ch);
                pieces.push((joiner, std::mem::take(&mut piece)));
                joiner = "";
            }
            _ => piece.push(ch),
        }
    }
    pieces.push((joiner, piece));
    let mut rows = vec![String::new()];
    for (joiner, piece) in pieces {
        let row = rows.last_mut().expect("a row");
        let longer = match row.is_empty() {
            true => piece.clone(),
            false => format!("{row}{joiner}{piece}"),
        };
        if row.is_empty() || display_px(&longer, HEADER_PX) <= room {
            *row = longer;
        } else {
            rows.push(piece);
        }
        if display_px(rows.last().expect("a row"), HEADER_PX) > room {
            return None;
        }
    }
    Some(rows)
}

/// How many rows `line` takes ([`prompt_lines`]), `usize::MAX` for a line
/// that runs on past the pill.
#[cfg(test)]
fn prompt_rows(line: &str, width: f32) -> usize {
    prompt_lines(line, width).map_or(usize::MAX, |rows| rows.len())
}

/// Whether `line` fits the prompt as `band` sets it, at the narrowest the
/// interface ever is ([`crate::app::settings::DESIGN_W`]): the window is
/// fitted to the design by its tighter side, so it is never less wide.
/// `Err` says how it overruns.
#[cfg(test)]
pub(crate) fn prompt_fits(band: PromptBand, line: &str) -> Result<(), String> {
    use crate::app::i18n::metrics::display_px;
    let width = crate::app::settings::DESIGN_W;
    let room = prompt_room(width);
    let line = &banded(band, line);
    let natural = display_px(line, HEADER_PX);
    let fits = match band {
        PromptBand::Shrinks => prompt_fit(natural, room).is_some(),
        PromptBand::OneRow => natural <= room,
        PromptBand::TwoRows => prompt_rows(line, width) <= 2,
    };
    match fits {
        true => Ok(()),
        false => Err(format!(
            "{natural:.0}px in {band:?} ({room:.0}px a row{})",
            match band {
                PromptBand::Shrinks => format!(
                    ", {:.0}px at the fine print",
                    room * HEADER_PX / PROMPT_MIN_PX
                ),
                PromptBand::OneRow | PromptBand::TwoRows => String::new(),
            }
        )),
    }
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
/// just above it ([`PromptBand::Shrinks`]).
///
/// The prompt sits at the foot and wraps upward when it is long, into a
/// second row the chrome once kept for it. The legend strip took that row,
/// so the two-seat prompt at 720p ran its second line over the legend. On
/// those screens it now stays one line and shrinks until it fits, which
/// text width makes a single step: it scales with the font size. Anywhere
/// else it wraps at full size, which only the [`PromptBand::TwoRows`]
/// screens have room for. A prompt too long even at the fine print's size
/// wraps too, since running off the window is worse than the overlap.
///
/// Reads last frame's layout, so a new prompt is one frame at the old
/// size before it settles.
pub fn fit_prompt(
    viewport: menu_ui::Viewport,
    screen: Res<State<Screen>>,
    playback: Res<crate::app::Playback>,
    mut prompts: Query<PromptText, With<PromptLabel>>,
    // The line given up on: too long even at the fine print, and wrapped.
    // Held until the words change, or measuring the wrapped pill as if it
    // were the line would unwrap it and give up again, every other frame.
    mut too_long: Local<Option<String>>,
) {
    let legend_up = prompt_band(*screen.get(), playback.0.is_some()) == PromptBand::Shrinks;
    let Some(size) = viewport.size() else {
        return;
    };
    let room = prompt_room(size.x);
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
    sim: Res<Sim>,
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
            // `header_backdrop` shades it on its first frame.
            BackgroundGradient::default(),
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
            // The status in a clipping box of its own, for the title's
            // reason: on one line, and cut at the bar's end rather than
            // wrapped down over the board. It gives way to the title, which
            // does not shrink, and to the score chips.
            bar.spawn(Node {
                overflow: Overflow::clip_x(),
                flex_shrink: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            })
            .with_children(|box_| {
                box_.spawn((
                    PostsLabel,
                    Text::new(""),
                    font.clone(),
                    TextColor(palette::HUD_INK),
                    TextLayout::no_wrap(),
                ));
            });
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
            // other end of this strip, and a long prompt wraps rather than
            // running under it. Which screens have room for that second
            // row is `prompt_band`'s to say.
            max_width: Val::Vw(PROMPT_SHARE * 100.0),
            padding: UiRect::axes(Val::Px(PROMPT_PAD_X), Val::Px(5.0)),
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
            // On a pill, as the prompt is: pale text straight on the sand
            // is unreadable.
            wrap.spawn((
                HintLabel,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(menu_ui::type_scale::ROW),
                    ..default()
                },
                TextColor(palette::SELECTED_ROW.with_alpha(0.85)),
                Node {
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(4.0)),
                    border_radius: BorderRadius::all(Val::Px(11.0)),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(palette::PILL_FILL),
            ));
        });

    // The tide clock: big, top-centre, red for the final scramble. A
    // full-width centring wrapper keeps the digits centred at any width.
    // `ZIndex(1)` lifts it above the header bar it overlaps: the bar is
    // spawned first, its shade nearly opaque behind the digits, and without
    // this they paint behind it.
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
    spawn_field_guide(&mut commands, settings.tr(), &art, &sim.0);
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

/// What the player has run into on this level: how stuck they are, and
/// the last placement refused.
#[derive(bevy::ecs::system::SystemParam)]
pub struct HintNotes<'w> {
    stuck: Res<'w, crate::app::hint::Hints>,
    denied: Res<'w, crate::app::hint::DeniedNote>,
}

/// The level's own lesson and what decides how it reads: the list it is
/// on, whether two play it ([`crate::app::Coop`]), the settings, this
/// keyboard's caps, and the hands last reached with.
#[derive(bevy::ecs::system::SystemParam)]
pub struct HintLesson<'w> {
    campaign: Res<'w, Campaign>,
    coop: Res<'w, crate::app::Coop>,
    settings: Res<'w, GameSettings>,
    caps: Res<'w, crate::app::keycaps::KeyCaps>,
    device: Res<'w, crate::app::gamepad::LastDevice>,
    face: Res<'w, crate::app::gamepad::PadFace>,
}

impl HintLesson<'_> {
    /// The current level's lesson as this player may be shown it, in this
    /// keyboard's key names. The key lesson is kept back under keys it
    /// does not describe, and the prompt line already points at Settings.
    fn shown(&self) -> Option<String> {
        let campaign = &self.campaign;
        let hint = campaign.shown_hint(campaign.index, &self.settings, *self.coop, *self.device)?;
        Some(self.caps.legend(&hint))
    }
}

/// Show a level's teaching hint while its signposts are being placed.
pub fn update_hint(
    screen: Res<State<Screen>>,
    phase: Res<State<Phase>>,
    lesson: HintLesson,
    notes: HintNotes,
    mut hints: Query<(&mut Text, &mut Node), With<HintLabel>>,
) {
    let HintNotes { stuck, denied } = notes;
    let line = if *screen.get() != Screen::Puzzle {
        String::new()
    } else {
        // A stuck player is told about the hint instead of being told the
        // lesson again; the lesson is what they have already tried.
        let tr = lesson.settings.tr();
        let setup = *phase.get() == Phase::Setup;
        // A pad's player is told the pad's button for the hint, as the
        // face's table has it.
        let button = match *lesson.device {
            crate::app::gamepad::LastDevice::Pad(family) => {
                crate::app::gamepad::scheme::button_name(
                    &lesson.face.0,
                    crate::app::gamepad::scheme::Does::Hint,
                    tr,
                    family,
                )
            }
            crate::app::gamepad::LastDevice::Keyboard => None,
        };
        crate::app::hint::hint_line(tr, &stuck, &denied, phase.get(), button.as_deref())
            .or_else(|| setup.then(|| lesson.shown()).flatten())
            .unwrap_or_default()
    };
    for (mut text, mut node) in &mut hints {
        menu_ui::set_text(&mut text, &line);
        // No pill with nothing on it.
        menu_ui::set_shown(&mut node, !line.is_empty());
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
    table: Res<'w, crate::app::rematch::LocalTable>,
    keep: Res<'w, crate::app::replays::RoundToKeep>,
    /// What a host's map page's cursor is on is picked by these.
    config: Res<'w, crate::app::match_setup::MatchConfig>,
    beaches: Res<'w, crate::app::match_setup::CustomBeaches>,
}

/// Everything the header, the status slot and the prompt are written from:
/// the sources of a [`text::Readout`], read together every frame.
#[derive(bevy::ecs::system::SystemParam)]
pub struct HudSources<'w> {
    modes: Modes<'w>,
    seating: crate::app::seating::Seating<'w>,
    sim: Res<'w, Sim>,
    campaign: Res<'w, Campaign>,
    settings: Res<'w, GameSettings>,
    keycaps: Res<'w, crate::app::keycaps::KeyCaps>,
    library: Res<'w, crate::app::replays::Library>,
    notice: Res<'w, crate::app::RoundNotice>,
    speed: Res<'w, crate::app::replays::PlaybackSpeed>,
    tournament: Res<'w, crate::app::tournament::Tournament>,
    spectators: Res<'w, crate::app::spectators::SpectatorChat>,
    pad: PadSources<'w>,
}

/// What the controller prompt is written from: the face whose table is in
/// force, the hands last used, and the pad's pick.
#[derive(bevy::ecs::system::SystemParam)]
pub struct PadSources<'w> {
    face: Res<'w, crate::app::gamepad::PadFace>,
    device: Res<'w, crate::app::gamepad::LastDevice>,
    pick: Res<'w, crate::app::gamepad::PadPick>,
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
            pad,
        } = self;
        text::Readout {
            tr: settings.tr(),
            lang: settings.language,
            sim,
            campaign,
            coop: modes.coop.0,
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
            // Any card, a spectator's own included: it stops nothing,
            // but it holds the keyboard all the same, and the line that
            // named the beach's keys over it named keys that were dead.
            paused: modes.pause_menu.is_open(),
            spectator_typing: spectators.0.as_deref(),
            crowd: seating
                .online
                .0
                .as_ref()
                .map_or_else(Default::default, |session| session.stands.tide.crowd),
            speed: speed.0,
            face: &pad.face.0,
            device: *pad.device,
            pick: *pad.pick,
            rematch: crate::app::rematch::rematch(&modes.table, &seating.online, tournament),
            keep: modes.keep.offer(),
            map_pick: crate::app::match_setup::card_to_pick(
                &modes.match_menu,
                &modes.config,
                &modes.beaches,
            )
            .is_some(),
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
        let band = prompt_band(*screen, sources.seating.playback.0.is_some());
        menu_ui::set_text(&mut text, &banded(band, &said.prompt));
        // An empty prompt keeps its pill off the sand: a bare dark lozenge
        // in the corner reads as a broken widget, not as quiet.
        menu_ui::set_shown(&mut node, !said.prompt.is_empty());
    }
}

/// The menu is a full-bleed postcard: the header backdrop gets out of the
/// way there and returns on every other screen.
///
/// Off the menu it is a shade falling away down the bar rather than a solid
/// strip: dark enough behind the title to read, and letting the beach run
/// up to the top of the window instead of ending at a black band.
///
/// Asked of each bar rather than remembered across frames: a bar spawned
/// after the screen last changed (a round entered straight from launch)
/// was born unshaded and stayed that way.
pub fn header_backdrop(
    screen: Res<State<Screen>>,
    mut bars: Query<&mut BackgroundGradient, With<HeaderBar>>,
) {
    let shaded = *screen.get() != Screen::Menu;
    for mut bg in &mut bars {
        if bg.0.is_empty() == shaded {
            *bg = if shaded {
                header_shade()
            } else {
                BackgroundGradient::default()
            };
        }
    }
}

/// The header's shade: nearly the bar's whole fill down past the foot of
/// its text, then gone by the bar's foot. Held dark that far for the
/// puzzle clock, whose digits reach four fifths of the way down: a shade
/// that thinned from halfway left the foot of a red closing-stretch clock
/// about as bright as the sand under it.
fn header_shade() -> BackgroundGradient {
    BackgroundGradient::from(LinearGradient::to_bottom(vec![
        ColorStop::percent(palette::HEADER_FILL.with_alpha(0.92), 0.0),
        ColorStop::percent(palette::HEADER_FILL.with_alpha(0.88), 80.0),
        ColorStop::percent(palette::HEADER_FILL.with_alpha(0.0), 100.0),
    ]))
}

/// How long a running round shows the controls line before it fades, and
/// how long the fade takes, in seconds.
const PROMPT_LINGER: f32 = 6.0;
const PROMPT_FADE: f32 = 0.8;

/// What decides whether the controls line may fade: a versus round under
/// way, not held by any pause (an online one included), with no card up,
/// for somebody holding a seat in it.
#[derive(bevy::ecs::system::SystemParam)]
pub struct PromptRound<'w> {
    round: crate::app::round_state::RoundState<'w>,
    pause_menu: Res<'w, crate::app::pause::PauseMenu>,
    online: Res<'w, crate::app::net::Online>,
    playback: Res<'w, crate::app::Playback>,
}

impl PromptRound<'_> {
    fn running(&self) -> bool {
        // Only for a seat: a spectator's line is their chat draft and the
        // crowd's vote, and a replay's is its transport keys, none of
        // which anyone has read once and for all.
        let seated = matches!(
            text::header_viewer(&self.online, &self.playback),
            crate::app::seating::Viewer::Peer(_) | crate::app::seating::Viewer::Table
        );
        // Any card, not only one that stops the round: a spectator's own
        // card holds the keyboard all the same.
        self.round.versus_running() && !self.round.held() && !self.pause_menu.is_open() && seated
    }
}

/// Let the controls line fade once a versus round is under way. Everyone
/// at the table has read it by then, and it is the biggest piece of text
/// on the screen, over the beach at the foot of the board.
///
/// It comes back whenever the round is not running (the countdown, a
/// pause, the end) and for a fresh stretch whenever what it says changes.
/// Only versus, and only for a seat (see [`PromptRound`]): a puzzle's line
/// is its state ("Running... R to reset") as much as its keys, and stays.
pub fn fade_prompt(
    time: Res<Time>,
    round: PromptRound,
    // How long the line has shown in a running round, and what it said.
    mut stretch: Local<(f32, String)>,
    mut prompts: Query<(&Text, &mut TextColor, &mut BackgroundColor), With<PromptLabel>>,
) {
    let running = round.running();
    let (shown_for, said) = &mut *stretch;
    for (text, mut color, mut bg) in &mut prompts {
        if !running || text.0 != *said {
            *shown_for = 0.0;
            said.clone_from(&text.0);
        } else {
            *shown_for += time.delta_secs();
        }
        let shown = prompt_shown(*shown_for);
        menu_ui::set_color(&mut color, palette::SELECTED_ROW.with_alpha(shown));
        menu_ui::set_bg(
            &mut bg,
            palette::PILL_FILL.with_alpha(palette::PILL_FILL.alpha() * shown),
        );
    }
}

/// How much of the controls line shows, `secs` into a stretch of running
/// round: all of it until it has lingered, then easing out to none.
fn prompt_shown(secs: f32) -> f32 {
    let t = ((secs - PROMPT_LINGER) / PROMPT_FADE).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The controls line stays whole while it lingers, then fades to
    /// nothing and stays gone, and never goes past either end.
    #[test]
    fn the_controls_line_lingers_then_fades_out() {
        assert_eq!(prompt_shown(0.0), 1.0);
        assert_eq!(prompt_shown(PROMPT_LINGER), 1.0);
        let midway = prompt_shown(PROMPT_LINGER + PROMPT_FADE / 2.0);
        assert!((0.1..0.9).contains(&midway), "midway through: {midway}");
        assert_eq!(prompt_shown(PROMPT_LINGER + PROMPT_FADE), 0.0);
        assert_eq!(prompt_shown(600.0), 0.0);
    }

    /// A peer's pause holds the round without setting `Paused` or raising
    /// a card here, and the controls line stops fading under it all the
    /// same: the line reads the one pause rule, online pauses included.
    #[test]
    fn the_controls_line_holds_under_a_peers_pause() {
        use crate::app::net::{Online, OnlineSession};
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.insert_state(Screen::Versus);
        app.insert_state(VersusPhase::Running);
        app.init_resource::<crate::app::Paused>();
        app.init_resource::<crate::app::pause::PauseMenu>();
        app.init_resource::<crate::app::Playback>();
        let session = OnlineSession::new(
            crate::transport::UdpTransport::host(0).expect("ephemeral port"),
            crate::sim::Lockstep::new(0, vec![0, 1], crate::sim::DEFAULT_DELAY),
            2,
            crate::transport::MatchTerms::default(),
        );
        app.insert_resource(Online(Some(session)));
        app.update();
        let running = |app: &mut App| {
            app.world_mut()
                .run_system_once(|round: PromptRound| round.running())
                .expect("runs")
        };
        assert!(running(&mut app), "a seat's running round fades the line");
        app.world_mut()
            .resource_mut::<Online>()
            .0
            .as_mut()
            .expect("a session")
            .session
            .receive_pause(5);
        assert!(!running(&mut app), "a peer's pause holds it");
    }

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

    /// The bands measure the pill the way it wraps: the longest line of
    /// words that stays inside the room is one row, one word more is two,
    /// which only the screens that keep a second row clear can take, and
    /// which the legend's screens shrink to one.
    #[test]
    fn a_line_past_the_room_takes_a_second_row() {
        use crate::app::i18n::metrics::display_px;
        let room = prompt_room(crate::app::settings::DESIGN_W);
        let mut line = "Esc".to_string();
        while display_px(&format!("{line} | Esc"), HEADER_PX) <= room {
            line.push_str(" | Esc");
        }
        assert_eq!(prompt_fits(PromptBand::OneRow, &line), Ok(()));
        let over = format!("{line} | Esc");
        assert!(prompt_fits(PromptBand::OneRow, &over).is_err(), "{over}");
        assert_eq!(prompt_rows(&over, crate::app::settings::DESIGN_W), 2);
        assert_eq!(prompt_fits(PromptBand::TwoRows, &over), Ok(()));
        assert_eq!(prompt_fits(PromptBand::Shrinks, &over), Ok(()));
        let three = format!("{line} | {line} | {line}");
        assert!(prompt_fits(PromptBand::TwoRows, &three).is_err(), "{three}");
    }

    /// Two rows break only between keys: every breakable space of a line
    /// set for them follows a separator, so a key is never parted from
    /// what it does and no row starts with the separator. One-row bands
    /// are set as they are.
    #[test]
    fn two_rows_break_only_between_keys() {
        let line = "WASD: bewegen | K: Möwen setzen | Esc: Menü";
        let set = banded(PromptBand::TwoRows, line);
        let plain = |set: &str| set.replace('\u{a0}', " ").replace('\u{2060}', "");
        assert_eq!(plain(&set), line, "the same words");
        let breaks: Vec<usize> = set.match_indices(' ').map(|(at, _)| at).collect();
        assert_eq!(breaks.len(), 2, "{set:?}");
        for at in breaks {
            assert!(set[..at].ends_with('|'), "a break inside a key: {set:?}");
        }
        // A list of picks breaks between its entries, never inside one.
        let picks = banded(PromptBand::TwoRows, "Esc: zurück | 7. Bo Li  8. Anna");
        assert_eq!(
            picks,
            "Esc:\u{a0}zurück\u{a0}| 7.\u{a0}Bo\u{a0}Li  8.\u{a0}Anna"
        );
        assert_eq!(banded(PromptBand::OneRow, line), line);
        assert_eq!(banded(PromptBand::Shrinks, line), line);
        // Measured as it is set: a first row of keys with room left for
        // "| K:" and not for "Möwen" takes the whole key to the next row,
        // and one with room for "| LB/" and not for "RB:" takes the whole
        // pair. Set as written, each would have been parted.
        let width = crate::app::settings::DESIGN_W;
        let room = prompt_room(width);
        let px = |text: &str| crate::app::i18n::metrics::display_px(text, HEADER_PX);
        for (last, cut) in [("K: Möwen", "K:"), ("LB/RB: Seite", "LB/")] {
            let mut keys = "Esc: Menü".to_string();
            while px(&format!("{keys} | Esc: Menü | {cut}")) <= room {
                keys.push_str(" | Esc: Menü");
            }
            // Then the last key's words a letter longer at a time, to
            // within a letter of the room the cut needs.
            while px(&format!("{keys}m | {cut}")) <= room {
                keys.push('m');
            }
            let split = format!("{keys} | {last}");
            assert!(px(&format!("{keys} | {cut}")) <= room, "room for {cut:?}");
            assert!(px(&split) > room, "no room for {last:?}");
            let naive = prompt_lines(&split, width).expect("fits two rows");
            assert!(
                naive[0].ends_with(cut),
                "set as written, {cut:?} ends the first row: {naive:?}"
            );
            let set =
                prompt_lines(&banded(PromptBand::TwoRows, &split), width).expect("fits two rows");
            assert_eq!(set.len(), 2, "{set:?}");
            assert_eq!(plain(&set[1]), last, "the whole key moves down: {set:?}");
        }
    }

    /// Where the prompt may wrap: over the crab legend never, so it
    /// shrinks; under a replay's transport and the cards not at all; and
    /// to a second row where the lobby and the editor keep one clear.
    #[test]
    fn the_prompt_wraps_only_where_a_second_row_is_kept() {
        assert_eq!(prompt_band(Screen::Versus, false), PromptBand::Shrinks);
        assert_eq!(prompt_band(Screen::Puzzle, false), PromptBand::Shrinks);
        assert_eq!(prompt_band(Screen::Versus, true), PromptBand::OneRow);
        assert_eq!(prompt_band(Screen::MatchSetup, false), PromptBand::OneRow);
        assert_eq!(prompt_band(Screen::Replays, false), PromptBand::OneRow);
        assert_eq!(prompt_band(Screen::Lobby, false), PromptBand::TwoRows);
        assert_eq!(prompt_band(Screen::Editor, false), PromptBand::TwoRows);
    }

    /// What a co-op stage says in the header fits beside the title it is
    /// said under, in every language: the stage list's refusal of a co-op
    /// stage with nobody seated as P2 beside the list's title, both hands'
    /// count and the tally beside a co-op stage's title at its longest
    /// name, and the editor's check of a co-op stage, every arrow named by
    /// its hand and led by the verdict on whether it needs both players,
    /// which is what must not clip (the arrows after it may, being drawn
    /// on the bench).
    #[test]
    fn the_coop_lines_fit_beside_their_titles() {
        use crate::app::i18n::fill;
        use crate::app::i18n::metrics::display_px;
        use crate::app::settings::DESIGN_W;
        let room = DESIGN_W - 2.0 * 12.0 - 18.0;
        let mut over = Vec::new();
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            let pair = |line: &str| crate::app::name_the_pair(tr, line);
            let shelf = crate::sim::coop_levels();
            let longest = shelf
                .iter()
                .map(|level| lang.level_name(&level.name).to_string())
                .max_by(|a, b| display_px(a, HEADER_PX).total_cmp(&display_px(b, HEADER_PX)))
                .unwrap_or_default();
            let of = shelf.len();
            let stage = format!("{} {of}/{of} - {longest}", tr.stage_coop);
            let count = fill(
                &pair(tr.hands_count),
                &[("a", "9"), ("b", "9"), ("c", "9"), ("d", "9")],
            );
            let saved = fill(tr.saved_count, &[("a", "12"), ("b", "12")]);
            let list = format!("{} - {}", tr.title_tide_pool, tr.title_stages);
            let editor = format!(
                "{} [{}] - {} 2",
                tr.title_editor, tr.ed_kind_puzzle, tr.ed_default_name
            );
            for (title, msg) in [
                (list.clone(), pair(tr.stage_needs_two)),
                (list, pair(tr.stage_coop_on)),
                (stage, format!("{count} | {saved}")),
                (editor.clone(), format!("{} - ", tr.ed_needs_both)),
                (editor, format!("{} - ", tr.ed_one_alone)),
            ] {
                let used = display_px(&title, HEADER_PX) + display_px(&msg, HEADER_PX);
                if used > room {
                    over.push(format!("{lang:?}: {used:.0}px: {title} | {msg}"));
                }
            }
            // The stage list's caption and hint pills under the grid, at
            // their sizes (`stage_select`), across the window less the
            // pill's padding.
            // Every stage's own caption, at the last place on the shelf
            // and locked behind the one before it: the font these are
            // measured in is not the one `longest` was picked by.
            let open = fill(&pair(tr.stage_open_hands), &[("a", "9"), ("b", "9")]);
            let before = (of - 1).to_string();
            let width = DESIGN_W - 2.0 * 12.0;
            let mut lines = vec![(pair(tr.coop_rule), 17.0)];
            for level in &shelf {
                let name = lang.level_name(&level.name);
                lines.push((format!("{} {of}. {name} - {open}", tr.stage_coop), 20.0));
                lines.push((
                    format!(
                        "{} {of}. {name} - {}",
                        tr.stage_coop,
                        fill(tr.stage_locked_coop, &[("n", before.as_str())])
                    ),
                    20.0,
                ));
            }
            for (line, px) in lines {
                let used = crate::app::i18n::metrics::text_px(&line, px);
                if used > width {
                    over.push(format!("{lang:?}: {used:.0}px: {line}"));
                }
            }
        }
        assert!(over.is_empty(), "too long:\n{}", over.join("\n"));
    }

    /// The editor's status line fits beside its title on one line, in every
    /// language, for the lines a save and a check say: the file a level
    /// was saved as, with the longest name a level may have, a found
    /// solution of four signposts, F2's question over another level, and
    /// each of a save's warnings, which lead the line they are on. It used to name the whole path, which ran
    /// off the header in every language. The box it is drawn in clips what
    /// is still longer (a save's warnings) rather than wrapping it down over
    /// the board.
    #[test]
    fn the_editors_status_fits_beside_its_title() {
        use crate::app::i18n::fill;
        use crate::app::i18n::metrics::display_px;
        use crate::app::settings::DESIGN_W;
        // The bar's padding on both sides and its gap between the two.
        let room = DESIGN_W - 2.0 * 12.0 - 18.0;
        // As long as a level's name may be (`editor::NAME_MAX`), in the
        // letters a file name keeps.
        let stem = "the-long-gull-alley-at-dusk2";
        assert_eq!(stem.chars().count(), 28);
        let mut over = Vec::new();
        for lang in crate::app::i18n::ALL_LANGS {
            let tr = lang.tr();
            let title = format!(
                "{} [{}] - {} 2",
                tr.title_editor, tr.ed_kind_puzzle, tr.ed_default_name
            );
            let saved = fill(tr.ed_saved_to, &[("file", &format!("{stem}.txt"))]);
            let solved = fill(
                tr.ed_solvable,
                &[
                    ("n", "4"),
                    ("g", "9"),
                    ("placements", "(10,8) (11,2) (3,12) (19,12)"),
                ],
            );
            // F2's question, naming the level it would replace, from the
            // keyboard and from a pad.
            let fresh = format!("{} 2", tr.ed_default_name);
            let asked = fill(tr.ed_save_taken, &[("name", &fresh)]);
            let save = crate::app::gamepad::scheme::button_name(
                &crate::app::gamepad::Face::Editor { layer: true },
                crate::app::gamepad::scheme::Does::Save,
                tr,
                crate::app::gamepad::PadFamily::Xbox,
            )
            .expect("a button saves");
            let pad = fill(tr.pad.save_taken, &[("name", &fresh), ("b", &save)]);
            // A save's warning, which leads its line so that the file it
            // was saved to is what clips (`editor::shelf::save_level`).
            let warned = [
                tr.ed_not_solvable_needs,
                tr.ed_puzzle_no_castle,
                tr.ed_puzzle_no_crabs,
                tr.ed_arena_no_crabs,
                tr.ed_arena_skips_seat,
                tr.ed_arena_needs_seats,
            ]
            .map(|warning| fill(warning, &[("n", "6"), ("m", "9")]));
            // A beach's check that weighs its seats, at its longest.
            let seat = |n| crate::app::seat_label(tr, n);
            let weighed = [
                fill(
                    tr.ed_fair_uneven,
                    &[("p", &seat(5)), ("n", "188"), ("q", &seat(4)), ("m", "199")],
                ),
                fill(tr.ed_fair_cut_off, &[("p", &seat(5))]),
                // What the mirror refuses, and a resize that turns it off.
                tr.ed_mirror_line.to_string(),
                tr.ed_mirror_across.to_string(),
                fill(tr.ed_mirror_seats, &[("p", &seat(4)), ("q", &seat(5))]),
                format!(
                    "{} - {}",
                    tr.ed_mirror_moved,
                    fill(tr.ed_resized, &[("w", "12"), ("h", "9")])
                ),
            ];
            for msg in [saved, solved, asked, pad]
                .into_iter()
                .chain(warned)
                .chain(weighed)
            {
                let used = display_px(&title, HEADER_PX) + display_px(&msg, HEADER_PX);
                if used > room {
                    over.push(format!("{lang:?}: {used:.0}px: {title} | {msg}"));
                }
            }
        }
        assert!(over.is_empty(), "past {room:.0}px:\n{}", over.join("\n"));
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
                [tr.title_tide_pool, tr.stage_custom]
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
}
