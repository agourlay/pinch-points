//! Per-player tile cursors and keyboard input (spec §8.2, §8.3).
//!
//! Two-stage model: move a cursor, commit a direction. Out of the box
//! player 0 uses WASD + arrow keys and player 1 (versus) IJKL + numpad;
//! every one of those keys is rebindable in settings (see `binds`). Cursors
//! are tinted to the player's flag colour and shaped differently per player
//! for colour-vision accessibility (§8.3). Gamepads (§8.1) slot in here
//! once bevy_gilrs is enabled; the commit path is identical.

use crate::app::layout::{self, TILE};
use crate::app::palette;
use crate::app::settings::{CommitScheme, GameSettings};
use crate::app::{Screen, Sim};
use crate::sim::Direction;
use bevy::prelude::*;

#[derive(Component)]
pub struct Cursor {
    pub player: u8,
    pub x: u8,
    pub y: u8,
    /// Hold-to-repeat state for the keyboard (spec §8.4 wants the rates
    /// adjustable later). A pad keeps its own, see `gamepad::PadRepeat`.
    repeat: HoldRepeat,
    /// Seconds left of the red denied-placement flash.
    pub flash: f32,
}

impl Cursor {
    /// A cursor for `player`, parked at the origin until something centres
    /// it on a board.
    ///
    /// A constructor because the repeat timer is private to this module and
    /// two places build one: here, and the pad code's tests.
    pub(crate) fn seated(player: u8) -> Cursor {
        Cursor {
            player,
            x: 0,
            y: 0,
            repeat: HoldRepeat::default(),
            flash: 0.0,
        }
    }

    /// Step `dx, dy` tiles, stopping at the board's edge.
    pub(crate) fn step(&mut self, board: &crate::sim::Board, dx: i16, dy: i16) {
        self.x = (i16::from(self.x) + dx).clamp(0, i16::from(board.width()) - 1) as u8;
        self.y = (i16::from(self.y) + dy).clamp(0, i16::from(board.height()) - 1) as u8;
    }
}

/// Hold-to-repeat, for a key or a pad direction: one step on the press,
/// one more once the hold has lasted the delay, then one every interval.
///
/// One rule for both inputs, each keeping its own state (the cursor keeps
/// the keyboard's, a pad's component keeps the pad's), so a key and a
/// stick held at once do not share a timer.
#[derive(Default)]
pub(crate) struct HoldRepeat(Timer);

impl HoldRepeat {
    /// Whether to step this frame. `just` is a fresh press, `held` any
    /// press still down; `delay` and `interval` are seconds.
    ///
    /// The press itself sets the delay running, and each step after it the
    /// interval. Kept apart rather than read back off the timer: asking a
    /// finished Bevy timer whether its elapsed equals its duration says
    /// yes for every one, since Bevy clamps it, and the pad's hold once
    /// never got past the first-step delay that way.
    pub(crate) fn advance(
        &mut self,
        just: bool,
        held: bool,
        dt: std::time::Duration,
        delay: f32,
        interval: f32,
    ) -> bool {
        if just {
            self.0 = Timer::from_seconds(delay, TimerMode::Once);
            return true;
        }
        if !held {
            return false;
        }
        self.0.tick(dt);
        if !self.0.is_finished() {
            return false;
        }
        self.0 = Timer::from_seconds(interval, TimerMode::Once);
        true
    }
}

pub struct KeyMap {
    pub moves: [(KeyCode, i16, i16); 4],
    pub places: [(KeyCode, Direction); 4],
    pub remove: KeyCode,
    pub clear_all: KeyCode,
}

impl KeyMap {
    /// The four movement keys, in the map's order.
    pub fn move_keys(&self) -> [KeyCode; 4] {
        self.moves.map(|(key, ..)| key)
    }

    /// Every key this map reads: its moves, its placements, its removal
    /// and its clear-all. The keys are copied out, so the iterator does
    /// not borrow the map.
    pub fn keys(&self) -> impl Iterator<Item = KeyCode> + use<> {
        let Self {
            moves: _,
            places,
            remove,
            clear_all,
        } = self;
        self.move_keys()
            .into_iter()
            .chain(places.map(|(key, _)| key))
            .chain([*remove, *clear_all])
    }
}

/// How long a denied placement tints the cursor red.
pub(super) const FLASH_SECS: f32 = 0.25;

/// Whether a co-op partner has a cursor on the keyboard's second seat,
/// which sets the one-hand preset aside (see
/// [`GameSettings::commit_in_play`]).
pub fn has_partner<'a>(cursors: impl IntoIterator<Item = &'a Cursor>) -> bool {
    cursors.into_iter().any(|c| c.player == 1)
}

/// The keymap the cursor of `player` answers to, if any. Online the one
/// local cursor is on the primary keys whatever its seat; locally the
/// keyboard seats two, and seats 3 and up are driven by pads alone.
pub fn seat_keymap(
    settings: &GameSettings,
    player: u8,
    online: bool,
    commit: CommitScheme,
) -> Option<KeyMap> {
    if online {
        Some(keymap(settings, 0, commit))
    } else if player < 2 {
        Some(keymap(settings, player, commit))
    } else {
        None
    }
}

/// The keys the seats playing on this screen are bound to, which a
/// global letter yields to (see
/// [`crate::app::keycaps::KeyCaps::global_pressed`]). Kept by
/// [`track_live_seat_keys`] from the bindings, the screen, whether the
/// round is online and whether a co-op partner is at the keyboard; empty
/// on the menus, where no seat is read.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct LiveSeatKeys(Vec<KeyCode>);

impl LiveSeatKeys {
    /// The keys read for the seats playing on `screen`, as the seats'
    /// input reads them ([`keymap`]), under the commit scheme that input
    /// uses ([`GameSettings::commit_in_play`]).
    /// `cursors` are the cursors on the board: a co-op partner's is on the
    /// second seat's keys.
    pub fn of<'a>(
        settings: &GameSettings,
        screen: Screen,
        online: bool,
        cursors: impl IntoIterator<Item = &'a Cursor>,
    ) -> Self {
        let versus = screen == Screen::Versus;
        let partner = has_partner(cursors);
        let commit = settings.commit_in_play(versus || partner);
        let keys = live_seats(screen, online, partner)
            .iter()
            .flat_map(|&seat| keymap(settings, seat as u8, commit).keys());
        Self(keys.collect())
    }

    pub fn holds(&self, key: KeyCode) -> bool {
        self.0.contains(&key)
    }
}

/// The seats whose bound keys are being read on `screen`: P1 on a puzzle
/// and in the editor, with P2's keys too when a co-op `partner` is on
/// them, both keyboard seats in a local versus round, and P1's layout
/// alone online, where every player's hands are on it (see
/// `play_input::versus_input`). None on the menus, where a global letter
/// owes a seat nothing.
pub fn live_seats(screen: Screen, online: bool, partner: bool) -> &'static [usize] {
    match screen {
        Screen::Puzzle | Screen::Editor if partner => &[0, 1],
        Screen::Puzzle | Screen::Editor => &[0],
        Screen::Versus if online => &[0],
        Screen::Versus => &[0, 1],
        Screen::Menu
        | Screen::Lobby
        | Screen::Settings
        | Screen::Controls
        | Screen::MatchSetup
        | Screen::Achievements
        | Screen::StageSelect
        | Screen::Replays
        | Screen::Interlude
        | Screen::Language
        | Screen::NewVersion => &[],
    }
}

