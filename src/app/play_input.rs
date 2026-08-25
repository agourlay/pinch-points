//! What the keys mean while a round is on: the phase-flow input systems.
//!
//! The cursor file answers "where is the cursor and how does it move";
//! this one answers "what does a press do to the round": place a post,
//! start the run, retry, advance the campaign, queue a versus action,
//! leave the results card. None of it moves a cursor, and two of the
//! systems never read one.

use crate::app::cursor::{Cursor, FLASH_SECS, keymap};
use crate::app::settings::{CommitScheme, GameSettings};
use crate::app::{
    Campaign, LoadLevel, Paused, PendingActions, Phase, PlacementDenied, Screen, Sim,
};
use crate::sim::PlayerAction;
use bevy::prelude::*;

/// Puzzle setup phase: spend the inventory directly on the board, Enter to
/// run, N/P to jump between levels.
#[allow(clippy::too_many_arguments)]
pub fn setup_input(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<GameSettings>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    progress: Res<crate::app::progress::Progress>,
    mut sim: ResMut<Sim>,
    mut campaign: ResMut<Campaign>,
    mut next_phase: ResMut<NextState<Phase>>,
    mut load: MessageWriter<LoadLevel>,
    mut denied: MessageWriter<PlacementDenied>,
    mut cursors: Query<&mut Cursor>,
) {
    // Esc is the pause card's key; it handles leaving.
    let Some(mut cursor) = cursors.iter_mut().find(|c| c.player == 0) else {
        return;
    };
    let map = keymap(&settings, 0, settings.commit);
    for (key, dir) in map.places {
        if keys.just_pressed(key) {
            // CapPolicy::Reject enforces the inventory; surface the no-op,
            // and say which no it was.
            let spent = sim.0.out_of_signposts(0, cursor.x, cursor.y);
            if !sim.0.place_signpost(0, cursor.x, cursor.y, dir) {
                cursor.flash = FLASH_SECS;
                denied.write(PlacementDenied {
                    player: 0,
                    out_of_signposts: spent,
                });
            }
        }
    }
    if keys.just_pressed(map.remove) {
        let _ = sim.0.remove_signpost(0, cursor.x, cursor.y);
    }
    if keys.just_pressed(KeyCode::Enter) {
        next_phase.set(Phase::Running);
    }
    if caps.just_pressed(&keys, 'N') {
        // Browsing forward stops at the ladder: the stage list is the only
        // way past a stage you have not cleared.
        let next = (campaign.index + 1) % campaign.levels.len();
        if progress.unlocked(&campaign, next) {
            campaign.index = next;
            load.write(LoadLevel { keep_posts: false });
        } else {
            cursor.flash = FLASH_SECS;
            denied.write(PlacementDenied {
                player: 0,
                out_of_signposts: false,
            });
        }
    }
    if caps.just_pressed(&keys, 'P') {
        campaign.index = (campaign.index + campaign.levels.len() - 1) % campaign.levels.len();
        load.write(LoadLevel { keep_posts: false });
    }
}

/// Puzzle running phase: R resets to setup (keeping placed posts), Esc pauses.
pub fn running_input(
    keys: Res<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    mut paused: ResMut<Paused>,
    mut load: MessageWriter<LoadLevel>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        paused.0 = !paused.0;
    }
    if caps.just_pressed(&keys, 'R') {
        load.write(LoadLevel { keep_posts: true });
    }
}

/// Puzzle won/lost phase: Enter advances (or retries after a loss), R replays.
pub fn done_input(
    keys: Res<ButtonInput<KeyCode>>,
    caps: Res<crate::app::keycaps::KeyCaps>,
    phase: Res<State<Phase>>,
    mut campaign: ResMut<Campaign>,
    mut next_screen: ResMut<NextState<Screen>>,
    mut load: MessageWriter<LoadLevel>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        // Back to the stage list, which is the door every puzzle is entered
        // through; Esc again from there reaches the menu.
        next_screen.set(Screen::StageSelect);
        return;
    }
    if keys.just_pressed(KeyCode::Enter) {
        if *phase.get() != Phase::Won {
            load.write(LoadLevel { keep_posts: true });
            return;
        }
        // The end of the list is the end of the list. Wrapping to level one
        // read as the game not having noticed: the card says "that was the
        // last level" and then puts you back on the first.
        if campaign.index + 1 == campaign.levels.len() {
            next_screen.set(Screen::Menu);
            return;
        }
        campaign.index += 1;
        load.write(LoadLevel { keep_posts: false });
    }
    if caps.just_pressed(&keys, 'R') {
        load.write(LoadLevel { keep_posts: true });
    }
}