/// Keep [`LiveSeatKeys`] to this frame's screen, bindings and cursors.
pub fn track_live_seat_keys(
    settings: Res<GameSettings>,
    screen: Res<State<Screen>>,
    online: Res<crate::app::net::Online>,
    cursors: Query<&Cursor>,
    mut live: ResMut<LiveSeatKeys>,
) {
    live.set_if_neq(LiveSeatKeys::of(
        &settings,
        *screen.get(),
        online.0.is_some(),
        cursors,
    ));
}

/// Where the key that does `action` for `player` comes from under the
/// scheme `commit`: one of the seat's bound slots, or a key the scheme
/// fixes whatever is bound.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyDoing {
    /// The key bound to this slot of the seat's [`SeatBinds`].
    ///
    /// [`SeatBinds`]: crate::app::binds::SeatBinds
    Bound(crate::app::binds::Action),
    /// A key the scheme sets, which no rebinding moves.
    Preset(KeyCode),
}

/// Which key does `action` for `player` under `commit`: the one rule the
/// cursor ([`keymap`]) and the Controls screen both read, so the screen
/// lists and rebinds the key that really does each job.
///
/// Only seat 1 has a scheme. The mirrored scheme trades seat 1's move
/// slots and place slots ([`Action::mirrored`]), whatever is bound to
/// them: the same keys, so nobody else's. The one-hand preset puts the
/// four place jobs on IJKL whatever is bound; it exists for solo play,
/// where the second seat's keys are free, so a caller with the keyboard
/// shared asks [`GameSettings::commit_in_play`], which sets it aside.
///
/// [`Action::mirrored`]: crate::app::binds::Action::mirrored
pub fn key_doing(player: u8, commit: CommitScheme, action: crate::app::binds::Action) -> KeyDoing {
    use crate::app::binds::{Action, ActionGroup};
    let commit = if player == 0 {
        commit
    } else {
        CommitScheme::Arrows
    };
    match (commit, action.group()) {
        (CommitScheme::Arrows, _)
        | (_, ActionGroup::Posts)
        | (CommitScheme::Ijkl, ActionGroup::Move) => KeyDoing::Bound(action),
        (CommitScheme::Wasd, ActionGroup::Move | ActionGroup::Place) => {
            KeyDoing::Bound(action.mirrored())
        }
        (CommitScheme::Ijkl, ActionGroup::Place) => KeyDoing::Preset(match action {
            Action::PlaceDown => KeyCode::KeyK,
            Action::PlaceLeft => KeyCode::KeyJ,
            Action::PlaceRight => KeyCode::KeyL,
            Action::PlaceUp
            | Action::MoveUp
            | Action::MoveDown
            | Action::MoveLeft
            | Action::MoveRight
            | Action::Remove
            | Action::ClearAll => KeyCode::KeyI,
        }),
    }
}

/// The key that does `action` for `player` under `commit`, read through
/// [`key_doing`].
pub fn key_for(
    settings: &GameSettings,
    player: u8,
    commit: CommitScheme,
    action: crate::app::binds::Action,
) -> KeyCode {
    let seat = usize::from(player).min(crate::app::binds::BOUND_SEATS - 1);
    match key_doing(player, commit, action) {
        KeyDoing::Bound(slot) => settings.binds[seat].key(slot),
        KeyDoing::Preset(key) => key,
    }
}

/// Keyboard layout for a seat, as bound in settings (spec §8.2 defaults:
/// seat 1 on WASD + arrows, seat 2 on IJKL + numpad), under the scheme
/// `commit` as [`key_doing`] reads it.
pub fn keymap(settings: &GameSettings, player: u8, commit: CommitScheme) -> KeyMap {
    use crate::app::binds::Action;
    let key = |action| key_for(settings, player, commit, action);
    KeyMap {
        moves: [
            (key(Action::MoveUp), 0, -1),
            (key(Action::MoveDown), 0, 1),
            (key(Action::MoveLeft), -1, 0),
            (key(Action::MoveRight), 1, 0),
        ],
        places: [
            (key(Action::PlaceUp), Direction::Up),
            (key(Action::PlaceDown), Direction::Down),
            (key(Action::PlaceLeft), Direction::Left),
            (key(Action::PlaceRight), Direction::Right),
        ],
        remove: key(Action::Remove),
        clear_all: key(Action::ClearAll),
    }
}

/// Spawn one cursor per player. Shape differs per player (§8.3): player 0 is
/// an axis-aligned square, player 1 a diamond.
fn bracket_color(player: u8) -> Color {
    palette::seat_paint(player).with_alpha(0.9)
}

/// Whether `seat` is being played by somebody sitting at this machine.
///
/// A cursor is spawned for exactly the seats this keyboard, these pads and
/// this screen answer for: seat one alone in a puzzle, every human seat in
/// a local match, and precisely the local seat online. Bots have none, and
/// neither do rivals down a wire.
///
/// So it is the question "is this one mine?", which the beach asks before
/// it makes a noise: a knock for every post a bot puts down is a click
/// track, and hearing a rival's placements online is hearing something
/// that did not happen in this room.
///
/// A **replay** and a **spectated** match answer `false` for every seat,
/// because neither spawns a cursor at all. Which is why a post's knock and
/// its ring ask [`post_noticed_here`] rather than this.
pub fn seated_here(cursors: &Query<&Cursor>, seat: u8) -> bool {
    cursors.iter().any(|cursor| cursor.player == seat)
}

/// Whether a post going in, coming out or traded away at `owner`'s hand is
/// one this machine marks: the knock it makes and the ring and sand under
/// it. One rule for the sound and the picture, so the two cannot disagree
/// about whose beach this is.
///
/// [`seated_here`] reads correctly wherever this machine answers for a
/// seat: alone in a puzzle it is your seat, at a table every human seat
/// sharing the screen, online your seat and nobody else's.
///
/// Where this machine answers for *no* seat it says "nobody" and means it.
/// An online spectator and a replay both spawn no cursor at all (see
/// `spawn_versus_cursors`), so every post on the beach went in silent and
/// unmarked while the banks, the gulls and the horn played on. A machine
/// holding no seat is watching, and a watcher sees and hears the whole
/// beach.
pub fn post_noticed_here(cursors: &Query<&Cursor>, owner: u8) -> bool {
    cursors.is_empty() || seated_here(cursors, owner)
}

/// Tint flashing (denied) cursors red, and dim a cursor standing on a tile
/// that will refuse it.
///
/// A rock, a rival's post or an empty inventory is knowable before the
/// press, and a cursor that has gone quiet says so without a word; told
/// only afterwards, the brackets flash red and the player works out why.
/// Only where placing is the verb: in the editor the cursor paints tiles.
pub fn flash_cursors(
    time: Res<Time>,
    sim: Res<Sim>,
    screen: Res<State<Screen>>,
    mut cursors: Query<(&mut Cursor, &mut Sprite)>,
) {
    let board = &sim.0;
    let placing = matches!(screen.get(), Screen::Puzzle | Screen::Versus);
    for (mut cursor, mut sprite) in &mut cursors {
        if cursor.flash > 0.0 {
            cursor.flash -= time.delta_secs();
            layout::tint(&mut sprite, Color::srgba(1.0, 0.25, 0.2, 0.95));
            continue;
        }
        let color = bracket_color(cursor.player);
        // A board can shrink under a cursor (`glide_cursors` guards the
        // same way). Off the board both questions below answer "no" on
        // their bounds check, which reads as "this tile refuses you" and
        // leaves the brackets dimmed until something recentres them.
        if cursor.x >= board.width() || cursor.y >= board.height() {
            layout::tint(&mut sprite, color);
            continue;
        }
        // Only when it is the *tile* refusing. An empty inventory refuses
        // every tile on the beach, and a cursor dimmed everywhere reads as
        // broken rather than as a message; the arrow count in the header
        // says that one, and `out_of_signposts` tells the two apart.
        let seat = acts_for(*screen.get(), board, &cursor);
        let tile_refuses = placing
            && !board.can_place_signpost(seat, cursor.x, cursor.y)
            && !board.out_of_signposts(seat, cursor.x, cursor.y);
        let shown = if tile_refuses {
            color.with_alpha(0.34)
        } else {
            color
        };
        layout::tint(&mut sprite, shown);
    }
}

/// The post a seat has committed but the board has not taken yet.
///
/// Placing is two stages, move the cursor and commit a direction, and the
/// commit is instant, so there is nothing to preview. What there *is*,
/// between the press and the post, is a queue: a placement sits in
/// [`crate::app::PendingActions`] until the sim takes it. Locally that is
/// at most one fixed step; online it is until the lockstep commits the
/// frame, which on a stalled peer is as long as the stall.
#[derive(Component)]
pub struct PostGhost;

/// Draw one faint arrow per queued placement, the way a hint draws its own
/// ghost: the same art, the same near-transparency, under the real posts.
pub fn ghost_pending_posts(
    mut commands: Commands,
    sim: Res<Sim>,
    art: Res<crate::app::art::Art>,
    pending: Res<crate::app::PendingActions>,
    ghosts: Query<Entity, With<PostGhost>>,
) {
    // Rebuilt rather than diffed: there are at most six of them, and
    // `PendingActions` is taken mutably by the fixed-step driver every
    // tick, so change detection on it says nothing useful.
    for ghost in &ghosts {
        commands.entity(ghost).despawn();
    }
    let board = &sim.0;
    // Nothing is queued on a frozen board: `advance_sim` stops taking the
    // queue at the wave, so a placement pressed on the round's last frames
    // would otherwise leave its ghost on the held board for the whole of
    // the results card, advertising a post that will never go in.
    if board.round_over() {
        return;
    }
    for (seat, action) in pending.0.iter().enumerate() {
        let crate::sim::PlayerAction::Place { x, y, dir } = *action else {
            continue;
        };
        if x >= board.width() || y >= board.height() {
            continue;
        }
        // The board's outline, faint, and its arrow a little less so: the
        // post it will be, in the seat's colour, before it goes in.
        let color = palette::player_color(seat as u8).lighter(0.2);
        let size = Some(Vec2::splat(layout::SIGN_SIZE));
        commands
            .spawn((
                PostGhost,
                Sprite {
                    image: art.sign_shape.clone(),
                    color: color.with_alpha(0.22),
                    custom_size: size,
                    ..default()
                },
                Transform::from_translation(
                    layout::tile_center(board, x, y).extend(layout::z::SIGNPOST - 0.1),
                )
                .with_rotation(layout::dir_rotation(dir)),
            ))
            .with_child((
                Sprite {
                    image: art.sign_paint.clone(),
                    color: color.with_alpha(0.5),
                    custom_size: size,
                    ..default()
                },
                Transform::from_translation(Vec3::new(0.0, 0.0, 0.01)),
            ));
    }
}

/// Carry each cursor to the tile it now sits on, and breathe.
///
/// Teleported by whichever input system moved it, the cursor is correct to
/// the tile and wrong to the eye: at the hold-repeat rate it reads as a
/// thing being redrawn rather than steered. It is placed here instead,
/// once, from the tile the input systems agreed on.
///
/// Long jumps still snap. A round starting, a level loading and a board
/// changing size all move a cursor halfway across the beach, and sliding
/// it there would draw a line through a board it is no longer on.
pub fn glide_cursors(
    time: Res<Time>,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    mut cursors: Query<(&Cursor, &mut Transform)>,
) {
    let board = &sim.0;
    // Frame-rate independent: the same fraction of the way there per
    // second whatever the frame took.
    let closed = 1.0 - (-time.delta_secs() * 26.0).exp();
    let t = time.elapsed_secs();
    for (cursor, mut transform) in &mut cursors {
        if cursor.x >= board.width() || cursor.y >= board.height() {
            continue;
        }
        let target = layout::tile_center(board, cursor.x, cursor.y);
        let here = transform.translation.truncate();
        let at = if settings.reduced_motion || here.distance(target) > TILE * 2.0 {
            target
        } else {
            here.lerp(target, closed)
        };
        transform.translation = at.extend(layout::z::CURSOR);
        // A slow breath, phased per seat so six cursors on one beach do
        // not pulse as one.
        transform.scale = Vec3::splat(if settings.reduced_motion {
            1.0
        } else {
            1.0 + 0.035 * (t * 3.4 + f32::from(cursor.player)).sin()
        });
    }
}

/// One player's tile cursor: corner brackets in the owner's colour. Odd
/// seats rotate 45 degrees (a diamond) so overlapping cursors stay distinct.
pub(crate) fn cursor_sprite(commands: &mut Commands, art: &crate::app::art::Art, player: u8) {
    let rotation = if player % 2 == 1 {
        Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)
    } else {
        Quat::IDENTITY
    };
    commands.spawn((
        Cursor::seated(player),
        layout::image_sprite(
            &art.bracket,
            bracket_color(player),
            Vec2::splat(TILE * if player % 2 == 1 { 0.80 } else { 1.0 }),
        ),
        Transform::from_translation(Vec3::new(0.0, 0.0, layout::z::CURSOR)).with_rotation(rotation),
    ));
}

pub fn spawn_cursors(commands: &mut Commands, art: &crate::app::art::Art, players: u8) {
    for player in 0..players {
        cursor_sprite(commands, art, player);
    }
}

pub fn spawn_puzzle_cursor(mut commands: Commands, art: Res<crate::app::art::Art>) {
    spawn_cursors(&mut commands, &art, 1);
}

/// A puzzle's cursors: one, or two in co-op, where the second is a second
/// pair of hands for the same seat's arrows (see [`crate::app::Coop`]).
pub fn spawn_stage_cursors(
    mut commands: Commands,
    art: Res<crate::app::art::Art>,
    coop: Res<crate::app::Coop>,
) {
    let hands = if coop.0 { 2 } else { 1 };
    spawn_cursors(&mut commands, &art, hands);
}

/// The seat a cursor's placements belong to. A puzzle has one, whoever's
/// hands are on the cursor (co-op's second cursor on a Tide Pool stage
/// places the first seat's arrows), except a co-op stage, where each
/// cursor places its own hand ([`stage_seat`]).
pub fn acts_for(screen: Screen, board: &crate::sim::Board, cursor: &Cursor) -> u8 {
    if screen == Screen::Puzzle {
        stage_seat(board, cursor)
    } else {
        cursor.player
    }
}