/// Versus: every player's commits go through `PendingActions` and take effect
/// on the next 30 Hz tick, the input path rollback netcode will feed.
/// One action per player per tick (spec §7.6); a later press in the same
/// window overwrites the earlier one.
pub fn versus_input(
    keys: Res<ButtonInput<KeyCode>>,
    sim: Res<Sim>,
    settings: Res<GameSettings>,
    online: Res<crate::app::net::Online>,
    mut pending: ResMut<PendingActions>,
    mut denied: MessageWriter<PlacementDenied>,
    mut cursors: Query<&mut Cursor>,
) {
    // Esc is the pause card's key; it handles leaving (including a dead
    // online session or truncated replay).
    let board = &sim.0;
    for mut cursor in &mut cursors {
        let p = cursor.player as usize;
        // Online: whatever your seat, your hands are on the primary layout.
        // Local: the keyboard seats two; pads drive seats 3 and up.
        let map = if online.0.is_some() {
            keymap(&settings, 0, CommitScheme::Arrows)
        } else if cursor.player < 2 {
            keymap(&settings, cursor.player, CommitScheme::Arrows)
        } else {
            continue;
        };
        // Online or not, a cursor carries the seat it places for: online
        // spawns one cursor and gives it the local seat.
        let seat = cursor.player;
        for (key, dir) in map.places {
            if keys.just_pressed(key) {
                if !board.can_place_signpost(seat, cursor.x, cursor.y) {
                    cursor.flash = FLASH_SECS;
                    denied.write(PlacementDenied {
                        player: seat,
                        out_of_signposts: board.out_of_signposts(seat, cursor.x, cursor.y),
                    });
                    continue;
                }
                pending.0[p] = PlayerAction::Place {
                    x: cursor.x,
                    y: cursor.y,
                    dir,
                };
            }
        }
        if keys.just_pressed(map.remove) {
            pending.0[p] = PlayerAction::Remove {
                x: cursor.x,
                y: cursor.y,
            };
        }
        // Clear-all: queue one removal per tick until none remain, staying
        // inside the one-action-per-tick input model.
        if keys.pressed(map.clear_all)
            && let Some((x, y)) = board.first_signpost_of(cursor.player)
        {
            pending.0[p] = PlayerAction::Remove { x, y };
        }
    }
}

/// Versus results: Enter continues a running series, otherwise back to
/// the menu - or, for a match formed in the beach lobby, back to that
/// lobby with the whole table still connected.
///
/// Online, the host's Enter calls the next round for the whole table,
/// admitting anyone who queued while this one played, and every peer is
/// walked back into the arena when the invitation lands. Mid-series a
/// joiner's Enter is its own way out, since the series plays on without
/// it; once the match is over there is nothing left to leave, and Enter
/// takes everyone back to the lobby they came from, ready for another.
pub fn versus_over_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut tournament: ResMut<crate::app::tournament::Tournament>,
    mut online: ResMut<crate::app::net::Online>,
    mut homecoming: ResMut<crate::app::lobby::Homecoming>,
    mut next_screen: ResMut<NextState<Screen>>,
) {
    let series_on = tournament.is_running();
    let Some(session) = &mut online.0 else {
        if crate::app::menu_ui::enter(&keys) {
            match series_on {
                true => next_screen.set(Screen::Interlude),
                false => next_screen.set(Screen::Menu),
            }
        }
        return;
    };
    // The invitation may already have arrived, in which case nobody
    // pressed anything here: the host decided and this peer follows.
    if session.next_round {
        // Left set on purpose: `end_versus` reads it as "keep me", and
        // the interlude clears it once the session is safely through.
        next_screen.set(Screen::Interlude);
        return;
    }
    if !crate::app::menu_ui::enter(&keys) {
        return;
    }
    if session.is_host() && series_on {
        // The host re-deals the seats for the next round and, with
        // them, the series tally: the returned standing is the same
        // wins moved onto the chairs their holders now sit in, which
        // this machine adopts so its own card agrees with the table.
        let standing = session.call_next_round(
            crate::app::match_setup::next_round_terms(
                session.terms,
                session.seats,
                crate::app::clock::fresh_seed(),
            ),
            tournament.standing(),
        );
        if let Some(crate::transport::SeriesStanding { round, wins }) = standing {
            tournament.round = round;
            tournament.wins = wins;
        }
        next_screen.set(Screen::Interlude);
        return;
    }
    // Mid-series a joiner's Enter still means leaving, and a direct
    // `PINCH_HOST` pair has no lobby to go back to.
    if series_on || !session.home.from_lobby {
        next_screen.set(Screen::Menu);
        return;
    }
    // The match is over and it was formed in the lobby: the whole table
    // goes back there together, sockets and all.
    let session = online.0.take().expect("matched Some above");
    homecoming.0 = Some(session.back_to_the_lobby());
    next_screen.set(Screen::Lobby);
}