/// The seat a cursor places for on a stage: its own on a co-op stage, where
/// P1's arrows turn the left claws and P2's the right
/// ([`crate::sim::Rules::claw_hands`]), and seat 0 on every other, where a
/// second cursor is a second pair of hands. Asked of the board, which is
/// what the arrows act on.
pub fn stage_seat(board: &crate::sim::Board, cursor: &Cursor) -> u8 {
    match board.rules().claw_hands {
        Some(_) => cursor.player,
        None => 0,
    }
}

/// Versus cursors: online spawns only the local seat's cursor (rivals'
/// placements arrive over the wire); local play seats two keyboard players
/// plus one per extra connected gamepad.
pub fn spawn_versus_cursors(
    mut commands: Commands,
    art: Res<crate::app::art::Art>,
    source: crate::app::session::RoundSource,
) {
    for seat in seats_played_here(&source) {
        cursor_sprite(&mut commands, &art, seat);
    }
}

/// The seats a versus round puts a cursor on at this machine: the one
/// answer the cursors are spawned by, and what tells a round one person
/// plays alone here from a couch's.
pub(crate) fn seats_played_here(source: &crate::app::session::RoundSource) -> Vec<u8> {
    let crate::app::session::RoundSource {
        online,
        playback,
        config,
        daily,
        resuming,
        pads,
        ..
    } = source;
    // Before `load_versus` takes the resumed round, which is why this can
    // still read it. Its table is the round's own: one cursor per human
    // seat, and none for the AI's, or a keypress on the AI's keys steered
    // its crabs over the head of the bot.
    //
    // By seat, not by head-count: the AI's seats need not be the top ones.
    // An online seat given up on mid-round passes to the AI where it sits,
    // and its code says so, so counting heads put a cursor on that seat
    // and left the human above it with none.
    if let Some(round) = &resuming.0 {
        return round.bots[..usize::from(round.seats)]
            .iter()
            .enumerate()
            .filter(|(_, bot)| bot.is_none())
            .map(|(seat, _)| seat as u8)
            .collect();
    }
    let daily_config = crate::app::match_setup::MatchConfig::daily();
    let config = if daily.active {
        &daily_config
    } else {
        &**config
    };
    // A replay has no cursor, for the same reason a spectator has none:
    // there is no seat here to place from. Cursor movement was never
    // recorded, so one drawn over a replay sits in the middle of the board
    // looking broken.
    if playback.0.is_some() {
        return Vec::new();
    }
    if online.0.is_none() && config.armed {
        // Configured match: one cursor per person's seat, where step 3
        // put them.
        return config.human_seats();
    }
    if let Some(session) = &online.0 {
        // A spectator has no cursor: there is no seat for it to place from.
        // Nor does a seat a bot drives from this machine: the keys here are
        // not what plays it.
        return session
            .session
            .seat()
            .filter(|_| session.bot.is_none())
            .into_iter()
            .collect();
    }
    (0..(pads.iter().count() as u8).max(2)).collect()
}

/// Snap every cursor to the board centre (used right after a mode spawns its
/// cursors over a fresh board).
pub fn center_cursors(sim: Res<Sim>, mut cursors: Query<(&mut Cursor, &mut Transform)>) {
    center_on(&sim.0, &mut cursors);
}

/// The centring itself, for callers that swap the board mid-screen (the
/// editor's resize and paste) and are not a system of their own. Movement
/// only clamps on a step, so a cursor left standing on a tile the new
/// board does not have would stay there until it was moved, and be
/// painted on in the meantime.
pub fn center_on(board: &crate::sim::Board, cursors: &mut Query<(&mut Cursor, &mut Transform)>) {
    for (mut cursor, mut transform) in cursors {
        cursor.x = board.width() / 2;
        cursor.y = board.height() / 2;
        transform.translation =
            layout::tile_center(board, cursor.x, cursor.y).extend(layout::z::CURSOR);
    }
}

/// Movement with hold-to-repeat, for every cursor; active in every phase.
pub fn move_cursor(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    screen: Res<State<Screen>>,
    online: Res<crate::app::net::Online>,
    mut cursors: Query<&mut Cursor>,
) {
    let board = &sim.0;
    let versus = *screen.get() == Screen::Versus;
    let commit = settings.commit_in_play(versus || has_partner(cursors.iter()));
    for mut cursor in &mut cursors {
        let Some(map) = seat_keymap(&settings, cursor.player, online.0.is_some(), commit) else {
            continue;
        };
        let any_just = map.moves.iter().any(|(k, ..)| keys.just_pressed(*k));
        let any_held = map.moves.iter().any(|(k, ..)| keys.pressed(*k));
        let step = cursor.repeat.advance(
            any_just,
            any_held,
            time.delta(),
            settings.repeat_delay,
            settings.repeat_interval,
        );
        if step {
            let (mut dx, mut dy) = (0i16, 0i16);
            for (key, kx, ky) in map.moves {
                if keys.pressed(key) {
                    dx += kx;
                    dy += ky;
                }
            }
            cursor.step(board, dx, dy);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::play_input::*;
    use crate::app::settings::GameSettings;
    use crate::app::{PendingActions, PlacementDenied};
    use crate::sim::PlayerAction;
    use crate::sim::{TileKind, classic_arena};
    use bevy::prelude::KeyCode;

    /// A board, a settings resource, and one cursor per seat: the smallest
    /// world the input systems will run in. No art, so no sprites: these
    /// check the decisions, not the drawing.
    fn beach(seats: u8) -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Versus));
        app.insert_resource(Sim(classic_arena(false, seats.max(2))));
        app.insert_resource(GameSettings::default());
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<Time>();
        app.init_resource::<PendingActions>();
        app.init_resource::<crate::app::net::Online>();
        app.add_message::<PlacementDenied>();
        for player in 0..seats {
            app.world_mut().spawn((
                Cursor {
                    player,
                    x: 5,
                    y: 4,
                    repeat: HoldRepeat::default(),
                    flash: 0.0,
                },
                Transform::default(),
            ));
        }
        app
    }

    /// A Tide Pool stage granting one arrow, in setup, with `hands`
    /// cursors on it: co-op when there are two.
    fn stage(hands: u8) -> App {
        use crate::app::{Campaign, LoadLevel, Phase};
        let levels = crate::sim::campaign_levels();
        let index = levels
            .iter()
            .position(|level| level.posts == 1)
            .expect("a one-arrow stage");
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin);
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Puzzle));
        app.init_state::<Phase>();
        app.insert_resource(Sim(levels[index].board()));
        app.insert_resource(Campaign {
            index,
            ..Campaign::tide_pool_of(levels)
        });
        app.insert_resource(GameSettings::default());
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<crate::app::progress::Progress>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_message::<PlacementDenied>();
        app.add_message::<LoadLevel>();
        app.add_systems(Update, setup_input);
        for player in 0..hands {
            app.world_mut()
                .spawn((Cursor::seated(player), Transform::default()));
        }
        app
    }

    /// Put `player`'s cursor on `(x, y)`.
    fn park(app: &mut App, player: u8, (x, y): (u8, u8)) {
        let mut query = app.world_mut().query::<&mut Cursor>();
        for mut cursor in query.iter_mut(app.world_mut()) {
            if cursor.player == player {
                (cursor.x, cursor.y) = (x, y);
            }
        }
    }

    /// Two empty tiles of the stage, for the two hands to stand on.
    fn two_sand_tiles(app: &App) -> [(u8, u8); 2] {
        let board = &app.world().resource::<Sim>().0;
        let sand: Vec<(u8, u8)> = board
            .tiles()
            .filter(|&(x, y, kind)| kind == TileKind::Empty && board.can_place_signpost(0, x, y))
            .map(|(x, y, _)| (x, y))
            .collect();
        [sand[0], sand[sand.len() - 1]]
    }

    /// Co-op is one pool of arrows between two pairs of hands: the second
    /// cursor, on the second seat's keys, places the same seat's arrows,
    /// is refused once the pool is spent, and can pick up or re-point an
    /// arrow its partner put down.
    #[test]
    fn two_hands_share_one_pool_of_arrows() {
        let mut app = stage(2);
        let [a, b] = two_sand_tiles(&app);
        park(&mut app, 0, a);
        park(&mut app, 1, b);
        let count = |app: &App| app.world().resource::<Sim>().0.signpost_count(0);

        tap(&mut app, KeyCode::ArrowUp);
        assert_eq!(count(&app), 1, "P1 spends the one arrow");

        tap(&mut app, KeyCode::Numpad8);
        assert_eq!(count(&app), 1, "and there is none left for P2");
        assert!(
            app.world()
                .resource::<Sim>()
                .0
                .signpost_at(b.0, b.1)
                .is_none(),
            "P2's tile stays bare"
        );

        // P2 walks over and picks up P1's arrow, then puts it down again
        // pointing another way: it is the pair's, not P1's.
        park(&mut app, 1, a);
        tap(&mut app, KeyCode::Numpad0);
        assert_eq!(count(&app), 0, "P2 lifts P1's arrow");
        tap(&mut app, KeyCode::Numpad6);
        let post = app
            .world()
            .resource::<Sim>()
            .0
            .signpost_at(a.0, a.1)
            .expect("placed");
        assert_eq!((post.owner, post.dir), (0, Direction::Right));
    }

    /// Alone, the second seat's keys do nothing to a puzzle, and the
    /// one-hand preset still commits on IJKL; with a partner the preset
    /// stands down, since IJKL is the partner's.
    #[test]
    fn the_one_hand_preset_gives_way_to_a_partner() {
        for (hands, placed) in [(1, true), (2, false)] {
            let mut app = stage(hands);
            app.world_mut().resource_mut::<GameSettings>().commit = CommitScheme::Ijkl;
            let [a, _] = two_sand_tiles(&app);
            park(&mut app, 0, a);
            tap(&mut app, KeyCode::KeyI);
            let board = &app.world().resource::<Sim>().0;
            assert_eq!(
                board.signpost_at(a.0, a.1).is_some(),
                placed,
                "{hands} hand(s): IJKL commits for P1 only when nobody else has it"
            );
        }
    }

    /// Under the mirrored keys P1's arrow keys walk the cursor and W/A/S/D
    /// place, alone and with a co-op partner: the scheme only trades P1's
    /// own keys, so it stays on with the partner, whose IJKL still moves
    /// the partner and nobody else.
    #[test]
    fn the_mirrored_keys_move_on_the_arrows_and_place_on_wasd() {
        for hands in [1, 2] {
            let mut app = stage(hands);
            app.init_resource::<Time>();
            app.init_resource::<crate::app::net::Online>();
            app.add_systems(Update, move_cursor.before(setup_input));
            app.world_mut().resource_mut::<GameSettings>().commit = CommitScheme::Wasd;
            let [a, b] = two_sand_tiles(&app);
            park(&mut app, 0, a);
            park(&mut app, 1, b);
            let width = app.world().resource::<Sim>().0.width();
            let count = |app: &App| app.world().resource::<Sim>().0.signpost_count(0);

            let (dx, key) = if a.0 + 1 < width {
                (1, KeyCode::ArrowRight)
            } else {
                (-1, KeyCode::ArrowLeft)
            };
            tap(&mut app, key);
            let moved = ((i16::from(a.0) + dx) as u8, a.1);
            assert_eq!(cursor_at(&mut app, 0), moved, "{hands}: an arrow key moves");
            assert_eq!(count(&app), 0, "{hands}: and places nothing");

            park(&mut app, 0, a);
            tap(&mut app, KeyCode::KeyW);
            assert_eq!(cursor_at(&mut app, 0), a, "{hands}: W does not move");
            let post = app
                .world()
                .resource::<Sim>()
                .0
                .signpost_at(a.0, a.1)
                .expect("W places");
            assert_eq!(post.dir, Direction::Up, "{hands}");

            if hands == 2 {
                tap(&mut app, KeyCode::KeyI);
                assert_eq!(cursor_at(&mut app, 0), a, "IJKL is not P1's");
                assert_ne!(cursor_at(&mut app, 1), b, "but moves the partner");
            }
        }
    }

    /// In a versus round, with a rival on the second seat's keys, the
    /// mirrored keys stay on: P1 moves on the arrows and queues on WASD,
    /// and P2's IJKL and numpad still answer for P2 alone.
    #[test]
    fn the_mirrored_keys_hold_in_a_versus_round() {
        let mut app = beach(2);
        app.world_mut().resource_mut::<GameSettings>().commit = CommitScheme::Wasd;
        app.add_systems(Update, (move_cursor, versus_input).chain());
        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .set_tile(5, 3, TileKind::Empty);
        let pending = |app: &App, seat: usize| app.world().resource::<PendingActions>().0[seat];

        tap(&mut app, KeyCode::ArrowUp);
        assert_eq!(cursor_at(&mut app, 0), (5, 3), "the arrow key moves P1");
        assert_eq!(cursor_at(&mut app, 1), (5, 4), "and only P1");
        assert_eq!(pending(&app, 0), PlayerAction::None, "and queues nothing");

        tap(&mut app, KeyCode::KeyW);
        assert_eq!(cursor_at(&mut app, 0), (5, 3), "W does not move");
        assert_eq!(
            pending(&app, 0),
            PlayerAction::Place {
                x: 5,
                y: 3,
                dir: Direction::Up
            }
        );

        tap(&mut app, KeyCode::KeyI);
        assert_eq!(cursor_at(&mut app, 1), (5, 3), "P2 still walks on IJKL");
        assert_eq!(cursor_at(&mut app, 0), (5, 3));
    }

    fn tap(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(key);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    }

    fn cursor_at(app: &mut App, player: u8) -> (u8, u8) {
        app.world_mut()
            .query::<&Cursor>()
            .iter(app.world())
            .find(|c| c.player == player)
            .map(|c| (c.x, c.y))
            .expect("seat has a cursor")
    }

    /// `seated_here` is the rule the beach asks before it acts: whose pad
    /// a raid reaches, and under [`post_noticed_here`] whether a post
    /// knocks and is marked. It answers for the seats with a cursor and no others,
    /// which leaves out the bots, the rivals down a wire, and a replay.
    #[test]
    fn only_the_seats_with_a_cursor_are_ours() {
        #[derive(Resource, Default)]
        struct Answers([bool; crate::sim::MAX_PLAYERS]);

        let mut app = beach(2);
        app.init_resource::<Answers>();
        app.add_systems(
            Update,
            |cursors: Query<&Cursor>, mut answers: ResMut<Answers>| {
                for (seat, answer) in answers.0.iter_mut().enumerate() {
                    *answer = seated_here(&cursors, seat as u8);
                }
            },
        );
        app.update();
        let answers = &app.world().resource::<Answers>().0;
        assert!(answers[0] && answers[1], "the two seated players are ours");
        assert!(
            answers[2..].iter().all(|here| !here),
            "and nobody else is: {answers:?}"
        );
    }

    /// A post is marked where its owner is seated, and everywhere on a
    /// machine that holds no seat: a replay or a spectator sees and hears
    /// every post, where `seated_here` alone would answer "nobody" for all
    /// of them. The knock and the ring both ask this, so they agree.
    #[test]
    fn a_watcher_notices_every_post_and_a_player_only_their_own() {
        #[derive(Resource, Default)]
        struct Answers([bool; crate::sim::MAX_PLAYERS]);

        let noticed = |app: &mut App| {
            app.init_resource::<Answers>();
            app.add_systems(
                Update,
                |cursors: Query<&Cursor>, mut answers: ResMut<Answers>| {
                    for (seat, answer) in answers.0.iter_mut().enumerate() {
                        *answer = post_noticed_here(&cursors, seat as u8);
                    }
                },
            );
            app.update();
            app.world().resource::<Answers>().0
        };
        let seated = noticed(&mut beach(2));
        assert!(seated[0] && seated[1], "the seats at this machine");
        assert!(!seated[2..].iter().any(|&n| n), "and no others: {seated:?}");
        let mut watching = beach(2);
        let cursors: Vec<Entity> = watching
            .world_mut()
            .query_filtered::<Entity, With<Cursor>>()
            .iter(watching.world())
            .collect();
        for cursor in cursors {
            watching.world_mut().entity_mut(cursor).despawn();
        }
        let watched = noticed(&mut watching);
        assert!(watched.iter().all(|&n| n), "a watcher sees every seat");
    }

    /// The snap [`glide_cursors`] keeps for long jumps, and the glide it
    /// keeps for everything else.
    #[test]
    fn a_long_jump_snaps_and_a_short_one_glides() {
        let mut app = beach(2);
        app.add_systems(Update, glide_cursors);
        // The board never changes under this test, so every tile centre it
        // needs can be read once, before the borrow checker gets involved.
        let (home, next_door, corner) = {
            let board = &app.world().resource::<Sim>().0;
            (
                layout::tile_center(board, 5, 4),
                layout::tile_center(board, 6, 4),
                layout::tile_center(board, 0, 0),
            )
        };
        // A frame has to pass for the glide to have a delta to work with.
        app.update();

        let mut place = |x: u8, y: u8| {
            let mut seats = app.world_mut().query::<&mut Cursor>();
            let world = app.world_mut();
            for mut cursor in seats.iter_mut(world) {
                if cursor.player == 0 {
                    cursor.x = x;
                    cursor.y = y;
                }
            }
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_millis(16));
            app.update();
            let mut at = app.world_mut().query::<(&Cursor, &Transform)>();
            let world = app.world();
            at.iter(world)
                .find(|(cursor, _)| cursor.player == 0)
                .map(|(_, t)| t.translation.truncate())
                .expect("seat one has a cursor")
        };

        // One tile over: on its way, not there yet.
        let stepped = place(6, 4);
        assert_ne!(stepped, home, "it did not move at all");
        assert!(
            stepped.distance(next_door) > 1.0,
            "one tile should glide, not snap: {stepped:?}"
        );
        // Right across the board: there in one frame.
        let jumped = place(0, 0);
        assert!(
            jumped.distance(corner) < 0.01,
            "a long jump has to snap: {jumped:?} vs {corner:?}"
        );
    }

    /// The keyboard drives two seats and only two: seats three and up wait
    /// for a pad rather than mirroring seat two's hands.
    #[test]
    fn the_keyboard_seats_two_and_stops() {
        let mut app = beach(4);
        app.add_systems(Update, move_cursor);

        // Seat 1 on W, seat 2 on I: the two default layouts.
        tap(&mut app, KeyCode::KeyW);
        assert_eq!(cursor_at(&mut app, 0), (5, 3), "seat 1 moved up");
        assert_eq!(cursor_at(&mut app, 1), (5, 4), "and only seat 1");

        tap(&mut app, KeyCode::KeyI);
        assert_eq!(cursor_at(&mut app, 1), (5, 3), "seat 2 moved up");
        assert_eq!(
            (cursor_at(&mut app, 2), cursor_at(&mut app, 3)),
            ((5, 4), (5, 4)),
            "seats 3 and 4 are pad seats and ignored the keyboard"
        );
    }

    /// Online there is one cursor, it may hold any seat, and it answers to
    /// the primary layout: a P4 online still plays on WASD.
    #[test]
    fn an_online_seat_plays_on_the_primary_keys() {
        let mut app = beach(0);
        app.world_mut().spawn((
            Cursor {
                player: 3,
                x: 5,
                y: 4,
                repeat: HoldRepeat::default(),
                flash: 0.0,
            },
            Transform::default(),
        ));
        app.add_systems(Update, move_cursor);

        // Offline, seat 4 is a pad seat and the keyboard does nothing.
        tap(&mut app, KeyCode::KeyA);
        assert_eq!(cursor_at(&mut app, 3), (5, 4));

        // A session that talks to nobody: `move_cursor` only asks whether
        // one exists, and an ephemeral socket keeps the test off the wire.
        let transport = crate::transport::UdpTransport::host(0).expect("ephemeral port");
        let lockstep = crate::sim::Lockstep::new(3, vec![0, 3], crate::sim::DEFAULT_DELAY);
        app.insert_resource(crate::app::net::Online(Some(
            crate::app::net::OnlineSession::new(
                transport,
                lockstep,
                4,
                crate::transport::MatchTerms::default(),
            ),
        )));
        tap(&mut app, KeyCode::KeyA);
        assert_eq!(cursor_at(&mut app, 3), (4, 4), "the same key now moves it");
    }

    /// A commit key queues a placement for the seat that pressed it, and a
    /// refused tile queues nothing, flashes the cursor, and says so.
    #[test]
    fn a_refused_placement_flashes_instead_of_queueing() {
        let mut app = beach(2);
        app.add_systems(Update, versus_input);
        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .set_tile(5, 4, TileKind::Empty);

        // Empty sand: the arrow keys are seat 1's commit keys.
        tap(&mut app, KeyCode::ArrowUp);
        assert_eq!(
            app.world().resource::<PendingActions>().0[0],
            PlayerAction::Place {
                x: 5,
                y: 4,
                dir: Direction::Up
            }
        );
        assert_eq!(
            app.world().resource::<PendingActions>().0[1],
            PlayerAction::None,
            "seat 2 pressed nothing"
        );

        // A rock refuses, and the refusal is reported rather than silently
        // dropped.
        app.world_mut().resource_mut::<PendingActions>().0[0] = PlayerAction::None;
        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .set_tile(5, 4, TileKind::Rock);
        tap(&mut app, KeyCode::ArrowUp);
        assert_eq!(
            app.world().resource::<PendingActions>().0[0],
            PlayerAction::None,
            "nothing was queued onto a rock"
        );
        let flash = app
            .world_mut()
            .query::<&Cursor>()
            .iter(app.world())
            .find(|c| c.player == 0)
            .map(|c| c.flash)
            .unwrap();
        assert!(flash > 0.0, "the cursor flashed red");
        assert_eq!(
            app.world()
                .resource::<Messages<PlacementDenied>>()
                .iter_current_update_messages()
                .count(),
            1
        );
    }

    /// Under the default scheme the two keyboard seats share no keys; the
    /// single-hand preset deliberately reuses IJKL and is guarded to solo
    /// modes elsewhere.
    #[test]
    fn default_keyboard_seats_do_not_collide() {
        let collect = |player: u8| -> Vec<KeyCode> {
            let map = keymap(&GameSettings::default(), player, CommitScheme::Arrows);
            let mut keys: Vec<KeyCode> = map.moves.iter().map(|(k, ..)| *k).collect();
            keys.extend(map.places.iter().map(|(k, _)| *k));
            keys.push(map.remove);
            keys.push(map.clear_all);
            keys
        };
        let p1 = collect(0);
        for key in collect(1) {
            assert!(!p1.contains(&key), "{key:?} is bound for both seats");
        }
    }

    /// A finished couch round with the results card up: a table set up
    /// at this machine, with nobody online.
    fn couch_results_card() -> App {
        use crate::app::tournament::Tournament;
        let mut app = beach(2);
        app.init_resource::<Tournament>();
        app.init_resource::<crate::app::lobby::Homecoming>();
        app.init_resource::<crate::app::pause::PauseMenu>();
        app.init_resource::<crate::app::keycaps::KeyCaps>();
        app.init_resource::<LiveSeatKeys>();
        app.init_resource::<crate::app::match_setup::MatchConfig>();
        app.init_state::<crate::app::VersusPhase>();
        let mut table = crate::app::rematch::LocalTable::default();
        table.note_round(true, &Default::default(), &Default::default());
        app.insert_resource(table);
        app.init_resource::<crate::app::next_map::NextMap>();
        app.init_resource::<crate::app::match_setup::MatchMenu>();
        app.add_systems(Update, versus_over_input);
        app
    }

    /// R on a couch round's card plays the same table again, through the
    /// arena's own teardown and build; the same R at an online table,
    /// whose next round is the lobby's to call, does nothing.
    #[test]
    fn r_on_a_couch_card_plays_the_table_again() {
        let mut app = couch_results_card();
        tap(&mut app, KeyCode::KeyR);
        assert!(matches!(
            *app.world().resource::<NextState<Screen>>(),
            NextState::Pending(Screen::Versus)
        ));
        assert!(
            app.world()
                .resource::<crate::app::match_setup::MatchConfig>()
                .armed
        );

        let mut app = results_card(0, true, false);
        tap(&mut app, KeyCode::KeyR);
        assert!(matches!(
            *app.world().resource::<NextState<Screen>>(),
            NextState::Unchanged
        ));
        assert!(
            !app.world()
                .resource::<crate::app::match_setup::MatchConfig>()
                .armed
        );
    }

    /// A finished online match with the results card up, over a real
    /// socket: seat 0 hosts (with the beacon it carried out of the lobby),
    /// anything else joins.
    fn results_card(seat: u8, from_lobby: bool, series_on: bool) -> App {
        use crate::app::net::{Online, OnlineSession};
        use crate::app::tournament::Tournament;
        use crate::sim::{DEFAULT_DELAY, Lockstep};
        use crate::transport::{Announcer, MatchTerms, UdpTransport};

        let mut app = couch_results_card();
        let host = seat == 0;
        let transport = match host {
            true => UdpTransport::host(0).expect("game socket"),
            false => UdpTransport::join(("127.0.0.1", 47999)).expect("join"),
        };
        let mut session = OnlineSession::new(
            transport,
            Lockstep::new(seat, vec![0, 1], DEFAULT_DELAY),
            2,
            MatchTerms::default(),
        );
        session.home.from_lobby = from_lobby;
        // A host formed in the lobby always carries the beacon out of it:
        // the two are set together at the launch.
        if host && from_lobby {
            session.stay_on_air(Announcer::new(0xB0A7).expect("announcer"));
        }
        if series_on {
            *app.world_mut().resource_mut::<Tournament>() =
                Tournament::start(crate::app::tournament::SeriesLength::BestOfFive);
        }
        app.world_mut().resource_mut::<Online>().0 = Some(session);
        app
    }

    /// Where a tap on the results card actually lands, transition applied.
    fn pressing_enter(app: &mut App) -> Screen {
        tap(app, KeyCode::Enter);
        // One more, to let the state transition apply.
        app.update();
        *app.world().resource::<State<Screen>>().get()
    }

    /// The end of a lobby match is a door back to the lobby, not out to the
    /// menu. The table came in together and goes back together, still
    /// connected, so the next game is a keypress rather than a
    /// rediscovery.
    #[test]
    fn the_end_of_a_lobby_match_walks_the_table_back_to_the_lobby() {
        use crate::app::lobby::Homecoming;
        use crate::app::net::{LobbyReturn, Online};

        for seat in [0, 1] {
            let mut app = results_card(seat, true, false);
            assert_eq!(pressing_enter(&mut app), Screen::Lobby, "seat {seat}");
            let homecoming = app.world().resource::<Homecoming>();
            let returned = homecoming.0.as_ref().expect("the table came home");
            assert_eq!(matches!(returned, LobbyReturn::Host { .. }), seat == 0);
            assert!(
                app.world().resource::<Online>().0.is_none(),
                "the session was handed over whole, not left behind"
            );
            if seat == 0 {
                assert!(
                    matches!(
                        returned,
                        LobbyReturn::Host {
                            announcer: Some(_),
                            ..
                        }
                    ),
                    "and the beacon came with it, to announce the beach open again"
                );
            }
        }
    }

    /// The direct `PINCH_HOST`/`PINCH_JOIN` pair never went through a
    /// lobby, so there is none to go back to: its Enter still means the
    /// menu, and nothing is left waiting in `Homecoming` for a lobby that
    /// is never opened.
    #[test]
    fn a_direct_pair_has_no_lobby_to_go_back_to() {
        use crate::app::lobby::Homecoming;

        let mut app = results_card(0, false, false);
        assert_eq!(pressing_enter(&mut app), Screen::Menu);
        assert!(app.world().resource::<Homecoming>().0.is_none());
    }

    /// Mid-series the card is a doorway between rounds, and Enter keeps
    /// its old meanings: the host calls the next round, and a joiner's
    /// Enter is still its way out, since the series plays on without it.
    /// Only once the series is over does the table walk home together.
    #[test]
    fn a_running_series_still_owns_enter() {
        use crate::app::lobby::Homecoming;

        let mut app = results_card(1, true, true);
        assert_eq!(
            pressing_enter(&mut app),
            Screen::Menu,
            "leaving a series is leaving"
        );
        assert!(app.world().resource::<Homecoming>().0.is_none());

        // The host's Enter opens its map page for the next round, and the
        // card it picks there calls the round.
        let mut app = results_card(0, true, true);
        assert_eq!(pressing_enter(&mut app), Screen::Versus, "the page first");
        assert!(crate::app::next_map::page_up(
            app.world().resource::<crate::app::match_setup::MatchMenu>()
        ));
        assert_eq!(
            pressing_enter(&mut app),
            Screen::Interlude,
            "the host calls the next round on the card picked"
        );
        assert!(app.world().resource::<Homecoming>().0.is_none());
    }

    /// Esc on the host's page between rounds puts the page away and calls
    /// nothing: the results card as it was, the series still running, and
    /// Enter opening the page again.
    #[test]
    fn esc_on_the_next_map_page_calls_nothing() {
        use crate::app::net::Online;
        let mut app = results_card(0, true, true);
        let dealt = |app: &App| {
            app.world()
                .resource::<Online>()
                .0
                .as_ref()
                .map(|session| session.rounds_dealt)
        };
        let before = dealt(&app);
        assert_eq!(pressing_enter(&mut app), Screen::Versus);
        tap(&mut app, KeyCode::KeyD);
        tap(&mut app, KeyCode::Escape);
        app.update();
        let menu = app.world().resource::<crate::app::match_setup::MatchMenu>();
        assert!(!crate::app::next_map::page_up(menu), "the page is down");
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Versus
        );
        assert_eq!(dealt(&app), before, "no round called");
        assert!(
            app.world()
                .resource::<crate::app::tournament::Tournament>()
                .is_running(),
            "the series plays on"
        );
        assert!(
            app.world()
                .resource::<Online>()
                .0
                .as_ref()
                .is_some_and(|session| !session.picking_map),
            "the table is told the host stopped picking"
        );
        assert_eq!(pressing_enter(&mut app), Screen::Versus);
        assert!(crate::app::next_map::page_up(
            app.world().resource::<crate::app::match_setup::MatchMenu>()
        ));
    }

    /// Esc on the host's page between rounds puts the page away and opens
    /// nothing behind it: the pause card's own systems run after the
    /// card's keys, on the condition the game runs them on. The page went
    /// down inside the card's keys, the pause card's condition (no page
    /// up) was true by the time it was read, and it opened on that Esc.
    #[test]
    fn esc_on_the_next_map_page_opens_no_pause_card() {
        let mut app = results_card(0, true, true);
        app.insert_state(Screen::Versus);
        app.insert_state(crate::app::Phase::default());
        app.init_resource::<crate::app::gamepad::PadFace>();
        app.init_resource::<crate::app::Paused>();
        app.add_message::<AppExit>();
        app.add_systems(
            Update,
            crate::app::schedule::pause_card_systems().after(versus_over_input),
        );
        assert_eq!(pressing_enter(&mut app), Screen::Versus);
        assert!(crate::app::next_map::page_up(
            app.world().resource::<crate::app::match_setup::MatchMenu>()
        ));
        tap(&mut app, KeyCode::Escape);
        app.update();
        let menu = app.world().resource::<crate::app::match_setup::MatchMenu>();
        assert!(!crate::app::next_map::page_up(menu), "the page is down");
        assert!(
            !app.world()
                .resource::<crate::app::pause::PauseMenu>()
                .is_open(),
            "and no pause card came up on the same Esc"
        );
        // A second Esc, on the results card, is the pause card's.
        tap(&mut app, KeyCode::Escape);
        assert!(
            app.world()
                .resource::<crate::app::pause::PauseMenu>()
                .is_open()
        );
    }

    /// Walking the host's page between rounds walks its cards and not the
    /// board cursor behind it: the cursor runs on the condition the game
    /// runs it on, after the card's keys. It answered the page's WASD (and
    /// a pad's d-pad, which the page's face sends as WASD) as well.
    #[test]
    fn walking_the_next_map_page_leaves_the_board_cursor_alone() {
        let mut app = results_card(0, true, true);
        app.insert_state(Screen::Versus);
        app.init_resource::<crate::app::editor::EditorState>();
        app.add_systems(
            Update,
            move_cursor
                .run_if(crate::app::conditions::board_cursor_free)
                .after(versus_over_input),
        );
        let home = cursor_at(&mut app, 0);
        assert_eq!(pressing_enter(&mut app), Screen::Versus);
        let slot = app
            .world()
            .resource::<crate::app::match_setup::MatchMenu>()
            .slot;
        for key in [KeyCode::KeyD, KeyCode::KeyS, KeyCode::ArrowRight] {
            tap(&mut app, key);
            app.update();
        }
        assert_ne!(
            app.world()
                .resource::<crate::app::match_setup::MatchMenu>()
                .slot,
            slot,
            "the page's cursor walked"
        );
        assert_eq!(cursor_at(&mut app, 0), home, "the board's did not");
        // Put away, the board has its keys back.
        tap(&mut app, KeyCode::Escape);
        app.update();
        tap(&mut app, KeyCode::KeyD);
        assert_ne!(cursor_at(&mut app, 0), home);
    }

    /// With the pause card up over the results card, Enter is the card's.
    /// It used to reach both: the host's "Back to menu" called the next
    /// round for the whole table first, then left it.
    #[test]
    fn the_pause_card_keeps_its_enter() {
        let mut app = results_card(0, true, true);
        app.insert_resource(crate::app::pause::PauseMenu::raised());
        assert_eq!(pressing_enter(&mut app), Screen::Versus);
    }

    /// The Enter that ends the match must not also start the next one.
    ///
    /// A host lands back in the lobby with its peers still aboard, and
    /// there Enter is the launch key: one press that both left the results
    /// card and reached `host_tick` would deal a fresh round before anybody
    /// had read the scores. What prevents it is the order the engine runs
    /// in, so that is what this checks, with the real input plugin: a press
    /// is cleared in `PreUpdate` and the state transition lands after it,
    /// so the screen a keypress arrives on is the only one that sees it.
    #[test]
    fn one_press_is_only_ever_read_by_one_screen() {
        use bevy::input::ButtonState;
        use bevy::input::keyboard::{Key, KeyboardInput};

        #[derive(Resource, Default)]
        struct SeenInTheLobby(bool);

        let mut app = App::new();
        app.add_plugins((bevy::state::app::StatesPlugin, bevy::input::InputPlugin));
        app.init_state::<Screen>();
        app.insert_resource(State::new(Screen::Versus));
        app.init_resource::<SeenInTheLobby>();
        app.add_systems(
            Update,
            (
                // Standing in for the results card: Enter leaves for the
                // lobby, exactly as `versus_over_input` does.
                (|keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<Screen>>| {
                    if keys.just_pressed(KeyCode::Enter) {
                        next.set(Screen::Lobby);
                    }
                })
                .run_if(in_state(Screen::Versus)),
                // Standing in for `host_tick`, where Enter launches.
                (|keys: Res<ButtonInput<KeyCode>>, mut seen: ResMut<SeenInTheLobby>| {
                    seen.0 |= keys.just_pressed(KeyCode::Enter);
                })
                .run_if(in_state(Screen::Lobby)),
            ),
        );

        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        // The key is never released: a player's finger is still on it as
        // the lobby comes up, which is the whole worry.
        app.update();
        app.update();
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Lobby
        );
        assert!(
            !app.world().resource::<SeenInTheLobby>().0,
            "the lobby read the Enter that was meant for the results card"
        );
    }
}
